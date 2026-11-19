//! In-kernel installer used from the OCTAGRAM Live shell.
//!
//! Run installer to list target disks, then use the explicit destructive
//! confirmation: installer sata0 INSTALL-OCTAGRAM-ERASE.

use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;

use crate::fs::{FileSystem, FsError};
use crate::storage::{
    block::{self, BlockDevice, BlockDeviceRef},
    ext2::Ext2,
    fat32::Fat32,
    partition::{self, PartitionDevice},
};

const CONFIRMATION: &str = "INSTALL-OCTAGRAM-ERASE";
const COPY_CHUNK_SIZE: usize = 32 * 1024;
const MINIMUM_ROOT_BYTES: u64 = 4 * 1024 * 1024 * 1024;

pub fn run(arguments: &str) {
    let mut parts = arguments.split_whitespace();
    let Some(device_name) = parts.next() else {
        print_devices();
        crate::println!("");
        crate::println!("Usage: installer <sata-device> {CONFIRMATION}");
        crate::println!("This erases the selected SATA disk and installs OCTAGRAM-OS.");
        return;
    };

    if device_name == "list" {
        print_devices();
        return;
    }

    let Some(confirmation) = parts.next() else {
        crate::println!("installer: destructive confirmation is required");
        crate::println!("Usage: installer {device_name} {CONFIRMATION}");
        return;
    };
    if parts.next().is_some() || confirmation != CONFIRMATION {
        crate::println!("installer: confirmation did not match");
        crate::println!("Usage: installer {device_name} {CONFIRMATION}");
        return;
    }

    match install_to_disk(device_name) {
        Ok(()) => {
            crate::println!("");
            crate::println!("OCTAGRAM-OS installation completed.");
            crate::println!("Restart, remove the Live USB, and select the internal disk in UEFI.");
            crate::println!("The first boot will populate built-in files on the persistent root.");
        }
        Err(error) => crate::println!("installer: {error}"),
    }
}

fn print_devices() {
    crate::println!("Available storage devices:");
    let mut found = false;
    for info in block::list() {
        let gib_tenths = (info.sectors as u128 * 512 * 10) / (1024 * 1024 * 1024);
        let target = info.name.starts_with("sata");
        let root = crate::fs::persistent_root_device()
            .is_some_and(|current| current == info.name);
        let source = block::get(&info.name)
            .is_some_and(|device| is_boot_source_device(device.as_ref()));
        let status = if root {
            " [CURRENT ROOT - PROTECTED]"
        } else if source {
            " [BOOT SOURCE - PROTECTED]"
        } else if target {
            " [INSTALL TARGET]"
        } else {
            " [not an installer target]"
        };
        crate::println!(
            "  {:<10} {}.{} GiB{}",
            info.name,
            gib_tenths / 10,
            gib_tenths % 10,
            status
        );
        found = true;
    }
    if !found {
        crate::println!("  no block devices detected");
    }
    crate::println!("Only internal SATA/AHCI disks are accepted by this installer.");
}

