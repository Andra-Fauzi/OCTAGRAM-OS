# Roadmap Self-Hosting OCTAGRAM-OS

> Dokumen utama perencanaan menuju OCTAGRAM-OS yang dapat membangun,
> menguji, mengemas, memasang, dan membangun ulang dirinya sendiri tanpa
> bergantung pada Linux/Windows dalam workflow pengembangan normal.

- **Branch implementasi:** `feature/native-self-hosting`
- **Arsitektur target awal:** x86_64
- **Filesystem kerja awal:** EXT2 persisten
- **Strategi bootstrap:** host sebagai Stage 0; OCTAGRAM sebagai Stage 1 dan seterusnya
- **Status keseluruhan:** belum self-hosting; fondasi userspace dan toolchain sedang dibangun
- **Dokumen teknis pendahulu:** [native-self-hosting-roadmap.md](native-self-hosting-roadmap.md)

---

## 1. Visi akhir

OCTAGRAM-OS harus bisa menjalankan siklus pengembangan penuh dari dalam OS itu
sendiri:

1. Membuka dan mengedit source code pada filesystem persisten.
2. Mengompilasi source C dan Rust menggunakan compiler yang berjalan native.
3. Menjalankan assembler, linker, build system, dan test runner native.
4. Membangun kernel, userspace, library, dan artefak boot.
5. Membuat image disk/EFI yang dapat di-boot.
6. Memasang image ke disk atau slot yang tidak sedang digunakan.
7. Memverifikasi hasil instalasi, mengatur boot berikutnya, lalu reboot.
8. Jika build baru gagal boot, kembali ke sistem terakhir yang diketahui baik.
9. Mengulang proses tersebut untuk membangun ulang toolchain dan OS sendiri.

Host Linux/Windows tetap boleh digunakan untuk bootstrap awal, recovery, dan
debugging tingkat rendah. Namun host tidak boleh menjadi dependency pada
workflow normal setelah target akhir tercapai.

### Kriteria self-hosting penuh

Self-hosting baru boleh dinyatakan tercapai jika seluruh hal berikut dibuktikan:

- [ ] Source tree berada di filesystem persisten dan tetap ada setelah reboot.
- [ ] Compiler, assembler, linker, build orchestrator, dan test runner berjalan
      sebagai executable native OCTAGRAM.
- [ ] Native toolchain membangun ulang dirinya sendiri (bootstrap berulang).
- [ ] Kernel dan seluruh userspace dibangun dari source di dalam OCTAGRAM.
- [ ] Test suite dijalankan dari dalam OCTAGRAM dan exit status-nya benar.
- [ ] Image builder native menghasilkan media bootable.
- [ ] Installer/update memasang ke target terpisah dan memverifikasi hasilnya.
- [ ] Rollback bekerja ketika build, write, verifikasi, atau boot gagal.
- [ ] Satu siklus source -> build -> test -> image -> install -> reboot berhasil
      tanpa Linux/Windows ikut menjalankan tool build.

Satu demo compiler atau keberhasilan cross-compile dari host bukan bukti
self-hosting penuh.

---

## 2. Status saat ini

Status harus dibedakan menjadi **source implemented**, **host build-tested**,
**QEMU-tested**, dan **hardware-tested**. Checklist di bawah hanya ditandai
selesai jika bukti sesuai tersedia.

| Area | Kondisi saat ini | Status |
| --- | --- | --- |
| Kernel/proses/ELF | Kernel x86_64, ELF loader, proses dan syscall sudah tersedia | Fondasi ada |
| Filesystem | VFS, EXT2, FAT32, mount dan instalasi persisten tersedia | Perlu validasi workflow source |
| Editor | Perintah editor native tersedia | Perlu uji edit file besar dan persistensi |
| C compiler | TinyCC dibangun untuk ABI OCTAGRAM dan masuk image; perbaikan runtime PT_TLS sudah di-commit | Menunggu CI/QEMU |
| Mini toolchain | Compiler C-subset, assembler/linker mini tersedia | Terbatas, bukan GNU toolchain |
| Native Make | Utility Rust no_std; dukungan target, variable, automatic variable, `-n`, `-s`, `-j1` | Implemented; QEMU test belum terbukti lulus |
| Rust application | Workspace Rust no_std dapat di-cross-compile dari host | Bukan Rust compiler native |
| rustc | Belum terbukti berjalan di OCTAGRAM | Belum tersedia |
| Cargo | Belum terbukti berjalan di OCTAGRAM | Belum tersedia |
| Kernel build native | Kernel masih dibangun dengan host Rust/Cargo/LLVM | Belum tersedia |
| Image builder native | Pembuatan ISO masih memakai host tools | Belum tersedia |
| Safe update/rollback | Belum ada alur A/B penuh yang terbukti | Belum tersedia |

