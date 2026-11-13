//! Tipe warna dan konstanta warna umum untuk graphics subsystem.

/// Warna dalam format 32-bit packed: 0x00RRGGBB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub u32);

impl Color {
    /// Buat warna dari komponen RGB (masing-masing 0-255).
    #[inline]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Color(((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
    }

    /// Nilai raw 32-bit untuk ditulis langsung ke framebuffer.
    #[inline]
    pub const fn as_u32(self) -> u32 {
        self.0
    }

    // --- Konstanta warna umum ---
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    pub const RED: Color = Color::rgb(255, 0, 0);
    pub const GREEN: Color = Color::rgb(0, 255, 0);
    pub const BLUE: Color = Color::rgb(0, 0, 255);
    pub const YELLOW: Color = Color::rgb(255, 255, 0);
    pub const CYAN: Color = Color::rgb(0, 255, 255);
    pub const MAGENTA: Color = Color::rgb(255, 0, 255);
    pub const ORANGE: Color = Color::rgb(255, 165, 0);
    pub const GRAY: Color = Color::rgb(128, 128, 128);
}
