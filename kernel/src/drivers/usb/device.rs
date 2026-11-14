//! Device enumeration, class dispatch, and USB hotplug lifecycle.

use super::{keyboard_usb, mass_storage, mouse_usb, xhci};
use crate::println;
use core::sync::atomic::Ordering;
use spin::Mutex;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DeviceKind {
    Keyboard,
    Mouse,
}

struct ActivePort {
    root_port: u8,
    slot_id: u32,
    has_keyboard: bool,
    has_mouse: bool,
    has_storage: bool,
}

static ACTIVE_PORTS: Mutex<alloc::vec::Vec<ActivePort>> = Mutex::new(alloc::vec::Vec::new());

/// Process pending port changes outside the xHCI interrupt handler.
///
/// # Safety
/// Must be called after the interrupt handler has returned.
pub unsafe fn poll_hotplug() {
    unsafe {
        if !xhci::PORT_CHANGE_PENDING.swap(false, Ordering::SeqCst) {
            return;
        }
        if xhci::XHCI_INSTANCE.get().is_none() {
            return;
        }
        println!("Port Status Change terdeteksi, sinkronisasi hotplug...");
        discover_and_init_hid_devices();
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

        let removed = active_ports.remove(index);
        unsafe { shutdown_device(&removed) };
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

unsafe fn shutdown_device(device: &ActivePort) {
    unsafe {
        println!(
            "--- Device di root port {} (slot={}) disconnect ---",
            device.root_port, device.slot_id
        );

        if device.has_keyboard {
            keyboard_usb::shutdown_keyboard(device.slot_id);
        }
        if device.has_mouse {
            mouse_usb::shutdown_mouse(device.slot_id);
        }
        if device.has_storage {
            mass_storage::shutdown(device.slot_id);
        }
        if let Err(error) = xhci::disable_slot(device.slot_id) {
            println!("  disable_slot gagal: {}", error);
        }
    }
}

unsafe fn init_one_device(root_port: u8, speed: u8) -> Option<ActivePort> {
    unsafe {
        println!(
            "--- Enumerasi device di root port {} (speed={}) ---",
            root_port, speed
        );
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
            return Some(active_port(root_port, slot_id, false, false, false));
        }

        let endpoints = match xhci::get_configuration_descriptor_endpoints(slot_id) {
            Ok(endpoints) => endpoints,
            Err(error) => {
                println!("  tidak ada endpoint konfigurasi: {}", error);
                return Some(active_port(root_port, slot_id, false, false, false));
            }
        };
        if let Err(error) = xhci::control_transfer_no_data(slot_id, 0x00, 0x09, 1, 0) {
            println!("  SET_CONFIGURATION gagal: {}", error);
            return Some(active_port(root_port, slot_id, false, false, false));
        }

        let mut has_keyboard = false;
        let mut has_mouse = false;
        for endpoint in &endpoints {
            match unsafe { dispatch_endpoint(slot_id, root_port, *endpoint) } {
                Some(DeviceKind::Keyboard) => has_keyboard = true,
                Some(DeviceKind::Mouse) => has_mouse = true,
                None => {}
            }
        }
        let has_storage = init_mass_storage_interfaces(slot_id, root_port, &endpoints);

        Some(active_port(
            root_port,
            slot_id,
            has_keyboard,
            has_mouse,
            has_storage,
        ))
    }
}

fn active_port(
    root_port: u8,
    slot_id: u32,
    has_keyboard: bool,
    has_mouse: bool,
    has_storage: bool,
) -> ActivePort {
    ActivePort {
        root_port,
        slot_id,
        has_keyboard,
        has_mouse,
        has_storage,
    }
}

unsafe fn dispatch_endpoint(
    slot_id: u32,
    root_port: u8,
    endpoint: xhci::EndpointInfo,
) -> Option<DeviceKind> {
    if endpoint.interface_class != 3
        || endpoint.attributes & 0x3 != 0x3
        || endpoint.address & 0x80 == 0
    {
        return None;
    }

    let handle = match unsafe { xhci::configure_endpoint(slot_id, root_port, endpoint) } {
        Ok(handle) => handle,
        Err(error) => {
            println!("  configure_endpoint gagal: {}", error);
            return None;
        }
    };

    match endpoint.interface_protocol {
        1 => {
            unsafe {
                keyboard_usb::init_keyboard(slot_id, handle, endpoint.inteface_number);
            }
            Some(DeviceKind::Keyboard)
        }
        2 => {
            unsafe {
                mouse_usb::init_mouse(slot_id, handle, endpoint.inteface_number);
            }
            Some(DeviceKind::Mouse)
        }
        protocol => {
            println!(
                "  HID interface protocol {:#04x} belum ada driver-nya, dilewati",
                protocol
            );
            None
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
        let first_handle = match unsafe { xhci::configure_endpoint(slot_id, root_port, first) } {
            Ok(handle) => handle,
            Err(error) => {
                println!("  configure bulk endpoint gagal: {}", error);
                continue;
            }
        };
        let second_handle = match unsafe { xhci::configure_endpoint(slot_id, root_port, second) } {
            Ok(handle) => handle,
            Err(error) => {
                println!("  configure bulk endpoint gagal: {}", error);
                continue;
            }
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
