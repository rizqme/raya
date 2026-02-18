//! std:fs — Filesystem operations

use raya_sdk::{NativeCallResult, NativeContext, NativeValue, IoRequest, IoCompletion};
use std::time::UNIX_EPOCH;

/// Read file as binary Buffer
pub fn read_file(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.readFile: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match std::fs::read(&path) {
                Ok(data) => IoCompletion::Bytes(data),
                Err(e) => IoCompletion::Error(format!("fs.readFile: {}", e)),
            }
        }),
    })
}

/// Read file as UTF-8 string
pub fn read_text_file(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.readTextFile: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match std::fs::read_to_string(&path) {
                Ok(data) => IoCompletion::String(data),
                Err(e) => IoCompletion::Error(format!("fs.readTextFile: {}", e)),
            }
        }),
    })
}

/// Write binary Buffer to file
pub fn write_file(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.writeFile: {}", e)),
    };
    let data = match ctx.read_buffer(args[1]) {
        Ok(d) => d,
        Err(e) => return NativeCallResult::Error(format!("fs.writeFile: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match std::fs::write(&path, &data) {
                Ok(_) => IoCompletion::Primitive(NativeValue::null()),
                Err(e) => IoCompletion::Error(format!("fs.writeFile: {}", e)),
            }
        }),
    })
}

/// Write string to file
pub fn write_text_file(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.writeTextFile: {}", e)),
    };
    let data = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.writeTextFile: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match std::fs::write(&path, data.as_bytes()) {
                Ok(_) => IoCompletion::Primitive(NativeValue::null()),
                Err(e) => IoCompletion::Error(format!("fs.writeTextFile: {}", e)),
            }
        }),
    })
}

/// Append string to file
pub fn append_file(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.appendFile: {}", e)),
    };
    let data = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.appendFile: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            use std::io::Write;
            match std::fs::OpenOptions::new().append(true).create(true).open(&path) {
                Ok(mut file) => match file.write_all(data.as_bytes()) {
                    Ok(_) => IoCompletion::Primitive(NativeValue::null()),
                    Err(e) => IoCompletion::Error(format!("fs.appendFile: {}", e)),
                },
                Err(e) => IoCompletion::Error(format!("fs.appendFile: {}", e)),
            }
        }),
    })
}

/// Check if path exists
pub fn exists(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.exists: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            IoCompletion::Primitive(NativeValue::bool(std::path::Path::new(&path).exists()))
        }),
    })
}

/// Check if path is a file
pub fn is_file(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.isFile: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            IoCompletion::Primitive(NativeValue::bool(std::path::Path::new(&path).is_file()))
        }),
    })
}

/// Check if path is a directory
pub fn is_dir(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.isDir: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            IoCompletion::Primitive(NativeValue::bool(std::path::Path::new(&path).is_dir()))
        }),
    })
}

/// Check if path is a symlink
pub fn is_symlink(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.isSymlink: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            IoCompletion::Primitive(NativeValue::bool(std::path::Path::new(&path).is_symlink()))
        }),
    })
}

/// Get file size in bytes
pub fn file_size(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.fileSize: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match std::fs::metadata(&path) {
                Ok(m) => IoCompletion::Primitive(NativeValue::f64(m.len() as f64)),
                Err(e) => IoCompletion::Error(format!("fs.fileSize: {}", e)),
            }
        }),
    })
}

/// Get last modified time (ms since epoch)
pub fn last_modified(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.lastModified: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match std::fs::metadata(&path) {
                Ok(m) => {
                    let ms = m.modified()
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as f64)
                        .unwrap_or(0.0);
                    IoCompletion::Primitive(NativeValue::f64(ms))
                }
                Err(e) => IoCompletion::Error(format!("fs.lastModified: {}", e)),
            }
        }),
    })
}

/// Packed stat: [size, isFile(0/1), isDir(0/1), isSymlink(0/1), modifiedMs, createdMs, mode]
/// Kept synchronous — metadata() is a fast kernel-cached syscall
pub fn stat(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.stat: {}", e)),
    };
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) => return NativeCallResult::Error(format!("fs.stat: {}", e)),
    };
    let size = meta.len() as f64;
    let is_file = if meta.is_file() { 1.0 } else { 0.0 };
    let is_dir = if meta.is_dir() { 1.0 } else { 0.0 };
    let is_symlink = if meta.file_type().is_symlink() { 1.0 } else { 0.0 };
    let modified = meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0);
    let created = meta.created()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0);
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() as f64
    };
    #[cfg(not(unix))]
    let mode = 0.0;

    let items = [
        NativeValue::f64(size),
        NativeValue::f64(is_file),
        NativeValue::f64(is_dir),
        NativeValue::f64(is_symlink),
        NativeValue::f64(modified),
        NativeValue::f64(created),
        NativeValue::f64(mode),
    ];
    NativeCallResult::Value(ctx.create_array(&items))
}

