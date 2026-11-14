# Cara Bikin Driver Lain — Panduan Umum + Resep Spesifik

Dokumen ini melanjutkan `dokumentasi-xhci-keyboard.md`. Fokusnya: pola umum yang bisa kamu pakai ulang untuk driver APAPUN di kernel ini, plus resep langkah-demi-langkah untuk beberapa driver yang paling masuk akal jadi langkah selanjutnya.

---

## Bagian A — Pola Umum (berlaku untuk semua driver)

Semua driver hardware, sesederhana apapun, ngikutin kerangka yang sama. Ini yang sudah kamu praktikkan tanpa sadar waktu bikin xHCI:

### 1. Cari device-nya
Dua cara utama tergantung jenis hardware:
- **Device PCI** (xHCI, AHCI, NVMe, kartu jaringan) → scan lewat `pci::scan_pci_bus()` / `pci::find_usb_controllers()` polanya, cari berdasarkan `class`/`subclass`/`prog_if`, atau `vendor_id`/`device_id` spesifik.
- **Device lama non-PCI** (PS/2, RTC/CMOS, PIT, serial COM1) → biasanya di alamat I/O port **tetap** (fixed, sudah dari jaman DOS), gak perlu di-scan, langsung dipakai.

### 2. Cari tahu peta register-nya
Baca datasheet/spesifikasi resminya (atau OSDev Wiki untuk device umum). Kamu butuh tahu:
- Register apa saja yang ada, di offset berapa, ukurannya berapa bit.
- Register itu diakses lewat **MMIO** (baca/tulis alamat memori, seperti xHCI) atau **Port I/O** (`in`/`out` instruction, seperti PIC/PS2/RTC)?

### 3. Definisikan struct "view" ke register
Sama seperti `XhciCapRegs`/`XhciOpRegs`, bikin struct simpel yang cuma nyimpen base address, terus method-method `unsafe fn read_x()`/`write_x()` yang dihitung dari offset:

```rust
pub struct FooRegs {
    base: *mut u8,
}
impl FooRegs {
    pub unsafe fn read_status(&self) -> u32 {
        unsafe { (self.base.add(0x04) as *const u32).read_volatile() }
    }
}
```
Kalau device-nya Port I/O, pakai `io::inb/outb/inl/outl` (sudah ada di `io.rs`) alih-alih pointer.

### 4. Kalau butuh DMA (device baca/tulis memori sendiri)
Ini pola yang selalu sama di kernel ini:
```rust
let thing_box: Box<SomeStruct> = Box::new(SomeStruct::new());
let thing_virt = Box::into_raw(thing_box) as u64;
let thing_phys = memory::virtual_to_physical(thing_virt as usize).unwrap() as u64;
// kasih thing_phys ke device lewat register yang sesuai
```
Device gak paham alamat virtual CPU — dia cuma paham alamat fisik. Ini kenapa hampir semua driver DMA di kernelmu punya baris "translate ke phys" sebelum ngasih alamat ke hardware.

### 5. Tentukan: polling atau interrupt?
- **Polling** — paling gampang buat awal development (baca status register di loop sampai berubah). Lambat & boros CPU, tapi bagus buat debugging awal karena gampang di-`println!` tiap langkah.
- **Interrupt** — lebih efisien, tapi butuh setup lebih (daftar vector di IDT, konfigurasi IOAPIC/MSI-X, kirim EOI). **Saran: selalu bikin versi polling dulu sampai jalan benar, baru upgrade ke interrupt** — itu juga kenapa kode xHCI kamu punya fallback `loop { xhci.poll_event_ring(); }`.

### 6. Kalau perlu interrupt, hook ke IDT
Pola yang sudah ada di `interrupt.rs`:
```rust
interrupt_stub!(nama_stub_baru, NOMOR_VECTOR, no_error_code);
```
Lalu daftarkan di `idt.rs`:
```rust
IDT[NOMOR_VECTOR] = IdtEntry::new(nama_stub_baru as u64, 0x08);
```
Dan tambahkan case di `common_interrupt_handler` (`interrupt.rs`) buat vector itu. Kalau device-nya legacy (IRQ line, bukan MSI-X), perlu daftarkan juga redirect-nya di IOAPIC lewat `apic::IoApic::set_redirect(irq, vector, apic_id)`.

