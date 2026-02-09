# Complex Native Binding Example

**Purpose:** Demonstrates advanced native module features including state management, async operations, string handling, and C library wrapping.

**Difficulty:** Advanced

---

## Overview

This example creates a database connection pool module that demonstrates:

- Opaque object handles (returning Rust objects to Raya)
- Zero-copy string handling (opaque handles)
- Async operations (spawning Tasks)
- Complex error handling
- Wrapping C libraries (libpq PostgreSQL client)
- Thread safety with Mutex
- Resource cleanup with Drop

**Performance:** ~50-100ns per call (includes opaque handle access)

---

## Example: PostgreSQL Connection Pool

This module wraps the libpq C library and provides a connection pool with async query execution.

---

## Step 1: Wrap C Library in Rust

**File:** `raya-postgres/src/ffi.rs`

```rust
// Wrap libpq C API using standard Rust FFI
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};

#[repr(C)]
struct PGconn {
    _private: [u8; 0],
}

#[repr(C)]
struct PGresult {
    _private: [u8; 0],
}

#[link(name = "pq")]
extern "C" {
    fn PQconnectdb(conninfo: *const c_char) -> *mut PGconn;
    fn PQfinish(conn: *mut PGconn);
    fn PQexec(conn: *mut PGconn, query: *const c_char) -> *mut PGresult;
    fn PQclear(result: *mut PGresult);
    fn PQntuples(result: *const PGresult) -> c_int;
    fn PQgetvalue(result: *const PGresult, row: c_int, col: c_int) -> *const c_char;
}

// Safe Rust wrapper
pub struct Connection {
    conn: *mut PGconn,
}

impl Connection {
    pub fn connect(connection_string: &str) -> Result<Self, String> {
        let c_str = CString::new(connection_string)
            .map_err(|e| format!("Invalid connection string: {}", e))?;

        let conn = unsafe { PQconnectdb(c_str.as_ptr()) };

        if conn.is_null() {
            return Err("Failed to connect to database".to_string());
        }

        Ok(Connection { conn })
    }

    pub fn execute(&mut self, query: &str) -> Result<QueryResult, String> {
        let c_query = CString::new(query)
            .map_err(|e| format!("Invalid query: {}", e))?;

        let result = unsafe { PQexec(self.conn, c_query.as_ptr()) };

        if result.is_null() {
            return Err("Query execution failed".to_string());
        }

        Ok(QueryResult { result })
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        unsafe { PQfinish(self.conn); }
    }
}

pub struct QueryResult {
    result: *mut PGresult,
}

impl QueryResult {
    pub fn row_count(&self) -> usize {
        unsafe { PQntuples(self.result) as usize }
    }

    pub fn get_value(&self, row: usize, col: usize) -> String {
        unsafe {
            let c_str = PQgetvalue(self.result, row as c_int, col as c_int);
            CStr::from_ptr(c_str).to_string_lossy().into_owned()
        }
    }
}

impl Drop for QueryResult {
    fn drop(&mut self) {
        unsafe { PQclear(self.result); }
    }
}
```

---

## Step 2: Implement Connection Pool

**File:** `raya-postgres/src/pool.rs`

```rust
use std::sync::{Arc, Mutex};
use std::collections::VecDeque;
use crate::ffi::Connection;

// Thread-safe connection pool
pub struct ConnectionPool {
    available: Mutex<VecDeque<Connection>>,
    connection_string: String,
    max_size: usize,
}

impl ConnectionPool {
    pub fn new(connection_string: String, max_size: usize) -> Result<Self, String> {
        Ok(ConnectionPool {
            available: Mutex::new(VecDeque::new()),
            connection_string,
            max_size,
        })
    }

    pub fn get_connection(&self) -> Result<PooledConnection, String> {
        let mut available = self.available.lock().unwrap();

        let conn = if let Some(conn) = available.pop_front() {
            // Reuse existing connection
            conn
        } else {
            // Create new connection
            Connection::connect(&self.connection_string)?
        };

        Ok(PooledConnection {
            conn: Some(conn),
            pool: self,
        })
    }

    fn return_connection(&self, conn: Connection) {
        let mut available = self.available.lock().unwrap();

        if available.len() < self.max_size {
            available.push_back(conn);
        }
        // Otherwise drop (RAII cleanup)
    }
}

// RAII wrapper - returns connection to pool on drop
pub struct PooledConnection<'a> {
    conn: Option<Connection>,
    pool: &'a ConnectionPool,
}

impl<'a> PooledConnection<'a> {
    pub fn execute(&mut self, query: &str) -> Result<QueryResult, String> {
        self.conn.as_mut().unwrap().execute(query)
    }
}

impl<'a> Drop for PooledConnection<'a> {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            self.pool.return_connection(conn);
        }
    }
}
```

