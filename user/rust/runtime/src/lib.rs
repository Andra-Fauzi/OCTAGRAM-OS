#![no_std]

use core::alloc::Layout;
#[cfg(not(test))]
use core::alloc::GlobalAlloc;
use core::ffi::{c_char, c_void, CStr};
use core::ptr;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicU16, Ordering};

pub const STDIN: u64 = 0;
pub const STDOUT: u64 = 1;
pub const STDERR: u64 = 2;

pub const O_RDONLY: u64 = 0;
pub const O_WRONLY: u64 = 1;
pub const O_RDWR: u64 = 2;
pub const O_CREAT: u64 = 1 << 8;
pub const O_TRUNC: u64 = 1 << 9;
pub const O_APPEND: u64 = 1 << 10;


/// Freestanding C memory primitives required by Rust core/alloc when linking
/// without a hosted libc. OCTAGRAM currently targets x86_64.
#[no_mangle]
pub unsafe extern "C" fn memset(destination: *mut c_void, value: i32, length: usize) -> *mut c_void {
    let mut cursor = destination.cast::<u8>();
    let mut count = length;
    let byte = value as u8;
    // SAFETY: The caller guarantees a writable range of length bytes.
    // CLD restores the ABI-required direction flag before returning.
    unsafe {
        core::arch::asm!(
            "cld",
            "rep stosb",
            inout("rdi") cursor => _,
            inout("rcx") count => _,
            in("al") byte,
            options(nostack)
        );
    }
    destination
}

#[no_mangle]
pub unsafe extern "C" fn memcpy(
    destination: *mut c_void,
    source: *const c_void,
    length: usize,
) -> *mut c_void {
    let mut destination_cursor = destination.cast::<u8>();
    let mut source_cursor = source.cast::<u8>();
    let mut count = length;
    // SAFETY: The caller guarantees valid, non-overlapping source/destination ranges.
    unsafe {
        core::arch::asm!(
            "cld",
            "rep movsb",
            inout("rdi") destination_cursor => _,
            inout("rsi") source_cursor => _,
            inout("rcx") count => _,
            options(nostack)
        );
    }
    destination
}

#[no_mangle]
pub unsafe extern "C" fn memmove(
    destination: *mut c_void,
    source: *const c_void,
    length: usize,
) -> *mut c_void {
    if length == 0 || destination as usize == source as usize {
        return destination;
    }

    let destination_address = destination as usize;
    let source_address = source as usize;
    if destination_address < source_address || destination_address - source_address >= length {
        // SAFETY: The ranges do not overlap in this branch.
        return unsafe { memcpy(destination, source, length) };
    }

    // Do not set DF=1 here: user code may be preempted during memmove, and
    // the interrupt handler/kernel ABI must always see the normal DF=0 state.
    let destination = destination.cast::<u8>();
    let source = source.cast::<u8>();
    for offset in (0..length).rev() {
        let byte = unsafe { source.add(offset).read_volatile() };
        unsafe { destination.add(offset).write_volatile(byte) };
    }
    destination.cast::<c_void>()
}

#[no_mangle]
pub unsafe extern "C" fn memcmp(
    left: *const c_void,
    right: *const c_void,
    length: usize,
) -> i32 {
    let left = left.cast::<u8>();
    let right = right.cast::<u8>();
    for offset in 0..length {
        let left_byte = unsafe { left.add(offset).read_volatile() };
        let right_byte = unsafe { right.add(offset).read_volatile() };
        if left_byte != right_byte {
            return i32::from(left_byte) - i32::from(right_byte);
        }
    }
    0
}

/// Freestanding C string length primitive required by compiler-generated code.
/// The caller must pass a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn strlen(string: *const c_char) -> usize {
    let mut length = 0usize;
    // SAFETY: The caller guarantees that string points to a readable
    // NUL-terminated byte sequence.
    unsafe {
        while string.add(length).read() != 0 {
            length += 1;
        }
    }
    length
}

#[no_mangle]
pub static mut environ: *mut *mut c_char = ptr::null_mut();

/// Look up an environment variable in an envp array.
///
/// Returns a pointer to the first byte after '=' or null when absent.
/// The caller must provide a valid, NUL-terminated envp array and name bytes
/// without NUL or '='. The scan is bounded to avoid walking a corrupt envp
/// indefinitely.
unsafe fn find_env_value(
    environment: *const *const c_char,
    name: &[u8],
) -> *const c_char {
    if environment.is_null() || name.is_empty() || name.contains(&b'=') {
        return ptr::null();
    }

    for index in 0..4096usize {
        let entry = unsafe { *environment.add(index) };
        if entry.is_null() {
            return ptr::null();
        }

        let value = unsafe { CStr::from_ptr(entry) }.to_bytes();
        if value.len() > name.len()
            && value[name.len()] == b'='
            && &value[..name.len()] == name
        {
            return unsafe { entry.add(name.len() + 1) };
        }
    }
    ptr::null()
}

