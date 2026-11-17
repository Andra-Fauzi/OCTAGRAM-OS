//! GDT, TSS, and the ring-3 entry/return path for x86_64.

use core::arch::{asm, naked_asm};
use core::sync::atomic::{AtomicU64, Ordering};

pub const KERNEL_CODE_SELECTOR: u16 = 0x08;
pub const KERNEL_DATA_SELECTOR: u16 = 0x10;
pub const USER_DATA_SELECTOR: u16 = 0x1B;
pub const USER_CODE_SELECTOR: u16 = 0x23;
const TSS_SELECTOR: u16 = 0x28;
const USER_KERNEL_STACK_SIZE: usize = 64 * 1024;
/// Stack khusus double fault (IST1), terpisah dari stack yang sedang jalan.
const DOUBLE_FAULT_STACK_SIZE: usize = 16 * 1024;

#[repr(C, packed)]
struct GdtDescriptor {
    limit: u16,
    base: u64,
}

#[repr(C, packed)]
struct TaskStateSegment {
    reserved0: u32,
    rsp: [u64; 3],
    reserved1: u64,
    ist: [u64; 7],
    reserved2: u64,
    reserved3: u16,
    iomap_base: u16,
}

impl TaskStateSegment {
    const fn new() -> Self {
        Self {
            reserved0: 0,
            rsp: [0; 3],
            reserved1: 0,
            ist: [0; 7],
            reserved2: 0,
            reserved3: 0,
            iomap_base: core::mem::size_of::<Self>() as u16,
        }
    }
}

/// Ring-0 stack top used by the `syscall` entry stub, mirroring TSS.RSP0.
///
/// Hardware interrupts switch stacks through the TSS, while `syscall` does
/// not, so the stub needs a readable copy of the current process stack top.
pub(crate) static RING0_STACK_TOP: AtomicU64 = AtomicU64::new(0);

unsafe fn set_ring0_stack(stack_top: u64) {
    RING0_STACK_TOP.store(stack_top, Ordering::Release);
    let tss = core::ptr::addr_of_mut!(TSS).cast::<u8>();
    let rsp0 = unsafe {
        tss.add(core::mem::offset_of!(TaskStateSegment, rsp))
            .cast::<u64>()
    };
    unsafe {
        rsp0.write_unaligned(stack_top);
    }
}

/// Pasang `stack_top` sebagai stack IST ke-`index` (0-based: IST1 = 0).
unsafe fn set_interrupt_stack(index: usize, stack_top: u64) {
    let tss = core::ptr::addr_of_mut!(TSS).cast::<u8>();
    let entry = unsafe {
        tss.add(core::mem::offset_of!(TaskStateSegment, ist))
            .cast::<u64>()
            .add(index)
    };
    unsafe {
        entry.write_unaligned(stack_top);
    }
}

/// Update the kernel stack used for the next privilege transition from ring 3.
///
/// # Safety
/// `stack_top` must be the top of a mapped, writable kernel stack.
pub unsafe fn set_ring0_stack_top(stack_top: u64) {
    unsafe { set_ring0_stack(stack_top) }
}

pub fn ring0_stack_top() -> u64 {
    RING0_STACK_TOP.load(Ordering::Acquire)
}

const IA32_EFER: u32 = 0xc000_0080;
const IA32_STAR: u32 = 0xc000_0081;
const IA32_LSTAR: u32 = 0xc000_0082;
const IA32_FMASK: u32 = 0xc000_0084;

unsafe fn read_msr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        asm!(
            "rdmsr",
            in("ecx") msr,
            out("eax") low,
            out("edx") high,
            options(nomem, nostack),
        );
    }
    (u64::from(high) << 32) | u64::from(low)
}

unsafe fn write_msr(msr: u32, value: u64) {
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") value as u32,
            in("edx") (value >> 32) as u32,
            options(nomem, nostack),
        );
    }
}

