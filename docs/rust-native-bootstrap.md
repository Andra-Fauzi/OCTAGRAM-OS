# Native Rust compiler bootstrap

Dokumen ini memisahkan fakta toolchain saat ini dari pekerjaan yang diperlukan
untuk menjalankan `rustc` dan Cargo *di dalam* OCTAGRAM-OS. Target akhirnya
bukan sekadar menghasilkan ELF untuk OCTAGRAM dari Linux, melainkan menjalankan
compiler host-native pada OCTAGRAM.

## 1. Dua target yang berbeda

| Peran | Target | Tujuan |
| --- | --- | --- |
| Kernel dan aplikasi freestanding saat ini | `x86_64-unknown-none` | Menghasilkan ELF tanpa Rust `std`; runtime userspace OCTAGRAM menyediakan syscall sendiri |
| Host compiler masa depan | `x86_64-unknown-octagram` (target yang harus dibuat) | Menjalankan executable `rustc`, Cargo, LLVM/LLD, dan tool build di OCTAGRAM |

`--target x86_64-unknown-none` hanya mengubah kode yang dihasilkan. Itu
**tidak** membuat executable `rustc` yang sedang berjalan di Linux menjadi
program OCTAGRAM. Host triple dan target triple adalah dua konsep berbeda.

## 2. Fakta dari repository

- `user/rust/rust-toolchain.toml` memakai Rust nightly dan target
  `x86_64-unknown-none`.
- `user/rust/.cargo/config.toml` menetapkan linker `ld.lld`, static
  relocation, no-red-zone, mematikan SSE/SSE2 untuk runtime userspace, serta
  linker script `user/rust/linker.ld` dan `build/crt0.o`.
- `user/rust/Cargo.toml` adalah workspace kecil berisi `hello`, `make`,
  dan `runtime`. Runtime menggunakan `#![no_std]` dan syscall ABI OCTAGRAM
  melalui `int 0x80`.
- `kernel/rust-toolchain.toml` juga memakai nightly dengan
  `x86_64-unknown-none`.
- `kernel/Cargo.toml` memiliki dependency eksternal `limine`,
  `linked_list_allocator`, `spin`, dan `seq-macro`.
- `kernel/GNUmakefile` masih menjalankan Cargo host untuk membangun userspace,
  TinyCC, dynamic userland, dan kernel, lalu menyalin artefak hasil build.
- Build kernel menggunakan linker script high-half
  `kernel/linker-x86_64.ld`; script ini mengatur PT_LOAD, alignment halaman,
  request bootloader, dan BSS.

Karena itu, keberhasilan build userspace Rust dari host saat ini bukan bukti
bahwa OCTAGRAM dapat menjalankan Rust compiler.

## 3. Mengapa rustc belum bisa sekadar disalin

Executable compiler harus dibangun untuk *host OS* OCTAGRAM. Agar compiler
besar seperti rustc dapat berjalan, platform perlu menyediakan setidaknya:

- filesystem dan metadata POSIX-like yang konsisten;
- file descriptor, pipe, argument vector, environment, dan subprocess;
- virtual memory yang memadai: mmap, munmap, mprotect, dan pemetaan file;
- thread, TLS, mutex/condition variable, atomic, dan primitive sinkronisasi;
- clock, timer, entropy, signal/exception handling, serta exit status;
- dynamic loader dan ABI untuk shared libraries, atau build statis yang
  dependency-nya benar-benar tersedia;
- linker native dan backend code generation yang cocok untuk x86_64;
- sysroot, header, library, target specification, dan runtime yang konsisten.

Sebagian syscall dan fasilitas proses sudah ada di OCTAGRAM, tetapi daftar
di atas belum seluruhnya diaudit atau dibuktikan kompatibel dengan kebutuhan
Rust `std`, LLVM, dan Cargo. Jangan tandai komponen selesai sebelum ada test
program native khusus untuk API tersebut.

## 3.1. Fasilitas OS yang sudah ada dan batas yang terverifikasi

Dokumen syscall ABI saat ini sudah mencantumkan keluarga API berikut:

- file I/O, descriptor, `openat`, `fstatat`, direktori, pipe, dan subprocess
  melalui `fork`, `execve`, serta `waitpid`;
- virtual memory melalui `mmap`, `munmap`, `mprotect`, `brk`, dan operasi
  pemetaan terkait;
- thread melalui syscall OCTAGRAM `thread_create/join/self/exit`, pointer TLS
  FS, serta `futex`;
- clock/timer, random, signal, poll/epoll, dan socket API.

Ini adalah inventaris syscall, bukan klaim bahwa Rust `std` sudah kompatibel.
Setiap API masih memerlukan conformance test untuk semantics, error code,
blocking behavior, concurrency, dan edge case.

Batas penting yang sudah terdokumentasi:

- syscall `sendmsg` dan `recvmsg` mengembalikan `ENOSYS`; libc menyusun
  sebagian perilakunya dari syscall lain;
- dynamic loader OCTAGRAM saat ini memakai interpreter `ET_EXEC` dengan
  alamat tetap;
