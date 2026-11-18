# OCTAGRAM syscall ABI

Syscall dilakukan dengan `int $0x80`. Nomor syscall ada di `RAX`; tiga
argumen pertama berada di `RDI`, `RSI`, `RDX`. Nilai balik ditulis ke `RAX`.
Hasil sukses tidak negatif. Error dikembalikan sebagai nilai negatif errno
(`-errno`), sign-extend sebagai `int64_t`. Header `user/include/octagram.h`
menyediakan inline wrappers.

## Tabel syscall

| `RAX` | Nama | Argumen | Hasil |
|---:|---|---|---|
| 0 | `exit` | `RDI=status` | Tidak kembali; kembali ke shell |
| 1 | `write` | `RDI=fd`, `RSI=buffer`, `RDX=count` | Byte yang ditulis |
| 2 | `read` | `RDI=fd`, `RSI=buffer`, `RDX=count` | Byte yang dibaca |
| 3 | `open` | `RDI=path`, `RSI=flags`, `RDX=mode` | Descriptor baru |
| 4 | `close` | `RDI=fd` | 0 |
| 5 | `lseek` | `RDI=fd`, `RSI=offset`, `RDX=whence` | Offset baru |
| 6 | `stat` | `RDI=path`, `RSI=struct oct_stat*` | 0 |
| 7 | `mkdir` | `RDI=path` | 0 |
| 8 | `unlink` | `RDI=path` | 0 |
| 9 | `ftruncate` | `RDI=fd`, `RSI=length` | 0 |
| 11 | `getpid` | — | PID proses saat ini |
| 12 | `isatty` | `RDI=fd` | 1 untuk console |
| 13 | `chdir` | `RDI=path` | 0 |
| 14 | `getcwd` | `RDI=buffer`, `RSI=size` | Byte path termasuk NUL |
| 16 | `mmap` | `RDI=address`, `RSI=length`, `RDX=protection`, `R10=flags`, `R8=fd`, `R9=offset` | Alamat awal atau `-errno` |
| 17 | `munmap` | `RDI=address`, `RSI=length` | 0 |
| 20 | `waitpid` | `RDI=pid` atau `OCT_WAIT_ANY`, `RSI=status*` (0 untuk abaikan), `RDX=options` | PID anak yang selesai, 0 untuk `WNOHANG` |
| 21 | `pipe` | `RDI=int fds[2]` | 0; `fds[0]` read, `fds[1]` write |
| 22 | `dup2` | `RDI=source_fd`, `RSI=target_fd` | `target_fd` |
| 25 | `chmod` | `RDI=path`, `RSI=mode` | 0 |
| 27 | `opendir` | `RDI=path` | Descriptor direktori |
| 28 | `readdir` | `RDI=fd`, `RSI=struct dirent*` | 1 untuk entri, 0 untuk EOF |
| 29 | `fstat` | `RDI=fd`, `RSI=struct oct_stat*` | 0 |
| 30 | `access` | `RDI=path`, `RSI=R_OK/W_OK/X_OK` | 0 |
| 31 | `rename` | `RDI=source`, `RSI=target` | 0 |
| 32 | `umask` | `RDI=mask` | Mask sebelumnya |
| 33 | `dup` | `RDI=fd` | Descriptor duplikat terendah |
| 34 | `fcntl` | `RDI=fd`, `RSI=command`, `RDX=argument` | Nilai sesuai command |
| 35 | `rmdir` | `RDI=path` | 0 |
| 36 | `symlink` | `RDI=target`, `RSI=link_path` | 0 |
| 37 | `readlink` | `RDI=path`, `RSI=buffer`, `RDX=size` | Jumlah byte target |
| 38 | `lstat` | `RDI=path`, `RSI=struct oct_stat*` | 0 |
| 39 | `fchmod` | `RDI=fd`, `RSI=mode` | 0 |
| 40 | `openat` | `RDI=dirfd`, `RSI=path`, `RDX=flags`, `R10=mode` | Descriptor baru |
| 41 | `fstatat` | `RDI=dirfd`, `RSI=path`, `RDX=struct oct_stat*`, `R10=flags` | 0 |
| 42 | `mkdirat` | `RDI=dirfd`, `RSI=path`, `RDX=mode` | 0 |
| 43 | `unlinkat` | `RDI=dirfd`, `RSI=path`, `RDX=flags` | 0 |
| 44 | `renameat` | `RDI=olddirfd`, `RSI=oldpath`, `RDX=newdirfd`, `R10=newpath` | 0 |
| 45 | `readlinkat` | `RDI=dirfd`, `RSI=path`, `RDX=buffer`, `R10=size` | Jumlah byte target |
| 46 | `symlinkat` | `RDI=target`, `RSI=newdirfd`, `RDX=link_path` | 0 |
| 47 | `execve` | `RDI=path`, `RSI=argv`, `RDX=envp` | Tidak kembali saat sukses |
| 48 | `fork` | — | PID child di parent, 0 di child |
| 50 | `clock_gettime` | `RDI=CLOCK_REALTIME`, `RSI=struct timespec*` | 0 |
| 51 | `msync` | `RDI=address`, `RSI=length`, `RDX=flags` | 0 |
| 52 | `mprotect` | `RDI=address`, `RSI=length`, `RDX=protection` | 0 |
| 53 | `mincore` | `RDI=address`, `RSI=length`, `RDX=vector` | 0 |
| 54 | `mlock` | `RDI=address`, `RSI=length` | 0 |
| 55 | `munlock` | `RDI=address`, `RSI=length` | 0 |
| 56 | `mlockall` | `RDI=flags` | 0 |
| 57 | `munlockall` | — | 0 |
| 58 | `posix_madvise` | `RDI=address`, `RSI=length`, `RDX=advice` | 0 |
| 59 | `set_thread_pointer` | `RDI=thread_pointer` | 0 |
| 60 | `get_thread_pointer` | — | `FS.base` thread pointer |
| 61 | `thread_create` | `RDI=entry`, `RSI=argument`, `RDX=stack_top`, `R10=FS.base`, `R8=owned_mapping_base`, `R9=owned_mapping_length` | Thread ID |
| 62 | `thread_exit` | `RDI=result` | Tidak kembali |
| 63 | `thread_join` | `RDI=thread_id`, `RSI=result*` (0 untuk abaikan) | 0 |
| 64 | `thread_self` | — | Thread ID saat ini |
| 65 | `kernel_yield` | — | 0; jalankan driver tertunda lalu lanjutkan proses |
| 66 | `udp_socket` | — | Descriptor socket UDP |
| 67 | `udp_bind` | `RDI=fd`, `RSI=port` (0 untuk ephemeral) | Port lokal yang terikat |
| 68 | `udp_sendto` | `RDI=fd`, `RSI=IPv4`, `RDX=port`, `R10=buffer`, `R8=count` | Byte terkirim |
| 69 | `udp_recvfrom` | `RDI=fd`, `RSI=buffer`, `RDX=count`, `R10=source_ip[4]` (0 untuk abaikan), `R8=source_port*` (0 untuk abaikan) | Panjang datagram asli (bisa lebih besar dari `count`; libc memotong dan melaporkan `MSG_TRUNC`) |
| 70 | `network_dns` | `RDI=IPv4[4]` | 0; `-ENETDOWN` jika DNS belum dikonfigurasi |
| 71 | `socket` | `RDI=domain`, `RSI=type`, `RDX=protocol` | Descriptor socket |
| 72 | `bind` | `RDI=fd`, `RSI=sockaddr_in*`, `RDX=length` | 0 |
| 73 | `listen` | `RDI=fd`, `RSI=backlog` | 0 |
| 74 | `accept` | `RDI=fd`, `RSI=sockaddr_in*` (0 untuk abaikan), `RDX=socklen_t*` | Descriptor koneksi baru |
| 75 | `connect` | `RDI=fd`, `RSI=sockaddr_in*`, `RDX=length` | 0; pada socket UDP menyimpan peer sehingga `send`/`recv` tidak perlu alamat |
| 76 | `send` | `RDI=fd`, `RSI=buffer`, `RDX=count`, `R10=flags` | Byte terkirim; memakai peer `connect` untuk socket UDP |
| 77 | `recv` | `RDI=fd`, `RSI=buffer`, `RDX=count`, `R10=flags` | Byte yang dibaca; socket UDP terhubung hanya menerima datagram dari peer |
| 78 | `shutdown` | `RDI=fd`, `RSI=how` (`SHUT_RD/WR/RDWR`) | 0 |
| 79 | `getsockname` | `RDI=fd`, `RSI=sockaddr_in*`, `RDX=socklen_t*` | 0 |
| 80 | `getpeername` | `RDI=fd`, `RSI=sockaddr_in*`, `RDX=socklen_t*` | 0 |
| 81 | `ioctl` | `RDI=fd`, `RSI=request`, `RDX=arg` | Hasil sesuai request |
| 82 | `getuid` | — | 0 |
| 83 | `getgid` | — | 0 |
| 84 | `geteuid` | — | 0 |
| 85 | `getegid` | — | 0 |
| 86 | `uname` | `RDI=struct utsname*` | 0 |
| 87 | `getppid` | — | 1 |
| 88 | `brk` | `RDI=address` (0 untuk membaca) | Program break saat ini/baru |
| 89 | `gettimeofday` | `RDI=struct timeval*` (0 untuk abaikan) | 0 |
| 90 | `nanosleep` | `RDI=struct timespec*` | 0 |
| 91 | `kill` | `RDI=pid`, `RSI=signal` | 0, `-ESRCH` bila pid tidak ada |
| 92 | `sigaction` | `RDI=signal`, `RSI=action*`, `RDX=old*` | 0, `-EINVAL` untuk `SIGKILL`/`SIGSTOP` |
| 93 | `sigprocmask` | `RDI=how`, `RSI=set*`, `RDX=old*` | 0 |
| 94 | `futex` | `RDI=address`, `RSI=operation`, `RDX=value`, `R10=timeout* `/`nr_requeue`, `R8=address2`, `R9=bitset` | 0 untuk `WAIT`/`WAIT_BITSET` bangun normal, jumlah waiter untuk `WAKE`/`WAKE_BITSET`/`REQUEUE`, `-ETIMEDOUT` saat timeout |
| 95 | `setsockopt` | `RDI=fd`, `RSI=level`, `RDX=option`, `R10=value*`, `R8=length` | 0 |
| 96 | `getsockopt` | `RDI=fd`, `RSI=level`, `RDX=option`, `R10=value*`, `R8=length*` | 0 |
| 97 | `poll` | `RDI=struct pollfd*`, `RSI=nfds`, `RDX=timeout_ms` | Jumlah fd siap |
| 98 | `pipe2` | `RDI=int fds[2]` | 0; flag diabaikan |
| 99 | `dup3` | `RDI=source_fd`, `RSI=target_fd`, `RDX=flags` | `target_fd` |
| 100 | `eventfd2` | `RDI=initial_value`, `RSI=flags` | fd baru |
| 101 | `timerfd_create` | `RDI=clock_id`, `RSI=flags` | fd baru |
| 102 | `pread` | `RDI=fd`, `RSI=buffer`, `RDX=count`, `R10=offset` | Byte dibaca |
| 103 | `pwrite` | `RDI=fd`, `RSI=buffer`, `RDX=count`, `R10=offset` | Byte ditulis |
| 104 | `writev` | `RDI=fd`, `RSI=iov*`, `RDX=iovcnt` | Byte ditulis |
| 105 | `readv` | `RDI=fd`, `RSI=iov*`, `RDX=iovcnt` | Byte dibaca |
| 106 | `sendmsg` | — | `-ENOSYS`; libc menyusunnya dari `send`/`sendto`/`write` |
| 107 | `recvmsg` | — | `-ENOSYS`; libc menyusunnya dari `recv`/`recvfrom`/`read` |
| 108 | `sysconf` | `RDI=nama _SC_*` | Nilai atau -1 |
| 109 | `getrlimit` | `RDI=resource`, `RSI=struct rlimit*` | 0; selalu tak terbatas |
| 110 | `setrlimit` | `RDI=resource`, `RSI=struct rlimit*` | 0; diabaikan |
| 111 | `getrusage` | `RDI=who`, `RSI=struct rusage*` | 0; nilai nol |
| 112 | `times` | `RDI=struct tms*` (0 untuk abaikan) | 0; nilai nol |
| 113 | `sigreturn` | frame di `RSP` | Memulihkan seluruh register dan mask |
| 114 | `sigpending` | `RDI=set*` | 0, mask sinyal tertunda yang terblokir |
| 115 | `sigsuspend` | `RDI=mask*` | 0 setelah ada sinyal atau batas tunggu |
| 116 | `signal` | `RDI=signal`, `RSI=handler` | Handler sebelumnya |
| 117 | `select` | `RDI=nfds`, `RSI=readfds*`, `RDX=writefds*`, `R10=exceptfds*`, `R8=timeval*` | Jumlah fd siap |
| 118 | `clock_getres` | `RDI=clock_id`, `RSI=timespec*` | 0 |
| 119 | `fsync` | `RDI=fd` | 0 untuk file/direktori, `-EINVAL` untuk pipe/console/socket |
| 120 | `fdatasync` | `RDI=fd` | Sama dengan `fsync` |
| 121 | `getrandom` | `RDI=buffer`, `RSI=length`, `RDX=flags` | Byte yang diisi (maks 256 per panggilan) |
| 122 | `epoll_create1` | `RDI=flags` | fd epoll baru |
| 123 | `epoll_ctl` | `RDI=epfd`, `RSI=op`, `RDX=fd`, `R10=event*` | 0, `-EEXIST`, atau `-ENOENT` |
| 124 | `epoll_wait` | `RDI=epfd`, `RSI=events*`, `RDX=maxevents`, `R10=timeout_ms` | Jumlah event siap |
| 125 | `timerfd_settime` | `RDI=fd`, `RSI=flags`, `RDX=new*`, `R10=old*` | 0 |
| 126 | `timerfd_gettime` | `RDI=fd`, `RSI=current*` | 0 |
| 127 | `socketpair` | `RDI=domain`, `RSI=type`, `RDX=protocol`, `R10=sv*` | 0 |
| 128 | `clock_nanosleep` | `RDI=clock_id`, `RSI=flags`, `RDX=request*`, `R10=remain*` | 0 |
| 129 | `getpgrp` | — | Process group ID |
| 130 | `setpgid` | `RDI=pid`, `RSI=process_group` | 0 |
| 131 | `getsid` | `RDI=pid` atau 0 | Session ID |
| 132 | `setsid` | — | Session/process group ID baru |
| 133 | `sigaltstack` | `RDI=stack_t* baru` (0 untuk baca), `RSI=stack_t* lama` | 0, `-EINVAL`, `-EPERM` bila stack alternatif sedang dipakai |
| 134 | `setpriority` | `RDI=which` (0 `PRIO_PROCESS`, 1 `PRIO_PGRP`, 2 `PRIO_USER`), `RSI=who` (0 = pemanggil/grup/euid pemanggil), `RDX=priority` (-20..19, dijepit) | 0, `-EINVAL`, `-EPERM` (butuh `CAP_SCHED_NICE` untuk task lain), `-ESRCH` |
| 135 | `getpriority` | `RDI=which`, `RSI=who` | Prioritas nice yang dibias +20 (0..39) dari task dengan prioritas tertinggi yang cocok, `-EINVAL`, `-ESRCH` |
| 136 | `sched_setscheduler` | `RDI=pid` (0 = pemanggil), `RSI=policy`, `RDX=sched_param*` | 0, `-EPERM` (hanya `SCHED_OTHER`), `-EINVAL`, `-ESRCH` |
| 137 | `sched_getscheduler` | `RDI=pid` | Policy (selalu 0 = `SCHED_OTHER`), `-ESRCH` |
| 138 | `sched_getparam` | `RDI=pid`, `RSI=sched_param*` | 0 setelah menulis prioritas 0, `-EFAULT`, `-ESRCH` |
| 139 | `sched_setparam` | `RDI=pid`, `RSI=sched_param*` | 0 bila prioritas 0, `-EINVAL`, `-ESRCH` |
| 140 | `setuid` | `RDI=uid` | 0, `-EPERM` bila non-root menaikkan id |
| 141 | `seteuid` | `RDI=uid` | 0, `-EPERM` |
| 142 | `setgid` | `RDI=gid` | 0, `-EPERM` |
| 143 | `setegid` | `RDI=gid` | 0, `-EPERM` |
| 144 | `capget` | `RDI=pid` (0 = pemanggil), `RSI=uint64_t*` | 0 setelah menulis bitmask, `-EFAULT`, `-ESRCH` |
| 145 | `capset` | `RDI=pid`, `RSI=mask` | 0, `-EPERM` tanpa `CAP_SYS_ADMIN`, `-EINVAL`, `-ESRCH` |

