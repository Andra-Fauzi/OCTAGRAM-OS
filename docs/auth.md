# Autentikasi, Kredensial, dan Keamanan

Dokumentasi ini menjelaskan model keamanan, identitas pengguna, kredensial proses,
kapabilitas, dan basis data autentikasi di OCTAGRAM-OS. Implementasi kernel berada di
`kernel/src/auth.rs`, `kernel/src/sha256.rs`, `kernel/src/elf.rs`, serta perintah shell
terkait di `kernel/src/shell.rs`.

---

## 1. Model Kredensial Pengguna

Setiap task dan proses di OCTAGRAM-OS melacak empat nilai identitas POSIX:

| Kredensial | Tipe | Keterangan |
| --- | --- | --- |
| `uid` | `u32` | User ID riil |
| `euid` | `u32` | User ID efektif (menentukan hak akses operasional) |
| `gid` | `u32` | Group ID riil |
| `egid` | `u32` | Group ID efektif |

### Pewarisan dan Batas Kredensial
- Saat boot, task awal berjalan dengan `uid = 0` (root), `gid = 0`, serta seluruh kapabilitas aktif (`CAP_ALL`).
- Saat `fork()`, proses anak mewarisi seluruh kredensial (`uid`, `euid`, `gid`, `egid`) dan kapabilitas induknya.
- Semua thread dalam satu proses (`pid` sama) berbagi kredensial yang sama.
- Saat `execve()`, kredensial dipertahankan kecuali bit setuid/setgid diterapkan.

### Syscall Kredensial
Syscall untuk manipulasi kredensial didefinisikan pada ABI nomor 140–143:
- `setuid(uid)` (Syscall 140)
- `seteuid(euid)` (Syscall 141)
- `setgid(gid)` (Syscall 142)
- `setegid(egid)` (Syscall 143)
- `getuid()` (Syscall 102), `geteuid()` (Syscall 107), `getgid()` (Syscall 104), `getegid()` (Syscall 108).

**Aturan Perubahan Kredensial (`credential_change_allowed`):**
- Proses dengan `euid == 0` (root) bebas mengubah `uid`/`euid`/`gid`/`egid` ke nilai berapapun.
- Proses non-root (`euid != 0`) hanya diizinkan mengubah ke ID riil atau ID efektif aslinya (misalnya kembali dari unprivileged ke privileged bila ID riil masih 0). Pelanggaran mengembalikan `-EPERM`.
- Menurunkan `uid`/`euid` dari root ke non-root secara otomatis mencabut seluruh kapabilitas proses (`capabilities_for_uid`).

---

## 2. Kapabilitas Proses (Capabilities)

OCTAGRAM-OS membagi hak istimewa superuser menjadi bitmask kapabilitas (`u32`).
Header C userspace tersedia di `<sys/capability.h>`:

| Kapabilitas | Bitmask | Deskripsi Penegakan |
| --- | --- | --- |
| `CAP_VFS_WRITE` | `1 << 0` | Wajib untuk memodifikasi filesystem: `open()` dengan mode tulis (`O_WRONLY`, `O_RDWR`, `O_CREAT`, `O_TRUNC`, `O_APPEND`), `write()` ke berkas reguler, `mkdir()`, `unlink()`, `rmdir()`, `unlinkat()`, `rename()`, `symlink()`, `chmod()`, dan `ftruncate()`. |
| `CAP_NETWORK` | `1 << 1` | Wajib untuk membuat soket jaringan (`socket()` domain `AF_INET`). |
| `CAP_SYS_ADMIN` | `1 << 2` | Hak administratif tingkat tinggi; wajib untuk mengubah kapabilitas via `capset()`. |
| `CAP_SCHED_NICE` | `1 << 3` | Wajib untuk menaikkan prioritas nice task lain atau menggunakan kebijakan penjadwalan real-time (`SCHED_FIFO` / `SCHED_RR`). |

### Syscall Kapabilitas
- `capget(pid, *caps)` (Syscall 144): Membaca bitmask kapabilitas proses target.
- `capset(pid, caps)` (Syscall 145): Menetapkan bitmask kapabilitas proses target. Membutuhkan `CAP_SYS_ADMIN`. Bit di luar `CAP_ALL` ditolak dengan `-EINVAL`.

Contoh implementasi pengujian kredensial dan kapabilitas terdapat pada program C `user/examples/credential_demo.c`.

---

## 3. Basis Data Akun

OCTAGRAM-OS menggunakan tiga berkas konfigurasi akun berbasis teks standar di `/etc`:

### `/etc/passwd`
Menyimpan informasi akun pengguna sesuai spesifikasi POSIX:
```text
root:x:0:0:root:/root:/bin/sh
octagram:x:1000:1000:octagram:/home/octagram:/bin/sh
```
Fungsi pustaka libc seperti `getpwnam()`, `getpwuid()`, `getpwent()`, dan varian reentrant `_r` membaca berkas ini (diimplementasikan di `user/runtime.c`).

### `/etc/group`
Menyimpan informasi grup:
```text
root:x:0:
octagram:x:1000:
```
Fungsi pustaka libc `getgrnam()`, `getgrgid()`, dan `getgrent()` membaca berkas ini dengan fallback grup utama dari `/etc/passwd`.

