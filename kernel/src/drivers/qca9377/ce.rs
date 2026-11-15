use super::bmi::BmiTransport;
use super::htc::HtcTransport;
use crate::memory;
use core::{
    ptr::NonNull,
    sync::atomic::{Ordering, fence},
};

const DMA_PAGE_SIZE: usize = 4096;
const CE_POLL_LIMIT: usize = 10_000_000;

pub const CE0_SOURCE_RING_ENTRIES: usize = 16;
pub const CE1_DESTINATION_RING_ENTRIES: usize = 512;
pub const CE_BMI_SOURCE_MAX_BYTES: usize = 256;
pub const CE_BMI_DESTINATION_BUFFER_BYTES: usize = 2048;
pub const CE0_REGISTER_BASE: u64 = 0x34400;
pub const CE_REGISTER_STRIDE: u64 = 0x400;
pub const CE1_REGISTER_BASE: u64 = CE0_REGISTER_BASE + CE_REGISTER_STRIDE;
pub const CE7_REGISTER_BASE: u64 = CE0_REGISTER_BASE + 7 * CE_REGISTER_STRIDE;
pub const CE_DIAGNOSTIC_RING_ENTRIES: usize = 2;
pub const CE_DIAGNOSTIC_MAX_BYTES: usize = 2048;
pub const CE_PACKET_RING_ENTRIES: usize = 16;
pub const CE_PACKET_MAX_BYTES: usize = 2048;
pub const CE_PIPE_COUNT: usize = 8;

