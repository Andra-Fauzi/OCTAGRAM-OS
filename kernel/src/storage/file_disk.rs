//! A block device backed by a regular file in the VFS.
//!
//! The native image builder needs a whole-disk target that is a file (so the
//! result can be copied to USB, attached to a VM, or checksummed) rather than a
//! physical SATA disk. This adapter presents such a file as a [`BlockDevice`]:
//!
//! * the file is created empty and grows as sectors are written, so an image
//!   costs only the sectors the builder actually touches until the final GPT
//!   backup header extends it to full size;
//! * reads past the current end of file return zeroes, matching a sparse image;
//! * partial reads are looped until the buffer is full or the file ends.
//!
//! Status: host-tested and wired to `mkimage file <path> <size-mib>`; booting a
//! produced file image under QEMU is still pending (see roadmap M6.2).

use alloc::string::{String, ToString};

use crate::fs::{self, FsError};

use super::block::{BlockDevice, BlockError, SECTOR_SIZE};

pub struct FileDisk {
    path: String,
    sectors: u64,
}

impl FileDisk {
    /// Create a new image file with a fixed logical capacity.
    ///
    /// Refuses to overwrite an existing file so an image build cannot silently
    /// destroy something else; remove the path first to rebuild.
    pub fn create(path: &str, sectors: u64) -> Result<Self, FsError> {
        if sectors == 0
            || sectors > (usize::MAX as u64) / SECTOR_SIZE as u64
            || path.is_empty()
        {
            return Err(FsError::NoSpace);
        }
        fs::with_vfs(|vfs| match vfs.metadata(path) {
            Ok(_) => Err(FsError::AlreadyExists),
            Err(FsError::NotFound) => vfs.create_file(path),
            Err(error) => Err(error),
        })??;
        Ok(Self {
            path: path.to_string(),
            sectors,
        })
    }

    /// Open an existing image file, whose capacity is its current length.
    pub fn open(path: &str) -> Result<Self, FsError> {
        let length = fs::with_vfs(|vfs| vfs.metadata(path).map(|metadata| metadata.len))??;
        if length == 0 || length % SECTOR_SIZE != 0 {
            return Err(FsError::InvalidData);
        }
        Ok(Self {
            path: path.to_string(),
            sectors: (length / SECTOR_SIZE) as u64,
        })
    }

    pub fn path(&self) -> &str {
        &self.path
    }
}

impl BlockDevice for FileDisk {
    fn name(&self) -> &str {
        &self.path
    }

    fn sector_count(&self) -> u64 {
        self.sectors
    }

    fn read_sectors(&self, lba: u64, buffer: &mut [u8]) -> Result<(), BlockError> {
        let offset = checked_range(lba, buffer.len(), self.sectors)?;
        if buffer.is_empty() {
            return Ok(());
        }
        let mut filled = 0usize;
        let result = fs::with_vfs(|vfs| -> Result<(), BlockError> {
            while filled < buffer.len() {
                let count = vfs
                    .read(&self.path, offset + filled, &mut buffer[filled..])
                    .map_err(|_| BlockError::Io)?;
                if count == 0 {
                    break;
                }
                filled += count;
            }
            Ok(())
        });
        match result {
            Ok(Ok(())) => {}
            _ => return Err(BlockError::Io),
        }
        // Sparse tail: bytes never written read back as zeroes.
        buffer[filled..].fill(0);
        Ok(())
    }

    fn write_sectors(&self, lba: u64, buffer: &[u8]) -> Result<(), BlockError> {
        let offset = checked_range(lba, buffer.len(), self.sectors)?;
        if buffer.is_empty() {
            return Ok(());
        }
        let written = fs::with_vfs(|vfs| vfs.write(&self.path, offset, buffer))
            .map_err(|_| BlockError::Io)?
            .map_err(|_| BlockError::Io)?;
        if written != buffer.len() {
            return Err(BlockError::Io);
        }
        Ok(())
    }
}

