//! Minimal Ethernet, ARP, IPv4, UDP, TCP, DHCP, and ICMP support.

mod tcp;
pub use tcp::{
    TcpSocket, accept_socket as accept_tcp_socket, bind_socket as bind_tcp_socket,
    close_socket as close_tcp_socket, connect_socket as connect_tcp_socket,
    create_socket as create_tcp_socket, listen_socket as listen_tcp_socket,
    poll as poll_tcp,     is_last_descriptor as tcp_socket_is_last_descriptor,
    receive_socket as receive_tcp_socket, send_socket as send_tcp_socket,
    shutdown_socket as shutdown_tcp_socket, socket_name as tcp_socket_name,
};

use alloc::collections::VecDeque;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU16, Ordering};
use spin::Mutex;

const ETHERTYPE_IPV4: u16 = 0x0800;
const ETHERTYPE_ARP: u16 = 0x0806;
const IP_PROTOCOL_ICMP: u8 = 1;
const IP_PROTOCOL_TCP: u8 = 6;
const IP_PROTOCOL_UDP: u8 = 17;
const DHCP_CLIENT_PORT: u16 = 68;
const DHCP_SERVER_PORT: u16 = 67;
const IPV4_MTU: usize = 1500;
const DHCP_COOKIE: [u8; 4] = [99, 130, 83, 99];
const BROADCAST_MAC: [u8; 6] = [0xff; 6];
const BROADCAST_IPV4: [u8; 4] = [255; 4];
const MAX_UDP_QUEUE: usize = 8;
const EPHEMERAL_PORT_START: u16 = 49152;
const EPHEMERAL_PORT_COUNT: u16 = u16::MAX - EPHEMERAL_PORT_START + 1;

pub trait LinkDevice: Send + Sync {
    fn slot_id(&self) -> u32;
    fn mac_address(&self) -> [u8; 6];
    fn transmit(&self, frame: &[u8]) -> Result<(), &'static str>;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NetworkConfig {
    pub mac: [u8; 6],
    pub address: [u8; 4],
    pub netmask: [u8; 4],
    pub gateway: [u8; 4],
    pub dns: [u8; 4],
    pub configured: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DhcpState {
    Idle,
    Discovering,
    Requesting,
    Bound,
}

#[derive(Clone, Copy)]
struct PendingPing {
    target: [u8; 4],
    next_hop: [u8; 4],
    sequence: u16,
}

#[derive(Default)]
struct NetworkState {
    config: NetworkConfig,
    dhcp_state: Option<DhcpState>,
    xid: u32,
    neighbor_ip: [u8; 4],
    neighbor_mac: [u8; 6],
    pending_ping: Option<PendingPing>,
    next_sequence: u16,
}

pub struct UdpSocket {
    local_port: u16,
    received: VecDeque<ReceivedDatagram>,
}

struct ReceivedDatagram {
    source_ip: [u8; 4],
    source_port: u16,
    payload: Vec<u8>,
}

static LINK: Mutex<Option<Arc<dyn LinkDevice>>> = Mutex::new(None);
static UDP_SOCKETS: Mutex<Vec<Weak<Mutex<UdpSocket>>>> = Mutex::new(Vec::new());
static NEXT_EPHEMERAL_PORT: AtomicU16 = AtomicU16::new(EPHEMERAL_PORT_START);
static STATE: Mutex<NetworkState> = Mutex::new(NetworkState {
    config: NetworkConfig {
        mac: [0; 6],
        address: [0; 4],
        netmask: [0; 4],
        gateway: [0; 4],
        dns: [0; 4],
        configured: false,
    },
    dhcp_state: None,
    xid: 0,
    neighbor_ip: [0; 4],
    neighbor_mac: [0; 6],
    pending_ping: None,
    next_sequence: 1,
});

pub fn register_link(device: Arc<dyn LinkDevice>) -> Result<(), &'static str> {
    let mut link = LINK.lock();
    if link.is_some() {
        return Err("a network interface is already registered");
    }
    let mut state = STATE.lock();
    state.config = NetworkConfig {
        mac: device.mac_address(),
        ..NetworkConfig::default()
    };
    state.dhcp_state = Some(DhcpState::Idle);
    state.xid = 0;
    state.neighbor_ip = [0; 4];
    state.neighbor_mac = [0; 6];
    state.pending_ping = None;
    state.next_sequence = 1;
    *link = Some(device);
    Ok(())
}

pub fn unregister_link(slot_id: u32) {
    let mut link = LINK.lock();
    if link
        .as_ref()
        .is_some_and(|device| device.slot_id() == slot_id)
    {
        *link = None;
        *STATE.lock() = NetworkState::default();
    }
}

pub fn config() -> NetworkConfig {
    STATE.lock().config
}

pub fn create_udp_socket() -> Arc<Mutex<UdpSocket>> {
    Arc::new(Mutex::new(UdpSocket {
        local_port: 0,
        received: VecDeque::new(),
    }))
}

