# Built-in Classes Design Document

**Last Updated:** 2026-01-28
**Status:** Draft

---

## Overview

This document defines Raya's built-in classes. All built-in classes are implemented as **normal Raya class definitions** that wrap opcode and native calls via compiler intrinsics.

### Design Principles

1. **All non-primitives inherit from Object** - Enables unified type checking via `instanceof`
2. **Normal class definitions** - Built-in classes are regular `.raya` files in stdlib
3. **Intrinsic wrappers** - Methods call `__OPCODE_*` or `__NATIVE_CALL` intrinsics
4. **No magic** - The class definitions are readable and behave like user classes
5. **Extensible** - Users can extend built-in classes normally

---

## Compiler Intrinsics

The compiler recognizes special intrinsic functions that emit opcodes or native calls.

### Mandatory Inlining

**CRITICAL:** Methods that only contain intrinsic calls MUST be inlined by the compiler. This ensures zero overhead - no double function call.

```typescript
// Source code:
mutex.lock();

// WITHOUT inlining (BAD - 2 calls):
CALL Mutex.lock        // Call method
  LOAD this.__mutexId  // Inside method body
  MUTEX_LOCK           // Emit opcode
  RETURN

// WITH inlining (GOOD - direct opcode):
LOAD mutex.__mutexId   // Inlined: load field
MUTEX_LOCK             // Inlined: emit opcode directly
```

The compiler marks methods containing only intrinsics as `@inline` automatically. For methods with a single intrinsic call and no other logic, the entire method body is replaced with the intrinsic's bytecode at the call site.

### Opcode Intrinsics

```typescript
// Syntax: __OPCODE_<NAME>(args...)
// Compiles directly to the specified opcode (NO function call overhead)

__OPCODE_MUTEX_NEW()              // → MUTEX_NEW opcode
__OPCODE_MUTEX_LOCK(mutexId)      // → MUTEX_LOCK opcode
__OPCODE_MUTEX_UNLOCK(mutexId)    // → MUTEX_UNLOCK opcode
__OPCODE_ARRAY_LEN(arr)           // → ARRAY_LEN opcode
__OPCODE_ARRAY_PUSH(arr, elem)    // → ARRAY_PUSH opcode
__OPCODE_ARRAY_POP(arr)           // → ARRAY_POP opcode
__OPCODE_CHANNEL_NEW(capacity)    // → CHANNEL_NEW opcode
__OPCODE_TASK_CANCEL(taskId)      // → TASK_CANCEL opcode
__OPCODE_INSTANCEOF(obj, classId) // → INSTANCEOF opcode
__OPCODE_CAST(obj, classId)       // → CAST opcode
```

### Native Call Intrinsic

```typescript
// Syntax: __NATIVE_CALL(nativeId, args...)
// Compiles to NATIVE_CALL opcode with the specified native function (NO wrapper call)

__NATIVE_CALL(__NATIVE_OBJECT_EQUAL, this, other)
__NATIVE_CALL(__NATIVE_ARRAY_SLICE, this, start, end)
__NATIVE_CALL(__NATIVE_MAP_GET, this, key)
```

### Native Function IDs

```typescript
// Object (0x00xx)
const __NATIVE_OBJECT_TO_STRING = 0x0001;
const __NATIVE_OBJECT_HASH_CODE = 0x0002;
const __NATIVE_OBJECT_EQUAL = 0x0003;

// Array (0x01xx)
const __NATIVE_ARRAY_UNSHIFT = 0x0100;
const __NATIVE_ARRAY_SHIFT = 0x0101;
const __NATIVE_ARRAY_SLICE = 0x0102;
const __NATIVE_ARRAY_CONCAT = 0x0103;
const __NATIVE_ARRAY_INDEX_OF = 0x0104;
const __NATIVE_ARRAY_INCLUDES = 0x0105;
const __NATIVE_ARRAY_REVERSE = 0x0106;
const __NATIVE_ARRAY_SORT = 0x0107;
const __NATIVE_ARRAY_JOIN = 0x0108;
const __NATIVE_ARRAY_MAP = 0x0109;
const __NATIVE_ARRAY_FILTER = 0x010A;
const __NATIVE_ARRAY_REDUCE = 0x010B;
const __NATIVE_ARRAY_FOR_EACH = 0x010C;
const __NATIVE_ARRAY_FIND = 0x010D;
const __NATIVE_ARRAY_FIND_INDEX = 0x010E;
const __NATIVE_ARRAY_EVERY = 0x010F;
const __NATIVE_ARRAY_SOME = 0x0110;

// Mutex
const __NATIVE_MUTEX_TRY_LOCK = 0x0200;
const __NATIVE_MUTEX_IS_LOCKED = 0x0201;

// Task
const __NATIVE_TASK_IS_DONE = 0x0300;
const __NATIVE_TASK_IS_CANCELLED = 0x0301;

// Channel
const __NATIVE_CHANNEL_SEND = 0x0400;
const __NATIVE_CHANNEL_RECEIVE = 0x0401;
const __NATIVE_CHANNEL_TRY_SEND = 0x0402;
const __NATIVE_CHANNEL_TRY_RECEIVE = 0x0403;
const __NATIVE_CHANNEL_CLOSE = 0x0404;
const __NATIVE_CHANNEL_IS_CLOSED = 0x0405;
const __NATIVE_CHANNEL_LENGTH = 0x0406;
const __NATIVE_CHANNEL_CAPACITY = 0x0407;

// Error
const __NATIVE_ERROR_STACK = 0x0500;

// Buffer
const __NATIVE_BUFFER_NEW = 0x0600;
// ... etc

// Map
const __NATIVE_MAP_NEW = 0x0700;
const __NATIVE_MAP_SIZE = 0x0701;
const __NATIVE_MAP_GET = 0x0702;
const __NATIVE_MAP_SET = 0x0703;
const __NATIVE_MAP_HAS = 0x0704;
const __NATIVE_MAP_DELETE = 0x0705;
const __NATIVE_MAP_CLEAR = 0x0706;
// ... etc

// Set
const __NATIVE_SET_NEW = 0x0800;
// ... etc

// Date
const __NATIVE_DATE_NOW = 0x0900;
// ... etc

// RegExp (0x0Axx)
const __NATIVE_REGEXP_NEW = 0x0A00;
const __NATIVE_REGEXP_TEST = 0x0A01;
const __NATIVE_REGEXP_EXEC = 0x0A02;
const __NATIVE_REGEXP_EXEC_ALL = 0x0A03;
const __NATIVE_REGEXP_REPLACE = 0x0A04;
const __NATIVE_REGEXP_REPLACE_WITH = 0x0A05;
const __NATIVE_REGEXP_SPLIT = 0x0A06;
```

