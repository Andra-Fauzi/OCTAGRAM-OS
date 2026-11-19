use limine::modules::InternalModule;
use limine::request::ModuleRequest;

const QCA9377_FIRMWARE_PATH: &[u8] = b"/boot/firmware/ath10k/QCA9377/hw1.0/firmware-6.bin";
const QCA9377_BOARD_PATH: &[u8] = b"/boot/firmware/ath10k/QCA9377/hw1.0/board-2.bin";
const ATH10K_FIRMWARE_MAGIC: &[u8] = b"QCA-ATH10K\0";
const ATH10K_BOARD_MAGIC: &[u8] = b"QCA-ATH10K-BOARD\0";
// `enum ath10k_fw_ie_type`. The shipped QCA9377 firmware-6.bin really carries
// its 727125-byte segment table as element 3 and the 24193-byte one as element 4,
// so element 3 is the firmware image on this blob.
const FW_IE_VERSION: u32 = 0;
const FW_IE_FEATURES: u32 = 2;
const FW_IE_IMAGE: u32 = 3;
const FW_IE_OTP_IMAGE: u32 = 4;
const FW_IE_WMI_OP_VERSION: u32 = 5;
const FW_IE_HTT_OP_VERSION: u32 = 6;
const BOARD_IE_DATA: u32 = 0;
const BOARD_IE_EXT_DATA: u32 = 1;
const BOARD_ENTRY_NAME: u32 = 0;
const BOARD_ENTRY_DATA: u32 = 1;
const BOARD_IE_INVALID: &str = "QCA9377 board bundle has an invalid information element";

// Limine exposes a single Modules request ID. Keep one shared request for both
// the driver's internal firmware modules and bootloader-configured modules
// (including EFI/BOOT/BOOTX64.EFI, which the installer copies to disk).
#[used]
#[unsafe(link_section = ".requests")]
pub(crate) static MODULE_REQUEST: ModuleRequest = ModuleRequest::new().with_internal_modules(&[
    &InternalModule::new().with_path(c"/boot/firmware/ath10k/QCA9377/hw1.0/firmware-6.bin"),
    &InternalModule::new().with_path(c"/boot/firmware/ath10k/QCA9377/hw1.0/board-2.bin"),
]);

pub struct Qca9377Firmware {
    pub firmware_version: &'static str,
    pub otp_image: Option<&'static [u8]>,
    pub firmware_image: &'static [u8],
    pub board_bundle: &'static [u8],
    /// `ATH10K_FW_IE_FEATURES` bits, zero when the element is absent.
    pub features: u32,
    /// `ATH10K_FW_IE_WMI_OP_VERSION`; zero means the blob predates the element.
    pub wmi_op_version: u32,
    /// `ATH10K_FW_IE_HTT_OP_VERSION`; zero means the blob predates the element.
    pub htt_op_version: u32,
}

pub struct FirmwareImages<'a> {
    pub version: &'a str,
    pub otp_image: Option<&'a [u8]>,
    pub firmware_image: &'a [u8],
}

/// The information elements that select the driver's command layouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirmwareOperations {
    /// `ATH10K_FW_IE_FEATURES` bits, zero when the element is absent.
    pub features: u32,
    /// `ATH10K_FW_IE_WMI_OP_VERSION`; zero means the blob predates the element.
    pub wmi_op_version: u32,
    /// `ATH10K_FW_IE_HTT_OP_VERSION`; zero means the blob predates the element.
    pub htt_op_version: u32,
}

pub struct Qca9377BoardData<'a> {
    pub board: &'a [u8],
    pub board_ext: Option<&'a [u8]>,
}

pub fn qca9377() -> Result<Qca9377Firmware, &'static str> {
    let response = MODULE_REQUEST
        .get_response()
        .ok_or("Limine did not return requested Wi-Fi firmware modules")?;
    let modules = response.modules();
    let firmware = find_module(modules, QCA9377_FIRMWARE_PATH)
        .ok_or("QCA9377 firmware-6.bin module is missing or empty")?;
    let board_bundle = find_module(modules, QCA9377_BOARD_PATH)
        .ok_or("QCA9377 board-2.bin module is missing or empty")?;
    let images = parse_firmware(firmware)?;
    let operations = firmware_operations(firmware)?;
    validate_board_bundle(board_bundle)?;
    Ok(Qca9377Firmware {
        firmware_version: images.version,
        otp_image: images.otp_image,
        firmware_image: images.firmware_image,
        board_bundle,
        features: operations.features,
        wmi_op_version: operations.wmi_op_version,
        htt_op_version: operations.htt_op_version,
    })
}

