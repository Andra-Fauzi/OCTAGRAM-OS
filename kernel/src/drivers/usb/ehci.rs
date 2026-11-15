//! EHCI (USB 2.0) controller register and transfer support.

use super::UsbPortEvent;
pub use super::descriptors::parse_configuration_endpoints;
use crate::drivers::pci::{self, PciDevice};
use crate::memory;
use alloc::boxed::Box;
use core::sync::atomic::{Ordering, compiler_fence};
use spin::Mutex;

const PCI_CLASS_USB: u8 = 0x0C;
const PCI_SUBCLASS_USB: u8 = 0x03;
const PCI_PROG_IF_EHCI: u8 = 0x20;
const CAP_HCCPARAMS: u64 = 0x08;
const OP_USBCMD: u64 = 0x00;
const OP_USBSTS: u64 = 0x04;
const OP_USBINTR: u64 = 0x08;
const OP_FRINDEX: u64 = 0x0c;
const OP_PERIODICLISTBASE: u64 = 0x14;
const OP_ASYNCLISTADDR: u64 = 0x18;
const OP_CONFIGFLAG: u64 = 0x40;
const OP_PORTSC: u64 = 0x44;
const CMD_RUN: u32 = 1;
const CMD_RESET: u32 = 1 << 1;
const CMD_ASYNC_ENABLE: u32 = 1 << 5;
const CMD_PERIODIC_ENABLE: u32 = 1 << 4;
const STS_PERIODIC: u32 = 1 << 14;
const STS_ASYNC: u32 = 1 << 15;
const STS_HALTED: u32 = 1 << 12;
const PORT_CONNECT: u32 = 1;
const PORT_ENABLE: u32 = 1 << 2;
const PORT_RESET: u32 = 1 << 8;
const PORT_POWER: u32 = 1 << 12;
const PORT_OWNER: u32 = 1 << 13;
const PORT_CHANGE_BITS: u32 = (1 << 1) | (1 << 3) | (1 << 5);
const QH_TYPE: u32 = 1 << 1;
const QH_TERMINATE: u32 = 1;
const QTD_TERMINATE: u32 = 1;
const QTD_ACTIVE: u32 = 1 << 7;
const QTD_PID_OUT: u32 = 0;
const QTD_PID_IN: u32 = 1 << 8;
const QTD_PID_SETUP: u32 = 2 << 8;
const QTD_CERR_3: u32 = 3 << 10;
const QTD_IOC: u32 = 1 << 15;
const QTD_TOGGLE: u32 = 1 << 31;
const QTD_ERROR_BITS: u32 = (1 << 6) | (1 << 5) | (1 << 4) | (1 << 3);
const FRAME_LIST_ENTRIES: usize = 1024;
const MAX_QTD_BYTES: usize = 0x7fff;
const TRANSFER_POLLS: usize = 10_000_000;

pub static EHCI_CONTROLLERS: Mutex<alloc::vec::Vec<EhciController>> =
    Mutex::new(alloc::vec::Vec::new());

