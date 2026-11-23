//! OCTAGRAM public raw Unix aliases used by std::os::unix::raw.
#![allow(non_camel_case_types)]

#[unstable(feature = "std_internals", issue = "none")]
pub type pthread_t = u64;
#[unstable(feature = "std_internals", issue = "none")]
pub type blkcnt_t = i64;
#[unstable(feature = "std_internals", issue = "none")]
pub type time_t = i64;
#[unstable(feature = "std_internals", issue = "none")]
pub type blksize_t = i64;
#[unstable(feature = "std_internals", issue = "none")]
pub type dev_t = u64;
#[unstable(feature = "std_internals", issue = "none")]
pub type ino_t = u64;
#[unstable(feature = "std_internals", issue = "none")]
pub type mode_t = u32;
#[unstable(feature = "std_internals", issue = "none")]
pub type nlink_t = u32;
#[unstable(feature = "std_internals", issue = "none")]
pub type off_t = i64;