### Temuan diagnostik TinyCC terbaru

CI run [44594022342](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/44594022342)
berhasil mencapai boot QEMU dan menjalankan command `tcc`, tetapi proses
berakhir dengan exception vector 6 pada RIP `0x402a2e`.

Disassembly pada alamat itu menunjukkan instruksi:

```asm
vpxor   %xmm0, %xmm0, %xmm0
```

Instruksi tersebut merupakan AVX/VEX. Ini mempersempit diagnosis: binary
TinyCC mengandung instruksi AVX, tetapi lingkungan CPU/OS guest tidak
menyediakan atau belum mengaktifkan dukungan AVX yang diperlukan. Penyebab
pastinya harus dikonfirmasi dengan memeriksa fitur CPU yang diekspos QEMU,
CR4.OSXSAVE, XCR0, dan flags compiler. Jangan menganggap masalah ini sudah
diperbaiki hanya karena kernel build berhasil.

**Perubahan yang sudah dibuat pada branch pengembangan:**

- `user/Makefile` kini memaksa `-march=x86-64 -mtune=generic` serta
  `-mno-avx -mno-avx2 -mno-avx512f` untuk kompilasi userland, termasuk
  runtime dan object TinyCC yang memakai `CFLAGS` tersebut.
- `.github/workflows/posix-core-stage1.yml` kini memeriksa disassembly
  `user/build/tcc.elf` dan menggagalkan CI jika byte awal instruksi VEX/EVEX
  terdeteksi (menghindari false positive mnemonic seperti `verr`/`verw`).
- `docs/toolchain.md` menjelaskan alasan baseline ISA: kernel belum memiliki
  kontrak XSAVE/XCR0 dan penyimpanan state AVX yang lengkap.

**Hasil diagnostik lanjutan:** compile TinyCC di guest mengembalikan status
0, tetapi ELF hasilnya mengalami user read page fault pada
`RIP=0x413104`, `address=0x0`, `FS_BASE=0`. Byte instruksi yang ditangkap
adalah `64 48 8b 04 25 00 00 00` (`mov %fs:0x0,%rax`). Ini membuktikan
fault terjadi saat akses TLS/TCB melalui FS, bukan lagi instruksi AVX.

**Tindakan isolasi terbaru:** smoke test dipisah menjadi dua sumber:

- `tcc_smoke_minimal.c`: program kecil yang hanya menggunakan `printf` dan
  dipakai sebagai gate compile-run pertama.
- `tcc_smoke.c`: test libc yang lebih luas; tetap dikompilasi sebagai
  pemeriksaan tambahan, tetapi belum dieksekusi sampai test minimal stabil.

Sumber minimal dipasang sebagai `/tmp/tcc.c` dan sumber luas sebagai
`/tmp/tcc-full.c`. CI juga memeriksa byte prefix VEX/EVEX pada instruksi
yang didisassembly, bukan mencari semua mnemonic yang diawali huruf `v`.

**Perbaikan TLS yang sedang diverifikasi:**

- Runtime sebelumnya hanya membaca simbol `__oct_tls_*` yang dihasilkan oleh
  linker script GNU. TinyCC memakai linker internal dan menyediakan metadata
  TLS melalui program header ELF standar `PT_TLS`.
- `user/runtime.c` kini mempertahankan jalur simbol GNU dan memiliki fallback
  untuk membaca `AT_PHDR`, `AT_PHENT`, `AT_PHNUM` dari auxiliary vector,
  menemukan `PT_TLS`, lalu menyalin `p_filesz`, mengosongkan sisa hingga
  `p_memsz`, dan menempatkan TCB di ujung blok TLS yang sudah disejajarkan.
- Jalur `pthread_create` juga memakai template TLS yang sama untuk membuat
  TLS masing-masing thread.
- Commit implementasi: `44d6961e`, `0f977148`, dan
  `b7bcd2ed`. Dua run awal menemukan pemanggil lama di
  `pthread_create` yang masih memakai signature sebelumnya; itu sudah
  diperbaiki pada `b7bcd2ed`.