Nomor 10, 15, 18, 19, 23, 24, 26, dan 49 sudah dicabut dan tetap
dicadangkan agar nomor syscall berikutnya tidak bergeser; pemanggilannya
mengembalikan `-ENOSYS`. API POSIX `posix_spawn` disediakan runtime melalui
`fork` dan `execve`, bukan syscall khusus.

Syscall 82-117 melayani runtime bahasa tingkat tinggi seperti Python dan
Java. `getuid`, `getgid`, `geteuid`, dan `getegid` membaca kredensial proses
(awalnya 0); `getppid` mengembalikan 1. `setuid`/`seteuid`/`setgid`/`setegid`
(140–143) mengubahnya untuk seluruh thread satu pid: root bebas, non-root hanya
boleh kembali ke id nyata/efektifnya. `capget`/`capset` (144/145) mengelola bit
kemampuan (`CAP_VFS_WRITE`, `CAP_NETWORK`, `CAP_SYS_ADMIN`, `CAP_SCHED_NICE`).
Penulisan VFS, `socket()` AF_INET, dan prioritas task lain mengembalikan `-EPERM`
bila proses tidak memegang bit yang sesuai (root memegang `CAP_ALL`). `uname` mengisi struct
`utsname` berisi enam field 65 byte dengan identitas OCTAGRAM.

