use crate::interrupt::{
    divide_error_stub, double_fault_stub, general_protection_fault_stub, page_fault_stub,
};
use core::arch::asm;

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

#[repr(C, packed)]
struct IdtDescriptor {
    limit: u16,
    base: u64,
}

static mut IDT: [IdtEntry; 256] = [IdtEntry::missing(); 256];

pub unsafe fn load_idt() {
    IDT[0] = IdtEntry::new(divide_error_stub as u64, 0x08);
    IDT[8] = IdtEntry::new(double_fault_stub as u64, 0x08);
    IDT[13] = IdtEntry::new(general_protection_fault_stub as u64, 0x08);
    IDT[14] = IdtEntry::new(page_fault_stub as u64, 0x08);
    // ... isi vector lain

    let descriptor = IdtDescriptor {
        limit: (core::mem::size_of::<IdtEntry>() * 256 - 1) as u16,
        base: core::ptr::addr_of!(IDT) as u64,
    };

    asm!("lidt [{0}]", in(reg) &descriptor);
}
