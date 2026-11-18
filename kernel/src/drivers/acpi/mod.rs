//! ACPI table discovery, power-off support for legacy PM1 control blocks, and
//! the AML reader used to switch a device's power resources on.

pub mod aml;
pub mod power;

use alloc::vec::Vec;
use core::arch::asm;
use core::hint::spin_loop;
use core::slice;

use spin::Once;

const RSDP_SIGNATURE: &[u8; 8] = b"RSD PTR ";
const SDT_HEADER_LENGTH: usize = 36;
const MAX_TABLE_LENGTH: usize = 1024 * 1024;
const MAX_RSDP_LENGTH: usize = 4096;
const SMI_CMD_OFFSET: usize = 48;
const ACPI_ENABLE_OFFSET: usize = 52;
const DSDT_ADDRESS_OFFSET: usize = 40;
const PM1A_CONTROL_OFFSET: usize = 64;
const PM1B_CONTROL_OFFSET: usize = 68;
const PM1_CONTROL_LENGTH_OFFSET: usize = 89;
const RESET_REGISTER_OFFSET: usize = 116;
const RESET_VALUE_OFFSET: usize = 128;
const GENERIC_ADDRESS_LENGTH: usize = 12;
/// `AddressSpaceId` GAS untuk memori dan I/O port.
const GAS_SYSTEM_MEMORY: u8 = 0;
const GAS_SYSTEM_IO: u8 = 1;
const SCI_ENABLE: u16 = 1;
const SLEEP_TYPE_SHIFT: u16 = 10;
const SLEEP_TYPE_MASK: u16 = 0b111 << SLEEP_TYPE_SHIFT;
const SLEEP_ENABLE: u16 = 1 << 13;
const INTERRUPT_FLAG: u64 = 1 << 9;
const MAX_POLL_ATTEMPTS: usize = 1_000_000;

static POWER_CONFIG: Once<Result<PowerConfig, AcpiError>> = Once::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpiError {
    RsdpUnavailable,
    InvalidRsdp,
    InvalidTable,
    FadtUnavailable,
    S5Unavailable,
    UnsupportedPowerInterface,
    AcpiEnableTimeout,
    PowerOffFailed,
    RebootFailed,
    MemoryMappingFailed,
}

#[derive(Debug, Clone, Copy)]
struct PowerConfig {
    pm1a_control: u16,
    pm1b_control: Option<u16>,
    smi_command: Option<(u16, u8)>,
    sleep_type_a: u8,
    sleep_type_b: u8,
    reset: Option<ResetRegister>,
}

/// Register reset FADT (`RESET_REG`/`RESET_VALUE`) yang dipakai `reboot`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResetRegister {
    address_space: u8,
    access_size: u8,
    address: u64,
    value: u8,
}

/// Discover and validate the ACPI tables needed by the shutdown command.
pub fn init() -> Result<(), AcpiError> {
    power_config().map(|_| ())
}

/// Request an ACPI S5 power-off. Returns an error if firmware does not power
/// down the machine after the sleep command is issued.
pub fn power_off() -> Result<(), AcpiError> {
    let config = *power_config()?;
    enable_acpi(config)?;

    let flags: u64;
    unsafe {
        asm!("pushfq; pop {}", out(reg) flags, options(preserves_flags));
        asm!("cli", options(nomem, nostack, preserves_flags));
    }

    if let Some(port) = config.pm1b_control {
        write_sleep_control(port, config.sleep_type_b);
    }
    write_sleep_control(config.pm1a_control, config.sleep_type_a);

    for _ in 0..MAX_POLL_ATTEMPTS {
        spin_loop();
    }

    if flags & INTERRUPT_FLAG != 0 {
        unsafe { asm!("sti", options(nomem, nostack, preserves_flags)) };
    }
    Err(AcpiError::PowerOffFailed)
}