fn install_to_disk(device_name: &str) -> Result<(), String> {
    if !device_name.starts_with("sata") {
        return Err(String::from("only internal SATA/AHCI devices (sataN) are supported"));
    }

    if crate::fs::persistent_root_device().as_deref() == Some(device_name) {
        return Err(String::from("refusing to erase the disk that currently contains /"));
    }

    let device = block::get(device_name)
        .ok_or_else(|| format!("storage device {device_name} was not found; run devices"))?;

    if !boot_source_identity_available() {
        return Err(String::from(
            "Limine did not report the source disk identity; refusing a destructive install because the Live boot disk cannot be protected",
        ));
    }
    if is_boot_source_device(device.as_ref()) {
        return Err(String::from(
            "refusing to erase the disk from which Limine loaded this kernel",
        ));
    }

    // Validate all source assets before changing the partition table.
    let (kernel, efi_loader) = live_boot_files()
        .ok_or_else(|| String::from(
            "Limine did not provide the running kernel and BOOTX64.EFI module; rebuild the Live image with the updated limine.conf",
        ))?;
    if kernel.len() < 4 || &kernel[..4] != b"\x7FELF" {
        return Err(String::from("loaded executable is not an ELF kernel image"));
    }
    if efi_loader.len() < 512 || &efi_loader[..2] != b"MZ" {
        return Err(String::from("preloaded BOOTX64.EFI is missing or invalid"));
    }

    let size_bytes = device.sector_count() as u128 * 512;
    if size_bytes < 16 * 1024 * 1024 * 1024u128 {
        return Err(String::from("target disk must be at least 16 GiB"));
    }
    let gib_tenths = size_bytes * 10 / (1024 * 1024 * 1024);
    crate::println!(
        "Target: {} ({}.{:01} GiB)",
        device_name,
        gib_tenths / 10,
        gib_tenths % 10
    );
    crate::println!("Creating GPT, EFI System Partition and persistent root...");
    crate::terminal::terminal_show_task_progress("Create GPT", 0, 1);

    let (efi_partition, root_partition) =
        partition::create_octagram_gpt(device.as_ref())
            .map_err(|error| format!("GPT partitioning failed: {error:?}"))?;
    crate::terminal::terminal_show_task_progress("Create GPT", 1, 1);
    crate::println!("");
    crate::terminal::terminal_show_task_progress("Verify GPT", 0, 2);
    let written_partitions = partition::read_gpt_partitions(device.as_ref())
        .map_err(|error| format!("GPT read-back failed: {error:?}"))?;
    let efi_verified = written_partitions.iter().any(|part| {
        part.index == efi_partition.index
            && part.type_guid == efi_partition.type_guid
            && part.first_lba == efi_partition.first_lba
            && part.last_lba == efi_partition.last_lba
    });
    let root_verified = written_partitions.iter().any(|part| {
        part.index == root_partition.index
            && part.type_guid == root_partition.type_guid
            && part.first_lba == root_partition.first_lba
            && part.last_lba == root_partition.last_lba
    });
    crate::terminal::terminal_show_task_progress("Verify GPT", 2, 2);
    crate::println!("");
    if !efi_verified || !root_verified {
        return Err(String::from("GPT verification did not find the expected ESP and root partitions"));
    }

    let efi_device: BlockDeviceRef = Arc::new(
        PartitionDevice::new(
            device.clone(),
            format!("{device_name}p1"),
            efi_partition.first_lba,
            efi_partition.sector_count(),
        )
        .map_err(|error| format!("EFI partition view failed: {error:?}"))?,
    );
    let root_device: BlockDeviceRef = Arc::new(
        PartitionDevice::new(
            device.clone(),
            format!("{device_name}p2"),
            root_partition.first_lba,
            root_partition.sector_count(),
        )
        .map_err(|error| format!("root partition view failed: {error:?}"))?,
    );

    if (root_device.sector_count() as u128) * 512 < MINIMUM_ROOT_BYTES as u128 {
        return Err(String::from("root partition is too small"));
    }

    crate::println!("Formatting EFI partition as FAT32...");
    let mut last_fat_report = None;
    let fat_format_result = Fat32::format_with_progress(efi_device.as_ref(), |written, total| {
        let interval = (total / 100).max(64 * 1024);
        if last_fat_report.is_some_and(|last: u64| {
            written.saturating_sub(last) < interval && written < total
        }) {
            return;
        }
        last_fat_report = Some(written);
        crate::terminal::terminal_show_progress("Format EFI FAT32", written, total);
    });
    crate::println!("");
    fat_format_result.map_err(|error| format!("EFI FAT32 format failed: {error:?}"))?;
    crate::println!("");
    crate::println!("Formatting root partition as EXT2...");
    let mut last_reported_bytes = None;
    let root_format_result =
        Ext2::format_with_progress(root_device.as_ref(), |written, total| {
            // Limit terminal redraws to roughly 100 updates on large volumes.
            let report_interval = (total / 100).max(1024 * 1024);
            if let Some(last_reported) = last_reported_bytes
                && written.saturating_sub(last_reported) < report_interval
                && written < total
            {
                return;
            }
            last_reported_bytes = Some(written);
            crate::terminal::terminal_show_progress("EXT2 root", written, total);
        });
    crate::println!("");
    root_format_result.map_err(|error| format!("root EXT2 format failed: {error:?}"))?;

    crate::println!("");
    crate::terminal::terminal_show_task_progress("Mount filesystems", 0, 2);
    let mut efi = Fat32::mount(efi_device)
        .map_err(|error| format!("cannot mount formatted EFI partition: {error:?}"))?;
    crate::terminal::terminal_show_task_progress("Mount filesystems", 1, 2);
    let mut root = Ext2::mount(root_device)
        .map_err(|error| format!("cannot mount formatted root partition: {error:?}"))?;
    crate::terminal::terminal_show_task_progress("Mount filesystems", 2, 2);
    crate::println!("");

    let efi_directories = ["/EFI", "/EFI/BOOT", "/boot", "/boot/limine"];
    for (index, directory) in efi_directories.iter().enumerate() {
        crate::terminal::terminal_show_task_progress("Create EFI directories", index as u64, efi_directories.len() as u64);
        ensure_directory(&mut efi, directory)
            .map_err(|error| format!("creating EFI directory {directory}: {error:?}"))?;
    }
    crate::terminal::terminal_show_task_progress("Create EFI directories", efi_directories.len() as u64, efi_directories.len() as u64);
    crate::println!("");
    let root_directories = [
        "/etc",
        "/home",
        "/home/octagram",
        "/bin",
        "/lib",
        "/lib/tcc",
        "/share",
        "/share/octagram",
        "/include",
        "/include/sys",
        "/tmp",
        "/var",
        "/var/log",
        "/var/lib",
        "/var/lib/octagram",
        "/var/lib/octagram/packages",
        "/opt",
        "/apps",
        "/usr",
        "/usr/bin",
        "/usr/local",
        "/usr/local/bin",
        "/mnt",
    ];
    for (index, directory) in root_directories.iter().enumerate() {
        crate::terminal::terminal_show_task_progress("Create root directories", index as u64, root_directories.len() as u64);
        ensure_directory(&mut root, directory)
            .map_err(|error| format!("creating root directory {directory}: {error:?}"))?;
    }
    crate::terminal::terminal_show_task_progress("Create root directories", root_directories.len() as u64, root_directories.len() as u64);
    crate::println!("");

    crate::println!("Copying kernel ({} bytes)...", kernel.len());
    let kernel_copy_result =
        write_file_with_progress(&mut efi, "/boot/kernel", kernel, "Copy kernel");
    crate::println!("");
    kernel_copy_result.map_err(|error| format!("copying kernel failed: {error:?}"))?;

    crate::println!("Copying Limine UEFI loader ({} bytes)...", efi_loader.len());
    let loader_copy_result = write_file_with_progress(
        &mut efi,
        "/EFI/BOOT/BOOTX64.EFI",
        efi_loader,
        "Copy EFI loader",
    );
    crate::println!("");
    loader_copy_result.map_err(|error| format!("copying UEFI loader failed: {error:?}"))?;
    crate::terminal::terminal_show_task_progress("Write boot config", 0, 1);
    write_file(
        &mut efi,
        "/boot/limine/limine.conf",
        include_bytes!("../../limine.conf"),
    )
    .map_err(|error| format!("writing limine.conf failed: {error:?}"))?;
    crate::terminal::terminal_show_task_progress("Write boot config", 1, 1);
    crate::println!("");

    let root_files: [(&str, &[u8], &str); 6] = [
        ("/etc/octagram-root", b"OCTAGRAM-OS persistent root v1\nfilesystem=ext2\n", "root marker"),
        ("/etc/os-release", b"OCTAGRAM_OS_NAME=\"OCTAGRAM-OS\"\nOCTAGRAM_OS_ID=octagram\nOCTAGRAM_OS_VERSION=1.0\n", "os-release"),
        ("/etc/passwd", b"octagram:x:0:0:OCTAGRAM User:/home/octagram:/bin/sh\n", "passwd"),
        ("/home/octagram/.profile", b"export PATH=/bin:/usr/bin:/usr/local/bin\n", "user profile"),
        ("/apps/apps.txt", b"# OCTAGRAM native application registry\n", "app registry"),
        ("/var/lib/octagram/packages/registry.txt", b"", "package registry"),
    ];
    for (index, (path, contents, description)) in root_files.iter().enumerate() {
        crate::terminal::terminal_show_task_progress("Write root files", index as u64, root_files.len() as u64);
        write_file(&mut root, path, contents)
            .map_err(|error| format!("writing {description} failed: {error:?}"))?;
    }
    crate::terminal::terminal_show_task_progress("Write root files", root_files.len() as u64, root_files.len() as u64);
    crate::println!("");

    crate::println!("Verifying installed filesystems...");
    let efi_paths = ["/boot/kernel", "/EFI/BOOT/BOOTX64.EFI", "/boot/limine/limine.conf"];
    for (index, path) in efi_paths.iter().enumerate() {
        crate::terminal::terminal_show_task_progress("Verify EFI files", index as u64, efi_paths.len() as u64);
        if efi.metadata(path).is_err() {
            return Err(format!("EFI verification failed for {path}"));
        }
    }
    crate::terminal::terminal_show_task_progress("Verify EFI files", efi_paths.len() as u64, efi_paths.len() as u64);
    crate::println!("");
    let root_paths = ["/etc/octagram-root", "/etc/os-release", "/home/octagram/.profile"];
    for (index, path) in root_paths.iter().enumerate() {
        crate::terminal::terminal_show_task_progress("Verify root files", index as u64, root_paths.len() as u64);
        if root.metadata(path).is_err() {
            return Err(format!("root filesystem verification failed for {path}"));
        }
    }
    crate::terminal::terminal_show_task_progress("Verify root files", root_paths.len() as u64, root_paths.len() as u64);
    crate::println!("");
    let read_back_checks: [(&str, &[u8], &str); 3] = [
        ("/boot/kernel", b"\x7FELF", "kernel"),
        ("/EFI/BOOT/BOOTX64.EFI", b"MZ", "UEFI loader"),
        ("/etc/octagram-root", b"OCTAGRAM-OS persistent root v1", "root marker"),
    ];
    for (index, (path, expected, description)) in read_back_checks.iter().enumerate() {
        crate::terminal::terminal_show_task_progress("Read-back checks", index as u64, read_back_checks.len() as u64);
        let filesystem: &dyn FileSystem = if *path == "/etc/octagram-root" { &root } else { &efi };
        verify_prefix(filesystem, path, expected)
            .map_err(|error| format!("{description} read-back verification failed: {error:?}"))?;
    }
    crate::terminal::terminal_show_task_progress("Read-back checks", read_back_checks.len() as u64, read_back_checks.len() as u64);
    crate::println!("");

    crate::println!("EFI: 512 MiB FAT32 at LBA {}", efi_partition.first_lba);
    crate::println!(
        "Root: EXT2 at LBA {} ({} sectors)",
        root_partition.first_lba,
        root_partition.sector_count()
    );
    crate::println!("Root marker and boot files were verified.");
    Ok(())
}

