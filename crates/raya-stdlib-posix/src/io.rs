//! std:io — Standard I/O (stdin/stdout/stderr)

use raya_sdk::{NativeCallResult, NativeContext, NativeValue, IoRequest, IoCompletion};
use std::io::{self, BufRead, Read, Write};

/// Read a line from stdin (blocking → IO pool)
pub fn read_line(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(|| {
            let stdin = io::stdin();
            let mut line = String::new();
            match stdin.lock().read_line(&mut line) {
                Ok(_) => {
                    if line.ends_with('\n') {
                        line.pop();
                        if line.ends_with('\r') {
                            line.pop();
                        }
                    }
                    IoCompletion::String(line)
                }
                Err(e) => IoCompletion::Error(format!("io.readLine: {}", e)),
            }
        }),
    })
}

/// Read all of stdin (blocking → IO pool)
pub fn read_all(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(|| {
            let mut buf = String::new();
            match io::stdin().lock().read_to_string(&mut buf) {
                Ok(_) => IoCompletion::String(buf),
                Err(e) => IoCompletion::Error(format!("io.readAll: {}", e)),
            }
        }),
    })
}

/// Write string to stdout
pub fn write(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let data = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("io.write: {}", e)),
    };
    match io::stdout().write_all(data.as_bytes()) {
        Ok(_) => NativeCallResult::null(),
        Err(e) => NativeCallResult::Error(format!("io.write: {}", e)),
    }
}

/// Write string + newline to stdout
pub fn writeln(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let data = match ctx.read_string(args[0]) {
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
pub fn write_err(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let data = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("io.writeErr: {}", e)),
    };
    match io::stderr().write_all(data.as_bytes()) {
        Ok(_) => NativeCallResult::null(),
        Err(e) => NativeCallResult::Error(format!("io.writeErr: {}", e)),
    }
}

/// Write string + newline to stderr
pub fn write_errln(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let data = match ctx.read_string(args[0]) {
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

/// Read exactly `n` bytes from stdin (blocking → IO pool)
pub fn read_exact(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let n = args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as usize;
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            let mut buf = vec![0u8; n];
            match io::stdin().lock().read_exact(&mut buf) {
                Ok(_) => IoCompletion::String(String::from_utf8_lossy(&buf).into_owned()),
                Err(e) => IoCompletion::Error(format!("io.readExact: {}", e)),
            }
        }),
    })
}

/// Flush stdout
pub fn flush(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    match io::stdout().flush() {
        Ok(_) => NativeCallResult::null(),
        Err(e) => NativeCallResult::Error(format!("io.flush: {}", e)),
    }
}
