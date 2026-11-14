# Membuat driver

Driver baru sebaiknya memisahkan akses hardware dari kebijakan kernel. Kode
arsitektur khusus CPU ada di `kernel/src/arch/<arch>/`; driver perangkat dan
kelas perangkat berada di `kernel/src/drivers/`; API reusable berada di
subsystem seperti `storage/`. Driver PCI dan USB terhubung ke
[`drivers/device_model.rs`](../kernel/src/drivers/device_model.rs), yang
mencocokkan perangkat dengan driver lalu mengelola `probe`, state per-device,
dan `remove`.

## Langkah umum

1. Identifikasi bus/perangkat dan sumber daya yang dimiliki: PCI BAR, port I/O,
   MMIO, IRQ/MSI, DMA, atau block device. Untuk PCI, `scan_pci_bus()`
   mengembalikan snapshot `PciDevice` sekaligus mencetak hasil scan.
2. Buat modul baru pada lokasi yang sesuai dan daftarkan dengan `pub mod` pada
   `mod.rs` pemiliknya.
3. Implementasikan `DeviceDriver`: `matches()` hanya mencocokkan identitas,
   `probe()` menginisialisasi satu perangkat dan mengembalikan state miliknya,
   sedangkan `remove()` menghentikan hardware dan melepas state.
4. Daftarkan instance driver yang berumur `'static` sebelum enumerasi jika
   memungkinkan. Device model juga mencoba driver baru terhadap perangkat
   unbound yang sudah terdaftar, tanpa menahan lock registry selama callback.
5. Validasi register, ukuran buffer, alignment, timeout, dan status error;
   jangan mengasumsikan perangkat merespons.
6. Hindari melakukan seluruh discovery di interrupt handler.
7. Untuk operasi asynchronous, handler IRQ sebaiknya acknowledge perangkat,
   menyimpan event ringkas, dan membangunkan task; kerjakan alokasi dan operasi
   kompleks di luar ISR.
8. Tambahkan tes host untuk parser/protokol/validasi input dengan fake device,
   lalu jalankan build bare-metal.

Saat ini driver didaftarkan sebagai kode built-in dan dikompilasi bersama
kernel. Belum ada pemuat modul kernel, ABI `.ko`, `insmod`, atau `rmmod`.

### Kontrak device model

`DeviceInfo` saat ini memiliki varian `Pci(PciDevice)` dan
`UsbInterface(UsbInterfaceInfo)`. `DeviceKey` mengidentifikasi PCI function
atau interface USB. Driver mengembalikan `DriverState` (umumnya
`Arc<Mutex<MyState>>`) dari `probe()`; state itu disimpan per-device dan
diberikan ke `remove()`. Gunakan `device_model::driver_state::<T>(key)` untuk
mengambil handle state dari jalur kerja driver.

Kerangka minimum driver PCI:

```rust,ignore
struct MyDriver;
static MY_DRIVER: MyDriver = MyDriver;

impl DeviceDriver for MyDriver {
    fn name(&self) -> &'static str { "my-device" }

    fn matches(&self, info: &DeviceInfo) -> bool {
        matches!(info, DeviceInfo::Pci(pci)
            if pci.matches_class(0x12, 0x34, 0x56))
    }

    unsafe fn probe(&self, info: &DeviceInfo)
        -> Result<DriverState, DriverError>
    {
        let DeviceInfo::Pci(pci) = info else {
            return Err(DriverError::Unsupported);
        };
        let state = MyState::initialize(pci)
            .map_err(|_| DriverError::ProbeFailed("hardware initialization failed"))?;
        Ok(Arc::new(Mutex::new(state)))
    }

    unsafe fn remove(&self, _info: &DeviceInfo, state: DriverState) {
        if let Some(state) = state.downcast_ref::<Mutex<MyState>>() {
            state.lock().shutdown();
        }
    }
}
```

Tambahkan modul dan daftarkan `&MY_DRIVER` dengan
`device_model::register_driver()` di orkestrasi `drivers::initialize()`. Fungsi
registrasi device/driver bersifat `unsafe`: panggil setelah bus serta resource
pendukung siap dan selalu dari konteks non-ISR. `matches()` harus murah dan
tidak mengubah hardware; alokasikan resource serta sentuh register di `probe()`.

Gunakan perintah shell `drivers` untuk melihat driver terdaftar dan `lsdev`
untuk melihat perangkat bus serta driver yang berhasil bind. `devices` tetap
menampilkan block device.

## Driver perangkat PCI

