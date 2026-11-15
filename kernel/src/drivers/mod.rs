//! Driver subsystem: ACPI, PCI, storage, keyboard, serial, and USB drivers.

pub mod acpi;
pub mod ahci;
pub mod device_model;
pub mod keyboard;
pub mod pci;
pub mod qca9377;
pub mod rtc;
pub mod serial;
pub mod usb;
pub mod wifi_firmware;

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
    if let Err(error) = unsafe { device_model::register_driver(&ahci::AHCI_DRIVER) } {
        crate::println!("driver ahci registration failed: {error}");
    }
    unsafe { usb::register_class_drivers() };

    let pci_devices = pci::scan_pci_bus();
    qca9377::report_pci_candidates(&pci_devices);
    for device in pci_devices {
        let result =
            unsafe { device_model::register_device(device_model::DeviceInfo::Pci(device)) };
        if let Err(error) = result {
            crate::println!("PCI device registration failed: {error}");
        }
    }

    if usb::XHCI_ENABLED {
        if let Err(error) = unsafe { device_model::register_driver(&usb::XHCI_DRIVER) } {
            crate::println!("driver xhci registration failed: {error}");
        }
    } else {
        crate::println!("xHCI dinonaktifkan sementara (usb::XHCI_ENABLED = false)");
    }
    if let Err(error) = unsafe { device_model::register_driver(&usb::EHCI_DRIVER) } {
        crate::println!("driver ehci registration failed: {error}");
    }
    if let Err(error) = unsafe { device_model::register_driver(&usb::OHCI_DRIVER) } {
        crate::println!("driver ohci registration failed: {error}");
    }
}

/// Process deferred driver work that must run outside interrupt handlers.
///
/// # Safety
/// Must be called from the idle loop after interrupt handlers have returned.
pub(crate) unsafe fn poll_deferred() {
    if usb::XHCI_ENABLED {
        usb::xhci::poll_manual_events();
    }
    unsafe { usb::poll_hotplug() };
    unsafe { usb::rndis::poll() };
}
