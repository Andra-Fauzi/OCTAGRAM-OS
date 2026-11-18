# OCTAGRAM-OS Development Roadmap

## Objective

Develop OCTAGRAM-OS into a self-hosting POSIX-style operating system with native
user applications, isolated processes, persistent storage, networking, desktop
services, and a package/application ecosystem.

## Priority 1: Core process isolation

- [x] Give each process a private user page table where technically feasible
- [x] Prevent ring 3 code from changing kernel mappings
- [x] Add process groups and session identifiers
- [x] Add ASLR for PIE, mmap, stack, and heap layouts
  - [x] Randomized PIE load bias
  - [x] Randomized mmap starting cursor
  - [x] Randomized user stack placement (entropi 0–63 halaman per eksekusi)
  - [x] Randomized heap placement (`brk` tumbuh dari cursor `mmap` ber-entropi; heap image beralamat tetap dihapus)
  - [ ] Randomized interpreter placement (`ld-octagram.so` masih ET_EXEC)
- [ ] Restrict fork/exec address-space inheritance
- [ ] Add process capability and policy metadata
- [ ] Test kernel faults, invalid mappings, and cross-process memory access

## Priority 2: POSIX runtime

- [x] Basic file and directory operations
- [x] Process fork, execve, waitpid, pipes, and threads
- [x] Signals, poll, select, epoll, sockets, eventfd, and timerfd
- [x] pread/pwrite/readv/writev and resource APIs
- [x] Implement `realpath`
- [x] Implement `execvp`
- [ ] Implement complete `posix_spawn` file actions and attributes
  - [x] File actions `addclose`, `adddup2`, and `addopen` (8 actions, 127-byte paths)
  - [x] Attributes `POSIX_SPAWN_SETPGROUP`, `SETSIGDEF`, and `SETSIGMASK`
  - [x] `posix_spawn_demo` covers file/pipe redirection, process group, mask/default signal attributes, and failures
  - [x] `POSIX_SPAWN_RESETIDS` (`setgid`/`setuid` ke id nyata di anak)
  - [x] Atribut scheduling (`POSIX_SPAWN_SETSCHEDPARAM`/`SETSCHEDULER`)
    - [x] Syscall `sched_setscheduler`/`sched_getscheduler`/`sched_setparam`/`sched_getparam` (136–139) dan header `sched.h` + `sched_yield`
    - [x] `posix_spawnattr_setscheduler`/`setschedparam` beserta getter; diterapkan di anak sebelum `execve`
    - [x] `SCHED_OTHER` prioritas 0 diterima, `SCHED_FIFO`/`SCHED_RR` ditolak `EPERM`; uji host `scheduler_policy_limits_to_sched_other`
    - [x] `posix_spawn_demo` memeriksa policy/param di anak dan menolak `SCHED_FIFO`
    - [ ] Penjadwalan real-time (`SCHED_FIFO`/`SCHED_RR`) dengan prioritas RT sesungguhnya
- [x] Implement basic process groups and `setsid`
- [x] Implement alternate signal stacks (`sigaltstack` 133, `SA_ONSTACK`, `SS_ONSTACK`/`SS_DISABLE`, diwarisi tanpa status aktif saat `fork`)
- [x] Implement dynamic shared module loading
  - [x] `dlopen`/`dlsym`/`dlerror` di libc dengan hook loader, pencarian di `LD_LIBRARY_PATH`//`lib`
  - [x] Library yang sudah dimuat dipakai ulang; dependency `DT_NEEDED` belum dimuat otomatis
  - [x] `dlopen_demo` memuat `libdyn_demo.so` saat runtime dan memanggil simbolnya
  - [x] `RTLD_DEFAULT` dan `RTLD_NEXT` pada `dlsym`
  - [x] `dlclose`/unload dengan hitungan referensi dan destructor sebelum `munmap`
