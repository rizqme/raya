//! std:archive — Archive creation and extraction (tar, tar.gz, zip)
//!
//! All operations are blocking I/O dispatched to the thread pool via
//! `Suspend(IoRequest::BlockingWork { ... })`.

use raya_sdk::{IoCompletion, IoRequest, NativeCallResult, NativeContext, NativeValue};
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;

// ============================================================================
// Helper: read string[] array from a NativeValue
// ============================================================================

fn read_string_array(ctx: &dyn NativeContext, val: NativeValue) -> Result<Vec<String>, String> {
    let len = ctx
        .array_len(val)
        .map_err(|e| format!("expected string array: {}", e))?;
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        let elem = ctx
            .array_get(val, i)
            .map_err(|e| format!("array index {}: {}", i, e))?;
        let s = ctx
            .read_string(elem)
            .map_err(|e| format!("array element {}: {}", i, e))?;
        out.push(s);
    }
    Ok(out)
}

// ============================================================================
// Tar helpers
// ============================================================================

fn create_tar_impl(output_path: &str, input_paths: &[String]) -> io::Result<()> {
    let file = File::create(output_path)?;
    let mut builder = tar::Builder::new(file);
    for path_str in input_paths {
        let path = Path::new(path_str);
        if path.is_dir() {
            builder.append_dir_all(path, path)?;
        } else {
            let name = path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy();
            let mut f = File::open(path)?;
            builder.append_file(name.as_ref(), &mut f)?;
        }
    }
    builder.finish()?;
    Ok(())
}

fn extract_tar_impl(archive_path: &str, output_dir: &str) -> io::Result<()> {
    let file = File::open(archive_path)?;
    let mut archive = tar::Archive::new(file);
    archive.unpack(output_dir)?;
    Ok(())
}

fn list_tar_impl(archive_path: &str) -> io::Result<Vec<String>> {
    let file = File::open(archive_path)?;
    let mut archive = tar::Archive::new(file);
    let mut entries_list = Vec::new();
    for entry in archive.entries()? {
        let entry = entry?;
        let path = entry.path()?;
        entries_list.push(path.to_string_lossy().into_owned());
    }
    Ok(entries_list)
}

// ============================================================================
// Tar.gz helpers
// ============================================================================

fn create_tgz_impl(output_path: &str, input_paths: &[String]) -> io::Result<()> {
    let file = File::create(output_path)?;
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut builder = tar::Builder::new(encoder);
    for path_str in input_paths {
        let path = Path::new(path_str);
        if path.is_dir() {
            builder.append_dir_all(path, path)?;
        } else {
            let name = path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy();
            let mut f = File::open(path)?;
            builder.append_file(name.as_ref(), &mut f)?;
        }
    }
    builder.into_inner()?.finish()?;
    Ok(())
}

fn extract_tgz_impl(archive_path: &str, output_dir: &str) -> io::Result<()> {
    let file = File::open(archive_path)?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(output_dir)?;
    Ok(())
}

// ============================================================================
// Zip helpers
// ============================================================================

fn create_zip_impl(output_path: &str, input_paths: &[String]) -> io::Result<()> {
    let file = File::create(output_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    for path_str in input_paths {
        let path = Path::new(path_str);
        if path.is_dir() {
            add_dir_to_zip(&mut zip, path, path, options)?;
        } else {
            let name = path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy();
            zip.start_file(name.as_ref(), options)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            let mut f = File::open(path)?;
            io::copy(&mut f, &mut zip)?;
        }
    }
    zip.finish()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    Ok(())
}

fn add_dir_to_zip<W: Write + io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    base: &Path,
    dir: &Path,
    options: zip::write::SimpleFileOptions,
) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = path
            .strip_prefix(base)
            .unwrap_or(&path)
            .to_string_lossy()
            .into_owned();
        if path.is_dir() {
            zip.add_directory(&name, options)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            add_dir_to_zip(zip, base, &path, options)?;
        } else {
            zip.start_file(&name, options)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            let mut f = File::open(&path)?;
            io::copy(&mut f, zip)?;
        }
    }
    Ok(())
}

fn extract_zip_impl(archive_path: &str, output_dir: &str) -> io::Result<()> {
    let file = File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let out = Path::new(output_dir);

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let entry_path = match entry.enclosed_name() {
            Some(p) => out.join(p),
            None => continue,
        };

        if entry.is_dir() {
            std::fs::create_dir_all(&entry_path)?;
        } else {
            if let Some(parent) = entry_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut outfile = File::create(&entry_path)?;
            io::copy(&mut entry, &mut outfile)?;
        }
    }
    Ok(())
}

fn list_zip_impl(archive_path: &str) -> io::Result<Vec<String>> {
    let file = File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let mut entries = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let entry = archive
            .by_index_raw(i)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        entries.push(entry.name().to_owned());
    }
    Ok(entries)
}

