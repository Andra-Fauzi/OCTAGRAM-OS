//! USB RNDIS transport for Android tethering devices.

use super::xhci::{self, EndpointHandle};
use crate::drivers::device_model::UsbInterfaceInfo;
use crate::memory;
use crate::network::{self, LinkDevice};
use alloc::boxed::Box;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

const RNDIS_PACKET_MESSAGE: u32 = 1;
const RNDIS_INITIALIZE_MESSAGE: u32 = 2;
const RNDIS_QUERY_MESSAGE: u32 = 4;
const RNDIS_SET_MESSAGE: u32 = 5;
const RNDIS_INITIALIZE_COMPLETE: u32 = 0x8000_0002;
const RNDIS_QUERY_COMPLETE: u32 = 0x8000_0004;
const RNDIS_SET_COMPLETE: u32 = 0x8000_0005;
const OID_GEN_CURRENT_PACKET_FILTER: u32 = 0x0001_010E;
const OID_802_3_CURRENT_ADDRESS: u32 = 0x0101_0102;
const PACKET_FILTER_DIRECTED: u32 = 0x0000_0001;
const PACKET_FILTER_BROADCAST: u32 = 0x0000_0008;
const MAX_TRANSFER_SIZE: usize = 16 * 1024;
const MAX_FRAME_SIZE: usize = 1514;
const RNDIS_HEADER_SIZE: usize = 44;
const CONTROL_BUFFER_SIZE: usize = 512;
/// Percobaan membaca `GET_ENCAPSULATED_RESPONSE`, 5 ms sekali (total ~1 detik).
///
/// HP Android baru menyiapkan balasan beberapa milidetik setelah perintahnya
/// diterima; retry tanpa jeda selalu mengembalikan response kosong di hardware
/// nyata dan seluruh bring-up RNDIS gagal dengan "short RNDIS control response".
const CONTROL_RESPONSE_ATTEMPTS: usize = 200;
const CONTROL_RESPONSE_DELAY_MS: u32 = 5;

pub static RNDIS_DCI: AtomicU32 = AtomicU32::new(0);
pub static RNDIS_SLOT_ID: AtomicU32 = AtomicU32::new(0);
pub static RNDIS_RX_PENDING: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);
pub static RNDIS_RX_LENGTH: AtomicU32 = AtomicU32::new(0);
pub static RNDIS_RX_CODE: AtomicU32 = AtomicU32::new(0);

#[repr(C, align(4096))]
struct DmaBuffer([u8; MAX_TRANSFER_SIZE]);

struct AdapterState {
    bulk_in: EndpointHandle,
    bulk_out: EndpointHandle,
    rx: Box<DmaBuffer>,
    tx: Box<DmaBuffer>,
    active: bool,
}

struct RndisAdapter {
    slot_id: u32,
    interface_number: u8,
    mac: [u8; 6],
    state: Mutex<AdapterState>,
}

static ACTIVE_ADAPTER: Mutex<Option<Arc<RndisAdapter>>> = Mutex::new(None);

impl LinkDevice for RndisAdapter {
    fn slot_id(&self) -> u32 {
        self.slot_id
    }

    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn transmit(&self, frame: &[u8]) -> Result<(), &'static str> {
        if frame.is_empty() || frame.len() > MAX_FRAME_SIZE {
            return Err("invalid Ethernet frame length");
        }

        let mut state = self.state.lock();
        if !state.active {
            return Err("RNDIS adapter is offline");
        }

        let message_length = build_packet_message(frame, &mut state.tx.0)?;

        let physical = physical_address(state.tx.0.as_ptr() as usize)?;
        let sent = unsafe {
            xhci::bulk_transfer(
                self.slot_id,
                &mut state.bulk_out,
                physical,
                message_length as u32,
            )
        }?;
        if sent != message_length as u32 {
            return Err("short RNDIS bulk OUT transfer");
        }
        Ok(())
    }
}

