# Native JSON Type Design

**Date:** 2026-01-05
**Status:** Proposed Design Change
**Affects:** Type system, runtime representation, standard library

---

## Summary

Add a native `json` type to Raya that behaves like JavaScript JSON values (dynamic, untyped) and can be safely cast to typed objects using the `as` operator with runtime validation.

---

## Motivation

### Problem
APIs and file formats return JSON data that doesn't have compile-time types. Current options are:
1. **Discriminated unions** - Requires wrapping every API response, verbose
2. **Code generation** - Complex tooling, breaks on schema changes
3. **No solution** - Can't work with external JSON

### Solution
A native `json` type that:
- ✅ Works like JavaScript JSON (dynamic access, no compile-time types)
- ✅ Integrates with Raya's type system via `as` casting
- ✅ Validates at runtime during casting
- ✅ Simple mental model: "JSON is dynamic until you cast it"

---

## Type Definition

### The `json` Type

```typescript
// Built-in primitive type (lowercase)
type json = /* opaque runtime type */
```

**Key Properties:**
- **Opaque type**: Cannot be inspected at compile time
- **Dynamic access**: All property/index access returns `json`
- **Runtime representation**: Tree of JSON values (object/array/primitive)
- **No type checking**: Compiler doesn't validate structure

---

## Behavior

### 1. JSON Values

A `json` value can hold any valid JSON:

```typescript
let data: json = JSON.parse('{"name": "Alice", "age": 30}');
let list: json = JSON.parse('[1, 2, 3]');
let text: json = JSON.parse('"hello"');
let num: json = JSON.parse('42');
let flag: json = JSON.parse('true');
let empty: json = JSON.parse('null');
```

### 2. Dynamic Property Access

Access any property - returns `json`:

```typescript
let data: json = JSON.parse('{"user": {"name": "Bob"}}');

// All property access returns json
let user: json = data.user;           // ✅ OK - returns json
let name: json = data.user.name;      // ✅ OK - returns json
let missing: json = data.foo.bar.baz; // ✅ OK - returns json (undefined at runtime)

// Can't use as typed value without casting
let s: string = data.user.name;       // ❌ Error: Cannot assign json to string
```

### 3. Dynamic Index Access

Arrays work the same way:

```typescript
let list: json = JSON.parse('[10, 20, 30]');

let first: json = list[0];     // ✅ OK - returns json
let second: json = list[1];    // ✅ OK - returns json

let n: number = list[0];       // ❌ Error: Cannot assign json to number
```

### 4. Runtime Semantics

Property/index access follows JavaScript semantics:

```typescript
let data: json = JSON.parse('{"a": 1}');

// Accessing missing property returns json representing undefined
let missing: json = data.foo;  // Runtime: JsonValue::Undefined

// Can check with typeof or equality
if (typeof missing === "undefined") {
  console.log("Property doesn't exist");
}
```

---

## Type Casting with `as`

### Syntax

```typescript
jsonValue as TargetType
```

**Semantics:**
1. **Compile-time**: Type checker allows the cast (assumes it will succeed)
2. **Runtime**: VM validates the JSON structure matches `TargetType`
3. **Success**: Returns typed object
4. **Failure**: Throws `TypeError` with details

### Example: Basic Casting

```typescript
interface User {
  name: string;
  age: number;
}

let response: json = fetch("https://api.example.com/user");

// Cast to typed object
let user = response as User;  // Runtime validation

// Now fully typed
console.log(user.name.toUpperCase());  // ✅ user.name is string
console.log(user.age + 1);             // ✅ user.age is number
```

### Runtime Validation Rules

The cast `jsonValue as TargetType` validates:

#### 1. Primitives

```typescript
let data: json = JSON.parse('42');

let n = data as number;    // ✅ OK - JSON number → number
let s = data as string;    // ❌ TypeError: Expected string, got number
```

#### 2. Objects

```typescript
interface Point {
  x: number;
  y: number;
}

let data: json = JSON.parse('{"x": 10, "y": 20}');

let point = data as Point;  // ✅ Validates:
                            //   - data is JSON object
                            //   - has field 'x' of type number
                            //   - has field 'y' of type number
```

**Missing fields:**
```typescript
let bad: json = JSON.parse('{"x": 10}');  // Missing 'y'

let point = bad as Point;  // ❌ TypeError: Missing required field 'y'
```

