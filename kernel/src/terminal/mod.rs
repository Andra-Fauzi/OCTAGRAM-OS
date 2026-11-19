//! Framebuffer-backed virtual terminals.

use crate::graphics::{Color, Framebuffer};
use crate::terminal::font::FONT8X16;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use spin::Mutex;

pub mod font;

use core::fmt::{self, Write};

pub const TERMINAL_COUNT: usize = 4;
const MAX_COLUMNS: usize = 160;
const MAX_ROWS: usize = 60;
const CHAR_W: u64 = 8;
const CHAR_H: u64 = 16;

struct TerminalState {
    cells: [[u16; MAX_COLUMNS]; MAX_ROWS],
    column: usize,
    row: usize,
}

impl TerminalState {
    const fn new() -> Self {
        Self {
            cells: [[b' ' as u16; MAX_COLUMNS]; MAX_ROWS],
            column: 0,
            row: 0,
        }
    }

    fn dimensions(&self, framebuffer: Option<&Framebuffer>) -> (usize, usize) {
        framebuffer.map_or((80, 25), |fb| {
            (
                ((fb.width() / CHAR_W) as usize).clamp(1, MAX_COLUMNS),
                ((fb.height() / CHAR_H) as usize).clamp(1, MAX_ROWS),
            )
        })
    }
}

static TERMINALS: [Mutex<TerminalState>; TERMINAL_COUNT] =
    [const { Mutex::new(TerminalState::new()) }; TERMINAL_COUNT];
static ACTIVE_TERMINAL: AtomicUsize = AtomicUsize::new(0);
static OUTPUT_TERMINAL: AtomicUsize = AtomicUsize::new(0);
static REDRAW_PENDING: AtomicBool = AtomicBool::new(false);

/// Return the currently focused virtual terminal.
pub fn active_terminal() -> usize {
    ACTIVE_TERMINAL.load(Ordering::Acquire)
}

/// Return the active terminal's text dimensions in character cells.
pub fn terminal_dimensions() -> (usize, usize) {
    let framebuffer = Framebuffer::get();
    TERMINALS[active_terminal()]
        .lock()
        .dimensions(framebuffer.as_ref())
}

/// Select the terminal used by default kernel output.
pub fn set_output_terminal(terminal: usize) {
    if terminal < TERMINAL_COUNT {
        OUTPUT_TERMINAL.store(terminal, Ordering::Release);
    }
}

/// Switch the visible terminal and redraw its saved text contents.
pub fn switch_terminal(terminal: usize) -> bool {
    if terminal >= TERMINAL_COUNT {
        return false;
    }
    ACTIVE_TERMINAL.store(terminal, Ordering::Release);
    OUTPUT_TERMINAL.store(terminal, Ordering::Release);
    REDRAW_PENDING.store(true, Ordering::Release);
    true
}

/// Redraw a pending terminal switch outside interrupt context.
pub fn redraw_pending() {
    if REDRAW_PENDING.swap(false, Ordering::AcqRel)
        && let Some(state) = TERMINALS.get(active_terminal())
    {
        redraw(state.lock());
    }
}

/// Write text to the default kernel output terminal.
pub fn terminal_print(text: &str) {
    terminal_print_to(OUTPUT_TERMINAL.load(Ordering::Acquire), text);
}

/// Show formatter progress on one terminal row without flooding the log.
pub fn terminal_show_progress(label: &str, written_bytes: u64, total_bytes: u64) {
    let percent = if total_bytes == 0 {
        100
    } else {
        written_bytes
            .saturating_mul(100)
            .checked_div(total_bytes)
            .unwrap_or(100)
            .min(100)
    };
    let bar_width = 20usize;
    let filled = (percent as usize * bar_width) / 100;
    let bar = alloc::format!(
        "{}{}",
        "#".repeat(filled),
        ".".repeat(bar_width - filled)
    );
    let written_mib = written_bytes / (1024 * 1024);
    let total_mib = total_bytes.div_ceil(1024 * 1024);
    terminal_replace_current_line(&alloc::format!(
        "{label} [{bar}] {written_mib}/{total_mib} MiB ({percent}%)"
    ));
}

