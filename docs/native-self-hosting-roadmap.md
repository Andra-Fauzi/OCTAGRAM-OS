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

## 40. Penyelarasan parent dan iterator Path (23 November 2026)

- Memperbaiki `Path::parent()` agar path relatif non-kosong tanpa separator
  (misalnya `single`) memiliki parent berupa path kosong, sesuai perilaku
  Unix `std::path::Path`.
- Menambahkan `Path::iter()` dengan `PathIter`, yang menghasilkan komponen
  sebagai `OsStr` pinjaman; root dipresentasikan sebagai `/`, sementara `.`
  dan `..` dipertahankan.
- Memperluas regression test untuk parent path tunggal, ancestors relatif,
  komponen root, dan path kosong.

**Validasi:** source dan regression test telah diperbarui. Workflow CI untuk
commit ini belum tersedia; jalankan host tests runtime dan QEMU sebelum
menganggap kompatibilitas API meningkat ini terverifikasi.

## 41. Perbaikan parent pada separator berulang (23 November 2026)

- Memperbaiki `Path::parent()` agar separator berulang sebelum komponen
  terakhir tidak tertinggal pada hasil parent; contoh `foo//bar` menjadi
  `foo`, dan `/foo///bar` menjadi `/foo`.
- Menambahkan regression test untuk bentuk relatif dan absolut tersebut.

**Validasi:** perubahan source dan test sudah di-commit, tetapi belum ada
hasil CI host/QEMU untuk commit ini. Perilaku path tetap bersifat leksikal;
canonicalization filesystem tidak dilakukan.


## 42. Penyelarasan PathBuf::pop dengan parent leksikal (23 November 2026)

- Mengubah `PathBuf::pop()` agar memakai batas parent yang sama dengan
  `Path::parent()`, sehingga separator berulang tidak tertinggal.
- Menambahkan regression test untuk `foo//bar`, `/foo///bar`, dan path
  dengan separator berulang di akhir.
- Root tetap tidak dapat di-pop; path relatif satu komponen tetap berubah
  menjadi path kosong.

**Validasi:** source dan regression test telah di-commit. Host unit test dan
QEMU belum dijalankan untuk perubahan ini; kompatibilitas runtime belum
dinyatakan lulus.


## 43. Kapasitas dan mutasi OsString (23 November 2026)

- Menambahkan `OsString::with_capacity`, `capacity`, `reserve`,
  `shrink_to_fit`, `truncate`, dan `clear`.
- Menambahkan delegasi kapasitas pada `PathBuf` agar buffer path dapat
  disiapkan tanpa konversi UTF-8.
- Menambahkan regression test untuk kapasitas alokasi dan pemotongan byte
  non-UTF-8; API tetap mempertahankan representasi byte Unix.

**Validasi:** source dan regression test telah di-commit. Host unit test,
target build, dan QEMU belum dijalankan pada perubahan ini.


## 44. Konversi UTF-8 dan panjang byte OsString (23 November 2026)

- Menambahkan `OsStr::len()` dan `OsString::len()` dengan semantik panjang
  byte, bukan jumlah karakter Unicode.
- Menambahkan `OsString::into_string()`. Konversi berhasil menghasilkan
  `String` untuk byte UTF-8 valid; jika byte tidak valid, error mengembalikan
  `OsString` asli sehingga nama file non-UTF-8 tidak hilang.
- Regression test mencakup konversi UTF-8 valid, panjang byte, serta
  pemulihan byte invalid secara persis.

**Validasi:** source dan test sudah di-commit. Host unit test, target build,
dan QEMU belum dijalankan pada perubahan ini. Ini memperluas subset API
`OsStr`/`OsString`, bukan berarti kompatibilitas penuh `std::ffi` telah
tercapai. Rustc native dan Cargo native masih belum diimplementasikan.


## 45. Konversi lossy OsStr dan konstruksi OsString dari String (23 November 2026)

- Menambahkan `OsStr::to_string_lossy()` berbasis
  `String::from_utf8_lossy`; byte invalid ditampilkan sebagai karakter
  replacement tanpa mengubah byte sumber.
- Menambahkan `From<String> for OsString` dengan mengambil ownership buffer
  UTF-8 tanpa menyalin ke buffer perantara.
- Regression test memeriksa jalur borrowed untuk UTF-8 valid, replacement
  untuk byte invalid, dan konversi ownership dari `String`.

**Validasi:** source dan test sudah di-commit. Host unit test, target build,
dan QEMU belum dijalankan. Ini adalah perluasan API string freestanding,
bukan implementasi lengkap `std::ffi` ataupun bukti bahwa Rust std runtime
telah stabil.


## 46. Akses UTF-8 dan tampilan lossy untuk Path (23 November 2026)

- Menambahkan `Path::to_str()` dan `Path::to_string_lossy()` dengan
  meneruskan operasi ke representasi `OsStr`.
- Menambahkan accessor serupa pada `PathBuf`, tanpa mengubah byte path
  yang dimiliki buffer.
- Regression test mencakup path UTF-8 valid, path dengan komponen non-UTF-8,
  tampilan replacement character, dan verifikasi byte asli tetap utuh.

**Validasi:** source dan test sudah di-commit. Host tests, target build, dan
QEMU belum dijalankan untuk perubahan ini. API tetap subset leksikal
berbasis byte; belum menggantikan `std::path` upstream secara penuh.


## 47. Penyelesaian jalur konversi OsStr ke OsString (23 November 2026)

- Menambahkan `From<OsStr> for OsString` agar borrowed OS string dapat
  diubah menjadi owned tanpa konversi UTF-8.
- Menambahkan `OsString::to_string_lossy()` sebagai accessor langsung
  untuk tampilan teks; operasi ini tidak mengubah byte yang disimpan.
- Regression test memastikan byte non-UTF-8 tetap sama setelah konversi
  ownership dan setelah pembuatan tampilan lossy.

**Validasi:** source dan test sudah di-commit. CI host, target build, dan
QEMU masih perlu dijalankan. Integrasi Rust std upstream masih menjadi
jalur utama berikutnya; API helper ini bukan pengganti crate
`std::ffi`/ `std::path` yang sesungguhnya.

## 48. Query path kosong pada runtime Rust (23 November 2026)

- Menambahkan `Path::is_empty()` sebagai query tanpa alokasi yang memeriksa
  apakah representasi byte path kosong.
- Menambahkan regression test untuk path kosong, root `/`, path relatif,
  path absolut, dan delegasi dari `PathBuf`.
- API ini mempertahankan model path Unix berbasis byte; tidak melakukan
  normalisasi atau akses filesystem.

**Validasi:** source dan test telah di-commit. Host unit test, target build,
dan QEMU belum dijalankan pada commit ini. Rust std upstream tetap menjadi
target kompatibilitas utama; helper `Path` OCTAGRAM belum menggantikan
`std::path::Path` secara penuh.

## 49. Konversi ownership dan string untuk PathBuf (23 November 2026)

- Menambahkan `From<OsString>` dan `From<OsStr>` untuk membentuk
  `PathBuf` tanpa mengasumsikan path valid UTF-8.
- Menambahkan `From<&str>` dan `From<String>` untuk konstruksi path teks;
  konversi dari `String` memindahkan buffer ke `OsString`.
- Regression test mencakup semua jalur konversi, termasuk byte non-UTF-8
  yang harus tetap identik setelah konstruksi.

**Validasi:** implementasi dan regression test telah di-commit. Tes host,
target build, dan QEMU belum dijalankan pada commit ini. Konversi tersebut
menambah ergonomi helper path, tetapi belum menyamakan seluruh trait dan
semantik `std::path::PathBuf`.

