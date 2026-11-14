// memory/paging.rs
use crate::memory::{allocate_frame, free_frame, physical_to_virtual};
use alloc::vec::Vec;
use core::arch::asm;

pub const PAGE_SIZE: u64 = 4096;
pub const USER_ADDRESS_MIN: u64 = 0x1_0000;
pub const USER_ADDRESS_MAX: u64 = 0x0000_8000_0000_0000;

const ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;
const USER_PML4_ENTRIES: usize = 256;

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
    NxeUnavailable,
    UserAddressOutOfRange,
    NotUserAccessible,
    NotWritable,
    AddressOverflow,
}

/// A process page table with a private lower half and shared kernel mappings.
pub struct AddressSpace {
    root_frame: u64,
    kernel_root_frame: u64,
}

impl AddressSpace {
    /// Create a PML4 that copies kernel mappings and starts with no user mappings.
    pub fn new() -> Result<Self, PagingError> {
        let kernel_root_frame = unsafe { read_cr3() } & ADDR_MASK;
        Self::new_with_kernel_root(kernel_root_frame)
    }

    fn new_with_kernel_root(kernel_root_frame: u64) -> Result<Self, PagingError> {
        let root_frame = allocate_frame().ok_or(PagingError::OutOfFrames)? as u64;
        let Some(kernel_root) = physical_to_virtual(kernel_root_frame as usize) else {
            free_frame(root_frame as usize);
            return Err(PagingError::HhdmUnavailable);
        };
        let Some(root) = physical_to_virtual(root_frame as usize) else {
            free_frame(root_frame as usize);
            return Err(PagingError::HhdmUnavailable);
        };

        unsafe {
            let kernel_root = kernel_root as *const u64;
            let root = root as *mut u64;
            for index in 0..USER_PML4_ENTRIES {
                root.add(index).write(0);
            }
            for index in USER_PML4_ENTRIES..512 {
                root.add(index).write(kernel_root.add(index).read());
            }
        }

        Ok(Self {
            root_frame,
            kernel_root_frame,
        })
    }

    /// Create an independent copy of the listed user pages and their mappings.
    ///
    /// The caller must list every user page owned by this address space.
    pub fn fork_clone(
        &self,
        user_pages: &[u64],
        shared_pages: &[u64],
    ) -> Result<Self, PagingError> {
        let mut mappings = Vec::new();
        mappings
            .try_reserve_exact(user_pages.len())
            .map_err(|_| PagingError::OutOfFrames)?;
        let mappings_result = (|| {
            unsafe { self.activate() };
            for &address in user_pages {
                let virtual_address = address & !(PAGE_SIZE - 1);
                let entry = unsafe { mapping_entry(virtual_address)? };
                mappings.push((
                    virtual_address,
                    entry & ADDR_MASK,
                    entry & !(ADDR_MASK | flags::PRESENT),
                    shared_pages.contains(&virtual_address),
                ));
            }
            Ok::<_, PagingError>(())
        })();
        unsafe { self.activate_kernel() };
        mappings_result?;

        let child = Self::new_with_kernel_root(self.kernel_root_frame)?;
        let mut mapped_pages = Vec::new();
        if mapped_pages.try_reserve_exact(mappings.len()).is_err() {
            child.destroy()?;
            return Err(PagingError::OutOfFrames);
        }

        let clone_result = (|| {
            unsafe { child.activate() };
            for (virtual_address, source_frame, mapping_flags, shared) in mappings {
                let destination_frame = if shared {
                    source_frame
                } else {
                    allocate_frame().ok_or(PagingError::OutOfFrames)? as u64
                };
                if !shared {
                    let Some(source) = physical_to_virtual(source_frame as usize) else {
                        free_frame(destination_frame as usize);
                        return Err(PagingError::HhdmUnavailable);
                    };
                    let Some(destination) = physical_to_virtual(destination_frame as usize) else {
                        free_frame(destination_frame as usize);
                        return Err(PagingError::HhdmUnavailable);
                    };
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            source as *const u8,
                            destination as *mut u8,
                            PAGE_SIZE as usize,
                        );
                    }
                }
                if let Err(error) =
                    unsafe { map_page(virtual_address, destination_frame, mapping_flags) }
                {
                    if !shared {
                        free_frame(destination_frame as usize);
                    }
                    return Err(error);
                }
                mapped_pages.push((virtual_address, !shared));
            }
            Ok::<_, PagingError>(())
        })();
        unsafe { child.activate_kernel() };

        if let Err(error) = clone_result {
            unsafe { child.activate() };
            let mut cleanup_error = None;
            for (virtual_address, owned) in mapped_pages {
                match unsafe { unmap_page(virtual_address) } {
                    Ok(frame) if owned => free_frame(frame as usize),
                    Ok(_) => {}
                    Err(error) if cleanup_error.is_none() => cleanup_error = Some(error),
                    Err(_) => {}
                }
            }
            unsafe { child.activate_kernel() };
            let destroy_result = child.destroy();
            return Err(cleanup_error
                .or_else(|| destroy_result.err())
                .unwrap_or(error));
        }

        Ok(child)
    }

    /// Switch to this address space. Kernel code and data remain mapped.
    ///
    /// # Safety
    /// This space must have valid kernel mappings and the caller must restore
    /// the kernel root before accessing user-only mappings from another task.
    pub unsafe fn activate(&self) {
        unsafe {
            asm!(
                "mov cr3, {}",
                in(reg) self.root_frame,
                options(nostack, preserves_flags)
            );
        }
    }

    /// Restore the kernel page table that was active when this space was built.
    ///
    /// # Safety
    /// The kernel root must still be valid.
    pub unsafe fn activate_kernel(&self) {
        unsafe {
            asm!(
                "mov cr3, {}",
                in(reg) self.kernel_root_frame,
                options(nostack, preserves_flags)
            );
        }
    }

    /// Free all private user page tables and the PML4 root.
    ///
    /// User data pages must be unmapped and freed by the owner first.
    pub fn destroy(self) -> Result<(), PagingError> {
        unsafe { self.activate() };
        let Some(root) = physical_to_virtual(self.root_frame as usize) else {
            unsafe { self.activate_kernel() };
            free_frame(self.root_frame as usize);
            return Err(PagingError::HhdmUnavailable);
        };
        let root = root as *mut u64;
        let mut first_error = None;
        for index in 0..USER_PML4_ENTRIES {
            let entry = unsafe { root.add(index).read() };
            if entry & flags::PRESENT == 0 {
                continue;
            }
            unsafe {
                root.add(index).write(0);
            }
            if let Err(error) = free_page_table(entry & ADDR_MASK, 3)
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        unsafe { self.activate_kernel() };
        free_frame(self.root_frame as usize);
        first_error.map_or(Ok(()), Err)
    }
}

