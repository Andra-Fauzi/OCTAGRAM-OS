//! Driver transport xHCI (USB 3.x host controller): setup MMIO/MSI-X,
//! reset & init controller (DCBAA, Command Ring, Event Ring), device
//! enumeration (enable slot, address device, get descriptor), sampai
//! configure endpoint. Device-class driver (mis. HID keyboard di
//! `keyboard_usb.rs`) dibangun di atas primitif-primitif di sini.

pub use super::EndpointInfo;
use super::keyboard_usb::{KBD_DCI, KBD_REPORT_LEN, KBD_REPORT_PENDING, KBD_SLOT_ID};
use super::mouse_usb::{MOUSE_DCI, MOUSE_REPORT_LEN, MOUSE_REPORT_PENDING, MOUSE_SLOT_ID};
use super::bulk_in::{BULK_IN_CODE, BULK_IN_DCI, BULK_IN_LENGTH, BULK_IN_PENDING, BULK_IN_SLOT_ID};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering, compiler_fence};
use spin::Mutex;
use spin::Once;

pub static COMPLETION_PENDING: AtomicBool = AtomicBool::new(false);
pub static COMPLETION_CODE: AtomicU32 = AtomicU32::new(0);
pub static COMPLETION_SLOT: AtomicU32 = AtomicU32::new(0);

pub static TRANSFER_COMPLETION_PENDING: AtomicBool = AtomicBool::new(false);
pub static TRANSFER_COMPLETION_CODE: AtomicU32 = AtomicU32::new(0);
pub static TRANSFER_COMPLETION_LENGTH: AtomicU32 = AtomicU32::new(0);
/// Slot/doorbell of the transfer event that last set
/// [`TRANSFER_COMPLETION_PENDING`], so a completion can be matched to the device
/// that asked for it instead of to whatever transfer happened to finish last.
pub static TRANSFER_COMPLETION_SLOT: AtomicU32 = AtomicU32::new(0);
pub static TRANSFER_COMPLETION_DCI: AtomicU32 = AtomicU32::new(0);
pub static MANUAL_POLLING: AtomicBool = AtomicBool::new(false);

// Di-set oleh `poll_event_ring` tiap kali ada Port Status Change Event
// (trb_type 34) -- artinya SATU ATAU LEBIH port berubah status (connect
// ATAU disconnect, event ini tidak bilang yang mana). Sengaja tidak
// langsung diproses di sini: proses hotplug (enable_slot/address_device/
// disable_slot) butuh cli/sti + spin-wait command completion, yang
// butuh interrupt vector 44 bisa nembak LAGI -- itu tidak akan terjadi
// selama kita masih di DALAM ISR vector 44 sekarang (EOI belum dikirim).
// Makanya flag ini cuma dibaca dari luar ISR, lihat `usb::poll_hotplug`
// yang dipanggil dari idle loop (`hcf`) di main.rs.
pub static PORT_CHANGE_PENDING: AtomicBool = AtomicBool::new(false);

pub static XHCI_INSTANCE: Once<Mutex<XHCI>> = Once::new();

/// Set when the command ring could not be restarted (or the controller stayed
/// halted), so the only way back is a full Host Controller Reset.
///
/// `usb::poll_hotplug` owns that reset because HCRST invalidates every slot, so
/// the USB class drivers have to be unbound first -- doing it from inside a
/// command-wait loop here would deadlock on drivers that take their own state
/// locks while still holding the controller lock.
pub static CONTROLLER_RESET_REQUIRED: AtomicBool = AtomicBool::new(false);

const PORTSC_CCS: u32 = 1 << 0;
const PORTSC_PED: u32 = 1 << 1;
const PORTSC_PR: u32 = 1 << 4;
const PORTSC_PP: u32 = 1 << 9;
const PORTSC_SPEED_MASK: u32 = 0xF << 10;
pub const PORTSC_CHANGE_BITS: u32 =
    (1 << 17) | (1 << 18) | (1 << 19) | (1 << 20) | (1 << 21) | (1 << 22) | (1 << 23) | (1 << 24);
const PORTSC_WPR: u32 = 1 << 31;

use super::super::pci;
use crate::{memory, println};

pub fn poll_manual_events() {
    if !MANUAL_POLLING.load(Ordering::SeqCst) {
        return;
    }
    if let Some(instance) = XHCI_INSTANCE.get() {
        unsafe {
            instance.lock().poll_event_ring();
        }
    }
    unsafe {
        super::keyboard_usb::poll_keyboard();
        super::mouse_usb::poll_mouse();
    }
}

/// Di-set oleh interrupt handler vector 44.
///
/// ISR **tidak boleh** mengambil `XHCI_INSTANCE`, `KBD_STATE`, atau
/// `MOUSE_STATE`: lock yang sama juga dipegang jalur normal
/// (`arm_next_report`, transfer command, `poll_manual_events`) dengan
/// interrupt aktif. Kalau IRQ masuk tepat saat lock sedang dipegang, handler
/// akan berputar selamanya di CPU yang sama (spin-lock tidak tahu bahwa
/// pemegangnya baru saja dia interupsi) -- mesin hang tanpa pesan apa pun.
/// Karena itu ISR hanya menandai flag ini, dan pemrosesan event ditunda ke
/// `poll_interrupt_deferred()` yang dipanggil dari idle loop lewat
/// `drivers::poll_deferred()`.
pub static INTERRUPT_PENDING: AtomicBool = AtomicBool::new(false);

/// Dipanggil dari handler vector 44; hanya menyentuh atomics.
pub fn note_interrupt() {
    INTERRUPT_PENDING.store(true, Ordering::Release);
}

/// Proses event xHCI yang tertunda, dari konteks non-IRQ.
pub fn poll_interrupt_deferred() {
    if !INTERRUPT_PENDING.swap(false, Ordering::AcqRel) {
        return;
    }
    if let Some(instance) = XHCI_INSTANCE.get() {
        // Safety: event ring ada di memori HHDM dan kita di luar ISR,
        // jadi aman mengambil lock yang juga dipakai jalur normal.
        unsafe {
            instance.lock().poll_event_ring();
        }
    }
    unsafe {
        super::keyboard_usb::poll_keyboard();
        super::mouse_usb::poll_mouse();
    }
}

#[derive(Debug)]
pub struct XhciCapRegs {
    base: *const u8, // read-only view
}

impl XhciCapRegs {
    pub unsafe fn cap_length(&self) -> u8 {
        unsafe { self.base.read_volatile() }
    }
    pub unsafe fn hci_version(&self) -> u16 {
        unsafe { (self.base.add(0x02) as *const u16).read_volatile() }
    }
    pub unsafe fn hcsparams1(&self) -> u32 {
        unsafe { (self.base.add(0x04) as *const u32).read_volatile() }
    }

    pub unsafe fn hcsparams2(&self) -> u32 {
        unsafe { (self.base.add(0x08) as *const u32).read_volatile() }
    }

    /// Jumlah scratchpad buffer yang diminta controller (field NSS).
    ///
    /// NSS dipecah di HCSPARAMS2: bit 21-25 = nilai low 5 bit, bit 27-31 =
    /// nilai high 5 bit. Controller yang meminta NSS > 0 mewajibkan
    /// `DCBAA[0]` menunjuk ke array alamat halaman scratchpad, kalau tidak
    /// setiap command gagal dengan Host Controller Error.
    pub unsafe fn scratchpad_buffers(&self) -> usize {
        let params = unsafe { self.hcsparams2() };
        let low = ((params >> 21) & 0x1F) as usize;
        let high = ((params >> 27) & 0x1F) as usize;
        (high << 5) | low
    }

    pub unsafe fn max_slots(&self) -> u8 {
        unsafe { (self.hcsparams1() & 0xFF) as u8 } // bit 0-7
    }

    pub unsafe fn max_ports(&self) -> u8 {
        unsafe { ((self.hcsparams1() >> 24) & 0xFF) as u8 } // bit 24-31
    }

    pub unsafe fn hccparams1(&self) -> u32 {
        unsafe { (self.base.add(0x10) as *const u32).read_volatile() }
    }

    /// Belum dipanggil di alur init sekarang -- berguna kalau nanti
    /// mau validasi asumsi 32-bit addressing sebelum setup DMA.
    #[allow(dead_code)]
    pub unsafe fn supports_64bit(&self) -> bool {
        unsafe { self.hccparams1() & 0x1 == 1 } // bit 0 = AC64
    }

    pub unsafe fn rtsoff(&self) -> u32 {
        unsafe { (self.base.add(0x18) as *const u32).read_volatile() & !0x1F }
    }

    pub unsafe fn dboff(&self) -> u32 {
        unsafe { (self.base.add(0x14) as *const u32).read_unaligned() & !0x3 }
    }

    pub unsafe fn context_size_64(&self) -> bool {
        unsafe { self.hccparams1() & (1 << 2) != 0 }
    }
}

#[derive(Debug)]
pub struct XhciOpRegs {
    base: *mut u8, // read-write view
}

impl XhciOpRegs {
    pub unsafe fn read_usbcmd(&self) -> u32 {
        unsafe { (self.base as *const u32).read_volatile() }
    }
    pub unsafe fn write_usbcmd(&self, value: u32) {
        unsafe { (self.base as *mut u32).write_volatile(value) }
    }
    pub unsafe fn read_usbsts(&self) -> u32 {
        unsafe { (self.base.add(0x04) as *const u32).read_volatile() }
    }
    pub unsafe fn write_usbsts(&self, value: u32) {
        unsafe { (self.base.add(0x04) as *mut u32).write_volatile(value) }
    }
    pub unsafe fn write_config(&self, value: u32) {
        unsafe { (self.base.add(0x38) as *mut u32).write_volatile(value) }
    }

    pub unsafe fn write_dcbaap(&self, phys_addr: u64) {
        unsafe { (self.base.add(0x30) as *mut u64).write_volatile(phys_addr) }
    }

    pub unsafe fn write_crcr(&self, value: u64) {
        unsafe { (self.base.add(0x18) as *mut u64).write_volatile(value) }
    }
    pub unsafe fn read_crcr(&self) -> u64 {
        unsafe { (self.base.add(0x18) as *const u64).read_volatile() }
    }

    pub unsafe fn read_portsc(&self, port: u8) -> u32 {
        unsafe {
            let offset = 0x400 + (port as usize) * 0x10;
            (self.base.add(offset) as *const u32).read_volatile()
        }
    }

    pub unsafe fn write_portsc(&self, port: u8, value: u32) {
        unsafe {
            let offset = 0x400 + (port as usize) * 0x10;
            (self.base.add(offset) as *mut u32).write_volatile(value)
        }
    }

