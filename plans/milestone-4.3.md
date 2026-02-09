# Milestone 4.3: std:math Module

**Status:** Complete
**Depends on:** Milestone 4.2 (std:logger — established stdlib pattern)
**Goal:** Implement the Math module as `std:math` with 20 methods and 2 constants, following the stdlib decoupling pattern (raya-stdlib + raya-runtime)

---

## Overview

Math functions are provided as a standard library module `std:math`, not as a global namespace. Follows the same architecture as `std:logger`:

```
Math.raya (raya-stdlib)  →  __NATIVE_CALL(ID, args)
                                    ↓
                         NativeCall opcode (VM)
                                    ↓
                         is_math_method() check (builtin.rs)
                                    ↓
                         NativeHandler::call() (trait)
                                    ↓
                         StdNativeHandler (raya-runtime)
                                    ↓
                         raya_stdlib::math::* (Rust)
```

### Usage

```typescript
// math is a global (default export from Math.raya, like logger)
let radius: number = 5;
let area: number = math.PI() * math.pow(radius, 2);
let angle: number = math.atan2(3, 4);
let rounded: number = math.floor(3.7);  // 3
```

---

## API Design

### Module: `std:math`

```typescript
// Math.raya — defines class + default export (like Logger.raya)
class Math {
    // Constants (as zero-arg methods, matching __NATIVE_CALL pattern)
    PI(): number;    // 3.141592653589793
    E(): number;     // 2.718281828459045

    // Basic
    abs(x: number): number;
    sign(x: number): number;

    // Rounding
    floor(x: number): number;
    ceil(x: number): number;
    round(x: number): number;
    trunc(x: number): number;

    // Min/Max
    min(a: number, b: number): number;
    max(a: number, b: number): number;

    // Power
    pow(base: number, exp: number): number;
    sqrt(x: number): number;

    // Trigonometry
    sin(x: number): number;
    cos(x: number): number;
    tan(x: number): number;
    asin(x: number): number;
    acos(x: number): number;
    atan(x: number): number;
    atan2(y: number, x: number): number;

    // Exponential/Logarithmic
    exp(x: number): number;
    log(x: number): number;
    log10(x: number): number;

    // Random
    random(): number;
}

const math = new Math();
export default math;
```

---

## Phases

### Phase 1: Native IDs & Engine Infrastructure ✅

**Status:** Complete

**Tasks:**
- [x] Define native IDs in `builtin.rs`
  - [x] Add `pub mod math { ... }` with IDs in 0x2000-0x20FF range
  - [x] Add `is_math_method()` helper (like `is_logger_method()`)
- [x] Add corresponding constants in `native_id.rs`
  - [x] `MATH_ABS` (0x2000)
  - [x] `MATH_SIGN` (0x2001)
  - [x] `MATH_FLOOR` (0x2002)
  - [x] `MATH_CEIL` (0x2003)
  - [x] `MATH_ROUND` (0x2004)
  - [x] `MATH_TRUNC` (0x2005)
  - [x] `MATH_MIN` (0x2006)
  - [x] `MATH_MAX` (0x2007)
  - [x] `MATH_POW` (0x2008)
  - [x] `MATH_SQRT` (0x2009)
  - [x] `MATH_SIN` (0x200A)
  - [x] `MATH_COS` (0x200B)
  - [x] `MATH_TAN` (0x200C)
  - [x] `MATH_ASIN` (0x200D)
  - [x] `MATH_ACOS` (0x200E)
  - [x] `MATH_ATAN` (0x200F)
  - [x] `MATH_ATAN2` (0x2010)
  - [x] `MATH_EXP` (0x2011)
  - [x] `MATH_LOG` (0x2012)
  - [x] `MATH_LOG10` (0x2013)
  - [x] `MATH_RANDOM` (0x2014)
  - [x] `MATH_PI` (0x2015)
  - [x] `MATH_E` (0x2016)
- [x] Add `native_name()` entries for all math IDs

**Files:**
- `crates/raya-engine/src/vm/builtin.rs`
- `crates/raya-engine/src/compiler/native_id.rs`

---

### Phase 2: Raya Source & Stdlib Implementation ✅

**Status:** Complete

**Tasks:**
- [x] Create `crates/raya-stdlib/Math.raya` — class with `__NATIVE_CALL` methods
- [x] Create `crates/raya-stdlib/src/math.rs` — Rust native implementations
  - [x] All 20 methods + 2 constants using `f64` std library
  - [x] `random()` using `rand` crate
- [x] Register `pub mod math;` in `crates/raya-stdlib/src/lib.rs`
- [x] Register `Math.raya` in std module registry (`std_modules.rs`)
- [x] Add `rand` dependency to `crates/raya-stdlib/Cargo.toml`

