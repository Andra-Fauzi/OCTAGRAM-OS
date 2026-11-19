use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::fs::{DirectoryEntry, FileSystem, FsError, Metadata, NodeType, normalize_path};

use super::block::{BlockDevice, BlockDeviceRef, SECTOR_SIZE};

const EXT2_MAGIC: u16 = 0xEF53;
const EXT2_SUPERBLOCK_OFFSET: u64 = 1024;
const EXT2_SUPERBLOCK_SIZE: usize = 1024;
const EXT2_ROOT_INODE: u32 = 2;
const EXT2_GOOD_OLD_INODE_SIZE: usize = 128;
const EXT2_GROUP_DESC_SIZE: usize = 32;
const EXT2_INODE_POINTERS_OFFSET: usize = 40;
const EXT2_DIRECT_BLOCKS: usize = 12;
const EXT2_FILETYPE_FEATURE: u32 = 0x0002;
const EXT2_JOURNAL_FEATURE: u32 = 0x0004;
const EXT2_ALLOWED_INCOMPAT: u32 = EXT2_FILETYPE_FEATURE;
const EXT2_ALLOWED_RO_COMPAT: u32 = 0x0003;
const EXT2_INDEX_FL: u32 = 0x0000_1000;
const EXT2_FT_REG_FILE: u8 = 1;
const EXT2_FT_DIR: u8 = 2;
const EXT2_FT_SYMLINK: u8 = 7;
const MAX_GROUPS: u32 = 65_536;
const MAX_FILE_NAME: usize = 255;

struct Superblock {
    block_size: usize,
    blocks_count: u32,
    inodes_count: u32,
    first_data_block: u32,
    blocks_per_group: u32,
    inodes_per_group: u32,
    inode_size: usize,
    first_inode: u32,
    groups_count: u32,
    feature_compat: u32,
    feature_incompat: u32,
    feature_ro_compat: u32,
    reserved_gdt_blocks: u16,
    group_desc_block: u32,
}

struct Inode {
    bytes: Vec<u8>,
}

#[derive(Clone)]
struct DirectoryRecord {
    inode: u32,
    name: String,
    file_type: u8,
    block_index: usize,
    offset: usize,
    record_length: usize,
}

pub struct Ext2 {
    device: BlockDeviceRef,
    partition_start: u64,
    partition_sectors: u64,
    superblock: Superblock,
}

impl Ext2 {
    pub fn mount(device: BlockDeviceRef) -> Result<Self, FsError> {
        let (partition_start, partition_sectors, raw_superblock) =
            locate_superblock(device.as_ref())?;
        let superblock = parse_superblock(&raw_superblock)?;
        let sectors_per_block = (superblock.block_size / SECTOR_SIZE) as u64;
        let required_sectors = (superblock.blocks_count as u64)
            .checked_mul(sectors_per_block)
            .ok_or(FsError::InvalidData)?;
        if required_sectors > partition_sectors {
            return Err(FsError::InvalidData);
        }

        let filesystem = Self {
            device,
            partition_start,
            partition_sectors,
            superblock,
        };
        let root = filesystem.read_inode(EXT2_ROOT_INODE)?;
        if root.node_type()? != NodeType::Directory {
            return Err(FsError::InvalidData);
        }
        Ok(filesystem)
    }

    fn sectors_per_block(&self) -> u64 {
        (self.superblock.block_size / SECTOR_SIZE) as u64
    }

    fn read_block(&self, block: u32) -> Result<Vec<u8>, FsError> {
        if block >= self.superblock.blocks_count {
            return Err(FsError::IoError);
        }
        let lba = self
            .partition_start
            .checked_add(
                (block as u64)
                    .checked_mul(self.sectors_per_block())
                    .ok_or(FsError::IoError)?,
            )
            .ok_or(FsError::IoError)?;
        let end = lba
            .checked_add(self.sectors_per_block())
            .ok_or(FsError::IoError)?;
        let partition_end = self
            .partition_start
            .checked_add(self.partition_sectors)
            .ok_or(FsError::IoError)?;
        if end > partition_end {
            return Err(FsError::IoError);
        }
        let mut data = vec![0; self.superblock.block_size];
        self.device
            .read_sectors(lba, &mut data)
            .map_err(|_| FsError::IoError)?;
        Ok(data)
    }

    fn write_block(&self, block: u32, data: &[u8]) -> Result<(), FsError> {
        if block >= self.superblock.blocks_count || data.len() != self.superblock.block_size {
            return Err(FsError::IoError);
        }
        let lba = self
            .partition_start
            .checked_add(
                (block as u64)
                    .checked_mul(self.sectors_per_block())
                    .ok_or(FsError::IoError)?,
            )
            .ok_or(FsError::IoError)?;
        let end = lba
            .checked_add(self.sectors_per_block())
            .ok_or(FsError::IoError)?;
        let partition_end = self
            .partition_start
            .checked_add(self.partition_sectors)
            .ok_or(FsError::IoError)?;
        if end > partition_end {
            return Err(FsError::IoError);
        }
        self.device
            .write_sectors(lba, data)
            .map_err(|_| FsError::IoError)
    }

    fn group_descriptor(&self, group: u32) -> Result<Vec<u8>, FsError> {
        if group >= self.superblock.groups_count {
            return Err(FsError::IoError);
        }
        let byte_offset = group as usize * EXT2_GROUP_DESC_SIZE;
        let block =
            self.superblock.group_desc_block + (byte_offset / self.superblock.block_size) as u32;
        let offset = byte_offset % self.superblock.block_size;
        let data = self.read_block(block)?;
        Ok(data[offset..offset + EXT2_GROUP_DESC_SIZE].to_vec())
    }

    fn update_group_descriptor(
        &self,
        group: u32,
        offset: usize,
        value: u16,
    ) -> Result<(), FsError> {
        let byte_offset = group as usize * EXT2_GROUP_DESC_SIZE;
        let descriptor_block_index = (byte_offset / self.superblock.block_size) as u32;
        let offset_in_block = byte_offset % self.superblock.block_size + offset;
        for backup_group in 0..self.superblock.groups_count {
            if !self.has_backup_superblock(backup_group) {
                continue;
            }
            let group_start =
                self.superblock.first_data_block + backup_group * self.superblock.blocks_per_group;
            let block_number = group_start + 1 + descriptor_block_index;
            let mut data = self.read_block(block_number)?;
            data[offset_in_block..offset_in_block + 2].copy_from_slice(&value.to_le_bytes());
            self.write_block(block_number, &data)?;
        }
        Ok(())
    }

    fn update_superblock_counter(&self, offset: usize, delta: i64) -> Result<(), FsError> {
        let offset_in_block =
            (EXT2_SUPERBLOCK_OFFSET % self.superblock.block_size as u64) as usize + offset;
        for backup_group in 0..self.superblock.groups_count {
            if !self.has_backup_superblock(backup_group) {
                continue;
            }
            let block_number =
                self.superblock.first_data_block + backup_group * self.superblock.blocks_per_group;
            let mut data = self.read_block(block_number)?;
            let old = read_u32(&data, offset_in_block)?;
            let updated = if delta < 0 {
                old.checked_sub(delta.unsigned_abs() as u32)
            } else {
                old.checked_add(delta as u32)
            }
            .ok_or(FsError::InvalidData)?;
            data[offset_in_block..offset_in_block + 4].copy_from_slice(&updated.to_le_bytes());
            self.write_block(block_number, &data)?;
        }
        Ok(())
    }

    fn has_backup_superblock(&self, group: u32) -> bool {
        if self.superblock.feature_ro_compat & 0x1 == 0 {
            return true;
        }
        group == 0 || group == 1 || [3, 5, 7].iter().any(|base| is_power_of(group, *base))
    }

    fn read_inode(&self, inode_number: u32) -> Result<Inode, FsError> {
        if inode_number == 0 || inode_number > self.superblock.inodes_count {
            return Err(FsError::NotFound);
        }
        let group = (inode_number - 1) / self.superblock.inodes_per_group;
        let index = (inode_number - 1) % self.superblock.inodes_per_group;
        let descriptor = self.group_descriptor(group)?;
        let table_block = read_u32(&descriptor, 8)?;
        let byte_offset = index as usize * self.superblock.inode_size;
        let block_number = table_block
            .checked_add((byte_offset / self.superblock.block_size) as u32)
            .ok_or(FsError::InvalidData)?;
        let offset = byte_offset % self.superblock.block_size;
        let data = self.read_block(block_number)?;
        Ok(Inode {
            bytes: data[offset..offset + self.superblock.inode_size].to_vec(),
        })
    }

