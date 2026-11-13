# OCTAGRAM syscall ABI

Syscall dilakukan dengan `int $0x80`. Nomor syscall ada di `RAX`; tiga
argumen pertama berada di `RDI`, `RSI`, `RDX`. Nilai balik ditulis ke `RAX`.
Hasil sukses tidak negatif. Error dikembalikan sebagai nilai negatif errno
(`-errno`), sign-extend sebagai `int64_t`. Header `user/include/octagram.h`
menyediakan inline wrappers.

## Tabel syscall

| `RAX` | Nama | Argumen | Hasil |
|---:|---|---|---|
| 0 | `exit` | `RDI=status` | Tidak kembali; kembali ke shell |
| 1 | `write` | `RDI=fd`, `RSI=buffer`, `RDX=count` | Byte yang ditulis |
| 2 | `read` | `RDI=fd`, `RSI=buffer`, `RDX=count` | Byte yang dibaca |
| 3 | `open` | `RDI=path`, `RSI=flags` | Descriptor baru |
| 4 | `close` | `RDI=fd` | 0 |
| 5 | `lseek` | `RDI=fd`, `RSI=offset`, `RDX=whence` | Offset baru |
| 6 | `stat` | `RDI=path`, `RSI=struct oct_stat*` | 0 |
| 7 | `mkdir` | `RDI=path` | 0 |
| 8 | `unlink` | `RDI=path` | 0 |
| 9 | `ftruncate` | `RDI=fd`, `RSI=length` | 0 |
| 10 | `brk` | `RDI=new_break`, 0 untuk query | Break baru |
| 11 | `getpid` | — | Selalu 1 |
| 12 | `isatty` | `RDI=fd` | 1 untuk console |
| 13 | `chdir` | `RDI=path` | 0 |
| 14 | `getcwd` | `RDI=buffer`, `RSI=size` | Byte path termasuk NUL |

`open` menerima mode akses `OCT_O_RDONLY=0`, `OCT_O_WRONLY=1`,
`OCT_O_RDWR=2` dan flag `OCT_O_CREAT`, `OCT_O_TRUNC`, `OCT_O_APPEND` dari
header. Mode create/truncate memerlukan akses tulis. Path boleh absolute atau
relatif terhadap cwd proses, harus NUL-terminated, dan maksimal 255 byte.

`lseek` memakai `OCT_SEEK_SET`, `OCT_SEEK_CUR`, atau `OCT_SEEK_END`.
`oct_stat_path` mengisi struct:

```c
struct oct_stat {
    uint64_t size;
    uint32_t mode;     /* node type bits plus Unix permission bits */
    uint32_t reserved;
};
```

`mode` menyertakan tipe node dan bit permission: file `0100000`, direktori
`0040000`, symlink `0120000`, character device `0020000`; bit permission
berada pada bagian bawah seperti `0644` atau `0755`.

Descriptor 0 membaca TTY keyboard dalam mode baris: input di-echo, Backspace
menghapus karakter, Enter menyelesaikan baris, dan Ctrl-D menghasilkan EOF.
Baris dibatasi 255 karakter; pembacaan menunggu input dan dapat mengembalikan
sebagian baris bila buffer pemanggil lebih kecil. Descriptor 1 dan 2 menulis
ke terminal framebuffer;
bytes harus UTF-8. `/dev/tty` dan `/dev/console` mengakses TTY yang sama,
sedangkan `/dev/stdin`, `/dev/stdout`, dan `/dev/stderr` menyediakan endpoint
terpisah sesuai arahnya. Path tersebut terdaftar pada devfs di `/dev`.

Path relatif di-resolve terhadap direktori kerja proses. `chdir` mengubah
direktori kerja proses saat ini; program yang dijalankan shell mewarisi
direktori kerja shell. Namespace VFS, mount, dan file tetap dibagi dengan
kernel dan proses lain, seperti Linux tanpa mount namespace. Descriptor
terbuka tetap berupa state per proses.
`getcwd` mengembalikan jumlah byte termasuk NUL; `-ERANGE` berarti buffer
terlalu kecil.
`lseek` pada perangkat TTY mengembalikan `-ESPIPE`.

## Argumen program

Saat ELF dimulai, stack berisi `argc`, pointer `argv`, terminator `argv`,
kemudian `envp` kosong. Startup assembly meneruskan `argc` dan `argv` ke
`main(int argc, char **argv)`. Maksimum 64 argumen termasuk `argv[0]` dengan
total isi string maksimum 4096 byte. Shell menerima argumen berkutip tunggal
atau ganda; environment variable belum tersedia.

`read`/`write` dibatasi 4096 byte per panggilan; alamat dan permission halaman
user divalidasi sebelum diakses. Maksimum ada 16 descriptor per program,
termasuk tiga descriptor console.

`brk` hanya mengubah break dalam rentang heap yang telah dipetakan saat load:
`0x10000000` sampai `0x10100000`. Ini bukan allocator; runtime atau aplikasi
harus mengelola alokasi sendiri. `getpid` mengembalikan PID lokal grup (`run`
menghasilkan PID 1; `runp` memberi PID berurutan mulai dari 1). Error umum
meliputi `-2` (not found), `-9` (bad descriptor),
`-13` (permission denied), `-14` (bad pointer), `-22` (invalid argument),
`-28` (no space), `-29` (TTY tidak dapat di-seek), `-34` (buffer terlalu
kecil), dan `-38` (syscall tidak dikenal).

ABI ini sengaja bukan Linux ABI dan belum mencakup `fork`, `execve`, `mmap`,
signal, socket, clock, atau dynamic linking. ELF memiliki address space privat
dan dapat dipreempt ketika berjalan di ring 3. Syscall yang menunggu secara
sinkron, termasuk pembacaan TTY, belum memblokir proses secara scheduler-aware
sehingga dapat menunda proses lain.
Menambahkan nomor syscall adalah perubahan ABI; jaga nomor lama dan update
header, tabel ini, implementasi, dan contoh program secara bersamaan.
