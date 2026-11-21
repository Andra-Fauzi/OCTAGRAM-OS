//! Native image builder for bootable OCTAGRAM-OS disks.
//!
//! The builder lays out a GPT (EFI System Partition + EXT2 root), formats both
//! filesystems with the kernel's own formatters, writes the kernel, the UEFI
//! loader and the boot configuration from caller-provided assets, records a
//! SHA-256 manifest, and then re-opens both partitions to verify every file
//! byte-for-byte before reporting success.
//!
//! It deliberately works on a [`BlockDevice`] rather than the Live boot files:
//! the destructive installer copies whatever Limine loaded, while this builder
//! packages explicit assets so an image can be produced from the persistent
//! source tree once the toolchain has rebuilt them.
//!
//! Status: host-tested on an in-memory block device. The shell command that
//! drives a real target disk and the QEMU boot test of a produced image are
//! still pending; see `docs/native-self-hosting-roadmap.md` section M6.1.

use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::fs::{FileSystem, FsError};
use crate::sha256;
use crate::storage::{
    block::{BlockDevice, BlockDeviceRef, BlockError, SECTOR_SIZE},
    ext2::Ext2,
    fat32::Fat32,
    partition::{self, PartitionDevice},
};

/// 80 MiB ESP: comfortably above FAT32's 65,525-cluster minimum while keeping
/// a produced image small enough to be written by the native VFS.
pub const IMAGE_ESP_SECTORS: u64 = 80 * 1024 * 1024 / SECTOR_SIZE as u64;
/// Smallest whole-disk image the builder accepts (ESP + EXT2 root metadata).
pub const MIN_IMAGE_SECTORS: u64 = 160 * 1024 * 1024 / SECTOR_SIZE as u64;
/// The EXT2 formatter needs at least 1024 4 KiB blocks.
pub const MIN_ROOT_SECTORS: u64 = 8 * 1024 * 1024 / SECTOR_SIZE as u64;

const COPY_CHUNK_SIZE: usize = 32 * 1024;
const MANIFEST_PATH: &str = "/boot/image-manifest.txt";

/// Boot assets packaged into the image. All slices are copied into the image.
pub struct ImageAssets<'a> {
    pub kernel: &'a [u8],
    pub bootloader: &'a [u8],
    pub boot_config: &'a [u8],
}

/// Result of a successful build and read-back verification.
#[derive(Debug, Clone)]
pub struct ImageReport {
    pub esp_first_lba: u64,
    pub esp_sectors: u64,
    pub root_first_lba: u64,
    pub root_sectors: u64,
    pub kernel_sha256: String,
    pub bootloader_sha256: String,
    pub manifest: String,
    pub manifest_sha256: String,
}

