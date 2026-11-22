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
2. Header publik `user/include/math.h` mendeklarasikan API matematika yang
   dipakai C `core`, tetapi implementasi libm nyata belum tersedia.
3. TinyCC sekarang mengenali dan menurunkan `__builtin_add_overflow`,
   `__builtin_sub_overflow`, dan `__builtin_mul_overflow` ke helper runtime
   `__tcc_overflow`. Tes host helper tersedia dan QEMU harness menguji parser,
   linker, hasil, serta overflow untuk beberapa lebar integer.
4. `tcc` masih belum memiliki opcode assembler `xgetbv` dan belum mendukung
   atomic 128-bit (`uint128_t _Atomic`); dua hal ini masih berupa rewrite
   khusus PoC, bukan implementasi compiler/runtime umum.

Helper overflow memakai aritmetika lebar untuk memeriksa rentang signed dan
unsigned 8/16/32/64 bit, serta tetap menyimpan bit hasil terpotong saat overflow.
Macro lama yang selalu mengembalikan false sudah dihapus dari `fix.h`.
Status tes QEMU tetap harus dibuktikan; keberadaan harness bukan bukti PASS.
Header `math.h` lokal di direktori ini tetap menjadi fixture PoC; stub
matematika masih tidak presisi karena jalur float tidak dipakai program uji.

`build_host.sh` sekarang memvalidasi fragmen keluaran `mrustc` sebelum dan sesudah
rewrite kompatibilitas `XGETBV`/`AtomicU128`. Jika format C yang dihasilkan berubah,
script berhenti dengan error alih-alih meneruskan source yang belum dipatch.

## Menjalankan

Uji regresi untuk rewrite kompatibilitas C (tanpa perlu build mrustc):

```sh
python3 -m unittest discover -s tools/mrustc_poc -p 'test_*.py'
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
