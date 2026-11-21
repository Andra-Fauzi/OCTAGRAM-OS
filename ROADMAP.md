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
- [x] Restrict fork/exec address-space inheritance
  - [x] `execve` membangun address space baru; hanya cwd, terminal, pid, umask, grup/sesi, dan descriptor non-`CLOEXEC` yang diwarisi
  - [x] `fork` tidak mewarisi kunci memori: `mlockall` dimatikan dan `MappingPage.locked` hasil klon di-reset (uji host `fork_does_not_inherit_memory_locks`)
  - [x] Pembatasan terpisah untuk pewarisan `MAP_SHARED` dan mapping device lintas `fork` (mapping device dan `shared_backing` disalin frame fisiknya tanpa alokasi ganda; uji host `fork_shared_pages_filter_includes_shared_backings_and_device_mappings`)
- [x] Add process capability and policy metadata
  - [x] Kredensial `uid`/`euid`/`gid`/`egid` per pid, diwarisi `fork`/thread, dibagi semua thread satu pid
  - [x] `getuid` family membaca kredensial nyata; syscall `setuid`/`seteuid`/`setgid`/`setegid` (140–143) dengan aturan root vs non-root (`credential_change_allowed`)
  - [x] Bitmask kemampuan `CAP_VFS_WRITE`/`CAP_NETWORK`/`CAP_SYS_ADMIN`/`CAP_SCHED_NICE`, diturunkan dari uid, `capget`/`capset` (144/145) dengan `CAP_SYS_ADMIN` untuk menulis
  - [x] Header `sys/capability.h`, contoh `credential_demo`, uji host `credentials_and_capabilities_follow_uid_rules`
  - [x] Penegakan `CAP_VFS_WRITE` untuk `open` bertulis, `write` file, `mkdir`, `unlink`, `rmdir`, `unlinkat`, `rename`, `symlink`, `chmod`, dan `ftruncate`
  - [x] Penegakan `CAP_NETWORK` pada `socket()` AF_INET dan `CAP_SCHED_NICE` pada prioritas task lain; uji host `capability_checks_require_the_requested_bit`
  - [x] Penegakan untuk mount/device, akses console, dan descriptor yang sudah terbuka sebelum uid diturunkan (`device_access_allowed`, pembatasan `/dev/fb0`, isolasi tty konsol antar terminal, penolakan tulis descriptor lama bila kredensial diturunkan; uji host `device_and_console_access_respects_credentials_and_capabilities`, demo `credential_demo`)
- [x] Test kernel faults, invalid mappings, and cross-process memory access
  - [x] Batas validasi alamat: panjang 0, di bawah `USER_ADDRESS_MIN`, melewati `USER_ADDRESS_MAX`, dan overflow (`user_range_validation_rejects_overflow_and_non_user_addresses`)
  - [x] `PT_LOAD` invalid: di luar user space, W+X, align tidak wajar, `filesz > memsz`, dan file terpotong (`rejects_load_segments_outside_the_user_address_space`, `rejects_malformed_load_segments`)
  - [x] `mapping_size` membulatkan halaman ke atas dan menolak panjang nol/overflow (`mapping_size_rounds_pages_and_rejects_bad_lengths`)
  - [ ] Uji runtime untuk page fault nyata, akses ke mapping yang sudah di-`munmap`, dan isolasi lintas proses (butuh QEMU, tidak bisa di uji host)

## Priority 2: POSIX runtime

- [x] Basic file and directory operations
- [x] Process fork, execve, waitpid, pipes, and threads
- [x] POSIX thread-specific data lifecycle
  - [x] Per-thread values and up to four destructor iterations
  - [x] Recycle deleted key IDs safely using per-thread generation counters