PCI memisahkan **driver bus** dari **driver perangkat**. Modul
[`drivers/pci`](../kernel/src/drivers/pci/mod.rs) menemukan fungsi PCI dan
menyediakan informasi BDF (`bus:device.function`), vendor/device ID, serta
class/subclass/programming interface dalam `PciDevice`. Driver perangkat
memakai informasi itu untuk memilih device, mengaktifkannya, lalu mengatur
BAR, interrupt, dan DMA sesuai spesifikasi perangkat.

### Pembagian modul

- [`pci/config.rs`](../kernel/src/drivers/pci/config.rs) menangani akses PCI
  Configuration Space.
- [`pci/enumeration.rs`](../kernel/src/drivers/pci/enumeration.rs) melakukan
  scan bus dan menghasilkan snapshot `Vec<PciDevice>`.
- [`pci/bar.rs`](../kernel/src/drivers/pci/bar.rs) menangani enable device dan
  pembacaan BAR.
- [`pci/capability.rs`](../kernel/src/drivers/pci/capability.rs) menangani
  linked list capability PCI dan helper MSI/MSI-X.
- [`ahci.rs`](../kernel/src/drivers/ahci.rs) dan
  [`usb/controller.rs`](../kernel/src/drivers/usb/controller.rs) memiliki
  pencocokan class dan inisialisasi controller masing-masing.
- [`usb/mod.rs`](../kernel/src/drivers/usb/mod.rs) adalah facade USB yang
  mengekspos modul dan entry point yang dipakai kernel.
- [`usb/device.rs`](../kernel/src/drivers/usb/device.rs) mengurus enumerasi
  port, pencatatan lifecycle, dispatch interface, dan cleanup hotplug.
- [`usb/xhci.rs`](../kernel/src/drivers/usb/xhci.rs) mengurus transport host
  controller; `keyboard_usb.rs`, `mouse_usb.rs`, dan `mass_storage.rs` adalah
  driver class.

### Alur membuat driver PCI

1. **Kenali device yang dicari.** Dari datasheet, tentukan apakah pencocokan
   memakai PCI class/subclass/`prog_if` atau `vendor_id`/`device_id`.
   Contohnya AHCI adalah class `0x01`, subclass `0x06`, `prog_if` `0x01`;
   USB host controller adalah class `0x0C`, subclass `0x03`, lalu `prog_if`
   membedakan UHCI/OHCI/EHCI/xHCI.
2. **Daftarkan driver di `drivers::initialize()`.** Fungsi ini memindai PCI
   sekali dan mengirim setiap `PciDevice` ke device model. Biasanya driver
   didaftarkan sebelum scan; driver xHCI saat ini didaftarkan setelah scan
   karena probe controller dapat masuk fallback polling. Device model tetap
   mencobanya pada perangkat yang sudah terdaftar dan belum bind. Snapshot
   mencakup function tambahan pada device multifungsi; jangan scan ulang di
   driver perangkat.
3. **Implementasikan matcher.** `DeviceDriver::matches()` menerima
   `DeviceInfo::Pci(PciDevice)` dan mencocokkan class/subclass/`prog_if` atau
   vendor/device ID. Simpan kebijakan pencocokan di driver, bukan di PCI bus.
4. **Inisialisasi satu perangkat di `probe()`.** Aktifkan PCI function, baca
   BAR, siapkan MMIO/interrupt/DMA, lalu kembalikan state milik perangkat.
   Kegagalan probe dicatat oleh device model dan tidak menghentikan scan.
5. **Sediakan cleanup.** Implementasikan `remove()` bila bus dapat melepas
   device. PCI hot-remove belum didukung; jangan melepas BAR atau DMA buffer
   saat controller masih aktif.
6. Tambahkan `pub mod nama_driver;` dan daftarkan `&MY_DRIVER` di
   [`drivers/mod.rs`](../kernel/src/drivers/mod.rs).

Contoh matcher PCI:

```rust,ignore
const MY_CLASS: u8 = 0x00;
const MY_SUBCLASS: u8 = 0x00;
const MY_PROG_IF: u8 = 0x00;

fn matches(&self, info: &DeviceInfo) -> bool {
    matches!(info, DeviceInfo::Pci(device)
        if device.matches_class(MY_CLASS, MY_SUBCLASS, MY_PROG_IF))
}
```

`probe()` adalah tempat memilih BAR, menginisialisasi register, dan menyiapkan
interrupt/DMA. Lihat langkah berikut untuk aturan resource.

### Resource dan BAR

`pci::enable_device(&dev)` mengaktifkan bit **Memory Space** dan **Bus Master**
pada PCI Command register. Bus mastering memberi device izin melakukan DMA;
helper ini mengaktifkan keduanya sekaligus, jadi pastikan driver siap
mengendalikan device sebelum memanggilnya.

