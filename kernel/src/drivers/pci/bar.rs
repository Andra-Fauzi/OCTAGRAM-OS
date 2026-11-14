use super::{PciDevice, config_read_u16, config_read_u32, config_write_u16, config_write_u32};
use crate::println;

const PCI_COMMAND: u8 = 0x04;
const CMD_MEMORY_SPACE: u16 = 1 << 1;
const CMD_BUS_MASTER: u16 = 1 << 2;
const PCI_BAR0: u8 = 0x10;

#[derive(Debug, Clone, Copy)]
pub struct BarInfo {
    pub address: u64,
    pub is_64bit: bool,
    pub is_prefetchable: bool,
}

/// Enable memory space access and bus mastering on a PCI device.
pub unsafe fn enable_device(device: &PciDevice) {
    let mut command =
        unsafe { config_read_u16(device.bus, device.device, device.function, PCI_COMMAND) };
    command |= CMD_MEMORY_SPACE | CMD_BUS_MASTER;

    unsafe {
        config_write_u16(
            device.bus,
            device.device,
            device.function,
            PCI_COMMAND,
            command,
        );
    }

    let verify =
        unsafe { config_read_u16(device.bus, device.device, device.function, PCI_COMMAND) };
    println!(
        "PCI {:02x}:{:02x}.{} command register: {:#06x} -> {:#06x}",
        device.bus, device.device, device.function, command, verify
    );
}

/// Read BAR0 as a memory-mapped BAR.
pub unsafe fn get_bar0(device: &PciDevice) -> BarInfo {
    unsafe { get_bar(device, 0) }
}

/// Read a memory BAR by index (0..=5).
pub unsafe fn get_bar(device: &PciDevice, index: u8) -> BarInfo {
    assert!(index < 6, "PCI BAR index out of range");
    let offset = PCI_BAR0 + index * 4;
    let bar = unsafe { config_read_u32(device.bus, device.device, device.function, offset) };
    assert!(bar & 0x1 == 0, "PCI BAR is I/O space, not memory space");

    let bar_type = (bar >> 1) & 0x3;
    let is_prefetchable = (bar >> 3) & 0x1 == 1;
    let is_64bit = bar_type == 0x2;
    let base_low = (bar & 0xFFFF_FFF0) as u64;
    let address = if is_64bit {
        assert!(index < 5, "64-bit BAR cannot start at BAR5");
        let base_high =
            unsafe { config_read_u32(device.bus, device.device, device.function, offset + 4) };
        ((base_high as u64) << 32) | base_low
    } else {
        base_low
    };

    BarInfo {
        address,
        is_64bit,
        is_prefetchable,
    }
}

/// Read the size of BAR0 in bytes, restoring its original value afterward.
pub unsafe fn get_bar0_size(device: &PciDevice) -> u64 {
    let original = unsafe { config_read_u32(device.bus, device.device, device.function, PCI_BAR0) };
    unsafe {
        config_write_u32(
            device.bus,
            device.device,
            device.function,
            PCI_BAR0,
            u32::MAX,
        )
    };
    let readback = unsafe { config_read_u32(device.bus, device.device, device.function, PCI_BAR0) };
    unsafe {
        config_write_u32(
            device.bus,
            device.device,
            device.function,
            PCI_BAR0,
            original,
        )
    };

    let mask = readback & 0xFFFF_FFF0;
    if (original >> 1) & 0x3 == 0x2 {
        let original_high =
            unsafe { config_read_u32(device.bus, device.device, device.function, PCI_BAR0 + 4) };
        unsafe {
            config_write_u32(
                device.bus,
                device.device,
                device.function,
                PCI_BAR0 + 4,
                u32::MAX,
            )
        };
        let readback_high =
            unsafe { config_read_u32(device.bus, device.device, device.function, PCI_BAR0 + 4) };
        unsafe {
            config_write_u32(
                device.bus,
                device.device,
                device.function,
                PCI_BAR0 + 4,
                original_high,
            )
        };

        let full_mask = ((readback_high as u64) << 32) | mask as u64;
        (!full_mask).wrapping_add(1)
    } else {
        (!(mask as u64) as u32).wrapping_add(1) as u64
    }
}