## 50. AsRef byte slice untuk OS string dan path (23 November 2026)

- Mengimplementasikan `AsRef<[u8]>` untuk `OsStr`, `OsString`,
  `Path`, dan `PathBuf`.
- Semua implementasi mengembalikan representasi byte yang sudah dimiliki;
  tidak melakukan alokasi maupun konversi UTF-8.
- Regression test memastikan byte non-UTF-8 tetap identik pada keempat tipe.

**Validasi:** implementasi dan test telah di-commit, tetapi belum dijalankan
oleh host test atau CI target. Ini adalah interoperabilitas byte-slice dasar,
bukan pengganti trait `AsRef<Path>` dan keseluruhan API path dari upstream
Rust std.

## 51. Konversi borrowed Path menjadi PathBuf (23 November 2026)

- Menambahkan `From<Path<'_>> for PathBuf` sebagai jalur konversi ownership
  yang setara dengan helper `Path::to_path_buf()`.
- Regression test memastikan path yang mengandung byte non-UTF-8 disalin
  secara persis dan tidak dipaksa menjadi teks UTF-8.

**Validasi:** source dan test telah di-commit. Host unit test, target build,
dan QEMU belum dijalankan. Konversi ini memperluas helper OCTAGRAM, tetapi
belum menyediakan integrasi penuh dengan tipe `Path` unsized upstream.

## 52. Probe upstream std untuk Unix OS-string dan path byte API (23 November 2026)

- Memperluas `user/rust/std-probe` agar memakai trait upstream
  `std::os::unix::ffi::OsStrExt` dan `OsStringExt`, bukan helper runtime
  OCTAGRAM.
- Probe membangun `OsString` dari byte non-UTF-8, memeriksa
  `as_bytes()`/ `into_vec()`, lalu memastikan byte yang sama dapat diakses
  melalui `PathBuf::as_os_str()`.
- Marker terpisah membedakan kegagalan OS-string FFI dari kegagalan integrasi
  path. Ini menguji apakah API Unix upstream tersedia pada target custom.

**Validasi:** perubahan probe telah di-commit, tetapi belum ada hasil
compile/link atau QEMU untuk commit ini. Tes hanya akan dianggap berhasil
ketika marker `RUST-STD-OS-STRING-FFI-PASS` dan
`RUST-STD-PATH-OS-STRING-FFI-PASS` muncul di guest.

## 53. Perluas probe lexical `std::path` upstream (23 November 2026)

- Probe kini membuat `Path` dari `OsStr::from_bytes` agar input non-UTF-8
  mengikuti kontrak Unix upstream, bukan mengandalkan konversi `&[u8]`
  langsung ke `Path`.
- Memeriksa normalisasi separator berulang dan komponen `.` oleh upstream
  `std::path`, pelestarian `..` secara leksikal, trailing slash, serta nama file.
- Memeriksa `file_name`, `extension`, dan konstruksi `PathBuf` melalui
  operasi `push` berurutan.
- Assertion dipisah dari tes OS-string supaya kegagalan komponen, nama file,
  dan join dapat dibedakan dari marker serial.

**Validasi:** source probe sudah diperbarui. Compile/link upstream std dan
eksekusi QEMU belum terkonfirmasi; bagian ini tetap berstatus implemented,
belum build-tested atau QEMU-tested.

## 54. Uji filesystem upstream dengan nama file non-UTF-8 (23 November 2026)

- Probe membuat nama file sementara di `/tmp` menggunakan byte `0xff` dan
  PID proses agar nama uji tidak bertabrakan dengan proses lain.
- Melakukan round-trip `std::fs::write`, `read`, `metadata`, dan
  `remove_file` menggunakan `PathBuf` yang dibentuk dari `OsStringExt`.
- Memeriksa isi file, tipe metadata, ukuran, dan keberhasilan penghapusan.
- Assertion hasil baca menggunakan pattern matching agar tidak memerlukan
  `PartialEq` pada tipe error I/O.

**Validasi:** implementasi probe telah di-commit. Belum ada hasil build atau
QEMU yang membuktikan filesystem target melewati tes ini. Jika tes gagal,
marker `RUST-STD-FS-BYTE-PATH-FAIL` mengisolasi jalur operasi file berbasis
nama non-UTF-8.

## 55. Uji direktori, rename, dan file descriptor melalui Rust std (23 November 2026)

- Probe membuat direktori unik per PID di `/tmp`, membuat file dengan
  `OpenOptions::create_new`, lalu menguji `write_all`, `sync_all`,
  `SeekFrom::Start`, dan `Read::read_to_end`.
- Direktori kemudian dipindahkan menggunakan `std::fs::rename`; isi file
  diverifikasi lagi melalui path baru sebelum file dan direktori dibersihkan.
- Jalur gagal melakukan cleanup best-effort dan mengeluarkan marker khusus
  untuk membedakan kegagalan operasi direktori dari cleanup.

**Validasi:** probe dan assertion telah diperbarui, tetapi belum ada hasil
compile/link atau eksekusi QEMU. Dukungan `OpenOptions`, `fsync`, `lseek`,
dan rename pada target belum boleh dianggap lulus sampai marker QEMU muncul.

## 56. Uji path management dan descriptor cloning Rust std (23 November 2026)

- Probe file descriptor kini menguji `File::try_clone` dan metadata dari
  descriptor asli maupun hasil clone.
- Probe membuat pohon direktori bertingkat dengan `create_dir_all`, memeriksa
  `canonicalize` menghasilkan path absolut, mengganti working directory
  dengan `set_current_dir`, lalu memverifikasi `current_dir` dan memulihkan
  direktori kerja semula.
- Pohon direktori sementara dibersihkan melalui `remove_dir_all`.
- Marker terpisah membedakan kegagalan create-dir-all, canonicalize,
  current-dir, metadata, dan recursive cleanup.

**Validasi:** source probe dan dokumentasi telah diperbarui. Belum ada hasil
build target atau eksekusi QEMU untuk API tambahan ini. Cargo native tetap
belum tersedia; probe ini hanya memeriksa bagian filesystem/process substrate
yang diperlukan sebelum implementasi Cargo.

## 57. Kaitkan probe filesystem Rust std ke assertion QEMU (23 November 2026)

- `tools/qemu_syscall_test.py` kini menunggu marker keberhasilan eksplisit
  untuk preservasi byte Unix `OsString`/PathBuf, komponen path upstream,
  filesystem path non-UTF-8, operasi direktori/descriptor, serta
  `create_dir_all`/canonicalize/current-directory.
- Marker diperiksa pada proses `/bin/rust-std-probe` yang sudah di-embed ke
  kernel melalui `kernel/src/fs/mod.rs`. Test juga tetap mensyaratkan exit
  status 0 dan shell kembali ke prompt.
- Dengan demikian, keberhasilan tidak lagi hanya bergantung pada marker
  generik `RUST-STD-RUNTIME-PASS`; setiap kelompok API baru menjadi gate CI
  QEMU tersendiri.

**Validasi:** perubahan test dan probe sudah di-commit. Belum ada workflow
result yang dapat diverifikasi untuk commit ini; status QEMU tetap pending
sampai GitHub Actions menjalankan job `qemu-smoke-tests`.

## 58. Uji truncation dan atomic artifact replacement Rust std (23 November 2026)

- Probe membuat file sementara unik, mengubah panjangnya melalui
  `File::set_len(8)`, menjalankan `sync_data`, lalu memeriksa metadata
  descriptor memastikan ukuran file tepat delapan byte.
