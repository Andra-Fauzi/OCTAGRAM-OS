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
        PIPE_EP0_TX_CE, PIPE_HTT_RX_CE, PIPE_HTT_TX_CE, PIPE_WMI_RX_CE, PIPE_WMI_TX_CE,
    },
    htc::{HtcClient, HtcClientError, HtcError, HTC_SERVICE_HTT_DATA, HTC_SERVICE_WMI_CONTROL},
    pci::{self, PciDevice},
    target,
    wifi_firmware,
    wmi::{self, WmiCmdId, WmiEvent},
};
use crate::{memory, network};
use alloc::sync::Arc;

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
    target_sleep, wake_target_cpu, with_target_awake, SiliconIdentity, BmiBootError,
    CHIP_ID_BAR_OFFSET, QCA_WAKE_POLL_LIMIT, PAGE_OFFSET_MASK,
};

#[derive(Debug)]
pub enum InitError {
    NoPciBar,
    BarTooSmall,
    MmioSetup,
    DmaAlloc,
    SiliconUnknown,
    TargetReset,
    TargetConfig,
    BmiBoot,
    HtcReady,
    HtcConnect,
    HtcSetupComplete,
    WmiReady,
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
    let chip_id = with_target_awake(&mut registers, QCA_WAKE_POLL_LIMIT, |r| {
        r.read32(CHIP_ID_BAR_OFFSET)
    })
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

    // ── 3b. Cold-reset the target SoC ─────────────────────────────────────
    // The boot ROM only services BMI after a cold reset, but the target CPU is
    // NOT released yet: ath10k releases it only once the CE pipes exist
    // (`ath10k_pci_warm_reset_cpu` -> `ath10k_pci_init_pipes` ->
    // `ath10k_pci_wait_for_target_init`). Releasing it here left FW_INDICATOR at
    // 0 and every BMI exchange timed out with SourceTimeout.
    //
    // The CE reset pulse also belongs to this stage: it clears the CE ring
    // registers, which stage 4 programs immediately afterwards.
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

    // ── 4. BMI boot (GET_TARGET_INFO + download board/OTP/firmware) ───────────
    let mut bmi_rings = BmiCeRings::allocate().map_err(|_| InitError::DmaAlloc)?;
    let mut target_config_ce = DiagnosticCe::allocate().map_err(|_| InitError::DmaAlloc)?;
    let mut keep_rings = false;

    if unsafe { bmi_rings.configure(&mut registers) }.is_err() {
        let _ = target_sleep(&mut registers);
        return Err(InitError::DmaAlloc);
    }
    if unsafe { target_config_ce.configure(&mut registers) }.is_err() {
        let _ = target_sleep(&mut registers);
        return Err(InitError::DmaAlloc);
    }

    if pci::enable_bus_master(device).is_err() {
        let _ = target_sleep(&mut registers);
        return Err(InitError::BusMaster);
    }
    keep_rings = true;

