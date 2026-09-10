#[repr(C)]
#[derive(Debug)]
pub struct InterruptStackFrame {
    pub instruction_pointer: u64,
    pub code_segment: u64,
    pub cpu_flags: u64,
    pub stack_pointer: u64,
    pub stack_segment: u64,
}

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

use crate::apic;

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
        "call {handler}",

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
extern "C" fn common_interrupt_handler(frame: *mut InterruptFrame) {
    let frame = unsafe { &*frame };
    crate::println!(
        "Interrupt vector={} error_code={:#x} rip={:#x}",
        frame.vector,
        frame.error_code,
        frame.stack_frame.instruction_pointer
    );
    if frame.vector >= 32 {
        unsafe {
            if let Some(lapic) = apic::LAPIC.get() {
                unsafe {
                    lapic.send_eoi();
                }
            }
        }
    }
}

interrupt_stub!(divide_error_stub, 0, no_error_code);
interrupt_stub!(debug_stub, 1, no_error_code);
interrupt_stub!(nmi_stub, 2, no_error_code);
interrupt_stub!(breakpoint_stub, 3, no_error_code);
interrupt_stub!(double_fault_stub, 8, has_error_code);
interrupt_stub!(general_protection_fault_stub, 13, has_error_code);
interrupt_stub!(page_fault_stub, 14, has_error_code);
