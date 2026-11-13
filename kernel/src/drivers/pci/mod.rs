//! Driver PCI: enumerasi bus, baca/tulis Configuration Space, MSI/MSI-X.
//!
//! # Cara pakai
//! ```ignore
//! // Scan semua bus:
//! let devices = pci::scan_pci_bus();
//!
//! // Cari USB controller:
//! let usb = pci::find_usb_controllers();
//! ```
//!
//! Implementasi menggunakan PCI configuration mechanism #1 (port I/O
//! 0xCF8/0xCFC) — kompatibel dengan semua hardware x86 modern.

use crate::arch::x86_64::io::{inl, outl};
use crate::println;

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

/// USB class code di PCI (class = 0x0C, subclass = 0x03).
pub const PCI_CLASS_USB: u8 = 0x0C;
/// USB subclass code di PCI.
pub const PCI_SUBCLASS_USB: u8 = 0x03;

/// USB Programming Interface values.
pub const PCI_PI_UHCI: u8 = 0x00;
/// USB Programming Interface: OHCI.
pub const PCI_PI_OHCI: u8 = 0x10;
/// USB Programming Interface: EHCI.
pub const PCI_PI_EHCI: u8 = 0x20;
/// USB Programming Interface: xHCI.
pub const PCI_PI_XHCI: u8 = 0x30;
pub const PCI_CLASS_MASS_STORAGE: u8 = 0x01;
pub const PCI_SUBCLASS_SATA: u8 = 0x06;
pub const PCI_PI_AHCI: u8 = 0x01;

/// Representasi satu PCI device/function yang berhasil dideteksi.
#[derive(Debug, Clone, Copy)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub header_type: u8,
}

/// Bikin alamat config space sesuai spec legacy PCI configuration mechanism #1
fn config_address(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let bus = bus as u32;
    let device = (device & 0x1F) as u32;
    let function = (function & 0x07) as u32;
    let offset = (offset & 0xFC) as u32; // harus 4-byte aligned

    (1 << 31) | (bus << 16) | (device << 11) | (function << 8) | offset
}

unsafe fn config_read_u32(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let addr = config_address(bus, device, function, offset);
    unsafe {
        outl(CONFIG_ADDRESS, addr);
        inl(CONFIG_DATA)
    }
}

unsafe fn config_read_u16(bus: u8, device: u8, function: u8, offset: u8) -> u16 {
    let value = unsafe { config_read_u32(bus, device, function, offset & 0xFC) };
    let shift = (offset & 2) * 8;
    ((value >> shift) & 0xFFFF) as u16
}

unsafe fn config_read_u8(bus: u8, device: u8, function: u8, offset: u8) -> u8 {
    let value = unsafe { config_read_u32(bus, device, function, offset & 0xFC) };
    let shift = (offset & 3) * 8;
    ((value >> shift) & 0xFF) as u8
}

unsafe fn config_write_u32(bus: u8, device: u8, function: u8, offset: u8, value: u32) {
    let addr = config_address(bus, device, function, offset);
    unsafe {
        outl(CONFIG_ADDRESS, addr);
        outl(CONFIG_DATA, value);
    }
}

unsafe fn config_write_u16(bus: u8, device: u8, function: u8, offset: u8, value: u16) {
    // 1. baca 32-bit penuh dari word yang relevan (offset dibulatkan ke bawah, align 4)
    let old = unsafe { config_read_u32(bus, device, function, offset & 0xFC) };

    // 2. tentukan posisi 16-bit yang mau diubah (atas atau bawah)
    let shift = (offset & 2) * 8; // 0 atau 16

    // mask buat "menghapus" 16-bit lama di posisi itu, biarin 16-bit lainnya utuh
    let mask = !(0xFFFFu32 << shift);
    let new = (old & mask) | ((value as u32) << shift);

    // 3. tulis balik 32-bit penuh
    unsafe { config_write_u32(bus, device, function, offset & 0xFC, new) };
}

