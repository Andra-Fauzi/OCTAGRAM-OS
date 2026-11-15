//! OHCI (USB 1.1) controller support for control and bulk transfers.

use super::UsbPortEvent;
use crate::drivers::pci::{self, PciDevice};
use crate::memory;
use alloc::boxed::Box;
use core::sync::atomic::{Ordering, compiler_fence};
use spin::Mutex;

const PCI_CLASS_USB: u8 = 0x0c;
const PCI_SUBCLASS_USB: u8 = 0x03;
const PCI_PROG_IF_OHCI: u8 = 0x10;

const REG_CONTROL: u64 = 0x04;
const REG_COMMAND_STATUS: u64 = 0x08;
const REG_INTERRUPT_STATUS: u64 = 0x0c;
const REG_INTERRUPT_DISABLE: u64 = 0x14;
const REG_HCCA: u64 = 0x18;
const REG_CONTROL_HEAD_ED: u64 = 0x20;
const REG_BULK_HEAD_ED: u64 = 0x28;
const REG_FRAME_INTERVAL: u64 = 0x34;
const REG_FRAME_NUMBER: u64 = 0x3c;
const REG_PERIODIC_START: u64 = 0x40;
const REG_ROOT_HUB_DESCRIPTOR_A: u64 = 0x48;
const REG_ROOT_HUB_STATUS: u64 = 0x50;
const REG_ROOT_HUB_PORT_STATUS: u64 = 0x54;

const CONTROL_CLE: u32 = 1 << 4;
const CONTROL_BLE: u32 = 1 << 5;
const CONTROL_PLE: u32 = 1 << 2;
const CONTROL_HCFS_MASK: u32 = 3 << 6;
const CONTROL_HCFS_OPERATIONAL: u32 = 2 << 6;
const COMMAND_HCR: u32 = 1;
const COMMAND_CLF: u32 = 1 << 1;
const COMMAND_BLF: u32 = 1 << 2;
const RH_DESCRIPTOR_PSM: u32 = 1 << 8;
const RH_DESCRIPTOR_NPS: u32 = 1 << 9;
const RH_STATUS_SET_GLOBAL_POWER: u32 = 1 << 16;
const PORT_CONNECT: u32 = 1;
const PORT_ENABLE: u32 = 1 << 1;
const PORT_RESET: u32 = 1 << 4;
const PORT_POWER: u32 = 1 << 8;
const PORT_LOW_SPEED: u32 = 1 << 9;
const PORT_CHANGE_BITS: u32 = (1 << 16) | (1 << 17) | (1 << 18) | (1 << 19) | (1 << 20);
const ED_SKIP: u32 = 1 << 14;
const TD_CC_NOT_ACCESSED: u32 = 0xf << 28;
const TD_CC_MASK: u32 = 0xf << 28;
const TD_CC_NO_ERROR: u32 = 0;
const TD_CC_DATA_UNDERRUN: u32 = 9;
const TD_DP_SETUP: u32 = 0;
const TD_DP_OUT: u32 = 1 << 19;
const TD_DP_IN: u32 = 2 << 19;
const TD_T_DATA0: u32 = 1 << 24;
const TD_T_DATA1: u32 = 2 << 24;
const TD_DELAY_INTERRUPT: u32 = 7 << 21;
const TD_ROUNDING: u32 = 1 << 18;
const HCCA_INTERRUPT_ENTRIES: usize = 32;
const TRANSFER_POLLS: usize = 10_000_000;

pub static OHCI_CONTROLLERS: Mutex<alloc::vec::Vec<OhciController>> =
    Mutex::new(alloc::vec::Vec::new());

#[repr(C, align(16))]
#[derive(Clone, Copy, Default)]
struct EndpointDescriptor {
    control: u32,
    tail_td: u32,
    head_td: u32,
    next_ed: u32,
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Default)]
struct TransferDescriptor {
    control: u32,
    current_buffer: u32,
    next_td: u32,
    buffer_end: u32,
}

#[repr(C, align(256))]
struct Hcca {
    interrupt_table: [u32; HCCA_INTERRUPT_ENTRIES],
    frame_number: u16,
    padding: u16,
    done_head: u32,
    reserved: [u32; 30],
}

#[repr(C, align(16))]
struct SetupPacket([u8; 8]);

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

