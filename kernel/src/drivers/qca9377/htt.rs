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
use super::htc::{
    HTC_FLAG_NEED_CREDIT_UPDATE, HtcCreditReport, HtcCredits, HtcDataError, data_message,
    encode_data_message, parse_credit_reports,
};
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
/// `offsets[HTT_RX_DESC_OFFSETS_MSDU_PAYLOAD]`: a non-offloaded rx buffer holds
/// the v1 rx descriptor (its `mac80211_hdr` copy fills bytes 236..300) followed by
/// the 802.11 frame, which starts at byte 300.
pub const HTT_RX_DESC_V1_MSDU_PAYLOAD_OFFSET: usize = 300;
/// `rx_attention` is the second word of the v1 descriptor (`flags`).
const RX_DESC_V1_ATTENTION_OFFSET: usize = 4;
/// `RX_ATTENTION_FLAGS_MSDU_LENGTH_ERR`.
const RX_ATTENTION_MSDU_LENGTH_ERR: u32 = 1 << 17;
/// `RX_ATTENTION_FLAGS_MPDU_LENGTH_ERR`.
const RX_ATTENTION_MPDU_LENGTH_ERR: u32 = 1 << 27;
/// `RX_ATTENTION_FLAGS_MSDU_DONE`: the hardware finished filling the descriptor.
const RX_ATTENTION_MSDU_DONE: u32 = 1 << 31;
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
    /// The endpoint ran out of HTC transmit credits.
    NoCredits,
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
    /// Transmit credits refunded by the target's credit reports.
    credits: HtcCredits,
    sequence: u8,
}

impl HttEndpoint {
    /// `endpoint` and `max_message_size` come from the HTC connect response for
    /// `HTC_SERVICE_HTT_DATA`; `credits` from the HTC READY message.
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
        // Ask for a credit report with every frame while credits are accounted.
        let flags = if self.credits.is_enabled() {
            HTC_FLAG_NEED_CREDIT_UPDATE
        } else {
            0
        };
        let total = encode_data_message(frame, self.endpoint, flags, self.sequence, message)
            .map_err(|_| HttError::HtcError)?;
        self.credits.take(total).map_err(|_| HttError::NoCredits)?;
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
        let message = data_message(bytes, self.endpoint).map_err(|error| match error {
            HtcDataError::FrameTooShort => HttError::FrameTooShort,
            HtcDataError::WrongEndpoint | HtcDataError::BadTrailer => HttError::UnexpectedMessage,
        })?;
        let mut reports = [HtcCreditReport {
            endpoint: 0,
            credits: 0,
        }; 8];
        let count = parse_credit_reports(message.trailer, &mut reports);
        for report in &reports[..count] {
            if report.endpoint == self.endpoint {
                self.credits.report(report.credits);
            }
        }
        Ok(message.payload)
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

/// TX framing (`HTT_H2T_MSG_TYPE_TX_FRM`), the descriptor ath10k's
/// `ath10k_htt_tx_32()` writes for data and management frames.
///
/// Nothing transmits frames yet, so the module is allowed to be unused until
/// the TX path lands.
#[allow(dead_code)]
pub mod tx {
    // ── TX frame (`HTT_H2T_MSG_TYPE_TX_FRM`) ─────────────────────────────────────

    /// HTT >= 3.0 sends data **and** management frames with `TX_FRM`;
    /// `HTT_H2T_MSG_TYPE_MGMT_TX` is only the HTT < 3.0 path.
    pub const HTT_H2T_MSG_TYPE_TX_FRM: u8 = 1;
    /// `ath10k_htt_tx_32()` copies `36 + 4 + 8 + 2` bytes of the frame behind the
    /// descriptor (`802.11 + qos + ht`, `802.1q`, `llc/snap`, `ip dscp/priority`) so
    /// the target can classify it, rounded up to a word.
    pub const HTT_TX_PREFETCH_LEN: usize = 50;
    /// `struct htt_data_tx_desc`: flags0, flags1, len, id, frags_paddr, peerid.
    pub const HTT_TX_DESC_LEN: usize = 15;
    /// `htt_cmd_hdr` in front of the descriptor.
    pub const HTT_TX_HEADER_LEN: usize = 1 + HTT_TX_DESC_LEN;
    /// `HTT_INVALID_PEERID`: alpha frames this driver sends are not per-peer.
    pub const HTT_INVALID_PEERID: u16 = 0xffff;
    /// `HTT_DATA_TX_EXT_TID_MGMT` and `HTT_DATA_TX_EXT_TID_NON_QOS_MCAST_BCAST`.
    pub const HTT_DATA_TX_EXT_TID_MGMT: u8 = 17;
    pub const HTT_DATA_TX_EXT_TID_NON_QOS_MCAST_BCAST: u8 = 16;

