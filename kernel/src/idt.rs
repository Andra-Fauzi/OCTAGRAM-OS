//! IDT (Interrupt Descriptor Table): bikin 256 entry, isi exception
//! CPU yang penting (#DE, #BP, #NMI, #DF, #GP, #PF) dengan stub
//! spesifik, sisanya (vector 33..255) dengan stub generik dari
//! `interrupt.rs`, lalu load lewat `lidt`.

use crate::interrupt::{self, xhci_irq_stub};
use crate::interrupt::{
    breakpoint_stub, debug_stub, divide_error_stub, double_fault_stub,
    general_protection_fault_stub, nmi_stub, page_fault_stub,
};
use core::arch::asm;
use seq_macro::seq;

// Layout memori harus persis sesuai spec IDT gate descriptor -- field
// individual dibaca CPU lewat `lidt`, bukan lewat kode Rust, makanya
// banyak yang "never read" dari sudut pandang compiler.
#[allow(dead_code)]
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    type_attr: u8,
    offset_mid: u16,
    offset_high: u32,
    zero: u32,
}

impl IdtEntry {
    const fn missing() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            type_attr: 0,
            offset_mid: 0,
            offset_high: 0,
            zero: 0,
        }
    }

    fn new(handler: u64, selector: u16) -> Self {
        Self {
            offset_low: handler as u16,
            selector,
            ist: 0,
            type_attr: 0x8E, // Present, Ring0, 64-bit Interrupt Gate
            offset_mid: (handler >> 16) as u16,
            offset_high: (handler >> 32) as u32,
            zero: 0,
        }
    }
}

#[allow(dead_code)]
#[repr(C, packed)]
struct IdtDescriptor {
    limit: u16,
    base: u64,
}

static mut IDT: [IdtEntry; 256] = [IdtEntry::missing(); 256];

pub unsafe fn load_idt() {
    // Semua penulisan ke `static mut IDT` & inline asm wajib eksplisit
    // `unsafe { }` di edisi 2024 (unsafe_op_in_unsafe_fn).
    unsafe {
        IDT[0] = IdtEntry::new(divide_error_stub as u64, 0x08);
        IDT[1] = IdtEntry::new(debug_stub as u64, 0x08);
        IDT[2] = IdtEntry::new(nmi_stub as u64, 0x08);
        IDT[3] = IdtEntry::new(breakpoint_stub as u64, 0x08);
        IDT[8] = IdtEntry::new(double_fault_stub as u64, 0x08);
        IDT[13] = IdtEntry::new(general_protection_fault_stub as u64, 0x08);
        IDT[14] = IdtEntry::new(page_fault_stub as u64, 0x08);
        // ... isi vector lain
        seq!(N in 33..255 {
            IDT[N] = IdtEntry::new(interrupt::irq~N as u64, 0x08);
        });
        IDT[44] = IdtEntry::new(xhci_irq_stub as u64, 0x08);

        let descriptor = IdtDescriptor {
            limit: (core::mem::size_of::<IdtEntry>() * 256 - 1) as u16,
            base: core::ptr::addr_of!(IDT) as u64,
        };

        asm!("lidt [{0}]", in(reg) &descriptor);
    }
}
