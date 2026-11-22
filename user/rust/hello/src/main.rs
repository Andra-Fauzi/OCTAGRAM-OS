#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

use alloc::vec::Vec;
use core::alloc::Layout;
use core::ffi::CStr;
use core::panic::PanicInfo;
use octagram_runtime::{close, exit, open, read, stat_path, write_all, O_RDONLY, STDOUT};

#[cfg(target_os = "octagram")]
const TARGET_MARKER: &[u8] = b"Rust custom target x86_64-unknown-octagram runs in OCTAGRAM!\n";
#[cfg(not(target_os = "octagram"))]
const TARGET_MARKER: &[u8] = b"Rust no_std + alloc runs in OCTAGRAM!\n";

#[no_mangle]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let mut buffer = Vec::with_capacity(2 * 1024 * 1024);
    buffer.resize(2 * 1024 * 1024, b'R');
    let index = (_argc as usize) % buffer.len();
    if write_all(STDOUT, &buffer[index..index + 1]).is_err() {
        return 1;
    }
    drop(buffer);

    // Exercise one mmap per allocation and munmap on every non-zero drop.
    let mut allocations = Vec::new();
    for round in 0..32usize {
        let mut block = Vec::with_capacity(4096 + round * 16);
        block.resize(4096 + round * 16, round as u8);
        if block.first() != Some(&(round as u8)) {
            return 7;
        }
        allocations.push(block);
    }
    drop(allocations);

    // Check over-aligned allocations and reuse after a full release cycle.
    let mut aligned = Vec::<u128>::with_capacity(256);
    aligned.resize(256, 0xA55A_u128);
    if (aligned.as_ptr() as usize) % core::mem::align_of::<u128>() != 0
        || aligned.iter().any(|value| *value != 0xA55A_u128)
    {
        return 8;
    }
    drop(aligned);

    let mut final_buffer = Vec::with_capacity(8192);
    final_buffer.resize(8192, b'Z');
    if final_buffer.iter().any(|byte| *byte != b'Z') {
        return 9;
    }
    drop(final_buffer);

    if write_all(STDOUT, TARGET_MARKER).is_err() {
        return 1;
    }
    if write_all(STDOUT, b"Rust allocator mmap/munmap cycle: PASS\n").is_err() {
        return 10;
    }

    // Exercise the Rust runtime's file/stat syscall ABI against a seeded VFS
    // source file, not just stdout and the allocator.
    let path = match CStr::from_bytes_with_nul(b"/share/octagram/message.c\0") {
        Ok(path) => path,
        Err(_) => return 2,
    };
    let metadata = match stat_path(path) {
        Ok(metadata) if metadata.size > 0 => metadata,
        _ => return 3,
    };
    let fd = match open(path, O_RDONLY) {
        Ok(fd) => fd,
        Err(_) => return 4,
    };
    let mut sample = [0u8; 32];
    let read_result = read(fd, &mut sample);
    let close_result = close(fd);
    match (read_result, close_result) {
        (Ok(count), Ok(())) if count > 0 && metadata.size >= count as u64 => {}
        _ => return 5,
    }
    if write_all(STDOUT, b"Rust runtime file/stat: PASS\n").is_err() {
        return 6;
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
