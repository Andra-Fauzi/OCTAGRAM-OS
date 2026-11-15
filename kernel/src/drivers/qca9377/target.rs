//! Target-side Copy Engine layout and HTC service map
//! (`ath10k_pci_init_config`).
//!
//! The boot ROM publishes, through HOST_INTEREST, where the firmware expects
//! its per-pipe CE table and its HTC service-to-pipe map. Filling those tables
//! tells the firmware which CE carries HTC control, WMI and HTT traffic; without
//! it HTC never starts even when BMI succeeds.
//!
//! Reference: Linux `drivers/net/wireless/ath/ath10k/pci.c`
//! (`ath10k_pci_init_config`, `pci_target_ce_config_wlan`,
//! `pci_target_service_to_ce_map_wlan`, `ath10k_pci_override_ce_config`) and
//! `targaddrs.h` (`struct host_interest`).

use super::ce::{CeError, DiagnosticCe, RegisterIo};

/// HOST_INTEREST base address in target memory.
pub(super) const HOST_INTEREST_ADDRESS: u32 = 0x0040_0800;
/// `struct host_interest` fields used here (`targaddrs.h`).
const HI_OPTION_FLAG2: u32 = 0xcc;
const HI_INTERCONNECT_STATE: u32 = 0xf8;
const HI_EARLY_ALLOC: u32 = 0x100;
const HI_OPTION_EARLY_CFG_DONE: u32 = 0x10;
const HI_EARLY_ALLOC_MAGIC: u32 = 0x6d8a;
const HI_EARLY_ALLOC_MAGIC_MASK: u32 = 0xffff_0000;
const HI_EARLY_ALLOC_MAGIC_SHIFT: u32 = 16;
const HI_EARLY_ALLOC_IRAM_BANKS_MASK: u32 = 0x0000_000f;

/// Offsets inside `struct pcie_state` (Linux `ath10k/pci.h`).
const PCIE_STATE_PIPE_CFG_ADDR: u32 = 0x00;
const PCIE_STATE_SVC_TO_PIPE_MAP: u32 = 0x04;
const PCIE_STATE_CONFIG_FLAGS: u32 = 0x20;
const PCIE_CONFIG_FLAG_ENABLE_L1: u32 = 1 << 0;

/// `PIPEDIR_*`: direction relative to the host.
const PIPEDIR_IN: u32 = 1;
const PIPEDIR_OUT: u32 = 2;
const PIPEDIR_INOUT: u32 = 3;

/// HTC service IDs (`enum ath10k_htc_svc_id`, `SVC(group, index)`).
const SVC_RSVD_CTRL: u32 = 0x001;
const SVC_WMI_CONTROL: u32 = 0x100;
const SVC_WMI_DATA_BE: u32 = 0x101;
const SVC_WMI_DATA_BK: u32 = 0x102;
const SVC_WMI_DATA_VI: u32 = 0x103;
const SVC_WMI_DATA_VO: u32 = 0x104;
const SVC_HTT_DATA_MSG: u32 = 0x300;
const SVC_TEST_RAW_STREAMS: u32 = 0xfe00;

/// IRAM banks reported for QCA9377 hw1.x (`ath10k_bus_get_num_banks`).
pub(super) const QCA9377_IRAM_BANKS: u32 = 9;

/// `struct ce_pipe_config` rows: pipe, direction, entries, max bytes, flags,
/// reserved.
///
/// QCA9377 reports seven target pipes (`num_target_ce_config_wlan = 7`) and
/// receives the QCA6174/QCA9377 CE5 override from
/// `ath10k_pci_override_ce_config`: CE5 becomes an outbound pipe with a
/// 2048-byte maximum instead of the generic inbound entry.
const PIPE_CONFIG: [[u32; 6]; 7] = [
    [0, PIPEDIR_OUT, 32, 256, 0, 0],
    [1, PIPEDIR_IN, 32, 2048, 0, 0],
    [2, PIPEDIR_IN, 64, 2048, 0, 0],
    [3, PIPEDIR_OUT, 32, 2048, 0, 0],
    [4, PIPEDIR_OUT, 256, 256, 0, 0],
    [5, PIPEDIR_OUT, 32, 2048, 0, 0],
    [6, PIPEDIR_INOUT, 32, 4096, 0, 0],
];