`brk` mengelola program break: alamat 0 membaca nilai saat ini, alamat yang lebih
tinggi memetakan halaman anonim RW, dan alamat yang lebih rendah tidak
menyusutkan heap. `gettimeofday` melaporkan detik Unix dari RTC dengan `tv_usec`
selalu 0. `nanosleep` menunggu secara sibuk di kernel (belum scheduler-aware),
dibatasi total 60 detik per panggilan; `sleep` dan `usleep` di runtime memakai
syscall ini.

Signal sudah nyata. Setiap proses menyimpan `pending_signals`, `blocked_signals`,
dan tabel 31 handler. `kill` menambahkan bit pending pada proses target (diri
sendiri bila `pid` 0 atau sama dengan pid saat ini, `-ESRCH` bila tidak ada);
`sigaction`/`signal` menyimpan handler, mask tambahan, flag, dan restorer.
Sinyal dikirim tepat sebelum kembali ke mode user dari sebuah syscall: kernel
menulis `UserSignalFrame` (16 register, `rip`, `rflags`, `rsp`, mask, nomor
sinyal) ke stack user, menaruh alamat restorer di bawahnya, lalu mengarahkan
`rip` ke handler dengan `rdi` berisi nomor sinyal dan `rsi`/`rdx` menunjuk frame.
Handler mengembalikan konteks lewat `sigreturn` (113), yang dipasang otomatis
oleh library C sebagai restorer bila program tidak mengisinya. Sinyal yang
sedang ditangani otomatis diblokir; `sigprocmask` menahan pengiriman dan
`sigpending` melaporkan bit yang tertunda. `SIGKILL` dan `SIGSTOP` tidak bisa
ditangani atau diblokir. Handler `SIG_IGN` membuang sinyal, sedangkan default
untuk `SIGCHLD`, `SIGCONT`, `SIGURG`, dan `SIGWINCH` diabaikan dan sinyal lain
menghentikan proses dengan status `128 + nomor`. `SA_RESETHAND` mengembalikan
handler ke default setelah sekali jalan, dan `SA_RESTART` membuat syscall yang
terinterupsi diulang otomatis setelah handler selesai (restorer menulis ulang
`rip` ke instruksi `int 0x80` dengan nomor syscall asli). `SA_SIGINFO` dan
`SA_NOCLDSTOP` diterima tetapi belum mengubah perilaku. `sigaltstack` (133)
menyimpan stack alternatif; handler ber-`SA_ONSTACK` menulis frame dan berjalan
di puncak stack itu selama stack alternatif belum aktif, lalu `sigreturn`
melepasnya kembali. Pembacaan melaporkan `SS_ONSTACK` bila handler sedang
memakainya dan `SS_DISABLE` bila tidak ada stack alternatif.

