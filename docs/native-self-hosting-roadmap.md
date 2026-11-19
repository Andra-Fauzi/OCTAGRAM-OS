# Roadmap Native Self-Hosting OCTAGRAM-OS

Status dokumen: rencana teknis awal, diaudit terhadap branch
`feature/persistent-root-install` pada commit
`177d283a451175a050b0caa9d41c6a85bd335e4a`.
Branch implementasi roadmap: `feature/native-self-hosting`.

## 1. Visi dan definisi selesai

Tujuan akhir bukan sekadar menjalankan compiler di OCTAGRAM. OCTAGRAM harus
mampu menjadi lingkungan pengembangan utamanya sendiri: mengedit source,
mengompilasi seluruh userspace dan kernel, menjalankan tes, membuat artefak
bootable, memasang versi baru ke media/partisi lain, memverifikasi hasil, dan
reboot ke build tersebut tanpa Linux/Windows ikut dalam workflow normal.

Host tetap boleh dipakai untuk bootstrap awal, recovery, dan debugging saat
pengembangan. Namun, setelah self-hosting tercapai, host bukan dependency
untuk build harian.

### Definisi self-hosting penuh

Semua butir berikut harus terbukti di mesin OCTAGRAM yang sedang berjalan:

1. Source tree OCTAGRAM tersimpan pada filesystem persisten dan dapat diedit.
2. Compiler, assembler, linker, build orchestrator, dan tool pendukung berjalan
   sebagai executable OCTAGRAM (bukan binary Linux).
3. Build dari source menghasilkan kernel dan seluruh userland OCTAGRAM.
4. Test suite native dapat dijalankan dan memberikan exit status yang dapat
   dipercaya.
5. Image builder menghasilkan media bootable dari dalam OCTAGRAM.
6. Installer/updater memasang hasil ke target yang berbeda dari root aktif,
   memverifikasi file dan boot configuration, serta menyediakan rollback.
7. Bootstrap berulang: toolchain native dapat membangun ulang dirinya sendiri
   dan menghasilkan toolchain generasi berikutnya yang berfungsi.

Tidak cukup jika hanya compiler kecil atau satu program C/Rust berhasil
dijalankan. Status implementasi, build, QEMU, dan hardware harus dicatat
terpisah.

## 2. Audit baseline

### Yang sudah tersedia pada baseline

| Komponen | Temuan audit | Status untuk self-hosting |
| --- | --- | --- |
| Kernel dan proses | ELF64 loader, proses user, fork/exec/wait, thread, syscall, scheduler | Fondasi tersedia; perlu verifikasi API yang dibutuhkan toolchain |
| VFS dan storage | RAMFS, devfs, FAT32, ext2, mount, GPT, installer root ext2 | Ada; source tree perlu hidup di root persisten dan storage perlu hardening |
| Native C compiler | TinyCC dibangun ke image sebagai `/bin/tcc.elf` | Ada jalur native; perlu uji compile/link/run end-to-end |
| Toolchain mini | Assembler, linker, dan C-subset di `kernel/src/toolchain.rs` | Eksperimental, bukan pengganti toolchain lengkap |
| Build utility | Program `make` Rust `no_std` | Native executable ada; parser dan semantics Make masih terbatas |
| Rust userspace | `user/rust`, `octagram-runtime`, `rust_hello`, `make` | Dapat di-cross-compile dari host ke ELF OCTAGRAM |
| Rust compiler | Build menggunakan Cargo/nightly dari host dengan target `x86_64-unknown-none` | `rustc` belum terbukti berjalan di OCTAGRAM |
| Cargo | Hanya dipanggil oleh Makefile host | Cargo native belum ada |
| Kernel build | Kernel utama ditulis dalam Rust dan dibangun lewat Cargo | Terblokir pada rustc/Cargo native |
| ISO builder | Root GNUmakefile memakai host tools, Limine, xorriso, shell utilities | Belum ada image builder native |
| Test | Unit test host dan QEMU syscall smoke test | Belum ada test runner native lengkap |

### Bukti baseline build yang perlu diingat

Pada workflow `POSIX Core Stage 1`, run 74 untuk commit
`177d283a451175a050b0caa9d41c6a85bd335e4a`, langkah build Rust utility,
program user, kernel, dan ISO berhasil. Langkah QEMU syscall compatibility
smoke test gagal: `syscall-compat-demo` mengalami invalid opcode
(exception vector 6, RIP `0x402a2e`) sebelum meminta input TTY.
Kegagalan baseline ini harus ditangani sebagai isu terpisah dan tidak boleh
ditutupi oleh milestone self-hosting.

