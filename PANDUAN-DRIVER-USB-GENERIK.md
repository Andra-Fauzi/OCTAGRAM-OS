# Panduan Generik: Driver USB untuk Class Apapun (HID, Mass Storage, dll)

Panduan sebelumnya (`panduan-usb-hid-driver.md`) spesifik untuk HID. Panduan
ini menarik mundur satu level: **bagian mana dari driver USB yang generik
untuk SEMUA device class**, dan bagian mana yang beda-beda tergantung
class (HID, Mass Storage/flashdisk, Audio, CDC/serial-over-USB, dst).

Intinya: transport layer (xHCI-mu) **tidak peduli** device-nya keyboard
atau flashdisk — yang beda cuma tipe endpoint yang dipakai dan protokol di
atasnya.

---

## 1. Yang Generik vs Yang Spesifik-Class

```
┌─────────────────────────────────────────┐
│  Device-class driver (BEDA per class)    │  ← HID parser, SCSI command, dst
├─────────────────────────────────────────┤
│  Endpoint I/O (SAMA untuk semua class)   │  ← submit TRB, tunggu completion
├─────────────────────────────────────────┤
│  Descriptor parsing (SAMA untuk semua)   │  ← cari interface & endpoint yang cocok
├─────────────────────────────────────────┤
│  Device enumeration (SAMA, sudah ada)    │  ← enable_slot, address_device, dst
├─────────────────────────────────────────┤
│  Transport xHCI (SAMA, sudah ada)        │  ← ring, doorbell, event
└─────────────────────────────────────────┘
```

3 layer paling bawah **sudah selesai** di project ini (`xhci.rs`). Yang
perlu ditambah untuk device class baru cuma 2 layer paling atas.

---

## 2. Empat Tipe Endpoint USB (Kenali Ini Dulu)

| Tipe | Karakteristik | Dipakai untuk |
|---|---|---|
| **Control** | Bidirectional, selalu ada (EP0), request/response terstruktur | Setup device, GET/SET_DESCRIPTOR, semua request standar |
| **Interrupt** | Polling berkala, latency rendah, data kecil | HID (keyboard, mouse) |
| **Bulk** | Throughput tinggi, tanpa jadwal tetap, retry otomatis di hardware | **Mass storage (flashdisk)**, printer |
| **Isochronous** | Bandwidth terjadwal tapi TIDAK ada retry (real-time, boleh drop) | Audio, webcam, streaming |

`EndpointInfo.attributes & 0x3` menentukan tipe ini:

```rust
match attributes & 0x3 {
    0x0 => "Control",
    0x1 => "Isochronous",
    0x2 => "Bulk",
    0x3 => "Interrupt",
    _ => unreachable!(),
}
```

Kode `configure_endpoint` yang sudah ada di `xhci.rs` **sudah generik** —
dia baca `ep_info.attributes` apa adanya, tinggal dipanggil dengan endpoint
Bulk hasil parsing, bukan cuma Interrupt.

---

## 3. Deteksi Device Class (Setelah Get Descriptor)

Setelah `get_device_descriptor()`, cek `device_class`:

| device_class | Class | Contoh |
|---|---|---|
| `0x00` | Didefinisikan di **Interface**, bukan Device | Paling umum — harus cek interface descriptor |
| `0x03` | HID | Keyboard, mouse, gamepad |
| `0x08` | Mass Storage | Flashdisk, hardisk USB |
| `0x02`/`0x0A` | CDC (Communication) | USB-to-serial, modem |
| `0x01` | Audio | Speaker/mic USB |
| `0x09` | Hub | USB hub (device khusus, punya port sendiri) |
| `0xE0` | Wireless Controller | Bluetooth dongle |

Kalau `device_class == 0x00` (paling sering), class sebenarnya ada di
**Interface Descriptor**, bukan Device Descriptor — makanya
`get_configuration_descriptor_and_find_interrupt_in()` di kodemu sudah baca
`interface_class` saat parsing (`buf_slice[i+5]`). Untuk driver generik,
generalisasi fungsi ini supaya bisa cari endpoint tipe apapun, bukan cuma
Interrupt IN, dan kembalikan `interface_class` juga supaya bisa
routing ke driver yang tepat.

### Struktur routing generik
```rust
pub struct DeviceInfo {
    pub interface_class: u8,
    pub interface_subclass: u8,
    pub interface_protocol: u8,
    pub endpoints: Vec<EndpointInfo>, // semua endpoint di interface itu, bukan cuma 1
}

pub unsafe fn probe_and_init_driver(slot_id: u32, info: DeviceInfo) {
    match info.interface_class {
        0x03 => hid::init(slot_id, &info),
        0x08 => mass_storage::init(slot_id, &info),
        _ => println!("Class {:#04x} belum ada driver", info.interface_class),
    }
}
```

