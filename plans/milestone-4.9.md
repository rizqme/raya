# Milestone 4.9: Native ABI Refactoring

**Goal:** Refactor the NativeHandler interface from string-based to unified ABI with full VM context access.

**Status:** In Progress (Phases 1-4 Complete, Phase 5 In Progress)
**Design:** [design/ABI.md](../design/ABI.md)

---

## Overview

Migrate from the old string-based native handler interface to a unified ABI that provides:
- Type-safe value access (no string parsing)
- Full VM context (GC, classes, scheduler)
- Support for all value types (primitives, strings, buffers, objects, arrays)
- Clear error handling (Result-based)

**Key Change:**
```rust
// OLD: String-based, primitive returns only
fn call(&self, id: u16, args: &[String]) -> NativeCallResult;

// NEW: Unified ABI with full VM access
fn call(&self, ctx: &NativeContext, id: u16, args: &[NativeValue]) -> NativeCallResult;
```

---

## Phase 1: Core ABI Infrastructure ✅

**Status:** Complete

### Tasks
- [x] Define `NativeContext<'a>` struct with GC, classes, scheduler, current_task
- [x] Define `NativeValue` wrapper around VM Value
- [x] Implement `NativeValue` accessors (as_i32, as_f64, as_bool, is_null, is_ptr)
- [x] Implement buffer operations (buffer_read_bytes, buffer_allocate)
- [x] Implement string operations (string_read, string_allocate)
- [x] Implement array operations (array_get, array_length, array_allocate)
- [x] Implement object operations (object_get_field, object_set_field, object_allocate, object_class_id)
- [x] Implement class registry operations (class_get_info → ClassInfo)
- [x] Stub task scheduler operations (task_spawn, task_cancel, task_is_done)
- [x] Export all ABI functions from vm/mod.rs

### Files Modified
- `crates/raya-engine/src/vm/abi.rs` (new file, 331 lines)
- `crates/raya-engine/src/vm/mod.rs` (added abi module and exports)

---

## Phase 2: Interface Refactor ✅

**Status:** Complete

### Tasks
- [x] Update `NativeHandler` trait signature to `call(ctx, id, args)`
- [x] Simplify `NativeCallResult` to 3 variants (Value, Unhandled, Error)
- [x] Remove old result variants (String, Number, Integer, Bool, Void)
- [x] Add helper constructors (null(), i32(), f64(), bool())
- [x] Update design document with examples and migration guide

### Files Modified
- `crates/raya-engine/src/vm/native_handler.rs` (72 lines, -37 lines)
- `design/ABI.md` (new file, 539 lines - comprehensive design doc)

---

## Phase 3: VM Dispatcher Migration

**Status:** Complete

### Goal
Update the VM interpreter to use the new ABI interface for all native calls.

### Tasks

#### 3.1: Update TaskInterpreter Context Creation
- [x] Locate NativeCall opcode handlers in task_interpreter.rs (2 locations)
- [x] Add scheduler field to TaskInterpreter struct (if not present)
- [x] Create NativeContext with all 4 parameters:
  ```rust
  let ctx = NativeContext::new(
      &self.gc,
      &self.classes,
      &self.scheduler,
      task.id()
  );
  ```

#### 3.2: Convert Arguments to NativeValue
- [x] Replace string conversion with NativeValue wrapping:
  ```rust
  // OLD: let arg_strings = Self::values_to_strings(&args);
  // NEW:
  let native_args: Vec<NativeValue> = args.iter()
      .map(|v| NativeValue::from_value(*v))
      .collect();
  ```

#### 3.3: Update Handler Call Sites
- [x] Replace all `native_handler.call(id, &arg_strings)` with:
  ```rust
  self.native_handler.call(&ctx, id, &native_args)
  ```
- [x] Update result handling for 3 variants (Value, Unhandled, Error)
- [x] Convert NativeValue back to Value for stack push: `val.into_value()`

#### 3.4: Remove Old Code
- [x] Delete `values_to_strings()` helper method
- [x] Remove old result variant handling (Number, String, Bool, Void)
- [x] Clean up any unused imports