- Probe atomic replacement membuat output lama dan file sementara baru,
  melakukan `sync_all` pada file sementara, lalu memakai
  `std::fs::rename` untuk mengganti output lama. Isi output baru diverifikasi
  sebelum kedua file dibersihkan.
- QEMU gate kini mewajibkan marker `RUST-STD-FS-TRUNCATE-PASS` dan
  `RUST-STD-FS-ATOMIC-RENAME-PASS`, sehingga regresi pada `ftruncate`,
  data sync, atau semantik rename replacement menghentikan smoke test.

**Validasi:** source probe, QEMU assertion, dan roadmap telah diperbarui.
Belum ada hasil build atau QEMU yang dapat diverifikasi untuk commit ini.


## 59. Uji read_dir bertingkat dan nama entri non-UTF-8 (23 November 2026)

- Probe filesystem kini memeriksa bahwa `std::fs::read_dir("/tmp")` dapat
  menemukan nama file yang mengandung byte `0xff`, lalu membandingkan nama
  melalui `DirEntry::file_name().as_bytes()` tanpa konversi UTF-8.
- Probe direktori bertingkat memeriksa `read_dir`, `file_name`, dan
  `DirEntry::metadata().is_file()` pada file yang baru dibuat.
- QEMU gate mewajibkan marker `RUST-STD-READ-DIR-BYTE-NAME-PASS` dan
  `RUST-STD-READ-DIR-NESTED-PASS`.

**Validasi:** perubahan source, assertion, dan roadmap sudah di-commit.
Build upstream std dan eksekusi QEMU untuk marker baru belum diverifikasi.


## 60. EXT2 rename mengganti artefak file yang sudah ada (23 November 2026)

- `Ext2::rename` kini mendukung penggantian target file reguler, yaitu pola
  yang dipakai Cargo ketika memublikasikan artefak hasil build.
- Inode target lama dipertahankan sampai entri sumber berhasil dipindahkan.
  Jika pemasangan entri baru gagal, implementasi mencoba memulihkan entri
  target lama; inode lama baru dilepas setelah rename berhasil.
- Target direktori dan penggantian tipe node yang belum didukung ditolak
  secara eksplisit. Pemindahan direktori lintas-parent tetap ditolak karena
  pembaruan entri `..` belum diimplementasikan.
- Regression test menguji isi file setelah replacement, persistensi setelah
  remount, serta penolakan target direktori tanpa menghilangkan sumber.

**Validasi:** kode dan regression test sudah di-commit. Unit test kernel dan
QEMU/EXT2 persistence belum dijalankan atau diverifikasi pada sesi ini.


## 61. EXT2 rename untuk dua hard link ke inode yang sama (23 November 2026)

- `Ext2::rename` kini mengembalikan sukses tanpa mengubah directory entry
  ketika nama sumber dan tujuan menunjuk inode yang sama. Ini mengikuti
  semantik POSIX dan mencegah pengurangan link count yang keliru.
- Regression test membuat dua nama yang menunjuk inode yang sama, menjalankan
  rename, lalu memastikan kedua nama dan link count tetap utuh.

**Validasi:** perubahan implementasi dan test sudah di-commit. Eksekusi unit
test kernel serta QEMU masih belum diverifikasi.


## 62. Propagasi kegagalan rollback EXT2 rename (23 November 2026)

- Jalur pemulihan ketika penggantian file gagal kini memeriksa hasil penghapusan
  entry baru dan pemulihan entry tujuan lama.
- Error rollback tidak lagi dibuang diam-diam; bila pemulihan gagal, error
  tersebut dikembalikan kepada pemanggil agar kerusakan parsial tidak tampak
  seperti kegagalan biasa yang berhasil dipulihkan.
- Ini meningkatkan keterlihatan kegagalan, tetapi belum memberikan jaminan
  crash-atomic untuk perubahan directory multi-block.

**Validasi:** commit implementasi dan dokumentasi tersedia; unit test, build
kernel, dan QEMU belum diverifikasi.


## 63. Validasi source untuk same-path EXT2 rename (23 November 2026)

- `rename(path, path)` kini memeriksa bahwa source benar-benar ada sebelum
  mengembalikan sukses tanpa perubahan.
- Path yang tidak ada menghasilkan `FsError::NotFound`, konsisten dengan
  perilaku RamFS dan menghindari sukses palsu.
- Regression test mencakup path yang hilang dan path file yang benar-benar ada.

**Validasi:** kode dan regression test sudah di-commit; eksekusi test/build/QEMU
belum tersedia untuk diverifikasi.


## 64. Uji syscall filesystem relatif terhadap directory descriptor (23 November 2026)

- Upstream Rust std probe kini membuka `/tmp` sebagai directory descriptor,
  lalu memanggil syscall `mkdirat`, `openat`, `renameat`, dan
  `unlinkat` secara langsung dengan nama relatif.
- File yang dibuat melalui `openat` ditulis dan di-`fsync`, kemudian
  dipindahkan melalui `renameat`, dibaca kembali lewat path absolut untuk
  memeriksa isi, dan dihapus dengan `unlinkat`. Direktori sementara juga
  dihapus menggunakan flag `AT_REMOVEDIR`.
- Pengujian langsung ini memisahkan validasi ABI kernel dari wrapper libc/std.
  Marker QEMU khusus memastikan semua operasi relatif terhadap descriptor
  berhasil sebelum probe dianggap lulus.

**Validasi:** source probe, gate QEMU, dan roadmap telah diperbarui.
Compile/link target dan eksekusi QEMU belum dijalankan atau diverifikasi;
dukungan runtime belum boleh dinyatakan lulus sampai marker
`RUST-STD-AT-DIRFD-PASS` muncul dan proses keluar dengan status 0.


## 65. Perbaikan kompilasi unit test runtime Rust (23 November 2026)

- Hasil GitHub Actions dari perubahan probe filesystem menunjukkan dua job
  gagal pada kompilasi unit test `octagram-runtime`: modul test menggunakan
  `String` tetapi tidak mengimpor tipe tersebut dari crate `alloc`.
- Modul test sekarang mengimpor `alloc::string::String`, sehingga pengujian
  host tidak lagi bergantung pada prelude `std` untuk nama tipe tersebut.
- Perubahan ini juga memperbaiki langkah validasi runtime freestanding pada
  workflow POSIX yang memakai target host Linux untuk menjalankan unit test.

**Validasi:** commit perbaikan memicu ulang workflow `Rust no_std userspace`
dan `POSIX Core Stage 1`. Keduanya masih berjalan saat catatan ini ditulis;
hasil akhir belum diklaim lulus.


## 66. Rust std link blocker: POSIX fdopendir (23 November 2026)

The latest POSIX Core Stage 1 run reached the upstream Rust std link step but
failed with an undefined reference to `fdopendir`, pulled in by
`std::sys::fs::unix::remove_dir_impl::remove_dir_all_recursive`. The
cross-check of `core`, `alloc`, and the upstream std probe had progressed
to native linking; this failure was a missing OCTAGRAM libc symbol, not a
Rust compiler diagnostic.

Implemented on `feature/native-self-hosting`:

- Declare `fdopendir(int fd)` in `user/include/dirent.h`.
- Implement it in `user/runtime.c`: reject negative descriptors with
  `EBADF`, validate the descriptor with `fstat`, reject non-directories
  with `ENOTDIR`, allocate the directory stream, and transfer descriptor
  ownership only after successful allocation.
- Preserve the caller's descriptor on validation or allocation failure,
  matching the POSIX ownership rule.
- Extend the Rust/libc ABI validator and manifest with the directory-stream
  symbol and its implementation contract.