**Extra fields:**
```typescript
let extra: json = JSON.parse('{"x": 10, "y": 20, "z": 30}');

let point = extra as Point;  // ✅ OK - extra fields ignored
```

**Wrong types:**
```typescript
let wrong: json = JSON.parse('{"x": "hello", "y": 20}');

let point = wrong as Point;  // ❌ TypeError: Field 'x' expected number, got string
```

#### 3. Arrays

```typescript
let data: json = JSON.parse('[1, 2, 3]');

let nums = data as number[];  // ✅ Validates each element is number
```

**Element validation:**
```typescript
let mixed: json = JSON.parse('[1, "two", 3]');

let nums = mixed as number[];  // ❌ TypeError: Element 1 expected number, got string
```

#### 4. Nullable Types

```typescript
let data: json = JSON.parse('null');

let maybe = data as string | null;  // ✅ OK - null matches null
```

```typescript
let value: json = JSON.parse('"hello"');

let s = value as string | null;  // ✅ OK - string matches string
```

#### 5. Nested Structures

```typescript
interface Address {
  street: string;
  city: string;
}

interface User {
  name: string;
  address: Address;
}

let data: json = JSON.parse(`{
  "name": "Alice",
  "address": {
    "street": "123 Main St",
    "city": "NYC"
  }
}`);

let user = data as User;  // ✅ Validates entire tree recursively
```

---

## Compile-Time Behavior

### Type Checking

The compiler treats `json` as opaque:

```typescript
function processUser(data: json) {
  // ❌ Cannot use json directly as typed value
  console.log(data.name.toUpperCase());  // Error: json has no known properties

  // ✅ Must cast first
  let user = data as User;
  console.log(user.name.toUpperCase());  // OK
}
```

### Type Safety

The cast is **assumed to succeed** at compile time:

```typescript
let data: json = getApiResponse();

let user = data as User;  // Compiler: "OK, I trust this will work at runtime"

// From this point, 'user' is fully typed
user.name.toUpperCase();  // ✅ Type-safe
```

**Rationale:** JSON structure can't be known at compile time (comes from APIs, files, etc.)

### Cast Validation Location

Validation happens **exactly at the `as` site**:

```typescript
let response: json = fetch("/api/users");

// Validation happens here ↓
let users = response as User[];

// If we get past the line above, 'users' is guaranteed to be User[]
for (let user of users) {
  console.log(user.name);  // No further validation needed
}
```

---

## Runtime Representation

### VM Type: `JsonValue`

Internal representation:

```rust
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),        // JSON numbers are always f64
    String(String),
    Array(Vec<JsonValue>),
    Object(HashMap<String, JsonValue>),
    Undefined,          // For missing properties
}
```

### Memory Layout

`json` values are **heap-allocated** GC objects:

```text
┌─────────────────────────────────────┐
│ GcHeader                            │
├─────────────────────────────────────┤
│ JsonValue (enum tag + data)         │
│  - Null/Bool/Number: inline         │
│  - String: GC pointer to string     │
│  - Array: Vec of GC pointers        │
│  - Object: Map of GC pointers       │
└─────────────────────────────────────┘
```

### Property Access Bytecode

```
LOAD_LOCAL 0      // load json value
JSON_GET "user"   // dynamic property access → json
JSON_GET "name"   // dynamic property access → json
```

**JSON_GET opcode:**
```
JSON_GET <field_name>
  Pop: json object
  Push: json value (field or Undefined)
```

### Array Index Bytecode

```
LOAD_LOCAL 0      // load json array
CONST_I32 0       // index
JSON_INDEX        // dynamic index access → json
```

**JSON_INDEX opcode:**
```
JSON_INDEX
  Pop: index (i32), json array
  Push: json value (element or Undefined)
```

### Casting Bytecode

```
LOAD_LOCAL 0           // load json value
JSON_CAST <type_id>    // validate and convert
```

**JSON_CAST opcode:**
```
JSON_CAST <type_id>
  Pop: json value
  Push: typed object (or throw TypeError)

  Validates json structure matches type_id schema
  Constructs typed object from JSON tree
```

---

## Type Casting Implementation

### Validation Algorithm

