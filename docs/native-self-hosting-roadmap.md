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
- **Tidak** ada bukti bahwa image hasil builder native berhasil di-boot;
  builder hanya host-tested dan belum menghasilkan image berbasis file.
- **Tidak** ada bukti self-update A/B atau rollback berjalan: policy engine dan
  journal host-tested, tetapi belum terhubung ke bootloader atau boot nyata.

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

Batas yang masih ada: arsip `.a` kini diekstrak secara lazy oleh `ld` dan GOT
PC-relative sudah didukung (lihat bagian M2.1), sedangkan PLT dinamis, TLS
relocations, dan COMDAT/group sections belum. QEMU smoke test multi-object
sudah ditambahkan untuk compile dua source terpisah, link dengan `ld`, dan
menjalankan ELF hasilnya.

## 18. Rencana kerja lanjutan

Linker ELF multi-object kini sudah diimplementasikan dan memiliki host tests
untuk resolusi simbol lintas object, definisi ganda, dan simbol undefined.
Tahap berikutnya adalah archive extraction dan perluasan ABI ELF.

### M2 — archive extraction dan relocation coverage

1. [x] Parser ar yang memvalidasi header/member bounds dan hanya menarik member
   object yang menyediakan simbol undefined yang masih dibutuhkan (lihat
   bagian M2.1).
2. Tambahkan test QEMU untuk weak-symbol fallback, alignment, BSS, dan
   relocation overflow.
3. [x] Relokasi GOT PC-relative (`GOTPCREL`, `GOTPCRELX`, `REX_GOTPCRELX`,
   `GOTPC32`) dengan slot writable di segmen data (host tests; lihat M2.1).
4. [ ] Implementasikan PLT dinamis dan TLS (`TPOFF*`) secara terpisah setelah
   kontrak dynamic/static linking ditentukan; PLT32 bukan bukti PLT dinamis
   tersedia.

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
   file yang disediakan image, verifikasi read-back) — **sudah ada** di
   `image_builder.rs` dan `mkimage` (host-tested). Sisa: image berbasis file dan
   boot QEMU/firmware dari image tersebut (lihat M6.1).
3. M7: slot A/B, verifikasi sebelum set boot berikutnya, rollback otomatis bila
   boot gagal — **policy engine dan journal sudah ada** (host-tested, lihat
   M7.1). Sisa: dua partisi root A/B, journal dibaca bootloader, penanda boot
   sukses, uji mati listrik, dan laporan self-hosting terpisah host/QEMU/hardware.


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
- [x] Execute `/usr/bin/gmake --version` successfully in QEMU.
- [x] Parse the TinyCC Makefile with GNU Make `-n` in QEMU.
- [x] Execute GNU Make recipes (`-B`) through native `/bin/sh`, then run
  the generated ELF.
- [x] Add dependency/incremental, phony, order-only, and automatic-variable
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

- [x] automatic variables `$@`, `$<`, `$?`, `$|` melalui dry-run;
- [x] `.PHONY` tetap menjalankan recipe pada dua pemanggilan;
- [x] order-only prerequisite yang lebih baru tidak memaksa rebuild target;
- [x] default target kedua kali tidak menjalankan recipe ketika output up-to-date;
- [x] seluruh tes lama tetap: versi, parser/dry-run, `-B`, TinyCC recipe, dan eksekusi ELF.

Semua tes tersebut sudah dieksekusi pada head branch saat ini oleh CI run
[44895785773](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/44895785773).
Log QEMU memuat marker eksplisit:

```text
GNU-MAKE-OCTAGRAM-PASS: upstream GNU Make 4.4.1 version, dry-run, automatic variables, phony/order-only, incremental build, forced TinyCC build, and guest ELF execution passed
```

Run yang sama juga menyelesaikan syscall compatibility suite dengan
`SYSCALL-TEST: PASS`. Ini menutup matrix runtime M3.2 yang diuji, tetapi
tidak mengklaim kompatibilitas GNU Make penuh: parallel execution, seluruh
fungsi GNU Make, dan semantik shell yang lebih luas masih di luar cakupan.

Bootstrap `/bin/make` tetap dipertahankan sampai pengganti native memiliki
kontrak kompatibilitas tersendiri dan seluruh resep build sistem terbukti
berjalan dengannya. Pekerjaan berikutnya adalah manifest build deterministik
dan pelaporan dependency graph native untuk M3.3.


## 16. M3.3 — Native build manifest dan integritas artefak (21 November 2026)

Branch: feature/native-self-hosting.

### Yang sudah diimplementasikan

- make --manifest <file> menulis manifest tab-separated dengan versi skema.
- Setiap target mencatat status build, mtime Unix, ukuran byte, SHA-256, dan
  daftar prerequisite (termasuk penanda order-only).
- SHA-256 dihitung streaming dari file melalui syscall open/read/close;
  artefak tidak perlu dimuat seluruhnya ke RAM.
- QEMU menguji schema manifest, ukuran executable, dependency graph, dan
  digest file kosong terhadap nilai SHA-256 standar.

### Status verifikasi

- [x] Manifest v2 (status, mtime, ukuran, dependency graph, SHA-256) lulus
  QEMU pada workflow [44902290133](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/44902290133).
- [x] SHA-256 file kosong cocok dengan digest standar.
- [ ] Tambahkan verifikasi file non-kosong multi-block pada workflow berikutnya
  (uji baru ditambahkan pada commit `87ea9ff`).
- Belum diklaim: reproducible build penuh, hash seluruh input compiler,
  validasi dependency graph lintas direktori, dan parallel build.

## M2.1 — Ekstraksi arsip dan relokasi GOT native (21 November 2026)

### Ekstraksi arsip statis

`ld` native menerima campuran object `ET_REL` dan arsip Unix lewat
`toolchain::link_elf_inputs`:

- header ar divalidasi (magic `!<arch>\n`, header 60 byte, batas member,
  padding ganjil); hanya member ELF64 `ET_REL` x86_64 yang diterima, selain itu
  `InvalidElfObject`;
- tabel simbol ar (`/`, `/SYM64/`) diabaikan karena resolusi memakai symtab
  setiap object;
- nama panjang gaya GNU (`//` + `/offset`) dan `#1/<len>` didukung;
- ekstraksi lazy dan berulang: member hanya diambil bila mendefinisikan simbol
  kuat yang belum terselesaikan; setelah itu symbol set diperbarui dan scan
  diulang sampai tidak ada member baru yang cocok, sehingga dependensi berantai
  antar member ikut tertarik;
- batas 256 member per arsip dan 32 object gabungan.

Status: host test `extracts_only_archive_members_needed_by_unresolved_symbols`
dan `rejects_malformed_static_archives` lulus. Jalur guest dicakup
`tools/qemu_tcc_selfhost_test.py` (`LD-ARCHIVE-CREATE/LINK/RUN`); QEMU tidak
tersedia di lingkungan pengembangan ini, jadi hasil workflow QEMU pada head ini
belum dijalankan di sini.

### Relokasi GOT PC-relative

`ld` kini menangani `R_X86_64_GOTPCREL` (9), `GOTPCRELX` (41),
`REX_GOTPCRELX` (42), dan `GOTPC32` (26):

- satu slot GOT 8 byte per simbol unik, dialokasikan di akhir segmen data
  (writable) sebelum BSS; offset BSS dan seluruh alamat simbol ikut digeser
  sehingga simbol data/BSS tetap konsisten;
- kunci dedup: nama untuk simbol global/weak, `(object, indeks simbol)` untuk
  simbol lokal;
- slot diisi alamat akhir simbol; simbol weak tak terdefinisi menjadi 0 dan
  simbol kuat tak terdefinisi tetap `UndefinedSymbol`;
- relokasi menulis `GOT + A - P` sebagai i32 dan melaporkan
  `RelocationOverflow` bila tidak muat;
- relaxation opcode `mov`→`lea` untuk varian `*X` sengaja tidak dilakukan:
  linker mempertahankan indirection GOT, yang tetap benar walau kurang optimal.

Status: host test `relocates_gotpcrel_through_a_writable_got_entry`,
`relaxable_gotpcrel_variants_use_the_same_layout`, dan
`rejects_got_relocations_to_undefined_symbols` lulus bersama seluruh 29 test
`toolchain::`. Belum ada uji guest karena TinyCC non-PIC belum menghasilkan
relokasi GOT, jadi status GOT adalah **host-tested only**.

### Yang masih terbuka

- PLT dinamis, dynamic TLS (`TLSGD`/`TLSLD`/`GOTTPOFF`),
  `R_X86_64_GOT32`/`GOT64` absolut, COMDAT/group sections, dan common symbol.
- Antarmuka `cc`/`as`/`ld` canonical belum disatukan; `cc`/`as` masih toolchain
  mini kernel, sedangkan `tcc` menjalankan `/bin/tcc.elf`.
- Test QEMU untuk weak-symbol fallback, alignment, BSS, dan relocation overflow
  (butir M2 nomor 2) masih terbuka.

## M2.2 — TLS statis di linker native (21 November 2026)

`ld` native kini mengumpulkan section bertanda `SHF_TLS` menjadi satu template
TLS dan menulis program header `PT_TLS`:

- `.tdata` (PROGBITS) masuk ke template file-backed; `.tbss` (NOBITS) hanya
  menambah `memsz`; alignment tiap section dihormati dan alignment terbesar
  dipakai untuk header;