/// Return an environment variable from the initial process environment.
///
/// OCTAGRAM startup initializes the exported environ pointer from the
/// kernel-provided initial stack before calling main. The returned string
/// points into that process-lifetime initial stack.
///
/// This runtime currently provides lookup only; setenv/unsetenv require an
/// owned environment table and are intentionally not emulated here.
pub fn getenv(name: &CStr) -> Option<&'static CStr> {
    let environment = unsafe { ptr::addr_of!(environ).read() };
    let value = unsafe { find_env_value(environment.cast_const(), name.to_bytes()) };
    if value.is_null() {
        None
    } else {
        Some(unsafe { CStr::from_ptr(value) })
    }
}

pub const PROT_READ: u64 = 1;
pub const PROT_WRITE: u64 = 2;
pub const PROT_EXEC: u64 = 4;

const MAX_SYSCALL_IO: usize = 4096;
const ENOMEM: i64 = -12;

unsafe fn syscall1(number: u64, arg1: u64) -> i64 {
    let result: i64;
    unsafe {
        core::arch::asm!(
            "int 0x80",
            inlateout("rax") number as i64 => result,
            in("rdi") arg1,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
    }
    result
}

unsafe fn syscall0(number: u64) -> i64 {
    let result: i64;
    unsafe {
        core::arch::asm!(
            "int 0x80",
            inlateout("rax") number as i64 => result,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
    }
    result
}

unsafe fn syscall2(number: u64, arg1: u64, arg2: u64) -> i64 {
    let result: i64;
    unsafe {
        core::arch::asm!(
            "int 0x80",
            inlateout("rax") number as i64 => result,
            in("rdi") arg1,
            in("rsi") arg2,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
    }
    result
}

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

unsafe fn syscall6(
    number: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
    arg6: u64,
) -> i64 {
    let result: i64;
    unsafe {
        core::arch::asm!(
            "int 0x80",
            inlateout("rax") number as i64 => result,
            in("rdi") arg1,
            in("rsi") arg2,
            in("rdx") arg3,
            in("r10") arg4,
            in("r8") arg5,
            in("r9") arg6,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack)
        );
    }
    result
}

fn checked_result(result: i64) -> Result<usize, i64> {
    if result < 0 {
        Err(result)
    } else {
        usize::try_from(result).map_err(|_| ENOMEM)
    }
}

pub fn write(fd: u64, bytes: &[u8]) -> Result<usize, i64> {
    let result = unsafe { syscall3(1, fd, bytes.as_ptr() as usize as u64, bytes.len() as u64) };
    checked_result(result)
}

pub fn write_all(fd: u64, mut bytes: &[u8]) -> Result<(), i64> {
    while !bytes.is_empty() {
        let chunk = bytes.len().min(MAX_SYSCALL_IO);
        let written = write(fd, &bytes[..chunk])?;
        if written == 0 || written > chunk {
            return Err(-5);
        }
        bytes = &bytes[written..];
    }
    Ok(())
}

pub fn read(fd: u64, buffer: &mut [u8]) -> Result<usize, i64> {
    let result = unsafe {
        syscall3(
            2,
            fd,
            buffer.as_mut_ptr() as usize as u64,
            buffer.len() as u64,
        )
    };
    checked_result(result)
}

pub fn open(path: &CStr, flags: u64) -> Result<u64, i64> {
    let mode = if flags & O_CREAT != 0 { 0o666 } else { 0 };
    let result = unsafe { syscall3(3, path.as_ptr() as usize as u64, flags, mode) };
    if result < 0 {
        Err(result)
    } else {
        Ok(result as u64)
    }
}

/// Compact metadata layout returned by OCTAGRAM's stat syscall.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileStat {
    pub size: u64,
    pub mode: u32,
    pub modified_seconds: u32,
}

/// Read file metadata without opening a descriptor.
pub fn stat_path(path: &CStr) -> Result<FileStat, i64> {
    let mut metadata = FileStat::default();
    let result = unsafe {
        syscall2(
            6,
            path.as_ptr() as usize as u64,
            &mut metadata as *mut FileStat as usize as u64,
        )
    };
    if result < 0 {
        Err(result)
    } else {
        Ok(metadata)
    }
}

pub fn getcwd(buffer: &mut [u8]) -> Result<usize, i64> {
    let result = unsafe { syscall2(14, buffer.as_mut_ptr() as usize as u64, buffer.len() as u64) };
    checked_result(result)
}

/// Anonymous/file-backed mapping flags used by the OCTAGRAM VM ABI.
pub const MAP_SHARED: u64 = 1;
pub const MAP_PRIVATE: u64 = 2;
pub const MAP_ANONYMOUS: u64 = 0x20;

/// Map a user virtual-memory range. The returned pointer is owned by the caller
/// and must be released with `munmap`. A null address requests kernel placement.
///
/// # Safety
/// The caller must ensure the mapping is used according to its protection flags
/// and that no references outlive a successful `munmap`.
pub unsafe fn mmap(
    address: Option<NonNull<u8>>,
    length: usize,
    protection: u64,
    flags: u64,
    fd: i64,
    offset: u64,
) -> Result<NonNull<u8>, i64> {
    if length == 0 {
        return Err(-22);
    }
    let requested = address.map_or(0, |value| value.as_ptr() as usize as u64);
    let result = unsafe { syscall6(16, requested, length as u64, protection, flags, fd as u64, offset) };
    let mapped = checked_result(result)?;
    NonNull::new(mapped as *mut u8).ok_or(ENOMEM)
}

/// Release a mapping created by `mmap`.
///
/// # Safety
/// `address..address+length` must be a live mapping owned by the caller.
pub unsafe fn munmap(address: NonNull<u8>, length: usize) -> Result<(), i64> {
    if length == 0 {
        return Err(-22);
    }
    let result = unsafe { syscall2(17, address.as_ptr() as usize as u64, length as u64) };
    if result < 0 { Err(result) } else { Ok(()) }
}

/// Change page protections on a mapped range.
///
/// # Safety
/// The range must be mapped and page-aligned as required by the kernel.
pub unsafe fn mprotect(address: NonNull<u8>, length: usize, protection: u64) -> Result<(), i64> {
    if length == 0 {
        return Err(-22);
    }
    let result = unsafe { syscall3(52, address.as_ptr() as usize as u64, length as u64, protection) };
    if result < 0 { Err(result) } else { Ok(()) }
}

/// Create a pipe. OCTAGRAM currently accepts the Linux-compatible pipe2 ABI.
pub fn pipe2(flags: u64) -> Result<[i32; 2], i64> {
    let mut descriptors = [-1i32; 2];
    let result = unsafe { syscall2(98, descriptors.as_mut_ptr() as usize as u64, flags) };
    if result < 0 { Err(result) } else { Ok(descriptors) }
}

/// Duplicate a descriptor onto a specific descriptor number.
pub fn dup3(source: u64, target: u64, flags: u64) -> Result<u64, i64> {
    let result = unsafe { syscall3(99, source, target, flags) };
    if result < 0 { Err(result) } else { Ok(result as u64) }
}

/// Set the current thread's FS-base thread pointer.
pub fn set_thread_pointer(pointer: usize) -> Result<(), i64> {
    let result = unsafe { syscall1(59, pointer as u64) };
    if result < 0 { Err(result) } else { Ok(()) }
}

/// Read the current thread's FS-base thread pointer.
pub fn get_thread_pointer() -> Result<usize, i64> {
    let result = unsafe { syscall0(60) };
    checked_result(result)
}

/// Return the current kernel thread identifier.
pub fn thread_self() -> Result<u64, i64> {
    let result = unsafe { syscall0(64) };
    if result < 0 { Err(result) } else { Ok(result as u64) }
}

/// Wait for a kernel thread to finish.
pub fn thread_join(thread_id: u64) -> Result<(), i64> {
    let result = unsafe { syscall2(63, thread_id, 0) };
    if result < 0 { Err(result) } else { Ok(()) }
}

/// Wait/wake a 32-bit futex word. Timeout is an optional pointer to an ABI
/// Timespec; operation-specific flags are passed unchanged to the kernel.
///
/// # Safety
/// The futex word and optional timeout must remain valid for the syscall.
pub unsafe fn futex(
    word: *const u32,
    operation: u64,
    value: u64,
    timeout_or_requeue: u64,
    second_word: u64,
    bitset: u64,
) -> Result<usize, i64> {
    if word.is_null() {
        return Err(-22);
    }
    let result = unsafe {
        syscall6(94, word as usize as u64, operation, value, timeout_or_requeue, second_word, bitset)
    };
    checked_result(result)
}

/// Futex operation numbers shared with the OCTAGRAM kernel ABI.
pub const FUTEX_WAIT: u64 = 0;
pub const FUTEX_WAKE: u64 = 1;
pub const FUTEX_REQUEUE: u64 = 3;
pub const FUTEX_WAIT_BITSET: u64 = 9;
pub const FUTEX_WAKE_BITSET: u64 = 10;
pub const FUTEX_BITSET_MATCH_ANY: u64 = u32::MAX as u64;

/// Create a user thread. The kernel takes ownership of the supplied mapping
/// and releases it when the thread is reaped.
///
/// # Safety
/// `entry` must be a valid non-returning userspace entry point; `stack_top`
/// must be aligned and lie inside `mapping`; `thread_pointer` must reference
/// a valid TLS/TCB block or be None. The mapping must not be released by caller.
pub unsafe fn thread_create(
    entry: extern "C" fn(*mut c_void) -> !,
    argument: *mut c_void,
    stack_top: NonNull<u8>,
    thread_pointer: Option<NonNull<u8>>,
    mapping: NonNull<u8>,
    mapping_length: usize,
) -> Result<u64, i64> {
    if mapping_length == 0 {
        return Err(-22);
    }
    let result = unsafe {
        syscall6(
            61,
            entry as usize as u64,
            argument as usize as u64,
            stack_top.as_ptr() as usize as u64,
            thread_pointer.map_or(0, |pointer| pointer.as_ptr() as usize as u64),
            mapping.as_ptr() as usize as u64,
            mapping_length as u64,
        )
    };
    if result < 0 { Err(result) } else { Ok(result as u64) }
}

/// Terminate the current thread and publish its result to a joining thread.
pub fn thread_exit(result: usize) -> ! {
    unsafe { syscall1(62, result as u64); }
    loop { core::hint::spin_loop(); }
}

/// POSIX-compatible clock identifiers exposed by the OCTAGRAM syscall ABI.
pub const CLOCK_REALTIME: u64 = 0;
pub const CLOCK_MONOTONIC: u64 = 1;

/// 64-bit seconds/nanoseconds pair matching OCTAGRAM's C timespec ABI.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Timespec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

/// Read a clock using syscall 50. The kernel reports failures as -errno.
pub fn clock_gettime(clock_id: u64) -> Result<Timespec, i64> {
    let mut value = Timespec::default();
    let result = unsafe {
        syscall2(
            50,
            clock_id,
            &mut value as *mut Timespec as usize as u64,
        )
    };
    if result < 0 {
        return Err(result);
    }
    if !(0..1_000_000_000).contains(&value.tv_nsec) || value.tv_sec < 0 {
        return Err(-22);
    }
    Ok(value)
}

/// Query the resolution of an OCTAGRAM clock (syscall 118).
pub fn clock_getres(clock_id: u64) -> Result<Timespec, i64> {
    let mut value = Timespec::default();
    let result = unsafe {
        syscall2(118, clock_id, &mut value as *mut Timespec as usize as u64)
    };
    if result < 0 { return Err(result); }
    if value.tv_sec < 0 || !(0..1_000_000_000).contains(&value.tv_nsec) {
        return Err(-22);
    }
    Ok(value)
}

/// Sleep for a relative interval (syscall 90).
pub fn nanosleep(request: &Timespec) -> Result<(), i64> {
    if request.tv_sec < 0 || !(0..1_000_000_000).contains(&request.tv_nsec) {
        return Err(-22);
    }
    let result = unsafe { syscall1(90, request as *const Timespec as usize as u64) };
    if result < 0 { Err(result) } else { Ok(()) }
}

/// Return the current process identifier (syscall 11).
pub fn getpid() -> Result<u64, i64> {
    let result = unsafe { syscall0(11) };
    if result < 0 {
        Err(result)
    } else {
        Ok(result as u64)
    }
}

/// Fill a buffer from the kernel random source (syscall 121).
/// OCTAGRAM currently limits each request to 256 bytes.
pub fn getrandom(buffer: &mut [u8], flags: u64) -> Result<usize, i64> {
    let mut completed = 0usize;
    while completed < buffer.len() {
        let chunk = (buffer.len() - completed).min(256);
        let result = unsafe {
            syscall3(
                121,
                buffer[completed..].as_mut_ptr() as usize as u64,
                chunk as u64,
                flags,
            )
        };
        if result < 0 {
            return if completed == 0 {
                Err(result)
            } else {
                Ok(completed)
            };
        }
        let count = usize::try_from(result).map_err(|_| -5)?;
        if count == 0 || count > chunk {
            return if completed == 0 { Err(-5) } else { Ok(completed) };
        }
        completed += count;
    }
    Ok(completed)
}

pub fn chdir(path: &CStr) -> Result<(), i64> {
    let result = unsafe { syscall1(13, path.as_ptr() as usize as u64) };
    if result < 0 {
        Err(result)
    } else {
        Ok(())
    }
}

pub fn chmod(path: &CStr, mode: u64) -> Result<(), i64> {
    let result = unsafe { syscall2(25, path.as_ptr() as usize as u64, mode) };
    if result < 0 {
        Err(result)
    } else {
        Ok(())
    }
}

pub fn close(fd: u64) -> Result<(), i64> {
    let result = unsafe { syscall1(4, fd) };
    if result < 0 {
        Err(result)
    } else {
        Ok(())
    }
}

pub fn yield_to_kernel() -> Result<(), i64> {
    let result = unsafe { syscall0(65) };
    if result < 0 {
        Err(result)
    } else {
        Ok(())
    }
}

pub fn udp_socket() -> Result<u64, i64> {
    let result = unsafe { syscall0(66) };
    if result < 0 {
        Err(result)
    } else {
        Ok(result as u64)
    }
}

pub fn udp_bind(fd: u64, port: u16) -> Result<u16, i64> {
    let result = unsafe { syscall2(67, fd, port as u64) };
    if result < 0 {
        Err(result)
    } else {
        u16::try_from(result).map_err(|_| -22)
    }
}

pub fn udp_sendto(fd: u64, destination: [u8; 4], port: u16, payload: &[u8]) -> Result<usize, i64> {
    let result = unsafe {
        syscall6(
            68,
            fd,
            u32::from_be_bytes(destination) as u64,
            port as u64,
            payload.as_ptr() as usize as u64,
            payload.len() as u64,
            0,
        )
    };
    checked_result(result)
}

pub fn udp_recvfrom(fd: u64, buffer: &mut [u8]) -> Result<(usize, [u8; 4], u16), i64> {
    let mut source_ip = [0; 4];
    let mut source_port = 0u16;
    let result = unsafe {
        syscall6(
            69,
            fd,
            buffer.as_mut_ptr() as usize as u64,
            buffer.len() as u64,
            source_ip.as_mut_ptr() as usize as u64,
            &mut source_port as *mut u16 as usize as u64,
            0,
        )
    };
    checked_result(result).map(|count| (count, source_ip, source_port))
}

pub fn dns_server() -> Result<[u8; 4], i64> {
    let mut address = [0; 4];
    let result = unsafe { syscall1(70, address.as_mut_ptr() as usize as u64) };
    if result < 0 {
        Err(result)
    } else {
        Ok(address)
    }
}

static NEXT_DNS_TRANSACTION_ID: AtomicU16 = AtomicU16::new(1);
const DNS_POLL_LIMIT: usize = 1000;
const DNS_SERVER_PORT: u16 = 53;
const EAGAIN: i64 = -11;
const EIO: i64 = -5;
const EINVAL: i64 = -22;
const ENOENT: i64 = -2;
const ETIMEDOUT: i64 = -110;

pub fn dns_lookup(hostname: &CStr) -> Result<[u8; 4], i64> {
    let server = dns_server()?;
    let hostname = hostname.to_bytes();
    let transaction_id = NEXT_DNS_TRANSACTION_ID.fetch_add(1, Ordering::Relaxed);
    let mut query = [0; 512];
    let query_length = build_dns_query(hostname, transaction_id, &mut query)?;
    let socket = udp_socket()?;
    let result = (|| {
        udp_bind(socket, 0)?;
        let mut sent = false;
        for _ in 0..DNS_POLL_LIMIT {
            match udp_sendto(socket, server, DNS_SERVER_PORT, &query[..query_length]) {
                Ok(length) if length == query_length => {
                    sent = true;
                    break;
                }
                Ok(_) => return Err(EIO),
                Err(EAGAIN) => yield_to_kernel()?,
                Err(error) => return Err(error),
            }
        }
        if !sent {
            return Err(ETIMEDOUT);
        }

        let mut response = [0; 512];
        for _ in 0..DNS_POLL_LIMIT {
            match udp_recvfrom(socket, &mut response) {
                Ok((length, source, port)) => {
                    if source != server || port != DNS_SERVER_PORT {
                        continue;
                    }
                    if let Some(address) = parse_dns_response(&response[..length], transaction_id)?
                    {
                        return Ok(address);
                    }
                }
                Err(EAGAIN) => yield_to_kernel()?,
                Err(error) => return Err(error),
            }
        }
        Err(ETIMEDOUT)
    })();
    let close_result = close(socket);
    match result {
        Err(error) => Err(error),
        Ok(address) => close_result.map(|()| address),
    }
}

fn build_dns_query(hostname: &[u8], id: u16, output: &mut [u8; 512]) -> Result<usize, i64> {
    if hostname.is_empty() || hostname.len() > 254 {
        return Err(EINVAL);
    }
    let name = hostname.strip_suffix(b".").unwrap_or(hostname);
    if name.is_empty() || name.len() > 253 {
        return Err(EINVAL);
    }
    output[..12].fill(0);
    output[..2].copy_from_slice(&id.to_be_bytes());
    output[2..4].copy_from_slice(&0x0100u16.to_be_bytes());
    output[4..6].copy_from_slice(&1u16.to_be_bytes());

    let mut cursor = 12;
    for label in name.split(|byte| *byte == b'.') {
        if label.is_empty() || label.len() > 63 {
            return Err(EINVAL);
        }
        if label
            .iter()
            .any(|byte| !byte.is_ascii_alphanumeric() && *byte != b'-')
            || label[0] == b'-'
            || label[label.len() - 1] == b'-'
        {
            return Err(EINVAL);
        }
        output[cursor] = label.len() as u8;
        cursor += 1;
        output[cursor..cursor + label.len()].copy_from_slice(label);
        cursor += label.len();
    }
    output[cursor] = 0;
    cursor += 1;
    output[cursor..cursor + 4].copy_from_slice(&[0, 1, 0, 1]);
    Ok(cursor + 4)
}

fn parse_dns_response(packet: &[u8], id: u16) -> Result<Option<[u8; 4]>, i64> {
    if packet.len() < 12 || u16::from_be_bytes([packet[0], packet[1]]) != id {
        return Ok(None);
    }
    let flags = u16::from_be_bytes([packet[2], packet[3]]);
    if flags & 0x8000 == 0 || flags & 0x7800 != 0 {
        return Ok(None);
    }
    if flags & 0x0200 != 0 {
        return Err(EIO);
    }
    match flags & 0x000f {
        0 => {}
        3 => return Err(ENOENT),
        _ => return Err(EIO),
    }
    let question_count = u16::from_be_bytes([packet[4], packet[5]]) as usize;
    let answer_count = u16::from_be_bytes([packet[6], packet[7]]) as usize;
    if question_count != 1 {
        return Err(EIO);
    }
    let mut cursor = 12;
    for _ in 0..question_count {
        cursor = skip_dns_name(packet, cursor).ok_or(EIO)?;
        cursor = cursor
            .checked_add(4)
            .filter(|end| *end <= packet.len())
            .ok_or(EIO)?;
    }
    for _ in 0..answer_count {
        cursor = skip_dns_name(packet, cursor).ok_or(EIO)?;
        let fixed_end = cursor
            .checked_add(10)
            .filter(|end| *end <= packet.len())
            .ok_or(EIO)?;
        let record_type = u16::from_be_bytes([packet[cursor], packet[cursor + 1]]);
        let record_class = u16::from_be_bytes([packet[cursor + 2], packet[cursor + 3]]);
        let data_length = u16::from_be_bytes([packet[cursor + 8], packet[cursor + 9]]) as usize;
        let data_start = fixed_end;
        let data_end = data_start
            .checked_add(data_length)
            .filter(|end| *end <= packet.len())
            .ok_or(EIO)?;
        if record_type == 1 && record_class == 1 && data_length == 4 {
            return Ok(Some(
                packet[data_start..data_end].try_into().map_err(|_| EIO)?,
            ));
        }
        cursor = data_end;
    }
    Err(ENOENT)
}

fn skip_dns_name(packet: &[u8], offset: usize) -> Option<usize> {
    let mut cursor = offset;
    let mut next = None;
    let mut expanded_length = 0usize;
    let mut pointer_count = 0usize;
    loop {
        let length = *packet.get(cursor)?;
        if length == 0 {
            if expanded_length == 255 {
                return None;
            }
            return Some(next.unwrap_or(cursor + 1));
        }
        if length & 0xc0 == 0xc0 {
            let second = *packet.get(cursor + 1)?;
            let target = (((length & 0x3f) as usize) << 8) | second as usize;
            if target >= packet.len() {
                return None;
            }
            next.get_or_insert(cursor.checked_add(2)?);
            cursor = target;
            pointer_count += 1;
            if pointer_count > 16 {
                return None;
            }
            continue;
        }
        if length & 0xc0 != 0 {
            return None;
        }
        let label_length = length as usize;
        expanded_length = expanded_length.checked_add(label_length + 1)?;
        if expanded_length > 255 {
            return None;
        }
        cursor = cursor.checked_add(label_length + 1)?;
        packet.get(cursor - label_length..cursor)?;
    }
}

pub fn fork() -> Result<u64, i64> {
    let result = unsafe { syscall1(48, 0) };
    if result < 0 {
        Err(result)
    } else {
        Ok(result as u64)
    }
}

pub fn execve(path: &CStr, argv: &[*const c_char], envp: &[*const c_char]) -> Result<(), i64> {
    let result = unsafe {
        syscall3(
            47,
            path.as_ptr() as usize as u64,
            argv.as_ptr() as usize as u64,
            envp.as_ptr() as usize as u64,
        )
    };
    if result < 0 {
        Err(result)
    } else {
        Ok(())
    }
}

pub fn waitpid(pid: u64, status: Option<&mut u64>) -> Result<u64, i64> {
    let status_pointer = status.map_or(0, |value| value as *mut u64 as usize as u64);
    let result = unsafe { syscall3(20, pid, status_pointer, 0) };
    checked_result(result).map(|value| value as u64)
}

pub fn exit(status: i32) -> ! {
    unsafe {
        syscall1(0, status as i64 as u64);
    }
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(not(test))]
#[cfg(not(test))]
struct MmapAllocator;

#[repr(C)]
struct AllocationHeader {
    mapping_base: *mut u8,
    mapping_length: usize,
}

/// Compute a mapping size that leaves room for the allocation header and
/// alignment padding. Kept separate so overflow behavior is host-testable.
fn allocation_mapping_size(size: usize, alignment: usize) -> Option<usize> {
    if alignment == 0 || !alignment.is_power_of_two() {
        return None;
    }
    let effective_alignment = alignment.max(core::mem::align_of::<AllocationHeader>());
    core::mem::size_of::<AllocationHeader>()
        .checked_add(effective_alignment - 1)?
        .checked_add(size)
}

/// Return the aligned user range inside a mapping, or None on overflow/bounds
/// errors. Keeping this arithmetic pure makes allocator layout testable on host.
fn allocation_user_range(
    mapping_base: usize,
    mapping_length: usize,
    size: usize,
    alignment: usize,
) -> Option<(usize, usize)> {
    if alignment == 0 || !alignment.is_power_of_two() {
        return None;
    }
    let effective_alignment = alignment.max(core::mem::align_of::<AllocationHeader>());
    let after_header = mapping_base.checked_add(core::mem::size_of::<AllocationHeader>())?;
    let aligned = after_header
        .checked_add(effective_alignment - 1)?
        & !(effective_alignment - 1);
    let end = aligned.checked_add(size)?;
    let mapping_end = mapping_base.checked_add(mapping_length)?;
    if end > mapping_end {
        return None;
    }
    Some((aligned, end))
}

#[cfg(not(test))]
unsafe impl GlobalAlloc for MmapAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() == 0 {
            return layout.align() as *mut u8;
        }

        let Some(length) = allocation_mapping_size(layout.size(), layout.align()) else {
            return ptr::null_mut();
        };
        let mapping = match unsafe {
            mmap(
                None,
                length,
                PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANONYMOUS,
                -1,
                0,
            )
        } {
            Ok(mapping) => mapping,
            Err(_) => return ptr::null_mut(),
        };
        let base = mapping.as_ptr() as usize;
        let Some((aligned, _end)) =
            allocation_user_range(base, length, layout.size(), layout.align())
        else {
            let _ = unsafe { munmap(mapping, length) };
            return ptr::null_mut();
        };

        let user_pointer = aligned as *mut u8;
        let header = unsafe {
            user_pointer
                .sub(core::mem::size_of::<AllocationHeader>())
                .cast::<AllocationHeader>()
        };
        unsafe {
            header.write(AllocationHeader {
                mapping_base: base as *mut u8,
                mapping_length: length,
            });
        }
        user_pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if pointer.is_null() || layout.size() == 0 {
            return;
        }
        let header = unsafe {
            pointer
                .sub(core::mem::size_of::<AllocationHeader>())
                .cast::<AllocationHeader>()
                .read()
        };
        if let Some(mapping) = NonNull::new(header.mapping_base) {
            let _ = unsafe { munmap(mapping, header.mapping_length) };
        }
    }
}