/// Baca register reset dari FADT. `None` bila firmware tidak menyediakan
/// register yang bisa dipakai (mis. hanya ACPI 1.0 atau bit width aneh).
fn reset_register(fadt: &[u8]) -> Option<ResetRegister> {
    if fadt.len() < RESET_VALUE_OFFSET + 1
        || RESET_REGISTER_OFFSET + GENERIC_ADDRESS_LENGTH > fadt.len()
    {
        return None;
    }
    let address_space = fadt[RESET_REGISTER_OFFSET];
    let bit_width = fadt[RESET_REGISTER_OFFSET + 1];
    let bit_offset = fadt[RESET_REGISTER_OFFSET + 2];
    let access_size = fadt[RESET_REGISTER_OFFSET + 3];
    let address = u64_at(fadt, RESET_REGISTER_OFFSET + 4).ok()?;
    if address == 0
        || bit_width != 8
        || bit_offset != 0
        || !matches!(address_space, GAS_SYSTEM_MEMORY | GAS_SYSTEM_IO)
        || !matches!(access_size, 1 | 2 | 4)
    {
        return None;
    }
    Some(ResetRegister {
        address_space,
        access_size,
        address,
        value: fadt[RESET_VALUE_OFFSET],
    })
}

/// Tulis nilai reset ke register FADT. Tidak ada yang bisa memverifikasi hasilnya
/// selain melihat mesin benar-benar reset.
fn write_reset_register(register: &ResetRegister) {
    match register.address_space {
        GAS_SYSTEM_IO if register.address <= u16::MAX as u64 => {
            let port = register.address as u16;
            unsafe {
                match register.access_size {
                    1 => crate::arch::x86_64::io::outb(port, register.value),
                    2 => crate::arch::x86_64::io::outw(port, register.value as u16),
                    _ => crate::arch::x86_64::io::outl(port, register.value as u32),
                }
            }
        }
        GAS_SYSTEM_MEMORY => {
            let Some(address) = crate::memory::physical_to_virtual(register.address as usize) else {
                return;
            };
            unsafe {
                match register.access_size {
                    1 => (address as *mut u8).write_volatile(register.value),
                    2 => (address as *mut u16).write_volatile(register.value as u16),
                    _ => (address as *mut u32).write_volatile(register.value as u32),
                }
            }
        }
        _ => {}
    }
}

/// Jalan terakhir bila tidak ada register reset: pulsa garis reset keyboard
/// controller 8042, lalu paksa triple fault (IDT kosong + `int3`).
fn fallback_reset() {
    unsafe { crate::arch::x86_64::io::outb(0x64, 0xFE) };
    for _ in 0..MAX_POLL_ATTEMPTS {
        spin_loop();
    }
    unsafe {
        let empty_idt = [0u8; 10];
        asm!("lidt [{}]", in(reg) empty_idt.as_ptr(), options(nostack, readonly));
        asm!("int3", options(nostack));
    }
    loop {
        spin_loop();
    }
}

/// Minta reset sistem lewat FADT `RESET_REG`, dengan fallback 8042/triple fault.
/// Tidak pernah kembali dengan normal; `Err` hanya bila triple fault juga gagal.
pub fn reboot() -> Result<(), AcpiError> {
    if let Ok(config) = power_config()
        && let Some(register) = config.reset
    {
        write_reset_register(&register);
        for _ in 0..MAX_POLL_ATTEMPTS {
            spin_loop();
        }
    }
    fallback_reset();
    Err(AcpiError::RebootFailed)
}

fn power_config() -> Result<&'static PowerConfig, AcpiError> {
    POWER_CONFIG
        .call_once(locate_power_config)
        .as_ref()
        .map_err(|error| *error)
}

