//! Bytecode module format

use super::constants::ConstantPool;
use super::encoder::{BytecodeReader, BytecodeWriter, DecodeError};
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
    ChecksumMismatch {
        /// Expected checksum value
        expected: u32,
        /// Actual checksum value
        actual: u32,
    },
}

/// Symbol type for exports
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolType {
    /// Function export
    Function,
    /// Class export
    Class,
    /// Constant export
    Constant,
}

/// Exported symbol from a module
#[derive(Debug, Clone)]
pub struct Export {
    /// Symbol name
    pub name: String,
    /// Type of symbol being exported
    pub symbol_type: SymbolType,
    /// Index into functions/classes/constants array
    pub index: usize,
}

/// Imported symbol/module dependency
#[derive(Debug, Clone)]
pub struct Import {
    /// Module specifier (e.g., "logging@1.2.3", "./utils.raya", "https://...")
    pub module_specifier: String,
    /// Symbol name to import
    pub symbol: String,
    /// Optional alias (for `import { foo as bar }`)
    pub alias: Option<String>,
    /// Version constraint (for semver resolution, e.g., "^1.2.0")
    pub version_constraint: Option<String>,
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
    /// Exported symbols
    pub exports: Vec<Export>,
    /// Imported dependencies
    pub imports: Vec<Import>,
    /// SHA-256 checksum for content-addressable storage
    pub checksum: [u8; 32],
    /// Reflection data (present when HAS_REFLECTION flag is set)
    pub reflection: Option<ReflectionData>,
    /// Debug information (present when HAS_DEBUG_INFO flag is set)
    pub debug_info: Option<DebugInfo>,
    /// Native function names used by this module (indexed by local native ID).
    /// Present when HAS_NATIVE_FUNCTIONS flag is set.
    /// At load time, these names are resolved to handler functions via the NativeFunctionRegistry.
    pub native_functions: Vec<String>,
}

/// Module flags
pub mod flags {
    /// Module has debug information
    pub const HAS_DEBUG_INFO: u32 = 1 << 0;
    /// Module has reflection data
    pub const HAS_REFLECTION: u32 = 1 << 1;
    /// Module has native function table (for ModuleNativeCall)
    pub const HAS_NATIVE_FUNCTIONS: u32 = 1 << 2;
}

/// Reflection data for the entire module
#[derive(Debug, Clone, Default)]
pub struct ReflectionData {
    /// Per-class reflection data indexed by class ID
    pub classes: Vec<ClassReflectionData>,
}

/// Reflection data for a single class
#[derive(Debug, Clone, Default)]
pub struct ClassReflectionData {
    /// Field reflection data
    pub fields: Vec<FieldReflectionData>,
    /// Method names in vtable order
    pub method_names: Vec<String>,
    /// Static field names
    pub static_field_names: Vec<String>,
}

/// Reflection data for a single field
#[derive(Debug, Clone)]
pub struct FieldReflectionData {
    /// Field name
    pub name: String,
    /// Type name (for display/debugging)
    pub type_name: String,
    /// Whether this field is readonly
    pub is_readonly: bool,
    /// Whether this field is static
    pub is_static: bool,
}

// ============================================================================
// Debug Information
// ============================================================================

/// Debug information for the entire module
///
/// Contains source location mappings for functions, methods, and classes.
/// This data is only present when the module is compiled with --emit-debug.
#[derive(Debug, Clone, Default)]
pub struct DebugInfo {
    /// Source file paths referenced by line entries
    pub source_files: Vec<String>,
    /// Per-function debug information indexed by function ID
    pub functions: Vec<FunctionDebugInfo>,
    /// Per-class debug information indexed by class ID
    pub classes: Vec<ClassDebugInfo>,
}

/// Debug information for a single function
#[derive(Debug, Clone, Default)]
pub struct FunctionDebugInfo {
    /// Index into source_files array
    pub source_file_index: u32,
    /// Starting line number of the function (1-indexed)
    pub start_line: u32,
    /// Starting column number (1-indexed)
    pub start_column: u32,
    /// Ending line number
    pub end_line: u32,
    /// Ending column
    pub end_column: u32,
    /// Line number table mapping bytecode offsets to source locations
    pub line_table: Vec<LineEntry>,
}

