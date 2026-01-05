# Raya VM Opcode Design

Bytecode instruction set for the Raya virtual machine.

---

## 1. Design Principles

### 1.1 Type-Aware Instructions

Raya's static type system allows the compiler to emit **typed opcodes** that:

* Avoid runtime type checking in hot paths
* Enable unboxed primitive operations
* Provide better optimization opportunities

### 1.2 Stack-Based Architecture

The Raya VM uses a **stack-based** bytecode model:

* Operands pushed/popped from operand stack
* Local variables accessed via indices
* Simpler to generate and verify

### 1.3 Opcode Categories

1. **Arithmetic & Logic** — typed operations on primitives
2. **Control Flow** — jumps, branches, calls, returns
3. **Memory Access** — locals, fields, arrays
4. **Object Operations** — allocation, field access
5. **Task & Concurrency** — spawn, await, yield
6. **Synchronization** — mutex operations
7. **Type Operations** — type checks, casts

---

## 2. Opcode Format

### 2.1 General Format

```
[OPCODE] [OPERAND1] [OPERAND2] ...
```

* **OPCODE**: 1 byte
* **OPERANDS**: variable-length (1-4 bytes each, depending on instruction)

### 2.2 Operand Types

* **u8**: 1-byte unsigned integer
* **u16**: 2-byte unsigned integer
* **u32**: 4-byte unsigned integer
* **i32**: 4-byte signed integer
* **index**: local/global/constant pool index (typically u16 or u32)

---

## 3. Instruction Set

### 3.1 Stack Manipulation

| Opcode | Operands | Description |
|--------|----------|-------------|
| `NOP` | — | No operation |
| `POP` | — | Pop top value from stack |
| `DUP` | — | Duplicate top stack value |
| `SWAP` | — | Swap top two stack values |

---

### 3.2 Constants

| Opcode | Operands | Description |
|--------|----------|-------------|
| `CONST_NULL` | — | Push `null` |
| `CONST_TRUE` | — | Push `true` |
| `CONST_FALSE` | — | Push `false` |
| `CONST_I32` | `i32` | Push 32-bit integer constant |
| `CONST_F64` | `f64` | Push 64-bit float constant |
| `CONST_STR` | `index` | Push string constant from pool |
| `LOAD_CONST` | `index` | Load constant from constant pool |

---

### 3.3 Local Variables

| Opcode | Operands | Description |
|--------|----------|-------------|
| `LOAD_LOCAL` | `index` | Push local variable onto stack |
| `STORE_LOCAL` | `index` | Pop stack, store in local variable |
| `LOAD_LOCAL_0` | — | Push local 0 (optimized) |
| `LOAD_LOCAL_1` | — | Push local 1 (optimized) |
| `STORE_LOCAL_0` | — | Store to local 0 (optimized) |
| `STORE_LOCAL_1` | — | Store to local 1 (optimized) |

---

### 3.4 Arithmetic Operations (Typed)

#### Integer Operations

| Opcode | Operands | Description |
|--------|----------|-------------|
| `IADD` | — | Pop two ints, push sum |
| `ISUB` | — | Pop two ints, push difference |
| `IMUL` | — | Pop two ints, push product |
| `IDIV` | — | Pop two ints, push quotient |
| `IMOD` | — | Pop two ints, push remainder |
| `INEG` | — | Pop int, push negation |

#### Float Operations

| Opcode | Operands | Description |
|--------|----------|-------------|
| `FADD` | — | Pop two floats, push sum |
| `FSUB` | — | Pop two floats, push difference |
| `FMUL` | — | Pop two floats, push product |
| `FDIV` | — | Pop two floats, push quotient |
| `FNEG` | — | Pop float, push negation |

#### Number Operations (Generic)

| Opcode | Operands | Description |
|--------|----------|-------------|
| `NADD` | — | Pop two numbers, push sum |
| `NSUB` | — | Pop two numbers, push difference |
| `NMUL` | — | Pop two numbers, push product |
| `NDIV` | — | Pop two numbers, push quotient |
| `NMOD` | — | Pop two numbers, push remainder |
| `NNEG` | — | Pop number, push negation |

---

### 3.5 Comparison Operations

