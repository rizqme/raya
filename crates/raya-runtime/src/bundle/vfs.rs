//! Virtual Filesystem (VFS)
//!
//! Provides a unified file access layer for both dev mode and bundle mode:
//! - **DiskBacked**: reads from the real filesystem (for `raya run`)
//! - **Embedded**: reads from data embedded in the binary (for bundled executables)
//!
//! The VFS is populated from `[assets]` in raya.toml. In dev mode, file paths
//! resolve relative to the project root. In bundle mode, file data is embedded
//! in the VFS section of the binary payload.
//!
//! Native fs handlers (fs.readTextFile, fs.readFile, fs.exists) check the VFS
//! first, falling through to the real filesystem if the path isn't in the VFS.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A single entry in the virtual filesystem.
#[derive(Debug, Clone)]
pub enum VfsEntry {
    /// Dev mode: file lives on disk, read on demand.
    DiskBacked(PathBuf),

    /// Bundle mode: file data embedded in the binary.
    Embedded(Vec<u8>),
}

/// The virtual filesystem.
///
/// Maps virtual paths (as used in Raya code) to file entries.
/// Paths are normalized: forward slashes, no leading `./`.
#[derive(Debug, Clone)]
pub struct Vfs {
    entries: HashMap<String, VfsEntry>,
}

impl Vfs {
    /// Create a VFS from a map of entries.
    pub fn new(entries: HashMap<String, VfsEntry>) -> Self {
        Self { entries }
    }

    /// Create an empty VFS (no assets).
    pub fn empty() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Create a disk-backed VFS from a list of include patterns.
    ///
    /// Resolves paths relative to `base_dir`.
    pub fn from_disk(
        base_dir: &Path,
        include: &[String],
        exclude: &[String],
    ) -> Self {
        let mut entries = HashMap::new();

        for pattern in include {
            let full_pattern = base_dir.join(pattern);

            // Simple glob matching: if pattern ends with /*, include all files
            // For now, handle direct files and simple directory patterns
            let pattern_str = full_pattern.to_string_lossy();

            if let Ok(paths) = glob_paths(&pattern_str) {
                for path in paths {
                    // Skip excluded patterns
                    let relative = path.strip_prefix(base_dir)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .replace('\\', "/");

                    if is_excluded(&relative, exclude) {
                        continue;
                    }

                    if path.is_file() {
                        entries.insert(relative, VfsEntry::DiskBacked(path));
                    }
                }
            } else {
                // Pattern didn't match as glob — try as direct file path
                let direct = base_dir.join(pattern);
                if direct.is_file() {
                    let relative = pattern.replace('\\', "/");
                    if !is_excluded(&relative, exclude) {
                        entries.insert(relative, VfsEntry::DiskBacked(direct));
                    }
                }
            }
        }

        Self { entries }
    }

    /// Read a file from the VFS.
    ///
    /// Returns `None` if the path is not in the VFS.
    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        let normalized = normalize_path(path);
        match self.entries.get(&normalized)? {
            VfsEntry::DiskBacked(p) => std::fs::read(p).ok(),
            VfsEntry::Embedded(data) => Some(data.clone()),
        }
    }

    /// Read a file as UTF-8 text from the VFS.
    pub fn read_text(&self, path: &str) -> Option<String> {
        let data = self.read(path)?;
        String::from_utf8(data).ok()
    }

    /// Check if a path exists in the VFS.
    pub fn exists(&self, path: &str) -> bool {
        let normalized = normalize_path(path);
        self.entries.contains_key(&normalized)
    }

    /// List all paths in the VFS.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(|s| s.as_str())
    }

    /// Number of entries in the VFS.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the VFS is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Collect all entries for embedding in a bundle.
    ///
    /// Returns (path, data) pairs. DiskBacked entries are read from disk.
    pub fn collect_for_embedding(&self) -> Vec<(String, Vec<u8>)> {
        let mut result = Vec::new();
        for (path, entry) in &self.entries {
            let data = match entry {
                VfsEntry::DiskBacked(p) => {
                    match std::fs::read(p) {
                        Ok(data) => data,
                        Err(_) => continue,
                    }
                }
                VfsEntry::Embedded(data) => data.clone(),
            };
            result.push((path.clone(), data));
        }
        result.sort_by(|a, b| a.0.cmp(&b.0)); // deterministic order
        result
    }
}

/// Normalize a path for VFS lookup.
///
/// - Replace backslashes with forward slashes
/// - Remove leading `./`
/// - Remove trailing `/`
fn normalize_path(path: &str) -> String {
    let mut p = path.replace('\\', "/");
    while p.starts_with("./") {
        p = p[2..].to_string();
    }
    while p.ends_with('/') {
        p.pop();
    }
    p
}

