const HTC_HEADER_LENGTH: usize = 8;
const HTC_CONTROL_ENDPOINT: u8 = 0;
const HTC_MAX_SERVICE_ENDPOINTS: usize = 7;
const HTC_READY_MESSAGE_ID: u16 = 1;
const HTC_CONNECT_SERVICE_MESSAGE_ID: u16 = 2;
const HTC_CONNECT_SERVICE_RESPONSE_ID: u16 = 3;

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

pub fn parse_ready(packet: HtcPacket<'_>) -> Result<HtcReady, HtcError> {
    if packet.header.endpoint != HTC_CONTROL_ENDPOINT {
        return Err(HtcError::UnexpectedEndpoint);
    }
    if packet.payload.len() != 8
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

pub fn parse_connect_response(
    packet: HtcPacket<'_>,
    expected_service: u16,
) -> Result<HtcConnectResponse, HtcError> {
    if packet.header.endpoint != HTC_CONTROL_ENDPOINT {
        return Err(HtcError::UnexpectedEndpoint);
    }
    if packet.payload.len() != 8
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

pub trait HtcTransport {
    type Error;

    fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error>;
    fn receive(&mut self, frame: &mut [u8]) -> Result<usize, Self::Error>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HtcServiceEndpoint {
    service_id: u16,
    endpoint: u8,
    max_message_size: u16,
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
}

impl<T: HtcTransport> HtcClient<T> {
    pub const fn new(transport: T) -> Self {
        Self {
            transport,
            ready: None,
            endpoints: [None; HTC_MAX_SERVICE_ENDPOINTS],
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
        self.ready = Some(ready);
        Ok(ready)
    }

    pub fn connect_service(
        &mut self,
        service_id: u16,
        flags: u16,
        frame: &mut [u8],
    ) -> Result<HtcConnectResponse, HtcClientError<T::Error>> {
        if self.ready.is_none() {
            return Err(HtcClientError::Protocol(HtcError::NotReady));
        }
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
        self.transport
            .send(&request)
            .map_err(HtcClientError::Transport)?;
        let packet = self.receive_packet(frame)?;
        let response =
            parse_connect_response(packet, service_id).map_err(HtcClientError::Protocol)?;
        let ready = self.ready.ok_or(HtcClientError::Protocol(HtcError::NotReady))?;
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
        let slot = self
            .endpoints
            .iter_mut()
            .find(|endpoint| endpoint.is_none())
            .ok_or(HtcClientError::Protocol(HtcError::EndpointTableFull))?;
        *slot = Some(HtcServiceEndpoint {
            service_id,
            endpoint: response.endpoint,
            max_message_size: response.max_message_size,
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
            .iter()
            .flatten()
            .find(|endpoint| endpoint.service_id == service_id)
            .ok_or(HtcClientError::Protocol(HtcError::EndpointNotConnected))?;
        if payload.len() > usize::from(endpoint.max_message_size) {
            return Err(HtcClientError::Protocol(HtcError::MessageTooLarge));
        }

        let packet = HtcPacket {
            header: HtcHeader::new(endpoint.endpoint, flags, [0; 4]),
            payload,
        };
        let length = packet
            .encode_into(frame)
            .map_err(HtcClientError::Protocol)?;
        self.transport
            .send(&frame[..length])
            .map_err(HtcClientError::Transport)
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
        HtcPacket::parse(received).map_err(HtcClientError::Protocol)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HTC_SERVICE_WMI_CONTROL, HtcClient, HtcClientError, HtcError, HtcHeader, HtcPacket,
        HtcTransport, connect_service_request, parse_connect_response, parse_ready,
    };
    use alloc::vec::Vec;

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
        assert_eq!(&transport.sent[1][..9], &[2, 0, 1, 0, 0, 0, 0, 0, 0xaa]);
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
}
