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
- Target JSON juga mendeklarasikan `target_family = ["unix"]` agar crate
  dapat memilih jalur `cfg(unix)`. Ini hanya klasifikasi target; bukan bukti
  bahwa OCTAGRAM telah memenuhi seluruh kontrak Unix/POSIX atau bahwa PAL
  `std` telah tersedia. Validator CI memeriksa cfg tersebut secara eksplisit.

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

### Patch upstream pertama

Direktori `patches/rust/` kini menyimpan patch awal untuk menambahkan
`target_os == "octagram"` ke allowlist `library/std/build.rs` pada upstream
Rust commit `342d74463af7dd3018ef6aa04dd5500c2deb5bd1`. Target JSON OCTAGRAM
sudah memilih `cfg(unix)`, sehingga dispatcher PAL upstream akan masuk ke
jalur Unix generik.

Patch ini sengaja hanya membuka gate `restricted_std`; ia **belum**
menambahkan dukungan target pada crate `libc`, belum membangun PAL, dan
belum membuktikan bahwa `std` dapat dikompilasi. Sebelum menganggapnya
terintegrasi, patch harus diuji dengan `git apply --check` pada checkout
yang persis sama, kemudian build-std harus dijalankan untuk mengumpulkan
daftar API libc yang benar-benar hilang. Jangan menambahkan target ke allowlist
sebagai jalan pintas untuk menyembunyikan kegagalan ABI.

## Peta awal PAL

| Kebutuhan Rust std | Primitive OCTAGRAM yang tersedia | Pekerjaan PAL yang tersisa |
| --- | --- | --- |
| File open/read/write/seek | `openat`, `read`, `write`, `lseek` | Map error ke `io::Error`; validasi partial I/O dan EOF |
| File metadata | `fstat`, `fstatat`, `stat` | Lengkapi field metadata yang dibutuhkan `Metadata` |
| File mutation/sync | `ftruncate`, `fsync`, `fdatasync` | Semantik permissions dan durability test |
| Positional I/O | `pread`, `pwrite` | Validasi offset signed, partial I/O, dan error |
| Directory | `opendir`, `readdir`, `closedir`, `DirectoryIter`, owned `DirEntry`, `fstatat` | Adapter iterator dan metadata entry tersedia; selaraskan `OsString`/`PathBuf`, dot entries, dan perubahan direktori |
| Path mutation | `mkdirat`, `unlinkat`, `renameat` | Mapping remove-file/remove-dir dan path edge cases |
| VM | `mmap`, `munmap`, `mprotect`, `brk` | PAL allocator/VM dan mapping file yang teruji |
| Process | `fork`, `execve`, `waitpid`, `pipe2(O_CLOEXEC)`, owned pipe descriptors, `dup3`, `Child`, `Stdio`, `Command::spawn` | Port ke `std::process`, signal/status mapping, full `Stdio`/`Command` semantics, and process status mapping |
| Threads | `thread_create/join/exit`, FS TLS, `futex` | Integrasi `std::thread`, mutex/condvar, TLS destructor |
| Clock/random | `clock_gettime`, `nanosleep`, `getrandom`, `poll` | Monotonic/instant semantics, error propagation, entropy tests |
| Dynamic loading | Loader ELF subset | Belum layak untuk mengasumsikan dylib Rust/LLVM; tentukan static-link strategy |

Nomor syscall dan keberadaan wrapper bukan bukti kompatibilitas semantik.
Setiap baris harus memiliki test guest yang menguji sukses, error, batas,
blocking, dan concurrency sesuai kebutuhan API Rust.

## Child process ownership update

Runtime kini menyediakan `Child`, pembungkus PID anak langsung dengan `wait()` dan `try_wait()`. `waitpid_with_options()` mengekspos flag `WNOHANG`, sedangkan `WAIT_ANY_PID` mewakili target semua anak. Status yang dikembalikan adalah nilai 64-bit mentah dari `exit(status)`, bukan encoding bit wait-status Linux. Smoke test guest melakukan `fork`, memeriksa jalur nonblocking, menunggu anak, dan memverifikasi status `42` serta konsumsi handle setelah reap. Ini belum menjadi `std::process::Child` dan belum mengimplementasikan `Command`/`posix_spawn`.

