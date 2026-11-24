# M4.3 — Rencana backend `rustc` native untuk OCTAGRAM

Dokumen ini melengkapi [rust-native-bootstrap.md](rust-native-bootstrap.md) bagian
M4.3. Isinya: perbandingan opsi backend code generation untuk `rustc` yang
berjalan di OCTAGRAM, keputusan jalur, tahapan kerja, serta bukti langkah
pertama yang sudah diimplementasikan.

Status seluruh dokumen ini: **rencana dan langkah awal**, bukan klaim bahwa
`rustc` native sudah ada. Gate M4.4 (Stage 0 `rustc` + Cargo) tetap belum
dimulai.

## 1. Definisi selesai M4.3

M4.3 selesai jika, direkam sebagai uji guest:

1. satu executable `rustc` hasil build untuk host `x86_64-unknown-octagram`
   dapat dijalankan dari shell OCTAGRAM;
2. compiler tersebut menghasilkan object/executable x86_64 yang dapat
   ditautkan oleh linker OCTAGRAM dan dijalankan di guest;
3. jalur build-nya reproducible (versi LLVM, rustc, libc, dan target spec
   dipin) dan tidak memerlukan host Linux pada saat runtime.

## 2. Batasan yang mengikat keputusan

Dari kode dan dokumen ABI saat ini:

- Userspace OCTAGRAM memakai ELF statis `ET_EXEC`; dynamic loader hanya
  mendukung subset relokasi dan TLS hanya local-exec/initial-exec, tanpa TLS
  pada shared object ([dynamic-linker.md](dynamic-linker.md)).
- Runtime C (`user/runtime.c`, `libc.a`) tersedia untuk static link; tidak ada
  libstdc++/libc++ dan tidak ada compiler C++ yang berjalan di guest.
- Thread (`thread_create`/`futex`), TLS statis, `mmap`/`mprotect`, `pipe2`,
  `fork`/`execve` sudah ada dan diuji di guest, tetapi `sendmsg`/`recvmsg`
  masih `ENOSYS` ([syscall-abi.md](syscall-abi.md)).
- `dlopen` ada tetapi terbatas; memuat backend codegen sebagai `.so` adalah
  risiko tambahan, bukan asumsi aman.
- Linker native (`kernel/src/toolchain.rs`) sudah menangani `R_X86_64_64`,
  `PC32`, `PLT32`, `32`, `32S`, GOT (`GOTPCREL`/`GOTPC32`), TLS local-exec
  (`TPOFF32/64`, `GOTTPOFF`), `SIZE32/64`, `PC64`, serta ekstraksi arsip `.a`
  ([toolchain.md](toolchain.md)).

## 3. Opsi backend

Biaya dalam perkiraan engineer-week, diurutkan dari yang paling kecil untuk
jalur native.

| # | Opsi | Yang harus diport | Biaya | Risiko utama | Catatan bukti |
| --- | --- | --- | --- | --- | --- |
| A | **LLVM statis** untuk host `x86_64-unknown-octagram`, LLD tidak diport; `ld` native OCTAGRAM dipakai sebagai linker eksternal `rustc` | C++ runtime minimal (libc++/libstdc++ subset), compiler-rt, libLLVM statis, bootstrap `rustc` host=octagram, ekstensi linker native | 6–10 | Port C++ runtime + TLS/thread di LLVM; ukuran binary besar; relokasi linker native harus menyusul | `rust.lld=false` didukung; `rustc` dapat memakai linker eksternal via `-C linker` |
| B | LLVM statis **+ port LLD** | Seperti A ditambah LLD dan dependensinya | 8–12 | Sama dengan A ditambah pemeliharaan dua linker | LLD memakai C++ yang sama dengan LLVM |
| C | `rustc_codegen_cranelift` sebagai backend | Tetap butuh binary `rustc` host OCTAGRAM (front end + driver), ditambah Cranelift untuk target baru | 6–9 (tidak lebih murah dari A) | Backend dimuat ke `rustc` yang `rustc_private`-nya harus identik; matrix upstream menandai "other unixes" belum diuji; compiler hasil cg_clif ±15x lebih lambat | rust-lang/rustc_codegen_cranelift issue #1450 (backend harus dilink ke `librustc_driver` yang sama); forum "Compiling rustc with only the cranelift backend" |
| D | `rustc_codegen_gcc` (libgccjit) | Port GCC + libgccjit + C++ runtime, lalu `rustc` host | 12+ | Port GCC jauh lebih besar dari LLVM; libgccjit butuh patch khusus | Komentar bootstrap pada PR rust-lang/rust #144787; dokumen cg_gcc |
| E | Compiler subset / terjemahan Rust→C | Tidak ada; tetapi bukan `rustc` | 4–8 | Tidak dapat membangun `std`, `proc_macro`, atau kernel; jalur ini sudah pernah dicoba dan dihapus dari repo (`xrustc`, `mrustc`) | Riwayat commit: "rust: drop xrustc and mrustc substitute paths" |
| F | Tanpa `rustc` native (cross-build dari host) | — | 0 | Tidak ada kemampuan baru | Status quo; gagal gate M4.3 |

