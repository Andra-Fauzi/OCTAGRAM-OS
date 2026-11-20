use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use super::{DirectoryEntry, FileSystem, FsError, Metadata, NodeType, normalize_path};

enum Node {
    Directory { permissions: u16 },
    File { data: Vec<u8>, permissions: u16 },
    Symlink(String),
}

/// In-memory filesystem. All data is lost when the kernel stops.
pub struct RamFs {
    nodes: BTreeMap<String, Node>,
    modified: BTreeMap<String, u64>,
}

impl RamFs {
    pub fn new() -> Self {
        let mut nodes = BTreeMap::new();
        nodes.insert(String::from("/"), Node::Directory { permissions: 0o755 });
        let mut modified = BTreeMap::new();
        if let Some(timestamp) = super::current_timestamp() {
            modified.insert(String::from("/"), timestamp);
        }
        Self { nodes, modified }
    }

    fn parent_path(path: &str) -> Option<&str> {
        if path == "/" {
            return None;
        }
        match path.rfind('/') {
            Some(0) => Some("/"),
            Some(index) => Some(&path[..index]),
            None => None,
        }
    }

    fn ensure_parent_directory(&self, path: &str) -> Result<(), FsError> {
        let parent = Self::parent_path(path).ok_or(FsError::InvalidPath)?;
        match self.nodes.get(parent) {
            Some(Node::Directory { .. }) => Ok(()),
            Some(Node::File { .. } | Node::Symlink(_)) => Err(FsError::NotDirectory),
            None => Err(FsError::NotFound),
        }
    }

    fn metadata_for(node: &Node, modified: Option<u64>) -> Metadata {
        match node {
            Node::Directory { permissions } => Metadata {
                node_type: NodeType::Directory,
                len: 0,
                permissions: *permissions,
                modified,
            },
            Node::File { data, permissions } => Metadata {
                node_type: NodeType::File,
                len: data.len(),
                permissions: *permissions,
                modified,
            },
            Node::Symlink(target) => Metadata {
                node_type: NodeType::Symlink,
                len: target.len(),
                permissions: 0o777,
                modified,
            },
        }
    }
}

/// Make room for `extra` more bytes.
///
/// `Vec::try_reserve` grows geometrically, which can ask the heap for up to
/// twice the needed size and fail with `NoSpace` while plenty of memory is
/// still free. Fall back to an exact reservation before giving up.
fn grow_vec(data: &mut Vec<u8>, extra: usize) -> Result<(), FsError> {
    if data.try_reserve(extra).is_ok() {
        return Ok(());
    }
    data.try_reserve_exact(extra)
        .map_err(|_| FsError::NoSpace)
}

impl Default for RamFs {
    fn default() -> Self {
        Self::new()
    }
}

