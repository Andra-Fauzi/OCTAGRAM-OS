use linked_list_allocator::LockedHeap;

use crate::print;

use super::frame_allocator::FrameAllocator;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

const FRAME_SIZE: usize = 4096;

/// Ukuran heap kernel: 256 frame x 4KB = 1 MiB.
/// Ubah nilai ini kalau butuh heap lebih besar.
const HEAP_FRAMES: usize = 256;
pub const HEAP_SIZE: usize = HEAP_FRAMES * FRAME_SIZE;

/// Inisialisasi heap global allocator.
///
/// Strategi: minta `HEAP_FRAMES` frame fisik dari `FrameAllocator` satu per satu.
/// Karena free-list frame allocator bersifat LIFO per region (frame terakhir yang
/// di-push jadi yang pertama di-pop), pemanggilan `allocate()` berturut-turut tepat
/// setelah `init()` akan menghasilkan alamat fisik yang KONTIGU (turun per FRAME_SIZE).
/// Kita verifikasi asumsi ini secara eksplisit; kalau ternyata tidak kontigu
/// (misal karena region memory terlalu kecil / terfragmentasi), inisialisasi gagal
/// dengan aman alih-alih menulis ke memori yang salah.
///
/// Setelah dapat blok fisik kontigu, kita konversi alamat awalnya ke alamat virtual
/// lewat HHDM (`physical_to_virtual`) dan serahkan range itu ke `LockedHeap`.
pub fn init_heap(frame_allocator: &mut FrameAllocator) -> bool {
    let mut lowest_frame: Option<usize> = None;
    let mut expected_next: Option<usize> = None;

    for _ in 0..HEAP_FRAMES {
        let frame = match frame_allocator.allocate() {
            Some(f) => f,
            None => {
                print!("\nHEAP INIT GAGAL: frame fisik tidak cukup");
                return false;
            }
        };

        if let Some(expected) = expected_next {
            if frame != expected {
                print!("\nHEAP INIT GAGAL: frame fisik tidak kontigu");
                return false;
            }
        }

        lowest_frame = Some(frame);
        expected_next = Some(frame.wrapping_sub(FRAME_SIZE));
    }

    let heap_start_phys = match lowest_frame {
        Some(addr) => addr,
        None => return false,
    };

    let heap_start_virt = match super::physical_to_virtual(heap_start_phys) {
        Some(addr) => addr,
        None => {
            print!("\nHEAP INIT GAGAL: HHDM tidak merespon");
            return false;
        }
    };

    unsafe {
        ALLOCATOR.lock().init(heap_start_virt as *mut u8, HEAP_SIZE);
    }

    print!(
        "\nHEAP OK: phys={:#x} virt={:#x} size={} bytes",
        heap_start_phys, heap_start_virt, HEAP_SIZE
    );

    true
}