pub fn select_board_data<'a>(
    board_bundle: &'a [u8],
    board_name: &[u8],
) -> Result<Qca9377BoardData<'a>, &'static str> {
    validate_board_bundle(board_bundle)?;
    let mut offset =
        align_four(ATH10K_BOARD_MAGIC.len()).ok_or("QCA9377 board magic length overflow")?;
    let mut board = None;
    let mut board_ext = None;

    while offset < board_bundle.len() {
        let (id, data, next) = read_ie(board_bundle, offset, BOARD_IE_INVALID)?;
        match id {
            BOARD_IE_DATA if board.is_none() => {
                board = find_named_board_data(data, board_name)?;
            }
            BOARD_IE_EXT_DATA if board_ext.is_none() => {
                board_ext = find_named_board_data(data, board_name)?;
            }
            _ => {}
        }
        offset = next;
    }

    Ok(Qca9377BoardData {
        board: board.ok_or("QCA9377 board bundle has no matching board name")?,
        board_ext,
    })
}

pub fn parse_firmware(firmware: &[u8]) -> Result<FirmwareImages<'_>, &'static str> {
    if !firmware.starts_with(ATH10K_FIRMWARE_MAGIC) {
        return Err("QCA9377 firmware has an invalid ath10k magic header");
    }

    let mut offset =
        align_four(ATH10K_FIRMWARE_MAGIC.len()).ok_or("QCA9377 firmware magic length overflow")?;
    if offset > firmware.len() {
        return Err("QCA9377 firmware is truncated after its magic header");
    }
    let mut version = None;
    let mut otp_image = None;
    let mut firmware_image = None;
    while offset < firmware.len() {
        let header_end = offset
            .checked_add(8)
            .ok_or("QCA9377 firmware information element offset overflow")?;
        let header = firmware
            .get(offset..header_end)
            .ok_or("QCA9377 firmware has a truncated information element header")?;
        let id = u32::from_le_bytes(header[..4].try_into().unwrap());
        let length = usize::try_from(u32::from_le_bytes(header[4..].try_into().unwrap()))
            .map_err(|_| "QCA9377 firmware information element is too large")?;
        let data_end = header_end
            .checked_add(length)
            .ok_or("QCA9377 firmware information element length overflow")?;
        let data = firmware
            .get(header_end..data_end)
            .ok_or("QCA9377 firmware has a truncated information element")?;
        let padded_length =
            align_four(length).ok_or("QCA9377 firmware information element length overflow")?;
        let next = header_end
            .checked_add(padded_length)
            .ok_or("QCA9377 firmware information element offset overflow")?;
        if next > firmware.len() {
            return Err("QCA9377 firmware has truncated information element padding");
        }

        match id {
            FW_IE_VERSION => {
                if data.is_empty() {
                    return Err("QCA9377 firmware has invalid version information");
                }
                let version_text = core::str::from_utf8(data)
                    .map_err(|_| "QCA9377 firmware version is not valid UTF-8")?;
                if version.replace(version_text).is_some() {
                    return Err("QCA9377 firmware has invalid version information");
                }
            }
            FW_IE_OTP_IMAGE => {
                if !data.is_empty() && otp_image.replace(data).is_some() {
                    return Err("QCA9377 firmware has duplicate OTP images");
                }
            }
            FW_IE_IMAGE => {
                if data.is_empty() || firmware_image.replace(data).is_some() {
                    return Err("QCA9377 firmware has invalid firmware image information");
                }
            }
            _ => {}
        }
        offset = next;
    }

    Ok(FirmwareImages {
        version: version.ok_or("QCA9377 firmware has no version information element")?,
        otp_image,
        firmware_image: firmware_image
            .ok_or("QCA9377 firmware has no firmware image information element")?,
    })
}