### 3.1. Mengapa cg_clif/cg_gcc bukan jalan pintas

Kedua backend itu **bukan pengganti `rustc`**, melainkan plugin code generation
yang dimuat oleh binary `rustc` yang sudah ada, dan keduanya harus dilink
terhadap `rustc_private` versi yang sama persis. Artinya binary `rustc` untuk
host OCTAGRAM tetap harus di-cross-bootstrap terlebih dahulu.

Selain itu, LLVM tidak dapat dihilangkan dari build `rustc` secara resmi:
`bootstrap.example.toml` menyatakan "the LLVM codegen backend is special and
will always be built and distributed", dan percobaan
`rust.codegen-backends = ["cranelift"]` gagal dengan
`unsupported builtin codegen backend "llvm"`. Jadi port LLVM (opsi A) tetap
berada di jalur kritis, apa pun backend yang dipakai untuk optimasi nanti.

## 4. Keputusan: opsi A

**Jalur yang dipilih: LLVM statis untuk host `x86_64-unknown-octagram`, dengan
linker eksternal `ld` native OCTAGRAM. LLD tidak diport pada tahap ini.**

Alasan:

1. LLVM tidak dapat dihindari untuk `rustc` upstream (lihat §3.1), sehingga
   opsi C dan D hanya menambah komponen tanpa mempersingkat jalur kritis.
2. Tidak memport LLD menghapus satu port C++ besar; `rustc` sudah memanggil
   linker eksternal, dan linker native OCTAGRAM sudah mendukung relokasi yang
   dihasilkan `rustc` untuk object `ET_REL` (dibuktikan di §5).
3. Static linking sesuai dengan ABI userspace OCTAGRAM saat ini dan
   menghindari kebutuhan TLS pada shared object serta `dlopen` untuk backend.
4. Setiap tahap bisa dibuktikan terpisah di guest, jadi kegagalan tidak
   menumpuk di satu langkah raksasa.

Fallback yang disiapkan: jika relokasi yang dibutuhkan `librustc_driver`/
`libstd` ternyata di luar cakupan linker native, port LLD (opsi B) menjadi
rencana cadangan, dan `rustc` tinggal diarahkan ke linker tersebut.

## 5. Tahapan M4.3

Urutan ini sengaja dimulai dari komponen yang bisa diuji hari ini.

| Tahap | Isi | Gate bukti | Perkiraan |
| --- | --- | --- | --- |
| M4.3.0 | `rustc --emit=obj` menghasilkan `ET_REL` untuk `x86_64-unknown-octagram`; `ld` native OCTAGRAM menautkannya; hasilnya dijalankan di guest | Uji host `toolchain::tests::links_a_rustc_emitted_object_into_a_runnable_executable` dan marker `RUST-NATIVE-LD-PASS` di QEMU | **selesai di source; uji QEMU menunggu run** |
| M4.3.1 | Cross-toolchain C/C++ freestanding untuk host OCTAGRAM (`g++` freestanding + sysroot `user/include` + `libc.a`), lalu executable C++ statis pertama | Executable C++ sederhana berjalan di guest | 1–2 |
| M4.3.2 | Runtime C++ minimal (libc++/libstdc++ subset, `-fno-exceptions -fno-rtti`) + compiler-rt untuk target OCTAGRAM | Test C++ (vektor/string/thread/mutex) berjalan di guest | 2–3 |
| M4.3.3 | Build libLLVM statis untuk host OCTAGRAM; uji satu tool LLVM (`llvm-nm`/`llc` mini) di guest | Tool LLVM berjalan di guest | 2–4 |
| M4.3.4 | Bootstrap `rustc` host=`x86_64-unknown-octagram`, target=`x86_64-unknown-none`, `-C linker` = `ld` native; sysroot `core`/`alloc`/`std` | `rustc --version` dan kompilasi crate `no_std` dari shell guest | 2–3 |
| M4.3.5 | Perluasan linker native untuk relokasi yang muncul dari `librustc_driver`/`std`, lalu `rustc` mengompilasi program `std` penuh | Binary `std` hasil `rustc` native berjalan di guest | 1–3 |

