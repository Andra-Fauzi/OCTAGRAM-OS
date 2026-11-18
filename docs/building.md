# Build kernel dan program user

## Build kernel

Gunakan Rust nightly sesuai `kernel/rust-toolchain.toml`:

```sh
cargo +nightly fmt --manifest-path kernel/Cargo.toml --check
make -C user/rust make
cargo +nightly test --manifest-path kernel/Cargo.toml --target x86_64-unknown-linux-gnu
make -C kernel KARCH=x86_64
```

Program Rust user (`user/rust`) ditautkan dengan `ld.lld`; sediakan lewat paket
`lld` atau symlink ke `ld.lld` bawaan toolchain Rust. `make -C kernel` membuat binary kernel; target root repository membuat ISO
Limine dengan `make all` dan menjalankannya melalui QEMU dengan `make run`.
Image memerlukan dependency yang dicantumkan di README utama, termasuk
`xorriso` untuk ISO dan `qemu-system-x86_64` untuk menjalankan QEMU.
Log kernel juga dikirim ke COM1; tampilkan di host saat boot QEMU dengan
`make run-bios QEMUFLAGS="-m 2G -serial stdio"`.

## Toolchain aplikasi user

Toolchain host tetap tersedia untuk membangun program C yang lebih besar
menggunakan GCC, GNU assembler (`as`), GNU linker (`ld`), dan GNU Make. Output
berupa ELF64 x86_64 dengan ABI OCTAGRAM, bukan executable Linux. Program statis
tetap didukung; dynamic linking awal mendukung executable PIE dengan
interpreter `/lib/ld-octagram.so` dan shared library di `/lib`.

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
make -C user dynamic
readelf -h -l -d user/build/dynamic_demo.elf
```

Makefile memisahkan kompilasi C (`gcc`), perakitan startup (`as`), dan link
statis (`ld`). `CROSS_COMPILE` dapat diberi prefix toolchain x86_64 khusus,
misalnya `make -C user CROSS_COMPILE=x86_64-elf-`. Bisa juga override `CC`,
`AS`, atau `LD` dari command line.

Target `dynamic` membangun `/lib/ld-octagram.so`, shared libc OCTAGRAM,
shared library contoh (dengan version script), executable dinamis PIE,
executable non-PIC `copy_reloc_demo.elf` yang menguji relokasi `R_X86_64_COPY`,
serta `versioned_demo.elf` yang menguji dua versi satu nama simbol. Jalankan
`/bin/dynamic-demo`, `/bin/copy-reloc-demo`, dan `/bin/versioned-demo` setelah
boot untuk menguji loader dan pemanggilan libc bersama. `aslr_demo.elf`
mencetak alamat `main`, fungsi shared library, hasil `mmap`, dan variabel
stack; jalankan dua kali untuk melihat load bias PIE, penempatan library, dan
puncak stack user yang berbeda. `dynamic_demo` juga
memverifikasi bahwa handler `atexit`/`__cxa_atexit` berjalan sebelum destructor
`DT_FINI_ARRAY` milik shared library.

`dlopen_demo.elf` menautkan libc dinamis saja, lalu memuat `libdyn_demo.so`
saat runtime dengan `dlopen`, mencari `shared_library_message` lewat `dlsym`,
dan memastikan simbol yang tidak ada mengisi `dlerror`. Demo juga mencari
simbol yang sama lewat `RTLD_DEFAULT` dan `RTLD_NEXT`. Jalankan
`/bin/dlopen-demo`; `dlclose` menurunkan hitungan referensi, menjalankan
destructor library, lalu melepas seluruh pemetaannya ketika referensi habis.
Dependency `DT_NEEDED` dari library yang di-dlopen tidak dimuat otomatis, dan
library tetap dicari di `LD_LIBRARY_PATH` lalu `/lib`. Batas format ELF dan relocation yang tersedia
dicatat di [`docs/porting-apps.md`](porting-apps.md).

Program statis `tls-demo` menunjukkan TLS model local-exec dan dapat dijalankan
dengan `run /bin/tls-demo`. `thread-demo` menguji `pthread_create`, join, mutex,
shared memory, dan salinan TLS per thread:

```text
run /bin/thread-demo
```

`<pthread.h>` menyediakan subset `pthread` dengan scheduler preemptive.
Thread berbagi address space; atribut thread hanya mendukung default dan
`pthread_mutex` memakai spin lock.

## Program Rust `no_std`

Program Rust user dapat di-cross-compile di host ke ELF statis OCTAGRAM:

```sh
rustup target add x86_64-unknown-none --toolchain nightly
make -C user rust
file user/rust/build/rust_hello.elf
```

Build membutuhkan Rust nightly dan `ld.lld`. Runtime pada
[`user/rust/runtime`](../user/rust/runtime/src/lib.rs) menyediakan syscall
OCTAGRAM, allocator berbasis pemetaan anonim, dan API untuk pemetaan memori. Contoh
[`rust_hello`](../user/rust/hello/src/main.rs) memakai `alloc::Vec` untuk
meminta heap lebih dari 1 MiB. Salin ELF ke volume yang bisa di-mount, lalu
jalankan dengan `run /mnt/rust_hello.elf`.

Ini target `no_std` khusus OCTAGRAM, bukan Rust `std`/Linux ABI. Compiler masih
berjalan di host; `rustc` dan Cargo belum berjalan di dalam OS.

## Membangun aplikasi C dari dalam OCTAGRAM

Shell menyediakan `/bin/make` dan `/bin/tcc`.
Contoh proyek tersedia di `/share/octagram`:

```sh
make -C /share/octagram
run /share/octagram/hello.elf
```

`make` mendukung target eksplisit, dependensi, variabel `NAME = value`,
assignment `NAME += value`, opsi `-f`, `-C`, `-B`, dan automatic variables
`$@`, `$<`, `$^`. Dependensi dibangun berurutan dan resep dijalankan satu
program per baris; shell operator, implicit rules, timestamp, dan build
incremental belum didukung, jadi target selalu dibangun ulang. Baris recipe
harus diawali karakter Tab sebenarnya, bukan spasi; tombol Tab di editor
bawaan kini memasukkan karakter tersebut. Gunakan
`make` memakai TCC secara default. Header OCTAGRAM tersedia di `/include`, dan
runtime TCC di `/lib/tcc`.

Ini memungkinkan pengembangan serta build aplikasi C langsung di OS, bukan
build kernel Rust secara native. Kernel masih perlu dibangun dari host dengan
Rust nightly, Cargo, Limine, dan tool build yang dijelaskan di atas. Filesystem
root awal berbasis RAM; simpan pekerjaan penting pada volume persisten yang
di-mount agar tidak hilang setelah reboot.

Runtime minimum menyediakan fungsi memory/string dasar seperti `memcpy`,
`memset`, `memchr`, `strncat`, `strspn`, `strcspn`, `strtok`, dan `strtok_r`,
`getenv`, `setenv`, `unsetenv`, serta allocator
`malloc`/`calloc`/`realloc` dengan `free` yang memakai ulang blok yang dibebaskan.
Allocator belum menggabungkan blok bersebelahan atau mengembalikan heap ke OS.
Wrapper POSIX dasar melalui `<fcntl.h>`, `<sys/stat.h>`, `<unistd.h>`,
`<dirent.h>`, dan `<errno.h>`:
`open`, `openat`, `read`, `write`, `lseek`, `close`, `pipe`, `dup`, `dup2`,
`fcntl`, `chdir`, `getcwd`, `getpid`, `isatty`, `unlink`, `unlinkat`, `rmdir`,
`symlink`, `symlinkat`, `readlink`, `readlinkat`, `rename`, `renameat`, `mkdir`,
`mkdirat`, `stat`, `lstat`, `fstat`, `fstatat`, `access`, `chmod`, `fchmod`,
`umask`, dan `ftruncate`, serta direktori `opendir`/`readdir`/`closedir`/
`dirfd`. `struct stat` menyediakan
`st_mode` dan `st_size`; `mkdir` serta `open` dengan `O_CREAT` menerapkan
permission setelah `umask`. Perubahan permission bergantung pada dukungan
filesystem; FAT32 belum menyediakan `chmod`. `posix_spawn`/`posix_spawnp`
serta `waitpid` tersedia untuk menjalankan dan menunggu ELF anak. Pada build statis,
kesalahan wrapper syscall dikembalikan sebagai `-1` dan mengisi `errno` TLS
per-thread; shared libc masih memakai `errno` global. Fungsi
`posix_spawn` mengembalikan nomor error langsung. File actions `posix_spawn`
mendukung `addclose`, `adddup2`, dan `addopen` (maksimal 8 aksi dengan path
127 byte); attributes mendukung `POSIX_SPAWN_SETPGROUP`,
`POSIX_SPAWN_SETSIGDEF`, `POSIX_SPAWN_SETSIGMASK`, dan `POSIX_SPAWN_RESETIDS`
(`setgid` lalu `setuid` ke id nyata di anak; id awal proses adalah 0), dan
atribut scheduling (`POSIX_SPAWN_SETSCHEDPARAM`/
`SETSCHEDULER`) yang menerima `SCHED_OTHER` dengan prioritas 0 (nice mengatur
bobot) maupun `SCHED_FIFO`/`SCHED_RR` dengan prioritas 1..99; kebijakan RT butuh
`CAP_SCHED_NICE` sehingga non-root mendapat `EPERM` dan prioritas di luar
rentang `EINVAL`. Aksi
diterapkan di anak setelah `fork` sebelum `execve`. Contoh `posix_spawn_demo`
menguji redirection file/pipe, process group, atribut scheduling, dan kasus
gagal tersebut. `waitpid`
mendukung `WNOHANG`, sedangkan opsi lain belum didukung. `unlink` menghapus
file/symlink, sedangkan `rmdir` menghapus
direktori kosong. `readlink` tidak menambahkan NUL ke buffer. Flag open dan
batas syscall mengikuti ABI OCTAGRAM. `fork` menyalin memori user dengan eager
copy dan `execve` mengganti image proses saat ini; heap belum dikembalikan ke OS.
Waktu dasar tersedia melalui `clock_gettime`, `time`, `gmtime`, dan `localtime`.
RTC memiliki resolusi satu detik dan dianggap UTC; zona waktu belum didukung.
Database akun tersedia melalui `<pwd.h>` dan `<grp.h>`: `getpwnam`/`getpwuid`/
`getpwent` membaca `/etc/passwd`, sedangkan `getgrnam`/`getgrgid`/`getgrent`
membaca `/etc/group` dan memakai grup utama dari `/etc/passwd` bila file itu
belum ada. Varian `_r` memakai buffer pemanggil dan mengembalikan `ERANGE` bila
buffer terlalu kecil; database shadow belum tersedia. `sendmsg`/`recvmsg`
disusun di libc dari `send`/`sendto`/`write` dan `recv`/`recvfrom`/`read`:
iovec digabung atau disebar dengan batas `OCT_MAX_SYSCALL_IO` per panggilan dan
maksimal 1024 iovec. Data kontrol (`SCM_RIGHTS`/`SCM_CREDENTIALS`) belum
didukung, sehingga `sendmsg` mengembalikan `ENOTSUP` bila `msg_control` diisi
dan `recvmsg` selalu mengosongkan `msg_controllen`. Kernel melaporkan panjang
datagram UDP asli, sehingga `recvmsg` mengisi `msg_flags` dengan `MSG_TRUNC`
bila data dipotong; `MSG_CTRUNC` tidak pernah diisi karena tidak ada data
kontrol. Socket TCP memakai pipeline sampai 4 segmen dengan congestion window
1→4 MSS, dibatasi `peer_mss` dan `peer_window`; segmen tertua yang belum di-ACK
dikirim ulang dengan RTO
eksponensial 30→60→120→240→480 tick, maksimal 5 percobaan sebelum socket
di-RST dengan error `TCP connection timed out`. Saat jendela peer nol, kernel
mengirim probe 1 byte setiap 60 tick agar pembukaan jendela tidak hilang.
Keepalive bersifat opt-in lewat `setsockopt(SO_KEEPALIVE)`: socket established
mengirim probe ACK setelah 60.000 tick tanpa aktivitas, dan menutup koneksi
dengan `TCP keepalive timed out` setelah 9 probe tanpa jawaban.
Socket UDP mendukung `connect`,
sehingga `sendmsg` tanpa `msg_name`,
`send`, dan `recv` memakai peer hasil `connect`; datagram dari sumber lain
diabaikan saat menerima. `<netdb.h>` menyediakan
`getaddrinfo` numerik IPv4 dan IPv6 dengan flag `AI_PASSIVE`, `AI_CANONNAME`,
`AI_NUMERICHOST`, dan `AI_NUMERICSERV`, serta `getnameinfo`, `gethostbyname`,
`gethostbyaddr`, dan `gai_strerror`; tanpa `AI_PASSIVE` node NULL menghasilkan
loopback (`::1` untuk IPv6). `inet_pton`/`inet_ntop` menerima `AF_INET6`, dan
`getnameinfo` memformat alamat IPv6 secara numerik. `ai_family` `AF_UNSPEC`
mengembalikan entri IPv4 lebih dulu lalu IPv6 (bila resolve berhasil) yang
dirantai lewat `ai_next`; alamat numerik hanya menghasilkan keluarga yang
cocok tanpa query DNS tambahan. Nama non-numerik di-resolve oleh
resolver A-record IPv4 di libc: query UDP ke server DNS dari DHCP
(`oct_dns_server`), timeout 2 detik lewat `poll`, lalu hasilnya di-cache
(maksimal 8 entri, TTL dari record dengan batas 60–86400 detik). Record AAAA
di-resolve dengan query tipe 28 dan cache terpisah, lalu dipakai
`getaddrinfo` dengan `AF_INET6`. `getaddrinfo` dan `gethostbyname` memakai
cache A itu. Reverse DNS (PTR) tersedia
dengan cache terpisah: `gethostbyaddr` dan `getnameinfo` tanpa `NI_NUMERICHOST`
memakai nama hasil PTR bila berhasil, lalu jatuh ke bentuk numerik. AAAA dan
Server DNS dibaca dari baris `nameserver <IPv4>` pertama di
`/etc/resolv.conf` bila file itu ada; jika tidak, dipakai DNS dari DHCP kernel.
Stack IPv6 di kernel belum ada, jadi hasil AAAA baru bisa dipakai untuk
kebutuhan alamat/nama.

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

`make -C user cxa-test` menjalankan runtime asli di host untuk memeriksa urutan
handler `atexit`/`__cxa_atexit`, filter DSO pada `__cxa_finalize`, dan jaminan
bahwa handler tidak berjalan dua kali. `make -C user resolv-test` menguji parser
`/etc/resolv.conf` (spasi, komentar, CRLF, oktet tidak valid, dan beberapa baris
`nameserver`). `make -C user ipv6-test` menguji `inet_pton`/`inet_ntop` IPv6,
termasuk kompresi `::`, input tidak valid, serta parser record A dan AAAA dari
paket DNS sintetis.

## Toolchain native di dalam OCTAGRAM

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
umum atau dukungan relocation umum. Untuk jalur shell, panggilan `puts` dan
`printf` dengan literal diterjemahkan langsung menjadi syscall `write`; fungsi
eksternal lain, linking libc, dan fitur stdio lainnya belum tersedia. Section data belum
dipisahkan dari kode pada output ELF; program yang memakai global writable atau
inline assembly belum didukung. Gunakan toolchain host di atas untuk program
yang memerlukan fitur tersebut.

Fondasi syscall `mmap`/`munmap` belum menyediakan Cargo, Rust `core`, atau
runtime hosted di OCTAGRAM. Kernel Rust tetap dibangun dari host.

```text
as /mnt/hello.s -o /mnt/hello.o
ld /mnt/hello.o -o /mnt/hello.elf
cc /mnt/hello.c -o /mnt/hello.elf
run /mnt/hello.elf
```

Compiler subset `cc` hanya menerima satu fungsi `int main(void)`,
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

`cc` mencari header kutip relatif terhadap source, lalu di
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

Program membuat dan menunggu ELF anak melalui API POSIX `posix_spawn`/
`waitpid`, yang dibangun di atas `fork` dan `execve`. Contoh `spawn_demo`
menjalankan ELF yang diberikan:

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

`account_demo` menguji database `pwd`/`grp`: lookup uid/nama, iterasi, varian
`_r`, entri yang tidak ada, dan `ERANGE` untuk buffer kecil:

```text
run /mnt/account_demo.elf
```

`socket_msg_demo` menguji `sendmsg`/`recvmsg` melalui pasangan socket stream:
penggabungan beberapa iovec saat mengirim, penyebaran hasil terima ke beberapa
iovec, batas makro `CMSG_*`, penolakan data kontrol `ENOTSUP`, serta kesalahan
`EFAULT`/`EMSGSIZE` untuk argumen yang tidak valid:

```text
run /mnt/socket_msg_demo.elf
```

`netdb_demo` menguji `getaddrinfo` (flag `AI_PASSIVE`/`AI_CANONNAME`/numerik),
`getnameinfo` termasuk `EAI_OVERFLOW`, `gethostbyname`/`gethostbyaddr` numerik,
serta kode kesalahan `EAI_FAMILY`/`EAI_BADFLAGS`/`EAI_SOCKTYPE`/`EAI_SERVICE`
dan `h_errno`:

```text
run /mnt/netdb_demo.elf
```

`dns_demo` menguji resolver jaringan: `gethostbyname("example.com")`, panggilan
kedua yang dilayani cache, penolakan `AI_NUMERICHOST` untuk nama, dan
`getaddrinfo` dengan `AI_CANONNAME`. Program ini membutuhkan DHCP/DNS aktif:

```text
run /mnt/dns_demo.elf
```

`stdio_demo` menguji formatting ke buffer, operasi file melalui `FILE`, dan
output formatted ke stdout:

```text
run /mnt/stdio_demo.elf
```

`posix_spawn_demo` menguji environment khusus child, encoding status
`waitpid`, file actions `addopen`/`adddup2`/`addclose`, attribute
`POSIX_SPAWN_SETPGROUP`, `POSIX_SPAWN_SETSIGMASK`, `POSIX_SPAWN_SETSIGDEF`,
`POSIX_SPAWN_RESETIDS`, dan `POSIX_SPAWN_SETSCHEDULER`/`SETSCHEDPARAM` beserta
getter-nya, kebijakan `SCHED_FIFO` prioritas 10 pada anak, serta penolakan path
terlalu panjang dan prioritas RT di luar rentang:

```text
run /mnt/posix_spawn_demo.elf
```

`execve_demo` menguji pergantian image, pemuatan argv/environment, kegagalan
tanpa merusak image lama, dan descriptor `FD_CLOEXEC`:

```text
run /mnt/execve_demo.elf
```

`fork_demo` menguji nilai balik parent/child, salinan memori privat, dan
`waitpid`:

```text
run /mnt/fork_demo.elf
```

`time_demo` memeriksa `clock_gettime(CLOCK_REALTIME)` dan konversi kalender:

```text
run /mnt/time_demo.elf
```

`mmap_demo` menguji pemetaan anonim dan file-backed, `MAP_SHARED`/
`MAP_PRIVATE`, `MAP_FIXED`, `msync`, `mprotect`, `mincore`, serta operasi
locking dan advice POSIX:

```text
run /bin/mmap-demo
```

Nama perangkat bergantung hasil `devices`. Perintah `run` menjalankan ELF dan
kembali ke shell setelah syscall `exit` atau user fault. Untuk menjalankan
beberapa ELF CPU-bound secara bersamaan, gunakan:

```text
runp /mnt/worker-a.elf /mnt/worker-b.elf
```

`runp` memakai round-robin timer preemption. Total proses dan thread user yang
terjadwal dibatasi 16. Shell kembali setelah semua proses keluar atau fault.
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

- x86_64, ELF64 little-endian: static `ET_EXEC` dan PIE `ET_DYN` dengan
  interpreter `/lib/ld-octagram.so`.
- Dynamic loader mendukung subset relocation x86_64 (termasuk `COPY` untuk
data shared library pada executable non-PIC) dan pencarian shared library di
  `/lib`; simbol weak tak terdefinisi menjadi 0 dan symbol versioning
  (`DT_VERSYM`/`DT_VERDEF`/`DT_VERNEED`) dihormati. Shared libc OCTAGRAM bukan
  libc POSIX penuh. TLS untuk executable statis menggunakan model local-exec;
  TLS pada shared object belum didukung.
- Segmen tidak boleh writable sekaligus executable.
- Entry point harus berada di segmen executable.
- Stack 8 halaman dengan puncak yang digeser acak tiap `execve`, tanpa heap
  image beralamat tetap. `brk`/`sbrk` dan `malloc` tumbuh dari cursor `mmap`
  ber-entropi melalui pemetaan anonim internal yang dibatasi alamat virtual dan
  memori fisik yang tersedia.
- Startup meneruskan `argc`, `argv`, dan `envp` ke `main`; environment tersedia
  melalui argumen ketiga atau global `environ`.

Contoh [tty_demo.c](../user/examples/tty_demo.c) membaca satu baris dari
keyboard melalui `oct_read(OCT_STDIN, ...)`, lalu mencetaknya ke terminal.
Build bersama contoh lain dengan `make -C user`.

Lihat [ABI syscall](syscall-abi.md) dan [panduan port aplikasi](porting-apps.md)
untuk batasan aplikasi.

Untuk driver jaringan tethering Android via USB RNDIS dan batasan stack
jaringannya, lihat [Android USB tethering](android-usb-tethering.md).
