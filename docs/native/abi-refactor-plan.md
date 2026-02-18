# Plan: ABI Native Type Wrappers + Complex Concurrency Tests

## Context

The ABI (`abi.rs`) has `NativeValue` with primitive accessors and free functions for buffer/string/array/object operations. There are no typed wrappers for complex Raya types, no execution capability, and no task management. The user wants a complete set of **native wrapper types** with full lifecycle support (create, access, execute, await).

**Underlying VM types:**
- `Object` — `class_id`, `fields: Vec<Value>`
- `Array` — `elements: Vec<Value>`
- `Class` — `id`, `name`, `field_count`, `parent_id`, `vtable`, `static_fields`, `constructor_id`
- `Closure` — `func_id`, `captures: Vec<Value>`
- `VTable` — `methods: Vec<usize>` (function IDs)
- `Task` — `id: TaskId`, `state`, `result()`, `wait_completion()`, `cancel()`
- `ClassMetadata` — `field_indices`, `field_names`, `method_indices`, `method_names`
- `ClassMetadataRegistry` — `HashMap<class_id, ClassMetadata>`

---

## Step 1: Expand `NativeContext` — central VM context

**File:** `crates/raya-engine/src/vm/abi.rs`

`NativeContext` becomes the **single entry point** for all ABI operations. It represents the current VM execution context and provides methods for creation, resolution, execution, and task management.

### Fields (internal)
```rust
pub struct NativeContext<'a> {
    pub(crate) gc: &'a Mutex<Gc>,
    pub(crate) classes: &'a RwLock<ClassRegistry>,
    pub(crate) scheduler: &'a Arc<Scheduler>,
    pub(crate) current_task: TaskId,
    pub(crate) class_metadata: &'a RwLock<ClassMetadataRegistry>,  // NEW
    pub(crate) module: &'a Arc<Module>,                             // NEW
}
```

### Methods — Value Creation
```rust
impl<'a> NativeContext<'a> {
    // Allocate new values on GC heap
    pub fn create_string(&self, s: String) -> NativeValue;
    pub fn create_array(&self, items: &[NativeValue]) -> NativeValue;
    pub fn create_buffer(&self, data: &[u8]) -> NativeValue;
    pub fn create_object(&self, class_id: usize) -> NativeValue;
}
```

### Methods — Type Resolution
```rust
impl<'a> NativeContext<'a> {
    // Class lookup
    pub fn get_class(&self, class_id: usize) -> AbiResult<NativeClass>;
    pub fn get_class_by_name(&self, name: &str) -> AbiResult<NativeClass>;

    // Schema resolution (cached by caller)
    pub fn resolve_schema(&self, class_id: usize) -> AbiResult<ObjectSchema>;

    // Method resolution
    pub fn resolve_method(&self, class_id: usize, method_name: &str) -> AbiResult<NativeMethod>;
}
```

### Methods — Execution
```rust
impl<'a> NativeContext<'a> {
    // Execute a function by ID (sync — blocks until complete)
    pub fn call_function(&self, func_id: usize, args: &[NativeValue]) -> AbiResult<NativeValue>;

    // Execute a function as async task (returns immediately)
    pub fn spawn_function(&self, func_id: usize, args: &[NativeValue]) -> AbiResult<NativeTask>;

    // Execute a closure (sync)
    pub fn call_closure(&self, closure: &NativeFunction, args: &[NativeValue]) -> AbiResult<NativeValue>;

    // Execute a closure as async task
    pub fn spawn_closure(&self, closure: &NativeFunction, args: &[NativeValue]) -> AbiResult<NativeTask>;

    // Call method on object (sync)
    pub fn call_method(&self, receiver: NativeValue, method: &NativeMethod, args: &[NativeValue]) -> AbiResult<NativeValue>;

    // Call method as async task
    pub fn spawn_method(&self, receiver: NativeValue, method: &NativeMethod, args: &[NativeValue]) -> AbiResult<NativeTask>;
}
```

