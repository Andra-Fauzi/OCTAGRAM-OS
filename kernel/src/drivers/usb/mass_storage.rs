use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;
use spin::Mutex;

use crate::drivers::usb::xhci::EndpointHandle;
use crate::drivers::usb::{EndpointInfo, UsbControllerId, UsbControllerKind, ehci, ohci, xhci};
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

#[derive(Clone, Copy)]
enum BulkEndpoint {
    Xhci(EndpointHandle),
    Ehci {
        controller_instance: u8,
        device_address: u8,
        endpoint_address: u8,
        max_packet_size: u16,
        data_toggle: bool,
    },
    Ohci {
        controller_instance: u8,
        device_address: u8,
        endpoint_address: u8,
        max_packet_size: u16,
        data_toggle: bool,
    },
}

impl BulkEndpoint {
    unsafe fn transfer(
        &mut self,
        slot_id: u32,
        buffer_phys: u64,
        length: u32,
    ) -> Result<u32, &'static str> {
        match self {
            Self::Xhci(endpoint) => unsafe {
                xhci::bulk_transfer(slot_id, endpoint, buffer_phys, length)
            },
            Self::Ehci {
                controller_instance,
                device_address,
                endpoint_address,
                max_packet_size,
                data_toggle,
            } => {
                let mut controllers = ehci::EHCI_CONTROLLERS.lock();
                let controller = controllers
                    .get_mut(usize::from(*controller_instance))
                    .ok_or("EHCI controller instance is unavailable")?;
                let transferred = controller.bulk_transfer(
                    *device_address,
                    *endpoint_address & 0x0f,
                    *max_packet_size,
                    *endpoint_address & 0x80 != 0,
                    buffer_phys,
                    length as usize,
                    data_toggle,
                )?;
                u32::try_from(transferred).map_err(|_| "EHCI transfer length overflow")
            }
            Self::Ohci {
                controller_instance,
                device_address,
                endpoint_address,
                max_packet_size,
                data_toggle,
            } => {
                let mut controllers = ohci::OHCI_CONTROLLERS.lock();
                let controller = controllers
                    .get_mut(usize::from(*controller_instance))
                    .ok_or("OHCI controller instance is unavailable")?;
                let transferred = controller.bulk_transfer(
                    *device_address,
                    *endpoint_address,
                    *max_packet_size,
                    buffer_phys,
                    length as usize,
                    data_toggle,
                )?;
                u32::try_from(transferred).map_err(|_| "OHCI transfer length overflow")
            }
        }
    }
}

struct MassStorageState {
    slot_id: u32,
    bulk_in: BulkEndpoint,
    bulk_out: BulkEndpoint,
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
            self.bulk_out
                .transfer(self.slot_id, cbw_phys, BOT_CBW_SIZE as u32)
        }
        .map_err(|_| BlockError::Io)?;
        if sent != BOT_CBW_SIZE as u32 {
            return Err(BlockError::Io);
        }

        if data_length != 0 {
            let data_phys = physical_address(self.data.0.as_ptr() as usize)?;
            let actual = if data_in {
                unsafe {
                    self.bulk_in
                        .transfer(self.slot_id, data_phys, data_length as u32)
                }
            } else {
                unsafe {
                    self.bulk_out
                        .transfer(self.slot_id, data_phys, data_length as u32)
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
            self.bulk_in
                .transfer(self.slot_id, csw_phys, BOT_CSW_SIZE as u32)
        }
        .map_err(|_| BlockError::Io)?;
        if received != BOT_CSW_SIZE as u32 {
            return Err(BlockError::Io);
        }
        let mut csw = [0; BOT_CSW_SIZE];
        unsafe {
            copy_dma_to_slice(self.csw.0.as_ptr(), &mut csw);
        }
        if u32::from_le_bytes(csw[0..4].try_into().unwrap()) != BOT_CSW_SIGNATURE
            || u32::from_le_bytes(csw[4..8].try_into().unwrap()) != self.tag
            || csw[12] != 0
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
                unsafe {
                    copy_dma_to_slice(self.data.0.as_ptr(), chunk);
                }
            }
        }
        Ok(())
    }
}