- template ditempatkan di akhir segmen data writable, sehingga runtime dapat
  menyalinnya dari `p_vaddr`; `p_filesz`/`p_memsz`/`p_align` diisi dari ukuran
  template;
- simbol TLS mendapat alamat `tls_vaddr + offset`, dan relokasi
  `R_X86_64_TPOFF32` (23) / `TPOFF64` (18) menulis
  `(S - tls_vaddr) + A - align_up(memsz, align)` — sesuai konvensi
  `user/runtime.c`, yang menaruh thread pointer di akhir blok dan menginisialisasi
  TCB pada `[TP]`;
- initial-exec `R_X86_64_GOTTPOFF` (22) memakai slot GOT terpisah yang berisi
  `S - TP`; slot `GOTTPOFF` tidak pernah dibagi dengan slot `GOTPCREL` untuk
  simbol yang sama karena isinya berbeda, dan simbol non-TLS ditolak;
- simbol di luar rentang template, template kosong, atau hasil yang tidak muat
  i32 untuk `TPOFF32` dilaporkan sebagai error, bukan output rusak.

Status: host test `links_static_tls_template_and_tpoff_relocations` lulus
(3 program header, `PT_TLS` offset/vaddr/filesz/memsz/align, `TPOFF64 = -16`,
slot `GOTTPOFF = -16`, displacement `GOT + A - P`, isi `.tdata`). **Belum ada uji guest**: program hasil `ld` native tidak otomatis
menyertakan `crt0`/runtime TLS bootstrap, sehingga uji QEMU memerlukan program
yang memanggil inisialisasi TLS sendiri. Dynamic TLS dan model initial-exec
(`GOTTPOFF`) tetap ditolak secara eksplisit.

## M6.1 — Native image builder dan verifikasi read-back (21 November 2026)

`kernel/src/image_builder.rs` membangun image bootable dari aset eksplisit,
bukan dari file Live yang kebetulan dimuat Limine:

- `partition::create_octagram_gpt_with_layout` memparameterkan ukuran ESP,
  ukuran disk minimum, dan root minimum. `installer` tetap memakai 512 MiB ESP
  dan batas 16 GiB; builder memakai 80 MiB ESP (di atas minimum 65.525 cluster
  FAT32) dan image minimum 160 MiB.
- Alur: validasi aset (kernel harus `\x7fELF`, loader harus `MZ`, konfigurasi
  tidak kosong) sebelum menulis sektor apa pun; tulis GPT; baca ulang GPT dan
  cocokkan kedua entri; format FAT32 + EXT2; buat direktori; tulis
  `/boot/kernel`, `/EFI/BOOT/BOOTX64.EFI`, `/boot/limine/limine.conf`, marker
  root, dan manifest SHA-256.
- Verifikasi tidak memakai objek filesystem in-memory: kedua partisi di-mount
  ulang dari device, seluruh file dibaca kembali dan dibandingkan byte-per-byte,
  digest kernel dihitung ulang, dan tabel partisi dibaca ulang. Kegagalan
  read-back menghasilkan `ImageError::Verification`.
- Manifest mencatat LBA/ukuran partisi, ukuran dan SHA-256 kernel, loader, serta
  konfigurasi boot; manifest ditulis ke ESP dan root dan digest-nya dilaporkan.
- Perintah shell `mkimage <sataN> MKIMAGE-ERASE [ab]` memakai kernel yang sedang
  berjalan dan `BOOTX64.EFI` preload Limine, menolak root aktif, device yang
  ter-mount, dan disk sumber boot lewat `installer::ensure_safe_format_target`.

Status: 3 host test `image_builder::` lulus (build+verifikasi penuh pada
memory disk 160 MiB, penolakan aset invalid sebelum menulis, penolakan loader
non-PE), bersama test `storage::`. Image berbasis file ditambahkan di M6.2.

Boot dari image hasil builder sudah diverifikasi di QEMU (21 November 2026):
guest membangun image A/B pada disk SATA (`mkimage sata0 MKIMAGE-ERASE ab`),
lalu image itu di-boot lewat firmware UEFI/OVMF. Kernel berjalan dan memilih
`persistent EXT2 root selected from sata0p2` (slot A). Yang **belum** adalah
verifikasi entry boot Limine pada perangkat keras nyata.

## M6.2 — Image berbasis file (21 November 2026)

`kernel/src/storage/file_disk.rs` menyediakan `BlockDevice` di atas file VFS:

- file dibuat kosong lalu tumbuh saat sektor ditulis, sehingga build hanya
  menyentuh sektor yang diperlukan sampai header GPT backup memperluas file ke
  kapasitas penuh;
- pembacaan melewati akhir file mengembalikan nol (semantik sparse image),
  pembacaan parsial diulang sampai buffer penuh atau EOF;
- `FileDisk::create` menolak menimpa file yang sudah ada; `open` memakai panjang
  file sebagai kapasitas dan menolak panjang yang bukan kelipatan 512 byte;
- perintah shell `mkimage file <absolute-path.img> <size-mib>` membuat image
  baru (minimum 160 MiB), memakai kernel/loader Live, dan melaporkan manifest
  SHA-256 setelah verifikasi read-back.

Status: 3 host test `storage::file_disk::tests::` lulus, termasuk build image
penuh 160 MiB ke file RamFs dan pembacaan kembali header GPT. Boot QEMU sudah
dibuktikan untuk image yang dibangun langsung pada disk SATA di dalam guest
(lihat M6.1); jalur `mkimage file` + boot dari file yang disalin keluar belum
dijalankan, dan di guest jalur itu masih dibatasi heap kernel 32 MiB (image
minimum 160 MiB).

## M6.3 — Ukuran kernel ELF dan I/O image builder (21 November 2026)

- `.bss` di keempat linker script sekarang `(NOLOAD)` dan `.got` dipindah ke
  dalam `.data`, sehingga tidak ada lagi bagian berisi berkas setelah `.bss`.
  Sebelumnya linker memadatkan ELF sampai akhir segmen writable: kernel release
  37,8 MB (teks hanya 3,7 MB) dan kernel debug 54,9 MB. Sekarang release 3,9 MB
  dan debug 21,0 MB; kernel release dengan linker baru sudah di-boot di QEMU
  sampai prompt shell.
- Ukuran itu penting untuk M7.2: ESP 80 MiB harus memuat dua salinan kernel,
  dan dua kernel 37,8 MB tidak akan pernah muat.
- FAT32 menyimpan jendela tulis 64 sektor dan menunda FSInfo sampai flush,
  serta driver AHCI memindahkan 64 sektor per perintah DMA. Pada uji guest,
  laju tulis naik dari ~6 KB/detik ke puluhan–ratusan KB/detik; build A/B penuh
  di disk QEMU tetap lambat karena TCG, sehingga verifikasi read-back guest
  masih terbuka.

## M7.1 — A/B update policy engine dan journal (21 November 2026)

`kernel/src/selfupdate.rs` mengimplementasikan aturan keselamatan update:

- dua slot (`A`/`B`) dengan status `Empty`/`Installed`/`KnownGood`/`Failed`,
  digest image, dan flag `verified`;
- image hanya boleh dipasang ke slot **nonaktif**; update kedua ditolak selama
  masih ada `pending`;
- `request_boot_switch` menolak image yang belum diverifikasi, sehingga boot
  berikutnya tidak pernah diarahkan ke image setengah tertulis;
- kandidat mendapat maksimum 3 percobaan boot; setiap percobaan dicatat dan
  setelah habis `rollback_required()` benar dan `boot_slot()` kembali ke slot
  aktif/known-good;
- `confirm_healthy` hanya dipanggil setelah boot sehat dan baru saat itu slot
  kandidat menjadi aktif dan known-good; `rollback` menandai kandidat `Failed`;
- journal tetap 128 byte dengan magic, versi, dan trailer SHA-256 sehingga
  tulisan robek/ korup ditolak (`InvalidJournal`), bukan dipercaya.
- Perintah shell `selfupdate status|init` membaca/menulis journal di
  `/var/lib/octagram/update-journal`;
- image builder menulis journal yang sudah berisi slot A sebagai known-good
  dengan digest kernel, dan verifikasi read-back mem-parse journal itu lagi;
  installer melakukan hal yang sama pada sistem yang baru dipasang.

Status: 7 host test `selfupdate::` lulus (slot aktif, verifikasi wajib, rollback
setelah percobaan habis, promosi known-good, round-trip journal, dan penolakan
korupsi). **Belum** terhubung ke bootloader: Limine belum membaca journal dan
belum ada penanda boot sukses. Layout A/B sudah tersedia di M7.2, tetapi belum
ada uji mati listrik pada tahap kritis. M7 belum selesai.

## M7.2 — Layout A/B dan image dua slot (21 November 2026)

- `partition::create_octagram_gpt_ab_with_layout` menulis GPT dengan ESP lalu
dua partisi root sama besar (dibulatkan ke bawah 1 MiB, dengan gap alignment),
memakai GUID Linux filesystem untuk keduanya dan GUID unik berbeda.
- `image_builder::build_ab_image` memformat FAT32 ESP dan EXT2 untuk kedua root,
menulis salinan kernel ke `/boot/slots/a/kernel` dan `/boot/slots/b/kernel`,
loader ke `/EFI/BOOT/BOOTX64.EFI`, serta `limine.conf` yang dihasilkan dengan
satu entri per slot (`kernel_path` berbeda).
- Tiap root mendapat marker `slot=A`/`slot=B`, manifest image, dan journal A/B
yang menyatakan slot A known-good dengan digest kernel; verifikasi read-back
me-mount ulang ketiga partisi, membandingkan semua file, mem-parse journal, dan
memeriksa tabel partisi berisi tiga entri.
- Perintah `mkimage file <path> <mib> ab` membangun image dua slot berbasis file
(mode tanpa `ab` tetap satu root), dan `mkimage <sataN> MKIMAGE-ERASE ab`
memakai layout yang sama pada disk SATA/AHCI nyata.
- Driver AHCI memindahkan 64 sektor (32 KiB) per perintah DMA dan memberi jeda
antara pembacaan register status, sehingga pembangunan image pada disk tidak
lagi memakan satu perintah SATA per sektor.