    pub unsafe fn acknowledge_port_changes(&self, port: u8, port_status: u32) {
        let changes = port_status & PORTSC_CHANGE_BITS;
        if changes != 0 {
            unsafe {
                self.write_portsc(port, (port_status & PORTSC_PP) | changes);
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct XHCI {
    bar: pci::BarInfo,
    bar_size: u64,
    virt_base: u64,
    event_ring_virt: u64,
    event_ring_phys: u64,
    dequeue_index: usize,
    cycle_state: bool,
    last_completion: Option<(u32, u32)>,
    cmd_ring_virt: u64,
    cmd_ring_phys: u64,
    cmd_enqueue_index: usize,
    cmd_cycle_state: bool,
    dcbaa_virt: u64,
    context_size: usize,
    ep0_ring_virt: u64,
    ep0_ring_phys: u64,
    ep0_enqueue_index: usize,
    ep0_cycle_state: bool,
}

impl XHCI {
    pub fn new(bar: pci::BarInfo, dev: &pci::PciDevice) -> Self {
        unsafe {
            let bar_size = pci::get_bar0_size(dev);
            println!("BAR0 size = {:#x} ({} bytes)", bar_size, bar_size);
            assert!(
                bar_size > 0 && bar_size < 0x1000_0000,
                "BAR size mencurigakan: {:#x}",
                bar_size
            );

            let page_count = (bar_size + 0xFFF) / 0x1000;
            println!("Need to map {} pages", page_count);
            let virt_base = memory::paging::map_mmio_page(bar.address);

            for i in 1..page_count {
                memory::paging::map_mmio_page(bar.address + i * 0x1000);
            }
            Self {
                bar: bar,
                bar_size,
                virt_base: virt_base,
                event_ring_phys: 0,
                event_ring_virt: 0,
                cycle_state: false,
                dequeue_index: 0,
                last_completion: None,
                cmd_ring_virt: 0,
                cmd_ring_phys: 0,
                cmd_enqueue_index: 0,
                cmd_cycle_state: true,
                dcbaa_virt: 0,
                context_size: 32,
                ep0_ring_virt: 0,
                ep0_ring_phys: 0,
                ep0_enqueue_index: 0,
                ep0_cycle_state: true,
            }
        }
    }

    pub unsafe fn setup_msix(&self, dev: &pci::PciDevice, vector: u8, apic_id: u8) -> bool {
        unsafe {
            let info = match pci::get_msix_info(dev) {
                Some(i) => i,
                None => {
                    println!("Device tidak support MSI-X!");
                    return false;
                }
            };

            println!(
                "MSI-X info: table_bar={} table_offset={:#x} size={}",
                info.table_bar, info.table_offset, info.table_size
            );

            if info.table_bar >= 6 {
                println!("MSI-X table references invalid BAR {}", info.table_bar);
                return false;
            }
            let table_bar = pci::get_bar(dev, info.table_bar);
            let Some(table_phys) = table_bar.address.checked_add(info.table_offset as u64) else {
                println!("MSI-X table address overflows");
                return false;
            };
            let table_page = memory::paging::map_mmio_page(table_phys);
            let table_addr = table_page + (table_phys & 0xFFF);
            if (table_phys & 0xFFF) > 0xFF0 {
                memory::paging::map_mmio_page((table_phys & !0xFFF) + 0x1000);
            }

            let msg_addr = 0xFEE0_0000u32 | ((apic_id as u32) << 12);
            let msg_data = vector as u32;

            (table_addr as *mut u32).write_volatile(msg_addr);
            ((table_addr + 4) as *mut u32).write_volatile(0);
            ((table_addr + 8) as *mut u32).write_volatile(msg_data);
            ((table_addr + 12) as *mut u32).write_volatile(0);

            pci::set_msix_enable(dev, info.cap_offset, true);

            println!(
                "MSI-X entry[0] configured: vector={} apic_id={}",
                vector, apic_id
            );
            true
        }
    }

    pub unsafe fn cap_regs(&self) -> XhciCapRegs {
        XhciCapRegs {
            base: self.virt_base as *const u8,
        }
    }

    pub unsafe fn op_regs(&self) -> XhciOpRegs {
        unsafe {
            let cap = self.cap_regs();
            XhciOpRegs {
                base: cap.base.add(cap.cap_length() as usize) as *mut u8,
            }
        }
    }

    pub unsafe fn interrupter0(&self) -> XhciInterrupterRegs {
        unsafe {
            let cap = self.cap_regs();
            let rt_base = self.virt_base + cap.rtsoff() as u64;
            let ir0_base = rt_base + 0x20;
            XhciInterrupterRegs {
                base: ir0_base as *mut u8,
            }
        }
    }

    pub unsafe fn poll_event_ring(&mut self) {
        unsafe {
            let trbs = self.event_ring_virt as *const Trb;
            let ir0 = self.interrupter0();
            loop {
                let mut processed = 0;
                while processed < EVENT_RING_SIZE {
                    let trb = trbs.add(self.dequeue_index).read_volatile();
                    if (trb.control & 1 != 0) != self.cycle_state {
                        break;
                    }

                    let trb_type = (trb.control >> 10) & 0x3F;
                    match trb_type {
                        34 => {
                            let port_id = (trb.parameter >> 24) & 0xFF;
                            println!("EVENT: Port Status Change on port {}", port_id);
                            PORT_CHANGE_PENDING.store(true, Ordering::SeqCst);
                        }
                        33 => {
                            let completion_code = (trb.status >> 24) & 0xFF;
                            let slot_id = (trb.control >> 24) & 0xFF;
                            println!(
                                "EVENT: Command Completion, code={} slot_id={}",
                                completion_code, slot_id
                            );
                            COMPLETION_CODE.store(completion_code, Ordering::SeqCst);
                            COMPLETION_SLOT.store(slot_id, Ordering::SeqCst);
                            COMPLETION_PENDING.store(true, Ordering::SeqCst);
                        }
                        32 => {
                            let completion_code = (trb.status >> 24) & 0xFF;
                            let residual_length = trb.status & 0xFF_FFFF;
                            let endpoint_id = (trb.control >> 16) & 0x1F;
                            let event_slot_id = (trb.control >> 24) & 0xFF;

                            let kbd_dci = KBD_DCI.load(Ordering::SeqCst);
                            let kbd_slot = KBD_SLOT_ID.load(Ordering::SeqCst);
                            let mouse_dci = MOUSE_DCI.load(Ordering::SeqCst);
                            let mouse_slot = MOUSE_SLOT_ID.load(Ordering::SeqCst);
                            let network_dci = BULK_IN_DCI.load(Ordering::SeqCst);
                            let network_slot = BULK_IN_SLOT_ID.load(Ordering::SeqCst);

                            if kbd_dci != 0 && event_slot_id == kbd_slot && endpoint_id == kbd_dci {
                                KBD_REPORT_LEN.store(residual_length, Ordering::SeqCst);
                                KBD_REPORT_PENDING.store(true, Ordering::SeqCst);
                            } else if mouse_dci != 0
                                && event_slot_id == mouse_slot
                                && endpoint_id == mouse_dci
                            {
                                MOUSE_REPORT_LEN.store(residual_length, Ordering::SeqCst);
                                MOUSE_REPORT_PENDING.store(true, Ordering::SeqCst);
                            } else if network_dci != 0
                                && event_slot_id == network_slot
                                && endpoint_id == network_dci
                            {
                                BULK_IN_CODE.store(completion_code, Ordering::SeqCst);
                                BULK_IN_LENGTH.store(residual_length, Ordering::SeqCst);
                                BULK_IN_PENDING.store(true, Ordering::SeqCst);
                            } else {
                                println!(
                                    "EVENT: Transfer Event, code={} residual={}",
                                    completion_code, residual_length
                                );
                                TRANSFER_COMPLETION_CODE.store(completion_code, Ordering::SeqCst);
                                TRANSFER_COMPLETION_LENGTH.store(residual_length, Ordering::SeqCst);
                                TRANSFER_COMPLETION_SLOT.store(event_slot_id, Ordering::SeqCst);
                                TRANSFER_COMPLETION_DCI.store(endpoint_id, Ordering::SeqCst);
                                TRANSFER_COMPLETION_PENDING.store(true, Ordering::SeqCst);
                            }
                        }
                        _ => println!("EVENT: unknown type={}", trb_type),
                    }

                    self.dequeue_index += 1;
                    if self.dequeue_index >= EVENT_RING_SIZE {
                        self.dequeue_index = 0;
                        self.cycle_state = !self.cycle_state;
                    }
                    processed += 1;
                }

                if processed != 0 {
                    let new_erdp = self.event_ring_phys + (self.dequeue_index as u64 * 16);
                    ir0.write_erdp(new_erdp | (1 << 3));
                }

                let iman = ir0.read_iman();
                ir0.write_iman((iman & (1 << 1)) | 1);
                self.op_regs().write_usbsts(USBSTS_EINT);

                let next = trbs.add(self.dequeue_index).read_volatile();
                if (next.control & 1 != 0) != self.cycle_state {
                    break;
                }
            }
        }
    }

    // CATATAN (bukan compiler warning, tapi ketemu pas review): `last_completion`
    // di struct XHCI cuma pernah di-set ke `None` di seluruh file ini --
    // tidak ada tempat yang menyetelnya ke `Some(...)`. Artinya loop di
    // bawah akan SELALU timeout, bukan langsung dapat completion. Command
    // completion sesungguhnya ditangani lewat `COMPLETION_PENDING` +
    // `COMPLETION_CODE`/`COMPLETION_SLOT` (lihat `enable_slot()` dan
    // `address_device()`), jadi kemungkinan besar fungsi ini memang belum
    // dipakai di alur sekarang -- perlu dicek lagi sebelum dipanggil.
    #[allow(dead_code)]
    pub unsafe fn send_command_and_wait(
        &mut self,
        parameter: u64,
        status: u32,
        trb_type: u32,
        slot_id: u32,
    ) -> Result<u32, &'static str> {
        unsafe {
            self.last_completion = None;
            self.send_command(parameter, status, trb_type, slot_id);

            let mut timeout = 1_000_000;
            loop {
                if let Some((code, slot_id)) = self.last_completion.take() {
                    if code == 1 {
                        return Ok(slot_id);
                    } else {
                        return Err("Command failed, completion code bukan Succes");
                    }
                }
                core::arch::asm!("hlt");
                timeout -= 1;
                if timeout == 0 {
                    return Err("Timeout menunggu command completion");
                }
            }
        }
    }

    pub unsafe fn ring_doorbell(&self, slot: u8, value: u32) {
        unsafe {
            let cap = self.cap_regs();
            let db_base = self.virt_base + cap.dboff() as u64;
            let db_ptr = (db_base + (slot as u64 * 4)) as *mut u32;
            compiler_fence(Ordering::Release);
            db_ptr.write_volatile(value);
        }
    }

    /// Pastikan `slots` TRB berikutnya masih muat sebelum slot Link TRB.
    ///
    /// Satu control transfer harus tetap kontigu, jadi kalau sisa ruang tidak
    /// cukup seluruh transfer digeser ke awal ring: Link TRB ditulis di slot
    /// terakhir dengan cycle state sekarang, lalu enqueue kembali ke 0 dengan
    /// cycle state yang sudah di-toggle. Tanpa Link TRB fisik, controller
    /// berhenti di slot kosong dan endpoint control membeku permanen.
    pub unsafe fn ep0_make_room(&mut self, slots: usize) {
        let capacity = CONTROL_RING_SIZE - 1;
        if self.ep0_enqueue_index + slots > capacity {
            unsafe {
                let trbs = self.ep0_ring_virt as *mut Trb;
                let control = (TRB_TYPE_LINK << 10) | (1 << 1) | (self.ep0_cycle_state as u32);
                write_trb_cycle_last(
                    trbs.add(self.ep0_enqueue_index),
                    Trb {
                        parameter: self.ep0_ring_phys,
                        status: 0,
                        control,
                    },
                );
                write_link_trb(
                    self.ep0_ring_virt,
                    self.ep0_ring_phys,
                    CONTROL_RING_SIZE,
                    self.ep0_cycle_state,
                );
            }
            self.ep0_enqueue_index = 0;
            self.ep0_cycle_state = !self.ep0_cycle_state;
        }
    }

    pub unsafe fn ep0_enqueue_trb(&mut self, parameter: u64, status: u32, control_no_cycle: u32) {
        unsafe {
            let trbs = self.ep0_ring_virt as *mut Trb;
            let control = control_no_cycle | (self.ep0_cycle_state as u32);

            let trb = Trb {
                parameter,
                status,
                control,
            };
            write_trb_cycle_last(trbs.add(self.ep0_enqueue_index), trb);

            // Wrap-around ditangani `ep0_make_room()` sebelum transfer dimulai,
            // supaya TRB satu transfer tidak pernah terpotong Link TRB.
            self.ep0_enqueue_index += 1;
        }
    }

    pub unsafe fn send_command(
        &mut self,
        parameter: u64,
        status: u32,
        trb_type: u32,
        slot_id: u32,
    ) {
        unsafe {
            let trbs = self.cmd_ring_virt as *mut Trb;
            let control = (trb_type << 10) | (self.cmd_cycle_state as u32) | (slot_id << 24);

            // Command ring butuh Link TRB fisik di slot terakhir. Tanpa ini
            // controller berhenti di slot kosong setelah 31 command dan setiap
            // Enable Slot / Address Device sesudahnya timeout.
            if self.cmd_enqueue_index >= COMMAND_RING_SIZE - 1 {
                write_link_trb(
                    self.cmd_ring_virt,
                    self.cmd_ring_phys,
                    COMMAND_RING_SIZE,
                    self.cmd_cycle_state,
                );
                self.cmd_enqueue_index = 0;
                self.cmd_cycle_state = !self.cmd_cycle_state;
            }

            let trb = Trb {
                parameter,
                status,
                control,
            };
            write_trb_cycle_last(trbs.add(self.cmd_enqueue_index), trb);
            self.cmd_enqueue_index += 1;

            self.ring_doorbell(0, 0);
        }
    }

    /// Enqueue satu TRB Normal ke ring endpoint mana pun lalu ring doorbell-nya.
    /// Generic untuk semua endpoint interrupt IN (keyboard, mouse, atau HID lain) --
    /// state ring (enqueue_index, cycle_state) dimiliki oleh device-class driver
    /// masing-masing (lihat `keyboard_usb.rs` / `mouse_usb.rs`), bukan oleh XHCI.
    pub unsafe fn enqueue_normal_trb_and_ring(
        &self,
        ring_virt: u64,
        ring_phys: u64,
        enqueue_index: &mut usize,
        cycle_state: &mut bool,
        ring_size: usize,
        slot_id: u32,
        dci: u32,
        buf_phys: u64,
        len: u32,
    ) {
        unsafe {
            let trbs = ring_virt as *mut Trb;

            let control = (TRB_TYPE_NORMAL << 10) | (1 << 5) | (1 << 2) | (*cycle_state as u32);

            let trb = Trb {
                parameter: buf_phys,
                status: len & 0x1_FFFF,
                control,
            };
            write_trb_cycle_last(trbs.add(*enqueue_index), trb);

            *enqueue_index += 1;
            if *enqueue_index >= ring_size - 1 {
                write_link_trb(ring_virt, ring_phys, ring_size, *cycle_state);
                *enqueue_index = 0;
                *cycle_state = !*cycle_state;
            }

            self.ring_doorbell(slot_id as u8, dci);
        }
    }
}

#[repr(C, align(64))]
struct Dcbaa {
    entries: [u64; 256], // maksimal 255 slot + 1 reserved, cukup buat semua kasus
}

impl Dcbaa {
    fn new() -> Self {
        Self {
            entries: [0u64; 256],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Trb {
    parameter: u64,
    status: u32,
    control: u32,
}

impl Trb {
    const fn empty() -> Self {
        Self {
            parameter: 0,
            status: 0,
            control: 0,
        }
    }
}

const COMMAND_RING_SIZE: usize = 32;

#[repr(C, align(64))]
struct CommandRing {
    trbs: [Trb; COMMAND_RING_SIZE],
}

impl CommandRing {
    fn new() -> Self {
        Self {
            trbs: [Trb::empty(); COMMAND_RING_SIZE],
        }
    }
}

/// Helper debug manual -- panggil sendiri dari `kmain` kalau perlu
/// cetak info cap register xHCI, tidak dipanggil otomatis.
#[allow(dead_code)]
pub fn test_xhci(xhci: &XHCI) {
    let xhci_cap_regs = XhciCapRegs {
        base: xhci.virt_base as *const u8,
    };

    println!("XHCI_CAP_REGS: {:#?}", xhci_cap_regs);
    unsafe {
        println!("XHCI_CAP_REGS REVISION: {}", xhci_cap_regs.hci_version());
        println!("XHCI_CAP_REGS CAPLENGTH: {}", xhci_cap_regs.cap_length());
    }
}

const TRB_TYPE_NORMAL: u32 = 1;
const TRB_TYPE_ENABLE_SLOT: u32 = 9;
const TRB_TYPE_DISABLE_SLOT: u32 = 10;
const TRB_TYPE_ADDRESS_DEVICE: u32 = 11;
const TRB_TYPE_CONFIGURE_ENDPOINT: u32 = 12;

/*
Bit    Nama        Fungsi
0      R/S         Run/Stop — 1=jalan, 0=stop (baca-tulis)
1      HCRST       Host Controller Reset — tulis 1 buat trigger reset, auto-clear sendiri
2      INTE        Interrupter Enable — 1=izinkan controller kirim interrupt
3      HSEE        Host System Error Enable
4-6    Reserved    (harus 0)
7      LHCRST      Light Host Controller Reset (opsional, cek HCCPARAMS1.LHRC dulu)
8      CSS         Controller Save State
9      CRS         Controller Restore State
10     EWE         Enable Wrap Event
11     EU3S        Enable U3 MFINDEX Stop
12     Reserved
13     CME         CEM Enable (opsional)
14     ETE         Extended TBC Enable
15     TSC_EN      TSC Enable
16     VTIOE       VTIO Enable
17-31  Reserved
*/
const USBCMD_RUN_OR_STOP: u32 = 1 << 0;
// host controller reset
const USBCMD_HCRST: u32 = 1 << 1;
// interrupt enable
const USBCMD_INTE: u32 = 1 << 2;

/*
Bit    Nama       Tipe      Fungsi
0      HCH        RO        HC Halted — 1=controller berhenti (bukan sedang jalan)
1      Reserved
2      HSE        RW1C      Host System Error — 1=ada serious error, tulis 1 buat clear
3      EINT       RW1C      Event Interrupt — 1=ada event pending di event ring
4      PCD        RW1C      Port Change Detect — 1=ada port yang statusnya berubah
5-7    Reserved
8      SSS        RO        Save State Status
9      RSS        RO        Restore State Status
10     SRE        RW1C      Save/Restore Error
11     CNR        RO        Controller Not Ready — 1=controller BELUM siap dipakai
12     HCE        RO        Host Controller Error — 1=fatal error, butuh reset total
13-31  Reserved
*/
const USBSTS_HCH: u32 = 1 << 0;
const USBSTS_HSE: u32 = 1 << 2;
const USBSTS_EINT: u32 = 1 << 3;
const USBSTS_PCD: u32 = 1 << 4;
const USBSTS_CNR: u32 = 1 << 11;
const USBSTS_HCE: u32 = 1 << 12;
const HCCPARAMS1_PPC: u32 = 1 << 3;
const XHCI_EXT_CAP_ID_LEGACY_SUPPORT: u32 = 1;
const LEGACY_SUPPORT_BIOS_OWNED: u32 = 1 << 16;
const LEGACY_SUPPORT_OS_OWNED: u32 = 1 << 24;

fn port_reset_command(speed: u8) -> u32 {
    if speed == 4 { PORTSC_WPR } else { PORTSC_PR }
}

/// Busy-wait for `milliseconds` using the 8254 PIT channel 2.
///
/// The xHCI spec puts real time between steps (10 ms reset recovery after a port
/// reset, 2 ms after Address Device before the first transfer), and a plain spin
/// count covers a wildly different amount of time on every machine. Reads and
/// writes go through the same PIT programming the Wi-Fi driver uses.
fn delay_milliseconds(milliseconds: u16) {
    if cfg!(test) {
        return;
    }
    #[cfg(target_arch = "x86_64")]
    {
        use crate::arch::x86_64::io;
        const PIT_HZ: u32 = 1_193_182;
        // One PIT period is ~0.84 ms, so a count needs a couple of ticks per ms.
        let ticks = ((PIT_HZ / 1000) * u32::from(milliseconds)).clamp(1, 0xffff);
        unsafe {
            let port_61 = io::inb(0x61);
            io::outb(0x61, (port_61 & 0xfd) | 0x01);
            io::outb(0x43, 0xb0);
            io::outb(0x42, (ticks & 0xff) as u8);
            io::outb(0x42, ((ticks >> 8) & 0xff) as u8);
            while io::inb(0x61) & 0x20 == 0 {
                core::hint::spin_loop();
            }
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let spins = u32::from(milliseconds).saturating_mul(20_000);
        for _ in 0..spins {
            core::hint::spin_loop();
        }
    }
}

fn next_extended_capability_offset(header: u32) -> Option<usize> {
    let next = ((header >> 8) & 0xFF) as usize;
    (next != 0).then_some(next * 4)
}

pub unsafe fn init_xhci(xhci: &mut XHCI, enable_interrupts: bool) {
    unsafe {
        let xhci_cap_regs = xhci.cap_regs();
        let xhci_op_regs = xhci.op_regs();
        handoff_firmware_ownership(xhci, &xhci_cap_regs);
        xhci.context_size = if xhci_cap_regs.context_size_64() {
            64
        } else {
            32
        };
        println!("xHCI context size: {} bytes", xhci.context_size);

        // --- RESET (yang udah kita fix kemarin) ---
        let mut cmd = xhci_op_regs.read_usbcmd();
        cmd &= !USBCMD_RUN_OR_STOP;
        xhci_op_regs.write_usbcmd(cmd);

        // Setiap tunggu MMIO di bawah dibatasi: controller yang tidak halt atau
        // tidak selesai reset harus dilaporkan, bukan membuat boot hang tanpa
        // pesan sama sekali.
        let mut spins = 1_000_000u32;
        while xhci_op_regs.read_usbsts() & USBSTS_HCH == 0 {
            if spins == 0 {
                println!(
                    "xHCI reset gagal: controller tidak halt (USBSTS={:#010x})",
                    xhci_op_regs.read_usbsts()
                );
                return;
            }
            spins -= 1;
            core::hint::spin_loop();
        }

        cmd = xhci_op_regs.read_usbcmd();
        cmd |= USBCMD_HCRST; // FIXED: set, bukan clear
        xhci_op_regs.write_usbcmd(cmd);

        let mut spins = 1_000_000u32;
        while xhci_op_regs.read_usbcmd() & USBCMD_HCRST != 0 {
            if spins == 0 {
                println!("xHCI reset gagal: HCRST tidak pernah clear");
                return;
            }
            spins -= 1;
            core::hint::spin_loop();
        }

        let mut spins = 10_000_000u32;
        while xhci_op_regs.read_usbsts() & USBSTS_CNR != 0 {
            if spins == 0 {
                println!(
                    "xHCI reset gagal: controller tidak ready (CNR, USBSTS={:#010x})",
                    xhci_op_regs.read_usbsts()
                );
                return;
            }
            spins -= 1;
            core::hint::spin_loop();
        }

        println!("XHCI RESET DONE");

        // --- MaxSlots & CONFIG ---
        let max_slots = xhci_cap_regs.max_slots();
        xhci_op_regs.write_config(max_slots as u32);
        println!("MaxSlots enabled: {}", max_slots);

        // --- DCBAA ---
        let dcbaa_box: Box<Dcbaa> = Box::new(Dcbaa::new());
        let dcbaa_virt = Box::into_raw(dcbaa_box) as u64;
        let dcbaa_phys = memory::virtual_to_physical(dcbaa_virt as usize).unwrap();
        xhci_op_regs.write_dcbaap(dcbaa_phys as u64);
        println!("DCBAA phys={:#x}", dcbaa_phys);

        // --- Scratchpad buffers ---
        let nss = xhci_cap_regs.scratchpad_buffers();
        println!("xHCI scratchpad buffers (NSS): {}", nss);
        if nss > 0 {
            setup_scratchpads(dcbaa_virt, nss);
        }

        // --- Command Ring ---
        let cmd_ring_box: Box<CommandRing> = Box::new(CommandRing::new());
        let cmd_ring_virt = Box::into_raw(cmd_ring_box) as u64;
        let cmd_ring_phys = memory::virtual_to_physical(cmd_ring_virt as usize).unwrap();
        let crcr_value = (cmd_ring_phys & !0x3F) | 1;
        xhci_op_regs.write_crcr(crcr_value as u64);
        println!("Command Ring phys={:#x}", cmd_ring_phys);

        xhci.cmd_ring_virt = cmd_ring_virt;
        xhci.cmd_ring_phys = cmd_ring_phys as u64;
        xhci.cmd_enqueue_index = 0;
        xhci.cmd_cycle_state = true;
        xhci.dcbaa_virt = dcbaa_virt;

        println!("XHCI INIT STAGE 1 COMPLETE");

        let _segment_phys = setup_event_ring(xhci, enable_interrupts);

        start_controller(xhci, enable_interrupts);
        power_root_ports(&xhci_cap_regs, &xhci_op_regs);
    }
}

unsafe fn handoff_firmware_ownership(xhci: &XHCI, cap: &XhciCapRegs) {
    unsafe {
        let mut offset = ((cap.hccparams1() >> 16) as usize) * 4;
        if offset == 0 {
            return;
        }

        for _ in 0..(xhci.bar_size as usize / 4) {
            if offset < 0x20 || offset + 4 > xhci.bar_size as usize {
                println!(
                    "xHCI extended capability offset out of range: {:#x}",
                    offset
                );
                return;
            }

            let header = (cap.base.add(offset) as *const u32).read_volatile();
            if header & 0xFF == XHCI_EXT_CAP_ID_LEGACY_SUPPORT {
                let ownership = cap.base.add(offset) as *mut u32;
                ownership.write_volatile(header | LEGACY_SUPPORT_OS_OWNED);

                let mut bios_released = false;
                for _ in 0..10_000_000 {
                    if ownership.read_volatile() & LEGACY_SUPPORT_BIOS_OWNED == 0 {
                        bios_released = true;
                        break;
                    }
                    core::hint::spin_loop();
                }
                if !bios_released {
                    println!("WARNING: firmware did not release xHCI ownership");
                }

                if offset + 8 <= xhci.bar_size as usize {
                    (cap.base.add(offset + 4) as *mut u32).write_volatile(0x000F_0000);
                }
                return;
            }

            let Some(next_offset) = next_extended_capability_offset(header) else {
                return;
            };
            offset += next_offset;
        }
        println!("xHCI extended capability list exceeded BAR bounds");
    }
}

unsafe fn power_root_ports(cap: &XhciCapRegs, op: &XhciOpRegs) {
    unsafe {
        if cap.hccparams1() & HCCPARAMS1_PPC == 0 {
            return;
        }

        let max_ports = cap.max_ports();
        for port in 0..max_ports {
            if op.read_portsc(port) & PORTSC_PP == 0 {
                op.write_portsc(port, PORTSC_PP);
                println!("Powered xHCI root port {}", port + 1);
            }
        }
    }
}

/// Batas jumlah scratchpad yang bisa dilayani driver ini (NSS maksimum 1023).
const SCRATCHPAD_MAX: usize = 1024;

#[repr(C, align(64))]
struct ScratchpadArray {
    entries: [u64; SCRATCHPAD_MAX],
}

/// Alokasi scratchpad buffer sesuai `HCSPARAMS2.NSS` lalu pasang di `DCBAA[0]`.
unsafe fn setup_scratchpads(dcbaa_virt: u64, count: usize) {
    unsafe {
        if count > SCRATCHPAD_MAX {
            println!(
                "xHCI scratchpad: NSS {} melebihi batas driver {}",
                count, SCRATCHPAD_MAX
            );
            return;
        }

        let array_box: Box<ScratchpadArray> = Box::new(ScratchpadArray {
            entries: [0u64; SCRATCHPAD_MAX],
        });
        let array_virt = Box::into_raw(array_box) as u64;
        let Some(array_phys) = memory::virtual_to_physical(array_virt as usize) else {
            println!("xHCI scratchpad: gagal translate array");
            return;
        };
        let entries = array_virt as *mut u64;

        for index in 0..count {
            let Some(frame) = memory::allocate_dma32_frame() else {
                println!(
                    "xHCI scratchpad: gagal alokasi halaman {}/{}",
                    index, count
                );
                return;
            };
            if let Some(virt) = memory::physical_to_virtual(frame) {
                core::ptr::write_bytes(virt as *mut u8, 0, 4096);
            }
            entries.add(index).write_volatile(frame as u64);
        }

        // DCBAA[0] bukan slot device: isinya menunjuk ke scratchpad array.
        (dcbaa_virt as *mut u64).write_volatile(array_phys as u64);
        println!(
            "xHCI scratchpad: {} halaman, array phys={:#x}",
            count, array_phys
        );
    }
}

const EVENT_RING_SIZE: usize = 32;

#[repr(C, align(64))]
struct EventRingSegment {
    trbs: [Trb; EVENT_RING_SIZE],
}

impl EventRingSegment {
    pub fn new() -> Self {
        Self {
            trbs: [Trb::empty(); EVENT_RING_SIZE],
        }
    }
}

// Layout memori ERST entry sesuai spec xHCI -- field ditulis buat
// dibaca controller lewat DMA, bukan lewat kode Rust.
#[allow(dead_code)]
#[repr(C, align(64))]
struct ErstEntry {
    ring_segment_base: u64, // physical address segment harus 64 byte aligned
    ring_segment_size: u32, // jumlah TRB di segment ini (bukan byte)
    reserved: u32,
}

#[derive(Debug)]
pub struct XhciInterrupterRegs {
    base: *mut u8,
}

impl XhciInterrupterRegs {
    pub unsafe fn write_iman(&self, value: u32) {
        unsafe { (self.base as *mut u32).write_volatile(value) }
    }
    pub unsafe fn read_iman(&self) -> u32 {
        unsafe { (self.base as *const u32).read_volatile() }
    }
    pub unsafe fn write_imod(&self, value: u32) {
        unsafe { (self.base.add(0x04) as *mut u32).write_volatile(value) }
    }
    pub unsafe fn write_erstsz(&self, value: u32) {
        unsafe { (self.base.add(0x08) as *mut u32).write_volatile(value) }
    }
    pub unsafe fn write_erstba(&self, value: u64) {
        unsafe { (self.base.add(0x10) as *mut u64).write_volatile(value) }
    }
    pub unsafe fn write_erdp(&self, value: u64) {
        unsafe { (self.base.add(0x18) as *mut u64).write_volatile(value) }
    }
    pub unsafe fn read_erdp(&self) -> u64 {
        unsafe { (self.base.add(0x18) as *const u64).read_volatile() }
    }
}

pub unsafe fn setup_event_ring(xhci: &mut XHCI, enable_interrupts: bool) -> u64 {
    unsafe {
        let segment_box: Box<EventRingSegment> = Box::new(EventRingSegment::new());
        let segment_virt = Box::into_raw(segment_box) as u64;
        let segment_phys = memory::virtual_to_physical(segment_virt as usize).unwrap() as u64;

        let erst_box: Box<ErstEntry> = Box::new(ErstEntry {
            ring_segment_base: segment_phys,
            ring_segment_size: EVENT_RING_SIZE as u32,
            reserved: 0,
        });
        let erst_virt = Box::into_raw(erst_box) as u64;
        let erst_phys = memory::virtual_to_physical(erst_virt as usize).unwrap() as u64;

        let ir0 = xhci.interrupter0();

        // Urutan wajib: ERSTSZ -> ERSTBA -> ERDP. ERDP ditulis terakhir karena
        // menulisnya memberi tahu controller bahwa tabel ERST sudah valid.
        ir0.write_erstsz(1);
        ir0.write_erstba(erst_phys);
        ir0.write_erdp(segment_phys);

        let mut iman = ir0.read_iman();
        if enable_interrupts {
            iman |= 1 << 1;
        } else {
            iman &= !(1 << 1);
        }
        ir0.write_iman(iman);

        xhci.event_ring_virt = segment_virt;
        xhci.event_ring_phys = segment_phys;
        xhci.dequeue_index = 0;
        xhci.cycle_state = true;

        println!(
            "Event Ring segment phys={:#x}, ERST phys={:#x}",
            segment_phys, erst_phys
        );

        segment_phys
    }
}

pub unsafe fn start_controller(xhci: &XHCI, enable_interrupts: bool) {
    unsafe {
        let op = xhci.op_regs();

        let mut cmd = op.read_usbcmd();
        if enable_interrupts {
            cmd |= USBCMD_INTE;
        } else {
            cmd &= !USBCMD_INTE;
        }
        cmd |= USBCMD_RUN_OR_STOP;
        op.write_usbcmd(cmd);

        let mut timeout = 1_000_000;
        while op.read_usbsts() & USBSTS_HCH != 0 {
            timeout -= 1;
            if timeout == 0 {
                println!("TIMEOUT: controller gagal start!");
                return;
            }
        }
        println!("XHCI CONTROLLER RUNNING!");
    }
}

pub unsafe fn scan_ports(xhci: &XHCI) {
    unsafe {
        let cap = xhci.cap_regs();
        let op = xhci.op_regs();
        let max_ports = cap.max_ports();

        println!("Scanning {} ports..", max_ports);

        for port in 0..max_ports {
            let portsc = op.read_portsc(port);
            op.acknowledge_port_changes(port, portsc);
            let connected = portsc & PORTSC_CCS != 0;

            if connected {
                let speed = (portsc & PORTSC_SPEED_MASK) >> 10;
                println!("Port {} CONNECTED, speed={}", port, speed);
            }
        }
    }
}

pub unsafe fn reset_port(root_port: u8) -> Result<u8, &'static str> {
    unsafe {
        if root_port == 0 {
            return Err("invalid xHCI root port");
        }
        let Some(instance) = XHCI_INSTANCE.get() else {
            return Err("xHCI controller is not initialized");
        };
        let port = root_port - 1;
        let op = {
            let controller = instance.lock();
            if root_port > controller.cap_regs().max_ports() {
                return Err("xHCI root port is out of range");
            }
            controller.op_regs()
        };
        let initial_status = op.read_portsc(port);
        if initial_status & PORTSC_CCS == 0 {
            return Err("USB device disconnected before port reset");
        }

        let speed = ((initial_status & PORTSC_SPEED_MASK) >> 10) as u8;
        let reset_bit = port_reset_command(speed);
        op.acknowledge_port_changes(port, initial_status);
        op.write_portsc(port, (initial_status & PORTSC_PP) | reset_bit);
        println!(
            "USB root port {} reset dimulai (PORTSC={:#010x})",
            root_port,
            op.read_portsc(port)
        );

        for _ in 0..1_000_000 {
            let status = op.read_portsc(port);
            if status & reset_bit == 0 {
                op.acknowledge_port_changes(port, status);
                if status & PORTSC_CCS == 0 {
                    return Err("USB device disconnected during port reset");
                }
                if status & PORTSC_PED == 0 {
                    return Err("USB port did not enable after reset");
                }
                // tRSTRCY: the device needs a moment after the reset before it
                // answers its address. Addressing it immediately is what made
                // flashdisks and phones fail on real hardware while a keyboard
                // (which recovers faster) still worked.
                delay_milliseconds(if speed == 4 { 50 } else { 10 });
                return Ok(((status & PORTSC_SPEED_MASK) >> 10) as u8);
            }
            poll_event_ring_once();
            core::hint::spin_loop();
        }
        println!(
            "USB root port {} reset timeout (PORTSC={:#010x})",
            root_port,
            op.read_portsc(port)
        );
        Err("USB port reset timed out")
    }
}

/// Poll the event ring once from a command-wait loop.
///
/// `poll_manual_events()` is a no-op when the controller runs in interrupt
/// mode, and the deferred handler only runs from the idle loop that these spins
/// block, so a completion could sit in the ring unnoticed until the wait timed
/// out.
fn poll_event_ring_once() {
    if let Some(instance) = XHCI_INSTANCE.get() {
        unsafe { instance.lock().poll_event_ring() };
    }
}

/// CRCR value that starts the command ring at `ring_phys` with cycle state 1.
///
/// Bit 0 is the Ring Cycle State software enqueues with, bits 1-3 are Command
/// Stop / Command Abort / Command Ring Running and stay untouched, and the base
/// address has to be 64-byte aligned (bits 5:0 reserved).
fn command_ring_crcr(ring_phys: u64) -> u64 {
    (ring_phys & !0x3F) | 1
}

/// Whether a Command Completion Event belongs to the command we are waiting on.
///
/// A completion without a slot id (Enable Slot sits in the same field as the
/// returned slot) always matches, and so does the requested slot; anything else
/// is a late completion for a different command and must be skipped instead of
/// being reported as the answer.
fn command_completion_matches(expected_slot: u32, event_slot: u32) -> bool {
    expected_slot == 0 || event_slot == 0 || event_slot == expected_slot
}

unsafe fn wait_controller_halt(op: &XhciOpRegs, mut spins: u32) -> bool {
    unsafe {
        while spins > 0 {
            if op.read_usbsts() & USBSTS_HCH != 0 {
                return true;
            }
            spins -= 1;
            core::hint::spin_loop();
        }
        op.read_usbsts() & USBSTS_HCH != 0
    }
}

unsafe fn wait_controller_run(op: &XhciOpRegs, mut spins: u32) -> bool {
    unsafe {
        while spins > 0 {
            if op.read_usbsts() & USBSTS_HCH == 0 {
                return true;
            }
            spins -= 1;
            core::hint::spin_loop();
        }
        op.read_usbsts() & USBSTS_HCH == 0
    }
}

/// Restart the command ring after the controller stopped consuming commands.
///
/// Two things made the previous version useless on real hardware:
///
/// * `CRCR` must only be written while the controller is stopped. Writing it
///   while the controller runs is undefined and the write is typically ignored,
///   so the ring stayed dead and every later command timed out again -- exactly
///   the `CRCR lama=0x...08` / `disable_slot gagal` loop in the boot log.
/// * A halted controller (`USBSTS.HCH`) never consumes commands at all, no
///   matter where `CRCR` points.
///
/// So: halt the controller, rewind the ring, re-arm `CRCR` and run it again.
/// Slots, DCBAA and the event ring all survive a Run/Stop cycle, so devices that
/// are already enumerated keep working.
pub unsafe fn recover_command_ring(reason: &str) -> bool {
    unsafe {
        let Some(instance) = XHCI_INSTANCE.get() else {
            return false;
        };
        let mut guard = instance.lock();
        let (ring_virt, ring_phys) = (guard.cmd_ring_virt, guard.cmd_ring_phys);
        if ring_virt == 0 || ring_phys == 0 {
            return false;
        }
        let op = guard.op_regs();
        let crcr = op.read_crcr();
        let usbsts = op.read_usbsts();

        // 1. Halt the controller: a CRCR write is only defined while halted.
        let mut cmd = op.read_usbcmd();
        cmd &= !USBCMD_RUN_OR_STOP;
        op.write_usbcmd(cmd);
        if !unsafe { wait_controller_halt(&op, 1_000_000) } {
            println!(
                "xHCI: controller tidak mau halt untuk restart command ring (USBSTS={:#010x})",
                op.read_usbsts()
            );
            CONTROLLER_RESET_REQUIRED.store(true, Ordering::SeqCst);
            return false;
        }

        // 1b. Drop a latched Host System Error: while HSE is set the controller
        //     stays halted and never consumes a command again.
        op.write_usbsts(USBSTS_HSE | USBSTS_HCE | USBSTS_EINT);
        compiler_fence(Ordering::SeqCst);

        // 2. Rewind the ring. Stale TRBs whose cycle bit still matches would be
        //    executed again as soon as the ring is re-armed.
        op.write_crcr(0);
        let trbs = ring_virt as *mut Trb;
        for index in 0..COMMAND_RING_SIZE {
            trbs.add(index).write_volatile(Trb::empty());
        }
        compiler_fence(Ordering::SeqCst);

        guard.cmd_enqueue_index = 0;
        guard.cmd_cycle_state = true;
        op.write_crcr(command_ring_crcr(ring_phys));
        compiler_fence(Ordering::SeqCst);

        // 3. Run again, and note whether the controller actually came back.
        cmd = op.read_usbcmd();
        cmd |= USBCMD_RUN_OR_STOP;
        op.write_usbcmd(cmd);
        let running = unsafe { wait_controller_run(&op, 1_000_000) };
        COMPLETION_PENDING.store(false, Ordering::SeqCst);
        let new_crcr = op.read_crcr();
        println!(
            "xHCI: command ring di-restart ({reason}, CRCR lama={crcr:#018x}, baru={new_crcr:#018x}, USBSTS={usbsts:#010x})"
        );
        if !running {
            println!("xHCI: controller tetap halt setelah restart command ring");
            CONTROLLER_RESET_REQUIRED.store(true, Ordering::SeqCst);
        }
        running
    }
}

/// Full Host Controller Reset + re-init after the command ring could not be
/// restarted. HCRST invalidates every slot and device context, so the caller
/// must unbind the USB class drivers before calling this.
pub unsafe fn reset_controller() -> bool {
    unsafe {
        let Some(instance) = XHCI_INSTANCE.get() else {
            return false;
        };
        let mut guard = instance.lock();
        let enable_interrupts = !MANUAL_POLLING.load(Ordering::SeqCst);
        println!("xHCI: reset controller penuh untuk memulihkan command ring");
        init_xhci(&mut guard, enable_interrupts);
        let running = guard.op_regs().read_usbsts() & USBSTS_HCH == 0;
        COMPLETION_PENDING.store(false, Ordering::SeqCst);
        TRANSFER_COMPLETION_PENDING.store(false, Ordering::SeqCst);
        // A reset rebinds every port, so make the idle loop re-enumerate now.
        PORT_CHANGE_PENDING.store(true, Ordering::SeqCst);
        if running {
            println!("xHCI: controller pulih setelah reset penuh");
            CONTROLLER_RESET_REQUIRED.store(false, Ordering::SeqCst);
        } else {
            println!("xHCI: reset penuh gagal, controller tetap halt");
        }
        running
    }
}

/// Poll the event ring until the completion of the command in `expected_slot`
/// arrives, returning its completion code.
///
/// `expected_slot == 0` accepts a completion for any slot (Enable Slot answers
/// with the slot it allocated). Returns `None` on timeout.
fn wait_command_completion(expected_slot: u32, mut timeout: u32) -> Option<u32> {
    loop {
        if COMPLETION_PENDING.swap(false, Ordering::SeqCst) {
            let code = COMPLETION_CODE.load(Ordering::SeqCst);
            let event_slot = COMPLETION_SLOT.load(Ordering::SeqCst);
            if command_completion_matches(expected_slot, event_slot) {
                return Some(code);
            }
            // Late completion for another command: keep waiting for ours.
            continue;
        }
        poll_event_ring_once();
        timeout -= 1;
        if timeout == 0 {
            return None;
        }
    }
}

/// Whether a Transfer Event belongs to the EP0 transfer of `slot_id`.
///
/// DCI 1 is EP0 and a completion without a slot id or endpoint id is accepted,
/// because some controllers leave those fields clear; every other transfer event
/// belongs to a different device (or a class endpoint) and must be skipped.
fn ep0_completion_matches(slot_id: u32, event_slot: u32, dci: u32) -> bool {
    (event_slot == 0 || event_slot == slot_id) && (dci == 0 || dci == 1)
}

/// Wait for the EP0 (control transfer) completion of `slot_id`.
///
/// Transfer events are matched by slot instead of "whatever finished last":
/// when a device is unplugged mid-enumeration its late completion used to be
/// picked up by the next device's first descriptor read, which then parsed a
/// garbage descriptor and failed enumeration.
fn wait_ep0_transfer(slot_id: u32, timeout: &mut u32) -> Option<(u32, u32)> {
    loop {
        if TRANSFER_COMPLETION_PENDING.swap(false, Ordering::SeqCst) {
            let code = TRANSFER_COMPLETION_CODE.load(Ordering::SeqCst);
            let residual = TRANSFER_COMPLETION_LENGTH.load(Ordering::SeqCst);
            let event_slot = TRANSFER_COMPLETION_SLOT.load(Ordering::SeqCst);
            let dci = TRANSFER_COMPLETION_DCI.load(Ordering::SeqCst);
            if ep0_completion_matches(slot_id, event_slot, dci) {
                return Some((code, residual));
            }
            continue;
        }
        poll_event_ring_once();
        *timeout -= 1;
        if *timeout == 0 {
            return None;
        }
        core::hint::spin_loop();
    }
}

fn print_controller_state(context: &str) {
    let Some(instance) = XHCI_INSTANCE.get() else {
        return;
    };
    unsafe {
        let controller = instance.lock();
        let op = controller.op_regs();
        let interrupter = controller.interrupter0();
        println!(
            "{context}: manual_poll={} USBCMD={:#010x} USBSTS={:#010x} CRCR={:#018x} IMAN={:#010x} ERDP={:#018x} cmd_ring={:#018x} cmd_index={} event_ring={:#018x} event_index={} event_cycle={}",
            MANUAL_POLLING.load(Ordering::SeqCst),
            op.read_usbcmd(),
            op.read_usbsts(),
            op.read_crcr(),
            interrupter.read_iman(),
            interrupter.read_erdp(),
            controller.cmd_ring_phys,
            controller.cmd_enqueue_index,
            controller.event_ring_phys,
            controller.dequeue_index,
            controller.cycle_state,
        );
    }
}

pub unsafe fn enable_slot() -> Result<u32, &'static str> {
    unsafe {
        // Two attempts: a controller that stopped consuming commands recovers
        // from a command-ring restart, and retrying once turns that into a
        // successful enumeration instead of a dead USB bus.
        for attempt in 0..2u32 {
            COMPLETION_PENDING.store(false, Ordering::SeqCst);

            core::arch::asm!("cli");
            {
                let mut guard = XHCI_INSTANCE.get().unwrap().lock();
                guard.send_command(0, 0, TRB_TYPE_ENABLE_SLOT, 0);
            } // lock dilepas sebelum sti
            core::arch::asm!("sti");

            if let Some(code) = wait_command_completion(0, 2_000_000) {
                return if code == 1 {
                    Ok(COMPLETION_SLOT.load(Ordering::SeqCst))
                } else {
                    Err("Command completion code bukan Success")
                };
            }

            if attempt == 0 && unsafe { recover_command_ring("Enable Slot timeout") } {
                continue;
            }
            print_controller_state("xHCI Enable Slot timeout");
            return Err("Timeout menunggu Enable Slot");
        }
        Err("Timeout menunggu Enable Slot")
    }
}

/// Lawan dari `enable_slot()` -- dipanggil waktu device dicabut (hotplug
/// disconnect). Ngirim Disable Slot command lalu, kalau sukses, bersihin
/// entry DCBAA milik slot itu (supaya controller/software lain tidak
/// nganggep slot ini masih valid).
///
/// CATATAN (belum ditangani): device context, input context, dan
/// transfer ring yang dulu dialokasikan buat slot ini (di
/// `address_device`/`configure_endpoint`) sengaja TIDAK di-`Box::from_raw`
/// balik di sini -- physical/virtual address-nya sudah tidak disimpan
/// di mana pun setelah fungsi-fungsi itu return (cuma dikirim ke
/// controller lewat DMA). Artinya tiap disconnect+reconnect sekarang
/// masih leak memory sebesar device context + input context + EP0 ring
/// per device. Cukup aman buat sekarang (heap kernel ini masih longgar),
/// tapi kalau nanti butuh hotplug berulang kali dalam jumlah besar,
/// `address_device`/`configure_endpoint` perlu direvisi supaya nyimpen
/// alamat itu (mis. di `XHCI` struct per-slot) biar bisa dibebaskan di
/// sini.
pub unsafe fn disable_slot(slot_id: u32) -> Result<(), &'static str> {
    unsafe {
        // Two attempts, like `enable_slot`: the first timeout is often a wedged
        // command ring, and once the ring has been restarted the very same
        // command goes through.
        for attempt in 0..2u32 {
            COMPLETION_PENDING.store(false, Ordering::SeqCst);

            core::arch::asm!("cli");
            {
                let mut guard = XHCI_INSTANCE.get().unwrap().lock();
                guard.send_command(0, 0, TRB_TYPE_DISABLE_SLOT, slot_id);
            }
            core::arch::asm!("sti");

            match wait_command_completion(slot_id, 10_000_000) {
                Some(1) | Some(4) => break, // 1 = Success, 4 = Slot Not Enabled (sudah nonaktif)
                Some(_) => return Err("Disable Slot gagal, completion code bukan Success"),
                None => {
                    if attempt == 0 && recover_command_ring("Disable Slot timeout") {
                        continue;
                    }
                    print_controller_state("xHCI Disable Slot timeout");
                    // Two timeouts in a row mean the controller is not
                    // executing commands at all anymore. Only a full HCRST
                    // clears that, and it also frees the slot we could not
                    // disable -- `usb::poll_hotplug` runs it on the next pass.
                    println!("xHCI: controller perlu reset penuh setelah Disable Slot timeout");
                    CONTROLLER_RESET_REQUIRED.store(true, Ordering::SeqCst);
                    return Err("Timeout menunggu Disable Slot completion");
                }
            }
        }

        let dcbaa_virt = XHCI_INSTANCE.get().unwrap().lock().dcbaa_virt;
        (dcbaa_virt as *mut u64)
            .add(slot_id as usize)
            .write_volatile(0);

        println!("Disable Slot sukses untuk slot {}", slot_id);
        Ok(())
    }
}

// Context struct di bawah ini adalah layout memori yang dibaca xHCI
// controller lewat DMA (Slot/Endpoint/Input Context) -- field-nya
// ditulis dari Rust tapi "dibaca" oleh hardware, bukan oleh kode Rust,
// jadi banyak yang kena dead_code kalau tidak di-allow.
#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy)]
struct SlotContext {
    dword0: u32, // Route String(0-19) | Speed(20-23) | MTT(25) | Hub(26) | Context Entries(27-31)
    dword1: u32, // Max Exit Latency(0-15) | Root Hub Port Number(16-23) | Number of Ports(24-31)
    dword2: u32, // TT Hub Slot ID | TT Port Number | TTT | Interrupter Target(22-31)
    dword3: u32, // USB Device Address(0-7) | Slot State(27-31)
    reserved: [u32; 4],
}

const MAX_CONTEXT_SIZE: usize = 64;
const INPUT_CONTEXT_COUNT: usize = 33;

#[repr(C, align(4096))]
struct ContextBuffer([u8; MAX_CONTEXT_SIZE * INPUT_CONTEXT_COUNT]);

impl ContextBuffer {
    fn zeroed() -> Self {
        Self([0; MAX_CONTEXT_SIZE * INPUT_CONTEXT_COUNT])
    }

    fn write_dword(&mut self, context_size: usize, context_index: usize, dword: usize, value: u32) {
        let offset = context_size * context_index + dword * core::mem::size_of::<u32>();
        self.0[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
}

const CONTROL_RING_SIZE: usize = 64;

#[repr(C, align(64))]
struct TransferRing {
    trbs: [Trb; CONTROL_RING_SIZE],
}

impl TransferRing {
    fn new() -> Self {
        Self {
            trbs: [Trb::empty(); CONTROL_RING_SIZE],
        }
    }
}

#[repr(C, align(4096))]
struct ControlDmaBuffer<const SIZE: usize>([u8; SIZE]);

/// Copy device-written DMA bytes into ordinary Rust memory.
///
/// # Safety
/// `source` must point to at least `destination.len()` readable bytes.
pub unsafe fn copy_dma_to_slice(source: *const u8, destination: &mut [u8]) {
    compiler_fence(Ordering::Acquire);
    for (index, byte) in destination.iter_mut().enumerate() {
        *byte = unsafe { source.add(index).read_volatile() };
    }
}

pub unsafe fn address_device(slot_id: u32, root_port: u8, speed: u8) -> Result<(), &'static str> {
    unsafe {
        let ep0_ring_box: Box<TransferRing> = Box::new(TransferRing::new());
        let ep0_ring_virt = Box::into_raw(ep0_ring_box) as u64;
        let ep0_ring_phys = memory::virtual_to_physical(ep0_ring_virt as usize)
            .ok_or("gagal translate ep0 ring")? as u64;
        write_link_trb(ep0_ring_virt, ep0_ring_phys, CONTROL_RING_SIZE, true);

        let dev_ctx_box: Box<ContextBuffer> = Box::new(ContextBuffer::zeroed());
        let dev_ctx_virt = Box::into_raw(dev_ctx_box) as u64;
        let dev_ctx_phys = memory::virtual_to_physical(dev_ctx_virt as usize)
            .ok_or("gagal translate device context")? as u64;

        let (dcbaa_virt, context_size) = {
            let guard = XHCI_INSTANCE.get().unwrap().lock();
            (guard.dcbaa_virt, guard.context_size)
        };
        let dcbaa_ptr = dcbaa_virt as *mut u64;
        dcbaa_ptr.add(slot_id as usize).write_volatile(dev_ctx_phys);

        let mut input_ctx = Box::new(ContextBuffer::zeroed());
        input_ctx.write_dword(context_size, 0, 1, 0b11);
        input_ctx.write_dword(context_size, 1, 0, (1u32 << 27) | ((speed as u32) << 20));
        input_ctx.write_dword(context_size, 1, 1, (root_port as u32) << 16);

        let max_packet_size: u32 = match speed {
            4 | 5 => 512,
            3 => 64,
            2 => 8,
            _ => 64, // full speed: 8/16/32/64, corrected by update_ep0_max_packet
        };

        input_ctx.write_dword(
            context_size,
            2,
            1,
            (4 << 3) | (3 << 1) | (max_packet_size << 16),
        );
        input_ctx.write_dword(context_size, 2, 2, (ep0_ring_phys as u32 & !0xF) | 1);
        input_ctx.write_dword(context_size, 2, 3, (ep0_ring_phys >> 32) as u32);
        input_ctx.write_dword(context_size, 2, 4, 8);

        let input_ctx_virt = Box::into_raw(input_ctx) as u64;
        let input_ctx_phys = memory::virtual_to_physical(input_ctx_virt as usize)
            .ok_or("gagal translate input context")? as u64;
        COMPLETION_PENDING.store(false, Ordering::SeqCst);

        core::arch::asm!("cli");
        {
            let mut guard = XHCI_INSTANCE.get().unwrap().lock();
            guard.ep0_ring_virt = ep0_ring_virt;
            guard.ep0_ring_phys = ep0_ring_phys;
            guard.ep0_enqueue_index = 0;
            guard.ep0_cycle_state = true;
            guard.send_command(input_ctx_phys, 0, TRB_TYPE_ADDRESS_DEVICE, slot_id);
        } // <-- lock dilepas DI SINI, sebelum sti
        core::arch::asm!("sti");

        let mut timeout = 1_000_000;
        loop {
            if COMPLETION_PENDING.load(Ordering::SeqCst) {
                let code = COMPLETION_CODE.load(Ordering::SeqCst);
                return if code == 1 {
                    println!("Address Device sukses untuk slot {}", slot_id);
                    // 2 ms after SET_ADDRESS before the first control transfer.
                    delay_milliseconds(2);
                    Ok(())
                } else {
                    Err("Address Device gagal, completion code bukan Success")
                };
            }
            poll_event_ring_once();
            timeout -= 1;
            if timeout == 0 {
                return Err("Timeout menunggu Address Device completion");
            }
        }
    }
}

// --- Get Device Descriptor lewat Control Transfer di EP0 ---

const TRB_TYPE_SETUP_STAGE: u32 = 2;
const TRB_TYPE_DATA_STAGE: u32 = 3;
const TRB_TYPE_STATUS_STAGE: u32 = 4;
const TRB_IDT: u32 = 1 << 6;
const TRB_ISP: u32 = 1 << 2;

fn setup_stage_control(transfer_type: u32) -> u32 {
    (TRB_TYPE_SETUP_STAGE << 10) | TRB_IDT | (transfer_type << 16)
}

fn data_stage_control(is_in: bool) -> u32 {
    (TRB_TYPE_DATA_STAGE << 10) | if is_in { TRB_ISP | (1 << 16) } else { 0 }
}

#[cfg(test)]
mod control_trb_tests {
    use super::*;

    #[test]
    fn endpoint_interval_matches_link_speed() {
        assert_eq!(xhci_endpoint_interval(3, 3, 4), 3); // HS exponent
        assert_eq!(xhci_endpoint_interval(1, 3, 10), 6); // FS 10 ms -> 8 ms
        assert_eq!(xhci_endpoint_interval(2, 3, 8), 6); // LS 8 ms
        assert_eq!(xhci_endpoint_interval(1, 3, 1), 3); // 1 ms -> 8 microframes
    }

    #[test]
    fn configure_endpoint_add_flags_live_in_dword_one() {
        let mut ctx = ContextBuffer::zeroed();
        ctx.write_dword(64, 0, 0, 0);
        ctx.write_dword(64, 0, 1, 0b11001);
        assert_eq!(u32::from_le_bytes(ctx.0[0..4].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(ctx.0[4..8].try_into().unwrap()), 0b11001);
    }

    #[test]
    fn context_fields_use_the_controller_context_stride() {
        for context_size in [32, 64] {
            let mut context = ContextBuffer::zeroed();
            context.write_dword(context_size, 0, 1, 0x11);
            context.write_dword(context_size, 1, 0, 0x22);
            context.write_dword(context_size, 2, 1, 0x33);

            for (context_index, dword_index, expected) in [(0, 1, 0x11), (1, 0, 0x22), (2, 1, 0x33)]
            {
                let offset = context_size * context_index + dword_index * 4;
                let actual = u32::from_le_bytes(context.0[offset..offset + 4].try_into().unwrap());
                assert_eq!(actual, expected);
            }
        }
    }

    #[test]
    fn input_context_buffer_fits_the_last_endpoint_at_64_byte_stride() {
        let mut context = ContextBuffer::zeroed();
        context.write_dword(64, 32, 4, 0x55);

        let offset = 64 * 32 + 4 * 4;
        assert_eq!(
            u32::from_le_bytes(context.0[offset..offset + 4].try_into().unwrap()),
            0x55
        );
    }

    #[test]
    fn port_change_acknowledgment_covers_only_w1c_status_bits() {
        assert_eq!(PORTSC_CHANGE_BITS, 0xFF << 17);
    }

    #[test]
    fn restarted_command_ring_keeps_the_cycle_bit_and_the_aligned_base() {
        let ring_phys = 0x0000_0000_6f8b_4041u64;
        let crcr = command_ring_crcr(ring_phys);
        assert_eq!(crcr & 0x3F, 1);
        assert_eq!(crcr & !0x3F, 0x0000_0000_6f8b_4040);
        assert_eq!(crcr, (ring_phys & !0x3F) | 1);
    }

    #[test]
    fn ep0_completions_are_only_taken_for_the_device_that_asked() {
        assert!(ep0_completion_matches(5, 5, 1));
        assert!(ep0_completion_matches(5, 0, 1));
        assert!(ep0_completion_matches(5, 5, 0));
        assert!(!ep0_completion_matches(5, 4, 1));
        assert!(!ep0_completion_matches(5, 5, 2));
    }

    #[test]
    fn command_completion_matching_skips_other_slots_only_when_known() {
        assert!(command_completion_matches(0, 7));
        assert!(command_completion_matches(3, 3));
        assert!(command_completion_matches(3, 0));
        assert!(!command_completion_matches(3, 4));
    }

    #[test]
    fn control_transfer_stages_use_xhci_control_trb_fields() {
        let setup_in = setup_stage_control(3);
        assert_eq!(setup_in & (1 << 4), 0);
        assert_ne!(setup_in & TRB_IDT, 0);
        assert_eq!((setup_in >> 16) & 0x3, 3);

        let data_in = data_stage_control(true);
        assert_eq!(data_in & (1 << 4), 0);
        assert_ne!(data_in & TRB_ISP, 0);
        assert_ne!(data_in & (1 << 16), 0);

        let data_out = data_stage_control(false);
        assert_eq!(data_out & (1 << 4), 0);
        assert_eq!(data_out & TRB_ISP, 0);
        assert_eq!(data_out & (1 << 16), 0);
    }

    #[test]
    fn setup_stage_encodes_out_data_and_no_data_requests() {
        assert_eq!((setup_stage_control(2) >> 16) & 0x3, 2);
        assert_eq!((setup_stage_control(0) >> 16) & 0x3, 0);
    }

    #[test]
    fn extended_capability_next_pointer_is_dword_relative() {
        assert_eq!(next_extended_capability_offset(0x0000_0500), Some(20));
        assert_eq!(next_extended_capability_offset(0), None);
    }

    #[test]
    fn port_reset_uses_warm_reset_for_superspeed() {
        assert_eq!(port_reset_command(4), PORTSC_WPR);
        assert_eq!(port_reset_command(3), PORTSC_PR);
    }

    #[test]
    fn dma_copy_reads_current_bytes() {
        let mut source = [0u8; 4];
        for (index, byte) in [0x12, 0x34, 0x56, 0x78].into_iter().enumerate() {
            unsafe { source.as_mut_ptr().add(index).write_volatile(byte) };
        }

        let mut destination = [0; 4];
        unsafe { copy_dma_to_slice(source.as_ptr(), &mut destination) };
        assert_eq!(destination, [0x12, 0x34, 0x56, 0x78]);
    }
}

const USB_REQ_GET_DESCRIPTOR: u8 = 0x06;
const USB_DESC_TYPE_DEVICE: u8 = 0x01;

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
// Sebagian field (bcd_usb, manufacturer_index, dst) belum dibaca kode
// Rust sekarang, disimpan lengkap sesuai layout spec USB Device
// Descriptor (18 byte) supaya `read_unaligned()` benar.
#[allow(dead_code)]
pub struct DeviceDescriptor {
    pub length: u8,
    pub descriptor_type: u8,
    pub bcd_usb: u16,
    pub device_class: u8,
    pub device_subclass: u8,
    pub device_protocol: u8,
    pub max_packet_size0: u8,
    pub vendor_id: u16,
    pub product_id: u16,
    pub bcd_device: u16,
    pub manufacturer_index: u8,
    pub product_index: u8,
    pub serial_number_index: u8,
    pub num_configurations: u8,
}

/// Kirim GET_DESCRIPTOR (Device Descriptor) lewat control transfer di EP0.
/// Wajib dipanggil SETELAH address_device() sukses untuk slot yang sama.

pub unsafe fn get_device_descriptor(slot_id: u32) -> Result<DeviceDescriptor, &'static str> {
    unsafe {
        // Buffer buat nampung hasil descriptor (18 byte standar)
        let buf_box = Box::new(ControlDmaBuffer([0u8; 18]));
        let buf_virt = Box::into_raw(buf_box) as u64;
        let buf_phys = memory::virtual_to_physical(buf_virt as usize)
            .ok_or("gagal translate buffer descriptor")? as u64;

        // Setup packet standar USB (8 byte), di-pack jadi satu u64 (little-endian,
        // sesuai urutan field asli): bmRequestType | bRequest | wValue | wIndex | wLength
        let bm_request_type: u64 = 0x80; // Device-to-Host, Standard, Device
        let b_request: u64 = USB_REQ_GET_DESCRIPTOR as u64;
        let w_value: u64 = (USB_DESC_TYPE_DEVICE as u64) << 8; // descriptor index 0
        let w_index: u64 = 0;
        let w_length: u64 = 18;

        let setup_parameter: u64 = bm_request_type
            | (b_request << 8)
            | (w_value << 16)
            | (w_index << 32)
            | (w_length << 48);

        TRANSFER_COMPLETION_PENDING.store(false, Ordering::SeqCst);

        core::arch::asm!("cli");
        {
            let mut guard = XHCI_INSTANCE.get().unwrap().lock();

            // Satu control transfer = setup + data + status (3 TRB) dan harus
            // kontigu, jadi pastikan ruangnya dulu sebelum menulis apa pun.
            guard.ep0_make_room(3);

            // --- Setup Stage TRB ---
            // IDT=1 (Immediate Data, parameter berisi data langsung bukan pointer)
            // TRT=3 (IN Data Stage, karena GET_DESCRIPTOR = device-to-host)
            let setup_status = 8u32; // TRB Transfer Length selalu 8 untuk setup packet
            let setup_control = setup_stage_control(3);
            guard.ep0_enqueue_trb(setup_parameter, setup_status, setup_control);

            // --- Data Stage TRB ---
            // DIR=1 (IN, device mengirim data descriptor ke kita)
            let data_status = 18u32; // TRB Transfer Length = wLength
            let data_control = data_stage_control(true);
            guard.ep0_enqueue_trb(buf_phys, data_status, data_control);

            // --- Status Stage TRB ---
            // DIR=0 (OUT, kebalikan dari data stage) | IOC=1 (biar dapet completion event)
            let status_control = (TRB_TYPE_STATUS_STAGE << 10) | (1 << 5);
            guard.ep0_enqueue_trb(0, 0, status_control);

            // Ring doorbell buat slot ini, target=1 artinya EP0 (default control endpoint)
            guard.ring_doorbell(slot_id as u8, 1);
        } // <-- lock dilepas sebelum sti
        core::arch::asm!("sti");

        let mut timeout = 1_000_000;
        let Some((code, _)) = wait_ep0_transfer(slot_id, &mut timeout) else {
            return Err("Timeout menunggu Get Device Descriptor completion");
        };
        // 1 = Success, 13 = Short Packet (device kirim kurang dari wLength,
        // tetap valid selama field-field awal descriptor sudah kebaca)
        if code != 1 && code != 13 {
            return Err("Get Device Descriptor gagal, completion code bukan Success");
        }

        let mut descriptor_bytes = [0; 18];
        copy_dma_to_slice(buf_virt as *const u8, &mut descriptor_bytes);
        let descriptor =
            core::ptr::read_unaligned(descriptor_bytes.as_ptr().cast::<DeviceDescriptor>());

        let device_class = descriptor.device_class;
        let vendor_id = descriptor.vendor_id;
        let product_id = descriptor.product_id;
        let max_packet_size0 = descriptor.max_packet_size0;
        let num_configurations = descriptor.num_configurations;

        if descriptor.length != core::mem::size_of::<DeviceDescriptor>() as u8
            || descriptor.descriptor_type != USB_DESC_TYPE_DEVICE
            || num_configurations == 0
        {
            println!(
                "Invalid USB device descriptor: length={} type={:#04x} vendor={:#06x} product={:#06x} configurations={}",
                descriptor.length,
                descriptor.descriptor_type,
                vendor_id,
                product_id,
                num_configurations
            );
            return Err("invalid USB device descriptor");
        }

        println!(
            "Device Descriptor: class={:#04x} vendor={:#06x} product={:#06x} max_packet_size0={} num_configs={}",
            device_class, vendor_id, product_id, max_packet_size0, num_configurations
        );

        Ok(descriptor)
    }
}

pub unsafe fn control_transfer_in(
    slot_id: u32,
    bm_request_type: u8,
    b_request: u8,
    w_value: u16,
    w_index: u16,
    buf_phys: u64,
    w_length: u16,
) -> Result<u32, &'static str> {
    unsafe {
        let setup_parameter: u64 = (bm_request_type as u64)
            | ((b_request as u64) << 8)
            | ((w_value as u64) << 16)
            | ((w_index as u64) << 32)
            | ((w_length as u64) << 48);

        TRANSFER_COMPLETION_PENDING.store(false, Ordering::SeqCst);

        core::arch::asm!("cli");
        {
            let mut guard = XHCI_INSTANCE.get().unwrap().lock();
            guard.ep0_make_room(3);

            let setup_control = setup_stage_control(3);
            guard.ep0_enqueue_trb(setup_parameter, 8, setup_control);

            let data_status = (w_length as u32) & 0x1_FFFF;
            let data_control = data_stage_control(true);
            guard.ep0_enqueue_trb(buf_phys, data_status, data_control);

            let status_control = (TRB_TYPE_STATUS_STAGE << 10) | (1 << 5);
            guard.ep0_enqueue_trb(0, 0, status_control);

            guard.ring_doorbell(slot_id as u8, 1);
        }

        core::arch::asm!("sti");

        let mut timeout = 1_000_000;
        let Some((code, residual)) = wait_ep0_transfer(slot_id, &mut timeout) else {
            return Err("timeout menunggu control transfer completion");
        };
        if code != 1 && code != 13 {
            return Err("Control transfer gagal, completion code bukan Success");
        }
        if code == 13 {
            let _ = wait_ep0_transfer(slot_id, &mut timeout);
        }
        return Ok((w_length as u32).saturating_sub(residual));
    }
}

pub unsafe fn control_transfer_out(
    slot_id: u32,
    bm_request_type: u8,
    b_request: u8,
    w_value: u16,
    w_index: u16,
    data: &[u8],
) -> Result<(), &'static str> {
    if data.is_empty() || data.len() > u16::MAX as usize {
        return Err("invalid control transfer length");
    }

    unsafe {
        let buf_phys = memory::virtual_to_physical(data.as_ptr() as usize)
            .ok_or("gagal translate control OUT buffer")? as u64;
        let setup_parameter: u64 = (bm_request_type as u64)
            | ((b_request as u64) << 8)
            | ((w_value as u64) << 16)
            | ((w_index as u64) << 32)
            | ((data.len() as u64) << 48);

        TRANSFER_COMPLETION_PENDING.store(false, Ordering::SeqCst);

        core::arch::asm!("cli");
        {
            let mut guard = XHCI_INSTANCE.get().unwrap().lock();
            guard.ep0_make_room(3);
            let setup_control = setup_stage_control(2);
            guard.ep0_enqueue_trb(setup_parameter, 8, setup_control);

            let data_control = data_stage_control(false);
            guard.ep0_enqueue_trb(buf_phys, data.len() as u32, data_control);

            let status_control = (TRB_TYPE_STATUS_STAGE << 10) | (1 << 5) | (1 << 16);
            guard.ep0_enqueue_trb(0, 0, status_control);
            guard.ring_doorbell(slot_id as u8, 1);
        }
        core::arch::asm!("sti");

        let mut timeout = 1_000_000;
        let Some((code, residual)) = wait_ep0_transfer(slot_id, &mut timeout) else {
            return Err("Control OUT transfer timed out");
        };
        if code != 1 && code != 13 {
            return Err("Control OUT transfer failed");
        }
        return if residual == 0 {
            Ok(())
        } else {
            Err("Control OUT transfer was incomplete")
        };
    }
}

const USB_DESC_TYPE_CONFIGURATION: u8 = 0x02;
const USB_DESC_TYPE_INTERFACE: u8 = 0x04;
const USB_DESC_TYPE_ENDPOINT: u8 = 0x05;

/// Ambil Configuration Descriptor lalu kembalikan semua endpoint di semua
/// interface. Caller memilih endpoint yang cocok untuk driver device-class.
pub unsafe fn get_configuration_descriptor_endpoints(
    slot_id: u32,
) -> Result<alloc::vec::Vec<EndpointInfo>, &'static str> {
    unsafe {
        const BUF_SIZE: usize = 256;
        const CONFIG_HEADER_SIZE: u16 = 9;
        let buf_box = Box::new(ControlDmaBuffer([0u8; BUF_SIZE]));
        let buf_virt = Box::into_raw(buf_box) as u64;
        let buf_phys = memory::virtual_to_physical(buf_virt as usize)
            .ok_or("gagal translate buffer config descriptor")? as u64;

        let w_value: u16 = (USB_DESC_TYPE_CONFIGURATION as u16) << 8;

        let header_len = control_transfer_in(
            slot_id,
            0x80,
            USB_REQ_GET_DESCRIPTOR,
            w_value,
            0,
            buf_phys,
            CONFIG_HEADER_SIZE,
        )?;

        if header_len < CONFIG_HEADER_SIZE as u32 {
            return Err("truncated USB configuration descriptor header");
        }

        let mut header = [0; 9];
        copy_dma_to_slice(buf_virt as *const u8, &mut header);
        if header[0] < CONFIG_HEADER_SIZE as u8 || header[1] != USB_DESC_TYPE_CONFIGURATION {
            println!(
                "Invalid USB configuration header: length={} type={:#04x}",
                header[0], header[1]
            );
            return Err("invalid USB configuration descriptor header");
        }
        let total_length = u16::from_le_bytes([header[2], header[3]]) as usize;
        if !(CONFIG_HEADER_SIZE as usize..=BUF_SIZE).contains(&total_length) {
            println!("Unsupported USB configuration length: {}", total_length);
            return Err("invalid USB configuration descriptor total length");
        }

        let actual_len = control_transfer_in(
            slot_id,
            0x80,
            USB_REQ_GET_DESCRIPTOR,
            w_value,
            0,
            buf_phys,
            total_length as u16,
        )?;
        if actual_len < total_length as u32 {
            return Err("truncated USB configuration descriptor");
        }
        println!("Configuration Descriptor: {} byte diterima", actual_len);

        let mut descriptor_bytes = [0; BUF_SIZE];
        copy_dma_to_slice(
            buf_virt as *const u8,
            &mut descriptor_bytes[..actual_len as usize],
        );
        let buf_slice = &descriptor_bytes[..actual_len as usize];

        let mut i = 0usize;
        let mut current_interface: u8 = 0;
        let mut current_interface_class: u8 = 0;
        let mut current_interface_subclass: u8 = 0;
        let mut current_interface_protocol: u8 = 0;
        let mut found: alloc::vec::Vec<EndpointInfo> = alloc::vec::Vec::new();

        while i + 2 <= buf_slice.len() {
            let b_length = buf_slice[i] as usize;
            let b_descriptor_type = buf_slice[i + 1];

            if b_length < 2 || i + b_length > buf_slice.len() {
                return Err("invalid USB configuration descriptor length");
            }

            if b_descriptor_type == USB_DESC_TYPE_CONFIGURATION {
                if b_length < CONFIG_HEADER_SIZE as usize {
                    return Err("invalid USB configuration descriptor");
                }
            } else if b_descriptor_type == USB_DESC_TYPE_INTERFACE {
                if b_length < 9 {
                    return Err("invalid USB interface descriptor");
                }
                current_interface = buf_slice[i + 2];
                current_interface_class = buf_slice[i + 5];
                current_interface_subclass = buf_slice[i + 6];
                current_interface_protocol = buf_slice[i + 7];
                println!(
                    "  Interface {} class={:#04x} subclass={:#04x} protocol={:#04x}",
                    current_interface,
                    current_interface_class,
                    current_interface_subclass,
                    current_interface_protocol
                );
            } else if b_descriptor_type == USB_DESC_TYPE_ENDPOINT {
                if b_length < 7 {
                    return Err("invalid USB endpoint descriptor");
                }
                let address = buf_slice[i + 2];
                let attributes = buf_slice[i + 3];
                let max_packet_size = (buf_slice[i + 4] as u16) | ((buf_slice[i + 5] as u16) << 8);
                let interval = buf_slice[i + 6];

                println!(
                    "   Endpoint addr={:#04x} attr={:#04x} max_packet_size={} interval={}",
                    address, attributes, max_packet_size, interval
                );

                found.push(EndpointInfo {
                    address,
                    attributes,
                    max_packet_size,
                    interval,
                    inteface_number: current_interface,
                    interface_subclass: current_interface_subclass,
                    interface_class: current_interface_class,
                    interface_protocol: current_interface_protocol,
                });
            }
            i += b_length;
        }

        if found.is_empty() {
            Err("Tidak ketemu endpoint di Configuration Descriptor")
        } else {
            Ok(found)
        }
    }
}

pub unsafe fn control_transfer_no_data(
    slot_id: u32,
    bm_request_type: u8,
    b_request: u8,
    w_value: u16,
    w_index: u16,
) -> Result<(), &'static str> {
    unsafe {
        let setup_parameter: u64 = (bm_request_type as u64)
            | ((b_request as u64) << 8)
            | ((w_value as u64) << 16)
            | ((w_index as u64) << 32);

        TRANSFER_COMPLETION_PENDING.store(false, Ordering::SeqCst);

        core::arch::asm!("cli");
        {
            let mut guard = XHCI_INSTANCE.get().unwrap().lock();

            // Setup + status saja (2 TRB), tapi tetap harus kontigu.
            guard.ep0_make_room(2);

            let setup_control = setup_stage_control(0);
            guard.ep0_enqueue_trb(setup_parameter, 8, setup_control);

            let status_control = (TRB_TYPE_STATUS_STAGE << 10) | (1 << 5) | (1 << 16);
            guard.ep0_enqueue_trb(0, 0, status_control);
            guard.ring_doorbell(slot_id as u8, 1);

            println!(
                "DEBUG: ep0 enqueue_index={} cycle_state={}",
                guard.ep0_enqueue_index, guard.ep0_cycle_state
            );
        }

        core::arch::asm!("sti");

        let mut timeout = 1_000_000;
        let Some((code, _residual)) = wait_ep0_transfer(slot_id, &mut timeout) else {
            return Err("Timeout control transfer no data");
        };
        return if code == 1 {
            Ok(())
        } else {
            Err("Control Transfer (no data gagal, completion code bukan Success")
        };
    }
}

/// Handle ke endpoint interrupt yang sudah dikonfigurasi: ring TRB miliknya
/// (virt/phys) plus DCI-nya. Generic untuk endpoint HID apa pun -- device-class
/// driver (keyboard_usb.rs, mouse_usb.rs, dst) yang menyimpan & memakainya.
#[derive(Debug, Clone, Copy)]
pub struct EndpointHandle {
    pub ring_virt: u64,
    pub ring_phys: u64,
    pub dci: u32,
    enqueue_index: usize,
    cycle_state: bool,
}

/// Configure every endpoint of one device in a single `Configure Endpoint`
/// command.
///
/// One command per endpoint is not equivalent: the command treats every
/// endpoint context whose Added flag is clear as "not part of this
/// configuration", so a second command for EP1 IN drops the EP1 OUT added by
/// the first. On real phones that showed up as the first endpoint configuring
/// fine and the next command never completing, which killed RNDIS and NCM
/// bring-up. Everything therefore goes into one input context.
pub unsafe fn configure_endpoints(
    slot_id: u32,
    root_port: u8,
    endpoints: &[EndpointInfo],
) -> Result<Vec<EndpointHandle>, &'static str> {
    unsafe {
        if endpoints.is_empty() {
            return Err("no endpoints to configure");
        }
        if endpoints.len() > 15 {
            return Err("too many endpoints for one configure command");
        }

        let (context_size, device_context, slot_context, portsc) = {
            let guard = XHCI_INSTANCE.get().unwrap().lock();
            let context_size = guard.context_size;
            let dcbaa_entry = (guard.dcbaa_virt as *const u64)
                .add(slot_id as usize)
                .read_volatile();
            if dcbaa_entry == 0 {
                return Err("device context tidak tersedia");
            }
            let device_context = memory::physical_to_virtual(dcbaa_entry as usize)
                .ok_or("gagal translate device context")?;
            let slot_context = (device_context as *const SlotContext).read_volatile();
            let portsc = if root_port == 0 {
                0
            } else {
                guard.op_regs().read_portsc(root_port - 1)
            };
            (context_size, device_context, slot_context, portsc)
        };
        let speed = ((portsc & PORTSC_SPEED_MASK) >> 10) as u8;

        let mut input_ctx = Box::new(ContextBuffer::zeroed());
        let mut added: u32 = 1 << 0;
        let mut highest_dci = 0u32;
        let mut handles = Vec::with_capacity(endpoints.len());

        for ep_info in endpoints {
            let ep_number = (ep_info.address & 0x0F) as u32;
            let is_in = ep_info.address & 0x80 != 0;
            let transfer_type = ep_info.attributes & 0x3;
            let endpoint_type = match (is_in, transfer_type) {
                (false, 1) => 1,
                (false, 2) => 2,
                (false, 3) => 3,
                (true, 1) => 5,
                (true, 2) => 6,
                (true, 3) => 7,
                _ => return Err("unsupported endpoint transfer type"),
            };
            let dci = (ep_number * 2) + if is_in { 1 } else { 0 };

            // A full- or low-speed link cannot carry 512-byte bulk packets;
            // phones that describe themselves that way get rejected with a
            // parameter error, so clamp to what the link allows.
            let max_packet_size = if speed <= 2 {
                ep_info.max_packet_size.min(64)
            } else {
                ep_info.max_packet_size
            };
            if max_packet_size != ep_info.max_packet_size {
                println!(
                    "  endpoint {:#04x}: max packet {} -> {} (speed={})",
                    ep_info.address, ep_info.max_packet_size, max_packet_size, speed
                );
            }

            let ep_ring_box: Box<TransferRing> = Box::new(TransferRing::new());
            let ep_ring_virt = Box::into_raw(ep_ring_box) as u64;
            let ep_ring_phys = memory::virtual_to_physical(ep_ring_virt as usize)
                .ok_or("gagal translate interrupt ep ring")? as u64;
            write_link_trb(ep_ring_virt, ep_ring_phys, CONTROL_RING_SIZE, true);

            let ep_context_index = dci as usize + 1;
            let interval = if transfer_type == 3 || transfer_type == 1 {
                xhci_endpoint_interval(speed, transfer_type, ep_info.interval)
            } else {
                0
            };
            input_ctx.write_dword(context_size, ep_context_index, 0, interval << 16);
            input_ctx.write_dword(
                context_size,
                ep_context_index,
                1,
                (endpoint_type << 3) | (3 << 1) | ((max_packet_size as u32) << 16),
            );
            input_ctx.write_dword(
                context_size,
                ep_context_index,
                2,
                (ep_ring_phys as u32 & !0xF) | 1,
            );
            input_ctx.write_dword(
                context_size,
                ep_context_index,
                3,
                (ep_ring_phys >> 32) as u32,
            );
            let average_trb_length = (max_packet_size as u32).max(8);
            input_ctx.write_dword(context_size, ep_context_index, 4, average_trb_length);

            added |= 1 << dci;
            highest_dci = highest_dci.max(dci);
            handles.push(EndpointHandle {
                ring_virt: ep_ring_virt,
                ring_phys: ep_ring_phys,
                dci,
                enqueue_index: 0,
                cycle_state: true,
            });
        }

        // Input Control Context: DW0 = Drop Context flags, DW1 = Add Context
        // flags. Writing `added` into DW0 made the controller report Success
        // while enabling nothing, so the first bulk transfer failed with
        // completion code 12 (Endpoint Not Enabled).
        input_ctx.write_dword(context_size, 0, 0, 0);
        input_ctx.write_dword(context_size, 0, 1, added);
        for dword in 0..8 {
            let value = (device_context as *const u32).add(dword).read_volatile();
            input_ctx.write_dword(context_size, 1, dword, value);
        }
        let current_entries = slot_context.dword0 >> 27;
        input_ctx.write_dword(
            context_size,
            1,
            0,
            ((current_entries.max(highest_dci)) << 27) | (slot_context.dword0 & 0x07FF_FFFF),
        );
        input_ctx.write_dword(
            context_size,
            1,
            1,
            (slot_context.dword1 & !0x00FF_0000) | ((root_port as u32) << 16),
        );

        let input_ctx_virt = Box::into_raw(input_ctx) as u64;
        let input_ctx_phys = memory::virtual_to_physical(input_ctx_virt as usize)
            .ok_or("gagal translate input context configure endpoint")?
            as u64;

        COMPLETION_PENDING.store(false, Ordering::SeqCst);
        COMPLETION_SLOT.store(0, Ordering::SeqCst);

        core::arch::asm!("cli");
        {
            let mut guard = XHCI_INSTANCE.get().unwrap().lock();
            guard.send_command(input_ctx_phys, 0, TRB_TYPE_CONFIGURE_ENDPOINT, slot_id);
        }
        core::arch::asm!("sti");

        let mut timeout = 1_000_000;
        loop {
            if COMPLETION_PENDING.load(Ordering::SeqCst) {
                let code = COMPLETION_CODE.load(Ordering::SeqCst);
                let event_slot = COMPLETION_SLOT.load(Ordering::SeqCst);
                COMPLETION_PENDING.store(false, Ordering::SeqCst);
                if event_slot != 0 && event_slot != slot_id {
                    println!(
                        "  Configure Endpoint: completion untuk slot {event_slot}, menunggu slot {slot_id}"
                    );
                    continue;
                }
                return if code == 1 {
                    println!(
                        "Configure Endpoint sukses (slot={slot_id}, endpoint={}, DCI tertinggi={highest_dci})",
                        endpoints.len()
                    );
                    Ok(handles)
                } else {
                    println!("  Configure Endpoint gagal: completion code {code}");
                    Err("Configure Endpoint gagal, completion code bukan Succes")
                };
            }
            // Poll the event ring directly. `poll_manual_events()` is a no-op
            // when the controller runs in interrupt mode, and the deferred
            // interrupt handler only runs from the idle loop — the very loop
            // this call blocks — so waiting on the flag alone made the second
            // Configure Endpoint spin until it timed out on real hardware.
            if let Some(instance) = XHCI_INSTANCE.get() {
                unsafe { instance.lock().poll_event_ring() };
            }
            timeout -= 1;
            if timeout == 0 {
                let usbsts = XHCI_INSTANCE
                    .get()
                    .map(|instance| unsafe { instance.lock().op_regs().read_usbsts() })
                    .unwrap_or(0);
                println!(
                    "  Configure Endpoint timeout: slot={slot_id}, speed={speed}, endpoints={}, portsc={portsc:#010x}, usbsts={usbsts:#010x}",
                    endpoints.len()
                );
                unsafe { recover_command_ring("Configure Endpoint timeout") };
                return Err("Timeout menunggu Configure Endpoint completion");
            }
        }
    }
}