## Command spawn dan exec handshake

Runtime kini menyediakan `Command::new(path, argv, envp)`, konfigurasi descriptor melalui `stdin_from`/`stdout_from`/`stderr_from`, serta `spawn()`. Vektor `argv` dan `envp` wajib menyertakan pointer null terakhir. Implementasi membuat `pipe2(O_CLOEXEC)`, melakukan `fork`, menerapkan pemetaan descriptor di child, lalu memanggil `execve`. EOF pada pipe berarti image berhasil diganti; payload errno negatif berarti `execve` gagal. Parent mereap child pada jalur kegagalan. Smoke test guest menguji eksekusi `/bin/sh -c 'exit 23'` dan kegagalan menjalankan file sumber C sebagai executable. Ini fondasi untuk `std::process`, belum kompatibilitas penuh `Command` (termasuk `Stdio` abstraction, signal semantics, dan process groups).

## `Command::output` stdout capture

Runtime menyediakan `Command::output()` membuat dua pipe `O_CLOEXEC`, mengalihkan stdout dan stderr child, lalu memakai `poll(2)` untuk menguras keduanya bersamaan sampai EOF sebelum mereap child. Pembacaan simultan mencegah deadlock ketika salah satu pipe penuh. Hasil `CapturedOutput` berisi status keluar mentah OCTAGRAM, byte stdout, dan byte stderr. API poll memakai layout `PollFd` 8-byte yang diuji terhadap ABI kernel. `CapturedOutput::exit_status()` dan `success()` menyediakan adapter `ExitStatus` awal, tetapi ini belum `std::process::Output` penuh: status sinyal belum direpresentasikan dan semantik stdin, sinyal, serta error masih perlu diselaraskan. Guest smoke test memeriksa output `printf` dan status nol; QEMU gate memiliki marker khusus untuk API ini.

## Owned pipes dan konfigurasi Stdio

Runtime kini menyediakan `pipe2_owned(flags)` yang mengembalikan dua `OwnedFd`; kedua ujung pipe otomatis ditutup ketika owner di-drop pada target OCTAGRAM. Builder `Command` menerima `Stdio::Inherit` atau `Stdio::Fd(fd)` melalui `.stdin()`, `.stdout()`, dan `.stderr()`, sementara metode `*_from(fd)` lama tetap tersedia. Descriptor sumber dipinjam selama `spawn()` dan ownership tetap pada parent. Smoke test guest menjalankan shell builtin `printf` dengan stdout diarahkan ke pipe, menutup ujung tulis parent, membaca sampai EOF, memeriksa byte output dan exit status. Ini memberi jalur dasar untuk `std::process::Stdio`, tetapi belum menyediakan enum `Piped` yang otomatis menghasilkan handle output seperti API standar.

## Byte-preserving OS strings and path helpers

Runtime no_std kini menyediakan helper internal `OsStr`/`OsString` dan `Path`/`PathBuf` yang menyimpan byte mentah, sehingga nama/path non-UTF-8 tidak dipaksa menjadi UTF-8. `OsString::try_to_cstring_bytes()` dan `PathBuf::try_to_cstring_bytes()` menolak byte NUL sebelum membuat buffer NUL-terminated untuk syscall. `Path` menyediakan `is_absolute`, `file_name`, `extension`, `parent`, dan konversi ke `PathBuf`; `PathBuf::push` menangani path absolut sebagai pengganti dan path relatif sebagai komponen tambahan.

Ini adalah primitive runtime internal untuk menyiapkan adapter OS, bukan implementasi tipe `std::ffi::OsStr`, `std::ffi::OsString`, `std::path::Path`, atau `std::path::PathBuf`. Normalisasi komponen (`.`/`..`), prefix/platform semantics, dan seluruh edge case kompatibilitas std belum tersedia.
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


## ExitStatus dan poll multi-stream

