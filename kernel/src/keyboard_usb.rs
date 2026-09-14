use crate::memory;
use crate::xhci::{self, KeyboardEndpoint, XHCI_INSTANCE};
use alloc::boxed::Box;
use spin::Mutex;

const REPORT_LEN: usize = 8;
const KBD_RING_SIZE: usize = 16; // sama seperti CONTROL_RING_SIZE di xhci.rs

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct KeyboardReport {
    modifier: u8,
    reserved: u8,
    keycodes: [u8; 6],
}

/// Seluruh state driver keyboard digabung di sini dan dilindungi Mutex,
/// supaya tidak butuh `static mut` sama sekali -- aman diakses dari
/// main loop maupun dari interrupt handler.
struct KeyboardState {
    slot_id: u32,
    ring_virt: u64,
    ring_phys: u64,
    enqueue_index: usize,
    cycle_state: bool,
    dci: u32,
    report_buf_virt: u64,
    report_buf_phys: u64,
    last_keycodes: [u8; 6],
}

static KBD_STATE: Mutex<Option<KeyboardState>> = Mutex::new(None);

const HID_SET_PROTOCOL: u8 = 0x0B;
const HID_BOOT_PROTOCOL: u16 = 0;

/// Panggil SEKALI, setelah xhci::configure_endpoint() sukses untuk
/// endpoint interrupt IN milik keyboard.
pub unsafe fn init_keyboard(slot_id: u32, ep: KeyboardEndpoint, interface_number: u8) {
    unsafe {
        // Set Boot Protocol -- bmRequestType=0x21 (Host->Device, Class, Interface)
        let _ = xhci::control_transfer_no_data(
            slot_id,
            0x21,
            HID_SET_PROTOCOL,
            HID_BOOT_PROTOCOL,
            interface_number as u16,
        );

        // buffer report -- dialokasikan sekali, dipakai berulang selama driver hidup
        let buf: Box<[u8; REPORT_LEN]> = Box::new([0u8; REPORT_LEN]);
        let virt = Box::into_raw(buf) as u64;
        let phys = memory::virtual_to_physical(virt as usize)
            .expect("gagal translate report buffer keyboard");

        xhci::KBD_DCI.store(ep.dci, core::sync::atomic::Ordering::SeqCst);

        {
            let mut state = KBD_STATE.lock();
            *state = Some(KeyboardState {
                slot_id,
                ring_virt: ep.ring_virt,
                ring_phys: ep.ring_phys,
                enqueue_index: 0,
                cycle_state: true,
                dci: ep.dci,
                report_buf_virt: virt,
                report_buf_phys: phys as u64,
                last_keycodes: [0; 6],
            });
        }

        arm_next_report();
        crate::println!("Keyboard driver siap, DCI={}", ep.dci);
    }
}

unsafe fn arm_next_report() {
    unsafe {
        let mut state_guard = KBD_STATE.lock();
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
            KBD_RING_SIZE,
            state.slot_id,
            state.dci,
            state.report_buf_phys,
            REPORT_LEN as u32,
        );
    }
}

/// Panggil dari interrupt handler (vector 44) setiap kali ada event masuk.
pub unsafe fn poll_keyboard() {
    unsafe {
        if !xhci::KBD_REPORT_PENDING.swap(false, core::sync::atomic::Ordering::SeqCst) {
            return;
        }

        let report_buf_virt = {
            let state_guard = KBD_STATE.lock();
            match state_guard.as_ref() {
                Some(s) => s.report_buf_virt,
                None => return,
            }
        };

        let report = (report_buf_virt as *const KeyboardReport).read_volatile();
        handle_report(&report);
        arm_next_report(); // WAJIB -- report berikutnya cuma masuk kalau di-re-arm
    }
}

fn handle_report(report: &KeyboardReport) {
    let mut state_guard = KBD_STATE.lock();
    let state = match state_guard.as_mut() {
        Some(s) => s,
        None => return,
    };

    for &kc in &report.keycodes {
        if kc == 0 {
            continue;
        }
        if state.last_keycodes.contains(&kc) {
            continue; // key masih ditahan dari report sebelumnya, jangan re-trigger
        }
        if let Some(ch) = hid_keycode_to_ascii(kc, report.modifier) {
            crate::print!("{}", ch);
        }
    }
    state.last_keycodes = report.keycodes;
}

fn hid_keycode_to_ascii(keycode: u8, modifier: u8) -> Option<char> {
    let shift = modifier & 0x22 != 0; // bit1=Left Shift, bit5=Right Shift
    match keycode {
        0x04..=0x1D => {
            let base = b'a' + (keycode - 0x04);
            let c = if shift {
                base.to_ascii_uppercase()
            } else {
                base
            };
            Some(c as char)
        }
        0x1E..=0x27 => {
            let normal = b"1234567890";
            let shifted = b"!@#$%^&*()";
            let idx = (keycode - 0x1E) as usize;
            Some((if shift { shifted[idx] } else { normal[idx] }) as char)
        }
        0x2C => Some(' '),
        0x28 => Some('\n'),
        0x2A => Some('\u{8}'), // backspace
        0x2B => Some('\t'),
        _ => None,
    }
}
