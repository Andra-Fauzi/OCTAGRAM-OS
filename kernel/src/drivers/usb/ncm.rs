//! USB CDC-NCM transport for Android tethering devices.
//!
//! Android 14 and newer — including Android 16, and every Pixel since the
//! Pixel 6 — expose USB tethering as CDC-NCM instead of RNDIS. Without this
//! driver such a phone stays unbound, the phone`s NCM function never sees the
//! host configure its data interface, and the tethering switch in Android
//! Settings fails to stay on.
//!
//! The host side of NCM is small: select alternate setting 1 on the data
//! interface, read the NTB parameters with a class request, then exchange
//! Ethernet frames wrapped in 16-bit NTB containers (`NTH16` header plus a
//! `NDP16` datagram pointer table). This module implements exactly that, and
//! registers the result as a [`LinkDevice`] so the IPv4 stack above it does not
//! care whether the link is NCM or RNDIS.

use super::bulk_in::{BULK_IN_CODE, BULK_IN_DCI, BULK_IN_LENGTH, BULK_IN_PENDING, BULK_IN_SLOT_ID};
use super::xhci::{self, EndpointHandle};
use crate::drivers::device_model::UsbInterfaceInfo;
use crate::memory;
use crate::network::{self, LinkDevice};
use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;
use spin::Mutex;

/// `NTH16.dwSignature` — "NCMH" in transfer order.
const NTH16_SIGNATURE: u32 = 0x484D_434E;
/// `NDP16.dwSignature` — "NCM0" in transfer order.
const NDP16_SIGNATURE: u32 = 0x304D_434E;
const NTH16_HEADER_LENGTH: u16 = 12;
const NDP16_HEADER_LENGTH: usize = 8;
const NDP16_ENTRY_LENGTH: usize = 4;
/// NDP16 with one datagram plus the mandatory zeroed terminating entry.
const NDP16_SINGLE_LENGTH: usize = NDP16_HEADER_LENGTH + 2 * NDP16_ENTRY_LENGTH;
const MAX_TRANSFER_SIZE: usize = 16 * 1024;
const MAX_FRAME_SIZE: usize = 1514;
const MAX_DATAGRAMS_PER_NTB: usize = 16;

const CDC_GET_NTB_PARAMETERS: u8 = 0x80;
const CDC_SET_ETHERNET_PACKET_FILTER: u8 = 0x43;
const USB_SET_INTERFACE: u8 = 0x0B;
const NTB_PARAMETERS_LENGTH: usize = 28;
const PACKET_TYPE_ALL_MULTICAST: u16 = 0x0002;
const PACKET_TYPE_DIRECTED: u16 = 0x0004;
const PACKET_TYPE_BROADCAST: u16 = 0x0008;

/// Local address the host uses on the tethering link: a fixed,
/// locally-administered unicast address. NCM has no MAC query, the phone side
/// keeps its own address, and this one only has to be stable across reboots.
const LOCAL_MAC: [u8; 6] = [0x02, 0x1a, 0x0c, 0x4e, 0x43, 0x4d];

#[repr(C, align(4096))]
struct ControlBuffer([u8; 64]);

#[repr(C, align(4096))]
struct DmaBuffer([u8; MAX_TRANSFER_SIZE]);

struct AdapterState {
    bulk_in: EndpointHandle,
    bulk_out: EndpointHandle,
    rx: Box<DmaBuffer>,
    tx: Box<DmaBuffer>,
    sequence: u16,
    active: bool,
}

struct NcmAdapter {
    slot_id: u32,
    interface_number: u8,
    mac: [u8; 6],
    state: Mutex<AdapterState>,
}

static ACTIVE_ADAPTER: Mutex<Option<Arc<NcmAdapter>>> = Mutex::new(None);

