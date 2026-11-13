//! Tipe error terpusat untuk seluruh kernel OCTAGRAM-OS.
//!
//! Semua subsystem (`memory`, `drivers`, `arch`) harus return
//! `KernelResult<T>` ketika bisa gagal, bukan langsung `panic!`.
//! Ini memudahkan error propagation dan future recovery logic.

/// Error yang bisa terjadi di berbagai subsystem kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelError {
    // --- Memory ---
    /// Frame allocator kehabisan frame fisik.
    OutOfMemory,
    /// HHDM offset dari Limine belum tersedia saat dibutuhkan.
    HhdmUnavailable,
    /// Memory map dari Limine tidak tersedia.
    MemoryMapUnavailable,

    // --- Graphics ---
    /// Framebuffer dari Limine tidak tersedia.
    FramebufferUnavailable,

    // --- PCI ---
    /// Device PCI dengan class/vendor yang dicari tidak ditemukan.
    PciDeviceNotFound,

    // --- USB/xHCI ---
    /// Inisialisasi xHCI controller gagal.
    XhciInitFailed,
    /// MSI-X setup gagal di controller xHCI.
    XhciMsiXFailed,
    /// Slot USB habis atau enable_slot gagal.
    XhciSlotFailed,

    // --- ACPI ---
    /// RSDP dari Limine tidak tersedia.
    RsdpUnavailable,
    /// Table ACPI yang dicari tidak ditemukan (mis. MADT).
    AcpiTableNotFound,

    // --- Generic ---
    /// Operasi tidak didukung di arsitektur ini.
    NotSupported,
    /// Nilai parameter di luar range yang valid.
    InvalidArgument,
}

impl core::fmt::Display for KernelError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            KernelError::OutOfMemory        => write!(f, "out of physical memory"),
            KernelError::HhdmUnavailable    => write!(f, "HHDM offset not available"),
            KernelError::MemoryMapUnavailable => write!(f, "memory map not available"),
            KernelError::FramebufferUnavailable => write!(f, "framebuffer not available"),
            KernelError::PciDeviceNotFound  => write!(f, "PCI device not found"),
            KernelError::XhciInitFailed     => write!(f, "xHCI controller init failed"),
            KernelError::XhciMsiXFailed     => write!(f, "xHCI MSI-X setup failed"),
            KernelError::XhciSlotFailed     => write!(f, "xHCI slot enable failed"),
            KernelError::RsdpUnavailable    => write!(f, "RSDP not available"),
            KernelError::AcpiTableNotFound  => write!(f, "ACPI table not found"),
            KernelError::NotSupported       => write!(f, "not supported"),
            KernelError::InvalidArgument    => write!(f, "invalid argument"),
        }
    }
}

/// Alias Result yang dipakai di seluruh kernel.
pub type KernelResult<T> = Result<T, KernelError>;
