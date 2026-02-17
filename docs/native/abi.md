---
title: "Native ABI"
---

# Internal Native ABI Design

> **Status:** In Progress (Milestone 4.9, Phases 1-2 complete)
> **Related:** [Native Bindings](./native-bindings.md), [Stdlib](../stdlib/stdlib.md), [Architecture](../runtime/architecture.md)

## Overview

The Native ABI provides a controlled interface for stdlib modules (in `raya-stdlib`) to access VM internals without exposing the full implementation. This allows modules like `std:crypto` to work with binary data (Buffers) while maintaining proper encapsulation.

## Key Design Decisions

### ✅ Unified Interface
**Single `call()` method for all handlers** - no separate simple/advanced interfaces.
- Every handler gets full `NativeContext` (GC, classes, scheduler)
- Handlers use what they need, ignore the rest
- Simpler mental model, easier to evolve

### ✅ Typed Arguments
**NativeValue instead of strings** - type-safe from the start.
- `args[0].as_f64()` instead of parsing strings
- Direct access to buffers, objects, arrays
- No conversion overhead

### ✅ Full VM Access
**Expose GC, class registry, and scheduler** - stdlib needs real power.
- Create objects dynamically
- Spawn tasks from native code
- Introspect types and classes
- Not just "helper functions" - full VM integration

### ✅ Result-Based Errors
**All fallible operations return Result** - no panics, clear error flow.
- `string_read(val) -> Result<String, String>`
- `NativeCallResult::Error(msg)` for handler errors
- VM converts to VmError automatically

### ✅ Lifetime Safety
**NativeContext is lifetime-bound** - can't escape the call.
- `NativeContext<'a>` tied to call scope
- `NativeValue` can't be stored across calls
- Prevents GC issues, dangling pointers

## Design Goals

1. **Minimal Surface Area**: Only expose what's needed, nothing more
2. **Type Safety**: Prevent invalid type casts and UB
3. **Zero Copy Where Possible**: Avoid unnecessary allocations
4. **Clear Ownership**: No ambiguity about who owns what
5. **Error Handling**: All operations return Result, never panic
6. **Unified Interface**: One method signature for all handlers
7. **Full Power**: stdlib can do anything the VM can do

## Before vs After

| Aspect | Before (String-based) | After (ABI) |
|--------|----------------------|-------------|
| **Interface** | `call(id, &[String])` | `call(ctx, id, &[NativeValue])` |
| **Result** | `String/Number/Bool/Void/Error/Unhandled` | `Value/Unhandled/Error` |
| **Arguments** | Parse strings manually | Type-safe accessors (`as_i32()`) |
| **Return** | Primitive types only | Any GC-managed type |
| **GC Access** | ❌ No | ✅ Via `ctx.gc` |
| **Classes** | ❌ No | ✅ Via `ctx.classes` |
| **Scheduler** | ❌ No | ✅ Via `ctx.scheduler` |
| **Buffers** | ❌ Can't work with binary | ✅ `buffer_read_bytes()`, `buffer_allocate()` |
| **Objects** | ❌ Can't create/inspect | ✅ `object_allocate()`, `object_get_field()` |
| **Arrays** | ❌ Can't access | ✅ `array_get()`, `array_allocate()` |
| **Use Cases** | Simple I/O (logger, math) | **Anything** (crypto, reflect, runtime, collections) |

### Migration Path

**Old Handler (String-based):**
```rust
impl NativeHandler for StdNativeHandler {
    fn call(&self, id: u16, args: &[String]) -> NativeCallResult {
        match id {
            0x2000 => {  // Math.abs
                let x = args[0].parse::<f64>().unwrap_or(0.0);  // ❌ Parse strings
                NativeCallResult::Number(x.abs())               // ❌ Limited types
            }
            _ => NativeCallResult::Unhandled
        }
    }
}
```

