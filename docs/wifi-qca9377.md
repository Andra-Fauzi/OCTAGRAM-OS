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

### The Failed Bring-up Is Restarted From the Idle Loop

ath10k can give up after its three reset attempts because Linux reloads the
driver later; this kernel has no module reload, so giving up left the machine
without a WLAN function until the next power cycle. `report_pci_candidates`
now records the WLAN function and, when the bring-up fails, arms a supervisor
(`drivers::qca9377::poll_retry`) that `main`'s idle loop calls next to
`drivers::poll_deferred`. Each pass runs one complete `initialize_device`
(another full chip reset, CE7 tables, BMI and firmware start) and logs it as
`QCA9377 restarting the driver (automatic attempt N)`; the loop only stops when
the link is registered, which is reported as `QCA9377 Wi-Fi active after N
restart(s)`.

The supervisor never runs from a shell task, because one chip reset blocks for
about 36 s; a typed command keeps queuing keyboard input while an attempt runs.
Every restart is a fresh `initialize_device`, so failures before the reset
(for example an MMIO window the cold reset dropped) are retried as well, not
only `TargetCrashed`. A failed attempt sets a spin throttle
(`RETRY_COOLDOWN_POLLS` idle-loop passes) so an instant failure cannot turn the
idle loop into a hot retry loop; a real reset failure already spends tens of
seconds in the reset waits. The throttle is a pass count rather than a
timeout because the kernel has no monotonic millisecond clock.

`net wifi retry` clears the throttle and asks for a restart immediately; it
reports when the link is already up or no WLAN function was found. Note that a
link that does come up still stops at `WmiTlvUnsupported`, because the shipped
`firmware-6.bin` speaks WMI-TLV and the packed WMI codec is the only one
implemented.

### The PCIe Link Is Waited For Around Every Reset

`SOC_GLOBAL_RESET` resets the endpoint's PCIe core, so the link has to be
re-established before the boot ROM can use it. Linux never waits for this because
the PCI core and the root-port driver own the link; the driver only sampled
`link_status` once, immediately after the cold reset, where the E203NAH log
showed `0x1011` — 2.5 GT/s, width 1, bit 12 (DLL Active) set and bit 10 (Link
Training) clear, i.e. a trained, live link. The first version of this decoder
read the wrong bits (13/11) and reported the link as dead; an earlier attempt
was abandoned on that false reading. A single sample still cannot tell "not
trained yet" from "never came back", so the wait loops instead of trusting one
read, but the log line is now trustworthy.

`wait_for_pcie_link` now samples the endpoint's `PCI_EXP_LNKSTA` once per
millisecond for up to 500 ms and logs
`QCA9377 PCIe link <stage>: dll_active=... training=... status=... root_port_status=... waited=...ms retrained=...`.
If DLL is still down after 50 ms it toggles Link Disable (`PCI_EXP_LNKCTL_LD`)
on the endpoint so both ends retrain, then keeps sampling. The wait runs before
each cold reset, **immediately after the cold reset inside `reset_target_soc`**
(before the 3 s wait for the ROM rather than after it), and again from
`program_pipes`, which is invoked after each CPU warm reset and before that
phase's wait for the boot ROM.

### The WLAN Rail Is Switched On Through ACPI (`_PR0`/`_PS0`)

Writing the PCI power-management state only moves the *function* to D0; it does
not run the platform's ACPI power methods, which is what Linux relies on. A
bare-metal kernel never evaluates `_PS0` for a device, so a rail or reference
clock that firmware left off stays off, and the target aborts on its first access —
which matches a boot ROM that latches `FW_IND_EVENT_PENDING` right after the host
clears the indicator. `PM_CSR 0x3000 -> 0x0000` on this laptop puts the function
in D0 already, so config space answered while the SoC had no working rail.

