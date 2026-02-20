//! Binary payload format
//!
//! Defines the structure of the AOT payload appended to the raya binary.
//!
//! ```text
//! ┌─────────────────────────┐
//! │  raya binary (unchanged)│  ← normal raya executable
//! ├─────────────────────────┤
//! │  Code Section           │  ← raw machine code (contiguous blob)
//! ├─────────────────────────┤
//! │  Function Table         │  ← array of FuncTableEntry
//! ├─────────────────────────┤
//! │  VFS Section            │  ← embedded asset files
//! ├─────────────────────────┤
//! │  Trailer                │  ← fixed-size, at very end of file
//! └─────────────────────────┘
//! ```

use std::io::{self, Write};

/// Magic bytes identifying an AOT bundle trailer.
pub const TRAILER_MAGIC: [u8; 8] = *b"RAYAAOT\0";

/// Size of the AOT trailer in bytes.
pub const TRAILER_SIZE: usize = std::mem::size_of::<AotTrailer>();

/// Fixed-size trailer at the very end of the bundled binary.
///
/// To detect if the current executable is a bundle, read the last
/// `TRAILER_SIZE` bytes and check if magic matches.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct AotTrailer {
    /// Magic bytes: b"RAYAAOT\0"
    pub magic: [u8; 8],

    /// Byte offset of the code section from start of payload.
    pub code_offset: u64,

    /// Size of the code section in bytes.
    pub code_size: u64,

    /// Byte offset of the function table from start of payload.
    pub func_table_offset: u64,

    /// Number of entries in the function table.
    pub func_table_count: u32,

    /// Byte offset of the VFS section from start of payload.
    pub vfs_offset: u64,

    /// Size of the VFS section in bytes.
    pub vfs_size: u64,

    /// Target triple (null-terminated, zero-padded).
    pub target_triple: [u8; 64],

    /// CRC32 checksum of the entire payload (code + func table + vfs).
    pub checksum: u32,

    /// Size of this trailer struct (for forward compatibility).
    pub trailer_size: u32,

    /// Offset from the start of the file to the start of the payload.
    /// This equals the size of the original raya binary.
    pub payload_offset: u64,
}

/// Entry in the function table within the bundle.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct BundledFuncEntry {
    /// Global function ID (module_index << 16 | func_index).
    pub global_func_id: u32,

    /// Byte offset within the code section.
    pub code_offset: u64,

    /// Size of this function's code in bytes.
    pub code_size: u64,

    /// Number of local variables (for frame allocation).
    pub local_count: u32,

    /// Number of parameters.
    pub param_count: u32,
}

/// Size of a single BundledFuncEntry in bytes.
pub const FUNC_ENTRY_SIZE: usize = std::mem::size_of::<BundledFuncEntry>();

/// Entry in the VFS section.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct VfsFileHeader {
    /// Length of the file path (UTF-8 bytes).
    pub path_len: u32,

    /// Size of the file data in bytes.
    pub data_size: u64,
}

impl AotTrailer {
    /// Check if this trailer has the correct magic bytes.
    pub fn is_valid(&self) -> bool {
        self.magic == TRAILER_MAGIC
    }

    /// Read a trailer from raw bytes (must be exactly TRAILER_SIZE bytes).
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < TRAILER_SIZE {
            return None;
        }

        // Safety: AotTrailer is repr(C, packed) with no padding
        let trailer = unsafe {
            std::ptr::read_unaligned(bytes.as_ptr() as *const AotTrailer)
        };

        if trailer.is_valid() {
            Some(trailer)
        } else {
            None
        }
    }

    /// Write the trailer to a byte buffer.
    pub fn to_bytes(&self) -> Vec<u8> {
        let size = TRAILER_SIZE;
        let mut bytes = vec![0u8; size];
        unsafe {
            std::ptr::write_unaligned(bytes.as_mut_ptr() as *mut AotTrailer, *self);
        }
        bytes
    }

    /// Write the trailer to a writer.
    pub fn write_to<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&self.to_bytes())
    }

    /// Encode a target triple string into the fixed-size buffer.
    pub fn encode_target_triple(triple: &str) -> [u8; 64] {
        let mut buf = [0u8; 64];
        let bytes = triple.as_bytes();
        let len = bytes.len().min(63); // leave room for null terminator
        buf[..len].copy_from_slice(&bytes[..len]);
        buf
    }

    /// Decode the target triple from the fixed-size buffer.
    pub fn decode_target_triple(&self) -> &str {
        let end = self.target_triple.iter().position(|&b| b == 0).unwrap_or(64);
        std::str::from_utf8(&self.target_triple[..end]).unwrap_or("unknown")
    }
}

impl BundledFuncEntry {
    /// Read a function table entry from raw bytes.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < FUNC_ENTRY_SIZE {
            return None;
        }
        Some(unsafe {
            std::ptr::read_unaligned(bytes.as_ptr() as *const BundledFuncEntry)
        })
    }

    /// Write to a byte buffer.
    pub fn to_bytes(&self) -> Vec<u8> {
        let size = FUNC_ENTRY_SIZE;
        let mut bytes = vec![0u8; size];
        unsafe {
            std::ptr::write_unaligned(bytes.as_mut_ptr() as *mut BundledFuncEntry, *self);
        }
        bytes
    }
}

