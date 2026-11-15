pub(crate) const HTC_HEADER_LENGTH: usize = 8;
const HTC_CONTROL_ENDPOINT: u8 = 0;
const HTC_MAX_SERVICE_ENDPOINTS: usize = 7;
const HTC_READY_MESSAGE_ID: u16 = 1;
const HTC_CONNECT_SERVICE_MESSAGE_ID: u16 = 2;
const HTC_CONNECT_SERVICE_RESPONSE_ID: u16 = 3;
const HTC_SETUP_COMPLETE_MESSAGE_ID: u16 = 4;
// Extended setup complete (used when credit flow control info is needed)
const HTC_SETUP_COMPLETE_EX_MESSAGE_ID: u16 = 5;

// HTC header flags
pub const HTC_FLAG_NEED_CREDIT_UPDATE: u8 = 0x01;
pub const HTC_FLAG_RECV_TRAILER: u8 = 0x02;

// HTC trailer item tags
const HTC_TRAILER_TAG_CREDIT_REPORT: u8 = 1;

pub const HTC_SERVICE_WMI_CONTROL: u16 = 0x0100;
pub const HTC_SERVICE_WMI_DATA_BEST_EFFORT: u16 = 0x0101;
pub const HTC_SERVICE_HTT_DATA: u16 = 0x0300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HtcError {
    HeaderTooShort,
    PayloadLengthMismatch,
    PayloadTooLarge,
    OutputBufferTooSmall,
    UnexpectedEndpoint,
    UnexpectedMessage,
    ServiceMismatch,
    ServiceRejected(u8),
    InvalidReady,
    InvalidEndpoint,
    NotReady,
    AlreadyReady,
    DuplicateService,
    EndpointTableFull,
    EndpointNotConnected,
    MessageTooLarge,
    /// The endpoint has fewer transmit credits than this frame needs.
    NoCredits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HtcHeader {
    endpoint: u8,
    flags: u8,
    control: [u8; 4],
}

impl HtcHeader {
    pub const fn new(endpoint: u8, flags: u8, control: [u8; 4]) -> Self {
        Self {
            endpoint,
            flags,
            control,
        }
    }

    pub const fn endpoint(self) -> u8 {
        self.endpoint
    }

    pub const fn flags(self) -> u8 {
        self.flags
    }

    pub const fn control(self) -> [u8; 4] {
        self.control
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HtcPacket<'a> {
    pub header: HtcHeader,
    pub payload: &'a [u8],
}

impl<'a> HtcPacket<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, HtcError> {
        let header_bytes = bytes.get(..HTC_HEADER_LENGTH).ok_or(HtcError::HeaderTooShort)?;
        let payload_length =
            usize::from(u16::from_le_bytes([header_bytes[2], header_bytes[3]]));
        let expected_length = HTC_HEADER_LENGTH
            .checked_add(payload_length)
            .ok_or(HtcError::PayloadTooLarge)?;
        if bytes.len() != expected_length {
            return Err(HtcError::PayloadLengthMismatch);
        }

        let mut control = [0; 4];
        control.copy_from_slice(&header_bytes[4..8]);
        Ok(Self {
            header: HtcHeader::new(header_bytes[0], header_bytes[1], control),
            payload: &bytes[HTC_HEADER_LENGTH..],
        })
    }

    pub fn encode_into(&self, output: &mut [u8]) -> Result<usize, HtcError> {
        let payload_length =
            u16::try_from(self.payload.len()).map_err(|_| HtcError::PayloadTooLarge)?;
        let frame_length = HTC_HEADER_LENGTH
            .checked_add(self.payload.len())
            .ok_or(HtcError::PayloadTooLarge)?;
        if output.len() < frame_length {
            return Err(HtcError::OutputBufferTooSmall);
        }

        output[0] = self.header.endpoint;
        output[1] = self.header.flags;
        output[2..4].copy_from_slice(&payload_length.to_le_bytes());
        output[4..8].copy_from_slice(&self.header.control);
        output[HTC_HEADER_LENGTH..frame_length].copy_from_slice(self.payload);
        Ok(frame_length)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HtcReady {
    pub credit_count: u16,
    pub credit_size: u16,
    pub max_endpoints: u8,
}

/// Parse an HTC READY message from the target.
///
/// The HTC spec requires a minimum payload of 8 bytes; extended versions of the
/// ready message may append additional fields that this driver does not yet use.
/// We therefore accept `payload.len() >= 8` rather than `== 8` and silently
/// ignore any trailing bytes.
pub fn parse_ready(packet: HtcPacket<'_>) -> Result<HtcReady, HtcError> {
    if packet.header.endpoint != HTC_CONTROL_ENDPOINT {
        return Err(HtcError::UnexpectedEndpoint);
    }
    // Require at least 8 bytes; ath10k only mandates the minimum length.
    // Extended ready messages may carry additional fields — ignore them.
    if packet.payload.len() < 8
        || u16::from_le_bytes([packet.payload[0], packet.payload[1]]) != HTC_READY_MESSAGE_ID
    {
        return Err(HtcError::UnexpectedMessage);
    }
    Ok(HtcReady {
        credit_count: u16::from_le_bytes([packet.payload[2], packet.payload[3]]),
        credit_size: u16::from_le_bytes([packet.payload[4], packet.payload[5]]),
        max_endpoints: packet.payload[6],
    })
}

pub fn connect_service_request(
    service_id: u16,
    flags: u16,
) -> Result<[u8; HTC_HEADER_LENGTH + 8], HtcError> {
    let payload = [
        HTC_CONNECT_SERVICE_MESSAGE_ID.to_le_bytes()[0],
        HTC_CONNECT_SERVICE_MESSAGE_ID.to_le_bytes()[1],
        service_id.to_le_bytes()[0],
        service_id.to_le_bytes()[1],
        flags.to_le_bytes()[0],
        flags.to_le_bytes()[1],
        0,
        0,
    ];
    let packet = HtcPacket {
        header: HtcHeader::new(HTC_CONTROL_ENDPOINT, 0, [0; 4]),
        payload: &payload,
    };
    let mut output = [0; HTC_HEADER_LENGTH + 8];
    packet.encode_into(&mut output)?;
    Ok(output)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HtcConnectResponse {
    pub endpoint: u8,
    pub max_message_size: u16,
}

/// Parse an HTC CONNECT_SERVICE_RESPONSE from the target.
///
/// The response payload is at least 8 bytes; extended responses may append
/// additional fields. We accept `payload.len() >= 8` and ignore any trailing
/// bytes, matching the ath10k minimum-length requirement.
pub fn parse_connect_response(
    packet: HtcPacket<'_>,
    expected_service: u16,
) -> Result<HtcConnectResponse, HtcError> {
    if packet.header.endpoint != HTC_CONTROL_ENDPOINT {
        return Err(HtcError::UnexpectedEndpoint);
    }
    // Require at least 8 bytes; ignore any extra bytes in extended responses.
    if packet.payload.len() < 8
        || u16::from_le_bytes([packet.payload[0], packet.payload[1]])
            != HTC_CONNECT_SERVICE_RESPONSE_ID
    {
        return Err(HtcError::UnexpectedMessage);
    }

    let service_id = u16::from_le_bytes([packet.payload[2], packet.payload[3]]);
    if service_id != expected_service {
        return Err(HtcError::ServiceMismatch);
    }
    let status = packet.payload[4];
    if status != 0 {
        return Err(HtcError::ServiceRejected(status));
    }

    Ok(HtcConnectResponse {
        endpoint: packet.payload[5],
        max_message_size: u16::from_le_bytes([packet.payload[6], packet.payload[7]]),
    })
}

/// Build a SETUP_COMPLETE frame (HTC message ID 4) addressed to EP0.
///
/// The host must send this after all services have been connected, before WMI
/// commands can flow.  The payload is two bytes: the message ID in little-endian.
pub fn setup_complete_request() -> [u8; HTC_HEADER_LENGTH + 2] {
    let payload = HTC_SETUP_COMPLETE_MESSAGE_ID.to_le_bytes();
    let packet = HtcPacket {
        header: HtcHeader::new(HTC_CONTROL_ENDPOINT, 0, [0; 4]),
        payload: &payload,
    };
    let mut output = [0u8; HTC_HEADER_LENGTH + 2];
    // encode_into can only fail if the buffer is too small or payload is too
    // large; neither applies to this fixed-size buffer.
    let _ = packet.encode_into(&mut output);
    output
}

/// Byte budget of one HTC transmit credit.
///
/// `ath10k_htc_consume_credit` divides the frame length (HTC header included)
/// by the credit size reported in HTC READY and refuses to send when the
/// endpoint has fewer credits than that; the target returns credits in trailer
/// records of the frames it sends to us.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HtcCredits {
    credit_size: u16,
    available: u16,
}

impl HtcCredits {
    /// Flow control disabled (nothing is accounted).
    pub const fn disabled() -> Self {
        Self {
            credit_size: 0,
            available: 0,
        }
    }

    pub const fn new(credit_size: u16, available: u16) -> Self {
        Self {
            credit_size,
            available,
        }
    }

    pub const fn is_enabled(&self) -> bool {
        self.credit_size != 0
    }

    pub const fn available(&self) -> u16 {
        self.available
    }

    pub const fn credit_size(&self) -> u16 {
        self.credit_size
    }

    /// Credits a frame of `frame_len` bytes (HTC header included) needs.
    pub fn required(&self, frame_len: usize) -> u16 {
        if !self.is_enabled() {
            return 0;
        }
        let size = usize::from(self.credit_size);
        ((frame_len + size - 1) / size) as u16
    }

    /// Account for a frame about to be sent.
    pub fn take(&mut self, frame_len: usize) -> Result<(), HtcError> {
        let needed = self.required(frame_len);
        if needed > self.available {
            return Err(HtcError::NoCredits);
        }
        self.available -= needed;
        Ok(())
    }

    /// Add credits the target reported back.
    pub fn report(&mut self, credits: u8) {
        self.available = self.available.saturating_add(u16::from(credits));
    }
}

/// One credit report entry (`struct ath10k_htc_credit_report`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HtcCreditReport {
    pub endpoint: u8,
    pub credits: u8,
}

/// `struct ath10k_ath10k_htc_record_hdr`.
pub const HTC_RECORD_HEADER_LEN: usize = 4;
/// `struct ath10k_htc_credit_report`.
pub const HTC_CREDIT_REPORT_LEN: usize = 4;

/// Parse the credit reports of an HTC receive trailer.
///
/// A trailer is a sequence of records `{ id, len, pad[2] }` plus `len` bytes of
/// body; a credits record holds `len / 4` credit reports. Returns how many
/// reports were written into `out`.
pub fn parse_credit_reports(trailer: &[u8], out: &mut [HtcCreditReport]) -> usize {
    let mut written = 0;
    let mut offset = 0;
    while offset + HTC_RECORD_HEADER_LEN <= trailer.len() && written < out.len() {
        let id = trailer[offset];
        let length = usize::from(trailer[offset + 1]);
        let body_start = offset + HTC_RECORD_HEADER_LEN;
        let Some(body_end) = body_start.checked_add(length) else {
            break;
        };
        if body_end > trailer.len() {
            break;
        }
        if id == HTC_TRAILER_TAG_CREDIT_REPORT {
            let body = &trailer[body_start..body_end];
            let mut entry = 0;
            while entry + HTC_CREDIT_REPORT_LEN <= body.len() && written < out.len() {
                out[written] = HtcCreditReport {
                    endpoint: body[entry],
                    credits: body[entry + 1],
                };
                written += 1;
                entry += HTC_CREDIT_REPORT_LEN;
            }
        }
        offset = body_end;
    }
    written
}

/// An HTC data message split into its payload and credit trailer.
pub struct HtcDataMessage<'a> {
    pub payload: &'a [u8],
    /// Trailer records, without the length byte that closes the trailer.
    pub trailer: &'a [u8],
}

