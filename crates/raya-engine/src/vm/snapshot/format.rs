//! Snapshot binary format definitions
//!
//! # Endianness Handling
//!
//! Raya snapshots use **little-endian encoding** as the canonical format for portability.
//! This ensures snapshots can be transferred between systems regardless of their native
//! byte order. All multi-byte integers are encoded using little-endian byte order.
//!
//! The snapshot header includes an endianness marker (0x01020304) which serves two purposes:
//! 1. Detect if the snapshot was created on a system with different endianness
//! 2. Verify the snapshot format is valid
//!
//! When reading a snapshot:
//! - If the marker reads as 0x01020304, the snapshot is in little-endian (correct)
//! - If the marker reads as 0x04030201, the snapshot is in big-endian (byte-swap needed)
//! - Any other value indicates a corrupted snapshot
//!
//! # Platform Support
//!
//! While Raya's canonical format is little-endian, the code includes endianness detection
//! to provide clear error messages on big-endian systems. Future versions may add automatic
//! byte-swapping for full big-endian support.

use sha2::{Digest, Sha256};
use std::io::{Read, Write};

/// Magic number for Raya snapshots: "RAYA\0\0\0\0" (little-endian)
pub const SNAPSHOT_MAGIC: u64 = 0x0000005941594152;

/// Current snapshot format version
pub const SNAPSHOT_VERSION: u32 = 1;

/// Endianness marker - should read as 0x01020304 in native byte order
pub const ENDIANNESS_MARKER: u32 = 0x01020304;

/// Endianness marker when byte-swapped (indicates different endianness)
pub const ENDIANNESS_MARKER_SWAPPED: u32 = 0x04030201;

/// Detect the system's native byte order
#[inline]
pub const fn is_little_endian() -> bool {
    // Check native byte order using const evaluation
    cfg!(target_endian = "little")
}

/// Detect the system's native byte order
#[inline]
pub const fn is_big_endian() -> bool {
    cfg!(target_endian = "big")
}

/// Check if byte-swapping is needed for a snapshot
///
/// # Arguments
/// * `marker` - The endianness marker read from the snapshot
///
/// # Returns
/// * `Ok(true)` - Byte-swapping is needed (different endianness)
/// * `Ok(false)` - No byte-swapping needed (same endianness)
/// * `Err(())` - Invalid marker (corrupted snapshot)
#[allow(clippy::result_unit_err)] // () error is sufficient: only indicates a corrupted marker with no extra context.
pub fn needs_byte_swap(marker: u32) -> Result<bool, ()> {
    match marker {
        ENDIANNESS_MARKER => Ok(false),        // Same endianness
        ENDIANNESS_MARKER_SWAPPED => Ok(true), // Different endianness
        _ => Err(()),                          // Corrupted
    }
}

/// Byte-swap utilities for endianness conversion
pub mod byteswap {
    /// Conditionally byte-swap a u16 value
    #[inline]
    pub fn swap_u16(value: u16, should_swap: bool) -> u16 {
        if should_swap {
            value.swap_bytes()
        } else {
            value
        }
    }

    /// Conditionally byte-swap a u32 value
    #[inline]
    pub fn swap_u32(value: u32, should_swap: bool) -> u32 {
        if should_swap {
            value.swap_bytes()
        } else {
            value
        }
    }

    /// Conditionally byte-swap a u64 value
    #[inline]
    pub fn swap_u64(value: u64, should_swap: bool) -> u64 {
        if should_swap {
            value.swap_bytes()
        } else {
            value
        }
    }

    /// Conditionally byte-swap an i32 value
    #[inline]
    pub fn swap_i32(value: i32, should_swap: bool) -> i32 {
        if should_swap {
            value.swap_bytes()
        } else {
            value
        }
    }

    /// Conditionally byte-swap an i64 value
    #[inline]
    pub fn swap_i64(value: i64, should_swap: bool) -> i64 {
        if should_swap {
            value.swap_bytes()
        } else {
            value
        }
    }