fn ensure_directory(filesystem: &mut dyn FileSystem, path: &str) -> Result<(), FsError> {
    match filesystem.metadata(path) {
        Ok(metadata) if metadata.node_type == crate::fs::NodeType::Directory => Ok(()),
        Ok(_) => Err(FsError::NotDirectory),
        Err(FsError::NotFound) => filesystem.create_dir(path),
        Err(error) => Err(error),
    }
}

fn verify_prefix(
    filesystem: &dyn FileSystem,
    path: &str,
    expected: &[u8],
) -> Result<(), FsError> {
    let mut actual = alloc::vec![0u8; expected.len()];
    let count = filesystem.read(path, 0, &mut actual)?;
    if count != expected.len() || actual.as_slice() != expected {
        return Err(FsError::InvalidData);
    }
    Ok(())
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

fn write_file_with_progress(
    filesystem: &mut dyn FileSystem,
    path: &str,
    bytes: &[u8],
    label: &str,
) -> Result<(), FsError> {
    match filesystem.metadata(path) {
        Ok(_) => filesystem.remove(path)?,
        Err(FsError::NotFound) => {}
        Err(error) => return Err(error),
    }
    filesystem.create_file(path)?;

    let total_bytes = bytes.len() as u64;
    let report_interval = (total_bytes / 50).max(COPY_CHUNK_SIZE as u64);
    let mut last_reported_bytes = None;
    crate::terminal::terminal_show_progress(label, 0, total_bytes);

    let mut offset = 0usize;
    for chunk in bytes.chunks(COPY_CHUNK_SIZE) {
        let written = filesystem.write(path, offset, chunk)?;
        if written != chunk.len() {
            let _ = filesystem.remove(path);
            return Err(FsError::NoSpace);
        }
        offset += written;

        let written_bytes = offset as u64;
        if last_reported_bytes.is_none_or(|last| {
            written_bytes.saturating_sub(last) >= report_interval
        }) || written_bytes == total_bytes
        {
            last_reported_bytes = Some(written_bytes);
            crate::terminal::terminal_show_progress(label, written_bytes, total_bytes);
        }
    }
    Ok(())
}

fn live_boot_files() -> Option<(&'static [u8], &'static [u8])> {
    let executable_response = crate::EXECUTABLE_FILE_REQUEST.get_response()?;
    let executable = executable_response.file();
    let kernel_size = usize::try_from(executable.size()).ok()?;
    if executable.addr().is_null() || kernel_size == 0 {
        return None;
    }
    let kernel = unsafe { core::slice::from_raw_parts(executable.addr(), kernel_size) };

    let modules_response = crate::drivers::wifi_firmware::MODULE_REQUEST.get_response()?;
    let mut loader = None;
    for module in modules_response.modules() {
        if module.path().to_bytes().ends_with(b"/BOOTX64.EFI") {
            let size = usize::try_from(module.size()).ok()?;
            if !module.addr().is_null() && size > 0 {
                loader = Some(unsafe { core::slice::from_raw_parts(module.addr(), size) });
                break;
            }
        }
    }
    Some((kernel, loader?))
}

/// Fail closed when formatting because the source Live disk cannot be identified.
pub(crate) fn ensure_safe_format_target(device: &dyn BlockDevice) -> Result<(), &'static str> {
    if !boot_source_identity_available() {
        return Err("Limine did not report the source disk identity; refusing to format a disk that might contain the Live system");
    }
    if is_boot_source_device(device) {
        return Err("refusing to format the disk from which Limine loaded this kernel");
    }
    Ok(())
}

