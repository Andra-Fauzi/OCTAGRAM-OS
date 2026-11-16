//! Device enumeration, class dispatch, and USB hotplug lifecycle.

use super::{
    UsbControllerId, UsbControllerKind, controller, hid_periodic, keyboard_usb, mass_storage,
    mouse_usb, ncm, rndis, xhci,
};
use crate::drivers::device_model::{
    self, DeviceDriver, DeviceInfo, DeviceKey, DriverError, DriverState, UsbInterfaceInfo,
};
use crate::println;
use alloc::sync::Arc;
use core::sync::atomic::Ordering;
use spin::Mutex;

struct ActivePort {
    root_port: u8,
    slot_id: u32,
    interfaces: alloc::vec::Vec<u8>,
}

unsafe fn init_ohci_mass_storage(interface: &UsbInterfaceInfo) -> bool {
    let bulk_in = find_mass_storage_endpoint(&interface.endpoints, interface.number, true);
    let bulk_out = find_mass_storage_endpoint(&interface.endpoints, interface.number, false);
    let (Some(bulk_in), Some(bulk_out)) = (bulk_in, bulk_out) else {
        return false;
    };

    let Ok(device_address) = u8::try_from(interface.slot_id) else {
        return false;
    };
    match unsafe {
        mass_storage::init_ohci_storage(interface.controller, device_address, bulk_in, bulk_out)
    } {
        Ok(_) => true,
        Err(error) => {
            println!("  OHCI USB storage gagal diinisialisasi: {:?}", error);
            false
        }
    }
}

static ACTIVE_PORTS: Mutex<alloc::vec::Vec<ActivePort>> = Mutex::new(alloc::vec::Vec::new());

pub(super) unsafe fn register_class_drivers() {
    for driver in [
        &HID_KEYBOARD_DRIVER as &dyn DeviceDriver,
        &HID_MOUSE_DRIVER,
        &MASS_STORAGE_DRIVER,
        &RNDIS_DRIVER,
        &NCM_DRIVER,
    ] {
        if let Err(error) = unsafe { device_model::register_driver(driver) } {
            println!("driver {} registration failed: {error}", driver.name());
        }
    }
}

/// Process pending port changes outside the xHCI interrupt handler.
///
/// # Safety
/// Must be called after the interrupt handler has returned.
pub unsafe fn poll_hotplug() {
    unsafe {
        if xhci::PORT_CHANGE_PENDING.swap(false, Ordering::SeqCst)
            && xhci::XHCI_INSTANCE.get().is_some()
        {
            println!("Port Status Change terdeteksi, sinkronisasi hotplug...");
            discover_and_init_hid_devices();
        }
        controller::poll_non_xhci_ports();
        hid_periodic::poll();
    }
}

/// Reconcile the active USB ports with the controller's current port state.
///
/// # Safety
/// Requires an initialized xHCI controller and must not run in its ISR.
pub(super) unsafe fn discover_and_init_hid_devices() {
    unsafe {
        let port_states = {
            let controller = xhci::XHCI_INSTANCE.get().unwrap().lock();
            let capabilities = controller.cap_regs();
            let operational = controller.op_regs();
            let mut states = alloc::vec::Vec::new();

            for port in 0..capabilities.max_ports() {
                let port_status = operational.read_portsc(port);
                operational.acknowledge_port_changes(port, port_status);
                if port_status & 0x1 != 0 {
                    let speed = ((port_status >> 10) & 0xF) as u8;
                    states.push((port, speed));
                }
            }
            states
        };

        println!(
            "USB discovery: {} port terhubung ditemukan",
            port_states.len()
        );
        disconnect_removed_ports(&port_states);
        connect_new_ports(&port_states);
    }
}

unsafe fn disconnect_removed_ports(port_states: &[(u8, u8)]) {
    let mut active_ports = ACTIVE_PORTS.lock();
    let mut index = 0;

    while index < active_ports.len() {
        let root_port = active_ports[index].root_port;
        let still_connected = port_states
            .iter()
            .any(|&(port_index, _)| port_index + 1 == root_port);
        if still_connected {
            index += 1;
            continue;
        }

        if unsafe { shutdown_device(&active_ports[index]) } {
            active_ports.remove(index);
        } else {
            index += 1;
        }
    }
}