Status: host test `builds_an_ab_image_with_both_root_slots` (image builder) dan
`ab_layout_has_two_equal_root_slots` (partition) lulus; seluruh suite kernel
408 test lulus. `tools/qemu_ab_image_boot_test.py` menjalankan rantai penuh di
QEMU: bangun image A/B pada disk SATA di guest, boot image itu lewat UEFI/OVMF,
root persisten dipilih dari `sata0p2`, lalu `selfupdate status` melaporkan
`active=A ... A: known-good B: empty (unverified)` dari journal yang di-seed
image builder.

Pemilihan slot juga sudah diimplementasikan (21 November 2026):

- `render_ab_boot_config(slot)` menghasilkan `default_entry` (1 = A, 2 = B) dan
  setiap entri meneruskan `cmdline: octagram.slot=a|b`; `build_ab_image` memakai
  slot A sebagai default.
- Kernel membaca cmdline executable dari Limine (`ExecutableCmdlineRequest`),
  lalu `discover_persistent_root` memilih partisi root yang penanda
  `/etc/octagram-root`-nya cocok dengan slot yang diminta (fallback ke root
  pertama bila tidak ada yang cocok, dengan pesan peringatan).
- `selfupdate selection <a|b>` menulis ulang `/boot/limine/limine.conf` pada ESP
  disk boot (`image_builder::update_boot_selection`), tanpa mengubah journal.
- Uji QEMU memverifikasi rantainya: setelah `selection b`, boot ulang memakai
  kernel slot B dan log menunjukkan root dipilih dari `sata0p3`.

Yang **belum**: pemilihan otomatis dari journal (switch saat ini masih perintah
manual), penanda boot sukses, uji mati listrik, dan bootstrap berulang. Uji guest QEMU (21 November 2026, disk target 256 MiB di
`/dev/shm`) menjalankan `mkimage sata0 MKIMAGE-ERASE ab`; hasil di disk sudah
diperiksa dari host: GPT berisi tiga entri (ESP 80 MiB di LBA 2048, root A dan
root B masing-masing 178176 sektor), kedua superblok EXT2 valid (`0xef53`, 8192
inode, 22272 blok), dan boot sector ESP adalah FAT32 valid. Build penuh beserta
verifikasi read-back **belum** selesai dalam waktu uji: di TCG setiap perintah
AHCI tetap sangat lambat (puluhan KB/detik), jadi boot dari image hasil builder
masih terbuka. **Belum** ada pemilihan entri Limine berdasarkan journal,
penanda boot sukses, atau uji boot QEMU/firmware; entri pertama (slot A) masih
yang dipilih bootloader.

## M4.2 — `xrustc`: frontend Rust-subset native (21 November 2026)

Status: **QEMU-tested untuk subset kecil; `rustc`, `core`/`alloc` native, dan
Cargo native tetap belum ada.**

Port `rustc` utuh tidak dapat dimulai dari sini: `rustc` membutuhkan Rust `std`
(thread, file, env, panic/unwind), `core`/`alloc` untuk target, dynamic loading
untuk proc-macro, dan sebuah backend LLVM. Semuanya belum ada untuk target
OCTAGRAM, dan binary `rustc` statis berukuran ratusan MB sehingga tidak bisa
dititipkan ke heap kernel 32 MiB. Karena itu M4 dimulai dari ujung pipeline yang
masih bisa dijalankan seluruhnya di dalam OCTAGRAM.

### Yang diimplementasikan

- `kernel/src/xrustc.rs`: lexer, parser rekursif-turun, dan code generator yang
  menghasilkan dialek assembly yang sudah dipakai `crate::toolchain`. Tidak ada
  langkah host: parse → assembly → `assemble` (encoder x86_64 native) → `link`
  (ELF writer native).
- Perintah shell `xrustc [-S] <input.rs> -o <output>`; `-S` hanya menulis
  assembly agar codegen bisa diperiksa, tanpa `-S` ditulis ELF statis.
- Subset yang didukung: `fn` (maksimum 6 parameter), rekursi dan pemanggilan
  maju, `let`/`let mut`, assignment, `return`, `if`/`else if`/`else`, `while`,
  `break`, `+ - *`, perbandingan, `&&`/`||`, `!`, unary `-`, `as`, literal
  string `*const u8`, serta intrinsic `putstr("literal")` dan `putchar(nilai)`
  yang memakai syscall tulis `int $0x80` guest.
- Yang **ditolak dengan nomor baris**, bukan salah-kompilasi: `/` dan `%`
  (backend native belum punya instruksi divide), array, struct, `match`,
  closure, trait, generik, `extern` block, tipe di luar daftar, `break` di luar
  loop, variabel/fungsi tak dikenal, dan jumlah argumen yang salah.
- `user/examples/selfhost/xrustc-sum.rs` (rekursi Fibonacci + loop + keluaran
  karakter) dan `xrustc-bad.rs` (pembagian, harus ditolak) di-seed ke
  `/share/octagram`.

### Bukti

- Host: 416 test lulus, termasuk 7 test `xrustc` — program rekursif berhasil
  dirakit dan di-link menjadi ELF dengan entry `0x400000`, dan semua kasus
  negatif memeriksa nomor baris serta pesannya.
- QEMU (`tools/qemu_xrustc_test.py`, image A/B 256 MiB dari
  `tools/build_xrustc_boot_image.sh`): `XRUSTC-SUBSET-PASS` —
  `xrustc /share/octagram/xrustc-sum.rs -o /tmp/xrustc-sum.elf` menulis 4822
  byte dan entry `0x400000`, `run` mencetak `xrustc fib sum: 88` lalu keluar
  status 0, dan `xrustc-bad.rs` ditolak pada baris 4 tanpa menghasilkan ELF.
- `tools/build_xrustc_boot_image.sh` memakai test host `#[ignore]`
  `writes_a_boot_image_from_real_artifacts` untuk menulis image A/B bootable
  dari kernel release + `limine/BOOTX64.EFI`, sehingga uji QEMU tidak lagi
  bergantung pada penulis ISO.

### Batas yang tidak boleh dilewati

- Ini **bukan** port `rustc` dan **bukan** `cargo`. Tidak ada `core`/`alloc`,
  `std`, borrow checker, type checker, monomorphization, proc-macro, build
  script, registry, atau dependency resolution.
- M5 (kernel Rust dibangun dari dalam OCTAGRAM) tetap belum tercapai: kernel
  memakai `core`, makro, `alloc`, dan fitur bahasa di luar subset ini.
- Catatan M7.2 di atas masih menggambarkan keadaan sebelum pemilihan entri boot
  selesai; status terbaru ada di `ROADMAP.md` dan bagian M7.2 pada commit
  `102282b` (pemilihan slot via `default_entry` + `octagram.slot=`).

## M4.3 — Evaluasi `mrustc` sebagai jalur "Rust asli" (22 November 2026)

Status: **lulus end-to-end untuk PoC** — C hasil transpile `mrustc`
dikompilasi `tcc` di dalam OCTAGRAM dan programnya mencetak
`mrustc-c-poc: 108`. Ini bukan `rustc` native dan belum mengubah status M5;
yang terbukti adalah kelayakan jalur Rust→C→`tcc`, bukan compiler Rust utuh.

Latar: port `rustc` utuh tidak realistis untuk sekarang (butuh `std`, target
`core`/`alloc`, dynamic loading untuk proc-macro, dan backend LLVM; binary
`rustc` ratusan MB sedangkan heap kernel 32 MiB). `mrustc` dipilih sebagai
kandidat karena ia **menghasilkan C** dan karena itu dapat memakai `tcc` yang
sudah ada di dalam OCTAGRAM sebagai backend, tanpa LLVM.

### Yang sudah terbukti (host-tested)

- `mrustc` (HEAD, g++ 13.3, 8 core) dibangun dari source: `bin/mrustc` 10,9 MB
  dalam sekitar satu menit.
- Rust `core` 1.90.0 di-transpile menjadi **34,5 MB / 587.406 baris C** dengan
  `MRUSTC_TARGET_VER=1.90 --edition 2024 --target x86_64-unknown-linux-gnu`;
  mrustc memanggil `gcc` host hanya untuk mengubah C itu menjadi objek.
- Program `no_std` `tools/mrustc_poc/poc.rs` (loop, static array, slice,
  `extern "C" write`, lang item `start`) menjadi **36 KB C** yang
  mendefinisikan `main` standar, yaitu bentuk yang siap di-link `tcc`.
