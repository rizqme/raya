//! Bundle loader
//!
//! Detects an AOT bundle appended to the current executable and loads it:
//! 1. Read trailer from end of binary
//! 2. Validate magic + checksum
//! 3. mmap code section as executable memory
//! 4. Parse function table → build function pointer map
//! 5. Parse VFS section → mount embedded assets

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use super::format::{AotTrailer, BundledFuncEntry, FUNC_ENTRY_SIZE, TRAILER_SIZE};
use super::vfs::{Vfs, VfsEntry};

/// A loaded AOT code region in executable memory.
pub struct AotCodeRegion {
    /// Base address of the executable memory region.
    #[cfg(unix)]
    base: *const u8,

    /// Size of the region in bytes.
    size: usize,
}

// Safety: The code region is immutable after loading (PROT_READ|PROT_EXEC).
// Multiple threads can safely read/execute from the same region.
unsafe impl Send for AotCodeRegion {}
unsafe impl Sync for AotCodeRegion {}

impl AotCodeRegion {
    /// Get a function pointer at the given offset within the code region.
    ///
    /// # Safety
    /// The offset must point to a valid function entry within the code region.
    pub unsafe fn get_fn_ptr(&self, offset: u64) -> *const u8 {
        #[cfg(unix)]
        {
            self.base.add(offset as usize)
        }
        #[cfg(not(unix))]
        {
            std::ptr::null()
        }
    }

    /// Size of the code region in bytes.
    pub fn size(&self) -> usize {
        self.size
    }
}

#[cfg(unix)]
impl Drop for AotCodeRegion {
    fn drop(&mut self) {
        if !self.base.is_null() && self.size > 0 {
            unsafe {
                libc::munmap(self.base as *mut libc::c_void, self.size);
            }
        }
    }
}

/// A fully loaded bundle: code, function pointers, and VFS.
pub struct BundlePayload {
    /// The executable code region (mmap'd).
    pub code: AotCodeRegion,

    /// Function table: global_func_id → (code_offset, local_count, param_count).
    pub functions: HashMap<u32, LoadedFunction>,

    /// Virtual filesystem with embedded assets.
    pub vfs: Vfs,

    /// Target triple this bundle was compiled for.
    pub target_triple: String,

    /// The entry function's global ID (first function in the first module).
    pub entry_func_id: u32,
}

/// A loaded function entry with its code pointer.
pub struct LoadedFunction {
    /// Offset within the code region.
    pub code_offset: u64,

    /// Number of locals for frame allocation.
    pub local_count: u32,

    /// Number of parameters.
    pub param_count: u32,
}

/// Detect if the current executable has an AOT bundle appended.
///
/// Reads the last `TRAILER_SIZE` bytes of the current binary and checks
/// for the magic marker.
pub fn detect_bundle() -> Option<BundlePayload> {
    let exe_path = std::env::current_exe().ok()?;
    detect_bundle_at(&exe_path)
}

/// Detect an AOT bundle in the given binary file.
pub fn detect_bundle_at(path: &Path) -> Option<BundlePayload> {
    let data = fs::read(path).ok()?;

    if data.len() < TRAILER_SIZE {
        return None;
    }

    // Read trailer from the end
    let trailer_bytes = &data[data.len() - TRAILER_SIZE..];
    let trailer = AotTrailer::from_bytes(trailer_bytes)?;

    // Validate checksum
    let payload_start = trailer.payload_offset as usize;
    let payload_end = data.len() - TRAILER_SIZE;

    if payload_start >= payload_end || payload_start >= data.len() {
        return None;
    }

    let payload = &data[payload_start..payload_end];
    let computed_checksum = crc32fast::hash(payload);
    if computed_checksum != trailer.checksum {
        return None;
    }

    // Load code section into executable memory
    let code_start = payload_start + trailer.code_offset as usize;
    let code_end = code_start + trailer.code_size as usize;
    if code_end > data.len() {
        return None;
    }
    let code_bytes = &data[code_start..code_end];
    let code_region = load_executable_code(code_bytes)?;

    // Parse function table
    let ft_start = payload_start + trailer.func_table_offset as usize;
    let ft_count = trailer.func_table_count as usize;
    let mut functions = HashMap::new();

    for i in 0..ft_count {
        let entry_start = ft_start + i * FUNC_ENTRY_SIZE;
        let entry_end = entry_start + FUNC_ENTRY_SIZE;
        if entry_end > data.len() {
            break;
        }
        if let Some(entry) = BundledFuncEntry::from_bytes(&data[entry_start..entry_end]) {
            functions.insert(entry.global_func_id, LoadedFunction {
                code_offset: entry.code_offset,
                local_count: entry.local_count,
                param_count: entry.param_count,
            });
        }
    }

    // Parse VFS section
    let vfs_start = payload_start + trailer.vfs_offset as usize;
    let vfs_end = vfs_start + trailer.vfs_size as usize;
    let vfs = if vfs_end <= data.len() && trailer.vfs_size > 0 {
        let vfs_data = &data[vfs_start..vfs_end];
        let files = super::format::read_vfs_section(vfs_data);
        let mut entries = HashMap::new();
        for (path, file_data) in files {
            entries.insert(path, VfsEntry::Embedded(file_data));
        }
        Vfs::new(entries)
    } else {
        Vfs::empty()
    };

    // Entry function = first function (global ID 0)
    let entry_func_id = functions.keys().copied().min().unwrap_or(0);

    Some(BundlePayload {
        code: code_region,
        functions,
        vfs,
        target_triple: trailer.decode_target_triple().to_string(),
        entry_func_id,
    })
}

/// Load raw machine code bytes into executable memory via mmap.
#[cfg(unix)]
fn load_executable_code(code_bytes: &[u8]) -> Option<AotCodeRegion> {
    if code_bytes.is_empty() {
        return Some(AotCodeRegion {
            base: std::ptr::null(),
            size: 0,
        });
    }

    unsafe {
        // 1. Allocate writable memory
        let ptr = libc::mmap(
            std::ptr::null_mut(),
            code_bytes.len(),
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANON,
            -1,
            0,
        );

        if ptr == libc::MAP_FAILED {
            return None;
        }

        // 2. Copy machine code
        std::ptr::copy_nonoverlapping(
            code_bytes.as_ptr(),
            ptr as *mut u8,
            code_bytes.len(),
        );

        // 3. Switch to executable (W^X: remove write, add execute)
        let result = libc::mprotect(
            ptr,
            code_bytes.len(),
            libc::PROT_READ | libc::PROT_EXEC,
        );

        if result != 0 {
            libc::munmap(ptr, code_bytes.len());
            return None;
        }

        Some(AotCodeRegion {
            base: ptr as *const u8,
            size: code_bytes.len(),
        })
    }
}

#[cfg(not(unix))]
fn load_executable_code(_code_bytes: &[u8]) -> Option<AotCodeRegion> {
    // TODO: Windows VirtualAlloc implementation
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_no_bundle() {
        // A non-bundle file should return None
        let temp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp.path(), b"not a bundle").unwrap();
        assert!(detect_bundle_at(temp.path()).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn test_load_executable_code() {
        // Load some dummy bytes into executable memory
        let code = vec![0xC3u8; 64]; // x86 RET instruction repeated
        let region = load_executable_code(&code).unwrap();
        assert_eq!(region.size(), 64);
    }

    #[cfg(unix)]
    #[test]
    fn test_load_empty_code() {
        let region = load_executable_code(&[]).unwrap();
        assert_eq!(region.size(), 0);
    }
}