fn checked_range(lba: u64, length: usize, sectors: u64) -> Result<usize, BlockError> {
    if !length.is_multiple_of(SECTOR_SIZE) {
        return Err(BlockError::InvalidBuffer);
    }
    let count = (length / SECTOR_SIZE) as u64;
    if lba.checked_add(count).is_none_or(|end| end > sectors) {
        return Err(BlockError::OutOfRange);
    }
    usize::try_from(lba)
        .ok()
        .and_then(|lba| lba.checked_mul(SECTOR_SIZE))
        .ok_or(BlockError::OutOfRange)
}

#[cfg(test)]
mod tests {
    use super::FileDisk;
    use crate::fs;
    use crate::storage::block::{BlockDevice, SECTOR_SIZE};
    use alloc::sync::Arc;

    fn cleanup(path: &str) {
        let _ = fs::with_vfs(|vfs| vfs.remove(path));
    }

    #[test]
    fn reads_and_writes_sectors_with_a_sparse_tail() {
        let path = "/file-disk-sparse.img";
        fs::init();
        cleanup(path);
        let disk = FileDisk::create(path, 64).unwrap();
        assert_eq!(disk.sector_count(), 64);
        assert_eq!(disk.name(), path);

        let mut block = [0u8; SECTOR_SIZE * 2];
        block[..SECTOR_SIZE].fill(0x33);
        disk.write_sectors(4, &block).unwrap();

        let mut read_back = [0u8; SECTOR_SIZE * 2];
        disk.read_sectors(4, &mut read_back).unwrap();
        assert_eq!(read_back, block);

        // Beyond the bytes written, a sparse file reads as zeroes.
        let mut tail = [0xFFu8; SECTOR_SIZE * 4];
        disk.read_sectors(50, &mut tail).unwrap();
        assert_eq!(tail, [0u8; SECTOR_SIZE * 4]);

        let mut small = [0u8; SECTOR_SIZE];
        assert!(matches!(
            disk.read_sectors(63, &mut [0u8; SECTOR_SIZE * 2]),
            Err(crate::storage::block::BlockError::OutOfRange)
        ));
        disk.read_sectors(63, &mut small).unwrap();

        cleanup(path);
    }

    #[test]
    fn refuses_to_overwrite_and_rejects_bad_lengths() {
        let path = "/file-disk-overwrite.img";
        fs::init();
        cleanup(path);
        FileDisk::create(path, 8).unwrap();
        assert!(matches!(
            FileDisk::create(path, 8),
            Err(fs::FsError::AlreadyExists)
        ));
        assert!(FileDisk::create("/file-disk-empty.img", 0).is_err());
        cleanup(path);
    }

    #[test]
    fn builds_a_bootable_image_into_a_file() {
        let path = "/file-disk-image.img";
        fs::init();
        cleanup(path);
        let sectors = crate::image_builder::MIN_IMAGE_SECTORS;
        let disk = Arc::new(FileDisk::create(path, sectors).unwrap());
        let kernel = {
            let mut bytes = alloc::vec![0xA5u8; 4096];
            bytes[..4].copy_from_slice(b"\x7FELF");
            bytes
        };
        let mut bootloader = alloc::vec![0x5Au8; 4096];
        bootloader[..2].copy_from_slice(b"MZ");
        let report = crate::image_builder::build_image(
            disk.clone(),
            crate::image_builder::ImageAssets {
                kernel: &kernel,
                bootloader: &bootloader,
                boot_config: b"timeout: 3\n/OCTAGRAM\n    protocol: limine\n",
            },
        )
        .unwrap();
        assert!(report.root_sectors > 0);

        let mut header = [0u8; SECTOR_SIZE];
        disk.read_sectors(1, &mut header).unwrap();
        assert_eq!(&header[..8], b"EFI PART");

        // The file must have grown to the full advertised capacity.
        let length = fs::with_vfs(|vfs| vfs.metadata(path).map(|m| m.len))
            .unwrap()
            .unwrap();
        assert_eq!(length as u64, sectors * SECTOR_SIZE as u64);
        cleanup(path);
    }
}
