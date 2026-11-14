# Storage dan filesystem

## Lapisan block device

Driver AHCI SATA dan USB Mass Storage mendaftarkan perangkat melalui
`storage::block`. Trait block device memakai sektor 512-byte dan operasi baca
serta tulis sinkron. Registry memberi nama seperti `sata0` atau `usb0`.

## Mount dari shell

VFS dimulai dengan RAMFS pada `/`. FAT32 tetap menjadi tipe default:

```text
mkdir /mnt
devices
mount usb0 /mnt
ls /mnt
cat /mnt/README.TXT
touch /mnt/new.txt
umount /mnt
```

Volume yang ditangani adalah FAT32 MBR partition atau superfloppy. Implementasi
mendukung baca/tulis file, truncate, pembuatan file/direktori, penghapusan,
nama panjang, FAT mirroring, dan FSInfo yang valid. GPT, exFAT, NTFS, dan UASP
belum didukung.

Ext2 dapat dipasang eksplisit dari primary MBR partition bertipe `0x83` atau
superfloppy:

```text
mount sata0 /mnt ext2
ls /mnt
touch /mnt/new.txt
umount /mnt
```

Ext2 mendukung baca/tulis, truncate, direktori, symbolic link, permission,
direct serta indirect block, dan block size 1/2/4 KiB. Filesystem ber-journal
(ext3), extents, 64-bit block addressing, dan fitur ext4 yang tidak dikenal
ditolak saat mount. Ext2 tidak memiliki journal; kegagalan I/O atau listrik
mati di tengah mutasi dapat merusak metadata. Selalu unmount sebelum melepas
media dan uji penulisan pada image atau media kosong.

FAT32 tidak memiliki journaling. Kegagalan I/O atau listrik mati di tengah
mutasi dapat meninggalkan FAT dan direktori tidak konsisten. Selalu unmount
sebelum melepas perangkat. Jangan menguji fitur tulis pada media berisi data
penting; gunakan image atau media uji.

## Kontrak VFS

Implementasi filesystem memenuhi trait `FileSystem` di `fs/mod.rs`.
`normalize_path` meminta path absolute dan menolak keluar dari root. Mount
router menggunakan mount terpanjang dengan batas komponen path. Root RAMFS
bersifat volatil, sedangkan mount FAT32 memiliki storage persisten.

`read` mengembalikan jumlah byte aktual, `write` menerima offset, `truncate`
mengubah panjang file, `create_file`/`create_dir` membuat node, `remove`
menghapus file atau direktori kosong, dan `read_dir` mengembalikan child
langsung. Error harus dikembalikan sebagai `FsError` alih-alih disamarkan.

RAMFS mendukung symbolic link melalui `ln -s <target> <link>`. Akses file dan
direktori mengikuti link, termasuk target relatif dan target yang melewati mount
lain; resolver membatasi rantai hingga 40 link. `rm` menghapus link-nya, bukan
target. FAT32 tidak memiliki symbolic link dan tidak mendukung pembuatan link,
tetapi link di RAMFS dapat menunjuk ke program pada volume FAT32:

```text
mkdir /bin
ln -s /mnt/program.elf /bin/program
run /bin/program
```

Gunakan path link tersebut dari direktori mana pun. Link di RAMFS hilang saat
reboot; target file pada FAT32 tetap persisten.

File dan direktori RAMFS memiliki permission Unix sederhana. File baru ber-mode
`0644`, direktori `0755`; gunakan `chmod +x program.elf` agar dapat dijalankan,
atau `chmod 755 program.elf` untuk mengatur seluruh bit permission. FAT32 tidak
menyimpan bit executable Unix: file dengan ekstensi `.elf` dianggap executable,
sedangkan `chmod` pada volume FAT32 tidak didukung.

Program user mengakses operasi tersebut melalui syscall descriptor/path;
lihat [ABI syscall](syscall-abi.md).