    fn write_inode(&self, inode_number: u32, inode: &Inode) -> Result<(), FsError> {
        if inode_number == 0
            || inode_number > self.superblock.inodes_count
            || inode.bytes.len() != self.superblock.inode_size
        {
            return Err(FsError::InvalidData);
        }
        let group = (inode_number - 1) / self.superblock.inodes_per_group;
        let index = (inode_number - 1) % self.superblock.inodes_per_group;
        let descriptor = self.group_descriptor(group)?;
        let table_block = read_u32(&descriptor, 8)?;
        let byte_offset = index as usize * self.superblock.inode_size;
        let block_number = table_block
            .checked_add((byte_offset / self.superblock.block_size) as u32)
            .ok_or(FsError::InvalidData)?;
        let offset = byte_offset % self.superblock.block_size;
        let mut data = self.read_block(block_number)?;
        data[offset..offset + self.superblock.inode_size].copy_from_slice(&inode.bytes);
        self.write_block(block_number, &data)
    }

    fn block_at(&self, inode: &Inode, logical_block: usize) -> Result<Option<u32>, FsError> {
        if logical_block < EXT2_DIRECT_BLOCKS {
            let block = inode.block_pointer(logical_block)?;
            return self.validate_optional_block(block);
        }

        let entries = self.superblock.block_size / 4;
        let mut index = logical_block - EXT2_DIRECT_BLOCKS;
        if index < entries {
            return self.indirect_block(inode.block_pointer(12)?, 1, &[index]);
        }
        index -= entries;
        let double_count = entries.checked_mul(entries).ok_or(FsError::NoSpace)?;
        if index < double_count {
            return self.indirect_block(
                inode.block_pointer(13)?,
                2,
                &[index / entries, index % entries],
            );
        }
        index -= double_count;
        let triple_count = double_count.checked_mul(entries).ok_or(FsError::NoSpace)?;
        if index >= triple_count {
            return Err(FsError::NoSpace);
        }
        self.indirect_block(
            inode.block_pointer(14)?,
            3,
            &[
                index / double_count,
                (index / entries) % entries,
                index % entries,
            ],
        )
    }

    fn indirect_block(
        &self,
        mut block: u32,
        levels: usize,
        indices: &[usize],
    ) -> Result<Option<u32>, FsError> {
        if block == 0 {
            return Ok(None);
        }
        self.validate_data_block(block)?;
        for index in indices.iter().take(levels) {
            let data = self.read_block(block)?;
            block = read_u32(&data, index * 4)?;
            if block == 0 {
                return Ok(None);
            }
            self.validate_data_block(block)?;
        }
        Ok(Some(block))
    }

    fn validate_optional_block(&self, block: u32) -> Result<Option<u32>, FsError> {
        if block == 0 {
            Ok(None)
        } else if block < self.superblock.blocks_count {
            self.validate_data_block(block)?;
            Ok(Some(block))
        } else {
            Err(FsError::IoError)
        }
    }

    fn validate_data_block(&self, block: u32) -> Result<(), FsError> {
        if block < self.superblock.first_data_block || block >= self.superblock.blocks_count {
            return Err(FsError::InvalidData);
        }
        let relative = block - self.superblock.first_data_block;
        let group = relative / self.superblock.blocks_per_group;
        let bit = (relative % self.superblock.blocks_per_group) as usize;
        if self.is_metadata_block(block, group)? {
            return Err(FsError::InvalidData);
        }
        let descriptor = self.group_descriptor(group)?;
        let bitmap_block = read_u32(&descriptor, 0)?;
        let bitmap = self.read_block(bitmap_block)?;
        if bitmap[bit / 8] & (1 << (bit % 8)) == 0 {
            return Err(FsError::InvalidData);
        }
        Ok(())
    }

    fn is_metadata_block(&self, block: u32, group: u32) -> Result<bool, FsError> {
        let group_start =
            self.superblock.first_data_block + group * self.superblock.blocks_per_group;
        if self.has_backup_superblock(group) {
            let descriptor_blocks = (self.superblock.groups_count as usize * EXT2_GROUP_DESC_SIZE)
                .div_ceil(self.superblock.block_size) as u32;
            let reserved_gdt = if self.superblock.feature_compat & 0x10 != 0 {
                self.superblock.reserved_gdt_blocks as u32
            } else {
                0
            };
            let metadata_end = group_start
                .checked_add(1 + descriptor_blocks + reserved_gdt)
                .ok_or(FsError::InvalidData)?;
            if (group_start..metadata_end).contains(&block) {
                return Ok(true);
            }
        }

        let descriptor = self.group_descriptor(group)?;
        let block_bitmap = read_u32(&descriptor, 0)?;
        let inode_bitmap = read_u32(&descriptor, 4)?;
        let inode_table = read_u32(&descriptor, 8)?;
        let inode_table_blocks = (self.superblock.inodes_per_group as usize
            * self.superblock.inode_size)
            .div_ceil(self.superblock.block_size) as u32;
        Ok(block == block_bitmap
            || block == inode_bitmap
            || (inode_table..inode_table.saturating_add(inode_table_blocks)).contains(&block))
    }

    fn ensure_data_block(
        &mut self,
        inode: &mut Inode,
        logical_block: usize,
    ) -> Result<u32, FsError> {
        if logical_block < EXT2_DIRECT_BLOCKS {
            let current = inode.block_pointer(logical_block)?;
            if current != 0 {
                return self
                    .validate_optional_block(current)?
                    .ok_or(FsError::IoError);
            }
            let block = self.allocate_block()?;
            inode.set_block_pointer(logical_block, block)?;
            inode.add_sector_count(self.sectors_per_block() as u32)?;
            return Ok(block);
        }

        let entries = self.superblock.block_size / 4;
        let mut index = logical_block - EXT2_DIRECT_BLOCKS;
        let (root_slot, indices): (usize, Vec<usize>) = if index < entries {
            (12, vec![index])
        } else {
            index -= entries;
            let double_count = entries.checked_mul(entries).ok_or(FsError::NoSpace)?;
            if index < double_count {
                (13, vec![index / entries, index % entries])
            } else {
                index -= double_count;
                let triple_count = double_count.checked_mul(entries).ok_or(FsError::NoSpace)?;
                if index >= triple_count {
                    return Err(FsError::NoSpace);
                }
                (
                    14,
                    vec![
                        index / double_count,
                        (index / entries) % entries,
                        index % entries,
                    ],
                )
            }
        };

        let mut block = inode.block_pointer(root_slot)?;
        if block == 0 {
            block = self.allocate_block()?;
            inode.set_block_pointer(root_slot, block)?;
            inode.add_sector_count(self.sectors_per_block() as u32)?;
        }

        for (depth, index) in indices.iter().enumerate() {
            self.validate_data_block(block)?;
            let mut indirect = self.read_block(block)?;
            let mut child = read_u32(&indirect, index * 4)?;
            if child != 0 {
                self.validate_data_block(child)?;
            }
            if child == 0 {
                child = self.allocate_block()?;
                inode.add_sector_count(self.sectors_per_block() as u32)?;
                indirect[index * 4..index * 4 + 4].copy_from_slice(&child.to_le_bytes());
                self.write_block(block, &indirect)?;
            }
            if depth + 1 == indices.len() {
                return Ok(child);
            }
            block = child;
        }
        Err(FsError::IoError)
    }

    fn allocate_block(&self) -> Result<u32, FsError> {
        for group in 0..self.superblock.groups_count {
            let descriptor = self.group_descriptor(group)?;
            let free_count = read_u16(&descriptor, 12)?;
            if free_count == 0 {
                continue;
            }
            let bitmap_block = read_u32(&descriptor, 0)?;
            let mut bitmap = self.read_block(bitmap_block)?;
            let first_block =
                self.superblock.first_data_block + group * self.superblock.blocks_per_group;
            let valid_blocks = self
                .superblock
                .blocks_count
                .saturating_sub(first_block)
                .min(self.superblock.blocks_per_group) as usize;
            for bit in 0..valid_blocks {
                let byte_index = bit / 8;
                let bit_mask = 1 << (bit % 8);
                if bitmap[byte_index] & bit_mask != 0 {
                    continue;
                }
                let block_number = first_block + bit as u32;
                if block_number < self.superblock.first_data_block {
                    continue;
                }
                bitmap[byte_index] |= bit_mask;
                self.write_block(bitmap_block, &bitmap)?;
                self.update_group_descriptor(group, 12, free_count - 1)?;
                self.update_superblock_counter(12, -1)?;
                let zeroes = vec![0; self.superblock.block_size];
                if let Err(error) = self.write_block(block_number, &zeroes) {
                    self.free_block(block_number)?;
                    return Err(error);
                }
                return Ok(block_number);
            }
        }
        Err(FsError::NoSpace)
    }

