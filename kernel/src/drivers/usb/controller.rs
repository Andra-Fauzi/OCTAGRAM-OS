use super::{UsbControllerId, UsbControllerKind, UsbPortEvent};
use super::{device, ehci, ohci, xhci};
use crate::drivers::device_model::{DeviceDriver, DeviceInfo, DriverError, DriverState};
use crate::drivers::pci;
use crate::println;
use alloc::sync::Arc;
use ehci::EhciController;
use ohci::OhciController;
use spin::Mutex;
use xhci::XHCI;

const PCI_CLASS_USB: u8 = 0x0C;
const PCI_SUBCLASS_USB: u8 = 0x03;
const PCI_PROG_IF_XHCI: u8 = 0x30;

pub(crate) struct XhciControllerDriver;
pub(crate) static XHCI_DRIVER: XhciControllerDriver = XhciControllerDriver;

pub(crate) struct EhciControllerDriver;
pub(crate) static EHCI_DRIVER: EhciControllerDriver = EhciControllerDriver;
pub(crate) struct OhciControllerDriver;
pub(crate) static OHCI_DRIVER: OhciControllerDriver = OhciControllerDriver;

impl DeviceDriver for EhciControllerDriver {
    fn name(&self) -> &'static str {
        "ehci"
    }

    fn matches(&self, device: &DeviceInfo) -> bool {
        matches!(device, DeviceInfo::Pci(device) if ehci::matches(device))
    }

    unsafe fn probe(&self, device: &DeviceInfo) -> Result<DriverState, DriverError> {
        let DeviceInfo::Pci(controller) = device else {
            return Err(DriverError::Unsupported);
        };
        let instance = ehci::EHCI_CONTROLLERS.lock().len();
        let instance_id = u8::try_from(instance)
            .map_err(|_| DriverError::ProbeFailed("too many EHCI controllers"))?;
        let mut controller = unsafe { EhciController::new(controller) }
            .map_err(|_| DriverError::ProbeFailed("EHCI controller initialization failed"))?;
        controller.scan_ports();
        let controller_id = UsbControllerId {
            kind: UsbControllerKind::Ehci,
            instance: instance_id,
        };
        let device_interfaces = controller
            .devices()
            .iter()
            .filter_map(|usb_device| {
                match super::descriptors::parse_configuration_endpoints(
                    &usb_device.configuration_descriptor,
                ) {
                    Ok(endpoints) => Some((usb_device.root_port, usb_device.address, endpoints)),
                    Err(error) => {
                        crate::println!(
                            "EHCI address {} configuration parse failed: {}",
                            usb_device.address,
                            error
                        );
                        None
                    }
                }
            })
            .collect::<alloc::vec::Vec<_>>();
        let mut controllers = ehci::EHCI_CONTROLLERS.lock();
        if controllers.len() != usize::from(instance_id) {
            return Err(DriverError::ProbeFailed(
                "EHCI controller instance changed during probe",
            ));
        }
        let port_count = controller.port_count();
        let device_count = controller.devices().len();
        controllers.push(controller);
        drop(controllers);
        crate::println!(
            "EHCI controller {} initialized with {} root ports and {} devices",
            instance,
            port_count,
            device_count
        );
        for (root_port, address, endpoints) in device_interfaces {
            device::register_usb_interfaces(
                controller_id,
                u32::from(address),
                root_port,
                &endpoints,
            );
        }
        Ok(Arc::new(()))
    }

    unsafe fn remove(&self, _device: &DeviceInfo, _state: DriverState) {}
}

