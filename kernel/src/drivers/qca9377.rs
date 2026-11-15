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
use crate::arch::x86_64::io;
use crate::memory;
use bmi::{BmiClient, BmiClientError, BmiTransport, TargetInfo};
use ce::RegisterIo;

/// IDT vector the target's MSI is routed to. ath10k takes the single MSI vector
/// `pci_enable_msi()` hands out; this kernel owns the IDT, so the vector is
/// fixed here and enabled with `pci::enable_msi()`.
pub const QCA9377_MSI_VECTOR: u8 = 45;

/// Service a target interrupt from the IDT vector above.
///
/// The handler only acknowledges the PCIe cause and records which copy engines
/// signalled (`ath10k_pci_interrupt_handler` + `ath10k_ce_per_engine_service_any`);
/// the frames themselves are copied out in task context.
pub fn handle_irq() -> bool {
    ce::interrupt::handle()
}

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
// Remaining host-interest words `ath10k_init_configure_target` and
// `ath10k_init_uart` touch, at the offsets from `ath10k_targaddrs.h`.
const HOST_INTEREST_APP_HOST_INTEREST_OFFSET: u32 = 0x00;
const HOST_INTEREST_OPTION_FLAG_OFFSET: u32 = 0x10;
const HOST_INTEREST_SERIAL_ENABLE_OFFSET: u32 = 0x14;
const HOST_INTEREST_BE_OFFSET: u32 = 0x44;
const HOST_INTEREST_HCI_UART_PWR_MGMT_EXT_OFFSET: u32 = 0xbc;
const HOST_INTEREST_FW_SWAP_OFFSET: u32 = 0x104;
/// `HTC_PROTOCOL_VERSION` from ath10k (2.0).
const HTC_PROTOCOL_VERSION: u32 = 0x0002;
/// `hi_option_flag` fields set by `ath10k_init_configure_target`.
const HI_OPTION_MAC_ADDR_METHOD_SHIFT: u32 = 3;
const HI_OPTION_NUM_DEV_SHIFT: u32 = 9;
const HI_OPTION_FW_MODE_AP: u32 = 0x2;
const HI_OPTION_FW_MODE_SHIFT: u32 = 0xc;
const QCA9377_BOARD_DATA_MAX_SIZE: usize = 8192;
pub(super) const QCA9377_OTP_LOAD_ADDRESS: u32 = 0x0040_4000;
pub(super) const QCA9377_FIRMWARE_LOAD_ADDRESS: u32 = 0x0040_8000;

// ── QCA6174/QCA9377 SoC reset window ─────────────────────────────────────
// BAR0 offsets shared by QCA6174 hw3.x and QCA9377 hw1.x, from Linux
// `ath10k/hw.h` and `ath10k/pci.c`.
const QCA6174_SOC_CORE_BASE: u64 = 0x0003_a000;
const QCA6174_FW_INDICATOR_ADDRESS: u64 = QCA6174_SOC_CORE_BASE + 0x28;
// `ath10k_pci_reg_write32()` is a wrapper that adds `PCIE_LOCAL_BASE_ADDRESS`
// (`return ath10k_pci_write32(ar, PCIE_LOCAL_BASE_ADDRESS + addr, val)`), so
// `SOC_GLOBAL_RESET_ADDRESS` (0x0008) really lives at BAR + 0x80008, next to
// RTC_STATE (+0x0) and PCIE_SOC_WAKE (+0x4) in the same window.
const QCA6174_SOC_GLOBAL_RESET_ADDRESS: u64 = QCA6174_PCIE_LOCAL_BASE + 0x8;
// Raw probe addresses for the cold-reset register dump: the BAR base window and
// the PCIe-local window, so one boot shows which one the latch really uses.
const QCA6174_BAR_PROBE_0: u64 = 0x0000_0000;
const QCA6174_BAR_PROBE_8: u64 = 0x0000_0008;
const QCA6174_PCIE_LOCAL_PROBE_8: u64 = QCA6174_SOC_GLOBAL_RESET_ADDRESS;
const QCA6174_RTC_SOC_TIMER_OFFSET: u64 = 0x50;
const SOC_GLOBAL_RESET_PULSE: u32 = 1 << 0;
const RTC_SOC_CE_RESET: u32 = 1 << 0;
const RTC_SOC_CPU_RESET: u32 = 1 << 6;
// `qca6174_regs.soc_reset_control_si0_rst_mask` in `ath10k_hw.c` is 0x0, so on
// QCA9377 `ath10k_pci_warm_reset_si0` only reads the register and waits. The
// step is kept because it still spends its 20 ms settling time exactly where
// ath10k does.
const RTC_SOC_SI0_RESET: u32 = 0;
const RTC_SOC_TIMER_LPO_SELECT: u32 = 1 << 2;
const FW_IND_INITIALIZED: u32 = 1 << 1;
const FW_IND_EVENT_PENDING: u32 = 1 << 0;

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

/// Read `SOC_CHIP_ID` with the target awake and leave it awake.
///
/// ath10k never writes `PCIE_SOC_WAKE=0` before `ath10k_pci_chip_reset`: for
/// QCA6174/QCA9377 `ath10k_pci_force_wake()` returns immediately (`pci_ps` is
/// true), so the SoC stays up from probe until the boot ROM finishes init.
/// Sleeping the target between the identity read and the reset was one more
/// difference to the working reference, and the reset, BMI and CE7 paths all
/// need the SoC awake anyway.
pub(super) fn read_chip_id_awake<R: ce::RegisterIo>(
    registers: &mut R,
    poll_limit: usize,
) -> Result<u32, ce::CeError> {
    target_wake(registers, poll_limit)?;
    let chip_id = registers.read32(CHIP_ID_BAR_OFFSET)?;
    if chip_id == u32::MAX {
        return Err(ce::CeError::DeviceUnavailable);
    }
    Ok(chip_id)
}

pub(super) fn wake_target_cpu<R: ce::RegisterIo>(registers: &mut R) -> Result<(), ce::CeError> {
    let current = registers.read32(ce::QCA6174_CORE_CTRL_ADDRESS)?;
    registers.write32(
        ce::QCA6174_CORE_CTRL_ADDRESS,
        current | ce::CORE_CTRL_CPU_INTR_MASK,
    )
}

/// Busy-wait for `milliseconds`, the equivalent of ath10k's `msleep()` around
/// the reset registers.
///
/// The count comes from the 8254 PIT channel 2, which runs at a fixed
/// 1.193182 MHz on every PC, so the delay does not depend on the CPU speed the
/// way `RESET_SETTLE_SPINS` did. ath10k calls out why the delays matter for
/// `SOC_GLOBAL_RESET`: "pulling out of reset pcie may not be stable for any
/// immediate pcie register access and cause bus error, add delay before any
/// pcie access request".
fn settle_reset_delay(milliseconds: u16) {
    // Tests must not touch real hardware ports; the delay only matters there.
    if cfg!(test) {
        return;
    }
    const PIT_HZ: u32 = 1_193_182;
    let ticks = ((PIT_HZ / 1000) * u32::from(milliseconds)).clamp(1, 0xffff);
    unsafe {
        // Speaker off, channel 2 gate enabled (bit 0 of port 0x61).
        let port_61 = io::inb(0x61);
        io::outb(0x61, (port_61 & 0xfd) | 0x01);
        // Channel 2, mode 0 (interrupt on terminal count), lobyte/hibyte.
        io::outb(0x43, 0xb0);
        io::outb(0x42, (ticks & 0xff) as u8);
        io::outb(0x42, ((ticks >> 8) & 0xff) as u8);
        // OUT2 (port 0x61 bit 5) rises when the count reaches zero.
        while io::inb(0x61) & 0x20 == 0 {
            core::hint::spin_loop();
        }
    }
}

