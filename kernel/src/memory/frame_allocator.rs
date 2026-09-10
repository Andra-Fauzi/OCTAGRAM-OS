use crate::{
    memory::{MEMORY_MAP_REQUEST, MemoryMapRequest},
    print,
};
use core::ptr;
use limine::memory_map::EntryType;
use spin::mutex::Mutex;

const FRAME_SIZE: usize = 4096;
const NULL_MARK: usize = usize::MAX; // penanda "next = None"

pub struct FrameAllocator {
    head: Option<usize>,
    initialized: bool,
}

impl FrameAllocator {
    pub const fn new() -> Self {
        Self {
            head: None,
            initialized: false,
        }
    }

    /// Bangun free-list dari memory map (dipanggil sekali, lazy)
    fn init(&mut self) {
        if let Some(response) = MEMORY_MAP_REQUEST.get_response() {
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
        } else {
            print!("MEMORY MAP TIDAK MERESPONSE");
        }
        self.initialized = true;
    }

    /// Tambahkan satu frame (alamat fisik) ke depan linked list (jadi head baru).
    /// Penulisan pointer "next" dilakukan lewat alamat virtual HHDM, karena
    /// alamat fisik mentah belum tentu bisa diakses langsung.
    fn push_free(&mut self, frame: usize) {
        let virt = super::physical_to_virtual(frame).expect("HHDM belum tersedia saat push_free");
        unsafe {
            let node_ptr = virt as *mut usize;
            ptr::write(node_ptr, self.head.unwrap_or(NULL_MARK));
        }
        self.head = Some(frame);
    }

    /// Mengembalikan alamat FISIK dari satu frame kosong yang sudah dizero-kan.
    pub fn allocate(&mut self) -> Option<usize> {
        if !self.initialized {
            self.init();
        }

        let frame = self.head?;
        let virt = super::physical_to_virtual(frame)?;

        // Baca "next" yang tersimpan di dalam frame, lalu jadikan head baru
        let next = unsafe { ptr::read(virt as *const usize) };
        self.head = if next == NULL_MARK { None } else { Some(next) };

        unsafe {
            ptr::write_bytes(virt as *mut u8, 0, FRAME_SIZE);
        }

        Some(frame)
    }

    /// Menerima alamat FISIK frame yang ingin dibebaskan.
    pub fn free(&mut self, frame: usize) {
        if let Some(virt) = super::physical_to_virtual(frame) {
            unsafe {
                ptr::write_bytes(virt as *mut u8, 0, FRAME_SIZE);
            }
        }
        self.push_free(frame);
    }
}

fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}