**New Handler (ABI):**
```rust
impl NativeHandler for StdNativeHandler {
    fn call(&self, ctx: &NativeContext, id: u16, args: &[NativeValue]) -> NativeCallResult {
        match id {
            0x2000 => {  // Math.abs
                let x = args[0].as_f64().unwrap_or(0.0);  // ✅ Type-safe
                NativeCallResult::f64(x.abs())             // ✅ Helper constructor
            }
            0x4000 => {  // Crypto.hash (NEW - wasn't possible before!)
                let algo = string_read(args[0])?;
                let data = string_read(args[1])?;
                let digest = sha256(data.as_bytes());
                NativeCallResult::Value(string_allocate(ctx, hex::encode(digest)))  // ✅ GC allocation
            }
            _ => NativeCallResult::Unhandled
        }
    }
}
```

## Use Cases

### 1. Crypto Module (std:crypto)

**Reading:**
- Extract Buffer bytes (for hashing, HMAC input)
- Extract String (for algorithm names)
- Extract primitives (sizes, integers)

**Returning:**
- Allocate new Buffer (hash digests, random bytes)
- Allocate new String (hex, base64 encoded)
- Return primitives (bool for timingSafeEqual)

**Example Flow:**
```rust
// crypto.hash(algorithm: string, data: string): string
fn hash(ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let algorithm = string_read(args[0])?;  // Read string
    let data = string_read(args[1])?;       // Read string

    let digest = sha256(data.as_bytes());   // Compute hash
    let hex = hex::encode(digest);

    NativeCallResult::Value(string_allocate(ctx, hex))  // Allocate & return
}
```

### 2. Future: File I/O Module (std:fs)

**Reading:**
- Extract String (file paths)
- Extract Buffer (write data)

**Returning:**
- Allocate Buffer (file contents)
- Allocate String (text contents)
- Return primitives (file size, success/failure)

### 3. Future: Network Module (std:net)

**Reading:**
- Extract String (URLs, hostnames)
- Extract Buffer (request/response bodies)
- Extract primitives (ports, timeouts)

**Returning:**
- Allocate Buffer (response bodies)
- Allocate String (headers, JSON)
- Return primitives (status codes)

### 4. Future: Image/Media Processing

**Reading:**
- Extract Buffer (image bytes)
- Extract primitives (width, height, quality)

**Returning:**
- Allocate Buffer (processed image)
- Return primitives (dimensions)

## Type Support Matrix

| Type | Read (Raya→Rust) | Allocate (Rust→Raya) | Notes |
|------|------------------|----------------------|-------|
| **i32** | ✅ `as_i32()` | ✅ `NativeValue::i32()` | Direct value |
| **f64** | ✅ `as_f64()` | ✅ `NativeValue::f64()` | Direct value |
| **bool** | ✅ `as_bool()` | ✅ `NativeValue::bool()` | Direct value |
| **null** | ✅ `is_null()` | ✅ `NativeValue::null()` | Direct value |
| **String** | ✅ `string_read()` | ✅ `string_allocate()` | Clones data, GC-managed |
| **Buffer** | ✅ `buffer_read_bytes()` | ✅ `buffer_allocate()` | Copies bytes, GC-managed |
| **Array** | ✅ `array_get()`, `array_length()` | ✅ `array_allocate()` | GC-managed |
| **Object** | ✅ `object_get_field()`, `object_class_id()` | ✅ `object_allocate()` | GC-managed, needs class_id |

## API Design

### Core Types

```rust
/// Context with full VM subsystem access (lifetime-bound)
pub struct NativeContext<'a> {
    pub(crate) gc: &'a Mutex<Gc>,              // GC for allocation
    pub(crate) classes: &'a RwLock<TypeRegistry>,  // Class registry
    pub(crate) scheduler: &'a Arc<Scheduler>,      // Task scheduler
    pub(crate) current_task: TaskId,               // Current task ID
}

impl<'a> NativeContext<'a> {
    pub fn current_task_id(&self) -> u64;  // Get current task ID
}

/// Safe wrapper around VM Value
#[repr(transparent)]
pub struct NativeValue(pub(crate) Value);

/// Class information
pub struct ClassInfo {
    pub class_id: usize,
    pub field_count: usize,
    pub name: String,
}
```