/// Pulse the SoC global reset latch (cold reset) so the PCIe link and RTC
/// restart from a known state.
///
/// Mirrors `ath10k_pci_cold_reset`: assert bit 0, wait for the PCIe link to
/// settle, release it, wait again. Returns the `RTC_STATE` value read while the
/// SoC was held in reset, so the boot log can show whether the latch actually
/// reached the chip instead of assuming it did.
pub(super) fn soc_global_reset<R: ce::RegisterIo>(
    registers: &mut R,
) -> Result<u32, ce::CeError> {
    let current = registers.read32(QCA6174_SOC_GLOBAL_RESET_ADDRESS)?;
    if current == u32::MAX {
        return Err(ce::CeError::DeviceUnavailable);
    }
    registers.write32(
        QCA6174_SOC_GLOBAL_RESET_ADDRESS,
        current | SOC_GLOBAL_RESET_PULSE,
    )?;
    // ath10k: 20 ms held in reset, 20 ms after release, before any PCIe access.
    settle_reset_delay(20);
    let held = registers.read32(RTC_STATE_BAR_OFFSET)?;
    registers.write32(
        QCA6174_SOC_GLOBAL_RESET_ADDRESS,
        current & !SOC_GLOBAL_RESET_PULSE,
    )?;
    settle_reset_delay(20);
    Ok(held)
}

/// Check that `FW_INDICATOR` is the writable register the driver believes it
/// is: write zero and read it back.
///
/// A target that keeps reporting `FW_IND_EVENT_PENDING` right after every reset
/// looks crashed, but the same log shows the chip id answering at +1 ms, so
/// before blaming the boot ROM the driver has to know whether the indicator
/// write lands at all. ath10k never checks this because its address comes from
/// `ar->regs->fw_indicator_address`; a bare-metal driver hardcodes it.
fn fw_indicator_accepts_zero<R: ce::RegisterIo>(
    registers: &mut R,
) -> Result<bool, ce::CeError> {
    registers.write32(QCA6174_FW_INDICATOR_ADDRESS, 0)?;
    Ok(registers.read32(QCA6174_FW_INDICATOR_ADDRESS)? == 0)
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

/// `ath10k_pci_warm_reset_si0`: settle the SI0 subsystem before the target CPU
/// is reset, because ath10k does it to keep the target out of the copy engines
/// while the host resets them.
fn warm_reset_si0<R: ce::RegisterIo>(registers: &mut R) -> Result<(), ce::CeError> {
    let current = registers.read32(QCA6174_RTC_SOC_BASE)?;
    if current == u32::MAX {
        return Err(ce::CeError::DeviceUnavailable);
    }
    if RTC_SOC_SI0_RESET != 0 {
        registers.write32(QCA6174_RTC_SOC_BASE, current | RTC_SOC_SI0_RESET)?;
        settle_reset_delay(10);
        let held = registers.read32(QCA6174_RTC_SOC_BASE)?;
        if held == u32::MAX {
            return Err(ce::CeError::DeviceUnavailable);
        }
        registers.write32(QCA6174_RTC_SOC_BASE, held & !RTC_SOC_SI0_RESET)?;
    }
    settle_reset_delay(10);
    Ok(())
}

/// Clear the latched `FW_IND_EVENT_PENDING` bit without touching the other
/// indicator bits, matching `ath10k_pci_fw_crashed_clear`.
///
/// The target raises that bit when it crashes during init and the host is the
/// only agent that can clear it again, so a failed chip reset has to release
/// the latch before the next attempt can tell a fresh crash from a leftover
/// one.
fn clear_fw_crash<R: ce::RegisterIo>(registers: &mut R) -> Result<u32, ce::CeError> {
    let current = registers.read32(QCA6174_FW_INDICATOR_ADDRESS)?;
    if current == u32::MAX {
        return Err(ce::CeError::DeviceUnavailable);
    }
    let cleared = current & !FW_IND_EVENT_PENDING;
    registers.write32(QCA6174_FW_INDICATOR_ADDRESS, cleared)?;
    Ok(cleared)
}

/// Cycle the copy-engine reset line so the boot ROM programs the CE ring
/// registers from scratch instead of inheriting a stale configuration.
pub(super) fn pulse_ce_reset<R: ce::RegisterIo>(
    registers: &mut R,
) -> Result<(), ce::CeError> {
    // `ath10k_pci_warm_reset_clear_lf`: stop the low-frequency timer first,
    // otherwise it keeps waking the target while the CE block is being reset.
    let timer = registers.read32(QCA6174_RTC_SOC_BASE + QCA6174_RTC_SOC_TIMER_OFFSET)?;
    if timer == u32::MAX {
        return Err(ce::CeError::DeviceUnavailable);
    }
    registers.write32(
        QCA6174_RTC_SOC_BASE + QCA6174_RTC_SOC_TIMER_OFFSET,
        timer & !RTC_SOC_TIMER_LPO_SELECT,
    )?;

    // `ath10k_pci_warm_reset_ce` reads `SOC_RESET_CONTROL` fresh here and writes
    // the original value back with only the CE bit cleared, so the CPU warm
    // reset bit stays set across the pulse.
    let rtc = registers.read32(QCA6174_RTC_SOC_BASE)?;
    if rtc == u32::MAX {
        return Err(ce::CeError::DeviceUnavailable);
    }
    registers.write32(QCA6174_RTC_SOC_BASE, rtc | RTC_SOC_CE_RESET)?;
    settle_reset_delay(10);
    registers.write32(QCA6174_RTC_SOC_BASE, rtc & !RTC_SOC_CE_RESET)
}

/// Polling rounds and their spacing. ath10k waits `ATH10K_PCI_TARGET_WAIT`
/// (3000 ms) with a 10 ms delay per iteration, so the rounds here are sized to
/// spend roughly three seconds in total instead of a few CPU-speed-dependent
/// spins.
const ROM_READY_ROUNDS: usize = 150;

/// Wait for the boot ROM to report `FW_IND_INITIALIZED`.
///
/// `ath10k_pci_wait_for_target_init` stops on the first sample that carries
/// `FW_IND_EVENT_PENDING` (`"the device has crashed so don't bother trying
/// anymore"`) and only keeps polling otherwise, so a crashed target is reported
/// immediately instead of after the full timeout. An expiring wait returns
/// `Ok((false, value))`; only an unusable register read is an error.
pub(super) fn wait_for_target_init<R: ce::RegisterIo>(
    registers: &mut R,
    poll_limit: usize,
) -> Result<(bool, u32), ce::CeError> {
    let mut indicator = 0;
    let spins_per_round = (poll_limit / ROM_READY_ROUNDS).max(1);
    for round in 0..ROM_READY_ROUNDS {
        indicator = registers.read32(QCA6174_FW_INDICATOR_ADDRESS)?;
        if indicator == u32::MAX {
            return Err(ce::CeError::DeviceUnavailable);
        }
        // ath10k tests `FW_IND_EVENT_PENDING` before `FW_IND_INITIALIZED`, so an
        // indicator carrying both bits is a crash, not a ready target.
        if indicator & FW_IND_EVENT_PENDING != 0 {
            return Ok((false, indicator));
        }
        if indicator & FW_IND_INITIALIZED != 0 {
            return Ok((true, indicator));
        }
        for _ in 0..spins_per_round {
            core::hint::spin_loop();
        }
        if round + 1 < ROM_READY_ROUNDS {
            // ath10k polls with `mdelay(10)` between samples.
            settle_reset_delay(10);
        }
    }
    Ok((false, indicator))
}

/// Millisecond ladder used to sample the SoC window after the cold reset.
///
/// `ath10k_pci_cold_reset` warns that "pulling out of reset pcie may not be
/// stable for any immediate pcie register access and cause bus error", so one
/// sample right after the release cannot tell a dead window from a window that
/// simply needs a few milliseconds.
const TIMELINE_CHECKPOINTS_MS: [u16; 6] = [1, 5, 20, 100, 500, 3000];
const TIMELINE_SAMPLES: usize = TIMELINE_CHECKPOINTS_MS.len();
/// Values that mean "this read never reached the device" rather than a real
/// register content.
pub(super) const READ_FAILED_PATTERN: u32 = 0xdead_c0de;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TimelineSample {
    pub elapsed_ms: u16,
    /// `RTC_STATE` (PCIE_LOCAL + 0x0) after the wake request was re-driven.
    pub rtc_state: u32,
    /// `SOC_GLOBAL_RESET` (PCIE_LOCAL + 0x8).
    pub soc_global_reset: u32,
    /// BAR base window + 0x0, the address the previous dump already sampled.
    pub bar0: u32,
    /// `SOC_CHIP_ID` (RTC_SOC + 0xf0); ath10k reads 0x003821ff here on the
    /// laptops that boot this device.
    pub chip_id: u32,
    pub fw_indicator: u32,
    /// `SOC_RESET_CONTROL` (RTC_SOC + 0x0), so a warm reset bit that stays set
    /// after the pulse is visible in the same line as the rest.
    pub soc_reset_control: u32,
    pub awake: bool,
}

impl TimelineSample {
    const EMPTY: Self = Self {
        elapsed_ms: 0,
        rtc_state: 0,
        soc_global_reset: 0,
        bar0: 0,
        chip_id: 0,
        fw_indicator: 0,
        soc_reset_control: 0,
        awake: false,
    };
}

/// Millisecond-by-millisecond record of the SoC window after the cold reset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ChipIdTimeline {
    samples: [TimelineSample; TIMELINE_SAMPLES],
    len: usize,
    /// First checkpoint that returned a real `SOC_CHIP_ID`; `None` means the
    /// window never answered inside the 3 s ladder.
    pub first_valid_ms: Option<u16>,
}

