# M4.3.3 — Build libLLVM statis untuk `x86_64-unknown-octagram`

Runbook ini menjelaskan cara membangun libLLVM yang menjadi prasyarat `rustc`
native (M4.3.4). Semua langkah berjalan di host Linux; hasilnya adalah library
dan tool LLVM berformat ELF OCTAGRAM yang nantinya dijalankan di guest.

Status: **scaffolding**. Skrip, toolchain file, dan sysroot sudah ada di repo,
tetapi belum pernah dijalankan sampai selesai di workspace mana pun. Jangan
menandai M4.3.3 selesai sebelum gate di §5 lulus di QEMU.

## 0. Jalur cepat

Seluruh rangkaian (sysroot → libLLVM → bootstrap rustc/cargo) punya satu pintu:
 
```sh
tools/build_octagram_toolchain.sh --jobs "$(nproc)"   --libc-checkout build/toolchain/libc
```

Skrip itu memanggil tiga langkah di bawah ini secara berurutan dan berhenti pada
kegagalan pertama. Gunakan `--prepare-only` untuk berhenti setelah sysroot dan
`config.toml` bootstrap tanpa menjalankan `x.py`, atau `--llvm-prefix DIR` untuk
memakai libLLVM yang sudah dibangun.

## 1. Prasyarat

Host (Debian/Ubuntu):

```sh
sudo apt-get install build-essential cmake ninja-build curl python3
```

Repo:

```sh
make -C user sysroot     # user/build/sysroot-octagram + sysroot.json
```

Sysroot berisi header publik, header libc++ 18, `crt0.o`, `libc.a`, `libc++.a`,
`libcxxrt.a`, dan `user/linker.ld`. Driver compiler ada di `user/toolchain/bin`
(lihat [user/toolchain/README.md](../user/toolchain/README.md)).

## 2. Pin versi LLVM

`rustc` menolak LLVM yang lebih lama dari yang diminta checkout-nya. Ambil versi
dari checkout Rust yang akan di-bootstrap, bukan dari ingatan:

```sh
grep -m1 'LLVM_VERSION_MAJOR' build/toolchain/rust/src/llvm-project/llvm/CMakeLists.txt
```

Versi default di `tools/build_llvm_octagram.sh` (`21.1.0`) harus diperbarui
bersamaan. Skrip memeriksa ulang `LLVM_VERSION_MAJOR` pada source yang
diextract dan berhenti jika tidak cocok.

Untuk build reproducible, set juga hash tarball:

```sh
export LLVM_SHA256=<sha256 llvm-project-<versi>.src.tar.xz>
```

## 2b. Preflight: probe link runtime

Sebelum CMake dijalankan, `tools/build_llvm_octagram.sh` memanggil
`tools/check_octagram_runtime_probes.sh`. Skrip itu hanya *menautkan* empat
program kecil dari `tools/toolchain-probes/`, jadi selesai dalam hitungan detik
dan tidak butuh QEMU:

| probe | kebutuhan libLLVM/rustc yang diwakili |
|---|---|
| `sync_primitives.cpp` | `std::mutex`, `std::condition_variable`, `std::call_once` (thread pool, cache, diagnostics) |
| `thread_local_state.cpp` | `thread_local` dengan destruktor (`__cxa_thread_atexit`) |
| `mmap_file_io.cpp` | `open`/`fstat`/`mmap`/`mprotect`/`munmap` seperti `MemoryBuffer` |
| `large_allocation.cpp` | `new[]`, `malloc`, dan `std::vector` puluhan MiB |

Setiap probe harus menghasilkan ELF64 `ET_EXEC`. Jika satu gagal, pesan linker
menyebut simbol yang kurang dan build LLVM dibatalkan sebelum waktu terbuang.
Menjalankannya manual:

```sh
make -C user sysroot
tools/check_octagram_runtime_probes.sh
```

Kegagalan pertama suite ini sudah menemukan dua hal nyata: operator
`new`/`delete` over-aligned dan nothrow tidak ada, dan `sys/mman.h` (serta
belasan header syscall lain) mendeklarasikan fungsinya tanpa `extern "C"`
sehingga kode C++ menautkan ke nama termangle.