**Status verifikasi:** implementasi sudah di-commit, tetapi belum lulus build
atau QEMU. Jangan menganggap TinyCC runtime sudah berjalan sampai CI terbaru
menunjukkan output smoke test dan exit status 0.

---

## 3. Prinsip desain

1. **Native berarti berjalan di OCTAGRAM.** Binary Linux yang menghasilkan
   ELF OCTAGRAM tetap dihitung sebagai host cross-build.
2. **Offline-first.** Workspace lokal harus bisa dibangun tanpa internet.
   Registry/package download ditambahkan setelah build lokal stabil.
3. **Bootstrap bertahap.** Stage 0 dibuat di host, Stage 1 berjalan di
   OCTAGRAM dan membangun Stage 2, lalu hasilnya dibandingkan.
4. **Build reproducible.** Catat versi toolchain, flags, input, manifest,
   checksum, dan output.
5. **Filesystem aman.** Jangan membangun atau memasang kernel baru dengan
   menimpa root filesystem yang sedang digunakan.
6. **Fail closed.** Kegagalan compile, link, test, atau verifikasi harus
   menghentikan instalasi.
7. **A/B atau target terpisah.** Pertahankan sistem yang diketahui baik hingga
   build baru lolos boot test.
8. **Jangan mengarang kompatibilitas.** Dukungan POSIX, GNU Make, Rust std,
   Cargo, dan driver harus dinyatakan sesuai fitur yang benar-benar tersedia.
9. **Test native adalah gate.** Source code yang berhasil dikompilasi host
   belum membuktikan executable berjalan di guest.
10. **Satu perubahan satu bukti.** Tiap milestone mempunyai test dan kriteria
    penerimaan yang jelas.

---

## 4. Roadmap milestone

## M0 — Baseline, build contract, dan CI

**Tujuan:** mempunyai baseline yang dapat diulang dan hasil CI yang mudah
ditafsirkan.

- [ ] Inventaris semua artefak: kernel, init, user programs, library, boot
      files, filesystem image, dan ISO.
- [ ] Dokumentasikan dependency build host dan command clean build.
- [ ] Pisahkan hasil host unit test, host build, QEMU boot, QEMU guest test,
      dan hardware test.
- [ ] Pastikan setiap QEMU smoke test mengirim command ke guest dan memeriksa
      output serta exit status.
- [ ] Perbaiki kegagalan formatting CI yang masih muncul pada file Rust.
- [ ] Buat manifest artefak berisi versi, flags, ukuran, dan checksum.
- [ ] Pastikan kegagalan satu smoke test tidak membuat test berikutnya tampak
      lulus karena output lama.

**Gate M0:** clean build host dapat direproduksi, CI memberi status terpisah,
dan setiap smoke test mempunyai bukti output/exit code.

## M1 — Native developer environment dan storage persisten

**Tujuan:** OCTAGRAM dapat menjadi tempat kerja source code sehari-hari.

- [ ] Pastikan root EXT2 persisten dapat di-mount dan dipakai setelah reboot.
- [ ] Tetapkan layout direktori standar:
      `/src`, `/build`, `/toolchain`, `/var/cache/build`,
      `/var/log/build`.
- [ ] Uji editor: create, open, save, overwrite, rename, delete, dan file besar.
- [ ] Pastikan `stat`, `fstat`, `lstat`, dan `fstatat` konsisten.
- [ ] Uji `mkdir`, `rename`, `unlink`, permission, `getcwd`, environment,
      dan path relatif/absolut.
- [ ] Uji redirection dan pipeline shell yang dibutuhkan build tools.
- [ ] Pastikan process spawn/wait, signal, dan exit code dapat diandalkan.
- [ ] Buat test persistensi: edit source -> reboot -> baca kembali -> checksum.

Catatan: syscall metadata sudah ada (termasuk stat family). Jangan membuat
syscall duplikat; perluas kontrak metadata VFS hanya jika data tambahan memang
dibutuhkan.

**Gate M1:** source project dapat diedit, disimpan pada EXT2, dibaca kembali
setelah reboot, dan diproses oleh beberapa executable native.

## M2 — Native C toolchain

**Tujuan:** TinyCC benar-benar bisa dipakai dari dalam OCTAGRAM untuk membangun
program C native.

- [x] Diagnosis exception vector 6: instruksi VEX/AVX telah dihilangkan
      dari TinyCC build dengan baseline x86-64.
