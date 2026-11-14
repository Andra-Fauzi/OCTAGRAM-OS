# Build kernel dan program user

## Build kernel

Gunakan Rust nightly sesuai `kernel/rust-toolchain.toml`:

```sh
cargo +nightly fmt --manifest-path kernel/Cargo.toml --check
make -C user/rust gcc
cargo +nightly test --manifest-path kernel/Cargo.toml --target x86_64-unknown-linux-gnu
make -C kernel KARCH=x86_64
```

`make -C kernel` membuat binary kernel; target root repository membuat ISO
Limine dengan `make all` dan menjalankannya melalui QEMU dengan `make run`.
Image memerlukan dependency yang dicantumkan di README utama, termasuk
`xorriso` untuk ISO dan `qemu-system-x86_64` untuk menjalankan QEMU.
Log kernel juga dikirim ke COM1; tampilkan di host saat boot QEMU dengan
`make run-bios QEMUFLAGS="-m 2G -serial stdio"`.

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

## Program Rust `no_std`

Program Rust user dapat di-cross-compile di host ke ELF statis OCTAGRAM:

```sh
rustup target add x86_64-unknown-none --toolchain nightly
make -C user rust
file user/rust/build/rust_hello.elf
```

Build membutuhkan Rust nightly dan `ld.lld`. Runtime pada
[`user/rust/runtime`](../user/rust/runtime/src/lib.rs) menyediakan syscall
OCTAGRAM, allocator bump di atas `brk`, dan API untuk pemetaan memori. Contoh
[`rust_hello`](../user/rust/hello/src/main.rs) memakai `alloc::Vec` untuk
meminta heap lebih dari 1 MiB. Salin ELF ke volume yang bisa di-mount, lalu
jalankan dengan `run /mnt/rust_hello.elf`.

Ini target `no_std` khusus OCTAGRAM, bukan Rust `std`/Linux ABI. Compiler masih
berjalan di host; `rustc` dan Cargo belum berjalan di dalam OS.

Runtime minimum menyediakan fungsi memory/string dasar seperti `memcpy`,
`memset`, `memchr`, `strncat`, `strspn`, `strcspn`, `strtok`, dan `strtok_r`,
`getenv`, `setenv`, `unsetenv`, serta allocator
`malloc`/`calloc`/`realloc` dengan `free` yang memakai ulang blok yang dibebaskan.
Allocator belum menggabungkan blok bersebelahan atau mengembalikan heap ke OS.
Wrapper POSIX dasar melalui `<fcntl.h>`, `<sys/stat.h>`, `<unistd.h>`,
`<dirent.h>`, dan `<errno.h>`:
`open`, `read`, `write`, `lseek`, `close`, `pipe`, `dup2`, `chdir`, `getcwd`,
`getpid`, `isatty`, `unlink`, `mkdir`, `stat`, `ftruncate`, serta direktori
`opendir`/`readdir`/`closedir`. `struct stat`
menyediakan `st_mode` dan `st_size`; `mkdir` menerapkan permission yang diminta,
tetapi `umask` belum tersedia. Perubahan permission bergantung pada dukungan
filesystem; FAT32 belum menyediakan `chmod`. `posix_spawn`/`posix_spawnp`
serta `waitpid` tersedia untuk menjalankan dan menunggu ELF anak. Kesalahan
wrapper syscall dikembalikan sebagai `-1` dan mengisi global `errno`; fungsi
`posix_spawn` mengembalikan nomor error langsung. File actions dan attributes
`posix_spawn` harus `NULL`; opsi `waitpid` selain 0 belum didukung. Flag open
dan batas syscall mengikuti ABI OCTAGRAM. Belum ada `fork`, `execve`, atau
dukungan proses `fork`/`execve` atau pemulihan heap ke OS.

