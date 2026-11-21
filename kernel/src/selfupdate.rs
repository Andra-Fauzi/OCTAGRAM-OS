//! A/B self-update policy engine.
//!
//! This module implements the safety rules for updating the running system
//! from one boot to the next, independent of the storage and bootloader
//! plumbing that carries them out:
//!
//! * an image may only be installed into the **inactive** slot;
//! * the next boot may only be pointed at an image that has been fully
//!   written and verified;
//! * a failed boot is retried a bounded number of times and then rolls back to
//!   the last known-good slot;
//! * the active slot is never changed before the new image proves healthy.
//!
//! The state is serialized to a fixed 128-byte journal with a SHA-256 trailer
//! so a torn write is detected and rejected rather than trusted.
//!
//! Status: policy engine implemented and host-tested; image builder and
//! installer seed a valid journal, and the A/B image layout has two root
//! partitions plus one kernel copy per slot. Having the bootloader read the
//! journal and record boot success is tracked in
//! `docs/native-self-hosting-roadmap.md` M7.2.

use alloc::format;
use alloc::string::String;

use crate::sha256;

const JOURNAL_MAGIC: &[u8; 8] = b"OCTASU01";
const JOURNAL_VERSION: u8 = 1;
const JOURNAL_LEN: usize = 128;
const JOURNAL_PREFIX_LEN: usize = 96;
const SLOT_RECORD_LEN: usize = 40;

/// Number of boot attempts a pending slot gets before it is rolled back.
pub const MAX_BOOT_ATTEMPTS: u8 = 3;
/// Persistent journal location on the EXT2 root.
pub const JOURNAL_PATH: &str = "/var/lib/octagram/update-journal";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    A,
    B,
}

