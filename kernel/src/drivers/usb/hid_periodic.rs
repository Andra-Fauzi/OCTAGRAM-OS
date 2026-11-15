use super::{
    EndpointInfo, UsbControllerId, UsbControllerKind, ehci, keyboard_usb, mouse_usb, ohci,
};
use crate::{drivers::device_model::UsbInterfaceInfo, memory};
use alloc::boxed::Box;
use spin::Mutex;

const MAX_HID_PACKET_SIZE: usize = 64;

#[repr(C, align(4096))]
struct ReportBuffer([u8; MAX_HID_PACKET_SIZE]);

struct PeriodicHidDevice {
    controller: UsbControllerId,
    slot_id: u32,
    interface_number: u8,
    protocol: u8,
    endpoint_address: u8,
    max_packet_size: u16,
    interval_frames: u16,
    last_frame: u16,
    data_toggle: bool,
    report_buffer: Box<ReportBuffer>,
    report_phys: u64,
}

static DEVICES: Mutex<alloc::vec::Vec<PeriodicHidDevice>> = Mutex::new(alloc::vec::Vec::new());

pub(super) fn start(
    interface: &UsbInterfaceInfo,
    endpoint: EndpointInfo,
) -> Result<(), &'static str> {
    if endpoint.address & 0x80 == 0 || endpoint.attributes & 0x3 != 0x3 || endpoint.interval == 0 {
        return Err("invalid HID interrupt-IN endpoint");
    }

    let max_packet_size = endpoint.max_packet_size & 0x07ff;
    if max_packet_size == 0 || usize::from(max_packet_size) > MAX_HID_PACKET_SIZE {
        return Err("unsupported HID interrupt packet size");
    }
    let address = u8::try_from(interface.slot_id).map_err(|_| "invalid USB device address")?;

    let last_frame = match interface.controller.kind {
        UsbControllerKind::Ehci => {
            let mut controllers = ehci::EHCI_CONTROLLERS.lock();
            let controller = controllers
                .get_mut(usize::from(interface.controller.instance))
                .ok_or("EHCI controller instance is unavailable")?;
            let device = controller
                .devices()
                .iter()
                .find(|device| device.address == address)
                .ok_or("EHCI device is unavailable")?
                .max_packet_size;
            controller.control_transfer(
                address,
                device,
                0x21,
                0x0b,
                0,
                u16::from(interface.number),
                None,
                0,
            )?;
            controller.frame_number()
        }
        UsbControllerKind::Ohci => {
            let mut controllers = ohci::OHCI_CONTROLLERS.lock();
            let controller = controllers
                .get_mut(usize::from(interface.controller.instance))
                .ok_or("OHCI controller instance is unavailable")?;
            let device = controller
                .devices()
                .iter()
                .find(|device| device.address == address)
                .ok_or("OHCI device is unavailable")?;
            let max_packet_size = device.max_packet_size;
            let low_speed = device.low_speed;
            controller.control_transfer(
                address,
                max_packet_size,
                low_speed,
                0x21,
                0x0b,
                0,
                u16::from(interface.number),
                None,
                0,
            )?;
            controller.frame_number()
        }
        UsbControllerKind::Xhci => return Err("xHCI HID uses its event-ring driver"),
    };

    let report_buffer = Box::new(ReportBuffer([0; MAX_HID_PACKET_SIZE]));
    let report_phys = memory::virtual_to_physical(&*report_buffer as *const ReportBuffer as usize)
        .ok_or("HID report buffer DMA translation failed")? as u64;
    let interval_frames = interval_frames(interface.controller.kind, endpoint.interval);
    DEVICES.lock().push(PeriodicHidDevice {
        controller: interface.controller,
        slot_id: interface.slot_id,
        interface_number: interface.number,
        protocol: endpoint.interface_protocol,
        endpoint_address: endpoint.address,
        max_packet_size,
        interval_frames,
        last_frame,
        data_toggle: false,
        report_buffer,
        report_phys,
    });
    crate::println!(
        "HID periodic endpoint {:#04x} aktif, interval {} frame",
        endpoint.address,
        interval_frames
    );
    Ok(())
}

pub(super) fn remove(
    controller: UsbControllerId,
    slot_id: u32,
    interface_number: u8,
    protocol: u8,
) {
    let mut devices = DEVICES.lock();
    devices.retain(|device| {
        device.controller != controller
            || device.slot_id != slot_id
            || device.interface_number != interface_number
    });
    match protocol {
        1 => keyboard_usb::reset_periodic_state(),
        2 => mouse_usb::reset_periodic_state(),
        _ => {}
    }
}

