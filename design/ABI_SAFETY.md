# ABI Thread Safety & GC Safety

**Status:** Design Document
**Version:** 1.0
**Last Updated:** 2026-01-24

---

## Overview

This document specifies the thread safety and GC safety guarantees for Raya's native module ABI. All FFI boundaries must be carefully designed to prevent:

1. **Data races** - Concurrent access to shared mutable state
2. **Use-after-free** - Accessing GC'd memory
3. **Memory leaks** - Failing to free allocated memory
4. **Dangling pointers** - Pointers to moved/freed memory

---

## Thread Safety Requirements

### 1. NativeFn Must Be Thread-Safe

Native functions can be called from any VM Task (green thread), which may be scheduled on any OS thread.

**Requirement:** All `NativeFn` implementations must be `Send + Sync`

```rust
// Function pointers are automatically Send + Sync if the function is safe
pub type NativeFn = extern "C" fn(args: *const NativeValue, arg_count: usize) -> NativeValue;

// This is enforced by Rust's type system - function pointers are Send + Sync
static_assertions::assert_impl_all!(NativeFn: Send, Sync);
```

**What this means:**
- Native functions cannot use `thread_local!` for mutable state
- Native functions cannot use `Rc` or other non-thread-safe types
- Native functions must use atomic operations for shared mutable state

**Safe patterns:**
```rust
// ✅ SAFE: No shared state
#[function]
fn add(a: i32, b: i32) -> i32 {
    a + b
}

// ✅ SAFE: Immutable shared state
static CONSTANT: &str = "hello";

#[function]
fn get_constant() -> &'static str {
    CONSTANT
}

// ✅ SAFE: Atomic shared state
use std::sync::atomic::{AtomicU64, Ordering};
static COUNTER: AtomicU64 = AtomicU64::new(0);

#[function]
fn increment_counter() -> u64 {
    COUNTER.fetch_add(1, Ordering::SeqCst)
}

// ✅ SAFE: Mutex-protected shared state
use std::sync::Mutex;
static STATE: Mutex<HashMap<String, i32>> = Mutex::new(HashMap::new());

#[function]
fn set_value(key: String, value: i32) {
    let mut state = STATE.lock().unwrap();
    state.insert(key, value);
}
```

**Unsafe patterns:**
```rust
// ❌ UNSAFE: thread_local mutable state (not Send)
thread_local! {
    static STATE: RefCell<i32> = RefCell::new(0);
}

#[function]
fn bad_increment() -> i32 {
    STATE.with(|s| {
        *s.borrow_mut() += 1;
        *s.borrow()
    })
}

// ❌ UNSAFE: Rc (not Send)
static BAD_STATE: Rc<RefCell<i32>> = Rc::new(RefCell::new(0));

#[function]
fn bad_function() -> i32 {
    // Won't compile - Rc is not Send
    *BAD_STATE.borrow()
}
```

### 2. NativeValue Must Be Send + Sync

`NativeValue` can be passed between threads (VM can migrate Tasks across threads).

**Current implementation issue:**
```rust
// ❌ PROBLEM: Box::into_raw creates non-Send pointer
pub struct NativeValue {
    inner: usize,  // Raw pointer - not inherently Send
}
```

**Solution:** Add explicit Send + Sync markers with safety invariants:

```rust
// NativeValue is Send + Sync because:
// 1. The pointed-to Value is heap-allocated and owned
// 2. The VM ensures no concurrent access during native calls
// 3. Values are pinned during native call lifetime
unsafe impl Send for NativeValue {}
unsafe impl Sync for NativeValue {}
```

**Safety invariants:**
1. Each `NativeValue` owns its pointed-to `Value` uniquely
2. No two `NativeValue` instances point to the same `Value`
3. The VM pins values before passing to native code
4. The VM unpins values after native call returns

### 3. Module Registration Must Be Thread-Safe

Module registration happens during VM initialization (single-threaded) but module access happens from any thread.