fn locate_power_config() -> Result<PowerConfig, AcpiError> {
    let rsdp_address = crate::rsdp::address().ok_or(AcpiError::RsdpUnavailable)?;
    let rsdp_header = physical_slice(rsdp_address, 20).map_err(|_| AcpiError::InvalidRsdp)?;
    if &rsdp_header[..8] != RSDP_SIGNATURE || checksum(rsdp_header) != 0 {
        return Err(AcpiError::InvalidRsdp);
    }

    let revision = rsdp_header[15];
    let rsdt_address = u32_at(rsdp_header, 16)? as u64;
    let root_address = if revision >= 2 {
        let extended = physical_slice(rsdp_address, 36).map_err(|_| AcpiError::InvalidRsdp)?;
        let length = u32_at(extended, 20)? as usize;
        if !(36..=MAX_RSDP_LENGTH).contains(&length) {
            return Err(AcpiError::InvalidRsdp);
        }
        let extended = physical_slice(rsdp_address, length).map_err(|_| AcpiError::InvalidRsdp)?;
        if checksum(extended) != 0 {
            return Err(AcpiError::InvalidRsdp);
        }
        let xsdt_address = u64_at(extended, 24)?;
        if xsdt_address != 0 {
            (xsdt_address, true)
        } else {
            (rsdt_address, false)
        }
    } else {
        (rsdt_address, false)
    };
    if root_address.0 == 0 {
        return Err(AcpiError::InvalidRsdp);
    }

    let root = sdt(root_address.0)?;
    let entry_size = if root_address.1 {
        if &root[..4] != b"XSDT" {
            return Err(AcpiError::InvalidTable);
        }
        8
    } else {
        if &root[..4] != b"RSDT" {
            return Err(AcpiError::InvalidTable);
        }
        4
    };
    let entries = root.len() - SDT_HEADER_LENGTH;
    if entries % entry_size != 0 {
        return Err(AcpiError::InvalidTable);
    }

    let mut fadt = None;
    for offset in (SDT_HEADER_LENGTH..root.len()).step_by(entry_size) {
        let address = if entry_size == 8 {
            u64_at(root, offset)?
        } else {
            u32_at(root, offset)? as u64
        };
        if address == 0 {
            continue;
        }
        let table = sdt(address)?;
        if &table[..4] == b"FACP" {
            fadt = Some(table);
            break;
        }
    }
    let fadt = fadt.ok_or(AcpiError::FadtUnavailable)?;
    if fadt.len() <= PM1_CONTROL_LENGTH_OFFSET {
        return Err(AcpiError::InvalidTable);
    }

    let pm1a = u32_at(fadt, PM1A_CONTROL_OFFSET)?;
    let pm1b = u32_at(fadt, PM1B_CONTROL_OFFSET)?;
    let control_length = fadt[PM1_CONTROL_LENGTH_OFFSET];
    if pm1a == 0 || pm1a > u16::MAX as u32 || control_length < 2 {
        return Err(AcpiError::UnsupportedPowerInterface);
    }
    let pm1b = if pm1b == 0 {
        None
    } else if pm1b <= u16::MAX as u32 {
        Some(pm1b as u16)
    } else {
        return Err(AcpiError::UnsupportedPowerInterface);
    };

    let dsdt_address = u32_at(fadt, DSDT_ADDRESS_OFFSET)? as u64;
    if dsdt_address == 0 {
        return Err(AcpiError::InvalidTable);
    }
    let dsdt = sdt(dsdt_address)?;
    if &dsdt[..4] != b"DSDT" {
        return Err(AcpiError::InvalidTable);
    }
    let (sleep_type_a, sleep_type_b) =
        find_s5(&dsdt[SDT_HEADER_LENGTH..]).ok_or(AcpiError::S5Unavailable)?;

    let smi_port = u32_at(fadt, SMI_CMD_OFFSET)?;
    let acpi_enable = fadt[ACPI_ENABLE_OFFSET];
    let smi_command = if smi_port == 0 || acpi_enable == 0 {
        None
    } else if smi_port <= u16::MAX as u32 {
        Some((smi_port as u16, acpi_enable))
    } else {
        return Err(AcpiError::UnsupportedPowerInterface);
    };

    Ok(PowerConfig {
        pm1a_control: pm1a as u16,
        pm1b_control: pm1b,
        smi_command,
        sleep_type_a,
        sleep_type_b,
        reset: reset_register(fadt),
    })
}