pub const CE_SOURCE_RING_BASE: u64 = 0x00;
pub const CE_SOURCE_RING_SIZE: u64 = 0x04;
pub const CE_DESTINATION_RING_BASE: u64 = 0x08;
pub const CE_DESTINATION_RING_SIZE: u64 = 0x0c;
pub const CE_CONTROL: u64 = 0x10;
pub const CE_COMMAND: u64 = 0x18;
pub const CE_HOST_INTERRUPT_ENABLE: u64 = 0x2c;
pub const CE_MISC_INTERRUPT_ENABLE: u64 = 0x34;
pub const CE_SOURCE_WRITE_INDEX: u64 = 0x3c;
pub const CE_DESTINATION_WRITE_INDEX: u64 = 0x40;
pub const CE_SOURCE_READ_INDEX: u64 = 0x44;
pub const CE_DESTINATION_READ_INDEX: u64 = 0x48;
pub const CE_SOURCE_WATERMARK: u64 = 0x4c;
pub const CE_DESTINATION_WATERMARK: u64 = 0x50;
const CE_CONTROL_DMAX_MASK: u32 = 0x0000_ffff;
const CE_CONTROL_SOURCE_BYTE_SWAP: u32 = 1 << 16;
const CE_CONTROL_DESTINATION_BYTE_SWAP: u32 = 1 << 17;
const CE_REGISTER_WINDOW_END: u64 = CE7_REGISTER_BASE + CE_DESTINATION_WATERMARK + 4;
const QCA6174_CORE_CTRL_ADDRESS: u64 = 0x0003_a000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CeError {
    InvalidRingSize,
    InvalidHardwareIndex,
    RingFull,
    DmaAddressOverflow,
    DmaAddressOutOfRange,
    DmaAddressMisaligned,
    OutOfDmaFrames,
    HhdmUnavailable,
    DmaAddressTranslationMismatch,
    TransferTooLarge,
    RegisterOutOfRange,
    DeviceUnavailable,
    EngineNotIdle,
    InvalidRequest,
    Timeout,
    SourceTimeout,
    DestinationTimeout,
    InvalidResponseLength,
    ResponseBufferTooSmall,
    TransportPoisoned,
    InvalidPipe,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Descriptor {
    address: u32,
    byte_count: u16,
    flags: u16,
}

impl Descriptor {
    pub fn new(address: u64, byte_count: usize, flags: u16) -> Result<Self, CeError> {
        let byte_count = u16::try_from(byte_count).map_err(|_| CeError::TransferTooLarge)?;
        let end = address
            .checked_add(u64::from(byte_count))
            .ok_or(CeError::DmaAddressOverflow)?;
        if address > u64::from(u32::MAX) || end > 1_u64 << 32 {
            return Err(CeError::DmaAddressOutOfRange);
        }
        if address & 3 != 0 {
            return Err(CeError::DmaAddressMisaligned);
        }

        Ok(Self {
            address: (address as u32).to_le(),
            byte_count: byte_count.to_le(),
            flags: flags.to_le(),
        })
    }

    pub const fn byte_len(self) -> u16 {
        u16::from_le(self.byte_count)
    }

    pub const fn address(self) -> u32 {
        u32::from_le(self.address)
    }

    pub const fn flags(self) -> u16 {
        u16::from_le(self.flags)
    }
}

struct DmaPage {
    physical: usize,
    virtual_address: NonNull<u8>,
}

impl DmaPage {
    fn allocate() -> Result<Self, CeError> {
        let physical = memory::allocate_dma32_frame().ok_or(CeError::OutOfDmaFrames)?;
        let Some(virtual_address) = memory::physical_to_virtual(physical) else {
            memory::free_frame(physical);
            return Err(CeError::HhdmUnavailable);
        };
        if physical & (DMA_PAGE_SIZE - 1) != 0 {
            memory::free_frame(physical);
            return Err(CeError::DmaAddressMisaligned);
        }
        if !is_dma32_range(physical as u64, DMA_PAGE_SIZE) {
            memory::free_frame(physical);
            return Err(CeError::DmaAddressOutOfRange);
        }
        if memory::virtual_to_physical(virtual_address) != Some(physical) {
            memory::free_frame(physical);
            return Err(CeError::DmaAddressTranslationMismatch);
        }

        let Some(virtual_address) = NonNull::new(virtual_address as *mut u8) else {
            memory::free_frame(physical);
            return Err(CeError::HhdmUnavailable);
        };
        Ok(Self {
            physical,
            virtual_address,
        })
    }

    const fn physical_address(&self) -> u32 {
        self.physical as u32
    }

    fn write(&self, source: &[u8]) {
        unsafe {
            core::ptr::copy_nonoverlapping(
                source.as_ptr(),
                self.virtual_address.as_ptr(),
                source.len(),
            );
        }
        fence(Ordering::Release);
    }

    fn read(&self, destination: &mut [u8]) {
        fence(Ordering::Acquire);
        for (index, byte) in destination.iter_mut().enumerate() {
            *byte = unsafe { self.virtual_address.as_ptr().add(index).read_volatile() };
        }
    }
}

impl Drop for DmaPage {
    fn drop(&mut self) {
        memory::free_frame(self.physical);
    }
}

pub struct DescriptorRing {
    page: DmaPage,
    indices: RingIndices,
}

impl DescriptorRing {
    pub fn allocate(entries: usize) -> Result<Self, CeError> {
        let indices = RingIndices::new(entries)?;
        let ring_bytes = entries
            .checked_mul(core::mem::size_of::<Descriptor>())
            .ok_or(CeError::InvalidRingSize)?;
        if ring_bytes > DMA_PAGE_SIZE {
            return Err(CeError::InvalidRingSize);
        }
        Ok(Self {
            page: DmaPage::allocate()?,
            indices,
        })
    }

    pub const fn physical_address(&self) -> u32 {
        self.page.physical as u32
    }

    pub const fn entry_count(&self) -> usize {
        self.indices.mask + 1
    }

    pub const fn read_index(&self) -> usize {
        self.indices.read_index()
    }

    pub const fn write_index(&self) -> usize {
        self.indices.write_index()
    }

    pub fn enqueue(&mut self, descriptor: Descriptor) -> Result<usize, CeError> {
        if !self.indices.has_free_entry() {
            return Err(CeError::RingFull);
        }
        let index = self.indices.write_index();
        let descriptor_pointer = unsafe {
            self.page
                .virtual_address
                .as_ptr()
                .cast::<Descriptor>()
                .add(index)
        };
        unsafe { descriptor_pointer.write_volatile(descriptor) };
        fence(Ordering::Release);
        self.indices.enqueue()
    }

    pub fn complete(&mut self, hardware_read_index: usize) -> Result<Option<Descriptor>, CeError> {
        let Some(index) = self.indices.take_completed(hardware_read_index)? else {
            return Ok(None);
        };
        fence(Ordering::Acquire);
        let descriptor_pointer = unsafe {
            self.page
                .virtual_address
                .as_ptr()
                .cast::<Descriptor>()
                .add(index)
        };
        let descriptor = unsafe { descriptor_pointer.read_volatile() };
        Ok(Some(descriptor))
    }
}

pub struct BmiCeRings {
    source: DescriptorRing,
    destination: DescriptorRing,
    receive_buffer: DmaPage,
    transmit_buffer: DmaPage,
    poisoned: bool,
}

impl BmiCeRings {
    pub fn allocate() -> Result<Self, CeError> {
        let source = DescriptorRing::allocate(CE0_SOURCE_RING_ENTRIES)?;
        let mut destination = DescriptorRing::allocate(CE1_DESTINATION_RING_ENTRIES)?;
        let receive_buffer = DmaPage::allocate()?;
        let transmit_buffer = DmaPage::allocate()?;
        let receive_descriptor =
            Descriptor::new(u64::from(receive_buffer.physical_address()), 0, 0)?;
        destination.enqueue(receive_descriptor)?;

        Ok(Self {
            source,
            destination,
            receive_buffer,
            transmit_buffer,
            poisoned: false,
        })
    }

    pub const fn source_ring(&self) -> &DescriptorRing {
        &self.source
    }

    pub const fn destination_ring(&self) -> &DescriptorRing {
        &self.destination
    }

    pub const fn receive_buffer_address(&self) -> u32 {
        self.receive_buffer.physical_address()
    }

    /// Program the CE0/CE1 BMI rings without enabling PCI bus mastering.
    ///
    /// # Safety
    /// The device must be awake and exclusively owned, PCI bus mastering must
    /// be disabled, and no other agent may access these engines. Keep `self`
    /// alive until DMA has stopped and the ring base registers are cleared.
    pub unsafe fn configure(&mut self, registers: &mut impl RegisterIo) -> Result<(), CeError> {
        if self.source.entry_count() != CE0_SOURCE_RING_ENTRIES
            || self.destination.entry_count() != CE1_DESTINATION_RING_ENTRIES
            || self.source.write_index() != 0
            || self.destination.read_index() != 0
            || self.destination.write_index() != 1
        {
            return Err(CeError::InvalidRingSize);
        }

        configure_bmi_registers(
            registers,
            self.source.physical_address(),
            self.destination.physical_address(),
        )
    }

    /// Clear the CE0/CE1 ring registers so the DMA pages can be released.
    ///
    /// # Safety
    /// PCI bus mastering must be disabled, the device must be awake, and no
    /// transfer may be in flight.
    pub unsafe fn deconfigure(&mut self, registers: &mut impl RegisterIo) -> Result<(), CeError> {
        if self.poisoned {
            return Err(CeError::TransportPoisoned);
        }
        if self.source.read_index() != self.source.write_index() {
            return Err(CeError::EngineNotIdle);
        }

        clear_bmi_registers(registers)
    }

    /// Exchange one aligned BMI message over CE0/CE1 using bounded polling.
    ///
    /// # Safety
    /// `configure` must have completed, PCI bus mastering must be enabled,
    /// this object must remain alive while the device can access its DMA pages,
    /// and no other caller may use the engines. After an error following the
    /// source write-index update, keep the object alive until DMA is stopped.
    pub unsafe fn exchange(
        &mut self,
        registers: &mut impl RegisterIo,
        request: &[u8],
        response: Option<&mut [u8]>,
    ) -> Result<usize, CeError> {
        validate_bmi_request(request)?;
        if self.poisoned {
            return Err(CeError::TransportPoisoned);
        }

        self.transmit_buffer.write(request);
        let descriptor = Descriptor::new(
            u64::from(self.transmit_buffer.physical_address()),
            request.len(),
            0,
        )?;
        self.source.enqueue(descriptor)?;
        self.poisoned = true;
        registers.write32(
            CE0_REGISTER_BASE + CE_SOURCE_WRITE_INDEX,
            self.source.write_index() as u32,
        )?;

        let sent = wait_for_completion(
            registers,
            CE0_REGISTER_BASE + CE_SOURCE_READ_INDEX,
            &mut self.source,
        )
        .map_err(|error| completion_error(error, CeError::SourceTimeout))?;
        if sent.address() != self.transmit_buffer.physical_address()
            || usize::from(sent.byte_len()) != request.len()
        {
            return Err(CeError::InvalidResponseLength);
        }

        let Some(response) = response else {
            self.poisoned = false;
            return Ok(0);
        };
        let received = wait_for_completion(
            registers,
            CE1_REGISTER_BASE + CE_DESTINATION_READ_INDEX,
            &mut self.destination,
        )
        .map_err(|error| completion_error(error, CeError::DestinationTimeout))?;
        let received_length = usize::from(received.byte_len());
        let response_error = if received.address() != self.receive_buffer.physical_address()
            || received_length == 0
            || received_length > CE_BMI_DESTINATION_BUFFER_BYTES
        {
            Some(CeError::InvalidResponseLength)
        } else if response.len() < received_length {
            Some(CeError::ResponseBufferTooSmall)
        } else {
            self.receive_buffer.read(&mut response[..received_length]);
            None
        };

        let receive_descriptor =
            Descriptor::new(u64::from(self.receive_buffer.physical_address()), 0, 0)?;
        self.destination.enqueue(receive_descriptor)?;
        registers.write32(
            CE1_REGISTER_BASE + CE_DESTINATION_WRITE_INDEX,
            self.destination.write_index() as u32,
        )?;
        self.poisoned = false;

        if let Some(error) = response_error {
            return Err(error);
        }
        Ok(received_length)
    }
}

pub struct BmiCeTransport<'a, R: RegisterIo> {
    rings: &'a mut BmiCeRings,
    registers: &'a mut R,
}

pub struct DiagnosticCe {
    source: DescriptorRing,
    destination: DescriptorRing,
    buffer: DmaPage,
    poisoned: bool,
}

impl DiagnosticCe {
    pub fn allocate() -> Result<Self, CeError> {
        Ok(Self {
            source: DescriptorRing::allocate(CE_DIAGNOSTIC_RING_ENTRIES)?,
            destination: DescriptorRing::allocate(CE_DIAGNOSTIC_RING_ENTRIES)?,
            buffer: DmaPage::allocate()?,
            poisoned: false,
        })
    }

