//! WMI (Wireless Module Interface) command codec for QCA9377 / ath10k.
//!
//! Reference: drivers/net/wireless/ath/ath10k/wmi.h and wmi.c (Linux kernel).
//! WMI rides on HTC endpoint connected to HTC_SERVICE_WMI_CONTROL.
//!
//! Frame layout: [WMI header 8 bytes][payload]
//!   WMI header: cmd_id(4) | flags(2) | len(2)   (little-endian)

use super::htc::{HtcClient, HtcClientError, HtcError, HtcTransport};

// ── WMI header ───────────────────────────────────────────────────────────────

const WMI_HEADER_LEN: usize = 8;

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
    Timeout,
    HtcError,
}

// ── Encode helpers ────────────────────────────────────────────────────────────

fn write_le32(buf: &mut [u8], offset: usize, v: u32) {
    buf[offset..offset + 4].copy_from_slice(&v.to_le_bytes());
}
fn write_le16(buf: &mut [u8], offset: usize, v: u16) {
    buf[offset..offset + 2].copy_from_slice(&v.to_le_bytes());
}
fn read_le32(buf: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(buf[offset..offset + 4].try_into().unwrap())
}

/// Encode a WMI frame into `out`: [cmd_id | flags=0 | len | payload].
/// Returns the total frame length.
fn encode_wmi(out: &mut [u8], cmd: WmiCmdId, payload: &[u8]) -> Result<usize, WmiError> {
    let total = WMI_HEADER_LEN + payload.len();
    if total > out.len() {
        return Err(WmiError::PayloadTooLarge);
    }
    write_le32(out, 0, cmd as u32);
    write_le16(out, 4, 0); // flags
    write_le16(out, 6, payload.len() as u16);
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
    pub mac_addr: [u8; 6],
    pub phy_capability: u32,
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
            if body.len() < 10 {
                return Err(WmiError::BadEvent);
            }
            let mut mac = [0u8; 6];
            mac.copy_from_slice(&body[0..6]);
            WmiEvent::Ready(WmiReadyEvent {
                mac_addr: mac,
                phy_capability: read_le32(body, 6),
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

// ── WmiClient ────────────────────────────────────────────────────────────────

/// HTC service ID for WMI control.
use super::htc::HTC_SERVICE_WMI_CONTROL;

pub struct WmiClient<T: HtcTransport> {
    htc: HtcClient<T>,
    wmi_ep: u8,
    buf: alloc::vec::Vec<u8>,
}

impl<T: HtcTransport> WmiClient<T> {
    /// Create from an already-ready HTC client that has WMI connected.
    /// `wmi_ep` is the endpoint assigned by `connect_service`.
    pub fn new(htc: HtcClient<T>, wmi_ep: u8) -> Self {
        Self {
            htc,
            wmi_ep,
            buf: alloc::vec![0u8; 2048],
        }
    }

    /// Send a raw WMI command.
    pub fn send<E: core::fmt::Debug>(
        &mut self,
        cmd: WmiCmdId,
        payload: &[u8],
    ) -> Result<(), WmiError>
    where
        T: HtcTransport<Error = E>,
    {
        let mut frame = alloc::vec![0u8; 8 + payload.len() + super::htc::HTC_HEADER_LENGTH];
        let mut wmi_buf = alloc::vec![0u8; WMI_HEADER_LEN + payload.len()];
        let len = encode_wmi(&mut wmi_buf, cmd, payload)?;
        self.htc
            .send_service(HTC_SERVICE_WMI_CONTROL, 0, &wmi_buf[..len], &mut frame)
            .map_err(|_| WmiError::HtcError)
    }

    /// Receive one WMI event (blocking poll via HTC).
    pub fn recv_event<E: core::fmt::Debug>(&mut self) -> Result<WmiEvent, WmiError>
    where
        T: HtcTransport<Error = E>,
    {
        let len = self
            .htc
            .transport_mut()
            .receive(&mut self.buf)
            .map_err(|_| WmiError::HtcError)?;
        parse_wmi_event(&self.buf[..len])
    }

    pub fn into_htc(self) -> HtcClient<T> {
        self.htc
    }
}

// HtcClient needs a transport_mut accessor - add it here as an extension.
// We achieve this via a helper trait rather than modifying htc.rs.
pub trait HtcClientExt {
    type T: HtcTransport;
    fn transport_mut(&mut self) -> &mut Self::T;
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
