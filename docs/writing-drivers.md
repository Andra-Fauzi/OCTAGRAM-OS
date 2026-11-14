# Membuat driver

Driver baru sebaiknya memisahkan akses hardware dari kebijakan kernel. Kode
arsitektur khusus CPU ada di `kernel/src/arch/<arch>/`; driver perangkat dan
kelas perangkat berada di `kernel/src/drivers/`; API reusable berada di
subsystem seperti `storage/`.

## Langkah umum

1. Identifikasi bus/perangkat dan sumber daya yang dimiliki: PCI BAR, port I/O,
   MMIO, IRQ/MSI, DMA, atau block device. Untuk PCI, `scan_pci_bus()`
   mengembalikan snapshot `PciDevice` sekaligus mencetak hasil scan.
2. Buat modul baru pada lokasi yang sesuai dan daftarkan dengan `pub mod` pada
   `mod.rs` pemiliknya.
3. Implementasikan operasi hardware dalam tipe driver yang memiliki state
   eksplisit. Validasi register, ukuran buffer, alignment, timeout, dan status
   error; jangan mengasumsikan perangkat merespons.
4. Panggil inisialisasi dari orkestrasi boot atau jalur enumerasi bus yang
   sesuai. Hindari melakukan seluruh discovery di interrupt handler.
5. Untuk operasi asynchronous, handler IRQ sebaiknya acknowledge perangkat,
   menyimpan event ringkas, dan membangunkan task; kerjakan alokasi dan operasi
   kompleks di luar ISR.
6. Tambahkan tes host untuk parser/protokol/validasi input dengan fake device,
   lalu jalankan build bare-metal.

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
2. **Gunakan snapshot PCI yang sudah ada.** Saat boot,
   [`pci::scan_pci_bus()`](../kernel/src/drivers/pci/enumeration.rs) memindai
   bus sekali, mencetak hasil, dan mengembalikan `Vec<PciDevice>`.
   [`drivers::initialize()`](../kernel/src/drivers/mod.rs) mengirim slice
   `&pci_devices` ke setiap driver PCI. Snapshot ini sudah berisi function
   tambahan pada device multifungsi; jangan scan ulang di driver perangkat.
3. **Filter device di modul driver.** Gunakan `PciDevice::matches_class()` atau
   bandingkan ID vendor/device. Simpan fungsi filter di modul driver itu sendiri,
   bukan di PCI bus layer. Contoh yang sudah ada: filter AHCI di
   [`ahci.rs`](../kernel/src/drivers/ahci.rs) dan filter USB host controller di
   [`usb/controller.rs`](../kernel/src/drivers/usb/controller.rs).
4. **Inisialisasi setiap device yang cocok.** Buat `init(devices: &[PciDevice])`
   untuk melakukan filter dan memanggil `init_one(device)`. Kegagalan satu
   controller sebaiknya dicatat dan tidak menghentikan inisialisasi device lain.
5. **Daftarkan modul dan hook boot.** Tambahkan `pub mod nama_driver;` di
   [`drivers/mod.rs`](../kernel/src/drivers/mod.rs), lalu tambahkan panggilan
   `nama_driver::init(&pci_devices)` di `drivers::initialize()`. Letakkan
   driver baru sebelum pemanggilan USB: fallback USB saat ini dapat masuk loop
   polling tanpa kembali jika MSI-X tidak tersedia.

Kerangka matcher dan initializer:

```rust,ignore
const MY_CLASS: u8 = 0x00;
const MY_SUBCLASS: u8 = 0x00;
const MY_PROG_IF: u8 = 0x00;

fn is_my_device(device: &PciDevice) -> bool {
    device.matches_class(MY_CLASS, MY_SUBCLASS, MY_PROG_IF)
}

pub unsafe fn init(devices: &[PciDevice]) {
    for device in devices.iter().filter(|device| is_my_device(device)) {
        if let Err(error) = unsafe { init_one(device) } {
            crate::println!(
                "driver gagal untuk PCI {:02x}:{:02x}.{}: {:?}",
                device.bus, device.device, device.function, error
            );
        }
    }
}
```

`init_one` adalah tempat memilih BAR, menginisialisasi register, dan
menyiapkan interrupt/DMA. Lihat langkah berikut untuk aturan resource.

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

1. [`usb/controller.rs`](../kernel/src/drivers/usb/controller.rs) memfilter
   USB host controller dari snapshot PCI dan memulai xHCI. Saat ini UHCI/OHCI/
   EHCI hanya dikenali/dilaporkan; controller yang dijalankan adalah xHCI
   (`prog_if = 0x30`).
2. [`usb/xhci.rs`](../kernel/src/drivers/usb/xhci.rs) menangani transport:
   command/event ring, control transfer, konfigurasi endpoint, DMA, dan
   interrupt.
3. [`usb/device.rs`](../kernel/src/drivers/usb/device.rs) menangani slot,
   descriptor USB, pencocokan interface/endpoint, pemasangan class driver,
   serta cleanup saat disconnect.
4. Class driver menangani protokol device: [`keyboard_usb.rs`](../kernel/src/drivers/usb/keyboard_usb.rs)
   dan [`mouse_usb.rs`](../kernel/src/drivers/usb/mouse_usb.rs) membaca report
   HID, sedangkan [`mass_storage.rs`](../kernel/src/drivers/usb/mass_storage.rs)
   menjalankan USB mass-storage/Bulk-Only Transport dan mendaftarkan block
   device.

Alur sederhananya:

`PCI scan -> xHCI init -> enable slot/address device -> baca configuration descriptor -> pilih interface/endpoint -> class driver init`

`usb/mod.rs` adalah facade; umumnya class driver baru tidak perlu mengubah
`drivers::initialize()` karena device USB dipasang saat enumerasi/hotplug.

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
2. **Buat modul class.** Letakkan di `kernel/src/drivers/usb/`, simpan state
   per slot/interface, dan sediakan fungsi init serta shutdown. Init menerima
   `slot_id`, `EndpointHandle` hasil konfigurasi, dan nomor interface jika
   diperlukan. Jangan mengakses PCI BAR atau menulis ring xHCI langsung dari
   class driver; gunakan API transport yang ada.
3. **Daftarkan modul.** Tambahkan `pub mod nama_driver;` di
   [`usb/mod.rs`](../kernel/src/drivers/usb/mod.rs).
4. **Tambahkan dispatch.** Di `device.rs`, pilih endpoint menggunakan metadata
   `EndpointInfo`, konfigurasi endpoint melalui xHCI, lalu panggil init class.
   Untuk transfer yang memerlukan beberapa endpoint (seperti mass storage),
   cocokkan endpoint berdasarkan interface dan arah, lalu konfigurasi sesuai
   urutan DCI yang diminta xHCI.
5. **Tambahkan lifecycle cleanup.** Catat class yang aktif pada `ActivePort`.
   Perbarui `shutdown_device` agar state class dibersihkan sebelum
   `xhci::disable_slot()` dijalankan. Pastikan satu device composite dapat
   memiliki lebih dari satu interface.
6. **Hubungkan penyelesaian transfer.** Saat ini `xhci::poll_event_ring`
   mengenali report pending keyboard dan mouse secara khusus. Driver baru yang
   menerima transfer asynchronous perlu menambahkan routing/state pending
   yang sesuai; jangan memproses command panjang dari interrupt handler.
7. **Uji terpisah.** Buat tes host untuk pencocokan descriptor dan parsing
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