/// Create directory
pub fn mkdir(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.mkdir: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match std::fs::create_dir(&path) {
                Ok(_) => IoCompletion::Primitive(NativeValue::null()),
                Err(e) => IoCompletion::Error(format!("fs.mkdir: {}", e)),
            }
        }),
    })
}

/// Create directory tree (recursive)
pub fn mkdir_recursive(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.mkdirRecursive: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match std::fs::create_dir_all(&path) {
                Ok(_) => IoCompletion::Primitive(NativeValue::null()),
                Err(e) => IoCompletion::Error(format!("fs.mkdirRecursive: {}", e)),
            }
        }),
    })
}

/// List directory entries
pub fn read_dir(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.readDir: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match std::fs::read_dir(&path) {
                Ok(entries) => {
                    let mut items = Vec::new();
                    for entry in entries {
                        if let Ok(entry) = entry {
                            items.push(entry.file_name().to_string_lossy().into_owned());
                        }
                    }
                    IoCompletion::StringArray(items)
                }
                Err(e) => IoCompletion::Error(format!("fs.readDir: {}", e)),
            }
        }),
    })
}

/// Remove empty directory
pub fn rmdir(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.rmdir: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match std::fs::remove_dir(&path) {
                Ok(_) => IoCompletion::Primitive(NativeValue::null()),
                Err(e) => IoCompletion::Error(format!("fs.rmdir: {}", e)),
            }
        }),
    })
}

/// Remove file
pub fn remove(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.remove: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match std::fs::remove_file(&path) {
                Ok(_) => IoCompletion::Primitive(NativeValue::null()),
                Err(e) => IoCompletion::Error(format!("fs.remove: {}", e)),
            }
        }),
    })
}

/// Rename/move file
pub fn rename(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let from = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.rename: {}", e)),
    };
    let to = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.rename: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match std::fs::rename(&from, &to) {
                Ok(_) => IoCompletion::Primitive(NativeValue::null()),
                Err(e) => IoCompletion::Error(format!("fs.rename: {}", e)),
            }
        }),
    })
}

/// Copy file
pub fn copy(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let from = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.copy: {}", e)),
    };
    let to = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.copy: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match std::fs::copy(&from, &to) {
                Ok(_) => IoCompletion::Primitive(NativeValue::null()),
                Err(e) => IoCompletion::Error(format!("fs.copy: {}", e)),
            }
        }),
    })
}

/// Change file permissions
#[cfg(unix)]
pub fn chmod(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.chmod: {}", e)),
    };
    let mode = args.get(1)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u32;
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(mode);
            match std::fs::set_permissions(&path, perms) {
                Ok(_) => IoCompletion::Primitive(NativeValue::null()),
                Err(e) => IoCompletion::Error(format!("fs.chmod: {}", e)),
            }
        }),
    })
}

#[cfg(not(unix))]
pub fn chmod(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    NativeCallResult::Error("fs.chmod: not supported on this platform".to_string())
}

/// Create symbolic link
#[cfg(unix)]
pub fn symlink(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let target = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.symlink: {}", e)),
    };
    let link = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.symlink: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match std::os::unix::fs::symlink(&target, &link) {
                Ok(_) => IoCompletion::Primitive(NativeValue::null()),
                Err(e) => IoCompletion::Error(format!("fs.symlink: {}", e)),
            }
        }),
    })
}

#[cfg(not(unix))]
pub fn symlink(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    NativeCallResult::Error("fs.symlink: not supported on this platform".to_string())
}

/// Read symlink target
pub fn readlink(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.readlink: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match std::fs::read_link(&path) {
                Ok(target) => IoCompletion::String(target.to_string_lossy().into_owned()),
                Err(e) => IoCompletion::Error(format!("fs.readlink: {}", e)),
            }
        }),
    })
}

/// Resolve to canonical absolute path
pub fn realpath(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.realpath: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            match std::fs::canonicalize(&path) {
                Ok(abs) => IoCompletion::String(abs.to_string_lossy().into_owned()),
                Err(e) => IoCompletion::Error(format!("fs.realpath: {}", e)),
            }
        }),
    })
}

/// Get OS temp directory
pub fn temp_dir(_ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(|| {
            let dir = std::env::temp_dir();
            IoCompletion::String(dir.to_string_lossy().into_owned())
        }),
    })
}

/// Create a temp file and return its path
pub fn temp_file(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let prefix = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.tempFile: {}", e)),
    };
    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || {
            let dir = std::env::temp_dir();
            let name = format!("{}{}", prefix, std::process::id());
            let path = dir.join(name);
            match std::fs::File::create(&path) {
                Ok(_) => IoCompletion::String(path.to_string_lossy().into_owned()),
                Err(e) => IoCompletion::Error(format!("fs.tempFile: {}", e)),
            }
        }),
    })
}
