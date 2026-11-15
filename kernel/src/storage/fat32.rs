use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::fs::{DirectoryEntry, FileSystem, FsError, Metadata, NodeType, normalize_path};

use super::block::{BlockDevice, BlockDeviceRef, SECTOR_SIZE};

const FAT32_PARTITION_TYPES: [u8; 4] = [0x0B, 0x0C, 0x1B, 0x1C];
const MAX_DIRECTORY_ENTRIES: usize = 8192;
const FAT32_EOC_MIN: u32 = 0x0FFF_FFF8;
const FAT32_EOC: u32 = 0x0FFF_FFFF;
const FSINFO_LEAD_SIGNATURE: u32 = 0x4161_5252;
const FSINFO_STRUCT_SIGNATURE: u32 = 0x6141_7272;
const FSINFO_TRAIL_SIGNATURE: u32 = 0xAA55_0000;

struct Geometry {
    partition_start: u64,
    partition_sectors: u64,
    sectors_per_cluster: u32,
    fat_start: u64,
    sectors_per_fat: u32,
    fat_count: u8,
    active_fat: u8,
    fat_mirroring: bool,
    data_start: u64,
    cluster_count: u32,
    root_cluster: u32,
    fsinfo_sector: Option<u32>,
}

#[derive(Clone, Copy)]
struct EntryLocation {
    sector_lba: u64,
    offset: usize,
}

#[derive(Clone)]
struct DirectoryRecord {
    name: String,
    short_name: [u8; 11],
    attributes: u8,
    first_cluster: u32,
    size: u32,
    short_entry: EntryLocation,
    long_entries: Vec<EntryLocation>,
}

struct LongName {
    units: [u16; 260],
    checksum: u8,
    valid: bool,
    locations: Vec<EntryLocation>,
}

impl LongName {
    const fn new() -> Self {
        Self {
            units: [0xFFFF; 260],
            checksum: 0,
            valid: false,
            locations: Vec::new(),
        }
    }

    fn reset(&mut self) {
        self.units.fill(0xFFFF);
        self.checksum = 0;
        self.valid = false;
        self.locations.clear();
    }

    fn push(&mut self, entry: &[u8], location: EntryLocation) {
        let sequence = entry[0] & 0x1F;
        if sequence == 0 || sequence as usize * 13 > self.units.len() {
            self.reset();
            return;
        }
        if entry[0] & 0x40 != 0 {
            self.reset();
            self.valid = true;
            self.checksum = entry[13];
        }
        if !self.valid || entry[13] != self.checksum {
            self.reset();
            return;
        }

        let start = (sequence as usize - 1) * 13;
        let mut unit_index = 0;
        for (offset, count) in [(1usize, 5usize), (14, 6), (28, 2)] {
            for unit in 0..count {
                let byte = offset + unit * 2;
                self.units[start + unit_index] = u16::from_le_bytes([entry[byte], entry[byte + 1]]);
                unit_index += 1;
            }
        }
        if self.locations.try_reserve(1).is_err() {
            self.reset();
            return;
        }
        self.locations.push(location);
    }

    fn name(&self, short_name: &[u8; 11]) -> Option<String> {
        if !self.valid || short_name_checksum(short_name) != self.checksum {
            return None;
        }
        let end = self
            .units
            .iter()
            .position(|unit| *unit == 0 || *unit == 0xFFFF)
            .unwrap_or(self.units.len());
        String::from_utf16(&self.units[..end]).ok()
    }
}

pub struct Fat32 {
    device: BlockDeviceRef,
    geometry: Geometry,
    next_free_cluster: u32,
    free_cluster_count: Option<u32>,
}

impl Fat32 {
    pub fn format(device: &dyn BlockDevice) -> Result<(), FsError> {
        const RESERVED_SECTORS: u32 = 32;
        const FAT_COUNT: u64 = 2;
        const MIN_FAT32_CLUSTERS: u64 = 65_525;
        const TARGET_MAX_CLUSTERS: u64 = 1_000_000;

        let total_sectors = device.sector_count();
        if total_sectors > u32::MAX as u64 {
            return Err(FsError::Unsupported);
        }

        let mut sectors_per_cluster = 1u32;
        while total_sectors.div_ceil(sectors_per_cluster as u64) > TARGET_MAX_CLUSTERS
            && sectors_per_cluster < 64
        {
            sectors_per_cluster *= 2;
        }

        let fat_bytes = total_sectors.checked_mul(4).ok_or(FsError::Unsupported)?;
        let sectors_per_fat = fat_bytes.div_ceil(sectors_per_cluster as u64 * SECTOR_SIZE as u64);
        let data_start = RESERVED_SECTORS as u64 + FAT_COUNT * sectors_per_fat;
        if data_start >= total_sectors {
            return Err(FsError::Unsupported);
        }
        let cluster_count = (total_sectors - data_start) / sectors_per_cluster as u64;
        let fat_entries = sectors_per_fat
            .checked_mul((SECTOR_SIZE / 4) as u64)
            .ok_or(FsError::Unsupported)?;
        if cluster_count < MIN_FAT32_CLUSTERS
            || cluster_count > 0x0FFF_FFF5
            || fat_entries < cluster_count + 2
            || sectors_per_fat > u32::MAX as u64
        {
            return Err(FsError::Unsupported);
        }

        let clear_end = data_start + sectors_per_cluster as u64;
        let zeroes = [0u8; SECTOR_SIZE * 8];
        let mut lba = 0;
        while lba < clear_end {
            let count = (clear_end - lba).min(8) as usize;
            device
                .write_sectors(lba, &zeroes[..count * SECTOR_SIZE])
                .map_err(|_| FsError::IoError)?;
            lba += count as u64;
        }

        let mut boot = [0u8; SECTOR_SIZE];
        boot[..3].copy_from_slice(&[0xEB, 0x58, 0x90]);
        boot[3..11].copy_from_slice(b"OCTAGOS ");
        boot[11..13].copy_from_slice(&(SECTOR_SIZE as u16).to_le_bytes());
        boot[13] = sectors_per_cluster as u8;
        boot[14..16].copy_from_slice(&(RESERVED_SECTORS as u16).to_le_bytes());
        boot[16] = FAT_COUNT as u8;
        boot[21] = 0xF8;
        boot[24..26].copy_from_slice(&63u16.to_le_bytes());
        boot[26..28].copy_from_slice(&255u16.to_le_bytes());
        boot[32..36].copy_from_slice(&(total_sectors as u32).to_le_bytes());
        boot[36..40].copy_from_slice(&(sectors_per_fat as u32).to_le_bytes());
        boot[44..48].copy_from_slice(&2u32.to_le_bytes());
        boot[48..50].copy_from_slice(&1u16.to_le_bytes());
        boot[50..52].copy_from_slice(&6u16.to_le_bytes());
        boot[64] = 0x80;
        boot[66] = 0x29;
        boot[67..71].copy_from_slice(&0x4F43_5447u32.to_le_bytes());
        boot[71..82].copy_from_slice(b"OCTAGRAMOS ");
        boot[82..90].copy_from_slice(b"FAT32   ");
        boot[510..512].copy_from_slice(&0xAA55u16.to_le_bytes());

        let mut fsinfo = [0u8; SECTOR_SIZE];
        fsinfo[..4].copy_from_slice(&FSINFO_LEAD_SIGNATURE.to_le_bytes());
        fsinfo[484..488].copy_from_slice(&FSINFO_STRUCT_SIGNATURE.to_le_bytes());
        fsinfo[488..492].copy_from_slice(&((cluster_count - 1) as u32).to_le_bytes());
        fsinfo[492..496].copy_from_slice(&3u32.to_le_bytes());
        fsinfo[508..512].copy_from_slice(&FSINFO_TRAIL_SIGNATURE.to_le_bytes());

        let mut first_fat_sector = [0u8; SECTOR_SIZE];
        first_fat_sector[..4].copy_from_slice(&0x0FFF_FFF8u32.to_le_bytes());
        first_fat_sector[4..8].copy_from_slice(&FAT32_EOC.to_le_bytes());
        first_fat_sector[8..12].copy_from_slice(&FAT32_EOC.to_le_bytes());

        let second_fat_start = RESERVED_SECTORS as u64 + sectors_per_fat;
        device
            .write_sectors(0, &boot)
            .map_err(|_| FsError::IoError)?;
        device
            .write_sectors(1, &fsinfo)
            .map_err(|_| FsError::IoError)?;
        device
            .write_sectors(6, &boot)
            .map_err(|_| FsError::IoError)?;
        device
            .write_sectors(7, &fsinfo)
            .map_err(|_| FsError::IoError)?;
        device
            .write_sectors(RESERVED_SECTORS as u64, &first_fat_sector)
            .map_err(|_| FsError::IoError)?;
        device
            .write_sectors(second_fat_start, &first_fat_sector)
            .map_err(|_| FsError::IoError)?;
        Ok(())
    }

