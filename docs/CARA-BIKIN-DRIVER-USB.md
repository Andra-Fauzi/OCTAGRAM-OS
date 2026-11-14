# Panduan Pembuatan USB Driver (bare-metal, xHCI, Rust)

Panduan ini disusun berdasarkan arsitektur yang sudah ada di project ini
(`pci.rs`, `xhci.rs`, `idt.rs`, `interrupt.rs`, `keyboard_usb.rs`). Fokusnya
adalah alur kerja membuat driver USB dari nol di atas host controller **xHCI**
(USB 3.x), dari level PCI sampai HID device driver seperti keyboard.

---

## 0. Gambaran Besar Alur

```
PCI enumeration
   → temukan xHCI controller (class 0x0C, subclass 0x03, prog_if 0x30)
   → enable device (Memory Space + Bus Master)
   → baca BAR0 (MMIO base address)
   → map BAR0 ke virtual memory
   → setup interrupt (MSI-X, fallback ke polling)
   → init xHCI controller (reset, DCBAA, Command Ring, Event Ring, Run)
   → scan port fisik
   → enable slot → address device → get descriptor
   → configure endpoint (interrupt IN, dst)
   → device-class driver (HID keyboard, mass storage, dll)
```

Setiap tahap wajib sukses sebelum lanjut ke tahap berikutnya — kalau BAR belum
di-map, controller belum di-reset dengan benar, atau ring belum disiapkan,
tahap sesudahnya akan crash atau silent-fail.

---

## 1. Tahap PCI: Temukan & Aktifkan Controller

### 1.1 Enumerasi bus
Scan brute-force `bus 0..255`, `device 0..32`, `function 0..8` lewat
Configuration Space Mechanism #1 (`CONFIG_ADDRESS`/`CONFIG_DATA` di port I/O
`0xCF8`/`0xCFC`). Device kosong ditandai `vendor_id == 0xFFFF`.

```rust
pub fn find_usb_controllers() -> Vec<PciDevice> {
    // cari class == 0x0C (Serial Bus Controller), subclass == 0x03 (USB)
}
```

`prog_if` menentukan jenis host controller:

| prog_if | Jenis  |
|--------:|--------|
| 0x00 | UHCI (USB 1.1) |
| 0x10 | OHCI (USB 1.1) |
| 0x20 | EHCI (USB 2.0) |
| 0x30 | **xHCI (USB 3.x, rekomendasi)** |

### 1.2 Enable device
Set bit `Memory Space` dan `Bus Master` di PCI Command register (offset
`0x04`) — wajib sebelum akses BAR atau sebelum controller bisa DMA.

### 1.3 Baca BAR0
xHCI hampir selalu pakai 64-bit MMIO BAR. Deteksi lewat bit 2-1 BAR:
`bar_type == 0x2` berarti 64-bit, gabungkan BAR0 (low 32-bit) dengan BAR1
(high 32-bit).

Untuk tahu ukurannya (dibutuhkan untuk page mapping): tulis semua bit `1` ke
BAR, baca balik (hardware cuma nyalain bit yang valid), lalu **kembalikan
nilai asli** — kalau lupa, konfigurasi device bisa rusak.

### 1.4 Map MMIO ke virtual memory
```rust
let virt_base = memory::paging::map_mmio_page(bar.address);
for i in 1..page_count { memory::paging::map_mmio_page(bar.address + i * 0x1000); }
```
Semua akses register controller sesudah ini pakai `virt_base`, bukan alamat
fisik.

---

## 2. Tahap Interrupt: MSI-X (dengan Fallback Polling)

xHCI mendukung legacy INTx, MSI, dan MSI-X — **selalu prioritaskan MSI-X**
kalau tersedia, karena lebih stabil untuk banyak device modern.

### 2.1 Setup MSI-X
1. Cari MSI-X capability lewat capability linked-list PCI (`find_capability`,
   cap_id `0x11`).
2. Ambil info table (`table_bar`, `table_offset`, `table_size`) dari MSI-X
   Capability Register.
3. Tulis entry pertama tabel MSI-X: `message address` (`0xFEE00000 |
   (apic_id << 12)`) dan `message data` (= vector interrupt).
4. Set bit *MSI-X Enable* di Message Control register.

### 2.2 Registrasi IDT + IOAPIC/LAPIC
- Buat *interrupt stub* naked-asm untuk vector yang dipakai (lihat pola
  `interrupt_stub!` macro + `stub_common` di `interrupt.rs`) — stub ini
  push semua register, panggil handler Rust, pop balik, lalu `iretq`.
- Daftarkan handler di IDT pada vector yang sama dengan yang dipakai di MSI-X
  (di project ini vector 44).