impl LinkDevice for NcmAdapter {
    fn slot_id(&self) -> u32 {
        self.slot_id
    }

    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn transmit(&self, frame: &[u8]) -> Result<(), &'static str> {
        let mut state = self.state.lock();
        if !state.active {
            return Err("NCM adapter is offline");
        }
        let sequence = state.sequence;
        let block_length = build_ntb(frame, &mut state.tx.0, sequence)?;
        state.sequence = sequence.wrapping_add(1);

        let physical = physical_address(state.tx.0.as_ptr() as usize)?;
        let sent = unsafe {
            xhci::bulk_transfer(
                self.slot_id,
                &mut state.bulk_out,
                physical,
                block_length as u32,
            )
        }?;
        if sent != block_length as u32 {
            return Err("short NCM bulk OUT transfer");
        }
        Ok(())
    }
}

pub fn matches(interface: &UsbInterfaceInfo) -> bool {
    interface.class == 0x02
        && interface.subclass == 0x0D
        && interface.protocol <= 0x01
        && find_bulk_endpoints(&interface.configuration_endpoints, interface.number).is_some()
}

pub unsafe fn probe(interface: &UsbInterfaceInfo) -> Result<(), &'static str> {
    let data_number = interface
        .number
        .checked_add(1)
        .ok_or("NCM data interface missing")?;
    let (bulk_in_info, bulk_out_info) =
        find_bulk_endpoints(&interface.configuration_endpoints, interface.number)
            .ok_or("NCM data endpoints missing")?;

    // Alternate setting 1 is the one that carries the bulk pipes: setting 0 has
    // no endpoints at all, and transfers stall until the host selects it.
    unsafe {
        xhci::control_transfer_no_data(
            interface.slot_id,
            0x01,
            USB_SET_INTERFACE,
            1,
            u16::from(data_number),
        )?;
    }

    let (bulk_in, bulk_out) = {
        let (first, second) = if endpoint_dci(bulk_in_info) < endpoint_dci(bulk_out_info) {
            (bulk_in_info, bulk_out_info)
        } else {
            (bulk_out_info, bulk_in_info)
        };
        let mut handles = unsafe {
            xhci::configure_endpoints(interface.slot_id, interface.root_port, &[first, second])?
        };
        let second_handle = handles.pop().ok_or("NCM endpoint configuration incomplete")?;
        let first_handle = handles.pop().ok_or("NCM endpoint configuration incomplete")?;
        if endpoint_dci(first) == endpoint_dci(bulk_in_info) {
            (first_handle, second_handle)
        } else {
            (second_handle, first_handle)
        }
    };

    // Mandatory in the NCM spec, but a firmware that answers with a short or
    // malformed message must not cost us the link: the limits are only used for
    // diagnostics, transfers always stay inside our own buffers.
    match unsafe { read_ntb_parameters(interface.slot_id, interface.number) } {
        Ok(parameters) => crate::println!(
            "NCM: NTB masuk maks {} byte, keluar maks {} byte, alignment {}",
            parameters.in_max, parameters.out_max, parameters.out_alignment
        ),
        Err(error) => crate::println!("NCM: parameter NTB tidak terbaca ({error}), memakai default"),
    }

    unsafe {
        xhci::control_transfer_no_data(
            interface.slot_id,
            0x21,
            CDC_SET_ETHERNET_PACKET_FILTER,
            PACKET_TYPE_DIRECTED | PACKET_TYPE_BROADCAST | PACKET_TYPE_ALL_MULTICAST,
            u16::from(interface.number),
        )?;
    }

    let rx = Box::new(DmaBuffer([0; MAX_TRANSFER_SIZE]));
    let tx = Box::new(DmaBuffer([0; MAX_TRANSFER_SIZE]));
    let rx_phys = physical_address(rx.0.as_ptr() as usize)?;
    let adapter = Arc::new(NcmAdapter {
        slot_id: interface.slot_id,
        interface_number: interface.number,
        mac: LOCAL_MAC,
        state: Mutex::new(AdapterState {
            bulk_in,
            bulk_out,
            rx,
            tx,
            sequence: 0,
            active: true,
        }),
    });

    network::register_link(adapter.clone())?;
    {
        let mut active = ACTIVE_ADAPTER.lock();
        if active.is_some() {
            network::unregister_link(interface.slot_id);
            return Err("only one NCM adapter is currently supported");
        }
        *active = Some(adapter.clone());
    }

    BULK_IN_DCI.store(adapter.state.lock().bulk_in.dci, Ordering::SeqCst);
    BULK_IN_SLOT_ID.store(interface.slot_id, Ordering::SeqCst);
    BULK_IN_PENDING.store(false, Ordering::SeqCst);
    let arm_result = {
        let mut state = adapter.state.lock();
        unsafe {
            xhci::arm_bulk_transfer(
                adapter.slot_id,
                &mut state.bulk_in,
                rx_phys,
                MAX_TRANSFER_SIZE as u32,
            )
        }
    };
    if let Err(error) = arm_result {
        shutdown(interface.slot_id);
        return Err(error);
    }

    crate::println!(
        "Android NCM siap: MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        LOCAL_MAC[0],
        LOCAL_MAC[1],
        LOCAL_MAC[2],
        LOCAL_MAC[3],
        LOCAL_MAC[4],
        LOCAL_MAC[5]
    );
    crate::task::spawn(async {
        match network::start_dhcp() {
            Ok(()) => crate::println!("NCM: DHCP discover sent (`net status` untuk melihat lease)"),
            Err(error) => crate::println!("NCM: DHCP tidak dimulai: {error} (jalankan `net dhcp`)"),
        }
    });
    Ok(())
}