pub struct OhciUsbDevice {
    pub root_port: u8,
    pub address: u8,
    pub max_packet_size: u16,
    pub low_speed: bool,
    pub configuration_descriptor: alloc::vec::Vec<u8>,
}

pub struct OhciController {
    mmio: u64,
    port_count: u8,
    hcca: Box<Hcca>,
    control_ed: Box<EndpointDescriptor>,
    bulk_ed: Box<EndpointDescriptor>,
    interrupt_ed: Box<EndpointDescriptor>,
    devices: alloc::vec::Vec<OhciUsbDevice>,
    known_ports: alloc::vec::Vec<u8>,
}

#[derive(Clone, Copy)]
enum Schedule {
    Control,
    Bulk,
}

impl OhciController {
    /// Initialize a PCI OHCI controller.
    ///
    /// # Safety
    /// The PCI function must be OHCI and exclusively owned by this driver.
    pub unsafe fn new(device: &PciDevice) -> Result<Self, &'static str> {
        unsafe {
            pci::enable_device(device);
            let bar = pci::get_bar0(device);
            if bar.address == 0 {
                return Err("OHCI BAR0 is not assigned");
            }
            let size = pci::get_bar0_size(device);
            if size < 0x58 {
                return Err("OHCI BAR0 is too small");
            }
            let mmio = memory::paging::map_mmio_range(bar.address, size);
            let root_hub = read32(mmio, REG_ROOT_HUB_DESCRIPTOR_A);
            let port_count = (root_hub & 0xff) as u8;
            if port_count == 0 || REG_ROOT_HUB_PORT_STATUS + u64::from(port_count) * 4 > size {
                return Err("invalid OHCI root-port count");
            }

            let mut controller = Self {
                mmio,
                port_count,
                hcca: Box::new(Hcca {
                    interrupt_table: [0; HCCA_INTERRUPT_ENTRIES],
                    frame_number: 0,
                    padding: 0,
                    done_head: 0,
                    reserved: [0; 30],
                }),
                control_ed: Box::new(EndpointDescriptor::default()),
                bulk_ed: Box::new(EndpointDescriptor::default()),
                interrupt_ed: Box::new(EndpointDescriptor::default()),
                devices: alloc::vec::Vec::new(),
                known_ports: alloc::vec::Vec::new(),
            };
            controller.initialize()?;
            Ok(controller)
        }
    }

    fn initialize(&mut self) -> Result<(), &'static str> {
        write32(self.mmio, REG_INTERRUPT_DISABLE, 0xffff_ffff);
        write32(self.mmio, REG_INTERRUPT_STATUS, 0x7f);
        write32(
            self.mmio,
            REG_CONTROL,
            read32(self.mmio, REG_CONTROL) & !CONTROL_HCFS_MASK,
        );
        if !wait_for(TRANSFER_POLLS, || {
            read32(self.mmio, REG_CONTROL) & CONTROL_HCFS_MASK == 0
        }) {
            return Err("OHCI did not enter USB reset state");
        }

        write32(self.mmio, REG_COMMAND_STATUS, COMMAND_HCR);
        if !wait_for(TRANSFER_POLLS, || {
            read32(self.mmio, REG_COMMAND_STATUS) & COMMAND_HCR == 0
        }) {
            return Err("OHCI host reset timed out");
        }

        let hcca_phys = dma32(&*self.hcca)?;
        write32(self.mmio, REG_HCCA, hcca_phys);
        write32(self.mmio, REG_CONTROL_HEAD_ED, 0);
        write32(self.mmio, REG_BULK_HEAD_ED, 0);

        let frame_interval = 0x2edf | (0x2778 << 16);
        write32(self.mmio, REG_FRAME_INTERVAL, frame_interval);
        write32(self.mmio, REG_PERIODIC_START, (0x2edf * 9) / 10);
        write32(self.mmio, REG_INTERRUPT_STATUS, 0x7f);

        let control = read32(self.mmio, REG_CONTROL);
        write32(
            self.mmio,
            REG_CONTROL,
            (control & !(CONTROL_HCFS_MASK | CONTROL_CLE | CONTROL_BLE)) | CONTROL_HCFS_OPERATIONAL,
        );
        if !wait_for(TRANSFER_POLLS, || {
            read32(self.mmio, REG_CONTROL) & CONTROL_HCFS_MASK == CONTROL_HCFS_OPERATIONAL
        }) {
            return Err("OHCI failed to enter operational state");
        }

        let root_hub = read32(self.mmio, REG_ROOT_HUB_DESCRIPTOR_A);
        if root_hub & RH_DESCRIPTOR_NPS == 0 {
            if root_hub & RH_DESCRIPTOR_PSM == 0 {
                write32(self.mmio, REG_ROOT_HUB_STATUS, RH_STATUS_SET_GLOBAL_POWER);
            } else {
                for port in 0..self.port_count {
                    let offset = REG_ROOT_HUB_PORT_STATUS + u64::from(port) * 4;
                    write32(self.mmio, offset, PORT_POWER);
                }
            }
        }
        Ok(())
    }

    pub fn port_count(&self) -> u8 {
        self.port_count
    }

    pub fn devices(&self) -> &[OhciUsbDevice] {
        &self.devices
    }

    pub fn frame_number(&self) -> u16 {
        read32(self.mmio, REG_FRAME_NUMBER) as u16
    }

    pub fn port_status(&self, port: u8) -> Result<u32, &'static str> {
        if port >= self.port_count {
            return Err("OHCI port index out of range");
        }
        Ok(read32(
            self.mmio,
            REG_ROOT_HUB_PORT_STATUS + u64::from(port) * 4,
        ))
    }

    pub fn reset_port(&self, port: u8) -> Result<bool, &'static str> {
        if port >= self.port_count {
            return Err("OHCI port index out of range");
        }
        let offset = REG_ROOT_HUB_PORT_STATUS + u64::from(port) * 4;
        let status = read32(self.mmio, offset);
        if status & PORT_CONNECT == 0 {
            return Err("OHCI port has no connected device");
        }
        let frame_start = read32(self.mmio, REG_FRAME_NUMBER) as u16;
        write32(self.mmio, offset, (status & PORT_CHANGE_BITS) | PORT_RESET);
        if !self.wait_frames_since(frame_start, 10) {
            return Err("OHCI port reset duration timed out");
        }
        if !wait_for(TRANSFER_POLLS, || {
            read32(self.mmio, offset) & PORT_RESET == 0
        }) {
            return Err("OHCI port reset timed out");
        }
        let status = read32(self.mmio, offset);
        write32(self.mmio, offset, status & PORT_CHANGE_BITS);
        if status & PORT_ENABLE == 0 {
            return Err("OHCI port did not enable after reset");
        }
        Ok(status & PORT_LOW_SPEED != 0)
    }

    fn wait_frames_since(&self, frame_start: u16, frames: u16) -> bool {
        wait_for(TRANSFER_POLLS, || {
            let current = read32(self.mmio, REG_FRAME_NUMBER) as u16;
            current.wrapping_sub(frame_start) >= frames
        })
    }

    pub fn scan_ports(&mut self) {
        for port_index in 0..self.port_count {
            if !matches!(self.port_status(port_index), Ok(status) if status & PORT_CONNECT != 0) {
                continue;
            }
            self.known_ports.push(port_index);
            match self.enumerate_port(port_index) {
                Ok(device) => {
                    crate::println!(
                        "OHCI USB device at port {} addressed as {} ({} config bytes)",
                        device.root_port,
                        device.address,
                        device.configuration_descriptor.len()
                    );
                    self.devices.push(device);
                }
                Err(error) => {
                    crate::println!("OHCI port {} enumeration failed: {}", port_index + 1, error);
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
                        "OHCI hotplug port {} status failed: {}",
                        port_index + 1,
                        error
                    );
                    continue;
                }
            };
            if status & PORT_CHANGE_BITS != 0 {
                write32(
                    self.mmio,
                    REG_ROOT_HUB_PORT_STATUS + u64::from(port_index) * 4,
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
                        "OHCI hotplug port {} enumeration failed: {}",
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

    fn enumerate_port(&mut self, port_index: u8) -> Result<OhciUsbDevice, &'static str> {
        let root_port = port_index + 1;
        let low_speed = self.reset_port(port_index)?;
        self.enumerate_device(root_port, root_port, low_speed)
    }

    fn enumerate_device(
        &mut self,
        root_port: u8,
        address: u8,
        low_speed: bool,
    ) -> Result<OhciUsbDevice, &'static str> {
        let buffer = Box::new(DescriptorBuffer([0; 4096]));
        let buffer_phys = u64::from(dma32(&*buffer)?);
        let length =
            self.control_transfer(0, 8, low_speed, 0x80, 0x06, 0x0100, 0, Some(buffer_phys), 8)?;
        if length != 8 {
            return Err("short initial OHCI device descriptor");
        }
        let first = buffer.read(8);
        if first[0] < 8 || first[1] != 1 {
            return Err("invalid initial OHCI device descriptor");
        }
        let max_packet_size = u16::from(first[7]);
        if max_packet_size == 0 || max_packet_size > 64 {
            return Err("unsupported OHCI endpoint-zero packet size");
        }

        self.control_transfer(
            0,
            max_packet_size,
            low_speed,
            0,
            0x05,
            u16::from(address),
            0,
            None,
            0,
        )?;
        let frame_start = read32(self.mmio, REG_FRAME_NUMBER) as u16;
        if !self.wait_frames_since(frame_start, 2) {
            return Err("OHCI SET_ADDRESS recovery delay timed out");
        }
        let length = self.control_transfer(
            address,
            max_packet_size,
            low_speed,
            0x80,
            0x06,
            0x0100,
            0,
            Some(buffer_phys),
            18,
        )?;
        if length != 18 {
            return Err("short OHCI device descriptor");
        }

        let length = self.control_transfer(
            address,
            max_packet_size,
            low_speed,
            0x80,
            0x06,
            0x0200,
            0,
            Some(buffer_phys),
            9,
        )?;
        if length != 9 {
            return Err("short OHCI configuration descriptor header");
        }
        let header = buffer.read(9);
        if header[0] < 9 || header[1] != 2 {
            return Err("invalid OHCI configuration descriptor header");
        }
        let total_length = u16::from_le_bytes([header[2], header[3]]) as usize;
        if !(9..=4096).contains(&total_length) {
            return Err("OHCI configuration descriptor length is unsupported");
        }
        let length = self.control_transfer(
            address,
            max_packet_size,
            low_speed,
            0x80,
            0x06,
            0x0200,
            0,
            Some(buffer_phys),
            total_length,
        )?;
        if length != total_length {
            return Err("short OHCI configuration descriptor");
        }
        let configuration_descriptor = buffer.read(total_length);
        if configuration_descriptor[0] < 9 || configuration_descriptor[1] != 2 {
            return Err("invalid OHCI configuration descriptor");
        }
        let configuration_value = configuration_descriptor[5];
        self.control_transfer(
            address,
            max_packet_size,
            low_speed,
            0,
            0x09,
            u16::from(configuration_value),
            0,
            None,
            0,
        )?;

        Ok(OhciUsbDevice {
            root_port,
            address,
            max_packet_size,
            low_speed,
            configuration_descriptor,
        })
    }

    pub fn control_transfer(
        &mut self,
        address: u8,
        max_packet_size: u16,
        low_speed: bool,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        data_phys: Option<u64>,
        length: usize,
    ) -> Result<usize, &'static str> {
        if address > 127 || max_packet_size == 0 || max_packet_size > 64 {
            return Err("invalid OHCI control endpoint parameters");
        }
        if length > 4096 || (length != 0 && data_phys.is_none()) {
            return Err("OHCI control transfer length is unsupported");
        }

        let mut setup = Box::new(SetupPacket([0; 8]));
        setup.0[0] = request_type;
        setup.0[1] = request;
        setup.0[2..4].copy_from_slice(&value.to_le_bytes());
        setup.0[4..6].copy_from_slice(&index.to_le_bytes());
        setup.0[6..8].copy_from_slice(&(length as u16).to_le_bytes());
        let setup_phys = u64::from(dma32(&*setup)?);

        let mut tds = alloc::vec::Vec::new();
        let mut setup_td = Box::new(TransferDescriptor::default());
        setup_td.control = td_control(TD_DP_SETUP, TD_T_DATA0, false);
        set_td_buffer(&mut setup_td, setup_phys, 8)?;
        tds.push(setup_td);
        if length != 0 {
            let is_in = request_type & 0x80 != 0;
            let mut data_td = Box::new(TransferDescriptor::default());
            data_td.control =
                td_control(if is_in { TD_DP_IN } else { TD_DP_OUT }, TD_T_DATA1, is_in);
            set_td_buffer(&mut data_td, data_phys.unwrap(), length)?;
            tds.push(data_td);
        }
        let status_in = length == 0 || request_type & 0x80 == 0;
        let mut status_td = Box::new(TransferDescriptor::default());
        status_td.control = td_control(
            if status_in { TD_DP_IN } else { TD_DP_OUT },
            TD_T_DATA1,
            false,
        );
        tds.push(status_td);

        let data_td_index = (length != 0).then_some(1);
        self.submit(
            Schedule::Control,
            address,
            0,
            max_packet_size,
            low_speed,
            &mut tds,
        )?;
        if let Some(data_index) = data_td_index {
            let td = &tds[data_index];
            check_td(td, true)?;
            Ok(length.saturating_sub(td_remaining(
                volatile_u32(&td.current_buffer),
                td.buffer_end,
            )))
        } else {
            Ok(0)
        }
    }

    pub fn bulk_transfer(
        &mut self,
        address: u8,
        endpoint_address: u8,
        max_packet_size: u16,
        data_phys: u64,
        length: usize,
        data_toggle: &mut bool,
    ) -> Result<usize, &'static str> {
        if address == 0
            || address > 127
            || endpoint_address & 0x0f == 0
            || max_packet_size == 0
            || max_packet_size > 64
            || length == 0
            || length > 4096
        {
            return Err("invalid OHCI bulk endpoint parameters");
        }
        let is_in = endpoint_address & 0x80 != 0;
        let mut td = Box::new(TransferDescriptor::default());
        td.control = td_control(
            if is_in { TD_DP_IN } else { TD_DP_OUT },
            if *data_toggle { TD_T_DATA1 } else { TD_T_DATA0 },
            is_in,
        );
        set_td_buffer(&mut td, data_phys, length)?;
        let mut tds = alloc::vec![td];
        self.submit(
            Schedule::Bulk,
            address,
            endpoint_address & 0x0f,
            max_packet_size,
            false,
            &mut tds,
        )?;
        check_td(&tds[0], is_in)?;
        let remaining = td_remaining(volatile_u32(&tds[0].current_buffer), tds[0].buffer_end);
        let transferred = length.saturating_sub(remaining);
        *data_toggle ^=
            ((transferred + usize::from(max_packet_size) - 1) / usize::from(max_packet_size)) % 2
                != 0;
        Ok(transferred)
    }

    pub fn periodic_interrupt_in(
        &mut self,
        address: u8,
        endpoint_address: u8,
        max_packet_size: u16,
        low_speed: bool,
        data_phys: u64,
        length: usize,
        data_toggle: &mut bool,
    ) -> Result<Option<usize>, &'static str> {
        let endpoint = endpoint_address & 0x0f;
        if address == 0
            || address > 127
            || endpoint == 0
            || endpoint_address & 0x80 == 0
            || max_packet_size == 0
            || max_packet_size > 64
            || length == 0
            || length > usize::from(max_packet_size)
        {
            return Err("invalid OHCI interrupt-IN parameters");
        }

        let frame_start = read32(self.mmio, REG_FRAME_NUMBER) as u16;
        let schedule_index =
            usize::from(frame_start.wrapping_add(1) % HCCA_INTERRUPT_ENTRIES as u16);
        let ed_phys = dma32(&*self.interrupt_ed)?;
        let mut td = Box::new(TransferDescriptor::default());
        td.control = td_control(
            TD_DP_IN,
            if *data_toggle { TD_T_DATA1 } else { TD_T_DATA0 },
            true,
        );
        set_td_buffer(&mut td, data_phys, length)?;
        let mut dummy = Box::new(TransferDescriptor::default());
        dummy.control = TD_CC_NOT_ACCESSED;
        let td_phys = dma32(&*td)?;
        let dummy_phys = dma32(&*dummy)?;

        self.interrupt_ed.control = (u32::from(address) & 0x7f)
            | (u32::from(endpoint) << 7)
            | (2 << 11)
            | (u32::from(low_speed) << 13)
            | ED_SKIP
            | ((u32::from(max_packet_size) & 0x7ff) << 16);
        self.interrupt_ed.tail_td = dummy_phys;
        self.interrupt_ed.head_td = td_phys;
        self.interrupt_ed.next_ed = 0;
        td.next_td = dummy_phys;
        self.hcca.interrupt_table[schedule_index] = ed_phys;
        compiler_fence(Ordering::Release);

        self.interrupt_ed.control &= !ED_SKIP;
        write32(
            self.mmio,
            REG_CONTROL,
            read32(self.mmio, REG_CONTROL) | CONTROL_PLE | CONTROL_HCFS_OPERATIONAL,
        );
        let frame_advanced = self.wait_frames_since(frame_start, 2);
        write32(
            self.mmio,
            REG_CONTROL,
            read32(self.mmio, REG_CONTROL) & !CONTROL_PLE,
        );
        self.interrupt_ed.control |= ED_SKIP;
        self.hcca.interrupt_table[schedule_index] = 0;
        compiler_fence(Ordering::Release);
        let schedule_stopped =
            self.wait_frames_since(read32(self.mmio, REG_FRAME_NUMBER) as u16, 2);

        if !frame_advanced {
            return Err("OHCI periodic frame did not advance");
        }
        if !schedule_stopped {
            return Err("OHCI periodic schedule did not stop");
        }
        let completion = (volatile_u32(&td.control) & TD_CC_MASK) >> 28;
        if completion == 0xf {
            return Ok(None);
        }
        check_td(&td, true)?;
        let transferred = length.saturating_sub(td_remaining(
            volatile_u32(&td.current_buffer),
            td.buffer_end,
        ));
        if transferred != 0 {
            *data_toggle = !*data_toggle;
        }
        Ok(Some(transferred))
    }

    fn submit(
        &mut self,
        schedule: Schedule,
        address: u8,
        endpoint: u8,
        max_packet_size: u16,
        low_speed: bool,
        tds: &mut [Box<TransferDescriptor>],
    ) -> Result<(), &'static str> {
        let ed = match schedule {
            Schedule::Control => &mut self.control_ed,
            Schedule::Bulk => &mut self.bulk_ed,
        };
        let ed_phys = dma32(&**ed)?;
        let mut dummy = Box::new(TransferDescriptor::default());
        dummy.control = TD_CC_NOT_ACCESSED;
        let dummy_phys = dma32(&*dummy)?;
        let td_phys = tds
            .iter()
            .map(|td| dma32(&**td))
            .collect::<Result<alloc::vec::Vec<_>, _>>()?;
        for (index, td) in tds.iter_mut().enumerate() {
            td.next_td = td_phys.get(index + 1).copied().unwrap_or(dummy_phys);
        }

        ed.control = (u32::from(address) & 0x7f)
            | ((u32::from(endpoint) & 0xf) << 7)
            | (u32::from(low_speed) << 13)
            | ED_SKIP
            | ((u32::from(max_packet_size) & 0x7ff) << 16);
        ed.tail_td = dummy_phys;
        ed.head_td = td_phys[0];
        ed.next_ed = 0;
        compiler_fence(Ordering::Release);

        let (head_reg, enable_bit, command_flag) = match schedule {
            Schedule::Control => (REG_CONTROL_HEAD_ED, CONTROL_CLE, COMMAND_CLF),
            Schedule::Bulk => (REG_BULK_HEAD_ED, CONTROL_BLE, COMMAND_BLF),
        };
        write32(self.mmio, head_reg, ed_phys);
        ed.control &= !ED_SKIP;
        compiler_fence(Ordering::Release);
        write32(
            self.mmio,
            REG_CONTROL,
            read32(self.mmio, REG_CONTROL) | enable_bit | CONTROL_HCFS_OPERATIONAL,
        );
        write32(self.mmio, REG_COMMAND_STATUS, command_flag);

        let mut complete = false;
        for _ in 0..TRANSFER_POLLS {
            compiler_fence(Ordering::Acquire);
            let head = volatile_u32(&ed.head_td);
            if head & 1 != 0 || head & !0xf == dummy_phys {
                complete = head & 1 == 0 && head & !0xf == dummy_phys;
                break;
            }
            core::hint::spin_loop();
        }

        write32(
            self.mmio,
            REG_CONTROL,
            read32(self.mmio, REG_CONTROL) & !enable_bit,
        );
        write32(self.mmio, head_reg, 0);
        ed.control |= ED_SKIP;
        let frame_start = read32(self.mmio, REG_FRAME_NUMBER) as u16;
        if !self.wait_frames_since(frame_start, 2) {
            return Err("OHCI schedule shutdown did not complete");
        }
        compiler_fence(Ordering::Acquire);
        if !complete {
            return Err("OHCI transfer timed out or endpoint halted");
        }
        for td in tds.iter() {
            let is_data_in = td.control & (3 << 19) == TD_DP_IN;
            check_td(td, is_data_in)?;
            if td.control & (3 << 19) == TD_DP_SETUP
                && td_remaining(volatile_u32(&td.current_buffer), td.buffer_end) != 0
            {
                return Err("OHCI setup/status TD completed with residual data");
            }
        }
        Ok(())
    }
}

