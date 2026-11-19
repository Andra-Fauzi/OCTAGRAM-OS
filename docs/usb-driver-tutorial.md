# Tutorial Membuat Driver USB di OCTAGRAM-OS

Panduan ini menjelaskan jalur dari controller PCI sampai driver class USB.
USB punya dua lapisan yang sering tertukar:

- **USB Host Controller** adalah hardware pada motherboard/PCI (xHCI, EHCI,
  OHCI). Driver host controller berkomunikasi dengan controller tersebut.
- **USB Device/Interface** adalah perangkat yang dicolok (keyboard, mouse,
  flashdisk, tethering). Class driver berkomunikasi melalui endpoint USB.

Contoh: keyboard USB bukan PCI device. PCI device-nya adalah xHCI/EHCI/OHCI
controller; keyboard baru ditemukan saat controller men-scan root port.

## 1. Peta file USB

| File | Peran |
| --- | --- |
| kernel/src/drivers/usb/mod.rs | Facade, controller ID/kind, EndpointInfo, XHCI_ENABLED, export entry point |
| usb/controller.rs | Driver PCI untuk xHCI/EHCI/OHCI, controller initialization dan port scan |
| usb/xhci.rs | xHCI capability/operational registers, command ring, transfer ring, event ring, slot, endpoint, control/bulk transfers |
| usb/ehci.rs | EHCI schedule, queue head/transfer descriptor, port dan transfer |
| usb/ohci.rs | OHCI HCCA, endpoint/transfer descriptor, port dan transfer |
| usb/descriptors.rs | Parser USB configuration/interface/endpoint descriptor |
| usb/device.rs | USB enumeration, interface grouping, class-driver registration/binding, disconnect cleanup/hotplug |
| usb/hid_periodic.rs | Polling endpoint HID periodik untuk controller EHCI/OHCI |
| usb/keyboard_usb.rs | HID boot keyboard report → input keyboard OS |
| usb/mouse_usb.rs | HID boot mouse report |
| usb/mass_storage.rs | Bulk-Only Transport dan SCSI command → block device |
| usb/bulk_in.rs | Helper penerimaan bulk IN |
| usb/rndis.rs | USB RNDIS network/tethering |
| usb/ncm.rs | USB CDC-NCM network/tethering |

Host controller saat ini dipilih lewat PCI class code USB
(class 0x0C, subclass 0x03) dan prog_if:
- OHCI 0x10
- EHCI 0x20
- xHCI 0x30

usb::XHCI_ENABLED di usb/mod.rs adalah build/runtime switch untuk
registrasi xHCI. EHCI/OHCI didaftarkan terpisah.

## 2. Alur dari colok perangkat sampai class driver

Alur konseptual:

    PCI scan
      -> DeviceInfo::Pci
      -> match host-controller driver
      -> probe xHCI/EHCI/OHCI
      -> reset/init controller and scan root ports
      -> enumerate USB device (address, descriptors, configuration)
      -> parse interface + endpoint descriptors
      -> register DeviceInfo::UsbInterface per interface
      -> device_model matches a USB class driver
      -> class-driver probe()
      -> transfer completion / polling / input or block subsystem

device_model.rs memakai dua key:
- PCI: bus/device/function;
- USB interface: controller ID, slot ID, interface number.

Karena satu USB device dapat memiliki beberapa interface, class matching
dilakukan terhadap interface, bukan hanya satu device-level class byte.

## 3. Pahami descriptor dan endpoint

Descriptor memberikan identitas dan struktur perangkat. Yang biasanya
dibutuhkan class driver:
- interface class, subclass, protocol;
- nomor interface;
- endpoint address;
- endpoint transfer attributes;
- maximum packet size;
- polling interval.

EndpointInfo di repository membawa field tersebut. Bit 0x80 pada
endpoint address menandakan arah IN (device ke host); tanpa bit itu adalah
OUT (host ke device). Dua bit rendah attributes mengidentifikasi transfer
type:
- 0: control (umumnya endpoint zero/EP0);
- 1: isochronous;
- 2: bulk;
- 3: interrupt.

