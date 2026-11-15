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
const LEFT_ALT: u8 = 1 << 5;
const RIGHT_ALT: u8 = 1 << 6;

struct InputQueue {
    events: [AtomicU32; INPUT_CAPACITY],
    head: AtomicUsize,
    tail: AtomicUsize,
    waker: Once<TaskHandle>,
}

impl InputQueue {
    const fn new() -> Self {
        Self {
            events: [const { AtomicU32::new(0) }; INPUT_CAPACITY],
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            waker: Once::new(),
        }
    }
}

static INPUT: [InputQueue; crate::terminal::TERMINAL_COUNT] =
    [const { InputQueue::new() }; crate::terminal::TERMINAL_COUNT];
static PS2_MODIFIERS: AtomicU8 = AtomicU8::new(0);
static PS2_EXTENDED: AtomicBool = AtomicBool::new(false);
static PS2_SKIP_REMAINING: AtomicU8 = AtomicU8::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyEvent {
    Character(char),
    Left,
    Right,
    Up,
    Down,
}

pub fn set_input_waker(terminal: usize, waker: TaskHandle) {
    if let Some(queue) = INPUT.get(terminal) {
        queue.waker.call_once(|| waker);
    }
}

/// Push one character from an interrupt handler. Returns false if the queue is full.
pub fn push_char(character: char) -> bool {
    push_event(KeyEvent::Character(character))
}

/// Push a character or navigation key from an interrupt handler.
pub fn push_event(event: KeyEvent) -> bool {
    push_event_to(crate::terminal::active_terminal(), event)
}

fn push_event_to(terminal: usize, event: KeyEvent) -> bool {
    let Some(queue) = INPUT.get(terminal) else {
        return false;
    };
    let head = queue.head.load(Ordering::Relaxed);
    let next = head.wrapping_add(1) & INPUT_MASK;
    if next == queue.tail.load(Ordering::Acquire) {
        return false;
    }

    let value = match event {
        KeyEvent::Character(character) => character as u32,
        KeyEvent::Left => KEY_EVENT_BASE,
        KeyEvent::Right => KEY_EVENT_BASE + 1,
        KeyEvent::Up => KEY_EVENT_BASE + 2,
        KeyEvent::Down => KEY_EVENT_BASE + 3,
    };
    queue.events[head].store(value, Ordering::Relaxed);
    queue.head.store(next, Ordering::Release);
    if let Some(waker) = queue.waker.get() {
        waker.wake();
    }
    true
}

pub fn pop_event_for(terminal: usize) -> Option<KeyEvent> {
    let queue = INPUT.get(terminal)?;
    let tail = queue.tail.load(Ordering::Relaxed);
    if tail == queue.head.load(Ordering::Acquire) {
        return None;
    }

    let value = queue.events[tail].load(Ordering::Relaxed);
    queue
        .tail
        .store(tail.wrapping_add(1) & INPUT_MASK, Ordering::Release);
    Some(match value {
        KEY_EVENT_BASE => KeyEvent::Left,
        value if value == KEY_EVENT_BASE + 1 => KeyEvent::Right,
        value if value == KEY_EVENT_BASE + 2 => KeyEvent::Up,
        value if value == KEY_EVENT_BASE + 3 => KeyEvent::Down,
        value => KeyEvent::Character(char::from_u32(value)?),
    })
}

pub async fn next_event_for(terminal: usize) -> KeyEvent {
    core::future::poll_fn(|_| {
        crate::terminal::redraw_pending();
        match pop_event_for(terminal) {
            Some(event) => core::task::Poll::Ready(event),
            None => core::task::Poll::Pending,
        }
    })
    .await
}

pub fn switch_terminal(terminal: usize) {
    if crate::terminal::switch_terminal(terminal)
        && let Some(waker) = INPUT[terminal].waker.get()
    {
        waker.wake();
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
        if key == 0x38 {
            update_modifier(RIGHT_ALT, released);
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
        0x38 => update_modifier(LEFT_ALT, released),
        0x3A if !released => {
            PS2_MODIFIERS.fetch_xor(CAPS_LOCK, Ordering::Relaxed);
        }
        0x3B..=0x3E if !released && has_ctrl_alt(PS2_MODIFIERS.load(Ordering::Relaxed)) => {
            switch_terminal((key - 0x3B) as usize);
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

fn has_ctrl_alt(modifiers: u8) -> bool {
    modifiers & (LEFT_CTRL | RIGHT_CTRL) != 0 && modifiers & (LEFT_ALT | RIGHT_ALT) != 0
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
    use super::{KeyEvent, pop_event_for, ps2_keycode_to_ascii, push_event_to};

    #[test]
    fn virtual_terminal_input_queues_are_independent() {
        assert!(push_event_to(2, KeyEvent::Character('a')));
        assert!(push_event_to(3, KeyEvent::Character('b')));
        assert_eq!(pop_event_for(2), Some(KeyEvent::Character('a')));
        assert_eq!(pop_event_for(3), Some(KeyEvent::Character('b')));
    }

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
        assert_eq!(ps2_keycode_to_ascii(0x0F, false, false, false), Some('\t'));
        assert_eq!(ps2_keycode_to_ascii(0x02, true, false, false), Some('!'));
        assert_eq!(ps2_keycode_to_ascii(0x35, false, false, false), Some('/'));
        assert_eq!(ps2_keycode_to_ascii(0x35, true, false, false), Some('?'));
        assert_eq!(ps2_keycode_to_ascii(0x3B, false, false, false), None);
        assert_eq!(ps2_keycode_to_ascii(0x10, false, false, true), Some('\x11'));
        assert_eq!(ps2_keycode_to_ascii(0x1F, false, false, true), Some('\x13'));
        assert_eq!(ps2_keycode_to_ascii(0x20, false, false, true), Some('\x04'));
    }
}