pub fn matches(interface: &UsbInterfaceInfo) -> bool {
    interface.class == 0xE0
        && interface.subclass == 0x01
        && interface.protocol == 0x03
        && find_bulk_endpoints(&interface.configuration_endpoints, interface.number).is_some()
}

pub unsafe fn probe(interface: &UsbInterfaceInfo) -> Result<(), &'static str> {
    let (bulk_in_info, bulk_out_info) =
        find_bulk_endpoints(&interface.configuration_endpoints, interface.number)
            .ok_or("RNDIS data endpoints missing")?;

    let (first, second) = if endpoint_dci(bulk_in_info) < endpoint_dci(bulk_out_info) {
        (bulk_in_info, bulk_out_info)
    } else {
        (bulk_out_info, bulk_in_info)
    };
    let first_handle =
        unsafe { xhci::configure_endpoint(interface.slot_id, interface.root_port, first)? };
    let second_handle =
        unsafe { xhci::configure_endpoint(interface.slot_id, interface.root_port, second)? };
    let (bulk_in, bulk_out) = if endpoint_dci(first) == endpoint_dci(bulk_in_info) {
        (first_handle, second_handle)
    } else {
        (second_handle, first_handle)
    };

    let mac = unsafe { initialize_rndis(interface.slot_id, interface.number)? };
    let rx = Box::new(DmaBuffer([0; MAX_TRANSFER_SIZE]));
    let tx = Box::new(DmaBuffer([0; MAX_TRANSFER_SIZE]));
    let rx_phys = physical_address(rx.0.as_ptr() as usize)?;
    let adapter = Arc::new(RndisAdapter {
        slot_id: interface.slot_id,
        interface_number: interface.number,
        mac,
        state: Mutex::new(AdapterState {
            bulk_in,
            bulk_out,
            rx,
            tx,
            active: true,
        }),
    });

    network::register_link(adapter.clone())?;
    {
        let mut active = ACTIVE_ADAPTER.lock();
        if active.is_some() {
            network::unregister_link(interface.slot_id);
            return Err("only one RNDIS adapter is currently supported");
        }
        *active = Some(adapter.clone());
    }

    RNDIS_DCI.store(bulk_in.dci, Ordering::SeqCst);
    RNDIS_SLOT_ID.store(interface.slot_id, Ordering::SeqCst);
    RNDIS_RX_PENDING.store(false, Ordering::SeqCst);
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
        "Android RNDIS siap: MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0],
        mac[1],
        mac[2],
        mac[3],
        mac[4],
        mac[5]
    );
    // Tethering langsung minta alamat: tanpa langkah ini link RNDIS terdaftar
    // tapi tidak punya IPv4 (DHCP baru jalan kalau `net dhcp` diketik manual).
    match network::start_dhcp() {
        Ok(()) => crate::println!("RNDIS: DHCP discover sent (`net status` untuk melihat lease)"),
        Err(error) => crate::println!("RNDIS: DHCP tidak dimulai: {error} (jalankan `net dhcp`)"),
    }
    Ok(())
}

pub unsafe fn poll() {
    if !RNDIS_RX_PENDING.swap(false, Ordering::SeqCst) {
        return;
    }

    let Some(adapter) = ACTIVE_ADAPTER.lock().clone() else {
        return;
    };
    let completion_code = RNDIS_RX_CODE.load(Ordering::SeqCst);
    let residual = RNDIS_RX_LENGTH.load(Ordering::SeqCst) as usize;
    if completion_code != 1 && completion_code != 13 {
        crate::println!("RNDIS bulk IN completion failed: code={completion_code}");
        shutdown(adapter.slot_id);
        return;
    }
    let frame = {
        let state = adapter.state.lock();
        let actual_length = MAX_TRANSFER_SIZE.saturating_sub(residual);
        if !state.active {
            None
        } else {
            extract_packet(&state.rx.0[..actual_length]).map(alloc::vec::Vec::from)
        }
    };

    if let Some(frame) = frame {
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
                    crate::println!("RNDIS receive re-arm failed: {error}");
                }
            }
            Err(error) => crate::println!("RNDIS receive buffer mapping failed: {error}"),
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
    RNDIS_DCI.store(0, Ordering::SeqCst);
    RNDIS_SLOT_ID.store(0, Ordering::SeqCst);
    RNDIS_RX_PENDING.store(false, Ordering::SeqCst);
    network::unregister_link(slot_id);
    crate::println!(
        "RNDIS interface {} stopped (slot={slot_id})",
        adapter.interface_number
    );
}

