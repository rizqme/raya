# Raya File Formats (v0.2)

Specification for all file formats used in the Raya ecosystem.

---

## Table of Contents

1. [Overview](#overview)
2. [Source Files (.raya)](#source-files-raya)
3. [Binary Files (.ryb)](#binary-files-rbin)
4. [Executable Bundles](#executable-bundles)

---

## Overview

Raya uses a simplified file format system:

| Extension | Type | Purpose | Format |
|-----------|------|---------|--------|
| `.raya` | Source | Human-readable code | UTF-8 text |
| `.ts` | Source | TypeScript compatibility | UTF-8 text |
| `.ryb` | Binary | Compiled module/library | Binary (with reflection) |
| `.exe`/etc | Executable | Standalone binary | Binary (native) |

**Key Design Principles:**

- **Unified binary format:** `.ryb` serves as both compiled module and library format
- **Mandatory reflection:** All type definitions are always included in binaries
- **Dual-purpose binaries:**
  - Binary with `main()` function → can be executed directly
  - Binary with public exports → can be imported as a library
- **No separate library archives:** Eliminates the complexity of `.rlib` files

---

## Source Files (.raya)

### Format

UTF-8 encoded text files containing Raya source code.

### Syntax

TypeScript-compatible syntax (strict subset). See [LANG.md](LANG.md) for complete language specification.

### Example

```typescript
// hello.raya
function main(): void {
  logger.info("Hello, Raya!");
}
```

### Characteristics

- **Encoding:** UTF-8
- **Line endings:** LF (`\n`) or CRLF (`\r\n`)
- **BOM:** Optional (discouraged)
- **Max file size:** Unlimited (practical limit: ~10 MB)

### Compatibility

Files with `.ts` extension are also accepted for TypeScript compatibility, but:
- Must conform to Raya's strict subset rules
- No banned features (`typeof`, `instanceof`, `any`, etc.)
- Type checking follows Raya semantics

---

## Binary Files (.ryb)

### Format Specification

Binary format containing compiled Raya bytecode with mandatory reflection metadata.

### File Structure

```
┌─────────────────────────────────────────┐
│ Header                                  │
├─────────────────────────────────────────┤
│ Constant Pool                           │
├─────────────────────────────────────────┤
│ Function Table                          │
├─────────────────────────────────────────┤
│ Class Table                             │
├─────────────────────────────────────────┤
│ Bytecode Sections                       │
├─────────────────────────────────────────┤
│ Type Metadata (mandatory)               │
├─────────────────────────────────────────┤
│ Export Table                            │
└─────────────────────────────────────────┘
```

### Header (16 bytes)

```rust
struct Header {
    magic: [u8; 4],      // "RAYA" (0x52 0x41 0x59 0x41)
    version: u32,        // Bytecode version (currently 1)
    flags: u32,          // Feature flags
    checksum: u32,       // CRC32 of file contents
}
```

#### Flags

```
Bit 0: Has main() entry point
Bit 1: Debug build
Bit 2: Optimized
Bit 3: Has public exports
Bit 4-31: Reserved
```

**Note:** Reflection metadata is always included (not optional).

### Constant Pool

```rust
struct ConstantPool {
    string_count: u32,
    strings: [String],    // UTF-8 strings with u32 length prefix

    integer_count: u32,
    integers: [i32],      // 32-bit signed integers

    float_count: u32,
    floats: [f64],        // 64-bit IEEE 754 floats
}
```

### Function Table

```rust
struct FunctionTable {
    count: u32,
    functions: [Function],
}

struct Function {
    name_idx: u32,        // Index into string constant pool
    param_count: u16,
    local_count: u16,
    code_offset: u32,     // Offset in bytecode section
    code_length: u32,     // Length in bytes
    flags: u32,           // Function flags
}
```

#### Function Flags

```
Bit 0: Is async
Bit 1: Is generator
Bit 2: Is exported
Bit 3: Is entry point (main)
Bit 4-31: Reserved
```

### Class Table

```rust
struct ClassTable {
    count: u32,
    classes: [Class],
}

struct Class {
    name_idx: u32,        // Index into string constant pool
    field_count: u16,
    method_count: u16,
    vtable_offset: u32,   // Offset to vtable
    fields: [Field],
    methods: [u32],       // Function indices
}

struct Field {
    name_idx: u32,
    type_idx: u32,        // Optional type info index
    flags: u32,
}
```

### Bytecode Sections

Raw bytecode instructions. See [OPCODE.md](OPCODE.md) for instruction set.

```rust
struct BytecodeSection {
    length: u32,
    code: [u8],           // Actual bytecode
}
```

### Type Metadata Section (Mandatory)

Complete type information and debug data for all types, functions, and classes.

```rust
struct TypeMetadata {
    // Type definitions
    type_count: u32,
    types: [TypeDefinition],

    // Interface definitions
    interface_count: u32,
    interfaces: [InterfaceDefinition],

    // Source mapping (optional, for debug builds)
    line_info: Vec<LineInfo>,
    source_file: Option<String>,
}

struct TypeDefinition {
    name_idx: u32,              // Index into string pool
    kind: TypeKind,             // Primitive, Class, Interface, Union, etc.
    properties: Vec<Property>,
    methods: Vec<MethodSignature>,
}

struct LineInfo {
    bytecode_offset: u32,
    source_line: u32,
    source_column: u16,
}
```

**Key Points:**
- Type metadata is **always** included (not optional)
- Enables runtime reflection and type introspection
- Required for importing binaries as libraries
- Supports dynamic type checking when needed
- Debug source mapping controlled by build flags

### Export Table

Public API exports for library usage.

```rust
struct ExportTable {
    export_count: u32,
    exports: [Export],
}

struct Export {
    name_idx: u32,        // Index into string pool
    kind: ExportKind,     // Function, Class, Constant, Type
    target_idx: u32,      // Index into appropriate table
    flags: u32,
}

enum ExportKind {
    Function = 0,
    Class = 1,
    Constant = 2,
    Type = 3,
}
```

### Example

```bash
# Compile to binary
$ raya build hello.raya
# Creates: dist/hello.ryb

# Binary with main() can be executed directly
$ raya run dist/hello.ryb
Hello, Raya!

# Inspect binary structure
$ xxd dist/hello.ryb | head
00000000: 5241 5941 0100 0000 0000 0001 1234 5678  RAYA.........4Vx
...

# View exported API (for library binaries)
$ raya inspect dist/mylib.ryb
Exports:
  - function add(a: number, b: number): number
  - class Calculator { ... }
  - type Result<T, E> = ...
```

### Verification

Binary files are verified on load:
1. Magic number check
2. Version compatibility
3. Checksum validation
4. Structural validation (bounds checks, etc.)

### Dual-Purpose Binaries

`.ryb` files serve two purposes depending on their contents:

#### Executable Binaries (with main)

```typescript
// hello.raya - Has main() function
function main(): void {
  logger.info("Hello, Raya!");
}
```

```bash
# Compile
$ raya build hello.raya
# Creates: hello.ryb with "Has main() entry point" flag set

# Execute directly
$ raya run hello.ryb
Hello, Raya!

# Or bundle as standalone executable
$ raya bundle hello.raya -o hello
$ ./hello
Hello, Raya!
```

#### Library Binaries (with exports)

```typescript
// math.raya - No main(), but has exports
export function add(a: number, b: number): number {
  return a + b;
}

export class Calculator {
  multiply(a: number, b: number): number {
    return a * b;
  }
}
```

```bash
# Compile
$ raya build math.raya
# Creates: math.ryb with "Has public exports" flag set

# Import in other code
```

```typescript
// app.raya - Imports from math.ryb
import { add, Calculator } from "./math.ryb";

function main(): void {
  logger.info(add(2, 3));  // 5

  const calc = new Calculator();
  logger.info(calc.multiply(4, 5));  // 20
}
```

#### Hybrid Binaries (both main and exports)

```typescript
// utils.raya - Has both main() and exports
export function helper(): string {
  return "Useful!";
}

function main(): void {
  logger.info("Running utils directly");
  logger.info(helper());
}
```

```bash
# Can be executed
$ raya run utils.ryb
Running utils directly
Useful!

# Can also be imported by other modules
import { helper } from "./utils.ryb";
```

### Compilation Options

```bash
# Standard build (with reflection, always included)
$ raya build module.raya

# Debug build (includes source mapping)
$ raya build module.raya --debug

# Release build (optimizations enabled)
$ raya build module.raya --release

# Specify output path
$ raya build module.raya -o dist/module.ryb
```

---

## Executable Bundles

### Format Specification

Platform-specific native executables with embedded Raya VM runtime and compiled bytecode.

### Structure

```
┌─────────────────────────────────────────┐
│ Native Executable Header                │
│ (ELF/PE/Mach-O)                        │
├─────────────────────────────────────────┤
│ Raya VM Runtime (embedded)              │
├─────────────────────────────────────────┤
│ Application Binary (.ryb)              │
├─────────────────────────────────────────┤
│ Bundled Dependencies (.ryb)            │
├─────────────────────────────────────────┤
│ Bundle Metadata                         │
└─────────────────────────────────────────┘
```

### Bundle Metadata

```rust
struct BundleMetadata {
    magic: [u8; 4],          // "RBND" (Raya Bundle)
    runtime_version: [u8; 3], // VM version (major.minor.patch)
    bytecode_offset: u64,
    bytecode_size: u64,
    dependencies_offset: u64,
    dependencies_size: u64,
}
```

### Creation

```bash
# Create native bundle
$ raya bundle main.raya -o myapp

# Cross-platform bundle
$ raya bundle main.raya --target windows -o myapp.exe
$ raya bundle main.raya --target linux -o myapp
$ raya bundle main.raya --target macos -o myapp

# WebAssembly bundle
$ raya bundle main.raya --target wasm -o myapp.wasm
```

### Platform-Specific Formats

#### Linux (ELF)

```bash
$ file myapp
myapp: ELF 64-bit LSB executable, x86-64, dynamically linked
```

#### Windows (PE)

```bash
$ file myapp.exe
myapp.exe: PE32+ executable (console) x86-64, for MS Windows
```

#### macOS (Mach-O)

```bash
$ file myapp
myapp: Mach-O 64-bit executable x86_64
```

#### WebAssembly

```bash
$ file myapp.wasm
myapp.wasm: WebAssembly (wasm) binary module version 0x1 (MVP)
```

### Size Optimization

| Optimization | Size Impact |
|--------------|-------------|
| Base runtime | ~2-3 MB |
| With stdlib | +2-3 MB |
| Reflection metadata | +500 KB (always included) |
| Debug symbols | +1-2 MB |
| Stripped | -1-2 MB |
| Compressed (UPX) | -50-70% |

**Note:** Reflection metadata is always included in all binaries (not optional).

### Example Sizes

```bash
# Minimal bundle (stripped)
$ raya bundle hello.raya --release --strip -o hello
$ ls -lh hello
-rwxr-xr-x  2.3M  hello

# With compression
$ raya bundle hello.raya --release --strip --compress -o hello
$ ls -lh hello
-rwxr-xr-x  1.0M  hello

# Full bundle with debug symbols
$ raya bundle app.raya --debug -o app
$ ls -lh app
-rwxr-xr-x  7.5M  app
```

---

## File Extension Summary

```
.raya  → Source code (TypeScript syntax)
.ts    → Source code (TypeScript compatibility)
.ryb  → Compiled binary (executable or library, with reflection)
.exe   → Windows executable bundle
(none) → Linux/macOS executable bundle
.wasm  → WebAssembly bundle
```

**Key Points:**
- `.ryb` replaces both `.rbc` and `.rlib` - single unified format
- All `.ryb` files include complete reflection metadata (mandatory)
- Binary with `main()` → can be executed
- Binary with exports → can be imported as library
- Binary can have both `main()` and exports (dual-purpose)

---

## Caching

### Binary Cache Location

```
~/.cache/raya/
├── binaries/
│   ├── <hash>.ryb      # Cached compiled binaries
│   └── ...
└── metadata/
    └── cache.db        # Cache metadata
```

### Cache Key

```rust
fn cache_key(source_path: &Path) -> String {
    let content = fs::read_to_string(source_path)?;
    let metadata = fs::metadata(source_path)?;

    // Hash: source content + mtime + compiler version
    let hash = blake3::hash(&[
        content.as_bytes(),
        &metadata.modified()?.to_bytes(),
        COMPILER_VERSION.as_bytes(),
    ].concat());

    format!("{:x}", hash)
}
```

### Cache Invalidation

Cached binaries are invalidated when:
- Source file modified
- Compiler version changed
- Dependencies updated
- Build flags changed

---

## Compatibility

### Bytecode Version Compatibility

| VM Version | Bytecode Version | Compatible |
|------------|------------------|------------|
| 0.1.x | 1 | ✅ |
| 0.2.x | 1-2 | ✅ |
| 1.0.x | 1-3 | ✅ |

### Library Version Compatibility

Libraries follow semantic versioning:
- **Major version:** Breaking changes
- **Minor version:** New features (backward compatible)
- **Patch version:** Bug fixes

---

**Status:** Design Document
**Version:** v0.2
**Last Updated:** 2026-01-05

**Key Changes in v0.2:**
- Replaced `.rbc` and `.rlib` with unified `.ryb` format
- Made reflection metadata mandatory (always included)
- Added dual-purpose binary support (executable + library)
- Simplified file format system
