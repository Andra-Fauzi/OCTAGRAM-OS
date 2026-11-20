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

- [x] Uji `tcc` dari shell: compile C -> ELF OCTAGRAM -> execute (QEMU-tested; CI run 44693171534).
- [ ] Uji include header, libc statis, relocation, argv/envp, file I/O, dan
  error diagnostics.
- [x] Tambahkan command wrapper `tcc` pada shell untuk menjalankan `/bin/tcc.elf`;
  `cc` tetap compiler C-subset kernel dan `as`/`ld` tetap toolchain mini.
- [ ] Satukan antarmuka compiler/assembler/linker menjadi toolchain ELF native
  yang konsisten; dokumentasikan satu sintaks canonical.
- [ ] Tambahkan assembler dan linker yang mampu menangani object ELF yang
  dihasilkan compiler; toolchain mini tetap diberi label terbatas.
- [x] Bangun program multi-file dengan beberapa object dan static library (QEMU-tested melalui TinyCC dan native Make).
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
- [x] Implementasikan mode `-n`/`--dry-run`, `-s`/`--silent`, dan `-j1` pada native `make`.
- [x] Validasi native Make di QEMU: `-n`, `-j1 -n`, target phony/order-only, incremental rebuild saat prerequisite berubah, dan `-B`.
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

## 9. Audit lanjutan branch native-self-hosting (20 November 2026)

Audit source dan CI terbaru menemukan beberapa hal yang mengubah urutan kerja:

- ABI syscall sudah menyediakan `stat` (6), `fstat` (29), `lstat` (38), dan
  `fstatat` (41). Header `user/include/sys/stat.h` dan wrapper di
  `user/runtime.c` juga sudah ada. Jadi M3 tidak boleh menambahkan syscall
  metadata duplikat.
- Representasi `struct oct_stat` saat ini hanya membawa `size` dan `mode`;
  `fs::Metadata` hanya membawa `node_type`, `len`, dan `permissions`.
  Timestamp modifikasi belum tersedia melalui metadata VFS yang dipakai
  syscall. Karena itu incremental Make belum aman untuk diimplementasikan
  hanya dengan membandingkan waktu file.
- CI run `44591453041` berhasil pada formatting diagnostic, build utility,
  build user programs, dynamic userland, 371 kernel host tests, libc host tests,
  kernel build, dan ISO build. Namun QEMU native TinyCC compile/run gagal:
  proses `tcc /tmp/tcc.c -o /tmp/tcc-selfhost-smoke.elf` mendapat exception
  vector 6 (invalid opcode) pada RIP `0x402a2e`. Langkah QEMU syscall
  compatibility dilewati karena langkah sebelumnya gagal.
- Workflow menambahkan disassembly diagnostic di sekitar alamat tersebut
  untuk run berikutnya. Sampai instruksi penyebab diketahui dan diperbaiki,
  M2 native TinyCC tetap berstatus implemented/build-tested tetapi belum
  QEMU-tested. Jangan menganggap Make dry-run QEMU test lulus karena test
  tersebut berada setelah compile/run TinyCC.

Urutan kerja yang benar setelah diagnosis: perbaiki instruksi/ABI penyebab
invalid opcode; buktikan TinyCC compile dan hasil ELF berjalan di QEMU; baru
perluas VFS metadata (termasuk mtime dari EXT2 dan perilaku filesystem yang
belum memiliki timestamp) lalu aktifkan incremental Make dengan test perubahan
prerequisite/target. 


## 10. Hasil implementasi dan validasi lanjutan (20 November 2026)

Branch saat audit: `feature/native-self-hosting`
Commit validasi: `c99bd185b40f624915b386914ccb08b0038d9897`
GitHub Actions: [POSIX Core Stage 1 run 44693171534](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/44693171534)
Hasil: **success** (build host, kernel tests, userland, ISO, dan dua smoke test QEMU).

### Perbaikan regresi Rust guest

Run sebelumnya `44692132324` gagal di QEMU syscall smoke test, bukan karena
kernel file syscall: program `rust-hello` mengembalikan exit status 2 sebelum
memanggil `stat/open/read/close`. Penyebabnya adalah byte string path memakai
`\\\\0` (karakter backslash dan angka nol), bukan terminator NUL Rust
`\\0`. Marker PASS juga memakai escape newline ganda. Keduanya diperbaiki
di `user/rust/hello/src/main.rs` pada commit `c99bd185`.

