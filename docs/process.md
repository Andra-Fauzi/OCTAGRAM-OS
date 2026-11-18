# Proses, thread, dan sinyal

Ada dua model eksekusi di OCTAGRAM: task kernel kooperatif (`kernel/src/task.rs`)
dan proses user preemptive (`kernel/src/elf.rs`). Keduanya berbagi satu kernel
address space dan satu VFS global, tetapi hanya proses user yang punya page table
dan descriptor sendiri.

## Task kernel

- `task::spawn(future)` mendaftarkan future `Send + 'static`.
- `task::yield_now().await` menyerahkan giliran secara sukarela.
- `TaskHandle::wake()` hanya mengubah flag atomik, jadi aman dipanggil dari
  interrupt handler (dipakai keyboard untuk membangunkan shell).
- Idle loop mem-poll task yang siap sebelum `hlt`.

Task kernel **tidak** dipreempt dan berbagi hak akses kernel. Task yang
memblokir tanpa `yield` menghentikan seluruh kernel; karena itu jangan memanggil
`task::spawn` dari interrupt handler.

## Proses user

Batas dan sumber daya:

| Properti | Nilai | Lokasi |
| --- | --- | --- |
| Proses terjadwal maksimum | 64 | `MAX_SCHEDULED_PROCESSES` |
| Kernel stack per proses | 64 KiB | `PROCESS_KERNEL_STACK_SIZE` |
| Stack user | 8 page (32 KiB) | `STACK_PAGES` |
| Group/descriptor | per proses | tabel descriptor di `UserProcess` |

Setiap proses memiliki PID/TID, page table privat, daftar descriptor, current
working directory, `brk`, kernel stack, status sinyal, dan status keluar. Program
mewarisi cwd dari shell dan berbagi namespace VFS serta daftar mount global.

### Penjadwalan

`run` menjalankan satu program sampai selesai. `runp a.elf b.elf ...` menjalankan
beberapa program dengan scheduler berprioritas yang dipicu timer LAPIC periodik.
Preemption hanya terjadi bila interrupt datang dari ring 3; interrupt saat kernel
menjalankan syscall tidak mempreempt agar operasi yang memegang lock tidak terputus.

Setiap proses/thread menyimpan prioritas nice (-20..19, makin kecil makin tinggi)
dan umur tunggu (`age`). Setiap kali scheduler memilih, proses runnable yang tidak
terpilih mendapat `age` +1 sehingga prioritas efektifnya naik sampai 39 level;
proses terpilih atau yang tidak runnable mengembalikan `age` ke 0. Nilai efektif
dibatasi pada -20 agar tidak melewati rentang nice, seri diputus oleh umur tunggu
terlama, lalu oleh urutan round-robin mulai dari proses saat ini. Anak `fork` dan
thread baru mewarisi prioritas induknya.

Syscall `setpriority`/`getpriority` (134/135) menerima `PRIO_PROCESS`,
`PRIO_PGRP`, dan `PRIO_USER`; `who` 0 berarti pemanggil/grup/euid pemanggil dan
`which` tak dikenal mengembalikan `EINVAL`. Target yang tidak ada menghasilkan
`ESRCH`. Mengubah prioritas task lain butuh `CAP_SCHED_NICE`; non-root hanya boleh
untuk euid sendiri dan hanya menurunkan prioritas (nice membesar), selain itu
`EPERM`. `getpriority` untuk grup/user mengembalikan nice terkecil (prioritas
tertinggi) di antara task yang cocok. `nice()` di libc memakai kedua syscall
tersebut dan menjepit hasilnya ke rentang -20..19.

Pengawasan scheduler: shell `sched` mencetak PID/TID, nice, policy, `AGE`, dan
status setiap task (`scheduler_status()` menyalin snapshot dengan
`try_reserve`). Umur tunggu runnable ≥ 78 (dua kali boost aging) ditandai
`WATCHDOG` lewat `starvation_watchdog_trips()`. Selain itu watchdog idle di loop
scheduler mencetak `[sched] watchdog: idle wakeup delayed ...` bila `hlt` baru
bangun lebih dari 250 ms (indikasi interrupt macet) saat hanya satu proses
runnable. Belum ada akuntansi waktu CPU per task.