/// Replace the current output row in-place using ASCII text.
/// The terminal cursor remains at column zero so a following newline advances
/// to the row below the progress display.
pub fn terminal_replace_current_line(text: &str) {
    let terminal = OUTPUT_TERMINAL.load(Ordering::Acquire);
    let Some(state) = TERMINALS.get(terminal) else {
        return;
    };
    let mut state = state.lock();
    let framebuffer = Framebuffer::get();
    let (columns, rows) = state.dimensions(framebuffer.as_ref());
    if state.row >= rows {
        return;
    }

    let row = state.row;
    let bytes = text.as_bytes();
    for column in 0..columns {
        let byte = bytes
            .get(column)
            .copied()
            .filter(|byte| byte.is_ascii())
            .unwrap_or(b' ');
        state.cells[row][column] = byte as u16;
        if terminal == active_terminal()
            && let Some(framebuffer) = framebuffer.as_ref()
        {
            draw_character(framebuffer, column, row, byte);
        }
    }
    state.column = 0;
}

/// Write text to a specific virtual terminal.
pub fn terminal_print_to(terminal: usize, text: &str) {
    let Some(state) = TERMINALS.get(terminal) else {
        return;
    };
    let mut state = state.lock();
    let framebuffer = Framebuffer::get();
    let (columns, rows) = state.dimensions(framebuffer.as_ref());

    for character in text.chars() {
        if character == '\n' || character == '\r' {
            state.column = 0;
            state.row += 1;
            if state.row >= rows {
                state.scroll(rows);
                if terminal == active_terminal()
                    && let Some(framebuffer) = framebuffer.as_ref()
                {
                    scroll_framebuffer(framebuffer, rows);
                }
            }
            continue;
        }

        if state.column >= columns {
            state.column = 0;
            state.row += 1;
            if state.row >= rows {
                state.scroll(rows);
                if terminal == active_terminal()
                    && let Some(framebuffer) = framebuffer.as_ref()
                {
                    scroll_framebuffer(framebuffer, rows);
                }
            }
        }

        let glyph = if (character as u32) < FONT8X16.len() as u32 {
            character as u16
        } else {
            u16::MAX
        };
        let row = state.row;
        let column = state.column;
        state.cells[row][column] = glyph;
        if terminal == active_terminal()
            && let Some(framebuffer) = framebuffer.as_ref()
        {
            draw_character(
                framebuffer,
                column,
                row,
                u8::try_from(glyph).unwrap_or(b' '),
            );
        }
        state.column += 1;
    }
}

/// Reset one terminal and clear its visible screen when selected.
pub fn terminal_clear_for(terminal: usize) {
    let Some(state) = TERMINALS.get(terminal) else {
        return;
    };
    let mut state = state.lock();
    *state = TerminalState::new();
    if terminal == active_terminal()
        && let Some(framebuffer) = Framebuffer::get()
    {
        framebuffer.clear();
    }
}

/// Draw one character cell without changing the terminal's output cursor.
pub fn terminal_put_char_at(terminal: usize, row: usize, column: usize, character: char) {
    terminal_put_char_at_with_style(terminal, row, column, character, false);
}

/// Draw one highlighted character cell without changing the terminal's output cursor.
pub fn terminal_put_char_at_highlighted(
    terminal: usize,
    row: usize,
    column: usize,
    character: char,
) {
    terminal_put_char_at_with_style(terminal, row, column, character, true);
}

fn terminal_put_char_at_with_style(
    terminal: usize,
    row: usize,
    column: usize,
    character: char,
    highlighted: bool,
) {
    let Some(state) = TERMINALS.get(terminal) else {
        return;
    };
    let mut state = state.lock();
    let framebuffer = Framebuffer::get();
    let (columns, rows) = state.dimensions(framebuffer.as_ref());
    if row >= rows || column >= columns {
        return;
    }

    let glyph = if (character as u32) < FONT8X16.len() as u32 {
        character as u16
    } else {
        u16::MAX
    };
    state.cells[row][column] = glyph;
    if terminal == active_terminal()
        && let Some(framebuffer) = framebuffer.as_ref()
    {
        let character = u8::try_from(glyph).unwrap_or(b' ');
        if highlighted {
            draw_character_with_colors(
                framebuffer,
                column,
                row,
                character,
                Color::BLACK,
                Color::WHITE,
            );
        } else {
            draw_character(framebuffer, column, row, character);
        }
    }
}

