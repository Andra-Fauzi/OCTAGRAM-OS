//! WMI-TLV wire format support for QCA9377 / ath10k firmware 10.2.
//!
//! WMI-TLV uses a 32-bit command/event id followed by a sequence of
//! little-endian { tag: u16, length: u16, value: [u8; length] } records.
//! Values are not implicitly padded between records.
//!
//! The parser deliberately exposes host-memory requests instead of silently
//! ignoring them. The current kernel allocator only provides single DMA32
//! pages; until a contiguous DMA-region allocator exists, init must reject a
//! firmware request that needs a larger or multi-unit pool.

use super::{
    ce::{RegisterIo, RxCe, TxCe},
    htc::{
        HTC_FLAG_NEED_CREDIT_UPDATE, HtcCreditReport, HtcCredits, HtcDataError,
        data_message, parse_credit_reports, write_data_header, HTC_HEADER_LENGTH,
    },
};

const WMI_HEADER_LEN: usize = 4;
const TLV_HEADER_LEN: usize = 4;

pub const EVENT_SERVICE_READY: u32 = 0x0001;
pub const EVENT_READY: u32 = 0x0002;
pub const CMD_INIT: u32 = 0x0001;

const TAG_ARRAY_STRUCT: u16 = 18;
const TAG_SERVICE_READY_EVENT: u16 = 32;
const TAG_WLAN_HOST_MEM_REQ: u16 = 34;
const TAG_READY_EVENT: u16 = 35;
// Values follow enum wmi_tlv_tag in ath10k/wmi-tlv.h. The enum starts at
// 32 after the reserved and array-tag ranges.
const TAG_INIT_CMD: u16 = 74;
const TAG_RESOURCE_CONFIG: u16 = 75;

const SERVICE_READY_MIN_LEN: usize = 104;
const READY_EVENT_MIN_LEN: usize = 36;
const HOST_MEM_REQ_LEN: usize = 16;
const MAX_HOST_MEM_REQUESTS: usize = 16;

