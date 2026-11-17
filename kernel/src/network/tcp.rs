use alloc::collections::VecDeque;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU16, AtomicU32, Ordering};
use spin::Mutex;

const TCP_PROTOCOL: u8 = 6;
const TCP_MSS: u16 = 1460;
const TCP_RECEIVE_CAPACITY: usize = 8192;
const MAX_LISTEN_BACKLOG: usize = 8;
const RETRANSMIT_TICKS: u32 = 30;
const MAX_RETRANSMITS: u8 = 5;

const FIN: u8 = 0x01;
const SYN: u8 = 0x02;
const RST: u8 = 0x04;
const PSH: u8 = 0x08;
const ACK: u8 = 0x10;

static SOCKETS: Mutex<Vec<Arc<Mutex<TcpSocket>>>> = Mutex::new(Vec::new());
static NEXT_PORT: AtomicU16 = AtomicU16::new(EPHEMERAL_PORT_START);
static NEXT_SEQUENCE: AtomicU32 = AtomicU32::new(0x4f43_5447);
static TICK: AtomicU32 = AtomicU32::new(0);

const EPHEMERAL_PORT_START: u16 = 49152;
const EPHEMERAL_PORT_COUNT: u16 = u16::MAX - EPHEMERAL_PORT_START + 1;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TcpState {
    Created,
    Bound,
    Listening,
    SynSent,
    SynReceived,
    Established,
    FinWait1,
    FinWait2,
    CloseWait,
    LastAck,
    Closed,
    Reset,
}

#[derive(Clone)]
struct PendingSegment {
    sequence: u32,
    acknowledgement: u32,
    flags: u8,
    payload: Vec<u8>,
    include_mss: bool,
    last_sent: u32,
    retransmits: u8,
}

pub struct TcpSocket {
    local_address: [u8; 4],
    local_port: u16,
    remote_address: [u8; 4],
    remote_port: u16,
    state: TcpState,
    send_unacknowledged: u32,
    send_next: u32,
    receive_next: u32,
    peer_window: u16,
    peer_mss: u16,
    received: VecDeque<u8>,
    receive_eof: bool,
    read_shutdown: bool,
    write_shutdown: bool,
    error: Option<&'static str>,
    pending: Option<PendingSegment>,
    close_after_pending: bool,
    backlog: usize,
    half_open: Vec<Weak<Mutex<TcpSocket>>>,
    accepted: VecDeque<Arc<Mutex<TcpSocket>>>,
    listener: Weak<Mutex<TcpSocket>>,
}

impl TcpSocket {
    fn new() -> Self {
        Self {
            local_address: [0; 4],
            local_port: 0,
            remote_address: [0; 4],
            remote_port: 0,
            state: TcpState::Created,
            send_unacknowledged: 0,
            send_next: 0,
            receive_next: 0,
            peer_window: u16::MAX,
            peer_mss: TCP_MSS,
            received: VecDeque::new(),
            receive_eof: false,
            read_shutdown: false,
            write_shutdown: false,
            error: None,
            pending: None,
            close_after_pending: false,
            backlog: 0,
            half_open: Vec::new(),
            accepted: VecDeque::new(),
            listener: Weak::new(),
        }
    }
}

pub fn create_socket() -> Arc<Mutex<TcpSocket>> {
    Arc::new(Mutex::new(TcpSocket::new()))
}

pub fn bind_socket(
    socket: &Arc<Mutex<TcpSocket>>,
    local_address: [u8; 4],
    requested_port: u16,
) -> Result<u16, &'static str> {
    let (state, current_port) = {
        let socket = socket.lock();
        (socket.state, socket.local_port)
    };
    if state != TcpState::Created && state != TcpState::Bound {
        return Err("TCP socket cannot be bound in its current state");
    }
    if current_port != 0 {
        let current_address = socket.lock().local_address;
        if requested_port == 0 || requested_port == current_port {
            return if local_address == [0; 4] || current_address == local_address {
                Ok(current_port)
            } else {
                Err("TCP socket is already bound")
            };
        }
        return Err("TCP socket is already bound");
    }

    let config = super::config();
    if local_address != [0; 4] && config.configured && local_address != config.address {
        return Err("local IPv4 address is not assigned to this interface");
    }

    let mut sockets = SOCKETS.lock();
    sockets.retain(|entry| !is_terminal(entry));
    let port = if requested_port == 0 {
        let mut selected = None;
        for _ in 0..EPHEMERAL_PORT_COUNT {
            let candidate = NEXT_PORT.fetch_add(1, Ordering::Relaxed);
            let candidate = EPHEMERAL_PORT_START
                + candidate.wrapping_sub(EPHEMERAL_PORT_START) % EPHEMERAL_PORT_COUNT;
            if !tcp_port_in_use(&sockets, local_address, candidate, socket) {
                selected = Some(candidate);
                break;
            }
        }
        selected.ok_or("no ephemeral TCP ports available")?
    } else {
        if tcp_port_in_use(&sockets, local_address, requested_port, socket) {
            return Err("TCP port is already in use");
        }
        requested_port
    };

    {
        let mut socket_state = socket.lock();
        socket_state.local_address = local_address;
        socket_state.local_port = port;
        socket_state.state = TcpState::Bound;
    }
    register_socket_locked(&mut sockets, socket);
    Ok(port)
}

