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


## Filesystem descriptor and directory-relative APIs

The runtime now also exposes typed wrappers for `openat`, `fstat`,
`fstatat`, `lseek`, `mkdirat`, `unlinkat`, and `renameat`, plus the
OCTAGRAM `AT_FDCWD`, `AT_SYMLINK_NOFOLLOW`, and `SEEK_*` constants.
The Rust guest smoke binary exercises `openat`, `fstat`, `fstatat`, and
`lseek` against a seeded source file. It also exercises `opendir`,
`readdir`, and `closedir` against the seeded `/share/octagram` directory,
checking for `message.c` and the end-of-directory result. The QEMU syscall
test requires the dedicated directory-iteration success marker. Host tests
check the C-compatible `FileStat` and `Dirent` layouts and their field
offsets.k constants.

These interfaces move the filesystem side closer to the needs of Cargo, but
they do not yet constitute a complete `std::fs` backend: directory iteration,
permissions and timestamps, symlink edge cases, rename/unlink behavior, and
error mapping still need conformance tests.

## Process environment support

The runtime now exports a read-only `getenv(&CStr)` lookup backed by the
initial `environ` pointer installed by `user/crt0.S`. Lookup requires an
exact variable-name match, rejects malformed names, and bounds the scan to
4096 entries. Host unit tests cover exact matches, prefix collisions,
missing variables, invalid names, and null environment pointers.

This is a first process-environment primitive for future Rust `std` and Cargo
support. It does not yet implement `setenv`, `unsetenv`, ownership of
environment strings, or mutation-safe environment replacement. Those belong
in a later process/environment layer.

## Runtime ABI expansion for Rust host support

The runtime now centralizes wrappers for the VM and synchronization/process
primitives needed by a future `std::sys` backend: `mmap`, `munmap`,
`mprotect`, `pipe2`, `dup3`, futex, thread ID/join, and FS thread-pointer
get/set. The global allocator uses the shared `mmap`/`munmap` wrappers rather
than embedding those syscall numbers directly.

These wrappers are an ABI boundary, not a claim of full POSIX or Rust `std`
compatibility. In particular, futex operation values, timeout clock semantics,
thread cancellation, robust mutexes, descriptor inheritance, and failure
semantics still require conformance tests before a Rust `std` backend can rely
on them.


### Safe Rust directory-entry name access

The runtime now exposes `octagram_runtime::Dirent::name_bytes()`. It checks that `d_reclen` stays within the fixed C ABI record and that a NUL terminator occurs inside the record's name region. It returns a borrowed byte slice, avoiding repeated raw-pointer conversions in applications. Host unit tests cover a valid name, a record too short for its name, a missing terminator, and an oversized record. The QEMU directory smoke test uses this same accessor. This validates the Rust-side record boundary; it does not change the kernel directory ABI or claim full `std::fs` compatibility.


### File metadata helpers

`octagram_runtime::FileStat` exposes POSIX file-type constants and helpers (`file_type`, `is_file`, `is_directory`, `is_symlink`, `is_character_device`, and `permissions`). The compact OCTAGRAM metadata record remains 16 bytes (`size`, `mode`, `modified_seconds`); these helpers interpret its mode bits without pretending that it is the larger public C `struct stat`. Host unit tests cover regular files, directories, symlinks, and permission extraction. This is a Rust runtime convenience layer, not yet the `std::fs::Metadata` implementation.


### Typed syscall error decoding

The runtime exposes `SysError` and `decode_syscall_result` for decoding the
kernel's sign-extended negative errno convention. The decoder accepts errno
values in the ABI range 1..=4095, preserves the positive errno code, and can
re-encode it as the original negative return value. Common predicates cover
interrupted operations, would-block, missing paths, and permission failures.
This is the low-level error boundary intended for a future Rust `std` PAL;
existing syscall wrappers still return their original `Result<T, i64>`
signatures for compatibility. Host unit tests cover both valid errno values
and non-error/out-of-range returns.


### Owned file descriptors

The runtime now provides `OwnedFd`, an ownership wrapper for descriptors
returned by `open` and `openat`. It exposes borrowed raw access, explicit
ownership transfer through `into_raw_fd`, explicit closure through
`close_owned`, and descriptor-based read/write/seek methods. On OCTAGRAM
guest builds, dropping an owner closes its descriptor; host unit-test builds
keep Drop inert so tests never execute guest syscalls. Unsafe
`from_raw_fd` documents the single-owner requirement. This is a first
resource-ownership primitive for a future `std::fs` implementation; it does
not yet provide buffered I/O, path normalization, or the complete
`std::os::fd` trait surface.


## `Command::output`

`Command::output()` menangkap stdout dan stderr ke `CapturedOutput { status, stdout, stderr }`. Dua pipe dikuras bersamaan memakai `poll`, sehingga child yang menulis banyak data ke kedua stream tidak terjebak karena pipe yang tidak dibaca penuh. `status` tetap nilai keluar mentah OCTAGRAM; pemetaan ke tipe `ExitStatus` belum tersedia.

## Owned pipe dan `Stdio`