### Reading Values (Raya → Rust)

```rust
// Primitives (fallible extraction)
impl NativeValue {
    pub fn as_i32(&self) -> Option<i32>;
    pub fn as_f64(&self) -> Option<f64>;
    pub fn as_bool(&self) -> Option<bool>;
    pub fn is_null(&self) -> bool;
    pub fn is_ptr(&self) -> bool;
}

// Complex types (fallible, returns owned data)
pub fn string_read(val: NativeValue) -> Result<String, String>;
pub fn buffer_read_bytes(val: NativeValue) -> Result<Vec<u8>, String>;
```

### Allocating Values (Rust → Raya)

```rust
// Primitives (infallible construction)
impl NativeValue {
    pub fn i32(val: i32) -> Self;
    pub fn f64(val: f64) -> Self;
    pub fn bool(val: bool) -> Self;
    pub fn null() -> Self;
}

// Complex types (allocates on GC heap)
pub fn string_allocate(ctx: &NativeContext, s: String) -> NativeValue;
pub fn buffer_allocate(ctx: &NativeContext, data: &[u8]) -> NativeValue;
```

### Native Handler Interface

```rust
/// Unified trait - ALL handlers get full VM context
pub trait NativeHandler: Send + Sync {
    fn call(&self, ctx: &NativeContext, id: u16, args: &[NativeValue]) -> NativeCallResult;
}

/// Simple result type - only 3 variants
pub enum NativeCallResult {
    Value(NativeValue),  // Success with value
    Unhandled,           // Not my ID
    Error(String),       // Failure with message
}

/// Helper constructors
impl NativeCallResult {
    pub fn null() -> Self;
    pub fn i32(val: i32) -> Self;
    pub fn f64(val: f64) -> Self;
    pub fn bool(val: bool) -> Self;
}

pub type AbiResult<T> = Result<T, String>;
```

## Safety Considerations

### 1. Type Checking

**Problem**: `NativeValue` wraps raw `Value` which could be any type.

**Solution**: All extraction methods are fallible:
```rust
string_read(val) -> Result<String, String>  // Returns Err if not a string
buffer_read_bytes(val) -> Result<Vec<u8>, String>  // Returns Err if not a buffer
```

### 2. Pointer Validity

**Problem**: Buffer/String are raw pointers that could be invalid.

**Solution**:
- Values come from VM stack (guaranteed valid during call)
- GC doesn't run during native call (pointers stay valid)
- No way to store `NativeValue` across calls (lifetime-bound via `NativeContext`)

### 3. Memory Leaks

**Problem**: Allocated objects must be tracked by GC.

**Solution**:
- All allocations go through `NativeContext` → `gc.lock().allocate()`
- GC owns all allocated objects
- No way to bypass GC allocation

### 4. Concurrency

**Problem**: Multiple tasks could call native methods concurrently.

**Solution**:
- `NativeHandler: Send + Sync` (required by trait)
- `NativeContext` locks GC during allocation
- No shared mutable state in handlers

## Implementation Strategy

### Phase 1: Crypto Migration (Current)

1. ✅ Define ABI types (`NativeContext`, `NativeValue`)
2. ✅ Implement Buffer/String read/allocate
3. ✅ Add `call_abi()` to `NativeHandler` trait
4. ⏳ Update VM dispatcher to call `call_abi()` for crypto range
5. ⏳ Implement crypto in `raya-stdlib` using ABI
6. ⏳ Remove engine-side crypto handler

### Phase 2: Array Support (Future)

Add if needed for batch operations:
```rust
pub fn array_read(val: NativeValue) -> Result<Vec<NativeValue>, String>;
pub fn array_allocate(ctx: &NativeContext, items: &[NativeValue]) -> NativeValue;
```

