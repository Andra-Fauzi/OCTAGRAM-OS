# Porting CPython ke OCTAGRAM-OS

## Status

**CPython belum dapat dijalankan langsung di OCTAGRAM-OS.** Kernel sudah
memiliki sebagian besar fondasi user-mode yang dibutuhkan interpreter, tetapi
belum ada port CPython, libc POSIX penuh, binary `python`, atau standard
library Python di image OS.

ELF64 yang dibuat untuk Linux juga tidak kompatibel hanya karena arsitekturnya
sama. Program Linux memakai Linux syscall ABI dan runtime Linux; OCTAGRAM
memakai `int 0x80`, nomor syscall, register argumen, errno, dan runtime sendiri.
Interpreter harus dibangun untuk ABI OCTAGRAM.

## Fondasi yang sudah tersedia

Di x86_64, kernel saat ini menyediakan:

- ELF64 `ET_EXEC` dan dynamic PIE terbatas melalui `PT_INTERP` serta
  `/lib/ld-octagram.so`.
- File descriptor dasar, `openat`, `read`, `write`, `close`, `lseek`,
  `fstat`, `fstatat`, direktori, pipe, dan VFS.
- `mmap`, `munmap`, `mprotect`, serta program break `brk` yang terpisah
  dari cursor `mmap`. Break memiliki presisi byte; halaman yang pernah
  dipetakan dipertahankan ketika break menyusut.
- `fork`, `execve`, `waitpid`, TLS, thread, futex, dan subset signal.
- `clock_gettime`, `nanosleep`, `uname`, `getrandom`, `poll`, dan
  `select`.

Daftar syscall dan register ABI tetap berada di
[`syscall-abi.md`](syscall-abi.md). Batas kompatibilitas libc berada di
[`porting-apps.md`](porting-apps.md).

## Hambatan yang masih harus diselesaikan

1. **Port libc dan konfigurasi platform.** `user/runtime.c` adalah runtime
   freestanding kecil, bukan libc POSIX lengkap. CPython memerlukan konfigurasi
   platform (`pyconfig.h`), fungsi libc/header yang sesuai, dan implementasi
   atau penonaktifan fitur yang tidak tersedia. Beberapa fungsi runtime saat
   ini masih mengembalikan `ENOSYS`.
2. **Dynamic extension loading.** Loader kernel dapat memulai executable
   dengan `PT_INTERP`, tetapi loader user-space belum menyediakan API
   `dlopen`/`dlsym` yang setara untuk memuat extension module pada saat
   runtime. Tahap awal harus menautkan extension yang dipilih secara statis
   sebagai built-in modules.
3. **Semantik POSIX yang belum penuh.** Beberapa operasi dan struktur POSIX
   masih berupa subset. Contohnya `sendmsg`/`recvmsg` belum
   diimplementasikan; perilaku signal, clock, socket, dan terminal juga belum
   setara Linux.
4. **Randomness.** Implementasi syscall `getrandom` sekarang memakai PRNG
   sederhana berbasis TSC, bukan sumber entropy kriptografis yang layak untuk
   `secrets`, TLS, atau token keamanan. Jangan anggap `os.urandom` aman
   sebelum kernel menyediakan CSPRNG yang benar.
5. **Filesystem dan paket.** Root filesystem awal adalah RAMFS volatile dan
   belum berisi interpreter maupun standard library. Penyimpanan persisten
   dapat dipasang, tetapi Python dan file library-nya tetap perlu dipaketkan.
6. **Timing dan blocking.** `nanosleep` saat ini menunggu secara aktif di
   kernel dan memiliki batas per pemanggilan. Ini cukup untuk eksperimen awal,
   bukan implementasi scheduler/timer POSIX yang lengkap.

## Urutan port yang disarankan

### 1. Uji toolchain dan runtime C

Sebelum membangun CPython, pastikan toolchain target dapat membangun program C
dinamis dan statis sederhana, termasuk pemanggilan file, alokasi memory,
`clock_gettime`, dan exit. Jalankan seluruh demo user yang sudah ada di QEMU.
Jangan memakai binary Linux host sebagai pengganti binary target.

### 2. Buat konfigurasi platform CPython

Port perlu menyediakan `pyconfig.h` yang sesuai dengan ABI, tipe data,
alignment, header, dan fungsi yang benar-benar tersedia. Gunakan hasil
pengujian target untuk mengisi konfigurasi; jangan menyalin nilai
`HAVE_*` dari Linux.

Mulai dengan interpreter statis dan built-in modules yang sedikit. Nonaktifkan
extension dan fitur yang belum didukung. Setelah prompt dasar berjalan,
tambahkan modul satu per satu.

### 3. Capai prompt minimal

Target milestone pertama:

```text
$ python -S
>>> 1 + 2
3
>>> print("OCTAGRAM")
OCTAGRAM
```

Opsi `-S` menonaktifkan import `site` saat startup. Ini membantu memisahkan
masalah interpreter inti dari masalah instalasi package dan path.

### 4. Tambahkan dukungan bertahap

Setelah prompt minimal stabil, uji secara terpisah:

- `sys`, `os`, `io`, `time`, `errno`, dan `pathlib`.
- Operasi file lebih besar dari satu halaman dan short read/write.
- `threading`, lock, dan kondisi menggunakan TLS/futex.
- `subprocess`, pipe, signal, dan wait.
- Modul extension statis; dynamic loading dapat menyusul setelah API loader
  tersedia.
- Randomness kriptografis sebelum mengaktifkan pemakaian untuk keamanan.

Jalankan test suite CPython secara bertahap dan tandai test yang memang tidak
relevan atau belum didukung oleh platform.

## Referensi porting

- [CPython: Porting and platform support](https://devguide.python.org/developer-workflow/porting/)
- [CPython configure options and cross-compilation](https://docs.python.org/3/using/configure.html)

Panduan upstream CPython menyarankan memulai dari program C yang berjalan di
toolchain target, menghasilkan konfigurasi platform sendiri, mendapatkan
prompt interpreter minimal terlebih dahulu, dan menambahkan modul yang
dibutuhkan secara bertahap.
