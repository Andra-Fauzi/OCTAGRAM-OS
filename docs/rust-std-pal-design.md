# Rust std PAL dan host-tool port untuk OCTAGRAM

Dokumen ini adalah kontrak implementasi bertahap, bukan klaim bahwa `std`,
`rustc`, atau Cargo native sudah tersedia.

## Status yang sudah ada

- Target freestanding `x86_64-unknown-none` dipakai untuk kernel dan
  aplikasi Rust `no_std`.
- Target eksperimen `x86_64-unknown-octagram` menetapkan
  `target_os = "octagram"` dan dapat menghasilkan ELF `no_std + alloc`
  yang dijalankan guest QEMU.
- `octagram-runtime` membungkus syscall ABI OCTAGRAM dan menyediakan
  allocator, environment lookup, file descriptor ownership, file I/O,
  positional I/O, metadata, directory iteration, VM, clock, entropy,
  futex, dan thread primitives.
- Host Linux masih menjalankan rustc/Cargo untuk menghasilkan guest ELF.

## Konsekuensi upstream Rust

Custom target JSON saja tidak mengaktifkan Rust standard library. Upstream
Rust harus diberi dukungan OS eksplisit pada pemilihan PAL dan konfigurasi
build `std`; kemudian PAL OCTAGRAM mengimplementasikan kontrak platform
yang dibutuhkan modul-modul `std::sys`. Referensi konsep:
[Porting Rust standard library](https://wiki.osdev.org/Porting_Rust_standard_library)
dan [Rust platform support](https://doc.rust-lang.org/stable/rustc/platform-support.html).

Port dilakukan terhadap checkout upstream Rust yang dipatok ke commit
tertentu. Patch lokal harus terpisah, dapat di-rebase, dan memiliki test
compile/test untuk target host OCTAGRAM. Jangan memodifikasi compiler
menggunakan mrustc atau xrustc.

## Peta awal PAL

| Kebutuhan Rust std | Primitive OCTAGRAM yang tersedia | Pekerjaan PAL yang tersisa |
| --- | --- | --- |
| File open/read/write/seek | `openat`, `read`, `write`, `lseek` | Map error ke `io::Error`; validasi partial I/O dan EOF |
| File metadata | `fstat`, `fstatat`, `stat` | Lengkapi field metadata yang dibutuhkan `Metadata` |
| File mutation/sync | `ftruncate`, `fsync`, `fdatasync` | Semantik permissions dan durability test |
| Positional I/O | `pread`, `pwrite` | Validasi offset signed, partial I/O, dan error |
| Directory | `opendir`, `readdir`, `closedir` | Implementasikan iterator `ReadDir`, dot entries, dan error terminal |
| Path mutation | `mkdirat`, `unlinkat`, `renameat` | Mapping remove-file/remove-dir dan path edge cases |
| VM | `mmap`, `munmap`, `mprotect`, `brk` | PAL allocator/VM dan mapping file yang teruji |
| Process | `fork`, `execve`, `waitpid`, `pipe2(O_CLOEXEC)`, owned pipe descriptors, `dup3`, `Child`, `Stdio`, `Command::spawn` | Port ke `std::process`, signal/status mapping, full `Stdio`/`Command` semantics |
| Threads | `thread_create/join/exit`, FS TLS, `futex` | Integrasi `std::thread`, mutex/condvar, TLS destructor |
| Clock/random | `clock_gettime`, `nanosleep`, `getrandom` | Monotonic/instant semantics, error propagation, entropy tests |
| Dynamic loading | Loader ELF subset | Belum layak untuk mengasumsikan dylib Rust/LLVM; tentukan static-link strategy |

Nomor syscall dan keberadaan wrapper bukan bukti kompatibilitas semantik.
Setiap baris harus memiliki test guest yang menguji sukses, error, batas,
blocking, dan concurrency sesuai kebutuhan API Rust.

## Child process ownership update

Runtime kini menyediakan `Child`, pembungkus PID anak langsung dengan `wait()` dan `try_wait()`. `waitpid_with_options()` mengekspos flag `WNOHANG`, sedangkan `WAIT_ANY_PID` mewakili target semua anak. Status yang dikembalikan adalah nilai 64-bit mentah dari `exit(status)`, bukan encoding bit wait-status Linux. Smoke test guest melakukan `fork`, memeriksa jalur nonblocking, menunggu anak, dan memverifikasi status `42` serta konsumsi handle setelah reap. Ini belum menjadi `std::process::Child` dan belum mengimplementasikan `Command`/`posix_spawn`.

## Command spawn dan exec handshake

Runtime kini menyediakan `Command::new(path, argv, envp)`, konfigurasi descriptor melalui `stdin_from`/`stdout_from`/`stderr_from`, serta `spawn()`. Vektor `argv` dan `envp` wajib menyertakan pointer null terakhir. Implementasi membuat `pipe2(O_CLOEXEC)`, melakukan `fork`, menerapkan pemetaan descriptor di child, lalu memanggil `execve`. EOF pada pipe berarti image berhasil diganti; payload errno negatif berarti `execve` gagal. Parent mereap child pada jalur kegagalan. Smoke test guest menguji eksekusi `/bin/sh -c 'exit 23'` dan kegagalan menjalankan file sumber C sebagai executable. Ini fondasi untuk `std::process`, belum kompatibilitas penuh `Command` (termasuk `Stdio` abstraction, signal semantics, dan process groups).

## Owned pipes dan konfigurasi Stdio

Runtime kini menyediakan `pipe2_owned(flags)` yang mengembalikan dua `OwnedFd`; kedua ujung pipe otomatis ditutup ketika owner di-drop pada target OCTAGRAM. Builder `Command` menerima `Stdio::Inherit` atau `Stdio::Fd(fd)` melalui `.stdin()`, `.stdout()`, dan `.stderr()`, sementara metode `*_from(fd)` lama tetap tersedia. Descriptor sumber dipinjam selama `spawn()` dan ownership tetap pada parent. Smoke test guest menjalankan shell builtin `printf` dengan stdout diarahkan ke pipe, menutup ujung tulis parent, membaca sampai EOF, memeriksa byte output dan exit status. Ini memberi jalur dasar untuk `std::process::Stdio`, tetapi belum menyediakan enum `Piped` yang otomatis menghasilkan handle output seperti API standar.

## Directory ownership update

Runtime kini menyediakan `Directory`, pembungkus RAII di atas descriptor
`opendir/readdir/close`:
- `Directory::open(path)` membuka stream direktori;
- `next_entry()` menghasilkan satu `Dirent` atau `None` saat EOF;
- `as_raw_fd()` meminjam descriptor;
- `close()` menutup secara eksplisit; `Drop` menutup pada target OCTAGRAM.

Smoke test guest menggunakan `Directory` alih-alih mengelola descriptor
direktori mentah. Ini merupakan building block untuk `std::fs::ReadDir`,
belum merupakan implementasi iterator std: semantik error setelah EOF,
snapshot saat direktori berubah, urutan entri, dan lifetime entry masih harus
diselaraskan dengan kontrak Rust std.

## API runtime terbaru

`File` sekarang memiliki:
- `set_len(length)` melalui syscall `ftruncate(9)`;
- `sync_all()` melalui `fsync(119)`;
- `sync_data()` melalui `fdatasync(120)`.

Smoke test guest membuat file sementara, menulis data, memotong ukurannya,
memanggil kedua operasi sync, memeriksa ukuran melalui `fstat`, menutup,
dan menghapus file. Gate QEMU harus memeriksa marker
`Rust runtime file mutation/sync: PASS` dan exit status nol sebelum
kemampuan ini dianggap QEMU-tested.

## Urutan bootstrap resmi

1. Pertahankan host Linux sebagai Stage 0 untuk bootstrap compiler.
2. Lengkapi PAL, libc ABI, dan `std` untuk host triple
   `x86_64-unknown-octagram`; kernel tetap target
   `x86_64-unknown-none`.
3. Bangun dan jalankan executable Rust sederhana dengan `std` di QEMU.
4. Bangun LLVM/LLD atau backend/linker yang dipilih sebagai tool native.
5. Bangun upstream `rustc` untuk host OCTAGRAM menggunakan Stage 0.
6. Jalankan native rustc di guest untuk membangun program Rust tahap berikut.
7. Port Cargo sebagai executable host OCTAGRAM; uji workspace lokal/offline
   lebih dahulu, baru registry/network.
8. Buktikan bootstrap berulang: native rustc membangun ulang rustc/Cargo dan
   hasilnya dapat dijalankan kembali.
9. Setelah toolchain host stabil, gunakan Cargo native untuk membangun kernel
   freestanding, userspace, test suite, dan image builder.

## Gate yang tidak boleh dilewati

- Tidak menyebut `std` selesai sebelum test suite PAL dan program std guest
  berjalan.
- Tidak menyebut rustc native selesai sebelum binary compiler berjalan di
  OCTAGRAM dan mengompilasi source di dalam guest.
- Tidak menyebut Cargo native selesai sebelum Cargo berjalan di guest,
  memanggil rustc native, dan membangun workspace offline.
- Tidak menyebut self-hosting sebelum bootstrap ulang dan siklus build/test/
  image/install/reboot berhasil dari dalam OCTAGRAM.
