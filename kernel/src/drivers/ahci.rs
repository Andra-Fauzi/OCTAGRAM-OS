use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use core::ptr;
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;

use crate::drivers::device_model::{DeviceDriver, DeviceInfo, DriverError, DriverState};
use crate::drivers::pci::{self, PciDevice};
use crate::memory;
use crate::storage::block::{self, BlockDevice, BlockDeviceRef, BlockError, SECTOR_SIZE};

const PCI_CLASS_MASS_STORAGE: u8 = 0x01;
const PCI_SUBCLASS_SATA: u8 = 0x06;
const PCI_PROG_IF_AHCI: u8 = 0x01;
static NEXT_DEVICE_ID: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct AhciDriver;
pub(crate) static AHCI_DRIVER: AhciDriver = AhciDriver;

impl DeviceDriver for AhciDriver {
    fn name(&self) -> &'static str {
        "ahci"
    }

    fn matches(&self, device: &DeviceInfo) -> bool {
        matches!(device, DeviceInfo::Pci(device)
            if device.matches_class(PCI_CLASS_MASS_STORAGE, PCI_SUBCLASS_SATA, PCI_PROG_IF_AHCI))
    }

    unsafe fn probe(&self, device: &DeviceInfo) -> Result<DriverState, DriverError> {
        let DeviceInfo::Pci(controller) = device else {
            return Err(DriverError::Unsupported);
        };
        unsafe { init_controller(controller) }
            .map_err(|_| DriverError::ProbeFailed("AHCI controller initialization failed"))?;
        Ok(Arc::new(()))
    }

    unsafe fn remove(&self, _device: &DeviceInfo, _state: DriverState) {}
}

const HBA_SIZE: u64 = 0x2000;
const HBA_GHC: usize = 0x04;
const HBA_PI: usize = 0x0C;
const HBA_PORTS: usize = 0x100;
const HBA_PORT_SIZE: usize = 0x80;
const PORT_CLB: usize = 0x00;
const PORT_FB: usize = 0x08;
const PORT_IS: usize = 0x10;
const PORT_IE: usize = 0x14;
const PORT_CMD: usize = 0x18;
const PORT_TFD: usize = 0x20;
const PORT_SIG: usize = 0x24;
const PORT_SSTS: usize = 0x28;
const PORT_SERR: usize = 0x30;
const PORT_CI: usize = 0x38;

const GHC_AE: u32 = 1 << 31;
const PORT_CMD_ST: u32 = 1;
const PORT_CMD_FRE: u32 = 1 << 4;
const PORT_CMD_FR: u32 = 1 << 14;
const PORT_CMD_CR: u32 = 1 << 15;
const PORT_TFD_BSY: u32 = 1 << 7;
const PORT_TFD_DRQ: u32 = 1 << 3;
const PORT_TFD_ERR: u32 = 1;
const PORT_IS_TFES: u32 = 1 << 30;
const SATA_SIG_ATA: u32 = 0x0000_0101;
const ATA_IDENTIFY: u8 = 0xEC;
const ATA_READ_DMA_EXT: u8 = 0x25;
const ATA_WRITE_DMA_EXT: u8 = 0x35;
const ATA_TIMEOUT: usize = 100_000_000;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CommandHeader {
    flags: u16,
    prdt_length: u16,
    transferred_bytes: u32,
    command_table_low: u32,
    command_table_high: u32,
    reserved: [u32; 4],
}

#[repr(C, align(1024))]
struct CommandList([CommandHeader; 32]);

impl CommandList {
    fn new() -> Self {
        Self([CommandHeader::default(); 32])
    }
}

#[repr(C, align(256))]
struct ReceivedFis([u8; 256]);

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct PrdtEntry {
    data_base: u64,
    reserved: u32,
    byte_count_and_interrupt: u32,
}

#[repr(C, align(128))]
struct CommandTable {
    command_fis: [u8; 64],
    atapi_command: [u8; 16],
    reserved: [u8; 48],
    prdt: [PrdtEntry; 1],
}