    fn free_block(&self, block_number: u32) -> Result<(), FsError> {
        self.validate_data_block(block_number)?;
        let relative = block_number - self.superblock.first_data_block;
        let group = relative / self.superblock.blocks_per_group;
        let bit = (relative % self.superblock.blocks_per_group) as usize;
        let descriptor = self.group_descriptor(group)?;
        let bitmap_block = read_u32(&descriptor, 0)?;
        let mut bitmap = self.read_block(bitmap_block)?;
        let byte_index = bit / 8;
        let mask = 1 << (bit % 8);
        if bitmap[byte_index] & mask == 0 {
            return Err(FsError::InvalidData);
        }
        bitmap[byte_index] &= !mask;
        self.write_block(bitmap_block, &bitmap)?;
        let free_count = read_u16(&descriptor, 12)?
            .checked_add(1)
            .ok_or(FsError::InvalidData)?;
        self.update_group_descriptor(group, 12, free_count)?;
        self.update_superblock_counter(12, 1)
    }

    fn allocate_inode(&self, is_directory: bool) -> Result<u32, FsError> {
        for group in 0..self.superblock.groups_count {
            let descriptor = self.group_descriptor(group)?;
            let free_count = read_u16(&descriptor, 14)?;
            if free_count == 0 {
                continue;
            }
            let bitmap_block = read_u32(&descriptor, 4)?;
            let mut bitmap = self.read_block(bitmap_block)?;
            let first_inode = group * self.superblock.inodes_per_group + 1;
            let valid_inodes = self
                .superblock
                .inodes_count
                .saturating_sub(first_inode - 1)
                .min(self.superblock.inodes_per_group) as usize;
            for bit in 0..valid_inodes {
                let inode_number = first_inode + bit as u32;
                if inode_number < self.superblock.first_inode {
                    continue;
                }
                let byte_index = bit / 8;
                let mask = 1 << (bit % 8);
                if bitmap[byte_index] & mask != 0 {
                    continue;
                }
                bitmap[byte_index] |= mask;
                self.write_block(bitmap_block, &bitmap)?;
                self.update_group_descriptor(group, 14, free_count - 1)?;
                self.update_superblock_counter(16, -1)?;
                if is_directory {
                    let used_dirs = read_u16(&descriptor, 16)?
                        .checked_add(1)
                        .ok_or(FsError::NoSpace)?;
                    self.update_group_descriptor(group, 16, used_dirs)?;
                }
                return Ok(inode_number);
            }
        }
        Err(FsError::NoSpace)
    }

    fn free_inode(&self, inode_number: u32, was_directory: bool) -> Result<(), FsError> {
        if inode_number < self.superblock.first_inode || inode_number > self.superblock.inodes_count
        {
            return Err(FsError::InvalidData);
        }
        let group = (inode_number - 1) / self.superblock.inodes_per_group;
        let bit = ((inode_number - 1) % self.superblock.inodes_per_group) as usize;
        let descriptor = self.group_descriptor(group)?;
        let bitmap_block = read_u32(&descriptor, 4)?;
        let mut bitmap = self.read_block(bitmap_block)?;
        let byte_index = bit / 8;
        let mask = 1 << (bit % 8);
        if bitmap[byte_index] & mask == 0 {
            return Err(FsError::InvalidData);
        }
        bitmap[byte_index] &= !mask;
        self.write_block(bitmap_block, &bitmap)?;
        let free_count = read_u16(&descriptor, 14)?
            .checked_add(1)
            .ok_or(FsError::InvalidData)?;
        self.update_group_descriptor(group, 14, free_count)?;
        self.update_superblock_counter(16, 1)?;
        if was_directory {
            let used_dirs = read_u16(&descriptor, 16)?
                .checked_sub(1)
                .ok_or(FsError::InvalidData)?;
            self.update_group_descriptor(group, 16, used_dirs)?;
        }
        Ok(())
    }

    fn adjust_inode_sectors(&self, inode: &mut Inode, delta: i64) -> Result<(), FsError> {
        let current = inode.sector_count();
        let updated = if delta < 0 {
            current.checked_sub(delta.unsigned_abs() as u32)
        } else {
            current.checked_add(delta as u32)
        }
        .ok_or(FsError::InvalidData)?;
        inode.set_sector_count(updated);
        Ok(())
    }

    fn read_file_range(
        &self,
        inode: &Inode,
        offset: usize,
        buffer: &mut [u8],
    ) -> Result<usize, FsError> {
        let size = usize::try_from(inode.size(self.superblock.feature_ro_compat)?)
            .map_err(|_| FsError::InvalidData)?;
        if offset >= size || buffer.is_empty() {
            return Ok(0);
        }
        let count = buffer.len().min(size - offset);
        let mut copied = 0;
        while copied < count {
            let position = offset.checked_add(copied).ok_or(FsError::InvalidData)?;
            let logical_block = position / self.superblock.block_size;
            let block_offset = position % self.superblock.block_size;
            let copy_length = (self.superblock.block_size - block_offset).min(count - copied);
            match self.block_at(inode, logical_block)? {
                Some(block) => {
                    let data = self.read_block(block)?;
                    buffer[copied..copied + copy_length]
                        .copy_from_slice(&data[block_offset..block_offset + copy_length]);
                }
                None => buffer[copied..copied + copy_length].fill(0),
            }
            copied += copy_length;
        }
        Ok(copied)
    }

    fn write_file_range(
        &mut self,
        inode: &mut Inode,
        offset: usize,
        bytes: &[u8],
    ) -> Result<(), FsError> {
        let mut written = 0;
        while written < bytes.len() {
            let position = offset.checked_add(written).ok_or(FsError::NoSpace)?;
            let logical_block = position / self.superblock.block_size;
            let block_offset = position % self.superblock.block_size;
            let count = (self.superblock.block_size - block_offset).min(bytes.len() - written);
            let block = self.ensure_data_block(inode, logical_block)?;
            let mut data = self.read_block(block)?;
            data[block_offset..block_offset + count]
                .copy_from_slice(&bytes[written..written + count]);
            self.write_block(block, &data)?;
            written += count;
        }
        Ok(())
    }

    fn directory_records(&self, inode: &Inode) -> Result<Vec<DirectoryRecord>, FsError> {
        if inode.node_type()? != NodeType::Directory {
            return Err(FsError::NotDirectory);
        }
        let size = usize::try_from(inode.size(self.superblock.feature_ro_compat)?)
            .map_err(|_| FsError::InvalidData)?;
        let block_count = size.div_ceil(self.superblock.block_size);
        if block_count > self.superblock.blocks_count as usize {
            return Err(FsError::InvalidData);
        }
        let mut records = Vec::new();
        for block_index in 0..block_count {
            let Some(block_number) = self.block_at(inode, block_index)? else {
                continue;
            };
            let data = self.read_block(block_number)?;
            let mut offset = 0;
            while offset < self.superblock.block_size {
                if offset + 8 > data.len() {
                    return Err(FsError::InvalidData);
                }
                let inode_number = read_u32(&data, offset)?;
                let record_length = read_u16(&data, offset + 4)? as usize;
                let name_length = if self.superblock.feature_incompat & EXT2_FILETYPE_FEATURE != 0 {
                    data[offset + 6] as usize
                } else {
                    read_u16(&data, offset + 6)? as usize
                };
                let file_type = if self.superblock.feature_incompat & EXT2_FILETYPE_FEATURE != 0 {
                    data[offset + 7]
                } else {
                    0
                };
                if record_length < 8
                    || record_length % 4 != 0
                    || offset + record_length > data.len()
                    || name_length > record_length - 8
                {
                    return Err(FsError::InvalidData);
                }
                if inode_number != 0 && name_length != 0 {
                    let name_bytes = &data[offset + 8..offset + 8 + name_length];
                    if name_bytes.contains(&0) || name_bytes.contains(&b'/') {
                        return Err(FsError::InvalidData);
                    }
                    let name =
                        String::from_utf8(name_bytes.to_vec()).map_err(|_| FsError::InvalidData)?;
                    records.push(DirectoryRecord {
                        inode: inode_number,
                        name,
                        file_type,
                        block_index,
                        offset,
                        record_length,
                    });
                }
                offset += record_length;
            }
        }
        Ok(records)
    }

    fn lookup_inode(&self, path: &str) -> Result<(u32, Inode), FsError> {
        let path = normalize_path(path)?;
        let mut inode_number = EXT2_ROOT_INODE;
        let mut inode = self.read_inode(inode_number)?;
        for component in path.split('/').filter(|part| !part.is_empty()) {
            if inode.node_type()? != NodeType::Directory {
                return Err(FsError::NotDirectory);
            }
            let record = self
                .directory_records(&inode)?
                .into_iter()
                .find(|record| record.name == component)
                .ok_or(FsError::NotFound)?;
            inode_number = record.inode;
            inode = self.read_inode(inode_number)?;
        }
        Ok((inode_number, inode))
    }

    fn parent_and_name(&self, path: &str) -> Result<(u32, Inode, String), FsError> {
        let path = normalize_path(path)?;
        if path == "/" {
            return Err(FsError::InvalidPath);
        }
        let split = path.rfind('/').ok_or(FsError::InvalidPath)?;
        let parent_path = if split == 0 { "/" } else { &path[..split] };
        let name = &path[split + 1..];
        validate_name(name)?;
        let (parent_number, parent_inode) = self.lookup_inode(parent_path)?;
        if parent_inode.node_type()? != NodeType::Directory {
            return Err(FsError::NotDirectory);
        }
        Ok((parent_number, parent_inode, name.to_string()))
    }

