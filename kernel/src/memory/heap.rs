//! Kernel global allocator backed by a statically mapped 64 MiB heap region.
//!
//! The RAM root filesystem stores every file in this heap, so its size is also
//! the capacity of `/` (the seeded toolchain and the CPython runtime use
//! roughly 20 MiB).

use core::mem::MaybeUninit;
use linked_list_allocator::LockedHeap;

const FRAME_SIZE: usize = 4096;
const HEAP_FRAMES: usize = 16384;
pub const HEAP_SIZE: usize = HEAP_FRAMES * FRAME_SIZE;

#[repr(align(16))]
struct HeapRegion([MaybeUninit<u8>; HEAP_SIZE]);

static mut HEAP_REGION: HeapRegion = HeapRegion([MaybeUninit::uninit(); HEAP_SIZE]);

#[cfg_attr(not(test), global_allocator)]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Initialize the allocator over the kernel's zero-initialized `.bss` region.
pub fn init_heap() {
    let heap_start = unsafe { core::ptr::addr_of_mut!(HEAP_REGION.0).cast::<u8>() };
    unsafe {
        ALLOCATOR.lock().init(heap_start, HEAP_SIZE);
    }
    crate::println!(
        "[heap] virt={:#x} size={} KiB",
        heap_start as usize,
        HEAP_SIZE / 1024
    );
}
