//! Device-class driver untuk HID Boot Keyboard di atas transport xHCI generic
//! (lihat `xhci.rs`). Modul ini yang "tahu" konsep keyboard -- xhci.rs sendiri
//! cuma menyediakan primitif transport (ring, control transfer, dst).

use super::xhci::{self, EndpointHandle, XHCI_INSTANCE};
use crate::{drivers::keyboard, memory};
use alloc::boxed::Box;
use core::sync::atomic::{AtomicBool, AtomicU32};
use spin::Mutex;

const REPORT_LEN: usize = 8;
const KBD_RING_SIZE: usize = 16; // sama seperti CONTROL_RING_SIZE di xhci.rs

// Statics ini di-import oleh xhci.rs (lihat `poll_event_ring`) supaya interrupt
// handler tahu Transfer Event yang masuk itu report keyboard atau bukan.
pub static KBD_DCI: AtomicU32 = AtomicU32::new(0);
// DCI (Device Context Index) itu index LOKAL per-slot, bukan ID unik
// global -- device HID lain (mis. mouse) bisa punya DCI yang sama kalau
// endpoint address-nya kebetulan sama (umum terjadi, keyboard & mouse
// boot protocol sering sama-sama pakai endpoint 0x81). Makanya slot_id
// WAJIB ikut dicocokkan di `xhci.rs::poll_event_ring`, tidak cukup DCI
// doang -- lihat komentar di sana.
pub static KBD_SLOT_ID: AtomicU32 = AtomicU32::new(0);
pub static KBD_REPORT_PENDING: AtomicBool = AtomicBool::new(false);
pub static KBD_REPORT_LEN: AtomicU32 = AtomicU32::new(0);

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
    caps_lock: bool,
}

static KBD_STATE: Mutex<Option<KeyboardState>> = Mutex::new(None);
static PERIODIC_INPUT_STATE: Mutex<([u8; 6], bool)> = Mutex::new(([0; 6], false));

const HID_SET_PROTOCOL: u8 = 0x0B;
const HID_BOOT_PROTOCOL: u16 = 0;

