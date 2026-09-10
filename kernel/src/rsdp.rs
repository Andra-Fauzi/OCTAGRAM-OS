use limine::request::RsdpRequest;

use crate::println;

// structure for revision 0 (version 1.0)
#[repr(C, packed)]
struct RSDP {
    signature: [u8; 8],
    checksum: u8,
    oemid: [u8; 6],
    revision: u8,
    rsdt_address: u32,
}

// structure for revision 2 (version 2.0+)
// but we make that later
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

pub fn test_rsdp() {
    if let Some(rsdp_response) = RSDP_REQUEST.get_response() {
        println!("RSDP REVISION: {}", rsdp_response.revision());
        let ptr: *const u8 = rsdp_response.address() as *const u8;
        println!("RSDP ADDRESS: {:p}", ptr);
    }
}
