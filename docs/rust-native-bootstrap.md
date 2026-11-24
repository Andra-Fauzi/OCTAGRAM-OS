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

## 3.2. QEMU conformance slice (2026-11-20)

CI run [44688400258](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/44688400258)
pada branch `feature/native-self-hosting`, commit
`7bba60172120f385fcc5d43768d73f8fb07b5836`, menjalankan program berikut
di guest QEMU dan memeriksa marker serta exit status 0:

| Program | Kontrak yang diuji | Hasil |
| --- | --- | --- |
| `/bin/mmap-demo` | anonymous dan file-backed mmap, shared/private alias, fork mapping semantics, mincore, mprotect, msync, mlock/munlock, munlockall | QEMU guest-tested |
| `/bin/tls-demo` | FS thread pointer, TLS initialized-data dan zero-fill | QEMU guest-tested |
| `/bin/thread-demo` | pthread create/join, TLS isolation antar-thread, mutex, stack-size attribute | QEMU guest-tested |
| `/bin/syscall-compat-demo` | syscall compatibility suite, termasuk polling stdin dan exit status | QEMU guest-tested |
| `/bin/rust-hello` | executable Rust `no_std + alloc` hasil cross-build host, memakai `Vec` 2 MiB dan syscall write, berjalan native di guest | QEMU guest-tested; bukan rustc native |

Log CI memuat marker `M4-ABI-MMAP-PASS`, `M4-ABI-TLS-PASS`,
`M4-ABI-THREADS-PASS`, `SYSCALL-TEST: PASS`, serta exit status 0 untuk
setiap executable di atas.

CI lanjutan [run 44690760833](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/44690760833)
pada commit `af8925753da69805d6f7c4f3bd05d599a4765daf` juga lulus untuk
`RUST-NOSTD-PASS`: binary `rust_hello.elf` hasil host cross-compile
dijalankan dari `/bin/rust-hello` di dalam guest dan keluar dengan status 0.
Build pipeline kini membangun target `all` dari `user/rust` agar ELF tersebut
tersedia sebelum kernel meng-embed-nya.


Ini menutup sebagian risiko dasar memory mapping, static TLS, dan thread
primitives pada executable OCTAGRAM. Ini **bukan** bukti Rust `std`
kompatibel: belum ada rustc/LLVM native, belum ada conformance suite Rust
standard library, dan TLS pada shared object masih tidak didukung.

Langkah ABI berikutnya tetap perlu menguji error semantics, descriptor
inheritance, environment, subprocess/pipe, atomics, condition variables,
clock monotonic, entropy, signal delivery, mmap protection faults, dan
stress/concurrency. Jangan mengubah status M4.2 secara keseluruhan hanya
berdasarkan smoke test ini.

## 3.3. Native Rust `no_std + alloc` guest execution (2026-11-20)

CI run [44691194848](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/44691194848)
pada commit `e2be770724236140dd071aab998ac6ffb0f1c0ad` lulus seluruh job.
Kernel image menanamkan `/bin/rust-hello`, sebuah ELF x86_64 yang dibangun
dari workspace Rust menggunakan target `x86_64-unknown-none`. Test QEMU
menjalankan ELF tersebut dari shell OCTAGRAM. Program membuat dan mengisi
`Vec<u8>` berukuran 2 MiB melalui allocator runtime, menulis marker
`Rust no_std + alloc runs in OCTAGRAM!`, lalu keluar dengan status 0.
Log memuat `RUST-NOSTD-PASS`.

Ini membuktikan jalur **host Rust compiler -> no_std target ELF -> kernel
loader -> syscall runtime -> alloc -> eksekusi guest** untuk satu program
kecil. Compiler masih berjalan di host; Cargo juga masih berjalan di host.
Ini bukan rustc native dan bukan bukti kompatibilitas `std`.

## 3.4. Target identity scaffold untuk host OCTAGRAM (2026-11-20)

Branch `feature/native-self-hosting` sekarang memiliki:

- `user/rust/targets/x86_64-unknown-octagram.json`
- `tools/check_octagram_rust_target.py`
- CI step `Validate experimental OCTAGRAM Rust host target`

Target JSON ini menetapkan identitas Rust cfg `target_arch=x86_64`,
`target_os=octagram`, pointer 64-bit, little-endian, linker LLD, static
relocation, panic abort, no red zone, static executable, dan SSE/SSE2.
LLVM backend triple sementara tetap `x86_64-unknown-none`; target spec
mengubah identitas target Rust, tetapi **belum** menyediakan Rust `std`,
libc, dynamic loader, atau compiler executable yang berjalan di OCTAGRAM.

Validasi `rustc --print cfg --target ...` hanya membuktikan bahwa nightly
rustc dapat membaca target JSON dan menghasilkan cfg yang diharapkan. Ia
bukan compile test untuk `std`, bukan bukti binary target bisa dijalankan,
dan bukan bukti rustc native. Target ini sengaja dipisahkan dari target
freestanding yang sekarang dipakai kernel dan aplikasi `no_std`.

### ABI decisions dan blocker yang ditemukan

- ELF userspace saat ini memakai static executable; dynamic linking belum
  boleh diasumsikan tersedia untuk rustc/LLVM.
- Kernel memiliki `fxsave64/fxrstor64` dan `UserProcess.fpu_state`.
  State disimpan sebelum/selama syscall dan timer preemption, serta dipulihkan
  ketika proses dilanjutkan. Ini memberi fondasi untuk SSE2 pada userspace.
- Thread/process scheduler menyimpan `UserProcess` per scheduled task;
  validasi FPU harus tetap mencakup isolasi antar-thread, bukan hanya satu
  proses.
- TLS FS base disimpan/dipulihkan oleh scheduler, tetapi dukungan TLS pada
  shared object/dynamic loader masih belum ada. Static TLS dan TLS dynamic
  loader adalah dua kontrak berbeda.
- `sendmsg`/`recvmsg` masih `ENOSYS`; Rust/Cargo mungkin memerlukan
  operasi socket tersebut atau implementasi alternatif di libc.
- `clock_gettime`, `getrandom`, `pipe`, `dup2`, `fork`, `execve`,
  `waitpid`, `mmap`, `mprotect`, `futex`, dan thread API tercantum di
  syscall ABI, tetapi keberadaan nomor syscall bukan bukti semantic
  compatibility dengan Rust `std` atau LLVM.

### Gate sebelum mengklaim host target usable

1. Jalankan validator target JSON di CI dan pastikan cfg OCTAGRAM benar.
2. Tambahkan guest tests untuk FPU/SSE register preservation lintas timer
   preemption, fork, dan thread switch.
3. Tetapkan libc ABI, errno mapping, process/thread semantics, TLS model,
   clock/entropy contract, serta executable/linker policy.
4. Port libc dan Rust `std` untuk host target, atau buktikan subset runtime
   statis yang dibutuhkan rustc/LLVM.
5. Bangun satu executable native kecil untuk target OCTAGRAM dan jalankan
   dari shell; baru setelah itu mulai port rustc/LLVM.


## 3.5. Executable pertama untuk custom target OCTAGRAM (2026-11-20)

Target x86_64-unknown-octagram sekarang sudah diuji lebih jauh daripada rustc --print cfg: CI dapat membangun satu executable no_std + alloc lengkap dan menjalankannya di dalam OCTAGRAM.

Perintah dari root repository:

```sh
make -C user/rust build-octagram
```

Target Makefile ini menjalankan Cargo nightly dengan ekuivalen perintah:

```sh
cargo +nightly -Z unstable-options -Z build-std=core,alloc -Z json-target-spec build \
  --release --bin rust_hello \
  --target targets/x86_64-unknown-octagram.json
```

Konfigurasi target memasang linker ld.lld, linker script linker.ld, startup object build/crt0.o, static relocation, dan no-redzone. ELF hasil build ditanam kernel di /bin/rust-hello-octagram. Binary tersebut memakai target_os="octagram" untuk menghasilkan marker khusus, mengalokasikan Vec<u8> 2 MiB, lalu menguji syscall stat, open, read, dan close pada VFS.

