//! OCTAGRAM metadata extensions for the generic Unix std filesystem PAL.
//!
//! The current OCTAGRAM stat ABI has second-resolution timestamps and does not
//! expose a special-device ID. Missing subsecond/rdev values are reported as 0.

use crate::fs::Metadata;
use crate::sys::AsInner;

pub trait MetadataExt {
    fn st_dev(&self) -> u64;
    fn st_ino(&self) -> u64;
    fn st_mode(&self) -> u32;
    fn st_nlink(&self) -> u64;
    fn st_uid(&self) -> u32;
    fn st_gid(&self) -> u32;
    fn st_rdev(&self) -> u64;
    fn st_size(&self) -> u64;
    fn st_atime(&self) -> i64;
    fn st_atime_nsec(&self) -> i64;
    fn st_mtime(&self) -> i64;
    fn st_mtime_nsec(&self) -> i64;
    fn st_ctime(&self) -> i64;
    fn st_ctime_nsec(&self) -> i64;
    fn st_blksize(&self) -> u64;
    fn st_blocks(&self) -> u64;
}

impl MetadataExt for Metadata {
    fn st_dev(&self) -> u64 { self.as_inner().as_inner().st_dev as u64 }
    fn st_ino(&self) -> u64 { self.as_inner().as_inner().st_ino as u64 }
    fn st_mode(&self) -> u32 { self.as_inner().as_inner().st_mode as u32 }
    fn st_nlink(&self) -> u64 { self.as_inner().as_inner().st_nlink as u64 }
    fn st_uid(&self) -> u32 { self.as_inner().as_inner().st_uid as u32 }
    fn st_gid(&self) -> u32 { self.as_inner().as_inner().st_gid as u32 }
    fn st_rdev(&self) -> u64 { 0 }
    fn st_size(&self) -> u64 { self.as_inner().as_inner().st_size as u64 }
    fn st_atime(&self) -> i64 { self.as_inner().as_inner().st_atime as i64 }
    fn st_atime_nsec(&self) -> i64 { 0 }
    fn st_mtime(&self) -> i64 { self.as_inner().as_inner().st_mtime as i64 }
    fn st_mtime_nsec(&self) -> i64 { 0 }
    fn st_ctime(&self) -> i64 { self.as_inner().as_inner().st_ctime as i64 }
    fn st_ctime_nsec(&self) -> i64 { 0 }
    fn st_blksize(&self) -> u64 { self.as_inner().as_inner().st_blksize as u64 }
    fn st_blocks(&self) -> u64 { self.as_inner().as_inner().st_blocks as u64 }
}