- [x] Signals, poll, select, epoll, sockets, eventfd, and timerfd
- [x] pread/pwrite/readv/writev and resource APIs
- [x] Implement `realpath`
- [x] Implement `execvp`
- [x] Implement complete `posix_spawn` file actions and attributes
  - [x] File actions `addclose`, `adddup2`, and `addopen` (dynamic action list, tested beyond 8 actions, paths up to 4095 bytes)
  - [x] File actions `addchdir` dan `addchdir_np` (mengubah direktori kerja anak sebelum `execve`)
  - [x] Attributes `POSIX_SPAWN_SETPGROUP`, `SETSIGDEF`, and `SETSIGMASK`
  - [x] Attribute `POSIX_SPAWN_SETSID` (membuat sesi baru di anak lewat `setsid`, ditolak bersamaan dengan `SETPGROUP`)
  - [x] `posix_spawn_demo` covers file/pipe redirection, process group, session, chdir, mask/default signal attributes, scheduling, resetids, and failures
  - [x] `POSIX_SPAWN_RESETIDS` (`setgid`/`setuid` ke id nyata di anak)
  - [x] Atribut scheduling (`POSIX_SPAWN_SETSCHEDPARAM`/`SETSCHEDULER`)
    - [x] Syscall `sched_setscheduler`/`sched_getscheduler`/`sched_setparam`/`sched_getparam` (136–139) dan header `sched.h` + `sched_yield`
    - [x] `posix_spawnattr_setscheduler`/`setschedparam` beserta getter; diterapkan di anak sebelum `execve`
    - [x] `SCHED_OTHER` prioritas 0 diterima; `SCHED_FIFO`/`SCHED_RR` kini menerima `sched_priority` 1..99 (`rt_priority` terpisah dari nice); uji host `scheduler_policies_accept_rt_priorities`
    - [x] `posix_spawn_demo` memeriksa policy/param di anak
    - [x] Penjadwalan real-time (`SCHED_FIFO`/`SCHED_RR`) dengan prioritas RT sesungguhnya
      - [x] `scheduling_key()` menaruh task RT di atas `SCHED_OTHER` dan memilih `rt_priority` terbesar; `SCHED_FIFO` mempertahankan pemilik CPU pada seri; uji host `rt_tasks_outrank_sched_other_and_higher_rt_priority_wins` dan `fifo_holds_ties_while_rr_rotates`
      - [x] `SCHED_RR` memakai kuota `RT_RR_QUANTUM_TICKS` (4 tick) dan dirotasi dari `handle_timer` lewat `schedule_next_rotating()`
      - [x] Izin `CAP_SCHED_NICE` untuk kebijakan RT/task lain (non-root hanya boleh turun ke `SCHED_OTHER`); uji host `sched_policy_changes_need_capability_or_self_demotion`
      - [x] `sched_get_priority_max/min` libc (99/1 untuk FIFO/RR) dan shell `sched` menampilkan policy serta `RTPRI`
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
- [x] Add pthread attributes and joinable thread lifecycle
  - [x] Configurable stack size via `pthread_attr_setstacksize` / `pthread_attr_getstacksize`
  - [ ] Detached thread reaping (`pthread_detach` and `PTHREAD_CREATE_DETACHED` return `ENOTSUP`)
- [x] Add scheduler priorities and aging
  - [x] Prioritas nice (-20..19) per proses/thread di `ScheduledProcess`, diwarisi anak `fork` dan thread baru
  - [x] Aging anti-starvasi: umur tunggu naik tiap pemilihan, boost maksimum 39 level, seri diputus umur terlama; uji host `scheduler_prefers_higher_nice_priority` dan `scheduler_aging_prevents_starvation`
  - [x] Syscall `setpriority`/`getpriority` (134/135, `PRIO_PROCESS`), `nice()` libc memakai keduanya
  - [x] `PRIO_PGRP`/`PRIO_USER` pada `setpriority`/`getpriority`, dengan izin `CAP_SCHED_NICE` atau euid sama untuk menurunkan prioritas; uji host `priority_targets_cover_process_group_and_user` dan `priority_changes_need_matching_euid_or_capability`
  - [ ] Real-time scheduling (`SCHED_FIFO`/`SCHED_RR`; `sched_*` kini hanya menerima `SCHED_OTHER`)
