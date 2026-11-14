use core::arch::asm;

use crate::memory::paging::{USER_ADDRESS_MAX, USER_ADDRESS_MIN};

const IA32_FS_BASE: u32 = 0xC000_0100;

pub fn is_valid_user_thread_pointer(value: u64) -> bool {
    value == 0 || (USER_ADDRESS_MIN..USER_ADDRESS_MAX).contains(&value)
}

pub fn read() -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        asm!(
            "rdmsr",
            in("ecx") IA32_FS_BASE,
            out("eax") low,
            out("edx") high,
            options(nostack, preserves_flags)
        );
    }
    (u64::from(high) << 32) | u64::from(low)
}

pub fn write(value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") IA32_FS_BASE,
            in("eax") low,
            in("edx") high,
            options(nostack, preserves_flags)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::is_valid_user_thread_pointer;
    use crate::memory::paging::{USER_ADDRESS_MAX, USER_ADDRESS_MIN};

    #[test]
    fn accepts_null_and_canonical_user_thread_pointers() {
        assert!(is_valid_user_thread_pointer(0));
        assert!(is_valid_user_thread_pointer(USER_ADDRESS_MIN));
        assert!(is_valid_user_thread_pointer(USER_ADDRESS_MAX - 1));
    }

    #[test]
    fn rejects_non_user_thread_pointers() {
        assert!(!is_valid_user_thread_pointer(USER_ADDRESS_MIN - 1));
        assert!(!is_valid_user_thread_pointer(USER_ADDRESS_MAX));
        assert!(!is_valid_user_thread_pointer(u64::MAX));
    }
}