Sinyal membangunkan proses yang sedang menunggu di `read`/`write` pada pipe,
`poll`, `select`, dan `futex`; syscall tersebut kembali dengan `-EINTR` kecuali
handler-nya memakai `SA_RESTART`. `kill` ke pid sendiri juga menyapu thread
saudara yang sedang menunggu, bukan hanya thread pemanggil. Untuk itu `poll` dan
`select` tidak lagi tidur di kernel: keduanya memblokir lewat scheduler dengan
tick 1 ms sambil menyimpan deadline total per proses, sehingga thread lain bisa
berjalan dan mengirim sinyal. `nanosleep`/`clock_nanosleep` hanya memeriksa
sinyal sebelum tidur, belum bisa dibangunkan di tengah tidur.

`futex` sudah nyata untuk `FUTEX_WAIT` dan `FUTEX_WAKE` (flag `FUTEX_PRIVATE_FLAG`
dan `FUTEX_CLOCK_REALTIME` diabaikan). `FUTEX_WAIT` membandingkan word 32-bit di
`address` dengan `value`: bila berbeda langsung `-EAGAIN`, bila sama proses
diblokir dan dijadwalkan keluar sampai ada `FUTEX_WAKE` atau `timeout` relatif
(`R10` menunjuk `timespec`) habis. Timeout diperiksa timer LAPIC sehingga
`-ETIMEDOUT` dikembalikan tanpa polling dari user space. `FUTEX_WAKE` membangunkan
hingga `value` waiter pada alamat yang sama dan mengembalikan jumlahnya. Bila
tidak ada proses lain yang bisa dijalankan dan tidak ada timeout, `FUTEX_WAIT`
mengembalikan `-EDEADLK`. Bila timeout ada tetapi belum ada thread siap, kernel
tidak tidur memblokir CPU: waiter tetap berstatus menunggu, kernel bangun tiap
1 ms untuk memeriksa deadline dan langsung menjadwalkan thread yang menjadi siap,
lalu mengembalikan `-ETIMEDOUT` saat deadline lewat. Operasi lain (mis. `FUTEX_REQUEUE`, `WAIT_BITSET`)
masih `-ENOSYS`, dan `FUTEX_WAIT` yang bangun karena wake mengembalikan 0 meski
word tidak berubah.

`epoll` memakai `struct epoll_event` padat (`u32 events`, `u64 data`, 12 byte).
`epoll_create1` menerima flag apa pun tanpa mengubah perilaku; `epoll_ctl`
mendukung `ADD`, `MOD`, dan `DEL` dengan semantik errno Linux (`EEXIST` untuk
fd ganda, `ENOENT` bila entri tidak ada, `EINVAL` bila target adalah epoll lain).
`epoll_wait` menghitung readiness dari state kernel yang sama dengan `poll`, jadi
pipe, TTY, socket, eventfd, dan timerfd bekerja tanpa registrasi tambahan.
Level-triggered adalah default; `EPOLLET` hanya melaporkan transisi readiness
0 → siap sehingga data yang belum dibaca tidak dilaporkan ulang, dan
`EPOLLONESHOT` mematikan minat setelah satu laporan sampai `EPOLL_CTL_MOD`
mengarm ulang. Menunggu memakai polling 1 ms sampai batas 60 detik.

`eventfd2` menyimpan counter 64-bit: `write` menambahkan nilai 8 byte (gagal
`EAGAIN` bila akan meluap) dan `read` mengembalikan counter lalu memusatkannya
ke nol, atau mengembalikan 1 per panggilan bila flag `EFD_SEMAPHORE` dipakai.
`timerfd_create` menerima `CLOCK_REALTIME`, `CLOCK_MONOTONIC`, dan
`CLOCK_BOOTTIME`; `timerfd_settime` mendukung `TFD_TIMER_ABSTIME` untuk clock
monoton, dan `read` mengembalikan jumlah expiration yang sudah lewat sebagai
`u64`. `socketpair` hanya untuk `AF_UNIX` dengan `SOCK_STREAM`/`SOCK_DGRAM` dan
dibangun dari dua pipe sehingga dua arah; `shutdown` belum berlaku untuknya.
`clock_nanosleep` menerima clock realtime/monoton/boottime dan `TIMER_ABSTIME`,
dengan sisa waktu selalu diisi nol karena tidur berjalan di kernel dan dibatasi
60 detik seperti `nanosleep`.

Status flag descriptor disimpan per fd di tabel yang sama dengan `FD_CLOEXEC`.
`open`/`openat` menerima `O_NONBLOCK`, `pipe2` menerima `O_NONBLOCK` dan
`O_CLOEXEC`, dan `fcntl` mendukung `F_GETFL`/`F_SETFL` untuk `O_NONBLOCK` dan
`O_APPEND`. Descriptor nonblocking tidak pernah masuk jalur blokir: `read`/`write`
mengembalikan `-EAGAIN` apa adanya. Library C memeriksa status itu lewat
`F_GETFL` sehingga `read`/`write` hanya memutar ulang `EAGAIN` pada descriptor
blocking, sesuai POSIX.

