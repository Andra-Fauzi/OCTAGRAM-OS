use core::arch::asm;

#[repr(C, align(16))]
#[derive(Clone, Copy)]
pub struct State {
    registers: [u8; 512],
}

impl State {
    pub const fn new() -> Self {
        let mut registers = [0; 512];
        registers[0] = 0x7f;
        registers[1] = 0x03;
        registers[24] = 0x80;
        registers[25] = 0x1f;
        Self { registers }
    }
}

pub unsafe fn init() {
    let mut cr0: u64;
    unsafe {
        asm!("mov {}, cr0", out(reg) cr0, options(nomem, nostack, preserves_flags));
    }
    cr0 &= !((1 << 2) | (1 << 3));
    cr0 |= (1 << 1) | (1 << 5);
    unsafe {
        asm!("mov cr0, {}", in(reg) cr0, options(nomem, nostack, preserves_flags));
    }

    let mut cr4: u64;
    unsafe {
        asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
    }
    cr4 |= (1 << 9) | (1 << 10);
    unsafe {
        asm!("mov cr4, {}", in(reg) cr4, options(nomem, nostack, preserves_flags));
        asm!("fninit", options(nomem, nostack));
    }
}

pub unsafe fn save(state: &mut State) {
    unsafe {
        asm!(
            "fxsave64 [{}]",
            in(reg) state.registers.as_mut_ptr(),
            options(nostack)
        );
    }
}

pub unsafe fn restore(state: &State) {
    unsafe {
        asm!(
            "fxrstor64 [{}]",
            in(reg) state.registers.as_ptr(),
            options(nostack)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{State, restore, save};

    #[test]
    fn fpu_state_has_hardware_layout_and_default_controls() {
        let state = State::new();
        assert_eq!(core::mem::size_of::<State>(), 512);
        assert_eq!(core::mem::align_of::<State>(), 16);
        assert_eq!(&state.registers[..2], &[0x7f, 0x03]);
        assert_eq!(&state.registers[24..26], &[0x80, 0x1f]);
    }

    #[test]
    fn fpu_state_round_trips_through_fxsave() {
        let mut state = State::new();
        unsafe {
            save(&mut state);
            restore(&state);
        }
    }
}
