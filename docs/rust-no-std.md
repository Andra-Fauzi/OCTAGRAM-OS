# Rust `no_std` untuk OCTAGRAM-OS

Dokumen ini menjelaskan target Rust userspace native OCTAGRAM. Compiler masih
dijalankan pada mesin pengembangan (cross-compilation); hasilnya adalah ELF64
statis untuk dijalankan oleh kernel OCTAGRAM. Ini **belum** berarti `rustc` atau
Cargo sudah berjalan di dalam OCTAGRAM.

## Desain target

- Target: `x86_64-unknown-octagram`
- Target specification: `user/rust/targets/x86_64-unknown-octagram.json`
- Format executable: ELF64 x86-64, static `ET_EXEC`
- Runtime: `user/rust/runtime`, `#![no_std]`
- Core libraries: `core` dan `alloc`, dibangun dari Rust source dengan
  Cargo `-Z build-std=core,alloc`
- Linker: `ld.lld`, script `user/rust/linker.ld`
- Startup: `user/crt0.S`
- Syscall ABI: `int 0x80`, syscall number di RAX, argumen di RDI/RSI/RDX/R10/R8/R9,
  hasil negatif menandakan errno.

Target ini sengaja tidak menggunakan Linux syscall ABI atau libc Linux.
Runtime OCTAGRAM menyediakan wrapper syscall dan allocator anonymous-mmap.
Program tetap `no_std`; crate aplikasi boleh menggunakan `alloc` setelah
global allocator runtime tersedia.

## Persyaratan host

Gunakan Rust nightly dengan komponen `rust-src` dan linker LLD:

```sh
rustup toolchain install nightly --component rust-src --component rustfmt
sudo apt install lld binutils
```

## Build

Dari root repository:

```sh
make -C user/rust clean
make -C user/rust
make -C user/rust check
file user/rust/build/rust_hello.elf user/rust/build/make.elf
```

Cargo membangun `core` dan `alloc` untuk custom target, lalu menautkan
program statis. `rust_hello.elf` menguji output, allocator `Vec`, dan akses
file/stat melalui syscall OCTAGRAM. `make.elf` adalah implementasi make
userspace yang ditulis dengan Rust `no_std` + `alloc`.

Salin `user/rust/build/rust_hello.elf` ke filesystem yang dapat di-mount
OCTAGRAM, lalu jalankan:

```text
run /mnt/rust_hello.elf
```

Binary `make.elf` dapat dipasang sebagai `/bin/make` setelah pengujian
integrasi. Jangan menimpa make bawaan sebelum uji regresi selesai.

## Batasan saat ini

- Tidak ada Rust `std`; crate yang membutuhkan `std` belum dapat digunakan.
- Belum ada native `rustc` atau Cargo di OCTAGRAM.
- Runtime belum menyediakan seluruh API OS yang dibutuhkan oleh Rust `std`.
- Allocator saat ini memakai satu pemetaan anonymous `mmap` per alokasi dan
  `dealloc` mengembalikan pemetaan lewat `munmap` (syscall 17). Ini menghindari
  lock global yang dapat deadlock jika thread pemilik lock ter-preempt pada
  single CPU. Biayanya: syscall dan pemetaan per objek; allocator arena per-thread
  yang lebih efisien masih menjadi optimasi berikutnya.
- Perubahan allocator baru host-build/unit-test; guest QEMU regression wajib
  dijalankan sebelum dianggap stabil.
- Panic strategy adalah `abort`; unwinding belum didukung.
- Custom target harus diuji pada QEMU/hardware untuk memastikan fitur CPU,
  alignment ABI, linker output, dan syscall runtime sesuai dengan kernel.

## Kriteria penerimaan

1. `make -C user/rust clean all check` berhasil pada host dengan nightly,
   rust-src, LLD, dan binutils.
2. Kedua binary terdeteksi sebagai ELF64 little-endian x86-64 statis.
3. Kernel OCTAGRAM memuat `rust_hello.elf` sebagai user process.
4. Output marker, alokasi `Vec` lebih dari 1 MiB, `stat/open/read/close`
   semuanya berhasil.
5. `make.elf` dapat menjalankan build sederhana dari dalam OCTAGRAM.

## Continuous integration

Workflow `.github/workflows/rust-no-std.yml` menjalankan pemeriksaan pada
perubahan Rust userspace:

- Memasang nightly, `rust-src`, LLD, binutils, dan Make.
- Menjalankan `make -C user/rust clean all check` untuk membangun kedua ELF
  dengan custom target OCTAGRAM.
- Menjalankan unit test runtime pada target host Linux agar parser DNS dan
  primitive memori dapat diuji tanpa menjalankan syscall guest.
- Memeriksa format ELF dan program headers dari hasil build.

Workflow ini baru menjadi gate setelah satu run CI benar-benar selesai dan
lulus. Keberhasilan host unit test tidak menggantikan pengujian executable
pada QEMU; syscall, allocator mmap, ELF loader, dan integrasi filesystem tetap
memerlukan guest test.


## Hasil CI host (2026-11-22)

GitHub Actions run [45028941471](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/45028941471)
pada commit `c56ebef6d431c07ad65e3fe4e4b1435e250eab58` lulus seluruh job:

- Custom target JSON terbaca dan menghasilkan `target_os="octagram"`,
  `target_arch="x86_64"`, dan pointer width 64-bit.
- `rust_hello.elf` dan `make.elf` berhasil di-build.
- Keduanya terdeteksi sebagai ELF64 x86-64 `ET_EXEC`, static, dengan dua
  `PT_LOAD` segment dan tanpa dynamic interpreter.
- Tujuh unit test runtime pada host lulus, termasuk test `memset`,
  `memcpy`, `memmove`, `memcmp`, `strlen`, dan parser DNS.

Build awal saat ini memerlukan akses jaringan untuk mengambil dependency
sysroot Rust yang belum ada di cache (contohnya `rustc-literal-escaper`).
Mode offline baru aman digunakan setelah dependency tersebut di-vendor atau
di-cache. Hal ini tidak mengubah target akhir: native OCTAGRAM build harus
mempunyai dependency cache lokal yang lengkap.

**Batas verifikasi:** run ini menguji host cross-build dan unit test runtime.
Ia tidak menjalankan kedua ELF di QEMU. Pengujian QEMU sebelumnya untuk
custom target tercatat pada [run 44713729286](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/44713729286);
perubahan runtime `strlen` masih perlu masuk regresi guest.


## Allocator guest regression

The Rust guest smoke program now performs 32 independent `Vec` allocations and drops, checks `u128` alignment, then allocates again after releasing the previous batches. The QEMU syscall smoke test requires the marker `Rust allocator mmap/munmap cycle: PASS` in addition to the existing file/stat marker and successful process exit. This specifically exercises the allocator's syscall 16 (`mmap`) and syscall 17 (`munmap`) path; it does not replace stress testing under concurrent threads.


## Runtime OS primitives for future Rust std port

The freestanding runtime now also exposes:

- `Timespec` with the OCTAGRAM C ABI layout (two signed 64-bit fields);
- `clock_gettime` for syscall 50, with validation of nanoseconds;
- `getpid` for syscall 11;
- `getrandom` for syscall 121, split into requests of at most 256 bytes and preserving partial-read semantics.

Host unit tests check the timespec size/alignment/field offsets and clock ID constants. These wrappers are groundwork for a future `std::sys` backend, not evidence that Rust `std` is ported. Guest QEMU conformance for the new wrappers remains required.