impl CommandTable {
    fn new() -> Self {
        Self {
            command_fis: [0; 64],
            atapi_command: [0; 16],
            reserved: [0; 48],
            prdt: [PrdtEntry::default(); 1],
        }
    }
}

#[repr(C, align(4096))]
struct DmaBuffer([u8; SECTOR_SIZE]);

struct PortState {
    port_regs: usize,
    command_list: Box<CommandList>,
    received_fis: Box<ReceivedFis>,
    command_table: Box<CommandTable>,
    buffer: Box<DmaBuffer>,
    sector_count: u64,
}

impl PortState {
    unsafe fn initialize(port_regs: usize) -> Result<Self, BlockError> {
        unsafe {
            let mut command = read_reg(port_regs, PORT_CMD);
            command &= !PORT_CMD_ST;
            write_reg(port_regs, PORT_CMD, command);
            wait_clear(port_regs, PORT_CMD, PORT_CMD_CR)?;
            command &= !PORT_CMD_FRE;
            write_reg(port_regs, PORT_CMD, command);
            wait_clear(port_regs, PORT_CMD, PORT_CMD_FR)?;

            let mut state = Self {
                port_regs,
                command_list: Box::new(CommandList::new()),
                received_fis: Box::new(ReceivedFis([0; 256])),
                command_table: Box::new(CommandTable::new()),
                buffer: Box::new(DmaBuffer([0; SECTOR_SIZE])),
                sector_count: 0,
            };
            let command_list_phys = physical_address(&*state.command_list as *const _ as usize)?;
            let received_fis_phys = physical_address(&*state.received_fis as *const _ as usize)?;
            write_reg(port_regs, PORT_CLB, command_list_phys as u32);
            write_reg(port_regs, PORT_CLB + 4, (command_list_phys >> 32) as u32);
            write_reg(port_regs, PORT_FB, received_fis_phys as u32);
            write_reg(port_regs, PORT_FB + 4, (received_fis_phys >> 32) as u32);
            write_reg(port_regs, PORT_IE, 0);
            write_reg(port_regs, PORT_IS, u32::MAX);
            write_reg(port_regs, PORT_SERR, u32::MAX);
            write_reg(
                port_regs,
                PORT_CMD,
                read_reg(port_regs, PORT_CMD) | PORT_CMD_FRE | PORT_CMD_ST,
            );

            state.issue_command(ATA_IDENTIFY, 0, false)?;
            let identify = &state.buffer.0;
            if u16::from_le_bytes([identify[166], identify[167]]) & (1 << 10) == 0 {
                return Err(BlockError::Io);
            }
            state.sector_count = u64::from_le_bytes(identify[200..208].try_into().unwrap());
            if state.sector_count == 0 {
                return Err(BlockError::Io);
            }
            Ok(state)
        }
    }

