# Raya Standard Library API Specification

This document defines the complete API for Raya's standard library modules.

---

## Table of Contents

1. [Core Types](#1-core-types)
2. [raya:std - Standard Utilities](#2-rayastd---standard-utilities)
3. [raya:json - JSON Support](#3-rayajson---json-support)
4. [raya:json/internal - Internal JSON Utilities](#4-rayajsoninternal---internal-json-utilities)
5. [raya:reflect - Reflection API](#5-rayareflect---reflection-api-optional)
6. [std:logger - Logging Module](#6-stdlogger---logging-module)
7. [std:vm - VM Operations Module](#7-stdvm---vm-operations-module)
8. [Built-in Types](#8-built-in-types)

---

## 1. Core Types

### Error

```ts
class Error {
  message: string;
  stack?: string;

  constructor(message: string);
}
```

**Description:** Standard error type used throughout the standard library.

**Fields:**
- `message`: Human-readable error description
- `stack`: Optional stack trace (implementation-defined format)

---

### Result<T, E>

```ts
type Result<T, E> =
  | { status: "ok"; value: T }
  | { status: "error"; error: E };
```

**Description:** Standard result type for operations that may fail.

**Usage:**
```ts
function divide(a: number, b: number): Result<number, Error> {
  if (b === 0) {
    return { status: "error", error: new Error("Division by zero") };
  }
  return { status: "ok", value: a / b };
}
```

---

### Task<T>

```ts
interface Task<T> extends PromiseLike<T> {
  // No additional methods - tasks are awaitable
}
```

**Description:** Represents a concurrent task (green thread). Created by `async` functions.

**Note:** Tasks start immediately when created (no lazy evaluation).

---

## 2. raya:std - Standard Utilities

```ts
module "raya:std" {
  // Pattern matching utility
  export function match<T, R>(
    value: T,
    handlers: MatchHandlers<T, R>
  ): R;

  // Task utilities
  export async function sleep(ms: number): Task<void>;
  export async function all<T>(tasks: Task<T>[]): Task<T[]>;
  export async function race<T>(tasks: Task<T>[]): Task<T>;
}
```

### match()

**Type Signature:**
```ts
function match<T, R>(
  value: T,
  handlers: MatchHandlers<T, R>
): R;
```

**Description:** Pattern matching for union types.

**For bare primitive unions:**
```ts
type ID = string | number;
const id: ID = 42;

match(id, {
  string: (s) => logger.info(`String: ${s}`),
  number: (n) => logger.info(`Number: ${n}`)
});
```

**For discriminated unions:**
```ts
type Result<T> =
  | { status: "ok"; value: T }
  | { status: "error"; error: string };

const msg = match(result, {
  ok: (r) => `Success: ${r.value}`,
  error: (r) => `Error: ${r.error}`
});
```

**Features:**
- Type-safe with full inference
- Exhaustiveness checking at compile time
- Zero-cost abstraction (inlined by compiler)
- Expression form (returns a value)

---

### sleep()

**Type Signature:**
```ts
async function sleep(ms: number): Task<void>;
```

**Description:** Suspends the current task for the specified duration.

**Parameters:**
- `ms`: Duration in milliseconds

**Example:**
```ts
async function delayed(): Task<void> {
  logger.info("Starting...");
  await sleep(1000);  // Wait 1 second
  logger.info("Done!");
}
```

**Implementation:** Does not block OS thread, only suspends the current Task.

---

### all()

**Type Signature:**
```ts
async function all<T>(tasks: Task<T>[]): Task<T[]>;
```

**Description:** Waits for all tasks to complete and returns their results in order.

**Parameters:**
- `tasks`: Array of tasks to wait for

**Returns:** Array of results in the same order as input tasks

**Behavior:**
- If any task throws an error, `all()` throws that error
- All tasks run concurrently
- Results are collected in original order

**Example:**
```ts
const tasks = [fetchUser(1), fetchUser(2), fetchUser(3)];
const users = await all(tasks);
```

---

### race()

**Type Signature:**
```ts
async function race<T>(tasks: Task<T>[]): Task<T>;
```

**Description:** Returns the result of the first task to complete.

**Parameters:**
- `tasks`: Array of tasks to race

**Returns:** Result of the first completed task

**Behavior:**
- Other tasks continue running (no cancellation)
- If the first task throws, `race()` throws

**Example:**
```ts
const result = await race([
  fetchFromPrimary(),
  fetchFromBackup()
]);
```

---

## 3. raya:json - JSON Support

```ts
module "raya:json" {
  export class JSON {
    static encode<T>(value: T): Result<string, Error>;
    static decode<T>(input: string): Result<T, Error>;
  }
}
```

### JSON.encode()

**Type Signature:**
```ts
static encode<T>(value: T): Result<string, Error>;
```

**Description:** Converts a Raya value to JSON string using compile-time code generation.

**Parameters:**
- `value`: Value to encode (must be a type known at compile time)

**Returns:** `Result` with JSON string or error

**Supported Types:**
- Primitives: `number`, `string`, `boolean`, `null`
- Objects: interfaces and classes with public fields
- Arrays: `T[]`
- Unions: Discriminated unions only (bare primitive unions supported)
- Optional: `T | null`

**Example:**
```ts
import { JSON } from "raya:json";

interface User {
  name: string;
  age: number;
  email: string | null;
}

const user: User = { name: "Alice", age: 30, email: null };
const result = JSON.encode(user);

if (result.status === "ok") {
  logger.info(result.value);  // {"name":"Alice","age":30,"email":null}
}
```

**Code Generation:** Compiler generates specialized encoder for each type used with `JSON.encode()`.

---

### JSON.decode()

**Type Signature:**
```ts
static decode<T>(input: string): Result<T, Error>;
```

**Description:** Parses JSON string and validates against type `T` using compile-time code generation.

**Parameters:**
- `input`: JSON string to parse

**Returns:** `Result` with parsed value or validation error

**Validation:**
- Type structure must match exactly
- Unknown fields are ignored
- Missing required fields cause errors
- Type mismatches cause errors

**Example:**
```ts
const jsonString = '{"name":"Bob","age":25,"email":"bob@example.com"}';
const result = JSON.decode<User>(jsonString);

if (result.status === "ok") {
  const user = result.value;
  logger.info(user.name);  // "Bob"
} else {
  logger.error(result.error.message);
}
```

**Code Generation:** Compiler generates specialized decoder for each type used with `JSON.decode<T>()`.

---

## 4. raya:json/internal - Internal JSON Utilities

```ts
module "raya:json/internal" {
  export type JsonValue =
    | { kind: "null" }
    | { kind: "boolean"; value: boolean }
    | { kind: "number"; value: number }
    | { kind: "string"; value: string }
    | { kind: "array"; value: JsonValue[] }
    | { kind: "object"; value: Map<string, JsonValue> };

  export function parseJson(input: string): Result<JsonValue, Error>;
}
```

### JsonValue

**Description:** Discriminated union representing parsed JSON structure.

**Variants:**
- `{ kind: "null" }` - JSON null
- `{ kind: "boolean"; value: boolean }` - JSON boolean
- `{ kind: "number"; value: number }` - JSON number
- `{ kind: "string"; value: string }` - JSON string
- `{ kind: "array"; value: JsonValue[] }` - JSON array
- `{ kind: "object"; value: Map<string, JsonValue> }` - JSON object

**Usage:** For manual JSON decoding when dealing with dynamic/unknown structures.

---

### parseJson()

**Type Signature:**
```ts
function parseJson(input: string): Result<JsonValue, Error>;
```

**Description:** Parses JSON into structural representation without type validation.

**Parameters:**
- `input`: JSON string to parse

**Returns:** `Result` with parsed `JsonValue` tree or parse error

**Example:**
```ts
import { parseJson, JsonValue } from "raya:json/internal";

const result = parseJson('{"id": 123}');
if (result.status === "ok") {
  const json = result.value;
  if (json.kind === "object") {
    const id = json.value.get("id");
    if (id && id.kind === "number") {
      logger.info(id.value);  // 123
    }
  }
}
```

**Use Case:** Manual decoders for third-party APIs without discriminants.

---

## 5. raya:reflect - Reflection API

**Note:** Reflection metadata is always included in compiled modules.

```ts
module "raya:reflect" {
  // Type information
  export interface TypeInfo {
    readonly kind: "primitive" | "class" | "interface" | "union" | "array" | "tuple";
    readonly name: string;
    readonly properties?: PropertyInfo[];
    readonly methods?: MethodInfo[];
    readonly constructors?: ConstructorInfo[];
  }

  export interface PropertyInfo {
    readonly name: string;
    readonly type: TypeInfo;
    readonly isReadonly: boolean;
  }

  export interface MethodInfo {
    readonly name: string;
    readonly parameters: ParameterInfo[];
    readonly returnType: TypeInfo;
  }

  export interface ParameterInfo {
    readonly name: string;
    readonly type: TypeInfo;
  }

  export interface ConstructorInfo {
    readonly parameters: ParameterInfo[];
  }

  // Reflection functions
  export function typeOf(value: any): TypeInfo;
  export function typeInfo<T>(): TypeInfo;
  export function instanceof(value: any, type: TypeInfo): boolean;
  export function getProperties(value: object): PropertyInfo[];
  export function getProperty(value: object, name: string): any;
  export function setProperty(value: object, name: string, val: any): void;
  export function hasProperty(value: object, name: string): boolean;
  export function construct(type: TypeInfo, args: any[]): any;
}
```

**See:** [LANG.md Section 18](LANG.md#18-optional-reflection-system) for detailed API documentation and examples.

**Performance:** Reflection calls have runtime overhead. Recommended for development builds and interop layers only.

---

## 6. std:logger - Logging Module

> **Note:** Console API has been replaced with `std:logger`. See [milestone-4.2.md](../plans/milestone-4.2.md) for the logger API.

```ts
import logger from "std:logger";

logger.info("Server started");
logger.error("Connection failed");
logger.warn("Deprecated API");
logger.debug("Request payload:", data);
```

---

## 7. std:runtime - Runtime Operations Module

> Compile, execute, load/save bytecode, spawn isolated VMs, manage permissions, and introspect the runtime. Uses **named exports** with five separate classes. See [milestone-4.5.md](../plans/milestone-4.5.md) for full implementation plan.

```ts
import { Compiler, Bytecode, Vm, Parser, TypeChecker } from "std:runtime";
```

### Compiler — Compile & Execute

```ts
Compiler.compile(source: string): number;           // Parse + type-check + compile, returns module ID
Compiler.compileExpression(expr: string): number;    // Compile a single expression
Compiler.compileAst(astId: number): number;          // Compile a pre-parsed AST
Compiler.eval(source: string): number;               // Compile and immediately execute
Compiler.execute(moduleId: number): number;          // Execute a compiled module's main function
Compiler.executeFunction(moduleId: number, funcName: string, ...args: number[]): number;
```

### Bytecode — Binary I/O & Dependencies

```ts
Bytecode.encode(moduleId: number): Buffer;           // Serialize module to .ryb binary
Bytecode.decode(data: Buffer): number;               // Deserialize .ryb binary to module
Bytecode.validate(moduleId: number): boolean;        // Verify module integrity
Bytecode.disassemble(moduleId: number): string;      // Human-readable bytecode listing
Bytecode.getModuleName(moduleId: number): string;
Bytecode.getModuleFunctions(moduleId: number): string[];
Bytecode.getModuleClasses(moduleId: number): string[];
Bytecode.loadLibrary(path: string): number;          // Load .ryb file from path
Bytecode.loadDependency(path: string, name: string): number;  // Load + register as importable
Bytecode.resolveDependency(name: string): number;    // Auto-resolve from search paths
```

**Dependency model:** By default, dependencies are bundled into the `.ryb` at compile time. When bundling isn't possible, use `resolveDependency` (auto-search: `./deps/` → `./lib/` → `<entry_dir>/deps/` → `~/.raya/libs/`) or `loadDependency` (explicit path) at runtime.

### Parser & TypeChecker — Advanced Pipeline Access

```ts
Parser.parse(source: string): number;               // Parse source to AST, returns AST ID
Parser.parseExpression(expr: string): number;        // Parse a single expression

TypeChecker.check(astId: number): number;            // Type-check AST, returns typed AST ID
TypeChecker.checkExpression(astId: number): number;  // Type-check expression AST
```

### Vm — Instances & Isolation

```ts
// Spawn isolated child VM
let child: VmInstance = Vm.spawn({
    maxHeap: 64 * 1024 * 1024,
    maxConcurrency: 4,
    maxResource: 0.25,
    priority: 5,
    timeout: 5000,
    permissions: { allowStdlib: ["std:math"], allowReflect: false }
});

// Load and run bytecode in child VM (from INNER_VM.md)
child.loadBytecode(bytes);
let task = child.runEntry("main");
let result: number = await task;
child.terminate();

// Current VM introspection (permission-gated)
let current: VmInstance = Vm.current();
```

**Isolation:** Each child VM gets a separate heap, globals, and module registry. Child failures never affect the parent. Resource limits and permissions are strictly bounded by the parent (recursive for nested VMs).

### Vm — Permissions & Introspection

```ts
Vm.hasPermission(name: string): boolean;
Vm.getPermissions(): VmPermissions;
Vm.heapUsed(): number;
Vm.heapLimit(): number;
Vm.taskCount(): number;
Vm.version(): string;
Vm.uptime(): number;
Vm.loadedModules(): string[];
```

**Permission names:** `"reflect"`, `"vmAccess"`, `"vmSpawn"`, `"libLoad"`, `"nativeCalls"`, `"eval"`, `"binaryIO"`, `"stdlib:<name>"`

---

## 8. Built-in Types

### String Methods

```ts
interface String {
  readonly length: number;

  // Case conversion
  toUpperCase(): string;
  toLowerCase(): string;

  // Searching
  indexOf(search: string, start?: number): number;
  lastIndexOf(search: string, start?: number): number;
  includes(search: string): boolean;
  startsWith(search: string): boolean;
  endsWith(search: string): boolean;

  // Extraction
  substring(start: number, end?: number): string;
  slice(start: number, end?: number): string;
  charAt(index: number): string;

  // Transformation
  trim(): string;
  trimStart(): string;
  trimEnd(): string;
  repeat(count: number): string;
  padStart(length: number, fill?: string): string;
  padEnd(length: number, fill?: string): string;

  // Splitting/Joining
  split(separator: string): string[];

  // Other
  toString(): string;
}
```

---

### Number Methods

```ts
interface Number {
  // Formatting
  toString(radix?: number): string;
  toFixed(digits: number): string;
  toExponential(digits?: number): string;
  toPrecision(precision: number): string;
}

// Math namespace
const Math: {
  // Constants
  readonly PI: number;
  readonly E: number;

  // Basic
  abs(x: number): number;
  sign(x: number): number;

  // Rounding
  floor(x: number): number;
  ceil(x: number): number;
  round(x: number): number;
  trunc(x: number): number;

  // Min/Max
  min(...values: number[]): number;
  max(...values: number[]): number;

  // Power/Root
  pow(base: number, exponent: number): number;
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
  random(): number;  // Returns [0, 1)
};
```

---

### Array Methods

```ts
interface Array<T> {
  readonly length: number;

  // Element access
  at(index: number): T | undefined;

  // Mutation
  push(item: T): void;
  pop(): T | undefined;
  shift(): T | undefined;
  unshift(item: T): void;
  splice(start: number, deleteCount?: number, ...items: T[]): T[];

  // Iteration
  forEach(fn: (item: T, index: number) => void): void;
  map<U>(fn: (item: T, index: number) => U): U[];
  filter(fn: (item: T, index: number) => boolean): T[];
  reduce<U>(fn: (acc: U, item: T, index: number) => U, initial: U): U;

  // Searching
  find(fn: (item: T, index: number) => boolean): T | undefined;
  findIndex(fn: (item: T, index: number) => boolean): number;
  indexOf(item: T): number;
  lastIndexOf(item: T): number;
  includes(item: T): boolean;

  // Testing
  some(fn: (item: T, index: number) => boolean): boolean;
  every(fn: (item: T, index: number) => boolean): boolean;

  // Transformation
  slice(start?: number, end?: number): T[];
  concat(...arrays: T[][]): T[];
  reverse(): T[];
  sort(compare?: (a: T, b: T) => number): T[];

  // Joining
  join(separator?: string): string;
}
```

---

### Map<K, V>

```ts
class Map<K, V> {
  constructor();

  // Size
  readonly size: number;

  // Access
  get(key: K): V | undefined;
  set(key: K, value: V): void;
  has(key: K): boolean;
  delete(key: K): boolean;
  clear(): void;

  // Iteration
  keys(): K[];
  values(): V[];
  entries(): [K, V][];
  forEach(fn: (value: V, key: K) => void): void;
}
```

---

### Set<T>

```ts
class Set<T> {
  constructor();

  // Size
  readonly size: number;

  // Access
  add(value: T): void;
  has(value: T): boolean;
  delete(value: T): boolean;
  clear(): void;

  // Iteration
  values(): T[];
  forEach(fn: (value: T) => void): void;
}
```

---

### Mutex

```ts
class Mutex {
  constructor();

  lock(): void;
  unlock(): void;
}
```

**Description:** Mutual exclusion lock for protecting shared data in concurrent tasks.

**Rules:**
- Must `unlock()` in the same Task that called `lock()`
- Cannot `await` while holding a lock (compile error)
- Undefined behavior if unlocked by wrong Task

**Example:**
```ts
const mu = new Mutex();
let counter = 0;

async function increment(): Task<void> {
  mu.lock();
  counter++;
  mu.unlock();
}
```

---

### ArrayBuffer

```ts
class ArrayBuffer {
  constructor(byteLength: number);

  readonly byteLength: number;

  slice(start?: number, end?: number): ArrayBuffer;
}
```

**Description:** Fixed-size binary buffer.

---

## Notes on Implementation

1. **Code Generation:** `JSON.encode()` and `JSON.decode()` use compile-time code generation, not reflection.

2. **Zero-Cost Abstractions:** `match()`, `all()`, and `race()` are inlined by the compiler where possible.

3. **Task Utilities:** All async functions in the standard library follow the same concurrency model as user code.

4. **Error Handling:** Most standard library functions return `Result<T, Error>` instead of throwing exceptions.

5. **Immutability:** String methods return new strings. Array mutation methods modify in place.

6. **Module Resolution:** All standard library modules use the `raya:` prefix and are resolved before user modules.

---

**Version:** v0.6 (Specification)

**Status:** This specification is complete but subject to minor refinements based on implementation experience.
