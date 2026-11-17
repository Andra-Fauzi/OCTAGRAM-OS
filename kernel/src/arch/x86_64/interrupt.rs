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
#[derive(Clone, Copy, Debug)]
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
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use seq_macro::seq;

use crate::arch::x86_64::apic;
use crate::drivers::keyboard;

pub const TIMER_VECTOR: u8 = 32;
/// Local APIC spurious-interrupt vector programmed in the SVR.
pub const SPURIOUS_VECTOR: u8 = 255;
const SPURIOUS_VECTOR_U64: u64 = SPURIOUS_VECTOR as u64;

static UNKNOWN_EXTERNAL_REPORTED: AtomicBool = AtomicBool::new(false);
/// Set once the fatal CPU-exception record has been written out.
///
/// A fault that repeats (or one that happens inside the reporter) must not
/// re-enter it: every entry would print another partial record and the screen
/// would end up covered in fragments instead of the original vector and RIP.
static FATAL_REPORTED: AtomicBool = AtomicBool::new(false);
static UNKNOWN_EXTERNAL_PENDING: AtomicBool = AtomicBool::new(false);
static UNKNOWN_EXTERNAL_VECTOR: AtomicU64 = AtomicU64::new(0);
static UNKNOWN_EXTERNAL_ERROR: AtomicU64 = AtomicU64::new(0);
static UNKNOWN_EXTERNAL_RIP: AtomicU64 = AtomicU64::new(0);

/// Return the first unhandled external interrupt captured by the ISR.
///
/// The interrupt handler only records scalar values; formatted output runs in
/// the main loop after the LAPIC EOI has been sent.
pub fn take_unknown_external_interrupt() -> Option<(u64, u64, u64)> {
    if !UNKNOWN_EXTERNAL_PENDING.swap(false, Ordering::Acquire) {
        return None;
    }
    Some((
        UNKNOWN_EXTERNAL_VECTOR.load(Ordering::Relaxed),
        UNKNOWN_EXTERNAL_ERROR.load(Ordering::Relaxed),
        UNKNOWN_EXTERNAL_RIP.load(Ordering::Relaxed),
    ))
}

