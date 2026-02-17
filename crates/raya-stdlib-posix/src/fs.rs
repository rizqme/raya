//! std:fs — Filesystem operations

use raya_engine::vm::{NativeCallResult, NativeContext, NativeValue, string_read, string_allocate, buffer_allocate, buffer_read_bytes, array_allocate};
use std::fs;
use std::io::Write;
use std::time::UNIX_EPOCH;

/// Read file as binary Buffer
pub fn read_file(ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.readFile: {}", e)),
    };
    match fs::read(&path) {
        Ok(data) => NativeCallResult::Value(buffer_allocate(ctx, &data)),
        Err(e) => NativeCallResult::Error(format!("fs.readFile: {}", e)),
    }
}

/// Read file as UTF-8 string
pub fn read_text_file(ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.readTextFile: {}", e)),
    };
    match fs::read_to_string(&path) {
        Ok(data) => NativeCallResult::Value(string_allocate(ctx, data)),
        Err(e) => NativeCallResult::Error(format!("fs.readTextFile: {}", e)),
    }
}

/// Write binary Buffer to file
pub fn write_file(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.writeFile: {}", e)),
    };
    let data = match buffer_read_bytes(args[1]) {
        Ok(d) => d,
        Err(e) => return NativeCallResult::Error(format!("fs.writeFile: {}", e)),
    };
    match fs::write(&path, &data) {
        Ok(_) => NativeCallResult::null(),
        Err(e) => NativeCallResult::Error(format!("fs.writeFile: {}", e)),
    }
}

/// Write string to file
pub fn write_text_file(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.writeTextFile: {}", e)),
    };
    let data = match string_read(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.writeTextFile: {}", e)),
    };
    match fs::write(&path, data.as_bytes()) {
        Ok(_) => NativeCallResult::null(),
        Err(e) => NativeCallResult::Error(format!("fs.writeTextFile: {}", e)),
    }
}

/// Append string to file
pub fn append_file(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.appendFile: {}", e)),
    };
    let data = match string_read(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.appendFile: {}", e)),
    };
    match std::fs::OpenOptions::new().append(true).create(true).open(&path) {
        Ok(mut file) => match file.write_all(data.as_bytes()) {
            Ok(_) => NativeCallResult::null(),
            Err(e) => NativeCallResult::Error(format!("fs.appendFile: {}", e)),
        },
        Err(e) => NativeCallResult::Error(format!("fs.appendFile: {}", e)),
    }
}

/// Check if path exists
pub fn exists(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.exists: {}", e)),
    };
    NativeCallResult::bool(std::path::Path::new(&path).exists())
}

/// Check if path is a file
pub fn is_file(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.isFile: {}", e)),
    };
    NativeCallResult::bool(std::path::Path::new(&path).is_file())
}

/// Check if path is a directory
pub fn is_dir(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.isDir: {}", e)),
    };
    NativeCallResult::bool(std::path::Path::new(&path).is_dir())
}

/// Check if path is a symlink
pub fn is_symlink(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.isSymlink: {}", e)),
    };
    NativeCallResult::bool(std::path::Path::new(&path).is_symlink())
}

/// Get file size in bytes
pub fn file_size(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.fileSize: {}", e)),
    };
    match fs::metadata(&path) {
        Ok(m) => NativeCallResult::f64(m.len() as f64),
        Err(e) => NativeCallResult::Error(format!("fs.fileSize: {}", e)),
    }
}

/// Get last modified time (ms since epoch)
pub fn last_modified(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.lastModified: {}", e)),
    };
    match fs::metadata(&path) {
        Ok(m) => {
            let ms = m.modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as f64)
                .unwrap_or(0.0);
            NativeCallResult::f64(ms)
        }
        Err(e) => NativeCallResult::Error(format!("fs.lastModified: {}", e)),
    }
}

/// Packed stat: [size, isFile(0/1), isDir(0/1), isSymlink(0/1), modifiedMs, createdMs, mode]
pub fn stat(ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.stat: {}", e)),
    };
    // Use symlink_metadata to not follow symlinks
    let meta = match fs::symlink_metadata(&path) {
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
    NativeCallResult::Value(array_allocate(ctx, &items))
}

/// Create directory
pub fn mkdir(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.mkdir: {}", e)),
    };
    match fs::create_dir(&path) {
        Ok(_) => NativeCallResult::null(),
        Err(e) => NativeCallResult::Error(format!("fs.mkdir: {}", e)),
    }
}

/// Create directory tree (recursive)
pub fn mkdir_recursive(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.mkdirRecursive: {}", e)),
    };
    match fs::create_dir_all(&path) {
        Ok(_) => NativeCallResult::null(),
        Err(e) => NativeCallResult::Error(format!("fs.mkdirRecursive: {}", e)),
    }
}