    /// Conditionally byte-swap an f64 value
    #[inline]
    pub fn swap_f64(value: f64, should_swap: bool) -> f64 {
        if should_swap {
            f64::from_bits(value.to_bits().swap_bytes())
        } else {
            value
        }
    }

    /// Conditionally byte-swap a usize value (64-bit on most platforms)
    #[inline]
    pub fn swap_usize(value: usize, should_swap: bool) -> usize {
        if should_swap {
            value.swap_bytes()
        } else {
            value
        }
    }
}

/// Snapshot header (32 bytes)
#[repr(C)]
#[derive(Debug, Clone)]
pub struct SnapshotHeader {
    /// Magic number (must be SNAPSHOT_MAGIC)
    pub magic: u64,

    /// Snapshot format version
    pub version: u32,

    /// Flags (compression, encryption, etc.)
    pub flags: u32,

    /// Endianness marker (0x01020304)
    pub endianness: u32,

    /// Timestamp when snapshot was created (Unix epoch millis)
    pub timestamp: u64,

    /// Offset to checksum in file
    pub checksum_offset: u32,

    /// Reserved for future use
    pub reserved: u32,
}

impl SnapshotHeader {
    /// Create a new snapshot header with current timestamp
    pub fn new() -> Self {
        Self {
            magic: SNAPSHOT_MAGIC,
            version: SNAPSHOT_VERSION,
            flags: 0,
            endianness: ENDIANNESS_MARKER,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            checksum_offset: 0,
            reserved: 0,
        }
    }

    /// Validate snapshot header and detect endianness
    ///
    /// Returns `Ok(true)` if byte-swapping is needed, `Ok(false)` otherwise
    pub fn validate(&self) -> Result<bool, SnapshotError> {
        if self.magic != SNAPSHOT_MAGIC {
            return Err(SnapshotError::InvalidMagic);
        }

        if self.version != SNAPSHOT_VERSION {
            return Err(SnapshotError::IncompatibleVersion {
                expected: SNAPSHOT_VERSION,
                actual: self.version,
            });
        }

        // Check endianness marker
        match needs_byte_swap(self.endianness) {
            Ok(needs_swap) => {
                // Return whether byte-swapping is needed
                // The reader will handle byte-swapping automatically
                Ok(needs_swap)
            }
            Err(()) => {
                // Invalid endianness marker - snapshot is corrupted
                Err(SnapshotError::CorruptedData)
            }
        }
    }

    /// Get system endianness as a string (for debugging)
    pub fn system_endianness() -> &'static str {
        if is_little_endian() {
            "little-endian"
        } else if is_big_endian() {
            "big-endian"
        } else {
            "unknown"
        }
    }

    /// Encode header to writer in little-endian format
    pub fn encode(&self, writer: &mut impl Write) -> std::io::Result<()> {
        writer.write_all(&self.magic.to_le_bytes())?;
        writer.write_all(&self.version.to_le_bytes())?;
        writer.write_all(&self.flags.to_le_bytes())?;
        writer.write_all(&self.endianness.to_le_bytes())?;
        writer.write_all(&self.timestamp.to_le_bytes())?;
        writer.write_all(&self.checksum_offset.to_le_bytes())?;
        writer.write_all(&self.reserved.to_le_bytes())?;
        Ok(())
    }

    /// Decode header from reader
    pub fn decode(reader: &mut impl Read) -> std::io::Result<Self> {
        let mut buf = [0u8; 8];
        reader.read_exact(&mut buf)?;
        let magic = u64::from_le_bytes(buf);

        let mut buf = [0u8; 4];
        reader.read_exact(&mut buf)?;
        let version = u32::from_le_bytes(buf);

        reader.read_exact(&mut buf)?;
        let flags = u32::from_le_bytes(buf);

        reader.read_exact(&mut buf)?;
        let endianness = u32::from_le_bytes(buf);

        let mut buf = [0u8; 8];
        reader.read_exact(&mut buf)?;
        let timestamp = u64::from_le_bytes(buf);

        let mut buf = [0u8; 4];
        reader.read_exact(&mut buf)?;
        let checksum_offset = u32::from_le_bytes(buf);

        reader.read_exact(&mut buf)?;
        let reserved = u32::from_le_bytes(buf);

        Ok(Self {
            magic,
            version,
            flags,
            endianness,
            timestamp,
            checksum_offset,
            reserved,
        })
    }
}

