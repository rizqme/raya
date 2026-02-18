//! Path module implementation (std:path)
//!
//! Native implementation using the SDK for path manipulation,
//! resolution, and OS-specific constants.

use raya_sdk::{NativeCallResult, NativeContext, NativeValue};

use std::path::{Path, PathBuf, MAIN_SEPARATOR_STR};

// ============================================================================
// Public API
// ============================================================================

/// Handle path method calls
pub fn call_path_method(
    ctx: &dyn NativeContext,
    method_id: u16,
    args: &[NativeValue],
) -> NativeCallResult {
    match method_id {
        0x6000 => join(ctx, args),           // JOIN
        0x6001 => normalize(ctx, args),      // NORMALIZE
        0x6002 => dirname(ctx, args),        // DIRNAME
        0x6003 => basename(ctx, args),       // BASENAME
        0x6004 => extname(ctx, args),        // EXTNAME
        0x6005 => is_absolute(ctx, args),    // IS_ABSOLUTE
        0x6006 => resolve(ctx, args),        // RESOLVE
        0x6007 => relative(ctx, args),       // RELATIVE
        0x6008 => cwd(ctx, args),            // CWD
        0x6009 => sep(ctx, args),            // SEP
        0x600A => delimiter(ctx, args),      // DELIMITER
        0x600B => strip_ext(ctx, args),      // STRIP_EXT
        0x600C => with_ext(ctx, args),       // WITH_EXT
        _ => NativeCallResult::Unhandled,
    }
}

// ============================================================================
// Method Implementations
// ============================================================================

/// path.join(a, b): string
fn join(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("path.join requires 2 arguments".to_string());
    }

    let a = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid path a: {}", e)),
    };

    let b = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid path b: {}", e)),
    };

    let joined = PathBuf::from(&a).join(&b).to_string_lossy().to_string();
    NativeCallResult::Value(ctx.create_string(&joined))
}

/// path.normalize(p): string
fn normalize(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("path.normalize requires 1 argument".to_string());
    }

    let p = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid path: {}", e)),
    };

    let path = Path::new(&p);
    let mut components = Vec::new();
    for comp in path.components() {
        match comp {
            std::path::Component::ParentDir => {
                components.pop();
            }
            std::path::Component::CurDir => {}
            other => components.push(other),
        }
    }
    let normalized = components
        .iter()
        .collect::<PathBuf>()
        .to_string_lossy()
        .to_string();
    NativeCallResult::Value(ctx.create_string(&normalized))
}

/// path.dirname(p): string
fn dirname(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("path.dirname requires 1 argument".to_string());
    }

    let p = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid path: {}", e)),
    };

    let dir = Path::new(&p)
        .parent()
        .map_or(".", |p| p.to_str().unwrap_or("."))
        .to_string();
    NativeCallResult::Value(ctx.create_string(&dir))
}

/// path.basename(p): string
fn basename(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("path.basename requires 1 argument".to_string());
    }

    let p = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid path: {}", e)),
    };

    let base = Path::new(&p)
        .file_name()
        .map_or("", |n| n.to_str().unwrap_or(""))
        .to_string();
    NativeCallResult::Value(ctx.create_string(&base))
}

/// path.extname(p): string
fn extname(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("path.extname requires 1 argument".to_string());
    }

    let p = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid path: {}", e)),
    };

    let ext = Path::new(&p)
        .extension()
        .map_or(String::new(), |e| format!(".{}", e.to_string_lossy()));
    NativeCallResult::Value(ctx.create_string(&ext))
}

/// path.isAbsolute(p): boolean
fn is_absolute(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("path.isAbsolute requires 1 argument".to_string());
    }

    let p = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid path: {}", e)),
    };

    NativeCallResult::bool(Path::new(&p).is_absolute())
}

/// path.resolve(from, to): string
fn resolve(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("path.resolve requires 2 arguments".to_string());
    }

    let from = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid from path: {}", e)),
    };

    let to = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid to path: {}", e)),
    };

    let base = if Path::new(&from).is_absolute() {
        PathBuf::from(&from)
    } else {
        std::env::current_dir().unwrap_or_default().join(&from)
    };
    let resolved = base.join(&to).to_string_lossy().to_string();
    NativeCallResult::Value(ctx.create_string(&resolved))
}

/// path.relative(from, to): string
fn relative(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("path.relative requires 2 arguments".to_string());
    }

    let from = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid from path: {}", e)),
    };

    let to = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid to path: {}", e)),
    };

    let rel = pathdiff::diff_paths(&to, &from)
        .map_or(to.clone(), |p| p.to_string_lossy().to_string());
    NativeCallResult::Value(ctx.create_string(&rel))
}

/// path.cwd(): string
fn cwd(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let cwd = std::env::current_dir()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    NativeCallResult::Value(ctx.create_string(&cwd))
}

/// path.sep(): string
fn sep(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    NativeCallResult::Value(ctx.create_string(MAIN_SEPARATOR_STR))
}

/// path.delimiter(): string
fn delimiter(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let delim = if cfg!(windows) { ";" } else { ":" };
    NativeCallResult::Value(ctx.create_string(delim))
}

/// path.stripExt(p): string
fn strip_ext(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("path.stripExt requires 1 argument".to_string());
    }

    let p = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid path: {}", e)),
    };

    let path = Path::new(&p);
    let base = path
        .file_name()
        .map_or(String::new(), |n| n.to_string_lossy().to_string());

    if let Some(ext) = path.extension() {
        let ext_str = ext.to_string_lossy();
        let result = base
            .strip_suffix(&format!(".{}", ext_str))
            .unwrap_or(&base)
            .to_string();
        NativeCallResult::Value(ctx.create_string(&result))
    } else {
        NativeCallResult::Value(ctx.create_string(&base))
    }
}

/// path.withExt(p, ext): string
fn with_ext(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("path.withExt requires 2 arguments".to_string());
    }

    let p = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid path: {}", e)),
    };

    let new_ext = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid extension: {}", e)),
    };

    let path = Path::new(&p);
    if let Some(old_ext) = path.extension() {
        let old_ext_str = format!(".{}", old_ext.to_string_lossy());
        let result = p
            .strip_suffix(&old_ext_str)
            .map(|s| format!("{}{}", s, new_ext))
            .unwrap_or_else(|| format!("{}{}", p, new_ext));
        NativeCallResult::Value(ctx.create_string(&result))
    } else {
        NativeCallResult::Value(ctx.create_string(&format!("{}{}", p, new_ext)))
    }
}