---

## 4. Basis Data Kata Sandi `/etc/shadow`

Basis data kata sandi disimpan di `/etc/shadow` (`SHADOW_PATH`) dan dikelola oleh kernel di `kernel/src/auth.rs`.

### Format Berkas
Setiap baris memiliki format:
```text
user:salt:hash
```
Contoh:
```text
# user:salt:sha256(salt+password)
octagram:a1b2c3d4e5f60718:8c6976e5b5410415bde908bd4dee15dfb167a9c873fc4bb8a81f6f2ab448a918
```

### Komponen:
1. **`user`**: Nama pengguna (misalnya `root` atau `octagram`).
2. **`salt`**: String heksadesimal 16 karakter (64-bit acak dari RNG pseudo-hardware kernel).
3. **`hash`**: Ringkasan SHA-256 sepanjang 64 karakter heksadesimal dari kombinasi `salt + password`:
   $$\text{hash} = \text{SHA256}(\text{salt} \mathbin{\Vert} \text{password})$$

### Parser Aman
Fungsi `auth::parse()` mengabaikan baris kosong, baris komentar `#`, entri dengan field kurang atau lebih, entri tanpa user/salt, serta entri yang panjang hash-nya bukan 64 karakter heksadesimal. Hal ini mencegah berkas shadow yang rusak mengunci seluruh akun sistem.

Fungsi `auth::upsert()` memperbarui hash jika user sudah ada, atau menambahkan entri baru jika belum ada.

---

## 5. Implementasi SHA-256

Algoritma hash SHA-256 diimplementasikan dari nol di `kernel/src/sha256.rs`:
- Sesuai standar FIPS PUB 180-4 dengan konstanta putaran $K$ (64 kata 32-bit) dan nilai awal $H$.
- Menangani blok 512-bit (64 byte) dengan penjadwalan pesan $W_{0..63}$.
- Fungsi kompresi memproses padding dengan tepat, termasuk pengujian vektor batas NIST pada panjang pesan 55 dan 56 byte (batas penyeberangan blok padding).
- Menyediakan fungsi `digest_bytes(&[u8]) -> [u8; 32]` dan `digest_hex(&[u8]) -> String`.

---

## 6. Perintah Shell Autentikasi dan Sesi

Shell interaktif (`kernel/src/shell.rs`) menyediakan perintah pengelolaan identitas dan sesi:

### `passwd [user]`
- Mengubah kata sandi untuk pengguna (`root` atau `octagram`).
- Hanya pengguna `root` yang diizinkan mengubah kata sandi pengguna lain; pengguna biasa hanya boleh mengubah kata sandinya sendiri.
- Meminta kata sandi baru dua kali tanpa menampilkan karakter di layar (`read_secret_line`).
- Menghasilkan salt acak baru, menghitung digest SHA-256, dan memperbarui `/etc/shadow` secara atomik melalui VFS.

### `login <user>`
- Berpindah sesi pengguna aktif pada virtual terminal yang bersangkutan.
- Memeriksa apakah berkas `/etc/shadow` ada dan memiliki entri untuk `user`:
  - Jika entri ada: meminta kata sandi secara rahasia dan memverifikasinya dengan `auth::verify()`. Jika salah, login ditolak dan sesi tidak berubah.
  - Jika berkas `/etc/shadow` belum dibuat atau user belum memiliki entri: login diizinkan langsung (kompatibilitas boot awal).
- Mengubah nama user terminal dan memperbarui prompt shell (misal `octagram@octagram-os$ ` atau `root@octagram-os# `).

### `whoami`
- Menampilkan nama pengguna yang sedang aktif di terminal saat ini (`current_user`).

### `session`
- Menampilkan rincian sesi konsol aktif, misalnya:
  ```text
  session user=octagram tty=tty0
  ```

---

## 7. Batasan dan Rencana Ke Depan

1. **KDF Berulang**: Saat ini hash kata sandi masih berupa satu putaran $\text{SHA256}(\text{salt} + \text{password})$. Belum menggunakan fungsi derivasi kunci yang diperkuat seperti PBKDF2, bcrypt, scrypt, atau Argon2.
2. **Kebijakan Kedaluwarsa**: Field shadow standar POSIX seperti waktu kedaluwarsa akun, masa berlaku kata sandi, dan peringatan belum diimplementasikan.
3. **Pemisahan Hak Sesi Shell**: Saat ini shell berjalan sebagai task kernel kooperatif; perpindahan user di shell mengubah state variabel sesi konsol dan kredensial eksekusi program anak (`execve`), namun belum mengisolasi memori internal shell itu sendiri.
4. **Cakupan Penegakan Kapabilitas**: Penegakan `CAP_VFS_WRITE` dan `CAP_SYS_ADMIN` mencakup `open`, `write`, `ioctl`, dan `mmap` untuk perangkat raw (`/dev/fb0`), isolasi tty konsol antar terminal (`/dev/tty0..3`), serta penolakan tulis descriptor yang sudah dibuka sebelum hak istimewa diturunkan. Operasi mount/umount saat ini masih berada di level perintah shell.
