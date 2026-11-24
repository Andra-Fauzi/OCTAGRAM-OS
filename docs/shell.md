# Shell

Shell interaktif ada di `kernel/src/shell.rs` dan berjalan sebagai task kernel
kooperatif, satu per virtual console. Setelah boot, empat shell (`tty0`..`tty3`)
aktif dan dapat dipindah dengan Ctrl+Alt+F1..F4.

## Model eksekusi

- Shell membaca baris dari TTY-nya, memecah argumen, lalu menjalankan perintah
  bawaan atau program ELF di `PATH`.
- Perintah bawaan dijalankan langsung di task shell; program user dijalankan
  lewat `run` (satu program, menunggu selesai) atau `runp` (beberapa program,
  round-robin).
- Pipeline `a | b | c` dan redirection `>`/`>>` didukung untuk perintah teks
  bawaan (`cat`, `head`, `tail`, `wc`, `grep`, `echo`); stage terakhir boleh
  menulis ke berkas. Operator di dalam tanda kutip tidak dianggap operator.
- `mv` memakai jalur VFS `rename`: pada EXT2 berkas dan symlink dapat dipindah
  antar direktori, direktori hanya dapat dipindah dalam parent yang sama, dan
  nama tujuan yang sudah ada ditolak (tidak menimpa).
- `export NAMA=nilai` menyimpan variabel lingkungan shell; program menerimanya
  sebagai environment saat `execve`.

## Perintah bawaan

**Navigasi dan teks**
`help`, `clear`, `pwd`, `path`, `cd`, `echo`, `export`, `ls`, `cat`, `head`,
`tail`, `wc`, `grep`, `touch`, `mkdir`, `ln`, `rm`, `chmod`, `stat`, `cp`,
`mv`, `edit`.

**Sistem**
`date`, `shutdown`, `reboot`, `sched`, `devices`, `lsdev`, `drivers`, `savelog`.
`shutdown` meminta S5 lewat PM1 control; `reboot` memakai register reset FADT
(`RESET_REG`/`RESET_VALUE`) dengan fallback pulsa 8042 lalu triple fault (detail di [acpi-power.md](acpi-power.md)).

`savelog <path>` mengambil snapshot buffer log kernel in-memory dan menyimpannya ke berkas VFS (detail di [logging.md](logging.md)).

`sched` menampilkan PID/TID, nice, policy, umur tunggu (`AGE`), dan status tiap
task terjadwal. Penanda `WATCHDOG` muncul bila umur tunggu proses runnable
melewati ambang anti-starvasi (78 putaran aging atau wait >= 500 ms).

**Jaringan**
`net` (`net status`, `net dhcp`, `net wifi ...`), `ping`, `dns`, `http`/`curl`.

**Storage**
`mount`, `umount`, `df`, `persist`.

**Paket dan sesi**
`apps`, `pkg add|update|remove|list|info|verify`, `login`, `whoami`, `session`,
`passwd [user]`. Detail autentikasi dan basis data `/etc/shadow` ada di [auth.md](auth.md); manajemen paket dan direktori `/apps` ada di [packages.md](packages.md).

`passwd` meminta kata sandi baru dua kali tanpa menampilkan karakter, lalu
menulis `/etc/shadow` dalam bentuk `user:salt:sha256(salt+password)` (SHA-256 di
`kernel/src/sha256.rs`, salt 16 heksadesimal dari RNG kernel). Hanya root yang
boleh mengganti kata sandi pengguna lain. Setelah entri ada, `login <user>`
meminta kata sandi dan menolak sesi baru bila salah; bila `/etc/shadow` belum
ada atau tidak memuat entri untuk user tersebut, `login` berjalan tanpa
verifikasi seperti sebelumnya.

**Toolchain dan eksekusi**
`as`, `ld`, `cc`, `tcc`, `run`, `runp`.

Program lain dapat dijalankan dengan menyebut namanya jika ada di `PATH`
(direktori program biasanya `/bin`).

## Batasan perintah teks

- Operasi berkas shell dibatasi `MAX_FILE_OPERATION` = 64 KiB per operasi.
- `head`/`tail` menampilkan maksimum sepuluh baris secara default.
- `wc` menghitung newline, kata, dan byte; `grep` mencari teks literal dan
  mendukung `-n` untuk nomor baris.
- `edit` adalah editor layar sederhana berbasis terminal, bukan `vi`.
- Pipeline hanya berlaku untuk perintah teks bawaan; program ELF tidak dapat
  dirangkai dengan `|`, dan input redirection `<` belum tersedia.
- Redirection stage terakhir hanya pada lintasan pipeline; perintah di luar
  daftar teks ditolak dengan pesan kesalahan, bukan diabaikan.
- Tanpa job control, background job, globbing, dan `~` expansion.
- `edit` memuat seluruh berkas ke memori dengan batas `MAX_EDITOR_FILE` = 256
  KiB dan menyediakan pencarian literal, bukan regex.
- Tanpa `man`; bantuan ada di `help` dan folder `docs/`.

## Editor terminal

`edit <path>` membuka editor layar penuh; berkas dibuat bila belum ada.

| Tombol | Fungsi |
| --- | --- |
| Ctrl+S | simpan |
| Ctrl+Q | keluar (dua kali bila ada perubahan yang belum disimpan) |
| Ctrl+F | mulai pencarian literal; Enter mencari dari kursor, Esc membatalkan |
| Ctrl+G | cari kemunculan berikutnya dari pencarian terakhir |
| Tab | sisipkan tab |
| Panah | navigasi kursor; viewport ikut bergulir |

Pencarian membungkus ke awal berkas dan status di baris bawah menampilkan
`Found:` atau `Not found:`. Berkas yang bukan UTF-8 valid ditolak.

## PATH dan program

`PATH` awal adalah `/bin`, `/usr/bin`, `/usr/local/bin`, lalu
`/toolchain/bin`; nilainya dapat diubah dengan `export` atau diperiksa dengan
`path`. Direktori terakhir adalah tempat penempatan toolchain native (misalnya
`rustc`, `cargo`, dan linker) sehingga binary compiler dapat dipanggil dengan
namanya saja; direktori itu sengaja dicari paling akhir agar tidak menutupi
program bawaan image. Program ELF harus statis atau memakai `ld-octagram.so`
yang tersedia; lihat
[userspace.md](userspace.md) dan [dynamic-linker.md](dynamic-linker.md).

## Paket aplikasi

`pkg` membaca metadata paket dari `/var/lib/octagram/packages/*.pkg` dan registry
`registry.txt`, lalu menyalin/menghapus berkas aplikasi. `apps` membaca direktori
`/apps` untuk daftar aplikasi yang terpasang. Format paket, perintah `pkg`, dan
batasannya dijelaskan lengkap di [packages.md](packages.md).

## Menambah perintah

1. Tambahkan cabang pada `match` perintah di `shell.rs`.
2. Letakkan implementasi di fungsi terpisah agar tidak memperbesar fungsi utama.
3. Gunakan helper VFS yang ada (`resolve_user_path`, `read_file`, ...) agar
   batas ukuran dan normalisasi path konsisten.
4. Perbarui `print_help()` dan dokumentasi ini.
5. Tambahkan uji host untuk logika parsing/format bila memungkinkan.