unsafe fn config_write_u8(bus: u8, device: u8, function: u8, offset: u8, value: u8) {
    let old = unsafe { config_read_u32(bus, device, function, offset & 0xFC) };
    let shift = (offset & 3) * 8;
    let mask = !(0xFFu32 << shift);
    let new = (old & mask) | ((value as u32) << shift);
    unsafe { config_write_u32(bus, device, function, offset & 0xFC, new) };
}

/// Cek satu function. Return None kalau vendor_id == 0xFFFF (artinya kosong/gak ada device)
fn probe_function(bus: u8, device: u8, function: u8) -> Option<PciDevice> {
    let vendor_id = unsafe { config_read_u16(bus, device, function, 0x00) };
    if vendor_id == 0xFFFF {
        return None;
    }

    let device_id = unsafe { config_read_u16(bus, device, function, 0x02) };
    let class = unsafe { config_read_u8(bus, device, function, 0x0B) };
    let subclass = unsafe { config_read_u8(bus, device, function, 0x0A) };
    let prog_if = unsafe { config_read_u8(bus, device, function, 0x09) };
    let header_type = unsafe { config_read_u8(bus, device, function, 0x0E) };

    Some(PciDevice {
        bus,
        device,
        function,
        vendor_id,
        device_id,
        class,
        subclass,
        prog_if,
        header_type,
    })
}

/// Brute-force scan semua bus/device/function.
/// Cukup buat hobby OS; nanti bisa diganti pakai MCFG (dari ACPI) buat akses lebih cepat.
pub fn scan_pci_bus() {
    println!("=== PCI SCAN START ===");

    for bus in 0..=255u8 {
        for device in 0..32u8 {
            // Function 0 dicek dulu buat tau apakah device ada & apakah multi-function
            if let Some(dev) = probe_function(bus, device, 0) {
                print_device(&dev);

                let is_multifunction = dev.header_type & 0x80 != 0;
                if is_multifunction {
                    for function in 1..8u8 {
                        if let Some(dev) = probe_function(bus, device, function) {
                            print_device(&dev);
                        }
                    }
                }
            }
        }

        if bus == 255 {
            break; // hindari overflow u8 di `bus + 1`
        }
    }

    println!("=== PCI SCAN END ===");
}

fn print_device(dev: &PciDevice) {
    println!(
        "PCI {:02x}:{:02x}.{} vendor={:#06x} device={:#06x} class={:#04x} subclass={:#04x} prog_if={:#04x}",
        dev.bus,
        dev.device,
        dev.function,
        dev.vendor_id,
        dev.device_id,
        dev.class,
        dev.subclass,
        dev.prog_if
    );
}

const PCI_COMMAND: u8 = 0x04;

const CMD_MEMORY_SPACE: u16 = 1 << 0;
const CMD_BUS_MASTER: u16 = 1 << 2;

/// Enable memory space access & bus mastering (DMA) pada device.
/// Wajib dipanggil sebelum akses BAR / sebelum controller bisa DMA.
pub unsafe fn enable_device(dev: &PciDevice) {
    let mut command = unsafe { config_read_u16(dev.bus, dev.device, dev.function, PCI_COMMAND) };

    command |= CMD_MEMORY_SPACE;
    command |= CMD_BUS_MASTER;

    unsafe {
        config_write_u16(dev.bus, dev.device, dev.function, PCI_COMMAND, command);
    }

    // baca balik buat verifikasi (opsional tapi berguna buat debug)
    let verify = unsafe { config_read_u16(dev.bus, dev.device, dev.function, PCI_COMMAND) };
    println!(
        "PCI {:02x}:{:02x}.{} command register: {:#06x} -> {:#06x}",
        dev.bus, dev.device, dev.function, command, verify
    );
}

const PCI_BAR0: u8 = 0x10;