#[cfg(not(test))]
#[global_allocator]
static ALLOCATOR: MmapAllocator = MmapAllocator;

#[cfg(test)]
mod tests {
    extern crate std;

    use super::{build_dns_query, find_env_value, memcmp, memcpy, memmove, memset, parse_dns_response, skip_dns_name, strlen};
    use core::alloc::Layout;

    #[test]
    fn environment_lookup_matches_exact_name_and_value() {
        let path = b"PATH=/bin:/usr/bin\0";
        let cargo_home = b"CARGO_HOME=/home/user/.cargo\0";
        let entries = [
            path.as_ptr().cast(),
            cargo_home.as_ptr().cast(),
            core::ptr::null(),
        ];
        unsafe {
            let found = find_env_value(entries.as_ptr(), b"CARGO_HOME");
            assert!(!found.is_null());
            assert_eq!(core::ffi::CStr::from_ptr(found).to_bytes(), b"/home/user/.cargo");

            assert!(find_env_value(entries.as_ptr(), b"CARGO").is_null());
            assert!(find_env_value(entries.as_ptr(), b"MISSING").is_null());
            assert!(find_env_value(entries.as_ptr(), b"").is_null());
            assert!(find_env_value(entries.as_ptr(), b"PATH=other").is_null());
            assert!(find_env_value(core::ptr::null(), b"PATH").is_null());
        }
    }