`poll` dan `select` menghitung readiness dari state kernel, bukan menebak:
pipe siap baca ketika ada byte di buffer atau semua writer sudah ditutup
(disertai `POLLHUP`), siap tulis selama buffer belum penuh, dan descriptor yang
tidak ada dilaporkan `POLLNVAL`. TTY siap baca setelah satu baris lengkap masuk
line discipline; event keyboard yang masih antre ikut dipompa saat readiness
diperiksa, dan selama menunggu interrupt tetap aktif sehingga huruf yang diketik
ketika `poll` berjalan tetap terlihat. Batas tunggu 60 detik. `select` memakai
`fd_set` berisi `uint64_t` dengan bit berurut little-endian dan batas 1024
descriptor. `setsockopt` menerima opsi
`SOL_SOCKET` tanpa mengubah perilaku jaringan; `getsockopt` mengisi `SO_TYPE`
(`SOCK_STREAM` atau `SOCK_DGRAM`) dan `SO_ERROR` (0), sedangkan opsi lain
mengembalikan `-ENOPROTOOPT`.

`sysconf` memakai penomoran `_SC_*` gaya glibc: `_SC_ARG_MAX` 0, `_SC_CHILD_MAX`
1, `_SC_CLK_TCK` 2, `_SC_NGROUPS_MAX` 3, `_SC_OPEN_MAX` 4, `_SC_PAGESIZE` 30,
`_SC_NPROCESSORS_CONF` 83, dan `_SC_NPROCESSORS_ONLN` 84. Nama lain menghasilkan
-1. `setenv`, `putenv`, dan `unsetenv` tetap berada di runtime user, bukan
syscall. `getrandom` mengisi hingga 256 byte per panggilan dari PRNG xorshift
yang di-seed TSC; flag `GRND_*` diterima tetapi belum mengubah perilaku.
`fsync` dan `fdatasync` sukses untuk file dan direktori RAM, dan mengembalikan
`-EINVAL` untuk pipe, console, serta socket seperti POSIX.

## Locale dan konversi UTF-8

`setlocale` mengenal `"C"`, `"POSIX"`, `"C.UTF-8"`, `"C.utf8"`, `"UTF-8"`,
`"utf8"`, dan `"en_US.UTF-8"`; query (`locale == NULL`) mengembalikan nama yang
sedang aktif dan locale lain mengembalikan `NULL`. String kosong memilih locale
dari `LC_ALL`, lalu `LC_CTYPE`, lalu `LANG`: bernilai UTF-8 menjadi `"C.UTF-8"`,
selain itu `"C"`. `MB_CUR_MAX` dan `MB_LEN_MAX` bernilai 4. Kategori diabaikan
(satu locale untuk semua). `localeconv` mengembalikan `struct lconv` dengan
`decimal_point = "."` dan `thousands_sep = ""`, field lain nol atau 127 seperti
glibc untuk nilai yang tidak tersedia.

`mbrtowc`, `wcrtomb`, `c16rtomb`, dan `c32rtomb` (di `<wchar.h>` dan `<uchar.h>`)
mengimplementasikan UTF-8 murni:

- `mbrtowc` menerima 1–4 byte, memvalidasi overlong, surrogate, dan batas
  `U+10FFFF`; urutan belum lengkap mengembalikan `(size_t)-2` dan menyimpan byte
  parsial di `mbstate_t`, byte invalid mengembalikan `(size_t)-1` dengan `EILSEQ`.
- `wcrtomb`/`c32rtomb` menolak surrogate dan nilai di atas `U+10FFFF` dengan
  `EILSEQ`.
- `c16rtomb` menyimpan high surrogate di `mbstate_t`, menggabungkannya dengan low
  surrogate menjadi satu karakter UTF-8 4 byte, dan menolak low surrogate tanpa
  pasangan atau high surrogate ganda (state direset saat `EILSEQ`).
  `c16rtomb(NULL, ...)` mereset state dan mengembalikan 1.

`mbstate_t` adalah struct milik OCTAGRAM (`count`, `bytes[4]`, `pending`), bukan
layout glibc, jadi state tidak boleh dibagi dengan binary yang dibangun untuk
glibc. Belum ada `mbsrtowcs`, `wcsrtombs`, `mbrtoc16`, `mbrtoc32`, collation,
atau encoding selain UTF-8.

## Termios dan isatty

`<termios.h>` menyediakan `struct termios` dengan layout x86_64 seperti glibc
(`tcflag_t`/`cc_t`/`speed_t`, 32 `c_cc`, kecepatan in/out), konstanta flag
(`ICANON`, `ECHO`, `ISIG`, `OPOST`, `ICRNL`, `IXON`, `CS8`, ...), indeks `c_cc`
(`VINTR`, `VERASE`, `VEOF`, `VMIN`, `VTIME`, ...), dan action `TCSANOW`,
`TCSADRAIN`, `TCSAFLUSH`.

Syscall `ioctl` (79) kini menangani `TCGETS` dan `TCSETS`/`TCSETSW`/`TCSETSF`
pada console dan `/dev/tty*`, di samping `TIOCGWINSZ` yang sudah ada. Kernel
menyimpan satu state termios console (default canonical: `ICANON|ECHO|ISIG`,
`ICRNL|IXON`, `OPOST|ONLCR`, `CREAD|CS8|B38400`) dan menyalinnya bolak-balik ke
user space. `isatty` di libc memakai `TCGETS`, jadi fd pipe, socket, dan file
biasa mengembalikan 0 dengan `ENOTTY`. `tcgetattr`, `tcsetattr`, `cfmakeraw`,
serta `cfgetispeed`/`cfsetispeed`/`cfgetospeed`/`cfsetospeed` tersedia.

Line discipline console (`fs/devfs.rs`) menghormati `ICANON` dan `ECHO`:

- `ICANON` aktif (default): input diedit per baris (`VERASE`/`VEOF`), baru bisa
  dibaca setelah newline.
- `ICANON` mati (mis. setelah `cfmakeraw`): setiap karakter langsung masuk buffer
  siap baca sebagai byte UTF-8 tanpa editing, termasuk karakter kontrol seperti
  `0x04`; `poll`/`read` melihatnya seketika.
- `ECHO` mengatur apakah karakter yang masuk dicetak kembali, di kedua mode.
- Saat `TCSETS`/`TCSETSW`/`TCSETSF` mematikan `ICANON`, baris canonical yang belum
  selesai langsung dipindahkan ke buffer baca (setelah data yang belum dibaca),
  jadi `read()` tidak menggantung menunggu newline yang tidak akan datang.
  `TCSETSF` (`TCSAFLUSH`) membuang semua input yang belum dibaca, termasuk baris
  parsial itu.

