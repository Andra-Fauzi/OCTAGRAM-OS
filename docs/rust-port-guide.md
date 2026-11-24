# Panduan port Rust/C++ ke OCTAGRAM (M4.3 → M4.4)

Dokumen ini adalah panduan kerja: status port, cara build, cara uji, peta file,
dan urutan langkah berikutnya. Rencana keputusan backend ada di
[rust-m4-3-backend-plan.md](rust-m4-3-backend-plan.md); latar belakang ABI ada
di [rust-native-bootstrap.md](rust-native-bootstrap.md).

Tujuan akhirnya: `rustc` dan `cargo` berjalan **di dalam** OCTAGRAM dan bisa
dipanggil dari shell, bukan cross-build dari host.

## 1. Status saat ini

| Bagian | Status | Bukti |
| --- | --- | --- |
| Rust `no_std + alloc` guest | jalan | `RUST-NOSTD-PASS` (uji guest) |
| Upstream Rust `std` di guest | jalan | `RUST-STD-QEMU-PASS`, 54 marker `RUST-STD-*` |
| `rustc --emit=obj` → `ld` native | terbukti di host | uji `toolchain::tests::links_a_rustc_emitted_object_into_a_runnable_executable`, marker guest `RUST-NATIVE-LD-PASS` sudah terpasang |
| C++ freestanding (`operator new`, vtable) | terbukti di host | `build/cpp_smoke.elf`, marker guest `CPP-FREESTANDING-PASS` sudah terpasang |
| Konstruktor/destruktor statis C++ | terbukti di host | `.init_array`/`.fini_array` di `user/linker.ld` + loop di `user/crt0.S` |
| Runtime C++ ABI minimal | terbukti di host | `user/cxx_runtime.cpp` |
| `pthread_once` (dipakai `std::call_once`) | terbukti di host | `kernel`/`runtime.c`, uji di `/bin/cpp-smoke` |
| Klasifikasi wide-character (`<wctype.h>`) | terbukti di host | `user/wctype.c` + `make -C user wctype-test` |
| `thread_local` C++ (destruktor per-thread) | terbukti di host | `__cxa_thread_atexit` di `runtime.c`, uji di `/bin/cpp-smoke` |
| compiler-rt 128-bit (`__udivmodti4`, shift) | terbukti di host | `user/compiler_rt.c` + `make -C user compiler-rt-test` |
| compiler-rt float↔128-bit (`__floatuntidf`, `__fixunsdfti`) | terbukti di host | `user/compiler_rt.c`, uji di `/bin/cpp-smoke` |
| compiler-rt `long double`↔128-bit (`__floatuntixf`, `__fixunsxfti`) | terbukti di host | `user/compiler_rt.c` (format x87 manual) |
| Header publik valid untuk C dan C++ | terbukti di host | `python3 tools/check_cpp_headers.py` (57 header) |
| libc++/libstdc++ subset, compiler-rt | **belum** | — |
| libLLVM untuk host OCTAGRAM | **belum** | — |
| `rustc` native (`rustc --version` di guest) | **belum** | — |
| `cargo` native | **belum** | — |

Catatan: “terbukti di host” berarti build/taut/link diperiksa di mesin
pengembang. Marker guest sudah ada di harness, tetapi **belum dijalankan pada
QEMU di workspace ini**; jalankan CI atau QEMU lokal untuk mengubah statusnya
menjadi terbukti di guest.

## 2. Arsitektur yang mengikat

Tiga triple yang tidak boleh dicampur:

| Peran | Triple | Keterangan |
| --- | --- | --- |
| Build | `x86_64-unknown-linux-gnu` | mesin yang menjalankan Rust/Cargo saat ini |
| Host (compiler masa depan) | `x86_64-unknown-octagram` | tempat `rustc`/`cargo` akan dieksekusi |
| Target (kernel & aplikasi) | `x86_64-unknown-none` | kernel freestanding |

Batasan userspace yang harus dihormati port:

- ELF statis `ET_EXEC`, base `0x400000`, entry `_start`, linker script
  `user/linker.ld`; tidak ada dynamic loader untuk toolchain.
- Syscall lewat `int 0x80`, nomor di RAX, argumen RDI/RSI/RDX
  ([syscall-abi.md](syscall-abi.md)); ini **bukan** ABI Linux.
- TLS hanya local-exec/initial-exec; tidak ada TLS pada shared object
  ([dynamic-linker.md](dynamic-linker.md)).
- `sendmsg`/`recvmsg` masih `ENOSYS`; Cargo dengan registry jaringannya belum
  bisa diasumsikan jalan.
- Pencarian program di shell: `/bin`, `/usr/bin`, `/usr/local/bin`,
  `/toolchain/bin` ([shell.md](shell.md)). Toolchain native nantinya ditaruh di
  `/toolchain/bin` supaya bisa dipanggil dengan nama saja.

