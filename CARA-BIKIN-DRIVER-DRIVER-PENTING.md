# Panduan Driver-Driver Umum Lainnya (selain USB/xHCI)

Project ini sudah punya fondasi: PCI enumeration (`pci.rs`), interrupt
routing (`idt.rs`, `interrupt.rs`, `apic.rs`), dan MMIO mapping
(`memory::paging::map_mmio_page`). Driver-driver lain di bawah ini semuanya
bisa dibangun di atas fondasi yang sama — pola PCI scan → enable device →
map BAR → setup interrupt tetap dipakai berulang.

Diurutkan dari yang **paling sering dibutuhkan duluan** di hobby OS.

---

## 1. Timer

Wajib ada paling awal karena banyak driver lain (termasuk xHCI-mu, lihat
`timeout -= 1` loop) butuh sumber waktu yang lebih baik daripada busy-loop
tebakan.

| Jenis | Akses | Resolusi | Catatan |
|---|---|---|---|
| **PIT (8253/8254)** | Port I/O `0x40-0x43` | ~1ms | Paling gampang, legacy, cukup untuk hobby OS |
| **LAPIC Timer** | MMIO (lewat `LocalApic` yang sudah kamu punya) | Tinggi, per-core | Terbaik untuk scheduler — sudah punya `LocalApic::write/read`, tinggal tambah register timer (offset `0x320` LVT Timer, `0x380` Initial Count, `0x390` Current Count) |
| **HPET** | MMIO, dari ACPI table | Sangat tinggi | Butuh parsing ACPI table tambahan (lihat §5) |
| **TSC** | `rdtsc` instruction | Sangat tinggi, per-core | Perlu kalibrasi frekuensi dulu (lewat PIT/HPET) |

Pola implementasi mirip LAPIC yang sudah kamu punya:
```rust
const APIC_LVT_TIMER: u32 = 0x320;
const APIC_TIMER_INITCNT: u32 = 0x380;
const APIC_TIMER_CURCNT: u32 = 0x390;
const APIC_TIMER_DIVIDE: u32 = 0x3E0;

pub unsafe fn init_lapic_timer(lapic: &LocalApic, vector: u8) {
    unsafe {
        lapic.write(APIC_TIMER_DIVIDE, 0x3);       // divide by 16
        lapic.write(APIC_LVT_TIMER, vector as u32 | (1 << 17)); // periodic mode
        lapic.write(APIC_TIMER_INITCNT, 10_000_000); // kalibrasi belakangan
    }
}
```
Daftarkan vector-nya di IDT sama seperti vector 44 untuk xHCI.

---

## 2. Serial Port (UART 16550)

Driver termudah di seluruh OS dev, sangat berguna untuk **debug logging**
sebelum framebuffer/keyboard siap.

- Akses lewat port I/O (`0x3F8` untuk COM1), bukan MMIO — pola `outb`/`inb`
  yang sudah ada di `io.rs` langsung bisa dipakai.
- Tidak butuh PCI scan (device legacy fixed-address) maupun interrupt
  (polling sudah cukup untuk logging).

```rust
const COM1: u16 = 0x3F8;

pub unsafe fn init_serial() {
    unsafe {
        outb(COM1 + 1, 0x00); // disable interrupt dulu
        outb(COM1 + 3, 0x80); // enable DLAB (set baud rate divisor)
        outb(COM1 + 0, 0x03); // divisor low byte (38400 baud)
        outb(COM1 + 1, 0x00); // divisor high byte
        outb(COM1 + 3, 0x03); // 8 bit, no parity, 1 stop bit
        outb(COM1 + 2, 0xC7); // enable FIFO
        outb(COM1 + 4, 0x0B); // IRQ enable, RTS/DSR set
    }
}

pub unsafe fn serial_write_byte(b: u8) {
    unsafe {
        while inb(COM1 + 5) & 0x20 == 0 {} // tunggu transmit buffer kosong
        outb(COM1, b);
    }
}
```

---

## 3. RTC (Real-Time Clock)

Untuk baca tanggal/jam wall-clock — port I/O `0x70`/`0x71` (CMOS), sama
sederhananya dengan PIT. Perlu handle **BCD encoding** (kebanyakan RTC
menyimpan angka dalam format BCD, bukan biner murni) dan cek Update-In-
Progress flag supaya tidak baca data yang sedang berubah.

---

## 4. Storage Driver

Ini lapisan yang biasanya paling banyak makan waktu setelah USB. Tiga jalur
umum:

### 4.1 AHCI (SATA) — paling umum di hardware modern/VM
- Ditemukan lewat PCI scan sama seperti xHCI: **class `0x01`, subclass
  `0x06`** (Mass Storage, SATA), prog_if `0x01` (AHCI).
- Pola sama persis dengan xHCI: enable device → baca BAR5 (ABAR) → map MMIO
  → reset controller (HBA_GHC register) → enable AHCI mode → enumerasi
  port aktif (`PxSSTS`, `PxSIG`) → setup Command List + FIS Receive Area per
  port (butuh alokasi memory align seperti DCBAA/Command Ring xHCI-mu) →
  kirim command (READ DMA EXT / WRITE DMA EXT) lewat Command Table + PRDT.
- Interrupt-nya bisa MSI/MSI-X (pola setup **identik** dengan
  `setup_msix` yang sudah kamu tulis).

### 4.2 NVMe — SSD modern, lebih cepat tapi struktur lebih rumit
- PCI **class `0x01`, subclass `0x08`** (Non-Volatile Memory Controller).
- Konsepnya mirip xHCI: **Submission Queue** dan **Completion Queue**
  menggantikan Command Ring/Event Ring xHCI, tapi prinsipnya sama (ring
  buffer + doorbell + polling completion).
