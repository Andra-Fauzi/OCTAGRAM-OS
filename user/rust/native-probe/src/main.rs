//! Probe for M4.3: an object file produced by rustc that OCTAGRAM's own
//! linker must be able to link.
//!
//! This crate is compiled with `--emit=obj` into a single x86_64 `ET_REL`
//! object for `x86_64-unknown-octagram`. It deliberately depends on nothing
//! beyond `core` and issues its syscalls with inline assembly, so the object
//! can be handed to the native `ld` without any C runtime or `crt0.o`.
//! The guest test links it with `ld <object> -o <elf>`, runs the result, and
//! checks for the marker below.

#![no_std]
#![no_main]

use core::panic::PanicInfo;

const MARKER: &[u8] = b"RUST-NATIVE-LD-PASS\n";

/// OCTAGRAM syscall ABI: `int 0x80`, number in RAX, arguments in
/// RDI/RSI/RDX. See docs/syscall-abi.md.
unsafe fn syscall3(number: u64, arg1: u64, arg2: u64, arg3: u64) -> i64 {
    let result: i64;
    unsafe {
        core::arch::asm!(
            "int 0x80",
            inlateout("rax") number as i64 => result,
            in("rdi") arg1,
            in("rsi") arg2,
            in("rdx") arg3,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
    }
    result
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    unsafe {
        // 1 = write, fd 1 = stdout.
        syscall3(1, 1, MARKER.as_ptr() as u64, MARKER.len() as u64);
        // 0 = exit with status 0.
        syscall3(0, 0, 0, 0);
    }
    loop {
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    unsafe {
        syscall3(0, 101, 0, 0);
    }
    loop {
        core::hint::spin_loop();
    }
}