pub fn bind_udp_socket(
    socket: &Arc<Mutex<UdpSocket>>,
    requested_port: u16,
) -> Result<u16, &'static str> {
    let current_port = socket.lock().local_port;
    if current_port != 0 {
        return if requested_port == 0 || requested_port == current_port {
            Ok(current_port)
        } else {
            Err("UDP socket is already bound")
        };
    }

    let mut sockets = UDP_SOCKETS.lock();
    sockets.retain(|entry| entry.strong_count() != 0);

    let port = if requested_port == 0 {
        let mut selected = None;
        for _ in 0..EPHEMERAL_PORT_COUNT {
            let candidate = NEXT_EPHEMERAL_PORT.fetch_add(1, Ordering::Relaxed);
            let candidate = EPHEMERAL_PORT_START
                + candidate.wrapping_sub(EPHEMERAL_PORT_START) % EPHEMERAL_PORT_COUNT;
            if !udp_port_in_use(&sockets, candidate, socket) {
                selected = Some(candidate);
                break;
            }
        }
        selected.ok_or("no ephemeral UDP ports available")?
    } else {
        if udp_port_in_use(&sockets, requested_port, socket) {
            return Err("UDP port is already in use");
        }
        requested_port
    };

    socket.lock().local_port = port;
    if !sockets
        .iter()
        .filter_map(Weak::upgrade)
        .any(|registered| Arc::ptr_eq(&registered, socket))
    {
        sockets.push(Arc::downgrade(socket));
    }
    Ok(port)
}

fn udp_port_in_use(
    sockets: &[Weak<Mutex<UdpSocket>>],
    port: u16,
    except: &Arc<Mutex<UdpSocket>>,
) -> bool {
    sockets.iter().filter_map(Weak::upgrade).any(|socket| {
        !Arc::ptr_eq(&socket, except) && socket.lock().local_port == port
    })
}

pub fn send_udp_socket(
    socket: &Arc<Mutex<UdpSocket>>,
    destination: [u8; 4],
    destination_port: u16,
    payload: &[u8],
) -> Result<(), &'static str> {
    let source_port = {
        let socket_state = socket.lock();
        socket_state.local_port
    };
    let source_port = if source_port == 0 {
        bind_udp_socket(socket, 0)?
    } else {
        source_port
    };
    send_udp_ipv4(destination, source_port, destination_port, payload)
}

pub fn receive_udp_socket(
    socket: &Arc<Mutex<UdpSocket>>,
    output: &mut [u8],
) -> Result<(usize, [u8; 4], u16), &'static str> {
    let mut socket = socket.lock();
    let Some(datagram) = socket.received.pop_front() else {
        return Err("UDP socket would block");
    };
    let length = output.len().min(datagram.payload.len());
    output[..length].copy_from_slice(&datagram.payload[..length]);
    Ok((length, datagram.source_ip, datagram.source_port))
}

pub fn print_status() {
    let (config, dhcp_state) = {
        let state = STATE.lock();
        (state.config, state.dhcp_state)
    };
    if LINK.lock().is_none() {
        crate::println!("network: no interface (connect Android with USB tethering enabled)");
        return;
    }
    crate::println!(
        "MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        config.mac[0],
        config.mac[1],
        config.mac[2],
        config.mac[3],
        config.mac[4],
        config.mac[5]
    );
    if config.configured {
        crate::println!(
            "IPv4 {}.{}.{}.{} / {}.{}.{}.{}",
            config.address[0],
            config.address[1],
            config.address[2],
            config.address[3],
            config.netmask[0],
            config.netmask[1],
            config.netmask[2],
            config.netmask[3]
        );
        crate::println!(
            "gateway {}.{}.{}.{} DNS {}.{}.{}.{}",
            config.gateway[0],
            config.gateway[1],
            config.gateway[2],
            config.gateway[3],
            config.dns[0],
            config.dns[1],
            config.dns[2],
            config.dns[3]
        );
    } else {
        let message = match dhcp_state {
            Some(DhcpState::Discovering) => "DHCP discovery in progress",
            Some(DhcpState::Requesting) => "waiting for DHCP acknowledgement",
            _ => "no IPv4 address (run `net dhcp`)",
        };
        crate::println!("{message}");
    }
    crate::println!("transport: IPv4/ARP/UDP/ICMP (TCP is not implemented)");
}

pub fn wifi_scan() {
    let link = LINK.lock();
    let Some(device) = link.as_ref() else {
        crate::println!("wifi: no network interface registered (QCA9377 not detected or not initialized)");
        return;
    };
    if device.slot_id() == 0x9377 {
        crate::println!(
            "wifi: QCA9377 scan is not implemented yet; WMI-TLV scan control is still missing"
        );
    } else {
        crate::println!(
            "wifi: registered interface (slot {:#x}) is not a WiFi device",
            device.slot_id()
        );
    }
}

