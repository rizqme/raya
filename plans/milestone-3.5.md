# Milestone 3.5: Built-in Types & Native Call System

**Goal:** Implement built-in types using hardcoded compiler support for primitives (string, Array, number) and normal Raya class definitions for classes that wrap opcodes and native calls.

**Dependencies:** Milestone 3.4 (Async/Await & Concurrency)

---

## Overview

### Type Classification

**Primitives** (hardcoded in compiler, cannot be extended):
- `number`, `boolean`, `null` - Basic values
- `string` - Immutable, interned strings with built-in methods
- `Array<T>` - Generic array with built-in methods
- `RegExp` - Immutable, stateless regex pattern with built-in methods

**Classes** (defined in stdlib, can be extended, inherit from Object):
- `Object`, `Mutex`, `Task<T>`, `Channel<T>`, `Error`, `Buffer`, `Map<K,V>`, `Set<T>`, `Date`, `RegExpMatch`

### Implementation Approaches

**1. Primitive Methods (hardcoded in compiler):**
```typescript
// The compiler has built-in knowledge of primitive methods
// No syntax for defining them - they're hardcoded

// User code:
let s = "hello";
s.toUpperCase();  // Compiler knows this is NATIVE_CALL(0x0210)
s.length;         // Compiler knows this is SLEN opcode

let arr = [1, 2, 3];
arr.push(4);      // Compiler knows this is ARRAY_PUSH opcode
arr.length;       // Compiler knows this is ARRAY_LEN opcode

let re = /hello/i;    // RegExp literal (or new RegExp("hello", "i"))
re.test("Hello");     // Compiler knows this is NATIVE_CALL(0x0A01)
```

**2. Class Methods (normal class definitions in stdlib):**
```typescript
// stdlib/Mutex.raya
class Mutex extends Object {
    private __mutexId: number;

    lock(): void {
        __OPCODE_MUTEX_LOCK(this.__mutexId);
    }
}
```

### Key Design Principles

**1. Primitive methods are hardcoded in the compiler.**
- Compiler recognizes method calls on `string`, `Array<T>`, `number`
- Emits NATIVE_CALL or opcodes directly
- No stdlib files for primitive types
- Cannot be extended or modified by users

**2. Classes are normal Raya class definitions.** This means:
- Built-in classes extend `Object` like any other class
- Users can extend built-in classes normally
- `instanceof` and `as` work correctly with inheritance
- The class definitions are readable and debuggable

**3. Zero overhead via mandatory inlining.** Methods containing only intrinsic calls are inlined at the call site:
```
// Source code:
mutex.lock();

// BAD (without inlining) - 2 calls:
CALL Mutex.lock         // Function call overhead
  LOAD this.__mutexId   // Load field
  MUTEX_LOCK            // Opcode
  RETURN

// GOOD (with inlining) - direct:
LOAD mutex.__mutexId    // Inlined: load field
MUTEX_LOCK              // Inlined: direct opcode
```
This ensures `mutex.lock()` has the same performance as a raw opcode.

---

## Implementation Tasks

### Phase 1: Compiler Intrinsics

#### 1.0 Mandatory Inlining (CRITICAL)
- [x] Detect methods that only contain intrinsic calls (1-2 instructions, single block)
- [x] Inline small functions automatically at call sites
- [x] **Result:** `mutex.lock()` emits `MUTEX_LOCK` directly, NOT `CALL Mutex.lock`

```
// Source:
mutex.lock();

// Compiled (with inlining):
LOAD_FIELD mutex.__mutexId   // Inlined from method body
MUTEX_LOCK                   // Direct opcode, no CALL overhead
```

#### 1.1 Opcode Intrinsics
- [x] Add `__OPCODE_*` intrinsic recognition in parser/binder
- [x] Emit corresponding opcode directly (no function call overhead)
- [x] Support intrinsics:
  - [x] `__OPCODE_ARRAY_LEN(ptr)` → `ARRAY_LEN`
  - [x] `__OPCODE_ARRAY_PUSH(ptr, elem)` → `ARRAY_PUSH`
  - [x] `__OPCODE_ARRAY_POP(ptr)` → `ARRAY_POP`
  - [x] `__OPCODE_MUTEX_NEW()` → `MUTEX_NEW`
  - [x] `__OPCODE_MUTEX_LOCK(id)` → `MUTEX_LOCK`
  - [x] `__OPCODE_MUTEX_UNLOCK(id)` → `MUTEX_UNLOCK`
  - [x] `__OPCODE_CHANNEL_NEW(cap)` → `CHANNEL_NEW`
  - [x] `__OPCODE_TASK_CANCEL(id)` → `TASK_CANCEL`
  - [x] `__OPCODE_YIELD()` → `YIELD`
  - [x] `__OPCODE_SLEEP(ms)` → `SLEEP`