unsafe fn initialize_rndis(slot_id: u32, interface_number: u8) -> Result<[u8; 6], &'static str> {
    let mut initialize = [0u8; 24];
    let initialize_length = initialize.len() as u32;
    write_u32(&mut initialize, 0, RNDIS_INITIALIZE_MESSAGE)?;
    write_u32(&mut initialize, 4, initialize_length)?;
    write_u32(&mut initialize, 8, 1)?;
    write_u32(&mut initialize, 12, 1)?;
    write_u32(&mut initialize, 16, 0)?;
    write_u32(&mut initialize, 20, MAX_TRANSFER_SIZE as u32)?;
    let response = unsafe { send_command(slot_id, interface_number, &initialize)? };
    if read_u32(response.as_ref(), 0)? != RNDIS_INITIALIZE_COMPLETE
        || read_u32(response.as_ref(), 12)? != 0
        || read_u32(response.as_ref(), 4)? < 40
    {
        return Err("RNDIS initialize response rejected");
    }
    if (read_u32(response.as_ref(), 36)? as usize) < RNDIS_HEADER_SIZE + MAX_FRAME_SIZE {
        return Err("RNDIS adapter reports an undersized transfer limit");
    }

    let mac = unsafe { query_mac_address(slot_id, interface_number)? };
    let mut set_filter = [0u8; 32];
    let set_filter_length = set_filter.len() as u32;
    write_u32(&mut set_filter, 0, RNDIS_SET_MESSAGE)?;
    write_u32(&mut set_filter, 4, set_filter_length)?;
    write_u32(&mut set_filter, 8, 2)?;
    write_u32(&mut set_filter, 12, OID_GEN_CURRENT_PACKET_FILTER)?;
    write_u32(&mut set_filter, 16, 4)?;
    write_u32(&mut set_filter, 20, 20)?;
    write_u32(
        &mut set_filter,
        28,
        PACKET_FILTER_DIRECTED | PACKET_FILTER_BROADCAST,
    )?;
    let response = unsafe { send_command(slot_id, interface_number, &set_filter)? };
    if read_u32(response.as_ref(), 0)? != RNDIS_SET_COMPLETE
        || read_u32(response.as_ref(), 12)? != 0
    {
        return Err("RNDIS packet-filter request rejected");
    }
    Ok(mac)
}

unsafe fn query_mac_address(slot_id: u32, interface_number: u8) -> Result<[u8; 6], &'static str> {
    let mut query = [0u8; 28];
    let query_length = query.len() as u32;
    write_u32(&mut query, 0, RNDIS_QUERY_MESSAGE)?;
    write_u32(&mut query, 4, query_length)?;
    write_u32(&mut query, 8, 3)?;
    write_u32(&mut query, 12, OID_802_3_CURRENT_ADDRESS)?;
    write_u32(&mut query, 20, 20)?;
    let response = unsafe { send_command(slot_id, interface_number, &query)? };
    if read_u32(response.as_ref(), 0)? != RNDIS_QUERY_COMPLETE
        || read_u32(response.as_ref(), 12)? != 0
    {
        return Err("RNDIS MAC address query failed");
    }
    let info_length = read_u32(response.as_ref(), 16)? as usize;
    let info_offset = read_u32(response.as_ref(), 20)? as usize;
    let start = 8usize
        .checked_add(info_offset)
        .ok_or("invalid RNDIS query offset")?;
    let end = start
        .checked_add(info_length)
        .ok_or("invalid RNDIS query length")?;
    if info_length < 6 || end > read_u32(response.as_ref(), 4)? as usize || end > response.len() {
        return Err("invalid RNDIS MAC address response");
    }
    let mut mac = [0u8; 6];
    mac.copy_from_slice(&response[start..start + 6]);
    if mac.iter().all(|byte| *byte == 0) || mac[0] & 1 != 0 {
        return Err("invalid RNDIS MAC address");
    }
    Ok(mac)
}