### Risiko dan ketergantungan utama

- Kernel ditulis dalam Rust. Native C compiler saja tidak dapat membangun kernel.
- `rustc` adalah compiler yang besar dan bergantung pada Rust runtime/library
  serta backend code generation; port native Rust merupakan pekerjaan besar.
- Cargo membutuhkan `rustc`, filesystem/process API, environment, working
  directory, pipes, locking, atomic rename, timestamps, threads, dan networking
  untuk workflow crates.io. Build offline harus didukung lebih dahulu.
- Build ISO saat ini mengandalkan Limine, xorriso, mtools, binutils, curl/git,
  dan utilitas host. Pengganti native atau strategi membuat disk image bootable
  dari tool yang tersedia harus dirancang eksplisit.
- Root RAMFS bersifat volatil jika tidak boot dari instalasi persisten.
- Installer tidak boleh menimpa root aktif ketika membangun atau memasang
  kernel baru. Gunakan target terpisah dan rollback.

## 3. Prinsip arsitektur

1. **Native executable only:** tool yang dihitung native harus memakai ABI
   OCTAGRAM dan berjalan melalui loader OCTAGRAM. Binary Linux tidak dihitung.
2. **Bootstrap bertahap:** host boleh menghasilkan Stage 0. Stage 0 berjalan
   di OCTAGRAM untuk membangun Stage 1; Stage 1 membangun Stage 2.
3. **Offline-first:** compiler dan build system harus dapat membangun source
   yang sudah tersedia tanpa jaringan. Package registry/networking ditambahkan
   setelah toolchain lokal stabil.
4. **Cross-build safety:** build output tidak boleh ditulis ke source tree
   secara tidak terkendali; gunakan direktori build dan dependency manifest.
5. **Reproducibility:** catat versi compiler, flags, input, dan hash artefak.
6. **Fail closed:** kegagalan compile, link, test, atau verifikasi menghentikan
   instalasi; jangan reboot otomatis ke image yang belum lolos.
7. **A/B update:** root aktif tetap utuh sampai image baru lolos verifikasi.
8. **Evidence-based status:** fitur hanya ditandai selesai setelah test native
   yang spesifik lulus; dokumentasi atau file source saja bukan bukti runtime.

## 4. Milestone implementasi

### M0 — Baseline dan kontrak build

Tujuan: memastikan baseline dapat direproduksi dan menentukan artefak yang
harus dibangun.

- [ ] Simpan daftar target build host saat ini dan semua dependency input.
- [ ] Pisahkan status build host, QEMU, dan hardware dalam CI.
- [ ] Perbaiki atau isolasi kegagalan QEMU smoke test baseline (vector 6 pada
  `syscall-compat-demo`).
- [ ] Tambahkan manifest artefak: kernel, init/userland, library, boot files,
  filesystem image, checksum.
- [ ] Dokumentasikan perintah clean build dan dependency graph.

Gate: clean host build menghasilkan ISO; smoke test mempunyai hasil yang
terdiagnosis; semua artefak mempunyai manifest.

### M1 — Native developer substrate

Tujuan: membuat OCTAGRAM nyaman dan aman untuk menyimpan serta mengedit source.

- [ ] Pastikan instalasi ext2 persisten dapat dipakai sebagai root kerja.
- [ ] Sediakan direktori standar: `/src`, `/build`, `/toolchain`,
  `/var/cache/build`, `/var/log/build`.
- [ ] Implementasikan editor teks terminal native (buat, buka, simpan,
  pencarian, navigasi, file besar, penanganan input).
- [ ] Pastikan `mkdir`, `stat`, `rename`, `unlink`, `chmod`,
  `getcwd`, environment, dan path panjang berperilaku konsisten.
- [ ] Tambahkan shell redirection dan pipeline yang diperlukan build system.
- [ ] Tambahkan proses spawn/wait dan propagasi exit code yang dapat diandalkan.
- [ ] Tambahkan uji read/write source pada ext2 setelah reboot.

Gate: source kecil dapat diedit, disimpan di ext2, dibaca ulang setelah reboot,
dan diproses beberapa executable native dengan exit status benar.

### M2 — Native C toolchain

Tujuan: mengubah TinyCC dari compiler yang sekadar ada di image menjadi
toolchain native yang teruji.

- [ ] Uji `tcc` dari shell: compile C -> ELF OCTAGRAM -> execute.
- [ ] Uji include header, libc statis, relocation, argv/envp, file I/O, dan
  error diagnostics.
- [x] Tambahkan command wrapper `tcc` pada shell untuk menjalankan `/bin/tcc.elf`;
  `cc` tetap compiler C-subset kernel dan `as`/`ld` tetap toolchain mini.