#[repr(C, align(32))]
#[derive(Clone, Copy, Default)]
struct Qtd {
    next: u32,
    alternate: u32,
    token: u32,
    buffers: [u32; 5],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct QtdOverlay {
    next: u32,
    alternate: u32,
    token: u32,
    buffers: [u32; 5],
}

#[repr(C, align(64))]
#[derive(Clone, Copy, Default)]
struct Qh {
    link: u32,
    endpoint_characteristics: u32,
    endpoint_capabilities: u32,
    current_qtd: u32,
    overlay: QtdOverlay,
}

#[repr(C, align(4096))]
struct FrameList([u32; FRAME_LIST_ENTRIES]);

#[repr(C, align(16))]
struct SetupPacket([u8; 8]);

pub struct EhciController {
    mmio: u64,
    op: u64,
    port_count: u8,
    async_head: Box<Qh>,
    transfer_qh: Box<Qh>,
    periodic_qh: Box<Qh>,
    frame_list: Box<FrameList>,
    devices: alloc::vec::Vec<EhciUsbDevice>,
    known_ports: alloc::vec::Vec<u8>,
}

pub struct EhciUsbDevice {
    pub root_port: u8,
    pub address: u8,
    pub max_packet_size: u16,
    pub configuration_value: u8,
    pub device_descriptor: [u8; 18],
    pub configuration_descriptor: alloc::vec::Vec<u8>,
}

#[repr(C, align(4096))]
struct DescriptorBuffer([u8; 4096]);

impl DescriptorBuffer {
    fn read(&self, length: usize) -> alloc::vec::Vec<u8> {
        compiler_fence(Ordering::Acquire);
        let mut bytes = alloc::vec::Vec::with_capacity(length);
        for index in 0..length {
            bytes.push(unsafe { core::ptr::addr_of!(self.0[index]).read_volatile() });
        }
        bytes
    }
}

impl EhciController {
    /// Initialize a PCI EHCI controller. Transfers are serialized and polled.
    ///
    /// # Safety
    /// The PCI function must be an EHCI controller and no other driver may own it.
    pub unsafe fn new(device: &PciDevice) -> Result<Self, &'static str> {
        unsafe {
            pci::enable_device(device);
            let bar = pci::get_bar0(device);
            if bar.address == 0 {
                return Err("EHCI BAR0 is not assigned");
            }
            let size = pci::get_bar0_size(device);
            if size < 0x44 {
                return Err("EHCI BAR0 is too small");
            }
            let mmio = memory::paging::map_mmio_range(bar.address, size);
            Self::handoff_legacy(device, mmio)?;
            let caplength_version = read32(mmio, 0);
            let caplength = (caplength_version & 0xff) as u64;
            if caplength < 0x10 || caplength + 0x44 > size {
                return Err("invalid EHCI capability length");
            }
            let structural = read32(mmio, 4);
            let ports = (structural & 0xf) as u8;
            if ports == 0 || caplength + OP_PORTSC + u64::from(ports) * 4 > size {
                return Err("invalid EHCI root-port count");
            }
            let op = mmio + caplength;

            let mut controller = Self {
                mmio,
                op,
                port_count: ports,
                async_head: Box::new(Qh::default()),
                transfer_qh: Box::new(Qh::default()),
                periodic_qh: Box::new(Qh::default()),
                frame_list: Box::new(FrameList([QH_TERMINATE; FRAME_LIST_ENTRIES])),
                devices: alloc::vec::Vec::new(),
                known_ports: alloc::vec::Vec::new(),
            };
            controller.initialize()?;
            Ok(controller)
        }
    }