fn td_control(pid: u32, toggle: u32, rounding: bool) -> u32 {
    TD_CC_NOT_ACCESSED | TD_DELAY_INTERRUPT | pid | toggle | if rounding { TD_ROUNDING } else { 0 }
}

fn set_td_buffer(
    td: &mut TransferDescriptor,
    address: u64,
    length: usize,
) -> Result<(), &'static str> {
    if length == 0 {
        td.current_buffer = 0;
        td.buffer_end = 0;
        return Ok(());
    }
    let end = address
        .checked_add((length - 1) as u64)
        .ok_or("OHCI transfer buffer address overflow")?;
    let cbp = u32::try_from(address).map_err(|_| "OHCI buffer exceeds 32 bits")?;
    let buffer_end = u32::try_from(end).map_err(|_| "OHCI buffer end exceeds 32 bits")?;
    let start_page = address & !0xfff;
    let end_page = end & !0xfff;
    if end_page != start_page && end_page != start_page + 4096 {
        return Err("OHCI TD buffer spans more than two pages");
    }
    td.current_buffer = cbp;
    td.buffer_end = buffer_end;
    Ok(())
}

fn check_td(td: &TransferDescriptor, allow_data_underrun: bool) -> Result<(), &'static str> {
    let completion = (volatile_u32(&td.control) & TD_CC_MASK) >> 28;
    if completion == TD_CC_NO_ERROR || (allow_data_underrun && completion == TD_CC_DATA_UNDERRUN) {
        Ok(())
    } else {
        Err("OHCI transfer descriptor reported an error")
    }
}

