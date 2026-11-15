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
service connection, and service packet send/receive. It is not yet attached to
a runtime CE data transport; WMI/HTT startup and packet handling remain
unimplemented.
An unintegrated CE7 diagnostic helper now validates transfer bounds and maps
QCA6174/QCA9377 target CPU addresses to CE address space. It is not yet wired
into startup, and this work does not change the probe's disabled-by-default
status.
There is not yet a hardware-validated BMI exchange, OTP calibration, firmware
boot, HTC/WMI/HTT support, Wi-Fi association, or a usable wireless link. Linux
[ath10k PCI](https://github.com/torvalds/linux/blob/master/drivers/net/wireless/ath/ath10k/pci.c),
[ath10k hardware IDs](https://github.com/torvalds/linux/blob/master/drivers/net/wireless/ath/ath10k/hw.h),
and [BMI](https://github.com/torvalds/linux/blob/master/drivers/net/wireless/ath/ath10k/bmi.c)
are protocol references. The Rust [NARF ath10k driver](https://github.com/hodgesds/narf/tree/main/drivers/wireless/src/ath10k)
is GPL-2.0; it is used only as a reference and no code is copied into
OCTAGRAM-OS. Device identification and firmware availability can be checked at
boot; hardware runtime validation remains necessary.