fn record_unknown_external_interrupt(frame: &InterruptFrame) {
    // Preserve the first event so an interrupt storm cannot overwrite the
    // original vector/RIP before the main loop prints the diagnostic.
    if UNKNOWN_EXTERNAL_REPORTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        UNKNOWN_EXTERNAL_VECTOR.store(frame.vector, Ordering::Relaxed);
        UNKNOWN_EXTERNAL_ERROR.store(frame.error_code, Ordering::Relaxed);
        UNKNOWN_EXTERNAL_RIP.store(frame.stack_frame.instruction_pointer, Ordering::Relaxed);
        UNKNOWN_EXTERNAL_PENDING.store(true, Ordering::Release);
    }
}

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
        // Vector 45: QCA9377 MSI (`drivers::qca9377::QCA9377_MSI_VECTOR`).
        45 => {
            let _ = crate::drivers::qca9377::handle_irq();
            frame as *mut InterruptFrame
        }
        // Vector 44: xHCI. Hanya set flag -- TIDAK mengambil lock apa pun.
        // `XHCI_INSTANCE`/`KBD_STATE`/`MOUSE_STATE` juga dipakai jalur normal
        // dengan interrupt aktif, jadi `lock()` di dalam ISR bisa deadlock
        // (spin selamanya karena pemegang lock di-interupsi oleh dirinya
        // sendiri). Pemrosesan event dilakukan di luar ISR oleh
        // `usb::xhci::poll_interrupt_deferred()` dari idle loop.
        44 => {
            crate::drivers::usb::xhci::note_interrupt();
            frame as *mut InterruptFrame
        }
        // The APIC spurious vector has no device source and must not send EOI.
        SPURIOUS_VECTOR_U64 => frame as *mut InterruptFrame,
        128 => crate::elf::handle_syscall(frame),
        // Vector 129 is not reachable through the IDT: the `syscall` entry
        // stub pushes it so the shared restore path can dispatch Linux ABI
        // calls without confusing them with OCTAGRAM `int 0x80` calls.
        129 => crate::elf::handle_linux_syscall(frame),
        // Unknown external IRQs are handled before the user-CPL exception arm:
        // a hardware interrupt may arrive while a user task is running.
        _ if frame.vector >= 32 => {
            record_unknown_external_interrupt(frame);
            frame as *mut InterruptFrame
        }
        _ if frame.stack_frame.code_segment & 3 == 3 => {
            if frame.vector == 14 {
                let fault_address: u64;
                unsafe {
                    core::arch::asm!(
                        "mov {}, cr2",
                        out(reg) fault_address,
                        options(nomem, nostack, preserves_flags)
                    );
                }
                crate::println!(
                    "User page fault error_code={:#x} rip={:#x} address={:#x} fs_base={:#x}",
                    frame.error_code,
                    frame.stack_frame.instruction_pointer,
                    fault_address,
                    crate::arch::x86_64::thread_pointer::read()
                );
            } else {
                crate::println!(
                    "User exception vector={} error_code={:#x} rip={:#x} fs_base={:#x}",
                    frame.vector,
                    frame.error_code,
                    frame.stack_frame.instruction_pointer,
                    crate::arch::x86_64::thread_pointer::read()
                );
            }
            crate::elf::terminate_user(frame, u64::MAX)
        }
        _ => {
            // Do not use println! here: it takes the terminal lock and writes
            // to the framebuffer, either of which may have caused this fault.
            // Record the crash once, to COM1 and to the framebuffer, then halt.
            if !FATAL_REPORTED.swap(true, Ordering::AcqRel) {
                let fault_address = if frame.vector == 14 {
                    let address: u64;
                    unsafe {
                        core::arch::asm!(
                            "mov {}, cr2",
                            out(reg) address,
                            options(nomem, nostack, preserves_flags)
                        );
                    }
                    Some(address)
                } else {
                    None
                };
                crate::drivers::serial::write_kernel_exception(
                    frame.vector,
                    frame.error_code,
                    frame.stack_frame.instruction_pointer,
                    frame.stack_frame.code_segment,
                    frame.stack_frame.cpu_flags,
                    fault_address,
                );
                crate::terminal::show_kernel_exception(
                    frame.vector,
                    frame.error_code,
                    frame.stack_frame.instruction_pointer,
                    fault_address,
                );
            }
            loop {
                core::hint::spin_loop();
            }
        }
    };
    // Acknowledge all external vectors handled above except the architectural
    // APIC spurious vector (255), which explicitly does not receive an EOI.
    if frame.vector >= 32
        && frame.vector != 128
        && frame.vector != 129
        && frame.vector != SPURIOUS_VECTOR_U64
    {
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
interrupt_stub!(qca9377_irq_stub, 45, no_error_code);
interrupt_stub!(spurious_interrupt_stub, 255, no_error_code);
interrupt_stub!(syscall_stub, 128, no_error_code);

/// User stack pointer captured by the `syscall` entry stub.
///
/// `syscall` does not switch stacks, so the stub stores the ring-3 RSP here
/// with interrupts disabled (IF is cleared by IA32_FMASK) before switching to
/// the kernel stack and building an interrupt frame.
static mut SYSCALL_USER_RSP: u64 = 0;

/// Linux x86_64 system-call entry (`syscall` instruction).
///
/// On entry RCX holds the return RIP and R11 the user RFLAGS.  The stub
/// synthesises the same frame layout as an interrupt gate and jumps to the
/// shared restore path, which returns with `iretq`.
#[unsafe(naked)]
pub unsafe extern "C" fn syscall_entry() -> ! {
    naked_asm!(
        "mov qword ptr [rip + {user_rsp}], rsp",
        "mov rsp, qword ptr [rip + {kernel_top}]",
        "push 0x1b", // user data selector (RPL 3)
        "push qword ptr [rip + {user_rsp}]",
        "push r11",  // user RFLAGS
        "push 0x23", // user code selector (RPL 3)
        "push rcx",  // user RIP
        "push 0",    // error-code slot
        "push 129",  // Linux syscall vector marker
        "jmp {common}",
        user_rsp = sym SYSCALL_USER_RSP,
        kernel_top = sym super::gdt::RING0_STACK_TOP,
        common = sym stub_common,
    )
}

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
