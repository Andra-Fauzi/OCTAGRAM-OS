//! WMI (Wireless Module Interface) command codec for QCA9377 / ath10k.
//!
//! Reference: drivers/net/wireless/ath/ath10k/wmi.h and wmi.c (Linux kernel).
//! WMI rides on HTC endpoint connected to HTC_SERVICE_WMI_CONTROL.
//!
//! Frame layout: [WMI header 4 bytes][payload]
//!   WMI header: cmd_id(4) masked with `WMI_CMD_HDR_CMD_ID_MASK`
//!
//! ath10k's `struct wmi_cmd_hdr` is a single little-endian word: the command or
//! event id in the low 24 bits, the "plt priv" field in the upper byte. The WMI
//! message itself is carried inside an HTC data message for the endpoint the
//! target assigned to the WMI control service.

use super::ce::{RegisterIo, RxCe, TxCe};
use super::htc::{
    HTC_FLAG_NEED_CREDIT_UPDATE, HTC_HEADER_LENGTH, HtcCreditReport, HtcCredits, HtcDataError,
    data_message, parse_credit_reports, write_data_header,
};

// ── WMI header ───────────────────────────────────────────────────────────────

const WMI_HEADER_LEN: usize = 4;
/// `WMI_CMD_HDR_CMD_ID_MASK` (`wmi.h`).
const WMI_CMD_HDR_CMD_ID_MASK: u32 = 0x00ff_ffff;

/// WMI command IDs (10.2 firmware / QCA9377).
#[allow(dead_code)]
#[repr(u32)]
#[derive(Clone, Copy)]
pub enum WmiCmdId {
    Init                  = 0x0001,
    StartScan             = 0x0007,
    StopScan              = 0x0008,
    ScanChannelList       = 0x0009,
    VdevCreate            = 0x0023,
    VdevDelete            = 0x0024,
    VdevStart             = 0x0025,
    VdevRestart           = 0x0026,
    VdevUp                = 0x0027,
    VdevStop              = 0x0028,
    VdevDown              = 0x0029,
    PeerCreate            = 0x0030,
    PeerDelete            = 0x0031,
    PeerFlushTids         = 0x0032,
    PeerSetParam          = 0x0033,
    StaPowersaveMode      = 0x004c,
    PreambleType          = 0x005a,
    SetChannel            = 0x0088,
    ConnectHtcService     = 0x0100,
    PeerAssoc             = 0x0034,
    VdevSetParam          = 0x004a,
    PdevSetParam          = 0x004b,
}
/// WMI event IDs.
#[allow(dead_code)]
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WmiEventId {
    Ready                 = 0x1001,
    Scan                  = 0x1002,
    Started               = 0x100b,
    Stopped               = 0x100c,
    PeerSta               = 0x100d,
    VdevStartResp         = 0x1016,
    Connected             = 0x101b,
    Disconnected          = 0x101c,
    RegDomain             = 0x101d,
    ChanInfo              = 0x101e,
    WowWakeupHost         = 0x1023,
    RttMeasReportV3       = 0x1030,
    Unknown,
}