### Files to Modify
- `crates/raya-engine/src/vm/vm/task_interpreter.rs`
  - Line ~2762: Old NativeCall handler (opcode loop)
  - Line ~5530: New NativeCall handler (execute_internal)
  - Line ~4000-4055: Logger dispatch
  - Line ~6370-6418: Math dispatch

### Acceptance Criteria
- ✅ All NativeCall opcodes use unified ABI
- ✅ No string conversion in dispatcher
- ✅ Single code path for all native methods
- ✅ All existing tests still pass (1,731 tests)

---

## Phase 4: StdNativeHandler Migration

**Status:** Complete

### Goal
Update raya-stdlib handler to use the new ABI interface.

### Tasks

#### 4.1: Update Handler Signature
- [x] Change `call(&self, id, args)` to `call(&self, ctx, id, args)`
- [x] Update argument access from string parsing to NativeValue:
  ```rust
  // OLD: let x = args[0].parse::<f64>().unwrap_or(0.0);
  // NEW: let x = args[0].as_f64().unwrap_or(0.0);
  ```
- [x] Create `get_f64()` helper to handle both i32 and f64 (Raya numeric literals are i32)

#### 4.2: Update Logger Methods (0x1000-0x1003)
- [x] Use `string_read()` for log messages (with multi-arg support)
- [x] Return `NativeCallResult::null()` instead of `Void`
- [x] Handle conversion errors gracefully

#### 4.3: Update Math Methods (0x2000-0x2016)
- [x] Use `get_f64()` for numeric arguments (handles both i32 and f64)
- [x] Return `NativeCallResult::f64(result)` instead of `Number(result)`
- [x] Update all 23 math methods

#### 4.4: Update Tests
- [x] Update test helper to create NativeContext (mock)
- [x] Convert test args from `&[String]` to `&[NativeValue]`
- [x] Update assertions for new result variants
- [x] Ensure all 4 existing tests pass

### Files to Modify
- `crates/raya-stdlib/src/handler.rs` (190 lines)
- `crates/raya-stdlib/src/handler.rs` tests (lines 153-189)

### Acceptance Criteria
- ✅ Handler uses new signature
- ✅ No string parsing, direct value access
- ✅ All 17 stdlib tests pass
- ✅ E2E tests work (logger, math)

---

## Phase 5: Crypto & Codec Migration to Stdlib

**Status:** Not Started

### Goal
Move crypto and codec handlers from engine to stdlib using ABI.

### Tasks

#### 5.1: Create Crypto Module in Stdlib
- [ ] Create `crates/raya-stdlib/src/crypto.rs`
- [ ] Add crypto crate dependencies to Cargo.toml (sha2, hmac, md5, sha1, etc.)
- [ ] Implement crypto operations using ABI:
  - `string_read()` for algorithm names
  - `buffer_read_bytes()` for input data
  - `buffer_allocate()` for hash/HMAC results
  - `string_allocate()` for hex/base64 encoding
- [ ] Add 12 crypto methods (0x4000-0x400B)

#### 5.2: Create Codec Module in Stdlib
- [ ] Create `crates/raya-stdlib/src/codec.rs`
- [ ] Add codec crate dependencies if not already present (rmp-serde, ciborium)
- [ ] Implement codec operations using ABI:
  - `string_read()` / `buffer_read_bytes()` for input
  - `buffer_allocate()` for encoded output
  - `object_allocate()` / `object_set_field()` for decode
  - `class_get_info()` for type metadata
- [ ] Add codec methods:
  - UTF-8: encode, decode, isValid, byteLength (0x7000-0x7003)
  - MessagePack: encode<T>, decode<T>, encodedSize (0x7010-0x7012)
  - CBOR: encode<T>, decode<T>, diagnostic (0x7020-0x7022)
  - Protobuf: encode<T>, decode<T> (0x7030-0x7031)

#### 5.3: Integrate with StdNativeHandler
- [ ] Add crypto module to lib.rs
- [ ] Add codec module to lib.rs
- [ ] Delegate crypto IDs (0x4000-0x40FF) to crypto module
- [ ] Delegate codec IDs (0x7000-0x71FF) to codec module
- [ ] Or: merge methods directly into StdNativeHandler match

