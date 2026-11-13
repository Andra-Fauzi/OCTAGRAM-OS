use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;

use crate::drivers::usb::xhci::{self, EndpointHandle};
use crate::memory;
use crate::storage::block::{self, BlockDevice, BlockDeviceRef, BlockError, SECTOR_SIZE};

const BOT_CBW_SIGNATURE: u32 = 0x4342_5355;
const BOT_CSW_SIGNATURE: u32 = 0x5342_5355;
const BOT_CBW_SIZE: usize = 31;
const BOT_CSW_SIZE: usize = 13;
const MAX_TRANSFER_SIZE: usize = 4096;
const MAX_BLOCKS_PER_TRANSFER: usize = MAX_TRANSFER_SIZE / SECTOR_SIZE;

#[repr(C, align(4096))]
struct DmaBuffer([u8; MAX_TRANSFER_SIZE]);

struct MassStorageState {
    slot_id: u32,
    bulk_in: EndpointHandle,
    bulk_out: EndpointHandle,
    tag: u32,
    sector_count: u64,
    block_size: u32,
    cbw: Box<DmaBuffer>,
    csw: Box<DmaBuffer>,
    data: Box<DmaBuffer>,
}

impl MassStorageState {
    unsafe fn scsi_command(
        &mut self,
        cdb: &[u8],
        data_length: usize,
        data_in: bool,
    ) -> Result<(), BlockError> {
        if cdb.is_empty() || cdb.len() > 16 || data_length > MAX_TRANSFER_SIZE {
            return Err(BlockError::InvalidBuffer);
        }

        self.tag = self.tag.wrapping_add(1).max(1);
        self.cbw.0[..BOT_CBW_SIZE].fill(0);
        self.cbw.0[0..4].copy_from_slice(&BOT_CBW_SIGNATURE.to_le_bytes());
        self.cbw.0[4..8].copy_from_slice(&self.tag.to_le_bytes());
        self.cbw.0[8..12].copy_from_slice(&(data_length as u32).to_le_bytes());
        self.cbw.0[12] = if data_in { 0x80 } else { 0 };
        self.cbw.0[13] = 0;
        self.cbw.0[14] = cdb.len() as u8;
        self.cbw.0[15..15 + cdb.len()].copy_from_slice(cdb);

        let cbw_phys = physical_address(self.cbw.0.as_ptr() as usize)?;
        let sent = unsafe {
            xhci::bulk_transfer(
                self.slot_id,
                &mut self.bulk_out,
                cbw_phys,
                BOT_CBW_SIZE as u32,
            )
        }
        .map_err(|_| BlockError::Io)?;
        if sent != BOT_CBW_SIZE as u32 {
            return Err(BlockError::Io);
        }

        if data_length != 0 {
            let data_phys = physical_address(self.data.0.as_ptr() as usize)?;
            let actual = if data_in {
                unsafe {
                    xhci::bulk_transfer(
                        self.slot_id,
                        &mut self.bulk_in,
                        data_phys,
                        data_length as u32,
                    )
                }
            } else {
                unsafe {
                    xhci::bulk_transfer(
                        self.slot_id,
                        &mut self.bulk_out,
                        data_phys,
                        data_length as u32,
                    )
                }
            }
            .map_err(|_| BlockError::Io)?;
            if actual != data_length as u32 {
                return Err(BlockError::Io);
            }
        }

        self.csw.0[..BOT_CSW_SIZE].fill(0);
        let csw_phys = physical_address(self.csw.0.as_ptr() as usize)?;
        let received = unsafe {
            xhci::bulk_transfer(
                self.slot_id,
                &mut self.bulk_in,
                csw_phys,
                BOT_CSW_SIZE as u32,
            )
        }
        .map_err(|_| BlockError::Io)?;
        if received != BOT_CSW_SIZE as u32
            || u32::from_le_bytes(self.csw.0[0..4].try_into().unwrap()) != BOT_CSW_SIGNATURE
            || u32::from_le_bytes(self.csw.0[4..8].try_into().unwrap()) != self.tag
            || self.csw.0[12] != 0
        {
            return Err(BlockError::Io);
        }
        Ok(())
    }

    fn transfer_blocks(
        &mut self,
        lba: u64,
        buffer: &mut [u8],
        write: bool,
    ) -> Result<(), BlockError> {
        let end = lba
            .checked_add((buffer.len() / SECTOR_SIZE) as u64)
            .ok_or(BlockError::OutOfRange)?;
        if buffer.len() % SECTOR_SIZE != 0 || end > self.sector_count {
            return Err(BlockError::OutOfRange);
        }
        if self.block_size as usize != SECTOR_SIZE {
            return Err(BlockError::Unsupported);
        }

        for (index, chunk) in buffer.chunks_mut(MAX_TRANSFER_SIZE).enumerate() {
            let block_count = chunk.len() / SECTOR_SIZE;
            let current_lba = lba + (index * MAX_BLOCKS_PER_TRANSFER) as u64;
            if current_lba > u32::MAX as u64 || block_count > u16::MAX as usize {
                return Err(BlockError::OutOfRange);
            }
            let mut cdb = [0u8; 10];
            cdb[0] = if write { 0x2A } else { 0x28 };
            cdb[2..6].copy_from_slice(&(current_lba as u32).to_be_bytes());
            cdb[7..9].copy_from_slice(&(block_count as u16).to_be_bytes());
            if write {
                self.data.0[..chunk.len()].copy_from_slice(chunk);
            }
            unsafe { self.scsi_command(&cdb, chunk.len(), !write) }?;
            if !write {
                chunk.copy_from_slice(&self.data.0[..chunk.len()]);
            }
        }
        Ok(())
    }
}

