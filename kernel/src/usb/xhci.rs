//! Driver transport xHCI (USB 3.x host controller): setup MMIO/MSI-X,
//! reset & init controller (DCBAA, Command Ring, Event Ring), device
//! enumeration (enable slot, address device, get descriptor), sampai
//! configure endpoint. Device-class driver (mis. HID keyboard di
//! `keyboard_usb.rs`) dibangun di atas primitif-primitif di sini.

use crate::usb::keyboard_usb::{KBD_DCI, KBD_REPORT_LEN, KBD_REPORT_PENDING, KBD_SLOT_ID};
use crate::usb::mouse_usb::{MOUSE_DCI, MOUSE_REPORT_LEN, MOUSE_REPORT_PENDING, MOUSE_SLOT_ID};
use alloc::boxed::Box;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;
use spin::Once;

pub static COMPLETION_PENDING: AtomicBool = AtomicBool::new(false);
pub static COMPLETION_CODE: AtomicU32 = AtomicU32::new(0);
pub static COMPLETION_SLOT: AtomicU32 = AtomicU32::new(0);

pub static TRANSFER_COMPLETION_PENDING: AtomicBool = AtomicBool::new(false);
pub static TRANSFER_COMPLETION_CODE: AtomicU32 = AtomicU32::new(0);
pub static TRANSFER_COMPLETION_LENGTH: AtomicU32 = AtomicU32::new(0);

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

use crate::{memory, pci, println};

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

    /// Belum dipakai -- kode sekarang mengasumsikan context size 32-byte;
    /// cek ini dulu kalau mau dukung context 64-byte.
    #[allow(dead_code)]
    pub unsafe fn context_size_64(&self) -> bool {
        unsafe { self.hccparams1() >> 2 & 0x1 == 1 }
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
}

#[derive(Debug, Clone)]
pub struct XHCI {
    bar: pci::BarInfo,
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

