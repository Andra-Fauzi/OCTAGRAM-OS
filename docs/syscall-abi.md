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
| 10 | `brk` | `RDI=new_break`, 0 untuk query | Break baru |
| 11 | `getpid` | — | PID proses saat ini |
| 12 | `isatty` | `RDI=fd` | 1 untuk console |
| 13 | `chdir` | `RDI=path` | 0 |
| 14 | `getcwd` | `RDI=buffer`, `RSI=size` | Byte path termasuk NUL |
| 15 | `putpixel` | `RDI=x`, `RSI=y`, `RDX=0xRRGGBB` | 0 |
| 16 | `mmap` | `RDI=address`, `RSI=length`, `RDX=protection` | Alamat awal atau `-errno` |
| 17 | `munmap` | `RDI=address`, `RSI=length` | 0 |
| 18 | `mmap_file` | `RDI=fd`, `RSI=offset`, `RDX=length` | Alamat awal atau `-errno` |
| 19 | `spawn` | `RDI=path`, `RSI=argv`, `RDX=argc` | PID anak |
| 20 | `waitpid` | `RDI=pid` atau `OCT_WAIT_ANY`, `RSI=status*` (0 untuk abaikan) | PID anak yang selesai |
| 21 | `pipe` | `RDI=int fds[2]` | 0; `fds[0]` read, `fds[1]` write |
| 22 | `dup2` | `RDI=source_fd`, `RSI=target_fd` | `target_fd` |
| 23 | `set_environ` | `RDI=char *const envp[]` | 0 |
| 24 | `spawn_env` | `RDI=path`, `RSI=argv`, `RDX=argc`, `R10=envp` | PID anak |
| 25 | `chmod` | `RDI=path`, `RSI=mode` | 0 |
| 26 | `printf` | `RDI=format`, argumen variadic mengikuti ABI x86-64 System V | Jumlah byte yang ditulis |
| 27 | `opendir` | `RDI=path` | Descriptor direktori |
| 28 | `readdir` | `RDI=fd`, `RSI=struct dirent*` | 1 untuk entri, 0 untuk EOF |

`open` menerima mode akses `OCT_O_RDONLY=0`, `OCT_O_WRONLY=1`,
`OCT_O_RDWR=2` dan flag `OCT_O_CREAT`, `OCT_O_TRUNC`, `OCT_O_APPEND` dari
header. Argumen `mode` hanya dipakai saat membuat file dan diterapkan langsung
tanpa umask; wrapper `oct_open()` memakai `0644`, sedangkan `open()` C memakai
argumen mode variadik saat `O_CREAT` diberikan. Mode create/truncate memerlukan
akses tulis. Path boleh absolute atau relatif terhadap cwd proses, harus
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
`lseek` pada perangkat TTY mengembalikan `-ESPIPE`.
`opendir` menyimpan snapshot entri direktori pada descriptor baru. `readdir`
mengembalikan satu entri per pemanggilan ke struct ABI `dirent`; tutup
descriptor direktori dengan syscall `close`. Wrapper C menyediakan
`opendir`/`readdir`/`closedir` melalui `<dirent.h>`. Nama entri dibatasi 255
byte; entri `.` dan `..` tidak disertakan.

`putpixel` menulis satu pixel framebuffer dengan warna packed `0xRRGGBB`.
Koordinat harus berada dalam ukuran framebuffer dan warna maksimal `0xFFFFFF`;
argumen di luar batas menghasilkan `-EINVAL`, dan framebuffer yang tidak
tersedia menghasilkan `-ENODEV`. Syscall ini menggambar langsung ke layar yang
juga dipakai terminal teks, sehingga teks berikutnya dapat menimpa pixel.
Wrapper C tersedia sebagai `oct_putpixel(x, y, rgb)`.

`mmap` membuat pemetaan anonim berisi nol. `address=0` memilih alamat berikutnya
mulai dari 4 GiB; alamat nonnol meminta alamat tepat dan harus page-aligned.
Panjang dibulatkan ke atas ke kelipatan 4096. `protection` harus menyertakan
`OCT_PROT_READ`; `OCT_PROT_WRITE` dan `OCT_PROT_EXEC` boleh dipakai sendiri,
tetapi tidak bersama-sama. `munmap` menerima alamat page-aligned dan panjang
positif, dan hanya dapat melepas halaman yang dibuat oleh `mmap`; rentang parsial
diperbolehkan bila seluruh halamannya memang hasil pemetaan tersebut. Keduanya
mengembalikan `-EINVAL` untuk rentang/izin yang tidak valid dan `-ENOMEM` bila
frame atau page table tidak tersedia. Ini ABI khusus OCTAGRAM, bukan implementasi
semantik Linux `mmap`/`munmap`. Alamat otomatis bertambah monoton; `munmap`
membebaskan frame, dan pemakaian ulang alamat dapat diminta dengan alamat eksplisit.
`mmap_file` membuat snapshot read-only dari descriptor file yang bisa dibaca.
Offset harus page-aligned dan rentang harus seluruhnya berada di dalam file.
Data disalin secara eager ke halaman anonim; perubahan file tidak mengubah
mapping, dan penulisan kembali ke file tidak didukung. Wrapper C:
`oct_mmap_file(fd, offset, length)`.