Commits:
- `5ca04b6f` — implement `fdopendir`.
- `34734658` — expose the public declaration.
- `c016d5ec` — validate the ABI contract.
- `9545a3b2` — record the directory-stream coverage.

**Validation status:** source changes are committed, but CI and QEMU have not
yet confirmed that the upstream Rust std executable links or that recursive
directory removal works at runtime. The next gate is the complete CI run;
additional undefined libc symbols may appear as more std code paths are
linked. This is incremental libc compatibility work, not a completed Rust
std port and not a native rustc/Cargo implementation.


## 67. Perbaikan import konstanta pada tes rename EXT2 (23 November 2026)

CI POSIX Core Stage 1 pada commit 984e8183 berhasil melewati validasi target Rust, build core/alloc dan runtime Rust, serta build userspace. Kompilasi unit test kernel kemudian berhenti karena tes hard-link EXT2 menggunakan EXT2_FT_REG_FILE tanpa mengimpornya ke modul tests.

Perbaikan:
- Tambahkan EXT2_FT_REG_FILE ke daftar import super di modul tes EXT2.
- Tidak mengubah implementasi rename atau ABI filesystem; ini hanya memperbaiki scope konstanta yang diperlukan oleh regression test.

Commit: 56f4e0f6 — Fix EXT2 hard-link test constant import.

CI juga melaporkan cargo fmt --check masih menemukan perbedaan format pada sejumlah file kernel (termasuk kernel/build.rs dan driver QCA9377). Ini merupakan isu terpisah dari error kompilasi test dan belum dirapikan dalam perubahan ini agar tidak menghasilkan perubahan format massal yang tidak berkaitan.

Validasi: perbaikan import sudah dikomit. Hasil workflow untuk commit perbaikan ini masih harus ditunggu; unit test EXT2 belum dinyatakan lulus.


## 68. Probe rekursif remove_dir_all untuk validasi fdopendir (23 November 2026)

Probe upstream Rust std kini mengisi direktori terdalam dengan file artefak, memverifikasi file tersebut, menjalankan std::fs::remove_dir_all pada pohon direktori bertingkat, lalu memastikan path root benar-benar hilang. Ini memaksa implementasi std filesystem menelusuri isi direktori dan menghapus file sebelum direktori induk; jalur ini menggunakan fdopendir/readdir, bukan hanya membuat dan menghapus direktori kosong.

- Marker baru: RUST-STD-REMOVE-DIR-ALL-RECURSIVE-PASS.
- QEMU runner mewajibkan marker tersebut sebelum menganggap probe filesystem berhasil.
- Perubahan probe dan gate ini juga memicu workflow Rust userspace yang mem-build/link upstream std diagnostic.

Commits: 0def96bd (probe) dan b49f6bcb (QEMU assertion).

Validasi runtime belum diklaim; CI dan QEMU harus menunjukkan marker baru serta exit status 0.


## 69. Uji syscall relatif melalui descriptor direktori bertingkat (23 November 2026)

Probe Rust std untuk ABI `*at` kini tidak lagi membuat file sebagai sibling langsung di `/tmp`. Setelah `mkdirat` membuat subdirektori unik, probe membuka subdirektori tersebut menjadi descriptor kedua. Operasi `openat`, `renameat`, dan `unlinkat` untuk file sumber/tujuan kemudian memakai descriptor subdirektori itu, sementara `unlinkat(AT_REMOVEDIR)` pada descriptor `/tmp` menghapus direktori setelah descriptor anak ditutup.

Perubahan ini memastikan pengujian memverifikasi resolusi nama relatif terhadap descriptor direktori yang benar-benar berbeda dari root `/tmp`. Probe tetap membaca kembali file melalui path absolut untuk memverifikasi lokasi dan isi hasil rename.

Commit: `f899a77a` — Exercise nested directory descriptors in Rust std probe.

**Validasi:** source probe sudah di-commit. Workflow QEMU pada commit sebelumnya masih berjalan; perubahan ini memicu build dan QEMU baru. Belum ada klaim bahwa nested dirfd test berhasil dijalankan.


## 70. Probe symlink upstream Rust std untuk source tree dan build output (23 November 2026)

Probe executable yang ditautkan dengan upstream Rust std kini membuat symlink ke file target di /tmp, memeriksa symlink_metadata().file_type().is_symlink(), membaca tujuan melalui read_link, mengikuti symlink untuk membaca konten, lalu menghapus link dan target secara terpisah. Jalur ini mewakili operasi yang lazim dipakai toolchain pada source tree dan artefak build.

- Marker baru: RUST-STD-FS-SYMLINK-PASS.
- QEMU runner mewajibkan marker tersebut; kegagalan pembuatan, metadata, pembacaan target, dereference, atau penghapusan membuat probe gagal.
- Implementasi libc OCTAGRAM telah menyediakan symlink, symlinkat, readlink, dan readlinkat; probe ini sekarang memeriksa jalur upstream std di atas ABI tersebut.

Commits:
- 60048beb — Exercise upstream Rust std symlink filesystem APIs.
- a14eabdd — Require Rust std symlink filesystem probe in QEMU.

Validasi: workflow CI untuk commit ini harus membuktikan build/link std dan marker QEMU. Belum ada klaim bahwa symlink std telah lulus pada guest. Ini menambah cakupan filesystem untuk Cargo, bukan berarti Cargo native sudah berjalan.


## 71. Probe permission metadata dan set_permissions upstream Rust std (23 November 2026)

Probe upstream std kini membuat file sementara, membaca Unix permission bits melalui PermissionsExt, mengubah mode menjadi 0600 dengan std::fs::set_permissions, lalu membaca metadata kembali dan memverifikasi mode efektif sebelum cleanup. Ini menguji jalur permission yang dibutuhkan build script, instalasi artefak, dan pengelolaan file oleh toolchain.

- Marker baru: RUST-STD-FS-PERMISSIONS-PASS.
- QEMU runner mewajibkan marker ini sehingga dukungan permission tidak hanya lolos kompilasi.
- Runtime libc OCTAGRAM telah menyediakan chmod/fchmod; probe memastikan jalur upstream std berjalan melewati antarmuka tersebut.

Commits:
- 7e65b14d — Exercise upstream Rust std file permission APIs.
- 6ecbe9a1 — Require Rust std file permission probe in QEMU.

Validasi guest masih menunggu workflow CI. Ini merupakan penguatan kompatibilitas std untuk Cargo, bukan klaim Cargo/rustc native sudah tersedia.



## 72. Uji environment dan working directory pada child process Rust std (23 November 2026)

Probe upstream Rust std kini menjalankan `/bin/sh` melalui `std::process::Command`
dengan `Command::env` dan `Command::current_dir`. Child process mencetak
environment variable yang disetel oleh parent serta direktori kerjanya; probe
memverifikasi keduanya dari stdout hasil `Command::output`.

- Marker baru: `RUST-STD-PROCESS-ENV-CWD-PASS`.
- QEMU runner mewajibkan marker tersebut sehingga pengujian memeriksa efek
  konfigurasi pada child, bukan hanya keberhasilan pemanggilan builder API.
- Cakupan ini penting bagi Cargo build scripts yang meneruskan konfigurasi
  melalui environment dan menjalankan perintah dari direktori paket/build.

**Validasi:** source probe, gate QEMU, dan roadmap telah diperbarui.
Build upstream std dan eksekusi guest untuk marker baru masih menunggu CI;
belum ada klaim bahwa pengujian ini sudah lulus pada QEMU.



## 73. Uji mutasi environment melalui upstream Rust std (23 November 2026)