pub unsafe fn poll() {
    if !BULK_IN_PENDING.swap(false, Ordering::SeqCst) {
        return;
    }

    let Some(adapter) = ACTIVE_ADAPTER.lock().clone() else {
        return;
    };
    let completion_code = BULK_IN_CODE.load(Ordering::SeqCst);
    let residual = BULK_IN_LENGTH.load(Ordering::SeqCst) as usize;
    if completion_code != 1 && completion_code != 13 {
        crate::println!("NCM bulk IN completion failed: code={completion_code}");
        shutdown(adapter.slot_id);
        return;
    }

    let frames: Vec<Vec<u8>> = {
        let state = adapter.state.lock();
        let actual_length = MAX_TRANSFER_SIZE.saturating_sub(residual);
        if !state.active || actual_length == 0 {
            Vec::new()
        } else {
            let mut buf = alloc::vec![0u8; actual_length];
            unsafe {
                xhci::copy_dma_to_slice(state.rx.0.as_ptr(), &mut buf);
            }
            datagrams(&buf)
                .take(MAX_DATAGRAMS_PER_NTB)
                .map(Vec::from)
                .collect()
        }
    };
    for frame in frames {
        network::receive_ethernet(&frame);
    }

    let mut state = adapter.state.lock();
    if state.active {
        match physical_address(state.rx.0.as_ptr() as usize) {
            Ok(rx_phys) => {
                if let Err(error) = unsafe {
                    xhci::arm_bulk_transfer(
                        adapter.slot_id,
                        &mut state.bulk_in,
                        rx_phys,
                        MAX_TRANSFER_SIZE as u32,
                    )
                } {
                    crate::println!("NCM receive re-arm failed: {error}");
                }
            }
            Err(error) => crate::println!("NCM receive buffer mapping failed: {error}"),
        }
    }
}

pub fn shutdown(slot_id: u32) {
    let adapter = {
        let mut active = ACTIVE_ADAPTER.lock();
        if active
            .as_ref()
            .is_some_and(|adapter| adapter.slot_id == slot_id)
        {
            active.take()
        } else {
            None
        }
    };
    let Some(adapter) = adapter else {
        return;
    };

    adapter.state.lock().active = false;
    super::bulk_in::disarm();
    network::unregister_link(slot_id);
    crate::println!(
        "NCM interface {} stopped (slot={slot_id})",
        adapter.interface_number
    );
}