| Opcode | Operands | Description |
|--------|----------|-------------|
| `IEQ` | — | Int equality: a == b |
| `INE` | — | Int inequality: a != b |
| `ILT` | — | Int less than: a < b |
| `ILE` | — | Int less or equal: a <= b |
| `IGT` | — | Int greater than: a > b |
| `IGE` | — | Int greater or equal: a >= b |
| `FEQ` | — | Float equality |
| `FNE` | — | Float inequality |
| `FLT` | — | Float less than |
| `FLE` | — | Float less or equal |
| `FGT` | — | Float greater than |
| `FGE` | — | Float greater or equal |
| `EQ` | — | Generic equality (structural) |
| `NE` | — | Generic inequality |
| `STRICT_EQ` | — | Strict equality (===) |
| `STRICT_NE` | — | Strict inequality (!==) |

---

### 3.6 Logical Operations & Type Checking

| Opcode | Operands | Description |
|--------|----------|-------------|
| `NOT` | — | Logical NOT |
| `AND` | — | Logical AND (short-circuit via jumps) |
| `OR` | — | Logical OR (short-circuit via jumps) |
| `TYPEOF` | — | Pop value, push type string ("null", "boolean", "number", "string", "object", "function") |

**TYPEOF Details:**
- Returns a string indicating the runtime type of a value
- Used for type narrowing in bare unions of primitives
- Return values:
  - `null` → `"null"`
  - `true`/`false` → `"boolean"`
  - `42`/`3.14` → `"number"`
  - `"hello"` → `"string"`
  - `{x:1}` / `[1,2]` → `"object"`
  - `() => {}` → `"function"`
- Combined with conditional jumps for control flow-based type narrowing
- Always available (not a reflection operation)

---

### 3.7 String Operations

| Opcode | Operands | Description |
|--------|----------|-------------|
| `SCONCAT` | — | Pop two strings, push concatenation |
| `SLEN` | — | Pop string, push length |

**SCONCAT Details:**
- Pops two string pointers from stack (str2, then str1)
- Type checked: both values must be heap pointers to RayaString
- Creates new RayaString with concatenated data
- Allocates result on GC heap
- Stack effect: `[str1, str2] → [result]`
- Result is UTF-8 string: `str1.data + str2.data`

**SLEN Details:**
- Returns string length in bytes (not character count)
- UTF-8 strings may have length != character count
- Stack effect: `[string] → [length:i32]`

---

### 3.8 Control Flow

| Opcode | Operands | Description |
|--------|----------|-------------|
| `JMP` | `offset` | Unconditional jump to offset |
| `JMP_IF_FALSE` | `offset` | Pop value, jump if false |
| `JMP_IF_TRUE` | `offset` | Pop value, jump if true |
| `JMP_IF_NULL` | `offset` | Pop value, jump if null |
| `JMP_IF_NOT_NULL` | `offset` | Pop value, jump if not null |

---

### 3.9 Function Calls

| Opcode | Operands | Description |
|--------|----------|-------------|
| `CALL` | `funcIndex`, `argCount` | Call function with N arguments |
| `CALL_METHOD` | `u16 methodIndex`, `u8 argCount` | Call method on object (vtable dispatch) |
| `RETURN` | — | Return from function (pop return value) |
| `RETURN_VOID` | — | Return from void function |

**CALL_METHOD Details:**
- Virtual method dispatch via vtable lookup
- Receiver object must be on stack at position `depth - argCount - 1`
- Peeks at receiver without popping to get class ID
- Looks up class from ClassRegistry
- Looks up method function ID from class vtable using methodIndex
- Executes function with receiver + arguments on stack
- Stack layout before call: `[receiver, arg1, arg2, ..., argN]`
- Stack effect: `[receiver, args...] → [result]`
- Supports polymorphism: different classes can have different implementations

---

### 3.10 Object Operations

| Opcode | Operands | Description |
|--------|----------|-------------|
| `NEW` | `u16 classIndex` | Allocate new object of class |
| `LOAD_FIELD` | `u16 fieldOffset` | Pop object, push field value |
| `STORE_FIELD` | `u16 fieldOffset` | Pop value, pop object, store field |
| `LOAD_FIELD_FAST` | `u8 offset` | Load field at known offset (optimized) |
| `STORE_FIELD_FAST` | `u8 offset` | Store field at known offset (optimized) |

**NEW Details:**
- Looks up class metadata from ClassRegistry by index
- Allocates object on GC heap with field_count slots
- All fields initialized to null
- Pushes GC pointer as tagged Value
- Stack effect: `[] → [object]`

**LOAD_FIELD / STORE_FIELD Details:**
- Field offset is absolute index into object's field array
- Bounds checked at runtime (errors if offset >= field_count)
- Type checked: value must be a heap pointer
- Stack effects:
  - LOAD_FIELD: `[object] → [value]`
  - STORE_FIELD: `[object, value] → []`