Gunakan `pci::get_bar(&dev, index)` untuk membaca BAR memori, lalu periksa
alamat, lebar 32/64-bit, dan spesifikasi device sebelum mengaksesnya. Helper
yang ada mengasumsikan BAR memori dan akan panic untuk BAR port-I/O. Jangan
memakainya untuk device yang menggunakan I/O BAR. Jangan langsung
dereference alamat BAR sebelum memastikan pemetaan MMIO dan cache attribute
sesuai dengan aturan paging kernel.

Jika device melakukan DMA, hardware harus menerima alamat fisik yang valid,
bukan pointer virtual biasa. Periksa alignment, ukuran dan rentang buffer;
pertahankan buffer tetap hidup sampai device berhenti menggunakannya. Atur
timeout untuk polling register/command dan kembalikan error jika hardware tidak
merespons.

### Interrupt dan operasi tertunda

Mulai dengan polling yang terbatas waktu jika protokol memungkinkan. Untuk
interrupt, siapkan vector dan rute interrupt sesuai dukungan device (misalnya
MSI/MSI-X), acknowledge sumber interrupt, dan hindari alokasi, logging berat,
atau menunggu hardware di ISR. ISR sebaiknya mencatat event; proses pekerjaan
yang lebih berat di jalur deferred/task. Periksa pola yang digunakan
[`usb/mod.rs`](../kernel/src/drivers/usb/mod.rs) dan driver keyboard sebelum
menambahkan jalur interrupt baru.

### Verifikasi driver PCI

Kembangkan bertahap: pastikan device terdeteksi; cocokkan ID dan BAR dengan
spesifikasi; baca status awal; jalankan satu operasi sederhana; baru tambahkan
DMA dan interrupt. Tambahkan tes host untuk parser atau state machine yang
tidak memerlukan hardware, lalu build kernel dan uji pada QEMU atau mesin
target. Catat asumsi device dan jalur cleanup jika inisialisasi gagal.

Saat ini PCI driver diinisialisasi saat boot; belum ada framework umum untuk
PCI hot-remove. Jangan menganggap BAR atau buffer DMA boleh dilepas selama
device masih aktif.

## Driver USB

USB memiliki dua lapisan device yang berbeda. **USB host controller** ditemukan
di PCI; **USB device** (keyboard, mouse, disk, dan sebagainya) kemudian
dienumerasi oleh host controller. Jangan samakan PCI class USB controller
(`0x0C/0x03`) dengan USB interface class pada descriptor (`0x03` HID,
`0x08` mass storage).

### Pembagian modul dan alur data

1. [`usb/controller.rs`](../kernel/src/drivers/usb/controller.rs) menyediakan
   driver PCI xHCI (`prog_if = 0x30`) yang dipasangkan melalui device model.
   UHCI/OHCI/EHCI belum memiliki driver.
2. [`usb/xhci.rs`](../kernel/src/drivers/usb/xhci.rs) menangani transport:
   command/event ring, control transfer, konfigurasi endpoint, DMA, dan
   interrupt.
3. [`usb/device.rs`](../kernel/src/drivers/usb/device.rs) menangani slot,
   descriptor USB, pembentukan device per interface, dan cleanup saat
   disconnect.
4. Class driver menangani protokol device: [`keyboard_usb.rs`](../kernel/src/drivers/usb/keyboard_usb.rs)
   dan [`mouse_usb.rs`](../kernel/src/drivers/usb/mouse_usb.rs) membaca report
   HID, sedangkan [`mass_storage.rs`](../kernel/src/drivers/usb/mass_storage.rs)
   menjalankan USB mass-storage/Bulk-Only Transport dan mendaftarkan block
   device. Ketiganya didaftarkan sebagai `DeviceDriver`.

Alur sederhananya:

`register drivers -> PCI scan/device model -> xHCI probe -> enumerate USB -> register interface -> class-driver probe`

`usb/mod.rs` adalah facade. Tambahkan driver class baru ke registrasi class
USB di [`usb/device.rs`](../kernel/src/drivers/usb/device.rs); tidak perlu
mengubah matcher PCI atau transport xHCI.

### Descriptor yang dipakai untuk memilih class

| Class interface | Field untuk pencocokan | Endpoint yang dipakai saat ini |
|---|---|---|
| HID keyboard | `interface_class = 3`, `interface_protocol = 1` | Interrupt IN |
| HID mouse | `interface_class = 3`, `interface_protocol = 2` | Interrupt IN |
| USB mass storage | class `8`, subclass `6`, protocol `0x50` | Pasangan Bulk IN dan Bulk OUT |