/// A single entry in the line number table
///
/// Maps a bytecode offset to a source location.
#[derive(Debug, Clone, Copy)]
pub struct LineEntry {
    /// Bytecode offset within the function's code
    pub bytecode_offset: u32,
    /// Line number (1-indexed)
    pub line: u32,
    /// Column number (1-indexed)
    pub column: u32,
}

/// Debug information for a class
#[derive(Debug, Clone, Default)]
pub struct ClassDebugInfo {
    /// Index into source_files array
    pub source_file_index: u32,
    /// Starting line number of the class declaration
    pub start_line: u32,
    /// Starting column
    pub start_column: u32,
    /// Ending line number
    pub end_line: u32,
    /// Ending column
    pub end_column: u32,
}

impl ReflectionData {
    /// Create new empty reflection data
    pub fn new() -> Self {
        Self::default()
    }

    /// Encode reflection data to binary
    fn encode(&self, writer: &mut BytecodeWriter) {
        // Write number of classes
        writer.emit_u32(self.classes.len() as u32);
        for class in &self.classes {
            class.encode(writer);
        }
    }

    /// Decode reflection data from binary
    fn decode(reader: &mut BytecodeReader<'_>) -> Result<Self, DecodeError> {
        let class_count = reader.read_u32()? as usize;
        let mut classes = Vec::with_capacity(class_count);
        for _ in 0..class_count {
            classes.push(ClassReflectionData::decode(reader)?);
        }
        Ok(Self { classes })
    }
}

impl ClassReflectionData {
    /// Create new empty class reflection data
    pub fn new() -> Self {
        Self::default()
    }

    /// Encode class reflection data to binary
    fn encode(&self, writer: &mut BytecodeWriter) {
        // Write fields
        writer.emit_u32(self.fields.len() as u32);
        for field in &self.fields {
            field.encode(writer);
        }

        // Write method names
        writer.emit_u32(self.method_names.len() as u32);
        for name in &self.method_names {
            writer.emit_u32(name.len() as u32);
            writer.buffer.extend_from_slice(name.as_bytes());
        }

        // Write static field names
        writer.emit_u32(self.static_field_names.len() as u32);
        for name in &self.static_field_names {
            writer.emit_u32(name.len() as u32);
            writer.buffer.extend_from_slice(name.as_bytes());
        }
    }

    /// Decode class reflection data from binary
    fn decode(reader: &mut BytecodeReader<'_>) -> Result<Self, DecodeError> {
        // Read fields
        let field_count = reader.read_u32()? as usize;
        let mut fields = Vec::with_capacity(field_count);
        for _ in 0..field_count {
            fields.push(FieldReflectionData::decode(reader)?);
        }

        // Read method names
        let method_count = reader.read_u32()? as usize;
        let mut method_names = Vec::with_capacity(method_count);
        for _ in 0..method_count {
            method_names.push(reader.read_string()?);
        }

        // Read static field names
        let static_count = reader.read_u32()? as usize;
        let mut static_field_names = Vec::with_capacity(static_count);
        for _ in 0..static_count {
            static_field_names.push(reader.read_string()?);
        }

        Ok(Self {
            fields,
            method_names,
            static_field_names,
        })
    }
}

impl FieldReflectionData {
    /// Create new field reflection data
    pub fn new(name: String, type_name: String, is_readonly: bool, is_static: bool) -> Self {
        Self {
            name,
            type_name,
            is_readonly,
            is_static,
        }
    }

    /// Encode field reflection data to binary
    fn encode(&self, writer: &mut BytecodeWriter) {
        // Write name
        writer.emit_u32(self.name.len() as u32);
        writer.buffer.extend_from_slice(self.name.as_bytes());

        // Write type name
        writer.emit_u32(self.type_name.len() as u32);
        writer.buffer.extend_from_slice(self.type_name.as_bytes());

        // Write flags (packed into single byte)
        let mut flags: u8 = 0;
        if self.is_readonly {
            flags |= 1;
        }
        if self.is_static {
            flags |= 2;
        }
        writer.emit_u8(flags);
    }

    /// Decode field reflection data from binary
    fn decode(reader: &mut BytecodeReader<'_>) -> Result<Self, DecodeError> {
        let name = reader.read_string()?;
        let type_name = reader.read_string()?;
        let flags = reader.read_u8()?;

        Ok(Self {
            name,
            type_name,
            is_readonly: (flags & 1) != 0,
            is_static: (flags & 2) != 0,
        })
    }
}

// ============================================================================
// Debug Info Implementations
// ============================================================================

