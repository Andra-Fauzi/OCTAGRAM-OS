# Panduan Driver USB Generik

Panduan ini membedakan lapisan USB yang bisa dipakai ulang dari protokol
masing-masing device class. Status di bawah mengacu pada implementasi saat ini,
bukan klaim bahwa seluruh fitur xHCI atau USB sudah tersedia.

Yang sudah ada:

- Enumerasi device yang terhubung langsung ke root port xHCI.
- Pembacaan satu Configuration Descriptor dan pengumpulan endpoint dengan
  metadata interface.
- Driver HID boot keyboard/mouse dan Mass Storage Bulk-Only Transport (BOT).
- Pendaftaran USB Mass Storage sebagai block device; filesystem dapat dipasang
  lewat shell.

Hub, CDC-ACM, USB Audio, UASP, konfigurasi alternatif, dan transfer isochronous
belum didukung sebagai fitur lengkap.

---

## 1. Yang Generik vs Yang Spesifik-Class

```
┌─────────────────────────────────────────┐
│  Device-class driver (BEDA per class)    │  ← HID parser, SCSI command, dst
├─────────────────────────────────────────┤
│  Endpoint I/O (primitif xHCI)             │  ← submit TRB; routing terbatas
├─────────────────────────────────────────┤
│  Descriptor parsing (dasar tersedia)      │  ← endpoint + metadata interface
├─────────────────────────────────────────┤
│  Device enumeration (root port)           │  ← hub belum didukung
├─────────────────────────────────────────┤
│  Transport xHCI (sebagian tersedia)      │  ← ring, doorbell, event
└─────────────────────────────────────────┘
```

Pembagian ini adalah model konseptual. Dalam kode saat ini, transport dan
enumerasi dasar sudah ada, tetapi routing completion dan transfer belum
sepenuhnya generik. Referensi implementasi:

- [xHCI dan endpoint](../kernel/src/drivers/usb/xhci.rs)
- [Enumerasi dan dispatch class](../kernel/src/drivers/usb/device.rs)
- [HID keyboard](../kernel/src/drivers/usb/keyboard_usb.rs) dan
  [HID mouse](../kernel/src/drivers/usb/mouse_usb.rs)
- [Mass Storage BOT](../kernel/src/drivers/usb/mass_storage.rs)

Driver class baru tetap memerlukan pemilihan interface, transfer yang sesuai
dengan tipe endpoint, routing completion yang aman, dan cleanup saat device
dilepas.

---

## 2. Empat Tipe Endpoint USB (Kenali Ini Dulu)

| Tipe | Karakteristik | Dipakai untuk |
|---|---|---|
| **Control** | Request/response terstruktur melalui endpoint 0 (EP0) | Descriptor dan request standar USB |
| **Interrupt** | Transfer kecil berkala | HID keyboard dan mouse |
| **Bulk** | Transfer data dengan retry transaksi pada bus; error command tetap perlu ditangani driver | Mass Storage BOT |
| **Isochronous** | Transfer dengan bandwidth/waktu terjadwal; tidak ada retry untuk data yang hilang | Audio, webcam, streaming |

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

`configure_endpoint()` membentuk context endpoint non-control untuk tipe
Interrupt, Bulk, dan Isochronous. Ini **belum** berarti semua tipe sudah bisa
dipakai untuk I/O: transfer HID memakai jalur Interrupt yang ada, sedangkan
`bulk_transfer()` dipakai oleh Mass Storage. Belum ada jalur transfer
Isochronous; EP0 memakai fungsi control-transfer tersendiri.

---

## 3. Deteksi Device Class (Setelah Get Descriptor)

Setelah `get_device_descriptor()`, cek `device_class`:

| device_class | Class | Contoh |
|---|---|---|
| `0x00` | Didefinisikan di **Interface**, bukan Device | Paling umum — harus cek interface descriptor |
| `0x03` | HID | Keyboard, mouse, gamepad |
| `0x08` | Mass Storage | Flashdisk, hardisk USB |
| `0x02` | CDC Communication | USB-to-serial, modem |
| `0x01` | Audio | Speaker/mic USB |
| `0x09` | Hub | USB hub (device khusus, punya port sendiri) |
| `0xE0` | Wireless Controller | Bluetooth dongle |

`0x0A` adalah class CDC Data yang lazim ditemui pada **Interface Descriptor**,
bukan nilai `device_class` yang setara dengan `0x02`.

Kalau `device_class == 0x00` (umum), class ada di **Interface Descriptor**.
Implementasi sekarang memakai `get_configuration_descriptor_endpoints()`:
hasilnya berupa daftar datar `EndpointInfo`, masing-masing membawa nomor,
class, subclass, dan protocol interface. Parser ini sudah mengumpulkan endpoint
dengan berbagai tipe, bukan hanya Interrupt IN.

