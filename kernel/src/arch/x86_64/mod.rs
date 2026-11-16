//! Platform-specific code untuk x86_64.

pub mod apic;
pub mod fpu;
pub mod gdt;
pub mod idt;
pub mod interrupt;
pub mod io;
pub mod thread_pointer;

/// Busy-wait for `milliseconds` using 8254 PIT channel 2.
///
/// Dipakai driver yang harus menunggu perangkat siap (mis. RNDIS control
/// transfer yang menunggu balasan dari HP) tanpa bergantung pada timer
/// interrupt, sama seperti `msleep()` di driver Linux. Channel 2 hanya bisa
/// menghitung ~55 ms sekali jalan, jadi delay dipecah per 50 ms.
pub fn delay_milliseconds(milliseconds: u32) {
    // Host tests tidak boleh menyentuh port I/O.
    if cfg!(test) {
        return;
    }
    let mut remaining = milliseconds;
    while remaining > 0 {
        let chunk = remaining.min(50);
        unsafe { pit_delay(chunk) };
        remaining -= chunk;
    }
}

unsafe fn pit_delay(milliseconds: u32) {
    const PIT_HZ: u32 = 1_193_182;
    let ticks = ((PIT_HZ / 1000) * milliseconds).clamp(1, 0xffff);
    unsafe {
        // Speaker off, channel 2 gate enabled (bit 0 of port 0x61).
        let port_61 = io::inb(0x61);
        io::outb(0x61, (port_61 & 0xfd) | 0x01);
        // Channel 2, mode 0 (interrupt on terminal count), lobyte/hibyte.
        io::outb(0x43, 0xb0);
        io::outb(0x42, (ticks & 0xff) as u8);
        io::outb(0x42, ((ticks >> 8) & 0xff) as u8);
        // OUT2 (port 0x61 bit 5) rises when the count reaches zero. The spin is
        // bounded so a machine without a working PIT cannot hang the kernel.
        let mut spins = 0u32;
        while io::inb(0x61) & 0x20 == 0 {
            spins += 1;
            if spins > 20_000_000 {
                break;
            }
            core::hint::spin_loop();
        }
    }
}
