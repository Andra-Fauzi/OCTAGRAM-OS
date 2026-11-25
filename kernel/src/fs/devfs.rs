use alloc::string::String;
use alloc::vec::Vec;
use core::arch::asm;
use spin::Mutex;

use super::{DirectoryEntry, FileSystem, FsError, Metadata, NodeType, normalize_path};

const TTY_BUFFER_SIZE: usize = 256;

#[derive(Clone, Copy, Debug)]
enum Device {
    Tty(Option<usize>),
    Stdin,
    Stdout,
    Stderr,
    Console,
    Fb0,
    Null,
}

impl Device {
    fn from_path(path: &str) -> Option<Self> {
        match path {
            "/tty" => Some(Self::Tty(None)),
            "/tty0" => Some(Self::Tty(Some(0))),
            "/tty1" => Some(Self::Tty(Some(1))),
            "/tty2" => Some(Self::Tty(Some(2))),
            "/tty3" => Some(Self::Tty(Some(3))),
            "/stdin" => Some(Self::Stdin),
            "/stdout" => Some(Self::Stdout),
            "/stderr" => Some(Self::Stderr),
            "/console" => Some(Self::Console),
            "/fb" | "/fb0" => Some(Self::Fb0),
            "/null" => Some(Self::Null),
            _ => None,
        }
    }

    fn readable(self) -> bool {
        matches!(
            self,
            Self::Tty(_) | Self::Stdin | Self::Console | Self::Fb0 | Self::Null
        )
    }

    fn writable(self) -> bool {
        matches!(
            self,
            Self::Tty(_) | Self::Stdout | Self::Stderr | Self::Console | Self::Fb0 | Self::Null
        )
    }

    fn terminal(self) -> usize {
        match self {
            Self::Tty(Some(terminal)) => terminal,
            _ => crate::terminal::active_terminal(),
        }
    }
}

pub(crate) fn is_tty_path(path: &str) -> bool {
    let Some(path) = path.strip_prefix("/dev") else {
        return false;
    };
    matches!(
        Device::from_path(path),
        Some(Device::Tty(_) | Device::Stdin | Device::Stdout | Device::Stderr | Device::Console)
    )
}

/// Terminal yang dilayani sebuah path `/dev` yang bisa dibaca sebagai tty.
/// `None` berarti path bukan tty yang bisa dibaca (mis. `/dev/fb0` atau file
/// biasa), sehingga pembaca tidak boleh memblokir sebagai input console.
pub(crate) fn tty_path_terminal(path: &str) -> Option<usize> {
    let path = path.strip_prefix("/dev")?;
    let device = Device::from_path(path)?;
    if device.readable() && !matches!(device, Device::Fb0) {
        Some(device.terminal())
    } else {
        None
    }
}

pub(crate) fn is_fb_path(path: &str) -> bool {
    path == "/dev/fb0" || path == "/dev/fb"
}

/// `/dev/null` adalah perangkat karakter yang membuang tulisan dan selalu EOF.
pub(crate) fn is_null_path(path: &str) -> bool {
    path == "/dev/null"
}

pub(crate) fn device_access(path: &str) -> Option<(bool, bool)> {
    let path = path.strip_prefix("/dev")?;
    let device = Device::from_path(path)?;
    Some((device.readable(), device.writable()))
}

// Layout `struct termios` x86_64 seperti glibc/Linux.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Termios {
    input_flags: u32,
    output_flags: u32,
    control_flags: u32,
    local_flags: u32,
    line: u8,
    control_characters: [u8; 32],
    input_speed: u32,
    output_speed: u32,
}

const ICRNL: u32 = 0x100;
const IXON: u32 = 0x400;
const OPOST: u32 = 0x1;
const ONLCR: u32 = 0x4;
const ISIG: u32 = 0x1;
const ICANON: u32 = 0x2;
const ECHO: u32 = 0x8;
const ECHOE: u32 = 0x10;
const ECHOK: u32 = 0x20;
const IEXTEN: u32 = 0x8000;
const CS8: u32 = 0x30;
const CREAD: u32 = 0x80;
const B38400: u32 = 0x0F;