    fn insert_directory_entry(
        &mut self,
        parent_number: u32,
        name: &str,
        child_number: u32,
        file_type: u8,
    ) -> Result<(), FsError> {
        validate_name(name)?;
        let mut parent = self.read_inode(parent_number)?;
        if parent.node_type()? != NodeType::Directory {
            return Err(FsError::NotDirectory);
        }
        if self
            .directory_records(&parent)?
            .iter()
            .any(|record| record.name == name)
        {
            return Err(FsError::AlreadyExists);
        }

        if parent.flags() & EXT2_INDEX_FL != 0 {
            parent.set_flags(parent.flags() & !EXT2_INDEX_FL);
            self.write_inode(parent_number, &parent)?;
        }

        let needed = directory_record_size(name.len());
        let block_count = usize::try_from(parent.size(self.superblock.feature_ro_compat)?)
            .map_err(|_| FsError::InvalidData)?
            .div_ceil(self.superblock.block_size);
        for block_index in 0..block_count {
            let Some(block_number) = self.block_at(&parent, block_index)? else {
                continue;
            };
            let mut data = self.read_block(block_number)?;
            let mut offset = 0;
            while offset < self.superblock.block_size {
                let current_inode = read_u32(&data, offset)?;
                let record_length = read_u16(&data, offset + 4)? as usize;
                let name_length = if self.superblock.feature_incompat & EXT2_FILETYPE_FEATURE != 0 {
                    data[offset + 6] as usize
                } else {
                    read_u16(&data, offset + 6)? as usize
                };
                if record_length < 8
                    || record_length % 4 != 0
                    || offset + record_length > self.superblock.block_size
                    || name_length > record_length - 8
                {
                    return Err(FsError::InvalidData);
                }
                if current_inode == 0 && record_length >= needed {
                    let used_length = if record_length - needed >= 8 {
                        needed
                    } else {
                        record_length
                    };
                    write_directory_record(
                        &mut data,
                        offset,
                        used_length,
                        child_number,
                        name,
                        file_type,
                        self.superblock.feature_incompat & EXT2_FILETYPE_FEATURE != 0,
                    )?;
                    if used_length < record_length {
                        write_free_directory_record(
                            &mut data,
                            offset + used_length,
                            record_length - used_length,
                        )?;
                    }
                    return self.write_block(block_number, &data);
                }
                if current_inode != 0 {
                    let used_length = directory_record_size(name_length);
                    if record_length >= used_length + needed {
                        write_u16(&mut data, offset + 4, used_length as u16)?;
                        write_directory_record(
                            &mut data,
                            offset + used_length,
                            record_length - used_length,
                            child_number,
                            name,
                            file_type,
                            self.superblock.feature_incompat & EXT2_FILETYPE_FEATURE != 0,
                        )?;
                        return self.write_block(block_number, &data);
                    }
                }
                offset += record_length;
            }
        }

        let size = usize::try_from(parent.size(self.superblock.feature_ro_compat)?)
            .map_err(|_| FsError::InvalidData)?;
        if size % self.superblock.block_size != 0 {
            return Err(FsError::InvalidData);
        }
        let new_block_index = size / self.superblock.block_size;
        let block_number = self.ensure_data_block(&mut parent, new_block_index)?;
        let mut data = vec![0; self.superblock.block_size];
        write_directory_record(
            &mut data,
            0,
            self.superblock.block_size,
            child_number,
            name,
            file_type,
            self.superblock.feature_incompat & EXT2_FILETYPE_FEATURE != 0,
        )?;
        self.write_block(block_number, &data)?;
        parent.set_size(
            (size + self.superblock.block_size) as u64,
            self.superblock.feature_ro_compat,
        )?;
        self.write_inode(parent_number, &parent)
    }

    fn remove_directory_entry(&self, parent: &Inode, name: &str) -> Result<u32, FsError> {
        for record in self.directory_records(parent)? {
            if record.name != name {
                continue;
            }
            let block_number = self
                .block_at(parent, record.block_index)?
                .ok_or(FsError::InvalidData)?;
            let mut data = self.read_block(block_number)?;
            if record.offset != 0 {
                let mut previous_offset = 0;
                let mut offset = 0;
                while offset < record.offset {
                    previous_offset = offset;
                    offset += read_u16(&data, offset + 4)? as usize;
                }
                if offset != record.offset {
                    return Err(FsError::InvalidData);
                }
                let previous_length = read_u16(&data, previous_offset + 4)? as usize;
                let merged = previous_length
                    .checked_add(record.record_length)
                    .ok_or(FsError::InvalidData)?;
                write_u16(&mut data, previous_offset + 4, merged as u16)?;
            } else {
                write_u32(&mut data, record.offset, 0)?;
                data[record.offset + 6] = 0;
                data[record.offset + 7] = 0;
            }
            self.write_block(block_number, &data)?;
            return Ok(record.inode);
        }
        Err(FsError::NotFound)
    }

    fn write_zero_range(
        &mut self,
        inode: &mut Inode,
        mut offset: usize,
        mut length: usize,
    ) -> Result<(), FsError> {
        let zeroes = [0u8; 4096];
        while length > 0 {
            let count = length.min(zeroes.len());
            self.write_file_range(inode, offset, &zeroes[..count])?;
            offset = offset.checked_add(count).ok_or(FsError::NoSpace)?;
            length -= count;
        }
        Ok(())
    }

    fn clear_data_block(&mut self, inode: &mut Inode, logical_block: usize) -> Result<(), FsError> {
        if logical_block < EXT2_DIRECT_BLOCKS {
            let block = inode.block_pointer(logical_block)?;
            if block != 0 {
                inode.set_block_pointer(logical_block, 0)?;
                self.free_block(block)?;
                self.adjust_inode_sectors(inode, -(self.sectors_per_block() as i64))?;
            }
            return Ok(());
        }

        let entries = self.superblock.block_size / 4;
        let mut index = logical_block - EXT2_DIRECT_BLOCKS;
        let (root_slot, indices): (usize, Vec<usize>) = if index < entries {
            (12, vec![index])
        } else {
            index -= entries;
            let double_count = entries.checked_mul(entries).ok_or(FsError::NoSpace)?;
            if index < double_count {
                (13, vec![index / entries, index % entries])
            } else {
                index -= double_count;
                let triple_count = double_count.checked_mul(entries).ok_or(FsError::NoSpace)?;
                if index >= triple_count {
                    return Err(FsError::NoSpace);
                }
                (
                    14,
                    vec![
                        index / double_count,
                        (index / entries) % entries,
                        index % entries,
                    ],
                )
            }
        };
        let root = inode.block_pointer(root_slot)?;
        if root == 0 {
            return Ok(());
        }
        self.validate_data_block(root)?;
        if self.clear_indirect_block(inode, root, indices.len(), &indices)? {
            self.adjust_inode_sectors(inode, -(self.sectors_per_block() as i64))?;
            self.free_block(root)?;
            inode.set_block_pointer(root_slot, 0)?;
        }
        Ok(())
    }

    fn clear_indirect_block(
        &mut self,
        inode: &mut Inode,
        block: u32,
        levels: usize,
        indices: &[usize],
    ) -> Result<bool, FsError> {
        self.validate_data_block(block)?;
        let mut data = self.read_block(block)?;
        let index = indices[0];
        let child = read_u32(&data, index * 4)?;
        if child != 0 {
            self.validate_data_block(child)?;
            if levels == 1 {
                self.free_block(child)?;
                self.adjust_inode_sectors(inode, -(self.sectors_per_block() as i64))?;
                write_u32(&mut data, index * 4, 0)?;
            } else if self.clear_indirect_block(inode, child, levels - 1, &indices[1..])? {
                self.adjust_inode_sectors(inode, -(self.sectors_per_block() as i64))?;
                write_u32(&mut data, index * 4, 0)?;
                self.free_block(child)?;
            }
            self.write_block(block, &data)?;
        }
        let empty = data
            .chunks_exact(4)
            .all(|entry| entry.iter().all(|byte| *byte == 0));
        Ok(empty)
    }