Parser saat ini membaca configuration index 0 ke buffer berukuran 256 byte.
Ia belum memodelkan interface sebagai objek tersendiri, alternate setting,
atau beberapa configuration. Nama field nomor interface di `EndpointInfo`
adalah `inteface_number` (ejaan lama yang salah); perhatikan ejaan ini jika
membaca kode yang ada.

Dispatch yang aktif saat ini mendukung HID boot keyboard/mouse dan
Mass Storage interface class `0x08`, subclass `0x06`, protocol `0x50`.
Class lain belum otomatis didukung hanya karena descriptor-nya berhasil
diparsing.

`device.rs` mendaftarkan setiap interface USB ke device model. Driver HID dan
Mass Storage BOT terdaftar sebagai driver built-in, lalu dicocokkan berdasarkan
class/subclass/protocol dan endpoint. Driver harus dikompilasi bersama kernel;
registry ini belum memuat modul driver saat OS berjalan.

---

## 4. Parsing Configuration Descriptor

Parser yang ada sudah melewati descriptor berdasarkan `bLength` dan
mengumpulkan endpoint dari interface yang ditemukan. Karena hasilnya daftar
datar, driver class harus mengelompokkan endpoint berdasarkan nomor interface
dan memeriksa class/subclass/protocol sebelum mengonfigurasi endpoint.

Perbaikan parser berikutnya yang berguna:

1. Validasi header configuration, `wTotalLength`, jumlah interface, dan batas
   buffer sebelum membaca descriptor.
2. Kembalikan error untuk descriptor terpotong atau panjang tidak valid,
   bukan mengakhiri loop lalu menerima hasil parsial.
3. Representasikan interface sebagai struktur yang memuat nomor, alternate
   setting, class/subclass/protocol, serta endpoint miliknya.
4. Batasi endpoint pada alternate setting yang dipilih, dan pilih konfigurasi
   dengan jelas.

Struktur `InterfaceInfo` dapat menjadi arah refactor, tetapi belum merupakan
API yang tersedia. Untuk class baru sederhana, gunakan API `EndpointInfo` yang
ada dan jangan menganggap semua descriptor valid.

---

## 5. Mass Storage BOT yang Sudah Ada

### 5.1 Deteksi dan endpoint
Driver saat ini menerima interface dengan class `0x08`, subclass `0x06`
(SCSI Transparent Command Set), dan protocol `0x50` (Bulk-Only Transport atau
**BOT**). Ia mencari endpoint Bulk IN dan Bulk OUT pada nomor interface yang
sama. Perangkat dengan subclass/protocol lain, UASP, atau beberapa konfigurasi
belum ditangani.

Secara umum pasangan endpoint diperiksa seperti ini:

```rust
let bulk_in = endpoints.iter().find(|e| {
    e.inteface_number == interface_number
        && e.interface_class == 0x08
        && e.interface_subclass == 0x06
        && e.interface_protocol == 0x50
        && e.attributes & 0x3 == 0x2
        && e.address & 0x80 != 0
});
let bulk_out = endpoints.iter().find(|e| {
    e.inteface_number == interface_number
        && e.interface_class == 0x08
        && e.interface_subclass == 0x06
        && e.interface_protocol == 0x50
        && e.attributes & 0x3 == 0x2
        && e.address & 0x80 == 0
});
```

Keduanya dikonfigurasi dengan `configure_endpoint()`, lalu transfer dilakukan
melalui `bulk_transfer()`. API ini bukan driver transport generik untuk semua
tipe endpoint.

### 5.2 Siklus BOT

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

Di implementasi sekarang, CBW, data, dan CSW dikirim memakai
`bulk_transfer()` ke ring endpoint arah yang sesuai. Buffer DMA disediakan
terpisah dari buffer milik caller.

### 5.3 Cakupan dan batas implementasi

Driver yang ada menjalankan `INQUIRY` dan `READ CAPACITY (10)` saat inisialisasi,
kemudian mendukung `READ (10)` dan `WRITE (10)` melalui block-device layer.
CBW/CSW memeriksa panjang transfer, signature, tag, dan status CSW.

Belum didukung: `TEST UNIT READY`/retry untuk media yang belum siap, SCSI
`REQUEST SENSE`, pemulihan BOT reset/phase error, pemeriksaan CSW residue,
multi-LUN, block size selain 512 byte, dan LBA di atas batas 32-bit
`READ/WRITE (10)`. Transfer data dibatasi buffer internal 4 KiB per potongan.

Perangkat didaftarkan sebagai block device bernama `usbN`. Mount FAT32 dan
perintah filesystem dijelaskan di [panduan storage](storage.md). Gunakan image
atau media uji kosong untuk pengujian tulis; FAT32 dan driver ini tidak
menyediakan jaminan pemulihan transaksi saat perangkat dicabut di tengah tulis.