`drivers::acpi::aml` is a minimal AML reader: it parses the DSDT and every SSDT
into a path-indexed namespace (`Device`, `Scope`, `Method`, `Name`,
`PowerResource`, `OperationRegion`, `Field`, `Alias`, `Processor`,
`ThermalZone`), skipping constructs it does not decode one byte at a time so an
unknown vendor block cannot hide the definitions that follow. `Object::Method`
records only the byte range of its body, and a small interpreter runs `Store`,
`If`/`Else`/`While`, `Return`, `Local`/`Arg` and field access over SystemIO and
SystemMemory, so the `_ON`/`_STA` bodies real firmware ships actually execute.

`drivers::acpi::power::power_on_pci_device` finds the host bridge (`_HID`
`PNP0A03`/`PNP0A08` whose `_BBN` matches the bus), finds the endpoint by
`_ADR = (device << 16) | function`, and evaluates `_PR0` for the endpoint and for
every device between it and the bridge (a PCIe port can own the rail that feeds
the slot below it). Each referenced power resource is checked with `_STA` and
switched on with `_ON`; `_PS0` is evaluated last when the device has one. The
namespace is parsed once, and a fully successful report is cached so the bring
-up retry does not re-run the same methods.

The result is logged before the first register access:

```text
QCA9377 ACPI power: device=\_SB.PCI0.RP01.WLAN power_resources=2 already_on=1 enabled=1 ps0=evaluated
```

A resource that cannot be switched on gets its own line with the AML error, and
a machine with no usable ACPI path logs
`QCA9377 ACPI power unavailable: <reason>` and continues, because the rest of the
bring-up is still worth attempting.

### The CPU Warm-Reset Bit Is Sampled Twice

ath10k writes `SOC_RESET_CONTROL_CPU_WARM_RST_MASK` (bit 6) and never reads it
back, so a write that does not stick is invisible there. `release_target_cpu`
now samples `SOC_RESET_CONTROL` immediately after the write and again 1 ms
later, and the reset line logs both values
(`... (after warm reset, 0x... after 1ms) cpu_warm_rst_visible=...`). The log can
therefore separate "the CPU was never taken through reset", which would make
every retry re-read the crash latch of the first failed boot, from "the bit is
self-clearing". `cpu_warm_rst_visible` is set when either sample shows the bit.

### The LF Timer And The CPU Clock Are Checked Before The Cold Reset

`ath10k_pci_warm_reset` stops the low-frequency timer before it touches the copy
engines, because an armed timer keeps waking the target while it is being reset.
The same timer keeps counting across `SOC_GLOBAL_RESET`, so the driver now reads
`SOC_LF_TIMER_CONTROL0` (`RTC_SOC + 0x50`), clears its enable bit (`0x4`), and
logs the value it found as `lf_timer_before=` on the cold-reset line.

The `QCA9377 window` samples now also carry `lf_timer`, `cpu_clock`
(`RTC_SOC + 0x20`), `clock_control` (`RTC_SOC + 0x28`) and `pcie_intr`
(`PCIE_LOCAL + 0xc`). On the E203NAH the target sets `FW_IND_EVENT_PENDING`
within one millisecond of the cold-reset release, before the host touches the
chip, so the remaining explanations are a timer/watchdog that fires during ROM
start-up and a CPU/PLL clock that is not up yet. These four registers separate
the two without another guess.

### The AML Parse Cannot Spin

One boot printed `QCA9377 ACPI power: parsing the ACPI namespace` and then froze;
the parse of a real vendor DSDT is the only work behind that line. Termination
now rests on two limits and one rule:

- `MAX_TERM_BYTES` (8 KiB, enforced by `Cursor`) bounds a single term, so no
  definition can read for ever;
- `MAX_PARSE_STEPS` (1M terms) bounds the whole parse across tables and
  recursion, so even an oscillation in `cursor.position` ends;
- every `parse_term_list` iteration must advance the cursor; a construct that
  reports success without consuming a byte is skipped like an unsupported one.