    fn free_inode_storage(&self, inode: &Inode) -> Result<(), FsError> {
        let xattr_block = read_u32(&inode.bytes, 104)?;
        if xattr_block != 0 {
            self.validate_data_block(xattr_block)?;
            let mut xattr = self.read_block(xattr_block)?;
            if read_u32(&xattr, 0)? != 0xEA02_0000 {
                return Err(FsError::InvalidData);
            }
            let references = read_u32(&xattr, 4)?;
            if references == 0 {
                return Err(FsError::InvalidData);
            }
            if references == 1 {
                self.free_block(xattr_block)?;
            } else {
                write_u32(&mut xattr, 4, references - 1)?;
                self.write_block(xattr_block, &xattr)?;
            }
        }
        if inode.node_type()? == NodeType::Symlink
            && inode.size(self.superblock.feature_ro_compat)? <= 60
            && inode.sector_count() == 0
        {
            return Ok(());
        }
        for index in 0..EXT2_DIRECT_BLOCKS {
            let block = inode.block_pointer(index)?;
            if block != 0 {
                self.free_block(block)?;
            }
        }
        for (index, levels) in [(12, 1), (13, 2), (14, 3)] {
            let block = inode.block_pointer(index)?;
            if block != 0 {
                self.free_indirect_tree(block, levels)?;
            }
        }
        Ok(())
    }

    fn free_indirect_tree(&self, block: u32, levels: usize) -> Result<(), FsError> {
        self.validate_data_block(block)?;
        let data = self.read_block(block)?;
        for index in 0..self.superblock.block_size / 4 {
            let child = read_u32(&data, index * 4)?;
            if child != 0 {
                self.validate_data_block(child)?;
                if levels == 1 {
                    self.free_block(child)?;
                } else {
                    self.free_indirect_tree(child, levels - 1)?;
                }
            }
        }
        self.free_block(block)
    }

    fn directory_is_empty(&self, inode: &Inode) -> Result<bool, FsError> {
        Ok(self
            .directory_records(inode)?
            .iter()
            .all(|record| record.name == "." || record.name == ".."))
    }
}

impl FileSystem for Ext2 {
    fn name(&self) -> &'static str {
        "ext2"
    }
    fn metadata(&self, path: &str) -> Result<Metadata, FsError> {
        let (_, inode) = self.lookup_inode(path)?;
        Ok(Metadata {
            node_type: inode.node_type()?,
            len: usize::try_from(inode.size(self.superblock.feature_ro_compat)?)
                .map_err(|_| FsError::InvalidData)?,
            permissions: inode.mode() & 0o777,
        })
    }

    fn read_link(&self, path: &str) -> Result<String, FsError> {
        let (_, inode) = self.lookup_inode(path)?;
        if inode.node_type()? != NodeType::Symlink {
            return Err(FsError::Unsupported);
        }
        let size = usize::try_from(inode.size(self.superblock.feature_ro_compat)?)
            .map_err(|_| FsError::InvalidData)?;
        let mut bytes = vec![0; size];
        if size <= 60 && inode.sector_count() == 0 {
            bytes.copy_from_slice(
                inode
                    .bytes
                    .get(EXT2_INODE_POINTERS_OFFSET..EXT2_INODE_POINTERS_OFFSET + size)
                    .ok_or(FsError::InvalidData)?,
            );
        } else {
            let count = self.read_file_range(&inode, 0, &mut bytes)?;
            if count != size {
                return Err(FsError::IoError);
            }
        }
        String::from_utf8(bytes).map_err(|_| FsError::InvalidData)
    }

    fn read(&self, path: &str, offset: usize, buffer: &mut [u8]) -> Result<usize, FsError> {
        let (_, inode) = self.lookup_inode(path)?;
        match inode.node_type()? {
            NodeType::Directory => Err(FsError::IsDirectory),
            NodeType::File => self.read_file_range(&inode, offset, buffer),
            NodeType::Symlink => Err(FsError::Unsupported),
        }
    }

    fn write(&mut self, path: &str, offset: usize, data: &[u8]) -> Result<usize, FsError> {
        let (inode_number, mut inode) = self.lookup_inode(path)?;
        if inode.node_type()? != NodeType::File {
            return Err(if inode.node_type()? == NodeType::Directory {
                FsError::IsDirectory
            } else {
                FsError::Unsupported
            });
        }
        if data.is_empty() {
            return Ok(0);
        }
        let old_size = usize::try_from(inode.size(self.superblock.feature_ro_compat)?)
            .map_err(|_| FsError::NoSpace)?;
        let end = offset.checked_add(data.len()).ok_or(FsError::NoSpace)?;
        if offset > old_size {
            self.write_zero_range(&mut inode, old_size, offset - old_size)?;
        }
        self.write_file_range(&mut inode, offset, data)?;
        if end > old_size {
            inode.set_size(end as u64, self.superblock.feature_ro_compat)?;
        }
        self.write_inode(inode_number, &inode)?;
        Ok(data.len())
    }

    fn truncate(&mut self, path: &str, length: usize) -> Result<(), FsError> {
        let (inode_number, mut inode) = self.lookup_inode(path)?;
        if inode.node_type()? != NodeType::File {
            return Err(if inode.node_type()? == NodeType::Directory {
                FsError::IsDirectory
            } else {
                FsError::Unsupported
            });
        }
        let old_size = usize::try_from(inode.size(self.superblock.feature_ro_compat)?)
            .map_err(|_| FsError::NoSpace)?;
        if length > old_size {
            self.write_zero_range(&mut inode, old_size, length - old_size)?;
        } else if length < old_size {
            let first_free_block = length.div_ceil(self.superblock.block_size);
            let old_block_count = old_size.div_ceil(self.superblock.block_size);
            for block_index in first_free_block..old_block_count {
                self.clear_data_block(&mut inode, block_index)?;
            }
            if length % self.superblock.block_size != 0 {
                let block_index = length / self.superblock.block_size;
                if let Some(block) = self.block_at(&inode, block_index)? {
                    let mut data = self.read_block(block)?;
                    data[length % self.superblock.block_size..].fill(0);
                    self.write_block(block, &data)?;
                }
            }
        }
        inode.set_size(length as u64, self.superblock.feature_ro_compat)?;
        self.write_inode(inode_number, &inode)
    }

    fn set_permissions(&mut self, path: &str, permissions: u16) -> Result<(), FsError> {
        if permissions & !0o777 != 0 {
            return Err(FsError::InvalidData);
        }
        let (inode_number, mut inode) = self.lookup_inode(path)?;
        inode.set_mode((inode.mode() & !0o777) | permissions);
        self.write_inode(inode_number, &inode)
    }

    fn create_file(&mut self, path: &str) -> Result<(), FsError> {
        let (parent_number, _, name) = self.parent_and_name(path)?;
        let inode_number = self.allocate_inode(false)?;
        let mut inode = Inode::new(self.superblock.inode_size);
        inode.set_mode(0x8000 | 0o644);
        inode.set_links(1);
        if let Err(error) = self.write_inode(inode_number, &inode).and_then(|_| {
            self.insert_directory_entry(parent_number, &name, inode_number, EXT2_FT_REG_FILE)
        }) {
            self.free_inode(inode_number, false)?;
            return Err(error);
        }
        Ok(())
    }

    fn create_dir(&mut self, path: &str) -> Result<(), FsError> {
        let (parent_number, _, name) = self.parent_and_name(path)?;
        let inode_number = self.allocate_inode(true)?;
        let mut inode = Inode::new(self.superblock.inode_size);
        inode.set_mode(0x4000 | 0o755);
        inode.set_links(2);
        let data_block = match self.allocate_block() {
            Ok(block) => block,
            Err(error) => {
                self.free_inode(inode_number, true)?;
                return Err(error);
            }
        };
        inode.set_size(
            self.superblock.block_size as u64,
            self.superblock.feature_ro_compat,
        )?;
        inode.set_block_pointer(0, data_block)?;
        inode.set_sector_count(self.sectors_per_block() as u32);
        let mut directory_block = vec![0; self.superblock.block_size];
        let filetype = self.superblock.feature_incompat & EXT2_FILETYPE_FEATURE != 0;
        write_directory_record(
            &mut directory_block,
            0,
            12,
            inode_number,
            ".",
            EXT2_FT_DIR,
            filetype,
        )?;
        write_directory_record(
            &mut directory_block,
            12,
            self.superblock.block_size - 12,
            parent_number,
            "..",
            EXT2_FT_DIR,
            filetype,
        )?;
        let result = self
            .write_block(data_block, &directory_block)
            .and_then(|_| self.write_inode(inode_number, &inode))
            .and_then(|_| {
                self.insert_directory_entry(parent_number, &name, inode_number, EXT2_FT_DIR)
            });
        if let Err(error) = result {
            self.free_block(data_block)?;
            self.free_inode(inode_number, true)?;
            return Err(error);
        }
        let mut parent = self.read_inode(parent_number)?;
        parent.set_links(parent.links().checked_add(1).ok_or(FsError::NoSpace)?);
        self.write_inode(parent_number, &parent)
    }

    fn create_symlink(&mut self, target: &str, path: &str) -> Result<(), FsError> {
        if target.is_empty() || target.len() > u32::MAX as usize || target.as_bytes().contains(&0) {
            return Err(FsError::InvalidPath);
        }
        let (parent_number, _, name) = self.parent_and_name(path)?;
        let inode_number = self.allocate_inode(false)?;
        let mut inode = Inode::new(self.superblock.inode_size);
        inode.set_mode(0xA000 | 0o777);
        inode.set_links(1);
        inode.set_size(target.len() as u64, self.superblock.feature_ro_compat)?;
        let result = if target.len() <= 60 {
            inode.bytes[EXT2_INODE_POINTERS_OFFSET..EXT2_INODE_POINTERS_OFFSET + target.len()]
                .copy_from_slice(target.as_bytes());
            self.write_inode(inode_number, &inode)
        } else {
            let result = self.write_file_range(&mut inode, 0, target.as_bytes());
            if result.is_ok() {
                self.write_inode(inode_number, &inode)
            } else {
                result
            }
        }
        .and_then(|_| {
            self.insert_directory_entry(parent_number, &name, inode_number, EXT2_FT_SYMLINK)
        });
        if let Err(error) = result {
            self.free_inode_storage(&inode)?;
            self.free_inode(inode_number, false)?;
            return Err(error);
        }
        Ok(())
    }

    fn remove(&mut self, path: &str) -> Result<(), FsError> {
        let (parent_number, mut parent, name) = self.parent_and_name(path)?;
        if parent.flags() & EXT2_INDEX_FL != 0 {
            parent.set_flags(parent.flags() & !EXT2_INDEX_FL);
            self.write_inode(parent_number, &parent)?;
        }
        let target_number = self
            .directory_records(&parent)?
            .into_iter()
            .find(|record| record.name == name)
            .map(|record| record.inode)
            .ok_or(FsError::NotFound)?;
        let mut target = self.read_inode(target_number)?;
        let node_type = target.node_type()?;
        if node_type == NodeType::Directory && !self.directory_is_empty(&target)? {
            return Err(FsError::DirectoryNotEmpty);
        }
        self.remove_directory_entry(&parent, &name)?;

        if node_type == NodeType::Directory {
            let mut parent = self.read_inode(parent_number)?;
            parent.set_links(parent.links().checked_sub(1).ok_or(FsError::InvalidData)?);
            self.write_inode(parent_number, &parent)?;
            self.free_inode_storage(&target)?;
            target.clear();
            self.write_inode(target_number, &target)?;
            self.free_inode(target_number, true)
        } else if target.links() > 1 {
            target.set_links(target.links() - 1);
            self.write_inode(target_number, &target)
        } else {
            self.free_inode_storage(&target)?;
            target.clear();
            self.write_inode(target_number, &target)?;
            self.free_inode(target_number, false)
        }
    }

    fn read_dir(&self, path: &str) -> Result<Vec<DirectoryEntry>, FsError> {
        let (_, inode) = self.lookup_inode(path)?;
        let records = self.directory_records(&inode)?;
        let mut entries = Vec::new();
        for record in records {
            if record.name == "." || record.name == ".." {
                continue;
            }
            let child = self.read_inode(record.inode)?;
            let node_type = child.node_type()?;
            let len = if node_type == NodeType::Directory {
                0
            } else {
                usize::try_from(child.size(self.superblock.feature_ro_compat)?)
                    .map_err(|_| FsError::InvalidData)?
            };
            let _ = record.file_type;
            entries.push(DirectoryEntry {
                name: record.name,
                metadata: Metadata {
                    node_type,
                    len,
                    permissions: child.mode() & 0o777,
                },
            });
        }
        Ok(entries)
    }
}

