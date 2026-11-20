#![no_std]

#[cfg(not(test))]
use core::alloc::{GlobalAlloc, Layout};
use core::ffi::{c_char, c_void, CStr};
use core::ptr;
#[cfg(not(test))]
use core::sync::atomic::AtomicBool;
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

#[no_mangle]
pub static mut environ: *mut *mut c_char = ptr::null_mut();

pub const PROT_READ: u64 = 1;
pub const PROT_WRITE: u64 = 2;
pub const PROT_EXEC: u64 = 4;
const MAP_PRIVATE: u64 = 2;
const MAP_ANONYMOUS: u64 = 0x20;

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
struct HeapGuard;

#[cfg(not(test))]
static HEAP_LOCKED: AtomicBool = AtomicBool::new(false);

#[cfg(not(test))]
impl HeapGuard {
    fn acquire() -> Self {
        while HEAP_LOCKED
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        Self
    }
}

#[cfg(not(test))]
impl Drop for HeapGuard {
    fn drop(&mut self) {
        HEAP_LOCKED.store(false, Ordering::Release);
    }
}

#[cfg(not(test))]
struct MmapAllocator;

#[cfg(not(test))]
static mut HEAP_NEXT: usize = 0;
#[cfg(not(test))]
static mut HEAP_END: usize = 0;

#[cfg(not(test))]
unsafe impl GlobalAlloc for MmapAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() == 0 {
            return layout.align() as *mut u8;
        }

        let _guard = HeapGuard::acquire();
        let Some(end) = layout.size().checked_add(layout.align() - 1) else {
            return ptr::null_mut();
        };
        let mut start = unsafe { HEAP_NEXT };
        let mut limit = unsafe { HEAP_END };
        let Some(aligned) = start
            .checked_add(layout.align() - 1)
            .map(|value| value & !(layout.align() - 1))
        else {
            return ptr::null_mut();
        };
        if aligned
            .checked_add(layout.size())
            .map_or(true, |end| end > limit)
        {
            let length = end.max(64 * 1024);
            let result = unsafe {
                syscall6(
                    16,
                    0,
                    length as u64,
                    PROT_READ | PROT_WRITE,
                    MAP_PRIVATE | MAP_ANONYMOUS,
                    u64::MAX,
                    0,
                )
            };
            if result < 0 {
                return ptr::null_mut();
            }
            start = result as usize;
            let Some(new_limit) = start.checked_add(length) else {
                return ptr::null_mut();
            };
            limit = new_limit;
        }
        let Some(aligned) = start
            .checked_add(layout.align() - 1)
            .map(|value| value & !(layout.align() - 1))
        else {
            return ptr::null_mut();
        };
        let Some(new_next) = aligned.checked_add(layout.size()) else {
            return ptr::null_mut();
        };
        if new_next > limit {
            return ptr::null_mut();
        }
        unsafe {
            HEAP_NEXT = new_next;
            HEAP_END = limit;
        }
        aligned as *mut u8
    }

    unsafe fn dealloc(&self, _pointer: *mut u8, _layout: Layout) {}
}

#[cfg(not(test))]
#[cfg_attr(not(test), global_allocator)]
static ALLOCATOR: MmapAllocator = MmapAllocator;

#[cfg(test)]
mod tests {
    extern crate std;

    use super::{build_dns_query, memcmp, memcpy, memmove, memset, parse_dns_response, skip_dns_name};

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
            assert_eq!(memcmp(
                destination.as_ptr().cast(),
                source.as_ptr().cast(),
                source.len(),
            ), 0);
        }
        assert_eq!(&source, b"AAAAAAAA");
        assert_eq!(&destination, b"AAAAAAAA");
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