    pub fn mount(device: BlockDeviceRef) -> Result<Self, FsError> {
        let geometry = find_geometry(device.as_ref())?;
        let (free_cluster_count, next_free_cluster) = read_fsinfo(device.as_ref(), &geometry)?;
        Ok(Self {
            device,
            geometry,
            next_free_cluster,
            free_cluster_count,
        })
    }

    fn read_sector(
        &self,
        relative_lba: u64,
        buffer: &mut [u8; SECTOR_SIZE],
    ) -> Result<(), FsError> {
        if relative_lba >= self.geometry.partition_sectors {
            return Err(FsError::IoError);
        }
        let absolute_lba = self
            .geometry
            .partition_start
            .checked_add(relative_lba)
            .ok_or(FsError::IoError)?;
        self.device
            .read_sectors(absolute_lba, buffer)
            .map_err(|_| FsError::IoError)
    }

    fn write_sector(&self, relative_lba: u64, buffer: &[u8; SECTOR_SIZE]) -> Result<(), FsError> {
        if relative_lba >= self.geometry.partition_sectors {
            return Err(FsError::IoError);
        }
        let absolute_lba = self
            .geometry
            .partition_start
            .checked_add(relative_lba)
            .ok_or(FsError::IoError)?;
        self.device
            .write_sectors(absolute_lba, buffer)
            .map_err(|_| FsError::IoError)
    }

    fn fat_entry(&self, cluster: u32) -> Result<u32, FsError> {
        let byte_offset = (cluster as u64).checked_mul(4).ok_or(FsError::IoError)?;
        let fat_start = self.geometry.fat_start
            + self.geometry.active_fat as u64 * self.geometry.sectors_per_fat as u64;
        let sector_lba = fat_start + byte_offset / SECTOR_SIZE as u64;
        let offset = (byte_offset % SECTOR_SIZE as u64) as usize;
        let mut sector = [0u8; SECTOR_SIZE];
        self.read_sector(sector_lba, &mut sector)?;
        Ok(u32::from_le_bytes(
            sector[offset..offset + 4].try_into().unwrap(),
        ))
    }

    fn set_fat_entry(&mut self, cluster: u32, value: u32) -> Result<(), FsError> {
        if cluster >= self.geometry.cluster_count + 2 {
            return Err(FsError::IoError);
        }
        let old_value = self.fat_entry(cluster)? & 0x0FFF_FFFF;
        let value = value & 0x0FFF_FFFF;
        let byte_offset = (cluster as u64).checked_mul(4).ok_or(FsError::IoError)?;
        let first_copy = if self.geometry.fat_mirroring {
            0
        } else {
            self.geometry.active_fat
        };
        let copy_count = if self.geometry.fat_mirroring {
            self.geometry.fat_count
        } else {
            1
        };

        for copy in first_copy..first_copy + copy_count {
            let fat_start =
                self.geometry.fat_start + copy as u64 * self.geometry.sectors_per_fat as u64;
            let sector_lba = fat_start + byte_offset / SECTOR_SIZE as u64;
            let offset = (byte_offset % SECTOR_SIZE as u64) as usize;
            let mut sector = [0u8; SECTOR_SIZE];
            self.read_sector(sector_lba, &mut sector)?;
            let existing = u32::from_le_bytes(sector[offset..offset + 4].try_into().unwrap());
            sector[offset..offset + 4]
                .copy_from_slice(&((existing & 0xF000_0000) | value).to_le_bytes());
            self.write_sector(sector_lba, &sector)?;
        }

        if (old_value == 0) != (value == 0) {
            if old_value == 0 {
                self.free_cluster_count = self
                    .free_cluster_count
                    .and_then(|count| count.checked_sub(1));
                self.next_free_cluster = cluster
                    .checked_add(1)
                    .filter(|next| *next < self.geometry.cluster_count + 2)
                    .unwrap_or(2);
            } else {
                self.free_cluster_count = self.free_cluster_count.and_then(|count| {
                    let updated = count.checked_add(1)?;
                    (updated <= self.geometry.cluster_count).then_some(updated)
                });
                self.next_free_cluster = self.next_free_cluster.min(cluster);
            }
            self.write_fsinfo()?;
        }
        Ok(())
    }

    fn write_fsinfo(&self) -> Result<(), FsError> {
        let Some(sector_lba) = self.geometry.fsinfo_sector else {
            return Ok(());
        };
        let mut sector = [0u8; SECTOR_SIZE];
        self.read_sector(sector_lba as u64, &mut sector)?;
        if u32::from_le_bytes(sector[0..4].try_into().unwrap()) != FSINFO_LEAD_SIGNATURE
            || u32::from_le_bytes(sector[484..488].try_into().unwrap()) != FSINFO_STRUCT_SIGNATURE
            || u32::from_le_bytes(sector[508..512].try_into().unwrap()) != FSINFO_TRAIL_SIGNATURE
        {
            return Ok(());
        }
        let free_count = self.free_cluster_count.unwrap_or(u32::MAX);
        sector[488..492].copy_from_slice(&free_count.to_le_bytes());
        sector[492..496].copy_from_slice(&self.next_free_cluster.to_le_bytes());
        self.write_sector(sector_lba as u64, &sector)
    }

    fn next_cluster(&self, cluster: u32) -> Result<Option<u32>, FsError> {
        if cluster < 2 || cluster >= self.geometry.cluster_count + 2 {
            return Err(FsError::IoError);
        }
        let value = self.fat_entry(cluster)? & 0x0FFF_FFFF;
        if value >= FAT32_EOC_MIN {
            Ok(None)
        } else if value >= 2 && value < self.geometry.cluster_count + 2 {
            Ok(Some(value))
        } else {
            Err(FsError::IoError)
        }
    }

    fn cluster_lba(&self, cluster: u32) -> Result<u64, FsError> {
        if cluster < 2 || cluster >= self.geometry.cluster_count + 2 {
            return Err(FsError::IoError);
        }
        self.geometry
            .data_start
            .checked_add((cluster as u64 - 2) * self.geometry.sectors_per_cluster as u64)
            .ok_or(FsError::IoError)
    }

    fn directory_records(&self, first_cluster: u32) -> Result<Vec<DirectoryRecord>, FsError> {
        let mut records = Vec::new();
        let mut long_name = LongName::new();
        let mut cluster = first_cluster;

        for _ in 0..self.geometry.cluster_count {
            let cluster_lba = self.cluster_lba(cluster)?;
            for sector_offset in 0..self.geometry.sectors_per_cluster {
                let mut sector = [0u8; SECTOR_SIZE];
                self.read_sector(cluster_lba + sector_offset as u64, &mut sector)?;
                for (slot_index, entry) in sector.chunks_exact(32).enumerate() {
                    let location = EntryLocation {
                        sector_lba: cluster_lba + sector_offset as u64,
                        offset: slot_index * 32,
                    };
                    if entry[0] == 0 {
                        return Ok(records);
                    }
                    if entry[0] == 0xE5 {
                        long_name.reset();
                        continue;
                    }
                    if entry[11] == 0x0F {
                        long_name.push(entry, location);
                        continue;
                    }

                    let mut short_bytes = [0u8; 11];
                    short_bytes.copy_from_slice(&entry[..11]);
                    let decoded_long_name = long_name.name(&short_bytes);
                    let name = decoded_long_name
                        .clone()
                        .unwrap_or_else(|| short_name(&short_bytes));
                    let long_entries = if decoded_long_name.is_some() {
                        long_name.locations.clone()
                    } else {
                        Vec::new()
                    };
                    long_name.reset();
                    let attributes = entry[11];
                    if attributes & 0x08 != 0 || name == "." || name == ".." {
                        continue;
                    }
                    if records.len() >= MAX_DIRECTORY_ENTRIES {
                        return Err(FsError::NoSpace);
                    }

                    let high = u16::from_le_bytes([entry[20], entry[21]]) as u32;
                    let low = u16::from_le_bytes([entry[26], entry[27]]) as u32;
                    records.push(DirectoryRecord {
                        name,
                        short_name: short_bytes,
                        attributes,
                        first_cluster: (high << 16) | low,
                        size: u32::from_le_bytes(entry[28..32].try_into().unwrap()),
                        short_entry: location,
                        long_entries,
                    });
                }
            }
            match self.next_cluster(cluster)? {
                Some(next) => cluster = next,
                None => return Ok(records),
            }
        }
        Err(FsError::IoError)
    }

