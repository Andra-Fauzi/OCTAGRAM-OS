//! OCTAGRAM public raw Unix aliases used by std::os::unix::raw.
#![allow(non_camel_case_types)]

pub type pthread_t = u64;
pub type blkcnt_t = i64;
pub type time_t = i64;
pub type blksize_t = i64;
pub type dev_t = u64;
pub type ino_t = u64;
pub type mode_t = u32;
pub type nlink_t = u32;
pub type off_t = i64;