impl WmiEventId {
    fn from_u32(v: u32) -> Self {
        match v {
            0x1001 => Self::Ready,
            0x1002 => Self::Scan,
            0x100b => Self::Started,
            0x100c => Self::Stopped,
            0x100d => Self::PeerSta,
            0x1016 => Self::VdevStartResp,
            0x101b => Self::Connected,
            0x101c => Self::Disconnected,
            0x101d => Self::RegDomain,
            0x101e => Self::ChanInfo,
            _      => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WmiError {
    PayloadTooLarge,
    FrameTooShort,
    BadEvent,
    /// The HTC message does not belong to the endpoint this client owns.
    BadEndpoint,
    /// The endpoint ran out of HTC transmit credits.
    NoCredits,
    Timeout,
    HtcError,
}

// ── Encode helpers ────────────────────────────────────────────────────────────

fn write_le32(buf: &mut [u8], offset: usize, v: u32) {
    buf[offset..offset + 4].copy_from_slice(&v.to_le_bytes());
}
fn read_le32(buf: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(buf[offset..offset + 4].try_into().unwrap())
}

/// Encode a WMI message into `out`: [cmd_id | payload].
/// Returns the total message length.
fn encode_wmi(out: &mut [u8], cmd: WmiCmdId, payload: &[u8]) -> Result<usize, WmiError> {
    let total = WMI_HEADER_LEN + payload.len();
    if total > out.len() {
        return Err(WmiError::PayloadTooLarge);
    }
    write_le32(out, 0, (cmd as u32) & WMI_CMD_HDR_CMD_ID_MASK);
    out[WMI_HEADER_LEN..total].copy_from_slice(payload);
    Ok(total)
}

// ── WMI payload builders ─────────────────────────────────────────────────────

/// WMI_INIT payload (minimal: resource config + mem chunks count = 0).
/// Layout follows ath10k wmi_init_cmd / wmi_resource_config_10x.
/// We fill only the fields needed for firmware to start; unused fields → 0.
pub fn wmi_init_payload() -> [u8; 60] {
    let mut p = [0u8; 60];
    // resource_config (52 bytes) followed by mem_chunks_count (4 bytes = 0).
    // num_vdevs=1, num_peers=2, num_offload_peers=0,
    // num_offload_reorder_buffs=0, num_peer_keys=1,
    // num_tids=2, ast_skid_limit=16, tx_chain_mask=1, rx_chain_mask=1,
    // rx_timeout_pri_vi=100, rx_timeout_pri_vo=40,
    // rx_timeout_pri_nc=150, rx_decap_mode=0, scan_max_pending_req=4,
    // bmiss_offload_max_vdev=1, roam_offload_max_vdev=1,
    // roam_offload_max_ap=2, num_mcast_groups=0, num_mcast_table_elems=0,
    // mcast2ucast_mode=0, tx_dbg_log_size=1024, num_wds_entries=32,
    // dma_burst_size=0, mac_aggr_delim=0, rx_skip_defrag=0,
    // num_pre_alloc_ctx=0, enable_pcss=0, num_msdu_desc=128,
    // max_frag_entries=8.
    let mut o = 0usize;
    let write = |buf: &mut [u8], off: &mut usize, v: u32| {
        buf[*off..*off + 4].copy_from_slice(&v.to_le_bytes());
        *off += 4;
    };
    write(&mut p, &mut o, 1);    // num_vdevs
    write(&mut p, &mut o, 2);    // num_peers
    write(&mut p, &mut o, 0);    // num_offload_peers
    write(&mut p, &mut o, 0);    // num_offload_reorder_buffs
    write(&mut p, &mut o, 1);    // num_peer_keys
    write(&mut p, &mut o, 2);    // num_tids
    write(&mut p, &mut o, 16);   // ast_skid_limit
    write(&mut p, &mut o, 1);    // tx_chain_mask
    write(&mut p, &mut o, 1);    // rx_chain_mask
    write(&mut p, &mut o, 100);  // rx_timeout_pri_vi
    write(&mut p, &mut o, 40);   // rx_timeout_pri_vo
    write(&mut p, &mut o, 150);  // rx_timeout_pri_nc
    write(&mut p, &mut o, 0);    // rx_decap_mode (raw)
    // mem_chunks_count = 0 (last 4 bytes, already 0)
    p
}

/// WMI_VDEV_CREATE payload.
/// vdev_id: 0, vdev_type: 2 (STA), vdev_subtype: 0, mac_addr: 6 bytes.
pub fn wmi_vdev_create(vdev_id: u32, mac: [u8; 6]) -> [u8; 20] {
    let mut p = [0u8; 20];
    write_le32(&mut p, 0, vdev_id);
    write_le32(&mut p, 4, 2); // VDEV_TYPE_STA
    write_le32(&mut p, 8, 0); // subtype
    p[12..18].copy_from_slice(&mac);
    p
}

/// WMI_VDEV_START_REQUEST payload (simplified, no channel flags).
/// freq in MHz, channel_mode: 1=11g, htmode: 0=no-HT.
pub fn wmi_vdev_start(
    vdev_id: u32,
    freq_mhz: u32,
    ssid: &[u8],
    bssid: [u8; 6],
    flags: u32,
) -> alloc::vec::Vec<u8> {
    // Header: vdev_id(4), requestor_id(4), chan struct(36),
    //         bcn_intval(4), dtim_period(4), flags(4),
    //         ssid_len(4), ssid(32), hidden_ssid(4), pmf_enabled(4)
    let mut p = alloc::vec![0u8; 4 + 4 + 36 + 4 + 4 + 4 + 4 + 32 + 4 + 4];
    let mut o = 0;
    write_le32(&mut p, o, vdev_id); o += 4;
    write_le32(&mut p, o, 0); o += 4; // requestor_id
    // Channel struct (36 bytes): mhz(4), band_center_freq1(4), ..., info(4), reg_info_1(4)...
    write_le32(&mut p, o, freq_mhz); o += 4;
    write_le32(&mut p, o, freq_mhz); o += 4; // band_center_freq1
    write_le32(&mut p, o, 0); o += 4;         // band_center_freq2
    write_le32(&mut p, o, 0x0000_0100); o += 4; // info: MODE_11G
    write_le32(&mut p, o, 0); o += 4; // reg_info_1
    write_le32(&mut p, o, 0); o += 4; // reg_info_2
    write_le32(&mut p, o, 0); o += 4; // phy_mode
    write_le32(&mut p, o, 0); o += 4; // max_power
    write_le32(&mut p, o, 0); o += 4; // min_power
    // bcn_intval, dtim_period, flags
    write_le32(&mut p, o, 100); o += 4; // beacon interval
    write_le32(&mut p, o, 1); o += 4;   // dtim_period
    write_le32(&mut p, o, flags); o += 4;
    // ssid
    let ssid_len = ssid.len().min(32) as u32;
    write_le32(&mut p, o, ssid_len); o += 4;
    p[o..o + ssid_len as usize].copy_from_slice(&ssid[..ssid_len as usize]); o += 32;
    write_le32(&mut p, o, 0); o += 4; // hidden_ssid
    write_le32(&mut p, o, 0); // pmf_enabled
    // bssid embedded in flags area - for STA this is not needed at start
    let _ = bssid;
    p
}

/// WMI_PEER_CREATE payload.
pub fn wmi_peer_create(vdev_id: u32, mac: [u8; 6], peer_type: u32) -> [u8; 16] {
    let mut p = [0u8; 16];
    write_le32(&mut p, 0, vdev_id);
    p[4..10].copy_from_slice(&mac);
    write_le32(&mut p, 12, peer_type); // 1 = STA
    p
}

/// WMI_STA_POWERSAVE_MODE.
pub fn wmi_sta_powersave_mode(vdev_id: u32, mode: u32) -> [u8; 8] {
    let mut p = [0u8; 8];
    write_le32(&mut p, 0, vdev_id);
    write_le32(&mut p, 4, mode); // 0 = disabled, 1 = enabled
    p
}

/// WMI scan request (passive scan on a single channel for association).
/// For a full scan this would include channel list; we do a simple active scan.
pub fn wmi_start_scan(vdev_id: u32, scan_id: u32, freq_mhz: u32) -> alloc::vec::Vec<u8> {
    // Minimal wmi_start_scan_cmd: scan_id, scan_req_id, vdev_id, scan_priority,
    // notify_scan_events, dwell_time_active, dwell_time_passive,
    // min_rest_time, max_rest_time, repeat_probe_time,
    // probe_spacing_time, idle_time, max_scan_time, probe_delay,
    // scan_ctrl_flags, burst_duration, num_chan, num_bssids, num_ssids,
    // ie_len, num_probes, ...
    let mut p = alloc::vec![0u8; 24 * 4 + 4]; // header fields
    let mut o = 0;
    write_le32(&mut p, o, scan_id); o += 4;
    write_le32(&mut p, o, 1); o += 4;        // scan_req_id
    write_le32(&mut p, o, vdev_id); o += 4;
    write_le32(&mut p, o, 1); o += 4;        // scan_priority = LOW
    write_le32(&mut p, o, 0x7f); o += 4;     // notify all events
    write_le32(&mut p, o, 50); o += 4;       // dwell_time_active ms
    write_le32(&mut p, o, 50); o += 4;       // dwell_time_passive ms
    write_le32(&mut p, o, 50); o += 4;       // min_rest_time
    write_le32(&mut p, o, 500); o += 4;      // max_rest_time
    write_le32(&mut p, o, 0); o += 4;        // repeat_probe_time
    write_le32(&mut p, o, 0); o += 4;        // probe_spacing_time
    write_le32(&mut p, o, 0); o += 4;        // idle_time
    write_le32(&mut p, o, 20000); o += 4;    // max_scan_time
    write_le32(&mut p, o, 5); o += 4;        // probe_delay
    write_le32(&mut p, o, 0x0001); o += 4;   // scan_ctrl_flags: active
    write_le32(&mut p, o, 0); o += 4;        // burst_duration
    write_le32(&mut p, o, 1); o += 4;        // num_chan
    write_le32(&mut p, o, 0); o += 4;        // num_bssids (any)
    write_le32(&mut p, o, 0); o += 4;        // num_ssids (wildcard)
    write_le32(&mut p, o, 0); o += 4;        // ie_len
    write_le32(&mut p, o, 1); o += 4;        // num_probes
    // channel list (1 entry)
    write_le32(&mut p, o, freq_mhz);
    p
}

// ── Parsed WMI events ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct WmiReadyEvent {
    /// `struct wmi_ready_event`: sw_version, abi_version, mac_addr[6], status.
    pub sw_version: u32,
    pub abi_version: u32,
    pub mac_addr: [u8; 6],
    pub status: u32,
}

#[derive(Debug, Clone)]
pub struct WmiScanEvent {
    pub event_type: u32, // 1=started,2=completed,4=bss_chan,...
    pub freq_mhz: u32,
    pub scan_id: u32,
}

#[derive(Debug, Clone)]
pub struct WmiConnectedEvent {
    pub vdev_id: u32,
    pub bssid: [u8; 6],
    pub channel: u32,
}

#[derive(Debug, Clone)]
pub enum WmiEvent {
    Ready(WmiReadyEvent),
    Scan(WmiScanEvent),
    Connected(WmiConnectedEvent),
    Disconnected { vdev_id: u32, reason: u32 },
    VdevStartResp { vdev_id: u32, status: u32 },
    Other { event_id: u32 },
}

pub fn parse_wmi_event(payload: &[u8]) -> Result<WmiEvent, WmiError> {
    if payload.len() < WMI_HEADER_LEN {
        return Err(WmiError::FrameTooShort);
    }
    let event_id = read_le32(payload, 0);
    let body = &payload[WMI_HEADER_LEN..];
    Ok(match WmiEventId::from_u32(event_id) {
        WmiEventId::Ready => {
            // The MAC address follows the two version words, not the event id.
            if body.len() < 18 {
                return Err(WmiError::BadEvent);
            }
            let mut mac = [0u8; 6];
            mac.copy_from_slice(&body[8..14]);
            WmiEvent::Ready(WmiReadyEvent {
                sw_version: read_le32(body, 0),
                abi_version: read_le32(body, 4),
                mac_addr: mac,
                status: read_le32(body, 14),
            })
        }
        WmiEventId::Scan => {
            if body.len() < 20 {
                return Err(WmiError::BadEvent);
            }
            WmiEvent::Scan(WmiScanEvent {
                event_type: read_le32(body, 0),
                freq_mhz: read_le32(body, 4),
                scan_id: read_le32(body, 8),
            })
        }
        WmiEventId::Connected => {
            if body.len() < 16 {
                return Err(WmiError::BadEvent);
            }
            let mut bssid = [0u8; 6];
            bssid.copy_from_slice(&body[4..10]);
            WmiEvent::Connected(WmiConnectedEvent {
                vdev_id: read_le32(body, 0),
                bssid,
                channel: read_le32(body, 10),
            })
        }
        WmiEventId::Disconnected => {
            if body.len() < 8 {
                return Err(WmiError::BadEvent);
            }
            WmiEvent::Disconnected {
                vdev_id: read_le32(body, 0),
                reason: read_le32(body, 4),
            }
        }
        WmiEventId::VdevStartResp => {
            if body.len() < 8 {
                return Err(WmiError::BadEvent);
            }
            WmiEvent::VdevStartResp {
                vdev_id: read_le32(body, 0),
                status: read_le32(body, 4),
            }
        }
        _ => WmiEvent::Other { event_id },
    })
}

// ── WMI over HTC ─────────────────────────────────────────────────────────────

/// WMI control endpoint plus the HTC framing it needs.
///
/// WMI does not ride the control pipe: HTC binds the WMI service to its own copy
/// engine (TX=CE3, RX=CE2 on QCA9377) and to the endpoint id returned by
/// `connect_service`. Commands therefore go out as HTC data messages for that
/// endpoint on CE3, and events come back the same way on CE2.
pub struct WmiEndpoint {
    endpoint: u8,
    max_message_size: u16,
    /// Transmit credits refunded by the target's credit reports.
    credits: HtcCredits,
    sequence: u8,
}

impl WmiEndpoint {
    /// `endpoint` and `max_message_size` come from the HTC connect response;
    /// `credits` from the HTC READY message.
    pub const fn new(endpoint: u8, max_message_size: u16, credits: HtcCredits) -> Self {
        Self {
            endpoint,
            max_message_size,
            credits,
            sequence: 0,
        }
    }

    pub const fn endpoint(&self) -> u8 {
        self.endpoint
    }

    /// Credits still available to this endpoint, for diagnostics.
    pub const fn credits(&self) -> HtcCredits {
        self.credits
    }

    /// Frame a WMI command as an HTC data message and send it on the WMI TX pipe.
    ///
    /// # Safety
    /// The WMI CE pipe must be configured, PCI bus mastering must be enabled and
    /// the target must have finished the HTC handshake for the WMI service.
    pub unsafe fn send<R: RegisterIo>(
        &mut self,
        tx: &mut TxCe,
        registers: &mut R,
        cmd: WmiCmdId,
        payload: &[u8],
        frame: &mut [u8],
    ) -> Result<(), WmiError> {
        if WMI_HEADER_LEN + payload.len() > usize::from(self.max_message_size) {
            return Err(WmiError::PayloadTooLarge);
        }
        // Ask for a credit report with every frame while credits are accounted.
        let flags = if self.credits.is_enabled() {
            HTC_FLAG_NEED_CREDIT_UPDATE
        } else {
            0
        };
        let total = encode_htc_wmi_frame(frame, self.endpoint, flags, self.sequence, cmd, payload)?;
        self.credits.take(total).map_err(|_| WmiError::NoCredits)?;
        self.sequence = self.sequence.wrapping_add(1);
        unsafe { tx.send(registers, &frame[..total]) }.map_err(|_| WmiError::HtcError)
    }

    /// Receive one WMI event from the WMI RX pipe.
    ///
    /// # Safety
    /// Same requirements as [`Self::send`].
    pub unsafe fn receive<R: RegisterIo>(
        &mut self,
        rx: &mut RxCe,
        registers: &mut R,
        frame: &mut [u8],
    ) -> Result<WmiEvent, WmiError> {
        let length = unsafe { rx.receive(registers, frame) }.map_err(|_| WmiError::HtcError)?;
        let bytes = frame.get(..length).ok_or(WmiError::FrameTooShort)?;
        decode_htc_wmi_event(bytes, self.endpoint, &mut self.credits)
    }
}

/// Wrap a WMI message in an HTC data-message header.
/// Layout: [endpoint | flags | len(2) | control(4)][wmi header | payload].
fn encode_htc_wmi_frame(
    frame: &mut [u8],
    endpoint: u8,
    flags: u8,
    sequence: u8,
    cmd: WmiCmdId,
    payload: &[u8],
) -> Result<usize, WmiError> {
    let total = HTC_HEADER_LENGTH + WMI_HEADER_LEN + payload.len();
    if total > frame.len() {
        return Err(WmiError::PayloadTooLarge);
    }
    let message = encode_wmi(&mut frame[HTC_HEADER_LENGTH..], cmd, payload)?;
    let payload_len = u16::try_from(message).map_err(|_| WmiError::PayloadTooLarge)?;
    write_data_header(frame, endpoint, flags, sequence, payload_len)
        .map_err(|_| WmiError::FrameTooShort)?;
    Ok(HTC_HEADER_LENGTH + message)
}

/// Parse an HTC data message that carries a WMI event for `endpoint`.
fn decode_htc_wmi_event(
    bytes: &[u8],
    endpoint: u8,
    credits: &mut HtcCredits,
) -> Result<WmiEvent, WmiError> {
    // The helper checks the endpoint and separates the credit trailer, so neither
    // can reach the WMI parser.
    let message = data_message(bytes, endpoint).map_err(|error| match error {
        HtcDataError::WrongEndpoint => WmiError::BadEndpoint,
        HtcDataError::FrameTooShort | HtcDataError::BadTrailer => WmiError::FrameTooShort,
    })?;
    let mut reports = [HtcCreditReport {
        endpoint: 0,
        credits: 0,
    }; 8];
    let count = parse_credit_reports(message.trailer, &mut reports);
    for report in &reports[..count] {
        if report.endpoint == endpoint {
            credits.report(report.credits);
        }
    }
    parse_wmi_event(message.payload)
}

#[cfg(test)]
mod tests {
    use super::super::htc::{
        HTC_FLAG_NEED_CREDIT_UPDATE, HTC_FLAG_RECV_TRAILER, HtcCredits,
    };
    use super::{
        HTC_HEADER_LENGTH, WMI_HEADER_LEN, WmiCmdId, WmiError, WmiEvent, WmiEventId,
        decode_htc_wmi_event, encode_htc_wmi_frame, parse_wmi_event, read_le32,
    };

    /// A READY event body: sw_version, abi_version, mac_addr[6], status.
    fn ready_body() -> [u8; 18] {
        let mut body = [0u8; 18];
        body[0..4].copy_from_slice(&0x0102_0304u32.to_le_bytes());
        body[4..8].copy_from_slice(&5u32.to_le_bytes());
        body[8..14].copy_from_slice(&[0x02, 0x11, 0x22, 0x33, 0x44, 0x55]);
        body
    }

    #[test]
    fn wmi_message_header_is_a_single_masked_command_id() {
        let mut frame = [0u8; 64];
        let total = encode_htc_wmi_frame(
            &mut frame,
            3,
            HTC_FLAG_NEED_CREDIT_UPDATE,
            7,
            WmiCmdId::VdevCreate,
            &[0xaa, 0xbb],
        )
        .expect("frame fits");

        assert_eq!(total, HTC_HEADER_LENGTH + WMI_HEADER_LEN + 2);
        assert_eq!(frame[0], 3, "HTC endpoint id");
        assert_eq!(
            frame[1], HTC_FLAG_NEED_CREDIT_UPDATE,
            "the frame asks the target for a credit report"
        );
        assert_eq!(u16::from_le_bytes([frame[2], frame[3]]), 6, "HTC payload length");
        assert_eq!(frame[4], 7, "HTC sequence");
        assert_eq!(
            read_le32(&frame, HTC_HEADER_LENGTH),
            WmiCmdId::VdevCreate as u32,
            "WMI command id sits right after the HTC header"
        );
        assert_eq!(&frame[HTC_HEADER_LENGTH + WMI_HEADER_LEN..total], &[0xaa, 0xbb]);
    }

    #[test]
    fn ready_event_reads_the_mac_after_the_two_version_words() {
        let mut message = [0u8; 4 + 18];
        message[0..4].copy_from_slice(&(WmiEventId::Ready as u32).to_le_bytes());
        message[4..].copy_from_slice(&ready_body());

        match parse_wmi_event(&message).expect("ready parses") {
            WmiEvent::Ready(ready) => {
                assert_eq!(ready.sw_version, 0x0102_0304);
                assert_eq!(ready.abi_version, 5);
                assert_eq!(ready.mac_addr, [0x02, 0x11, 0x22, 0x33, 0x44, 0x55]);
                assert_eq!(ready.status, 0);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn credit_trailer_is_stripped_before_the_event_is_parsed() {
        let body = ready_body();
        let wmi_len = 4 + body.len();
        // Trailer: one credits record (id 1, four-byte body) reporting five
        // credits for endpoint 3, closed by the total trailer length (9).
        let trailer = [0x01u8, 0x04, 0x00, 0x00, 0x03, 0x05, 0x00, 0x00, 0x09];
        let htc_len = wmi_len + trailer.len();
        let mut frame = [0u8; HTC_HEADER_LENGTH + 32];
        frame[0] = 3;
        frame[1] = HTC_FLAG_RECV_TRAILER;
        frame[2..4].copy_from_slice(&(htc_len as u16).to_le_bytes());
        frame[HTC_HEADER_LENGTH..HTC_HEADER_LENGTH + 4]
            .copy_from_slice(&(WmiEventId::Ready as u32).to_le_bytes());
        frame[HTC_HEADER_LENGTH + 4..HTC_HEADER_LENGTH + wmi_len].copy_from_slice(&body);
        frame[HTC_HEADER_LENGTH + wmi_len..HTC_HEADER_LENGTH + htc_len].copy_from_slice(&trailer);

        let bytes = &frame[..HTC_HEADER_LENGTH + htc_len];
        let mut credits = HtcCredits::new(2048, 0);
        match decode_htc_wmi_event(bytes, 3, &mut credits)
            .expect("ready parses through the trailer")
        {
            WmiEvent::Ready(ready) => {
                assert_eq!(ready.mac_addr, [0x02, 0x11, 0x22, 0x33, 0x44, 0x55]);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert_eq!(
            credits.available(),
            5,
            "the credit report in the trailer is applied"
        );
        assert_eq!(
            decode_htc_wmi_event(bytes, 2, &mut credits).expect_err("wrong endpoint"),
            WmiError::BadEndpoint
        );
    }
}

// ── Additional WMI payload builders ──────────────────────────────────────────

/// WMI_VDEV_UP payload (STA mode, association ID, BSSID).
/// Sent after VDEV_START_RESP and successful association.
pub fn wmi_vdev_up(vdev_id: u32, assoc_id: u32, bssid: [u8; 6]) -> [u8; 20] {
    let mut p = [0u8; 20];
    write_le32(&mut p, 0, vdev_id);
    write_le32(&mut p, 4, assoc_id);
    p[8..14].copy_from_slice(&bssid);
    p
}

/// WMI_PEER_ASSOC payload (simplified, 11g only, no HT/VHT).
/// Tells firmware to associate a peer (the AP) on the given vdev.
pub fn wmi_peer_assoc(
    vdev_id: u32,
    bssid: [u8; 6],
    assoc_id: u16,
    capinfo: u16,
    listen_interval: u16,
    rates: &[u8], // supported rates (up to 8)
) -> alloc::vec::Vec<u8> {
    // peer_assoc_complete_cmd layout (simplified 10.2):
    // vdev_id(4), new_assoc(4), assoc_id(4), flags(4),
    // peer_mac(6+2pad), peer_rate_caps(4), peer_caps(4),
    // peer_listen_intval(4), peer_ht_caps(4), peer_max_mpdu(4),
    // peer_mpdu_density(4), peer_vht_caps(4), peer_phymode(4),
    // peer_legacy_rates_num(4), peer_legacy_rates([12]),
    // peer_ht_rates_num(4), peer_ht_rates([128])
    let mut p = alloc::vec![0u8; 4+4+4+4 + 8 + 4+4+4+4+4+4+4+4 + 4+12 + 4+128];
    let mut o = 0usize;
    write_le32(&mut p, o, vdev_id); o += 4;
    write_le32(&mut p, o, 1); o += 4;              // new_assoc=1
    write_le32(&mut p, o, assoc_id as u32); o += 4;
    write_le32(&mut p, o, 0); o += 4;              // flags
    p[o..o+6].copy_from_slice(&bssid); o += 8;     // mac + 2 pad
    write_le32(&mut p, o, 0); o += 4;              // peer_rate_caps
    write_le32(&mut p, o, capinfo as u32); o += 4;
    write_le32(&mut p, o, listen_interval as u32); o += 4;
    write_le32(&mut p, o, 0); o += 4;              // peer_ht_caps
    write_le32(&mut p, o, 0x40000); o += 4;        // peer_max_mpdu (262143)
    write_le32(&mut p, o, 0); o += 4;              // peer_mpdu_density
    write_le32(&mut p, o, 0); o += 4;              // peer_vht_caps
    write_le32(&mut p, o, 1); o += 4;              // peer_phymode = 11g
    // legacy rates
    let nrates = rates.len().min(12);
    write_le32(&mut p, o, nrates as u32); o += 4;
    p[o..o+nrates].copy_from_slice(&rates[..nrates]);
    // rest zero (ht_rates etc.)
    p
}

/// WMI_VDEV_SET_PARAM — generic vdev parameter.
pub fn wmi_vdev_set_param(vdev_id: u32, param_id: u32, param_value: u32) -> [u8; 12] {
    let mut p = [0u8; 12];
    write_le32(&mut p, 0, vdev_id);
    write_le32(&mut p, 4, param_id);
    write_le32(&mut p, 8, param_value);
    p
}

/// WMI_PDEV_SET_PARAM — global pdev parameter.
pub fn wmi_pdev_set_param(param_id: u32, param_value: u32) -> [u8; 8] {
    let mut p = [0u8; 8];
    write_le32(&mut p, 0, param_id);
    write_le32(&mut p, 4, param_value);
    p
}

/// Parse a WmiEvent from an rx buffer, returning (event, body) if known.
/// Non-fatal: unknown events return WmiEvent::Other.
pub fn try_parse_wmi_event(buf: &[u8]) -> Option<WmiEvent> {
    parse_wmi_event(buf).ok()
}