fn tcp_port_in_use(
    sockets: &[Arc<Mutex<TcpSocket>>],
    local_address: [u8; 4],
    port: u16,
    except: &Arc<Mutex<TcpSocket>>,
) -> bool {
    sockets.iter().any(|socket| {
        if Arc::ptr_eq(socket, except) {
            return false;
        }
        let socket = socket.lock();
        socket.local_port == port
            && socket.state != TcpState::Closed
            && socket.state != TcpState::Reset
            && (local_address == [0; 4]
                || socket.local_address == [0; 4]
                || socket.local_address == local_address)
    })
}

fn register_socket_locked(
    sockets: &mut Vec<Arc<Mutex<TcpSocket>>>,
    socket: &Arc<Mutex<TcpSocket>>,
) {
    if !sockets
        .iter()
        .any(|registered| Arc::ptr_eq(registered, socket))
    {
        sockets.push(Arc::clone(socket));
    }
}

fn is_terminal(socket: &Arc<Mutex<TcpSocket>>) -> bool {
    matches!(socket.lock().state, TcpState::Closed | TcpState::Reset)
}

pub fn is_last_descriptor(socket: &Arc<Mutex<TcpSocket>>) -> bool {
    let sockets = SOCKETS.lock();
    let registry_reference = sockets
        .iter()
        .any(|registered| Arc::ptr_eq(registered, socket)) as usize;
    Arc::strong_count(socket) == 1 + registry_reference
}

pub fn listen_socket(socket: &Arc<Mutex<TcpSocket>>, backlog: usize) -> Result<(), &'static str> {
    if backlog == 0 {
        return Err("TCP listen backlog must be positive");
    }
    if socket.lock().local_port == 0 {
        bind_socket(socket, [0; 4], 0)?;
    }
    let mut socket = socket.lock();
    if socket.state != TcpState::Bound && socket.state != TcpState::Listening {
        return Err("TCP socket cannot listen in its current state");
    }
    socket.backlog = backlog.min(MAX_LISTEN_BACKLOG);
    socket.state = TcpState::Listening;
    Ok(())
}

pub fn accept_socket(
    socket: &Arc<Mutex<TcpSocket>>,
) -> Result<(Arc<Mutex<TcpSocket>>, [u8; 4], u16), &'static str> {
    let mut socket = socket.lock();
    if socket.state != TcpState::Listening {
        return Err("TCP socket is not listening");
    }
    let Some(accepted) = socket.accepted.pop_front() else {
        return Err("TCP accept would block");
    };
    let accepted_state = accepted.lock();
    let address = accepted_state.remote_address;
    let port = accepted_state.remote_port;
    drop(accepted_state);
    Ok((accepted, address, port))
}

pub fn connect_socket(
    socket: &Arc<Mutex<TcpSocket>>,
    remote_address: [u8; 4],
    remote_port: u16,
) -> Result<(), &'static str> {
    if remote_address == [0; 4] || remote_address == [255; 4] || remote_port == 0 {
        return Err("invalid TCP peer address");
    }
    let state = socket.lock().state;
    if state == TcpState::Created {
        bind_socket(socket, [0; 4], 0)?;
    }
    let config = super::config();
    if !config.configured {
        return Err("network interface has no IPv4 address");
    }

    let pending = {
        let mut socket = socket.lock();
        match socket.state {
            TcpState::Established => {
                return if socket.remote_address == remote_address
                    && socket.remote_port == remote_port
                {
                    Ok(())
                } else {
                    Err("TCP socket is already connected")
                };
            }
            TcpState::SynSent => return Err("TCP connection is in progress"),
            TcpState::Reset => {
                return Err(socket.error.unwrap_or("TCP connection failed"));
            }
            TcpState::Bound => {}
            _ => return Err("TCP socket cannot connect in its current state"),
        }
        socket.remote_address = remote_address;
        socket.remote_port = remote_port;
        socket.state = TcpState::SynSent;
        let sequence = next_initial_sequence();
        socket.send_unacknowledged = sequence;
        socket.send_next = sequence.wrapping_add(1);
        socket.pending = Some(PendingSegment {
            sequence,
            acknowledgement: 0,
            flags: SYN,
            payload: Vec::new(),
            include_mss: true,
            last_sent: TICK.load(Ordering::Relaxed),
            retransmits: 0,
        });
        socket.pending.clone().unwrap()
    };
    if let Err(error) = transmit_pending(socket, pending) {
        let mut socket = socket.lock();
        socket.state = TcpState::Reset;
        socket.error = Some(error);
        return Err(error);
    }
    Err("TCP connection is in progress")
}

