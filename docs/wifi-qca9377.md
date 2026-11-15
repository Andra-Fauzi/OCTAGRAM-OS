# Wi-Fi Qualcomm QCA9377

ASUS lists the E203NAH wireless module as AW-CM251NF/QCA9377 (1 antenna) in
its [official driver page](https://www.asus.com/supportonly/e203nah/helpdesk_download/).
This is a chipset-specific ath10k PCIe device, not a generic Ethernet adapter.
The kernel's `network::LinkDevice` is the Ethernet-frame interface that a future
Wi-Fi driver can use; USB RNDIS is currently the only registered network link.

The repository includes QCA9377 `firmware-6.bin` and `board-2.bin` from the
`wkennington/linux-firmware` mirror at commit
`711d3297bac870af42088a467459a0634c1970ca`. The blobs are unmodified; the
upstream Qualcomm Atheros license and firmware notice are included beside them
and in generated boot images. That license permits binary redistribution only
for use with a Qualcomm Atheros chipset. The kernel requests these files as
Limine modules. To use different firmware, place the files in
`firmware/ath10k/QCA9377/hw1.0/`, or set `ATH10K_FIRMWARE_DIR` when building:

The bundled blobs' SHA-256 digests are:

```text
firmware-6.bin  8f8b002fccfe81d42238f27dd1f56d189604f180bd4772c7c8e75ae1fef16f01
board-2.bin     0fdcc7838f478da81704de88f7b33e28862110c6d5decf7818543f8e37e6cd98
```

```sh
ATH10K_FIRMWARE_DIR=/path/to/QCA9377/hw1.0 make all
```

On Ubuntu, the files are also provided by `linux-firmware`; on Debian, enable
the `non-free-firmware` component and install `firmware-atheros`. The files
are typically installed under `/lib/firmware/ath10k/QCA9377/hw1.0/`.

PCI discovery now reports matching Qualcomm Atheros `168c:0042` devices, the
PCI revision and subsystem IDs. It enables PCI MMIO decoding, briefly requests
the target wake for register reads, then returns it to sleep; PCI bus mastering
remains disabled. The
QCA6174/QCA9377 shared map places SoC chip ID at BAR offset `0x8f0` and RTC
state at `0x80000`; the probe reads both while the RTC reports the target awake.
On the E203NAH laptop, the boot log showed `RTC_STATE=0x73` (ON) and
`SOC_CHIP_ID=0` from the previously used `0x8ec` offset. That read was from the
wrong register and does not confirm the silicon revision. The corrected `0x8f0`
read and BMI target-info response must be checked before firmware startup. PCI
ID `168c:0042` alone remains ambiguous with QCA6174 hw3.2. The
`firmware-6.bin` ath10k container header and its
required version and image information elements are validated before use. The
`board-2.bin` container is also bounds-checked, and its API can select exact
board and optional extended-board entries using the PCI subsystem IDs, matching
ath10k's fallback board naming. BMI-derived board identifiers are not
implemented yet. The same laptop log confirms firmware modules were loaded
(version `WLAN.TF.2.1-00021-QCARMSWP-1`) and the 8124-byte board data entry was
selected; this does not mean either was transferred to the device. The driver
rejects BMI target versions outside QCA9377 hw1.0/1.1, but that check has not
yet run on the laptop. The feature-gated target-info probe, when enabled, uses
a short CE DMA transaction; it does not boot firmware or claim that Wi-Fi is
running.

The loader extracts the firmware version, OTP image, and executable image.
An experimental `qca9377-bmi-probe` feature contains a bounded BMI
`GET_TARGET_INFO` request over CE0/CE1, with bus mastering temporarily enabled
and DMA pages retained on uncertain cleanup. It is disabled by default: the
upstream PCI reset, target-side CE configuration, and target CPU wake sequence
are not yet implemented. Do not enable this feature for laptop tests until
those prerequisites are ported. The probe does not upload board data, execute
OTP, or start firmware.
The BMI command codec and client support target info, memory/register
operations, execute, app-start, LZ image download, and BMI DONE through a
transport. It also has tested command sequences to download and execute OTP,
then download an app, set its entry point, and finish BMI. LZ downloads pad a
final partial word and flush the stream as ath10k does. The QCA CE adapter
allocates DMA32 pages, programs CE0/CE1 BMI rings, and provides a bounded
polling exchange. A separate BMI boot routine checks target version, uploads
selected board data through the Host Interest address, sets the
board-data-initialized Host Interest flag to `1`, runs OTP, then starts the
firmware. These operations are not invoked during boot because the complete CE
and firmware lifecycle has not been validated. The feature-gated target-info
probe is disabled by default; when enabled, it temporarily enables PCI bus
mastering and verifies it disabled again before releasing DMA pages.
The HTC layer now provides a transport-independent client for READY negotiation,
service connection, and service packet send/receive. It has been updated with
relaxed length checks (`>= 8` instead of `== 8`) for `parse_ready` and
`parse_connect_response` to handle extended HTC ready messages. It also
includes `SETUP_COMPLETE` message support, `HTC_FLAG_NEED_CREDIT_UPDATE` and
`HTC_FLAG_RECV_TRAILER` flag constants, credit tracking per endpoint, trailer
strip on receive, and sequence-number tracking (`send_seq`) for outgoing
packets. The HTC client is not yet attached to a runtime CE data transport;
credit flow control is not yet active, and WMI/HTT startup and packet handling
remain unimplemented.
The CE layer now has separate `TxCe` (source-ring only) and `RxCe`
(destination-ring only) types implementing the ath10k pipe map:
  - EP0 Control: TX=CE0, RX=CE1
  - WMI Control: TX=CE3, RX=CE2
  - HTT Data:    TX=CE4, RX=CE5
The `configure_packet_registers`, `configure_bmi_registers`, and
`configure_diagnostic_registers` functions no longer enforce a strict
zero-index check; instead they read the hardware's current ring index and use
it as the starting index, matching ath10k behaviour after BMI.
`PipePair<R>` wraps a `TxCe`/`RxCe` pair and implements `HtcTransport`,
giving the HTC client a concrete CE-backed send/receive path.
The boot firmware routine now uses the correct QCA9377 hw1.0 load addresses:
OTP at `0x0040_4000` and firmware at `0x0040_8000`; the previous `0x1234`
placeholder values have been replaced. `init_htc_pipes` in `qca9377.rs`
allocates and configures all six CE pipes (EP0/WMI/HTT TX+RX) after BMI
completes, returning an `HtcPipes` struct ready for HTC negotiation.
An unintegrated CE7 diagnostic helper now validates transfer bounds and maps
QCA6174/QCA9377 target CPU addresses to CE address space. It is not yet wired
into startup, and this work does not change the probe's disabled-by-default
status.

Before the first BMI command the driver follows ath10k's QCA6174/QCA9377
bring-up, split into two stages so the ordering matches
`ath10k_pci_warm_reset_cpu` → `ath10k_pci_init_pipes` →
`ath10k_pci_wait_for_target_init`. Stage 3b pulses the SoC global reset (`PCIE_LOCAL + 0x8`; ath10k's
`ath10k_pci_reg_write32()` adds `PCIE_LOCAL_BASE_ADDRESS` itself), wakes the
target and cycles the copy-engine reset line. Stage 4b then releases the target
CPU and waits up to ~3 s for `FW_IND_INITIALIZED`, and only after that does
stage 4c write the target CE pipe/service tables via CE7 — the same order as
`ath10k_pci_qca6174_chip_reset` followed by `ath10k_pci_hif_power_up`
(`init_pipes` → `init_config` → `wake_target_cpu`). ASPM L0s/L1 is disabled
before the device is touched, matching `ath10k_pci_aspm_disable`. Stage 4 then programs the CE0/CE1 BMI rings,
enables PCI bus mastering and writes the target pipe tables, and only after
that stage 4c clears the stale `FW_INDICATOR`, asserts the CPU warm reset
(`RTC_SOC` bit 6) and waits, bounded, for `FW_IND_INITIALIZED`. The warm reset
itself now follows `ath10k_pci_warm_reset` in two phases instead of pulsing the
CPU three times: CPU warm reset -> program the CE pipes -> wait; then clear the
low-frequency timer, pulse the CE reset (which wipes the ring registers), reset
the CPU again -> program the pipes again -> second wait. The pipes are passed to
`release_target_cpu` as a callback so they are only ever programmed in those two
places. Releasing the
CPU before the rings existed left `FW_INDICATOR` at 0 and made every BMI
exchange fail with `SourceTimeout`. The boot log also prints the live
`SOC_RESET_CONTROL` readback and the CE0/CE1 ring registers before the first
BMI command, so a target that never starts can be told apart from one that
starts but never picks the rings up. A target that never reports readiness ends
the bring-up (see below) instead of reaching BMI.

### Boot ROM crash handling

ath10k fails the chip reset when the target sets `FW_IND_EVENT_PENDING`
("device has crashed during init", `-ECOMM` in
`ath10k_pci_wait_for_target_init`) or never sets `FW_IND_INITIALIZED`
(`-ETIMEDOUT`), and `ath10k_pci_hif_power_up` releases the latched crash bit
before giving up. The driver follows
the same rule: `release_target_cpu` clears
`FW_IND_EVENT_PENDING` with a read-modify-write once its warm-reset pulses are
exhausted (`crash_latch_cleared` in the boot log), and `initialize_device`
returns `InitError::TargetCrashed` instead of writing the CE7 target tables and
running BMI against a halted target.

Before it gives up, `initialize_device` retries the complete chip reset
`CHIP_RESET_ATTEMPTS` (3) times, mirroring the loop in
`ath10k_pci_qca988x_chip_reset`: each attempt re-drives `SOC_GLOBAL_RESET`,
re-programs the CE0/CE1 BMI rings and the CE7 diagnostic pipe (the cold reset
and the CE reset pulse clear those registers), and releases the target CPU
again. A target that never comes up therefore spends roughly 12 s per attempt
(`ROM_READY_ROUNDS` after the cold reset plus three warm-reset waits) and about
36 s in total before reporting `TargetCrashed`.

### Host setup before the reset (Linux reference)

A reference Linux boot on the same E203NAH reports
`qca9377 hw1.1 target 0x05020001 chip_id 0x003821ff` and brings `wlp1s0` up, so
the silicon and the power rails are fine and the remaining differences are
host-side. ath10k enables bus mastering in `ath10k_pci_claim` (`pci_set_master`)
and zeroes every CE pipe (`ath10k_pci_ce_deinit` plus
`ath10k_ce_disable_interrupts`) *before* `ath10k_pci_chip_reset`. The driver now
clears all eight pipes while bus mastering is still off and enables the master
bit before the reset, so the boot ROM never starts against a CE configuration
left behind by the firmware that ran before the driver. It also reads the chip id
*after* the chip reset, the way ath10k does, because `SOC_CHIP_ID` reads back
zero before the reset (the earlier pre-reset read reported `0x00000000`, i.e.
"hw1.0", where the chip is really hw1.1).

ath10k also never clears `PCIE_SOC_WAKE` before the chip reset: for
QCA6174/QCA9377 `ath10k_pci_force_wake()` returns immediately, so the SoC stays
awake from probe until the boot ROM finishes initialising. The identity read and
the silicon probe therefore wake the target and leave it awake
(`read_chip_id_awake`), instead of sleeping it microseconds before
`SOC_GLOBAL_RESET`. Everything after the reset — the pipe/service
tables and every BMI command — depends on a running boot ROM, so continuing
only produced a misleading `MissingPcieState` from an unpopulated
`HOST_INTEREST`.

The last E203NAH boot log shows exactly that state:
`FW_INDICATOR 0x00000000 -> 0x00000001`, `boot_rom_ready=false`, `attempts=3`,
`crash_event=true`. Bit 0 is the target's crash latch, not a boot-in-progress
bit, so the ROM is crashing during init and the remaining suspects are outside
the CE/BMI code: PCIe link state after the SoC global reset, reference clock /
power sequencing, and platform ASPM (the known QCA9377 workaround is
`pcie_aspm=off`, which the driver already applies to both the endpoint and its
root port).

After the CE rings are programmed the driver writes the target-side CE layout
and HTC service map (`qca9377/target.rs`, mirroring `ath10k_pci_init_config`):
it follows the HOST_INTEREST pointers to the pipe table and service map, writes
seven `ce_pipe_config` rows plus the QCA9377 service map (including the CE5
outbound override and HTT RX on CE1), clears `PCIE_CONFIG_FLAG_ENABLE_L1`,
stamps the early-allocation magic with nine IRAM banks, and sets
`HI_OPTION_EARLY_CFG_DONE`. The tables go over the CE7 diagnostic pipe before
BMI DONE starts the firmware, which reads them while it boots.

### Complete Initialization & Connection Pipeline (`qca9377/init.rs`)

The full driver initialization sequence is implemented in `kernel/src/drivers/qca9377/init.rs`:
1. **BAR0 Mapping & Silicon Verification**: Maps PCI BAR0, wakes target SoC registers (`0x8f0`), and validates QCA9377 hw1.0 / hw1.1 silicon.
2. **Firmware & Board Data Loading**: Extracts `firmware-6.bin` (FW 10.2) and selects board data from `board-2.bin`.
3. **BMI Boot Sequence**: Configures CE0/CE1, loads board data, executes OTP at `0x0040_4000`, loads and boots firmware at `0x0040_8000`.
4. **HTC Pipe Initialization**: Configures all 6 Copy Engine pipes:
   - Control (EP0): TX=CE0, RX=CE1
   - WMI Control:   TX=CE3, RX=CE2
   - HTT Data:      TX=CE4, RX=CE5
5. **HTC Handshake**: Waits for `HTC_READY`, connects `HTC_SERVICE_WMI_CONTROL` and `HTC_SERVICE_HTT_DATA`, and transmits `HTC_SETUP_COMPLETE`.
6. **WMI Initialization**: Sends `WMI_INIT` inside an HTC data message on the WMI TX pipe (CE3), then polls the WMI RX pipe (CE2) for `WMI_READY` and extracts the hardware MAC address.
7. **HTT Version Handshake & RX Ring**: Sends `HTT_H2T_MSG_TYPE_VERSION_REQ` as an HTC data message on the HTT endpoint (CE4), reads `HTT_T2H_MSG_TYPE_VERSION_CONF` from CE5, then describes the RX buffer ring with `HTT_H2T_MSG_TYPE_RX_RING_CFG`.
8. **STA Vdev & Peer Creation**: Sends `WMI_VDEV_CREATE` (STA mode), `WMI_PEER_CREATE` (self), and disables STA powersave.
9. **Radio Bring-up & Scanning**: Sends `WMI_VDEV_START` (2.4 GHz channel), waits for `VDEV_START_RESP`, triggers active scan via `WMI_START_SCAN`, and drains scan completion events.
10. **AP Association**: Issues `WMI_VDEV_UP`, `WMI_PEER_CREATE` (BSS peer), and `WMI_PEER_ASSOC` to associate with the target AP.
11. **Network Link Registration**: Registers `Qca9377Link` into kernel `network::register_link()`, providing slot ID `0x9377`.

### WMI over HTC (`qca9377/wmi.rs`)

WMI is an HTC service, not a raw copy-engine protocol. The previous path wrote
the command straight to CE3 with a private header and read unparsed payloads off
CE2, so the firmware never received a valid HTC data message. It now follows
ath10k:

- Each WMI command is framed as an HTC data message for the endpoint the target
  returned from `connect_service(HTC_SERVICE_WMI_CONTROL)`:
  `[endpoint | flags | len(2) | control(4)][WMI message]`, with the HTC sequence
  number stamped into `control[0]`. Commands go to the WMI TX pipe (CE3), never
  to the control pipe CE0.
- Events are read from the WMI RX pipe (CE2) and parsed as HTC messages, so the
  endpoint id is checked and a credit trailer (`HTC_FLAG_RECV_TRAILER`, whose
  last byte is the trailer length) is stripped before the payload reaches the
  WMI parser.
- The WMI message header is a single little-endian word (`struct wmi_cmd_hdr`,
  `WMI_HEADER_LEN = 4`) masked with `WMI_CMD_HDR_CMD_ID_MASK`, replacing the
  earlier ad-hoc 8-byte `cmd_id | flags | len` header.
- `WMI_READY_EVENT` follows `struct wmi_ready_event`: `sw_version`,
  `abi_version`, `mac_addr[6]`, `status`. The MAC address is at payload offset 8,
  so the driver now logs the firmware version and ABI and takes the hardware
  address from the correct field. If READY never arrives, bring-up fails with
  `InitError::WmiReady` instead of registering a link with an all-zero MAC.

### HTT over HTC (`qca9377/htt.rs`)

HTT is an HTC service too (`ATH10K_HTC_SVC_ID_HTT_DATA_MSG`, group 3 index 0, i.e.
`0x0300`), so the version handshake follows the same rule as WMI: the request is
written to the HTT TX pipe (CE4) inside an HTC data message for the endpoint the
target assigned to HTT, and the confirmation is decoded from CE5 with the same
endpoint check and credit-trailer handling. Earlier a bare 4-byte frame went to
CE4 and the reply was parsed without any HTC framing.

After the handshake the driver describes the RX buffer ring, as
`ath10k_htt_send_rx_ring_cfg` does: a coherent array of 32-bit buffer addresses
(`htt_rx_ring_setup_32.rings[0].rx_ring_base_paddr`), one coherent word the
firmware uses as its next-free index (`fw_idx_shadow_reg_paddr`), the ring length
(`rx_ring_len`, a power of two — 64 here instead of ath10k's 2048 because every
buffer is a whole 4 KiB DMA frame), `rx_ring_bufsize` = `HTT_RX_BUF_SIZE`
(2048), every `HTT_RX_RING_FLAGS_*` bit in `flags`, and `fw_idx_init_val` = 0
because the host posted buffers from index 0. The message goes out as
`HTT_H2T_MSG_TYPE_RX_RING_CFG` on the HTT HTC endpoint, and the ring is leaked
on purpose: the firmware keeps DMA-ing into those buffers.

The `htt_rx_ring_rx_desc_offsets` field carries the v1 rx descriptor layout that
QCA9377 shares with QCA6174/QCA988x/QCA99x0. `qca99x0_rx_desc_ops` gives no
`rx_desc_get_offsets`, so ath10k falls back to
`offsetof(struct htt_rx_desc_v1, field) / 4`: `mac80211_hdr` 59, `msdu_payload`
75, `ppdu_start` 21, `ppdu_end` 31, `mpdu_start` 3, `mpdu_end` 20, `msdu_start`
6, `msdu_end` 10, `rx_attention` 1, `frag_info` 2. Those come from the byte
offsets 4, 8, 12, 24, 40, 80, 84, 124, 236 and 300 in the packed descriptor
(`rx_desc.h`), which makes `sizeof(struct htt_rx_desc_v1)` 300 bytes — the MSDU
payload starts right after it.

The message contents now match `ath10k_htt_h2t_ver_req_msg` and
`ath10k_htt_t2h_version_conf`: the request is `htt_cmd_hdr { msg_type = 0 }`
padded to a word, and the response is `msg_type = 0` followed by
`struct htt_ver_resp { minor, major, rsvd0 }`. The driver previously expected
`msg_type = 1` and read major before minor.

### HTT RX indications (`qca9377/htt.rs`, `qca9377/init.rs`)

Received frames arrive as in-order paddr indications: TLV firmware (`htt-op 3`)
sends `HTT_TLV_T2H_MSG_TYPE_RX_IN_ORD_PADDR_IND` (0x12) on the HTT HTC endpoint,
with `struct htt_rx_in_ord_ind` — `info`, `peer_id`, `vdev_id`, `msdu_count` and
one `htt_rx_in_ord_msdu_desc { msdu_paddr, msdu_len, fw_desc }` per MSDU. The
driver looks each `msdu_paddr` up in the RX ring it described, copies the buffer
out, and hands the frame to `network::receive_ethernet`. After copying it calls
`HttRxRing::release`, which advances the ring's allocation index the way ath10k
does when it re-posts a buffer.

Offloaded indications (`HTT_RX_IN_ORD_IND_INFO_OFFLOAD_MASK`) hold a
`struct htt_rx_offload_msdu` header followed by a complete Ethernet frame, so
they go straight to the stack. Non-offloaded buffers hold an rx descriptor plus
an 802.11 frame; those are counted as dropped, because 802.11 decapsulation is
not implemented here. After association the bring-up drains up to 32 HTT
messages and logs `QCA9377 HTT RX: <n> frame(s) delivered, <m> dropped`.

### Shell Commands

The shell provides user commands under `net wifi`:
- `net status`: Show current interface MAC, link state, and IPv4 lease info.
- `net wifi status`: Display current WiFi interface status.
- `net wifi scan`: Trigger 2.4 GHz active scan across channels.
- `net wifi connect <ssid>`: Associate to a target SSID.
- `net dhcp`: Request an IPv4 address lease via DHCP once WiFi link is associated.

Linux
[ath10k PCI](https://github.com/torvalds/linux/blob/master/drivers/net/wireless/ath/ath10k/pci.c),
[ath10k hardware IDs](https://github.com/torvalds/linux/blob/master/drivers/net/wireless/ath/ath10k/hw.h),
and [BMI](https://github.com/torvalds/linux/blob/master/drivers/net/wireless/ath/ath10k/bmi.c)
are protocol references. The Rust [NARF ath10k driver](https://github.com/hodgesds/narf/tree/main/drivers/wireless/src/ath10k)
is GPL-2.0; it is used only as a reference and no code is copied into
OCTAGRAM-OS. Device identification and firmware availability can be checked at
boot; hardware runtime validation remains necessary.