**LOAD_FIELD_FAST / STORE_FIELD_FAST:**
- Same as regular variants but with u8 offset (0-255)
- Used when compiler knows field index is small
- Saves 1 byte per instruction

---

### 3.11 Array Operations

| Opcode | Operands | Description |
|--------|----------|-------------|
| `NEW_ARRAY` | `u16 typeIndex` | Pop length, create array of type |
| `LOAD_ELEM` | — | Pop index, pop array, push element |
| `STORE_ELEM` | — | Pop value, pop index, pop array, store |
| `ARRAY_LEN` | — | Pop array, push length |

**NEW_ARRAY Details:**
- Pops length from stack (must be i32)
- Maximum length: 10,000,000 elements (runtime check)
- All elements initialized to null
- Allocates on GC heap with type metadata
- Stack effect: `[length:i32] → [array]`

**LOAD_ELEM / STORE_ELEM Details:**
- Index must be i32, converted to usize
- Bounds checked at runtime (errors if index >= length)
- Type checked: array value must be heap pointer
- Stack effects:
  - LOAD_ELEM: `[array, index:i32] → [value]`
  - STORE_ELEM: `[array, index:i32, value] → []`

**ARRAY_LEN Details:**
- Returns length as i32
- Stack effect: `[array] → [length:i32]`

---

### 3.12 Task & Concurrency

| Opcode | Operands | Description |
|--------|----------|-------------|
| `SPAWN` | `funcIndex`, `argCount` | Spawn new Task, push TaskHandle |
| `AWAIT` | — | Pop TaskHandle, block until complete, push result |
| `YIELD` | — | Voluntary yield to scheduler |
| `TASK_THEN` | `funcIndex` | Register continuation on Task |

---

### 3.13 Synchronization (Mutex)

| Opcode | Operands | Description |
|--------|----------|-------------|
| `MUTEX_LOCK` | `mutexRef` | Acquire mutex (may block) |
| `MUTEX_UNLOCK` | `mutexRef` | Release mutex |
| `NEW_MUTEX` | — | Create new Mutex object, push reference |

---

### 3.14 Type Operations - REMOVED

All runtime type operations are **removed** from Raya. Types are verified at compile time only.

| Opcode | Status | Replacement |
|--------|--------|-------------|
| ~~`TYPEOF`~~ | **REMOVED** | Discriminated unions at compile time |
| ~~`INSTANCEOF`~~ | **REMOVED** | Discriminated unions at compile time |
| ~~`CHECK_TYPE`~~ | **REMOVED** | Compiler type checking |
| ~~`CAST`~~ | **REMOVED** | Compiler-verified type narrowing |

**Rationale:**

Raya is a **fully statically typed language**. All type information is:
- **Known at compile time** — Every value's type is determined during compilation
- **Verified by compiler** — Type errors are caught before execution
- **Erased at runtime** — VM operates on typed values without type tags

**How it works:**

1. **Discriminated Unions** — Variants identified by value-based discriminants (strings), not type tags
2. **Monomorphization** — Generic code specialized for each concrete type at compile time
3. **Vtables** — Method dispatch resolved via class metadata, not runtime type queries
4. **Compile-time narrowing** — Switch on discriminant fields narrows types statically

---

### 3.15 Error Handling

| Opcode | Operands | Description |
|--------|----------|-------------|
| `THROW` | — | Pop error value, terminate Task with error |
| `TRAP` | `errorCode` | Immediate error trap (e.g., null access, bounds check) |

---

### 3.16 String Conversion

| Opcode | Operands | Description |
|--------|----------|-------------|
| `TO_STRING` | — | Pop value, push string representation |

---

### 3.17 Global Variables

| Opcode | Operands | Description |
|--------|----------|-------------|
| `LOAD_GLOBAL` | `index` | Load global variable onto stack |
| `STORE_GLOBAL` | `index` | Pop stack, store in global variable |

---

### 3.18 Closures

| Opcode | Operands | Description |
|--------|----------|-------------|
| `MAKE_CLOSURE` | `funcIndex`, `captureCount` | Create closure object |
| `CLOSE_VAR` | `localIndex` | Capture local variable in closure |
| `LOAD_CAPTURED` | `index` | Load captured variable from closure |
| `STORE_CAPTURED` | `index` | Store to captured variable in closure |

---

### 3.19 Advanced Object Operations

| Opcode | Operands | Description |
|--------|----------|-------------|
| `CALL_CONSTRUCTOR` | `ctorIndex`, `argCount` | Call constructor on object |
| `CALL_SUPER` | `superCtorIndex`, `argCount` | Call parent class constructor |
| `OBJECT_LITERAL` | `typeIndex`, `fieldCount` | Create object literal |
| `INIT_OBJECT` | `count` | Pop N values and initialize object fields |
| `OPTIONAL_FIELD` | `offset` | Optional chaining field access (null-safe) |

