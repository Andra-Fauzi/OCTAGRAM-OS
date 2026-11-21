# Toolchain native

OCTAGRAM bisa membangun program di dalam OS sendiri. Ada dua jalur: toolchain
mini di kernel (`kernel/src/toolchain.rs`) dan TinyCC yang dibangun ke dalam
filesystem (`user/third_party/tinycc`).

## Toolchain mini kernel

`kernel/src/toolchain.rs` adalah assembler, linker, dan compiler C-subset yang
menulis ELF64 x86_64 langsung ke VFS. Perintah shell:

```
as input.s -o output.o     # assembler toolchain mini
ld input.o [more.o ...] -o output.elf # link objek mini atau ELF64 relocatable
cc input.c -o output.elf   # compile C-subset dengan include
```

Batas yang dipaksakan (melebihi batas = error, bukan output rusak):

| Batas | Nilai |
| --- | --- |
| Ukuran sumber C | 8 KiB |
| Ukuran assembly | 256 KiB |
| Kedalaman ekspresi C | 64 |
| Operasi per ekspresi | 64 |
| Kedalaman kontrol | 32 |
| Kedalaman include | 8 |
| Jumlah berkas include | 32 |
| Total byte include | 32 KiB |
| Ukuran kode keluaran | 512 KiB |

ELF keluaran memakai base `0x400000`, satu segmen load di offset `0x1000`,
dan header standar 64 byte + program header 56 byte. Compiler hanya menerima
subset C yang didokumentasikan oleh pesan `UnsupportedC`/`UnsupportedPreprocessor`;
bukan pengganti GCC.

### Linking ELF64 relocatable (`tcc -c`)

`ld` mendeteksi objek ELF lewat magic `\x7fELF` dan memakai
`toolchain::link_elf_objects`; objek format toolchain mini (`OCTOBJ1`) tetap
memakai jalur lama. Linker ELF menerima satu atau beberapa objek `ET_REL`
x86_64, menyelesaikan simbol global/weak lintas object, dan menghasilkan
executable statis dengan dua segmen:

| Segmen | Isi | Flag |
| --- | --- | --- |
| teks | section `SHF_ALLOC` + `SHF_EXECINSTR` (`.text`) | `R|X` |
| data | section `SHF_ALLOC` non-eksekusi (`.data`, `.rodata`), GOT, template TLS, dan `.bss` | `R|W` |
| TLS | `PT_TLS` untuk section `SHF_TLS` (`.tdata` + `.tbss`) | `R` |

Relokasi yang didukung: `R_X86_64_64`, `PC32`, `PLT32`, `32`, `32S`,
`GOTPCREL`/`GOTPCRELX`/`REX_GOTPCRELX`/`GOTPC32` (slot GOT writable per simbol),
TLS local-exec `TPOFF32`/`TPOFF64`, serta initial-exec `GOTTPOFF` (slot berisi
`S - TP`; template `PT_TLS`, thread pointer di aligned end blok sesuai
`user/runtime.c`). Definisi global kuat ganda, simbol
kuat undefined, COMMON, PLT dinamis, dynamic TLS (`TLSGD`/`TLSLD`/`GOTTPOFF`),
dan arsip gagal dengan error spesifik: arsip `.a` justru didukung lewat
`link_elf_inputs` (ekstraksi lazy per member). Relokasi lain gagal dengan
`UnsupportedRelocation` alih-alih menghasilkan binary rusak. Simbol undefined
non-weak dianggap error (`UndefinedSymbol`), common symbol ditolak
(`UnsupportedCommonSymbol`), dan entry point diambil dari simbol `_start`
(`NoEntryPoint` bila tidak ada). `.bss` ditempatkan setelah data dengan
`p_memsz` lebih besar dari `p_filesz`.

Batasan yang masih ada: PLT dinamis dan dynamic TLS (`TLSGD`/`TLSLD`) belum ada;
`TPOFF`/`GOTTPOFF` hanya berlaku di dalam satu executable statis, dan program
hasil `ld` native belum otomatis menyertakan bootstrap TLS `crt0`/runtime.

Uji host: `cargo +nightly test ... toolchain::tests::` mencakup link objek
`ET_REL` multi-segmen, resolusi simbol lintas objek, ekstraksi arsip, relokasi
GOT, template TLS + `TPOFF64`, dan penolakan objek tanpa simbol `_start`.

