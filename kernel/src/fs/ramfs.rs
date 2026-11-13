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
}

impl RamFs {
    pub fn new() -> Self {
        let mut nodes = BTreeMap::new();
        nodes.insert(String::from("/"), Node::Directory { permissions: 0o755 });
        Self { nodes }
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

    fn metadata_for(node: &Node) -> Metadata {
        match node {
            Node::Directory { permissions } => Metadata {
                node_type: NodeType::Directory,
                len: 0,
                permissions: *permissions,
            },
            Node::File { data, permissions } => Metadata {
                node_type: NodeType::File,
                len: data.len(),
                permissions: *permissions,
            },
            Node::Symlink(target) => Metadata {
                node_type: NodeType::Symlink,
                len: target.len(),
                permissions: 0o777,
            },
        }
    }
}

impl Default for RamFs {
    fn default() -> Self {
        Self::new()
    }
}

impl FileSystem for RamFs {
    fn metadata(&self, path: &str) -> Result<Metadata, FsError> {
        let path = normalize_path(path)?;
        self.nodes
            .get(&path)
            .map(Self::metadata_for)
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
        match self.nodes.get_mut(&path).ok_or(FsError::NotFound)? {
            Node::Directory { .. } => Err(FsError::IsDirectory),
            Node::File { data, .. } => {
                if bytes.is_empty() {
                    return Ok(0);
                }
                if end > data.len() {
                    data.try_reserve(end - data.len())
                        .map_err(|_| FsError::NoSpace)?;
                    data.resize(end, 0);
                }
                data[offset..end].copy_from_slice(bytes);
                Ok(bytes.len())
            }
            Node::Symlink(_) => Err(FsError::InvalidData),
        }
    }

    fn truncate(&mut self, path: &str, len: usize) -> Result<(), FsError> {
        let path = normalize_path(path)?;
        match self.nodes.get_mut(&path).ok_or(FsError::NotFound)? {
            Node::Directory { .. } => Err(FsError::IsDirectory),
            Node::File { data, .. } => {
                if len > data.len() {
                    data.try_reserve(len - data.len())
                        .map_err(|_| FsError::NoSpace)?;
                    data.resize(len, 0);
                } else {
                    data.truncate(len);
                }
                Ok(())
            }
            Node::Symlink(_) => Err(FsError::InvalidData),
        }
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
            path,
            Node::File {
                data: Vec::new(),
                permissions: 0o644,
            },
        );
        Ok(())
    }

    fn create_dir(&mut self, path: &str) -> Result<(), FsError> {
        let path = normalize_path(path)?;
        if self.nodes.contains_key(&path) {
            return Err(FsError::AlreadyExists);
        }
        self.ensure_parent_directory(&path)?;
        self.nodes
            .insert(path, Node::Directory { permissions: 0o755 });
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
        self.nodes.insert(path, Node::Symlink(String::from(target)));
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
                metadata: Self::metadata_for(node),
            });
        }
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