    // `enum htt_data_tx_desc_flags0`.
    const HTT_TX_FLAGS0_MAC_HDR_PRESENT: u8 = 1 << 0;
    const HTT_TX_FLAGS0_NO_ENCRYPT: u8 = 1 << 2;
    const HTT_TX_FLAGS0_PKT_TYPE_MASK: u8 = 0xe0;
    const HTT_TX_FLAGS0_PKT_TYPE_LSB: u8 = 5;
    // `enum htt_data_tx_desc_flags1`.
    const HTT_TX_FLAGS1_VDEV_ID_MASK: u16 = 0x003f;
    const HTT_TX_FLAGS1_EXT_TID_MASK: u16 = 0x001f;
    const HTT_TX_FLAGS1_EXT_TID_LSB: u16 = 6;
    const HTT_TX_FLAGS1_POSTPONED: u16 = 1 << 11;

    /// `enum ath10k_hw_txrx_mode`: the value `ath10k_htt_tx_32()` puts in the
    /// descriptor's `flags0` packet type field.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum HttTxMode {
        Raw,
        NativeWifi,
        Ethernet,
        Mgmt,
    }

    impl HttTxMode {
        /// `HTT_DATA_TX_DESC_FLAGS0_PKT_TYPE`.
        const fn packet_type(self) -> u8 {
            match self {
                Self::Raw => 0,
                Self::NativeWifi => 1,
                Self::Ethernet => 2,
                Self::Mgmt => 3,
            }
        }

        /// `ath10k_htt_tx_32()` sets `MAC_HDR_PRESENT` for raw, native-wifi and
        /// management frames; only Ethernet frames carry no MAC header.
        const fn mac_header_present(self) -> bool {
            matches!(self, Self::Raw | Self::NativeWifi | Self::Mgmt)
        }
    }