    fn find_record(&self, path: &str) -> Result<Option<DirectoryRecord>, FsError> {
        let path = normalize_path(path)?;
        if path == "/" {
            return Ok(None);
        }
        let components: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
        let mut directory_cluster = self.geometry.root_cluster;
        let mut result = None;
        for (index, component) in components.iter().enumerate() {
            let record = self
                .directory_records(directory_cluster)?
                .into_iter()
                .find(|entry| entry.name.eq_ignore_ascii_case(component))
                .ok_or(FsError::NotFound)?;
            if index + 1 < components.len() {
                if record.attributes & 0x10 == 0 {
                    return Err(FsError::NotDirectory);
                }
                directory_cluster = record.first_cluster;
            }
            result = Some(record);
        }
        Ok(result)
    }

    fn read_file(
        &self,
        record: &DirectoryRecord,
        offset: usize,
        buffer: &mut [u8],
    ) -> Result<usize, FsError> {
        if record.attributes & 0x10 != 0 {
            return Err(FsError::IsDirectory);
        }
        let file_size = record.size as usize;
        if offset >= file_size || buffer.is_empty() {
            return Ok(0);
        }
        let count = buffer.len().min(file_size - offset);
        let cluster_size = self.geometry.sectors_per_cluster as usize * SECTOR_SIZE;
        let mut cluster = record.first_cluster;
        let mut skip_clusters = offset / cluster_size;
        while skip_clusters != 0 {
            cluster = self.next_cluster(cluster)?.ok_or(FsError::IoError)?;
            skip_clusters -= 1;
        }

        let mut within_cluster = offset % cluster_size;
        let mut copied = 0;
        while copied < count {
            let cluster_lba = self.cluster_lba(cluster)?;
            let sector_index = within_cluster / SECTOR_SIZE;
            let sector_offset = within_cluster % SECTOR_SIZE;
            let mut sector = [0u8; SECTOR_SIZE];
            self.read_sector(cluster_lba + sector_index as u64, &mut sector)?;
            let copy_len = (SECTOR_SIZE - sector_offset).min(count - copied);
            buffer[copied..copied + copy_len]
                .copy_from_slice(&sector[sector_offset..sector_offset + copy_len]);
            copied += copy_len;
            within_cluster += copy_len;
            if copied < count && within_cluster >= cluster_size {
                cluster = self.next_cluster(cluster)?.ok_or(FsError::IoError)?;
                within_cluster = 0;
            }
        }
        Ok(copied)
    }

    fn cluster_chain(&self, first_cluster: u32) -> Result<Vec<u32>, FsError> {
        let mut chain = Vec::new();
        if first_cluster == 0 {
            return Ok(chain);
        }
        let mut cluster = first_cluster;
        for _ in 0..self.geometry.cluster_count {
            if cluster < 2 || cluster >= self.geometry.cluster_count + 2 {
                return Err(FsError::IoError);
            }
            chain.try_reserve(1).map_err(|_| FsError::NoSpace)?;
            chain.push(cluster);
            match self.next_cluster(cluster)? {
                Some(next) => cluster = next,
                None => return Ok(chain),
            }
        }
        Err(FsError::IoError)
    }

    fn allocate_clusters(&mut self, count: usize) -> Result<Vec<u32>, FsError> {
        let mut allocated = Vec::new();
        allocated
            .try_reserve_exact(count)
            .map_err(|_| FsError::NoSpace)?;
        if count == 0 {
            return Ok(allocated);
        }
        let cluster_limit = self.geometry.cluster_count + 2;
        let mut candidate = if (2..cluster_limit).contains(&self.next_free_cluster) {
            self.next_free_cluster
        } else {
            2
        };

        for _ in 0..self.geometry.cluster_count {
            let free = match self.fat_entry(candidate) {
                Ok(entry) => entry & 0x0FFF_FFFF == 0,
                Err(error) => {
                    self.release_clusters(&allocated)?;
                    return Err(error);
                }
            };
            if free {
                if let Err(error) = self.set_fat_entry(candidate, FAT32_EOC) {
                    self.set_fat_entry(candidate, 0)?;
                    self.release_clusters(&allocated)?;
                    return Err(error);
                }
                allocated.push(candidate);
                if allocated.len() == count {
                    return Ok(allocated);
                }
            }
            candidate = if candidate + 1 >= cluster_limit {
                2
            } else {
                candidate + 1
            };
        }

        self.release_clusters(&allocated)?;
        Err(FsError::NoSpace)
    }

    fn release_clusters(&mut self, clusters: &[u32]) -> Result<(), FsError> {
        for &cluster in clusters.iter().rev() {
            self.set_fat_entry(cluster, 0)?;
        }
        Ok(())
    }

    fn zero_cluster(&self, cluster: u32) -> Result<(), FsError> {
        let first_sector = self.cluster_lba(cluster)?;
        let zeroes = [0u8; SECTOR_SIZE];
        for sector_offset in 0..self.geometry.sectors_per_cluster {
            self.write_sector(first_sector + sector_offset as u64, &zeroes)?;
        }
        Ok(())
    }

    fn write_chain_range(&self, chain: &[u32], offset: usize, data: &[u8]) -> Result<(), FsError> {
        let cluster_size = self.geometry.sectors_per_cluster as usize * SECTOR_SIZE;
        let mut position = offset;
        let mut written = 0;
        while written < data.len() {
            let cluster_index = position / cluster_size;
            let cluster = *chain.get(cluster_index).ok_or(FsError::IoError)?;
            let cluster_offset = position % cluster_size;
            let sector_lba = self.cluster_lba(cluster)? + (cluster_offset / SECTOR_SIZE) as u64;
            let sector_offset = cluster_offset % SECTOR_SIZE;
            let copy_len = (SECTOR_SIZE - sector_offset).min(data.len() - written);
            let mut sector = [0u8; SECTOR_SIZE];
            if sector_offset != 0 || copy_len != SECTOR_SIZE {
                self.read_sector(sector_lba, &mut sector)?;
            }
            sector[sector_offset..sector_offset + copy_len]
                .copy_from_slice(&data[written..written + copy_len]);
            self.write_sector(sector_lba, &sector)?;
            position = position.checked_add(copy_len).ok_or(FsError::NoSpace)?;
            written += copy_len;
        }
        Ok(())
    }

    fn zero_chain_range(
        &self,
        chain: &[u32],
        mut offset: usize,
        mut length: usize,
    ) -> Result<(), FsError> {
        let zeroes = [0u8; SECTOR_SIZE];
        while length != 0 {
            let count = length.min(SECTOR_SIZE - (offset % SECTOR_SIZE));
            self.write_chain_range(chain, offset, &zeroes[..count])?;
            offset = offset.checked_add(count).ok_or(FsError::NoSpace)?;
            length -= count;
        }
        Ok(())
    }

