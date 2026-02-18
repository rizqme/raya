//! std:env — Environment variables

use raya_engine::vm::{NativeCallResult, NativeContext, NativeValue, string_read, string_allocate, array_allocate};

/// Get environment variable (empty string if unset)
pub fn get(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let key = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("env.get: {}", e)),
    };
    let val = std::env::var(&key).unwrap_or_default();
    NativeCallResult::Value(string_allocate(ctx, val))
}

/// Set environment variable
pub fn set(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let key = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("env.set: {}", e)),
    };
    let val = match string_read(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("env.set: {}", e)),
    };
    // SAFETY: This is intentional — Raya programs are single-process
    unsafe { std::env::set_var(&key, &val); }
    NativeCallResult::null()
}

/// Remove environment variable
pub fn remove(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let key = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("env.remove: {}", e)),
    };
    // SAFETY: This is intentional — Raya programs are single-process
    unsafe { std::env::remove_var(&key); }
    NativeCallResult::null()
}

/// Check if environment variable exists
pub fn has(_ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let key = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("env.has: {}", e)),
    };
    NativeCallResult::bool(std::env::var(&key).is_ok())
}

/// Get all environment variables as flat [key, value, key, value, ...] array
pub fn all(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let mut items = Vec::new();
    for (key, val) in std::env::vars() {
        items.push(string_allocate(ctx, key));
        items.push(string_allocate(ctx, val));
    }
    NativeCallResult::Value(array_allocate(ctx, &items))
}

/// Get current working directory
pub fn cwd(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    match std::env::current_dir() {
        Ok(path) => NativeCallResult::Value(string_allocate(ctx, path.to_string_lossy().into_owned())),
        Err(e) => NativeCallResult::Error(format!("env.cwd: {}", e)),
    }
}

/// Get home directory
pub fn home(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    NativeCallResult::Value(string_allocate(ctx, home))
}