    /// The HTT `TX_FRM` header for one frame, as `ath10k_htt_tx_32()` builds it.
    pub struct HttTxFrame<'a> {
        pub mode: HttTxMode,
        pub vdev_id: u8,
        /// `ext_tid`: 0-15 for QoS data, `HTT_DATA_TX_EXT_TID_MGMT` for management.
        pub tid: u8,
        pub msdu_id: u16,
        /// Physical address of the fragment list the target reads for the full frame.
        pub frags_paddr: u32,
        /// `ATH10K_SKB_F_NO_HWCRYPT`: the frame was already encrypted by the host.
        pub no_encrypt: bool,
        pub frame: &'a [u8],
    }

    impl HttTxFrame<'_> {
        /// Bytes of the frame copied behind the descriptor, rounded up to a word,
        /// exactly like ath10k's `roundup(min(prefetch_len, msdu->len), 4)`.
        pub fn prefetch_len(&self) -> usize {
            let prefetch = self.frame.len().min(HTT_TX_PREFETCH_LEN);
            (prefetch + 3) & !3
        }

        /// Encode `htt_cmd_hdr` + `htt_data_tx_desc` + the prefetched frame bytes.
        ///
        /// The result is one HTT message for `HttEndpoint::send`, which prepends the
        /// HTC header. `HTT_TX_HEADER_LEN + prefetch_len` is word aligned, which is
        /// the length ath10k writes into its HTC header for this message.
        ///
        /// Returns the message length, or `None` when the frame does not fit a
        /// 16-bit length or `out` is too small.
        pub fn encode(&self, out: &mut [u8]) -> Option<usize> {
            let frame_len = u16::try_from(self.frame.len()).ok()?;
            let prefetch = self.prefetch_len();
            let total = HTT_TX_HEADER_LEN.checked_add(prefetch)?;
            if out.len() < total {
                return None;
            }

            let mut flags0 = (self.mode.packet_type() << HTT_TX_FLAGS0_PKT_TYPE_LSB)
                & HTT_TX_FLAGS0_PKT_TYPE_MASK;
            if self.mode.mac_header_present() {
                flags0 |= HTT_TX_FLAGS0_MAC_HDR_PRESENT;
            }
            if self.no_encrypt {
                flags0 |= HTT_TX_FLAGS0_NO_ENCRYPT;
            }
            let flags1 = (u16::from(self.vdev_id) & HTT_TX_FLAGS1_VDEV_ID_MASK)
                | ((u16::from(self.tid) & HTT_TX_FLAGS1_EXT_TID_MASK) << HTT_TX_FLAGS1_EXT_TID_LSB)
                | HTT_TX_FLAGS1_POSTPONED;

            out[0] = HTT_H2T_MSG_TYPE_TX_FRM;
            out[1] = flags0;
            out[2..4].copy_from_slice(&flags1.to_le_bytes());
            out[4..6].copy_from_slice(&frame_len.to_le_bytes());
            out[6..8].copy_from_slice(&self.msdu_id.to_le_bytes());
            out[8..12].copy_from_slice(&self.frags_paddr.to_le_bytes());
            out[12..16].copy_from_slice(&u32::from(HTT_INVALID_PEERID).to_le_bytes());
            // Only the bytes the frame actually has are copied; the word padding is
            // explicit rather than read past the end of the frame.
            let copied = self.frame.len().min(prefetch);
            out[HTT_TX_HEADER_LEN..HTT_TX_HEADER_LEN + copied]
                .copy_from_slice(&self.frame[..copied]);
            out[HTT_TX_HEADER_LEN + copied..total].fill(0);
            Some(total)
        }
    }
}

// The TX path will import these from `htt`; until it exists only the tests do.
#[allow(unused_imports)]
pub use tx::*;

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

/// Slice the 802.11 frame out of a non-offloaded MSDU buffer.
///
/// A non-offloaded indication points at an rx ring buffer holding the v1 rx
/// descriptor followed by the frame at `HTT_RX_DESC_V1_MSDU_PAYLOAD_OFFSET`.
/// `msdu_len` is the frame length the indication carried
/// (`htt_rx_in_ord_msdu_desc.msdu_len`, ath10k's `amsdu_len`), and it covers only
/// the frame, not the descriptor.
///
/// The descriptor has to show `RX_ATTENTION_FLAGS_MSDU_DONE` before its contents
/// mean anything, and frames the MAC flagged with a length error are refused
/// rather than handed to the stack truncated.
pub fn non_offload_frame(buffer: &[u8], msdu_len: usize) -> Option<&[u8]> {
    let attention: [u8; 4] = buffer
        .get(RX_DESC_V1_ATTENTION_OFFSET..RX_DESC_V1_ATTENTION_OFFSET + 4)?
        .try_into()
        .ok()?;
    let attention = u32::from_le_bytes(attention);
    if attention & RX_ATTENTION_MSDU_DONE == 0
        || attention & (RX_ATTENTION_MSDU_LENGTH_ERR | RX_ATTENTION_MPDU_LENGTH_ERR) != 0
    {
        return None;
    }

    let start = HTT_RX_DESC_V1_MSDU_PAYLOAD_OFFSET;
    let end = start.checked_add(msdu_len)?;
    // The shortest 802.11 header is 24 bytes; anything smaller is a bad length.
    if msdu_len < 24 || end > buffer.len() {
        return None;
    }
    Some(&buffer[start..end])
}