Setelah menulis output, `ld` mencetak `ld: entry point 0x…` dari `e_entry` ELF
hasil link. Baris ini dipakai untuk membandingkan alamat entry dengan RIP pada
pesan page fault guest tanpa disassembler host; helper `elf_entry_address`
diuji terpisah untuk input non-ELF dan buffer terpotong.

## TinyCC di dalam OS

`user/Makefile` membangun TinyCC `ONE_SOURCE=0` dengan target
`TARGETOS_OCTAGRAM`, lalu menghasilkan:

- `build/tcc.elf` — compiler;
- `build/libtcc1.a` — runtime support;
- `build/libc.a` — libc statis untuk program yang dikompilasi di OS.

Direktori yang diharapkan saat runtime: `/lib/tcc` (binary dan library),
`/include` dan `/include/sys` (header), `/share/octagram` (data pendukung).
Header yang dipasang adalah header `user/include/`, sehingga program yang
dikompilasi TinyCC memakai ABI dan deklarasi yang sama dengan program bawaan.

Build userland memakai baseline `-march=x86-64 -mtune=generic` dan secara
eksplisit menonaktifkan AVX/AVX2/AVX-512. Kernel belum menyediakan kontrak
penyimpanan/restorasi state AVX (XSAVE/XCR0) untuk semua jalur eksekusi dan
context switch, jadi binary userland harus tetap pada baseline x86-64. CI
memeriksa disassembly `tcc.elf` agar instruksi AVX/VEX tidak masuk ke image
guest secara tidak sengaja.
dikompilasi TinyCC memakai ABI dan deklarasi yang sama dengan program bawaan.

Perintah `tcc` kini menjadi alias shell untuk executable native `/bin/tcc.elf`.
Ia menerima argumen TinyCC secara langsung, sehingga pola compile dan eksekusi
program sederhana adalah:

```
tcc -o /tmp/a.elf /tmp/a.c
run /tmp/a.elf
```

Perintah `cc` tetap menjalankan compiler C-subset eksperimental di kernel;
`as` dan `ld` juga tetap memakai toolchain mini. Jangan samakan ketiganya
dengan assembler/linker ELF lengkap. Jalur TinyCC perlu divalidasi dengan
program nyata dan tidak otomatis membuktikan seluruh kompatibilitas C/POSIX.

## Target Rust userland

`make -C user rust` membangun userland Rust `no_std` di `user/rust/`. Target ini
memakai `crt0` yang sama dan `exit` weak agar program tanpa `std` tetap
ter-link.

## Kapan memakai yang mana

- **Toolchain mini**: eksperimen cepat di dalam OS, berkas kecil, tanpa
  dependency, dan untuk menguji assembler/linker.
- **TinyCC**: program C yang lebih besar, selama masih dalam batas header yang tersedia.
- **GCC lintas (host)**: program utama di `user/examples`; ini jalur yang dipakai
  CI/build normal dan paling teruji.

## Batasan

- Toolchain mini bukan compiler C lengkap: tanpa struct kompleks, pointer
  aritmetika lanjutan, atau preprocessor penuh.
- TinyCC di OS belum punya debugger, profiler, atau `make`.
- Tidak ada package manager compiler; pembaruan TinyCC dilakukan dengan
  mengganti sumber di `user/third_party/tinycc`.

## Native build utility (OCTAGRAM)

The Rust `/bin/make` utility is a deliberately limited native build orchestrator.
It supports explicit targets and prerequisites, simple variable assignments
(`=`, `?=`, `+=`), recursive variable expansion, automatic variables `$@`, `$<`,
`$^`, `-C`, `-f`, `-B`, `-n`/`--dry-run`, `-s`/`--silent`, and serial `-j1`.
It currently rebuilds recipes on each invocation; timestamp-based incremental
builds, include directives, pattern rules, parallel jobs, and shell operators
are not implemented.

Example inside OCTAGRAM:

```sh
cd /share/octagram
make -n
make -j1 -n
make
```

The QEMU native-toolchain smoke test checks TinyCC compile/execute and the
native make dry-run expansion. A passing host build alone does not prove these
commands work in the guest; use the QEMU CI result as the runtime gate.