pub(super) fn poll() {
    let mut devices = DEVICES.lock();
    for device in devices.iter_mut() {
        let current_frame = match device.controller.kind {
            UsbControllerKind::Ehci => ehci::EHCI_CONTROLLERS
                .lock()
                .get(usize::from(device.controller.instance))
                .map(ehci::EhciController::frame_number),
            UsbControllerKind::Ohci => ohci::OHCI_CONTROLLERS
                .lock()
                .get(usize::from(device.controller.instance))
                .map(ohci::OhciController::frame_number),
            UsbControllerKind::Xhci => None,
        };
        let Some(current_frame) = current_frame else {
            crate::println!(
                "HID periodic controller {:?} {} is unavailable",
                device.controller.kind,
                device.controller.instance
            );
            continue;
        };
        if !frame_due(
            device.controller.kind,
            current_frame,
            device.last_frame,
            device.interval_frames,
        ) {
            continue;
        }
        device.last_frame = current_frame;

        match poll_endpoint(device) {
            Ok(Some(length)) if length != 0 => {
                let report = device
                    .report_buffer
                    .0
                    .iter()
                    .take(length)
                    .map(|byte| unsafe { core::ptr::addr_of!(*byte).read_volatile() })
                    .collect::<alloc::vec::Vec<_>>();
                match device.protocol {
                    1 => keyboard_usb::process_periodic_report(&report),
                    2 => mouse_usb::process_periodic_report(&report),
                    _ => {}
                }
            }
            Ok(_) => {}
            Err(error) => crate::println!(
                "HID periodic transfer failed for address {} endpoint {:#04x}: {}",
                device.slot_id,
                device.endpoint_address,
                error
            ),
        }
    }
}

fn poll_endpoint(device: &mut PeriodicHidDevice) -> Result<Option<usize>, &'static str> {
    match device.controller.kind {
        UsbControllerKind::Ehci => {
            let mut controllers = ehci::EHCI_CONTROLLERS.lock();
            let controller = controllers
                .get_mut(usize::from(device.controller.instance))
                .ok_or("EHCI controller instance is unavailable")?;
            controller.periodic_interrupt_in(
                u8::try_from(device.slot_id).map_err(|_| "invalid USB device address")?,
                device.endpoint_address & 0x0f,
                device.max_packet_size,
                device.report_phys,
                usize::from(device.max_packet_size),
                &mut device.data_toggle,
            )
        }
        UsbControllerKind::Ohci => {
            let mut controllers = ohci::OHCI_CONTROLLERS.lock();
            let controller = controllers
                .get_mut(usize::from(device.controller.instance))
                .ok_or("OHCI controller instance is unavailable")?;
            let low_speed = controller
                .devices()
                .iter()
                .find(|usb_device| usb_device.address == device.slot_id as u8)
                .ok_or("OHCI device is unavailable")?
                .low_speed;
            controller.periodic_interrupt_in(
                u8::try_from(device.slot_id).map_err(|_| "invalid USB device address")?,
                device.endpoint_address,
                device.max_packet_size,
                low_speed,
                device.report_phys,
                usize::from(device.max_packet_size),
                &mut device.data_toggle,
            )
        }
        UsbControllerKind::Xhci => Err("xHCI HID uses its event-ring driver"),
    }
}

fn interval_frames(kind: UsbControllerKind, interval: u8) -> u16 {
    match kind {
        UsbControllerKind::Ehci => {
            let exponent = interval.saturating_sub(4).min(10);
            1u16 << exponent
        }
        UsbControllerKind::Ohci => u16::from(interval),
        UsbControllerKind::Xhci => 1,
    }
}

fn frame_due(kind: UsbControllerKind, current: u16, previous: u16, interval: u16) -> bool {
    let elapsed = match kind {
        UsbControllerKind::Ehci => current.wrapping_sub(previous) & 0x03ff,
        UsbControllerKind::Ohci => current.wrapping_sub(previous),
        UsbControllerKind::Xhci => current.wrapping_sub(previous),
    };
    elapsed >= interval
}

#[cfg(test)]
mod tests {
    use super::{frame_due, interval_frames};
    use crate::drivers::usb::UsbControllerKind;

    #[test]
    fn maps_interrupt_intervals_to_frame_periods() {
        assert_eq!(interval_frames(UsbControllerKind::Ohci, 8), 8);
        assert_eq!(interval_frames(UsbControllerKind::Ehci, 1), 1);
        assert_eq!(interval_frames(UsbControllerKind::Ehci, 6), 4);
    }

    #[test]
    fn detects_frame_period_across_controller_counter_wrap() {
        assert!(frame_due(UsbControllerKind::Ohci, 1, u16::MAX - 1, 3));
        assert!(frame_due(UsbControllerKind::Ehci, 1, 1022, 3));
        assert!(!frame_due(UsbControllerKind::Ehci, 0, 1022, 3));
    }
}
