# Milestone 4.3: std:math Module

**Status:** Not Started
**Depends on:** Milestone 3.7 (Module System)
**Goal:** Implement the Math module as `std:math` with 20 methods and 2 constants, imported explicitly

---

## Overview

Math functions are provided as a standard library module `std:math`, not as a global namespace. Users must import it explicitly.

### Usage

```typescript
import math from "std:math";

let radius = 5.0;
let area = math.PI * math.pow(radius, 2);
let angle = math.atan2(3, 4);
let rounded = math.floor(3.7);  // 3
```

---

## API Design

### Module: `std:math`

```typescript
module "std:math" {
  interface Math {
    // Constants
    readonly PI: number;   // 3.141592653589793
    readonly E: number;    // 2.718281828459045

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

  const math: Math;
  export default math;
}
```

---

## Phases

### Phase 1: Native IDs & Type Registration ⬜

**Status:** Not Started

**Tasks:**
- [ ] Define native IDs in `builtin.rs`
  - [ ] Add `pub mod math { ... }` with IDs 0x0F10-0x0F2F
- [ ] Add corresponding constants in `native_id.rs`
  - [ ] `MATH_ABS` (0x0F10)
  - [ ] `MATH_SIGN` (0x0F11)
  - [ ] `MATH_FLOOR` (0x0F12)
  - [ ] `MATH_CEIL` (0x0F13)
  - [ ] `MATH_ROUND` (0x0F14)
  - [ ] `MATH_TRUNC` (0x0F15)
  - [ ] `MATH_MIN` (0x0F16)
  - [ ] `MATH_MAX` (0x0F17)
  - [ ] `MATH_POW` (0x0F18)
  - [ ] `MATH_SQRT` (0x0F19)
  - [ ] `MATH_SIN` (0x0F1A)
  - [ ] `MATH_COS` (0x0F1B)
  - [ ] `MATH_TAN` (0x0F1C)
  - [ ] `MATH_ASIN` (0x0F1D)
  - [ ] `MATH_ACOS` (0x0F1E)
  - [ ] `MATH_ATAN` (0x0F1F)
  - [ ] `MATH_ATAN2` (0x0F20)
  - [ ] `MATH_EXP` (0x0F21)
  - [ ] `MATH_LOG` (0x0F22)
  - [ ] `MATH_LOG10` (0x0F23)
  - [ ] `MATH_RANDOM` (0x0F24)
- [ ] Register math as default-exported singleton in type checker
  - [ ] Define `Math` interface with all method signatures
  - [ ] `PI` and `E` as readonly number properties

**Files:**
- `crates/raya-engine/src/vm/builtin.rs`
- `crates/raya-engine/src/compiler/native_id.rs`
- `crates/raya-engine/src/parser/checker/checker.rs`
- `crates/raya-engine/src/parser/checker/builtins.rs`

---

### Phase 2: Compiler Lowering ⬜

**Status:** Not Started

**Tasks:**
- [ ] Lower `math.PI` → `PUSH_F64 3.141592653589793` (constant fold)
- [ ] Lower `math.E` → `PUSH_F64 2.718281828459045` (constant fold)
- [ ] Lower `math.abs(x)` → `NATIVE_CALL(MATH_ABS, x)`
- [ ] Same pattern for all 20 methods
- [ ] Handle `math.min(a, b)` / `math.max(a, b)` — variadic → pairwise in compiler

**Files:**
- `crates/raya-engine/src/compiler/lower/expr.rs`

---

### Phase 3: VM Handlers ⬜

**Status:** Not Started

All handlers take f64 args, return f64.

**Tasks:**
- [ ] Implement 20 match arms in `task_interpreter.rs`

| Native ID | Method | Rust Implementation | Notes |
|-----------|--------|---------------------|-------|
| 0x0F10 | `abs(x)` | `f64::abs()` | |
| 0x0F11 | `sign(x)` | `f64::signum()` | Returns -1, 0, or 1 |
| 0x0F12 | `floor(x)` | `f64::floor()` | |
| 0x0F13 | `ceil(x)` | `f64::ceil()` | |
| 0x0F14 | `round(x)` | `f64::round()` | |
| 0x0F15 | `trunc(x)` | `f64::trunc()` | |
| 0x0F16 | `min(a, b)` | `f64::min(a, b)` | |
| 0x0F17 | `max(a, b)` | `f64::max(a, b)` | |
| 0x0F18 | `pow(base, exp)` | `f64::powf(exp)` | |
| 0x0F19 | `sqrt(x)` | `f64::sqrt()` | |
| 0x0F1A | `sin(x)` | `f64::sin()` | |
| 0x0F1B | `cos(x)` | `f64::cos()` | |
| 0x0F1C | `tan(x)` | `f64::tan()` | |
| 0x0F1D | `asin(x)` | `f64::asin()` | |
| 0x0F1E | `acos(x)` | `f64::acos()` | |
| 0x0F1F | `atan(x)` | `f64::atan()` | |
| 0x0F20 | `atan2(y, x)` | `f64::atan2(x)` | Note: y.atan2(x) |
| 0x0F21 | `exp(x)` | `f64::exp()` | |
| 0x0F22 | `log(x)` | `f64::ln()` | Natural logarithm |
| 0x0F23 | `log10(x)` | `f64::log10()` | |
| 0x0F24 | `random()` | `rand::thread_rng().gen::<f64>()` | Returns [0, 1) |

**Files:**
- `crates/raya-engine/src/vm/vm/task_interpreter.rs`

---

### Phase 4: Tests ⬜

**Status:** Not Started

**Tasks:**
- [ ] `test_math_constants` — `math.PI`, `math.E`
- [ ] `test_math_abs_sign` — `math.abs(-5)`, `math.sign(-3)`
- [ ] `test_math_floor_ceil_round_trunc` — Rounding functions
- [ ] `test_math_min_max` — `math.min(1, 2)`, `math.max(1, 2)`
- [ ] `test_math_pow_sqrt` — `math.pow(2, 10)`, `math.sqrt(16)`
- [ ] `test_math_trig` — sin, cos, tan, asin, acos, atan, atan2
- [ ] `test_math_exp_log` — `math.exp(1)`, `math.log(math.E)`, `math.log10(100)`
- [ ] `test_math_random` — Returns value in [0, 1)
- [ ] `test_math_import` — Must import from "std:math" (no global)

**Files:**
- `crates/raya-engine/tests/e2e/math.rs`

---

## Dependencies

- **rand** crate for `math.random()`

---

## Key Files Reference

| File | Purpose |
|------|---------|
| `crates/raya-engine/src/vm/builtin.rs` | Math native ID module |
| `crates/raya-engine/src/compiler/native_id.rs` | Math native ID constants |
| `crates/raya-engine/src/parser/checker/checker.rs` | Math type resolution |
| `crates/raya-engine/src/compiler/lower/expr.rs` | math → NATIVE_CALL lowering |
| `crates/raya-engine/src/vm/vm/task_interpreter.rs` | Math VM handlers (20 arms) |
| `design/STDLIB.md` | API specification |