---

## Type Classification

### Primitives (Not Classes)

| Type | Storage | Notes |
|------|---------|-------|
| `number` | 64-bit float | IEEE 754 double |
| `boolean` | 8-bit | true/false |
| `null` | Tag only | Singleton value |
| `string` | Interned pointer | Immutable, interned strings |
| `Array<T>` | Heap pointer | Generic array, but cannot be extended |
| `RegExp` | Heap pointer | Immutable, stateless regex pattern |

**Important:** `string`, `Array<T>`, and `RegExp` are primitives. They do not inherit from Object and cannot be extended. Their methods are hardcoded in the compiler (see Primitive Methods section below).

### Built-in Classes (Heap Objects)

```
Object
├── Mutex
├── Task<T>
├── Channel<T>
├── Error
├── Buffer
├── Map<K, V>
├── Set<T>
├── Date
└── RegExpMatch     (Result of RegExp.exec())
```

---

## Primitive Methods (Hardcoded in Compiler)

Primitives (`string`, `Array<T>`, `RegExp`) have methods that are hardcoded in the compiler. These are NOT classes - they cannot be extended. The compiler recognizes method calls on primitive types and emits the appropriate opcodes or native calls directly.

### No Special Syntax

There is no special syntax for defining primitive methods. They are built into the compiler:

```typescript
// User code - just call methods normally:
let s = "hello";
s.charAt(0);      // Compiler knows this is NATIVE_CALL(0x0201)
s.toUpperCase();  // Compiler knows this is NATIVE_CALL(0x0210)
s.length;         // Compiler knows this is SLEN opcode

let arr = [1, 2, 3];
arr.push(4);      // Compiler knows this is ARRAY_PUSH opcode
arr.length;       // Compiler knows this is ARRAY_LEN opcode
arr.map(x => x * 2);  // Compiler knows this is NATIVE_CALL(0x0110)

let re = /hello/i;    // RegExp literal (or new RegExp("hello", "i"))
re.test("Hello");     // Compiler knows this is NATIVE_CALL(0x0A01)
re.exec("Hello");     // Compiler knows this is NATIVE_CALL(0x0A02)
```

### How It Works

```typescript
// Source code:
let s = "hello";
s.charAt(0);

// Type checker recognizes:
// - s is type `string`
// - `charAt` is a built-in string method with signature (index: number): string
// - Returns NATIVE_CALL(0x0201, s, 0)

// Which emits:
LOAD s
CONST 0
NATIVE_CALL 0x0201    // String.charAt
```

### Implementation

The compiler has a built-in registry of primitive methods:

```rust
// In type checker
fn get_primitive_method(ty: &Type, name: &str) -> Option<MethodSignature> {
    match ty {
        Type::String => match name {
            "charAt" => Some(sig!((index: number) -> string)),
            "toUpperCase" => Some(sig!(() -> string)),
            "split" => Some(sig!((separator: string) -> Array<string>)),
            // ... more string methods
            _ => None,
        },
        Type::Array(elem_ty) => match name {
            "push" => Some(sig!((element: T) -> void)),
            "pop" => Some(sig!(() -> T | null)),
            "map" => Some(sig!(<U>((fn: (T, number) -> U)) -> Array<U>)),
            // ... more array methods
            _ => None,
        },
        Type::RegExp => match name {
            "test" => Some(sig!((str: string) -> boolean)),
            "exec" => Some(sig!((str: string) -> RegExpMatch | null)),
            "execAll" => Some(sig!((str: string) -> Array<RegExpMatch>)),
            "replace" => Some(sig!((str: string, replacement: string) -> string)),
            // ... more regexp methods
            _ => None,
        },
        _ => None,
    }
}
```

