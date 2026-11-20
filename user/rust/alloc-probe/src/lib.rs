#![no_std]

extern crate alloc;

use alloc::vec::Vec;

/// Compile-time probe for the OCTAGRAM target's core + alloc sysroot.
/// This is an rlib-only probe; it deliberately makes no claim that a global
/// allocator or Rust std is available on the target yet.
#[no_mangle]
pub extern "C" fn octagram_alloc_probe(count: usize, value: u64) -> usize {
    let count = count.min(4096);
    let mut values = Vec::with_capacity(count);
    values.resize(count, value);
    values.iter().fold(values.capacity(), |sum, item| {
        sum.wrapping_add(*item as usize)
    })
}