    fn connect_clusters(
        &mut self,
        existing_last: Option<u32>,
        new_clusters: &[u32],
    ) -> Result<(), FsError> {
        for pair in new_clusters.windows(2) {
            self.set_fat_entry(pair[0], pair[1])?;
        }
        if let Some(&last) = new_clusters.last() {
            self.set_fat_entry(last, FAT32_EOC)?;
            if let Some(existing_last) = existing_last {
                self.set_fat_entry(existing_last, new_clusters[0])?;
            }
        }
        Ok(())
    }

    fn rollback_new_clusters(
        &mut self,
        existing_last: Option<u32>,
        new_clusters: &[u32],
    ) -> Result<(), FsError> {
        if let Some(existing_last) = existing_last {
            self.set_fat_entry(existing_last, FAT32_EOC)?;
        }
        self.release_clusters(new_clusters)
    }

    fn find_free_directory_slots(
        &mut self,
        first_cluster: u32,
        needed: usize,
    ) -> Result<Vec<EntryLocation>, FsError> {
        let chain = self.cluster_chain(first_cluster)?;
        if chain.is_empty() || needed == 0 {
            return Err(FsError::IoError);
        }
        let mut free_run = Vec::new();
        free_run
            .try_reserve_exact(needed)
            .map_err(|_| FsError::NoSpace)?;
        let mut reached_end = false;
        for &cluster in &chain {
            let first_sector = self.cluster_lba(cluster)?;
            for sector_offset in 0..self.geometry.sectors_per_cluster {
                let sector_lba = first_sector + sector_offset as u64;
                let mut sector = [0u8; SECTOR_SIZE];
                self.read_sector(sector_lba, &mut sector)?;
                for (slot_index, entry) in sector.chunks_exact(32).enumerate() {
                    if entry[0] == 0 {
                        reached_end = true;
                    }
                    if reached_end || entry[0] == 0xE5 {
                        free_run.push(EntryLocation {
                            sector_lba,
                            offset: slot_index * 32,
                        });
                        if free_run.len() == needed {
                            return Ok(free_run);
                        }
                    } else {
                        free_run.clear();
                    }
                }
            }
        }

        let new_cluster = self.allocate_clusters(1)?;
        let new_cluster = new_cluster[0];
        if let Err(error) = self.zero_cluster(new_cluster) {
            self.release_clusters(&[new_cluster])?;
            return Err(error);
        }
        let last_cluster = *chain.last().ok_or(FsError::IoError)?;
        if let Err(error) = self.set_fat_entry(last_cluster, new_cluster) {
            self.set_fat_entry(last_cluster, FAT32_EOC)?;
            self.release_clusters(&[new_cluster])?;
            return Err(error);
        }

        let first_sector = self.cluster_lba(new_cluster)?;
        for sector_offset in 0..self.geometry.sectors_per_cluster {
            let sector_lba = first_sector + sector_offset as u64;
            for slot_index in 0..SECTOR_SIZE / 32 {
                free_run.push(EntryLocation {
                    sector_lba,
                    offset: slot_index * 32,
                });
                if free_run.len() == needed {
                    return Ok(free_run);
                }
            }
        }
        Err(FsError::NoSpace)
    }

    fn write_directory_entries(
        &self,
        locations: &[EntryLocation],
        entries: &[[u8; 32]],
    ) -> Result<(), FsError> {
        if locations.len() != entries.len() {
            return Err(FsError::IoError);
        }
        for (location, entry) in locations.iter().zip(entries) {
            let mut sector = [0u8; SECTOR_SIZE];
            self.read_sector(location.sector_lba, &mut sector)?;
            sector[location.offset..location.offset + 32].copy_from_slice(entry);
            self.write_sector(location.sector_lba, &sector)?;
        }
        Ok(())
    }

    fn update_record(
        &self,
        record: &DirectoryRecord,
        first_cluster: u32,
        size: u32,
    ) -> Result<(), FsError> {
        let mut sector = [0u8; SECTOR_SIZE];
        self.read_sector(record.short_entry.sector_lba, &mut sector)?;
        let entry = &mut sector[record.short_entry.offset..record.short_entry.offset + 32];
        entry[20..22].copy_from_slice(&((first_cluster >> 16) as u16).to_le_bytes());
        entry[26..28].copy_from_slice(&(first_cluster as u16).to_le_bytes());
        entry[28..32].copy_from_slice(&size.to_le_bytes());
        self.write_sector(record.short_entry.sector_lba, &sector)
    }

    fn parent_directory(&self, path: &str) -> Result<(u32, String), FsError> {
        let path = normalize_path(path)?;
        if path == "/" {
            return Err(FsError::InvalidPath);
        }
        let separator = path.rfind('/').ok_or(FsError::InvalidPath)?;
        let parent_path = if separator == 0 {
            "/"
        } else {
            &path[..separator]
        };
        let name = String::from(&path[separator + 1..]);
        validate_name(&name)?;
        let parent_cluster = match self.find_record(parent_path)? {
            None => self.geometry.root_cluster,
            Some(record) if record.attributes & 0x10 != 0 => record.first_cluster,
            Some(_) => return Err(FsError::NotDirectory),
        };
        Ok((parent_cluster, name))
    }

    fn insert_entry(
        &mut self,
        parent_cluster: u32,
        name: &str,
        attributes: u8,
        first_cluster: u32,
        size: u32,
    ) -> Result<(), FsError> {
        let records = self.directory_records(parent_cluster)?;
        if records
            .iter()
            .any(|record| record.name.eq_ignore_ascii_case(name))
        {
            return Err(FsError::AlreadyExists);
        }
        let short_name = generate_short_name(name, &records)?;
        let entries = make_directory_entries(name, &short_name, attributes, first_cluster, size)?;
        let locations = self.find_free_directory_slots(parent_cluster, entries.len())?;
        self.write_directory_entries(&locations, &entries)
    }
}

impl FileSystem for Fat32 {
    fn metadata(&self, path: &str) -> Result<Metadata, FsError> {
        match self.find_record(path)? {
            None => Ok(Metadata {
                node_type: NodeType::Directory,
                len: 0,
                permissions: 0o755,
            }),
            Some(record) => Ok(Metadata {
                node_type: if record.attributes & 0x10 != 0 {
                    NodeType::Directory
                } else {
                    NodeType::File
                },
                len: record.size as usize,
                permissions: record_permissions(&record),
            }),
        }
    }

    fn read(&self, path: &str, offset: usize, buffer: &mut [u8]) -> Result<usize, FsError> {
        let record = self.find_record(path)?.ok_or(FsError::IsDirectory)?;
        self.read_file(&record, offset, buffer)
    }

    fn write(&mut self, path: &str, offset: usize, data: &[u8]) -> Result<usize, FsError> {
        let path = normalize_path(path)?;
        let record = self.find_record(&path)?.ok_or(FsError::IsDirectory)?;
        if record.attributes & 0x10 != 0 {
            return Err(FsError::IsDirectory);
        }
        if record.attributes & 0x01 != 0 {
            return Err(FsError::ReadOnly);
        }
        if data.is_empty() {
            return Ok(0);
        }
        let end = offset.checked_add(data.len()).ok_or(FsError::NoSpace)?;
        if end > u32::MAX as usize {
            return Err(FsError::NoSpace);
        }

        let old_size = record.size as usize;
        let cluster_size = self.geometry.sectors_per_cluster as usize * SECTOR_SIZE;
        let required_clusters = end.div_ceil(cluster_size);
        let mut chain = self.cluster_chain(record.first_cluster)?;
        if chain.len() < (old_size.div_ceil(cluster_size)) {
            return Err(FsError::IoError);
        }
        let new_count = required_clusters.saturating_sub(chain.len());
        let new_clusters = self.allocate_clusters(new_count)?;
        if chain.try_reserve(new_clusters.len()).is_err() {
            self.release_clusters(&new_clusters)?;
            return Err(FsError::NoSpace);
        }
        for &cluster in &new_clusters {
            if let Err(error) = self.zero_cluster(cluster) {
                self.release_clusters(&new_clusters)?;
                return Err(error);
            }
        }
        chain.extend_from_slice(&new_clusters);

        if offset > old_size
            && let Err(error) = self.zero_chain_range(&chain, old_size, offset - old_size)
        {
            self.release_clusters(&new_clusters)?;
            return Err(error);
        }
        if let Err(error) = self.write_chain_range(&chain, offset, data) {
            self.release_clusters(&new_clusters)?;
            return Err(error);
        }

        let existing_last = if chain.len() <= new_clusters.len() {
            None
        } else {
            chain.get(chain.len() - new_clusters.len() - 1).copied()
        };
        if let Err(error) = self.connect_clusters(existing_last, &new_clusters) {
            self.rollback_new_clusters(existing_last, &new_clusters)?;
            return Err(error);
        }
        let first_cluster = chain.first().copied().unwrap_or(0);
        let size = old_size.max(end) as u32;
        if let Err(error) = self.update_record(&record, first_cluster, size) {
            self.rollback_new_clusters(existing_last, &new_clusters)?;
            return Err(error);
        }
        Ok(data.len())
    }