#### 5.4: Remove Engine-Side Handlers
- [ ] Delete `crates/raya-engine/src/vm/vm/handlers/crypto.rs` (336 lines)
- [ ] Delete `crates/raya-engine/src/vm/vm/handlers/codec.rs` (~400 lines)
- [ ] Remove crypto dispatch from task_interpreter.rs (line ~6428-6430)
- [ ] Remove codec dispatch from task_interpreter.rs (line ~6440-6442)
- [ ] Remove crypto/codec dependencies from engine Cargo.toml
- [ ] Update builtin.rs if needed

#### 5.5: Add Tests
- [ ] Port 27 crypto e2e tests from raya-runtime
  - Test all algorithms (SHA-256/384/512, SHA-1, MD5)
  - Test HMAC, random, encoding functions
- [ ] Port 31 codec e2e tests from raya-runtime
  - 9 UTF-8, 8 MessagePack, 7 CBOR, 7 Protobuf tests
  - Test encode/decode roundtrips
  - Test error handling

### Files to Create
- `crates/raya-stdlib/src/crypto.rs` (new, ~300 lines)
- `crates/raya-stdlib/src/codec.rs` (new, ~400 lines)

### Files to Modify
- `crates/raya-stdlib/Cargo.toml` (add crypto/codec deps)
- `crates/raya-stdlib/src/lib.rs` (add crypto/codec modules)
- `crates/raya-stdlib/src/handler.rs` (delegate crypto/codec IDs)

### Files to Delete
- `crates/raya-engine/src/vm/vm/handlers/crypto.rs`
- `crates/raya-engine/src/vm/vm/handlers/codec.rs`

### Acceptance Criteria
- ✅ Crypto fully implemented in stdlib
- ✅ Codec fully implemented in stdlib
- ✅ All 27 crypto + 31 codec e2e tests pass (58 total)
- ✅ No crypto/codec code in engine
- ✅ Total test count unchanged (1,731)

---

## Phase 6: Testing & Validation

**Status:** Not Started

### Goal
Ensure the ABI refactor doesn't break existing functionality.

### Tasks

#### 6.1: Update Existing Tests
- [ ] Fix any test compilation errors from API changes
- [ ] Update test fixtures to use NativeContext
- [ ] Verify test coverage for all modules

#### 6.2: Add ABI-Specific Tests
- [ ] Test buffer operations (read, allocate, roundtrip)
- [ ] Test string operations (read, allocate, unicode)
- [ ] Test array operations (get, length, allocate, bounds)
- [ ] Test object operations (get_field, set_field, class_id)
- [ ] Test class registry (get_info, invalid IDs)
- [ ] Test error handling (invalid types, null checks)

#### 6.3: Run Full Test Suite
- [ ] Run all engine tests: `cargo test -p raya-engine` (831 tests)
- [ ] Run all runtime tests: `cargo test -p raya-runtime runtime -- --test-threads=2` (883 tests)
- [ ] Run all stdlib tests: `cargo test -p raya-stdlib` (17 tests)
- [ ] Verify total: 1,731 tests passing

#### 6.4: Benchmark Performance
- [ ] Measure overhead of NativeValue wrapping
- [ ] Compare dispatch latency (old vs new)
- [ ] Ensure < 5% performance regression
- [ ] Profile GC allocation patterns

### Acceptance Criteria
- ✅ All 1,731 tests pass
- ✅ No performance regression
- ✅ ABI operations have test coverage
- ✅ Error paths tested

---

## Phase 7: Documentation

**Status:** Not Started

### Goal
Document the new ABI for future maintainers.

### Tasks

#### 7.1: Update CLAUDE.md Files
- [ ] Update root CLAUDE.md with ABI summary
- [ ] Update engine CLAUDE.md to reference ABI
- [ ] Update stdlib CLAUDE.md with handler examples
- [ ] Add ABI design reference

#### 7.2: Update STDLIB.md
- [ ] Document NativeHandler interface
- [ ] Add code examples for each module type
- [ ] Explain NativeContext usage
- [ ] Document error handling patterns

