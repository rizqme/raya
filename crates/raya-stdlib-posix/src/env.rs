//! std:env — Environment variables

use raya_sdk::{NativeCallResult, NativeContext, NativeValue};

/// Get environment variable (empty string if unset)
pub fn get(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let key = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("env.get: {}", e)),
    };
    let val = std::env::var(&key).unwrap_or_default();
    NativeCallResult::Value(ctx.create_string(&val))
}

/// Set environment variable
pub fn set(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let key = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("env.set: {}", e)),
    };
    let val = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("env.set: {}", e)),
    };
    // SAFETY: This is intentional — Raya programs are single-process
    unsafe { std::env::set_var(&key, &val); }
    NativeCallResult::null()
}

/// Remove environment variable
pub fn remove(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let key = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("env.remove: {}", e)),
    };
    // SAFETY: This is intentional — Raya programs are single-process
    unsafe { std::env::remove_var(&key); }
    NativeCallResult::null()
}

/// Check if environment variable exists
pub fn has(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let key = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("env.has: {}", e)),
    };
    NativeCallResult::bool(std::env::var(&key).is_ok())
}

/// Get all environment variables as flat [key, value, key, value, ...] array
pub fn all(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let mut items = Vec::new();
    for (key, val) in std::env::vars() {
        items.push(ctx.create_string(&key));
        items.push(ctx.create_string(&val));
    }
    NativeCallResult::Value(ctx.create_array(&items))
}

/// Get current working directory
pub fn cwd(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    match std::env::current_dir() {
        Ok(path) => NativeCallResult::Value(ctx.create_string(&path.to_string_lossy())),
        Err(e) => NativeCallResult::Error(format!("env.cwd: {}", e)),
    }
}

/// Get home directory
pub fn home(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    NativeCallResult::Value(ctx.create_string(&home))
}

/// Get user config directory
/// XDG_CONFIG_HOME, or macOS ~/Library/Application Support, fallback ~/.config
pub fn config_dir(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let dir = std::env::var("XDG_CONFIG_HOME").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_default();
        if cfg!(target_os = "macos") {
            format!("{}/Library/Application Support", home)
        } else {
            format!("{}/.config", home)
        }
    });
    NativeCallResult::Value(ctx.create_string(&dir))
}

/// Get user cache directory
/// XDG_CACHE_HOME, or macOS ~/Library/Caches, fallback ~/.cache
pub fn cache_dir(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let dir = std::env::var("XDG_CACHE_HOME").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_default();
        if cfg!(target_os = "macos") {
            format!("{}/Library/Caches", home)
        } else {
            format!("{}/.cache", home)
        }
    });
    NativeCallResult::Value(ctx.create_string(&dir))
}

/// Get user data directory
/// XDG_DATA_HOME, or macOS ~/Library/Application Support, fallback ~/.local/share
pub fn data_dir(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let dir = std::env::var("XDG_DATA_HOME").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_default();
        if cfg!(target_os = "macos") {
            format!("{}/Library/Application Support", home)
        } else {
            format!("{}/.local/share", home)
        }
    });
    NativeCallResult::Value(ctx.create_string(&dir))
}

/// Get user state directory
/// XDG_STATE_HOME, fallback ~/.local/state
pub fn state_dir(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let dir = std::env::var("XDG_STATE_HOME").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{}/.local/state", home)
    });
    NativeCallResult::Value(ctx.create_string(&dir))
}

/// Get runtime directory
/// XDG_RUNTIME_DIR, fallback /tmp
pub fn runtime_dir(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let dir = std::env::var("XDG_RUNTIME_DIR").ok().filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/tmp".to_string());
    NativeCallResult::Value(ctx.create_string(&dir))
}