## 3. Build

```sh
tools/build_llvm_octagram.sh --llvm-version 21.1.0 --jobs "$(nproc)"
```

Yang dijalankan skrip:

1. mengunduh `llvm-project-<versi>.src.tar.xz` (opsional `--source DIR` untuk
   source lokal), memeriksa versi, dan mengekstrak ke `build/toolchain/`;
2. mengonfigurasi CMake dengan `user/toolchain/octagram-toolchain.cmake` dan
   opsi statis:

   | Opsi | Alasan |
   | --- | --- |
   | `LLVM_TARGETS_TO_BUILD=X86` | hanya backend x86_64 yang dibutuhkan |
   | `BUILD_SHARED_LIBS=OFF`, `LLVM_BUILD_LLVM_DYLIB=OFF` | guest belum punya TLS/dynamic linking untuk toolchain |
   | `LLVM_ENABLE_THREADS=OFF` | menghindari ketergantungan pthread/TLS di LLVM saat bootstrap; rustc single-thread cukup |
   | `LLVM_ENABLE_EH=OFF`, `LLVM_ENABLE_RTTI=OFF` | libc++ OCTAGRAM dibangun `-fno-exceptions -fno-rtti` |
   | `LLVM_ENABLE_TERMINFO/ZLIB/ZSTD/LIBXML2=OFF` | tidak ada library host tersebut di target |
   | `LLVM_INCLUDE_TESTS/BENCHMARKS/EXAMPLES/UTILS=OFF` | tidak menambah permukaan port |
   | `LLVM_ENABLE_UNWIND_TABLES=OFF` | guest memakai frame unwinding sendiri, bukan `.eh_frame` |

3. `cmake --build ... --target install` ke `build/toolchain/llvm-<versi>-octagram`;
4. mencetak `llvm-config --version` dan lokasi library.

## 4. Keluaran

```
build/toolchain/llvm-<versi>-octagram/
  bin/llvm-config
  bin/llc
  bin/llvm-nm
  lib/libLLVM*.a
  include/llvm/...
```

`llvm-config` inilah yang dipakai `tools/bootstrap_rustc_octagram.sh`.

## 5. Gate verifikasi

Build host sukses bukan bukti apa pun. Sebelum M4.3.3 dianggap selesai:

| Gate | Cara |
| --- | --- |
| ELF valid | `readelf -h build/toolchain/llvm-*/bin/llvm-nm`: `ET_EXEC`, x86-64, tanpa `INTERP`/dynamic section |
| `llvm-config` jalan di guest | salin ke image, jalankan `llvm-config --version` dari shell OCTAGRAM |
| tool LLVM jalan di guest | `llvm-nm` pada satu object, `llc` pada satu `.ll` kecil, exit status 0 |
| tidak ada simbol host yang bocor | `ld` native menautkan object LLVM hasil `llc` (uji M4.3.0 sudah ada di `kernel/src/toolchain.rs`) |
| pengulangan | hash `sysroot.json` dan versi LLVM dicatat di log CI |

Belum ada langkah image yang menyalin toolchain ke guest: kernel saat ini
menanam program lewat `include_bytes!`, yang tidak cocok untuk binary ratusan
MB. Penyalinan ke root persisten (`/toolchain`) adalah pekerjaan terpisah —
lihat [rustc-native-bootstrap-runbook.md](rustc-native-bootstrap-runbook.md) §5.

## 6. Risiko yang diketahui

- **Threads off bukan solusi permanen.** `rustc` butuh `std::thread` untuk
  codegen paralel; LLVM tanpa thread hanya memindahkan masalah. Setelah
  pthread/TLS guest lolos uji beban, `LLVM_ENABLE_THREADS=ON` harus dicoba.
- **pthread/TLS pada libc++** sudah dipakai `std::shared_mutex` dan
  `std::thread` di `libc++.a`, tetapi belum diverifikasi di QEMU.
