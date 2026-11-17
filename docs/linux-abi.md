# Lapisan kompatibilitas ABI Linux

OCTAGRAM-OS menyediakan dua ABI syscall:

- `int $0x80` — ABI asli OCTAGRAM (lihat [syscall-abi.md](syscall-abi.md)).
- instruksi `syscall` — ABI Linux x86_64, agar binary Linux yang sudah ada
  dapat dijalankan tanpa di-rebuild.

Keduanya berbagi jalur pemulihan frame interrupt, penanganan sinyal, dan
penyimpanan state FPU/TLS.

## Cara masuk ke kernel

1. Saat boot, `arch/x86_64/gdt.rs` menginisialisasi `STAR`, `LSTAR`, `FMASK`,
   dan `EFER.SCE`. `LSTAR` menunjuk ke stub `syscall_entry`.
2. `syscall_entry` (di `arch/x86_64/interrupt.rs`) menyimpan register user ke
   `InterruptFrame` dengan nomor vektor sintetis `129`, lalu memanggil handler
   interrupt yang sama dengan `int $0x80`.
3. `elf.rs` membedakan asal syscall dari nomor vektor: vektor `129` diteruskan
   ke `linux_abi::dispatch`, vektor lain ke dispatcher OCTAGRAM.
4. Nilai balik Linux ditulis ke `RAX` sebagai `-errno` bila gagal.

Saat proses pertama dijalankan, `gdt::enter_user` mengosongkan register
umum sebelum `iretq`, meniru `ELF_PLAT_INIT` Linux. Tanpa ini, runtime C
Linux membaca `RDX` sebagai `rtld_fini` dan mendaftarkan nilai sisa kernel
sebagai handler `exit` (gejalanya lompat ke alamat `1` saat program selesai).

## Stack awal dan auxv

`build_initial_stack` menyusun stack awal dengan format Linux: `argc`, `argv`,
`envp`, lalu pasangan auxv yang diakhiri `AT_NULL`. Entri Linux yang disediakan
antara lain `AT_PHDR`, `AT_PHENT`, `AT_PHNUM`, `AT_BASE`, `AT_ENTRY`,
`AT_PAGESZ`, `AT_RANDOM`, `AT_PLATFORM`, `AT_EXECFN`, `AT_UID`/`AT_GID`,
`AT_SECURE`, dan `AT_CLKTCK`. Stack user memakai 256 halaman.

## Syscall yang ditranslasikan

`kernel/src/linux_abi.rs` memetakan nomor syscall Linux ke layanan kernel yang
sudah ada. Kelompok yang sudah berfungsi:

- proses: `exit`, `exit_group`, `fork`, `execve`, `wait4`, `getpid`, `getppid`,
  `getuid`/`getgid`/`geteuid`/`getegid`, `uname`, `prlimit64`, `arch_prctl`,
  `set_tid_address`, `set_robust_list`, `sched_getaffinity`;
- file: `openat`/`open`, `read`, `write`, `close`, `lseek`, `pread64`,
  `pwrite64`, `readv`, `writev`, `stat`, `lstat`, `fstat`, `newfstatat`,
  `statx`, `getdents64`, `mkdirat`, `unlinkat`, `renameat`, `readlinkat`,
  `symlinkat`, `fchmod`, `fsync`, `fdatasync`, `dup`/`dup2`/`dup3`, `pipe2`,
  `fcntl`, `getcwd`, `chdir`;
- memori: `mmap`, `munmap`, `mprotect`, `brk`;
- waktu dan sinkronisasi: `clock_gettime`, `clock_getres`, `gettimeofday`,
  `nanosleep`, `clock_nanosleep`, `futex`, `poll`, `ppoll`, `select`,
  `epoll_wait`/`epoll_pwait`;
- lain-lain: `ioctl`, `getrandom`, `eventfd`, `timerfd`, `socketpair`,
  `getrusage`, `rt_sigaction`, `rt_sigprocmask`, `rt_sigreturn`, `kill`.

Yang belum ada mengembalikan `-ENOSYS` tanpa logging, sehingga program Linux
yang memprobe antarmuka opsional (`rseq`, `clone3`, `memfd_create`) tetap
tenang. `read`/`write` menerima payload hingga 1 MiB; batas 4096 byte hanya
berlaku untuk argumen string pada syscall printf-style OCTAGRAM.

### Detail semantik yang penting