unsafe fn send_command(
    slot_id: u32,
    interface_number: u8,
    message: &[u8],
) -> Result<Box<[u8; CONTROL_BUFFER_SIZE]>, &'static str> {
    if message.is_empty() || message.len() > CONTROL_BUFFER_SIZE {
        return Err("invalid RNDIS control message length");
    }
    let request_id = read_u32(message, 8)?;
    let mut command = Box::new([0u8; CONTROL_BUFFER_SIZE]);
    command[..message.len()].copy_from_slice(message);
    unsafe {
        xhci::control_transfer_out(
            slot_id,
            0x21,
            0x00,
            0,
            interface_number as u16,
            &command[..message.len()],
        )?;
    }

    let mut response = Box::new([0u8; CONTROL_BUFFER_SIZE]);
    let response_phys = physical_address(response.as_ptr() as usize)?;
    let mut length = 0usize;
    for attempt in 0..CONTROL_RESPONSE_ATTEMPTS {
        response.fill(0);
        length = unsafe {
            xhci::control_transfer_in(
                slot_id,
                0xA1,
                0x01,
                0,
                interface_number as u16,
                response_phys,
                CONTROL_BUFFER_SIZE as u16,
            )?
        } as usize;
        // A response is only usable once its own header is complete: the device
        // may return a stub while it is still preparing the real message.
        let header_length = read_u32(response.as_ref(), 4).unwrap_or(0) as usize;
        if length >= 8 && header_length >= 8 && header_length <= length {
            break;
        }
        if attempt + 1 < CONTROL_RESPONSE_ATTEMPTS {
            crate::arch::x86_64::delay_milliseconds(CONTROL_RESPONSE_DELAY_MS);
        }
    }
    if length < 8 || length > response.len() {
        return Err("short RNDIS control response");
    }
    let message_length = read_u32(response.as_ref(), 4)? as usize;
    if message_length < 8 || message_length > length {
        return Err("invalid RNDIS control response length");
    }
    if read_u32(response.as_ref(), 8)? != request_id {
        return Err("RNDIS control response request ID mismatch");
    }
    response[message_length..].fill(0);
    Ok(response)
}