fn next_initial_sequence() -> u32 {
    NEXT_SEQUENCE.fetch_add(64_001, Ordering::Relaxed)
}

pub fn send_socket(socket: &Arc<Mutex<TcpSocket>>, payload: &[u8]) -> Result<usize, &'static str> {
    if payload.is_empty() {
        return Ok(0);
    }
    let pending = {
        let mut socket = socket.lock();
        if socket.write_shutdown {
            return Err("TCP write side is shut down");
        }
        match socket.state {
            TcpState::Established | TcpState::CloseWait => {}
            TcpState::Reset => return Err(socket.error.unwrap_or("TCP connection reset")),
            TcpState::Closed => return Err("TCP socket is closed"),
            _ => return Err("TCP socket is not connected"),
        }
        if socket.pending.is_some() {
            return Err("TCP send would block");
        }
        let length = payload
            .len()
            .min(socket.peer_mss as usize)
            .min(socket.peer_window as usize);
        if length == 0 {
            return Err("TCP send would block");
        }
        let sequence = socket.send_next;
        let payload = payload[..length].to_vec();
        socket.send_next = socket.send_next.wrapping_add(length as u32);
        socket.pending = Some(PendingSegment {
            sequence,
            acknowledgement: socket.receive_next,
            flags: ACK | PSH,
            payload,
            include_mss: false,
            last_sent: TICK.load(Ordering::Relaxed),
            retransmits: 0,
        });
        socket.pending.clone().unwrap()
    };
    if let Err(error) = transmit_pending(socket, pending.clone()) {
        if error != "ARP request sent; retry after the neighbor responds" {
            let mut socket = socket.lock();
            if socket
                .pending
                .as_ref()
                .is_some_and(|queued| queued.sequence == pending.sequence)
            {
                socket.pending = None;
                socket.send_next = pending.sequence;
            }
            return Err(error);
        }
    }
    Ok(pending.payload.len())
}

pub fn is_established(socket: &Arc<Mutex<TcpSocket>>) -> bool {
    socket.lock().state == TcpState::Established
}

pub fn is_closed(socket: &Arc<Mutex<TcpSocket>>) -> bool {
    let s = socket.lock();
    s.state == TcpState::Closed || s.state == TcpState::Reset
}

pub fn socket_error(socket: &Arc<Mutex<TcpSocket>>) -> Option<&'static str> {
    socket.lock().error
}

pub fn is_receive_eof(socket: &Arc<Mutex<TcpSocket>>) -> bool {
    let s = socket.lock();
    s.receive_eof || s.state == TcpState::Closed || s.state == TcpState::CloseWait
}

pub fn receive_socket(
    socket: &Arc<Mutex<TcpSocket>>,
    output: &mut [u8],
    peek: bool,
) -> Result<usize, &'static str> {
    if output.is_empty() {
        return Ok(0);
    }
    let (length, acknowledgement, address, port, send_ack) = {
        let mut socket = socket.lock();
        if socket.read_shutdown {
            return Ok(0);
        }
        if socket.received.is_empty() {
            if let Some(error) = socket.error {
                return Err(error);
            }
            return if socket.receive_eof || socket.state == TcpState::Closed {
                Ok(0)
            } else {
                Err("TCP receive would block")
            };
        }
        let length = output.len().min(socket.received.len());
        for (index, byte) in socket.received.iter().take(length).enumerate() {
            output[index] = *byte;
        }
        if !peek {
            for _ in 0..length {
                socket.received.pop_front();
            }
        }
        (
            length,
            socket.receive_next,
            socket.remote_address,
            socket.remote_port,
            !peek,
        )
    };
    if send_ack {
        let _ = send_acknowledgement(socket, address, port, acknowledgement);
    }
    Ok(length)
}

