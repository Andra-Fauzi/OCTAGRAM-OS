# Rust userspace dan rencana bootstrap native

Direktori ini berisi target Rust OCTAGRAM, runtime freestanding, program
userspace, dan probe untuk mengukur kesiapan port Rust upstream.

## Status yang benar saat ini

- Host Rust nightly + Cargo dapat melakukan cross-build untuk target
  `x86_64-unknown-octagram`.
- Crate runtime `no_std` dan probe `no_std + alloc` berhasil dikompilasi.
- CI memverifikasi hasil ELF64 statis dan membangun probe upstream `std`
  sebagai eksperimen diagnostik.
- Binary `rustc` dan `cargo` native yang berjalan di OCTAGRAM **belum ada**.
- Karena itu, keberhasilan cross-build tidak boleh disebut native bootstrap.

## Build dari host

Dari root repository:

```sh
make -C user/rust clean all check
```

Toolchain yang dipakai ditentukan oleh `rust-toolchain.toml` (Rust nightly
dengan `rust-src` dan `rustfmt`). Build memakai target JSON
`targets/x86_64-unknown-octagram.json`, `core` dan `alloc` dari rust-src,
serta linker script OCTAGRAM. Output program berada di `user/rust/build/`.

Untuk menguji runtime di host:

```sh
cargo +nightly test --manifest-path user/rust/Cargo.toml \
  -p octagram-runtime --lib --target x86_64-unknown-linux-gnu
```

Tes host hanya menguji logika runtime yang dapat dijalankan pada host; tes ini
bukan bukti bahwa syscall OCTAGRAM bekerja.

## Batas ABI dan runtime

Target OCTAGRAM memakai ABI x86_64, ELF statis, model relokasi statis,
panic-abort, dan tidak menyediakan dynamic loader. Runtime freestanding
menggunakan syscall ABI kernel dan allocator sendiri untuk `alloc`.
Fitur yang bergantung pada thread, dynamic loading, unwinding, atau layanan
POSIX yang belum tersedia tidak boleh diasumsikan bekerja hanya karena
`core`/`alloc` berhasil dikompilasi.

## Urutan bootstrap resmi

1. **Stage 0 execution:** hasilkan toolchain bootstrap yang benar-benar dapat
   dieksekusi oleh loader OCTAGRAM. ELF yang hanya ditautkan untuk target belum
   cukup; jalankan dan uji seluruh dependency-nya di QEMU.
2. **Stage 1 no_std:** dari terminal OCTAGRAM, kompilasi crate kecil
   `#![no_std]` menjadi ELF target OCTAGRAM, lalu jalankan.
3. **Stage 1 core/alloc:** bangun sysroot `core` dan `alloc` secara native
   dengan versi compiler yang dipin.
4. **Cargo offline:** jalankan Cargo native untuk workspace lokal tanpa
   mengakses jaringan. Cache/source dependency harus sudah tersedia di disk.
5. **Stage 2:** gunakan toolchain Stage 1 untuk membangun ulang compiler dan
   Cargo; bandingkan artefak dan jalankan kembali tes.
6. **Perluasan bertahap:** baru setelah gate tersebut, kerjakan build-script,
   proc-macro, networking/crates.io, dan dukungan `std` yang lebih lengkap.

## Gate penerimaan native

Milestone native tidak dianggap lulus sampai semua ini direkam sebagai tes
guest OCTAGRAM:

- `rustc --version` berjalan sebagai proses OCTAGRAM.
- `rustc` mengompilasi crate `no_std` lokal menjadi ELF yang dapat dieksekusi.
- `cargo build --offline` membangun workspace lokal tanpa host.
- Toolchain hasil build dapat membangun ulang dirinya sendiri (Stage 2).

## Aturan pelaporan

Pisahkan hasil **host cross-build**, **QEMU guest execution**, dan **hardware**.
Jangan menyamakan compile sukses dengan runtime sukses, dan jangan menandai
Stage 0/1 selesai sebelum executable native benar-benar dijalankan.