`pipe2_owned(flags)` membungkus kedua ujung pipe menjadi `OwnedFd`, sehingga descriptor tidak tertinggal terbuka ketika owner di-drop pada target OCTAGRAM. `Command` menerima sumber stdio `Stdio::Inherit` atau `Stdio::Fd(fd)` untuk stdin, stdout, dan stderr. Descriptor `Fd` dipinjam ketika `spawn()` dipanggil; kepemilikannya tetap di parent. Smoke test guest mengalihkan stdout shell ke pipe, menutup ujung tulis parent, membaca sampai EOF, lalu memeriksa isi dan exit status. API ini belum menyediakan capture otomatis untuk ketiga stream atau semantik penuh `std::process::Stdio`.

## Process spawning (`Command`)

Runtime menyediakan builder `Command` dengan `spawn()` dan pemetaan descriptor opsional untuk stdin/stdout/stderr. Spawn memakai pipe `O_CLOEXEC` sebagai handshake: EOF tanpa payload menunjukkan `execve` berhasil, sedangkan payload berisi errno negatif menunjukkan kegagalan eksekusi. Smoke test guest menjalankan `/bin/sh -c 'exit 23'`, memeriksa status keluar, lalu memastikan file non-ELF ditolak sebagai executable. Ini belum sama dengan `std::process::Command`; pengaturan environment tingkat tinggi, signal, process group, dan seluruh semantik `Stdio` masih belum tersedia.

## Child process ownership

Runtime menyediakan `Child` untuk melacak PID anak langsung. `wait()` menunggu hingga anak selesai dan mengembalikan nilai mentah yang diberikan ke `exit(status)`; `try_wait()` memakai `WNOHANG` agar pemeriksaan tidak memblokir. `waitpid_with_options()` dan `WAIT_ANY_PID` juga tersedia untuk kebutuhan tingkat rendah. Smoke test guest menjalankan `fork`, menguji pemeriksaan nonblocking, lalu mereap anak dan memverifikasi status keluar. API ini belum setara dengan `std::process::Child`/`Command`; belum ada spawn error handshake atau abstraksi stdio command.

## Rust file-handle layer

The octagram_runtime crate now exposes a small no_std File handle over OwnedFd: open/openat, descriptor borrowing, read/read_exact, write/write_all, seek, metadata, explicit close, and raw-fd ownership transfer. The exact-read and write-all loops handle partial progress; early EOF or a zero-progress write returns -EIO. This is not std::fs::File or a complete Rust standard library. Host tests cover partial I/O and ownership transfer; the guest smoke program exercises File::openat, metadata, exact read, seek, and close.


The file layer also exposes positional I/O using OCTAGRAM syscalls 102 (`pread`) and 103 (`pwrite`). `File::read_at`/`write_at` do not alter the shared file offset; `read_exact_at`/`write_all_at` handle partial transfers and checked offset advancement. Host tests exercise offset progression and failure cases, and the guest smoke test verifies positional reads leave the sequential file position unchanged. The guest test does not yet exercise writes to a persistent file.


## ExitStatus dan poll multi-stream

Runtime menyediakan `ExitStatus` sebagai wrapper atas nilai wait mentah OCTAGRAM, dengan `raw()`, `code()`, dan `success()`. `Child::wait_status()` serta `try_wait_status()` menyediakan bentuk bertipe tanpa menghapus API raw lama. `CapturedOutput::exit_status()` dan `success()` memakai wrapper yang sama. Karena kernel belum mengodekan terminasi sinyal ke status wait, `code()` saat ini selalu `Some(i32)` hasil interpretasi nilai mentah; ini bukan jaminan kompatibilitas penuh dengan `std::process::ExitStatus`.

Saat satu stream output mencapai EOF, `Command::output()` sekarang mengubah descriptor terkait menjadi `fd = -1` pada array `poll`. Kernel mengabaikan descriptor negatif. Ini mencegah `POLLHUP` pada stream yang sudah selesai membuat loop polling sibuk ketika stream lainnya masih terbuka.

## Directory iterator

`IntoIterator for Directory` menghasilkan `DirectoryIter`, sebuah owning iterator dengan item `Result<DirEntry, i64>`. Setiap `DirEntry` mempertahankan descriptor parent duplikat dan menyediakan `name_bytes()`, `file_name()`, `path()`, `metadata()`, serta `file_type()`, sehingga metadata tetap dapat dibaca setelah iterator mencapai EOF. EOF mengakhiri iterasi; error dikeluarkan sekali dan menghentikan iterasi. `DirectoryIter` mengimplementasikan `FusedIterator`.

## OS strings dan path byte-preserving

Runtime no_std menyediakan primitive internal `OsStr`/`OsString` dan `Path`/`PathBuf` berbasis byte mentah untuk menyiapkan integrasi PAL filesystem. Nama/path non-UTF-8 dipertahankan; `try_to_cstring_bytes()` menolak NUL sebelum menyiapkan argumen syscall. Path helper menyediakan absolute check, filename, extension, parent, dan push. Tipe ini belum merupakan implementasi resmi `std::ffi`/`std::path` dan belum mencakup normalisasi `.`/`..` maupun seluruh edge case upstream.
