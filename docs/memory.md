# Memori

Subsystem memori ada di `kernel/src/memory/` dengan tiga bagian: frame allocator,
heap global, dan paging. Semua pemetaan kernel memakai direct map (HHDM) dari
Limine; alamat fisik diakses lewat `physical_to_virtual()`.

## Frame allocator

`memory/frame.rs` membangun free-list dari `MemoryMapRequest` Limine saat
`memory::init()` dipanggil sekali di boot. API publiknya:

- `allocate_frame()` → `Option<usize>` (alamat fisik satu page 4 KiB);
- `deallocate_frame(frame)` untuk mengembalikan page;
- `FrameAllocator::allocate_dma32()` untuk halaman di bawah 4 GiB (dipakai
  driver yang butuh alamat 32-bit, mis. ring USB).

Tidak ada quota per proses: alokasi dibatasi oleh frame yang benar-benar
tersedia dan ruang alamat virtual.

## Heap kernel

`memory/heap.rs` memasang `#[global_allocator]` berbasis
`linked_list_allocator` di atas region yang dipesan dari frame allocator. Heap
hanya untuk kernel; program user memakai `brk`/`mmap` sendiri.

## Paging

`memory/paging.rs` menyediakan:

- `map`, `unmap`, dan helper MMIO untuk driver;
- pembuatan page table privat per image user (separuh bawah alamat user kosong,
  separuh atas memetakan kernel agar syscall dan interrupt tetap berjalan);
- `validate_user_range(address, length, write)` untuk memeriksa buffer syscall
  sebelum disentuh.

Program user tidak bisa mengubah pemetaan kernel karena page table kernel tidak
dapat ditulis dari ring 3 dan segmen user yang `writable + executable` ditolak
loader.

## Tata letak alamat user

| Wilayah | Nilai | Catatan |
| --- | --- | --- |
| Batas alamat user | `0x0000_8000_0000_0000` | Setengah bawah ruang alamat |
| Awal wilayah `mmap` | `0x1_0000_0000` | Cursor naik dengan entropi awal 0–127 halaman |
| Akhir wilayah `mmap` | tepat di bawah stack | Menyisakan ruang stack + cadangan |
| Stack | 8 halaman (32 KiB) | Puncak digeser acak 0–63 halaman tiap `execve` |
| Heap proses | dari `brk` | `brk` tumbuh dari cursor `mmap`, bukan image tetap |

Karena heap dan `mmap` berbagi kursor, `sbrk` dan `mmap(NULL, ...)` tidak saling
menimpa. Tidak ada heap image beralamat tetap di ELF, sehingga `malloc` selalu
memakai `mmap`/`brk` runtime.

## ASLR

- **PIE**: load bias diacak per eksekusi dalam rentang `PIE_LOAD_BIAS..MAX`.
- **mmap**: cursor awal diberi entropi 0–127 halaman.
- **stack**: puncak stack digeser 0–63 halaman per eksekusi.
- **heap**: mengikuti cursor `mmap` yang sudah berentropi.

Yang belum: interpreter (`ld-octagram.so`) masih `ET_EXEC` sehingga selalu
dimuat di alamat tetap. Ini tercatat sebagai item roadmap yang belum selesai.

## Batasan

- Tanpa copy-on-write: `fork` menyalin halaman user secara eager.
- Tanpa swap atau anonymous memory backed disk.
- Tanpa SMP: hanya satu CPU yang menjalankan kernel dan user.
- `mlock`/`mlockall` hanya menandai halaman (tidak ada swap untuk dikunci) dan
  kuncinya tidak diwarisi `fork`; tidak ada huge page atau `madvise`
  (tersedia hanya `posix_madvise` untuk validasi).
- `mmap` hanya file anonymous/`MAP_PRIVATE`/`MAP_SHARED` sesuai dukungan
  descriptor; pemetaan file dieksekusi lewat loader ELF, bukan `mmap` umum.

## Uji

`cargo +nightly test --manifest-path kernel/Cargo.toml --target x86_64-unknown-linux-gnu`
menjalankan uji host untuk helper murni seperti `stack_layout()`,
`validate_mapping_range()`, dan perhitungan ukuran pemetaan. Perilaku paging
nyata hanya bisa diperiksa dengan menjalankan image di QEMU.