State termios masih global untuk semua terminal (bukan per `/dev/ttyN`), dan
`VMIN`/`VTIME` belum diterapkan: `read` mengembalikan byte yang tersedia begitu
ada. `TIOCGWINSZ` mengikuti ukuran framebuffer (80×25 bila tidak ada framebuffer).

## Threading di libc

`pthread_mutex_t` memakai futex: `lock` melakukan CAS 0 → 1, menandai state 2 saat
ada kontensi, dan tidur di `FUTEX_WAIT` sampai `unlock` membangunkan satu waiter.
`pthread_cond_t` menyimpan penghitung urutan: `pthread_cond_wait` membaca urutan,
melepas mutex, tidur di futex pada urutan tersebut, lalu mengambil mutex lagi;
`cond_signal`/`cond_broadcast` menaikkan urutan dan membangunkan satu/semua
waiter, sehingga sinyal yang datang sebelum waiter tidur tidak hilang.
`pthread_cond_timedwait` memakai `CLOCK_REALTIME` absolut dan mengembalikan
`ETIMEDOUT` saat deadline lewat. `sem_t` (di `<sys/semaphore.h>`) memakai
penghitung atomik dengan futex untuk `sem_wait`/`sem_timedwait`; `sem_init`
menolak `pshared != 0` dengan `ENOSYS` karena belum ada shared memory antar
proses. Diuji di `/bin/syscall-compat-demo`: kontensi mutex 2 × 500 kenaikan,
signal/broadcast condition variable, serta timeout 50 ms untuk cond dan sem.

`set_thread_pointer` mengatur register `FS.base` untuk thread saat ini. Kernel
menyimpan dan memulihkan nilainya ketika berpindah proses. Nilai harus nol atau
berada pada rentang alamat user canonical. Ini merupakan fondasi ABI TLS; syscall
`get_thread_pointer` membaca kembali nilainya. Syscall ini sendiri tidak membuat
thread atau menyediakan `pthread`; syscall 61–64 menyediakan primitive kernel
untuk thread user.

Thread yang dibuat dengan `thread_create` berbagi seluruh address space user,
termasuk heap, data, dan pemetaan, tetapi mendapat kernel stack, register/FPU
state, dan `FS.base` sendiri. Entry point harus berada pada halaman user yang
executable. Stack dan TLS harus berada di pemetaan anonim milik pemanggil;
kernel melepas pemetaan tersebut saat thread keluar. `thread_join` hanya dapat
menunggu thread dalam proses yang sama dan hanya satu kali. `thread_exit` membawa
nilai pointer hasil yang disimpan oleh `thread_join`. `getpid` tetap mengembalikan
ID proses; `thread_self` mengembalikan ID thread.

`oct_udp_socket`, `oct_udp_bind`, `oct_udp_sendto`, dan `oct_udp_recvfrom`
menyediakan datagram IPv4 UDP sederhana. Alamat tujuan pada `oct_udp_sendto`
memakai nilai `OCT_IPV4(a,b,c,d)`. Bind port 0 memilih port ephemeral dan
mengembalikan port yang dipakai. `recvfrom` menyalin alamat sumber sebagai empat
oktet dan port sumber sebagai `uint16_t` host-order. Antrean dibatasi delapan
datagram per socket; `-EAGAIN` berarti belum ada data, atau pengiriman menunggu
resolusi ARP. Datagram yang datang saat antrean penuh dibuang. Ulangi operasi
setelah `oct_yield()` agar loop kernel dapat memproses event USB tertunda. Jika
tidak ada task user lain yang siap, `oct_yield()` juga menunggu interrupt
berikutnya sebelum melanjutkan, sehingga retry I/O tidak terus-menerus memutar
CPU.

API C `<sys/socket.h>` dan `<netinet/in.h>` menyediakan IPv4 TCP `SOCK_STREAM`:
`socket`, `bind`, `listen`, `accept`, `connect`, `send`, `recv`, `shutdown`,
`getsockname`, dan `getpeername`. `sockaddr_in.sin_port` memakai byte order
jaringan (`htons`/`ntohs`), sedangkan `sin_addr.s_addr` dapat diisi dengan
`htonl`. `socket` saat ini hanya menerima `AF_INET`, `SOCK_STREAM`, dan protocol
0 atau `IPPROTO_TCP`; datagram POSIX, IPv6, socket options, dan mode nonblocking
belum didukung. Operasi `accept`, `connect`, `send`, dan `recv` yang blocking
mengulang `-EAGAIN` setelah `oct_yield`; `MSG_DONTWAIT` pada `send`/`recv`
mengembalikan `EAGAIN`. `MSG_PEEK` dan `MSG_WAITALL` tersedia terbatas pada
`recv`. TCP memakai pipeline sampai 4 segmen dengan congestion window 1→4 MSS,
buffer terima 8192 byte, dan MSS hingga 1460 byte; ini masih fondasi, belum
implementasi TCP penuh.
`oct_dns_server()` mengambil alamat DNS yang diterima dari DHCP; Rust runtime
menyediakan `dns_server()` dan resolver IPv4 A-record `dns_lookup()`.

`open` menerima mode akses `OCT_O_RDONLY=0`, `OCT_O_WRONLY=1`,
`OCT_O_RDWR=2` dan flag `OCT_O_CREAT`, `OCT_O_TRUNC`, `OCT_O_APPEND` dari
header. Argumen `mode` hanya dipakai saat membuat file dan dikurangi dengan
umask proses; nilai awal umask adalah `022`. `mkdir` juga menerima mode dan
menerapkan umask. Jika filesystem tidak mendukung permission per-file, mode saat
pembuatan diabaikan dan file dibuat dengan permission default filesystem;
operasi `chmod` eksplisit tetap melaporkan `ENOSYS`. Wrapper `oct_open()` memakai
`0644`, sedangkan `open()` C memakai argumen mode variadik saat `O_CREAT`
diberikan. Mode create/truncate memerlukan akses tulis. Path boleh absolute atau relatif terhadap cwd proses, harus
NUL-terminated, dan maksimal 255 byte.

`lseek` memakai `OCT_SEEK_SET`, `OCT_SEEK_CUR`, atau `OCT_SEEK_END`.
`oct_stat_path` mengisi struct:

```c
struct oct_stat {
    uint64_t size;
    uint32_t mode;     /* node type bits plus Unix permission bits */
    uint32_t reserved;
};
```

`mode` menyertakan tipe node dan bit permission: file `0100000`, direktori
`0040000`, symlink `0120000`, character device `0020000`; bit permission
berada pada bagian bawah seperti `0644` atau `0755`.