/// Read the elements that pick the WMI and HTT command layouts.
///
/// ath10k stores `wmi_op_version` and `htt_op_version` while walking the same
/// information elements, then selects an operations table from them. The
/// shipped QCA9377 blob declares WMI-TLV here, which is why the packed codec in
/// `qca9377/wmi.rs` must not be pointed at it.
pub fn firmware_operations(firmware: &[u8]) -> Result<FirmwareOperations, &'static str> {
    if !firmware.starts_with(ATH10K_FIRMWARE_MAGIC) {
        return Err("QCA9377 firmware has an invalid ath10k magic header");
    }
    let mut offset =
        align_four(ATH10K_FIRMWARE_MAGIC.len()).ok_or("QCA9377 firmware magic length overflow")?;
    let mut operations = FirmwareOperations {
        features: 0,
        wmi_op_version: 0,
        htt_op_version: 0,
    };
    while offset < firmware.len() {
        let (id, data, next) = read_ie(firmware, offset, "QCA9377 firmware has an invalid element")?;
        match id {
            // ath10k reads this as a raw bit array; the first word holds bits 0-31.
            FW_IE_FEATURES => {
                let mut word = [0u8; 4];
                let count = core::cmp::min(data.len(), 4);
                word[..count].copy_from_slice(&data[..count]);
                operations.features = u32::from_le_bytes(word);
            }
            FW_IE_WMI_OP_VERSION if data.len() == 4 => {
                operations.wmi_op_version = u32::from_le_bytes(data.try_into().unwrap());
            }
            FW_IE_HTT_OP_VERSION if data.len() == 4 => {
                operations.htt_op_version = u32::from_le_bytes(data.try_into().unwrap());
            }
            _ => {}
        }
        offset = next;
    }
    Ok(operations)
}

fn validate_firmware(firmware: &[u8]) -> Result<(), &'static str> {
    parse_firmware(firmware).map(|_| ())
}

fn validate_board_bundle(board_bundle: &[u8]) -> Result<(), &'static str> {
    if !board_bundle.starts_with(ATH10K_BOARD_MAGIC) {
        return Err("QCA9377 board bundle has an invalid ath10k magic header");
    }

    let mut offset =
        align_four(ATH10K_BOARD_MAGIC.len()).ok_or("QCA9377 board magic length overflow")?;
    if offset > board_bundle.len() {
        return Err("QCA9377 board bundle is truncated after its magic header");
    }

    let mut has_board_data = false;
    while offset < board_bundle.len() {
        let (id, data, next) = read_ie(board_bundle, offset, BOARD_IE_INVALID)?;
        if id == BOARD_IE_DATA {
            validate_board_entries(data)?;
            has_board_data = true;
        } else if id == BOARD_IE_EXT_DATA {
            validate_board_entries(data)?;
        }
        offset = next;
    }

    if !has_board_data {
        return Err("QCA9377 board bundle has no board-data container");
    }
    Ok(())
}

fn validate_board_entries(entries: &[u8]) -> Result<(), &'static str> {
    let mut offset = 0;
    let mut has_named_data = false;
    let mut has_name = false;

    while offset < entries.len() {
        let (id, data, next) = read_ie(entries, offset, BOARD_IE_INVALID)?;
        match id {
            BOARD_ENTRY_NAME => has_name = !data.is_empty(),
            BOARD_ENTRY_DATA if has_name => has_named_data |= !data.is_empty(),
            _ => {}
        }
        offset = next;
    }

    if !has_named_data {
        return Err("QCA9377 board bundle has an incomplete named board entry");
    }
    Ok(())
}

fn find_named_board_data<'a>(
    entries: &'a [u8],
    board_name: &[u8],
) -> Result<Option<&'a [u8]>, &'static str> {
    let mut offset = 0;
    let mut name_matches = false;

    while offset < entries.len() {
        let (id, data, next) = read_ie(entries, offset, BOARD_IE_INVALID)?;
        match id {
            BOARD_ENTRY_NAME => name_matches = data == board_name,
            BOARD_ENTRY_DATA if name_matches && !data.is_empty() => return Ok(Some(data)),
            _ => {}
        }
        offset = next;
    }

    Ok(None)
}

