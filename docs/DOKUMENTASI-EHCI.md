# Dukungan EHCI

EHCI dikenali sebagai PCI USB host controller (`class=0x0C`, `subclass=0x03`,
`prog_if=0x20`). Driver melakukan firmware handoff bila capability tersedia,
reset controller, menyiapkan asynchronous schedule dan frame list, memberi daya
root port, lalu mencoba enumerasi perangkat high-speed yang tersambung.

Fondasi transfer saat ini meliputi control transfer dan satu-qTD bulk transfer.
Descriptor DMA disejajarkan; alamat DMA di atas 4 GiB dan buffer yang melampaui
kapasitas lima page pointer satu qTD ditolak. Port yang tidak menjadi enabled
setelah reset dialihkan ke companion controller. Configuration descriptor
diparse untuk interface aktif dan endpoint-nya didaftarkan ke device model.

Interface Mass Storage BOT dengan bulk IN/OUT dirutekan ke transfer EHCI dan
block device 512-byte. Uji QEMU `q35` dengan `ich9-usb-ehci1` dan `usb-storage`
berhasil mengenumerasi perangkat dan mendaftarkan `usb0` berkapasitas 32768
sektor. Hardware fisik belum diuji.

## Status integrasi

Enumerasi membaca device/configuration descriptor dan memilih konfigurasi
pertama. Poll deferred memantau perubahan status port, mencoba enumerasi saat
perangkat tersambung, dan melepas interface driver saat perangkat dicabut.
Kegagalan enumerasi tidak diulang sampai port mengalami disconnect lalu
connect kembali. HID boot keyboard/mouse memakai schedule periodic satu frame
dan polling sesuai `bInterval`; interval high-speed EHCI dipetakan ke resolusi
frame 1 ms (interval di bawah 1 ms tidak dapat dipenuhi oleh jalur ini).
RNDIS tetap xHCI-only. HID periodic dan hardware fisik belum diuji di QEMU
maupun perangkat nyata.

## Referensi

- [CrabEFI EHCI](https://github.com/ArthurHeymans/CrabEFI/blob/4e3a3e24b4007563640af9d2b90e741ce1da0034/crabefi-core/src/drivers/usb/ehci.rs)
  — referensi implementasi; repositori CrabEFI memakai lisensi MIT atau
  Apache-2.0.
- [Linux EHCI host controller](https://github.com/torvalds/linux/blob/master/drivers/usb/host/ehci-hcd.c)
  — referensi untuk penjadwalan dan operasi controller.

Implementasi OCTAGRAM-OS ditulis terpisah; referensi digunakan untuk memahami
protokol dan struktur EHCI, bukan menyalin source.
