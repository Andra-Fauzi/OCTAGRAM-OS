use crate::arch::x86_64::io::{inl, outl};

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

fn config_address(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let bus = bus as u32;
    let device = (device & 0x1F) as u32;
    let function = (function & 0x07) as u32;
    let offset = (offset & 0xFC) as u32;

    (1 << 31) | (bus << 16) | (device << 11) | (function << 8) | offset
}

pub(super) unsafe fn config_read_u32(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let address = config_address(bus, device, function, offset);
    unsafe {
        outl(CONFIG_ADDRESS, address);
        inl(CONFIG_DATA)
    }
}

pub(super) unsafe fn config_read_u16(bus: u8, device: u8, function: u8, offset: u8) -> u16 {
    let value = unsafe { config_read_u32(bus, device, function, offset & 0xFC) };
    let shift = (offset & 2) * 8;
    ((value >> shift) & 0xFFFF) as u16
}

pub(super) unsafe fn config_read_u8(bus: u8, device: u8, function: u8, offset: u8) -> u8 {
    let value = unsafe { config_read_u32(bus, device, function, offset & 0xFC) };
    let shift = (offset & 3) * 8;
    ((value >> shift) & 0xFF) as u8
}

pub(super) unsafe fn config_write_u32(bus: u8, device: u8, function: u8, offset: u8, value: u32) {
    let address = config_address(bus, device, function, offset);
    unsafe {
        outl(CONFIG_ADDRESS, address);
        outl(CONFIG_DATA, value);
    }
}

pub(super) unsafe fn config_write_u16(bus: u8, device: u8, function: u8, offset: u8, value: u16) {
    let old = unsafe { config_read_u32(bus, device, function, offset & 0xFC) };
    let shift = (offset & 2) * 8;
    let mask = !(0xFFFFu32 << shift);
    let new = (old & mask) | ((value as u32) << shift);

    unsafe { config_write_u32(bus, device, function, offset & 0xFC, new) };
}
