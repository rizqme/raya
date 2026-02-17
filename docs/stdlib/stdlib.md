---
title: "Standard Library"
---

# Raya Standard Library API Specification

> **Status:** Implemented (16 modules: 8 core + 8 system)
> **Related:** [Language Spec](../language/lang.md), [Native ABI](../native/abi.md), [Modules](../runtime/modules.md)

This document defines the complete API for Raya's standard library modules.

---

## Table of Contents

### Core
1. [Core Types](#1-core-types)
2. [raya:std - Standard Utilities](#2-rayastd---standard-utilities)
3. [raya:json - JSON Support](#3-rayajson---json-support)
4. [raya:json/internal - Internal JSON Utilities](#4-rayajsoninternal---internal-json-utilities)
5. [raya:reflect - Reflection API](#5-rayareflect---reflection-api-optional)
6. [std:logger - Logging Module](#6-stdlogger---logging-module)
7. [std:runtime - Runtime Operations Module](#7-stdruntime---runtime-operations-module)
8. [Built-in Types](#8-built-in-types)
9. [std:math](#9-stdmath---math-operations-module-)
10. [std:crypto](#10-stdcrypto---cryptographic-operations-module-)
11. [std:time](#11-stdtime---time-and-duration-module-)
12. [std:path](#12-stdpath---path-manipulation-module-)

### System (POSIX)
13. [std:fs - Filesystem](#13-stdfs---filesystem-module)
14. [std:net - TCP/UDP Sockets](#14-stdnet---tcpudp-sockets-module)
15. [std:http - HTTP Server](#15-stdhttp---http-server-module)
16. [std:fetch - HTTP Client](#16-stdfetch---http-client-module)
17. [std:env - Environment Variables](#17-stdenv---environment-variables-module)
18. [std:process - Process Management](#18-stdprocess---process-management-module)
19. [std:os - OS Information](#19-stdos---os-information-module)
20. [std:io - Standard I/O](#20-stdio---standard-io-module)

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

**See:** [LANG.md Section 18](../language/lang.md#18-optional-reflection-system) for detailed API documentation and examples.

**Performance:** Reflection calls have runtime overhead. Recommended for development builds and interop layers only.

---

## 6. std:logger - Logging Module

> **Note:** Console API has been replaced with `std:logger`. See [milestone-4.2.md](https://github.com/rizqme/raya/blob/main/plans/milestone-4.2.md) for the logger API.

```ts
import logger from "std:logger";

logger.info("Server started");
logger.error("Connection failed");
logger.warn("Deprecated API");
logger.debug("Request payload:", data);
```

---

## 7. std:runtime - Runtime Operations Module

> Compile, execute, load/save bytecode, spawn isolated VMs, manage permissions, and introspect the runtime. Uses **named exports** with five separate classes. See [milestone-4.5.md](https://github.com/rizqme/raya/blob/main/plans/milestone-4.5.md) for full implementation plan.

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

## 9. std:math - Math Operations Module ✅

```ts
import math from "std:math";
```

**Class:** `Math` (singleton, default export)

| Method | Signature | Description |
|--------|-----------|-------------|
| `PI` | `(): number` | Pi constant (~3.14159) |
| `E` | `(): number` | Euler's number (~2.71828) |
| `abs` | `(x: number): number` | Absolute value |
| `sign` | `(x: number): number` | Sign (-1, 0, 1) |
| `floor` | `(x: number): number` | Round down |
| `ceil` | `(x: number): number` | Round up |
| `round` | `(x: number): number` | Round to nearest |
| `trunc` | `(x: number): number` | Truncate decimal |
| `min` | `(a: number, b: number): number` | Minimum |
| `max` | `(a: number, b: number): number` | Maximum |
| `pow` | `(base: number, exp: number): number` | Power |
| `sqrt` | `(x: number): number` | Square root |
| `sin` | `(x: number): number` | Sine |
| `cos` | `(x: number): number` | Cosine |
| `tan` | `(x: number): number` | Tangent |
| `asin` | `(x: number): number` | Arc sine |
| `acos` | `(x: number): number` | Arc cosine |
| `atan` | `(x: number): number` | Arc tangent |
| `atan2` | `(y: number, x: number): number` | 2-argument arc tangent |
| `exp` | `(x: number): number` | e^x |
| `log` | `(x: number): number` | Natural logarithm |
| `log10` | `(x: number): number` | Base-10 logarithm |
| `random` | `(): number` | Random number [0, 1) |

**Native IDs:** 0x2000-0x2016

---

## 10. std:crypto - Cryptographic Operations Module ✅

```ts
import crypto from "std:crypto";
```

**Class:** `Crypto` (singleton, default export)

| Method | Signature | Description |
|--------|-----------|-------------|
| `hash` | `(algorithm: string, data: string): string` | Hash string → hex digest |
| `hashBytes` | `(algorithm: string, data: Buffer): Buffer` | Hash bytes → raw digest |
| `hmac` | `(algorithm: string, key: string, data: string): string` | HMAC → hex |
| `hmacBytes` | `(algorithm: string, key: Buffer, data: Buffer): Buffer` | HMAC → raw |
| `randomBytes` | `(size: number): Buffer` | Cryptographic random bytes |
| `randomInt` | `(min: number, max: number): number` | Random integer in range |
| `randomUUID` | `(): string` | UUID v4 |
| `toHex` | `(data: Buffer): string` | Buffer → hex string |
| `fromHex` | `(hex: string): Buffer` | Hex string → buffer |
| `toBase64` | `(data: Buffer): string` | Buffer → base64 string |
| `fromBase64` | `(b64: string): Buffer` | Base64 string → buffer |
| `timingSafeEqual` | `(a: Buffer, b: Buffer): boolean` | Constant-time comparison |

**Supported algorithms:** SHA-256, SHA-384, SHA-512, SHA-1, MD5 (hash); SHA-256, SHA-384, SHA-512 (HMAC)

**Native IDs:** 0x4000-0x400B

---

## 11. std:time - Time and Duration Module ✅

```ts
import time from "std:time";
```

**Class:** `Time` (singleton, default export)

| Method | Signature | Native? | Description |
|--------|-----------|---------|-------------|
| `now` | `(): number` | Yes | Wall clock time (ms since epoch) |
| `monotonic` | `(): number` | Yes | Monotonic clock (ms since VM start) |
| `hrtime` | `(): number` | Yes | High-resolution nanosecond clock |
| `elapsed` | `(start: number): number` | Pure Raya | Duration since start (ms) |
| `sleep` | `(ms: number): void` | Yes | Sleep for milliseconds |
| `sleepMicros` | `(us: number): void` | Yes | Sleep for microseconds |
| `seconds` | `(n: number): number` | Pure Raya | Convert seconds → ms |
| `minutes` | `(n: number): number` | Pure Raya | Convert minutes → ms |
| `hours` | `(n: number): number` | Pure Raya | Convert hours → ms |
| `toSeconds` | `(ms: number): number` | Pure Raya | Convert ms → seconds |
| `toMinutes` | `(ms: number): number` | Pure Raya | Convert ms → minutes |
| `toHours` | `(ms: number): number` | Pure Raya | Convert ms → hours |

**Native IDs:** 0x5000-0x5004 (5 native calls; 7 methods are pure Raya)

---

## 12. std:path - Path Manipulation Module ✅

```ts
import path from "std:path";
```

**Class:** `Path` (singleton, default export)

| Method | Signature | Description |
|--------|-----------|-------------|
| `join` | `(a: string, b: string): string` | Join path components |
| `normalize` | `(p: string): string` | Normalize path (resolve `.`, `..`) |
| `dirname` | `(p: string): string` | Parent directory |
| `basename` | `(p: string): string` | File name component |
| `extname` | `(p: string): string` | File extension (including `.`) |
| `isAbsolute` | `(p: string): boolean` | Check if path is absolute |
| `resolve` | `(base: string, target: string): string` | Resolve relative to base |
| `relative` | `(base: string, target: string): string` | Relative path between two paths |
| `cwd` | `(): string` | Current working directory |
| `sep` | `(): string` | OS path separator (`/` or `\`) |
| `delimiter` | `(): string` | OS path delimiter (`:` or `;`) |
| `stripExt` | `(p: string): string` | Remove file extension |
| `withExt` | `(p: string, ext: string): string` | Replace file extension |
| `isRelative` | `(p: string): boolean` | Check if path is relative (pure Raya) |

**Native IDs:** 0x6000-0x600C

---

---

## System Modules (`raya-stdlib-posix`)

All system I/O is synchronous. Async is achieved at the call site via goroutines: `async fs.readFile(path)`.

---

## 13. std:fs - Filesystem Module

```ts
import fs from "std:fs";
```

**Class:** `Fs` (singleton, default export)

| Method | Signature | Description |
|--------|-----------|-------------|
| `readFile` | `(path: string): Buffer` | Read file as binary |
| `readTextFile` | `(path: string): string` | Read file as UTF-8 text |
| `writeFile` | `(path: string, data: Buffer): void` | Write binary to file |
| `writeTextFile` | `(path: string, data: string): void` | Write text to file |
| `appendFile` | `(path: string, data: string): void` | Append text to file |
| `exists` | `(path: string): boolean` | Check if path exists |
| `isFile` | `(path: string): boolean` | Check if path is a file |
| `isDir` | `(path: string): boolean` | Check if path is a directory |
| `isSymlink` | `(path: string): boolean` | Check if path is a symlink |
| `fileSize` | `(path: string): number` | File size in bytes |
| `lastModified` | `(path: string): number` | Last modified time (ms since epoch) |
| `stat` | `(path: string): number[]` | Packed stat: [size, isFile, isDir, isSymlink, modifiedMs, createdMs, mode] |
| `mkdir` | `(path: string): void` | Create directory |
| `mkdirRecursive` | `(path: string): void` | Create directory tree |
| `readDir` | `(path: string): string[]` | List directory entries |
| `rmdir` | `(path: string): void` | Remove empty directory |
| `remove` | `(path: string): void` | Remove file |
| `rename` | `(from: string, to: string): void` | Rename/move file |
| `copy` | `(from: string, to: string): void` | Copy file |
| `chmod` | `(path: string, mode: number): void` | Change permissions |
| `symlink` | `(target: string, path: string): void` | Create symbolic link |
| `readlink` | `(path: string): string` | Read symlink target |
| `realpath` | `(path: string): string` | Resolve to absolute path |
| `tempDir` | `(): string` | OS temp directory |
| `tempFile` | `(prefix: string): string` | Create temp file, return path |

---

## 14. std:net - TCP/UDP Sockets Module

```ts
import { TcpListener, TcpStream, UdpSocket } from "std:net";
import net from "std:net";
```

### Net (singleton, default export)

| Method | Signature | Description |
|--------|-----------|-------------|
| `listen` | `(host: string, port: number): TcpListener` | Bind TCP listener |
| `connect` | `(host: string, port: number): TcpStream` | Connect TCP stream |
| `bindUdp` | `(host: string, port: number): UdpSocket` | Bind UDP socket |

### TcpListener

| Method | Signature | Description |
|--------|-----------|-------------|
| `accept` | `(): TcpStream` | Accept connection (blocking) |
| `close` | `(): void` | Close listener |
| `localAddr` | `(): string` | Local address (host:port) |

### TcpStream

| Method | Signature | Description |
|--------|-----------|-------------|
| `read` | `(size: number): Buffer` | Read up to N bytes |
| `readAll` | `(): Buffer` | Read until EOF |
| `readLine` | `(): string` | Read line (until \n) |
| `write` | `(data: Buffer): number` | Write bytes, return count |
| `writeText` | `(data: string): number` | Write string, return bytes |
| `close` | `(): void` | Close stream |
| `remoteAddr` | `(): string` | Remote address |
| `localAddr` | `(): string` | Local address |

### UdpSocket

| Method | Signature | Description |
|--------|-----------|-------------|
| `sendTo` | `(data: Buffer, addr: string): number` | Send to address |
| `sendText` | `(data: string, addr: string): number` | Send string to address |
| `receive` | `(size: number): Buffer` | Receive up to N bytes |
| `close` | `(): void` | Close socket |
| `localAddr` | `(): string` | Local address |

---

## 15. std:http - HTTP Server Module

```ts
import { HttpServer, HttpRequest } from "std:http";
```

### HttpServer

| Method | Signature | Description |
|--------|-----------|-------------|
| `constructor` | `(host: string, port: number)` | Create and bind server |
| `accept` | `(): HttpRequest` | Accept next request (blocking) |
| `respond` | `(reqHandle: number, status: number, body: string): void` | Send text response |
| `respondBytes` | `(reqHandle: number, status: number, body: Buffer): void` | Send binary response |
| `respondWithHeaders` | `(reqHandle: number, status: number, headers: string[], body: string): void` | Send response with custom headers |
| `close` | `(): void` | Close server |
| `localAddr` | `(): string` | Server address |

### HttpRequest

| Method | Signature | Description |
|--------|-----------|-------------|
| `method` | `(): string` | HTTP method (GET, POST, etc.) |
| `path` | `(): string` | Request path |
| `query` | `(): string` | Query string |
| `header` | `(name: string): string` | Get header value |
| `headers` | `(): string[]` | All headers as [key, value, ...] pairs |
| `body` | `(): string` | Request body as text |
| `bodyBytes` | `(): Buffer` | Request body as bytes |

---

## 16. std:fetch - HTTP Client Module

```ts
import fetch from "std:fetch";
import { Response } from "std:fetch";
```

### Fetch (singleton, default export)

| Method | Signature | Description |
|--------|-----------|-------------|
| `get` | `(url: string): Response` | HTTP GET |
| `post` | `(url: string, body: string): Response` | HTTP POST |
| `put` | `(url: string, body: string): Response` | HTTP PUT |
| `delete` | `(url: string): Response` | HTTP DELETE |
| `request` | `(method: string, url: string, body: string, headers: string): Response` | Custom request |

### Response

| Method | Signature | Description |
|--------|-----------|-------------|
| `status` | `(): number` | HTTP status code |
| `statusText` | `(): string` | Status text (e.g., "OK") |
| `header` | `(name: string): string` | Get response header |
| `headers` | `(): string[]` | All headers as [key, value, ...] pairs |
| `text` | `(): string` | Response body as text |
| `bytes` | `(): Buffer` | Response body as bytes |

---

## 17. std:env - Environment Variables Module

```ts
import env from "std:env";
```

**Class:** `Env` (singleton, default export)

| Method | Signature | Description |
|--------|-----------|-------------|
| `get` | `(key: string): string` | Get env var (empty string if unset) |
| `set` | `(key: string, value: string): void` | Set env var |
| `remove` | `(key: string): void` | Remove env var |
| `has` | `(key: string): boolean` | Check if env var exists |
| `all` | `(): string[]` | All vars as [key, value, ...] pairs |
| `cwd` | `(): string` | Current working directory |
| `home` | `(): string` | Home directory |

---

## 18. std:process - Process Management Module

```ts
import process from "std:process";
```

**Class:** `Process` (singleton, default export)

| Method | Signature | Description |
|--------|-----------|-------------|
| `exit` | `(code: number): void` | Exit process |
| `pid` | `(): number` | Current process ID |
| `argv` | `(): string[]` | Command-line arguments |
| `execPath` | `(): string` | Path to executable |
| `exec` | `(command: string): number` | Execute shell command, return handle |
| `execGetCode` | `(handle: number): number` | Get exit code from exec handle |
| `execGetStdout` | `(handle: number): string` | Get stdout from exec handle |
| `execGetStderr` | `(handle: number): string` | Get stderr from exec handle |
| `execRelease` | `(handle: number): void` | Release exec handle |
| `run` | `(command: string): number` | Execute and return exit code (pure Raya convenience) |

---

## 19. std:os - OS Information Module

```ts
import os from "std:os";
```

**Class:** `Os` (singleton, default export)

| Method | Signature | Description |
|--------|-----------|-------------|
| `platform` | `(): string` | OS name (darwin, linux, windows) |
| `arch` | `(): string` | CPU architecture (x86_64, aarch64) |
| `hostname` | `(): string` | Machine hostname |
| `cpus` | `(): number` | Number of logical CPUs |
| `totalMemory` | `(): number` | Total RAM in bytes |
| `freeMemory` | `(): number` | Available RAM in bytes |
| `uptime` | `(): number` | System uptime in seconds |
| `eol` | `(): string` | Line ending (\n or \r\n) |
| `tmpdir` | `(): string` | OS temp directory |

---

## 20. std:io - Standard I/O Module

```ts
import io from "std:io";
```

**Class:** `Io` (singleton, default export)

| Method | Signature | Description |
|--------|-----------|-------------|
| `readLine` | `(): string` | Read line from stdin |
| `readAll` | `(): string` | Read all stdin |
| `write` | `(data: string): void` | Write to stdout |
| `writeln` | `(data: string): void` | Write line to stdout |
| `writeErr` | `(data: string): void` | Write to stderr |
| `writeErrln` | `(data: string): void` | Write line to stderr |
| `flush` | `(): void` | Flush stdout |

---

## Notes on Implementation

1. **Code Generation:** `JSON.encode()` and `JSON.decode()` use compile-time code generation, not reflection.

2. **Zero-Cost Abstractions:** `match()`, `all()`, and `race()` are inlined by the compiler where possible.

3. **Task Utilities:** All async functions in the standard library follow the same concurrency model as user code.

4. **Error Handling:** Most standard library functions return `Result<T, Error>` instead of throwing exceptions.

5. **Immutability:** String methods return new strings. Array mutation methods modify in place.

6. **Module Resolution:** All standard library modules use the `std:` prefix and are resolved before user modules.

7. **System I/O Model:** All POSIX modules use synchronous I/O. Async is achieved at the call site using Raya's goroutine model: `async fs.readFile(path)` spawns a Task that runs the blocking read on a worker thread.

8. **Handle-Based Resources:** Stateful resources (sockets, HTTP connections, process results) use numeric handles. Handles are stored in thread-safe registries and must be explicitly closed/released.

---

**Version:** v0.7 (Specification)

**Status:** This specification is complete but subject to minor refinements based on implementation experience.