- [x] Tetapkan baseline ISA x86_64 yang didukung oleh kernel dan QEMU.
- [x] Hilangkan instruksi AVX dari TinyCC build; AVX kernel tidak diaktifkan.
- [ ] Verifikasi runtime TLS lewat `PT_TLS` dan auxiliary vector pada QEMU.
- [ ] Uji command `tcc source.c -o program.elf` dari shell.
- [ ] Jalankan hasil ELF dari shell dan periksa output serta exit status.
- [ ] Uji argv, envp, file I/O, malloc, TLS, errno, signal, dan stack alignment.
- [ ] Uji include header OCTAGRAM, static libc, relocation, dan diagnostics.
- [ ] Uji proyek C multi-file: compile beberapa object, archive, lalu link.
- [ ] Sediakan antarmuka compiler/assembler/linker yang konsisten.
- [ ] Tegaskan perbedaan `cc` (compiler C-subset kernel) dengan `tcc`
      (TinyCC userspace).

**Gate M2:** proyek C multi-file dibangun sepenuhnya dari shell OCTAGRAM,
hasilnya berjalan, dan tidak memanggil GCC/binutils host.

## M3 — Native build system

**Tujuan:** Make native dapat membangun proyek nyata dengan dependency graph
dan rebuild yang benar.

### Fitur yang sudah diimplementasikan pada source

- [x] Target dan prerequisites eksplisit.
- [x] Variable assignment dasar (`=`, `?=`, `+=`) dan ekspansi.
- [x] Automatic variables dasar (`$@`, `$<`, `$^`).
- [x] Opsi `-C`, `-f`, `-B`.
- [x] Dry-run `-n` / `--dry-run`.
- [x] Silent `-s` / `--silent`.
- [x] Mode serial eksplisit `-j1` (belum berarti parallel build).

### Pekerjaan lanjutan

- [ ] Jalankan smoke test Make di QEMU setelah blocker TinyCC selesai.
- [ ] Tambahkan timestamp modification yang konsisten di VFS.
- [ ] Baca dan simpan mtime EXT2 dengan benar.
- [ ] Tentukan perilaku timestamp untuk RAMFS/devfs dan filesystem tanpa mtime.
- [ ] Tambahkan incremental rebuild berdasarkan target/prerequisite.
- [ ] Tambahkan `include`, pattern rules, dan implicit rules yang diperlukan.
- [ ] Perbaiki quoting dan argument parsing recipe.
- [ ] Pastikan recipe berjalan di shell dengan cwd dan environment yang benar.
- [ ] Tambahkan build log dan manifest dependency.
- [ ] Tambahkan parallel build setelah graph, locking, dan output ordering aman.

Penting: `fs::Metadata` saat ini membawa node type, length, dan permissions;
`struct oct_stat` hanya membawa size dan mode. Timestamp belum tersedia pada
jalur metadata tersebut. Jangan mengimplementasikan incremental Make dengan
mengandalkan timestamp yang belum ada.

**Gate M3:** Make native membangun proyek multi-directory, dan perubahan satu
prerequisite hanya membangun ulang target yang terdampak. Build kedua tanpa
perubahan tidak menjalankan recipe yang tidak perlu.

## M4 — Rust compiler native bootstrap

**Tujuan:** menjalankan `rustc` native di OCTAGRAM. Ini salah satu jalur kritis
karena kernel utama ditulis dalam Rust.

- [ ] Inventaris komponen bootstrap: rustc, LLVM/codegen backend, compiler
      builtins, target spec, sysroot, rust-src, dan linker.
- [ ] Pilih versi Rust dan pin seluruh komponen bootstrap.
- [ ] Tentukan target triple dan ABI OCTAGRAM secara eksplisit.
- [ ] Buat Stage 0 compiler yang dapat dieksekusi oleh OCTAGRAM.
- [ ] Lengkapi syscall/runtime minimum yang dibutuhkan compiler.
- [ ] Port backend/codegen dan dependensi native yang diperlukan.
- [ ] Jalankan `rustc` native untuk compile program sederhana.
- [ ] Bangun `core` dan `alloc` untuk target OCTAGRAM.
- [ ] Compile dan jalankan crate `no_std` dari source di dalam OS.
- [ ] Bangun Cargo native untuk workspace offline.
- [ ] Implementasikan build scripts/proc macros jika dibutuhkan dependency
      workspace.
- [ ] Tambahkan `std` secara bertahap; jangan menyamakan `core + alloc`
      dengan Rust `std` lengkap.