Probe kini menguji `std::env::set_var`, `std::env::var`, dan
`std::env::remove_var` pada proses yang sedang berjalan. Probe memastikan
nilai yang ditulis dapat dibaca kembali dan benar-benar tidak lagi tersedia
setelah dihapus. Mutasi dilakukan setelah tes thread selesai agar tidak ada
thread aplikasi lain yang membaca environment secara bersamaan.

- Marker baru: `RUST-STD-ENV-MUTATION-PASS`.
- QEMU runner mewajibkan marker tersebut.
- Jalur ini memvalidasi implementasi environment POSIX `setenv`/`unsetenv`
  melalui abstraksi upstream std, bukan hanya konfigurasi environment child
  lewat `Command::env`.

**Validasi:** source probe, gate QEMU, dan roadmap telah diperbarui.
Build Rust userspace dan eksekusi QEMU untuk marker ini masih menunggu CI.


## 74. Uji argumen, stderr, dan exit status child process Rust std (23 November 2026)

Probe upstream `std::process::Command` kini memanggil shell dengan argumen
posisional tambahan dan memverifikasi bahwa `$1` serta `$2` diteruskan
dengan urutan yang benar. Child process juga menulis keluaran terpisah ke
stderr dan keluar dengan status non-zero (7).

- Marker baru: `RUST-STD-PROCESS-ARGV-IO-PASS`.
- QEMU runner mewajibkan marker tersebut; tes gagal jika argumen berubah,
  stderr tidak tertangkap, atau status keluar tidak dilaporkan sebagai 7.
- Cakupan ini memvalidasi lebih jauh kontrak `Command::output` yang
  dibutuhkan Cargo untuk menjalankan build script dan helper compiler.

**Validasi:** probe, gate QEMU, dan roadmap telah diperbarui. Hasil QEMU
belum diklaim lulus sampai marker baru muncul dan proses probe berakhir
dengan status 0.

## 75. Makro status proses POSIX (23 November 2026)

Header `sys/wait.h` kini mendekode format status wait POSIX untuk exit normal,
terminasi sinyal, core dump, proses berhenti, dan status continued. Sebelumnya
beberapa makro selalu mengembalikan nilai konstan sehingga program dapat
salah mengklasifikasikan hasil child process.

- Tes host baru: `user/tests/wait_status_test.c`.
- Target Make: `make -C user wait-status-test`.
- POSIX Core Stage 1 menjalankan tes tersebut pada job host.

**Batasan kernel yang masih terbuka:** jalur wait saat ini mengirim kode exit
normal sebagai status POSIX, tetapi terminasi akibat sinyal masih disimpan
sebagai nilai `128 + signal`. Karena itu, makro header sudah mengikuti
encoding POSIX, tetapi klasifikasi sinyal belum dapat dianggap terintegrasi
penuh sampai kernel menyimpan status terminasi bertipe secara terpisah.

## 76. Encoding wait status untuk terminasi sinyal (23 November 2026)

Kernel kini membedakan status wait untuk exit normal dan terminasi akibat
sinyal. Exit normal disimpan sebagai `(code & 0xff) << 8`, sedangkan
terminasi sinyal menyimpan nomor sinyal pada tujuh bit rendah. Syscall
`waitpid` mengirim encoding tersebut apa adanya; libc tidak lagi mengubah
setiap hasil menjadi exit normal.

Probe upstream Rust std mengeksekusi child shell yang mengirim SIGTERM ke
dirinya sendiri dan memeriksa `ExitStatusExt::signal() == Some(15)` serta
`code() == None`. QEMU gate mewajibkan marker
`RUST-STD-PROCESS-SIGNAL-STATUS-PASS`.

**Validasi:** perubahan kode dan gate sudah ditambahkan. Pengujian QEMU harus
lulus sebelum kompatibilitas terminasi sinyal dianggap terverifikasi.

## 77. Probe Child::try_wait, kill, dan wait pada Rust std (23 November 2026)

Probe upstream Rust std kini membuat child shell yang tetap berjalan, lalu memanggil
`Child::try_wait()`. Hasil yang diharapkan sebelum child dihentikan adalah
`None`, yang memvalidasi jalur non-blocking `waitpid(WNOHANG)`. Probe kemudian
memanggil `Child::kill()`, menunggu dengan `Child::wait()`, dan memastikan
`ExitStatusExt::signal()` melaporkan SIGKILL (9), bukan exit code sintetis.

- Marker QEMU wajib: `RUST-STD-PROCESS-TRY-WAIT-KILL-PASS`.
- Probe SIGTERM sebelumnya juga diperbaiki agar shell menjalankan
  `kill -TERM $$`, sehingga sinyal benar-benar dikirim ke proses child sendiri.
- Kernel `syscall_waitpid` sudah memiliki cabang `WNOHANG` yang mengembalikan
  nol ketika child belum selesai; tes upstream std sekarang menguji kontrak itu
  dari API `Child::try_wait`.

**Validasi:** perubahan source dan gate QEMU memicu workflow baru. Hasil runtime
belum dinyatakan lulus sampai marker terlihat pada output guest.

## 78. Validasi status Child yang sudah di-reap (23 November 2026)

Setelah child dihentikan dengan SIGKILL dan `Child::wait()` berhasil, probe
memanggil `wait()` sekali lagi dan `try_wait()`. Keduanya harus mengembalikan
status yang sama dari cache objek `Child`, bukan mencoba melakukan reap kedua
pada kernel atau menghasilkan `ECHILD`.

- Marker QEMU wajib: `RUST-STD-PROCESS-WAIT-CACHE-PASS`.
- Pemeriksaan memastikan status tetap menunjukkan SIGKILL (9).
- Ini melengkapi uji `spawn`, `try_wait`, `kill`, dan `wait` dengan
  perilaku pemanggilan berulang yang dijanjikan API Rust std.

**Validasi:** perubahan source, gate QEMU, dan roadmap telah diperbarui.
Runtime tetap menunggu hasil job QEMU.


## 79. Validasi PID child dan perbaikan probe SIGTERM (23 November 2026)

Probe proses Rust std diperbaiki agar child yang dikirim SIGTERM benar-benar
menjalankan `kill -TERM $$` dan mengirim sinyal kepada dirinya sendiri.
Perintah `kill -TERM $` sebelumnya tidak memiliki ekspansi PID yang valid,
sehingga tidak dapat membuktikan encoding wait status untuk terminasi sinyal.

Probe lifecycle juga memeriksa `Child::id()` segera setelah `spawn()` dan
mewajibkan PID child bukan nol sebelum `try_wait`, `kill`, dan `wait`.
Marker QEMU wajib baru: `RUST-STD-PROCESS-CHILD-ID-PASS`.

**Validasi:** source probe dan gate QEMU diperbarui. Build host pada run
sebelumnya telah menyelesaikan kompilasi target Rust std; hasil runtime QEMU
untuk revisi ini masih menunggu workflow baru.


## 80. Perbaikan pencarian executable execvp untuk PATH (23 November 2026)

Implementasi libc `execvp` sebelumnya menjalankan `continue` ketika entri
PATH yang ditemukan ternyata bukan file reguler. Karena `continue` melewati
bagian loop yang memajukan komponen PATH, direktori bernama sama dengan
executable dapat membuat proses macet selamanya.

- Cabang non-regular kini mencatat `EACCES` lalu melanjutkan ke komponen PATH
  berikutnya secara normal.
- `execve_demo` kini membuat direktori `/tmp/execve-demo`, menetapkan
  `PATH=/tmp:/bin`, lalu menjalankan `execvp("execve-demo", ...)`.