Descriptor 0 membaca TTY keyboard dalam mode baris: input di-echo, Backspace
menghapus karakter, Enter menyelesaikan baris, dan Ctrl-D menghasilkan EOF.
Baris dibatasi 255 karakter; pembacaan menunggu input dan dapat mengembalikan
sebagian baris bila buffer pemanggil lebih kecil. Ada empat virtual terminal,
masing-masing menyimpan layar teks dan antrean input sendiri. Tekan Ctrl+Alt+F1
sampai Ctrl+Alt+F4 untuk berpindah console. Shell dan program yang dijalankan
dari console memakai TTY console tersebut. `/dev/tty0` sampai `/dev/tty3`
mengakses TTY tertentu; `/dev/tty` mengikuti console aktif. Descriptor 1 dan 2
menulis ke terminal framebuffer;
bytes harus UTF-8. Setiap penulisan ke TTY juga dicerminkan ke COM1 sehingga
boot headless dapat membaca output program. `/dev/tty` dan `/dev/console`
mengakses TTY yang sama,
sedangkan `/dev/stdin`, `/dev/stdout`, dan `/dev/stderr` menyediakan endpoint
terpisah sesuai arahnya. Path tersebut terdaftar pada devfs di `/dev`.

Path relatif di-resolve terhadap direktori kerja proses. `chdir` mengubah
direktori kerja proses saat ini; program yang dijalankan shell mewarisi
direktori kerja shell. Namespace VFS, mount, dan file tetap dibagi dengan
kernel dan proses lain, seperti Linux tanpa mount namespace. Descriptor
terbuka tetap berupa state per proses.
`getcwd` mengembalikan jumlah byte termasuk NUL; `-ERANGE` berarti buffer
terlalu kecil.
`chmod` menerima mode permission hingga `0777`; tipe file tidak dapat diubah.
`access` menerima `F_OK`, `R_OK`, `W_OK`, dan `X_OK`. Karena proses belum punya
UID/GID, pemeriksaan permission saat ini memakai bit owner pada mode file.
`fstat` mendukung descriptor file dan direktori; pipe dan console mengembalikan
`ENOTSUP`.
`rename` mengganti file tujuan jika ada dan dapat memindahkan subtree direktori
di filesystem RAM bawaan. Filesystem lain dan pemindahan lintas mount belum
mendukung rename dan mengembalikan `ENOSYS`.
`lseek` pada perangkat TTY mengembalikan `-ESPIPE`.
`opendir` menyimpan snapshot entri direktori pada descriptor baru. `readdir`
mengembalikan satu entri per pemanggilan ke struct ABI `dirent`; tutup
descriptor direktori dengan syscall `close`. Wrapper C menyediakan
`opendir`/`readdir`/`closedir` melalui `<dirent.h>`. Nama entri dibatasi 255
byte; entri `.` dan `..` tidak disertakan.

Header `<sys/mman.h>` menyediakan API `mmap`, `munmap`, `mprotect`, `msync`,
`mincore`, `mlock`/`munlock`, `mlockall`/`munlockall`, dan `posix_madvise`.
`mmap` mendukung `MAP_PRIVATE` dan `MAP_SHARED`, pemetaan anonim
(`MAP_ANONYMOUS`/`MAP_ANON`) serta file-backed, offset yang page-aligned, dan
`MAP_FIXED`. Tanpa `MAP_FIXED`, alamat diberikan sebagai hint; alamat 0 memilih
ruang mulai dari 4 GiB. Pemetaan anonim harus menggunakan `fd=-1` dan offset 0.
Pemetaan file memerlukan descriptor file yang dapat dibaca; `MAP_SHARED` yang
dapat ditulis juga memerlukan descriptor writable. File-backed `MAP_SHARED`
menggunakan halaman cache bersama antar-pemetaan, `msync(MS_SYNC)` menulisnya
kembali, dan `MAP_PRIVATE` menggunakan salinan privat.
`MAP_PRIVATE` saat ini disalin saat pemetaan, bukan copy-on-write. OS belum
memiliki signal `SIGBUS`; akses ke halaman file-backed setelah EOF menghasilkan
halaman nol, bukan fault POSIX.

Ukuran pemetaan dibulatkan ke atas ke halaman 4096-byte. `munmap` menerima
rentang page-aligned dan membebaskan halaman yang dipetakan; rentang parsial
diperbolehkan. `mprotect` mengubah izin, `mincore` melaporkan semua halaman
terpetakan sebagai resident karena OS belum melakukan swapping, dan operasi
`mlock` sukses karena halaman tidak dapat di-swap. `posix_madvise` memvalidasi
rentang dan advice tetapi hint-nya tidak mengubah kebijakan paging.

`MAP_FIXED` hanya boleh mengganti pemetaan mmap OCTAGRAM; pemetaan ELF, stack,
dan area kernel tidak dapat ditimpa. Ruang alamat user juga dibatasi di bawah
stack. Jika file di-unlink saat masih dipetakan, pemetaan tetap hidup, tetapi
write-back ke inode yang sudah tidak tersedia tidak didukung. ABI register
syscall OCTAGRAM tetap berbeda dari Linux ABI.
`pipe` membuat kanal byte satu arah berkapasitas 4096 byte. Pembacaan saat
kosong dan penulisan saat penuh memblokir proses sampai ada data atau ruang;
jika semua writer ditutup, pembacaan mengembalikan EOF setelah buffer habis.
Penulisan tanpa reader menghasilkan `-EPIPE`. Operasi boleh mengembalikan
sebagian byte. Wrapper C: `oct_pipe(fds)`.
`dup2` menutup descriptor tujuan bila terbuka, lalu menduplikasi sumber ke nomor
tujuan; menggunakan descriptor yang sama tidak mengubahnya. Wrapper C:
`oct_dup2(source, target)`. `dup` memilih descriptor terendah yang tersedia.
Duplikat berbagi offset dan status flag open-file (`O_APPEND`). `fcntl` saat ini
mendukung `F_DUPFD`, `F_GETFD`, `F_SETFD`, `F_GETFL`, dan `F_SETFL`; `F_SETFL`
hanya dapat mengubah `O_APPEND`. `FD_CLOEXEC` didukung dan descriptor tersebut
tidak diwariskan oleh `spawn`.
`umask(mask)` menetapkan mask permission proses dan mengembalikan nilai lama.
Contoh uji seluruh API file tersedia di `/bin/file-api-demo` pada image OS.
`unlink` hanya menghapus file atau symlink; `rmdir` hanya menghapus direktori
kosong. API symlink memakai target relatif terhadap direktori link saat target
diikuti. `readlink` tidak mengikuti symlink terakhir dan menyalin maksimal
`size` byte tanpa menambahkan NUL, sesuai pola API POSIX. Demo tersedia sebagai
`/bin/path-api-demo`.
`stat` mengikuti symlink terakhir, sedangkan `lstat` melaporkan metadata
symlink itu sendiri. `fchmod` mengubah permission file atau direktori dari
descriptor terbuka; mode di luar `0777` ditolak dengan `EINVAL`. Permission
dapat diubah hanya pada filesystem yang mendukungnya.
Syscall `*at` menerima `AT_FDCWD` (`-100`) untuk menggunakan cwd atau descriptor
direktori terbuka untuk path relatif; path absolut mengabaikan descriptor.
`fstatat` hanya mendukung flag `AT_SYMLINK_NOFOLLOW`, sedangkan `unlinkat`
hanya mendukung `AT_REMOVEDIR`. API C menyediakan `dirfd()` untuk memperoleh
descriptor dari `DIR*`; selain itu, descriptor direktori berasal dari
`opendir()`.

