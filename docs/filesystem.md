# Filesystem dan VFS

`kernel/src/fs/mod.rs` berisi VFS, router mount, dan tipe node. Dua filesystem
in-memory tersedia: **RAMFS** untuk root dan **devfs** untuk `/dev`. Filesystem
on-disk (FAT32 dan ext2) berada di `kernel/src/storage/` dan didokumentasikan di
[storage.md](storage.md).

## Model VFS

```rust
pub trait FileSystem: Send {
    fn create_file / create_dir / remove / rename / read / write ...
    fn read_dir(&self, path) -> Vec<DirectoryEntry>
    fn metadata(&self, path) -> Metadata
    fn read_link / symlink ...
}
```

- `NodeType`: file, direktori, symlink, device, pipe, socket (sesuai yang
  dikembalikan tiap filesystem).
- `Metadata`: tipe node, ukuran, mode, dan waktu sebisanya.
- Path dinormalisasi (`normalize_path`) dan dilacak relatif terhadap cwd proses.
- Router memilih mount point **terpanjang** yang cocok, sehingga `/mnt/data`
  mengalahkan `/`.

Operasi node memakai path absolut di dalam VFS; proses hanya melihat cwd-nya
sendiri, sedangkan daftar mount bersifat global.

## Isi root saat boot

`fs::init()` membuat RAMFS volatil dan direktori awal:

```
/            root RAMFS
/dev         devfs
/bin         program bawaan
/lib         library
/lib/tcc     header dan libc untuk TinyCC
/share/octagram
/include     header C untuk program yang dikompilasi di dalam OS
/include/sys
/tmp         berkas sementara
```

Karena root adalah RAMFS, semua isi hilang saat reboot kecuali volume persisten
dipasang dan diisi ulang.

## devfs

`/dev` menyediakan:

- `/dev/tty0` … `/dev/tty3` — empat virtual console;
- `/dev/tty` — terminal aktif;
- standard stream (`stdin`/`stdout`/`stderr`) dipetakan ke TTY asal proses.

Pembacaan TTY memakai mode baris dengan echo, Backspace, Enter, dan Ctrl-D.
Karena input berasal dari interrupt keyboard/USB, loop tunggu di `read_tty` wajib
memanggil `drivers::poll_input()`.

## Descriptor dan batas operasi

- Descriptor 0/1/2 terhubung ke TTY shell asal program; `dup`/`dup2`/`dup3`
  bekerja pada tabel per proses.
- Tipe descriptor yang dikenal: file, direktori, pipe, TTY, TCP socket, UDP
  socket, eventfd, timerfd, epoll, dan signal/pipe internal.
- Deskriptor tertutup saat `execve` bila `FD_CLOEXEC` diset (lewat `fcntl`).
- Perintah shell yang membaca berkas memakai batas `MAX_FILE_OPERATION` = 64 KiB
  per operasi.
- `MAX_PATH` dan batas nama komponen mengikuti helper `resolve_user_path` di
  kernel dan `PATH_MAX` di libc.

## Mount dan persistensi

Dari shell:

```
devices                 # daftar block device
mount ahci0 /mnt        # pasang FAT32/ext2
df                      # daftar mount dan penggunaan
umount /mnt
persist /mnt            # siapkan /home/octagram di volume persisten
```

Program user juga bisa memakai path di bawah mount tersebut; tidak ada mount
namespace per proses.

## Batasan

- Tanpa journaling; kegagalan di tengah penulisan bisa meninggalkan struktur
  tidak konsisten.
- Tanpa permission enforcement penuh: mode disimpan, tetapi pemeriksaan akses
  bergantung pada filesystem (RAMFS menganggap operasi diizinkan).
- Tanpa hard link, tanpa `rename` lintas mount, tanpa quota, tanpa inotify.
- Tanpa filesystem terisolasi per proses atau per container.
- Root RAMFS volatil; belum ada image root persisten otomatis.
