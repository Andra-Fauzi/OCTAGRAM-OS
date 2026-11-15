//! PCI bus access split by responsibility: enumeration, configuration space,
//! BARs, and optional capabilities.
//!
//! Enumerate devices once with [`scan_pci_bus`] and pass the snapshot to
//! device drivers. Driver-specific device matching belongs in each driver.

mod bar;
mod capability;
mod config;
mod enumeration;

pub(crate) use bar::disable_bus_master;
pub use bar::{
    BarInfo, enable_bus_master, enable_device, enable_mmio_only, get_bar, get_bar0, get_bar0_size,
};
pub use capability::{
    CAP_ID_MSI, CAP_ID_MSIX, CAP_ID_PCIE, CAP_ID_POWER_MGMT, MsixInfo, disable_aspm, enable_msi,
    find_capability, force_d0, get_msix_info, set_msix_enable,
};
pub use enumeration::{find_parent_bridge, scan_pci_bus};

use config::{
    config_read_u8, config_read_u16, config_read_u32, config_write_u16, config_write_u32,
};

/// One discovered PCI function and its standard identification fields.
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

impl PciDevice {
    /// Match the PCI class, subclass, and programming interface fields.
    pub const fn matches_class(&self, class: u8, subclass: u8, prog_if: u8) -> bool {
        self.class == class && self.subclass == subclass && self.prog_if == prog_if
    }
}

/// Read a dword from a discovered function's PCI configuration space.
pub fn read_config_u32(device: &PciDevice, offset: u8) -> u32 {
    unsafe { config_read_u32(device.bus, device.device, device.function, offset) }
}

/// Write a dword to a discovered function's PCI configuration space.
pub fn write_config_u32(device: &PciDevice, offset: u8, value: u32) {
    unsafe { config::config_write_u32(device.bus, device.device, device.function, offset, value) }
}

#[cfg(test)]
mod tests {
    use super::PciDevice;

    #[test]
    fn matches_class_requires_all_three_fields() {
        let device = PciDevice {
            bus: 0,
            device: 1,
            function: 0,
            vendor_id: 0x1234,
            device_id: 0x5678,
            class: 0x01,
            subclass: 0x06,
            prog_if: 0x01,
            header_type: 0,
        };

        assert!(device.matches_class(0x01, 0x06, 0x01));
        assert!(!device.matches_class(0x01, 0x06, 0x02));
    }
}
