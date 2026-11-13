use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

pub const SECTOR_SIZE: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockError {
    InvalidBuffer,
    OutOfRange,
    NotReady,
    Unsupported,
    AlreadyExists,
    Io,
    ReadOnly,
}

pub trait BlockDevice: Send + Sync {
    fn name(&self) -> &str;
    fn sector_count(&self) -> u64;
    fn read_sectors(&self, lba: u64, buffer: &mut [u8]) -> Result<(), BlockError>;
    fn write_sectors(&self, lba: u64, buffer: &[u8]) -> Result<(), BlockError>;
}

pub type BlockDeviceRef = Arc<dyn BlockDevice>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockDeviceInfo {
    pub name: String,
    pub sectors: u64,
}

static DEVICES: Mutex<Vec<BlockDeviceRef>> = Mutex::new(Vec::new());

pub fn register(device: BlockDeviceRef) -> Result<(), BlockError> {
    let mut devices = DEVICES.lock();
    if devices
        .iter()
        .any(|current| current.name() == device.name())
    {
        return Err(BlockError::AlreadyExists);
    }
    devices.push(device);
    Ok(())
}

pub fn unregister(name: &str) -> Option<BlockDeviceRef> {
    let mut devices = DEVICES.lock();
    let index = devices.iter().position(|device| device.name() == name)?;
    Some(devices.remove(index))
}

pub fn get(name: &str) -> Option<BlockDeviceRef> {
    DEVICES
        .lock()
        .iter()
        .find(|device| device.name() == name)
        .cloned()
}

pub fn list() -> Vec<BlockDeviceInfo> {
    DEVICES
        .lock()
        .iter()
        .map(|device| BlockDeviceInfo {
            name: device.name().to_string(),
            sectors: device.sector_count(),
        })
        .collect()
}
