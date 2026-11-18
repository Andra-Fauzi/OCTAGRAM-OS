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
- Redirection `>` dan `>>` didukung untuk perintah yang menulis berkas.
- `export NAMA=nilai` menyimpan variabel lingkungan shell; program menerimanya
  sebagai environment saat `execve`.

## Perintah bawaan

**Navigasi dan teks**
`help`, `clear`, `pwd`, `path`, `cd`, `echo`, `export`, `ls`, `cat`, `head`,
`tail`, `wc`, `grep`, `touch`, `mkdir`, `ln`, `rm`, `chmod`, `stat`, `cp`,
`edit`.

**Sistem**
`date`, `shutdown`, `sched`, `devices`, `lsdev`, `drivers`, `savelog`.

`sched` menampilkan PID/TID, nice, policy, umur tunggu (`AGE`), dan status tiap
task terjadwal. Penanda `WATCHDOG` muncul bila umur tunggu proses runnable
melewati ambang anti-starvasi (78 putaran aging).

**Jaringan**
`net` (`net status`, `net dhcp`, `net wifi ...`), `ping`, `dns`, `http`/`curl`.

**Storage**
`mount`, `umount`, `df`, `persist`.

**Paket dan sesi**
`apps`, `pkg add|update|remove|list|info|verify`, `login`, `whoami`, `session`,
`passwd` (belum diimplementasikan, mencetak pesan bahwa autentikasi password
tidak tersedia).

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
- Tanpa pipe antar perintah, tanpa job control, tanpa background job, tanpa
  globbing, dan tanpa `~` expansion.
- Tanpa `man`; bantuan ada di `help` dan folder `docs/`.

## PATH dan program

`PATH` awal menunjuk ke `/bin` dan dapat diubah dengan `export`. Program ELF
harus statis atau memakai `ld-octagram.so` yang tersedia; lihat
[userspace.md](userspace.md) dan [dynamic-linker.md](dynamic-linker.md).

## Paket aplikasi

`pkg` membaca metadata paket dari `/var/lib/octagram/packages/*.pkg` dan registry
`registry.txt`, lalu menyalin/menghapus berkas aplikasi. `apps` membaca direktori
`/apps` untuk daftar aplikasi yang terpasang. Format dan batasannya dijelaskan
di [filesystem.md](filesystem.md) dan [limitations.md](limitations.md).

## Menambah perintah

1. Tambahkan cabang pada `match` perintah di `shell.rs`.
2. Letakkan implementasi di fungsi terpisah agar tidak memperbesar fungsi utama.
3. Gunakan helper VFS yang ada (`resolve_user_path`, `read_file`, ...) agar
   batas ukuran dan normalisasi path konsisten.
4. Perbarui `print_help()` dan dokumentasi ini.
5. Tambahkan uji host untuk logika parsing/format bila memungkinkan.
