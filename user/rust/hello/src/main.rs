#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

use alloc::vec::Vec;
use core::alloc::Layout;
use core::ffi::CStr;
use core::panic::PanicInfo;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicU64, Ordering};
use octagram_runtime::{
    close, closedir, clock_getres, clock_gettime, dup3, exit, fstat, fstatat, futex, getpid,
    getrandom, lseek, mmap, mprotect, munmap, nanosleep, openat, opendir, pipe2, read, readdir,
    stat_path, thread_create,
    thread_exit, thread_join, thread_self, write, write_all,
    AT_FDCWD, CLOCK_MONOTONIC, MAP_ANONYMOUS, MAP_PRIVATE, O_RDONLY, PROT_READ, PROT_WRITE, SEEK_SET, STDOUT,
};

static RUST_THREAD_RESULT: AtomicU64 = AtomicU64::new(0);

extern "C" fn rust_thread_entry(_argument: *mut core::ffi::c_void) -> ! {
    RUST_THREAD_RESULT.store(0x0C7A_6A4A, Ordering::SeqCst);
    thread_exit(0)
}

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
    let relative_metadata = match fstatat(AT_FDCWD, path, 0) {
        Ok(metadata) if metadata.size > 0 => metadata,
        _ => return 4,
    };
    let fd = match openat(AT_FDCWD, path, O_RDONLY, 0) {
        Ok(fd) => fd,
        Err(_) => return 5,
    };
    let descriptor_metadata = match fstat(fd) {
        Ok(metadata) if metadata.size > 0 => metadata,
        _ => return 6,
    };
    if descriptor_metadata.size != metadata.size || relative_metadata.size != metadata.size {
        return 7;
    }
    if lseek(fd, 0, SEEK_SET) != Ok(0) {
        return 8;
    }
    let mut sample = [0u8; 32];
    let read_result = read(fd, &mut sample);
    let close_result = close(fd);
    match (read_result, close_result) {
        (Ok(count), Ok(())) if count > 0 && metadata.size >= count as u64 => {}
        _ => return 9,
    }
    // Directory iteration is required by std::fs::read_dir and Cargo discovery.
    let directory_path = match CStr::from_bytes_with_nul(b"/share/octagram\0") {
        Ok(path) => path,
        Err(_) => return 29,
    };
    let directory_fd = match opendir(directory_path) {
        Ok(fd) => fd,
        Err(_) => return 30,
    };
    let mut found_message = false;
    let mut reached_eof = false;
    for _ in 0..128 {
        match readdir(directory_fd) {
            Ok(Some(entry)) => {
                if entry.d_reclen as usize != core::mem::size_of::<octagram_runtime::Dirent>() {
                    let _ = closedir(directory_fd);
                    return 33;
                }
                let name_length = match entry.d_name.iter().position(|&byte| byte == 0) {
                    Some(length) => length,
                    None => {
                        let _ = closedir(directory_fd);
                        return 34;
                    }
                };
                let name_bytes = unsafe {
                    core::slice::from_raw_parts(entry.d_name.as_ptr().cast::<u8>(), name_length)
                };
                if name_bytes == b"message.c" { found_message = true; }
            }
            Ok(None) => { reached_eof = true; break; }
            Err(_) => break,
        }
    }
    if closedir(directory_fd).is_err() || !found_message || !reached_eof {
        return 31;
    }
    if write_all(STDOUT, b"Rust runtime directory iteration: PASS\n").is_err() {
        return 32;
    }

    if write_all(STDOUT, b"Rust runtime file/stat: PASS\n").is_err() {
        return 6;
    }

    // Exercise the runtime's clock, process identity, and entropy syscalls.
    let resolution = match clock_getres(CLOCK_MONOTONIC) {
        Ok(value) if value.tv_sec >= 0 && value.tv_nsec >= 0 => value,
        _ => return 23,
    };
    let _ = resolution;
    if nanosleep(&octagram_runtime::Timespec { tv_sec: 0, tv_nsec: 1_000_000 }).is_err() {
        return 24;
    }
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
    // Create a real Rust thread with an owned stack mapping, then join it.
    RUST_THREAD_RESULT.store(0, Ordering::SeqCst);
    let stack_length = 64 * 1024usize;
    let stack_mapping = match unsafe {
        mmap(None, stack_length, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0)
    } {
        Ok(mapping) => mapping,
        Err(_) => return 25,
    };
    let stack_top = match NonNull::new(unsafe { stack_mapping.as_ptr().add(stack_length) }) {
        Some(pointer) => pointer,
        None => return 26,
    };
    let thread_id = match unsafe {
        thread_create(rust_thread_entry, core::ptr::null_mut(), stack_top, None, stack_mapping, stack_length)
    } {
        Ok(thread_id) => thread_id,
        Err(_) => {
            let _ = unsafe { munmap(stack_mapping, stack_length) };
            return 27;
        }
    };
    if thread_join(thread_id).is_err()
        || RUST_THREAD_RESULT.load(Ordering::SeqCst) != 0x0C7A_6A4A
    {
        return 28;
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