/// ABI constants used by Linux ath10k's WMI-TLV operations table.
pub const ABI_VERSION: [u32; 6] = [
    0x0100_0000, // ABI_VER0: major 1, minor 0
    53,          // ABI_VER1
    0x5f41_4341, // ABI_VER_NS0: "QCA_"
    0x0000_4c4d, // ABI_VER_NS1
    0,
    0,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WmiTlvError {
    FrameTooShort,
    MalformedTlv,
    DuplicateRequiredTag,
    MissingServiceReady,
    MissingReady,
    BadEventId,
    BadStructLength,
    HostRequestCountMismatch,
    TooManyHostMemoryRequests,
    OutputBufferTooSmall,
    BadEndpoint,
    BadTrailer,
    NoCredits,
    Transport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostMemoryRequest {
    pub request_id: u32,
    pub unit_size: u32,
    pub unit_info: u32,
    pub num_units: u32,
}

impl HostMemoryRequest {
    const EMPTY: Self = Self {
        request_id: 0,
        unit_size: 0,
        unit_info: 0,
        num_units: 0,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceReadyEvent {
    pub firmware_build: u32,
    pub abi: [u32; 6],
    pub num_memory_requests: u32,
    pub memory_requests: [HostMemoryRequest; MAX_HOST_MEM_REQUESTS],
    pub memory_request_count: usize,
}

impl ServiceReadyEvent {
    pub const fn has_host_memory_requests(&self) -> bool {
        self.num_memory_requests != 0
    }

    pub const fn abi_is_supported(&self) -> bool {
        let mut index = 0;
        while index < ABI_VERSION.len() {
            if self.abi[index] != ABI_VERSION[index] {
                return false;
            }
            index += 1;
        }
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadyEvent {
    pub abi: [u32; 6],
    pub mac_address: [u8; 6],
    pub status: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WmiTlvEvent {
    ServiceReady(ServiceReadyEvent),
    Ready(ReadyEvent),
    Other(u32),
}

#[derive(Debug, Clone, Copy)]
struct Tlv<'a> {
    tag: u16,
    value: &'a [u8],
}

fn read_le16(bytes: &[u8], offset: usize) -> Option<u16> {
    let pair = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([pair[0], pair[1]]))
}

fn read_le32(bytes: &[u8], offset: usize) -> Option<u32> {
    let word = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
}

fn write_le16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_le32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

/// Visit every TLV in a message body. Truncated headers and values are errors;
/// a malformed tail is never silently treated as an empty or partial message.
fn walk_tlvs(
    mut bytes: &[u8],
    mut visit: impl FnMut(Tlv<'_>) -> Result<(), WmiTlvError>,
) -> Result<(), WmiTlvError> {
    while !bytes.is_empty() {
        if bytes.len() < TLV_HEADER_LEN {
            return Err(WmiTlvError::MalformedTlv);
        }
        let tag = read_le16(bytes, 0).ok_or(WmiTlvError::MalformedTlv)?;
        let length = usize::from(read_le16(bytes, 2).ok_or(WmiTlvError::MalformedTlv)?);
        bytes = &bytes[TLV_HEADER_LEN..];
        if length > bytes.len() {
            return Err(WmiTlvError::MalformedTlv);
        }
        let (value, remainder) = bytes.split_at(length);
        visit(Tlv { tag, value })?;
        bytes = remainder;
    }
    Ok(())
}

fn parse_host_memory_request(value: &[u8]) -> Result<HostMemoryRequest, WmiTlvError> {
    if value.len() < HOST_MEM_REQ_LEN {
        return Err(WmiTlvError::BadStructLength);
    }
    Ok(HostMemoryRequest {
        request_id: read_le32(value, 0).ok_or(WmiTlvError::BadStructLength)?,
        unit_size: read_le32(value, 4).ok_or(WmiTlvError::BadStructLength)?,
        unit_info: read_le32(value, 8).ok_or(WmiTlvError::BadStructLength)?,
        num_units: read_le32(value, 12).ok_or(WmiTlvError::BadStructLength)?,
    })
}

fn parse_service_ready(payload: &[u8]) -> Result<ServiceReadyEvent, WmiTlvError> {
    if payload.len() < WMI_HEADER_LEN {
        return Err(WmiTlvError::FrameTooShort);
    }
    let event_id = read_le32(payload, 0).ok_or(WmiTlvError::FrameTooShort)?;
    if event_id != EVENT_SERVICE_READY {
        return Err(WmiTlvError::BadEventId);
    }

    let mut service_ready: Option<&[u8]> = None;
    let mut memory_requests = [HostMemoryRequest::EMPTY; MAX_HOST_MEM_REQUESTS];
    let mut memory_request_count = 0usize;

    walk_tlvs(&payload[WMI_HEADER_LEN..], |tlv| {
        match tlv.tag {
            TAG_SERVICE_READY_EVENT => {
                if service_ready.is_some() {
                    return Err(WmiTlvError::DuplicateRequiredTag);
                }
                if tlv.value.len() < SERVICE_READY_MIN_LEN {
                    return Err(WmiTlvError::BadStructLength);
                }
                service_ready = Some(tlv.value);
            }
            // Host-memory records are nested in the ARRAY_STRUCT TLV.
            TAG_ARRAY_STRUCT => {
                walk_tlvs(tlv.value, |nested| {
                    if nested.tag == TAG_WLAN_HOST_MEM_REQ {
                        if memory_request_count == MAX_HOST_MEM_REQUESTS {
                            return Err(WmiTlvError::TooManyHostMemoryRequests);
                        }
                        memory_requests[memory_request_count] =
                            parse_host_memory_request(nested.value)?;
                        memory_request_count += 1;
                    }
                    Ok(())
                })?;
            }
            _ => {}
        }
        Ok(())
    })?;

    let value = service_ready.ok_or(WmiTlvError::MissingServiceReady)?;
    // wmi_tlv_svc_rdy_ev: build version (4), ABI (24), then 14 u32 fields.
    let firmware_build = read_le32(value, 0).ok_or(WmiTlvError::BadStructLength)?;
    let mut abi = [0u32; 6];
    for (index, word) in abi.iter_mut().enumerate() {
        *word = read_le32(value, 4 + index * 4).ok_or(WmiTlvError::BadStructLength)?;
    }
    let num_memory_requests = read_le32(value, 72).ok_or(WmiTlvError::BadStructLength)?;
    if num_memory_requests as usize != memory_request_count {
        return Err(WmiTlvError::HostRequestCountMismatch);
    }

    Ok(ServiceReadyEvent {
        firmware_build,
        abi,
        num_memory_requests,
        memory_requests,
        memory_request_count,
    })
}

fn parse_ready(payload: &[u8]) -> Result<ReadyEvent, WmiTlvError> {
    if payload.len() < WMI_HEADER_LEN {
        return Err(WmiTlvError::FrameTooShort);
    }
    let event_id = read_le32(payload, 0).ok_or(WmiTlvError::FrameTooShort)?;
    if event_id != EVENT_READY {
        return Err(WmiTlvError::BadEventId);
    }

    let mut ready: Option<&[u8]> = None;
    walk_tlvs(&payload[WMI_HEADER_LEN..], |tlv| {
        if tlv.tag == TAG_READY_EVENT {
            if ready.is_some() {
                return Err(WmiTlvError::DuplicateRequiredTag);
            }
            if tlv.value.len() < READY_EVENT_MIN_LEN {
                return Err(WmiTlvError::BadStructLength);
            }
            ready = Some(tlv.value);
        }
        Ok(())
    })?;

    let value = ready.ok_or(WmiTlvError::MissingReady)?;
    let mut abi = [0u32; 6];
    for (index, word) in abi.iter_mut().enumerate() {
        *word = read_le32(value, index * 4).ok_or(WmiTlvError::BadStructLength)?;
    }
    let mut mac_address = [0u8; 6];
    mac_address.copy_from_slice(
        value
            .get(24..30)
            .ok_or(WmiTlvError::BadStructLength)?,
    );
    let status = read_le32(value, 32).ok_or(WmiTlvError::BadStructLength)?;

    Ok(ReadyEvent {
        abi,
        mac_address,
        status,
    })
}

/// Parse one WMI-TLV event payload, including the 32-bit event id at offset 0.
pub fn parse_event(payload: &[u8]) -> Result<WmiTlvEvent, WmiTlvError> {
    if payload.len() < WMI_HEADER_LEN {
        return Err(WmiTlvError::FrameTooShort);
    }
    match read_le32(payload, 0).ok_or(WmiTlvError::FrameTooShort)? {
        EVENT_SERVICE_READY => Ok(WmiTlvEvent::ServiceReady(parse_service_ready(payload)?)),
        EVENT_READY => Ok(WmiTlvEvent::Ready(parse_ready(payload)?)),
        event_id => Ok(WmiTlvEvent::Other(event_id)),
    }
}


/// HTC-backed endpoint for WMI-TLV commands and events.
///
/// Kept separate from the packed WMI endpoint so version-4 firmware cannot
/// accidentally receive the old command/event format.
pub struct WmiTlvEndpoint {
    endpoint: u8,
    max_message_size: u16,
    credits: HtcCredits,
    sequence: u8,
}

impl WmiTlvEndpoint {
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

    pub const fn credits(&self) -> HtcCredits {
        self.credits
    }

    /// Send a WMI-TLV command body. command_id is the WMI-TLV command map id,
    /// not a packed WmiCmdId.
    ///
    /// # Safety
    /// The WMI CE TX pipe must be configured, PCI bus mastering must be enabled,
    /// and the firmware must have completed the HTC service handshake.
    pub unsafe fn send<R: RegisterIo>(
        &mut self,
        tx: &mut TxCe,
        registers: &mut R,
        command_id: u32,
        tlvs: &[u8],
        frame: &mut [u8],
    ) -> Result<(), WmiTlvError> {
        let message_size = WMI_HEADER_LEN
            .checked_add(tlvs.len())
            .ok_or(WmiTlvError::OutputBufferTooSmall)?;
        if message_size > usize::from(self.max_message_size) {
            return Err(WmiTlvError::OutputBufferTooSmall);
        }

        let flags = if self.credits.is_enabled() {
            HTC_FLAG_NEED_CREDIT_UPDATE
        } else {
            0
        };
        let total = encode_htc_tlv_frame(
            frame,
            self.endpoint,
            flags,
            self.sequence,
            command_id,
            tlvs,
        )?;
        self.credits.take(total).map_err(|_| WmiTlvError::NoCredits)?;
        unsafe { tx.send(registers, &frame[..total]) }
            .map_err(|_| WmiTlvError::Transport)?;
        self.sequence = self.sequence.wrapping_add(1);
        Ok(())
    }

    /// Receive one WMI-TLV event from the WMI HTC endpoint.
    ///
    /// # Safety
    /// The WMI CE RX pipe must be configured, PCI bus mastering enabled, and
    /// its DMA pages must remain alive for the entire receive operation.
    pub unsafe fn receive<R: RegisterIo>(
        &mut self,
        rx: &mut RxCe,
        registers: &mut R,
        frame: &mut [u8],
    ) -> Result<WmiTlvEvent, WmiTlvError> {
        let length = unsafe { rx.receive(registers, frame) }
            .map_err(|_| WmiTlvError::Transport)?;
        let bytes = frame.get(..length).ok_or(WmiTlvError::FrameTooShort)?;
        let message = data_message(bytes, self.endpoint).map_err(|error| match error {
            HtcDataError::WrongEndpoint => WmiTlvError::BadEndpoint,
            HtcDataError::BadTrailer => WmiTlvError::BadTrailer,
            HtcDataError::FrameTooShort => WmiTlvError::FrameTooShort,
        })?;

        let mut reports = [HtcCreditReport { endpoint: 0, credits: 0 }; 8];
        let count = parse_credit_reports(message.trailer, &mut reports);
        for report in &reports[..count] {
            if report.endpoint == self.endpoint {
                self.credits.report(report.credits);
            }
        }
        parse_event(message.payload)
    }
}

fn encode_htc_tlv_frame(
    frame: &mut [u8],
    endpoint: u8,
    flags: u8,
    sequence: u8,
    command_id: u32,
    tlvs: &[u8],
) -> Result<usize, WmiTlvError> {
    let total = HTC_HEADER_LENGTH
        .checked_add(WMI_HEADER_LEN)
        .and_then(|length| length.checked_add(tlvs.len()))
        .ok_or(WmiTlvError::OutputBufferTooSmall)?;
    if total > frame.len() {
        return Err(WmiTlvError::OutputBufferTooSmall);
    }

    let id = command_id & 0x00ff_ffff;
    frame[HTC_HEADER_LENGTH..HTC_HEADER_LENGTH + WMI_HEADER_LEN]
        .copy_from_slice(&id.to_le_bytes());
    frame[HTC_HEADER_LENGTH + WMI_HEADER_LEN..total].copy_from_slice(tlvs);
    let payload_len = u16::try_from(WMI_HEADER_LEN + tlvs.len())
        .map_err(|_| WmiTlvError::OutputBufferTooSmall)?;
    write_data_header(frame, endpoint, flags, sequence, payload_len)
        .map_err(|_| WmiTlvError::FrameTooShort)?;
    Ok(total)
}

fn append_tlv(
    output: &mut [u8],
    offset: &mut usize,
    tag: u16,
    value: &[u8],
) -> Result<(), WmiTlvError> {
    let length = u16::try_from(value.len()).map_err(|_| WmiTlvError::OutputBufferTooSmall)?;
    let end = offset
        .checked_add(TLV_HEADER_LEN)
        .and_then(|v| v.checked_add(value.len()))
        .ok_or(WmiTlvError::OutputBufferTooSmall)?;
    if end > output.len() {
        return Err(WmiTlvError::OutputBufferTooSmall);
    }
    write_le16(output, *offset, tag);
    write_le16(output, *offset + 2, length);
    output[*offset + TLV_HEADER_LEN..end].copy_from_slice(value);
    *offset = end;
    Ok(())
}

/// Build the three TLVs required by ath10k's WMI-TLV INIT command.
///
/// This builder advertises zero host-memory chunks. Callers MUST first parse
/// SERVICE_READY and only use this builder when the firmware requested none.
pub fn build_init_payload(output: &mut [u8]) -> Result<usize, WmiTlvError> {
    let mut offset = 0usize;
    let mut init = [0u8; 28];
    for (index, word) in ABI_VERSION.iter().enumerate() {
        write_le32(&mut init, index * 4, *word);
    }
    write_le32(&mut init, 24, 0); // num_host_mem_chunks = 0
    append_tlv(output, &mut offset, TAG_INIT_CMD, &init)?;

    // Conservative one-radio STA resource configuration. All 44 words follow
    // struct wmi_tlv_resource_config order from Linux ath10k.
    let resource_config: [u32; 44] = [
        1,   // num_vdevs
        4,   // num_peers
        0,   // num_offload_peers
        0,   // num_offload_reorder_bufs
        2,   // num_peer_keys
        8,   // num_tids
        0,   // ast_skid_limit
        1,   // tx_chain_mask
        1,   // rx_chain_mask
        100, // rx_timeout_pri[0]
        100, // rx_timeout_pri[1]
        100, // rx_timeout_pri[2]
        40,  // rx_timeout_pri[3]
        0,   // rx_decap_mode
        4,   // scan_max_pending_reqs
        1,   // bmiss_offload_max_vdev
        1,   // roam_offload_max_vdev
        8,   // roam_offload_max_ap_profiles
        0,   // num_mcast_groups
        0,   // num_mcast_table_elems
        0,   // mcast2ucast_mode
        1024,// tx_dbg_log_size
        0,   // num_wds_entries
        0,   // dma_burst_size
        0,   // mac_aggr_delim
        0,   // rx_skip_defrag_timeout_dup_detection_check
        0,   // vow_config
        2,   // gtk_offload_max_vdev
        128, // num_msdu_desc
        2,   // max_frag_entries
        0,   // num_tdls_vdevs
        32,  // num_tdls_conn_table_entries
        2,   // beacon_tx_offload_max_vdev
        5,   // num_multicast_filter_entries
        0,   // num_wow_filters
        6,   // num_keep_alive_pattern
        0,   // keep_alive_pattern_size
        1,   // max_tdls_concurrent_sleep_sta
        1,   // max_tdls_concurrent_buffer_sta
        0,   // wmi_send_separate
        0,   // num_ocb_vdevs
        0,   // num_ocb_channels
        0,   // num_ocb_schedules
        0,   // host_capab
    ];
    let mut resource_bytes = [0u8; 44 * 4];
    for (index, word) in resource_config.iter().enumerate() {
        write_le32(&mut resource_bytes, index * 4, *word);
    }
    append_tlv(
        output,
        &mut offset,
        TAG_RESOURCE_CONFIG,
        &resource_bytes,
    )?;
    append_tlv(output, &mut offset, TAG_ARRAY_STRUCT, &[])?;
    Ok(offset)
}

#[cfg(test)]
mod tests {
    use super::{
        ABI_VERSION, EVENT_READY, EVENT_SERVICE_READY, TAG_ARRAY_STRUCT, TAG_INIT_CMD,
        TAG_READY_EVENT, TAG_RESOURCE_CONFIG, TAG_SERVICE_READY_EVENT, WmiTlvError, WmiTlvEvent,
        build_init_payload, parse_event,
    };

    fn put_tlv(buffer: &mut [u8], offset: &mut usize, tag: u16, value: &[u8]) {
        buffer[*offset..*offset + 2].copy_from_slice(&tag.to_le_bytes());
        buffer[*offset + 2..*offset + 4]
            .copy_from_slice(&(value.len() as u16).to_le_bytes());
        buffer[*offset + 4..*offset + 4 + value.len()].copy_from_slice(value);
        *offset += 4 + value.len();
    }

    #[test]
    fn parses_service_ready_and_abi() {
        let mut frame = [0u8; 160];
        frame[..4].copy_from_slice(&EVENT_SERVICE_READY.to_le_bytes());
        let mut body = [0u8; 104];
        body[0..4].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        for (index, word) in ABI_VERSION.iter().enumerate() {
            body[4 + index * 4..8 + index * 4].copy_from_slice(&word.to_le_bytes());
        }
        let mut offset = 4;
        put_tlv(&mut frame, &mut offset, TAG_SERVICE_READY_EVENT, &body);
        put_tlv(&mut frame, &mut offset, TAG_ARRAY_STRUCT, &[]);
        match parse_event(&frame[..offset]).expect("valid SERVICE_READY") {
            WmiTlvEvent::ServiceReady(event) => {
                assert_eq!(event.firmware_build, 0x1234_5678);
                assert_eq!(event.num_memory_requests, 0);
                assert!(event.abi_is_supported());
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn parses_ready_event_mac_at_tlv_offset() {
        let mut frame = [0u8; 64];
        frame[..4].copy_from_slice(&EVENT_READY.to_le_bytes());
        let mut body = [0u8; 36];
        for (index, word) in ABI_VERSION.iter().enumerate() {
            body[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        body[24..30].copy_from_slice(&[0x02, 0x11, 0x22, 0x33, 0x44, 0x55]);
        let mut offset = 4;
        put_tlv(&mut frame, &mut offset, TAG_READY_EVENT, &body);
        match parse_event(&frame[..offset]).expect("valid READY") {
            WmiTlvEvent::Ready(event) => {
                assert_eq!(event.mac_address, [0x02, 0x11, 0x22, 0x33, 0x44, 0x55]);
                assert_eq!(event.status, 0);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn rejects_truncated_tlv_value() {
        let mut frame = [0u8; 12];
        frame[..4].copy_from_slice(&EVENT_READY.to_le_bytes());
        frame[4..6].copy_from_slice(&TAG_READY_EVENT.to_le_bytes());
        frame[6..8].copy_from_slice(&36u16.to_le_bytes());
        assert_eq!(
            parse_event(&frame),
            Err(WmiTlvError::MalformedTlv)
        );
    }

    #[test]
    fn init_payload_contains_all_required_tlvs() {
        let mut payload = [0u8; 256];
        let length = build_init_payload(&mut payload).expect("buffer is large enough");
        assert_eq!(length, 216);
        assert_eq!(u16::from_le_bytes([payload[0], payload[1]]), TAG_INIT_CMD);
        assert_eq!(u16::from_le_bytes([payload[32], payload[33]]), TAG_RESOURCE_CONFIG);
        assert_eq!(u16::from_le_bytes([payload[212], payload[213]]), TAG_ARRAY_STRUCT);
        assert_eq!(u16::from_le_bytes([payload[214], payload[215]]), 0);
    }
}
