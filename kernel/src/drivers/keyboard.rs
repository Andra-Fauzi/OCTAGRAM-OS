//! Shared keyboard input queue and legacy PS/2 scancode handling.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicUsize, Ordering};
use spin::Once;

use crate::task::TaskHandle;

const INPUT_CAPACITY: usize = 256;
const INPUT_MASK: usize = INPUT_CAPACITY - 1;
const KEY_EVENT_BASE: u32 = 0x11_0000;
const PS2_DATA_PORT: u16 = 0x60;
const LEFT_SHIFT: u8 = 1 << 0;
const RIGHT_SHIFT: u8 = 1 << 1;
const CAPS_LOCK: u8 = 1 << 2;
const LEFT_CTRL: u8 = 1 << 3;
const RIGHT_CTRL: u8 = 1 << 4;

static INPUT: [AtomicU32; INPUT_CAPACITY] = [const { AtomicU32::new(0) }; INPUT_CAPACITY];
static INPUT_HEAD: AtomicUsize = AtomicUsize::new(0);
static INPUT_TAIL: AtomicUsize = AtomicUsize::new(0);
static PS2_MODIFIERS: AtomicU8 = AtomicU8::new(0);
static PS2_EXTENDED: AtomicBool = AtomicBool::new(false);
static PS2_SKIP_REMAINING: AtomicU8 = AtomicU8::new(0);
static INPUT_WAKER: Once<TaskHandle> = Once::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyEvent {
    Character(char),
    Left,
    Right,
    Up,
    Down,
}

pub fn set_input_waker(waker: TaskHandle) {
    INPUT_WAKER.call_once(|| waker);
}

/// Push one character from an interrupt handler. Returns false if the queue is full.
pub fn push_char(character: char) -> bool {
    push_event(KeyEvent::Character(character))
}

/// Push a character or navigation key from an interrupt handler.
pub fn push_event(event: KeyEvent) -> bool {
    let head = INPUT_HEAD.load(Ordering::Relaxed);
    let next = head.wrapping_add(1) & INPUT_MASK;
    if next == INPUT_TAIL.load(Ordering::Acquire) {
        return false;
    }

    let value = match event {
        KeyEvent::Character(character) => character as u32,
        KeyEvent::Left => KEY_EVENT_BASE,
        KeyEvent::Right => KEY_EVENT_BASE + 1,
        KeyEvent::Up => KEY_EVENT_BASE + 2,
        KeyEvent::Down => KEY_EVENT_BASE + 3,
    };
    INPUT[head].store(value, Ordering::Relaxed);
    INPUT_HEAD.store(next, Ordering::Release);
    if let Some(waker) = INPUT_WAKER.get() {
        waker.wake();
    }
    true
}

/// Remove the next event entered by either keyboard driver.
pub fn pop_event() -> Option<KeyEvent> {
    let tail = INPUT_TAIL.load(Ordering::Relaxed);
    if tail == INPUT_HEAD.load(Ordering::Acquire) {
        return None;
    }

    let value = INPUT[tail].load(Ordering::Relaxed);
    INPUT_TAIL.store(tail.wrapping_add(1) & INPUT_MASK, Ordering::Release);
    Some(match value {
        KEY_EVENT_BASE => KeyEvent::Left,
        value if value == KEY_EVENT_BASE + 1 => KeyEvent::Right,
        value if value == KEY_EVENT_BASE + 2 => KeyEvent::Up,
        value if value == KEY_EVENT_BASE + 3 => KeyEvent::Down,
        value => KeyEvent::Character(char::from_u32(value)?),
    })
}

/// Remove the next character, discarding navigation keys.
pub fn pop_char() -> Option<char> {
    match pop_event()? {
        KeyEvent::Character(character) => Some(character),
        _ => None,
    }
}

pub async fn next_event() -> KeyEvent {
    core::future::poll_fn(|_| match pop_event() {
        Some(event) => core::task::Poll::Ready(event),
        None => core::task::Poll::Pending,
    })
    .await
}

pub async fn next_char() -> char {
    loop {
        if let KeyEvent::Character(character) = next_event().await {
            return character;
        }
    }
}

/// Read and process the scan code associated with legacy IRQ1.
pub fn handle_ps2_irq() {
    let scan_code = unsafe { crate::arch::x86_64::io::inb(PS2_DATA_PORT) };
    if PS2_SKIP_REMAINING.load(Ordering::Relaxed) != 0 {
        PS2_SKIP_REMAINING.fetch_sub(1, Ordering::Relaxed);
        return;
    }
    if scan_code == 0xE1 {
        PS2_SKIP_REMAINING.store(5, Ordering::Relaxed);
        return;
    }
    if scan_code == 0xE0 {
        PS2_EXTENDED.store(true, Ordering::Relaxed);
        return;
    }
    let released = scan_code & 0x80 != 0;
    let key = scan_code & 0x7F;
    if PS2_EXTENDED.swap(false, Ordering::Relaxed) {
        if key == 0x1D {
            update_modifier(RIGHT_CTRL, released);
            return;
        }
        if released {
            return;
        }
        let event = match key {
            0x4B => Some(KeyEvent::Left),
            0x4D => Some(KeyEvent::Right),
            0x48 => Some(KeyEvent::Up),
            0x50 => Some(KeyEvent::Down),
            _ => None,
        };
        if let Some(event) = event {
            let _ = push_event(event);
        }
        return;
    }

    match key {
        0x2A => update_modifier(LEFT_SHIFT, released),
        0x36 => update_modifier(RIGHT_SHIFT, released),
        0x1D => update_modifier(LEFT_CTRL, released),
        0x3A if !released => {
            PS2_MODIFIERS.fetch_xor(CAPS_LOCK, Ordering::Relaxed);
        }
        _ if released => {}
        _ => {
            let modifiers = PS2_MODIFIERS.load(Ordering::Relaxed);
            let shift = modifiers & (LEFT_SHIFT | RIGHT_SHIFT) != 0;
            let caps_lock = modifiers & CAPS_LOCK != 0;
            let control = modifiers & (LEFT_CTRL | RIGHT_CTRL) != 0;
            if let Some(character) = ps2_keycode_to_ascii(key, shift, caps_lock, control) {
                let _ = push_char(character);
            }
        }
    }
}