impl Default for SnapshotHeader {
    fn default() -> Self {
        Self::new()
    }
}

/// Segment type identifier
#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SegmentType {
    /// Metadata segment containing snapshot information
    Metadata = 1,
    /// Heap segment containing allocated objects
    Heap = 2,
    /// Task segment containing task state
    Task = 3,
    /// Scheduler segment containing scheduler state
    Scheduler = 4,
    /// Sync segment containing synchronization primitives
    Sync = 5,
}

/// Segment header
#[repr(C)]
#[derive(Debug, Clone)]
pub struct SegmentHeader {
    /// Segment type identifier
    pub segment_type: u8,
    /// Segment flags
    pub flags: u8,
    /// Reserved for future use
    pub reserved: u16,
    /// Length of segment data in bytes
    pub length: u64,
}

impl SegmentHeader {
    /// Create a new segment header
    pub fn new(segment_type: SegmentType, length: u64) -> Self {
        Self {
            segment_type: segment_type as u8,
            flags: 0,
            reserved: 0,
            length,
        }
    }

    /// Encode segment header to writer
    pub fn encode(&self, writer: &mut impl Write) -> std::io::Result<()> {
        writer.write_all(&[self.segment_type])?;
        writer.write_all(&[self.flags])?;
        writer.write_all(&self.reserved.to_le_bytes())?;
        writer.write_all(&self.length.to_le_bytes())?;
        Ok(())
    }

    /// Decode segment header from reader
    pub fn decode(reader: &mut impl Read) -> std::io::Result<Self> {
        let mut buf = [0u8; 1];
        reader.read_exact(&mut buf)?;
        let segment_type = buf[0];

        reader.read_exact(&mut buf)?;
        let flags = buf[0];

        let mut buf = [0u8; 2];
        reader.read_exact(&mut buf)?;
        let reserved = u16::from_le_bytes(buf);

        let mut buf = [0u8; 8];
        reader.read_exact(&mut buf)?;
        let length = u64::from_le_bytes(buf);

        Ok(Self {
            segment_type,
            flags,
            reserved,
            length,
        })
    }
}

/// Checksum for snapshot integrity
#[derive(Debug, Clone)]
pub struct SnapshotChecksum {
    hash: [u8; 32], // SHA-256
}

impl SnapshotChecksum {
    /// Compute SHA-256 checksum of data
    pub fn compute(data: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let result = hasher.finalize();

        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);

        Self { hash }
    }

    /// Verify that checksum matches the given data
    pub fn verify(&self, data: &[u8]) -> bool {
        let computed = Self::compute(data);
        self.hash == computed.hash
    }

    /// Encode checksum to writer
    pub fn encode(&self, writer: &mut impl Write) -> std::io::Result<()> {
        writer.write_all(&self.hash)
    }

    /// Decode checksum from reader
    pub fn decode(reader: &mut impl Read) -> std::io::Result<Self> {
        let mut hash = [0u8; 32];
        reader.read_exact(&mut hash)?;
        Ok(Self { hash })
    }
}

/// Snapshot error types
#[derive(Debug)]
pub enum SnapshotError {
    /// Invalid magic number in snapshot header
    InvalidMagic,
    /// Incompatible snapshot version
    IncompatibleVersion {
        /// Expected version
        expected: u32,
        /// Actual version found
        actual: u32,
    },
    /// Endianness mismatch between snapshot and current system
    EndiannessMismatch,
    /// Checksum verification failed
    ChecksumMismatch,
    /// Corrupted snapshot data
    CorruptedData,
    /// I/O error during snapshot read/write
    IoError(std::io::Error),
}