- Kirim **EOI ke LAPIC** di akhir handler untuk semua vector ≥ 32, atau
  interrupt berikutnya tidak akan pernah masuk lagi.

### 2.3 Fallback
Kalau MSI-X gagal (device tidak mendukung), tetap sediakan mode
polling manual (`loop { xhci.poll_event_ring(); }`) supaya driver tidak
mati total di hardware yang lebih tua/aneh.

---

## 3. Tahap Init xHCI Controller

Urutan ini **kaku**, jangan diacak:

1. **Stop controller** — clear `Run/Stop` bit di USBCMD, tunggu `HCH` (HC
   Halted) jadi 1 di USBSTS.
2. **Host Controller Reset** — set bit `HCRST`, tunggu bit itu auto-clear,
   lalu tunggu `CNR` (Controller Not Ready) jadi 0.
3. **Set MaxSlots** — baca `HCSPARAMS1.MaxSlots`, tulis ke register CONFIG.
4. **Setup DCBAA** (Device Context Base Address Array) — alokasikan array
   pointer fisik (align 64 byte), tulis alamat fisiknya ke register DCBAAP.
5. **Setup Command Ring** — alokasikan ring TRB (align 64 byte), tulis
   `phys_addr | cycle_bit` ke register CRCR.
6. **Setup Event Ring** — perlu 3 struktur: segment TRB, ERST (Event Ring
   Segment Table) entry, dan register interrupter (`ERSTSZ`, `ERSTBA`,
   `ERDP`). Set bit *Interrupt Enable* di IMAN interrupter.
7. **Start controller** — set bit `INTE` + `Run/Stop` di USBCMD, tunggu
   `HCH` jadi 0.

> Semua struktur (DCBAA, ring, ERST) **wajib align 64-byte** dan pakai
> alamat fisik (bukan virtual) saat ditulis ke register hardware — pakai
> helper semacam `memory::virtual_to_physical()`.

---

## 4. Tahap Port & Device Enumeration

### 4.1 Scan port
Baca register `PORTSC` per port (offset `0x400 + port*0x10` dari operational
base). Bit 0 = *Current Connect Status*. Bit 10-13 = speed device.

### 4.2 Enable Slot
Kirim Command TRB `TRB_TYPE_ENABLE_SLOT` ke Command Ring, ring doorbell 0,
tunggu **Command Completion Event** muncul di Event Ring (lewat polling
`COMPLETION_PENDING` yang di-set oleh interrupt handler). Completion code `1`
= sukses, hasilnya `slot_id`.

### 4.3 Address Device
1. Alokasikan **Transfer Ring EP0**, **Device Context**, dan **Input
   Context**.
2. Isi slot context: route string, speed, root port number.
3. Isi endpoint context EP0: `max_packet_size` sesuai speed (Low=8,
   Full=64, High=64, Super=512), pointer ke Transfer Ring EP0.
4. Simpan pointer Device Context ke slot `slot_id` di DCBAA.
5. Kirim Command TRB `TRB_TYPE_ADDRESS_DEVICE`, tunggu completion.

Setelah ini device sudah punya USB address dan EP0 siap dipakai untuk
control transfer.

### 4.4 Get Device Descriptor
Kirim **Control Transfer** standar 3 tahap lewat EP0:
- **Setup Stage TRB**: `bmRequestType=0x80, bRequest=GET_DESCRIPTOR(0x06),
  wValue=(DEVICE<<8), wLength=18`
- **Data Stage TRB**: arah IN, buffer 18 byte
- **Status Stage TRB**: arah OUT (kebalikan data stage), `IOC=1` supaya dapat
  completion event

Tunggu **Transfer Completion Event**. Completion code `1` (Success) atau `13`
(Short Packet) sama-sama valid.

### 4.5 Get Configuration Descriptor & Cari Endpoint
Minta Configuration Descriptor (buffer lebih besar, ~256 byte cukup untuk
device sederhana), lalu **parse manual** rangkaian descriptor:
```
[Configuration][Interface][Endpoint][Endpoint]...
```
Setiap descriptor diawali `bLength` + `bDescriptorType`, jadi bisa di-`while`
loop maju sejumlah `bLength`. Cari Endpoint dengan:
- `address & 0x80 != 0` → arah IN
- `attributes & 0x3 == 0x3` → tipe Interrupt

### 4.6 Set Configuration
`control_transfer_no_data(slot_id, 0x00, SET_CONFIGURATION(0x09), config_value, 0)`
— wajib sebelum endpoint non-default bisa dipakai.

---

## 5. Tahap Configure Endpoint

Hitung **DCI (Device Context Index)**: `dci = ep_number*2 + (1 jika IN, 0 jika OUT)`.