/// Encode bInterval as the xHCI Interval field (period = 2^n * 125 us).
///
/// High/SuperSpeed bInterval is already an exponent (value - 1). Full/low
/// speed interrupt endpoints give milliseconds, so they must be converted:
/// 1 ms = 8 microframes, i.e. n = floor(log2(ms * 8)).
fn xhci_endpoint_interval(speed: u8, transfer_type: u8, b_interval: u8) -> u32 {
    let b = u32::from(b_interval.max(1));
    if speed >= 3 {
        return (b - 1).min(15);
    }
    if transfer_type == 1 {
        // Full-speed isochronous: bInterval is an exponent in frames.
        return ((b - 1) + 3).min(15);
    }
    let microframes = b * 8;
    (31 - microframes.leading_zeros()).clamp(3, 10)
}

/// Update the EP0 max packet size after the real value is known
/// (Evaluate Context). Needed for full-speed devices, whose EP0 size can be
/// 8/16/32/64 and is only known after the first 8 descriptor bytes.
pub unsafe fn update_ep0_max_packet(slot_id: u32, max_packet: u16) -> Result<(), &'static str> {
    unsafe {
        let context_size = XHCI_INSTANCE.get().unwrap().lock().context_size;
        let mut input_ctx = Box::new(ContextBuffer::zeroed());
        input_ctx.write_dword(context_size, 0, 1, 1 << 1); // A1 = EP0
        input_ctx.write_dword(context_size, 2, 1, (4 << 3) | (3 << 1) | ((max_packet as u32) << 16));
        let input_ctx_virt = Box::into_raw(input_ctx) as u64;
        let input_ctx_phys = memory::virtual_to_physical(input_ctx_virt as usize)
            .ok_or("gagal translate input context evaluate")? as u64;

        COMPLETION_PENDING.store(false, Ordering::SeqCst);
        core::arch::asm!("cli");
        {
            let mut guard = XHCI_INSTANCE.get().unwrap().lock();
            guard.send_command(input_ctx_phys, 0, TRB_TYPE_EVALUATE_CONTEXT, slot_id);
        }
        core::arch::asm!("sti");
        match wait_command_completion(slot_id, 10_000_000) {
            Some(1) => Ok(()),
            Some(_) => Err("Evaluate Context gagal"),
            None => Err("Timeout menunggu Evaluate Context"),
        }
    }
}