impl FileSystem for RamFs {
    fn name(&self) -> &'static str {
        "ramfs"
    }
    fn metadata(&self, path: &str) -> Result<Metadata, FsError> {
        let path = normalize_path(path)?;
        self.nodes
            .get(&path)
            .map(|node| Self::metadata_for(node, self.modified.get(&path).copied()))
            .ok_or(FsError::NotFound)
    }

    fn read(&self, path: &str, offset: usize, buffer: &mut [u8]) -> Result<usize, FsError> {
        let path = normalize_path(path)?;
        match self.nodes.get(&path).ok_or(FsError::NotFound)? {
            Node::Directory { .. } => Err(FsError::IsDirectory),
            Node::File { data, .. } => {
                if offset >= data.len() {
                    return Ok(0);
                }
                let count = buffer.len().min(data.len() - offset);
                buffer[..count].copy_from_slice(&data[offset..offset + count]);
                Ok(count)
            }
            Node::Symlink(_) => Err(FsError::InvalidData),
        }
    }

    fn read_link(&self, path: &str) -> Result<String, FsError> {
        let path = normalize_path(path)?;
        match self.nodes.get(&path).ok_or(FsError::NotFound)? {
            Node::Symlink(target) => Ok(target.clone()),
            _ => Err(FsError::InvalidData),
        }
    }

    fn write(&mut self, path: &str, offset: usize, bytes: &[u8]) -> Result<usize, FsError> {
        let path = normalize_path(path)?;
        let end = offset.checked_add(bytes.len()).ok_or(FsError::NoSpace)?;
        let written = match self.nodes.get_mut(&path).ok_or(FsError::NotFound)? {
            Node::Directory { .. } => return Err(FsError::IsDirectory),
            Node::File { data, .. } => {
                if bytes.is_empty() {
                    return Ok(0);
                }
                if end > data.len() {
                    grow_vec(data, end - data.len())?;
                    data.resize(end, 0);
                }
                data[offset..end].copy_from_slice(bytes);
                bytes.len()
            }
            Node::Symlink(_) => return Err(FsError::InvalidData),
        };
        if let Some(timestamp) = super::current_timestamp() {
            self.modified.insert(path, timestamp);
        }
        Ok(written)
    }

    fn truncate(&mut self, path: &str, len: usize) -> Result<(), FsError> {
        let path = normalize_path(path)?;
        match self.nodes.get_mut(&path).ok_or(FsError::NotFound)? {
            Node::Directory { .. } => return Err(FsError::IsDirectory),
            Node::File { data, .. } => {
                if len > data.len() {
                    grow_vec(data, len - data.len())?;
                    data.resize(len, 0);
                } else {
                    data.truncate(len);
                    // Give the memory back: it is the same heap every file shares.
                    data.shrink_to_fit();
                }
            }
            Node::Symlink(_) => return Err(FsError::InvalidData),
        }
        if let Some(timestamp) = super::current_timestamp() {
            self.modified.insert(path, timestamp);
        }
        Ok(())
    }

    fn set_permissions(&mut self, path: &str, permissions: u16) -> Result<(), FsError> {
        if permissions & !0o777 != 0 {
            return Err(FsError::InvalidData);
        }
        let path = normalize_path(path)?;
        match self.nodes.get_mut(&path).ok_or(FsError::NotFound)? {
            Node::Directory {
                permissions: current,
            }
            | Node::File {
                permissions: current,
                ..
            } => {
                *current = permissions;
                Ok(())
            }
            Node::Symlink(_) => Err(FsError::InvalidData),
        }
    }

    fn create_file(&mut self, path: &str) -> Result<(), FsError> {
        let path = normalize_path(path)?;
        if self.nodes.contains_key(&path) {
            return Err(FsError::AlreadyExists);
        }
        self.ensure_parent_directory(&path)?;
        self.nodes.insert(
            path.clone(),
            Node::File {
                data: Vec::new(),
                permissions: 0o644,
            },
        );
        if let Some(timestamp) = super::current_timestamp() {
            self.modified.insert(path, timestamp);
        }
        Ok(())
    }

    fn create_dir(&mut self, path: &str) -> Result<(), FsError> {
        let path = normalize_path(path)?;
        if self.nodes.contains_key(&path) {
            return Err(FsError::AlreadyExists);
        }
        self.ensure_parent_directory(&path)?;
        self.nodes
            .insert(path.clone(), Node::Directory { permissions: 0o755 });
        if let Some(timestamp) = super::current_timestamp() {
            self.modified.insert(path, timestamp);
        }
        Ok(())
    }

    fn create_symlink(&mut self, target: &str, path: &str) -> Result<(), FsError> {
        if target.is_empty() || target.as_bytes().contains(&0) {
            return Err(FsError::InvalidPath);
        }
        let path = normalize_path(path)?;
        if self.nodes.contains_key(&path) {
            return Err(FsError::AlreadyExists);
        }
        self.ensure_parent_directory(&path)?;
        self.nodes.insert(path.clone(), Node::Symlink(String::from(target)));
        if let Some(timestamp) = super::current_timestamp() {
            self.modified.insert(path, timestamp);
        }
        Ok(())
    }

    fn remove(&mut self, path: &str) -> Result<(), FsError> {
        let path = normalize_path(path)?;
        let node = self.nodes.get(&path).ok_or(FsError::NotFound)?;
        if path == "/" {
            return Err(FsError::Busy);
        }
        if matches!(node, Node::Directory { .. }) {
            let child_prefix = alloc::format!("{path}/");
            if self
                .nodes
                .keys()
                .any(|candidate| candidate.starts_with(&child_prefix))
            {
                return Err(FsError::DirectoryNotEmpty);
            }
        }
        self.nodes.remove(&path);
        self.modified.remove(&path);
        Ok(())
    }

    fn rename(&mut self, source: &str, target: &str) -> Result<(), FsError> {
        let source = normalize_path(source)?;
        let target = normalize_path(target)?;
        if source == "/" || target == "/" {
            return Err(FsError::Busy);
        }
        if source == target {
            return self
                .nodes
                .contains_key(&source)
                .then_some(())
                .ok_or(FsError::NotFound);
        }
        let source_node = self.nodes.get(&source).ok_or(FsError::NotFound)?;
        self.ensure_parent_directory(&target)?;

        let source_is_directory = matches!(source_node, Node::Directory { .. });
        if source_is_directory && target.starts_with(&alloc::format!("{source}/")) {
            return Err(FsError::InvalidPath);
        }
        if let Some(target_node) = self.nodes.get(&target) {
            let target_is_directory = matches!(target_node, Node::Directory { .. });
            if source_is_directory != target_is_directory {
                return Err(if target_is_directory {
                    FsError::IsDirectory
                } else {
                    FsError::NotDirectory
                });
            }
            if target_is_directory {
                let target_prefix = alloc::format!("{target}/");
                if self
                    .nodes
                    .keys()
                    .any(|path| path.starts_with(&target_prefix))
                {
                    return Err(FsError::DirectoryNotEmpty);
                }
            }
        }

        let source_prefix = alloc::format!("{source}/");
        let mut moved = Vec::new();
        for path in self.nodes.keys() {
            if path == &source || path.starts_with(&source_prefix) {
                moved.push((path.clone(), path.replacen(&source, &target, 1)));
            }
        }
        if self.nodes.contains_key(&target) {
            self.nodes.remove(&target);
        }
        for (old_path, new_path) in moved {
            if let Some(node) = self.nodes.remove(&old_path) {
                self.nodes.insert(new_path.clone(), node);
            }
            if let Some(timestamp) = self.modified.remove(&old_path) {
                self.modified.insert(new_path, timestamp);
            }
        }
        Ok(())
    }

    fn read_dir(&self, path: &str) -> Result<Vec<DirectoryEntry>, FsError> {
        let path = normalize_path(path)?;
        match self.nodes.get(&path).ok_or(FsError::NotFound)? {
            Node::File { .. } | Node::Symlink(_) => return Err(FsError::NotDirectory),
            Node::Directory { .. } => {}
        }

        let prefix = if path == "/" {
            String::from("/")
        } else {
            alloc::format!("{path}/")
        };
        let mut entries = Vec::new();
        for (candidate, node) in &self.nodes {
            let Some(name) = candidate.strip_prefix(&prefix) else {
                continue;
            };
            if name.is_empty() || name.contains('/') {
                continue;
            }
            entries.push(DirectoryEntry {
                name: String::from(name),
                metadata: Self::metadata_for(node, self.modified.get(candidate).copied()),
            });
        }
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modification_time_advances_after_file_mutations() {
        let mut filesystem = RamFs::new();
        filesystem.create_file("/mtime").unwrap();
        let created = filesystem.metadata("/mtime").unwrap().modified.unwrap();

        filesystem.write("/mtime", 0, b"first").unwrap();
        let written = filesystem.metadata("/mtime").unwrap().modified.unwrap();
        assert!(written > created);

        filesystem.truncate("/mtime", 2).unwrap();
        let truncated = filesystem.metadata("/mtime").unwrap().modified.unwrap();
        assert!(truncated > written);
        assert_eq!(
            filesystem.read_dir("/").unwrap()[0].metadata.modified,
            Some(truncated)
        );
    }

    #[test]
    fn truncate_removes_stale_bytes_after_shorter_overwrite() {
        let mut fs = RamFs::new();
        fs.create_file("/note").unwrap();
        fs.write("/note", 0, b"longer text").unwrap();
        fs.write("/note", 0, b"short").unwrap();
        fs.truncate("/note", 5).unwrap();

        let mut bytes = [0; 16];
        let count = fs.read("/note", 0, &mut bytes).unwrap();
        assert_eq!(&bytes[..count], b"short");
    }

    #[test]
    fn rename_moves_files_and_directory_subtrees_and_replaces_files() {
        let mut fs = RamFs::new();
        fs.create_dir("/old").unwrap();
        fs.create_file("/old/note").unwrap();
        fs.write("/old/note", 0, b"data").unwrap();
        fs.create_file("/existing").unwrap();

        fs.rename("/old", "/new").unwrap();
        let mut data = [0; 4];
        assert_eq!(fs.read("/new/note", 0, &mut data), Ok(4));
        assert_eq!(&data, b"data");
        fs.rename("/new/note", "/existing").unwrap();

        assert_eq!(fs.metadata("/old"), Err(FsError::NotFound));
        assert_eq!(fs.metadata("/new/note"), Err(FsError::NotFound));
        data.fill(0);
        assert_eq!(fs.read("/existing", 0, &mut data), Ok(4));
        assert_eq!(&data, b"data");
    }

    #[test]
    fn rename_rejects_directory_cycles_and_nonempty_directory_replacement() {
        let mut fs = RamFs::new();
        fs.create_dir("/parent").unwrap();
        fs.create_dir("/parent/child").unwrap();
        fs.create_file("/parent/child/file").unwrap();
        fs.create_dir("/destination").unwrap();
        fs.create_file("/destination/file").unwrap();

        assert_eq!(
            fs.rename("/parent", "/parent/child/moved"),
            Err(FsError::InvalidPath)
        );
        assert_eq!(
            fs.rename("/parent/child", "/destination"),
            Err(FsError::DirectoryNotEmpty)
        );
    }
}