/// `struct ce_service_to_pipe` rows: service, direction, pipe. The last entry
/// is the zero terminator.
///
/// QCA6174/QCA9377 move HTT RX onto CE1 (`serv_to_pipe[15]` in the Linux
/// override), so the host receive map must follow the same pipe.
const SERVICE_MAP: [[u32; 3]; 17] = [
    [SVC_WMI_DATA_VO, PIPEDIR_OUT, 3],
    [SVC_WMI_DATA_VO, PIPEDIR_IN, 2],
    [SVC_WMI_DATA_BK, PIPEDIR_OUT, 3],
    [SVC_WMI_DATA_BK, PIPEDIR_IN, 2],
    [SVC_WMI_DATA_BE, PIPEDIR_OUT, 3],
    [SVC_WMI_DATA_BE, PIPEDIR_IN, 2],
    [SVC_WMI_DATA_VI, PIPEDIR_OUT, 3],
    [SVC_WMI_DATA_VI, PIPEDIR_IN, 2],
    [SVC_WMI_CONTROL, PIPEDIR_OUT, 3],
    [SVC_WMI_CONTROL, PIPEDIR_IN, 2],
    [SVC_RSVD_CTRL, PIPEDIR_OUT, 0],
    [SVC_RSVD_CTRL, PIPEDIR_IN, 1],
    [SVC_TEST_RAW_STREAMS, PIPEDIR_OUT, 0],
    [SVC_TEST_RAW_STREAMS, PIPEDIR_IN, 1],
    [SVC_HTT_DATA_MSG, PIPEDIR_OUT, 4],
    [SVC_HTT_DATA_MSG, PIPEDIR_IN, 1],
    [0, 0, 0],
];

const PIPE_CONFIG_BYTES: usize = PIPE_CONFIG.len() * 6 * 4;
const SERVICE_MAP_BYTES: usize = SERVICE_MAP.len() * 3 * 4;

/// 32-bit and block access to target memory.
pub trait TargetMemory {
    fn read32(&mut self, address: u32) -> Result<u32, CeError>;
    fn write32(&mut self, address: u32, value: u32) -> Result<(), CeError>;
    fn write_block(&mut self, address: u32, bytes: &[u8]) -> Result<(), CeError>;
}

/// Addresses the boot ROM published, for the boot log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetConfigReport {
    pub pcie_state: u32,
    pub pipe_config_address: u32,
    pub service_map_address: u32,
    pub config_flags: u32,
    pub early_alloc: u32,
    pub option_flag2: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetConfigError {
    Ce(CeError),
    MissingPcieState,
    MissingPipeConfigAddress,
    MissingServiceMapAddress,
}

impl From<CeError> for TargetConfigError {
    fn from(error: CeError) -> Self {
        TargetConfigError::Ce(error)
    }
}

/// Hand the target its CE layout and HTC service map.
///
/// Mirrors `ath10k_pci_init_config`: every address comes from HOST_INTEREST, so
/// a zero there means the boot ROM never published it and the tables must not
/// be written.
pub fn configure_pipes(
    memory: &mut impl TargetMemory,
) -> Result<TargetConfigReport, TargetConfigError> {
    let pcie_state = memory.read32(HOST_INTEREST_ADDRESS + HI_INTERCONNECT_STATE)?;
    if pcie_state == 0 {
        return Err(TargetConfigError::MissingPcieState);
    }

    let pipe_config_address = memory.read32(pcie_state + PCIE_STATE_PIPE_CFG_ADDR)?;
    if pipe_config_address == 0 {
        return Err(TargetConfigError::MissingPipeConfigAddress);
    }
    memory.write_block(pipe_config_address, &encode_pipe_config())?;

    let service_map_address = memory.read32(pcie_state + PCIE_STATE_SVC_TO_PIPE_MAP)?;
    if service_map_address == 0 {
        return Err(TargetConfigError::MissingServiceMapAddress);
    }
    memory.write_block(service_map_address, &encode_service_map())?;

    let config_flags = memory.read32(pcie_state + PCIE_STATE_CONFIG_FLAGS)?;
    let config_flags = config_flags & !PCIE_CONFIG_FLAG_ENABLE_L1;
    memory.write32(pcie_state + PCIE_STATE_CONFIG_FLAGS, config_flags)?;

    let early_alloc = memory.read32(HOST_INTEREST_ADDRESS + HI_EARLY_ALLOC)?;
    let early_alloc = (early_alloc
        & !(HI_EARLY_ALLOC_MAGIC_MASK | HI_EARLY_ALLOC_IRAM_BANKS_MASK))
        | ((HI_EARLY_ALLOC_MAGIC << HI_EARLY_ALLOC_MAGIC_SHIFT) & HI_EARLY_ALLOC_MAGIC_MASK)
        | (QCA9377_IRAM_BANKS & HI_EARLY_ALLOC_IRAM_BANKS_MASK);
    memory.write32(HOST_INTEREST_ADDRESS + HI_EARLY_ALLOC, early_alloc)?;

    let option_flag2 = memory.read32(HOST_INTEREST_ADDRESS + HI_OPTION_FLAG2)?;
    let option_flag2 = option_flag2 | HI_OPTION_EARLY_CFG_DONE;
    memory.write32(HOST_INTEREST_ADDRESS + HI_OPTION_FLAG2, option_flag2)?;

    Ok(TargetConfigReport {
        pcie_state,
        pipe_config_address,
        service_map_address,
        config_flags,
        early_alloc,
        option_flag2,
    })
}