pub fn shutdown_socket(socket: &Arc<Mutex<TcpSocket>>, how: u64) -> Result<(), &'static str> {
    if how > 2 {
        return Err("invalid TCP shutdown mode");
    }
    let pending = {
        let mut socket = socket.lock();
        if how == 0 || how == 2 {
            socket.read_shutdown = true;
            socket.received.clear();
        }
        if how == 1 || how == 2 {
            if socket.write_shutdown {
                return Ok(());
            }
            match socket.state {
                TcpState::Established | TcpState::CloseWait => {}
                TcpState::Reset => return Err(socket.error.unwrap_or("TCP connection reset")),
                TcpState::Closed => return Ok(()),
                _ => return Err("TCP socket is not connected"),
            }
            socket.write_shutdown = true;
            if socket.pending.is_some() {
                socket.close_after_pending = true;
                return Ok(());
            }
            Some(queue_fin_locked(&mut socket))
        } else {
            None
        }
    };
    if let Some(pending) = pending {
        transmit_pending(socket, pending)?;
    }
    Ok(())
}

fn queue_fin_locked(socket: &mut TcpSocket) -> PendingSegment {
    let sequence = socket.send_next;
    let state = if socket.state == TcpState::CloseWait {
        TcpState::LastAck
    } else {
        TcpState::FinWait1
    };
    socket.state = state;
    socket.send_next = socket.send_next.wrapping_add(1);
    let pending = PendingSegment {
        sequence,
        acknowledgement: socket.receive_next,
        flags: FIN | ACK,
        payload: Vec::new(),
        include_mss: false,
        last_sent: TICK.load(Ordering::Relaxed),
        retransmits: 0,
    };
    socket.pending = Some(pending.clone());
    pending
}

pub fn close_socket(socket: &Arc<Mutex<TcpSocket>>) {
    let children = {
        let mut socket_state = socket.lock();
        match socket_state.state {
            TcpState::Established | TcpState::CloseWait => {
                drop(socket_state);
                let _ = shutdown_socket(socket, 1);
                return;
            }
            TcpState::FinWait1 | TcpState::FinWait2 | TcpState::LastAck => return,
            TcpState::Listening => {
                let mut children = socket_state.accepted.drain(..).collect::<Vec<_>>();
                children.extend(socket_state.half_open.iter().filter_map(Weak::upgrade));
                socket_state.half_open.clear();
                socket_state.state = TcpState::Closed;
                socket_state.pending = None;
                children
            }
            _ => {
                socket_state.state = TcpState::Closed;
                socket_state.pending = None;
                Vec::new()
            }
        }
    };
    for child in children {
        close_socket(&child);
    }
}

pub fn socket_name(
    socket: &Arc<Mutex<TcpSocket>>,
    peer: bool,
) -> Result<([u8; 4], u16), &'static str> {
    let socket = socket.lock();
    if peer {
        if socket.remote_port == 0 {
            return Err("TCP socket is not connected");
        }
        Ok((socket.remote_address, socket.remote_port))
    } else {
        Ok((socket.local_address, socket.local_port))
    }
}

pub fn poll() {
    let tick = TICK.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    let sockets = SOCKETS.lock().clone();
    for socket in sockets {
        let action = {
            let mut socket_state = socket.lock();
            if socket_state.close_after_pending && socket_state.pending.is_none() {
                socket_state.close_after_pending = false;
                Some(queue_fin_locked(&mut socket_state))
            } else if let Some(pending) = socket_state.pending.as_ref() {
                if tick.wrapping_sub(pending.last_sent) < RETRANSMIT_TICKS {
                    None
                } else if pending.retransmits >= MAX_RETRANSMITS {
                    socket_state.pending = None;
                    socket_state.state = TcpState::Reset;
                    socket_state.error = Some("TCP connection timed out");
                    continue;
                } else {
                    let pending = socket_state.pending.as_mut().unwrap();
                    pending.retransmits += 1;
                    pending.last_sent = tick;
                    Some(pending.clone())
                }
            } else {
                None
            }
        };
        if let Some(pending) = action {
            if let Err(error) = transmit_pending(&socket, pending) {
                if error != "ARP request sent; retry after the neighbor responds" {
                    let mut socket = socket.lock();
                    socket.error = Some(error);
                    socket.state = TcpState::Reset;
                }
            }
        }
    }
    SOCKETS.lock().retain(|socket| !is_terminal(socket));
}

