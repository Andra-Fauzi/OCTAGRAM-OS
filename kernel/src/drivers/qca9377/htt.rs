//! HTT (Host-Target Transport) for QCA9377 / ath10k: version handshake and the
//! RX buffer ring configuration.
//!
//! HTT is an HTC service (`ATH10K_HTC_SVC_ID_HTT_DATA_MSG`, group 3 index 0,
//! i.e. 0x0300), so every HTT message travels inside an HTC data message for the
//! endpoint the target assigned to that service. A bare HTT frame written to the
//! copy engine is never seen by the firmware. On QCA9377 the HTT pair is
//! TX=CE4, RX=CE5.
//!
//! Reference: drivers/net/wireless/ath/ath10k/htt.h, htt_tx.c, htt.c.

use super::ce::{DmaPage, RegisterIo, RxCe, TxCe};
use super::htc::{HtcDataError, data_message_payload, encode_data_message};
use alloc::vec::Vec;

// H2T message types (`enum htt_h2t_msg_type`).
pub const HTT_H2T_MSG_TYPE_VERSION_REQ: u8 = 0x00;
pub const HTT_H2T_MSG_TYPE_RX_RING_CFG: u8 = 0x02;

// T2H message types (`enum htt_t2h_msg_type`).
pub const HTT_T2H_MSG_TYPE_VERSION_CONF: u8 = 0x00;

/// `ath10k_htt_h2t_ver_req_msg` sends a one-byte `htt_cmd_hdr` padded to a word.
pub const HTT_VERSION_REQ_LEN: usize = 4;
/// `struct htt_ver_resp`: `msg_type(1)` then `minor`, `major`, `rsvd0`.
pub const HTT_VERSION_CONF_LEN: usize = 4;

/// `HTT_RX_BUF_SIZE`: each RX buffer the firmware may DMA an MSDU into.
pub const HTT_RX_BUF_SIZE: usize = 2048;
/// Number of RX buffers described to the firmware.
///
/// ath10k uses 2048 coherent buffers; here every buffer is a whole 4 KiB DMA
/// frame, so the ring is smaller to keep the low DMA pool usable.
pub const HTT_RX_RING_SIZE: usize = 64;
/// Every `HTT_RX_RING_FLAGS_*` bit, as `ath10k_htt_send_rx_ring_cfg` sets them.
pub const HTT_RX_RING_FLAGS: u16 = 0xffff;
/// `htt_cmd_hdr` + `htt_rx_ring_setup_32` + one `htt_rx_ring_setup_ring32`.
const HTT_RX_RING_CFG_LEN: usize = 1 + 3 + 36;
const HTT_VERSION_ATTEMPTS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttError {
    /// The HTC data message could not be framed, sent or parsed.
    HtcError,
    /// The target answered with a message other than VERSION_CONF.
    UnexpectedMessage,
    FrameTooShort,
    /// No version confirmation arrived within the polling budget.
    Timeout,
    /// The RX ring size is not a power of two / does not fit a DMA page.
    InvalidRing,
    /// The RX ring buffers could not be allocated.
    OutOfMemory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttVersion {
    pub major: u8,
    pub minor: u8,
}

/// The HTT endpoint plus the HTC framing it needs.
pub struct HttEndpoint {
    endpoint: u8,
    max_message_size: u16,
    sequence: u8,
}

impl HttEndpoint {
    /// `endpoint` and `max_message_size` come from the HTC connect response for
    /// `HTC_SERVICE_HTT_DATA`.
    pub const fn new(endpoint: u8, max_message_size: u16) -> Self {
        Self {
            endpoint,
            max_message_size,
            sequence: 0,
        }
    }

    pub const fn endpoint(&self) -> u8 {
        self.endpoint
    }

    /// Send one HTT message as an HTC data message on the HTT TX pipe.
    ///
    /// # Safety
    /// The HTT CE pipe must be configured, PCI bus mastering must be enabled and
    /// the target must have finished the HTC handshake for the HTT service.
    pub unsafe fn send<R: RegisterIo>(
        &mut self,
        tx: &mut TxCe,
        registers: &mut R,
        message: &[u8],
        frame: &mut [u8],
    ) -> Result<(), HttError> {
        if message.len() > usize::from(self.max_message_size) {
            return Err(HttError::HtcError);
        }
        let total = encode_data_message(frame, self.endpoint, self.sequence, message)
            .map_err(|_| HttError::HtcError)?;
        self.sequence = self.sequence.wrapping_add(1);
        unsafe { tx.send(registers, &frame[..total]) }.map_err(|_| HttError::HtcError)
    }

    /// Receive one HTT message from the HTT RX pipe.
    ///
    /// # Safety
    /// Same requirements as [`Self::send`].
    pub unsafe fn receive<'a, R: RegisterIo>(
        &mut self,
        rx: &mut RxCe,
        registers: &mut R,
        frame: &'a mut [u8],
    ) -> Result<&'a [u8], HttError> {
        let length = unsafe { rx.receive(registers, frame) }.map_err(|_| HttError::HtcError)?;
        let bytes = frame.get(..length).ok_or(HttError::FrameTooShort)?;
        data_message_payload(bytes, self.endpoint).map_err(|error| match error {
            HtcDataError::FrameTooShort => HttError::FrameTooShort,
            HtcDataError::WrongEndpoint | HtcDataError::BadTrailer => HttError::UnexpectedMessage,
        })
    }

    /// HTT version handshake: `ath10k_htt_h2t_ver_req_msg` followed by
    /// `ath10k_htt_t2h_version_conf`.
    ///
    /// # Safety
    /// Same requirements as [`Self::send`].
    pub unsafe fn version_handshake<R: RegisterIo>(
        &mut self,
        tx: &mut TxCe,
        rx: &mut RxCe,
        registers: &mut R,
        frame: &mut [u8],
    ) -> Result<HttVersion, HttError> {
        let mut request = [0u8; HTT_VERSION_REQ_LEN];
        request[0] = HTT_H2T_MSG_TYPE_VERSION_REQ;
        unsafe { self.send(tx, registers, &request, frame)? };
        for _ in 0..HTT_VERSION_ATTEMPTS {
            let response = unsafe { self.receive(rx, registers, frame) }?;
            match parse_version_conf(response) {
                Ok(version) => return Ok(version),
                // A different T2H message arrived first; keep polling.
                Err(HttError::UnexpectedMessage) => continue,
                Err(error) => return Err(error),
            }
        }
        Err(HttError::Timeout)
    }
}

