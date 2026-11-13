//! Abstraksi arsitektur. Saat ini hanya mendukung x86_64.
//!
//! Tambahkan `#[cfg(target_arch = "aarch64")]` dst untuk arsitektur lain.

#[cfg(target_arch = "x86_64")]
pub mod x86_64;