    fn truncate(&mut self, path: &str, len: usize) -> Result<(), FsError> {
        let path = normalize_path(path)?;
        let record = self.find_record(&path)?.ok_or(FsError::IsDirectory)?;
        if record.attributes & 0x10 != 0 {
            return Err(FsError::IsDirectory);
        }
        if record.attributes & 0x01 != 0 {
            return Err(FsError::ReadOnly);
        }
        if len > u32::MAX as usize {
            return Err(FsError::NoSpace);
        }
        let old_size = record.size as usize;
        if len == old_size {
            return Ok(());
        }

        let cluster_size = self.geometry.sectors_per_cluster as usize * SECTOR_SIZE;
        let required_clusters = len.div_ceil(cluster_size);
        let mut chain = self.cluster_chain(record.first_cluster)?;
        if chain.len() < old_size.div_ceil(cluster_size) {
            return Err(FsError::IoError);
        }

        if len > old_size {
            let new_count = required_clusters.saturating_sub(chain.len());
            let new_clusters = self.allocate_clusters(new_count)?;
            if chain.try_reserve(new_clusters.len()).is_err() {
                self.release_clusters(&new_clusters)?;
                return Err(FsError::NoSpace);
            }
            for &cluster in &new_clusters {
                if let Err(error) = self.zero_cluster(cluster) {
                    self.release_clusters(&new_clusters)?;
                    return Err(error);
                }
            }
            chain.extend_from_slice(&new_clusters);
            if let Err(error) = self.zero_chain_range(&chain, old_size, len - old_size) {
                self.release_clusters(&new_clusters)?;
                return Err(error);
            }
            let existing_last = if chain.len() <= new_clusters.len() {
                None
            } else {
                chain.get(chain.len() - new_clusters.len() - 1).copied()
            };
            if let Err(error) = self.connect_clusters(existing_last, &new_clusters) {
                self.rollback_new_clusters(existing_last, &new_clusters)?;
                return Err(error);
            }
            let first_cluster = chain.first().copied().unwrap_or(0);
            if let Err(error) = self.update_record(&record, first_cluster, len as u32) {
                self.rollback_new_clusters(existing_last, &new_clusters)?;
                return Err(error);
            }
            return Ok(());
        }

        let first_cluster = if required_clusters == 0 {
            0
        } else {
            *chain.first().ok_or(FsError::IoError)?
        };
        self.update_record(&record, first_cluster, len as u32)?;
        if required_clusters < chain.len() {
            let tail = chain.split_off(required_clusters);
            if let Some(&last_kept) = chain.last() {
                self.set_fat_entry(last_kept, FAT32_EOC)?;
            }
            self.release_clusters(&tail)?;
        }
        Ok(())
    }

    fn create_file(&mut self, path: &str) -> Result<(), FsError> {
        let (parent_cluster, name) = self.parent_directory(path)?;
        self.insert_entry(parent_cluster, &name, 0x20, 0, 0)
    }

    fn create_dir(&mut self, path: &str) -> Result<(), FsError> {
        let (parent_cluster, name) = self.parent_directory(path)?;
        if self
            .directory_records(parent_cluster)?
            .iter()
            .any(|record| record.name.eq_ignore_ascii_case(&name))
        {
            return Err(FsError::AlreadyExists);
        }
        let cluster = self.allocate_clusters(1)?[0];
        if let Err(error) = self.zero_cluster(cluster) {
            self.release_clusters(&[cluster])?;
            return Err(error);
        }

        let mut dot = [0u8; 32];
        dot[..11].copy_from_slice(b".          ");
        dot[11] = 0x10;
        write_cluster_fields(&mut dot, cluster, 0);
        let mut dot_dot = [0u8; 32];
        dot_dot[..11].copy_from_slice(b"..         ");
        dot_dot[11] = 0x10;
        write_cluster_fields(&mut dot_dot, parent_cluster, 0);
        let location = EntryLocation {
            sector_lba: self.cluster_lba(cluster)?,
            offset: 0,
        };
        let dot_dot_location = EntryLocation {
            sector_lba: location.sector_lba,
            offset: 32,
        };
        if let Err(error) =
            self.write_directory_entries(&[location, dot_dot_location], &[dot, dot_dot])
        {
            self.release_clusters(&[cluster])?;
            return Err(error);
        }
        match self.insert_entry(parent_cluster, &name, 0x10, cluster, 0) {
            Ok(()) => Ok(()),
            Err(error @ (FsError::AlreadyExists | FsError::NoSpace)) => {
                self.release_clusters(&[cluster])?;
                Err(error)
            }
            Err(error) => Err(error),
        }
    }

    fn remove(&mut self, path: &str) -> Result<(), FsError> {
        let path = normalize_path(path)?;
        if path == "/" {
            return Err(FsError::Busy);
        }
        let record = self.find_record(&path)?.ok_or(FsError::NotFound)?;
        if record.attributes & 0x01 != 0 {
            return Err(FsError::ReadOnly);
        }
        if record.attributes & 0x10 != 0
            && !self.directory_records(record.first_cluster)?.is_empty()
        {
            return Err(FsError::DirectoryNotEmpty);
        }

        mark_deleted(self, record.short_entry)?;
        for location in record.long_entries {
            mark_deleted(self, location)?;
        }
        if record.first_cluster >= 2 {
            let chain = self.cluster_chain(record.first_cluster)?;
            self.release_clusters(&chain)?;
        }
        Ok(())
    }

    fn read_dir(&self, path: &str) -> Result<Vec<DirectoryEntry>, FsError> {
        let cluster = match self.find_record(path)? {
            None => self.geometry.root_cluster,
            Some(record) if record.attributes & 0x10 != 0 => record.first_cluster,
            Some(_) => return Err(FsError::NotDirectory),
        };
        Ok(self
            .directory_records(cluster)?
            .into_iter()
            .map(|record| {
                let permissions = record_permissions(&record);
                DirectoryEntry {
                    name: record.name,
                    metadata: Metadata {
                        node_type: if record.attributes & 0x10 != 0 {
                            NodeType::Directory
                        } else {
                            NodeType::File
                        },
                        len: record.size as usize,
                        permissions,
                    },
                }
            })
            .collect())
    }
}

fn record_permissions(record: &DirectoryRecord) -> u16 {
    let read_only = record.attributes & 0x01 != 0;
    if record.attributes & 0x10 != 0 {
        return if read_only { 0o555 } else { 0o755 };
    }

    let mut permissions = if read_only { 0o444 } else { 0o644 };
    if record
        .name
        .rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("elf"))
    {
        permissions |= 0o111;
    }
    permissions
}