#### 1.2 Native Call Intrinsic
- [x] Add `__NATIVE_CALL(nativeId, args...)` intrinsic
- [x] Emit `NATIVE_CALL` opcode with native function ID
- [x] Define native function ID constants (0x0001, 0x0100, etc.) in `native_id.rs`

#### 1.3 Native Function ID Registry
- [x] Create native ID constants in `crates/raya-compiler/src/native_id.rs`
- [x] Group by class: Object (0x00xx), Array (0x01xx), String (0x02xx), etc.
- [x] Add `native_name()` helper for debugging

---

### Phase 2: Type Operators

#### 2.1 instanceof Operator
- [x] Add `INSTANCEOF` opcode (0xEE)
- [x] Parser support for `expr instanceof ClassName`
- [x] Type checker: validate class exists, returns boolean
- [x] IR instruction: `InstanceOf { dest, obj, class_id }`
- [x] IR lowering: `Expression::InstanceOf` → `IrInstr::InstanceOf`
- [x] Codegen: emit `INSTANCEOF`
- [x] VM interpreter: check object's class against target (with inheritance)

#### 2.2 as (Cast) Operator
- [x] Add `CAST` opcode (0xEF)
- [x] Parser support for `expr as TargetType`
- [x] Type checker: resolve target type
- [x] IR instruction: `Cast { dest, obj, class_id }`
- [x] IR lowering: `Expression::TypeCast` → `IrInstr::Cast`
- [x] Codegen: emit `CAST`
- [x] VM interpreter: validate cast, throw TypeError if invalid

---

### Phase 3: Object Base Class

#### 3.1 Object Class Definition
- [x] Create `stdlib/Object.raya`
- [x] `toString()` method (returns "[object Object]")
- [x] Internal `object_id` field (assigned by VM on allocation)
- [x] Additional methods: `hashCode()`, `equals()`

#### 3.2 VM Object Header
- [x] Add `class_id` to object header (already existed)
- [x] Add `object_id` generation on `Object::new()`
- [x] Implement native functions:
  - `__NATIVE_OBJECT_HASH_CODE` (0x0002)
  - `__NATIVE_OBJECT_EQUAL` (0x0003)
  - Note: `toString()` uses pure Raya code, no native call needed

---

### Phase 4: Hardcoded Primitive Methods

The compiler has built-in knowledge of primitive type methods. No new syntax is needed - the type checker and lowering phases recognize method calls on primitives and emit the appropriate opcodes or native calls.

#### 4.1 Type Checker Support
- [x] Recognize method calls on `string` type
- [x] Recognize method calls on `Array<T>` type
- [x] Recognize method calls on `RegExp` type
- [x] Recognize method calls on `number` type (toFixed, toPrecision, toString)
- [x] Return correct type signatures for built-in methods
- [x] Report errors for unknown methods on primitives (returns unknown type)

#### 4.2 String Methods (Hardcoded)
- [x] Register string methods in type checker with signatures
- [x] Opcode-backed: `length` (property via SLEN)
- [x] Native-backed methods:
  - `charAt(index: number): string` → 0x0200
  - `charCodeAt(index: number): number` → 0x020E
  - `substring(start: number, end?: number): string` → 0x0201
  - `indexOf(search: string, position?: number): number` → 0x0205
  - `lastIndexOf(search: string, position?: number): number` → 0x020F
  - `toUpperCase(): string` → 0x0202
  - `toLowerCase(): string` → 0x0203
  - `trim(): string` → 0x0204
  - `trimStart(): string` → 0x0210
  - `trimEnd(): string` → 0x0211
  - `split(separator: string): Array<string>` → 0x0207
  - `repeat(count: number): string` → 0x020B
  - `startsWith(prefix: string): boolean` → 0x0208
  - `endsWith(suffix: string): boolean` → 0x0209
  - `includes(search: string): boolean` → 0x0206
  - `replace(search: string, replacement: string): string` → 0x020A
  - `padStart(length: number, pad?: string): string` → 0x020C
  - `padEnd(length: number, pad?: string): string` → 0x020D
