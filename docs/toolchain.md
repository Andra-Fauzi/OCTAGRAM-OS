# Toolchain native

OCTAGRAM bisa membangun program di dalam OS sendiri. Ada dua jalur: toolchain
mini di kernel (`kernel/src/toolchain.rs`) dan TinyCC yang dibangun ke dalam
filesystem (`user/third_party/tinycc`).

## Toolchain mini kernel

`kernel/src/toolchain.rs` adalah assembler, linker, dan compiler C-subset yang
menulis ELF64 x86_64 langsung ke VFS. Perintah shell:

```
as input.s -o output.o     # assembler
ld input.s -o output.elf   # link objek/sumber menjadi ELF
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