impl DebugInfo {
    /// Create new empty debug info
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or get index for a source file
    pub fn add_source_file(&mut self, path: String) -> u32 {
        if let Some(idx) = self.source_files.iter().position(|p| p == &path) {
            idx as u32
        } else {
            let idx = self.source_files.len() as u32;
            self.source_files.push(path);
            idx
        }
    }

    /// Get source file by index
    pub fn get_source_file(&self, index: u32) -> Option<&str> {
        self.source_files.get(index as usize).map(|s| s.as_str())
    }

    /// Encode debug info to binary
    pub(crate) fn encode(&self, writer: &mut BytecodeWriter) {
        // Write source files
        writer.emit_u32(self.source_files.len() as u32);
        for path in &self.source_files {
            writer.emit_u32(path.len() as u32);
            writer.buffer.extend_from_slice(path.as_bytes());
        }

        // Write function debug info
        writer.emit_u32(self.functions.len() as u32);
        for func in &self.functions {
            func.encode(writer);
        }

        // Write class debug info
        writer.emit_u32(self.classes.len() as u32);
        for class in &self.classes {
            class.encode(writer);
        }
    }

    /// Decode debug info from binary
    pub(crate) fn decode(reader: &mut BytecodeReader<'_>) -> Result<Self, DecodeError> {
        // Read source files
        let file_count = reader.read_u32()? as usize;
        let mut source_files = Vec::with_capacity(file_count);
        for _ in 0..file_count {
            source_files.push(reader.read_string()?);
        }

        // Read function debug info
        let func_count = reader.read_u32()? as usize;
        let mut functions = Vec::with_capacity(func_count);
        for _ in 0..func_count {
            functions.push(FunctionDebugInfo::decode(reader)?);
        }

        // Read class debug info
        let class_count = reader.read_u32()? as usize;
        let mut classes = Vec::with_capacity(class_count);
        for _ in 0..class_count {
            classes.push(ClassDebugInfo::decode(reader)?);
        }

        Ok(Self {
            source_files,
            functions,
            classes,
        })
    }
}

impl FunctionDebugInfo {
    /// Create new function debug info
    pub fn new(
        source_file_index: u32,
        start_line: u32,
        start_column: u32,
        end_line: u32,
        end_column: u32,
    ) -> Self {
        Self {
            source_file_index,
            start_line,
            start_column,
            end_line,
            end_column,
            line_table: Vec::new(),
        }
    }

    /// Add a line entry to the table
    pub fn add_line_entry(&mut self, bytecode_offset: u32, line: u32, column: u32) {
        self.line_table.push(LineEntry {
            bytecode_offset,
            line,
            column,
        });
    }

    /// Look up source location for a bytecode offset
    ///
    /// Returns the line entry that covers the given offset, or None if not found.
    /// Uses binary search for efficiency.
    pub fn lookup_location(&self, bytecode_offset: u32) -> Option<&LineEntry> {
        if self.line_table.is_empty() {
            return None;
        }

        // Binary search for the largest offset <= bytecode_offset
        let idx = self.line_table
            .partition_point(|e| e.bytecode_offset <= bytecode_offset);

        if idx == 0 {
            // bytecode_offset is before first entry
            Some(&self.line_table[0])
        } else {
            Some(&self.line_table[idx - 1])
        }
    }

    /// Encode to binary
    fn encode(&self, writer: &mut BytecodeWriter) {
        writer.emit_u32(self.source_file_index);
        writer.emit_u32(self.start_line);
        writer.emit_u32(self.start_column);
        writer.emit_u32(self.end_line);
        writer.emit_u32(self.end_column);

        // Write line table with delta encoding for compactness
        writer.emit_u32(self.line_table.len() as u32);
        for entry in &self.line_table {
            writer.emit_u32(entry.bytecode_offset);
            writer.emit_u32(entry.line);
            writer.emit_u32(entry.column);
        }
    }

    /// Decode from binary
    fn decode(reader: &mut BytecodeReader<'_>) -> Result<Self, DecodeError> {
        let source_file_index = reader.read_u32()?;
        let start_line = reader.read_u32()?;
        let start_column = reader.read_u32()?;
        let end_line = reader.read_u32()?;
        let end_column = reader.read_u32()?;

        let entry_count = reader.read_u32()? as usize;
        let mut line_table = Vec::with_capacity(entry_count);
        for _ in 0..entry_count {
            let bytecode_offset = reader.read_u32()?;
            let line = reader.read_u32()?;
            let column = reader.read_u32()?;
            line_table.push(LineEntry {
                bytecode_offset,
                line,
                column,
            });
        }

        Ok(Self {
            source_file_index,
            start_line,
            start_column,
            end_line,
            end_column,
            line_table,
        })
    }
}