- [x] Implement `__NATIVE_STRING_*` functions in VM (0x0200-0x0211)

#### 4.3 Array Methods (Hardcoded)
- [x] Register array methods in type checker with generic signatures
- [x] Opcode-backed:
  - `length` (property) → ARRAY_LEN opcode
  - `push(element: T): void` → ARRAY_PUSH opcode
  - `pop(): T | null` → ARRAY_POP opcode
- [x] Native-backed methods:
  - `unshift(element: T): void` → NATIVE_CALL(0x0103)
  - `shift(): T | null` → NATIVE_CALL(0x0102)
  - `slice(start?: number, end?: number): Array<T>` → NATIVE_CALL(0x0106)
  - `concat(other: Array<T>): Array<T>` → NATIVE_CALL(0x0107)
  - `indexOf(element: T): number` → NATIVE_CALL(0x0104)
  - `lastIndexOf(element: T): number` → NATIVE_CALL(0x0110)
  - `includes(element: T): boolean` → NATIVE_CALL(0x0105)
  - `reverse(): Array<T>` → NATIVE_CALL(0x0108)
  - `sort(compareFn?: (a: T, b: T) => number): Array<T>` → NATIVE_CALL(0x0111)
  - `join(separator?: string): string` → NATIVE_CALL(0x0109)
  - `map<U>(fn: (element: T, index: number) => U): Array<U>` → NATIVE_CALL(0x0112)
  - `filter(predicate: (element: T, index: number) => boolean): Array<T>` → NATIVE_CALL(0x010B)
  - `reduce<U>(fn: (acc: U, element: T, index: number) => U, initial: U): U` → NATIVE_CALL(0x0113)
  - `forEach(fn: (element: T, index: number) => void): void` → NATIVE_CALL(0x010A)
  - `find(predicate: (element: T, index: number) => boolean): T | null` → NATIVE_CALL(0x010C)
  - `findIndex(predicate: (element: T, index: number) => boolean): number` → NATIVE_CALL(0x010D)
  - `every(predicate: (element: T, index: number) => boolean): boolean` → NATIVE_CALL(0x010E)
  - `some(predicate: (element: T, index: number) => boolean): boolean` → NATIVE_CALL(0x010F)
  - `fill(value: T, start?: number, end?: number): Array<T>` → NATIVE_CALL(0x0114)
  - `flat<U>(depth?: number): Array<U>` → NATIVE_CALL(0x0115)
- [x] Implement all `__NATIVE_ARRAY_*` functions in VM (0x0100-0x0115)
- [x] Handle callbacks (map, filter, etc.) via VM callback mechanism

#### 4.4 RegExp Methods (Hardcoded)
- [x] Register RegExp methods in type checker with signatures
- [x] Properties:
  - `source: string` - The pattern string
  - `flags: string` - The flags string
  - `global: boolean` - Whether 'g' flag is set
  - `ignoreCase: boolean` - Whether 'i' flag is set
  - `multiline: boolean` - Whether 'm' flag is set
  - `dotAll: boolean` - Whether 's' flag is set
  - `unicode: boolean` - Whether 'u' flag is set
- [x] Native-backed methods:
  - `test(str: string): boolean` → NATIVE_CALL(0x0A01)
  - `exec(str: string): RegExpMatch | null` → NATIVE_CALL(0x0A02)
  - `execAll(str: string): Array<RegExpMatch>` → NATIVE_CALL(0x0A03)
  - `replace(str: string, replacement: string): string` → NATIVE_CALL(0x0A04)
  - `replaceWith(str: string, replacer: (RegExpMatch) => string): string` → NATIVE_CALL(0x0A05) (not yet implemented - requires callback)
  - `split(str: string, limit: number): Array<string>` → NATIVE_CALL(0x0A06)
- [x] Implement all `__NATIVE_REGEXP_*` functions in VM (0x0A00-0x0A06)
- [x] Use Rust regex crate for pattern matching
- [x] **Stateless design**: No `lastIndex`, each call starts from beginning