    /// Configure CE7 for polled target-memory transfers.
    ///
    /// # Safety
    /// The device must be awake and exclusively owned, PCI bus mastering must
    /// be disabled, and no agent may access CE7.
    pub unsafe fn configure(&mut self, registers: &mut impl RegisterIo) -> Result<(), CeError> {
        if self.source.entry_count() != CE_DIAGNOSTIC_RING_ENTRIES
            || self.destination.entry_count() != CE_DIAGNOSTIC_RING_ENTRIES
            || self.source.write_index() != 0
            || self.destination.write_index() != 0
        {
            return Err(CeError::InvalidRingSize);
        }
        configure_diagnostic_registers(
            registers,
            self.source.physical_address(),
            self.destination.physical_address(),
        )
    }

    /// Clear CE7's register references before releasing its DMA pages.
    ///
    /// # Safety
    /// PCI bus mastering must be disabled and no CE7 transfer may be in flight.
    pub unsafe fn deconfigure(&mut self, registers: &mut impl RegisterIo) -> Result<(), CeError> {
        if self.poisoned {
            return Err(CeError::TransportPoisoned);
        }
        if self.source.read_index() != self.source.write_index()
            || self.destination.read_index() != self.destination.write_index()
        {
            return Err(CeError::EngineNotIdle);
        }
        clear_pipe_registers(registers, CE7_REGISTER_BASE)
    }

    /// Read target memory using a target CPU address converted to CE space.
    ///
    /// # Safety
    /// CE7 must be configured, PCI bus mastering enabled, and this object must
    /// remain alive until DMA has stopped.
    pub unsafe fn read_target(
        &mut self,
        registers: &mut impl RegisterIo,
        target_address: u32,
        output: &mut [u8],
    ) -> Result<(), CeError> {
        validate_diagnostic_transfer(target_address, output.len())?;
        let core_ctrl = registers.read32(QCA6174_CORE_CTRL_ADDRESS)?;
        let ce_address = target_cpu_to_ce_address(core_ctrl, target_address);
        self.transfer(
            registers,
            ce_address,
            self.buffer.physical_address(),
            output.len(),
        )?;
        self.buffer.read(output);
        Ok(())
    }

    /// Write target memory using a target CPU address converted to CE space.
    ///
    /// # Safety
    /// CE7 must be configured, PCI bus mastering enabled, and this object must
    /// remain alive until DMA has stopped.
    pub unsafe fn write_target(
        &mut self,
        registers: &mut impl RegisterIo,
        target_address: u32,
        data: &[u8],
    ) -> Result<(), CeError> {
        validate_diagnostic_transfer(target_address, data.len())?;
        let core_ctrl = registers.read32(QCA6174_CORE_CTRL_ADDRESS)?;
        let ce_address = target_cpu_to_ce_address(core_ctrl, target_address);
        self.buffer.write(data);
        self.transfer(
            registers,
            self.buffer.physical_address(),
            ce_address,
            data.len(),
        )
    }

    fn transfer(
        &mut self,
        registers: &mut impl RegisterIo,
        source_address: u32,
        destination_address: u32,
        length: usize,
    ) -> Result<(), CeError> {
        if self.poisoned {
            return Err(CeError::TransportPoisoned);
        }

        let destination = Descriptor::new(u64::from(destination_address), 0, 0)?;
        let source = Descriptor::new(u64::from(source_address), length, 0)?;
        self.poisoned = true;
        self.destination.enqueue(destination)?;
        registers.write32(
            CE7_REGISTER_BASE + CE_DESTINATION_WRITE_INDEX,
            self.destination.write_index() as u32,
        )?;
        self.source.enqueue(source)?;
        registers.write32(
            CE7_REGISTER_BASE + CE_SOURCE_WRITE_INDEX,
            self.source.write_index() as u32,
        )?;

        let sent = wait_for_completion(
            registers,
            CE7_REGISTER_BASE + CE_SOURCE_READ_INDEX,
            &mut self.source,
        )?;
        if sent.address() != source_address || usize::from(sent.byte_len()) != length {
            return Err(CeError::InvalidResponseLength);
        }
        let received = wait_for_completion(
            registers,
            CE7_REGISTER_BASE + CE_DESTINATION_READ_INDEX,
            &mut self.destination,
        )?;
        if received.address() != destination_address || usize::from(received.byte_len()) != length {
            return Err(CeError::InvalidResponseLength);
        }

        self.poisoned = false;
        Ok(())
    }
}

impl<'a, R: RegisterIo> BmiCeTransport<'a, R> {
    /// # Safety
    /// The rings must already be configured, PCI bus mastering must be enabled,
    /// and the device and DMA pages must remain valid for the transport lifetime.
    pub unsafe fn new(rings: &'a mut BmiCeRings, registers: &'a mut R) -> Self {
        Self { rings, registers }
    }
}

impl<R: RegisterIo> BmiTransport for BmiCeTransport<'_, R> {
    type Error = CeError;

    fn exchange(
        &mut self,
        request: &[u8],
        response: Option<&mut [u8]>,
    ) -> Result<usize, Self::Error> {
        unsafe { self.rings.exchange(self.registers, request, response) }
    }
}

pub struct PacketCe {
    pipe_index: usize,
    max_packet_bytes: usize,
    source: DescriptorRing,
    destination: DescriptorRing,
    receive_buffer: DmaPage,
    transmit_buffer: DmaPage,
    poisoned: bool,
}

impl PacketCe {
    pub fn allocate(pipe_index: usize, max_packet_bytes: usize) -> Result<Self, CeError> {
        if pipe_index >= CE_PIPE_COUNT {
            return Err(CeError::InvalidPipe);
        }
        if max_packet_bytes == 0 || max_packet_bytes > CE_PACKET_MAX_BYTES {
            return Err(CeError::TransferTooLarge);
        }
        Ok(Self {
            pipe_index,
            max_packet_bytes,
            source: DescriptorRing::allocate(CE_PACKET_RING_ENTRIES)?,
            destination: DescriptorRing::allocate(CE_PACKET_RING_ENTRIES)?,
            receive_buffer: DmaPage::allocate()?,
            transmit_buffer: DmaPage::allocate()?,
            poisoned: false,
        })
    }

    fn register_base(&self) -> u64 {
        CE0_REGISTER_BASE + self.pipe_index as u64 * CE_REGISTER_STRIDE
    }

    /// Configure one CE as a polled bidirectional packet pipe.
    ///
    /// # Safety
    /// The device must be awake and exclusively owned, PCI bus mastering must
    /// be disabled, and no agent may access this CE. Keep `self` alive until
    /// DMA stops and its register references are cleared.
    pub unsafe fn configure(&mut self, registers: &mut impl RegisterIo) -> Result<(), CeError> {
        configure_packet_registers(
            registers,
            self.register_base(),
            self.source.physical_address(),
            self.destination.physical_address(),
            self.source.entry_count(),
            self.destination.entry_count(),
            self.max_packet_bytes,
        )?;
        let descriptor = Descriptor::new(u64::from(self.receive_buffer.physical_address()), 0, 0)?;
        self.destination.enqueue(descriptor)?;
        registers.write32(
            self.register_base() + CE_DESTINATION_WRITE_INDEX,
            self.destination.write_index() as u32,
        )
    }

    /// Clear this CE's ring registers before releasing its DMA pages.
    ///
    /// # Safety
    /// PCI bus mastering must be disabled and no transfer may be in flight.
    pub unsafe fn deconfigure(&mut self, registers: &mut impl RegisterIo) -> Result<(), CeError> {
        if self.poisoned || self.source.read_index() != self.source.write_index() {
            return Err(CeError::TransportPoisoned);
        }
        clear_pipe_registers(registers, self.register_base())
    }