#[derive(Clone, Copy)]
struct NtbParameters {
    in_max: u32,
    out_max: u32,
    out_alignment: u16,
}

unsafe fn read_ntb_parameters(
    slot_id: u32,
    interface_number: u8,
) -> Result<NtbParameters, &'static str> {
    let mut response = Box::new(ControlBuffer([0u8; 64]));
    let response_phys = physical_address(response.0.as_ptr() as usize)?;
    let length = unsafe {
        xhci::control_transfer_in(
            slot_id,
            0xA1,
            CDC_GET_NTB_PARAMETERS,
            0,
            u16::from(interface_number),
            response_phys,
            NTB_PARAMETERS_LENGTH as u16,
        )
    }? as usize;
    if length < NTB_PARAMETERS_LENGTH {
        return Err("short NCM NTB parameters response");
    }
    let mut result = [0u8; 64];
    unsafe {
        xhci::copy_dma_to_slice(response.0.as_ptr(), &mut result[..length]);
    }
    let in_max = read_u32(&result, 4)?;
    let out_max = read_u32(&result, 16)?;
    let out_alignment = read_u16(&result, 24)?;
    if out_max < 64 || in_max < 64 {
        return Err("NCM adapter reports an unusable NTB size");
    }
    Ok(NtbParameters {
        in_max: in_max.min(MAX_TRANSFER_SIZE as u32),
        out_max: out_max.min(MAX_TRANSFER_SIZE as u32),
        out_alignment: if out_alignment.is_power_of_two() {
            out_alignment
        } else {
            4
        },
    })
}

/// Wrap one Ethernet frame in an NTB that the device can hand straight to its
/// network stack. Layout: `NTH16` header, `NDP16` table with a single datagram
/// and a zeroed terminator, then the frame itself on a 4-byte boundary.
fn build_ntb(frame: &[u8], buffer: &mut [u8], sequence: u16) -> Result<usize, &'static str> {
    if frame.is_empty() || frame.len() > MAX_FRAME_SIZE {
        return Err("invalid Ethernet frame length");
    }
    let payload_offset = NTH16_HEADER_LENGTH as usize + NDP16_SINGLE_LENGTH;
    let unpadded = payload_offset + frame.len();
    let block_length = (unpadded + 3) & !3;
    let block = buffer
        .get_mut(..block_length)
        .ok_or("NCM transmit buffer is too small")?;
    block.fill(0);

    write_u32(block, 0, NTH16_SIGNATURE)?;
    write_u16(block, 4, NTH16_HEADER_LENGTH)?;
    write_u16(block, 6, sequence)?;
    write_u16(block, 8, block_length as u16)?;
    write_u16(block, 10, NTH16_HEADER_LENGTH)?;

    write_u32(block, 12, NDP16_SIGNATURE)?;
    write_u16(block, 16, NDP16_SINGLE_LENGTH as u16)?;
    write_u16(block, 18, 0)?;
    write_u16(block, 20, payload_offset as u16)?;
    write_u16(block, 22, frame.len() as u16)?;

    block[payload_offset..unpadded].copy_from_slice(frame);
    Ok(block_length)
}

/// Ethernet frames carried by one received NTB.
///
/// A device may pack several frames into a single transfer, and some firmware
/// sends zero-length transfers as keep-alives; both cases simply yield zero
/// datagrams here.
fn datagrams(ntb: &[u8]) -> Datagrams<'_> {
    let empty = Datagrams {
        ntb,
        block_length: 0,
        offset: 0,
        done: true,
    };
    if ntb.len() < NTH16_HEADER_LENGTH as usize {
        return empty;
    }
    let Ok(signature) = read_u32(ntb, 0) else {
        return empty;
    };
    if signature != NTH16_SIGNATURE {
        return empty;
    }
    let block_length = read_u16(ntb, 8).unwrap_or(0) as usize;
    let ndp_offset = read_u16(ntb, 10).unwrap_or(0) as usize;
    if block_length < NTH16_HEADER_LENGTH as usize
        || block_length > ntb.len()
        || ndp_offset + NDP16_HEADER_LENGTH > block_length
        || read_u32(ntb, ndp_offset).unwrap_or(0) != NDP16_SIGNATURE
    {
        return empty;
    }
    Datagrams {
        ntb,
        block_length,
        offset: ndp_offset + NDP16_HEADER_LENGTH,
        done: false,
    }
}

