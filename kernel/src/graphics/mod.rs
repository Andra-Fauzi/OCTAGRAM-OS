//! Graphics subsystem: framebuffer singleton dan operasi pixel dasar.
//!
//! # Cara pakai
//! ```ignore
//! use crate::graphics::{Framebuffer, Color};
//!
//! if let Some(fb) = Framebuffer::get() {
//!     fb.write_pixel(100, 100, Color::WHITE);
//! }
//! ```

pub mod color;
pub use color::Color;

use limine::framebuffer::Framebuffer as LimineFramebuffer;
use limine::request::FramebufferRequest;

/// Limine request untuk framebuffer — hanya boleh ada satu instance.
#[used]
#[unsafe(link_section = ".requests")]
pub(crate) static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

/// Handle ke framebuffer utama yang dipakai kernel.
///
/// Dapatkan instance dengan [`Framebuffer::get()`]. Struct ini hanya
/// menyimpan referensi; tidak ada ownership transfer.
pub struct Framebuffer {
    inner: LimineFramebuffer<'static>,
}

impl Framebuffer {
    /// Dapatkan handle ke framebuffer pertama yang dilaporkan Limine.
    ///
    /// Mengembalikan `None` kalau Limine tidak menyediakan framebuffer
    /// (tidak seharusnya terjadi pada hardware/QEMU normal).
    pub fn get() -> Option<Self> {
        let resp = FRAMEBUFFER_REQUEST.get_response()?;
        let fb = resp.framebuffers().next()?;
        Some(Self { inner: fb })
    }

    /// Lebar framebuffer dalam pixel.
    #[inline]
    pub fn width(&self) -> u64 {
        self.inner.width()
    }
    /// Tinggi framebuffer dalam pixel.
    #[inline]
    pub fn height(&self) -> u64 {
        self.inner.height()
    }
    /// Pitch (bytes per scanline).
    #[inline]
    pub fn pitch(&self) -> u64 {
        self.inner.pitch()
    }
    /// Pointer ke byte pertama framebuffer (writable).
    #[inline]
    pub fn addr(&self) -> *mut u8 {
        self.inner.addr()
    }

    /// Tulis satu pixel di koordinat (x, y) dengan warna `color`.
    ///
    /// Tidak melakukan bounds check — pastikan x < width dan y < height.
    ///
    /// # Safety
    /// Caller harus memastikan (x, y) berada dalam bounds framebuffer.
    pub unsafe fn write_pixel_unchecked(&self, x: u64, y: u64, color: Color) {
        let off = (y * self.pitch() + x * 4) as usize;
        // Safety: caller menjamin in-bounds.
        unsafe {
            (self.addr().add(off) as *mut u32).write_volatile(color.as_u32());
        }
    }

    /// Tulis satu pixel di koordinat (x, y). Bounds-checked; no-op kalau di luar.
    pub fn write_pixel(&self, x: u64, y: u64, color: Color) {
        if x < self.width() && y < self.height() {
            // Safety: bounds sudah dicek.
            unsafe { self.write_pixel_unchecked(x, y, color) };
        }
    }

    /// Isi rectangle [x0..x1) x [y0..y1) dengan warna solid.
    pub fn fill_rect(&self, x0: u64, y0: u64, x1: u64, y1: u64, color: Color) {
        let x1 = x1.min(self.width());
        let y1 = y1.min(self.height());
        for y in y0..y1 {
            for x in x0..x1 {
                // Safety: dibatasi oleh min() di atas.
                unsafe { self.write_pixel_unchecked(x, y, color) };
            }
        }
    }

    /// Clear seluruh layar dengan warna hitam.
    pub fn clear(&self) {
        // Safety: addr valid dan ukurannya pitch * height.
        unsafe {
            core::ptr::write_bytes(self.addr(), 0, (self.pitch() * self.height()) as usize);
        }
    }
}