/// Simple glob matching for file paths.
fn glob_paths(pattern: &str) -> Result<Vec<PathBuf>, ()> {
    // Handle ** and * glob patterns
    let pattern_str = pattern.to_string();

    if pattern_str.contains('*') {
        // Use simple recursive directory scanning for glob patterns
        let (base, glob) = split_at_glob(&pattern_str);
        let base_path = Path::new(&base);

        if !base_path.exists() {
            return Ok(Vec::new());
        }

        let mut results = Vec::new();
        collect_matching_files(base_path, &glob, &mut results);
        Ok(results)
    } else {
        let path = PathBuf::from(pattern);
        if path.exists() {
            if path.is_dir() {
                // Include all files in directory
                let mut results = Vec::new();
                collect_all_files(&path, &mut results);
                Ok(results)
            } else {
                Ok(vec![path])
            }
        } else {
            Ok(Vec::new())
        }
    }
}

/// Split a glob pattern at the first `*` character.
fn split_at_glob(pattern: &str) -> (String, String) {
    if let Some(pos) = pattern.find('*') {
        let base = &pattern[..pos];
        // Find the last separator before the glob
        let sep_pos = base.rfind('/').map(|p| p + 1).unwrap_or(0);
        (pattern[..sep_pos].to_string(), pattern[sep_pos..].to_string())
    } else {
        (pattern.to_string(), String::new())
    }
}

/// Recursively collect all files in a directory.
fn collect_all_files(dir: &Path, results: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_all_files(&path, results);
            } else {
                results.push(path);
            }
        }
    }
}

/// Collect files matching a simple glob pattern (supports * and **).
fn collect_matching_files(base: &Path, _glob: &str, results: &mut Vec<PathBuf>) {
    // Simplified: for now, just collect all files under base
    // TODO: Implement proper glob matching
    collect_all_files(base, results);
}

/// Check if a path matches any exclusion pattern.
fn is_excluded(path: &str, exclude: &[String]) -> bool {
    for pattern in exclude {
        if pattern.contains("**") {
            let suffix = pattern.trim_start_matches("**/");
            if suffix.starts_with('*') {
                // e.g. *.tmp → match extension
                let ext = &suffix[1..]; // ".tmp"
                if path.ends_with(ext) {
                    return true;
                }
            } else if path.ends_with(suffix) {
                return true;
            }
        } else if path.starts_with(pattern.as_str()) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path() {
        assert_eq!(normalize_path("./config.json"), "config.json");
        assert_eq!(normalize_path("assets/image.png"), "assets/image.png");
        assert_eq!(normalize_path("assets\\image.png"), "assets/image.png");
        assert_eq!(normalize_path("./assets/"), "assets");
    }

    #[test]
    fn test_vfs_empty() {
        let vfs = Vfs::empty();
        assert!(vfs.is_empty());
        assert_eq!(vfs.len(), 0);
        assert!(!vfs.exists("anything"));
        assert!(vfs.read("anything").is_none());
    }

    #[test]
    fn test_vfs_embedded() {
        let mut entries = HashMap::new();
        entries.insert(
            "config.json".to_string(),
            VfsEntry::Embedded(b"{\"key\": \"value\"}".to_vec()),
        );
        entries.insert(
            "assets/logo.png".to_string(),
            VfsEntry::Embedded(vec![0x89, 0x50, 0x4E, 0x47]),
        );

        let vfs = Vfs::new(entries);

        assert_eq!(vfs.len(), 2);
        assert!(vfs.exists("config.json"));
        assert!(vfs.exists("assets/logo.png"));
        assert!(!vfs.exists("missing.txt"));

        assert_eq!(
            vfs.read_text("config.json").unwrap(),
            "{\"key\": \"value\"}"
        );

        assert_eq!(
            vfs.read("assets/logo.png").unwrap(),
            vec![0x89, 0x50, 0x4E, 0x47]
        );
    }

    #[test]
    fn test_vfs_disk_backed() {
        let temp = tempfile::tempdir().unwrap();
        let file_path = temp.path().join("test.txt");
        std::fs::write(&file_path, "hello world").unwrap();

        let mut entries = HashMap::new();
        entries.insert(
            "test.txt".to_string(),
            VfsEntry::DiskBacked(file_path),
        );

        let vfs = Vfs::new(entries);
        assert!(vfs.exists("test.txt"));
        assert_eq!(vfs.read_text("test.txt").unwrap(), "hello world");
    }

    #[test]
    fn test_vfs_collect_for_embedding() {
        let mut entries = HashMap::new();
        entries.insert(
            "b.txt".to_string(),
            VfsEntry::Embedded(b"second".to_vec()),
        );
        entries.insert(
            "a.txt".to_string(),
            VfsEntry::Embedded(b"first".to_vec()),
        );

        let vfs = Vfs::new(entries);
        let collected = vfs.collect_for_embedding();

        assert_eq!(collected.len(), 2);
        // Sorted alphabetically
        assert_eq!(collected[0].0, "a.txt");
        assert_eq!(collected[1].0, "b.txt");
    }

    #[test]
    fn test_is_excluded() {
        assert!(is_excluded("test.tmp", &["**/*.tmp".to_string()]));
        assert!(is_excluded("dir/file.tmp", &["**/*.tmp".to_string()]));
        assert!(!is_excluded("test.txt", &["**/*.tmp".to_string()]));
        assert!(is_excluded("assets/dev-only/file.txt", &["assets/dev-only/".to_string()]));
    }
}