struct Datagrams<'a> {
    ntb: &'a [u8],
    block_length: usize,
    offset: usize,
    done: bool,
}

impl<'a> Iterator for Datagrams<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        while !self.done {
            if self.offset + NDP16_ENTRY_LENGTH > self.block_length {
                self.done = true;
                return None;
            }
            let start = read_u16(self.ntb, self.offset).unwrap_or(0) as usize;
            let length = read_u16(self.ntb, self.offset + 2).unwrap_or(0) as usize;
            self.offset += NDP16_ENTRY_LENGTH;
            // A zeroed index or length entry terminates the table.
            if start == 0 || length == 0 {
                self.done = true;
                return None;
            }
            let end = match start.checked_add(length) {
                Some(end) if end <= self.block_length => end,
                _ => {
                    self.done = true;
                    return None;
                }
            };
            if length < 14 || length > MAX_FRAME_SIZE {
                continue;
            }
            return Some(&self.ntb[start..end]);
        }
        None
    }
}

fn find_bulk_endpoints(
    endpoints: &[xhci::EndpointInfo],
    control_interface: u8,
) -> Option<(xhci::EndpointInfo, xhci::EndpointInfo)> {
    let data_interface = control_interface.checked_add(1)?;
    let mut bulk_in = None;
    let mut bulk_out = None;
    for endpoint in endpoints.iter().copied().filter(|endpoint| {
        (endpoint.inteface_number == data_interface || endpoint.inteface_number == control_interface)
            && endpoint.attributes & 0x3 == 0x2
    }) {
        if endpoint.address & 0x80 != 0 {
            bulk_in.get_or_insert(endpoint);
        } else {
            bulk_out.get_or_insert(endpoint);
        }
    }
    Some((bulk_in?, bulk_out?))
}

fn endpoint_dci(endpoint: xhci::EndpointInfo) -> u32 {
    let endpoint_number = (endpoint.address & 0x0F) as u32;
    endpoint_number * 2 + u32::from(endpoint.address & 0x80 != 0)
}

fn physical_address(virtual_address: usize) -> Result<u64, &'static str> {
    memory::virtual_to_physical(virtual_address)
        .map(|address| address as u64)
        .ok_or("NCM DMA buffer mapping failed")
}

fn write_u16(buffer: &mut [u8], offset: usize, value: u16) -> Result<(), &'static str> {
    let target = buffer
        .get_mut(offset..offset + 2)
        .ok_or("NCM message field out of bounds")?;
    target.copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn write_u32(buffer: &mut [u8], offset: usize, value: u32) -> Result<(), &'static str> {
    let target = buffer
        .get_mut(offset..offset + 4)
        .ok_or("NCM message field out of bounds")?;
    target.copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn read_u16(buffer: &[u8], offset: usize) -> Result<u16, &'static str> {
    let value = buffer
        .get(offset..offset + 2)
        .ok_or("NCM message field out of bounds")?;
    Ok(u16::from_le_bytes(value.try_into().unwrap()))
}