---

## 4. Generalisasi Parser Configuration Descriptor

Fungsi yang sudah ada (`get_configuration_descriptor_and_find_interrupt_in`)
cuma cari **satu** endpoint Interrupt IN. Untuk driver generik, ubah supaya
mengumpulkan **semua** endpoint di interface, apapun tipenya:

```rust
pub unsafe fn get_configuration_descriptor(slot_id: u32) -> Result<Vec<InterfaceInfo>, &'static str> {
    // ... sama seperti sebelumnya sampai dapat buf_slice ...

    let mut interfaces = Vec::new();
    let mut current: Option<InterfaceInfo> = None;

    while i + 2 <= buf_slice.len() {
        let b_length = buf_slice[i] as usize;
        let b_type = buf_slice[i + 1];
        if b_length == 0 || i + b_length > buf_slice.len() { break; }

        match b_type {
            USB_DESC_TYPE_INTERFACE => {
                if let Some(iface) = current.take() { interfaces.push(iface); }
                current = Some(InterfaceInfo {
                    number: buf_slice[i + 2],
                    class: buf_slice[i + 5],
                    subclass: buf_slice[i + 6],
                    protocol: buf_slice[i + 7],
                    endpoints: alloc::vec::Vec::new(),
                });
            }
            USB_DESC_TYPE_ENDPOINT => {
                if let Some(iface) = current.as_mut() {
                    iface.endpoints.push(EndpointInfo {
                        address: buf_slice[i + 2],
                        attributes: buf_slice[i + 3],
                        max_packet_size: (buf_slice[i+4] as u16) | ((buf_slice[i+5] as u16) << 8),
                        interval: buf_slice[i + 6],
                        inteface_number: iface.number,
                    });
                }
            }
            _ => {}
        }
        i += b_length;
    }
    if let Some(iface) = current.take() { interfaces.push(iface); }
    Ok(interfaces)
}
```

Ini pure generalisasi — logika parsingnya (loop `bLength`) **persis sama**
dengan yang sudah kamu tulis, cuma tidak berhenti di endpoint pertama yang
cocok.

---

## 5. Contoh Konkret: Driver Mass Storage (Flashdisk)

### 5.1 Deteksi
`interface_class == 0x08`. Subclass umum: `0x06` (SCSI Transparent Command
Set — hampir semua flashdisk pakai ini). Protocol: `0x50` (Bulk-Only
Transport, disingkat **BOT**).

### 5.2 Endpoint yang dibutuhkan
Cari 2 endpoint **Bulk** (`attributes & 0x3 == 0x2`) di interface itu — satu
arah IN (`address & 0x80 != 0`), satu arah OUT.

```rust
let bulk_in = interface.endpoints.iter().find(|e| e.attributes & 0x3 == 0x2 && e.address & 0x80 != 0);
let bulk_out = interface.endpoints.iter().find(|e| e.attributes & 0x3 == 0x2 && e.address & 0x80 == 0);
```

Keduanya di-`configure_endpoint()` dengan cara **sama persis** seperti
endpoint interrupt di HID — cuma beda tipe endpoint yang ditemukan.

### 5.3 Protokol Bulk-Only Transport (BOT)
Berbeda dari HID yang cuma "baca report", BOT punya siklus command:

```
1. Kirim CBW (Command Block Wrapper, 31 byte) lewat Bulk OUT
   → berisi signature "USBC", tag, panjang data yang diharapkan,
     arah transfer, dan SCSI command (misal READ(10)/WRITE(10))
2. Data stage:
   - kalau READ: baca data lewat Bulk IN
   - kalau WRITE: kirim data lewat Bulk OUT
3. Terima CSW (Command Status Wrapper, 13 byte) lewat Bulk IN
   → berisi signature "USBS", tag (harus cocok CBW), status (0=sukses)
```

```rust
#[repr(C, packed)]
struct CommandBlockWrapper {
    signature: u32,      // 0x43425355 ("USBC" little-endian)
    tag: u32,             // nomor unik, dicocokkan dengan CSW
    data_transfer_length: u32,
    flags: u8,             // bit7: 1=IN(read), 0=OUT(write)
    lun: u8,               // logical unit number, biasanya 0
    cb_length: u8,          // panjang SCSI command
    cb: [u8; 16],           // SCSI command block
}

#[repr(C, packed)]
struct CommandStatusWrapper {
    signature: u32, // 0x53425355 ("USBS")
    tag: u32,
    data_residue: u32,
    status: u8, // 0=Passed, 1=Failed, 2=Phase Error
}
```