## 3. Peta file

Kernel (penyedia layanan untuk toolchain):

| Path | Peran |
| --- | --- |
| `kernel/src/shell.rs` | PATH default, dispatch perintah, perintah `as`/`ld`/`cc` |
| `kernel/src/toolchain.rs` | assembler, linker ELF64 native, compiler C-subset, uji hostnya |
| `kernel/src/fs/mod.rs` | menanam ELF/object ke image (`/bin/...`, `/share/octagram/...`) |
| `kernel/src/elf.rs` | loader ELF, syscall |
| `kernel/src/syscall` (lihat `docs/syscall-abi.md`) | kontrak ABI |

Userspace C/C++:

| Path | Peran |
| --- | --- |
| `user/crt0.S` | `_start`: TLS, errno, `.init_array`, `main`, `.fini_array`, exit |
| `user/linker.ld` | layout ELF statis, PT_TLS, `.init_array`/`.fini_array` |
| `user/runtime.c` | libc bootstrap (`libc.a`), termasuk `__cxa_atexit`/`atexit` |
| `user/cxx_runtime.cpp` | `operator new/delete`, `__cxa_guard_*`, `__cxa_pure_virtual`, `__dso_handle` |
| `user/include/*.h` | header publik; header C yang dipakai C++ wajib punya `extern "C"` |
| `user/examples/cpp_smoke.cpp` | program C++ freestanding pertama |
| `user/Makefile` | build userspace C/C++ (`CXXFLAGS` freestanding) |

Rust userspace & port:

| Path | Peran |
| --- | --- |
| `user/rust/targets/x86_64-unknown-octagram.json` | target spec |
| `user/rust/runtime/` | `octagram-runtime` (`no_std`, syscall wrapper, allocator) |
| `user/rust/hello/`, `make/` | program Rust `no_std` |
| `user/rust/native-probe/` | object `ET_REL` untuk uji `ld` native |
| `user/rust/std-probe/` | probe upstream `std` (dibangun di CI) |
| `user/rust/Makefile` | cross-build host → ELF/object OCTAGRAM |
| `patches/rust/`, `patches/libc/`, `patches/rust-libc/`, `patches/rust-std/` | patch upstream yang dipin |
| `tools/patch_rust_std_octagram.py`, `tools/apply_rust_libc_octagram.py` | aplikator patch |
| `tools/qemu_syscall_test.py` | harness guest (menjalankan semua program uji) |
| `docs/rust-m4-3-backend-plan.md` | keputusan backend + tahapan |

## 4. Cara build

Dependensi host (Debian/Ubuntu): `build-essential binutils nasm lld xorriso
mtools gdisk qemu-system-x86 python3 curl git` + Rust nightly (`rust-src`,
`rustfmt`, `llvm-tools`).

```sh
rustup toolchain install nightly
rustup component add --toolchain nightly rust-src rustfmt llvm-tools
# ld.lld untuk link Rust userspace:
export PATH="$(rustc +nightly --print sysroot)/lib/rustlib/x86_64-unknown-linux-gnu/bin/gcc-ld:$PATH"
```

Urutan build penting karena kernel menanam artefak userspace dengan
`include_bytes!`:

```sh
make -C user/rust make          # make.elf + build/rust_native_probe.o
make -C user                    # ELF C + build/cpp_smoke.elf (+ tcc, libc.a, ...)
make -C user dynamic            # ld-octagram.so + liboctagram.so
make -C kernel KARCH=x86_64     # kernel
make all                        # ISO
make run                        # boot ISO di QEMU
```

Target khusus:

```sh
make -C user/rust all check                  # semua program Rust + readelf check
make -C user/rust build/rust_native_probe.o  # object ET_REL untuk uji ld native
make -C user build/cpp_smoke.elf             # program C++ freestanding
make -C user wctype-test                     # uji host klasifikasi wide-character
make -C user compiler-rt-test                # helper 128-bit vs pembagi libgcc
make -C user cpp-header-test                 # semua header publik sebagai C dan C++
```

Catatan: `make -C user/rust build/rust_native_probe.o` memakai linker no-op
(`-C linker=true`) hanya untuk melewati tahap link, karena yang diuji adalah
object `rustc`, bukan hasil link host.

## 5. Cara uji

### 5.1 Uji host (cepat, tanpa QEMU)

```sh
cargo +nightly fmt --manifest-path kernel/Cargo.toml --check
cargo +nightly test --manifest-path kernel/Cargo.toml --target x86_64-unknown-linux-gnu
cargo +nightly test --manifest-path kernel/Cargo.toml --target x86_64-unknown-linux-gnu toolchain::tests
```

