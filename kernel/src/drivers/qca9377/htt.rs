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
/// TLV firmware (`htt-op 3`) message type for in-order paddr indications.
pub const HTT_TLV_T2H_MSG_TYPE_RX_IN_ORD_PADDR_IND: u8 = 0x12;
/// `htt_resp_hdr` (msg_type) + the fixed part of `htt_rx_in_ord_ind`
/// (`info`, `peer_id`, `vdev_id`, `reserved`, `msdu_count`); the MSDU
/// descriptors follow at this offset.
const RX_IN_ORD_IND_HEADER_LEN: usize = 1 + 1 + 2 + 1 + 1 + 2;
/// `struct htt_rx_in_ord_msdu_desc`.
const RX_IN_ORD_MSDU_DESC_LEN: usize = 8;
/// `HTT_RX_IN_ORD_IND_INFO_OFFLOAD_MASK`.
const RX_IN_ORD_IND_INFO_OFFLOAD_MASK: u8 = 0x20;
/// `HTT_RX_IN_ORD_IND_INFO_TID_MASK`.
const RX_IN_ORD_IND_INFO_TID_MASK: u8 = 0x1f;
/// `struct htt_rx_offload_msdu` in front of an offloaded Ethernet frame.
pub const HTT_RX_OFFLOAD_MSDU_HEADER_LEN: usize = 7;
/// `htt_cmd_hdr` + `htt_rx_ring_setup_32` + one `htt_rx_ring_setup_ring32`.
const HTT_RX_RING_CFG_LEN: usize = 1 + 3 + 36;
/// `offsetof(struct htt_rx_desc_v1, field) / 4` in `htt_rx_ring_rx_desc_offsets`
/// order: mac80211_hdr, msdu_payload, ppdu_start, ppdu_end, mpdu_start,
/// mpdu_end, msdu_start, msdu_end, rx_attention, frag_info.
///
/// QCA9377 uses the v1 rx descriptor it shares with QCA6174/QCA988x/QCA99x0, and
/// `ath10k_htt_rx_desc_get_offsets` falls back to exactly these `offsetof`
/// values because `qca99x0_rx_desc_ops` does not provide `rx_desc_get_offsets`.
/// Byte offsets in the descriptor are 4, 8, 12, 24, 40, 80, 84, 124, 236 and
/// 300 (`rx_hdr_status` + `RX_HTT_HDR_STATUS_LEN`), all four-byte aligned.
pub const HTT_RX_DESC_V1_OFFSETS_WORDS: [u16; 10] = [59, 75, 21, 31, 3, 20, 6, 10, 1, 2];
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
    /// Software copy of the addresses in `paddrs`, for paddr → index lookups.
    addresses: Vec<u32>,
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

        let mut encoded = Vec::with_capacity(size * 4);
        let mut addresses = Vec::with_capacity(size);
        let mut buffers = Vec::with_capacity(size);
        for _ in 0..size {
            let buffer = DmaPage::allocate().map_err(|_| HttError::OutOfMemory)?;
            let address = buffer.physical_address();
            encoded.extend_from_slice(&address.to_le_bytes());
            addresses.push(address);
            buffers.push(buffer);
        }
        paddrs.write(&encoded);

        Ok(Self {
            paddrs,
            allocation_index,
            buffers,
            addresses,
        })
    }

    /// Ring index whose buffer starts at `paddr`, if any.
    pub fn index_of(&self, paddr: u32) -> Option<usize> {
        self.addresses.iter().position(|address| *address == paddr)
    }

    /// Copy up to `out.len()` bytes of buffer `index` into `out`.
    pub fn read_buffer(&self, index: usize, out: &mut [u8]) -> bool {
        let Some(buffer) = self.buffers.get(index) else {
            return false;
        };
        if out.len() > HTT_RX_BUF_SIZE {
            return false;
        }
        buffer.read(out);
        true
    }

    /// Hand buffer `index` back to the firmware, the way ath10k advances
    /// `rx_ring.alloc_idx` after posting a buffer again.
    pub fn release(&mut self, index: usize) {
        if self.buffers.is_empty() {
            return;
        }
        let next = (index + 1) % self.buffers.len();
        self.allocation_index.write(&(next as u32).to_le_bytes());
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

/// One MSDU the firmware placed in the RX ring (`htt_rx_in_ord_msdu_desc`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RxMsdu {
    /// Physical address of the ring buffer holding the frame.
    pub paddr: u32,
    /// Bytes the firmware wrote into that buffer.
    pub len: u16,
    pub fw_desc: u8,
}