    fn handoff_legacy(device: &PciDevice, mmio: u64) -> Result<(), &'static str> {
        let hccparams = read32(mmio, CAP_HCCPARAMS);
        let mut offset = ((hccparams >> 8) & 0xff) as u8;
        for _ in 0..48 {
            if offset == 0 {
                return Ok(());
            }
            let header = pci::read_config_u32(device, offset);
            let cap_id = header as u8;
            let next = ((header >> 8) & 0xff) as u8;
            if cap_id == 1 {
                pci::write_config_u32(device, offset, header | (1 << 24));
                for _ in 0..1_000_000 {
                    if pci::read_config_u32(device, offset) & (1 << 16) == 0 {
                        let smi = pci::read_config_u32(device, offset + 4);
                        pci::write_config_u32(device, offset + 4, smi & !0x3f);
                        return Ok(());
                    }
                    core::hint::spin_loop();
                }
                return Err("timed out waiting for EHCI BIOS handoff");
            }
            if next == 0 || next < 0x40 || next > 0xf8 || next & 3 != 0 {
                break;
            }
            offset = next;
        }
        Ok(())
    }

    fn initialize(&mut self) -> Result<(), &'static str> {
        let command = read32(self.op, OP_USBCMD);
        write32(self.op, OP_USBCMD, command & !CMD_RUN);
        if !wait_for(TRANSFER_POLLS, || {
            read32(self.op, OP_USBSTS) & STS_HALTED != 0
        }) {
            return Err("EHCI failed to halt");
        }
        write32(self.op, OP_USBCMD, read32(self.op, OP_USBCMD) | CMD_RESET);
        if !wait_for(TRANSFER_POLLS, || {
            read32(self.op, OP_USBCMD) & CMD_RESET == 0
        }) {
            return Err("EHCI host reset timed out");
        }

        let head_phys = dma32(&*self.async_head)?;
        let frame_phys = dma32(&*self.frame_list)?;
        self.async_head.link = head_phys | QH_TYPE;
        self.async_head.endpoint_characteristics = (1 << 15) | (2 << 12) | (64 << 16);
        self.async_head.endpoint_capabilities = 1 << 30;
        set_qh_overlay(&mut self.async_head, QTD_TERMINATE);
        self.transfer_qh.link = head_phys | QH_TYPE;
        set_qh_overlay(&mut self.transfer_qh, QTD_TERMINATE);
        self.frame_list.0.fill(QH_TERMINATE);
        compiler_fence(Ordering::Release);

        write32(self.op, OP_ASYNCLISTADDR, head_phys);
        write32(self.op, OP_PERIODICLISTBASE, frame_phys);
        write32(self.op, OP_USBINTR, 0);
        write32(self.op, OP_USBSTS, 0x3f);
        write32(self.op, OP_CONFIGFLAG, 1);
        for port in 0..self.port_count {
            let offset = OP_PORTSC + u64::from(port) * 4;
            let status = read32(self.op, offset);
            if read32(self.mmio, 4) & (1 << 4) != 0 {
                write32(self.op, offset, (status & !PORT_CHANGE_BITS) | PORT_POWER);
            }
        }
        write32(self.op, OP_USBCMD, CMD_RUN);
        if !wait_for(TRANSFER_POLLS, || {
            read32(self.op, OP_USBSTS) & STS_HALTED == 0
        }) {
            return Err("EHCI failed to start");
        }
        Ok(())
    }

    pub fn port_count(&self) -> u8 {
        self.port_count
    }

    pub fn port_status(&self, port: u8) -> Result<u32, &'static str> {
        if port >= self.port_count {
            return Err("EHCI port index out of range");
        }
        Ok(read32(self.op, OP_PORTSC + u64::from(port) * 4))
    }

    pub fn devices(&self) -> &[EhciUsbDevice] {
        &self.devices
    }

    pub fn frame_number(&self) -> u16 {
        ((read32(self.op, OP_FRINDEX) >> 3) & 0x3ff) as u16
    }

    pub fn scan_ports(&mut self) {
        for port_index in 0..self.port_count {
            match self.port_status(port_index) {
                Ok(status) if status & PORT_CONNECT != 0 => {
                    self.known_ports.push(port_index);
                    match self.enumerate_port(port_index) {
                        Ok(device) => {
                            crate::println!(
                                "EHCI USB device at port {} addressed as {} ({} config bytes)",
                                device.root_port,
                                device.address,
                                device.configuration_descriptor.len()
                            );
                            self.devices.push(device);
                        }
                        Err(error) => {
                            crate::println!(
                                "EHCI port {} enumeration failed: {}",
                                port_index + 1,
                                error
                            );
                        }
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    crate::println!("EHCI port {} status failed: {}", port_index + 1, error)
                }
            }
        }
    }

    pub fn poll_ports(&mut self) -> alloc::vec::Vec<UsbPortEvent> {
        let mut events = alloc::vec::Vec::new();
        for port_index in 0..self.port_count {
            let status = match self.port_status(port_index) {
                Ok(status) => status,
                Err(error) => {
                    crate::println!(
                        "EHCI hotplug port {} status failed: {}",
                        port_index + 1,
                        error
                    );
                    continue;
                }
            };
            if status & PORT_CHANGE_BITS != 0 {
                write32(
                    self.op,
                    OP_PORTSC + u64::from(port_index) * 4,
                    status & PORT_CHANGE_BITS,
                );
            }
            let connected = status & PORT_CONNECT != 0;
            let known = self.known_ports.contains(&port_index);
            if connected && !known {
                self.known_ports.push(port_index);
                match self.enumerate_port(port_index) {
                    Ok(device) => {
                        events.push(UsbPortEvent::Connected {
                            root_port: device.root_port,
                            address: device.address,
                            configuration_descriptor: device.configuration_descriptor.clone(),
                        });
                        self.devices.push(device);
                    }
                    Err(error) => crate::println!(
                        "EHCI hotplug port {} enumeration failed: {}",
                        port_index + 1,
                        error
                    ),
                }
            } else if !connected && known {
                self.known_ports.retain(|&port| port != port_index);
                if let Some(index) = self
                    .devices
                    .iter()
                    .position(|device| device.root_port == port_index + 1)
                {
                    let device = self.devices.remove(index);
                    events.push(UsbPortEvent::Disconnected {
                        root_port: device.root_port,
                        address: device.address,
                        configuration_descriptor: device.configuration_descriptor,
                    });
                }
            }
        }
        events
    }

    fn enumerate_port(&mut self, port_index: u8) -> Result<EhciUsbDevice, &'static str> {
        self.reset_port(port_index)?;
        let root_port = port_index + 1;
        self.enumerate_device(root_port, root_port)
    }

    fn enumerate_device(
        &mut self,
        root_port: u8,
        address: u8,
    ) -> Result<EhciUsbDevice, &'static str> {
        let buffer = Box::new(DescriptorBuffer([0; 4096]));
        let buffer_phys = u64::from(dma32(&*buffer)?);
        let length = self.control_transfer(0, 64, 0x80, 0x06, 0x0100, 0, Some(buffer_phys), 8)?;
        if length != 8 {
            return Err("short initial USB device descriptor");
        }
        let first = buffer.read(8);
        if first[0] < 8 || first[1] != 1 {
            return Err("invalid initial USB device descriptor");
        }
        let max_packet_size = u16::from(first[7]);
        if max_packet_size == 0 || max_packet_size > 64 {
            return Err("unsupported EHCI endpoint-zero packet size");
        }

        self.control_transfer(0, max_packet_size, 0, 0x05, u16::from(address), 0, None, 0)?;
        delay(2_000);
        let length = self.control_transfer(
            address,
            max_packet_size,
            0x80,
            0x06,
            0x0100,
            0,
            Some(buffer_phys),
            18,
        )?;
        if length != 18 {
            return Err("short USB device descriptor");
        }
        let descriptor_bytes = buffer.read(18);
        if descriptor_bytes[0] < 18 || descriptor_bytes[1] != 1 {
            return Err("invalid USB device descriptor");
        }
        let mut device_descriptor = [0; 18];
        device_descriptor.copy_from_slice(&descriptor_bytes[..18]);

        let length = self.control_transfer(
            address,
            max_packet_size,
            0x80,
            0x06,
            0x0200,
            0,
            Some(buffer_phys),
            9,
        )?;
        if length != 9 {
            return Err("short USB configuration descriptor header");
        }
        let header = buffer.read(9);
        if header[0] < 9 || header[1] != 2 {
            return Err("invalid USB configuration descriptor header");
        }
        let total_length = u16::from_le_bytes([header[2], header[3]]) as usize;
        if !(9..=4096).contains(&total_length) {
            return Err("USB configuration descriptor length is unsupported");
        }
        let length = self.control_transfer(
            address,
            max_packet_size,
            0x80,
            0x06,
            0x0200,
            0,
            Some(buffer_phys),
            total_length,
        )?;
        if length != total_length {
            return Err("short USB configuration descriptor");
        }
        let configuration_descriptor = buffer.read(total_length);
        if configuration_descriptor[0] < 9 || configuration_descriptor[1] != 2 {
            return Err("invalid USB configuration descriptor");
        }
        let configuration_value = *configuration_descriptor
            .get(5)
            .ok_or("missing USB configuration value")?;
        self.control_transfer(
            address,
            max_packet_size,
            0,
            0x09,
            u16::from(configuration_value),
            0,
            None,
            0,
        )?;

        Ok(EhciUsbDevice {
            root_port,
            address,
            max_packet_size,
            configuration_value,
            device_descriptor,
            configuration_descriptor,
        })
    }

    /// Reset an EHCI root port. Full/low-speed devices are handed to companions.
    pub fn reset_port(&self, port: u8) -> Result<(), &'static str> {
        if port >= self.port_count {
            return Err("EHCI port index out of range");
        }
        let offset = OP_PORTSC + u64::from(port) * 4;
        let status = read32(self.op, offset);
        if status & PORT_CONNECT == 0 {
            return Err("EHCI port has no connected device");
        }
        write32(self.op, offset, (status & !PORT_CHANGE_BITS) | PORT_RESET);
        delay(50_000);
        let after_reset = read32(self.op, offset) & !PORT_RESET & !PORT_CHANGE_BITS;
        write32(self.op, offset, after_reset);
        if !wait_for(TRANSFER_POLLS, || {
            let current = read32(self.op, offset);
            current & PORT_ENABLE != 0 || current & PORT_CONNECT == 0
        }) {
            return Err("EHCI port reset timed out");
        }
        let current = read32(self.op, offset);
        if current & PORT_ENABLE == 0 {
            write32(self.op, offset, (current & !PORT_CHANGE_BITS) | PORT_OWNER);
            return Err("EHCI port routed to a companion controller");
        }
        Ok(())
    }

    pub fn control_transfer(
        &mut self,
        device_address: u8,
        max_packet_size: u16,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        data_phys: Option<u64>,
        length: usize,
    ) -> Result<usize, &'static str> {
        if device_address > 127 || max_packet_size == 0 || max_packet_size > 64 {
            return Err("invalid EHCI control endpoint parameters");
        }
        if length > MAX_QTD_BYTES || (length != 0 && data_phys.is_none()) {
            return Err("EHCI control transfer length is unsupported");
        }
        let mut setup = Box::new(SetupPacket([0; 8]));
        setup.0[0] = request_type;
        setup.0[1] = request;
        setup.0[2..4].copy_from_slice(&value.to_le_bytes());
        setup.0[4..6].copy_from_slice(&index.to_le_bytes());
        setup.0[6..8].copy_from_slice(&(length as u16).to_le_bytes());
        let setup_phys = dma32(&*setup)?;

        let mut qtds = alloc::vec::Vec::new();
        let mut setup_qtd = Box::new(Qtd::default());
        setup_qtd.token = qtd_token(QTD_PID_SETUP, 8, false, true);
        set_qtd_buffer(&mut setup_qtd, setup_phys as u64, 8)?;
        qtds.push(setup_qtd);

        if length != 0 {
            let mut data_qtd = Box::new(Qtd::default());
            let is_in = request_type & 0x80 != 0;
            data_qtd.token = qtd_token(
                if is_in { QTD_PID_IN } else { QTD_PID_OUT },
                length,
                true,
                true,
            );
            set_qtd_buffer(&mut data_qtd, data_phys.unwrap(), length)?;
            qtds.push(data_qtd);
        }

        let mut status_qtd = Box::new(Qtd::default());
        let status_in = length == 0 || request_type & 0x80 == 0;
        status_qtd.token = qtd_token(
            if status_in { QTD_PID_IN } else { QTD_PID_OUT },
            0,
            true,
            true,
        ) | QTD_IOC;
        qtds.push(status_qtd);

        self.submit_qtds(device_address, 0, max_packet_size, true, &mut qtds)?;
        if length == 0 {
            Ok(0)
        } else {
            Ok(length.saturating_sub(qtd_remaining(volatile_token(&qtds[1]))))
        }
    }

    pub fn bulk_transfer(
        &mut self,
        device_address: u8,
        endpoint: u8,
        max_packet_size: u16,
        is_in: bool,
        data_phys: u64,
        length: usize,
        data_toggle: &mut bool,
    ) -> Result<usize, &'static str> {
        if device_address == 0
            || device_address > 127
            || endpoint == 0
            || endpoint > 15
            || max_packet_size == 0
            || max_packet_size > 512
        {
            return Err("invalid EHCI bulk endpoint parameters");
        }
        if length == 0 || length > MAX_QTD_BYTES {
            return Err("EHCI bulk transfer length is unsupported");
        }
        let mut qtd = Box::new(Qtd::default());
        qtd.token = qtd_token(
            if is_in { QTD_PID_IN } else { QTD_PID_OUT },
            length,
            *data_toggle,
            true,
        ) | QTD_IOC;
        set_qtd_buffer(&mut qtd, data_phys, length)?;
        let mut qtds = alloc::vec![qtd];
        self.submit_qtds(device_address, endpoint, max_packet_size, false, &mut qtds)?;
        let qtd = &qtds[0];
        let token = volatile_token(qtd);
        let transferred = length.saturating_sub(qtd_remaining(token));
        if token & QTD_ERROR_BITS != 0 {
            return Err("EHCI bulk qTD reported a transfer error");
        }
        *data_toggle ^=
            ((transferred + usize::from(max_packet_size) - 1) / usize::from(max_packet_size)) % 2
                != 0;
        Ok(transferred)
    }

    pub fn periodic_interrupt_in(
        &mut self,
        device_address: u8,
        endpoint: u8,
        max_packet_size: u16,
        data_phys: u64,
        length: usize,
        data_toggle: &mut bool,
    ) -> Result<Option<usize>, &'static str> {
        if device_address == 0
            || device_address > 127
            || endpoint == 0
            || endpoint > 15
            || max_packet_size == 0
            || max_packet_size > 1024
            || length == 0
            || length > usize::from(max_packet_size)
        {
            return Err("invalid EHCI interrupt-IN parameters");
        }

        let frame = (read32(self.op, OP_FRINDEX) >> 3) & 0x3ff;
        let target_frame = (frame + 1) & 0x3ff;
        let frame_index = target_frame as usize % FRAME_LIST_ENTRIES;
        let qh_phys = dma32(&*self.periodic_qh)?;
        let mut qtd = Box::new(Qtd::default());
        qtd.token = qtd_token(QTD_PID_IN, length, *data_toggle, true) | QTD_IOC;
        set_qtd_buffer(&mut qtd, data_phys, length)?;
        let qtd_phys = dma32(&*qtd)?;

        self.periodic_qh.link = QH_TERMINATE;
        self.periodic_qh.endpoint_characteristics = u32::from(device_address)
            | (u32::from(endpoint) << 8)
            | (2 << 12)
            | (1 << 14)
            | (u32::from(max_packet_size) << 16);
        self.periodic_qh.endpoint_capabilities = 1;
        set_qh_overlay(&mut self.periodic_qh, qtd_phys);
        self.frame_list.0[frame_index] = qh_phys | QH_TYPE;
        compiler_fence(Ordering::Release);
        write32(
            self.op,
            OP_USBCMD,
            read32(self.op, OP_USBCMD) | CMD_PERIODIC_ENABLE,
        );

        let frame_advanced = wait_for(TRANSFER_POLLS, || {
            ((read32(self.op, OP_FRINDEX) >> 3) & 0x3ff).wrapping_sub(frame) & 0x3ff != 0
        });
        write32(
            self.op,
            OP_USBCMD,
            read32(self.op, OP_USBCMD) & !CMD_PERIODIC_ENABLE,
        );
        let schedule_stopped = wait_for(TRANSFER_POLLS, || {
            read32(self.op, OP_USBSTS) & STS_PERIODIC == 0
        });
        self.frame_list.0[frame_index] = QH_TERMINATE;
        compiler_fence(Ordering::Acquire);

        if !frame_advanced {
            return Err("EHCI periodic frame did not advance");
        }
        if !schedule_stopped {
            return Err("EHCI periodic schedule did not stop");
        }
        let token = volatile_overlay_token(&self.periodic_qh.overlay);
        if token & QTD_ERROR_BITS != 0 {
            return Err("EHCI interrupt qTD reported a transfer error");
        }
        if token & QTD_ACTIVE != 0 {
            return Ok(None);
        }
        let transferred = length.saturating_sub(qtd_remaining(token));
        if transferred != 0 {
            *data_toggle = !*data_toggle;
        }
        Ok(Some(transferred))
    }

    fn submit_qtds(
        &mut self,
        device_address: u8,
        endpoint: u8,
        max_packet_size: u16,
        control: bool,
        qtds: &mut [Box<Qtd>],
    ) -> Result<(), &'static str> {
        let qh_phys = dma32(&*self.transfer_qh)?;
        let qtd_phys = qtds
            .iter()
            .map(|qtd| dma32(&**qtd))
            .collect::<Result<alloc::vec::Vec<_>, _>>()?;
        for index in 0..qtds.len() {
            qtds[index].next = qtd_phys.get(index + 1).copied().unwrap_or(QTD_TERMINATE);
            let is_in_data_stage = control && qtds[index].token & 0x300 == QTD_PID_IN;
            qtds[index].alternate = if is_in_data_stage {
                qtd_phys.get(index + 1).copied().unwrap_or(QTD_TERMINATE)
            } else {
                QTD_TERMINATE
            };
        }

        self.stop_async()?;
        self.transfer_qh.link = self.async_head.link;
        self.transfer_qh.endpoint_characteristics = (u32::from(device_address) & 0x7f)
            | ((u32::from(endpoint) & 0xf) << 8)
            | (2 << 12)
            | (1 << 14)
            | ((u32::from(max_packet_size) & 0x7ff) << 16)
            | (4 << 28)
            | if control { 1 << 27 } else { 0 };
        self.transfer_qh.endpoint_capabilities = 1 << 30;
        set_qh_overlay(&mut self.transfer_qh, qtd_phys[0]);
        self.async_head.link = qh_phys | QH_TYPE;
        compiler_fence(Ordering::Release);

        write32(
            self.op,
            OP_USBCMD,
            read32(self.op, OP_USBCMD) | CMD_ASYNC_ENABLE,
        );
        if !wait_for(TRANSFER_POLLS, || {
            read32(self.op, OP_USBSTS) & STS_ASYNC != 0
        }) {
            crate::println!(
                "EHCI async schedule timeout: USBCMD={:#010x} USBSTS={:#010x} ASYNCLISTADDR={:#010x} head_link={:#010x} qh_phys={:#010x} qh_link={:#010x} qh_current={:#010x} overlay_next={:#010x} overlay_alt={:#010x} overlay_token={:#010x} qtd_phys={:#010x}",
                read32(self.op, OP_USBCMD),
                read32(self.op, OP_USBSTS),
                read32(self.op, OP_ASYNCLISTADDR),
                self.async_head.link,
                qh_phys,
                self.transfer_qh.link,
                self.transfer_qh.current_qtd,
                self.transfer_qh.overlay.next,
                self.transfer_qh.overlay.alternate,
                self.transfer_qh.overlay.token,
                qtd_phys[0]
            );
            self.remove_transfer_qh();
            return Err("EHCI async schedule failed to enable");
        }

        let mut complete = false;
        for _ in 0..TRANSFER_POLLS {
            compiler_fence(Ordering::Acquire);
            if qtds.iter().all(|qtd| volatile_token(qtd) & QTD_ACTIVE == 0) {
                complete = true;
                break;
            }
            core::hint::spin_loop();
        }
        self.stop_async()?;
        self.remove_transfer_qh();
        if !complete {
            return Err("EHCI transfer timed out");
        }
        if qtds
            .iter()
            .any(|qtd| volatile_token(qtd) & QTD_ERROR_BITS != 0)
        {
            return Err("EHCI qTD reported a transfer error");
        }
        Ok(())
    }

    fn stop_async(&self) -> Result<(), &'static str> {
        write32(
            self.op,
            OP_USBCMD,
            read32(self.op, OP_USBCMD) & !CMD_ASYNC_ENABLE,
        );
        if wait_for(TRANSFER_POLLS, || {
            read32(self.op, OP_USBSTS) & STS_ASYNC == 0
        }) {
            Ok(())
        } else {
            Err("EHCI async schedule did not stop")
        }
    }

    fn remove_transfer_qh(&mut self) {
        self.async_head.link = self.transfer_qh.link;
        compiler_fence(Ordering::Release);
        set_qh_overlay(&mut self.transfer_qh, QTD_TERMINATE);
    }
}