/// Info lengkap tentang satu BAR (Base Address Register).
#[derive(Debug, Clone, Copy)]
pub struct BarInfo {
    pub address: u64,
    pub is_64bit: bool,
    pub is_prefetchable: bool,
}

/// Baca BAR0. Otomatis deteksi apakah 32-bit atau 64-bit BAR
/// (xHCI hampir selalu 64-bit karena butuh address space besar).
pub unsafe fn get_bar0(dev: &PciDevice) -> BarInfo {
    unsafe { get_bar(dev, 0) }
}

/// Read a memory BAR by index (0..=5).
pub unsafe fn get_bar(dev: &PciDevice, index: u8) -> BarInfo {
    assert!(index < 6, "PCI BAR index out of range");
    let offset = PCI_BAR0 + index * 4;
    let bar0 = unsafe { config_read_u32(dev.bus, dev.device, dev.function, offset) };

    // bit 0 harus 0 (memory space BAR, bukan I/O space BAR)
    assert!(
        bar0 & 0x1 == 0,
        "BAR0 bukan memory-mapped BAR (ini I/O BAR)"
    );

    let bar_type = (bar0 >> 1) & 0x3; // bit 2-1
    let is_prefetchable = (bar0 >> 3) & 0x1 == 1; // bit 3
    let is_64bit = bar_type == 0x2;

    let base_low = (bar0 & 0xFFFF_FFF0) as u64; // buang 4 bit flag terbawah

    let address = if is_64bit {
        assert!(index < 5, "64-bit BAR cannot start at BAR5");
        let bar1 = unsafe { config_read_u32(dev.bus, dev.device, dev.function, offset + 4) };
        ((bar1 as u64) << 32) | base_low
    } else {
        base_low
    };

    BarInfo {
        address,
        is_64bit,
        is_prefetchable,
    }
}

/// Baca ukuran region yang di-map oleh BAR0 (dalam byte).
pub unsafe fn get_bar0_size(dev: &PciDevice) -> u64 {
    // 1. simpan nilai asli BAR (biar bisa dikembalikan lagi)
    let original = unsafe { config_read_u32(dev.bus, dev.device, dev.function, PCI_BAR0) };

    // 2. tulis semua bit 1
    unsafe { config_write_u32(dev.bus, dev.device, dev.function, PCI_BAR0, 0xFFFF_FFFF) };

    // 3. baca balik — hardware cuma bakal nge-set bit yang "valid" (sisanya otomatis 0
    //    karena bit-bit itu hardwired ke 0 sesuai berapa address space yang device butuh)
    let readback = unsafe { config_read_u32(dev.bus, dev.device, dev.function, PCI_BAR0) };

    // 4. kembalikan nilai asli (WAJIB, biar device gak "rusak" konfigurasinya)
    unsafe { config_write_u32(dev.bus, dev.device, dev.function, PCI_BAR0, original) };

    // 5. hitung size dari hasil readback
    let is_64bit = (original >> 1) & 0x3 == 0x2;
    let mask = readback & 0xFFFF_FFF0; // buang 4 bit flag bawah

    if is_64bit {
        // buat 64-bit BAR, ulangi proses yang sama di BAR1 (bagian atas)
        let original_hi =
            unsafe { config_read_u32(dev.bus, dev.device, dev.function, PCI_BAR0 + 4) };
        unsafe { config_write_u32(dev.bus, dev.device, dev.function, PCI_BAR0 + 4, 0xFFFF_FFFF) };
        let readback_hi =
            unsafe { config_read_u32(dev.bus, dev.device, dev.function, PCI_BAR0 + 4) };
        unsafe { config_write_u32(dev.bus, dev.device, dev.function, PCI_BAR0 + 4, original_hi) };

        let full_mask = ((readback_hi as u64) << 32) | (mask as u64);
        (!full_mask).wrapping_add(1) // two's complement buat dapetin size
    } else {
        (!(mask as u64) as u32).wrapping_add(1) as u64
    }
}