/// A parsed `htt_rx_in_ord_ind` (`HTT_TLV_T2H_MSG_TYPE_RX_IN_ORD_PADDR_IND`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RxIndication {
    pub vdev_id: u8,
    pub peer_id: u16,
    pub tid: u8,
    /// Offloaded indications hold complete Ethernet frames, decapsulated by the
    /// firmware; non-offloaded ones hold the rx descriptor plus an 802.11 frame.
    pub offload: bool,
    pub msdus: Vec<RxMsdu>,
}

/// Parse an in-order RX paddr indication.
///
/// Layout (`struct htt_resp` + `struct htt_rx_in_ord_ind`):
/// `msg_type(1) | info(1) | peer_id(2) | vdev_id(1) | rsvd(1) | count(2)` then
/// `count` × `{ paddr(4) | len(2) | fw_desc(1) | rsvd(1) }`.
pub fn parse_rx_indication(message: &[u8]) -> Result<RxIndication, HttError> {
    if message.len() < RX_IN_ORD_IND_HEADER_LEN {
        return Err(HttError::FrameTooShort);
    }
    if message[0] != HTT_TLV_T2H_MSG_TYPE_RX_IN_ORD_PADDR_IND {
        return Err(HttError::UnexpectedMessage);
    }
    let info = message[1];
    let peer_id = u16::from_le_bytes([message[2], message[3]]);
    let vdev_id = message[4];
    let count = usize::from(u16::from_le_bytes([message[6], message[7]]));
    let needed = RX_IN_ORD_IND_HEADER_LEN
        .checked_add(count * RX_IN_ORD_MSDU_DESC_LEN)
        .ok_or(HttError::FrameTooShort)?;
    if message.len() < needed {
        return Err(HttError::FrameTooShort);
    }

    let mut msdus = Vec::with_capacity(count);
    for index in 0..count {
        let start = RX_IN_ORD_IND_HEADER_LEN + index * RX_IN_ORD_MSDU_DESC_LEN;
        let descriptor = &message[start..start + RX_IN_ORD_MSDU_DESC_LEN];
        msdus.push(RxMsdu {
            paddr: u32::from_le_bytes([
                descriptor[0],
                descriptor[1],
                descriptor[2],
                descriptor[3],
            ]),
            len: u16::from_le_bytes([descriptor[4], descriptor[5]]),
            fw_desc: descriptor[6],
        });
    }

    Ok(RxIndication {
        vdev_id,
        peer_id,
        tid: info & RX_IN_ORD_IND_INFO_TID_MASK,
        offload: info & RX_IN_ORD_IND_INFO_OFFLOAD_MASK != 0,
        msdus,
    })
}

