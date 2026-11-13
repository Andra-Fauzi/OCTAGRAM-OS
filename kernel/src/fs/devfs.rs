use alloc::string::String;
use alloc::vec::Vec;
use core::arch::asm;
use spin::Mutex;

use super::{DirectoryEntry, FileSystem, FsError, Metadata, NodeType, normalize_path};

const TTY_BUFFER_SIZE: usize = 256;

#[derive(Clone, Copy, Debug)]
enum Device {
    Tty,
    Stdin,
    Stdout,
    Stderr,
    Console,
}

impl Device {
    fn from_path(path: &str) -> Option<Self> {
        match path {
            "/tty" => Some(Self::Tty),
            "/stdin" => Some(Self::Stdin),
            "/stdout" => Some(Self::Stdout),
            "/stderr" => Some(Self::Stderr),
            "/console" => Some(Self::Console),
            _ => None,
        }
    }

    fn readable(self) -> bool {
        matches!(self, Self::Tty | Self::Stdin | Self::Console)
    }

    fn writable(self) -> bool {
        matches!(
            self,
            Self::Tty | Self::Stdout | Self::Stderr | Self::Console
        )
    }
}

pub(crate) fn is_tty_path(path: &str) -> bool {
    device_access(path).is_some()
}

pub(crate) fn device_access(path: &str) -> Option<(bool, bool)> {
    let path = path.strip_prefix("/dev")?;
    let device = Device::from_path(path)?;
    Some((device.readable(), device.writable()))
}

struct LineDiscipline {
    line: [u8; TTY_BUFFER_SIZE],
    line_len: usize,
    ready: [u8; TTY_BUFFER_SIZE],
    ready_start: usize,
    ready_len: usize,
}

impl LineDiscipline {
    const fn new() -> Self {
        Self {
            line: [0; TTY_BUFFER_SIZE],
            line_len: 0,
            ready: [0; TTY_BUFFER_SIZE],
            ready_start: 0,
            ready_len: 0,
        }
    }

    fn push_character(&mut self, character: char) -> TtyAction {
        match character {
            '\x08' | '\x7f' if self.line_len != 0 => {
                self.line_len -= 1;
                TtyAction::Erase
            }
            '\x08' | '\x7f' => TtyAction::Ignore,
            '\x04' if self.line_len == 0 => TtyAction::EndOfFile,
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
        if self.ready_start == self.ready_len || output.is_empty() {
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

static TTY: Mutex<LineDiscipline> = Mutex::new(LineDiscipline::new());

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
            });
        }
        Device::from_path(path)
            .map(|device| Metadata {
                node_type: NodeType::File,
                len: 0,
                permissions: (if device.readable() { 0o444 } else { 0 })
                    | (if device.writable() { 0o222 } else { 0 }),
            })
            .ok_or(FsError::NotFound)
    }
}

impl FileSystem for DevFs {
    fn metadata(&self, path: &str) -> Result<Metadata, FsError> {
        let path = normalize_path(path)?;
        Self::metadata_for(&path)
    }

    fn read(&self, path: &str, _offset: usize, buffer: &mut [u8]) -> Result<usize, FsError> {
        let path = normalize_path(path)?;
        let device = Device::from_path(&path).ok_or(FsError::NotFound)?;
        if !device.readable() {
            return Err(FsError::PermissionDenied);
        }
        read_tty(buffer)
    }

    fn write(&mut self, path: &str, _offset: usize, bytes: &[u8]) -> Result<usize, FsError> {
        let path = normalize_path(path)?;
        let device = Device::from_path(&path).ok_or(FsError::NotFound)?;
        if !device.writable() {
            return Err(FsError::PermissionDenied);
        }
        let text = core::str::from_utf8(bytes).map_err(|_| FsError::InvalidData)?;
        crate::terminal::terminal_print(text);
        Ok(bytes.len())
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
            directory_entry("stderr"),
            directory_entry("stdin"),
            directory_entry("stdout"),
            directory_entry("tty"),
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
        },
    }
}

fn read_tty(buffer: &mut [u8]) -> Result<usize, FsError> {
    if buffer.is_empty() {
        return Ok(0);
    }
    loop {
        if let Some(count) = TTY.lock().read_ready(buffer) {
            return Ok(count);
        }

        let event = match crate::drivers::keyboard::pop_event() {
            Some(event) => event,
            None => {
                unsafe {
                    asm!("sti; hlt; cli", options(nomem, nostack));
                }
                continue;
            }
        };
        let action = match event {
            crate::drivers::keyboard::KeyEvent::Character(character) => {
                TTY.lock().push_character(character)
            }
            _ => TtyAction::Ignore,
        };
        match action {
            TtyAction::Echo(character) => {
                let mut encoded = [0; 4];
                crate::terminal::terminal_print(character.encode_utf8(&mut encoded));
            }
            TtyAction::Erase => crate::terminal::terminal_backspace(),
            TtyAction::EndOfFile => return Ok(0),
            TtyAction::Ignore => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DevFs, Device, FileSystem, FsError, LineDiscipline, Metadata, NodeType, device_access,
    };

    #[test]
    fn devfs_exposes_tty_and_standard_stream_devices() {
        let mut devfs = DevFs::new();
        assert_eq!(
            devfs.metadata("/tty"),
            Ok(Metadata {
                node_type: NodeType::File,
                len: 0,
                permissions: 0o666,
            })
        );
        let entries = devfs.read_dir("/").unwrap();
        assert_eq!(entries.len(), 5);
        assert!(Device::from_path("/stdin").unwrap().readable());
        assert!(Device::from_path("/stdout").unwrap().writable());
        assert_eq!(device_access("/dev/stdin"), Some((true, false)));
        assert_eq!(device_access("/dev/stdout"), Some((false, true)));
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