impl ClassDebugInfo {
    /// Create new class debug info
    pub fn new(
        source_file_index: u32,
        start_line: u32,
        start_column: u32,
        end_line: u32,
        end_column: u32,
    ) -> Self {
        Self {
            source_file_index,
            start_line,
            start_column,
            end_line,
            end_column,
        }
    }

    /// Encode to binary
    fn encode(&self, writer: &mut BytecodeWriter) {
        writer.emit_u32(self.source_file_index);
        writer.emit_u32(self.start_line);
        writer.emit_u32(self.start_column);
        writer.emit_u32(self.end_line);
        writer.emit_u32(self.end_column);
    }

    /// Decode from binary
    fn decode(reader: &mut BytecodeReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            source_file_index: reader.read_u32()?,
            start_line: reader.read_u32()?,
            start_column: reader.read_u32()?,
            end_line: reader.read_u32()?,
            end_column: reader.read_u32()?,
        })
    }
}

impl LineEntry {
    /// Create a new line entry
    pub fn new(bytecode_offset: u32, line: u32, column: u32) -> Self {
        Self {
            bytecode_offset,
            line,
            column,
        }
    }
}

/// Function definition
#[derive(Debug, Clone)]
pub struct Function {
    /// Function name
    pub name: String,
    /// Number of parameters
    pub param_count: usize,
    /// Number of local variables (stack-based interpreter)
    pub local_count: usize,
    /// Stack-based bytecode instructions
    pub code: Vec<u8>,
    /// Number of registers needed (register-based interpreter)
    /// Includes params + locals + temporaries. Set by register codegen.
    pub register_count: u16,
    /// Register-based bytecode instructions (32-bit fixed-width words)
    /// Empty when using stack-based codegen.
    pub reg_code: Vec<u32>,
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
    fn decode(reader: &mut BytecodeReader<'_>) -> Result<Self, DecodeError> {
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
            register_count: 0,
            reg_code: Vec::new(),
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
    /// Parent class ID (None for root classes)
    pub parent_id: Option<u32>,
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

        // Write parent class ID (0xFFFFFFFF means no parent)
        match self.parent_id {
            Some(id) => writer.emit_u32(id),
            None => writer.emit_u32(0xFFFFFFFF),
        }

        // Write methods
        writer.emit_u32(self.methods.len() as u32);
        for method in &self.methods {
            method.encode(writer);
        }
    }