impl Inode {
    fn new(size: usize) -> Self {
        Self {
            bytes: vec![0; size],
        }
    }

    fn clear(&mut self) {
        self.bytes.fill(0);
    }

    fn mode(&self) -> u16 {
        u16::from_le_bytes([self.bytes[0], self.bytes[1]])
    }

    fn set_mode(&mut self, mode: u16) {
        self.bytes[0..2].copy_from_slice(&mode.to_le_bytes());
    }

    fn node_type(&self) -> Result<NodeType, FsError> {
        match self.mode() & 0xF000 {
            0x8000 => Ok(NodeType::File),
            0x4000 => Ok(NodeType::Directory),
            0xA000 => Ok(NodeType::Symlink),
            _ => Err(FsError::Unsupported),
        }
    }

    fn links(&self) -> u16 {
        u16::from_le_bytes([self.bytes[26], self.bytes[27]])
    }

    fn set_links(&mut self, links: u16) {
        self.bytes[26..28].copy_from_slice(&links.to_le_bytes());
    }

    fn flags(&self) -> u32 {
        u32::from_le_bytes(self.bytes[32..36].try_into().unwrap())
    }

    fn set_flags(&mut self, flags: u32) {
        self.bytes[32..36].copy_from_slice(&flags.to_le_bytes());
    }

    fn sector_count(&self) -> u32 {
        u32::from_le_bytes(self.bytes[28..32].try_into().unwrap())
    }

    fn set_sector_count(&mut self, count: u32) {
        self.bytes[28..32].copy_from_slice(&count.to_le_bytes());
    }

    fn add_sector_count(&mut self, count: u32) -> Result<(), FsError> {
        self.set_sector_count(
            self.sector_count()
                .checked_add(count)
                .ok_or(FsError::NoSpace)?,
        );
        Ok(())
    }

    fn block_pointer(&self, index: usize) -> Result<u32, FsError> {
        if index >= 15 {
            return Err(FsError::InvalidData);
        }
        read_u32(&self.bytes, EXT2_INODE_POINTERS_OFFSET + index * 4)
    }

    fn set_block_pointer(&mut self, index: usize, block: u32) -> Result<(), FsError> {
        if index >= 15 {
            return Err(FsError::InvalidData);
        }
        write_u32(
            &mut self.bytes,
            EXT2_INODE_POINTERS_OFFSET + index * 4,
            block,
        )
    }

    fn size(&self, feature_ro_compat: u32) -> Result<u64, FsError> {
        let low = read_u32(&self.bytes, 4)? as u64;
        let high = if self.node_type()? == NodeType::File && feature_ro_compat & 0x2 != 0 {
            read_u32(&self.bytes, 108)? as u64
        } else {
            0
        };
        Ok((high << 32) | low)
    }

    fn set_size(&mut self, size: u64, feature_ro_compat: u32) -> Result<(), FsError> {
        let high = size >> 32;
        if high != 0 && (self.node_type()? != NodeType::File || feature_ro_compat & 0x2 == 0) {
            return Err(FsError::NoSpace);
        }
        write_u32(&mut self.bytes, 4, size as u32)?;
        if self.node_type()? == NodeType::File && feature_ro_compat & 0x2 != 0 {
            write_u32(&mut self.bytes, 108, high as u32)?;
        }
        Ok(())
    }
}

fn locate_superblock(
    device: &dyn BlockDevice,
) -> Result<(u64, u64, [u8; EXT2_SUPERBLOCK_SIZE]), FsError> {
    let total_sectors = device.sector_count();
    if total_sectors < 4 { return Err(FsError::InvalidData); }
    let mut mbr = [0; SECTOR_SIZE];
    device.read_sectors(0, &mut mbr).map_err(|_| FsError::IoError)?;
    if mbr[510..512] == [0x55, 0xAA] && mbr[450] == 0xEE {
        if let Some(root) = locate_gpt_ext2(device, total_sectors)? { return Ok(root); }
    }
    if mbr[510..512] == [0x55, 0xAA] {
        for index in 0..4 {
            let offset = 446 + index * 16;
            if mbr[offset + 4] != 0x83 { continue; }
            let start = read_u32(&mbr, offset + 8)? as u64;
            let sectors = read_u32(&mbr, offset + 12)? as u64;
            if sectors == 0 || start.checked_add(sectors).is_none_or(|end| end > total_sectors) { continue; }
            if let Some(superblock) = read_superblock(device, start, sectors)? { return Ok((start, sectors, superblock)); }
        }
    }
    if let Some(superblock) = read_superblock(device, 0, total_sectors)? { return Ok((0, total_sectors, superblock)); }
    Err(FsError::InvalidData)
}