### Phase 3: Object Support (Future)

Add if needed for structured data:
```rust
pub fn object_get_field(val: NativeValue, index: usize) -> Result<NativeValue, String>;
pub fn object_allocate(ctx: &NativeContext, class_id: usize, fields: &[NativeValue]) -> NativeValue;
```

## Comparison with Alternatives

### Alternative 1: Expose Full VM Types

**Pros**: Maximum flexibility, no wrapper overhead
**Cons**: Tight coupling, hard to evolve, security risk

### Alternative 2: C-style FFI

**Pros**: Standard approach, language-independent
**Cons**: No safety, manual memory management, verbose

### Alternative 3: Current Design (ABI)

**Pros**: Type-safe, minimal coupling, evolvable, Rust-native
**Cons**: Small wrapper overhead (negligible)

## Open Questions

1. **Should we support Array/Object initially?**
   - Current answer: No, add only when needed
   - Rationale: YAGNI - crypto doesn't need them

2. **Should NativeValue be opaque or expose methods?**
   - Current answer: Expose safe methods (`as_i32()`, etc.)
   - Rationale: Ergonomic, still type-safe

3. **How to handle type-specific methods (e.g., Buffer.length)?**
   - Current answer: Not exposed in ABI (read full bytes)
   - Rationale: Keep ABI minimal, stdlib can inspect after reading

4. **Should we allow storing NativeValue across calls?**
   - Current answer: No (lifetime-bound to NativeContext)
   - Rationale: Prevents GC issues, simpler model

## Complete Examples

### Example 1: Simple Math Handler

**Stdlib side (raya-stdlib/src/handler.rs):**
```rust
use raya_engine::vm::{
    NativeHandler, NativeCallResult, NativeContext, NativeValue
};

pub struct StdNativeHandler;

impl NativeHandler for StdNativeHandler {
    fn call(&self, ctx: &NativeContext, id: u16, args: &[NativeValue]) -> NativeCallResult {
        match id {
            // std:math methods (0x2000-0x2016)
            0x2000 => {  // Math.abs(x)
                let x = args[0].as_f64().unwrap_or(0.0);
                NativeCallResult::f64(crate::math::abs(x))
            }
            0x2001 => {  // Math.sign(x)
                let x = args[0].as_f64().unwrap_or(0.0);
                NativeCallResult::f64(crate::math::sign(x))
            }
            // ... other math methods ...

            _ => NativeCallResult::Unhandled
        }
    }
}
```

**Key points:**
- Direct access to typed values via `args[0].as_f64()`
- No string conversion needed
- Context available but not needed for simple math

### Example 2: Crypto Handler with Buffer/GC

**Stdlib side (raya-stdlib/src/crypto.rs):**
```rust
use raya_engine::vm::{
    NativeHandler, NativeCallResult, NativeContext, NativeValue,
    buffer_read_bytes, string_read, buffer_allocate, string_allocate
};

pub struct CryptoHandler;

impl NativeHandler for CryptoHandler {
    fn call(&self, ctx: &NativeContext, id: u16, args: &[NativeValue]) -> NativeCallResult {
        match id {
            0x4000 => {  // Crypto.hash(algorithm, data)
                let algorithm = match string_read(args[0]) {
                    Ok(s) => s,
                    Err(e) => return NativeCallResult::Error(e),
                };
                let data = match string_read(args[1]) {
                    Ok(s) => s,
                    Err(e) => return NativeCallResult::Error(e),
                };

                let digest = hash_impl(&algorithm, data.as_bytes());
                let hex = hex::encode(digest);

                // Use ctx to allocate GC-managed string
                NativeCallResult::Value(string_allocate(ctx, hex))
            }

            0x4004 => {  // Crypto.randomBytes(size)
                let size = match args[0].as_i32() {
                    Some(n) => n as usize,
                    None => return NativeCallResult::Error("Expected number".into()),
                };

                let bytes = generate_random_bytes(size);

                // Use ctx to allocate GC-managed buffer
                NativeCallResult::Value(buffer_allocate(ctx, &bytes))
            }

            _ => NativeCallResult::Unhandled
        }
    }
}
```