unsafe fn connect_new_ports(port_states: &[(u8, u8)]) {
    let mut active_ports = ACTIVE_PORTS.lock();
    for &(port_index, speed) in port_states {
        let root_port = port_index + 1;
        if active_ports
            .iter()
            .any(|active| active.root_port == root_port)
        {
            continue;
        }

        if let Some(active) = unsafe { init_one_device(root_port, speed) } {
            active_ports.push(active);
        }
    }
}

unsafe fn shutdown_device(device: &ActivePort) -> bool {
    unsafe {
        println!(
            "--- Device di root port {} (slot={}) disconnect ---",
            device.root_port, device.slot_id
        );

        if let Err(error) = xhci::disable_slot(device.slot_id) {
            println!("  disable_slot gagal: {}", error);
            return false;
        }

        for &interface_number in &device.interfaces {
            let key = DeviceKey::UsbInterface {
                controller: UsbControllerId::XHCI,
                slot_id: device.slot_id,
                interface_number,
            };
            if let Err(error) = device_model::remove_device(key) {
                println!("  USB interface cleanup failed: {error}");
            }
        }
        true
    }
}

unsafe fn init_one_device(root_port: u8, speed: u8) -> Option<ActivePort> {
    unsafe {
        println!(
            "--- Enumerasi device di root port {} (speed={}) ---",
            root_port, speed
        );
        let speed = match xhci::reset_port(root_port) {
            Ok(speed) => speed,
            Err(error) => {
                println!("  USB port reset gagal: {}", error);
                return None;
            }
        };
        let slot_id = match xhci::enable_slot() {
            Ok(slot_id) => slot_id,
            Err(error) => {
                println!("  enable_slot gagal: {}", error);
                return None;
            }
        };

        if let Err(error) = xhci::address_device(slot_id, root_port, speed) {
            println!("  address_device gagal: {}", error);
            return None;
        }
        if let Err(error) = xhci::get_device_descriptor(slot_id) {
            println!("  get_device_descriptor gagal: {}", error);
            return unsafe { abandon_slot(root_port, slot_id) };
        }

        let endpoints = match xhci::get_configuration_descriptor_endpoints(slot_id) {
            Ok(endpoints) => endpoints,
            Err(error) => {
                println!("  tidak ada endpoint konfigurasi: {}", error);
                return unsafe { abandon_slot(root_port, slot_id) };
            }
        };
        if let Err(error) = xhci::control_transfer_no_data(slot_id, 0x00, 0x09, 1, 0) {
            println!("  SET_CONFIGURATION gagal: {}", error);
            return unsafe { abandon_slot(root_port, slot_id) };
        }

        let interfaces = group_interfaces(UsbControllerId::XHCI, slot_id, root_port, &endpoints);
        let mut interface_numbers = alloc::vec::Vec::new();
        for interface in interfaces {
            let interface_number = interface.number;
            match device_model::register_device(DeviceInfo::UsbInterface(interface)) {
                Ok(()) => interface_numbers.push(interface_number),
                Err(error) => {
                    println!(
                        "  USB interface {} registration failed: {error}",
                        interface_number
                    );
                }
            }
        }

        Some(active_port(root_port, slot_id, interface_numbers))
    }
}

/// Give a slot back when a device cannot be brought up.
///
/// Every enable-slot that is never disabled stays allocated in the controller,
/// so a port that keeps failing enumeration (a phone that resets its gadget, a
/// flaky cable) burned a new slot on every port-status change until the 32-slot
/// table was exhausted and no USB device could enumerate at all. Returning
/// `None` also stops tracking the port, so the next port change simply retries
/// with the lowest free slot instead of stacking failures.
unsafe fn abandon_slot(root_port: u8, slot_id: u32) -> Option<ActivePort> {
    unsafe {
        match xhci::disable_slot(slot_id) {
            Ok(()) => println!("  slot {} dikembalikan (enumerasi port {} gagal)", slot_id, root_port),
            Err(error) => println!("  disable_slot slot {} gagal: {}", slot_id, error),
        }
    }
    None
}