- [x] Implement sendmsg/recvmsg
  - [x] `sendmsg`/`recvmsg` gather/scatter over `send`/`sendto`/`write` and `recv`/`recvfrom`/`read`
  - [x] `socket_msg_demo` covers gather/scatter, `CMSG_*` bounds, and validation failures
  - [ ] Ancillary data (`SCM_RIGHTS`/`SCM_CREDENTIALS`)
  - [x] `MSG_TRUNC` reporting: kernel menyimpan panjang datagram asli, `recvmsg` mengisi `msg_flags`
  - [x] Datagram `sendmsg` tanpa `msg_name` (UDP `connect`, `send`/`recv` memakai peer)
  - [ ] `MSG_CTRUNC` (butuh ancillary data)
- [x] Implement a complete `pwd`/`grp` database
  - [x] `<pwd.h>` lookup, iteration, and `_r` variants over `/etc/passwd`
  - [x] `<grp.h>` over `/etc/group` with primary-group fallback from `/etc/passwd`
  - [x] `account_demo` covers found/not-found, `_r` storage, and `ERANGE`

## Priority 3: Dynamic linker and native runtime

- [x] PIC/PIE validation for dynamic executables
- [ ] TLS support in shared objects
- [x] Constructors and init arrays
- [x] Destructors and fini arrays
  - [x] `atexit` di libc menjalankan handler terbalik dengan pelindung reentrancy
  - [x] Loader menjalankan `DT_FINI_ARRAY`/`DT_FINI` (executable dulu, library terbalik)
  - [x] `crt0` memanggil `exit()` setelah `main` agar handler dan destructor berjalan
  - [x] `dynamic_demo` menguji handler `atexit` dan destructor shared library
  - [x] `__cxa_atexit`/`__cxa_finalize` untuk destructor C++ (satu daftar dengan `atexit`, filter DSO, dipanggil saat `dlclose`)
  - [x] Uji host `make -C user cxa-test` menjalankan urutan handler di atas runtime asli
- [x] Symbol versioning
  - [x] Loader membaca `DT_VERSYM`, `DT_VERDEF`, dan `DT_VERNEED`
  - [x] Resolusi menghormati versi yang diminta dan versi default (`@@`)
  - [x] `versioned_demo` menguji referensi `@EXAMPLE_1.0` dan `@@EXAMPLE_2.0`
- [ ] Lazy binding
- [x] `LD_LIBRARY_PATH` library search path
- [x] Copy relocations and weak symbols
  - [x] `R_X86_64_COPY` menyalin data shared library ke executable non-PIC dan mengekspor salinan itu
  - [x] Simbol weak tak terdefinisi diselesaikan sebagai 0
  - [x] `copy_reloc_demo` (ET_EXEC) menguji salinan data dua arah dan simbol weak yang tidak tersedia
- [x] ASLR and randomized fixed load bias
  - [x] Load bias PIE diacak kernel per eksekusi (`PIE_LOAD_BIAS`..`MAX`, xorshift ber-seed TSC)
  - [x] Shared library dipetakan lewat mmap anonim sehingga mengikuti kursor mmap berentropi
  - [x] `aslr_demo` mencetak alamat PIE, library, dan mmap untuk dibandingkan antar-run
  - [ ] Stack dan heap image (dilacak di Priority 1)

## Priority 4: Scheduler and threads

- [x] Replace the fixed 16-task limit with a 64-task scheduler limit
- [x] Add basic thread attributes and detached-state handling
- [x] Add scheduler priorities and aging
  - [x] Prioritas nice (-20..19) per proses/thread di `ScheduledProcess`, diwarisi anak `fork` dan thread baru
  - [x] Aging anti-starvasi: umur tunggu naik tiap pemilihan, boost maksimum 39 level, seri diputus umur terlama; uji host `scheduler_prefers_higher_nice_priority` dan `scheduler_aging_prevents_starvation`
  - [x] Syscall `setpriority`/`getpriority` (134/135, `PRIO_PROCESS`), `nice()` libc memakai keduanya
  - [ ] `PRIO_PGRP`/`PRIO_USER` dan real-time scheduling (`SCHED_FIFO`/`SCHED_RR`; `sched_*` kini hanya menerima `SCHED_OTHER`)