impl Termios {
    const fn new() -> Self {
        let mut control_characters = [0u8; 32];
        control_characters[0] = 3;   // VINTR
        control_characters[1] = 28;  // VQUIT
        control_characters[2] = 127; // VERASE
        control_characters[3] = 21;  // VKILL
        control_characters[4] = 4;   // VEOF
        control_characters[5] = 0;   // VTIME
        control_characters[6] = 1;   // VMIN
        control_characters[8] = 17;  // VSTART
        control_characters[9] = 19;  // VSTOP
        control_characters[10] = 26; // VSUSP
        control_characters[12] = 18; // VREPRINT
        control_characters[13] = 15; // VDISCARD
        control_characters[14] = 23; // VWERASE
        control_characters[15] = 22; // VLNEXT
        Self {
            input_flags: ICRNL | IXON,
            output_flags: OPOST | ONLCR,
            control_flags: CREAD | CS8 | B38400,
            local_flags: ISIG | ICANON | ECHO | ECHOE | ECHOK | IEXTEN,
            line: 0,
            control_characters,
            input_speed: B38400,
            output_speed: B38400,
        }
    }
}

static CONSOLE_TERMIOS: Mutex<Termios> = Mutex::new(Termios::new());

pub(crate) fn console_termios() -> Termios {
    *CONSOLE_TERMIOS.lock()
}

/// Mengubah state termios console. Saat `ICANON` dimatikan, baris canonical yang
/// belum selesai diteruskan ke buffer baca supaya `read()` tidak menggantung.
pub(crate) fn set_console_termios(termios: Termios) {
    let mut current = CONSOLE_TERMIOS.lock();
    let was_canonical = current.local_flags & ICANON != 0;
    *current = termios;
    let is_canonical = current.local_flags & ICANON != 0;
    drop(current);
    if was_canonical && !is_canonical {
        for tty in &TTY {
            tty.lock().flush_partial_line();
        }
    }
}

/// `TCSAFLUSH`: buang semua input yang belum dibaca, termasuk baris parsial.
pub(crate) fn discard_pending_input() {
    for tty in &TTY {
        tty.lock().discard_input();
    }
}

/// Satu kali lock untuk kedua flag, dipakai jalur input per karakter.
pub(crate) fn console_mode() -> (bool, bool) {
    let flags = CONSOLE_TERMIOS.lock().local_flags;
    (flags & ICANON != 0, flags & ECHO != 0)
}

struct LineDiscipline {
    line: [u8; TTY_BUFFER_SIZE],
    line_len: usize,
    ready: [u8; TTY_BUFFER_SIZE],
    ready_start: usize,
    ready_len: usize,
    /// `Ctrl-D` pada baris kosong; `read` berikutnya mengembalikan 0 (EOF).
    eof_pending: bool,
}

impl LineDiscipline {
    const fn new() -> Self {
        Self {
            line: [0; TTY_BUFFER_SIZE],
            line_len: 0,
            ready: [0; TTY_BUFFER_SIZE],
            ready_start: 0,
            ready_len: 0,
            eof_pending: false,
        }
    }

    fn push_character(&mut self, character: char) -> TtyAction {
        match character {
            '\x08' | '\x7f' if self.line_len != 0 => {
                self.line_len -= 1;
                TtyAction::Erase
            }
            '\x08' | '\x7f' => TtyAction::Ignore,
            '\x04' if self.line_len == 0 => {
                self.eof_pending = true;
                TtyAction::EndOfFile
            }
            '\x04' => {
                self.finish_line(false);
                TtyAction::Ignore
            }
            '\n' | '\r' => {
                self.finish_line(true);
                TtyAction::Echo('\n')
            }
            character if character.is_ascii_graphic() || character == ' ' => {
                if self.line_len == TTY_BUFFER_SIZE - 1 {
                    return TtyAction::Ignore;
                }
                self.line[self.line_len] = character as u8;
                self.line_len += 1;
                TtyAction::Echo(character)
            }
            _ => TtyAction::Ignore,
        }
    }