fn read_ie<'a>(
    data: &'a [u8],
    offset: usize,
    error: &'static str,
) -> Result<(u32, &'a [u8], usize), &'static str> {
    let header_end = offset.checked_add(8).ok_or(error)?;
    let header = data.get(offset..header_end).ok_or(error)?;
    let id = u32::from_le_bytes(header[..4].try_into().unwrap());
    let length =
        usize::try_from(u32::from_le_bytes(header[4..].try_into().unwrap())).map_err(|_| error)?;
    let data_end = header_end.checked_add(length).ok_or(error)?;
    let payload = data.get(header_end..data_end).ok_or(error)?;
    let padded_length = align_four(length).ok_or(error)?;
    let next = header_end.checked_add(padded_length).ok_or(error)?;
    if next > data.len() {
        return Err(error);
    }
    Ok((id, payload, next))
}

fn align_four(length: usize) -> Option<usize> {
    length.checked_add(3).map(|value| value & !3)
}

fn find_module<'a>(modules: &'a [&limine::file::File], path: &[u8]) -> Option<&'a [u8]> {
    let file = modules
        .iter()
        .copied()
        .find(|file| file.path().to_bytes() == path)?;
    let length = usize::try_from(file.size()).ok()?;
    let address = file.addr();
    if length == 0 || address.is_null() {
        return None;
    }
    Some(unsafe { core::slice::from_raw_parts(address.cast_const(), length) })
}

#[cfg(test)]
mod tests {
    use super::{
        ATH10K_BOARD_MAGIC, ATH10K_FIRMWARE_MAGIC, firmware_operations, parse_firmware,
        select_board_data,
        validate_firmware,
    };
    use alloc::vec::Vec;

    fn append_ie(firmware: &mut Vec<u8>, id: u32, data: &[u8]) {
        firmware.extend_from_slice(&id.to_le_bytes());
        firmware.extend_from_slice(&(data.len() as u32).to_le_bytes());
        firmware.extend_from_slice(data);
        while firmware.len() % 4 != 0 {
            firmware.push(0);
        }
    }

    fn append_firmware_ie(firmware: &mut Vec<u8>, id: u32, data: &[u8]) {
        firmware.extend_from_slice(&id.to_le_bytes());
        firmware.extend_from_slice(&(data.len() as u32).to_le_bytes());
        firmware.extend_from_slice(data);
        while firmware.len() % 4 != 0 {
            firmware.push(0);
        }
    }

    fn empty_firmware() -> Vec<u8> {
        let mut firmware = ATH10K_FIRMWARE_MAGIC.to_vec();
        while firmware.len() % 4 != 0 {
            firmware.push(0);
        }
        firmware
    }

    fn board_entries(name: &[u8], data: &[u8]) -> Vec<u8> {
        let mut entries = Vec::new();
        append_ie(&mut entries, 0, name);
        append_ie(&mut entries, 1, data);
        entries
    }

    fn board_bundle(name: &[u8], board: &[u8], board_ext: Option<&[u8]>) -> Vec<u8> {
        let mut bundle = ATH10K_BOARD_MAGIC.to_vec();
        while bundle.len() % 4 != 0 {
            bundle.push(0);
        }
        let entries = board_entries(name, board);
        append_ie(&mut bundle, 0, &entries);
        if let Some(board_ext) = board_ext {
            let entries = board_entries(name, board_ext);
            append_ie(&mut bundle, 1, &entries);
        }
        bundle
    }

    #[test]
    fn validates_firmware_magic_and_required_elements() {
        let mut firmware = empty_firmware();
        append_firmware_ie(&mut firmware, 0, b"api 6");
        append_firmware_ie(&mut firmware, 3, &[0xAA, 0x55]);

        assert_eq!(validate_firmware(&firmware), Ok(()));
        let images = parse_firmware(&firmware).unwrap();
        assert_eq!(images.version, "api 6");
        assert_eq!(images.otp_image, None);
        assert_eq!(images.firmware_image, &[0xAA, 0x55]);
    }

    #[test]
    fn rejects_truncated_information_element() {
        let mut firmware = empty_firmware();
        append_firmware_ie(&mut firmware, 3, &[0xAA, 0x55]);
        firmware.truncate(firmware.len() - 3);

        assert_eq!(
            validate_firmware(&firmware),
            Err("QCA9377 firmware has a truncated information element")
        );
    }

    #[test]
    fn rejects_truncated_information_element_padding() {
        let mut firmware = empty_firmware();
        append_firmware_ie(&mut firmware, 3, &[0xAA]);
        firmware.pop();

        assert_eq!(
            validate_firmware(&firmware),
            Err("QCA9377 firmware has truncated information element padding")
        );
    }

