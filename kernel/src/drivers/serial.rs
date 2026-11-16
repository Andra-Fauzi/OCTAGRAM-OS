//! Polled 16550-compatible COM1 output for kernel diagnostics.

use core::sync::atomic::{AtomicU8, Ordering};

const COM1: u16 = 0x3F8;
const DATA: u16 = COM1;
const INTERRUPT_ENABLE: u16 = COM1 + 1;
const FIFO_CONTROL: u16 = COM1 + 2;
const LINE_CONTROL: u16 = COM1 + 3;
const MODEM_CONTROL: u16 = COM1 + 4;
const LINE_STATUS: u16 = COM1 + 5;
const TRANSMIT_READY: u8 = 1 << 5;

static STATE: AtomicU8 = AtomicU8::new(0);

/// Configure COM1 for 38400 baud, 8 data bits, no parity, and one stop bit.
pub fn init() {
    if STATE
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    unsafe {
        crate::arch::x86_64::io::outb(INTERRUPT_ENABLE, 0x00);
        crate::arch::x86_64::io::outb(LINE_CONTROL, 0x80);
        crate::arch::x86_64::io::outb(DATA, 0x03);
        crate::arch::x86_64::io::outb(INTERRUPT_ENABLE, 0x00);
        crate::arch::x86_64::io::outb(LINE_CONTROL, 0x03);
        crate::arch::x86_64::io::outb(FIFO_CONTROL, 0xC7);
        crate::arch::x86_64::io::outb(MODEM_CONTROL, 0x0B);
    }
    STATE.store(2, Ordering::Release);
}

/// Write kernel diagnostic text using bounded polling so a missing UART
/// cannot stall the kernel.
pub fn write_str(text: &str) {
    if STATE.load(Ordering::Acquire) != 2 {
        return;
    }

    for byte in text.bytes() {
        if byte == b'\n' {
            write_byte(b'\r');
        }
        write_byte(byte);
    }
}

/// Write fatal CPU-exception details directly to COM1.
///
/// This deliberately bypasses the framebuffer terminal: an exception may have
/// happened while that terminal's lock is held or while framebuffer output is
/// the code that faulted. The path uses only bounded port-I/O polling and fixed
/// stack data, so it does not allocate or take a lock.
pub fn write_kernel_exception(
    vector: u64,
    error_code: u64,
    instruction_pointer: u64,
    code_segment: u64,
    flags: u64,
    fault_address: Option<u64>,
) {
    if STATE.load(Ordering::Acquire) != 2 {
        return;
    }

    write_str("\n[FATAL] kernel exception vector=");
    write_hex(vector);
    write_str(" error=");
    write_hex(error_code);
    write_str(" rip=");
    write_hex(instruction_pointer);
    write_str(" cs=");
    write_hex(code_segment);
    write_str(" rflags=");
    write_hex(flags);
    if let Some(address) = fault_address {
        write_str(" cr2=");
        write_hex(address);
    }
    write_str("\n");
}

fn write_hex(value: u64) {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    write_byte(b'0');
    write_byte(b'x');
    for shift in (0..16).rev() {
        let digit = ((value >> (shift * 4)) & 0x0f) as usize;
        write_byte(DIGITS[digit]);
    }
}

fn write_byte(byte: u8) {
    for _ in 0..100_000 {
        if unsafe { crate::arch::x86_64::io::inb(LINE_STATUS) } & TRANSMIT_READY != 0 {
            unsafe { crate::arch::x86_64::io::outb(DATA, byte) };
            return;
        }
        core::hint::spin_loop();
    }
}