Kebijakan penjadwalan tersedia lewat `sched_setscheduler`/`sched_getscheduler`
(136/137) dan `sched_setparam`/`sched_getparam` (138/139), ditambah `sched.h`
(`sched_yield` memakai syscall 65). Hanya `SCHED_OTHER` dengan `sched_priority` 0
yang diterima; `SCHED_FIFO`/`SCHED_RR` ditolak `EPERM`. `posix_spawn` menerapkan
`POSIX_SPAWN_SETSCHEDULER`/`SETSCHEDPARAM` di anak sebelum `execve`, dan
prioritas bobot tetap diatur nice.

Syscall yang menunggu (pipe, `poll`, futex, `waitpid`, tidur, input console)
memakai `ProcessWait` sehingga proses tidur sampai event atau deadline, bukan
busy-loop: `Child(pid)`, `Thread(tid)`, `PipeReadable(pipe)`,
`PipeWritable(pipe)`, `Futex { address, bitset }`, `PollWait`, `Sleep`, dan
`ConsoleInput(terminal)`.

`read()` dari descriptor console tidak lagi berputar di kernel; hal yang sama
berlaku untuk tty yang dibuka lewat path (`/dev/stdin`, `/dev/tty*`,
`/dev/console`) karena `fs::tty_path_terminal()` memetakan path ke terminal dan
`syscall_read` memakai `poll_tty()` di jalur file. `poll_tty()` mengembalikan
`Ok(None)` bila belum ada data; syscall lalu menyimpan state dan memilih proses
lain. Setiap putaran idle scheduler memanggil
`pump_console_input()`, yang memindahkan event keyboard ke line discipline semua
terminal dan mengembalikan bitmask terminal siap baca; penunggu dengan terminal
yang cocok dibangunkan (`wake_console_waiters`). Bila tidak ada proses lain yang
runnable, jalur lama `fs::read_tty` (blocking dengan `hlt`) dipakai agar proses
tunggal seperti shell tidak menerima `EDEADLK`. EOF (`Ctrl-D` pada baris kosong)
disimpan sebagai `eof_pending` sehingga `read()` mengembalikan 0 dan `poll`
melaporkan siap.

`poll`/`select` yang belum siap menyimpan `ProcessWait::PollWait` dengan deadline
retry 1 ms. Sejak wait itu dibangunkan oleh perubahan isi pipe, input console,
dan penulisan `eventfd` (`wake_poll_waiters`/`is_poll_wait`), scan ulang terjadi
segera; deadline 1 ms tetap ada sebagai jaring pengaman untuk sumber lain
(socket, timerfd). Setiap wait tanpa batas waktu mengosongkan `wait_deadline`
lama, dan setiap pembangunanan wait membatalkan deadline-nya, sehingga proses
tidak pernah terbangun oleh timeout yang sudah basi.

### Membuat dan mengakhiri proses

- `fork` menyalin halaman user secara eager (tanpa copy-on-write). Kunci memori
  tidak diwarisi: `mlockall` (`lock_future_mappings`) dimatikan dan setiap
  `MappingPage.locked` hasil klon di-reset sesuai POSIX.
- `execve`/`execvp` memakai address space yang benar-benar baru (hanya cwd,
  terminal, pid, umask, grup/sesi, dan descriptor non-`CLOEXEC` yang diwarisi);
  stack baru diberi entropi, signal handler di-reset sesuai aturan `exec`.
- `waitpid` menerima PID tertentu atau `WAIT_ANY_PID`; status keluar dan sinyal
  pelapor disimpan sampai diambil induk.
- Saat proses keluar atau fault, scheduler memilih proses berikutnya dan page
  milik proses dilepas setelah grupnya selesai.

### Sinyal

Sinyal dikirim lewat `kill`/`tgkill`, disimpan sebagai bitmask pending, dan
disampaikan saat proses kembali ke ring 3. Yang tersedia:

- `sigaction`/`signal`, `sigprocmask`, `sigpending`, `sigsuspend`, `sigreturn`;
- alternate signal stack lewat `sigaltstack` (syscall 133) dengan `SA_ONSTACK`,
  `SS_ONSTACK`, dan `SS_DISABLE`;
- `SIGKILL`/`SIGSTOP` tidak dapat ditahan; sinyal lain bisa diabaikan atau
  ditangani handler user.