/// Enable the `syscall`/`sysret` pair so Linux x86_64 ELF binaries can enter
/// the kernel through the ABI they were built for.
///
/// `syscall` loads CS from `STAR[47:32]` (the kernel code selector) and leaves
/// the return selector arithmetic to `sysret`: CS = `STAR[63:48] + 16` and
/// SS = `STAR[63:48] + 8`, both with RPL 3.  OCTAGRAM's GDT places user data
/// at 0x18 and user code at 0x20, so the high half is 0x13.
pub unsafe fn init_syscall_entry() {
    let star = ((u64::from(USER_DATA_SELECTOR) - 8) << 48)
        | (u64::from(KERNEL_CODE_SELECTOR) << 32);
    unsafe {
        write_msr(IA32_STAR, star);
        write_msr(IA32_LSTAR, super::interrupt::syscall_entry as u64);
        // Clear IF, TF, DF, AC and NT on entry, like Linux's SFMASK.
        write_msr(IA32_FMASK, 0x4_4700);
        write_msr(IA32_EFER, read_msr(IA32_EFER) | 1);
    }
}

#[repr(align(16))]
struct KernelStack([u8; USER_KERNEL_STACK_SIZE]);

#[repr(align(16))]
struct DoubleFaultStack([u8; DOUBLE_FAULT_STACK_SIZE]);

static mut GDT: [u64; 7] = [
    0,
    0x00AF_9A00_0000_FFFF, // ring 0 code
    0x00CF_9200_0000_FFFF, // ring 0 data
    0x00CF_F200_0000_FFFF, // ring 3 data
    0x00AF_FA00_0000_FFFF, // ring 3 code
    0,
    0,
];
static mut TSS: TaskStateSegment = TaskStateSegment::new();
static mut USER_KERNEL_STACK: KernelStack = KernelStack([0; USER_KERNEL_STACK_SIZE]);
/// IST1 untuk #DF. Wajib ada: kalau double fault dipicu oleh stack overflow
/// (stack yang sedang dipakai sudah habis), CPU tidak bisa push frame
/// exception ke stack itu dan langsung triple fault / reboot tanpa
/// melaporkan apa pun.
static mut DOUBLE_FAULT_STACK: DoubleFaultStack =
    DoubleFaultStack([0; DOUBLE_FAULT_STACK_SIZE]);
static mut USER_KERNEL_RSP: u64 = 0;
static mut USER_EXIT_STATUS: u64 = 0;
static mut GDTR: GdtDescriptor = GdtDescriptor {
    limit: (core::mem::size_of::<[u64; 7]>() - 1) as u16,
    base: 0,
};

pub fn user_kernel_stack_top() -> u64 {
    core::ptr::addr_of!(USER_KERNEL_STACK) as u64 + core::mem::size_of::<KernelStack>() as u64
}

pub unsafe fn load_gdt() {
    unsafe {
        set_ring0_stack(user_kernel_stack_top());
        set_interrupt_stack(
            0,
            core::ptr::addr_of!(DOUBLE_FAULT_STACK) as u64 + DOUBLE_FAULT_STACK_SIZE as u64,
        );

        let tss_base = core::ptr::addr_of!(TSS) as u64;
        let tss_limit = (core::mem::size_of::<TaskStateSegment>() - 1) as u64;
        let tss_low = (tss_limit & 0xFFFF)
            | ((tss_base & 0x00FF_FFFF) << 16)
            | (0x89 << 40)
            | (((tss_limit >> 16) & 0xF) << 48)
            | (((tss_base >> 24) & 0xFF) << 56);
        let gdt = core::ptr::addr_of_mut!(GDT);
        (*gdt)[5] = tss_low;
        (*gdt)[6] = tss_base >> 32;
        (*core::ptr::addr_of_mut!(GDTR)).base = gdt as u64;

        asm!(
            "lgdt [{gdtr}]",
            "mov ax, {kernel_data}",
            "mov ds, ax",
            "mov es, ax",
            "mov ss, ax",
            "mov ax, {tss_selector}",
            "ltr ax",
            "push {kernel_code}",
            "lea rax, [rip + 2f]",
            "push rax",
            "retfq",
            "2:",
            gdtr = in(reg) core::ptr::addr_of!(GDTR),
            kernel_data = const KERNEL_DATA_SELECTOR,
            tss_selector = const TSS_SELECTOR,
            kernel_code = const KERNEL_CODE_SELECTOR,
            out("rax") _,
        );
    }
}