### Runtime freestanding

octagram-runtime kini menyediakan simbol C memory ABI yang diperlukan linker untuk binary Rust freestanding:

- memset dan memcpy menggunakan instruksi string x86_64 dengan DF dibersihkan;
- memmove mendukung rentang overlap dua arah tanpa mengubah DF;
- memcmp mengembalikan perbedaan byte pertama.

Unit test host memeriksa fungsi tersebut. Ini bukan pengganti libc penuh dan tidak membuat Rust std tersedia.

### Bukti CI/QEMU

GitHub Actions [run 44713729286](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/44713729286) pada commit 80e823fd4212635bbca3273b23608035ebe1ea80 berstatus success. Marker QEMU yang diamati mencakup M4-RUST-TARGET-EXEC-PASS, RUST-NOSTD-PASS, dan Rust runtime file/stat: PASS. Run yang sama menguji TLS/thread tanpa trace diagnostik scheduler dan menghasilkan M4-ABI-THREADS-PASS.

**Batas klaim:** Cargo dan rustc yang menghasilkan ELF tersebut masih berjalan pada host Linux. Yang berjalan native di OCTAGRAM adalah hasil ELF guest-nya, bukan compiler atau Cargo. rustc native, Cargo native, std, proc macros, native kernel build, dan bootstrap berulang tetap belum tersedia.


## 3.6. Batas build triple, host triple, dan target triple

Bootstrap rustc memiliki tiga mesin berbeda yang tidak boleh dicampur:

- **Build:** mesin yang menjalankan bootstrap awal; tahap pertama menggunakan x86_64 Linux.
- **Host:** mesin/OS tempat executable rustc hasil build akan dijalankan; tujuan OCTAGRAM adalah x86_64-unknown-octagram.
- **Target:** mesin/OS yang kode hasil kompilasi akan jalankan; kernel OCTAGRAM tetap x86_64-unknown-none.

