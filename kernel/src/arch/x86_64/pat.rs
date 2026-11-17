//! Page Attribute Table (PAT) initialization for Write-Combining (WC) video memory caching.

use core::arch::asm;

pub const IA32_PAT_MSR: u32 = 0x277;

pub const PAT_TYPE_UC: u64 = 0x00;      // Uncacheable
pub const PAT_TYPE_WC: u64 = 0x01;      // Write Combining
pub const PAT_TYPE_WT: u64 = 0x04;      // Write Through
pub const PAT_TYPE_WP: u64 = 0x05;      // Write Protected
pub const PAT_TYPE_WB: u64 = 0x06;      // Write Back
pub const PAT_TYPE_UCMINUS: u64 = 0x07; // UC-

/// Initialize Page Attribute Table (PAT) on x86_64 CPU.
///
/// Configures PA1 as Write-Combining (WC) so any page mapped with
/// `PWT = 1, PCD = 0, PAT = 0` (e.g. framebuffer video memory)
/// benefits from hardware write combining buffers.
pub unsafe fn init() {
    if cfg!(test) {
        return;
    }
    // Standard layout with PA1 replaced with Write-Combining (0x01):
    // PA0: WB      (0x06)
    // PA1: WC      (0x01)  <-- replaces WT (0x04) with WC
    // PA2: UC-     (0x07)
    // PA3: UC      (0x00)
    // PA4: WB      (0x06)
    // PA5: WT      (0x04)
    // PA6: UC-     (0x07)
    // PA7: UC      (0x00)
    let pat_value: u64 = 0x00_07_04_06_00_07_01_06;
    let eax = pat_value as u32;
    let edx = (pat_value >> 32) as u32;
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") IA32_PAT_MSR,
            in("eax") eax,
            in("edx") edx,
            options(nomem, nostack, preserves_flags)
        );
    }
}