Header `<stdio.h>` menyediakan operasi stream dasar (`fopen`, `fdopen`,
`fclose`, `fread`, `fwrite`, `fgets`, `fputs`, `fseek`, dan standard streams),
`printf`/`fprintf`/`snprintf`, serta keluarga `scanf` (`scanf`, `fscanf`,
`sscanf` dan bentuk `v`). Scanning mendukung integer (`d i o u x X p`), string,
karakter, scanset, angka floating-point desimal/heksadesimal (`a A e E f F g G`),
width, assignment suppression, dan `%n`. Input `inf`/`nan` serta
wide-character belum didukung. Stream tidak di-buffer;
formatter mendukung string, karakter, pointer, dan integer (`d i u o x X`)
dengan width, precision, dan flag dasar. Floating-point formatting belum
tersedia.

Jalankan `make -C user scanf-test` untuk menguji parser input string runtime
tanpa boot OS. Contoh `scanf_demo` membaca token dan angka dari stdin.

## Toolchain native di dalam OCTAGRAM

Build kernel juga membangun ELF userland `/bin/gcc`, yang memakai compiler C
subset OCTAGRAM. Kernel menanam executable tersebut dan header di `/include`
ke RAMFS saat boot. Shell tetap menyediakan `cc` sebagai compiler bawaan kernel;
`gcc` sekarang dicari dan dijalankan sebagai program ELF melalui `PATH`.
Compiler membaca dan menulis file di VFS, jadi source bisa berada di RAMFS atau
volume FAT32 yang sudah di-mount.

Build kernel juga membangun ELF userland `/bin/chibicc`, port front-end
chibicc. Di shell, `-E` dan `-S` dijalankan langsung oleh front-end; mode
kompilasi biasa meneruskan assembly-nya ke assembler dan linker internal untuk
menghasilkan ELF statis:

```text
chibicc /tmp/test.c -o /tmp/smoke.elf
run /tmp/smoke.elf
chibicc -c /tmp/test.c -o /tmp/test.o
ld /tmp/test.o -o /tmp/from-object.elf
run /tmp/from-object.elf
```

Image menyertakan `/tmp/test.c` sebagai smoke test. Program itu mencetak
`berapa umurnya` dan `umurnya 24`, lalu keluar dengan status 0. Tanpa `-o`,
hasil link ditulis ke `a.out` di direktori saat ini. Jalur ini menerima satu
file C dan tidak mendukung linking library eksternal. `-c` menghasilkan object
internal OCTAGRAM, bukan object ELF relocatable standar. Header standar yang
disediakan port ini berada di `/include`, sedangkan header internal chibicc
berada di `/include/chibicc`. `/bin/gcc` tetap compiler subset terpisah;
chibicc belum menggantikannya.

Kernel juga menanam TinyCC sebagai `/bin/tcc`, beserta `crt1.o`, `libc.a`,
`libtcc1.a`, dan header C di `/include`. TCC menghasilkan ELF64 statis OCTAGRAM
dari satu atau beberapa file C:

```text
tcc /tmp/tcc.c -o /tmp/tcc-smoke.elf
run /tmp/tcc-smoke.elf
```

Default linker TCC menggunakan `/lib/tcc`; opsi `-I`, `-L`, dan `-l` dapat
dipakai untuk header dan arsip tambahan yang tersedia di VFS. `/tmp/tcc.c`
adalah contoh smoke test boot yang mencakup allocator, string,
`setjmp`/`longjmp`, `stdio`/`sscanf`, `stat`, dan `mkdir`. Runtime C merupakan
subset OCTAGRAM, bukan libc POSIX penuh.

Ini belum merupakan toolchain C11 lengkap: assembler hanya memahami subset
instruksi x86_64, linker hanya menerima satu object, dan belum ada runtime C
umum atau dukungan relocation umum. Untuk jalur shell, panggilan `printf`
langsung ditautkan ke stub kernel dengan formatter terbatas; fungsi eksternal
lain, linking libc, dan fitur stdio lainnya belum tersedia. Section data belum
dipisahkan dari kode pada output ELF; program yang memakai global writable atau
inline assembly belum didukung. Gunakan toolchain host di atas untuk program
yang memerlukan fitur tersebut.

