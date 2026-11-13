//! Alokasi page frame fisik 4 KiB via linked free-list.
//!
//! [`FrameAllocator`] di-build lazy dari memory map Limine pada panggilan
//! [`allocate`][FrameAllocator::allocate] pertama. Setelah itu, alokasi
//! adalah O(1) pop dari head linked list, dan deallokasi adalah O(1) push.
//!
//! **Thread safety**: struct ini tidak thread-safe sendiri; bungkus dengan
//! [`spin::Mutex`] sebelum dipakai dari banyak konteks (lihat
//! [`memory::FRAME_ALLOCATOR`]).

use crate::memory::{HHDM_REQUEST, MEMORY_MAP_REQUEST};
use core::ptr;
use limine::memory_map::EntryType;

const FRAME_SIZE: usize = 4096;

/// Sentinel "next = None" di linked list.
const NULL_MARK: usize = usize::MAX;

/// Alokator frame fisik berbasis linked free-list.
///
/// Setiap frame kosong menyimpan pointer "next" di byte pertamanya
/// (lewat HHDM). Ini tidak memerlukan alokasi metadata terpisah —
/// metadata-nya ada di dalam frame itu sendiri.
pub struct FrameAllocator {
    head: Option<usize>,
    initialized: bool,
}

impl FrameAllocator {
    /// Buat instance baru (belum diinisialisasi — lazy init).
    pub const fn new() -> Self {
        Self {
            head: None,
            initialized: false,
        }
    }

    /// Build free-list dari Limine memory map (dipanggil otomatis sekali).
    fn init(&mut self) {
        let Some(response) = MEMORY_MAP_REQUEST.get_response() else {
            crate::println!("[frame_alloc] PERINGATAN: memory map tidak tersedia");
            self.initialized = true;
            return;
        };

        for entry in response.entries() {
            if entry.entry_type == EntryType::USABLE {
                let base = align_up(entry.base as usize, FRAME_SIZE);
                let end = (entry.base + entry.length) as usize;
                let mut addr = base;
                while addr + FRAME_SIZE <= end {
                    self.push_free(addr);
                    addr += FRAME_SIZE;
                }
            }
        }

        self.initialized = true;
    }

    /// Tambahkan frame (alamat fisik) ke head free-list.
    fn push_free(&mut self, frame: usize) {
        let virt = physical_to_virtual_internal(frame).expect("push_free: HHDM belum tersedia");
        // Safety: virt adalah alamat valid di HHDM, frame belum dipakai.
        unsafe {
            ptr::write(virt as *mut usize, self.head.unwrap_or(NULL_MARK));
        }
        self.head = Some(frame);
    }

    /// Alokasi satu frame fisik (4 KiB), sudah di-zero.
    ///
    /// Mengembalikan **alamat fisik**, atau `None` kalau free-list kosong.
    pub fn allocate(&mut self) -> Option<usize> {
        if !self.initialized {
            self.init();
        }

        let frame = self.head?;
        let virt = physical_to_virtual_internal(frame)?;

        // Baca "next" dari dalam frame, jadikan head baru.
        let next = unsafe { ptr::read(virt as *const usize) };
        self.head = if next == NULL_MARK { None } else { Some(next) };

        // Zero-kan frame sebelum dikembalikan.
        unsafe { ptr::write_bytes(virt as *mut u8, 0, FRAME_SIZE) };

        Some(frame)
    }

    /// Bebaskan satu frame fisik (dikembalikan ke free-list).
    ///
    /// `frame` harus **alamat fisik** yang valid — memanggil ini dengan
    /// alamat sembarang akan merusak free-list.
    pub fn free(&mut self, frame: usize) {
        // Zero dulu sebelum di-push (hygiene + keamanan).
        if let Some(virt) = physical_to_virtual_internal(frame) {
            unsafe { ptr::write_bytes(virt as *mut u8, 0, FRAME_SIZE) };
        }
        self.push_free(frame);
    }
}

// Helper internal agar tidak circular-import dengan memory::mod
fn physical_to_virtual_internal(phys: usize) -> Option<usize> {
    let offset = HHDM_REQUEST.get_response()?.offset() as usize;
    Some(phys + offset)
}

#[inline]
fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}