fn active_port(root_port: u8, slot_id: u32, interfaces: alloc::vec::Vec<u8>) -> ActivePort {
    ActivePort {
        root_port,
        slot_id,
        interfaces,
    }
}

fn group_interfaces(
    controller: UsbControllerId,
    slot_id: u32,
    root_port: u8,
    endpoints: &[xhci::EndpointInfo],
) -> alloc::vec::Vec<UsbInterfaceInfo> {
    let mut interfaces: alloc::vec::Vec<UsbInterfaceInfo> = alloc::vec::Vec::new();
    for endpoint in endpoints.iter().copied() {
        if let Some(interface) = interfaces.iter_mut().find(|interface| {
            interface.number == endpoint.inteface_number
                && interface.class == endpoint.interface_class
                && interface.subclass == endpoint.interface_subclass
                && interface.protocol == endpoint.interface_protocol
        }) {
            interface.endpoints.push(endpoint);
        } else {
            interfaces.push(UsbInterfaceInfo {
                controller,
                slot_id,
                root_port,
                number: endpoint.inteface_number,
                class: endpoint.interface_class,
                subclass: endpoint.interface_subclass,
                protocol: endpoint.interface_protocol,
                endpoints: alloc::vec![endpoint],
                configuration_endpoints: alloc::vec::Vec::new(),
            });
        }
    }
    for interface in &mut interfaces {
        interface.configuration_endpoints = endpoints.to_vec();
    }
    interfaces
}

pub(super) fn register_usb_interfaces(
    controller: UsbControllerId,
    slot_id: u32,
    root_port: u8,
    endpoints: &[xhci::EndpointInfo],
) {
    for interface in group_interfaces(controller, slot_id, root_port, endpoints) {
        let number = interface.number;
        if let Err(error) =
            unsafe { device_model::register_device(DeviceInfo::UsbInterface(interface)) }
        {
            println!("  USB interface {} registration failed: {error}", number);
        }
    }
}

pub(super) fn unregister_usb_interfaces(
    controller: UsbControllerId,
    slot_id: u32,
    root_port: u8,
    endpoints: &[xhci::EndpointInfo],
) {
    let interfaces = group_interfaces(controller, slot_id, root_port, endpoints);
    for interface in interfaces {
        let key = DeviceKey::UsbInterface {
            controller,
            slot_id,
            interface_number: interface.number,
        };
        match unsafe { device_model::remove_device(key) } {
            Ok(true) => {}
            Ok(false) => println!(
                "  USB interface {} at port {} was already removed",
                interface.number, root_port
            ),
            Err(error) => println!("  USB interface cleanup failed: {error}"),
        }
    }
}

unsafe fn dispatch_endpoint(
    slot_id: u32,
    root_port: u8,
    endpoint: xhci::EndpointInfo,
) -> Result<(), DriverError> {
    if endpoint.interface_class != 3
        || endpoint.attributes & 0x3 != 0x3
        || endpoint.address & 0x80 == 0
    {
        return Err(DriverError::Unsupported);
    }

    let handle = match unsafe { xhci::configure_endpoint(slot_id, root_port, endpoint) } {
        Ok(handle) => handle,
        Err(error) => {
            println!("  configure_endpoint gagal: {}", error);
            return Err(DriverError::ProbeFailed(
                "HID endpoint configuration failed",
            ));
        }
    };

    match endpoint.interface_protocol {
        1 => {
            unsafe {
                keyboard_usb::init_keyboard(slot_id, handle, endpoint.inteface_number);
            }
            Ok(())
        }
        2 => {
            unsafe {
                mouse_usb::init_mouse(slot_id, handle, endpoint.inteface_number);
            }
            Ok(())
        }
        protocol => {
            println!(
                "  HID interface protocol {:#04x} belum ada driver-nya, dilewati",
                protocol
            );
            Err(DriverError::Unsupported)
        }
    }
}

