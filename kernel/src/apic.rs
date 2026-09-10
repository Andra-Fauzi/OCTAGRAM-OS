use crate::{
    io::{inb, outb},
    memory::paging::map_mmio_page,
    memory::physical_to_virtual,
};
use spin::Once;

// GLOBAL
pub static LAPIC: Once<LocalApic> = Once::new();
//

const PIC1_CMD: u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
const PIC2_CMD: u16 = 0xA0;
const PIC2_DATA: u16 = 0xA1;

pub fn disable_pic() {
    unsafe {
        outb(PIC1_CMD, 0x11);
        outb(PIC2_CMD, 0x11);

        outb(PIC1_DATA, 0x20);
        outb(PIC2_DATA, 0x28);

        outb(PIC1_DATA, 0x04);
        outb(PIC2_DATA, 0x02);

        outb(PIC1_DATA, 0x01);
        outb(PIC2_DATA, 0x01);

        outb(PIC1_DATA, 0xFF);
        outb(PIC2_DATA, 0xFF);
    }
}

const IA32_APIC_BASE_MSR: u32 = 0x1B;

unsafe fn read_msr(msr: u32) -> u64 {
    let (low, high): (u32, u32);
    unsafe {
        core::arch::asm!(
            "rdmsr",
            in("ecx") msr,
            out("eax") low,
            out("edx") high,
        );
    }
    ((high as u64) << 32) | (low as u64)
}

unsafe fn write_msr(msr: u32, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;
    unsafe {
        core::arch::asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") low,
            in("edx") high,
        );
    }
}

pub unsafe fn enable_apic() -> u64 {
    unsafe {
        let mut base = read_msr(IA32_APIC_BASE_MSR);
        base |= 1 << 11;
        write_msr(IA32_APIC_BASE_MSR, base);
        base & 0xFFFFF000
    }
}

pub struct LocalApic {
    base: u64,
}

impl LocalApic {
    pub unsafe fn read(&self, offset: u32) -> u32 {
        unsafe { ((self.base + offset as u64) as *const u32).read_volatile() }
    }

    pub unsafe fn write(&self, offset: u32, value: u32) {
        unsafe { ((self.base + offset as u64) as *mut u32).write_volatile(value) }
    }

    pub unsafe fn send_eoi(&self) {
        unsafe { self.write(0xB0, 0) }
    }
}

const APIC_SPURIOUS_VECTOR_REG: u32 = 0xF0;

pub unsafe fn init_lapic() -> LocalApic {
    unsafe {
        let phys_base = enable_apic();
        let virt_base = map_mmio_page(phys_base);
        let lapic = LocalApic { base: virt_base };

        lapic.write(APIC_SPURIOUS_VECTOR_REG, 0x1FF);

        lapic
    }
}

pub struct IoApic {
    base: u64,
}

impl IoApic {
    unsafe fn read(&self, reg: u32) -> u32 {
        unsafe {
            (self.base as *mut u32).write_volatile(reg);
            ((self.base + 0x10) as *const u32).read_volatile()
        }
    }

    unsafe fn write(&self, reg: u32, value: u32) {
        unsafe {
            (self.base as *mut u32).write_volatile(reg);
            ((self.base + 0x10) as *mut u32).write_volatile(value);
        }
    }

    pub unsafe fn set_redirect(&self, irq: u8, vector: u8, apic_id: u8) {
        unsafe {
            let low_index = 0x10 + (irq as u32) * 2;
            let high_index = low_index + 1;

            let high = (apic_id as u32) << 24;
            let low = (vector as u32);

            self.write(high_index, high);
            self.write(low_index, low);
        }
    }
}

pub unsafe fn init_ioapic() -> IoApic {
    let phys_base = 0xFEC00000u64;
    let virt_base = unsafe { map_mmio_page(phys_base) };
    let ioapic = IoApic { base: virt_base };

    unsafe {
        ioapic.set_redirect(1, 33, 0);
    }

    ioapic
}