/// Collect the DSDT and every SSDT: the tables the AML reader parses.
///
/// The DSDT comes first because it holds the device tree; the SSDTs are listed
/// in RSDT/XSDT order. A table that fails its checksum is skipped instead of
/// failing the whole walk, so one vendor table cannot hide the rest.
pub(crate) fn aml_tables() -> Result<Vec<&'static [u8]>, AcpiError> {
    let rsdp_address = crate::rsdp::address().ok_or(AcpiError::RsdpUnavailable)?;
    let rsdp_header = physical_slice(rsdp_address, 20).map_err(|_| AcpiError::InvalidRsdp)?;
    if &rsdp_header[..8] != RSDP_SIGNATURE || checksum(rsdp_header) != 0 {
        return Err(AcpiError::InvalidRsdp);
    }

    let revision = rsdp_header[15];
    let rsdt_address = u32_at(rsdp_header, 16)? as u64;
    let (root_address, extended) = if revision >= 2 {
        let extended = physical_slice(rsdp_address, 36).map_err(|_| AcpiError::InvalidRsdp)?;
        let xsdt_address = u64_at(extended, 24)?;
        if xsdt_address != 0 {
            (xsdt_address, true)
        } else {
            (rsdt_address, false)
        }
    } else {
        (rsdt_address, false)
    };
    if root_address == 0 {
        return Err(AcpiError::InvalidTable);
    }

    let root = sdt(root_address)?;
    let entry_size = if extended { 8 } else { 4 };
    let mut dsdt = None;
    let mut ssdts = Vec::new();
    for offset in (SDT_HEADER_LENGTH..root.len()).step_by(entry_size) {
        let address = if extended {
            u64_at(root, offset)?
        } else {
            u32_at(root, offset)? as u64
        };
        if address == 0 {
            continue;
        }
        let Ok(table) = sdt(address) else { continue };
        let signature = &table[..4];
        if signature == b"FACP" {
            let dsdt_address = u32_at(table, DSDT_ADDRESS_OFFSET)? as u64;
            if dsdt_address != 0 {
                if let Ok(candidate) = sdt(dsdt_address) {
                    if &candidate[..4] == b"DSDT" {
                        dsdt = Some(candidate);
                    }
                }
            }
        } else if signature == b"SSDT" {
            ssdts.push(table);
        }
    }

    let dsdt = dsdt.ok_or(AcpiError::InvalidTable)?;
    let mut tables = Vec::with_capacity(ssdts.len() + 1);
    tables.push(dsdt);
    tables.extend(ssdts);
    Ok(tables)
}

fn enable_acpi(config: PowerConfig) -> Result<(), AcpiError> {
    if unsafe { crate::arch::x86_64::io::inw(config.pm1a_control) } & SCI_ENABLE != 0 {
        return Ok(());
    }
    let Some((smi_port, enable_command)) = config.smi_command else {
        return Err(AcpiError::AcpiEnableTimeout);
    };

    unsafe { crate::arch::x86_64::io::outb(smi_port, enable_command) };
    for _ in 0..MAX_POLL_ATTEMPTS {
        if unsafe { crate::arch::x86_64::io::inw(config.pm1a_control) } & SCI_ENABLE != 0 {
            return Ok(());
        }
        spin_loop();
    }
    Err(AcpiError::AcpiEnableTimeout)
}

fn write_sleep_control(port: u16, sleep_type: u8) {
    let current = unsafe { crate::arch::x86_64::io::inw(port) };
    let value = (current & !SLEEP_TYPE_MASK & !SLEEP_ENABLE)
        | ((sleep_type as u16) << SLEEP_TYPE_SHIFT)
        | SLEEP_ENABLE;
    unsafe { crate::arch::x86_64::io::outw(port, value) };
}

fn sdt(address: u64) -> Result<&'static [u8], AcpiError> {
    let header = physical_slice(address, SDT_HEADER_LENGTH).map_err(|_| AcpiError::InvalidTable)?;
    let length = u32_at(header, 4)? as usize;
    if !(SDT_HEADER_LENGTH..=MAX_TABLE_LENGTH).contains(&length) {
        return Err(AcpiError::InvalidTable);
    }
    let table = physical_slice(address, length).map_err(|_| AcpiError::InvalidTable)?;
    if checksum(table) != 0 {
        return Err(AcpiError::InvalidTable);
    }
    Ok(table)
}