---

### 3.20 Static Members

| Opcode | Operands | Description |
|--------|----------|-------------|
| `LOAD_STATIC` | `staticIndex` | Load static field |
| `STORE_STATIC` | `staticIndex` | Store static field |
| `CALL_STATIC` | `methodIndex`, `argCount` | Call static method |

---

### 3.21 Advanced Array Operations

| Opcode | Operands | Description |
|--------|----------|-------------|
| `ARRAY_LITERAL` | `typeIndex`, `length` | Allocate array for literal |
| `INIT_ARRAY` | `count` | Pop N values and store in array |

---

### 3.22 Tuple Operations

| Opcode | Operands | Description |
|--------|----------|-------------|
| `TUPLE_LITERAL` | `typeIndex`, `length` | Allocate tuple |
| `INIT_TUPLE` | `count` | Pop N values and initialize tuple |
| `TUPLE_GET` | — | Pop index, pop tuple, push element |

---

### 3.23 Module Operations

| Opcode | Operands | Description |
|--------|----------|-------------|
| `LOAD_MODULE` | `moduleIndex` | Load module namespace object |

---

### 3.24 String Comparison

| Opcode | Operands | Description |
|--------|----------|-------------|
| `SEQ` | — | String equality |
| `SNE` | — | String inequality |
| `SLT` | — | String less than (lexicographic) |
| `SLE` | — | String less or equal |
| `SGT` | — | String greater than |
| `SGE` | — | String greater or equal |

---

### 3.25 Reflection Operations (Optional)

**Note:** These opcodes are only available when code is compiled with `--emit-reflection` flag.

| Opcode | Operands | Description |
|--------|----------|-------------|
| `REFLECT_TYPEOF` | — | Pop value, push TypeInfo object |
| `REFLECT_TYPEINFO` | `typeIndex` | Push TypeInfo for type at index |
| `REFLECT_INSTANCEOF` | — | Pop TypeInfo, pop value, push boolean |
| `REFLECT_GET_PROPS` | — | Pop object, push PropertyInfo array |
| `REFLECT_GET_PROP` | — | Pop property name (string), pop object, push property value |
| `REFLECT_SET_PROP` | — | Pop value, pop property name (string), pop object, set property |
| `REFLECT_HAS_PROP` | — | Pop property name (string), pop object, push boolean |
| `REFLECT_CONSTRUCT` | `argCount` | Pop N args, pop TypeInfo, construct instance, push object |

**Rationale:**

Reflection opcodes enable **optional runtime type introspection** for:
- **TypeScript compatibility shims** — Implement `typeof`/`instanceof` using reflection
- **Dynamic serialization** — Runtime-based JSON converters (note: standard JSON uses compile-time codegen, see LANG.md 17.7)
- **Debugging tools** — Runtime inspection of object structure
- **Interoperability** — Bridge to dynamic languages

**Performance:**
- Reflection metadata embedded when `--emit-reflection` is used
- ~10-30% binary size increase (metadata only)
- No overhead for code that doesn't use Reflect API
- Reflection API calls have runtime cost (property lookups, etc.)

**Design:**
- Reflection is **compile-time opt-in**, not a language feature
- Core language remains fully statically typed
- TypeInfo objects are regular Raya objects (can be inspected, stored, etc.)
- Monomorphized types have separate TypeInfo (e.g., `Box_number` vs `Box_string`)

---

## 4. Opcode Encoding

### 4.1 Opcode Numbering

Opcodes are numbered sequentially:

```
0x00: NOP
0x01: POP
0x02: DUP
0x03: SWAP
...
0x40: SPAWN
0x41: AWAIT
0x42: YIELD
...
```

### 4.2 Extended Opcodes

For future expansion, reserve `0xFF` as an **extended opcode prefix**:

```
[0xFF] [EXTENDED_OPCODE]
```

This allows 256 base opcodes + 256 extended opcodes.

---

## 5. Bytecode Optimization Strategies

### 5.1 Specialized Fast Paths

* `LOAD_LOCAL_0`, `LOAD_LOCAL_1` — single-byte opcodes for common locals
* `LOAD_FIELD_FAST` — inlined field access with known offset

### 5.2 Type-Specific Operations

* Emit `IADD` / `FADD` based on static type analysis
* Reduces need for dynamic dispatch and type checking

### 5.3 Constant Folding

