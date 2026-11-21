//! Virtual filesystem and the in-memory root filesystem.
//!
//! The VFS provides path normalization, mount routing, and a small common
//! filesystem interface. The initial root is a [`RamFs`], so its contents are
//! volatile until a block-device driver and a persistent filesystem exist.

pub(crate) use devfs::{Termios, console_termios, discard_pending_input, set_console_termios};
mod devfs;
mod ramfs;

pub(crate) use devfs::{
    DevFs, device_access, is_fb_path, is_tty_path, poll_tty, pump_console_input, read_tty,
    tty_path_readable, tty_path_terminal, tty_readable,
};
pub use ramfs::RamFs;

use crate::storage::{
    block,
    ext2::Ext2,
    fat32::Fat32,
    partition::{self, LINUX_FILESYSTEM_GUID, PartitionDevice},
};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
#[cfg(test)]
use core::sync::atomic::{AtomicU64, Ordering};
use spin::{Mutex, Once};

const MAKE_ELF: &[u8] = include_bytes!("../../../user/rust/build/make.elf");
const GNU_MAKE_ELF: &[u8] = include_bytes!("../../../user/build/gnu-make-octagram/make");
const RUST_HELLO_ELF: &[u8] = include_bytes!("../../../user/rust/build/rust_hello.elf");
const RUST_HELLO_OCTAGRAM_ELF: &[u8] =
    include_bytes!("../../../user/rust/build/rust_hello_octagram.elf");
const SH_ELF: &[u8] = include_bytes!("../../../user/build/sh.elf");
const TCC_ELF: &[u8] = include_bytes!("../../../user/build/tcc.elf");
const FILE_API_DEMO_ELF: &[u8] = include_bytes!("../../../user/build/file_api_demo.elf");
const PATH_API_DEMO_ELF: &[u8] = include_bytes!("../../../user/build/path_api_demo.elf");
const AT_API_DEMO_ELF: &[u8] = include_bytes!("../../../user/build/at_api_demo.elf");
const EXECVE_DEMO_ELF: &[u8] = include_bytes!("../../../user/build/execve_demo.elf");
const FORK_DEMO_ELF: &[u8] = include_bytes!("../../../user/build/fork_demo.elf");
const TIME_DEMO_ELF: &[u8] = include_bytes!("../../../user/build/time_demo.elf");
const SYSCALL_COMPAT_DEMO_ELF: &[u8] =
    include_bytes!("../../../user/build/syscall_compat_demo.elf");
const MMAP_DEMO_ELF: &[u8] = include_bytes!("../../../user/build/mmap_demo.elf");
const TLS_DEMO_ELF: &[u8] = include_bytes!("../../../user/build/tls_demo.elf");
const THREAD_DEMO_ELF: &[u8] = include_bytes!("../../../user/build/thread_demo.elf");
const FB_DEMO_ELF: &[u8] = include_bytes!("../../../user/build/fb_demo.elf");
const LDSO_ELF: &[u8] = include_bytes!("../../../user/build/ld-octagram.elf");
const DYNAMIC_DEMO_ELF: &[u8] = include_bytes!("../../../user/build/dynamic_demo.elf");
const DYNAMIC_DEMO_LIB: &[u8] = include_bytes!("../../../user/build/libdyn_demo.so");
const SHARED_LIBC: &[u8] = include_bytes!("../../../user/build/liboctagram.so");
const TCC_CRT1: &[u8] = include_bytes!("../../../user/build/crt0.o");
const TCC_LIBC: &[u8] = include_bytes!("../../../user/build/libc.a");
const TCC_LIBTCC1: &[u8] = include_bytes!("../../../user/build/libtcc1.a");
const TCC_SMOKE_SOURCE: &[u8] = include_bytes!("../../../user/examples/tcc_smoke.c");
const TCC_SMOKE_MINIMAL_SOURCE: &[u8] =
    include_bytes!("../../../user/examples/tcc_smoke_minimal.c");
const USER_HEADERS: &[(&str, &[u8])] = &[
    (
        "/include/assert.h",
        include_bytes!("../../../user/include/assert.h"),
    ),
    (
        "/include/ctype.h",
        include_bytes!("../../../user/include/ctype.h"),
    ),
    (
        "/include/dirent.h",
        include_bytes!("../../../user/include/dirent.h"),
    ),
    (
        "/include/dlfcn.h",
        include_bytes!("../../../user/include/dlfcn.h"),
    ),
    (
        "/include/errno.h",
        include_bytes!("../../../user/include/errno.h"),
    ),
    (
        "/include/grp.h",
        include_bytes!("../../../user/include/grp.h"),
    ),
    (
        "/include/pwd.h",
        include_bytes!("../../../user/include/pwd.h"),
    ),
    (
        "/include/float.h",
        include_bytes!("../../../user/third_party/tinycc/include/float.h"),
    ),
    (
        "/include/setjmp.h",
        include_bytes!("../../../user/include/setjmp.h"),
    ),
    (
        "/include/stdalign.h",
        include_bytes!("../../../user/third_party/tinycc/include/stdalign.h"),
    ),
    (
        "/include/stdatomic.h",
        include_bytes!("../../../user/third_party/tinycc/include/stdatomic.h"),
    ),
    (
        "/include/stdbool.h",
        include_bytes!("../../../user/third_party/tinycc/include/stdbool.h"),
    ),
    (
        "/include/stdnoreturn.h",
        include_bytes!("../../../user/third_party/tinycc/include/stdnoreturn.h"),
    ),
    (
        "/include/tccdefs.h",
        include_bytes!("../../../user/third_party/tinycc/include/tccdefs.h"),
    ),
    (
        "/include/tgmath.h",
        include_bytes!("../../../user/third_party/tinycc/include/tgmath.h"),
    ),
    (
        "/include/varargs.h",
        include_bytes!("../../../user/third_party/tinycc/include/varargs.h"),
    ),
    (
        "/include/fcntl.h",
        include_bytes!("../../../user/include/fcntl.h"),
    ),
    (
        "/include/octagram.h",
        include_bytes!("../../../user/include/octagram.h"),
    ),
    (
        "/include/spawn.h",
        include_bytes!("../../../user/include/spawn.h"),
    ),
    (
        "/include/stdarg.h",
        include_bytes!("../../../user/include/stdarg.h"),
    ),
    (
        "/include/stddef.h",
        include_bytes!("../../../user/include/stddef.h"),
    ),
    (
        "/include/stdint.h",
        include_bytes!("../../../user/include/stdint.h"),
    ),
    (
        "/include/stdio.h",
        include_bytes!("../../../user/include/stdio.h"),
    ),
    (
        "/include/pthread.h",
        include_bytes!("../../../user/include/pthread.h"),
    ),
    (
        "/include/stdlib.h",
        include_bytes!("../../../user/include/stdlib.h"),
    ),
    (
        "/include/string.h",
        include_bytes!("../../../user/include/string.h"),
    ),
    (
        "/include/unistd.h",
        include_bytes!("../../../user/include/unistd.h"),
    ),
    (
        "/include/sys/mman.h",
        include_bytes!("../../../user/include/sys/mman.h"),
    ),
    (
        "/include/sys/ioctl.h",
        include_bytes!("../../../user/include/sys/ioctl.h"),
    ),
    (
        "/include/sys/stat.h",
        include_bytes!("../../../user/include/sys/stat.h"),
    ),
    (
        "/include/sys/types.h",
        include_bytes!("../../../user/include/sys/types.h"),
    ),
    (
        "/include/sys/wait.h",
        include_bytes!("../../../user/include/sys/wait.h"),
    ),
    (
        "/include/sys/uio.h",
        include_bytes!("../../../user/include/sys/uio.h"),
    ),
    (
        "/include/sys/resource.h",
        include_bytes!("../../../user/include/sys/resource.h"),
    ),
    (
        "/include/sys/times.h",
        include_bytes!("../../../user/include/sys/times.h"),
    ),
    (
        "/include/sys/sendfile.h",
        include_bytes!("../../../user/include/sys/sendfile.h"),
    ),
    (
        "/include/sys/syscall.h",
        include_bytes!("../../../user/include/sys/syscall.h"),
    ),
    (
        "/include/sys/file.h",
        include_bytes!("../../../user/include/sys/file.h"),
    ),
    (
        "/include/sys/param.h",
        include_bytes!("../../../user/include/sys/param.h"),
    ),
    (
        "/include/netdb.h",
        include_bytes!("../../../user/include/netdb.h"),
    ),
    (
        "/include/time.h",
        include_bytes!("../../../user/include/time.h"),
    ),
];

