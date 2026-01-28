//! Constant pool for bytecode modules

use super::encoder::{BytecodeReader, BytecodeWriter, DecodeError};

/// Constant pool containing literal values
#[derive(Debug, Clone, Default)]
pub struct ConstantPool {
    /// String constants
    pub strings: Vec<String>,
    /// Integer constants
    pub integers: Vec<i32>,
    /// Float constants
    pub floats: Vec<f64>,
}

impl ConstantPool {
    /// Create a new empty constant pool
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a string constant and return its index
    pub fn add_string(&mut self, s: String) -> u32 {
        let index = self.strings.len();
        self.strings.push(s);
        index as u32
    }

    /// Add an integer constant and return its index
    pub fn add_integer(&mut self, i: i32) -> u32 {
        let index = self.integers.len();
        self.integers.push(i);
        index as u32
    }

    /// Add a float constant and return its index
    pub fn add_float(&mut self, f: f64) -> u32 {
        let index = self.floats.len();
        self.floats.push(f);
        index as u32
    }

    /// Get a string constant by index
    pub fn get_string(&self, index: u32) -> Option<&str> {
        self.strings.get(index as usize).map(|s| s.as_str())
    }

    /// Get an integer constant by index
    pub fn get_integer(&self, index: u32) -> Option<i32> {
        self.integers.get(index as usize).copied()
    }

    /// Get a float constant by index
    pub fn get_float(&self, index: u32) -> Option<f64> {
        self.floats.get(index as usize).copied()
    }

    /// Encode the constant pool to binary format
    ///
    /// Format:
    /// - String count (u32)
    /// - For each string: length (u32) + UTF-8 bytes
    /// - Integer count (u32)
    /// - For each integer: i32
    /// - Float count (u32)
    /// - For each float: f64
    pub fn encode(&self, writer: &mut BytecodeWriter) {
        // Encode strings
        writer.emit_u32(self.strings.len() as u32);
        for s in &self.strings {
            writer.emit_u32(s.len() as u32);
            writer.buffer.extend_from_slice(s.as_bytes());
        }

        // Encode integers
        writer.emit_u32(self.integers.len() as u32);
        for &i in &self.integers {
            writer.emit_i32(i);
        }

        // Encode floats
        writer.emit_u32(self.floats.len() as u32);
        for &f in &self.floats {
            writer.emit_f64(f);
        }
    }

    /// Decode the constant pool from binary format
    pub fn decode(reader: &mut BytecodeReader<'_>) -> Result<Self, DecodeError> {
        let mut pool = ConstantPool::new();

        // Decode strings
        let string_count = reader.read_u32()? as usize;
        pool.strings.reserve(string_count);
        for _ in 0..string_count {
            let s = reader.read_string()?;
            pool.strings.push(s);
        }

        // Decode integers
        let int_count = reader.read_u32()? as usize;
        pool.integers.reserve(int_count);
        for _ in 0..int_count {
            pool.integers.push(reader.read_i32()?);
        }

        // Decode floats
        let float_count = reader.read_u32()? as usize;
        pool.floats.reserve(float_count);
        for _ in 0..float_count {
            pool.floats.push(reader.read_f64()?);
        }

        Ok(pool)
    }

    /// Calculate the size in bytes when encoded
    pub fn encoded_size(&self) -> usize {
        let mut size = 0;

        // String count + strings
        size += 4; // count
        for s in &self.strings {
            size += 4; // length prefix
            size += s.len(); // UTF-8 bytes
        }

        // Integer count + integers
        size += 4; // count
        size += self.integers.len() * 4; // i32 values

        // Float count + floats
        size += 4; // count
        size += self.floats.len() * 8; // f64 values

        size
    }
}

#[cfg(test)]
#[allow(clippy::approx_constant)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_pool() {
        let mut pool = ConstantPool::new();

        let str_idx = pool.add_string("hello".to_string());
        let int_idx = pool.add_integer(42);
        let float_idx = pool.add_float(3.14);

        assert_eq!(pool.get_string(str_idx), Some("hello"));
        assert_eq!(pool.get_integer(int_idx), Some(42));
        assert_eq!(pool.get_float(float_idx), Some(3.14));
    }

    #[test]
    fn test_constant_pool_encoding() {
        let mut pool = ConstantPool::new();
        pool.add_string("hello".to_string());
        pool.add_string("world".to_string());
        pool.add_integer(42);
        pool.add_integer(-17);
        pool.add_float(3.14);
        pool.add_float(2.718);

        let mut writer = BytecodeWriter::new();
        pool.encode(&mut writer);

        let bytes = writer.buffer();
        let mut reader = BytecodeReader::new(bytes);

        let decoded = ConstantPool::decode(&mut reader).unwrap();

        assert_eq!(decoded.strings.len(), 2);
        assert_eq!(decoded.strings[0], "hello");
        assert_eq!(decoded.strings[1], "world");
        assert_eq!(decoded.integers.len(), 2);
        assert_eq!(decoded.integers[0], 42);
        assert_eq!(decoded.integers[1], -17);
        assert_eq!(decoded.floats.len(), 2);
        assert!((decoded.floats[0] - 3.14).abs() < 0.001);
        assert!((decoded.floats[1] - 2.718).abs() < 0.001);
    }

    #[test]
    fn test_empty_constant_pool_encoding() {
        let pool = ConstantPool::new();

        let mut writer = BytecodeWriter::new();
        pool.encode(&mut writer);

        let bytes = writer.buffer();
        let mut reader = BytecodeReader::new(bytes);

        let decoded = ConstantPool::decode(&mut reader).unwrap();

        assert_eq!(decoded.strings.len(), 0);
        assert_eq!(decoded.integers.len(), 0);
        assert_eq!(decoded.floats.len(), 0);
    }

    #[test]
    fn test_constant_pool_encoded_size() {
        let mut pool = ConstantPool::new();
        pool.add_string("hello".to_string()); // 4 (count) + 4 (len) + 5 (bytes) = 9
        pool.add_integer(42); // 4 (count) + 4 (value) = 8
        pool.add_float(3.14); // 4 (count) + 8 (value) = 12

        // Total: 9 + 8 + 12 = 29 bytes
        let expected_size = 4 + 4 + 5 + 4 + 4 + 4 + 8;
        assert_eq!(pool.encoded_size(), expected_size);

        // Verify by actually encoding
        let mut writer = BytecodeWriter::new();
        pool.encode(&mut writer);
        assert_eq!(writer.offset(), expected_size);
    }

    #[test]
    fn test_constant_pool_unicode() {
        let mut pool = ConstantPool::new();
        pool.add_string("こんにちは".to_string()); // Japanese "hello"
        pool.add_string("🎉".to_string()); // Emoji

        let mut writer = BytecodeWriter::new();
        pool.encode(&mut writer);

        let bytes = writer.buffer();
        let mut reader = BytecodeReader::new(bytes);

        let decoded = ConstantPool::decode(&mut reader).unwrap();

        assert_eq!(decoded.strings[0], "こんにちは");
        assert_eq!(decoded.strings[1], "🎉");
    }
}