---

## Step 3: Implement Raya Native Module

**File:** `raya-postgres/src/lib.rs`

```rust
use raya_ffi::{FromRaya, NativeModule, NativeValue, ToRaya};
use raya_native::{function, module};
use std::sync::Arc;

mod ffi;
mod pool;
use pool::ConnectionPool;

// Opaque handle to ConnectionPool (returned to Raya as object)
pub struct PoolHandle {
    pool: Arc<ConnectionPool>,
}

impl ToRaya for PoolHandle {
    fn to_raya(self) -> NativeValue {
        // Return opaque handle - Raya sees this as an object
        // The VM stores the Box<PoolHandle> and gives Raya a reference
        NativeValue::from_value(Value::opaque(Box::new(self)))
    }
}

impl FromRaya for &PoolHandle {
    fn from_raya(value: NativeValue) -> Result<Self, NativeError> {
        // Extract opaque handle back to &PoolHandle
        unsafe {
            value.as_value().as_opaque::<PoolHandle>()
                .ok_or_else(|| NativeError::TypeMismatch {
                    expected: "PoolHandle".to_string(),
                    got: "other".to_string(),
                })
        }
    }
}

// Opaque handle to QueryResult (returned to Raya as object)
pub struct QueryResult {
    rows: Vec<Vec<String>>,
}

impl ToRaya for QueryResult {
    fn to_raya(self) -> NativeValue {
        NativeValue::from_value(Value::opaque(Box::new(self)))
    }
}

impl FromRaya for &QueryResult {
    fn from_raya(value: NativeValue) -> Result<Self, NativeError> {
        unsafe {
            value.as_value().as_opaque::<QueryResult>()
                .ok_or_else(|| NativeError::TypeMismatch {
                    expected: "QueryResult".to_string(),
                    got: "other".to_string(),
                })
        }
    }
}

/// Create a new connection pool (returns opaque PoolHandle)
#[function]
fn create_pool(connection_string: NativeValue, max_size: i32) -> Result<PoolHandle, String> {
    // Zero-copy string access via opaque handle
    let conn_str = unsafe { connection_string.as_value().as_string()? };

    let pool = ConnectionPool::new(conn_str.to_string(), max_size as usize)?;

    Ok(PoolHandle {
        pool: Arc::new(pool),
    })
}

/// Execute query asynchronously (spawns Task)
#[function]
async fn query(pool: &PoolHandle, sql: NativeValue) -> Result<QueryResult, String> {
    let sql_str = unsafe { sql.as_value().as_string()? };

    // Get connection from pool
    let mut conn = pool.pool.get_connection()?;

    // Execute query (may block - but we're in a Task so we don't block OS thread)
    let result = conn.execute(sql_str)?;

    // Convert to Raya-friendly format
    let rows = (0..result.row_count())
        .map(|i| {
            // For simplicity, assume 2 columns
            vec![
                result.get_value(i, 0),
                result.get_value(i, 1),
            ]
        })
        .collect::<Vec<_>>();

    Ok(QueryResult { rows })
}

/// Get number of rows in result
#[function]
fn get_row_count(result: &QueryResult) -> i32 {
    result.rows.len() as i32
}

/// Get a specific row from result
#[function]
fn get_row(result: &QueryResult, index: i32) -> Result<Vec<String>, String> {
    result.rows.get(index as usize)
        .cloned()
        .ok_or_else(|| format!("Row {} out of bounds", index))
}

/// Register module
#[module]
fn init() -> NativeModule {
    let mut module = NativeModule::new("postgres", "1.0.0");

    module.register_function("createPool", create_pool);
    module.register_function("query", query);
    module.register_function("getRowCount", get_row_count);
    module.register_function("getRow", get_row);

    module
}
```

**File:** `raya-postgres/Cargo.toml`

```toml
[package]
name = "raya-postgres"
version = "1.0.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
raya-ffi = { path = "../raya/crates/raya-ffi" }
raya-native = { path = "../raya/crates/raya-native" }
lazy_static = "1.4"
```