- [x] Make blocking syscalls scheduler-aware
  - [x] `read()` descriptor console memakai `ProcessWait::ConsoleInput(terminal)`; `poll_tty()` non-blocking menggantikan loop busy-wait di kernel
  - [x] `pump_console_input()` memindahkan event keyboard ke line discipline tiap putaran idle scheduler dan membangunkan penunggu lewat bitmask terminal, termasuk cabang saat semua proses tidur
  - [x] EOF (`Ctrl-D` baris kosong) disimpan sebagai `eof_pending` sehingga `read` mengembalikan 0 dan `poll` melaporkan siap; uji host `canonical_eof_is_reported_once_when_the_line_is_empty`
  - [x] Proses tunggal tanpa proses lain memakai jalur blocking kernel `fs::read_tty` agar shell tidak menerima `EDEADLK`
  - [ ] `read()` dari `/dev/stdin` yang dibuka sebagai file VFS masih blocking di kernel
- [ ] Implement wakeup cancellation and robust timed waits
- [ ] Improve mutex/condvar/futex implementation
  - [x] Real `FUTEX_WAIT`/`FUTEX_WAKE` with LAPIC timeout handling
  - [x] `pthread_mutex`, condition variables, and semaphores built on futex
  - [x] `FUTEX_REQUEUE`, `FUTEX_WAIT_BITSET`, dan `FUTEX_WAKE_BITSET` (timeout absolut, `FUTEX_CLOCK_REALTIME`, uji host pencocokan bitset)
  - [ ] Robust futex (daftar `FUTEX_OWNER_DIED` milik userspace)
- [ ] Add kernel task fairness and watchdog monitoring

## Priority 5: Networking

- [x] TCP IPv4 reliability and socket lifecycle hardening
  - [x] Retransmisi satu segmen dengan RTO eksponensial (30→60→120→240→480 tick, maks 5 percobaan; uji host `retransmit_backoff`)
  - [x] `send` menghormati `peer_mss` dan `peer_window`, serta mengembalikan `EAGAIN` saat jendela penuh
  - [x] Zero-window probing tiap 60 tick saat `peer_window` nol (probe 1 byte di bawah `send_next`; uji host `zero_window_probe_due`)
  - [x] Keepalive opt-in lewat `SO_KEEPALIVE` (probe ACK tiap 60.000 tick idle, maksimal 9 probe lalu RST; uji host `keepalive_due`)
  - [x] Antrean lebih dari satu segmen terkirim (pipeline/congestion window)
    - [x] Pipeline hingga 4 segmen di luar segmen tertua (`VecDeque<PendingSegment>`), ACK kumulatif melepas beberapa segmen sekaligus
    - [x] Congestion window byte: 1 MSS awal, +1 MSS per ACK sampai 4 MSS, reset saat RTO; dibatasi bersama `peer_window`
    - [x] Rollback `send_next` bila transmisi segmen gagal (segmen tertua maupun pipeline)
    - [x] Uji host `tcp_pipeline_advances_cumulative_acknowledgements`, `tcp_send_capacity_respects_peer_window_and_congestion_window`, `tcp_congestion_window_grows_per_ack_and_resets_on_timeout`
    - [ ] Segmentasi ulang saat peer MSS lebih kecil dari sisa buffer kirim (belum ada buffer kirim antrean penuh)
