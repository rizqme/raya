//! Bytecode module format

use crate::constants::ConstantPool;
use crate::encoder::{BytecodeReader, BytecodeWriter, DecodeError};
use thiserror::Error;

/// Magic number for Raya bytecode files: "RAYA"
pub const MAGIC: [u8; 4] = *b"RAYA";

/// Current bytecode version
pub const VERSION: u32 = 1;

/// Module encoding/decoding errors
#[derive(Debug, Error)]
pub enum ModuleError {
    /// Decode error
    #[error("Decode error: {0}")]
    DecodeError(#[from] DecodeError),

    /// Invalid magic number
    #[error("Invalid magic number: expected RAYA, got {0:?}")]
    InvalidMagic([u8; 4]),

    /// Unsupported version
    #[error("Unsupported version: {0} (current: {VERSION})")]
    UnsupportedVersion(u32),

    /// Checksum mismatch
    #[error("Checksum mismatch: expected {expected:#x}, got {actual:#x}")]
    ChecksumMismatch { expected: u32, actual: u32 },
}

/// A compiled Raya module
#[derive(Debug, Clone)]
pub struct Module {
    /// Magic number (must be "RAYA")
    pub magic: [u8; 4],
    /// Bytecode version
    pub version: u32,
    /// Module flags
    pub flags: u32,
    /// Constant pool
    pub constants: ConstantPool,
    /// Function definitions
    pub functions: Vec<Function>,
    /// Class definitions
    pub classes: Vec<ClassDef>,
    /// Module metadata
    pub metadata: Metadata,
}

/// Module flags
pub mod flags {
    /// Module has debug information
    pub const HAS_DEBUG_INFO: u32 = 1 << 0;
    /// Module has reflection data
    pub const HAS_REFLECTION: u32 = 1 << 1;
}

/// Function definition
#[derive(Debug, Clone)]
pub struct Function {
    /// Function name
    pub name: String,
    /// Number of parameters
    pub param_count: usize,
    /// Number of local variables
    pub local_count: usize,
    /// Bytecode instructions
    pub code: Vec<u8>,
}

impl Function {
    /// Encode function to binary
    fn encode(&self, writer: &mut BytecodeWriter) {
        // Write name length and name
        writer.emit_u32(self.name.len() as u32);
        writer.buffer.extend_from_slice(self.name.as_bytes());

        // Write counts
        writer.emit_u32(self.param_count as u32);
        writer.emit_u32(self.local_count as u32);

        // Write code length and code
        writer.emit_u32(self.code.len() as u32);
        writer.buffer.extend_from_slice(&self.code);
    }

    /// Decode function from binary
    fn decode(reader: &mut BytecodeReader) -> Result<Self, DecodeError> {
        // Read name
        let name = reader.read_string()?;

        // Read counts
        let param_count = reader.read_u32()? as usize;
        let local_count = reader.read_u32()? as usize;

        // Read code
        let code_len = reader.read_u32()? as usize;
        let code = reader.read_bytes(code_len)?;

        Ok(Self {
            name,
            param_count,
            local_count,
            code,
        })
    }
}

/// Class definition
#[derive(Debug, Clone)]
pub struct ClassDef {
    /// Class name
    pub name: String,
    /// Number of fields
    pub field_count: usize,
    /// Method definitions
    pub methods: Vec<Method>,
}

impl ClassDef {
    /// Encode class definition to binary
    fn encode(&self, writer: &mut BytecodeWriter) {
        // Write name
        writer.emit_u32(self.name.len() as u32);
        writer.buffer.extend_from_slice(self.name.as_bytes());

        // Write field count
        writer.emit_u32(self.field_count as u32);

        // Write methods
        writer.emit_u32(self.methods.len() as u32);
        for method in &self.methods {
            method.encode(writer);
        }
    }