---

## Step 4: Create Type Definitions

**File:** `types/postgres.d.raya`

```typescript
/**
 * PostgreSQL database client with connection pooling
 */

/** Opaque connection pool handle */
export interface Pool {
    // Opaque object - no accessible properties
    // Only usable with module functions
}

/** Opaque query result handle */
export interface QueryResult {
    // Opaque object - access via functions
}

/**
 * Create a new connection pool
 * @param connectionString - PostgreSQL connection string (e.g., "host=localhost dbname=mydb")
 * @param maxSize - Maximum number of connections in pool
 * @returns Pool handle
 */
export function createPool(connectionString: string, maxSize: number): Pool;

/**
 * Execute a SQL query asynchronously
 * @param pool - Pool handle from createPool()
 * @param sql - SQL query string
 * @returns Query results
 */
export async function query(pool: Pool, sql: string): Task<QueryResult>;

/**
 * Get number of rows in result
 * @param result - Query result from query()
 * @returns Row count
 */
export function getRowCount(result: QueryResult): number;

/**
 * Get a specific row from result
 * @param result - Query result from query()
 * @param index - Row index (0-based)
 * @returns Row data as array of strings
 */
export function getRow(result: QueryResult, index: number): string[];
```

---

## Step 5: Configure in raya.toml

**File:** `raya.toml`

```toml
[package]
name = "my-web-app"
version = "1.0.0"

[native-bindings]
"custom:postgres" = {
    lib = "raya_postgres",
    types = "./types/postgres.d.raya"
}

# System library dependency (libpq)
# User must install: apt-get install libpq-dev (Ubuntu)
#                    brew install libpq (macOS)
```

---

## Step 6: Use in Raya Code

**File:** `server.raya`

```typescript
import { createPool, query, getRowCount, getRow } from "custom:postgres";

// Create connection pool (returns opaque Pool object)
const pool = createPool(
    "host=localhost port=5432 dbname=myapp user=postgres password=secret",
    10  // max connections
);

// Query function (async - spawns Task automatically)
async function getUsers(): Task<void> {
    // pool is passed as object - zero overhead
    const result = await query(pool, "SELECT id, name FROM users");

    const rowCount = getRowCount(result);
    logger.info(`Found ${rowCount} users`);

    // Iterate through rows
    for (let i = 0; i < rowCount; i++) {
        const row = getRow(result, i);
        logger.info(`User ${row[0]}: ${row[1]}`);
    }
}

// Multiple concurrent queries (work-stealing scheduler)
async function main(): Task<void> {
    // Spawn 100 concurrent queries (efficient thanks to connection pool)
    const tasks: Task<void>[] = [];

    for (let i = 0; i < 100; i++) {
        tasks.push(getUsers());  // Each call spawns a new Task
    }

    // Wait for all to complete
    for (const task of tasks) {
        await task;
    }

    // Pool is automatically cleaned up when GC collects it (Drop trait)
}

main();
```

**Output:**
```
Found 3 users
User 1: Alice
User 2: Bob
User 3: Charlie
...
(100 concurrent queries, efficiently pooled)
```

---

## What Happens Under the Hood

### 1. Module Loading (VM startup)

```rust
// VM loads libpq and libraya_postgres
let libpq = Library::new("libpq.so")?;  // System library
let lib = Library::new("libraya_postgres.so")?;  // Raya native module

// Initialize module
let init: extern "C" fn() -> NativeModule = lib.get("raya_module_init")?;
let module = init();

// Register as "custom:postgres"
vm.register_native_module("custom:postgres", module)?;
```

### 2. Creating Opaque Objects (Returned to Raya)

```typescript
const pool = createPool("host=localhost...", 10);
```

Rust side:
```rust
#[function]
fn create_pool(connection_string: NativeValue, max_size: i32) -> Result<PoolHandle, String> {
    let pool = ConnectionPool::new(...)?;

    // Return opaque handle
    Ok(PoolHandle {
        pool: Arc::new(pool),
    })
}

impl ToRaya for PoolHandle {
    fn to_raya(self) -> NativeValue {
        // Box the handle and store in VM heap
        NativeValue::from_value(Value::opaque(Box::new(self)))
    }
}
```