unsafe fn init_mass_storage_interfaces(
    slot_id: u32,
    root_port: u8,
    endpoints: &[xhci::EndpointInfo],
) -> bool {
    let mut interfaces = alloc::vec::Vec::new();
    for endpoint in endpoints {
        if is_mass_storage_bulk_endpoint(*endpoint)
            && !interfaces.contains(&endpoint.inteface_number)
        {
            interfaces.push(endpoint.inteface_number);
        }
    }

    let mut initialized = false;
    for interface in interfaces {
        let bulk_in = find_mass_storage_endpoint(endpoints, interface, true);
        let bulk_out = find_mass_storage_endpoint(endpoints, interface, false);
        let (Some(bulk_in), Some(bulk_out)) = (bulk_in, bulk_out) else {
            println!(
                "  USB mass storage interface {} tidak punya pasangan bulk IN/OUT",
                interface
            );
            continue;
        };

        let in_dci = endpoint_dci(bulk_in);
        let out_dci = endpoint_dci(bulk_out);
        let (first, second) = if in_dci < out_dci {
            (bulk_in, bulk_out)
        } else {
            (bulk_out, bulk_in)
        };
        let mut handles = match unsafe {
            xhci::configure_endpoints(slot_id, root_port, &[first, second])
        } {
            Ok(handles) => handles,
            Err(error) => {
                println!("  configure bulk endpoint gagal: {}", error);
                continue;
            }
        };
        let (Some(second_handle), Some(first_handle)) = (handles.pop(), handles.pop()) else {
            println!("  configure bulk endpoint gagal: handle tidak lengkap");
            continue;
        };
        let (in_handle, out_handle) = if in_dci < out_dci {
            (first_handle, second_handle)
        } else {
            (second_handle, first_handle)
        };

        match unsafe { mass_storage::init_storage(slot_id, in_handle, out_handle) } {
            Ok(_) => initialized = true,
            Err(error) => println!("  USB storage gagal diinisialisasi: {:?}", error),
        }
    }
    initialized
}

unsafe fn init_ehci_mass_storage(interface: &UsbInterfaceInfo) -> bool {
    let bulk_in = find_mass_storage_endpoint(&interface.endpoints, interface.number, true);
    let bulk_out = find_mass_storage_endpoint(&interface.endpoints, interface.number, false);
    let (Some(bulk_in), Some(bulk_out)) = (bulk_in, bulk_out) else {
        return false;
    };

    let Ok(device_address) = u8::try_from(interface.slot_id) else {
        return false;
    };
    match unsafe {
        mass_storage::init_ehci_storage(interface.controller, device_address, bulk_in, bulk_out)
    } {
        Ok(_) => true,
        Err(error) => {
            println!("  EHCI USB storage gagal diinisialisasi: {:?}", error);
            false
        }
    }
}

fn is_mass_storage_bulk_endpoint(endpoint: xhci::EndpointInfo) -> bool {
    endpoint.interface_class == 0x08
        && endpoint.interface_subclass == 0x06
        && endpoint.interface_protocol == 0x50
        && endpoint.attributes & 0x3 == 0x2
}

fn find_mass_storage_endpoint(
    endpoints: &[xhci::EndpointInfo],
    interface: u8,
    input: bool,
) -> Option<xhci::EndpointInfo> {
    endpoints.iter().copied().find(|endpoint| {
        is_mass_storage_bulk_endpoint(*endpoint)
            && endpoint.inteface_number == interface
            && (endpoint.address & 0x80 != 0) == input
    })
}

fn endpoint_dci(endpoint: xhci::EndpointInfo) -> u32 {
    let endpoint_number = (endpoint.address & 0x0F) as u32;
    endpoint_number * 2 + u32::from(endpoint.address & 0x80 != 0)
}

struct HidKeyboardDriver;
struct HidMouseDriver;
struct MassStorageDriver;
struct RndisDriver;

/// CDC-NCM tethering driver: the USB network function Android 14+ phones use.
struct NcmDriver;