- [ ] Lakukan bootstrap Stage 0 -> Stage 1 -> Stage 2 dan uji hasilnya.

**Gate M4:** dari terminal OCTAGRAM, `rustc` mengompilasi crate target
OCTAGRAM dan executable hasilnya berjalan. Cargo native membangun workspace
offline. Compiler hasil bootstrap dapat membangun ulang versi berikutnya.

## M5 — Native kernel build

**Tujuan:** membangun kernel OCTAGRAM dari source di dalam OCTAGRAM.

- [ ] Port seluruh tahapan build kernel yang saat ini memakai Cargo, rustc,
      LLVM, assembler, linker, dan build scripts host.
- [ ] Implementasikan dukungan linker script, ELF sections, symbols,
      relocations, entry point, dan boot protocol yang diperlukan.
- [ ] Bangun kernel dari source tree EXT2 menggunakan toolchain native.
- [ ] Simpan artefak debug/symbol terpisah dari image produksi.
- [ ] Boot kernel hasil build di QEMU sebagai target kedua.
- [ ] Jalankan smoke test kernel dan userspace yang sama seperti build host.
- [ ] Bandingkan manifest dan hasil build Stage 0/native.

**Gate M5:** kernel yang dibangun di dalam OCTAGRAM berhasil boot pada target
kedua dan lulus smoke test yang sama dengan kernel hasil host build.

## M6 — Native image builder dan installer

**Tujuan:** membuat disk image bootable tanpa mengandalkan xorriso, mtools,
GNU coreutils, atau shell host.

- [ ] Implementasikan image builder native.
- [ ] Tulis GPT dengan CRC dan validasi header/partition entries.
- [ ] Buat ESP FAT32 dan root EXT2.
- [ ] Salin kernel, init/userspace, library, bootloader, dan konfigurasi.
- [ ] Tentukan cara memasang bootloader BIOS/UEFI dan format boot files secara
      benar; filesystem image saja belum tentu bootable.
- [ ] Buat manifest file, checksum, dan batas ukuran.
- [ ] Baca ulang image dan verifikasi GPT, filesystem, dan seluruh file.
- [ ] Sediakan mode dry-run dan progress/error reporting.
- [ ] Installer harus menolak target yang sama dengan root aktif.

**Gate M6:** OCTAGRAM membuat image bootable dari source/artefak native dan
image tersebut berhasil boot di QEMU.

## M7 — Safe update, A/B slot, dan rollback

**Tujuan:** menghindari kerusakan sistem saat memasang kernel atau root baru.

- [ ] Rancang slot A/B atau dua target boot yang terpisah.
- [ ] Build ke direktori/partisi staging, bukan root aktif.
- [ ] Jalankan test dan verifikasi checksum sebelum update boot entry.
- [ ] Tulis ke slot tidak aktif.
- [ ] Simpan known-good kernel/root dan boot configuration.
- [ ] Atur percobaan boot terbatas dan rollback jika boot tidak sehat.
- [ ] Uji kegagalan write, image corrupt, checksum mismatch, dan power loss.
- [ ] Dokumentasikan recovery melalui USB/media eksternal.

**Gate M7:** update gagal tidak merusak sistem yang sedang berfungsi; rollback
dapat mengembalikan sistem ke versi terakhir yang diketahui baik.

## M8 — Pembuktian self-hosting penuh

**Tujuan:** membuktikan seluruh alur pengembangan dilakukan oleh OCTAGRAM.

- [ ] Boot OCTAGRAM dari instalasi persisten.
- [ ] Edit source di dalam OCTAGRAM.
- [ ] Build ulang compiler dan toolchain native.
- [ ] Build kernel, library, dan seluruh userspace dari source.
- [ ] Jalankan unit/integration/smoke tests native.
- [ ] Buat image bootable native.
- [ ] Install ke slot/disk lain dan verifikasi.
- [ ] Reboot ke kernel/image yang baru dibangun.
- [ ] Jalankan test pasca-boot dan catat hasil.
- [ ] Ulangi bootstrap compiler untuk membuktikan toolchain dapat membangun
      dirinya sendiri.
- [ ] Simpan laporan final berisi commit, command, versi toolchain, manifest,
      checksum, log, QEMU result, dan hardware result.

**Gate M8:** satu siklus source -> compiler -> kernel -> image -> test -> install
-> reboot -> test berjalan dari dalam OCTAGRAM tanpa host build tools.

