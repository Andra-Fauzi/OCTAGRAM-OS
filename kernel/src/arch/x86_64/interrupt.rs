//! Stub interrupt/exception level rendah: naked-asm entry point yang
//! nyimpen semua general-purpose register, panggil handler Rust biasa
//! (`common_interrupt_handler`), lalu restore & `iretq`.
//!
//! `interrupt_stub!` generate satu fungsi naked per vector; `seq!`
//! dipakai supaya vector 33..255 (semua IRQ non-exception) tidak perlu
//! ditulis manual satu-satu.

// Field individual di struct berikut cuma dibaca lewat memory layout
// (CPU yang isi lewat `push`, kode Rust cuma butuh sebagian field-nya
// misalnya `instruction_pointer` buat logging) -- bukan dead code yang
// sengaja tak terpakai.
#[allow(dead_code)]
#[repr(C)]
#[derive(Debug)]
pub struct InterruptStackFrame {
    pub instruction_pointer: u64,
    pub code_segment: u64,
    pub cpu_flags: u64,
    pub stack_pointer: u64,
    pub stack_segment: u64,
}

#[allow(dead_code)]
#[repr(C)]
pub struct InterruptFrame {
    // register yang kita push manual, urutannya harus cocok dengan urutan push di asm
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,

    pub vector: u64,     // nomor interrupt (kita push sendiri)
    pub error_code: u64, // error code asli/dummy

    pub stack_frame: InterruptStackFrame, // ini yang di-push CPU otomatis
}

use core::arch::naked_asm;
use seq_macro::seq;

use crate::arch::x86_64::apic;
use crate::drivers::keyboard;
use crate::drivers::usb::keyboard_usb;
use crate::drivers::usb::mouse_usb;

pub const TIMER_VECTOR: u8 = 32;

macro_rules! interrupt_stub {
    ($name:ident, $vector:expr, no_error_code) => {
        #[unsafe(naked)]
        pub unsafe extern "C" fn $name() -> ! {
            naked_asm!(
                "push 0",         // dummy error code
                "push {vector}",
                "jmp {common}",
                vector = const $vector,
                common = sym stub_common,
            )
        }
    };
    ($name:ident, $vector:expr, has_error_code) => {
        #[unsafe(naked)]
        pub unsafe extern "C" fn $name() -> ! {
            naked_asm!(
                "push {vector}",
                "jmp {common}",
                vector = const $vector,
                common = sym stub_common,
            )
        }
    };
}

#[unsafe(naked)]
unsafe extern "C" fn stub_common() -> ! {
    naked_asm!(
        "push rax",
        "push rbx",
        "push rcx",
        "push rdx",
        "push rsi",
        "push rdi",
        "push rbp",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "push r12",
        "push r13",
        "push r14",
        "push r15",

        "mov rdi, rsp",
        "and rsp, -16",
        "call {handler}",
        "mov rsp, rax",

        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rbp",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop rcx",
        "pop rbx",
        "pop rax",

        "add rsp, 16",
        "iretq",

        handler = sym common_interrupt_handler,
    )
}

#[unsafe(no_mangle)]
extern "C" fn common_interrupt_handler(frame: *mut InterruptFrame) -> *mut InterruptFrame {
    let frame = unsafe { &mut *frame };
    let return_frame = match frame.vector {
        32 => crate::elf::handle_timer(frame),
        33 => {
            keyboard::handle_ps2_irq();
            frame as *mut InterruptFrame
        }
        44 => {
            if let Some(xhci_mutex) = crate::drivers::usb::xhci::XHCI_INSTANCE.get() {
                let mut xhci = xhci_mutex.lock();
                unsafe {
                    xhci.poll_event_ring();
                }
            }
            unsafe {
                keyboard_usb::poll_keyboard();
                mouse_usb::poll_mouse();
            }
            frame as *mut InterruptFrame
        }
        128 => crate::elf::handle_syscall(frame),
        _ if frame.stack_frame.code_segment & 3 == 3 => crate::elf::terminate_user(frame, u64::MAX),
        _ => {
            crate::println!(
                "Interrupt vector={} error_code={:#x} rip={:#x}",
                frame.vector,
                frame.error_code,
                frame.stack_frame.instruction_pointer
            );
            loop {}
        }
    };
    if matches!(frame.vector, 32 | 33 | 44) {
        if let Some(lapic) = apic::LAPIC.get() {
            unsafe {
                lapic.send_eoi();
            }
        }
    }
    return_frame
}

interrupt_stub!(divide_error_stub, 0, no_error_code);
interrupt_stub!(timer_irq_stub, 32, no_error_code);
interrupt_stub!(debug_stub, 1, no_error_code);
interrupt_stub!(nmi_stub, 2, no_error_code);
interrupt_stub!(breakpoint_stub, 3, no_error_code);
interrupt_stub!(double_fault_stub, 8, has_error_code);
interrupt_stub!(general_protection_fault_stub, 13, has_error_code);
interrupt_stub!(page_fault_stub, 14, has_error_code);

seq!(N in 4..8 {
    interrupt_stub!(exception_stub~N, N, no_error_code);
});
interrupt_stub!(exception_stub9, 9, no_error_code);
interrupt_stub!(exception_stub10, 10, has_error_code);
interrupt_stub!(exception_stub11, 11, has_error_code);
interrupt_stub!(exception_stub12, 12, has_error_code);
seq!(N in 15..17 {
    interrupt_stub!(exception_stub~N, N, no_error_code);
});
interrupt_stub!(exception_stub17, 17, has_error_code);
seq!(N in 18..21 {
    interrupt_stub!(exception_stub~N, N, no_error_code);
});
interrupt_stub!(exception_stub21, 21, has_error_code);
seq!(N in 22..29 {
    interrupt_stub!(exception_stub~N, N, no_error_code);
});
interrupt_stub!(exception_stub29, 29, has_error_code);
interrupt_stub!(exception_stub30, 30, has_error_code);
interrupt_stub!(exception_stub31, 31, no_error_code);

// stub generic buat semua vector yang belum di-handle spesifik.
// nama fungsinya: irq33, irq34, ..., irq254
seq!(N in 33..255 {
    interrupt_stub!(irq~N, N, no_error_code);
});

interrupt_stub!(xhci_irq_stub, 44, no_error_code);
interrupt_stub!(syscall_stub, 128, no_error_code);

#[cfg(test)]
mod tests {
    use super::{InterruptFrame, InterruptStackFrame};

    #[test]
    fn interrupt_frame_matches_the_assembly_save_layout() {
        assert_eq!(core::mem::offset_of!(InterruptFrame, vector), 120);
        assert_eq!(core::mem::offset_of!(InterruptFrame, stack_frame), 136);
        assert_eq!(
            core::mem::size_of::<InterruptFrame>(),
            15 * core::mem::size_of::<u64>()
                + 2 * core::mem::size_of::<u64>()
                + core::mem::size_of::<InterruptStackFrame>()
        );
    }
}