Fondasi syscall `mmap`/`munmap` belum menyediakan `rustc`, Cargo, Rust `core`,
atau runtime hosted; compiler Rust penuh yang berjalan di OCTAGRAM belum tersedia.

```text
as /mnt/hello.s -o /mnt/hello.o
ld /mnt/hello.o -o /mnt/hello.elf
gcc /mnt/hello.c -o /mnt/hello.elf
chibicc -E /mnt/hello.c
chibicc -S /mnt/hello.c -o /mnt/hello.s
chibicc /mnt/hello.c -o /mnt/hello.elf
run /mnt/hello.elf
```

`gcc` menerima satu file input, `-o` opsional (default `a.out`), serta
`--help`, `--version`, `-std=c11`, dan beberapa opsi umum yang diabaikan.
`gcc` menghasilkan ELF yang dapat dijalankan, dengan permission executable.

Compiler subset `cc` dan `/bin/gcc` hanya menerima satu fungsi `int main(void)`,
panggilan `oct_write(OCT_STDOUT, "teks", panjang)` dengan string literal atau
macro string dan panjang literal atau macro bilangan bulat, serta `puts("teks")`
dan `printf("teks")` tanpa format specifier. Header standar seperti `<stdio.h>`,
`<stdlib.h>`, `<stddef.h>`, `<stdint.h>`, dan beberapa header POSIX minimal
tersedia di `/include`; deklarasi header tidak menyediakan implementasi fungsi
tambahan di compiler subset. Compiler juga mendukung sampai dua variabel lokal
`int` yang diinisialisasi di blok utama, assignment sederhana, serta `return`
dan kondisi dengan ekspresi integer: literal/macro/variabel, unary `+`, `-`,
`!`, aritmetika `+`, `-`, `*`, perbandingan `==`, `!=`, `<`, `<=`, `>`, `>=`,
serta logika short-circuit `&&` dan `||`. `if`/`else` dan `while` dengan blok
`{}` juga didukung; kondisi dianggap benar jika hasil ekspresinya bukan nol.
`return` hanya boleh berada di blok utama.
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

`cc` dan `/bin/gcc` mencari header kutip relatif terhadap source, lalu di
subfolder `include` source dan `/include`. Header kurung sudut dicari di
subfolder `include`, `/include`, lalu folder source. Header standar yang
disediakan dianggap header bawaan; deklarasi C di header diabaikan. Preprocessor
mendukung include bertingkat (maksimal 8 tingkat/32 file/32 KiB), include guard
`#ifndef`/`#ifdef`/`#else`/`#endif`, `#define`/`#undef` object-like, dan
`#pragma once`. Macro string dan bilangan bulat dapat digunakan pada argumen
output dan ekspresi `return`; macro ekspresi dan macro fungsi belum didukung.
Ini bukan preprocessor GCC lengkap, dan format `printf` belum didukung.

Assembler menerima subset instruksi x86_64 sederhana: `mov`, `push`, `pop`,
`add`, `sub`, `imul`, `test`, `cmp`, `setcc`, `movzb`, `movsxd`,
`rep stosb`, `lea` RIP-relative, `call`, `jmp`, conditional branch,
`int $0x80`, `ret`, `nop`, label, serta directive `.text`, `.rodata`,
`.global`, `.byte`, dan `.asciz`. File `.o` adalah format objek internal
OCTAGRAM; linker saat ini menerima satu objek dan menghasilkan ELF64 statis
`ET_EXEC`.

Toolchain native belum mendukung GNU Make, preprocessor C penuh, variabel
global, array, pointer, banyak fungsi, relocations umum, atau dynamic linking.
`for` mendukung klausa initializer deklarasi
`int`/assignment, kondisi benar-nol, dan increment assignment. `switch`
mendukung label `case` integer/macro, satu `default`, fall-through, serta
`break`; `break` juga berlaku di `while` dan `for`, sedangkan `continue`
melanjutkan ke kondisi `while` atau klausa increment `for`. Gunakan toolchain
host di atas untuk aplikasi di luar subset tersebut.

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