Uji yang relevan untuk port:

- `toolchain::tests::links_a_rustc_emitted_object_into_a_runnable_executable` —
  linker native menerima object `rustc` (section symbol, `R_X86_64_32`).
- `toolchain::tests::links_*` — relokasi GOT/PLT/TLS/SIZE yang dibutuhkan
  object Rust yang lebih besar.

Jika `kernel/` dikompilasi tanpa artefak `user/build`/`user/rust/build`,
`include_bytes!` akan gagal — itu memang urutan build yang benar, bukan bug.
Menjalankan `rustfmt --check` pada repo ini bisa gagal karena selisih versi
rustfmt dengan ratusan baris lama; fokuskan pemeriksaan pada file yang diubah.

### 5.2 Uji guest (QEMU)

CI melakukannya seperti ini (lihat `.github/workflows/posix-core-stage1.yml`):

```sh
qemu-system-x86_64 -M q35 -cdrom OCTAGRAM-x86_64.iso -boot d -m 2G \
  -display none -serial file:/tmp/serial.log \
  -monitor unix:/tmp/mon.sock,server,nowait -no-reboot -no-shutdown &

MON=/tmp/mon.sock SERIAL=/tmp/serial.log python3 tools/qemu_syscall_test.py
```

Harness mengetikkan perintah ke shell lewat monitor QEMU dan memeriksa output
serial. Marker yang harus muncul:

| Marker | Arti |
| --- | --- |
| `RUST-NOSTD-PASS` | Rust `no_std + alloc` berjalan |
| `RUST-STD-QEMU-PASS` | upstream `std` berjalan (54 sub-marker) |
| `CPP-FREESTANDING-PASS` | C++ freestanding, `new/delete`, konstruktor statis |
| `RUST-NATIVE-LD-PASS` | object `rustc` ditautkan `ld` native lalu dijalankan |
| `SYSCALL-TEST: PASS` | suite kompatibilitas syscall |

## 6. Cara menambah tahap port

Ikuti urutan ini supaya klaim tetap bisa dipertanggungjawabkan:

1. **Bukti host dulu.** Tambahkan uji di `kernel/src/toolchain.rs` atau build
   artefak dengan `make -C user ...`, lalu pastikan perintah build/uji lulus.
2. **Tanam ke image.** Daftarkan artefak di `kernel/src/fs/mod.rs`
   (`include_bytes!` + `seed_file`).
3. **Tambahkan langkah guest.** Tulis perintah + marker di
   `tools/qemu_syscall_test.py` (pola `send_line` → `wait_for_after`).
4. **Perbarui dokumentasi & ROADMAP.** Tandai apa yang host-tested dan apa yang
   masih menunggu guest.
5. **Jalankan QEMU/CI.** Baru setelah marker terlihat, status boleh disebut
   guest-tested.
6. **Jangan menaikkan status M4/M4.3** sebelum `rustc --version` dan kompilasi
   crate benar-benar terjadi di guest.

## 7. Troubleshooting yang sudah ditemui

| Gejala | Penyebab | Solusi |
| --- | --- | --- |
| `linker ld.lld not found` | komponen LLVM belum ada | `rustup component add llvm-tools`, tambahkan `gcc-ld/` ke PATH |
| `undefined reference to printf(char const*, ...)` | header C dipakai dari C++ tanpa `extern "C"` | tambahkan guard seperti di `stdio.h`/`stdlib.h` |
| `redeclaration of C++ built-in type wchar_t` | `stddef.h` mem-typedef `wchar_t` | guard `#ifndef __cplusplus` |
| `expected ',' or '...' before 'template'` | parameter memakai keyword C++ (`mkstemp`) | ganti nama parameter |
| konstruktor global tidak jalan | `.init_array` tidak ada / tidak dipanggil | section di `linker.ld` + loop di `crt0.S` |
| `operator new` undefined | runtime C++ belum ditautkan | tautkan `build/cxx_runtime.o` |
| `couldn't read user/rust/build/*.elf` | artefak userspace belum dibangun | jalankan `make -C user/rust` lalu `make -C user` |
| `expected ',' or '...' before 'wide'` | header memakai `restrict`, yang bukan keyword C++ | shim `#define restrict __restrict` di header (`wchar.h`, `spawn.h`) |
| `char *const argv[restrict]` gagal di C++ | `restrict` di dalam `[]` parameter tidak valid di C++ | tulis `char *const argv[]` (kualifikasi itu redundan) |
| `redeclaration of C++ built-in type char16_t` | typedef C1X bentrok dengan keyword C++11 | guard `#ifndef __cplusplus` di `uchar.h` |
| `_Atomic does not name a type` | `#include_next <stdatomic.h>` milik GCC hanya untuk C | guard C++ di `stdatomic.h` |
| `uint64_t/size_t has not been declared` | header tidak mandiri | tambahkan include yang hilang; `tools/check_cpp_headers.py` menangkapnya |
| `undefined reference to __udivmodti4` | kode memakai `__int128`/`u128` dan helper belum ikut di-link | tambahkan `build/compiler_rt.o` ke baris `ld` (sudah default untuk `%.elf`) |
| `undefined reference to __floatuntidf` | cast `u128`/`__int128` ↔ `double`/`float` | helper ada di `compiler_rt.c`; pastikan object itu ikut di-link |
| `long double` menghasilkan nilai aneh | format x87 80-bit berbeda dari IEEE binary64 | helper `__*xfti` di `compiler_rt.c` menangani mantissa 64 bit + bit integer eksplisit |
| `Option 'out-dir' given more than once` | `cargo rustc` sudah memberi `--out-dir` | jangan tambahkan `--out-dir`; ambil `.o` dari `target/.../build/<pkg>/*/out/` |