### 7. Bikin static instance global (kalau perlu diakses dari interrupt handler)
Sama seperti `XHCI_INSTANCE: Once<Mutex<XHCI>>`:
```rust
pub static FOO_INSTANCE: Once<Mutex<Foo>> = Once::new();
```
`Once` supaya bisa diisi belakangan (setelah alokasi dinamis selesai), `Mutex` supaya aman diakses dari main context maupun interrupt context (ingat: selalu `cli` sebelum lock dari main context, seperti dibahas di dokumentasi sebelumnya).

### 8. Uji incremental, jangan langsung full flow
Urutan development yang paling gak bikin frustrasi:
1. Deteksi device-nya dulu → `println!` info dasarnya (vendor/device id, BAR, dst). Stop di sini dulu, pastikan device kedetect benar.
2. Reset & inisialisasi paling minimal → `println!` status register buat pastikan device "hidup".
3. Satu operasi paling sederhana (baca 1 data) via polling → pastikan hasilnya masuk akal.
4. Baru pindah ke interrupt & alur penuh.

### 9. Checklist umum bug yang sering kejadian (dari pengalaman xHCI)
- Lupa **Status Stage** di control transfer tanpa data → timeout.
- Ring/queue yang wrap tanpa **Link TRB / Link descriptor** yang benar → device macet setelah sekian operasi.
- Lupa **kirim EOI** di akhir interrupt handler → interrupt berikutnya gak pernah masuk.
- Lupa translate alamat virtual→fisik sebelum dikasih ke device → device baca/tulis ke alamat yang salah/nyasar (bisa crash acak, susah dilacak).
- Buffer yang di-reuse tanpa re-arm ring → device diam padahal harusnya lanjut kirim data.

---

## Bagian B — Resep Driver Spesifik

Diurutkan dari yang paling gampang ke yang paling mirip kompleksitasnya sama xHCI.

### B1. Serial Port / UART 16550 (COM1) — **paling gampang, cocok buat pemanasan**

Kenapa mulai dari sini: **Port I/O murni, gak ada DMA, gak wajib interrupt.** Cocok banget buat latihan pola "device lama" sebelum lanjut ke yang PCI.

- Alamat tetap: COM1 = `0x3F8`.
- Register penting (offset dari base):
  - `+0`: data register (baca/tulis byte)
  - `+5`: Line Status Register — bit 5 = "transmitter empty" (boleh kirim byte baru), bit 0 = "data ready" (ada byte masuk buat dibaca)
- Alur inisialisasi: disable interrupt, set baud rate divisor (lewat DLAB bit), set 8N1 (8 data bit, no parity, 1 stop bit), enable FIFO.
- Kirim 1 byte: tunggu bit "transmitter empty" di LSR, baru tulis ke data register.
- Baca 1 byte: tunggu bit "data ready" di LSR, baru baca dari data register.

**Manfaat nyata:** begitu ini jalan, kamu bisa `println!` ke luar VM lewat serial console (`-serial stdio` di QEMU) — ini jauh lebih gampang di-debug/di-log dibanding cuma lihat framebuffer, apalagi kalau kernel-nya sampai crash sebelum sempat gambar apa-apa ke layar.

### B2. RTC / CMOS (Real Time Clock) — **kedua tergampang**

- Port I/O juga: index register `0x70`, data register `0x71`.
- Cara baca: tulis nomor register (misal `0x00`=detik, `0x02`=menit, `0x04`=jam, `0x07`=tanggal, `0x08`=bulan, `0x09`=tahun) ke `0x70`, baca hasilnya dari `0x71`.
- Gotcha: hasilnya biasanya dalam format **BCD** (Binary Coded Decimal), bukan biner biasa — perlu konversi (`((val >> 4) * 10) + (val & 0xF)`).
- Gotcha kedua: RTC bisa lagi "update" saat kamu baca (hasil kebaca setengah-update, jadi salah) — cek bit "Update In Progress" di register `0x0A` dulu sebelum baca, atau baca 2x dan bandingkan.

**Manfaat:** dapat waktu real buat timestamp log, atau buat fitur "jam" di OS kamu nanti.

### B3. PIT (Programmable Interval Timer) — timer dasar

