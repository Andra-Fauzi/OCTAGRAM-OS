# Pengembangan Program Userland Berbasis Rust

Dokumentasi ini menjelaskan arsitektur dan tata cara menulis, mengompilasi, serta menjalankan
aplikasi pengguna (userspace) yang ditulis dalam bahasa Rust murni untuk OCTAGRAM-OS.
Berkas proyek pengguna Rust berada di subdirektori `user/rust/`.

---

## 1. Arsitektur Userspace Rust

Meskipun OCTAGRAM-OS menyediakan pustaka libc bergaya C (`user/runtime.c`) untuk program C,
sistem operasi ini juga mendukung pengembangan aplikasi mandiri berkinerja tinggi menggunakan
Rust dalam mode `#![no_std]`.

Struktur subdirektori `user/rust/`:
```text
user/rust/
├── Cargo.toml               # Workspace konfigurasi Cargo
├── rust-toolchain.toml      # Toolchain target x86_64-unknown-none
├── linker.ld                # Skrip linker untuk executable ELF userspace
├── Makefile                 # Otomasi kompilasi dan penyalinan biner ke build/
├── runtime/                 # Crate runtime: allocator & syscall bindings
├── hello/                   # Contoh aplikasi sederhana "Hello World"
└── make/                    # Implementasi utilitas 'make' berbasis Rust
```

Target kompilasi yang digunakan adalah target bare-metal Rust `x86_64-unknown-none`.

---

## 2. Crate Runtime (`user/rust/runtime/`)

Crate `octagram-runtime` adalah pustaka pendukung no-std yang menjembatani program Rust dengan
kernel OCTAGRAM-OS:

### Alokator Memori Dinamis (`alloc`)
- Mengimplementasikan trait `core::alloc::GlobalAlloc`.
- Memanfaatkan alokasi memori halaman anonim melalui syscall `mmap` (Syscall 9) dan `munmap` (Syscall 11).
- Mengaktifkan fitur `alloc` standar Rust sehingga tipe data seperti `Vec`, `String`, `Box`, dan `BTreeMap` dapat digunakan secara penuh di userspace.

### Pembungkus Syscall ABI (`int 0x80`)
- Menyediakan wrapper perakitan inline (`core::arch::asm!`) untuk instruksi software interrupt `int 0x80`.
- Menyediakan konstanta deskriptor standar:
  - `STDIN = 0`
  - `STDOUT = 1`
  - `STDERR = 2`
- Menyediakan fungsi I/O tingkat tinggi, antara lain:
  - `write_all(fd, &[u8]) -> Result<usize, i64>`
  - `read(fd, &mut [u8]) -> Result<usize, i64>`
  - `open(path, flags, mode) -> Result<u64, i64>`
  - `close(fd) -> Result<(), i64>`
  - `exit(code) -> !`
  - `fork() -> Result<u64, i64>`
  - `execve(path, argv, envp) -> Result<!, i64>`

---

## 3. Struktur Program Rust Userspace

Setiap program pengguna berbasis Rust harus mendefinisikan atribut no-std, entry point `main`,
panic handler, dan alokasi error handler:

```rust
#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

use alloc::vec::Vec;
use core::alloc::Layout;
use core::panic::PanicInfo;
use octagram_runtime::{exit, write_all, STDOUT};

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    // Alokasi memori dinamis di userspace OCTAGRAM
    let mut pesan = Vec::new();
    pesan.extend_from_slice(b"Halo dari aplikasi Rust di OCTAGRAM-OS!\n");

    if write_all(STDOUT, &pesan).is_err() {
        return 1;
    }

    0
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    let _ = write_all(STDOUT, b"Rust program panicked\n");
    exit(101)
}

#[alloc_error_handler]
fn allocation_error(_layout: Layout) -> ! {
    let _ = write_all(STDOUT, b"Rust allocation failed\n");
    exit(127)
}
```

Titik masuk sebenarnya sebelum `main()` dipanggil adalah `crt0.S` (`user/crt0.S`), yang menyiapkan register dan argumen stack sebelum melompat ke fungsi `main`.

---

## 4. Kompilasi dan Build

### Membangun Biner di Host Linux
Dari root direktori repositori:

```sh
# Mengompilasi seluruh program Rust userspace
make -C user/rust
```

Perintah ini akan:
1. Merakit `crt0.S` menjadi `build/crt0.o`.
2. Menjalankan `cargo build --release --target x86_64-unknown-none`.
3. Menautkan objek dengan `linker.ld`.
4. Menghasilkan biner ELF di `user/rust/build/`:
   - `user/rust/build/rust_hello.elf`
   - `user/rust/build/make.elf`

### Memasukkan ke Image ISO
Biner yang dihasilkan otomatis disertakan ke dalam direktori root RAMFS ISO saat `make iso` atau `make run` dijalankan, biasanya diletakkan pada `/bin/rust_hello.elf`.

---

## 5. Menjalankan di OCTAGRAM-OS

Dari shell OCTAGRAM-OS, jalankan program menggunakan perintah `run` atau `runp`:

```text
# Menjalankan secara serial
run /bin/rust_hello.elf

# Atau jika biner berada di PATH (/bin)
run rust_hello.elf

# Menjalankan bersamaan secara paralel dengan preemption
runp /bin/rust_hello.elf /bin/hello.elf
```

---

## 6. Batasan Saat Ini

1. **Standard Library Penuh (`std`)**: Belum ada port pustaka standar `std` resmi; saat ini menggunakan kombinasi `core` dan `alloc` dengan modul runtime kustom.
2. **Pustaka Pihak Ketiga (Crates.io)**: Crate eksternal yang memerlukan `std` (seperti `std::fs`, `std::net`, atau `std::thread`) tidak dapat digunakan secara langsung tanpa diadaptasi ke no-std atau libc OCTAGRAM.
3. **Format Biner**: Biner harus berupa ELF64 statis (`ET_EXEC`); shared library dinamis (`.so`) untuk Rust belum didukung.
