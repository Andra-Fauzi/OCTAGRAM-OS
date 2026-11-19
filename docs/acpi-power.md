# ACPI dan Manajemen Daya

Dokumentasi ini menjelaskan implementasi ACPI (Advanced Configuration and Power Interface)
dan manajemen daya di OCTAGRAM-OS. Modul terkait berada di `kernel/src/rsdp.rs` dan
`kernel/src/drivers/acpi/` (`mod.rs`, `aml.rs`, `power.rs`), serta perintah shell `shutdown`
dan `reboot` di `kernel/src/shell.rs`.

---

## 1. Penemuan Tabel ACPI (Table Discovery)

OCTAGRAM-OS menemukan tabel deskripsi ACPI melalui Root System Description Pointer (RSDP):
1. **Pencarian RSDP**:
   - Jika bootloader (Limine) menyediakan pointer RSDP melalui Limine RSDP Request, kernel menggunakannya langsung.
   - Fallback manual memindai Extended BIOS Data Area (EBDA) pada `0x80000..0x9FFFF` atau area memori BIOS utama pada `0xE0000..0xFFFFF` untuk mencari signature 8-byte `"RSD PTR "`.
2. **Checksum & Versi**:
   - Memvalidasi checksum 20 byte pertama (ACPI 1.0). Jika versi $\ge 2$ (ACPI 2.0+), 16 byte extended checksum juga divalidasi.
3. **Traversal Tabel (RSDT / XSDT)**:
   - Jika XSDT (Extended System Description Table) tersedia, kernel membaca pointer 64-bit fisik ke tabel-tabel sekunder.
   - Jika hanya RSDT yang tersedia, pointer 32-bit digunakan.
4. **Tabel yang Diproses**:
   - **FADT (Fixed ACPI Description Table)**: Menyediakan alamat register daya PM1a/PM1b, SMI command port, bit ACPI enable, pointer DSDT, serta register reset sistem (`RESET_REG`).
   - **DSDT (Differentiated System Description Table)**: Berisi kode AML utama platform.
   - **SSDT (Secondary System Description Table)**: Berisi definisi AML tambahan dari firmware OEM.

---

## 2. Parser dan Interpreter AML (`drivers/acpi/aml.rs`)

Untuk membaca definisi perangkat dan mengeksekusi metode daya pada firmware modern,
OCTAGRAM-OS memiliki parser dan interpreter bytecode AML (ACPI Machine Language) sendiri:

- **Namespace Hirarkis**: Membangun pohon namespace ACPI (seperti `\_SB_`, `\_PR_`, `\_GPE`).
- **Objek yang Didukung**: `Scope`, `Device`, `Method`, `PowerResource`, `Name`, `Integer`, `String`, `Package`, `Buffer`, `FieldUnit`, `OpRegion`.
- **Eksekusi Metode**: Mampu mengevaluasi ekspresi aritmatika AML, percabangan kondisional (`If`/`Else`), pembacaan/penulisan OpRegion (`SystemMemory`, `SystemIO`), dan pemanggilan metode dengan argumen $Arg_0..Arg_6$ serta variabel lokal $Local_0..Local_7$.

---

## 3. Manajemen Daya Perangkat PCI (`drivers/acpi/power.rs`)

Sistem operasi berbasis bare-metal sering mengalami kegagalan deteksi perangkat PCI jika hanya mengandalkan bit power state di PCI Power Management Capability. Pada motherboard laptop modern (misalnya SoC laptop dengan WiFi PCIe), firmware platform mematikan clock referensi atau power rail ketika perangkat berada dalam status D3.

Modul `power.rs` menyelesaikan masalah ini dengan mengaktifkan power resource ACPI:
1. **Penelusuran Host Bridge**: Menemukan PCI host bridge (`PNP0A03` atau PCIe `PNP0A08`) yang mencakup bus PCI target.
2. **Pencocokan Alamat (`_ADR`)**: Menemukan node `Device` yang memiliki nomor fungsi dan slot yang cocok.
3. **Evaluasi Paket `_PR0`**:
   - Membaca daftar power resource pada paket `_PR0` (Power Resources for D0).
   - Menelusuri seluruh dependensi power resource pada perangkat dan node induknya.