impl ChipIdTimeline {
    const fn empty() -> Self {
        Self {
            samples: [TimelineSample::EMPTY; TIMELINE_SAMPLES],
            len: 0,
            first_valid_ms: None,
        }
    }

    pub(super) fn samples(&self) -> &[TimelineSample] {
        &self.samples[..self.len]
    }
}

/// A zero, all-ones or bus-error read means no register answered.
pub(super) fn chip_id_is_valid(value: u32) -> bool {
    value != 0 && value != u32::MAX && value != READ_FAILED_PATTERN
}

/// Sample the SoC window across [`TIMELINE_CHECKPOINTS_MS`] after the cold
/// reset, re-driving `PCIE_SOC_WAKE` at every step.
///
/// The reset drops the wake request the driver set before it, so the first read
/// after the release can hit a sleeping SoC and return zeros; re-asserting wake
/// and waiting for `RTC_STATE` first is what `ath10k_pci_force_wake` does. The
/// walk stops at the first valid chip id, and otherwise keeps the whole ladder
/// so the boot log proves the window never answered instead of guessing.
pub(super) fn sample_chip_id_timeline<R: ce::RegisterIo>(
    registers: &mut R,
) -> Result<ChipIdTimeline, ce::CeError> {
    let mut timeline = ChipIdTimeline::empty();
    let mut elapsed = 0u16;
    for checkpoint in TIMELINE_CHECKPOINTS_MS {
        settle_reset_delay(checkpoint.saturating_sub(elapsed));
        elapsed = elapsed.max(checkpoint);

        registers.write32(QCA6174_PCIE_SOC_WAKE_ADDRESS, PCIE_SOC_WAKE_V_MASK)?;
        let rtc_state = registers
            .read32(RTC_STATE_BAR_OFFSET)
            .unwrap_or(u32::MAX);
        let sample = TimelineSample {
            elapsed_ms: elapsed,
            rtc_state,
            awake: rtc_state & RTC_STATE_V_MASK == RTC_STATE_V_ON,
            soc_global_reset: registers
                .read32(QCA6174_SOC_GLOBAL_RESET_ADDRESS)
                .unwrap_or(u32::MAX),
            bar0: registers.read32(QCA6174_BAR_PROBE_0).unwrap_or(u32::MAX),
            chip_id: registers
                .read32(CHIP_ID_BAR_OFFSET)
                .unwrap_or(u32::MAX),
            fw_indicator: registers
                .read32(QCA6174_FW_INDICATOR_ADDRESS)
                .unwrap_or(u32::MAX),
            soc_reset_control: registers.read32(QCA6174_RTC_SOC_BASE).unwrap_or(u32::MAX),
        };
        timeline.samples[timeline.len] = sample;
        timeline.len += 1;
        if chip_id_is_valid(sample.chip_id) {
            timeline.first_valid_ms = Some(elapsed);
            break;
        }
    }
    Ok(timeline)
}

