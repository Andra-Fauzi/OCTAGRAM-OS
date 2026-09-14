#![no_std]
#![no_main]

use core::arch::asm;

use limine::BaseRevision;
use limine::request::{FramebufferRequest, RequestsEndMarker, RequestsStartMarker};

use crate::graphics::write_pixel;
use crate::memory::{
    stress_test_heap, test_frame_allocator, test_get_request_memory_map,
    test_get_request_memory_map_usable, test_heap_allocator,
};
use crate::xhci::{XHCI, XhciOpRegs, init_xhci, scan_ports, test_xhci};

/// Sets the base revision to the latest revision supported by the crate.
/// See specification for further info.
/// Be sure to mark all limine requests with #[used], otherwise they may be removed by the compiler.
#[used]
// The .requests section allows limine to find the requests faster and more safely.
#[unsafe(link_section = ".requests")]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
#[unsafe(link_section = ".requests")]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

/// Define the stand and end markers for Limine requests.
#[used]
#[unsafe(link_section = ".requests_start_marker")]
static _START_MARKER: RequestsStartMarker = RequestsStartMarker::new();
#[used]
#[unsafe(link_section = ".requests_end_marker")]
static _END_MARKER: RequestsEndMarker = RequestsEndMarker::new();

mod apic;
mod gdt;
mod graphics;
mod idt;
mod interrupt;
mod io;
mod keyboard_usb;
mod memory;
mod pci;
mod rsdp;
mod terminal;
mod xhci;

extern crate alloc;

use alloc::boxed::Box;

#[unsafe(no_mangle)]
unsafe extern "C" fn kmain() -> ! {
    // All limine requests must also be referenced in a called function, otherwise they may be
    // removed by the linker.
    assert!(BASE_REVISION.is_supported());

    if let Some(framebuffer_response) = FRAMEBUFFER_REQUEST.get_response() {
        if let Some(framebuffer) = framebuffer_response.framebuffers().next() {
            for i in 0..100_u64 {
                // Calculate the pixel offset using the framebuffer information we obtained above.
                // We skip `i` scanlines (pitch is provided in bytes) and add `i * 4` to skip `i` pixels forward.
                let pixel_offset = i * framebuffer.pitch() + i * 4;

                // Write 0xFFFFFFFF to the provided pixel offset to fill it white.
                unsafe {
                    framebuffer
                        .addr()
                        .add(pixel_offset as usize)
                        .cast::<u32>()
                        .write(0xFFFFFFFF)
                };
            }
        }
    }
    asm!("cli");

    gdt::load_gdt();
    println!("GDT LOADED");
    idt::load_idt();
    println!("IDT LOADED");
    println!("TES RSDP");
    rsdp::test_rsdp();
    apic::disable_pic();
    apic::LAPIC.call_once(|| apic::init_lapic());
    let ioapic = apic::init_ioapic();

    asm!("sti");
    memory::test_get_request_memory_map_usable();
    memory::init();
    pci::scan_pci_bus();

    let usb_controllers = pci::find_usb_controllers();
    for ctrl in &usb_controllers {
        let kind = match ctrl.prog_if {
            0x00 => "UHCI",
            0x10 => "OHCI",
            0x20 => "EHCI",
            0x30 => "xHCI",
            _ => "Unknown",
        };
        println!(
            "USB Controller found: {} at {:02x}:{:02x}.{}",
            kind, ctrl.bus, ctrl.device, ctrl.function
        );

        if ctrl.prog_if == 0x30 {
            // xHCI ditemukan — enable & baca BAR0
            unsafe {
                pci::enable_device(ctrl);
                let bar = pci::get_bar0(ctrl);
                println!(
                    "xHCI BAR0: phys={:#x} 64bit={} prefetchable={}",
                    bar.address, bar.is_64bit, bar.is_prefetchable
                );
                // the controller
                let mut xhci = XHCI::new(bar, ctrl);

                let msix_ok = xhci.setup_msix(ctrl, 44, 0);

                //pci::enable_msi(ctrl, 44, 0);
                init_xhci(&mut xhci);
                scan_ports(&xhci);
                if msix_ok {
                    xhci::XHCI_INSTANCE.call_once(|| spin::Mutex::new(xhci));
                    println!("Interrupt-driven mode aktif (MSI-X)");

                    println!("Sending enable slot command...");
                    let slot_id = xhci::enable_slot().unwrap();
                    println!("enable_slot returned: {:?}", slot_id);
                    xhci::address_device(slot_id, 5, 3).unwrap();
                    xhci::get_device_descriptor(slot_id).unwrap();
                    let ep_info =
                        xhci::get_configuration_descriptor_and_find_interrupt_in(slot_id).unwrap();
                    println!(
                        "Interrupt IN endpoint ketemu: addr={:#04x} max_packet_size={} interval={}",
                        ep_info.address, ep_info.max_packet_size, ep_info.interval
                    );
                    xhci::control_transfer_no_data(slot_id, 0x00, 0x09, 1, 0).unwrap();
                    let ep = xhci::configure_endpoint(slot_id, ep_info).unwrap();
                    println!("Endpoint interrupt siap, device sudah configured!");
                    keyboard_usb::init_keyboard(slot_id, ep, ep_info.inteface_number);
                } else {
                    println!("Fallback ke polling manual...");
                    loop {
                        xhci.poll_event_ring();
                    }
                }
            }
        }
    }

    hcf();
}

#[panic_handler]
fn rust_panic(_info: &core::panic::PanicInfo) -> ! {
    print!("\n{:#?}\n", _info);
    hcf();
}

fn hcf() -> ! {
    loop {
        unsafe {
            #[cfg(target_arch = "x86_64")]
            asm!("hlt");
            #[cfg(any(target_arch = "aarch64", target_arch = "riscv64"))]
            asm!("wfi");
            #[cfg(target_arch = "loongarch64")]
            asm!("idle 0");
        }
    }
}
