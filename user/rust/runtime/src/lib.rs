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
pub const O_CLOEXEC: u64 = 1 << 19;


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
    let value = unsafe { find_env_value(environment as *const *const c_char, name.to_bytes()) };
    if value.is_null() {
        None
    } else {
        Some(unsafe { CStr::from_ptr(value) })
    }
}

/// Maximum errno value reserved by the OCTAGRAM syscall ABI.
pub const MAX_ERRNO: i32 = 4095;

/// A decoded negative errno returned by an OCTAGRAM syscall.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SysError {
    code: i32,
}

impl SysError {
    /// Decode a syscall return value. Non-error values return None.
    pub const fn from_return(value: i64) -> Option<Self> {
        if value < 0 && value >= -(MAX_ERRNO as i64) {
            Some(Self { code: (-value) as i32 })
        } else {
            None
        }
    }

    /// Positive POSIX errno number.
    pub const fn code(self) -> i32 { self.code }

    /// Convert back to the OCTAGRAM syscall convention.
    pub const fn to_return(self) -> i64 { -(self.code as i64) }

    pub const fn is_interrupted(self) -> bool { self.code == 4 }
    pub const fn is_would_block(self) -> bool { self.code == 11 }
    pub const fn is_not_found(self) -> bool { self.code == 2 }
    pub const fn is_permission_denied(self) -> bool {
        self.code == 1 || self.code == 13
    }
}

impl core::fmt::Display for SysError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "OCTAGRAM syscall error {}", self.code)
    }
}