static HID_KEYBOARD_DRIVER: HidKeyboardDriver = HidKeyboardDriver;
static HID_MOUSE_DRIVER: HidMouseDriver = HidMouseDriver;
static MASS_STORAGE_DRIVER: MassStorageDriver = MassStorageDriver;
static RNDIS_DRIVER: RndisDriver = RndisDriver;
static NCM_DRIVER: NcmDriver = NcmDriver;

fn usb_interface(device: &DeviceInfo) -> Option<&UsbInterfaceInfo> {
    match device {
        DeviceInfo::UsbInterface(interface) => Some(interface),
        DeviceInfo::Pci(_) => None,
    }
}

fn hid_interrupt_in(interface: &UsbInterfaceInfo, protocol: u8) -> Option<xhci::EndpointInfo> {
    interface.endpoints.iter().copied().find(|endpoint| {
        endpoint.interface_class == 0x03
            && endpoint.interface_protocol == protocol
            && endpoint.attributes & 0x3 == 0x3
            && endpoint.address & 0x80 != 0
    })
}

impl DeviceDriver for HidKeyboardDriver {
    fn name(&self) -> &'static str {
        "usb-hid-keyboard"
    }

    fn matches(&self, device: &DeviceInfo) -> bool {
        usb_interface(device).is_some_and(|interface| {
            interface.class == 0x03
                && interface.subclass == 0x01
                && hid_interrupt_in(interface, 1).is_some()
        })
    }

    unsafe fn probe(&self, device: &DeviceInfo) -> Result<DriverState, DriverError> {
        let Some(interface) = usb_interface(device) else {
            return Err(DriverError::Unsupported);
        };
        let Some(endpoint) = hid_interrupt_in(interface, 1) else {
            return Err(DriverError::Unsupported);
        };
        match interface.controller.kind {
            UsbControllerKind::Xhci => unsafe {
                dispatch_endpoint(interface.slot_id, interface.root_port, endpoint)?;
            },
            UsbControllerKind::Ehci | UsbControllerKind::Ohci => {
                hid_periodic::start(interface, endpoint)
                    .map_err(|_| DriverError::ProbeFailed("HID periodic endpoint setup failed"))?;
            }
        }
        Ok(Arc::new(()))
    }

    unsafe fn remove(&self, device: &DeviceInfo, _state: DriverState) {
        if let Some(interface) = usb_interface(device) {
            if interface.controller.kind == UsbControllerKind::Xhci {
                unsafe { keyboard_usb::shutdown_keyboard(interface.slot_id) };
            } else {
                hid_periodic::remove(interface.controller, interface.slot_id, interface.number, 1);
            }
        }
    }
}

impl DeviceDriver for HidMouseDriver {
    fn name(&self) -> &'static str {
        "usb-hid-mouse"
    }

    fn matches(&self, device: &DeviceInfo) -> bool {
        usb_interface(device).is_some_and(|interface| {
            interface.class == 0x03
                && interface.subclass == 0x01
                && hid_interrupt_in(interface, 2).is_some()
        })
    }

    unsafe fn probe(&self, device: &DeviceInfo) -> Result<DriverState, DriverError> {
        let Some(interface) = usb_interface(device) else {
            return Err(DriverError::Unsupported);
        };
        let Some(endpoint) = hid_interrupt_in(interface, 2) else {
            return Err(DriverError::Unsupported);
        };
        match interface.controller.kind {
            UsbControllerKind::Xhci => unsafe {
                dispatch_endpoint(interface.slot_id, interface.root_port, endpoint)?;
            },
            UsbControllerKind::Ehci | UsbControllerKind::Ohci => {
                hid_periodic::start(interface, endpoint)
                    .map_err(|_| DriverError::ProbeFailed("HID periodic endpoint setup failed"))?;
            }
        }
        Ok(Arc::new(()))
    }

    unsafe fn remove(&self, device: &DeviceInfo, _state: DriverState) {
        if let Some(interface) = usb_interface(device) {
            if interface.controller.kind == UsbControllerKind::Xhci {
                unsafe { mouse_usb::shutdown_mouse(interface.slot_id) };
            } else {
                hid_periodic::remove(interface.controller, interface.slot_id, interface.number, 2);
            }
        }
    }
}