            let table_addr = self.virt_base + info.table_offset as u64;

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
        XhciOpRegs {
            base: self
                .cap_regs()
                .base
                .add(self.cap_regs().cap_length() as usize) as *mut u8,
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
            let trb = trbs.add(self.dequeue_index).read_volatile();

            let trb_cycle_bit = (trb.control & 0x1) != 0;

            if trb_cycle_bit != self.cycle_state {
                return;
            }

            let trb_type = (trb.control >> 10) & 0x3F;

            match trb_type {
                34 => {
                    let port_id = (trb.parameter >> 24) & 0xFF;
                    println!("EVENT: Port Status Change on port {}", port_id);
                    // Jangan proses hotplug langsung di sini (lihat komentar
                    // di deklarasi PORT_CHANGE_PENDING) -- cukup kasih tanda,
                    // idle loop yang nanti diff & proses port mana yang
                    // connect/disconnect.
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
                    let residual_length = trb.status & 0xFF_FFFF; // bit 0-23
                    let endpoint_id = (trb.control >> 16) & 0x1F;
                    let event_slot_id = (trb.control >> 24) & 0xFF; // sama posisi bit dengan Command Completion (trb_type 33)

                    let kbd_dci = KBD_DCI.load(Ordering::SeqCst);
                    let kbd_slot = KBD_SLOT_ID.load(Ordering::SeqCst);
                    let mouse_dci = MOUSE_DCI.load(Ordering::SeqCst);
                    let mouse_slot = MOUSE_SLOT_ID.load(Ordering::SeqCst);

                    // PENTING: DCI itu index LOKAL per-slot (dihitung dari
                    // endpoint address device), BUKAN id unik global --
                    // keyboard & mouse boot protocol sering sama-sama pakai
                    // endpoint 0x81 sehingga DCI-nya kebetulan sama. Kalau
                    // cuma dicocokkan lewat DCI doang, event dari slot mouse
                    // bisa salah kebaca sebagai event keyboard (atau
                    // sebaliknya). Makanya slot_id WAJIB ikut dicocokkan.
                    if kbd_dci != 0 && event_slot_id == kbd_slot && endpoint_id == kbd_dci {
                        KBD_REPORT_LEN.store(residual_length, Ordering::SeqCst);
                        KBD_REPORT_PENDING.store(true, Ordering::SeqCst);
                    } else if mouse_dci != 0
                        && event_slot_id == mouse_slot
                        && endpoint_id == mouse_dci
                    {
                        MOUSE_REPORT_LEN.store(residual_length, Ordering::SeqCst);
                        MOUSE_REPORT_PENDING.store(true, Ordering::SeqCst);
                    } else {
                        println!(
                            "EVENT: Transfer Event, code={} residual={}",
                            completion_code, residual_length
                        );
                        TRANSFER_COMPLETION_CODE.store(completion_code, Ordering::SeqCst);
                        TRANSFER_COMPLETION_LENGTH.store(residual_length, Ordering::SeqCst);
                        TRANSFER_COMPLETION_PENDING.store(true, Ordering::SeqCst);
                    }
                }
                _ => {
                    println!("EVENT: unknown type={}", trb_type);
                }
            }

            self.dequeue_index += 1;
            if self.dequeue_index >= EVENT_RING_SIZE {
                self.dequeue_index = 0;
                self.cycle_state = !self.cycle_state;
            }

            let new_erdp = self.event_ring_phys + (self.dequeue_index as u64 * 16);
            let ir0 = self.interrupter0();
            ir0.write_erdp(new_erdp | (1 << 3));
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

            let mut timeout = 10_000_000;
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
            db_ptr.write_volatile(value);
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
            trbs.add(self.ep0_enqueue_index).write_volatile(trb);

            self.ep0_enqueue_index += 1;
            if self.ep0_enqueue_index >= CONTROL_RING_SIZE - 1 {
                self.ep0_enqueue_index = 0;
                self.ep0_cycle_state = !self.ep0_cycle_state;
                // CATATAN: belum ada Link TRB di akhir ring, jadi wrap-around ini
                // cuma toggle cycle state software-side tanpa TRB Link fisik.
                // Aman untuk sekarang karena tiap control transfer cuma 3 TRB dan
                // ring size 16 (jarang wrap), tapi kalau kirim banyak transfer
                // beruntun ini WAJIB diperbaiki dengan Link TRB asli.
            }
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

            let trb = Trb {
                parameter,
                status,
                control,
            };
            trbs.add(self.cmd_enqueue_index).write_volatile(trb);

            self.cmd_enqueue_index += 1;
            if self.cmd_enqueue_index >= COMMAND_RING_SIZE - 1 {
                self.cmd_enqueue_index = 0;
                self.cmd_cycle_state = !self.cmd_cycle_state;
            }

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

            let control = (TRB_TYPE_NORMAL << 10) | (1 << 5) | (*cycle_state as u32);

            let trb = Trb {
                parameter: buf_phys,
                status: len & 0x1_FFFF,
                control,
            };
            trbs.add(*enqueue_index).write_volatile(trb);

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
const USBCMD_RUN_OR_STOP: u32 = (1 << 0);
// host controller reset
const USBCMD_HCRST: u32 = (1 << 1);
// interrupt enable
const USBCMD_INTE: u32 = (1 << 2);

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

pub unsafe fn init_xhci(xhci: &mut XHCI) {
    unsafe {
        let xhci_cap_regs = xhci.cap_regs();
        let xhci_op_regs = xhci.op_regs();

        // --- RESET (yang udah kita fix kemarin) ---
        let mut cmd = xhci_op_regs.read_usbcmd();
        cmd &= !USBCMD_RUN_OR_STOP;
        xhci_op_regs.write_usbcmd(cmd);
        while xhci_op_regs.read_usbsts() & USBSTS_HCH == 0 {}

        cmd = xhci_op_regs.read_usbcmd();
        cmd |= USBCMD_HCRST; // FIXED: set, bukan clear
        xhci_op_regs.write_usbcmd(cmd);
        while xhci_op_regs.read_usbcmd() & USBCMD_HCRST != 0 {}
        while xhci_op_regs.read_usbsts() & USBSTS_CNR != 0 {}

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

        let _segment_phys = setup_event_ring(xhci);

        start_controller(xhci);
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

pub unsafe fn setup_event_ring(xhci: &mut XHCI) -> u64 {
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

        ir0.write_erstsz(1);

        ir0.write_erdp(segment_phys);

        ir0.write_erstba(erst_phys);

        let mut iman = ir0.read_iman();
        iman |= 1 << 1;
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

pub unsafe fn start_controller(xhci: &XHCI) {
    unsafe {
        let op = xhci.op_regs();

        let mut cmd = op.read_usbcmd();
        cmd |= USBCMD_INTE;
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
            let connected = portsc & 0x1 != 0;

            if connected {
                let speed = (portsc >> 10) & 0xF;
                println!("Port {} CONNECTED, speed={}", port, speed);
            }
        }
    }
}

pub unsafe fn enable_slot() -> Result<u32, &'static str> {
    COMPLETION_PENDING.store(false, Ordering::SeqCst);

    unsafe {
        core::arch::asm!("cli");
        {
            let mut guard = XHCI_INSTANCE.get().unwrap().lock();
            guard.send_command(0, 0, TRB_TYPE_ENABLE_SLOT, 0);
        } // <-- guard di-drop DI SINI, lock dilepas sebelum sti
        core::arch::asm!("sti");
    }

    let mut timeout = 10_000_000;
    loop {
        if COMPLETION_PENDING.load(Ordering::SeqCst) {
            let code = COMPLETION_CODE.load(Ordering::SeqCst);
            let slot = COMPLETION_SLOT.load(Ordering::SeqCst);
            return if code == 1 {
                Ok(slot)
            } else {
                Err("Command completion code bukan Success")
            };
        }
        timeout -= 1;
        if timeout == 0 {
            return Err("Timeout");
        }
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
    COMPLETION_PENDING.store(false, Ordering::SeqCst);

    unsafe {
        core::arch::asm!("cli");
        {
            let mut guard = XHCI_INSTANCE.get().unwrap().lock();
            guard.send_command(0, 0, TRB_TYPE_DISABLE_SLOT, slot_id);
        }
        core::arch::asm!("sti");
    }

    let mut timeout = 10_000_000;
    loop {
        if COMPLETION_PENDING.load(Ordering::SeqCst) {
            let code = COMPLETION_CODE.load(Ordering::SeqCst);
            if code != 1 {
                return Err("Disable Slot gagal, completion code bukan Success");
            }
            break;
        }
        timeout -= 1;
        if timeout == 0 {
            return Err("Timeout menunggu Disable Slot completion");
        }
    }

    unsafe {
        let dcbaa_virt = XHCI_INSTANCE.get().unwrap().lock().dcbaa_virt;
        (dcbaa_virt as *mut u64)
            .add(slot_id as usize)
            .write_volatile(0);
    }

    println!("Disable Slot sukses untuk slot {}", slot_id);
    Ok(())
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

#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy)]
struct EndpointContext {
    dword0: u32,        // Mult(8-9) | MaxPStreams(10-14) | LSA(15) | Interval(16-23)
    dword1: u32,        // CErr(1-2) | EP Type(3-5) | Max Burst Size(8-15) | Max Packet Size(16-31)
    tr_dequeue_lo: u32, // + DCS di bit 0
    tr_dequeue_hi: u32,
    dword4: u32, // Average TRB Length(0-15) | Max ESIT Payload Low(16-31)
    reserved: [u32; 3],
}

#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy)]
struct InputControlContext {
    drop_flags: u32,
    add_flags: u32,
    reserved: [u32; 6],
}

#[repr(C, align(64))]
struct InputContext {
    control: InputControlContext,
    slot: SlotContext,
    ep0: EndpointContext,
}

impl InputContext {
    fn zeroed() -> Self {
        unsafe { core::mem::zeroed() }
    }
}

#[allow(dead_code)]
#[repr(C, align(64))]
struct DeviceContext {
    slot: SlotContext,
    endpoints: [EndpointContext; 31],
}

impl DeviceContext {
    fn zeroed() -> Self {
        unsafe { core::mem::zeroed() }
    }
}

const CONTROL_RING_SIZE: usize = 16;

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

pub unsafe fn address_device(slot_id: u32, root_port: u8, speed: u8) -> Result<(), &'static str> {
    unsafe {
        let ep0_ring_box: Box<TransferRing> = Box::new(TransferRing::new());
        let ep0_ring_virt = Box::into_raw(ep0_ring_box) as u64;
        let ep0_ring_phys = memory::virtual_to_physical(ep0_ring_virt as usize)
            .ok_or("gagal translate ep0 ring")? as u64;

        let dev_ctx_box: Box<DeviceContext> = Box::new(DeviceContext::zeroed());
        let dev_ctx_virt = Box::into_raw(dev_ctx_box) as u64;
        let dev_ctx_phys = memory::virtual_to_physical(dev_ctx_virt as usize)
            .ok_or("gagal translate device context")? as u64;

        let dcbaa_virt = {
            let guard = XHCI_INSTANCE.get().unwrap().lock();
            guard.dcbaa_virt
        };
        let dcbaa_ptr = dcbaa_virt as *mut u64;
        dcbaa_ptr.add(slot_id as usize).write_volatile(dev_ctx_phys);

        let mut input_ctx: Box<InputContext> = Box::new(InputContext::zeroed());
        input_ctx.control.add_flags = 0b11;

        input_ctx.slot.dword0 = (1u32 << 27) | ((speed as u32) << 20);
        input_ctx.slot.dword1 = (root_port as u32) << 16;

        let max_packet_size: u32 = match speed {
            4 => 512,
            3 => 64,
            2 => 8,
            _ => 8,
        };

        input_ctx.ep0.dword1 = (4 << 3) | (3 << 1) | (max_packet_size << 16);

        input_ctx.ep0.tr_dequeue_lo = (ep0_ring_phys as u32 & !0xF) | 1;
        input_ctx.ep0.tr_dequeue_hi = (ep0_ring_phys >> 32) as u32;
        input_ctx.ep0.dword4 = 8; // Average TRB Length = 8 (bit 0-15), Max ESIT Payload Low = 0 (bit 16-31)

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

        let mut timeout = 10_000_000;
        loop {
            if COMPLETION_PENDING.load(Ordering::SeqCst) {
                let code = COMPLETION_CODE.load(Ordering::SeqCst);
                return if code == 1 {
                    println!("Address Device sukses untuk slot {}", slot_id);
                    Ok(())
                } else {
                    Err("Address Device gagal, completion code bukan Success")
                };
            }
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
        let buf_box: Box<[u8; 18]> = Box::new([0u8; 18]);
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

            // --- Setup Stage TRB ---
            // IDT=1 (Immediate Data, parameter berisi data langsung bukan pointer)
            // TRT=3 (IN Data Stage, karena GET_DESCRIPTOR = device-to-host)
            let setup_status = 8u32; // TRB Transfer Length selalu 8 untuk setup packet
            let setup_control = (TRB_TYPE_SETUP_STAGE << 10) | (1 << 6) | (3 << 16);
            guard.ep0_enqueue_trb(setup_parameter, setup_status, setup_control);

            // --- Data Stage TRB ---
            // DIR=1 (IN, device mengirim data descriptor ke kita)
            let data_status = 18u32 & 0x1_FFFF; // TRB Transfer Length = wLength
            let data_control = (TRB_TYPE_DATA_STAGE << 10) | (1 << 16);
            guard.ep0_enqueue_trb(buf_phys, data_status, data_control);

            // --- Status Stage TRB ---
            // DIR=0 (OUT, kebalikan dari data stage) | IOC=1 (biar dapet completion event)
            let status_control = (TRB_TYPE_STATUS_STAGE << 10) | (1 << 5);
            guard.ep0_enqueue_trb(0, 0, status_control);

            // Ring doorbell buat slot ini, target=1 artinya EP0 (default control endpoint)
            guard.ring_doorbell(slot_id as u8, 1);
        } // <-- lock dilepas sebelum sti
        core::arch::asm!("sti");

        let mut timeout = 10_000_000;
        loop {
            if TRANSFER_COMPLETION_PENDING.load(Ordering::SeqCst) {
                let code = TRANSFER_COMPLETION_CODE.load(Ordering::SeqCst);
                // 1 = Success, 13 = Short Packet (device kirim kurang dari wLength,
                // tetap valid selama field-field awal descriptor sudah kebaca)
                if code != 1 && code != 13 {
                    return Err("Get Device Descriptor gagal, completion code bukan Success");
                }
                break;
            }
            timeout -= 1;
            if timeout == 0 {
                return Err("Timeout menunggu Get Device Descriptor completion");
            }
        }

        let buf_ptr = buf_virt as *const DeviceDescriptor;
        let descriptor = buf_ptr.read_unaligned(); // #[repr(packed)] -> harus read_unaligned

        let device_class = descriptor.device_class;
        let vendor_id = descriptor.vendor_id;
        let product_id = descriptor.product_id;
        let max_packet_size0 = descriptor.max_packet_size0;
        let num_configurations = descriptor.num_configurations;

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

            let setup_control = (TRB_TYPE_SETUP_STAGE << 10) | (1 << 6) | (3 << 16);
            guard.ep0_enqueue_trb(setup_parameter, 8, setup_control);

            let data_status = (w_length as u32) & 0x1_FFFF;
            let data_control = (TRB_TYPE_DATA_STAGE << 10) | (1 << 16);
            guard.ep0_enqueue_trb(buf_phys, data_status, data_control);

            let status_control = (TRB_TYPE_STATUS_STAGE << 10) | (1 << 5);
            guard.ep0_enqueue_trb(0, 0, status_control);

            guard.ring_doorbell(slot_id as u8, 1);
        }

        core::arch::asm!("sti");

        let mut timeout = 10_000_000;
        loop {
            if TRANSFER_COMPLETION_PENDING.load(Ordering::SeqCst) {
                let code = TRANSFER_COMPLETION_CODE.load(Ordering::SeqCst);
                let residual = TRANSFER_COMPLETION_LENGTH.load(Ordering::SeqCst);
                if code != 1 && code != 13 {
                    return Err("Control transfer gagal, completion code bukan Success");
                }
                return Ok((w_length as u32).saturating_sub(residual));
            }
            timeout -= 1;
            if timeout == 0 {
                return Err("timeout menunggu control transfer completion");
            }
        }
    }
}

const USB_DESC_TYPE_CONFIGURATION: u8 = 0x02;
const USB_DESC_TYPE_INTERFACE: u8 = 0x04;
const USB_DESC_TYPE_ENDPOINT: u8 = 0x05;

/// Info satu endpoint Interrupt IN beserta interface pemiliknya.
/// Generic untuk endpoint HID apa pun (keyboard, mouse, atau device HID
/// lain) -- xhci.rs tidak perlu tahu ini "keyboard" atau "mouse", itu
/// keputusan device-class driver di atasnya lewat `interface_class`/
/// `interface_protocol`.
#[derive(Debug, Clone, Copy)]
pub struct EndpointInfo {
    pub address: u8,
    pub attributes: u8,
    pub max_packet_size: u16,
    pub interval: u8,
    pub inteface_number: u8,
    /// bInterfaceClass (mis. 3 = HID)
    pub interface_class: u8,
    /// bInterfaceProtocol (mis. 1 = Boot Keyboard, 2 = Boot Mouse; 0 kalau
    /// device bukan Boot Protocol / field tidak relevan)
    pub interface_protocol: u8,
}

/// Ambil Configuration Descriptor lalu kembalikan SEMUA endpoint Interrupt IN
/// yang ditemukan di semua interface -- bukan cuma yang pertama ketemu.
/// Caller (device-class driver di main.rs/keyboard_usb.rs/mouse_usb.rs) yang
/// memilih endpoint mana yang relevan, biasanya lewat `interface_protocol`.
pub unsafe fn get_configuration_descriptor_and_find_interrupt_endpoints(
    slot_id: u32,
) -> Result<alloc::vec::Vec<EndpointInfo>, &'static str> {
    unsafe {
        const BUF_SIZE: usize = 256;
        let buf_box: Box<[u8; BUF_SIZE]> = Box::new([0u8; BUF_SIZE]);
        let buf_virt = Box::into_raw(buf_box) as u64;
        let buf_phys = memory::virtual_to_physical(buf_virt as usize)
            .ok_or("gagal translate buffer config descriptor")? as u64;

        let w_value: u16 = (USB_DESC_TYPE_CONFIGURATION as u16) << 8;

        let actual_len = control_transfer_in(
            slot_id,
            0x80,
            USB_REQ_GET_DESCRIPTOR,
            w_value,
            0,
            buf_phys,
            BUF_SIZE as u16,
        )?;

        println!("Configuration Descriptor: {} byte diterima", actual_len);

        let buf_slice = core::slice::from_raw_parts(buf_virt as *const u8, actual_len as usize);

        let mut i = 0usize;
        let mut current_interface: u8 = 0;
        let mut current_interface_class: u8 = 0;
        let mut current_interface_protocol: u8 = 0;
        let mut found: alloc::vec::Vec<EndpointInfo> = alloc::vec::Vec::new();

        while i + 2 <= buf_slice.len() {
            let b_length = buf_slice[i] as usize;
            let b_descriptor_type = buf_slice[i + 1];

            if b_length == 0 || i + b_length > buf_slice.len() {
                break;
            }

            if b_descriptor_type == USB_DESC_TYPE_INTERFACE {
                current_interface = buf_slice[i + 2];
                current_interface_class = buf_slice[i + 5];
                current_interface_protocol = buf_slice[i + 7];
                println!(
                    "  Interface {} class={:#04x} protocol={:#04x}",
                    current_interface, current_interface_class, current_interface_protocol
                );
            } else if b_descriptor_type == USB_DESC_TYPE_ENDPOINT {
                let address = buf_slice[i + 2];
                let attributes = buf_slice[i + 3];
                let max_packet_size = (buf_slice[i + 4] as u16) | ((buf_slice[i + 5] as u16) << 8);
                let interval = buf_slice[i + 6];

                println!(
                    "   Endpoint addr={:#04x} attr={:#04x} max_packet_size={} interval={}",
                    address, attributes, max_packet_size, interval
                );

                let is_in = address & 0x80 != 0;
                let is_interrupt = attributes & 0x3 == 0x3;
                if is_in && is_interrupt {
                    found.push(EndpointInfo {
                        address,
                        attributes,
                        max_packet_size,
                        interval,
                        inteface_number: current_interface,
                        interface_class: current_interface_class,
                        interface_protocol: current_interface_protocol,
                    });
                }
            }
            i += b_length;
        }

        if found.is_empty() {
            Err("Tidak ketemu Interrupt IN endpoint di Configuration Descriptor")
        } else {
            Ok(found)
        }
    }
}

#[repr(C, align(64))]
#[allow(dead_code)]
struct InputContextFull {
    control: InputControlContext,
    slot: SlotContext,
    endpoints: [EndpointContext; 31],
}

impl InputContextFull {
    fn zeroed() -> Self {
        unsafe { core::mem::zeroed() }
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

            let setup_control = (TRB_TYPE_SETUP_STAGE << 10) | (1 << 6) | (0 << 16);
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

        let mut timeout = 10_000_000;

        loop {
            if TRANSFER_COMPLETION_PENDING.load(Ordering::SeqCst) {
                let code = TRANSFER_COMPLETION_CODE.load(Ordering::SeqCst);
                return if code == 1 {
                    Ok(())
                } else {
                    Err("Control Transfer (no data gagal, completion code bukan Success")
                };
            }
            timeout -= 1;
            if timeout == 0 {
                return Err("Timeout control transfer no data");
            }
        }
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
}

pub unsafe fn configure_endpoint(
    slot_id: u32,
    root_port: u8,
    ep_info: EndpointInfo,
) -> Result<EndpointHandle, &'static str> {
    unsafe {
        let ep_number = (ep_info.address & 0x0F) as u32;
        let is_in = ep_info.address & 0x80 != 0;
        let dci = (ep_number * 2) + if is_in { 1 } else { 0 };

        let ep_ring_box: Box<TransferRing> = Box::new(TransferRing::new());
        let ep_ring_virt = Box::into_raw(ep_ring_box) as u64;
        let ep_ring_phys = memory::virtual_to_physical(ep_ring_virt as usize)
            .ok_or("gagal translate interrupt ep ring")? as u64;

        write_link_trb(ep_ring_virt, ep_ring_phys, CONTROL_RING_SIZE, true);

        let mut input_ctx: Box<InputContextFull> = Box::new(InputContextFull::zeroed());

        input_ctx.control.add_flags = (1 << 0) | (1 << dci);

        input_ctx.slot.dword0 = (dci << 27) | (3u32 << 20);
        input_ctx.slot.dword1 = (root_port as u32) << 16;

        let idx = (dci - 1) as usize;
        let ep_ctx = &mut input_ctx.endpoints[idx];
        ep_ctx.dword0 = (ep_info.interval.saturating_sub(1) as u32) << 16;
        ep_ctx.dword1 = (7u32 << 3) | (3 << 1) | ((ep_info.max_packet_size as u32) << 16);
        ep_ctx.tr_dequeue_lo = (ep_ring_phys as u32 & !0xF) | 1;
        ep_ctx.tr_dequeue_hi = (ep_ring_phys >> 32) as u32;
        ep_ctx.dword4 = ep_info.max_packet_size as u32;

        let input_ctx_virt = Box::into_raw(input_ctx) as u64;
        let input_ctx_phys = memory::virtual_to_physical(input_ctx_virt as usize)
            .ok_or("gagal translate input context configure endpoint")?
            as u64;

        COMPLETION_PENDING.store(false, Ordering::SeqCst);

        core::arch::asm!("cli");
        {
            let mut guard = XHCI_INSTANCE.get().unwrap().lock();
            guard.send_command(input_ctx_phys, 0, TRB_TYPE_CONFIGURE_ENDPOINT, slot_id);
        }
        core::arch::asm!("sti");
        let mut timeout = 10_000_000;

        loop {
            if COMPLETION_PENDING.load(Ordering::SeqCst) {
                let code = COMPLETION_CODE.load(Ordering::SeqCst);
                return if code == 1 {
                    println!("Configure Endpoint sukses (DCI={}", dci);
                    Ok(EndpointHandle {
                        ring_virt: ep_ring_virt,
                        ring_phys: ep_ring_phys,
                        dci,
                    })
                } else {
                    Err("Configure Endpoint gagal, completion code bukan Succes")
                };
            }
            timeout -= 1;
            if timeout == 0 {
                return Err("Timeout menunggu Configure Endpoint completion");
            }
        }
    }
}

const TRB_TYPE_LINK: u32 = 6;

unsafe fn write_link_trb(ring_virt: u64, ring_phys: u64, ring_size: usize, cycle: bool) {
    unsafe {
        let trbs = ring_virt as *mut Trb;
        let control = (TRB_TYPE_LINK << 10) | (1 << 1) | (cycle as u32);
        trbs.add(ring_size - 1).write_volatile(Trb {
            parameter: ring_phys,
            status: 0,
            control,
        });
    }
}