impl From<std::io::Error> for SnapshotError {
    fn from(e: std::io::Error) -> Self {
        SnapshotError::IoError(e)
    }
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotError::InvalidMagic => write!(f, "Invalid snapshot magic number"),
            SnapshotError::IncompatibleVersion { expected, actual } => {
                write!(
                    f,
                    "Incompatible snapshot version (expected {}, got {})",
                    expected, actual
                )
            }
            SnapshotError::EndiannessMismatch => write!(f, "Endianness mismatch"),
            SnapshotError::ChecksumMismatch => write!(f, "Checksum verification failed"),
            SnapshotError::CorruptedData => write!(f, "Corrupted snapshot data"),
            SnapshotError::IoError(e) => write!(f, "I/O error: {}", e),
        }
    }
}

impl std::error::Error for SnapshotError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_encode_decode() {
        let header = SnapshotHeader::new();
        let mut buf = Vec::new();
        header.encode(&mut buf).unwrap();

        let decoded = SnapshotHeader::decode(&mut &buf[..]).unwrap();
        assert_eq!(decoded.magic, SNAPSHOT_MAGIC);
        assert_eq!(decoded.version, SNAPSHOT_VERSION);
        assert_eq!(decoded.endianness, ENDIANNESS_MARKER);
    }

    #[test]
    fn test_header_validation() {
        let header = SnapshotHeader::new();
        let result = header.validate();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), false); // No byte-swapping needed

        let mut invalid = header.clone();
        invalid.magic = 0;
        assert!(matches!(
            invalid.validate(),
            Err(SnapshotError::InvalidMagic)
        ));

        // Test invalid endianness marker (corrupted)
        let mut corrupted = header.clone();
        corrupted.endianness = 0xDEADBEEF;
        assert!(matches!(
            corrupted.validate(),
            Err(SnapshotError::CorruptedData)
        ));
    }

    #[test]
    fn test_endianness_detection() {
        // Test system endianness detection
        let is_le = is_little_endian();
        let is_be = is_big_endian();

        // Exactly one should be true
        assert!(is_le ^ is_be);

        // Get endianness string
        let endian_str = SnapshotHeader::system_endianness();
        if is_le {
            assert_eq!(endian_str, "little-endian");
        } else if is_be {
            assert_eq!(endian_str, "big-endian");
        }
    }

    #[test]
    fn test_needs_byte_swap() {
        // Test endianness marker detection
        assert_eq!(needs_byte_swap(ENDIANNESS_MARKER), Ok(false));
        assert_eq!(needs_byte_swap(ENDIANNESS_MARKER_SWAPPED), Ok(true));
        assert_eq!(needs_byte_swap(0xDEADBEEF), Err(()));
    }

    #[test]
    fn test_endianness_marker_values() {
        // Verify marker constants are correct
        assert_eq!(ENDIANNESS_MARKER, 0x01020304);
        assert_eq!(ENDIANNESS_MARKER_SWAPPED, 0x04030201);

        // Verify they are byte-swapped versions of each other
        assert_eq!(ENDIANNESS_MARKER.swap_bytes(), ENDIANNESS_MARKER_SWAPPED);
        assert_eq!(ENDIANNESS_MARKER_SWAPPED.swap_bytes(), ENDIANNESS_MARKER);
    }

    #[test]
    fn test_segment_header_encode_decode() {
        let seg = SegmentHeader::new(SegmentType::Heap, 1024);
        let mut buf = Vec::new();
        seg.encode(&mut buf).unwrap();

        let decoded = SegmentHeader::decode(&mut &buf[..]).unwrap();
        assert_eq!(decoded.segment_type, SegmentType::Heap as u8);
        assert_eq!(decoded.length, 1024);
    }

    #[test]
    fn test_checksum_compute_verify() {
        let data = b"test data for checksum";
        let checksum = SnapshotChecksum::compute(data);
        assert!(checksum.verify(data));
        assert!(!checksum.verify(b"different data"));
    }

    #[test]
    fn test_checksum_encode_decode() {
        let data = b"test data";
        let checksum = SnapshotChecksum::compute(data);

        let mut buf = Vec::new();
        checksum.encode(&mut buf).unwrap();

        let decoded = SnapshotChecksum::decode(&mut &buf[..]).unwrap();
        assert!(decoded.verify(data));
    }
}