Parser saat ini:
usb::descriptors::parse_configuration_endpoints(&[u8]).
Parser mengembalikan vector endpoint yang ditautkan ke interface terkait.
Validasi panjang descriptor penting: jangan membaca field sebelum memastikan
panjang descriptor minimal dan tidak melampaui total length configuration.

Contoh class umum:
| Kelas interface | Nilai |
| --- | --- |
| HID | class 0x03 |
| HID Boot Keyboard | class 0x03, subclass 0x01, protocol 0x01 |
| HID Boot Mouse | class 0x03, subclass 0x01, protocol 0x02 |
| Mass Storage BOT | class 0x08, subclass 0x06, protocol 0x50 |

Jangan mengasumsikan semua HID memakai boot protocol. Gamepad, tablet, dan
banyak device custom memerlukan parsing HID Report Descriptor dan bukan
struktur report keyboard 8-byte.

## 4. Tambahkan USB class driver

Class driver baru biasanya berada di kernel/src/drivers/usb/<class>.rs.
Ia mengimplementasikan DeviceDriver dan hanya menerima
DeviceInfo::UsbInterface.

Contoh matcher konseptual untuk interface vendor-specific:

    fn usb_interface(info: &DeviceInfo) -> Option<&UsbInterfaceInfo> {
        match info {
            DeviceInfo::UsbInterface(interface) => Some(interface),
            DeviceInfo::Pci(_) => None,
        }
    }

    impl DeviceDriver for ExampleUsbDriver {
        fn name(&self) -> &'static str { "example-usb" }

        fn matches(&self, info: &DeviceInfo) -> bool {
            usb_interface(info).is_some_and(|interface| {
                interface.class == 0xff
                    && interface.subclass == 0x01
                    && interface.protocol == 0x01
                    && interface.endpoints.iter().any(|ep| {
                        ep.attributes & 0x03 == 0x02
                    })
            })
        }

        unsafe fn probe(&self, info: &DeviceInfo)
            -> Result<DriverState, DriverError>
        {
            let Some(interface) = usb_interface(info) else {
                return Err(DriverError::Unsupported);
            };
            // Validasi endpoint, pilih transport sesuai controller.kind,
            // lakukan class-specific init, lalu simpan state per interface.
            let state = initialize_example_interface(interface)
                .map_err(|_| DriverError::ProbeFailed("USB class init failed"))?;
            Ok(Arc::new(Mutex::new(state)))
        }

        unsafe fn remove(&self, info: &DeviceInfo, state: DriverState) {
            // Hentikan transfer, lepaskan resource, dan bersihkan state.
        }
    }

Nama ExampleUsbDriver, initialize_example_interface, dan kode state di
atas adalah placeholder untuk memperlihatkan struktur, bukan simbol yang sudah
ada di repo. Implementasi harus mengimpor tipe dan transport yang benar.

Daftarkan instance driver 'static ke array di
usb::device::register_class_drivers() pada device.rs. Fungsi tersebut
dipanggil oleh usb::register_class_drivers() dari
drivers::initialize(). Setelah interface didaftarkan, device model mencoba
driver yang cocok dan memanggil probe().

## 5. Pilih transport sesuai host controller

Jangan menganggap semua USB controller memakai API xHCI. Interface menyimpan
controller.kind dan controller.instance.

Implementasi sekarang:
- xHCI menggunakan slot ID, DCI, transfer ring, event ring, dan EndpointHandle.
- EHCI/OHCI mempunyai queue/schedule dan jalur transfer sendiri.
- HID boot keyboard/mouse menggunakan xHCI endpoint path untuk xHCI dan
  hid_periodic untuk EHCI/OHCI.
- USB Mass Storage memilih implementasi transfer sesuai jenis controller.

Pada xHCI, API penting antara lain:
- configure_endpoint(slot_id, root_port, endpoint);
- configure_endpoints(slot_id, root_port, endpoints) untuk beberapa
  endpoint dalam satu Configure Endpoint command;
- control_transfer_in, control_transfer_out, control_transfer_no_data;
- bulk_transfer.