/// Parse `struct htt_ver_resp` out of a T2H message.
fn parse_version_conf(message: &[u8]) -> Result<HttVersion, HttError> {
    if message.len() < HTT_VERSION_CONF_LEN {
        return Err(HttError::FrameTooShort);
    }
    if message[0] != HTT_T2H_MSG_TYPE_VERSION_CONF {
        return Err(HttError::UnexpectedMessage);
    }
    Ok(HttVersion {
        minor: message[1],
        major: message[2],
    })
}

/// The RX buffer ring the firmware DMAs received MSDUs into.
///
/// Mirrors `ath10k_htt_rx_alloc`: a coherent array of 32-bit buffer addresses
/// (`rx_ring.base_paddr`), one coherent word with the firmware's next free index
/// (`rx_ring.alloc_idx`), and the buffers themselves. The firmware keeps writing
/// into these buffers, so callers must keep the ring alive for the device
/// lifetime (the bring-up leaks it).
pub struct HttRxRing {
    /// Array of little-endian buffer addresses handed to the firmware.
    paddrs: DmaPage,
    /// Word the firmware stores its next free buffer index in.
    allocation_index: DmaPage,
    buffers: Vec<DmaPage>,
}

impl HttRxRing {
    /// Allocate `size` (a power of two) buffers of [`HTT_RX_BUF_SIZE`] bytes.
    pub fn allocate(size: usize) -> Result<Self, HttError> {
        if size == 0 || !size.is_power_of_two() || size * 4 > DmaPage::SIZE {
            return Err(HttError::InvalidRing);
        }
        if HTT_RX_BUF_SIZE > DmaPage::SIZE {
            return Err(HttError::InvalidRing);
        }

        let paddrs = DmaPage::allocate().map_err(|_| HttError::OutOfMemory)?;
        let allocation_index = DmaPage::allocate().map_err(|_| HttError::OutOfMemory)?;
        // The host posts buffers starting at index 0, so the firmware starts
        // allocating at 0 as well.
        allocation_index.write(&0u32.to_le_bytes());

        let mut addresses = Vec::with_capacity(size * 4);
        let mut buffers = Vec::with_capacity(size);
        for _ in 0..size {
            let buffer = DmaPage::allocate().map_err(|_| HttError::OutOfMemory)?;
            addresses.extend_from_slice(&buffer.physical_address().to_le_bytes());
            buffers.push(buffer);
        }
        paddrs.write(&addresses);

        Ok(Self {
            paddrs,
            allocation_index,
            buffers,
        })
    }

    pub fn size(&self) -> u16 {
        self.buffers.len() as u16
    }

    /// The `htt_rx_ring_setup_32` message body.
    pub fn message(&self) -> [u8; HTT_RX_RING_CFG_LEN] {
        encode_rx_ring_cfg(&RxRingLayout {
            index_shadow: self.allocation_index.physical_address(),
            ring_base: self.paddrs.physical_address(),
            ring_len: self.size(),
            buffer_size: HTT_RX_BUF_SIZE as u16,
            flags: HTT_RX_RING_FLAGS,
            first_index: 0,
        })
    }

