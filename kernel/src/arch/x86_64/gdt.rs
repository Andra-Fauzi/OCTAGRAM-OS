//! GDT, TSS, and the ring-3 entry/return path for x86_64.

use core::arch::{asm, naked_asm};

pub const KERNEL_CODE_SELECTOR: u16 = 0x08;
pub const KERNEL_DATA_SELECTOR: u16 = 0x10;
pub const USER_DATA_SELECTOR: u16 = 0x1B;
pub const USER_CODE_SELECTOR: u16 = 0x23;
const TSS_SELECTOR: u16 = 0x28;
const USER_KERNEL_STACK_SIZE: usize = 64 * 1024;

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

unsafe fn set_ring0_stack(stack_top: u64) {
    let tss = core::ptr::addr_of_mut!(TSS).cast::<u8>();
    let rsp0 = unsafe {
        tss.add(core::mem::offset_of!(TaskStateSegment, rsp))
            .cast::<u64>()
    };
    unsafe {
        rsp0.write_unaligned(stack_top);
    }
}

/// Update the kernel stack used for the next privilege transition from ring 3.
///
/// # Safety
/// `stack_top` must be the top of a mapped, writable kernel stack.
pub unsafe fn set_ring0_stack_top(stack_top: u64) {
    unsafe { set_ring0_stack(stack_top) }
}

#[repr(align(16))]
struct KernelStack([u8; USER_KERNEL_STACK_SIZE]);

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
        "iretq",
        saved_rsp = sym USER_KERNEL_RSP,
        user_data = const USER_DATA_SELECTOR,
        user_code = const USER_CODE_SELECTOR,
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