- `tools/mrustc_poc/build_host.sh` mereproduksi seluruh rantai itu dan menulis
  disk EXT2 256 MiB berisi sumber C untuk guest. Penerapan patch source Rust
  kini fail-closed: script membedakan patch yang belum diterapkan, sudah
  diterapkan, dan source archive yang tidak cocok; kegagalan patch tidak lagi
  ditutupi dengan `|| true`. Tahap transpile PoC menghapus `poc.c` lama,
  memeriksa keluaran C segar dan entry point `main`, serta hanya mengizinkan
  status `mrustc` nonzero bila C yang valid secara struktural sudah dihasilkan;
  status nonzero tersebut dicatat sebagai kegagalan link host yang memang
  dialihkan ke linker TinyCC guest. Pemeriksaan struktur C dipisah ke
  `validate_poc_output.py` dan memiliki tes unit untuk output kosong, definisi
  `main`, fungsi non-main, serta komentar yang hanya menyebut `main`.
  Artefak `libcore.c` dan `libcore.hir` juga dibersihkan sebelum transpile dan
  diwajibkan muncul kembali sebelum patcher dijalankan, agar build tidak
  memakai keluaran `core` dari percobaan sebelumnya.
- Reproducibility dan integritas input bootstrap diperketat: `build_host.sh`
  mencetak SHA commit mrustc yang dipakai (`MRUSTC-COMMIT`), menyediakan
  `MRUSTC_REF` untuk memilih branch/tag saat clone, menolak titik patch
  `alloc_system` yang hilang/duplikat/bercampur, memvalidasi arsip Rust sebelum
  ekstraksi, mengekstrak ke staging bersih lalu memindahkannya hanya setelah
  `library/core/src/lib.rs` terverifikasi, dan memeriksa keberadaan patch
  kompatibilitas sebelum dry-run. Builder disk sumber juga menjalankan
  `e2fsck -fn`, mengekstrak kembali sembilan payload EXT2, dan membandingkan
  masing-masing byte-per-byte dengan sumber sebelum image dinyatakan siap.
  Ini memperkuat integritas artefak host, bukan bukti compiler Rust native atau
  hasil QEMU.

### Status hambatan di sisi guest

`tcc` guest sebelumnya berhasil melewati preprocessor dan **227 ribu dari
587 ribu baris** sebelum berhenti. Audit empat hambatan itu kini menghasilkan
status berikut:

1. **Header integer — diperbaiki di source.** `user/include/stdint.h`
   menyediakan tipe `int_leastN_t`/`uint_leastN_t` dan
   `int_fastN_t`/`uint_fastN_t`; pemeriksaan sintaks tersedia.
2. **Libm — bootstrap implementation tersedia.** `user/math.c` sekarang
   menyediakan implementasi untuk 112 simbol double/float yang dideklarasikan
   oleh `user/include/math.h`; link test memastikan seluruh simbol tersedia.
   Objek math dimasukkan ke `libc.a` yang dipakai
   TinyCC. Operasi bit IEEE-754 dasar (sign, abs, trunc, frexp/ldexp,
   nextafter, pembulatan) dibuat lewat manipulasi bit; sqrt memakai x87;
   exp/log/trig/gamma/error-function memakai aproksimasi numerik. Tes host tersedia lewat `make -C user math-header-test math-test`, dan dua
   QEMU harness menguji
   program yang menautkan fungsi dari archive. Ini **belum** correctly-rounded
   atau selengkap/sekokoh libm produksi; error numerik, domain dan rounding
   mode masih perlu diverifikasi lebih luas.
   Regression terbaru juga mencakup signed zero dan transisi `nextafter`
   dari/toward infinity, propagasi NaN oleh `nexttoward[f]`, serta edge case
   float untuk `ilogbf` (termasuk subnormal), `fminf`/`fmaxf`, dan
   `nearbyintf`. Tes `ilogb` double memakai input subnormal eksplisit agar
   tidak keliru menguji nilai finite maksimum.
3. **Overflow builtin — implementasi compiler ditambahkan.** Token
   `__builtin_add_overflow`, `__builtin_sub_overflow`, dan
   `__builtin_mul_overflow` kini dikenali di `tcctok.h` dan diturunkan oleh
   parser `tccgen.c` ke helper `__tcc_overflow`. Helper di
   `user/third_party/tinycc/lib/overflow.c` memeriksa rentang signed/unsigned
   8/16/32/64 bit dan menyimpan bit hasil, termasuk ketika overflow terjadi.
   Tes helper host tersedia; `qemu_test.py` kini lebih dahulu mengompilasi dan
   menjalankan source builtin kecil di guest. Implementasi builtin dibatasi
   integer 8/16/32/64 bit; TinyCC belum memiliki tipe integer sumber 128-bit
   (`VT_QLONG` saat ini hanya dipakai sebagai representasi ABI x86-64, bukan
   tipe aritmetika C yang dapat diparse/dioperasikan).
   **Hasil QEMU belum dijalankan / diverifikasi dalam sesi ini.**
4. **XGETBV assembler — implementasi ditambahkan.** Tabel assembler x86
   TinyCC mengenali mnemonic `xgetbv` dan mengeluarkan encoding tetap
   `0F 01 D0`. Source `user/examples/tcc_xgetbv.c` menggunakan inline asm GCC-style. Dua
   harness QEMU mengompilasi dan menautkan fungsi itu, kemudian menjalankan
   `main()` yang memeriksa keberadaan byte `0F 01 D0` dalam kode fungsi tetapi
   tidak memanggil fungsi XGETBV; instruksi tersebut tidak dieksekusi karena
   OSXSAVE/XCR0 mungkin belum aktif. Hasil guest QEMU
   belum diverifikasi. **Atomic 128-bit** (`uint128_t _Atomic`) masih belum
   didukung TinyCC.
   
Macro lama di `fix.h` yang memaksa ketiga overflow builtin selalu
mengembalikan false sudah dihapus, sehingga PoC memakai implementasi TinyCC.
Stub matematika dari `shim.c` juga sudah dihapus; simbol math harus dipenuhi
oleh `libc.a`. Patcher `patch_core.py` mempertahankan fragmen XGETBV asli
dan hanya me-rewrite AtomicU128 ke helper untuk jalur PoC yang tidak disentuh.
Jika keluaran `mrustc` berubah, harness berhenti alih-alih meneruskan hasil
yang tidak kompatibel. Sebelas tes regresi patcher ada di `test_patch_core.py`, termasuk penolakan fragmen AtomicU128 duplikat/output campuran dan verifikasi bahwa file sumber tetap utuh saat validasi gagal. Patcher menulis ke file temporer, melakukan flush+fsync, lalu mengganti file dengan `os.replace` agar kegagalan penulisan tidak meninggalkan `libcore.c` setengah tertulis.

Harness QEMU `qemu_test.py` juga diperketat pada 22 November: proses QEMU
dibersihkan meskipun koneksi monitor gagal sebelum boot. File OVMF-vars,
log serial, dan socket monitor memakai path unik per proses agar run paralel
tidak saling mengganggu; socket dan file firmware sementara dibersihkan pada
seluruh jalur keluar, sedangkan log serial dan log QEMU host dipertahankan untuk
diagnosis. Output host diarahkan ke log terpisah yang dapat ditentukan melalui
`OCTAGRAM_POC_QEMU_LOG`. Pada setiap kegagalan setelah QEMU dijalankan,
harness otomatis mencetak 5.000 byte terakhir log host, termasuk kegagalan
boot/monitor dan kompilasi; pembacaan tail berbasis byte dengan decoding
replacement agar batas UTF-8 yang terpotong tidak menggagalkan diagnostik.
Pembuatan proses/socket monitor dilindungi cleanup sehingga kegagalan startup
tidak meninggalkan proses yatim; kegagalan menyalin OVMF-vars juga menghapus
file temporernya. Tes unit `test_qemu_test.py` memeriksa tail log pendek,
pembatasan ukuran baca, decoding byte UTF-8 yang tidak lengkap, serta syarat
bahwa compile baru dinyatakan selesai jika status keluar `0` dan prompt shell
kembali. Kompilasi `libcore.c` memakai `tcc -bench` untuk merekam statistik
pada run yang berhasil; penantiannya menggunakan satu deadline gabungan agar
waktu tunggu tidak terduplikasi. Ini perbaikan keandalan/observabilitas
harness, bukan bukti bahwa uji guest telah lulus.

### Yang belum terbukti

- Belum ada `PASS`: run QEMU terakhir terpotong sebelum ELF di-link dan
  dijalankan, jadi belum ada bukti bahwa `tcc` menyelesaikan 587 ribu baris,
  bahwa `ld`/link internal `tcc` menerima simbol `core`, dan bahwa hasilnya
  mencetak `mrustc-c-poc: 108`.
- Waktu kompilasi `tcc` untuk satu TU `core` 34,5 MB sudah sekitar 100–170 detik
  di QEMU/TCG; ini akan jauh lebih berat untuk `std` + `rustc`.
- `mrustc` tidak mendukung crate `#![no_core]` (gagal dengan
  `BUG: src/hir/hir.cpp: ... ::ops::RangeFull`), jadi `core` wajib ikut
  di-transpile; dan injeksi crate implisit `alloc_system` harus dimatikan.

### Perbaikan build libm dan diagnostik kompilasi guest (22 November 2026)

CI run [45001275932](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/45001275932)
gagal pada langkah `Build user programs`, bukan pada QEMU. Penyebabnya regresi
libm di userspace:

- `user/include/math.h` mendeklarasikan `double ilogb(double)`, sedangkan C99
dan implementasi di `user/math.c` memakai `int ilogb(double)`. `gcc -Werror`
menghentikan build dengan `conflicting types for 'ilogb'`. Header, definisi,
dan pointer uji di `user/tests/math_header_test.c` sekarang konsisten memakai
`int`.
- `nextafterf` menyimpan `OctFloatBits other` yang tidak pernah dibaca; variabel
itu dihapus (perilakunya tidak berubah karena keputusan arah tetap memakai
perbandingan `x`/`y`).

