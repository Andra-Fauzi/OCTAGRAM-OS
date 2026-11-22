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
- Allocator saat ini memakai arena mmap monotonik; `dealloc` belum mengembalikan
  blok ke allocator.
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