VM behavior:
1. Creates `Box<PoolHandle>` on heap
2. Returns `Value::opaque(ptr)` to Raya
3. Raya variable `pool` holds opaque reference
4. When `pool` is passed back to native functions, VM extracts `&PoolHandle`

**Thread Safety:**
- Arc inside PoolHandle allows sharing across Tasks
- Each Task may run on different OS threads
- VM ensures opaque object isn't accessed concurrently during native calls

### 3. Passing Opaque Objects to Functions

```typescript
const result = await query(pool, "SELECT ...");
```

Rust side:
```rust
#[function]
async fn query(pool: &PoolHandle, sql: NativeValue) -> Result<QueryResult, String> {
    // pool is &PoolHandle - extracted from opaque Value
    let mut conn = pool.pool.get_connection()?;
    // ...
}

impl FromRaya for &PoolHandle {
    fn from_raya(value: NativeValue) -> Result<Self, NativeError> {
        // Extract reference from opaque Value
        unsafe { value.as_value().as_opaque::<PoolHandle>()? }
    }
}
```

VM behavior:
1. Raya passes `pool` variable (opaque Value)
2. VM pins the opaque Value (GC safety)
3. Proc-macro extracts `&PoolHandle` from opaque pointer
4. Function executes with direct Rust reference
5. VM unpins after function returns

**Performance:**
- Zero-copy object passing
- Direct pointer dereference (~1ns)
- No serialization/deserialization

### 4. Async Query Execution (Task Spawning)

```typescript
const result = await query(pool, "SELECT ...");
```