impl Slot {
    pub fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }

    pub(crate) fn index(self) -> usize {
        match self {
            Self::A => 0,
            Self::B => 1,
        }
    }

    fn encode(self) -> u8 {
        self.index() as u8
    }

    fn decode(value: u8) -> Result<Self, UpdateError> {
        match value {
            0 => Ok(Self::A),
            1 => Ok(Self::B),
            _ => Err(UpdateError::InvalidJournal),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotHealth {
    Empty,
    Installed,
    KnownGood,
    Failed,
}

impl SlotHealth {
    fn encode(self) -> u8 {
        match self {
            Self::Empty => 0,
            Self::Installed => 1,
            Self::KnownGood => 2,
            Self::Failed => 3,
        }
    }

    fn decode(value: u8) -> Result<Self, UpdateError> {
        match value {
            0 => Ok(Self::Empty),
            1 => Ok(Self::Installed),
            2 => Ok(Self::KnownGood),
            3 => Ok(Self::Failed),
            _ => Err(UpdateError::InvalidJournal),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotState {
    pub health: SlotHealth,
    /// Set only after the image in this slot was read back and hashed.
    pub verified: bool,
    pub image_sha256: [u8; 32],
}

impl SlotState {
    const EMPTY: Self = Self {
        health: SlotHealth::Empty,
        verified: false,
        image_sha256: [0u8; 32],
    };
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateJournal {
    pub active: Slot,
    pub known_good: Option<Slot>,
    pub pending: Option<Slot>,
    pub attempts_left: u8,
    pub slots: [SlotState; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateError {
    /// The requested target is the slot currently running.
    SlotInUse,
    /// Another update already owns the inactive slot.
    PendingUpdate,
    /// There is no pending update to boot or confirm.
    NoPendingUpdate,
    /// The candidate image was not written back and verified.
    ImageNotVerified,
    /// Rollback was requested without a known-good slot.
    NoKnownGoodSlot,
    /// The journal is corrupt or from an unsupported version.
    InvalidJournal,
}

impl UpdateJournal {
    pub const fn new() -> Self {
        Self {
            active: Slot::A,
            known_good: Some(Slot::A),
            pending: None,
            attempts_left: 0,
            slots: [SlotState::EMPTY; 2],
        }
    }

    /// Record that an image was written into `target`.
    ///
    /// `verified` must only be true after the whole image was read back and
    /// matched the expected digest. A false value keeps the update non-bootable
    /// until [`Self::mark_verified`] succeeds.
    pub fn install_image(
        &mut self,
        target: Slot,
        image_sha256: [u8; 32],
        verified: bool,
    ) -> Result<(), UpdateError> {
        if target == self.active {
            return Err(UpdateError::SlotInUse);
        }
        if self.pending.is_some() {
            return Err(UpdateError::PendingUpdate);
        }
        self.slots[target.index()] = SlotState {
            health: SlotHealth::Installed,
            verified,
            image_sha256,
        };
        self.pending = Some(target);
        self.attempts_left = 0;
        Ok(())
    }

    /// Mark the pending image as verified after a successful read-back.
    pub fn mark_verified(&mut self, image_sha256: [u8; 32]) -> Result<(), UpdateError> {
        let pending = self.pending.ok_or(UpdateError::NoPendingUpdate)?;
        let slot = &mut self.slots[pending.index()];
        if slot.image_sha256 != image_sha256 {
            return Err(UpdateError::InvalidJournal);
        }
        slot.verified = true;
        Ok(())
    }

    /// Point the next boot at the pending slot.
    ///
    /// This is the only operation that makes the candidate bootable, and it
    /// refuses to run before the image has been verified.
    pub fn request_boot_switch(&mut self) -> Result<Slot, UpdateError> {
        let pending = self.pending.ok_or(UpdateError::NoPendingUpdate)?;
        if !self.slots[pending.index()].verified {
            return Err(UpdateError::ImageNotVerified);
        }
        self.attempts_left = MAX_BOOT_ATTEMPTS;
        Ok(pending)
    }

    /// Slot the bootloader must select for this boot.
    ///
    /// A candidate that exhausted its attempts is not booted again; the
    /// caller is expected to call [`Self::rollback`] during that boot.
    pub fn boot_slot(&self) -> Slot {
        match self.pending {
            Some(pending) if self.attempts_left > 0 => pending,
            _ => self.active,
        }
    }

    /// Record one attempt to boot the candidate. Called once per boot of a
    /// pending slot, before the health check runs.
    pub fn record_boot_attempt(&mut self) {
        self.attempts_left = self.attempts_left.saturating_sub(1);
    }

    /// True when the candidate used up its attempts and the system must boot
    /// the known-good slot instead.
    pub fn rollback_required(&self) -> bool {
        self.pending.is_some() && self.attempts_left == 0
    }

    /// Commit the update: the pending slot becomes active and known-good.
    /// Only a caller that observed a successful boot may invoke this.
    pub fn confirm_healthy(&mut self) -> Result<Slot, UpdateError> {
        let pending = self.pending.ok_or(UpdateError::NoPendingUpdate)?;
        self.slots[pending.index()].health = SlotHealth::KnownGood;
        self.slots[pending.index()].verified = true;
        self.active = pending;
        self.known_good = Some(pending);
        self.pending = None;
        self.attempts_left = 0;
        Ok(pending)
    }

    /// Return to the known-good slot and mark the candidate as failed.
    pub fn rollback(&mut self) -> Result<Slot, UpdateError> {
        let known_good = self.known_good.ok_or(UpdateError::NoKnownGoodSlot)?;
        if let Some(pending) = self.pending {
            self.slots[pending.index()].health = SlotHealth::Failed;
        }
        self.active = known_good;
        self.pending = None;
        self.attempts_left = 0;
        Ok(known_good)
    }

    /// Record a previously installed slot as the known-good baseline.
    pub fn mark_known_good(&mut self, slot: Slot, image_sha256: [u8; 32]) {
        self.slots[slot.index()] = SlotState {
            health: SlotHealth::KnownGood,
            verified: true,
            image_sha256,
        };
        self.active = slot;
        self.known_good = Some(slot);
    }

    /// Fixed-size journal record with a SHA-256 trailer.
    pub fn to_bytes(&self) -> [u8; JOURNAL_LEN] {
        let mut bytes = [0u8; JOURNAL_LEN];
        bytes[..8].copy_from_slice(JOURNAL_MAGIC);
        bytes[8] = JOURNAL_VERSION;
        bytes[9] = self.active.encode();
        bytes[10] = self.known_good.map_or(0xFF, Slot::encode);
        bytes[11] = self.pending.map_or(0xFF, Slot::encode);
        bytes[12] = self.attempts_left;
        for (index, slot) in self.slots.iter().enumerate() {
            let start = 16 + index * SLOT_RECORD_LEN;
            bytes[start] = slot.health.encode();
            bytes[start + 1] = u8::from(slot.verified);
            bytes[start + 8..start + SLOT_RECORD_LEN].copy_from_slice(&slot.image_sha256);
        }
        let digest = sha256::digest(&bytes[..JOURNAL_PREFIX_LEN]);
        bytes[JOURNAL_PREFIX_LEN..].copy_from_slice(&digest);
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, UpdateError> {
        if bytes.len() != JOURNAL_LEN || &bytes[..8] != JOURNAL_MAGIC {
            return Err(UpdateError::InvalidJournal);
        }
        if bytes[8] != JOURNAL_VERSION {
            return Err(UpdateError::InvalidJournal);
        }
        let digest = sha256::digest(&bytes[..JOURNAL_PREFIX_LEN]);
        if bytes[JOURNAL_PREFIX_LEN..] != digest {
            return Err(UpdateError::InvalidJournal);
        }
        let decode_optional = |value: u8| -> Result<Option<Slot>, UpdateError> {
            match value {
                0xFF => Ok(None),
                other => Ok(Some(Slot::decode(other)?)),
            }
        };
        let mut slots = [SlotState::EMPTY; 2];
        for (index, slot) in slots.iter_mut().enumerate() {
            let start = 16 + index * SLOT_RECORD_LEN;
            let verified = match bytes[start + 1] {
                0 => false,
                1 => true,
                _ => return Err(UpdateError::InvalidJournal),
            };
            let mut digest = [0u8; 32];
            digest.copy_from_slice(&bytes[start + 8..start + SLOT_RECORD_LEN]);
            *slot = SlotState {
                health: SlotHealth::decode(bytes[start])?,
                verified,
                image_sha256: digest,
            };
        }
        Ok(Self {
            active: Slot::decode(bytes[9])?,
            known_good: decode_optional(bytes[10])?,
            pending: decode_optional(bytes[11])?,
            attempts_left: bytes[12],
            slots,
        })
    }

    /// Human-readable status for the shell.
    pub fn summary(&self) -> String {
        let slot = |slot: Slot| {
            let state = self.slots[slot.index()];
            let health = match state.health {
                SlotHealth::Empty => "empty",
                SlotHealth::Installed => "installed",
                SlotHealth::KnownGood => "known-good",
                SlotHealth::Failed => "failed",
            };
            format!(
                "{:?}: {health}{}",
                slot,
                if state.verified { "" } else { " (unverified)" }
            )
        };
        let name = |slot: Option<Slot>| match slot {
            None => "none",
            Some(Slot::A) => "A",
            Some(Slot::B) => "B",
        };
        format!(
            "active={} pending={} attempts_left={} {} {}",
            name(Some(self.active)),
            name(self.pending),
            self.attempts_left,
            slot(Slot::A),
            slot(Slot::B),
        )
    }
}

impl Default for UpdateJournal {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_BOOT_ATTEMPTS, Slot, SlotHealth, UpdateError, UpdateJournal,
    };

    fn digest(byte: u8) -> [u8; 32] {
        [byte; 32]
    }

    fn known_good_a() -> UpdateJournal {
        let mut journal = UpdateJournal::new();
        journal.mark_known_good(Slot::A, digest(0x11));
        journal
    }

    #[test]
    fn install_requires_the_inactive_slot() {
        let mut journal = known_good_a();
        assert_eq!(
            journal.install_image(Slot::A, digest(0x22), true),
            Err(UpdateError::SlotInUse)
        );

        journal.install_image(Slot::B, digest(0x22), true).unwrap();
        assert_eq!(
            journal.install_image(Slot::B, digest(0x33), true),
            Err(UpdateError::PendingUpdate)
        );
        assert_eq!(journal.pending, Some(Slot::B));
    }

    #[test]
    fn boot_switch_requires_a_verified_image() {
        let mut journal = known_good_a();
        journal.install_image(Slot::B, digest(0x22), false).unwrap();
        assert_eq!(
            journal.request_boot_switch(),
            Err(UpdateError::ImageNotVerified)
        );
        assert_eq!(journal.boot_slot(), Slot::A);

        journal.mark_verified(digest(0x22)).unwrap();
        assert_eq!(journal.request_boot_switch(), Ok(Slot::B));
        assert_eq!(journal.boot_slot(), Slot::B);
        assert_eq!(journal.attempts_left, MAX_BOOT_ATTEMPTS);
    }

    #[test]
    fn verification_must_match_the_installed_digest() {
        let mut journal = known_good_a();
        journal.install_image(Slot::B, digest(0x22), false).unwrap();
        assert_eq!(
            journal.mark_verified(digest(0x23)),
            Err(UpdateError::InvalidJournal)
        );
    }

    #[test]
    fn a_candidate_that_never_boots_rolls_back() {
        let mut journal = known_good_a();
        journal.install_image(Slot::B, digest(0x22), true).unwrap();
        journal.request_boot_switch().unwrap();

        for _ in 0..MAX_BOOT_ATTEMPTS {
            assert!(!journal.rollback_required());
            assert_eq!(journal.boot_slot(), Slot::B);
            journal.record_boot_attempt();
        }

        assert!(journal.rollback_required());
        assert_eq!(journal.boot_slot(), Slot::A);
        assert_eq!(journal.rollback(), Ok(Slot::A));
        assert_eq!(journal.active, Slot::A);
        assert_eq!(journal.pending, None);
        assert_eq!(journal.slots[Slot::B.index()].health, SlotHealth::Failed);
        assert_eq!(journal.known_good, Some(Slot::A));
    }

    #[test]
    fn a_healthy_candidate_becomes_the_known_good_slot() {
        let mut journal = known_good_a();
        journal.install_image(Slot::B, digest(0x22), true).unwrap();
        journal.request_boot_switch().unwrap();
        journal.record_boot_attempt();
        assert_eq!(journal.confirm_healthy(), Ok(Slot::B));

        assert_eq!(journal.active, Slot::B);
        assert_eq!(journal.known_good, Some(Slot::B));
        assert_eq!(journal.pending, None);
        assert_eq!(journal.slots[Slot::B.index()].health, SlotHealth::KnownGood);
        assert_eq!(journal.attempts_left, 0);
    }

    #[test]
    fn rollback_without_a_known_good_slot_is_refused() {
        let mut journal = UpdateJournal::new();
        journal.known_good = None;
        assert_eq!(
            journal.rollback(),
            Err(UpdateError::NoKnownGoodSlot)
        );
    }

    #[test]
    fn journal_round_trips_and_rejects_corruption() {
        let mut journal = known_good_a();
        journal.install_image(Slot::B, digest(0x22), true).unwrap();
        journal.request_boot_switch().unwrap();
        journal.record_boot_attempt();

        let bytes = journal.to_bytes();
        assert_eq!(UpdateJournal::from_bytes(&bytes), Ok(journal.clone()));
        assert!(journal.summary().contains("pending=B"));

        let mut corrupt = bytes;
        corrupt[20] ^= 0x01;
        assert_eq!(
            UpdateJournal::from_bytes(&corrupt),
            Err(UpdateError::InvalidJournal)
        );

        let mut truncated = bytes;
        truncated[127] = 0;
        assert_eq!(
            UpdateJournal::from_bytes(&truncated),
            Err(UpdateError::InvalidJournal)
        );
    }
}
