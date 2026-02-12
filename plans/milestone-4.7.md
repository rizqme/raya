# Milestone 4.7: std:time Module

**Status:** Complete
**Depends on:** Milestone 4.2 (stdlib pattern)
**Goal:** Implement time primitives as `std:time` with monotonic/wall clocks, sleep, and duration utilities

---

## Overview

Time functions are provided as a standard library module `std:time`. Uses engine-side handler for system clock access:

```
Time.raya (raya-stdlib)  →  __NATIVE_CALL(ID, args)  [5 native IDs]
                                      ↓
                          + pure Raya methods           [7 methods]
                                      ↓
                           NativeCall opcode (VM)
                                      ↓
                           is_time_method() check (builtin.rs)
                                      ↓
                           call_time_method() (handlers/time.rs)
                                      ↓
                           std::time::Instant / SystemTime / thread::sleep
```

**Design:** Only 5 native calls for actual system operations. Duration conversions are pure Raya (no FFI overhead). The monotonic clock uses a process-level `Instant` epoch for stable measurements.

**Relationship to Date:** The `Date` builtin handles calendar manipulation (year/month/day getters, setters, formatting). `std:time` handles clocks, measurement, sleep, and duration math — no overlap.

### Usage

```typescript
import time from "std:time";

// ── Wall Clock ──
let timestamp: number = time.now();           // ms since Unix epoch

// ── Monotonic Clock (for benchmarking/measurement) ──
let start: number = time.monotonic();         // monotonic ms
// ... some work ...
let elapsed: number = time.elapsed(start);    // ms since start
let precise: number = time.hrtime();          // nanoseconds (monotonic)

// ── Sleep ──
time.sleep(1000);                             // sleep 1 second
time.sleepMicros(500);                        // sleep 500μs

// ── Duration Utilities (pure Raya) ──
let timeout: number = time.seconds(30);       // 30_000 ms
let interval: number = time.minutes(5);       // 300_000 ms
let limit: number = time.hours(1);            // 3_600_000 ms

let secs: number = time.toSeconds(elapsed);   // ms → seconds
let mins: number = time.toMinutes(elapsed);   // ms → minutes
let hrs: number = time.toHours(elapsed);      // ms → hours

// ── Practical patterns ──
let t0: number = time.monotonic();
time.sleep(time.seconds(2));
let dur: number = time.elapsed(t0);
// dur ≈ 2000
```

---

## API Design

### Module: `std:time`

```typescript
class Time {
    // ── Wall Clock ──
    /** Milliseconds since Unix epoch (Jan 1 1970 00:00:00 UTC). */
    now(): number;

    // ── Monotonic Clock ──
    /** Monotonic timestamp in milliseconds. Not affected by system clock changes.
     *  Use for measuring durations and benchmarking. */
    monotonic(): number;

    /** High-resolution monotonic timestamp in nanoseconds.
     *  Use for sub-millisecond precision timing. */
    hrtime(): number;

    /** Milliseconds elapsed since a monotonic start point.
     *  Equivalent to `time.monotonic() - start`. */
    elapsed(start: number): number;

    // ── Sleep ──
    /** Sleep for the given number of milliseconds. Blocks the current thread. */
    sleep(ms: number): void;

    /** Sleep for the given number of microseconds. Blocks the current thread. */
    sleepMicros(us: number): void;

    // ── Duration Conversion (pure Raya, no native calls) ──
    /** Convert seconds to milliseconds. */
    seconds(n: number): number;    // n * 1000

    /** Convert minutes to milliseconds. */
    minutes(n: number): number;    // n * 60_000

    /** Convert hours to milliseconds. */
    hours(n: number): number;      // n * 3_600_000

    /** Convert milliseconds to seconds. */
    toSeconds(ms: number): number; // ms / 1000

    /** Convert milliseconds to minutes. */
    toMinutes(ms: number): number; // ms / 60_000

    /** Convert milliseconds to hours. */
    toHours(ms: number): number;   // ms / 3_600_000
}

const time = new Time();
export default time;
```