**Design:**
```rust
pub struct NativeModule {
    name: String,
    version: String,
    functions: HashMap<String, NativeFn>,  // Immutable after registration
}

// NativeModule is Send + Sync because:
// 1. After registration, it's immutable
// 2. HashMap<String, NativeFn> is Send + Sync when immutable
// 3. Function pointers are Send + Sync
unsafe impl Send for NativeModule {}
unsafe impl Sync for NativeModule {}
```

**VM Context storage:**
```rust
pub struct VmContext {
    // ... other fields ...

    /// Native modules (immutable after initialization)
    /// Uses DashMap for concurrent access if needed, or RwLock for rare writes
    native_modules: Arc<RwLock<HashMap<String, Arc<NativeModule>>>>,
}
```

---

## GC Safety Requirements

### 1. Value Lifetime Guarantees

**Problem:** GC can move or free `Value` objects at any time. Native code must not hold dangling pointers.

**Solution: Pinning Protocol**

```rust
/// Pin a value to prevent GC from moving/freeing it.
///
/// MUST be called before passing value to native code.
/// MUST be paired with unpin after native call returns.
pub fn pin_value(value: NativeValue) {
    unsafe {
        let val_ptr = value.inner as *mut ValueHeader;
        (*val_ptr).pin_count.fetch_add(1, Ordering::AcqRel);
    }
}

/// Unpin a value to allow GC to collect it.
pub fn unpin_value(value: NativeValue) {
    unsafe {
        let val_ptr = value.inner as *mut ValueHeader;
        let old_count = (*val_ptr).pin_count.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(old_count > 0, "Unpin called on unpinned value");
    }
}

/// Value header with GC metadata
#[repr(C)]
struct ValueHeader {
    /// Type tag
    type_tag: u8,

    /// GC mark bit (0 = white, 1 = marked)
    gc_mark: AtomicU8,

    /// Pin count (prevents GC from moving/freeing)
    pin_count: AtomicU32,

    /// Size in bytes
    size: u32,
}
```

**VM integration:**
```rust
// Before calling native function
for arg in &args {
    pin_value(*arg);
}

// Call native function
let result = native_fn(args.as_ptr(), args.len());

// After native function returns
for arg in &args {
    unpin_value(*arg);
}
```

**GC respects pin count:**
```rust
// In GC sweep phase
fn gc_sweep(&mut self, object: *mut ValueHeader) {
    unsafe {
        if (*object).pin_count.load(Ordering::Acquire) > 0 {
            // Object is pinned - DO NOT COLLECT
            return;
        }

        if (*object).gc_mark.load(Ordering::Acquire) == 0 {
            // Unmarked and unpinned - safe to collect
            self.free_object(object);
        }
    }
}
```

### 2. No Heap Allocation During GC

**Problem:** Native code might allocate new Values while GC is running, causing races.

**Solution: GC Pause Protocol**

```rust
/// Global GC state
static GC_STATE: AtomicU8 = AtomicU8::new(GC_IDLE);

const GC_IDLE: u8 = 0;
const GC_MARKING: u8 = 1;
const GC_SWEEPING: u8 = 2;

/// Check if allocation is safe
fn can_allocate() -> bool {
    GC_STATE.load(Ordering::Acquire) == GC_IDLE
}

/// Allocate new Value (used by ToRaya)
pub fn allocate_value(value: Value) -> NativeValue {
    // Wait for GC to finish if necessary
    while !can_allocate() {
        std::hint::spin_loop();
    }

    // Allocate and return
    NativeValue::from_value(value)
}
```

**Alternative: Per-Task Allocation Arena**
```rust
/// Each Task has its own allocation arena
/// Native code allocates from Task-local arena
/// GC only sweeps arena when Task is idle
pub struct TaskArena {
    allocations: Vec<*mut ValueHeader>,
    pin_count: AtomicU32,  // Non-zero = native code active
}
```

### 3. Value Ownership Model

**Three ownership levels:**

