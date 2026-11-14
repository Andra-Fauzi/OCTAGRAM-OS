#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

use alloc::vec::Vec;
use core::alloc::Layout;
use core::panic::PanicInfo;
use octagram_runtime::{exit, write_all, STDOUT};

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let mut buffer = Vec::with_capacity(2 * 1024 * 1024);
    buffer.resize(2 * 1024 * 1024, b'R');
    let index = (_argc as usize) % buffer.len();
    if write_all(STDOUT, &buffer[index..index + 1]).is_err() {
        return 1;
    }
    if write_all(STDOUT, b"Rust no_std + alloc runs in OCTAGRAM!\n").is_err() {
        return 1;
    }
    0
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    let _ = write_all(STDOUT, b"Rust program panicked\n");
    exit(101)
}

#[alloc_error_handler]
fn allocation_error(_layout: Layout) -> ! {
    let _ = write_all(STDOUT, b"Rust allocation failed\n");
    exit(127)
}
