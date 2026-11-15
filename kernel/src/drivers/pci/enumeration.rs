use super::{PciDevice, config_read_u8, config_read_u16};
use crate::println;

fn probe_function(bus: u8, device: u8, function: u8) -> Option<PciDevice> {
    let vendor_id = unsafe { config_read_u16(bus, device, function, 0x00) };
    if vendor_id == 0xFFFF {
        return None;
    }

    Some(PciDevice {
        bus,
        device,
        function,
        vendor_id,
        device_id: unsafe { config_read_u16(bus, device, function, 0x02) },
        class: unsafe { config_read_u8(bus, device, function, 0x0B) },
        subclass: unsafe { config_read_u8(bus, device, function, 0x0A) },
        prog_if: unsafe { config_read_u8(bus, device, function, 0x09) },
        header_type: unsafe { config_read_u8(bus, device, function, 0x0E) },
    })
}

/// Enumerate PCI functions once and return a reusable snapshot for drivers.
/// Find the PCI-to-PCI bridge that owns `target_bus`, without printing anything.
///
/// Used to reach the root port of a slot: ASPM has to be disabled on both ends
/// of the link, and the QCA9377 is known to raise PCIe errors on this platform
/// family when the root port keeps requesting L1.
pub fn find_parent_bridge(target_bus: u8) -> Option<PciDevice> {
    for bus in 0..=u8::MAX {
        for device in 0..32 {
            let Some(function) = probe_function(bus, device, 0) else {
                continue;
            };
            if function.class != 0x06 || function.subclass != 0x04 {
                continue;
            }
            // Configuration Space header byte 0x19 is the secondary bus number.
            let secondary = unsafe { super::config_read_u8(bus, device, 0, 0x19) };
            if secondary == target_bus {
                return Some(function);
            }
        }
    }
    None
}

pub fn scan_pci_bus() -> alloc::vec::Vec<PciDevice> {
    println!("=== PCI SCAN START ===");
    let mut devices = alloc::vec::Vec::new();

    for bus in 0..=u8::MAX {
        for device in 0..32 {
            let Some(function_zero) = probe_function(bus, device, 0) else {
                continue;
            };
            print_device(&function_zero);
            enable_bridge_bus_master(&function_zero);
            let is_multifunction = function_zero.header_type & 0x80 != 0;
            devices.push(function_zero);

            if is_multifunction {
                for function in 1..8 {
                    if let Some(device) = probe_function(bus, device, function) {
                        print_device(&device);
                        enable_bridge_bus_master(&device);
                        devices.push(device);
                    }
                }
            }
        }
    }

    println!("=== PCI SCAN END ===");
    devices
}

fn print_device(device: &PciDevice) {
    println!(
        "PCI {:02x}:{:02x}.{} vendor={:#06x} device={:#06x} class={:#04x} subclass={:#04x} prog_if={:#04x}",
        device.bus,
        device.device,
        device.function,
        device.vendor_id,
        device.device_id,
        device.class,
        device.subclass,
        device.prog_if
    );
}

fn enable_bridge_bus_master(dev: &PciDevice) {
    if dev.class == 0x06 && dev.subclass == 0x04 {
        let cmd = unsafe { super::config_read_u16(dev.bus, dev.device, dev.function, 0x04) };
        let updated = cmd | 0x0006; // CMD_MEMORY_SPACE (0x2) | CMD_BUS_MASTER (0x4)
        if updated != cmd {
            unsafe { super::config_write_u16(dev.bus, dev.device, dev.function, 0x04, updated) };
        }
    }
}