const TRB_TYPE_EVALUATE_CONTEXT: u32 = 13;

/// Configure a single endpoint. Kept for callers that only need one; it shares
/// the batching path so the Added flags of other endpoints are never cleared by
/// accident.
pub unsafe fn configure_endpoint(
    slot_id: u32,
    root_port: u8,
    ep_info: EndpointInfo,
) -> Result<EndpointHandle, &'static str> {
    let mut handles = unsafe { configure_endpoints(slot_id, root_port, &[ep_info])? };
    handles.pop().ok_or("configure endpoint returned no handle")
}

pub unsafe fn bulk_transfer(
    slot_id: u32,
    endpoint: &mut EndpointHandle,
    buffer_phys: u64,
    length: u32,
) -> Result<u32, &'static str> {
    if length == 0 || length > 0x1_FFFF {
        return Err("invalid bulk transfer length");
    }

    TRANSFER_COMPLETION_PENDING.store(false, Ordering::SeqCst);
    unsafe {
        arm_bulk_transfer(slot_id, endpoint, buffer_phys, length)?;

        let mut timeout = 1_000_000;
        loop {
            if TRANSFER_COMPLETION_PENDING.swap(false, Ordering::SeqCst) {
                let code = TRANSFER_COMPLETION_CODE.load(Ordering::SeqCst);
                let residual = TRANSFER_COMPLETION_LENGTH.load(Ordering::SeqCst);
                let event_slot = TRANSFER_COMPLETION_SLOT.load(Ordering::SeqCst);
                let event_dci = TRANSFER_COMPLETION_DCI.load(Ordering::SeqCst);
                if event_slot != slot_id || event_dci != endpoint.dci {
                    // Stale event from EP0 / another endpoint: keep waiting.
                    continue;
                }
                if code != 1 && code != 13 {
                    println!(
                        "bulk transfer gagal: slot={slot_id} dci={event_dci} completion code={code}"
                    );
                    return Err("bulk transfer failed");
                }
                return Ok(length.saturating_sub(residual));
            }
            poll_event_ring_once();
            timeout -= 1;
            if timeout == 0 {
                return Err("bulk transfer timeout");
            }
            core::hint::spin_loop();
        }
    }
}

