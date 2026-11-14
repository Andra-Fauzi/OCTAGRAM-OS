//! Limine-provided ACPI Root System Description Pointer.

use limine::request::RsdpRequest;

use crate::println;

#[used]
#[unsafe(link_section = ".requests")]
static RSDP_REQUEST: RsdpRequest = RsdpRequest::new();

/// Return the physical address of the firmware-provided RSDP.
pub fn address() -> Option<u64> {
    RSDP_REQUEST
        .get_response()
        .map(|response| response.address() as u64)
}

pub fn test_rsdp() {
    match (RSDP_REQUEST.get_response(), address()) {
        (Some(response), Some(address)) => {
            println!("ACPI RSDP revision {} at {address:#x}", response.revision());
        }
        _ => println!("ACPI RSDP unavailable"),
    }
}