/// Result of the cold reset, carried until the target CPU is released.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TargetReset {
    pub indicator_before: u32,
    /// `SOC_RESET_CONTROL` before any reset work, so the boot log can show what
    /// the SoC was holding and whether our writes actually change it.
    pub reset_control_before: u32,
    /// `RTC_STATE` (PCIE_LOCAL + 0x0) sampled while `SOC_GLOBAL_RESET` was
    /// asserted.
    pub cold_reset_state: u32,
    /// `FW_INDICATOR` after the cold reset, before the CPU warm reset. ath10k
    /// waits for `FW_IND_INITIALIZED` here, so a value without bit 1 means the
    /// ROM did not come back after the cold reset.
    pub cold_indicator: u32,
    pub cold_ready: bool,
    /// True when writing zero to `FW_INDICATOR` reads back as zero, so the
    /// indicator address and its write path are confirmed.
    pub fw_indicator_writable: bool,
    /// Raw registers sampled immediately before the cold reset.
    pub dump_before: ResetRegisterDump,
    /// The same registers sampled immediately after the cold reset.
    pub dump_after: ResetRegisterDump,
    /// Chip-id/wake samples taken across the first three seconds after the cold
    /// reset, showing when (or whether) the SoC window answered.
    pub chip_id_timeline: ChipIdTimeline,
    /// True when the latched crash bit was cleared after the cold reset and the
    /// ROM then answered `FW_IND_INITIALIZED`, which is the retry
    /// `ath10k_pci_hif_power_up` performs on a failed chip reset.
    pub rom_started_after_clear: bool,
}

/// Raw snapshot of the four registers that decide whether `0x8` really is the
/// cold reset latch: the BAR base window and the PCIe-local window. One boot
/// then proves which window the latch lives in instead of assuming it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ResetRegisterDump {
    pub bar0: u32,
    pub bar8: u32,
    pub local0: u32,
    pub local8: u32,
}

impl ResetRegisterDump {
    /// A read that fails is recorded as `u32::MAX` so a diagnostic dump never
    /// aborts the reset sequence; the bad value is still visible in the log.
    fn capture<R: ce::RegisterIo>(registers: &mut R) -> Self {
        Self {
            bar0: registers.read32(QCA6174_BAR_PROBE_0).unwrap_or(u32::MAX),
            bar8: registers.read32(QCA6174_BAR_PROBE_8).unwrap_or(u32::MAX),
            local0: registers.read32(RTC_STATE_BAR_OFFSET).unwrap_or(u32::MAX),
            local8: registers
                .read32(QCA6174_PCIE_LOCAL_PROBE_8)
                .unwrap_or(u32::MAX),
        }
    }
}

/// Result of a completed reset attempt, for the boot log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ResetOutcome {
    pub indicator_before: u32,
    pub indicator_after: u32,
    pub ready: bool,
    /// Live `SOC_RESET_CONTROL` (RTC_SOC + 0x0) read back right after the CPU
    /// warm reset bit was written.
    pub reset_control_asserted: u32,
    /// True when the CPU warm reset bit was still set when it was read back.
    /// QCA9377 clears it by itself, so a live unit reports `false` here and the
    /// write is only an edge.
    pub cpu_warm_reset_visible: bool,
    /// How many warm-reset phases (1 or 2) ran before the ROM answered.
    pub attempts: u32,
    /// True if the ROM ever reported `FW_IND_EVENT_PENDING`, which ath10k
    /// treats as "device crashed during init".
    pub saw_crash: bool,
    /// True when the latched crash bit was released after the failed attempts.
    pub crash_cleared: bool,
}

/// Phases of one warm reset, matching `ath10k_pci_warm_reset`: the first resets
/// the target CPU and lets the boot ROM pick up the CE pipes, the second repeats
/// it after the low-frequency timer and the copy-engine reset.
const WARM_RESET_PHASES: u32 = 2;

/// Complete chip resets (cold reset, CE rings, warm reset) attempted before the
/// bring-up gives up, matching the loop in `ath10k_pci_qca988x_chip_reset`.
/// A target that did not come up on one cold reset can still come up on the
/// next one, so the retry re-drives the SOC_GLOBAL_RESET rather than only
/// pulsing the CPU.
pub(super) const CHIP_RESET_ATTEMPTS: u32 = 3;

/// A reset step that failed, so the boot log can say where the target stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ResetFailure {
    pub step: &'static str,
    pub error: ce::CeError,
}

/// Cold-reset the target SoC and cycle the copy engines.
///
/// This runs *before* the BMI CE rings are programmed, because the CE reset
/// pulse clears the ring registers. The target CPU is deliberately left in
/// reset here; `release_target_cpu` starts it once the rings exist.
///
/// ath10k orders this the same way for QCA6174/QCA9377:
/// `ath10k_pci_warm_reset_cpu` -> `ath10k_pci_init_pipes` ->
/// `ath10k_pci_wait_for_target_init`.
pub(super) fn reset_target_soc<R: ce::RegisterIo>(
    registers: &mut R,
    poll_limit: usize,
) -> Result<TargetReset, ResetFailure> {
    let failed = |step, error| ResetFailure { step, error };

    let indicator_before = registers
        .read32(QCA6174_FW_INDICATOR_ADDRESS)
        .map_err(|error| failed("read FW_INDICATOR", error))?;
    let reset_control_before = registers
        .read32(QCA6174_RTC_SOC_BASE)
        .map_err(|error| failed("read SOC_RESET_CONTROL", error))?;
    let fw_indicator_writable = fw_indicator_accepts_zero(registers)
        .map_err(|error| failed("verify FW_INDICATOR write", error))?;
    target_wake(registers, poll_limit).map_err(|error| failed("wake target", error))?;
    let dump_before = ResetRegisterDump::capture(registers);
    let cold_reset_state =
        soc_global_reset(registers).map_err(|error| failed("SoC global reset", error))?;
    target_wake(registers, poll_limit)
        .map_err(|error| failed("wake after cold reset", error))?;
    let dump_after = ResetRegisterDump::capture(registers);
    let chip_id_timeline = sample_chip_id_timeline(registers)
        .map_err(|error| failed("sample chip id window", error))?;

    // `ath10k_pci_qca6174_chip_reset` waits for target init between the cold and
    // warm reset. Keeping the value tells the boot log whether the ROM ever came
    // back after the cold reset, instead of only seeing it after the warm reset.
    let (mut cold_ready, mut cold_indicator) =
        wait_for_target_init(registers, poll_limit)
            .map_err(|error| failed("wait for boot ROM after cold reset", error))?;

    // `ath10k_pci_hif_power_up` does not stop at "firmware crashed during chip
    // reset": it clears the latched `FW_IND_EVENT_PENDING` and goes on. The ROM
    // answers only once that bit is gone, so the wait is repeated after the
    // clear instead of reporting the stale latch as the device's final state.
    let mut rom_started_after_clear = cold_ready;
    if !cold_ready && cold_indicator & FW_IND_EVENT_PENDING != 0 {
        if clear_fw_crash(registers).is_ok() {
            let (ready, value) = wait_for_target_init(registers, poll_limit).map_err(|error| {
                failed("wait for boot ROM after clearing the crash latch", error)
            })?;
            cold_ready = ready;
            cold_indicator = value;
            rom_started_after_clear = ready;
        }
    }

    pulse_ce_reset(registers).map_err(|error| failed("reset copy engines", error))?;

    Ok(TargetReset {
        indicator_before,
        reset_control_before,
        cold_reset_state,
        cold_indicator,
        cold_ready,
        fw_indicator_writable,
        dump_before,
        dump_after,
        chip_id_timeline,
        rom_started_after_clear,
    })
}

