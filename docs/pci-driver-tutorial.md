# Tutorial Membuat Driver PCI di OCTAGRAM-OS

Dokumen ini memandu dari mengenali PCI function sampai driver terikat melalui
device model. Baca juga [arsitektur](architecture.md),
[driver overview](drivers.md), dan [panduan umum driver](writing-drivers.md).

## 1. PCI itu apa di kernel ini?

PCI/PCIe adalah bus untuk menemukan function hardware seperti SATA controller,
USB host controller, kartu jaringan, dan perangkat lain. Satu function
diidentifikasi oleh **BDF**:

bus:device.function

Hasil scan kernel disimpan sebagai PciDevice yang berisi BDF,
vendor_id, device_id, class, subclass, prog_if, dan header_type.

Kode terkait:

| File | Tugas |
| --- | --- |
| kernel/src/drivers/pci/config.rs | Membaca/menulis configuration space PCI |
| kernel/src/drivers/pci/enumeration.rs | Scan bus 0–255, device 0–31, function, hasilkan Vec<PciDevice> |
| kernel/src/drivers/pci/bar.rs | Mengaktifkan decoding, membaca BAR, mengatur bus mastering |
| kernel/src/drivers/pci/capability.rs | Capability list, MSI/MSI-X info, power management, ASPM helper |
| kernel/src/drivers/pci/mod.rs | Struktur PciDevice dan API publik PCI |
| kernel/src/drivers/device_model.rs | Registry, matching, probe, state per-device, remove |
| kernel/src/drivers/mod.rs | Titik registrasi/initialization driver saat boot |

Implementasi config access yang ada menggunakan mechanism #1 lewat port I/O
0xCF8/0xCFC. Ini bukan implementasi semua jenis PCI host bridge atau PCIe
enumeration tingkat lanjut.

## 2. Temukan perangkat target

Pertama boot dan lihat hasil scan PCI di kernel log. Cari:
- vendor/device ID dari datasheet atau lspci -nn di Linux pada mesin yang sama;
- class/subclass/prog_if untuk driver standar;
- BAR yang dipakai perangkat;
- interrupt capability (legacy/MSI/MSI-X);
- batas DMA, alignment, dan protocol register dari datasheet.

Contoh pencocokan controller:
- xHCI: class 0x0C, subclass 0x03, prog_if 0x30;
- EHCI: class 0x0C, subclass 0x03, prog_if 0x20;
- OHCI: class 0x0C, subclass 0x03, prog_if 0x10;
- AHCI: class 0x01, subclass 0x06, prog_if 0x01.

Untuk perangkat proprietary, utamakan pasangan vendor/device ID. Jangan
mengklaim semua revision perangkat pasti identik; cek datasheet/revision
register bila relevan.

## 3. Gunakan device model

Driver di OCTAGRAM adalah built-in Rust object, bukan modul .ko. Kontraknya:

- name(): nama unik driver.
- matches(&DeviceInfo): predicate murah; jangan mengubah hardware.
- probe(&DeviceInfo): inisialisasi satu perangkat, dipanggil di luar ISR,
  mengembalikan DriverState.
- remove(&DeviceInfo, state): hentikan device sebelum resource dilepas.

DeviceInfo saat ini memiliki Pci(PciDevice) dan
UsbInterface(UsbInterfaceInfo). DriverState adalah
Arc<dyn Any + Send + Sync>; state typed dapat diambil kembali dengan
device_model::driver_state::<T>(key).

Contoh bentuk matcher dan registrasi (kerangka konseptual; ganti ID dan state
sesuai hardware sebenarnya):

    use alloc::sync::Arc;
    use spin::Mutex;

    use crate::drivers::device_model::{
        self, DeviceDriver, DeviceInfo, DriverError, DriverState,
    };
    use crate::drivers::pci::{self, PciDevice};

    struct ExamplePciDriver;
    static EXAMPLE_PCI_DRIVER: ExamplePciDriver = ExamplePciDriver;

    struct ExampleState {
        registers: u64,
    }

    impl DeviceDriver for ExamplePciDriver {
        fn name(&self) -> &'static str { "example-pci" }

        fn matches(&self, info: &DeviceInfo) -> bool {
            matches!(info, DeviceInfo::Pci(pci)
                if pci.vendor_id == 0x1234 && pci.device_id == 0x5678)
        }

        unsafe fn probe(&self, info: &DeviceInfo)
            -> Result<DriverState, DriverError>
        {
            let DeviceInfo::Pci(pci) = info else {
                return Err(DriverError::Unsupported);
            };

            let state = initialize_example_device(pci)
                .map_err(|_| DriverError::ProbeFailed("device init failed"))?;
            Ok(Arc::new(Mutex::new(state)))
        }

        unsafe fn remove(&self, _info: &DeviceInfo, state: DriverState) {
            if let Ok(state) = Arc::downcast::<Mutex<ExampleState>>(state) {
                state.lock().shutdown();
            }
        }
    }

    // Di drivers::initialize(), setelah dependencies driver siap:
    // unsafe { device_model::register_driver(&EXAMPLE_PCI_DRIVER)?; }

Nama initialize_example_device dan shutdown adalah placeholder,
bukan API repository. Tujuannya memperlihatkan lokasi kode hardware dan
pengelolaan ownership. Implementasi nyata harus memakai API yang tersedia
pada pci/, memory/, dan spesifikasi perangkat.

Urutan register_driver dan register_device bisa berbeda. Device model
mencoba driver baru terhadap device unbound yang sudah terdaftar, sehingga
driver yang didaftarkan setelah scan juga bisa bind. Di drivers::initialize()
saat ini AHCI dan USB class driver didaftarkan sebelum scan; driver host
controller xHCI/EHCI/OHCI didaftarkan setelah perangkat PCI terdaftar.
Gunakan urutan eksplisit dan jangan mengasumsikan matches() otomatis dipanggil
dari interrupt.

