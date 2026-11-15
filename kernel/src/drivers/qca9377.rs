//! PCI discovery and firmware readiness checks for the ASUS E203NAH WLAN.

pub mod bmi;
pub mod ce;
pub mod htc;
pub mod htt;
pub mod init;
pub mod target;
pub mod wmi;

use super::{
    pci::{self, PciDevice},
    wifi_firmware,
};
use crate::memory;
use bmi::{BmiClient, BmiClientError, BmiTransport, TargetInfo};
use ce::RegisterIo;

const QUALCOMM_ATHEROS_VENDOR_ID: u16 = 0x168c;
const QCA9377_PCI_DEVICE_ID: u16 = 0x0042;
const QCA6174_RTC_SOC_BASE: u64 = 0x0000_0800;
const QCA6174_SOC_CHIP_ID_OFFSET: u64 = 0x0000_00f0;
const QCA6174_PCIE_LOCAL_BASE: u64 = 0x0008_0000;
const QCA6174_RTC_STATE_ADDRESS: u64 = 0;
pub(super) const QCA6174_PCIE_SOC_WAKE_ADDRESS: u64 = QCA6174_PCIE_LOCAL_BASE + 0x4;
pub(super) const CHIP_ID_BAR_OFFSET: u64 = QCA6174_RTC_SOC_BASE + QCA6174_SOC_CHIP_ID_OFFSET;
pub(super) const RTC_STATE_BAR_OFFSET: u64 = QCA6174_PCIE_LOCAL_BASE + QCA6174_RTC_STATE_ADDRESS;
const RTC_STATE_V_MASK: u32 = 0x7;
const RTC_STATE_V_ON: u32 = 3;
const PCIE_SOC_WAKE_V_MASK: u32 = 1;
const SOC_CHIP_ID_REV_LSB: u32 = 8;
const SOC_CHIP_ID_REV_MASK: u32 = 0x0000_0f00;
const MMIO_READ_SIZE: u64 = core::mem::size_of::<u32>() as u64;
pub(super) const PAGE_OFFSET_MASK: u64 = 0xfff;
pub(super) const QCA_WAKE_POLL_LIMIT: usize = 10_000_000;
const QCA9377_TARGET_VERSION_1_0: u32 = 0x0502_0000;
const QCA9377_TARGET_VERSION_1_1: u32 = 0x0502_0001;
const HOST_INTEREST_ADDRESS: u32 = 0x0040_0800;
const HOST_INTEREST_BOARD_DATA_OFFSET: u32 = 0x54;
const HOST_INTEREST_BOARD_DATA_INITIALIZED_OFFSET: u32 = 0x58;
const QCA9377_BOARD_DATA_MAX_SIZE: usize = 8192;
pub(super) const QCA9377_OTP_LOAD_ADDRESS: u32 = 0x0040_4000;
pub(super) const QCA9377_FIRMWARE_LOAD_ADDRESS: u32 = 0x0040_8000;

// ── QCA6174/QCA9377 SoC reset window ─────────────────────────────────────
// BAR0 offsets shared by QCA6174 hw3.x and QCA9377 hw1.x, from Linux
// `ath10k/hw.h` and `ath10k/pci.c`.
const QCA6174_SOC_CORE_BASE: u64 = 0x0003_a000;
const QCA6174_FW_INDICATOR_ADDRESS: u64 = QCA6174_SOC_CORE_BASE + 0x28;
const QCA6174_SOC_GLOBAL_RESET_ADDRESS: u64 = QCA6174_PCIE_LOCAL_BASE + 0x8;
const QCA6174_RTC_SOC_TIMER_OFFSET: u64 = 0x50;
const SOC_GLOBAL_RESET_PULSE: u32 = 1 << 0;
const RTC_SOC_CE_RESET: u32 = 1 << 0;
const RTC_SOC_CPU_RESET: u32 = 1 << 6;
const RTC_SOC_TIMER_LPO_SELECT: u32 = 1 << 2;
const FW_IND_INITIALIZED: u32 = 1 << 1;
const FW_IND_EVENT_PENDING: u32 = 1 << 0;
/// Roughly 20 ms at the E203NAH's Celeron N3350 spin-loop rate.
const RESET_SETTLE_SPINS: usize = 2_000_000;

