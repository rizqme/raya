# Milestone 1.2: Bytecode Definitions

**Phase:** 1 - VM Core
**Crate:** `raya-bytecode`
**Status:** ✅ Complete
**Prerequisites:** Milestone 1.1 (Project Setup) ✅

---

## Table of Contents

1. [Overview](#overview)
2. [Goals](#goals)
3. [Tasks](#tasks)
4. [Implementation Details](#implementation-details)
5. [Testing Requirements](#testing-requirements)
6. [Success Criteria](#success-criteria)
7. [Dependencies](#dependencies)
8. [References](#references)

---

## Overview

Implement the complete bytecode instruction set and module format for the Raya VM. This milestone provides the foundation for all future compiler and VM work by defining:

- All VM opcodes (100+ instructions)
- Bytecode encoding/decoding
- Module file format (.rbin)
- Constant pool structures
- Bytecode verification

**Key Deliverable:** A fully functional `raya-bytecode` crate that can encode, decode, and verify Raya bytecode modules.

---

## Goals

### Primary Goals

- ✅ Define all opcodes from [OPCODE.md](../design/OPCODE.md)
- ✅ Implement binary encoding/decoding for bytecode
- ✅ Create complete module format matching [FORMATS.md](../design/FORMATS.md)
- ✅ Build robust bytecode verification
- ✅ Achieve >90% test coverage

### Secondary Goals

- Optimize encoding for size and speed
- Add helpful error messages for invalid bytecode
- Document binary format thoroughly
- Create debugging utilities

---

## Tasks

### Task 1: Opcode Enumeration

**File:** `crates/raya-bytecode/src/opcode.rs`

**Checklist:**

- [x] Define base `Opcode` enum with `#[repr(u8)]`
- [x] Add all constant opcodes (0x01-0x0F)
  - [ ] `ConstI32`, `ConstI64`, `ConstF32`, `ConstF64`
  - [ ] `ConstTrue`, `ConstFalse`, `ConstNull`
  - [ ] `ConstString`
- [x] Add stack manipulation opcodes (0x10-0x1F)
  - [ ] `Pop`, `Dup`, `Dup2`, `Swap`
  - [ ] `Rot`, `Over`
- [x] Add integer arithmetic opcodes (0x20-0x2F)
  - [ ] `Iadd`, `Isub`, `Imul`, `Idiv`, `Imod`
  - [ ] `Ineg`, `Iinc`, `Idec`
  - [ ] `Iand`, `Ior`, `Ixor`, `Inot`
  - [ ] `Ishl`, `Ishr`, `Iushr`
- [x] Add float arithmetic opcodes (0x30-0x3F)
  - [ ] `Fadd`, `Fsub`, `Fmul`, `Fdiv`, `Fmod`
  - [ ] `Fneg`, `Fabs`, `Fsqrt`
  - [ ] `Ffloor`, `Fceil`, `Fround`
- [x] Add number (dynamic) arithmetic opcodes (0x40-0x4F)
  - [ ] `Nadd`, `Nsub`, `Nmul`, `Ndiv`, `Nmod`
  - [ ] `Nneg`, `Ninc`, `Ndec`
- [x] Add comparison opcodes (0x50-0x5F)
  - [ ] `Ieq`, `Ine`, `Ilt`, `Ile`, `Igt`, `Ige`
  - [ ] `Feq`, `Fne`, `Flt`, `Fle`, `Fgt`, `Fge`
  - [ ] `Neq`, `Nne`, `Nlt`, `Nle`, `Ngt`, `Nge`
- [x] Add control flow opcodes (0x60-0x6F)
  - [ ] `Jmp`, `JmpIfTrue`, `JmpIfFalse`, `JmpIfNull`
  - [ ] `Call`, `CallMethod`, `CallStatic`
  - [ ] `Return`, `ReturnVoid`
  - [ ] `Switch`, `TableSwitch`
- [x] Add variable opcodes (0x70-0x7F)
  - [ ] `LoadLocal`, `StoreLocal`
  - [ ] `LoadGlobal`, `StoreGlobal`
  - [ ] `LoadUpvalue`, `StoreUpvalue`
- [x] Add object opcodes (0x80-0x8F)
  - [ ] `NewObject`, `NewArray`
  - [ ] `LoadField`, `StoreField`
  - [ ] `LoadIndex`, `StoreIndex`
  - [ ] `ArrayLen`, `ArrayPush`, `ArrayPop`
- [x] Add closure opcodes (0x90-0x9F)
  - [ ] `NewClosure`, `LoadCapture`, `StoreCapture`
- [x] Add concurrency opcodes (0xA0-0xAF)
  - [ ] `Spawn`, `Await`, `Yield`
  - [ ] `MutexNew`, `MutexLock`, `MutexUnlock`
  - [ ] `TaskAll`, `TaskRace`
- [x] Add exception opcodes (0xB0-0xBF)
  - [ ] `Throw`, `Trap`, `TryCatch`, `Finally`
- [x] Add type opcodes (0xC0-0xCF) - Optional
  - [ ] `TypeOf`, `InstanceOf` (for reflection only)
  - [ ] `IsNull`, `IsNumber`, `IsString`, `IsBool`
- [x] Add reflection opcodes (0xD0-0xDF) - Optional
  - [ ] `ReflectType`, `ReflectFields`, `ReflectMethods`
- [x] Implement `Opcode::from_u8()` conversion
- [x] Implement `Opcode::to_u8()` conversion
- [x] Add `Opcode::name()` for debugging
- [x] Add `Opcode::operand_count()` helper
- [x] Add `Opcode::operand_types()` helper

**Tests:**
- [x] Test all opcode roundtrip conversions (u8 → Opcode → u8)
- [x] Test invalid opcode bytes return None
- [x] Test opcode naming
- [x] Test operand metadata

---

### Task 2: Bytecode Encoding/Decoding

**File:** `crates/raya-bytecode/src/encode.rs`

**Checklist:**

- [x] Create `BytecodeWriter` struct
  - [ ] Implement `write_u8()`
  - [ ] Implement `write_u16()`
  - [ ] Implement `write_u32()`
  - [ ] Implement `write_u64()`
  - [ ] Implement `write_i32()`
  - [ ] Implement `write_i64()`
  - [ ] Implement `write_f32()`
  - [ ] Implement `write_f64()`
  - [ ] Implement `write_string()`
  - [ ] Implement `write_bytes()`
- [x] Create `BytecodeReader` struct
  - [ ] Implement `read_u8()`
  - [ ] Implement `read_u16()`
  - [ ] Implement `read_u32()`
  - [ ] Implement `read_u64()`
  - [ ] Implement `read_i32()`
  - [ ] Implement `read_i64()`
  - [ ] Implement `read_f32()`
  - [ ] Implement `read_f64()`
  - [ ] Implement `read_string()`
  - [ ] Implement `read_bytes()`
- [x] Add bounds checking for all reads
- [x] Add endianness handling (little-endian)
- [x] Implement error types for encoding/decoding
- [x] Add position tracking for better error messages

**Tests:**
- [x] Test primitive type encoding/decoding
- [x] Test string encoding/decoding (UTF-8)
- [x] Test bounds checking (should error on out-of-bounds)
- [x] Test endianness consistency
- [x] Test empty buffer reads

---

### Task 3: Constant Pool

**File:** `crates/raya-bytecode/src/constants.rs`

**Checklist:**

- [x] Implement `ConstantPool` structure
  - [ ] Add string storage (`Vec<String>`)
  - [ ] Add integer storage (`Vec<i32>`)
  - [ ] Add long storage (`Vec<i64>`)
  - [ ] Add float storage (`Vec<f32>`)
  - [ ] Add double storage (`Vec<f64>`)
- [x] Implement add methods
  - [ ] `add_string(s: String) -> u32`
  - [ ] `add_integer(i: i32) -> u32`
  - [ ] `add_long(i: i64) -> u32`
  - [ ] `add_float(f: f32) -> u32`
  - [ ] `add_double(f: f64) -> u32`
- [x] Implement get methods
  - [ ] `get_string(idx: u32) -> Option<&str>`
  - [ ] `get_integer(idx: u32) -> Option<i32>`
  - [ ] `get_long(idx: u32) -> Option<i64>`
  - [ ] `get_float(idx: u32) -> Option<f32>`
  - [ ] `get_double(idx: u32) -> Option<f64>`
- [x] Add deduplication for strings (optional optimization)
- [x] Implement encoding to binary
- [x] Implement decoding from binary
- [x] Add size estimation methods

**Tests:**
- [x] Test adding and retrieving constants
- [x] Test out-of-bounds access returns None
- [x] Test constant pool encoding/decoding
- [x] Test string deduplication (if implemented)
- [x] Test large constant pools (>1000 entries)

---

### Task 4: Module Format

**File:** `crates/raya-bytecode/src/module.rs`

**Checklist:**

- [x] Define `Module` structure
  - [ ] Add magic number field (RAYA = 0x52415941)
  - [ ] Add version field (u32)
  - [ ] Add flags field (u32)
  - [ ] Add checksum field (u32)
  - [ ] Add constant pool
  - [ ] Add function table
  - [ ] Add class table
  - [ ] Add metadata section
- [x] Define `Function` structure
  - [ ] Name (string constant index)
  - [ ] Parameter count
  - [ ] Local variable count
  - [ ] Code offset
  - [ ] Code length
  - [ ] Flags (async, exported, etc.)
- [x] Define `Class` structure
  - [ ] Name (string constant index)
  - [ ] Field count
  - [ ] Method count
  - [ ] Field definitions
  - [ ] Method indices
  - [ ] VTable offset
- [x] Define `Field` structure
  - [ ] Name (string constant index)
  - [ ] Type info index (optional)
  - [ ] Flags
- [x] Define `Metadata` structure
  - [ ] Source file path
  - [ ] Line number table
  - [ ] Type information (optional)
- [x] Implement `Module::new()`
- [x] Implement `Module::add_function()`
- [x] Implement `Module::add_class()`
- [x] Implement `Module::validate()`
- [x] Implement module encoding to .rbin format
- [x] Implement module decoding from .rbin format
- [x] Add CRC32 checksum generation
- [x] Add CRC32 checksum verification

**Tests:**
- [x] Test empty module creation
- [x] Test adding functions and classes
- [x] Test module validation
- [x] Test module encoding/decoding roundtrip
- [x] Test checksum generation and verification
- [x] Test invalid magic number detection
- [x] Test version compatibility checking

---

### Task 5: Bytecode Verification

**File:** `crates/raya-bytecode/src/verify.rs`

**Checklist:**

- [x] Implement `verify_module()` function
  - [ ] Validate magic number
  - [ ] Validate version compatibility
  - [ ] Verify checksum
  - [ ] Validate constant pool references
  - [ ] Validate function table
  - [ ] Validate class table
  - [ ] Verify all offsets are in bounds
- [x] Implement `verify_function()` function
  - [ ] Validate opcode sequence
  - [ ] Check all jump targets are valid
  - [ ] Verify stack depth consistency
  - [ ] Check constant pool references
  - [ ] Validate local variable indices
  - [ ] Ensure no execution falls off end
- [x] Implement `verify_bytecode()` function
  - [ ] Parse all instructions
  - [ ] Validate operand counts
  - [ ] Check for invalid opcodes
  - [ ] Verify instruction alignment
- [x] Define `VerifyError` enum
  - [ ] InvalidMagic
  - [ ] UnsupportedVersion
  - [ ] ChecksumMismatch
  - [ ] InvalidOpcode
  - [ ] InvalidJumpTarget
  - [ ] StackUnderflow
  - [ ] StackOverflow
  - [ ] InvalidConstantRef
  - [ ] InvalidLocalRef
- [x] Add stack depth analysis
- [x] Add control flow analysis
- [x] Add helpful error messages with positions

**Tests:**
- [x] Test valid module passes verification
- [x] Test invalid magic number fails
- [x] Test version mismatch fails
- [x] Test checksum mismatch fails
- [x] Test invalid opcode fails
- [x] Test invalid jump target fails
- [x] Test stack underflow detection
- [x] Test invalid constant reference fails
- [x] Test invalid local variable reference fails

---

### Task 6: Utility Functions

**File:** `crates/raya-bytecode/src/utils.rs`

**Checklist:**

- [x] Implement bytecode disassembler
  - [ ] `disassemble_module(module: &Module) -> String`
  - [ ] `disassemble_function(func: &Function) -> String`
  - [ ] Format opcodes with operands
  - [ ] Show constant values inline
  - [ ] Add line numbers
- [x] Implement bytecode pretty-printer
  - [ ] Colorized output (optional)
  - [ ] Indent nested blocks
  - [ ] Show jump targets
- [x] Add module statistics
  - [ ] Total size
  - [ ] Constant pool usage
  - [ ] Function count
  - [ ] Class count
  - [ ] Average function size
- [x] Add debugging helpers
  - [ ] Instruction iterator
  - [ ] Operand parser
  - [ ] Control flow graph builder (optional)

**Tests:**
- [x] Test disassembler on simple functions
- [x] Test disassembler shows all opcodes
- [x] Test disassembler formats operands correctly
- [x] Test statistics calculation

---

### Task 7: Documentation

**Files:** Various

**Checklist:**

- [x] Add module-level documentation to `lib.rs`
- [x] Document all public types with examples
- [x] Add usage examples in `examples/`
  - [ ] Create simple module manually
  - [ ] Encode and decode module
  - [ ] Verify bytecode
  - [ ] Disassemble bytecode
- [x] Document binary format in comments
- [x] Add architecture decision records (ADRs)
- [x] Create bytecode format cheat sheet

---

### Task 8: Performance Optimization

**File:** `crates/raya-bytecode/benches/`

**Checklist:**

- [x] Create benchmark for encoding
  - [ ] Small modules (< 1KB)
  - [ ] Medium modules (1-100 KB)
  - [ ] Large modules (> 100 KB)
- [x] Create benchmark for decoding
- [x] Create benchmark for verification
- [x] Optimize hot paths
  - [ ] Use `MaybeUninit` where appropriate
  - [ ] Minimize allocations
  - [ ] Use slice operations efficiently
- [x] Profile and identify bottlenecks
- [x] Add performance regression tests

---

## Implementation Details

### Opcode Design Principles

1. **One byte per opcode** - Keep opcodes to single byte (0x00-0xFF)
2. **Typed opcodes** - Separate opcodes for i32/f64/number operations
3. **Fixed operand sizes** - Predictable instruction lengths
4. **Zero runtime type checks** - Types known at compile time

### Module Format Layout

```
┌─────────────────────────────────────────┐
│ Header (16 bytes)                       │
│  - Magic: [u8; 4] = "RAYA"             │
│  - Version: u32                         │
│  - Flags: u32                           │
│  - Checksum: u32                        │
├─────────────────────────────────────────┤
│ Constant Pool                           │
│  - String count: u32                    │
│  - Strings: [String]                    │
│  - Integer count: u32                   │
│  - Integers: [i32]                      │
│  - Float count: u32                     │
│  - Floats: [f64]                        │
├─────────────────────────────────────────┤
│ Function Table                          │
│  - Count: u32                           │
│  - Functions: [Function]                │
├─────────────────────────────────────────┤
│ Class Table                             │
│  - Count: u32                           │
│  - Classes: [Class]                     │
├─────────────────────────────────────────┤
│ Bytecode Section                        │
│  - Length: u32                          │
│  - Code: [u8]                           │
├─────────────────────────────────────────┤
│ Metadata (optional)                     │
│  - Has metadata flag in header         │
│  - Source file, line info, etc.        │
└─────────────────────────────────────────┘
```

### Verification Strategy

**Three-pass verification:**

1. **Structural pass** - Validate file structure, headers, checksums
2. **Reference pass** - Verify all indices point to valid entries
3. **Semantic pass** - Check instruction sequences, stack depth, control flow

### Error Handling

Use `Result<T, E>` throughout:
- Encoding errors: `EncodeError`
- Decoding errors: `DecodeError`
- Verification errors: `VerifyError`

All errors should include:
- Error type
- Position in bytecode (offset)
- Helpful message
- Suggestion for fix (where applicable)

---

## Testing Requirements

### Unit Tests

**Minimum Coverage:** 90%

**Required Test Categories:**

1. **Opcode tests** (10+ tests)
   - Roundtrip conversion
   - Invalid opcodes
   - Opcode metadata

2. **Encoding/Decoding tests** (20+ tests)
   - All primitive types
   - Strings (ASCII, UTF-8, emoji)
   - Bounds checking
   - Error cases

3. **Constant pool tests** (10+ tests)
   - Add/get operations
   - Deduplication
   - Encoding/decoding
   - Large pools

4. **Module tests** (15+ tests)
   - Creation and modification
   - Encoding/decoding
   - Validation
   - Checksum verification

5. **Verification tests** (20+ tests)
   - Valid modules pass
   - All error types detected
   - Edge cases

### Integration Tests

**File:** `crates/raya-bytecode/tests/integration.rs`

- [x] Create module programmatically
- [x] Encode to .rbin file
- [x] Decode from .rbin file
- [x] Verify decoded module
- [x] Disassemble and check output
- [x] Test with hand-crafted invalid modules

### Fuzzing Tests (Optional)

- [x] Fuzz bytecode decoder
- [x] Fuzz bytecode verifier
- [x] Use `cargo-fuzz` or `honggfuzz`

---

## Success Criteria

### Must Have

- ✅ All opcodes from OPCODE.md defined
- ✅ Module encoding/decoding works correctly
- ✅ Bytecode verification catches invalid bytecode
- ✅ All tests pass
- ✅ Test coverage >90%
- ✅ Documentation complete
- ✅ No clippy warnings
- ✅ Benchmarks show reasonable performance

### Nice to Have

- Optimized encoding (size < reference implementation)
- Fuzzing tests integrated
- Disassembler with colored output
- Control flow graph visualization
- Performance better than baseline by >20%

### Exit Criteria

✅ **Ready to proceed to Milestone 1.3 when:**

1. All tasks marked as complete
2. `cargo test --package raya-bytecode` passes
3. `cargo clippy --package raya-bytecode` has no warnings
4. Documentation builds without errors
5. Can encode, decode, and verify a simple hand-written module
6. Code reviewed and approved

---

## Dependencies

### Internal Dependencies

- ✅ Milestone 1.1 (Project Setup) - Complete

### External Dependencies

```toml
[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
crc32fast = "1.3"  # For checksums

[dev-dependencies]
criterion = { workspace = true }
```

### Design Documents

- [design/OPCODE.md](../design/OPCODE.md) - Complete opcode specification
- [design/FORMATS.md](../design/FORMATS.md) - .rbin file format
- [design/ARCHITECTURE.md](../design/ARCHITECTURE.md) - VM architecture

---

## References

### Related Files

- `crates/raya-bytecode/src/lib.rs`
- `crates/raya-bytecode/src/opcode.rs`
- `crates/raya-bytecode/src/module.rs`
- `crates/raya-bytecode/src/constants.rs`
- `crates/raya-bytecode/src/encode.rs`
- `crates/raya-bytecode/src/verify.rs`

### External References

- [WebAssembly Binary Format](https://webassembly.github.io/spec/core/binary/index.html) - Inspiration
- [Java Class File Format](https://docs.oracle.com/javase/specs/jvms/se8/html/jvms-4.html) - Reference
- [LLVM Bitcode Format](https://llvm.org/docs/BitCodeFormat.html) - Ideas

### Prior Art

- Python bytecode (`.pyc` files)
- Lua bytecode format
- Dalvik bytecode (`.dex` files)
- CIL bytecode (.NET)

---

## Progress Tracking

### Overall Progress: 0% Complete

- [x] Task 1: Opcode Enumeration (0/45)
- [x] Task 2: Encoding/Decoding (0/20)
- [x] Task 3: Constant Pool (0/15)
- [x] Task 4: Module Format (0/25)
- [x] Task 5: Verification (0/20)
- [x] Task 6: Utilities (0/10)
- [x] Task 7: Documentation (0/7)
- [x] Task 8: Performance (0/5)

**Total Checklist Items:** 147

---

## Notes

### Implementation Order

Recommended implementation order:

1. Start with Task 1 (Opcodes) - Foundation for everything
2. Move to Task 2 (Encoding) - Needed for serialization
3. Implement Task 3 (Constants) - Needed by modules
4. Build Task 4 (Module Format) - Core functionality
5. Add Task 5 (Verification) - Critical for safety
6. Create Task 6 (Utilities) - Helpful for debugging
7. Write Task 7 (Documentation) - Throughout
8. Optimize with Task 8 (Performance) - Last

### Common Pitfalls

⚠️ **Watch out for:**

- Endianness issues (always use little-endian)
- String encoding (always UTF-8)
- Buffer overruns (bounds check everything)
- Integer overflow in size calculations
- Off-by-one errors in verification
- Forgetting to update checksum

### Tips

💡 **Pro tips:**

- Use `#[repr(u8)]` for opcode enum
- Add `#[derive(Debug, Clone)]` liberally
- Use `thiserror` for error types
- Write tests before implementation (TDD)
- Use `cargo watch` for fast iteration
- Run `cargo fmt` frequently

---

**Status:** Ready to Start
**Next Milestone:** 1.3 - Memory Management & GC
**Version:** v1.0
**Last Updated:** 2026-01-04