fn set_qh_overlay(qh: &mut Qh, first_qtd: u32) {
    qh.overlay.next = first_qtd;
    qh.overlay.alternate = QTD_TERMINATE;
    qh.overlay.token = 0;
    qh.overlay.buffers = [0; 5];
}

fn read32(base: u64, offset: u64) -> u32 {
    unsafe { ((base + offset) as *const u32).read_volatile() }
}

fn write32(base: u64, offset: u64, value: u32) {
    unsafe { ((base + offset) as *mut u32).write_volatile(value) }
}

fn dma32<T>(value: &T) -> Result<u32, &'static str> {
    let physical = memory::virtual_to_physical(value as *const T as usize)
        .ok_or("EHCI DMA address translation failed")?;
    u32::try_from(physical).map_err(|_| "EHCI requires DMA memory below 4 GiB")
}

fn set_qtd_buffer(qtd: &mut Qtd, address: u64, length: usize) -> Result<(), &'static str> {
    if length > MAX_QTD_BYTES || address.checked_add(length as u64).is_none() {
        return Err("EHCI qTD buffer range is invalid");
    }
    let first = u32::try_from(address).map_err(|_| "EHCI buffer address exceeds 32 bits")?;
    let pages = ((address as usize & 0xfff) + length.max(1)).div_ceil(4096);
    if pages > qtd.buffers.len() {
        return Err("EHCI qTD buffer spans too many pages");
    }
    qtd.buffers[0] = first;
    for (index, buffer) in qtd.buffers.iter_mut().enumerate().take(pages).skip(1) {
        let page = (address & !0xfff)
            .checked_add((index * 4096) as u64)
            .ok_or("EHCI qTD page address overflow")?;
        *buffer = u32::try_from(page).map_err(|_| "EHCI qTD page exceeds 32 bits")?;
    }
    Ok(())
}