- **Ukuran binary.** `llvm-nm`/`llc` untuk target ini masih besar; heap guest
  dan batas `mmap` perlu diukur sebelum `rustc` (ratusan MB RSS) dijalankan.
- **Relokasi.** Jika `librustc_driver` menghasilkan relokasi di luar cakupan
  linker native (`kernel/src/toolchain.rs`), fallback dokumen adalah memport
  LLD (opsi B di [rust-m4-3-backend-plan.md](rust-m4-3-backend-plan.md)).
- **CI belum pernah menjalankan build penuh.** Workflow menyediakan job-nya
  (lihat §7), dan build libLLVM sekarang bisa dilanjutkan antar-run, tetapi
  satu rangkaian penuh sampai artefak `octagram-toolchain` belum tercatat
  selesai di runner GitHub.
- **Build lebih lama dari satu job.** Runner GitHub mematikan job di 6 jam,
  jadi libLLVM tidak mungkin selesai dalam satu run dengan `--jobs 4`. Yang
  menjamin kemajuan adalah checkpoint build tree (§7b), bukan satu run panjang.

## 7. CI

Workflow `.github/workflows/octagram-native-toolchain.yml` menjalankan tiga
langkah di atas sebagai tiga job berurutan, dengan artefak di antaranya:

| Job | Perintah inti | Artefak |
| --- | --- | --- |
| `preflight` | `make -C user sysroot`, `check_octagram_runtime_probes.sh`, `check_octagram_rust_target.py` | `octagram-sysroot` |
| `llvm` | `tools/build_llvm_octagram.sh --llvm-version ... --jobs ...` | `octagram-llvm-<versi>` |
| `rustc-cargo` | `tools/bootstrap_rustc_octagram.sh --llvm-prefix ... --libc-checkout upstream-libc --time-budget ... --resume --build` | `octagram-toolchain` |

Pull request hanya menjalankan `preflight`. Push ke
`feature/native-self-hosting-testing-4` dan `workflow_dispatch` menjalankan
`llvm` lalu `rustc-cargo`, dengan artefak di antaranya.

### 7b. Build yang bisa dilanjutkan (batas 6 jam GitHub)

Runner GitHub mematikan job di **6 jam** dan tidak menjalankan step
sesudahnya, sedangkan cache dependensi dibatasi **10 GB per repo** (entri yang
tidak diakses 7 hari dihapus, dan yang paling lama dipakai dibuang lebih dulu
saat kuota penuh). Build libLLVM statis untuk target ini lebih lama dari itu,
jadi satu run tidak dirancang untuk menyelesaikannya sendirian:

1. `timeout-minutes: 350` (di bawah batas 360) dan `--time-budget` default
   **270 menit** untuk langkah build. Sisa ~80 menit dipakai untuk setup,
   restore cache, dan menyimpan checkpoint.
2. Saat budget habis, `build_llvm_octagram.sh` mengirim `SIGINT` ke ninja,
   membiarkan ninja menyelesaikan edge yang sedang jalan, menulis
   `octagram-build-state.json` dengan `"phase": "incomplete"`, dan keluar
   dengan **75** (`EX_TEMPFAIL`). Build tree tidak dihapus. Kalau ninja tidak
   keluar setelah `SIGINT` (satu edge `-O2` masih jalan), script mengeskalasi
   ke `SIGTERM` lalu `SIGKILL` — default tunggu 120 detik, lalu 60 detik — dan
   tetap menulis state itu, jadi budget stop tidak pernah berakhir tanpa
   checkpoint. Anak prosesnya (`cc1plus`, `rustc`, `cargo`) ikut disinyal dan
   ditunggu; sisa yang masih hidup dibunuh, supaya checkpoint tidak
   mengarsipkan file yang masih ditulis. Sinyal yang akhirnya mematikan build
   tercatat di `"stop_signal"`.
3. Step build memetakan exit 75 menjadi `incomplete=true` dan **exit 0**: job
tidak merah karena batas waktu, dan step verifikasi/upload dilewati. Step
   `Save libLLVM build checkpoint` menyimpan `build-octagram/` dengan key
   `octagram-llvm-build-<versi>-<hash>-<run_id>`; `restore-keys` berakhiran `-`
   membuat run berikutnya mengambil checkpoint terbaru untuk input yang sama.
