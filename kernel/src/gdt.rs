use core::arch::asm;

use crate::println;

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
struct GdtEntry {
    limit_low: u16,
    base_low: u16,
    base_middle: u8,
    access: u8,
    limit_high_flags: u8,
    base_high: u8,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
struct GdtDescriptor {
    limit: u16,
    base: u64,
}

impl GdtEntry {
    pub const fn new(base: u32, limit: u32, access: u8, flags: u8) -> Self {
        Self {
            limit_low: (limit & 0xFFFF) as u16,

            base_low: (base & 0xFFFF) as u16,

            base_middle: ((base >> 16) & 0xFF) as u8,

            access,

            limit_high_flags: (((limit >> 16) & 0x0F) as u8) | ((flags & 0x0F) << 4),

            base_high: ((base >> 24) & 0xFF) as u8,
        }
    }
}

#[unsafe(link_section = ".data")]
static GDT: [GdtEntry; 3] = [
    GdtEntry::new(0, 0, 0, 0),
    GdtEntry::new(0, 0xFFFFF, 0x9B, 0b1010),
    GdtEntry::new(0, 0xFFFFF, 0x93, 0b0000),
];

pub unsafe fn load_gdt() {
    const _: () = assert!(core::mem::size_of::<GdtEntry>() == 8);

    // Gunakan 'const' untuk ukuran memori karena dihitung saat compile-time
    const LIMIT: u16 = (core::mem::size_of::<GdtEntry>() * GDT.len() - 1) as u16;

    // Definisikan GDTR sebagai static global/lokal menggunakan bantuan casting pointer yang valid
    static mut GDTR: GdtDescriptor = GdtDescriptor {
        limit: LIMIT,
        base: 0, // Akan diisi sesaat sebelum lgdt dijalankan
    };

    // Isi nilai base address secara dinamis saat runtime sebelum LGDT
    GDTR.base = core::ptr::addr_of!(GDT) as u64;

    //println!("GDT : {:#?}", GDT);
    //println!("GDTR : {:#?}", GDTR);

    asm!(
        // Gunakan instruksi 'sym' untuk langsung merujuk ke alamat static GDTR
        "lgdt [{gdtr_ptr}]",

        // Reload data segmen terlebih dahulu agar aman
        "mov ax, 0x10",
        "mov ds, ax",
        "mov es, ax",
        "mov ss, ax",

        // Gunakan retfq (Far Return) untuk mengubah Code Segment (CS) ke 0x08
        "push 0x08",
        "lea rax, [rip + 2f]",
        "push rax",
        "retfq",
        "2:",
        gdtr_ptr = in(reg) core::ptr::addr_of!(GDTR),
        out("rax") _,
    );
}