- **`brk` terpisah dari `mmap`.** `brk` tumbuh dari ujung image yang dimuat
  (`HEAP_INITIAL_END`) memakai cursor sendiri. Sebelumnya `brk` dan `mmap`
  berbagi satu cursor, sehingga `brk` bisa melompati arena 1 MiB milik
  pymalloc; glibc lalu menganggap area itu milik heap dan menimpanya
  (gejalanya `MemoryError` acak di `pymalloc_alloc`).
- **Argumen bertipe `int` adalah 32 bit.** glibc mengirim `fd = -1` sebagai
  `0xFFFF_FFFF` dan `AT_FDCWD` sebagai `0xFFFF_FF9C`, bukan sign-extended
  64 bit. `mmap` anonim dan resolusi `dirfd` membandingkan word rendah saja;
  tanpa itu setiap arena pymalloc gagal `EINVAL` dan setiap `openat` relatif
  gagal `EBADF`.
- **Tabel sinyal 1..=64.** glibc mendaftarkan handler untuk sinyal real-time
  (43..64) saat startup; tabel 32 entri membuat `rt_sigaction` mengembalikan
  `EINVAL` untuk semuanya.
- **`clock_gettime` memvalidasi clock id sebelum pointer**, seperti Linux,
  lalu baru membaca RTC/TSC.

## Perbedaan layout struct

Beberapa struct Linux sengaja dikonversi, bukan disalin apa adanya:

- `struct stat` Linux x86_64 = 144 byte (`LinuxStat`), berbeda dari `oct_stat`.
- `struct statx` = 256 byte (`LinuxStatx`).
- `struct rlimit` = 2 × `u64`.
- `struct termios` untuk `TCGETS`/`TCSETS` = **36 byte** (16 byte flag,
  1 byte line, 19 control character), bukan layout 60 byte milik glibc.
  Menulis 60 byte ke buffer 36 byte merusak stack pemanggil; karena itu
  `ioctl` terminal Linux dikonversi di `linux_abi::linux_ioctl` memakai
  `fs::devfs::linux_termios_bytes` dan `termios_from_linux_bytes`.
- `rt_sigaction` dan `sigprocmask` dikonversi ke tabel sinyal internal.

## Status pengujian

Diuji di QEMU dengan binary **static glibc** yang dibangun memakai
`gcc -static -no-pie`:

- `write` + `printf` + `return 7` → output benar, exit status `7`;
- program dengan `malloc`, `memset`, `getenv`, `stderr`, `fflush` → benar;
- `fstat`, `isatty` (`TCGETS`), buffered `printf` → benar, exit status `0`;
- `atexit` ganda berjalan LIFO lalu proses selesai normal.

CPython 3.12.7 yang di-link statis berjalan penuh. Image menyematkan
`/bin/python3` dan stdlib sebagai `/usr/local/lib/python312.zip`, sehingga:

```
octagram:/ $ run /bin/python3 -S -c print(40+2)
42
```

Proses keluar dengan status 0. `-S` melewati `site`; `zipimport` membaca
langsung dari arsip zip karena build statis ini tidak menyertakan modul
`zlib`, jadi arsip stdlib harus disimpan dengan metode **STORE** (tanpa
kompresi). Heap kernel dinaikkan ke 64 MiB agar toolchain dan runtime Python
muat di RAM filesystem saat boot.

## Batasan dan langkah berikutnya

- Belum ada `ld-linux.so`: binary Linux **dinamis** (termasuk CPython/OpenJDK
  dari distro) belum bisa dijalankan. Diperlukan dukungan `PT_INTERP`,
  relokasi, `mmap` file, dan `/proc/self/exe`.
- `clone`/thread Linux, `rseq`, `memfd_create`, `/proc`, `/sys`, dan `epoll`
  penuh belum lengkap. JVM membutuhkan thread, `mprotect` untuk JIT, dan
  dukungan sinyal yang lebih lengkap.
- CPython statis sudah berjalan; yang belum adalah modul ekstensi dinamis
  (`.so`), `ctypes`, dan thread Python karena `clone` Linux belum ada.
- Langkah berikutnya: dukung `clone`/futex untuk thread, `rseq` sebagai
  sukses no-op, `/proc/self/exe`, lalu `ld.so` + OpenJDK.

## Menguji binary Linux

Build binary uji static di host:

```sh
gcc -static -O2 -no-pie -o hello hello.c
```

Lalu salah satu dari:

1. Sematkan sementara lewat `include_bytes!` + `seed_file` di
   `kernel/src/fs/mod.rs` (cara yang dipakai saat bring-up), atau
2. salin ke volume FAT32 yang di-mount dari shell, lalu jalankan
   `run /mnt/hello` dari shell OCTAGRAM.