* Compiler can pre-compute constant expressions
* Emit direct `CONST_I32` instead of operations

### 5.4 Dead Code Elimination

* Remove unreachable code paths
* Reduce bytecode size and improve cache locality

---

## 6. Bytecode Verification

Before execution, the VM verifies:

* Type consistency (stack types match expected types)
* No stack underflow/overflow
* Valid jump targets
* Valid constant pool indices
* Mutex lock/unlock pairing (static analysis where possible)

---

## 7. Example Bytecode

### 7.1 Simple Addition

Raya source:

```ts
function add(a: number, b: number): number {
  return a + b;
}
```

Bytecode:

```
LOAD_LOCAL_0        // load a
LOAD_LOCAL_1        // load b
NADD                // a + b
RETURN              // return result
```

### 7.2 Async Task

Raya source:

```ts
async function compute(): Task<number> {
  return 42;
}

async function main(): Task<void> {
  const result = await compute();
}
```

Bytecode for `compute`:

```
CONST_I32 42
RETURN
```

Bytecode for `main`:

```
SPAWN <compute>, 0     // spawn compute Task
AWAIT                  // await its completion
POP                    // discard result
RETURN_VOID
```

### 7.3 Mutex Lock/Unlock

Raya source:

```ts
const mu = new Mutex();
let counter = 0;

function increment(): void {
  mu.lock();
  counter = counter + 1;
  mu.unlock();
}
```

Bytecode:

```
LOAD_GLOBAL <mu>       // load mutex
MUTEX_LOCK             // acquire lock
LOAD_GLOBAL <counter>  // load counter
CONST_I32 1
IADD                   // counter + 1
STORE_GLOBAL <counter> // store back
LOAD_GLOBAL <mu>
MUTEX_UNLOCK           // release lock
RETURN_VOID
```

---

### 7.4 Closure

Raya source:

```ts
function makeCounter(): () => number {
  let count = 0;
  return () => {
    count = count + 1;
    return count;
  };
}
```

Bytecode for `makeCounter`:

```
CONST_I32 0
STORE_LOCAL 0         // count

MAKE_CLOSURE <inner_func>, 1
LOAD_LOCAL 0
CLOSE_VAR 0
RETURN
```

Bytecode for inner function:

```
LOAD_CAPTURED 0
CONST_I32 1
IADD
STORE_CAPTURED 0
LOAD_CAPTURED 0
RETURN
```

---

### 7.5 Class with Constructor

Raya source:

```ts
class Point {
  constructor(public x: number, public y: number) {}
}

let p = new Point(10, 20);
```

Bytecode for instantiation:

```
NEW <Point>
DUP
CONST_I32 10
CONST_I32 20
CALL_CONSTRUCTOR <Point.constructor>, 2
STORE_LOCAL 0
```

---

### 7.6 Array Literal

Raya source:

```ts
let arr = [1, 2, 3];
```

Bytecode:

```
ARRAY_LITERAL <number>, 3
CONST_I32 1
CONST_I32 2
CONST_I32 3
INIT_ARRAY 3
STORE_LOCAL 0
```

---

### 7.7 Template String

Raya source:

```ts
let name = "World";
let greeting = `Hello, ${name}!`;
```

Bytecode:

```
CONST_STR 0           // "World"
STORE_LOCAL 0

CONST_STR 1           // "Hello, "
LOAD_LOCAL 0
TO_STRING
SCONCAT
CONST_STR 2           // "!"
SCONCAT
STORE_LOCAL 1
```

---

### 7.8 Optional Chaining

Raya source:

```ts
let city = user?.address?.city;
```

Bytecode:

```
LOAD_LOCAL 0          // user
OPTIONAL_FIELD <address_offset>
OPTIONAL_FIELD <city_offset>
STORE_LOCAL 1
```

---

## 8. Future Opcode Extensions

### 8.1 SIMD Operations

* `VADD_F32x4` — vectorized float addition
* `VMUL_F32x4` — vectorized float multiplication

### 8.2 Channel Operations (Go-style)

* `CHAN_SEND` — send value to channel
* `CHAN_RECV` — receive value from channel
* `CHAN_SELECT` — select on multiple channels

### 8.3 Atomic Operations

* `ATOMIC_LOAD` — atomic read
* `ATOMIC_STORE` — atomic write
* `ATOMIC_ADD` — atomic fetch-and-add
* `ATOMIC_CAS` — compare-and-swap

---

The Raya opcode design provides a clean, type-aware instruction set that maps efficiently to Raya's semantics while enabling performance optimizations through static type information.
