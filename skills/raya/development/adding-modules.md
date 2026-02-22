# Adding Standard Library Modules

Step-by-step guide for adding new stdlib modules.

## Overview

Standard library modules can be:
- **Cross-platform** (`raya-stdlib`) - Work on all platforms
- **POSIX-specific** (`raya-stdlib-posix`) - Require POSIX APIs

## Process

### 1. Define Native IDs (Optional)

If using ID-based dispatch, add to `raya-engine/src/vm/builtin.rs`:

```rust
// My Module (0x7000-0x70FF)
pub const MY_FUNC_1: u16 = 0x7000;
pub const MY_FUNC_2: u16 = 0x7001;
// ...
```

**ID Ranges:**
- Check existing allocations
- Choose unused range
- Document in [Native IDs](../stdlib/native-ids.md)

### 2. Create .raya Source Files

Create paired files in `crates/raya-stdlib/raya/` (or `raya-stdlib-posix/raya/`):

**mymodule.raya** (implementation):
```typescript
export function myFunc1(x: int): int {
    return __NATIVE_CALL(0x7000, x);
}

export function myFunc2(y: string): string {
    return __NATIVE_CALL(0x7001, y);
}
```

**mymodule.d.raya** (type definitions):
```typescript
export function myFunc1(x: int): int;
export function myFunc2(y: string): string;
```

### 3. Register in std:* Module System

Add to `raya-engine/src/compiler/module/std_modules.rs`:

```rust
pub fn get_std_module_source(name: &str) -> Option<&'static str> {
    match name {
        // ... existing modules
        "mymodule" => Some(include_str!("../../../raya-stdlib/raya/mymodule.raya")),
        _ => None,
    }
}
```

### 4. Implement Rust Functions

Create `crates/raya-stdlib/src/mymodule.rs`:

```rust
use raya_sdk::{NativeContext, NativeValue, NativeCallResult};

pub fn call_mymodule_method(
    ctx: &NativeContext,
    id: u16,
    args: &[NativeValue],
) -> NativeCallResult {
    match id {
        0x7000 => my_func_1(ctx, args),
        0x7001 => my_func_2(ctx, args),
        _ => NativeCallResult::Unhandled,
    }
}

fn my_func_1(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let x = args[0].as_i32().unwrap();
    // Implementation
    NativeCallResult::Return(NativeValue::Int(x * 2))
}

fn my_func_2(_ctx: &NativeContext, args: &[NativeValue]) -> NativeCallResult {
    let y = args[0].as_string().unwrap();
    // Implementation
    NativeCallResult::Return(NativeValue::String(y.to_uppercase()))
}
```

### 5. Route in StdNativeHandler

Update `crates/raya-stdlib/src/handler.rs`:

```rust
impl NativeHandler for StdNativeHandler {
    fn call(&self, ctx: &NativeContext, id: u16, args: &[NativeValue]) 
        -> NativeCallResult {
        match id {
            // ... existing ranges
            0x7000..=0x70FF => crate::mymodule::call_mymodule_method(ctx, id, args),
            _ => NativeCallResult::Unhandled,
        }
    }
}
```

### 6. Register for Name-Based Dispatch (Optional)

Update `crates/raya-stdlib/src/registry.rs`:

```rust
pub fn register_stdlib(registry: &mut NativeFunctionRegistry) {
    // ... existing registrations
    
    registry.register("mymodule", "myFunc1", Box::new(my_func_1));
    registry.register("mymodule", "myFunc2", Box::new(my_func_2));
}
```

### 7. Write Tests

Add E2E tests in `crates/raya-runtime/tests/e2e/mymodule.rs`:

```rust
use crate::harness::*;

#[test]
fn test_my_func_1() {
    let source = r#"
        import mymodule from "std:mymodule";
        
        function main(): void {
            const result = mymodule.myFunc1(21);
            assert(result == 42);
        }
    "#;
    assert_execution(source, 0);
}

#[test]
fn test_my_func_2() {
    let source = r#"
        import mymodule from "std:mymodule";
        
        function main(): void {
            const result = mymodule.myFunc2("hello");
            assert(result == "HELLO");
        }
    "#;
    assert_execution(source, 0);
}
```

Add to `crates/raya-runtime/tests/e2e_tests.rs`:

```rust
mod mymodule;
```

### 8. Update Documentation

1. Update [Native IDs](../stdlib/native-ids.md) with your range
2. Add to [Cross-Platform](../stdlib/cross-platform.md) or [POSIX](../stdlib/posix.md)
3. Update root [CLAUDE.md](../../../../CLAUDE.md) if major module
4. Update crate-specific `CLAUDE.md` files

## Name-Based Dispatch (Alternative)

For simpler modules, skip native IDs and use name-based dispatch only:

**mymodule.raya:**
```typescript
export function myFunc(x: int): int {
    return __MODULE_NATIVE_CALL("myFunc", x);
}
```

**Registry:**
```rust
registry.register("mymodule", "myFunc", Box::new(my_func));
```

**Benefits:**
- No native ID allocation
- Simpler dispatch
- More flexible

**Drawbacks:**
- Slightly slower (string lookup)
- Less explicit

## Best Practices

1. **Keep native implementations simple** - Complex logic in Raya
2. **Use NativeContext** for GC allocation
3. **Handle errors gracefully** - Return Error or throw
4. **Test edge cases** - Null, empty, negative, etc.
5. **Document behavior** - Clear docstrings
6. **Update CLAUDE.md** - Keep docs in sync

## Example: Adding std:base64

1. ✅ IDs: 0x7100-0x7101 (encode, decode)
2. ✅ Files: `base64.raya`, `base64.d.raya`
3. ✅ Register: `std_modules.rs`
4. ✅ Implement: `crates/raya-stdlib/src/base64.rs`
5. ✅ Route: `handler.rs` (0x7100-0x7101 range)
6. ✅ Tests: `tests/e2e/base64.rs`
7. ✅ Docs: Update `native-ids.md` and `cross-platform.md`

## Related

- [Workflow](workflow.md) - Development practices
- [Testing](testing.md) - Test infrastructure
- [Native IDs](../stdlib/native-ids.md) - ID allocation
