//! std:readline — Interactive line input with history, prompting, and password input

use crate::handles::HandleRegistry;
use raya_sdk::{IoCompletion, IoRequest, NativeCallResult, NativeContext, NativeValue};
use std::io::{self, BufRead, Write};
use std::sync::{LazyLock, Mutex};

/// Internal readline state held behind a handle.
struct ReadlineHandle {
    history: Vec<String>,
}

/// Global handle registry for readline instances.
static READLINE_HANDLES: LazyLock<HandleRegistry<Mutex<ReadlineHandle>>> =
    LazyLock::new(HandleRegistry::new);

/// Helper: extract a handle ID from the first argument.
fn extract_handle(args: &[NativeValue]) -> u64 {
    args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64
}

// ── readline.new() -> handle ──

/// Create a new readline instance with empty history.
pub fn readline_new(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let handle = ReadlineHandle {
        history: Vec::new(),
    };
    let id = READLINE_HANDLES.insert(Mutex::new(handle));
    NativeCallResult::f64(id as f64)
}

// ── readline.prompt(handle, text) -> string ──

/// Show prompt text, read a line from stdin (blocking).
pub fn readline_prompt(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let _handle = extract_handle(args);
    let prompt_text = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("readline.prompt: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            print!("{}", prompt_text);
            io::stdout().flush().ok();
            let mut line = String::new();
            match io::stdin().lock().read_line(&mut line) {
                Ok(0) => IoCompletion::String(String::new()), // EOF
                Ok(_) => {
                    if line.ends_with('\n') {
                        line.pop();
                    }
                    if line.ends_with('\r') {
                        line.pop();
                    }
                    IoCompletion::String(line)
                }
                Err(e) => IoCompletion::Error(format!("readline.prompt: {}", e)),
            }
        }),
    })
}

// ── readline.addHistory(handle, line) -> void ──

/// Add a line to the readline instance's history.
pub fn readline_add_history(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = extract_handle(args);
    let line = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("readline.addHistory: {}", e)),
    };
    match READLINE_HANDLES.get(handle) {
        Some(entry) => {
            entry.lock().unwrap().history.push(line);
            NativeCallResult::null()
        }
        None => NativeCallResult::Error(format!("readline.addHistory: invalid handle {}", handle)),
    }
}

// ── readline.loadHistory(handle, path) -> void ──

/// Load history from a file (one line per entry). Blocking.
pub fn readline_load_history(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = extract_handle(args);
    let path = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("readline.loadHistory: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || match READLINE_HANDLES.get(handle) {
            Some(entry) => match std::fs::read_to_string(&path) {
                Ok(content) => {
                    let mut rl = entry.lock().unwrap();
                    for line in content.lines() {
                        rl.history.push(line.to_string());
                    }
                    IoCompletion::Primitive(NativeValue::null())
                }
                Err(e) => IoCompletion::Error(format!("readline.loadHistory: {}", e)),
            },
            None => IoCompletion::Error(format!(
                "readline.loadHistory: invalid handle {}",
                handle
            )),
        }),
    })
}

// ── readline.saveHistory(handle, path) -> void ──

/// Save history to a file (one line per entry). Blocking.
pub fn readline_save_history(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = extract_handle(args);
    let path = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("readline.saveHistory: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || match READLINE_HANDLES.get(handle) {
            Some(entry) => {
                let rl = entry.lock().unwrap();
                let content = rl.history.join("\n");
                match std::fs::write(&path, content) {
                    Ok(_) => IoCompletion::Primitive(NativeValue::null()),
                    Err(e) => IoCompletion::Error(format!("readline.saveHistory: {}", e)),
                }
            }
            None => IoCompletion::Error(format!(
                "readline.saveHistory: invalid handle {}",
                handle
            )),
        }),
    })
}

// ── readline.clearHistory(handle) -> void ──

/// Clear all history entries from the readline instance.
pub fn readline_clear_history(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = extract_handle(args);
    match READLINE_HANDLES.get(handle) {
        Some(entry) => {
            entry.lock().unwrap().history.clear();
            NativeCallResult::null()
        }
        None => {
            NativeCallResult::Error(format!("readline.clearHistory: invalid handle {}", handle))
        }
    }
}

// ── readline.historySize(handle) -> number ──

/// Get the number of history entries.
pub fn readline_history_size(
    _ctx: &dyn NativeContext,
    args: &[NativeValue],
) -> NativeCallResult {
    let handle = extract_handle(args);
    match READLINE_HANDLES.get(handle) {
        Some(entry) => {
            let len = entry.lock().unwrap().history.len();
            NativeCallResult::f64(len as f64)
        }
        None => {
            NativeCallResult::Error(format!("readline.historySize: invalid handle {}", handle))
        }
    }
}