---

## String Methods

String methods are hardcoded in the compiler. The following methods are recognized:

| Method | Signature | Implementation |
|--------|-----------|----------------|
| `length` | Property: `number` | SLEN opcode |
| `charAt` | `(index: number): string` | NATIVE_CALL(0x0201) |
| `charCodeAt` | `(index: number): number` | NATIVE_CALL(0x0202) |
| `substring` | `(start: number, end?: number): string` | NATIVE_CALL(0x0203) |
| `indexOf` | `(search: string, position?: number): number` | NATIVE_CALL(0x0204) |
| `lastIndexOf` | `(search: string, position?: number): number` | NATIVE_CALL(0x0205) |
| `toUpperCase` | `(): string` | NATIVE_CALL(0x0210) |
| `toLowerCase` | `(): string` | NATIVE_CALL(0x0211) |
| `trim` | `(): string` | NATIVE_CALL(0x0220) |
| `trimStart` | `(): string` | NATIVE_CALL(0x0221) |
| `trimEnd` | `(): string` | NATIVE_CALL(0x0222) |
| `split` | `(separator: string): Array<string>` | NATIVE_CALL(0x0230) |
| `repeat` | `(count: number): string` | NATIVE_CALL(0x0240) |
| `startsWith` | `(prefix: string): boolean` | NATIVE_CALL(0x0250) |
| `endsWith` | `(suffix: string): boolean` | NATIVE_CALL(0x0251) |
| `includes` | `(search: string): boolean` | NATIVE_CALL(0x0252) |
| `replace` | `(search: string, replacement: string): string` | NATIVE_CALL(0x0260) |
| `padStart` | `(length: number, pad?: string): string` | NATIVE_CALL(0x0270) |
| `padEnd` | `(length: number, pad?: string): string` | NATIVE_CALL(0x0271) |

### String Native IDs

```typescript
const __NATIVE_STRING_CHAR_AT = 0x0200;
const __NATIVE_STRING_SUBSTRING = 0x0201;
const __NATIVE_STRING_TO_UPPER_CASE = 0x0202;
const __NATIVE_STRING_TO_LOWER_CASE = 0x0203;
const __NATIVE_STRING_TRIM = 0x0204;
const __NATIVE_STRING_INDEX_OF = 0x0205;
const __NATIVE_STRING_INCLUDES = 0x0206;
const __NATIVE_STRING_SPLIT = 0x0207;
const __NATIVE_STRING_STARTS_WITH = 0x0208;
const __NATIVE_STRING_ENDS_WITH = 0x0209;
const __NATIVE_STRING_REPLACE = 0x020A;
const __NATIVE_STRING_REPEAT = 0x020B;
const __NATIVE_STRING_PAD_START = 0x020C;
const __NATIVE_STRING_PAD_END = 0x020D;
```

---

## Array Methods

Array methods are hardcoded in the compiler. The following methods are recognized:

**Opcode-backed (performance critical):**

| Method | Signature | Implementation |
|--------|-----------|----------------|
| `length` | Property: `number` | ARRAY_LEN opcode |
| `push` | `(element: T): void` | ARRAY_PUSH opcode |
| `pop` | `(): T \| null` | ARRAY_POP opcode |

Note: Index access is handled by the compiler: `arr[i]` → LOAD_ELEM, `arr[i] = x` → STORE_ELEM

**Native-backed:**

| Method | Signature | Implementation |
|--------|-----------|----------------|
| `unshift` | `(element: T): void` | NATIVE_CALL(0x0100) |
| `shift` | `(): T \| null` | NATIVE_CALL(0x0101) |
| `slice` | `(start?: number, end?: number): Array<T>` | NATIVE_CALL(0x0102) |
| `concat` | `(other: Array<T>): Array<T>` | NATIVE_CALL(0x0103) |
| `indexOf` | `(element: T): number` | NATIVE_CALL(0x0104) |
| `lastIndexOf` | `(element: T): number` | NATIVE_CALL(0x0105) |
| `includes` | `(element: T): boolean` | NATIVE_CALL(0x0106) |
| `reverse` | `(): Array<T>` | NATIVE_CALL(0x0107) |
| `sort` | `(compareFn?: (a: T, b: T) => number): Array<T>` | NATIVE_CALL(0x0108) |
| `join` | `(separator?: string): string` | NATIVE_CALL(0x0109) |
| `map` | `<U>(fn: (T, number) => U): Array<U>` | NATIVE_CALL(0x0110) |
| `filter` | `(fn: (T, number) => boolean): Array<T>` | NATIVE_CALL(0x0111) |
| `reduce` | `<U>(fn: (U, T, number) => U, initial: U): U` | NATIVE_CALL(0x0112) |
| `forEach` | `(fn: (T, number) => void): void` | NATIVE_CALL(0x0113) |
| `find` | `(fn: (T, number) => boolean): T \| null` | NATIVE_CALL(0x0114) |
| `findIndex` | `(fn: (T, number) => boolean): number` | NATIVE_CALL(0x0115) |
| `every` | `(fn: (T, number) => boolean): boolean` | NATIVE_CALL(0x0116) |
| `some` | `(fn: (T, number) => boolean): boolean` | NATIVE_CALL(0x0117) |
| `fill` | `(value: T, start?: number, end?: number): Array<T>` | NATIVE_CALL(0x0118) |
| `flat` | `<U>(depth?: number): Array<U>` | NATIVE_CALL(0x0119) |