- [ ] Satukan antarmuka compiler/assembler/linker menjadi toolchain ELF native
  yang konsisten; dokumentasikan satu sintaks canonical.
- [ ] Tambahkan assembler dan linker yang mampu menangani object ELF yang
  dihasilkan compiler; toolchain mini tetap diberi label terbatas.
- [ ] Bangun program multi-file dengan beberapa object dan static library.
- [ ] Tambahkan test positif dan negatif untuk compiler/linker.

Gate: proyek C multi-file dibangun sepenuhnya di OCTAGRAM dan hasilnya
dijalankan; tidak ada GCC/binutils host yang dipanggil.

### M3 — Native build orchestrator

Tujuan: build system native yang cukup kuat untuk membangun proyek OS.

- [ ] Kembangkan `make` agar mendukung target, prerequisites, variables,
  automatic variables, pattern/implicit rules yang dibutuhkan, dan include.
- [ ] Tambahkan timestamp/metadata yang andal dan incremental rebuild.
- [ ] Tambahkan recipe exit code, quoting, environment, cwd, dan log.
- [ ] Tambahkan parallel build hanya setelah dependency graph dan locking aman.
- [ ] Sediakan mode `-n`, `-B`, `-j1`, dan build log.
- [ ] Buat build manifest agar hasil build dapat direproduksi.

Gate: build proyek C multi-directory dan dependency graph bertingkat di
OCTAGRAM, lalu rebuild incremental hanya mengompilasi input yang berubah.

### M4 — Rust native bootstrap

Tujuan: jalankan Rust compiler di OCTAGRAM. Ini milestone paling berat dan
menjadi prasyarat native build kernel.

Tahapan:
1. [ ] Inventaris target Rust host components: `rustc`, LLVM/backend,
   `rust-std`, linker, compiler-builtins, proc-macro runtime.
2. [ ] Tentukan strategi bootstrap Stage 0 yang dapat dijalankan pada ABI
   OCTAGRAM; host hanya menghasilkan artefak awal.
3. [ ] Lengkapi API OS yang dibutuhkan runtime Rust: file/process, memory,
   threads/synchronization, time, environment, dynamic loading sesuai kebutuhan.
4. [ ] Bangun dan jalankan executable `rustc` native pada OCTAGRAM.
5. [ ] Kompilasi crate `no_std` target OCTAGRAM dari dalam OS.
6. [ ] Bangun Rust `core` dan `alloc` untuk target OCTAGRAM secara konsisten.
7. [ ] Bangun Cargo native untuk build offline workspace.
8. [ ] Tambahkan proc-macro/build-script support dan registry/networking setelah
   Cargo offline stabil.
9. [ ] Evaluasi port `std` bertahap; jangan menganggap `core + alloc` berarti
   `std` sudah tersedia.
10. [ ] Jalankan bootstrap Stage 1 -> Stage 2 dan bandingkan versi, test, serta
    hash artefak sejauh build reproducible memungkinkan.

Gate: dari terminal OCTAGRAM, `rustc` mengompilasi crate no_std menjadi ELF
yang dijalankan OS; Cargo native membangun workspace offline; toolchain hasil
bootstrap dapat membangun ulang dirinya.

### M5 — Native kernel build

Tujuan: membangun kernel OCTAGRAM dari source di dalam OCTAGRAM.

- [ ] Port semua langkah kernel build yang sekarang dijalankan Cargo/LLVM,
  linker, assembler, dan build scripts.
- [ ] Pastikan linker script, symbol, relocation, section, entry point, dan
  boot protocol cocok dengan kernel saat ini.
- [ ] Bangun kernel dari source tree persisten menggunakan toolchain native.
- [ ] Jalankan kernel hasil build di QEMU atau target kedua tanpa mengganti
  root/source yang sedang dipakai.
- [ ] Tambahkan kernel config/build profile dan symbol/debug artifact.

Gate: kernel yang dibuat dari dalam OCTAGRAM boot di target kedua dan lulus
smoke test yang sama dengan kernel host-built.

### M6 — Native image builder dan installer

Tujuan: membuat bootable disk/ISO tanpa xorriso/mtools/host shell.

- [ ] Bangun image builder native untuk GPT, FAT32 ESP, ext2 root, dan file copy.
- [ ] Sediakan penulisan bootloader Limine/EFI yang lisensi dan formatnya jelas;
  jangan mengasumsikan formatter filesystem otomatis menghasilkan bootable disk.
- [ ] Buat image manifest dan checksum.
- [ ] Verifikasi tabel partisi, filesystem metadata, file kernel, init/userland,
  konfigurasi boot, dan batas ukuran.