1. **Owned Value** - Native code owns the Value, must free it
   ```rust
   impl NativeValue {
       /// Consume NativeValue and take ownership of Value
       pub unsafe fn into_value(self) -> Value {
           *Box::from_raw(self.inner as *mut Value)
       }
   }
   ```

2. **Borrowed Value** - Native code has temporary read access
   ```rust
   impl NativeValue {
       /// Borrow Value (pinned by VM)
       pub unsafe fn as_value(&self) -> &Value {
           &*(self.inner as *const Value)
       }
   }
   ```

3. **Shared Value** - Multiple references, reference counted
   ```rust
   /// For values that need to outlive native call
   pub struct SharedValue {
       inner: Arc<Value>,
   }
   ```

**Current implementation uses Borrowed model** - VM owns Values, native code borrows during call.

### 4. Return Value Handling

**Problem:** Who owns the returned `NativeValue`?

**Answer:** Caller (VM) takes ownership.

```rust
// Native function returns owned NativeValue
extern "C" fn my_function(args: *const NativeValue, arg_count: usize) -> NativeValue {
    let result = 42_i32.to_raya();
    result  // Ownership transferred to caller
}

// VM takes ownership and stores in register/stack
let result = native_fn(args.as_ptr(), args.len());
// VM now owns result, will free it when no longer needed
```

---

## Memory Leak Prevention

### 1. RAII Guard for Pinning

Use RAII to ensure unpinning even on panic:

```rust
pub struct PinGuard {
    value: NativeValue,
}

impl PinGuard {
    pub fn new(value: NativeValue) -> Self {
        pin_value(value);
        PinGuard { value }
    }
}

impl Drop for PinGuard {
    fn drop(&mut self) {
        unpin_value(self.value);
    }
}

// Usage in VM
let guards: Vec<PinGuard> = args.iter().map(|&v| PinGuard::new(v)).collect();
let result = native_fn(args.as_ptr(), args.len());
// guards dropped here - automatic unpinning
```

### 2. Panic Safety

Native functions must not leak on panic:

```rust
// Generated by #[function] macro
extern "C" fn function_ffi(args: *const NativeValue, arg_count: usize) -> NativeValue {
    // Catch panics - prevents unwinding across FFI boundary
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        // ... function logic ...
    }));

    match result {
        Ok(value) => value,
        Err(_) => NativeValue::error("Function panicked"),
    }
}
```

### 3. Cleanup on Module Unload

When module is unloaded, free all resources:

```rust
#[no_mangle]
pub extern "C" fn raya_module_cleanup(module: *mut NativeModule) {
    unsafe {
        // Take ownership and drop
        let _ = Box::from_raw(module);

        // All internal allocations (HashMap, Strings) automatically freed
    }
}
```

---

## Data Race Prevention

### 1. No Interior Mutability Without Synchronization

```rust
// ❌ BAD: RefCell is not thread-safe
struct BadState {
    data: RefCell<i32>,
}

// ✅ GOOD: Mutex provides synchronization
struct GoodState {
    data: Mutex<i32>,
}

// ✅ GOOD: Atomic for simple values
struct GoodCounter {
    count: AtomicU64,
}
```

### 2. Function-Local State Only

Native functions should avoid shared mutable state entirely:

```rust
// ✅ PREFERRED: Pure function (no state)
#[function]
fn pure_function(a: i32, b: i32) -> i32 {
    a + b
}

// ⚠️ ACCEPTABLE: Immutable shared state
static CONFIG: &str = "config";

#[function]
fn get_config() -> &'static str {
    CONFIG
}

// ⚠️ USE WITH CARE: Synchronized shared state
static STATE: Mutex<HashMap<String, i32>> = Mutex::new(HashMap::new());

#[function]
fn update_state(key: String, value: i32) {
    STATE.lock().unwrap().insert(key, value);
}
```

### 3. Task-Local Storage (Future Feature)

For per-Task state without synchronization:

```rust
// Future API design
#[function]
fn set_task_local(key: String, value: Value) {
    task_local::set(key, value);
}

#[function]
fn get_task_local(key: String) -> Option<Value> {
    task_local::get(&key)
}
```