    /// Send one packet and wait for its source descriptor to complete.
    ///
    /// # Safety
    /// `configure` must have completed, PCI bus mastering must be enabled,
    /// and this object must remain alive while the device can access its pages.
    pub unsafe fn send(
        &mut self,
        registers: &mut impl RegisterIo,
        packet: &[u8],
    ) -> Result<(), CeError> {
        if self.poisoned {
            return Err(CeError::TransportPoisoned);
        }
        if packet.is_empty() || packet.len() > self.max_packet_bytes {
            return Err(CeError::TransferTooLarge);
        }

        self.transmit_buffer.write(packet);
        let descriptor = Descriptor::new(
            u64::from(self.transmit_buffer.physical_address()),
            packet.len(),
            0,
        )?;
        self.source.enqueue(descriptor)?;
        self.poisoned = true;
        registers.write32(
            self.register_base() + CE_SOURCE_WRITE_INDEX,
            self.source.write_index() as u32,
        )?;

        let sent = wait_for_completion(
            registers,
            self.register_base() + CE_SOURCE_READ_INDEX,
            &mut self.source,
        )
        .map_err(|error| completion_error(error, CeError::SourceTimeout))?;
        if sent.address() != self.transmit_buffer.physical_address()
            || usize::from(sent.byte_len()) != packet.len()
        {
            return Err(CeError::InvalidResponseLength);
        }
        self.poisoned = false;
        Ok(())
    }

    /// Receive one posted packet and immediately repost the receive buffer.
    ///
    /// # Safety
    /// `configure` must have completed, PCI bus mastering must be enabled,
    /// and this object must remain alive while the device can access its pages.
    pub unsafe fn receive(
        &mut self,
        registers: &mut impl RegisterIo,
        output: &mut [u8],
    ) -> Result<usize, CeError> {
        if self.poisoned {
            return Err(CeError::TransportPoisoned);
        }
        self.poisoned = true;
        let received = wait_for_completion(
            registers,
            self.register_base() + CE_DESTINATION_READ_INDEX,
            &mut self.destination,
        )
        .map_err(|error| completion_error(error, CeError::DestinationTimeout))?;
        let received_length = usize::from(received.byte_len());
        let response_error = if received.address() != self.receive_buffer.physical_address()
            || received_length == 0
            || received_length > self.max_packet_bytes
        {
            Some(CeError::InvalidResponseLength)
        } else if output.len() < received_length {
            Some(CeError::ResponseBufferTooSmall)
        } else {
            self.receive_buffer.read(&mut output[..received_length]);
            None
        };

        let descriptor = Descriptor::new(u64::from(self.receive_buffer.physical_address()), 0, 0)?;
        self.destination.enqueue(descriptor)?;
        registers.write32(
            self.register_base() + CE_DESTINATION_WRITE_INDEX,
            self.destination.write_index() as u32,
        )?;
        self.poisoned = false;

        response_error.map_or(Ok(received_length), Err)
    }
}

pub struct PacketCeTransport<'a, R: RegisterIo> {
    pipe: &'a mut PacketCe,
    registers: &'a mut R,
}

impl<'a, R: RegisterIo> PacketCeTransport<'a, R> {
    /// # Safety
    /// The pipe must be configured, PCI bus mastering enabled, and both the
    /// registers and DMA pages must remain valid for this transport lifetime.
    pub unsafe fn new(pipe: &'a mut PacketCe, registers: &'a mut R) -> Self {
        Self { pipe, registers }
    }
}

impl<R: RegisterIo> HtcTransport for PacketCeTransport<'_, R> {
    type Error = CeError;

    fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error> {
        unsafe { self.pipe.send(self.registers, frame) }
    }

    fn receive(&mut self, frame: &mut [u8]) -> Result<usize, Self::Error> {
        unsafe { self.pipe.receive(self.registers, frame) }
    }
}

fn validate_bmi_request(request: &[u8]) -> Result<(), CeError> {
    if request.is_empty() || request.len() > CE_BMI_SOURCE_MAX_BYTES || request.len() & 3 != 0 {
        return Err(CeError::InvalidRequest);
    }
    Ok(())
}

fn wait_for_completion(
    registers: &mut impl RegisterIo,
    read_index_register: u64,
    ring: &mut DescriptorRing,
) -> Result<Descriptor, CeError> {
    for _ in 0..CE_POLL_LIMIT {
        let hardware_index = registers.read32(read_index_register)?;
        if hardware_index == u32::MAX {
            return Err(CeError::DeviceUnavailable);
        }
        if let Some(descriptor) = ring.complete(hardware_index as usize)? {
            return Ok(descriptor);
        }
        core::hint::spin_loop();
    }
    Err(CeError::Timeout)
}

fn completion_error(error: CeError, timeout_error: CeError) -> CeError {
    if error == CeError::Timeout {
        timeout_error
    } else {
        error
    }
}

fn configure_bmi_registers(
    registers: &mut impl RegisterIo,
    source_ring_address: u32,
    destination_ring_address: u32,
) -> Result<(), CeError> {
    let source_ring_bytes = CE0_SOURCE_RING_ENTRIES * core::mem::size_of::<Descriptor>();
    let destination_ring_bytes = CE1_DESTINATION_RING_ENTRIES * core::mem::size_of::<Descriptor>();
    if source_ring_address & 7 != 0 || destination_ring_address & 7 != 0 {
        return Err(CeError::DmaAddressMisaligned);
    }
    if !is_dma32_range(u64::from(source_ring_address), source_ring_bytes)
        || !is_dma32_range(u64::from(destination_ring_address), destination_ring_bytes)
    {
        return Err(CeError::DmaAddressOutOfRange);
    }

    let indices = [
        registers.read32(CE0_REGISTER_BASE + CE_SOURCE_READ_INDEX)?,
        registers.read32(CE0_REGISTER_BASE + CE_SOURCE_WRITE_INDEX)?,
        registers.read32(CE1_REGISTER_BASE + CE_DESTINATION_READ_INDEX)?,
        registers.read32(CE1_REGISTER_BASE + CE_DESTINATION_WRITE_INDEX)?,
    ];
    if indices.iter().any(|index| *index == u32::MAX) {
        return Err(CeError::DeviceUnavailable);
    }
    if indices.iter().any(|index| *index != 0) {
        return Err(CeError::EngineNotIdle);
    }
    registers.read32(CE_REGISTER_WINDOW_END - 4)?;

    registers.write32(CE0_REGISTER_BASE + CE_SOURCE_RING_BASE, source_ring_address)?;
    registers.write32(
        CE0_REGISTER_BASE + CE_SOURCE_RING_SIZE,
        CE0_SOURCE_RING_ENTRIES as u32,
    )?;
    let control = registers.read32(CE0_REGISTER_BASE + CE_CONTROL)?;
    let control = (control
        & !(CE_CONTROL_DMAX_MASK | CE_CONTROL_SOURCE_BYTE_SWAP | CE_CONTROL_DESTINATION_BYTE_SWAP))
        | CE_BMI_SOURCE_MAX_BYTES as u32;
    registers.write32(CE0_REGISTER_BASE + CE_CONTROL, control)?;
    registers.write32(CE0_REGISTER_BASE + CE_HOST_INTERRUPT_ENABLE, 0)?;
    registers.write32(CE0_REGISTER_BASE + CE_MISC_INTERRUPT_ENABLE, 0)?;
    registers.write32(
        CE0_REGISTER_BASE + CE_SOURCE_WATERMARK,
        CE0_SOURCE_RING_ENTRIES as u32,
    )?;

    registers.write32(
        CE1_REGISTER_BASE + CE_DESTINATION_RING_BASE,
        destination_ring_address,
    )?;
    registers.write32(
        CE1_REGISTER_BASE + CE_DESTINATION_RING_SIZE,
        CE1_DESTINATION_RING_ENTRIES as u32,
    )?;
    let control = registers.read32(CE1_REGISTER_BASE + CE_CONTROL)?;
    registers.write32(
        CE1_REGISTER_BASE + CE_CONTROL,
        control & !CE_CONTROL_DESTINATION_BYTE_SWAP,
    )?;
    registers.write32(CE1_REGISTER_BASE + CE_HOST_INTERRUPT_ENABLE, 0)?;
    registers.write32(CE1_REGISTER_BASE + CE_MISC_INTERRUPT_ENABLE, 0)?;
    registers.write32(
        CE1_REGISTER_BASE + CE_DESTINATION_WATERMARK,
        CE1_DESTINATION_RING_ENTRIES as u32,
    )?;
    registers.write32(CE1_REGISTER_BASE + CE_DESTINATION_WRITE_INDEX, 1)?;
    Ok(())
}

