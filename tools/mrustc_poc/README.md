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
   Implementasi menyediakan 112 simbol double/float yang dideklarasikan header,
   termasuk operasi IEEE-754 dasar, eksponensial, logaritma, trigonometrik,
   gamma dan error-function berbasis aproksimasi. `math-header-test` kini
   mengompilasi dan menautkan seluruh simbol; `math-test` menguji perilaku host; QEMU harness juga menguji program
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
   source guest `tcc_atomic128_shim.c` menguji parser TinyCC dan dua jalur helper.
   Helper ini **bukan atomic** dan tidak aman untuk concurrency.

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

`build_host.sh` juga mencetak SHA commit `mrustc` yang benar-benar dipakai (`MRUSTC-COMMIT`) dan menerima `MRUSTC_REF` untuk memilih branch/tag saat clone. Patch `alloc_system` kini mensyaratkan tepat satu titik patch (atau kondisi sudah diterapkan yang konsisten), sehingga perubahan source upstream yang ambigu ditolak. Arsip Rust divalidasi dengan `tar -tzf`, diekstrak ke direktori staging baru, dan baru dipindahkan setelah `library/core/src/lib.rs` terverifikasi; ekstraksi parsial lama tidak lagi dianggap lengkap. File patch Rust juga diperiksa eksplisit sebelum dry-run.

`build_host.sh` sekarang memvalidasi bahwa keluaran `mrustc` masih memuat XGETBV
asli, lalu hanya me-rewrite AtomicU128 yang belum didukung. Sebelas uji regresi
memastikan XGETBV dipertahankan, patch idempoten, perubahan format yang tak
dikenali ditolak, fragmen AtomicU128 duplikat/campuran ditolak, dan file asli
tidak berubah jika validasi gagal. Hasil ditulis ke file temporer dalam
direktori yang sama, di-flush dan di-fsync, lalu dipasang dengan `os.replace`. Patch sumber Rust juga diterapkan secara fail-closed: script
membedakan patch yang belum diterapkan, yang sudah diterapkan, dan source
archive yang tidak cocok. Kegagalan patch tidak lagi disembunyikan oleh
`|| true`. Tahap PoC juga menghapus `poc.c` lama sebelum transpile dan hanya
menerima status `mrustc` nonzero bila file C baru benar-benar dihasilkan serta
memuat definisi `main` yang dikenali validator Python terpisah dan diuji unit;
kegagalan tanpa output segar menghentikan build. Tes validator mencakup output
kosong, fungsi selain `main`, serta penyebutan `main` yang hanya berada di komentar.
Artefak `libcore.c` dan `libcore.hir` juga dihapus sebelum transpile dan wajib
muncul kembali sebelum patcher dijalankan, sehingga build tidak memakai hasil
`core` dari percobaan sebelumnya.

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
Setelah semua file dimasukkan, builder menjalankan `e2fsck -fn` dan mengekstrak
kembali sembilan payload dengan `debugfs dump`; masing-masing dibandingkan
byte-per-byte terhadap sumber aslinya. Ini mencegah kegagalan perintah `debugfs`
yang tidak tercermin pada exit status menghasilkan image sumber yang tampak sukses.

Perintah guest yang diuji:

```sh
mount sata1 /mnt ext2
tcc -I /mnt/src -include /mnt/src/fix.h /mnt/src/shim.c /mnt/src/poc.c \
    /mnt/src/libcore.c -o /mnt/poc.elf
run /mnt/poc.elf          # diharapkan mencetak: mrustc-c-poc: 108
```

Sebelum kompilasi besar `libcore.c`, harness menjalankan tes kecil:
`TCC-OVERFLOW-PASS` (overflow builtins), `TCC-XGETBV-ASSEMBLE-PASS`
(encoding `0F 01 D0` ditemukan tanpa mengeksekusi instruksi),
`TCC-MATH-PASS` (libm dari `libc.a`), dan
`TCC-ATOMIC128-SHIM-PASS` (helper bytewise non-atomic). Hasil tersebut
memisahkan kegagalan fondasi dari kegagalan kompilasi seluruh core.
Setiap tahap compile dan run pada harness QEMU juga menunggu status
keluar `0` serta prompt shell kembali; marker PASS saja tidak cukup untuk
meloloskan tahap. Ini mencegah artefak lama atau program yang mencetak marker
tetapi gagal keluar dianggap sukses. Harness memakai file OVMF-vars sementara,
serta path unik per proses untuk log serial dan socket monitor agar run paralel
tidak saling mengganggu. Proses QEMU, socket monitor, dan file firmware
sementara dibersihkan pada semua jalur keluar, termasuk kegagalan sebelum
monitor tersambung; log serial dan log QEMU host dipertahankan untuk diagnosis.
Output QEMU host (termasuk error startup) ditulis ke log terpisah yang path-nya
dapat diatur lewat `OCTAGRAM_POC_QEMU_LOG`. Pada setiap kegagalan setelah
QEMU dijalankan, harness otomatis mencetak 5.000 byte terakhir log host agar
diagnosis boot, monitor, dan kompilasi tidak hanya bergantung pada serial log.
Pembacaan tail dilakukan secara biner agar batas UTF-8 yang terpotong tidak
membuat pelaporan diagnostik gagal. Kegagalan menyalin template OVMF-vars juga
menghapus file temporernya.

Kompilasi besar `libcore.c` dijalankan dengan opsi `tcc -bench` agar run yang
berhasil juga mencatat statistik kompilasi TinyCC. Penantian tahap ini memakai
satu deadline gabungan untuk status keluar `0` dan prompt shell, sehingga
waktu tunggu tidak berlipat ketika salah satu penanda tidak muncul. Tes unit
`test_qemu_test.py` mencakup kedua syarat keberhasilan tersebut.

## Catatan

- `mrustc` tidak mendukung crate `#![no_core]`: tanpa `core` ia gagal dengan
  `BUG: src/hir/hir.cpp: ... ::ops::RangeFull`. Karena itu `core` wajib
  di-transpile.
- Untuk crate `no_core`/`no_std`, injeksi crate implisit `alloc_system`
  dimatikan oleh `build_host.sh` (patch kecil pada `src/main.cpp`); tanpa itu
  `mrustc` menolak karena tidak menemukan `alloc_system`.
- Transpile `core` butuh `--edition 2024`; dengan edition lain `mrustc` gagal
  mem-parse `macro` 2.0 di `core/src/macros/mod.rs`.