// ── readline.close(handle) -> void ──

/// Close a readline instance, removing it from the handle registry.
pub fn readline_close(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = extract_handle(args);
    READLINE_HANDLES.remove(handle);
    NativeCallResult::null()
}

// ── readline.simplePrompt(text) -> string ──

/// One-shot prompt: show text, read line. No handle needed.
pub fn readline_simple_prompt(
    ctx: &dyn NativeContext,
    args: &[NativeValue],
) -> NativeCallResult {
    let prompt_text = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("readline.simplePrompt: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            print!("{}", prompt_text);
            io::stdout().flush().ok();
            let mut line = String::new();
            match io::stdin().lock().read_line(&mut line) {
                Ok(0) => IoCompletion::String(String::new()),
                Ok(_) => {
                    if line.ends_with('\n') {
                        line.pop();
                    }
                    if line.ends_with('\r') {
                        line.pop();
                    }
                    IoCompletion::String(line)
                }
                Err(e) => IoCompletion::Error(format!("readline.simplePrompt: {}", e)),
            }
        }),
    })
}

// ── readline.confirm(text) -> boolean ──

/// Prompt with " (y/n) " appended, return true for y/yes.
pub fn readline_confirm(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let prompt_text = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("readline.confirm: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            print!("{} (y/n) ", prompt_text);
            io::stdout().flush().ok();
            let mut line = String::new();
            io::stdin().lock().read_line(&mut line).ok();
            let trimmed = line.trim().to_lowercase();
            let confirmed = trimmed == "y" || trimmed == "yes";
            IoCompletion::Primitive(NativeValue::bool(confirmed))
        }),
    })
}

// ── readline.password(text) -> string ──

/// Prompt for input with echo disabled (raw mode via termios). Blocking.
pub fn readline_password(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let prompt_text = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("readline.password: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            print!("{}", prompt_text);
            io::stdout().flush().ok();

            // SAFETY: tcgetattr/tcsetattr are safe with valid fd (stdin=0)
            unsafe {
                let mut termios: libc::termios = std::mem::zeroed();
                if libc::tcgetattr(0, &mut termios) != 0 {
                    // Not a terminal — fall back to normal read
                    let mut line = String::new();
                    return match io::stdin().lock().read_line(&mut line) {
                        Ok(_) => {
                            if line.ends_with('\n') {
                                line.pop();
                            }
                            if line.ends_with('\r') {
                                line.pop();
                            }
                            IoCompletion::String(line)
                        }
                        Err(e) => IoCompletion::Error(format!("readline.password: {}", e)),
                    };
                }

                let original = termios;
                termios.c_lflag &= !(libc::ECHO);
                libc::tcsetattr(0, libc::TCSANOW, &termios);

                let mut line = String::new();
                let result = io::stdin().lock().read_line(&mut line);

                // Restore echo
                libc::tcsetattr(0, libc::TCSANOW, &original);
                println!(); // newline after hidden input

                match result {
                    Ok(_) => {
                        if line.ends_with('\n') {
                            line.pop();
                        }
                        if line.ends_with('\r') {
                            line.pop();
                        }
                        IoCompletion::String(line)
                    }
                    Err(e) => IoCompletion::Error(format!("readline.password: {}", e)),
                }
            }
        }),
    })
}

// ── readline.select(text, options[]) -> number ──

/// Display numbered list of options, prompt for choice, return 0-based index.
/// Returns -1 if the choice is invalid.
pub fn readline_select(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let prompt_text = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("readline.select: {}", e)),
    };

    // Read options array
    let arr = args[1];
    let len = match ctx.array_len(arr) {
        Ok(n) => n,
        Err(e) => return NativeCallResult::Error(format!("readline.select: {}", e)),
    };
    let mut options = Vec::with_capacity(len);
    for i in 0..len {
        let elem = match ctx.array_get(arr, i) {
            Ok(v) => v,
            Err(e) => return NativeCallResult::Error(format!("readline.select: {}", e)),
        };
        let s = match ctx.read_string(elem) {
            Ok(s) => s,
            Err(e) => return NativeCallResult::Error(format!("readline.select: {}", e)),
        };
        options.push(s);
    }

    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            println!("{}", prompt_text);
            for (i, opt) in options.iter().enumerate() {
                println!("  {}. {}", i + 1, opt);
            }
            print!("Choice: ");
            io::stdout().flush().ok();
            let mut line = String::new();
            io::stdin().lock().read_line(&mut line).ok();
            match line.trim().parse::<usize>() {
                Ok(n) if n >= 1 && n <= options.len() => {
                    IoCompletion::Primitive(NativeValue::f64((n - 1) as f64))
                }
                _ => IoCompletion::Primitive(NativeValue::f64(-1.0)),
            }
        }),
    })
}