Running out of either limit marks the namespace `truncated` and lets the boot
continue: a partial namespace only loses definitions, a hung parse loses the
machine. `MAX_PARSE_DEPTH` is unchanged and is about the stack, not about work.
Each table's signature and size, the object count, the resync count and the
truncation flag are printed, and while tracing is on the parser reports its
offset every 4 KiB, so the next boot says exactly where it stopped.

### BAR0 Is Sized Only After The Function Is In D0

`pci::get_bar0_size` writes `u32::MAX` into BAR0 and reads the mask back. Doing
that to an endpoint that firmware left in D3cold wedges the root complex: the
E203NAH froze between the probe's `board data selected` line and the first line
of `initialize_device`, with nothing but the BAR reads in between. The ACPI
`_PR0`/`_PS0` step and `force_d0` now run first, and the log prints
`QCA9377 BAR0 window at ...` and `QCA9377 BAR0 size ...` so a future hang is
placed on the exact call.

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

The current QCA9377 bring-up is a work in progress, not a usable Wi-Fi
interface. The PCI/BMI/HTC groundwork exists, but the bundled firmware declares
WMI-TLV and initialization deliberately stops at `WmiTlvUnsupported` before
sending `WMI_INIT`. Consequently, the later packed-WMI VDEV/scan/association
code below is not reachable with the bundled firmware. It must not be treated
as implemented functionality until the WMI-TLV path is ported:
1. **BAR0 Mapping & Silicon Verification**: Maps PCI BAR0, wakes target SoC registers (`0x8f0`), and validates QCA9377 hw1.0 / hw1.1 silicon.
2. **Firmware & Board Data Loading**: Extracts `firmware-6.bin` (FW 10.2) and selects board data from `board-2.bin`.
3. **BMI Boot Sequence**: Configures CE0/CE1, loads board data, executes OTP at `0x0040_4000`, loads and boots firmware at `0x0040_8000`.
4. **HTC Pipe Initialization**: Configures all 6 Copy Engine pipes:
   - Control (EP0): TX=CE0, RX=CE1
   - WMI Control:   TX=CE3, RX=CE2
   - HTT Data:      TX=CE4, RX=CE5
5. **HTC Handshake**: Waits for `HTC_READY`, connects `HTC_SERVICE_WMI_CONTROL` and `HTC_SERVICE_HTT_DATA`, and transmits `HTC_SETUP_COMPLETE`.
6. **WMI Initialization (blocked)**: The packed WMI codec does not match the bundled WMI-TLV firmware. The driver stops with `WmiTlvUnsupported` before sending `WMI_INIT`.
7. **HTT, VDEV, Scan and Association (not reached with this firmware)**: These packed-WMI stages are not currently reachable. In addition, HTT handshake and RX-ring setup now fail initialization instead of continuing after errors, and the VDEV start acknowledgement is required.
8. **Network data path (disabled)**: The QCA9377 is not registered as a `LinkDevice` because HTT TX and a complete 802.11 authentication/association state machine are not implemented. DHCP, ARP, and IP traffic must not be reported as available.

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

### PCIe interrupts (`qca9377/ce.rs`, `arch/x86_64/interrupt.rs`)

ath10k does not poll the copy engines: the target raises one MSI
(`pci irq msi oper_irq_mode 2`) and `ath10k_pci_interrupt_handler()` acknowledges
`SOC_CORE + PCIE_INTR_CAUSE (0x000c)` after masking `PCIE_INTR_ENABLE (0x0008)`
and writing `PCIE_INTR_CLR (0x0014)` — the clear only latches with the enable at
zero. Work is deferred: `ath10k_ce_per_engine_service_any()` reads
`CE_WRAPPER_INTERRUPT_SUMMARY` (0x34000), clears the per-engine copy-complete
status (`host_ie_addr 0x2c`, `cc_mask 0x1`, `wm_mask 0x1e`) and the pipe is only
unmasked again once its descriptors have been reaped.