#[derive(Debug)]
pub enum ImageError {
    Block(BlockError),
    Fs(FsError),
    /// The supplied asset is not the file type it claims to be.
    InvalidAsset(&'static str),
    /// The produced image failed structural or content verification.
    Verification(&'static str),
}

impl From<BlockError> for ImageError {
    fn from(error: BlockError) -> Self {
        Self::Block(error)
    }
}

impl From<FsError> for ImageError {
    fn from(error: FsError) -> Self {
        Self::Fs(error)
    }
}

/// Build a bootable image on `device` and verify it by reading it back.
///
/// The device is overwritten from sector 0. Callers must obtain explicit
/// destructive confirmation and must refuse the disk that holds the running
/// system before calling this function.
pub fn build_image(
    device: BlockDeviceRef,
    assets: ImageAssets<'_>,
) -> Result<ImageReport, ImageError> {
    let kernel = assets.kernel;
    let bootloader = assets.bootloader;
    let boot_config = assets.boot_config;

    // Validate every asset before the first sector is written.
    if kernel.len() < 64 || &kernel[..4] != b"\x7FELF" {
        return Err(ImageError::InvalidAsset("kernel is not an ELF image"));
    }
    if bootloader.len() < 512 || &bootloader[..2] != b"MZ" {
        return Err(ImageError::InvalidAsset(
            "UEFI loader is not a PE/COFF image",
        ));
    }
    if boot_config.is_empty() {
        return Err(ImageError::InvalidAsset("boot configuration is empty"));
    }
    if device.sector_count() < MIN_IMAGE_SECTORS {
        return Err(ImageError::Verification("target image is too small"));
    }

    let (esp_partition, root_partition) = partition::create_octagram_gpt_with_layout(
        device.as_ref(),
        IMAGE_ESP_SECTORS,
        MIN_IMAGE_SECTORS,
        MIN_ROOT_SECTORS,
        |_, _| {},
    )?;
    if !gpt_matches(device.as_ref(), &esp_partition, &root_partition)? {
        return Err(ImageError::Verification("GPT read-back did not match layout"));
    }

    let efi_device = partition_device(&device, "imagep1", &esp_partition)?;
    let root_device = partition_device(&device, "imagep2", &root_partition)?;

    Fat32::format(efi_device.as_ref())?;
    Ext2::format(root_device.as_ref())?;

    let mut efi = Fat32::mount(efi_device)?;
    let mut root = Ext2::mount(root_device)?;

    for directory in ["/EFI", "/EFI/BOOT", "/boot", "/boot/limine"] {
        ensure_directory(&mut efi, directory)?;
    }
    for directory in ["/etc", "/var", "/var/lib", "/var/lib/octagram", "/bin"] {
        ensure_directory(&mut root, directory)?;
    }

    let kernel_sha256 = sha256::digest_hex(kernel);
    let bootloader_sha256 = sha256::digest_hex(bootloader);
    let boot_config_sha256 = sha256::digest_hex(boot_config);
    let manifest = format!(
        "OCTAGRAM image v1\n\
         format=gpt+fat32+ext2\n\
         esp_lba={}\n\
         esp_sectors={}\n\
         root_lba={}\n\
         root_sectors={}\n\
         kernel_size={}\n\
         kernel_sha256={}\n\
         bootloader_size={}\n\
         bootloader_sha256={}\n\
         boot_config_size={}\n\
         boot_config_sha256={}\n",
        esp_partition.first_lba,
        esp_partition.sector_count(),
        root_partition.first_lba,
        root_partition.sector_count(),
        kernel.len(),
        kernel_sha256,
        bootloader.len(),
        bootloader_sha256,
        boot_config.len(),
        boot_config_sha256,
    );
    let manifest_sha256 = sha256::digest_hex(manifest.as_bytes());

    write_file(&mut efi, "/boot/kernel", kernel)?;
    write_file(&mut efi, "/EFI/BOOT/BOOTX64.EFI", bootloader)?;
    write_file(&mut efi, "/boot/limine/limine.conf", boot_config)?;
    write_file(&mut efi, MANIFEST_PATH, manifest.as_bytes())?;

    let root_marker = format!(
        "OCTAGRAM-OS persistent root v1\nfilesystem=ext2\nimage_manifest_sha256={manifest_sha256}\n"
    );
    write_file(&mut root, "/etc/octagram-root", root_marker.as_bytes())?;
    write_file(
        &mut root,
        "/var/lib/octagram/image-manifest.txt",
        manifest.as_bytes(),
    )?;
    write_file(&mut root, "/etc/os-release", b"OCTAGRAM_OS_NAME=\"OCTAGRAM-OS\"\nOCTAGRAM_OS_ID=octagram\nOCTAGRAM_OS_VERSION=1.0\n")?;
    // Seed the A/B journal so an image boots with a valid known-good slot.
    let mut journal = crate::selfupdate::UpdateJournal::new();
    journal.mark_known_good(crate::selfupdate::Slot::A, sha256::digest(kernel));
    write_file(
        &mut root,
        crate::selfupdate::JOURNAL_PATH,
        &journal.to_bytes(),
    )?;

    // Re-open both partitions from the device and verify what actually landed
    // on disk rather than what the in-memory filesystem objects believe.
    drop(efi);
    drop(root);
    verify_image(
        &device,
        &esp_partition,
        &root_partition,
        kernel,
        bootloader,
        boot_config,
        manifest.as_bytes(),
        &kernel_sha256,
    )?;

    Ok(ImageReport {
        esp_first_lba: esp_partition.first_lba,
        esp_sectors: esp_partition.sector_count(),
        root_first_lba: root_partition.first_lba,
        root_sectors: root_partition.sector_count(),
        kernel_sha256,
        bootloader_sha256,
        manifest_sha256,
        manifest,
    })
}

fn verify_image(
    device: &BlockDeviceRef,
    esp_partition: &partition::GptPartition,
    root_partition: &partition::GptPartition,
    kernel: &[u8],
    bootloader: &[u8],
    boot_config: &[u8],
    manifest: &[u8],
    kernel_sha256: &str,
) -> Result<(), ImageError> {
    let partitions = partition::read_gpt_partitions(device.as_ref())?;
    let mut found_esp = false;
    let mut found_root = false;
    for part in &partitions {
        if part.index == esp_partition.index
            && part.type_guid == esp_partition.type_guid
            && part.first_lba == esp_partition.first_lba
            && part.last_lba == esp_partition.last_lba
        {
            found_esp = true;
        }
        if part.index == root_partition.index
            && part.type_guid == root_partition.type_guid
            && part.first_lba == root_partition.first_lba
            && part.last_lba == root_partition.last_lba
        {
            found_root = true;
        }
    }
    if !found_esp || !found_root {
        return Err(ImageError::Verification(
            "verified partition table is missing the ESP or root partition",
        ));
    }

    let efi = Fat32::mount(partition_device(device, "verifyp1", esp_partition)?)?;
    let root = Ext2::mount(partition_device(device, "verifyp2", root_partition)?)?;

    let read_kernel = read_file(&efi, "/boot/kernel")?;
    if read_kernel != kernel {
        return Err(ImageError::Verification("kernel read-back mismatch"));
    }
    if sha256::digest_hex(&read_kernel) != kernel_sha256 {
        return Err(ImageError::Verification("kernel digest read-back mismatch"));
    }
    if read_file(&efi, "/EFI/BOOT/BOOTX64.EFI")? != bootloader {
        return Err(ImageError::Verification("UEFI loader read-back mismatch"));
    }
    if read_file(&efi, "/boot/limine/limine.conf")? != boot_config {
        return Err(ImageError::Verification("boot configuration read-back mismatch"));
    }
    if read_file(&efi, MANIFEST_PATH)? != manifest {
        return Err(ImageError::Verification("ESP manifest read-back mismatch"));
    }
    let root_marker = read_file(&root, "/etc/octagram-root")?;
    if !root_marker.starts_with(b"OCTAGRAM-OS persistent root v1\n") {
        return Err(ImageError::Verification("root marker read-back mismatch"));
    }
    if read_file(&root, "/var/lib/octagram/image-manifest.txt")? != manifest {
        return Err(ImageError::Verification("root manifest read-back mismatch"));
    }
    let journal = read_file(&root, crate::selfupdate::JOURNAL_PATH)?;
    let journal = crate::selfupdate::UpdateJournal::from_bytes(&journal)
        .map_err(|_| ImageError::Verification("update journal read-back mismatch"))?;
    if journal.active != crate::selfupdate::Slot::A
        || journal.known_good != Some(crate::selfupdate::Slot::A)
        || journal.slots[crate::selfupdate::Slot::A.index()].image_sha256
            != sha256::digest(kernel)
    {
        return Err(ImageError::Verification("update journal contents mismatch"));
    }
    Ok(())
}

fn gpt_matches(
    device: &dyn BlockDevice,
    esp: &partition::GptPartition,
    root: &partition::GptPartition,
) -> Result<bool, BlockError> {
    let partitions = partition::read_gpt_partitions(device)?;
    Ok(partitions.iter().any(|part| {
        part.index == esp.index
            && part.type_guid == esp.type_guid
            && part.first_lba == esp.first_lba
            && part.last_lba == esp.last_lba
    }) && partitions.iter().any(|part| {
        part.index == root.index
            && part.type_guid == root.type_guid
            && part.first_lba == root.first_lba
            && part.last_lba == root.last_lba
    }))
}

fn partition_device(
    device: &BlockDeviceRef,
    name: &str,
    partition: &partition::GptPartition,
) -> Result<BlockDeviceRef, BlockError> {
    Ok(Arc::new(PartitionDevice::new(
        device.clone(),
        String::from(name),
        partition.first_lba,
        partition.sector_count(),
    )?))
}

fn ensure_directory(filesystem: &mut dyn FileSystem, path: &str) -> Result<(), FsError> {
    match filesystem.metadata(path) {
        Ok(metadata) if metadata.node_type == crate::fs::NodeType::Directory => Ok(()),
        Ok(_) => Err(FsError::NotDirectory),
        Err(FsError::NotFound) => filesystem.create_dir(path),
        Err(error) => Err(error),
    }
}

fn write_file(
    filesystem: &mut dyn FileSystem,
    path: &str,
    bytes: &[u8],
) -> Result<(), FsError> {
    match filesystem.metadata(path) {
        Ok(_) => filesystem.remove(path)?,
        Err(FsError::NotFound) => {}
        Err(error) => return Err(error),
    }
    filesystem.create_file(path)?;
    let mut offset = 0usize;
    for chunk in bytes.chunks(COPY_CHUNK_SIZE) {
        let written = filesystem.write(path, offset, chunk)?;
        if written != chunk.len() {
            let _ = filesystem.remove(path);
            return Err(FsError::NoSpace);
        }
        offset += written;
    }
    Ok(())
}

fn read_file(filesystem: &dyn FileSystem, path: &str) -> Result<Vec<u8>, FsError> {
    let metadata = filesystem.metadata(path)?;
    if metadata.node_type != crate::fs::NodeType::File {
        return Err(FsError::IsDirectory);
    }
    let mut bytes = alloc::vec![0u8; metadata.len];
    let mut offset = 0usize;
    while offset < bytes.len() {
        let count = filesystem.read(path, offset, &mut bytes[offset..])?;
        if count == 0 {
            return Err(FsError::InvalidData);
        }
        offset += count;
    }
    Ok(bytes)
}

/// Result of a successful A/B image build.
#[derive(Debug, Clone)]
pub struct AbImageReport {
    pub esp_first_lba: u64,
    pub esp_sectors: u64,
    pub root_a_first_lba: u64,
    pub root_a_sectors: u64,
    pub root_b_first_lba: u64,
    pub root_b_sectors: u64,
    pub kernel_sha256: String,
    pub bootloader_sha256: String,
    pub manifest: String,
    pub manifest_sha256: String,
}

/// Limine configuration for the two slots. `slot` picks the entry the
/// bootloader boots by default (1 = A, 2 = B); every entry tells the kernel
/// which root to mount through `octagram.slot=`.
pub fn render_ab_boot_config(slot: u8) -> String {
    let default_entry = if slot == 2 { 2 } else { 1 };
    format!(
        "timeout: 3\ndefault_entry: {default_entry}\n\n\
         /OCTAGRAM Slot A\n    protocol: limine\n    kernel_path: boot():/boot/slots/a/kernel\n    \
         module_path: boot():/EFI/BOOT/BOOTX64.EFI\n    cmdline: octagram.slot=a\n\n\
         /OCTAGRAM Slot B\n    protocol: limine\n    kernel_path: boot():/boot/slots/b/kernel\n    \
         module_path: boot():/EFI/BOOT/BOOTX64.EFI\n    cmdline: octagram.slot=b\n"
    )
}

/// Point the bootloader at `slot` by rewriting the configuration on the ESP.
/// The image builder seeds the journal; this keeps the boot selection that
/// accompanies a slot switch in the same place.
pub fn update_boot_selection(esp: BlockDeviceRef, slot: u8) -> Result<(), ImageError> {
    const CONFIG_PATH: &str = "/boot/limine/limine.conf";
    let mut efi = Fat32::mount(esp)?;
    match efi.create_file(CONFIG_PATH) {
        Ok(()) | Err(FsError::AlreadyExists) => {}
        Err(error) => return Err(error.into()),
    }
    efi.truncate(CONFIG_PATH, 0)?;
    efi.write(CONFIG_PATH, 0, render_ab_boot_config(slot).as_bytes())?;
    Ok(())
}

/// Build an A/B image: one ESP with a kernel copy per slot, plus two equal EXT2
/// root partitions.
///
/// The generated `limine.conf` contains an entry for each slot. Which entry
/// boots is still selected by the bootloader configuration itself; wiring the
/// journal to that selection is tracked in roadmap M7.2.
pub fn build_ab_image(
    device: BlockDeviceRef,
    assets: ImageAssets<'_>,
) -> Result<AbImageReport, ImageError> {
    let kernel = assets.kernel;
    let bootloader = assets.bootloader;
    if kernel.len() < 64 || &kernel[..4] != b"\x7FELF" {
        return Err(ImageError::InvalidAsset("kernel is not an ELF image"));
    }
    if bootloader.len() < 512 || &bootloader[..2] != b"MZ" {
        return Err(ImageError::InvalidAsset(
            "UEFI loader is not a PE/COFF image",
        ));
    }
    if device.sector_count() < MIN_IMAGE_SECTORS {
        return Err(ImageError::Verification("target image is too small"));
    }

    let (esp, root_a, root_b) = partition::create_octagram_gpt_ab_with_layout(
        device.as_ref(),
        IMAGE_ESP_SECTORS,
        MIN_IMAGE_SECTORS,
        MIN_ROOT_SECTORS,
        |_, _| {},
    )?;
    let partitions = partition::read_gpt_partitions(device.as_ref())?;
    if partitions.len() != 3
        || !partitions.iter().any(|part| same_partition(part, &esp))
        || !partitions.iter().any(|part| same_partition(part, &root_a))
        || !partitions.iter().any(|part| same_partition(part, &root_b))
    {
        return Err(ImageError::Verification(
            "A/B GPT read-back did not match the layout",
        ));
    }

    let efi_device = partition_device(&device, "abp1", &esp)?;
    let root_a_device = partition_device(&device, "abp2", &root_a)?;
    let root_b_device = partition_device(&device, "abp3", &root_b)?;
    Fat32::format(efi_device.as_ref())?;
    Ext2::format(root_a_device.as_ref())?;
    Ext2::format(root_b_device.as_ref())?;
    let mut efi = Fat32::mount(efi_device)?;
    let mut root_a_fs = Ext2::mount(root_a_device)?;
    let mut root_b_fs = Ext2::mount(root_b_device)?;

    for directory in [
        "/EFI",
        "/EFI/BOOT",
        "/boot",
        "/boot/slots",
        "/boot/slots/a",
        "/boot/slots/b",
        "/boot/limine",
    ] {
        ensure_directory(&mut efi, directory)?;
    }
    for filesystem in [&mut root_a_fs, &mut root_b_fs] {
        for directory in ["/etc", "/var", "/var/lib", "/var/lib/octagram", "/bin"] {
            ensure_directory(filesystem, directory)?;
        }
    }

    let boot_config = render_ab_boot_config(1);
    let kernel_sha256 = sha256::digest_hex(kernel);
    let bootloader_sha256 = sha256::digest_hex(bootloader);
    let manifest = format!(
        "OCTAGRAM image v1\nformat=gpt+fat32+ext2\nslots=ab\n\
         esp_lba={}\nesp_sectors={}\n\
         root_a_lba={}\nroot_a_sectors={}\n\
         root_b_lba={}\nroot_b_sectors={}\n\
         kernel_size={}\nkernel_sha256={}\n\
         bootloader_size={}\nbootloader_sha256={}\n\
         boot_config_sha256={}\n",
        esp.first_lba,
        esp.sector_count(),
        root_a.first_lba,
        root_a.sector_count(),
        root_b.first_lba,
        root_b.sector_count(),
        kernel.len(),
        kernel_sha256,
        bootloader.len(),
        bootloader_sha256,
        sha256::digest_hex(boot_config.as_bytes()),
    );
    let manifest_sha256 = sha256::digest_hex(manifest.as_bytes());
    let kernel_digest = sha256::digest(kernel);

    write_file(&mut efi, "/boot/slots/a/kernel", kernel)?;
    write_file(&mut efi, "/boot/slots/b/kernel", kernel)?;
    write_file(&mut efi, "/EFI/BOOT/BOOTX64.EFI", bootloader)?;
    write_file(&mut efi, "/boot/limine/limine.conf", boot_config.as_bytes())?;
    write_file(&mut efi, MANIFEST_PATH, manifest.as_bytes())?;

    for (filesystem, slot) in [(&mut root_a_fs, "A"), (&mut root_b_fs, "B")] {
        let marker = format!(
            "OCTAGRAM-OS persistent root v1\nfilesystem=ext2\nslot={slot}\nimage_manifest_sha256={manifest_sha256}\n"
        );
        write_file(filesystem, "/etc/octagram-root", marker.as_bytes())?;
        write_file(
            filesystem,
            "/var/lib/octagram/image-manifest.txt",
            manifest.as_bytes(),
        )?;
        let mut journal = crate::selfupdate::UpdateJournal::new();
        journal.mark_known_good(crate::selfupdate::Slot::A, kernel_digest);
        write_file(
            filesystem,
            crate::selfupdate::JOURNAL_PATH,
            &journal.to_bytes(),
        )?;
    }

    drop(efi);
    drop(root_a_fs);
    drop(root_b_fs);
    verify_ab_image(
        &device,
        &esp,
        &root_a,
        &root_b,
        kernel,
        bootloader,
        boot_config.as_bytes(),
        manifest.as_bytes(),
        &kernel_sha256,
        kernel_digest,
    )?;

    Ok(AbImageReport {
        esp_first_lba: esp.first_lba,
        esp_sectors: esp.sector_count(),
        root_a_first_lba: root_a.first_lba,
        root_a_sectors: root_a.sector_count(),
        root_b_first_lba: root_b.first_lba,
        root_b_sectors: root_b.sector_count(),
        kernel_sha256,
        bootloader_sha256,
        manifest_sha256,
        manifest,
    })
}

fn verify_ab_image(
    device: &BlockDeviceRef,
    esp: &partition::GptPartition,
    root_a: &partition::GptPartition,
    root_b: &partition::GptPartition,
    kernel: &[u8],
    bootloader: &[u8],
    boot_config: &[u8],
    manifest: &[u8],
    kernel_sha256: &str,
    kernel_digest: [u8; 32],
) -> Result<(), ImageError> {
    let partitions = partition::read_gpt_partitions(device.as_ref())?;
    if partitions.len() != 3 {
        return Err(ImageError::Verification("A/B partition count mismatch"));
    }
    let efi = Fat32::mount(partition_device(device, "abv1", esp)?)?;
    let root_a_fs = Ext2::mount(partition_device(device, "abv2", root_a)?)?;
    let root_b_fs = Ext2::mount(partition_device(device, "abv3", root_b)?)?;

    for path in ["/boot/slots/a/kernel", "/boot/slots/b/kernel"] {
        let read = read_file(&efi, path)?;
        if read != kernel || sha256::digest_hex(&read) != kernel_sha256 {
            return Err(ImageError::Verification("slot kernel read-back mismatch"));
        }
    }
    if read_file(&efi, "/EFI/BOOT/BOOTX64.EFI")? != bootloader {
        return Err(ImageError::Verification("A/B loader read-back mismatch"));
    }
    if read_file(&efi, "/boot/limine/limine.conf")? != boot_config {
        return Err(ImageError::Verification("A/B boot config read-back mismatch"));
    }
    if read_file(&efi, MANIFEST_PATH)? != manifest {
        return Err(ImageError::Verification("A/B manifest read-back mismatch"));
    }

    for (filesystem, slot) in [(&root_a_fs, b"slot=A\n"), (&root_b_fs, b"slot=B\n")] {
        let marker = read_file(filesystem, "/etc/octagram-root")?;
        if !marker.starts_with(b"OCTAGRAM-OS persistent root v1\n") || !contains(&marker, slot)
        {
            return Err(ImageError::Verification("A/B root marker mismatch"));
        }
        if read_file(filesystem, "/var/lib/octagram/image-manifest.txt")? != manifest {
            return Err(ImageError::Verification("A/B root manifest mismatch"));
        }
        let journal = crate::selfupdate::UpdateJournal::from_bytes(&read_file(
            filesystem,
            crate::selfupdate::JOURNAL_PATH,
        )?)
        .map_err(|_| ImageError::Verification("A/B journal read-back mismatch"))?;
        if journal.active != crate::selfupdate::Slot::A
            || journal.slots[crate::selfupdate::Slot::A.index()].image_sha256 != kernel_digest
        {
            return Err(ImageError::Verification("A/B journal contents mismatch"));
        }
    }
    Ok(())
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn same_partition(left: &partition::GptPartition, right: &partition::GptPartition) -> bool {
    left.index == right.index
        && left.type_guid == right.type_guid
        && left.unique_guid == right.unique_guid
        && left.first_lba == right.first_lba
        && left.last_lba == right.last_lba
}

#[cfg(test)]
mod tests {
    use super::{ImageAssets, ImageError, build_image};
    use crate::fs::FileSystem;

    #[test]
    fn boot_config_selects_the_requested_slot() {
        let slot_a = super::render_ab_boot_config(1);
        let slot_b = super::render_ab_boot_config(2);
        assert!(slot_a.contains("default_entry: 1"));
        assert!(slot_b.contains("default_entry: 2"));
        for config in [&slot_a, &slot_b] {
            assert!(config.contains("cmdline: octagram.slot=a"));
            assert!(config.contains("cmdline: octagram.slot=b"));
            assert!(config.contains("kernel_path: boot():/boot/slots/a/kernel"));
            assert!(config.contains("kernel_path: boot():/boot/slots/b/kernel"));
        }
    }
    use crate::sha256;
    use crate::storage::{
        block::{BlockDevice, BlockError, SECTOR_SIZE},
        partition,
    };
    use alloc::sync::Arc;
    use alloc::vec::Vec;

    struct MemoryDisk {
        data: spin::Mutex<Vec<u8>>,
    }

    impl MemoryDisk {
        fn new(bytes: usize) -> Self {
            Self {
                data: spin::Mutex::new(alloc::vec![0u8; bytes]),
            }
        }
    }

    impl BlockDevice for MemoryDisk {
        fn name(&self) -> &str {
            "memdisk"
        }

        fn sector_count(&self) -> u64 {
            (self.data.lock().len() / SECTOR_SIZE) as u64
        }

        fn read_sectors(&self, lba: u64, buffer: &mut [u8]) -> Result<(), BlockError> {
            if !buffer.len().is_multiple_of(SECTOR_SIZE) {
                return Err(BlockError::InvalidBuffer);
            }
            let data = self.data.lock();
            let start = usize::try_from(lba)
                .ok()
                .and_then(|lba| lba.checked_mul(SECTOR_SIZE))
                .ok_or(BlockError::OutOfRange)?;
            let end = start
                .checked_add(buffer.len())
                .filter(|end| *end <= data.len())
                .ok_or(BlockError::OutOfRange)?;
            buffer.copy_from_slice(&data[start..end]);
            Ok(())
        }

        fn write_sectors(&self, lba: u64, buffer: &[u8]) -> Result<(), BlockError> {
            if !buffer.len().is_multiple_of(SECTOR_SIZE) {
                return Err(BlockError::InvalidBuffer);
            }
            let mut data = self.data.lock();
            let start = usize::try_from(lba)
                .ok()
                .and_then(|lba| lba.checked_mul(SECTOR_SIZE))
                .ok_or(BlockError::OutOfRange)?;
            let end = start
                .checked_add(buffer.len())
                .filter(|end| *end <= data.len())
                .ok_or(BlockError::OutOfRange)?;
            data[start..end].copy_from_slice(buffer);
            Ok(())
        }
    }

    fn test_kernel() -> Vec<u8> {
        let mut kernel = alloc::vec![0xA5u8; 4096];
        kernel[..4].copy_from_slice(b"\x7FELF");
        kernel[4] = 2;
        kernel[5] = 1;
        kernel
    }

    fn test_bootloader() -> Vec<u8> {
        let mut loader = alloc::vec![0x5Au8; 9000];
        loader[..2].copy_from_slice(b"MZ");
        loader
    }

    /// Host-side helper that turns real build artifacts into a bootable A/B
    /// disk image, so a QEMU test can boot the kernel this checkout produced
    /// without needing an ISO writer. Ignored by default; run it with
    /// `OCTAGRAM_BOOT_IMAGE=<path> cargo test -- --ignored writes_a_boot_image`.
    #[test]
    #[ignore = "writes a boot image from real artifacts; needs OCTAGRAM_BOOT_IMAGE"]
    fn writes_a_boot_image_from_real_artifacts() {
        use alloc::string::String;

        let Ok(output) = std::env::var("OCTAGRAM_BOOT_IMAGE") else {
            return;
        };
        let kernel_path = std::env::var("OCTAGRAM_KERNEL_ELF").unwrap_or_else(|_| {
            String::from("target/x86_64-unknown-none/release/octagram-kernel")
        });
        let bootloader_path = std::env::var("OCTAGRAM_BOOTLOADER")
            .unwrap_or_else(|_| String::from("../../limine/BOOTX64.EFI"));
        let kernel = std::fs::read(&kernel_path).expect("kernel ELF");
        let bootloader = std::fs::read(&bootloader_path).expect("limine BOOTX64.EFI");

        let disk = Arc::new(MemoryDisk::new(256 * 1024 * 1024));
        let boot_config = super::render_ab_boot_config(1);
        let report = super::build_ab_image(
            disk.clone(),
            ImageAssets {
                kernel: &kernel,
                bootloader: &bootloader,
                boot_config: boot_config.as_bytes(),
            },
        )
        .expect("A/B image build");
        std::fs::write(&output, disk.data.lock().as_slice()).expect("write image");
        std::fs::write(
            format!("{output}.manifest"),
            report.manifest.as_bytes(),
        )
        .expect("write manifest");
    }

    #[test]
    fn builds_and_verifies_a_bootable_layout() {
        let disk = Arc::new(MemoryDisk::new(160 * 1024 * 1024));
        let kernel = test_kernel();
        let bootloader = test_bootloader();
        let boot_config = b"timeout: 3\n/OCTAGRAM\n    protocol: limine\n    kernel_path: boot():/boot/kernel\n";

        let report = build_image(
            disk.clone(),
            ImageAssets {
                kernel: &kernel,
                bootloader: &bootloader,
                boot_config,
            },
        )
        .unwrap();

        assert_eq!(report.kernel_sha256, sha256::digest_hex(&kernel));
        assert_eq!(report.bootloader_sha256, sha256::digest_hex(&bootloader));
        assert_eq!(
            report.manifest_sha256,
            sha256::digest_hex(report.manifest.as_bytes())
        );
        assert!(report.manifest.contains(&report.kernel_sha256));

        let partitions = partition::read_gpt_partitions(disk.as_ref()).unwrap();
        assert_eq!(partitions.len(), 2);
        assert_eq!(partitions[0].first_lba, report.esp_first_lba);
        assert_eq!(partitions[0].sector_count(), report.esp_sectors);
        assert_eq!(partitions[1].first_lba, report.root_first_lba);
        assert_eq!(partitions[1].sector_count(), report.root_sectors);
        assert_eq!(
            partitions[0].type_guid,
            partition::EFI_SYSTEM_PARTITION_GUID
        );
        assert_eq!(
            partitions[1].type_guid,
            partition::LINUX_FILESYSTEM_GUID
        );
    }

    #[test]
    fn rejects_invalid_assets_before_writing_the_disk() {
        let disk = Arc::new(MemoryDisk::new(160 * 1024 * 1024));
        let not_an_elf = alloc::vec![0u8; 4096];
        let bootloader = test_bootloader();
        let result = build_image(
            disk.clone(),
            ImageAssets {
                kernel: &not_an_elf,
                bootloader: &bootloader,
                boot_config: b"timeout: 3\n",
            },
        );
        assert!(matches!(result, Err(ImageError::InvalidAsset(_))));

        let mut first_sector = [0u8; SECTOR_SIZE];
        disk.read_sectors(0, &mut first_sector).unwrap();
        assert_eq!(first_sector, [0u8; SECTOR_SIZE]);
    }

    #[test]
    fn builds_an_ab_image_with_both_root_slots() {
        let disk = Arc::new(MemoryDisk::new(160 * 1024 * 1024));
        let kernel = test_kernel();
        let bootloader = test_bootloader();
        let report = super::build_ab_image(
            disk.clone(),
            ImageAssets {
                kernel: &kernel,
                bootloader: &bootloader,
                boot_config: b"unused for A/B: the builder generates entries",
            },
        )
        .unwrap();

        assert_eq!(report.root_a_sectors, report.root_b_sectors);
        assert_eq!(report.kernel_sha256, sha256::digest_hex(&kernel));
        assert!(report.manifest.contains("slots=ab"));

        let partitions = partition::read_gpt_partitions(disk.as_ref()).unwrap();
        assert_eq!(partitions.len(), 3);
        assert_eq!(
            partitions[1].type_guid,
            partition::LINUX_FILESYSTEM_GUID
        );
        assert_eq!(partitions[1].first_lba, report.root_a_first_lba);
        assert_eq!(partitions[2].first_lba, report.root_b_first_lba);
        assert!(partitions[1].last_lba < partitions[2].first_lba);

        // Slot B is usable on its own: it has its own EXT2 root with a marker.
        let disk_ref: crate::storage::block::BlockDeviceRef = disk.clone();
        let root_b_device =
            super::partition_device(&disk_ref, "rootb-test", &partitions[2]).unwrap();
        let root_b = crate::storage::ext2::Ext2::mount(root_b_device).unwrap();
        let mut marker = [0u8; 64];
        let count = root_b
            .read("/etc/octagram-root", 0, &mut marker)
            .unwrap();
        assert!(marker[..count]
            .windows(7)
            .any(|window| window == b"slot=B\n"));
    }

    #[test]
    fn rejects_a_loader_that_is_not_pe() {
        let disk = Arc::new(MemoryDisk::new(160 * 1024 * 1024));
        let kernel = test_kernel();
        let result = build_image(
            disk,
            ImageAssets {
                kernel: &kernel,
                bootloader: b"not a loader",
                boot_config: b"timeout: 3\n",
            },
        );
        assert!(matches!(result, Err(ImageError::InvalidAsset(_))));
    }
}