Runtime menyediakan `ExitStatus` sebagai wrapper atas nilai wait mentah OCTAGRAM, dengan `raw()`, `code()`, dan `success()`. `Child::wait_status()` serta `try_wait_status()` menyediakan bentuk bertipe tanpa menghapus API raw lama. `CapturedOutput::exit_status()` dan `success()` memakai wrapper yang sama. Karena kernel belum mengodekan terminasi sinyal ke status wait, `code()` saat ini selalu `Some(i32)` hasil interpretasi nilai mentah; ini bukan jaminan kompatibilitas penuh dengan `std::process::ExitStatus`.

Saat satu stream output mencapai EOF, `Command::output()` sekarang mengubah descriptor terkait menjadi `fd = -1` pada array `poll`. Kernel mengabaikan descriptor negatif. Ini mencegah `POLLHUP` pada stream yang sudah selesai membuat loop polling sibuk ketika stream lainnya masih terbuka.

## Directory iterator adapter

`IntoIterator for Directory` memindahkan ownership stream ke `DirectoryIter`, yang mengimplementasikan `Iterator<Item = Result<DirEntry, i64>>` dan `FusedIterator`. EOF mengakhiri iterasi; error pertama dikeluarkan satu kali lalu iterator berhenti permanen. Guest smoke test menyimpan `DirEntry` melewati EOF iterator lalu memeriksa metadata melalui descriptor parent duplikat. Ini masih adapter internal, bukan `std::fs::ReadDir`/`std::fs::DirEntry`: konversi nama ke `OsString`, representasi `PathBuf`, metadata penuh, dan semantik dot entries masih perlu dipetakan.

## Audit ABI libc untuk upstream Rust

Source C OCTAGRAM kini dirangkum dalam
`tools/rust_libc_abi_surface.json`, dengan pemeriksaan konsistensi melalui
`tools/check_rust_libc_abi_surface.py`. Workflow Rust menjalankan validator
ini saat header/runtime berubah. Audit mengunci beberapa fakta penting:

- x86_64 memakai `off_t/ssize_t/time_t` 64-bit, `pid_t` 32-bit, dan
  `pthread_t` 64-bit.
- Record kernel `oct_stat` hanya 16 byte (`size`, `mode`,
  `modified_seconds`). Ini **bukan** layout publik `struct stat`.
- Wrapper C `stat/lstat/fstat/fstatat` menerjemahkan record kernel ke
  `struct stat`, tetapi saat ini hanya mengisi `st_size`, `st_mode`, dan
  `st_mtime`; field lain dinolkan.
- `dirent` adalah record 280 byte dengan nama fixed 256 byte; `timespec`
  adalah dua field 64-bit pada x86_64.
- `pthread_detach` saat ini mengembalikan `ENOTSUP`; socket messaging
  `sendmsg/recvmsg` belum tersedia.

Ini merupakan inventaris ABI C yang ada, bukan upstream Rust `libc` port.
Crate upstream `libc` harus mendapat modul OCTAGRAM tersendiri dan harus
mendefinisikan tepat ABI yang dipakai OCTAGRAM; jangan mewarisi deklarasi
Linux/glibc hanya karena target memakai `cfg(unix)`. Berikutnya adalah
memetakan API PAL Unix yang benar-benar dipakai ke simbol C yang tersedia,
lalu mengimplementasikan modul libc target-specific dan menambahkan compile
test. Sampai itu lulus, status Rust `std` tetap belum diimplementasikan.

## Inventaris awal API Unix PAL dari upstream Rust

Audit source pada commit Rust
`342d74463af7dd3018ef6aa04dd5500c2deb5bd1` menunjukkan bahwa generic Unix
PAL membutuhkan jauh lebih banyak API daripada file I/O dasar. Area yang
terlihat mencakup filesystem, process/wait, descriptor/poll, pthread mutex dan
condition variable, clocks, environment, serta socket.

Subset awal yang dipetakan tersedia dalam
`tools/rust_std_pal_api_inventory.json`. Checker
`tools/check_rust_std_pal_api_inventory.py` memastikan setiap simbol terpilih
memiliki deklarasi pada header C OCTAGRAM dan definisi pada `user/runtime.c`.
Workflow Rust no_std menjalankan checker ini.

