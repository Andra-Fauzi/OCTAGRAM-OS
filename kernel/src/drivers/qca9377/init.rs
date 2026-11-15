//! Full QCA9377 initialization sequence: chip reset → BMI → firmware →
//! HTC handshake → WMI init → vdev/peer create → scan → connect.
//!
//! This module is the top-level orchestrator.  Call `initialize_device` once
//! the PCI device has been identified as a QCA9377.

use super::{
    bmi::BmiClient,
    ce::{
        BmiCeRings, BmiCeTransport, CeError, DiagnosticCe, MmioWindow, PipePair,
        RegisterIo as _, RxCe, TxCe, CE0_REGISTER_BASE, CE1_REGISTER_BASE,
        CE_DESTINATION_READ_INDEX, CE_DESTINATION_RING_BASE, CE_DESTINATION_RING_SIZE,
        CE_DESTINATION_WRITE_INDEX, CE_PACKET_MAX_BYTES, CE_SOURCE_READ_INDEX,
        CE_SOURCE_RING_BASE, CE_SOURCE_RING_SIZE, CE_SOURCE_WRITE_INDEX, PIPE_EP0_RX_CE,
        clear_all_pipe_registers, interrupt as ce_interrupt,
        PIPE_EP0_TX_CE, PIPE_HTT_RX_CE, PIPE_HTT_TX_CE, PIPE_WMI_RX_CE, PIPE_WMI_TX_CE,
    },
    htc::{
        HTC_SERVICE_HTT_DATA, HTC_SERVICE_WMI_CONTROL, HtcClient, HtcClientError, HtcCredits,
        HtcError,
    },
    htt,
    pci::{self, PciDevice},
    target,
    wifi_firmware,
    wmi::{self, WmiCmdId, WmiEvent},
};
use crate::{memory, network};
use alloc::{boxed::Box, sync::Arc};

const QCA9377_SLOT_ID: u32 = 0x9377;
const VDEV_ID: u32 = 0;
const PEER_TYPE_STA: u32 = 1;
const WMI_VDEV_TYPE_STA: u32 = 2;
const HTT_RING_SIZE: u16 = 32;
const SCAN_ID: u32 = 0x1234;
// Default channel 6 (2437 MHz) — AP umumnya di channel 1/6/11
const DEFAULT_FREQ_MHZ: u32 = 2437;

// Convenience re-exports from parent.
use super::{
    boot_firmware, pci_board_name, release_target_cpu, reset_target_soc, silicon_identity,
    target_sleep, wake_target_cpu, SiliconIdentity, BmiBootError,
    CHIP_RESET_ATTEMPTS, QCA_WAKE_POLL_LIMIT, PAGE_OFFSET_MASK, read_chip_id_awake,
};

#[derive(Debug)]
pub enum InitError {
    NoPciBar,
    BarTooSmall,
    MmioSetup,
    DmaAlloc,
    SiliconUnknown,
    TargetReset,
    /// The target boot ROM never reached `FW_IND_INITIALIZED` (or reported
    /// `FW_IND_EVENT_PENDING`), so BMI cannot run on it.
    TargetCrashed,
    /// The copy engine registers could not be cleared before the chip reset.
    CeReset,
    TargetConfig,
    BmiBoot,
    HtcReady,
    HtcConnect,
    HtcSetupComplete,
    WmiReady,
    /// A WMI command or event failed after the handshake.
    Wmi,
    HttHandshake,
    VdevCreate,
    FirmwareUnavailable,
    BoardDataUnavailable,
    BusMaster,
}

impl From<CeError> for InitError {
    fn from(_: CeError) -> Self {
        InitError::DmaAlloc
    }
}

impl From<HtcError> for InitError {
    fn from(_: HtcError) -> Self {
        InitError::HtcReady
    }
}

impl From<HtcClientError<CeError>> for InitError {
    fn from(_: HtcClientError<CeError>) -> Self {
        InitError::HtcReady
    }
}

impl From<wmi::WmiError> for InitError {
    fn from(_: wmi::WmiError) -> Self {
        InitError::Wmi
    }
}

/// A live QCA9377 network link, implements `network::LinkDevice`.
pub struct Qca9377Link {
    slot: u32,
    mac: [u8; 6],
    // In a real interrupt-driven driver the TX path goes here.
    // For now we store the MAC and report it; TX is stubbed.
}

impl network::LinkDevice for Qca9377Link {
    fn slot_id(&self) -> u32 {
        self.slot
    }

    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn transmit(&self, _frame: &[u8]) -> Result<(), &'static str> {
        // TODO: post frame to HTT TX ring.
        Err("QCA9377: TX not yet implemented")
    }
}