fn find_bulk_endpoints(
    endpoints: &[xhci::EndpointInfo],
    control_interface: u8,
) -> Option<(xhci::EndpointInfo, xhci::EndpointInfo)> {
    let data_interface = control_interface.checked_add(1)?;
    let mut bulk_in = None;
    let mut bulk_out = None;
    for endpoint in endpoints.iter().copied().filter(|endpoint| {
        endpoint.interface_class == 0x0A
            && endpoint.inteface_number == data_interface
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

fn extract_packet(message: &[u8]) -> Option<&[u8]> {
    if message.len() < RNDIS_HEADER_SIZE || read_u32(message, 0).ok()? != RNDIS_PACKET_MESSAGE {
        return None;
    }
    let message_length = read_u32(message, 4).ok()? as usize;
    let data_offset = read_u32(message, 8).ok()? as usize;
    let data_length = read_u32(message, 12).ok()? as usize;
    let data_start = 8usize.checked_add(data_offset)?;
    let data_end = data_start.checked_add(data_length)?;
    if message_length < RNDIS_HEADER_SIZE
        || message_length > message.len()
        || data_start < RNDIS_HEADER_SIZE
        || data_end > message_length
        || data_length < 14
        || data_length > MAX_FRAME_SIZE
    {
        return None;
    }
    Some(&message[data_start..data_end])
}

fn build_packet_message(frame: &[u8], buffer: &mut [u8]) -> Result<usize, &'static str> {
    if frame.is_empty() || frame.len() > MAX_FRAME_SIZE {
        return Err("invalid Ethernet frame length");
    }
    let message_length = RNDIS_HEADER_SIZE + frame.len();
    let message = buffer
        .get_mut(..message_length)
        .ok_or("RNDIS transmit buffer is too small")?;
    message.fill(0);
    write_u32(message, 0, RNDIS_PACKET_MESSAGE)?;
    write_u32(message, 4, message_length as u32)?;
    write_u32(message, 8, 36)?;
    write_u32(message, 12, frame.len() as u32)?;
    message[RNDIS_HEADER_SIZE..].copy_from_slice(frame);
    Ok(message_length)
}

fn physical_address(virtual_address: usize) -> Result<u64, &'static str> {
    memory::virtual_to_physical(virtual_address)
        .map(|address| address as u64)
        .ok_or("RNDIS DMA buffer mapping failed")
}

fn write_u32(buffer: &mut [u8], offset: usize, value: u32) -> Result<(), &'static str> {
    let target = buffer
        .get_mut(offset..offset + 4)
        .ok_or("RNDIS message field out of bounds")?;
    target.copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn read_u32(buffer: &[u8], offset: usize) -> Result<u32, &'static str> {
    let value = buffer
        .get(offset..offset + 4)
        .ok_or("RNDIS response field out of bounds")?;
    Ok(u32::from_le_bytes(value.try_into().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::{build_packet_message, extract_packet, find_bulk_endpoints, matches};
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
    fn recognizes_rndis_control_interface_with_data_bulk_pair() {
        let configuration_endpoints = alloc::vec![
            endpoint(0x81, 0, 0xE0),
            endpoint(0x82, 1, 0x0A),
            endpoint(0x02, 1, 0x0A),
        ];
        let interface = UsbInterfaceInfo {
            controller: UsbControllerId::XHCI,
            slot_id: 1,
            root_port: 1,
            number: 0,
            class: 0xE0,
            subclass: 1,
            protocol: 3,
            endpoints: alloc::vec![configuration_endpoints[0]],
            configuration_endpoints,
        };
        assert!(matches(&interface));
    }

    #[test]
    fn ignores_rndis_interfaces_without_both_bulk_directions() {
        let endpoints = [endpoint(0x82, 1, 0x0A)];
        assert!(find_bulk_endpoints(&endpoints, 0).is_none());
    }

    #[test]
    fn extracts_only_well_formed_rndis_packet_payloads() {
        let mut message = [0u8; 58];
        let message_len = message.len() as u32;
        message[0..4].copy_from_slice(&1u32.to_le_bytes());
        message[4..8].copy_from_slice(&message_len.to_le_bytes());
        message[8..12].copy_from_slice(&36u32.to_le_bytes());
        message[12..16].copy_from_slice(&14u32.to_le_bytes());
        assert_eq!(extract_packet(&message).unwrap().len(), 14);

        message[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(extract_packet(&message).is_none());
    }

    #[test]
    fn wraps_ethernet_frames_in_rndis_packet_messages() {
        let frame = [0x5a; 60];
        let mut message = [0u8; 104];
        let length = build_packet_message(&frame, &mut message).unwrap();
        assert_eq!(length, 104);
        assert_eq!(u32::from_le_bytes(message[0..4].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(message[8..12].try_into().unwrap()), 36);
        assert_eq!(&message[44..], &frame);
        assert!(build_packet_message(&frame, &mut message[..50]).is_err());
    }
}