/// Filesystem operation failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    InvalidPath,
    NotFound,
    SymlinkLoop,
    NotSymlink,
    AlreadyExists,
    NotDirectory,
    IsDirectory,
    DirectoryNotEmpty,
    AlreadyMounted,
    NotMounted,
    Busy,
    NotInitialized,
    ReadOnly,
    NoSpace,
    IoError,
    PermissionDenied,
    InvalidData,
    Unsupported,
}

/// Kind of a filesystem node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    File,
    Directory,
    Symlink,
}

/// Metadata returned for a filesystem node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metadata {
    pub node_type: NodeType,
    pub len: usize,
    pub permissions: u16,
    /// Unix modification time in seconds since the epoch, when available.
    pub modified: Option<u64>,
}

/// Return the best available wall-clock timestamp for filesystem mutations.
/// EXT2's classic inode stores timestamps as whole Unix seconds.
#[cfg(test)]
static TEST_TIMESTAMP: AtomicU64 = AtomicU64::new(1);

#[cfg(test)]
pub(crate) fn current_timestamp() -> Option<u64> {
    Some(TEST_TIMESTAMP.fetch_add(1, Ordering::Relaxed))
}

#[cfg(not(test))]
pub(crate) fn current_timestamp() -> Option<u64> {
    crate::drivers::rtc::unix_seconds()
        .ok()
        .and_then(|seconds| u64::try_from(seconds).ok())
}

/// One direct child returned by [`FileSystem::read_dir`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryEntry {
    pub name: String,
    pub metadata: Metadata,
}

/// Common interface implemented by mounted filesystems.
///
/// Paths passed to implementations are absolute paths within that mounted
/// filesystem, normalized by the VFS (for example, `/etc/config`).
pub trait FileSystem: Send {
    fn name(&self) -> &'static str {
        "unknown"
    }
    fn metadata(&self, path: &str) -> Result<Metadata, FsError>;
    fn read_link(&self, _path: &str) -> Result<String, FsError> {
        Err(FsError::Unsupported)
    }
    fn read(&self, path: &str, offset: usize, buffer: &mut [u8]) -> Result<usize, FsError>;
    fn write(&mut self, path: &str, offset: usize, data: &[u8]) -> Result<usize, FsError>;
    fn truncate(&mut self, path: &str, len: usize) -> Result<(), FsError>;
    fn set_permissions(&mut self, _path: &str, _permissions: u16) -> Result<(), FsError> {
        Err(FsError::Unsupported)
    }
    fn create_file(&mut self, path: &str) -> Result<(), FsError>;
    fn create_dir(&mut self, path: &str) -> Result<(), FsError>;
    fn create_symlink(&mut self, _target: &str, _path: &str) -> Result<(), FsError> {
        Err(FsError::Unsupported)
    }
    fn remove(&mut self, path: &str) -> Result<(), FsError>;
    fn rename(&mut self, _source: &str, _target: &str) -> Result<(), FsError> {
        Err(FsError::Unsupported)
    }
    fn read_dir(&self, path: &str) -> Result<Vec<DirectoryEntry>, FsError>;
}

struct Mount {
    mount_point: String,
    filesystem: Box<dyn FileSystem>,
}

/// Mount table and filesystem operation router.
pub struct Vfs {
    mounts: Vec<Mount>,
}

impl Vfs {
    pub const fn new() -> Self {
        Self { mounts: Vec::new() }
    }

    pub fn mounts(&self) -> alloc::vec::Vec<(String, &'static str)> {
        let mut mounts = Vec::new();
        if mounts.try_reserve(self.mounts.len()).is_err() {
            return mounts;
        }
        for mount in &self.mounts {
            let filesystem = mount.filesystem.name();
            mounts.push((mount.mount_point.clone(), filesystem));
        }
        mounts
    }

