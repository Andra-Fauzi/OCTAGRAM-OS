# Build kernel dan program user

## Build kernel

Gunakan Rust nightly sesuai `kernel/rust-toolchain.toml`:

```sh
cargo +nightly fmt --manifest-path kernel/Cargo.toml --check
cargo +nightly test --manifest-path kernel/Cargo.toml --target x86_64-unknown-linux-gnu
make -C kernel KARCH=x86_64
```

`make -C kernel` membuat binary kernel; target root repository membuat ISO
Limine dengan `make all` dan menjalankannya melalui QEMU dengan `make run`.
Image memerlukan dependency yang dicantumkan di README utama, termasuk
`xorriso` untuk ISO dan `qemu-system-x86_64` untuk menjalankan QEMU.

## Toolchain aplikasi user

Toolchain host tetap tersedia untuk membangun program C yang lebih besar
menggunakan GCC, GNU assembler (`as`), GNU linker (`ld`), dan GNU Make. Output
berupa ELF64 x86_64 statis dengan ABI OCTAGRAM, bukan executable Linux.

Pada Debian/Ubuntu, paket umumnya:

```sh
sudo apt install build-essential binutils
```

Bangun contoh:

```sh
make -C user
file user/build/hello.elf user/build/file_demo.elf user/build/tty_demo.elf
file user/build/argv_demo.elf
readelf -h -l user/build/hello.elf
```

Makefile memisahkan kompilasi C (`gcc`), perakitan startup (`as`), dan link
statis (`ld`). `CROSS_COMPILE` dapat diberi prefix toolchain x86_64 khusus,
misalnya `make -C user CROSS_COMPILE=x86_64-elf-`. Bisa juga override `CC`,
`AS`, atau `LD` dari command line.

Runtime minimum menyediakan fungsi memory/string dasar (`memcpy`, `memset`,
`strlen`, dan lainnya), allocator bump `malloc`/`calloc`, serta syscall wrappers.
Tidak ada `printf`, `stdio`, atau implementasi `free` yang mengembalikan memori.

## Toolchain native di dalam OCTAGRAM

Shell menyediakan `as`, `ld`, dan `cc` bawaan kernel. Ini implementasi kecil
native, bukan GNU GCC atau GNU binutils. Perintah membaca dan menulis file di
VFS, jadi source bisa berada di RAMFS atau volume FAT32 yang sudah di-mount.

```text
as /mnt/hello.s -o /mnt/hello.o
ld /mnt/hello.o -o /mnt/hello.elf
cc /mnt/hello.c -o /mnt/hello.elf
run /mnt/hello.elf
```

Compiler C hanya menerima satu fungsi `int main(void)`, panggilan
`oct_write(OCT_STDOUT, "teks", panjang, ...)` dengan string literal atau macro
string dan panjang literal atau macro bilangan bulat, sampai dua variabel lokal
`int` yang diinisialisasi di blok utama, assignment sederhana, serta `return`
dengan ekspresi integer: literal/macro/variabel, tanda unary, `+`, `-`, `*`, dan
tanda kurung.
`if`/`else` dan `while` dengan blok `{}` juga didukung; kondisi dianggap benar
jika hasil ekspresinya bukan nol. `return` hanya boleh berada di blok utama.
Ekspresi dijalankan sebagai operasi 64-bit; semantik tipe C lengkap belum
didukung. Contoh:

```c
#include "octagram.h"

int main(void) {
    oct_write(OCT_STDOUT, "Halo OCTAGRAM!\n", 15);
    return 0;
}
```

Contoh loop dan kondisi sederhana:

```c
int main(void) {
    int count = 3;
    while (count) {
        oct_write(OCT_STDOUT, "tick\n", 5);
        count = count - 1;
    }
    if (count) {
        oct_write(OCT_STDOUT, "unexpected\n", 11);
    } else {
        oct_write(OCT_STDOUT, "done\n", 5);
    }
    return count;
}
```

