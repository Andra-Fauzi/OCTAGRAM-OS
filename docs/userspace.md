# Program user dan libc

Program user adalah ELF64 x86_64 yang dijalankan kernel di ring 3 dengan page
table privat. Semua program di repo dibangun dengan `user/Makefile` dan memakai
libc OCTAGRAM (`user/runtime.c`) — bukan glibc.

## ABI singkat

- Syscall memakai `int $0x80` dengan nomor di `RAX` dan argumen di
  `RDI, RSI, RDX, R10, R8, R9` (stub di `user/include/octagram.h`).
- Nilai balik negatif adalah `-errno`; libc mengubahnya menjadi `-1` + `errno`.
- Nomor syscall, nilai balik, dan batas buffer: [syscall-abi.md](syscall-abi.md).
- `errno` disimpan per thread lewat TLS statis (`-DOCTAGRAM_TLS_ERRNO`).

## Entry point dan exit

`user/crt0.S` adalah `_start`:

1. membaca `argc`/`argv`/`envp` dari stack awal;
2. menyiapkan state runtime (termasuk TLS statis);
3. memanggil `main(argc, argv, envp)`;
4. memanggil `exit()` sehingga handler `atexit` dan destructor berjalan.

`exit` dirujuk sebagai simbol **weak** agar program Rust `no_std` yang tidak
mendefinisikannya tetap bisa di-link; bila tidak ada, `crt0` memakai jalur
syscall `int $0x80` langsung.

## Struktur libc

`user/runtime.c` adalah satu translation unit besar yang mengimplementasikan:

- string/memori (`memcpy`, `strlen`, `strtol`, ...);
- stdio (`printf`, `scanf`, stream `FILE`, buffering);
- berkas dan direktori (`open`, `read`, `write`, `stat`, `opendir`, `realpath`);
- proses (`fork`, `execve`, `waitpid`, `posix_spawn`), pipe, thread,
  penjadwalan (`sched_*`, `setpriority`/`getpriority`), serta kredensial dan
  kemampuan (`setuid` family, `capget`/`capset`);
- sinyal (`sigaction`, `sigprocmask`, `sigaltstack`, `sigreturn`);
- socket (`socket`, `bind`, `connect`, `sendmsg`/`recvmsg`, `poll`, `select`,
  `epoll`), resolver (`getaddrinfo`, `gethostbyname`, A/AAAA/PTR);
- `pthread` (mutex termasuk mutex robust dengan `EOWNERDEAD`, condvar,
  semaphore) di atas futex;
- dynamic loading (`dlopen`, `dlsym`, `dlclose`, `dlerror`);
- locale/multibyte UTF-8, termios, `time`, `setjmp`, `stdatomic`.

Header publik ada di `user/include/` (`stdio.h`, `stdlib.h`, `sys/*.h`,
`netinet/*.h`, `pthread.h`, `dlfcn.h`, `spawn.h`, `sched.h`, `sys/capability.h`,
...). Header ini adalah
kontrak yang dipakai program dan TinyCC, jadi perubahan tanda tangan harus
disertai penyesuaian runtime dan uji.

## Build

```sh
make -C user            # program contoh + TinyCC + dynamic linker
make -C user dynamic    # libc.so, libdyn_demo.so, demo dynamic/PIE
make -C user rust       # target Rust userland
```

`CFLAGS` memakai `-ffreestanding -fno-builtin -fno-pie -fno-pic
-mno-red-zone`; build PIE memakai `PIC_CFLAGS`. Linker script `user/linker.ld`
menetapkan base statis `0x400000`, sedangkan build PIE/dynamic memakai
`crt0.pic.o` dan `liboctagram.so`.

Program contoh yang dibangun (`PROGRAMS` di `user/Makefile`):
`hello`, `file_demo`, `runtime_demo`, `tty_demo`, `argv_demo`, `spawn_demo`,
`pipe_demo`, `env_demo`, `posix_demo`, `stdio_demo`, `posix_spawn_demo`,
`file_api_demo`, `path_api_demo`, `at_api_demo`, `execve_demo`, `fork_demo`,
`time_demo`, `mmap_demo`, `scanf_demo`, `tls_demo`, `thread_demo`, `fb_demo`,
`syscall_compat_demo`, `account_demo`, `socket_msg_demo`, `netdb_demo`,
`dns_demo`, `cxa_atexit_demo`, `credential_demo`, `robust_mutex_demo`.

Selain itu dibangun `dynamic_demo`, `copy_reloc_demo`, `versioned_demo`,
`dlopen_demo`, `aslr_demo`, `ld-octagram.so`, `liboctagram.so`,
`libdyn_demo.so`, serta `tcc.elf` dan `libc.a` untuk TinyCC.

Pengembangan aplikasi userspace dengan bahasa Rust (tanpa pustaka C) didukung
melalui workspace `user/rust/` dengan crate `octagram-runtime`; panduan lengkap
ada di [rust-userspace.md](rust-userspace.md).

## Menjalankan di OS

Salin ELF ke image atau filesystem yang bisa diakses OS, lalu dari shell:

```
run /bin/hello.elf
runp /bin/a.elf /bin/b.elf
```

PATH default (`/bin`) membuat `hello.elf` bisa dijalankan sebagai `hello.elf`.

## Batasan

- Tanpa TLS dinamis untuk shared object (hanya TLS statis executable/libc).
- Tanpa `fork` copy-on-write, tanpa `vfork`, tanpa `clone3`.
- Tanpa `mmap` file umum, tanpa `mprotect` granular, tanpa `mremap`.
- Tanpa NSS, `iconv`, atau locale database; hanya konversi UTF-8 bawaan.
- Tanpa `/proc`, `/sys`, atau `sysconf` dinamis.


## POSIX Core Stage 1

- TSD key generation mencegah nilai thread lama muncul kembali setelah key dihapus dan ID-nya dipakai ulang.
- Pthread attributes mendukung ukuran stack custom untuk thread joinable. Detached threads belum didukung (`ENOTSUP`).
- File actions `posix_spawn` memakai kapasitas dinamis, menyimpan path secara terpisah, dan tidak dibatasi delapan aksi/127 byte path; demo menguji lebih dari delapan aksi dan batas path.
- `fork()` mengunci metadata libc yang relevan sebelum membuat snapshot agar child tidak mewarisi lock yang sedang dipegang thread lain.