## 4. Baca BAR dan aktifkan resource dengan aman

BAR (Base Address Register) menunjukkan lokasi register device:
- memory BAR → MMIO;
- I/O BAR → port I/O.

API di repository:
- pci::get_bar(device, index) dan get_bar0() mengembalikan
  BarInfo { address, is_64bit, is_prefetchable };
- pci::get_bar0_size() mengukur ukuran BAR0 dengan menulis mask sementara
  lalu memulihkan nilai semula;
- pci::enable_mmio_only() mengaktifkan memory decoding dan memastikan Bus
  Master tetap mati;
- pci::enable_bus_master() mengaktifkan DMA setelah persyaratan driver
  terpenuhi;
- pci::enable_device() menyalakan Memory Space dan Bus Master sekaligus.

get_bar() pada implementasi saat ini mengasumsikan BAR memory dan akan
panic untuk BAR I/O. Periksa bit jenis BAR/datasheet terlebih dahulu. Jangan
memakai contoh MMIO untuk BAR I/O.

Contoh pola map region MMIO jika BAR sudah dipastikan memory BAR dan ukuran
region diketahui:

    let bar = unsafe { pci::get_bar(pci_device, 0) };
    let size = unsafe { pci::get_bar0_size(pci_device) };
    if bar.address == 0 || size == 0 {
        return Err("BAR belum dialokasikan");
    }

    let page_offset = bar.address & 0xfff;
    let map_size = size.checked_add(page_offset).ok_or("BAR size overflow")?;
    let mapped_base = unsafe {
        crate::memory::paging::map_mmio_range(bar.address, map_size)
    };
    let registers = mapped_base + page_offset;

map_mmio_range() membulatkan alamat fisik ke bawah ke boundary halaman dan
mengembalikan alamat virtual HHDM. Karena itu pointer register perlu
ditambah offset byte dalam halaman. Contoh tersebut belum memvalidasi layout
register perangkat dan tidak menggantikan datasheet.

**Jangan aktifkan Bus Master terlalu awal.** Urutan aman: validasi perangkat,
map register, siapkan DMA buffer/ring yang aligned dan berada dalam batas DMA
controller, tulis alamat fisik yang benar ke device, baru aktifkan
enable_bus_master(). Pointer virtual kernel tidak otomatis merupakan
alamat DMA yang bisa dipakai hardware. Frame allocator menyediakan helper
DMA32, tetapi alokasi contiguous multi-page dan ownership tetap harus
dirancang sesuai controller.

## 5. Interrupt, MSI/MSI-X, dan deferred work

PCI capability list dibaca melalui pci::find_capability(). Helper terkait
MSI/MSI-X tersedia di capability.rs, tetapi menemukan capability tidak
berarti seluruh jalur interrupt device sudah siap.

Urutkan langkah:
1. sediakan vector dan handler arsitektur yang benar;
2. konfigurasi destination APIC/vector dan register device;
3. siapkan ring/buffer/status sebelum enable interrupt;
4. acknowledge sumber interrupt;
5. ISR hanya menyimpan status/event ringkas;
6. proses completion, alokasi, dan logging kompleks di deferred context.

Untuk awal, gunakan polling terbatas waktu bila device/protokol mengizinkan.
Jangan membuat spin loop tanpa timeout dan jangan memegang global spinlock
selama menunggu hardware.

## 6. Error handling dan cleanup

Setiap tahap probe harus gagal dengan jelas:
- BAR tidak ada/ukuran tidak masuk akal;
- MMIO mapping gagal;
- controller reset timeout;
- device status invalid;
- DMA address misaligned/out of range;
- interrupt tidak tersedia;
- command completion timeout.

Jika gagal setelah resource dialokasikan, hentikan hardware yang mungkin
sudah aktif sebelum membebaskan ring/buffer. Simpan state per-device agar
remove() tahu apa yang perlu dimatikan. PCI hot-remove belum menjadi lifecycle
umum yang lengkap; jangan mengimplementasikan pelepasan resource seolah-olah
device bisa dicabut kapan saja.

## 7. Tambahkan test

Pisahkan test menjadi tiga lapisan:

1. **Unit test host:** matcher, parser register, descriptor, state machine,
   batas ukuran/alignment/overflow dengan input/fake register.
2. **Build bare-metal:** cargo +nightly test, lalu
   make -C kernel KARCH=x86_64.
3. **Runtime:** boot QEMU dengan controller yang sesuai, periksa PCI log,
   binding lsdev/drivers, lalu jalankan operasi sederhana.

Jangan menandai driver berhasil hanya karena probe() terpanggil. Bukti
minimal adalah perangkat terdeteksi, resource tervalidasi, operasi nyata
berhasil, jalur error timeout bekerja, dan cleanup tidak meninggalkan DMA/IRQ
aktif.

## 8. Contoh yang dapat dipelajari

- kernel/src/drivers/ahci.rs: PCI controller yang menyediakan block device.
- kernel/src/drivers/usb/controller.rs: PCI matching untuk xHCI/EHCI/OHCI.
- kernel/src/drivers/qca9377.rs dan drivers/qca9377/: driver PCIe
  dengan banyak tahap firmware/protocol; status runtime-nya harus dicek di
  [wifi-qca9377.md](wifi-qca9377.md).
- [Tutorial USB](usb-driver-tutorial.md): contoh pemisahan host controller
  PCI dari USB interface class driver.
