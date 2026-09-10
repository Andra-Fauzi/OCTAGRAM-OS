// memory/paging.rs
use crate::memory::{allocate_frame, physical_to_virtual};
use core::arch::asm;

pub const PAGE_SIZE: u64 = 4096;

const ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

/// Bit-bit flag entry page table. Formatnya sama di semua level (PML4E, PDPTE, PDE, PTE).
pub mod flags {
    pub const PRESENT: u64 = 1 << 0;
    pub const WRITABLE: u64 = 1 << 1;
    pub const USER: u64 = 1 << 2;
    pub const WRITE_THROUGH: u64 = 1 << 3;
    pub const NO_CACHE: u64 = 1 << 4;
    pub const HUGE_PAGE: u64 = 1 << 7; // cuma valid di PDPTE (1GB) / PDE (2MB)
    pub const GLOBAL: u64 = 1 << 8;
    pub const NO_EXECUTE: u64 = 1 << 63; // butuh EFER.NXE=1 buat aktif beneran
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PagingError {
    /// Ada huge page (2MB/1GB) di tengah jalur, jadi nggak bisa turun ke level 4K.
    HugePageInPath,
    /// Alamat virtual ini sudah punya mapping — map_page menolak menimpa diam-diam.
    AlreadyMapped,
    /// Alamat virtual ini belum di-mapping sama sekali.
    NotMapped,
    /// Frame allocator kehabisan memori fisik.
    OutOfFrames,
    /// HHDM offset dari Limine belum tersedia (harusnya nggak pernah terjadi setelah boot).
    HhdmUnavailable,
}

fn table_indices(virt: u64) -> (usize, usize, usize, usize) {
    (
        ((virt >> 39) & 0x1FF) as usize,
        ((virt >> 30) & 0x1FF) as usize,
        ((virt >> 21) & 0x1FF) as usize,
        ((virt >> 12) & 0x1FF) as usize,
    )
}

unsafe fn read_cr3() -> u64 {
    let cr3: u64;
    unsafe { asm!("mov {}, cr3", out(reg) cr3) };
    cr3
}

fn pml4_virt() -> Result<*mut u64, PagingError> {
    let cr3 = unsafe { read_cr3() };
    let phys = cr3 & ADDR_MASK;
    let virt = physical_to_virtual(phys as usize).ok_or(PagingError::HhdmUnavailable)?;
    Ok(virt as *mut u64)
}

/// Ambil table di level berikutnya. Kalau belum present, alokasikan frame baru
/// (lewat frame allocator kita) dan zero-kan otomatis.
unsafe fn next_table_or_create(entry: &mut u64) -> Result<*mut u64, PagingError> {
    unsafe {
        if *entry & flags::HUGE_PAGE != 0 {
            return Err(PagingError::HugePageInPath);
        }
        if *entry & flags::PRESENT == 0 {
            let frame = allocate_frame().ok_or(PagingError::OutOfFrames)? as u64;
            // allocate_frame() sudah menzero-kan isi frame, jadi table barunya kosong.
            *entry = frame | flags::PRESENT | flags::WRITABLE;
        }
        let phys = *entry & ADDR_MASK;
        let virt = physical_to_virtual(phys as usize).ok_or(PagingError::HhdmUnavailable)?;
        Ok(virt as *mut u64)
    }
}

/// Sama seperti di atas, tapi TIDAK membuat table baru — dipakai untuk unmap/translate
/// di mana kita cuma mau membaca struktur yang sudah ada.
fn next_table_existing(entry: u64) -> Result<*mut u64, PagingError> {
    if entry & flags::PRESENT == 0 {
        return Err(PagingError::NotMapped);
    }
    if entry & flags::HUGE_PAGE != 0 {
        return Err(PagingError::HugePageInPath);
    }
    let phys = entry & ADDR_MASK;
    let virt = physical_to_virtual(phys as usize).ok_or(PagingError::HhdmUnavailable)?;
    Ok(virt as *mut u64)
}

/// Petakan satu halaman 4KB: virtual -> fisik, dengan flag tambahan (WRITABLE, NO_CACHE, dst).
/// PRESENT selalu otomatis ditambahkan. Gagal kalau virt_addr sudah punya mapping.
pub unsafe fn map_page(
    virt_addr: u64,
    phys_addr: u64,
    extra_flags: u64,
) -> Result<(), PagingError> {
    unsafe {
        let virt = virt_addr & !(PAGE_SIZE - 1);
        let phys = phys_addr & !(PAGE_SIZE - 1);

        let (pml4_idx, pdpt_idx, pd_idx, pt_idx) = table_indices(virt);

        let pml4 = pml4_virt()?;
        let pdpt = next_table_or_create(&mut *pml4.add(pml4_idx))?;
        let pd = next_table_or_create(&mut *pdpt.add(pdpt_idx))?;
        let pt = next_table_or_create(&mut *pd.add(pd_idx))?;

        let pte = &mut *pt.add(pt_idx);
        if *pte & flags::PRESENT != 0 {
            return Err(PagingError::AlreadyMapped);
        }

        *pte = phys | flags::PRESENT | extra_flags;
        asm!("invlpg [{}]", in(reg) virt);
        Ok(())
    }
}

/// Petakan region contiguous sepanjang `size` byte, dibulatkan ke atas per halaman 4KB.
pub unsafe fn map_range(
    virt_start: u64,
    phys_start: u64,
    size: u64,
    extra_flags: u64,
) -> Result<(), PagingError> {
    let page_count = size.div_ceil(PAGE_SIZE);
    for i in 0..page_count {
        let offset = i * PAGE_SIZE;
        unsafe { map_page(virt_start + offset, phys_start + offset, extra_flags)? };
    }
    Ok(())
}

/// Hapus mapping satu halaman. Mengembalikan alamat fisik yang tadinya di-mapping
/// (berguna kalau kamu mau `free_frame()` frame itu juga).
pub unsafe fn unmap_page(virt_addr: u64) -> Result<u64, PagingError> {
    unsafe {
        let virt = virt_addr & !(PAGE_SIZE - 1);
        let (pml4_idx, pdpt_idx, pd_idx, pt_idx) = table_indices(virt);

        let pml4 = pml4_virt()?;
        let pdpt = next_table_existing(*pml4.add(pml4_idx))?;
        let pd = next_table_existing(*pdpt.add(pdpt_idx))?;
        let pt = next_table_existing(*pd.add(pd_idx))?;

        let pte = &mut *pt.add(pt_idx);
        if *pte & flags::PRESENT == 0 {
            return Err(PagingError::NotMapped);
        }

        let phys = *pte & ADDR_MASK;
        *pte = 0;
        asm!("invlpg [{}]", in(reg) virt);
        Ok(phys)
    }
}

/// Cek alamat fisik tempat sebuah alamat virtual sekarang menunjuk (kalau ada).
/// Berguna buat debugging ("kenapa alamat ini page fault?").
pub fn translate(virt_addr: u64) -> Result<u64, PagingError> {
    let virt = virt_addr & !(PAGE_SIZE - 1);
    let offset = virt_addr & (PAGE_SIZE - 1);
    let (pml4_idx, pdpt_idx, pd_idx, pt_idx) = table_indices(virt);

    let pml4 = pml4_virt()?;
    unsafe {
        let pdpt = next_table_existing(*pml4.add(pml4_idx))?;
        let pd = next_table_existing(*pdpt.add(pdpt_idx))?;
        let pt = next_table_existing(*pd.add(pd_idx))?;
        let pte = *pt.add(pt_idx);
        if pte & flags::PRESENT == 0 {
            return Err(PagingError::NotMapped);
        }
        Ok((pte & ADDR_MASK) + offset)
    }
}

/// Petakan satu halaman fisik MMIO (device register seperti LAPIC/IOAPIC) ke alamat
/// virtual HHDM (phys + HHDM offset), dengan flag WRITABLE + NO_CACHE.
/// Aman dipanggil dua kali untuk alamat yang sama (idempotent) — kalau sudah pernah
/// di-mapping sebelumnya, langsung kembalikan virtual address-nya tanpa error.
pub unsafe fn map_mmio_page(phys_addr: u64) -> u64 {
    unsafe {
        let phys_aligned = phys_addr & !(PAGE_SIZE - 1);
        let virt = physical_to_virtual(phys_aligned as usize).expect("HHDM belum tersedia") as u64;

        match map_page(virt, phys_aligned, flags::WRITABLE | flags::NO_CACHE) {
            Ok(()) => {}
            Err(PagingError::AlreadyMapped) => {
                // sudah pernah dipetakan (misal init dipanggil dua kali) — aman, lanjut saja
            }
            Err(e) => panic!("gagal memetakan MMIO {:#x}: {:?}", phys_addr, e),
        }

        virt
    }
}

/// Sama seperti map_mmio_page, tapi untuk region MMIO yang lebih dari satu halaman
/// (misal HPET atau device dengan banyak register berdekatan).
pub unsafe fn map_mmio_range(phys_addr: u64, size: u64) -> u64 {
    unsafe {
        let phys_aligned = phys_addr & !(PAGE_SIZE - 1);
        let virt = physical_to_virtual(phys_aligned as usize).expect("HHDM belum tersedia") as u64;

        match map_range(virt, phys_aligned, size, flags::WRITABLE | flags::NO_CACHE) {
            Ok(()) => {}
            Err(PagingError::AlreadyMapped) => {}
            Err(e) => panic!("gagal memetakan MMIO range {:#x}: {:?}", phys_addr, e),
        }

        virt
    }
}