#### 7.3: Add Inline Documentation
- [ ] Add module-level docs to abi.rs
- [ ] Add examples to each ABI function
- [ ] Document safety invariants
- [ ] Add troubleshooting guide

#### 7.4: Create Migration Guide
- [ ] Document old vs new interface
- [ ] Provide step-by-step migration steps
- [ ] List common pitfalls
- [ ] Add checklist for new handlers

### Files to Modify
- `CLAUDE.md` (milestone progress)
- `crates/raya-engine/CLAUDE.md` (ABI reference)
- `crates/raya-stdlib/CLAUDE.md` (handler guide)
- `design/STDLIB.md` (add ABI section)
- `crates/raya-engine/src/vm/abi.rs` (inline docs)

### Acceptance Criteria
- ✅ All documentation updated
- ✅ Examples compile and work
- ✅ Migration guide complete
- ✅ Future handlers have clear template

---

## Success Criteria

### Functional
- [x] Core ABI infrastructure implemented
- [x] NativeHandler interface refactored
- [ ] VM dispatcher uses ABI
- [ ] StdNativeHandler migrated
- [ ] Crypto & codec moved to stdlib
- [ ] All 1,731 tests pass

### Quality
- [ ] No performance regression (< 5%)
- [ ] Type-safe value access (no string parsing)
- [ ] Clear error messages
- [ ] Comprehensive test coverage

### Maintainability
- [ ] Design documented (ABI.md)
- [ ] Code examples provided
- [ ] Migration guide available
- [ ] Future handler template ready

---

## Benefits

**Before (String-based):**
- ❌ Manual string parsing (error-prone)
- ❌ Primitive return types only
- ❌ No GC access (can't work with buffers)
- ❌ No class introspection
- ❌ No task spawning from native
- ❌ Two code paths (simple vs advanced)

**After (Unified ABI):**
- ✅ Type-safe value access
- ✅ Full VM context (GC, classes, scheduler)
- ✅ Work with all types (buffers, objects, arrays)
- ✅ Single unified interface
- ✅ Result-based error handling
- ✅ Enables powerful stdlib modules

**Unlocked Capabilities:**
- 🔓 Crypto with binary data (buffers, hashing, encoding)
- 🔓 Codec with GC allocation (encode/decode objects)
- 🔓 Reflection with class registry
- 🔓 Runtime with task spawning
- 🔓 Collections with GC allocation
- 🔓 Any future module needs VM access

---

## Risk Mitigation

### Risk 1: Breaking Existing Tests
**Mitigation:** Incremental migration, run tests at each phase

### Risk 2: Performance Regression
**Mitigation:** Benchmark before/after, profile hotspots

### Risk 3: Memory Safety Issues
**Mitigation:** Lifetime bounds on NativeContext, careful unsafe review

### Risk 4: API Complexity
**Mitigation:** Comprehensive docs, examples, helper functions

---

## Next Steps

1. **Phase 3:** Update VM dispatcher (task_interpreter.rs)
2. **Phase 4:** Migrate StdNativeHandler (handler.rs)
3. **Phase 5:** Move crypto & codec to stdlib
4. **Phase 6:** Test everything
5. **Phase 7:** Document

**Estimated Effort:**
- Phase 3: 2-3 hours (dispatcher migration)
- Phase 4: 1-2 hours (stdlib handler)
- Phase 5: 4-6 hours (crypto + codec migration)
- Phase 6: 1-2 hours (testing)
- Phase 7: 1-2 hours (docs)
- **Total:** 9-16 hours

---

## References

- **Design:** [design/ABI.md](../design/ABI.md) - Complete ABI specification
- **Engine Code:** `crates/raya-engine/src/vm/abi.rs` - ABI implementation
- **Handler Trait:** `crates/raya-engine/src/vm/native_handler.rs` - Interface definition
- **Stdlib Handler:** `crates/raya-stdlib/src/handler.rs` - Current implementation
- **VM Dispatcher:** `crates/raya-engine/src/vm/vm/task_interpreter.rs` - Native call handling