/// Clear the default kernel output terminal.
pub fn terminal_clear() {
    terminal_clear_for(OUTPUT_TERMINAL.load(Ordering::Acquire));
}

/// Erase the character immediately before the cursor on the default terminal.
pub fn terminal_backspace() {
    terminal_backspace_for(OUTPUT_TERMINAL.load(Ordering::Acquire));
}

/// Erase the character immediately before the cursor on a specific terminal.
pub fn terminal_backspace_for(terminal: usize) {
    let Some(state) = TERMINALS.get(terminal) else {
        return;
    };
    let mut state = state.lock();
    let framebuffer = Framebuffer::get();
    let (columns, _) = state.dimensions(framebuffer.as_ref());
    if state.column != 0 {
        state.column -= 1;
    } else if state.row != 0 {
        state.row -= 1;
        state.column = columns - 1;
    } else {
        return;
    }
    let row = state.row;
    let column = state.column;
    state.cells[row][column] = b' ' as u16;
    if terminal == active_terminal()
        && let Some(framebuffer) = framebuffer.as_ref()
    {
        draw_character(framebuffer, column, row, b' ');
    }
}

impl TerminalState {
    fn scroll(&mut self, rows: usize) {
        self.cells.copy_within(1..rows, 0);
        self.cells[rows - 1].fill(b' ' as u16);
        self.row = rows - 1;
    }
}

fn redraw(state: spin::mutex::MutexGuard<'_, TerminalState>) {
    if let Some(framebuffer) = Framebuffer::get() {
        redraw_with_framebuffer(&state, &framebuffer);
    }
}

fn redraw_with_framebuffer(state: &TerminalState, framebuffer: &Framebuffer) {
    framebuffer.clear();
    let (columns, rows) = state.dimensions(Some(framebuffer));
    for row in 0..rows {
        for column in 0..columns {
            let glyph = state.cells[row][column];
            if glyph != b' ' as u16
                && let Ok(character) = u8::try_from(glyph)
            {
                draw_character(framebuffer, column, row, character);
            }
        }
    }
}

fn scroll_framebuffer(framebuffer: &Framebuffer, rows: usize) {
    let pitch = framebuffer.pitch() as usize;
    let height = rows * CHAR_H as usize;
    let length = pitch * height;
    // Safety: the framebuffer mapping covers pitch * framebuffer.height() bytes,
    // and the terminal viewport is no taller than that mapping.
    let pixels = unsafe { core::slice::from_raw_parts_mut(framebuffer.addr(), length) };
    scroll_pixel_rows(pixels, pitch, height, CHAR_H as usize);
}

fn scroll_pixel_rows(pixels: &mut [u8], pitch: usize, height: usize, line_height: usize) {
    let scroll_bytes = pitch * line_height;
    let total_bytes = pitch * height;
    pixels.copy_within(scroll_bytes..total_bytes, 0);
    pixels[total_bytes - scroll_bytes..total_bytes].fill(0);
}

/// Build one fixed diagnostic line without `core::fmt` or an allocator.
struct CrashLine {
    bytes: [u8; 79],
    length: usize,
}

impl CrashLine {
    fn new() -> Self {
        Self {
            bytes: [b' '; 79],
            length: 0,
        }
    }

    fn text(&mut self, text: &str) {
        for byte in text.bytes() {
            self.push(byte);
        }
    }

    fn hex(&mut self, value: u64) {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        self.text("0x");
        for shift in (0..16).rev() {
            self.push(DIGITS[((value >> (shift * 4)) & 0x0f) as usize]);
        }
    }

    fn push(&mut self, byte: u8) {
        if self.length < self.bytes.len() {
            self.bytes[self.length] = byte;
            self.length += 1;
        }
    }
}