### Array Native IDs

```typescript
const __NATIVE_ARRAY_UNSHIFT = 0x0100;
const __NATIVE_ARRAY_SHIFT = 0x0101;
const __NATIVE_ARRAY_SLICE = 0x0102;
const __NATIVE_ARRAY_CONCAT = 0x0103;
const __NATIVE_ARRAY_INDEX_OF = 0x0104;
const __NATIVE_ARRAY_INCLUDES = 0x0105;
const __NATIVE_ARRAY_REVERSE = 0x0106;
const __NATIVE_ARRAY_SORT = 0x0107;
const __NATIVE_ARRAY_JOIN = 0x0108;
const __NATIVE_ARRAY_MAP = 0x0109;
const __NATIVE_ARRAY_FILTER = 0x010A;
const __NATIVE_ARRAY_REDUCE = 0x010B;
const __NATIVE_ARRAY_FOR_EACH = 0x010C;
const __NATIVE_ARRAY_FIND = 0x010D;
const __NATIVE_ARRAY_FIND_INDEX = 0x010E;
const __NATIVE_ARRAY_EVERY = 0x010F;
const __NATIVE_ARRAY_SOME = 0x0110;
```

---

## RegExp Methods

RegExp methods are hardcoded in the compiler. RegExp is **stateless** - unlike JavaScript, there is no `lastIndex` property. Each method call is independent and always starts matching from the beginning of the string. This makes RegExp immutable and safe for concurrent use.

### Creating RegExp

```typescript
// RegExp literal syntax
let re = /pattern/flags;

// Or constructor (compiles at runtime)
let re = new RegExp("pattern", "flags");
```

Both forms create a primitive RegExp value. The literal form is optimized at compile time.

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `source` | `string` | The pattern string |
| `flags` | `string` | The flags string |
| `global` | `boolean` | Whether 'g' flag is set |
| `ignoreCase` | `boolean` | Whether 'i' flag is set |
| `multiline` | `boolean` | Whether 'm' flag is set |
| `dotAll` | `boolean` | Whether 's' flag is set |
| `unicode` | `boolean` | Whether 'u' flag is set |

### Methods

| Method | Signature | Implementation |
|--------|-----------|----------------|
| `test` | `(str: string): boolean` | NATIVE_CALL(0x0A01) |
| `exec` | `(str: string): RegExpMatch \| null` | NATIVE_CALL(0x0A02) |
| `execAll` | `(str: string): Array<RegExpMatch>` | NATIVE_CALL(0x0A03) |
| `replace` | `(str: string, replacement: string): string` | NATIVE_CALL(0x0A04) |
| `replaceWith` | `(str: string, replacer: (match: RegExpMatch) => string): string` | NATIVE_CALL(0x0A05) |
| `split` | `(str: string, limit?: number): Array<string>` | NATIVE_CALL(0x0A06) |

### RegExp Native IDs

```typescript
const __NATIVE_REGEXP_NEW = 0x0A00;        // new RegExp(pattern, flags)
const __NATIVE_REGEXP_TEST = 0x0A01;       // re.test(str)
const __NATIVE_REGEXP_EXEC = 0x0A02;       // re.exec(str)
const __NATIVE_REGEXP_EXEC_ALL = 0x0A03;   // re.execAll(str)
const __NATIVE_REGEXP_REPLACE = 0x0A04;    // re.replace(str, replacement)
const __NATIVE_REGEXP_REPLACE_WITH = 0x0A05; // re.replaceWith(str, fn)
const __NATIVE_REGEXP_SPLIT = 0x0A06;      // re.split(str, limit)
```

### Design Notes

- **No `lastIndex`**: Every call to `test()`, `exec()`, etc. starts fresh from the beginning
- **Immutable**: RegExp instances cannot be modified after creation
- **Thread-safe**: Safe to share across tasks without synchronization
- **Stateless methods**: Same input always produces same output
- The 'g' (global) flag affects methods like `replace()` and `execAll()` but doesn't create internal state

### String Methods with RegExp

Additional string methods that accept RegExp are hardcoded in the compiler:

| Method | Signature | Implementation |
|--------|-----------|----------------|
| `match` | `(pattern: RegExp): RegExpMatch \| null` | Calls `pattern.exec(this)` |
| `matchAll` | `(pattern: RegExp): Array<RegExpMatch>` | Calls `pattern.execAll(this)` |
| `replace` | `(pattern: RegExp, replacement: string): string` | Calls `pattern.replace(this, replacement)` |
| `replaceWith` | `(pattern: RegExp, replacer: (RegExpMatch) => string): string` | Calls `pattern.replaceWith(this, replacer)` |
| `split` | `(pattern: RegExp, limit?: number): Array<string>` | Calls `pattern.split(this, limit)` |
| `search` | `(pattern: RegExp): number` | Calls `pattern.exec(this)`, returns index or -1 |