Jika interface membutuhkan Bulk IN dan Bulk OUT, pertimbangkan konfigurasi
keduanya dalam satu pemanggilan configure_endpoints(). xHCI mengolah Added
Context flags dalam satu command; memecah konfigurasi endpoint bisa mengubah
context endpoint sebelumnya. Selalu baca komentar/invariant pada implementasi
transport sebelum mengubahnya.

## 6. Pola lifecycle USB

### Probe
1. Pastikan DeviceInfo::UsbInterface.
2. Cocokkan class/subclass/protocol.
3. Validasi endpoint, arah, transfer type, ukuran paket, dan interval.
4. Lakukan class request melalui EP0 bila diperlukan.
5. Alokasikan buffer/ring dengan alignment dan alamat DMA yang tepat.
6. Configure endpoint lalu simpan state milik interface.

### Data path
- Control transfer dipakai untuk request standard/class/vendor.
- Bulk dipakai untuk throughput data (mis. Mass Storage BOT).
- Interrupt dipakai untuk report periodik HID.
- Isochronous digunakan untuk stream timing-sensitive (belum ada class audio
  driver pada dukungan saat ini).

Jangan alokasikan buffer baru setiap kali polling report. Reuse buffer, cek
actual transfer length/status, dan re-arm transfer jika protokol transport
memerlukannya.

### Disconnect/remove
Hotplug USB diproses melalui usb::poll_hotplug() pada
drivers::poll_deferred(), bukan dari ISR. Jalur disconnect harus:
1. mencegah transfer baru;
2. menghentikan endpoint/controller activity;
3. melepas atau menandai state transfer;
4. unregister class interface dan memanggil remove();
5. melepas DMA buffer hanya setelah controller tidak dapat mengaksesnya.

Jangan membebaskan ring/buffer DMA hanya karena port change event terdeteksi;
pastikan tidak ada transfer in-flight yang masih mereferensikan buffer tersebut.

## 7. Tambahkan test dan jalankan

Test host yang baik menguji bagian murni:
- parser descriptor dengan length 0, truncated, malformed, dan valid;
- matching class/protocol;
- pemetaan endpoint address/transfer type;
- decode report HID;
- pembentukan CBW/CSW/SCSI command untuk mass storage;
- state machine error/timeout.

Lalu build dan uji runtime:
    make -C user/rust make
    make -C user
    make -C user dynamic
    cargo +nightly test --manifest-path kernel/Cargo.toml --target x86_64-unknown-linux-gnu
    make -C kernel KARCH=x86_64
    make run

Untuk QEMU xHCI dan keyboard:
    make run QEMUFLAGS="-m 2G -device qemu-xhci,id=xhci -device usb-kbd,bus=xhci.0"

Verifikasi PCI controller ditemukan, xHCI/EHCI/OHCI probe sukses, port/device
terenumerasi, class driver bind, lalu operasi nyata berhasil. Cabut/pasang
ulang hanya bila jalur hotplug perangkat tersebut sudah aman untuk diuji.

## 8. Status dukungan saat ini

Kode repository mencakup host controller xHCI, EHCI, OHCI, USB descriptor
parsing, HID boot keyboard/mouse, USB Mass Storage Bulk-Only Transport,
RNDIS, dan CDC-NCM. Itu **tidak** berarti semua model device kompatibel:
perangkat dapat memerlukan descriptor/protokol khusus, firmware, power
sequencing, atau transport yang belum diimplementasikan.

Belum ada dukungan class driver umum untuk audio, webcam/UVC, printer, USB
serial, HID Report Protocol generik, atau USB Attached SCSI/UASP. QEMU test
dan hardware target perlu dicatat terpisah; parser/unit test host tidak
membuktikan kompatibilitas setiap perangkat fisik.

## 9. Dokumentasi implementasi lanjutan

- [Dokumentasi xHCI](DOKUMENTASI-XHCI.md)
- [Dokumentasi EHCI](DOKUMENTASI-EHCI.md)
- [Dokumentasi OHCI](DOKUMENTASI-OHCI.md)
- [HID USB](CARA-BIKIN-DRIVER-HID-USB.md)
- [USB generik](PANDUAN-DRIVER-USB-GENERIK.md)
- [Storage/block](storage.md)
- [Network](networking.md)
