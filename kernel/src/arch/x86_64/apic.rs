//! Driver Local APIC & I/O APIC, plus disable legacy 8259 PIC.
//!
//! Alur pemakaian normal: [`init()`] yang akan memanggil `disable_pic()` →
//! `init_lapic()` (isi `LAPIC` global) → `init_ioapic()` untuk routing IRQ
//! legacy (mis. keyboard PS/2 lewat IRQ1) ke vector interrupt yang kita pakai.

use super::io::outb;
use crate::memory::paging::map_mmio_page;
use spin::Once;

/// Global Local APIC instance; diisi sekali saat boot oleh [`init()`].
pub static LAPIC: Once<LocalApic> = Once::new();

const PIC1_CMD: u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
const PIC2_CMD: u16 = 0xA0;
const PIC2_DATA: u16 = 0xA1;

/// Disable legacy 8259 PIC dengan mask semua IRQ-nya.
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

/// Enable APIC lewat MSR dan return physical base address.
pub unsafe fn enable_apic() -> u64 {
    unsafe {
        let mut base = read_msr(IA32_APIC_BASE_MSR);
        base |= 1 << 11;
        write_msr(IA32_APIC_BASE_MSR, base);
        base & 0xFFFFF000
    }
}

/// Handle ke Local APIC yang di-map ke virtual memory.
pub struct LocalApic {
    base: u64,
}

impl LocalApic {
    /// Baca register LAPIC di offset tertentu.
    pub unsafe fn read(&self, offset: u32) -> u32 {
        unsafe { ((self.base + offset as u64) as *const u32).read_volatile() }
    }

    /// Tulis register LAPIC di offset tertentu.
    pub unsafe fn write(&self, offset: u32, value: u32) {
        unsafe { ((self.base + offset as u64) as *mut u32).write_volatile(value) }
    }

    /// Kirim End-Of-Interrupt signal ke LAPIC.
    pub unsafe fn send_eoi(&self) {
        unsafe { self.write(0xB0, 0) }
    }

    /// Start a periodic local timer using the supplied vector and initial count.
    ///
    /// # Safety
    /// The IDT must contain a handler for `vector`, and that handler must send EOI.
    pub unsafe fn start_periodic_timer(&self, vector: u8, initial_count: u32) {
        unsafe {
            self.write(APIC_TIMER_DIVIDE_CONFIG_REG, 0x3);
            self.write(APIC_TIMER_LVT_REG, (1 << 17) | u32::from(vector));
            self.write(APIC_TIMER_INITIAL_COUNT_REG, initial_count);
        }
    }
}

const APIC_SPURIOUS_VECTOR_REG: u32 = 0xF0;
const APIC_TIMER_LVT_REG: u32 = 0x320;
const APIC_TIMER_INITIAL_COUNT_REG: u32 = 0x380;
const APIC_TIMER_DIVIDE_CONFIG_REG: u32 = 0x3E0;

/// Inisialisasi Local APIC: enable via MSR, map MMIO, enable spurious vector.
pub unsafe fn init_lapic() -> LocalApic {
    unsafe {
        let phys_base = enable_apic();
        let virt_base = map_mmio_page(phys_base);
        let lapic = LocalApic { base: virt_base };
        lapic.write(APIC_SPURIOUS_VECTOR_REG, 0x1FF);
        lapic
    }
}

/// Handle ke I/O APIC yang di-map ke virtual memory.
pub struct IoApic {
    base: u64,
}

impl IoApic {
    /// Baca register I/O APIC (disimpan untuk kebutuhan baca redirection entry nanti).
    #[allow(dead_code)]
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

    /// Set redirection entry: routing `irq` ke `vector` di CPU dengan `apic_id`.
    pub unsafe fn set_redirect(&self, irq: u8, vector: u8, apic_id: u8) {
        unsafe {
            let low_index = 0x10 + (irq as u32) * 2;
            let high_index = low_index + 1;

            let high = (apic_id as u32) << 24;
            let low = vector as u32;

            self.write(high_index, high);
            self.write(low_index, low);
        }
    }
}

/// Inisialisasi I/O APIC: map MMIO dan set redirect IRQ1 → vector 33.
///
/// Physical base diambil dari fixed address `0xFEC00000` (TODO: parse ACPI MADT).
pub unsafe fn init_ioapic() -> IoApic {
    let phys_base = 0xFEC00000u64;
    let virt_base = unsafe { map_mmio_page(phys_base) };
    let ioapic = IoApic { base: virt_base };

    unsafe {
        ioapic.set_redirect(1, 33, 0);
    }

    ioapic
}

/// Inisialisasi APIC: disable legacy PIC, init LAPIC global, init I/O APIC.
/// Dipanggil sekali saat boot setelah GDT dan IDT sudah di-load.
pub fn init() {
    disable_pic();
    LAPIC.call_once(|| unsafe { init_lapic() });
    unsafe { init_ioapic() };
    crate::println!("[apic] LAPIC + IOAPIC ready");
}