    #[test]
    fn allocation_mapping_size_accounts_for_alignment() {
        let layout = Layout::from_size_align(123, 4096).unwrap();
        assert_eq!(
            super::allocation_mapping_size(layout.size(), layout.align()),
            Some(core::mem::size_of::<super::AllocationHeader>() + 4095 + 123)
        );
    }

    #[test]
    fn allocation_mapping_size_rejects_overflow() {
        assert_eq!(super::allocation_mapping_size(usize::MAX, 4096), None);
        assert_eq!(super::allocation_mapping_size(32, 3), None);
    }

    #[test]
    fn allocation_user_range_honors_alignment_and_mapping_bounds() {
        let header = core::mem::size_of::<super::AllocationHeader>();
        let base = 0x1000usize;
        let (pointer, end) =
            super::allocation_user_range(base, 0x4000, 256, 128).unwrap();
        assert_eq!(pointer % 128, 0);
        assert_eq!(end - pointer, 256);
        assert!(pointer >= base + header);

        assert_eq!(
            super::allocation_user_range(base, 64, 4096, 16),
            None
        );
        assert_eq!(
            super::allocation_user_range(usize::MAX - 8, 64, 8, 8),
            None
        );
        assert_eq!(super::allocation_user_range(base, 4096, 8, 0), None);
    }