/// List directory entries
pub fn read_dir(ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.readDir: {}", e)),
    };
    match fs::read_dir(&path) {
        Ok(entries) => {
            let mut items = Vec::new();
            for entry in entries {
                if let Ok(entry) = entry {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    items.push(string_allocate(ctx, name));
                }
            }
            NativeCallResult::Value(array_allocate(ctx, &items))
        }
        Err(e) => NativeCallResult::Error(format!("fs.readDir: {}", e)),
    }
}

/// Remove empty directory
pub fn rmdir(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.rmdir: {}", e)),
    };
    match fs::remove_dir(&path) {
        Ok(_) => NativeCallResult::null(),
        Err(e) => NativeCallResult::Error(format!("fs.rmdir: {}", e)),
    }
}

/// Remove file
pub fn remove(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.remove: {}", e)),
    };
    match fs::remove_file(&path) {
        Ok(_) => NativeCallResult::null(),
        Err(e) => NativeCallResult::Error(format!("fs.remove: {}", e)),
    }
}

/// Rename/move file
pub fn rename(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let from = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.rename: {}", e)),
    };
    let to = match string_read(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.rename: {}", e)),
    };
    match fs::rename(&from, &to) {
        Ok(_) => NativeCallResult::null(),
        Err(e) => NativeCallResult::Error(format!("fs.rename: {}", e)),
    }
}

/// Copy file
pub fn copy(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let from = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.copy: {}", e)),
    };
    let to = match string_read(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.copy: {}", e)),
    };
    match fs::copy(&from, &to) {
        Ok(_) => NativeCallResult::null(),
        Err(e) => NativeCallResult::Error(format!("fs.copy: {}", e)),
    }
}

/// Change file permissions
#[cfg(unix)]
pub fn chmod(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    use std::os::unix::fs::PermissionsExt;
    let path = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.chmod: {}", e)),
    };
    let mode = args.get(1)
        .and_then(|v| v.as_f64().or_else(|| v.as_i32().map(|i| i as f64)))
        .unwrap_or(0.0) as u32;
    let perms = std::fs::Permissions::from_mode(mode);
    match fs::set_permissions(&path, perms) {
        Ok(_) => NativeCallResult::null(),
        Err(e) => NativeCallResult::Error(format!("fs.chmod: {}", e)),
    }
}

#[cfg(not(unix))]
pub fn chmod(_ctx: &NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    NativeCallResult::Error("fs.chmod: not supported on this platform".to_string())
}

/// Create symbolic link
#[cfg(unix)]
pub fn symlink(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let target = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.symlink: {}", e)),
    };
    let link = match string_read(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.symlink: {}", e)),
    };
    match std::os::unix::fs::symlink(&target, &link) {
        Ok(_) => NativeCallResult::null(),
        Err(e) => NativeCallResult::Error(format!("fs.symlink: {}", e)),
    }
}

#[cfg(not(unix))]
pub fn symlink(_ctx: &NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    NativeCallResult::Error("fs.symlink: not supported on this platform".to_string())
}

/// Read symlink target
pub fn readlink(ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.readlink: {}", e)),
    };
    match fs::read_link(&path) {
        Ok(target) => NativeCallResult::Value(string_allocate(ctx, target.to_string_lossy().into_owned())),
        Err(e) => NativeCallResult::Error(format!("fs.readlink: {}", e)),
    }
}

/// Resolve to canonical absolute path
pub fn realpath(ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let path = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.realpath: {}", e)),
    };
    match fs::canonicalize(&path) {
        Ok(abs) => NativeCallResult::Value(string_allocate(ctx, abs.to_string_lossy().into_owned())),
        Err(e) => NativeCallResult::Error(format!("fs.realpath: {}", e)),
    }
}

/// Get OS temp directory
pub fn temp_dir(ctx: &NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let dir = std::env::temp_dir();
    NativeCallResult::Value(string_allocate(ctx, dir.to_string_lossy().into_owned()))
}

/// Create a temp file and return its path
pub fn temp_file(ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let prefix = match string_read(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("fs.tempFile: {}", e)),
    };
    let dir = std::env::temp_dir();
    let name = format!("{}{}", prefix, std::process::id());
    let path = dir.join(name);
    match std::fs::File::create(&path) {
        Ok(_) => NativeCallResult::Value(string_allocate(ctx, path.to_string_lossy().into_owned())),
        Err(e) => NativeCallResult::Error(format!("fs.tempFile: {}", e)),
    }
}
