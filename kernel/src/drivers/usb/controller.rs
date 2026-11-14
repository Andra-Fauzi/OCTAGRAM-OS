use super::{device, xhci};
use crate::drivers::pci;
use crate::println;
use spin::Mutex;
use xhci::XHCI;

const PCI_CLASS_USB: u8 = 0x0C;
const PCI_SUBCLASS_USB: u8 = 0x03;
const PCI_PROG_IF_UHCI: u8 = 0x00;
const PCI_PROG_IF_OHCI: u8 = 0x10;
const PCI_PROG_IF_EHCI: u8 = 0x20;
const PCI_PROG_IF_XHCI: u8 = 0x30;

/// Initialize supported USB host controllers from the PCI scan snapshot.
///
/// # Safety
/// Requires initialized PCI, memory, and interrupt subsystems.
pub unsafe fn init_usb_controllers(pci_devices: &[pci::PciDevice]) {
    unsafe {
        for controller in find_usb_controllers(pci_devices) {
            let kind = match controller.prog_if {
                PCI_PROG_IF_UHCI => "UHCI",
                PCI_PROG_IF_OHCI => "OHCI",
                PCI_PROG_IF_EHCI => "EHCI",
                PCI_PROG_IF_XHCI => "xHCI",
                _ => "Unknown",
            };
            println!(
                "USB Controller found: {} at {:02x}:{:02x}.{}",
                kind, controller.bus, controller.device, controller.function
            );

            if controller.prog_if == PCI_PROG_IF_XHCI {
                init_xhci_controller(controller);
            }
        }
    }
}

fn find_usb_controllers(devices: &[pci::PciDevice]) -> impl Iterator<Item = &pci::PciDevice> {
    devices
        .iter()
        .filter(|device| device.class == PCI_CLASS_USB && device.subclass == PCI_SUBCLASS_USB)
}

/// Initialize one xHCI controller and publish it when MSI-X setup succeeds.
///
/// If MSI-X is unavailable, this retains the existing polling fallback.
unsafe fn init_xhci_controller(controller: &pci::PciDevice) {
    unsafe {
        pci::enable_device(controller);
        let bar = pci::get_bar0(controller);
        println!(
            "xHCI BAR0: phys={:#x} 64bit={} prefetchable={}",
            bar.address, bar.is_64bit, bar.is_prefetchable
        );

        let mut xhci_device = XHCI::new(bar, controller);
        let msix_enabled = xhci_device.setup_msix(controller, 44, 0);

        xhci::init_xhci(&mut xhci_device);
        xhci::scan_ports(&xhci_device);

        if msix_enabled {
            xhci::XHCI_INSTANCE.call_once(|| Mutex::new(xhci_device));
            core::arch::asm!("cli");
            if let Some(instance) = xhci::XHCI_INSTANCE.get() {
                instance.lock().poll_event_ring();
            }
            core::arch::asm!("sti");
            println!("Interrupt-driven mode aktif (MSI-X)");
            device::discover_and_init_hid_devices();
        } else {
            println!("Fallback ke polling manual...");
            loop {
                xhci_device.poll_event_ring();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PCI_CLASS_USB, PCI_PROG_IF_UHCI, PCI_PROG_IF_XHCI, PCI_SUBCLASS_USB, find_usb_controllers,
    };
    use crate::drivers::pci::PciDevice;

    fn device(class: u8, subclass: u8, prog_if: u8) -> PciDevice {
        PciDevice {
            bus: 0,
            device: 1,
            function: 0,
            vendor_id: 0x1234,
            device_id: 0x5678,
            class,
            subclass,
            prog_if,
            header_type: 0,
        }
    }

    #[test]
    fn finds_usb_host_controllers_regardless_of_programming_interface() {
        let devices = [
            device(PCI_CLASS_USB, PCI_SUBCLASS_USB, PCI_PROG_IF_XHCI),
            device(PCI_CLASS_USB, PCI_SUBCLASS_USB, PCI_PROG_IF_UHCI),
            device(PCI_CLASS_USB, 0x02, PCI_PROG_IF_XHCI),
        ];

        assert_eq!(find_usb_controllers(&devices).count(), 2);
    }
}