4. **Pemeriksaan Status (`_STA`) & Pengaktifan (`_ON`)**:
   - Memeriksa apakah power resource sudah aktif via `_STA`.
   - Jika belum aktif, mengeksekusi method `_ON` agar power rail dan clock dinyalakan oleh platform.
5. **Eksekusi `_PS0`**:
   - Menjalankan method `_PS0` pada perangkat target bila didefinisikan.
6. **Caching State**:
   - Pohon namespace diurai sekali (`Once<Namespace>`).
   - Hasil evaluasi disimpan (`PowerReport`) agar pengulangan inisialisasi driver tidak mengevaluasi ulang method berulang kali.

Fitur ini krusial untuk perangkat berdaya dinamis seperti Wi-Fi Qualcomm Atheros QCA9377.

---

## 4. Mekanisme Power-Off Sistem (ACPI S5 Shutdown)

Perintah shell `shutdown` menginisialisasi proses pemadaman daya ACPI status tidur S5 (Soft Off):

1. **Penentuan Nilai Sleep Type**:
   - Membaca objek `\_S5` dari DSDT/SSDT melalui interpreter AML.
   - Objek `\_S5` mengembalikan paket yang berisi nilai numerik `SLP_TYPa` dan `SLP_TYPb`.
2. **Pengaktifan Mode ACPI**:
   - Memeriksa bit `SCI_EN` pada register kontrol PM1a.
   - Jika `SCI_EN` belum aktif, kernel menulis nilai `ACPI_ENABLE` ke port I/O `SMI_CMD` (offset 48 pada FADT) dan menunggu hingga hardware mengalihkan kendali daya ke ACPI.
3. **Pemicuan Status S5**:
   - Membentuk kata kontrol:
     $$\text{PM1\_CNT} = (\text{SLP\_TYP} \ll 10) \mid \text{SLP\_EN}$$
     di mana `SLP_EN` adalah bit 13 (`1 << 13`).
   - Menulis nilai ke port I/O register `PM1a_CNT` dan register sekunder `PM1b_CNT` (jika ada).
   - Melakukan loop `spin_loop` dan `hlt` hingga firmware mematikan unit suplai daya (PSU).

---

## 5. Mekanisme Reset Sistem (Reboot)

Perintah shell `reboot` mengimplementasikan strategi reset tiga lapis:

### Lapis 1: Register Reset FADT (`RESET_REG`)
- FADT spesifikasi ACPI 2.0+ menyediakan Generic Address Structure (GAS) untuk reset:
  - **System I/O (`GAS_SYSTEM_IO`)**: Menulis `RESET_VALUE` ke port I/O yang ditentukan oleh `RESET_REG`.
  - **System Memory (`GAS_SYSTEM_MEMORY`)**: Memetakan alamat fisik ke memori virtual kernel via paging dan menulis `RESET_VALUE`.
- Jika didukung oleh firmware, CPU dan chipset akan langsung melakukan reset hardware.

### Lapis 2: Fallback Controller Keyboard PS/2 (8042)
- Jika tabel FADT tidak memiliki `RESET_REG` (atau operasi gagal):
  - Mengirim pulsa reset jalur CPU baris perintah dengan menulis byte `0xFE` ke register perintah keyboard controller (I/O port `0x64`).

### Lapis 3: Fallback Triple Fault
- Jika kontroler 8042 tidak merespons (misalnya pada sistem UEFI tanpa legacy controller):
  - Memuat IDT kosong (limit 0) menggunakan instruksi `lidt`.
  - Memicu interrupt software (`int 3` atau `ud2`). Karena IDT tidak valid, CPU memicu Double Fault, yang kemudian gagal lagi dan memicu Triple Fault, memaksa sirkuit hardware motherboard melakukan reset instan.

---

## 6. Perintah Shell Terkait

- `shutdown`: Menjalankan siklus ACPI S5 shutdown.
- `reboot`: Menjalankan siklus reset perangkat keras.

---

## 7. Batasan

- **Sleep State Lain**: Belum mendukung status tidur S1 (Standby), S3 (Suspend-to-RAM), atau S4 (Hibernate).
- **Thermal & Fan Control**: Belum mengimplementasikan penanganan zona termal ACPI (`_TZ_`) atau kontrol kipas dinamis.
- **ACPI Hardware Reduced**: Platform hardware-reduced ACPI modern yang tidak menyediakan blok PM1 legacy belum didukung penuh.
