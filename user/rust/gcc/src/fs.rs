use alloc::string::String;
use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    InvalidPath,
    NotFound,
    NotDirectory,
    PermissionDenied,
    NoSpace,
    IoError,
}

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