- [x] UDP POSIX API for socket/bind/sendto/recvfrom
- [ ] IPv6
- [x] Nonblocking socket support via SOCK_NONBLOCK and MSG_DONTWAIT
- [x] DNS resolver cache and complete getaddrinfo-like API
  - [x] Numeric IPv4 `getaddrinfo`, `inet_*`
  - [x] `AI_PASSIVE`, `AI_CANONNAME`, `AI_NUMERICHOST`, `AI_NUMERICSERV` pada `getaddrinfo`
  - [x] Numeric `getnameinfo`, `gethostbyname`, `gethostbyaddr`, and `gai_strerror` codes
  - [x] `netdb_demo` covers flags, numeric round-trips, `EAI_OVERFLOW`, and `h_errno`
  - [x] Resolver A-record IPv4 lewat UDP ke server DNS DHCP dengan timeout/poll dan cache TTL 8 entri
  - [x] `getaddrinfo`/`gethostbyname` memakai resolver; `dns_demo` menguji resolve dan cache
  - [x] Resolver PTR (reverse DNS) dengan cache terpisah; `getnameinfo`/`gethostbyaddr` memakainya lalu jatuh ke numerik
  - [x] Konfigurasi `/etc/resolv.conf` (`nameserver` pertama, fallback ke DNS DHCP kernel)
  - [x] Resolver AAAA (query tipe 28, cache terpisah, `getaddrinfo` `AF_INET6`, `inet_pton`/`inet_ntop` IPv6)
  - [x] `getaddrinfo` `AF_UNSPEC` mengembalikan entri IPv4 dan IPv6 berantai lewat `ai_next`; `netdb_demo` menguji alamat numerik kedua keluarga
- [ ] Network namespace or policy layer
- [ ] DHCP/client and routing persistence
  - [x] DHCP client lease request through `net dhcp`
  - [ ] Saved routing/lease persistence across reboot

## Priority 6: Storage

- [ ] Persistent root and per-user home
  - [x] `/home/octagram`, `/etc`, `/var`, `/opt`, `/apps`
  - [x] `df` mount reporting
  - [x] `persist <mount>` home setup
- [ ] Journaling/recovery for mutation failures
- [ ] Ext3/ext4 read support or focused write support
- [ ] EFI/GPT partition mounting improvements
- [ ] ext2 race and corruption hardening
- [ ] Swap or swap-backed anonymous memory
- [ ] RAID/LVM volume abstraction
- [ ] Disk image tools and recovery utility

## Priority 7: Drivers

- [ ] Audio/HDA
- [ ] Generic GPU framebuffer and accelerated graphics
- [ ] Printer and webcam USB classes
- [ ] HID improvements and multi-pointer support
- [ ] Network-interface abstraction
- [ ] NVMe
- [ ] UASP
- [ ] Stable device hotplug lifecycle

## Priority 8: Desktop and ecosystem

- [ ] Compositor/window manager
- [ ] GUI toolkit and drawing API
- [ ] Font rasterization and locale support
- [ ] Terminal emulator improvements
- [ ] Session manager and login
  - [x] `login`, `whoami`, `session`, and prompt user
  - [ ] Password/authentication database
- [x] Package metadata and repository format
  - [x] `/var/lib/octagram/packages/*.pkg`
  - [x] `registry.txt` package registry
- [ ] Package manager with install/remove/update/verify
  - [x] `pkg add/update/remove/list/info/verify`
- [ ] App launcher and file manager
  - [x] `/apps` registry directory
  - [x] `apps` shell command
- [ ] Power/session settings

## CPython 3.11/3.12

CPython is intentionally separate from the roadmap. Use `make python311` or
`make python312` only after the POSIX and linker milestones above are stable.
Do not install a Linux CPython ELF in OCTAGRAM; it will not use the OCTAGRAM
syscall ABI.

## Acceptance rules

No milestone is complete until:

1. It builds from a clean repository.
2. It has an OCTAGRAM ELF or kernel smoke test.
3. It is seeded into the runtime/VFS when required.
4. It has a focused positive and negative test.
5. Documentation records unsupported behavior explicitly.