impl DeviceDriver for OhciControllerDriver {
    fn name(&self) -> &'static str {
        "ohci"
    }

    fn matches(&self, device: &DeviceInfo) -> bool {
        matches!(device, DeviceInfo::Pci(device) if ohci::matches(device))
    }

    unsafe fn probe(&self, device: &DeviceInfo) -> Result<DriverState, DriverError> {
        let DeviceInfo::Pci(controller) = device else {
            return Err(DriverError::Unsupported);
        };
        let instance = ohci::OHCI_CONTROLLERS.lock().len();
        let instance_id = u8::try_from(instance)
            .map_err(|_| DriverError::ProbeFailed("too many OHCI controllers"))?;
        let mut controller = unsafe { OhciController::new(controller) }
            .map_err(|_| DriverError::ProbeFailed("OHCI controller initialization failed"))?;
        controller.scan_ports();
        let controller_id = UsbControllerId {
            kind: UsbControllerKind::Ohci,
            instance: instance_id,
        };
        let device_interfaces = controller
            .devices()
            .iter()
            .filter_map(|usb_device| {
                match super::descriptors::parse_configuration_endpoints(
                    &usb_device.configuration_descriptor,
                ) {
                    Ok(endpoints) => Some((usb_device.root_port, usb_device.address, endpoints)),
                    Err(error) => {
                        crate::println!(
                            "OHCI address {} configuration parse failed: {}",
                            usb_device.address,
                            error
                        );
                        None
                    }
                }
            })
            .collect::<alloc::vec::Vec<_>>();
        let mut controllers = ohci::OHCI_CONTROLLERS.lock();
        if controllers.len() != usize::from(instance_id) {
            return Err(DriverError::ProbeFailed(
                "OHCI controller instance changed during probe",
            ));
        }
        let port_count = controller.port_count();
        let device_count = controller.devices().len();
        controllers.push(controller);
        drop(controllers);
        crate::println!(
            "OHCI controller {} initialized with {} root ports and {} devices",
            instance_id,
            port_count,
            device_count
        );
        for (root_port, address, endpoints) in device_interfaces {
            device::register_usb_interfaces(
                controller_id,
                u32::from(address),
                root_port,
                &endpoints,
            );
        }
        Ok(Arc::new(()))
    }

    unsafe fn remove(&self, _device: &DeviceInfo, _state: DriverState) {}
}

pub(super) fn poll_non_xhci_ports() {
    let mut events = alloc::vec::Vec::new();
    {
        let mut controllers = ehci::EHCI_CONTROLLERS.lock();
        for (instance, controller) in controllers.iter_mut().enumerate() {
            let Ok(instance) = u8::try_from(instance) else {
                continue;
            };
            let controller_id = UsbControllerId {
                kind: UsbControllerKind::Ehci,
                instance,
            };
            events.extend(
                controller
                    .poll_ports()
                    .into_iter()
                    .map(|event| (controller_id, event)),
            );
        }
    }
    {
        let mut controllers = ohci::OHCI_CONTROLLERS.lock();
        for (instance, controller) in controllers.iter_mut().enumerate() {
            let Ok(instance) = u8::try_from(instance) else {
                continue;
            };
            let controller_id = UsbControllerId {
                kind: UsbControllerKind::Ohci,
                instance,
            };
            events.extend(
                controller
                    .poll_ports()
                    .into_iter()
                    .map(|event| (controller_id, event)),
            );
        }
    }

    for (controller_id, event) in events {
        match event {
            UsbPortEvent::Connected {
                root_port,
                address,
                configuration_descriptor,
            } => match super::descriptors::parse_configuration_endpoints(&configuration_descriptor)
            {
                Ok(endpoints) => device::register_usb_interfaces(
                    controller_id,
                    u32::from(address),
                    root_port,
                    &endpoints,
                ),
                Err(error) => println!(
                    "{:?} address {} configuration parse failed: {}",
                    controller_id.kind, address, error
                ),
            },
            UsbPortEvent::Disconnected {
                root_port,
                address,
                configuration_descriptor,
            } => match super::descriptors::parse_configuration_endpoints(&configuration_descriptor)
            {
                Ok(endpoints) => device::unregister_usb_interfaces(
                    controller_id,
                    u32::from(address),
                    root_port,
                    &endpoints,
                ),
                Err(error) => println!(
                    "{:?} address {} configuration parse failed during disconnect: {}",
                    controller_id.kind, address, error
                ),
            },
        }
    }
}

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

/// Initialize one xHCI controller and publish it before device enumeration.
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

        xhci::init_xhci(&mut xhci_device, msix_enabled);
        xhci::scan_ports(&xhci_device);

        xhci::XHCI_INSTANCE.call_once(|| Mutex::new(xhci_device));
        if msix_enabled {
            core::arch::asm!("cli");
            if let Some(instance) = xhci::XHCI_INSTANCE.get() {
                instance.lock().poll_event_ring();
            }
            core::arch::asm!("sti");
            println!("Interrupt-driven mode aktif (MSI-X)");
        } else {
            xhci::MANUAL_POLLING.store(true, core::sync::atomic::Ordering::SeqCst);
            println!("Polling mode aktif (MSI-X tidak tersedia)");
        }
        device::discover_and_init_hid_devices();
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
