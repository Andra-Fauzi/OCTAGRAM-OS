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

- [x] Simpan daftar target build host saat ini dan semua dependency input
  (`docs/building.md`, bagian "Clean build, dependency graph, dan manifest
  artefak": tabel input host, tabel target, dan graph dependensi).
- [x] Pisahkan status build host, QEMU, dan hardware dalam CI: job
  `host-build-and-tests`, job `qemu-smoke-tests` (mengunduh ISO dari artefak),
  dan job `status-summary` yang melaporkan ketiga tingkat status termasuk
  `hardware=not-run`.
- [x] Perbaiki kegagalan QEMU smoke test baseline: lihat bagian 14 (TLS
  context restore, desinkronisasi switch task, deadlock lock scheduler di jalur
  interrupt). Empat boot berturut-turut lulus tanpa fault.
- [x] Tambahkan manifest artefak: `tools/build_manifest.py` menulis
  `build/manifest.json` (kernel, ISO, userspace, target Rust, file boot; ukuran
  dan SHA-256) dan `make manifest-check` memverifikasinya.
- [x] Dokumentasikan perintah clean build dan dependency graph
  (`docs/building.md`).

Gate: clean host build menghasilkan ISO; smoke test mempunyai hasil yang
terdiagnosis; semua artefak mempunyai manifest.

### M1 — Native developer substrate

Tujuan: membuat OCTAGRAM nyaman dan aman untuk menyimpan serta mengedit source.

- [x] Pastikan instalasi ext2 persisten dapat dipakai sebagai root kerja
  (root discovery EXT2/FAT32 yang sudah ada, ditambah uji persistensi reboot
  `tools/qemu_ext2_persistence_test.py`).
- [x] Sediakan direktori standar: `/src`, `/build`, `/toolchain`,
  `/var/cache/build`, `/var/log/build` (dibuat di `fs::init`).
- [x] Implementasikan editor teks terminal native (buat, buka, simpan,
  pencarian literal Ctrl+F/Ctrl+G, navigasi, berkas sampai `MAX_EDITOR_FILE`).
- [x] Pastikan `mkdir`, `stat`, `rename`, `unlink`, `chmod`,
  `getcwd`, environment, dan path panjang berperilaku konsisten; `mv` memakai
  `Vfs::rename` dan EXT2 sekarang mengimplementasikan `rename` untuk berkas,
  symlink, dan direktori dalam parent yang sama.
- [x] Tambahkan shell redirection dan pipeline yang diperlukan build system
  (`cat`/`head`/`tail`/`wc`/`grep`/`echo`, `>` dan `>>`).
- [x] Tambahkan proses spawn/wait dan propagasi exit code yang dapat diandalkan
  (`run`, `runp`, `waitpid`, status recipe native `make`).
- [x] Tambahkan uji read/write source pada ext2 setelah reboot
  (`tools/qemu_ext2_persistence_test.py`).

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
- [x] Tambahkan assembler dan linker yang mampu menangani object ELF yang
  dihasilkan compiler (`toolchain::link_elf_object`: ET_REL x86_64 → ET_EXEC
  dengan segmen teks R|X dan data R|W, relokasi 64/PC32/PLT32/32/32S);
  toolchain mini tetap diberi label terbatas. Verifikasi QEMU menyusul.
- [x] Bangun program multi-file dengan beberapa object dan static library (QEMU-tested melalui TinyCC dan native Make).
- [x] Tambahkan test positif dan negatif untuk compiler/linker (host test
  `links_elf_relocatable_into_two_load_segments` dan
  `rejects_relocatable_objects_without_an_entry_symbol`, di samping test
  assembler/C-subset yang sudah ada).

Gate: proyek C multi-file dibangun sepenuhnya di OCTAGRAM dan hasilnya
dijalankan; tidak ada GCC/binutils host yang dipanggil.

### M3 — Native build orchestrator

Tujuan: build system native yang cukup kuat untuk membangun proyek OS.

- [ ] Kembangkan `make` agar mendukung target, prerequisites, variables,
  automatic variables, pattern/implicit rules yang dibutuhkan, dan include.
- [ ] Tambahkan timestamp/metadata yang andal dan incremental rebuild.
- [x] Tambahkan recipe exit code, quoting, environment, cwd, dan log: setiap
  perintah dijalankan lewat `fork`/`execve` dengan environment induk, `-C`
  mengganti cwd, kegagalan menghentikan build (kecuali prefix `-`), `@`/`-s`
  membisukan echo, dan `;`/`&&` mengatur urutan perintah dalam satu recipe.
- [ ] Tambahkan parallel build hanya setelah dependency graph dan locking aman.
- [x] Implementasikan mode `-n`/`--dry-run`, `-s`/`--silent`, dan `-j1` pada native `make`,
  plus `-p`/`--print-database` untuk mencetak variabel dan rule tanpa build.
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

M4.1 kini memiliki bukti executable custom target yang berjalan, tetapi M4 Rust native bootstrap belum selesai. Langkah berikutnya adalah mengaudit Stage 0 dengan memisahkan build/host/target triple, lalu menentukan cara menyediakan compiler dan sysroot yang benar-benar dapat berjalan di OCTAGRAM. ELF aplikasi yang di-cross-compile untuk target OCTAGRAM bukan compiler native; keberhasilan core + alloc juga bukan bukti port std atau compiler native.

## 13. Audit dependency host Rust dan conformance futex (20 November 2026)

- Matriks host ABI Build/Host/Target tersedia di `docs/rust-host-abi-matrix.md` dan `tools/rust_host_abi_matrix.json`; CI memvalidasi skema serta status klaim.
- Matriks membedakan `qemu_tested`, `build_tested`, `implemented_partial`, dan `not_implemented`. Rust `std`, rustc native, Cargo native, LLVM/LLD native, Stage 0, native kernel build, image builder, dan rollback tetap belum diimplementasikan.
- Audit source menemukan dokumentasi futex lama tidak sinkron: kernel sudah menangani `FUTEX_WAIT_BITSET`, `FUTEX_WAKE_BITSET`, dan `FUTEX_REQUEUE`. Dokumentasi diperbarui; guest tests untuk absolute deadline, bitset filtering, dan requeue-to-target lulus di QEMU run [44716140003](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/44716140003). Batas same-PID/shared-futex serta operasi PI/CMP_REQUEUE tetap dicatat.
- Batas semantik yang dipertahankan eksplisit: futex waiter di-matching dalam PID yang sama, `FUTEX_PRIVATE_FLAG` belum mengubah perilaku, dan `CMP_REQUEUE`/`WAKE_OP`/PI operations belum didukung.

## 14. Audit TLS scheduler dan perbaikan context restore (20 November 2026)

Branch: `feature/native-self-hosting`. Fokus: kegagalan QEMU syscall
compatibility smoke test yang berulang, yaitu `M4-ABI-THREADS-FAIL` pada
`/bin/thread-demo`.

### Bukti kegagalan

Fault selalu berbentuk sama:

```text
User page fault error_code=0x4 rip=0x400c5c address=0x0 fs_base=0x0
bytes=[64, 4c, 8b, 24, 25, 00, 00, 00]      ; mov %fs:0x0,%r12
User fault task pid=1 tid=2 is_thread=true ... process_tls=0x0 actual_fs=0x0
```

RIP tersebut adalah awal `oct_pthread_run_key_destructors`, yaitu kode yang
membaca TCB lewat `%fs:0` setelah fungsi worker selesai. Artinya worker sudah
berjalan normal, lalu **kembali ke user mode dengan `FS_BASE` nol**.

### Yang diperbaiki

1. `terminate_user` hanya memulihkan FPU, tidak memulihkan TLS. Setiap fault
   user yang mengalihkan kendali ke task lain (parent `waitpid` atau task
   berikutnya) membuat task itu berjalan dengan `FS_BASE` milik task yang mati
   (atau nol), sehingga akses `__thread` berikutnya fault. Sekarang TLS dan FPU
   dipulihkan lewat satu helper `restore_user_context()` yang dipakai jalur
   syscall, timer, dan fault.
2. Base TLS dipindahkan menjadi satu record per task di `UserProcess`
   (`thread_pointer`, `tls_anchor`, `tls_cleared`). `tls_anchor` adalah base
   terakhir yang sah; `select_tls_base()` memakainya bila base tersimpan nol
   dan program belum pernah mengosongkan TLS sendiri lewat
   `set_thread_pointer(0)`. `save_current_thread_pointer()` tidak lagi
   membuang base hidup hanya karena satu pembacaan `FS_BASE` bernilai nol.
3. Trace diagnostik sementara (`TLS-FIRST-LOSS`, ring buffer switch) dihapus
   lagi; baris diagnostik fault tetap ada karena berguna.

### Akar masalah kedua: desinkronisasi perpindahan task

Sebelas jalur perpindahan task (yield, timer, `thread_join`, `thread_exit`,
`waitpid`, `block_current_on_wait`, futex, `poll`, fault parent/next, dan loop
resume) masing-masing menyalin state dengan urutan swap sendiri. Kalau satu
jalur mengubah `scheduler.current` tanpa menyimpan state hidup task yang
ditinggalkan, slotnya berisi placeholder dan task berikutnya dijalankan dengan
state kosong (TLS nol) — persis fault di awal dokumen ini.

Sekarang semua jalur memakai satu helper:

- `save_task_state()` menyimpan state hidup `USER_PROCESS` ke slot task yang
  ditinggalkan, dan menolak menimpa slot yang masih memuat state hidup.
- `load_task_state()` hanya memuat slot yang benar-benar state hidup
  (`state.active`) dan mengembalikan frame user task itu.
- `switch_task()` = simpan lalu muat, dipakai seluruh jalur perpindahan;
  `terminate_user_inner` memakai `save_task_state` + `load_task_state` karena
  task yang mati tidak boleh dimuat lagi.
- `keep_running_after_switch_refusal()` melanjutkan task sekarang (dan
  melepas penantian yang sempat dicatat) bila invariant dilanggar, alih-alih
  menjalankan task dengan state kosong.

Pelanggaran invariant dicetak sekali sebagai
`[sched] invariant switch dilanggar di <site>: ...` beserta pid/tid/TLS slot
dan state aktif. Pada seluruh uji di bawah tidak ada pelanggaran yang tercatat,
jadi desinkronisasi yang tampak pada trace lama tidak lagi terjadi.

### Akar masalah ketiga: deadlock lock scheduler di jalur interrupt

Sisa kegagalan intermiten ternyata bukan lagi soal TLS. Monitor QEMU saat
sistem macet menunjukkan CPU berputar di `spin_loop_hint()` dengan `CPL=0` dan
`RFL=0x2` — artinya interrupt mati. `handle_timer_inner()` mengunci
`USER_SCHEDULER` tanpa syarat (untuk `expire_futex_timeouts`) **sebelum**
memeriksa apakah frame yang diinterupsi milik user atau kernel. Bila timer
datang saat task kernel (shell/pemuat program) sedang memegang lock itu,
handler berputar selamanya dengan interrupt mati dan pemegang lock tidak akan
pernah dijadwalkan lagi.

Perbaikan:

1. `handle_timer_inner()` memakai `try_lock()`; bila lock sedang dipegang task
   kernel, tick itu dilewati (tick berikutnya 1 ms kemudian).
2. `restore_current_thread_pointer()` tidak lagi mengunci `USER_SCHEDULER`
   karena hanya membutuhkan `USER_PROCESS`; ini menghapus satu lagi jalur
   deadlock dari konteks interrupt.

### Status verifikasi

- **Build-tested:** kernel `x86_64-unknown-none` (release) dan ISO bootable.
- **Host-tested:** 375 unit test kernel lulus, termasuk
  `task_never_resumes_with_a_lost_tls_base`,
  `user_selector_requires_the_ring3_privilege_bits`, dan
  `a_task_slot_is_resumable_only_while_it_holds_a_live_state`.
- **QEMU-tested:** `tools/qemu_syscall_test.py` lulus **empat boot
  berturut-turut** (exit=0, tanpa `User page fault`, tanpa pelanggaran
  invariant): `SYSCALL-TEST`, `M4-ABI-MMAP`, `M4-ABI-TLS`,
  `M4-ABI-THREADS`, `M4-RUST-TARGET-EXEC`, dan `RUST-NOSTD` semuanya PASS.
- **Uji tekan:** satu boot menjalankan 78 program berurutan
  (`mmap-demo`, `tls-demo`, `thread-demo`, `rust-hello` bergantian) tanpa hang
  dan tanpa fault.

### Cara mereproduksi lokal

Build ISO dan jalankan smoke test seperti CI:

```sh
make -C user && make -C user/rust all && make -C user/rust build-octagram
make -C kernel KARCH=x86_64 && make all
qemu-system-x86_64 -M q35 -cdrom OCTAGRAM-x86_64.iso -boot d -m 2G \
  -display none -serial file:/tmp/serial.log \
  -monitor unix:/tmp/mon.sock,server,nowait -no-reboot -no-shutdown &
python3 tools/qemu_syscall_test.py
```

Bila sistem macet, tanyakan monitor QEMU (`info registers`) untuk melihat RIP
dan flag interrupt; CPU berputar di `spin_loop_hint` dengan `RFL` tanpa bit
`0x200` berarti deadlock spinlock di dalam kernel, bukan program user.

## 15. M0 selesai — kontrak build, manifest artefak, dan status CI terpisah (20 November 2026)

Gate M0: clean host build menghasilkan ISO; smoke test mempunyai hasil yang
terdiagnosis; semua artefak mempunyai manifest. Ketiganya kini terpenuhi.

### Manifest artefak

- `tools/build_manifest.py` memindai artefak build dan menulis
  `build/manifest.json` (di-ignore git): `path`, `kind`, `size`, `sha256` per
  artefak, serta metadata `git` (commit, branch, dirty), `host`, `targets`, dan
  `toolchain` (`rustc`, `cargo`, `cc`, `ld.lld`, `nasm`, `xorriso`, Python).
- Kind yang dicatat: `kernel`, `boot-image` (ISO), `userland` (`user/build/*.elf`),
  `rust-target` (`user/rust/build/*.elf`), dan `boot` (`limine.conf`,
  `limine-bios.sys`, `limine-bios-cd.bin`, `limine-uefi-cd.bin`).
- Integrasi build: `make all` membangun ISO lalu menjalankan target `manifest`;
  `make manifest-check` memverifikasi ulang (gagal bila artefak hilang, berubah
  ukuran/SHA-256, atau ada artefak tak tercatat).
- Bukti lokal: 48 artefak, 116.226.643 byte,
  `MANIFEST-PASS: 48 artefak cocok dengan manifest`. Jalur gagal juga diuji
  dengan manifest yang SHA-256-nya dirusak: `MANIFEST-FAIL: sha256 berubah`.

### Pemisahan status CI

`.github/workflows/posix-core-stage1.yml` sekarang terdiri dari tiga job:

1. `host-build-and-tests` — kompilasi target Rust kustom, unit test host,
   build kernel/ISO, pemeriksaan manifest, lalu mengunggah ISO sebagai artefak.
2. `qemu-smoke-tests` — mengunduh ISO hasil job 1 dan menjalankan dua smoke
   test guest (TinyCC self-host dan syscall/thread compatibility). Kegagalan
   guest test tidak lagi menyembunyikan status build host dan sebaliknya.
3. `status-summary` — melaporkan `build-host`, `qemu`, `hardware` (eksplisit
   `not-run`, karena runner CI tidak punya mesin fisik), dan `manifest`.

### Higiene clean checkout

Symlink yang dulu ikut ter-commit dan menunjuk
`/tmp/octagram-tinycc-upstream/` dihapus: `Makefile` di root (file build root
adalah `GNUmakefile`), serta `tests/Makefile`, `tests/pp/Makefile`, dan
`tests/tests2/Makefile` (sisa suite upstream TinyCC yang tidak dapat dipakai
pada clean checkout). `/iso_root/` ditambahkan ke `.gitignore` karena
merupakan direktori kerja pembuatan ISO.

### Status

M0 selesai. M1 diimplementasikan pada bagian 16 dan masih menunggu verifikasi
QEMU. M2–M7 belum selesai: assembler/linker native lengkap (M2), orchestrator
build native penuh termasuk linking hasil `tcc -c` (M3), `rustc` atau Cargo
native (M4), build kernel dari dalam OCTAGRAM (M5), image builder native (M6),
maupun A/B update/rollback (M7).

## 16. M1 diimplementasikan — substrate developer native (20 November 2026)

Perubahan menyentuh kernel (`fs`, `storage/ext2`, `shell`) dan tool build native.
Semua butir di bawah berstatus source tersedia; verifikasi QEMU dijadwalkan
setelah rangkaian milestone siap, sesuai permintaan untuk menunda pengujian.

### Direktori kerja standar

`fs::init` membuat `/src`, `/build`, `/toolchain`, `/var/cache`,
`/var/cache/build`, `/var/log`, dan `/var/log/build` pada root persisten
(EXT2/FAT32) maupun RAMFS, sehingga alur build native punya lokasi tetap.

### Rename konsisten di EXT2

Sebelumnya `Vfs::rename` hanya berfungsi pada RAMFS karena implementasi default
trait mengembalikan error. `Ext2::rename` kini:

- memindahkan berkas dan symlink antar direktori dengan menambah entry baru lalu
  menghapus entry lama memakai inode yang sama (isi tidak disalin);
- menolak mengganti nama tujuan yang sudah ada (`AlreadyExists`) agar data tidak
  hilang diam-diam;
- hanya mengizinkan direktori dipindah dalam parent yang sama, karena `..` pada
  direktori yang berpindah parent belum ditulis ulang (`Unsupported`);
- melakukan rollback entry tujuan bila penghapusan entry sumber gagal sehingga
  satu inode tidak pernah punya dua nama;
- memperbarui mtime kedua direktori parent.

`mv` di shell memakai jalur yang sama, jadi perilaku RAMFS dan EXT2 konsisten.

### Pipeline dan redirection teks

Shell mengenali `|`, `>`, dan `>>` di luar tanda kutip lalu menjalankannya
sebagai pipeline perintah teks (`cat`, `head`, `tail`, `wc`, `grep`, `echo`).
Stage terakhir dapat menulis ke berkas (`>` truncate, `>>` append). Operator di
dalam kutipan tetap literal, dan perintah di luar daftar ditolak dengan pesan
eksplisit sehingga tidak ada eksekusi setengah jalan.

### Editor terminal

`edit` ditambah pencarian literal (`Ctrl+F`; `Ctrl+G` untuk kemunculan
berikutnya dengan wrap-around), batas berkas dinaikkan ke `MAX_EDITOR_FILE` =
256 KiB, dan baris bantuan menampilkan tombol pencarian.

### Native Make

`user/rust/make` sudah mendukung include, variabel (`=`, `:=`, `?=`, `+=`),
pattern rule satu stem, automatic variables `$@ $< $^ $* $? $|`, `.PHONY`, order-only
prerequisites, `-n`, `-s`, `-j1`, `-B`, `-C`, dan `-f`. `$?` berisi prerequisite
normal yang lebih baru dari target (atau semuanya jika target belum ada), sedangkan
`$|` berisi order-only prerequisites. Test QEMU native Make memeriksa ekspansi
`$@`, `$<`, `$?`, dan `$|`. Recipe mendukung chaining dengan `;` dan `&&`
(berhenti pada kegagalan pertama untuk `&&`); `|`, `<`, `>`, dan backtick tetap
ditolak agar semantik tidak berbeda dari shell.

### Uji persistensi EXT2 lintas reboot

`tools/qemu_ext2_persistence_test.py`: format disk kedua dengan `mkfs.ext2`
native, mount, tulis berkas source, uji pipeline/redirection dan `mv`, unmount,
reset mesin lewat monitor QEMU, lalu mount ulang dan baca kembali isi berkas
yang sama. Skrip ini adalah calon bukti gate M1 ("source kecil dapat diedit,
disimpan di ext2, dibaca ulang setelah reboot").

### Yang masih terbuka

- Belum ada verifikasi QEMU pada commit ini; jalankan
  `tools/qemu_ext2_persistence_test.py` dan `tools/qemu_syscall_test.py` pada
  image hasil build terbaru sebelum menandai M1 QEMU-tested.
- `mv` belum menimpa nama tujuan (perilaku POSIX `rename`), dan direktori belum
  dapat dipindah antar parent.
- Pipeline belum tersedia untuk program ELF, dan input redirection `<` belum ada.

## 17. M2 dimulai — linker ELF64 relocatable native (20 November 2026)

`ld` sebelumnya hanya menerima object format internal (`OCTOBJ1`, satu blok kode
tanpa simbol atau relokasi). Sekarang `ld` mendeteksi magic `\x7fELF` dan
memanggil `toolchain::link_elf_objects` untuk satu atau beberapa ELF64 ET_REL:

- mem-parse ELF64 `ET_REL` x86_64: section header, `SHT_SYMTAB`, string table,
  dan `SHT_RELA`;
- menggabungkan section `SHF_ALLOC` dari beberapa object: section eksekusi ke
  segmen teks `R|X`, data/rodata ke segmen data `R|W`, dan `SHT_NOBITS` (`.bss`)
  ke memori setelah data (`p_memsz` > `p_filesz`);
- menerapkan relokasi `R_X86_64_64`, `PC32`, `PLT32`, `32`, dan `32S` dengan
  pengecekan batas serta overflow 32-bit;
- menyelesaikan simbol global/weak antar-object, memilih definisi global kuat
  di atas weak, serta menolak definisi ganda dan simbol kuat yang belum terdefinisi;
- menolak common symbol, relokasi tak dikenal, input malformed, ukuran berlebih,
  dan object tanpa `_start` alih-alih menghasilkan binary rusak.

Batas yang masih ada: arsip `.a` belum diekstrak oleh `ld`; GOT/PLT/TLS
relocations dan COMDAT/group sections belum didukung. QEMU smoke test multi-object
sudah ditambahkan untuk compile dua source terpisah, link dengan `ld`, dan
menjalankan ELF hasilnya.

## 18. Rencana kerja lanjutan

Linker ELF multi-object kini sudah diimplementasikan dan memiliki host tests
untuk resolusi simbol lintas object, definisi ganda, dan simbol undefined.
Tahap berikutnya adalah archive extraction dan perluasan ABI ELF.

### M2 — archive extraction dan relocation coverage

1. Tambahkan parser ar yang memvalidasi header/member bounds dan hanya menarik
   member object yang menyediakan simbol undefined yang masih dibutuhkan.
2. Tambahkan test QEMU untuk weak-symbol fallback, alignment, BSS, dan
   relocation overflow.
3. Implementasikan relocation GOT/PLT/TLS secara terpisah setelah kontrak
   dynamic/static linking ditentukan; PLT32 bukan bukti PLT dinamis tersedia.

### M3 — manifest build native

1. Setelah build selesai, tulis manifest berisi target, status (dibangun /
   sudah terbaru), ukuran, dan mtime agar hasil build dapat dibandingkan.
2. Hash file memerlukan SHA-256 di userspace; port rutin dari
   `kernel/src/sha256.rs` bila manifest harus tahan terhadap perubahan mtime.
3. Baru setelah dependency graph deterministik, pertimbangkan `-j` paralel
   dengan locking pada output recipe.

### M4 — bootstrap `rustc` native

1. Inventaris komponen: `rustc`, backend, `rust-std`, linker, builtins,
   proc-macro; catat kebutuhan syscall tiap komponen di
   `docs/rust-host-abi-matrix.md`.
2. Pilih Stage 0 yang benar-benar dapat dieksekusi pada ABI OCTAGRAM (bukan
   Linux) dan buktikan dengan executable kecil yang memakai file, mmap, thread,
   dan TLS — bukan hanya kompilasi.
3. Lengkapi API OS yang kurang (dlopen, environment, clock, process group)
   sebelum mencoba membangun `rustc` utuh.
4. Bangun `rustc` native untuk crate `no_std`, lalu `core`/`alloc` target
   OCTAGRAM, lalu Cargo offline.

### M5–M7

1. M5: port langkah build kernel (linker script, symbol, section, entry) ke
   toolchain native dan bandingkan hash/behavior dengan kernel host-built.
2. M6: image builder native (GPT + FAT32 ESP + EXT2 root, tulis bootloader dari
   file yang disediakan image, verifikasi read-back) — `installer.rs` hanya
   menulis ke disk target, belum menghasilkan image bootable.
3. M7: slot A/B, verifikasi sebelum set boot berikutnya, rollback otomatis bila
   boot gagal, dan laporan self-hosting yang memisahkan hasil host, QEMU, dan
   hardware.


## M3.1 — Import GNU Make upstream (21 November 2026)

- [x] Pin GNU Make 4.4.1 as a submodule at `user/third_party/gnu-make`.
- [x] Verify the pinned source and core upstream files in CI.
- [x] Add the OCTAGRAM native shell executable and embed it in the initial VFS.
- [x] Add a reproducible OCTAGRAM cross-build driver and target-owned headers.

## M3.2 — GNU Make cross-build and VFS integration

- [x] Bootstrap upstream Autotools/gnulib and build GNU Make 4.4.1 for
  `x86_64-unknown-octagram` using `-nostdinc`.
- [x] Verify the output is a static x86_64 ELF without a host interpreter or
  dynamic section.
- [x] Embed the result separately at `/usr/bin/gmake`; keep bootstrap
  `/bin/make` unchanged until runtime compatibility is proven.
- [ ] Execute `/usr/bin/gmake --version` successfully in QEMU.
- [ ] Parse the TinyCC Makefile with GNU Make `-n` in QEMU.
- [ ] Execute GNU Make recipes (`-B`) through native `/bin/sh`, then run
  the generated ELF.
- [ ] Add dependency/incremental, phony, order-only, and automatic-variable
  guest tests for upstream GNU Make.
- [ ] Replace or alias `/bin/make` only after the full guest test matrix passes.

### M3.2 verification update — 21 November 2026

The host build job completed successfully in CI run
[44877885636](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/44877885636).
This proves the cross-build, static ELF checks, kernel build, and ISO build.
It does **not** prove guest execution.

The QEMU TinyCC test confirms the existing native TinyCC, ELF linker,
archive handling, and bootstrap `/bin/make` tests still pass. The first
upstream GNU Make invocation, `/usr/bin/gmake --version`, faults before
printing its version:

```text
User page fault error_code=0x4 rip=0x4134ee address=0x41f
bytes=[49, 8b, 3f, 5b, 4d, 89, f8, 41]
```

Therefore GNU Make guest execution remains unchecked. The kernel page-fault trace now includes all saved general-purpose registers
(`r8`–`r15` as well as the original register set). The next diagnostic
the latest guest trace reports `r15=0x41f` at RIP `0x4134ee`, while
`rdi=0`. The faulting instruction bytes begin with `49 8b 3f` (a load
through `r15`). CI now disassembles the GNU Make ELF around that exact RIP
and prints nearby symbols, so the next fix can target the responsible code
path instead of guessing at TLS or the ELF loader. Do not mark M3.2
runtime items complete until the `GNU-MAKE-OCTAGRAM-PASS` marker appears
in QEMU CI. The CI host job now also emits disassembly and nearby symbols
for GNU Make at the observed fault RIP (`0x4134ee`).

## 13. Validasi GNU Make native dan diagnosis syscall (21 November 2026)

Commit `cca31f97694088d56ea3b3df007a5b1688992b24`, CI
[run 44894360524](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/44894360524):

### GNU Make 4.4.1 — QEMU-tested

Langkah `QEMU native TinyCC compile and run smoke test` berhasil dan
mengeluarkan `GNU-MAKE-OCTAGRAM-PASS`. Log membuktikan:

- `/usr/bin/gmake --version` berjalan dan melaporkan GNU Make 4.4.1,
  target `x86_64-unknown-octagram`.
- `gmake -C /share/octagram -n` membaca Makefile dan mencetak empat resep
  tanpa menjalankannya.
- `gmake -B -C /share/octagram` menjalankan resep melalui shell OCTAGRAM,
  memanggil TinyCC, membuat executable ELF, dan executable tersebut berjalan
  serta mencetak `Built inside OCTAGRAM!`.
- Setup timestamp fixture diperbaiki dengan menunggu prompt shell setelah
  builtin `touch`, bukan menunggu pesan exit dari proses eksternal.

Ini membuktikan jalur versi, parser Makefile sederhana, dry-run, recipe
execution, pemanggilan TinyCC, dan eksekusi hasil build. Ini **belum**
membuktikan kompatibilitas penuh GNU Make, parallel build, pattern rules,
include, atau seluruh semantics dependency graph di guest.

### CI keseluruhan — masih gagal pada tes syscall terpisah

Job host build, kernel tests, build kernel, ISO, serta job TinyCC/GNU Make
berhasil. Job `qemu-smoke-tests` kemudian gagal pada langkah
`QEMU syscall compatibility smoke test`, setelah tes GNU Make lulus.
Program syscall compatibility berakhir dengan `SYSCALL-TEST: FAIL(2)`.
Cuplikan log yang dicetak runner hanya memuat bagian akhir output sehingga
dua baris pemeriksaan yang gagal belum teridentifikasi.

Perubahan diagnostik berikutnya pada `tools/qemu_syscall_test.py` akan
mencetak semua baris `FAIL` individual ketika program mengembalikan status
non-zero. Sampai dua kegagalan itu ditemukan dan diperbaiki, status QEMU
keseluruhan tetap **failure**; jangan menandai syscall suite atau keseluruhan
CI sebagai lulus.

### Langkah berikut

1. Jalankan ulang CI dengan diagnostik daftar kegagalan individual.
2. Perbaiki dua assertion syscall yang benar-benar gagal; jangan menurunkan
   assertion atau mengabaikan exit status hanya agar CI hijau.
3. Tambahkan tes khusus GNU Make untuk dependency/incremental, phony,
   order-only prerequisite, automatic variables, dan error handling.
4. Setelah test matrix GNU Make stabil, lanjutkan build orchestrator/manifest
   native dan audit dependency bootstrap rustc. Rust runtime yang berjalan
   sebagai ELF native tetap bukan bukti bahwa `rustc` atau Cargo sudah native.

## 14. M3.2 — GNU Make guest semantics diperluas (21 November 2026)

Validasi sebelumnya pada commit `6c5ab65006ae926a3c2a123779be6df98786c774`
berhasil seluruhnya: host build, kernel/libc tests, ISO, QEMU TinyCC/GNU Make,
dan QEMU syscall compatibility. GitHub Actions
[run 44894990743](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/44894990743)
berstatus **success**.

Pada commit `3b586ec91b91435ea9c883f684d77f93a6af75a3`, guest test GNU Make
4.4.1 diperluas untuk menguji langsung binary upstream `/usr/bin/gmake`:

- [ ] automatic variables `$@`, `$<`, `$?`, `$|` melalui dry-run;
- [ ] `.PHONY` tetap menjalankan recipe pada dua pemanggilan;
- [ ] order-only prerequisite yang lebih baru tidak memaksa rebuild target;
- [ ] default target kedua kali tidak menjalankan recipe ketika output up-to-date;
- [ ] seluruh tes lama tetap: versi, parser/dry-run, `-B`, TinyCC recipe, dan eksekusi ELF.

Item di atas **baru ditambahkan sebagai pengujian** dan belum boleh ditandai
QEMU-tested sebelum workflow pada commit tersebut selesai. Test ini memakai
fixture `/share/octagram` yang sama dan timestamp EXT2 dengan resolusi satu
detik; jeda eksplisit dipertahankan sebelum menyegarkan prerequisite.

Setelah matrix GNU Make ini stabil, langkah selanjutnya bukan langsung
mengklaim GNU Make penuh, melainkan menambahkan manifest build deterministik
dan dependency graph untuk native build orchestrator. M4 tetap memerlukan
jalur compiler Rust native yang terpisah.