/// Split an incoming HTC data message into payload and credit trailer.
pub fn data_message(bytes: &[u8], endpoint: u8) -> Result<HtcDataMessage<'_>, HtcDataError> {
    let packet = HtcPacket::parse(bytes).map_err(|_| HtcDataError::FrameTooShort)?;
    if packet.header.endpoint != endpoint {
        return Err(HtcDataError::WrongEndpoint);
    }
    let mut payload = packet.payload;
    let mut trailer: &[u8] = &[];
    if packet.header.flags & HTC_FLAG_RECV_TRAILER != 0 {
        let trailer_len = usize::from(*payload.last().ok_or(HtcDataError::BadTrailer)?);
        if trailer_len == 0 || trailer_len > payload.len() {
            return Err(HtcDataError::BadTrailer);
        }
        let start = payload.len() - trailer_len;
        // The trailer's last byte is its own length, not part of a record.
        trailer = &payload[start..payload.len() - 1];
        payload = &payload[..start];
    }
    Ok(HtcDataMessage { payload, trailer })
}

/// Write an HTC data-message header: `[endpoint | flags | len(2) | control(4)]`.
pub fn write_data_header(
    frame: &mut [u8],
    endpoint: u8,
    flags: u8,
    sequence: u8,
    payload_len: u16,
) -> Result<(), HtcError> {
    if frame.len() < HTC_HEADER_LENGTH {
        return Err(HtcError::OutputBufferTooSmall);
    }
    frame[0] = endpoint;
    frame[1] = flags;
    frame[2..4].copy_from_slice(&payload_len.to_le_bytes());
    frame[4] = sequence;
    frame[5..HTC_HEADER_LENGTH].fill(0);
    Ok(())
}