### Methods — Task Management
```rust
impl<'a> NativeContext<'a> {
    // Current task info
    pub fn current_task_id(&self) -> u64;

    // Await a task (blocks until result available)
    pub fn await_task(&self, task: &NativeTask) -> AbiResult<NativeValue>;

    // Await with timeout
    pub fn await_task_timeout(&self, task: &NativeTask, timeout: Duration) -> AbiResult<Option<NativeValue>>;

    // Await multiple tasks in parallel
    pub fn await_all(&self, tasks: &[NativeTask]) -> AbiResult<Vec<NativeValue>>;

    // Cancel a task
    pub fn cancel_task(&self, task: &NativeTask);
}
```

### Methods — Value Conversion
```rust
impl<'a> NativeContext<'a> {
    // Wrap raw values into typed wrappers
    pub fn as_array(&self, val: NativeValue) -> AbiResult<NativeArray>;
    pub fn as_object(&self, val: NativeValue, schema: &ObjectSchema) -> AbiResult<NativeObject>;
    pub fn as_function(&self, val: NativeValue) -> AbiResult<NativeFunction>;
}
```

Update call sites: `core.rs` (Interpreter::new), `worker.rs` (task execution loop).

---

## Step 2: `NativeArray`

```rust
pub struct NativeArray {
    value: NativeValue,
}

impl NativeArray {
    pub fn from_value(val: NativeValue) -> AbiResult<Self>;
    pub fn len(&self) -> AbiResult<usize>;
    pub fn is_empty(&self) -> AbiResult<bool>;

    // Element access
    pub fn get(&self, index: usize) -> AbiResult<NativeValue>;
    pub fn get_i32(&self, index: usize) -> AbiResult<i32>;
    pub fn get_f64(&self, index: usize) -> AbiResult<f64>;
    pub fn get_bool(&self, index: usize) -> AbiResult<bool>;
    pub fn get_string(&self, index: usize) -> AbiResult<String>;

    // Bulk conversion
    pub fn to_vec(&self) -> AbiResult<Vec<NativeValue>>;
    pub fn to_vec_i32(&self) -> AbiResult<Vec<i32>>;
    pub fn to_vec_f64(&self) -> AbiResult<Vec<f64>>;
    pub fn to_vec_string(&self) -> AbiResult<Vec<String>>;

    pub fn into_value(self) -> NativeValue;
}
```

---

## Step 3: `ObjectSchema` + `NativeObject`

```rust
/// Cached schema resolved from reflect metadata (create once, reuse for all objects of same class)
pub struct ObjectSchema {
    class_id: usize,
    class_name: String,
    field_lookup: FxHashMap<String, usize>,
    field_names: Vec<String>,
    method_lookup: FxHashMap<String, usize>,
    method_names: Vec<String>,
}

impl ObjectSchema {
    pub fn from_metadata(ctx: &NativeContext, class_id: usize) -> AbiResult<Self>;
    pub fn builder(class_id: usize, class_name: &str) -> ObjectSchemaBuilder;
    pub fn field_index(&self, name: &str) -> Option<usize>;
    pub fn method_index(&self, name: &str) -> Option<usize>;
    pub fn field_count(&self) -> usize;
    pub fn class_name(&self) -> &str;
}

/// Object wrapper with cached schema-based named access
pub struct NativeObject<'s> {
    value: NativeValue,
    schema: &'s ObjectSchema,
}

impl<'s> NativeObject<'s> {
    pub fn wrap(val: NativeValue, schema: &'s ObjectSchema) -> AbiResult<Self>;

    // Field getters by name
    pub fn get(&self, name: &str) -> AbiResult<NativeValue>;
    pub fn get_i32(&self, name: &str) -> AbiResult<i32>;
    pub fn get_f64(&self, name: &str) -> AbiResult<f64>;
    pub fn get_bool(&self, name: &str) -> AbiResult<bool>;
    pub fn get_string(&self, name: &str) -> AbiResult<String>;
    pub fn get_array(&self, name: &str) -> AbiResult<NativeArray>;
    pub fn get_object(&self, name: &str, schema: &'s ObjectSchema) -> AbiResult<NativeObject<'s>>;

    // Field setters by name
    pub fn set(&self, name: &str, value: NativeValue) -> AbiResult<()>;
    pub fn set_i32(&self, name: &str, value: i32) -> AbiResult<()>;

    pub fn field_count(&self) -> usize;
    pub fn class_id(&self) -> AbiResult<usize>;
    pub fn schema(&self) -> &ObjectSchema;
    pub fn into_value(self) -> NativeValue;
}
```