fn transmit_pending(
    socket: &Arc<Mutex<TcpSocket>>,
    pending: PendingSegment,
) -> Result<(), &'static str> {
    let (local_port, remote_address, remote_port, window) = {
        let socket = socket.lock();
        (
            socket.local_port,
            socket.remote_address,
            socket.remote_port,
            (TCP_RECEIVE_CAPACITY - socket.received.len()).min(u16::MAX as usize) as u16,
        )
    };
    let tcp = build_tcp_segment(
        pending.sequence,
        pending.acknowledgement,
        pending.flags,
        window,
        pending.include_mss,
        &pending.payload,
    )?;
    send_tcp_packet(local_port, remote_address, remote_port, &tcp)
}

fn send_acknowledgement(
    socket: &Arc<Mutex<TcpSocket>>,
    remote_address: [u8; 4],
    remote_port: u16,
    acknowledgement: u32,
) -> Result<(), &'static str> {
    let (local_port, sequence, window) = {
        let socket = socket.lock();
        (
            socket.local_port,
            socket.send_next,
            (TCP_RECEIVE_CAPACITY - socket.received.len()).min(u16::MAX as usize) as u16,
        )
    };
    let segment = build_tcp_segment(sequence, acknowledgement, ACK, window, false, &[])?;
    send_tcp_packet(local_port, remote_address, remote_port, &segment)
}

fn send_tcp_packet(
    source_port: u16,
    destination_address: [u8; 4],
    destination_port: u16,
    segment: &[u8],
) -> Result<(), &'static str> {
    let device = super::LINK.lock().clone().ok_or("no network interface")?;
    let (config, next_hop, destination_mac) = {
        let state = super::STATE.lock();
        if !state.config.configured {
            return Err("network interface has no IPv4 address");
        }
        let next_hop = if super::same_subnet(
            destination_address,
            state.config.address,
            state.config.netmask,
        ) {
            destination_address
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
        device.transmit(&super::build_arp_request(
            config.mac,
            config.address,
            next_hop,
        ))?;
        return Err("ARP request sent; retry after the neighbor responds");
    };
    let mut frame = super::build_ipv4_frame(
        config.mac,
        destination_mac,
        config.address,
        destination_address,
        TCP_PROTOCOL,
        segment,
    )?;
    frame[34..36].copy_from_slice(&source_port.to_be_bytes());
    frame[36..38].copy_from_slice(&destination_port.to_be_bytes());
    let checksum = tcp_checksum(config.address, destination_address, &frame[34..]);
    frame[50..52].copy_from_slice(&checksum.to_be_bytes());
    device.transmit(&frame)
}

fn build_tcp_segment(
    sequence: u32,
    acknowledgement: u32,
    flags: u8,
    window: u16,
    include_mss: bool,
    payload: &[u8],
) -> Result<Vec<u8>, &'static str> {
    let header_length = if include_mss { 24 } else { 20 };
    if header_length + payload.len() > super::IPV4_MTU - 20 {
        return Err("TCP payload exceeds the IPv4 MTU");
    }
    let mut segment = alloc::vec![0; header_length + payload.len()];
    segment[4..8].copy_from_slice(&sequence.to_be_bytes());
    segment[8..12].copy_from_slice(&acknowledgement.to_be_bytes());
    segment[12] = ((header_length / 4) as u8) << 4;
    segment[13] = flags;
    segment[14..16].copy_from_slice(&window.to_be_bytes());
    if include_mss {
        segment[20..24].copy_from_slice(&[2, 4, (TCP_MSS >> 8) as u8, TCP_MSS as u8]);
    }
    segment[header_length..].copy_from_slice(payload);
    Ok(segment)
}

fn tcp_checksum(source: [u8; 4], destination: [u8; 4], segment: &[u8]) -> u16 {
    let mut pseudo = alloc::vec![0; 12 + segment.len()];
    pseudo[..4].copy_from_slice(&source);
    pseudo[4..8].copy_from_slice(&destination);
    pseudo[9] = TCP_PROTOCOL;
    pseudo[10..12].copy_from_slice(&(segment.len() as u16).to_be_bytes());
    pseudo[12..].copy_from_slice(segment);
    super::checksum(&pseudo)
}

