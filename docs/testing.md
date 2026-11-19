# Pengujian

Ada dua lapis pengujian yang bisa dijalankan tanpa hardware: uji host kernel
(Rust) dan uji host user (C, memakai runtime OCTAGRAM yang dikompilasi untuk
Linux). Uji runtime OS sebenarnya memerlukan QEMU.

## Uji host kernel

```sh
cargo +nightly test --manifest-path kernel/Cargo.toml \
    --target x86_64-unknown-linux-gnu
```

Kernel dikompilasi dengan `cfg(test)` sehingga bagian yang tidak menyentuh
hardware bisa diuji sebagai binary biasa. Saat ini ada **325 uji** yang mencakup
antara lain:

- ELF loader: validasi header/segmen, penolakan W+X, load bias PIE;
- tata letak stack/ASLR (`stack_layout`, `STACK_RANDOM_PAGES`);
- `validate_user_range`/`validate_mapping_range` dan batas alamat;
- pipe, descriptor, `dup`, `wait_target_matches`, `next_runnable`;
- parser DHCP, pembentukan frame IPv4/UDP, checksum, antrean UDP;
- UDP connect (peer untuk `send`/`recv` dan penyaringan sumber);
- TCP: `retransmit_backoff`, `zero_window_probe_due`, `keepalive_due`;
- futex: `futex_bitset_matches`;
- VFS/RAMFS/devfs, parser FAT32/ext2, shell (parsing perintah dan path);
- helper framebuffer/terminal dan ioctl.

Menambah uji: letakkan `#[cfg(test)] mod tests` di berkas modul, gunakan helper
murni (tanpa alokasi hardware) agar bisa dijalankan di host, dan hindari
ketergantungan pada urutan eksekusi paralel. Uji yang memasang link jaringan
palsu harus memakai `LINK_TEST_LOCK` karena hanya satu link aktif.

## Uji host user

Runtime libc dikompilasi dengan `gcc` host dan dijalankan sebagai program Linux
biasa. Target di `user/Makefile`:

```sh
make -C user scanf-test     # printf/scanf round-trip lewat runtime asli
make -C user cxa-test       # urutan atexit/__cxa_atexit/__cxa_finalize
make -C user resolv-test    # parser /etc/resolv.conf
make -C user ipv6-test      # inet_pton/inet_ntop IPv6 + parser A/AAAA
```

Tekniknya: bagian runtime yang diuji di-link dengan `--gc-sections` dan stub
startup (`scanf_runtime_start.S`) agar syscall OCTAGRAM yang tidak ada di Linux
tidak ikut tertarik. Batasnya: fungsi yang membutuhkan `mmap`/`brk` OCTAGRAM
(mis. `calloc` untuk `getaddrinfo`) tidak bisa diuji di host dan hanya
diverifikasi kompilasi.

## Build yang harus hijau

Perubahan dianggap sehat bila semua perintah ini exit 0 tanpa `error:`:

```sh
make -C user
make -C user dynamic
make -C kernel KARCH=x86_64
cargo +nightly test --manifest-path kernel/Cargo.toml --target x86_64-unknown-linux-gnu
```

## Uji di QEMU

- `make run` / `make run-hdd` menjalankan image dengan QEMU (x86_64 default).
- `tools/qemu_syscall_test.py` mengotomatiskan interaksi shell lewat serial
  monitor (`/tmp/mon.sock`) dan menunggu teks tertentu di `/tmp/serial.log`.
- `tools/cpython_port.py` membantu workflow port CPython.

Uji di QEMU memerlukan image, OVMF, dan perangkat yang sesuai; perilaku driver
atau jaringan nyata tidak bisa disimpulkan dari uji host.

Cakupan uji host untuk kesalahan memori sengaja dibatasi pada jalur yang tidak
menyentuh page table: batas `validate_user_range`/`validate_mapping_range`,
penolakan `PT_LOAD` tidak valid oleh `parse`, dan pembulatan `mapping_size`.
Page fault sungguhan, akses ke mapping yang sudah di-`munmap`, dan isolasi
lintas proses hanya bisa dibuktikan di QEMU karena memerlukan CR3 dan frame
allocator nyata.

## Direktori `tests/`

`tests/pp` dan `tests/tests2` berisi symlink `Makefile` ke
`/tmp/octagram-tinycc-upstream/` pada checkout ini; isinya adalah suite uji
upstream TinyCC, bukan suite milik OCTAGRAM. Jangan menganggapnya sebagai
cakupan uji kernel.

## Yang belum tercakup

- Perilaku runtime penuh (scheduler, sinyal, dlopen, TCP, keepalive, futex
  bitset) hanya diverifikasi kompilasi dan uji helper.
- Tanpa uji otomatis boot QEMU di CI.
- Tanpa fuzzing untuk parser FAT32/ext2/DHCP/DNS.
- Tanpa pengukuran performa atau regression benchmark.


## Shared libc thread-state regression

Build the dynamically linked userland examples:

```sh
make -C user dynamic
```

Copy `shared_errno_demo.elf`, `ld-octagram.so`, and `liboctagram.so` into
the target image (the executable expects `/lib/ld-octagram.so` and
`/lib/liboctagram.so`). Run:

```text
run /bin/shared_errno_demo.elf
```

Expected output:

```text
shared errno/h_errno + allocator: PASS
```

This QEMU/runtime test checks independent `errno` and `h_errno` values for
two threads while the shared allocator services concurrent allocations and
frees. It must be run on OCTAGRAM because the accessors use the OCTAGRAM
thread/process ID syscalls.