4. Run berikutnya me-restore checkpoint, mendeteksi `CMakeCache.txt`, dan
   memanggil script dengan `--resume`: konfigurasi CMake dilewati dan ninja
   hanya membangun yang belum selesai.
5. Setelah build benar-benar selesai dan verifikasi ELF statis lulus, install
tree disimpan dengan key `octagram-llvm-install-<versi>-<hash>`. Key itu
**restore-only** sampai verifikasi lulus, jadi prefix yang setengah jadi tidak
pernah dianggap selesai.
6. Artefak `octagram-llvm-<versi>` adalah **satu arsip tar**, bukan direktori.
   `actions/upload-artifact` tidak menyimpan bit executable (semua file kembali
   0644), sehingga `bin/llvm-config` yang diunduh job `rustc-cargo` tidak bisa
   dieksekusi dan bootstrap berhenti dengan `no static libLLVM install found`
   padahal build LLVM-nya lulus. Arsip menjaga mode file sekaligus tata letak
   prefix yang dipakai key cache install tree. Kalau script tetap menemukan
   `llvm-config` yang ada tapi tidak executable, pesan errornya sekarang
   menyebut mode file dan penyebabnya, bukan menyuruh membangun LLVM lagi.

Kenapa resume tidak memicu rebuild penuh: source LLVM di-extract ulang dari
tarball yang di-cache, dan `tar` memulihkan mtime dari arsip. Objek yang sudah
dibangun tetap lebih baru dari source, jadi ninja tidak menganggap semuanya
basi. Build tree sendirilah yang di-cache, bukan source hasil extract.

Konsekuensi yang perlu diketahui:

- Cache checkpoint bisa beberapa GB. Simpan satu checkpoint per hash input;
  kalau kuota 10 GB repo kurang, naikkan *Cache size eviction limit* di
  Settings → Actions (di atas 10 GB ditagih) atau pakai runner yang lebih besar.
- Job yang dibatalkan tidak pernah sampai ke step simpan checkpoint. Karena itu
  workflow memakai `cancel-in-progress: false`: push baru mengantre dan
  melanjutkan dari checkpoint, bukan mematikan build yang sedang jalan.
- `workflow_dispatch` punya input `time_budget` (menit) dan `fresh_build`
  (buang checkpoint dan mulai dari nol, misalnya setelah mengubah flag CMake
  yang tidak tercakup hash).
- `status-summary` melaporkan `overall=incomplete` (dengan warning annotation)
  untuk run yang berhenti di budget, dan job `rustc-cargo` tidak dijalankan
  sampai libLLVM lengkap.

Job `rustc-cargo` memakai pola yang sama untuk bootstrap rustc: input
`time_budget` yang sama, `SIGINT` ke `x.py` (eskalasi `SIGTERM`/`SIGKILL` bila
`x.py` tidak keluar), exit 75, dan checkpoint `build/toolchain/rust` (checkout + build
directory) dengan key
`octagram-rust-build-<versi>-<hash>-<run_id>-<attempt>`. Checkout ikut di-cache
karena clone baru akan mengembalikan mtime sumber ke waktu sekarang dan memaksa
cargo membangun ulang seluruh tree; `.git` juga harus ikut karena bootstrap
menghitung stempel build dari `git diff`/`git status`.

Supaya checkpoint itu muat di kuota **10 GB per repo**, `prune_checkpoint`
membuang `build/<host>/stage0` dan `stage0-sysroot` sebelum menulis state
`incomplete`. Keduanya salinan toolchain rustup yang dipin: stage0 disalin ulang
ke `build/<host>/stage0` dan `stage0-sysroot` diisi ulang dari
`initial_sysroot` setiap run, dan keduanya tidak dipakai sebagai input
fingerprint cargo maupun stempel bootstrap (yang berbasis hash isi). Jadi
stage1/stage2 yang sudah selesai tetap valid dan run berikutnya hanya menyalin
ulang stage0. Sisa tree (`build/tmp`, `stage1*`, `stage2*`, checkout, `.git`)
tetap ikut karena itulah progres build-nya. Setiap run tetap menulis key baru
(key cache tidak bisa ditimpa), dan saat kuota penuh GitHub membuang entri yang
paling lama dipakai; kalau beberapa generasi masih tidak muat, naikkan *Cache
size eviction limit* di Settings → Actions.

