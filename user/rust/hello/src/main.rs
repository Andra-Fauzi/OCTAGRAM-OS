#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

use alloc::vec::Vec;
use core::alloc::Layout;
use core::ffi::CStr;
use core::panic::PanicInfo;
use octagram_runtime::{
    close, clock_gettime, dup3, exit, futex, getpid, getrandom, mmap, mprotect,
    munmap, open, pipe2, read, stat_path, thread_self, write, write_all,
    CLOCK_MONOTONIC, MAP_ANONYMOUS, MAP_PRIVATE, O_RDONLY, PROT_READ, PROT_WRITE, STDOUT,
};

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

    // Exercise the runtime's clock, process identity, and entropy syscalls.
    let now = match clock_gettime(CLOCK_MONOTONIC) {
        Ok(value) if value.tv_nsec >= 0 && value.tv_nsec < 1_000_000_000 => value,
        _ => return 11,
    };
    if now.tv_sec < 0 || getpid().map(|pid| pid == 0).unwrap_or(true) {
        return 12;
    }
    let mut entropy = [0u8; 64];
    if getrandom(&mut entropy, 0) != Ok(entropy.len()) {
        return 13;
    }
    if write_all(STDOUT, b"Rust runtime clock/random/pid: PASS\n").is_err() {
        return 14;
    }

    // Exercise the typed VM, descriptor, futex, and thread wrappers used by
    // a future Rust std backend.
    let mapping = match unsafe {
        mmap(None, 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0)
    } {
        Ok(mapping) => mapping,
        Err(_) => return 15,
    };
    unsafe {
        mapping.as_ptr().write_volatile(0x5Au8);
    }
    if unsafe { mapping.as_ptr().read_volatile() } != 0x5A
        || unsafe { mprotect(mapping, 4096, PROT_READ | PROT_WRITE) }.is_err()
        || unsafe { munmap(mapping, 4096) }.is_err()
    {
        return 16;
    }

    let descriptors = match pipe2(0) {
        Ok(descriptors) if descriptors[0] >= 0 && descriptors[1] >= 0 => descriptors,
        _ => return 17,
    };
    let pipe_message = *b"rust-pipe";
    let mut pipe_buffer = [0u8; 9];
    let pipe_result = write(descriptors[1] as u64, &pipe_message)
        .and_then(|count| {
            if count != pipe_message.len() { return Err(-5); }
            read(descriptors[0] as u64, &mut pipe_buffer)
        });
    let close_read = close(descriptors[0] as u64);
    let close_write = close(descriptors[1] as u64);
    if pipe_result != Ok(pipe_buffer.len())
        || pipe_buffer != pipe_message
        || close_read.is_err()
        || close_write.is_err()
    {
        return 18;
    }

    let mut futex_word = 0u32;
    if unsafe { futex(&futex_word, 1, 1, 0, 0, 0) }.is_err()
        || thread_self().map(|tid| tid == 0).unwrap_or(true)
    {
        return 19;
    }

    // dup3 is also part of the process plumbing needed by Cargo's subprocesses.
    let duplicate = match dup3(STDOUT, 10, 0) {
        Ok(fd) if fd == 10 => fd,
        _ => return 20,
    };
    let duplicate_result = write_all(duplicate, b"");
    let duplicate_close = close(duplicate);
    if duplicate_result.is_err() || duplicate_close.is_err() {
        return 21;
    }
    if write_all(STDOUT, b"Rust runtime VM/pipe/futex/thread: PASS\n").is_err() {
        return 22;
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
