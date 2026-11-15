use super::{
    PciDevice, config_read_u8, config_read_u16, config_read_u32, config_write_u16, config_write_u32,
};

const PCI_STATUS: u8 = 0x06;
const PCI_CAPABILITIES_PTR: u8 = 0x34;

#[allow(dead_code)]
pub const CAP_ID_POWER_MGMT: u8 = 0x01;
pub const CAP_ID_MSI: u8 = 0x05;
pub const CAP_ID_MSIX: u8 = 0x11;
#[allow(dead_code)]
pub const CAP_ID_PCIE: u8 = 0x10;

/// PCI MSI-X table and pending-bit-array locations.
#[allow(dead_code)]
pub struct MsixInfo {
    pub cap_offset: u8,
    pub table_bar: u8,
    pub table_offset: u32,
    pub pba_bar: u8,
    pub pba_offset: u32,
    pub table_size: u16,
}

/// Find a capability in the PCI linked list, bounded to avoid malformed loops.
const PM_CSR_OFFSET: u8 = 0x04;
const PM_CSR_POWER_STATE_MASK: u16 = 0x0003;

/// Force this function into D0. Apollo Lake firmware can leave the WLAN in
/// D3hot/D3cold, where config space still answers but the target ROM stays
/// halted, and the ath10k report "Unable to change power state from D3cold to
/// D0, device inaccessible" shows the same platform behaviour. Returns the PM
/// control/status value before and after so the boot log can show the move.
pub unsafe fn force_d0(device: &PciDevice) -> Option<(u16, u16)> {
    let capability = unsafe { find_capability(device, CAP_ID_POWER_MGMT) }?;
    let offset = capability + PM_CSR_OFFSET;
    let before = unsafe { config_read_u16(device.bus, device.device, device.function, offset) };
    let powered = before & !PM_CSR_POWER_STATE_MASK;
    if powered != before {
        unsafe {
            config_write_u16(device.bus, device.device, device.function, offset, powered)
        };
    }
    let after = unsafe { config_read_u16(device.bus, device.device, device.function, offset) };
    Some((before, after))
}

const PCIE_LINK_CONTROL_OFFSET: u8 = 0x10;
const PCIE_LINK_CONTROL_ASPMC: u16 = 0x0003;

/// Disable PCIe ASPM L0s/L1 on this function, mirroring
/// `ath10k_pci_aspm_disable()`. An endpoint that is allowed to enter a link
/// low-power state between host accesses can stall copy-engine DMA, which then
/// surfaces as a transfer timeout instead of a link error. Returns the link
/// control value before and after so the boot log can show the change.
pub unsafe fn disable_aspm(device: &PciDevice) -> Option<(u16, u16)> {
    let capability = unsafe { find_capability(device, CAP_ID_PCIE) }?;
    let offset = capability + PCIE_LINK_CONTROL_OFFSET;
    let before = unsafe { config_read_u16(device.bus, device.device, device.function, offset) };
    let disabled = before & !PCIE_LINK_CONTROL_ASPMC;
    if disabled != before {
        unsafe {
            config_write_u16(device.bus, device.device, device.function, offset, disabled)
        };
    }
    let after = unsafe { config_read_u16(device.bus, device.device, device.function, offset) };
    Some((before, after))
}

pub unsafe fn find_capability(device: &PciDevice, cap_id: u8) -> Option<u8> {
    let status = unsafe { config_read_u16(device.bus, device.device, device.function, PCI_STATUS) };
    if status & (1 << 4) == 0 {
        return None;
    }

    let mut offset = unsafe {
        config_read_u8(
            device.bus,
            device.device,
            device.function,
            PCI_CAPABILITIES_PTR,
        )
    };
    offset &= 0xFC;

    for _ in 0..48 {
        if offset == 0 {
            break;
        }
        if unsafe { config_read_u8(device.bus, device.device, device.function, offset) } == cap_id {
            return Some(offset);
        }
        offset = unsafe { config_read_u8(device.bus, device.device, device.function, offset + 1) }
            & 0xFC;
    }
    None
}

/// Enable legacy MSI for a device.
#[allow(dead_code)]
pub unsafe fn enable_msi(device: &PciDevice, vector: u8, apic_id: u8) {
    if let Some(offset) = unsafe { find_capability(device, CAP_ID_MSI) } {
        let message_address = 0xFEE0_0000u32 | ((apic_id as u32) << 12);
        unsafe {
            config_write_u32(
                device.bus,
                device.device,
                device.function,
                offset + 4,
                message_address,
            );
            config_write_u16(
                device.bus,
                device.device,
                device.function,
                offset + 8,
                vector as u16,
            );
            let control = config_read_u16(device.bus, device.device, device.function, offset + 2);
            config_write_u16(
                device.bus,
                device.device,
                device.function,
                offset + 2,
                control | 1,
            );
        }
        crate::println!("MSI enabled: vector={} apic_id={}", vector, apic_id);
    } else {
        crate::println!("WARNING: device does not support MSI");
    }
}

/// Read MSI-X table and pending-bit-array locations.
pub unsafe fn get_msix_info(device: &PciDevice) -> Option<MsixInfo> {
    let cap_offset = unsafe { find_capability(device, CAP_ID_MSIX) }?;
    let control =
        unsafe { config_read_u16(device.bus, device.device, device.function, cap_offset + 2) };
    let table_reg =
        unsafe { config_read_u32(device.bus, device.device, device.function, cap_offset + 4) };
    let pba_reg =
        unsafe { config_read_u32(device.bus, device.device, device.function, cap_offset + 8) };

    Some(MsixInfo {
        cap_offset,
        table_bar: (table_reg & 0x7) as u8,
        table_offset: table_reg & !0x7,
        pba_bar: (pba_reg & 0x7) as u8,
        pba_offset: pba_reg & !0x7,
        table_size: (control & 0x7FF) + 1,
    })
}

/// Enable or disable MSI-X globally for this PCI device.
pub unsafe fn set_msix_enable(device: &PciDevice, cap_offset: u8, enable: bool) {
    let control =
        unsafe { config_read_u16(device.bus, device.device, device.function, cap_offset + 2) };
    let control = if enable {
        (control | (1 << 15)) & !(1 << 14)
    } else {
        control & !(1 << 15)
    };
    unsafe {
        config_write_u16(
            device.bus,
            device.device,
            device.function,
            cap_offset + 2,
            control,
        )
    };
}
