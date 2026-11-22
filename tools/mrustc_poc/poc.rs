#![no_std]
#![allow(internal_features)]
#![feature(lang_items)]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[lang = "start"]
fn lang_start<T>(main: fn() -> T, _argc: isize, _argv: *const *const u8, _sigpipe: u8) -> isize {
    main();
    0
}

extern "C" {
    fn write(fd: i32, buffer: *const u8, length: usize) -> isize;
}

static TABLE: [u32; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
static MESSAGE: &[u8] = b"mrustc-c-poc: ";

fn emit(bytes: &[u8]) {
    unsafe {
        write(1, bytes.as_ptr(), bytes.len());
    }
}

fn emit_u32(mut value: u32) {
    let mut digits = [0u8; 10];
    let mut index = digits.len();
    loop {
        index -= 1;
        digits[index] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    emit(&digits[index..]);
}

fn main() -> i32 {
    let mut sum: u32 = 0;
    let mut i = 0usize;
    while i < TABLE.len() {
        sum += TABLE[i] * 3;
        i += 1;
    }
    emit(MESSAGE);
    emit_u32(sum);
    emit(b"\n");
    sum as i32
}
