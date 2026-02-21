//! std:terminal — Terminal control (TTY detection, raw mode, cursor, colors, input)

use raya_sdk::{IoCompletion, IoRequest, NativeCallResult, NativeContext, NativeValue};
use std::io::{self, Read, Write};
use std::sync::Mutex;

/// Saved original termios state for raw mode restore.
static ORIGINAL_TERMIOS: Mutex<Option<libc::termios>> = Mutex::new(None);

// ── TTY Detection ──

/// Check if stdout is a terminal
pub fn is_terminal(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    // SAFETY: isatty is safe for any fd
    let result = unsafe { libc::isatty(1) } != 0;
    NativeCallResult::bool(result)
}

/// Check if stdin is a terminal
pub fn is_terminal_stdin(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let result = unsafe { libc::isatty(0) } != 0;
    NativeCallResult::bool(result)
}

/// Check if stderr is a terminal
pub fn is_terminal_stderr(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let result = unsafe { libc::isatty(2) } != 0;
    NativeCallResult::bool(result)
}

// ── Terminal Size ──

/// Get terminal width (columns)
pub fn columns(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    match get_winsize() {
        Some(ws) => NativeCallResult::f64(ws.ws_col as f64),
        None => NativeCallResult::f64(80.0), // sensible default
    }
}

/// Get terminal height (rows)
pub fn rows(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    match get_winsize() {
        Some(ws) => NativeCallResult::f64(ws.ws_row as f64),
        None => NativeCallResult::f64(24.0), // sensible default
    }
}

/// Helper: get terminal window size via ioctl
fn get_winsize() -> Option<libc::winsize> {
    // SAFETY: ioctl with TIOCGWINSZ is safe on a valid fd (stdout=1)
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    let ret = unsafe { libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) };
    if ret == 0 && ws.ws_col > 0 && ws.ws_row > 0 {
        Some(ws)
    } else {
        None
    }
}

// ── Raw Mode ──

/// Enable raw mode on stdin (save original termios, set raw flags)
pub fn enable_raw_mode(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    // SAFETY: tcgetattr/tcsetattr are safe with valid fd
    let mut original: libc::termios = unsafe { std::mem::zeroed() };
    if unsafe { libc::tcgetattr(0, &mut original) } != 0 {
        return NativeCallResult::Error("terminal.enableRawMode: tcgetattr failed".into());
    }

    // Save original state
    {
        let mut guard = ORIGINAL_TERMIOS.lock().unwrap();
        *guard = Some(original);
    }

    // Create raw mode termios
    let mut raw = original;
    // Input: disable BRKINT, ICRNL, INPCK, ISTRIP, IXON
    raw.c_iflag &= !(libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON);
    // Output: disable OPOST (output processing)
    raw.c_oflag &= !libc::OPOST;
    // Control: set CS8 (8-bit chars)
    raw.c_cflag |= libc::CS8;
    // Local: disable ECHO, ICANON, IEXTEN, ISIG
    raw.c_lflag &= !(libc::ECHO | libc::ICANON | libc::IEXTEN | libc::ISIG);
    // Read returns after 1 byte, no timeout
    raw.c_cc[libc::VMIN] = 1;
    raw.c_cc[libc::VTIME] = 0;

    if unsafe { libc::tcsetattr(0, libc::TCSAFLUSH, &raw) } != 0 {
        return NativeCallResult::Error("terminal.enableRawMode: tcsetattr failed".into());
    }

    NativeCallResult::null()
}

/// Disable raw mode on stdin (restore saved termios)
pub fn disable_raw_mode(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let guard = ORIGINAL_TERMIOS.lock().unwrap();
    if let Some(ref original) = *guard {
        // SAFETY: tcsetattr is safe with a valid termios struct
        if unsafe { libc::tcsetattr(0, libc::TCSAFLUSH, original) } != 0 {
            return NativeCallResult::Error("terminal.disableRawMode: tcsetattr failed".into());
        }
    }
    NativeCallResult::null()
}

// ── Cursor Control ──

/// Move cursor to absolute position (0-based col, row)
pub fn move_to(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let col = args[0].as_f64().unwrap_or(0.0) as u32 + 1; // ANSI is 1-based
    let row = args[1].as_f64().unwrap_or(0.0) as u32 + 1;
    write_ansi(&format!("\x1b[{};{}H", row, col))
}

/// Move cursor up by n lines
pub fn move_up(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let n = args[0].as_f64().unwrap_or(0.0) as u32;
    write_ansi(&format!("\x1b[{}A", n))
}