Note: These are overloads of the string methods. The compiler selects the RegExp overload when the argument is of type `RegExp`.

---

## Class Definitions

### Object (Base Class)

```typescript
// stdlib/Object.raya
class Object {
    // Internal object ID (assigned by VM)
    private __objectId: number;

    toString(): string {
        return __NATIVE_CALL(__NATIVE_OBJECT_TO_STRING, this.__objectId);
    }

    hashCode(): number {
        return __NATIVE_CALL(__NATIVE_OBJECT_HASH_CODE, this.__objectId);
    }

    equals(other: Object): boolean {
        return __NATIVE_CALL(__NATIVE_OBJECT_EQUAL, this.__objectId, other.__objectId);
    }
}
```

---

### Mutex

```typescript
// stdlib/Mutex.raya
class Mutex extends Object {
    // Internal mutex ID (unique identifier)
    private __mutexId: number;

    constructor() {
        super();
        this.__mutexId = __OPCODE_MUTEX_NEW();
    }

    // ═══════════════════════════════════════════════════════════════════
    // OPCODE-BACKED OPERATIONS
    // ═══════════════════════════════════════════════════════════════════

    lock(): void {
        __OPCODE_MUTEX_LOCK(this.__mutexId);
    }

    unlock(): void {
        __OPCODE_MUTEX_UNLOCK(this.__mutexId);
    }

    // ═══════════════════════════════════════════════════════════════════
    // NATIVE CALL OPERATIONS
    // ═══════════════════════════════════════════════════════════════════

    tryLock(): boolean {
        return __NATIVE_CALL(__NATIVE_MUTEX_TRY_LOCK, this.__mutexId);
    }

    isLocked(): boolean {
        return __NATIVE_CALL(__NATIVE_MUTEX_IS_LOCKED, this.__mutexId);
    }
}
```

---

### Task\<T>

```typescript
// stdlib/Task.raya
class Task<T> extends Object {
    // Internal task ID (assigned when spawned)
    private __taskId: number;

    // Task cannot be constructed directly - created by async function calls
    // The compiler creates Task instances internally via SPAWN opcode

    // ═══════════════════════════════════════════════════════════════════
    // OPCODE-BACKED OPERATIONS
    // ═══════════════════════════════════════════════════════════════════

    cancel(): void {
        __OPCODE_TASK_CANCEL(this.__taskId);
    }

    // Note: await is a language keyword, not a method
    // `await task` compiles to AWAIT opcode
    // `await [t1, t2]` compiles to WAIT_ALL opcode

    // ═══════════════════════════════════════════════════════════════════
    // NATIVE CALL OPERATIONS
    // ═══════════════════════════════════════════════════════════════════

    isDone(): boolean {
        return __NATIVE_CALL(__NATIVE_TASK_IS_DONE, this.__taskId);
    }

    isCancelled(): boolean {
        return __NATIVE_CALL(__NATIVE_TASK_IS_CANCELLED, this.__taskId);
    }
}
```

---

### Channel\<T>

```typescript
// stdlib/Channel.raya
class Channel<T> extends Object {
    // Internal channel ID
    private __channelId: number;

    // ═══════════════════════════════════════════════════════════════════
    // OPCODE-BACKED OPERATIONS
    // ═══════════════════════════════════════════════════════════════════

    constructor(capacity?: number) {
        super();
        this.__channelId = __OPCODE_CHANNEL_NEW(capacity ?? 0);
    }

    // ═══════════════════════════════════════════════════════════════════
    // NATIVE CALL OPERATIONS (need scheduler integration)
    // ═══════════════════════════════════════════════════════════════════

    send(value: T): void {
        __NATIVE_CALL(__NATIVE_CHANNEL_SEND, this.__channelId, value);
    }

    receive(): T | null {
        return __NATIVE_CALL(__NATIVE_CHANNEL_RECEIVE, this.__channelId);
    }

    trySend(value: T): boolean {
        return __NATIVE_CALL(__NATIVE_CHANNEL_TRY_SEND, this.__channelId, value);
    }

    tryReceive(): T | null {
        return __NATIVE_CALL(__NATIVE_CHANNEL_TRY_RECEIVE, this.__channelId);
    }

    close(): void {
        __NATIVE_CALL(__NATIVE_CHANNEL_CLOSE, this.__channelId);
    }

    isClosed(): boolean {
        return __NATIVE_CALL(__NATIVE_CHANNEL_IS_CLOSED, this.__channelId);
    }

    get length(): number {
        return __NATIVE_CALL(__NATIVE_CHANNEL_LENGTH, this.__channelId);
    }

    get capacity(): number {
        return __NATIVE_CALL(__NATIVE_CHANNEL_CAPACITY, this.__channelId);
    }
}
```

---

### Error