Run `44693171534` kemudian membuktikan:
- `Rust no_std + alloc` berjalan sebagai ELF OCTAGRAM native.
- Runtime Rust berhasil melakukan `stat`, `open`, `read`, dan `close`
  pada `/share/octagram/message.c`, lalu mencetak
  `Rust runtime file/stat: PASS` dan keluar dengan status 0.
- Guest mmap, static TLS, dan preemptive threads mengeluarkan marker lulus.
- QEMU syscall compatibility test berakhir dengan `SYSCALL-TEST: PASS`.

### M2 — Native C toolchain

Status: **QEMU-tested untuk jalur TinyCC compile/link/run yang diuji**.
CI mencatat `TCC-SELFHOST-PASS`: TinyCC mengompilasi source menjadi ELF,
menjalankan hasilnya, membangun program multi-file dengan static archive,
dan mengembalikan exit status 0. Ini belum berarti semua fitur TinyCC,
header, relocation, atau diagnostics sudah kompatibel penuh.

### M3 — Native Make

Status: **QEMU-tested untuk subset build graph yang diuji**.
CI mencatat keberhasilan:
- `make -n` dan `make -j1 -n`;
- build dan eksekusi target;
- skip ketika target masih up-to-date;
- rebuild setelah prerequisite diperbarui;
- rebuild paksa dengan `make -B`;
- target phony dan order-only prerequisite.

Belum dianggap sebagai implementasi GNU Make penuh. Pattern rules, include,
variable expansion lengkap, quoting/shell semantics, parallel execution,
locking, dan reproducible build manifest masih perlu ditangani sesuai
kebutuhan proyek.

### M4 — Rust native bootstrap

Status: **runtime ABI groundwork QEMU-tested; rustc/Cargo native belum ada**.
Bukti baru adalah aplikasi `no_std + alloc` yang di-cross-compile pada host
kemudian dijalankan sebagai ELF native OCTAGRAM. Ini membuktikan sebagian
jalur runtime (allocator/mmap, file syscall, TLS, dan thread guest demos),
bukan compiler Rust yang berjalan di OCTAGRAM.

Langkah berikut M4:
1. Audit ABI dan dependency runtime yang dibutuhkan executable host Rust
   (`rustc` dan komponen LLVM/backend).
2. Tentukan Stage 0 yang benar-benar dapat dieksekusi pada ABI OCTAGRAM.
3. Bangun dependency minimum untuk menjalankan toolchain bootstrap native;
   pisahkan kebutuhan `rustc` dari kebutuhan Cargo.
4. Lanjutkan API process, filesystem, memory mapping, thread, TLS, time,
   environment, dynamic loading, dan subprocess sesuai hasil audit.
5. Pertahankan target pertama x86_64 dan workflow offline.

### Batas status yang tidak boleh dilewati

- **Tidak** ada bukti bahwa `rustc` berjalan native di OCTAGRAM.
- **Tidak** ada bukti bahwa Cargo berjalan native atau membangun workspace.
- **Tidak** ada bukti bahwa kernel Rust dibangun dari dalam OCTAGRAM.
- **Tidak** ada bukti bahwa ISO/disk image dibuat native.
- **Tidak** ada bukti self-update A/B atau rollback.

Karena itu M4–M7 tetap belum selesai. Fokus berikutnya adalah riset dan
bootstrap `rustc`, bukan memperluas installer atau mengklaim self-hosting
penuh dari keberhasilan TinyCC.


## 11. M4.1 — Experimental Rust host target scaffold (20 November 2026)

Branch `feature/native-self-hosting` menambahkan:
- `user/rust/targets/x86_64-unknown-octagram.json`: target identity
  scaffold dengan cfg `target_os=octagram`, x86_64, static executable,
  static relocation, panic abort, dan SSE/SSE2.
- `tools/check_octagram_rust_target.py`: menjalankan
  `rustc +nightly --print cfg` dan memeriksa identitas target.
- CI step `Validate experimental OCTAGRAM Rust host target`.

Dokumen detail: [rust-native-bootstrap.md](rust-native-bootstrap.md),
bagian 3.4.

**Batas penting:** LLVM backend triple masih `x86_64-unknown-none`; target
JSON hanya menyiapkan identitas/cfg untuk eksperimen `no_std`. Rust `std`,
libc port, dynamic loader, dan executable rustc/Cargo untuk OCTAGRAM belum
tersedia. Jangan menandai M4 selesai.