fn read_u32(buffer: &[u8], offset: usize) -> Result<u32, &'static str> {
    let value = buffer
        .get(offset..offset + 4)
        .ok_or("NCM message field out of bounds")?;
    Ok(u32::from_le_bytes(value.try_into().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::{build_ntb, datagrams, find_bulk_endpoints, matches, read_u16, read_u32};
    use crate::drivers::device_model::UsbInterfaceInfo;
    use crate::drivers::usb::{EndpointInfo, UsbControllerId};

    fn endpoint(address: u8, interface: u8, class: u8) -> EndpointInfo {
        EndpointInfo {
            address,
            attributes: 2,
            max_packet_size: 512,
            interval: 0,
            inteface_number: interface,
            interface_subclass: 0,
            interface_class: class,
            interface_protocol: 0,
        }
    }

    #[test]
    fn recognizes_ncm_control_interface_with_data_bulk_pair() {
        let configuration_endpoints = alloc::vec![
            endpoint(0x81, 0, 0x02),
            endpoint(0x82, 1, 0x0A),
            endpoint(0x02, 1, 0x0A),
        ];
        let interface = UsbInterfaceInfo {
            controller: UsbControllerId::XHCI,
            slot_id: 1,
            root_port: 1,
            number: 0,
            class: 0x02,
            subclass: 0x0D,
            protocol: 0,
            endpoints: alloc::vec![configuration_endpoints[0]],
            configuration_endpoints,
        };
        assert!(matches(&interface));
    }

    #[test]
    fn ignores_rndis_and_mbim_control_interfaces() {
        let make = |class: u8, subclass: u8, protocol: u8| UsbInterfaceInfo {
            controller: UsbControllerId::XHCI,
            slot_id: 1,
            root_port: 1,
            number: 0,
            class,
            subclass,
            protocol,
            endpoints: alloc::vec![endpoint(0x81, 0, class)],
            configuration_endpoints: alloc::vec![
                endpoint(0x81, 0, class),
                endpoint(0x82, 1, 0x0A),
                endpoint(0x02, 1, 0x0A),
            ],
        };
        assert!(!matches(&make(0xE0, 0x01, 0x03)));
        assert!(!matches(&make(0x02, 0x0E, 0x00)));
    }

    #[test]
    fn wraps_a_frame_in_a_parsable_ntb() {
        let frame = [0x5a; 60];
        let mut buffer = [0u8; 2048];
        let length = build_ntb(&frame, &mut buffer, 7).unwrap();
        assert_eq!(length, 88);
        assert_eq!(read_u32(&buffer, 0).unwrap(), super::NTH16_SIGNATURE);
        assert_eq!(read_u16(&buffer, 6).unwrap(), 7);
        assert_eq!(read_u16(&buffer, 8).unwrap() as usize, length);
        let frames: alloc::vec::Vec<_> = datagrams(&buffer[..length]).collect();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0], &frame);
        assert!(build_ntb(&frame, &mut buffer[..40], 8).is_err());
    }

    #[test]
    fn parses_multiple_datagrams_and_stops_at_the_terminator() {
        let mut block = [0u8; 96];
        block[0..4].copy_from_slice(&super::NTH16_SIGNATURE.to_le_bytes());
        block[4..6].copy_from_slice(&12u16.to_le_bytes());
        block[8..10].copy_from_slice(&96u16.to_le_bytes());
        block[10..12].copy_from_slice(&12u16.to_le_bytes());
        block[12..16].copy_from_slice(&super::NDP16_SIGNATURE.to_le_bytes());
        block[16..18].copy_from_slice(&24u16.to_le_bytes());
        block[20..22].copy_from_slice(&32u16.to_le_bytes());
        block[22..24].copy_from_slice(&14u16.to_le_bytes());
        block[24..26].copy_from_slice(&48u16.to_le_bytes());
        block[26..28].copy_from_slice(&30u16.to_le_bytes());
        let mut frames = datagrams(&block);
        assert_eq!(frames.next().map(<[u8]>::len), Some(14));
        assert_eq!(frames.next().map(<[u8]>::len), Some(30));
        assert!(frames.next().is_none());
    }

    #[test]
    fn rejects_blocks_with_a_foreign_signature() {
        let mut block = [0u8; 32];
        block[0..4].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        assert_eq!(datagrams(&block).count(), 0);
        assert!(find_bulk_endpoints(&[endpoint(0x82, 1, 0x0A)], 0).is_none());
    }
}