`pipe` membuat kanal byte satu arah berkapasitas 4096 byte. Pembacaan saat
kosong dan penulisan saat penuh memblokir proses sampai ada data atau ruang;
jika semua writer ditutup, pembacaan mengembalikan EOF setelah buffer habis.
Penulisan tanpa reader menghasilkan `-EPIPE`. Operasi boleh mengembalikan
sebagian byte. Wrapper C: `oct_pipe(fds)`.
`dup2` menutup descriptor tujuan bila terbuka, lalu menduplikasi sumber ke nomor
tujuan; menggunakan descriptor yang sama tidak mengubahnya. Wrapper C:
`oct_dup2(source, target)`.

`set_environ` memvalidasi dan menyalin environment saat ini ke state proses
kernel, sehingga perubahan `setenv`/`unsetenv` pada runtime C diwariskan oleh
panggilan `spawn` berikutnya. Array harus diakhiri pointer null; setiap entri
berbentuk `NAME=value`, dengan batas yang sama seperti environment saat ELF
dimuat.

`spawn` menjalankan ELF statis sebagai proses anak, mewarisi cwd, TTY, dan semua
descriptor terbuka, serta menerima `argv` sebagai array pointer NUL-terminated.
Pipe yang diwariskan berbagi buffer antarproses. Child perlu menutup ujung pipe
yang tidak dipakai agar EOF dan `EPIPE` terdeteksi dengan benar. `waitpid`
menunggu anak tertentu dan menyimpan status keluar mentah sebagai `uint64_t`
jika pointer status bukan nol. `run` memberi PID 1,
`runp` memakai PID berurutan, dan proses anak mendapat PID unik dalam grup.
Proses anak berbagi VFS dan filesystem ter-mount dengan parent. `waitpid`
menerima `OCT_WAIT_ANY` untuk menunggu anak langsung mana pun; syscall
mengembalikan PID anak yang selesai, juga ketika targetnya `OCT_WAIT_ANY`.
Wrapper C: `oct_spawn(path, argv, argc)` dan `oct_waitpid(pid, &status)`.
`spawn_env` memiliki perilaku yang sama, tetapi menyalin environment yang
diberikan di `R10`; syscall ini dipakai `posix_spawn` agar environment khusus
child tidak mengubah environment parent.

## Argumen program

Saat ELF dimulai, stack mengikuti layout `argc`, pointer `argv`, terminator
`argv`, pointer `envp`, dan terminator `envp`. Startup assembly meneruskan
`argc`, `argv`, dan `envp` di `rdx` ke `main`; global C `environ` juga menunjuk
ke `envp`. Proses anak mewarisi environment parent.

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

Syscall `printf` adalah layanan terbatas untuk program Chibicc: format berada di
`RDI`, lima argumen variadic pertama di `RSI`, `RDX`, `RCX`, `R8`, dan `R9`,
serta argumen berikutnya di stack sesuai ABI x86-64 System V. Format yang
didukung adalah `%s`, `%c`, `%d`, `%i`, `%u`, `%o`, `%x`, `%X`, `%p`, dan `%%`,
dengan flag, width, dan precision dasar. Output dibatasi 4096 byte dan dikirim
ke descriptor stdout. Ini bukan implementasi stdio atau libc umum; shell
menyediakan stub `printf` untuk jalur kompilasi Chibicc.

`brk` dengan alamat 0 membaca break saat ini. Break dapat bergerak dari
`0x10000000` sampai batas heap image (segmen ELF berikutnya atau 4 GiB).
Halaman tambahan dipetakan saat heap tumbuh dan dilepas saat menyusut; 1 MiB
pertama tetap dipetakan. Ini bukan allocator; runtime atau aplikasi harus
mengelola alokasi sendiri. Error umum
meliputi `-2` (not found), `-9` (bad descriptor),
`-13` (permission denied), `-14` (bad pointer), `-22` (invalid argument),
`-28` (no space), `-29` (TTY tidak dapat di-seek), `-34` (buffer terlalu
kecil), dan `-38` (syscall tidak dikenal).

ABI ini sengaja bukan Linux ABI dan belum mencakup `fork`, signal, socket,
clock, atau dynamic linking. ELF memiliki address space privat
dan dapat dipreempt ketika berjalan di ring 3. Pembacaan TTY yang menunggu
belum memblokir proses secara scheduler-aware sehingga dapat menunda proses lain.
Menambahkan nomor syscall adalah perubahan ABI; jaga nomor lama dan update
header, tabel ini, implementasi, dan contoh program secara bersamaan.