/// Find PCI AHCI SATA controllers (class 01h, subclass 06h, prog-if 01h).
pub fn find_ahci_controllers() -> alloc::vec::Vec<PciDevice> {
    let mut found = alloc::vec::Vec::new();
    for bus in 0..=255u8 {
        for device in 0..32u8 {
            if let Some(dev) = probe_function(bus, device, 0) {
                if dev.class == PCI_CLASS_MASS_STORAGE
                    && dev.subclass == PCI_SUBCLASS_SATA
                    && dev.prog_if == PCI_PI_AHCI
                {
                    found.push(dev);
                }

                if dev.header_type & 0x80 != 0 {
                    for function in 1..8u8 {
                        if let Some(dev) = probe_function(bus, device, function) {
                            if dev.class == PCI_CLASS_MASS_STORAGE
                                && dev.subclass == PCI_SUBCLASS_SATA
                                && dev.prog_if == PCI_PI_AHCI
                            {
                                found.push(dev);
                            }
                        }
                    }
                }
            }
        }
        if bus == 255 {
            break;
        }
    }
    found
}

/// Cari semua USB host controller (class 0x0C, subclass 0x03)
/// prog_if: 0x00=UHCI, 0x10=OHCI, 0x20=EHCI, 0x30=xHCI
pub fn find_usb_controllers() -> alloc::vec::Vec<PciDevice> {
    let mut found = alloc::vec::Vec::new();

    for bus in 0..=255u8 {
        for device in 0..32u8 {
            if let Some(dev) = probe_function(bus, device, 0) {
                if dev.class == PCI_CLASS_USB && dev.subclass == PCI_SUBCLASS_USB {
                    found.push(dev);
                }

                let is_multifunction = dev.header_type & 0x80 != 0;
                if is_multifunction {
                    for function in 1..8u8 {
                        if let Some(dev) = probe_function(bus, device, function) {
                            if dev.class == PCI_CLASS_USB && dev.subclass == PCI_SUBCLASS_USB {
                                found.push(dev);
                            }
                        }
                    }
                }
            }
        }

        if bus == 255 {
            break;
        }
    }

    found
}

const PCI_STATUS: u8 = 0x06;
const PCI_CAPABILITIES_PTR: u8 = 0x34;

// Tabel referensi capability ID PCI -- POWER_MGMT & PCIE belum dipakai
// di kode sekarang, disimpan supaya lengkap sesuai spec.
#[allow(dead_code)]
pub const CAP_ID_POWER_MGMT: u8 = 0x01;
/// Capability ID untuk MSI (Message Signaled Interrupts).
pub const CAP_ID_MSI: u8 = 0x05;
/// Capability ID untuk MSI-X (Extended MSI).
pub const CAP_ID_MSIX: u8 = 0x11;
#[allow(dead_code)]
pub const CAP_ID_PCIE: u8 = 0x10;

/// Cari capability tertentu di linked-list capability PCI device.
/// Return offset-nya kalau ketemu, None kalau device gak punya fitur itu.
pub unsafe fn find_capability(dev: &PciDevice, cap_id: u8) -> Option<u8> {
    let status = unsafe { config_read_u16(dev.bus, dev.device, dev.function, PCI_STATUS) };

    // bit 4 di Status register = device punya Capability List atau nggak
    if status & (1 << 4) == 0 {
        return None;
    }

    let mut ptr =
        unsafe { config_read_u8(dev.bus, dev.device, dev.function, PCI_CAPABILITIES_PTR) };
    ptr &= 0xFC; // align, 2 bit bawah reserved

    // batasin iterasi biar gak infinite loop kalau ada data korup/aneh
    let mut safety_counter = 0;

    while ptr != 0 && safety_counter < 48 {
        let id = unsafe { config_read_u8(dev.bus, dev.device, dev.function, ptr) };

        if id == cap_id {
            return Some(ptr);
        }

        ptr = unsafe { config_read_u8(dev.bus, dev.device, dev.function, ptr + 1) };
        ptr &= 0xFC;

        safety_counter += 1;
    }

    None
}