struct UsbMassStorageDevice {
    name: String,
    slot_id: u32,
    online: core::sync::atomic::AtomicBool,
    state: Mutex<MassStorageState>,
}

impl BlockDevice for UsbMassStorageDevice {
    fn name(&self) -> &str {
        &self.name
    }

    fn sector_count(&self) -> u64 {
        self.state.lock().sector_count
    }

    fn read_sectors(&self, lba: u64, buffer: &mut [u8]) -> Result<(), BlockError> {
        if !self.online.load(Ordering::Acquire) {
            return Err(BlockError::NotReady);
        }
        if buffer.len() % SECTOR_SIZE != 0 {
            return Err(BlockError::InvalidBuffer);
        }
        self.state.lock().transfer_blocks(lba, buffer, false)
    }

    fn write_sectors(&self, lba: u64, buffer: &[u8]) -> Result<(), BlockError> {
        if !self.online.load(Ordering::Acquire) {
            return Err(BlockError::NotReady);
        }
        if buffer.len() % SECTOR_SIZE != 0 {
            return Err(BlockError::InvalidBuffer);
        }
        let mut state = self.state.lock();
        let mut mutable = alloc::vec::Vec::new();
        mutable
            .try_reserve_exact(buffer.len())
            .map_err(|_| BlockError::Io)?;
        mutable.extend_from_slice(buffer);
        state.transfer_blocks(lba, &mut mutable, true)
    }
}

static NEXT_DEVICE_ID: AtomicUsize = AtomicUsize::new(0);
static ACTIVE_DEVICES: Mutex<Vec<Arc<UsbMassStorageDevice>>> = Mutex::new(Vec::new());

pub unsafe fn init_storage(
    slot_id: u32,
    bulk_in: EndpointHandle,
    bulk_out: EndpointHandle,
) -> Result<BlockDeviceRef, BlockError> {
    let name = format!("usb{}", NEXT_DEVICE_ID.fetch_add(1, Ordering::Relaxed));
    let mut state = MassStorageState {
        slot_id,
        bulk_in,
        bulk_out,
        tag: 0,
        sector_count: 0,
        block_size: 0,
        cbw: Box::new(DmaBuffer([0; MAX_TRANSFER_SIZE])),
        csw: Box::new(DmaBuffer([0; MAX_TRANSFER_SIZE])),
        data: Box::new(DmaBuffer([0; MAX_TRANSFER_SIZE])),
    };

    state.data.0[..36].fill(0);
    let mut inquiry_cdb = [0u8; 6];
    inquiry_cdb[0] = 0x12;
    inquiry_cdb[4] = 36;
    unsafe { state.scsi_command(&inquiry_cdb, 36, true) }?;

    state.data.0[..8].fill(0);
    let mut capacity_cdb = [0u8; 10];
    capacity_cdb[0] = 0x25;
    unsafe { state.scsi_command(&capacity_cdb, 8, true) }?;
    let last_lba = u32::from_be_bytes(state.data.0[0..4].try_into().unwrap());
    state.block_size = u32::from_be_bytes(state.data.0[4..8].try_into().unwrap());
    state.sector_count = last_lba as u64 + 1;
    if state.block_size != SECTOR_SIZE as u32 || state.sector_count == 0 {
        return Err(BlockError::Unsupported);
    }

    let device = Arc::new(UsbMassStorageDevice {
        name: name.clone(),
        slot_id,
        online: core::sync::atomic::AtomicBool::new(true),
        state: Mutex::new(state),
    });
    let block_device: BlockDeviceRef = device.clone();
    block::register(block_device.clone())?;
    ACTIVE_DEVICES.lock().push(device);
    crate::println!(
        "USB storage {} siap: {} sektor",
        block_device.name(),
        block_device.sector_count()
    );
    Ok(block_device)
}

pub fn shutdown(slot_id: u32) {
    let mut devices = ACTIVE_DEVICES.lock();
    let mut index = 0;
    while index < devices.len() {
        if devices[index].slot_id != slot_id {
            index += 1;
            continue;
        }
        let device = devices.remove(index);
        device.online.store(false, Ordering::Release);
        let _ = block::unregister(&device.name);
    }
}

fn physical_address(virtual_address: usize) -> Result<u64, BlockError> {
    memory::virtual_to_physical(virtual_address)
        .map(|address| address as u64)
        .ok_or(BlockError::Io)
}