/// Move cursor down by n lines
pub fn move_down(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let n = args[0].as_f64().unwrap_or(0.0) as u32;
    write_ansi(&format!("\x1b[{}B", n))
}

/// Move cursor right by n columns
pub fn move_right(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let n = args[0].as_f64().unwrap_or(0.0) as u32;
    write_ansi(&format!("\x1b[{}C", n))
}

/// Move cursor left by n columns
pub fn move_left(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let n = args[0].as_f64().unwrap_or(0.0) as u32;
    write_ansi(&format!("\x1b[{}D", n))
}

/// Save cursor position
pub fn save_cursor(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    write_ansi("\x1b[s")
}

/// Restore cursor position
pub fn restore_cursor(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    write_ansi("\x1b[u")
}

/// Hide cursor
pub fn hide_cursor(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    write_ansi("\x1b[?25l")
}

/// Show cursor
pub fn show_cursor(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    write_ansi("\x1b[?25h")
}

// ── Screen Control ──

/// Clear entire screen and move cursor to home
pub fn clear_screen(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    write_ansi("\x1b[2J\x1b[H")
}

/// Clear entire current line
pub fn clear_line(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    write_ansi("\x1b[2K")
}

/// Clear from cursor to end of line
pub fn clear_to_end_of_line(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    write_ansi("\x1b[K")
}

/// Clear from cursor to end of screen
pub fn clear_to_end_of_screen(
    _ctx: &dyn NativeContext,
    _args: &[NativeValue],
) -> NativeCallResult {
    write_ansi("\x1b[J")
}

/// Helper: write an ANSI escape sequence to stdout and flush
fn write_ansi(seq: &str) -> NativeCallResult {
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    match lock.write_all(seq.as_bytes()).and_then(|_| lock.flush()) {
        Ok(_) => NativeCallResult::null(),
        Err(e) => NativeCallResult::Error(format!("terminal: write failed: {}", e)),
    }
}

// ── Input ──