fn physical_slice(address: u64, length: usize) -> Result<&'static [u8], AcpiError> {
    let physical = usize::try_from(address).map_err(|_| AcpiError::InvalidTable)?;
    let length_u64 = u64::try_from(length).map_err(|_| AcpiError::InvalidTable)?;
    let physical_end = address
        .checked_add(length_u64)
        .ok_or(AcpiError::InvalidTable)?;
    let page_size = crate::memory::paging::PAGE_SIZE;
    let page_start = address & !(page_size - 1);
    let page_end = physical_end
        .checked_add(page_size - 1)
        .ok_or(AcpiError::InvalidTable)?
        & !(page_size - 1);

    for page in (page_start..page_end).step_by(page_size as usize) {
        let virtual_page = crate::memory::physical_to_virtual(
            usize::try_from(page).map_err(|_| AcpiError::InvalidTable)?,
        )
        .ok_or(AcpiError::MemoryMappingFailed)?;
        match crate::memory::paging::translate_address(virtual_page as u64) {
            Ok(mapped) if mapped == page => {}
            Ok(_) => return Err(AcpiError::MemoryMappingFailed),
            Err(crate::memory::paging::PagingError::NotMapped) => unsafe {
                crate::memory::paging::map_page(virtual_page as u64, page, 0)
                    .map_err(|_| AcpiError::MemoryMappingFailed)?;
            },
            Err(_) => return Err(AcpiError::MemoryMappingFailed),
        }
    }

    let virtual_address =
        crate::memory::physical_to_virtual(physical).ok_or(AcpiError::InvalidTable)?;
    Ok(unsafe { slice::from_raw_parts(virtual_address as *const u8, length) })
}

fn checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte))
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, AcpiError> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or(AcpiError::InvalidTable)?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn u64_at(bytes: &[u8], offset: usize) -> Result<u64, AcpiError> {
    let value = bytes
        .get(offset..offset + 8)
        .ok_or(AcpiError::InvalidTable)?;
    Ok(u64::from_le_bytes([
        value[0], value[1], value[2], value[3], value[4], value[5], value[6], value[7],
    ]))
}

fn find_s5(aml: &[u8]) -> Option<(u8, u8)> {
    for index in 0..aml.len().saturating_sub(4) {
        let package_index = if aml.get(index..index + 5) == Some(b"\x08_S5_") {
            index + 5
        } else if aml.get(index..index + 6) == Some(b"\x08\\_S5_") {
            index + 6
        } else {
            continue;
        };
        if aml.get(package_index) != Some(&0x12) {
            continue;
        }
        if let Some(sleep_types) = decode_s5_package(aml, package_index) {
            return Some(sleep_types);
        }
    }
    None
}

fn decode_s5_package(aml: &[u8], package_index: usize) -> Option<(u8, u8)> {
    let length_index = package_index.checked_add(1)?;
    let (package_length, encoded_length) = decode_package_length(aml, length_index)?;
    let package_end = length_index.checked_add(package_length)?;
    if package_end > aml.len() {
        return None;
    }

    let mut cursor = length_index.checked_add(encoded_length)?;
    let element_count = *aml.get(cursor)?;
    if element_count < 2 || cursor >= package_end {
        return None;
    }
    cursor += 1;
    let package = aml.get(..package_end)?;
    let (sleep_type_a, consumed_a) = decode_integer(package, cursor)?;
    cursor = cursor.checked_add(consumed_a)?;
    let (sleep_type_b, _) = decode_integer(package, cursor)?;
    if sleep_type_a <= 7 && sleep_type_b <= 7 {
        Some((sleep_type_a, sleep_type_b))
    } else {
        None
    }
}

fn decode_package_length(bytes: &[u8], offset: usize) -> Option<(usize, usize)> {
    let lead = *bytes.get(offset)?;
    let following_bytes = (lead >> 6) as usize;
    let mut length = if following_bytes == 0 {
        (lead & 0x3F) as usize
    } else {
        (lead & 0x0F) as usize
    };
    for index in 0..following_bytes {
        let byte = *bytes.get(offset + index + 1)? as usize;
        length |= byte << (4 + index * 8);
    }
    Some((length, following_bytes + 1))
}