    /// Mount `filesystem` at an existing directory.
    ///
    /// The root mount (`/`) is allowed before any other mount. Every later
    /// mount point must exist as a directory in a filesystem already mounted.
    pub fn mount<F: FileSystem + 'static>(
        &mut self,
        mount_point: &str,
        filesystem: F,
    ) -> Result<(), FsError> {
        self.mount_boxed(mount_point, Box::new(filesystem))
    }

    /// Mount an already boxed filesystem. This is used by boot when the root
    /// filesystem is selected dynamically from an installed disk.
    pub fn mount_boxed(
        &mut self,
        mount_point: &str,
        filesystem: Box<dyn FileSystem>,
    ) -> Result<(), FsError> {
        let mount_point = normalize_path(mount_point)?;
        if self
            .mounts
            .iter()
            .any(|mount| mount.mount_point == mount_point)
        {
            return Err(FsError::AlreadyMounted);
        }

        if mount_point != "/" {
            let (parent_mount, relative_path) = self.resolve(&mount_point)?;
            let metadata = self.mounts[parent_mount]
                .filesystem
                .metadata(&relative_path)?;
            if metadata.node_type != NodeType::Directory {
                return Err(FsError::NotDirectory);
            }
        } else if !self.mounts.is_empty() {
            return Err(FsError::AlreadyMounted);
        }

        self.mounts.push(Mount {
            mount_point,
            filesystem,
        });
        Ok(())
    }

    /// Unmount a filesystem. A mount with nested mounts cannot be removed.
    pub fn unmount(&mut self, mount_point: &str) -> Result<(), FsError> {
        let mount_point = normalize_path(mount_point)?;
        let index = self
            .mounts
            .iter()
            .position(|mount| mount.mount_point == mount_point)
            .ok_or(FsError::NotMounted)?;

        if self.mounts.iter().any(|mount| {
            mount.mount_point != mount_point && is_below(&mount.mount_point, &mount_point)
        }) {
            return Err(FsError::Busy);
        }

        self.mounts.remove(index);
        Ok(())
    }

    pub fn metadata(&self, path: &str) -> Result<Metadata, FsError> {
        let path = self.resolve_symlinks(path, true)?;
        let (mount, relative_path) = self.resolve(&path)?;
        self.mounts[mount].filesystem.metadata(&relative_path)
    }

    pub fn symlink_metadata(&self, path: &str) -> Result<Metadata, FsError> {
        let path = self.resolve_symlinks(path, false)?;
        let (mount, relative_path) = self.resolve(&path)?;
        self.mounts[mount].filesystem.metadata(&relative_path)
    }

    /// Read up to `buffer.len()` bytes beginning at `offset`; returns zero at EOF.
    pub fn read(&self, path: &str, offset: usize, buffer: &mut [u8]) -> Result<usize, FsError> {
        let path = self.resolve_symlinks(path, true)?;
        let (mount, relative_path) = self.resolve(&path)?;
        self.mounts[mount]
            .filesystem
            .read(&relative_path, offset, buffer)
    }

    /// Write bytes beginning at `offset`; the filesystem decides whether gaps
    /// are supported and returns the number of bytes written.
    pub fn write(&mut self, path: &str, offset: usize, data: &[u8]) -> Result<usize, FsError> {
        let path = self.resolve_symlinks(path, true)?;
        let (mount, relative_path) = self.resolve(&path)?;
        self.mounts[mount]
            .filesystem
            .write(&relative_path, offset, data)
    }

    pub fn truncate(&mut self, path: &str, len: usize) -> Result<(), FsError> {
        let path = self.resolve_symlinks(path, true)?;
        let (mount, relative_path) = self.resolve(&path)?;
        self.mounts[mount].filesystem.truncate(&relative_path, len)
    }

    pub fn set_permissions(&mut self, path: &str, permissions: u16) -> Result<(), FsError> {
        if permissions & !0o777 != 0 {
            return Err(FsError::InvalidData);
        }
        let path = self.resolve_symlinks(path, true)?;
        let (mount, relative_path) = self.resolve(&path)?;
        self.mounts[mount]
            .filesystem
            .set_permissions(&relative_path, permissions)
    }

    pub fn create_file(&mut self, path: &str) -> Result<(), FsError> {
        let path = self.resolve_symlinks(path, false)?;
        let (mount, relative_path) = self.resolve(&path)?;
        self.mounts[mount].filesystem.create_file(&relative_path)
    }

    pub fn create_dir(&mut self, path: &str) -> Result<(), FsError> {
        let path = self.resolve_symlinks(path, false)?;
        let (mount, relative_path) = self.resolve(&path)?;
        self.mounts[mount].filesystem.create_dir(&relative_path)
    }

    /// Create a symbolic link. Relative targets are interpreted from the
    /// directory containing the link.
    pub fn symlink(&mut self, target: &str, link_path: &str) -> Result<(), FsError> {
        if target.is_empty() || target.as_bytes().contains(&0) {
            return Err(FsError::InvalidPath);
        }
        let link_path = self.resolve_symlinks(link_path, false)?;
        let (mount, relative_path) = self.resolve(&link_path)?;
        self.mounts[mount]
            .filesystem
            .create_symlink(target, &relative_path)
    }

    /// Return the stored target without following the final symbolic link.
    pub fn read_link(&self, path: &str) -> Result<String, FsError> {
        let path = self.resolve_symlinks(path, false)?;
        let (mount, relative_path) = self.resolve(&path)?;
        let filesystem = &self.mounts[mount].filesystem;
        if filesystem.metadata(&relative_path)?.node_type != NodeType::Symlink {
            return Err(FsError::NotSymlink);
        }
        filesystem.read_link(&relative_path)
    }

    pub fn remove(&mut self, path: &str) -> Result<(), FsError> {
        let path = self.resolve_symlinks(path, false)?;
        let (mount, relative_path) = self.resolve(&path)?;
        self.mounts[mount].filesystem.remove(&relative_path)
    }

    pub fn remove_file(&mut self, path: &str) -> Result<(), FsError> {
        let path = self.resolve_symlinks(path, false)?;
        let (mount, relative_path) = self.resolve(&path)?;
        let filesystem = &mut self.mounts[mount].filesystem;
        if filesystem.metadata(&relative_path)?.node_type == NodeType::Directory {
            return Err(FsError::IsDirectory);
        }
        filesystem.remove(&relative_path)
    }

    pub fn remove_dir(&mut self, path: &str) -> Result<(), FsError> {
        let path = self.resolve_symlinks(path, false)?;
        let (mount, relative_path) = self.resolve(&path)?;
        let filesystem = &mut self.mounts[mount].filesystem;
        if filesystem.metadata(&relative_path)?.node_type != NodeType::Directory {
            return Err(FsError::NotDirectory);
        }
        filesystem.remove(&relative_path)
    }

    pub fn rename(&mut self, source: &str, target: &str) -> Result<(), FsError> {
        let source = self.resolve_symlinks(source, false)?;
        let target = self.resolve_symlinks(target, false)?;
        let (source_mount, source_path) = self.resolve(&source)?;
        let (target_mount, target_path) = self.resolve(&target)?;
        if source_mount != target_mount {
            return Err(FsError::Unsupported);
        }
        self.mounts[source_mount]
            .filesystem
            .rename(&source_path, &target_path)
    }

    pub fn read_dir(&self, path: &str) -> Result<Vec<DirectoryEntry>, FsError> {
        let path = self.resolve_symlinks(path, true)?;
        let (mount, relative_path) = self.resolve(&path)?;
        self.mounts[mount].filesystem.read_dir(&relative_path)
    }

    fn resolve_symlinks(&self, path: &str, follow_final: bool) -> Result<String, FsError> {
        const MAX_SYMLINKS: usize = 40;
        let mut current = normalize_path(path)?;
        for _ in 0..=MAX_SYMLINKS {
            let components: Vec<String> = current
                .split('/')
                .filter(|component| !component.is_empty())
                .map(String::from)
                .collect();
            let mut prefix = String::new();
            let mut replaced = false;
            for (index, component) in components.iter().enumerate() {
                prefix.push('/');
                prefix.push_str(component);
                let is_final = index + 1 == components.len();
                if is_final && !follow_final {
                    break;
                }

                let (mount, relative_path) = self.resolve(&prefix)?;
                let filesystem = &self.mounts[mount].filesystem;
                let metadata = filesystem.metadata(&relative_path)?;
                if metadata.node_type == NodeType::Symlink {
                    let target = filesystem.read_link(&relative_path)?;
                    if target.is_empty() || target.as_bytes().contains(&0) {
                        return Err(FsError::InvalidData);
                    }
                    let parent = prefix
                        .rfind('/')
                        .map(|index| &prefix[..index])
                        .filter(|parent| !parent.is_empty())
                        .unwrap_or("/");
                    let mut replacement = if target.starts_with('/') {
                        target
                    } else {
                        alloc::format!("{parent}/{target}")
                    };
                    for suffix in components.iter().skip(index + 1) {
                        replacement.push('/');
                        replacement.push_str(suffix);
                    }
                    current = normalize_path(&replacement)?;
                    replaced = true;
                    break;
                }
                if !is_final && metadata.node_type != NodeType::Directory {
                    return Err(FsError::NotDirectory);
                }
            }
            if !replaced {
                return Ok(current);
            }
        }
        Err(FsError::SymlinkLoop)
    }

    fn resolve(&self, path: &str) -> Result<(usize, String), FsError> {
        let path = normalize_path(path)?;
        let (index, mount) = self
            .mounts
            .iter()
            .enumerate()
            .filter(|(_, mount)| is_below(&path, &mount.mount_point))
            .max_by_key(|(_, mount)| mount.mount_point.len())
            .ok_or(FsError::NotMounted)?;

        let relative_path = if mount.mount_point == "/" {
            path
        } else if path == mount.mount_point {
            String::from("/")
        } else {
            String::from(&path[mount.mount_point.len()..])
        };
        Ok((index, relative_path))
    }
}

