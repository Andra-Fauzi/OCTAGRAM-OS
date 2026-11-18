# Dokumentasi OCTAGRAM-OS

Dokumentasi ini menjelaskan OCTAGRAM-OS apa adanya: kernel x86_64 Rust yang
boot lewat Limine, runtime libc sendiri, dynamic linker, driver perangkat, dan
shell interaktif. Setiap bagian punya berkasnya sendiri, dan berkas ini adalah
indeksnya.

Mulai dari [arsitektur](architecture.md) bila baru mengenal proyek, lalu buka
bagian yang relevan. Dokumentasi mencerminkan implementasi sekarang, bukan janji
kompatibilitas Linux/POSIX; batasan yang belum didukung dicatat eksplisit di
[batasan](limitations.md).

## Gambaran umum

| Bagian | Berkas | Isi |
| --- | --- | --- |
| Arsitektur | [architecture.md](architecture.md) | Peta modul, alur boot ringkas, batas isolasi, model task dan proses |
| Boot dan inisialisasi | [boot.md](boot.md) | Limine, urutan `kmain`, APIC, timer, idle loop |
| Memori | [memory.md](memory.md) | Frame allocator, heap, paging, pemetaan user, ASLR |
| Proses, thread, sinyal | [process.md](process.md) | Executor kernel, scheduler user, `fork`/`execve`, sinyal, futex |
| Filesystem dan VFS | [filesystem.md](filesystem.md) | Router mount, RAMFS, devfs, path, descriptor |
| Block device dan on-disk FS | [storage.md](storage.md) | Block layer, FAT32, ext2, mount/umount |
| Jaringan | [networking.md](networking.md) | Link layer, ARP, IPv4, UDP, TCP, DHCP, DNS, HTTP, socket |
| Driver | [drivers.md](drivers.md) | PCI, AHCI, USB (xHCI/EHCI/OHCI), HID, RTC, serial, ACPI |
| Grafis dan terminal | [graphics-terminal.md](graphics-terminal.md) | Framebuffer, font, empat virtual console, input keyboard |
| Shell | [shell.md](shell.md) | Perintah bawaan, PATH, `run`/`runp`, `pkg`, `mount` |
| Program user dan libc | [userspace.md](userspace.md) | ABI user, `crt0`, struktur runtime, daftar program contoh |
| Dynamic linker | [dynamic-linker.md](dynamic-linker.md) | `ld-octagram.so`, relokasi, versi simbol, `dlopen`/`dlclose`, TLS |
| Toolchain native | [toolchain.md](toolchain.md) | `as`, `ld`, `cc` bawaan kernel dan TinyCC |
| Build | [building.md](building.md) | Dependency host, target build, ISO/HDD, menjalankan QEMU |
| ABI syscall | [syscall-abi.md](syscall-abi.md) | Nomor syscall, register, nilai balik, error, descriptor |
| Pengujian | [testing.md](testing.md) | Uji host kernel, uji host user, uji POSIX, yang belum tercakup |
| Batasan | [limitations.md](limitations.md) | Fitur yang sengaja belum ada dan konsekuensinya |

## Panduan dan catatan hardware

| Berkas | Isi |
| --- | --- |
| [writing-drivers.md](writing-drivers.md) | Pola menulis driver baru: PCI, interrupt, DMA, block device |
| [CARA-BIKIN-DRIVER.md](CARA-BIKIN-DRIVER.md) | Alur kerja driver dari nol |
| [CARA-BIKIN-DRIVER-DRIVER-PENTING.md](CARA-BIKIN-DRIVER-DRIVER-PENTING.md) | Driver yang paling penting untuk dibangun lebih dulu |
| [CARA-BIKIN-DRIVER-USB.md](CARA-BIKIN-DRIVER-USB.md) | Driver USB umum |
| [CARA-BIKIN-DRIVER-HID-USB.md](CARA-BIKIN-DRIVER-HID-USB.md) | Driver HID di atas USB |
| [PANDUAN-DRIVER-USB-GENERIK.md](PANDUAN-DRIVER-USB-GENERIK.md) | Driver USB generik dan kelas perangkat |
| [DOKUMENTASI-XHCI.md](DOKUMENTASI-XHCI.md) | Catatan implementasi xHCI |
| [DOKUMENTASI-EHCI.md](DOKUMENTASI-EHCI.md) | Catatan implementasi EHCI |
| [DOKUMENTASI-OHCI.md](DOKUMENTASI-OHCI.md) | Catatan implementasi OHCI |
| [wifi-qca9377.md](wifi-qca9377.md) | Status bring-up Qualcomm QCA9377 |
| [android-usb-tethering.md](android-usb-tethering.md) | RNDIS/NCM untuk tethering Android |

## Porting

| Berkas | Isi |
| --- | --- |
| [porting-apps.md](porting-apps.md) | Menyesuaikan program C kecil dengan runtime dan ABI OCTAGRAM |
| [porting-python.md](porting-python.md) | Target port CPython 3.11/3.12 dan API libc yang masih kurang |

## Konvensi dokumentasi

- Bahasa Indonesia, istilah teknis dibiarkan dalam bentuk aslinya.
- Setiap klaim perilaku merujuk modul yang mengimplementasikannya; bila sesuatu
  belum ada, tulis belum ada.
- Item roadmap yang selesai harus punya uji atau demo yang bisa dijalankan,
  sesuai aturan penerimaan di [ROADMAP.md](../ROADMAP.md).
