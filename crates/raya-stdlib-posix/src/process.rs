//! std:process — Process management

use crate::handles::HandleRegistry;
use raya_engine::vm::{NativeCallResult, NativeContext, NativeValue, string_read, string_allocate, array_allocate};
use std::sync::LazyLock;

/// Cached result of a process execution
struct ExecResult {
    exit_code: i32,
    stdout: String,
    stderr: String,
}

static EXEC_HANDLES: LazyLock<HandleRegistry<ExecResult>> = LazyLock::new(HandleRegistry::new);

/// Exit the process
pub fn exit(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let code = args.first()
        .and_then(|v| v.as_i32().or_else(|| v.as_f64().map(|f| f as i32)))
        .unwrap_or(0);
    std::process::exit(code);
}

/// Get current process ID
pub fn pid(_ctx: &NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    NativeCallResult::f64(std::process::id() as f64)
}

/// Get command-line arguments
pub fn argv(ctx: &NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let args: Vec<NativeValue> = std::env::args()
        .map(|a| string_allocate(ctx, a))
        .collect();
    NativeCallResult::Value(array_allocate(ctx, &args))
}

/// Get path to current executable
pub fn exec_path(ctx: &NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    match std::env::current_exe() {
        Ok(path) => NativeCallResult::Value(string_allocate(ctx, path.to_string_lossy().into_owned())),
        Err(e) => NativeCallResult::Error(format!("process.execPath: {}", e)),
    }
}

/// Execute shell command, return handle to read results
pub fn exec(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let command = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("process.exec: {}", e)),
    };

    let output = if cfg!(target_os = "windows") {
        std::process::Command::new("cmd").args(["/C", &command]).output()
    } else {
        std::process::Command::new("sh").args(["-c", &command]).output()
    };

    match output {
        Ok(out) => {
            let result = ExecResult {
                exit_code: out.status.code().unwrap_or(-1),
                stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            };
            let handle = EXEC_HANDLES.insert(result);
            NativeCallResult::f64(handle as f64)
        }
        Err(e) => NativeCallResult::Error(format!("process.exec: {}", e)),
    }
}

/// Get exit code from exec handle
pub fn exec_get_code(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match EXEC_HANDLES.get(handle) {
        Some(r) => NativeCallResult::i32(r.exit_code),
        None => NativeCallResult::Error(format!("process.execGetCode: invalid handle {}", handle)),
    }
}

/// Get stdout from exec handle
pub fn exec_get_stdout(ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match EXEC_HANDLES.get(handle) {
        Some(r) => NativeCallResult::Value(string_allocate(ctx, r.stdout.clone())),
        None => NativeCallResult::Error(format!("process.execGetStdout: invalid handle {}", handle)),
    }
}

/// Get stderr from exec handle
pub fn exec_get_stderr(ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    match EXEC_HANDLES.get(handle) {
        Some(r) => NativeCallResult::Value(string_allocate(ctx, r.stderr.clone())),
        None => NativeCallResult::Error(format!("process.execGetStderr: invalid handle {}", handle)),
    }
}

/// Release exec handle
pub fn exec_release(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let handle = args.first()
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u64;
    EXEC_HANDLES.remove(handle);
    NativeCallResult::null()
}