**Files:**
- `crates/raya-stdlib/Math.raya` (new)
- `crates/raya-stdlib/src/math.rs` (new)
- `crates/raya-stdlib/src/lib.rs`
- `crates/raya-engine/src/compiler/module/std_modules.rs`
- `crates/raya-stdlib/Cargo.toml`

---

### Phase 3: VM Dispatch & Runtime Handler ✅

**Status:** Complete

**Tasks:**
- [x] Add math dispatch in `task_interpreter.rs` (like logger dispatch)
  - [x] `is_math_method(id)` → convert args to string, call handler
  - [x] Handle `NativeCallResult::Number(f64)` → push `Value::f64()` onto stack
  - [x] Handle both NativeCall handler locations (2 sites in task_interpreter.rs)
- [x] Update `StdNativeHandler` in `crates/raya-runtime/src/lib.rs`
  - [x] Add match arms for 0x2000-0x2016 routing to `raya_stdlib::math::*`
  - [x] Parse string args to f64, call math function, return `NativeCallResult::Number(result)`

| Native ID | Method | Rust Implementation | Notes |
|-----------|--------|---------------------|-------|
| 0x2000 | `abs(x)` | `f64::abs()` | |
| 0x2001 | `sign(x)` | `f64::signum()` | Returns -1, 0, or 1 |
| 0x2002 | `floor(x)` | `f64::floor()` | |
| 0x2003 | `ceil(x)` | `f64::ceil()` | |
| 0x2004 | `round(x)` | `f64::round()` | |
| 0x2005 | `trunc(x)` | `f64::trunc()` | |
| 0x2006 | `min(a, b)` | `f64::min(a, b)` | |
| 0x2007 | `max(a, b)` | `f64::max(a, b)` | |
| 0x2008 | `pow(base, exp)` | `f64::powf(exp)` | |
| 0x2009 | `sqrt(x)` | `f64::sqrt()` | |
| 0x200A | `sin(x)` | `f64::sin()` | |
| 0x200B | `cos(x)` | `f64::cos()` | |
| 0x200C | `tan(x)` | `f64::tan()` | |
| 0x200D | `asin(x)` | `f64::asin()` | |
| 0x200E | `acos(x)` | `f64::acos()` | |
| 0x200F | `atan(x)` | `f64::atan()` | |
| 0x2010 | `atan2(y, x)` | `y.atan2(x)` | Note: y.atan2(x) |
| 0x2011 | `exp(x)` | `f64::exp()` | |
| 0x2012 | `log(x)` | `f64::ln()` | Natural logarithm |
| 0x2013 | `log10(x)` | `f64::log10()` | |
| 0x2014 | `random()` | `rand::random::<f64>()` | Returns [0, 1) |
| 0x2015 | `PI()` | `std::f64::consts::PI` | 3.141592653589793 |
| 0x2016 | `E()` | `std::f64::consts::E` | 2.718281828459045 |

**Files:**
- `crates/raya-engine/src/vm/vm/task_interpreter.rs`
- `crates/raya-runtime/src/lib.rs`

---

### Phase 4: Test Harness & E2E Tests ✅

**Status:** Complete

**Tasks:**
- [x] Update test harness `get_std_sources()` to include `Math.raya`
- [x] Add `expect_f64_with_builtins()` helper for floating-point assertions
- [x] Create `crates/raya-runtime/tests/e2e/math.rs` with 44 tests
- [x] Register `mod math;` in `crates/raya-runtime/tests/e2e/mod.rs`
- [x] All tests include `import math from "std:math"` syntax

**Files:**
- `crates/raya-runtime/tests/e2e/harness.rs`
- `crates/raya-runtime/tests/e2e/math.rs` (new)
- `crates/raya-runtime/tests/e2e/mod.rs`

---

## Dependencies

- **rand** crate for `math.random()` (add to raya-stdlib)

---

## Key Files Reference

| File | Purpose |
|------|---------|
| `crates/raya-stdlib/Math.raya` | Raya source: Math class + `__NATIVE_CALL` + `export default` |
| `crates/raya-stdlib/src/math.rs` | Rust implementations (22 functions) |
| `crates/raya-stdlib/src/lib.rs` | Register `pub mod math;` |
| `crates/raya-engine/src/vm/builtin.rs` | `pub mod math` IDs + `is_math_method()` |
| `crates/raya-engine/src/compiler/native_id.rs` | `MATH_*` constants + `native_name()` entries |
| `crates/raya-engine/src/compiler/module/std_modules.rs` | Register `Math.raya` in std module registry |
| `crates/raya-engine/src/vm/vm/task_interpreter.rs` | VM dispatch: `is_math_method()` → native handler |
| `crates/raya-runtime/src/lib.rs` | `StdNativeHandler` match arms for math IDs |
| `crates/raya-runtime/tests/e2e/harness.rs` | `get_std_sources()` includes `Math.raya` |
| `crates/raya-runtime/tests/e2e/math.rs` | E2E tests |
| `design/STDLIB.md` | API specification |