- [x] Make blocking syscalls scheduler-aware
  - [x] `read()` descriptor console memakai `ProcessWait::ConsoleInput(terminal)`; `poll_tty()` non-blocking menggantikan loop busy-wait di kernel
  - [x] `pump_console_input()` memindahkan event keyboard ke line discipline tiap putaran idle scheduler dan membangunkan penunggu lewat bitmask terminal, termasuk cabang saat semua proses tidur
  - [x] EOF (`Ctrl-D` baris kosong) disimpan sebagai `eof_pending` sehingga `read` mengembalikan 0 dan `poll` melaporkan siap; uji host `canonical_eof_is_reported_once_when_the_line_is_empty`
  - [x] Proses tunggal tanpa proses lain memakai jalur blocking kernel `fs::read_tty` agar shell tidak menerima `EDEADLK`
  - [x] `read()` dari tty yang dibuka lewat path VFS (`/dev/stdin`, `/dev/tty*`, `/dev/console`) juga scheduler-aware lewat `tty_path_terminal()` + `poll_tty()`; uji host `tty_path_terminal_matches_readable_terminals_only`
- [x] Implement wakeup cancellation and robust timed waits
  - [x] `poll`/`select` dibangunkan segera saat isi pipe, input console, atau `eventfd` berubah (`is_poll_wait` / `wake_poll_waiters`), tidak lagi menunggu deadline 1 ms
  - [x] Wait tanpa batas waktu mengosongkan `wait_deadline` lama dan setiap pembangunanan wait membatalkan deadline, sehingga tidak ada timeout palsu
  - [ ] Sumber lain yang dipantau `poll` (socket TCP/UDP, timerfd) masih memakai scan ulang 1 ms
- [ ] Improve mutex/condvar/futex implementation
  - [x] Real `FUTEX_WAIT`/`FUTEX_WAKE` with LAPIC timeout handling
  - [x] `pthread_mutex`, condition variables, and semaphores built on futex
  - [x] `FUTEX_REQUEUE`, `FUTEX_WAIT_BITSET`, dan `FUTEX_WAKE_BITSET` (timeout absolut, `FUTEX_CLOCK_REALTIME`, uji host pencocokan bitset)
  - [x] Robust futex (daftar `FUTEX_OWNER_DIED` milik userspace)
    - [x] Syscall `set_robust_list`/`get_robust_list` (146/147) dan field per task (panjang head wajib 24 byte)
    - [x] `release_robust_list()` saat task keluar menandai `FUTEX_OWNER_DIED`, melepas TID, membangunkan waiter; batas 64 entri dan melewati `list_op_pending`
    - [x] Uji host `robust_list_marks_only_the_owner_as_dead`
    - [x] API libc `pthread_mutexattr_setrobust`/`pthread_mutex_consistent` dengan `EOWNERDEAD`
      - [x] `pthread_mutex_t` menyimpan TID pemilik di kata futex dan tautan `robust_list_head` di offset 0; daftar per thread didaftarkan lewat `set_robust_list` (146) dengan `futex_offset` = `offsetof(state)`
      - [x] `pthread_mutex_lock` mengembalikan `EOWNERDEAD` setelah pemiliknya keluar, `pthread_mutex_consistent()` membersihkan `FUTEX_OWNER_DIED`, dan unlock tanpa konsisten membuat kunci berikutnya `ENOTRECOVERABLE`; contoh `robust_mutex_demo`
- [x] Add kernel task fairness and watchdog monitoring
  - [x] Fairness lewat aging: setiap proses runnable yang dilewati mendapat `age` +1 dan prioritas efektif naik sampai 39 level
  - [x] Snapshot `scheduler_status()` dan shell `sched` menampilkan PID/TID, nice, policy, `AGE`, serta status runnable/waiting/exited
  - [x] Penanda starvasi `WATCHDOG` bila `age` ≥ 78 (`starvation_watchdog_trips`, uji host `starvation_watchdog_trips_after_the_aging_budget`)
  - [x] Watchdog idle: peringatan `[sched] watchdog: idle wakeup delayed ...` bila `hlt` terlambat > 250 ms
  - [x] Akuntansi waktu CPU per task dan watchdog berbasis runtime nyata
    - [x] `TaskRuntime` menambah `cpu_milliseconds`/`wait_milliseconds` dari selisih `monotonic_milliseconds()` tiap tick timer; hitungan tunggu dimulai ulang saat task dipilih
    - [x] Shell `sched` menampilkan `CPUms`, `CPU%`, `WAITms`, dan watchdog starvasi berbasis waktu nyata (`wait ≥ 500 ms`, uji host `wall_clock_watchdog_and_clock_ticks_follow_the_thresholds`)
    - [x] `times()` (112) dan `getrusage()` (111) memakai waktu CPU task, bukan nilai nol

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
  - [x] Lease DHCP disimpan ke `/var/lib/octagram/net/lease` saat bound dan dimuat lagi oleh `restore_saved_lease()` setelah driver siap
  - [ ] Tabel routing statis yang persisten (saat ini hanya alamat/netmask/gateway/DNS yang disimpan)

