//! USB host-controller and device-class drivers.
//!
//! `controller` owns PCI matching and xHCI startup. `device` owns USB
//! enumeration, class dispatch, and hotplug cleanup. `xhci` is the transport
//! layer; keyboard, mouse, and mass-storage modules implement device classes.

mod controller;
mod device;

pub mod keyboard_usb;
pub mod mass_storage;
pub mod mouse_usb;
pub mod xhci;

pub(crate) use controller::XHCI_DRIVER;
pub use device::poll_hotplug;

pub(crate) unsafe fn register_class_drivers() {
    unsafe { device::register_class_drivers() };
}