## 8. Urutan lanjutan

| Tahap | Isi | Gate |
| --- | --- | --- |
| M4.3.2 (lanjutan) | subset libc++/libstdc++ + compiler-rt (`strtold`, `wcs*`, locale/ctype) | test C++ (string/vector/mutex) di guest |
| M4.3.3 | libLLVM statis untuk host OCTAGRAM | satu tool LLVM berjalan di guest |
| M4.3.4 | bootstrap `rustc` host OCTAGRAM + sysroot, linker `ld` native | `rustc --version` dan kompilasi `no_std` dari shell |
| M4.3.5 | perluasan relokasi linker untuk `librustc_driver`/`std` | binary `std` hasil `rustc` native berjalan |
| M4.4 | Cargo native/offline | `cargo build --offline` di guest |

## 9. Aturan pelaporan

Pisahkan tiga tingkat bukti: **host build/test**, **QEMU guest**, dan
**hardware**. Jangan menyebut “native” untuk cross-build, jangan menyebut
“lulus” untuk kompilasi yang belum dijalankan, dan catat kegagalan yang
diketahui (misalnya `fs::devfs` yang masih mengharapkan 10 entri `/dev`).## C++ standard library (libc++)

Header libc++ 18.1.8 di-vendor di
[user/third_party/libcxx](../user/third_party/libcxx/README.md) (hanya `include/`,
seperti upstream, plus `__config_site` dan `__assertion_handler` buatan kita).
Program C++ memakainya dengan `-nostdinc` dan urutan `-isystem` yang sama seperti
build guest:

```sh
make -C user build/libcxx_smoke.elf   # std::array/vector/optional/tuple/string_view
make -C user libcxx-header-check      # ratchet: header mana yang sudah bisa dikompilasi
```

Status per gelombang ini: **102 header standar bisa dikompilasi** (semua yang
dilacak kecuali `cstdalign`, `flat_map`, `flat_set` yang memang tidak ada di
libc++ 18). `libcxx-header-check` gagal bila salah satu dari daftar itu mundur.

Yang membuat ini mungkin, dan yang perlu diingat kalau ada yang rusak lagi:

- `__config_site` **wajib** mendefinisikan `_LIBCPP_ABI_NAMESPACE __1`. Tanpa
  itu inline namespace-nya benar-benar bernama `_LIBCPP_ABI_NAMESPACE` dan semua
  simbol `std::__1::…` jadi salah nama (gejalanya: `std::sort` tidak ketemu saat
  link, bukan error kompilasi).
- Checker memakai `-nostdinc` seperti build guest. Tanpa itu, `__has_include`
  libc++ menemukan header glibc host (mis. `<bits/types/mbstate_t.h>`) dan
  bertengkar dengan `mbstate_t` milik OCTAGRAM — persis kegagalan yang dulu
  dilaporkan sebagai "blocker mbstate_t".
- `user/include/features.h` sengaja kosong untuk guest, tetapi melakukan
  `#include_next <features.h>` bila header host masih terjangkau. Tanpa rantai
  itu, `tccrun.c` (yang mencampur header kita dengan `<signal.h>` glibc) gagal
  karena `__BEGIN_DECLS` tidak pernah didefinisikan.

Yang **belum** bisa dipakai bukan lagi header, melainkan isi library libc++ yang
dikompilasi (`libcxx/src`): `std::string` butuh `basic_string::__init`/`append`,
`std::sort` butuh instantiasi eksplisit `std::__1::__sort` (ditandai
`__exclude_from_explicit_instantiation__`), dan `std::iostream` butuh seluruh
locale/facet. Langkah berikutnya adalah membangun `libcxx/src` dengan
`-nostdinc` yang sama dan menautkannya sebagai `libc++.a`.

