//! Baca RSDP (Root System Description Pointer) dari Limine, titik masuk
//! ke seluruh table ACPI (MADT, MCFG, HPET, dst).
//!
//! Struct `RSDP`/`XSDP` di bawah ini mendeskripsikan layout memori ACPI
//! sesuai spec -- belum dipakai untuk parsing penuh (baru sekadar
//! `test_rsdp` yang cetak revision & alamatnya), jadi field-nya belum
//! semua terbaca lewat kode Rust.

use core::sync::atomic::Ordering;

use limine::request::RsdpRequest;

use crate::{
    memory::paging::{
        flags::{PRESENT, WRITABLE},
        map_page,
    },
    println,
};

// structure for revision 0 (version 1.0)
#[allow(dead_code)]
#[repr(C, packed)]
#[derive(Debug)]
struct RSDP {
    signature: [u8; 8],
    checksum: u8,
    oemid: [u8; 6],
    revision: u8,
    rsdt_address: u32,
}

// structure for revision 2 (version 2.0+)
// but we make that later
#[allow(dead_code)]
#[repr(C, packed)]
struct XSDP {
    signature: [u8; 8],
    checksum: u8,
    oemid: [u8; 6],
    revision: u8,
    rsdt_address: u32, // deprecated since version 2.0

    length: u32,
    xsdt_address: u64,
    extendedchecksum: u8,
    reserved: [u8; 3],
}

#[used]
#[unsafe(link_section = ".requests")]
static RSDP_REQUEST: RsdpRequest = RsdpRequest::new();

pub unsafe fn test_rsdp() {
    if let Some(rsdp_response) = RSDP_REQUEST.get_response() {
        println!("RSDP REVISION: {}", rsdp_response.revision());
        let ptr: *const u8 = rsdp_response.address() as *const u8;
        println!("RSDP ADDRESS: {:p}", ptr);
        map_page(ptr as u64, ptr as u64, WRITABLE | PRESENT);
        unsafe {
            let rsdp = &*(ptr as *const RSDP);
            println!("{:#?}", rsdp);
            map_page(
                rsdp.rsdt_address as u64,
                rsdp.rsdt_address as u64,
                WRITABLE | PRESENT,
            );
            let madt = find_MADT(rsdp.rsdt_address as *const u32);
            if let Some(madt) = madt {
                println!("MADT: {:#?}", madt);
                map_page(madt.entries as u64, madt.entries as u64, PRESENT | WRITABLE);
                println!("entry type {}", (*madt.entries));
                println!("record length: {}", (*madt.entries.add(1)));
                // let entry = madt.entries.byte_add((madt.entries >> 8) as u8)
            }
        }
    }
}

#[repr(C, packed)]
#[derive(Debug)]
struct SDTHeader {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: u32,
    creator_revisioin: u32,
}

#[repr(C, packed)]
struct RSDT {
    header: SDTHeader,
    entries: [u32; 0],
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct MADT {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: u32,
    creator_revisioin: u32,
    local_apic_address: u32,
    flags: u32,
    entries: *const u8,
}

struct MADTEntry {
    entry_type: u8,
    record_length: u8,
}

pub unsafe fn find_MADT(rsdt_addr: *const u32) -> Option<MADT> {
    unsafe {
        let rsdt = &*(rsdt_addr as *const RSDT);
        let size = size_of::<SDTHeader>() as u32;
        let count = (rsdt.header.length - size) / 4;

        let entries = unsafe { (rsdt_addr as *const u8).add(size as usize) as *const u32 };

        for i in 0..count {
            let table_addr = entries.add(i as usize).read_unaligned();

            println!("TABLE {} @ {:#x}", i, table_addr);

            map_page(table_addr as u64, table_addr as u64, WRITABLE | PRESENT);

            let header = &*(table_addr as *const SDTHeader);

            println!("{:?}", header);

            if header.signature == *b"APIC" {
                println!("DAPAT MADT COY");
                return Some(*(table_addr as *const MADT));
            }
        }

        println!("gak dapat jir");
        return None;
    }
}