pub fn wifi_connect(ssid: &str) {
    let link = LINK.lock();
    let Some(device) = link.as_ref() else {
        crate::println!("wifi: no network interface registered");
        return;
    };
    if device.slot_id() == 0x9377 {
        crate::println!(
            "wifi: cannot connect to {:?}: 802.11 authentication/association is not implemented",
            ssid
        );
        crate::println!("wifi: no connection was started; DHCP will not work yet");
    } else {
        crate::println!(
            "wifi: registered interface (slot {:#x}) is not a WiFi device",
            device.slot_id()
        );
    }
}

pub fn start_dhcp() -> Result<(), &'static str> {
    let device = LINK.lock().clone().ok_or("no network interface")?;
    let (mac, xid) = {
        let mut state = STATE.lock();
        if state.dhcp_state.is_none() {
            return Err("network interface is not ready");
        }
        state.xid = state.xid.wrapping_add(1).max(1);
        state.dhcp_state = Some(DhcpState::Discovering);
        state.config.configured = false;
        (state.config.mac, state.xid)
    };
    let packet = build_dhcp_packet(mac, xid, 1, None, None);
    if let Err(error) = device.transmit(&packet) {
        STATE.lock().dhcp_state = Some(DhcpState::Idle);
        return Err(error);
    }
    crate::println!("DHCP discover sent");
    Ok(())
}

pub fn ping(target: [u8; 4]) -> Result<(), &'static str> {
    let device = LINK.lock().clone().ok_or("no network interface")?;
    let (request, sequence) = {
        let mut state = STATE.lock();
        if !state.config.configured {
            return Err("network interface has no IPv4 address; run `net dhcp` first");
        }
        let config = state.config;
        let next_hop = if same_subnet(target, config.address, config.netmask) {
            target
        } else {
            config.gateway
        };
        if next_hop == [0; 4] {
            return Err("no route to target");
        }
        let sequence = state.next_sequence;
        state.next_sequence = state.next_sequence.wrapping_add(1);
        state.pending_ping = Some(PendingPing {
            target,
            next_hop,
            sequence,
        });
        (
            build_arp_request(config.mac, config.address, next_hop),
            sequence,
        )
    };
    if let Err(error) = device.transmit(&request) {
        STATE.lock().pending_ping = None;
        return Err(error);
    }
    crate::println!(
        "PING {}.{}.{}.{}: ARP lookup started (seq={sequence})",
        target[0],
        target[1],
        target[2],
        target[3]
    );
    Ok(())
}

pub fn receive_ethernet(frame: &[u8]) {
    if frame.len() < 14 {
        return;
    }
    let ethertype = u16::from_be_bytes([frame[12], frame[13]]);
    match ethertype {
        ETHERTYPE_ARP => receive_arp(frame),
        ETHERTYPE_IPV4 => receive_ipv4(frame),
        _ => {}
    }
}

pub fn send_udp_ipv4(
    destination: [u8; 4],
    source_port: u16,
    destination_port: u16,
    payload: &[u8],
) -> Result<(), &'static str> {
    let device = LINK.lock().clone().ok_or("no network interface")?;
    let (config, next_hop, destination_mac) = {
        let state = STATE.lock();
        if !state.config.configured {
            return Err("network interface has no IPv4 address");
        }
        let next_hop = if same_subnet(destination, state.config.address, state.config.netmask) {
            destination
        } else {
            state.config.gateway
        };
        if next_hop == [0; 4] {
            return Err("no route to destination");
        }
        let destination_mac = (next_hop == state.neighbor_ip && state.neighbor_mac != [0; 6])
            .then_some(state.neighbor_mac);
        (state.config, next_hop, destination_mac)
    };
    let Some(destination_mac) = destination_mac else {
        device.transmit(&build_arp_request(config.mac, config.address, next_hop))?;
        return Err("ARP request sent; retry after the neighbor responds");
    };
    let frame = build_udp_frame(
        config.mac,
        destination_mac,
        config.address,
        destination,
        source_port,
        destination_port,
        payload,
    )?;
    device.transmit(&frame)
}

