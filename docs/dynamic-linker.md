# Dynamic linker

`user/ldso/loader.c` adalah dynamic linker OCTAGRAM (`ld-octagram.so`), dibangun
sebagai ELF statis dan dijalankan kernel saat program memiliki `PT_INTERP`.
Program yang memakai `ld-octagram.so` dibangun dengan `crt0.pic.o` dan
`liboctagram.so` lewat `make -C user dynamic`.

## Batas dan konstanta

| Konstanta | Nilai | Arti |
| --- | --- | --- |
| `MAX_OBJECTS` | 16 | Objek yang bisa dimuat bersamaan |
| `MAX_PHDRS` | 128 | Program header per objek |
| `MAX_DYNAMIC` | 512 | Entri `.dynamic` |
| `MAX_FILE_SIZE` | 16 MiB | Ukuran maksimum berkas objek |
| `MAX_NAME` | 256 | Panjang nama simbol/library |
| `USER_ADDRESS_MAX` | `0x0000_8000_0000_0000` | Batas atas alamat user |

## Alur load

1. Kernel memetakan interpreter dan menyerahkan kontrol ke `ldso_start`.
2. Loader membaca header ELF, memvalidasi `ET_DYN`/`ET_EXEC`, `EM_X86_64`, dan
   program header `PT_LOAD`/`PT_DYNAMIC`/`PT_PHDR`.
3. Setiap segmen `PT_LOAD` dipetakan dengan `mmap`; segmen `PF_W|PF_X` ditolak.
4. Objek `ET_DYN` mendapat load bias (PIE executable diacak kernel; library
   mengikuti kursor `mmap` yang berentropi).
5. Simbol di-resolve dari executable lalu library dalam urutan dependency;
   `DT_NEEDED` dimuat lewat `LD_LIBRARY_PATH` dan `/lib`.
6. Relokasi diterapkan, lalu `DT_INIT_ARRAY` dan `DT_INIT` dijalankan.
7. Kontrol diserahkan ke entry point program.

## Relokasi yang didukung

`R_X86_64_64`, `PC32`, `GLOB_DAT`, `JUMP_SLOT`, `COPY`, `RELATIVE`, `32`, dan
`32S`. Relokasi lain ditolak dengan pesan error, bukan diabaikan.

Binding bersifat **eager**: seluruh `JUMP_SLOT` diselesaikan saat load. Lazy
binding (PLT resolver) belum diimplementasikan.

## Versi simbol

Loader membaca `DT_VERSYM`, `DT_VERDEF`, dan `DT_VERNEED`. Referensi
`nama@VERSI` hanya cocok dengan definisi versi tersebut, sedangkan `nama@@VERSI`
adalah versi default. `versioned_demo` menguji kedua bentuk terhadap
`libdyn_demo.so` dengan skrip versi `ldso/dynlib.versions`.

## Simbol weak dan copy relocation

- Simbol weak yang tidak terdefinisi di-resolve sebagai 0 tanpa menggagalkan
  load.
- `R_X86_64_COPY` menyalin data dari shared library ke executable non-PIC dan
  mengekspor salinan itu, sehingga kedua sisi melihat objek yang sama.
  `copy_reloc_demo` membuktikan perubahan data terlihat dua arah.

## `dlopen`/`dlsym`/`dlclose`

Libc menyediakan `dlopen`, `dlsym`, `dlerror`, dan `dlclose` dengan hook ke
loader (`__oct_*`). Perilaku:

- library yang sudah dimuat dipakai ulang dan refcount-nya naik;
- `RTLD_DEFAULT` mencari simbol di semua objek yang dimuat;
- `RTLD_NEXT` mencari definisi berikutnya setelah objek pemanggil
  (`__builtin_return_address(0)` dipakai untuk menentukan pemanggil);
- `dlclose` menurunkan refcount, menjalankan destructor (`DT_FINI_ARRAY`,
  `DT_FINI`, `__cxa_finalize` untuk DSO tersebut) sebelum `munmap`, lalu
  membebaskan slot objek;
- `dlopen_demo` memeriksa refcount dan pemakaian ulang slot.

## Constructor, destructor, dan `atexit`

- `DT_INIT_ARRAY`/`DT_INIT` dijalankan saat load (executable dulu, lalu library).
- Destructor dijalankan terbalik saat `exit` atau `dlclose`.
- `atexit` dan `__cxa_atexit` berbagi satu daftar 64 entri dengan argumen dan
  `__dso_handle`; `__cxa_finalize(dso)` hanya menjalankan entri milik DSO itu.
- `cxa_atexit_demo` dan uji host `make -C user cxa-test` memverifikasi urutan.

## TLS

Libc memakai TLS statis untuk `errno` (`-DOCTAGRAM_TLS_ERRNO`). Shared object
yang mendefinisikan TLS sendiri belum didukung: loader tidak mengalokasikan blok
TLS per objek dan tidak menangani relokasi `TPOFF`/`DTPMOD`. Ini item roadmap
yang belum selesai.

## Batasan

- Interpreter masih `ET_EXEC`, jadi alamatnya tetap (satu-satunya bagian ASLR
  yang belum).
- Tanpa lazy binding, tanpa `DT_RUNPATH` (hanya `LD_LIBRARY_PATH` dan `/lib`),
  tanpa `dlmopen`, tanpa namespace library.
- `DT_NEEDED` dimuat, tetapi dependency melingkar dan `dlopen` rekursif
  kedalamannya perlu diuji lebih lanjut.