struct UsbMassStorageDevice {
    name: String,
    slot_id: u32,
    controller: UsbControllerId,
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


static ACTIVE_DEVICES: Mutex<Vec<Arc<UsbMassStorageDevice>>> = Mutex::new(Vec::new());

pub unsafe fn init_storage(
    slot_id: u32,
    bulk_in: EndpointHandle,
    bulk_out: EndpointHandle,
) -> Result<BlockDeviceRef, BlockError> {
    unsafe {
        init_storage_with_endpoints(
            UsbControllerId::XHCI,
            slot_id,
            BulkEndpoint::Xhci(bulk_in),
            BulkEndpoint::Xhci(bulk_out),
        )
    }
}

pub unsafe fn init_ehci_storage(
    controller: UsbControllerId,
    device_address: u8,
    bulk_in: EndpointInfo,
    bulk_out: EndpointInfo,
) -> Result<BlockDeviceRef, BlockError> {
    let UsbControllerKind::Ehci = controller.kind else {
        return Err(BlockError::Unsupported);
    };
    if bulk_in.address & 0x80 == 0 || bulk_out.address & 0x80 != 0 {
        return Err(BlockError::InvalidBuffer);
    }
    let make_endpoint = |endpoint: EndpointInfo| BulkEndpoint::Ehci {
        controller_instance: controller.instance,
        device_address,
        endpoint_address: endpoint.address,
        max_packet_size: endpoint.max_packet_size & 0x07ff,
        data_toggle: false,
    };
    unsafe {
        init_storage_with_endpoints(
            controller,
            u32::from(device_address),
            make_endpoint(bulk_in),
            make_endpoint(bulk_out),
        )
    }
}

pub unsafe fn init_ohci_storage(
    controller: UsbControllerId,
    device_address: u8,
    bulk_in: EndpointInfo,
    bulk_out: EndpointInfo,
) -> Result<BlockDeviceRef, BlockError> {
    let UsbControllerKind::Ohci = controller.kind else {
        return Err(BlockError::Unsupported);
    };
    if bulk_in.address & 0x80 == 0 || bulk_out.address & 0x80 != 0 {
        return Err(BlockError::InvalidBuffer);
    }
    let make_endpoint = |endpoint: EndpointInfo| BulkEndpoint::Ohci {
        controller_instance: controller.instance,
        device_address,
        endpoint_address: endpoint.address,
        max_packet_size: endpoint.max_packet_size & 0x07ff,
        data_toggle: false,
    };
    unsafe {
        init_storage_with_endpoints(
            controller,
            u32::from(device_address),
            make_endpoint(bulk_in),
            make_endpoint(bulk_out),
        )
    }
}

unsafe fn init_storage_with_endpoints(
    controller: UsbControllerId,
    slot_id: u32,
    bulk_in: BulkEndpoint,
    bulk_out: BulkEndpoint,
) -> Result<BlockDeviceRef, BlockError> {
    // Reuse the lowest free name so an unplug/replug cycle keeps the same
    // device name (`usb0`) instead of climbing to usb1, usb2, ... forever.
    let name = {
        let devices = ACTIVE_DEVICES.lock();
        next_free_device_name(&devices)
    };
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

    // Test Unit Ready untuk membersihkan Unit Attention / menunggu flash disk siap
    let tur_cdb = [0u8; 6];
    for _ in 0..5 {
        if unsafe { state.scsi_command(&tur_cdb, 0, false) }.is_ok() {
            break;
        }
        crate::arch::x86_64::delay_milliseconds(20);
    }

    state.data.0[..36].fill(0);
    let mut inquiry_cdb = [0u8; 6];
    inquiry_cdb[0] = 0x12;
    inquiry_cdb[4] = 36;
    let mut inquiry_ok = false;
    for _ in 0..3 {
        if unsafe { state.scsi_command(&inquiry_cdb, 36, true) }.is_ok() {
            inquiry_ok = true;
            break;
        }
        crate::arch::x86_64::delay_milliseconds(20);
    }
    if !inquiry_ok {
        return Err(BlockError::Io);
    }

    state.data.0[..8].fill(0);
    let mut capacity_cdb = [0u8; 10];
    capacity_cdb[0] = 0x25;
    let mut capacity_ok = false;
    for _ in 0..5 {
        if unsafe { state.scsi_command(&capacity_cdb, 8, true) }.is_ok() {
            capacity_ok = true;
            break;
        }
        crate::arch::x86_64::delay_milliseconds(50);
    }
    if !capacity_ok {
        return Err(BlockError::Io);
    }
    let mut capacity = [0; 8];
    copy_dma_to_slice(state.data.0.as_ptr(), &mut capacity);
    let last_lba = u32::from_be_bytes(capacity[0..4].try_into().unwrap());
    state.block_size = u32::from_be_bytes(capacity[4..8].try_into().unwrap());
    state.sector_count = last_lba as u64 + 1;
    if state.block_size != SECTOR_SIZE as u32 || state.sector_count == 0 {
        return Err(BlockError::Unsupported);
    }

    let device = Arc::new(UsbMassStorageDevice {
        name: name.clone(),
        slot_id,
        controller,
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

fn next_free_device_name(devices: &[Arc<UsbMassStorageDevice>]) -> String {
    let mut index = 0usize;
    loop {
        let candidate = format!("usb{index}");
        if devices.iter().all(|device| device.name != candidate) {
            return candidate;
        }
        index += 1;
    }
}

pub fn shutdown(controller: UsbControllerId, slot_id: u32) {
    let mut devices = ACTIVE_DEVICES.lock();
    let mut index = 0;
    while index < devices.len() {
        if devices[index].slot_id != slot_id || devices[index].controller != controller {
            index += 1;
            continue;
        }
        let device = devices.remove(index);
        device.online.store(false, Ordering::Release);
        let _ = block::unregister(&device.name);
    }
}

fn copy_dma_to_slice(source: *const u8, destination: &mut [u8]) {
    core::sync::atomic::compiler_fence(Ordering::Acquire);
    for (index, byte) in destination.iter_mut().enumerate() {
        *byte = unsafe { source.add(index).read_volatile() };
    }
}

fn physical_address(virtual_address: usize) -> Result<u64, BlockError> {
    memory::virtual_to_physical(virtual_address)
        .map(|address| address as u64)
        .ok_or(BlockError::Io)
}