pub(super) fn receive_packet(source: [u8; 4], destination: [u8; 4], packet: &[u8]) {
    if packet.len() < 20 {
        return;
    }
    let header_length = ((packet[12] >> 4) as usize) * 4;
    if header_length < 20 || header_length > packet.len() {
        return;
    }
    let config = super::config();
    if tcp_checksum(source, destination, packet) != 0 || !config.configured {
        return;
    }
    let source_port = u16::from_be_bytes([packet[0], packet[1]]);
    let destination_port = u16::from_be_bytes([packet[2], packet[3]]);
    let sequence = u32::from_be_bytes(packet[4..8].try_into().unwrap());
    let acknowledgement = u32::from_be_bytes(packet[8..12].try_into().unwrap());
    let flags = packet[13];
    let window = u16::from_be_bytes([packet[14], packet[15]]);
    let payload = &packet[header_length..];
    let mss = parse_mss(&packet[20..header_length]);

    let sockets = SOCKETS.lock().clone();
    let connected = sockets.iter().find(|socket| {
        let socket = socket.lock();
        socket.local_port == destination_port
            && socket.remote_port == source_port
            && socket.remote_address == source
            && socket.state != TcpState::Listening
            && socket.state != TcpState::Created
            && socket.state != TcpState::Bound
    });
    if let Some(socket) = connected {
        if flags & SYN != 0 && socket.lock().state == TcpState::SynReceived {
            if let Some(pending) = socket.lock().pending.clone() {
                let _ = transmit_pending(socket, pending);
            }
            return;
        }
        process_connected_segment(
            socket,
            source,
            source_port,
            sequence,
            acknowledgement,
            flags,
            window,
            payload,
            mss,
        );
        return;
    }

    if flags & SYN != 0 && flags & ACK == 0 {
        if let Some(listener) = sockets.iter().find(|socket| {
            let socket = socket.lock();
            socket.state == TcpState::Listening
                && socket.local_port == destination_port
                && (socket.local_address == [0; 4] || socket.local_address == destination)
        }) {
            handle_incoming_syn(
                listener,
                source,
                source_port,
                destination_port,
                sequence,
                window,
                mss,
            );
            return;
        }
        send_reset(
            source,
            destination_port,
            source_port,
            sequence,
            acknowledgement,
            flags,
            payload.len(),
        );
    }
}

fn handle_incoming_syn(
    listener: &Arc<Mutex<TcpSocket>>,
    remote_address: [u8; 4],
    remote_port: u16,
    local_port: u16,
    sequence: u32,
    window: u16,
    peer_mss: Option<u16>,
) {
    let duplicate = {
        let mut listener_state = listener.lock();
        listener_state
            .half_open
            .retain(|entry| entry.strong_count() != 0);
        listener_state
            .half_open
            .iter()
            .filter_map(Weak::upgrade)
            .find(|child| {
                let child = child.lock();
                child.remote_address == remote_address
                    && child.remote_port == remote_port
                    && child.state == TcpState::SynReceived
            })
    };
    if let Some(child) = duplicate {
        let pending = child.lock().pending.clone();
        if let Some(pending) = pending {
            let _ = transmit_pending(&child, pending);
        }
        return;
    }
    {
        let listener_state = listener.lock();
        if listener_state.accepted.len() + listener_state.half_open.len() >= listener_state.backlog
        {
            drop(listener_state);
            send_reset(remote_address, local_port, remote_port, sequence, 0, SYN, 0);
            return;
        }
    }

    let child = Arc::new(Mutex::new(TcpSocket::new()));
    let pending = {
        let mut child_state = child.lock();
        child_state.local_address = super::config().address;
        child_state.local_port = local_port;
        child_state.remote_address = remote_address;
        child_state.remote_port = remote_port;
        child_state.state = TcpState::SynReceived;
        child_state.receive_next = sequence.wrapping_add(1);
        child_state.peer_window = window;
        child_state.peer_mss = peer_mss.unwrap_or(TCP_MSS).min(TCP_MSS).max(536);
        child_state.listener = Arc::downgrade(listener);
        let initial_sequence = next_initial_sequence();
        child_state.send_unacknowledged = initial_sequence;
        child_state.send_next = initial_sequence.wrapping_add(1);
        child_state.pending = Some(PendingSegment {
            sequence: initial_sequence,
            acknowledgement: child_state.receive_next,
            flags: SYN | ACK,
            payload: Vec::new(),
            include_mss: true,
            last_sent: TICK.load(Ordering::Relaxed),
            retransmits: 0,
        });
        child_state.pending.clone().unwrap()
    };
    {
        let mut sockets = SOCKETS.lock();
        register_socket_locked(&mut sockets, &child);
    }
    listener.lock().half_open.push(Arc::downgrade(&child));
    let _ = transmit_pending(&child, pending);
}