Audit juga mengonfirmasi kernel memiliki FPU save/restore (`fxsave64` /
`fxrstor64`) dan state FPU pada setiap scheduled process. Ini fondasi SSE2,
tetapi guest test khusus preservasi register FPU lintas preemption, fork, dan
thread switch masih perlu ditambahkan sebelum mengandalkan SSE2 untuk compiler
host. TLS FS base juga disimpan/dipulihkan, sedangkan TLS pada shared object
belum didukung.

Commit implementasi:
- `7362afa5` — target JSON.
- `cd100765` — target validator.
- `b3387092` — CI validation step.
- `0b3c92df` — dokumentasi bootstrap dan ABI gates.

M4.1 masih **in progress**. Selain target identity scaffold, sekarang tersedia
satu executable Rust no_std + alloc yang dibangun dengan custom target
x86_64-unknown-octagram dan dijalankan dari shell OCTAGRAM (QEMU-tested).
Namun ABI host belum lengkap dan compiler/Cargo native belum tersedia.

## 12. M4.1.1 — Custom-target Rust executable dan runtime ABI (20 November 2026)

Branch implementasi: feature/native-self-hosting
Commit yang diuji tanpa diagnostik scheduler: 80e823fd4212635bbca3273b23608035ebe1ea80
GitHub Actions: [run 44713729286](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/44713729286) — **success**.

### Implementasi

- user/rust/.cargo/config.toml menambahkan konfigurasi khusus target x86_64-unknown-octagram dengan LLD, linker script ELF userspace OCTAGRAM, crt0.o, static relocation, dan no-redzone.
- make -C user/rust build-octagram menjalankan Cargo nightly dengan -Z build-std=core,alloc dan menghasilkan user/rust/build/rust_hello_octagram.elf.
- Kernel menanamkan binary itu sebagai /bin/rust-hello-octagram. Program mengalokasikan Vec<u8> berukuran 2 MiB, menguji stat/open/read/close pada file VFS, dan mencetak marker khusus target_os="octagram".
- octagram-runtime menyediakan ABI memori freestanding memset/memcpy/memmove/memcmp. memmove overlap-safe tanpa mengaktifkan direction flag (DF), sehingga tidak meninggalkan state CPU yang berbahaya ketika thread dipreempt.
- Tes unit runtime memeriksa fill, copy, compare, dan overlap memmove dua arah. CI menjalankan 6 unit test runtime dan semuanya lulus.
- Perbaikan scheduler yang dipertahankan: capture TLS sebelum menukar state task, jangan memulihkan state proses lama ketika syscall mengembalikan kontrol ke trampoline kernel, dan pulihkan TLS/FPU sebelum kembali ke user mode. Trace serial diagnostik sudah dihapus pada commit yang diuji.

### Bukti build dan runtime

Run 44713729286 pada commit 80e823f berhasil melewati:

- validasi target JSON OCTAGRAM;
- kompilasi core dan runtime no_std untuk target;
- kompilasi probe core + alloc yang menggunakan Vec;
- link executable rust_hello_octagram.elf dengan linker script OCTAGRAM;
- 6 unit test runtime memory ABI;
- 373 kernel host tests dan libc host tests;
- build kernel dan bootable ISO;
- QEMU TinyCC/Make smoke test: TCC-SELFHOST-PASS;
- QEMU syscall/thread test: M4-ABI-MMAP-PASS, M4-ABI-TLS-PASS, M4-ABI-THREADS-PASS, M4-RUST-TARGET-EXEC-PASS, RUST-NOSTD-PASS, dan SYSCALL-TEST: PASS.
- Guest Rust mencetak Rust runtime file/stat: PASS dan keluar dengan status 0. Test thread berjalan tanpa trace diagnostik scheduler.

### Status yang boleh diklaim

- **Implemented:** custom target config, linker flags, freestanding memory ABI, embedding guest ELF, dan test coverage terkait.
- **Build-tested:** core, alloc, runtime, dan executable Rust custom target.
- **QEMU-tested:** satu ELF no_std + alloc custom target berjalan, mengalokasikan Vec, mengakses file/stat, dan keluar sukses; thread/TLS/SSE smoke test lulus.
- **Belum tercapai:** rustc native, Cargo native, Rust std, proc-macro/build scripts, native kernel build, native image builder, serta A/B update/rollback. Seluruh build Rust di atas masih dijalankan oleh Cargo/rustc nightly pada host.

M4.1 kini memiliki bukti executable custom target yang berjalan, tetapi M4 Rust native bootstrap belum selesai. Langkah berikutnya adalah mengaudit dependency host rustc/LLVM dan memilih strategi Stage 0 yang realistis; jangan menganggap keberhasilan core + alloc sebagai keberhasilan port std atau compiler native.