fn receive_arp(frame: &[u8]) {
    if frame.len() < 42 {
        return;
    }
    let arp = &frame[14..42];
    if u16::from_be_bytes([arp[0], arp[1]]) != 1
        || u16::from_be_bytes([arp[2], arp[3]]) != ETHERTYPE_IPV4
        || arp[4] != 6
        || arp[5] != 4
    {
        return;
    }
    let operation = u16::from_be_bytes([arp[6], arp[7]]);
    if operation != 1 && operation != 2 {
        return;
    }
    let sender_mac: [u8; 6] = arp[8..14].try_into().unwrap();
    let sender_ip: [u8; 4] = arp[14..18].try_into().unwrap();
    let target_ip: [u8; 4] = arp[24..28].try_into().unwrap();
    let Some(device) = LINK.lock().clone() else {
        return;
    };
    let (config, pending, answer_request) = {
        let mut state = STATE.lock();
        if !state.config.configured || target_ip != state.config.address {
            return;
        }
        state.neighbor_ip = sender_ip;
        state.neighbor_mac = sender_mac;
        (state.config, state.pending_ping, operation == 1)
    };
    if answer_request {
        let mut reply = alloc::vec![0u8; 42];
        reply[..6].copy_from_slice(&sender_mac);
        reply[6..12].copy_from_slice(&config.mac);
        reply[12..14].copy_from_slice(&ETHERTYPE_ARP.to_be_bytes());
        reply[14..16].copy_from_slice(&1u16.to_be_bytes());
        reply[16..18].copy_from_slice(&ETHERTYPE_IPV4.to_be_bytes());
        reply[18] = 6;
        reply[19] = 4;
        reply[20..22].copy_from_slice(&2u16.to_be_bytes());
        reply[22..28].copy_from_slice(&config.mac);
        reply[28..32].copy_from_slice(&config.address);
        reply[32..38].copy_from_slice(&sender_mac);
        reply[38..42].copy_from_slice(&sender_ip);
        if let Err(error) = device.transmit(&reply) {
            crate::println!("ARP reply failed: {error}");
        }
    }
    if operation == 2 {
        if let Some(pending) = pending {
            if pending.next_hop == sender_ip {
                if let Err(error) = send_icmp_echo(config, sender_mac, pending) {
                    crate::println!("ICMP echo request failed: {error}");
                }
            }
        }
    }
}

fn receive_ipv4(frame: &[u8]) {
    if frame.len() < 34 {
        return;
    }
    let packet = &frame[14..];
    if packet[0] >> 4 != 4 {
        return;
    }
    let header_len = ((packet[0] & 0x0F) as usize) * 4;
    let total_len = u16::from_be_bytes([packet[2], packet[3]]) as usize;
    let fragment_field = u16::from_be_bytes([packet[6], packet[7]]);
    if header_len < 20
        || total_len < header_len
        || total_len > packet.len()
        || fragment_field & 0x3fff != 0
        || checksum(&packet[..header_len]) != 0
    {
        return;
    }
    let source_ip: [u8; 4] = packet[12..16].try_into().unwrap();
    let destination_ip: [u8; 4] = packet[16..20].try_into().unwrap();
    match packet[9] {
        IP_PROTOCOL_ICMP => {
            let config = STATE.lock().config;
            if config.configured && destination_ip == config.address {
                receive_icmp(source_ip, &packet[header_len..total_len]);
            }
        }
        IP_PROTOCOL_UDP => {
            receive_udp(source_ip, destination_ip, &packet[header_len..total_len])
        }
        IP_PROTOCOL_TCP => {
            let config = STATE.lock().config;
            if config.configured && destination_ip == config.address {
                tcp::receive_packet(source_ip, destination_ip, &packet[header_len..total_len]);
            }
        }
        _ => {}
    }
}

fn receive_udp(source_ip: [u8; 4], destination_ip: [u8; 4], packet: &[u8]) {
    if packet.len() < 8 {
        return;
    }
    let source_port = u16::from_be_bytes([packet[0], packet[1]]);
    let destination_port = u16::from_be_bytes([packet[2], packet[3]]);
    let length = u16::from_be_bytes([packet[4], packet[5]]) as usize;
    if length < 8 || length > packet.len() {
        return;
    }
    if source_port == DHCP_SERVER_PORT && destination_port == DHCP_CLIENT_PORT {
        handle_dhcp_packet(source_ip, &packet[8..length]);
        return;
    }

    let config = STATE.lock().config;
    if !config.configured
        || (destination_ip != config.address && destination_ip != BROADCAST_IPV4)
    {
        return;
    }
    let mut sockets = UDP_SOCKETS.lock();
    sockets.retain(|entry| entry.strong_count() != 0);
    for socket in sockets.iter().filter_map(Weak::upgrade) {
        let mut socket = socket.lock();
        if socket.local_port == destination_port {
            queue_udp_datagram(&mut socket, source_ip, source_port, &packet[8..length]);
        }
    }
}

fn queue_udp_datagram(
    socket: &mut UdpSocket,
    source_ip: [u8; 4],
    source_port: u16,
    payload: &[u8],
) -> bool {
    if socket.local_port == 0 || socket.received.len() >= MAX_UDP_QUEUE {
        return false;
    }
    socket.received.push_back(ReceivedDatagram {
        source_ip,
        source_port,
        payload: payload.to_vec(),
    });
    true
}