---

## Native IDs

Range: `0x5000-0x50FF`

Only 5 native calls — duration utilities are pure Raya.

| ID | Constant | Method | Args | Return |
|----|----------|--------|------|--------|
| 0x5000 | `TIME_NOW` | `now()` | — | number (ms) |
| 0x5001 | `TIME_MONOTONIC` | `monotonic()` | — | number (ms) |
| 0x5002 | `TIME_HRTIME` | `hrtime()` | — | number (ns) |
| 0x5003 | `TIME_SLEEP` | `sleep(ms)` | number | void |
| 0x5004 | `TIME_SLEEP_MICROS` | `sleepMicros(us)` | number | void |

---

## Rust Implementation

```rust
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::sync::LazyLock;

/// Process-start monotonic reference point
static MONOTONIC_EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

fn time_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as f64
}

fn time_monotonic() -> f64 {
    Instant::now()
        .duration_since(*MONOTONIC_EPOCH)
        .as_millis() as f64
}

fn time_hrtime() -> f64 {
    Instant::now()
        .duration_since(*MONOTONIC_EPOCH)
        .as_nanos() as f64
}

fn time_sleep(ms: f64) {
    std::thread::sleep(Duration::from_millis(ms as u64));
}

fn time_sleep_micros(us: f64) {
    std::thread::sleep(Duration::from_micros(us as u64));
}
```

---

## Phases

### Phase 1: Native IDs & Engine Infrastructure

**Status:** Complete

**Tasks:**
- [x] Define native IDs in `builtin.rs`
  - [x] Add `pub mod time { ... }` with IDs 0x5000-0x5004
  - [x] Add `is_time_method()` helper
- [x] Add corresponding constants in `native_id.rs`
  - [x] `TIME_NOW` (0x5000)
  - [x] `TIME_MONOTONIC` (0x5001)
  - [x] `TIME_HRTIME` (0x5002)
  - [x] `TIME_SLEEP` (0x5003)
  - [x] `TIME_SLEEP_MICROS` (0x5004)
- [x] Add `native_name()` entries for all time IDs

**Files:**
- `crates/raya-engine/src/vm/builtin.rs`
- `crates/raya-engine/src/compiler/native_id.rs`

---

### Phase 2: Raya Source & Engine Handler

**Status:** Complete

**Tasks:**
- [x] Create `crates/raya-stdlib/raya/Time.raya`
  - [x] 5 native methods: now, monotonic, hrtime, sleep, sleepMicros
  - [x] 7 pure Raya methods: elapsed, seconds, minutes, hours, toSeconds, toMinutes, toHours
  - [x] `export default time;`
- [x] Create `crates/raya-stdlib/raya/time.d.raya` — type declarations
- [x] Register `Time.raya` in std module registry (`std_modules.rs`)
- [x] Create `crates/raya-engine/src/vm/vm/handlers/time.rs`
  - [x] `call_time_method()` with 5 match arms (no GC context needed — only returns numbers/null)
  - [x] `MONOTONIC_EPOCH` static LazyLock<Instant>
- [x] Register in `handlers/mod.rs`
- [x] Add dispatch in `task_interpreter.rs` at both native call sites
- [x] Add `call_time_method` bridge to TaskInterpreter impl

**Files:**
- `crates/raya-stdlib/raya/Time.raya` (new)
- `crates/raya-stdlib/raya/time.d.raya` (new)
- `crates/raya-engine/src/compiler/module/std_modules.rs`
- `crates/raya-engine/src/vm/vm/handlers/time.rs` (new)
- `crates/raya-engine/src/vm/vm/handlers/mod.rs`
- `crates/raya-engine/src/vm/vm/task_interpreter.rs`

---

### Phase 3: E2E Tests

**Status:** Complete

