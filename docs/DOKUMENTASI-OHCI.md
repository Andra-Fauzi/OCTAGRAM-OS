# Dukungan OHCI

OHCI dikenali sebagai PCI USB host controller (`class=0x0C`, `subclass=0x03`,
`prog_if=0x10`). Driver menyiapkan HCCA 256-byte aligned, control/bulk ED dan
TD, frame timing, power root hub, serta operasi control dan bulk sinkron.
Enumerasi membaca descriptor, memberi alamat perangkat, memilih konfigurasi,
dan mendaftarkan endpoint aktif ke device model. Device low/full-speed
didukung; high-speed ditangani EHCI.

Mass Storage BOT menggunakan transport bulk OHCI yang sama dengan jalur block
device lainnya. Poll deferred memantau status root port, melakukan enumerasi
perangkat baru, dan memanggil cleanup interface setelah disconnect. HID boot
keyboard/mouse menggunakan periodic list OHCI satu-frame dan polling report
sesuai `bInterval` (frame 1 ms). Kegagalan enumerasi tidak diulang sampai
perangkat dicabut dan disambungkan kembali. Pengujian runtime QEMU dan hardware
nyata belum dilakukan.

## Referensi

- [CrabEFI OHCI](https://github.com/ArthurHeymans/CrabEFI/blob/4e3a3e24b4007563640af9d2b90e741ce1da0034/crabefi-core/src/drivers/usb/ohci.rs)
  — referensi implementasi; repositori CrabEFI memakai lisensi MIT atau
  Apache-2.0.
- [Linux OHCI host controller](https://github.com/torvalds/linux/blob/master/drivers/usb/host/ohci-hcd.c)
  — referensi untuk struktur schedule dan register controller.

Implementasi OCTAGRAM-OS ditulis terpisah; referensi digunakan untuk memahami
protokol dan struktur OHCI, bukan menyalin source.