fn handle_dhcp_packet(source_ip: [u8; 4], packet: &[u8]) {
    let (mac, xid, state_kind) = {
        let state = STATE.lock();
        (state.config.mac, state.xid, state.dhcp_state)
    };
    let Some(message) = parse_dhcp(packet, xid, mac) else {
        return;
    };
    match (state_kind, message.message_type) {
        (Some(DhcpState::Discovering), 2) => {
            let device = match LINK.lock().clone() {
                Some(device) => device,
                None => return,
            };
            {
                let mut state = STATE.lock();
                state.dhcp_state = Some(DhcpState::Requesting);
            }
            let request = build_dhcp_packet(
                mac,
                xid,
                3,
                Some(message.address),
                Some(message.server.unwrap_or(source_ip)),
            );
            if let Err(error) = device.transmit(&request) {
                crate::println!("DHCP request failed: {error}");
                STATE.lock().dhcp_state = Some(DhcpState::Idle);
            } else {
                crate::println!(
                    "DHCP offer received: {}.{}.{}.{}",
                    message.address[0],
                    message.address[1],
                    message.address[2],
                    message.address[3]
                );
            }
        }
        (Some(DhcpState::Requesting), 5) | (Some(DhcpState::Discovering), 5) => {
            let mut state = STATE.lock();
            state.config.address = message.address;
            state.config.netmask = message.netmask.unwrap_or([255, 255, 255, 0]);
            state.config.gateway = message.router.unwrap_or([0; 4]);
            state.config.dns = message.dns.unwrap_or([0; 4]);
            state.config.configured = message.address != [0; 4];
            state.dhcp_state = Some(DhcpState::Bound);
            state.neighbor_ip = [0; 4];
            state.neighbor_mac = [0; 6];
            crate::println!(
                "DHCP bound: {}.{}.{}.{} gateway {}.{}.{}.{}",
                message.address[0],
                message.address[1],
                message.address[2],
                message.address[3],
                state.config.gateway[0],
                state.config.gateway[1],
                state.config.gateway[2],
                state.config.gateway[3]
            );
        }
        (Some(DhcpState::Requesting), 6) | (Some(DhcpState::Discovering), 6) => {
            let mut state = STATE.lock();
            state.config.configured = false;
            state.dhcp_state = Some(DhcpState::Idle);
            crate::println!("DHCP server rejected the lease request");
        }
        _ => {}
    }
}

fn receive_icmp(source_ip: [u8; 4], packet: &[u8]) {
    if packet.len() < 8 || checksum(packet) != 0 {
        return;
    }
    match packet[0] {
        0 => {
            let identifier = u16::from_be_bytes([packet[4], packet[5]]);
            let sequence = u16::from_be_bytes([packet[6], packet[7]]);
            let mut state = STATE.lock();
            if identifier == 0x4F43
                && state.pending_ping.is_some_and(|pending| {
                    pending.sequence == sequence && pending.target == source_ip
                })
            {
                state.pending_ping = None;
                crate::println!(
                    "Reply from {}.{}.{}.{}: seq={sequence}",
                    source_ip[0],
                    source_ip[1],
                    source_ip[2],
                    source_ip[3]
                );
            }
        }
        8 => {
            let Some(device) = LINK.lock().clone() else {
                return;
            };
            let (config, destination_mac) = {
                let state = STATE.lock();
                if !state.config.configured {
                    return;
                }
                let destination_mac = if state.neighbor_ip == source_ip {
                    state.neighbor_mac
                } else {
                    BROADCAST_MAC
                };
                (state.config, destination_mac)
            };
            let mut response = packet.to_vec();
            response[0] = 0;
            response[2] = 0;
            response[3] = 0;
            let checksum = checksum(&response);
            response[2..4].copy_from_slice(&checksum.to_be_bytes());
            match build_ipv4_frame(
                config.mac,
                destination_mac,
                config.address,
                source_ip,
                IP_PROTOCOL_ICMP,
                &response,
            ) {
                Ok(frame) => {
                    if let Err(error) = device.transmit(&frame) {
                        crate::println!("ICMP echo reply failed: {error}");
                    }
                }
                Err(error) => crate::println!("ICMP echo reply failed: {error}"),
            }
        }
        _ => {}
    }
}

fn send_icmp_echo(
    config: NetworkConfig,
    destination_mac: [u8; 6],
    pending: PendingPing,
) -> Result<(), &'static str> {
    let device = LINK.lock().clone().ok_or("no network interface")?;
    let mut icmp = alloc::vec![0u8; 16];
    icmp[0] = 8;
    icmp[4..6].copy_from_slice(&0x4F43u16.to_be_bytes());
    icmp[6..8].copy_from_slice(&pending.sequence.to_be_bytes());
    icmp[8..16].copy_from_slice(b"OCTAGRAM");
    let sum = checksum(&icmp);
    icmp[2..4].copy_from_slice(&sum.to_be_bytes());
    let frame = build_ipv4_frame(
        config.mac,
        destination_mac,
        config.address,
        pending.target,
        IP_PROTOCOL_ICMP,
        &icmp,
    )?;
    device.transmit(&frame)
}

fn build_arp_request(source_mac: [u8; 6], source_ip: [u8; 4], target_ip: [u8; 4]) -> Vec<u8> {
    let mut frame = alloc::vec![0u8; 42];
    frame[..6].copy_from_slice(&BROADCAST_MAC);
    frame[6..12].copy_from_slice(&source_mac);
    frame[12..14].copy_from_slice(&ETHERTYPE_ARP.to_be_bytes());
    frame[14..16].copy_from_slice(&1u16.to_be_bytes());
    frame[16..18].copy_from_slice(&ETHERTYPE_IPV4.to_be_bytes());
    frame[18] = 6;
    frame[19] = 4;
    frame[20..22].copy_from_slice(&1u16.to_be_bytes());
    frame[22..28].copy_from_slice(&source_mac);
    frame[28..32].copy_from_slice(&source_ip);
    frame[38..42].copy_from_slice(&target_ip);
    frame
}