fn find_geometry(device: &dyn BlockDevice) -> Result<Geometry, FsError> {
    let mut mbr = [0u8; SECTOR_SIZE];
    device
        .read_sectors(0, &mut mbr)
        .map_err(|_| FsError::IoError)?;
    let has_mbr_signature = mbr[510] == 0x55 && mbr[511] == 0xAA;
    let mut candidates = Vec::new();

    if has_mbr_signature {
        for index in 0..4 {
            let offset = 446 + index * 16;
            let partition_type = mbr[offset + 4];
            if FAT32_PARTITION_TYPES.contains(&partition_type) {
                let start = u32::from_le_bytes(mbr[offset + 8..offset + 12].try_into().unwrap());
                let sectors = u32::from_le_bytes(mbr[offset + 12..offset + 16].try_into().unwrap());
                candidates.push((start as u64, sectors as u64));
            }
        }
    }
    let whole_device = (0, device.sector_count());
    if !candidates.contains(&whole_device) {
        candidates.push(whole_device);
    }

    for (partition_start, partition_sectors) in candidates {
        let mut boot = [0u8; SECTOR_SIZE];
        if partition_sectors == 0
            || partition_start >= device.sector_count()
            || partition_start
                .checked_add(partition_sectors)
                .is_none_or(|end| end > device.sector_count())
            || device.read_sectors(partition_start, &mut boot).is_err()
        {
            continue;
        }
        if boot[510] != 0x55 || boot[511] != 0xAA {
            continue;
        }

        let bytes_per_sector = u16::from_le_bytes([boot[11], boot[12]]) as usize;
        let sectors_per_cluster = boot[13] as u32;
        let reserved_sectors = u16::from_le_bytes([boot[14], boot[15]]) as u32;
        let fat_count = boot[16];
        let total_sectors_16 = u16::from_le_bytes([boot[19], boot[20]]) as u32;
        let sectors_per_fat_16 = u16::from_le_bytes([boot[22], boot[23]]) as u32;
        let total_sectors_32 = u32::from_le_bytes(boot[32..36].try_into().unwrap());
        let sectors_per_fat = u32::from_le_bytes(boot[36..40].try_into().unwrap());
        let root_cluster = u32::from_le_bytes(boot[44..48].try_into().unwrap()) & 0x0FFF_FFFF;
        let total_sectors = if total_sectors_16 != 0 {
            total_sectors_16
        } else {
            total_sectors_32
        };
        let sectors_per_fat = if sectors_per_fat != 0 {
            sectors_per_fat
        } else {
            sectors_per_fat_16
        };
        let extended_flags = u16::from_le_bytes([boot[40], boot[41]]);
        let fat_mirroring = extended_flags & 0x0080 == 0;
        let active_fat = if fat_mirroring {
            0
        } else {
            (extended_flags & 0x000F) as u8
        };
        let fsinfo_sector = u16::from_le_bytes([boot[48], boot[49]]) as u32;
        let fsinfo_sector =
            (fsinfo_sector > 0 && fsinfo_sector < reserved_sectors).then_some(fsinfo_sector);
        let fat_start = reserved_sectors as u64;
        let data_start = fat_start + fat_count as u64 * sectors_per_fat as u64;
        if bytes_per_sector != SECTOR_SIZE
            || sectors_per_cluster == 0
            || !sectors_per_cluster.is_power_of_two()
            || reserved_sectors == 0
            || !(1..=2).contains(&fat_count)
            || active_fat >= fat_count
            || sectors_per_fat == 0
            || total_sectors == 0
            || total_sectors as u64 > partition_sectors
            || data_start >= total_sectors as u64
            || root_cluster < 2
        {
            continue;
        }

        let cluster_count =
            ((total_sectors as u64 - data_start) / sectors_per_cluster as u64) as u32;
        let fat_entries = sectors_per_fat as u64 * SECTOR_SIZE as u64 / 4;
        if cluster_count == 0
            || root_cluster as u64 >= cluster_count as u64 + 2
            || cluster_count > 0x0FFF_FFF5
            || fat_entries < cluster_count as u64 + 2
        {
            continue;
        }

        return Ok(Geometry {
            partition_start,
            partition_sectors: total_sectors as u64,
            sectors_per_cluster,
            fat_start,
            sectors_per_fat,
            fat_count,
            active_fat,
            fat_mirroring,
            data_start,
            cluster_count,
            root_cluster,
            fsinfo_sector,
        });
    }

    Err(FsError::Unsupported)
}

fn read_fsinfo(
    device: &dyn BlockDevice,
    geometry: &Geometry,
) -> Result<(Option<u32>, u32), FsError> {
    let Some(sector_lba) = geometry.fsinfo_sector else {
        return Ok((None, 2));
    };
    let mut sector = [0u8; SECTOR_SIZE];
    let absolute_lba = geometry
        .partition_start
        .checked_add(sector_lba as u64)
        .ok_or(FsError::IoError)?;
    device
        .read_sectors(absolute_lba, &mut sector)
        .map_err(|_| FsError::IoError)?;
    if u32::from_le_bytes(sector[0..4].try_into().unwrap()) != FSINFO_LEAD_SIGNATURE
        || u32::from_le_bytes(sector[484..488].try_into().unwrap()) != FSINFO_STRUCT_SIGNATURE
        || u32::from_le_bytes(sector[508..512].try_into().unwrap()) != FSINFO_TRAIL_SIGNATURE
    {
        return Ok((None, 2));
    }
    let free_count = u32::from_le_bytes(sector[488..492].try_into().unwrap());
    let free_count =
        (free_count != u32::MAX && free_count <= geometry.cluster_count).then_some(free_count);
    let next_free = u32::from_le_bytes(sector[492..496].try_into().unwrap());
    let next_free = if (2..geometry.cluster_count + 2).contains(&next_free) {
        next_free
    } else {
        2
    };
    Ok((free_count, next_free))
}

fn validate_name(name: &str) -> Result<(), FsError> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.ends_with(' ')
        || name.ends_with('.')
        || name.encode_utf16().count() > 255
        || name.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    '"' | '*' | '/' | ':' | '<' | '>' | '?' | '\\' | '|'
                )
        })
    {
        return Err(FsError::InvalidPath);
    }
    Ok(())
}

fn generate_short_name(name: &str, records: &[DirectoryRecord]) -> Result<[u8; 11], FsError> {
    let (base, extension) = name.rsplit_once('.').unwrap_or((name, ""));
    let sanitize = |part: &str| {
        part.chars()
            .filter_map(|character| {
                if !character.is_ascii() {
                    return None;
                }
                let byte = character.to_ascii_uppercase() as u8;
                is_valid_short_name_byte(byte).then_some(byte)
            })
            .collect::<Vec<_>>()
    };
    let base = sanitize(base);
    let extension = sanitize(extension);
    let collides =
        |candidate: &[u8; 11]| records.iter().any(|record| record.short_name == *candidate);

    if !base.is_empty() && base.len() <= 8 && extension.len() <= 3 {
        let mut candidate = [b' '; 11];
        candidate[..base.len()].copy_from_slice(&base);
        candidate[8..8 + extension.len()].copy_from_slice(&extension);
        if !collides(&candidate) {
            return Ok(candidate);
        }
    }

    let base = if base.is_empty() {
        b"FILE".as_slice()
    } else {
        base.as_slice()
    };
    for suffix in 1..=999_999u32 {
        let suffix = format!("~{suffix}");
        let prefix_length = 8usize.saturating_sub(suffix.len());
        if prefix_length == 0 {
            continue;
        }
        let mut candidate = [b' '; 11];
        let base_length = base.len().min(prefix_length);
        candidate[..base_length].copy_from_slice(&base[..base_length]);
        candidate[base_length..base_length + suffix.len()].copy_from_slice(suffix.as_bytes());
        let extension_length = extension.len().min(3);
        candidate[8..8 + extension_length].copy_from_slice(&extension[..extension_length]);
        if !collides(&candidate) {
            return Ok(candidate);
        }
    }
    Err(FsError::NoSpace)
}

fn is_valid_short_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'$' | b'%'
                | b'\''
                | b'-'
                | b'_'
                | b'@'
                | b'~'
                | b'`'
                | b'!'
                | b'('
                | b')'
                | b'{'
                | b'}'
                | b'^'
                | b'#'
                | b'&'
        )
}

