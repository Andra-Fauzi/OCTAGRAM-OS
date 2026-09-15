//! Device-class driver untuk HID Boot Mouse di atas transport xHCI generic
//! (lihat `xhci.rs`). Strukturnya sengaja mirip `keyboard_usb.rs` -- bedanya
//! cuma bentuk report dan cara menginterpretasikannya (delta posisi + bitmask
//! tombol, bukan keycode).

use crate::memory;
use crate::usb::xhci::{self, EndpointHandle, XHCI_INSTANCE};
use alloc::boxed::Box;
use core::sync::atomic::{AtomicBool, AtomicU32};
use spin::Mutex;

// Buffer report kita lebihkan jadi 8 byte biar aman menampung boot mouse
// yang punya byte wheel/tombol ekstra (device standar cuma isi 3-4 byte
// pertama: buttons, x, y, [wheel]).
const REPORT_BUF_LEN: usize = 8;
const MOUSE_RING_SIZE: usize = 16; // sama seperti CONTROL_RING_SIZE di xhci.rs

// Statics ini di-import oleh xhci.rs (lihat `poll_event_ring`) supaya interrupt
// handler tahu Transfer Event yang masuk itu report mouse atau bukan.
pub static MOUSE_DCI: AtomicU32 = AtomicU32::new(0);
// Sama kayak KBD_SLOT_ID -- DCI mouse bisa (dan sering) sama dengan DCI
// keyboard, jadi slot_id wajib ikut dicocokkan di poll_event_ring.
pub static MOUSE_SLOT_ID: AtomicU32 = AtomicU32::new(0);
pub static MOUSE_REPORT_PENDING: AtomicBool = AtomicBool::new(false);
pub static MOUSE_REPORT_LEN: AtomicU32 = AtomicU32::new(0);

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct MouseReport {
    buttons: u8, // bit0=left, bit1=right, bit2=middle, sisanya vendor-specific
    x: i8,       // delta X sejak report terakhir
    y: i8,       // delta Y sejak report terakhir
    wheel: i8,   // delta scroll wheel (0 kalau device tidak punya wheel report)
}

/// Seluruh state driver mouse digabung di sini dan dilindungi Mutex, sama
/// seperti `KeyboardState` -- aman diakses dari main loop maupun interrupt.
struct MouseState {
    slot_id: u32,
    ring_virt: u64,
    ring_phys: u64,
    enqueue_index: usize,
    cycle_state: bool,
    dci: u32,
    report_buf_virt: u64,
    report_buf_phys: u64,
    last_buttons: u8,
    pos_x: i32,
    pos_y: i32,
}

static MOUSE_STATE: Mutex<Option<MouseState>> = Mutex::new(None);

const HID_SET_PROTOCOL: u8 = 0x0B;
const HID_BOOT_PROTOCOL: u16 = 0;

/// Panggil SEKALI, setelah `xhci::configure_endpoint()` sukses untuk
/// endpoint interrupt IN milik mouse (protocol == 2 dari `EndpointInfo`).
pub unsafe fn init_mouse(slot_id: u32, ep: EndpointHandle, interface_number: u8) {
    unsafe {
        // Set Boot Protocol -- bmRequestType=0x21 (Host->Device, Class, Interface)
        let _ = xhci::control_transfer_no_data(
            slot_id,
            0x21,
            HID_SET_PROTOCOL,
            HID_BOOT_PROTOCOL,
            interface_number as u16,
        );

        let buf: Box<[u8; REPORT_BUF_LEN]> = Box::new([0u8; REPORT_BUF_LEN]);
        let virt = Box::into_raw(buf) as u64;
        let phys = memory::virtual_to_physical(virt as usize)
            .expect("gagal translate report buffer mouse");

        MOUSE_DCI.store(ep.dci, core::sync::atomic::Ordering::SeqCst);
        MOUSE_SLOT_ID.store(slot_id, core::sync::atomic::Ordering::SeqCst);

        {
            let mut state = MOUSE_STATE.lock();
            *state = Some(MouseState {
                slot_id,
                ring_virt: ep.ring_virt,
                ring_phys: ep.ring_phys,
                enqueue_index: 0,
                cycle_state: true,
                dci: ep.dci,
                report_buf_virt: virt,
                report_buf_phys: phys as u64,
                last_buttons: 0,
                pos_x: 0,
                pos_y: 0,
            });
        }

        arm_next_report();
        crate::println!("Mouse driver siap, DCI={}", ep.dci);
    }
}