pub unsafe fn arm_bulk_transfer(
    slot_id: u32,
    endpoint: &mut EndpointHandle,
    buffer_phys: u64,
    length: u32,
) -> Result<(), &'static str> {
    if length == 0 || length > 0x1_FFFF {
        return Err("invalid bulk transfer length");
    }

    unsafe {
        core::arch::asm!("cli");
        {
            let guard = XHCI_INSTANCE.get().unwrap().lock();
            guard.enqueue_normal_trb_and_ring(
                endpoint.ring_virt,
                endpoint.ring_phys,
                &mut endpoint.enqueue_index,
                &mut endpoint.cycle_state,
                CONTROL_RING_SIZE,
                slot_id,
                endpoint.dci,
                buffer_phys,
                length,
            );
        }
        core::arch::asm!("sti");
    }
    Ok(())
}

const TRB_TYPE_LINK: u32 = 6;

unsafe fn write_trb_cycle_last(destination: *mut Trb, trb: Trb) {
    unsafe {
        let dwords = destination.cast::<u32>();
        dwords.write_volatile(trb.parameter as u32);
        dwords.add(1).write_volatile((trb.parameter >> 32) as u32);
        dwords.add(2).write_volatile(trb.status);
        compiler_fence(Ordering::Release);
        dwords.add(3).write_volatile(trb.control);
    }
}

unsafe fn write_link_trb(ring_virt: u64, ring_phys: u64, ring_size: usize, cycle: bool) {
    unsafe {
        let trbs = ring_virt as *mut Trb;
        let control = (TRB_TYPE_LINK << 10) | (1 << 1) | (cycle as u32);
        write_trb_cycle_last(
            trbs.add(ring_size - 1),
            Trb {
                parameter: ring_phys,
                status: 0,
                control,
            },
        );
    }
}