Bukti lokal: `make -C user -j8` sukses membangun seluruh program, TinyCC,
libc, dan dynamic userland; `make -C user math-header-test math-test` lulus,
termasuk `ilogb(8.0)`, `ilogb(0.0) == FP_ILOGB0`, `ilogb(NAN)`,dan
`ilogb(INFINITY)`.

Harness QEMU juga tidak lagi hanya mencetak potongan serial mentah saat
kompilasi `libcore.c` gagal. `extract_compile_errors()` mengumpulkan baris
`file:line: error: pesan` dari TinyCC, membuang duplikat, dan membatasinya;
`source_excerpt()` menampilkan baris source yang ditolak dari salinan host di
`$OCTAGRAM_POC_SOURCE_DIR`. Kegagalan tanpa diagnostik dicetak sebagai
`TCC-COMPILE-NO-DIAGNOSTIC` beserta `OCTAGRAM_POC_COMPILE_TIMEOUT`, agar timeout
(100–170 detik untuk satu TU 34,5 MB di TCG) dapat dibedakan dari kesalahan
source. Tujuh tes unit baru di `tools/mrustc_poc/test_qemu_test.py` mencakup
format `file:line`, bentuk tanpa nomor baris, deduplikasi, batas entri, excerpt
di luar jangkauan, dan file yang tidak ada; total 29 tes PoC lulus.

Itu perbaikan build dan observabilitas. Bukti guest end-to-end dicatat pada
bagian berikutnya.

### Perbaikan jalur host dan status QEMU (22 November 2026, lanjutan)

Rantai host M4.3 kini dapat direproduksi ulang dari clean checkout, setelah dua
bug harness diperbaiki:

- `build_host.sh` meneruskan `--extern core="$WORK/libcore.hir"`, sedangkan
  `HIR_Deserialise()` di mrustc menambahkan sendiri akhiran `.hir`
  (`Reader in{ filename + ".hir" }`). Akibatnya mrustc mencari
  `libcore.hir.hir` dan berhenti dengan
  `Unable to deserialise crate metadata ...: Unable to open file`. Argumen
  sekarang memakai `--extern core="$WORK/libcore"` tanpa akhiran.
- `patch_core.py` masih mengasumsikan tepat satu fragmen `AtomicU128`.
  Keluaran mrustc saat ini meng-emit satu fragmen per instansiasi
  monomorphised (15 fragmen), sehingga patcher fail-closed dan build berhenti.
  Patcher sekarang me-rewrite **semua** fragmen, tetap menolak keadaan campuran
  patched+unpatched, dan tetap idempoten untuk file yang sudah dipatch.
  Tes `test_patch_core.py` diperbarui (29 tes PoC lulus).

Bukti lokal (WSL Ubuntu, rustc nightly 1.100, g++ 13):

- `libcore.c` 34.568.514 byte dan `poc.c` 36.616 byte dihasilkan; validator
  menemukan definisi `main`; patcher me-rewrite 15 fragmen AtomicU128.
- `sources.img` EXT2 256 MiB dibuat dan diverifikasi (9 payload byte-per-byte,
  `e2fsck -fn` bersih).
- Image A/B 256 MiB dibuat lewat host test `writes_a_boot_image_from_real_artifacts`
  (`BOOT-IMAGE`), memakai kernel release hasil build lokal.

Status guest saat itu: `BOOT-FAIL: shell prompt never appeared` setelah
`[rootfs] persistent EXT2 root selected from sata0p2`. **Diagnosis awalnya
salah.** Shell memang dijalankan; yang terjadi adalah boot pertama image baru
menyalin userspace embedded ke root EXT2 blok demi blok lewat AHCI emulasi,
sehingga melewati batas 240 detik milik harness. Setelah root terisi, prompt
muncul dalam 10–24 detik. Harness sekarang mencetak `BOOT-PROMPT: <detik>` dan
memakai `OCTAGRAM_POC_BOOT_TIMEOUT` (default 600 detik).

### Bukti end-to-end guest (22 November 2026, lanjutan)

Setelah boot dan `mount sata1 /mnt ext2` berhasil, penulisan ke disk sumber
justru gagal dengan `InvalidData` (`touch`, `mkdir`, dan output `tcc`).
Dua bug nyata ditemukan dan diperbaiki:

- **EXT2 menolak menulis filesystem buatan `mke2fs`.**
  `update_superblock_counter()` memperbarui *setiap* backup superblock dan
  memakai `checked_sub`, sedangkan `e2fsprogs` membiarkan counter pada backup
  superblock bernilai nol (hanya primary yang otoritatif). Pengurangan 1 dari 0
  menjadi `InvalidData`, sehingga alokasi inode/block gagal pada semua image
  `mke2fs`. Sekarang mirror dengan counter nol dilewati; primary tetap
  diperbarui. Regresi dijaga tes
  `writes_when_backup_superblock_counters_are_zeroed`.
- **`log2` tidak eksak untuk pangkat dua.** `log(8.0) * log2(e)` menghasilkan
  `2.9999999999999996`, sehingga `TCC-MATH` guest keluar dengan status 3.
  `log2()` kini memakai `frexp` dan mengembalikan eksponen langsung bila
  mantissa `0.5`; `user/tests/math_runtime_test.c` menuntut `log2(8.0) == 3.0`
  dan `pow(2.0, 10.0) == 1024.0` secara eksak.

Dengan itu seluruh rantai guest lulus:

```
TCC-OVERFLOW-PASS
TCC-XGETBV-ASSEMBLE-PASS / TCC-XGETBV-RUN-PASS
TCC-MATH-PASS
TCC-ATOMIC128-SHIM-PASS
tcc -bench ... shim.c poc.c libcore.c   -> exit 0
# 31401 idents, 588976 lines, 34652630 bytes
# text 3794661, data.rw 67112, data.ro 86664, bss 68344
run /mnt/poc.elf -> mrustc-c-poc: 108 (exit 0)
MRUSTC-C-POC-PASS
```

Kompilasi 34,6 MB C itu memakan waktu beberapa menit di TCG (masih di bawah
`OCTAGRAM_POC_COMPILE_TIMEOUT` 540 detik) dan menghasilkan ELF ±3,9 MB text.
Untuk mencapai link, tiga jenis simbol runtime yang tidak di-emit `mrustc`
harus disediakan oleh lapisan compat PoC:

- `__rdl_alloc`/`__rdl_dealloc`/`__rdl_realloc`/`__rdl_alloc_zeroed`/`__rdl_oom`
  (glue default allocator Rust untuk crate `no_std`; PoC tidak pernah
  mengalokasi, jadi stub berhenti alih-alih mengembalikan pointer palsu),
- trampoline panic `ZRG1cD3bin5panic0g` yang dipanggil `panic_impl` hasil
generate tetapi tidak pernah didefinisikan,
- `__builtin_bswap16/32/64` yang tidak dikenal TinyCC (diganti byte shuffle
  murni di `fix.h`, hanya untuk `__TINYC__` agar GCC host tetap memakai
  builtin aslinya).

Yang **belum** dibuktikan: `mrustc` berjalan di dalam guest (ia C++ dan butuh
C++ runtime; `tcc` hanya C), `std`/`alloc` di luar `core`, dan Cargo native.
PoC ini juga memakai shim bytewise non-atomic untuk `AtomicU128` dan helper
`__rdl_*` yang tidak berfungsi — keduanya tidak boleh dipakai kode nyata.

### Langkah berikutnya

1. Perluas PoC ke `alloc`: sediakan allocator nyata untuk `__rdl_*` (bukan
   stub yang berhenti) lalu transpile crate yang benar-benar mengalokasi.
2. Lanjutkan validasi libm: jalankan tes host dan QEMU, tambah pengujian
   batas/domain/NaN/Inf/rounding, lalu tingkatkan akurasi aproksimasi.
   Header `stdint.h` sudah menyediakan tipe least/fast dan libm bootstrap
   kini berada di `libc.a`.
3. Ukur batas praktis: berapa RAM/waktu `tcc` untuk `core`, lalu putuskan
   apakah `std` layak dicoba atau cukup `core` + kernel subset.
4. Baru setelah itu pertimbangkan memindahkan `mrustc` sendiri ke dalam guest
   (butuh libc/C++ runtime di guest — `tcc` hanya C, jadi ini pekerjaan
   tersendiri yang jauh lebih besar).

## 16. Kemajuan Rust process PAL dan konsistensi build alloc (22 November 2026)

Runtime Rust memperluas primitive proses untuk mendekati kebutuhan std::process:

- PollFd memakai layout C 8-byte (fd:i32, events:i16, revents:i16) dan wrapper poll() memanggil syscall 97. Unit test host memeriksa ukuran, alignment, dan offset field terhadap ABI kernel.
- Command::output() sekarang membuat pipe terpisah untuk stdout dan stderr, mengalihkan kedua descriptor pada child, lalu menguras keduanya secara bergantian menggunakan poll. Membaca dua pipe secara simultan penting untuk menghindari deadlock ketika child menulis lebih banyak dari kapasitas salah satu pipe.
- CapturedOutput berisi status mentah OCTAGRAM, stdout, dan stderr. Smoke test guest menjalankan shell yang menulis marker berbeda ke kedua stream dan memeriksa keduanya serta status exit.
- Adapter ExitStatus awal kini tersedia melalui CapturedOutput::exit_status()/success(); namun signal/process-group semantics dan pemetaan status penuh std::process masih perlu dikerjakan.