    /// Menggeser sisa data yang belum dibaca ke awal buffer.
    fn compact_ready(&mut self) {
        if self.ready_start == 0 {
            return;
        }
        let remaining = self.ready_len - self.ready_start;
        self.ready.copy_within(self.ready_start..self.ready_len, 0);
        self.ready_start = 0;
        self.ready_len = remaining;
    }

    /// Memindahkan baris canonical yang belum selesai ke buffer siap baca.
    fn flush_partial_line(&mut self) {
        if self.line_len == 0 {
            return;
        }
        self.compact_ready();
        let room = TTY_BUFFER_SIZE - self.ready_len;
        let count = self.line_len.min(room);
        self.ready[self.ready_len..self.ready_len + count]
            .copy_from_slice(&self.line[..count]);
        self.ready_len += count;
        self.line_len = 0;
    }

    /// `TCSAFLUSH`: buang baris parsial dan data yang belum dibaca.
    fn discard_input(&mut self) {
        self.line_len = 0;
        self.ready_start = 0;
        self.ready_len = 0;
        self.eof_pending = false;
    }

    /// True bila ada data siap dibaca atau EOF tertunda.
    fn has_input(&self) -> bool {
        self.ready_start < self.ready_len || self.eof_pending
    }

    /// Mode raw: byte langsung masuk ke buffer siap baca tanpa editing baris.
    fn push_raw(&mut self, character: char) -> TtyAction {
        let mut encoded = [0u8; 4];
        let bytes = character.encode_utf8(&mut encoded).as_bytes();
        self.compact_ready();
        if self.ready_len + bytes.len() > TTY_BUFFER_SIZE {
            return TtyAction::Ignore;
        }
        self.ready[self.ready_len..self.ready_len + bytes.len()].copy_from_slice(bytes);
        self.ready_len += bytes.len();
        if character.is_control() {
            TtyAction::Ignore
        } else {
            TtyAction::Echo(character)
        }
    }

    fn finish_line(&mut self, newline: bool) {
        self.ready[..self.line_len].copy_from_slice(&self.line[..self.line_len]);
        self.ready_len = self.line_len;
        if newline {
            self.ready[self.ready_len] = b'\n';
            self.ready_len += 1;
        }
        self.ready_start = 0;
        self.line_len = 0;
    }

    fn read_ready(&mut self, output: &mut [u8]) -> Option<usize> {
        if output.is_empty() {
            return None;
        }
        if self.ready_start == self.ready_len {
            if self.eof_pending {
                self.eof_pending = false;
                return Some(0);
            }
            return None;
        }
        let count = output.len().min(self.ready_len - self.ready_start);
        output[..count].copy_from_slice(&self.ready[self.ready_start..self.ready_start + count]);
        self.ready_start += count;
        if self.ready_start == self.ready_len {
            self.ready_start = 0;
            self.ready_len = 0;
        }
        Some(count)
    }
}

enum TtyAction {
    Echo(char),
    Erase,
    EndOfFile,
    Ignore,
}

static TTY: [Mutex<LineDiscipline>; crate::terminal::TERMINAL_COUNT] =
    [const { Mutex::new(LineDiscipline::new()) }; crate::terminal::TERMINAL_COUNT];

pub struct DevFs;

impl DevFs {
    pub const fn new() -> Self {
        Self
    }

    fn metadata_for(path: &str) -> Result<Metadata, FsError> {
        if path == "/" {
            return Ok(Metadata {
                node_type: NodeType::Directory,
                len: 0,
                permissions: 0o755,
                modified: None,
            });
        }
        Device::from_path(path)
            .map(|device| {
                let len = match device {
                    Device::Fb0 => crate::graphics::Framebuffer::get()
                        .map_or(0, |fb| (fb.pitch() * fb.height()) as usize),
                    _ => 0,
                };
                Metadata {
                    node_type: NodeType::File,
                    len,
                    permissions: (if device.readable() { 0o444 } else { 0 })
                        | (if device.writable() { 0o222 } else { 0 }),
                    modified: None,
                }
            })
            .ok_or(FsError::NotFound)
    }
}

