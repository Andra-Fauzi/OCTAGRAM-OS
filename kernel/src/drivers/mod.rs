//! Driver subsystem: ACPI, PCI, storage, keyboard, serial, and USB drivers.

pub mod acpi;
pub mod ahci;
pub mod keyboard;
pub mod pci;
pub mod rtc;
pub mod serial;
pub mod usb;

/// Enable early boot diagnostics before the framebuffer and bus drivers start.
pub(crate) fn early_init() {
    serial::init();
}

/// Initialize bus discovery and the currently supported hardware drivers.
///
/// # Safety
/// Call once after memory, filesystem, interrupt, and APIC initialization,
/// with interrupts enabled and outside interrupt context.
pub(crate) unsafe fn initialize() {
    match acpi::init() {
        Ok(()) => crate::println!("[acpi] shutdown support ready"),
        Err(error) => crate::println!("[acpi] shutdown support unavailable: {error:?}"),
    }
    let pci_devices = pci::scan_pci_bus();
    unsafe {
        ahci::init(&pci_devices);
        usb::init_usb_controllers(&pci_devices);
    }
}

/// Process deferred driver work that must run outside interrupt handlers.
///
/// # Safety
/// Must be called from the idle loop after interrupt handlers have returned.
pub(crate) unsafe fn poll_deferred() {
    unsafe { usb::poll_hotplug() };
}