    unsafe fn issue_command(
        &mut self,
        command: u8,
        lba: u64,
        write: bool,
    ) -> Result<(), BlockError> {
        unsafe {
            wait_clear(self.port_regs, PORT_TFD, PORT_TFD_BSY | PORT_TFD_DRQ)?;
            let table_phys = physical_address(&*self.command_table as *const _ as usize)?;
            let buffer_phys = physical_address(self.buffer.0.as_ptr() as usize)?;

            self.command_table.command_fis.fill(0);
            self.command_table.command_fis[0] = 0x27;
            self.command_table.command_fis[1] = 1 << 7;
            self.command_table.command_fis[2] = command;
            self.command_table.command_fis[4] = lba as u8;
            self.command_table.command_fis[5] = (lba >> 8) as u8;
            self.command_table.command_fis[6] = (lba >> 16) as u8;
            self.command_table.command_fis[7] = 1 << 6;
            self.command_table.command_fis[8] = (lba >> 24) as u8;
            self.command_table.command_fis[9] = (lba >> 32) as u8;
            self.command_table.command_fis[10] = (lba >> 40) as u8;
            if command != ATA_IDENTIFY {
                self.command_table.command_fis[12] = 1;
            }
            self.command_table.prdt[0] = PrdtEntry {
                data_base: buffer_phys as u64,
                reserved: 0,
                byte_count_and_interrupt: (SECTOR_SIZE as u32 - 1) | (1 << 31),
            };

            let header = &mut self.command_list.0[0];
            header.flags = 5 | if write { 1 << 6 } else { 0 };
            header.prdt_length = 1;
            header.transferred_bytes = 0;
            header.command_table_low = table_phys as u32;
            header.command_table_high = (table_phys >> 32) as u32;

            write_reg(self.port_regs, PORT_IS, u32::MAX);
            write_reg(self.port_regs, PORT_CI, 1);
            let mut timeout = ATA_TIMEOUT;
            while read_reg(self.port_regs, PORT_CI) & 1 != 0 {
                if read_reg(self.port_regs, PORT_TFD) & PORT_TFD_ERR != 0
                    || read_reg(self.port_regs, PORT_IS) & PORT_IS_TFES != 0
                {
                    return Err(BlockError::Io);
                }
                timeout -= 1;
                if timeout == 0 {
                    return Err(BlockError::NotReady);
                }
                core::hint::spin_loop();
            }
            if read_reg(self.port_regs, PORT_TFD) & PORT_TFD_ERR != 0
                || read_reg(self.port_regs, PORT_IS) & PORT_IS_TFES != 0
            {
                return Err(BlockError::Io);
            }
            write_reg(self.port_regs, PORT_IS, u32::MAX);
            write_reg(self.port_regs, PORT_SERR, u32::MAX);
            Ok(())
        }
    }
}

struct AhciDevice {
    name: String,
    state: Mutex<PortState>,
}

impl BlockDevice for AhciDevice {
    fn name(&self) -> &str {
        &self.name
    }

    fn sector_count(&self) -> u64 {
        self.state.lock().sector_count
    }

    fn read_sectors(&self, lba: u64, buffer: &mut [u8]) -> Result<(), BlockError> {
        validate_range(lba, buffer.len(), self.sector_count())?;
        let mut state = self.state.lock();
        for (index, output) in buffer.chunks_exact_mut(SECTOR_SIZE).enumerate() {
            unsafe { state.issue_command(ATA_READ_DMA_EXT, lba + index as u64, false) }?;
            output.copy_from_slice(&state.buffer.0);
        }
        Ok(())
    }

    fn write_sectors(&self, lba: u64, buffer: &[u8]) -> Result<(), BlockError> {
        validate_range(lba, buffer.len(), self.sector_count())?;
        let mut state = self.state.lock();
        for (index, input) in buffer.chunks_exact(SECTOR_SIZE).enumerate() {
            state.buffer.0.copy_from_slice(input);
            unsafe { state.issue_command(ATA_WRITE_DMA_EXT, lba + index as u64, true) }?;
        }
        Ok(())
    }
}