    #[test]
    fn vm_abi_constants_match_kernel_contract() {
        assert_eq!(super::PROT_READ, 1);
        assert_eq!(super::PROT_WRITE, 2);
        assert_eq!(super::PROT_EXEC, 4);
        assert_eq!(super::MAP_SHARED, 1);
        assert_eq!(super::MAP_PRIVATE, 2);
        assert_eq!(super::MAP_ANONYMOUS, 0x20);
    }

    #[test]
    fn timespec_matches_octagram_c_abi() {
        assert_eq!(core::mem::size_of::<super::Timespec>(), 16);
        assert_eq!(core::mem::align_of::<super::Timespec>(), 8);
        assert_eq!(core::mem::offset_of!(super::Timespec, tv_sec), 0);
        assert_eq!(core::mem::offset_of!(super::Timespec, tv_nsec), 8);
    }

    #[test]
    fn futex_operations_match_octagram_kernel_abi() {
        assert_eq!(super::FUTEX_WAIT, 0);
        assert_eq!(super::FUTEX_WAKE, 1);
        assert_eq!(super::FUTEX_REQUEUE, 3);
        assert_eq!(super::FUTEX_WAIT_BITSET, 9);
        assert_eq!(super::FUTEX_WAKE_BITSET, 10);
        assert_eq!(super::FUTEX_BITSET_MATCH_ANY, u32::MAX as u64);
    }