**Perbaikan CI:** POSIX Core Stage 1 run [45038538793](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/45038538793) gagal karena langkah compile runtime hanya memakai -Z build-std=core, sementara runtime kini menggunakan alloc::Vec untuk hasil Command::output(). Jalur Makefile dan workflow Rust khusus sudah memakai core,alloc; workflow POSIX disamakan menjadi -Z build-std=core,alloc. Ini memperbaiki konfigurasi build, bukan menandai PAL atau std selesai.

Status validasi setelah perubahan:
- Rust no_std workflow sebelumnya lulus pada run [45038500294](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/45038500294), tetapi itu mendahului penambahan dual-stream capture.
- Perubahan terbaru harus kembali melewati build runtime, unit test host, ELF verification, dan QEMU guest marker Rust runtime Command::output: PASS.
- M4 tetap **belum selesai**. Tidak ada std, native rustc, atau Cargo native yang dinyatakan tersedia.

## 17. ExitStatus dan perbaikan drain multi-stream (22 November 2026)

- Ditambahkan ExitStatus sebagai wrapper kompatibilitas awal di atas nilai wait mentah kernel, dengan raw(), code(), dan success(). Child memiliki wait_status()/try_wait_status(); CapturedOutput menyediakan exit_status()/success(). API raw wait tetap dipertahankan agar tidak mematahkan pemakai sebelumnya.
- Command::output() sekarang menandai descriptor stream yang sudah EOF sebagai fd = -1 pada array poll. Kernel poll_revents mengabaikan fd negatif; tanpa ini POLLHUP dari stream yang sudah selesai dapat membuat poll langsung kembali terus-menerus sementara stream lain masih aktif.
- Unit test host memeriksa nilai status normal, gagal, dan nilai signed. Guest smoke test memeriksa ExitStatus::success() dan code() pada output command.
- Dihapus atribut cfg(test) duplikat yang tidak diperlukan pada deklarasi allocator.

Validasi: Rust no_std workflow sebelumnya lulus pada commit be5dd8c8, tetapi perubahan bagian 17 memerlukan run CI baru. POSIX Core Stage 1 run 45039013607 masih berjalan pada pemeriksaan sebelumnya; hasil QEMU belum dinyatakan lulus.

Status: ExitStatus ini masih adapter khusus OCTAGRAM. Kernel belum menyimpan signal termination / core-dump state seperti Unix wait status, jadi code() tidak boleh dianggap setara penuh dengan std::process::ExitStatus.

## 18. Owning directory iterator untuk fondasi std::fs (22 November 2026)

- Ditambahkan DirectoryIter yang mengambil ownership Directory dan mengimplementasikan Iterator<Item = Result<Dirent, i64>>.
- EOF mengakhiri iterator; error pertama dikembalikan satu kali dan iterator kini mengimplementasikan core::iter::FusedIterator secara eksplisit. Drop pada iterator melepaskan OwnedFd sehingga stream ditutup.
- Guest smoke test kini mengiterasi /share/octagram melalui into_iter(), memvalidasi ukuran record dan terminasi nama, serta menemukan message.c. Ini menguji ownership dan jalur iterator pada guest, bukan hanya pemanggilan readdir manual.
- Matriks PAL diperbarui: primitive iterator tersedia; adapter std::fs::ReadDir, DirEntry/PathBuf, metadata entry, dot-entry behavior, dan semantik perubahan direktori tetap terbuka.

Validasi terkait: Rust no_std workflow run [45039201141](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/45039201141) lulus pada commit ead26cd0 sebelum penambahan DirectoryIter. Perubahan iterator terbaru masih menunggu workflow CI baru.

## 19. Kontrak FusedIterator dan konsistensi dokumentasi PAL (22 November 2026)

- `DirectoryIter` kini secara eksplisit mengimplementasikan `core::iter::FusedIterator`. Ini menyatakan kontrak terminal: setelah EOF atau setelah error pertama dikembalikan, pemanggilan `next()` berikutnya selalu `None`.
- Ditambahkan unit test host yang memverifikasi implementasi trait FusedIterator dan perilaku `next()` berulang pada iterator yang sudah selesai.
- Dokumentasi PAL diperbaiki agar tidak lagi menyebut `CapturedOutput` belum memiliki adapter ExitStatus atau `Directory` belum mempunyai iterator. Keduanya sudah tersedia sebagai adapter awal, tetapi tetap belum setara penuh dengan API std.

Validasi: run Rust no_std sebelumnya lulus untuk commit 4ed89d72 ([45039325350](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/45039325350)). Perubahan FusedIterator pada commit 9d70dd15 dan dokumentasi berikutnya memerlukan CI terbaru. POSIX Core Stage 1 run [45039360299](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/45039360299) masih berjalan saat pemeriksaan terakhir.

## 20. IntoIterator standar untuk Directory (22 November 2026)

- `Directory` kini mengimplementasikan `IntoIterator<Item = Result<DirEntry, i64>>` dengan `DirectoryIter` sebagai associated IntoIter. Setiap `DirEntry` menyimpan duplikat descriptor parent sehingga metadata bisa dibaca setelah iterasi berakhir.
- Guest smoke test diperbarui untuk memakai `for` langsung pada Directory, sehingga menguji dispatch `IntoIterator` standar, ownership stream, EOF, dan jalur error terminal.
- `DirectoryIter` tetap mengimplementasikan `FusedIterator`; status terminal setelah EOF/error dijaga.

Status: primitive iterator dan adapter internal `DirEntry` kini tersedia untuk no_std. Ini belum mengimplementasikan `std::fs::ReadDir`/`std::fs::DirEntry`, `OsString`, atau `PathBuf`.

## 21. Owned DirEntry dan metadata relatif direktori (22 November 2026)

- Ditambahkan `dup(33)` dan `OwnedFd::try_clone()` untuk menggandakan ownership descriptor tanpa berbagi wrapper pemilik.
- `DirectoryIter` kini menghasilkan `DirEntry`, bukan record mentah `Dirent`. Setiap entry membawa descriptor parent duplikat; karena itu `metadata()` tetap valid setelah iterator mencapai EOF dan descriptor stream utamanya ditutup.
- `DirEntry::name_bytes()` mempertahankan nama file sebagai byte mentah (tidak memaksakan UTF-8), `dirent()` menyediakan record ABI, `metadata()` memakai `fstatat(AT_SYMLINK_NOFOLLOW)`, dan `file_type()` memetakan metadata ke `FileType`.
- Ditambahkan unit test host untuk klasifikasi tipe file dan guest smoke test yang menyimpan entry `message.c`, menyelesaikan iterasi, lalu memeriksa metadata dan tipe file dari entry yang masih hidup.

Batasan: ini belum `std::fs::DirEntry`; belum ada `OsString`/`PathBuf`, canonicalization, metadata lengkap POSIX, atau penanganan seluruh semantik symlink/path. Perubahan perlu lulus Rust no_std dan QEMU sebelum dianggap tervalidasi di guest.

## 22. Byte-preserving OS strings dan path primitives (22 November 2026)

- Ditambahkan `OsStr` dan `OsString` internal yang menyimpan byte mentah dan tidak memaksa nama file menjadi UTF-8.
- Ditambahkan `Path` dan `PathBuf` internal dengan `is_absolute`, `file_name`, `extension`, `parent`, `to_path_buf`, serta `push` untuk path absolut/relatif.
- Konversi ke buffer NUL-terminated untuk syscall memvalidasi dan menolak byte NUL (`-EINVAL`) sebelum pemanggilan kernel.
- Unit test host memeriksa nama non-UTF-8, penolakan NUL, parent/root, extension, dan join path. Guest smoke test membangun `/share/octagram/message.c` dengan `PathBuf`, mengubahnya ke C string, lalu menjalankan `stat`.

Batasan: tipe-tipe ini adalah primitive internal no_std, bukan tipe resmi `std::ffi` atau `std::path`; normalisasi `.`/`..`, semantics `OsStr` upstream, dan kompatibilitas penuh `PathBuf` belum ada.

## 23. DirEntry filename dan path (22 November 2026)

- `Directory` kini menyimpan path asal sebagai byte OS string ketika stream dibuka.
- `DirEntry::file_name()` menghasilkan `OsString` milik sendiri; `DirEntry::path()` menghasilkan `PathBuf` dengan menggabungkan path induk dan nama entry.
- Guest smoke test memverifikasi `file_name()` menghasilkan `message.c` dan `path()` menghasilkan `/share/octagram/message.c`, kemudian tetap menguji metadata setelah iterator selesai.

Batasan: `PathBuf` masih helper internal, belum `std::path::PathBuf`; path relative/canonicalization dan edge case separator tetap memerlukan pengujian tambahan.

## 24. Deklarasi Unix target family untuk Rust host (22 November 2026)

- Target JSON `x86_64-unknown-octagram` kini mendeklarasikan
  `target-family = ["unix"]`, sehingga compiler mengeluarkan cfg `unix`
  selain `target_os = "octagram"`.
- Validator `tools/check_octagram_rust_target.py` kini mensyaratkan cfg
  `unix`; workflow Rust khusus menjalankan validator tersebut sebelum build.