/// Frame a fully built protocol message (WMI, HTT, …) as an HTC data message.
/// Returns the total frame length.
pub fn encode_data_message(
    frame: &mut [u8],
    endpoint: u8,
    flags: u8,
    sequence: u8,
    message: &[u8],
) -> Result<usize, HtcError> {
    let total = HTC_HEADER_LENGTH + message.len();
    if total > frame.len() {
        return Err(HtcError::OutputBufferTooSmall);
    }
    let payload_len = u16::try_from(message.len()).map_err(|_| HtcError::PayloadTooLarge)?;
    write_data_header(frame, endpoint, flags, sequence, payload_len)?;
    frame[HTC_HEADER_LENGTH..total].copy_from_slice(message);
    Ok(total)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HtcDataError {
    /// The message is shorter than its header or declares a different length.
    FrameTooShort,
    /// The message belongs to an HTC endpoint other than the expected one.
    WrongEndpoint,
    /// The credit trailer length is missing or inconsistent.
    BadTrailer,
}

pub trait HtcTransport {
    type Error;

    fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error>;
    fn receive(&mut self, frame: &mut [u8]) -> Result<usize, Self::Error>;
}

/// Extension to access the underlying transport from `HtcClient`.
/// Used by WMI and HTT layers that need to receive raw frames.
pub trait HtcClientTransportExt {
    type T: HtcTransport;
    fn transport_mut(&mut self) -> &mut Self::T;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HtcServiceEndpoint {
    service_id: u16,
    endpoint: u8,
    max_message_size: u16,
    /// Transmit credits this endpoint still has.
    credits: HtcCredits,
}

#[derive(Debug, PartialEq, Eq)]
pub enum HtcClientError<E> {
    Transport(E),
    Protocol(HtcError),
}

pub struct HtcClient<T> {
    transport: T,
    ready: Option<HtcReady>,
    endpoints: [Option<HtcServiceEndpoint>; HTC_MAX_SERVICE_ENDPOINTS],
    /// Credits for EP0 (connect requests, SETUP_COMPLETE).
    control_credits: HtcCredits,
    /// Outgoing sequence number, incremented with each sent data frame.
    send_seq: u8,
}

impl<T: HtcTransport> HtcClient<T> {
    pub const fn new(transport: T) -> Self {
        Self {
            transport,
            ready: None,
            endpoints: [None; HTC_MAX_SERVICE_ENDPOINTS],
            control_credits: HtcCredits::disabled(),
            send_seq: 0,
        }
    }

    pub fn wait_ready(
        &mut self,
        frame: &mut [u8],
    ) -> Result<HtcReady, HtcClientError<T::Error>> {
        if self.ready.is_some() {
            return Err(HtcClientError::Protocol(HtcError::AlreadyReady));
        }
        let packet = self.receive_packet(frame)?;
        let ready = parse_ready(packet).map_err(HtcClientError::Protocol)?;
        if ready.max_endpoints <= 1 || ready.credit_size == 0 {
            return Err(HtcClientError::Protocol(HtcError::InvalidReady));
        }
        // HTC READY advertises the credit size and the total credit count; from
        // here on every send costs credits and trailers replenish them.
        self.control_credits = HtcCredits::new(ready.credit_size, ready.credit_count);
        self.ready = Some(ready);
        Ok(ready)
    }

    pub fn connect_service(
        &mut self,
        service_id: u16,
        flags: u16,
        frame: &mut [u8],
    ) -> Result<HtcConnectResponse, HtcClientError<T::Error>> {
        let ready = self
            .ready
            .ok_or(HtcClientError::Protocol(HtcError::NotReady))?;
        if self
            .endpoints
            .iter()
            .flatten()
            .any(|endpoint| endpoint.service_id == service_id)
        {
            return Err(HtcClientError::Protocol(HtcError::DuplicateService));
        }

        let request =
            connect_service_request(service_id, flags).map_err(HtcClientError::Protocol)?;
        self.control_credits
            .take(request.len())
            .map_err(HtcClientError::Protocol)?;
        self.transport
            .send(&request)
            .map_err(HtcClientError::Transport)?;
        let packet = self.receive_packet(frame)?;
        let response =
            parse_connect_response(packet, service_id).map_err(HtcClientError::Protocol)?;
        if response.endpoint == HTC_CONTROL_ENDPOINT
            || response.endpoint >= ready.max_endpoints
            || response.max_message_size == 0
        {
            return Err(HtcClientError::Protocol(HtcError::InvalidEndpoint));
        }
        if self.endpoints.iter().flatten().any(|endpoint| {
            endpoint.endpoint == response.endpoint
        }) {
            return Err(HtcClientError::Protocol(HtcError::InvalidEndpoint));
        }
        // Every endpoint starts with the pool HTC READY advertised; credit reports
        // in receive trailers replenish it afterwards. This driver drives one
        // endpoint at a time, so the pool is not split between them.
        let slot = self
            .endpoints
            .iter_mut()
            .find(|endpoint| endpoint.is_none())
            .ok_or(HtcClientError::Protocol(HtcError::EndpointTableFull))?;
        *slot = Some(HtcServiceEndpoint {
            service_id,
            endpoint: response.endpoint,
            max_message_size: response.max_message_size,
            credits: HtcCredits::new(ready.credit_size, ready.credit_count),
        });
        Ok(response)
    }

    pub fn send_service(
        &mut self,
        service_id: u16,
        flags: u8,
        payload: &[u8],
        frame: &mut [u8],
    ) -> Result<(), HtcClientError<T::Error>> {
        let endpoint = self
            .endpoints
            .iter_mut()
            .flatten()
            .find(|endpoint| endpoint.service_id == service_id)
            .ok_or(HtcClientError::Protocol(HtcError::EndpointNotConnected))?;
        if payload.len() > usize::from(endpoint.max_message_size) {
            return Err(HtcClientError::Protocol(HtcError::MessageTooLarge));
        }

        // Stamp the outgoing sequence number into control[0].
        let seq = self.send_seq;
        self.send_seq = self.send_seq.wrapping_add(1);

        let mut control = [0u8; 4];
        control[0] = seq;

        // Ask the target for a credit report whenever credits are accounted for.
        let flags = if endpoint.credits.is_enabled() {
            flags | HTC_FLAG_NEED_CREDIT_UPDATE
        } else {
            flags
        };
        let ep_id = endpoint.endpoint;
        let packet = HtcPacket {
            header: HtcHeader::new(ep_id, flags, control),
            payload,
        };
        let length = packet
            .encode_into(frame)
            .map_err(HtcClientError::Protocol)?;
        endpoint
            .credits
            .take(length)
            .map_err(HtcClientError::Protocol)?;
        self.transport
            .send(&frame[..length])
            .map_err(HtcClientError::Transport)
    }

    /// Send an HTC SETUP_COMPLETE message to EP0.
    ///
    /// Must be called after all services are connected and before any WMI
    /// commands are issued.  Returns `NotReady` if `wait_ready` has not yet
    /// been called.
    pub fn send_setup_complete(
        &mut self,
        frame: &mut [u8],
    ) -> Result<(), HtcClientError<T::Error>> {
        if self.ready.is_none() {
            return Err(HtcClientError::Protocol(HtcError::NotReady));
        }
        let msg = setup_complete_request();
        if frame.len() < msg.len() {
            return Err(HtcClientError::Protocol(HtcError::OutputBufferTooSmall));
        }
        frame[..msg.len()].copy_from_slice(&msg);
        self.control_credits
            .take(msg.len())
            .map_err(HtcClientError::Protocol)?;
        self.transport
            .send(&frame[..msg.len()])
            .map_err(HtcClientError::Transport)
    }

    fn report_credits(&mut self, endpoint: u8, credits: u8) {
        if endpoint == HTC_CONTROL_ENDPOINT {
            self.control_credits.report(credits);
            return;
        }
        if let Some(entry) = self
            .endpoints
            .iter_mut()
            .flatten()
            .find(|entry| entry.endpoint == endpoint)
        {
            entry.credits.report(credits);
        }
    }

    pub fn receive<'a>(
        &mut self,
        frame: &'a mut [u8],
    ) -> Result<HtcPacket<'a>, HtcClientError<T::Error>> {
        let packet = self.receive_packet(frame)?;
        if packet.header.endpoint != HTC_CONTROL_ENDPOINT {
            let ready = self.ready.ok_or(HtcClientError::Protocol(HtcError::NotReady))?;
            if packet.header.endpoint >= ready.max_endpoints
                || !self.endpoints.iter().flatten().any(|endpoint| {
                    endpoint.endpoint == packet.header.endpoint
                })
            {
                return Err(HtcClientError::Protocol(HtcError::EndpointNotConnected));
            }
        }
        Ok(packet)
    }

    pub fn into_transport(self) -> T {
        self.transport
    }

    /// Borrow the underlying transport mutably (used by WMI/HTT to receive
    /// raw frames outside the HTC framing path, e.g. after the handshake).
    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    fn receive_packet<'a>(
        &mut self,
        frame: &'a mut [u8],
    ) -> Result<HtcPacket<'a>, HtcClientError<T::Error>> {
        let length = self
            .transport
            .receive(frame)
            .map_err(HtcClientError::Transport)?;
        let received = frame
            .get(..length)
            .ok_or(HtcClientError::Protocol(HtcError::OutputBufferTooSmall))?;
        let mut packet = HtcPacket::parse(received).map_err(HtcClientError::Protocol)?;

        // Fix 5: strip trailer if HTC_FLAG_RECV_TRAILER is set.
        // Trailer format: items..., last_byte = total trailer length in bytes.
        // We strip the trailer from the payload so callers never see it.
        if packet.header.flags & HTC_FLAG_RECV_TRAILER != 0 {
            let payload = packet.payload;
            if payload.is_empty() {
                return Err(HtcClientError::Protocol(HtcError::UnexpectedMessage));
            }
            let trailer_len = usize::from(*payload.last().unwrap());
            // trailer_len counts itself, so must be >= 1; must not exceed payload.
            if trailer_len == 0 || trailer_len > payload.len() {
                return Err(HtcClientError::Protocol(HtcError::UnexpectedMessage));
            }
            let stripped_len = payload.len() - trailer_len;
            // The trailer's last byte is its own length, not part of a record.
            let mut reports = [HtcCreditReport {
                endpoint: 0,
                credits: 0,
            }; 8];
            let count = parse_credit_reports(&payload[stripped_len..payload.len() - 1], &mut reports);
            for report in &reports[..count] {
                self.report_credits(report.endpoint, report.credits);
            }
            packet.payload = &payload[..stripped_len];
        }

        Ok(packet)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HTC_FLAG_NEED_CREDIT_UPDATE, HTC_FLAG_RECV_TRAILER, HTC_HEADER_LENGTH,
        HTC_SERVICE_WMI_CONTROL, HTC_SETUP_COMPLETE_MESSAGE_ID, HtcClient, HtcClientError,
        HtcDataError, HtcError, HtcHeader,
        HtcCreditReport, HtcCredits, HtcPacket, HtcTransport, connect_service_request, data_message,
        encode_data_message, parse_connect_response, parse_credit_reports, parse_ready,
        setup_complete_request,
    };
    use alloc::vec::Vec;

    #[test]
    fn credits_are_byte_based_and_replenished_by_reports() {
        // 2048-byte credits: a 100-byte frame needs one, a 4096-byte frame two.
        let mut credits = HtcCredits::new(2048, 3);
        assert_eq!(credits.required(100), 1);
        assert_eq!(credits.required(2048), 1);
        assert_eq!(credits.required(2049), 2);

        credits.take(100).expect("one credit");
        assert_eq!(credits.available(), 2);
        credits.take(4096).expect("two credits");
        assert_eq!(credits.available(), 0);
        assert_eq!(credits.take(1), Err(HtcError::NoCredits));

        credits.report(5);
        assert_eq!(credits.available(), 5);

        // Flow control disabled accounts for nothing.
        let mut disabled = HtcCredits::disabled();
        assert_eq!(disabled.take(1 << 20), Ok(()));
        assert_eq!(disabled.required(1 << 20), 0);
    }

    #[test]
    fn credit_reports_are_read_from_the_trailer_records() {
        // A credits record (id 1, eight-byte body) with two reports followed by an
        // unknown record (id 2) that must be skipped.
        let trailer = [
            0x01u8, 0x08, 0x00, 0x00, // credits record, 8-byte body
            0x03, 0x05, 0x00, 0x00, // endpoint 3, 5 credits
            0x01, 0x02, 0x00, 0x00, // endpoint 1, 2 credits
            0x02, 0x04, 0x00, 0x00, // lookahead record, 4-byte body
            0xaa, 0xbb, 0xcc, 0xdd,
        ];
        let mut reports = [HtcCreditReport {
            endpoint: 0,
            credits: 0,
        }; 8];
        let count = parse_credit_reports(&trailer, &mut reports);
        assert_eq!(count, 2);
        assert_eq!(
            reports[0],
            HtcCreditReport {
                endpoint: 3,
                credits: 5,
            }
        );
        assert_eq!(
            reports[1],
            HtcCreditReport {
                endpoint: 1,
                credits: 2,
            }
        );
    }

    #[test]
    fn data_message_wraps_the_payload_for_the_requested_endpoint() {
        let mut frame = [0u8; 32];
        let total = encode_data_message(&mut frame, 3, 0, 9, &[0x00, 0x00, 0x00, 0x00])
            .expect("frame fits");

        assert_eq!(total, HTC_HEADER_LENGTH + 4);
        assert_eq!(&frame[..HTC_HEADER_LENGTH], &[3, 0, 4, 0, 9, 0, 0, 0]);
        assert_eq!(&frame[HTC_HEADER_LENGTH..total], &[0, 0, 0, 0]);
        assert_eq!(
            data_message(&frame[..total], 3).expect("payload").payload,
            &[0, 0, 0, 0][..]
        );
        assert_eq!(
            data_message(&frame[..total], 2).err(),
            Some(HtcDataError::WrongEndpoint)
        );
    }

    #[derive(Default)]
    struct MockTransport {
        sent: Vec<Vec<u8>>,
        received: Vec<Vec<u8>>,
    }

    impl HtcTransport for MockTransport {
        type Error = &'static str;

        fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error> {
            self.sent.push(frame.into());
            Ok(())
        }

        fn receive(&mut self, frame: &mut [u8]) -> Result<usize, Self::Error> {
            let packet = self.received.first().ok_or("no queued HTC frame")?;
            if frame.len() < packet.len() {
                return Err("HTC frame buffer too small");
            }
            let packet = self.received.remove(0);
            frame[..packet.len()].copy_from_slice(&packet);
            Ok(packet.len())
        }
    }

    #[test]
    fn encodes_and_parses_a_packet() {
        let packet = HtcPacket {
            header: HtcHeader::new(2, 1, [3, 4, 5, 6]),
            payload: &[0xaa, 0xbb, 0xcc],
        };
        let mut encoded = [0; 11];
        assert_eq!(packet.encode_into(&mut encoded), Ok(encoded.len()));
        assert_eq!(HtcPacket::parse(&encoded), Ok(packet));
    }

    #[test]
    fn rejects_truncated_and_inconsistent_frames() {
        assert_eq!(HtcPacket::parse(&[0; 7]), Err(HtcError::HeaderTooShort));
        assert_eq!(
            HtcPacket::parse(&[0, 0, 1, 0, 0, 0, 0, 0]),
            Err(HtcError::PayloadLengthMismatch)
        );
    }

    #[test]
    fn refuses_to_encode_into_a_short_buffer() {
        let packet = HtcPacket {
            header: HtcHeader::new(0, 0, [0; 4]),
            payload: &[1],
        };
        assert_eq!(
            packet.encode_into(&mut [0; 8]),
            Err(HtcError::OutputBufferTooSmall)
        );
    }

    #[test]
    fn encodes_service_request_and_parses_target_ready() {
        let request = connect_service_request(HTC_SERVICE_WMI_CONTROL, 0).unwrap();
        assert_eq!(&request[8..12], &[2, 0, 0, 1]);

        // Existing test: exact 8-byte payload still works (>= 8 accepts 8).
        let ready = HtcPacket::parse(&[0, 0, 8, 0, 0, 0, 0, 0, 1, 0, 32, 0, 64, 0, 8, 0])
            .unwrap();
        assert_eq!(
            parse_ready(ready),
            Ok(super::HtcReady {
                credit_count: 32,
                credit_size: 64,
                max_endpoints: 8,
            })
        );
    }

    #[test]
    fn parses_service_connection_response() {
        // Existing test: exact 8-byte payload still works (>= 8 accepts 8).
        let response = HtcPacket::parse(&[0, 0, 8, 0, 0, 0, 0, 0, 3, 0, 0, 1, 0, 2, 0, 8])
            .unwrap();
        assert_eq!(
            parse_connect_response(response, HTC_SERVICE_WMI_CONTROL),
            Ok(super::HtcConnectResponse {
                endpoint: 2,
                max_message_size: 2048,
            })
        );
    }

    #[test]
    fn negotiates_ready_and_connects_a_service_before_sending_data() {
        let transport = MockTransport {
            received: alloc::vec![
                alloc::vec![0, 0, 8, 0, 0, 0, 0, 0, 1, 0, 32, 0, 64, 0, 8, 0],
                alloc::vec![0, 0, 8, 0, 0, 0, 0, 0, 3, 0, 0, 1, 0, 2, 0, 8],
            ],
            ..MockTransport::default()
        };
        let mut client = HtcClient::new(transport);
        let mut frame = [0; 32];
        assert!(client.wait_ready(&mut frame).is_ok());
        assert_eq!(
            client.connect_service(HTC_SERVICE_WMI_CONTROL, 0, &mut frame),
            Ok(super::HtcConnectResponse {
                endpoint: 2,
                max_message_size: 2048,
            })
        );
        assert_eq!(
            client.send_service(HTC_SERVICE_WMI_CONTROL, 0, &[0xaa], &mut frame),
            Ok(())
        );

        let transport = client.into_transport();
        assert_eq!(&transport.sent[0][8..12], &[2, 0, 0, 1]);
        // seq_no is now in control[0], the frame asks the target for a credit
        // report, and the data byte is still at offset 8.
        assert_eq!(
            &transport.sent[1][..9],
            &[2, HTC_FLAG_NEED_CREDIT_UPDATE, 1, 0, 0, 0, 0, 0, 0xaa]
        );
    }

    #[test]
    fn refuses_service_operations_until_ready_and_rejects_bad_endpoints() {
        let transport = MockTransport::default();
        let mut client = HtcClient::new(transport);
        assert_eq!(
            client.connect_service(HTC_SERVICE_WMI_CONTROL, 0, &mut [0; 16]),
            Err(HtcClientError::Protocol(HtcError::NotReady))
        );

        let transport = MockTransport {
            received: alloc::vec![
                alloc::vec![0, 0, 8, 0, 0, 0, 0, 0, 1, 0, 32, 0, 64, 0, 8, 0],
                alloc::vec![0, 0, 8, 0, 0, 0, 0, 0, 3, 0, 0, 1, 0, 8, 0, 8],
            ],
            ..MockTransport::default()
        };
        let mut client = HtcClient::new(transport);
        let mut frame = [0; 16];
        client.wait_ready(&mut frame).unwrap();
        assert_eq!(
            client.connect_service(HTC_SERVICE_WMI_CONTROL, 0, &mut frame),
            Err(HtcClientError::Protocol(HtcError::InvalidEndpoint))
        );
    }

    // --- New tests ---

    /// parse_ready accepts a payload longer than 8 bytes (extended ready message).
    #[test]
    fn parse_ready_accepts_extended_payload() {
        // 10-byte payload: standard 8 bytes + 2 extra (ignored extension bytes)
        let frame: [u8; HTC_HEADER_LENGTH + 10] = [
            0, 0,       // endpoint=0, flags=0
            10, 0,      // payload_length = 10 (LE)
            0, 0, 0, 0, // control
            // payload (10 bytes):
            1, 0,       // message_id = HTC_READY_MESSAGE_ID
            16, 0,      // credit_count = 16
            128, 0,     // credit_size = 128
            4,          // max_endpoints = 4
            0,          // reserved (payload[7])
            0xAB, 0xCD, // extension bytes – must be silently ignored
        ];
        let packet = HtcPacket::parse(&frame).unwrap();
        assert_eq!(
            parse_ready(packet),
            Ok(super::HtcReady {
                credit_count: 16,
                credit_size: 128,
                max_endpoints: 4,
            })
        );
    }

    /// parse_connect_response accepts a payload longer than 8 bytes.
    #[test]
    fn parse_connect_response_accepts_extended_payload() {
        // 10-byte payload: standard 8 bytes + 2 extra (ignored extension bytes)
        let frame: [u8; HTC_HEADER_LENGTH + 10] = [
            0, 0,       // endpoint=0, flags=0
            10, 0,      // payload_length = 10 (LE)
            0, 0, 0, 0, // control
            // payload (10 bytes):
            3, 0,       // message_id = HTC_CONNECT_SERVICE_RESPONSE_ID
            0, 1,       // service_id = HTC_SERVICE_WMI_CONTROL (0x0100)
            0,          // status = 0 (success)
            3,          // assigned endpoint = 3
            0, 8,       // max_message_size = 2048 (LE: 0x0800)
            0xDE, 0xAD, // extension bytes – must be silently ignored
        ];
        let packet = HtcPacket::parse(&frame).unwrap();
        assert_eq!(
            parse_connect_response(packet, HTC_SERVICE_WMI_CONTROL),
            Ok(super::HtcConnectResponse {
                endpoint: 3,
                max_message_size: 2048,
            })
        );
    }

    /// setup_complete_request encodes the correct message ID on EP0.
    #[test]
    fn setup_complete_request_encodes_correctly() {
        let frame = setup_complete_request();
        // Frame layout: [endpoint, flags, payload_len_lo, payload_len_hi,
        //                ctrl0, ctrl1, ctrl2, ctrl3, msg_id_lo, msg_id_hi]
        assert_eq!(frame[0], 0, "must target EP0");
        assert_eq!(frame[1], 0, "flags must be 0");
        assert_eq!(u16::from_le_bytes([frame[2], frame[3]]), 2, "payload_len must be 2");
        let msg_id = u16::from_le_bytes([frame[8], frame[9]]);
        assert_eq!(msg_id, HTC_SETUP_COMPLETE_MESSAGE_ID);
    }

    /// send_setup_complete transmits the correct frame to EP0 via the transport.
    #[test]
    fn send_setup_complete_sends_to_ep0() {
        // We need a ready message so that send_setup_complete does not bail out.
        let transport = MockTransport {
            received: alloc::vec![
                alloc::vec![0, 0, 8, 0, 0, 0, 0, 0, 1, 0, 32, 0, 64, 0, 8, 0],
            ],
            ..MockTransport::default()
        };
        let mut client = HtcClient::new(transport);
        let mut frame = [0u8; 32];
        client.wait_ready(&mut frame).unwrap();
        assert_eq!(client.send_setup_complete(&mut frame), Ok(()));

        let transport = client.into_transport();
        assert_eq!(transport.sent.len(), 1);
        let sent = &transport.sent[0];
        assert_eq!(sent[0], 0, "must be addressed to EP0");
        let msg_id = u16::from_le_bytes([sent[8], sent[9]]);
        assert_eq!(msg_id, HTC_SETUP_COMPLETE_MESSAGE_ID);
    }

    /// send_setup_complete returns NotReady when called before wait_ready.
    #[test]
    fn send_setup_complete_requires_ready() {
        let transport = MockTransport::default();
        let mut client = HtcClient::new(transport);
        let mut frame = [0u8; 32];
        assert_eq!(
            client.send_setup_complete(&mut frame),
            Err(HtcClientError::Protocol(HtcError::NotReady))
        );
    }

    /// receive_packet strips a trailer when HTC_FLAG_RECV_TRAILER is set.
    #[test]
    fn receive_strips_trailer() {
        // Build a frame with 5-byte raw payload: [data, data, data, trailer_item, trailer_len]
        // trailer_len = 2 (the last byte plus 1 item byte), data = [0xAA, 0xBB, 0xCC]
        // So stripped payload should be [0xAA, 0xBB, 0xCC].
        let raw_payload: &[u8] = &[0xAA, 0xBB, 0xCC, 0x01, 0x02];
        let payload_len = raw_payload.len() as u16;
        let mut raw_frame = alloc::vec![0u8; HTC_HEADER_LENGTH + raw_payload.len()];
        raw_frame[0] = 0;                       // endpoint = 0
        raw_frame[1] = HTC_FLAG_RECV_TRAILER;   // flags
        raw_frame[2..4].copy_from_slice(&payload_len.to_le_bytes());
        // control bytes [4..8] = 0
        raw_frame[HTC_HEADER_LENGTH..].copy_from_slice(raw_payload);

        let transport = MockTransport {
            received: alloc::vec![raw_frame],
            ..MockTransport::default()
        };
        let mut client = HtcClient::new(transport);
        let mut frame = [0u8; 32];
        // We call receive_packet indirectly via wait_ready which will fail (wrong msg_id),
        // but the trailer stripping happens before that, so test the lower-level path
        // through receive() after the client is ready. Instead, test via a data receive
        // by using receive() with appropriate setup.
        //
        // For simplicity, test via the parse path: build a ready frame first.
        // Actually, let us re-queue the frame and use a client that is already ready.
        let transport2 = MockTransport {
            received: alloc::vec![
                alloc::vec![0, 0, 8, 0, 0, 0, 0, 0, 1, 0, 32, 0, 64, 0, 8, 0], // ready
                // A data frame on EP0 with trailer flag set, payload=[0xAA,0xBB,0xCC, item, len=2]
                {
                    let raw_payload: &[u8] = &[0xAA, 0xBB, 0xCC, 0x01, 0x02];
                    let payload_len = raw_payload.len() as u16;
                    let mut f = alloc::vec![0u8; HTC_HEADER_LENGTH + raw_payload.len()];
                    f[0] = 0;
                    f[1] = HTC_FLAG_RECV_TRAILER;
                    f[2..4].copy_from_slice(&payload_len.to_le_bytes());
                    f[HTC_HEADER_LENGTH..].copy_from_slice(raw_payload);
                    f
                },
            ],
            ..MockTransport::default()
        };
        let mut client2 = HtcClient::new(transport2);
        let mut buf = [0u8; 64];
        client2.wait_ready(&mut buf).unwrap();
        // receive() on EP0 (control endpoint) is allowed without endpoint registration.
        let packet = client2.receive(&mut buf).unwrap();
        assert_eq!(packet.payload, &[0xAA, 0xBB, 0xCC]);
    }

    /// seq_no increments with each send_service call.
    #[test]
    fn send_service_increments_seq_no() {
        let transport = MockTransport {
            received: alloc::vec![
                alloc::vec![0, 0, 8, 0, 0, 0, 0, 0, 1, 0, 32, 0, 64, 0, 8, 0],
                alloc::vec![0, 0, 8, 0, 0, 0, 0, 0, 3, 0, 0, 1, 0, 2, 0, 8],
            ],
            ..MockTransport::default()
        };
        let mut client = HtcClient::new(transport);
        let mut frame = [0u8; 64];
        client.wait_ready(&mut frame).unwrap();
        client.connect_service(HTC_SERVICE_WMI_CONTROL, 0, &mut frame).unwrap();

        client.send_service(HTC_SERVICE_WMI_CONTROL, 0, &[1], &mut frame).unwrap();
        client.send_service(HTC_SERVICE_WMI_CONTROL, 0, &[2], &mut frame).unwrap();
        client.send_service(HTC_SERVICE_WMI_CONTROL, 0, &[3], &mut frame).unwrap();

        let transport = client.into_transport();
        // sent[0] is the connect_service_request; sent[1..] are the data frames.
        // control[0] = seq_no is at byte offset 4 of the frame.
        assert_eq!(transport.sent[1][4], 0, "first data frame seq=0");
        assert_eq!(transport.sent[2][4], 1, "second data frame seq=1");
        assert_eq!(transport.sent[3][4], 2, "third data frame seq=2");
    }
}