`EndpointInfo` berisi alamat endpoint, jenis transfer (`attributes & 0x3`),
ukuran paket, interval, nomor interface, dan class/subclass/protocol interface.
Alamat endpoint bit `0x80` menandakan arah IN. Cocokkan seluruh syarat sebelum
memanggil `xhci::configure_endpoint`; jangan menganggap semua endpoint dalam
satu interface memiliki jenis atau arah yang sama.

### Langkah menambahkan USB class driver

1. **Tentukan interface dan protokolnya.** Catat class/subclass/protocol,
   transfer type, arah, ukuran report/buffer, dan request control yang
   diwajibkan device. Boot protocol keyboard/mouse tidak mewakili semua HID;
   device HID lain mungkin memerlukan parser HID Report Descriptor.
2. **Buat modul dan driver.** Implementasikan `DeviceDriver` dengan
   `matches()` untuk `DeviceInfo::UsbInterface`. Kembalikan state per-interface
   dari `probe()` dan bersihkan melalui `remove()`. Jangan mengakses PCI BAR
   atau menulis ring xHCI langsung dari class driver; gunakan API transport.
3. **Pilih dan konfigurasi endpoint di `probe()`.** Cocokkan endpoint
   berdasarkan interface, tipe transfer, dan arah. Untuk beberapa endpoint
   (seperti Mass Storage), konfigurasi sesuai urutan DCI yang diminta xHCI.
4. **Daftarkan driver.** Tambahkan modul dan instance driver ke daftar
   `register_class_drivers()` di `device.rs`; device model akan mencobanya saat
   interface USB didaftarkan ketika enumerasi/hotplug.
5. **Hubungkan penyelesaian transfer.** Saat ini `xhci::poll_event_ring`
   mengenali report pending keyboard dan mouse secara khusus. Driver baru yang
   menerima transfer asynchronous perlu menambahkan routing/state pending
   yang sesuai; jangan memproses command panjang dari interrupt handler.
6. **Uji terpisah.** Buat tes host untuk pencocokan descriptor dan parsing
   report/protokol; lalu uji init, input/output, disconnect, dan reconnect di
   QEMU atau hardware.

Untuk hotplug, event interrupt hanya menandai perubahan port. Pekerjaan
enumerasi dan pelepasan device dijalankan oleh `usb::poll_hotplug()` dari
`drivers::poll_deferred()`, setelah ISR selesai. Jangan memanggil fungsi
enumerasi slot dari ISR.

## Driver block

Untuk mengekspos media sektor ke FAT32, implementasikan trait
`storage::block::BlockDevice`:

```rust,ignore
impl BlockDevice for MyDisk {
    fn name(&self) -> &str { "disk0" }
    fn sector_count(&self) -> u64 { self.sectors }
    fn read_sectors(&self, lba: u64, buffer: &mut [u8])
        -> Result<(), BlockError> { /* validate and read */ }
    fn write_sectors(&self, lba: u64, buffer: &[u8])
        -> Result<(), BlockError> { /* validate and write */ }
}
```

Buffer length must be a multiple of 512, and `lba + count` must remain within
device capacity. Register an `Arc<dyn BlockDevice>` with
`storage::block::register`; the shell discovers registered devices through
`devices`. Follow the AHCI and USB mass-storage drivers for existing patterns.

## MMIO, DMA, dan locking

- Map MMIO through the paging helpers and use volatile accesses where required.
- Convert virtual and physical addresses deliberately; DMA addresses are not
  automatically equivalent to kernel virtual pointers.
- Keep DMA buffers aligned and alive while the controller can access them.
- Do not hold a spinlock while waiting indefinitely for hardware. Use bounded
  polling/timeout and return a typed error.
- Avoid printing, allocating, or taking long-held locks in an ISR.
- Document ownership and invariants around every unsafe register/DMA operation.

## Keyboard/IRQ pattern

The keyboard driver uses atomics for a bounded event queue in interrupt context.
The shell consumes queued events in a task. New interrupt handlers must match
the exact stack-frame convention in `arch/x86_64/interrupt.rs`, acknowledge the
interrupt controller, and avoid blocking. Add a host unit test for scan-code or
event translation rather than trying to emulate hardware in a unit test.

## Checklist sebelum merge

- [ ] Validasi semua input hardware dan batas indeks/alamat.
- [ ] Error perangkat diteruskan ke caller, tidak dianggap sukses.
- [ ] Tidak ada alokasi tak terbatas atau blocking di ISR.
- [ ] Resource dilepas pada jalur gagal dan unplug/hot-remove.
- [ ] Ada tes untuk parsing/state transition dan build target kernel berhasil.
- [ ] Dokumentasi menjelaskan asumsi hardware dan fitur yang belum didukung.
