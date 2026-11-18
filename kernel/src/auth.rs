//! Basis data kata sandi `/etc/shadow`.
//!
//! Format satu baris: `user:salt:sha256(salt + password)` dengan hash heksadesimal
//! 64 karakter. Salt dibuat dari RNG kernel; ini bukan KDF berulang, hanya
//! penghalang agar hash tidak bisa dipakai ulang tanpa salt.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::fs::{self, FsError};

pub const SHADOW_PATH: &str = "/etc/shadow";
const SHADOW_HEADER: &str = "# user:salt:sha256(salt+password)\n";
const MAX_SHADOW_LENGTH: usize = 4096;
const HASH_HEX_LENGTH: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShadowEntry {
    pub user: String,
    pub salt: String,
    pub hash: String,
}

pub fn hash_password(salt: &str, password: &str) -> String {
    let mut input = String::with_capacity(salt.len() + password.len());
    input.push_str(salt);
    input.push_str(password);
    crate::sha256::digest_hex(input.as_bytes())
}

pub fn new_entry(user: &str, password: &str, random: u64) -> ShadowEntry {
    let salt = format!("{random:016x}");
    let hash = hash_password(&salt, password);
    ShadowEntry {
        user: user.to_string(),
        salt,
        hash,
    }
}

pub fn verify(entry: &ShadowEntry, password: &str) -> bool {
    hash_password(&entry.salt, password) == entry.hash
}

/// Baris yang tidak lengkap, berisi hash bukan heksadesimal, atau user kosong
/// dilewati agar berkas rusak tidak mengunci semua akun.
pub fn parse(text: &str) -> Vec<ShadowEntry> {
    let mut entries = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split(':');
        let (Some(user), Some(salt), Some(hash), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        if user.is_empty()
            || salt.is_empty()
            || hash.len() != HASH_HEX_LENGTH
            || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            continue;
        }
        entries.push(ShadowEntry {
            user: user.to_string(),
            salt: salt.to_string(),
            hash: hash.to_string(),
        });
    }
    entries
}

pub fn format_entries(entries: &[ShadowEntry]) -> String {
    let mut text = String::from(SHADOW_HEADER);
    for entry in entries {
        text.push_str(&format!("{}:{}:{}\n", entry.user, entry.salt, entry.hash));
    }
    text
}

/// Ganti entri user yang sudah ada atau tambahkan entri baru.
pub fn upsert(entries: &mut Vec<ShadowEntry>, entry: ShadowEntry) {
    match entries.iter_mut().find(|existing| existing.user == entry.user) {
        Some(existing) => *existing = entry,
        None => entries.push(entry),
    }
}

pub fn load() -> Vec<ShadowEntry> {
    let mut buffer = [0u8; MAX_SHADOW_LENGTH];
    match fs::with_vfs(|vfs| vfs.read(SHADOW_PATH, 0, &mut buffer)) {
        Ok(Ok(count)) => core::str::from_utf8(&buffer[..count])
            .map(parse)
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

pub fn save(entries: &[ShadowEntry]) -> Result<(), FsError> {
    let text = format_entries(entries);
    fs::with_vfs(|vfs| {
        match vfs.remove_file(SHADOW_PATH) {
            Ok(()) | Err(FsError::NotFound) => Ok(()),
            Err(error) => Err(error),
        }?;
        vfs.create_file(SHADOW_PATH)?;
        vfs.write(SHADOW_PATH, 0, text.as_bytes()).map(|_| ())
    })?
}

#[cfg(test)]
mod tests {
    use super::{ShadowEntry, format_entries, hash_password, new_entry, parse, upsert, verify};
    use alloc::string::String;
    use alloc::vec::Vec;

    #[test]
    fn entries_round_trip_and_verify_passwords() {
        let entry = new_entry("root", "rahasia", 0x0123_4567_89ab_cdef);
        assert_eq!(entry.salt, "0123456789abcdef");
        assert!(verify(&entry, "rahasia"));
        assert!(!verify(&entry, "salah"));

        let text = format_entries(&[entry.clone()]);
        assert!(text.starts_with("# user:salt:sha256"));
        assert_eq!(parse(&text), alloc::vec![entry]);
    }

    #[test]
    fn same_password_with_different_salts_hashes_differently() {
        let first = new_entry("octagram", "sama", 1);
        let second = new_entry("octagram", "sama", 2);
        assert_ne!(first.hash, second.hash);
        assert_eq!(hash_password("salt", "pw"), hash_password("salt", "pw"));
        assert_ne!(hash_password("salt", "pw"), hash_password("salt2", "pw"));
    }

    #[test]
    fn parser_skips_malformed_and_comment_lines() {
        let mut text = String::from("# komentar\n\nroot::kosong\n");
        text.push_str("octagram:salt:zz\n");
        text.push_str("lengkap:salt:");
        text.push_str(&"a".repeat(64));
        text.push('\n');
        text.push_str("ekstra:salt:hash:lain\n");
        let entries = parse(&text);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].user, "lengkap");
    }

    #[test]
    fn upsert_replaces_the_existing_user() {
        let mut entries = Vec::new();
        upsert(&mut entries, ShadowEntry {
            user: String::from("root"),
            salt: String::from("a"),
            hash: String::from("b"),
        });
        upsert(&mut entries, ShadowEntry {
            user: String::from("root"),
            salt: String::from("c"),
            hash: String::from("d"),
        });
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].salt, "c");
    }
}