/// Draw a fatal CPU-exception record straight into the framebuffer.
///
/// The fatal handler cannot use the normal console: the fault may have happened
/// while this module held the terminal lock, while it was scrolling, or inside
/// `core::fmt`, and repeating that work would only fault again. This path takes
/// no lock, does not touch the output cursor, does not scroll, does not
/// allocate and does not format, so it either draws the record or returns.
///
/// It also gives hardware without a serial port the only crash record it can
/// show: the kernel reports the same values to COM1, which a laptop cannot read.
pub fn show_kernel_exception(vector: u64, error_code: u64, rip: u64, address: Option<u64>) {
    let Some(framebuffer) = Framebuffer::get() else {
        return;
    };
    let rows = ((framebuffer.height() / CHAR_H) as usize).clamp(1, MAX_ROWS);
    let mut line = CrashLine::new();

    line.text("KERNEL EXCEPTION vector=");
    line.hex(vector);
    line.text(" error=");
    line.hex(error_code);
    draw_crash_line(&framebuffer, rows.saturating_sub(2), &line);

    let mut line = CrashLine::new();
    line.text("rip=");
    line.hex(rip);
    if let Some(address) = address {
        line.text(" cr2=");
        line.hex(address);
    }
    draw_crash_line(&framebuffer, rows - 1, &line);
}

fn draw_crash_line(framebuffer: &Framebuffer, row: usize, line: &CrashLine) {
    let columns = ((framebuffer.width() / CHAR_W) as usize).clamp(1, MAX_COLUMNS);
    // Clear the whole row first: leftover terminal text would otherwise sit
    // next to the record and hide where the value ends.
    for column in 0..columns {
        draw_character_with_colors(framebuffer, column, row, b' ', Color::WHITE, Color::BLACK);
    }
    for (column, byte) in line.bytes[..line.length.min(columns)]
        .iter()
        .copied()
        .enumerate()
    {
        draw_character_with_colors(framebuffer, column, row, byte, Color::WHITE, Color::BLACK);
    }
}

fn draw_character(framebuffer: &Framebuffer, column: usize, row: usize, character: u8) {
    draw_character_with_colors(
        framebuffer,
        column,
        row,
        character,
        Color::WHITE,
        Color::BLACK,
    );
}

fn draw_character_with_colors(
    framebuffer: &Framebuffer,
    column: usize,
    row: usize,
    character: u8,
    foreground: Color,
    background: Color,
) {
    let x = column as u64 * CHAR_W;
    let y = row as u64 * CHAR_H;
    framebuffer.fill_rect(x, y, x + CHAR_W, y + CHAR_H, background);
    let glyph = &FONT8X16[character as usize];
    for (row_offset, bits) in glyph.iter().enumerate() {
        for column_offset in 0..CHAR_W as usize {
            if bits & (0x80 >> column_offset) != 0 {
                framebuffer.write_pixel(
                    x + column_offset as u64,
                    y + row_offset as u64,
                    foreground,
                );
            }
        }
    }
}

/// Stateless formatting adapter used by the kernel `print!` macros.
pub struct Terminal;

impl Write for Terminal {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        terminal_print(text);
        crate::drivers::serial::write_str(text);
        crate::klog::record(text);
        Ok(())
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        let mut terminal = $crate::terminal::Terminal;
        terminal.write_fmt(format_args!($($arg)*)).unwrap();
    }};
}

#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        let mut terminal = $crate::terminal::Terminal;
        terminal.write_fmt(format_args!($($arg)*)).unwrap();
        terminal.write_fmt(format_args!("\n")).unwrap();
    }};
}

#[cfg(test)]
mod tests {
    use super::{TERMINALS, scroll_pixel_rows, terminal_put_char_at_highlighted};

    #[test]
    fn scrolls_pixel_rows_and_clears_only_the_viewport_bottom() {
        let mut pixels = [1, 1, 2, 2, 3, 3, 9, 9];
        scroll_pixel_rows(&mut pixels, 2, 3, 1);
        assert_eq!(pixels, [2, 2, 3, 3, 0, 0, 9, 9]);
    }

    #[test]
    fn highlighted_cell_keeps_its_original_character() {
        terminal_put_char_at_highlighted(0, 0, 0, '>');
        assert_eq!(TERMINALS[0].lock().cells[0][0], b'>' as u16);
    }
}