/// Write the VFS section to a writer.
///
/// Format: for each file:
///   [VfsFileHeader][path_bytes][file_data]
pub fn write_vfs_section<W: Write>(
    writer: &mut W,
    files: &[(String, Vec<u8>)],
) -> io::Result<u64> {
    let mut total = 0u64;

    for (path, data) in files {
        let header = VfsFileHeader {
            path_len: path.len() as u32,
            data_size: data.len() as u64,
        };

        let header_bytes = unsafe {
            let size = std::mem::size_of::<VfsFileHeader>();
            let mut bytes = vec![0u8; size];
            std::ptr::write_unaligned(bytes.as_mut_ptr() as *mut VfsFileHeader, header);
            bytes
        };

        writer.write_all(&header_bytes)?;
        writer.write_all(path.as_bytes())?;
        writer.write_all(data)?;

        total += header_bytes.len() as u64 + path.len() as u64 + data.len() as u64;
    }

    Ok(total)
}

/// Read the VFS section from a byte slice.
///
/// Returns a list of (path, data) pairs.
pub fn read_vfs_section(data: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut result = Vec::new();
    let mut offset = 0;
    let header_size = std::mem::size_of::<VfsFileHeader>();

    while offset + header_size <= data.len() {
        let header = unsafe {
            std::ptr::read_unaligned(data[offset..].as_ptr() as *const VfsFileHeader)
        };
        offset += header_size;

        let path_len = header.path_len as usize;
        let data_size = header.data_size as usize;

        if offset + path_len + data_size > data.len() {
            break;
        }

        let path = String::from_utf8_lossy(&data[offset..offset + path_len]).to_string();
        offset += path_len;

        let file_data = data[offset..offset + data_size].to_vec();
        offset += data_size;

        result.push((path, file_data));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trailer_magic() {
        assert_eq!(TRAILER_MAGIC, *b"RAYAAOT\0");
    }

    #[test]
    fn test_trailer_roundtrip() {
        let trailer = AotTrailer {
            magic: TRAILER_MAGIC,
            code_offset: 0,
            code_size: 1024,
            func_table_offset: 1024,
            func_table_count: 5,
            vfs_offset: 2048,
            vfs_size: 512,
            target_triple: AotTrailer::encode_target_triple("aarch64-apple-darwin"),
            checksum: 0xDEADBEEF,
            trailer_size: TRAILER_SIZE as u32,
            payload_offset: 4096,
        };

        let bytes = trailer.to_bytes();
        let restored = AotTrailer::from_bytes(&bytes).unwrap();

        assert!(restored.is_valid());
        // Copy packed fields to locals to avoid unaligned references
        let code_size = restored.code_size;
        let func_table_count = restored.func_table_count;
        let checksum = restored.checksum;
        assert_eq!(code_size, 1024);
        assert_eq!(func_table_count, 5);
        assert_eq!(checksum, 0xDEADBEEF);
        assert_eq!(restored.decode_target_triple(), "aarch64-apple-darwin");
    }

    #[test]
    fn test_trailer_invalid_magic() {
        let bytes = vec![0u8; TRAILER_SIZE];
        assert!(AotTrailer::from_bytes(&bytes).is_none());
    }

    #[test]
    fn test_func_entry_roundtrip() {
        let entry = BundledFuncEntry {
            global_func_id: (3 << 16) | 42,
            code_offset: 128,
            code_size: 256,
            local_count: 8,
            param_count: 2,
        };

        let bytes = entry.to_bytes();
        let restored = BundledFuncEntry::from_bytes(&bytes).unwrap();

        // Copy packed fields to locals to avoid unaligned references
        let global_func_id = restored.global_func_id;
        let code_offset = restored.code_offset;
        let code_size = restored.code_size;
        let local_count = restored.local_count;
        let param_count = restored.param_count;
        assert_eq!(global_func_id, (3 << 16) | 42);
        assert_eq!(code_offset, 128);
        assert_eq!(code_size, 256);
        assert_eq!(local_count, 8);
        assert_eq!(param_count, 2);
    }

    #[test]
    fn test_vfs_roundtrip() {
        let files = vec![
            ("config.json".to_string(), b"{\"key\": \"value\"}".to_vec()),
            ("assets/logo.png".to_string(), vec![0x89, 0x50, 0x4E, 0x47]),
        ];

        let mut buffer = Vec::new();
        write_vfs_section(&mut buffer, &files).unwrap();

        let restored = read_vfs_section(&buffer);
        assert_eq!(restored.len(), 2);
        assert_eq!(restored[0].0, "config.json");
        assert_eq!(restored[0].1, b"{\"key\": \"value\"}");
        assert_eq!(restored[1].0, "assets/logo.png");
        assert_eq!(restored[1].1, vec![0x89, 0x50, 0x4E, 0x47]);
    }

    #[test]
    fn test_target_triple_encoding() {
        let buf = AotTrailer::encode_target_triple("x86_64-unknown-linux-gnu");
        let trailer = AotTrailer {
            magic: TRAILER_MAGIC,
            target_triple: buf,
            code_offset: 0,
            code_size: 0,
            func_table_offset: 0,
            func_table_count: 0,
            vfs_offset: 0,
            vfs_size: 0,
            checksum: 0,
            trailer_size: TRAILER_SIZE as u32,
            payload_offset: 0,
        };
        assert_eq!(trailer.decode_target_triple(), "x86_64-unknown-linux-gnu");
    }
}