```rust
fn validate_cast(json: JsonValue, target_type: TypeInfo) -> Result<Object, TypeError> {
    match (json, target_type) {
        // Primitives
        (JsonValue::Number(n), Type::Number) => Ok(Value::f64(n)),
        (JsonValue::String(s), Type::String) => Ok(Value::string(s)),
        (JsonValue::Bool(b), Type::Bool) => Ok(Value::bool(b)),
        (JsonValue::Null, Type::Null) => Ok(Value::null()),

        // Objects
        (JsonValue::Object(map), Type::Interface(schema)) => {
            let mut obj = Object::new(schema.class_id, schema.field_count);

            for (field_name, field_type) in &schema.fields {
                let json_field = map.get(field_name)
                    .ok_or_else(|| TypeError::MissingField(field_name))?;

                let value = validate_cast(json_field.clone(), field_type.clone())?;
                obj.set_field(field_index, value)?;
            }

            Ok(obj)
        }

        // Arrays
        (JsonValue::Array(elements), Type::Array(elem_type)) => {
            let mut array = Array::new(elem_type.id, elements.len());

            for (i, json_elem) in elements.iter().enumerate() {
                let value = validate_cast(json_elem.clone(), elem_type)?;
                array.set(i, value)?;
            }

            Ok(array)
        }

        // Type mismatch
        _ => Err(TypeError::CastFailed {
            expected: target_type,
            got: json.type_name(),
        })
    }
}
```

### Error Messages

```typescript
let data: json = JSON.parse('{"x": "hello"}');

let point = data as Point;
// ❌ TypeError: Cannot cast json to Point
//    Field 'x' expected number, got string
//    at line 3
```

```typescript
let data: json = JSON.parse('[1, "two", 3]');

let nums = data as number[];
// ❌ TypeError: Cannot cast json to number[]
//    Element 1 expected number, got string
//    at line 3
```

---

## Standard Library Functions

### JSON.parse

```typescript
namespace JSON {
  function parse(text: string): json;
}
```

**Example:**
```typescript
let data: json = JSON.parse('{"name": "Alice"}');
```

**Implementation:**
- Parse JSON string using standard parser
- Construct `JsonValue` tree
- Return as `json` type

### JSON.stringify

```typescript
namespace JSON {
  function stringify(value: json): string;
  function stringify(value: json, replacer: null, space: number): string;
}
```

**Example:**
```typescript
let data: json = JSON.parse('{"name": "Alice"}');
let text: string = JSON.stringify(data);
```

**Implementation:**
- Traverse `JsonValue` tree
- Serialize to JSON string

### Type Guards

```typescript
namespace JSON {
  function isNull(value: json): boolean;
  function isBoolean(value: json): boolean;
  function isNumber(value: json): boolean;
  function isString(value: json): boolean;
  function isArray(value: json): boolean;
  function isObject(value: json): boolean;
}
```

**Example:**
```typescript
let data: json = getResponse();

if (JSON.isObject(data)) {
  // data is JSON object, but still json type
  let user = data as User;
}
```

---

## Examples

### Example 1: Fetching API Data

```typescript
interface User {
  id: number;
  name: string;
  email: string;
}

async function getUser(id: number): Promise<User> {
  let response: json = await fetch(`/api/users/${id}`);

  // Validate and cast to typed object
  let user = response as User;

  return user;
}

// Usage
let user = await getUser(123);
console.log(user.name.toUpperCase());  // Fully typed
```

### Example 2: Nested Structures

```typescript
interface Address {
  street: string;
  city: string;
  country: string;
}

interface Company {
  name: string;
  address: Address;
}

interface User {
  name: string;
  company: Company;
}

let response: json = await fetch("/api/user");

// Single cast validates entire tree
let user = response as User;

console.log(user.company.address.city);  // Fully typed
```

### Example 3: Arrays

```typescript
interface Product {
  id: number;
  name: string;
  price: number;
}

let response: json = await fetch("/api/products");

// Cast to array of objects
let products = response as Product[];

for (let product of products) {
  console.log(`${product.name}: $${product.price}`);
}
```

### Example 4: Optional Fields

```typescript
interface User {
  name: string;
  age: number;
  email: string | null;  // Optional
}

let data: json = JSON.parse('{"name": "Bob", "age": 25, "email": null}');

let user = data as User;  // ✅ null matches string | null

if (user.email !== null) {
  sendEmail(user.email);  // Narrowed to string
}
```