---

## Step 4: `NativeClass`

```rust
pub struct NativeClass {
    class_id: usize,
    name: String,
    field_count: usize,
    parent_id: Option<usize>,
    constructor_id: Option<usize>,
    method_count: usize,
}

impl NativeClass {
    pub fn from_id(ctx: &NativeContext, class_id: usize) -> AbiResult<Self>;
    pub fn from_name(ctx: &NativeContext, name: &str) -> AbiResult<Self>;

    pub fn id(&self) -> usize;
    pub fn name(&self) -> &str;
    pub fn field_count(&self) -> usize;
    pub fn parent_id(&self) -> Option<usize>;
    pub fn constructor_id(&self) -> Option<usize>;
    pub fn method_count(&self) -> usize;

    /// Create ObjectSchema for this class from reflect metadata
    pub fn schema(&self, ctx: &NativeContext) -> AbiResult<ObjectSchema>;

    /// Allocate a new instance (uninitialized fields)
    pub fn instantiate(&self, ctx: &NativeContext) -> NativeValue;
}
```

---

## Step 5: `NativeFunction` — callable

```rust
pub struct NativeFunction {
    value: NativeValue,
    func_id: usize,
    capture_count: usize,
}

impl NativeFunction {
    pub fn from_value(val: NativeValue) -> AbiResult<Self>;

    pub fn func_id(&self) -> usize;
    pub fn capture_count(&self) -> usize;
    pub fn get_capture(&self, index: usize) -> AbiResult<NativeValue>;

    /// Execute this function synchronously, blocking until complete
    /// Spawns as task internally, waits for result
    pub fn call(&self, ctx: &NativeContext, args: &[NativeValue]) -> AbiResult<NativeValue>;

    /// Execute as async task, returns NativeTask handle (non-blocking)
    pub fn call_async(&self, ctx: &NativeContext, args: &[NativeValue]) -> AbiResult<NativeTask>;

    pub fn into_value(self) -> NativeValue;
}
```

**Implementation:** `call()` creates a `Task` with the function ID and args, spawns it via `ctx.scheduler.spawn()`, then calls `task.wait_completion()` + `task.result()`. `call_async()` spawns but returns immediately with a `NativeTask` handle.

---

## Step 6: `NativeMethod` — callable on receiver

```rust
pub struct NativeMethod {
    class_id: usize,
    method_name: String,
    vtable_index: usize,
    function_id: usize,
}

impl NativeMethod {
    /// Resolve from class registry + schema
    pub fn resolve(ctx: &NativeContext, class_id: usize, method_name: &str) -> AbiResult<Self>;

    pub fn class_id(&self) -> usize;
    pub fn name(&self) -> &str;
    pub fn vtable_index(&self) -> usize;
    pub fn function_id(&self) -> usize;

    /// Call method on an object (sync, blocks until complete)
    pub fn call(&self, ctx: &NativeContext, receiver: NativeValue, args: &[NativeValue]) -> AbiResult<NativeValue>;

    /// Call method as async task (returns immediately with task handle)
    pub fn call_async(&self, ctx: &NativeContext, receiver: NativeValue, args: &[NativeValue]) -> AbiResult<NativeTask>;
}
```

---

## Step 7: `NativeTask` — await capability

```rust
/// Wrapper around a Raya Task with await/cancel/status capabilities
pub struct NativeTask {
    task_id: u64,
    scheduler: Arc<Scheduler>,
}

impl NativeTask {
    /// Wrap an existing task by ID
    pub fn from_id(ctx: &NativeContext, task_id: u64) -> Self;

    /// Task ID
    pub fn id(&self) -> u64;

    /// Check if task is done (non-blocking)
    pub fn is_done(&self) -> bool;

    /// Check if task was cancelled
    pub fn is_cancelled(&self) -> bool;

    /// Get task state
    pub fn state(&self) -> TaskState;

    /// Await the task result (blocks current thread until complete)
    pub fn await_result(&self) -> AbiResult<NativeValue>;

    /// Await with timeout (returns None if timeout exceeded)
    pub fn await_timeout(&self, timeout: std::time::Duration) -> AbiResult<Option<NativeValue>>;

    /// Cancel the task
    pub fn cancel(&self);

    /// Await multiple tasks in parallel, returns results in order
    pub fn await_all(tasks: &[NativeTask]) -> AbiResult<Vec<NativeValue>>;
}
```

