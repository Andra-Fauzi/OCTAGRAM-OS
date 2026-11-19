# Manajemen Paket dan Aplikasi

Dokumentasi ini menjelaskan sistem manajemen paket native (`pkg`), format metadata,
direktori aplikasi (`/apps`), serta app launcher di OCTAGRAM-OS.
Implementasi berada di `kernel/src/shell.rs` dan berinteraksi langsung dengan VFS (`kernel/src/fs/`).

---

## 1. Konsep dan Arsitektur

OCTAGRAM-OS menyediakan sistem instalasi dan pengelolaan paket native untuk menginstal,
memperbarui, memeriksa, dan menghapus biner aplikasi pengguna.

Sistem ini terdiri dari tiga komponen utama pada hierarki filesystem:
1. **`/apps/`**: Direktori tempat biner program yang terpasang disimpan dan dapat dieksekusi langsung.
2. **`/var/lib/octagram/packages/*.pkg`**: Berkas metadata per paket.
3. **`/var/lib/octagram/packages/registry.txt`**: Basis data registry sentral daftar seluruh paket yang terpasang di sistem.

Nama paket yang valid (`valid_package_name`) harus terdiri dari 1–64 karakter ASCII alfanumerik, tanda hubung (`-`), atau garis bawah (`_`).

---

## 2. Format Berkas Metadata

### Metadata Paket: `/var/lib/octagram/packages/<name>.pkg`
Setiap paket yang terpasang memiliki berkas teks berformat pasangan kunci-nilai:
```text
name=<name>
version=<version>
path=/apps/<name>
size=<ukuran_dalam_byte>
```

Contoh `/var/lib/octagram/packages/editor.pkg`:
```text
name=editor
version=1.2.0
path=/apps/editor
size=48120
```

### Registry Paket: `/var/lib/octagram/packages/registry.txt`
Berkas registry menggabungkan daftar seluruh paket terpasang dalam format baris dengan pemisah pipa (`|`):
```text
name|version|path|size
```
Contoh:
```text
editor|1.2.0|/apps/editor|48120
calculator|0.9|/apps/calculator|16384
tcc|0.9.27|/apps/tcc|289540
```

---

## 3. Perintah Shell `pkg`

Perintah bawaan `pkg` di shell mendukung enam subperintah:

### `pkg list`
Membaca `/var/lib/octagram/packages/registry.txt` dan mencetak seluruh paket yang terdaftar beserta versi, path biner, dan ukurannya.
- Jika berkas registry kosong atau belum ada paket: mencetak `no packages installed`.

### `pkg info <name>`
Membaca dan menampilkan metadata lengkap dari berkas `/var/lib/octagram/packages/<name>.pkg`.
- Mengembalikan pesan error jika nama tidak valid atau paket tidak ditemukan.

### `pkg add <name> <source> [version]`
Menginstal biner baru ke dalam sistem:
1. Memvalidasi nama paket (`valid_package_name`) dan memastikan sumber berkas (`source`) dapat ditemukan di VFS.
2. Membaca biner sumber hingga batas ukuran `MAX_TOOLCHAIN_FILE` (2 MiB).
3. Memastikan `/apps/<name>` belum ada (jika sudah ada, menghasilkan error `AlreadyExists`; gunakan `pkg update` untuk memperbarui).
4. Menulis berkas biner ke `/apps/<name>` dan memberikan izin eksekusi (`0o755`).
5. Membuat berkas metadata `/var/lib/octagram/packages/<name>.pkg`.
6. Memperbarui berkas registry sentral `/var/lib/octagram/packages/registry.txt`.

### `pkg update <name> <source> [version]`
Memperbarui paket yang sudah terpasang atau menginstal ulang:
- Jika berkas biner lama atau metadata lama sudah ada, biner dan metadata lama dihapus terlebih dahulu.
- Menuliskan biner baru ke `/apps/<name>` dengan izin `0o755`.
- Memperbarui metadata `.pkg` dan entri di `registry.txt`.

### `pkg remove <name>`
Menghapus paket dari sistem:
1. Membaca metadata `/var/lib/octagram/packages/<name>.pkg` untuk mengetahui path instalasi biner (biasanya `/apps/<name>`).
2. Menghapus berkas biner dari direktori aplikasi jika masih ada.
3. Menghapus berkas metadata `.pkg`.
4. Menghapus entri paket dari `registry.txt`.

### `pkg verify <name>`
Memverifikasi integritas instalasi paket:
1. Membaca berkas metadata `/var/lib/octagram/packages/<name>.pkg`.
2. Mengurai field `path` dan ukuran yang diharapkan (`size`).
3. Memeriksa keberadaan berkas biner di path tersebut dan membandingkan ukuran fisiknya.
4. Menampilkan `pkg verify: <name> OK size=<size>` jika cocok, atau melaporkan error jika berkas hilang atau ukuran tidak sesuai.

---

## 4. App Launcher: Perintah `apps`

Perintah `apps` di shell bertindak sebagai peluncur dan pemeriksa aplikasi terpasang:
- Membaca isi direktori `/apps` melalui VFS (`vfs.read_dir("/apps")`).
- Mencetak seluruh path aplikasi yang tersedia, misalnya:
  ```text
  /apps/editor
  /apps/calculator
  /apps/tcc
  ```
- Jika direktori kosong, mencetak `no apps installed`.

Program di dalam direktori `/apps` dapat dijalankan langsung dengan menyebutkan path absolutnya (misal `run /apps/tcc`) atau dengan menambahkan `/apps` ke variabel lingkungan `PATH` via `export PATH=/bin:/apps`.

---

## 5. Alur Kerja Praktis

### Mengompilasi dan Menginstal Paket dari Source
Pengguna dapat mengompilasi program C secara native di OCTAGRAM-OS menggunakan `tcc` atau `cc`, lalu mendaftarkannya sebagai paket:

```text
# 1. Kompilasi program
tcc -o /tmp/halo.elf /mnt/halo.c

# 2. Pasang sebagai paket native
pkg add halo /tmp/halo.elf 1.0.0

# 3. Verifikasi instalasi
pkg verify halo
pkg info halo

# 4. Jalankan aplikasi
/apps/halo
```

### Menyimpan Paket di Media Persisten
Karena root sistem secara default adalah RAMFS in-memory, semua paket di `/apps` dan `/var/lib/octagram/packages` akan hilang saat sistem reboot kecuali dipertahankan.
Gunakan perintah `persist <mount>` setelah memasang partisi persisten (FAT32 atau ext2) agar konfigurasi dan aplikasi tersimpan ke media penyimpanan fisik.

---

## 6. Batasan Saat Ini

- **Tanpa Kompresi / Arsip**: Paket saat ini berupa berkas biner tunggal, belum berupa arsip tar/gz yang memuat banyak berkas (seperti aset, dokumen, konfigurasi default).
- **Tanpa Resolusi Dependensi**: Belum ada pelacakan pustaka dinamis atau dependensi otomatis antar-paket.
- **Repository Remote**: Belum ada pengunduhan paket otomatis dari repositori HTTP; instalasi dilakukan dari berkas lokal yang sudah ada di VFS.
