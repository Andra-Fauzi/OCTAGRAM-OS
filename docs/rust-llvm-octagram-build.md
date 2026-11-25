# M4.3.3 — Build libLLVM statis untuk `x86_64-unknown-octagram`

Runbook ini menjelaskan cara membangun libLLVM yang menjadi prasyarat `rustc`
native (M4.3.4). Semua langkah berjalan di host Linux; hasilnya adalah library
dan tool LLVM berformat ELF OCTAGRAM yang nantinya dijalankan di guest.

Status: **scaffolding**. Skrip, toolchain file, dan sysroot sudah ada di repo,
tetapi belum pernah dijalankan sampai selesai di workspace mana pun. Jangan
menandai M4.3.3 selesai sebelum gate di §5 lulus di QEMU.

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
- **Belum ada CI job.** Skrip ini belum dipanggil workflow mana pun karena
  durasi build dan kebutuhan artefak sysroot belum diputuskan.