#### 4.5 String Methods with RegExp Overloads
- [x] Add overloaded string methods that accept RegExp:
  - [x] `match(pattern: RegExp): Array<string> | null` → NATIVE_CALL(0x0212)
  - [x] `matchAll(pattern: RegExp): Array<Array<string | number>>` → NATIVE_CALL(0x0213)
  - [x] `replace(pattern: RegExp, replacement: string): string` → NATIVE_CALL(0x0215)
  - [x] `replaceWith(pattern: RegExp, replacer: (match: Array) => string): string` → NATIVE_CALL(0x0217)
  - [x] `split(pattern: RegExp, limit: number): Array<string>` → NATIVE_CALL(0x0216)
  - [x] `search(pattern: RegExp): number` → NATIVE_CALL(0x0214)
- [x] Compiler selects RegExp overload when argument is of type `RegExp`
- [x] Type checker accepts `string | RegExp` union for replace/split first param

#### 4.6 IR Lowering for Primitive Methods
- [x] In `lower_call()`, detect method calls on primitive types
- [x] For opcode-backed methods, emit appropriate IR instruction
- [x] For native-backed methods, emit `NativeCall` IR instruction
- [x] Handle generic type parameters (Array<T>.map<U>)

---

### Phase 5: Mutex Class

#### 5.1 Mutex as Normal Class
- [x] Create `stdlib/Mutex.raya` (simplified: lock/unlock only, no generics)
- [x] Constructor calls `__OPCODE_MUTEX_NEW()` → stores `handle`
- [x] `lock()` calls `__OPCODE_MUTEX_LOCK(this.handle)`
- [x] `unlock()` calls `__OPCODE_MUTEX_UNLOCK(this.handle)`
- [ ] Native: `tryLock()`, `isLocked()` (not implemented)

#### 5.2 VM Native Functions
- [ ] Implement `__NATIVE_MUTEX_TRY_LOCK` (0x0200)
- [ ] Implement `__NATIVE_MUTEX_IS_LOCKED` (0x0201)

---

### Phase 6: Task Class

#### 6.1 Task as Normal Class
- [x] Create `stdlib/Task.raya`
- [x] Generic `Task<T>` with private constructor (runtime creates via SPAWN)
- [x] Internal `handle` field (set by SPAWN)
- [x] `cancel()` calls `__OPCODE_TASK_CANCEL(this.handle)`
- [x] Standalone functions: `taskYield()` → `__OPCODE_YIELD()`, `taskSleep(ms)` → `__OPCODE_SLEEP(ms)`
- [ ] Native: `isDone()`, `isCancelled()` (not implemented)

#### 6.2 VM Native Functions
- [ ] Implement `__NATIVE_TASK_IS_DONE` (0x0300)
- [ ] Implement `__NATIVE_TASK_IS_CANCELLED` (0x0301)

---

### Phase 7: Channel Class

#### 7.1 Channel as Normal Class
- [x] Create `stdlib/Channel.raya`
- [x] Generic `Channel<T>`
- [x] Constructor uses `__NATIVE_CALL(0x0400, capacity)` → stores `channelId`
- [x] All methods use `__NATIVE_CALL` (0x0401-0x0408)

#### 7.2 VM Native Functions
- [ ] Implement all `__NATIVE_CHANNEL_*` functions (0x0400-0x0408)
- [ ] `send()` and `receive()` must integrate with task scheduler

---

### Phase 8: Error Classes

#### 8.1 Error Class Hierarchy
- [x] Create `stdlib/Error.raya`
- [x] Error classes: `Error`, `TypeError`, `RangeError`, `ReferenceError`, `SyntaxError`, `ChannelClosedError`, `AssertionError`
- [x] All error classes defined in single file with inheritance

#### 8.2 Stack Trace Support
- [ ] Implement `__NATIVE_ERROR_STACK` (0x0500)
- [ ] Capture stack trace on throw

---

### Phase 9: Collection Classes

#### 9.1 Map Class
- [x] Create `stdlib/Map.raya`
- [x] Generic `Map<K, V>`
- [x] All methods use `__NATIVE_CALL` (0x0800-0x080A)
- [ ] Implement `__NATIVE_MAP_*` functions in VM

#### 9.2 Set Class
- [x] Create `stdlib/Set.raya`
- [x] Generic `Set<T>` with set operations (`union`, `intersection`, `difference`)
- [x] All methods use `__NATIVE_CALL` (0x0900-0x090A)
- [ ] Implement `__NATIVE_SET_*` functions in VM

---

### Phase 10: Buffer & Date Classes

#### 10.1 Buffer Class
- [x] Create `stdlib/Buffer.raya`
- [x] All methods use `__NATIVE_CALL`
- [ ] Implement `__NATIVE_BUFFER_*` functions in VM