/// Legacy MSI (bukan MSI-X). Tidak dipanggil di alur sekarang karena
/// MSI-X diprioritaskan (lihat `main.rs`), disimpan sebagai fallback
/// untuk controller yang cuma dukung MSI biasa.
#[allow(dead_code)]
pub unsafe fn enable_msi(dev: &PciDevice, vector: u8, apic_id: u8) {
    unsafe {
        if let Some(msi_ptr) = find_capability(dev, 0x05) {
            let msg_addr = 0xFEE0_0000u32 | ((apic_id as u32) << 12);
            let msg_data = vector as u32;

            config_write_u32(dev.bus, dev.device, dev.function, msi_ptr + 4, msg_addr);
            config_write_u16(
                dev.bus,
                dev.device,
                dev.function,
                msi_ptr + 8,
                msg_data as u16,
            );

            let mut ctrl = config_read_u16(dev.bus, dev.device, dev.function, msi_ptr + 2);
            ctrl |= 1;
            config_write_u16(dev.bus, dev.device, dev.function, msi_ptr + 2, ctrl);

            println!("MSI enabled: vector={} apic_id={}", vector, apic_id);
        } else {
            println!("WARNING: device tidak support MSI!");
        }
    }
}

// Belum dipakai langsung (xhci.rs::setup_msix nulis raw u32 ke MMIO
// table), disimpan sebagai dokumentasi layout satu entry tabel MSI-X.
#[allow(dead_code)]
#[repr(C)]
struct MsixTableEntry {
    msg_addr_low: u32,
    msg_addr_high: u32,
    msg_data: u32,
    vector_control: u32,
}

// pba_bar/pba_offset (Pending Bit Array) belum dipakai -- disimpan
// karena tetap bagian dari layout MSI-X Capability Register.
#[allow(dead_code)]
pub struct MsixInfo {
    pub cap_offset: u8,
    pub table_bar: u8,
    pub table_offset: u32,
    pub pba_bar: u8,
    pub pba_offset: u32,
    pub table_size: u16,
}

/// Baca info MSI-X capability (table location, size) dari device.
pub unsafe fn get_msix_info(dev: &PciDevice) -> Option<MsixInfo> {
    unsafe {
        let cap_offset = find_capability(dev, CAP_ID_MSIX)?;

        let msg_ctrl = config_read_u16(dev.bus, dev.device, dev.function, cap_offset + 2);

        let table_size = (msg_ctrl & 0x7FF) + 1;

        let table_reg = config_read_u32(dev.bus, dev.device, dev.function, cap_offset + 4);
        let table_bar = (table_reg & 0x7) as u8;
        let table_offset = table_reg & !0x7;

        let pba_reg = config_read_u32(dev.bus, dev.device, dev.function, cap_offset + 8);
        let pba_bar = (pba_reg & 0x7) as u8;
        let pba_offset = pba_reg & !0x7;

        Some(MsixInfo {
            cap_offset,
            table_bar,
            table_offset,
            pba_bar,
            pba_offset,
            table_size,
        })
    }
}

/// Enable atau disable MSI-X global untuk device ini.
pub unsafe fn set_msix_enable(dev: &PciDevice, cap_offset: u8, enable: bool) {
    unsafe {
        let mut ctrl = config_read_u16(dev.bus, dev.device, dev.function, cap_offset + 2);
        if enable {
            ctrl |= 1 << 15;
            ctrl &= !(1 << 14);
        } else {
            ctrl &= !(1 << 15)
        }
        config_write_u16(dev.bus, dev.device, dev.function, cap_offset + 2, ctrl);
    }
}