```typescript
// stdlib/Error.raya
class Error extends Object {
    message: string;

    constructor(message: string) {
        super();
        this.message = message;
    }

    toString(): string {
        return this.message;
    }

    get stack(): string {
        return __NATIVE_CALL(__NATIVE_ERROR_STACK, this);
    }
}

class TypeError extends Error {
    constructor(message: string) {
        super(message);
    }
}

class RangeError extends Error {
    constructor(message: string) {
        super(message);
    }
}

class ReferenceError extends Error {
    constructor(message: string) {
        super(message);
    }
}
```

---

### Buffer

```typescript
// stdlib/Buffer.raya
class Buffer extends Object {
    private __bufferPtr: number;

    constructor(size: number) {
        super();
        this.__bufferPtr = __NATIVE_CALL(__NATIVE_BUFFER_NEW, size);
    }

    get length(): number {
        return __NATIVE_CALL(__NATIVE_BUFFER_LENGTH, this.__bufferPtr);
    }

    getByte(index: number): number {
        return __NATIVE_CALL(__NATIVE_BUFFER_GET_BYTE, this.__bufferPtr, index);
    }

    setByte(index: number, value: number): void {
        __NATIVE_CALL(__NATIVE_BUFFER_SET_BYTE, this.__bufferPtr, index, value);
    }

    getInt32(index: number): number {
        return __NATIVE_CALL(__NATIVE_BUFFER_GET_INT32, this.__bufferPtr, index);
    }

    setInt32(index: number, value: number): void {
        __NATIVE_CALL(__NATIVE_BUFFER_SET_INT32, this.__bufferPtr, index, value);
    }

    getFloat64(index: number): number {
        return __NATIVE_CALL(__NATIVE_BUFFER_GET_FLOAT64, this.__bufferPtr, index);
    }

    setFloat64(index: number, value: number): void {
        __NATIVE_CALL(__NATIVE_BUFFER_SET_FLOAT64, this.__bufferPtr, index, value);
    }

    slice(start: number, end?: number): Buffer {
        return __NATIVE_CALL(__NATIVE_BUFFER_SLICE, this.__bufferPtr, start, end);
    }

    copy(target: Buffer, targetStart?: number, sourceStart?: number, sourceEnd?: number): number {
        return __NATIVE_CALL(__NATIVE_BUFFER_COPY, this.__bufferPtr, target.__bufferPtr, targetStart, sourceStart, sourceEnd);
    }

    toString(encoding?: string): string {
        return __NATIVE_CALL(__NATIVE_BUFFER_TO_STRING, this.__bufferPtr, encoding);
    }

    static from(str: string, encoding?: string): Buffer {
        return __NATIVE_CALL(__NATIVE_BUFFER_FROM_STRING, str, encoding);
    }
}
```

---

### Map\<K, V>

```typescript
// stdlib/Map.raya
class Map<K, V> extends Object {
    private __mapPtr: number;

    constructor() {
        super();
        this.__mapPtr = __NATIVE_CALL(__NATIVE_MAP_NEW);
    }

    get size(): number {
        return __NATIVE_CALL(__NATIVE_MAP_SIZE, this.__mapPtr);
    }

    get(key: K): V | null {
        return __NATIVE_CALL(__NATIVE_MAP_GET, this.__mapPtr, key);
    }

    set(key: K, value: V): void {
        __NATIVE_CALL(__NATIVE_MAP_SET, this.__mapPtr, key, value);
    }

    has(key: K): boolean {
        return __NATIVE_CALL(__NATIVE_MAP_HAS, this.__mapPtr, key);
    }

    delete(key: K): boolean {
        return __NATIVE_CALL(__NATIVE_MAP_DELETE, this.__mapPtr, key);
    }

    clear(): void {
        __NATIVE_CALL(__NATIVE_MAP_CLEAR, this.__mapPtr);
    }

    keys(): Array<K> {
        return __NATIVE_CALL(__NATIVE_MAP_KEYS, this.__mapPtr);
    }

    values(): Array<V> {
        return __NATIVE_CALL(__NATIVE_MAP_VALUES, this.__mapPtr);
    }

    entries(): Array<[K, V]> {
        return __NATIVE_CALL(__NATIVE_MAP_ENTRIES, this.__mapPtr);
    }

    forEach(fn: (value: V, key: K) => void): void {
        __NATIVE_CALL(__NATIVE_MAP_FOR_EACH, this.__mapPtr, fn);
    }
}
```

---

### Set\<T>