## Priority 6: Storage

- [ ] Persistent root and per-user home
  - [x] `/home/octagram`, `/etc`, `/var`, `/opt`, `/apps`
  - [x] `df` mount reporting
  - [x] `persist <mount>` home setup
  - [x] GPT reader/writer and bounded partition block devices
  - [x] Native EXT2 formatter (4 KiB blocks, sparse superblocks, no journal)
  - [x] Live shell installer for SATA/AHCI + UEFI (FAT32 EFI + EXT2 root)
  - [x] Boot-time EXT2 root selection via `/etc/octagram-root` (legacy FAT32 detection retained)
  - [ ] Installer integration test in QEMU and real hardware
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
  - [x] Password/authentication database
    - [x] SHA-256 kernel (`sha256.rs`) dengan vektor NIST (termasuk batas padding 55/56 byte)
    - [x] `/etc/shadow` format `user:salt:hash`, parser melewati baris rusak, `upsert` untuk mengganti entri
    - [x] `passwd [user]` menulis hash bergaram; `login` meminta kata sandi bila entri ada
    - [ ] KDF berulang (Argon2/PBKDF2), kebijakan kedaluwarsa, dan `su`/sesi per terminal
- [x] Package metadata and repository format
  - [x] `/var/lib/octagram/packages/*.pkg`
  - [x] `registry.txt` package registry
- [ ] Package manager with install/remove/update/verify
  - [x] `pkg add/update/remove/list/info/verify`
- [ ] App launcher and file manager
  - [x] `/apps` registry directory
  - [x] `apps` shell command
- [ ] Power/session settings
  - [x] Power-off `shutdown` (ACPI S5) dan reset `reboot` (FADT `RESET_REG`, fallback 8042/triple fault)
  - [ ] Pengaturan sesi/pengguna yang persisten (mis. zona waktu, preferensi terminal)

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


## Priority 9: Documentation and contributor workflow

- [x] Add a central development guide with a map of the main kernel and userspace files
- [x] Document the boot sequence and subsystem initialization order
- [x] Document the clean build dependency order, including embedded userspace binaries and the `ld.lld` requirement
- [x] Add a PCI driver tutorial covering BDF discovery, device-model matching, BAR/MMIO, DMA, interrupt handling, cleanup, and tests
- [x] Add a USB driver tutorial separating PCI host-controller drivers from USB interface class drivers
- [x] Link the development guide and driver tutorials from the documentation index and driver overview
- [ ] Keep source maps synchronized as files/modules are added or renamed
- [ ] Add focused automated QEMU coverage for pthread attributes, TSD, posix_spawn, dynamic loading, USB hotplug, and block I/O
- [ ] Validate USB controller and class-driver behavior on physical hardware; QEMU smoke tests are not a substitute

Documentation milestone means the instructions match the code that exists today.
A tutorial or source-map entry does not imply that every device, controller,
POSIX feature, or architecture has been validated on real hardware.



## Priority 10: Native self-hosting (jalur strategis)

Tujuan: OCTAGRAM mengedit source, membangun userspace dan kernel, menguji,
membuat image bootable, memasang ke slot nonaktif, dan reboot ke hasil build
sendiri tanpa host dalam workflow normal.

Rencana teknis dan audit baseline rinci berada di
[Native Self-Hosting Roadmap](docs/native-self-hosting-roadmap.md).
Milestone M0–M7 pada dokumen tersebut adalah sumber status dan acceptance gate.

