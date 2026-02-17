//! std:io — Standard I/O (stdin/stdout/stderr)

use raya_engine::vm::{NativeCallResult, NativeContext, NativeValue, string_read, string_allocate};
use std::io::{self, BufRead, Read, Write};

/// Read a line from stdin (blocking)
pub fn read_line(ctx: &NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let stdin = io::stdin();
    let mut line = String::new();
    match stdin.lock().read_line(&mut line) {
        Ok(_) => {
            // Remove trailing newline
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }
            NativeCallResult::Value(string_allocate(ctx, line))
        }
        Err(e) => NativeCallResult::Error(format!("io.readLine: {}", e)),
    }
}

/// Read all of stdin (blocking until EOF)
pub fn read_all(ctx: &NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let mut buf = String::new();
    match io::stdin().lock().read_to_string(&mut buf) {
        Ok(_) => NativeCallResult::Value(string_allocate(ctx, buf)),
        Err(e) => NativeCallResult::Error(format!("io.readAll: {}", e)),
    }
}

/// Write string to stdout
pub fn write(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let data = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("io.write: {}", e)),
    };
    match io::stdout().write_all(data.as_bytes()) {
        Ok(_) => NativeCallResult::null(),
        Err(e) => NativeCallResult::Error(format!("io.write: {}", e)),
    }
}

/// Write string + newline to stdout
pub fn writeln(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let data = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("io.writeln: {}", e)),
    };
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    match lock.write_all(data.as_bytes()).and_then(|_| lock.write_all(b"\n")) {
        Ok(_) => NativeCallResult::null(),
        Err(e) => NativeCallResult::Error(format!("io.writeln: {}", e)),
    }
}

/// Write string to stderr
pub fn write_err(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let data = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("io.writeErr: {}", e)),
    };
    match io::stderr().write_all(data.as_bytes()) {
        Ok(_) => NativeCallResult::null(),
        Err(e) => NativeCallResult::Error(format!("io.writeErr: {}", e)),
    }
}

/// Write string + newline to stderr
pub fn write_errln(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let data = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("io.writeErrln: {}", e)),
    };
    let stderr = io::stderr();
    let mut lock = stderr.lock();
    match lock.write_all(data.as_bytes()).and_then(|_| lock.write_all(b"\n")) {
        Ok(_) => NativeCallResult::null(),
        Err(e) => NativeCallResult::Error(format!("io.writeErrln: {}", e)),
    }
}

/// Flush stdout
pub fn flush(_ctx: &NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    match io::stdout().flush() {
        Ok(_) => NativeCallResult::null(),
        Err(e) => NativeCallResult::Error(format!("io.flush: {}", e)),
    }
}
