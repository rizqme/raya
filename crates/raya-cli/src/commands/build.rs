//! `raya build` — Compile Raya source to .ryb bytecode.

use raya_runtime::compile::CompileOptions;
use raya_runtime::Runtime;
use std::path::{Path, PathBuf};

pub fn execute(
    files: Vec<String>,
    out_dir: String,
    release: bool,
    watch: bool,
    sourcemap: bool,
    dry_run: bool,
) -> anyhow::Result<()> {
    let _ = (release, watch); // TODO: wire these flags

    let rt = Runtime::new();
    let out_dir = PathBuf::from(&out_dir);

    let options = CompileOptions {
        sourcemap,
    };

    // Collect .raya files from input paths
    let source_files = collect_raya_files(&files)?;

    if source_files.is_empty() {
        anyhow::bail!("No .raya source files found in: {:?}", files);
    }

    println!("Building {} file(s)...", source_files.len());

    for src_path in &source_files {
        let out_path = compute_output_path(src_path, &out_dir);

        if dry_run {
            println!("  {} → {} (dry run)", src_path.display(), out_path.display());
            continue;
        }

        let compiled = rt
            .compile_file_with_options(src_path, &options)
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(&out_path, compiled.encode())?;
        println!("  {} → {}", src_path.display(), out_path.display());
    }

    Ok(())
}

/// Collect all .raya source files from the given paths (files or directories).
fn collect_raya_files(paths: &[String]) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for path_str in paths {
        let path = Path::new(path_str);
        if path.is_file() {
            if path.extension().and_then(|e| e.to_str()) == Some("raya") {
                files.push(path.to_path_buf());
            }
        } else if path.is_dir() {
            collect_raya_in_dir(path, &mut files)?;
        } else if path_str == "." {
            // Current directory
            collect_raya_in_dir(Path::new("."), &mut files)?;
        }
    }

    Ok(files)
}

/// Recursively collect .raya files in a directory.
fn collect_raya_in_dir(dir: &Path, files: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        // Skip hidden dirs, raya_packages, dist, node_modules
        if path.is_dir() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('.')
                || name_str == "raya_packages"
                || name_str == "dist"
                || name_str == "node_modules"
            {
                continue;
            }
            collect_raya_in_dir(&path, files)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("raya") {
            files.push(path);
        }
    }
    Ok(())
}

/// Compute the output .ryb path for a source file.
///
/// `src/main.raya` → `dist/src/main.ryb`
fn compute_output_path(src: &Path, out_dir: &Path) -> PathBuf {
    let stem = src.with_extension("ryb");
    out_dir.join(stem.file_name().unwrap_or_default())
}
