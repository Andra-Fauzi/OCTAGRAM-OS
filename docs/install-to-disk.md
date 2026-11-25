# Instalasi OCTAGRAM-OS ke hard disk

OCTAGRAM-OS menyediakan dua jalur instalasi untuk perangkat x86_64 dengan
controller SATA yang berjalan dalam mode AHCI. Jalur yang disarankan adalah
menjalankan installer **dari dalam Live OCTAGRAM-OS**, langsung dari flashdisk.
Tidak perlu keluar ke Linux/WSL pada laptop target.

## A. Instalasi langsung dari Live OCTAGRAM-OS

### 1. Siapkan flashdisk Live

Build ISO dari host pengembangan mengikuti [building.md](building.md):

```sh
make -C user/rust make
make -C user
make -C user dynamic
make all
```

Tulis `OCTAGRAM-x86_64.iso` ke flashdisk sebagai media boot, lalu boot laptop
dalam mode UEFI. ISO Live harus dibangun dari branch yang memuat permintaan
Limine `ExecutableFileRequest`, `ModuleRequest`, dan
`module_path: boot():/EFI/BOOT/BOOTX64.EFI` agar kernel bisa menyalin kernel
yang sedang berjalan dan loader UEFI dari memori bootloader.

### 2. Lihat disk dari shell OCTAGRAM

Jalankan:

```text
installer
```

atau:

```text
installer list
```

Shell menampilkan nama device, kapasitas, dan status perlindungan. Untuk saat
ini hanya disk internal yang terdeteksi sebagai `sataN` (SATA/AHCI) yang dapat
dipilih. Perangkat USB tidak ditawarkan sebagai target. Disk yang menjalankan
root terpasang dan disk yang identitasnya cocok dengan sumber kernel Limine
ditolak sebagai target. Jika firmware/bootloader tidak memberi identitas disk
sumber, installer menolak operasi hapus demi mencegah flashdisk Live terhapus.

### 3. Instal ke disk internal

Contoh bila disk internal ditampilkan sebagai `sata0`:

```text
installer sata0 INSTALL-OCTAGRAM-ERASE
```

Perintah ini **menghapus seluruh tabel partisi dan data pada sata0**. Pastikan
nama device dan kapasitasnya benar sebelum menjalankan konfirmasi. Jangan pilih
disk Windows, disk data, atau flashdisk Live.

Installer akan membuat layout:

| Partisi | Ukuran | Format | Fungsi |
| --- | ---: | --- | --- |
| 1 | 512 MiB | FAT32 / EFI System Partition | Limine UEFI dan kernel |
| 2 | Sisa disk (dalam batas EXT2) | EXT2 | Root filesystem permanen `/` |

Installer menyalin `BOOTX64.EFI`, kernel ELF yang sedang berjalan, dan
`limine.conf` ke ESP. Ia juga membuat direktori sistem awal dan marker
`/etc/octagram-root` pada partisi root. Saat boot pertama, kernel mengisi
program bawaan, header, dan library yang belum ada tanpa menimpa file yang
sudah tersimpan.

Setelah pesan `OCTAGRAM-OS installation completed` muncul, lakukan shutdown
atau restart.

### 4. Boot dari hard disk

1. Restart laptop.
2. Lepaskan flashdisk Live.
3. Buka firmware boot menu.
4. Pastikan UEFI aktif dan pilih disk OCTAGRAM.

Di log kernel, instalasi sukses harus menampilkan pesan berikut:

```text
[rootfs] persistent EXT2 root selected from sata0p2
```

Jika muncul `using volatile RAMFS`, sistem belum menggunakan root permanen.
Periksa apakah disk terdeteksi sebagai `sataN`, controller berjalan dalam mode
AHCI, GPT dapat dibaca, dan marker root tersedia.

### 5. Verifikasi persistensi

Dari shell:

```text
df
devices
touch /home/octagram/persistence-test.txt
echo persistent > /home/octagram/persistence-test.txt
```

Restart tanpa flashdisk, lalu:

```text
cat /home/octagram/persistence-test.txt
```

Jika isi file tetap ada dan log menunjukkan persistent EXT2 root, root
filesystem dijalankan dari disk internal.

## B. Instalasi dari Linux/WSL (jalur pengembang)

Script host tetap tersedia bila ingin menyiapkan disk dari Linux:

```sh
sudo tools/install-disk.sh /dev/sdX
```

Script host juga bisa sekaligus memasang toolchain native Rust/Cargo:

```sh
sudo tools/install-disk.sh --toolchain build/toolchain/octagram-toolchain /dev/sdX
```

Direktori itu berasal dari `tools/bootstrap_rustc_octagram.sh` dan berisi
`bin/rustc`, `bin/cargo`, serta `lib/rustlib`. Isinya disalin ke `/toolchain`
pada root EXT2, dan `/toolchain/bin` sudah ada di PATH shell OCTAGRAM, jadi
setelah boot `rustc --version` dan `cargo build --offline` bisa langsung
dijalankan. Tanpa opsi ini root tetap dibuat dan `/toolchain/bin` kosong.

Toolchain sebaiknya dihasilkan lewat `tools/stage_octagram_toolchain.sh`, yang
menolak binary non-ELF atau `std` host yang hilang dan menulis
`/toolchain/toolchain.json` (commit Rust, versi LLVM, ukuran binary). Ringkasan
pemakaiannya juga tersedia di guest sebagai `/home/octagram/RUST.md`.

Ganti `/dev/sdX` dengan **whole disk** yang benar, bukan partisi. Script host
membuat ESP FAT32 dan root EXT2. Kernel mendeteksi marker root EXT2 tersebut.

Jalur host memerlukan Linux/WSL; jalur A tidak.

## Batasan saat ini

- **SATA/AHCI saja.** Driver NVMe belum tersedia. Banyak laptop modern memakai
  SSD NVMe; disk yang tidak muncul di perintah `devices` belum bisa dipasang
  oleh installer internal.
- **UEFI saja** untuk layout installer internal. Instalasi BIOS/Legacy belum
  disediakan.
- Root filesystem internal menggunakan EXT2 dengan blok 4 KiB, inode 128 byte,
  sparse superblocks, dan tanpa journaling. Shutdown paksa ketika ada penulisan
  dapat merusak metadata filesystem; lakukan shutdown dengan benar dan simpan
  backup.
- EFI System Partition tetap FAT32 sesuai persyaratan UEFI.
- EXT2 memakai 32-bit block numbering; installer membatasi ukuran partisi pada
  kapasitas yang didukung kernel dan menyisakan ruang disk berlebih tanpa partisi.
- Sebelum mencoba pada hardware, uji terlebih dahulu di QEMU dengan disk virtual
  kosong. Jangan gunakan disk yang berisi data penting.