Angka di atas adalah perkiraan kasar, bukan komitmen; tahap M4.3.1–M4.3.3
adalah bagian yang paling tidak pasti karena bergantung pada port C++ runtime.

## 6. Langkah awal yang diimplementasikan

### 6.1. M4.3.0 — object `rustc` ditautkan oleh `ld` native

Yang ditambahkan:

- `user/rust/native-probe/`: crate `no_std` minimal yang menghasilkan satu
  object `ET_REL` lewat `rustc --emit=obj` (tanpa crt0, libc, atau runtime).
- `user/rust/Makefile`: target `build/rust_native_probe.o`. Link step
  dilewati dengan linker no-op (`-C linker=true`) supaya yang diuji benar-benar
  keluaran object dari `rustc`.
- `kernel/src/fs/mod.rs`: object ditanam di image sebagai
  `/share/octagram/rust_native_probe.o`.
- `kernel/src/toolchain.rs`: uji host yang menautkan object tersebut dengan
  `link_elf_objects` dan memeriksa `ET_EXEC`, entry `_start`, dan marker.
- `tools/qemu_syscall_test.py`: langkah guest `ld <object> -o <elf>` lalu
  `run <elf>`, dengan marker `RUST-NATIVE-LD-PASS`.

Hasil yang sudah terbukti di host: object `rustc` berisi section per-fungsi
(`.text._start`), `.rodata`, dan satu relokasi `R_X86_64_32` terhadap **section
symbol**; linker native menautkannya menjadi `ET_EXEC` dengan entry dari
`_start`. Tambahan ini penting karena `rustc` memakai section symbol dan
`symtab`, bukan bentuk object yang selama ini diuji linker.

Belum terbukti: eksekusi ELF hasil link di guest (QEMU) — harness sudah
ditambahkan tetapi belum dijalankan pada workspace ini.

### 6.2. M4.3.1 — executable C++ freestanding pertama

Cross-compiler yang dipakai adalah `g++` dari `build-essential` (sudah menjadi
dependensi build repo), bukan clang: target OCTAGRAM adalah x86_64 dengan
`linker.ld` dan `libc.a` sendiri, jadi compiler host cukup dipanggil dengan
mode freestanding (`-ffreestanding -fno-pie -fno-pic -mno-red-zone -nostdlib`).
Menambah clang akan menjadi dependensi baru tanpa keuntungan pada tahap ini.

Yang ditambahkan:

- `user/examples/cpp_smoke.cpp`: program C++ pertama (template `constexpr`,
  kelas dengan virtual dispatch, `enum class`, range-based for) yang tidak
  memakai `new`/`delete`, exception, RTTI, atau header libstdc++.
- `user/Makefile`: `CXXFLAGS` freestanding (`-std=c++17 -fno-exceptions
  -fno-rtti -fno-threadsafe-statics -nostdinc++`) dan target `build/cpp_smoke.elf`.
- `user/include/stddef.h`: `wchar_t` tidak lagi di-typedef untuk C++ (builtin)
  dan `NULL` menjadi `nullptr`; `user/include/stdio.h` mendapat guard
  `extern "C"` seperti `math.h`, karena tanpa itu simbol C termangle menjadi
  `printf(char const*, ...)` dan gagal ditautkan.
- `kernel/src/fs/mod.rs` + `tools/qemu_syscall_test.py`: `/bin/cpp-smoke`
  ditanam di image dan dijalankan oleh harness guest (marker
  `CPP-FREESTANDING-PASS`).

Terbukti di host: ELF statis `ET_EXEC`, entry `0x400000`, dua `PT_LOAD` plus
`PT_TLS`, tanpa dynamic section, dan program C biasa (`hello.elf`,
`syscall_compat_demo.elf`) tetap terbangun setelah perubahan header.
Belum terbukti: eksekusi `/bin/cpp-smoke` di guest QEMU.

### 6.3. M4.3.2 — irisan pertama runtime C++

Libc++/libstdc++ penuh belum disentuh; yang dibangun adalah simbol yang benar-
benar dibutuhkan compiler untuk C++ freestanding, yaitu:

- `user/linker.ld`: section `.init_array`/`.fini_array` beserta
  `__init_array_start/end` dan `__fini_array_start/end`. Tanpa ini konstruktor
  objek global tidak pernah dijalankan.
- `user/crt0.S`: menjalankan `.init_array` sebelum `main()` dan `.fini_array`
  setelah `main()` kembali, sambil menyimpan `argc`/`argv` di register
  callee-saved. Program C murni mendapat daftar kosong, jadi perilakunya tidak
  berubah.