fn td_remaining(current_buffer: u32, buffer_end: u32) -> usize {
    if current_buffer == 0 {
        0
    } else {
        (buffer_end.wrapping_sub(current_buffer) as usize).saturating_add(1)
    }
}

fn volatile_u32(value: &u32) -> u32 {
    unsafe { core::ptr::addr_of!(*value).read_volatile() }
}

fn read32(base: u64, offset: u64) -> u32 {
    unsafe { ((base + offset) as *const u32).read_volatile() }
}

fn write32(base: u64, offset: u64, value: u32) {
    unsafe { ((base + offset) as *mut u32).write_volatile(value) }
}

fn dma32<T>(value: &T) -> Result<u32, &'static str> {
    let physical = memory::virtual_to_physical(value as *const T as usize)
        .ok_or("OHCI DMA address translation failed")?;
    u32::try_from(physical).map_err(|_| "OHCI requires DMA memory below 4 GiB")
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

pub(crate) fn matches(device: &PciDevice) -> bool {
    device.matches_class(PCI_CLASS_USB, PCI_SUBCLASS_USB, PCI_PROG_IF_OHCI)
}

#[cfg(test)]
mod tests {
    use super::{
        EndpointDescriptor, HCCA_INTERRUPT_ENTRIES, Hcca, TD_DP_IN, TD_T_DATA1, TransferDescriptor,
        set_td_buffer, td_control,
    };

    #[test]
    fn ohci_dma_structures_match_required_layouts() {
        assert_eq!(core::mem::size_of::<EndpointDescriptor>(), 16);
        assert_eq!(core::mem::align_of::<EndpointDescriptor>(), 16);
        assert_eq!(core::mem::size_of::<TransferDescriptor>(), 16);
        assert_eq!(core::mem::align_of::<TransferDescriptor>(), 16);
        assert_eq!(core::mem::size_of::<Hcca>(), 256);
        assert_eq!(core::mem::align_of::<Hcca>(), 256);
        assert_eq!(HCCA_INTERRUPT_ENTRIES, 32);
    }

    #[test]
    fn ohci_td_encodes_in_pid_and_data1_toggle() {
        let control = td_control(TD_DP_IN, TD_T_DATA1, true);
        assert_eq!(control & (3 << 19), TD_DP_IN);
        assert_eq!(control & (3 << 24), TD_T_DATA1);
    }

    #[test]
    fn ohci_td_buffer_rejects_crossing_more_than_two_pages() {
        let mut td = TransferDescriptor::default();
        assert!(set_td_buffer(&mut td, 0x1ff0, 32).is_ok());
        assert!(set_td_buffer(&mut td, 0x1ff0, 8192).is_err());
    }
}