- Probe hanya berhasil bila pencarian melewati direktori yang tidak dapat
  dieksekusi sebagai program dan menemukan executable di `/bin`.

**Validasi:** source libc, demo, gate QEMU, dan roadmap diperbarui. Tes runtime
guest masih harus dijalankan; perubahan belum dianggap lulus QEMU.


## 81. Rust std Command executable lookup melalui PATH (23 November 2026)

Probe upstream Rust std kini menjalankan `Command::new("sh")` tanpa path
absolut dan memberikan `PATH=/bin` pada child. Program harus ditemukan,
berjalan, dan menghasilkan marker dari stdout child.

- Marker QEMU wajib: `RUST-STD-PROCESS-PATH-LOOKUP-PASS`.
- Pengujian ini memvalidasi jalur yang dipakai Cargo ketika meluncurkan
  helper seperti compiler, linker, dan build script melalui nama executable.
- Tes ini melengkapi regresi libc `execvp` yang melewati entri PATH berupa
  direktori dan melanjutkan pencarian ke direktori berikutnya.

**Validasi:** probe, gate QEMU, dan roadmap diperbarui. Hasil runtime guest
masih menunggu workflow.


## 82. execvp: bedakan PATH kosong dan PATH yang tidak disetel (23 November 2026)

Implementasi `execvp` sebelumnya menggunakan fallback direktori standar
ketika `PATH` bernilai string kosong. Ini keliru: PATH yang tidak disetel
dapat memakai fallback implementasi, tetapi komponen kosong pada PATH
secara POSIX merepresentasikan direktori kerja saat ini.

- Fallback `/bin:/usr/bin:/usr/local/bin` sekarang hanya dipakai jika
  `getenv("PATH")` menghasilkan NULL.
- PATH kosong tetap diproses sebagai satu komponen kosong sehingga kandidat
  executable dicari relatif terhadap current working directory.
- `execve_demo` kini menguji dua perilaku berurutan: melewati direktori
  bernama sama pada `PATH=/tmp:/bin`, lalu berpindah ke `/bin` dan
  menjalankan executable dengan `PATH=""`.
- Marker guest tetap `EXECVP-PATH-SEARCH-PASS`, tetapi kini baru dicetak
  setelah kedua skenario berhasil.

**Validasi:** perubahan source, demo, dan roadmap telah di-commit secara
terpisah. Hasil host/QEMU untuk commit ini masih menunggu workflow CI;
perubahan belum diklaim lulus runtime.


## 83. posix_spawnp: pencarian executable melalui PATH kosong (23 November 2026)

Probe `posix_spawn_demo` kini menguji jalur `posix_spawnp`, bukan hanya
`execvp`. Program menyimpan direktori kerja awal, berpindah ke `/bin`,
mengatur `PATH=""`, lalu meluncurkan child dengan nama executable tanpa
slash (`posix_spawn_demo`).

Child memvalidasi environment yang diteruskan dan parent menunggu status
keluar sukses. Tes ini memastikan komponen PATH kosong diperlakukan sebagai
direktori kerja saat ini dalam pencarian executable `posix_spawnp`, yang
merupakan bagian penting dari kompatibilitas proses untuk Cargo.

**Validasi:** tes ditambahkan ke demo yang sudah dibangun dan dijalankan oleh
guest QEMU. Hasil runtime untuk revisi ini menunggu workflow CI; belum
diklaim lulus.


## 84. Rust std filesystem artifact copy (23 November 2026)

Probe upstream Rust std kini menguji `std::fs::copy`, API yang relevan untuk
menyalin artefak build dan output build script.

- Probe menulis payload sumber, menyalinnya ke file tujuan, memeriksa jumlah
  byte yang dilaporkan, lalu membandingkan seluruh isi file tujuan.
- File sumber dan tujuan dibersihkan setelah pengujian, termasuk jalur gagal.
- Marker QEMU wajib: `RUST-STD-FS-COPY-PASS`.

**Validasi:** probe dan gate QEMU telah diperbarui. Build host dan eksekusi
guest untuk commit ini masih menunggu workflow CI; belum diklaim lulus.


## 85. POSIX hard links for Rust std and Cargo (23 November 2026)

EXT2, VFS, syscall ABI, and libc now expose the initial `link`/`linkat`
path. The syscall uses number 148 and currently accepts only `flags=0`;
the final source symlink is linked as a symlink rather than followed.

- EXT2 increments the existing inode link count and inserts a second
  directory entry; directories are rejected.
- Removing one name decrements the inode link count and preserves its data
  while another name remains.
- VFS rejects cross-mount hard links.
- libc exposes POSIX `link` and `linkat`; the Rust std guest probe checks
  alias contents and verifies that unlinking the first name leaves the second
  name usable.
- Kernel EXT2 tests cover inode identity, shared writes, duplicate/missing
  names, directory rejection, remount persistence, and hard-linking a symlink.

**Status and limits:** EXT2 and RAMFS implement shared-inode hard links;
RAMFS shares file data and permissions through reference-counted inode state.
FAT32 retains the default `Unsupported` result. `AT_SYMLINK_FOLLOW` and
`AT_EMPTY_PATH` are not implemented. Guest QEMU and CI results are pending
for this revision.


## 86. Samakan Rust runtime dengan POSIX wait status (23 November 2026)

QEMU mengungkap kegagalan probe Rust target setelah syscall waitpid diubah
agar mengembalikan status POSIX. Kernel sudah mengemas exit code normal pada
bit 8..15, tetapi helper `Child` pada `octagram-runtime` dan assertion probe
masih menganggap hasil wait sebagai angka exit mentah. Akibatnya child yang
keluar dengan kode 42 terlihat sebagai `42 << 8`, dan probe mengembalikan 42.

- `ExitStatus::code()` kini mendekode exit code dari status POSIX dan
  mengembalikan `None` untuk terminasi sinyal atau status non-normal.
- `ExitStatus::success()` kini memeriksa status normal dengan exit code nol.
- Probe no_std memeriksa nilai wait yang benar, yaitu `42 << 8`, sekaligus
  memastikan hasil `wait()` yang sudah di-cache tetap sama.
- Unit test mencakup exit sukses, exit nonzero, terminasi sinyal, dan status
  stopped.

**Validasi:** workflow sebelumnya berhasil pada build host, kernel tests,
libc tests, ISO, native TinyCC dan GNU Make. QEMU syscall suite gagal pada
probe Rust target akibat mismatch encoding ini. Perubahan koreksi sudah
di-commit; hasil host dan QEMU untuk revisi koreksi menunggu CI berikutnya.


## 87. Berbagi status O_APPEND pada descriptor hasil dup (23 November 2026)

Audit kompatibilitas POSIX menemukan bahwa `F_SETFL(O_APPEND)` sudah
mengubah flag pada objek `OpenFile` yang dibagi oleh descriptor hasil
`dup`, tetapi `F_GETFL` sebelumnya membaca salinan flag per-descriptor.
Akibatnya, perubahan melalui descriptor pertama tidak terlihat ketika
dibaca melalui descriptor duplikat.

- `F_GETFL` kini mengambil `O_APPEND` dari objek open-file bersama.
- `F_SETFL` tidak lagi menyimpan salinan `O_APPEND` pada tabel flag per-fd;
  tabel tersebut tetap menyimpan `FD_CLOEXEC` dan status nonblocking yang
  saat ini dikelola per descriptor.
- `file-api-demo` kini memeriksa `F_GETFL` melalui descriptor hasil `dup`.
- QEMU smoke suite menjalankan demo tersebut dan mewajibkan marker
  `POSIX-FCNTL-SHARED-APPEND`.