- `user/cxx_runtime.cpp`: `operator new`/`new[]`/`delete`/`delete[]` (termasuk
  varian sized), `__cxa_pure_virtual`, `__cxa_guard_acquire/release/abort`, dan
  `__dso_handle`. Exception, RTTI, dan `std::` tetap di luar cakupan.
- Header C: `stdlib.h` dan `stdio.h` sekarang memakai guard `extern "C"`
  seperti `math.h`, dan parameter `mkstemp(char *template)` diganti nama karena
  `template` adalah keyword C++. Tanpa guard, `free`/`printf` termangle dan
  gagal ditautkan.

Bukti host pada `build/cpp_smoke.elf`: `.init_array` berisi satu pointer ke
`_GLOBAL__sub_I_main`, simbol `__init_array_start/end`, `__fini_array_start/end`
(kosong karena tidak ada destruktor global), `__dso_handle`, dan
`__cxa_pure_virtual` semuanya terdefinisi; disassembly `_start` menunjukkan
loop konstruktor sebelum `call main` dan loop destruktor sesudahnya. Program C
(`hello.elf`, `syscall_compat_demo.elf`) tetap terbangun dengan `.init_array`
kosong.

Irisan berikutnya dari M4.3.2 menutup prasyarat libc++ yang lebih dekat ke
kodenya:

- `user/wctype.c` + `user/include/wctype.h`: klasifikasi wide-character
  (`iswalpha`, `iswspace`, …), `towlower`/`towupper`, `wctype`/`iswctype`, dan
  `wctrans`/`towctrans` untuk ASCII + Latin-1. Diuji host lewat
  `make -C user wctype-test`.
- `pthread_once`: dibutuhkan `std::call_once` dan guarded statics; diuji di
  `/bin/cpp-smoke` (initializer harus berjalan tepat sekali).
- `thread_local` C++: compiler memanggil `__cxa_thread_atexit`, jadi runtime
  menyediakan rantai destruktor per-thread yang dijalankan saat thread keluar
  (juga saat `exit()` untuk thread utama). `__cxa_thread_atexit_impl` disediakan
  sebagai alias karena sebagian runtime memakai nama glibc itu. Diuji di
  `/bin/cpp-smoke` dengan thread yang memiliki `thread_local` berdestruktor.
- Kesiapan header C untuk C++: `tools/check_cpp_headers.py` mengompilasi setiap
  header publik sebagai C11 dan C++17 (`make -C user cpp-header-test`, juga
  dipanggil CI). Perbaikan yang muncul: `restrict` bukan keyword C++, `char16_t`
  adalah keyword C++, `<stdatomic.h>` GCC hanya untuk C, dan beberapa header
  tidak mandiri (`uint64_t`, `size_t`).

Belum terbukti: konstruktor/destruktor, `pthread_once`, dan wide-character
benar-benar berjalan di guest (semuanya sudah masuk `/bin/cpp-smoke`, tetapi
QEMU belum dijalankan di workspace ini). Langkah berikutnya tetap subset libc++
(`<string>`, `<vector>`, `<mutex>`) dan compiler-rt.

## 7. Risiko dan mitigasi

| Risiko | Dampak | Mitigasi |
| --- | --- | --- |
| Port C++ runtime (exception/RTTI) lebih besar dari perkiraan | M4.3.1–M4.3.3 mundur | Bangun LLVM dengan `-fno-exceptions -fno-rtti`, mulai dari subset libc++ yang dipakai LLVM saja |
| Relokasi dari `librustc_driver`/`std` di luar cakupan `ld` native | M4.3.5 gagal | Fallback opsi B (port LLD); jalur `ld` native tetap dipertahankan untuk build kecil |
| TLS/thread di LLVM tidak cocok dengan model local-exec | Crash runtime | Tambah uji conformance TLS/thread per tahap sebelum melanjutkan |
| Ukuran image dan RAM | Build lambat / gagal boot | Simpan toolchain di disk EXT2 (`/toolchain`), bukan di RAMFS; ukur ukuran per tahap |
| `dlopen` terbatas untuk backend codegen | Backend gagal dimuat | Pilih static linking backend (opsi A) dan hindari plugin `.so` |
| Performa compiler di guest | Waktu build tidak praktis | Ukur lebih dulu; optimasi belakangan, jangan mengklaim self-hosting penuh |

## 8. Yang tidak boleh diklaim

- M4.3.0 yang lulus uji host **bukan** bukti `rustc` native; ia hanya
  membuktikan linker native menerima object `rustc`.
- Belum ada `rustc` atau Cargo yang berjalan di OCTAGRAM sampai M4.3.4 dan
  M4.4 lulus uji guest.
- Keberhasilan cross-build dari host tetap dikategorikan cross-build.