    /// Send the ring configuration on the HTT endpoint.
    ///
    /// # Safety
    /// Same requirements as [`HttEndpoint::send`].
    pub unsafe fn configure<R: RegisterIo>(
        &mut self,
        htt: &mut HttEndpoint,
        tx: &mut TxCe,
        registers: &mut R,
        frame: &mut [u8],
    ) -> Result<(), HttError> {
        let message = self.message();
        unsafe { htt.send(tx, registers, &message, frame) }
    }
}

struct RxRingLayout {
    index_shadow: u32,
    ring_base: u32,
    ring_len: u16,
    buffer_size: u16,
    flags: u16,
    first_index: u16,
}

/// Encode `ath10k_htt_send_rx_ring_cfg`'s message for 32-bit addressing.
fn encode_rx_ring_cfg(layout: &RxRingLayout) -> [u8; HTT_RX_RING_CFG_LEN] {
    let mut out = [0u8; HTT_RX_RING_CFG_LEN];
    out[0] = HTT_H2T_MSG_TYPE_RX_RING_CFG;
    out[1] = 1; // `rx_setup_32.hdr.num_rings`
    // [2..4] rsvd0
    out[4..8].copy_from_slice(&layout.index_shadow.to_le_bytes());
    out[8..12].copy_from_slice(&layout.ring_base.to_le_bytes());
    out[12..14].copy_from_slice(&layout.ring_len.to_le_bytes());
    out[14..16].copy_from_slice(&layout.buffer_size.to_le_bytes());
    out[16..18].copy_from_slice(&layout.flags.to_le_bytes());
    out[18..20].copy_from_slice(&layout.first_index.to_le_bytes());
    // `htt_rx_ring_rx_desc_offsets` stay zero: this driver does not parse HTT RX
    // descriptors yet, and ath10k's HL variant (`ath10k_htt_send_rx_ring_cfg_hl`)
    // sends them zeroed as well.
    out
}

#[cfg(test)]
mod tests {
    use super::{
        HTT_RX_BUF_SIZE, HTT_RX_RING_CFG_LEN, HTT_RX_RING_FLAGS, HTT_VERSION_REQ_LEN, HttError,
        HttVersion, RxRingLayout, encode_rx_ring_cfg, parse_version_conf,
    };

    #[test]
    fn version_req_header_is_a_padded_word() {
        assert_eq!(HTT_VERSION_REQ_LEN, 4);
    }

    #[test]
    fn version_confirmation_is_msg_type_zero_then_minor_and_major() {
        let mut message = [0u8; 4];
        message[1] = 6; // minor
        message[2] = 3; // major
        assert_eq!(
            parse_version_conf(&message),
            Ok(HttVersion { major: 3, minor: 6 })
        );
        assert_eq!(
            parse_version_conf(&[0x01, 0, 0, 0]),
            Err(HttError::UnexpectedMessage)
        );
        assert_eq!(parse_version_conf(&[0x00, 0]), Err(HttError::FrameTooShort));
    }

    #[test]
    fn rx_ring_config_describes_the_ring_the_way_ath10k_does() {
        let layout = RxRingLayout {
            index_shadow: 0x1234_5000,
            ring_base: 0x1234_6000,
            ring_len: 64,
            buffer_size: HTT_RX_BUF_SIZE as u16,
            flags: HTT_RX_RING_FLAGS,
            first_index: 0,
        };
        let message = encode_rx_ring_cfg(&layout);

        assert_eq!(message.len(), HTT_RX_RING_CFG_LEN);
        assert_eq!(message[0], 0x02, "HTT_H2T_MSG_TYPE_RX_RING_CFG");
        assert_eq!(message[1], 1, "one ring");
        assert_eq!(&message[2..4], &[0, 0], "reserved");
        assert_eq!(
            u32::from_le_bytes([message[4], message[5], message[6], message[7]]),
            0x1234_5000,
            "fw_idx_shadow_reg_paddr"
        );
        assert_eq!(
            u32::from_le_bytes([message[8], message[9], message[10], message[11]]),
            0x1234_6000,
            "rx_ring_base_paddr"
        );
        assert_eq!(u16::from_le_bytes([message[12], message[13]]), 64);
        assert_eq!(u16::from_le_bytes([message[14], message[15]]), 2048);
        assert_eq!(u16::from_le_bytes([message[16], message[17]]), 0xffff);
        assert_eq!(u16::from_le_bytes([message[18], message[19]]), 0);
        assert_eq!(&message[20..], &[0u8; 20], "descriptor offsets");
    }
}