// ============================================================================
// Native functions
// ============================================================================

/// Create a tar archive from the given input paths.
///
/// Args: `[output_path: string, input_paths: string[]]`
pub fn tar_create(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let output_path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("archive.tarCreate: {}", e)),
    };
    let paths = match read_string_array(ctx, args[1]) {
        Ok(p) => p,
        Err(e) => return NativeCallResult::Error(format!("archive.tarCreate: {}", e)),
    };

    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || match create_tar_impl(&output_path, &paths) {
            Ok(()) => IoCompletion::Primitive(NativeValue::null()),
            Err(e) => IoCompletion::Error(format!("archive.tarCreate: {}", e)),
        }),
    })
}

/// Extract a tar archive to the given output directory.
///
/// Args: `[archive_path: string, output_dir: string]`
pub fn tar_extract(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let archive_path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("archive.tarExtract: {}", e)),
    };
    let output_dir = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("archive.tarExtract: {}", e)),
    };

    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || match extract_tar_impl(&archive_path, &output_dir) {
            Ok(()) => IoCompletion::Primitive(NativeValue::null()),
            Err(e) => IoCompletion::Error(format!("archive.tarExtract: {}", e)),
        }),
    })
}

/// List the contents of a tar archive.
///
/// Args: `[archive_path: string]`
/// Returns: `string[]` of entry paths.
pub fn tar_list(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let archive_path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("archive.tarList: {}", e)),
    };

    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || match list_tar_impl(&archive_path) {
            Ok(entries) => IoCompletion::StringArray(entries),
            Err(e) => IoCompletion::Error(format!("archive.tarList: {}", e)),
        }),
    })
}

/// Create a gzip-compressed tar archive (.tar.gz / .tgz) from the given input paths.
///
/// Args: `[output_path: string, input_paths: string[]]`
pub fn tgz_create(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let output_path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("archive.tgzCreate: {}", e)),
    };
    let paths = match read_string_array(ctx, args[1]) {
        Ok(p) => p,
        Err(e) => return NativeCallResult::Error(format!("archive.tgzCreate: {}", e)),
    };

    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || match create_tgz_impl(&output_path, &paths) {
            Ok(()) => IoCompletion::Primitive(NativeValue::null()),
            Err(e) => IoCompletion::Error(format!("archive.tgzCreate: {}", e)),
        }),
    })
}

/// Extract a gzip-compressed tar archive (.tar.gz / .tgz) to the given output directory.
///
/// Args: `[archive_path: string, output_dir: string]`
pub fn tgz_extract(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let archive_path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("archive.tgzExtract: {}", e)),
    };
    let output_dir = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("archive.tgzExtract: {}", e)),
    };

    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || match extract_tgz_impl(&archive_path, &output_dir) {
            Ok(()) => IoCompletion::Primitive(NativeValue::null()),
            Err(e) => IoCompletion::Error(format!("archive.tgzExtract: {}", e)),
        }),
    })
}

/// Create a zip archive from the given input paths.
///
/// Args: `[output_path: string, input_paths: string[]]`
pub fn zip_create(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let output_path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("archive.zipCreate: {}", e)),
    };
    let paths = match read_string_array(ctx, args[1]) {
        Ok(p) => p,
        Err(e) => return NativeCallResult::Error(format!("archive.zipCreate: {}", e)),
    };

    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || match create_zip_impl(&output_path, &paths) {
            Ok(()) => IoCompletion::Primitive(NativeValue::null()),
            Err(e) => IoCompletion::Error(format!("archive.zipCreate: {}", e)),
        }),
    })
}

/// Extract a zip archive to the given output directory.
///
/// Args: `[archive_path: string, output_dir: string]`
pub fn zip_extract(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let archive_path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("archive.zipExtract: {}", e)),
    };
    let output_dir = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("archive.zipExtract: {}", e)),
    };

    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || match extract_zip_impl(&archive_path, &output_dir) {
            Ok(()) => IoCompletion::Primitive(NativeValue::null()),
            Err(e) => IoCompletion::Error(format!("archive.zipExtract: {}", e)),
        }),
    })
}

/// List the contents of a zip archive.
///
/// Args: `[archive_path: string]`
/// Returns: `string[]` of entry names.
pub fn zip_list(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let archive_path = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("archive.zipList: {}", e)),
    };

    NativeCallResult::Suspend(IoRequest::BlockingWork {
        work: Box::new(move || match list_zip_impl(&archive_path) {
            Ok(entries) => IoCompletion::StringArray(entries),
            Err(e) => IoCompletion::Error(format!("archive.zipList: {}", e)),
        }),
    })
}
