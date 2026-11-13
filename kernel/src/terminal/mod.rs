//! Terminal berbasis framebuffer: render teks, scroll, kursor.
//!
//! Dua macro publik yang dipakai di seluruh kernel:
//! - [`print!`] — tulis string ke terminal tanpa newline.
//! - [`println!`] — tulis string ke terminal lalu newline.
//!
//! Terminal tidak punya state threading yang complex; cursor position
//! dijaga lewat [`CURSOR`] Mutex sehingga aman dari data race.

use crate::graphics::Framebuffer;
use crate::terminal::font::FONT8X16;
use spin::Mutex;

pub mod font;

use core::fmt::{self, Write};

/// Posisi kursor saat ini: (x, y) dalam satuan pixel.
static CURSOR: Mutex<(u64, u64)> = Mutex::new((0, 0));

/// Font dimensions.
const CHAR_W: u64 = 8;
const CHAR_H: u64 = 16;

/// Zero-argument helper supaya `Write` impl bisa dibuat tanpa instance
/// (terminal adalah stateless struct, state-nya di [`CURSOR`]).
pub struct Terminal;

impl Write for Terminal {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        terminal_print(s);
        Ok(())
    }
}

/// Tulis string ke terminal framebuffer.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        let mut t = $crate::terminal::Terminal;
        t.write_fmt(format_args!($($arg)*)).unwrap();
    }};
}

/// Tulis string ke terminal framebuffer, diakhiri newline.
#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        let mut t = $crate::terminal::Terminal;
        t.write_fmt(format_args!($($arg)*)).unwrap();
        t.write_fmt(format_args!("\n")).unwrap();
    }};
}

/// Geser seluruh isi framebuffer ke atas sebanyak `line_height` pixel.
/// Baris paling bawah yang dikosongkan akan di-clear ke hitam.
///
/// # Safety
/// `addr` harus pointer valid ke buffer framebuffer yang bisa ditulis.
unsafe fn scroll_up(addr: *mut u8, pitch: u64, height: u64, line_height: u64) {
    // Safety: caller menjamin addr valid.
    unsafe {
        let pitch = pitch as usize;
        let height = height as usize;
        let lh = line_height as usize;
        let scroll_bytes = pitch * lh;
        let total_bytes = pitch * height;
        core::ptr::copy(addr.add(scroll_bytes), addr, total_bytes - scroll_bytes);
        core::ptr::write_bytes(addr.add(total_bytes - scroll_bytes), 0, scroll_bytes);
    }
}

/// Reset posisi kursor dan clear layar.
pub fn terminal_clear() {
    if let Some(fb) = Framebuffer::get() {
        unsafe {
            core::ptr::write_bytes(fb.addr(), 0, (fb.pitch() * fb.height()) as usize);
        }
    }
    *CURSOR.lock() = (0, 0);
}

/// Erase the character immediately before the terminal cursor.
pub fn terminal_backspace() {
    let Some(framebuffer) = Framebuffer::get() else {
        return;
    };
    let mut cursor = CURSOR.lock();
    if cursor.0 >= CHAR_W {
        cursor.0 -= CHAR_W;
    } else if cursor.1 >= CHAR_H {
        cursor.1 -= CHAR_H;
        let columns = framebuffer.width() / CHAR_W;
        if columns == 0 {
            return;
        }
        cursor.0 = (columns - 1) * CHAR_W;
    } else {
        return;
    }

    framebuffer.fill_rect(
        cursor.0,
        cursor.1,
        cursor.0 + CHAR_W,
        cursor.1 + CHAR_H,
        crate::graphics::Color::BLACK,
    );
}

/// Render satu string ke terminal. Semua kontrol karakter yang didukung: `\n`.
pub fn terminal_print(s: &str) {
    let fb = match Framebuffer::get() {
        Some(f) => f,
        None => return,
    };

    let (fb_w, fb_h) = (fb.width(), fb.height());
    let pitch = fb.pitch();
    let addr = fb.addr();

    let mut cur = CURSOR.lock();

    for ch in s.chars() {
        if ch == '\n' {
            cur.1 += CHAR_H;
            cur.0 = 0;
            if cur.1 + CHAR_H > fb_h {
                // Safety: addr valid, kita punya gambar framebuffer.
                unsafe { scroll_up(addr, pitch, fb_h, CHAR_H) };
                cur.1 -= CHAR_H;
            }
            continue;
        }

        // Word-wrap otomatis
        if cur.0 + CHAR_W > fb_w {
            cur.1 += CHAR_H;
            cur.0 = 0;
            if cur.1 + CHAR_H > fb_h {
                unsafe { scroll_up(addr, pitch, fb_h, CHAR_H) };
                cur.1 -= CHAR_H;
            }
        }

        let idx = ch as usize;
        if idx < 256 {
            let glyph = &FONT8X16[idx];
            for row in 0..CHAR_H as usize {
                let bits = glyph[row];
                for col in 0..CHAR_W as usize {
                    if bits & (0x80 >> col) != 0 {
                        let px = cur.0 + col as u64;
                        let py = cur.1 + row as u64;
                        let off = (py * pitch + px * 4) as usize;
                        // Safety: bounds dijaga oleh scroll_up dan word-wrap di atas.
                        unsafe {
                            (addr.add(off) as *mut u32).write_volatile(0x00FF_FFFF);
                        }
                    }
                }
            }
        }

        cur.0 += CHAR_W;
    }
}