#### 10.2 Date Class
- [x] Create `stdlib/Date.raya`
- [x] All methods use `__NATIVE_CALL`
- [ ] Implement `__NATIVE_DATE_*` functions in VM

---

### Phase 11: RegExpMatch Class

**Note:** `RegExp` itself is a primitive type (see Phase 4.4). Only `RegExpMatch` (the result type) is a class.

#### 11.1 RegExpMatch Class
- [x] Create `stdlib/RegExpMatch.raya`
- [x] Fields: `match`, `index`, `input`, `groups`
- [x] Methods:
  - `group(index: number): string | null` - Get captured group by index
  - `groupCount(): number` - Get number of captured groups
  - `end(): number` - Get end position of match
  - `toString(): string` - Returns the matched text
- [x] Used as return type for `RegExp.exec()` and string `match()` methods

---

## File Changes

### New Files

```
crates/raya-engine/builtins/    # ✅ All created (consolidated from raya-builtins)
├── Object.raya           # Base class for all classes
├── Mutex.raya            # Simplified: lock/unlock only
├── Task.raya
├── Channel.raya
├── Error.raya            # Includes TypeError, RangeError, ReferenceError
├── Buffer.raya
├── Map.raya
├── Set.raya
├── Date.raya
└── RegExpMatch.raya      # Result type for RegExp.exec()

crates/raya-engine/src/compiler/
├── native_id.rs          # ✅ Native function ID constants

crates/raya-engine/src/vm/
├── builtin.rs            # ✅ Native call dispatch (NATIVE_CALL handler)
```

Note: No stdlib files for `string`, `array`, or `RegExp` - these are hardcoded primitives in the compiler. Only `RegExpMatch` (result class) needs a stdlib file.

**Removed:** `RefCell.raya` - not exposed as public API

### Modified Files

Note: Crates have been consolidated into `raya-engine`.

| File | Changes |
|------|---------|
| `crates/raya-engine/src/compiler/bytecode/opcode.rs` | Add `INSTANCEOF`, `CAST`, `NATIVE_CALL` opcodes (✅ done) |
| `crates/raya-engine/src/parser/parser/expr.rs` | Parse `instanceof`, `as`, intrinsics (✅ done) |
| `crates/raya-engine/src/parser/checker/checker.rs` | Type check `instanceof`, `as`, intrinsics, **hardcoded primitive methods** (✅ done) |
| `crates/raya-engine/src/parser/checker/builtins.rs` | Define signatures for string/array built-in methods (✅ done) |
| `crates/raya-engine/src/compiler/ir/instr.rs` | Add `InstanceOf`, `Cast`, `NativeCall` instructions (✅ done) |
| `crates/raya-engine/src/compiler/lower/expr.rs` | Lower intrinsics, type operators, **primitive method calls** (✅ done) |
| `crates/raya-engine/src/compiler/codegen/context.rs` | Emit new opcodes (✅ done) |
| `crates/raya-engine/src/vm/vm/interpreter.rs` | Handle `INSTANCEOF`, `CAST`, `NATIVE_CALL` (✅ done) |

---

## Opcodes

| Opcode | Hex | Stack Effect | Description |
|--------|-----|--------------|-------------|
| `ARRAY_PUSH` | `0xC9` | `[arr, value] → []` | Push element to array |
| `ARRAY_POP` | `0xCA` | `[arr] → [value]` | Pop element from array |
| `INSTANCE_OF` | `0xEE` | `[obj, class_id] → [bool]` | Check if object is instance of class |
| `CAST` | `0xEF` | `[obj, class_id] → [obj]` | Cast object to type (throws if invalid) |
| `NATIVE_CALL` | `0xFD` | `[args...] → [result]` | Call native function by ID |

---

## Native Call Dispatch

