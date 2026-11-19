# Kernel Logging dan Diagnostik

Dokumentasi ini menjelaskan subsistem logging kernel, struktur buffer sirkular,
saluran output, perintah `savelog`, serta sarana diagnostik sistem di OCTAGRAM-OS.
Implementasi berada di `kernel/src/klog.rs`, `kernel/src/drivers/serial.rs`, dan
`kernel/src/shell.rs`.

---

## 1. Arsitektur Buffer Log Kernel (`kernel/src/klog.rs`)

Semua teks yang dicetak oleh kernel melalui makro `println!` atau `print!` disalin
ke dalam buffer memori in-memory kernel. Buffer ini dirancang khusus untuk keperluan
bring-up hardware dan diagnosa driver yang sering menghasilkan log dalam jumlah besar.

### Model Buffer Ganda (Dual-Stage Buffer)

Struktur `LogBuffer` membagi kapasitas log menjadi dua segmen:

```
+------------------------------------+------------------------------------+
|       HEAD (Boot Log)              |       TAIL (Circular Ring)         |
|         96 KiB Tetap               |            160 KiB                 |
+------------------------------------+------------------------------------+
```

1. **Segmen `HEAD` (`HEAD_CAPACITY` = 96 KiB)**:
   - Menyimpan seluruh log boot dari awal kernel hidup.
   - Bersifat permanen dan tidak pernah ditimpa (append-only sampai penuh).
   - Menjamin log inisialisasi awal (deteksi CPU, memori, ACPI, PCI, dan probe awal perangkat) tetap utuh dan tidak terdorong keluar oleh driver yang mengalami pengulangan pesan error di latar belakang.
2. **Segmen `TAIL` (`TAIL_CAPACITY` = 160 KiB)**:
   - Bekerja sebagai circular ring buffer untuk pesan-pesan log operasional runtime terbaru.
   - Ketika kapasitas penuh, pesan baru menimpa pesan terlama pada ring, dan penghitung `dropped` bertambah.
3. **Ekstraksi Terpadu (`snapshot()` / `to_vec()`)**:
   - Menghasilkan vektor gabungan: bagian `HEAD`, disusul baris penanda jika ada data yang terlewat:
     ```text
     [... N bytes of log omitted ...]
     ```
     diikuti oleh segmen `TAIL` dalam urutan kronologis yang benar.

### Karakteristik Bebas-Deadlock (Non-blocking)

- `klog::record(text)` menggunakan `LOG.try_lock()`.
- Jika spinlock sedang dipegang oleh tugas lain (misalnya saat shell sedang mengambil snapshot buffer untuk diekspor ke disk), pemanggilan log dari interrupt handler tidak akan memblokir atau menyebabkan deadlock. Teks hanya dilepaskan dari buffer log memori tanpa memengaruhi tampilan konsol layar.

---

## 2. Saluran Output Log (Output Sinks)

Pesan kernel disalurkan ke beberapa saluran:

1. **Serial Console COM1 (`drivers/serial.rs`)**:
   - Diinisialisasi di awal `kmain` sebelum subsistem lainnya (`serial::early_init()`).
   - Beroperasi pada port I/O standar `0x3F8` dengan baud rate 115200 8N1.
   - Memungkinkan logging ditangkap langsung oleh emulator (misal opsi QEMU `-serial stdio` atau `-serial file:qemu.log`) bahkan jika driver grafis atau konsol gagal menginisialisasi.
2. **Virtual Console Framebuffer (`terminal/`)**:
   - Menampilkan teks pada konsol aktif layar framebuffer menggunakan font bitmap bawaan.
3. **Memory Log Buffer (`klog.rs`)**:
   - Disimpan untuk inspeksi dan penyimpanan kemudian.

---

## 3. Menyimpan Log ke Berkas: Perintah `savelog`

Kernel log dapat disimpan ke berkas penyimpanan fisik (misalnya USB flash drive atau partisi hard disk) menggunakan perintah bawaan shell `savelog <path>`.

### Alur Eksekusi
1. Mengambil salinan snapshot lengkap buffer log via `klog::snapshot()`.
   - Pengambilan dilakukan sebelum operasi penulisan berkas dimulai agar lock mutex log tidak ditahan selama operasi I/O filesystem.
2. Membuka atau membuat berkas target pada path VFS yang diberikan (misal `/mnt/LOG.TXT`).
3. Mengosongkan berkas lama (`truncate(0)`) jika berkas sudah ada.
4. Menulis data log secara bertahap dalam blok 4 KiB.
5. Melaporkan jumlah byte yang berhasil ditulis ke layar.

### Contoh Skenario Penggunaan Praktis
Untuk mengambil log diagnosa dari laptop atau pengujian bare-metal:
```text
# 1. Pasang USB flash drive berformat FAT32
mount usb0 /mnt

# 2. Simpan log kernel
savelog /mnt/kernel_boot.log

# 3. Lepaskan drive sebelum mencabut media fisik
umount /mnt
```

---

## 4. Perintah Diagnostik Sistem Lainnya di Shell

Shell OCTAGRAM-OS menyediakan serangkaian perintah diagnostik bawaan:

### `sched`
Menampilkan status dan akuntansi runtime scheduler proses:
- Menampilkan tabel proses terjadwal: `PID`, `TID`, prioritas `NICE`, kebijakan penjadwalan (`OTHER`, `FIFO`, `RR`), prioritas RT (`RTPRI`), umur tunggu anti-starvasi (`AGE`), penggunaan waktu CPU (`CPUms`), persentase CPU (`CPU%`), waktu tunggu runnable (`WAITms`), serta status runnable/waiting/exited.
- Menampilkan bendera peringatan `WATCHDOG` bila task runnable mengalami kelaparan eksekusi (`age >= 78` atau `wait >= 500 ms`).

### `devices`
Mencetak seluruh block device penyimpanan yang terdeteksi dan terdaftar di subsistem `storage::block`:
- Menampilkan nama perangkat (misal `sata0`, `sata1`, `usb0`), kapasitas blok, dan ukuran sektor.

### `lsdev`
Menampilkan seluruh perangkat keras bus yang terdeteksi di sistem (khususnya perangkat PCI yang ditemukan pada pemindaian bus):
- Alamat bus/device/function (BDF), Vendor ID, Device ID, kelas perangkat, dan status binding dengan driver kernel.

### `drivers`
Menampilkan daftar seluruh driver perangkat yang terdaftar di registry model perangkat kernel (`drivers/device_model.rs`) beserta tipe perangkat yang didukung.

---

## 5. Ringkasan Batasan

- **Tanpa Syscall `dmesg` / `syslog` Userspace**: Saat ini buffer log kernel hanya dapat diakses melalui fungsi kernel internal dan perintah bawaan shell `savelog`. Program userspace belum memiliki syscall khusus untuk membaca buffer log kernel secara terisolasi.
- **Tingkat Filter Dinamis**: Filter log verbosity (misalnya pemisahan level trace/debug/info/warn) diatur statis melalui kompilasi atau konstanta driver, belum dapat diubah saat runtime via sysctl.