- CI Rust no_std run
  [45040711459](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/45040711459)
  lulus. Run tersebut melewati validasi target dan cfg Unix, build binary
  userspace Rust, seluruh unit test runtime host, dan verifikasi ELF statis.

Batasan: `target_family = "unix"` hanya mengaktifkan pemilihan kode berbasis
`cfg(unix)`. Ini bukan klaim bahwa seluruh ABI POSIX telah lengkap, bukan
aktivasi otomatis Rust `std`, dan bukan bukti `rustc`/Cargo berjalan native.
Langkah M4 berikutnya tetap mengintegrasikan PAL upstream Rust dan libc
OCTAGRAM pada checkout Rust yang dipatok, lalu membangun serta menjalankan
program `std` pertama di QEMU.

## 25. Patch awal untuk upstream Rust std build gate (22 November 2026)

- Patch `patches/rust/0001-std-recognize-octagram-target.patch` menambahkan
  `target_os = "octagram"` pada allowlist `library/std/build.rs` upstream
  Rust commit `342d74463af7dd3018ef6aa04dd5500c2deb5bd1`.
- Target JSON OCTAGRAM menetapkan `target_family = ["unix"]`, sehingga
  dispatcher PAL Unix generik dapat dipilih setelah libc target-specific
  tersedia.
- Workflow upstream patch memverifikasi patch dapat diterapkan pada source
  yang dipatok.

Perkembangan selanjutnya: workflow Rust no_std kini juga memodifikasi
`rust-src` secara terkendali untuk target glue dan berhasil menjalankan
`build-std=std` serta menautkan executable probe. Lihat bagian 33.