**Validasi:** perubahan kode, demo, dan QEMU gate sudah di-commit.
Hasil workflow untuk commit terbaru masih menunggu.


## 88. Implementasi positional I/O tanpa mengubah file offset (23 November 2026)

Audit menemukan bahwa `pread` dan `pwrite` sebelumnya diimplementasikan
dengan urutan `lseek -> read/write -> lseek kembali`. Pola ini tidak aman:
descriptor hasil `dup` berbagi offset, sehingga operasi positional dapat
mengganggu posisi I/O thread atau descriptor lain.

- Kernel kini membaca dan menulis regular file langsung pada offset eksplisit.
- File offset pada open-file description tidak diubah.
- Operasi pada pipe/socket/terminal mengembalikan `ESPIPE`.
- Penulisan tetap memeriksa capability dan permission perangkat serta
  memperbarui halaman shared mapping setelah perubahan data.
- `file-api-demo` menguji `pwrite`, `pread`, dan bahwa offset descriptor
  asli maupun duplikat tetap pada posisi semula; QEMU mewajibkan demo tersebut.

**Validasi:** perubahan kernel, regression demo, dan dokumentasi sudah
di-commit. Host dan QEMU CI untuk revisi ini masih menunggu.

## 89. Semantik preadv2/pwritev2 yang eksplisit (23 November 2026)

Libc sebelumnya mengabaikan parameter `flags` pada `preadv2` dan
`pwritev2`, sehingga pemanggil dapat mengira mode I/O khusus berhasil
meskipun kernel tidak menerapkannya. Implementasi kini menolak semua flag
nonzero dengan `ENOTSUP`, menerima hanya offset non-negatif atau sentinel
`-1`, dan meneruskan offset `-1` ke `readv`/`writev` agar memakai file
offset saat ini. Offset negatif selain `-1` menghasilkan `EINVAL`.

- `file-api-demo` menguji I/O dengan offset `-1`, perubahan file offset,
  penolakan flag yang belum didukung, dan offset negatif yang tidak valid.
- Flag `RWF_*` belum didukung; implementasi tidak lagi mengabaikannya diam-diam.
- QEMU smoke test menjalankan demo file API yang diperluas.

**Validasi:** source, regression demo, dan roadmap diperbarui. Hasil host dan
QEMU untuk commit ini menunggu workflow CI.

## 90. close_range menolak flag yang belum didukung (23 November 2026)

Implementasi libc `close_range` sebelumnya mengabaikan seluruh argumen
`flags`. Sekarang rentang terbalik menghasilkan `EINVAL`, sedangkan flag
nonzero ditolak dengan `ENOTSUP` karena `CLOSE_RANGE_UNSHARE` dan
`CLOSE_RANGE_CLOEXEC` belum diimplementasikan. Operasi dengan flag nol
tetap menutup descriptor pada rentang yang diminta dan mengabaikan descriptor
yang memang sudah tertutup.

- Regression demo menguji flag yang belum didukung, rentang terbalik, dan
  penutupan descriptor.
- Tidak ada klaim dukungan terhadap semantik CLOEXEC/UNSHARE sampai kernel
  dan runtime benar-benar mengimplementasikannya.

**Validasi:** perubahan source, regression test, dan roadmap di-commit;
hasil CI untuk revisi ini menunggu.


## 91. Pemeriksaan otomatis prasyarat Stage 0 Rust (23 November 2026)

Target Make `stage0-inventory` dan skrip `tools/stage0-inventory.sh` kini
memeriksa tool host yang diperlukan sebelum pekerjaan bootstrap native:
Rust nightly/rustup, Cargo, LLD, readelf, lokasi sysroot, source `rust-src`,
versi LLVM yang dilaporkan rustc, dan kemampuan nightly membaca target JSON
OCTAGRAM.

Pemeriksaan ini sengaja hanya merupakan inventaris host. Ia tidak membangun
compiler native, tidak menguji ELF compiler di guest, dan tidak mengubah status
milestone Stage 0 execution. Channel Rust masih berupa `nightly` bergerak;
log inventaris perlu digunakan untuk menentukan tanggal toolchain yang akan
dipin setelah CI menjalankan pemeriksaan ini.

**Validasi:** target dan skrip telah ditambahkan. Hasil CI untuk perubahan ini
masih menunggu; belum ada klaim bahwa Stage 0 native sudah berjalan.


## 92. Jadikan build dan link upstream Rust std sebagai gate CI (23 November 2026)

Probe upstream Rust `std` dan proses link executable OCTAGRAM sebelumnya
memakai `continue-on-error`, sehingga kegagalan keduanya tidak menggagalkan
workflow. Pada revisi ini kedua langkah dijadikan gate wajib setelah berhasil
melewati build CI:

- `cargo -Zbuild-std=std check` harus berhasil untuk target OCTAGRAM.
- Executable probe `std` harus berhasil ditautkan dengan linker OCTAGRAM.
- Pemeriksaan ELF tetap memvalidasi executable x86_64 statis.
- Build no_std, unit test runtime host, dan validasi ELF tetap menjadi gate
  terpisah.

**Batas validasi:** ini membuktikan kompilasi dan linking silang dari host,
bukan eksekusi program `std` di guest dan bukan compiler native. Tahap
berikutnya tetap menjalankan probe di QEMU, lalu membangun Stage 0 compiler
yang dapat dieksekusi oleh OCTAGRAM.


## 93. Perbaiki lingkungan link Rust std dengan runtime target (23 November 2026)

Gate linking Rust std mengungkap dua kesalahan konfigurasi build, bukan
kegagalan kompilasi std: path linker script dan `crt0.o` relatif terhadap
direktori kerja linker, serta pustaka target `-lc`, `-lm`, `-lrt`, dan
`-lpthread` belum disediakan.

CI kini membangun `user/build/libc.a` dari runtime C OCTAGRAM dan membuat
alias nama pustaka Unix konvensional ke arsip runtime terpadu tersebut.
Langkah link menggunakan path absolut untuk linker script dan startup object,
serta menambahkan direktori arsip target ke native library search path.

**Batas validasi:** alias `libm`, `librt`, dan `libpthread` adalah
kompatibilitas penamaan sementara karena runtime masih satu arsip; ini bukan
klaim bahwa subsistem pustaka tersebut telah dipisahkan. Hasil linking
selanjutnya tetap harus lolos CI. Executable belum dianggap berjalan di QEMU,
dan rustc/Cargo belum native.

## 94. Celah PAL yang ditemukan saat menjalankan std asli di guest (24 November 2026)

- `user/rust/std-probe` dibangun ulang dengan `-Zbuild-std=std` dari `rust-src`
  terpatched plus upstream libc commit `ccc2ff4aaab962f7fde7b8edda7379c045b1ca12`,
  lalu ditautkan memakai `user/crt0.S`, `user/linker.ld`, dan
  `user/build/libc.a`. Hasilnya ELF64 `EXEC` x86-64 639936 byte tanpa
  `PT_INTERP`, tanpa dynamic section, tanpa simbol undefined, dan dengan
  `PT_TLS` (FileSiz 0x48, MemSiz 0x480).
- Eksekusi guest QEMU (2 GiB, TCG) menjalankan `/bin/rust-std-probe` dan
  melewati 41 marker, termasuk getrandom, socketpair/sendmsg/recvmsg, TLS
  local-exec, OsString/Path byte-preserving, filesystem nama non-UTF-8,
  `remove_dir_all` rekursif, operasi `*at` relatif, symlink, `fs::copy`,
  hard link, permission, mutasi environment, metadata, serta thread
  spawn/join, isolasi TLS, dan detach.