/// Log the CE0/CE1 ring registers the boot ROM has to consume.
///
/// Captured right before the first BMI command: a zero source base means the
/// host programming never landed, and a source write index that never advances
/// means the ROM is not reading the ring at all (CPU still held in reset, or no
/// clock). The boot ROM programs the read indices, so the host cannot fake them.
fn log_bmi_ce_ring(registers: &mut MmioWindow, name: &str, base: u64) {
    let source_base = registers
        .read32(base + CE_SOURCE_RING_BASE)
        .unwrap_or(u32::MAX);
    let source_size = registers
        .read32(base + CE_SOURCE_RING_SIZE)
        .unwrap_or(u32::MAX);
    let destination_base = registers
        .read32(base + CE_DESTINATION_RING_BASE)
        .unwrap_or(u32::MAX);
    let destination_size = registers
        .read32(base + CE_DESTINATION_RING_SIZE)
        .unwrap_or(u32::MAX);
    let source_write = registers
        .read32(base + CE_SOURCE_WRITE_INDEX)
        .unwrap_or(u32::MAX);
    let source_read = registers
        .read32(base + CE_SOURCE_READ_INDEX)
        .unwrap_or(u32::MAX);
    let destination_write = registers
        .read32(base + CE_DESTINATION_WRITE_INDEX)
        .unwrap_or(u32::MAX);
    let destination_read = registers
        .read32(base + CE_DESTINATION_READ_INDEX)
        .unwrap_or(u32::MAX);
    crate::println!(
        "QCA9377 {} ring: src_base={:#010x} src_size={} src_wr={} src_rd={} dst_base={:#010x} dst_size={} dst_wr={} dst_rd={}",
        name,
        source_base,
        source_size,
        source_write,
        source_read,
        destination_base,
        destination_size,
        destination_write,
        destination_read
    );
}