    /// Decode class definition from binary
    fn decode(reader: &mut BytecodeReader<'_>) -> Result<Self, DecodeError> {
        // Read name
        let name = reader.read_string()?;

        // Read field count
        let field_count = reader.read_u32()? as usize;

        // Read parent class ID (0xFFFFFFFF means no parent)
        let parent_raw = reader.read_u32()?;
        let parent_id = if parent_raw == 0xFFFFFFFF {
            None
        } else {
            Some(parent_raw)
        };

        // Read methods
        let method_count = reader.read_u32()? as usize;
        let mut methods = Vec::with_capacity(method_count);
        for _ in 0..method_count {
            methods.push(Method::decode(reader)?);
        }

        Ok(Self {
            name,
            field_count,
            parent_id,
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
    /// Vtable slot index for virtual dispatch
    pub slot: usize,
}

impl Method {
    /// Encode method to binary
    fn encode(&self, writer: &mut BytecodeWriter) {
        // Write name
        writer.emit_u32(self.name.len() as u32);
        writer.buffer.extend_from_slice(self.name.as_bytes());

        // Write function ID
        writer.emit_u32(self.function_id as u32);

        // Write slot index
        writer.emit_u32(self.slot as u32);
    }

    /// Decode method from binary
    fn decode(reader: &mut BytecodeReader<'_>) -> Result<Self, DecodeError> {
        // Read name
        let name = reader.read_string()?;

        // Read function ID
        let function_id = reader.read_u32()? as usize;

        // Read slot index
        let slot = reader.read_u32()? as usize;

        Ok(Self { name, function_id, slot })
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
    fn decode(reader: &mut BytecodeReader<'_>) -> Result<Self, DecodeError> {
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

impl SymbolType {
    /// Encode to u8
    fn to_u8(&self) -> u8 {
        match self {
            SymbolType::Function => 0,
            SymbolType::Class => 1,
            SymbolType::Constant => 2,
        }
    }

    /// Decode from u8
    fn from_u8(value: u8) -> Result<Self, DecodeError> {
        match value {
            0 => Ok(SymbolType::Function),
            1 => Ok(SymbolType::Class),
            2 => Ok(SymbolType::Constant),
            _ => Err(DecodeError::InvalidOpcode(value, 0)), // Reuse InvalidOpcode for invalid symbol type
        }
    }
}

impl Export {
    /// Encode export to binary
    fn encode(&self, writer: &mut BytecodeWriter) {
        // Write name
        writer.emit_u32(self.name.len() as u32);
        writer.buffer.extend_from_slice(self.name.as_bytes());

        // Write symbol type
        writer.emit_u8(self.symbol_type.to_u8());

        // Write index
        writer.emit_u32(self.index as u32);
    }

    /// Decode export from binary
    fn decode(reader: &mut BytecodeReader<'_>) -> Result<Self, DecodeError> {
        // Read name
        let name = reader.read_string()?;

        // Read symbol type
        let symbol_type = SymbolType::from_u8(reader.read_u8()?)?;

        // Read index
        let index = reader.read_u32()? as usize;

        Ok(Self { name, symbol_type, index })
    }
}

impl Import {
    /// Encode import to binary
    fn encode(&self, writer: &mut BytecodeWriter) {
        // Write module specifier
        writer.emit_u32(self.module_specifier.len() as u32);
        writer.buffer.extend_from_slice(self.module_specifier.as_bytes());

        // Write symbol name
        writer.emit_u32(self.symbol.len() as u32);
        writer.buffer.extend_from_slice(self.symbol.as_bytes());

        // Write alias (optional)
        match &self.alias {
            Some(alias) => {
                writer.emit_u8(1); // has alias
                writer.emit_u32(alias.len() as u32);
                writer.buffer.extend_from_slice(alias.as_bytes());
            }
            None => {
                writer.emit_u8(0); // no alias
            }
        }

        // Write version constraint (optional)
        match &self.version_constraint {
            Some(constraint) => {
                writer.emit_u8(1); // has constraint
                writer.emit_u32(constraint.len() as u32);
                writer.buffer.extend_from_slice(constraint.as_bytes());
            }
            None => {
                writer.emit_u8(0); // no constraint
            }
        }
    }

    /// Decode import from binary
    fn decode(reader: &mut BytecodeReader<'_>) -> Result<Self, DecodeError> {
        // Read module specifier
        let module_specifier = reader.read_string()?;

        // Read symbol name
        let symbol = reader.read_string()?;

        // Read alias
        let has_alias = reader.read_u8()? != 0;
        let alias = if has_alias {
            Some(reader.read_string()?)
        } else {
            None
        };

        // Read version constraint
        let has_constraint = reader.read_u8()? != 0;
        let version_constraint = if has_constraint {
            Some(reader.read_string()?)
        } else {
            None
        };

        Ok(Self {
            module_specifier,
            symbol,
            alias,
            version_constraint,
        })
    }
}

impl Module {
    /// Create a new empty module
    ///
    /// Reflection metadata is always enabled to support runtime introspection.
    pub fn new(name: String) -> Self {
        Self {
            magic: MAGIC,
            version: VERSION,
            flags: flags::HAS_REFLECTION,
            constants: ConstantPool::new(),
            functions: Vec::new(),
            classes: Vec::new(),
            metadata: Metadata {
                name,
                source_file: None,
            },
            exports: Vec::new(),
            imports: Vec::new(),
            checksum: [0; 32], // Will be computed during encode()
            reflection: Some(ReflectionData::new()),
            debug_info: None,
            native_functions: Vec::new(),
        }
    }

    /// Check if this module has reflection data
    pub fn has_reflection(&self) -> bool {
        (self.flags & flags::HAS_REFLECTION) != 0
    }

    /// Check if this module has debug information
    pub fn has_debug_info(&self) -> bool {
        (self.flags & flags::HAS_DEBUG_INFO) != 0
    }

    /// Enable reflection for this module
    pub fn enable_reflection(&mut self) {
        self.flags |= flags::HAS_REFLECTION;
        if self.reflection.is_none() {
            self.reflection = Some(ReflectionData::new());
        }
    }

    /// Enable debug info for this module
    pub fn enable_debug_info(&mut self) {
        self.flags |= flags::HAS_DEBUG_INFO;
        if self.debug_info.is_none() {
            self.debug_info = Some(DebugInfo::new());
        }
    }

    /// Get mutable reference to debug info, creating if needed
    pub fn debug_info_mut(&mut self) -> &mut DebugInfo {
        self.enable_debug_info();
        self.debug_info.as_mut().unwrap()
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

    /// Encode the module to binary format (.ryb)
    ///
    /// Format:
    /// - Header: magic (4 bytes) + version (u32) + flags (u32) + crc32 (u32) + checksum (32 bytes SHA-256)
    /// - Constant pool
    /// - Function table
    /// - Class table
    /// - Export table
    /// - Import table
    /// - Metadata
    pub fn encode(&self) -> Vec<u8> {
        use sha2::{Sha256, Digest};

        let mut writer = BytecodeWriter::new();

        // Reserve space for header (we'll fill in checksums later)
        let header_start = writer.offset();
        writer.buffer.extend_from_slice(&self.magic);
        writer.emit_u32(self.version);
        writer.emit_u32(self.flags);
        let crc32_offset = writer.offset();
        writer.emit_u32(0); // Placeholder for CRC32
        let sha256_offset = writer.offset();
        writer.buffer.extend_from_slice(&[0u8; 32]); // Placeholder for SHA-256

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

        // Encode exports
        writer.emit_u32(self.exports.len() as u32);
        for export in &self.exports {
            export.encode(&mut writer);
        }

        // Encode imports
        writer.emit_u32(self.imports.len() as u32);
        for import in &self.imports {
            import.encode(&mut writer);
        }

        // Encode metadata
        self.metadata.encode(&mut writer);

        // Encode reflection data if present
        if (self.flags & flags::HAS_REFLECTION) != 0 {
            if let Some(ref reflection) = self.reflection {
                reflection.encode(&mut writer);
            } else {
                // Write empty reflection data if flag is set but no data
                ReflectionData::new().encode(&mut writer);
            }
        }

        // Encode debug info if present
        if (self.flags & flags::HAS_DEBUG_INFO) != 0 {
            if let Some(ref debug_info) = self.debug_info {
                debug_info.encode(&mut writer);
            } else {
                // Write empty debug info if flag is set but no data
                DebugInfo::new().encode(&mut writer);
            }
        }

        // Encode native function table if present
        if (self.flags & flags::HAS_NATIVE_FUNCTIONS) != 0 {
            writer.emit_u32(self.native_functions.len() as u32);
            for name in &self.native_functions {
                writer.emit_u32(name.len() as u32);
                writer.buffer.extend_from_slice(name.as_bytes());
            }
        }

        // Calculate checksums (of everything after header)
        let payload_start = header_start + 48; // Skip magic + version + flags + crc32 + sha256
        let payload = writer.buffer[payload_start..].to_vec(); // Clone to avoid borrow issues
        let crc32 = crc32fast::hash(&payload);
        let hash = Sha256::digest(&payload);
        let checksum_bytes: [u8; 32] = hash.into();

        // Patch CRC32
        writer.patch_u32(crc32_offset, crc32);

        // Patch SHA-256
        writer.buffer[sha256_offset..sha256_offset + 32].copy_from_slice(&checksum_bytes);

        writer.into_bytes()
    }

    /// Decode a module from binary format
    pub fn decode(data: &[u8]) -> Result<Self, ModuleError> {
        use sha2::{Sha256, Digest};

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
        let stored_crc32 = reader.read_u32()?;

        // Read SHA-256 checksum
        let stored_sha256 = reader.read_bytes(32)?;
        let checksum: [u8; 32] = stored_sha256.try_into().unwrap();

        // Verify checksums (skip magic + version + flags + crc32 + sha256)
        let payload = &data[48..];

        // Verify CRC32
        let calculated_crc32 = crc32fast::hash(payload);
        if stored_crc32 != calculated_crc32 {
            return Err(ModuleError::ChecksumMismatch {
                expected: stored_crc32,
                actual: calculated_crc32,
            });
        }

        // Verify SHA-256
        let calculated_sha256 = Sha256::digest(payload);
        if checksum != calculated_sha256.as_slice() {
            return Err(ModuleError::ChecksumMismatch {
                expected: stored_crc32, // Using CRC32 for error message
                actual: calculated_crc32,
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

        // Decode exports
        let export_count = reader.read_u32()? as usize;
        let mut exports = Vec::with_capacity(export_count);
        for _ in 0..export_count {
            exports.push(Export::decode(&mut reader)?);
        }

        // Decode imports
        let import_count = reader.read_u32()? as usize;
        let mut imports = Vec::with_capacity(import_count);
        for _ in 0..import_count {
            imports.push(Import::decode(&mut reader)?);
        }

        // Decode metadata
        let metadata = Metadata::decode(&mut reader)?;

        // Decode reflection data if present
        let reflection = if (flags & flags::HAS_REFLECTION) != 0 {
            Some(ReflectionData::decode(&mut reader)?)
        } else {
            None
        };

        // Decode debug info if present
        let debug_info = if (flags & flags::HAS_DEBUG_INFO) != 0 {
            Some(DebugInfo::decode(&mut reader)?)
        } else {
            None
        };

        // Decode native function table if present
        let native_functions = if (flags & flags::HAS_NATIVE_FUNCTIONS) != 0 {
            let count = reader.read_u32()? as usize;
            let mut names = Vec::with_capacity(count);
            for _ in 0..count {
                names.push(reader.read_string()?);
            }
            names
        } else {
            Vec::new()
        };

        Ok(Self {
            magic,
            version,
            flags,
            constants,
            functions,
            classes,
            metadata,
            exports,
            imports,
            checksum,
            reflection,
            debug_info,
            native_functions,
        })
    }
}

#[cfg(test)]
#[allow(clippy::approx_constant)]
mod tests {
    use super::*;

    #[test]
    fn test_module_creation() {
        let module = Module::new("test".to_string());
        assert_eq!(module.magic, MAGIC);
        assert_eq!(module.version, VERSION);
        // Reflection is always enabled by default
        assert_eq!(module.flags, flags::HAS_REFLECTION);
        assert!(module.has_reflection());
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
            register_count: 0,
            reg_code: Vec::new(),
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
            parent_id: None,
            methods: vec![
                Method {
                    name: "constructor".to_string(),
                    function_id: 0,
                    slot: 0,
                },
                Method {
                    name: "doSomething".to_string(),
                    function_id: 1,
                    slot: 1,
                },
            ],
        });

        // Encode and decode
        let bytes = module.encode();
        let decoded = Module::decode(&bytes).unwrap();

        assert_eq!(decoded.classes.len(), 1);
        assert_eq!(decoded.classes[0].name, "MyClass");
        assert_eq!(decoded.classes[0].field_count, 3);
        assert_eq!(decoded.classes[0].parent_id, None);
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
        assert_eq!(
            decoded.metadata.source_file,
            Some("src/main.raya".to_string())
        );
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
            register_count: 0,
            reg_code: Vec::new(),
        });

        // Add class
        module.classes.push(ClassDef {
            name: "Calculator".to_string(),
            field_count: 2,
            parent_id: None,
            methods: vec![Method {
                name: "add42".to_string(),
                function_id: 0,
                slot: 0,
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

    #[test]
    fn test_module_with_reflection() {
        let mut module = Module::new("test_reflection".to_string());
        // Reflection is now always enabled by default

        // Add class reflection data
        let mut class_reflection = ClassReflectionData::new();
        class_reflection.fields.push(FieldReflectionData::new(
            "name".to_string(),
            "string".to_string(),
            false,
            false,
        ));
        class_reflection.fields.push(FieldReflectionData::new(
            "age".to_string(),
            "number".to_string(),
            true, // readonly
            false,
        ));
        class_reflection.method_names.push("greet".to_string());
        class_reflection.method_names.push("compute".to_string());
        class_reflection.static_field_names.push("CONSTANT".to_string());

        module.reflection.as_mut().unwrap().classes.push(class_reflection);

        // Also add a corresponding class definition
        module.classes.push(ClassDef {
            name: "Person".to_string(),
            field_count: 2,
            parent_id: None,
            methods: vec![],
        });

        // Encode and decode
        let bytes = module.encode();
        let decoded = Module::decode(&bytes).unwrap();

        // Verify flags
        assert!(decoded.has_reflection());
        assert_eq!(decoded.flags & flags::HAS_REFLECTION, flags::HAS_REFLECTION);

        // Verify reflection data
        let reflection = decoded.reflection.as_ref().unwrap();
        assert_eq!(reflection.classes.len(), 1);

        let class_ref = &reflection.classes[0];
        assert_eq!(class_ref.fields.len(), 2);
        assert_eq!(class_ref.fields[0].name, "name");
        assert_eq!(class_ref.fields[0].type_name, "string");
        assert!(!class_ref.fields[0].is_readonly);

        assert_eq!(class_ref.fields[1].name, "age");
        assert_eq!(class_ref.fields[1].type_name, "number");
        assert!(class_ref.fields[1].is_readonly);

        assert_eq!(class_ref.method_names, vec!["greet", "compute"]);
        assert_eq!(class_ref.static_field_names, vec!["CONSTANT"]);
    }

    #[test]
    fn test_module_always_has_reflection() {
        // Reflection is always enabled - there's no "without reflection" mode
        let module = Module::new("test_reflection".to_string());

        // Encode and decode
        let bytes = module.encode();
        let decoded = Module::decode(&bytes).unwrap();

        // Verify reflection is always present
        assert!(decoded.has_reflection());
        assert!(decoded.reflection.is_some());
    }

    #[test]
    fn test_module_with_debug_info() {
        let mut module = Module::new("test_debug".to_string());
        module.enable_debug_info();

        // Add source file
        let file_idx = module.debug_info_mut().add_source_file("src/main.raya".to_string());
        assert_eq!(file_idx, 0);

        // Add function debug info
        let mut func_debug = FunctionDebugInfo::new(0, 10, 1, 25, 1);
        func_debug.add_line_entry(0, 11, 5);  // first instruction at line 11
        func_debug.add_line_entry(5, 12, 5);  // next instruction at line 12
        func_debug.add_line_entry(10, 15, 5); // jump at line 15
        module.debug_info_mut().functions.push(func_debug);

        // Add class debug info
        let class_debug = ClassDebugInfo::new(0, 1, 1, 50, 1);
        module.debug_info_mut().classes.push(class_debug);

        // Encode and decode
        let bytes = module.encode();
        let decoded = Module::decode(&bytes).unwrap();

        // Verify debug info
        assert!(decoded.has_debug_info());
        let debug_info = decoded.debug_info.as_ref().unwrap();

        assert_eq!(debug_info.source_files.len(), 1);
        assert_eq!(debug_info.source_files[0], "src/main.raya");

        assert_eq!(debug_info.functions.len(), 1);
        let func = &debug_info.functions[0];
        assert_eq!(func.source_file_index, 0);
        assert_eq!(func.start_line, 10);
        assert_eq!(func.end_line, 25);
        assert_eq!(func.line_table.len(), 3);
        assert_eq!(func.line_table[0].bytecode_offset, 0);
        assert_eq!(func.line_table[0].line, 11);
        assert_eq!(func.line_table[1].bytecode_offset, 5);
        assert_eq!(func.line_table[1].line, 12);

        assert_eq!(debug_info.classes.len(), 1);
        let class = &debug_info.classes[0];
        assert_eq!(class.start_line, 1);
        assert_eq!(class.end_line, 50);
    }

    #[test]
    fn test_function_debug_info_lookup() {
        let mut func_debug = FunctionDebugInfo::new(0, 10, 1, 25, 1);
        func_debug.add_line_entry(0, 11, 5);
        func_debug.add_line_entry(5, 12, 10);
        func_debug.add_line_entry(10, 15, 3);
        func_debug.add_line_entry(20, 18, 7);

        // Lookup at exact offset
        let entry = func_debug.lookup_location(5).unwrap();
        assert_eq!(entry.line, 12);
        assert_eq!(entry.column, 10);

        // Lookup between entries (should get previous entry)
        let entry = func_debug.lookup_location(7).unwrap();
        assert_eq!(entry.line, 12);

        // Lookup at end
        let entry = func_debug.lookup_location(25).unwrap();
        assert_eq!(entry.line, 18);

        // Lookup at beginning
        let entry = func_debug.lookup_location(0).unwrap();
        assert_eq!(entry.line, 11);
    }

    #[test]
    fn test_module_without_debug_info() {
        let module = Module::new("test_no_debug".to_string());

        // Encode and decode
        let bytes = module.encode();
        let decoded = Module::decode(&bytes).unwrap();

        // Verify no debug info
        assert!(!decoded.has_debug_info());
        assert!(decoded.debug_info.is_none());
    }
}