pub unsafe fn enter_user_mode(entry: u64, user_stack: u64) -> u64 {
    unsafe {
        set_ring0_stack(user_kernel_stack_top());
        enter_user(entry, user_stack)
    }
}

pub unsafe fn resume_user_frame(frame: *mut u8) -> u64 {
    unsafe { resume_user(frame) }
}

#[unsafe(naked)]
unsafe extern "C" fn enter_user(entry: u64, user_stack: u64) -> u64 {
    naked_asm!(
        "push rbx",
        "push rbp",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "mov qword ptr [rip + {saved_rsp}], rsp",
        "mov ax, {user_data}",
        "mov ds, ax",
        "mov es, ax",
        "push {user_data}",
        "push rsi",
        "push 0x202",
        "push {user_code}",
        "push rdi",
        // Linux enters a fresh program with the general-purpose registers
        // zeroed (ELF_PLAT_INIT). Linux C runtimes read RDX as `rtld_fini`
        // and would otherwise register a stale kernel value as an exit
        // handler.
        "xor eax, eax",
        "xor ebx, ebx",
        "xor ecx, ecx",
        "xor edx, edx",
        "xor esi, esi",
        "xor edi, edi",
        "xor ebp, ebp",
        "xor r8d, r8d",
        "xor r9d, r9d",
        "xor r10d, r10d",
        "xor r11d, r11d",
        "xor r12d, r12d",
        "xor r13d, r13d",
        "xor r14d, r14d",
        "xor r15d, r15d",
        "iretq",
        saved_rsp = sym USER_KERNEL_RSP,
        user_data = const USER_DATA_SELECTOR,
        user_code = const USER_CODE_SELECTOR,
    );
}

#[unsafe(naked)]
unsafe extern "C" fn resume_user(frame: *mut u8) -> u64 {
    naked_asm!(
        "push rbx",
        "push rbp",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "mov qword ptr [rip + {saved_rsp}], rsp",
        "mov ax, {user_data}",
        "mov ds, ax",
        "mov es, ax",
        "mov rsp, rdi",
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
        saved_rsp = sym USER_KERNEL_RSP,
        user_data = const USER_DATA_SELECTOR,
    );
}

#[unsafe(naked)]
extern "C" fn user_exit_trampoline() {
    naked_asm!(
        "mov ax, {kernel_data}",
        "mov ds, ax",
        "mov es, ax",
        "mov ss, ax",
        "mov rsp, qword ptr [rip + {saved_rsp}]",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbp",
        "pop rbx",
        "mov rax, qword ptr [rip + {exit_status}]",
        "ret",
        kernel_data = const KERNEL_DATA_SELECTOR,
        saved_rsp = sym USER_KERNEL_RSP,
        exit_status = sym USER_EXIT_STATUS,
    );
}

pub fn record_user_exit(status: u64) {
    unsafe {
        core::ptr::addr_of_mut!(USER_EXIT_STATUS).write(status);
    }
}

pub fn user_exit_address() -> u64 {
    user_exit_trampoline as *const () as u64
}

#[cfg(test)]
mod tests {
    use super::TaskStateSegment;

    #[test]
    fn task_state_segment_matches_the_x86_64_layout() {
        assert_eq!(core::mem::offset_of!(TaskStateSegment, rsp), 4);
        assert_eq!(core::mem::offset_of!(TaskStateSegment, ist), 36);
        assert_eq!(core::mem::offset_of!(TaskStateSegment, iomap_base), 102);
        assert_eq!(core::mem::size_of::<TaskStateSegment>(), 104);
    }
}
