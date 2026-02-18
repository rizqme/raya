---
title: "Compilation Mapping"
---

# Raya Language to Bytecode Mapping

> **Status:** Implemented
> **Related:** [Language Spec](../language/lang.md), [Opcodes](./opcode.md), [Architecture](../runtime/architecture.md)

This document describes how each Raya language feature translates to VM bytecode opcodes.

---

## Table of Contents

1. [Literals](#1-literals)
2. [Variables](#2-variables)
3. [Arithmetic Operations](#3-arithmetic-operations)
4. [Comparison Operations](#4-comparison-operations)
5. [Logical Operations](#5-logical-operations)
6. [Control Flow](#6-control-flow)
7. [Functions](#7-functions)
8. [Classes](#8-classes)
9. [Arrays](#9-arrays)
10. [Tuples](#10-tuples)
11. [Object Literals](#11-object-literals)
12. [Concurrency](#12-concurrency)
13. [Synchronization](#13-synchronization)
14. [Modules](#14-modules)
15. [Type Operations](#15-type-operations)
16. [String Operations](#16-string-operations)

---

## 1. Literals

### 1.1 Null Literal

**Raya:**
```ts
let x = null;
```

**Bytecode:**
```
CONST_NULL
STORE_LOCAL 0
```

**Explanation:** Push `null` constant, store in local variable 0.

---

### 1.2 Boolean Literals

**Raya:**
```ts
let flag = true;
let other = false;
```

**Bytecode:**
```
CONST_TRUE
STORE_LOCAL 0

CONST_FALSE
STORE_LOCAL 1
```

**Explanation:** Push boolean constants directly onto stack.

---

### 1.3 Number Literals

**Raya:**
```ts
let x = 42;
let y = 3.14;
```

**Bytecode:**
```
CONST_I32 42
STORE_LOCAL 0

CONST_F64 3.14
STORE_LOCAL 1
```

**Explanation:**
- Small integers use `CONST_I32` for efficiency
- Floating-point values use `CONST_F64`
- Large constants may use `LOAD_CONST` with constant pool index

---

### 1.4 String Literals

**Raya:**
```ts
let name = "Alice";
```

**Bytecode:**
```
CONST_STR 0    // index into string constant pool
STORE_LOCAL 0
```

**Explanation:** String constants stored in pool, referenced by index.

---

### 1.5 Template Strings

**Raya:**
```ts
let name = "World";
let greeting = `Hello, ${name}!`;
```

**Bytecode:**
```
CONST_STR 0        // "World"
STORE_LOCAL 0

CONST_STR 1        // "Hello, "
LOAD_LOCAL 0       // name
TO_STRING          // ensure string (NEW OPCODE)
SCONCAT
CONST_STR 2        // "!"
SCONCAT
STORE_LOCAL 1
```

**New Opcode:**
```
TO_STRING  // Pop value, push string representation
```

---

## 2. Variables

### 2.1 Let Declaration

**Raya:**
```ts
let x: number = 42;
x = 100;
```

**Bytecode:**
```
CONST_I32 42
STORE_LOCAL 0

CONST_I32 100
STORE_LOCAL 0
```

**Explanation:** Local variables map to stack slots, accessed by index.

---

### 2.2 Const Declaration

**Raya:**
```ts
const MAX = 100;
```

**Bytecode:**
```
CONST_I32 100
STORE_LOCAL 0
```

**Explanation:**
- `const` is enforced at compile-time
- Runtime representation same as `let`
- Compiler prevents reassignment

---

### 2.3 Global Variables

**Raya:**
```ts
let globalCounter = 0;

function increment(): void {
  globalCounter = globalCounter + 1;
}
```

**Bytecode:**
```
// Module initialization
CONST_I32 0
STORE_GLOBAL 0

// Function increment
LOAD_GLOBAL 0
CONST_I32 1
IADD
STORE_GLOBAL 0
RETURN_VOID
```

**New Opcodes:**
```
LOAD_GLOBAL <index>   // Load global variable
STORE_GLOBAL <index>  // Store global variable
```

---

## 3. Arithmetic Operations

### 3.1 Integer Arithmetic (Type-Inferred)

**Raya:**
```ts
let a: number = 10;
let b: number = 20;
let sum = a + b;
let diff = a - b;
let prod = a * b;
let quot = a / b;
let rem = a % b;
```

**Bytecode:**
```
CONST_I32 10
STORE_LOCAL 0

CONST_I32 20
STORE_LOCAL 1

LOAD_LOCAL 0
LOAD_LOCAL 1
IADD              // Use IADD for known integers
STORE_LOCAL 2

LOAD_LOCAL 0
LOAD_LOCAL 1
ISUB
STORE_LOCAL 3

LOAD_LOCAL 0
LOAD_LOCAL 1
IMUL
STORE_LOCAL 4

LOAD_LOCAL 0
LOAD_LOCAL 1
IDIV
STORE_LOCAL 5

LOAD_LOCAL 0
LOAD_LOCAL 1
IMOD
STORE_LOCAL 6
```

**Explanation:** Compiler emits typed opcodes based on static type information.

---

### 3.2 Float Arithmetic

**Raya:**
```ts
let x: number = 3.14;
let y: number = 2.0;
let result = x * y;
```

**Bytecode:**
```
CONST_F64 3.14
STORE_LOCAL 0

CONST_F64 2.0
STORE_LOCAL 1

LOAD_LOCAL 0
LOAD_LOCAL 1
FMUL
STORE_LOCAL 2
```

---

### 3.3 Float Arithmetic (number type)

**Raya:**
```ts
function add(a: number, b: number): number {
  return a + b;
}
```

**Bytecode:**
```
LOAD_LOCAL 0
LOAD_LOCAL 1
FADD           // number + number uses float addition
RETURN
```

**Explanation:** `number` is 64-bit floating point (f64), so `number + number` always uses `FADD`.

---

### 3.3a Mixed Arithmetic (int + number promotion)

**Raya:**
```ts
function addMixed(a: int, b: number): number {
  return a + b;
}
```

**Bytecode:**
```
LOAD_LOCAL 0
LOAD_LOCAL 1
FADD           // int promoted to number, uses float addition
RETURN
```

**Explanation:** When mixing `int` and `number` operands, the compiler promotes the `int` to `number` (f64) and emits `FADD`. The result type is `number`.

---

### 3.4 Negation

**Raya:**
```ts
let x = 42;
let y = -x;
```

**Bytecode:**
```
CONST_I32 42
STORE_LOCAL 0

LOAD_LOCAL 0
INEG
STORE_LOCAL 1
```

---

### 3.5 Increment/Decrement

**Raya:**
```ts
let count = 0;
count++;
count--;
```

**Bytecode:**
```
CONST_I32 0
STORE_LOCAL 0

LOAD_LOCAL 0
CONST_I32 1
IADD
STORE_LOCAL 0

LOAD_LOCAL 0
CONST_I32 1
ISUB
STORE_LOCAL 0
```

**Explanation:** `++` and `--` desugar to add/subtract operations.

---

## 4. Comparison Operations

### 4.1 Equality Comparisons

**Raya:**
```ts
let a = 10;
let b = 20;
let eq = a == b;
let neq = a != b;
let strict_eq = a === b;
```

**Bytecode:**
```
CONST_I32 10
STORE_LOCAL 0

CONST_I32 20
STORE_LOCAL 1

LOAD_LOCAL 0
LOAD_LOCAL 1
IEQ
STORE_LOCAL 2

LOAD_LOCAL 0
LOAD_LOCAL 1
INE
STORE_LOCAL 3

LOAD_LOCAL 0
LOAD_LOCAL 1
STRICT_EQ
STORE_LOCAL 4
```

---

### 4.2 Relational Comparisons

**Raya:**
```ts
let less = a < b;
let lessEq = a <= b;
let greater = a > b;
let greaterEq = a >= b;
```

**Bytecode:**
```
LOAD_LOCAL 0
LOAD_LOCAL 1
ILT
STORE_LOCAL 5

LOAD_LOCAL 0
LOAD_LOCAL 1
ILE
STORE_LOCAL 6

LOAD_LOCAL 0
LOAD_LOCAL 1
IGT
STORE_LOCAL 7

LOAD_LOCAL 0
LOAD_LOCAL 1
IGE
STORE_LOCAL 8
```

---

## 5. Logical Operations

### 5.1 Logical NOT

**Raya:**
```ts
let flag = true;
let notFlag = !flag;
```

**Bytecode:**
```
CONST_TRUE
STORE_LOCAL 0

LOAD_LOCAL 0
NOT
STORE_LOCAL 1
```

---

### 5.2 Logical AND (Short-Circuit)

**Raya:**
```ts
let result = a && b;
```

**Bytecode:**
```
LOAD_LOCAL 0
DUP
JMP_IF_FALSE skip_b
POP
LOAD_LOCAL 1
skip_b:
STORE_LOCAL 2
```

**Explanation:**
- Load `a`, duplicate it
- If false, jump to end (keep false value)
- Otherwise pop duplicate, load `b`
- Short-circuit evaluation via conditional jump

---

### 5.3 Logical OR (Short-Circuit)

**Raya:**
```ts
let result = a || b;
```

**Bytecode:**
```
LOAD_LOCAL 0
DUP
JMP_IF_TRUE skip_b
POP
LOAD_LOCAL 1
skip_b:
STORE_LOCAL 2
```

---

## 6. Control Flow

### 6.1 If Statement

**Raya:**
```ts
if (condition) {
  doSomething();
}
```

**Bytecode:**
```
LOAD_LOCAL 0       // condition
JMP_IF_FALSE end
CALL <doSomething>, 0
end:
```

---

### 6.2 If-Else Statement

**Raya:**
```ts
if (condition) {
  doA();
} else {
  doB();
}
```

**Bytecode:**
```
LOAD_LOCAL 0
JMP_IF_FALSE else_branch
CALL <doA>, 0
JMP end
else_branch:
CALL <doB>, 0
end:
```

---

### 6.3 Ternary Expression

**Raya:**
```ts
let result = condition ? valueA : valueB;
```

**Bytecode:**
```
LOAD_LOCAL 0       // condition
JMP_IF_FALSE false_branch
CONST_I32 10       // valueA
JMP end
false_branch:
CONST_I32 20       // valueB
end:
STORE_LOCAL 1
```

---

### 6.4 While Loop

**Raya:**
```ts
while (condition) {
  doWork();
}
```

**Bytecode:**
```
loop_start:
LOAD_LOCAL 0
JMP_IF_FALSE loop_end
CALL <doWork>, 0
JMP loop_start
loop_end:
```

---

### 6.5 Do-While Loop

**Raya:**
```ts
do {
  doWork();
} while (condition);
```

**Bytecode:**
```
loop_start:
CALL <doWork>, 0
LOAD_LOCAL 0
JMP_IF_TRUE loop_start
```

---

### 6.6 For Loop

**Raya:**
```ts
for (let i = 0; i < 10; i++) {
  doWork();
}
```

**Bytecode:**
```
CONST_I32 0
STORE_LOCAL 0         // i = 0

loop_start:
LOAD_LOCAL 0
CONST_I32 10
ILT
JMP_IF_FALSE loop_end

CALL <doWork>, 0

LOAD_LOCAL 0          // i++
CONST_I32 1
IADD
STORE_LOCAL 0

JMP loop_start
loop_end:
```

---

### 6.7 For-Of Loop (Array)

**Raya:**
```ts
for (const item of items) {
  process(item);
}
```

**Bytecode:**
```
CONST_I32 0
STORE_LOCAL 1         // index = 0

loop_start:
LOAD_LOCAL 1
LOAD_LOCAL 0          // items
ARRAY_LEN
ILT
JMP_IF_FALSE loop_end

LOAD_LOCAL 0          // items
LOAD_LOCAL 1          // index
LOAD_ELEM             // items[index]
STORE_LOCAL 2         // item

LOAD_LOCAL 2
CALL <process>, 1

LOAD_LOCAL 1          // index++
CONST_I32 1
IADD
STORE_LOCAL 1

JMP loop_start
loop_end:
```

---

### 6.8 Break Statement

**Raya:**
```ts
while (true) {
  if (done) break;
}
```

**Bytecode:**
```
loop_start:
CONST_TRUE
JMP_IF_FALSE loop_end

LOAD_LOCAL 0          // done
JMP_IF_TRUE loop_end  // break

JMP loop_start
loop_end:
```

---

### 6.9 Continue Statement

**Raya:**
```ts
for (let i = 0; i < 10; i++) {
  if (skip) continue;
  doWork();
}
```

**Bytecode:**
```
CONST_I32 0
STORE_LOCAL 0

loop_start:
LOAD_LOCAL 0
CONST_I32 10
ILT
JMP_IF_FALSE loop_end

LOAD_LOCAL 1          // skip
JMP_IF_TRUE loop_continue  // continue

CALL <doWork>, 0

loop_continue:
LOAD_LOCAL 0
CONST_I32 1
IADD
STORE_LOCAL 0
JMP loop_start

loop_end:
```

---

### 6.10 Switch Statement

**Raya:**
```ts
switch (value) {
  case 1:
    doA();
    break;
  case 2:
    doB();
    break;
  default:
    doDefault();
}
```

**Bytecode:**
```
LOAD_LOCAL 0          // value
DUP
CONST_I32 1
IEQ
JMP_IF_TRUE case_1

DUP
CONST_I32 2
IEQ
JMP_IF_TRUE case_2

JMP default_case

case_1:
POP                   // remove duplicate
CALL <doA>, 0
JMP switch_end

case_2:
POP
CALL <doB>, 0
JMP switch_end

default_case:
POP
CALL <doDefault>, 0

switch_end:
```

**Explanation:** Switch statements compile to a series of comparisons and jumps.

---

## 7. Functions

### 7.1 Function Declaration

**Raya:**
```ts
function add(a: number, b: number): number {
  return a + b;
}
```

**Bytecode (Function body):**
```
LOAD_LOCAL 0          // a
LOAD_LOCAL 1          // b
FADD                  // number + number uses float addition
RETURN
```

**Explanation:** Parameters stored in locals 0, 1, etc.

---

### 7.2 Function Call

**Raya:**
```ts
let result = add(10, 20);
```

**Bytecode:**
```
CONST_I32 10
CONST_I32 20
CALL <add>, 2         // call function with 2 args
STORE_LOCAL 0
```

---

### 7.3 Arrow Function

**Raya:**
```ts
const add = (a: number, b: number): number => a + b;
```

**Bytecode:**
```
// Arrow functions compile to same bytecode as regular functions
// The function object is stored in a local

LOAD_CONST <function_add>
STORE_LOCAL 0
```

---

### 7.4 Optional Parameters

**Raya:**
```ts
function greet(name: string, title?: string): void {
  if (title !== null) {
    logger.info(`Hello, ${title} ${name}`);
  } else {
    logger.info(`Hello, ${name}`);
  }
}
```

**Bytecode:**
```
LOAD_LOCAL 1          // title
CONST_NULL
STRICT_NE
JMP_IF_FALSE else_branch

// then branch
CONST_STR 0           // "Hello, "
LOAD_LOCAL 1
SCONCAT
CONST_STR 1           // " "
SCONCAT
LOAD_LOCAL 0
SCONCAT
CALL <logger.info>, 1
JMP end

else_branch:
CONST_STR 2           // "Hello, "
LOAD_LOCAL 0
SCONCAT
CALL <logger.info>, 1

end:
RETURN_VOID
```

---

### 7.5 Default Parameters

**Raya:**
```ts
function greet(name: string, greeting: string = "Hello"): void {
  logger.info(`${greeting}, ${name}`);
}
```

**Bytecode:**
```
// Default parameter initialization at function entry
LOAD_LOCAL 1          // greeting
CONST_NULL
STRICT_NE
JMP_IF_TRUE has_value

CONST_STR 0           // "Hello"
STORE_LOCAL 1

has_value:
// Function body
LOAD_LOCAL 1
CONST_STR 1           // ", "
SCONCAT
LOAD_LOCAL 0
SCONCAT
CALL <logger.info>, 1
RETURN_VOID
```

---

### 7.6 Rest Parameters

**Raya:**
```ts
function sum(...numbers: number[]): number {
  let total = 0;
  for (const n of numbers) {
    total = total + n;
  }
  return total;
}
```

**Bytecode:**
```
// Rest params collected into array before function entry
CONST_I32 0
STORE_LOCAL 1         // total

CONST_I32 0
STORE_LOCAL 2         // index

loop_start:
LOAD_LOCAL 2
LOAD_LOCAL 0          // numbers (rest param)
ARRAY_LEN
ILT
JMP_IF_FALSE loop_end

LOAD_LOCAL 1          // total
LOAD_LOCAL 0          // numbers
LOAD_LOCAL 2          // index
LOAD_ELEM             // numbers[index]
IADD
STORE_LOCAL 1

LOAD_LOCAL 2
CONST_I32 1
IADD
STORE_LOCAL 2

JMP loop_start

loop_end:
LOAD_LOCAL 1
RETURN
```

---

### 7.7 Closures

**Raya:**
```ts
function makeCounter(): () => number {
  let count = 0;
  return () => {
    count = count + 1;
    return count;
  };
}
```

**Bytecode:**
```
// makeCounter function
CONST_I32 0
STORE_LOCAL 0         // count

// Create closure capturing 'count'
MAKE_CLOSURE <inner_func>, 1
LOAD_LOCAL 0          // capture count
CLOSE_VAR 0
RETURN

// Inner function (accesses captured variables)
LOAD_CAPTURED 0       // load captured count
CONST_I32 1
IADD
STORE_CAPTURED 0      // store back
LOAD_CAPTURED 0
RETURN
```

**New Opcodes:**
```
MAKE_CLOSURE <funcIndex>, <captureCount>  // Create closure object
CLOSE_VAR <localIndex>                     // Capture local variable
LOAD_CAPTURED <index>                      // Load captured variable
STORE_CAPTURED <index>                     // Store captured variable
```

---

## 8. Classes

### 8.1 Class Declaration

**Raya:**
```ts
class Point {
  x: number;
  y: number;

  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }

  distance(): number {
    return math.sqrt(this.x * this.x + this.y * this.y);
  }
}
```

**Bytecode (Constructor):**
```
// Constructor allocates object and initializes fields
LOAD_LOCAL 0          // this (implicitly passed)
LOAD_LOCAL 1          // x parameter
STORE_FIELD 0         // this.x = x

LOAD_LOCAL 0          // this
LOAD_LOCAL 2          // y parameter
STORE_FIELD 1         // this.y = y

RETURN_VOID
```

**Bytecode (Method):**
```
// distance method
LOAD_LOCAL 0          // this
LOAD_FIELD 0          // this.x
LOAD_LOCAL 0
LOAD_FIELD 0
FMUL                  // number * number = number
LOAD_LOCAL 0
LOAD_FIELD 1          // this.y
LOAD_LOCAL 0
LOAD_FIELD 1
FMUL                  // number * number = number
FADD                  // number + number = number
CALL <Math.sqrt>, 1
RETURN
```

---

### 8.2 Object Instantiation

**Raya:**
```ts
let p = new Point(10, 20);
```

**Bytecode:**
```
NEW <Point>           // Allocate object of class Point
DUP                   // Duplicate for constructor call
CONST_I32 10
CONST_I32 20
CALL_CONSTRUCTOR <Point.constructor>, 2
STORE_LOCAL 0         // Object left on stack after constructor
```

**New Opcode:**
```
CALL_CONSTRUCTOR <ctorIndex>, <argCount>  // Call constructor on object
```

---

### 8.3 Field Access

**Raya:**
```ts
let x = p.x;
p.y = 30;
```

**Bytecode:**
```
LOAD_LOCAL 0          // p
LOAD_FIELD 0          // p.x (field offset 0)
STORE_LOCAL 1

LOAD_LOCAL 0          // p
CONST_I32 30
STORE_FIELD 1         // p.y = 30 (field offset 1)
```

**Explanation:** Field offsets determined at compile time from class layout.

---

### 8.4 Method Call

**Raya:**
```ts
let dist = p.distance();
```

**Bytecode:**
```
LOAD_LOCAL 0          // p (receiver)
CALL_METHOD <Point.distance>, 0
STORE_LOCAL 1
```

**Explanation:**
- `CALL_METHOD` does virtual dispatch via vtable
- Receiver (`this`) implicitly passed as first argument

---

### 8.5 Inheritance

**Raya:**
```ts
class Animal {
  constructor(public name: string) {}

  move(): void {
    logger.info(`${this.name} is moving`);
  }
}

class Dog extends Animal {
  constructor(name: string) {
    super(name);
  }

  bark(): void {
    logger.info("Woof!");
  }
}
```

**Bytecode (Dog constructor):**
```
LOAD_LOCAL 0          // this
LOAD_LOCAL 1          // name
CALL_SUPER <Animal.constructor>, 1
RETURN_VOID
```

**New Opcode:**
```
CALL_SUPER <superCtorIndex>, <argCount>  // Call parent constructor
```

---

### 8.6 Static Members

**Raya:**
```ts
class Math {
  static PI: number = 3.14159;

  static abs(x: number): number {
    return x < 0 ? -x : x;
  }
}

let pi = Math.PI;
let abs5 = Math.abs(-5);
```

**Bytecode:**
```
// Access static field
LOAD_STATIC <Math.PI>
STORE_LOCAL 0

// Call static method
CONST_I32 -5
CALL_STATIC <Math.abs>, 1
STORE_LOCAL 1
```

**New Opcodes:**
```
LOAD_STATIC <staticIndex>              // Load static field
STORE_STATIC <staticIndex>             // Store static field
CALL_STATIC <staticMethodIndex>, <argCount>  // Call static method
```

---

## 9. Arrays

### 9.1 Array Creation

**Raya:**
```ts
let arr: number[] = [1, 2, 3];
```

**Bytecode:**
```
CONST_I32 3
NEW_ARRAY <number>    // Create array of length 3

DUP
CONST_I32 0
CONST_I32 1
STORE_ELEM           // arr[0] = 1

DUP
CONST_I32 1
CONST_I32 2
STORE_ELEM           // arr[1] = 2

DUP
CONST_I32 2
CONST_I32 3
STORE_ELEM           // arr[2] = 3

STORE_LOCAL 0
```

---

### 9.2 Array Literal (Optimized)

**Raya:**
```ts
let arr = [1, 2, 3];
```

**Bytecode (Optimized):**
```
ARRAY_LITERAL <typeIndex>, 3
CONST_I32 1
CONST_I32 2
CONST_I32 3
INIT_ARRAY 3
STORE_LOCAL 0
```

**New Opcodes:**
```
ARRAY_LITERAL <typeIndex>, <length>  // Allocate array for literal
INIT_ARRAY <count>                    // Pop N values and store in array
```

---

### 9.3 Array Access

**Raya:**
```ts
let first = arr[0];
arr[1] = 42;
```

**Bytecode:**
```
LOAD_LOCAL 0          // arr
CONST_I32 0
LOAD_ELEM
STORE_LOCAL 1

LOAD_LOCAL 0          // arr
CONST_I32 1
CONST_I32 42
STORE_ELEM
```

---

### 9.4 Array Length

**Raya:**
```ts
let len = arr.length;
```

**Bytecode:**
```
LOAD_LOCAL 0
ARRAY_LEN
STORE_LOCAL 1
```

---

### 9.5 Array Methods

**Raya:**
```ts
arr.push(4);
let last = arr.pop();
```

**Bytecode:**
```
LOAD_LOCAL 0          // arr
CONST_I32 4
CALL_METHOD <Array.push>, 1

LOAD_LOCAL 0          // arr
CALL_METHOD <Array.pop>, 0
STORE_LOCAL 1
```

**Explanation:** Array methods are builtin methods called via `CALL_METHOD`.

---

## 10. Tuples

### 10.1 Tuple Creation

**Raya:**
```ts
let pair: [number, string] = [42, "answer"];
```

**Bytecode:**
```
TUPLE_LITERAL <typeIndex>, 2
CONST_I32 42
CONST_STR 0           // "answer"
INIT_TUPLE 2
STORE_LOCAL 0
```

**New Opcodes:**
```
TUPLE_LITERAL <typeIndex>, <length>  // Allocate tuple
INIT_TUPLE <count>                    // Pop N values into tuple
```

---

### 10.2 Tuple Access

**Raya:**
```ts
let num = pair[0];
let str = pair[1];
```

**Bytecode:**
```
LOAD_LOCAL 0
CONST_I32 0
TUPLE_GET             // Specialized tuple access
STORE_LOCAL 1

LOAD_LOCAL 0
CONST_I32 1
TUPLE_GET
STORE_LOCAL 2
```

**New Opcode:**
```
TUPLE_GET  // Pop index, pop tuple, push element
```

**Explanation:** Tuples use specialized opcodes for bounds-checked access.

---

### 10.3 Tuple Destructuring

**Raya:**
```ts
let [a, b] = pair;
```

**Bytecode:**
```
LOAD_LOCAL 0          // pair
CONST_I32 0
TUPLE_GET
STORE_LOCAL 1         // a

LOAD_LOCAL 0
CONST_I32 1
TUPLE_GET
STORE_LOCAL 2         // b
```

---

## 11. Object Literals

### 11.1 Object Literal

**Raya:**
```ts
let obj = { x: 10, y: 20 };
```

**Bytecode:**
```
OBJECT_LITERAL <typeIndex>, 2  // Anonymous object with 2 fields
CONST_I32 10
CONST_I32 20
INIT_OBJECT 2
STORE_LOCAL 0
```

**New Opcodes:**
```
OBJECT_LITERAL <typeIndex>, <fieldCount>
INIT_OBJECT <count>  // Pop N values and initialize fields
```

---

### 11.2 Property Access

**Raya:**
```ts
let x = obj.x;
obj.y = 30;
```

**Bytecode:**
```
LOAD_LOCAL 0
LOAD_FIELD 0          // x is field 0
STORE_LOCAL 1

LOAD_LOCAL 0
CONST_I32 30
STORE_FIELD 1         // y is field 1
```

---

### 11.3 Computed Property Access

**Raya:**
```ts
let key = "x";
let value = obj[key];  // Not supported in v0.5
```

**Explanation:** Raya v0.5 does not support computed property access. Use `Map<K, V>` instead.

---

## 12. Concurrency

### 12.1 Async Function Declaration

**Raya:**
```ts
async function compute(): Task<number> {
  return 42;
}
```

**Bytecode (Function marked as async):**
```
CONST_I32 42
RETURN
```

**Explanation:**
- Async functions have metadata marking them as async
- Calling async function uses `SPAWN` instead of `CALL`

---

### 12.2 Calling Async Function

**Raya:**
```ts
let task = compute();
```

**Bytecode:**
```
SPAWN <compute>, 0    // Start Task, returns TaskHandle
STORE_LOCAL 0
```

**Explanation:** `SPAWN` creates new Task and returns handle immediately.

---

### 12.3 Await Expression

**Raya:**
```ts
let result = await task;
```

**Bytecode:**
```
LOAD_LOCAL 0          // task
AWAIT                 // Suspend current Task, resume when complete
STORE_LOCAL 1
```

**Explanation:**
- `AWAIT` suspends current Task
- VM scheduler runs other Tasks
- Resumes when awaited Task completes
- Result pushed on stack

---

### 12.4 Concurrent Execution

**Raya:**
```ts
async function main(): Task<void> {
  let task1 = fetchData(1);
  let task2 = fetchData(2);
  let result1 = await task1;
  let result2 = await task2;
}
```

**Bytecode:**
```
// Start both Tasks concurrently
CONST_I32 1
SPAWN <fetchData>, 1
STORE_LOCAL 0         // task1

CONST_I32 2
SPAWN <fetchData>, 1
STORE_LOCAL 1         // task2

// Wait for completion
LOAD_LOCAL 0
AWAIT
STORE_LOCAL 2         // result1

LOAD_LOCAL 1
AWAIT
STORE_LOCAL 3         // result2

RETURN_VOID
```

---

### 12.5 Task Cancellation

**Raya:**
```ts
task.cancel();
```

**Bytecode:**
```
LOAD_LOCAL 0          // task
CALL_METHOD <Task.cancel>, 0
```

**Explanation:** Cancellation is a method call on Task object.

---

## 13. Synchronization

### 13.1 Mutex Creation

**Raya:**
```ts
const mu = new Mutex();
```

**Bytecode:**
```
NEW_MUTEX
STORE_LOCAL 0
```

---

### 13.2 Mutex Lock/Unlock

**Raya:**
```ts
mu.lock();
counter = counter + 1;
mu.unlock();
```

**Bytecode:**
```
LOAD_LOCAL 0          // mu
MUTEX_LOCK

LOAD_GLOBAL 0         // counter
CONST_I32 1
IADD
STORE_GLOBAL 0

LOAD_LOCAL 0          // mu
MUTEX_UNLOCK
```

**Explanation:**
- `MUTEX_LOCK` may block Task if mutex is held
- VM enforces no `AWAIT` between lock/unlock

---

### 13.3 Critical Section Pattern

**Raya:**
```ts
mu.lock();
try {
  // critical section
  sharedData.modify();
} finally {
  mu.unlock();
}
```

**Bytecode:**
```
LOAD_LOCAL 0
MUTEX_LOCK

// Critical section
LOAD_GLOBAL 0
CALL_METHOD <modify>, 0

// Finally block (always executed)
LOAD_LOCAL 0
MUTEX_UNLOCK

// Note: Exception handling TBD in future version
```

---

## 14. Modules

### 14.1 Named Export

**Raya:**
```ts
export function add(a: number, b: number): number {
  return a + b;
}

export const PI = 3.14159;
```

**Bytecode:**
```
// Function definition as normal
// PI initialization as normal

// Export table metadata (not bytecode)
// Marks 'add' and 'PI' as exported symbols
```

**Explanation:**
- Exports handled at module linking time
- No runtime opcodes for export
- Module metadata includes export table

---

### 14.2 Named Import

**Raya:**
```ts
import { add, PI } from "./math";
```

**Bytecode:**
```
// No runtime bytecode
// Module loader resolves imports at load time
// Imported symbols accessible via LOAD_GLOBAL
```

**Explanation:**
- Imports resolved statically at module load
- Imported names mapped to global indices

---

### 14.3 Namespace Import

**Raya:**
```ts
import * as Math from "./math";
let sum = Math.add(1, 2);
```

**Bytecode:**
```
// Create module object
LOAD_MODULE <Math>
STORE_LOCAL 0

// Access via module object
LOAD_LOCAL 0
LOAD_FIELD <add_offset>
CONST_I32 1
CONST_I32 2
CALL 2
STORE_LOCAL 1
```

**New Opcode:**
```
LOAD_MODULE <moduleIndex>  // Load module namespace object
```

---

## 15. Type Operations (Discriminated Unions)

**Note:** Raya does **not** support `typeof` or `instanceof`. Use discriminated unions instead.

### 15.1 Discriminated Union Pattern

**Raya:**
```ts
type Value =
  | { kind: "string"; value: string }
  | { kind: "number"; value: number };

function process(v: Value): void {
  switch (v.kind) {
    case "string":
      logger.info(v.value.toUpperCase());
      break;
    case "number":
      logger.info(v.value.toFixed(2));
      break;
  }
}
```

**Bytecode:**
```
// Load discriminant field
LOAD_LOCAL 0          // v
LOAD_FIELD 0          // v.kind

// Compare with "string"
DUP
CONST_STR 0           // "string"
STRICT_EQ
JMP_IF_TRUE string_case

// Compare with "number"
CONST_STR 1           // "number"
STRICT_EQ
JMP_IF_TRUE number_case

JMP end               // Should never reach (exhaustiveness)

string_case:
POP                   // remove duplicate
LOAD_LOCAL 0
LOAD_FIELD 1          // v.value (string)
CALL_METHOD <String.toUpperCase>, 0
CALL <logger.info>, 1
JMP end

number_case:
POP
LOAD_LOCAL 0
LOAD_FIELD 1          // v.value (number)
CONST_I32 2
CALL_METHOD <Number.toFixed>, 1
CALL <logger.info>, 1

end:
RETURN_VOID
```

**Explanation:** Discriminant field checked at compile time. No runtime type introspection needed.

---

### 15.2 Type Predicate Pattern

**Raya:**
```ts
type Animal =
  | { kind: "dog"; bark(): void }
  | { kind: "cat"; meow(): void };

function isDog(animal: Animal): animal is { kind: "dog"; bark(): void } {
  return animal.kind === "dog";
}

function handle(animal: Animal): void {
  if (isDog(animal)) {
    animal.bark();
  } else {
    animal.meow();
  }
}
```

**Bytecode:**
```
// isDog function
LOAD_LOCAL 0          // animal
LOAD_FIELD 0          // animal.kind
CONST_STR 0           // "dog"
STRICT_EQ
RETURN

// handle function
LOAD_LOCAL 0
CALL <isDog>, 1
JMP_IF_FALSE else_branch

LOAD_LOCAL 0
CALL_METHOD <bark>, 0
JMP end

else_branch:
LOAD_LOCAL 0
CALL_METHOD <meow>, 0

end:
RETURN_VOID
```

---

### 15.3 Exhaustiveness Checking

**Raya:**
```ts
type Action =
  | { type: "increment" }
  | { type: "decrement" }
  | { type: "reset" };

function reducer(state: number, action: Action): number {
  switch (action.type) {
    case "increment":
      return state + 1;
    case "decrement":
      return state - 1;
    case "reset":
      return 0;
  }
}
```

**Bytecode:**
```
LOAD_LOCAL 1          // action
LOAD_FIELD 0          // action.type

DUP
CONST_STR 0           // "increment"
STRICT_EQ
JMP_IF_TRUE case_increment

DUP
CONST_STR 1           // "decrement"
STRICT_EQ
JMP_IF_TRUE case_decrement

CONST_STR 2           // "reset"
STRICT_EQ
JMP_IF_TRUE case_reset

// Should never reach (compiler verifies exhaustiveness)
TRAP <unreachable>

case_increment:
POP
LOAD_LOCAL 0
CONST_I32 1
IADD
RETURN

case_decrement:
POP
LOAD_LOCAL 0
CONST_I32 1
ISUB
RETURN

case_reset:
POP
CONST_I32 0
RETURN
```

**Explanation:** Compiler ensures all discriminant values are handled. Runtime trap if unreachable code is executed.

---

### 15.4 Bare Primitive Union Pattern

**Raya:**
```ts
type ID = string | number;

let id: ID = 42;
id = "abc";

function printId(value: ID): void {
  // Access methods - compiler unwraps automatically
  logger.info(value.toString());
}
```

**Bytecode:**

```
// let id: ID = 42;
// Create union wrapper object
NEW <Union_string_number>      // Allocate union object
DUP
CONST_STR 0                    // "number" - $type field
STORE_FIELD 0                  // Store discriminant
CONST_I32 42                   // 42 - $value field
STORE_FIELD 1                  // Store value
STORE_LOCAL 0                  // id

// id = "abc";
// Re-assign with different type
NEW <Union_string_number>
DUP
CONST_STR 1                    // "string" - $type field
STORE_FIELD 0
CONST_STR 2                    // "abc" - $value field
STORE_FIELD 1
STORE_LOCAL 0                  // id

// printId(id)
// Method call - compiler unwraps
LOAD_LOCAL 0                   // id
LOAD_FIELD 1                   // id.$value (unwrap)
CALL_METHOD <toString>, 0      // Call method on unwrapped value
CALL <logger.info>, 1
RETURN_VOID
```

**Object Layout:**

```
Union_string_number {
  field[0]: $type (string discriminant)
  field[1]: $value (any type - runtime value)
}
```

**Explanation:**
- Bare unions are boxed objects at runtime
- Each assignment creates a new wrapper object
- Compiler automatically inserts boxing/unboxing code
- Method calls are unwrapped transparently
- Memory overhead: ~16 bytes per value (2 fields)

---

### 15.5 Bare Union with match()

**Raya:**
```ts
import { match } from "raya:std";

type ID = string | number;
const id: ID = 42;

const desc = match(id, {
  string: (s) => `String: ${s}`,
  number: (n) => `Number: ${n}`
});
```

**Bytecode (Inlined match):**

```
// Compiler inlines match() for performance
LOAD_LOCAL 0                   // id
LOAD_FIELD 0                   // id.$type (discriminant)

// Check if "string"
DUP
CONST_STR 0                    // "string"
STRICT_EQ
JMP_IF_TRUE string_handler

// Check if "number"
CONST_STR 1                    // "number"
STRICT_EQ
JMP_IF_TRUE number_handler

// Unreachable (exhaustiveness guaranteed)
TRAP <unreachable>

string_handler:
POP                            // Remove duplicate
LOAD_LOCAL 0                   // id
LOAD_FIELD 1                   // id.$value (unwrap string)
// Inline handler: s => `String: ${s}`
CONST_STR 2                    // "String: "
SWAP
TO_STRING
SCONCAT
JMP end

number_handler:
POP
LOAD_LOCAL 0                   // id
LOAD_FIELD 1                   // id.$value (unwrap number)
// Inline handler: n => `Number: ${n}`
CONST_STR 3                    // "Number: "
SWAP
TO_STRING
SCONCAT

end:
STORE_LOCAL 1                  // desc
```

**Explanation:**
- `match()` is inlined by the compiler for zero-cost abstraction
- Discriminant check uses string comparison on `$type` field
- Each handler receives the unwrapped `$value`
- Exhaustiveness checked at compile time
- No function call overhead (fully inlined)

---

### 15.6 match() with Discriminated Unions

**Raya:**
```ts
import { match } from "raya:std";

type Result<T> =
  | { status: "ok"; value: T }
  | { status: "error"; error: string };

const result: Result<number> = { status: "ok", value: 42 };

const message = match(result, {
  ok: (r) => `Success: ${r.value}`,
  error: (r) => `Error: ${r.error}`
});
```

**Bytecode (Inlined match):**

```
// Compiler inlines and infers discriminant field "status"
LOAD_LOCAL 0                   // result
LOAD_FIELD 0                   // result.status (inferred discriminant)

// Check if "ok"
DUP
CONST_STR 0                    // "ok"
STRICT_EQ
JMP_IF_TRUE ok_handler

// Check if "error"
CONST_STR 1                    // "error"
STRICT_EQ
JMP_IF_TRUE error_handler

// Unreachable (exhaustiveness guaranteed)
TRAP <unreachable>

ok_handler:
POP                            // Remove duplicate
LOAD_LOCAL 0                   // result (full object)
// Inline handler: r => `Success: ${r.value}`
CONST_STR 2                    // "Success: "
LOAD_LOCAL 0
LOAD_FIELD 1                   // r.value
TO_STRING
SCONCAT
JMP end

error_handler:
POP
LOAD_LOCAL 0                   // result (full object)
// Inline handler: r => `Error: ${r.error}`
CONST_STR 3                    // "Error: "
LOAD_LOCAL 0
LOAD_FIELD 2                   // r.error (assuming status is field 0, value is 1, error is 2)
SCONCAT

end:
STORE_LOCAL 1                  // message
```

**Explanation:**
- Compiler infers "status" as discriminant (first common literal-typed field)
- No boxing/unboxing needed (discriminated unions are plain objects)
- Handler receives the full variant object
- Fully inlined for zero overhead
- Field offsets resolved at compile time

---

## 16. String Operations

### 16.1 String Concatenation

**Raya:**
```ts
let full = first + " " + last;
```

**Bytecode:**
```
LOAD_LOCAL 0          // first
CONST_STR 0           // " "
SCONCAT
LOAD_LOCAL 1          // last
SCONCAT
STORE_LOCAL 2
```

---

### 16.2 String Methods

**Raya:**
```ts
let upper = name.toUpperCase();
let index = name.indexOf("test");
```

**Bytecode:**
```
LOAD_LOCAL 0
CALL_METHOD <String.toUpperCase>, 0
STORE_LOCAL 1

LOAD_LOCAL 0
CONST_STR 1           // "test"
CALL_METHOD <String.indexOf>, 1
STORE_LOCAL 2
```

---

### 16.3 String Length

**Raya:**
```ts
let len = str.length;
```

**Bytecode:**
```
LOAD_LOCAL 0
CALL_METHOD <String.length>, 0  // length is a property, but accessed as method
STORE_LOCAL 1
```

**Alternative (if length is a field):**
```
LOAD_LOCAL 0
LOAD_FIELD <length_offset>
STORE_LOCAL 1
```

---

## 17. Generics & Monomorphization

**Raya uses monomorphization** — generic code is specialized for each concrete type at compile time.

### 17.1 Generic Function Monomorphization

**Raya:**
```ts
function identity<T>(x: T): T {
  return x;
}

let a = identity(42);
let b = identity("hello");
let c = identity(true);
```

**Compiler generates specialized functions:**
```
// identity_number (monomorphized for number)
LOAD_LOCAL 0
RETURN

// identity_string (monomorphized for string)
LOAD_LOCAL 0
RETURN

// identity_boolean (monomorphized for boolean)
LOAD_LOCAL 0
RETURN
```

**Call sites:**
```
// let a = identity(42);
CONST_I32 42
CALL <identity_number>, 1
STORE_LOCAL 0

// let b = identity("hello");
CONST_STR 0
CALL <identity_string>, 1
STORE_LOCAL 1

// let c = identity(true);
CONST_TRUE
CALL <identity_boolean>, 1
STORE_LOCAL 2
```

**Explanation:** Each call site uses a specialized version. No generic dispatch at runtime.

---

### 17.2 Generic Class Monomorphization

**Raya:**
```ts
class Box<T> {
  constructor(public value: T) {}

  get(): T {
    return this.value;
  }
}

let numBox = new Box(42);
let strBox = new Box("hello");
```

**Compiler generates specialized classes:**
```
// Box_number class
class Box_number {
  value: number;

  constructor(value: number) {
    this.value = value;
  }

  get(): number {
    return this.value;
  }
}

// Box_string class
class Box_string {
  value: string;

  constructor(value: string) {
    this.value = value;
  }

  get(): string {
    return this.value;
  }
}
```

**Bytecode:**
```
// let numBox = new Box(42);
NEW <Box_number>
DUP
CONST_I32 42
CALL_CONSTRUCTOR <Box_number.constructor>, 1
STORE_LOCAL 0

// let strBox = new Box("hello");
NEW <Box_string>
DUP
CONST_STR 0
CALL_CONSTRUCTOR <Box_string.constructor>, 1
STORE_LOCAL 1
```

**Explanation:** Each generic instantiation creates a distinct class. No type parameters at runtime.

---

### 17.3 Monomorphization Benefits

1. **Zero runtime overhead** — Direct calls, no dispatch
2. **Type-specific optimization** — Each variant optimized for its type
3. **Better inlining** — Compiler can inline specialized code
4. **Smaller runtime** — No generic dispatch mechanism needed
5. **Type safety** — Impossible to call with wrong type

---

## 18. Special Constructs

### 18.1 Null Coalescing

**Raya:**
```ts
let display = name ?? "Anonymous";
```

**Bytecode:**
```
LOAD_LOCAL 0          // name
DUP
CONST_NULL
STRICT_NE
JMP_IF_TRUE use_name

POP
CONST_STR 0           // "Anonymous"

use_name:
STORE_LOCAL 1
```

---

### 18.2 Optional Chaining

**Raya:**
```ts
let city = user?.address?.city;
```

**Bytecode:**
```
LOAD_LOCAL 0          // user
DUP
CONST_NULL
STRICT_EQ
JMP_IF_TRUE result_null

LOAD_FIELD <address_offset>
DUP
CONST_NULL
STRICT_EQ
JMP_IF_TRUE result_null

LOAD_FIELD <city_offset>
JMP store_result

result_null:
POP
CONST_NULL

store_result:
STORE_LOCAL 1
```

**Alternative with dedicated opcode:**
```
LOAD_LOCAL 0
OPTIONAL_FIELD <address_offset>
OPTIONAL_FIELD <city_offset>
STORE_LOCAL 1
```

**New Opcode:**
```
OPTIONAL_FIELD <offset>  // Pop object, push field or null if object is null
```

---

### 18.3 Destructuring Assignment

**Raya:**
```ts
let [a, b] = [1, 2];
let { x, y } = point;
```

**Bytecode (Array destructuring):**
```
CONST_I32 1
CONST_I32 2
ARRAY_LITERAL <number>, 2
INIT_ARRAY 2

DUP
CONST_I32 0
LOAD_ELEM
STORE_LOCAL 0         // a

CONST_I32 1
LOAD_ELEM
STORE_LOCAL 1         // b
```

**Bytecode (Object destructuring):**
```
LOAD_LOCAL 0          // point
DUP
LOAD_FIELD 0          // x
STORE_LOCAL 1

LOAD_FIELD 1          // y
STORE_LOCAL 2
```

---

## 18. Optimization Examples

### 18.1 Constant Folding

**Raya:**
```ts
let x = 2 + 3;
```

**Unoptimized:**
```
CONST_I32 2
CONST_I32 3
IADD
STORE_LOCAL 0
```

**Optimized:**
```
CONST_I32 5
STORE_LOCAL 0
```

---

### 18.2 Dead Code Elimination

**Raya:**
```ts
if (false) {
  unreachableCode();
}
```

**Unoptimized:**
```
CONST_FALSE
JMP_IF_FALSE end
CALL <unreachableCode>, 0
end:
```

**Optimized:**
```
// Entire block eliminated
```

---

### 18.3 Inlining

**Raya:**
```ts
function square(x: number): number {
  return x * x;
}

let result = square(5);
```

**Unoptimized:**
```
CONST_I32 5
CALL <square>, 1
STORE_LOCAL 0
```

**Optimized (inlined):**
```
CONST_I32 5
DUP
IMUL
STORE_LOCAL 0
```

---

## Summary of Opcodes

### New Opcodes

```
TO_STRING                                // Convert value to string
LOAD_GLOBAL <index>                      // Load global variable
STORE_GLOBAL <index>                     // Store global variable
MAKE_CLOSURE <funcIndex>, <captureCount> // Create closure
CLOSE_VAR <localIndex>                   // Capture variable
LOAD_CAPTURED <index>                    // Load captured var
STORE_CAPTURED <index>                   // Store captured var
CALL_CONSTRUCTOR <ctorIndex>, <argCount> // Call constructor
CALL_SUPER <superCtorIndex>, <argCount>  // Call parent constructor
LOAD_STATIC <staticIndex>                // Load static field
STORE_STATIC <staticIndex>               // Store static field
CALL_STATIC <methodIndex>, <argCount>    // Call static method
ARRAY_LITERAL <typeIndex>, <length>      // Create array literal
INIT_ARRAY <count>                        // Initialize array
TUPLE_LITERAL <typeIndex>, <length>      // Create tuple
INIT_TUPLE <count>                        // Initialize tuple
TUPLE_GET                                 // Get tuple element
OBJECT_LITERAL <typeIndex>, <fieldCount> // Create object literal
INIT_OBJECT <count>                       // Initialize object
LOAD_MODULE <moduleIndex>                 // Load module namespace
OPTIONAL_FIELD <offset>                   // Optional chaining field access
```

### Removed Opcodes

**All runtime type checking opcodes are removed:**

```
TYPEOF       // REMOVED - use discriminated unions instead
INSTANCEOF   // REMOVED - use discriminated unions instead
CHECK_TYPE   // REMOVED - compiler verifies types
CAST         // REMOVED - compiler verifies type narrowing
```

**Rationale:**

Raya is a **fully statically typed language** with **zero runtime type checks**:

1. **All types known at compile time** — Compiler determines every value's type
2. **Type erasure** — No type tags or RTTI at runtime
3. **Monomorphization** — Generics specialized to concrete types during compilation
4. **Value-based discrimination** — Variants identified by discriminant field values (strings), not type tags
5. **Compiler verification** — Type safety guaranteed before execution

**The VM trusts the compiler** — If bytecode is generated, it's type-safe by construction.

---

## Compiler Optimizations

### Type-Specialized Opcodes

The compiler emits specialized opcodes based on static type information:

| Operation | Integer (`int`) | Float (`number`/`float`) | String |
|-----------|-----------------|--------------------------|--------|
| Add       | `IADD`          | `FADD`                   | `SCONCAT` |
| Subtract  | `ISUB`          | `FSUB`                   | - |
| Multiply  | `IMUL`          | `FMUL`                   | - |
| Divide    | `IDIV`          | `FDIV`                   | - |
| Modulo    | `IMOD`          | -                        | - |
| Negate    | `INEG`          | `FNEG`                   | - |
| Equal     | `IEQ`           | `FEQ`                    | `SEQ` |
| Less Than | `ILT`           | `FLT`                    | `SLT` |

**Mixed operands (`int + number`):** The `int` operand is promoted to `number` (f64) and the `F*` opcode is used.

**Benefits:**
- Avoids runtime type checking
- Enables unboxed arithmetic
- Better performance

---

## 17. Exception Handling

### 17.1 Basic Try-Catch

**Raya:**
```ts
try {
  riskyOperation();
} catch (e) {
  logger.info("Error: " + e);
}
```

**Bytecode:**
```
TRY catch_offset=8 finally_offset=-1    // Install handler
CALL riskyOperation                     // Protected code
END_TRY                                 // Remove handler
JMP end_offset                          // Skip catch block

// catch block (offset 8):
STORE_LOCAL 0                           // Store exception in local 0
CONST_STR "Error: "
LOAD_LOCAL 0
SCONCAT
CALL logger.info
// end:
```

**Explanation:**
- TRY installs handler with catch at offset 8, no finally (-1)
- Protected code executes normally
- If exception thrown, jumps to offset 8
- Exception value automatically on stack for catch
- END_TRY removes handler when block completes

---

### 17.2 Try-Finally

**Raya:**
```ts
try {
  performWork();
} finally {
  cleanup();
}
```

**Bytecode:**
```
TRY catch_offset=-1 finally_offset=12   // Install handler (no catch)
CALL performWork                        // Protected code
END_TRY                                 // Remove handler
CALL cleanup                            // Finally block
JMP end_offset                          // Done

// finally block (offset 12):
CALL cleanup                            // Finally cleanup
END_TRY                                 // Remove handler
RETHROW                                 // Re-raise if exception
// end:
```

**Explanation:**
- TRY with no catch (-1), finally at offset 12
- If exception thrown, jumps to finally
- Finally always executes (normal or exception path)
- RETHROW continues exception propagation

---

### 17.3 Try-Catch-Finally

**Raya:**
```ts
try {
  riskyOperation();
} catch (e) {
  handleError(e);
} finally {
  cleanup();
}
```

**Bytecode:**
```
TRY catch_offset=8 finally_offset=16    // Install handler
CALL riskyOperation                     // Protected code
END_TRY                                 // Remove handler
CALL cleanup                            // Finally (normal path)
JMP end_offset                          // Skip catch

// catch block (offset 8):
STORE_LOCAL 0                           // Store exception
LOAD_LOCAL 0
CALL handleError                        // Handle error
CALL cleanup                            // Finally (catch path)
JMP end_offset

// finally block (offset 16):
CALL cleanup                            // Finally (exception path)
RETHROW                                 // Re-raise
// end:
```

**Explanation:**
- TRY with both catch and finally offsets
- Normal path: code → finally → end
- Exception path: code → catch → finally → end
- Uncaught exception: code → finally → rethrow

---

### 17.4 Nested Try-Catch

**Raya:**
```ts
try {
  try {
    innerOperation();
  } catch (inner) {
    handleInner(inner);
  }
} catch (outer) {
  handleOuter(outer);
}
```

**Bytecode:**
```
TRY catch_offset=20 finally_offset=-1   // Outer try
TRY catch_offset=8 finally_offset=-1    // Inner try
CALL innerOperation
END_TRY                                 // Inner end
JMP outer_end                           // Skip inner catch

// inner catch (offset 8):
STORE_LOCAL 0
LOAD_LOCAL 0
CALL handleInner
END_TRY                                 // Outer end
JMP end                                 // Skip outer catch

// outer catch (offset 20):
STORE_LOCAL 1
LOAD_LOCAL 1
CALL handleOuter
// end:
```

**Explanation:**
- Handler stack maintains LIFO order
- Inner exception caught by inner handler
- If inner doesn't catch, outer handler tries
- END_TRY pops from handler stack

---

### 17.5 Rethrow Pattern

**Raya:**
```ts
try {
  operation();
} catch (e) {
  log(e);
  throw e;  // Re-raise
}
```

**Bytecode:**
```
TRY catch_offset=8 finally_offset=-1
CALL operation
END_TRY
JMP end

// catch block (offset 8):
STORE_LOCAL 0                           // Store exception
LOAD_LOCAL 0
CALL log                                // Log it
LOAD_LOCAL 0                            // Load exception
THROW                                   // Throw it (unwinds to next handler)
// end:
```

**Explanation:**
- Catch block can rethrow with THROW
- RETHROW opcode also available (for implicit rethrow)
- Exception propagates to next handler in stack

---

### 17.6 Exception with Mutex (Auto-Unlock)

**Raya:**
```ts
const mtx = new Mutex();
mtx.lock();
try {
  await operation();
} finally {
  mtx.unlock();
}
```

**Bytecode:**
```
NEW_MUTEX
STORE_LOCAL 0                           // Store mutex
LOAD_LOCAL 0
MUTEX_LOCK                              // Lock mutex
TRY catch_offset=-1 finally_offset=12   // Install handler
LOAD_LOCAL 0
AWAIT operation                         // Protected code
END_TRY
LOAD_LOCAL 0
MUTEX_UNLOCK                            // Finally unlock
JMP end

// finally (offset 12):
LOAD_LOCAL 0
MUTEX_UNLOCK                            // Unlock on exception
RETHROW                                 // Re-raise
// end:
```

**Explanation:**
- Mutex tracked by Task
- If exception thrown between LOCK/UNLOCK, auto-unlocks
- Finally block ensures explicit unlock
- VM tracks mutexes per Task for auto-unlock during unwinding

---

### 17.7 Stack Unwinding

When exception thrown:

1. **Find handler:** Search exception_handlers stack (LIFO)
2. **Execute finally:** If handler has finally_offset, execute it
3. **Unwind stack:** Pop operand stack to handler's stack_size
4. **Unwind frames:** Pop call frames to handler's frame_count
5. **Unlock mutexes:** Unlock all mutexes acquired since handler
6. **Jump to catch:** If handler has catch_offset, jump there
7. **Continue unwinding:** If no catch, pop handler and repeat

**Stack state during unwinding:**
```
Handler installed:
  - stack_size = 5
  - frame_count = 2
  - mutex_count = 1

Exception thrown:
  - Current stack_size = 12
  - Current frame_count = 4
  - Current mutex_count = 3

Unwinding:
  - Pop stack: 12 → 5 (restore to handler)
  - Pop frames: 4 → 2 (return to handler's frame)
  - Unlock mutexes: 3 → 1 (unlock 2 mutexes)
  - Jump to catch or finally
```

---

## Bytecode Verification

Before execution, the VM verifies:

1. **Type stack consistency** - Each instruction's input/output types match
2. **Stack depth** - No underflow or overflow
3. **Jump targets** - All jumps target valid instruction boundaries
4. **Local indices** - All local variable accesses are in bounds
5. **Field offsets** - Field accesses match class layout
6. **Mutex pairing** - No await between lock/unlock (static analysis)
7. **Constant pool** - All constant indices are valid

---

**End of Language to Bytecode Mapping**

This document provides a complete reference for translating Raya language features to VM bytecode. Implementers can use this as a guide for both compiler development and VM implementation.