This driver now follows the same split. `initialize_device` enables MSI on
vector `QCA9377_MSI_VECTOR` (45) through `pci::enable_msi()`, unmasks copy
complete on the six EP0/WMI/HTT pipes and arms the chip's interrupt output with
`pcie_intr_fw_mask | pcie_intr_ce_mask_all` (`0x400 | 0x7f800`, qca6174/qca9377
values). The IDT handler (`common_interrupt_handler` → `qca9377::handle_irq`) only
acknowledges the cause, clears the engines it saw and parks them in
`ce::interrupt::PENDING`; `wait_for_completion()` sleeps on that bit and reads the
ring index only after the wake-up, then unmasks its engine again. Firmware and
CE causes are both kept enabled, so WMI and HTT events are delivered the moment
the target posts them.

Two honest limits. The pipe still samples the ring index every 65 536 spins as a
safety net, counted as `QCA9377 PCIe interrupts serviced: <n> (fallback ring
samples: <m>)` — a non-zero `m` means the interrupt did not wake the wait. And if
interrupts are not armed (`ce::interrupt::is_armed()`), the original bounded
polling loop runs unchanged, so a boot without MSI cannot hang.

### HTC credit flow control (`qca9377/htc.rs`)

HTC READY advertises `credit_size` and `credit_count`, and every send then costs
credits: `ath10k_htc_consume_credit` computes
`DIV_ROUND_UP(frame_len, credit_size)` over the whole HTC frame (header
included), refuses the send when the endpoint has fewer, and
`ath10k_htc_prepare_tx_skb` sets `ATH10K_HTC_FLAG_NEED_CREDIT_UPDATE` so the
target answers with a credit report. Reports arrive in receive trailers: a
trailer is a sequence of records `{ id, len, pad[2] }` plus a `len`-byte body,
and record `ATH10K_HTC_RECORD_CREDITS` (1) holds `len / 4`
`struct ath10k_htc_credit_report { eid, credits, pad[2] }` entries that are added
back to the matching endpoint.

The driver now mirrors that: `HtcCredits` tracks `credit_size` and the remaining
credits per endpoint, every WMI/HTT/EP0 send takes its credits (failing with
`NoCredits` instead of overrunning the target) and sets the
`NEED_CREDIT_UPDATE` flag, and receive trailers are parsed with
`parse_credit_reports` so refunds land on the right endpoint. Initial credits
come from HTC READY; because this driver drives one endpoint at a time they are
not split between services the way ath10k splits its pool. The bring-up logs the
remaining credits as `QCA9377 HTC credits left: WMI ... HTT ...`.

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
they go straight to the stack.

Non-offloaded indications carry the raw 802.11 frame instead. The buffer holds
the v1 rx descriptor first — its `mac80211_hdr` copy fills bytes 236..300 — and
the frame itself starts at the `msdu_payload` offset, **300**. The frame length is
the indication's `msdu_len` (ath10k's `amsdu_len`), and the descriptor only counts
once `RX_ATTENTION_FLAGS_MSDU_DONE` (bit 31) is set; `MSDU_LENGTH_ERR` (bit 17)
and `MPDU_LENGTH_ERR` (bit 27) reject the frame rather than pass it truncated.

Because this kernel has no mac80211, `htt::ieee80211_to_ethernet()` does the
`ieee80211_data_to_8023_exthdr()` job: it maps the IBSS, ToDS and FromDS address
triples to Ethernet destination/source, skips the 2-byte QoS control field (and
the 4-byte HT control field when the order bit is set), requires the LLC/SNAP
header `aa aa 03 00 00 00` plus an EtherType, and refuses what it cannot decode —
WDS (four-address), A-MSDU aggregates (QoS bit 7), protected frames (no key is
installed here) and non-data frames. The resulting Ethernet frame goes to
`network::receive_ethernet`. After association the bring-up drains up to 32 HTT
messages and logs `QCA9377 HTT RX: <n> frame(s) delivered, <m> dropped`.

