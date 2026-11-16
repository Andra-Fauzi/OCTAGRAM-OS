//! Kernel log buffer.
//!
//! Everything printed through `println!` is also copied here so it can be saved
//! to a file later (`savelog <path>` in the shell), for example onto a USB stick
//! mounted with `mount usb0 /mnt`. The buffer keeps the first `HEAD_CAPACITY`
//! bytes untouched (the boot log, which is what matters for driver bring-up) and
//! a ring of the most recent `TAIL_CAPACITY` bytes, so a driver that keeps
//! retrying in the background cannot push the boot log out.

use spin::Mutex;
use alloc::vec::Vec;

const HEAD_CAPACITY: usize = 96 * 1024;
const TAIL_CAPACITY: usize = 160 * 1024;

struct LogBuffer {
    head: [u8; HEAD_CAPACITY],
    head_len: usize,
    tail: [u8; TAIL_CAPACITY],
    /// Index where the next tail byte is written.
    tail_next: usize,
    tail_len: usize,
    /// Bytes that were overwritten in the tail ring.
    dropped: usize,
}

impl LogBuffer {
    const fn new() -> Self {
        Self {
            head: [0; HEAD_CAPACITY],
            head_len: 0,
            tail: [0; TAIL_CAPACITY],
            tail_next: 0,
            tail_len: 0,
            dropped: 0,
        }
    }

    fn push(&mut self, bytes: &[u8]) {
        let mut bytes = bytes;
        if self.head_len < HEAD_CAPACITY {
            let take = bytes.len().min(HEAD_CAPACITY - self.head_len);
            self.head[self.head_len..self.head_len + take].copy_from_slice(&bytes[..take]);
            self.head_len += take;
            bytes = &bytes[take..];
        }
        for &byte in bytes {
            self.tail[self.tail_next] = byte;
            self.tail_next = (self.tail_next + 1) % TAIL_CAPACITY;
            if self.tail_len < TAIL_CAPACITY {
                self.tail_len += 1;
            } else {
                self.dropped += 1;
            }
        }
    }

    fn to_vec(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.head_len + self.tail_len + 64);
        out.extend_from_slice(&self.head[..self.head_len]);
        if self.dropped > 0 {
            out.extend_from_slice(
                alloc::format!("\n[... {} bytes of log omitted ...]\n", self.dropped).as_bytes(),
            );
        }
        let start = (self.tail_next + TAIL_CAPACITY - self.tail_len) % TAIL_CAPACITY;
        for index in 0..self.tail_len {
            out.push(self.tail[(start + index) % TAIL_CAPACITY]);
        }
        out
    }
}

static LOG: Mutex<LogBuffer> = Mutex::new(LogBuffer::new());

/// Append text to the log. Never blocks: if the buffer is busy (for example a
/// print from an interrupt handler while the shell is saving the log) the text
/// is dropped from the log, not from the screen.
pub fn record(text: &str) {
    if let Some(mut log) = LOG.try_lock() {
        log.push(text.as_bytes());
    }
}

/// Copy of the whole log, oldest byte first.
pub fn snapshot() -> Vec<u8> {
    LOG.lock().to_vec()
}

#[cfg(test)]
mod tests {
    use super::{HEAD_CAPACITY, LogBuffer, TAIL_CAPACITY};
    use alloc::boxed::Box;

    #[test]
    fn short_log_is_returned_unchanged() {
        let mut log = Box::new(LogBuffer::new());
        log.push(b"hello ");
        log.push(b"world");
        assert_eq!(log.to_vec(), b"hello world");
    }

    #[test]
    fn head_is_kept_and_tail_is_a_ring() {
        let mut log = Box::new(LogBuffer::new());
        log.push(&alloc::vec![b'h'; HEAD_CAPACITY]);
        log.push(&alloc::vec![b'a'; TAIL_CAPACITY]);
        log.push(b"END");
        let out = log.to_vec();
        assert!(out.starts_with(&alloc::vec![b'h'; HEAD_CAPACITY]));
        assert!(out.ends_with(b"END"));
        assert!(
            core::str::from_utf8(&out)
                .unwrap()
                .contains("bytes of log omitted")
        );
    }
}