### Example 5: Error Handling

```typescript
interface User {
  name: string;
  age: number;
}

let response: json = await fetch("/api/user");

try {
  let user = response as User;
  console.log(user.name);
} catch (e: TypeError) {
  console.error("Invalid user data:", e.message);
  // Handle validation error
}
```

### Example 6: Dynamic Access Before Casting

```typescript
let config: json = loadConfig("app.json");

// Can access dynamically before casting
let version: json = config.version;
let features: json = config.features;

// Cast specific parts to typed structures
interface Features {
  darkMode: boolean;
  experimental: string[];
}

let typedFeatures = features as Features;

if (typedFeatures.darkMode) {
  enableDarkMode();
}
```

---

## Comparison with Other Approaches

### vs. Discriminated Unions

**Before (manual unions):**
```typescript
type ApiResponse =
  | { status: "ok"; data: User }
  | { status: "error"; message: string };

let response = await fetch("/api/user");  // Already parsed to union

if (response.status === "ok") {
  console.log(response.data.name);
}
```

**With json type:**
```typescript
let response: json = await fetch("/api/user");
let user = response as User;  // Direct cast
console.log(user.name);
```

**Advantage:** No need to wrap API responses in discriminated unions.

### vs. Code Generation

**Before (generated types):**
```bash
$ openapi-codegen api.yaml --out types.ts
```

```typescript
import { User } from "./types";  // Generated

let response = await fetchUser(123);  // Already typed
console.log(response.name);
```

**With json type:**
```typescript
interface User {  // Manual, but simple
  name: string;
  age: number;
}

let response: json = await fetch("/api/user");
let user = response as User;
console.log(user.name);
```

**Advantage:** No build step, no generated code to maintain.

---

## Type System Integration

### Type Compatibility

`json` is **not compatible** with any other type without casting:

```typescript
let data: json = JSON.parse("42");

let n: number = data;           // ❌ Error
let s: string = data;           // ❌ Error
let obj: User = data;           // ❌ Error

let n: number = data as number; // ✅ OK
```

### Generic Functions

```typescript
function parseAs<T>(text: string): T {
  let data: json = JSON.parse(text);
  return data as T;  // Runtime validation
}

let user = parseAs<User>('{"name": "Alice", "age": 30}');
```

### Union Types

```typescript
type Response = User | Error;

let data: json = await fetch("/api/user");

// Cast to union
let response = data as Response;  // Validates against union
```

---

## Compiler Changes

### Type Checker

1. **Add `json` as built-in primitive type**
   - Opaque type (no structure inspection)
   - All property/index access returns `json`

2. **Allow `json as T` casts**
   - Permit casting from `json` to any type
   - No compile-time validation (runtime only)

3. **Forbid implicit conversions**
   - Cannot assign `json` to typed variables
   - Cannot pass `json` to typed parameters

### Code Generation

1. **Property access: `json.field` → `JSON_GET`**
2. **Index access: `json[i]` → `JSON_INDEX`**
3. **Casting: `json as T` → `JSON_CAST <type_id>`**

---

## VM Changes

### New Opcodes

```
JSON_GET <field_name>
  Pop: json
  Push: json (field value or Undefined)

JSON_INDEX
  Pop: index (i32), json
  Push: json (element or Undefined)

JSON_CAST <type_id>
  Pop: json
  Push: typed object (or throw TypeError)

  Validates json against TypeRegistry[type_id]
  Throws TypeError on validation failure
```

### Type Registry Enhancement

Store schemas for validation:

```rust
pub struct TypeSchema {
    pub type_id: TypeId,
    pub kind: TypeKind,
}

pub enum TypeKind {
    Primitive(PrimitiveType),
    Interface {
        fields: Vec<(String, TypeId)>,
    },
    Array {
        element_type: TypeId,
    },
    Union {
        variants: Vec<TypeId>,
    },
}
```

---

## GC Integration

### Marking JsonValue

```rust
// Special case in GC mark_value()
match type_name {
    "JsonValue" => {
        let json = unsafe { &*(ptr as *const JsonValue) };
        match json {
            JsonValue::Array(elements) => {
                for elem in elements {
                    self.mark_json_value(elem);
                }
            }
            JsonValue::Object(map) => {
                for value in map.values() {
                    self.mark_json_value(value);
                }
            }
            _ => {}  // Primitives have no pointers
        }
    }
}
```

