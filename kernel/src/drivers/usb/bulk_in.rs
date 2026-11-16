//! Bookkeeping for the single bulk-IN transfer that network class drivers keep
//! armed.
//!
//! The xHCI completion handler cannot call into a class driver (it would take a
//! lock that the deferred poll path also takes), so it only records which
//! endpoint completed and how. USB tethering class drivers — RNDIS and CDC-NCM
//! — share this state because a machine has at most one tethering adapter.

use core::sync::atomic::{AtomicBool, AtomicU32};

/// Device context index of the armed bulk-IN endpoint, 0 when nothing is armed.
pub static BULK_IN_DCI: AtomicU32 = AtomicU32::new(0);
/// Slot that owns [`BULK_IN_DCI`].
pub static BULK_IN_SLOT_ID: AtomicU32 = AtomicU32::new(0);
/// Set by the xHCI interrupt handler when the armed transfer completes.
pub static BULK_IN_PENDING: AtomicBool = AtomicBool::new(false);
/// Transfer residual (bytes not transferred) reported on completion.
pub static BULK_IN_LENGTH: AtomicU32 = AtomicU32::new(0);
/// xHCI completion code of the finished transfer.
pub static BULK_IN_CODE: AtomicU32 = AtomicU32::new(0);

/// Forget the armed endpoint, e.g. after the adapter was torn down.
pub fn disarm() {
    BULK_IN_DCI.store(0, core::sync::atomic::Ordering::SeqCst);
    BULK_IN_SLOT_ID.store(0, core::sync::atomic::Ordering::SeqCst);
    BULK_IN_PENDING.store(false, core::sync::atomic::Ordering::SeqCst);
}