- Port I/O: `0x40`-`0x43`.
- Kegunaan: bikin interrupt reguler (misal tiap 10ms) — fondasi buat scheduler, sleep(), atau animasi framebuffer.
- Alur: hitung divisor dari frekuensi target (`1193182 / freq_target`), tulis mode command ke `0x43`, tulis divisor (low byte lalu high byte) ke `0x40`.
- Hook interrupt-nya ke **IRQ 0** (vector 32 di skema kamu, karena PIC di-disable dan pakai IOAPIC — daftarkan lewat `ioapic.set_redirect(0, 32, apic_id)` seperti pola `set_redirect(1, 33, 0)` yang sudah ada buat keyboard PS/2).

Pola ini **lebih simpel** dari xHCI karena gak ada DMA/ring sama sekali — payload interrupt-nya cuma "waktunya sudah lewat", gak ada data yang perlu ditransfer.

### B4. USB Mouse — **paling gampang di antara yang butuh xHCI, karena infrastrukturnya sudah ada!**

Ini literally perluasan dari yang sudah kamu bikin, bukan driver baru dari nol:

- Alur enumerasi **sama persis** kayak keyboard (Enable Slot → Address Device → Get Descriptor → Configure Endpoint → dst), device class-nya juga HID.
- Bedanya cuma di format report & Boot Protocol yang dipilih. Boot Protocol Mouse formatnya:
  ```
  byte 0 : tombol (bit0=kiri, bit1=kanan, bit2=tengah)
  byte 1 : delta X (signed, -127..127)
  byte 2 : delta Y (signed, -127..127)
  ```
- Yang perlu kamu ubah dari `keyboard_usb.rs`: struct report-nya, `handle_report`-nya (bukan mapping keycode→ascii, tapi update posisi kursor `x += delta_x; y += delta_y;` lalu gambar ulang di framebuffer), dan cara membedakan device ini mouse vs keyboard — cek **Interface Protocol** di deskriptor interface (byte offset 7 dari interface descriptor: `1`=keyboard, `2`=mouse) waktu parsing configuration descriptor, supaya driver tahu mau load handler yang mana.
- Konsep ring/DCI/Link TRB/re-arm-nya **identik** sama keyboard — modul `xhci.rs` gak perlu diubah sama sekali, cuma perlu modul baru `mouse_usb.rs` yang isinya mirip `keyboard_usb.rs`.

### B5. PS/2 Keyboard/Mouse — driver fallback yang berharga

Meski kamu sudah punya USB keyboard, PS/2 tetap berguna sebagai **fallback** kalau xHCI/USB gagal init (banyak OS hobi bikin ini duluan sebelum USB karena jauh lebih simpel):

- Port I/O: data port `0x60`, command/status port `0x64`.
- IRQ 1 (keyboard) dan IRQ 12 (mouse), keduanya lewat PIC/IOAPIC legacy.
- Data yang masuk adalah **scancode** (bukan HID keycode) — perlu tabel mapping scancode Set 1/2 ke karakter, beda sama sekali dari mapping HID yang sudah kamu buat.
- Ini driver bagus buat dibandingkan sama USB: kamu akan lihat langsung kenapa USB "berasa ribet" — karena PS/2 gak ada konsep device descriptor, enumerasi, ring, DMA sama sekali; semua cuma byte mentah lewat 1 I/O port.

### B6. AHCI (SATA storage) — mirip xHCI, level kompleksitas serupa

Kalau kamu sudah nyaman sama pola ring/command xHCI, AHCI itu **konsepnya paralel banget**:

| Konsep xHCI | Konsep AHCI |
|---|---|
| Slot | Port (tiap port = 1 disk) |
| Command Ring | Command List (32 command slot per port) |
| TRB | Command Header + Command Table (FIS - Frame Information Structure) |
| Doorbell | Register `PxCI` (Command Issue) — set bit sesuai slot buat "kirim" command |
| Event Ring | Register `PxIS` (Interrupt Status) per port, + `IS` global |
| Device Context | `PxSIG`, `PxSSTS` (status register per port, dibaca bukan struct di memori) |