impl FileSystem for DevFs {
    fn name(&self) -> &'static str {
        "devfs"
    }
    fn metadata(&self, path: &str) -> Result<Metadata, FsError> {
        let path = normalize_path(path)?;
        Self::metadata_for(&path)
    }

    fn read(&self, path: &str, offset: usize, buffer: &mut [u8]) -> Result<usize, FsError> {
        let path = normalize_path(path)?;
        let device = Device::from_path(&path).ok_or(FsError::NotFound)?;
        if !device.readable() {
            return Err(FsError::PermissionDenied);
        }
        match device {
            Device::Fb0 => {
                let fb = crate::graphics::Framebuffer::get().ok_or(FsError::NotFound)?;
                let total_len = (fb.pitch() * fb.height()) as usize;
                if offset >= total_len {
                    return Ok(0);
                }
                let to_read = buffer.len().min(total_len - offset);
                unsafe {
                    let src = fb.addr().add(offset);
                    core::ptr::copy_nonoverlapping(src, buffer.as_mut_ptr(), to_read);
                }
                Ok(to_read)
            }
            Device::Null => Ok(0),
            _ => read_tty(device.terminal(), buffer),
        }
    }

    fn write(&mut self, path: &str, offset: usize, bytes: &[u8]) -> Result<usize, FsError> {
        let path = normalize_path(path)?;
        let device = Device::from_path(&path).ok_or(FsError::NotFound)?;
        if !device.writable() {
            return Err(FsError::PermissionDenied);
        }
        match device {
            Device::Fb0 => {
                let fb = crate::graphics::Framebuffer::get().ok_or(FsError::NotFound)?;
                let total_len = (fb.pitch() * fb.height()) as usize;
                if offset >= total_len {
                    return Ok(0);
                }
                let to_write = bytes.len().min(total_len - offset);
                unsafe {
                    let dst = fb.addr().add(offset);
                    core::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, to_write);
                }
                Ok(to_write)
            }
            Device::Null => Ok(bytes.len()),
            _ => {
                let text = core::str::from_utf8(bytes).map_err(|_| FsError::InvalidData)?;
                crate::terminal::terminal_print_to(device.terminal(), text);
                // Mirror terminal output to COM1 so headless boots can observe
                // program output; the kernel's own print! already writes serial.
                crate::drivers::serial::write_str(text);
                Ok(bytes.len())
            }
        }
    }

    fn truncate(&mut self, path: &str, _len: usize) -> Result<(), FsError> {
        let path = normalize_path(path)?;
        Self::metadata_for(&path)?;
        Err(FsError::ReadOnly)
    }

    fn create_file(&mut self, _path: &str) -> Result<(), FsError> {
        Err(FsError::ReadOnly)
    }

    fn create_dir(&mut self, _path: &str) -> Result<(), FsError> {
        Err(FsError::ReadOnly)
    }

    fn remove(&mut self, path: &str) -> Result<(), FsError> {
        let path = normalize_path(path)?;
        Self::metadata_for(&path)?;
        Err(FsError::ReadOnly)
    }

    fn read_dir(&self, path: &str) -> Result<Vec<DirectoryEntry>, FsError> {
        let path = normalize_path(path)?;
        if path != "/" {
            return match Self::metadata_for(&path) {
                Ok(_) => Err(FsError::NotDirectory),
                Err(error) => Err(error),
            };
        }
        Ok(alloc::vec![
            directory_entry("console"),
            directory_entry("fb0"),
            directory_entry("null"),
            directory_entry("stderr"),
            directory_entry("stdin"),
            directory_entry("stdout"),
            directory_entry("tty"),
            directory_entry("tty0"),
            directory_entry("tty1"),
            directory_entry("tty2"),
            directory_entry("tty3"),
        ])
    }
}

