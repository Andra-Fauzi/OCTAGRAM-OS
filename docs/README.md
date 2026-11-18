# Dokumentasi OCTAGRAM-OS

Dokumentasi ini menjelaskan struktur kernel, cara membangun program user,
kontrak syscall, dan pola kontribusi subsystem.

## Mulai cepat

- [Arsitektur kernel](architecture.md): alur boot, modul utama, task, VFS,
  user mode, dan batas isolasi saat ini.
- [Build kernel dan program user](building.md): dependency host, target build,
  toolchain GCC/binutils dan Rust `no_std`, contoh program, TTY input, dan cara
  menjalankannya.
- [ABI syscall](syscall-abi.md): nomor syscall, register, nilai balik, error,
  descriptor, dan batas buffer.
- [Membuat driver](writing-drivers.md): lokasi kode, inisialisasi PCI,
  interrupt, DMA, registrasi block device, dan pola pengujian.
- [Storage dan filesystem](storage.md): block layer, FAT32, VFS, mount, dan
  aturan pelepasan perangkat.
- [Panduan port aplikasi](porting-apps.md): cara menyesuaikan program C kecil
  dengan runtime dan ABI OCTAGRAM.
- [Porting CPython](python-porting.md): status dukungan, syscall yang tersedia,
  blocker kompatibilitas, dan tahapan untuk membawa interpreter Python.

Dokumentasi ini mencerminkan implementasi sekarang, bukan janji kompatibilitas
Linux/POSIX. Periksa batasan di setiap halaman sebelum mengandalkan fitur untuk
aplikasi nyata.