- Cari device lewat PCI class `0x01` subclass `0x06`.
- Command paling awal buat dicoba: `IDENTIFY DEVICE` (ATA command `0xEC`) — hasilnya struct 512 byte berisi info disk (model, ukuran, dst), bagus buat langkah pertama karena gak butuh alokasi buffer data segede file beneran.
- Setelah itu: `READ DMA EXT` / `WRITE DMA EXT` buat baca/tulis sector.

### B7. NVMe (SSD modern) — kalau mau yang lebih baru dari AHCI

Konsepnya **paling mirip xHCI** dari semua di daftar ini — sama-sama pakai submission queue/completion queue berbasis ring dengan doorbell:

| Konsep xHCI | Konsep NVMe |
|---|---|
| Command Ring | Submission Queue (SQ) |
| Event Ring | Completion Queue (CQ) |
| Doorbell | SQ Tail Doorbell / CQ Head Doorbell (register terpisah per queue) |
| Cycle bit | Phase bit di tiap entry Completion Queue — **konsepnya identik 1:1** |
| Slot/DCI | Namespace ID |

Karena kamu **sudah paham cycle bit & Link TRB** dari xHCI, NVMe akan terasa familiar — bedanya cuma nama register dan format command 64 byte-nya.

### B8. Network Card (e1000 / Intel Gigabit) — **paling deket ke xHCI dari sisi pola DMA**

- Sama-sama PCI, sama-sama pakai **descriptor ring** buat TX (transmit) dan RX (receive), masing-masing ring terpisah (mirip Transfer Ring per endpoint).
- RX ring: kamu siapkan N buffer kosong di ring, device isi begitu paket datang, device kasih tahu lewat interrupt, kamu proses lalu **re-arm** slot itu (identik banget sama pola `arm_next_report` di driver keyboard kamu!).
- TX ring: kamu isi buffer data + panjang, geser "tail pointer" (mirip doorbell), device kirim, kasih interrupt waktu selesai.
- Ini driver bagus buat "naik level" setelah AHCI/NVMe karena polanya udah sangat kamu kenal dari xHCI, tapi datanya (paket Ethernet) beda domain — bagus buat latihan baca protokol baru (Ethernet frame, ARP, dst) di atas fondasi driver yang mekanismenya sudah familiar.

---

## Bagian C — Rekomendasi Urutan Belajar

Kalau mau progresif (dari paling gampang ke paling mirip xHCI), urutan yang masuk akal:

1. **Serial UART** — pemanasan, port I/O murni, langsung kepake buat debug driver-driver berikutnya.
2. **RTC** — port I/O murni juga, hasil konkret gampang divalidasi (waktu sekarang).
3. **PIT + interrupt timer** — mulai kenal interrupt lewat IOAPIC (bukan MSI-X), fondasi buat scheduler nanti.
4. **PS/2 keyboard** — device I/O sederhana + interrupt, tapi punya alur "parsing data mentah" yang beda dari HID, bagus buat perbandingan.
5. **USB Mouse** — hampir gratis, infrastruktur xHCI kamu sudah siap pakai, tinggal modul device-specific baru.
6. **AHCI atau NVMe** — mulai kompleks (perlu paham konsep storage/sector), tapi pola ring-nya sudah kamu kuasai dari xHCI.
7. **e1000 Network** — paling kompleks karena butuh pemahaman tambahan (protokol jaringan), tapi mekanisme driver-nya paling mirip xHCI dari semua pilihan di atas.

---

## Bagian D — Sumber Referensi yang Berguna

- **OSDev Wiki** (osdev.org) — hampir semua device yang disebut di atas punya halaman sendiri dengan detail register & contoh kode.
- **Datasheet resmi Intel** untuk AHCI, NVMe, e1000 — biasanya open dan detail (Intel sering merilis spesifikasi lengkap).
- **QEMU source code** — kalau bingung device di QEMU itu perilakunya kayak apa persisnya, source QEMU (`hw/usb`, `hw/ide`, `hw/net`, dst) adalah "ground truth" paling akurat buat testing di VM kamu.

---

*Panduan ini disusun berdasarkan pola yang sudah terbukti jalan di driver xHCI/keyboard kamu. Semua "resep" di atas sengaja ditulis level konsep + peta register penting, bukan kode jadi — supaya proses ngoding & debug-nya kamu alami sendiri seperti waktu bikin xHCI (itu yang bikin paham beneran, bukan cuma copy-paste).*