Pemeriksaan kontrak ini ada di `tools/check_octagram_llvm_ci.py` (dijalankan
job `fixture` di `octagram-llvm-config.yml`): timeout kedua job di bawah 360
menit, budget di bawah timeout, checkpoint di-restore lewat prefix dan hanya
disimpan saat `incomplete`, install tree hanya di-cache setelah verifikasi, dan
kedua script mendukung `--time-budget`/`--resume`/`--status` serta exit 75.

### Workflow cepat: `octagram-llvm-config.yml`

Jalur `llvm-config` punya workflow sendiri supaya bisa diuji tanpa menunggu build
LLVM:

| Job | Isi | Durasi |
| --- | --- | --- |
| `fixture` | parse (`py_compile`, `sh -n`), pemeriksaan wiring (generator dipanggil build script, bootstrap memakai wrapper dan `--host-llvm-config`, nomor perintah `fcntl` tidak bertabrakan), `tools/check_octagram_llvm_ci.py` (kontrak build yang bisa dilanjutkan), lalu `tools/check_octagram_llvm_config.sh` | detik |
| `real-llvm` | unduh source LLVM, **configure saja** (tanpa build), lalu jalankan pemeriksaan yang sama dengan `--build-dir` ke manifest komponen sungguhan | beberapa menit |

`tools/check_octagram_llvm_config.sh` menguji: `--version`/`--host-target`,
`--components`, arsip yang belum ada dilaporkan `missing:` + exit 1, `--link-shared`
ditolak, tidak ada library duplikat, library daun (`-lLLVMDemangle`) di akhir,
`--libnames`/`--libfiles`, serta empat kasus preflight bootstrap (tanpa wrapper,
versi beda, wrapper tidak bisa dijalankan, tanpa `llvm-config` host). Untuk
fixture, urutan persisnya ikut diassert. Semuanya bebas jaringan dan tanpa
Rust toolchain: bootstrap mendapat flag `--check` yang berhenti setelah semua
input tervalidasi dan mencetak `OCTAGRAM-BOOTSTRAP-PREFLIGHT-PASS`.
Input dispatch: `llvm_version`, `jobs`, `build_llvm` (matikan untuk memakai
cache libLLVM), dan `rustc_mode` (`full`, `prepare-only`, atau `skip`).

Verifikasi di CI bersifat statis karena binary hasilnya adalah ELF OCTAGRAM yang
tidak bisa dijalankan di runner:

- setiap `bin/llvm-config`, `llc`, `llvm-nm`, `rustc`, dan `cargo` harus ELF64
  x86-64 `ET_EXEC` tanpa `PT_INTERP` dan tanpa `PT_DYNAMIC`;
- `lib/libLLVM*.a` harus ada, dan `rustc` harus membawa `lib/rustlib` dengan
  `std` untuk `x86_64-unknown-octagram` (`stage_octagram_toolchain.sh` yang
  menolak toolchain host);
- cache `actions/cache` menyimpan tarball sumber LLVM dan direktori install
  libLLVM, sehingga `build_llvm=false` dapat memakai hasil sebelumnya.

Gate §5 (menjalankan `llvm-config`, `llc`, `rustc --version`, dan
`cargo run --offline` di guest) tetap manual: belum ada langkah image yang
menyalin toolchain ke root persisten, dan runner tidak punya guest OCTAGRAM.
Hasil CI yang lulus berarti artefak berhasil dibangun untuk ABI target, bukan
bahwa toolchain sudah berjalan di QEMU.

## 8. Temuan audit build LLVM di host