- [ ] Tambahkan simulasi image dan test read-back.
- [ ] Installer harus menargetkan disk/partisi lain, bukan root aktif.

Gate: OCTAGRAM membuat image bootable dari source dan artefak native, kemudian
image itu berhasil boot di QEMU.

### M7 — Safe self-update, rollback, dan self-host proof

Tujuan: workflow penuh dan aman dari satu boot OCTAGRAM ke boot berikutnya.

- [ ] Terapkan A/B root atau mekanisme target-slot terpisah.
- [ ] Build -> test -> package -> write inactive slot -> verify -> set next boot.
- [ ] Simpan known-good kernel/root dan mekanisme rollback otomatis/manual.
- [ ] Jangan ubah boot default sebelum verifikasi lengkap berhasil.
- [ ] Uji listrik mati atau write failure pada setiap tahap kritis.
- [ ] Dokumentasikan prosedur recovery dari media eksternal.
- [ ] Lakukan bootstrap berulang: native toolchain membangun toolchain generasi
  berikutnya dan membangun kernel/image baru.
- [ ] Buat laporan self-hosting yang mencatat command, compiler versions,
  hashes, test results, QEMU boot result, dan hardware result terpisah.

Gate final: pengembangan rutin source -> compiler -> kernel -> image -> test ->
install -> reboot dilakukan sepenuhnya dari OCTAGRAM-OS, dengan rollback
berfungsi. Host tidak diperlukan dalam workflow normal.

## 5. Urutan dependency dan jalur kritis

```text
Persistent ext2 + editor + process/file APIs
                  |
                  v
        Native TinyCC + assembler/linker
                  |
                  v
          Native make/build engine
                  |
                  v
       Rust compiler bootstrap + Cargo
                  |
                  v
          Native kernel build
                  |
                  v
       Native image builder + installer
                  |
                  v
       A/B update + rollback + proof
```

Catatan: jalur Rust harus dimulai lebih awal sebagai riset/bootstrap paralel,
karena kernel utama ditulis dalam Rust. M2/M3 tetap berguna untuk membangun
utilities, editor, image tools, dan test harness native.

## 6. Definisi native vs bukan native

| Aktivitas | Dihitung native? |
| --- | --- |
| GCC Linux menghasilkan ELF OCTAGRAM | Tidak; cross-build host |
| Cargo Linux menghasilkan `no_std` ELF OCTAGRAM | Tidak; cross-build host |
| TinyCC OCTAGRAM menghasilkan ELF OCTAGRAM dan ELF itu berjalan di OCTAGRAM | Ya |
| Toolchain mini kernel mengeluarkan ELF sederhana | Ya, tetapi hanya untuk subset yang didukung |
| `rustc` OCTAGRAM mengompilasi crate OCTAGRAM | Ya |
| Cargo OCTAGRAM membangun workspace secara offline | Ya |
| OCTAGRAM menjalankan script host melalui emulasi/compat layer | Tidak, kecuali seluruh tool dan ABI native benar-benar berjalan sebagai proses OCTAGRAM |
| OCTAGRAM membangun kernel dan image yang kemudian boot | Bukti utama self-hosting |

## 7. Aturan pengujian dan pelaporan

Setiap item roadmap memakai label berikut:

- **Implemented:** source code tersedia.
- **Build-tested:** target berhasil dikompilasi.
- **QEMU-tested:** executable/OS berjalan di QEMU dan output yang diharapkan diamati.
- **Hardware-tested:** diuji pada perangkat fisik yang disebutkan.

Satu label tidak otomatis menyiratkan label berikutnya. Untuk tiap milestone simpan
command exact, commit SHA, toolchain version, log ringkas, dan artefak uji.
Jangan tandai milestone selesai hanya karena CI build berhasil.

## 8. Keputusan awal yang direkomendasikan

- Arsitektur target pertama: x86_64 saja sampai self-hosting terbukti; port
  arsitektur lain tidak menjadi blocker.
- Filesystem kerja: ext2 persisten yang sudah ada, dengan backup/rollback
  karena ext2 belum memiliki journaling.
- Compiler C awal: TinyCC native.
- Rust: bootstrap compiler native adalah jalur kritis; `no_std` runtime saat
  ini hanya bukti aplikasi Rust dapat berjalan, bukan bukti compiler native.
- Build pertama: offline dan serial (`-j1)); paralelisme dan crates.io datang
  setelah correctness.
- Instalasi: selalu build ke target disk/slot terpisah. Jangan pernah
  overwrite root yang sedang menjalankan compiler/build.