### 5.4 Perintah SCSI terkait

| Command | Opcode | Fungsi |
|---|---|---|
| `INQUIRY` | 0x12 | Info device (vendor, produk) |
| `READ CAPACITY (10)` | 0x25 | Dapat jumlah block & ukuran block |
| `READ (10)` | 0x28 | Baca block data |
| `WRITE (10)` | 0x2A | Tulis block data |
| `TEST UNIT READY` | 0x00 | Belum diimplementasikan; berguna untuk cek kesiapan media |

---

## 6. Contoh Pola Class Lain (Ringkas)

### CDC (USB-to-Serial / modem)
- CDC-ACM umumnya memakai interface Communication (`0x02`) dan interface Data
  (`0x0A`); subclass/protocol dan descriptor class-specific juga perlu dibaca.
- Pasangkan control dan data interface melalui descriptor asosiasi CDC, bukan
  dengan mengasumsikan nomor interface berurutan. Data biasanya memakai Bulk
  IN/OUT; notifikasi control dapat memakai Interrupt IN.
- Belum ada driver CDC-ACM. Transfer data saja belum cukup: implementasikan
  request class seperti pengaturan line coding dan kontrol modem yang memang
  diperlukan device.

### Audio
- Interface class `0x01` umumnya memerlukan endpoint Isochronous dan parsing
  descriptor class-specific untuk format, sample rate, serta kanal audio.
- Context Isochronous dapat dikonfigurasi xHCI, tetapi jalur submit/completion
  Isochronous belum tersedia; driver audio belum didukung.

### Hub
- Hub sering mengiklankan class `0x09` pada Device Descriptor dan memiliki
  descriptor/request hub tersendiri. Dukungan hub perlu memantau perubahan
  status port downstream dan menghubungkan proses enable/address device ke
  parent hub serta port-nya.
- Enumerasi saat ini hanya menemukan device langsung pada root port; hub dan
  device di belakang hub belum didukung.

---

## 7. Registrasi Driver

Device model bersama ada di
[`drivers/device_model.rs`](../kernel/src/drivers/device_model.rs). Bus
mendaftarkan `DeviceInfo::UsbInterface`; `DeviceDriver::matches()` memilih
driver, `probe()` menginisialisasi interface dan menyimpan state per-device,
serta `remove()` membersihkannya saat hot-unplug.

Untuk menambah class, buat implementasi `DeviceDriver`, lalu daftarkan instance
statisnya di `register_class_drivers()` pada `device.rs`. Callback `probe()` dan
`remove()` dijalankan di luar lock registry dan tidak boleh dipanggil dari ISR.
Perintah shell `drivers` dan `lsdev` menampilkan driver yang terdaftar dan
perangkat yang sudah bind. Driver masih built-in; dynamic module loading belum
tersedia.

---

## 8. Checklist Bikin Driver USB Class Baru

- [ ] Cek `interface_class`/`subclass`/`protocol` dari Configuration
      Descriptor — jangan asumsi dari Device Descriptor kalau `class==0x00`
- [ ] Pastikan endpoint berasal dari interface dan alternate setting yang
      sama; cocokkan arah serta tipe transfer.
- [ ] Identifikasi tipe endpoint yang dibutuhkan (Bulk? Interrupt?
      Isochronous?) dari spec class tersebut
- [ ] Pastikan jalur transfer dan completion untuk tipe endpoint tersebut
      benar-benar tersedia; `configure_endpoint()` saja belum cukup
- [ ] Baca spec protokol di atas transport (BOT untuk mass storage, HID
      report untuk HID, dst) — ini bagian yang benar-benar beda per class
- [ ] Validasi alamat DMA, panjang transfer, timeout, completion code, dan
      data residue sesuai protokol
- [ ] Route completion berdasarkan slot dan DCI; saat ini event keyboard/mouse
      punya state khusus, sementara completion transfer lain memakai state
      bersama
- [ ] Bersihkan endpoint, state class, dan registrasi block device saat
      hot-unplug; pastikan filesystem tidak memakai media yang sudah dilepas
- [ ] Uji dengan device atau image disposable, termasuk error dan unplug

---

## 8. Langkah Lanjutan yang Disarankan

Sebelum menambah class baru, uji alur USB Mass Storage yang sudah ada secara
end-to-end dengan image FAT32 disposable: enumerasi, mount, baca/tulis,
unmount, lalu verifikasi data tetap ada setelah reboot. Tambahkan pengujian
hot-unplug dan kegagalan transfer. Setelah jalur Bulk dan cleanup stabil,
CDC-ACM merupakan kandidat class berikutnya; Audio dan Hub memerlukan
dukungan transport/controller tambahan yang lebih besar.
