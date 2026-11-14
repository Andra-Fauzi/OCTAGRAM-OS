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

Nomor 10, 15, 18, 19, 23, 24, 26, dan 49 sudah dicabut dan tetap
dicadangkan agar nomor syscall berikutnya tidak bergeser; pemanggilannya
mengembalikan `-ENOSYS`. API POSIX `posix_spawn` disediakan runtime melalui
`fork` dan `execve`, bukan syscall khusus.

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

`open` menerima mode akses `OCT_O_RDONLY=0`, `OCT_O_WRONLY=1`,
`OCT_O_RDWR=2` dan flag `OCT_O_CREAT`, `OCT_O_TRUNC`, `OCT_O_APPEND` dari
header. Argumen `mode` hanya dipakai saat membuat file dan dikurangi dengan
umask proses; nilai awal umask adalah `022`. `mkdir` juga menerima mode dan
menerapkan umask. Wrapper `oct_open()` memakai `0644`, sedangkan `open()` C
memakai argumen mode variadik saat `O_CREAT` diberikan. Mode create/truncate
memerlukan akses tulis. Path boleh absolute atau relatif terhadap cwd proses, harus
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
bytes harus UTF-8. `/dev/tty` dan `/dev/console` mengakses TTY yang sama,
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

`clock_gettime` saat ini mendukung `CLOCK_REALTIME` dari RTC CMOS; `tv_sec`
berisi detik Unix dan `tv_nsec` selalu 0 karena RTC hanya menyediakan resolusi
satu detik. Nilai RTC diperlakukan sebagai UTC karena OS belum memiliki
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

ABI ini sengaja bukan Linux ABI dan belum mencakup signal, socket,
`CLOCK_MONOTONIC`, atau dynamic linking penuh. Interpreter `PT_INTERP` awal
tersedia untuk executable PIE dan batas formatnya dijelaskan di
[`docs/porting-apps.md`](porting-apps.md). Setiap proses ELF memiliki address space privat dan dapat dipreempt ketika
berjalan di ring 3; thread dalam proses yang sama berbagi address space tersebut.
Batas total task terjadwal (proses dan thread) adalah 16. Pembacaan TTY yang
menunggu belum memblokir proses secara scheduler-aware sehingga dapat menunda
proses lain. Menambahkan nomor syscall adalah perubahan ABI; jaga nomor lama dan
update header, tabel ini, implementasi, dan contoh program secara bersamaan.