fn qtd_token(pid: u32, length: usize, toggle: bool, active: bool) -> u32 {
    pid | QTD_CERR_3
        | ((length as u32 & 0x7fff) << 16)
        | if toggle { QTD_TOGGLE } else { 0 }
        | if active { QTD_ACTIVE } else { 0 }
}

fn qtd_remaining(token: u32) -> usize {
    ((token >> 16) & 0x7fff) as usize
}

fn volatile_token(qtd: &Qtd) -> u32 {
    unsafe { core::ptr::addr_of!(qtd.token).read_volatile() }
}

fn volatile_overlay_token(qtd: &QtdOverlay) -> u32 {
    unsafe { core::ptr::addr_of!(qtd.token).read_volatile() }
}

fn wait_for(mut polls: usize, mut predicate: impl FnMut() -> bool) -> bool {
    while polls > 0 {
        if predicate() {
            return true;
        }
        polls -= 1;
        core::hint::spin_loop();
    }
    false
}

fn delay(microseconds: usize) {
    for _ in 0..microseconds.saturating_mul(64) {
        core::hint::spin_loop();
    }
}

pub(crate) fn matches(device: &PciDevice) -> bool {
    device.matches_class(PCI_CLASS_USB, PCI_SUBCLASS_USB, PCI_PROG_IF_EHCI)
}