fn directory_entry(name: &str) -> DirectoryEntry {
    DirectoryEntry {
        name: String::from(name),
        metadata: Metadata {
            node_type: NodeType::File,
            len: 0,
            permissions: Device::from_path(&alloc::format!("/{name}"))
                .map(|device| {
                    (if device.readable() { 0o444 } else { 0 })
                        | (if device.writable() { 0o222 } else { 0 })
                })
                .unwrap_or(0o444),
            modified: None,
        },
    }
}

/// Move queued keyboard events into the terminal's line discipline so a
/// finished line becomes visible without waiting for a `read`.
/// Returns true when Ctrl-D asked for EOF.
fn pump_keyboard_into_line(terminal: usize) -> bool {
    let Some(tty) = TTY.get(terminal) else {
        return false;
    };
    let mut eof = false;
    while let Some(event) = crate::drivers::keyboard::pop_event_for(terminal) {
        let crate::drivers::keyboard::KeyEvent::Character(character) = event else {
            continue;
        };
        let (canonical, echo) = console_mode();
        let action = if canonical {
            tty.lock().push_character(character)
        } else {
            tty.lock().push_raw(character)
        };
        match action {
            TtyAction::Echo(character) => {
                if echo {
                    let mut encoded = [0; 4];
                    crate::terminal::terminal_print_to(
                        terminal,
                        character.encode_utf8(&mut encoded),
                    );
                }
            }
            TtyAction::Erase => crate::terminal::terminal_backspace_for(terminal),
            TtyAction::EndOfFile => eof = true,
            TtyAction::Ignore => {}
        }
    }
    eof
}

/// True when a complete canonical line (or EOF) is waiting on the terminal.
pub(crate) fn tty_readable(terminal: usize) -> bool {
    unsafe { crate::drivers::poll_input() };
    pump_keyboard_into_line(terminal);
    TTY.get(terminal).is_some_and(|tty| tty.lock().has_input())
}

/// Salin event keyboard yang tersedia ke line discipline semua terminal tanpa
/// menunggu, lalu kembalikan bitmask terminal yang punya data/EOF siap dibaca.
pub(crate) fn pump_console_input() -> u32 {
    unsafe { crate::drivers::poll_input() };
    let mut mask = 0u32;
    for terminal in 0..crate::terminal::TERMINAL_COUNT {
        pump_keyboard_into_line(terminal);
        let ready = TTY.get(terminal).is_some_and(|tty| tty.lock().has_input());
        if ready && terminal < u32::BITS as usize {
            mask |= 1 << terminal;
        }
    }
    mask
}

/// Readiness of a `/dev` path used by poll/select. `None` means the path is
/// not a readable terminal device (a regular file, for example).
pub(crate) fn tty_path_readable(path: &str) -> Option<bool> {
    let path = path.strip_prefix("/dev")?;
    match Device::from_path(path)? {
        Device::Fb0 => Some(true),
        device if device.readable() => Some(tty_readable(device.terminal())),
        _ => None,
    }
}

/// Baca terminal tanpa menunggu: `Ok(Some(0))` berarti EOF, `Ok(None)` berarti
/// belum ada input. Semua efek samping (echo, backspace, EOF) tetap dijalankan.
pub(crate) fn poll_tty(terminal: usize, buffer: &mut [u8]) -> Result<Option<usize>, FsError> {
    if buffer.is_empty() {
        return Ok(Some(0));
    }
    let tty = TTY.get(terminal).ok_or(FsError::InvalidPath)?;
    if let Some(count) = tty.lock().read_ready(buffer) {
        return Ok(Some(count));
    }
    // Keyboard USB tidak lagi diproses di ISR (lihat `xhci::INTERRUPT_PENDING`),
    // jadi event ring harus di-poll sebelum menyatakan belum ada input.
    unsafe { crate::drivers::poll_input() };
    pump_keyboard_into_line(terminal);
    Ok(tty.lock().read_ready(buffer))
}

