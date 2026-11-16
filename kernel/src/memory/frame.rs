//! Alokasi page frame fisik 4 KiB via linked free-list.
//!
//! [`FrameAllocator`] di-build lazy dari memory map Limine pada panggilan
//! alokasi pertama. Frame di bawah 4 GiB dan frame lainnya disimpan dalam
//! free-list terpisah agar alokasi DMA32 tetap O(1).
//!
//! **Thread safety**: struct ini tidak thread-safe sendiri; bungkus dengan
//! [`spin::Mutex`] sebelum dipakai dari banyak konteks (lihat
//! [`memory::FRAME_ALLOCATOR`]).

use crate::memory::{HHDM_REQUEST, MEMORY_MAP_REQUEST};
use core::ptr;
use limine::memory_map::EntryType;

const FRAME_SIZE: usize = 4096;
const DMA32_LIMIT: usize = 1 << 32;

/// Penanda "frame ini sedang ada di free-list", disimpan di offset 8 frame
/// kosong (offset 0 dipakai untuk pointer `next`). `allocate` meng-zero-kan
/// frame sebelum diserahkan ke pemanggil, jadi penanda ini hilang begitu
/// frame dipakai.
const FREE_MAGIC: usize = 0xF4EE_F0E5_F4EE_F0E5;

/// Sentinel "next = None" di linked list.
const NULL_MARK: usize = usize::MAX;

/// Alokator frame fisik berbasis linked free-list.
///
/// Setiap frame kosong menyimpan pointer "next" di byte pertamanya
/// (lewat HHDM). Ini tidak memerlukan alokasi metadata terpisah —
/// metadata-nya ada di dalam frame itu sendiri.
pub struct FrameAllocator {
    low_head: Option<usize>,
    high_head: Option<usize>,
    initialized: bool,
}

impl FrameAllocator {
    /// Buat instance baru (belum diinisialisasi — lazy init).
    pub const fn new() -> Self {
        Self {
            low_head: None,
            high_head: None,
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

    /// Tambahkan frame (alamat fisik) ke free-list dengan batas alamat sesuai.
    fn push_free(&mut self, frame: usize) {
        let virt = physical_to_virtual_internal(frame).expect("push_free: HHDM belum tersedia");
        let head = if is_dma32_frame(frame) {
            &mut self.low_head
        } else {
            &mut self.high_head
        };
        // Safety: virt adalah alamat valid di HHDM, frame belum dipakai.
        unsafe {
            ptr::write(virt as *mut usize, head.unwrap_or(NULL_MARK));
            ptr::write((virt as *mut usize).add(1), FREE_MAGIC);
        }
        *head = Some(frame);
    }

    /// Alokasi satu frame fisik (4 KiB), sudah di-zero.
    ///
    /// Mengembalikan **alamat fisik**, atau `None` kalau free-list kosong.
    pub fn allocate(&mut self) -> Option<usize> {
        if !self.initialized {
            self.init();
        }

        Self::pop(&mut self.high_head).or_else(|| Self::pop(&mut self.low_head))
    }

    /// Alokasi satu page frame yang seluruhnya dapat dialamati dengan DMA 32-bit.
    pub fn allocate_dma32(&mut self) -> Option<usize> {
        if !self.initialized {
            self.init();
        }

        Self::pop(&mut self.low_head)
    }

    /// Bebaskan satu frame fisik (dikembalikan ke free-list).
    ///
    /// Frame yang tidak valid — alamat 0, tidak ter-align `FRAME_SIZE`, di
    /// luar semua region `USABLE`, atau **sudah ada di free-list** (double
    /// free) — diabaikan. Tanpa cek ini, double free menyisipkan node yang
    /// sama dua kali sehingga satu frame bisa diserahkan ke dua pemilik
    /// sekaligus (korupsi memori yang sulit dilacak).
    pub fn free(&mut self, frame: usize) {
        if frame == 0 || frame % FRAME_SIZE != 0 || !self.is_usable_frame(frame) {
            return;
        }
        let Some(virt) = physical_to_virtual_internal(frame) else {
            return;
        };
        // Safety: virt di HHDM; offset 8 frame kosong menyimpan penanda
        // free-list, dan penanda hanya ditulis di `push_free`.
        let already_free = unsafe { ptr::read((virt as *const usize).add(1)) } == FREE_MAGIC;
        if already_free {
            return;
        }
        // Zero dulu sebelum di-push (hygiene + keamanan).
        unsafe { ptr::write_bytes(virt as *mut u8, 0, FRAME_SIZE) };
        self.push_free(frame);
    }

    /// Apakah `frame` seluruhnya berada di dalam region `USABLE` memory map.
    fn is_usable_frame(&self, frame: usize) -> bool {
        let Some(response) = MEMORY_MAP_REQUEST.get_response() else {
            // Tanpa memory map kita tidak bisa memvalidasi apa pun;
            // pertahankan perilaku lama supaya pembebasan tidak gagal semua.
            return true;
        };
        let Some(end) = frame.checked_add(FRAME_SIZE) else {
            return false;
        };
        response.entries().iter().any(|entry| {
            entry.entry_type == EntryType::USABLE
                && frame >= entry.base as usize
                && end <= (entry.base as usize).saturating_add(entry.length as usize)
        })
    }

    fn pop(head: &mut Option<usize>) -> Option<usize> {
        let frame = (*head)?;
        let virt = physical_to_virtual_internal(frame)?;

        // Safety: free-list node contains the next physical frame or sentinel.
        let next = unsafe { ptr::read(virt as *const usize) };
        *head = if next == NULL_MARK { None } else { Some(next) };

        // Safety: the popped frame is exclusively owned by the caller.
        unsafe { ptr::write_bytes(virt as *mut u8, 0, FRAME_SIZE) };

        Some(frame)
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

const fn is_dma32_frame(frame: usize) -> bool {
    frame < DMA32_LIMIT
}

#[cfg(test)]
mod tests {
    use super::{DMA32_LIMIT, is_dma32_frame};

    #[test]
    fn classifies_frames_by_their_full_dma_address_range() {
        assert!(is_dma32_frame(DMA32_LIMIT - 4096));
        assert!(!is_dma32_frame(DMA32_LIMIT));
    }
}
