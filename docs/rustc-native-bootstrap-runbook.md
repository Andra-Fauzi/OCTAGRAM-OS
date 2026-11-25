# M4.3.4 — Bootstrap `rustc` dan Cargo untuk host `x86_64-unknown-octagram`

Runbook ini adalah lanjutan langsung dari
[rust-llvm-octagram-build.md](rust-llvm-octagram-build.md) (M4.3.3): dengan
libLLVM statis dan sysroot tersedia, langkah berikutnya adalah membangun
compiler yang berjalan **di dalam** OCTAGRAM, lalu memakainya dari shell.

Status: **scaffolding**. Skrip persiapan (`tools/bootstrap_rustc_octagram.sh`)
dan sysroot/compiler driver sudah ada; bootstrap penuh belum pernah dijalankan.
Gate M4.4 tetap belum dimulai.

## 1. Pembagian peran triple

| Peran | Triple | Catatan |
| --- | --- | --- |
| Build (host build) | `x86_64-unknown-linux-gnu` | menjalankan stage0 `rustc`/`x.py` |
| Host compiler baru | `x86_64-unknown-octagram` | `rustc`/`cargo` yang lahir dari bootstrap |
| Target program | `x86_64-unknown-none` | kernel dan aplikasi freestanding |

Bootstrapping compiler berarti **cross-compiling compiler**: stage0 di Linux
menghasilkan `rustc` yang merupakan ELF OCTAGRAM. Bukan cross-build program
biasa, karena `std` untuk host baru harus dibangun lebih dulu.

## 2. Prasyarat perangkat lunak

1. `make -C user sysroot` — sysroot target.
2. `tools/build_llvm_octagram.sh` — `llvm-config` statis (M4.3.3).
3. Rust nightly stage0 dari `rust-toolchain.toml` checkout Rust yang dipin,
   dengan komponen `rust-src` dan `rustfmt`.
4. Checkout `rust-lang/libc` pada `0.2.191`
   (`ccc2ff4aaab962f7fde7b8edda7379c045b1ca12`) untuk
   `tools/apply_rust_libc_octagram.py`.

## 3. Host target definition

Bootstrap menolak host triple yang hanya ada sebagai JSON, jadi target harus
dikenal `rustc_target` sendiri. Itu dikerjakan
`tools/apply_rust_octagram_host_target.py`, yang menerapkan tiga perubahan pada
checkout Rust yang dipin:

```
compiler/rustc_target/src/spec/targets/x86_64_unknown_octagram.rs  (file baru)
compiler/rustc_target/src/spec/mod.rs   Os::Octagram + supported_targets!
```

Isinya mengikuti `user/rust/targets/x86_64-unknown-octagram.json`:

- basis `X86_64UnknownNone` (atau `X86_64UnknownLinuxGnu` tanpa `env`) untuk
  data layout dan fitur `+sse,+sse2`;
- `target_family: unix`, `os: "octagram"`, `env: ""`;
- `linker_flavor: Gnu`, `linker: Some("x86_64-octagram-gcc")` supaya rustc
  memakai driver di `user/toolchain/bin` (bukan `ld.lld` yang tidak diport);
- `crt_static_default: true`, `crt_static_respected: true`,
  `relocation_model: Static`, `panic_strategy: Abort`,
  `disable_redzone: true`, `has_thread_local: true`, `max_atomic_width: 64`;
- `dynamic_linking: false`, `position_independent_executables: false`,
  `eh_frame_header: false`.

JSON spec yang sekarang dipakai `-Z build-std=core,alloc` tetap dipertahankan
untuk build target dari host; keduanya harus dijaga sinkron. Aplikator
memverifikasi revisi upstream (`342d744…`), memakai anchor unik untuk setiap
sisipan, dan idempoten, sehingga aman dijalankan ulang setelah kegagalan
sebagian. Skrip bootstrap memanggilnya otomatis pada langkah 3.

`python3 tools/check_octagram_rust_target_sync.py` membandingkan kedua spec
secara statis (tanpa rustc): ia menurunkan nilai ABI dari JSON lalu memastikan
definisi builtin memakai `TargetOptions` yang sama. Jalankan setiap kali salah
satu file berubah.

## 4. Menjalankan bootstrap

Persiapan saja (menulis `config.toml`, menerapkan patch, tanpa compile):

```sh
tools/bootstrap_rustc_octagram.sh \
  --llvm-prefix build/toolchain/llvm-21.1.0-octagram \
  --libc-checkout build/toolchain/libc
```

Build penuh:

```sh
tools/bootstrap_rustc_octagram.sh \
  --llvm-prefix build/toolchain/llvm-21.1.0-octagram \
  --libc-checkout build/toolchain/libc \
  --build --jobs "$(nproc)"
```

Perintah `x.py` yang dijalankan skrip:

```sh
cd build/toolchain/rust
./x.py build --stage 2 --host x86_64-unknown-octagram \
  compiler/rustc src/tools/cargo -j N
```

Konfigurasi penting di `config.toml` yang dihasilkan:

| Kunci | Nilai | Alasan |
| --- | --- | --- |
| `build.host` | `x86_64-unknown-octagram` | compiler yang dihasilkan adalah program OCTAGRAM |
| `build.target` | `x86_64-unknown-none` | program hasil build tetap freestanding |
| `rust.llvm-config` / `target.<host>.llvm-config` | prefix M4.3.3 | memakai libLLVM statis, bukan LLVM CI |
| `target.<host>.linker` | `user/toolchain/bin/x86_64-octagram-gcc` | driver menambahkan `crt0.o`, `libc.a`, `libc++.a` |
| `rust.lld` / `rust.use-lld` | `false` | LLD tidak diport |
| `rust.jemalloc`, `rust.profiler`, `rust.backtrace` | `false` | mengurangi permukaan port |
| `rust.channel` | `nightly` | bootstrap butuh nightly |
| `llvm.download-ci-llvm` | `false` | LLVM harus benar-benar target OCTAGRAM |

Hasil stage 2 berada di `build/<host>/stage2/bin/{rustc,cargo}` dan disalin
skrip ke `build/toolchain/octagram-toolchain/` beserta `lib/rustlib`.

## 5. Memasang ke guest

`/toolchain/bin` sudah ada di `PATH` shell OCTAGRAM dan dicadangkan untuk
toolchain native (`kernel/src/shell.rs`), jadi tidak ada perubahan pencarian
program yang diperlukan. Yang belum ada adalah mekanisme menyalin toolchain ke
image: kernel menanam program lewat `include_bytes!`, yang tidak layak untuk
puluhan MB `rustc` + `lib/rustlib`.

Pilihan yang tersedia untuk langkah image:

1. **Root ext2 persisten** — sudah tersedia lewat script host:

   ```sh
   sudo tools/install-disk.sh --toolchain build/toolchain/octagram-toolchain /dev/sdX
   ```

   `bin/` dan `lib/` disalin ke `/toolchain` saat disk dipasang, sehingga
   `rustc`/`cargo` langsung ditemukan lewat PATH setelah boot. Ini jalur
   Linux/WSL untuk pengembang; installer internal dan `selfupdate` di guest
   belum punya langkah yang setara.
2. **Paket `/apps`** — bungkus toolchain sebagai paket `pkg`, dengan
   `/apps/<nama>/bin` di `PATH`; ukuran paket perlu diukur dulu.
3. **Image builder native** — `kernel/src/image_builder.rs` tumbuh menjadi
   penulis image yang menyertakan toolchain.

Sampai salah satunya dipilih dan diuji, `rustc`/`cargo` hasil bootstrap belum
"bisa dipakai di shell": binary-nya valid sebagai ELF OCTAGRAM tetapi belum
sampai ke guest. Langkah ini tidak boleh dilewati dalam pelaporan.

## 6. Gate M4.3.4 / M4.4

Semua harus direkam sebagai uji guest, bukan build host:

| Gate | Perintah di shell OCTAGRAM |
| --- | --- |
| compiler jalan | `/toolchain/bin/rustc --version` |
| kompilasi `no_std` | `rustc --target x86_64-unknown-none hello.rs -o /tmp/hello && run /tmp/hello` |
| Cargo offline | `cargo build --offline --manifest-path /cargo-demo/Cargo.toml` |
| Cargo memanggil rustc native | log `-v` menunjukkan `rustc` dari `/toolchain/bin`, bukan host |
| Stage 1 → Stage 2 | toolchain hasil build membangun ulang dirinya dan artefaknya sama |

Driver Cargo di guest menambahkan `-C linker=tcc` pada setiap pemanggilan
`rustc`, karena linker bawaan target (`x86_64-octagram-gcc`) hanya ada di host
dan satu-satunya linker executable di image adalah TinyCC. Belum terbukti tcc
mau menautkan object `librustc_driver`/`std`; bila gagal, pasang linker native
lalu set `OCTAGRAM_RUST_LINKER`. Bootstrap di host tidak terpengaruh karena
`config.toml` menetapkan `target.<host>.linker` sendiri.

Harness `tools/qemu_syscall_test.py` sudah menjalankan gate ini secara otomatis bila
`/toolchain/bin/rustc` ada di image: `rustc --version`, lalu
`cargo build --offline` pada `/cargo-demo`, dengan marker
`OCTAGRAM-RUSTC-PASS`. Image tanpa toolchain mencetak `OCTAGRAM-RUSTC-SKIP`
dan tidak menggagalkan run.

`/bin/cargo` di image saat ini adalah `octagram-cargo` (driver C++ offline,
lihat [user/cargo/README.md](../user/cargo/README.md)). Ia sudah menjadi jalur
`plan`/`build`/`metadata`/`clean`, tetapi `build` tetap membutuhkan `rustc`
yang disediakan di atas.

## 7. Batas dan risiko

- **libc++ belum lengkap untuk `librustc_driver`.** `std::filesystem`, thread,
  dan locale sudah ada, tetapi exception/RTTI dimatikan; rustc dibangun tanpa
  keduanya (`-fno-exceptions -fno-rtti`), jadi setiap tempat di LLVM/rustc yang
  memakai keduanya harus dipatch, bukan diasumsikan jalan.
- **`std` harus lolos PAL Unix** untuk `target_os = "octagram"`; patch 0001
  hanya membuka build gate. `docs/rust-std-pal-design.md` memuat daftar API
  yang masih perlu implementasi.
- **Relokasi linker.** Jika `librustc_driver`/`std` menghasilkan relokasi di
  luar cakupan `kernel/src/toolchain.rs`, fallback adalah memport LLD.
- **Memori.** `rustc` butuh heap besar; batas `mmap` dan allocator guest perlu
  diukur sebelum dinyatakan layak.
- **Belum ada CI.** Job bootstrap belum ditambahkan; artefak `sysroot.json`
  dan `llvm-config --version` harus dicatat saat job itu dibuat.