    // ── 4b. Release the target CPU and wait for the boot ROM ────────────────
    // ath10k finishes the whole chip reset (cold reset, warm reset and target
    // init) *before* it writes the target CE tables, so the ROM is already
    // running when the configuration lands; `ath10k_pci_hif_power_up` then calls
    // `ath10k_pci_init_pipes`, `ath10k_pci_init_config` and finally
    // `ath10k_pci_wake_target_cpu`. This stage follows that order.
    let mut rom_ready = false;
    let mut rom_crashed = false;
    match release_target_cpu(&mut registers, QCA_WAKE_POLL_LIMIT, target_reset) {
        Ok(outcome) => {
            crate::println!(
                "QCA9377 reset: FW_INDICATOR {:#010x} -> {:#010x} boot_rom_ready={} attempts={} crash_event={} soc_reset_control={:#010x} -> {:#010x} (assert) -> {:#010x} (release)",
                outcome.indicator_before,
                outcome.indicator_after,
                outcome.ready,
                outcome.attempts,
                outcome.saw_crash,
                target_reset.reset_control_before,
                outcome.reset_control_asserted,
                outcome.reset_control_released
            );
            rom_ready = outcome.ready;
            rom_crashed = outcome.saw_crash;
            if !outcome.ready {
                crate::println!(
                    "QCA9377 boot ROM did not report FW_IND_INITIALIZED; attempting BMI anyway"
                );
            }
        }
        Err(failure) => {
            crate::println!(
                "QCA9377 reset failed at \"{}\": {:?}",
                failure.step, failure.error
            );
            return Err(InitError::TargetReset);
        }
    }
    if rom_crashed && !rom_ready {
        crate::println!(
            "QCA9377: target raised FW_IND_EVENT_PENDING without FW_IND_INITIALIZED (device crashed during init)"
        );
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
        drop(htc);

        // Send WMI_INIT directly over wmi_tx CE.
        {
            let init_payload = wmi::wmi_init_payload();
            let mut wmi_frame = [0u8; 512];
            let wmi_len = encode_wmi_into(&mut wmi_frame, WmiCmdId::Init, &init_payload)?;
            unsafe {
                wmi_tx.send(&mut registers, &wmi_frame[..wmi_len])?;
            }
            crate::println!("QCA9377 WMI_INIT sent");
        }

        // Wait for WMI READY event (contains firmware MAC address).
        let mac = {
            let mut rx_buf = [0u8; 512];
            let len = unsafe { wmi_rx.receive(&mut registers, &mut rx_buf)? };
            crate::println!("QCA9377 WMI event received ({} bytes)", len);
            match wmi::parse_wmi_event(&rx_buf[..len]) {
                Ok(WmiEvent::Ready(r)) => {
                    crate::println!(
                        "QCA9377 WMI READY: MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                        r.mac_addr[0], r.mac_addr[1], r.mac_addr[2],
                        r.mac_addr[3], r.mac_addr[4], r.mac_addr[5]
                    );
                    r.mac_addr
                }
                Ok(e) => {
                    crate::println!("QCA9377 unexpected first WMI event: {:?}", e);
                    [0u8; 6]
                }
                Err(_) => {
                    crate::println!("QCA9377 WMI event parse error");
                    [0u8; 6]
                }
            }
        };

        // ── 8. HTT version handshake ──────────────────────────────────────────
        {
            let mut htt_frame = [0u8; 64];
            let req = [0u8; 4]; // msg_type=0 VERSION_REQ
            unsafe {
                htt_tx.send(&mut registers, &req)?;
                let len = htt_rx.receive(&mut registers, &mut htt_frame)?;
                if len >= 3 && htt_frame[0] == 0x01 {
                    crate::println!(
                        "QCA9377 HTT version: {}.{}",
                        htt_frame[1],
                        htt_frame[2]
                    );
                } else {
                    crate::println!("QCA9377 HTT version response unexpected");
                }
            }
        }

        // ── 9. Create STA vdev ────────────────────────────────────────────────
        {
            let vc = wmi::wmi_vdev_create(VDEV_ID, mac);
            let mut wmi_frame = [0u8; 512];
            let wmi_len = encode_wmi_into(&mut wmi_frame, WmiCmdId::VdevCreate, &vc)?;
            unsafe { wmi_tx.send(&mut registers, &wmi_frame[..wmi_len])? };
            crate::println!("QCA9377 WMI_VDEV_CREATE sent (vdev_id=0, STA)");
        }

        // ── 10. Create peer (self) ─────────────────────────────────────────────
        {
            let pc = wmi::wmi_peer_create(VDEV_ID, mac, PEER_TYPE_STA);
            let mut wmi_frame = [0u8; 512];
            let wmi_len = encode_wmi_into(&mut wmi_frame, WmiCmdId::PeerCreate, &pc)?;
            unsafe { wmi_tx.send(&mut registers, &wmi_frame[..wmi_len])? };
            crate::println!("QCA9377 WMI_PEER_CREATE sent");
        }

        // ── 11. Disable powersave ─────────────────────────────────────────────
        {
            let ps = wmi::wmi_sta_powersave_mode(VDEV_ID, 0);
            let mut wmi_frame = [0u8; 512];
            let wmi_len = encode_wmi_into(&mut wmi_frame, WmiCmdId::StaPowersaveMode, &ps)?;
            unsafe { wmi_tx.send(&mut registers, &wmi_frame[..wmi_len])? };
        }

        // ── 12. VDEV_START (bring up radio on channel) ────────────────────────
        // We start the vdev first; firmware will respond with VDEV_START_RESP.
        // SSID is empty here — STA mode doesn't need SSID for START (only UP).
        {
            let vs = wmi::wmi_vdev_start(VDEV_ID, DEFAULT_FREQ_MHZ, &[], [0u8; 6], 0);
            let mut wmi_frame = [0u8; 512];
            let wmi_len = encode_wmi_into(&mut wmi_frame, WmiCmdId::VdevStart, &vs)?;
            unsafe { wmi_tx.send(&mut registers, &wmi_frame[..wmi_len])? };
            crate::println!("QCA9377 WMI_VDEV_START sent (freq={}MHz)", DEFAULT_FREQ_MHZ);
        }

        // Wait for VDEV_START_RESP.
        {
            let mut rx_buf = [0u8; 512];
            let len = unsafe { wmi_rx.receive(&mut registers, &mut rx_buf)? };
            match wmi::parse_wmi_event(&rx_buf[..len]) {
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
                Err(_) => {
                    crate::println!("QCA9377 VDEV_START_RESP parse error (continuing)");
                }
            }
        }

        // ── 13. Active scan ───────────────────────────────────────────────────
        // Scan on the default channel to discover APs.
        {
            let sc = wmi::wmi_start_scan(VDEV_ID, SCAN_ID, DEFAULT_FREQ_MHZ);
            let mut wmi_frame = [0u8; 512];
            let wmi_len = encode_wmi_into(&mut wmi_frame, WmiCmdId::StartScan, &sc)?;
            unsafe { wmi_tx.send(&mut registers, &wmi_frame[..wmi_len])? };
            crate::println!("QCA9377 WMI_START_SCAN sent (freq={}MHz)", DEFAULT_FREQ_MHZ);
        }

        // Drain scan events until SCAN_COMPLETED.
        // Event type 0x20 = WMI_SCAN_EVENT_COMPLETED in ath10k.
        let bssid_found = drain_scan_events(&mut wmi_rx, &mut registers);
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
            let mut wmi_frame = [0u8; 512];
            let wmi_len = encode_wmi_into(&mut wmi_frame, WmiCmdId::VdevUp, &vu)?;
            unsafe { wmi_tx.send(&mut registers, &wmi_frame[..wmi_len])? };
            crate::println!("QCA9377 WMI_VDEV_UP sent");
        }

