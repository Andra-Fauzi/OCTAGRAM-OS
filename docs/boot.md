# Boot dan inisialisasi

Kernel adalah ELF x86_64 `no_std` yang dimuat oleh bootloader Limine. Tidak ada
tahap assembly terpisah di repo ini: Limine menyiapkan long mode, stack, dan
framebuffer, lalu memanggil `kmain` di `kernel/src/main.rs`.

## Permintaan Limine

Semua request ditandai `#[used]` dan diletakkan di section `.requests` agar tidak
dibuang linker:

| Request | Lokasi | Kegunaan |
| --- | --- | --- |
| `BaseRevision` | `main.rs` | Versi protokol Limine yang didukung |
| `StackSizeRequest` | `main.rs` | Stack boot 256 KiB |
| `ExecutableFileRequest` | `main.rs` | Akses byte kernel ELF yang sedang dijalankan untuk installer Live |
| `ModuleRequest` | `main.rs` | Akses `BOOTX64.EFI` yang dimuat sebagai modul oleh Limine |
| `FramebufferRequest` | `graphics/mod.rs` | Framebuffer linier untuk terminal |
| `MemoryMapRequest` | `memory/mod.rs` | Peta memori fisik untuk frame allocator |
| `HhdmRequest` | `memory/mod.rs` | Offset direct map untuk konversi fisik→virtual |

`kmain` memanggil `assert!(BASE_REVISION.is_supported())` agar image yang tidak
cocok gagal cepat, bukan berjalan dengan asumsi yang salah.

## Urutan `kmain`

Urutan ini penting; mengubahnya bisa mematikan output atau membuat lock dipakai
sebelum siap.

1. `drivers::early_init()` — inisialisasi serial COM1 sehingga pesan boot
   berikutnya terlihat di serial console.
2. Probe framebuffer dan tulis beberapa pixel putih (indikator awal bahwa
   framebuffer sudah dipetakan).
3. `cli`, lalu `fpu::init()`.
4. Cetak versi kernel dan `BUILD_STAMP` (hash revisi + status dirty + waktu
   build, diinjeksi `build.rs`) supaya image yang bukan hasil build terbaru
   langsung kelihatan.
5. `gdt::load_gdt()` lalu `idt::load_idt()`.
6. `rsdp::test_rsdp()`, `apic::disable_pic()`, `init_lapic()`, `init_ioapic()`.
7. `sti`, lalu `memory::init()` (frame allocator + heap global).
8. `drivers::initialize()` — ACPI, registrasi driver AHCI/USB, scan PCI,
   registrasi perangkat storage.
9. `fs::init()` — pilih EXT2 atau FAT32 persisten pada partisi GPT disk
   `sataN` dengan marker `/etc/octagram-root`; gunakan RAMFS hanya sebagai
   fallback rescue.
10. Timer periodik LAPIC (`TIMER_VECTOR`) untuk preemption user mode.
11. `task::spawn` untuk task kernel penanda executor siap.
12. Empat shell: `shell::run(terminal)` untuk `tty0`..`tty3`, masing-masing
    didaftarkan sebagai input waker keyboard.
13. `hcf()` — idle loop.

## Installer Live

`installer` berjalan langsung dari shell. Limine memuat file kernel melalui
`ExecutableFileRequest` dan `BOOTX64.EFI` melalui `ModuleRequest`; installer
menyalinnya ke ESP disk target. `kernel/src/storage/partition.rs` menulis GPT
serta menyediakan block-device view terbatas untuk setiap partisi. Detail
perintah dan batas dukungan ada di [install-to-disk.md](install-to-disk.md).

## Idle loop

`hcf()` tidak hanya `hlt`. Setiap putaran ia:

- menjalankan task kernel yang siap (`task::run_ready`), dan hanya `hlt` bila
  tidak ada task siap;
- melaporkan interrupt eksternal tak dikenal dari konteks non-IRQ;
- memanggil `drivers::poll_deferred()` (input xHCI, hotplug USB, RNDIS/NCM);
- menjalankan retry QCA9377 bila fitur itu diaktifkan (default mati);
- memanggil `network::poll_tcp()` untuk retransmisi, zero-window probe,
  keepalive, dan pembersihan socket terminal.

Karena itu pekerjaan yang memblokir (mis. menunggu input keyboard di dalam
syscall `read`) wajib memanggil `drivers::poll_input()` di loop tunggunya,
bukan hanya mengandalkan idle loop.

## Build image

`GNUmakefile` di root membangun ISO dan HDD image lewat `limine.conf`.
Target yang tersedia antara lain `all`, `all-hdd`, `run`, `run-hdd`,
`run-bios`, `kernel`, `python311`/`python312`, `clean`, dan `distclean`.
Detail dependency host, toolchain Rust nightly, dan cara menjalankan QEMU ada di
[building.md](building.md).

Catatan: berkas `Makefile` di root serta `tests/Makefile` pada checkout ini
adalah symlink ke direktori `/tmp/octagram-tinycc-upstream/`. GNU Make memilih
`GNUmakefile` lebih dulu, jadi build normal tetap memakai `GNUmakefile`.

## Selanjutnya

- Peta modul lengkap: [architecture.md](architecture.md)
- Memori dan paging: [memory.md](memory.md)
- Driver yang diinisialisasi saat boot: [drivers.md](drivers.md)