fn update_modifier(mask: u8, released: bool) {
    if released {
        PS2_MODIFIERS.fetch_and(!mask, Ordering::Relaxed);
    } else {
        PS2_MODIFIERS.fetch_or(mask, Ordering::Relaxed);
    }
}

fn ps2_keycode_to_ascii(key: u8, shift: bool, caps_lock: bool, control: bool) -> Option<char> {
    if control {
        return match key {
            0x10 => Some('\x11'),
            0x1F => Some('\x13'),
            0x20 => Some('\x04'),
            _ => None,
        };
    }

    let letter = match key {
        0x10 => Some(b'q'),
        0x11 => Some(b'w'),
        0x12 => Some(b'e'),
        0x13 => Some(b'r'),
        0x14 => Some(b't'),
        0x15 => Some(b'y'),
        0x16 => Some(b'u'),
        0x17 => Some(b'i'),
        0x18 => Some(b'o'),
        0x19 => Some(b'p'),
        0x1E => Some(b'a'),
        0x1F => Some(b's'),
        0x20 => Some(b'd'),
        0x21 => Some(b'f'),
        0x22 => Some(b'g'),
        0x23 => Some(b'h'),
        0x24 => Some(b'j'),
        0x25 => Some(b'k'),
        0x26 => Some(b'l'),
        0x2C => Some(b'z'),
        0x2D => Some(b'x'),
        0x2E => Some(b'c'),
        0x2F => Some(b'v'),
        0x30 => Some(b'b'),
        0x31 => Some(b'n'),
        0x32 => Some(b'm'),
        _ => None,
    };
    if let Some(letter) = letter {
        let uppercase = shift ^ caps_lock;
        return Some(if uppercase {
            letter.to_ascii_uppercase() as char
        } else {
            letter as char
        });
    }

    let (normal, shifted) = match key {
        0x02 => (b'1', b'!'),
        0x03 => (b'2', b'@'),
        0x04 => (b'3', b'#'),
        0x05 => (b'4', b'$'),
        0x06 => (b'5', b'%'),
        0x07 => (b'6', b'^'),
        0x08 => (b'7', b'&'),
        0x09 => (b'8', b'*'),
        0x0A => (b'9', b'('),
        0x0B => (b'0', b')'),
        0x0C => (b'-', b'_'),
        0x0D => (b'=', b'+'),
        0x1A => (b'[', b'{'),
        0x1B => (b']', b'}'),
        0x27 => (b';', b':'),
        0x28 => (b'\'', b'"'),
        0x29 => (b'`', b'~'),
        0x2B => (b'\\', b'|'),
        0x33 => (b',', b'<'),
        0x34 => (b'.', b'>'),
        0x35 => (b'/', b'?'),
        0x39 => (b' ', b' '),
        0x0E => (b'\x08', b'\x08'),
        0x0F => (b'\t', b'\t'),
        0x1C => (b'\n', b'\n'),
        _ => return None,
    };
    Some((if shift { shifted } else { normal }) as char)
}

#[cfg(test)]
mod tests {
    use super::ps2_keycode_to_ascii;

    #[test]
    fn maps_ps2_letters_with_shift_and_caps_lock() {
        assert_eq!(ps2_keycode_to_ascii(0x1E, false, false, false), Some('a'));
        assert_eq!(ps2_keycode_to_ascii(0x1E, true, false, false), Some('A'));
        assert_eq!(ps2_keycode_to_ascii(0x1E, false, true, false), Some('A'));
        assert_eq!(ps2_keycode_to_ascii(0x1E, true, true, false), Some('a'));
    }

    #[test]
    fn maps_ps2_control_keys_and_shifted_symbols() {
        assert_eq!(ps2_keycode_to_ascii(0x1C, false, false, false), Some('\n'));
        assert_eq!(
            ps2_keycode_to_ascii(0x0E, false, false, false),
            Some('\x08')
        );
        assert_eq!(ps2_keycode_to_ascii(0x02, true, false, false), Some('!'));
        assert_eq!(ps2_keycode_to_ascii(0x35, false, false, false), Some('/'));
        assert_eq!(ps2_keycode_to_ascii(0x35, true, false, false), Some('?'));
        assert_eq!(ps2_keycode_to_ascii(0x3B, false, false, false), None);
        assert_eq!(ps2_keycode_to_ascii(0x10, false, false, true), Some('\x11'));
        assert_eq!(ps2_keycode_to_ascii(0x1F, false, false, true), Some('\x13'));
        assert_eq!(ps2_keycode_to_ascii(0x20, false, false, true), Some('\x04'));
    }
}