#[allow(clippy::too_many_arguments)]
fn process_connected_segment(
    socket: &Arc<Mutex<TcpSocket>>,
    remote_address: [u8; 4],
    remote_port: u16,
    sequence: u32,
    acknowledgement: u32,
    flags: u8,
    window: u16,
    payload: &[u8],
    peer_mss: Option<u16>,
) {
    let mut send_ack = false;
    let mut accepted_child = false;
    let mut response_reset = false;
    let mut close_pending = None;
    let ack_number = {
        let mut socket = socket.lock();
        if flags & RST != 0 {
            let error = if socket.state == TcpState::SynSent {
                "TCP connection refused"
            } else {
                "TCP connection reset"
            };
            socket.state = TcpState::Reset;
            socket.error = Some(error);
            socket.pending = None;
            return;
        }
        socket.peer_window = window;
        if let Some(mss) = peer_mss {
            socket.peer_mss = mss.min(TCP_MSS).max(536);
        }

        match socket.state {
            TcpState::SynSent => {
                if flags & (SYN | ACK) == (SYN | ACK) && acknowledgement == socket.send_next {
                    socket.receive_next = sequence.wrapping_add(1);
                    socket.send_unacknowledged = acknowledgement;
                    socket.pending = None;
                    socket.state = TcpState::Established;
                    send_ack = true;
                } else if flags & RST != 0 {
                    socket.state = TcpState::Reset;
                    socket.error = Some("TCP connection refused");
                } else {
                    return;
                }
            }
            TcpState::SynReceived => {
                if flags & ACK != 0
                    && acknowledgement == socket.send_next
                    && sequence == socket.receive_next
                {
                    socket.send_unacknowledged = acknowledgement;
                    socket.pending = None;
                    socket.state = TcpState::Established;
                    accepted_child = true;
                } else {
                    return;
                }
            }
            TcpState::Established
            | TcpState::FinWait1
            | TcpState::FinWait2
            | TcpState::CloseWait
            | TcpState::LastAck => {
                if flags & ACK != 0 {
                    if let Some(pending) = socket.pending.as_ref() {
                        let end = pending.sequence.wrapping_add(
                            pending.payload.len() as u32
                                + u32::from(pending.flags & SYN != 0)
                                + u32::from(pending.flags & FIN != 0),
                        );
                        if acknowledgement == end {
                            let fin_was_pending = pending.flags & FIN != 0;
                            socket.send_unacknowledged = acknowledgement;
                            socket.pending = None;
                            if fin_was_pending {
                                socket.state = match socket.state {
                                    TcpState::FinWait1 if socket.receive_eof => TcpState::Closed,
                                    TcpState::FinWait1 => TcpState::FinWait2,
                                    TcpState::LastAck => TcpState::Closed,
                                    state => state,
                                };
                            }
                            if socket.close_after_pending {
                                socket.close_after_pending = false;
                                close_pending = Some(queue_fin_locked(&mut socket));
                            }
                        }
                    }
                }
                let payload_end = sequence.wrapping_add(payload.len() as u32);
                if !payload.is_empty() || flags & FIN != 0 {
                    if sequence == socket.receive_next {
                        let available = TCP_RECEIVE_CAPACITY.saturating_sub(socket.received.len());
                        let accepted = payload.len().min(available);
                        socket.received.extend(payload[..accepted].iter().copied());
                        socket.receive_next = socket.receive_next.wrapping_add(accepted as u32);
                        if accepted == payload.len() && flags & FIN != 0 {
                            socket.receive_next = socket.receive_next.wrapping_add(1);
                            socket.receive_eof = true;
                            socket.state = match socket.state {
                                TcpState::FinWait1 => TcpState::Closed,
                                TcpState::FinWait2 => TcpState::Closed,
                                TcpState::Established => TcpState::CloseWait,
                                state => state,
                            };
                        }
                    }
                    send_ack = true;
                }
                if payload_end == socket.receive_next && flags & FIN != 0 {
                    send_ack = true;
                }
            }
            _ => return,
        }
        socket.receive_next
    };
    if accepted_child {
        if let Some(listener) = socket.lock().listener.upgrade() {
            let mut listener = listener.lock();
            listener.half_open.retain(|entry| {
                entry
                    .upgrade()
                    .is_some_and(|child| !Arc::ptr_eq(&child, socket))
            });
            if listener.accepted.len() < listener.backlog {
                listener.accepted.push_back(Arc::clone(socket));
            } else {
                socket.lock().state = TcpState::Reset;
                response_reset = true;
            }
        }
    }
    if response_reset {
        send_reset(
            remote_address,
            socket.lock().local_port,
            remote_port,
            sequence,
            acknowledgement,
            ACK,
            0,
        );
        return;
    }
    if send_ack {
        let _ = send_acknowledgement(socket, remote_address, remote_port, ack_number);
    }
    if let Some(pending) = close_pending {
        let _ = transmit_pending(socket, pending);
    }
}