    #[test]
    fn clock_ids_match_octagram_abi() {
        assert_eq!(super::CLOCK_REALTIME, 0);
        assert_eq!(super::CLOCK_MONOTONIC, 1);
    }

    #[test]
    fn freestanding_strlen_stops_at_nul() {
        let bytes = b"octagram\0ignored";
        assert_eq!(unsafe { strlen(bytes.as_ptr().cast()) }, 8);
    }

    #[test]
    fn freestanding_memset_memcpy_memcmp_work() {
        let mut source = *b"abcdefgh";
        let mut destination = [0u8; 8];
        unsafe {
            memset(source.as_mut_ptr().cast(), 0x41, source.len());
            memcpy(
                destination.as_mut_ptr().cast(),
                source.as_ptr().cast(),
                source.len(),
            );
            assert_eq!(
                memcmp(
                    destination.as_ptr().cast(),
                    source.as_ptr().cast(),
                    source.len(),
                ),
                0
            );
            destination[3] = b'B';
            assert!(memcmp(
                destination.as_ptr().cast(),
                source.as_ptr().cast(),
                source.len(),
            ) > 0);
        }
        assert_eq!(&source, b"AAAAAAAA");
        assert_eq!(&destination, b"AAABAAAA");
    }

    #[test]
    fn freestanding_memmove_handles_overlapping_ranges_both_directions() {
        let mut bytes = *b"abcdefgh";
        unsafe {
            memmove(bytes.as_mut_ptr().add(2).cast(), bytes.as_ptr().cast(), 6);
        }
        assert_eq!(&bytes, b"ababcdef");
        unsafe {
            memmove(bytes.as_mut_ptr().cast(), bytes.as_ptr().add(2).cast(), 6);
        }
        assert_eq!(&bytes, b"abcdefef");
    }