#[cfg(test)]
mod tests {
    use super::{
        FRAME_LIST_ENTRIES, FrameList, MAX_QTD_BYTES, QH_TERMINATE, QTD_ACTIVE, QTD_PID_SETUP,
        QTD_TERMINATE, Qh, Qtd, parse_configuration_endpoints, qtd_token, set_qh_overlay,
        set_qtd_buffer,
    };

    #[test]
    fn schedule_descriptors_have_ehci_required_alignment_and_size() {
        assert_eq!(core::mem::align_of::<Qtd>(), 32);
        assert_eq!(core::mem::size_of::<Qtd>(), 32);
        assert_eq!(core::mem::align_of::<Qh>(), 64);
        assert_eq!(core::mem::size_of::<Qh>(), 64);
        assert_eq!(core::mem::offset_of!(Qh, overlay), 16);
        assert_eq!(core::mem::align_of::<FrameList>(), 4096);
        assert_eq!(core::mem::size_of::<FrameList>(), FRAME_LIST_ENTRIES * 4);
        assert_eq!(QH_TERMINATE, 1);
    }

    #[test]
    fn queue_head_overlay_points_to_first_qtd() {
        let mut qh = Qh::default();
        set_qh_overlay(&mut qh, 0x1234_5600);

        assert_eq!(qh.current_qtd, 0);
        assert_eq!(qh.overlay.next, 0x1234_5600);
        assert_eq!(qh.overlay.alternate, QTD_TERMINATE);
        assert_eq!(qh.overlay.token, 0);
        assert_eq!(qh.overlay.buffers, [0; 5]);
    }