---

## 5. Urutan dependency dan jalur kritis

```text
M0 Baseline + CI
       |
       v
M1 Persistent source + editor + process/filesystem APIs
       |
       +--------------------------+
       |                          |
       v                          v
M2 Native C toolchain       M4 Rust compiler bootstrap
       |                          |
       v                          |
M3 Native Make/build engine <-----+
       |                          |
       +-------------+------------+
                     |
                     v
              M5 Native kernel build
                     |
                     v
          M6 Native image builder
                     |
                     v
          M7 Safe update + rollback
                     |
                     v
             M8 Full proof
```

M4 harus diteliti sejak awal secara paralel karena kernel menggunakan Rust.
TinyCC dan Make tetap bernilai untuk membangun utility, test tools, dan image
builder, tetapi tidak menggantikan kebutuhan rustc native untuk kernel.

---

## 6. Aturan pengujian dan pelaporan

Gunakan status berikut untuk setiap fitur:

| Status | Arti |
| --- | --- |
| Planned | Baru direncanakan |
| Implemented | Source code tersedia |
| Host build-tested | Berhasil dibangun di host |
| Host test-passed | Unit/integration test di host lulus |
| QEMU boot-tested | OS berhasil boot di QEMU |
| QEMU guest-tested | Command/executable benar-benar dijalankan di guest dan hasil diperiksa |
| Hardware-tested | Diuji pada perangkat fisik; sebutkan model/perangkat |

Untuk setiap milestone, catat:

- Commit SHA dan branch.
- Perintah build/test yang persis.
- Compiler/toolchain version dan flags.
- Log kegagalan maupun keberhasilan.
- Artefak dan checksum jika relevan.
- Lingkungan uji: host, QEMU, atau hardware.

Jangan mengubah checklist menjadi selesai hanya karena kode sudah ditulis.
Jangan menyebut build host sebagai native runtime test.

---

## 7. Keputusan teknis awal

- Fokus x86_64 sampai self-hosting terbukti; arsitektur lain bukan blocker awal.
- Gunakan EXT2 persisten untuk source dan build awal, dengan backup karena
  EXT2 tidak menyediakan journaling.
- Gunakan TinyCC sebagai compiler C awal, setelah ISA baseline dan runtime
  guest konsisten.
- Gunakan native Make serial terlebih dahulu. Parallel build ditunda sampai
  dependency graph dan locking dapat dipercaya.
- Mulai Rust bootstrap sebagai jalur riset paralel, bukan menunggu seluruh
  fitur Make selesai.
- Prioritaskan Cargo offline; networking/package registry datang belakangan.
- Jangan menulis ke root filesystem aktif saat memasang kernel baru.
- Jangan menambahkan syscall metadata baru sebelum audit ABI/VFS menunjukkan
  kebutuhan yang belum tercakup.

---

## 8. Next actions — urutan kerja terdekat

1. **M2.1 — baseline ISA:** sudah diterapkan dan guard disassembly
   `.text` tidak menemukan instruksi AVX/VEX.
2. **M2.2 — runtime TLS:** fault teridentifikasi sebagai
   `mov %fs:0x0,%rax` dengan `FS_BASE=0`; runtime kini membaca `PT_TLS`
   dari auxiliary vector saat simbol khusus GNU tidak tersedia.
3. **M2.3 — kompilasi:** pastikan seluruh pemanggil `oct_tls_layout` dan
   `oct_tls_copy` sudah memakai signature baru; commit terakhir
   `b7bcd2ed` memperbarui jalur `pthread_create`.
4. **M2.4 — verifikasi QEMU:** jalankan workflow terbaru. Gate pertama adalah
   compile dan execute `/tmp/tcc-selfhost-minimal.elf`; setelah itu uji
   executable smoke lebih lengkap, errno, TLS, dan thread.
5. **M3.1:** setelah TinyCC runtime lulus, jalankan Make `-n`, `-s`, dan
   `-j1` pada guest; kemudian implementasikan timestamp-based incremental
   build secara backward-compatible dengan metadata VFS yang tersedia.
6. **M4.1:** mulai desain bootstrap rustc secara paralel: versi, target triple,
   sysroot, backend, dependensi OS, dan strategi Stage 0.

Jangan lanjut ke M6 sebagai prioritas sebelum jalur native compiler dan build
kernel dapat dibuktikan. Image builder tidak menyelesaikan blocker compiler.