---

## Security Considerations

### 1. Denial of Service

**Issue:** Deeply nested JSON can cause stack overflow during validation.

**Mitigation:**
- Limit maximum nesting depth (e.g., 100 levels)
- Track recursion depth in `validate_cast()`

```rust
fn validate_cast(json: JsonValue, target_type: TypeInfo, depth: usize) -> Result<Object, TypeError> {
    if depth > MAX_NEST_DEPTH {
        return Err(TypeError::TooDeepNesting);
    }
    // ... rest of validation
}
```

### 2. Large Objects

**Issue:** Casting huge JSON can consume excessive memory.

**Mitigation:**
- Enforce maximum object size (e.g., 100 MB)
- Check size before validation

### 3. Type Confusion

**Issue:** Cast allows lying about types.

**Protection:** Runtime validation catches all mismatches.

---

## Performance Considerations

### 1. Validation Cost

Casting validates **entire tree** recursively:

```typescript
let huge: json = JSON.parse(/* 10 MB response */);
let data = huge as BigType;  // ⚠️ Validates all 10 MB
```

**Optimization:** Cache validation results for repeated casts.

### 2. Property Access Cost

Each `json.field` access is a **runtime HashMap lookup**:

```typescript
let data: json = getConfig();

// Each access is O(1) hash lookup
let a = data.a;
let b = data.b;
let c = data.c;
```

**Optimization:** Cast early, access typed fields (no lookups).

---

## TypeScript Compatibility

This design is **inspired by TypeScript** but more explicit:

### TypeScript

```typescript
let data: any = JSON.parse('...');  // any - no validation
let user = data as User;            // Assumed, never validated
```

### Raya

```typescript
let data: json = JSON.parse('...');  // json - dynamic
let user = data as User;             // ✅ Runtime validation
```

**Key difference:** Raya validates at runtime, TypeScript doesn't.

---

## Migration Path

### From Current Code

**Before (no json type):**
```typescript
// Had to use discriminated unions or code generation
type ApiResponse = /* complex union */;
```

**After (with json type):**
```typescript
let response: json = await fetch("/api/data");
let typed = response as MyType;
```

**Impact:** Optional - existing code continues to work.

---

## Open Questions

### 1. Should `json as T` be soft or hard error on failure?

**Option A: Hard error (throw TypeError)**
```typescript
let user = response as User;  // Throws on mismatch
```

**Option B: Soft error (return null)**
```typescript
let user = response as User;  // Returns null on mismatch
```

**Recommendation:** Hard error (fail fast, clear errors).

### 2. Should we allow `T as json` (reverse cast)?

```typescript
let user: User = { name: "Alice", age: 30 };
let data: json = user as json;  // Convert to JSON?
```

**Recommendation:** No - use `JSON.parse(JSON.stringify(user))`.

### 3. Should json have literal syntax?

```typescript
let data: json = #{ name: "Alice", age: 30 };  // Hypothetical
```

**Recommendation:** No - keep it simple, use `JSON.parse()`.

---

## Summary

### ✅ Benefits

1. **Simple mental model**: JSON is dynamic until you cast it
2. **Type safe**: Runtime validation catches all mismatches
3. **No codegen**: No build tools required
4. **Compatible**: Works with any JSON API
5. **Familiar**: Similar to TypeScript `any` but with validation

### ⚠️ Trade-offs

1. **Runtime cost**: Validation happens at runtime
2. **No compile-time checks**: Structure unknown until cast
3. **Error handling**: Must handle TypeError from failed casts

### 🎯 Use Cases

- ✅ Parsing API responses
- ✅ Loading config files
- ✅ Reading JSON from files
- ✅ Working with external data
- ❌ Internal typed data (use interfaces directly)

---

## Next Steps

1. Update LANG.md with `json` type specification
2. Implement `JsonValue` in VM
3. Add `JSON_GET`, `JSON_INDEX`, `JSON_CAST` opcodes
4. Implement runtime validation algorithm
5. Add `JSON.parse()` and `JSON.stringify()` to stdlib
6. Write comprehensive tests