/// Panggil SEKALI, setelah `xhci::configure_endpoint()` sukses untuk
/// endpoint interrupt IN milik keyboard (protocol == 1 dari `EndpointInfo`).
pub unsafe fn init_keyboard(slot_id: u32, ep: EndpointHandle, interface_number: u8) {
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

        KBD_DCI.store(ep.dci, core::sync::atomic::Ordering::SeqCst);
        KBD_SLOT_ID.store(slot_id, core::sync::atomic::Ordering::SeqCst);

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
                caps_lock: false,
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

        // Jangan `unwrap()`: fungsi ini juga dipanggil dari jalur IRQ (lewat
        // `poll_keyboard`), dan panic di sana menjatuhkan kernel.
        let Some(xhci) = XHCI_INSTANCE.get() else {
            return;
        };
        let xhci_guard = xhci.lock();
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

/// Panggil waktu keyboard dicabut (hotplug disconnect). No-op kalau slot
/// yang dicabut bukan slot milik keyboard yang lagi aktif -- aman
/// dipanggil dari `usb.rs` tanpa perlu cek dulu driver mana yang punya
/// slot itu.
pub unsafe fn shutdown_keyboard(slot_id: u32) {
    unsafe {
        let mut state_guard = KBD_STATE.lock();

        let owns_slot = matches!(state_guard.as_ref(), Some(s) if s.slot_id == slot_id);
        if !owns_slot {
            return;
        }

        // Endpoint-nya sudah/segera di-disable di controller lewat
        // `xhci::disable_slot`, jadi tidak akan ada DMA baru yang nulis
        // ke buffer report ini -- aman untuk di-drop balik ke allocator
        // (bukan cuma dibiarkan leak).
        if let Some(state) = state_guard.take() {
            let _ = Box::from_raw(state.report_buf_virt as *mut [u8; REPORT_LEN]);
        }

        KBD_DCI.store(0, core::sync::atomic::Ordering::SeqCst);
        KBD_SLOT_ID.store(0, core::sync::atomic::Ordering::SeqCst);
        KBD_REPORT_PENDING.store(false, core::sync::atomic::Ordering::SeqCst);

        crate::println!("Keyboard driver shutdown (slot={})", slot_id);
    }
}

/// Panggil setiap kali ada event masuk -- dari `poll_interrupt_deferred()`
/// (idle loop), BUKAN langsung dari interrupt handler: lihat catatan
/// `xhci::INTERRUPT_PENDING` soal spin-lock di konteks IRQ.
pub unsafe fn poll_keyboard() {
    unsafe {
        if !KBD_REPORT_PENDING.swap(false, core::sync::atomic::Ordering::SeqCst) {
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

    handle_report_with_state(report, &mut state.last_keycodes, &mut state.caps_lock);
}

pub(super) fn process_periodic_report(bytes: &[u8]) {
    if bytes.len() < REPORT_LEN {
        return;
    }
    let report = KeyboardReport {
        modifier: bytes[0],
        reserved: bytes[1],
        keycodes: [bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]],
    };
    let mut state = PERIODIC_INPUT_STATE.lock();
    let (last_keycodes, caps_lock) = &mut *state;
    handle_report_with_state(&report, last_keycodes, caps_lock);
}

pub(super) fn reset_periodic_state() {
    *PERIODIC_INPUT_STATE.lock() = ([0; 6], false);
}

fn handle_report_with_state(
    report: &KeyboardReport,
    last_keycodes: &mut [u8; 6],
    caps_lock: &mut bool,
) {
    for &keycode in &report.keycodes {
        if keycode == 0 || last_keycodes.contains(&keycode) {
            continue;
        }
        if (0x3A..=0x3D).contains(&keycode)
            && report.modifier & 0x11 != 0
            && report.modifier & 0x44 != 0
        {
            keyboard::switch_terminal((keycode - 0x3A) as usize);
            continue;
        }
        if keycode == 0x39 {
            *caps_lock = !*caps_lock;
            continue;
        }
        match keycode {
            0x4F => {
                let _ = keyboard::push_event(keyboard::KeyEvent::Right);
            }
            0x50 => {
                let _ = keyboard::push_event(keyboard::KeyEvent::Left);
            }
            0x51 => {
                let _ = keyboard::push_event(keyboard::KeyEvent::Down);
            }
            0x52 => {
                let _ = keyboard::push_event(keyboard::KeyEvent::Up);
            }
            _ => {
                if let Some(character) = hid_keycode_to_ascii(keycode, report.modifier, *caps_lock)
                {
                    let _ = keyboard::push_char(character);
                }
            }
        }
    }
    *last_keycodes = report.keycodes;
}

fn hid_keycode_to_ascii(keycode: u8, modifier: u8, caps_lock: bool) -> Option<char> {
    let shift = modifier & 0x22 != 0; // bit1=Left Shift, bit5=Right Shift
    if modifier & 0x11 != 0 {
        return match keycode {
            0x14 => Some('\x11'),
            0x16 => Some('\x13'),
            0x07 => Some('\x04'),
            _ => None,
        };
    }
    match keycode {
        0x04..=0x1D => {
            let base = b'a' + (keycode - 0x04);
            let c = if shift ^ caps_lock {
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
        0x2D => Some(if shift { '_' } else { '-' }),
        0x2E => Some(if shift { '+' } else { '=' }),
        0x2F => Some(if shift { '{' } else { '[' }),
        0x30 => Some(if shift { '}' } else { ']' }),
        0x31 => Some(if shift { '|' } else { '\\' }),
        0x32 => Some(if shift { '~' } else { '#' }),
        0x33 => Some(if shift { ':' } else { ';' }),
        0x34 => Some(if shift { '"' } else { '\'' }),
        0x35 => Some(if shift { '~' } else { '`' }),
        0x36 => Some(if shift { '<' } else { ',' }),
        0x37 => Some(if shift { '>' } else { '.' }),
        0x38 => Some(if shift { '?' } else { '/' }),
        0x2C => Some(' '),
        0x28 => Some('\n'),
        0x2A => Some('\u{8}'), // backspace
        0x2B => Some('\t'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::hid_keycode_to_ascii;

    #[test]
    fn maps_usb_slash_and_shifted_question_mark() {
        assert_eq!(hid_keycode_to_ascii(0x38, 0, false), Some('/'));
        assert_eq!(hid_keycode_to_ascii(0x38, 0x02, false), Some('?'));
        assert_eq!(hid_keycode_to_ascii(0x14, 0x01, false), Some('\x11'));
        assert_eq!(hid_keycode_to_ascii(0x16, 0x01, false), Some('\x13'));
        assert_eq!(hid_keycode_to_ascii(0x07, 0x01, false), Some('\x04'));
    }

    #[test]
    fn maps_usb_common_punctuation() {
        assert_eq!(hid_keycode_to_ascii(0x36, 0, false), Some(','));
        assert_eq!(hid_keycode_to_ascii(0x37, 0x02, false), Some('>'));
        assert_eq!(hid_keycode_to_ascii(0x31, 0, false), Some('\\'));
    }
}