impl Default for Vfs {
    fn default() -> Self {
        Self::new()
    }
}

/// Normalize an absolute path and reject attempts to walk above the root.
pub fn normalize_path(path: &str) -> Result<String, FsError> {
    if !path.starts_with('/') || path.as_bytes().contains(&0) {
        return Err(FsError::InvalidPath);
    }

    let mut components: Vec<&str> = Vec::new();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(FsError::InvalidPath);
                }
            }
            _ => components.push(component),
        }
    }

    let mut normalized = String::new();
    for component in components {
        normalized.push('/');
        normalized.push_str(component);
    }
    if normalized.is_empty() {
        normalized.push('/');
    }
    Ok(normalized)
}

fn is_below(path: &str, mount_point: &str) -> bool {
    mount_point == "/"
        || path == mount_point
        || path
            .strip_prefix(mount_point)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

static GLOBAL_VFS: Once<Mutex<Vfs>> = Once::new();

/// Name of the internal block device currently providing the persistent root.
static PERSISTENT_ROOT_DEVICE: Once<String> = Once::new();

pub fn persistent_root_device() -> Option<String> {
    PERSISTENT_ROOT_DEVICE.get().cloned()
}

/// Select a marked EXT2 or FAT32 root partition from an internal SATA disk.
/// USB devices are excluded so the Live installer flash drive can never become
/// the installed root by accident.
fn discover_persistent_root() -> Option<(String, Box<dyn FileSystem>)> {
    let requested_slot = crate::boot_slot();
    if let Some(slot) = requested_slot {
        crate::println!(
            "[rootfs] bootloader requested slot {}",
            if slot == 2 { "B" } else { "A" }
        );
    }
    let mut fallback: Option<(String, String, Box<dyn FileSystem>)> = None;
    for info in block::list() {
        if !info.name.starts_with("sata") {
            continue;
        }
        let Some(device) = block::get(&info.name) else {
            continue;
        };

        if let Ok(partitions) = partition::read_gpt_partitions(device.as_ref()) {
            for part in partitions {
                if part.type_guid != LINUX_FILESYSTEM_GUID {
                    continue;
                }
                let part_name = alloc::format!("{}p{}", info.name, part.index);
                let Ok(partition_device) = PartitionDevice::new(
                    device.clone(),
                    part_name.clone(),
                    part.first_lba,
                    part.sector_count(),
                ) else {
                    continue;
                };
                let partition_ref = Arc::new(partition_device);
                if let Ok(filesystem) = Ext2::mount(partition_ref.clone())
                    && let Some(root) = consider_root(
                        Box::new(filesystem),
                        &info.name,
                        &part_name,
                        requested_slot,
                        &mut fallback,
                    )
                {
                    return Some(root);
                }
                if let Ok(filesystem) = Fat32::mount(partition_ref)
                    && let Some(root) = consider_root(
                        Box::new(filesystem),
                        &info.name,
                        &part_name,
                        requested_slot,
                        &mut fallback,
                    )
                {
                    return Some(root);
                }
            }
            continue;
        }

        // Compatibility with older MBR/superfloppy EXT2 installations.
        if let Ok(filesystem) = Ext2::mount(device.clone())
            && filesystem.metadata("/etc/octagram-root").is_ok()
        {
            crate::println!("[rootfs] persistent EXT2 root selected from {}", info.name);
            let root_name = info.name.clone();
            let _ = PERSISTENT_ROOT_DEVICE.call_once(|| root_name);
            return Some((info.name, Box::new(filesystem)));
        }
        if let Ok(filesystem) = Fat32::mount(device)
            && filesystem.metadata("/etc/octagram-root").is_ok()
        {
            crate::println!("[rootfs] persistent FAT32 root selected from {}", info.name);
            let root_name = info.name.clone();
            let _ = PERSISTENT_ROOT_DEVICE.call_once(|| root_name);
            return Some((info.name, Box::new(filesystem)));
        }
    }
    if let Some((disk_name, part_name, filesystem)) = fallback {
        crate::println!(
            "[rootfs] persistent {} root selected from {part_name} (requested slot unavailable)",
            filesystem.name().to_uppercase()
        );
        let _ = PERSISTENT_ROOT_DEVICE.call_once(|| disk_name.clone());
        return Some((disk_name, filesystem));
    }
    None
}

/// Accept a mounted root only when it is the slot the bootloader asked for;
/// otherwise remember it as a fallback for images without slot cmdlines.
fn consider_root(
    filesystem: Box<dyn FileSystem>,
    disk_name: &str,
    part_name: &str,
    requested_slot: Option<u8>,
    fallback: &mut Option<(String, String, Box<dyn FileSystem>)>,
) -> Option<(String, Box<dyn FileSystem>)> {
    if filesystem.metadata("/etc/octagram-root").is_err() {
        return None;
    }
    if let Some(want) = requested_slot
        && read_slot_marker(filesystem.as_ref()) != Some(want)
    {
        if fallback.is_none() {
            *fallback = Some((
                String::from(disk_name),
                String::from(part_name),
                filesystem,
            ));
        }
        return None;
    }
    crate::println!(
        "[rootfs] persistent {} root selected from {part_name}",
        filesystem.name().to_uppercase()
    );
    let _ = PERSISTENT_ROOT_DEVICE.call_once(|| String::from(disk_name));
    Some((String::from(disk_name), filesystem))
}

/// The marker is a small multi-line file ("OCTAGRAM-OS persistent root v1",
/// filesystem and slot lines), so look for the slot line anywhere in it.
fn read_slot_marker(filesystem: &dyn FileSystem) -> Option<u8> {
    let mut buffer = [0u8; 192];
    let read = filesystem.read("/etc/octagram-root", 0, &mut buffer).ok()?;
    let content = &buffer[..read];
    if content.windows(6).any(|window| window == b"slot=A") {
        return Some(1);
    }
    if content.windows(6).any(|window| window == b"slot=B") {
        return Some(2);
    }
    None
}

fn ensure_root_directory(root: &mut dyn FileSystem, path: &str) {
    match root.metadata(path) {
        Ok(metadata) if metadata.node_type == NodeType::Directory => {}
        Ok(_) => panic!("root path {path} exists but is not a directory"),
        Err(FsError::NotFound) => root
            .create_dir(path)
            .unwrap_or_else(|error| panic!("creating root directory {path} failed: {error:?}")),
        Err(error) => panic!("checking root directory {path} failed: {error:?}"),
    }
}

/// Initialize the global VFS. A marked EXT2 or FAT32 partition on an internal
/// SATA disk becomes '/'; RAMFS remains as a development/rescue fallback.
pub fn init() {
    let persistent_root = discover_persistent_root();
    GLOBAL_VFS.call_once(|| {
        let mut vfs = Vfs::new();
        let mut root: Box<dyn FileSystem> = match persistent_root {
            Some((_device_name, filesystem)) => filesystem,
            None => {
                crate::println!(
                    "[rootfs] WARNING: no installed SATA root found; using volatile RAMFS"
                );
                Box::new(RamFs::new())
            }
        };
        for directory in [
            "/dev",
            "/bin",
            "/lib",
            "/lib/tcc",
            "/share",
            "/share/octagram",
            "/include",
            "/include/sys",
            "/tmp",
            "/home",
            "/home/octagram",
            "/etc",
            "/src",
            "/build",
            "/toolchain",
            "/var",
            "/var/log",
            "/var/log/build",
            "/var/cache",
            "/var/cache/build",
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
        ] {
            ensure_root_directory(root.as_mut(), directory);
        }
        vfs.mount_boxed("/", root)
            .expect("mounting the selected root filesystem must succeed");
        vfs.mount("/dev", DevFs::new())
            .expect("mounting the device filesystem must succeed");
        seed_file(&mut vfs, "/bin/sh", SH_ELF, 0o755);
        seed_file(&mut vfs, "/bin/make", MAKE_ELF, 0o755);
        seed_file(&mut vfs, "/usr/bin/gmake", GNU_MAKE_ELF, 0o755);
        seed_file(&mut vfs, "/bin/rust-hello", RUST_HELLO_ELF, 0o755);
        seed_file(
            &mut vfs,
            "/bin/rust-hello-octagram",
            RUST_HELLO_OCTAGRAM_ELF,
            0o755,
        );
        seed_file(&mut vfs, "/bin/tcc", TCC_ELF, 0o755);
        seed_file(&mut vfs, "/bin/file-api-demo", FILE_API_DEMO_ELF, 0o755);
        seed_file(&mut vfs, "/bin/path-api-demo", PATH_API_DEMO_ELF, 0o755);
        seed_file(&mut vfs, "/bin/at-api-demo", AT_API_DEMO_ELF, 0o755);
        seed_file(&mut vfs, "/bin/execve-demo", EXECVE_DEMO_ELF, 0o755);
        seed_file(&mut vfs, "/bin/fork-demo", FORK_DEMO_ELF, 0o755);
        seed_file(&mut vfs, "/bin/time-demo", TIME_DEMO_ELF, 0o755);
        seed_file(
            &mut vfs,
            "/bin/syscall-compat-demo",
            SYSCALL_COMPAT_DEMO_ELF,
            0o755,
        );
        seed_file(&mut vfs, "/bin/mmap-demo", MMAP_DEMO_ELF, 0o755);
        seed_file(&mut vfs, "/bin/tls-demo", TLS_DEMO_ELF, 0o755);
        seed_file(&mut vfs, "/bin/thread-demo", THREAD_DEMO_ELF, 0o755);
        seed_file(&mut vfs, "/bin/fb-demo", FB_DEMO_ELF, 0o755);
        seed_file(&mut vfs, "/bin/fb_demo", FB_DEMO_ELF, 0o755);
        seed_file(&mut vfs, "/bin/dynamic-demo", DYNAMIC_DEMO_ELF, 0o755);
        seed_file(&mut vfs, "/lib/ld-octagram.so", LDSO_ELF, 0o755);
        seed_file(&mut vfs, "/lib/liboctagram.so", SHARED_LIBC, 0o755);
        seed_file(&mut vfs, "/lib/libdyn_demo.so", DYNAMIC_DEMO_LIB, 0o755);
        seed_file(&mut vfs, "/lib/tcc/crt1.o", TCC_CRT1, 0o644);
        seed_file(&mut vfs, "/lib/tcc/libc.a", TCC_LIBC, 0o644);
        seed_file(&mut vfs, "/lib/tcc/libtcc1.a", TCC_LIBTCC1, 0o644);
        seed_file(&mut vfs, "/tmp/tcc.c", TCC_SMOKE_MINIMAL_SOURCE, 0o644);
        seed_file(&mut vfs, "/tmp/tcc-full.c", TCC_SMOKE_SOURCE, 0o644);
        seed_file(
            &mut vfs,
            "/etc/os-release",
            b"OCTAGRAM_OS_NAME=\"OCTAGRAM-OS\"\nOCTAGRAM_OS_ID=octagram\nOCTAGRAM_OS_VERSION=1.0\n",
            0o644,
        );
        seed_file(
            &mut vfs,
            "/etc/passwd",
            b"octagram:x:0:0:OCTAGRAM User:/home/octagram:/bin/sh\n",
            0o644,
        );
        seed_file(
            &mut vfs,
            "/home/octagram/.profile",
            b"export PATH=/bin:/usr/bin:/usr/local/bin\n",
            0o644,
        );
        seed_file(
            &mut vfs,
            "/apps/apps.txt",
            b"# OCTAGRAM native application registry\n",
            0o644,
        );
        seed_file(
            &mut vfs,
            "/var/lib/octagram/packages/registry.txt",
            b"",
            0o644,
        );
        seed_file(
            &mut vfs,
            "/share/octagram/Makefile",
            include_bytes!("../../../user/examples/selfhost/Makefile"),
            0o644,
        );
        seed_file(
            &mut vfs,
            "/share/octagram/selfhost-rules.mk",
            include_bytes!("../../../user/examples/selfhost/selfhost-rules.mk"),
            0o644,
        );
        seed_file(
            &mut vfs,
            "/share/octagram/hello.c",
            include_bytes!("../../../user/examples/selfhost/hello.c"),
            0o644,
        );
        seed_file(
            &mut vfs,
            "/share/octagram/message.c",
            include_bytes!("../../../user/examples/selfhost/message.c"),
            0o644,
        );
        seed_file(
            &mut vfs,
            "/share/octagram/ld-main.c",
            include_bytes!("../../../user/examples/selfhost/ld-main.c"),
            0o644,
        );
        seed_file(
            &mut vfs,
            "/share/octagram/ld-helper.c",
            include_bytes!("../../../user/examples/selfhost/ld-helper.c"),
            0o644,
        );
        seed_file(
            &mut vfs,
            "/share/octagram/xrustc-sum.rs",
            include_bytes!("../../../user/examples/selfhost/xrustc-sum.rs"),
            0o644,
        );
        seed_file(
            &mut vfs,
            "/share/octagram/xrustc-bad.rs",
            include_bytes!("../../../user/examples/selfhost/xrustc-bad.rs"),
            0o644,
        );
        seed_file(
            &mut vfs,
            "/share/octagram/fb_demo.c",
            include_bytes!("../../../user/examples/fb_demo.c"),
            0o644,
        );
        for (path, contents) in USER_HEADERS {
            seed_file(&mut vfs, path, contents, 0o644);
        }
        Mutex::new(vfs)
    });
}

fn seed_file(vfs: &mut Vfs, path: &str, contents: &[u8], permissions: u16) {
    // Seed only missing files. Existing files on an installed root belong to
    // the user and must survive subsequent boots and kernel upgrades.
    match vfs.metadata(path) {
        Ok(_) => return,
        Err(FsError::NotFound) => {}
        Err(error) => panic!("checking boot file {path} failed: {error:?}"),
    }
    vfs.create_file(path)
        .unwrap_or_else(|error| panic!("creating boot file {path} failed: {error:?}"));
    let written = vfs
        .write(path, 0, contents)
        .unwrap_or_else(|error| panic!("writing boot file {path} failed: {error:?}"));
    assert_eq!(written, contents.len(), "boot file {path} must be complete");
    match vfs.set_permissions(path, permissions) {
        Ok(()) | Err(FsError::Unsupported) => {}
        Err(error) => panic!("setting boot file permissions for {path} failed: {error:?}"),
    }
}

/// Run an operation while holding the global VFS lock.
///
/// Do not call this recursively from `operation`, or from an interrupt handler.
pub fn with_vfs<R>(operation: impl FnOnce(&mut Vfs) -> R) -> Result<R, FsError> {
    let vfs = GLOBAL_VFS.get().ok_or(FsError::NotInitialized)?;
    let mut guard = vfs.lock();
    Ok(operation(&mut guard))
}

#[cfg(test)]
mod tests {
    use super::{DevFs, DirectoryEntry, FileSystem, FsError, NodeType, RamFs, Vfs, normalize_path};

    #[test]
    fn initial_vfs_contains_make_and_tcc_toolchain() {
        super::init();
        super::with_vfs(|vfs| {
            let gmake = vfs.metadata("/usr/bin/gmake").unwrap();
            assert_eq!(gmake.permissions, 0o755);
            assert!(gmake.len > 0);
            let mut gmake_magic = [0; 4];
            assert_eq!(vfs.read("/usr/bin/gmake", 0, &mut gmake_magic), Ok(4));
            assert_eq!(&gmake_magic, b"\x7FELF");

            let make = vfs.metadata("/bin/make").unwrap();
            assert_eq!(make.permissions, 0o755);
            assert!(make.len > 0);
            let mut magic = [0; 4];
            assert_eq!(vfs.read("/bin/make", 0, &mut magic), Ok(4));
            assert_eq!(&magic, b"\x7FELF");

            let rust_hello = vfs.metadata("/bin/rust-hello").unwrap();
            assert_eq!(rust_hello.permissions, 0o755);
            assert!(rust_hello.len > 0);
            assert_eq!(vfs.read("/bin/rust-hello", 0, &mut magic), Ok(4));
            assert_eq!(&magic, b"\x7FELF");

            let rust_hello_octagram = vfs.metadata("/bin/rust-hello-octagram").unwrap();
            assert_eq!(rust_hello_octagram.permissions, 0o755);
            assert!(rust_hello_octagram.len > 0);
            assert_eq!(vfs.read("/bin/rust-hello-octagram", 0, &mut magic), Ok(4));
            assert_eq!(&magic, b"\x7FELF");

            for path in ["/bin/gcc", "/bin/rustc", "/bin/chibicc"] {
                assert_eq!(vfs.metadata(path), Err(FsError::NotFound));
            }

            let tcc = vfs.metadata("/bin/tcc").unwrap();
            assert_eq!(tcc.permissions, 0o755);
            assert!(tcc.len > 0);
            assert_eq!(vfs.read("/bin/tcc", 0, &mut magic), Ok(4));
            assert_eq!(&magic, b"\x7FELF");

            let file_api_demo = vfs.metadata("/bin/file-api-demo").unwrap();
            assert_eq!(file_api_demo.permissions, 0o755);
            assert_eq!(vfs.read("/bin/file-api-demo", 0, &mut magic), Ok(4));
            assert_eq!(&magic, b"\x7FELF");

            let path_api_demo = vfs.metadata("/bin/path-api-demo").unwrap();
            assert_eq!(path_api_demo.permissions, 0o755);
            assert_eq!(vfs.read("/bin/path-api-demo", 0, &mut magic), Ok(4));
            assert_eq!(&magic, b"\x7FELF");

            let at_api_demo = vfs.metadata("/bin/at-api-demo").unwrap();
            assert_eq!(at_api_demo.permissions, 0o755);
            assert_eq!(vfs.read("/bin/at-api-demo", 0, &mut magic), Ok(4));
            assert_eq!(&magic, b"\x7FELF");

            let execve_demo = vfs.metadata("/bin/execve-demo").unwrap();
            assert_eq!(execve_demo.permissions, 0o755);
            assert_eq!(vfs.read("/bin/execve-demo", 0, &mut magic), Ok(4));
            assert_eq!(&magic, b"\x7FELF");

            let fork_demo = vfs.metadata("/bin/fork-demo").unwrap();
            assert_eq!(fork_demo.permissions, 0o755);
            assert_eq!(vfs.read("/bin/fork-demo", 0, &mut magic), Ok(4));
            assert_eq!(&magic, b"\x7FELF");

            let time_demo = vfs.metadata("/bin/time-demo").unwrap();
            assert_eq!(time_demo.permissions, 0o755);
            assert_eq!(vfs.read("/bin/time-demo", 0, &mut magic), Ok(4));
            assert_eq!(&magic, b"\x7FELF");

            let mmap_demo = vfs.metadata("/bin/mmap-demo").unwrap();
            assert_eq!(mmap_demo.permissions, 0o755);
            assert_eq!(vfs.read("/bin/mmap-demo", 0, &mut magic), Ok(4));
            assert_eq!(&magic, b"\x7FELF");

            let tls_demo = vfs.metadata("/bin/tls-demo").unwrap();
            assert_eq!(tls_demo.permissions, 0o755);
            assert_eq!(vfs.read("/bin/tls-demo", 0, &mut magic), Ok(4));
            assert_eq!(&magic, b"\x7FELF");

            let dynamic_demo = vfs.metadata("/bin/dynamic-demo").unwrap();
            assert_eq!(dynamic_demo.permissions, 0o755);
            assert_eq!(vfs.read("/bin/dynamic-demo", 0, &mut magic), Ok(4));
            assert_eq!(&magic, b"\x7FELF");
            let loader = vfs.metadata("/lib/ld-octagram.so").unwrap();
            assert_eq!(loader.permissions, 0o755);
            assert_eq!(vfs.read("/lib/ld-octagram.so", 0, &mut magic), Ok(4));
            assert_eq!(&magic, b"\x7FELF");
            let shared_library = vfs.metadata("/lib/libdyn_demo.so").unwrap();
            assert!(shared_library.len > 0);
            assert!(vfs.metadata("/lib/liboctagram.so").unwrap().len > 0);

            assert!(vfs.metadata("/lib/tcc/crt1.o").unwrap().len > 0);
            assert_eq!(vfs.read("/lib/tcc/crt1.o", 0, &mut magic), Ok(4));
            assert_eq!(&magic, b"\x7FELF");
            for path in ["/lib/tcc/libc.a", "/lib/tcc/libtcc1.a"] {
                assert!(vfs.metadata(path).unwrap().len > 8);
                let mut archive_magic = [0; 8];
                assert_eq!(vfs.read(path, 0, &mut archive_magic), Ok(8));
                assert_eq!(&archive_magic, b"!<arch>\n");
            }

            let tcc_source = include_bytes!("../../../user/examples/tcc_smoke_minimal.c");
            let tcc_metadata = vfs.metadata("/tmp/tcc.c").unwrap();
            assert_eq!(tcc_metadata.len, tcc_source.len());
            let mut tcc_contents = alloc::vec![0; tcc_metadata.len];
            assert_eq!(
                vfs.read("/tmp/tcc.c", 0, &mut tcc_contents),
                Ok(tcc_source.len())
            );
            assert_eq!(&tcc_contents, tcc_source);

            let tcc_full_source = include_bytes!("../../../user/examples/tcc_smoke.c");
            let tcc_full_metadata = vfs.metadata("/tmp/tcc-full.c").unwrap();
            assert_eq!(tcc_full_metadata.len, tcc_full_source.len());

            for (path, expected) in [
                (
                    "/share/octagram/Makefile",
                    include_bytes!("../../../user/examples/selfhost/Makefile").as_slice(),
                ),
                (
                    "/share/octagram/hello.c",
                    include_bytes!("../../../user/examples/selfhost/hello.c").as_slice(),
                ),
                (
                    "/share/octagram/message.c",
                    include_bytes!("../../../user/examples/selfhost/message.c").as_slice(),
                ),
                (
                    "/share/octagram/xrustc-sum.rs",
                    include_bytes!("../../../user/examples/selfhost/xrustc-sum.rs").as_slice(),
                ),
                (
                    "/share/octagram/xrustc-bad.rs",
                    include_bytes!("../../../user/examples/selfhost/xrustc-bad.rs").as_slice(),
                ),
            ] {
                let metadata = vfs.metadata(path).unwrap();
                assert_eq!(metadata.len, expected.len());
                let mut contents = alloc::vec![0; metadata.len];
                assert_eq!(vfs.read(path, 0, &mut contents), Ok(expected.len()));
                assert_eq!(contents, expected);
            }

            let header = include_bytes!("../../../user/include/stdio.h");
            let metadata = vfs.metadata("/include/stdio.h").unwrap();
            assert_eq!(metadata.permissions, 0o644);
            assert_eq!(metadata.len, header.len());
            let mut contents = alloc::vec![0; metadata.len];
            assert_eq!(
                vfs.read("/include/stdio.h", 0, &mut contents),
                Ok(header.len())
            );
            assert_eq!(&contents, header);

            assert!(vfs.metadata("/include/stdlib.h").is_ok());
            assert!(vfs.metadata("/include/sys/mman.h").is_ok());
            assert!(vfs.metadata("/include/sys/stat.h").is_ok());
            assert!(vfs.metadata("/include/assert.h").is_ok());
            assert!(vfs.metadata("/include/ctype.h").is_ok());
            assert!(vfs.metadata("/include/setjmp.h").is_ok());
            assert!(vfs.metadata("/include/tccdefs.h").is_ok());
        })
        .unwrap();
    }

    #[test]
    fn normalizes_paths_and_rejects_root_escape() {
        assert_eq!(normalize_path("/a//b/../c").unwrap(), "/a/c");
        assert_eq!(normalize_path("/../../outside"), Err(FsError::InvalidPath));
        assert_eq!(normalize_path("relative/path"), Err(FsError::InvalidPath));
    }

    #[test]
    fn ramfs_supports_nested_files_and_sparse_writes() {
        let mut filesystem = RamFs::new();
        filesystem.create_dir("/tmp").unwrap();
        filesystem.create_file("/tmp/data").unwrap();
        filesystem.write("/tmp/data", 3, b"xy").unwrap();
        assert_eq!(filesystem.write("/tmp/data", 100, b""), Ok(0));
        assert_eq!(filesystem.metadata("/tmp/data").unwrap().len, 5);

        let mut contents = [0; 5];
        assert_eq!(filesystem.read("/tmp/data", 0, &mut contents), Ok(5));
        assert_eq!(&contents, &[0, 0, 0, b'x', b'y']);
        assert_eq!(
            filesystem.metadata("/tmp/data").unwrap().len,
            contents.len()
        );
        assert_eq!(
            filesystem.read_dir("/tmp").unwrap(),
            alloc::vec![DirectoryEntry {
                name: alloc::string::String::from("data"),
                metadata: filesystem.metadata("/tmp/data").unwrap(),
            }]
        );
        assert_eq!(filesystem.remove("/tmp"), Err(FsError::DirectoryNotEmpty));
    }

    #[test]
    fn vfs_routes_longest_mount_and_respects_component_boundaries() {
        let mut root = RamFs::new();
        root.create_dir("/mnt").unwrap();
        root.create_file("/mnted").unwrap();
        root.create_file("/root-file").unwrap();

        let mut mounted = RamFs::new();
        mounted.create_file("/mounted-file").unwrap();
        mounted.write("/mounted-file", 0, b"child").unwrap();

        let mut vfs = Vfs::new();
        vfs.mount("/", root).unwrap();
        vfs.mount("/mnt", mounted).unwrap();

        let mut contents = [0; 5];
        assert_eq!(vfs.read("/mnt/mounted-file", 0, &mut contents), Ok(5));
        assert_eq!(&contents, b"child");
        assert_eq!(vfs.metadata("/mnted").unwrap().node_type, NodeType::File);
        assert_eq!(
            vfs.metadata("/root-file").unwrap().node_type,
            NodeType::File
        );
    }

    #[test]
    fn devfs_mount_exposes_devices_below_dev() {
        let mut root = RamFs::new();
        root.create_dir("/dev").unwrap();
        let mut vfs = Vfs::new();
        vfs.mount("/", root).unwrap();
        vfs.mount("/dev", DevFs::new()).unwrap();

        assert_eq!(vfs.metadata("/dev/tty").unwrap().node_type, NodeType::File);
        assert_eq!(vfs.read_dir("/dev").unwrap().len(), 10);
        assert_eq!(vfs.metadata("/dev/fb0").unwrap().node_type, NodeType::File);
        assert_eq!(vfs.metadata("/dev/tty3").unwrap().node_type, NodeType::File);
    }

    #[test]
    fn symlinks_resolve_relative_targets_and_intermediate_directories() {
        let mut root = RamFs::new();
        root.create_dir("/opt").unwrap();
        root.create_dir("/opt/tools").unwrap();
        root.create_dir("/work").unwrap();
        root.create_file("/opt/tools/app").unwrap();
        root.write("/opt/tools/app", 0, b"program").unwrap();

        let mut vfs = Vfs::new();
        vfs.mount("/", root).unwrap();
        vfs.symlink("../opt/tools", "/work/tools").unwrap();
        vfs.symlink("tools/app", "/work/app").unwrap();

        assert_eq!(
            vfs.metadata("/work/tools").unwrap().node_type,
            NodeType::Directory
        );
        assert_eq!(vfs.read_link("/work/tools").unwrap(), "../opt/tools");
        let mut contents = [0; 7];
        assert_eq!(vfs.read("/work/app", 0, &mut contents), Ok(7));
        assert_eq!(&contents, b"program");
    }

    #[test]
    fn symlink_can_target_another_mount_and_unlink_without_removing_target() {
        let mut root = RamFs::new();
        root.create_dir("/mnt").unwrap();
        root.create_dir("/bin").unwrap();

        let mut mounted = RamFs::new();
        mounted.create_file("/app").unwrap();
        mounted.write("/app", 0, b"mounted").unwrap();

        let mut vfs = Vfs::new();
        vfs.mount("/", root).unwrap();
        vfs.mount("/mnt", mounted).unwrap();
        vfs.symlink("/mnt/app", "/bin/app").unwrap();

        let mut contents = [0; 7];
        assert_eq!(vfs.read("/bin/app", 0, &mut contents), Ok(7));
        assert_eq!(&contents, b"mounted");
        assert_eq!(vfs.remove("/bin/app"), Ok(()));
        assert_eq!(vfs.metadata("/mnt/app").unwrap().node_type, NodeType::File);
    }

    #[test]
    fn symlink_loops_return_an_error() {
        let mut vfs = Vfs::new();
        vfs.mount("/", RamFs::new()).unwrap();
        vfs.symlink("/b", "/a").unwrap();
        vfs.symlink("/a", "/b").unwrap();

        assert_eq!(vfs.metadata("/a"), Err(FsError::SymlinkLoop));
    }

    #[test]
    fn symlink_metadata_reports_link_while_metadata_follows_it() {
        let mut root = RamFs::new();
        root.create_file("/target").unwrap();
        let mut vfs = Vfs::new();
        vfs.mount("/", root).unwrap();
        vfs.symlink("/target", "/link").unwrap();

        assert_eq!(vfs.metadata("/link").unwrap().node_type, NodeType::File);
        assert_eq!(
            vfs.symlink_metadata("/link").unwrap().node_type,
            NodeType::Symlink
        );
    }

    #[test]
    fn remove_file_and_directory_enforce_node_types_and_do_not_follow_final_links() {
        let mut root = RamFs::new();
        root.create_dir("/dir").unwrap();
        root.create_file("/dir/file").unwrap();
        let mut vfs = Vfs::new();
        vfs.mount("/", root).unwrap();
        vfs.symlink("/dir/file", "/file-link").unwrap();

        assert_eq!(vfs.remove_file("/dir"), Err(FsError::IsDirectory));
        assert_eq!(vfs.remove_dir("/dir/file"), Err(FsError::NotDirectory));
        assert_eq!(vfs.remove_dir("/dir"), Err(FsError::DirectoryNotEmpty));
        assert_eq!(vfs.read_link("/dir/file"), Err(FsError::NotSymlink));

        vfs.remove_file("/file-link").unwrap();
        assert_eq!(vfs.metadata("/dir/file").unwrap().node_type, NodeType::File);
        vfs.remove_file("/dir/file").unwrap();
        vfs.remove_dir("/dir").unwrap();
        assert_eq!(vfs.metadata("/dir"), Err(FsError::NotFound));
    }

    #[test]
    fn permissions_follow_symlinks_and_control_executable_mode() {
        let mut root = RamFs::new();
        root.create_file("/program").unwrap();
        let mut vfs = Vfs::new();
        vfs.mount("/", root).unwrap();
        assert_eq!(vfs.metadata("/program").unwrap().permissions, 0o644);

        vfs.set_permissions("/program", 0o755).unwrap();
        vfs.symlink("/program", "/program-link").unwrap();
        assert_eq!(vfs.metadata("/program-link").unwrap().permissions, 0o755);
        vfs.set_permissions("/program-link", 0o700).unwrap();
        assert_eq!(vfs.metadata("/program").unwrap().permissions, 0o700);
        assert_eq!(
            vfs.set_permissions("/program", 0o1777),
            Err(FsError::InvalidData)
        );
    }

    #[test]
    fn rename_routes_through_mount_and_rejects_cross_mount_moves() {
        let mut root = RamFs::new();
        root.create_dir("/mnt").unwrap();
        root.create_file("/source").unwrap();
        root.write("/source", 0, b"data").unwrap();
        let mut mounted = RamFs::new();
        mounted.create_file("/target").unwrap();
        let mut vfs = Vfs::new();
        vfs.mount("/", root).unwrap();
        vfs.mount("/mnt", mounted).unwrap();

        assert_eq!(vfs.rename("/source", "/renamed"), Ok(()));
        assert_eq!(vfs.metadata("/source"), Err(FsError::NotFound));
        assert_eq!(
            vfs.rename("/renamed", "/mnt/target"),
            Err(FsError::Unsupported)
        );
    }
}