fn build_dhcp_packet(
    mac: [u8; 6],
    xid: u32,
    message_type: u8,
    requested_address: Option<[u8; 4]>,
    server: Option<[u8; 4]>,
) -> Vec<u8> {
    let mut bootp = alloc::vec![0u8; 240];
    bootp[0] = 1;
    bootp[1] = 1;
    bootp[2] = 6;
    bootp[4..8].copy_from_slice(&xid.to_be_bytes());
    bootp[10..12].copy_from_slice(&0x8000u16.to_be_bytes());
    bootp[28..34].copy_from_slice(&mac);
    bootp[236..240].copy_from_slice(&DHCP_COOKIE);
    let mut options = Vec::new();
    options.extend_from_slice(&[53, 1, message_type]);
    if let Some(address) = requested_address {
        options.extend_from_slice(&[50, 4]);
        options.extend_from_slice(&address);
    }
    if let Some(server) = server {
        options.extend_from_slice(&[54, 4]);
        options.extend_from_slice(&server);
    }
    options.extend_from_slice(&[55, 5, 1, 3, 6, 51, 54, 255]);
    bootp.extend_from_slice(&options);
    build_udp_frame(
        mac,
        BROADCAST_MAC,
        [0; 4],
        BROADCAST_IPV4,
        DHCP_CLIENT_PORT,
        DHCP_SERVER_PORT,
        &bootp,
    )
    .expect("DHCP packet fits a standard Ethernet frame")
}

fn build_udp_frame(
    source_mac: [u8; 6],
    destination_mac: [u8; 6],
    source_ip: [u8; 4],
    destination_ip: [u8; 4],
    source_port: u16,
    destination_port: u16,
    payload: &[u8],
) -> Result<Vec<u8>, &'static str> {
    if payload.len() > IPV4_MTU - 28 {
        return Err("UDP payload exceeds the IPv4 MTU");
    }
    let mut udp = alloc::vec![0u8; 8 + payload.len()];
    udp[0..2].copy_from_slice(&source_port.to_be_bytes());
    udp[2..4].copy_from_slice(&destination_port.to_be_bytes());
    let udp_length = udp.len() as u16;
    udp[4..6].copy_from_slice(&udp_length.to_be_bytes());
    udp[8..].copy_from_slice(payload);
    build_ipv4_frame(
        source_mac,
        destination_mac,
        source_ip,
        destination_ip,
        IP_PROTOCOL_UDP,
        &udp,
    )
}

fn build_ipv4_frame(
    source_mac: [u8; 6],
    destination_mac: [u8; 6],
    source_ip: [u8; 4],
    destination_ip: [u8; 4],
    protocol: u8,
    payload: &[u8],
) -> Result<Vec<u8>, &'static str> {
    if payload.len() > IPV4_MTU - 20 {
        return Err("IPv4 payload exceeds the MTU");
    }
    let mut frame = alloc::vec![0u8; 14 + 20 + payload.len()];
    frame[..6].copy_from_slice(&destination_mac);
    frame[6..12].copy_from_slice(&source_mac);
    frame[12..14].copy_from_slice(&ETHERTYPE_IPV4.to_be_bytes());
    let ip = &mut frame[14..34];
    ip[0] = 0x45;
    ip[2..4].copy_from_slice(&((20 + payload.len()) as u16).to_be_bytes());
    ip[4..6].copy_from_slice(&1u16.to_be_bytes());
    ip[8] = 64;
    ip[9] = protocol;
    ip[12..16].copy_from_slice(&source_ip);
    ip[16..20].copy_from_slice(&destination_ip);
    let sum = checksum(ip);
    ip[10..12].copy_from_slice(&sum.to_be_bytes());
    frame[34..].copy_from_slice(payload);
    Ok(frame)
}

fn parse_dhcp(packet: &[u8], expected_xid: u32, mac: [u8; 6]) -> Option<DhcpMessage> {
    if packet.len() < 240
        || packet[0] != 2
        || packet[1] != 1
        || packet[2] != 6
        || u32::from_be_bytes(packet[4..8].try_into().ok()?) != expected_xid
        || packet[28..34] != mac
        || packet[236..240] != DHCP_COOKIE
    {
        return None;
    }
    let address = packet[16..20].try_into().ok()?;
    let mut message_type = None;
    let mut server = None;
    let mut netmask = None;
    let mut router = None;
    let mut dns = None;
    let mut index = 240;
    while index < packet.len() {
        let option = packet[index];
        index += 1;
        match option {
            0 => continue,
            255 => break,
            _ => {}
        }
        let length = *packet.get(index)? as usize;
        index += 1;
        let value = packet.get(index..index.checked_add(length)?)?;
        index += length;
        match (option, value.len()) {
            (53, 1) => message_type = Some(value[0]),
            (54, 4) => server = Some(value.try_into().ok()?),
            (1, 4) => netmask = Some(value.try_into().ok()?),
            (3, length) if length >= 4 => router = Some(value[..4].try_into().ok()?),
            (6, length) if length >= 4 => dns = Some(value[..4].try_into().ok()?),
            _ => {}
        }
    }
    Some(DhcpMessage {
        message_type: message_type?,
        address,
        server,
        netmask,
        router,
        dns,
    })
}