/// Extract the Ethernet frame from an offloaded MSDU buffer
/// (`struct htt_rx_offload_msdu` followed by the frame).
pub fn offload_frame(buffer: &[u8]) -> Option<&[u8]> {
    if buffer.len() < HTT_RX_OFFLOAD_MSDU_HEADER_LEN {
        return None;
    }
    let frame_len = usize::from(u16::from_le_bytes([buffer[0], buffer[1]]));
    let start = HTT_RX_OFFLOAD_MSDU_HEADER_LEN;
    if frame_len == 0 || start + frame_len > buffer.len() {
        return None;
    }
    Some(&buffer[start..start + frame_len])
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
    for (index, offset) in HTT_RX_DESC_V1_OFFSETS_WORDS.iter().enumerate() {
        let start = 20 + index * 2;
        out[start..start + 2].copy_from_slice(&offset.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{
        HTT_RX_BUF_SIZE, HTT_RX_DESC_V1_OFFSETS_WORDS, HTT_RX_OFFLOAD_MSDU_HEADER_LEN,
        HTT_RX_RING_CFG_LEN, HTT_RX_RING_FLAGS, HTT_VERSION_REQ_LEN, HttError, HttVersion,
        RxIndication, RxMsdu, RxRingLayout, encode_rx_ring_cfg, offload_frame, parse_rx_indication,
        parse_version_conf,
    };
    use alloc::vec::Vec;

    /// One offloaded MSDU descriptor: paddr 0xdead_beef, 32 bytes, fw_desc 0.
    fn offload_indication(count: u8) -> [u8; 16] {
        let mut message = [0u8; 16];
        message[0] = 0x12; // HTT_TLV_T2H_MSG_TYPE_RX_IN_ORD_PADDR_IND
        message[1] = 0x20 | 0x05; // offload + tid 5
        message[2..4].copy_from_slice(&7u16.to_le_bytes()); // peer_id
        message[4] = 2; // vdev_id
        message[6..8].copy_from_slice(&u16::from(count).to_le_bytes());
        message[8..12].copy_from_slice(&0xdead_beefu32.to_le_bytes());
        message[12..14].copy_from_slice(&32u16.to_le_bytes());
        message
    }

    #[test]
    fn in_order_indication_is_parsed_into_msdus() {
        let message = offload_indication(1);
        let indication = parse_rx_indication(&message).expect("parses");
        assert_eq!(
            indication,
            RxIndication {
                vdev_id: 2,
                peer_id: 7,
                tid: 5,
                offload: true,
                msdus: alloc::vec![RxMsdu {
                    paddr: 0xdead_beef,
                    len: 32,
                    fw_desc: 0,
                }],
            }
        );
    }

    #[test]
    fn in_order_indication_rejects_other_messages_and_short_bodies() {
        let mut message = offload_indication(1);
        message[0] = 0x07; // TX completion, not an RX indication
        assert_eq!(
            parse_rx_indication(&message),
            Err(HttError::UnexpectedMessage)
        );

        // Claims two MSDU descriptors but only one fits in the message.
        let mut truncated = offload_indication(2);
        truncated[0] = 0x12;
        assert_eq!(
            parse_rx_indication(&truncated),
            Err(HttError::FrameTooShort)
        );
    }

    #[test]
    fn offloaded_msdu_header_prefixes_the_ethernet_frame() {
        let mut buffer = [0u8; 32];
        buffer[0..2].copy_from_slice(&20u16.to_le_bytes()); // frame length
        buffer[2..4].copy_from_slice(&7u16.to_le_bytes()); // peer_id
        buffer[4] = 2; // vdev_id
        buffer[5] = 5; // tid
        buffer[6] = 0; // fw_desc
        for (index, byte) in buffer[HTT_RX_OFFLOAD_MSDU_HEADER_LEN..][..20]
            .iter_mut()
            .enumerate()
        {
            *byte = index as u8;
        }

        let frame = offload_frame(&buffer).expect("frame inside the buffer");
        assert_eq!(frame.len(), 20);
        assert_eq!(frame[0], 0);
        assert_eq!(frame[19], 19);

        // A length that runs past the buffer is rejected instead of over-read.
        buffer[0..2].copy_from_slice(&60u16.to_le_bytes());
        assert_eq!(offload_frame(&buffer), None);
    }

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

        // htt_rx_ring_rx_desc_offsets: one little-endian word offset each.
        let offsets: Vec<u16> = message[20..]
            .chunks_exact(2)
            .map(|word| u16::from_le_bytes([word[0], word[1]]))
            .collect();
        assert_eq!(offsets, HTT_RX_DESC_V1_OFFSETS_WORDS);
        assert_eq!(offsets[0], 59, "mac80211_hdr_offset");
        assert_eq!(offsets[1], 75, "msdu_payload_offset");
        assert_eq!(offsets[4], 3, "mpdu_start_offset");
        assert_eq!(offsets[8], 1, "rx_attention_offset");
        assert_eq!(offsets[9], 2, "frag_info_offset");
        assert_eq!(
            usize::from(offsets[1]) * 4,
            300,
            "the payload follows the whole 300-byte v1 descriptor"
        );
    }
}