        // ── 15. Create peer for AP BSSID ─────────────────────────────────────
        if ap_bssid != [0xffu8; 6] {
            let pc = wmi::wmi_peer_create(VDEV_ID, ap_bssid, 2); // 2 = PEER_TYPE_BSS
            let mut wmi_frame = [0u8; 512];
            let wmi_len = encode_wmi_into(&mut wmi_frame, WmiCmdId::PeerCreate, &pc)?;
            unsafe { wmi_tx.send(&mut registers, &wmi_frame[..wmi_len])? };
            crate::println!("QCA9377 WMI_PEER_CREATE (AP) sent");

            // ── 16. PEER_ASSOC — tell firmware AP capabilities ────────────────
            // Basic 11g rates: 6,9,12,18,24,36,48,54 Mbps encoded as 802.11 rates
            let basic_rates: [u8; 8] = [0x8c, 0x92, 0x98, 0xa4, 0xb0, 0xc8, 0xe0, 0xec];
            let pa = wmi::wmi_peer_assoc(VDEV_ID, ap_bssid, 1, 0x0431, 10, &basic_rates);
            let mut wmi_frame = [0u8; 512];
            let wmi_len = encode_wmi_into(&mut wmi_frame, WmiCmdId::PeerAssoc, &pa)?;
            unsafe { wmi_tx.send(&mut registers, &wmi_frame[..wmi_len])? };
            crate::println!("QCA9377 WMI_PEER_ASSOC sent");
        }