unsafe fn arm_next_report() {
    unsafe {
        let mut state_guard = MOUSE_STATE.lock();
        let state = match state_guard.as_mut() {
            Some(s) => s,
            None => return,
        };

        let xhci_guard = XHCI_INSTANCE.get().unwrap().lock();
        xhci_guard.enqueue_normal_trb_and_ring(
            state.ring_virt,
            state.ring_phys,
            &mut state.enqueue_index,
            &mut state.cycle_state,
            MOUSE_RING_SIZE,
            state.slot_id,
            state.dci,
            state.report_buf_phys,
            REPORT_BUF_LEN as u32,
        );
    }
}

/// Panggil waktu mouse dicabut (hotplug disconnect). No-op kalau slot
/// yang dicabut bukan slot milik mouse yang lagi aktif.
pub unsafe fn shutdown_mouse(slot_id: u32) {
    unsafe {
        let mut state_guard = MOUSE_STATE.lock();

        let owns_slot = matches!(state_guard.as_ref(), Some(s) if s.slot_id == slot_id);
        if !owns_slot {
            return;
        }

        if let Some(state) = state_guard.take() {
            let _ = Box::from_raw(state.report_buf_virt as *mut [u8; REPORT_BUF_LEN]);
        }

        MOUSE_DCI.store(0, core::sync::atomic::Ordering::SeqCst);
        MOUSE_SLOT_ID.store(0, core::sync::atomic::Ordering::SeqCst);
        MOUSE_REPORT_PENDING.store(false, core::sync::atomic::Ordering::SeqCst);

        crate::println!("Mouse driver shutdown (slot={})", slot_id);
    }
}

/// Panggil dari interrupt handler (vector 44) setiap kali ada event masuk.
pub unsafe fn poll_mouse() {
    unsafe {
        if !MOUSE_REPORT_PENDING.swap(false, core::sync::atomic::Ordering::SeqCst) {
            return;
        }

        let report_buf_virt = {
            let state_guard = MOUSE_STATE.lock();
            match state_guard.as_ref() {
                Some(s) => s.report_buf_virt,
                None => return,
            }
        };

        let report = (report_buf_virt as *const MouseReport).read_volatile();
        handle_report(&report);
        arm_next_report(); // WAJIB -- report berikutnya cuma masuk kalau di-re-arm
    }
}

fn handle_report(report: &MouseReport) {
    let mut state_guard = MOUSE_STATE.lock();
    let state = match state_guard.as_mut() {
        Some(s) => s,
        None => return,
    };

    state.pos_x += report.x as i32;
    state.pos_y += report.y as i32;

    // Deteksi klik/lepas per bit dibanding report sebelumnya, biar tidak
    // spam log tiap report walau tombol masih ditahan.
    let changed = report.buttons ^ state.last_buttons;
    if changed != 0 {
        for (bit, name) in [(0u8, "left"), (1u8, "right"), (2u8, "middle")] {
            if changed & (1 << bit) != 0 {
                let pressed = report.buttons & (1 << bit) != 0;
                crate::println!(
                    "Mouse {} {} @ ({}, {})",
                    name,
                    if pressed { "pressed" } else { "released" },
                    state.pos_x,
                    state.pos_y
                );
            }
        }
    }

    if report.x != 0 || report.y != 0 {
        crate::println!(
            "Mouse move dx={} dy={} pos=({}, {})",
            report.x,
            report.y,
            state.pos_x,
            state.pos_y
        );
    }

    state.last_buttons = report.buttons;
}
