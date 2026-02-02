# bytecode module

Bytecode format definitions, encoding/decoding, and verification.

## Module Structure

```
bytecode/
├── mod.rs        # Re-exports
├── opcode.rs     # Opcode enum and encoding
├── module.rs     # Module format (functions, classes, constants)
├── constants.rs  # Constant pool
├── encoder.rs    # BytecodeWriter, BytecodeReader
└── verify.rs     # Module verification
```

## Opcode Categories

### Stack Operations
```
NOP, DUP, POP, SWAP
```

### Constants
```
CONST_NULL, CONST_TRUE, CONST_FALSE
CONST_I32 <i32>
CONST_I64 <i64>
CONST_F64 <f64>
CONST_STR <idx>
```

### Typed Arithmetic
```
IADD, ISUB, IMUL, IDIV, IMOD   // Integer
FADD, FSUB, FMUL, FDIV         // Float
NADD, NSUB, NMUL, NDIV, NMOD   // Number (runtime dispatch)
```

### Comparison
```
IEQ, INE, ILT, ILE, IGT, IGE   // Integer
FEQ, FNE, FLT, FLE, FGT, FGE   // Float
NEQ, NNE, NLT, NLE, NGT, NGE   // Number
SEQ, SNE, SLT, SLE, SGT, SGE   // String
```

### Locals/Globals
```
LOAD_LOCAL <idx>
STORE_LOCAL <idx>
LOAD_GLOBAL <idx>
STORE_GLOBAL <idx>
LOAD_LOCAL_0..7  // Optimized variants
```

### Objects
```
NEW_OBJECT <class_id>
GET_FIELD <field_idx>
SET_FIELD <field_idx>
```

### Arrays
```
NEW_ARRAY
ARRAY_GET
ARRAY_SET
ARRAY_LEN
```

### Control Flow
```
JUMP <offset>
JUMP_IF <offset>
JUMP_IF_FALSE <offset>
RETURN
```

### Calls
```
CALL <func_id> <arg_count>
CALL_METHOD <method_idx> <arg_count>
NATIVE_CALL <native_id> <arg_count>
RETURN
```

### Tasks
```
SPAWN <func_id> <arg_count>
AWAIT
```

### Exceptions
```
THROW
SETUP_TRY <catch_offset> <finally_offset>
END_TRY
```

### Type Operations
```
TYPEOF
INSTANCEOF <class_id>
```

## Module Format (.ryb)

```
┌─────────────────────────────────────────┐
│ Header (48 bytes)                       │
│   magic: [u8; 4]     "RAYA"            │
│   version: u32                          │
│   flags: u32         HAS_DEBUG_INFO=1  │
│                      HAS_REFLECTION=2   │
│   crc32: u32                            │
│   sha256: [u8; 32]                      │
├─────────────────────────────────────────┤
│ Constant Pool                           │
│   strings: Vec<String>                  │
│   integers: Vec<i32>                    │
│   floats: Vec<f64>                      │
├─────────────────────────────────────────┤
│ Functions                               │
│   name, param_count, local_count, code  │
├─────────────────────────────────────────┤
│ Classes                                 │
│   name, field_count, parent_id, methods │
├─────────────────────────────────────────┤
│ Exports                                 │
│   name, symbol_type, index              │
├─────────────────────────────────────────┤
│ Imports                                 │
│   module_specifier, symbol, alias       │
├─────────────────────────────────────────┤
│ Metadata                                │
│   module_name, source_file              │
├─────────────────────────────────────────┤
│ Reflection Data (if HAS_REFLECTION)     │
│   per-class field names, method names   │
├─────────────────────────────────────────┤
│ Debug Info (if HAS_DEBUG_INFO)          │
│   source_files: Vec<String>             │
│   functions: Vec<FunctionDebugInfo>     │
│     - source_file_index                 │
│     - start_line, start_column          │
│     - line_table: Vec<LineEntry>        │
│   classes: Vec<ClassDebugInfo>          │
└─────────────────────────────────────────┘
```

## Debug Info (for `getSourceLocation`)

When `HAS_DEBUG_INFO` flag is set:
- `FunctionDebugInfo` maps bytecode offsets to source lines
- `LineEntry { bytecode_offset, line, column }` for line table
- `lookup_location(offset)` finds source line for any bytecode position

## BytecodeWriter

```rust
let mut writer = BytecodeWriter::new();
writer.emit_const_i32(42);
writer.emit_load_local(0);
writer.emit_iadd();
writer.emit_return();
let bytes = writer.into_bytes();
```

## BytecodeReader

```rust
let mut reader = BytecodeReader::new(&bytes);
let opcode = Opcode::from_u8(reader.read_u8()?)?;
let value = reader.read_i32()?;
```

## Module Verification

```rust
verify_module(&module)?;
```

Checks:
- Valid opcodes
- Jump targets in bounds
- Stack balance
- Function references valid
- Class references valid

## For AI Assistants

- Opcodes are single bytes, operands follow in little-endian
- Use typed opcodes (`IADD`) when type is known statically
- Use number opcodes (`NADD`) when type is `number` (runtime dispatch)
- Module checksum is SHA-256 of payload (after header)
- All indices are u16 or u32 depending on context
- `CALL` uses function index, `CALL_METHOD` uses vtable index