fn locate_gpt_ext2(device: &dyn BlockDevice, total_sectors: u64) -> Result<Option<(u64, u64, [u8; EXT2_SUPERBLOCK_SIZE])>, FsError> {
    const LINUX_FS_GUID: [u8; 16] = [0xAF,0x3D,0xC6,0x0F,0x83,0x84,0x72,0x47,0x8E,0x79,0x3D,0x69,0xD8,0x47,0x7D,0xE4];
    let mut header = [0; SECTOR_SIZE];
    device.read_sectors(1, &mut header).map_err(|_| FsError::IoError)?;
    if &header[..8] != b"EFI PART" { return Ok(None); }
    let entries_lba = u64::from_le_bytes(header[72..80].try_into().unwrap());
    let count = u32::from_le_bytes(header[80..84].try_into().unwrap()).min(4096);
    let size = u32::from_le_bytes(header[84..88].try_into().unwrap());
    if !(128..=512).contains(&size) || size % 8 != 0 { return Err(FsError::InvalidData); }
    let bytes = (count as u64).checked_mul(size as u64).ok_or(FsError::InvalidData)?;
    let sectors = bytes.div_ceil(SECTOR_SIZE as u64);
    if bytes > 2 * 1024 * 1024 || entries_lba == 0 || entries_lba.checked_add(sectors).is_none_or(|end| end > total_sectors) { return Err(FsError::InvalidData); }
    let mut entries = alloc::vec![0u8; sectors as usize * SECTOR_SIZE];
    if !entries.is_empty() { device.read_sectors(entries_lba, &mut entries).map_err(|_| FsError::IoError)?; }
    for index in 0..count as usize {
        let off = index * size as usize;
        let entry = &entries[off..off + size as usize];
        if entry[..16] != LINUX_FS_GUID { continue; }
        let start = u64::from_le_bytes(entry[32..40].try_into().unwrap());
        let last = u64::from_le_bytes(entry[40..48].try_into().unwrap());
        let Some(length) = last.checked_sub(start).and_then(|n| n.checked_add(1)) else { continue; };
        if length == 0 || start.checked_add(length).is_none_or(|end| end > total_sectors) { continue; }
        if let Some(superblock) = read_superblock(device, start, length)? { return Ok(Some((start, length, superblock))); }
    }
    Ok(None)
}
fn read_superblock(
    device: &dyn BlockDevice,
    partition_start: u64,
    partition_sectors: u64,
) -> Result<Option<[u8; EXT2_SUPERBLOCK_SIZE]>, FsError> {
    let superblock_lba = partition_start.checked_add(2).ok_or(FsError::IoError)?;
    let partition_end = partition_start
        .checked_add(partition_sectors)
        .ok_or(FsError::IoError)?;
    if partition_sectors < 4
        || superblock_lba
            .checked_add(2)
            .is_none_or(|end| end > partition_end)
    {
        return Ok(None);
    }
    let mut bytes = [0; EXT2_SUPERBLOCK_SIZE];
    device
        .read_sectors(superblock_lba, &mut bytes)
        .map_err(|_| FsError::IoError)?;
    if read_u16(&bytes, 56)? == EXT2_MAGIC {
        Ok(Some(bytes))
    } else {
        Ok(None)
    }
}

fn parse_superblock(bytes: &[u8; EXT2_SUPERBLOCK_SIZE]) -> Result<Superblock, FsError> {
    if read_u16(bytes, 56)? != EXT2_MAGIC {
        return Err(FsError::InvalidData);
    }
    let revision = read_u32(bytes, 76)?;
    let block_log = read_u32(bytes, 24)?;
    if block_log > 2 {
        return Err(FsError::Unsupported);
    }
    let block_size = 1024usize << block_log;
    let blocks_count = read_u32(bytes, 4)?;
    let inodes_count = read_u32(bytes, 0)?;
    let first_data_block = read_u32(bytes, 20)?;
    let blocks_per_group = read_u32(bytes, 32)?;
    let inodes_per_group = read_u32(bytes, 40)?;
    if blocks_count == 0
        || inodes_count < EXT2_ROOT_INODE
        || first_data_block >= blocks_count
        || (block_size == 1024 && first_data_block != 1)
        || (block_size > 1024 && first_data_block != 0)
        || blocks_per_group == 0
        || inodes_per_group == 0
        || blocks_per_group as usize > block_size * 8
        || inodes_per_group as usize > block_size * 8
    {
        return Err(FsError::InvalidData);
    }
    let groups_count = blocks_count
        .checked_sub(first_data_block)
        .ok_or(FsError::InvalidData)?
        .div_ceil(blocks_per_group);
    if groups_count == 0 || groups_count > MAX_GROUPS {
        return Err(FsError::Unsupported);
    }
    if inodes_count as u64 > groups_count as u64 * inodes_per_group as u64 {
        return Err(FsError::InvalidData);
    }

    let inode_size = if revision == 0 {
        EXT2_GOOD_OLD_INODE_SIZE
    } else {
        read_u16(bytes, 88)? as usize
    };
    if inode_size < EXT2_GOOD_OLD_INODE_SIZE
        || inode_size > block_size
        || !inode_size.is_multiple_of(4)
        || block_size % inode_size != 0
    {
        return Err(FsError::Unsupported);
    }
    let feature_compat = if revision == 0 {
        0
    } else {
        read_u32(bytes, 92)?
    };
    let feature_incompat = if revision == 0 {
        0
    } else {
        read_u32(bytes, 96)?
    };
    let feature_ro_compat = if revision == 0 {
        0
    } else {
        read_u32(bytes, 100)?
    };
    if feature_compat & EXT2_JOURNAL_FEATURE != 0
        || feature_incompat & !EXT2_ALLOWED_INCOMPAT != 0
        || feature_ro_compat & !EXT2_ALLOWED_RO_COMPAT != 0
    {
        return Err(FsError::Unsupported);
    }
    let first_inode = if revision == 0 {
        11
    } else {
        read_u32(bytes, 84)?.max(11)
    };
    let reserved_gdt_blocks = if feature_compat & 0x10 != 0 {
        read_u16(bytes, 206)?
    } else {
        0
    };
    let group_desc_block = first_data_block
        .checked_add(1)
        .ok_or(FsError::InvalidData)?;

    Ok(Superblock {
        block_size,
        blocks_count,
        inodes_count,
        first_data_block,
        blocks_per_group,
        inodes_per_group,
        inode_size,
        first_inode,
        groups_count,
        feature_compat,
        feature_incompat,
        feature_ro_compat,
        reserved_gdt_blocks,
        group_desc_block,
    })
}

fn validate_name(name: &str) -> Result<(), FsError> {
    if name.is_empty()
        || name.len() > MAX_FILE_NAME
        || name == "."
        || name == ".."
        || name.as_bytes().contains(&0)
        || name.as_bytes().contains(&b'/')
    {
        return Err(FsError::InvalidPath);
    }
    Ok(())
}

fn directory_record_size(name_length: usize) -> usize {
    (8 + name_length + 3) & !3
}

fn write_directory_record(
    data: &mut [u8],
    offset: usize,
    record_length: usize,
    inode_number: u32,
    name: &str,
    file_type: u8,
    has_file_type: bool,
) -> Result<(), FsError> {
    if name.len() > MAX_FILE_NAME
        || record_length < directory_record_size(name.len())
        || record_length > u16::MAX as usize
        || offset
            .checked_add(record_length)
            .is_none_or(|end| end > data.len())
    {
        return Err(FsError::InvalidData);
    }
    data[offset..offset + record_length].fill(0);
    write_u32(data, offset, inode_number)?;
    write_u16(data, offset + 4, record_length as u16)?;
    if has_file_type {
        data[offset + 6] = name.len() as u8;
        data[offset + 7] = file_type;
    } else {
        write_u16(data, offset + 6, name.len() as u16)?;
    }
    data[offset + 8..offset + 8 + name.len()].copy_from_slice(name.as_bytes());
    Ok(())
}