fn parse_mss(options: &[u8]) -> Option<u16> {
    let mut offset = 0;
    while offset < options.len() {
        match options[offset] {
            0 => break,
            1 => offset += 1,
            kind => {
                let length = *options.get(offset + 1)? as usize;
                if length < 2 || offset + length > options.len() {
                    return None;
                }
                if kind == 2 && length == 4 {
                    return Some(u16::from_be_bytes([
                        options[offset + 2],
                        options[offset + 3],
                    ]));
                }
                offset += length;
            }
        }
    }
    None
}

fn send_reset(
    remote_address: [u8; 4],
    local_port: u16,
    remote_port: u16,
    incoming_sequence: u32,
    incoming_acknowledgement: u32,
    incoming_flags: u8,
    incoming_payload_length: usize,
) {
    let (reset_sequence, reset_acknowledgement, flags) = if incoming_flags & ACK != 0 {
        (incoming_acknowledgement, 0, RST)
    } else {
        (
            0,
            incoming_sequence
                .wrapping_add(incoming_payload_length as u32)
                .wrapping_add(u32::from(incoming_flags & SYN != 0))
                .wrapping_add(u32::from(incoming_flags & FIN != 0)),
            RST | ACK,
        )
    };
    if let Ok(segment) =
        build_tcp_segment(reset_sequence, reset_acknowledgement, flags, 0, false, &[])
    {
        let _ = send_tcp_packet(local_port, remote_address, remote_port, &segment);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ACK, PendingSegment, SOCKETS, SYN, TCP_MSS, TcpSocket, TcpState, build_tcp_segment,
        close_socket, is_last_descriptor, parse_mss, register_socket_locked, tcp_checksum,
    };
    use alloc::sync::{Arc, Weak};
    use alloc::vec::Vec;
    use spin::Mutex;

    #[test]
    fn tcp_syn_segment_includes_mss_and_valid_pseudoheader_checksum() {
        let source = [192, 168, 42, 2];
        let destination = [192, 168, 42, 1];
        let mut segment = build_tcp_segment(0x1020_3040, 0, SYN, u16::MAX, true, b"").unwrap();
        segment[..2].copy_from_slice(&49152u16.to_be_bytes());
        segment[2..4].copy_from_slice(&80u16.to_be_bytes());
        let checksum = tcp_checksum(source, destination, &segment);
        segment[16..18].copy_from_slice(&checksum.to_be_bytes());

        assert_eq!(segment[12] >> 4, 6);
        assert_eq!(segment[13], SYN);
        assert_eq!(u16::from_be_bytes([segment[22], segment[23]]), TCP_MSS);
        assert_eq!(tcp_checksum(source, destination, &segment), 0);
    }

    #[test]
    fn tcp_mss_parser_ignores_invalid_options() {
        assert_eq!(parse_mss(&[2, 4, 0x05, 0xb4]), Some(TCP_MSS));
        assert_eq!(parse_mss(&[2, 1]), None);
        assert_eq!(parse_mss(&[1, 0]), None);
    }

    #[test]
    fn tcp_ack_flag_is_distinct_from_syn() {
        let segment = build_tcp_segment(1, 2, ACK, 1024, false, b"payload").unwrap();
        assert_eq!(segment[13], ACK);
        assert_eq!(&segment[20..], b"payload");
    }

    #[test]
    fn closing_last_descriptor_keeps_pending_tcp_state_alive() {
        let socket = Arc::new(Mutex::new(TcpSocket::new()));
        {
            let mut socket_state = socket.lock();
            socket_state.state = TcpState::Established;
            socket_state.pending = Some(PendingSegment {
                sequence: 10,
                acknowledgement: 20,
                flags: ACK,
                payload: Vec::from(&b"data"[..]),
                include_mss: false,
                last_sent: 0,
                retransmits: 0,
            });
        }
        {
            let mut sockets = SOCKETS.lock();
            register_socket_locked(&mut sockets, &socket);
        }

        assert!(is_last_descriptor(&socket));
        close_socket(&socket);
        assert!(socket.lock().close_after_pending);

        let weak: Weak<Mutex<TcpSocket>> = Arc::downgrade(&socket);
        drop(socket);
        let retained = weak.upgrade().expect("closing socket remains registered");
        SOCKETS
            .lock()
            .retain(|registered| !Arc::ptr_eq(registered, &retained));
        drop(retained);
        assert!(weak.upgrade().is_none());
    }
}