Inventaris menandai beberapa batas yang tidak boleh disembunyikan:
- `pthread_detach` tersedia sebagai simbol, tetapi mengembalikan `ENOTSUP`.
- `sendmsg/recvmsg` tersedia dengan dukungan terbatas; ancillary data dan
  descriptor passing tidak didukung.
- `getrandom` saat ini bukan sumber entropy kriptografis.
- Pemeriksaan deklarasi/definisi tidak membuktikan kesamaan semantik POSIX,
  struktur ABI, atau kecocokan semua API yang dipanggil `std`.

Ini adalah subset audit, bukan daftar lengkap kebutuhan `std`. Berikutnya
perlu membangun modul `libc` OCTAGRAM dengan tipe dan struktur yang diverifikasi,
lalu menjalankan compile test terhadap checkout upstream. Fitur PAL yang belum
didukung harus diberi cfg/implementasi eksplisit; jangan mengarahkan target
OCTAGRAM ke ABI Linux.

## Upstream libc target OCTAGRAM: compile gate lulus

Integrasi awal upstream `rust-lang/libc` kini disimpan sebagai modul
`patches/rust-libc/octagram/mod.rs` dan diterapkan oleh
`tools/apply_rust_libc_octagram.py`. Applicator memverifikasi checkout
upstream tepat pada commit
`a03dd7c2e298b0ca482f30ce07efd3115938328e`, lalu mengubah source hanya jika
setiap anchor ditemukan tepat satu kali. Modul tersebut mendefinisikan ABI
OCTAGRAM sendiri dan tidak meminjam layout Linux atau Redox.

Modul awal mencakup scalar aliases, `stat`, `dirent`, `sockaddr`,
`addrinfo`, `passwd`, `tm`, `fd_set`, `lconv`, `sem_t`, `termios`,
`sigaction`, dan object pthread. Compile-time assertions mengunci ukuran
struktur utama. Binding generik untuk pthread rwlock, `statvfs`, dan
`dladdr` dikecualikan karena interface tersebut belum disediakan OCTAGRAM.

CI menjalankan validator header/ABI, menerapkan modul ke source upstream yang
dipatok, lalu melakukan
`cargo +nightly -Zjson-target-spec -Zbuild-std=core check` untuk
`x86_64-unknown-octagram`. Run
[45045939233](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/45045939233)
lulus, termasuk compile upstream libc, build userspace Rust, runtime host
unit tests, dan pemeriksaan ELF statis.

**Batas:** ini membuktikan crate upstream `libc` dapat di-cross-compile untuk
custom target. Ini belum membangun Rust `std`, belum membuktikan seluruh
simbol PAL tersedia saat link/run, dan belum menghasilkan rustc atau Cargo
native. Tahap berikutnya adalah percobaan `build-std=std` dan penyelesaian
error PAL yang benar-benar muncul, dengan ABI setiap fungsi diverifikasi
terhadap header serta runtime OCTAGRAM.

### pthread mutex API required by the generic Unix PAL

The pinned upstream Unix PAL mutex implementation explicitly initializes
mutexes with `pthread_mutexattr_settype(PTHREAD_MUTEX_NORMAL)` and uses
`PTHREAD_MUTEX_INITIALIZER` / `PTHREAD_COND_INITIALIZER` for static
objects. OCTAGRAM now exposes these constants and initializers through the
upstream libc target module. The C runtime implements
`pthread_mutexattr_settype` as a validation-only operation: normal is the
only supported type and all other values return `EINVAL`. The existing
mutex ABI remains unchanged because its implementation already has normal,
non-recursive behavior.

`thread_demo` now exercises the normal attribute and rejects an invalid
mutex type. Rust no_std CI run
[45046390159](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/45046390159)
confirms the updated libc module still cross-compiles. C build and QEMU
execution of the updated demo are tracked by POSIX Core Stage 1 separately.
Recursive/error-checking mutexes and full thread lifecycle semantics remain
unsupported; in particular `pthread_detach` still returns `ENOTSUP`.