fn configure_diagnostic_registers(
    registers: &mut impl RegisterIo,
    source_ring_address: u32,
    destination_ring_address: u32,
) -> Result<(), CeError> {
    let ring_bytes = CE_DIAGNOSTIC_RING_ENTRIES * core::mem::size_of::<Descriptor>();
    if source_ring_address & 7 != 0 || destination_ring_address & 7 != 0 {
        return Err(CeError::DmaAddressMisaligned);
    }
    if !is_dma32_range(u64::from(source_ring_address), ring_bytes)
        || !is_dma32_range(u64::from(destination_ring_address), ring_bytes)
    {
        return Err(CeError::DmaAddressOutOfRange);
    }

    let indices = [
        registers.read32(CE7_REGISTER_BASE + CE_SOURCE_READ_INDEX)?,
        registers.read32(CE7_REGISTER_BASE + CE_SOURCE_WRITE_INDEX)?,
        registers.read32(CE7_REGISTER_BASE + CE_DESTINATION_READ_INDEX)?,
        registers.read32(CE7_REGISTER_BASE + CE_DESTINATION_WRITE_INDEX)?,
    ];
    if indices.iter().any(|index| *index == u32::MAX) {
        return Err(CeError::DeviceUnavailable);
    }
    if indices.iter().any(|index| *index != 0) {
        return Err(CeError::EngineNotIdle);
    }

    registers.write32(CE7_REGISTER_BASE + CE_SOURCE_RING_BASE, source_ring_address)?;
    registers.write32(
        CE7_REGISTER_BASE + CE_SOURCE_RING_SIZE,
        CE_DIAGNOSTIC_RING_ENTRIES as u32,
    )?;
    let control = registers.read32(CE7_REGISTER_BASE + CE_CONTROL)?;
    let control = (control
        & !(CE_CONTROL_DMAX_MASK | CE_CONTROL_SOURCE_BYTE_SWAP | CE_CONTROL_DESTINATION_BYTE_SWAP))
        | CE_DIAGNOSTIC_MAX_BYTES as u32;
    registers.write32(CE7_REGISTER_BASE + CE_CONTROL, control)?;
    registers.write32(CE7_REGISTER_BASE + CE_HOST_INTERRUPT_ENABLE, 0)?;
    registers.write32(CE7_REGISTER_BASE + CE_MISC_INTERRUPT_ENABLE, 0)?;
    registers.write32(
        CE7_REGISTER_BASE + CE_SOURCE_WATERMARK,
        CE_DIAGNOSTIC_RING_ENTRIES as u32,
    )?;

    registers.write32(
        CE7_REGISTER_BASE + CE_DESTINATION_RING_BASE,
        destination_ring_address,
    )?;
    registers.write32(
        CE7_REGISTER_BASE + CE_DESTINATION_RING_SIZE,
        CE_DIAGNOSTIC_RING_ENTRIES as u32,
    )?;
    registers.write32(
        CE7_REGISTER_BASE + CE_CONTROL,
        control & !CE_CONTROL_DESTINATION_BYTE_SWAP,
    )?;
    registers.write32(
        CE7_REGISTER_BASE + CE_DESTINATION_WATERMARK,
        CE_DIAGNOSTIC_RING_ENTRIES as u32,
    )?;
    Ok(())
}

fn configure_packet_registers(
    registers: &mut impl RegisterIo,
    base: u64,
    source_ring_address: u32,
    destination_ring_address: u32,
    source_entries: usize,
    destination_entries: usize,
    max_packet_bytes: usize,
) -> Result<(), CeError> {
    if base < CE0_REGISTER_BASE
        || base > CE7_REGISTER_BASE
        || (base - CE0_REGISTER_BASE) % CE_REGISTER_STRIDE != 0
    {
        return Err(CeError::InvalidPipe);
    }
    if max_packet_bytes == 0 || max_packet_bytes > CE_PACKET_MAX_BYTES {
        return Err(CeError::TransferTooLarge);
    }

    let ring_bytes = |entries: usize| {
        if entries < 2 || !entries.is_power_of_two() {
            return Err(CeError::InvalidRingSize);
        }
        entries
            .checked_mul(core::mem::size_of::<Descriptor>())
            .filter(|bytes| *bytes <= DMA_PAGE_SIZE)
            .ok_or(CeError::InvalidRingSize)
    };
    let source_ring_bytes = ring_bytes(source_entries)?;
    let destination_ring_bytes = ring_bytes(destination_entries)?;

    if source_ring_address & 7 != 0 || destination_ring_address & 7 != 0 {
        return Err(CeError::DmaAddressMisaligned);
    }
    if !is_dma32_range(u64::from(source_ring_address), source_ring_bytes)
        || !is_dma32_range(
            u64::from(destination_ring_address),
            destination_ring_bytes,
        )
    {
        return Err(CeError::DmaAddressOutOfRange);
    }

    let indices = [
        registers.read32(base + CE_SOURCE_READ_INDEX)?,
        registers.read32(base + CE_SOURCE_WRITE_INDEX)?,
        registers.read32(base + CE_DESTINATION_READ_INDEX)?,
        registers.read32(base + CE_DESTINATION_WRITE_INDEX)?,
    ];
    if indices.iter().any(|index| *index == u32::MAX) {
        return Err(CeError::DeviceUnavailable);
    }
    if indices.iter().any(|index| *index != 0) {
        return Err(CeError::EngineNotIdle);
    }

    registers.write32(base + CE_SOURCE_RING_BASE, source_ring_address)?;
    registers.write32(base + CE_SOURCE_RING_SIZE, source_entries as u32)?;
    registers.write32(
        base + CE_DESTINATION_RING_BASE,
        destination_ring_address,
    )?;
    registers.write32(
        base + CE_DESTINATION_RING_SIZE,
        destination_entries as u32,
    )?;
    let control = registers.read32(base + CE_CONTROL)?;
    let control = (control
        & !(CE_CONTROL_DMAX_MASK
            | CE_CONTROL_SOURCE_BYTE_SWAP
            | CE_CONTROL_DESTINATION_BYTE_SWAP))
        | max_packet_bytes as u32;
    registers.write32(base + CE_CONTROL, control)?;
    registers.write32(base + CE_HOST_INTERRUPT_ENABLE, 0)?;
    registers.write32(base + CE_MISC_INTERRUPT_ENABLE, 0)?;
    registers.write32(base + CE_SOURCE_WATERMARK, source_entries as u32)?;
    registers.write32(
        base + CE_DESTINATION_WATERMARK,
        destination_entries as u32,
    )?;
    Ok(())
}