- loader belum mendukung TLS umum pada shared object (relokasi
  `TPOFF`/`DTPMOD` dan alokasi TLS per objek);
- dukungan relocation dan dynamic-linking masih subset, bukan ABI Linux penuh.

Artinya, sebelum compiler besar dijalankan, pilihan realistis antara
static-linking penuh dan perluasan dynamic loader harus ditentukan. TLS shared
object, thread, dan memory mapping merupakan area berisiko tinggi untuk
toolchain Rust/LLVM.

## 4. Urutan bootstrap yang direkomendasikan

### M4.1 — Host ABI dan target specification

- [ ] Dokumentasikan ABI host OCTAGRAM: syscall, errno, file descriptor,
      process, thread, TLS, memory mapping, clock, signal, dan dynamic loading.
- [ ] Buat target specification `x86_64-unknown-octagram` terpisah dari
      target freestanding `x86_64-unknown-none`.
- [ ] Tentukan ELF interpreter, dynamic/static linking policy, libc ABI,
      calling convention, TLS model, dan baseline CPU.
- [ ] Buat test conformance kecil untuk setiap fasilitas OS yang dibutuhkan
      host compiler.

### M4.2 — Runtime dan Rust standard library

- [ ] Lengkapi libc dan syscall wrapper sesuai kebutuhan `std`.
- [ ] Implementasikan thread/TLS dan synchronization primitive yang diperlukan.
- [ ] Implementasikan filesystem/process/environment/memory mapping.
- [ ] Port atau konfigurasi Rust `std` untuk host target OCTAGRAM.
- [ ] Jalankan test suite bertahap: file I/O, process, thread, mmap, TLS, dan
      panic/unwind policy.

### M4.3 — Native linker dan compiler backend

- [ ] Tentukan backend awal: LLVM/LLD atau backend alternatif yang realistis.
- [ ] Audit kebutuhan compiler backend terhadap thread, mmap, filesystem,
      dynamic loader, dan executable format OCTAGRAM.
- [ ] Port/build linker dan runtime compiler sebagai executable OCTAGRAM.
- [ ] Verifikasi linking binary Rust sederhana, termasuk relocation dan TLS.

TinyCC saat ini dapat menghasilkan object, static archive, dan ELF userspace
untuk smoke test. Itu belum setara dengan GNU `as`, GNU `ld`, atau LLD,
dan belum terbukti dapat menautkan kernel high-half dengan seluruh linker
script kernel.

### M4.4 — Stage 0 rustc dan Cargo

- [ ] Bangun Stage 0 rustc untuk host OCTAGRAM menggunakan bootstrap host
      yang tersedia di Linux.
- [ ] Jalankan `rustc --version`, kompilasi program Rust sederhana, link,
      dan jalankan hasilnya di OCTAGRAM.
- [ ] Port Cargo sebagai executable host OCTAGRAM.
- [ ] Pastikan Cargo dapat menjalankan rustc, membaca workspace/lockfile,
      mengelola cache offline, dan menjalankan build script yang dibutuhkan.
- [ ] Sediakan registry/vendor cache untuk dependency yang dipakai OS agar
      bootstrap awal tidak bergantung pada jaringan.

### M4.5 — Bootstrap berulang

- [ ] Gunakan Stage 0 rustc untuk membangun ulang rustc Stage 1.
- [ ] Gunakan Stage 1 untuk membangun Stage 2.
- [ ] Bandingkan artefak/build metadata dan dokumentasikan variasi yang
      memang tidak deterministik.
- [ ] Jalankan test suite compiler dan program OCTAGRAM dari dalam OCTAGRAM.

## 5. Build kernel dari dalam OCTAGRAM

Setelah compiler host-native dan linker yang sesuai tersedia:

1. Sediakan source tree lengkap dan dependency vendor pada filesystem persisten.
2. Bangun userspace Rust/C dan kernel dengan native build orchestrator.
3. Gunakan target freestanding `x86_64-unknown-none` untuk kernel; jangan
   mengganti target kernel menjadi host triple OCTAGRAM.
4. Gunakan linker script high-half kernel dan verifikasi ELF program headers,
   section, relocation, entry point, serta bootloader requests.
5. Jalankan test suite native dan boot image hasil build di QEMU.
6. Baru setelah itu integrasikan native image builder dan update/rollback.

## 6. Definisi bukti

| Klaim | Bukti yang wajib ada |
| --- | --- |
| Rust cross-compile | Host rustc menghasilkan ELF target freestanding |
| Rust program native | ELF Rust berjalan pada OCTAGRAM |
| rustc native | Executable rustc berjalan pada OCTAGRAM dan mengompilasi program |
| Cargo native | Cargo berjalan pada OCTAGRAM dan menjalankan rustc native |
| Rust self-hosting | rustc native membangun ulang rustc dan hasilnya lulus test |
| Kernel self-build | Cargo/rustc/linker native membangun kernel dari source di OCTAGRAM |
| Full OS self-hosting | Build, test, image, install/update, boot, dan rollback selesai tanpa host build tools |

Jangan menaikkan status milestone hanya karena source code sudah tersedia atau
karena cross-compile dari host berhasil.