fn write_free_directory_record(
    data: &mut [u8],
    offset: usize,
    record_length: usize,
) -> Result<(), FsError> {
    if record_length < 8 || record_length > u16::MAX as usize {
        return Err(FsError::InvalidData);
    }
    data[offset..offset + record_length].fill(0);
    write_u16(data, offset + 4, record_length as u16)
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, FsError> {
    let value = bytes.get(offset..offset + 2).ok_or(FsError::InvalidData)?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, FsError> {
    let value = bytes.get(offset..offset + 4).ok_or(FsError::InvalidData)?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn write_u16(bytes: &mut [u8], offset: usize, value: u16) -> Result<(), FsError> {
    let destination = bytes
        .get_mut(offset..offset + 2)
        .ok_or(FsError::InvalidData)?;
    destination.copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) -> Result<(), FsError> {
    let destination = bytes
        .get_mut(offset..offset + 4)
        .ok_or(FsError::InvalidData)?;
    destination.copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn is_power_of(mut number: u32, base: u32) -> bool {
    if number == 0 {
        return false;
    }
    while number % base == 0 {
        number /= base;
    }
    number == 1
}

#[cfg(test)]
mod tests {
    use alloc::sync::Arc;
    use alloc::vec;
    use spin::Mutex;

    use super::{EXT2_MAGIC, Ext2};
    use crate::fs::{FileSystem, FsError, NodeType};
    use crate::storage::block::{BlockDevice, SECTOR_SIZE};

    struct MemoryDisk {
        bytes: Mutex<alloc::vec::Vec<u8>>,
    }

    impl BlockDevice for MemoryDisk {
        fn name(&self) -> &str {
            "ext2-test"
        }

        fn sector_count(&self) -> u64 {
            (self.bytes.lock().len() / SECTOR_SIZE) as u64
        }

        fn read_sectors(
            &self,
            lba: u64,
            buffer: &mut [u8],
        ) -> Result<(), super::super::block::BlockError> {
            if !buffer.len().is_multiple_of(SECTOR_SIZE) {
                return Err(super::super::block::BlockError::InvalidBuffer);
            }
            let start = usize::try_from(lba)
                .ok()
                .and_then(|sector| sector.checked_mul(SECTOR_SIZE))
                .ok_or(super::super::block::BlockError::OutOfRange)?;
            let end = start
                .checked_add(buffer.len())
                .filter(|end| *end <= self.bytes.lock().len())
                .ok_or(super::super::block::BlockError::OutOfRange)?;
            buffer.copy_from_slice(&self.bytes.lock()[start..end]);
            Ok(())
        }

        fn write_sectors(
            &self,
            lba: u64,
            buffer: &[u8],
        ) -> Result<(), super::super::block::BlockError> {
            if !buffer.len().is_multiple_of(SECTOR_SIZE) {
                return Err(super::super::block::BlockError::InvalidBuffer);
            }
            let start = usize::try_from(lba)
                .ok()
                .and_then(|sector| sector.checked_mul(SECTOR_SIZE))
                .ok_or(super::super::block::BlockError::OutOfRange)?;
            let end = start
                .checked_add(buffer.len())
                .filter(|end| *end <= self.bytes.lock().len())
                .ok_or(super::super::block::BlockError::OutOfRange)?;
            self.bytes.lock()[start..end].copy_from_slice(buffer);
            Ok(())
        }
    }

    fn test_disk() -> Arc<MemoryDisk> {
        let mut bytes = vec![0; 1024 * 1024];
        {
            let superblock = &mut bytes[1024..2048];
            put_u32(superblock, 0, 32);
            put_u32(superblock, 4, 1024);
            put_u32(superblock, 12, 1014);
            put_u32(superblock, 16, 22);
            put_u32(superblock, 20, 1);
            put_u32(superblock, 24, 0);
            put_u32(superblock, 32, 1023);
            put_u32(superblock, 40, 32);
            put_u16(superblock, 56, EXT2_MAGIC);
            put_u16(superblock, 58, 1);
            put_u32(superblock, 76, 1);
            put_u32(superblock, 84, 11);
            put_u16(superblock, 88, 128);
            put_u32(superblock, 96, 2);
            put_u32(superblock, 100, 1);
        }
        {
            let descriptor = &mut bytes[2 * 1024..2 * 1024 + 32];
            put_u32(descriptor, 0, 3);
            put_u32(descriptor, 4, 4);
            put_u32(descriptor, 8, 5);
            put_u16(descriptor, 12, 1014);
            put_u16(descriptor, 14, 22);
            put_u16(descriptor, 16, 1);
        }
        for block in 1..=9 {
            let bit = block - 1;
            bytes[3 * 1024 + bit / 8] |= 1 << (bit % 8);
        }
        for inode in 1..=10 {
            let bit = inode - 1;
            bytes[4 * 1024 + bit / 8] |= 1 << (bit % 8);
        }
        {
            let root = &mut bytes[5 * 1024 + 128..5 * 1024 + 256];
            put_u16(root, 0, 0x41ED);
            put_u32(root, 4, 1024);
            put_u16(root, 26, 2);
            put_u32(root, 28, 2);
            put_u32(root, 40, 9);
        }
        {
            let root_dir = &mut bytes[9 * 1024..10 * 1024];
            write_test_dirent(root_dir, 0, 12, 2, b".");
            write_test_dirent(root_dir, 12, 1012, 2, b"..");
        }
        Arc::new(MemoryDisk {
            bytes: Mutex::new(bytes),
        })
    }

    fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn write_test_dirent(
        block: &mut [u8],
        offset: usize,
        record_length: usize,
        inode: u32,
        name: &[u8],
    ) {
        put_u32(block, offset, inode);
        put_u16(block, offset + 4, record_length as u16);
        block[offset + 6] = name.len() as u8;
        block[offset + 7] = 2;
        block[offset + 8..offset + 8 + name.len()].copy_from_slice(name);
    }

    #[test]
    fn mounts_reads_writes_and_truncates_files() {
        let disk = test_disk();
        let mut ext2 = Ext2::mount(disk.clone()).unwrap();
        ext2.create_file("/note").unwrap();
        assert_eq!(
            ext2.read_dir("/")
                .unwrap()
                .iter()
                .filter(|entry| entry.name == "note")
                .count(),
            1
        );
        assert_eq!(ext2.write("/note", 0, b"hello").unwrap(), 5);
        assert_eq!(ext2.write("/note", 1300, b"end").unwrap(), 3);

        let mut data = vec![0xFF; 1303];
        assert_eq!(ext2.read("/note", 0, &mut data).unwrap(), 1303);
        assert_eq!(&data[..5], b"hello");
        assert!(data[5..1300].iter().all(|byte| *byte == 0));
        assert_eq!(&data[1300..], b"end");

        ext2.truncate("/note", 2).unwrap();
        let mut short = [0; 8];
        assert_eq!(ext2.read("/note", 0, &mut short).unwrap(), 2);
        assert_eq!(&short[..2], b"he");
        ext2.truncate("/note", 0).unwrap();
        assert_eq!(ext2.metadata("/note").unwrap().len, 0);
        drop(ext2);

        let remounted = Ext2::mount(disk).unwrap();
        assert_eq!(remounted.metadata("/note").unwrap().len, 0);
    }

    #[test]
    fn supports_indirect_blocks_and_reclaims_them_on_truncate() {
        let disk = test_disk();
        let mut ext2 = Ext2::mount(disk.clone()).unwrap();
        ext2.create_file("/large").unwrap();
        let offset = 12 * 1024;
        ext2.write("/large", offset, b"x").unwrap();
        let mut byte = [0];
        assert_eq!(ext2.read("/large", offset, &mut byte).unwrap(), 1);
        assert_eq!(byte, [b'x']);

        ext2.truncate("/large", 0).unwrap();
        drop(ext2);
        let bytes = disk.bytes.lock();
        assert_eq!(
            u32::from_le_bytes(bytes[1024 + 12..1024 + 16].try_into().unwrap()),
            1014
        );
    }

    #[test]
    fn supports_double_indirect_file_blocks() {
        let disk = test_disk();
        let mut ext2 = Ext2::mount(disk.clone()).unwrap();
        ext2.create_file("/double").unwrap();
        let offset = (12 + 1024 / 4) * 1024;
        ext2.write("/double", offset, b"d").unwrap();
        let mut byte = [0];
        assert_eq!(ext2.read("/double", offset, &mut byte).unwrap(), 1);
        assert_eq!(byte, [b'd']);
        ext2.truncate("/double", 0).unwrap();
        drop(ext2);

        let bytes = disk.bytes.lock();
        assert_eq!(
            u32::from_le_bytes(bytes[1024 + 12..1024 + 16].try_into().unwrap()),
            1014
        );
    }

    #[test]
    fn creates_links_and_directories_and_refuses_nonempty_removal() {
        let mut ext2 = Ext2::mount(test_disk()).unwrap();
        ext2.create_dir("/docs").unwrap();
        ext2.create_file("/docs/readme").unwrap();
        ext2.set_permissions("/docs/readme", 0o600).unwrap();
        assert_eq!(ext2.metadata("/docs/readme").unwrap().permissions, 0o600);
        assert_eq!(ext2.read_dir("/docs").unwrap().len(), 1);
        ext2.create_symlink("/docs/readme", "/readme-link").unwrap();
        assert_eq!(ext2.read_link("/readme-link").unwrap(), "/docs/readme");
        assert_eq!(ext2.remove("/docs"), Err(FsError::DirectoryNotEmpty));
        assert_eq!(
            ext2.metadata("/docs").unwrap().node_type,
            NodeType::Directory
        );
        ext2.remove("/docs/readme").unwrap();
        ext2.remove("/readme-link").unwrap();
        ext2.remove("/docs").unwrap();
        assert_eq!(ext2.metadata("/docs"), Err(FsError::NotFound));
    }

    #[test]
    fn mounts_ext2_in_a_primary_mbr_partition() {
        let source = test_disk();
        let source_bytes = source.bytes.lock().clone();
        let partition_start = 8 * SECTOR_SIZE;
        let mut bytes = vec![0; partition_start + source_bytes.len()];
        bytes[510..512].copy_from_slice(&[0x55, 0xAA]);
        let partition = 446;
        bytes[partition + 4] = 0x83;
        bytes[partition + 8..partition + 12].copy_from_slice(&8u32.to_le_bytes());
        bytes[partition + 12..partition + 16]
            .copy_from_slice(&((source_bytes.len() / SECTOR_SIZE) as u32).to_le_bytes());
        bytes[partition_start..].copy_from_slice(&source_bytes);
        let disk = Arc::new(MemoryDisk {
            bytes: Mutex::new(bytes),
        });

        let ext2 = Ext2::mount(disk).unwrap();
        assert_eq!(ext2.metadata("/").unwrap().node_type, NodeType::Directory);
    }
}
