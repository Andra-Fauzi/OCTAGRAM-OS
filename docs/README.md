# Dokumentasi OCTAGRAM-OS

Dokumentasi ini menjelaskan implementasi OCTAGRAM-OS apa adanya: kernel
x86_64 Rust yang boot lewat Limine, runtime libc sendiri, dynamic linker,
driver perangkat, dan shell interaktif. Ini adalah indeks utama dokumentasi.

Mulai dari [arsitektur](architecture.md), lanjutkan ke
[panduan pengembangan](development-guide.md), lalu gunakan topik spesifik
sesuai pekerjaan. Dokumentasi harus membedakan fitur yang benar-benar
diimplementasikan, fitur yang baru berupa rancangan, dan fitur yang sudah
diuji. Jangan menganggap kompatibilitas POSIX/Linux lengkap hanya karena
sebagian API tersedia; lihat [batasan](limitations.md).

## Panduan utama

| Area | Dokumen | Cakupan |
| --- | --- | --- |
| Orientasi proyek | [architecture.md](architecture.md) | Arsitektur, subsistem, alur data dan batas isolasi |
| Mulai mengembangkan | [development-guide.md](development-guide.md) | Peta repository/file, alur boot, build, debug dan workflow perubahan |
| Boot | [boot.md](boot.md) | Limine, urutan inisialisasi kernel, APIC dan idle loop |
| Memori | [memory.md](memory.md) | Frame allocator, heap, paging dan pemetaan memori |
| Proses dan thread | [process.md](process.md) | Proses, thread, fork/exec, sinyal dan futex |
| ABI syscall | [syscall-abi.md](syscall-abi.md) | Nomor syscall, register, return value dan error |
| Userspace/libc | [userspace.md](userspace.md) | ABI program, crt0, runtime, libc dan demo |
| Dynamic linker | [dynamic-linker.md](dynamic-linker.md) | Relokasi, shared object, dlopen/dlclose dan TLS |
| Filesystem | [filesystem.md](filesystem.md) | VFS, mount, RAMFS, devfs dan path |
| Storage | [storage.md](storage.md) | Block layer, FAT32, ext2 dan mount |
| Networking | [networking.md](networking.md) | Ethernet, ARP, IP, UDP/TCP, DHCP, DNS dan socket |
| Driver | [drivers.md](drivers.md) | Inventaris driver dan status dukungan |
| ACPI/power | [acpi-power.md](acpi-power.md) | ACPI, resource PCI, shutdown dan reboot |
| Logging | [logging.md](logging.md) | Kernel log, serial output dan diagnostik |
| Grafis/terminal | [graphics-terminal.md](graphics-terminal.md) | Framebuffer, console dan input |
| Shell | [shell.md](shell.md) | Perintah shell, PATH, run, pkg dan mount |
| Paket | [packages.md](packages.md) | Format paket, registry dan app launcher |
| Userspace Rust | [rust-userspace.md](rust-userspace.md) | Aplikasi Rust no-std dan octagram-runtime |
| Rust custom target | [rust-no-std.md](rust-no-std.md) | Custom target x86_64 OCTAGRAM, build-std core/alloc, linker, dan kriteria uji |
| Rust backend M4.3 | [rust-m4-3-backend-plan.md](rust-m4-3-backend-plan.md) | Perbandingan opsi backend `rustc` native, keputusan LLVM + `ld` native, tahapan dan risiko |
| Panduan port Rust/C++ | [rust-port-guide.md](rust-port-guide.md) | Status port, peta file, cara build/uji host dan QEMU, cara menambah tahap, troubleshooting |
| Toolchain | [toolchain.md](toolchain.md) | Assembler, linker, compiler dan TinyCC |
| Build | [building.md](building.md) | Dependencies, target build, ISO dan QEMU |
| Self-hosting (roadmap utama) | [roadmap-self-hosting.md](roadmap-self-hosting.md) | Milestone, dependency, status, acceptance gates, dan bukti menuju full self-hosting |
| Self-hosting (audit teknis) | [native-self-hosting-roadmap.md](native-self-hosting-roadmap.md) | Audit baseline dan temuan teknis terdahulu |
| Pengujian | [testing.md](testing.md) | Unit test, tes userspace/POSIX dan runtime |
| Batasan | [limitations.md](limitations.md) | Fitur yang belum tersedia dan dampaknya |

## Membuat dan mengembangkan driver

Urutan yang disarankan:

1. Baca [writing-drivers.md](writing-drivers.md) untuk kontrak umum, ownership,
   resource, interrupt, DMA dan cleanup.
2. Ikuti [pci-driver-tutorial.md](pci-driver-tutorial.md) untuk perangkat PCI,
   BAR/MMIO, DMA, interrupt dan device model.
3. Ikuti [usb-driver-tutorial.md](usb-driver-tutorial.md) untuk host controller
   dan USB class driver.
4. Baca catatan controller atau class tertentu di bagian referensi lanjutan.

| Dokumen | Kegunaan |
| --- | --- |
| [CARA-BIKIN-DRIVER.md](CARA-BIKIN-DRIVER.md) | Catatan resep tambahan untuk UART, RTC, timer, storage dan network; ide di dalamnya harus diverifikasi terhadap source terkini sebelum dipakai |
| [CARA-BIKIN-DRIVER-HID-USB.md](CARA-BIKIN-DRIVER-HID-USB.md) | Detail HID boot protocol, report keyboard/mouse dan re-arm transfer |
| [PANDUAN-DRIVER-USB-GENERIK.md](PANDUAN-DRIVER-USB-GENERIK.md) | Batas generik vs class-specific, descriptor, endpoint dan Mass Storage BOT |
| [DOKUMENTASI-XHCI.md](DOKUMENTASI-XHCI.md) | Detail struktur ring, TRB, event, enumerasi dan riwayat bug xHCI |
| [DOKUMENTASI-EHCI.md](DOKUMENTASI-EHCI.md) | Status implementasi, jalur transfer dan keterbatasan EHCI |
| [DOKUMENTASI-OHCI.md](DOKUMENTASI-OHCI.md) | Status implementasi dan keterbatasan OHCI |
| [wifi-qca9377.md](wifi-qca9377.md) | Status bring-up driver Qualcomm QCA9377 |
| [android-usb-tethering.md](android-usb-tethering.md) | Catatan RNDIS/NCM untuk tethering Android |

## Porting aplikasi

| Dokumen | Kegunaan |
| --- | --- |
| [porting-apps.md](porting-apps.md) | Porting aplikasi C kecil ke ABI dan libc OCTAGRAM |
| [porting-python.md](porting-python.md) | Catatan target CPython dan API yang masih perlu dilengkapi |

## Konvensi dokumentasi

- Gunakan Bahasa Indonesia; istilah teknis boleh tetap dalam bahasa aslinya.
- Jelaskan fungsi file berdasarkan source aktual, bukan nama atau asumsi.
- Tandai contoh konseptual dan placeholder secara eksplisit; jangan tampilkan
  nama fungsi placeholder seolah-olah API yang sudah tersedia.
- Pisahkan status implementasi dari status pengujian. Sebutkan lingkungan uji
  (host, QEMU, atau hardware fisik) dan hasil yang benar-benar diamati.
- Saat file dipindah/dihapus/ditambahkan, perbarui indeks ini.
- Item roadmap yang selesai harus memiliki uji atau demo yang dapat dijalankan,
  sesuai aturan penerimaan di [ROADMAP.md](../ROADMAP.md).