---

## ABI Safety Checklist

### For Native Module Authors

- [ ] All functions are pure or use proper synchronization
- [ ] No `thread_local!` with mutable state
- [ ] No `Rc`, `RefCell` without `Mutex`
- [ ] All shared mutable state uses `Mutex`, `RwLock`, or atomics
- [ ] Panic handling in place (or use `#[function]` macro)
- [ ] No assumptions about which thread calls the function
- [ ] Return values properly owned (not borrowed from static)

### For VM Implementation

- [ ] Pin all arguments before native call
- [ ] Unpin all arguments after native call (even on error)
- [ ] Use RAII guards for automatic unpinning
- [ ] Check GC state before allocation
- [ ] Respect pin_count during GC sweep
- [ ] Handle panics from native code gracefully
- [ ] Prevent unwinding across FFI boundary
- [ ] Proper cleanup on module unload

### For Proc-Macro Generated Code

- [ ] Wrap function in `catch_unwind`
- [ ] Convert panics to error NativeValue
- [ ] Validate argument count
- [ ] Type check arguments with clear errors
- [ ] Ensure all FromRaya conversions are safe
- [ ] Ensure all ToRaya conversions transfer ownership correctly

---

## Testing Strategy

### 1. Thread Safety Tests

```rust
#[test]
fn test_concurrent_native_calls() {
    let module = create_test_module();

    // Call from multiple threads
    let handles: Vec<_> = (0..100).map(|i| {
        std::thread::spawn(move || {
            let result = call_native_function("add", &[i, i]);
            assert_eq!(result, i + i);
        })
    }).collect();

    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn test_task_migration() {
    // Start Task on thread A
    // Migrate to thread B mid-execution
    // Call native function on thread B
    // Ensure no data races
}
```

### 2. GC Safety Tests

```rust
#[test]
fn test_gc_during_native_call() {
    // Pin value
    // Start native call
    // Trigger GC on another thread
    // Ensure pinned value not collected
    // Complete native call
    // Unpin value
    // Trigger GC
    // Ensure value now collectible
}

#[test]
fn test_allocation_during_gc() {
    // Start GC
    // Attempt allocation from native code
    // Ensure no races
    // Ensure allocation waits or succeeds safely
}
```

### 3. Memory Leak Tests

```rust
#[test]
fn test_no_leaks_on_panic() {
    let before = get_heap_usage();

    // Call function that panics
    let _ = std::panic::catch_unwind(|| {
        call_native_function_that_panics();
    });

    run_gc();
    let after = get_heap_usage();

    assert_eq!(before, after, "Memory leaked on panic");
}
```

### 4. Stress Tests

```rust
#[test]
fn stress_test_concurrent_gc_and_native() {
    // 100 threads calling native functions
    // GC running continuously
    // Random Task migrations
    // Run for 60 seconds
    // No crashes, no data races, no leaks
}
```

---

## Summary

**Thread Safety:**
- All native functions must be `Send + Sync`
- No unsynchronized shared mutable state
- Use atomics, `Mutex`, or `RwLock` for shared state

**GC Safety:**
- Pin values before passing to native code
- Unpin after native call returns
- GC respects pin_count
- RAII guards prevent leaks on panic

**Memory Safety:**
- Clear ownership model (borrowed during call)
- Panic catching prevents unwinding across FFI
- Proper cleanup on module unload
- No dangling pointers

**ABI Guarantees:**
- `NativeFn` is `Send + Sync` (enforced by type system)
- `NativeValue` is `Send + Sync` (explicit markers)
- `NativeModule` is `Send + Sync` after initialization
- All conversions are safe with proper error handling

**See Also:**
- [design/NATIVE_BINDINGS.md](./NATIVE_BINDINGS.md) - Full native module design
- [design/ARCHITECTURE.md](./ARCHITECTURE.md) - VM architecture
- [plans/milestone-1.15.md](../plans/milestone-1.15.md) - Implementation plan
