//! Virtual filesystem and the in-memory root filesystem.
//!
//! The VFS provides path normalization, mount routing, and a small common
//! filesystem interface. The initial root is a [`RamFs`], so its contents are
//! volatile until a block-device driver and a persistent filesystem exist.

mod devfs;
mod ramfs;

pub(crate) use devfs::{DevFs, device_access, is_tty_path};
pub use ramfs::RamFs;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use spin::{Mutex, Once};

/// Filesystem operation failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    InvalidPath,
    NotFound,
    SymlinkLoop,
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

    /// Mount `filesystem` at an existing directory.
    ///
    /// The root mount (`/`) is allowed before any other mount. Every later
    /// mount point must exist as a directory in a filesystem already mounted.
    pub fn mount<F: FileSystem + 'static>(
        &mut self,
        mount_point: &str,
        filesystem: F,
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
            filesystem: Box::new(filesystem),
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
        self.mounts[mount].filesystem.read_link(&relative_path)
    }

    pub fn remove(&mut self, path: &str) -> Result<(), FsError> {
        let path = self.resolve_symlinks(path, false)?;
        let (mount, relative_path) = self.resolve(&path)?;
        self.mounts[mount].filesystem.remove(&relative_path)
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

/// Initialize the global VFS with a volatile RAM filesystem mounted at `/`.
pub fn init() {
    GLOBAL_VFS.call_once(|| {
        let mut vfs = Vfs::new();
        let mut root = RamFs::new();
        root.create_dir("/dev")
            .expect("creating the initial /dev directory must succeed");
        vfs.mount("/", root)
            .expect("mounting the initial root filesystem must succeed");
        vfs.mount("/dev", DevFs::new())
            .expect("mounting the device filesystem must succeed");
        Mutex::new(vfs)
    });
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
                metadata: super::Metadata {
                    node_type: NodeType::File,
                    len: 5,
                    permissions: 0o644,
                },
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
        assert_eq!(vfs.read_dir("/dev").unwrap().len(), 5);
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
}