- Command Set Identify → baca namespace → baca/tulis block via NVM command
  (Read/Write opcode 0x02/0x01).

### 4.3 USB Mass Storage — kalau storage-nya lewat USB
Kalau xHCI driver sudah jalan, ini paling murah untuk ditambahkan karena
transport layer-nya sudah ada:
- Device class USB `0x08` (Mass Storage), biasanya subclass `0x06` (SCSI
  transparent) + protocol `0x50` (Bulk-Only Transport/BOT).
- Butuh 2 endpoint **Bulk** (IN & OUT), bukan Interrupt seperti keyboard —
  pola `configure_endpoint` yang sudah ada bisa dipakai, tinggal cari
  endpoint dengan `attributes & 0x3 == 0x2` (Bulk).
- Protokol di atasnya: CBW (Command Block Wrapper) → kirim command SCSI
  (READ(10)/WRITE(10)) → data stage → CSW (Command Status Wrapper).

**Rekomendasi urutan belajar**: AHCI dulu (paling sederhana & umum di
QEMU/VirtualBox untuk testing), baru NVMe atau USB Mass Storage.

---

## 5. ACPI Table Parsing (dependency untuk banyak driver lain)

Kamu sudah punya `rsdp.rs` yang baca RSDP dari Limine. Langkah lanjutannya:

1. Dari RSDP, ambil `rsdt_address` (32-bit) atau `xsdt_address` (64-bit,
   revisi 2+).
2. RSDT/XSDT berisi **array pointer** ke table lain (`MADT`, `MCFG`, `FADT`,
   `HPET`, dll) — tiap table diawali header dengan `signature` 4 karakter.
3. Table yang paling berguna duluan:
   - **MADT**: daftar LAPIC & IOAPIC yang sebenarnya (kamu sekarang masih
     hardcode `0xFEC00000` untuk IOAPIC — MADT kasih alamat yang benar &
     bisa multiple IOAPIC).
   - **MCFG**: base address utuk PCIe Enhanced Configuration Access
     (menggantikan I/O port `0xCF8`/`0xCFC` yang lambat, dan wajib untuk
     akses offset config space > 256 byte).
   - **HPET**: base address timer presisi tinggi.

---

## 6. Framebuffer / Display (peningkatan dari Limine framebuffer)

Kamu sudah pakai `FramebufferRequest` dari Limine untuk mode awal (linear
framebuffer, sudah cukup untuk kebanyakan hobby OS). Driver GPU asli (Intel
i915, virtio-gpu) jauh lebih kompleks dan biasanya **tidak perlu** kecuali
mau akselerasi 2D/3D — untuk OS hobby, framebuffer Limine + software
rendering (yang sudah kamu pakai di `write_pixel`) sudah memadai.

Kalau tetap mau coba: **virtio-gpu** paling ramah untuk dipelajari (dipakai
QEMU), jauh lebih sederhana dari driver GPU fisik.

---

## 7. Network (kalau dibutuhkan)

- **virtio-net**: paling gampang untuk lingkungan virtual (QEMU) — PCI
  device dengan vendor ID `0x1AF4`. Struktur virtqueue mirip ring buffer
  seperti xHCI (descriptor table + available ring + used ring).
- **Intel e1000**: umum di hardware fisik & emulasi VirtualBox/QEMU, pola
  PCI+MMIO+interrupt sama seperti driver lain di project ini (class
  `0x02`, subclass `0x00`).

---

## 8. Pola Umum yang Berulang di Semua Driver Ini

Kalau diperhatikan, hampir semua driver hardware modern (xHCI-mu jadi
contoh terbaik) mengikuti skeleton yang sama:

```
1. Temukan device      → PCI scan (class/subclass/vendor tertentu), atau alamat fixed (legacy port I/O)
2. Enable device        → set Command register (Memory Space + Bus Master)
3. Map resource         → BAR → MMIO (map_mmio_page), atau port I/O langsung
4. Setup interrupt      → cari capability MSI/MSI-X → daftar IDT vector → EOI di handler
5. Reset & init         → urutan register write sesuai datasheet/spec device
6. Setup ring/queue     → alokasi struktur align, kasih tahu device lewat register base-address
7. Enable & jalankan    → set bit "run", tunggu status "ready"
8. Command/data loop    → submit request → ring doorbell/notify → tunggu completion via interrupt/polling
```

Begitu kamu paham skeleton ini dari xHCI, driver lain (AHCI, NVMe, virtio-*)
sebenarnya "isi ulang" pola yang sama dengan register dan format data
berbeda — bukan konsep yang benar-benar baru.

---

## 9. Rekomendasi Urutan Implementasi

1. **Serial (UART)** — cepat, buat debug logging sebelum lanjut yang lain
2. **PIT/LAPIC Timer** — dibutuhkan banyak driver lain untuk timeout yang akurat
3. **RTC** — opsional, gampang, kalau butuh wall-clock
4. **ACPI table parsing (MADT/MCFG)** — supaya IOAPIC address & PCI config akses tidak hardcode
5. **AHCI (storage)** — driver "besar" kedua setelah xHCI, pola sangat mirip
6. **Filesystem (FAT32/ext2 read-only dulu)** — lapisan di atas storage driver
7. **Network (virtio-net)** — kalau butuh, biasanya prioritas lebih rendah untuk hobby OS

---

*Semua driver di atas bisa reuse infrastruktur yang sudah ada di project:
`pci::scan_pci_bus`-style enumeration, `memory::paging::map_mmio_page` untuk
MMIO, dan pola interrupt stub + IDT + EOI dari `interrupt.rs`/`idt.rs`/
`apic.rs`. Kalau mau, saya bisa bikinkan panduan detail per driver (misal
AHCI) dengan level kedalaman yang sama seperti panduan xHCI/HID.*