1. Alokasikan Transfer Ring khusus endpoint ini, pasang **Link TRB** di akhir
   ring supaya ring bisa wrap-around dengan benar (beda dari EP0 yang masih
   pakai cara sederhana tanpa Link TRB).
2. Isi Input Context: `add_flags` menandai slot context + endpoint context
   yang diubah, isi `EndpointContext` (tipe endpoint, max packet size,
   interval, pointer transfer ring).
3. Kirim Command TRB `TRB_TYPE_CONFIGURE_ENDPOINT`, tunggu completion.

Endpoint sekarang siap menerima transfer data (submit lewat Normal TRB +
ring doorbell dengan target = DCI).

---

## 6. Tahap Device-Class Driver (contoh: HID Keyboard)

Pola umum driver class di atas transport xHCI:

1. **Set protokol class-specific** (misal HID Boot Protocol lewat
   `SET_PROTOCOL` request, `bmRequestType=0x21`).
2. **Alokasikan buffer laporan (report buffer)** sekali di awal, pakai
   berulang — jangan alokasi ulang tiap interrupt (mahal & bisa gagal di
   dalam interrupt context).
3. **Arm transfer pertama**: submit Normal TRB ke endpoint interrupt IN,
   ring doorbell. Controller akan generate Transfer Completion Event begitu
   device kirim data.
4. **Di interrupt handler**: cek vector, baca flag "report pending" (set oleh
   `poll_event_ring`), baca buffer, proses data, **re-arm transfer
   berikutnya**. Kalau lupa re-arm, device cuma kirim data sekali lalu diam.
5. **State driver** sebaiknya dibungkus `Mutex<Option<State>>` global —
   hindari `static mut` supaya aman diakses dari main loop *dan* dari
   interrupt handler tanpa data race.

### Pola deteksi "key baru ditekan" (debouncing sederhana)
Bandingkan `keycodes` laporan sekarang dengan laporan sebelumnya
(`last_keycodes`) — kalau keycode sama masih ada di laporan lama, berarti
tombol masih ditahan, jangan trigger ulang.

---

## 7. Checklist Kesalahan Umum

| Gejala | Kemungkinan Penyebab |
|---|---|
| Controller stuck di `CNR` / tidak pernah `HCH=0` | Urutan reset salah, atau lupa tunggu `HCRST` auto-clear dulu |
| Command tidak pernah selesai (`COMPLETION_PENDING` tidak pernah true) | Event Ring belum di-setup dengan benar, atau EOI ke LAPIC tidak dikirim sehingga interrupt berikutnya tidak masuk |
| Data descriptor isinya sampah / alignment error | Struct pakai `#[repr(C, packed)]` tapi dibaca dengan `read()` biasa — harus `read_unaligned()` |
| Endpoint cuma kirim data sekali lalu berhenti | Lupa re-arm (submit ulang) Normal TRB setelah setiap Transfer Completion Event |
| BAR/MMIO garbage setelah probe ukuran BAR | Lupa mengembalikan nilai asli BAR setelah menulis `0xFFFFFFFF` untuk `get_bar0_size` |
| Ring korup setelah banyak transfer | Ring wrap-around tanpa Link TRB fisik (aman untuk sedikit transfer, wajib diperbaiki untuk beban tinggi) |
| Device di-detect tapi Address Device gagal | Slot context / speed field salah, atau port number salah (root port harus sesuai `PORTSC` yang connected) |

---

## 8. Urutan Referensi Cepat (checklist implementasi)

- [ ] PCI scan + temukan controller class 0x0C/0x03
- [ ] Enable Memory Space + Bus Master
- [ ] Baca & map BAR0 (hitung ukuran dengan aman, kembalikan nilai asli)
- [ ] Setup MSI-X (fallback: polling)
- [ ] IDT entry + EOI di handler
- [ ] Reset controller → MaxSlots → DCBAA → Command Ring → Event Ring → Run
- [ ] Scan port fisik
- [ ] Enable Slot → Address Device → Get Device Descriptor
- [ ] Get Configuration Descriptor → parse endpoint → Set Configuration
- [ ] Configure Endpoint (dengan Link TRB)
- [ ] Driver class-specific (set protocol, arm transfer, handle + re-arm)

---

*Disusun berdasarkan pola implementasi xHCI + HID keyboard di project ini
(`pci.rs`, `xhci.rs`, `idt.rs`, `interrupt.rs`, `keyboard_usb.rs`, `apic.rs`,
`main.rs`). Bisa dipakai sebagai basis untuk driver USB class lain seperti
mass storage (bulk endpoint) atau HID mouse — pola transport (slot, address,
configure endpoint) sama, yang beda hanya parsing descriptor & protokol
class-specific-nya.*