impl DeviceDriver for MassStorageDriver {
    fn name(&self) -> &'static str {
        "usb-mass-storage-bot"
    }

    fn matches(&self, device: &DeviceInfo) -> bool {
        usb_interface(device).is_some_and(|interface| {
            interface.class == 0x08
                && interface.subclass == 0x06
                && interface.protocol == 0x50
                && find_mass_storage_endpoint(&interface.endpoints, interface.number, true)
                    .is_some()
                && find_mass_storage_endpoint(&interface.endpoints, interface.number, false)
                    .is_some()
        })
    }

    unsafe fn probe(&self, device: &DeviceInfo) -> Result<DriverState, DriverError> {
        let Some(interface) = usb_interface(device) else {
            return Err(DriverError::Unsupported);
        };
        let initialized = match interface.controller.kind {
            UsbControllerKind::Xhci => unsafe {
                init_mass_storage_interfaces(
                    interface.slot_id,
                    interface.root_port,
                    &interface.endpoints,
                )
            },
            UsbControllerKind::Ehci => unsafe { init_ehci_mass_storage(interface) },
            UsbControllerKind::Ohci => unsafe { init_ohci_mass_storage(interface) },
        };
        if initialized {
            Ok(Arc::new(()))
        } else {
            Err(DriverError::ProbeFailed(
                "USB Mass Storage BOT initialization failed",
            ))
        }
    }

    unsafe fn remove(&self, device: &DeviceInfo, _state: DriverState) {
        if let Some(interface) = usb_interface(device) {
            mass_storage::shutdown(interface.controller, interface.slot_id);
        }
    }
}

impl DeviceDriver for RndisDriver {
    fn name(&self) -> &'static str {
        "usb-rndis"
    }

    fn matches(&self, device: &DeviceInfo) -> bool {
        usb_interface(device).is_some_and(|interface| {
            interface.controller.kind == UsbControllerKind::Xhci && rndis::matches(interface)
        })
    }

    unsafe fn probe(&self, device: &DeviceInfo) -> Result<DriverState, DriverError> {
        let Some(interface) = usb_interface(device) else {
            return Err(DriverError::Unsupported);
        };
        match unsafe { rndis::probe(interface) } {
            Ok(()) => Ok(Arc::new(())),
            Err(error) => {
                println!("  RNDIS initialization failed: {error}");
                Err(DriverError::ProbeFailed("RNDIS initialization failed"))
            }
        }
    }

    unsafe fn remove(&self, device: &DeviceInfo, _state: DriverState) {
        if let Some(interface) = usb_interface(device) {
            rndis::shutdown(interface.slot_id);
        }
    }
}

impl DeviceDriver for NcmDriver {
    fn name(&self) -> &'static str {
        "usb-cdc-ncm"
    }

    /// Android 14+ (including Android 16) exposes USB tethering as CDC-NCM
    /// rather than RNDIS, so this is the driver those phones bind to.
    fn matches(&self, device: &DeviceInfo) -> bool {
        usb_interface(device).is_some_and(|interface| {
            interface.controller.kind == UsbControllerKind::Xhci && ncm::matches(interface)
        })
    }

    unsafe fn probe(&self, device: &DeviceInfo) -> Result<DriverState, DriverError> {
        let Some(interface) = usb_interface(device) else {
            return Err(DriverError::Unsupported);
        };
        match unsafe { ncm::probe(interface) } {
            Ok(()) => Ok(Arc::new(())),
            Err(error) => {
                println!("  NCM initialization failed: {error}");
                Err(DriverError::ProbeFailed("NCM initialization failed"))
            }
        }
    }

    unsafe fn remove(&self, device: &DeviceInfo, _state: DriverState) {
        if let Some(interface) = usb_interface(device) {
            ncm::shutdown(interface.slot_id);
        }
    }
}