/// Target memory reached over the CE7 diagnostic pipe.
pub struct Ce7TargetMemory<'a, R: RegisterIo> {
    diagnostic: &'a mut DiagnosticCe,
    registers: &'a mut R,
}

impl<'a, R: RegisterIo> Ce7TargetMemory<'a, R> {
    /// # Safety
    /// CE7 must be configured for polled transfers, PCI bus mastering must be
    /// enabled, and both the diagnostic rings and `registers` must stay alive
    /// and exclusively owned for this value's lifetime.
    pub unsafe fn new(diagnostic: &'a mut DiagnosticCe, registers: &'a mut R) -> Self {
        Self {
            diagnostic,
            registers,
        }
    }
}

impl<R: RegisterIo> TargetMemory for Ce7TargetMemory<'_, R> {
    fn read32(&mut self, address: u32) -> Result<u32, CeError> {
        let mut bytes = [0u8; 4];
        unsafe {
            self.diagnostic
                .read_target(self.registers, address, &mut bytes)?;
        }
        Ok(u32::from_le_bytes(bytes))
    }

    fn write32(&mut self, address: u32, value: u32) -> Result<(), CeError> {
        unsafe {
            self.diagnostic
                .write_target(self.registers, address, &value.to_le_bytes())
        }
    }

    fn write_block(&mut self, address: u32, bytes: &[u8]) -> Result<(), CeError> {
        unsafe {
            self.diagnostic
                .write_target(self.registers, address, bytes)
        }
    }
}