const fn required_register_window_size() -> u64 {
    let chip_id_end = CHIP_ID_BAR_OFFSET + MMIO_READ_SIZE;
    let rtc_state_end = RTC_STATE_BAR_OFFSET + MMIO_READ_SIZE;
    let wake_register_end = QCA6174_PCIE_SOC_WAKE_ADDRESS + MMIO_READ_SIZE;
    let max_target_register_end = if chip_id_end > rtc_state_end {
        chip_id_end
    } else {
        rtc_state_end
    };
    if max_target_register_end > wake_register_end {
        max_target_register_end
    } else {
        wake_register_end
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum BmiBootError<E> {
    Transport(BmiClientError<E>),
    UnsupportedTarget(TargetInfo),
    OtpExecutionFailed(u32),
    InvalidBoardData,
    InvalidBoardDataAddress,
    AddressOverflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SiliconIdentity {
    Qca9377Hw10,
    Qca9377Hw11,
    Qca6174Hw32,
    UnknownRevision(u8),
}

#[derive(Debug)]
enum BmiProbeError {
    Setup(&'static str),
    Operation {
        stage: &'static str,
        error: ce::CeError,
    },
    BusMasterDisable,
}

const fn is_ath10k_pci_candidate(vendor_id: u16, device_id: u16) -> bool {
    vendor_id == QUALCOMM_ATHEROS_VENDOR_ID && device_id == QCA9377_PCI_DEVICE_ID
}

fn supports_qca9377_firmware(target_info: TargetInfo) -> bool {
    matches!(
        target_info.version,
        QCA9377_TARGET_VERSION_1_0 | QCA9377_TARGET_VERSION_1_1
    )
}

pub(super) fn silicon_identity(chip_id: u32) -> SiliconIdentity {
    let revision = ((chip_id & SOC_CHIP_ID_REV_MASK) >> SOC_CHIP_ID_REV_LSB) as u8;
    match revision {
        0 => SiliconIdentity::Qca9377Hw10,
        1 => SiliconIdentity::Qca9377Hw11,
        10 => SiliconIdentity::Qca6174Hw32,
        other => SiliconIdentity::UnknownRevision(other),
    }
}

pub(super) fn target_wake<R: ce::RegisterIo>(
    registers: &mut R,
    poll_limit: usize,
) -> Result<(), ce::CeError> {
    registers.write32(QCA6174_PCIE_SOC_WAKE_ADDRESS, PCIE_SOC_WAKE_V_MASK)?;
    for _ in 0..poll_limit {
        let current = registers.read32(RTC_STATE_BAR_OFFSET)?;
        if current == u32::MAX {
            let _ = target_sleep(registers);
            return Err(ce::CeError::DeviceUnavailable);
        }
        if current & RTC_STATE_V_MASK == RTC_STATE_V_ON {
            return Ok(());
        }
        core::hint::spin_loop();
    }
    let _ = target_sleep(registers);
    Err(ce::CeError::Timeout)
}

pub(super) fn target_sleep<R: ce::RegisterIo>(registers: &mut R) -> Result<(), ce::CeError> {
    registers.write32(QCA6174_PCIE_SOC_WAKE_ADDRESS, 0)
}

pub(super) fn wake_target_cpu<R: ce::RegisterIo>(registers: &mut R) -> Result<(), ce::CeError> {
    let current = registers.read32(ce::QCA6174_CORE_CTRL_ADDRESS)?;
    registers.write32(
        ce::QCA6174_CORE_CTRL_ADDRESS,
        current | ce::CORE_CTRL_CPU_INTR_MASK,
    )
}

fn settle_reset_delay() {
    for _ in 0..RESET_SETTLE_SPINS {
        core::hint::spin_loop();
    }
}

/// Pulse the SoC global reset latch (cold reset) so the PCIe link and RTC
/// restart from a known state.
pub(super) fn soc_global_reset<R: ce::RegisterIo>(registers: &mut R) -> Result<(), ce::CeError> {
    let current = registers.read32(QCA6174_SOC_GLOBAL_RESET_ADDRESS)?;
    if current == u32::MAX {
        return Err(ce::CeError::DeviceUnavailable);
    }
    registers.write32(
        QCA6174_SOC_GLOBAL_RESET_ADDRESS,
        current | SOC_GLOBAL_RESET_PULSE,
    )?;
    settle_reset_delay();
    registers.write32(
        QCA6174_SOC_GLOBAL_RESET_ADDRESS,
        current & !SOC_GLOBAL_RESET_PULSE,
    )?;
    settle_reset_delay();
    Ok(())
}

/// Clear the stale firmware indicator and put the target CPU through reset,
/// matching `ath10k_pci_reset_cpu`. The boot ROM rewrites the indicator once
/// it is ready to service BMI.
pub(super) fn reset_target_cpu<R: ce::RegisterIo>(
    registers: &mut R,
) -> Result<(), ce::CeError> {
    registers.write32(QCA6174_FW_INDICATOR_ADDRESS, 0)?;
    let rtc = registers.read32(QCA6174_RTC_SOC_BASE)?;
    if rtc == u32::MAX {
        return Err(ce::CeError::DeviceUnavailable);
    }
    registers.write32(QCA6174_RTC_SOC_BASE, rtc | RTC_SOC_CPU_RESET)
}

/// Cycle the copy-engine reset line so the boot ROM programs the CE ring
/// registers from scratch instead of inheriting a stale configuration.
pub(super) fn pulse_ce_reset<R: ce::RegisterIo>(
    registers: &mut R,
) -> Result<(), ce::CeError> {
    let timer = registers.read32(QCA6174_RTC_SOC_BASE + QCA6174_RTC_SOC_TIMER_OFFSET)?;
    if timer == u32::MAX {
        return Err(ce::CeError::DeviceUnavailable);
    }
    registers.write32(
        QCA6174_RTC_SOC_BASE + QCA6174_RTC_SOC_TIMER_OFFSET,
        timer & !RTC_SOC_TIMER_LPO_SELECT,
    )?;

    let rtc = registers.read32(QCA6174_RTC_SOC_BASE)?;
    if rtc == u32::MAX {
        return Err(ce::CeError::DeviceUnavailable);
    }
    registers.write32(QCA6174_RTC_SOC_BASE, rtc | RTC_SOC_CE_RESET)?;
    settle_reset_delay();
    registers.write32(QCA6174_RTC_SOC_BASE, rtc & !RTC_SOC_CE_RESET)
}

/// Wait for the boot ROM to report `FW_IND_INITIALIZED`.
///
/// Returns the readiness flag together with the last `FW_INDICATOR` value that
/// was read, so the caller can log the live register instead of guessing. A
/// crash event or an expiring wait returns `Ok((false, value))`; only an
/// unusable register read is an error.
pub(super) fn wait_for_target_init<R: ce::RegisterIo>(
    registers: &mut R,
    poll_limit: usize,
) -> Result<(bool, u32), ce::CeError> {
    let mut indicator = 0;
    for _ in 0..poll_limit {
        indicator = registers.read32(QCA6174_FW_INDICATOR_ADDRESS)?;
        if indicator == u32::MAX {
            return Err(ce::CeError::DeviceUnavailable);
        }
        if indicator & FW_IND_EVENT_PENDING != 0 {
            return Ok((false, indicator));
        }
        if indicator & FW_IND_INITIALIZED != 0 {
            return Ok((true, indicator));
        }
        core::hint::spin_loop();
    }
    Ok((false, indicator))
}

/// Result of a completed reset attempt, for the boot log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ResetOutcome {
    pub indicator_before: u32,
    pub indicator_after: u32,
    pub ready: bool,
}

/// A reset step that failed, so the boot log can say where the target stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ResetFailure {
    pub step: &'static str,
    pub error: ce::CeError,
}

/// Reset the target SoC and release its CPU so the boot ROM services BMI.
///
/// ath10k always cold-resets QCA6174/QCA9377 and resets the target CPU before
/// the first BMI command. Without it CE0's source ring is never consumed and
/// the first exchange fails with `SourceTimeout`.
///
/// On success the target is left awake; the caller must keep the wake request
/// asserted for the whole BMI transfer.
pub(super) fn reset_for_bmi<R: ce::RegisterIo>(
    registers: &mut R,
    poll_limit: usize,
) -> Result<ResetOutcome, ResetFailure> {
    let failed = |step, error| ResetFailure { step, error };

    let indicator_before = registers
        .read32(QCA6174_FW_INDICATOR_ADDRESS)
        .map_err(|error| failed("read FW_INDICATOR", error))?;
    target_wake(registers, poll_limit).map_err(|error| failed("wake target", error))?;
    soc_global_reset(registers).map_err(|error| failed("SoC global reset", error))?;
    target_wake(registers, poll_limit)
        .map_err(|error| failed("wake after cold reset", error))?;
    reset_target_cpu(registers).map_err(|error| failed("reset target CPU", error))?;
    pulse_ce_reset(registers).map_err(|error| failed("reset copy engines", error))?;
    reset_target_cpu(registers)
        .map_err(|error| failed("reset target CPU after CE reset", error))?;
    let (ready, indicator_after) = wait_for_target_init(registers, poll_limit)
        .map_err(|error| failed("wait for boot ROM", error))?;
    target_wake(registers, poll_limit).map_err(|error| failed("final wake", error))?;

    Ok(ResetOutcome {
        indicator_before,
        indicator_after,
        ready,
    })
}

pub(super) fn with_target_awake<R: ce::RegisterIo, T>(
    registers: &mut R,
    poll_limit: usize,
    operation: impl FnOnce(&mut R) -> Result<T, ce::CeError>,
) -> Result<T, ce::CeError> {
    target_wake(registers, poll_limit)?;
    let result = operation(registers);
    let sleep_result = target_sleep(registers);
    match (result, sleep_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        (_, Err(error)) => Err(error),
    }
}

pub fn download_board_data<T: BmiTransport>(
    client: &mut BmiClient<T>,
    board_data: &[u8],
) -> Result<u32, BmiBootError<T::Error>> {
    if board_data.is_empty() {
        return Err(BmiBootError::InvalidBoardData);
    }

    let board_data_address_location = HOST_INTEREST_ADDRESS
        .checked_add(HOST_INTEREST_BOARD_DATA_OFFSET)
        .ok_or(BmiBootError::AddressOverflow)?;
    let mut address_bytes = [0; 4];
    client
        .read_memory(board_data_address_location, &mut address_bytes)
        .map_err(BmiBootError::Transport)?;
    let board_data_address = u32::from_le_bytes(address_bytes);
    let write_length = board_data.len().min(QCA9377_BOARD_DATA_MAX_SIZE);
    let write_length_u32 =
        u32::try_from(write_length).map_err(|_| BmiBootError::InvalidBoardData)?;
    if board_data_address == 0
        || board_data_address == u32::MAX
        || board_data_address & 3 != 0
        || board_data_address.checked_add(write_length_u32).is_none()
    {
        return Err(BmiBootError::InvalidBoardDataAddress);
    }

    client
        .write_memory(board_data_address, &board_data[..write_length])
        .map_err(BmiBootError::Transport)?;

    let initialized_address = HOST_INTEREST_ADDRESS
        .checked_add(HOST_INTEREST_BOARD_DATA_INITIALIZED_OFFSET)
        .ok_or(BmiBootError::AddressOverflow)?;
    client
        .write_memory(initialized_address, &1_u32.to_le_bytes())
        .map_err(BmiBootError::Transport)?;
    Ok(board_data_address)
}

/// Run the BMI-only QCA9377 initialization sequence.
///
/// # Safety
/// The target must be awake, CE rings configured, PCI bus mastering enabled,
/// and `board_data` must match the detected device. Keep the hardware disabled
/// in the boot path until its lifecycle is validated.
pub unsafe fn boot_firmware<T: BmiTransport>(
    client: &mut BmiClient<T>,
    firmware: wifi_firmware::FirmwareImages<'_>,
    board_data: &[u8],
) -> Result<TargetInfo, BmiBootError<T::Error>> {
    let target_info = client.get_target_info().map_err(BmiBootError::Transport)?;
    if !supports_qca9377_firmware(target_info) {
        return Err(BmiBootError::UnsupportedTarget(target_info));
    }

    download_board_data(client, board_data)?;
    if let Some(otp_image) = firmware.otp_image {
        let result = client
            .download_and_run_otp(QCA9377_OTP_LOAD_ADDRESS, 0, otp_image)
            .map_err(BmiBootError::Transport)?;
        if result != 0 {
            return Err(BmiBootError::OtpExecutionFailed(result));
        }
    }
    client
        .download_and_start_firmware(QCA9377_FIRMWARE_LOAD_ADDRESS, firmware.firmware_image)
        .map_err(BmiBootError::Transport)?;
    Ok(target_info)
}

/// Configured CE pipe pairs for HTC endpoints.
///
/// Pipe map (from ath10k pci.c):
///   EP0 Control: TX=CE0, RX=CE1
///   WMI Control: TX=CE3, RX=CE2
///   HTT Data:    TX=CE4, RX=CE5
pub struct HtcPipes {
    /// EP0 Control TX pipe (CE0)
    pub ep0_tx: ce::TxCe,
    /// EP0 Control RX pipe (CE1)
    pub ep0_rx: ce::RxCe,
    /// WMI Control TX pipe (CE3)
    pub wmi_tx: ce::TxCe,
    /// WMI Control RX pipe (CE2)
    pub wmi_rx: ce::RxCe,
    /// HTT Data TX pipe (CE4)
    pub htt_tx: ce::TxCe,
    /// HTT Data RX pipe (CE5)
    pub htt_rx: ce::RxCe,
}

/// Configure the HTC CE pipe pairs after BMI completes.
///
/// Pipe map (from ath10k pci.c):
///   EP0 Control: TX=CE0, RX=CE1
///   WMI Control: TX=CE3, RX=CE2
///   HTT Data:    TX=CE4, RX=CE5
///
/// # Safety
/// BMI must be complete, PCI bus mastering must be enabled, and the register
/// window referenced by `registers` must be valid for all CE offsets.
pub unsafe fn init_htc_pipes<R: ce::RegisterIo>(
    registers: &mut R,
) -> Result<HtcPipes, ce::CeError> {
    let max = ce::CE_PACKET_MAX_BYTES;

    let mut ep0_tx = ce::TxCe::allocate(ce::PIPE_EP0_TX_CE, max)?;
    let mut ep0_rx = ce::RxCe::allocate(ce::PIPE_EP0_RX_CE, max)?;
    let mut wmi_tx = ce::TxCe::allocate(ce::PIPE_WMI_TX_CE, max)?;
    let mut wmi_rx = ce::RxCe::allocate(ce::PIPE_WMI_RX_CE, max)?;
    let mut htt_tx = ce::TxCe::allocate(ce::PIPE_HTT_TX_CE, max)?;
    let mut htt_rx = ce::RxCe::allocate(ce::PIPE_HTT_RX_CE, max)?;

    ep0_tx.configure(registers)?;
    ep0_rx.configure(registers)?;
    wmi_tx.configure(registers)?;
    wmi_rx.configure(registers)?;
    htt_tx.configure(registers)?;
    htt_rx.configure(registers)?;

    Ok(HtcPipes {
        ep0_tx,
        ep0_rx,
        wmi_tx,
        wmi_rx,
        htt_tx,
        htt_rx,
    })
}

pub(super) fn pci_board_name(
    device: &PciDevice,
    subsystem_vendor: u16,
    subsystem_device: u16,
) -> alloc::string::String {
    alloc::format!(
        "bus=pci,vendor={:04x},device={:04x},subsystem-vendor={:04x},subsystem-device={:04x}",
        device.vendor_id,
        device.device_id,
        subsystem_vendor,
        subsystem_device
    )
}

fn read_target_registers(device: &PciDevice) -> Result<(u32, u32), &'static str> {
    let bar = unsafe { pci::get_bar0(device) };
    if bar.address == 0 {
        return Err("QCA9377 BAR0 is not assigned");
    }
    let bar_size = unsafe { pci::get_bar0_size(device) };
    let required_size = required_register_window_size();
    if bar_size < required_size {
        return Err("QCA9377 BAR0 is too small for target identification");
    }
    unsafe { pci::enable_mmio_only(device) }?;

    let page_offset = bar.address & PAGE_OFFSET_MASK;
    let mapping_size = page_offset
        .checked_add(required_size)
        .ok_or("QCA9377 MMIO mapping size overflow")?;
    let mmio = unsafe { memory::paging::map_mmio_range(bar.address, mapping_size) };
    let bar_virt = mmio + page_offset;
    let mut registers = unsafe { ce::MmioWindow::new(bar_virt as usize, bar_size) }
        .map_err(|_| "QCA9377 MMIO window is unavailable")?;
    with_target_awake(&mut registers, QCA_WAKE_POLL_LIMIT, |registers| {
        let rtc_state = registers.read32(RTC_STATE_BAR_OFFSET)?;
        let chip_id = registers.read32(CHIP_ID_BAR_OFFSET)?;
        if chip_id == u32::MAX {
            return Err(ce::CeError::DeviceUnavailable);
        }
        Ok((rtc_state, chip_id))
    })
    .map_err(|_| "QCA9377 target wake or register read failed")
}

fn probe_bmi_target_info(device: &PciDevice) -> Result<TargetInfo, BmiProbeError> {
    let bar = unsafe { pci::get_bar0(device) };
    if bar.address == 0 {
        return Err(BmiProbeError::Setup("BAR0 is not assigned"));
    }
    let bar_size = unsafe { pci::get_bar0_size(device) };
    let required_size = required_register_window_size();
    if bar_size < required_size {
        return Err(BmiProbeError::Setup(
            "BAR0 is too small for BMI target-info",
        ));
    }
    unsafe { pci::enable_mmio_only(device) }.map_err(BmiProbeError::Setup)?;

    let page_offset = bar.address & PAGE_OFFSET_MASK;
    let mapping_size = page_offset
        .checked_add(required_size)
        .ok_or(BmiProbeError::Setup("MMIO mapping size overflow"))?;
    let mmio = unsafe { memory::paging::map_mmio_range(bar.address, mapping_size) };
    let bar_virt = mmio + page_offset;
    let mut registers = unsafe { ce::MmioWindow::new(bar_virt as usize, bar_size) }
        .map_err(|_| BmiProbeError::Setup("MMIO window is unavailable"))?;
    let mut rings = ce::BmiCeRings::allocate()
        .map_err(|_| BmiProbeError::Setup("BMI DMA ring allocation failed"))?;
    let mut keep_rings = false;
    let mut bus_master_may_be_enabled = false;
    let mut failure_stage = None;

    let result = with_target_awake(&mut registers, QCA_WAKE_POLL_LIMIT, |registers| {
        failure_stage = Some("validate live SoC identity");
        let chip_id = registers.read32(CHIP_ID_BAR_OFFSET)?;
        if !matches!(
            silicon_identity(chip_id),
            SiliconIdentity::Qca9377Hw10 | SiliconIdentity::Qca9377Hw11
        ) {
            return Err(ce::CeError::DeviceUnavailable);
        }

        keep_rings = true;
        failure_stage = Some("configure CE0/CE1 BMI rings");
        unsafe { rings.configure(registers)? };
        failure_stage = Some("enable PCI bus mastering");
        bus_master_may_be_enabled = true;
        if unsafe { pci::enable_bus_master(device) }.is_err() {
            if unsafe { pci::disable_bus_master(device) }.is_ok() {
                bus_master_may_be_enabled = false;
            }
            return Err(ce::CeError::DeviceUnavailable);
        }

        let target_info_result = {
            let transport = unsafe { ce::BmiCeTransport::new(&mut rings, registers) };
            let mut client = BmiClient::new(transport);
            client.get_target_info().map_err(|error| match error {
                BmiClientError::Transport(error) => error,
                BmiClientError::Protocol(_) => ce::CeError::InvalidResponseLength,
            })
        };
        failure_stage = Some("disable PCI bus mastering");
        if unsafe { pci::disable_bus_master(device) }.is_ok() {
            bus_master_may_be_enabled = false;
        } else {
            return Err(ce::CeError::DeviceUnavailable);
        }

        failure_stage = Some("send BMI GET_TARGET_INFO");
        let target_info = target_info_result?;
        failure_stage = Some("clear CE ring registers");
        unsafe { rings.deconfigure(registers)? };
        keep_rings = false;
        Ok(target_info)
    });

    let bus_master_disable_failed =
        bus_master_may_be_enabled && unsafe { pci::disable_bus_master(device) }.is_err();
    if bus_master_disable_failed {
        keep_rings = true;
    }
    if keep_rings {
        // The CE registers may still reference these pages after a failed transfer.
        core::mem::forget(rings);
    }

    if bus_master_disable_failed {
        return Err(BmiProbeError::BusMasterDisable);
    }
    result.map_err(|error| BmiProbeError::Operation {
        stage: failure_stage.unwrap_or("request wake and wait for RTC ON"),
        error,
    })
}

pub(crate) fn report_pci_candidates(devices: &[PciDevice]) {
    let mut found = false;
    for device in devices {
        if !is_ath10k_pci_candidate(device.vendor_id, device.device_id) {
            continue;
        }
        found = true;
        let revision = pci::read_config_u32(device, 0x08) as u8;
        let subsystem = pci::read_config_u32(device, 0x2c);
        let subsystem_vendor = subsystem as u16;
        let subsystem_device = (subsystem >> 16) as u16;
        crate::println!(
            "Qualcomm Atheros ath10k PCI candidate {:02x}:{:02x}.{} id={:#06x}:{:#06x} revision={:#04x} subsystem={:#06x}:{:#06x}",
            device.bus,
            device.device,
            device.function,
            device.vendor_id,
            device.device_id,
            revision,
            subsystem_vendor,
            subsystem_device
        );
        crate::println!(
            "PCI id 168c:0042 is shared by QCA9377 and QCA6174 hw3.2; probing live SoC chip revision"
        );

        let qca9377_revision = match read_target_registers(device) {
            Ok((rtc_state, chip_id)) => {
                crate::println!(
                    "Qualcomm target registers: RTC_STATE={rtc_state:#010x} SOC_CHIP_ID@{CHIP_ID_BAR_OFFSET:#x}={chip_id:#010x}"
                );
                match silicon_identity(chip_id) {
                    SiliconIdentity::Qca9377Hw10 => {
                        crate::println!("Qualcomm silicon identity: QCA9377 hw1.0");
                        true
                    }
                    SiliconIdentity::Qca9377Hw11 => {
                        crate::println!("Qualcomm silicon identity: QCA9377 hw1.1");
                        true
                    }
                    SiliconIdentity::Qca6174Hw32 => {
                        crate::println!(
                            "Qualcomm silicon identity: QCA6174 hw3.2; QCA9377 firmware will not be used"
                        );
                        false
                    }
                    SiliconIdentity::UnknownRevision(revision) => {
                        crate::println!("Qualcomm silicon revision is unsupported: {revision}");
                        false
                    }
                }
            }
            Err(error) => {
                crate::println!("Qualcomm target register probe unavailable: {error}");
                false
            }
        };

        match wifi_firmware::qca9377() {
            Ok(firmware) => {
                crate::println!(
                    "QCA9377 firmware modules loaded: version={} image={} bytes otp={} bytes board_bundle={} bytes",
                    firmware.firmware_version,
                    firmware.firmware_image.len(),
                    firmware.otp_image.map_or(0, |image| image.len()),
                    firmware.board_bundle.len()
                );
                let board_name = pci_board_name(device, subsystem_vendor, subsystem_device);
                match wifi_firmware::select_board_data(firmware.board_bundle, board_name.as_bytes())
                {
                    Ok(board) => crate::println!(
                        "QCA9377 board data selected: name={} board={} bytes extension={} bytes",
                        board_name,
                        board.board.len(),
                        board.board_ext.map_or(0, |extension| extension.len())
                    ),
                    Err(error) => {
                        crate::println!("QCA9377 board data unavailable for {board_name}: {error}")
                    }
                }
            }
            Err(error) => crate::println!("QCA9377 firmware unavailable: {error}"),
        }

        // ── Full initialization attempt ───────────────────────────────────────
        if qca9377_revision {
            match unsafe { init::initialize_device(device, subsystem_vendor, subsystem_device) } {
                Ok(mac) => {
                    crate::println!(
                        "QCA9377 Wi-Fi active: MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
                    );
                }
                Err(e) => {
                    crate::println!("QCA9377 initialization failed: {e:?}");
                }
            }
        } else {
            crate::println!("QCA9377 Wi-Fi skipped: silicon not identified as QCA9377");
        }
    }

    if !found {
        crate::println!("No Qualcomm Atheros ath10k PCI candidate found");
    }
}

#[cfg(test)]
mod tests {
    use super::bmi::{BmiClient, BmiTransport, TargetInfo};
    use super::ce::{CeError, RegisterIo};
    use super::{
        BmiBootError, CHIP_ID_BAR_OFFSET, FW_IND_EVENT_PENDING, FW_IND_INITIALIZED,
        HOST_INTEREST_ADDRESS, QCA6174_FW_INDICATOR_ADDRESS, QCA6174_PCIE_SOC_WAKE_ADDRESS,
        QCA6174_RTC_SOC_BASE, QCA6174_RTC_SOC_TIMER_OFFSET, QCA6174_SOC_GLOBAL_RESET_ADDRESS,
        QCA9377_TARGET_VERSION_1_0, QCA9377_TARGET_VERSION_1_1, RTC_SOC_CPU_RESET,
        RTC_STATE_BAR_OFFSET, SiliconIdentity, download_board_data, is_ath10k_pci_candidate,
        pci_board_name, required_register_window_size, reset_for_bmi, silicon_identity,
        soc_global_reset, supports_qca9377_firmware, wait_for_target_init, with_target_awake,
    };
    use crate::drivers::pci::PciDevice;
    use crate::drivers::wifi_firmware::FirmwareImages;

    #[derive(Default)]
    struct MockBmiTransport {
        requests: alloc::vec::Vec<alloc::vec::Vec<u8>>,
        responses: alloc::vec::Vec<alloc::vec::Vec<u8>>,
    }

    impl BmiTransport for MockBmiTransport {
        type Error = &'static str;

        fn exchange(
            &mut self,
            request: &[u8],
            response: Option<&mut [u8]>,
        ) -> Result<usize, Self::Error> {
            self.requests.push(request.into());
            if let Some(response) = response {
                let Some(next_response) = self.responses.first() else {
                    return Err("mock response queue is empty");
                };
                if response.len() != next_response.len() {
                    return Err("unexpected mock response size");
                }
                let next_response = self.responses.remove(0);
                response.copy_from_slice(&next_response);
                return Ok(next_response.len());
            }
            Ok(0)
        }
    }

    fn read_word(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    struct MockWakeRegisters {
        awake: bool,
        can_wake: bool,
        chip_id: u32,
        writes: alloc::vec::Vec<(u64, u32)>,
    }

    impl RegisterIo for MockWakeRegisters {
        fn read32(&mut self, offset: u64) -> Result<u32, CeError> {
            match offset {
                RTC_STATE_BAR_OFFSET => Ok(if self.awake { 0x73 } else { 0x71 }),
                CHIP_ID_BAR_OFFSET => Ok(self.chip_id),
                _ => Err(CeError::RegisterOutOfRange),
            }
        }

        fn write32(&mut self, offset: u64, value: u32) -> Result<(), CeError> {
            if offset != QCA6174_PCIE_SOC_WAKE_ADDRESS {
                return Err(CeError::RegisterOutOfRange);
            }
            self.writes.push((offset, value));
            self.awake = value != 0 && self.can_wake;
            Ok(())
        }
    }

    /// Reset-window mock: the boot ROM is emulated by reporting
    /// `FW_IND_INITIALIZED` as soon as the CPU reset bit is written.
    #[derive(Default)]
    struct MockResetRegisters {
        awake: bool,
        values: alloc::collections::BTreeMap<u64, u32>,
        writes: alloc::vec::Vec<(u64, u32)>,
    }

    impl RegisterIo for MockResetRegisters {
        fn read32(&mut self, offset: u64) -> Result<u32, CeError> {
            if offset == RTC_STATE_BAR_OFFSET {
                return Ok(if self.awake { 0x73 } else { 0x71 });
            }
            Ok(self.values.get(&offset).copied().unwrap_or(0))
        }

        fn write32(&mut self, offset: u64, value: u32) -> Result<(), CeError> {
            self.writes.push((offset, value));
            self.values.insert(offset, value);
            match offset {
                QCA6174_PCIE_SOC_WAKE_ADDRESS => self.awake = value & 1 != 0,
                QCA6174_RTC_SOC_BASE if value & RTC_SOC_CPU_RESET != 0 => {
                    self.values
                        .insert(QCA6174_FW_INDICATOR_ADDRESS, FW_IND_INITIALIZED);
                }
                _ => {}
            }
            Ok(())
        }
    }

    #[test]
    fn matches_qca9377_pci_id() {
        assert!(is_ath10k_pci_candidate(0x168c, 0x0042));
    }

    #[test]
    fn reset_for_bmi_cold_resets_and_releases_the_target_cpu() {
        let mut registers = MockResetRegisters::default();
        let outcome = reset_for_bmi(&mut registers, 8).unwrap();
        assert!(outcome.ready);
        assert_eq!(outcome.indicator_before, 0);
        assert_eq!(outcome.indicator_after, FW_IND_INITIALIZED);
        assert!(registers.awake);

        let global_reset: alloc::vec::Vec<u32> = registers
            .writes
            .iter()
            .filter(|(offset, _)| *offset == QCA6174_SOC_GLOBAL_RESET_ADDRESS)
            .map(|(_, value)| *value)
            .collect();
        assert_eq!(global_reset, [1, 0]);

        let rtc_writes: alloc::vec::Vec<u32> = registers
            .writes
            .iter()
            .filter(|(offset, _)| *offset == QCA6174_RTC_SOC_BASE)
            .map(|(_, value)| *value)
            .collect();
        assert!(rtc_writes.iter().any(|value| value & RTC_SOC_CPU_RESET != 0));
        assert!(rtc_writes.iter().any(|value| value & 1 != 0));
        assert!(rtc_writes.iter().any(|value| value & 1 == 0));

        assert!(registers
            .writes
            .iter()
            .any(|(offset, value)| *offset == QCA6174_FW_INDICATOR_ADDRESS && *value == 0));
        assert!(registers.writes.iter().any(|(offset, value)| *offset
            == QCA6174_RTC_SOC_BASE + QCA6174_RTC_SOC_TIMER_OFFSET
            && *value & 4 == 0));
        assert_eq!(
            registers.writes.last(),
            Some(&(QCA6174_PCIE_SOC_WAKE_ADDRESS, 1))
        );
    }

    #[test]
    fn target_init_wait_tracks_rom_readiness_and_crash_events() {
        let mut registers = MockResetRegisters::default();
        assert_eq!(wait_for_target_init(&mut registers, 8), Ok((false, 0)));

        registers
            .values
            .insert(QCA6174_FW_INDICATOR_ADDRESS, FW_IND_INITIALIZED);
        assert_eq!(
            wait_for_target_init(&mut registers, 8),
            Ok((true, FW_IND_INITIALIZED))
        );

        registers
            .values
            .insert(QCA6174_FW_INDICATOR_ADDRESS, FW_IND_EVENT_PENDING);
        assert_eq!(
            wait_for_target_init(&mut registers, 8),
            Ok((false, FW_IND_EVENT_PENDING))
        );
    }

    #[test]
    fn reset_failure_names_the_step_that_stopped() {
        let mut registers = MockResetRegisters::default();
        registers
            .values
            .insert(QCA6174_SOC_GLOBAL_RESET_ADDRESS, u32::MAX);
        let failure = reset_for_bmi(&mut registers, 8).unwrap_err();
        assert_eq!(failure.step, "SoC global reset");
        assert_eq!(failure.error, CeError::DeviceUnavailable);
    }

    #[test]
    fn cold_reset_reports_a_missing_device() {
        let mut registers = MockResetRegisters::default();
        registers
            .values
            .insert(QCA6174_SOC_GLOBAL_RESET_ADDRESS, u32::MAX);
        assert_eq!(
            soc_global_reset(&mut registers),
            Err(CeError::DeviceUnavailable)
        );
    }

    #[test]
    fn maps_qca6174_shared_registers_at_the_documented_bar_offsets() {
        assert_eq!(CHIP_ID_BAR_OFFSET, 0x0000_08f0);
        assert_eq!(RTC_STATE_BAR_OFFSET, 0x0008_0000);
        assert_eq!(QCA6174_PCIE_SOC_WAKE_ADDRESS, 0x0008_0004);
        assert_eq!(required_register_window_size(), 0x0008_0008);
    }

    #[test]
    fn target_register_reads_wake_then_return_device_to_sleep() {
        let mut registers = MockWakeRegisters {
            awake: false,
            can_wake: true,
            chip_id: 0,
            writes: alloc::vec::Vec::new(),
        };
        let (state, chip_id) = with_target_awake(&mut registers, 2, |registers| {
            Ok((
                registers.read32(RTC_STATE_BAR_OFFSET)?,
                registers.read32(CHIP_ID_BAR_OFFSET)?,
            ))
        })
        .unwrap();
        assert_eq!(state & 0x7, 3);
        assert_eq!(chip_id, 0);
        assert_eq!(
            registers.writes,
            [
                (QCA6174_PCIE_SOC_WAKE_ADDRESS, 1),
                (QCA6174_PCIE_SOC_WAKE_ADDRESS, 0)
            ]
        );
        assert!(!registers.awake);
    }

    #[test]
    fn failed_target_wake_is_bounded_and_clears_the_wake_request() {
        let mut registers = MockWakeRegisters {
            awake: false,
            can_wake: false,
            chip_id: 0,
            writes: alloc::vec::Vec::new(),
        };
        let mut operation_called = false;
        assert_eq!(
            with_target_awake(&mut registers, 2, |_| {
                operation_called = true;
                Ok(())
            }),
            Err(CeError::Timeout)
        );
        assert!(!operation_called);
        assert_eq!(
            registers.writes,
            [
                (QCA6174_PCIE_SOC_WAKE_ADDRESS, 1),
                (QCA6174_PCIE_SOC_WAKE_ADDRESS, 0)
            ]
        );
    }

    #[test]
    fn decodes_shared_pci_id_silicon_revision_from_soc_chip_id() {
        assert_eq!(silicon_identity(0x0000_0000), SiliconIdentity::Qca9377Hw10);
        assert_eq!(silicon_identity(0x0000_0100), SiliconIdentity::Qca9377Hw11);
        assert_eq!(silicon_identity(0x0000_0a00), SiliconIdentity::Qca6174Hw32);
        assert_eq!(
            silicon_identity(0x0000_0200),
            SiliconIdentity::UnknownRevision(2)
        );
    }

    #[test]
    fn rejects_other_pci_ids() {
        assert!(!is_ath10k_pci_candidate(0x168c, 0x003e));
        assert!(!is_ath10k_pci_candidate(0x1234, 0x0042));
    }

    #[test]
    fn matches_only_qca9377_bmi_firmware_versions() {
        assert!(supports_qca9377_firmware(TargetInfo {
            version: QCA9377_TARGET_VERSION_1_0,
            target_type: 0,
        }));
        assert!(supports_qca9377_firmware(TargetInfo {
            version: QCA9377_TARGET_VERSION_1_1,
            target_type: 0,
        }));
        assert!(!supports_qca9377_firmware(TargetInfo {
            version: 0x0503_0000,
            target_type: 0,
        }));
    }

    #[test]
    fn downloads_board_data_before_marking_it_initialized() {
        let transport = MockBmiTransport {
            responses: alloc::vec![0x2000_u32.to_le_bytes().into()],
            ..MockBmiTransport::default()
        };
        let mut client = BmiClient::new(transport);
        assert_eq!(download_board_data(&mut client, &[1, 2, 3, 4]), Ok(0x2000));

        let transport = client.into_transport();
        assert_eq!(transport.requests.len(), 3);
        assert_eq!(read_word(&transport.requests[0], 0), 2);
        assert_eq!(
            read_word(&transport.requests[0], 4),
            HOST_INTEREST_ADDRESS + 0x54
        );
        assert_eq!(read_word(&transport.requests[1], 0), 3);
        assert_eq!(read_word(&transport.requests[1], 4), 0x2000);
        assert_eq!(read_word(&transport.requests[1], 8), 4);
        assert_eq!(&transport.requests[1][12..16], &[1, 2, 3, 4]);
        assert_eq!(read_word(&transport.requests[2], 0), 3);
        assert_eq!(
            read_word(&transport.requests[2], 4),
            HOST_INTEREST_ADDRESS + 0x58
        );
        assert_eq!(&transport.requests[2][12..16], &1_u32.to_le_bytes());
    }

    #[test]
    fn refuses_empty_board_data_and_invalid_target_addresses() {
        let transport = MockBmiTransport::default();
        let mut client = BmiClient::new(transport);
        assert_eq!(
            download_board_data(&mut client, &[]),
            Err(BmiBootError::InvalidBoardData)
        );
        assert!(client.into_transport().requests.is_empty());

        let transport = MockBmiTransport {
            responses: alloc::vec![0_u32.to_le_bytes().into()],
            ..MockBmiTransport::default()
        };
        let mut client = BmiClient::new(transport);
        assert_eq!(
            download_board_data(&mut client, &[1, 2, 3, 4]),
            Err(BmiBootError::InvalidBoardDataAddress)
        );
        assert_eq!(client.into_transport().requests.len(), 1);
    }

    #[test]
    fn boot_sequence_checks_target_then_prepares_board_otp_and_firmware() {
        let mut target_info_response = alloc::vec::Vec::new();
        target_info_response.extend_from_slice(&12_u32.to_le_bytes());
        target_info_response.extend_from_slice(&QCA9377_TARGET_VERSION_1_0.to_le_bytes());
        target_info_response.extend_from_slice(&1_u32.to_le_bytes());
        let transport = MockBmiTransport {
            responses: alloc::vec![
                target_info_response,
                0x2000_u32.to_le_bytes().into(),
                0_u32.to_le_bytes().into(),
            ],
            ..MockBmiTransport::default()
        };
        let mut client = BmiClient::new(transport);
        let firmware = FirmwareImages {
            version: "mock",
            otp_image: Some(&[5, 6, 7, 8]),
            firmware_image: &[1, 2, 3, 4],
        };
        assert_eq!(
            unsafe { super::boot_firmware(&mut client, firmware, &[0xaa, 0xbb, 0xcc, 0xdd]) },
            Ok(TargetInfo {
                version: QCA9377_TARGET_VERSION_1_0,
                target_type: 1,
            })
        );

        let transport = client.into_transport();
        let command_ids: alloc::vec::Vec<u32> = transport
            .requests
            .iter()
            .map(|request| read_word(request, 0))
            .collect();
        assert_eq!(command_ids, [8, 2, 3, 3, 13, 14, 13, 4, 13, 14, 13, 5, 1]);
    }

    #[test]
    fn boot_sequence_rejects_non_qca9377_target_before_board_writes() {
        let mut target_info_response = alloc::vec::Vec::new();
        target_info_response.extend_from_slice(&12_u32.to_le_bytes());
        target_info_response.extend_from_slice(&0x0503_0000_u32.to_le_bytes());
        target_info_response.extend_from_slice(&1_u32.to_le_bytes());
        let transport = MockBmiTransport {
            responses: alloc::vec![target_info_response],
            ..MockBmiTransport::default()
        };
        let mut client = BmiClient::new(transport);
        let firmware = FirmwareImages {
            version: "mock",
            otp_image: None,
            firmware_image: &[1, 2, 3, 4],
        };
        assert_eq!(
            unsafe { super::boot_firmware(&mut client, firmware, &[1, 2, 3, 4]) },
            Err(BmiBootError::UnsupportedTarget(TargetInfo {
                version: 0x0503_0000,
                target_type: 1,
            }))
        );
        assert_eq!(client.into_transport().requests.len(), 1);
    }

    #[test]
    fn builds_linux_compatible_pci_board_name() {
        let device = PciDevice {
            bus: 2,
            device: 0,
            function: 0,
            vendor_id: 0x168c,
            device_id: 0x0042,
            class: 0x02,
            subclass: 0x80,
            prog_if: 0,
            header_type: 0,
        };
        assert_eq!(
            pci_board_name(&device, 0x105b, 0xe09a),
            "bus=pci,vendor=168c,device=0042,subsystem-vendor=105b,subsystem-device=e09a"
        );
    }
}