### Grup dan sesi

`setpgid`/`getpgrp` (130/129) dan `setsid`/`getsid` (132/131) tersedia. Grup
proses dipakai untuk pengiriman sinyal (`kill(-pgid, ...)`).

### Kredensial dan kemampuan

Setiap task menyimpan `uid`, `euid`, `gid`, `egid`, dan bitmask kemampuan;
semua thread dengan pid sama berbagi nilai ini dan anak `fork` mewarisinya.
Awalnya uid/gid 0 dengan `CAP_ALL`. `setuid`/`seteuid`/`setgid`/`setegid`
(140–143) mengubahnya: root bebas, non-root hanya boleh kembali ke id
nyata/efektifnya (`credential_change_allowed`); menurunkan uid ke selain 0
mencabut semua kemampuan (`capabilities_for_uid`).

Kemampuan yang didefinisikan: `CAP_VFS_WRITE`, `CAP_NETWORK`, `CAP_SYS_ADMIN`,
`CAP_SCHED_NICE`. `capget` (144) membaca bitmask, `capset` (145) menulisnya dan
membutuhkan `CAP_SYS_ADMIN`; bit di luar `CAP_ALL` ditolak `EINVAL`.

Penegakan baru sebagian: `CAP_VFS_WRITE` diperlukan untuk `open` dengan niat tulis
(`O_WRONLY`/`O_RDWR`/`O_CREAT`/`O_TRUNC`/`O_APPEND`), `write` ke file biasa,
`mkdir`, `unlink`, `rmdir`, `unlinkat`, `rename`, `symlink`, `chmod`, dan
`ftruncate`; `CAP_NETWORK` diperlukan untuk `socket()` AF_INET; `CAP_SCHED_NICE`
untuk menaikkan prioritas task lain. Pelanggaran mengembalikan `EPERM`. Yang belum
diperiksa: mount/device, akses console, dan descriptor yang sudah terbuka sebelum
uid diturunkan. Header userspace-nya `sys/capability.h`, contoh `credential_demo`.

## Sinkronisasi dan futex

`pthread_mutex`, condition variable, dan semaphore di libc dibangun di atas
syscall `futex` (94). Kernel mendukung:

- `FUTEX_WAIT`/`FUTEX_WAKE` dengan timeout relatif dan deadline LAPIC;
- `FUTEX_WAIT_BITSET` (timeout absolut, `FUTEX_CLOCK_REALTIME`),
  `FUTEX_WAKE_BITSET`, dan `FUTEX_REQUEUE`;
- flag private (`FUTEX_PRIVATE_FLAG`).

Robust futex didukung di sisi kernel: `set_robust_list`/`get_robust_list`
(146/147) menyimpan alamat `robust_list_head` per task (panjang wajib 24 byte).
Saat task keluar, `release_robust_list()` menelusuri daftar (maksimum 64 entri,
melewati `list_op_pending`) dan untuk setiap futex yang TID-nya masih dimiliki
task itu menulis `FUTEX_OWNER_DIED`, mempertahankan `FUTEX_WAITERS`, melepas TID,
lalu membangunkan waiter pada alamat tersebut. API libc seperti
`pthread_mutexattr_setrobust` dan `EOWNERDEAD` belum ada, jadi program memakai
syscall langsung lewat `octagram.h`.

## Batasan

- Prioritas hanya `SCHED_OTHER`; tanpa real-time scheduling.
- `sched_*` hanya menerima `SCHED_OTHER`/prioritas 0; tidak ada penjadwalan
  real-time.
- Tanpa SMP dan tanpa migrasi thread antar CPU.
- Thread user berbagi address space tetapi belum punya TLS dinamis penuh untuk
  shared object (lihat [dynamic-linker.md](dynamic-linker.md)).
- Blocking syscall tertentu masih menunggu di kernel dengan polling input, bukan
  sepenuhnya digerakkan event.

## Uji

`cargo test` menyertakan uji host untuk helper penjadwalan (`next_runnable`),
pencocokan bitset futex, `wait_target_matches`, tata letak stack, dan
`signal_stack_top`. Perilaku runtime penuh hanya bisa dilihat dengan menjalankan
OS di QEMU.
