//! Memory subsystem: frame allocator, heap, dan address translation.
//!
//! # Urutan inisialisasi (WAJIB)
//! 1. [`init()`] — panggil SEKALI saat boot, sebelum kode lain memakai `alloc::`.
//!    Fungsi ini build free-list frame allocator dari memory map Limine,
//!    lalu inisialisasi heap global allocator.
//!
//! # Subsystem
//! - [`frame`] — [`FrameAllocator`]: alokasi/deallokasi page frame 4KB.
//! - [`heap`]  — [`init_heap`]: setup `#[global_allocator]` (linked_list_allocator).
//! - [`paging`] — map/unmap halaman virtual, helper MMIO.
//!
//! # Contoh
//! ```ignore
//! // Di main.rs, setelah GDT/IDT/APIC ready:
//! memory::init();
//!
//! // Alokasi satu frame fisik:
//! let frame_phys = memory::allocate_frame().expect("OOM");
//! let frame_virt = memory::physical_to_virtual(frame_phys).unwrap();
//! ```

use limine::memory_map::EntryType;
use limine::request::{HhdmRequest, MemoryMapRequest};
use spin::Mutex;

mod frame;
mod heap;
pub mod paging;

use frame::FrameAllocator;

// --- Limine requests ---

#[used]
#[unsafe(link_section = ".requests")]
static MEMORY_MAP_REQUEST: MemoryMapRequest = MemoryMapRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

// --- Global frame allocator ---

/// Frame allocator global. Dibungkus Mutex supaya bisa dipakai dari mana saja.
static FRAME_ALLOCATOR: Mutex<FrameAllocator> = Mutex::new(FrameAllocator::new());

// --- Public API ---

/// Konversi alamat fisik ke alamat virtual lewat HHDM offset.
///
/// Mengembalikan `None` kalau Limine tidak menyediakan HHDM (tidak seharusnya
/// terjadi pada hardware/QEMU normal setelah boot).
pub fn physical_to_virtual(phys_addr: usize) -> Option<usize> {
    let offset = HHDM_REQUEST.get_response()?.offset() as usize;
    Some(phys_addr + offset)
}

/// Konversi alamat virtual yang sedang terpetakan ke alamat fisik.
///
/// Berbeda dari [`physical_to_virtual`], fungsi ini juga bekerja untuk
/// alamat kernel seperti heap dan stack yang bukan bagian dari HHDM.
pub fn virtual_to_physical(virt_addr: usize) -> Option<usize> {
    paging::translate_address(virt_addr as u64)
        .ok()
        .map(|phys_addr| phys_addr as usize)
}

/// Inisialisasi memory subsystem.
///
/// Panggil tepat SEKALI saat boot, setelah Limine requests siap dan
/// sebelum kode lain memakai `alloc::` (Box, Vec, String, dll).
///
/// # Panics
/// Panic kalau heap gagal diinisialisasi (frame fisik tidak cukup).
pub fn init() {
    heap::init_heap();
    crate::println!("[memory] heap OK");
}

/// Alokasi satu page frame fisik (4 KiB), sudah di-zero.
///
/// Mengembalikan **alamat fisik** frame, atau `None` kalau OOM.
pub fn allocate_frame() -> Option<usize> {
    FRAME_ALLOCATOR.lock().allocate()
}

/// Alokasi satu page frame fisik di bawah batas 4 GiB untuk perangkat DMA 32-bit.
///
/// Mengembalikan **alamat fisik** frame yang seluruh rentang 4 KiB-nya dapat
/// direpresentasikan sebagai alamat DMA 32-bit, atau `None` jika tidak tersedia.
pub fn allocate_dma32_frame() -> Option<usize> {
    FRAME_ALLOCATOR.lock().allocate_dma32()
}

/// Bebaskan satu page frame fisik.
///
/// `frame` harus **alamat fisik** yang sebelumnya dikembalikan oleh
/// [`allocate_frame`]. Memanggil ini dengan alamat sembarang adalah UB.
pub fn free_frame(frame: usize) {
    FRAME_ALLOCATOR.lock().free(frame);
}

/// Jumlah frame usable yang dilaporkan Limine (berguna untuk info saat boot).
pub fn total_usable_frames() -> u64 {
    MEMORY_MAP_REQUEST
        .get_response()
        .map(|r| {
            r.entries()
                .iter()
                .filter(|e| e.entry_type == EntryType::USABLE)
                .map(|e| e.length / 4096)
                .sum()
        })
        .unwrap_or(0)
}