VM behavior:
1. `query()` is marked `async` in Rust
2. VM spawns a new Task automatically
3. Task executes query (may block, but doesn't block OS thread)
4. Work-stealing scheduler distributes Tasks across CPU cores
5. `await` suspends caller Task until result ready

**Concurrency:**
- 100 queries → 100 Tasks
- Tasks distributed across all CPU cores
- Connection pool manages resource contention
- No OS thread blocking

### 5. Zero-Copy String Handling

```rust
#[function]
async fn query(pool: &PoolHandle, sql: NativeValue) -> Result<QueryResult, String> {
    // Zero-copy access to string (opaque handle)
    let sql_str = unsafe { sql.as_value().as_string()? };

    // sql_str: &str pointing into VM heap (no copy!)
    let mut conn = pool.pool.get_connection()?;
    conn.execute(sql_str)?;
}
```

**Performance:**
- No string allocation
- No memcpy
- Direct pointer into VM heap
- GC pins strings during call (atomically)

---

## Advanced Patterns

### Pattern 1: Error Handling with Custom Types

```rust
#[derive(Debug, Clone)]
pub enum DbError {
    ConnectionFailed(String),
    QueryFailed(String),
    Timeout,
}

impl ToString for DbError {
    fn to_string(&self) -> String {
        match self {
            DbError::ConnectionFailed(msg) => format!("Connection error: {}", msg),
            DbError::QueryFailed(msg) => format!("Query error: {}", msg),
            DbError::Timeout => "Query timeout".to_string(),
        }
    }
}

#[function]
fn query_with_timeout(pool: NativeValue, sql: NativeValue, timeout_ms: i32)
    -> Result<QueryResult, DbError>
{
    // Implementation...
}
```

Raya side:
```typescript
try {
    const result = await queryWithTimeout("main", "SELECT ...", 5000);
} catch (e) {
    // e is string from DbError::to_string()
    logger.error(`Database error: ${e}`);
}
```

### Pattern 2: Object Methods (Opaque Handles)

```rust
// Instead of global state, pass objects around
#[function]
fn pool_get_stats(pool: &PoolHandle) -> PoolStats {
    let stats = pool.pool.get_stats();
    PoolStats {
        active: stats.active,
        idle: stats.idle,
    }
}

pub struct PoolStats {
    active: i32,
    idle: i32,
}

impl ToRaya for PoolStats {
    fn to_raya(self) -> NativeValue {
        // Could return as object with properties
        // Or as opaque handle with accessor functions
        NativeValue::from_value(Value::opaque(Box::new(self)))
    }
}
```

Raya side:
```typescript
const stats = poolGetStats(pool);
logger.info(`Active: ${stats.active}, Idle: ${stats.idle}`);
```

### Pattern 3: Resource Cleanup (RAII)

```rust
// Opaque handle with Drop implementation
pub struct PoolHandle {
    pool: Arc<ConnectionPool>,
}

impl Drop for PoolHandle {
    fn drop(&mut self) {
        // Automatic cleanup when Raya GC collects the opaque object
        println!("Pool handle dropped, cleaning up connections");
        // Arc automatically cleans up when last reference is dropped
    }
}
```

Raya side:
```typescript
{
    const pool = createPool("...", 10);
    // Use pool...
}  // pool goes out of scope

// Later: GC runs, collects opaque Pool object, calls Drop
```

**Key insight:** Raya users never need to manually call cleanup functions - RAII handles it!

---

## Performance Characteristics

### Overhead Breakdown

**Simple function call (primitives only):**
- Type checking: 0ns (compile-time)
- GC pinning: ~5ns (atomic increment)
- FFI call: ~10ns (function pointer)
- Value conversion: ~5ns (unboxing)
- GC unpinning: ~5ns (atomic decrement)
- **Total: ~25ns**

**Complex function call (opaque object + string):**
- Type checking: 0ns
- GC pinning: ~10ns (object + string)
- FFI call: ~10ns
- Opaque handle extraction: ~1ns (pointer dereference)
- String access: ~1ns (zero-copy)
- Arc clone (if needed): ~5ns (atomic increment)
- Value conversion: ~5ns
- GC unpinning: ~10ns
- **Total: ~42ns**

**Async query execution:**
- Above + Task spawn: ~500ns
- Query execution: depends on database (ms)
- Task resume: ~200ns
- **FFI overhead: ~1μs (negligible vs query time)**

---

## Thread Safety Guarantees

### Safe Patterns

**✅ Mutex-protected shared state:**
```rust
static STATE: Mutex<HashMap<String, Data>> = Mutex::new(HashMap::new());
```

**✅ Atomic counters:**
```rust
static COUNTER: AtomicU64 = AtomicU64::new(0);
```

**✅ Immutable shared state:**
```rust
static CONFIG: &str = "config_value";
```

### Unsafe Patterns

**❌ Thread-local mutable state:**
```rust
thread_local! {
    static STATE: RefCell<Data> = RefCell::new(Data::new());
}
// Won't work - Tasks migrate across threads!
```

**❌ Rc (non-atomic reference counting):**
```rust
static STATE: Rc<Data> = ...;  // Not Send
// Won't compile - Rc is not Send
```

---

## GC Safety Guarantees

**Before native call:**
```rust
// VM pins all arguments
for arg in &args {
    pin_value(*arg);  // atomic increment
}
```

**During native call:**
```rust
// GC sweep phase
if (*value_header).pin_count.load(Ordering::Acquire) > 0 {
    // Pinned - DO NOT COLLECT
    return;
}
```

**After native call:**
```rust
// RAII guard ensures unpinning even on panic
impl Drop for PinGuard {
    fn drop(&mut self) {
        unpin_value(self.value);  // atomic decrement
    }
}
```

---

## Key Takeaways

**For Rust developers:**
- Wrap C libraries using standard Rust FFI first
- Return opaque handles (custom structs) for stateful objects
- Use Arc inside handles for safe sharing across Tasks
- Implement Drop for automatic cleanup (RAII)
- Accept `&Handle` parameters to access opaque objects
- Return Result<T, E> for error handling
- Mark async functions with `async` for Task spawning
- Zero-copy string access via opaque handles

**For Raya users:**
- Native modules are completely transparent
- Objects returned from native functions work seamlessly
- Pass objects to native functions like any other value
- Async functions automatically create Tasks
- Error handling via try/catch (Rust Result → exception)
- Resource cleanup is automatic (RAII + GC) - no manual cleanup needed
- No performance overhead awareness needed

**Performance:**
- Opaque object operations: ~40-50ns overhead
- Zero-copy strings: ~1ns access
- Zero-copy object passing: ~1ns
- Async operations: ~1μs overhead (negligible)
- Connection pooling: efficient concurrency
- Work-stealing: automatic CPU utilization

---

## See Also

- [NATIVE_BINDING_SIMPLE_EXAMPLE.md](./NATIVE_BINDING_SIMPLE_EXAMPLE.md) - Simple pure functions
- [NATIVE_BINDINGS.md](./NATIVE_BINDINGS.md) - Full design specification
- [ABI_SAFETY.md](./ABI_SAFETY.md) - Thread safety and GC safety details
- [plans/milestone-1.15.md](../plans/milestone-1.15.md) - Implementation roadmap
