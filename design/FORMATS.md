# Raya File Formats (v0.1)

Specification for all file formats used in the Raya ecosystem.

---

## Table of Contents

1. [Overview](#overview)
2. [Source Files (.raya)](#source-files-raya)
3. [Bytecode Files (.rbc)](#bytecode-files-rbc)
4. [Library Archives (.rlib)](#library-archives-rlib)
5. [Executable Bundles](#executable-bundles)

---

## Overview

Raya uses distinct file formats for different stages of the development pipeline:

| Extension | Type | Purpose | Format |
|-----------|------|---------|--------|
| `.raya` | Source | Human-readable code | UTF-8 text |
| `.ts` | Source | TypeScript compatibility | UTF-8 text |
| `.rbc` | Bytecode | Compiled module | Binary |
| `.rlib` | Library | Package archive | Binary (archive) |
| `.exe`/etc | Executable | Standalone binary | Binary (native) |

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
  console.log("Hello, Raya!");
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

## Bytecode Files (.rbc)

### Format Specification

Binary format containing compiled Raya bytecode.

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
│ Metadata (optional)                     │
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
Bit 0: Has reflection metadata
Bit 1: Debug build
Bit 2: Optimized
Bit 3-31: Reserved
```

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

### Metadata Section (Optional)

Debug information and reflection data.

```rust
struct Metadata {
    source_file: Option<String>,
    line_info: Vec<LineInfo>,
    type_info: Option<TypeMetadata>,
}

struct LineInfo {
    bytecode_offset: u32,
    source_line: u32,
    source_column: u16,
}
```

### Example

```bash
# Compile to bytecode
$ raya build hello.raya
# Creates: dist/hello.rbc

# Inspect bytecode
$ xxd dist/hello.rbc | head
00000000: 5241 5941 0100 0000 0000 0001 1234 5678  RAYA.........4Vx
...
```

### Verification

Bytecode files are verified on load:
1. Magic number check
2. Version compatibility
3. Checksum validation
4. Structural validation (bounds checks, etc.)

---

## Library Archives (.rlib)

### Format Specification

Archive format containing multiple compiled modules with metadata.

### File Structure

```
┌─────────────────────────────────────────┐
│ Archive Header                          │
├─────────────────────────────────────────┤
│ Manifest                                │
├─────────────────────────────────────────┤
│ Module 1 (.rbc)                         │
├─────────────────────────────────────────┤
│ Module 2 (.rbc)                         │
├─────────────────────────────────────────┤
│ ...                                     │
├─────────────────────────────────────────┤
│ Type Definitions                        │
├─────────────────────────────────────────┤
│ Documentation                           │
└─────────────────────────────────────────┘
```

### Archive Header

```rust
struct LibHeader {
    magic: [u8; 4],      // "RLIB" (0x52 0x4C 0x49 0x42)
    version: u32,        // Library format version
    manifest_offset: u64,
    manifest_size: u64,
}
```

### Manifest

JSON or binary format containing package metadata:

```json
{
  "name": "mylib",
  "version": "1.0.0",
  "modules": [
    {
      "name": "index",
      "offset": 1024,
      "size": 4096,
      "exports": ["function1", "class1"]
    },
    {
      "name": "utils",
      "offset": 5120,
      "size": 2048,
      "exports": ["helper"]
    }
  ],
  "dependencies": {
    "lodash": "^4.17.0"
  }
}
```

### Compilation

```bash
# Build library from directory
$ raya build --lib src/
# Creates: dist/mylib.rlib

# Build with specific name
$ raya build --lib src/ -o mylib.rlib
```

### Usage in Code

```typescript
// Import from library
import { helper } from "./mylib.rlib";
import * as lib from "./mylib.rlib";
```

### Characteristics

- **Self-contained:** Includes all compiled modules
- **Versioned:** Semantic versioning in manifest
- **Discoverable:** Public API in manifest
- **Optimized:** Dead code eliminated
- **Portable:** Binary-compatible across platforms

---

## Executable Bundles

### Format

Platform-specific executable formats with embedded Raya runtime and bytecode.

### Structure

```
┌─────────────────────────────────────────┐
│ Native Executable Header                │
│ (ELF/PE/Mach-O)                        │
├─────────────────────────────────────────┤
│ Raya VM Runtime (embedded)              │
├─────────────────────────────────────────┤
│ Application Bytecode (.rbc)             │
├─────────────────────────────────────────┤
│ Bundled Dependencies (.rlib)            │
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
| With reflection | +500 KB |
| Debug symbols | +1-2 MB |
| Stripped | -1-2 MB |
| Compressed (UPX) | -50-70% |

### Example Sizes

```bash
# Minimal bundle (stripped)
$ raya bundle hello.raya --release --strip -o hello
$ ls -lh hello
-rwxr-xr-x  1.8M  hello

# With compression
$ raya bundle hello.raya --release --strip --compress -o hello
$ ls -lh hello
-rwxr-xr-x  800K  hello

# Full bundle with reflection
$ raya bundle app.raya --emit-reflection -o app
$ ls -lh app
-rwxr-xr-x  7.2M  app
```

---

## File Extension Summary

```
.raya  → Source code (TypeScript syntax)
.ts    → Source code (TypeScript compatibility)
.rbc   → Compiled bytecode module
.rlib  → Library archive (multiple modules)
.exe   → Windows executable bundle
(none) → Linux/macOS executable bundle
.wasm  → WebAssembly bundle
```

---

## Caching

### Bytecode Cache Location

```
~/.cache/raya/
├── bytecode/
│   ├── <hash>.rbc      # Cached bytecode
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

Cached bytecode is invalidated when:
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
**Version:** v0.1
**Last Updated:** 2026-01-04