fn boot_source_identity_available() -> bool {
    crate::EXECUTABLE_FILE_REQUEST
        .get_response()
        .is_some_and(|response| {
            let file = response.file();
            file.gpt_disk_id().is_some() || file.mbr_disk_id().is_some()
        })
}

fn is_boot_source_device(device: &dyn BlockDevice) -> bool {
    let Some(response) = crate::EXECUTABLE_FILE_REQUEST.get_response() else {
        return false;
    };
    let file = response.file();

    if let Some(source_guid) = file.gpt_disk_id() {
        let mut header = [0u8; 512];
        if device.read_sectors(1, &mut header).is_ok() && &header[..8] == b"EFI PART" {
            let mut guid_le = [0u8; 16];
            guid_le[..4].copy_from_slice(&source_guid.a.to_le_bytes());
            guid_le[4..6].copy_from_slice(&source_guid.b.to_le_bytes());
            guid_le[6..8].copy_from_slice(&source_guid.c.to_le_bytes());
            guid_le[8..16].copy_from_slice(&source_guid.d);
            let mut guid_be = [0u8; 16];
            guid_be[..4].copy_from_slice(&source_guid.a.to_be_bytes());
            guid_be[4..6].copy_from_slice(&source_guid.b.to_be_bytes());
            guid_be[6..8].copy_from_slice(&source_guid.c.to_be_bytes());
            guid_be[8..16].copy_from_slice(&source_guid.d);
            // Be defensive about the GUID field byte order used by firmware /
            // bootloader implementations; either representation protects the
            // source disk from being selected as the installation target.
            if &header[56..72] == &guid_le[..] || &header[56..72] == &guid_be[..] {
                return true;
            }
        }
    }

    if let Some(source_mbr_id) = file.mbr_disk_id() {
        let mut mbr = [0u8; 512];
        if device.read_sectors(0, &mut mbr).is_ok() {
            let disk_id = u32::from_le_bytes(mbr[440..444].try_into().unwrap());
            if disk_id == source_mbr_id.get() {
                return true;
            }
        }
    }
    false
}
