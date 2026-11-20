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
hardware bisa diuji sebagai binary biasa. Jumlah test dapat berubah seiring
perkembangan branch; cakupan yang sudah ada antara lain:

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
make -C user/rust make
make -C user
make -C user dynamic
cargo +nightly fmt --manifest-path kernel/Cargo.toml --check
cargo +nightly test --manifest-path kernel/Cargo.toml --target x86_64-unknown-linux-gnu
make -C kernel KARCH=x86_64
make all
```

Artefak userspace harus dibuat lebih dahulu karena `kernel/src/fs/mod.rs`
memakai `include_bytes!` untuk executable seperti `make.elf`, `tcc.elf`,
dan beberapa demo ELF. Program Rust userspace memerlukan `ld.lld` (paket
Debian/Ubuntu: `lld`).

## Uji di QEMU

- `make run` / `make run-hdd` menjalankan image dengan QEMU (x86_64 default).
- `tools/qemu_syscall_test.py` mengotomatiskan interaksi shell lewat serial
  monitor (`/tmp/mon.sock`) dan menunggu teks tertentu di `/tmp/serial.log`.
- `tools/qemu_ext2_persistence_test.py` menguji penyimpanan source di EXT2:
  format disk kedua dengan `mkfs.ext2` native, mount, tulis berkas, pipeline,
  redirection, `mv`, unmount, `system_reset` lewat monitor, lalu mount ulang
  dan membaca kembali isi berkas yang sama. Jalankan QEMU dengan disk kedua
  dan monitor socket, misalnya
  `-drive file=/tmp/ext2-test.img,if=none,id=ext2,format=raw
  -device ide-hd,drive=ext2,bus=ide.1 -monitor unix:/tmp/mon-ext2.sock,server,nowait`,
  lalu `DISK=sata1 python3 tools/qemu_ext2_persistence_test.py` (sesuaikan
  `DISK` dan `MON`/`SERIAL` dengan konfigurasi QEMU).
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

Direktori `tests/` (symlink `Makefile` ke direktori TinyCC sementara) sudah
dihapus dari repo karena menunjuk path `/tmp` yang tidak ada pada clean
checkout. Suite uji yang berlaku untuk OCTAGRAM adalah unit test kernel/libc,
`tools/qemu_*_test.py`, dan workflow CI di
`.github/workflows/posix-core-stage1.yml`; jangan menganggap suite uji upstream
TinyCC sebagai cakupan uji kernel.

## Yang belum tercakup

- Perilaku runtime penuh (scheduler, sinyal, dlopen, TCP, keepalive, futex
  bitset) hanya diverifikasi kompilasi dan uji helper.
- Workflow GitHub Actions ada di `.github/workflows/posix-core-stage1.yml`;
  jalur QEMU smoke test hanya menjalankan `syscall-compat-demo` setelah build
  sebelumnya sukses. Ini bukan pengujian seluruh POSIX API atau semua driver
  pada hardware fisik.
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


## POSIX thread-specific data regression

`make -C user` builds `thread_tsd_demo.elf`. Run it on OCTAGRAM:

```text
run /bin/thread_tsd_demo.elf
```

Expected output:

```text
pthread TSD isolation and destructors: PASS
```

The test checks that two threads can store different values under the same
key, that the main thread's value remains isolated, and that key destructors may repopulate a value and run again on a later
POSIX destructor iteration.


The `posix_spawn_demo` also appends twelve file actions to one action object,
exercising dynamic growth beyond the former fixed eight-action limit, and
checks the 4095-byte `addopen` path bound.


## Thread stack attributes

`thread_demo.elf` initializes a `pthread_attr_t`, configures a 96 KiB
stack, reads the value back, and creates both workers with that attribute.
Run `run /bin/thread_demo.elf`; expected output is
`preemptive threads and TLS passed`.


## Uji integrasi driver

Pengujian driver perlu dibagi menjadi:

1. **Host unit test** untuk parser descriptor, BAR size/address helper,
   register encoding, state machine, batas panjang/alignment, dan error handling
   menggunakan fake register/device. Tes ini tidak membuktikan hardware hidup.
2. **Build bare-metal** untuk memastikan unsafe/register path bisa dikompilasi
   pada target kernel.
3. **QEMU** untuk controller/peripheral yang diemulasikan; catat konfigurasi
   QEMU dan model device yang digunakan.
4. **Hardware fisik** untuk fitur yang perilakunya bergantung firmware, power
   sequencing, DMA/IOMMU, interrupt routing, atau implementasi controller
   nyata.

Saat ini workflow CI menjalankan smoke test syscall kompatibilitas otomatis
setelah seluruh build berhasil. Pengujian khusus USB hotplug, seluruh alur
Mass Storage, Wi-Fi QCA9377, dan kompatibilitas seluruh perangkat fisik tetap
memerlukan pengujian terarah. Jangan mengartikan CI build sukses sebagai bukti
seluruh kelas perangkat bekerja.
