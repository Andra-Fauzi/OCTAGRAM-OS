const BMI_DONE: u32 = 1;
const BMI_READ_MEMORY: u32 = 2;
const BMI_WRITE_MEMORY: u32 = 3;
const BMI_EXECUTE: u32 = 4;
const BMI_SET_APP_START: u32 = 5;
const BMI_READ_SOC_REGISTER: u32 = 6;
const BMI_WRITE_SOC_REGISTER: u32 = 7;
const BMI_GET_TARGET_INFO: u32 = 8;
const BMI_LZ_STREAM_START: u32 = 13;
const BMI_LZ_DATA: u32 = 14;

pub const BMI_MAX_DATA_SIZE: usize = 256;
const COMMAND_ID_SIZE: usize = 4;
const MEMORY_COMMAND_HEADER_SIZE: usize = COMMAND_ID_SIZE + 8;
const MAX_WRITE_DATA_SIZE: usize = BMI_MAX_DATA_SIZE - MEMORY_COMMAND_HEADER_SIZE;
const LZ_DATA_HEADER_SIZE: usize = COMMAND_ID_SIZE + 4;
const MAX_LZ_DATA_SIZE: usize = BMI_MAX_DATA_SIZE - LZ_DATA_HEADER_SIZE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BmiError {
    ZeroLength,
    LengthTooLarge,
    AddressOverflow,
    UnalignedWriteAddress,
    UnalignedLzData,
    OutputBufferTooSmall,
    InvalidMemoryResponseLength,
    InvalidRegisterResponseLength,
    InvalidCommandResponseLength,
    TargetInfoResponseTooShort,
    InvalidTargetInfoResponseLength,
    CommandAfterDone,
    CommandStateUncertain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetInfo {
    pub version: u32,
    pub target_type: u32,
}

pub trait BmiTransport {
    type Error;

    fn exchange(
        &mut self,
        request: &[u8],
        response: Option<&mut [u8]>,
    ) -> Result<usize, Self::Error>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum BmiClientError<E> {
    Transport(E),
    Protocol(BmiError),
}

pub struct BmiClient<T> {
    transport: T,
    state: BmiClientState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BmiClientState {
    Active,
    Done,
    Uncertain,
}

impl<T: BmiTransport> BmiClient<T> {
    pub const fn new(transport: T) -> Self {
        Self {
            transport,
            state: BmiClientState::Active,
        }
    }

    pub fn get_target_info(&mut self) -> Result<TargetInfo, BmiClientError<T::Error>> {
        let mut response = [0; 12];
        let response_length =
            self.exchange_active(&get_target_info_command(), Some(&mut response))?;
        let response = response
            .get(..response_length)
            .ok_or(BmiClientError::Protocol(
                BmiError::InvalidTargetInfoResponseLength,
            ))?;
        parse_target_info(response).map_err(BmiClientError::Protocol)
    }

    pub fn read_memory(
        &mut self,
        address: u32,
        output: &mut [u8],
    ) -> Result<(), BmiClientError<T::Error>> {
        if output.is_empty() {
            return Err(BmiClientError::Protocol(BmiError::ZeroLength));
        }
        let output_length = u32::try_from(output.len())
            .map_err(|_| BmiClientError::Protocol(BmiError::LengthTooLarge))?;
        address
            .checked_add(output_length)
            .ok_or(BmiClientError::Protocol(BmiError::AddressOverflow))?;

        let mut current_address = address;
        for chunk in output.chunks_mut(BMI_MAX_DATA_SIZE) {
            let request = read_memory_command(current_address, chunk.len())
                .map_err(BmiClientError::Protocol)?;
            let response_length = self.exchange_active(&request, Some(chunk))?;
            if response_length != chunk.len() {
                return Err(BmiClientError::Protocol(
                    BmiError::InvalidMemoryResponseLength,
                ));
            }
            current_address = current_address
                .checked_add(chunk.len() as u32)
                .ok_or(BmiClientError::Protocol(BmiError::AddressOverflow))?;
        }
        Ok(())
    }

    pub fn write_memory(
        &mut self,
        address: u32,
        data: &[u8],
    ) -> Result<(), BmiClientError<T::Error>> {
        if data.is_empty() {
            return Err(BmiClientError::Protocol(BmiError::ZeroLength));
        }
        let padded_total_length = data
            .len()
            .checked_add(3)
            .ok_or(BmiClientError::Protocol(BmiError::LengthTooLarge))?
            & !3;
        let padded_total_length = u32::try_from(padded_total_length)
            .map_err(|_| BmiClientError::Protocol(BmiError::LengthTooLarge))?;
        address
            .checked_add(padded_total_length)
            .ok_or(BmiClientError::Protocol(BmiError::AddressOverflow))?;

        let mut current_address = address;
        for chunk in data.chunks(MAX_WRITE_DATA_SIZE) {
            let mut request = [0; BMI_MAX_DATA_SIZE];
            let request_length = write_memory_command(current_address, chunk, &mut request)
                .map_err(BmiClientError::Protocol)?;
            self.exchange_active(&request[..request_length], None)?;
            let padded_length = chunk
                .len()
                .checked_add(3)
                .ok_or(BmiClientError::Protocol(BmiError::LengthTooLarge))?
                & !3;
            current_address = current_address
                .checked_add(padded_length as u32)
                .ok_or(BmiClientError::Protocol(BmiError::AddressOverflow))?;
        }
        Ok(())
    }

    pub fn read_soc_register(&mut self, address: u32) -> Result<u32, BmiClientError<T::Error>> {
        let request = read_soc_register_command(address);
        let mut response = [0; 4];
        let response_length = self.exchange_active(&request, Some(&mut response))?;
        if response_length != response.len() {
            return Err(BmiClientError::Protocol(
                BmiError::InvalidRegisterResponseLength,
            ));
        }
        Ok(u32::from_le_bytes(response))
    }

    pub fn write_soc_register(
        &mut self,
        address: u32,
        value: u32,
    ) -> Result<(), BmiClientError<T::Error>> {
        self.exchange_active(&write_soc_register_command(address, value), None)?;
        Ok(())
    }

    pub fn execute(
        &mut self,
        address: u32,
        parameter: u32,
    ) -> Result<u32, BmiClientError<T::Error>> {
        let request = execute_command(address, parameter);
        let mut response = [0; 4];
        let response_length = self.exchange_active(&request, Some(&mut response))?;
        if response_length != response.len() {
            return Err(BmiClientError::Protocol(
                BmiError::InvalidCommandResponseLength,
            ));
        }
        Ok(u32::from_le_bytes(response))
    }

    pub fn set_app_start(&mut self, address: u32) -> Result<(), BmiClientError<T::Error>> {
        self.exchange_active(&set_app_start_command(address), None)?;
        Ok(())
    }

    pub fn lz_stream_start(&mut self, address: u32) -> Result<(), BmiClientError<T::Error>> {
        self.exchange_active(&lz_stream_start_command(address), None)?;
        Ok(())
    }

    pub fn lz_data(&mut self, data: &[u8]) -> Result<(), BmiClientError<T::Error>> {
        if data.len() & 3 != 0 {
            return Err(BmiClientError::Protocol(BmiError::UnalignedLzData));
        }
        for chunk in data.chunks(MAX_LZ_DATA_SIZE) {
            let mut request = [0; BMI_MAX_DATA_SIZE];
            let request_length =
                lz_data_command(chunk, &mut request).map_err(BmiClientError::Protocol)?;
            self.exchange_active(&request[..request_length], None)?;
        }
        Ok(())
    }

    pub fn fast_download(
        &mut self,
        address: u32,
        image: &[u8],
    ) -> Result<(), BmiClientError<T::Error>> {
        if image.is_empty() {
            return Err(BmiClientError::Protocol(BmiError::ZeroLength));
        }

        self.lz_stream_start(address)?;
        let aligned_length = image.len() & !3;
        self.lz_data(&image[..aligned_length])?;
        if aligned_length != image.len() {
            let mut trailer = [0; 4];
            trailer[..image.len() - aligned_length].copy_from_slice(&image[aligned_length..]);
            self.lz_data(&trailer)?;
        }
        self.lz_stream_start(0)?;
        Ok(())
    }

    /// Upload and execute OTP after the matching board data is available.
    pub fn download_and_run_otp(
        &mut self,
        address: u32,
        parameter: u32,
        image: &[u8],
    ) -> Result<u32, BmiClientError<T::Error>> {
        self.fast_download(address, image)?;
        self.execute(address, parameter)
    }

    /// Upload an image, set its entry point, and finish BMI to start it.
    /// The caller must complete target identification, board-data setup, and OTP calibration first.
    pub fn download_and_start_firmware(
        &mut self,
        address: u32,
        image: &[u8],
    ) -> Result<(), BmiClientError<T::Error>> {
        self.fast_download(address, image)?;
        self.set_app_start(address)?;
        self.done()
    }

    pub fn done(&mut self) -> Result<(), BmiClientError<T::Error>> {
        match self.state {
            BmiClientState::Active => {}
            BmiClientState::Done => return Ok(()),
            BmiClientState::Uncertain => {
                return Err(BmiClientError::Protocol(
                    BmiError::CommandStateUncertain,
                ));
            }
        }

        self.state = BmiClientState::Uncertain;
        self.transport
            .exchange(&done_command(), None)
            .map_err(BmiClientError::Transport)?;
        self.state = BmiClientState::Done;
        Ok(())
    }

    pub fn into_transport(self) -> T {
        self.transport
    }

    fn exchange_active(
        &mut self,
        request: &[u8],
        response: Option<&mut [u8]>,
    ) -> Result<usize, BmiClientError<T::Error>> {
        match self.state {
            BmiClientState::Active => {}
            BmiClientState::Done => {
                return Err(BmiClientError::Protocol(BmiError::CommandAfterDone));
            }
            BmiClientState::Uncertain => {
                return Err(BmiClientError::Protocol(BmiError::CommandStateUncertain));
            }
        }
        self.transport
            .exchange(request, response)
            .map_err(BmiClientError::Transport)
    }
}

pub const fn done_command() -> [u8; COMMAND_ID_SIZE] {
    command_id(BMI_DONE)
}

pub const fn get_target_info_command() -> [u8; COMMAND_ID_SIZE] {
    command_id(BMI_GET_TARGET_INFO)
}

pub fn read_memory_command(address: u32, length: usize) -> Result<[u8; 12], BmiError> {
    if length == 0 {
        return Err(BmiError::ZeroLength);
    }
    if length > BMI_MAX_DATA_SIZE {
        return Err(BmiError::LengthTooLarge);
    }
    let length = u32::try_from(length).map_err(|_| BmiError::LengthTooLarge)?;
    address
        .checked_add(length)
        .ok_or(BmiError::AddressOverflow)?;

    let mut command = [0; 12];
    write_u32(&mut command, 0, BMI_READ_MEMORY);
    write_u32(&mut command, 4, address);
    write_u32(&mut command, 8, length);
    Ok(command)
}

pub fn write_memory_command(
    address: u32,
    data: &[u8],
    output: &mut [u8],
) -> Result<usize, BmiError> {
    if data.is_empty() {
        return Err(BmiError::ZeroLength);
    }
    if data.len() > MAX_WRITE_DATA_SIZE {
        return Err(BmiError::LengthTooLarge);
    }
    if address & 3 != 0 {
        return Err(BmiError::UnalignedWriteAddress);
    }
    let padded_length = data.len().checked_add(3).ok_or(BmiError::LengthTooLarge)? & !3;
    let length = u32::try_from(padded_length).map_err(|_| BmiError::LengthTooLarge)?;
    address
        .checked_add(length)
        .ok_or(BmiError::AddressOverflow)?;
    let command_length = MEMORY_COMMAND_HEADER_SIZE
        .checked_add(padded_length)
        .ok_or(BmiError::LengthTooLarge)?;
    if output.len() < command_length {
        return Err(BmiError::OutputBufferTooSmall);
    }

    write_u32(output, 0, BMI_WRITE_MEMORY);
    write_u32(output, 4, address);
    write_u32(output, 8, length);
    output[MEMORY_COMMAND_HEADER_SIZE..MEMORY_COMMAND_HEADER_SIZE + padded_length].fill(0);
    output[MEMORY_COMMAND_HEADER_SIZE..MEMORY_COMMAND_HEADER_SIZE + data.len()]
        .copy_from_slice(data);
    Ok(command_length)
}

pub const fn execute_command(address: u32, parameter: u32) -> [u8; 12] {
    let mut command = [0; 12];
    write_u32(&mut command, 0, BMI_EXECUTE);
    write_u32(&mut command, 4, address);
    write_u32(&mut command, 8, parameter);
    command
}

pub const fn set_app_start_command(address: u32) -> [u8; 8] {
    let mut command = [0; 8];
    write_u32(&mut command, 0, BMI_SET_APP_START);
    write_u32(&mut command, 4, address);
    command
}

pub const fn read_soc_register_command(address: u32) -> [u8; 8] {
    let mut command = [0; 8];
    write_u32(&mut command, 0, BMI_READ_SOC_REGISTER);
    write_u32(&mut command, 4, address);
    command
}

pub const fn write_soc_register_command(address: u32, value: u32) -> [u8; 12] {
    let mut command = [0; 12];
    write_u32(&mut command, 0, BMI_WRITE_SOC_REGISTER);
    write_u32(&mut command, 4, address);
    write_u32(&mut command, 8, value);
    command
}

pub const fn lz_stream_start_command(address: u32) -> [u8; 8] {
    let mut command = [0; 8];
    write_u32(&mut command, 0, BMI_LZ_STREAM_START);
    write_u32(&mut command, 4, address);
    command
}

pub fn lz_data_command(data: &[u8], output: &mut [u8]) -> Result<usize, BmiError> {
    if data.is_empty() {
        return Err(BmiError::ZeroLength);
    }
    if data.len() > MAX_LZ_DATA_SIZE {
        return Err(BmiError::LengthTooLarge);
    }
    if data.len() & 3 != 0 {
        return Err(BmiError::UnalignedLzData);
    }
    let command_length = LZ_DATA_HEADER_SIZE + data.len();
    if output.len() < command_length {
        return Err(BmiError::OutputBufferTooSmall);
    }
    write_u32(output, 0, BMI_LZ_DATA);
    write_u32(output, 4, data.len() as u32);
    output[LZ_DATA_HEADER_SIZE..command_length].copy_from_slice(data);
    Ok(command_length)
}

pub fn parse_target_info(response: &[u8]) -> Result<TargetInfo, BmiError> {
    if response.len() < 12 {
        return Err(BmiError::TargetInfoResponseTooShort);
    }
    if read_u32(response, 0) != 12 {
        return Err(BmiError::InvalidTargetInfoResponseLength);
    }
    Ok(TargetInfo {
        version: read_u32(response, 4),
        target_type: read_u32(response, 8),
    })
}

const fn command_id(id: u32) -> [u8; COMMAND_ID_SIZE] {
    let mut command = [0; COMMAND_ID_SIZE];
    write_u32(&mut command, 0, id);
    command
}

const fn write_u32(output: &mut [u8], offset: usize, value: u32) {
    let bytes = value.to_le_bytes();
    output[offset] = bytes[0];
    output[offset + 1] = bytes[1];
    output[offset + 2] = bytes[2];
    output[offset + 3] = bytes[3];
}

fn read_u32(input: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        input[offset],
        input[offset + 1],
        input[offset + 2],
        input[offset + 3],
    ])
}

#[cfg(test)]
mod tests {
    use super::{
        done_command, execute_command, get_target_info_command, lz_data_command,
        lz_stream_start_command, parse_target_info, read_memory_command, read_soc_register_command,
        set_app_start_command, write_memory_command, write_soc_register_command, BmiClient,
        BmiClientError, BmiError, BmiTransport, TargetInfo, BMI_MAX_DATA_SIZE,
        MAX_WRITE_DATA_SIZE,
    };

    #[derive(Default)]
    struct MockBmiTransport {
        request: alloc::vec::Vec<u8>,
        response: alloc::vec::Vec<u8>,
        fail: bool,
    }

    impl BmiTransport for MockBmiTransport {
        type Error = &'static str;

        fn exchange(
            &mut self,
            request: &[u8],
            response: Option<&mut [u8]>,
        ) -> Result<usize, Self::Error> {
            if self.fail {
                return Err("mock transport failure");
            }
            self.request.extend_from_slice(request);
            let Some(response) = response else {
                return Ok(0);
            };
            if response.len() < self.response.len() {
                return Err("mock response buffer too small");
            }
            response[..self.response.len()].copy_from_slice(&self.response);
            Ok(self.response.len())
        }
    }

    #[test]
    fn encodes_fixed_commands_as_little_endian_words() {
        assert_eq!(done_command(), [1, 0, 0, 0]);
        assert_eq!(get_target_info_command(), [8, 0, 0, 0]);
        assert_eq!(
            execute_command(0x1234, 0x5678),
            [4, 0, 0, 0, 0x34, 0x12, 0, 0, 0x78, 0x56, 0, 0]
        );
        assert_eq!(
            set_app_start_command(0x1234),
            [5, 0, 0, 0, 0x34, 0x12, 0, 0]
        );
        assert_eq!(
            read_soc_register_command(0x1234),
            [6, 0, 0, 0, 0x34, 0x12, 0, 0]
        );
        assert_eq!(
            write_soc_register_command(0x1234, 0x5678),
            [7, 0, 0, 0, 0x34, 0x12, 0, 0, 0x78, 0x56, 0, 0]
        );
        assert_eq!(
            lz_stream_start_command(0x1234),
            [13, 0, 0, 0, 0x34, 0x12, 0, 0]
        );
    }

    #[test]
    fn encodes_read_memory_bounds() {
        assert_eq!(
            read_memory_command(0x1234, 4).unwrap(),
            [2, 0, 0, 0, 0x34, 0x12, 0, 0, 4, 0, 0, 0]
        );
        assert_eq!(read_memory_command(0, 0), Err(BmiError::ZeroLength));
        assert_eq!(
            read_memory_command(0, BMI_MAX_DATA_SIZE + 1),
            Err(BmiError::LengthTooLarge)
        );
    }

    #[test]
    fn pads_write_memory_data_without_reading_past_input() {
        let mut output = [0xFF; 16];
        let length = write_memory_command(0x1000, &[1, 2, 3], &mut output).unwrap();
        assert_eq!(length, 16);
        assert_eq!(&output[..12], &[3, 0, 0, 0, 0, 0x10, 0, 0, 4, 0, 0, 0]);
        assert_eq!(&output[12..], &[1, 2, 3, 0]);
    }

    #[test]
    fn rejects_invalid_write_memory_requests() {
        let mut output = [0; 16];
        assert_eq!(
            write_memory_command(0x1001, &[1], &mut output),
            Err(BmiError::UnalignedWriteAddress)
        );
        assert_eq!(
            write_memory_command(0x1000, &[1], &mut output[..12]),
            Err(BmiError::OutputBufferTooSmall)
        );
        assert_eq!(
            write_memory_command(0x1000, &[], &mut output),
            Err(BmiError::ZeroLength)
        );
    }

    #[test]
    fn encodes_aligned_lz_data_chunks() {
        let mut output = [0; 12];
        let length = lz_data_command(&[1, 2, 3, 4], &mut output).unwrap();
        assert_eq!(length, 12);
        assert_eq!(&output, &[14, 0, 0, 0, 4, 0, 0, 0, 1, 2, 3, 4]);
        assert_eq!(
            lz_data_command(&[1, 2, 3], &mut output),
            Err(BmiError::UnalignedLzData)
        );
    }

    #[test]
    fn parses_target_version_and_type_from_response() {
        let response = [12, 0, 0, 0, 0, 0, 2, 5, 1, 0, 0, 0];
        let info = parse_target_info(&response).unwrap();
        assert_eq!(info.version, 0x0502_0000);
        assert_eq!(info.target_type, 1);
        assert_eq!(
            parse_target_info(&response[..11]),
            Err(BmiError::TargetInfoResponseTooShort)
        );
    }

    #[test]
    fn rejects_target_info_with_unexpected_reported_length() {
        let response = [8, 0, 0, 0, 0, 0, 2, 5, 1, 0, 0, 0];
        assert_eq!(
            parse_target_info(&response),
            Err(BmiError::InvalidTargetInfoResponseLength)
        );
    }

    #[test]
    fn bmi_client_uses_transport_to_query_target_info() {
        let transport = MockBmiTransport {
            request: alloc::vec::Vec::new(),
            response: [12, 0, 0, 0, 0, 0, 2, 5, 1, 0, 0, 0].into(),
            fail: false,
        };
        let mut client = BmiClient::new(transport);
        assert_eq!(
            client.get_target_info(),
            Ok(TargetInfo {
                version: 0x0502_0000,
                target_type: 1
            })
        );
        assert_eq!(client.into_transport().request, get_target_info_command());
    }

    #[test]
    fn bmi_client_reports_malformed_responses_and_transport_failures() {
        let transport = MockBmiTransport {
            request: alloc::vec::Vec::new(),
            response: [8, 0, 0, 0, 0, 0, 2, 5, 1, 0, 0, 0].into(),
            fail: false,
        };
        let mut client = BmiClient::new(transport);
        assert_eq!(
            client.get_target_info(),
            Err(BmiClientError::Protocol(
                BmiError::InvalidTargetInfoResponseLength
            ))
        );

        let transport = MockBmiTransport {
            request: alloc::vec::Vec::new(),
            response: alloc::vec::Vec::new(),
            fail: true,
        };
        let mut client = BmiClient::new(transport);
        assert_eq!(
            client.get_target_info(),
            Err(BmiClientError::Transport("mock transport failure"))
        );
    }

    #[test]
    fn bmi_client_reads_and_writes_memory_through_transport() {
        let transport = MockBmiTransport {
            request: alloc::vec::Vec::new(),
            response: [9, 8, 7, 6].into(),
            fail: false,
        };
        let mut client = BmiClient::new(transport);
        let mut output = [0; 4];
        client.read_memory(0x1000, &mut output).unwrap();
        assert_eq!(output, [9, 8, 7, 6]);
        assert_eq!(
            client.into_transport().request,
            read_memory_command(0x1000, 4).unwrap()
        );

        let transport = MockBmiTransport {
            request: alloc::vec::Vec::new(),
            response: alloc::vec::Vec::new(),
            fail: false,
        };
        let mut client = BmiClient::new(transport);
        client.write_memory(0x1000, &[1, 2, 3]).unwrap();
        assert_eq!(
            client.into_transport().request,
            [3, 0, 0, 0, 0, 0x10, 0, 0, 4, 0, 0, 0, 1, 2, 3, 0]
        );
    }

    #[test]
    fn bmi_client_rejects_empty_or_overflowing_writes_before_transport() {
        let transport = MockBmiTransport::default();
        let mut client = BmiClient::new(transport);
        assert_eq!(
            client.write_memory(0x1000, &[]),
            Err(BmiClientError::Protocol(BmiError::ZeroLength))
        );
        assert!(client.into_transport().request.is_empty());

        let transport = MockBmiTransport::default();
        let mut client = BmiClient::new(transport);
        let data = alloc::vec![0; MAX_WRITE_DATA_SIZE + 1];
        assert_eq!(
            client.write_memory(0xffff_ff08, &data),
            Err(BmiClientError::Protocol(BmiError::AddressOverflow))
        );
        assert!(client.into_transport().request.is_empty());
    }

    #[test]
    fn bmi_client_checks_register_response_and_closes_command_stream() {
        let transport = MockBmiTransport {
            request: alloc::vec::Vec::new(),
            response: [0x78, 0x56, 0x34, 0x12].into(),
            fail: false,
        };
        let mut client = BmiClient::new(transport);
        assert_eq!(client.read_soc_register(0x1234), Ok(0x1234_5678));
        assert_eq!(client.execute(0x5678, 0x1234), Ok(0x1234_5678));
        client.write_soc_register(0x1000, 0xfeed_beef).unwrap();
        client.set_app_start(0x2000).unwrap();
        client.done().unwrap();
        client.done().unwrap();
        assert_eq!(
            client.read_soc_register(0x1000),
            Err(BmiClientError::Protocol(BmiError::CommandAfterDone))
        );
    }

    #[test]
    fn bmi_client_does_not_report_done_after_an_uncertain_transport_failure() {
        let transport = MockBmiTransport {
            fail: true,
            ..MockBmiTransport::default()
        };
        let mut client = BmiClient::new(transport);
        assert_eq!(
            client.done(),
            Err(BmiClientError::Transport("mock transport failure"))
        );
        assert_eq!(
            client.done(),
            Err(BmiClientError::Protocol(BmiError::CommandStateUncertain))
        );
        assert_eq!(
            client.write_soc_register(0x1000, 1),
            Err(BmiClientError::Protocol(BmiError::CommandStateUncertain))
        );
        assert!(client.into_transport().request.is_empty());
    }

    #[test]
    fn bmi_client_sends_aligned_lz_chunks_and_rejects_partial_words() {
        let transport = MockBmiTransport {
            request: alloc::vec::Vec::new(),
            response: alloc::vec::Vec::new(),
            fail: false,
        };
        let mut client = BmiClient::new(transport);
        client.lz_stream_start(0x1234).unwrap();
        client.lz_data(&[1, 2, 3, 4]).unwrap();
        assert_eq!(
            client.into_transport().request,
            [
                13, 0, 0, 0, 0x34, 0x12, 0, 0, 14, 0, 0, 0, 4, 0, 0, 0, 1, 2, 3, 4
            ]
        );

        let transport = MockBmiTransport {
            request: alloc::vec::Vec::new(),
            response: alloc::vec::Vec::new(),
            fail: false,
        };
        let mut client = BmiClient::new(transport);
        assert_eq!(
            client.lz_data(&[1, 2, 3]),
            Err(BmiClientError::Protocol(BmiError::UnalignedLzData))
        );
        assert!(client.into_transport().request.is_empty());
    }

    #[test]
    fn bmi_client_fast_download_pads_the_tail_and_flushes_the_stream() {
        let transport = MockBmiTransport {
            request: alloc::vec::Vec::new(),
            response: alloc::vec::Vec::new(),
            fail: false,
        };
        let mut client = BmiClient::new(transport);
        client.fast_download(0x1234, &[1, 2, 3, 4, 5]).unwrap();
        assert_eq!(
            client.into_transport().request,
            [
                13, 0, 0, 0, 0x34, 0x12, 0, 0, 14, 0, 0, 0, 4, 0, 0, 0, 1, 2, 3, 4, 14, 0, 0, 0, 4,
                0, 0, 0, 5, 0, 0, 0, 13, 0, 0, 0, 0, 0, 0, 0
            ]
        );

        let transport = MockBmiTransport {
            request: alloc::vec::Vec::new(),
            response: alloc::vec::Vec::new(),
            fail: false,
        };
        let mut client = BmiClient::new(transport);
        assert_eq!(
            client.fast_download(0x1234, &[]),
            Err(BmiClientError::Protocol(BmiError::ZeroLength))
        );
        assert!(client.into_transport().request.is_empty());
    }

    #[test]
    fn bmi_client_downloads_otp_before_executing_it() {
        let transport = MockBmiTransport {
            request: alloc::vec::Vec::new(),
            response: [0, 0, 0, 0].into(),
            fail: false,
        };
        let mut client = BmiClient::new(transport);
        assert_eq!(client.download_and_run_otp(0x1234, 0, &[1, 2, 3, 4]), Ok(0));
        assert_eq!(
            client.into_transport().request,
            [
                13, 0, 0, 0, 0x34, 0x12, 0, 0, 14, 0, 0, 0, 4, 0, 0, 0, 1, 2, 3, 4, 13, 0, 0, 0, 0,
                0, 0, 0, 4, 0, 0, 0, 0x34, 0x12, 0, 0, 0, 0, 0, 0
            ]
        );
    }

    #[test]
    fn bmi_client_starts_firmware_only_after_upload_and_app_start() {
        let transport = MockBmiTransport {
            request: alloc::vec::Vec::new(),
            response: alloc::vec::Vec::new(),
            fail: false,
        };
        let mut client = BmiClient::new(transport);
        client
            .download_and_start_firmware(0x1234, &[1, 2, 3, 4])
            .unwrap();
        assert_eq!(
            client.into_transport().request,
            [
                13, 0, 0, 0, 0x34, 0x12, 0, 0, 14, 0, 0, 0, 4, 0, 0, 0, 1, 2, 3, 4, 13, 0, 0, 0, 0,
                0, 0, 0, 5, 0, 0, 0, 0x34, 0x12, 0, 0, 1, 0, 0, 0
            ]
        );
    }
}