fn clear_bmi_registers(registers: &mut impl RegisterIo) -> Result<(), CeError> {
    clear_registers(
        registers,
        &[
            (CE1_REGISTER_BASE + CE_DESTINATION_WRITE_INDEX, 0),
            (CE0_REGISTER_BASE + CE_SOURCE_WRITE_INDEX, 0),
            (CE1_REGISTER_BASE + CE_DESTINATION_RING_SIZE, 0),
            (CE1_REGISTER_BASE + CE_DESTINATION_RING_BASE, 0),
            (CE0_REGISTER_BASE + CE_SOURCE_RING_SIZE, 0),
            (CE0_REGISTER_BASE + CE_SOURCE_RING_BASE, 0),
        ],
    )
}

fn clear_pipe_registers(registers: &mut impl RegisterIo, base: u64) -> Result<(), CeError> {
    clear_registers(
        registers,
        &[
            (base + CE_DESTINATION_WRITE_INDEX, 0),
            (base + CE_SOURCE_WRITE_INDEX, 0),
            (base + CE_DESTINATION_RING_SIZE, 0),
            (base + CE_DESTINATION_RING_BASE, 0),
            (base + CE_SOURCE_RING_SIZE, 0),
            (base + CE_SOURCE_RING_BASE, 0),
        ],
    )
}

fn clear_registers(
    registers: &mut impl RegisterIo,
    registers_to_clear: &[(u64, u32)],
) -> Result<(), CeError> {
    for (offset, value) in registers_to_clear {
        registers.write32(*offset, *value)?;
    }
    for (offset, _) in registers_to_clear {
        let value = registers.read32(*offset)?;
        if value == u32::MAX {
            return Err(CeError::DeviceUnavailable);
        }
        if value != 0 {
            return Err(CeError::EngineNotIdle);
        }
    }

    Ok(())
}

fn validate_diagnostic_transfer(address: u32, length: usize) -> Result<(), CeError> {
    if length == 0 || length > CE_DIAGNOSTIC_MAX_BYTES || length & 3 != 0 {
        return Err(CeError::TransferTooLarge);
    }
    if address & 3 != 0 {
        return Err(CeError::DmaAddressMisaligned);
    }
    let length = u32::try_from(length).map_err(|_| CeError::TransferTooLarge)?;
    address
        .checked_add(length)
        .ok_or(CeError::DmaAddressOverflow)?;
    if (address & 0x000f_ffff) + length > 0x0010_0000 {
        return Err(CeError::InvalidRequest);
    }
    Ok(())
}

fn target_cpu_to_ce_address(core_ctrl: u32, address: u32) -> u32 {
    ((core_ctrl & 0x7ff) << 21)
        | (if address >= 0x0010_0000 {
            0x0010_0000
        } else {
            0
        })
        | (address & 0x000f_ffff)
}

pub trait RegisterIo {
    fn read32(&mut self, offset: u64) -> Result<u32, CeError>;
    fn write32(&mut self, offset: u64, value: u32) -> Result<(), CeError>;
}

pub struct MmioWindow {
    base: NonNull<u8>,
    length: u64,
}

impl MmioWindow {
    /// # Safety
    /// `base..base+length` must be a live, mapped PCI MMIO region.
    pub unsafe fn new(base: usize, length: u64) -> Result<Self, CeError> {
        let base = NonNull::new(base as *mut u8).ok_or(CeError::RegisterOutOfRange)?;
        if length < CE_REGISTER_WINDOW_END {
            return Err(CeError::RegisterOutOfRange);
        }
        Ok(Self { base, length })
    }

    fn register(&self, offset: u64) -> Result<*mut u32, CeError> {
        if offset & 3 != 0 || offset.checked_add(4).is_none_or(|end| end > self.length) {
            return Err(CeError::RegisterOutOfRange);
        }
        let offset = usize::try_from(offset).map_err(|_| CeError::RegisterOutOfRange)?;
        Ok(unsafe { self.base.as_ptr().add(offset).cast::<u32>() })
    }
}

impl RegisterIo for MmioWindow {
    fn read32(&mut self, offset: u64) -> Result<u32, CeError> {
        let register = self.register(offset)?;
        Ok(u32::from_le(unsafe { register.read_volatile() }))
    }

    fn write32(&mut self, offset: u64, value: u32) -> Result<(), CeError> {
        let register = self.register(offset)?;
        unsafe { register.write_volatile(value.to_le()) };
        Ok(())
    }
}

pub struct RingIndices {
    mask: usize,
    read: usize,
    write: usize,
}

impl RingIndices {
    pub const fn new(entries: usize) -> Result<Self, CeError> {
        if entries < 2 || !entries.is_power_of_two() {
            return Err(CeError::InvalidRingSize);
        }
        Ok(Self {
            mask: entries - 1,
            read: 0,
            write: 0,
        })
    }

    pub const fn read_index(&self) -> usize {
        self.read
    }

    pub const fn write_index(&self) -> usize {
        self.write
    }

    const fn has_free_entry(&self) -> bool {
        self.next(self.write) != self.read
    }

    pub fn enqueue(&mut self) -> Result<usize, CeError> {
        let next = self.next(self.write);
        if next == self.read {
            return Err(CeError::RingFull);
        }
        let index = self.write;
        self.write = next;
        Ok(index)
    }

    pub fn take_completed(&mut self, hardware_read_index: usize) -> Result<Option<usize>, CeError> {
        if hardware_read_index > self.mask {
            return Err(CeError::InvalidHardwareIndex);
        }
        let pending = self.write.wrapping_sub(self.read) & self.mask;
        let completed = hardware_read_index.wrapping_sub(self.read) & self.mask;
        if completed > pending {
            return Err(CeError::InvalidHardwareIndex);
        }
        if completed == 0 {
            return Ok(None);
        }
        let index = self.read;
        self.read = self.next(self.read);
        Ok(Some(index))
    }

    const fn next(&self, index: usize) -> usize {
        (index + 1) & self.mask
    }
}

fn is_dma32_range(address: u64, length: usize) -> bool {
    let Ok(length) = u64::try_from(length) else {
        return false;
    };
    address
        .checked_add(length)
        .is_some_and(|end| address <= u64::from(u32::MAX) && end <= 1_u64 << 32)
}

#[cfg(test)]
mod tests {
    use super::{
        CE_BMI_DESTINATION_BUFFER_BYTES, CE_BMI_SOURCE_MAX_BYTES, CE_CONTROL,
        CE_DESTINATION_RING_BASE, CE_DESTINATION_RING_SIZE, CE_DESTINATION_WATERMARK,
        CE_DESTINATION_WRITE_INDEX, CE_DIAGNOSTIC_MAX_BYTES, CE_DIAGNOSTIC_RING_ENTRIES,
        CE_HOST_INTERRUPT_ENABLE, CE_MISC_INTERRUPT_ENABLE, CE_SOURCE_READ_INDEX,
        CE_SOURCE_RING_BASE, CE_SOURCE_RING_SIZE, CE_SOURCE_WATERMARK, CE0_REGISTER_BASE,
        CE0_SOURCE_RING_ENTRIES, CE1_DESTINATION_RING_ENTRIES, CE1_REGISTER_BASE,
        CE7_REGISTER_BASE, CE_PACKET_MAX_BYTES, CE_PACKET_RING_ENTRIES, CeError, Descriptor,
        RegisterIo, RingIndices, clear_bmi_registers, clear_pipe_registers, completion_error,
        configure_bmi_registers, configure_packet_registers,
        configure_diagnostic_registers, is_dma32_range, target_cpu_to_ce_address,
        validate_bmi_request, validate_diagnostic_transfer,
    };
    use core::mem::size_of;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct MockRegisters {
        values: BTreeMap<u64, u32>,
        writes: Vec<(u64, u32)>,
    }

