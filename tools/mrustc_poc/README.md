# PoC: `mrustc` sebagai jalur "Rust asli" di OCTAGRAM

Tujuan PoC ini **bukan** membuat `rustc` native, melainkan menguji satu asumsi
paling berisiko sebelum investasi besar: apakah kode C yang dihasilkan `mrustc`
(transpiler Rust → C, ditulis dalam C++) dapat dicerna `tcc` di dalam
OCTAGRAM dan hasilnya berjalan.

Status: **belum lulus end-to-end.** Rincian bukti dan celah yang ditemukan ada
di `docs/native-self-hosting-roadmap.md` bagian **M4.3**.

## Yang sudah terbukti (host)

- `mrustc` (commit terbaru, g++ 13.3) dibangun dari source: `bin/mrustc`
  10.9 MB, sekitar satu menit dengan 8 core.
- Rust `core` 1.90.0 di-transpile menjadi **34.5 MB / 587 ribu baris C**
  (`MRUSTC_TARGET_VER=1.90`, `--edition 2024`, target
  `x86_64-unknown-linux-gnu`, backend `gcc` untuk object-nya).
- Program `no_std` `poc.rs` (loop, static array, slice, `extern "C" write`,
  lang item `start`) menjadi **36 KB C** yang mendefinisikan `main` standar.

## Celah yang ditemukan di sisi guest

1. Header publik `/include/stdint.h` sekarang menyediakan tipe C99
   `int_leastN_t`/`uint_leastN_t` dan `int_fastN_t`/`uint_fastN_t`;
   target pemeriksaan sintaks `stdint-test` tersedia.
2. Bootstrap libm kini tersedia di `user/math.c` dan masuk ke `libc.a`.
   Implementasi mencakup operasi IEEE-754 dasar serta fungsi eksponensial,
   logaritma, trigonometrik, gamma/error-function berbasis aproksimasi.
   `make -C user math-test` menguji host; QEMU harness juga menguji program
   TinyCC yang menautkan math dari `libc.a`. Ini belum correctly-rounded
   dan belum boleh dianggap setara libm matang seperti musl/glibc.
3. TinyCC sekarang mengenali dan menurunkan `__builtin_add_overflow`,
   `__builtin_sub_overflow`, dan `__builtin_mul_overflow` ke helper runtime
   `__tcc_overflow`. Tes host helper tersedia dan QEMU harness menguji parser,
   linker, hasil, serta overflow untuk beberapa lebar integer.
4. Assembler x86 TinyCC kini mengenali `xgetbv` dengan encoding `0F 01 D0`.
   Source `tcc_xgetbv.c` dan dua QEMU harness mengompilasi serta menautkan
   fungsi itu, lalu memeriksa byte `0F 01 D0` lewat `main()` yang tidak
   memanggilnya. Instruksi XGETBV tidak dieksekusi. Atomic 128-bit (`uint128_t _Atomic`) masih belum didukung TinyCC. Rewrite
   PoC mengalihkan jalur yang tidak dieksekusi ke helper bytewise 16-byte;
   helper ini **bukan atomic** dan tidak aman untuk concurrency.

Helper overflow memakai aritmetika lebar untuk memeriksa rentang signed dan
unsigned 8/16/32/64 bit, serta tetap menyimpan bit hasil terpotong saat overflow.
TinyCC belum mendukung integer 128-bit sebagai tipe sumber; builtin overflow
untuk `__int128` belum tercakup. Helper AtomicU128 PoC diuji terpisah melalui
`test_atomic128_shim.c` saat `build_host.sh` berjalan, namun helper itu hanya
simulasi bytewise, bukan operasi atomic/concurrent.
Macro lama yang selalu mengembalikan false sudah dihapus dari `fix.h`.
Status tes QEMU tetap harus dibuktikan; keberadaan harness bukan bukti PASS.
Stub matematika di `shim.c` telah dihapus agar simbol disediakan oleh
`libc.a`. Fungsi transendental di bootstrap libm masih aproksimasi dan
membutuhkan pengujian numerik lebih luas.

`build_host.sh` sekarang memvalidasi bahwa keluaran `mrustc` masih memuat XGETBV
asli, lalu hanya me-rewrite AtomicU128 yang belum didukung. Enam uji regresi
memastikan XGETBV dipertahankan, patch idempoten, dan perubahan format yang tak
dikenali ditolak.

## Menjalankan

Uji regresi untuk patch kompatibilitas C (tanpa perlu build mrustc):

```sh
python3 -m unittest discover -s tools/mrustc_poc -p 'test_*.py'
```

Uji host lengkap untuk rewrite dan helper bytewise AtomicU128:

```sh
bash tools/test_mrustc_compat_host.sh
```

```sh
# 1. Build mrustc + transpile core dan program PoC ke C (host, butuh g++, curl)
bash tools/mrustc_poc/build_host.sh

# 2. Boot image A/B yang sudah ada + disk ext2 berisi sumber C
#    (image A/B: tools/build_xrustc_boot_image.sh)
python3 tools/mrustc_poc/qemu_test.py
```

`build_host.sh` menaruh semuanya di `$WORK` (default `/dev/shm/mrustc-poc`) dan
menulis `sources.img` (ext2 256 MiB) lewat `mke2fs` + `debugfs`, lalu
`qemu_test.py` memasangnya sebagai `sata1` di guest.

Perintah guest yang diuji:

```sh
mount sata1 /mnt ext2
tcc -I /mnt/src -include /mnt/src/fix.h /mnt/src/shim.c /mnt/src/poc.c \
    /mnt/src/libcore.c -o /mnt/poc.elf
run /mnt/poc.elf          # diharapkan mencetak: mrustc-c-poc: 108
```

## Catatan

- `mrustc` tidak mendukung crate `#![no_core]`: tanpa `core` ia gagal dengan
  `BUG: src/hir/hir.cpp: ... ::ops::RangeFull`. Karena itu `core` wajib
  di-transpile.
- Untuk crate `no_core`/`no_std`, injeksi crate implisit `alloc_system`
  dimatikan oleh `build_host.sh` (patch kecil pada `src/main.cpp`); tanpa itu
  `mrustc` menolak karena tidak menemukan `alloc_system`.
- Transpile `core` butuh `--edition 2024`; dengan edition lain `mrustc` gagal
  mem-parse `macro` 2.0 di `core/src/macros/mod.rs`.
