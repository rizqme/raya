//! Shared file collection utilities for CLI commands.

use std::path::{Path, PathBuf};

/// Collect all .raya source files from the given paths (files or directories).
pub fn collect_raya_files(paths: &[String]) -> anyhow::Result<Vec<PathBuf>> {
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