```typescript
// stdlib/Set.raya
class Set<T> extends Object {
    private __setPtr: number;

    constructor() {
        super();
        this.__setPtr = __NATIVE_CALL(__NATIVE_SET_NEW);
    }

    get size(): number {
        return __NATIVE_CALL(__NATIVE_SET_SIZE, this.__setPtr);
    }

    add(value: T): void {
        __NATIVE_CALL(__NATIVE_SET_ADD, this.__setPtr, value);
    }

    has(value: T): boolean {
        return __NATIVE_CALL(__NATIVE_SET_HAS, this.__setPtr, value);
    }

    delete(value: T): boolean {
        return __NATIVE_CALL(__NATIVE_SET_DELETE, this.__setPtr, value);
    }

    clear(): void {
        __NATIVE_CALL(__NATIVE_SET_CLEAR, this.__setPtr);
    }

    values(): Array<T> {
        return __NATIVE_CALL(__NATIVE_SET_VALUES, this.__setPtr);
    }

    forEach(fn: (value: T) => void): void {
        __NATIVE_CALL(__NATIVE_SET_FOR_EACH, this.__setPtr, fn);
    }

    union(other: Set<T>): Set<T> {
        return __NATIVE_CALL(__NATIVE_SET_UNION, this.__setPtr, other.__setPtr);
    }

    intersection(other: Set<T>): Set<T> {
        return __NATIVE_CALL(__NATIVE_SET_INTERSECTION, this.__setPtr, other.__setPtr);
    }

    difference(other: Set<T>): Set<T> {
        return __NATIVE_CALL(__NATIVE_SET_DIFFERENCE, this.__setPtr, other.__setPtr);
    }
}
```

---

### Date

```typescript
// stdlib/Date.raya
class Date extends Object {
    private __timestamp: number;

    constructor() {
        super();
        this.__timestamp = __NATIVE_CALL(__NATIVE_DATE_NOW);
    }

    // Overloaded constructors handled by compiler
    // constructor(timestamp: number)
    // constructor(year, month, day, ...)

    getTime(): number {
        return this.__timestamp;
    }

    getFullYear(): number {
        return __NATIVE_CALL(__NATIVE_DATE_GET_FULL_YEAR, this.__timestamp);
    }

    getMonth(): number {
        return __NATIVE_CALL(__NATIVE_DATE_GET_MONTH, this.__timestamp);
    }

    getDate(): number {
        return __NATIVE_CALL(__NATIVE_DATE_GET_DATE, this.__timestamp);
    }

    getDay(): number {
        return __NATIVE_CALL(__NATIVE_DATE_GET_DAY, this.__timestamp);
    }

    getHours(): number {
        return __NATIVE_CALL(__NATIVE_DATE_GET_HOURS, this.__timestamp);
    }

    getMinutes(): number {
        return __NATIVE_CALL(__NATIVE_DATE_GET_MINUTES, this.__timestamp);
    }

    getSeconds(): number {
        return __NATIVE_CALL(__NATIVE_DATE_GET_SECONDS, this.__timestamp);
    }

    getMilliseconds(): number {
        return __NATIVE_CALL(__NATIVE_DATE_GET_MILLISECONDS, this.__timestamp);
    }

    setTime(ms: number): void {
        this.__timestamp = ms;
    }

    setFullYear(year: number): void {
        this.__timestamp = __NATIVE_CALL(__NATIVE_DATE_SET_FULL_YEAR, this.__timestamp, year);
    }

    setMonth(month: number): void {
        this.__timestamp = __NATIVE_CALL(__NATIVE_DATE_SET_MONTH, this.__timestamp, month);
    }

    setDate(day: number): void {
        this.__timestamp = __NATIVE_CALL(__NATIVE_DATE_SET_DATE, this.__timestamp, day);
    }

    setHours(hours: number): void {
        this.__timestamp = __NATIVE_CALL(__NATIVE_DATE_SET_HOURS, this.__timestamp, hours);
    }

    setMinutes(minutes: number): void {
        this.__timestamp = __NATIVE_CALL(__NATIVE_DATE_SET_MINUTES, this.__timestamp, minutes);
    }

    setSeconds(seconds: number): void {
        this.__timestamp = __NATIVE_CALL(__NATIVE_DATE_SET_SECONDS, this.__timestamp, seconds);
    }

    setMilliseconds(ms: number): void {
        this.__timestamp = __NATIVE_CALL(__NATIVE_DATE_SET_MILLISECONDS, this.__timestamp, ms);
    }

    toString(): string {
        return __NATIVE_CALL(__NATIVE_DATE_TO_STRING, this.__timestamp);
    }

    toISOString(): string {
        return __NATIVE_CALL(__NATIVE_DATE_TO_ISO_STRING, this.__timestamp);
    }

    toDateString(): string {
        return __NATIVE_CALL(__NATIVE_DATE_TO_DATE_STRING, this.__timestamp);
    }

    toTimeString(): string {
        return __NATIVE_CALL(__NATIVE_DATE_TO_TIME_STRING, this.__timestamp);
    }

    static now(): number {
        return __NATIVE_CALL(__NATIVE_DATE_NOW);
    }

    static parse(str: string): number {
        return __NATIVE_CALL(__NATIVE_DATE_PARSE, str);
    }
}
```

---

## Type Operators

### instanceof

```typescript
// Syntax
expression instanceof ClassName

// Compiles to:
__OPCODE_INSTANCEOF(expression, ClassName.__classId)

// Example
let obj: Object = new Array<number>();
if (obj instanceof Array) {
    // obj is narrowed to Array<unknown> here
}
```

**Semantics:**
- Returns `true` if object's class is the target class or a subclass
- Works with generics: `obj instanceof Array` matches any `Array<T>`
- Always returns `false` for primitives
- Compile-time error if check is impossible