    #[test]
    fn dns_query_encodes_labels_and_accepts_trailing_dot() {
        let mut query = [0; 512];
        let length = build_dns_query(b"example.com.", 0x1234, &mut query).unwrap();
        assert_eq!(&query[..12], &[0x12, 0x34, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            &query[12..length],
            &[7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0, 0, 1, 0, 1]
        );
        assert_eq!(build_dns_query(b"bad..name", 0x1234, &mut query), Err(-22));
    }

    #[test]
    fn dns_response_reads_compressed_ipv4_answer() {
        let mut packet = [0; 45];
        packet[..12].copy_from_slice(&[0x12, 0x34, 0x81, 0x80, 0, 1, 0, 1, 0, 0, 0, 0]);
        packet[12..25].copy_from_slice(&[
            7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0,
        ]);
        packet[25..29].copy_from_slice(&[0, 1, 0, 1]);
        packet[29..41].copy_from_slice(&[0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4]);
        packet[41..45].copy_from_slice(&[93, 184, 216, 34]);
        assert_eq!(
            parse_dns_response(&packet, 0x1234),
            Ok(Some([93, 184, 216, 34]))
        );
    }

    #[test]
    fn dns_response_rejects_bad_compression_and_reports_nxdomain() {
        let mut cyclic_name = [0; 14];
        cyclic_name[12..14].copy_from_slice(&[0xc0, 0x0c]);
        assert_eq!(skip_dns_name(&cyclic_name, 12), None);

        let mut response = [0; 12];
        response[..4].copy_from_slice(&[0x12, 0x34, 0x81, 0x83]);
        assert_eq!(parse_dns_response(&response, 0x1234), Err(-2));
    }

    #[test]
    fn dns_response_reports_no_a_record_and_truncation() {
        let mut response = [0; 29];
        response[..12].copy_from_slice(&[0x12, 0x34, 0x81, 0x80, 0, 1, 0, 0, 0, 0, 0, 0]);
        response[12..25].copy_from_slice(&[
            7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm', 0,
        ]);
        response[25..29].copy_from_slice(&[0, 1, 0, 1]);
        assert_eq!(parse_dns_response(&response, 0x1234), Err(-2));

        response[2] |= 0x02;
        assert_eq!(parse_dns_response(&response, 0x1234), Err(-5));
    }
}