    impl RegisterIo for MockRegisters {
        fn read32(&mut self, offset: u64) -> Result<u32, CeError> {
            Ok(*self.values.get(&offset).unwrap_or(&0))
        }

        fn write32(&mut self, offset: u64, value: u32) -> Result<(), CeError> {
            self.values.insert(offset, value);
            self.writes.push((offset, value));
            Ok(())
        }
    }

    #[test]
    fn descriptor_matches_legacy_qca_layout_and_accepts_dma32_boundary() {
        assert_eq!(size_of::<Descriptor>(), 8);
        let descriptor = Descriptor::new(0xffff_fffc, 4, 0x12).unwrap();
        assert_eq!(descriptor.address(), 0xffff_fffc);
        assert_eq!(descriptor.byte_len(), 4);
        assert_eq!(descriptor.flags(), 0x12);
        assert_eq!(
            Descriptor::new(0xffff_fffc, 5, 0),
            Err(CeError::DmaAddressOutOfRange)
        );
    }

    #[test]
    fn dma_range_must_fit_below_four_gibibytes() {
        assert!(is_dma32_range(0xffff_f000, 4096));
        assert!(!is_dma32_range(0xffff_f000, 4097));
        assert!(!is_dma32_range(1_u64 << 32, 1));
    }

    #[test]
    fn descriptor_rejects_unrepresentable_lengths_and_addresses() {
        assert_eq!(
            Descriptor::new(0, usize::from(u16::MAX) + 1, 0),
            Err(CeError::TransferTooLarge)
        );
        assert_eq!(
            Descriptor::new(1_u64 << 32, 1, 0),
            Err(CeError::DmaAddressOutOfRange)
        );
        assert_eq!(Descriptor::new(1, 1, 0), Err(CeError::DmaAddressMisaligned));
    }

    #[test]
    fn ring_keeps_one_slot_empty_and_wraps_indices() {
        let mut ring = RingIndices::new(4).unwrap();
        assert_eq!(ring.enqueue(), Ok(0));
        assert_eq!(ring.take_completed(2), Err(CeError::InvalidHardwareIndex));
        assert_eq!(ring.enqueue(), Ok(1));
        assert_eq!(ring.enqueue(), Ok(2));
        assert_eq!(ring.enqueue(), Err(CeError::RingFull));
        assert_eq!(ring.take_completed(1), Ok(Some(0)));
        assert_eq!(ring.enqueue(), Ok(3));
        assert_eq!(ring.write_index(), 0);
        assert_eq!(ring.take_completed(2), Ok(Some(1)));
        assert_eq!(ring.take_completed(0), Ok(Some(2)));
        assert_eq!(ring.take_completed(0), Ok(Some(3)));
        assert_eq!(ring.take_completed(0), Ok(None));
        assert_eq!(ring.take_completed(4), Err(CeError::InvalidHardwareIndex));
    }

    #[test]
    fn ring_requires_a_power_of_two_entry_count() {
        assert!(RingIndices::new(1).is_err());
        assert!(RingIndices::new(3).is_err());
        assert!(RingIndices::new(16).is_ok());
    }

    #[test]
    fn bmi_ring_shapes_fit_in_one_dma_page() {
        assert!(RingIndices::new(CE0_SOURCE_RING_ENTRIES).is_ok());
        assert!(RingIndices::new(CE1_DESTINATION_RING_ENTRIES).is_ok());
        assert_eq!(size_of::<Descriptor>() * CE1_DESTINATION_RING_ENTRIES, 4096);
        assert!(CE_BMI_DESTINATION_BUFFER_BYTES <= 4096);
    }

    #[test]
    fn configures_bmi_rings_with_polling_and_posts_rx_buffer_last() {
        let mut registers = MockRegisters::default();
        let ce0_control = 0xabcd_0000;
        let ce1_control = 0x0003_1234;
        registers
            .values
            .insert(CE0_REGISTER_BASE + CE_CONTROL, ce0_control);
        registers
            .values
            .insert(CE1_REGISTER_BASE + CE_CONTROL, ce1_control);

        configure_bmi_registers(&mut registers, 0x1000, 0x2000).unwrap();

        assert_eq!(
            registers.values[&(CE0_REGISTER_BASE + CE_SOURCE_RING_BASE)],
            0x1000
        );
        assert_eq!(
            registers.values[&(CE0_REGISTER_BASE + CE_SOURCE_RING_SIZE)],
            CE0_SOURCE_RING_ENTRIES as u32
        );
        assert_eq!(
            registers.values[&(CE0_REGISTER_BASE + CE_CONTROL)],
            (ce0_control & !0x0003_ffff) | CE_BMI_SOURCE_MAX_BYTES as u32
        );
        assert_eq!(
            registers.values[&(CE0_REGISTER_BASE + CE_SOURCE_WATERMARK)],
            CE0_SOURCE_RING_ENTRIES as u32
        );
        assert_eq!(
            registers.values[&(CE1_REGISTER_BASE + CE_DESTINATION_RING_BASE)],
            0x2000
        );
        assert_eq!(
            registers.values[&(CE1_REGISTER_BASE + CE_DESTINATION_RING_SIZE)],
            CE1_DESTINATION_RING_ENTRIES as u32
        );
        assert_eq!(
            registers.values[&(CE1_REGISTER_BASE + CE_CONTROL)],
            ce1_control & !(1 << 17)
        );
        assert_eq!(
            registers.values[&(CE1_REGISTER_BASE + CE_DESTINATION_WATERMARK)],
            CE1_DESTINATION_RING_ENTRIES as u32
        );
        assert_eq!(
            registers.values[&(CE1_REGISTER_BASE + CE_HOST_INTERRUPT_ENABLE)],
            0
        );
        assert_eq!(
            registers.values[&(CE1_REGISTER_BASE + CE_MISC_INTERRUPT_ENABLE)],
            0
        );
        assert_eq!(
            registers.writes.last(),
            Some(&(CE1_REGISTER_BASE + CE_DESTINATION_WRITE_INDEX, 1))
        );
    }

    #[test]
    fn configures_ce7_for_polled_diagnostic_transfers() {
        let mut registers = MockRegisters::default();
        let control = 0xabcd_0000;
        registers
            .values
            .insert(CE7_REGISTER_BASE + CE_CONTROL, control);

        configure_diagnostic_registers(&mut registers, 0x1000, 0x2000).unwrap();

        assert_eq!(
            registers.values[&(CE7_REGISTER_BASE + CE_SOURCE_RING_BASE)],
            0x1000
        );
        assert_eq!(
            registers.values[&(CE7_REGISTER_BASE + CE_SOURCE_RING_SIZE)],
            CE_DIAGNOSTIC_RING_ENTRIES as u32
        );
        assert_eq!(
            registers.values[&(CE7_REGISTER_BASE + CE_DESTINATION_RING_BASE)],
            0x2000
        );
        assert_eq!(
            registers.values[&(CE7_REGISTER_BASE + CE_DESTINATION_RING_SIZE)],
            CE_DIAGNOSTIC_RING_ENTRIES as u32
        );
        assert_eq!(
            registers.values[&(CE7_REGISTER_BASE + CE_CONTROL)],
            (control & !0x0003_ffff) | CE_DIAGNOSTIC_MAX_BYTES as u32
        );
        assert_eq!(
            registers.values[&(CE7_REGISTER_BASE + CE_HOST_INTERRUPT_ENABLE)],
            0
        );
        assert_eq!(
            registers.values[&(CE7_REGISTER_BASE + CE_MISC_INTERRUPT_ENABLE)],
            0
        );
    }