- [x] M0: baseline build/test dan manifest artefak yang reproducible (`make manifest`, lihat dokumen roadmap bagian 15)
- [ ] M1: native developer substrate, editor terminal, source tree persisten
  (implemented di source: direktori `/src`/`/build`/`/toolchain`, `mv` + rename
  EXT2, pipeline/redirection teks, pencarian editor, uji persistensi reboot;
  menunggu verifikasi QEMU)
- [ ] M2: native C toolchain (TinyCC + assembler/linker) teruji end-to-end
  - [x] TinyCC compile/run, multi-file object/archive via TinyCC, extended libc smoke (QEMU)
  - [x] Native `ld` ELF64 multi-object symbol resolution (host tests + QEMU smoke added; CI pending)
  - [x] Lazy static archive extraction in native `ld` (host tests + QEMU smoke)
  - [x] PC-relative GOT relocations `GOTPCREL`/`GOTPCRELX`/`REX_GOTPCRELX`/`GOTPC32` dengan slot writable per simbol (host tests)
  - [x] Static TLS local-exec: template `PT_TLS` dari `.tdata`/`.tbss`, relokasi `TPOFF32`/`TPOFF64`, dan initial-exec `GOTTPOFF` (host tests)
  - [ ] PLT dinamis, dynamic TLS (`TLSGD`/`TLSLD`), dan antarmuka `cc/as/ld` canonical
- [ ] M3: native build orchestrator dengan dependency dan incremental rebuild
  - [x] Make serial, include, single-stem pattern, `.PHONY`, order-only, incremental mtime rebuild, `-B` (QEMU)
  - [ ] Build manifest native dan full shell recipe semantics
- [ ] M4: bootstrap `rustc`, Rust `core/alloc`, lalu Cargo native/offline
- [ ] M5: build kernel Rust sepenuhnya dari dalam OCTAGRAM
- [ ] M6: native disk/boot image builder dan installer
  - [x] Builder image native: GPT + FAT32 ESP + EXT2 root, manifest SHA-256,
    verifikasi read-back byte-per-byte, perintah `mkimage` (host tests)
  - [x] Image berbasis file lewat `FileDisk` + `mkimage file <path> <mib>`
    (host tests, termasuk build image penuh ke file VFS)
  - [x] Boot QEMU (UEFI/OVMF) dari image hasil builder, root persisten dipilih
    dari slot A, dan journal A/B terbaca (`tools/qemu_ab_image_boot_test.py`)
  - [ ] Verifikasi entry boot Limine pada perangkat keras/firmware nyata
- [ ] M7: A/B update, verifikasi, rollback, dan bootstrap proof
  - [x] Policy engine A/B + journal 128 byte ber-SHA-256 (`selfupdate`): hanya
    slot nonaktif, switch wajib setelah verifikasi, rollback setelah 3 boot
    gagal, promosi known-good (host tests)
  - [x] Journal di-seed oleh image builder dan installer (slot A known-good +
    digest kernel), dan diverifikasi saat read-back image
  - [x] Layout A/B di image builder: ESP + root A + root B, salinan kernel per
    slot, entri Limine per slot, journal A known-good di kedua root, dan
    verifikasi read-back ketiga partisi (host tests; `mkimage file ... ab` dan
    `mkimage sataN MKIMAGE-ERASE ab`)
  - [x] Image A/B hasil `mkimage ... ab` di-boot di QEMU/UEFI: kernel memilih
    root EXT2 persisten `sata0p2` dan journal melaporkan slot A `known-good`
  - [ ] Pemilihan slot oleh bootloader dari journal, penanda boot sukses, uji
    mati listrik, dan bootstrap berulang

Status awal pada branch roadmap: fondasi userspace, TinyCC, toolchain mini,
Rust `no_std`, ELF loader, dan installer persisten sudah ada. Tidak ada milestone
self-hosting yang dianggap selesai pada saat roadmap ini dibuat. Build Rust yang
menggunakan Cargo host tetap dikategorikan cross-build; QEMU smoke test baseline
juga masih memiliki kegagalan yang dicatat di dokumen audit.