unsafe fn init_controller(controller: &PciDevice) -> Result<(), BlockError> {
    unsafe {
        pci::enable_device(controller);
        let bar = pci::get_bar(controller, 5);
        if bar.address == 0 {
            return Err(BlockError::NotReady);
        }
        let base = memory::paging::map_mmio_range(bar.address, HBA_SIZE) as usize;
        write_reg(base, HBA_GHC, read_reg(base, HBA_GHC) | GHC_AE);

        let implemented_ports = read_reg(base, HBA_PI);
        crate::println!(
            "AHCI controller {:02x}:{:02x}.{} PI={:#010x}",
            controller.bus,
            controller.device,
            controller.function,
            implemented_ports
        );
        let mut found_disk = false;
        for port_index in 0..32 {
            if implemented_ports & (1 << port_index) == 0 {
                continue;
            }
            let port_regs = base + HBA_PORTS + port_index * HBA_PORT_SIZE;
            let ssts = read_reg(port_regs, PORT_SSTS);
            let signature = read_reg(port_regs, PORT_SIG);
            crate::println!(
                "  AHCI port {} SSTS={:#010x} SIG={:#010x}",
                port_index,
                ssts,
                signature
            );
            if ssts & 0x0F != 3 || (ssts >> 8) & 0x0F != 1 {
                continue;
            }
            if signature != SATA_SIG_ATA {
                continue;
            }

            match PortState::initialize(port_regs) {
                Ok(state) => {
                    let name = format!("sata{}", NEXT_DEVICE_ID.fetch_add(1, Ordering::Relaxed));
                    found_disk = true;
                    let device: BlockDeviceRef = Arc::new(AhciDevice {
                        name: name.clone(),
                        state: Mutex::new(state),
                    });
                    match block::register(device) {
                        Ok(()) => crate::println!(
                            "AHCI {} terdeteksi: {} sektor",
                            name,
                            block::get(&name).unwrap().sector_count()
                        ),
                        Err(error) => {
                            crate::println!("AHCI {} gagal didaftarkan: {:?}", name, error)
                        }
                    }
                }
                Err(error) => crate::println!("AHCI port {} tidak siap: {:?}", port_index, error),
            }
        }
        if !found_disk {
            crate::println!(
                "AHCI {:02x}:{:02x}.{} tidak menemukan disk SATA ATA yang siap",
                controller.bus,
                controller.device,
                controller.function
            );
        }
    }
    Ok(())
}

fn validate_range(lba: u64, length: usize, total_sectors: u64) -> Result<(), BlockError> {
    if length % SECTOR_SIZE != 0 {
        return Err(BlockError::InvalidBuffer);
    }
    let sectors = (length / SECTOR_SIZE) as u64;
    if lba
        .checked_add(sectors)
        .is_none_or(|end| end > total_sectors)
    {
        return Err(BlockError::OutOfRange);
    }
    Ok(())
}

fn physical_address(virtual_address: usize) -> Result<u64, BlockError> {
    memory::virtual_to_physical(virtual_address)
        .map(|address| address as u64)
        .ok_or(BlockError::Io)
}

unsafe fn wait_clear(base: usize, offset: usize, mask: u32) -> Result<(), BlockError> {
    let mut timeout = ATA_TIMEOUT;
    while unsafe { read_reg(base, offset) } & mask != 0 {
        timeout -= 1;
        if timeout == 0 {
            return Err(BlockError::NotReady);
        }
        core::hint::spin_loop();
    }
    Ok(())
}

unsafe fn read_reg(base: usize, offset: usize) -> u32 {
    unsafe { ptr::read_volatile((base + offset) as *const u32) }
}

unsafe fn write_reg(base: usize, offset: usize, value: u32) {
    unsafe { ptr::write_volatile((base + offset) as *mut u32, value) };
}

#[cfg(test)]
mod tests {
    use super::{AHCI_DRIVER, PCI_CLASS_MASS_STORAGE, PCI_PROG_IF_AHCI, PCI_SUBCLASS_SATA};
    use crate::drivers::device_model::{DeviceDriver, DeviceInfo};
    use crate::drivers::pci::PciDevice;

    fn device(class: u8, subclass: u8, prog_if: u8) -> PciDevice {
        PciDevice {
            bus: 0,
            device: 1,
            function: 0,
            vendor_id: 0x1234,
            device_id: 0x5678,
            class,
            subclass,
            prog_if,
            header_type: 0,
        }
    }

    #[test]
    fn matches_only_ahci_controllers() {
        let ahci = [
            device(PCI_CLASS_MASS_STORAGE, PCI_SUBCLASS_SATA, PCI_PROG_IF_AHCI),
            device(PCI_CLASS_MASS_STORAGE, PCI_SUBCLASS_SATA, 0x02),
            device(PCI_CLASS_MASS_STORAGE, 0x04, PCI_PROG_IF_AHCI),
        ];

        assert!(AHCI_DRIVER.matches(&DeviceInfo::Pci(ahci[0])));
        assert!(!AHCI_DRIVER.matches(&DeviceInfo::Pci(ahci[1])));
        assert!(!AHCI_DRIVER.matches(&DeviceInfo::Pci(ahci[2])));
    }
}