struct DhcpMessage {
    message_type: u8,
    address: [u8; 4],
    server: Option<[u8; 4]>,
    netmask: Option<[u8; 4]>,
    router: Option<[u8; 4]>,
    dns: Option<[u8; 4]>,
}

fn same_subnet(address: [u8; 4], local: [u8; 4], mask: [u8; 4]) -> bool {
    (0..4).all(|index| address[index] & mask[index] == local[index] & mask[index])
}

fn checksum(bytes: &[u8]) -> u16 {
    let mut sum = 0u32;
    let mut chunks = bytes.chunks_exact(2);
    for chunk in &mut chunks {
        sum += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
    }
    if let Some(&last) = chunks.remainder().first() {
        sum += (last as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

#[cfg(test)]
mod tests {
    use super::{
        LinkDevice, MAX_UDP_QUEUE, NetworkConfig, UdpSocket, bind_udp_socket,
        build_arp_request, build_dhcp_packet, build_udp_frame, checksum, create_udp_socket,
        parse_dhcp, queue_udp_datagram, receive_ethernet, receive_udp_socket, register_link,
        send_udp_socket, unregister_link,
    };
    use alloc::collections::VecDeque;
    use alloc::sync::Arc;
    use spin::Mutex;

    struct TestLink {
        slot_id: u32,
        mac: [u8; 6],
        frames: Mutex<Vec<Vec<u8>>>,
    }

    impl LinkDevice for TestLink {
        fn slot_id(&self) -> u32 {
            self.slot_id
        }

        fn mac_address(&self) -> [u8; 6] {
            self.mac
        }

        fn transmit(&self, frame: &[u8]) -> Result<(), &'static str> {
            self.frames.lock().push(frame.to_vec());
            Ok(())
        }
    }

    struct RegisteredLink(u32);

    impl Drop for RegisteredLink {
        fn drop(&mut self) {
            unregister_link(self.0);
        }
    }

    #[test]
    fn builds_a_broadcast_arp_request() {
        let frame = build_arp_request([2, 0, 0, 0, 0, 1], [10, 0, 0, 2], [10, 0, 0, 1]);
        assert_eq!(&frame[..6], &[0xff; 6]);
        assert_eq!(&frame[12..14], &[0x08, 0x06]);
        assert_eq!(&frame[38..42], &[10, 0, 0, 1]);
    }

    #[test]
    fn builds_dhcp_discover_with_valid_bootp_identity() {
        let frame = build_dhcp_packet([2, 1, 2, 3, 4, 5], 0x1234, 1, None, None);
        assert_eq!(&frame[12..14], &[0x08, 0x00]);
        assert_eq!(&frame[34 + 8 + 4..34 + 8 + 8], &0x1234u32.to_be_bytes());
        assert_eq!(&frame[34 + 8 + 236..34 + 8 + 240], &super::DHCP_COOKIE);
    }

    #[test]
    fn parses_dhcp_offer_and_rejects_wrong_transaction() {
        let mac = [2, 1, 2, 3, 4, 5];
        let xid = 0x1122_3344u32;
        let options = [
            53, 1, 2, 54, 4, 192, 168, 42, 1, 1, 4, 255, 255, 255, 0, 3, 4, 192, 168, 42, 1, 6, 4,
            8, 8, 8, 8, 255,
        ];
        let mut packet = vec![0u8; 240 + options.len()];
        packet[0] = 2;
        packet[1] = 1;
        packet[2] = 6;
        packet[4..8].copy_from_slice(&xid.to_be_bytes());
        packet[16..20].copy_from_slice(&[192, 168, 42, 3]);
        packet[28..34].copy_from_slice(&mac);
        packet[236..240].copy_from_slice(&super::DHCP_COOKIE);
        packet[240..].copy_from_slice(&options);
        let offer = parse_dhcp(&packet, xid, mac).unwrap();
        assert_eq!(offer.message_type, 2);
        assert_eq!(offer.address, [192, 168, 42, 3]);
        assert_eq!(offer.server, Some([192, 168, 42, 1]));
        assert_eq!(offer.netmask, Some([255, 255, 255, 0]));
        assert_eq!(offer.router, Some([192, 168, 42, 1]));
        assert_eq!(offer.dns, Some([8, 8, 8, 8]));
        assert!(parse_dhcp(&packet, xid + 1, mac).is_none());
    }

    #[test]
    fn internet_checksum_matches_standard_header_example() {
        let header = [
            0x45, 0x00, 0x00, 0x73, 0x00, 0x00, 0x40, 0x00, 0x40, 0x11, 0x00, 0x00, 0xc0, 0xa8,
            0x00, 0x01, 0xc0, 0xa8, 0x00, 0xc7,
        ];
        assert_eq!(checksum(&header), 0xb861);
    }

    #[test]
    fn builds_udp_ipv4_frames_with_a_valid_header_checksum() {
        let frame = build_udp_frame(
            [2, 0, 0, 0, 0, 1],
            [2, 0, 0, 0, 0, 2],
            [192, 168, 42, 2],
            [192, 168, 42, 1],
            49152,
            7,
            b"octagram",
        )
        .unwrap();
        assert_eq!(&frame[12..14], &[0x08, 0x00]);
        assert_eq!(checksum(&frame[14..34]), 0);
        assert_eq!(&frame[34..36], &49152u16.to_be_bytes());
        assert_eq!(&frame[36..38], &7u16.to_be_bytes());
        assert_eq!(&frame[42..], b"octagram");
    }

    #[test]
    fn rejects_udp_payloads_that_exceed_the_link_mtu() {
        let payload = alloc::vec![0u8; 1473];
        assert!(
            build_udp_frame(
                [2, 0, 0, 0, 0, 1],
                [2, 0, 0, 0, 0, 2],
                [192, 168, 42, 2],
                [192, 168, 42, 1],
                1,
                2,
                &payload
            )
            .is_err()
        );
    }

    #[test]
    fn udp_socket_receives_source_and_payload_and_bounds_its_queue() {
        let socket = Arc::new(Mutex::new(UdpSocket {
            local_port: 53000,
            received: VecDeque::new(),
        }));
        {
            let mut socket_state = socket.lock();
            assert!(queue_udp_datagram(
                &mut socket_state,
                [192, 168, 42, 1],
                53,
                b"response",
            ));
        }

        let mut output = [0; 4];
        let (length, source_ip, source_port) =
            receive_udp_socket(&socket, &mut output).unwrap();
        assert_eq!(length, 4);
        assert_eq!(source_ip, [192, 168, 42, 1]);
        assert_eq!(source_port, 53);
        assert_eq!(&output, b"resp");

        let mut socket_state = socket.lock();
        for _ in 0..MAX_UDP_QUEUE {
            assert!(queue_udp_datagram(
                &mut socket_state,
                [192, 168, 42, 1],
                53,
                b"x",
            ));
        }
        assert!(!queue_udp_datagram(
            &mut socket_state,
            [192, 168, 42, 1],
            53,
            b"overflow",
        ));
    }

    #[test]
    fn udp_bind_rejects_a_port_owned_by_another_socket() {
        let bound = create_udp_socket();
        let unbound = create_udp_socket();
        assert_eq!(bind_udp_socket(&bound, 53000), Ok(53000));
        assert_eq!(
            bind_udp_socket(&unbound, 53000),
            Err("UDP port is already in use")
        );
        assert_ne!(bind_udp_socket(&unbound, 0), Ok(53000));
    }

    #[test]
    fn udp_socket_transmits_and_receives_ipv4_datagrams() {
        let local_mac = [2, 0, 0, 0, 0, 1];
        let peer_mac = [2, 0, 0, 0, 0, 2];
        let local_ip = [192, 168, 42, 2];
        let peer_ip = [192, 168, 42, 1];
        let link = Arc::new(TestLink {
            slot_id: 0x5544,
            mac: local_mac,
            frames: Mutex::new(Vec::new()),
        });
        register_link(link.clone()).unwrap();
        let _registered_link = RegisteredLink(link.slot_id);
        {
            let mut state = super::STATE.lock();
            state.config = NetworkConfig {
                mac: local_mac,
                address: local_ip,
                netmask: [255, 255, 255, 0],
                gateway: peer_ip,
                dns: peer_ip,
                configured: true,
            };
            state.neighbor_ip = peer_ip;
            state.neighbor_mac = peer_mac;
        }

        let socket = create_udp_socket();
        let local_port = bind_udp_socket(&socket, 53001).unwrap();
        let incoming = build_udp_frame(
            peer_mac,
            local_mac,
            peer_ip,
            local_ip,
            53,
            local_port,
            b"dns response",
        )
        .unwrap();
        receive_ethernet(&incoming);

        let mut received = [0; 32];
        let (length, source_ip, source_port) =
            receive_udp_socket(&socket, &mut received).unwrap();
        assert_eq!(&received[..length], b"dns response");
        assert_eq!(source_ip, peer_ip);
        assert_eq!(source_port, 53);

        send_udp_socket(&socket, peer_ip, 53, b"dns query").unwrap();
        let transmitted = link.frames.lock();
        let frame = transmitted.last().unwrap();
        assert_eq!(&frame[12..14], &super::ETHERTYPE_IPV4.to_be_bytes());
        assert_eq!(&frame[34..36], &local_port.to_be_bytes());
        assert_eq!(&frame[36..38], &53u16.to_be_bytes());
        assert_eq!(&frame[42..], b"dns query");
    }
}