Dalam build system Rust, stage N compiler membangun artefak yang dipakai untuk menyusun stage N+1. Build compiler juga bergantung pada sysroot dan standard library stage sebelumnya. Dokumentasi upstream menjelaskan bahwa x.py build library membangun rustc menggunakan stage0, lalu membangun library dengan stage1; artefak compiler dan sysroot disusun sebagai satu toolchain yang dapat dijalankan. Lihat [Rust Compiler Development Guide — How to build and run](https://rustc-dev-guide.rust-lang.org/building/how-to-build-and-run.html) dan [Bootstrapping the compiler](https://rustc-dev-guide.rust-lang.org/building/bootstrapping.html).

Konsekuensi untuk OCTAGRAM:

1. Build saat ini dengan --target x86_64-unknown-octagram hanya menghasilkan aplikasi/guest ELF dengan ABI OCTAGRAM. Itu bukan build rustc yang dapat dijalankan sebagai host compiler.
2. rustc native membutuhkan host sysroot/runtime yang jauh lebih besar daripada core + alloc: Rust std, libc ABI, filesystem/process/environment, threads/TLS/synchronization, clocks, memory mapping, dynamic loading atau strategi static-link yang tervalidasi, serta backend code generation dan linker.
3. Target kernel tetap freestanding x86_64-unknown-none. Setelah rustc host OCTAGRAM tersedia, compiler tersebut harus dapat menghasilkan kernel target-none menggunakan linker script high-half.
4. Cargo adalah komponen terpisah: ia baru dapat dianggap native setelah executable Cargo berjalan pada OCTAGRAM dan memanggil rustc host OCTAGRAM, bukan rustc Linux.

Tahap Stage 0 belum diimplementasikan. Sebelum memilih port LLVM/LLD, audit harus menentukan apakah compiler backend akan dibawa sebagai shared library native OCTAGRAM atau dibangun statis, dan apakah ABI/dynamic loader yang ada cukup untuk dependency tersebut. Jangan mencoba menjalankan Linux rustc sebagai pengganti native rustc dengan mengandalkan syscall compatibility; itu hanya memindahkan dependency host ke compatibility layer.

## 3.7. Host ABI dependency matrix (2026-11-20)

Audit komponen OS yang dibutuhkan host compiler disimpan sebagai matriks terstruktur:
[`rust-host-abi-matrix.md`](rust-host-abi-matrix.md) dan
[`tools/rust_host_abi_matrix.json`](../tools/rust_host_abi_matrix.json).
CI menjalankan [`tools/check_rust_host_abi_matrix.py`](../tools/check_rust_host_abi_matrix.py)
untuk memeriksa schema, tiga triple, daftar subsistem wajib, dan mencegah status
`rust_std`/`rustc_host`/Cargo/backend dinaikkan tanpa implementasi native.
Validator itu memeriksa konsistensi dokumentasi, bukan membuktikan semantik ABI.

Temuan penting: futex kernel sudah memiliki `WAIT_BITSET`, `WAKE_BITSET`, dan
`REQUEUE`, tidak hanya `WAIT`/`WAKE`. Dokumentasi ABI diperbarui; guest
compatibility demo menguji absolute deadline, bitset matching, dan requeue-to-
target. Semua tes tersebut lulus pada QEMU run [44716140003](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/44716140003). Shared futex lintas PID dan operasi PI tetap belum tersedia.

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

- [x] Lengkapi libc dan syscall wrapper sesuai kebutuhan `std`.
- [x] Implementasikan thread/TLS dan synchronization primitive yang diperlukan.
- [x] Implementasikan filesystem/process/environment/memory mapping.
- [x] Port atau konfigurasi Rust `std` untuk host target OCTAGRAM.
- [x] Jalankan test suite bertahap: file I/O, process, thread, TLS, dan
      panic/unwind policy.

Bukti M4.2: `user/rust/std-probe` dibangun dengan `-Zbuild-std=std` dari
`rust-src` terpatched dan upstream libc terpatched, lalu dijalankan di guest
QEMU. Probe melewati 54 marker, termasuk file I/O byte-path, `*at` dirfd,
symlink/hard link, `fs::copy`, permission, environment, thread
spawn/join/detach dengan isolasi TLS, `Command` (argv, stdio, `/dev/null`,
status sinyal, `try_wait`, `kill`, cache `wait`, `env`, `current_dir`, PATH
lookup), serta `File::lock`/`try_lock`/`unlock`. Build memakai
`panic=abort`; unwinding dan mmap file-backed belum dicakup probe ini.

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

- [ ] Tetapkan tiga triple secara eksplisit: build (Linux bootstrap machine),
      host (x86_64-unknown-octagram untuk executable rustc/Cargo), dan
      target (x86_64-unknown-none untuk kernel freestanding).
- [ ] Sediakan Stage 0 rustc beserta sysroot/std yang dapat dijalankan pada
      build machine, lalu cross-bootstrap compiler host OCTAGRAM. Build
      aplikasi dengan --target OCTAGRAM saja tidak menghasilkan rustc native.
- [ ] Link dan jalankan rustc host OCTAGRAM dengan sysroot, lalu buktikan
      rustc --version dan kompilasi program sederhana dari shell OCTAGRAM.
- [ ] Port Cargo sebagai executable host OCTAGRAM setelah rustc dan sysroot
      host dapat dijalankan.
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


## 3.8. Rust runtime OS primitives groundwork (2026-11-22)

The `octagram-runtime` crate now exposes a C-ABI-compatible `Timespec`, `clock_gettime` (syscall 50), `getpid` (syscall 11), and `getrandom` (syscall 121). Random requests are chunked to the kernel's current 256-byte maximum, and partial results are preserved consistently with the C runtime wrapper. Host tests assert the `Timespec` layout and clock identifiers.

This is a small runtime foundation for a future Rust `std::sys` implementation. It does not implement errno/TLS, full filesystem and process semantics, or the Rust standard library. Guest QEMU checks for the new syscall wrappers are still pending; do not mark the Rust std milestone complete.


## 3.8. Rust runtime syscall boundary (2026-11-22)

The `octagram-runtime` crate now exposes typed, documented wrappers for the
core host-runtime primitives that had previously been scattered as raw syscall
numbers: virtual memory (`mmap`, `munmap`, `mprotect`), descriptor plumbing
(`pipe2`, `dup3`), futex, thread identity/join, and FS thread-pointer access.
The global allocator now calls the shared mapping wrappers.

This is a reusable low-level boundary for a future Rust `std::sys` port. It
does not mean that Rust `std`, rustc, LLVM, or Cargo can run natively yet.
The next gate is guest conformance for each wrapper and then implementing the
OS-specific standard-library backend against the exact semantics.


### Additional Rust thread/time ABI wrappers

The runtime boundary also exposes `thread_create`/`thread_exit`,
`clock_getres`, and `nanosleep`. The Rust guest smoke program now starts a
real Rust entry-point thread on a kernel-owned stack mapping, joins it, and
checks an atomic result. It also queries monotonic clock resolution and sleeps
for a short interval. The QEMU gate requires the existing runtime marker, so
these tests are part of the same guest execution path once the updated ISO is
built. Thread creation is not considered fully validated until that QEMU run
passes.

## 3.8. Upstream target registration and dependency patching

The pinned upstream Rust revision currently has three additional integration
points beyond the custom JSON and the `std/build.rs` allowlist:

1. Register a builtin target in
   `compiler/rustc_target/src/spec/mod.rs` and add a target module under
   `compiler/rustc_target/src/spec/targets/`. The module must describe the
   actual OCTAGRAM host ABI and linker policy; it must not simply copy the
   Linux target.
2. Add `x86_64-unknown-octagram` to
   `src/bootstrap/src/core/sanity.rs`'s `STAGE0_MISSING_TARGETS` while the
   Stage 0 compiler does not know the new builtin target. Remove it only after
   a later Stage 0 compiler contains the target.
3. Patch and pin dependent crates such as upstream `libc` (and `cc` only if
   the host build actually requires it) using the top-level Cargo patch
   mechanism. Update lockfiles from the pinned source revision.

These steps follow the upstream
[Rust compiler guide for adding a target](https://rustc-dev-guide.rust-lang.org/building/new-target.html).
They are recorded as required follow-up work; no builtin target registration
or Stage 0 bootstrap patch is claimed in this repository yet. The immediate
blocker remains the OCTAGRAM-specific libc/PAL contract and a successful
`build-std=std` experiment.

### M4.2 progress — upstream libc target slice (2026-11-22)

The first OCTAGRAM-specific module for upstream `rust-lang/libc` now
cross-compiles on the pinned upstream revision
`ccc2ff4aaab962f7fde7b8edda7379c045b1ca12`. The integration is applied by
`tools/apply_rust_libc_octagram.py`; CI runs it before compiling libc with
`-Zbuild-std=core`. The module has compile-time ABI layout assertions and
the public C header checker validates its source layouts.

This advances only the libc dependency groundwork. The M4.2 tasks for full
libc/PAL conformance and Rust `std` remain open. The successful compile does
not prove that every declared function links or behaves correctly at runtime.

### M4.2 progress — Rust std compile and link probe (2026-11-23)

The pinned Rust source uses upstream `libc 0.2.191`; the OCTAGRAM libc
integration is now pinned to the matching release commit
`ccc2ff4aaab962f7fde7b8edda7379c045b1ca12`.

CI patches the installed `rust-src` with OCTAGRAM Unix target glue and
successfully runs both `-Zbuild-std=std check` and a diagnostic
`-Zbuild-std=std build`. The latter links an executable using the OCTAGRAM
startup object and linker script, then inspects it with `readelf`.
Run [45088069322](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/45088069322)
passed the complete Rust no_std workflow, including both std probes.

This is compile/link evidence only. The std executable has not yet been
included in the boot image or executed in QEMU. The next gate is guest runtime
validation of std filesystem, environment, clock, thread, process, and panic
paths. Native rustc and Cargo remain future milestones.