**Batas:** patch ini bukan registrasi builtin target pada rustc compiler,
bukan native compiler, dan bukan bukti bahwa seluruh API `std` sudah bekerja
di guest. Referensi: [upstream std build.rs](https://github.com/rust-lang/rust/blob/342d74463af7dd3018ef6aa04dd5500c2deb5bd1/library/std/build.rs).

## 26. Audit ABI C untuk port libc upstream Rust (22 November 2026)

- Menambahkan `tools/rust_libc_abi_surface.json` sebagai inventaris terstruktur
  untuk scalar types, record kernel, ABI syscall, translasi libc, dan blocker.
- Menambahkan `tools/check_rust_libc_abi_surface.py` untuk memeriksa bahwa
  definisi `sys/types.h`, `oct_stat`, `dirent`, `timespec`, translasi
  `struct stat`, dan batas `pthread_detach` tidak berubah diam-diam.
- Workflow Rust no_std menjalankan validator tersebut dan memicu ulang CI
  ketika header, runtime C, manifest, atau validator berubah.

Temuan ABI penting: `oct_stat` kernel berukuran 16 byte dan tidak sama dengan
`struct stat` publik; wrapper C melakukan translasi tetapi baru mengisi size,
mode, dan mtime. `pthread_detach` mengembalikan `ENOTSUP`. Data ini menjadi
input untuk modul `libc` upstream yang spesifik OCTAGRAM.

**Batas:** ini audit dan CI guard untuk ABI C yang sudah ada, bukan implementasi
crate upstream `libc`, bukan build `std`, dan bukan native rustc/Cargo.
Langkah berikutnya adalah membuat modul `libc` target-specific dari kebutuhan
PAL Unix yang terukur, lalu menguji kompilasi terhadap checkout upstream
yang dipatok.

## 27. CI untuk validasi patch upstream Rust (22 November 2026)

- Menambahkan workflow `.github/workflows/rust-upstream-patches.yml`.
- Workflow mengambil source Rust pada commit tetap
  `342d74463af7dd3018ef6aa04dd5500c2deb5bd1` dengan sparse checkout untuk
  `library/std/build.rs`.
- Menjalankan `git apply --check`, menerapkan patch di checkout sementara,
  lalu memastikan target `octagram` benar-benar muncul pada allowlist.
- README patch menjelaskan cakupan dan batas tes ini.

**Batas:** workflow ini memverifikasi patch dapat diterapkan pada source yang
dipatok. Ia tidak membangun `std`, tidak membuktikan Unix PAL cocok, dan tidak
menjalankan binary Rust di QEMU. Langkah berikutnya adalah audit simbol PAL
dan ABI yang diperlukan, lalu patch libc target-specific dengan compile test.

## 28. Inventaris API Unix PAL untuk Rust std (22 November 2026)

- Memetakan subset simbol dari source Unix PAL upstream Rust yang dipatok.
- Menambahkan `tools/rust_std_pal_api_inventory.json` dengan status
  `implemented`, `partial`, atau `unsupported`.
- Menambahkan checker yang memverifikasi deklarasi header dan definisi runtime
  C untuk setiap simbol terpilih.
- Menjalankan checker pada workflow Rust no_std saat header/runtime/inventaris
  berubah.

Cakupan awal meliputi filesystem, proses, poll/descriptor, pthread,
clock/sleep, environment, memory mapping, dan beberapa socket API. Audit
mengonfirmasi bahwa simbol tersedia belum tentu berarti semantik lengkap:
`pthread_detach` mengembalikan `ENOTSUP`, ancillary data socket belum
didukung, dan `getrandom` belum memberikan jaminan cryptographic entropy.

**Batas:** ini inventaris subset API dan pemeriksaan source-level; belum
merupakan modul upstream `libc` dan belum membuktikan `std` dapat dikompilasi.
Tahap berikutnya adalah implementasi modul `libc` target-specific serta compile
test bertahap terhadap source upstream yang dipatok.

## 30. Upstream libc OCTAGRAM: applicator deterministik dan compile gate lulus (22 November 2026)

- Modul target khusus berada di `patches/rust-libc/octagram/mod.rs`; applicator
  `tools/apply_rust_libc_octagram.py` memverifikasi upstream commit
  `ccc2ff4aaab962f7fde7b8edda7379c045b1ca12` (release `0.2.191` yang
  dipakai Rust std) dan menerapkan edit melalui anchor unik.
- Modul menyediakan ABI OCTAGRAM sendiri: scalar aliases, record publik,
  konstanta, dan deklarasi pthread/time. Layout utama dikunci dengan
  compile-time assertions serta checker header C.
- CI cross-compiles upstream libc untuk custom target. Ini adalah dependency
  yang tepat untuk versi Rust std yang dipatok, bukan upstream libc main.

## 31. Pemeriksaan layout ABI publik dan validasi header penuh (22 November 2026)

- Modul `patches/rust-libc/octagram/mod.rs` kini mengunci ukuran dan alignment
  struktur `stat`, `dirent`, `pthread_attr_t`, `pthread_mutex_t`,
  `pthread_cond_t`, `sem_t`, `termios`, `sigaction`, `sockaddr`,
  `addrinfo`, `passwd`, `tm`, `fd_set`, dan `lconv` melalui
  compile-time assertions.
- Manifest `tools/rust_libc_abi_surface.json` mencatat public structs
  terpisah dari record kernel `oct_stat`, termasuk ukuran dan alignment.
- Checker ABI diperluas agar memeriksa urutan field pada header C socket,
  passwd/netdb, pthread, terminal, semaphore, select, signal, dan stat.
- CI run `45045939233` lulus validator header, penerapan applicator upstream,
  cross-compile crate `libc`, build Rust no_std, unit test runtime host, dan
  verifikasi ELF statis.

**Batas:** compile-time size checks tidak menggantikan conformance test runtime
atau pembuktian semantik POSIX. Khususnya, `struct stat` publik memuat field
yang sebagian saat ini dinolkan oleh wrapper C; ukuran cocok tidak berarti
semua metadata sudah tersedia. Rust `std` tetap belum dibangun.

## 32. Lengkapi mutex attribute yang dipakai Unix PAL Rust std (22 November 2026)

- Audit source Unix PAL upstream Rust pada commit `342d74463af7dd3018ef6aa04dd5500c2deb5bd1` menemukan bahwa `sync/mutex.rs` memanggil `pthread_mutexattr_settype(..., PTHREAD_MUTEX_NORMAL)` dan menginisialisasi mutex/condvar memakai `PTHREAD_MUTEX_INITIALIZER` serta `PTHREAD_COND_INITIALIZER`.
- Menambahkan `PTHREAD_MUTEX_NORMAL` dan `PTHREAD_MUTEX_DEFAULT` pada header OCTAGRAM. `pthread_mutexattr_settype` kini menerima hanya tipe normal; tipe lain menghasilkan `EINVAL`. Ini cocok dengan implementasi mutex OCTAGRAM yang non-recursive dan tidak membutuhkan field type tambahan pada `pthread_mutexattr_t`.
- Menambahkan deklarasi `pthread_mutexattr_settype` dan initializer statis pada modul upstream libc OCTAGRAM. `thread_demo` kini menguji tipe normal diterima dan nilai tidak valid ditolak.
- Inventaris Unix PAL diperbarui agar API ini tercatat sebagai implemented; ABI `pthread_mutexattr_t` tidak berubah.

<badge color="success">Rust no_std CI lulus</badge> Run [45046390159](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/45046390159) pada commit `7c529470` lulus validasi ABI, kompilasi upstream libc untuk target OCTAGRAM, build Rust no_std, unit test runtime host, dan verifikasi ELF. Build C userspace serta eksekusi `thread_demo` di QEMU masih harus dinilai dari POSIX Core Stage 1 run yang terpisah.

**Batas:** baru tipe mutex normal yang didukung; recursive/error-checking mutex belum tersedia. `pthread_detach` masih `ENOTSUP`, sehingga semantik lifecycle thread Rust std belum lengkap. Ini belum merupakan build `std`.

## 33. Rust std berhasil di-compile dan executable berhasil di-link (23 November 2026)

- Patch `rust-src` melalui `tools/patch_rust_std_octagram.py` mendaftarkan
  modul OS OCTAGRAM, memilih target pada Unix PAL generik, dan memberi
  implementasi eksplisit untuk bagian target-specific yang belum tersedia.
- Probe `user/rust/std-probe` memakai `std::env`, `std::fs`,
  `std::time`, `std::thread`, dan `std::process::Command`.
- CI menjalankan `cargo -Zbuild-std=std check`, lalu mencoba
  `cargo -Zbuild-std=std build` dengan startup `user/crt0.S` dan
  `user/rust/linker.ld`. Langkah link juga menjalankan `readelf -h` dan
  `readelf -l` terhadap executable hasil.
- Run [45088069322](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/45088069322)
  pada commit `7fa968b7` lulus seluruh workflow Rust no_std, termasuk kedua
  probe std, build binary Rust no_std, unit test runtime host, dan verifikasi
  ELF.

<badge color="success">Compile + link probe lulus</badge>

**Belum selesai:** langkah link masih diagnostic/non-blocking; binary std
belum disalin ke ISO atau dieksekusi di QEMU. Semantik filesystem, process,
thread, clock, entropy, panic, dan TLS harus divalidasi saat runtime sebelum
Rust std dinyatakan siap. Rustc native dan Cargo native tetap belum dibuat.

## 34. Integrasi executable upstream std ke image OCTAGRAM (23 November 2026)

- Pipeline POSIX kini mengambil upstream libc pada commit tetap
  `ccc2ff4aaab962f7fde7b8edda7379c045b1ca12`, menerapkan target glue pada
  `rust-src`, dan membangun executable `octagram-std-probe` menggunakan
  `-Zbuild-std=std`.
- Build std diposisikan setelah `make -C user` agar static archive
  `user/build/libc.a` tersedia. Linker memakai script ELF dengan PT_TLS
  (`user/linker.ld`), startup `user/crt0.S`, dan alias archive libc untuk
  memenuhi nama library Unix standar.
- Menambahkan binary hasil link sebagai
  `user/rust/build/rust_std_probe.elf`, menyematkannya di VFS sebagai
  `/bin/rust-std-probe`, dan menambahkan QEMU test untuk environment,
  metadata filesystem, directory iteration, thread/clock, process output,
  dan stdout.
- Menambahkan shim libc untuk `posix_memalign`, `errno_location`,
  `strerror_r`, `chroot`, serta simbol unwinder minimal. Backtrace native
  belum tersedia; shim unwinder saat ini melaporkan end-of-stack.
- POSIX Core Stage 1 run
  [45103601028](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/45103601028)
  telah lulus build host, termasuk compile dan link upstream std, serta
  berhasil membangun ISO. Job QEMU masih berjalan pada saat audit ini ditulis.

**Status:** implemented dan build-tested; QEMU runtime test masih pending.
Jangan menandai Rust std runtime siap sebelum semua marker
`RUST-STD-*-PASS` dan exit status 0 teramati di guest. Rustc native dan Cargo
native tetap belum diimplementasikan.

## 35. Isolasi kegagalan thread upstream Rust std di QEMU (23 November 2026)

- Run POSIX Core Stage 1 [45107032289](https://github.com/Andra-Fauzi/OCTAGRAM-OS/actions/runs/45107032289) lulus seluruh job build host: upstream std executable berhasil dibangun, GNU Make berhasil di-cross-build, dynamic userland, kernel host tests, libc tests, kernel, ISO, dan manifest juga lulus.
- QEMU native TinyCC smoke test lulus. QEMU syscall compatibility test gagal hanya ketika menjalankan /bin/rust-std-probe.
- Probe berhasil melewati environment, filesystem metadata, dan directory iteration. Guest kemudian mengalami user page fault pada TID 2 ketika memasuki jalur thread pertama; marker thread/clock tidak tercapai.
- Fault diagnostic mencatat FS_BASE=0x10025e470, RIP=0x413731, RAX=2, RCX=2, dan akses alamat rendah 0x1. Ini mempersempit area investigasi ke inisialisasi thread/TLS upstream std, tetapi belum cukup untuk menyimpulkan akar masalah.
- Commit a4a2845d15e3156a804b2334ae12f6bfff8a123d menambahkan marker fase sebelum spawn, sesudah spawn kembali, dan pada instruksi pertama closure. Hasil QEMU berikutnya akan membedakan kegagalan TLS/thread-init dari kegagalan closure atau join.

**Status:** build host dan ISO lulus; runtime upstream std thread gagal. Jangan menonaktifkan tes atau menurunkan persyaratan marker. Berikutnya cocokkan disassembly pada RIP dengan TLS model dan implementasi local_pointer / pthread-key pada Rust std. Belum ada native rustc atau Cargo.

## 36. Isolasi model TLS native Rust pada thread baru (23 November 2026)

- Artefak ISO dari run 45108235145 dibongkar dan executable `/bin/rust-std-probe` didisassemble. RIP `0x4137e1` berada di `std::thread::current::set_current`:
  `mov %fs:0x0, %rcx; mov %rax, 0x18(%rcx)`.
- Pada saat fault, `FS_BASE` menunjuk ke alamat TLS yang valid, tetapi word pada `FS:0` bernilai `2` (TID thread), sehingga akses berikutnya mencoba menulis ke `0x1a`. Ini mengidentifikasi jalur native TLS Rust sebagai sumber langsung fault; bukan kegagalan `pthread_create` C biasa, karena tes thread/TLS C tetap lulus.
- Target JSON sebelumnya menyatakan `has-thread-local: true`. Rust std lalu memilih implementasi TLS native dan mengasumsikan TCB layout x86_64 yang belum cocok dengan ABI OCTAGRAM.
- Commit a6041ac0b5bdf25317a103b5d815c2d94fc24022 mengubah `has-thread-local` menjadi `false` agar Rust std menggunakan TLS berbasis POSIX pthread keys, yang sudah disediakan oleh runtime OCTAGRAM. Ini bukan menonaktifkan thread; ini memilih backend TLS yang sesuai dengan kontrak runtime yang saat ini tersedia.
- ELF TLS untuk C runtime tetap dipertahankan; perubahan ini hanya mengubah cfg/model TLS yang dipilih Rust std. CI wajib membuktikan std dapat membangun dan probe QEMU mencapai closure, join, clock, proses, dan I/O.

**Status:** perubahan model TLS Rust sudah di-commit; runtime std thread masih menunggu verifikasi QEMU. Jika probe lulus, lanjutkan menguji destructor TLS, banyak thread serentak, dan cleanup. Belum ada native rustc atau Cargo.

## 37. Perluasan helper path runtime Rust (23 November 2026)

- Menambahkan `Path::strip_prefix` pada helper path berbasis byte untuk
  mendukung pemotongan prefix berdasarkan batas komponen, tanpa mengubah
  nama file non-UTF-8.
- Menambahkan regression test untuk prefix absolut/relatif, prefix yang
  memiliki trailing slash, prefix yang tidak cocok, prefix kosong, dan
  hasil path kosong.
- Memperbaiki semantik prefix/suffix kosong pada `starts_with` dan
  `ends_with`.

**Batas:** helper ini masih merupakan subset byte-preserving di
`octagram-runtime`, bukan implementasi penuh `std::path::Path`; normalisasi
komponen seperti `.` dan `..`, iterator komponen, serta integrasi
filesystem masih belum disediakan. Perubahan source dan test belum dianggap
lulus sampai CI host test dan QEMU runtime yang relevan benar-benar berjalan.

## 38. Iterator komponen path byte-preserving (23 November 2026)

- Menambahkan enum `Component` untuk `RootDir`, `CurDir`, `ParentDir`, dan
  `Normal(OsStr)`.
- Menambahkan `Path::components()` dan iterator `Components` tanpa alokasi
  serta tanpa konversi UTF-8.
- Iterator menggabungkan separator berulang, mengabaikan separator akhir,
  dan mempertahankan komponen `.` serta `..` secara eksplisit.
- Menambahkan regression test untuk root, path relatif, separator berulang,
  komponen khusus, dan nama komponen non-UTF-8.

**Batas:** ini iterator leksikal sederhana, belum menjamin seluruh detail
semantik `std::path::Components` (khususnya normalisasi `.` pada posisi
tertentu dan operasi prefix berbasis komponen). Tes host dan CI harus lulus
sebelum API ini dipakai sebagai dasar implementasi `std::path` yang lebih luas.

## 39. Iterator ancestors path (23 November 2026)

- Menambahkan `Path::ancestors()` beserta iterator `Ancestors` yang berjalan
  dari path awal menuju parent-parent leksikalnya.
- Menambahkan regression test untuk path absolut sampai root, path relatif,
  trailing slash, path kosong, dan root.
- Iterator meminjam slice byte asli sehingga tidak mengalokasikan buffer dan
  tidak memerlukan nama path valid UTF-8.

**Batas:** perilaku `parent()` saat path relatif hanya memiliki satu
komponen masih mengikuti implementasi helper OCTAGRAM saat ini; kompatibilitas
penuh dengan seluruh edge case `std::path::Path` belum diklaim. CI host dan
QEMU tetap perlu dijalankan untuk memvalidasi perubahan.
