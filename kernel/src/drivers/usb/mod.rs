//! USB host-controller and device-class drivers.
//!
//! `controller` owns PCI matching and host-controller startup. `device` owns
//! USB enumeration, class dispatch, and hotplug cleanup. Controller-neutral
//! USB descriptors are shared by the xHCI, EHCI, and OHCI transports.

mod controller;
pub(crate) mod descriptors;
mod device;
pub mod ehci;
mod hid_periodic;
pub mod ohci;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UsbControllerKind {
    Xhci,
    Ehci,
    Ohci,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UsbControllerId {
    pub kind: UsbControllerKind,
    pub instance: u8,
}

impl UsbControllerId {
    pub const XHCI: Self = Self {
        kind: UsbControllerKind::Xhci,
        instance: 0,
    };
}

#[derive(Debug, Clone, Copy)]
pub struct EndpointInfo {
    pub address: u8,
    pub attributes: u8,
    pub max_packet_size: u16,
    pub interval: u8,
    pub inteface_number: u8,
    pub interface_subclass: u8,
    pub interface_class: u8,
    pub interface_protocol: u8,
}

pub(crate) enum UsbPortEvent {
    Connected {
        root_port: u8,
        address: u8,
        configuration_descriptor: alloc::vec::Vec<u8>,
    },
    Disconnected {
        root_port: u8,
        address: u8,
        configuration_descriptor: alloc::vec::Vec<u8>,
    },
}

pub mod keyboard_usb;
pub mod mass_storage;
pub mod mouse_usb;
pub mod rndis;
pub mod xhci;

pub(crate) use controller::{EHCI_DRIVER, OHCI_DRIVER, XHCI_DRIVER};
pub use device::poll_hotplug;

#[cfg(test)]
mod tests {
    use super::{UsbControllerId, UsbControllerKind};

    #[test]
    fn controller_id_distinguishes_host_controller_kinds_and_instances() {
        let xhci = UsbControllerId::XHCI;
        let ehci = UsbControllerId {
            kind: UsbControllerKind::Ehci,
            instance: 0,
        };
        let second_ehci = UsbControllerId {
            kind: UsbControllerKind::Ehci,
            instance: 1,
        };

        assert_ne!(xhci, ehci);
        assert_ne!(ehci, second_ehci);
    }
}

pub(crate) unsafe fn register_class_drivers() {
    unsafe { device::register_class_drivers() };
}
