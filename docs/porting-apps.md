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
Format executable adalah ELF64 little-endian x86_64. Program statis memakai
`ET_EXEC` dan `user/linker.ld`; program dinamis menyertakan `PT_INTERP` yang
menunjuk ke `/lib/ld-octagram.so` dan dapat memakai PIE (`ET_DYN`). Gunakan
`-ffreestanding`, `-fno-stack-protector`, dan `-mno-red-zone` untuk objek
utama. Kernel menginisialisasi FPU dan menyimpan state FPU/SIMD per proses;
build C mengaktifkan SSE2 dan x87 serta menonaktifkan MMX. Segmen W+X ditolak.

### Dynamic linking awal

`make -C user dynamic` membangun loader user-space, shared libc OCTAGRAM,
`libdyn_demo.so`, dan `dynamic_demo.elf`. Setelah boot image, jalankan
`run /bin/dynamic-demo` untuk menguji `PT_INTERP`, pemetaan `DT_NEEDED`,
resolusi simbol, serta relocation.
Jalankan `run /bin/thread-demo` untuk menguji thread preemptive dan TLS.
Shared library dicari di `/lib`; executable dan library dapat memakai SysV
atau GNU ELF hash. Loader menangani relocation x86_64 `RELATIVE`, `GLOB_DAT`,
`JUMP_SLOT`, `64`, `PC32`, `32`, `32S`, dan `COPY`; simbol weak yang tidak
terdefinisi diselesaikan sebagai 0. Symbol versioning didukung: loader membaca
`DT_VERSYM`, `DT_VERDEF`, dan `DT_VERNEED`, lalu memilih definisi yang cocok
dengan versi yang diminta (`simbol@VERSI`) atau versi default (`simbol@@VERSI`)
untuk referensi tanpa versi. PIE memakai load bias acak dari kernel (`0x4000_0000`–`0x7000_0000`, kelipatan
halaman), dan shared library mengikuti kursor mmap berentropi (hingga 128
halaman) sehingga alamat muat berbeda tiap eksekusi. Puncak stack user juga
digeser 0–63 halaman per eksekusi, dan heap tumbuh dari cursor `mmap` yang juga
ber-entropi (tidak ada heap image beralamat tetap). Interpreter dinamis masih
di alamat tetap. Jalankan `run /bin/aslr-demo` dua kali untuk melihat perbedaan
alamat `main`, fungsi library, `mmap`, heap, dan variabel stack.

Pemuatan modul saat runtime tersedia lewat `<dlfcn.h>`: `dlopen` mencari
library di `LD_LIBRARY_PATH` lalu `/lib`, memetakan dan merelokasinya,
menjalankan initializer, dan memakai ulang library yang sudah dimuat (berdasarkan
nama yang diminta). `dlsym` mencari simbol pada handle hasil `dlopen`, pada
seluruh objek yang dimuat dengan `RTLD_DEFAULT`, atau pada objek setelah
pemanggil dengan `RTLD_NEXT` (objek pemanggil dikenali dari alamat
kembalinya). `dlerror` mengembalikan pesan terakhir, dan `dlclose` menurunkan hitungan
referensi lalu menjalankan destructor library serta `munmap` seluruh rentang
petanya saat referensi habis. Pointer simbol yang diperoleh sebelumnya menjadi
tidak valid setelah unload, dan library lain yang sudah direlokasi ke simbol itu
tidak diperbarui. Keempat fungsi libc bersifat weak agar stub
`dlopen` milik TinyCC (`CONFIG_TCC_STATIC`) tetap menang saat menautkan
`tcc.elf`. Dependency `DT_NEEDED` library yang di-dlopen belum dimuat otomatis.

Destructor didukung: `crt0` memanggil `exit()` setelah `main`, libc menjalankan
handler `atexit` dan `__cxa_atexit` dari satu daftar terbalik (maksimal 64
entri, argumen dan DSO dicatat), `__cxa_finalize(dso)` hanya menjalankan handler
milik DSO tersebut dan mengosongkannya agar tidak berjalan dua kali, dan loader
mendaftarkan `run_finalizers` lewat `atexit` sehingga `DT_FINI_ARRAY`/`DT_FINI`
library berjalan setelah handler aplikasi — executable lebih dulu, lalu library
dalam urutan muat terbalik. `_exit` dan `abort` tetap melewati handler.

Contoh `versioned_demo.elf` memakai library dengan version script
`ldso/dynlib.versions`; jalankan `run /bin/versioned-demo` untuk menguji dua
versi dari satu nama simbol. Executable non-PIC `ET_EXEC` yang mengakses data
dari shared library memakai relokasi `COPY`: loader menyalin nilai awal dari library ke salinan milik
executable, dan simbol data itu diekspor executable sehingga referensi library
menunjuk salinan yang sama. Contoh `make -C user dynamic` juga membangun
`copy_reloc_demo.elf`; jalankan `run /bin/copy-reloc-demo` untuk menguji salinan
data dua arah dan simbol weak yang tidak tersedia.

Untuk membuat shared object gunakan objek PIC dan GNU hash, lalu tautkan
executable PIE dengan interpreter OCTAGRAM:

