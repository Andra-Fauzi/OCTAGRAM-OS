# Instalasi langsung ke hard disk (UEFI + EXT2)

OCTAGRAM-OS dapat dipasang ke disk internal SATA yang diekspos firmware sebagai
AHCI. Layout instalasi adalah:

| Partisi | Ukuran | Format | Fungsi |
| --- | ---: | --- | --- |
| 1 | 512 MiB | FAT32 / EFI System Partition | Limine UEFI dan kernel |
| 2 | Sisa disk | EXT2 | Root filesystem permanen `/` |

Kernel mencari filesystem EXT2 pada perangkat `sataN` yang memiliki marker
`/etc/octagram-root`. Perangkat USB tidak pernah dipilih otomatis sebagai root.
Jika marker tidak ditemukan, sistem tetap boot menggunakan RAMFS rescue dan
mencetak peringatan bahwa root tidak persisten.

## 1. Persiapan build di Linux/WSL

Ikuti [building.md](building.md) untuk memasang Rust nightly dan dependensi.
Dari root repository:

```sh
make -C user/rust make
make -C user
make -C user dynamic
make -C kernel KARCH=x86_64
make limine/limine
```

Hasil yang dibutuhkan:
- `kernel/kernel`
- `limine/BOOTX64.EFI`
- `limine.conf`

## 2. Identifikasi disk dengan teliti

Colok/siapkan disk internal target, lalu:

```sh
lsblk -o NAME,SIZE,MODEL,TYPE,MOUNTPOINTS
```

Pilih **whole disk** seperti `/dev/sda`, bukan partisi seperti `/dev/sda1`.
Installer akan menghapus seluruh tabel partisi dan data pada disk target. Backup
semua data penting terlebih dahulu. Jangan jalankan pada disk Windows atau disk
yang berisi data yang ingin dipertahankan.

## 3. Jalankan installer

```sh
sudo tools/install-disk.sh /dev/sdX
```

Ganti `/dev/sdX` dengan disk yang benar. Script meminta konfirmasi dengan
mengetik persis `ERASE-/dev/sdX`. Script membuat GPT, ESP FAT32, EXT2 root,
menyalin kernel dan Limine UEFI, serta menulis marker root.

Untuk disk NVMe, jangan gunakan installer ini: kernel saat ini belum memiliki
driver NVMe. Dukungan SATA/AHCI diperlukan agar root terdeteksi sebagai `sataN`.

## 4. Boot dari hard disk

1. Matikan laptop setelah installer selesai.
2. Lepaskan flashdisk installer (jika digunakan).
3. Buka firmware setup / boot menu.
4. Pastikan mode UEFI aktif dan pilih disk OCTAGRAM.
5. Boot OCTAGRAM-OS.

Di log awal kernel, instalasi yang benar harus menampilkan:

```text
[rootfs] persistent EXT2 root selected from sata0 ...
```

Jika muncul `using volatile RAMFS`, disk tidak ditemukan, controller tidak
berjalan dalam AHCI, marker root tidak ada, atau driver AHCI gagal. Dalam kondisi
itu sistem belum berjalan dari root permanen.

## 5. Verifikasi dari shell OCTAGRAM

```text
df
devices
touch /home/octagram/persistence-test.txt
echo persistent > /home/octagram/persistence-test.txt
```

Restart komputer, lalu jalankan:

```text
cat /home/octagram/persistence-test.txt
```

Jika isinya tetap ada setelah reboot dan log menunjukkan persistent EXT2 root,
root filesystem berjalan langsung dari hard disk. `/tmp` juga berada di EXT2
untuk saat ini; tmpfs terpisah belum disediakan.

## Batasan penting

- Driver disk internal saat ini mendukung SATA/AHCI. NVMe belum didukung.
- Root EXT2 belum memiliki journaling; shutdown paksa saat penulisan dapat
  merusak filesystem. Lakukan shutdown dengan benar dan simpan backup.
- Kernel dan bootloader berada di ESP FAT32; file sistem, home, konfigurasi,
  aplikasi, dan data berada di EXT2 root.
- Ini installer UEFI. Boot BIOS/Legacy tidak dikonfigurasi oleh script ini.
- Installer hanya menyiapkan struktur root dan marker; kernel akan mengisi
  program bawaan/header yang belum ada saat boot pertama. File yang sudah ada
  tidak ditimpa saat boot agar data pengguna tidak hilang.
