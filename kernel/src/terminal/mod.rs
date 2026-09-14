use crate::FRAMEBUFFER_REQUEST;
use crate::graphics::write_pixel;
use crate::terminal::font::FONT8X16;

pub mod font;

use core::fmt::{self, Write};

pub struct Terminal;

impl Write for Terminal {
    fn write_str(&mut self, string: &str) -> fmt::Result {
        terminal_print(string);
        Ok(())
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        use core::fmt::Write;

        let mut terminal = crate::terminal::Terminal;
        terminal.write_fmt(format_args!($($arg)*)).unwrap();
    }};
}

#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => {{
        use core::fmt::Write;

        let mut terminal = crate::terminal::Terminal;
        terminal.write_fmt(format_args!($($arg)*)).unwrap();
        terminal.write_fmt(format_args!("\n")).unwrap();
    }};
}

/// Geser seluruh isi framebuffer ke atas sebanyak `line_height` baris pixel,
/// lalu bersihkan (hitamkan) baris paling bawah yang baru kosong.
/// Ini yang bikin efek "scroll" pas teks sampai baris terakhir layar.
unsafe fn scroll_framebuffer(addr: *mut u8, pitch: u64, height: u64, line_height: u64) {
    unsafe {
        let pitch = pitch as usize;
        let height = height as usize;
        let line_height = line_height as usize;

        let scroll_bytes = pitch * line_height;
        let total_bytes = pitch * height;

        // Geser semua baris ke atas: baris ke-N jadi baris ke-(N - line_height)
        core::ptr::copy(addr.add(scroll_bytes), addr, total_bytes - scroll_bytes);

        // Bersihkan baris-baris paling bawah yang sekarang "duplikat" dari sebelumnya
        core::ptr::write_bytes(addr.add(total_bytes - scroll_bytes), 0, scroll_bytes);
    }
}

pub fn terminal_clear() {
    if let Some(framebuffer_response) = FRAMEBUFFER_REQUEST.get_response() {
        if let Some(framebuffer) = framebuffer_response.framebuffers().next() {
            unsafe {
                let height = framebuffer.height();
                let width = framebuffer.width();
                write_pixel((0, 0), (width, height), (0, 0, 0));
            }
        }
    }
}

pub fn terminal_print(string: &str) {
    if let Some(framebuffer_response) = FRAMEBUFFER_REQUEST.get_response() {
        if let Some(framebuffer) = framebuffer_response.framebuffers().next() {
            static mut POSITION_X: u64 = 0;
            static mut POSITION_Y: u64 = 0;

            for char in string.chars() {
                if char == '\n' {
                    unsafe {
                        POSITION_Y += 16;
                        POSITION_X = 0;
                        if POSITION_Y + 16 > framebuffer.height() {
                            scroll_framebuffer(
                                framebuffer.addr(),
                                framebuffer.pitch(),
                                framebuffer.height(),
                                16,
                            );
                            POSITION_Y -= 16;
                        }
                    }
                    continue;
                }
                unsafe {
                    if POSITION_X + 16 > framebuffer.width() {
                        POSITION_Y += 16;
                        POSITION_X = 0;
                        if POSITION_Y + 16 > framebuffer.height() {
                            scroll_framebuffer(
                                framebuffer.addr(),
                                framebuffer.pitch(),
                                framebuffer.height(),
                                16,
                            );
                            POSITION_Y -= 16;
                        }
                    }
                }
                let index = char as usize;

                for line in 0..16 {
                    let bit = FONT8X16[index][line];

                    for i in 0..8 {
                        let mask = 0x80 >> i;

                        if (bit & mask) != 0 {
                            unsafe {
                                write_pixel(
                                    ((POSITION_X + i as u64), (POSITION_Y + line as u64)),
                                    ((POSITION_X + i as u64 + 1), (POSITION_Y + line as u64 + 1)),
                                    (255, 255, 255),
                                );
                            }
                        }
                    }
                }

                // Pindah ke karakter berikutnya
                unsafe {
                    POSITION_X += 8;
                }
            }
        }
    }
}