- Celah yang ditemukan lalu diperbaiki di kernel:
  - `open()` read-only pada direktori kini diizinkan; sebelumnya selalu
    `EISDIR`, sehingga `std::fs::File::open` pada direktori gagal.
  - `O_DIRECTORY` dan `O_NOFOLLOW` diterima dan diterapkan (`ENOTDIR` /
    `ELOOP`), bukan diabaikan; `O_CREAT | O_DIRECTORY` tetap `EINVAL`.
  - `readdir` menerima descriptor `open()` yang menunjuk direktori dan
    mempromosikannya menjadi directory stream, sesuai `fdopendir()` POSIX.
    `std::fs::remove_dir_all` memakai jalur ini.
  - `openat`, `mkdirat`, `renameat`, `unlinkat`, dan `fstatat` menerima
    dirfd dari `open()` direktori, tidak hanya dari `opendir()`.
  - Argumen `mode` pada `open`, `mkdir`, `chmod`, dan `fchmod` memakai mask
    `0o777` sehingga bit tipe file diabaikan; `std::fs::copy` dan
    `set_permissions` meneruskan `st_mode` penuh.
  - `ioctl(FIOCLEX)`/`ioctl(FIONCLEX)` kini mengubah `FD_CLOEXEC` untuk semua
    jenis descriptor; Rust std memakai `FIOCLEX` pada pipe hasil
    `libc::pipe()`.
- Probe kini mencetak error `remove_dir_all` dan langkah `fs::copy` yang
  gagal, serta memisahkan kegagalan `Command::spawn` dari `wait`.

- Celah lanjutan yang muncul setelah `Command` berjalan, dan perbaikannya:
  - `/dev/null` belum ada; `Stdio::Null` membuka perangkat itu sehingga
    `Command::output` gagal ENOENT. `DevFs` sekarang punya perangkat `Null`
    (tulis dibuang, baca selalu EOF) dan `stat` melaporkannya sebagai
    karakter device.
  - Makro wait-status libc untuk target `octagram` salah: `WEXITSTATUS`
    memakai `status & 0xff` dan `WIFEXITED` selalu `true`, padahal kernel
    menyimpan `(code & 0xff) << 8`. Patch `patches/rust-libc` sekarang
    memakai encoding POSIX/Linux, sehingga `ExitStatus::code()` dan
    `success()` benar.
  - `sh` belum punya `printf`, positional parameter `$1`/`$2`, `$#`, `$$`,
    builtin `kill`, atau loop `while`. Semua ditambahkan; `sh -c` kini
    mengisi `$0`-`$n` sesuai POSIX.
  - `kill` ke PID lain selalu `ESRCH` karena `syscall_kill` hanya
    menangani proses pemanggil. Sinyal untuk proses lain sekarang dicatat
    pada entry scheduler target dan `Child::kill` berhasil.
  - Thread yang exit tidak menutup tabel descriptor-nya, sehingga salinan
    `Arc<OpenFile>` (dan advisory `flock`) bocor sampai proses induk
    selesai. `syscall_thread_exit` sekarang menutup descriptor thread
    seperti jalur exit proses. Ini yang membuat tes "close melepas flock"
    deterministic.

**Validasi:** build probe dan `cargo check --tests` kernel lulus. Run QEMU
terakhir (2 GiB, TCG) melewati **54 marker** dan berakhir dengan
`RUST-STD-RUNTIME-PASS`, `program exited with status 0`, lalu prompt shell
kembali. Marker terakhir mencakup `Command` dengan stdin `/dev/null`, argv
+ stderr + exit code 7, status sinyal `SIGTERM`, `try_wait`/`Child::kill`,
cache status `wait`, `env`/`current_dir`, PATH lookup, serta
`File::lock`/`try_lock`/`unlock`, konversi shared/exclusive, waiter thread
yang dibangunkan `unlock`, dan pelepasan lock saat descriptor terakhir
ditutup. Native `rustc`/Cargo **belum ada**: keduanya masih menunggu gate
M4.3 (backend compiler dan linker native), jadi belum ada perintah
`rustc`/`cargo` di shell.

## 95. Pustaka libc++ hasil kompilasi pertama (25 November 2026)

Program C++ OCTAGRAM sebelumnya hanya memakai header libc++ 18.1.8; isi
library (`libcxx/src`) belum dibangun, sehingga `std::string`, `std::sort`,
dan `std::mutex` masih undefined saat link. Revisi ini mem-vendor `src/` dan
membangun `user/build/libc++.a`:

- `user/third_party/libcxx/src/` berisi source libc++ dari tag yang sama
  (`llvmorg-18.1.8`), tanpa modifikasi.
- `user/Makefile` menambahkan target `build/libc++.a` yang mengompilasi dua
  belas translation unit: `string`, `vector`, `algorithm`, `stdexcept`,
  `mutex`, `mutex_destructor`, `error_category`, `system_error`,
  `charconv`, serta `ryu/d2fixed`, `ryu/d2s`, dan `ryu/f2s`.
- `user/include/float.h` ditambahkan karena `charconv.cpp` membutuhkan
  `FLT_MANT_DIG`/`DBL_MANT_DIG`/`LDBL_*`/`DECIMAL_DIG` dan sysroot OCTAGRAM
  belum punya header itu. Nilainya x86_64: binary32, binary64, x87 80-bit
  untuk `long double`, `FLT_EVAL_METHOD` 0, `FLT_ROUNDS` 1.
- `user/libcxx_runtime.cpp` menambahkan `std::terminate`,
  `std::exception::~exception()`, `std::exception::what()`, serta anggota
  `std::condition_variable` di atas `pthread_cond_*`; `src/exception.cpp`
  (masih memakai `unexpected_handler` C++17-removed) dan
  `src/condition_variable.cpp`/`src/thread.cpp` belum dikompilasi.
- `user/wchar.c` mendapat `vswprintf`/`swprintf` C99 untuk buffer karena
  `src/string.cpp` memakainya; wide stdio berbasis `FILE *` tetap belum ada.
- Objek library memakai flag freestanding yang sama dengan header probe tetapi
  `-std=c++20`, karena `src/algorithm.cpp` menginstansiasi internal
  `std::ranges`/`std::__bit_log2` yang libc++ sembunyikan pada C++17.
- `user/examples/libcxx_string_smoke.cpp` menjadi probe guest pertama yang
  menautkan `libc++.a`, di-seed sebagai `/bin/libcxx-string-smoke`, dan
  mencetak `LIBCXX-STRING-PASS`, `LIBCXX-VECTOR-PASS`, `LIBCXX-MUTEX-PASS`,
  lalu `LIBCXX-LIBRARY-PASS`.
- `tools/qemu_syscall_test.py` menjalankan probe itu, memeriksa
  `LIBCXX-LIBRARY-PASS` dan exit status 0. Selama QEMU belum dijalankan di
  workspace ini, statusnya tetap **terbukti di host**, bukan guest-tested.

Batas yang masih berlaku: `std::iostream` (locale/ios/fstream), `std::thread`,
`std::filesystem`, `random`, `std::thread`, dan iostream belum masuk arsip.
Untuk `<charconv>`, hanya integer `to_chars`/`from_chars` dan floating
`to_chars` yang tersedia: libc++ 18 tidak punya floating-point
`std::from_chars`. Native `rustc` dan Cargo tetap belum ada, dan M4.3/M4.4
tidak boleh ditandai selesai hanya karena `libc++.a` terbangun.