fn make_directory_entries(
    name: &str,
    short_name: &[u8; 11],
    attributes: u8,
    first_cluster: u32,
    size: u32,
) -> Result<Vec<[u8; 32]>, FsError> {
    validate_name(name)?;
    let mut units = Vec::new();
    units
        .try_reserve_exact(name.encode_utf16().count())
        .map_err(|_| FsError::NoSpace)?;
    units.extend(name.encode_utf16());
    let long_entry_count = units.len().div_ceil(13);
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(long_entry_count + 1)
        .map_err(|_| FsError::NoSpace)?;
    let checksum = short_name_checksum(short_name);
    const UTF16_OFFSETS: [usize; 13] = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];

    for sequence in (1..=long_entry_count).rev() {
        let mut entry = [0u8; 32];
        entry[0] = sequence as u8;
        if sequence == long_entry_count {
            entry[0] |= 0x40;
        }
        entry[11] = 0x0F;
        entry[13] = checksum;
        for (index, &field_offset) in UTF16_OFFSETS.iter().enumerate() {
            let unit_index = (sequence - 1) * 13 + index;
            let unit = if unit_index < units.len() {
                units[unit_index]
            } else if unit_index == units.len() {
                0
            } else {
                0xFFFF
            };
            entry[field_offset..field_offset + 2].copy_from_slice(&unit.to_le_bytes());
        }
        entries.push(entry);
    }

    let mut short_entry = [0u8; 32];
    short_entry[..11].copy_from_slice(short_name);
    short_entry[11] = attributes;
    write_cluster_fields(&mut short_entry, first_cluster, size);
    const FAT_EPOCH_DATE: u16 = (1 << 5) | 1;
    short_entry[16..18].copy_from_slice(&FAT_EPOCH_DATE.to_le_bytes());
    short_entry[18..20].copy_from_slice(&FAT_EPOCH_DATE.to_le_bytes());
    short_entry[24..26].copy_from_slice(&FAT_EPOCH_DATE.to_le_bytes());
    entries.push(short_entry);
    Ok(entries)
}

fn write_cluster_fields(entry: &mut [u8; 32], first_cluster: u32, size: u32) {
    entry[20..22].copy_from_slice(&((first_cluster >> 16) as u16).to_le_bytes());
    entry[26..28].copy_from_slice(&(first_cluster as u16).to_le_bytes());
    entry[28..32].copy_from_slice(&size.to_le_bytes());
}

fn mark_deleted(filesystem: &Fat32, location: EntryLocation) -> Result<(), FsError> {
    let mut sector = [0u8; SECTOR_SIZE];
    filesystem.read_sector(location.sector_lba, &mut sector)?;
    sector[location.offset] = 0xE5;
    filesystem.write_sector(location.sector_lba, &sector)
}

fn short_name(bytes: &[u8; 11]) -> String {
    let base_end = bytes[..8]
        .iter()
        .rposition(|byte| *byte != b' ')
        .map_or(0, |i| i + 1);
    let ext_end = bytes[8..]
        .iter()
        .rposition(|byte| *byte != b' ')
        .map_or(0, |i| i + 1);
    let mut name = String::new();
    for &byte in &bytes[..base_end] {
        name.push(if byte == 0x05 {
            '\u{00e5}'
        } else {
            byte as char
        });
    }
    if ext_end != 0 {
        name.push('.');
        for &byte in &bytes[8..8 + ext_end] {
            name.push(byte as char);
        }
    }
    name
}

