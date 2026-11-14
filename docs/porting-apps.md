# Port aplikasi sederhana

## Mulai dari program freestanding

Contoh freestanding pada `user/examples/` memakai wrapper OCTAGRAM tanpa libc.
Untuk aplikasi C yang menggunakan subset libc, lihat `tcc_smoke.c` dan
`stdio_demo.c`. Runtime `user/crt0.S` memanggil `main(void)`, lalu meneruskan
nilai baliknya ke syscall `exit`.

Contoh output:

```c
#include "octagram.h"

int main(void)
{
    static const char message[] = "hello\n";
    return oct_write(OCT_STDOUT, message, sizeof(message) - 1) < 0;
}
```

Bangun dengan `make -C user`, lalu periksa hasilnya memakai `readelf -h -l`.
Program harus berupa static `ET_EXEC`, little-endian ELF64 x86_64. Gunakan
`-ffreestanding`, `-fno-pie`, `-fno-stack-protector`, dan `-mno-red-zone`.
Kernel menginisialisasi FPU dan menyimpan state FPU/SIMD per proses; build C
mengaktifkan SSE2 dan x87 serta menonaktifkan MMX. Linker script menetapkan
alamat virtual dan menghasilkan segmen RX serta RW yang terpisah. Loader
menolak dynamic linking dan segmen W+X.

## Menyesuaikan aplikasi

1. Gunakan subset runtime C yang tersedia: `stdio` dasar, wrapper file POSIX
   (`open`, `read`, `write`, `stat`, `mkdir`, dan lainnya), dan syscall
   OCTAGRAM. Port hanya subset tambahan yang benar-benar dibutuhkan.
2. Hindari syscall Linux langsung; nomor syscall dan register Linux tidak
   digunakan OS ini.
3. Path relatif dimulai dari direktori shell saat program dijalankan (default
   `/`); gunakan `oct_chdir` dan `oct_getcwd` untuk
   mengubah atau membaca direktori kerja program. Semua program masih berbagi
   namespace VFS dan mount yang sama.
4. Aplikasi harus menangani hasil negatif `-errno`; descriptor file dimulai
   dari 3, stdin/stdout/stderr adalah 0/1/2.
5. Allocator libc memakai `oct_brk`; `free` memakai ulang blok yang dibebaskan,
   tetapi belum menggabungkan blok bersebelahan atau mengembalikan heap ke OS.
   `oct_mmap`/`oct_munmap` tersedia untuk pemetaan anonim dengan ABI khusus
   OCTAGRAM, bukan kontrak POSIX/Linux.
6. `spawn`/`waitpid` dan subset `posix_spawn` tersedia, tetapi belum ada
   `fork`, `execve`, signals, sockets, user threads, atau dynamic loader.
   Hilangkan dependensi fitur tersebut atau port library yang lebih kecil.

`user/runtime.c` menyediakan operasi memory/string dasar dan allocator
`malloc`/`calloc`/`realloc` dengan reuse blok lewat `free`. `stdio` belum
memiliki buffering atau floating-point formatting. Lihat
[`docs/building.md`](building.md) untuk subset API yang tersedia.

## Batas keselamatan dan runtime

Program berjalan sinkron pada page table aktif kernel, bukan dalam address
space terisolasi. User page permission membantu validasi syscall tetapi belum
mencegah program merusak mapping kernel yang berbagi page table. Kernel task
lain tidak dapat preempt program yang loop selamanya. Gunakan hanya program
yang Anda kontrol, dan selalu panggil exit.

`oct_read(OCT_STDIN, ...)` menunggu satu baris keyboard, menampilkan input,
mendukung Backspace, Enter, dan Ctrl-D. Input keyboard saat ini ASCII.
`/dev/tty` dan `/dev/console` mengakses terminal yang sama; endpoint
`/dev/stdin`, `/dev/stdout`, dan `/dev/stderr` juga tersedia di devfs.
`write` ke console/TTY menerima UTF-8, sementara descriptor file mendukung data
biner. Panjang syscall read/write maksimum 4096 byte per panggilan. Untuk file
lebih besar, lakukan operasi bertahap.

Contoh `file_demo.c` menunjukkan `open`, `write`, `lseek`, `read`, dan `close`.
File yang ditulis ke `/` tersimpan pada RAMFS kecuali path diarahkan ke mount
volume persisten.