**Key points:**
- Uses `string_read()` to extract string from NativeValue
- Uses `ctx` to allocate return values on GC heap
- Type-safe error handling with `Result`

### Example 3: Reflection Handler with Class Registry

**Stdlib side (raya-stdlib/src/reflect.rs):**
```rust
use raya_engine::vm::{
    NativeHandler, NativeCallResult, NativeContext, NativeValue,
    class_get_info, object_allocate, object_get_field, string_allocate
};

pub struct ReflectHandler;

impl NativeHandler for ReflectHandler {
    fn call(&self, ctx: &NativeContext, id: u16, args: &[NativeValue]) -> NativeCallResult {
        match id {
            0x0D00 => {  // Reflect.getClassName(obj)
                let class_id = match object_class_id(args[0]) {
                    Ok(id) => id,
                    Err(e) => return NativeCallResult::Error(e),
                };

                // Use ctx.classes to get class info
                let info = match class_get_info(ctx, class_id) {
                    Ok(i) => i,
                    Err(e) => return NativeCallResult::Error(e),
                };

                NativeCallResult::Value(string_allocate(ctx, info.name))
            }

            0x0D01 => {  // Reflect.createInstance(class_id, field_count)
                let class_id = args[0].as_i32().unwrap_or(0) as usize;
                let field_count = args[1].as_i32().unwrap_or(0) as usize;

                // Use ctx.gc to allocate new object
                NativeCallResult::Value(object_allocate(ctx, class_id, field_count))
            }

            _ => NativeCallResult::Unhandled
        }
    }
}
```

**Key points:**
- Uses `ctx.classes` (via `class_get_info()`) for type introspection
- Uses `ctx.gc` (via `object_allocate()`) to create instances
- Full object model access

### Example 4: Task Handler with Scheduler

**Stdlib side (raya-stdlib/src/runtime.rs):**
```rust
use raya_engine::vm::{
    NativeHandler, NativeCallResult, NativeContext, NativeValue,
    task_spawn, task_cancel, task_is_done
};

pub struct RuntimeHandler;

impl NativeHandler for RuntimeHandler {
    fn call(&self, ctx: &NativeContext, id: u16, args: &[NativeValue]) -> NativeCallResult {
        match id {
            0x3000 => {  // Runtime.spawn(function_id, args)
                let function_id = args[0].as_i32().unwrap_or(0) as usize;
                let task_args = &args[1..];

                // Use ctx.scheduler to spawn task
                match task_spawn(ctx, function_id, task_args) {
                    Ok(task_id) => NativeCallResult::Value(NativeValue::i64(task_id as i64)),
                    Err(e) => NativeCallResult::Error(e),
                }
            }

            0x3001 => {  // Runtime.currentTaskId()
                // Direct access to ctx.current_task
                let task_id = ctx.current_task_id();
                NativeCallResult::Value(NativeValue::i64(task_id as i64))
            }

            _ => NativeCallResult::Unhandled
        }
    }
}
```

**Key points:**
- Uses `ctx.scheduler` (via `task_spawn()`) for concurrency
- Direct access to `ctx.current_task_id()`
- Full scheduler integration

### VM Dispatcher (Unified)

**Engine side (task_interpreter.rs):**
```rust
// ALL native calls dispatch through unified interface
id if is_stdlib_method(id as u16) => {
    use crate::vm::abi::{NativeContext, NativeValue};

    // Create context with full VM access
    let ctx = NativeContext::new(
        &self.gc,
        &self.classes,
        &self.scheduler,
        task.id()
    );

    // Convert stack values to NativeValue
    let native_args: Vec<NativeValue> = args.iter()
        .map(|v| NativeValue::from_value(*v))
        .collect();

    // Single unified call
    match self.native_handler.call(&ctx, id as u16, &native_args) {
        NativeCallResult::Value(val) => {
            call_stack.push(val.into_value())?;
        }
        NativeCallResult::Error(msg) => {
            return Err(VmError::RuntimeError(msg));
        }
        NativeCallResult::Unhandled => {
            return Err(VmError::RuntimeError(format!(
                "Native call {:#06x} not implemented", id
            )));
        }
    }
}
```

