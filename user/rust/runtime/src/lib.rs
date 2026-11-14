#![no_std]

use core::alloc::{GlobalAlloc, Layout};
use core::ffi::{c_char, CStr};
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

pub const STDIN: u64 = 0;
pub const STDOUT: u64 = 1;
pub const STDERR: u64 = 2;

pub const O_RDONLY: u64 = 0;
pub const O_WRONLY: u64 = 1;
pub const O_RDWR: u64 = 2;
pub const O_CREAT: u64 = 1 << 8;
pub const O_TRUNC: u64 = 1 << 9;
pub const O_APPEND: u64 = 1 << 10;

#[no_mangle]
pub static mut environ: *mut *mut c_char = ptr::null_mut();

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
    let result = unsafe { syscall2(3, path.as_ptr() as usize as u64, flags) };
    if result < 0 {
        Err(result)
    } else {
        Ok(result as u64)
    }
}

pub fn getcwd(buffer: &mut [u8]) -> Result<usize, i64> {
    let result = unsafe { syscall2(14, buffer.as_mut_ptr() as usize as u64, buffer.len() as u64) };
    checked_result(result)
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

pub fn brk(address: usize) -> Result<usize, i64> {
    let result = unsafe { syscall1(10, address as u64) };
    checked_result(result)
}

pub fn mmap(address: usize, length: usize, protection: u64) -> Result<*mut u8, i64> {
    let result = unsafe { syscall3(16, address as u64, length as u64, protection) };
    if result < 0 {
        Err(result)
    } else {
        Ok(result as usize as *mut u8)
    }
}

pub fn munmap(address: *mut u8, length: usize) -> Result<(), i64> {
    let result = unsafe { syscall2(17, address as usize as u64, length as u64) };
    if result < 0 {
        Err(result)
    } else {
        Ok(())
    }
}

pub fn mmap_file(fd: u64, offset: u64, length: usize) -> Result<*const u8, i64> {
    let result = unsafe { syscall3(18, fd, offset, length as u64) };
    if result < 0 {
        Err(result)
    } else {
        Ok(result as usize as *const u8)
    }
}

pub fn spawn(path: &CStr, argv: &[*const c_char]) -> Result<u64, i64> {
    let result = unsafe {
        syscall3(
            19,
            path.as_ptr() as usize as u64,
            argv.as_ptr() as usize as u64,
            argv.len() as u64,
        )
    };
    if result < 0 {
        Err(result)
    } else {
        Ok(result as u64)
    }
}

pub fn waitpid(pid: u64, status: Option<&mut u64>) -> Result<u64, i64> {
    let status_pointer = status.map_or(0, |value| value as *mut u64 as usize as u64);
    let result = unsafe { syscall2(20, pid, status_pointer) };
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

struct HeapGuard;

static HEAP_LOCKED: AtomicBool = AtomicBool::new(false);

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

impl Drop for HeapGuard {
    fn drop(&mut self) {
        HEAP_LOCKED.store(false, Ordering::Release);
    }
}

struct BrkAllocator;

unsafe impl GlobalAlloc for BrkAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() == 0 {
            return layout.align() as *mut u8;
        }

        let _guard = HeapGuard::acquire();
        let Ok(current) = brk(0) else {
            return ptr::null_mut();
        };
        let Some(start) = current
            .checked_add(layout.align() - 1)
            .map(|value| value & !(layout.align() - 1))
        else {
            return ptr::null_mut();
        };
        let Some(end) = start.checked_add(layout.size()) else {
            return ptr::null_mut();
        };
        match brk(end) {
            Ok(actual) if actual == end => start as *mut u8,
            _ => ptr::null_mut(),
        }
    }

    unsafe fn dealloc(&self, _pointer: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOCATOR: BrkAllocator = BrkAllocator;