    #[test]
    fn configures_packet_pipe_rings_and_rejects_bad_pipe_sizes() {
        let base = CE0_REGISTER_BASE + 2 * super::CE_REGISTER_STRIDE;
        let mut registers = MockRegisters::default();
        registers.values.insert(base + CE_CONTROL, 0xabcd_0000);

        configure_packet_registers(
            &mut registers,
            base,
            0x1000,
            0x2000,
            CE_PACKET_RING_ENTRIES,
            CE_PACKET_RING_ENTRIES,
            CE_PACKET_MAX_BYTES,
        )
        .unwrap();

        assert_eq!(
            registers.values[&(base + CE_SOURCE_RING_SIZE)],
            CE_PACKET_RING_ENTRIES as u32
        );
        assert_eq!(
            registers.values[&(base + CE_DESTINATION_RING_SIZE)],
            CE_PACKET_RING_ENTRIES as u32
        );
        assert_eq!(
            registers.values[&(base + CE_CONTROL)],
            0xabcc_0000 | CE_PACKET_MAX_BYTES as u32
        );
        assert_eq!(
            registers.values[&(base + CE_SOURCE_WATERMARK)],
            CE_PACKET_RING_ENTRIES as u32
        );
        assert_eq!(
            registers.values[&(base + CE_DESTINATION_WATERMARK)],
            CE_PACKET_RING_ENTRIES as u32
        );
        assert_eq!(
            registers.values[&(base + CE_HOST_INTERRUPT_ENABLE)],
            0
        );
        assert_eq!(
            registers.values[&(base + CE_MISC_INTERRUPT_ENABLE)],
            0
        );

        let mut registers = MockRegisters::default();
        assert_eq!(
            configure_packet_registers(
                &mut registers,
                base,
                0x1000,
                0x2000,
                CE_PACKET_RING_ENTRIES,
                CE_PACKET_RING_ENTRIES,
                CE_PACKET_MAX_BYTES + 1,
            ),
            Err(CeError::TransferTooLarge)
        );
        assert!(registers.writes.is_empty());
    }

    #[test]
    fn clears_only_the_selected_diagnostic_pipe_registers() {
        let mut registers = MockRegisters::default();
        clear_pipe_registers(&mut registers, CE7_REGISTER_BASE).unwrap();

        assert_eq!(registers.writes.len(), 6);
        assert!(
            registers
                .writes
                .iter()
                .all(|(offset, value)| *offset >= CE7_REGISTER_BASE && *value == 0)
        );
    }

    #[test]
    fn validates_diagnostic_transfer_alignment_and_size() {
        assert_eq!(validate_diagnostic_transfer(0x1000, 4), Ok(()));
        assert_eq!(
            validate_diagnostic_transfer(0x1001, 4),
            Err(CeError::DmaAddressMisaligned)
        );
        assert_eq!(
            validate_diagnostic_transfer(0x1000, CE_DIAGNOSTIC_MAX_BYTES + 4),
            Err(CeError::TransferTooLarge)
        );
        assert_eq!(
            validate_diagnostic_transfer(0xffff_fffc, 4),
            Err(CeError::DmaAddressOverflow)
        );
        assert_eq!(
            validate_diagnostic_transfer(0x000f_fffc, 8),
            Err(CeError::InvalidRequest)
        );
    }

    #[test]
    fn maps_target_cpu_addresses_into_qca6174_ce_space() {
        assert_eq!(target_cpu_to_ce_address(0x7ab, 0x1234), 0xf560_1234);
        assert_eq!(target_cpu_to_ce_address(0x7ab, 0x0010_1234), 0xf570_1234);
    }

    #[test]
    fn clears_bmi_ring_registers_and_verifies_the_readback() {
        let mut registers = MockRegisters::default();
        for offset in [
            CE1_REGISTER_BASE + CE_DESTINATION_WRITE_INDEX,
            CE0_REGISTER_BASE + super::CE_SOURCE_WRITE_INDEX,
            CE1_REGISTER_BASE + CE_DESTINATION_RING_SIZE,
            CE1_REGISTER_BASE + CE_DESTINATION_RING_BASE,
            CE0_REGISTER_BASE + CE_SOURCE_RING_SIZE,
            CE0_REGISTER_BASE + CE_SOURCE_RING_BASE,
        ] {
            registers.values.insert(offset, 1);
        }

        clear_bmi_registers(&mut registers).unwrap();

        assert_eq!(registers.writes.len(), 6);
        assert!(registers.writes.iter().all(|(_, value)| *value == 0));
        assert!(registers.values.values().all(|value| *value == 0));
    }

    #[test]
    fn refuses_busy_or_missing_engines_before_programming_rings() {
        let mut registers = MockRegisters::default();
        registers
            .values
            .insert(CE0_REGISTER_BASE + CE_SOURCE_READ_INDEX, 1);
        assert_eq!(
            configure_bmi_registers(&mut registers, 0x1000, 0x2000),
            Err(CeError::EngineNotIdle)
        );
        assert!(registers.writes.is_empty());

        registers
            .values
            .insert(CE0_REGISTER_BASE + CE_SOURCE_READ_INDEX, u32::MAX);
        assert_eq!(
            configure_bmi_registers(&mut registers, 0x1000, 0x2000),
            Err(CeError::DeviceUnavailable)
        );
        assert!(registers.writes.is_empty());
    }

    #[test]
    fn refuses_misaligned_or_out_of_range_ring_pages_before_mmio() {
        let mut registers = MockRegisters::default();
        assert_eq!(
            configure_bmi_registers(&mut registers, 0x1004, 0x2000),
            Err(CeError::DmaAddressMisaligned)
        );
        assert_eq!(
            configure_bmi_registers(&mut registers, 0xffff_fff8, 0x2000),
            Err(CeError::DmaAddressOutOfRange)
        );
        assert!(registers.writes.is_empty());
    }

    #[test]
    fn bmi_requests_must_fit_the_ce_and_word_alignment_limits() {
        assert_eq!(validate_bmi_request(&[0; 4]), Ok(()));
        assert_eq!(validate_bmi_request(&[0; CE_BMI_SOURCE_MAX_BYTES]), Ok(()));
        assert_eq!(validate_bmi_request(&[]), Err(CeError::InvalidRequest));
        assert_eq!(
            validate_bmi_request(&[0; CE_BMI_SOURCE_MAX_BYTES + 4]),
            Err(CeError::InvalidRequest)
        );
        assert_eq!(validate_bmi_request(&[0; 3]), Err(CeError::InvalidRequest));
    }

    #[test]
    fn completion_timeouts_identify_source_or_destination_engine() {
        assert_eq!(
            completion_error(CeError::Timeout, CeError::SourceTimeout),
            CeError::SourceTimeout
        );
        assert_eq!(
            completion_error(CeError::Timeout, CeError::DestinationTimeout),
            CeError::DestinationTimeout
        );
        assert_eq!(
            completion_error(CeError::DeviceUnavailable, CeError::DestinationTimeout),
            CeError::DeviceUnavailable
        );
    }
}
