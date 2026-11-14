use super::{device, xhci};
use crate::drivers::device_model::{DeviceDriver, DeviceInfo, DriverError, DriverState};
use crate::drivers::pci;
use crate::println;
use alloc::sync::Arc;
use spin::Mutex;
use xhci::XHCI;

const PCI_CLASS_USB: u8 = 0x0C;
const PCI_SUBCLASS_USB: u8 = 0x03;
const PCI_PROG_IF_XHCI: u8 = 0x30;

pub(crate) struct XhciControllerDriver;
pub(crate) static XHCI_DRIVER: XhciControllerDriver = XhciControllerDriver;

impl DeviceDriver for XhciControllerDriver {
    fn name(&self) -> &'static str {
        "xhci"
    }

    fn matches(&self, device: &DeviceInfo) -> bool {
        matches!(device, DeviceInfo::Pci(device)
            if device.class == PCI_CLASS_USB
                && device.subclass == PCI_SUBCLASS_USB
                && device.prog_if == PCI_PROG_IF_XHCI)
    }

    unsafe fn probe(&self, device: &DeviceInfo) -> Result<DriverState, DriverError> {
        let DeviceInfo::Pci(controller) = device else {
            return Err(DriverError::Unsupported);
        };
        unsafe { init_xhci_controller(controller) };
        Ok(Arc::new(()))
    }

    unsafe fn remove(&self, _device: &DeviceInfo, _state: DriverState) {}
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
    use super::{PCI_CLASS_USB, PCI_PROG_IF_XHCI, PCI_SUBCLASS_USB, XHCI_DRIVER};
    use crate::drivers::device_model::{DeviceDriver, DeviceInfo};
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
    fn matches_only_xhci_host_controllers() {
        let devices = [
            device(PCI_CLASS_USB, PCI_SUBCLASS_USB, PCI_PROG_IF_XHCI),
            device(PCI_CLASS_USB, PCI_SUBCLASS_USB, 0x00),
            device(PCI_CLASS_USB, 0x02, PCI_PROG_IF_XHCI),
        ];

        assert!(XHCI_DRIVER.matches(&DeviceInfo::Pci(devices[0])));
        assert!(!XHCI_DRIVER.matches(&DeviceInfo::Pci(devices[1])));
        assert!(!XHCI_DRIVER.matches(&DeviceInfo::Pci(devices[2])));
    }
}