/// Release the target CPU so the boot ROM services BMI, then wait for
/// `FW_IND_INITIALIZED`.
///
/// Runs ath10k's `ath10k_pci_warm_reset` in its two phases and in its order:
/// reset the CPU warm-reset line and program the CE pipes, wait for the boot
/// ROM; then clear the low-frequency timer, pulse the copy-engine reset (which
/// wipes the ring registers), reset the CPU again, program the pipes a second
/// time and wait again. `program_pipes` is invoked in exactly those two places,
/// because the ROM only services CE0 once it can see a complete configuration.
///
/// The `SOC_RESET_CONTROL` readback is kept for the boot log, because a warm
/// reset bit that does not stick is the difference between "ROM never started"
/// and "ROM started but never signalled".
pub(super) fn release_target_cpu<R, F>(
    registers: &mut R,
    poll_limit: usize,
    reset: TargetReset,
    mut program_pipes: F,
) -> Result<ResetOutcome, ResetFailure>
where
    R: ce::RegisterIo,
    F: FnMut(&mut R) -> Result<(), ce::CeError>,
{
    let failed = |step, error| ResetFailure { step, error };

    let mut ready = false;
    let mut indicator_after = 0;
    let mut reset_control_asserted = 0;
    let mut cpu_warm_reset_visible = false;
    let mut attempts = 0;
    let mut saw_crash = false;

    // `ath10k_pci_warm_reset` opens with `ath10k_pci_warm_reset_si0` so the
    // target is not touching the copy engines while the host resets them.
    warm_reset_si0(registers).map_err(|error| failed("settle SI0 reset", error))?;

    // `ath10k_pci_warm_reset`: CPU warm reset -> `init_pipes` -> wait (ath10k
    // ignores this result) -> clear the low-frequency timer -> CE reset pulse ->
    // CPU warm reset -> `init_pipes` -> wait (this one decides). Both phases
    // always run, because the first wait still sees `FW_IND_EVENT_PENDING` left
    // by the firmware that was running before the reset.
    for phase in 0..WARM_RESET_PHASES {
        attempts += 1;
        reset_target_cpu(registers).map_err(|error| failed("reset target CPU", error))?;
        reset_control_asserted = registers
            .read32(QCA6174_RTC_SOC_BASE)
            .map_err(|error| failed("read SOC_RESET_CONTROL", error))?;
        cpu_warm_reset_visible |= reset_control_asserted & RTC_SOC_CPU_RESET != 0;

        program_pipes(registers).map_err(|error| failed("program CE pipes", error))?;

        let (phase_ready, value) = wait_for_target_init(registers, poll_limit)
            .map_err(|error| failed("wait for boot ROM", error))?;
        indicator_after = value;
        saw_crash |= value & FW_IND_EVENT_PENDING != 0;

        // ath10k never clears the CPU warm reset bit again: on QCA9377 it clears
        // itself, and writing the register back would be a read-modify-write of
        // `SOC_RESET_CONTROL` bits the driver does not own (this hardware
        // reports 0x33000000 there, none of which ath10k ever writes).
        if phase + 1 == WARM_RESET_PHASES {
            ready = phase_ready;
            break;
        }

        // `ath10k_pci_warm_reset_clear_lf` plus `ath10k_pci_warm_reset_ce`: the
        // CE reset pulse clears the ring registers, so the pipes below have to
        // be programmed again afterwards.
        pulse_ce_reset(registers).map_err(|error| failed("pulse CE reset", error))?;
    }

    target_wake(registers, poll_limit).map_err(|error| failed("final wake", error))?;

    // A target that never reported `FW_IND_INITIALIZED` leaves
    // `FW_IND_EVENT_PENDING` latched. ath10k releases it on the error path
    // (`ath10k_pci_fw_crashed_clear` in `ath10k_pci_hif_power_up`) so the next
    // power-up starts from a clean indicator; a read that fails here must not
    // replace the "not ready" result that the caller reports.
    let crash_cleared = !ready && clear_fw_crash(registers).is_ok();

    Ok(ResetOutcome {
        indicator_before: reset.indicator_before,
        indicator_after,
        ready,
        reset_control_asserted,
        cpu_warm_reset_visible,
        attempts,
        saw_crash,
        crash_cleared,
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

/// Host-interest access, the way ath10k does it.
///
/// `ath10k_bmi_read32`/`ath10k_bmi_write32` are `ath10k_bmi_read_soc_reg` and
/// `ath10k_bmi_write_soc_reg`, so every host-interest word travels in
/// `BMI_READ_SOC_REGISTER`/`BMI_WRITE_SOC_REGISTER`. Only blobs (board data,
/// firmware) use the memory commands.
fn read_host_interest<T: BmiTransport>(
    client: &mut BmiClient<T>,
    offset: u32,
) -> Result<u32, BmiBootError<T::Error>> {
    let address = HOST_INTEREST_ADDRESS
        .checked_add(offset)
        .ok_or(BmiBootError::AddressOverflow)?;
    client
        .read_soc_register(address)
        .map_err(BmiBootError::Transport)
}

fn write_host_interest<T: BmiTransport>(
    client: &mut BmiClient<T>,
    offset: u32,
    value: u32,
) -> Result<(), BmiBootError<T::Error>> {
    let address = HOST_INTEREST_ADDRESS
        .checked_add(offset)
        .ok_or(BmiBootError::AddressOverflow)?;
    client
        .write_soc_register(address, value)
        .map_err(BmiBootError::Transport)
}

/// Mirror `ath10k_init_configure_target`.
///
/// The target picks its HTC version, firmware mode and MAC address method from
/// these host-interest words. ath10k runs this after `ath10k_bmi_start` and
/// before any calibration or firmware download, so the values are already in
/// place when the firmware starts reading them.
pub fn configure_target<T: BmiTransport>(
    client: &mut BmiClient<T>,
    device_id: u16,
) -> Result<(), BmiBootError<T::Error>> {
    write_host_interest(
        client,
        HOST_INTEREST_APP_HOST_INTEREST_OFFSET,
        HTC_PROTOCOL_VERSION,
    )?;

    let mut option_flag = read_host_interest(client, HOST_INTEREST_OPTION_FLAG_OFFSET)?;
    option_flag |= 1 << HI_OPTION_NUM_DEV_SHIFT;
    option_flag |= HI_OPTION_FW_MODE_AP << HI_OPTION_FW_MODE_SHIFT;
    option_flag |= 1 << HI_OPTION_MAC_ADDR_METHOD_SHIFT;
    write_host_interest(client, HOST_INTEREST_OPTION_FLAG_OFFSET, option_flag)?;

    // The host does all byte swapping, so both swap switches stay cleared.
    write_host_interest(client, HOST_INTEREST_BE_OFFSET, 0)?;
    write_host_interest(client, HOST_INTEREST_FW_SWAP_OFFSET, 0)?;
    // Some targets sanity-check the PCI device id in this word before booting.
    write_host_interest(
        client,
        HOST_INTEREST_HCI_UART_PWR_MGMT_EXT_OFFSET,
        u32::from(device_id),
    )?;
    Ok(())
}

/// Mirror `ath10k_init_uart`: the target enables UART prints from a scratch
/// register, so the host turns them off explicitly after the firmware upload.
pub fn disable_uart_output<T: BmiTransport>(
    client: &mut BmiClient<T>,
) -> Result<(), BmiBootError<T::Error>> {
    write_host_interest(client, HOST_INTEREST_SERIAL_ENABLE_OFFSET, 0)
}

pub fn download_board_data<T: BmiTransport>(
    client: &mut BmiClient<T>,
    board_data: &[u8],
) -> Result<u32, BmiBootError<T::Error>> {
    if board_data.is_empty() {
        return Err(BmiBootError::InvalidBoardData);
    }

    let board_data_address = read_host_interest(client, HOST_INTEREST_BOARD_DATA_OFFSET)?;
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

    write_host_interest(client, HOST_INTEREST_BOARD_DATA_INITIALIZED_OFFSET, 1)?;
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

    configure_target(client, QCA9377_PCI_DEVICE_ID)?;
    download_board_data(client, board_data)?;
    if let Some(otp_image) = firmware.otp_image {
        let result = client
            .download_and_run_otp(QCA9377_OTP_LOAD_ADDRESS, 0, otp_image)
            .map_err(BmiBootError::Transport)?;
        if result != 0 {
            return Err(BmiBootError::OtpExecutionFailed(result));
        }
    }
    // `ath10k_core_start` order: `ath10k_download_fw` then
    // `ath10k_init_uart`, and only after that the BMI stream is closed.
    client
        .fast_download(QCA9377_FIRMWARE_LOAD_ADDRESS, firmware.firmware_image)
        .map_err(BmiBootError::Transport)?;
    disable_uart_output(client)?;
    client
        .set_app_start(QCA9377_FIRMWARE_LOAD_ADDRESS)
        .map_err(BmiBootError::Transport)?;
    client.done().map_err(BmiBootError::Transport)?;
    Ok(target_info)
}

/// Re-run the BMI handshake from the shell and print every answer.
///
/// The boot path gives up on a target that never reports
/// `FW_IND_INITIALIZED`, which leaves BMI untested. This runs the same bring-up
/// (wake, CE0/CE1 rings, bus mastering) and then asks the ROM for the
/// host-interest words the driver depends on, so a failure names the command
/// that stopped instead of ending at a single `TargetCrashed`.
///
/// # Safety
/// The caller must be sure no other agent drives the WLAN function while this
/// runs. Bus mastering is enabled for the exchange and disabled afterwards, and
/// `BMI_DONE` is never sent, so the boot ROM is left where the probe found it.
pub unsafe fn run_bmi_diagnostics() -> Result<(), &'static str> {
    let devices = pci::scan_pci_bus();
    let Some(device) = devices
        .iter()
        .find(|device| is_ath10k_pci_candidate(device.vendor_id, device.device_id))
    else {
        return Err("no Qualcomm Atheros WLAN function found on the PCI bus");
    };

    let bar = unsafe { pci::get_bar0(device) };
    if bar.address == 0 {
        return Err("BAR0 is not assigned");
    }
    let bar_size = unsafe { pci::get_bar0_size(device) };
    let required_size = required_register_window_size();
    if bar_size < required_size {
        return Err("BAR0 is too small for the target registers");
    }
    unsafe { pci::enable_mmio_only(device)? };

    let page_offset = bar.address & PAGE_OFFSET_MASK;
    let mapping_size = page_offset + required_size;
    let mmio = unsafe { memory::paging::map_mmio_range(bar.address, mapping_size) };
    let bar_virt = mmio + page_offset;
    let mut registers = unsafe { ce::MmioWindow::new(bar_virt as usize, required_size) }
        .map_err(|_| "MMIO window is unavailable")?;
    let mut rings = ce::BmiCeRings::allocate().map_err(|_| "BMI DMA ring allocation failed")?;

    target_wake(&mut registers, QCA_WAKE_POLL_LIMIT).map_err(|_| "target wake failed")?;
    let chip_id = registers.read32(CHIP_ID_BAR_OFFSET).unwrap_or(u32::MAX);
    let indicator = registers
        .read32(QCA6174_FW_INDICATOR_ADDRESS)
        .unwrap_or(u32::MAX);
    crate::println!(
        "wifi bmi: chip_id={chip_id:#010x} fw_indicator={indicator:#010x} event_pending={} initialized={}",
        indicator & FW_IND_EVENT_PENDING != 0,
        indicator & FW_IND_INITIALIZED != 0
    );

    unsafe { rings.configure(&mut registers) }.map_err(|_| "CE0/CE1 ring configuration failed")?;
    unsafe { pci::enable_bus_master(device)? };

    let mut outcome = Ok(());
    {
        let transport = unsafe { ce::BmiCeTransport::new(&mut rings, &mut registers) };
        let mut client = BmiClient::new(transport);

        match client.get_target_info() {
            Ok(info) => crate::println!(
                "wifi bmi: GET_TARGET_INFO version={:#010x} type={:#010x}",
                info.version, info.target_type
            ),
            Err(error) => {
                crate::println!("wifi bmi: GET_TARGET_INFO failed: {error:?}");
                outcome = Err("GET_TARGET_INFO failed, the boot ROM is not serving BMI");
            }
        }

        if outcome.is_ok() {
            for (name, offset) in [
                (
                    "hi_app_host_interest",
                    HOST_INTEREST_APP_HOST_INTEREST_OFFSET,
                ),
                ("hi_option_flag", HOST_INTEREST_OPTION_FLAG_OFFSET),
                ("hi_serial_enable", HOST_INTEREST_SERIAL_ENABLE_OFFSET),
                ("hi_board_data", HOST_INTEREST_BOARD_DATA_OFFSET),
                (
                    "hi_board_data_initialized",
                    HOST_INTEREST_BOARD_DATA_INITIALIZED_OFFSET,
                ),
                ("hi_fw_swap", HOST_INTEREST_FW_SWAP_OFFSET),
                (
                    "hi_uart_pwr_mgmt",
                    HOST_INTEREST_HCI_UART_PWR_MGMT_EXT_OFFSET,
                ),
            ] {
                match client.read_soc_register(HOST_INTEREST_ADDRESS + offset) {
                    Ok(value) => crate::println!("wifi bmi: {name} = {value:#010x}"),
                    Err(error) => {
                        crate::println!("wifi bmi: {name} read failed: {error:?}");
                        outcome = Err("a host-interest read failed");
                        break;
                    }
                }
            }
        }
    }

    if unsafe { pci::disable_bus_master(device) }.is_err() {
        crate::println!("wifi bmi: warning: bus mastering could not be disabled again");
    }
    let _ = target_sleep(&mut registers);
    outcome
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
    // Only `required_size` bytes are mapped past the page-aligned base, so the
    // window must refuse accesses beyond that even though BAR0 is larger.
    let mut registers = unsafe { ce::MmioWindow::new(bar_virt as usize, required_size) }
        .map_err(|_| "QCA9377 MMIO window is unavailable")?;
    let chip_id = read_chip_id_awake(&mut registers, QCA_WAKE_POLL_LIMIT)
        .map_err(|_| "QCA9377 target wake or register read failed")?;
    let rtc_state = registers
        .read32(RTC_STATE_BAR_OFFSET)
        .map_err(|_| "QCA9377 target wake or register read failed")?;
    Ok((rtc_state, chip_id))
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
    // Bounded by what was actually mapped, not by the whole BAR.
    let mut registers = unsafe { ce::MmioWindow::new(bar_virt as usize, required_size) }
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
                if !chip_id_is_valid(chip_id) {
                    // Linux ath10k reads this same register as 0x003821ff on the
                    // laptops that boot this device, so an all-zero window means
                    // the RTC_SOC block is not answering yet. The PCI id still
                    // matches, and the chip reset below brings the window up, so
                    // the identity is decided by the post-reset read instead of
                    // decoding "no answer" as hw1.0.
                    crate::println!(
                        "Qualcomm silicon identity: unknown before the chip reset (SOC_CHIP_ID@{CHIP_ID_BAR_OFFSET:#x}={chip_id:#010x}); the post-reset read decides"
                    );
                    true
                } else {
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
        HOST_INTEREST_ADDRESS, QCA6174_BAR_PROBE_8, QCA6174_FW_INDICATOR_ADDRESS,
        QCA6174_PCIE_LOCAL_PROBE_8, QCA6174_PCIE_SOC_WAKE_ADDRESS, ResetRegisterDump,
        QCA6174_RTC_SOC_BASE, QCA6174_RTC_SOC_TIMER_OFFSET, QCA6174_SOC_GLOBAL_RESET_ADDRESS,
        QCA9377_TARGET_VERSION_1_0, QCA9377_TARGET_VERSION_1_1, RTC_SOC_CE_RESET,
        RTC_SOC_CPU_RESET, RTC_STATE_BAR_OFFSET, SiliconIdentity, download_board_data,
        chip_id_is_valid, is_ath10k_pci_candidate, pci_board_name, release_target_cpu,
        required_register_window_size, reset_target_soc, sample_chip_id_timeline, silicon_identity,
        soc_global_reset, supports_qca9377_firmware, wait_for_target_init, with_target_awake,
        READ_FAILED_PATTERN,
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
    fn cold_reset_keeps_the_cpu_in_reset_until_the_rings_exist() {
        let mut registers = MockResetRegisters::default();
        let prepared = reset_target_soc(&mut registers, 8).unwrap();
        assert_eq!(prepared.indicator_before, 0);

        // Cold reset happened...
        let global_reset: alloc::vec::Vec<u32> = registers
            .writes
            .iter()
            .filter(|(offset, _)| *offset == QCA6174_SOC_GLOBAL_RESET_ADDRESS)
            .map(|(_, value)| *value)
            .collect();
        assert_eq!(global_reset, [1, 0]);
        assert!(registers.writes.iter().any(|(offset, value)| *offset
            == QCA6174_RTC_SOC_BASE + QCA6174_RTC_SOC_TIMER_OFFSET
            && *value & 4 == 0));

        // ...but the CPU warm reset was NOT asserted yet, so the ROM cannot be
        // running and BMI rings configured afterwards are never cleared.
        assert!(registers
            .writes
            .iter()
            .all(|(offset, value)| *offset != QCA6174_RTC_SOC_BASE
                || value & RTC_SOC_CPU_RESET == 0));
        assert_eq!(wait_for_target_init(&mut registers, 8), Ok((false, 0)));

        // Releasing the CPU after the rings exists boots the ROM.
        let outcome =
            release_target_cpu(&mut registers, 8, prepared, |_| Ok(())).unwrap();
        assert!(outcome.ready);
        assert_eq!(outcome.indicator_before, 0);
        assert_eq!(outcome.indicator_after, FW_IND_INITIALIZED);
        assert_eq!(prepared.reset_control_before & RTC_SOC_CPU_RESET, 0);
        assert_ne!(outcome.reset_control_asserted & RTC_SOC_CPU_RESET, 0);
        // Both warm-reset phases run, exactly like `ath10k_pci_warm_reset`.
        assert_eq!(outcome.attempts, 2);
        assert!(registers.awake);

        assert!(registers
            .writes
            .iter()
            .any(|(offset, value)| *offset == QCA6174_FW_INDICATOR_ADDRESS && *value == 0));
        assert_eq!(
            registers.writes.last(),
            Some(&(QCA6174_PCIE_SOC_WAKE_ADDRESS, 1))
        );
    }

    /// Target mock that latches `FW_IND_EVENT_PENDING` on the cold reset and
    /// only starts its boot ROM once the host clears that bit, which is what the
    /// laptop reports.
    #[derive(Default)]
    struct MockCrashLatch {
        awake: bool,
        latched: bool,
        released: bool,
        values: alloc::collections::BTreeMap<u64, u32>,
    }

    impl RegisterIo for MockCrashLatch {
        fn read32(&mut self, offset: u64) -> Result<u32, CeError> {
            if offset == RTC_STATE_BAR_OFFSET {
                return Ok(if self.awake { 0x73 } else { 0x71 });
            }
            if offset == QCA6174_FW_INDICATOR_ADDRESS {
                return Ok(if self.latched {
                    FW_IND_EVENT_PENDING
                } else if self.released {
                    FW_IND_INITIALIZED
                } else {
                    0
                });
            }
            Ok(self.values.get(&offset).copied().unwrap_or(0))
        }

        fn write32(&mut self, offset: u64, value: u32) -> Result<(), CeError> {
            self.values.insert(offset, value);
            match offset {
                QCA6174_PCIE_SOC_WAKE_ADDRESS => self.awake = value & 1 != 0,
                QCA6174_SOC_GLOBAL_RESET_ADDRESS if value & 1 == 0 => self.latched = true,
                QCA6174_FW_INDICATOR_ADDRESS if value & FW_IND_EVENT_PENDING == 0 => {
                    if self.latched {
                        self.latched = false;
                        self.released = true;
                    }
                }
                _ => {}
            }
            Ok(())
        }
    }

    #[test]
    fn a_latched_crash_is_cleared_and_the_rom_is_polled_again() {
        let mut registers = MockCrashLatch::default();
        let prepared = reset_target_soc(&mut registers, 8).unwrap();

        // The latch the firmware left behind is released and the second wait
        // sees a live boot ROM, so the reset is not reported as a crash.
        assert!(prepared.rom_started_after_clear);
        assert!(prepared.cold_ready);
        assert_eq!(prepared.cold_indicator, FW_IND_INITIALIZED);
    }

    #[test]
    fn ce_reset_pulse_precedes_the_cpu_release() {
        let mut registers = MockResetRegisters::default();
        let prepared = reset_target_soc(&mut registers, 8).unwrap();
        let _ = release_target_cpu(&mut registers, 8, prepared, |_| Ok(())).unwrap();

        let ce_write = registers
            .writes
            .iter()
            .position(|(offset, value)| {
                *offset == QCA6174_RTC_SOC_BASE && value & RTC_SOC_CE_RESET != 0
            })
            .expect("CE reset pulse missing");
        let cpu_write = registers
            .writes
            .iter()
            .position(|(offset, value)| {
                *offset == QCA6174_RTC_SOC_BASE && value & RTC_SOC_CPU_RESET != 0
            })
            .expect("CPU warm reset missing");
        assert!(ce_write < cpu_write);
    }

    #[test]
    fn cold_reset_dump_samples_both_register_windows() {
        let mut registers = MockResetRegisters::default();
        registers.values.insert(QCA6174_BAR_PROBE_8, 0xdead_beef);
        registers
            .values
            .insert(QCA6174_PCIE_LOCAL_PROBE_8, 0xfeed_face);
        let dump = ResetRegisterDump::capture(&mut registers);
        assert_eq!(dump.bar8, 0xdead_beef);
        assert_eq!(dump.local8, 0xfeed_face);
        assert_eq!(dump.local0, 0x71);
    }

    /// Chip-id window mock that serves prepared values in order, so the
    /// millisecond ladder after the cold reset can be observed without hardware.
    struct MockChipIdWindow {
        chip_ids: alloc::vec::Vec<u32>,
        next: usize,
        awake: bool,
    }

    impl RegisterIo for MockChipIdWindow {
        fn read32(&mut self, offset: u64) -> Result<u32, CeError> {
            match offset {
                CHIP_ID_BAR_OFFSET => {
                    let value = self.chip_ids.get(self.next).copied().unwrap_or(u32::MAX);
                    self.next += 1;
                    Ok(value)
                }
                RTC_STATE_BAR_OFFSET => Ok(if self.awake { 0x73 } else { 0x71 }),
                _ => Ok(0),
            }
        }

        fn write32(&mut self, offset: u64, value: u32) -> Result<(), CeError> {
            if offset == QCA6174_PCIE_SOC_WAKE_ADDRESS {
                self.awake = value & 1 != 0;
            }
            Ok(())
        }
    }

    #[test]
    fn only_a_real_chip_id_counts_as_an_identity() {
        // The pre-reset read answers zero on this platform and a dead window can
        // answer all-ones or the bus-error pattern; none of those may be decoded
        // as a silicon revision.
        assert!(!chip_id_is_valid(0));
        assert!(!chip_id_is_valid(u32::MAX));
        assert!(!chip_id_is_valid(READ_FAILED_PATTERN));
        assert!(chip_id_is_valid(0x0038_21ff));
    }

    #[test]
    fn chip_id_timeline_stops_at_the_first_real_value() {
        // Right after the reset the window still answers zero or a bus-error
        // pattern; the ladder has to keep going and report when it answers.
        let mut window = MockChipIdWindow {
            chip_ids: alloc::vec![0, READ_FAILED_PATTERN, 0x0038_21ff],
            next: 0,
            awake: false,
        };
        let timeline = sample_chip_id_timeline(&mut window).unwrap();
        assert_eq!(timeline.first_valid_ms, Some(20));
        assert_eq!(timeline.samples().len(), 3);
        assert_eq!(timeline.samples()[2].chip_id, 0x0038_21ff);
        // The cold reset drops the wake request, so every step re-drives it
        // before reading the SoC window.
        assert!(timeline.samples().iter().all(|sample| sample.awake));
    }

    #[test]
    fn chip_id_timeline_keeps_the_full_ladder_when_the_window_stays_silent() {
        let mut window = MockChipIdWindow {
            chip_ids: alloc::vec![],
            next: 0,
            awake: false,
        };
        let timeline = sample_chip_id_timeline(&mut window).unwrap();
        assert_eq!(timeline.first_valid_ms, None);
        assert_eq!(timeline.samples().len(), 6);
        assert_eq!(timeline.samples().last().unwrap().elapsed_ms, 3000);
    }

    #[test]
    fn target_init_wait_follows_ath10k_indicator_bits() {
        let mut registers = MockResetRegisters::default();
        // Nothing latched yet: the wait expires instead of claiming readiness.
        assert_eq!(wait_for_target_init(&mut registers, 8), Ok((false, 0)));

        registers
            .values
            .insert(QCA6174_FW_INDICATOR_ADDRESS, FW_IND_INITIALIZED);
        assert_eq!(
            wait_for_target_init(&mut registers, 8),
            Ok((true, FW_IND_INITIALIZED))
        );

        // `ath10k_pci_wait_for_target_init` breaks on `FW_IND_EVENT_PENDING`
        // first, so a crash latch outranks the initialized bit.
        registers.values.insert(
            QCA6174_FW_INDICATOR_ADDRESS,
            FW_IND_INITIALIZED | FW_IND_EVENT_PENDING,
        );
        assert_eq!(
            wait_for_target_init(&mut registers, 8),
            Ok((false, FW_IND_INITIALIZED | FW_IND_EVENT_PENDING))
        );
    }

    #[test]
    fn reset_failure_names_the_step_that_stopped() {
        let mut registers = MockResetRegisters::default();
        registers
            .values
            .insert(QCA6174_SOC_GLOBAL_RESET_ADDRESS, u32::MAX);
        let failure = reset_target_soc(&mut registers, 8).unwrap_err();
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
        // `ath10k_bmi_read32(hi_board_data)`: a SoC register read, not a memory
        // read.
        assert_eq!(read_word(&transport.requests[0], 0), 6);
        assert_eq!(
            read_word(&transport.requests[0], 4),
            HOST_INTEREST_ADDRESS + 0x54
        );
        // The payload itself is a memory write.
        assert_eq!(read_word(&transport.requests[1], 0), 3);
        assert_eq!(read_word(&transport.requests[1], 4), 0x2000);
        assert_eq!(read_word(&transport.requests[1], 8), 4);
        assert_eq!(&transport.requests[1][12..16], &[1, 2, 3, 4]);
        // `ath10k_bmi_write32(hi_board_data_initialized, 1)`.
        assert_eq!(read_word(&transport.requests[2], 0), 7);
        assert_eq!(
            read_word(&transport.requests[2], 4),
            HOST_INTEREST_ADDRESS + 0x58
        );
        assert_eq!(read_word(&transport.requests[2], 8), 1);
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
                // `hi_option_flag` read by `configure_target`.
                0_u32.to_le_bytes().into(),
                // `hi_board_data` address, now read as an SoC register.
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
        // 8 = target info, then `ath10k_init_configure_target` (7/6 writes and
        // the option-flag read), board data (soc-reg read, memory write,
        // soc-reg write), OTP, firmware upload, `ath10k_init_uart` (7), app
        // start and BMI done.
        assert_eq!(
            command_ids,
            [8, 7, 6, 7, 7, 7, 7, 6, 3, 7, 13, 14, 13, 4, 13, 14, 13, 7, 5, 1]
        );
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