`cc` mencari header kutip relatif terhadap source, lalu di subfolder `include`
source dan `/include`. Header kurung sudut dicari di subfolder `include`,
`/include`, lalu folder source. Header `<stddef.h>` dan `<stdint.h>` dianggap
header bawaan kosong; deklarasi C di header lain diabaikan. Preprocessor
mendukung include bertingkat (maksimal 8 tingkat/32 file/32 KiB), include guard
`#ifndef`/`#ifdef`/`#else`/`#endif`, `#define`/`#undef` object-like, dan
`#pragma once`. Macro string dan bilangan bulat dapat digunakan pada argumen
`oct_write` dan ekspresi `return`; macro ekspresi dan macro fungsi belum
didukung. Deklarasi dan fungsi dari header diabaikan. Ini bukan preprocessor
GCC lengkap, jadi `octagram.h` dapat disertakan untuk konstanta sederhana tetapi
syscall wrapper-nya tidak otomatis tersedia.

Assembler menerima subset instruksi x86_64 sederhana: `mov`, `push`, `pop`,
`add`, `sub`, `imul`, `test`, `lea` RIP-relative, `call`, `jmp`, `je`/`jne`,
`int $0x80`, `ret`, `nop`, label, serta directive `.text`, `.rodata`, `.global`,
`.byte`, dan `.asciz`. File `.o` adalah format objek internal OCTAGRAM; linker
saat ini menerima satu objek dan menghasilkan ELF64 statis `ET_EXEC`.

Toolchain native belum mendukung GNU Make, preprocessor C penuh, variabel
global, array, pointer, perbandingan/logika kondisi, `for`, `switch`,
`break`/`continue`, banyak fungsi, relocations umum, atau dynamic linking.
Gunakan toolchain host di atas untuk aplikasi di luar subset tersebut.

## Menyalin dan menjalankan aplikasi

Letakkan file `.elf` pada filesystem FAT32 yang dapat dibaca OS. Boot OCTAGRAM,
buat mount point, lalu mount perangkat yang memuat ELF:

```text
mkdir /mnt
devices
mount usb0 /mnt
ls /mnt
run /mnt/hello.elf
```

Nama perangkat bergantung hasil `devices`. Perintah `run` menjalankan ELF dan
kembali ke shell setelah syscall `exit` atau user fault. Untuk menjalankan
beberapa ELF CPU-bound secara bersamaan, gunakan:

```text
runp /mnt/worker-a.elf /mnt/worker-b.elf
```

`runp` memakai round-robin timer preemption dan membatasi grup menjadi 16
proses. Shell kembali setelah semua proses keluar atau fault.
Shell juga mencari perintah langsung melalui `PATH` bawaan `/bin`, `/usr/bin`,
dan `/usr/local/bin`. Atur direktori pencarian tambahan dengan `export PATH=...`,
pisahkan direktori memakai `:`. Contoh:

```text
mkdir /bin
ln -s /mnt/argv_demo.elf /bin/argv_demo
export PATH=/bin:/usr/bin:/mnt
cd /
argv_demo hello "dua kata"
```

Argumen yang dipisahkan spasi dikirim ke `main(argc, argv)`. Tanda kutip tunggal
atau ganda dapat dipakai untuk satu argumen yang berisi spasi. Program belum
mempunyai environment variable. `runp` menerima daftar path executable; setiap
proses mendapat path-nya sebagai `argv[0]`.
Contoh `file_demo.elf` membuat `/hello.txt` di VFS saat ini, yang bila root masih
RAMFS akan hilang saat reboot. Untuk penyimpanan FAT32, ubah path contoh
menjadi `/mnt/hello.txt` lalu build ulang.

## Format program yang didukung

- x86_64, ELF64 little-endian, `ET_EXEC`.
- Static; tidak ada interpreter, dynamic linker, full libc, atau relocations
  runtime.
- Segmen tidak boleh writable sekaligus executable.
- Entry point harus berada di segmen executable.
- Stack 8 halaman, heap tetap 1 MiB, total mapping maksimal 4096 halaman.
- Runtime awal memanggil `int main(void)` tanpa `argc`, `argv`, atau environment.

Contoh [tty_demo.c](../user/examples/tty_demo.c) membaca satu baris dari
keyboard melalui `oct_read(OCT_STDIN, ...)`, lalu mencetaknya ke terminal.
Build bersama contoh lain dengan `make -C user`.

Lihat [ABI syscall](syscall-abi.md) dan [panduan port aplikasi](porting-apps.md)
untuk batasan aplikasi.
