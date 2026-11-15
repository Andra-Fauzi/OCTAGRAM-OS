//! HTT (Host-Target Transport) minimal handshake for QCA9377 / ath10k.
//!
//! HTT sits on HTC endpoint HTC_SERVICE_HTT_DATA.
//! We implement only the version handshake needed before data can flow.
//!
//! Reference: drivers/net/wireless/ath/ath10k/htt.h (Linux kernel).

use super::htc::{HtcClient, HtcClientError, HTC_SERVICE_HTT_DATA, HtcTransport};

const HTT_HEADER_LEN: usize = 4; // msg_type(1) + pad(3)

// H2T message types
const HTT_H2T_MSG_TYPE_VERSION_REQ: u8 = 0x00;
const HTT_H2T_MSG_TYPE_RX_RING_CFG: u8 = 0x02;

// T2H message types
const HTT_T2H_MSG_TYPE_VERSION_CONF: u8 = 0x01;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttError {
    HtcError,
    UnexpectedMessage,
    FrameTooShort,
}

#[derive(Debug, Clone, Copy)]
pub struct HttVersion {
    pub major: u8,
    pub minor: u8,
}

/// Send HTT version request and parse the firmware's version response.
pub fn htt_version_handshake<T: HtcTransport>(
    htc: &mut HtcClient<T>,
    htt_ep: u8,
    frame: &mut [u8],
) -> Result<HttVersion, HttError> {
    // Build HTT VERSION_REQ: [msg_type=0, pad, pad, pad]
    let req = [HTT_H2T_MSG_TYPE_VERSION_REQ, 0, 0, 0];
    htc.send_service(HTC_SERVICE_HTT_DATA, 0, &req, frame)
        .map_err(|_| HttError::HtcError)?;

    // Receive VERSION_CONF: [msg_type=1, major, minor, pad]
    let len = htc
        .transport_mut()
        .receive(frame)
        .map_err(|_| HttError::HtcError)?;
    if len < 4 {
        return Err(HttError::FrameTooShort);
    }
    if frame[0] != HTT_T2H_MSG_TYPE_VERSION_CONF {
        return Err(HttError::UnexpectedMessage);
    }
    Ok(HttVersion {
        major: frame[1],
        minor: frame[2],
    })
}

/// Configure HTT RX ring (minimal: ring_id=0, ring_size=8, 1 buffer).
/// For a real data path this would post many scatter-gather buffers.
pub fn htt_rx_ring_setup<T: HtcTransport>(
    htc: &mut HtcClient<T>,
    _htt_ep: u8,
    frame: &mut [u8],
    ring_phys_addr: u32,
    ring_size: u16,
) -> Result<(), HttError> {
    // HTT_H2T_RX_RING_CFG: msg_type, num_rings=1, ...
    // Simplified: 20-byte payload covering the essential ring config.
    let mut payload = [0u8; 20];
    payload[0] = HTT_H2T_MSG_TYPE_RX_RING_CFG;
    payload[1] = 1; // num_rings
    // ring_cfg[0]: paddr(4), size(2), flags(2), fw_idx_shadow_addr(4)...
    payload[4..8].copy_from_slice(&ring_phys_addr.to_le_bytes());
    payload[8..10].copy_from_slice(&ring_size.to_le_bytes());
    htc.send_service(HTC_SERVICE_HTT_DATA, 0, &payload, frame)
        .map_err(|_| HttError::HtcError)
}

// HtcClient transport accessor (implemented here via the extension pattern
// that wmi.rs also uses; avoids touching htc.rs again).
use super::htc::HtcClientTransportExt;