fn free_page_table(frame: u64, level: usize) -> Result<(), PagingError> {
    let table =
        physical_to_virtual(frame as usize).ok_or(PagingError::HhdmUnavailable)? as *mut u64;
    if level > 1 {
        for index in 0..512 {
            let entry = unsafe { table.add(index).read() };
            if entry & flags::PRESENT != 0 && entry & flags::HUGE_PAGE == 0 {
                free_page_table(entry & ADDR_MASK, level - 1)?;
            }
        }
    }
    free_frame(frame as usize);
    Ok(())
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
unsafe fn next_table_or_create(
    entry: &mut u64,
    user_accessible: bool,
) -> Result<*mut u64, PagingError> {
    unsafe {
        if *entry & flags::HUGE_PAGE != 0 {
            return Err(PagingError::HugePageInPath);
        }
        if *entry & flags::PRESENT == 0 {
            let frame = allocate_frame().ok_or(PagingError::OutOfFrames)? as u64;
            // allocate_frame() sudah menzero-kan isi frame, jadi table barunya kosong.
            *entry = frame | flags::PRESENT | flags::WRITABLE;
        }
        if user_accessible {
            *entry |= flags::USER | flags::WRITABLE;
        }
        let phys = *entry & ADDR_MASK;
        let virt = physical_to_virtual(phys as usize).ok_or(PagingError::HhdmUnavailable)?;
        Ok(virt as *mut u64)
    }
}

fn user_mapping_flags(virt: u64, write_access: bool) -> Result<u64, PagingError> {
    let (pml4_idx, pdpt_idx, pd_idx, pt_idx) = table_indices(virt);
    unsafe {
        let pml4_entry = *pml4_virt()?.add(pml4_idx);
        let pdpt_entry = *next_table_existing(pml4_entry)?.add(pdpt_idx);
        let pd_entry = *next_table_existing(pdpt_entry)?.add(pd_idx);
        let pte = *next_table_existing(pd_entry)?.add(pt_idx);
        for entry in [pml4_entry, pdpt_entry, pd_entry, pte] {
            if entry & flags::USER == 0 {
                return Err(PagingError::NotUserAccessible);
            }
            if write_access && entry & flags::WRITABLE == 0 {
                return Err(PagingError::NotWritable);
            }
        }
        Ok(pte)
    }
}

unsafe fn mapping_entry(virt: u64) -> Result<u64, PagingError> {
    let (pml4_idx, pdpt_idx, pd_idx, pt_idx) = table_indices(virt);
    unsafe {
        let pml4_entry = pml4_virt()?.add(pml4_idx).read_volatile();
        let pdpt_entry = next_table_existing(pml4_entry)?
            .add(pdpt_idx)
            .read_volatile();
        let pd_entry = next_table_existing(pdpt_entry)?.add(pd_idx).read_volatile();
        let pte = next_table_existing(pd_entry)?.add(pt_idx).read_volatile();
        if pte & flags::PRESENT == 0 {
            return Err(PagingError::NotMapped);
        }
        Ok(pte)
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

/// Translate a mapped virtual address using the active x86_64 page tables.
pub fn translate_address(virt_addr: u64) -> Result<u64, PagingError> {
    let (pml4_idx, pdpt_idx, pd_idx, pt_idx) = table_indices(virt_addr);
    unsafe {
        let pml4_entry = pml4_virt()?.add(pml4_idx).read_volatile();
        let pdpt = next_table_existing(pml4_entry)?;
        let pdpt_entry = pdpt.add(pdpt_idx).read_volatile();
        if pdpt_entry & flags::HUGE_PAGE != 0 {
            let base = pdpt_entry & 0x000F_FFFF_C000_0000;
            return Ok(base | (virt_addr & ((1 << 30) - 1)));
        }

        let pd = next_table_existing(pdpt_entry)?;
        let pd_entry = pd.add(pd_idx).read_volatile();
        if pd_entry & flags::HUGE_PAGE != 0 {
            let base = pd_entry & 0x000F_FFFF_FFE0_0000;
            return Ok(base | (virt_addr & ((1 << 21) - 1)));
        }

        let pt = next_table_existing(pd_entry)?;
        let pt_entry = pt.add(pt_idx).read_volatile();
        if pt_entry & flags::PRESENT == 0 {
            return Err(PagingError::NotMapped);
        }
        Ok((pt_entry & ADDR_MASK) | (virt_addr & (PAGE_SIZE - 1)))
    }
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
        let user_accessible =
            extra_flags & flags::USER != 0 || (USER_ADDRESS_MIN..USER_ADDRESS_MAX).contains(&virt);
        if user_accessible && !(USER_ADDRESS_MIN..USER_ADDRESS_MAX).contains(&virt) {
            return Err(PagingError::UserAddressOutOfRange);
        }

        let pml4 = pml4_virt()?;
        let pdpt = next_table_or_create(&mut *pml4.add(pml4_idx), user_accessible)?;
        let pd = next_table_or_create(&mut *pdpt.add(pdpt_idx), user_accessible)?;
        let pt = next_table_or_create(&mut *pd.add(pd_idx), user_accessible)?;

        let pte = &mut *pt.add(pt_idx);
        if *pte & flags::PRESENT != 0 {
            return Err(PagingError::AlreadyMapped);
        }

        *pte = phys | flags::PRESENT | extra_flags;
        asm!("invlpg [{}]", in(reg) virt);
        Ok(())
    }
}

/// Update permissions on an existing user mapping without changing its frame.
///
/// # Safety
/// The caller must be operating on the address space that owns `virt_addr`.
pub unsafe fn set_page_flags(virt_addr: u64, extra_flags: u64) -> Result<(), PagingError> {
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
        let physical = *pte & ADDR_MASK;
        *pte = physical | flags::PRESENT | extra_flags;
        asm!("invlpg [{}]", in(reg) virt);
        Ok(())
    }
}

/// Enable x86 execute-disable page permissions.
pub unsafe fn enable_nxe() -> Result<(), PagingError> {
    let max_extended_leaf = core::arch::x86_64::__cpuid(0x8000_0000).eax;
    if max_extended_leaf < 0x8000_0001 {
        return Err(PagingError::NxeUnavailable);
    }
    let features = core::arch::x86_64::__cpuid(0x8000_0001);
    if features.edx & (1 << 20) == 0 {
        return Err(PagingError::NxeUnavailable);
    }

    unsafe {
        let msr = 0xC000_0080u32;
        let low: u32;
        let high: u32;
        asm!("rdmsr", in("ecx") msr, out("eax") low, out("edx") high);
        let efer = ((high as u64) << 32) | low as u64 | (1 << 11);
        asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") efer as u32,
            in("edx") (efer >> 32) as u32
        );
    }
    Ok(())
}