```sh
gcc $CFLAGS -fPIE -c app.c -o build/app.o
gcc $CFLAGS -fPIC -c libexample.c -o build/libexample.o
ld -shared --hash-style=gnu -soname libexample.so \
  -o build/libexample.so build/libexample.o
ld --hash-style=gnu -pie -z now -dynamic-linker /lib/ld-octagram.so \
  -e _start -o build/app.elf build/crt0.o build/runtime.o \
  build/app.o -Lbuild -lexample
```

Salin `libexample.so` ke `/lib` pada image OS sebelum menjalankan `app.elf`.

Shared libc mengekspor subset runtime yang sudah tersedia, bukan libc POSIX
`pthread` penuh. Program statis x86_64 dapat memakai TLS model local-exec;
runtime membuat salinan `PT_TLS` per thread dan kernel menyimpan `FS.base`
secara terpisah saat preemption. Subset `pthread` menyediakan create, join,
self, exit, dan mutex spin; thread memakai address space bersama dan jumlah
task terjadwal dibatasi 16. Atribut thread selain default belum didukung.
Descriptor, cwd, dan environment disalin saat thread dibuat, sehingga perubahan
tabel descriptor tidak terlihat otomatis oleh thread lain. TLS pada shared
object dan TLS relocation pada dynamic loader belum didukung. Ini fondasi,
bukan `ld.so` Linux penuh: lazy binding dan pencarian library di luar `/lib`
belum didukung. Saat `dlclose` melepas library, loader menjalankan
`__cxa_finalize` untuk `__dso_handle` library itu agar destructor C++ ikut
berjalan. Binary tetap harus menggunakan syscall dan runtime
OCTAGRAM, bukan Linux ABI atau glibc.
Pada build statis, `errno`, `h_errno`, dan allocator free-list memakai TLS
per thread. Shared libc menggunakan `__errno_location()` dan
`__h_errno_location()` dengan tabel state per-TID, serta mengunci free-list
allocator saat alokasi/pembebasan. Ini tidak mengimplementasikan ELF TLS untuk
shared object; TLS umum pada shared library tetap belum didukung.

## Menyesuaikan aplikasi

1. Gunakan subset runtime C yang tersedia: `stdio` dasar, wrapper file POSIX
   (`open`, `read`, `write`, `stat`, `mkdir`, memory mapping, dan lainnya), dan
   syscall OCTAGRAM. Port hanya subset tambahan yang benar-benar dibutuhkan.
2. Hindari syscall Linux langsung; nomor syscall dan register Linux tidak
   digunakan OS ini.
3. Path relatif dimulai dari direktori shell saat program dijalankan (default
   `/`); gunakan `oct_chdir` dan `oct_getcwd` untuk
   mengubah atau membaca direktori kerja program. Semua program masih berbagi
   namespace VFS dan mount yang sama.
4. Aplikasi harus menangani hasil negatif `-errno`; descriptor file dimulai
   dari 3, stdin/stdout/stderr adalah 0/1/2.
5. Allocator libc memakai `mmap(MAP_PRIVATE | MAP_ANONYMOUS)` dan mengulang
   penggunaan blok yang dibebaskan; ia belum menggabungkan blok bersebelahan
   atau mengembalikan heap ke OS.
6. `oct_yield()` mengembalikan eksekusi sementara ke loop kernel agar pekerjaan
   driver tertunda dapat diproses, lalu melanjutkan program pada instruksi
   berikutnya. Gunakan untuk menunggu I/O yang belum memiliki syscall blocking.
7. API proses POSIX `fork`, `execve`, `execvp`, `waitpid`, process groups,
   dan subset `posix_spawn` tersedia. `fork` memakai eager copy; subset thread
   tersedia melalui `<pthread.h>`. Signal dan alternate stack terbatas
   tersedia; `setsid`/`setpgid` punya model dasar kernel. Socket IPv4 TCP `SOCK_STREAM` tersedia
   melalui `<sys/socket.h>` dan `<netinet/in.h>`; UDP datagram tersedia melalui
   API OCTAGRAM (`oct_udp_socket`, `oct_udp_bind`, `oct_udp_sendto`, dan
   `oct_udp_recvfrom`) di `octagram.h`. API socket masih terbatas: tidak ada
   IPv6, UDP POSIX, atau mode nonblocking. Operasi socket blocking mengulang
   `-EAGAIN` setelah `oct_yield()`. `oct_dns_server()` mengambil alamat DNS dari
   DHCP; libc C memakai alamat itu untuk resolver A-record dan PTR IPv4 dengan
   cache TTL (8 entri per arah, timeout 2 detik) yang dipakai
   `getaddrinfo`/`gethostbyname`/`gethostbyaddr`/`getnameinfo`, sedangkan
   runtime Rust menyediakan `dns_lookup()` untuk hostname dengan hasil
   IPv4 A-record. Dynamic executable dengan `PT_INTERP` dapat memakai loader
   terbatas yang dijelaskan di atas.
8. `clock_gettime(CLOCK_REALTIME)` dan `time()` membaca RTC dengan resolusi
   satu detik. Zona waktu dan `CLOCK_MONOTONIC` belum tersedia.
9. `<sys/mman.h>` menyediakan pemetaan anonim/file-backed, `MAP_SHARED`/
   `MAP_PRIVATE`, dan API pengelolaan pemetaan POSIX. Batas ruang alamat dan
   operasi yang tidak didukung dijelaskan di
   [`docs/syscall-abi.md`](syscall-abi.md).

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
