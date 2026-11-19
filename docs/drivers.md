# Driver

Driver berada di `kernel/src/drivers/`. Inisialisasi dipusatkan di
`drivers::initialize()` dan pekerjaan yang tidak boleh berjalan di interrupt
diproses dari idle loop lewat `drivers::poll_deferred()`.

## Model driver

`drivers/device_model.rs` menyediakan registry perangkat dan driver:

- `DeviceInfo::Pci(device)` untuk perangkat PCI hasil scan;
- `register_driver()` mendaftarkan driver dengan kriteria match;
- `register_device()` mencocokkan perangkat ke driver dan memanggil probe;
- daftar isi registry bisa dilihat dari shell dengan `lsdev` dan `drivers`.

Driver baru sebaiknya mengikuti pola ini agar hotplug dan urutan probe konsisten.
Panduan lengkap: [writing-drivers.md](writing-drivers.md).

## PCI

`drivers/pci/` menyediakan:

- `config.rs` — akses ruang konfigurasi PCI (I/O port 0xCF8/0xCFC);
- `enumeration.rs` — scan bus/devices/functions;
- `bar.rs` — pembacaan dan pemetaan BAR (MMIO atau I/O port);
- `capability.rs` — traversal capability list (MSI/MSI-X, power management).

## Storage

- `drivers/ahci.rs`: driver AHCI SATA; mendaftarkan block device ke
  `storage::block`.
- `drivers/usb/mass_storage.rs`: USB mass storage (BOT) sebagai block device.
- Detail block layer dan filesystem: [storage.md](storage.md).

## USB

| Modul | Isi |
| --- | --- |
| `usb/xhci.rs` | Host controller xHCI (default), event ring, transfer ring, slot |
| `usb/ehci.rs` | Host controller USB 2.0 EHCI |
| `usb/ohci.rs` | Host controller USB 1.1 OHCI |
| `usb/device.rs` | Enumerasi perangkat, deskriptor, konfigurasi, endpoint |
| `usb/descriptors.rs` | Parser deskriptor USB |
| `usb/keyboard_usb.rs` | Keyboard HID → input TTY |
| `usb/mouse_usb.rs` | Mouse HID |
| `usb/hid_periodic.rs` | Polling laporan HID periodik |
| `usb/mass_storage.rs` | Block device di atas BOT |
| `usb/rndis.rs`, `usb/ncm.rs` | Tethering jaringan Android (RNDIS/NCM) |

Interrupt xHCI hanya menandai flag; event diproses di konteks non-IRQ
(`poll_input`, `poll_manual_events`, `poll_interrupt_deferred`) untuk menghindari
deadlock spin-lock. `XHCI_ENABLED` bisa dimatikan sementara lewat konstanta di
`usb/mod.rs`.

## Input dan console

- `drivers/keyboard.rs`: keyboard PS/2 (IRQ) dengan waker per terminal.
- `drivers/serial.rs`: COM1 untuk log kernel (`early_init`); detail subsistem logging ada di [logging.md](logging.md).
- `drivers/rtc.rs`: RTC CMOS, dipakai `date` dan `CLOCK_REALTIME` (resolusi
  satu detik).
- `drivers/acpi/`: parser ACPI/AML, evaluasi power resources `_PR0`/`_PS0` untuk perangkat PCI, serta power management (`shutdown` meminta S5 dan `reboot` memakai FADT `RESET_REG` dengan fallback 8042 dan triple fault); detail lengkap di [acpi-power.md](acpi-power.md).

## Wi-Fi QCA9377

`drivers/qca9377/` berisi bring-up Qualcomm QCA9377 (PCIe, CE rings, BMI, HTC,
HTT, WMI, firmware). Status saat ini: probe PCI dan pembacaan register terbatas
sudah ada, tetapi upload firmware dan koneksi Wi-Fi belum berjalan, dan driver
dinonaktifkan secara default (`QCA9377_ENABLED = false`). Detail dan catatan
percobaan ada di [wifi-qca9377.md](wifi-qca9377.md).

## Batasan

- Tanpa hotplug lifecycle yang stabil untuk semua kelas perangkat; USB hotplug
  ditangani lewat polling.
- Tanpa driver audio, GPU, printer, webcam, NVMe, atau UASP.
- Tanpa abstraksi network interface generik; link layer hanya menerima satu
  perangkat aktif.

## Uji

Uji host mencakup parser deskriptor, perhitungan alamat BAR, dan helper murni
lain. Driver nyata memerlukan QEMU dengan perangkat yang sesuai atau hardware
fisik.