/// Attempt to fully initialize the QCA9377 and register a network link.
/// Returns `Ok(mac)` on success.
///
/// # Safety
/// Must be called once after PCI enumeration, with interrupts enabled and
/// memory/DMA allocator ready.  The calling context must not hold any locks
/// that the allocator or PCI subsystem acquires.
pub unsafe fn initialize_device(
    device: &PciDevice,
    subsystem_vendor: u16,
    subsystem_device: u16,
) -> Result<[u8; 6], InitError> {
    // ── 1. Map BAR0 ──────────────────────────────────────────────────────────
    let bar = pci::get_bar0(device);
    if bar.address == 0 {
        return Err(InitError::NoPciBar);
    }
    let bar_size = pci::get_bar0_size(device);
    let min_size = super::required_register_window_size();
    if bar_size < min_size {
        return Err(InitError::BarTooSmall);
    }
    // Apollo Lake firmware on this laptop family can leave the WLAN in D3hot or
    // D3cold; config space still answers there, but the target ROM stays halted.
    match unsafe { pci::force_d0(device) } {
        Some((before, after)) => crate::println!(
            "QCA9377 power state: PM_CSR {:#06x} -> {:#06x} (D0 asserted)",
            before, after
        ),
        None => crate::println!("QCA9377 has no PCI power-management capability"),
    }

    pci::enable_mmio_only(device).map_err(|_| InitError::MmioSetup)?;

    // ASPM has to go before the target is exercised: an endpoint allowed to
    // enter L1 can drop copy-engine DMA, which looks like a transfer timeout.
    // ath10k does the same in `ath10k_pci_aspm_disable()`.
    match unsafe { pci::disable_aspm(device) } {
        Some((before, after)) => crate::println!(
            "QCA9377 PCIe link control: {:#06x} -> {:#06x} (ASPM L0s/L1 disabled)",
            before, after
        ),
        None => crate::println!("QCA9377 PCIe capability missing; ASPM left unchanged"),
    }
    // ASPM needs both ends of the link: the root port keeps requesting L1 even
    // when the endpoint is configured otherwise. The QCA9377 is known to raise
    // PCIe AER errors on this platform family with ASPM enabled.
    match pci::find_parent_bridge(device.bus) {
        Some(bridge) => match unsafe { pci::disable_aspm(&bridge) } {
            Some((before, after)) => crate::println!(
                "QCA9377 root port {:02x}:{:02x}.{} link control: {:#06x} -> {:#06x} (ASPM off)",
                bridge.bus, bridge.device, bridge.function, before, after
            ),
            None => crate::println!("QCA9377 root port is not PCIe-capable; ASPM unchanged"),
        },
        None => crate::println!("QCA9377 parent bridge not found; root-port ASPM unchanged"),
    }

    let page_off = bar.address & PAGE_OFFSET_MASK;
    let mapping_size = page_off + bar_size;
    let mmio_virt = memory::paging::map_mmio_range(bar.address, mapping_size);
    let bar_virt = mmio_virt + page_off;
    let mut registers = unsafe {
        MmioWindow::new(bar_virt as usize, bar_size).map_err(|_| InitError::MmioSetup)?
    };

    // ── 2. Verify silicon ────────────────────────────────────────────────────
    // The target is woken for the identity read and deliberately left awake:
    // ath10k's `ath10k_pci_force_wake()` is a no-op for QCA9377, so Linux never
    // puts the SoC back to sleep between probe and `ath10k_pci_chip_reset`.
    let chip_id = read_chip_id_awake(&mut registers, QCA_WAKE_POLL_LIMIT)
        .map_err(|_| InitError::SiliconUnknown)?;

    match silicon_identity(chip_id) {
        SiliconIdentity::Qca9377Hw10 | SiliconIdentity::Qca9377Hw11 => {}
        _ => return Err(InitError::SiliconUnknown),
    }

    // ── 3. Load firmware images ───────────────────────────────────────────────
    let firmware =
        wifi_firmware::qca9377().map_err(|_| InitError::FirmwareUnavailable)?;
    let board_name =
        pci_board_name(device, subsystem_vendor, subsystem_device);
    let board = wifi_firmware::select_board_data(firmware.board_bundle, board_name.as_bytes())
        .map_err(|_| InitError::BoardDataUnavailable)?;

    // ── 3a. Clean copy engines, then allow target DMA ───────────────────────
    // Linux enables bus mastering in `ath10k_pci_claim` (`pci_set_master`) and
    // zeroes every CE pipe (`ath10k_pci_ce_deinit` + `ath10k_ce_disable_interrupts`)
    // before `ath10k_pci_chip_reset`. Starting the boot ROM against a stale CE
    // configuration left by the firmware that ran before the driver is the most
    // likely reason this device set `FW_IND_EVENT_PENDING` here while the same
    // laptop boots ath10k fine. Clear the registers while bus mastering is still
    // off, then set the master bit the way `pci_set_master` does.
    if unsafe { clear_all_pipe_registers(&mut registers) }.is_err() {
        let _ = target_sleep(&mut registers);
        return Err(InitError::CeReset);
    }
    if pci::enable_bus_master(device).is_err() {
        let _ = target_sleep(&mut registers);
        return Err(InitError::BusMaster);
    }
    crate::println!(
        "QCA9377 copy engines cleared and bus mastering enabled before the chip reset"
    );

    // ── 3b/4b. Chip reset: cold reset, CE rings, warm reset ─────────────────
    // The boot ROM only services BMI after a cold reset, but the target CPU is
    // NOT released yet: ath10k releases it only once the CE pipes exist
    // (`ath10k_pci_warm_reset_cpu` -> `ath10k_pci_init_pipes` ->
    // `ath10k_pci_wait_for_target_init`). Releasing it here left FW_INDICATOR at
    // 0 and every BMI exchange timed out with SourceTimeout.
    //
    // The CE reset pulse also belongs to the cold-reset stage: it clears the CE
    // ring registers, so the BMI rings and the CE7 diagnostic pipe are
    // programmed again on every attempt.
    //
    // ath10k retries the whole chip reset `ATH10K_PCI_NUM_WARM_RESET_ATTEMPTS`
    // times (`ath10k_pci_qca988x_chip_reset`) because a target that did not come
    // up on one cold reset can still come up on the next one. Only a running
    // boot ROM can service BMI, so the bring-up retries the reset before it
    // reports `TargetCrashed`.
    let mut bmi_rings = BmiCeRings::allocate().map_err(|_| InitError::DmaAlloc)?;
    let mut target_config_ce = DiagnosticCe::allocate().map_err(|_| InitError::DmaAlloc)?;
    let mut keep_rings = false;

    let mut attempt = 0;
    let (rom_ready, rom_crashed) = loop {
        attempt += 1;
        crate::println!("QCA9377 chip reset attempt {attempt}/{CHIP_RESET_ATTEMPTS}");

        let target_reset = match reset_target_soc(&mut registers, QCA_WAKE_POLL_LIMIT) {
            Ok(state) => state,
            Err(failure) => {
                crate::println!(
                    "QCA9377 reset failed at \"{}\": {:?}",
                    failure.step, failure.error
                );
                return Err(InitError::TargetReset);
            }
        };
        crate::println!(
            "QCA9377 regs before cold reset: bar0={:#010x} bar8={:#010x} local0={:#010x} local8={:#010x}",
            target_reset.dump_before.bar0,
            target_reset.dump_before.bar8,
            target_reset.dump_before.local0,
            target_reset.dump_before.local8
        );
        crate::println!(
            "QCA9377 regs after cold reset:  bar0={:#010x} bar8={:#010x} local0={:#010x} local8={:#010x}",
            target_reset.dump_after.bar0,
            target_reset.dump_after.bar8,
            target_reset.dump_after.local0,
            target_reset.dump_after.local8
        );
        crate::println!(
            "QCA9377 cold reset: rtc_state_in_reset={:#010x} fw_indicator={:#010x} boot_rom_ready={}",
            target_reset.cold_reset_state,
            target_reset.cold_indicator,
            target_reset.cold_ready
        );

        // Bus mastering is still enabled from the previous attempt, but the
        // global reset is re-driven every time, so confirm the bit instead of
        // assuming PCIe configuration space kept it.
        if pci::enable_bus_master(device).is_err() {
            let _ = target_sleep(&mut registers);
            return Err(InitError::BusMaster);
        }
        keep_rings = true;

        // ── 4b. Warm reset, with the CE pipes programmed between the waits ────
        // `ath10k_pci_warm_reset` resets the target CPU, runs
        // `ath10k_pci_init_pipes`, waits for the boot ROM, then clears the
        // low-frequency timer, pulses the CE reset, resets the CPU again,
        // re-programs the pipes and waits a second time. The pipes are passed in
        // as a callback so they are programmed in exactly those two places and
        // the ROM never runs against a CE layout that has already been
        // invalidated.
        let ready_and_crash = match release_target_cpu(
            &mut registers,
            QCA_WAKE_POLL_LIMIT,
            target_reset,
            |registers| {
                unsafe { bmi_rings.configure(registers)? };
                unsafe { target_config_ce.configure(registers)? };
                Ok(())
            },
        ) {
            Ok(outcome) => {
                // Reading the chip id again separates "the reset ran but the
                // identity read was early" from "the RTC_SOC window never
                // answered in the first place".
                let chip_id_after = registers
                    .read32(super::CHIP_ID_BAR_OFFSET)
                    .unwrap_or(u32::MAX);
                crate::println!(
                    "QCA9377 reset: FW_INDICATOR {:#010x} -> {:#010x} boot_rom_ready={} warm_reset_phases={} crash_event={} crash_latch_cleared={} soc_reset_control={:#010x} -> {:#010x} (assert) -> {:#010x} (release) cpu_warm_rst_visible={} chip_id_after_reset={:#010x}",
                    outcome.indicator_before,
                    outcome.indicator_after,
                    outcome.ready,
                    outcome.attempts,
                    outcome.saw_crash,
                    outcome.crash_cleared,
                    target_reset.reset_control_before,
                    outcome.reset_control_asserted,
                    outcome.reset_control_released,
                    outcome.cpu_warm_reset_visible,
                    chip_id_after
                );
                (outcome.ready, outcome.saw_crash)
            }
            Err(failure) => {
                crate::println!(
                    "QCA9377 reset failed at \"{}\": {:?}",
                    failure.step, failure.error
                );
                return Err(InitError::TargetReset);
            }
        };

        if ready_and_crash.0 || attempt >= CHIP_RESET_ATTEMPTS {
            break ready_and_crash;
        }
        crate::println!(
            "QCA9377 boot ROM is not running after attempt {attempt}/{CHIP_RESET_ATTEMPTS}; retrying the full chip reset"
        );
    };
    // ath10k gives up here as well: `ath10k_pci_wait_for_target_init` returns
    // -ECOMM for `FW_IND_EVENT_PENDING` ("device has crashed during init") and
    // -ETIMEDOUT when the bit is never set, and `ath10k_pci_hif_power_up` then
    // fails the whole power-up. Everything below — the CE7 target tables and
    // every BMI command — needs a running boot ROM, so continuing only turned
    // this into a misleading `MissingPcieState` from an unpopulated
    // HOST_INTEREST.
    if !rom_ready {
        if rom_crashed {
            crate::println!(
                "QCA9377: device has crashed during init (target set FW_IND_EVENT_PENDING without FW_IND_INITIALIZED)"
            );
        } else {
            crate::println!(
                "QCA9377 boot ROM never reported FW_IND_INITIALIZED after the cold and warm reset"
            );
        }
        crate::println!("QCA9377: boot ROM is not running; skipping target CE config and BMI");
        let _ = pci::disable_bus_master(device);
        let _ = target_sleep(&mut registers);
        return Err(InitError::TargetCrashed);
    }

    // Linux reads the chip id only after a successful chip reset
    // (`ath10k_pci_chip_reset` -> `ath10k_pci_soc_read32(SOC_CHIP_ID_ADDRESS)`),
    // and that is the first point where `SOC_CHIP_ID` holds the real value: the
    // pre-reset read on the E203NAH returns 0. Log the identity ath10k reports
    // (`qca9377 hw1.1 chip_id ...`) so the boot log can be compared directly.
    match read_chip_id_awake(&mut registers, QCA_WAKE_POLL_LIMIT) {
        Ok(chip_id) => crate::println!(
            "QCA9377 target identity after chip reset: chip_id={chip_id:#010x} silicon={:?}",
            silicon_identity(chip_id)
        ),
        Err(error) => {
            crate::println!("QCA9377 chip id read after the chip reset failed: {error:?}")
        }
    }

    // ── 4c. Target-side CE layout and HTC service map ────────────────────────
    // Tells the target which CE carries HTC control, WMI and HTT. Linux writes
    // these tables only after the chip reset completed, because the tables are
    // consumed by the boot path, not by the ROM's own start-up.
    {
        let mut memory =
            unsafe { target::Ce7TargetMemory::new(&mut target_config_ce, &mut registers) };
        match target::configure_pipes(&mut memory) {
            Ok(report) => crate::println!(
                "QCA9377 target CE config: pcie_state={:#010x} pipe_cfg={:#010x} svc_map={:#010x} banks={} flags={:#x} option_flag2={:#x}->{:#x}",
                report.pcie_state,
                report.pipe_config_address,
                report.service_map_address,
                target::QCA9377_IRAM_BANKS,
                report.config_flags,
                report.option_flag2,
                report.option_flag2_after
            ),
            Err(error) => {
                crate::println!("QCA9377 target CE config failed: {error:?}");
                let _ = pci::disable_bus_master(device);
                let _ = target_sleep(&mut registers);
                return Err(InitError::TargetConfig);
            }
        }
    }
    // CE7 is host-only (its target pipe entry has zero entries), so its ring
    // registers can be cleared once the tables are written.
    if unsafe { target_config_ce.deconfigure(&mut registers) }.is_err() {
        core::mem::forget(target_config_ce);
    }

    // ── 4d. Wake the target CPU (trigger CPU interrupt) before BMI ──────────
    let _ = wake_target_cpu(&mut registers);

    // What the ROM is supposed to consume, captured before the first command.
    log_bmi_ce_ring(&mut registers, "CE0", CE0_REGISTER_BASE);
    log_bmi_ce_ring(&mut registers, "CE1", CE1_REGISTER_BASE);

    let mac_addr = {
        let transport = unsafe { BmiCeTransport::new(&mut bmi_rings, &mut registers) };
        let mut client = BmiClient::new(transport);

        let result = unsafe {
            boot_firmware(
                &mut client,
                wifi_firmware::FirmwareImages {
                    version: firmware.firmware_version,
                    otp_image: firmware.otp_image,
                    firmware_image: firmware.firmware_image,
                },
                board.board,
            )
        };
        match result {
            Ok(target_info) => {
                crate::println!(
                    "QCA9377 target info: version={:#x} type={:#x}",
                    target_info.version,
                    target_info.target_type
                );
            }
            Err(e) => {
                crate::println!("QCA9377 boot_firmware error: {:?}", e);
                let _ = pci::disable_bus_master(device);
                let _ = target_sleep(&mut registers);
                return Err(InitError::BmiBoot);
            }
        }

        // BMI DONE: firmware is now starting.
        // We need to give firmware ~100 ms to boot before HTC.
        for _ in 0..10_000_000u64 {
            core::hint::spin_loop();
        }

        // ── 5. Configure HTC CE pipe pairs ────────────────────────────────────
        let mut ep0_tx = TxCe::allocate(PIPE_EP0_TX_CE, CE_PACKET_MAX_BYTES)?;
        let mut ep0_rx = RxCe::allocate(PIPE_EP0_RX_CE, CE_PACKET_MAX_BYTES)?;
        let mut wmi_tx = TxCe::allocate(PIPE_WMI_TX_CE, CE_PACKET_MAX_BYTES)?;
        let mut wmi_rx = RxCe::allocate(PIPE_WMI_RX_CE, CE_PACKET_MAX_BYTES)?;
        let mut htt_tx = TxCe::allocate(PIPE_HTT_TX_CE, CE_PACKET_MAX_BYTES)?;
        let mut htt_rx = RxCe::allocate(PIPE_HTT_RX_CE, CE_PACKET_MAX_BYTES)?;

        unsafe {
            ep0_tx.configure(&mut registers)?;
            ep0_rx.configure(&mut registers)?;
            wmi_tx.configure(&mut registers)?;
            wmi_rx.configure(&mut registers)?;
            htt_tx.configure(&mut registers)?;
            htt_rx.configure(&mut registers)?;
        }

        // ── 5b. PCIe interrupts ──────────────────────────────────────────────
        // ath10k runs the target on MSI (`pci irq msi oper_irq_mode 2`) and only
        // then enables the chip's interrupt output, so every completion below is
        // signalled by an IRQ instead of a register poll. `ath10k_pci_enable_intx_irq`
        // enables both firmware and copy-engine causes; the per-engine copy
        // complete bit is what unmasks one pipe.
        let msi = super::QCA9377_MSI_VECTOR;
        let msi_enabled = unsafe { pci::enable_msi(device, msi, 0) };
        if msi_enabled {
            unsafe {
                ce_interrupt::register_window(&registers);
                for ce_index in [
                    PIPE_EP0_TX_CE,
                    PIPE_EP0_RX_CE,
                    PIPE_WMI_TX_CE,
                    PIPE_WMI_RX_CE,
                    PIPE_HTT_TX_CE,
                    PIPE_HTT_RX_CE,
                ] {
                    ce_interrupt::enable(&mut registers, ce_index)?;
                }
                ce_interrupt::arm(&mut registers)?;
            }
            crate::println!(
                "QCA9377 PCIe interrupts armed: MSI vector {}, CE copy-complete on EP0/WMI/HTT pipes",
                msi
            );
        } else {
            crate::println!("QCA9377 has no MSI; copy engines stay in register polling mode");
        }

        // ── 6. HTC handshake (EP0 pair) ───────────────────────────────────────
        let ep0_pair = unsafe { PipePair::new(&mut ep0_tx, &mut ep0_rx, &mut registers) };
        let mut htc = HtcClient::new(ep0_pair);
        let mut htc_frame = [0u8; 512];

        let ready = htc
            .wait_ready(&mut htc_frame)?;
        crate::println!(
            "QCA9377 HTC ready: credit_count={} credit_size={} max_ep={}",
            ready.credit_count,
            ready.credit_size,
            ready.max_endpoints
        );

        // Connect WMI control service.
        let wmi_resp = htc
            .connect_service(HTC_SERVICE_WMI_CONTROL, 0, &mut htc_frame)?;
        let wmi_ep = wmi_resp.endpoint;
        crate::println!("QCA9377 WMI connected: ep={}", wmi_ep);

        // Connect HTT data service.
        let htt_resp = htc
            .connect_service(HTC_SERVICE_HTT_DATA, 0, &mut htc_frame)?;
        let htt_ep = htt_resp.endpoint;
        crate::println!("QCA9377 HTT connected: ep={}", htt_ep);

        // Send SETUP_COMPLETE.
        htc.send_setup_complete(&mut htc_frame)?;
        crate::println!("QCA9377 HTC setup complete sent");

        // ── 7. WMI init ───────────────────────────────────────────────────────
        // The handshake borrowed the EP0 pipes. WMI is a different HTC endpoint
        // carried by its own copy engine (TX=CE3, RX=CE2), so the control client
        // is released here and WMI is addressed by the endpoint id the target
        // returned for the WMI service.
        drop(htc);
        let mut wmi_ep = wmi::WmiEndpoint::new(
            wmi_resp.endpoint,
            wmi_resp.max_message_size,
            HtcCredits::new(ready.credit_size, ready.credit_count),
        );
        let mut wmi_frame = [0u8; 512];
        crate::println!(
            "QCA9377 WMI endpoint {} (max {} bytes), framed as HTC data messages",
            wmi_ep.endpoint(),
            wmi_resp.max_message_size
        );

        unsafe {
            wmi_ep.send(
                &mut wmi_tx,
                &mut registers,
                WmiCmdId::Init,
                &wmi::wmi_init_payload(),
                &mut wmi_frame,
            )?;
        }
        crate::println!("QCA9377 WMI_INIT sent");

        // Wait for WMI READY, which carries the firmware MAC address. Other
        // events may arrive first, so poll a bounded number of times.
        let mac = {
            let mut firmware_mac = None;
            for _ in 0..64 {
                match unsafe { wmi_ep.receive(&mut wmi_rx, &mut registers, &mut wmi_frame) } {
                    Ok(WmiEvent::Ready(ready)) => {
                        crate::println!(
                            "QCA9377 WMI READY: sw_version={:#x} abi_version={} status={} MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                            ready.sw_version,
                            ready.abi_version,
                            ready.status,
                            ready.mac_addr[0], ready.mac_addr[1], ready.mac_addr[2],
                            ready.mac_addr[3], ready.mac_addr[4], ready.mac_addr[5]
                        );
                        firmware_mac = Some(ready.mac_addr);
                        break;
                    }
                    Ok(other) => {
                        crate::println!("QCA9377 WMI event before READY: {other:?}");
                    }
                    Err(error) => {
                        crate::println!("QCA9377 WMI receive failed before READY: {error:?}");
                        break;
                    }
                }
            }
            match firmware_mac {
                Some(mac) => mac,
                None => {
                    // Without WMI READY the firmware is not up, so vdev and peer
                    // commands would only fail later with less context.
                    crate::println!("QCA9377 firmware never reported WMI READY");
                    let _ = pci::disable_bus_master(device);
                    let _ = target_sleep(&mut registers);
                    return Err(InitError::WmiReady);
                }
            }
        };

        // ── 8. HTT version handshake ──────────────────────────────────────────
        // HTT is an HTC service as well: the request goes out as an HTC data
        // message for the HTT endpoint on CE4, and the version confirmation
        // comes back on CE5. ath10k sends it from `ath10k_htt_h2t_ver_req_msg`.
        let mut htt_ep = htt::HttEndpoint::new(
            htt_resp.endpoint,
            htt_resp.max_message_size,
            HtcCredits::new(ready.credit_size, ready.credit_count),
        );
        // Large enough for an RX indication carrying many MSDU descriptors.
        let mut htt_frame = [0u8; 512];
        crate::println!(
            "QCA9377 HTT endpoint {} (max {} bytes), framed as HTC data messages",
            htt_ep.endpoint(),
            htt_resp.max_message_size
        );
        match unsafe {
            htt_ep.version_handshake(&mut htt_tx, &mut htt_rx, &mut registers, &mut htt_frame)
        } {
            Ok(version) => crate::println!(
                "QCA9377 HTT version: {}.{}",
                version.major,
                version.minor
            ),
            Err(error) => {
                crate::println!("QCA9377 HTT version handshake failed: {error:?}");
            }
        }

        // ── 8b. HTT RX ring ───────────────────────────────────────────────────
        // ath10k_htt_send_rx_ring_cfg: tell the firmware which buffer ring it may
        // DMA received MSDUs into. The firmware keeps writing into those buffers
        // for the whole device lifetime, so the ring is leaked rather than dropped
        // by a later error path.
        let mut htt_rx_ring = match htt::HttRxRing::allocate(htt::HTT_RX_RING_SIZE) {
            Ok(ring) => {
                let ring: &'static mut htt::HttRxRing = Box::leak(Box::new(ring));
                let mut config_frame = [0u8; 128];
                match unsafe {
                    ring.configure(&mut htt_ep, &mut htt_tx, &mut registers, &mut config_frame)
                } {
                    Ok(()) => crate::println!(
                        "QCA9377 HTT RX ring: {} buffers of {} bytes",
                        ring.size(),
                        htt::HTT_RX_BUF_SIZE
                    ),
                    Err(error) => {
                        crate::println!("QCA9377 HTT RX ring config failed: {error:?}");
                    }
                }
                Some(ring)
            }
            Err(error) => {
                crate::println!("QCA9377 HTT RX ring allocation failed: {error:?}");
                None
            }
        };

        // ── 9. Create STA vdev ────────────────────────────────────────────────
        {
            let vc = wmi::wmi_vdev_create(VDEV_ID, mac);
            unsafe {
                wmi_ep.send(&mut wmi_tx, &mut registers, WmiCmdId::VdevCreate, &vc, &mut wmi_frame)?
            };
            crate::println!("QCA9377 WMI_VDEV_CREATE sent (vdev_id=0, STA)");
        }

        // ── 10. Create peer (self) ─────────────────────────────────────────────
        {
            let pc = wmi::wmi_peer_create(VDEV_ID, mac, PEER_TYPE_STA);
            unsafe {
                wmi_ep.send(&mut wmi_tx, &mut registers, WmiCmdId::PeerCreate, &pc, &mut wmi_frame)?
            };
            crate::println!("QCA9377 WMI_PEER_CREATE sent");
        }

        // ── 11. Disable powersave ─────────────────────────────────────────────
        {
            let ps = wmi::wmi_sta_powersave_mode(VDEV_ID, 0);
            unsafe {
                wmi_ep.send(
                    &mut wmi_tx,
                    &mut registers,
                    WmiCmdId::StaPowersaveMode,
                    &ps,
                    &mut wmi_frame,
                )?
            };
        }

        // ── 12. VDEV_START (bring up radio on channel) ────────────────────────
        // We start the vdev first; firmware will respond with VDEV_START_RESP.
        // SSID is empty here — STA mode doesn't need SSID for START (only UP).
        {
            let vs = wmi::wmi_vdev_start(VDEV_ID, DEFAULT_FREQ_MHZ, &[], [0u8; 6], 0);
            unsafe {
                wmi_ep.send(&mut wmi_tx, &mut registers, WmiCmdId::VdevStart, &vs, &mut wmi_frame)?
            };
            crate::println!("QCA9377 WMI_VDEV_START sent (freq={}MHz)", DEFAULT_FREQ_MHZ);
        }

        // Wait for VDEV_START_RESP.
        {
            match unsafe { wmi_ep.receive(&mut wmi_rx, &mut registers, &mut wmi_frame) } {
                Ok(WmiEvent::VdevStartResp { vdev_id, status }) => {
                    crate::println!(
                        "QCA9377 VDEV_START_RESP: vdev={} status={}",
                        vdev_id, status
                    );
                    if status != 0 {
                        crate::println!("QCA9377 VDEV_START failed, status={}", status);
                        return Err(InitError::VdevCreate);
                    }
                }
                Ok(e) => {
                    crate::println!("QCA9377 unexpected event (expected VDEV_START_RESP): {:?}", e);
                }
                Err(error) => {
                    crate::println!("QCA9377 VDEV_START_RESP receive failed: {error:?}");
                }
            }
        }

        // ── 13. Active scan ───────────────────────────────────────────────────
        // Scan on the default channel to discover APs.
        {
            let sc = wmi::wmi_start_scan(VDEV_ID, SCAN_ID, DEFAULT_FREQ_MHZ);
            unsafe {
                wmi_ep.send(&mut wmi_tx, &mut registers, WmiCmdId::StartScan, &sc, &mut wmi_frame)?
            };
            crate::println!("QCA9377 WMI_START_SCAN sent (freq={}MHz)", DEFAULT_FREQ_MHZ);
        }

        // Drain scan events until SCAN_COMPLETED.
        // Event type 0x20 = WMI_SCAN_EVENT_COMPLETED in ath10k.
        let bssid_found = drain_scan_events(&mut wmi_ep, &mut wmi_rx, &mut registers);
        if let Some(bssid) = bssid_found {
            crate::println!(
                "QCA9377 scan found AP: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                bssid[0], bssid[1], bssid[2], bssid[3], bssid[4], bssid[5]
            );
        } else {
            crate::println!("QCA9377 scan complete (no AP BSSID reported in event)");
        }

        // ── 14. VDEV_UP — bring STA up ────────────────────────────────────────
        // assoc_id=1 is a placeholder; real assoc_id comes from 802.11 assoc
        // response. At this layer (no full MLME) we set it to 0 — firmware
        // will fill it in when the association succeeds.
        let ap_bssid = bssid_found.unwrap_or([0xffu8; 6]); // broadcast if no AP yet
        {
            let vu = wmi::wmi_vdev_up(VDEV_ID, 0, ap_bssid);
            unsafe {
                wmi_ep.send(&mut wmi_tx, &mut registers, WmiCmdId::VdevUp, &vu, &mut wmi_frame)?
            };
            crate::println!("QCA9377 WMI_VDEV_UP sent");
        }

        // ── 15. Create peer for AP BSSID ─────────────────────────────────────
        if ap_bssid != [0xffu8; 6] {
            let pc = wmi::wmi_peer_create(VDEV_ID, ap_bssid, 2); // 2 = PEER_TYPE_BSS
            unsafe {
                wmi_ep.send(&mut wmi_tx, &mut registers, WmiCmdId::PeerCreate, &pc, &mut wmi_frame)?
            };
            crate::println!("QCA9377 WMI_PEER_CREATE (AP) sent");

            // ── 16. PEER_ASSOC — tell firmware AP capabilities ────────────────
            // Basic 11g rates: 6,9,12,18,24,36,48,54 Mbps encoded as 802.11 rates
            let basic_rates: [u8; 8] = [0x8c, 0x92, 0x98, 0xa4, 0xb0, 0xc8, 0xe0, 0xec];
            let pa = wmi::wmi_peer_assoc(VDEV_ID, ap_bssid, 1, 0x0431, 10, &basic_rates);
            unsafe {
                wmi_ep.send(&mut wmi_tx, &mut registers, WmiCmdId::PeerAssoc, &pa, &mut wmi_frame)?
            };
            crate::println!("QCA9377 WMI_PEER_ASSOC sent");
        }

        // Poll for WMI_CONNECTED event (non-fatal if we don't get one here —
        // the event may arrive asynchronously after the OS starts the event loop).
        {
            // Try once — if no event in time, continue. Real driver uses IRQ.
            match unsafe { wmi_ep.receive(&mut wmi_rx, &mut registers, &mut wmi_frame) } {
                Ok(WmiEvent::Connected(c)) => {
                    crate::println!(
                        "QCA9377 CONNECTED: vdev={} bssid={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} ch={}",
                        c.vdev_id,
                        c.bssid[0], c.bssid[1], c.bssid[2],
                        c.bssid[3], c.bssid[4], c.bssid[5],
                        c.channel
                    );
                }
                Ok(other) => {
                    crate::println!("QCA9377 event after assoc: {:?}", other);
                }
                Err(error) => {
                    crate::println!("QCA9377 no event after assoc: {error:?}");
                }
            }
        }

        // ── 17. Drain received frames ─────────────────────────────────────────
        // HTT reports the MSDUs it placed in the RX ring as in-order paddr
        // indications. Offloaded indications already carry a complete Ethernet
        // frame, which is exactly what the kernel network stack consumes.
        if let Some(ring) = htt_rx_ring.as_deref_mut() {
            let mut delivered = 0usize;
            let mut dropped = 0usize;
            for _ in 0..32 {
                let message =
                    match unsafe { htt_ep.receive(&mut htt_rx, &mut registers, &mut htt_frame) } {
                        Ok(message) => message,
                        // No indication pending (or the target stopped answering).
                        Err(_) => break,
                    };
                match unsafe { process_htt_rx(ring, message) } {
                    RxOutcome::Delivered => delivered += 1,
                    RxOutcome::Dropped => dropped += 1,
                    RxOutcome::Skipped => {}
                }
            }
            crate::println!(
                "QCA9377 HTT RX: {} frame(s) delivered, {} dropped",
                delivered,
                dropped
            );
        }

        crate::println!(
            "QCA9377 HTC credits left: WMI {} of {} bytes, HTT {} of {} bytes",
            wmi_ep.credits().available(),
            wmi_ep.credits().credit_size(),
            htt_ep.credits().available(),
            htt_ep.credits().credit_size()
        );
        crate::println!(
            "QCA9377 PCIe interrupts serviced: {} (fallback ring samples: {})",
            ce_interrupt::interrupts(),
            ce_interrupt::fallback_reads()
        );

        keep_rings = false;
        mac
    };

    if keep_rings {
        // DMA still potentially in flight — leak rings to prevent UAF.
        core::mem::forget(bmi_rings);
    }

    crate::println!(
        "QCA9377 initialization complete, registering network link"
    );

    let link = Arc::new(Qca9377Link {
        slot: QCA9377_SLOT_ID,
        mac: mac_addr,
    });
    if let Err(e) = network::register_link(link) {
        crate::println!("QCA9377 register_link failed: {e}");
    }

    Ok(mac_addr)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

enum RxOutcome {
    /// At least one Ethernet frame reached the network stack.
    Delivered,
    /// An RX indication arrived but nothing usable could be delivered.
    Dropped,
    /// The message was not an RX indication.
    Skipped,
}

/// Turn one HTT T2H message into frames for the network stack.
///
/// # Safety
/// `ring` must describe buffers that are currently posted to the firmware.
unsafe fn process_htt_rx(ring: &mut htt::HttRxRing, message: &[u8]) -> RxOutcome {
    let indication = match htt::parse_rx_indication(message) {
        Ok(indication) => indication,
        // TX completions, credit updates and other T2H traffic pass through.
        Err(_) => return RxOutcome::Skipped,
    };
    if !indication.offload {
        // Non-offloaded indications point at a v1 rx descriptor followed by the
        // 802.11 frame, which mac80211 would decapsulate. This driver has no
        // mac80211, so it converts the frame itself before handing it to the
        // network stack.
        let mut buffer = alloc::vec![0u8; htt::HTT_RX_BUF_SIZE];
        let mut ethernet = alloc::vec![0u8; htt::HTT_RX_BUF_SIZE];
        let mut delivered = false;
        for msdu in &indication.msdus {
            let Some(index) = ring.index_of(msdu.paddr) else {
                continue;
            };
            if !ring.read_buffer(index, &mut buffer) {
                continue;
            }
            if let Some(frame) = htt::non_offload_frame(&buffer, usize::from(msdu.len)) {
                if let Some(length) = htt::ieee80211_to_ethernet(frame, &mut ethernet) {
                    network::receive_ethernet(&ethernet[..length]);
                    delivered = true;
                }
            }
            ring.release(index);
        }
        return if delivered {
            RxOutcome::Delivered
        } else {
            RxOutcome::Dropped
        };
    }

    let mut buffer = alloc::vec![0u8; htt::HTT_RX_BUF_SIZE];
    let mut delivered = false;
    for msdu in &indication.msdus {
        let Some(index) = ring.index_of(msdu.paddr) else {
            continue;
        };
        let length = usize::from(msdu.len).min(htt::HTT_RX_BUF_SIZE);
        if !ring.read_buffer(index, &mut buffer[..length]) {
            continue;
        }
        if let Some(frame) = htt::offload_frame(&buffer[..length]) {
            network::receive_ethernet(frame);
            delivered = true;
        }
        ring.release(index);
    }

    if delivered {
        RxOutcome::Delivered
    } else {
        RxOutcome::Dropped
    }
}

/// Drain WMI scan events until SCAN_COMPLETED or error.
/// Returns the first BSSID seen in a PEER_STA or SCAN event (if any).
/// WMI_SCAN_EVENT types: 0x1=started, 0x2=completed, 0x4=bss_chan,
/// 0x8=foreign_chan, 0x10=dequeued, 0x20=preempted, 0x40=restarted.
unsafe fn drain_scan_events(
    wmi_ep: &mut wmi::WmiEndpoint,
    wmi_rx: &mut RxCe,
    registers: &mut MmioWindow,
) -> Option<[u8; 6]> {
    let mut found_bssid: Option<[u8; 6]> = None;
    // Poll up to 500 iterations (each receive has internal timeout).
    for _ in 0..500 {
        let mut rx_buf = [0u8; 512];
        let event = match unsafe { wmi_ep.receive(wmi_rx, registers, &mut rx_buf) } {
            Ok(event) => event,
            Err(_) => break,
        };
        match event {
            WmiEvent::Scan(s) => {
                crate::println!(
                    "QCA9377 SCAN_EVENT type={:#x} freq={}MHz scan_id={:#x}",
                    s.event_type, s.freq_mhz, s.scan_id
                );
                // 0x2 = WMI_SCAN_EVENT_COMPLETED
                if s.event_type & 0x2 != 0 {
                    break;
                }
            }
            WmiEvent::Other { event_id } => {
                // WMI_ROAM_EVENTID or BSS info events may carry BSSID.
                // For now just log and continue.
                crate::println!("QCA9377 scan drain event_id={:#x}", event_id);
            }
            e => {
                crate::println!("QCA9377 scan drain event: {:?}", e);
            }
        }
    }
    found_bssid
}
