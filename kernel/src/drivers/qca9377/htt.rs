//! HTT (Host-Target Transport) version handshake for QCA9377 / ath10k.
//!
//! HTT is an HTC service (`ATH10K_HTC_SVC_ID_HTT_DATA_MSG`, group 3 index 0,
//! i.e. 0x0300), so every HTT message travels inside an HTC data message for the
//! endpoint the target assigned to that service. A bare HTT frame written to the
//! copy engine is never seen by the firmware. On QCA9377 the HTT pair is
//! TX=CE4, RX=CE5.
//!
//! Reference: drivers/net/wireless/ath/ath10k/htt.h, htt_tx.c, htt.c.

use super::ce::{RegisterIo, RxCe, TxCe};
use super::htc::{HtcDataError, data_message_payload, encode_data_message};

// H2T message types (`enum htt_h2t_msg_type`).
pub const HTT_H2T_MSG_TYPE_VERSION_REQ: u8 = 0x00;

// T2H message types (`enum htt_t2h_msg_type`).
pub const HTT_T2H_MSG_TYPE_VERSION_CONF: u8 = 0x00;

/// `ath10k_htt_h2t_ver_req_msg` sends a one-byte `htt_cmd_hdr` padded to a word.
pub const HTT_VERSION_REQ_LEN: usize = 4;
/// `struct htt_ver_resp`: `msg_type(1)` then `minor`, `major`, `rsvd0`.
pub const HTT_VERSION_CONF_LEN: usize = 4;
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

#[cfg(test)]
mod tests {
    use super::{HTT_VERSION_REQ_LEN, HttError, HttVersion, parse_version_conf};

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
}