// `IEEE80211_FCTL_*` frame control bits and `IEEE80211_FTYPE_DATA`.
const IEEE80211_FCTL_FTYPE: u16 = 0x000c;
const IEEE80211_FCTL_TODS: u16 = 0x0100;
const IEEE80211_FCTL_FROMDS: u16 = 0x0200;
const IEEE80211_FCTL_PROTECTED: u16 = 0x4000;
const IEEE80211_FCTL_ORDER: u16 = 0x8000;
const IEEE80211_FTYPE_DATA: u16 = 0x0008;
/// The QoS bit of `IEEE80211_STYPE_QOS_DATA` inside the subtype field.
const IEEE80211_STYPE_QOS: u16 = 0x0080;
/// QoS control bit 7: the frame aggregates A-MSDUs, which need deaggregation.
const IEEE80211_QOS_CTL_A_MSDU_PRESENT: u16 = 0x0080;
const IEEE80211_HEADER_LEN: usize = 24;
const LLC_SNAP_LEN: usize = 8;
const ETHERNET_HEADER_LEN: usize = 14;

/// Convert a plain 802.11 data frame into an Ethernet II frame.
///
/// This is the driver-side equivalent of mac80211's
/// `ieee80211_data_to_8023_exthdr()`: the three non-WDS address mappings are
/// translated, the QoS and HT-Control fields are skipped so the LLC/SNAP header
/// is found, and only an untagged SNAP payload (OUI `00:00:00`) is accepted.
/// Frames this driver cannot decode are refused instead of guessed at: WDS
/// (four-address) frames, A-MSDU aggregates, and encrypted payloads, for which no
/// key is installed here.
///
/// Returns the number of bytes written to `out`, or `None`.
pub fn ieee80211_to_ethernet(frame: &[u8], out: &mut [u8]) -> Option<usize> {
    if frame.len() < IEEE80211_HEADER_LEN {
        return None;
    }
    let frame_control = u16::from_le_bytes([frame[0], frame[1]]);
    if frame_control & IEEE80211_FCTL_FTYPE != IEEE80211_FTYPE_DATA {
        return None;
    }
    if frame_control & IEEE80211_FCTL_PROTECTED != 0 {
        return None;
    }
    let to_ds = frame_control & IEEE80211_FCTL_TODS != 0;
    let from_ds = frame_control & IEEE80211_FCTL_FROMDS != 0;
    if to_ds && from_ds {
        return None;
    }

    let mut header_len = IEEE80211_HEADER_LEN;
    if frame_control & IEEE80211_STYPE_QOS != 0 {
        let qos_control = u16::from_le_bytes([*frame.get(24)?, *frame.get(25)?]);
        if qos_control & IEEE80211_QOS_CTL_A_MSDU_PRESENT != 0 {
            return None;
        }
        header_len += 2;
    }
    if frame_control & IEEE80211_FCTL_ORDER != 0 {
        // HT control field follows the QoS control field.
        header_len += 4;
    }

    // Address mapping from `ieee80211_data_to_8023_exthdr()`: a station only ever
    // sees the IBSS, ToDS and FromDS cases.
    let (destination, source) = if to_ds {
        // ToDS: addr2 is the transmitter (us), addr3 is the destination behind DS.
        (frame.get(16..22)?, frame.get(10..16)?)
    } else if from_ds {
        // FromDS: addr1 is the destination, addr3 is the original source.
        (frame.get(4..10)?, frame.get(16..22)?)
    } else {
        // IBSS: addr1 and addr2 are destination and source.
        (frame.get(4..10)?, frame.get(10..16)?)
    };

    let payload = frame.get(header_len..)?;
    if payload.len() < LLC_SNAP_LEN {
        return None;
    }
    // IEEE 802.2 LLC/SNAP: dsap/ssap 0xaa, control 0x03, OUI 00:00:00.
    if payload[0] != 0xaa || payload[1] != 0xaa || payload[2] != 0x03 {
        return None;
    }
    if payload[3] != 0 || payload[4] != 0 || payload[5] != 0 {
        return None;
    }
    let ether_type = [payload[6], payload[7]];
    let body = &payload[LLC_SNAP_LEN..];
    let total = ETHERNET_HEADER_LEN + body.len();
    if out.len() < total {
        return None;
    }
    out[..6].copy_from_slice(destination);
    out[6..12].copy_from_slice(source);
    out[12..14].copy_from_slice(&ether_type);
    out[ETHERNET_HEADER_LEN..total].copy_from_slice(body);
    Some(total)
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
        HTT_RX_BUF_SIZE, HTT_RX_DESC_V1_MSDU_PAYLOAD_OFFSET, HTT_RX_DESC_V1_OFFSETS_WORDS,
        HTT_RX_OFFLOAD_MSDU_HEADER_LEN, HTT_RX_RING_CFG_LEN, HTT_RX_RING_FLAGS,
        HTT_VERSION_REQ_LEN, HttError, HttVersion, RxIndication, RxMsdu, RxRingLayout,
        encode_data_message, encode_rx_ring_cfg, ieee80211_to_ethernet, non_offload_frame,
        offload_frame, parse_rx_indication, parse_version_conf, HTT_DATA_TX_EXT_TID_MGMT,
        HTT_TX_HEADER_LEN, HTT_TX_PREFETCH_LEN, HttTxFrame, HttTxMode,
    };
    use alloc::vec::Vec;

    /// A v1 rx descriptor holding `frame`, with `MSDU_DONE` set.
    fn non_offload_buffer(frame: &[u8]) -> Vec<u8> {
        let mut buffer = alloc::vec![0u8; HTT_RX_DESC_V1_MSDU_PAYLOAD_OFFSET + frame.len()];
        buffer[4..8].copy_from_slice(&(1u32 << 31).to_le_bytes());
        buffer[HTT_RX_DESC_V1_MSDU_PAYLOAD_OFFSET..].copy_from_slice(frame);
        buffer
    }

    /// One 802.11 data frame with an LLC/SNAP payload around `body`.
    fn data_frame(
        frame_control: u16,
        addresses: [&[u8; 6]; 3],
        qos_control: Option<u16>,
        body: &[u8],
    ) -> Vec<u8> {
        let mut frame = Vec::new();
        frame.extend_from_slice(&frame_control.to_le_bytes());
        frame.extend_from_slice(&[0, 0]);
        for address in addresses {
            frame.extend_from_slice(address);
        }
        frame.extend_from_slice(&[0, 0]);
        if let Some(qos_control) = qos_control {
            frame.extend_from_slice(&qos_control.to_le_bytes());
        }
        frame.extend_from_slice(&[0xaa, 0xaa, 0x03, 0x00, 0x00, 0x00, 0x08, 0x00]);
        frame.extend_from_slice(body);
        frame
    }

    /// A `TX_FRM` descriptor for `frame`, filled like `ath10k_htt_tx_32()`.
    fn tx_frame<'a>(mode: HttTxMode, frame: &'a [u8]) -> HttTxFrame<'a> {
        HttTxFrame {
            mode,
            vdev_id: 0,
            tid: 0,
            msdu_id: 0,
            frags_paddr: 0,
            no_encrypt: false,
            frame,
        }
    }

    #[test]
    fn tx_frm_data_frame_uses_the_ath10k_descriptor_layout() {
        let frame = [0x42u8; 60];
        let tx = HttTxFrame {
            mode: HttTxMode::Ethernet,
            vdev_id: 1,
            tid: 5,
            msdu_id: 0x1234,
            frags_paddr: 0xdead_beef,
            no_encrypt: false,
            frame: &frame,
        };

        let mut message = [0u8; 128];
        let message_len = tx.encode(&mut message).expect("encoded");

        // `roundup(min(50, 60), 4)` bytes of the frame follow the descriptor.
        assert_eq!(tx.prefetch_len(), 52);
        assert_eq!(message_len, HTT_TX_HEADER_LEN + 52);
        // htt_cmd_hdr(msg_type = TX_FRM) | flags0 | flags1 | len | id |
        // frags_paddr | peerid, all little endian.
        assert_eq!(
            &message[..HTT_TX_HEADER_LEN],
            &[
                1, // HTT_H2T_MSG_TYPE_TX_FRM
                0x40, // pkt_type Ethernet (2 << 5), no MAC header
                0x41, 0x09, // vdev 1 | ext_tid 5 << 6 | POSTPONED
                60, 0x00, // len
                0x34, 0x12, // msdu id
                0xef, 0xbe, 0xad, 0xde, // frags_paddr
                0xff, 0xff, 0x00, 0x00, // HTT_INVALID_PEERID
            ]
        );
        assert_eq!(&message[HTT_TX_HEADER_LEN..message_len], &frame[..52]);
    }

    #[test]
    fn tx_frm_management_frame_sets_mac_hdr_present() {
        let frame = [0x80u8; 24];
        let tx = HttTxFrame {
            mode: HttTxMode::Mgmt,
            vdev_id: 0,
            tid: HTT_DATA_TX_EXT_TID_MGMT,
            msdu_id: 7,
            frags_paddr: 0x1000,
            no_encrypt: true,
            frame: &frame,
        };

        let mut message = [0u8; 64];
        let message_len = tx.encode(&mut message).expect("encoded");

        assert_eq!(tx.prefetch_len(), 24);
        assert_eq!(message_len, HTT_TX_HEADER_LEN + 24);
        // mgmt packet type (3 << 5) + MAC_HDR_PRESENT + NO_ENCRYPT, then ext_tid
        // 17 and POSTPONED.
        assert_eq!(
            &message[..HTT_TX_HEADER_LEN],
            &[
                1, 0x65, 0x40, 0x0c, 24, 0x00, 7, 0x00, 0x00, 0x10, 0x00, 0x00, 0xff, 0xff, 0x00,
                0x00,
            ]
        );
        assert_eq!(&message[HTT_TX_HEADER_LEN..message_len], &frame[..]);
    }

    #[test]
    fn tx_frm_flags0_packet_type_follows_ath10k_txrx_modes() {
        let frame = [0u8; 8];
        let mut message = [0u8; 32];
        // Raw and native-wifi carry an 802.11 header; Ethernet does not.
        for (mode, flags0) in [
            (HttTxMode::Raw, 0x01),
            (HttTxMode::NativeWifi, 0x21),
            (HttTxMode::Ethernet, 0x40),
            (HttTxMode::Mgmt, 0x61),
        ] {
            let tx = tx_frame(mode, &frame);
            tx.encode(&mut message).expect("encoded");
            assert_eq!(message[0], 1, "message type for {mode:?}");
            assert_eq!(message[1], flags0, "flags0 for {mode:?}");
        }
    }

    #[test]
    fn tx_frm_refuses_frames_that_do_not_fit() {
        let frame = [0u8; 32];
        let tx = tx_frame(HttTxMode::NativeWifi, &frame);
        let mut small = [0u8; HTT_TX_HEADER_LEN];
        assert_eq!(tx.encode(&mut small), None);

        // A frame longer than the descriptor's 16-bit length is refused.
        let oversized = alloc::vec![0u8; usize::from(u16::MAX) + 1];
        let tx = tx_frame(HttTxMode::NativeWifi, &oversized);
        let mut message = [0u8; 128];
        assert_eq!(tx.encode(&mut message), None);

        // Short frames still round the prefetch up to a word and zero the pad.
        let short = [0x5au8; 3];
        let tx = tx_frame(HttTxMode::NativeWifi, &short);
        let message_len = tx.encode(&mut message).expect("encoded");
        assert_eq!(tx.prefetch_len(), 4);
        assert_eq!(message_len, HTT_TX_HEADER_LEN + 4);
        assert_eq!(&message[HTT_TX_HEADER_LEN..message_len], &[0x5a, 0x5a, 0x5a, 0]);
    }

    #[test]
    fn tx_frame_travels_as_one_word_aligned_htc_message() {
        let frame = [0x11u8; 40];
        let mut tx = tx_frame(HttTxMode::NativeWifi, &frame);
        tx.vdev_id = 2;
        tx.msdu_id = 1;
        tx.frags_paddr = 0x2000;

        let mut message = [0u8; 128];
        let message_len = tx.encode(&mut message).expect("encoded");
        // 16 + roundup(min(50, 40), 4) = 56, which HTC can carry unpadded.
        assert_eq!(message_len, HTT_TX_HEADER_LEN + 40);
        assert_eq!(message_len % 4, 0);
        assert!(HTT_TX_PREFETCH_LEN >= 36 + 4 + 8 + 2);

        let mut htc_frame = [0u8; 128];
        let total = encode_data_message(&mut htc_frame, 3, 0, 9, &message[..message_len])
            .expect("framed");
        assert_eq!(total, 8 + message_len);
        assert_eq!(htc_frame[0], 3); // HTT data endpoint
        assert_eq!(&htc_frame[2..4], &(message_len as u16).to_le_bytes());
        assert_eq!(&htc_frame[8..total], &message[..message_len]);
    }

    #[test]
    fn non_offload_frame_starts_after_the_v1_descriptor() {
        let frame = data_frame(0x0208, [&[1; 6], &[2; 6], &[3; 6]], None, b"payload");
        let buffer = non_offload_buffer(&frame);
        assert_eq!(
            non_offload_frame(&buffer, frame.len()),
            Some(frame.as_slice())
        );

        // Without MSDU_DONE the descriptor may still be stale.
        let mut stale = buffer.clone();
        stale[4..8].fill(0);
        assert_eq!(non_offload_frame(&stale, frame.len()), None);

        // Frames the MAC flagged as truncated are refused.
        let mut broken = buffer.clone();
        broken[4..8].copy_from_slice(&((1u32 << 31) | (1u32 << 27)).to_le_bytes());
        assert_eq!(non_offload_frame(&broken, frame.len()), None);

        // A length that runs past the buffer is refused too.
        assert_eq!(non_offload_frame(&buffer, buffer.len()), None);
    }

    #[test]
    fn decapsulates_a_from_ds_frame_into_ethernet() {
        let destination = [0x02, 0, 0, 0, 0, 0x11];
        let bssid = [0x02, 0, 0, 0, 0, 0x22];
        let source = [0x02, 0, 0, 0, 0, 0x33];
        // FromDS data frame: addr1 destination, addr2 BSSID, addr3 source.
        let frame = data_frame(0x0208, [&destination, &bssid, &source], None, b"hello");

        let mut out = [0u8; 64];
        let length = ieee80211_to_ethernet(&frame, &mut out).expect("ethernet frame");
        assert_eq!(&out[..6], &destination);
        assert_eq!(&out[6..12], &source);
        assert_eq!(&out[12..14], &[0x08, 0x00]);
        assert_eq!(&out[14..length], b"hello");
        assert_eq!(length, 14 + 5);
    }

    #[test]
    fn decapsulates_a_qos_to_ds_frame_and_skips_its_qos_field() {
        let bssid = [0x02, 0, 0, 0, 0, 0x22];
        let source = [0x02, 0, 0, 0, 0, 0x44];
        let destination = [0x02, 0, 0, 0, 0, 0x55];
        // ToDS QoS data frame: addr1 BSSID, addr2 source, addr3 destination.
        let frame = data_frame(0x0188, [&bssid, &source, &destination], Some(5), b"abc");

        let mut out = [0u8; 32];
        let length = ieee80211_to_ethernet(&frame, &mut out).expect("ethernet frame");
        assert_eq!(&out[..6], &destination);
        assert_eq!(&out[6..12], &source);
        assert_eq!(&out[14..length], b"abc");
    }

    #[test]
    fn refuses_frames_the_driver_cannot_decode() {
        let first = [1u8; 6];
        let second = [2u8; 6];
        let third = [3u8; 6];
        let mut out = [0u8; 64];

        // WDS (both DS bits) needs the fourth address this mapping lacks.
        let wds = data_frame(0x0308, [&first, &second, &third], None, b"x");
        assert_eq!(ieee80211_to_ethernet(&wds, &mut out), None);
        // Protected frames need a key installed here.
        let protected = data_frame(0x4208, [&first, &second, &third], None, b"x");
        assert_eq!(ieee80211_to_ethernet(&protected, &mut out), None);
        // A-MSDU aggregates carry several subframes behind one header.
        let amsdu = data_frame(0x0288, [&first, &second, &third], Some(0x0080), b"x");
        assert_eq!(ieee80211_to_ethernet(&amsdu, &mut out), None);
        // Management frames never reach the network stack.
        let beacon = data_frame(0x0080, [&first, &second, &third], None, b"x");
        assert_eq!(ieee80211_to_ethernet(&beacon, &mut out), None);
        // A payload shorter than the LLC/SNAP header is refused.
        let mut truncated = data_frame(0x0208, [&first, &second, &third], None, b"");
        truncated.truncate(24 + 4);
        assert_eq!(ieee80211_to_ethernet(&truncated, &mut out), None);
    }

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
