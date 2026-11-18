# Batasan

Halaman ini mengumpulkan fitur yang **belum** ada atau baru sebagian, sesuai
aturan penerimaan roadmap: dokumentasi harus mencatat perilaku yang tidak
didukung. Status per item ada di [ROADMAP.md](../ROADMAP.md).

## Isolasi dan keamanan

- Tanpa copy-on-write; `fork` menyalin seluruh halaman user.
- Kredensial (uid/euid/gid/egid) dan metadata kemampuan per pid sudah ada
  (`setuid`, `capget`/`capset`). Kemampuan sudah ditegakkan untuk penulisan VFS
  (`CAP_VFS_WRITE`), `socket()` AF_INET (`CAP_NETWORK`), dan prioritas task lain
  (`CAP_SCHED_NICE`), tetapi belum untuk mount/device, console, atau descriptor
  yang sudah terbuka; semua proses masih berbagi mount dan konfigurasi jaringan.
- Tanpa permission enforcement penuh; mode berkas disimpan tetapi tidak selalu
  diperiksa.
- Tanpa autentikasi password (`passwd` belum diimplementasikan); `login` hanya
  mengganti nama pengguna sesi.
- Tanpa sandbox syscall, seccomp, atau namespace.

## Proses dan scheduler

- Prioritas nice, aging, dan watchdog starvasi/idle tersedia, tetapi tanpa
  real-time scheduling (`SCHED_FIFO`/`SCHED_RR`) dan tanpa
  `PRIO_PGRP`/`PRIO_USER`.
- Tanpa SMP; hanya satu CPU.
- `read()` console dan `read()` dari tty yang dibuka lewat path
  (`/dev/stdin`, `/dev/tty*`, `/dev/console`) sama-sama scheduler-aware; tidak
  ada jalur tty yang menahan CPU di kernel saat masih ada proses lain.
- Tanpa robust futex, tanpa `clone3`, tanpa `vfork`.
- Kata sandi disimpan sebagai SHA-256 bergaram sekali jalan (`/etc/shadow`),
  bukan KDF berulang; belum ada kebijakan kedaluwarsa, `su`, atau sesi terpisah
  per terminal.
- Batas 64 proses terjadwal dan 64 KiB kernel stack per proses.

## Memori

- Tanpa swap dan huge page. `mlock`/`mlockall` hanya menandai halaman dan
  kuncinya tidak diwarisi `fork`; `madvise` hanya `posix_madvise` validasi.
- Tanpa `mprotect` granular atau `mremap`.
- Interpreter dynamic linker masih `ET_EXEC`, jadi alamatnya tidak diacak.
- Heap kernel memakai allocator sederhana tanpa defragmentasi.

## Filesystem dan storage

- Root RAMFS volatil; belum ada image root persisten otomatis.
- Tanpa journaling; kegagalan penulisan bisa merusak FAT32/ext2.
- Tanpa hard link, quota, inotify, atau `rename` lintas mount.
- Tanpa ext3/ext4, NVMe, UASP, RAID/LVM, atau swap-backed storage.
- Operasi berkas shell dibatasi 64 KiB per operasi.

## Jaringan

- Tanpa stack IPv6 (hanya primitif alamat dan AAAA di resolver).
- Congestion window masih sederhana (slow start 1→4 MSS, tanpa fast retransmit
  atau congestion avoidance penuh); tanpa SACK dan window scaling.
- Tanpa ARP cache besar, routing table, atau network namespace.
- Lease DHCP dan rute tidak persisten antar reboot.
- Tanpa TLS di klien HTTP; hanya HTTP/1.0 sederhana.

## Dynamic linking dan runtime

- Tanpa lazy binding; semua `JUMP_SLOT` diselesaikan saat load.
- Tanpa TLS untuk shared object; hanya TLS statis untuk `errno`.
- Tanpa `DT_RUNPATH`, `dlmopen`, atau namespace library.
- Libc bukan glibc: banyak fungsi POSIX lanjutan tidak tersedia (lihat
  [userspace.md](userspace.md)).
- Tanpa `/proc`, `/sys`, atau `sysconf` dinamis.

## Grafis dan desktop

- Tanpa compositor/window manager, toolkit GUI, atau akselerasi GPU.
- Program user tidak bisa menggambar langsung ke framebuffer.
- Tanpa font vektor, clipboard, atau multi-monitor.

## Driver dan hardware

- Driver audio, GPU, printer, webcam, NVMe, dan UASP belum ada.
- Hotplug USB ditangani lewat polling, bukan lifecycle penuh.
- QCA9377 dinonaktifkan secara default; upload firmware dan koneksi Wi-Fi belum
  berjalan.

## Toolchain dan pengujian

- Toolchain mini kernel hanya menerima subset C.
- Perintah `tcc` di `help` belum terhubung ke cabang shell; jalankan
  `run /bin/tcc.elf` sebagai gantinya.
- Tanpa uji otomatis boot QEMU/CI, tanpa fuzzing parser, tanpa benchmark.
- Perilaku runtime (scheduler, sinyal, dlopen, TCP, futex) baru diverifikasi
  lewat uji host helper dan kompilasi.