/// Decode a syscall result into a value or a typed OS error.
pub const fn decode_syscall_result(value: i64) -> Result<i64, SysError> {
    match SysError::from_return(value) {
        Some(error) => Err(error),
        None => Ok(value),
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

unsafe fn syscall4(number: u64, arg1: u64, arg2: u64, arg3: u64, arg4: u64) -> i64 {
    let result: i64;
    unsafe {
        core::arch::asm!(
            "int 0x80",
            inlateout("rax") number as i64 => result,
            in("rdi") arg1,
            in("rsi") arg2,
            in("rdx") arg3,
            in("r10") arg4,
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

pub fn write_all(fd: u64, bytes: &[u8]) -> Result<(), i64> {
    write_all_with(bytes, |remaining| write(fd, remaining))
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

/// POSIX-compatible file type bits carried in OCTAGRAM's compact metadata.
pub const S_IFMT: u32 = 0o170000;
pub const S_IFIFO: u32 = 0o010000;
pub const S_IFCHR: u32 = 0o020000;
pub const S_IFDIR: u32 = 0o040000;
pub const S_IFBLK: u32 = 0o060000;
pub const S_IFREG: u32 = 0o100000;
pub const S_IFLNK: u32 = 0o120000;
pub const S_IFSOCK: u32 = 0o140000;

impl FileStat {
    /// Return the POSIX file type portion of the mode.
    pub const fn file_type(&self) -> u32 {
        self.mode & S_IFMT
    }

    pub const fn is_file(&self) -> bool {
        self.file_type() == S_IFREG
    }

    pub const fn is_directory(&self) -> bool {
        self.file_type() == S_IFDIR
    }

    pub const fn is_symlink(&self) -> bool {
        self.file_type() == S_IFLNK
    }

    pub const fn is_character_device(&self) -> bool {
        self.file_type() == S_IFCHR
    }

    /// Return the permission and special bits, excluding the file type.
    pub const fn permissions(&self) -> u32 {
        self.mode & !S_IFMT
    }
}

/// Read file metadata without opening a descriptor.
pub const AT_FDCWD: i64 = -100;
pub const AT_SYMLINK_NOFOLLOW: u64 = 0x100;
pub const SEEK_SET: u64 = 0;
pub const SEEK_CUR: u64 = 1;
pub const SEEK_END: u64 = 2;

/// Open a path relative to a directory descriptor (syscall 40).
pub fn openat(dirfd: i64, path: &CStr, flags: u64, mode: u32) -> Result<u64, i64> {
    let result = unsafe {
        syscall4(40, dirfd as u64, path.as_ptr() as usize as u64, flags, mode as u64)
    };
    if result < 0 { Err(result) } else { Ok(result as u64) }
}

/// Change or query a descriptor's current file offset (syscall 5).
pub fn lseek(fd: u64, offset: i64, whence: u64) -> Result<i64, i64> {
    let result = unsafe { syscall3(5, fd, offset as u64, whence) };
    if result < 0 { Err(result) } else { Ok(result) }
}

/// Resize an open file (syscall 9).
pub fn ftruncate(fd: u64, length: u64) -> Result<(), i64> {
    let result = unsafe { syscall2(9, fd, length) };
    if result < 0 { Err(result) } else { Ok(()) }
}

/// Flush file data and metadata to the filesystem (syscall 119).
pub fn fsync(fd: u64) -> Result<(), i64> {
    let result = unsafe { syscall1(119, fd) };
    if result < 0 { Err(result) } else { Ok(()) }
}

/// Flush file data without requiring metadata synchronization (syscall 120).
pub fn fdatasync(fd: u64) -> Result<(), i64> {
    let result = unsafe { syscall1(120, fd) };
    if result < 0 { Err(result) } else { Ok(()) }
}

/// Read from a descriptor at an explicit offset without changing its file position (syscall 102).
pub fn pread(fd: u64, buffer: &mut [u8], offset: i64) -> Result<usize, i64> {
    let result = unsafe {
        syscall4(102, fd, buffer.as_mut_ptr() as usize as u64, buffer.len() as u64, offset as u64)
    };
    checked_result(result)
}

/// Write to a descriptor at an explicit offset without changing its file position (syscall 103).
pub fn pwrite(fd: u64, bytes: &[u8], offset: i64) -> Result<usize, i64> {
    let result = unsafe {
        syscall4(103, fd, bytes.as_ptr() as usize as u64, bytes.len() as u64, offset as u64)
    };
    checked_result(result)
}
/// Read metadata for an already-open descriptor (syscall 29).
pub fn fstat(fd: u64) -> Result<FileStat, i64> {
    let mut metadata = FileStat::default();
    let result = unsafe { syscall2(29, fd, &mut metadata as *mut FileStat as usize as u64) };
    if result < 0 { Err(result) } else { Ok(metadata) }
}

/// Read metadata relative to a directory descriptor (syscall 41).
pub fn fstatat(dirfd: i64, path: &CStr, flags: u64) -> Result<FileStat, i64> {
    let mut metadata = FileStat::default();
    let result = unsafe {
        syscall4(
            41,
            dirfd as u64,
            path.as_ptr() as usize as u64,
            &mut metadata as *mut FileStat as usize as u64,
            flags,
        )
    };
    if result < 0 { Err(result) } else { Ok(metadata) }
}

/// Directory entry layout shared with OCTAGRAM's C `struct dirent`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dirent {
    pub d_ino: u64,
    pub d_off: i64,
    pub d_reclen: u16,
    pub d_type: u8,
    pub d_name: [c_char; 256],
}

/// Directory-entry type values shared with user/include/dirent.h.
pub const DT_UNKNOWN: u8 = 0;
pub const DT_FIFO: u8 = 1;
pub const DT_CHR: u8 = 2;
pub const DT_DIR: u8 = 4;
pub const DT_BLK: u8 = 6;
pub const DT_REG: u8 = 8;
pub const DT_LNK: u8 = 10;
pub const DT_SOCK: u8 = 12;

impl Dirent {
    pub const fn is_file(&self) -> bool {
        self.d_type == DT_REG
    }

    pub const fn is_directory(&self) -> bool {
        self.d_type == DT_DIR
    }

    pub const fn is_symlink(&self) -> bool {
        self.d_type == DT_LNK
    }

    /// Return the entry name only when the record length contains a
    /// NUL-terminated name within the fixed OCTAGRAM ABI buffer.
    pub fn name_bytes(&self) -> Option<&[u8]> {
        let record_len = usize::from(self.d_reclen);
        let name_offset = core::mem::offset_of!(Self, d_name);
        if record_len > core::mem::size_of::<Self>() || record_len <= name_offset {
            return None;
        }

        let name_capacity = (record_len - name_offset).min(self.d_name.len());
        let name = unsafe {
            core::slice::from_raw_parts(self.d_name.as_ptr().cast::<u8>(), name_capacity)
        };
        let nul = name.iter().position(|byte| *byte == 0)?;
        Some(&name[..nul])
    }
}

/// Open a directory and return its descriptor (syscall 27).
pub fn opendir(path: &CStr) -> Result<u64, i64> {
    let result = unsafe { syscall1(27, path.as_ptr() as usize as u64) };
    if result < 0 { Err(result) } else { Ok(result as u64) }
}

/// Read the next snapshot entry from a directory descriptor (syscall 28).
/// Returns `Ok(None)` at end-of-directory.
pub fn readdir(directory_fd: u64) -> Result<Option<Dirent>, i64> {
    let mut entry = Dirent {
        d_ino: 0, d_off: 0, d_reclen: 0, d_type: 0, d_name: [0; 256],
    };
    let result = unsafe {
        syscall2(28, directory_fd, &mut entry as *mut Dirent as usize as u64)
    };
    match result {
        0 => Ok(None),
        1 => Ok(Some(entry)),
        value if value < 0 => Err(value),
        _ => Err(-5),
    }
}

/// Close a directory descriptor returned by `opendir`.
pub fn closedir(directory_fd: u64) -> Result<(), i64> {
    close(directory_fd)
}

/// Create a directory relative to a directory descriptor (syscall 42).
pub fn mkdirat(dirfd: i64, path: &CStr, mode: u32) -> Result<(), i64> {
    let result = unsafe {
        syscall3(42, dirfd as u64, path.as_ptr() as usize as u64, mode as u64)
    };
    if result < 0 { Err(result) } else { Ok(()) }
}

/// Remove a file or directory relative to a directory descriptor (syscall 43).
pub fn unlinkat(dirfd: i64, path: &CStr, flags: u64) -> Result<(), i64> {
    let result = unsafe {
        syscall3(43, dirfd as u64, path.as_ptr() as usize as u64, flags)
    };
    if result < 0 { Err(result) } else { Ok(()) }
}

/// Rename a path between directory descriptors (syscall 44).
pub fn renameat(
    old_dirfd: i64,
    old_path: &CStr,
    new_dirfd: i64,
    new_path: &CStr,
) -> Result<(), i64> {
    let result = unsafe {
        syscall4(
            44,
            old_dirfd as u64,
            old_path.as_ptr() as usize as u64,
            new_dirfd as u64,
            new_path.as_ptr() as usize as u64,
        )
    };
    if result < 0 { Err(result) } else { Ok(()) }
}

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

/// An owned OCTAGRAM file descriptor.
///
/// Dropping this value closes the descriptor on the OCTAGRAM target. On host
/// builds used for unit tests, Drop is inert because OCTAGRAM syscalls cannot
/// be issued by the host test process.
#[derive(Debug, PartialEq, Eq)]
pub struct OwnedFd {
    raw: u64,
}

const INVALID_FD: u64 = u64::MAX;

impl OwnedFd {
    /// Take ownership of a descriptor returned by an OCTAGRAM syscall.
    ///
    /// # Safety
    /// The caller must own `raw` and must not close it or create another
    /// owning wrapper for the same descriptor.
    pub const unsafe fn from_raw_fd(raw: u64) -> Self {
        Self { raw }
    }

    /// Return the underlying descriptor without transferring ownership.
    pub const fn as_raw_fd(&self) -> u64 {
        self.raw
    }

    /// Consume this wrapper and transfer descriptor ownership to the caller.
    pub fn into_raw_fd(mut self) -> u64 {
        core::mem::replace(&mut self.raw, INVALID_FD)
    }

    /// Explicitly close the descriptor and consume this owner.
    pub fn close_owned(mut self) -> Result<(), i64> {
        let raw = core::mem::replace(&mut self.raw, INVALID_FD);
        if raw == INVALID_FD {
            Ok(())
        } else {
            close(raw)
        }
    }

    /// Read bytes through this owned descriptor.
    pub fn read(&self, buffer: &mut [u8]) -> Result<usize, i64> {
        read(self.raw, buffer)
    }

    /// Write bytes through this owned descriptor.
    pub fn write(&self, bytes: &[u8]) -> Result<usize, i64> {
        write(self.raw, bytes)
    }

    /// Seek this descriptor.
    pub fn seek(&self, offset: i64, whence: u64) -> Result<i64, i64> {
        lseek(self.raw, offset, whence)
    }
}

impl Drop for OwnedFd {
    fn drop(&mut self) {
        if self.raw != INVALID_FD {
            #[cfg(target_os = "octagram")]
            {
                let _ = close(self.raw);
            }
            self.raw = INVALID_FD;
        }
    }
}

/// A small std-like file handle built on OCTAGRAM's owned descriptor.
///
/// This is deliberately a no_std abstraction: it provides the byte-oriented
/// operations needed by a future std::sys::pal implementation without
/// pretending that the full Rust standard library is available.
#[derive(Debug, PartialEq, Eq)]
pub struct File {
    fd: OwnedFd,
}

impl File {
    /// Open an existing path with the supplied OCTAGRAM open flags.
    pub fn open(path: &CStr, flags: u64) -> Result<Self, i64> {
        open_owned(path, flags).map(Self::from_owned_fd)
    }

    /// Open a path relative to a directory descriptor.
    pub fn openat(dirfd: i64, path: &CStr, flags: u64, mode: u32) -> Result<Self, i64> {
        openat_owned(dirfd, path, flags, mode).map(Self::from_owned_fd)
    }

    /// Wrap an already-owned descriptor.
    pub const fn from_owned_fd(fd: OwnedFd) -> Self { Self { fd } }
    /// Borrow the underlying descriptor.
    pub const fn as_raw_fd(&self) -> u64 { self.fd.as_raw_fd() }
    /// Read up to the buffer length. A zero result is EOF.
    pub fn read(&self, buffer: &mut [u8]) -> Result<usize, i64> { self.fd.read(buffer) }
    /// Fill the complete buffer, returning -EIO if EOF arrives too early.
    pub fn read_exact(&self, buffer: &mut [u8]) -> Result<(), i64> {
        read_exact_with(buffer, |remaining| self.read(remaining))
    }
    /// Write one buffer; a short write is allowed.
    pub fn write(&self, bytes: &[u8]) -> Result<usize, i64> { self.fd.write(bytes) }
    /// Write all bytes, handling partial writes. A zero write is -EIO.
    pub fn write_all(&self, bytes: &[u8]) -> Result<(), i64> {
        write_all_with(bytes, |remaining| self.write(remaining))
    }
    /// Read at an explicit offset without changing the shared file position.
    pub fn read_at(&self, buffer: &mut [u8], offset: i64) -> Result<usize, i64> {
        pread(self.as_raw_fd(), buffer, offset)
    }
    /// Fill a buffer using positional reads; early EOF returns -EIO.
    pub fn read_exact_at(&self, buffer: &mut [u8], offset: i64) -> Result<(), i64> {
        read_exact_at_with(buffer, offset, |remaining, position| self.read_at(remaining, position))
    }
    /// Write at an explicit offset without changing the shared file position.
    pub fn write_at(&self, bytes: &[u8], offset: i64) -> Result<usize, i64> {
        pwrite(self.as_raw_fd(), bytes, offset)
    }
    /// Write all bytes positionally, handling partial writes.
    pub fn write_all_at(&self, bytes: &[u8], offset: i64) -> Result<(), i64> {
        write_all_at_with(bytes, offset, |remaining, position| self.write_at(remaining, position))
    }
    /// Change or query the current file offset.
    pub fn seek(&self, offset: i64, whence: u64) -> Result<i64, i64> {
        self.fd.seek(offset, whence)
    }
    /// Resize this file. The new length is measured in bytes.
    pub fn set_len(&self, length: u64) -> Result<(), i64> {
        ftruncate(self.as_raw_fd(), length)
    }
    /// Flush file contents and metadata to the filesystem.
    pub fn sync_all(&self) -> Result<(), i64> { fsync(self.as_raw_fd()) }
    /// Flush file contents without requiring metadata synchronization.
    pub fn sync_data(&self) -> Result<(), i64> { fdatasync(self.as_raw_fd()) }
    /// Read metadata for this open descriptor.
    pub fn metadata(&self) -> Result<FileStat, i64> { fstat(self.as_raw_fd()) }
    /// Transfer ownership of the raw descriptor to the caller.
    pub fn into_raw_fd(self) -> u64 { self.fd.into_raw_fd() }
    /// Close the descriptor explicitly.
    pub fn close(self) -> Result<(), i64> { self.fd.close_owned() }
}

/// RAII directory stream for the OCTAGRAM snapshot-based readdir ABI.
#[derive(Debug, PartialEq, Eq)]
pub struct Directory {
    fd: OwnedFd,
}

impl Directory {
    /// Open a directory stream. The descriptor is closed when this value drops
    /// on the OCTAGRAM target.
    pub fn open(path: &CStr) -> Result<Self, i64> {
        opendir(path).map(|fd| Self {
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
        })
    }

    /// Read the next directory entry, or None after end-of-directory.
    pub fn next_entry(&self) -> Result<Option<Dirent>, i64> {
        readdir(self.fd.as_raw_fd())
    }

    /// Borrow the underlying directory descriptor.
    pub const fn as_raw_fd(&self) -> u64 {
        self.fd.as_raw_fd()
    }

    /// Close the directory stream explicitly.
    pub fn close(self) -> Result<(), i64> {
        self.fd.close_owned()
    }
}

fn read_exact_with<F>(mut buffer: &mut [u8], mut read_once: F) -> Result<(), i64>
where F: FnMut(&mut [u8]) -> Result<usize, i64>,
{
    while !buffer.is_empty() {
        let count = read_once(buffer)?;
        if count == 0 || count > buffer.len() { return Err(-5); }
        buffer = &mut buffer[count..];
    }
    Ok(())
}

fn read_exact_at_with<F>(mut buffer: &mut [u8], mut offset: i64, mut read_once: F) -> Result<(), i64>
where F: FnMut(&mut [u8], i64) -> Result<usize, i64>,
{
    while !buffer.is_empty() {
        let count = read_once(buffer, offset)?;
        if count == 0 || count > buffer.len() { return Err(-5); }
        offset = offset.checked_add(count as i64).ok_or(-22)?;
        buffer = &mut buffer[count..];
    }
    Ok(())
}

fn write_all_at_with<F>(mut bytes: &[u8], mut offset: i64, mut write_once: F) -> Result<(), i64>
where F: FnMut(&[u8], i64) -> Result<usize, i64>,
{
    while !bytes.is_empty() {
        let chunk_len = bytes.len().min(MAX_SYSCALL_IO);
        let count = write_once(&bytes[..chunk_len], offset)?;
        if count == 0 || count > chunk_len { return Err(-5); }
        offset = offset.checked_add(count as i64).ok_or(-22)?;
        bytes = &bytes[count..];
    }
    Ok(())
}
fn write_all_with<F>(mut bytes: &[u8], mut write_once: F) -> Result<(), i64>
where F: FnMut(&[u8]) -> Result<usize, i64>,
{
    while !bytes.is_empty() {
        let chunk_len = bytes.len().min(MAX_SYSCALL_IO);
        let count = write_once(&bytes[..chunk_len])?;
        if count == 0 || count > chunk_len { return Err(-5); }
        bytes = &bytes[count..];
    }
    Ok(())
}
/// Open a path and return an owning descriptor wrapper.
pub fn open_owned(path: &CStr, flags: u64) -> Result<OwnedFd, i64> {
    open(path, flags).map(|fd| unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Open a directory-relative path and return an owning descriptor wrapper.
pub fn openat_owned(
    dirfd: i64,
    path: &CStr,
    flags: u64,
    mode: u32,
) -> Result<OwnedFd, i64> {
    openat(dirfd, path, flags, mode).map(|fd| unsafe { OwnedFd::from_raw_fd(fd) })
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

/// Decode the close-on-exec handshake sent by a child when execve fails.
/// None means EOF without a payload: execve replaced the child image.
fn decode_exec_handshake(bytes: &[u8], eof: bool) -> Result<Option<i64>, i64> {
    if bytes.is_empty() && eof {
        return Ok(None);
    }
    if bytes.len() != core::mem::size_of::<i64>() {
        return Err(-5);
    }
    let mut encoded = [0u8; 8];
    encoded.copy_from_slice(bytes);
    let error = i64::from_le_bytes(encoded);
    if error >= -(MAX_ERRNO as i64) && error < 0 {
        Ok(Some(error))
    } else {
        Err(-5)
    }
}

/// Spawn a program by forking, applying optional stdio descriptor mappings,
/// then executing it. argv and envp must each include a trailing null pointer.
/// The stdio array is [stdin, stdout, stderr]; None preserves the inherited fd.
pub fn spawn(
    path: &CStr,
    argv: &[*const c_char],
    envp: &[*const c_char],
) -> Result<Child, i64> {
    spawn_with_stdio(path, argv, envp, [None, None, None])
}

/// Spawn with explicit stdin/stdout/stderr descriptor sources.
///
/// A close-on-exec pipe reports the exact execve error to the parent. EOF
/// without a payload means the kernel successfully replaced the child image.
pub fn spawn_with_stdio(
    path: &CStr,
    argv: &[*const c_char],
    envp: &[*const c_char],
    stdio: [Option<u64>; 3],
) -> Result<Child, i64> {
    if argv.last().copied() != Some(ptr::null())
        || envp.last().copied() != Some(ptr::null())
    {
        return Err(-22);
    }

    let descriptors = pipe2(O_CLOEXEC)?;
    let read_fd = descriptors[0] as u64;
    let write_fd = descriptors[1] as u64;
    let pid = match fork() {
        Ok(pid) => pid,
        Err(error) => {
            let _ = close(read_fd);
            let _ = close(write_fd);
            return Err(error);
        }
    };

    if pid == 0 {
        let _ = close(read_fd);
        for (target, source) in stdio.iter().enumerate() {
            if let Some(source) = source {
                if let Err(error) = dup3(*source, target as u64, 0) {
                    let _ = write_all(write_fd, &error.to_le_bytes());
                    let _ = close(write_fd);
                    exit(127);
                }
            }
        }
        if let Err(error) = execve(path, argv, envp) {
            let _ = write_all(write_fd, &error.to_le_bytes());
            let _ = close(write_fd);
            exit(127);
        }
        // A successful execve does not return to this image.
        exit(127);
    }

    let _ = close(write_fd);
    let mut payload = [0u8; 8];
    let mut received = 0usize;
    let mut eof = false;
    while received < payload.len() {
        match read(read_fd, &mut payload[received..]) {
            Ok(0) => { eof = true; break; }
            Ok(count) if count <= payload.len() - received => received += count,
            Ok(_) => {
                let _ = close(read_fd);
                let mut child = Child::from_pid(pid);
                let _ = child.wait();
                return Err(-5);
            }
            Err(error) => {
                let _ = close(read_fd);
                let mut child = Child::from_pid(pid);
                let _ = child.wait();
                return Err(error);
            }
        }
    }
    let _ = close(read_fd);

    match decode_exec_handshake(&payload[..received], eof) {
        Ok(None) => Ok(Child::from_pid(pid)),
        Ok(Some(error)) => {
            let mut child = Child::from_pid(pid);
            let _ = child.wait();
            Err(error)
        }
        Err(error) => {
            let mut child = Child::from_pid(pid);
            let _ = child.wait();
            Err(error)
        }
    }
}

/// Special target accepted by waitpid to wait for any direct child.
pub const WAIT_ANY_PID: u64 = u64::MAX;
/// Return immediately when the selected child has not exited yet.
pub const WNOHANG: u64 = 1;

/// Wait for a child using the OCTAGRAM waitpid options bitmask.
pub fn waitpid_with_options(
    pid: u64,
    status: Option<&mut u64>,
    options: u64,
) -> Result<u64, i64> {
    let status_pointer = status.map_or(0, |value| value as *mut u64 as usize as u64);
    let result = unsafe { syscall3(20, pid, status_pointer, options) };
    checked_result(result).map(|value| value as u64)
}

/// Wait until a child exits, preserving the original simple API.
pub fn waitpid(pid: u64, status: Option<&mut u64>) -> Result<u64, i64> {
    waitpid_with_options(pid, status, 0)
}

/// A handle to a direct child process created by fork or another spawn API.
///
/// The status returned by wait methods is the raw 64-bit value passed by the
/// child to exit; OCTAGRAM does not apply Linux wait-status bit encoding.
#[derive(Debug, PartialEq, Eq)]
pub struct Child {
    pid: Option<u64>,
}

impl Child {
    /// Track a PID known to be a direct child of the current process.
    pub const fn from_pid(pid: u64) -> Self { Self { pid: Some(pid) } }

    /// Return the child's PID, or None after it has been reaped.
    pub const fn id(&self) -> Option<u64> { self.pid }

    /// Block until the child exits and return its raw exit value.
    pub fn wait(&mut self) -> Result<u64, i64> {
        let pid = self.pid.ok_or(-10)?;
        let mut status = 0u64;
        let reaped = waitpid(pid, Some(&mut status))?;
        if reaped != pid { return Err(-10); }
        self.pid = None;
        Ok(status)
    }

    /// Check whether the child has exited without blocking.
    pub fn try_wait(&mut self) -> Result<Option<u64>, i64> {
        let pid = self.pid.ok_or(-10)?;
        let mut status = 0u64;
        let reaped = waitpid_with_options(pid, Some(&mut status), WNOHANG)?;
        if reaped == 0 { return Ok(None); }
        if reaped != pid { return Err(-10); }
        self.pid = None;
        Ok(Some(status))
    }
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

    use super::{build_dns_query, find_env_value, memcmp, memcpy, memmove, memset, parse_dns_response, skip_dns_name, strlen, Dirent};
    use core::alloc::Layout;

    #[test]
    fn exec_handshake_distinguishes_success_failure_and_corruption() {
        assert_eq!(super::decode_exec_handshake(&[], true), Ok(None));
        assert_eq!(super::decode_exec_handshake(&(-2i64).to_le_bytes(), true), Ok(Some(-2)));
        assert_eq!(super::decode_exec_handshake(&[], false), Err(-5));
        assert_eq!(super::decode_exec_handshake(&[1, 2, 3], true), Err(-5));
        assert_eq!(super::decode_exec_handshake(&0i64.to_le_bytes(), true), Err(-5));
        assert_eq!(super::decode_exec_handshake(&(-4096i64).to_le_bytes(), true), Err(-5));
    }

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
    fn owned_fd_supports_borrow_and_ownership_transfer() {
        let owned = unsafe { super::OwnedFd::from_raw_fd(42) };
        assert_eq!(owned.as_raw_fd(), 42);
        let raw = owned.into_raw_fd();
        assert_eq!(raw, 42);

        let owned = unsafe { super::OwnedFd::from_raw_fd(7) };
        assert_eq!(owned.as_raw_fd(), 7);
        // Host Drop is intentionally inert; this test must not issue int 0x80.
        drop(owned);
    }

    #[test]
    fn file_read_exact_handles_partial_reads_and_reports_early_eof() {
        let source = b"octagram";
        let mut offset = 0usize;
        let mut output = [0u8; 8];
        let result = super::read_exact_with(&mut output, |remaining| {
            let count = remaining.len().min(3).min(source.len() - offset);
            remaining[..count].copy_from_slice(&source[offset..offset + count]);
            offset += count;
            Ok(count)
        });
        assert_eq!(result, Ok(()));
        assert_eq!(&output, source);
        let mut short = [0u8; 9];
        assert_eq!(super::read_exact_with(&mut short, |remaining| {
            let count = remaining.len().min(source.len() - offset);
            remaining[..count].copy_from_slice(&source[offset..offset + count]);
            offset += count;
            Ok(count)
        }), Err(-5));
    }

    #[test]
    fn file_write_all_handles_partial_writes_and_rejects_zero_progress() {
        let source = b"native-file-api";
        let mut output = std::vec::Vec::new();
        assert_eq!(super::write_all_with(source, |remaining| {
            let count = remaining.len().min(4);
            output.extend_from_slice(&remaining[..count]);
            Ok(count)
        }), Ok(()));
        assert_eq!(output, source);
        assert_eq!(super::write_all_with(source, |_| Ok(0)), Err(-5));
    }

    #[test]
    fn positional_read_exact_handles_partial_reads_and_offsets() {
        let source = b"0123456789";
        let mut output = [0u8; 5];
        let mut observed = std::vec::Vec::new();
        assert_eq!(super::read_exact_at_with(&mut output, 3, |remaining, offset| {
            observed.push(offset);
            let start = offset as usize;
            let count = remaining.len().min(2).min(source.len() - start);
            remaining[..count].copy_from_slice(&source[start..start + count]);
            Ok(count)
        }), Ok(()));
        assert_eq!(&output, b"34567");
        assert_eq!(observed, [3, 5, 7]);
    }

    #[test]
    fn positional_write_all_advances_offset_after_partial_writes() {
        let mut output = [0u8; 12];
        let mut observed = std::vec::Vec::new();
        assert_eq!(super::write_all_at_with(b"octagram", 2, |remaining, offset| {
            observed.push(offset);
            let count = remaining.len().min(3);
            let start = offset as usize;
            output[start..start + count].copy_from_slice(&remaining[..count]);
            Ok(count)
        }), Ok(()));
        assert_eq!(&output[2..10], b"octagram");
        assert_eq!(observed, [2, 5, 8]);
        assert_eq!(super::write_all_at_with(b"x", 0, |_, _| Ok(0)), Err(-5));
        assert_eq!(super::write_all_at_with(b"x", i64::MAX, |_, _| Ok(1)), Err(-22));
    }
    #[test]
    fn file_wraps_owned_descriptor_and_transfers_it() {
        let owned = unsafe { super::OwnedFd::from_raw_fd(19) };
        let file = super::File::from_owned_fd(owned);
        assert_eq!(file.as_raw_fd(), 19);
        assert_eq!(file.into_raw_fd(), 19);
    }
    #[test]
    fn syscall_error_decoding_preserves_errno_semantics() {
        let interrupted = super::SysError::from_return(-4).unwrap();
        assert_eq!(interrupted.code(), 4);
        assert_eq!(interrupted.to_return(), -4);
        assert!(interrupted.is_interrupted());

        let missing = super::SysError::from_return(-2).unwrap();
        assert!(missing.is_not_found());
        assert!(!missing.is_permission_denied());

        let denied = super::SysError::from_return(-13).unwrap();
        assert!(denied.is_permission_denied());
        assert!(super::SysError::from_return(-1).unwrap().is_permission_denied());

        let blocked = super::SysError::from_return(-11).unwrap();
        assert!(blocked.is_would_block());

        assert!(super::SysError::from_return(0).is_none());
        assert!(super::SysError::from_return(17).is_none());
        assert!(super::SysError::from_return(-4096).is_none());
        assert_eq!(super::decode_syscall_result(23), Ok(23));
        assert_eq!(super::decode_syscall_result(-2), Err(missing));
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
    fn file_stat_matches_octagram_c_abi() {
        assert_eq!(core::mem::size_of::<super::FileStat>(), 16);
        assert_eq!(core::mem::align_of::<super::FileStat>(), 8);
        assert_eq!(core::mem::offset_of!(super::FileStat, size), 0);
        assert_eq!(core::mem::offset_of!(super::FileStat, mode), 8);
        assert_eq!(core::mem::offset_of!(super::FileStat, modified_seconds), 12);
    }

    #[test]
    fn at_and_seek_constants_match_octagram_abi() {
        assert_eq!(super::AT_FDCWD, -100);
        assert_eq!(super::AT_SYMLINK_NOFOLLOW, 0x100);
        assert_eq!(super::SEEK_SET, 0);
        assert_eq!(super::SEEK_CUR, 1);
        assert_eq!(super::SEEK_END, 2);
    }

    #[test]
    fn dirent_matches_octagram_c_abi() {
        assert_eq!(core::mem::size_of::<super::Dirent>(), 280);
        assert_eq!(core::mem::align_of::<super::Dirent>(), 8);
        assert_eq!(core::mem::offset_of!(super::Dirent, d_ino), 0);
        assert_eq!(core::mem::offset_of!(super::Dirent, d_off), 8);
        assert_eq!(core::mem::offset_of!(super::Dirent, d_reclen), 16);
        assert_eq!(core::mem::offset_of!(super::Dirent, d_type), 18);
        assert_eq!(core::mem::offset_of!(super::Dirent, d_name), 19);
    }

    #[test]
    fn file_stat_exposes_posix_file_type_and_permissions() {
        let file = super::FileStat { size: 42, mode: super::S_IFREG | 0o755, modified_seconds: 7 };
        assert!(file.is_file());
        assert!(!file.is_directory());
        assert_eq!(file.file_type(), super::S_IFREG);
        assert_eq!(file.permissions(), 0o755);

        let directory = super::FileStat { mode: super::S_IFDIR | 0o750, ..file };
        assert!(directory.is_directory());
        assert!(!directory.is_file());

        let symlink = super::FileStat { mode: super::S_IFLNK | 0o777, ..file };
        assert!(symlink.is_symlink());
    }

    #[test]
    fn dirent_type_helpers_match_c_header_values() {
        let entry = Dirent {
            d_ino: 1,
            d_off: 0,
            d_reclen: core::mem::size_of::<Dirent>() as u16,
            d_type: super::DT_DIR,
            d_name: [0; 256],
        };
        assert!(entry.is_directory());
        assert!(!entry.is_file());
        assert!(!entry.is_symlink());
        assert_eq!(super::DT_REG, 8);
        assert_eq!(super::DT_LNK, 10);
    }

    #[test]
    fn dirent_name_bytes_respects_record_length_and_terminator() {
        let mut entry = Dirent {
            d_ino: 1,
            d_off: 2,
            d_reclen: core::mem::size_of::<Dirent>() as u16,
            d_type: 8,
            d_name: [0; 256],
        };
        for (slot, byte) in entry.d_name.iter_mut().zip(b"message.c") {
            *slot = *byte as _;
        }
        assert_eq!(entry.name_bytes(), Some(&b"message.c"[..]));

        entry.d_reclen = (core::mem::offset_of!(Dirent, d_name) + 4) as u16;
        assert_eq!(entry.name_bytes(), None);

        entry.d_reclen = (core::mem::offset_of!(Dirent, d_name) + 3) as u16;
        for (slot, byte) in entry.d_name.iter_mut().zip(b"abc") {
            *slot = *byte as _;
        }
        assert_eq!(entry.name_bytes(), None);

        entry.d_reclen = (core::mem::size_of::<Dirent>() + 1) as u16;
        assert_eq!(entry.name_bytes(), None);
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