**Implementation:** `await_result()` looks up the `Task` in the scheduler's task registry, calls `task.wait_completion()`, then `task.result()`. `await_all()` iterates and waits for each (or uses a more efficient approach if available).

---

## Step 8: `NativeValue` convenience methods

```rust
impl NativeValue {
    pub fn as_array(&self) -> AbiResult<NativeArray>;
    pub fn as_function(&self) -> AbiResult<NativeFunction>;
    pub fn as_string(&self) -> AbiResult<String>;
    pub fn to_vec_i32(&self) -> AbiResult<Vec<i32>>;
    pub fn to_vec_f64(&self) -> AbiResult<Vec<f64>>;
}
```

---

## Step 9: ABI Unit Tests

**File:** `crates/raya-engine/src/vm/abi.rs` (inline `#[cfg(test)] mod tests`)

Comprehensive tests for every ABI type. Tests use the VM's actual types (Array, Object, Class, Closure, etc.) to construct `NativeValue`s and verify the wrappers work correctly.

### NativeArray tests
- `test_native_array_from_value` — wrap Array pointer, verify `len()` and `get()`
- `test_native_array_get_i32` — `get_i32()` on array of ints
- `test_native_array_get_f64` — `get_f64()` on array of floats
- `test_native_array_get_string` — `get_string()` on array of string pointers
- `test_native_array_to_vec_i32` — bulk `to_vec_i32()`
- `test_native_array_to_vec_f64` — bulk `to_vec_f64()`
- `test_native_array_to_vec` — `to_vec()` returns NativeValues
- `test_native_array_empty` — empty array, `len()` == 0, `is_empty()` == true
- `test_native_array_out_of_bounds` — `get(999)` returns error
- `test_native_array_from_non_pointer` — `from_value(NativeValue::i32(42))` returns error

### ObjectSchema tests
- `test_object_schema_builder` — build schema with fields, verify `field_index()` and `field_count()`
- `test_object_schema_field_lookup` — `field_index("x")` returns correct index
- `test_object_schema_method_lookup` — `method_index("foo")` returns correct index
- `test_object_schema_unknown_field` — `field_index("missing")` returns None

### NativeObject tests
- `test_native_object_get_i32` — named field access
- `test_native_object_get_f64` — float field access
- `test_native_object_get_string` — string field access
- `test_native_object_get_array` — array field access
- `test_native_object_get_object` — nested object field access (with nested schema)
- `test_native_object_set_i32` — set field by name
- `test_native_object_set_value` — set field with NativeValue
- `test_native_object_unknown_field` — `get("missing")` returns error
- `test_native_object_class_id` — verify `class_id()` matches

### NativeClass tests
- `test_native_class_from_id` — lookup by class ID
- `test_native_class_from_name` — lookup by class name
- `test_native_class_properties` — `name()`, `field_count()`, `parent_id()`, `constructor_id()`
- `test_native_class_instantiate` — allocate new instance via class
- `test_native_class_not_found` — lookup missing class returns error

### NativeFunction tests
- `test_native_function_from_closure` — wrap Closure pointer, verify `func_id()` and `capture_count()`
- `test_native_function_get_capture` — read captured values
- `test_native_function_from_non_closure` — error on non-closure value

### NativeMethod tests
- `test_native_method_resolve` — resolve method from class vtable
- `test_native_method_properties` — `class_id()`, `name()`, `function_id()`
- `test_native_method_not_found` — resolve missing method returns error

### NativeTask tests
- `test_native_task_from_id` — create from task ID
- `test_native_task_id` — verify `id()` returns correct value

### NativeValue convenience tests
- `test_native_value_as_array` — `as_array()` on array value
- `test_native_value_as_function` — `as_function()` on closure value
- `test_native_value_as_string` — `as_string()` on string value
- `test_native_value_primitives` — `i32()`, `f64()`, `bool()`, `null()` round-trip