### Chip Reset Follows `ath10k_pci_warm_reset`

The reset path is now a transcription of the ath10k functions rather than a
reconstruction of them:

- `ath10k_pci_warm_reset_si0` → `warm_reset_cpu` → `ath10k_pci_init_pipes` →
  `ath10k_pci_wait_for_target_init` (ath10k discards this result) →
  `ath10k_pci_warm_reset_clear_lf` → `ath10k_pci_warm_reset_ce` →
  `warm_reset_cpu` → `init_pipes` → `wait_for_target_init` (this one decides).
  Both phases always run; the driver no longer stops after the first one.
- `warm_reset_cpu` only *sets* `SOC_RESET_CONTROL_CPU_WARM_RST_MASK` (0x40) and
  clears `FW_INDICATOR` first. ath10k never clears the bit again, so the previous
  "release" write — a read-modify-write of `SOC_RESET_CONTROL` bits the driver
  does not own — is gone.
- `warm_reset_ce` re-reads `SOC_RESET_CONTROL` immediately before the pulse and
  writes the original value back with only the CE bit cleared, so the CPU warm
  reset bit survives the pulse.
- `ath10k_pci_wait_for_target_init` breaks on `FW_IND_EVENT_PENDING` before it
  looks at `FW_IND_INITIALIZED`, so a crashed target is reported on the first
  sample instead of after the 3 s timeout.
- Delays are real milliseconds: `SOC_GLOBAL_RESET` holds 20 ms per edge and the
  CE pulse holds 10 ms, matching the `msleep()` calls ath10k makes because PCIe
  is not stable for immediate register access after a global reset. The count
  comes from the 8254 PIT channel 2 (1.193182 MHz), not from a spin loop whose
  duration depends on the CPU.

After the cold reset the driver walks a millisecond ladder (+1, +5, +20, +100,
+500, +3000 ms) over the SoC window, re-driving `PCIE_SOC_WAKE` at every step
because the reset drops the wake request. Each sample logs `RTC_STATE`,
`SOC_GLOBAL_RESET`, BAR0, `SOC_CHIP_ID` and `FW_INDICATOR`, and the walk stops at
the first valid chip id. A zero, all-ones or `0xdeadc0de` chip id counts as "no
register answered", so a window that never comes back is visible in the log
instead of being read as hw1.0. PCIe configuration space is re-read at the same
point (id, command, BAR0, link control/status), and memory decoding, bus
mastering and BAR0 are restored if the reset cleared them — ath10k relies on
`ath10k_pci_claim` having set those once, which a bare-metal driver cannot
assume.

### BMI Sequence Follows `ath10k_core_start`

`boot_firmware` now runs the same steps in the same order as ath10k's
pre-firmware path:

- `ath10k_bmi_get_target_info` (BMI command 8) and the QCA9377 target-version
  check.
- `ath10k_init_configure_target`: `hi_app_host_interest = HTC_PROTOCOL_VERSION`
  (0x0002), `hi_option_flag` with one device, AP firmware mode and the
  globally-unique MAC address method, `hi_be = 0`, `hi_fw_swap = 0` and
  `hi_hci_uart_pwr_mgmt_params_ext = device id`.