fn encode_rows<const COLUMNS: usize>(rows: &[[u32; COLUMNS]], out: &mut [u8]) {
    for (row_index, row) in rows.iter().enumerate() {
        for (column, value) in row.iter().enumerate() {
            let offset = (row_index * COLUMNS + column) * 4;
            out[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
}

fn encode_pipe_config() -> [u8; PIPE_CONFIG_BYTES] {
    let mut out = [0u8; PIPE_CONFIG_BYTES];
    encode_rows(&PIPE_CONFIG, &mut out);
    out
}

fn encode_service_map() -> [u8; SERVICE_MAP_BYTES] {
    let mut out = [0u8; SERVICE_MAP_BYTES];
    encode_rows(&SERVICE_MAP, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::BTreeMap;
    use alloc::vec::Vec;

    #[derive(Default)]
    struct MockTarget {
        memory: BTreeMap<u32, u32>,
        blocks: Vec<(u32, Vec<u8>)>,
        writes: Vec<(u32, u32)>,
    }

    impl MockTarget {
        fn with_rom_addresses() -> Self {
            let mut target = Self::default();
            target
                .memory
                .insert(HOST_INTEREST_ADDRESS + HI_INTERCONNECT_STATE, 0x1000);
            target
                .memory
                .insert(0x1000 + PCIE_STATE_PIPE_CFG_ADDR, 0x2000);
            target
                .memory
                .insert(0x1000 + PCIE_STATE_SVC_TO_PIPE_MAP, 0x3000);
            target
                .memory
                .insert(0x1000 + PCIE_STATE_CONFIG_FLAGS, 0x1);
            target
        }
    }

    impl TargetMemory for MockTarget {
        fn read32(&mut self, address: u32) -> Result<u32, CeError> {
            Ok(self.memory.get(&address).copied().unwrap_or(0))
        }

        fn write32(&mut self, address: u32, value: u32) -> Result<(), CeError> {
            self.writes.push((address, value));
            self.memory.insert(address, value);
            Ok(())
        }

        fn write_block(&mut self, address: u32, bytes: &[u8]) -> Result<(), CeError> {
            self.blocks.push((address, bytes.into()));
            Ok(())
        }
    }

    fn word(bytes: &[u8], index: usize) -> u32 {
        u32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap())
    }

    #[test]
    fn writes_pipe_and_service_tables_at_rom_published_addresses() {
        let mut target = MockTarget::with_rom_addresses();
        let report = configure_pipes(&mut target).unwrap();

        assert_eq!(report.pcie_state, 0x1000);
        assert_eq!(report.pipe_config_address, 0x2000);
        assert_eq!(report.service_map_address, 0x3000);
        assert_eq!(target.blocks.len(), 2);

        let (pipe_address, pipe_config) = &target.blocks[0];
        assert_eq!(*pipe_address, 0x2000);
        assert_eq!(pipe_config.len(), 168);
        assert_eq!(word(pipe_config, 0), 0);
        assert_eq!(word(pipe_config, 1), PIPEDIR_OUT);
        assert_eq!(word(pipe_config, 2), 32);
        assert_eq!(word(pipe_config, 3), 256);
        // CE5 row (six words per row) carries the QCA9377 override.
        assert_eq!(word(pipe_config, 30 + 1), PIPEDIR_OUT);
        assert_eq!(word(pipe_config, 30 + 2), 32);
        assert_eq!(word(pipe_config, 30 + 3), 2048);
        // CE6 stays bidirectional with a 4096-byte maximum.
        assert_eq!(word(pipe_config, 36 + 1), PIPEDIR_INOUT);
        assert_eq!(word(pipe_config, 36 + 3), 4096);

        let (map_address, service_map) = &target.blocks[1];
        assert_eq!(*map_address, 0x3000);
        assert_eq!(service_map.len(), 204);
        assert_eq!(word(service_map, 0), SVC_WMI_DATA_VO);
        // HTT DATA MSG IN (entry 15) is remapped from CE5 to CE1.
        assert_eq!(word(service_map, 45), SVC_HTT_DATA_MSG);
        assert_eq!(word(service_map, 46), PIPEDIR_IN);
        assert_eq!(word(service_map, 47), 1);
        // Zero terminator.
        assert_eq!(word(service_map, 48), 0);
        assert_eq!(word(service_map, 49), 0);
        assert_eq!(word(service_map, 50), 0);
    }

    #[test]
    fn clears_l1_and_marks_early_configuration_done() {
        let mut target = MockTarget::with_rom_addresses();
        let report = configure_pipes(&mut target).unwrap();

        assert_eq!(report.config_flags, 0);
        assert!(target
            .writes
            .contains(&(0x1000 + PCIE_STATE_CONFIG_FLAGS, 0)));
        assert_eq!(report.early_alloc, (HI_EARLY_ALLOC_MAGIC << 16) | 9);
        assert!(target
            .writes
            .contains(&(HOST_INTEREST_ADDRESS + HI_EARLY_ALLOC, report.early_alloc)));
        assert_eq!(report.option_flag2, HI_OPTION_EARLY_CFG_DONE);
        assert!(target
            .writes
            .contains(&(HOST_INTEREST_ADDRESS + HI_OPTION_FLAG2, 0x10)));
    }

    #[test]
    fn refuses_a_target_that_did_not_publish_its_tables() {
        let mut target = MockTarget::default();
        assert_eq!(
            configure_pipes(&mut target),
            Err(TargetConfigError::MissingPcieState)
        );
        assert!(target.blocks.is_empty());

        let mut target = MockTarget::with_rom_addresses();
        target
            .memory
            .insert(0x1000 + PCIE_STATE_PIPE_CFG_ADDR, 0);
        assert_eq!(
            configure_pipes(&mut target),
            Err(TargetConfigError::MissingPipeConfigAddress)
        );
        assert!(target.blocks.is_empty());

        let mut target = MockTarget::with_rom_addresses();
        target
            .memory
            .insert(0x1000 + PCIE_STATE_SVC_TO_PIPE_MAP, 0);
        assert_eq!(
            configure_pipes(&mut target),
            Err(TargetConfigError::MissingServiceMapAddress)
        );
        assert_eq!(target.blocks.len(), 1);
    }
}