/// Read a single key/escape sequence from stdin (blocking)
pub fn read_key(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(|| {
            let stdin = io::stdin();
            let mut handle = stdin.lock();
            let mut buf = [0u8; 1];

            match handle.read_exact(&mut buf) {
                Ok(_) => {}
                Err(e) => return IoCompletion::Error(format!("terminal.readKey: {}", e)),
            }

            let byte = buf[0];

            // Ctrl+letter (bytes 1-26, except common ones)
            if byte == 0 {
                return IoCompletion::String("Ctrl+@".into());
            }
            if byte == b'\r' || byte == 10 {
                return IoCompletion::String("Enter".into());
            }
            if byte == b'\t' {
                return IoCompletion::String("Tab".into());
            }
            if byte == 0x7f {
                return IoCompletion::String("Backspace".into());
            }
            if byte == 8 {
                return IoCompletion::String("Backspace".into());
            }
            if (1..=26).contains(&byte) {
                let letter = (b'A' + byte - 1) as char;
                return IoCompletion::String(format!("Ctrl+{}", letter));
            }

            // Escape sequences
            if byte == 0x1b {
                // Try to read more bytes (with a short timeout approach:
                // set VMIN=0, VTIME=1 to read with 100ms timeout)
                let mut seq = [0u8; 2];

                // Temporarily set stdin to non-blocking-ish read for sequence detection
                let mut termios: libc::termios = unsafe { std::mem::zeroed() };
                let got_termios = unsafe { libc::tcgetattr(0, &mut termios) } == 0;
                let old_vmin;
                let old_vtime;

                if got_termios {
                    old_vmin = termios.c_cc[libc::VMIN];
                    old_vtime = termios.c_cc[libc::VTIME];
                    termios.c_cc[libc::VMIN] = 0;
                    termios.c_cc[libc::VTIME] = 1; // 100ms timeout
                    unsafe { libc::tcsetattr(0, libc::TCSANOW, &termios) };
                } else {
                    old_vmin = 1;
                    old_vtime = 0;
                }

                let n: usize = handle.read(&mut seq).unwrap_or_default();

                // Restore original VMIN/VTIME
                if got_termios {
                    termios.c_cc[libc::VMIN] = old_vmin;
                    termios.c_cc[libc::VTIME] = old_vtime;
                    unsafe { libc::tcsetattr(0, libc::TCSANOW, &termios) };
                }

                if n == 0 {
                    return IoCompletion::String("Escape".into());
                }

                if n == 1 && seq[0] == b'[' {
                    // CSI sequence: read the final byte
                    let mut final_byte = [0u8; 1];

                    if got_termios {
                        termios.c_cc[libc::VMIN] = 0;
                        termios.c_cc[libc::VTIME] = 1;
                        unsafe { libc::tcsetattr(0, libc::TCSANOW, &termios) };
                    }

                    let n2: usize = handle.read(&mut final_byte).unwrap_or_default();

                    if got_termios {
                        termios.c_cc[libc::VMIN] = old_vmin;
                        termios.c_cc[libc::VTIME] = old_vtime;
                        unsafe { libc::tcsetattr(0, libc::TCSANOW, &termios) };
                    }

                    if n2 == 1 {
                        return IoCompletion::String(
                            match final_byte[0] {
                                b'A' => "ArrowUp",
                                b'B' => "ArrowDown",
                                b'C' => "ArrowRight",
                                b'D' => "ArrowLeft",
                                b'H' => "Home",
                                b'F' => "End",
                                b'Z' => "Shift+Tab",
                                _ => {
                                    // Extended sequences like \x1b[3~ (Delete), \x1b[5~ (PageUp), etc.
                                    if final_byte[0] >= b'0' && final_byte[0] <= b'9' {
                                        // Read the tilde or additional chars
                                        let mut tilde = [0u8; 1];

                                        if got_termios {
                                            termios.c_cc[libc::VMIN] = 0;
                                            termios.c_cc[libc::VTIME] = 1;
                                            unsafe {
                                                libc::tcsetattr(0, libc::TCSANOW, &termios)
                                            };
                                        }

                                        let n3: usize = handle.read(&mut tilde).unwrap_or_default();

                                        if got_termios {
                                            termios.c_cc[libc::VMIN] = old_vmin;
                                            termios.c_cc[libc::VTIME] = old_vtime;
                                            unsafe {
                                                libc::tcsetattr(0, libc::TCSANOW, &termios)
                                            };
                                        }

                                        if n3 == 1 && tilde[0] == b'~' {
                                            return IoCompletion::String(
                                                match final_byte[0] {
                                                    b'1' => "Home",
                                                    b'2' => "Insert",
                                                    b'3' => "Delete",
                                                    b'4' => "End",
                                                    b'5' => "PageUp",
                                                    b'6' => "PageDown",
                                                    b'7' => "Home",
                                                    b'8' => "End",
                                                    _ => "Unknown",
                                                }
                                                .into(),
                                            );
                                        }
                                    }
                                    return IoCompletion::String("Unknown".into());
                                }
                            }
                            .into(),
                        );
                    }

                    return IoCompletion::String("Escape+[".into());
                }

                if n == 1 && seq[0] == b'O' {
                    // SS3 sequences (e.g., \x1bOH = Home, \x1bOF = End)
                    let mut final_byte = [0u8; 1];

                    if got_termios {
                        termios.c_cc[libc::VMIN] = 0;
                        termios.c_cc[libc::VTIME] = 1;
                        unsafe { libc::tcsetattr(0, libc::TCSANOW, &termios) };
                    }

                    let n2: usize = handle.read(&mut final_byte).unwrap_or_default();

                    if got_termios {
                        termios.c_cc[libc::VMIN] = old_vmin;
                        termios.c_cc[libc::VTIME] = old_vtime;
                        unsafe { libc::tcsetattr(0, libc::TCSANOW, &termios) };
                    }

                    if n2 == 1 {
                        return IoCompletion::String(
                            match final_byte[0] {
                                b'H' => "Home",
                                b'F' => "End",
                                b'P' => "F1",
                                b'Q' => "F2",
                                b'R' => "F3",
                                b'S' => "F4",
                                _ => "Unknown",
                            }
                            .into(),
                        );
                    }
                }

                // Alt+key
                if n >= 1 && seq[0] >= 0x20 {
                    return IoCompletion::String(format!("Alt+{}", seq[0] as char));
                }

                return IoCompletion::String("Escape".into());
            }

            // Regular printable character (or UTF-8 lead byte)
            if byte >= 0x80 {
                // UTF-8 multi-byte: read continuation bytes
                let extra = if byte < 0xE0 {
                    1
                } else if byte < 0xF0 {
                    2
                } else {
                    3
                };
                let mut utf8_buf = vec![byte];
                let mut cont = vec![0u8; extra];
                if handle.read_exact(&mut cont).is_ok() {
                    utf8_buf.extend_from_slice(&cont);
                }
                let s = String::from_utf8(utf8_buf).unwrap_or_else(|_| "?".into());
                return IoCompletion::String(s);
            }

            // ASCII printable
            IoCompletion::String(String::from(byte as char))
        }),
    })
}