    #[test]
    fn qtd_token_encodes_pid_length_toggle_and_active() {
        let token = qtd_token(QTD_PID_SETUP, 8, false, true);
        assert_eq!(token & 0x300, QTD_PID_SETUP);
        assert_eq!((token >> 16) & 0x7fff, 8);
        assert_ne!(token & QTD_ACTIVE, 0);
    }

    #[test]
    fn qtd_buffer_encodes_cross_page_addresses() {
        let mut qtd = Qtd::default();
        set_qtd_buffer(&mut qtd, 0x1ff0, 32).unwrap();
        assert_eq!(qtd.buffers[0], 0x1ff0);
        assert_eq!(qtd.buffers[1], 0x2000);
    }

    #[test]
    fn qtd_buffer_rejects_ranges_outside_descriptor_capacity() {
        let mut qtd = Qtd::default();
        assert!(set_qtd_buffer(&mut qtd, 0x1000, MAX_QTD_BYTES + 1).is_err());
        assert!(set_qtd_buffer(&mut qtd, 0xffff_f000, 0x2000).is_err());
    }

    #[test]
    fn parses_mass_storage_interface_and_bulk_endpoints() {
        let configuration = [
            9, 2, 32, 0, 1, 1, 0, 0x80, 50, // configuration
            9, 4, 0, 0, 2, 8, 6, 0x50, 0, // interface
            7, 5, 0x02, 2, 0, 2, 0, // bulk OUT, 512 bytes
            7, 5, 0x81, 2, 0, 2, 0, // bulk IN, 512 bytes
        ];
        let endpoints = parse_configuration_endpoints(&configuration).unwrap();
        assert_eq!(endpoints.len(), 2);
        assert_eq!(endpoints[0].inteface_number, 0);
        assert_eq!(endpoints[0].interface_class, 8);
        assert_eq!(endpoints[0].interface_subclass, 6);
        assert_eq!(endpoints[0].interface_protocol, 0x50);
        assert_eq!(endpoints[0].max_packet_size, 512);
        assert_eq!(endpoints[1].address, 0x81);
    }

    #[test]
    fn rejects_truncated_configuration_descriptors() {
        assert!(parse_configuration_endpoints(&[9, 2, 10, 0, 1, 1, 0, 0x80, 50]).is_err());
    }
}