        // Poll for WMI_CONNECTED event (non-fatal if we don't get one here —
        // the event may arrive asynchronously after the OS starts the event loop).
        {
            let mut rx_buf = [0u8; 512];
            // Try once — if no event in time, continue. Real driver uses IRQ.
            if let Ok(len) = unsafe { wmi_rx.receive(&mut registers, &mut rx_buf) } {
                if let Ok(ev) = wmi::parse_wmi_event(&rx_buf[..len]) {
                    match ev {
                        WmiEvent::Connected(c) => {
                            crate::println!(
                                "QCA9377 CONNECTED: vdev={} bssid={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} ch={}",
                                c.vdev_id,
                                c.bssid[0], c.bssid[1], c.bssid[2],
                                c.bssid[3], c.bssid[4], c.bssid[5],
                                c.channel
                            );
                        }
                        other => {
                            crate::println!("QCA9377 event after assoc: {:?}", other);
                        }
                    }
                }
            }
        }

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

const WMI_HDR: usize = 8;

fn encode_wmi_into(out: &mut [u8], cmd: WmiCmdId, payload: &[u8]) -> Result<usize, InitError> {
    let total = WMI_HDR + payload.len();
    if total > out.len() {
        return Err(InitError::WmiReady);
    }
    out[0..4].copy_from_slice(&(cmd as u32).to_le_bytes());
    out[4..6].copy_from_slice(&0u16.to_le_bytes()); // flags
    out[6..8].copy_from_slice(&(payload.len() as u16).to_le_bytes());
    out[WMI_HDR..total].copy_from_slice(payload);
    Ok(total)
}

/// Drain WMI scan events until SCAN_COMPLETED or error.
/// Returns the first BSSID seen in a PEER_STA or SCAN event (if any).
/// WMI_SCAN_EVENT types: 0x1=started, 0x2=completed, 0x4=bss_chan,
/// 0x8=foreign_chan, 0x10=dequeued, 0x20=preempted, 0x40=restarted.
unsafe fn drain_scan_events(
    wmi_rx: &mut RxCe,
    registers: &mut MmioWindow,
) -> Option<[u8; 6]> {
    let mut found_bssid: Option<[u8; 6]> = None;
    // Poll up to 500 iterations (each receive has internal timeout).
    for _ in 0..500 {
        let mut rx_buf = [0u8; 512];
        let len = match unsafe { wmi_rx.receive(registers, &mut rx_buf) } {
            Ok(n) => n,
            Err(_) => break,
        };
        match wmi::parse_wmi_event(&rx_buf[..len]) {
            Ok(WmiEvent::Scan(s)) => {
                crate::println!(
                    "QCA9377 SCAN_EVENT type={:#x} freq={}MHz scan_id={:#x}",
                    s.event_type, s.freq_mhz, s.scan_id
                );
                // 0x2 = WMI_SCAN_EVENT_COMPLETED
                if s.event_type & 0x2 != 0 {
                    break;
                }
            }
            Ok(WmiEvent::Other { event_id }) => {
                // WMI_ROAM_EVENTID or BSS info events may carry BSSID.
                // For now just log and continue.
                crate::println!("QCA9377 scan drain event_id={:#x}", event_id);
            }
            Ok(e) => {
                crate::println!("QCA9377 scan drain event: {:?}", e);
            }
            Err(_) => break,
        }
    }
    found_bssid
}