/// Ensure every page in a user buffer has the requested access.
pub fn validate_user_range(
    address: u64,
    length: usize,
    write_access: bool,
) -> Result<(), PagingError> {
    if length == 0 {
        return Ok(());
    }
    let length = u64::try_from(length).map_err(|_| PagingError::AddressOverflow)?;
    let end = address
        .checked_add(length)
        .ok_or(PagingError::AddressOverflow)?;
    if address < USER_ADDRESS_MIN || end > USER_ADDRESS_MAX {
        return Err(PagingError::UserAddressOutOfRange);
    }

    let mut page = address & !(PAGE_SIZE - 1);
    while page < end {
        user_mapping_flags(page, write_access)?;
        page = page
            .checked_add(PAGE_SIZE)
            .ok_or(PagingError::AddressOverflow)?;
    }
    Ok(())
}

/// Ensure an address is mapped for user execution and is not marked NX.
pub fn validate_user_executable(address: u64) -> Result<(), PagingError> {
    if !(USER_ADDRESS_MIN..USER_ADDRESS_MAX).contains(&address) {
        return Err(PagingError::UserAddressOutOfRange);
    }
    let entry = user_mapping_flags(address, false)?;
    if entry & flags::NO_EXECUTE != 0 {
        return Err(PagingError::NotUserAccessible);
    }
    Ok(())
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