fn short_name_checksum(name: &[u8; 11]) -> u8 {
    name.iter()
        .fold(0u8, |sum, byte| sum.rotate_right(1).wrapping_add(*byte))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::block::BlockError;
    use alloc::sync::Arc;
    use spin::Mutex;

    struct MemoryDisk {
        sectors: Mutex<Vec<[u8; SECTOR_SIZE]>>,
    }

    #[test]
    fn fat_permissions_mark_elf_files_executable() {
        let record = |name: &str, attributes| DirectoryRecord {
            name: String::from(name),
            short_name: [0; 11],
            attributes,
            first_cluster: 0,
            size: 0,
            short_entry: EntryLocation {
                sector_lba: 0,
                offset: 0,
            },
            long_entries: Vec::new(),
        };
        assert_eq!(record_permissions(&record("program.elf", 0x20)), 0o755);
        assert_eq!(record_permissions(&record("read-only.ELF", 0x21)), 0o555);
        assert_eq!(record_permissions(&record("notes.txt", 0x20)), 0o644);
    }

    #[test]
    fn formats_and_mounts_fat32_superfloppy() {
        let disk = Arc::new(MemoryDisk {
            sectors: Mutex::new(alloc::vec![[0u8; SECTOR_SIZE]; 131_072]),
        });

        Fat32::format(disk.as_ref()).unwrap();
        let filesystem = Fat32::mount(disk).unwrap();
        assert!(filesystem.read_dir("/").unwrap().is_empty());
    }

    #[test]
    fn rejects_too_small_device_before_writing() {
        let disk = Arc::new(MemoryDisk {
            sectors: Mutex::new(alloc::vec![[0u8; SECTOR_SIZE]; 2048]),
        });

        assert_eq!(Fat32::format(disk.as_ref()), Err(FsError::Unsupported));
        assert!(
            disk.sectors
                .lock()
                .iter()
                .all(|sector| sector.iter().all(|b| *b == 0))
        );
    }

    impl BlockDevice for MemoryDisk {
        fn name(&self) -> &str {
            "fat-test"
        }

        fn sector_count(&self) -> u64 {
            self.sectors.lock().len() as u64
        }

        fn read_sectors(&self, lba: u64, buffer: &mut [u8]) -> Result<(), BlockError> {
            if buffer.len() % SECTOR_SIZE != 0 {
                return Err(BlockError::InvalidBuffer);
            }
            let sectors = self.sectors.lock();
            let first = lba as usize;
            let count = buffer.len() / SECTOR_SIZE;
            if first
                .checked_add(count)
                .is_none_or(|end| end > sectors.len())
            {
                return Err(BlockError::OutOfRange);
            }
            for index in 0..count {
                buffer[index * SECTOR_SIZE..(index + 1) * SECTOR_SIZE]
                    .copy_from_slice(&sectors[first + index]);
            }
            Ok(())
        }

        fn write_sectors(&self, lba: u64, buffer: &[u8]) -> Result<(), BlockError> {
            if buffer.len() % SECTOR_SIZE != 0 {
                return Err(BlockError::InvalidBuffer);
            }
            let mut sectors = self.sectors.lock();
            let first = lba as usize;
            let count = buffer.len() / SECTOR_SIZE;
            if first
                .checked_add(count)
                .is_none_or(|end| end > sectors.len())
            {
                return Err(BlockError::OutOfRange);
            }
            for index in 0..count {
                sectors[first + index]
                    .copy_from_slice(&buffer[index * SECTOR_SIZE..(index + 1) * SECTOR_SIZE]);
            }
            Ok(())
        }
    }

    fn test_disk() -> Arc<MemoryDisk> {
        let mut sectors = alloc::vec![[0u8; SECTOR_SIZE]; 2048];
        let mbr = &mut sectors[0];
        mbr[446 + 4] = 0x0C;
        mbr[446 + 8..446 + 12].copy_from_slice(&1u32.to_le_bytes());
        mbr[446 + 12..446 + 16].copy_from_slice(&2047u32.to_le_bytes());
        mbr[510] = 0x55;
        mbr[511] = 0xAA;

        let boot = &mut sectors[1];
        boot[11..13].copy_from_slice(&(SECTOR_SIZE as u16).to_le_bytes());
        boot[13] = 1;
        boot[14..16].copy_from_slice(&1u16.to_le_bytes());
        boot[16] = 1;
        boot[32..36].copy_from_slice(&2047u32.to_le_bytes());
        boot[36..40].copy_from_slice(&16u32.to_le_bytes());
        boot[44..48].copy_from_slice(&2u32.to_le_bytes());
        boot[510] = 0x55;
        boot[511] = 0xAA;

        sectors[2][8..12].copy_from_slice(&0x0FFF_FFFFu32.to_le_bytes());
        sectors[2][12..16].copy_from_slice(&0x0FFF_FFFFu32.to_le_bytes());
        let root = &mut sectors[18];
        root[..11].copy_from_slice(b"HELLO   TXT");
        root[11] = 0x20;
        root[26..28].copy_from_slice(&3u16.to_le_bytes());
        root[28..32].copy_from_slice(&5u32.to_le_bytes());
        sectors[19][..5].copy_from_slice(b"hello");

        Arc::new(MemoryDisk {
            sectors: Mutex::new(sectors),
        })
    }

    fn test_disk_with_mirrored_fats_and_fsinfo() -> Arc<MemoryDisk> {
        let disk = test_disk();
        let mut sectors = disk.sectors.lock().clone();
        let root = sectors[18];
        let file = sectors[19];

        sectors[1][14..16].copy_from_slice(&2u16.to_le_bytes());
        sectors[1][16] = 2;
        sectors[1][48..50].copy_from_slice(&1u16.to_le_bytes());
        sectors[35] = root;
        sectors[36] = file;
        sectors[2] = [0; SECTOR_SIZE];

        for fat_start in [3usize, 19] {
            sectors[fat_start][8..12].copy_from_slice(&0xAFFF_FFFFu32.to_le_bytes());
            sectors[fat_start][12..16].copy_from_slice(&0xAFFF_FFFFu32.to_le_bytes());
            sectors[fat_start][16..20].copy_from_slice(&0xA000_0000u32.to_le_bytes());
            sectors[fat_start][20..24].copy_from_slice(&0xA000_0000u32.to_le_bytes());
        }

        let fsinfo = &mut sectors[2];
        fsinfo[0..4].copy_from_slice(&FSINFO_LEAD_SIGNATURE.to_le_bytes());
        fsinfo[484..488].copy_from_slice(&FSINFO_STRUCT_SIGNATURE.to_le_bytes());
        fsinfo[488..492].copy_from_slice(&2011u32.to_le_bytes());
        fsinfo[492..496].copy_from_slice(&4u32.to_le_bytes());
        fsinfo[508..512].copy_from_slice(&FSINFO_TRAIL_SIGNATURE.to_le_bytes());

        Arc::new(MemoryDisk {
            sectors: Mutex::new(sectors),
        })
    }

    fn test_superfloppy_with_mbr_like_boot_code() -> Arc<MemoryDisk> {
        let disk = test_disk();
        let mut sectors = disk.sectors.lock().clone();
        let boot = sectors[1];
        let fat = sectors[2];
        let root = sectors[18];
        let file = sectors[19];

        sectors[0] = boot;
        sectors[0][450] = 0x0C;
        sectors[0][454..458].copy_from_slice(&2048u32.to_le_bytes());
        sectors[0][458..462].copy_from_slice(&2047u32.to_le_bytes());
        sectors[1] = fat;
        sectors[17] = root;
        sectors[18] = file;

        Arc::new(MemoryDisk {
            sectors: Mutex::new(sectors),
        })
    }

    #[test]
    fn mounts_mbr_fat32_partition_and_reads_a_file() {
        let disk = test_disk();
        let fat = Fat32::mount(disk).unwrap();
        assert_eq!(
            fat.metadata("/hello.txt").unwrap().node_type,
            NodeType::File
        );
        let mut contents = [0; 8];
        assert_eq!(fat.read("/HELLO.TXT", 0, &mut contents), Ok(5));
        assert_eq!(&contents[..5], b"hello");
    }

    #[test]
    fn mounts_superfloppy_even_when_boot_code_looks_like_an_mbr_entry() {
        let fat = Fat32::mount(test_superfloppy_with_mbr_like_boot_code()).unwrap();
        let mut contents = [0; 8];
        assert_eq!(fat.read("/hello.txt", 0, &mut contents), Ok(5));
        assert_eq!(&contents[..5], b"hello");
    }

    #[test]
    fn writes_with_zero_filled_gap_and_truncates_file_chains() {
        let disk = test_disk();
        let mut fat = Fat32::mount(disk.clone()).unwrap();

        fat.create_file("/notes.txt").unwrap();
        assert_eq!(fat.write("/notes.txt", 0, b"hello"), Ok(5));
        assert_eq!(fat.write("/notes.txt", 8, b"world"), Ok(5));

        let mut contents = [0xFF; 16];
        assert_eq!(fat.read("/notes.txt", 0, &mut contents), Ok(13));
        assert_eq!(&contents[..13], b"hello\0\0\0world");

        fat.truncate("/notes.txt", 3).unwrap();
        fat.truncate("/notes.txt", 520).unwrap();
        let mut grown = [0xFF; 520];
        assert_eq!(fat.read("/notes.txt", 0, &mut grown), Ok(520));
        assert_eq!(&grown[..3], b"hel");
        assert!(grown[3..].iter().all(|byte| *byte == 0));

        fat.truncate("/notes.txt", 0).unwrap();
        assert_eq!(fat.metadata("/notes.txt").unwrap().len, 0);
        assert_eq!(fat.remove("/notes.txt"), Ok(()));
        assert_eq!(fat.read_dir("/").unwrap().len(), 1);

        let sectors = disk.sectors.lock();
        let fat_sector = &sectors[2];
        assert_eq!(
            u32::from_le_bytes(fat_sector[4 * 4..4 * 4 + 4].try_into().unwrap()) & 0x0FFF_FFFF,
            0
        );
        assert_eq!(
            u32::from_le_bytes(fat_sector[5 * 4..5 * 4 + 4].try_into().unwrap()) & 0x0FFF_FFFF,
            0
        );
    }

    #[test]
    fn creates_and_removes_nested_long_names() {
        let disk = test_disk();
        let mut fat = Fat32::mount(disk).unwrap();
        fat.create_dir("/Documents").unwrap();
        fat.create_file("/Documents/very long note.txt").unwrap();
        fat.write("/Documents/very long note.txt", 0, b"persisted")
            .unwrap();

        assert_eq!(
            fat.read_dir("/Documents").unwrap()[0].name,
            "very long note.txt"
        );
        assert_eq!(fat.remove("/Documents"), Err(FsError::DirectoryNotEmpty));
        fat.remove("/Documents/very long note.txt").unwrap();
        fat.remove("/Documents").unwrap();
        assert_eq!(fat.create_dir("/Documents"), Ok(()));
    }

    #[test]
    fn extends_directory_and_preserves_entries() {
        let disk = test_disk();
        let mut fat = Fat32::mount(disk).unwrap();
        for index in 0..10 {
            fat.create_file(&format!("/document number {index}.txt"))
                .unwrap();
        }

        let entries = fat.read_dir("/").unwrap();
        assert_eq!(entries.len(), 11);
        for index in 0..10 {
            assert!(
                entries
                    .iter()
                    .any(|entry| entry.name == format!("document number {index}.txt"))
            );
        }
    }

    #[test]
    fn updates_mirrored_fats_and_fsinfo_during_allocation_and_release() {
        let disk = test_disk_with_mirrored_fats_and_fsinfo();
        let mut fat = Fat32::mount(disk.clone()).unwrap();

        fat.create_file("/large.bin").unwrap();
        fat.write("/large.bin", 0, &[0x5A; 600]).unwrap();
        {
            let sectors = disk.sectors.lock();
            for fat_start in [3usize, 19] {
                for (cluster, expected) in [(4usize, 0xA000_0005), (5, 0xAFFF_FFFF)] {
                    let entry = u32::from_le_bytes(
                        sectors[fat_start][cluster * 4..cluster * 4 + 4]
                            .try_into()
                            .unwrap(),
                    );
                    assert_eq!(entry, expected);
                }
            }
            assert_eq!(
                u32::from_le_bytes(sectors[2][488..492].try_into().unwrap()),
                2009
            );
            assert_eq!(
                u32::from_le_bytes(sectors[2][492..496].try_into().unwrap()),
                6
            );
        }

        fat.truncate("/large.bin", 0).unwrap();
        let sectors = disk.sectors.lock();
        for fat_start in [3usize, 19] {
            for cluster in [4usize, 5] {
                let entry = u32::from_le_bytes(
                    sectors[fat_start][cluster * 4..cluster * 4 + 4]
                        .try_into()
                        .unwrap(),
                );
                assert_eq!(entry, 0xA000_0000);
            }
        }
        assert_eq!(
            u32::from_le_bytes(sectors[2][488..492].try_into().unwrap()),
            2011
        );
        assert_eq!(
            u32::from_le_bytes(sectors[2][492..496].try_into().unwrap()),
            4
        );
    }
}
