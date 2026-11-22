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
    close, clock_getres, clock_gettime, dup3, exit, fork, fstatat, futex, getpid,
    getrandom, mmap, mprotect, munmap, nanosleep, pipe2, pipe2_owned, read,
    stat_path, thread_create, unlinkat, Child, Command, Directory, File,
    thread_exit, thread_join, thread_self, write, write_all, Stdio,
    AT_FDCWD, PathBuf, CLOCK_MONOTONIC, MAP_ANONYMOUS, MAP_PRIVATE, O_RDONLY, O_RDWR, O_CREAT, O_TRUNC, O_CLOEXEC, PROT_READ, PROT_WRITE, SEEK_SET, STDOUT,
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
    let file = match File::openat(AT_FDCWD, path, O_RDONLY, 0) {
        Ok(file) => file,
        Err(_) => return 5,
    };
    let descriptor_metadata = match file.metadata() {
        Ok(metadata) if metadata.size > 0 => metadata,
        _ => return 6,
    };
    if descriptor_metadata.size != metadata.size || relative_metadata.size != metadata.size {
        return 7;
    }
    // Exercise byte-preserving path construction and conversion at the syscall boundary.
    let mut runtime_path = PathBuf::from_bytes(b"/share/octagram");
    runtime_path.push(b"message.c");
    let nul_terminated_path = match runtime_path.try_to_cstring_bytes() {
        Ok(path) => path,
        Err(_) => return 39,
    };
    let runtime_c_path = match CStr::from_bytes_with_nul(&nul_terminated_path) {
        Ok(path) => path,
        Err(_) => return 40,
    };
    if stat_path(runtime_c_path).map(|value| value.size != metadata.size).unwrap_or(true) {
        return 41;
    }
    if file.seek(0, SEEK_SET) != Ok(0) {
        return 8;
    }
    let mut sample = [0u8; 32];
    if file.read_exact(&mut sample).is_err() || metadata.size < sample.len() as u64 {
        return 9;
    }
    let mut positional_sample = [0u8; 8];
    if file.read_exact_at(&mut positional_sample, 4).is_err() {
        return 11;
    }
    if file.seek(0, SEEK_SET) != Ok(0) {
        return 12;
    }
    let mut position_check = [0u8; 8];
    if file.read_exact(&mut position_check).is_err() || position_check != sample[..8] {
        return 13;
    }
    if file.close().is_err() {
        return 10;
    }

    // Verify writable-file operations required by std::fs::File:
    // create, partial overwrite via truncate, fsync/fdatasync, metadata, unlink.
    let scratch_path = match CStr::from_bytes_with_nul(b"/tmp/rust-runtime-sync-test.tmp\\0") {
        Ok(path) => path,
        Err(_) => return 35,
    };
    let scratch = match File::openat(AT_FDCWD, scratch_path, O_RDWR | O_CREAT | O_TRUNC, 0o600) {
        Ok(file) => file,
        Err(_) => return 36,
    };
    if scratch.write_all(b"octagram-file-api").is_err()
        || scratch.set_len(4).is_err()
        || scratch.sync_data().is_err()
        || scratch.sync_all().is_err()
        || scratch.metadata().map(|value| value.size != 4).unwrap_or(true)
        || scratch.close().is_err()
        || unlinkat(AT_FDCWD, scratch_path, 0).is_err()
    {
        return 37;
    }
    if write_all(STDOUT, b"Rust runtime file mutation/sync: PASS\\n").is_err() {
        return 38;
    }

    // Directory iteration is required by std::fs::read_dir and Cargo discovery.
    let directory_path = match CStr::from_bytes_with_nul(b"/share/octagram\0") {
        Ok(path) => path,
        Err(_) => return 29,
    };
    let directory = match Directory::open(directory_path) {
        Ok(directory) => directory,
        Err(_) => return 30,
    };
    let mut found_message = false;
    let mut found_entry = None;
    let mut directory_error = false;
    for entry_result in directory {
        let entry = match entry_result {
            Ok(entry) => entry,
            Err(_) => {
                directory_error = true;
                break;
            }
        };
        if entry.dirent().d_reclen as usize != core::mem::size_of::<octagram_runtime::Dirent>() {
            return 33;
        }
        let name_bytes = match entry.name_bytes() {
            Some(name) => name,
            None => return 34,
        };
        if name_bytes == b"message.c" {
            let file_name = match entry.file_name() {
                Some(name) if name.as_bytes() == b"message.c" => name,
                _ => return 42,
            };
            let entry_path = match entry.path() {
                Some(path) if path.as_bytes() == b"/share/octagram/message.c" => path,
                _ => return 43,
            };
            let _ = (file_name, entry_path);
            found_message = true;
            found_entry = Some(entry);
        }
    }
    if directory_error || !found_message {
        return 31;
    }
    // The iterator's parent descriptor is closed at EOF, but each DirEntry
    // owns a duplicate so metadata remains available after iteration.
    let found_entry = match found_entry {
        Some(entry) => entry,
        None => return 35,
    };
    let entry_metadata = match found_entry.metadata() {
        Ok(metadata) => metadata,
        Err(_) => return 36,
    };
    let entry_type = match found_entry.file_type() {
        Ok(file_type) if file_type.is_file() => file_type,
        _ => return 37,
    };
    let _ = entry_type;
    if entry_metadata.size == 0 {
        return 38;
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

    // Exercise fork + nonblocking/blocking waitpid and raw exit-status delivery.
    let child_pid = match fork() {
        Ok(0) => exit(42),
        Ok(pid) if pid > 0 => pid,
        _ => return 39,
    };
    let mut child = Child::from_pid(child_pid);
    if child.try_wait().is_err() {
        return 40;
    }
    let child_status = match child.wait() {
        Ok(status) => status,
        Err(_) => return 41,
    };
    if child_status != 42 || child.id().is_some() || child.wait().is_ok() {
        return 42;
    }
    if write_all(STDOUT, b"Rust runtime child process/waitpid: PASS\\n").is_err() {
        return 43;
    }

    // Exercise Command's close-on-exec handshake with a successful shell exec.
    let shell_path = CStr::from_bytes_with_nul(b"/bin/sh\0").unwrap();
    let shell_arg0 = CStr::from_bytes_with_nul(b"sh\0").unwrap();
    let shell_arg1 = CStr::from_bytes_with_nul(b"-c\0").unwrap();
    let shell_arg2 = CStr::from_bytes_with_nul(b"exit 23\0").unwrap();
    let shell_argv = [
        shell_arg0.as_ptr(),
        shell_arg1.as_ptr(),
        shell_arg2.as_ptr(),
        core::ptr::null(),
    ];
    let empty_env = [core::ptr::null()];
    let mut command_child = match Command::new(shell_path, &shell_argv, &empty_env).spawn() {
        Ok(child) => child,
        Err(_) => return 44,
    };
    if command_child.wait() != Ok(23) {
        return 45;
    }

    // A non-ELF source file must return execve's error, not a false spawn success.
    let invalid_executable = match CStr::from_bytes_with_nul(b"/share/octagram/message.c\0") {
        Ok(path) => path,
        Err(_) => return 46,
    };
    let invalid_arg = CStr::from_bytes_with_nul(b"message.c\0").unwrap();
    let invalid_argv = [invalid_arg.as_ptr(), core::ptr::null()];
    match Command::new(invalid_executable, &invalid_argv, &empty_env).spawn() {
        Err(error) if error < 0 => {}
        _ => return 47,
    }
    if write_all(STDOUT, b"Rust runtime Command exec handshake: PASS\n").is_err() {
        return 48;
    }

    // Capture child stdout through an owned close-on-exec pipe.
    let [output_read, output_write] = match pipe2_owned(O_CLOEXEC) {
        Ok(pipe) => pipe,
        Err(_) => return 49,
    };
    let capture_arg2 = CStr::from_bytes_with_nul(b"printf OCTAGRAM-PIPE-PASS; printf OCTAGRAM-STDERR-PASS >&2\0").unwrap();
    let capture_argv = [
        shell_arg0.as_ptr(),
        shell_arg1.as_ptr(),
        capture_arg2.as_ptr(),
        core::ptr::null(),
    ];
    let mut capture_child = match Command::new(shell_path, &capture_argv, &empty_env)
        .stdout(Stdio::from_fd(output_write.as_raw_fd()))
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return 50,
    };
    drop(output_write);
    let mut captured = [0u8; 64];
    let mut captured_len = 0usize;
    loop {
        if captured_len == captured.len() { return 51; }
        match output_read.read(&mut captured[captured_len..]) {
            Ok(0) => break,
            Ok(count) => captured_len += count,
            Err(_) => return 52,
        }
    }
    if capture_child.wait() != Ok(0)
        || &captured[..captured_len] != b"OCTAGRAM-PIPE-PASS"
    {
        return 53;
    }
    if write_all(STDOUT, b"Rust runtime Command stdout capture: PASS\n").is_err() {
        return 54;
    }

    // Exercise the higher-level Command::output convenience path.
    let output = match Command::new(shell_path, &capture_argv, &empty_env).output() {
        Ok(output) => output,
        Err(_) => return 55,
    };
    if !output.success()
        || output.exit_status().code() != Some(0)
        || output.stdout.as_slice() != b"OCTAGRAM-PIPE-PASS"
        || output.stderr.as_slice() != b"OCTAGRAM-STDERR-PASS"
    {
        return 56;
    }
    if write_all(STDOUT, b"Rust runtime Command::output: PASS\n").is_err() {
        return 57;
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
