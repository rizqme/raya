# Simple Native Binding Example

**Purpose:** Demonstrates basic native module creation with pure functions and primitive types.

**Difficulty:** Beginner

---

## Overview

This example creates a simple math utilities module with pure functions that operate on primitives (numbers). It demonstrates:

- Basic `#[function]` and `#[module]` macros
- Primitive type conversion (i32, f64, bool)
- Error handling with Result types
- Zero overhead FFI calls

**Performance:** Each function call ~25-50ns overhead.

---

## Step 1: Create Rust Library

**File:** `my-math-utils/src/lib.rs`

```rust
use raya_ffi::{FromRaya, NativeModule, ToRaya};
use raya_native::{function, module};

/// Add two integers
#[function]
fn add(a: i32, b: i32) -> i32 {
    a + b
}

/// Multiply two floats
#[function]
fn multiply(a: f64, b: f64) -> f64 {
    a * b
}

/// Check if number is even
#[function]
fn is_even(n: i32) -> bool {
    n % 2 == 0
}

/// Calculate power (with error handling)
#[function]
fn power(base: f64, exp: f64) -> Result<f64, String> {
    if exp < 0.0 && base == 0.0 {
        Err("Cannot raise zero to negative power".to_string())
    } else {
        Ok(base.powf(exp))
    }
}

/// Register the module
#[module]
fn init() -> NativeModule {
    let mut module = NativeModule::new("math-utils", "1.0.0");

    module.register_function("add", add);
    module.register_function("multiply", multiply);
    module.register_function("isEven", is_even);
    module.register_function("power", power);

    module
}
```

**File:** `my-math-utils/Cargo.toml`

```toml
[package]
name = "my-math-utils"
version = "1.0.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
raya-ffi = { path = "../raya/crates/raya-ffi" }
raya-native = { path = "../raya/crates/raya-native" }
```

---

## Step 2: Build Native Module

```bash
cd my-math-utils
cargo build --release

# Output: target/release/libmy_math_utils.so (Linux)
#         target/release/libmy_math_utils.dylib (macOS)
#         target/release/my_math_utils.dll (Windows)
```

---

## Step 3: Create Type Definitions

**File:** `types/math-utils.d.raya`

```typescript
/**
 * Math utility functions
 */

/** Add two integers */
export function add(a: number, b: number): number;

/** Multiply two floats */
export function multiply(a: number, b: number): number;

/** Check if number is even */
export function isEven(n: number): boolean;

/** Calculate power (may throw on invalid input) */
export function power(base: number, exp: number): number;
```

---

## Step 4: Configure in raya.toml

**File:** `raya.toml`

```toml
[package]
name = "my-app"
version = "1.0.0"

[native-bindings]
"custom:math-utils" = {
    lib = "my_math_utils",
    types = "./types/math-utils.d.raya"
}
```

**How it works:**
- `"custom:math-utils"` - Module specifier for imports
- `lib = "my_math_utils"` - Shared library name (without extension)
- `types = "./types/math-utils.d.raya"` - Type definitions for type checker

The VM will search for the library in:
1. `~/.raya/native/` (global native modules)
2. `./native/` (project-local native modules)
3. System library paths (LD_LIBRARY_PATH, etc.)

---

## Step 5: Use in Raya Code

**File:** `main.raya`

```typescript
// Import looks identical to bytecode modules - full transparency
import { add, multiply, isEven, power } from "custom:math-utils";

// Use the functions normally
const sum = add(5, 3);           // 8
const product = multiply(4.5, 2.0);  // 9.0
const even = isEven(42);          // true

logger.info(`5 + 3 = ${sum}`);
logger.info(`4.5 * 2.0 = ${product}`);
logger.info(`Is 42 even? ${even}`);

// Error handling with Result types
try {
    const result = power(2.0, 8.0);  // 256.0
    logger.info(`2^8 = ${result}`);
} catch (e) {
    logger.error(`Error: ${e}`);
}

// This will throw
try {
    const bad = power(0.0, -1.0);  // Error!
} catch (e) {
    logger.error(`Expected error: ${e}`);
}
```

**Output:**
```
5 + 3 = 8
4.5 * 2.0 = 9.0
Is 42 even? true
2^8 = 256.0
Expected error: Cannot raise zero to negative power
```