    /// Decode class definition from binary
    fn decode(reader: &mut BytecodeReader) -> Result<Self, DecodeError> {
        // Read name
        let name = reader.read_string()?;

        // Read field count
        let field_count = reader.read_u32()? as usize;

        // Read methods
        let method_count = reader.read_u32()? as usize;
        let mut methods = Vec::with_capacity(method_count);
        for _ in 0..method_count {
            methods.push(Method::decode(reader)?);
        }

        Ok(Self {
            name,
            field_count,
            methods,
        })
    }
}

/// Method definition
#[derive(Debug, Clone)]
pub struct Method {
    /// Method name
    pub name: String,
    /// Function ID in the module
    pub function_id: usize,
}

impl Method {
    /// Encode method to binary
    fn encode(&self, writer: &mut BytecodeWriter) {
        // Write name
        writer.emit_u32(self.name.len() as u32);
        writer.buffer.extend_from_slice(self.name.as_bytes());

        // Write function ID
        writer.emit_u32(self.function_id as u32);
    }

    /// Decode method from binary
    fn decode(reader: &mut BytecodeReader) -> Result<Self, DecodeError> {
        // Read name
        let name = reader.read_string()?;

        // Read function ID
        let function_id = reader.read_u32()? as usize;

        Ok(Self { name, function_id })
    }
}

/// Module metadata
#[derive(Debug, Clone, Default)]
pub struct Metadata {
    /// Module name
    pub name: String,
    /// Source file path
    pub source_file: Option<String>,
}

impl Metadata {
    /// Encode metadata to binary
    fn encode(&self, writer: &mut BytecodeWriter) {
        // Write name
        writer.emit_u32(self.name.len() as u32);
        writer.buffer.extend_from_slice(self.name.as_bytes());

        // Write source file (optional)
        match &self.source_file {
            Some(path) => {
                writer.emit_u8(1); // has source file
                writer.emit_u32(path.len() as u32);
                writer.buffer.extend_from_slice(path.as_bytes());
            }
            None => {
                writer.emit_u8(0); // no source file
            }
        }
    }

    /// Decode metadata from binary
    fn decode(reader: &mut BytecodeReader) -> Result<Self, DecodeError> {
        // Read name
        let name = reader.read_string()?;

        // Read source file
        let has_source = reader.read_u8()? != 0;
        let source_file = if has_source {
            Some(reader.read_string()?)
        } else {
            None
        };

        Ok(Self { name, source_file })
    }
}

impl Module {
    /// Create a new empty module
    pub fn new(name: String) -> Self {
        Self {
            magic: MAGIC,
            version: VERSION,
            flags: 0,
            constants: ConstantPool::new(),
            functions: Vec::new(),
            classes: Vec::new(),
            metadata: Metadata {
                name,
                source_file: None,
            },
        }
    }

    /// Validate module structure
    pub fn validate(&self) -> Result<(), String> {
        if self.magic != MAGIC {
            return Err("Invalid magic number".to_string());
        }
        if self.version != VERSION {
            return Err(format!("Unsupported version: {}", self.version));
        }
        Ok(())
    }

    /// Encode the module to binary format (.rbc)
    ///
    /// Format:
    /// - Header: magic (4 bytes) + version (u32) + flags (u32) + checksum (u32)
    /// - Constant pool
    /// - Function table
    /// - Class table
    /// - Bytecode section
    /// - Metadata (if flags indicate)
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = BytecodeWriter::new();

        // Reserve space for header (we'll fill in checksum later)
        let header_start = writer.offset();
        writer.buffer.extend_from_slice(&self.magic);
        writer.emit_u32(self.version);
        writer.emit_u32(self.flags);
        let checksum_offset = writer.offset();
        writer.emit_u32(0); // Placeholder for checksum

        // Encode constant pool
        self.constants.encode(&mut writer);

        // Encode functions
        writer.emit_u32(self.functions.len() as u32);
        for func in &self.functions {
            func.encode(&mut writer);
        }

        // Encode classes
        writer.emit_u32(self.classes.len() as u32);
        for class in &self.classes {
            class.encode(&mut writer);
        }

        // Encode metadata
        self.metadata.encode(&mut writer);

        // Calculate and write checksum (CRC32 of everything after the header)
        let payload = &writer.buffer[header_start + 16..];
        let checksum = crc32fast::hash(payload);
        writer.patch_u32(checksum_offset, checksum);

        writer.into_bytes()
    }