Audit ini menjalankan configure CMake dan build sebagian LLVM 21.1.0 dengan
toolchain OCTAGRAM sungguhan (bukan hanya membaca skrip). Hasil yang terbukti:
`LLVMSupport`, `LLVMTableGen`, dan `LLVMCore` ter-archive, dan `bin/llvm-min-tblgen`
ter-link sebagai ELF64 `ET_EXEC` statis x86-64 tanpa `PT_INTERP`. `tablegen`
host dibangun otomatis sebagai sub-build `NATIVE/` dengan compiler host, jadi
cross-build tidak pernah mencoba mengeksekusi binary target.

Gap yang ditemukan dan diperbaiki:

| Gejala | Akar masalah | Perbaikan |
| --- | --- | --- |
| `endian.h: No such file` | sysroot tidak punya `<endian.h>`/`<machine/endian.h>` | header baru dengan`__BYTE_ORDER` dan `htole*`/`be*toh` |
| `basic_file_status has no member getSize` | `CMAKE_SYSTEM_NAME Generic` membuat LLVM `LLVM_ON_UNIX=0` dan membuang semua implementasi Unix | `user/toolchain/octagram-project-include.cmake` menyetel `UNIX` setelah `project()` (via `CMAKE_PROJECT_INCLUDE`) |
| `EX_IOERR not available` | tidak ada `<sysexits.h>` | header baru + re-detect `HAVE_SYSEXITS_H` |
| `jmp_buf declared using unnamed type` | `setjmp.h` memakai struct anonim tanpa `extern "C"` | struct bertag `__octagram_jmp_buf` + `extern "C"` |
| `unknown type name size_t` (regex) | `sys/types.h` tidak menyediakan `size_t` | `#include <stddef.h>` |
| `sys/vfs.h` / `F_WRLCK` / `struct flock` | header Linux-shaped belum lengkap | `<sys/vfs.h>` + `statfs`/`fstatfs` (ENOSYS seperti `statvfs`), `struct flock` + `F_*LCK` di `<fcntl.h>`, pemetaan lock ke `flock` kernel |
| `rlim_t`, `ru_maxrss`, `_POSIX_ARG_MAX` | `sys/resource.h` dan `limits.h` kurang lengkap | `rlim_t`, layout `rusage` Linux, makro `_POSIX_*` |
| `void (*)(int, void*, void*)` vs `siginfo_t*` | tipe `sa_sigaction` salah | tipe POSIX di `<signal.h>` |
| `SYS_rt_tgsigqueueinfo` | `sys/syscall.h` hanya berisi sebagian nomor | nomor syscall ditambahkan (kernel menjawab ENOSYS) |
| `cpuid.h: No such file` | header bawaan GCC tidak ada di sysroot (`-nostdinc`) | `user/include/cpuid.h` kompatibel GCC |
| `TLS reference ... mismatches non-TLS reference` | driver tidak mendefinisikan `OCTAGRAM_TLS_ERRNO`, jadi objek melihat `errno` non-TLS | driver menambahkan `-DOCTAGRAM_TLS_ERRNO` |
| TLS GD vs local-exec | CMake menambahkan `-fPIC` setelah flag driver | driver mengulang `-fno-pie -fno-pic` setelah argumen pemanggil |
| `-lm` mengambil libm glibc host | sysroot tidak punya nama library Unix standar | staging membuat `libm.a`/`librt.a`/`libdl.a`/`libpthread.a` kosong (simbol berasal dari `libc.a` yang selalu ditautkan driver) |

### `llvm-config` yang bisa dieksekusi di mesin build

`bootstrap_rustc_octagram.sh` menjalankan `llvm-config` (untuk `--libs`,
`--system-libs`, `--cxxflags`, `--version`) di mesin build, sedangkan
`bin/llvm-config` hasil cross-build adalah ELF OCTAGRAM yang hanya bisa berjalan
di guest. Konfigurasi lama menaruh ELF itu di `[target.x86_64-unknown-linux-gnu]`,
sehingga `x.py` berhenti dengan `could not execute llvm-config` sebelum
kompilasi dimulai. Sekarang ada dua llvm-config, satu untuk tiap platform:

| Konfigurasi | Binary | Sumber |
| --- | --- | --- |
| `[target.x86_64-unknown-linux-gnu]` | `--host-llvm-config` (atau `HOST_LLVM_CONFIG`, atau `llvm-config` di `PATH`) | LLVM 21 host, dipakai untuk compiler yang berjalan di mesin build |
| `[target.x86_64-unknown-octagram]` | `$llvm_prefix/bin/octagram-llvm-config` (atau `--target-llvm-config`) | dibuat oleh `tools/octagram_llvm_config.py` |

`tools/octagram_llvm_config.py` menulis skrip Python yang bisa dijalankan host
dan melaporkan isi install target. Datanya diambil dari build tree LLVM sendiri:
`tools/llvm-config/BuildVariables.inc` (versi, flag, `LLVM_SYSTEM_LIBS`),
`tools/llvm-config/LibraryDependencies.inc` (komponen → library + dependensi),
dan `include/llvm/Config/llvm-config.h`. Urutan library mengikuti
`llvm-config.cpp`: telusuri komponen secara depth-first dan tambahkan library
setelah dependensinya, lalu balik seluruh daftar sehingga dependen muncul lebih
dulu daripada dependensi. Itu sebabnya `--libs core support` menghasilkan
`-lLLVMCore -lLLVMRemarks … -lLLVMSupport -lLLVMDemangle`, sama seperti
`llvm-config` asli; urutan alfabetis akan gagal saat link statis.

Perilaku yang dijaga sama dengan `llvm-config` asli: `--link-shared` gagal
karena install ini hanya statis, library yang belum dibangun dilaporkan sebagai
`missing:` dengan exit 1 (jadi tahap yang masih build tidak mengklaim flag yang
belum bisa dipenuhi), `--libnames`/`--libfiles` mengikuti urutan yang sama, dan
argumen tak dikenal ditolak.

Rangkaian pemakaiannya:

- `tools/build_llvm_octagram.sh` menjalankan generator setelah configure lalu
  memverifikasi hasilnya setelah install (`--version` harus sama dengan versi
  LLVM dan `--libs` tidak boleh kosong);
- `bootstrap_rustc_octagram.sh` mewajibkan `llvm-config` host yang major-nya
  sama dengan LLVM target, mewajibkan wrapper target bisa dieksekusi dan
  melaporkan versi yang sama, lalu memakai keduanya di `config.toml`;
- job CI `rustc-cargo` memasang `llvm-<major>-dev` untuk runner (fallback ke
  tarball rilis resmi `LLVM-<ver>-Linux-X64.tar.xz`) dan mengekspor
  `HOST_LLVM_CONFIG`, sedangkan job `llvm` memverifikasi wrapper target karena
  skrip itu memang bisa dijalankan di runner.

Konsekuensi: mesin build butuh `python3` untuk `octagram-llvm-config` (sudah
menjadi syarat `build_llvm_octagram.sh` dan tersedia di runner), dan runner butuh
arsip statis `libLLVM*.a` milik LLVM host karena compiler untuk mesin build ikut
ditautkan ke LLVM.

Invariant yang harus dijaga saat mengubah driver atau sysroot:

- objek target **tidak boleh** PIC: TLS OCTAGRAM hanya local-exec, dan objek
  PIC akan memanggil `__tls_get_addr` yang tidak ada di libc;
- `errno` **harus** dilihat sebagai TLS (`OCTAGRAM_TLS_ERRNO`) agar tipe
  referensinya sama dengan `libc.a`/`libc++.a`;
- `CMAKE_SYSTEM_NAME` tetap `Generic` (supaya `CMAKE_CROSSCOMPILING` benar),
  sedangkan flag `UNIX` disetel ulang setelah `project()`;
- header yang disertakan LLVM dari C++ wajib punya `extern "C"` dan tipe
  bernama, karena `tools/check_cpp_headers.py` hanya menguji header secara
  terpisah, bukan seluruh rantai include.