/// Versi blocking untuk pemanggil kernel (mis. `/dev/stdin` lewat VFS).
pub(crate) fn read_tty(terminal: usize, buffer: &mut [u8]) -> Result<usize, FsError> {
    if buffer.is_empty() {
        return Ok(0);
    }
    loop {
        if let Some(count) = poll_tty(terminal, buffer)? {
            return Ok(count);
        }
        unsafe {
            asm!("sti; hlt; cli", options(nomem, nostack));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DevFs, Device, FileSystem, FsError, LineDiscipline, Metadata, NodeType, device_access,
        tty_path_terminal,
    };

    #[test]
    fn tty_path_terminal_matches_readable_terminals_only() {
        assert_eq!(tty_path_terminal("/dev/tty2"), Some(2));
        assert_eq!(
            tty_path_terminal("/dev/stdin"),
            Some(crate::terminal::active_terminal())
        );
        assert_eq!(
            tty_path_terminal("/dev/console"),
            Some(crate::terminal::active_terminal())
        );
        assert_eq!(tty_path_terminal("/dev/fb0"), None);
        assert_eq!(tty_path_terminal("/dev/stdout"), None);
        assert_eq!(tty_path_terminal("/etc/hosts"), None);
    }

    #[test]
    fn devfs_exposes_tty_and_standard_stream_devices() {
        let mut devfs = DevFs::new();
        assert_eq!(
            devfs.metadata("/tty"),
            Ok(Metadata {
                node_type: NodeType::File,
                len: 0,
                permissions: 0o666,
                modified: None,
            })
        );
        let entries = devfs.read_dir("/").unwrap();
        assert_eq!(entries.len(), 11);
        assert!(entries.iter().any(|entry| entry.name == "fb0"));
        assert!(entries.iter().any(|entry| entry.name == "tty3"));
        assert!(Device::from_path("/stdin").unwrap().readable());
        assert!(Device::from_path("/stdout").unwrap().writable());
        assert_eq!(device_access("/dev/stdin"), Some((true, false)));
        assert_eq!(device_access("/dev/stdout"), Some((false, true)));
        assert_eq!(device_access("/dev/tty2"), Some((true, true)));
        assert_eq!(
            devfs.write("/stdin", 0, b"no"),
            Err(FsError::PermissionDenied)
        );
        assert_eq!(devfs.write("/stdout", 0, b"ok\n"), Ok(3));
        assert_eq!(
            devfs.write("/stdout", 0, &[0xff]),
            Err(FsError::InvalidData)
        );
    }

    #[test]
    fn canonical_eof_is_reported_once_when_the_line_is_empty() {
        let mut tty = LineDiscipline::new();
        assert!(matches!(
            tty.push_character('\x04'),
            super::TtyAction::EndOfFile
        ));
        assert!(tty.has_input());
        let mut buffer = [0u8; 4];
        assert_eq!(tty.read_ready(&mut buffer), Some(0));
        assert!(!tty.has_input());
        assert_eq!(tty.read_ready(&mut buffer), None);
    }

    #[test]
    fn canonical_input_echoes_edits_and_reads_partial_lines() {
        let mut tty = LineDiscipline::new();
        assert!(matches!(
            tty.push_character('h'),
            super::TtyAction::Echo('h')
        ));
        assert!(matches!(
            tty.push_character('x'),
            super::TtyAction::Echo('x')
        ));
        assert!(matches!(
            tty.push_character('\x08'),
            super::TtyAction::Erase
        ));
        tty.push_character('i');
        tty.push_character('\n');

        let mut first = [0; 2];
        assert_eq!(tty.read_ready(&mut first), Some(2));
        assert_eq!(&first, b"hi");
        let mut second = [0; 2];
        assert_eq!(tty.read_ready(&mut second), Some(1));
        assert_eq!(second[0], b'\n');
        assert_eq!(tty.read_ready(&mut second), None);
    }

    #[test]
    fn disabling_canonical_flushes_partial_line() {
        let mut tty = LineDiscipline::new();
        tty.push_character('h');
        tty.push_character('i');
        // Belum ada newline, jadi belum ada yang bisa dibaca.
        let mut buffer = [0; 4];
        assert_eq!(tty.read_ready(&mut buffer), None);
        // Setelah ICANON dimatikan, baris parsial harus segera terbaca.
        tty.flush_partial_line();
        assert_eq!(tty.read_ready(&mut buffer), Some(2));
        assert_eq!(&buffer[..2], b"hi");
        assert_eq!(tty.read_ready(&mut buffer), None);
        // Baris kosong tidak mengubah apa pun.
        tty.flush_partial_line();
        assert_eq!(tty.read_ready(&mut buffer), None);
    }

    #[test]
    fn flush_appends_partial_line_after_ready_data() {
        let mut tty = LineDiscipline::new();
        tty.push_character('a');
        tty.push_character('\n');
        tty.push_character('b');
        tty.flush_partial_line();
        let mut buffer = [0; 4];
        assert_eq!(tty.read_ready(&mut buffer), Some(3));
        assert_eq!(&buffer[..3], b"a\nb");
    }

    #[test]
    fn tcsaflush_discards_partial_and_ready_input() {
        let mut tty = LineDiscipline::new();
        tty.push_character('x');
        tty.push_character('\n');
        tty.push_character('y');
        tty.discard_input();
        let mut buffer = [0; 4];
        assert_eq!(tty.read_ready(&mut buffer), None);
    }

    #[test]
    fn raw_input_bypasses_line_editing() {
        let mut tty = LineDiscipline::new();
        // Mode canonical: belum ada yang bisa dibaca sebelum newline.
        tty.push_character('a');
        let mut buffer = [0; 4];
        assert_eq!(tty.read_ready(&mut buffer), None);
        // Mode raw: byte langsung siap dibaca, termasuk karakter kontrol.
        assert!(matches!(tty.push_raw('b'), super::TtyAction::Echo('b')));
        assert!(matches!(tty.push_raw('\x04'), super::TtyAction::Ignore));
        assert_eq!(tty.read_ready(&mut buffer), Some(2));
        assert_eq!(&buffer[..2], b"b\x04");
        assert!(matches!(tty.push_raw('\u{20ac}'), super::TtyAction::Echo('\u{20ac}')));
        assert_eq!(tty.read_ready(&mut buffer), Some(3));
        assert_eq!(&buffer[..3], "\u{20ac}".as_bytes());
    }

    #[test]
    fn tty_reports_readable_only_for_complete_lines() {
        assert!(!super::tty_readable(2));
        assert_eq!(super::tty_path_readable("/dev/tty2"), Some(false));
        assert_eq!(super::tty_path_readable("/etc/hosts"), None);

        let tty = super::TTY.get(2).unwrap();
        for character in ['h', 'i'] {
            tty.lock().push_character(character);
        }
        assert!(!super::tty_readable(2));
        tty.lock().push_character('\n');
        assert!(super::tty_readable(2));
        assert_eq!(super::tty_path_readable("/dev/tty2"), Some(true));

        let mut output = [0; 3];
        assert_eq!(tty.lock().read_ready(&mut output), Some(3));
        assert!(!super::tty_readable(2));
    }

    #[test]
    fn devfs_stdin_reads_a_line_from_the_shared_keyboard_queue() {
        let devfs = DevFs::new();
        for character in ['o', 'k', '\n'] {
            assert!(crate::drivers::keyboard::push_char(character));
        }

        let mut output = [0; 8];
        assert_eq!(devfs.read("/stdin", 0, &mut output), Ok(3));
        assert_eq!(&output[..3], b"ok\n");
    }

    #[test]
    fn control_d_returns_eof_or_the_pending_line() {
        let mut tty = LineDiscipline::new();
        assert!(matches!(
            tty.push_character('\x04'),
            super::TtyAction::EndOfFile
        ));
        tty.push_character('x');
        tty.push_character('\x04');
        let mut output = [0; 2];
        assert_eq!(tty.read_ready(&mut output), Some(1));
        assert_eq!(&output[..1], b"x");
    }
}
