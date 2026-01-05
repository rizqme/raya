//! Snapshot binary format definitions

use sha2::{Digest, Sha256};
use std::io::{Read, Write};

/// Magic number for Raya snapshots: "RAYA\0\0\0\0"
pub const SNAPSHOT_MAGIC: u64 = 0x0000005941594152;

/// Current snapshot format version
pub const SNAPSHOT_VERSION: u32 = 1;

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
            endianness: 0x01020304,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            checksum_offset: 0,
            reserved: 0,
        }
    }

    /// Validate snapshot header
    pub fn validate(&self) -> Result<(), SnapshotError> {
        if self.magic != SNAPSHOT_MAGIC {
            return Err(SnapshotError::InvalidMagic);
        }

        if self.version != SNAPSHOT_VERSION {
            return Err(SnapshotError::IncompatibleVersion {
                expected: SNAPSHOT_VERSION,
                actual: self.version,
            });
        }

        if self.endianness != 0x01020304 {
            return Err(SnapshotError::EndiannessMismatch);
        }

        Ok(())
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
        assert_eq!(decoded.endianness, 0x01020304);
    }

    #[test]
    fn test_header_validation() {
        let header = SnapshotHeader::new();
        assert!(header.validate().is_ok());

        let mut invalid = header.clone();
        invalid.magic = 0;
        assert!(matches!(
            invalid.validate(),
            Err(SnapshotError::InvalidMagic)
        ));
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