**Tasks:**
- [x] Update test harness `get_std_sources()` to include `Time.raya`
- [x] Create `crates/raya-runtime/tests/e2e/time.rs` — 19 tests
- [x] Register `mod time;` in `crates/raya-runtime/tests/e2e/mod.rs`

**Test Plan:**

| Test | What it verifies |
|------|-----------------|
| `test_time_import` | `import time from "std:time"` compiles and runs |
| `test_time_now_returns_number` | `time.now()` returns a positive number |
| `test_time_now_is_recent` | `time.now()` returns a value > 1_700_000_000_000 (post-2023) |
| `test_time_monotonic_positive` | `time.monotonic()` returns >= 0 |
| `test_time_monotonic_increases` | Two calls return increasing values |
| `test_time_hrtime_positive` | `time.hrtime()` returns >= 0 |
| `test_time_hrtime_nanosecond_scale` | `time.hrtime()` > `time.monotonic() * 1000` (roughly) |
| `test_time_elapsed` | `time.elapsed(start)` returns a non-negative value |
| `test_time_sleep` | `time.sleep(10)` — elapsed time >= 10ms |
| `test_time_sleep_micros` | `time.sleepMicros(1000)` — elapsed time >= ~1ms |
| `test_time_seconds` | `time.seconds(3)` == 3000 |
| `test_time_minutes` | `time.minutes(2)` == 120000 |
| `test_time_hours` | `time.hours(1)` == 3600000 |
| `test_time_to_seconds` | `time.toSeconds(5000)` == 5 |
| `test_time_to_minutes` | `time.toMinutes(120000)` == 2 |
| `test_time_to_hours` | `time.toHours(7200000)` == 2 |
| `test_time_roundtrip` | `time.toSeconds(time.seconds(42))` == 42 |
| `test_time_measure_sleep` | monotonic before/after sleep(50), elapsed ≈ 50ms |

**Files:**
- `crates/raya-runtime/tests/e2e/harness.rs`
- `crates/raya-runtime/tests/e2e/time.rs` (new)
- `crates/raya-runtime/tests/e2e/mod.rs`

---

## Key Files Reference

| File | Purpose |
|------|---------|
| `crates/raya-stdlib/raya/Time.raya` | Raya source: Time class (5 native + 7 pure Raya) + `export default` |
| `crates/raya-stdlib/raya/time.d.raya` | Type declarations for IDE/tooling |
| `crates/raya-engine/src/vm/builtin.rs` | `pub mod time` IDs + `is_time_method()` |
| `crates/raya-engine/src/compiler/native_id.rs` | `TIME_*` constants + `native_name()` entries |
| `crates/raya-engine/src/compiler/module/std_modules.rs` | Register `Time.raya` in std module registry |
| `crates/raya-engine/src/vm/vm/handlers/time.rs` | Engine-side handler with 5 methods + MONOTONIC_EPOCH |
| `crates/raya-engine/src/vm/vm/handlers/mod.rs` | Register + re-export time handler |
| `crates/raya-engine/src/vm/vm/task_interpreter.rs` | VM dispatch at both native call sites |
| `crates/raya-runtime/tests/e2e/harness.rs` | `get_std_sources()` includes `Time.raya` |
| `crates/raya-runtime/tests/e2e/time.rs` | E2E tests |

---

## Future Work (Deferred)

- **Async sleep** — `time.delay(ms): Task<void>` that suspends the current Task instead of blocking the OS thread. Requires VM scheduler integration (timer wheel or similar).
- **Timers/Intervals** — `time.setTimeout(fn, ms)`, `time.setInterval(fn, ms)`, `time.clearTimeout(id)`. Requires closure-as-callback support in native calls + scheduler timer queue.
- **Formatting** — `time.format(ms, pattern)` for timestamp formatting. Currently handled by `Date.toISOString()` and `Date.toString()` builtins.
- **Timezone support** — `time.utcOffset()`, timezone-aware formatting. Requires `chrono` or `iana-time-zone` crate.