### NativeContext creation tests
- `test_native_context_create_string` — allocate string via context
- `test_native_context_create_array` — allocate array via context
- `test_native_context_create_buffer` — allocate buffer via context
- `test_native_context_create_object` — allocate object via context
- `test_native_context_current_task_id` — verify task ID accessor

---

## Step 10: Refactor Test Harness

**File:** `crates/raya-runtime/tests/e2e/harness.rs`

Replace raw unsafe extraction with ABI wrappers:

```rust
fn extract_array_i32(value: &Value, source: &str) -> Vec<i32> {
    let nv = NativeValue::from_value(*value);
    NativeArray::from_value(nv)
        .and_then(|arr| arr.to_vec_i32())
        .unwrap_or_else(|e| panic!("...: {}\nSource:\n{}", e, source))
}
```

---

## Step 11: Complex Concurrency Tests

**File:** `crates/raya-runtime/tests/e2e/concurrency_edge_cases.rs`

18 new tests in 4 categories.

### 13. Async Recursive Algorithms (4 tests)
- `test_async_recursive_fibonacci_parallel` — `await [fib(n-1), fib(n-2)]` → 55
- `test_async_recursive_fibonacci_returns_sequence` — fib(0..7) → `[0,1,1,2,3,5,8,13]`
- `test_async_recursive_sum_divide_and_conquer` — parallel range sum → 5050
- `test_async_recursive_power` — fast exponentiation → 1024

### 14. Async Parallel Matrix Multiply (4 tests)
- `test_parallel_matrix_multiply_2x2` → `[19,22,43,50]`
- `test_parallel_matrix_multiply_3x3` → 9-element array
- `test_parallel_vector_dot_product` → i32
- `test_parallel_map_reduce` → array

### 15. Nested Closures with Captured Tasks (5 tests)
- `test_closure_captures_task_and_awaits_it`
- `test_nested_closure_captures_outer_task`
- `test_closure_factory_producing_task_awaiters`
- `test_multiple_closures_share_captured_task` → `[v, v]`
- `test_closure_captures_parallel_await_results`

### 16. Complex Mutex Scenarios (5 tests, 4 workers)
- `test_mutex_prevents_lost_updates_heavy` → 400
- `test_mutex_protects_compound_read_modify_write`
- `test_mutex_bank_transfer_atomicity`
- `test_mutex_protects_running_max`
- `test_mutex_producer_consumer_with_shared_buffer`

---

## Files to Modify

| File | Changes |
|------|---------|
| `crates/raya-engine/src/vm/abi.rs` | Add NativeArray, ObjectSchema, NativeObject, NativeClass, NativeFunction, NativeMethod, NativeTask, NativeValue convenience |
| `crates/raya-engine/src/vm/mod.rs` | Re-export new ABI types |
| `crates/raya-engine/src/vm/interpreter/core.rs` | Pass class_metadata + module to NativeContext |
| `crates/raya-engine/src/vm/scheduler/worker.rs` | Pass class_metadata + module to NativeContext |
| `crates/raya-runtime/tests/e2e/harness.rs` | Refactor extractors to use NativeArray |
| `crates/raya-runtime/tests/e2e/concurrency_edge_cases.rs` | Add 18 new tests |

---

## Execution Order

1. Expand `NativeContext` (add class_metadata, module) + update call sites
2. `NativeArray` (simplest — no schema)
3. `ObjectSchema` + `NativeObject` (cached schema from reflect)
4. `NativeClass` (ClassRegistry wrapper)
5. `NativeFunction` with `call()` / `call_async()` (closure wrapper + execution)
6. `NativeMethod` with `call()` / `call_async()` (vtable resolution + execution)
7. `NativeTask` with `await_result()` / `await_all()` / `cancel()` (task lifecycle)
8. `NativeValue` convenience methods
9. ABI unit tests (~40 tests)
10. Refactor test harness
11. Add concurrency tests (18 tests)
12. Full regression

---

## Verification

```bash
cargo test -p raya-engine              # ABI + existing tests
cargo test -p raya-runtime -- harness  # Harness refactor
cargo test -p raya-runtime -- concurrency_edge_cases
cargo test -p raya-runtime             # Full regression
cargo test -p raya-stdlib
```