- `ath10k_download_board_data`: `hi_board_data` is read and
  `hi_board_data_initialized` written through `BMI_READ_SOC_REGISTER` /
  `BMI_WRITE_SOC_REGISTER` (ath10k's `ath10k_bmi_read32`/`write32`), while the
  board-data payload itself uses `BMI_WRITE_MEMORY`.
- OTP upload and `BMI_EXECUTE`, with a non-zero result treated as a calibration
  failure (`ath10k_download_and_run_otp`).
- Firmware upload through BMI `LZ_STREAM_START` / `LZ_DATA` / closing
  `LZ_STREAM_START(0)`, then `ath10k_init_uart` (`hi_serial_enable = 0`), then
  `BMI_SET_APP_START` and `BMI_DONE`.

Two ath10k steps deliberately do not exist here. `ath10k_hw_qca6174_enable_pll_clock`
is only reachable through `qca6174_ops`; the QCA9377 hw1.0/hw1.1 entries use
`qca988x_ops`, which has no `enable_pll_clk`. And `fw_diag_ce_download` is
`false` for QCA9377, so ath10k itself never takes the diag-CE fast download path
on this chip.

### A Latched Crash Is Cleared, Not Accepted

A target that keeps reporting `FW_IND_EVENT_PENDING` after a cold reset is the
situation `ath10k_pci_hif_power_up` handles on the error path: it calls
`ath10k_pci_fw_crashed_clear` and keeps going. The bring-up now does the same —
it clears the latch after the cold reset and polls `FW_INDICATOR` again for the
full 3 s window, so a ROM that only starts once the stale bit is gone is no
longer reported as a dead device. The boot log prints the outcome as
`rom_started_after_clear`.

The warm reset also opens with `ath10k_pci_warm_reset_si0`. On QCA6174/QCA9377
`qca6174_regs.soc_reset_control_si0_rst_mask` is `0x0`, so that step only reads
`SOC_RESET_CONTROL` and waits 20 ms; it is kept because ath10k settles the SI0
block there before resetting the target CPU.

### `ath10k_pci_init_config` Audit

`target.rs` was checked field by field against Linux `pci.h` and
`ath10k_pci_init_config`, with no divergence left:

- `hi_interconnect_state` is at `0xf8` of HOST_INTEREST, and its value is used
  directly as `pcie_state_targ_addr` (ath10k does the same).
- `struct pcie_state` offsets: `pipe_cfg_addr` 0x00, `svc_to_pipe_map` 0x04,
  `config_flags` 0x20, with `PCIE_CONFIG_FLAG_ENABLE_L1 = 0x1`.
- `ath10k_pci_get_num_banks` returns 9 for QCA9377, and `hi_early_alloc` carries
  magic `0x6d8a` in bits 31:16 plus the bank count in bits 3:0.
- `hi_option_flag2 |= HI_OPTION_EARLY_CFG_DONE (0x10)` is the last write, and it
  is read back through CE7 — the only value that proves the tables reached
  target IRAM.
- CE7 transfers are bounded by `DIAG_TRANSFER_LIMIT` (2048 bytes), which matches
  `CE_DIAGNOSTIC_MAX_BYTES`.

ath10k does not programme the MSI fields of `pcie_state` here either, so the
absence of those writes is correct.

A silicon identity is only decoded from a real `SOC_CHIP_ID` value: zero,
all-ones and `0xdeadc0de` are reported as "unknown before the chip reset" and
the identity is decided by the post-reset read. If that read is a valid value
that is not a QCA9377, the driver stops instead of loading QCA9377 firmware.

### The Firmware Speaks WMI-TLV

Reading the information elements of the shipped `firmware-6.bin` settles which
WMI flavour this chip runs (`QCA-ATH10K` header, then `id, len, data` elements
padded to four bytes):

| element | value | meaning |
| --- | --- | --- |
| 0 | `WLAN.TF.2.1-00021-QCARMSWP-1` | firmware version |
| 4 | 24193 B segment table | OTP image |
| 3 | 727125 B segment table | firmware image |
| 2 | `0xc0` | features: wowlan + ignore-otp |
| 5 | `4` | `ATH10K_FW_WMI_OP_VERSION_TLV` |
| 6 | `3` | `ATH10K_FW_HTT_OP_VERSION_TLV` |

So the firmware is **WMI-TLV**, the same thing a Linux boot prints as
`htt-ver 3.56 wmi-op 4 htt-op 3`. `ath10k_wmi_attach` selects an entire
operations table from that element, and for TLV the table lives in `wmi_tlv.c`:
commands and events become nested `{tag, len, value}` elements and the event ids
come from a different enumeration than the packed branches.

Comparing our handler with `ath10k_wmi.c` therefore finds four divergences, in
the order they matter:

1. **Flavour.** `wmi.rs` encodes packed structures and uses ids such as `0x1001`
   for READY. No ath10k header contains that value: the packed families use
   `0x1`/`0x2` (main) or `0x8000`/`0x8001` (10.x), and TLV uses its own tags. A TLV
   firmware would never match any of our event ids.
2. **Order.** `ath10k_core_start` waits for `SERVICE_READY` first, sends
   `WMI_INIT` second and waits for `READY` third. `SERVICE_READY` is what carries
   the service bitmap (`ath10k_wmi_map_svc`) and the host-memory requests;
   `READY` only carries the MAC address and status. We send `INIT` before any
   service information exists and never build a service map.
3. **Payload.** `ath10k_wmi_10_2_op_gen_init` sends `wmi_resource_config_10_2`
   (28 words of `wmi_resource_config_10x` plus 7 words, 136 bytes) followed by
   the host-memory chunks; ours is a 60-byte sketch that matches no ath10k
   structure. `TARGET_10X_*` values would have to replace the guessed numbers.
4. **Memory chunks.** `SERVICE_READY` may ask the host for DMA pools that
   `WMI_INIT` must hand back. We answer with `count = 0` unconditionally.

The driver now parses elements 2, 5 and 6, prints them next to the version, and
refuses to send the packed `WMI_INIT` when the blob declares TLV. The failure is
reported as `WmiTlvUnsupported` rather than a misleading `WmiReady` timeout.
Porting `wmi_tlv.c`'s tag parser, command encoding, `SERVICE_READY` host-memory
request handling and `READY` event parsing is the prerequisite for anything
past the HTC handshake.

The initialization path has also been hardened:
- A failed HTT version handshake or RX-ring setup now aborts bring-up.
- A missing, unexpected or unsuccessful `VDEV_START_RESP` aborts bring-up.
- The kernel no longer registers a QCA9377 `LinkDevice` whose TX method always
  returns an error.
- The shell does not claim that QCA9377 scan or connection commands succeeded
  when the corresponding WMI-TLV and 802.11 MLME paths are not implemented.

These changes make failure reporting more accurate; they do **not** make the
QCA9377 data path operational yet.

### Shell Commands

The shell provides user commands under `net wifi`:
- `net status`: Show current interface MAC, link state, and IPv4 lease info.
- `net wifi status`: Display current WiFi interface status.
- `net wifi scan`: Trigger 2.4 GHz active scan across channels.
- `net wifi connect <ssid>`: Associate to a target SSID.
- `net wifi bmi`: Re-run the BMI handshake and dump the host-interest words
  (`hi_app_host_interest`, `hi_option_flag`, `hi_serial_enable`, `hi_board_data`,
  `hi_board_data_initialized`, `hi_fw_swap`, UART power-management word). `BMI_DONE`
  is never sent, so the boot ROM is left where the probe found it.
- `net wifi retry`: Restart the driver after a failed bring-up without waiting
  for the idle-loop supervisor's next attempt.
- `net dhcp`: Request an IPv4 address lease via DHCP once WiFi link is associated.

Linux
[ath10k PCI](https://github.com/torvalds/linux/blob/master/drivers/net/wireless/ath/ath10k/pci.c),
[ath10k hardware IDs](https://github.com/torvalds/linux/blob/master/drivers/net/wireless/ath/ath10k/hw.h),
and [BMI](https://github.com/torvalds/linux/blob/master/drivers/net/wireless/ath/ath10k/bmi.c)
are protocol references. The Rust [NARF ath10k driver](https://github.com/hodgesds/narf/tree/main/drivers/wireless/src/ath10k)
is GPL-2.0; it is used only as a reference and no code is copied into
OCTAGRAM-OS. Device identification and firmware availability can be checked at
boot; hardware runtime validation remains necessary.