    /// Decode a module from binary format
    pub fn decode(data: &[u8]) -> Result<Self, ModuleError> {
        let mut reader = BytecodeReader::new(data);

        // Read header
        let magic = reader.read_bytes(4)?;
        let magic: [u8; 4] = magic.try_into().unwrap();
        if magic != MAGIC {
            return Err(ModuleError::InvalidMagic(magic));
        }

        let version = reader.read_u32()?;
        if version != VERSION {
            return Err(ModuleError::UnsupportedVersion(version));
        }

        let flags = reader.read_u32()?;
        let stored_checksum = reader.read_u32()?;

        // Verify checksum
        let payload = &data[16..];
        let calculated_checksum = crc32fast::hash(payload);
        if stored_checksum != calculated_checksum {
            return Err(ModuleError::ChecksumMismatch {
                expected: stored_checksum,
                actual: calculated_checksum,
            });
        }

        // Decode constant pool
        let constants = ConstantPool::decode(&mut reader)?;

        // Decode functions
        let func_count = reader.read_u32()? as usize;
        let mut functions = Vec::with_capacity(func_count);
        for _ in 0..func_count {
            functions.push(Function::decode(&mut reader)?);
        }

        // Decode classes
        let class_count = reader.read_u32()? as usize;
        let mut classes = Vec::with_capacity(class_count);
        for _ in 0..class_count {
            classes.push(ClassDef::decode(&mut reader)?);
        }

        // Decode metadata
        let metadata = Metadata::decode(&mut reader)?;

        Ok(Self {
            magic,
            version,
            flags,
            constants,
            functions,
            classes,
            metadata,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opcode::Opcode;

    #[test]
    fn test_module_creation() {
        let module = Module::new("test".to_string());
        assert_eq!(module.magic, MAGIC);
        assert_eq!(module.version, VERSION);
        assert_eq!(module.flags, 0);
        assert!(module.validate().is_ok());
    }

    #[test]
    fn test_empty_module_encoding() {
        let module = Module::new("test_module".to_string());
        let bytes = module.encode();

        // Decode it back
        let decoded = Module::decode(&bytes).unwrap();

        assert_eq!(decoded.magic, MAGIC);
        assert_eq!(decoded.version, VERSION);
        assert_eq!(decoded.metadata.name, "test_module");
        assert_eq!(decoded.functions.len(), 0);
        assert_eq!(decoded.classes.len(), 0);
    }

    #[test]
    fn test_module_with_function() {
        let mut module = Module::new("test".to_string());

        // Add a simple function
        let mut writer = BytecodeWriter::new();
        writer.emit_const_i32(42);
        writer.emit_return();

        module.functions.push(Function {
            name: "main".to_string(),
            param_count: 0,
            local_count: 1,
            code: writer.into_bytes(),
        });

        // Encode and decode
        let bytes = module.encode();
        let decoded = Module::decode(&bytes).unwrap();

        assert_eq!(decoded.functions.len(), 1);
        assert_eq!(decoded.functions[0].name, "main");
        assert_eq!(decoded.functions[0].param_count, 0);
        assert_eq!(decoded.functions[0].local_count, 1);
        assert_eq!(decoded.functions[0].code.len(), 6); // CONST_I32 (1) + i32 (4) + RETURN (1)
    }

    #[test]
    fn test_module_with_constants() {
        let mut module = Module::new("test".to_string());

        // Add constants
        module.constants.add_string("hello".to_string());
        module.constants.add_integer(42);
        module.constants.add_float(3.14);

        // Encode and decode
        let bytes = module.encode();
        let decoded = Module::decode(&bytes).unwrap();

        assert_eq!(decoded.constants.get_string(0), Some("hello"));
        assert_eq!(decoded.constants.get_integer(0), Some(42));
        assert!((decoded.constants.get_float(0).unwrap() - 3.14).abs() < 0.001);
    }

    #[test]
    fn test_module_with_class() {
        let mut module = Module::new("test".to_string());

        // Add a class
        module.classes.push(ClassDef {
            name: "MyClass".to_string(),
            field_count: 3,
            methods: vec![
                Method {
                    name: "constructor".to_string(),
                    function_id: 0,
                },
                Method {
                    name: "doSomething".to_string(),
                    function_id: 1,
                },
            ],
        });

        // Encode and decode
        let bytes = module.encode();
        let decoded = Module::decode(&bytes).unwrap();

        assert_eq!(decoded.classes.len(), 1);
        assert_eq!(decoded.classes[0].name, "MyClass");
        assert_eq!(decoded.classes[0].field_count, 3);
        assert_eq!(decoded.classes[0].methods.len(), 2);
        assert_eq!(decoded.classes[0].methods[0].name, "constructor");
        assert_eq!(decoded.classes[0].methods[1].name, "doSomething");
    }

    #[test]
    fn test_module_with_metadata() {
        let mut module = Module::new("test_module".to_string());
        module.metadata.source_file = Some("src/main.raya".to_string());

        // Encode and decode
        let bytes = module.encode();
        let decoded = Module::decode(&bytes).unwrap();

        assert_eq!(decoded.metadata.name, "test_module");
        assert_eq!(decoded.metadata.source_file, Some("src/main.raya".to_string()));
    }

    #[test]
    fn test_module_checksum_validation() {
        let module = Module::new("test".to_string());
        let mut bytes = module.encode();

        // Corrupt the data (change a byte after the header)
        if bytes.len() > 20 {
            bytes[20] ^= 0xFF;

            // Decoding should fail due to checksum mismatch
            let result = Module::decode(&bytes);
            assert!(matches!(result, Err(ModuleError::ChecksumMismatch { .. })));
        }
    }

    #[test]
    fn test_invalid_magic_number() {
        let mut bytes = vec![b'X', b'X', b'X', b'X']; // Invalid magic
        bytes.extend_from_slice(&1u32.to_le_bytes()); // version
        bytes.extend_from_slice(&0u32.to_le_bytes()); // flags
        bytes.extend_from_slice(&0u32.to_le_bytes()); // checksum

        let result = Module::decode(&bytes);
        assert!(matches!(result, Err(ModuleError::InvalidMagic(_))));
    }

    #[test]
    fn test_unsupported_version() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RAYA"); // magic
        bytes.extend_from_slice(&999u32.to_le_bytes()); // unsupported version
        bytes.extend_from_slice(&0u32.to_le_bytes()); // flags
        bytes.extend_from_slice(&0u32.to_le_bytes()); // checksum

        let result = Module::decode(&bytes);
        assert!(matches!(result, Err(ModuleError::UnsupportedVersion(999))));
    }

    #[test]
    fn test_complex_module() {
        let mut module = Module::new("complex_module".to_string());
        module.metadata.source_file = Some("test.raya".to_string());
        module.flags = flags::HAS_DEBUG_INFO;

        // Add constants
        module.constants.add_string("hello".to_string());
        module.constants.add_string("world".to_string());
        module.constants.add_integer(42);
        module.constants.add_float(3.14159);

        // Add functions
        let mut writer = BytecodeWriter::new();
        writer.emit_const_i32(42);
        writer.emit_load_local_0();
        writer.emit_iadd();
        writer.emit_return();

        module.functions.push(Function {
            name: "add42".to_string(),
            param_count: 1,
            local_count: 2,
            code: writer.into_bytes(),
        });

        // Add class
        module.classes.push(ClassDef {
            name: "Calculator".to_string(),
            field_count: 2,
            methods: vec![Method {
                name: "add42".to_string(),
                function_id: 0,
            }],
        });

        // Encode and decode
        let bytes = module.encode();
        let decoded = Module::decode(&bytes).unwrap();

        // Verify everything
        assert_eq!(decoded.metadata.name, "complex_module");
        assert_eq!(decoded.metadata.source_file, Some("test.raya".to_string()));
        assert_eq!(decoded.flags, flags::HAS_DEBUG_INFO);
        assert_eq!(decoded.constants.strings.len(), 2);
        assert_eq!(decoded.constants.integers.len(), 1);
        assert_eq!(decoded.constants.floats.len(), 1);
        assert_eq!(decoded.functions.len(), 1);
        assert_eq!(decoded.classes.len(), 1);
    }
}