`waitpid` menunggu anak tertentu dan menyimpan status keluar mentah sebagai
`uint64_t` jika pointer status bukan nol. Opsi yang didukung saat ini adalah
`WNOHANG`: hasilnya 0 bila anak belum selesai dan nilai status tidak diubah.
Opsi lain menghasilkan `EINVAL`. `waitpid` menerima `OCT_WAIT_ANY` untuk
menunggu anak langsung mana pun; syscall mengembalikan PID anak yang selesai,
juga ketika targetnya `OCT_WAIT_ANY`. Proses anak berbagi VFS dan filesystem
ter-mount dengan parent.

`execve` mengganti image proses saat ini tanpa mengubah PID, cwd, TTY, umask,
atau descriptor terbuka yang tidak memiliki `FD_CLOEXEC`. Descriptor dengan
flag itu ditutup dan flag pada descriptor yang diwariskan dibersihkan. `argv`
dan `envp` harus berupa array pointer yang diakhiri null. Jika gagal, image dan
state proses lama tetap aktif dan syscall mengembalikan `-errno`.

`fork` membuat proses baru dengan salinan independen seluruh halaman user.
Descriptor diwariskan, termasuk flag `FD_CLOEXEC`; file terbuka dan pipe tetap
berbagi open description/state seperti proses hasil `fork`. Batas proses
terjadwal saat ini adalah 16; kegagalan alokasi atau tercapainya batas
mengembalikan `-ENOMEM`. Gunakan `waitpid` untuk menunggu child.

`clock_gettime` mendukung `CLOCK_REALTIME` dari RTC CMOS (`tv_sec` detik Unix,
`tv_nsec` selalu 0 karena RTC beresolusi satu detik) dan clock monoton
(`CLOCK_MONOTONIC`, `CLOCK_MONOTONIC_RAW`, `CLOCK_BOOTTIME`,
`CLOCK_PROCESS_CPUTIME_ID`, `CLOCK_THREAD_CPUTIME_ID`) yang memakai TSC dan
dikalibrasi sekali terhadap PIT dengan resolusi 1 ms. `clock_getres` melaporkan
1 detik untuk realtime dan 1 ms untuk clock monoton. Nilai RTC diperlakukan sebagai UTC karena OS belum memiliki
konfigurasi zona waktu. API C juga menyediakan `time()`, `gmtime()`, dan
`localtime()`; saat ini `localtime()` sama dengan UTC.

## Argumen program

Saat ELF dimulai, stack mengikuti layout `argc`, pointer `argv`, terminator
`argv`, pointer `envp`, terminator `envp`, lalu pasangan tag/nilai auxiliary
vector yang diakhiri `AT_NULL`. Startup assembly meneruskan `argc`, `argv`, dan
`envp` di `rdx` ke `main`; global C `environ` juga menunjuk ke `envp`. Proses
anak mewarisi environment parent.

Maksimum 64 argumen termasuk `argv[0]` dengan total isi string maksimum 4096
byte. Environment dibatasi 64 entri dengan total isi string maksimum 4096 byte;
setiap entri harus berbentuk `NAME=value`, dan `NAME` hanya boleh berisi huruf
ASCII, angka, atau `_` (tidak diawali angka). Shell meneruskan variabel yang
diatur dengan `export NAME=value`; runtime C juga menyediakan `setenv` dan
`unsetenv`, dan environment hasil perubahan dipakai oleh proses anak. `PATH`
bawaan adalah
`/bin:/usr/bin:/usr/local/bin`.

`read`/`write` dibatasi 4096 byte per panggilan; alamat dan permission halaman
user divalidasi sebelum diakses. Wrapper C `read()` dan `write()` membatasi
permintaan yang lebih besar ke ukuran tersebut, sehingga satu panggilan dapat
mengembalikan lebih sedikit byte daripada jumlah yang diminta. Maksimum ada 16
descriptor per program, termasuk tiga descriptor console.

Error umum
meliputi `-2` (not found), `-9` (bad descriptor),
`-13` (permission denied), `-14` (bad pointer), `-22` (invalid argument),
`-28` (no space), `-29` (TTY tidak dapat di-seek), `-34` (buffer terlalu
kecil), dan `-38` (syscall tidak dikenal).

ABI ini sengaja bukan Linux ABI dan belum mencakup signal, IPv6, UDP POSIX,
`CLOCK_MONOTONIC`, atau dynamic linking penuh. Interpreter `PT_INTERP` awal
tersedia untuk executable PIE dan batas formatnya dijelaskan di
[`docs/porting-apps.md`](porting-apps.md). Setiap proses ELF memiliki address space privat dan dapat dipreempt ketika
berjalan di ring 3; thread dalam proses yang sama berbagi address space tersebut.
Batas total task terjadwal (proses dan thread) adalah 64. Penjadwalan memakai
prioritas nice dengan aging; lihat [`docs/process.md`](process.md#penjadwalan).
`read()` dari descriptor console dan dari tty yang dibuka lewat path
(`/dev/stdin`, `/dev/tty*`, `/dev/console`) memblokir secara scheduler-aware
(`ProcessWait::ConsoleInput`); jalur VFS memakai `poll_tty()` dan mengembalikan
`EAGAIN` ketika belum ada input. Menambahkan nomor syscall adalah
perubahan ABI; jaga nomor lama dan update header, tabel ini, implementasi, dan
contoh program secara bersamaan.
