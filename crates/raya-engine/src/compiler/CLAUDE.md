# compiler module

Middle-end and backend of the Raya compiler: IR, optimizations, and bytecode generation.

## Module Structure

```
compiler/
├── mod.rs            # Entry point, Compiler struct
├── error.rs          # Compilation errors
├── module_builder.rs # Bytecode module construction
├── native_id.rs      # Native function IDs
├── codegen_ast.rs    # Direct AST → Bytecode (legacy)
├── lower/            # AST → IR lowering
├── ir/               # Intermediate representation
├── monomorphize/     # Generic specialization
├── optimize/         # IR optimizations
├── codegen/          # IR → Bytecode
├── bytecode/         # Bytecode format definitions
├── intrinsic/        # Compiler intrinsics (JSON, etc.)
└── module/           # Multi-module compilation
```

## Compilation Pipeline

```
AST (typed)
    │
    ▼
┌─────────────────┐
│    Lowering     │  lower/
└────────┬────────┘
         │
         ▼
IR (three-address code)
         │
         ▼
┌─────────────────┐
│ Monomorphization│  monomorphize/
└────────┬────────┘
         │
         ▼
Specialized IR
         │
         ▼
┌─────────────────┐
│  Optimizations  │  optimize/
└────────┬────────┘
         │
         ▼
Optimized IR
         │
         ▼
┌─────────────────┐
│    Codegen      │  codegen/
└────────┬────────┘
         │
         ▼
Bytecode Module
```

## Key Types

### Compiler
```rust
pub struct Compiler<'a> {
    type_ctx: TypeContext,
    interner: &'a Interner,
    expr_types: HashMap<usize, TypeId>,
}

// Preferred compilation path
compiler.compile_via_ir(&ast_module) -> CompileResult<Module>

// With verification
compiler.compile_via_ir_verified(&ast_module) -> CompileResult<Module>

// Debug output
compiler.compile_with_debug(&ast_module) -> CompileResult<(Module, String)>
```

### Module (Bytecode)
```rust
pub struct Module {
    pub magic: [u8; 4],        // "RAYA"
    pub version: u32,
    pub flags: u32,
    pub constants: ConstantPool,
    pub functions: Vec<Function>,
    pub classes: Vec<ClassDef>,
    pub exports: Vec<Export>,
    pub imports: Vec<Import>,
    pub checksum: [u8; 32],
}
```

## Submodules

### `lower/` - AST to IR
See [lower/CLAUDE.md](lower/CLAUDE.md).
- Statement lowering
- Expression lowering
- Control flow conversion

### `ir/` - Intermediate Representation
See [ir/CLAUDE.md](ir/CLAUDE.md).
- Three-address code
- Basic blocks
- SSA-like form

### `monomorphize/` - Generic Specialization
See [monomorphize/CLAUDE.md](monomorphize/CLAUDE.md).
- Collects generic instantiations
- Generates specialized functions

### `optimize/` - Optimizations
See [optimize/CLAUDE.md](optimize/CLAUDE.md).
- Constant folding
- Dead code elimination
- Function inlining

### `codegen/` - IR to Bytecode
See [codegen/CLAUDE.md](codegen/CLAUDE.md).
- Register allocation
- Instruction emission
- Module building

### `bytecode/` - Bytecode Format
See [bytecode/CLAUDE.md](bytecode/CLAUDE.md).
- Opcode definitions
- Module encoding/decoding
- Verification

### `module/` - Multi-Module Compilation
See [module/CLAUDE.md](module/CLAUDE.md).
- Import resolution
- Dependency graph
- Module caching
- Multi-file compilation

## Native IDs (`native_id.rs`)

Constants for native function dispatch:
```rust
// Object: 0x00xx
pub const OBJECT_TO_STRING: u16 = 0x0001;

// Array: 0x01xx
pub const ARRAY_PUSH: u16 = 0x0100;
pub const ARRAY_POP: u16 = 0x0101;

// String: 0x02xx
pub const STRING_CHAR_AT: u16 = 0x0200;
// ... etc
```

## For AI Assistants

- Use `compile_via_ir()` - it's the full optimizing pipeline
- `codegen_ast.rs` is legacy, prefer IR-based compilation
- Native IDs must match VM dispatch in `vm/vm/interpreter.rs`
- Monomorphization happens before optimization
- All generics are specialized at compile time