---

### as (Type Cast)

```typescript
// Syntax
expression as TargetType

// Compiles to:
__OPCODE_CAST(expression, TargetType.__classId)

// Example - use instanceof first for safety
let vehicle: Vehicle = getVehicle();
if (vehicle instanceof Car) {
    let car: Car = vehicle as Car;  // Safe
    car.drive();
}
```

**Semantics:**
- **Downcasting** (parent → child): Runtime check, throws `TypeError` if invalid
- **Upcasting** (child → parent): Always succeeds, no runtime check
- **Same type**: No-op
- **Incompatible types**: Compile-time error

---

## Summary Tables

### Opcode Intrinsics

| Intrinsic | Opcode | Stack Effect | Type |
|-----------|--------|--------------|------|
| `__OPCODE_STRING_LEN(str)` | `STRING_LEN` | `[str] → [len]` | Primitive |
| `__OPCODE_ARRAY_LEN(arr)` | `ARRAY_LEN` | `[arr] → [len]` | Primitive |
| `__OPCODE_ARRAY_PUSH(arr, elem)` | `ARRAY_PUSH` | `[arr, elem] → []` | Primitive |
| `__OPCODE_ARRAY_POP(arr)` | `ARRAY_POP` | `[arr] → [elem]` | Primitive |
| `__OPCODE_MUTEX_NEW()` | `MUTEX_NEW` | `[] → [mutexId]` | Class |
| `__OPCODE_MUTEX_LOCK(id)` | `MUTEX_LOCK` | `[id] → []` | Class |
| `__OPCODE_MUTEX_UNLOCK(id)` | `MUTEX_UNLOCK` | `[id] → []` | Class |
| `__OPCODE_CHANNEL_NEW(cap)` | `CHANNEL_NEW` | `[cap] → [chId]` | Class |
| `__OPCODE_TASK_CANCEL(id)` | `TASK_CANCEL` | `[id] → []` | Class |
| `__OPCODE_INSTANCEOF(obj, cls)` | `INSTANCEOF` | `[obj, cls] → [bool]` | Operator |
| `__OPCODE_CAST(obj, cls)` | `CAST` | `[obj, cls] → [obj]` | Operator |

### Native Call Dispatch

| Native ID | Name | Used By |
|-----------|------|---------|
| `0x0001` | `__NATIVE_OBJECT_TO_STRING` | `Object.toString()` |
| `0x0002` | `__NATIVE_OBJECT_HASH_CODE` | `Object.hashCode()` |
| `0x0003` | `__NATIVE_OBJECT_EQUAL` | `Object.equals()` |
| `0x0100` | `__NATIVE_ARRAY_UNSHIFT` | `arr.unshift()` (primitive) |
| `0x0101` | `__NATIVE_ARRAY_SHIFT` | `arr.shift()` (primitive) |
| `0x0102` | `__NATIVE_ARRAY_SLICE` | `arr.slice()` (primitive) |
| `0x0200` | `__NATIVE_STRING_CHAR_AT` | `str.charAt()` (primitive) |
| `0x0201` | `__NATIVE_STRING_SUBSTRING` | `str.substring()` (primitive) |
| ... | ... | ... |

---

## Extending Built-in Classes

**Note:** Primitives (`string`, `Array<T>`, `RegExp`) CANNOT be extended. Only classes can be extended.

```typescript
// ERROR: Cannot extend primitive type
class MyArray<T> extends Array<T> { }  // Compile error!
class MyRegExp extends RegExp { }       // Compile error!

// OK: Classes can be extended
class TimestampedError extends Error {
    timestamp: Date;

    constructor(message: string) {
        super(message);
        this.timestamp = new Date();
    }
}

class MyMutex extends Mutex {
    name: string;

    constructor(name: string) {
        super();
        this.name = name;
    }
}
```

**Rules:**
- Primitives (`number`, `boolean`, `null`, `string`, `Array<T>`, `RegExp`) cannot be extended
- Classes (`Object`, `Mutex`, `Task`, `Channel`, `Error`, `Buffer`, `Map`, `Set`, `Date`, `RegExpMatch`) can be extended
- Extended classes inherit all methods and properties
- Can override methods
- `super` calls work as expected
- `instanceof` works correctly with inheritance chain (classes only)

---

### RegExpMatch

RegExpMatch is the result object returned by `RegExp.exec()` and related methods. It is a simple immutable class.

```typescript
// stdlib/RegExpMatch.raya
class RegExpMatch extends Object {
    readonly match: string;           // The matched text
    readonly index: number;           // Start position in input string
    readonly input: string;           // Original input string
    readonly groups: Array<string>;   // Captured groups (index 0 is full match)
}
```

**Note:** `RegExp` itself is a **primitive type**, not a class. See the "RegExp Methods" section above for its methods. Only `RegExpMatch` (the result type) is a class.

---

## References

- [design/OPCODE.md](OPCODE.md) - Bytecode instruction set
- [design/ARCHITECTURE.md](ARCHITECTURE.md) - VM architecture
- [design/CHANNELS.md](CHANNELS.md) - Channel design details