Transfer CBW/CSW & data-nya sama-sama pakai `enqueue_normal_trb_and_ring`
yang sudah ada — dikirim ke ring Bulk OUT untuk CBW & data-write, ring Bulk
IN untuk data-read & CSW.

### 5.4 SCSI Command Dasar yang Wajib Diimplementasi
| Command | Opcode | Fungsi |
|---|---|---|
| `INQUIRY` | 0x12 | Info device (vendor, produk) |
| `READ CAPACITY (10)` | 0x25 | Dapat jumlah block & ukuran block |
| `READ (10)` | 0x28 | Baca block data |
| `WRITE (10)` | 0x2A | Tulis block data |
| `TEST UNIT READY` | 0x00 | Cek device siap (kadang perlu di-retry setelah insert) |

Setelah driver ini jalan, block device (`read_block(lba, buf)` /
`write_block(lba, buf)`) siap dipakai filesystem layer di atasnya (FAT32,
dst).

---

## 6. Contoh Pola Class Lain (Ringkas)

### CDC (USB-to-Serial / modem)
- Interface class `0x02` (Communication) + `0x0A` (Data).
- Butuh **2 interface**: satu untuk control (Interrupt IN kecil buat status
  notification), satu untuk data (Bulk IN/OUT untuk data serial aktual).
- Protokol jauh lebih sederhana dari BOT — kebanyakan cuma pipe data mentah.

### Audio
- Interface class `0x01`, pakai endpoint **Isochronous**.
- Butuh format sample rate/bit depth dinegosiasikan lewat class-specific
  descriptor tambahan — lebih rumit dari HID/Mass Storage, biasanya
  prioritas paling akhir untuk hobby OS.

### Hub
- Interface class `0x09`. Device ini **punya port USB sendiri** — kalau
  device di-plug ke hub (bukan langsung ke root port controller), driver
  hub-lah yang mendeteksi & memicu enumerasi device baru itu (`enable_slot`
  dst, tapi dengan parent hub slot, bukan langsung root port). Ini
  menambah kompleksitas ke `address_device` (perlu info hub slot & port),
  jadi biasanya diimplementasi belakangan setelah device langsung di root
  port sudah stabil.

---

## 7. Kerangka Registry Driver (Supaya Gampang Nambah Class Baru)

Supaya `main.rs` tidak makin gemuk tiap nambah class baru, pisahkan jadi
tabel dispatch:

```rust
type DriverInitFn = unsafe fn(slot_id: u32, info: &InterfaceInfo) -> bool;

const DRIVER_TABLE: &[(u8, u8, DriverInitFn)] = &[
    // (interface_class, interface_subclass, init_fn)
    (0x03, 0x01, hid::init),           // HID boot device
    (0x08, 0x06, mass_storage::init),  // SCSI mass storage
];

pub unsafe fn dispatch_driver(slot_id: u32, info: &InterfaceInfo) {
    for &(class, subclass, init_fn) in DRIVER_TABLE {
        if info.class == class && info.subclass == subclass {
            unsafe { init_fn(slot_id, info); }
            return;
        }
    }
    println!("Tidak ada driver untuk class={:#04x} subclass={:#04x}", info.class, info.subclass);
}
```

Setiap device class baru = tambah 1 baris tabel + 1 modul baru, tanpa ubah
`main.rs` atau transport layer sama sekali.

---

## 8. Checklist Bikin Driver USB Class Baru

- [ ] Cek `interface_class`/`subclass`/`protocol` dari Configuration
      Descriptor — jangan asumsi dari Device Descriptor kalau `class==0x00`
- [ ] Identifikasi tipe endpoint yang dibutuhkan (Bulk? Interrupt?
      Isochronous?) dari spec class tersebut
- [ ] `configure_endpoint()` untuk tiap endpoint yang dipakai — fungsi ini
      **sudah generik**, tidak perlu diubah
- [ ] Baca spec protokol di atas transport (BOT untuk mass storage, HID
      report untuk HID, dst) — ini bagian yang benar-benar beda per class
- [ ] Alokasikan buffer & state sekali di awal, bukan tiap transfer
- [ ] Tangani completion event di layer yang sesuai (`poll_event_ring`
      sudah generik untuk semua endpoint, tinggal routing berdasarkan DCI
      seperti pola `KBD_DCI` yang sudah ada)
- [ ] Daftarkan ke dispatch table supaya device baru otomatis dapat driver
      yang tepat

---

*Kesimpulan: xHCI transport + device enumeration yang sudah kamu bangun
adalah "kerja berat" yang cuma dilakukan sekali. Menambah driver class baru
(flashdisk, serial, dst) itu kerjaan lebih ringan — tinggal parsing
descriptor lebih lengkap, pilih endpoint yang tepat, dan implementasi
protokol spesifik class-nya di atas primitif yang sudah ada.*