```rust
// crates/raya-core/src/vm/native_dispatch.rs
pub fn dispatch_native(id: u16, args: &[Value]) -> Result<Value, VmError> {
    match id {
        // Object (0x00xx)
        0x0001 => native_object_to_string(args),
        0x0002 => native_object_hash_code(args),
        0x0003 => native_object_equal(args),

        // Array (0x01xx)
        0x0100 => native_array_unshift(args),
        0x0101 => native_array_shift(args),
        0x0102 => native_array_slice(args),
        // ...

        // Mutex (0x02xx)
        0x0200 => native_mutex_try_lock(args),
        0x0201 => native_mutex_is_locked(args),

        // Task (0x03xx)
        0x0300 => native_task_is_done(args),
        0x0301 => native_task_is_cancelled(args),

        // Channel (0x04xx)
        0x0400 => native_channel_send(args),
        0x0401 => native_channel_receive(args),
        // ...

        // String (0x02xx) - primitive methods
        0x0200 => native_string_char_at(args),
        0x0201 => native_string_substring(args),
        // ...

        // RegExp (0x0Axx)
        0x0A00 => native_regexp_new(args),
        0x0A01 => native_regexp_test(args),
        0x0A02 => native_regexp_exec(args),
        // ...

        _ => Err(VmError::UnknownNativeFunction(id)),
    }
}
```

---

## Tests

### Phase 1 Tests
- [x] `test_mutex_basic_lock_unlock` (e2e/concurrency.rs)
- [x] `test_sleep_basic` (e2e/concurrency.rs)
- [x] `test_sleep_with_value` (e2e/concurrency.rs)
- [ ] `test_native_call_basic`

### Phase 2 Tests
- [x] `test_instanceof_same_class` (e2e/classes.rs)
- [x] `test_instanceof_inheritance` (e2e/classes.rs)
- [x] `test_instanceof_returns_false` (e2e/classes.rs)
- [ ] `test_instanceof_with_generics`
- [x] `test_cast_basic` (e2e/classes.rs)
- [ ] `test_cast_invalid_throws`
- [ ] `test_cast_upcast_always_succeeds`

### Phase 4 Tests (Primitive Methods)
- [x] `test_string_length` (e2e/strings.rs)
- [x] `test_string_char_at` (e2e/strings.rs)
- [x] `test_string_substring` (e2e/strings.rs)
- [x] `test_string_to_upper_case` (e2e/strings.rs)
- [x] `test_string_to_lower_case` (e2e/strings.rs)
- [x] `test_string_index_of` (e2e/strings.rs)
- [x] `test_string_split` (e2e/builtins.rs)
- [x] `test_string_trim` (e2e/strings.rs)
- [x] `test_string_starts_ends_with` (e2e/strings.rs)
- [x] `test_array_length`
- [x] `test_array_push_pop`
- [x] `test_array_slice`
- [x] `test_array_concat`
- [x] `test_array_index_of_includes`
- [x] `test_array_map`
- [x] `test_array_filter`
- [x] `test_array_reduce`
- [x] `test_array_find`
- [x] `test_array_every_some`
- [x] `test_regexp_test_basic`
- [x] `test_regexp_exec_match`
- [x] `test_regexp_exec_all`
- [x] `test_regexp_replace`
- [x] `test_regexp_split`
- [x] `test_regexp_stateless` (multiple calls return same result)
- [x] `test_regexp_flags`
- [x] `test_string_match_with_regexp` (match, matchAll, search, replace, split, replaceWith)

### Phase 5-10 Tests
- [ ] Tests for each built-in class (Map, Set, Buffer, Date - native functions not implemented)
- [x] Tests for class extension (e2e/classes.rs::test_class_extends)
- [ ] Tests for native method calls

### Phase 11 Tests (RegExpMatch)
- [ ] `test_regexp_match_properties`
- [ ] `test_regexp_match_groups`

---

## Success Criteria

1. Primitive methods work via hardcoded compiler support
2. String methods (`toUpperCase`, `split`, etc.) emit correct native calls
3. Array methods (`push`, `map`, `filter`, etc.) emit correct opcodes/native calls
4. RegExp methods (`test`, `exec`, etc.) emit correct native calls
5. All built-in classes defined as normal `.raya` files
6. `instanceof` and `as` operators work correctly
7. Users can extend built-in classes (but NOT primitives like `string`, `Array`, `RegExp`)
8. RegExp is stateless (no `lastIndex`) - same input always produces same output
9. Native call dispatch is efficient (no string lookup)
10. All existing tests continue to pass
11. New tests for built-in types pass

---

## References

- [design/BUILTIN_CLASSES.md](../design/BUILTIN_CLASSES.md) - Full class definitions
- [design/OPCODE.md](../design/OPCODE.md) - Bytecode instruction set
- [design/CHANNELS.md](../design/CHANNELS.md) - Channel semantics

---

**Last Updated:** 2026-01-28
