//! OCTAGRAM public raw Unix aliases used by std::os::unix::raw.
#![allow(non_camel_case_types)]

pub(crate) type pthread_t = u64;
pub(crate) type blkcnt_t = i64;
pub(crate) type time_t = i64;
pub(crate) type blksize_t = i64;
pub(crate) type dev_t = u64;
pub(crate) type ino_t = u64;
pub(crate) type mode_t = u32;
pub(crate) type nlink_t = u32;
pub(crate) type off_t = i64;