Program dapat menjalankan ELF anak dan menunggu status keluarnya melalui
syscall `spawn`/`waitpid`. Contoh `spawn_demo` menjalankan ELF yang diberikan:

```text
run /mnt/spawn_demo.elf /mnt/hello.elf
```

`pipe_demo` menguji pipe yang mewarisi descriptor ke child, `dup2` ke stdout,
serta read yang memblokir sampai child menulis:

```text
run /mnt/pipe_demo.elf
```

`posix_demo` menguji wrapper POSIX untuk direktori dan file, termasuk
`chdir`/`getcwd`, iterasi direktori, permission, `ftruncate`, pipe, dan pemetaan
error ke `errno`:

```text
run /mnt/posix_demo.elf
```

`stdio_demo` menguji formatting ke buffer, operasi file melalui `FILE`, dan
output formatted ke stdout:

```text
run /mnt/stdio_demo.elf
```

`posix_spawn_demo` menguji environment khusus child serta encoding status
`waitpid`:

```text
run /mnt/posix_spawn_demo.elf
```

Nama perangkat bergantung hasil `devices`. Perintah `run` menjalankan ELF dan
kembali ke shell setelah syscall `exit` atau user fault. Untuk menjalankan
beberapa ELF CPU-bound secara bersamaan, gunakan:

```text
runp /mnt/worker-a.elf /mnt/worker-b.elf
```

`runp` memakai round-robin timer preemption dan membatasi grup menjadi 16
proses. Shell kembali setelah semua proses keluar atau fault.
Shell mewariskan environment ke program dan proses anak. Variabel dapat diatur
dengan `export NAME=value`; `PATH` bawaan adalah `/bin`, `/usr/bin`, dan
`/usr/local/bin`. Atur direktori pencarian dengan `export PATH=...`, pisahkan
direktori memakai `:`. Contoh:

```text
mkdir /bin
ln -s /mnt/argv_demo.elf /bin/argv_demo
export PATH=/bin:/usr/bin:/mnt
export testvar=passed
cd /
argv_demo hello "dua kata"
run /mnt/env_demo.elf
```

Argumen yang dipisahkan spasi dikirim ke `main(argc, argv)`. Tanda kutip tunggal
atau ganda dapat dipakai untuk satu argumen yang berisi spasi. `env_demo`
memastikan `envp` dan global `environ` memuat nilai yang diekspor.
`runp` menerima daftar path executable; setiap proses mendapat path-nya sebagai
`argv[0]`.
Contoh `file_demo.elf` membuat `/hello.txt` di VFS saat ini, yang bila root masih
RAMFS akan hilang saat reboot. Untuk penyimpanan FAT32, ubah path contoh
menjadi `/mnt/hello.txt` lalu build ulang.

## Format program yang didukung

- x86_64, ELF64 little-endian, `ET_EXEC`.
- Static; tidak ada interpreter, dynamic linker, full libc, atau relocations
  runtime.
- Segmen tidak boleh writable sekaligus executable.
- Entry point harus berada di segmen executable.
- Stack 8 halaman dan heap awal 1 MiB; `brk` dapat menumbuhkan heap sampai batas
  image. `mmap`/`munmap` menyediakan pemetaan anonim tambahan yang dibatasi
  alamat virtual dan memori fisik yang tersedia.
- Startup meneruskan `argc`, `argv`, dan `envp` ke `main`; environment tersedia
  melalui argumen ketiga atau global `environ`.

Contoh [tty_demo.c](../user/examples/tty_demo.c) membaca satu baris dari
keyboard melalui `oct_read(OCT_STDIN, ...)`, lalu mencetaknya ke terminal.
Contoh [graphics_demo.c](../user/examples/graphics_demo.c) menggambar gradien
pixel melalui syscall `oct_putpixel`; area gambar berbagi framebuffer dengan
teks terminal. Build bersama contoh lain dengan `make -C user`.

Lihat [ABI syscall](syscall-abi.md) dan [panduan port aplikasi](porting-apps.md)
untuk batasan aplikasi.