fn decode_integer(bytes: &[u8], offset: usize) -> Option<(u8, usize)> {
    match *bytes.get(offset)? {
        0x00 => Some((0, 1)),
        0x01 => Some((1, 1)),
        0x0A => Some((*bytes.get(offset + 1)?, 2)),
        0x0B => {
            let value = u16::from_le_bytes([*bytes.get(offset + 1)?, *bytes.get(offset + 2)?]);
            u8::try_from(value).ok().map(|value| (value, 3))
        }
        0x0C => {
            let value = u32_at(bytes, offset + 1).ok()?;
            u8::try_from(value).ok().map(|value| (value, 5))
        }
        0x0E => {
            let value = u64_at(bytes, offset + 1).ok()?;
            u8::try_from(value).ok().map(|value| (value, 9))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GAS_SYSTEM_IO, RESET_REGISTER_OFFSET, RESET_VALUE_OFFSET, decode_integer,
        decode_package_length, find_s5, reset_register,
    };

    fn fadt_with_reset(address: u64, access_size: u8, bit_width: u8) -> [u8; 140] {
        let mut fadt = [0u8; 140];
        fadt[RESET_REGISTER_OFFSET] = GAS_SYSTEM_IO;
        fadt[RESET_REGISTER_OFFSET + 1] = bit_width;
        fadt[RESET_REGISTER_OFFSET + 3] = access_size;
        fadt[RESET_REGISTER_OFFSET + 4..RESET_REGISTER_OFFSET + 12]
            .copy_from_slice(&address.to_le_bytes());
        fadt[RESET_VALUE_OFFSET] = 0x06;
        fadt
    }

    #[test]
    fn fadt_reset_register_is_parsed_when_well_formed() {
        let fadt = fadt_with_reset(0x0CF9, 1, 8);
        let register = reset_register(&fadt).expect("reset register");
        assert_eq!(register.address, 0x0CF9);
        assert_eq!(register.address_space, GAS_SYSTEM_IO);
        assert_eq!(register.access_size, 1);
        assert_eq!(register.value, 0x06);
    }

    #[test]
    fn fadt_reset_register_is_rejected_when_unusable() {
        let mut no_address = fadt_with_reset(0, 1, 8);
        assert_eq!(reset_register(&no_address), None);
        no_address[RESET_REGISTER_OFFSET + 4] = 0x10;
        assert!(reset_register(&no_address).is_some());
        let wide = fadt_with_reset(0x0CF9, 1, 16);
        assert_eq!(reset_register(&wide), None);
        let bad_size = fadt_with_reset(0x0CF9, 3, 8);
        assert_eq!(reset_register(&bad_size), None);
        assert_eq!(reset_register(&[0u8; 64]), None);
    }

    #[test]
    fn finds_s5_package_with_byte_constants() {
        let aml = [
            0x08, b'_', b'S', b'5', b'_', 0x12, 0x06, 0x04, 0x0A, 0x05, 0x0A, 0x05,
        ];
        assert_eq!(find_s5(&aml), Some((5, 5)));
    }

    #[test]
    fn finds_root_qualified_s5_package() {
        let aml = [
            0x08, b'\\', b'_', b'S', b'5', b'_', 0x12, 0x04, 0x02, 0x00, 0x01,
        ];
        assert_eq!(find_s5(&aml), Some((0, 1)));
    }

    #[test]
    fn decodes_integer_and_package_lengths() {
        assert_eq!(decode_integer(&[0x0B, 0x05, 0x00], 0), Some((5, 3)));
        assert_eq!(decode_package_length(&[0x41, 0x01], 0), Some((17, 2)));
    }

    #[test]
    fn rejects_truncated_s5_package() {
        let aml = [0x08, b'_', b'S', b'5', b'_', 0x12, 0x06, 0x04, 0x0A, 0x05];
        assert_eq!(find_s5(&aml), None);
    }
}