**Key points:**
- Single code path for ALL native calls
- Context passed to every handler
- Handlers decide what they need from context

## Implementation Checklist

### Phase 1: Core ABI ✅ (Done)
- [x] Define `NativeContext<'a>` with GC, classes, scheduler
- [x] Define `NativeValue` wrapper with type-safe accessors
- [x] Implement buffer operations (read_bytes, allocate)
- [x] Implement string operations (read, allocate)
- [x] Implement array operations (get, length, allocate)
- [x] Implement object operations (get_field, set_field, allocate, class_id)
- [x] Implement class registry (get_info)
- [x] Stub task scheduler (spawn, cancel, is_done)

### Phase 2: Interface Refactor ✅ (Done)
- [x] Update `NativeHandler` trait to unified `call(ctx, id, args)`
- [x] Simplify `NativeCallResult` to 3 variants (Value/Unhandled/Error)
- [x] Add helper constructors (`null()`, `i32()`, `f64()`, `bool()`)
- [x] Update design doc with examples

### Phase 3: Handler Migration (Next)
- [ ] Update `StdNativeHandler` in raya-stdlib to use new signature
  - [ ] Math methods: use `args[0].as_f64()` instead of parsing
  - [ ] Logger methods: use `string_read()` for messages
  - [ ] Return `NativeCallResult::f64()` / `null()` instead of Number/Void
- [ ] Update VM dispatcher in task_interpreter.rs
  - [ ] Create `NativeContext::new()` with all 4 parameters
  - [ ] Convert args to `Vec<NativeValue>`
  - [ ] Call `native_handler.call(&ctx, id, &args)`
  - [ ] Handle all 3 result variants
- [ ] Remove old string-based dispatch code

### Phase 4: Crypto Migration (Next)
- [ ] Create `crypto.rs` in raya-stdlib
- [ ] Implement `CryptoHandler` using ABI:
  - [ ] Use `string_read()` for algorithm names
  - [ ] Use `buffer_read_bytes()` for binary data
  - [ ] Use `buffer_allocate()` / `string_allocate()` for results
- [ ] Update `StdNativeHandler` to delegate crypto IDs to `CryptoHandler`
- [ ] Remove crypto handler from raya-engine

### Phase 5: Testing & Validation
- [ ] Update tests in raya-stdlib/src/handler.rs
- [ ] Add ABI-specific tests (buffer/object/array operations)
- [ ] Run full test suite (1,731 tests should still pass)
- [ ] Benchmark overhead (should be negligible)

### Phase 6: Documentation
- [ ] Update CLAUDE.md to document ABI
- [ ] Update STDLIB.md with handler examples
- [ ] Add inline docs to abi.rs
- [ ] Create migration guide for future handlers

## Summary

This ABI design provides:
- ✅ **Unified interface** - One `call()` method for all handlers
- ✅ **Type-safe** - No string parsing, direct value access
- ✅ **Full VM access** - GC, classes, scheduler, objects, arrays
- ✅ **Clear errors** - Result-based, no panics
- ✅ **Lifetime safety** - Context can't escape call scope
- ✅ **Extensible** - Easy to add new capabilities
- ✅ **Performant** - Minimal overhead, zero-copy where possible
- ✅ **Evolvable** - Clean separation, easy to change implementation

**Key insight:** By exposing GC, class registry, and scheduler through a controlled context, we enable stdlib to be as powerful as the VM itself while maintaining safety and encapsulation. The unified interface makes this natural - every handler can use what it needs.
