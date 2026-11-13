# Port aplikasi sederhana

## Mulai dari program freestanding

Contoh pada `user/examples/` tidak menggunakan libc. Runtime
`user/crt0.S` memanggil `main(void)`, lalu meneruskan nilai baliknya ke syscall
`exit`. Gunakan wrapper pada `user/include/octagram.h` atau panggil syscall
langsung.

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
Linker script menetapkan alamat virtual dan menghasilkan segmen RX serta RW
yang terpisah. Loader menolak dynamic linking dan segmen W+X.

## Menyesuaikan aplikasi

1. Ganti pemanggilan libc seperti `printf`, `open`, dan `read` dengan wrapper
   OCTAGRAM atau implementasikan subset kecil yang dibutuhkan.
2. Hindari syscall Linux langsung; nomor syscall dan register Linux tidak
   digunakan OS ini.
3. Path relatif dimulai dari direktori shell saat program dijalankan (default
   `/`); gunakan `oct_chdir` dan `oct_getcwd` untuk
   mengubah atau membaca direktori kerja program. Semua program masih berbagi
   namespace VFS dan mount yang sama.
4. Aplikasi harus menangani hasil negatif `-errno`; descriptor file dimulai
   dari 3, stdin/stdout/stderr adalah 0/1/2.
5. Implementasikan allocator sederhana di atas `oct_brk`, atau hindari
   alokasi dinamis. Heap yang dipetakan hanya 1 MiB.
6. Tidak ada `argv`/environment, clock, process creation, signals, sockets,
   `mmap`, threads user, atau dynamic loader. Hilangkan dependensi fitur
   tersebut atau port library yang lebih kecil.

`user/runtime.c` menyediakan operasi memory/string dasar dan allocator
`malloc`/`calloc` bump-only. `free` belum mengembalikan ruang ke heap. Tidak ada
`printf`/`stdio`; tulis output dengan `oct_write`.

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