---

## Step 6: Run

```bash
rayac run main.raya

# The VM automatically:
# 1. Reads raya.toml
# 2. Loads libmy_math_utils.so
# 3. Calls raya_module_init() to get NativeModule
# 4. Registers functions in module registry
# 5. Resolves imports transparently
```

---

## What Happens Under the Hood

### Type-Aware Optimization

When the compiler knows the types statically:

```typescript
const sum = add(5, 3);  // Compiler knows both are number
```

The generated code uses **optimized unwrap**:

```
LOAD_CONST 5          // Load Value::i32(5)
LOAD_CONST 3          // Load Value::i32(3)
CALL_NATIVE "add"     // Direct call, no type checks
```

The `#[function]` macro generates:

```rust
extern "C" fn add_ffi(args: *const NativeValue, arg_count: usize) -> NativeValue {
    // FAST PATH: No type checking needed
    // Compiler guarantees args[0] is i32, args[1] is i32

    let a = unsafe { (*args.offset(0)).as_value().as_i32_unchecked() };
    let b = unsafe { (*args.offset(1)).as_value().as_i32_unchecked() };

    (a + b).to_raya()
}
```

**Performance:** ~25ns total overhead (no type checking)

---

## VM Lifecycle

### 1. Module Loading (VM startup)

```rust
// VM reads raya.toml
let config = Config::load("raya.toml")?;

// Load native library
let lib = Library::new("libmy_math_utils.so")?;

// Get module initializer
let init: extern "C" fn() -> NativeModule = lib.get("raya_module_init")?;

// Register module
let module = init();
vm.register_native_module("custom:math-utils", module)?;
```

### 2. Import Resolution (compile time)

```typescript
import { add } from "custom:math-utils";
```

Compiler:
1. Checks `raya.toml` for `"custom:math-utils"`
2. Loads type definitions from `./types/math-utils.d.raya`
3. Type-checks usage against declarations
4. Generates optimized CALL_NATIVE instruction

### 3. Function Call (runtime)

```typescript
const result = add(5, 3);
```

VM:
1. Looks up "custom:math-utils" in module registry
2. Gets `add` function pointer
3. Pins argument Values (GC safety)
4. Calls `add_ffi(args, 2)`
5. Unpins arguments (RAII guard)
6. Returns NativeValue result

**Total overhead:** ~25-50ns

---

## Key Takeaways

**For Rust developers:**
- Use `#[function]` for pure functions with primitives
- Return `Result<T, E>` for fallible operations
- No manual FFI wrapper code needed
- Proc-macros handle all marshalling

**For Raya users:**
- Native modules look identical to bytecode modules
- No special syntax or imports required
- Full type safety from type definitions
- Transparent performance - no overhead awareness needed

**Performance:**
- Pure functions: ~25-50ns overhead
- Type-aware optimization when types known
- Zero-copy for primitives
- No runtime type checking

---

## Troubleshooting

### Error: "Module not found: custom:math-utils"

**Cause:** Library not found in search paths

**Fix:** Copy library to `~/.raya/native/` or `./native/`:

```bash
mkdir -p ~/.raya/native/
cp target/release/libmy_math_utils.so ~/.raya/native/
```

### Error: "Symbol not found: raya_module_init"

**Cause:** Missing `#[module]` macro or wrong crate type

**Fix:** Ensure Cargo.toml has:

```toml
[lib]
crate-type = ["cdylib"]
```

### Error: "Type mismatch: expected number, got string"

**Cause:** Type definitions don't match Rust implementation

**Fix:** Ensure `.d.raya` file matches Rust function signatures exactly.

---

## Next Steps

- See [NATIVE_BINDING_COMPLEX_EXAMPLE.md](./NATIVE_BINDING_COMPLEX_EXAMPLE.md) for:
  - Stateful modules (persistent state across calls)
  - String handling (zero-copy)
  - Async operations (Tasks)
  - Error handling patterns
  - Wrapping C libraries in Rust

- See [NATIVE_BINDINGS.md](./NATIVE_BINDINGS.md) for full design specification

- See [ABI_SAFETY.md](./ABI_SAFETY.md) for thread safety and GC safety details