    #[test]
    fn rejects_firmware_without_image() {
        let mut firmware = empty_firmware();
        append_firmware_ie(&mut firmware, 0, b"api 6");

        assert_eq!(
            validate_firmware(&firmware),
            Err("QCA9377 firmware has no firmware image information element")
        );
    }

    #[test]
    fn rejects_non_utf8_firmware_version() {
        let mut firmware = empty_firmware();
        append_firmware_ie(&mut firmware, 0, &[0xFF]);

        assert!(matches!(
            parse_firmware(&firmware),
            Err("QCA9377 firmware version is not valid UTF-8")
        ));
    }

    #[test]
    fn reads_the_wmi_and_htt_operations_versions() {
        // The QCA9377 blob ends with features 0xc0, wmi-op 4 and htt-op 3.
        let mut firmware = empty_firmware();
        append_firmware_ie(&mut firmware, 0, b"WLAN.TF.2.1");
        append_firmware_ie(&mut firmware, 3, &[0xAA]);
        append_firmware_ie(&mut firmware, 2, &[0xC0]);
        append_firmware_ie(&mut firmware, 5, &4u32.to_le_bytes());
        append_firmware_ie(&mut firmware, 6, &3u32.to_le_bytes());

        let operations = firmware_operations(&firmware).unwrap();
        assert_eq!(operations.features, 0xC0);
        assert_eq!(operations.wmi_op_version, 4, "WMI-TLV");
        assert_eq!(operations.htt_op_version, 3, "HTT-TLV");
    }

    #[test]
    fn extracts_optional_otp_image() {
        let mut firmware = empty_firmware();
        append_firmware_ie(&mut firmware, 0, b"api 6");
        append_firmware_ie(&mut firmware, 4, &[0x10, 0x20]);
        append_firmware_ie(&mut firmware, 3, &[0x30, 0x40]);

        assert_eq!(
            parse_firmware(&firmware).unwrap().otp_image,
            Some(&[0x10, 0x20][..])
        );
    }

    #[test]
    fn selects_only_the_exact_named_board_data_and_extension() {
        let mut bundle = ATH10K_BOARD_MAGIC.to_vec();
        while bundle.len() % 4 != 0 {
            bundle.push(0);
        }
        let other_board = board_entries(b"bus=pci,bmi-chip-id=1", b"wrong");
        let target_board = board_entries(b"bus=pci,bmi-chip-id=2", b"target");
        append_ie(&mut bundle, 0, &other_board);
        append_ie(&mut bundle, 0, &target_board);
        let extension = board_entries(b"bus=pci,bmi-chip-id=2", b"extension");
        append_ie(&mut bundle, 1, &extension);

        let selected = select_board_data(&bundle, b"bus=pci,bmi-chip-id=2").unwrap();
        assert_eq!(selected.board, b"target");
        assert_eq!(selected.board_ext, Some(&b"extension"[..]));
    }

    #[test]
    fn refuses_a_board_bundle_without_the_requested_name() {
        let bundle = board_bundle(b"bus=pci,bmi-board-id=3", b"calibration", None);

        assert!(select_board_data(&bundle, b"bus=pci,bmi-board-id=4").is_err());
    }

    #[test]
    fn bundled_qca9377_firmware_and_board_data_are_valid() {
        let firmware = include_bytes!("../../../firmware/ath10k/QCA9377/hw1.0/firmware-6.bin");
        let board_bundle = include_bytes!("../../../firmware/ath10k/QCA9377/hw1.0/board-2.bin");
        let images = parse_firmware(firmware).unwrap();
        assert_eq!(images.version, "WLAN.TF.2.1-00021-QCARMSWP-1");

        let board = select_board_data(
            board_bundle,
            b"bus=pci,vendor=168c,device=0042,subsystem-vendor=169c,subsystem-device=3365",
        )
        .unwrap();
        assert!(!board.board.is_empty());

        let e203_board = select_board_data(
            board_bundle,
            b"bus=pci,vendor=168c,device=0042,subsystem-vendor=105b,subsystem-device=e09a",
        )
        .unwrap();
        assert!(!e203_board.board.is_empty());
    }

    #[test]
    fn rejects_a_truncated_board_bundle() {
        let mut bundle = board_bundle(b"board", b"calibration", None);
        bundle.pop();

        assert!(select_board_data(&bundle, b"board").is_err());
    }
}
