//! Minimal GPT partition-table support and bounded block-device slices.
//!
//! The installer uses GPT with a 512 MiB EFI System Partition and an EXT2/FAT32
//! Linux filesystem partition. All I/O is bounded to the parent block device.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::sync::Arc;

use super::block::{BlockDevice, BlockDeviceRef, BlockError, SECTOR_SIZE};

const GPT_HEADER_SIGNATURE: &[u8; 8] = b"EFI PART";
const GPT_HEADER_SIZE: usize = 92;
const GPT_ENTRY_SIZE: usize = 128;
const GPT_ENTRY_COUNT: usize = 128;
const GPT_ENTRY_SECTORS: u64 =
    (GPT_ENTRY_SIZE as u64 * GPT_ENTRY_COUNT as u64).div_ceil(SECTOR_SIZE as u64);

pub const EFI_SYSTEM_PARTITION_GUID: [u8; 16] = [
    0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11,
    0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B,
];

/// Standard GPT Linux filesystem data type GUID (on-disk byte order).
pub const LINUX_FILESYSTEM_GUID: [u8; 16] = [
    0xAF, 0x3D, 0xC6, 0x0F, 0x83, 0x84, 0x72, 0x47,
    0x8E, 0x79, 0x3D, 0x69, 0xD8, 0x47, 0x7D, 0xE4,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GptPartition {
    pub index: u32,
    pub type_guid: [u8; 16],
    pub unique_guid: [u8; 16],
    pub first_lba: u64,
    pub last_lba: u64,
}

impl GptPartition {
    pub fn sector_count(&self) -> u64 {
        self.last_lba - self.first_lba + 1
    }
}

/// A block device view limited to one GPT partition.
pub struct PartitionDevice {
    parent: BlockDeviceRef,
    name: String,
    start_lba: u64,
    sectors: u64,
}

impl PartitionDevice {
    pub fn new(
        parent: BlockDeviceRef,
        name: String,
        start_lba: u64,
        sectors: u64,
    ) -> Result<Self, BlockError> {
        if sectors == 0
            || start_lba
                .checked_add(sectors)
                .is_none_or(|end| end > parent.sector_count())
        {
            return Err(BlockError::OutOfRange);
        }
        Ok(Self {
            parent,
            name,
            start_lba,
            sectors,
        })
    }
}

impl BlockDevice for PartitionDevice {
    fn name(&self) -> &str {
        &self.name
    }

    fn sector_count(&self) -> u64 {
        self.sectors
    }

    fn read_sectors(&self, lba: u64, buffer: &mut [u8]) -> Result<(), BlockError> {
        validate_io_range(lba, buffer.len(), self.sectors)?;
        self.parent
            .read_sectors(self.start_lba + lba, buffer)
    }

    fn write_sectors(&self, lba: u64, buffer: &[u8]) -> Result<(), BlockError> {
        validate_io_range(lba, buffer.len(), self.sectors)?;
        self.parent
            .write_sectors(self.start_lba + lba, buffer)
    }
}

fn validate_io_range(lba: u64, length: usize, sectors: u64) -> Result<(), BlockError> {
    if !length.is_multiple_of(SECTOR_SIZE) {
        return Err(BlockError::InvalidBuffer);
    }
    let count = (length / SECTOR_SIZE) as u64;
    if lba.checked_add(count).is_none_or(|end| end > sectors) {
        return Err(BlockError::OutOfRange);
    }
    Ok(())
}

pub fn read_gpt_partitions(device: &dyn BlockDevice) -> Result<Vec<GptPartition>, BlockError> {
    let total = device.sector_count();
    if total < 128 {
        return Err(BlockError::Unsupported);
    }

    let mut header = [0u8; SECTOR_SIZE];
    device.read_sectors(1, &mut header)?;
    if &header[..8] != GPT_HEADER_SIGNATURE {
        return Err(BlockError::Unsupported);
    }

    let header_size = read_u32(&header, 12)? as usize;
    if !(GPT_HEADER_SIZE..=SECTOR_SIZE).contains(&header_size) {
        return Err(BlockError::Unsupported);
    }
    let expected_header_crc = read_u32(&header, 16)?;
    let mut header_for_crc = header;
    header_for_crc[16..20].fill(0);
    if crc32(&header_for_crc[..header_size]) != expected_header_crc {
        return Err(BlockError::Io);
    }

    let current_lba = read_u64(&header, 24)?;
    let backup_lba = read_u64(&header, 32)?;
    let first_usable = read_u64(&header, 40)?;
    let last_usable = read_u64(&header, 48)?;
    let entries_lba = read_u64(&header, 72)?;
    let entry_count = read_u32(&header, 80)?.min(GPT_ENTRY_COUNT as u32) as usize;
    let entry_size = read_u32(&header, 84)? as usize;
    let expected_entries_crc = read_u32(&header, 88)?;

    if current_lba != 1
        || backup_lba >= total
        || first_usable >= total
        || last_usable >= total
        || first_usable > last_usable
        || entry_size < GPT_ENTRY_SIZE
        || entry_size > 512
        || !entry_size.is_multiple_of(8)
    {
        return Err(BlockError::Unsupported);
    }

    let bytes = entry_count
        .checked_mul(entry_size)
        .ok_or(BlockError::Unsupported)?;
    if bytes > 2 * 1024 * 1024 {
        return Err(BlockError::Unsupported);
    }
    let sectors = (bytes as u64).div_ceil(SECTOR_SIZE as u64);
    if entries_lba == 0
        || entries_lba
            .checked_add(sectors)
            .is_none_or(|end| end > total)
    {
        return Err(BlockError::OutOfRange);
    }

    let mut raw_entries = alloc::vec![0u8; sectors as usize * SECTOR_SIZE];
    if !raw_entries.is_empty() {
        device.read_sectors(entries_lba, &mut raw_entries)?;
    }
    if crc32(&raw_entries[..bytes]) != expected_entries_crc {
        return Err(BlockError::Io);
    }

    let mut partitions = Vec::new();
    for index in 0..entry_count {
        let offset = index * entry_size;
        let entry = &raw_entries[offset..offset + entry_size];
        let mut type_guid = [0; 16];
        type_guid.copy_from_slice(&entry[..16]);
        if type_guid == [0; 16] {
            continue;
        }
        let mut unique_guid = [0; 16];
        unique_guid.copy_from_slice(&entry[16..32]);
        let first_lba = read_u64(entry, 32)?;
        let last_lba = read_u64(entry, 40)?;
        if first_lba < first_usable
            || last_lba > last_usable
            || first_lba > last_lba
            || last_lba >= total
        {
            return Err(BlockError::OutOfRange);
        }
        partitions.push(GptPartition {
            index: index as u32 + 1,
            type_guid,
            unique_guid,
            first_lba,
            last_lba,
        });
    }
    Ok(partitions)
}

/// Replace a whole disk with an ESP + Linux filesystem GPT layout.
/// The caller must obtain explicit destructive confirmation before calling.
pub fn create_octagram_gpt(
    device: &dyn BlockDevice,
) -> Result<(GptPartition, GptPartition), BlockError> {
    const ESP_SECTORS: u64 = 512 * 1024 * 1024 / SECTOR_SIZE as u64;
    const FIRST_PARTITION_LBA: u64 = 2048;
    const MINIMUM_DISK_SECTORS: u64 = 16 * 1024 * 1024 * 1024 / SECTOR_SIZE as u64;

    let total = device.sector_count();
    if total < MINIMUM_DISK_SECTORS || total > u32::MAX as u64 * 1024 {
        return Err(BlockError::Unsupported);
    }

    let backup_header_lba = total - 1;
    let backup_entries_lba = backup_header_lba - GPT_ENTRY_SECTORS;
    let last_usable = backup_entries_lba - 1;
    let esp_first = FIRST_PARTITION_LBA;
    let esp_last = esp_first + ESP_SECTORS - 1;
    let root_first = align_lba(esp_last + 1, 2048);
    if root_first >= last_usable || last_usable - root_first < 1024 * 1024 {
        return Err(BlockError::Unsupported);
    }
    let root_last = last_usable;

    let seed = unique_seed(total);
    let disk_guid = guid_from_seed(seed ^ 0x4F4354414752414D);
    let esp_guid = guid_from_seed(seed ^ 0x4553502D4F435441);
    let root_guid = guid_from_seed(seed ^ 0x524F4F542D4F4354);

    let mut entries = alloc::vec![0u8; GPT_ENTRY_SIZE * GPT_ENTRY_COUNT];
    write_partition_entry(
        &mut entries[..GPT_ENTRY_SIZE],
        &EFI_SYSTEM_PARTITION_GUID,
        &esp_guid,
        esp_first,
        esp_last,
        "OCTAGRAM EFI",
    );
    write_partition_entry(
        &mut entries[GPT_ENTRY_SIZE..GPT_ENTRY_SIZE * 2],
        &LINUX_FILESYSTEM_GUID,
        &root_guid,
        root_first,
        root_last,
        "OCTAGRAM ROOT",
    );
    let entries_crc = crc32(&entries);

    let mut mbr = [0u8; SECTOR_SIZE];
    mbr[446] = 0x00;
    mbr[447..450].copy_from_slice(&[0x00, 0x02, 0x00]);
    mbr[450] = 0xEE;
    mbr[451..454].copy_from_slice(&[0xFF, 0xFF, 0xFF]);
    mbr[454..458].copy_from_slice(&1u32.to_le_bytes());
    mbr[458..462].copy_from_slice(&(total.saturating_sub(1).min(u32::MAX as u64) as u32).to_le_bytes());
    mbr[510..512].copy_from_slice(&[0x55, 0xAA]);
    device.write_sectors(0, &mbr)?;

    let mut primary_header = make_header(
        1,
        backup_header_lba,
        FIRST_PARTITION_LBA,
        last_usable,
        entries_crc,
        2,
        &disk_guid,
    );
    let primary_crc = crc32(&primary_header[..GPT_HEADER_SIZE]);
    primary_header[16..20].copy_from_slice(&primary_crc.to_le_bytes());
    let mut backup_header = make_header(
        backup_header_lba,
        1,
        FIRST_PARTITION_LBA,
        last_usable,
        entries_crc,
        backup_entries_lba,
        &disk_guid,
    );
    let backup_crc = crc32(&backup_header[..GPT_HEADER_SIZE]);
    backup_header[16..20].copy_from_slice(&backup_crc.to_le_bytes());

    for (index, sector) in entries.chunks_exact(SECTOR_SIZE).enumerate() {
        let mut block = [0u8; SECTOR_SIZE];
        block.copy_from_slice(sector);
        device.write_sectors(2 + index as u64, &block)?;
        device.write_sectors(backup_entries_lba + index as u64, &block)?;
    }
    device.write_sectors(1, &primary_header)?;
    device.write_sectors(backup_header_lba, &backup_header)?;

    Ok((
        GptPartition {
            index: 1,
            type_guid: EFI_SYSTEM_PARTITION_GUID,
            unique_guid: esp_guid,
            first_lba: esp_first,
            last_lba: esp_last,
        },
        GptPartition {
            index: 2,
            type_guid: LINUX_FILESYSTEM_GUID,
            unique_guid: root_guid,
            first_lba: root_first,
            last_lba: root_last,
        },
    ))
}

fn make_header(
    current_lba: u64,
    backup_lba: u64,
    first_usable: u64,
    last_usable: u64,
    entries_crc: u32,
    entries_lba: u64,
    disk_guid: &[u8; 16],
) -> [u8; SECTOR_SIZE] {
    let mut header = [0u8; SECTOR_SIZE];
    header[..8].copy_from_slice(GPT_HEADER_SIGNATURE);
    header[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
    header[12..16].copy_from_slice(&(GPT_HEADER_SIZE as u32).to_le_bytes());
    header[24..32].copy_from_slice(&current_lba.to_le_bytes());
    header[32..40].copy_from_slice(&backup_lba.to_le_bytes());
    header[40..48].copy_from_slice(&first_usable.to_le_bytes());
    header[48..56].copy_from_slice(&last_usable.to_le_bytes());
    header[56..72].copy_from_slice(disk_guid);
    header[72..80].copy_from_slice(&entries_lba.to_le_bytes());
    header[80..84].copy_from_slice(&(GPT_ENTRY_COUNT as u32).to_le_bytes());
    header[84..88].copy_from_slice(&(GPT_ENTRY_SIZE as u32).to_le_bytes());
    header[88..92].copy_from_slice(&entries_crc.to_le_bytes());
    header
}

fn write_partition_entry(
    entry: &mut [u8],
    type_guid: &[u8; 16],
    unique_guid: &[u8; 16],
    first_lba: u64,
    last_lba: u64,
    name: &str,
) {
    entry[..16].copy_from_slice(type_guid);
    entry[16..32].copy_from_slice(unique_guid);
    entry[32..40].copy_from_slice(&first_lba.to_le_bytes());
    entry[40..48].copy_from_slice(&last_lba.to_le_bytes());
    for (index, unit) in name.encode_utf16().take(36).enumerate() {
        let offset = 56 + index * 2;
        entry[offset..offset + 2].copy_from_slice(&unit.to_le_bytes());
    }
}

fn align_lba(value: u64, alignment: u64) -> u64 {
    value.div_ceil(alignment) * alignment
}

fn unique_seed(sectors: u64) -> u64 {
    #[cfg(target_arch = "x86_64")]
    let clock = unsafe { core::arch::x86_64::_rdtsc() };
    #[cfg(not(target_arch = "x86_64"))]
    let clock = 0x4F4354414752414Du64;
    clock ^ sectors.rotate_left(17) ^ 0x4F4354414752414D
}

fn guid_from_seed(seed: u64) -> [u8; 16] {
    let mut bytes = [0u8; 16];
    let a = seed.to_le_bytes();
    let b = seed.rotate_left(29).wrapping_mul(0x9E37_79B9_7F4A_7C15).to_le_bytes();
    bytes[..8].copy_from_slice(&a);
    bytes[8..].copy_from_slice(&b);
    bytes[7] = (bytes[7] & 0x0F) | 0x40;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;
    bytes
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32, BlockError> {
    let bytes = data.get(offset..offset + 4).ok_or(BlockError::InvalidBuffer)?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

fn read_u64(data: &[u8], offset: usize) -> Result<u64, BlockError> {
    let bytes = data.get(offset..offset + 8).ok_or(BlockError::InvalidBuffer)?;
    Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::{crc32, make_header, GPT_HEADER_SIZE};

    #[test]
    fn crc32_matches_standard_vector() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn gpt_header_has_expected_signature_and_size() {
        let header = make_header(1, 99, 34, 90, 123, 2, &[1; 16]);
        assert_eq!(&header[..8], b"EFI PART");
        assert_eq!(u32::from_le_bytes(header[12..16].try_into().unwrap()) as usize, GPT_HEADER_SIZE);
        assert_eq!(u32::from_le_bytes(header[16..20].try_into().unwrap()), 0);
    }
}
