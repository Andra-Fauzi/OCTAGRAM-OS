use limine::request::MemoryMapRequest;
use limine::{memory_map::EntryType, request::HhdmRequest};
use spin::Mutex;

use crate::{FRAMEBUFFER_REQUEST, print};

mod frame_allocator;
mod heap_allocator;
pub mod paging;

use frame_allocator::FrameAllocator;

#[used]
#[unsafe(link_section = ".requests")]
static MEMORY_MAP_REQUEST: MemoryMapRequest = MemoryMapRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

/// Frame allocator global, dibungkus Mutex supaya bisa dipakai dari mana saja
/// (mis. saat init heap, page mapping, dsb).
static FRAME_ALLOCATOR: Mutex<FrameAllocator> = Mutex::new(FrameAllocator::new());

pub fn physical_to_virtual(phys_addr: usize) -> Option<usize> {
    if let Some(response) = HHDM_REQUEST.get_response() {
        let offset = response.offset();

        let virt_addr = phys_addr + offset as usize;

        return Some(virt_addr);
    } else {
        return None;
    }
}

pub fn virtual_to_physical(virt_addr: usize) -> Option<usize> {
    if let Some(response) = HHDM_REQUEST.get_response() {
        let offset = response.offset();

        // BUG LAMA: sebelumnya ini menambahkan offset (sama seperti physical_to_virtual).
        // Yang benar: alamat virtual HHDM = alamat fisik + offset,
        // jadi untuk baliknya harus DIKURANGI offset.
        let phys_addr = virt_addr - offset as usize;

        return Some(phys_addr);
    } else {
        return None;
    }
}

fn cetak_entry_type(entry_type: EntryType) -> &'static str {
    // Karena EntryType mengimplementasikan PartialEq, kita bisa pakai match
    let nama_tipe = match entry_type {
        EntryType::USABLE => "USABLE",
        EntryType::RESERVED => "RESERVED",
        EntryType::ACPI_RECLAIMABLE => "ACPI_RECLAIMABLE",
        EntryType::ACPI_NVS => "ACPI_NVS",
        EntryType::BAD_MEMORY => "BAD_MEMORY",
        EntryType::BOOTLOADER_RECLAIMABLE => "BOOTLOADER_RECLAIMABLE",
        EntryType::EXECUTABLE_AND_MODULES => "EXECUTABLE_AND_MODULES",
        EntryType::FRAMEBUFFER => "FRAMEBUFFER",
        _ => "UNKNOWN_TYPE", // Jaga-jaga jika ada tipe baru atau tidak dikenal
    };

    //println!("Tipe Memori: {}", nama_tipe);
    return nama_tipe;
}

pub fn test_get_request_memory_map() {
    print!("MEMORY MAP FUNCTION DIPANGGIL!\n");
    if let Some(memory_map_response) = MEMORY_MAP_REQUEST.get_response() {
        print!("MEMORY MAP ADA RESPONSE\n");
        for entry in memory_map_response.entries() {
            print!(
                "\n Base: {},  Type: {}, Length: {}",
                entry.base,
                cetak_entry_type(entry.entry_type),
                entry.length
            );
        }
    } else {
        print!("MEMORY MAP TIDAK ADA RESPONSE\n");
    }
}

pub fn test_get_request_memory_map_usable() {
    print!("MEMORY MAP FUNCTION DIPANGGIL!\n");
    if let Some(memory_map_response) = MEMORY_MAP_REQUEST.get_response() {
        print!("MEMORY MAP ADA RESPONSE\n");
        for entry in memory_map_response.entries() {
            if entry.entry_type == EntryType::USABLE {
                print!(
                    "\n Base: {},  Type: {}, Length: {}, Total Frame {}",
                    entry.base,
                    cetak_entry_type(entry.entry_type),
                    entry.length,
                    entry.length / 4096
                );
            }
        }
    } else {
        print!("MEMORY MAP TIDAK ADA RESPONSE\n");
    }
}

pub fn test_frame_allocator() {
    let mut frame_allocator = FRAME_ALLOCATOR.lock();

    let frame_test = frame_allocator.allocate().unwrap();

    print!("\nFrame: {:#?}\n", frame_test);

    let virt_address = physical_to_virtual(frame_test).unwrap();

    let ptr = virt_address as *mut u8;

    unsafe {
        ptr.write(123);
        let value = *ptr;
        print!("\nvaluenya: {:?}", ptr);
        print!("\nvaluenya: {}", value);
    }
}

/// Panggil ini SEKALI di awal boot (setelah Limine request siap),
/// sebelum kode lain memakai `alloc::` (Box, Vec, String, dll).
pub fn init() {
    let mut frame_allocator = FRAME_ALLOCATOR.lock();

    if !heap_allocator::init_heap(&mut frame_allocator) {
        print!("\nGAGAL INIT HEAP, KERNEL BERHENTI\n");
        loop {}
    }
}

extern crate alloc;
use alloc::{boxed::Box, string::String, vec::Vec};

pub fn test_heap_allocator() {
    print!("\n--- TEST HEAP ALLOCATOR ---");

    // Test 1: Box sederhana
    let boxed = Box::new(42);
    print!("\nBox value: {}, addr: {:p}", *boxed, boxed);

    // Test 2: Vec yang tumbuh (memaksa realokasi beberapa kali)
    let mut vec = Vec::new();
    for i in 0..100 {
        vec.push(i);
    }
    print!("\nVec len: {}, sum: {}", vec.len(), vec.iter().sum::<i32>());

    // Test 3: String
    let s = String::from("halo dari heap!");
    print!("\nString: {}", s);

    // Test 4: alokasi besar untuk pastikan ga cuma kebetulan cocok di cache kecil
    let big: Vec<u8> = alloc::vec![0u8; 100_000];
    print!("\nBig vec len: {}", big.len());

    // Test 5: drop manual, pastikan dealloc jalan tanpa crash
    drop(boxed);
    drop(vec);
    drop(s);
    drop(big);

    print!("\n--- HEAP TEST SELESAI, TIDAK CRASH ---\n");
}

pub fn stress_test_heap() {
    let mut boxes = Vec::new();
    for i in 0..1000 {
        boxes.push(Box::new(i));
    }
    for (i, b) in boxes.iter().enumerate() {
        assert_eq!(**b, i, "DATA CORRUPT di index {}", i);
    }
    print!("\nSTRESS TEST OK: 1000 alokasi konsisten\n");
}

pub fn allocate_frame() -> Option<usize> {
    FRAME_ALLOCATOR.lock().allocate()
}

pub fn free_frame(frame: usize) {
    FRAME_ALLOCATOR.lock().free(frame);
}
