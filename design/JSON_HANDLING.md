# JSON Handling in Raya

This document describes how to work with JSON data in Raya while maintaining **full static type safety**.

---

## Table of Contents

1. [Core Principles](#1-core-principles)
2. [Simple API (Go-Style)](#2-simple-api-go-style)
3. [Advanced: Manual Decoders](#3-advanced-manual-decoders)
4. [Advanced: JSON Type Definition](#4-advanced-json-type-definition)
5. [Advanced: Decoder Combinators](#5-advanced-decoder-combinators)
6. [Common Patterns](#6-common-patterns)
7. [Examples](#7-examples)

---

## 1. Core Principles

**Challenge:** JSON is dynamically typed, but Raya requires static types.

**Solution:** Use **discriminated unions** to represent JSON values, then provide **type-safe decoders** that validate and transform JSON into strongly-typed Raya objects.

**Key Rules:**
1. **Never use `any`** — All JSON values have explicit types
2. **Validate at boundaries** — Check JSON structure when parsing
3. **Fail fast** — Return `Result<T, Error>` for parsing operations
4. **Compile-time guarantees** — Use discriminated unions for JSON representation

---

## 2. Simple API (Go-Style)

**For most use cases, use the simple API.** It provides Go-like ergonomics with full type safety.

### 2.1 Quick Start

```ts
import { JSON } from "raya:json";

interface User {
  name: string;
  age: number;
  email: string | null;
}

// Encoding (like Go's json.Marshal)
const user: User = { name: "Alice", age: 30, email: "alice@example.com" };
const result = JSON.encode(user);

switch (result.status) {
  case "ok":
    const jsonString = result.value;  // jsonString: string
    console.log(jsonString);
    break;
  case "error":
    console.error(`Encode error: ${result.error}`);
    break;
}

// Decoding (like Go's json.Unmarshal)
const jsonString = '{"name":"Alice","age":30,"email":"alice@example.com"}';
const decodeResult = JSON.decode<User>(jsonString);

switch (decodeResult.status) {
  case "ok":
    const user = decodeResult.value;  // user: User
    console.log(`User: ${user.name}, ${user.age}`);
    break;
  case "error":
    console.error(`Decode error: ${decodeResult.error}`);
    break;
}
```

### 2.2 API Reference

```ts
namespace JSON {
  // Encode any value to JSON string
  export function encode<T>(value: T): Result<string, Error>;

  // Decode JSON string to typed value
  export function decode<T>(input: string): Result<T, Error>;
}

type Result<T, E> =
  | { status: "ok"; value: T }
  | { status: "error"; error: E };
```

### 2.3 How It Works

**Compile-time (default, no reflection):**
- Compiler generates specialized encode/decode functions for each type
- Uses type structure to generate serialization code
- Zero runtime overhead, no reflection needed

**Runtime (with `--emit-reflection`):**
- Uses reflection API to inspect types dynamically
- More flexible but slower
- Useful for generic libraries and debugging

**Recommendation:** Use the simple API unless you need maximum performance or have complex custom validation logic.

### 2.4 Complete Example

```ts
import { JSON } from "raya:json";

interface Address {
  street: string;
  city: string;
  zipCode: string;
}

interface Person {
  name: string;
  age: number;
  address: Address;
  tags: string[];
}

async function fetchPerson(id: number): Task<Result<Person, string>> {
  const response = await fetch(`/api/person/${id}`);
  const text = await response.text();

  const result = JSON.decode<Person>(text);

  if (result.status !== "ok") {
    return { status: "error", error: result.error.message };
  }

  return { status: "ok", value: result.value };
}

async function savePerson(person: Person): Task<Result<void, string>> {
  const encodeResult = JSON.encode(person);

  if (encodeResult.status !== "ok") {
    return { status: "error", error: encodeResult.error.message };
  }

  const response = await fetch("/api/person", {
    method: "POST",
    body: encodeResult.value,
    headers: { "Content-Type": "application/json" }
  });

  if (!response.ok) {
    return { status: "error", error: "Failed to save person" };
  }

  return { status: "ok", value: undefined };
}
```

### 2.5 Arrays and Collections

```ts
// Encode/decode arrays
const users: User[] = [
  { name: "Alice", age: 30, email: null },
  { name: "Bob", age: 25, email: "bob@example.com" }
];

const encoded = JSON.encode(users);
// Result<string, Error>

const decoded = JSON.decode<User[]>('[ ... ]');
// Result<User[], Error>

// Encode/decode maps
const userMap = new Map<string, User>();
userMap.set("alice", { name: "Alice", age: 30, email: null });

const encodedMap = JSON.encode(userMap);
const decodedMap = JSON.decode<Map<string, User>>('{ ... }');
```

### 2.6 Error Handling

```ts
interface Error {
  message: string;
  path?: string[];  // JSON path where error occurred
}

// Example error messages:
// { message: "Expected number at .age", path: ["age"] }
// { message: "Missing required field 'name'", path: [] }
// { message: "Invalid JSON syntax at position 42" }
```

### 2.7 When to Use Advanced API

Use the **Advanced Manual Decoders** (Section 3) when you need:
- Custom validation logic (e.g., age > 0, email format validation)
- Different JSON field names than struct fields
- Performance-critical code (avoid reflection overhead)
- Backwards compatibility with legacy JSON formats
- Fine-grained error messages

---

## 3. Advanced: Manual Decoders

**For advanced use cases requiring custom validation or maximum performance.**

### 3.1 Decoder Pattern

**Problem:** Need custom validation, field name mapping, or maximum performance.

**Solution:** Write **decoder functions** that validate and transform JSON with full control.

#### JSON Value Type

First, define the JSON discriminated union:

```ts
type JsonValue =
  | { kind: "null" }
  | { kind: "boolean"; value: boolean }
  | { kind: "number"; value: number }
  | { kind: "string"; value: string }
  | { kind: "array"; value: JsonValue[] }
  | { kind: "object"; value: Map<string, JsonValue> };

type ParseResult<T> =
  | { status: "ok"; value: T }
  | { status: "error"; error: string };
```

#### Example Decoder

```ts
interface User {
  name: string;
  age: number;
  email: string | null;
}

function decodeUser(json: JsonValue): ParseResult<User> {
  // Check that json is an object
  if (json.kind !== "object") {
    return { status: "error", error: "Expected object" };
  }

  const obj = json.value;

  // Extract and validate 'name' field
  const nameField = obj.get("name");
  if (!nameField) {
    return { status: "error", error: "Missing 'name' field" };
  }
  if (nameField.kind !== "string") {
    return { status: "error", error: "'name' must be string" };
  }
  const name = nameField.value;

  // Extract and validate 'age' field
  const ageField = obj.get("age");
  if (!ageField) {
    return { status: "error", error: "Missing 'age' field" };
  }
  if (ageField.kind !== "number") {
    return { status: "error", error: "'age' must be number" };
  }
  const age = ageField.value;

  // Extract and validate optional 'email' field
  const emailField = obj.get("email");
  let email: string | null = null;
  if (emailField) {
    if (emailField.kind === "string") {
      email = emailField.value;
    } else if (emailField.kind !== "null") {
      return { status: "error", error: "'email' must be string or null" };
    }
  }

  return {
    status: "ok",
    value: { name, age, email }
  };
}
```

### 3.2 Custom Validation

Manual decoders allow custom validation logic:

```ts
function decodeUser(json: JsonValue): ParseResult<User> {
  if (json.kind !== "object") {
    return { status: "error", error: "Expected object" };
  }

  const obj = json.value;

  // Name validation
  const nameField = obj.get("name");
  if (!nameField || nameField.kind !== "string") {
    return { status: "error", error: "Invalid name field" };
  }
  const name = nameField.value.trim();
  if (name.length === 0) {
    return { status: "error", error: "Name cannot be empty" };
  }

  // Age validation with range check
  const ageField = obj.get("age");
  if (!ageField || ageField.kind !== "number") {
    return { status: "error", error: "Invalid age field" };
  }
  const age = ageField.value;
  if (age < 0 || age > 150) {
    return { status: "error", error: "Age must be between 0 and 150" };
  }

  // Email validation
  const emailField = obj.get("email");
  let email: string | null = null;
  if (emailField && emailField.kind === "string") {
    email = emailField.value;
    // Basic email format validation
    if (!email.includes("@")) {
      return { status: "error", error: "Invalid email format" };
    }
  }

  return { status: "ok", value: { name, age, email } };
}
```

### 3.3 Usage

```ts
const input = '{"name":"Alice","age":30,"email":"alice@example.com"}';
const parseResult = parseJson(input);

if (parseResult.status !== "ok") {
  console.error("Parse failed");
  return;
}

const decodeResult = decodeUser(parseResult.value);

switch (decodeResult.status) {
  case "ok":
    const user = decodeResult.value; // user: User
    console.log(`User: ${user.name}, ${user.age}`);
    break;
  case "error":
    console.error(`Decode error: ${decodeResult.error}`);
    break;
}
```

---

## 4. Advanced: JSON Type Definition

### 4.1 Alternative JSON Representations

For simpler code or performance-critical paths, you can use a simpler union type:

```ts
type Json = null | boolean | number | string | Json[] | JsonObject;

interface JsonObject {
  [key: string]: Json;
}
```

**Trade-offs:**
- ✅ More ergonomic and less verbose
- ✅ Closer to JavaScript's native types
- ❌ Less explicit than discriminated unions
- ❌ Harder to validate exhaustively

### 4.2 Parsing and Stringifying

```ts
// Parse JSON string to JsonValue
function parseJson(input: string): ParseResult<JsonValue>;

// Stringify JsonValue to JSON string
function stringifyJson(value: JsonValue): string;
```

**Example:**

```ts
const input = '{"name": "Alice", "age": 30}';
const result = parseJson(input);

switch (result.status) {
  case "ok":
    const json = result.value;  // json: JsonValue
    const output = stringifyJson(json);
    console.log(output);
    break;
  case "error":
    console.error(`Parse error: ${result.error}`);
    break;
}
```

---

## 5. Advanced: Decoder Combinators

### 5.1 Primitive Decoders

Build complex decoders from simple ones:

```ts
// Decode a JSON string
function decodeString(json: JsonValue): ParseResult<string> {
  if (json.kind !== "string") {
    return { status: "error", error: "Expected string" };
  }
  return { status: "ok", value: json.value };
}

// Decode a JSON number
function decodeNumber(json: JsonValue): ParseResult<number> {
  if (json.kind !== "number") {
    return { status: "error", error: "Expected number" };
  }
  return { status: "ok", value: json.value };
}

// Decode a JSON boolean
function decodeBoolean(json: JsonValue): ParseResult<boolean> {
  if (json.kind !== "boolean") {
    return { status: "error", error: "Expected boolean" };
  }
  return { status: "ok", value: json.value };
}

// Decode an array of T
function decodeArray<T>(
  decoder: (json: JsonValue) => ParseResult<T>
): (json: JsonValue) => ParseResult<T[]> {
  return (json: JsonValue) => {
    if (json.kind !== "array") {
      return { status: "error", error: "Expected array" };
    }

    const result: T[] = [];
    for (let i = 0; i < json.value.length; i++) {
      const itemResult = decoder(json.value[i]);
      if (itemResult.status !== "ok") {
        return {
          status: "error",
          error: `Array item ${i}: ${itemResult.error}`
        };
      }
      result.push(itemResult.value);
    }

    return { status: "ok", value: result };
  };
}

// Decode optional field
function decodeOptional<T>(
  decoder: (json: JsonValue) => ParseResult<T>
): (json: JsonValue | null) => ParseResult<T | null> {
  return (json: JsonValue | null) => {
    if (json === null || json.kind === "null") {
      return { status: "ok", value: null };
    }
    return decoder(json);
  };
}
```

### 5.2 Field Extraction Helpers

```ts
function getField(
  obj: Map<string, JsonValue>,
  key: string
): ParseResult<JsonValue> {
  const value = obj.get(key);
  if (!value) {
    return { status: "error", error: `Missing field '${key}'` };
  }
  return { status: "ok", value };
}

function getOptionalField(
  obj: Map<string, JsonValue>,
  key: string
): JsonValue | null {
  const value = obj.get(key);
  return value || null;
}
```

### 5.3 Composable Decoder Example

```ts
function decodeUser(json: JsonValue): ParseResult<User> {
  if (json.kind !== "object") {
    return { status: "error", error: "Expected object" };
  }

  const obj = json.value;

  // Decode 'name'
  const nameResult = getField(obj, "name");
  if (nameResult.status !== "ok") return nameResult;
  const nameDecoded = decodeString(nameResult.value);
  if (nameDecoded.status !== "ok") return nameDecoded;
  const name = nameDecoded.value;

  // Decode 'age'
  const ageResult = getField(obj, "age");
  if (ageResult.status !== "ok") return ageResult;
  const ageDecoded = decodeNumber(ageResult.value);
  if (ageDecoded.status !== "ok") return ageDecoded;
  const age = ageDecoded.value;

  // Decode optional 'email'
  const emailField = getOptionalField(obj, "email");
  const emailDecoded = decodeOptional(decodeString)(emailField);
  if (emailDecoded.status !== "ok") return emailDecoded;
  const email = emailDecoded.value;

  return {
    status: "ok",
    value: { name, age, email }
  };
}
```

---

## 6. Common Patterns

### 6.1 API Response Handling

```ts
type ApiResponse<T> =
  | { status: "success"; data: T }
  | { status: "error"; message: string };

function decodeApiResponse<T>(
  dataDecoder: (json: JsonValue) => ParseResult<T>
): (json: JsonValue) => ParseResult<ApiResponse<T>> {
  return (json: JsonValue) => {
    if (json.kind !== "object") {
      return { status: "error", error: "Expected object" };
    }

    const obj = json.value;
    const statusField = obj.get("status");

    if (!statusField || statusField.kind !== "string") {
      return { status: "error", error: "Invalid status field" };
    }

    switch (statusField.value) {
      case "success":
        const dataField = obj.get("data");
        if (!dataField) {
          return { status: "error", error: "Missing data field" };
        }
        const dataResult = dataDecoder(dataField);
        if (dataResult.status !== "ok") {
          return dataResult;
        }
        return {
          status: "ok",
          value: { status: "success", data: dataResult.value }
        };

      case "error":
        const messageField = obj.get("message");
        if (!messageField || messageField.kind !== "string") {
          return { status: "error", error: "Invalid message field" };
        }
        return {
          status: "ok",
          value: { status: "error", message: messageField.value }
        };

      default:
        return { status: "error", error: `Unknown status: ${statusField.value}` };
    }
  };
}
```

**Usage:**

```ts
const responseDecoder = decodeApiResponse(decodeUser);
const result = responseDecoder(jsonValue);

if (result.status === "ok") {
  const response = result.value;
  switch (response.status) {
    case "success":
      console.log(`User: ${response.data.name}`);
      break;
    case "error":
      console.error(`API error: ${response.message}`);
      break;
  }
}
```

### 6.2 Encoding to JSON

```ts
function encodeUser(user: User): JsonValue {
  const fields = new Map<string, JsonValue>();
  fields.set("name", { kind: "string", value: user.name });
  fields.set("age", { kind: "number", value: user.age });

  if (user.email !== null) {
    fields.set("email", { kind: "string", value: user.email });
  } else {
    fields.set("email", { kind: "null" });
  }

  return { kind: "object", value: fields };
}

// Usage
const user: User = { name: "Alice", age: 30, email: "alice@example.com" };
const json = encodeUser(user);
const jsonString = stringifyJson(json);
```

### 6.3 Array Handling

```ts
function decodeUserList(json: JsonValue): ParseResult<User[]> {
  return decodeArray(decodeUser)(json);
}

// Usage
const input = '[{"name":"Alice","age":30},{"name":"Bob","age":25}]';
const parseResult = parseJson(input);

if (parseResult.status === "ok") {
  const usersResult = decodeUserList(parseResult.value);
  if (usersResult.status === "ok") {
    const users = usersResult.value; // users: User[]
    for (const user of users) {
      console.log(user.name);
    }
  }
}
```

---

## 7. Examples

### 7.1 Complete Example: User API

```ts
interface User {
  id: number;
  name: string;
  email: string;
  isActive: boolean;
}

interface UserListResponse {
  users: User[];
  total: number;
  page: number;
}

// Decoder
function decodeUser(json: JsonValue): ParseResult<User> {
  if (json.kind !== "object") {
    return { status: "error", error: "Expected object" };
  }

  const obj = json.value;

  const id = getField(obj, "id");
  if (id.status !== "ok") return id;
  const idNum = decodeNumber(id.value);
  if (idNum.status !== "ok") return idNum;

  const name = getField(obj, "name");
  if (name.status !== "ok") return name;
  const nameStr = decodeString(name.value);
  if (nameStr.status !== "ok") return nameStr;

  const email = getField(obj, "email");
  if (email.status !== "ok") return email;
  const emailStr = decodeString(email.value);
  if (emailStr.status !== "ok") return emailStr;

  const isActive = getField(obj, "isActive");
  if (isActive.status !== "ok") return isActive;
  const isActiveBool = decodeBoolean(isActive.value);
  if (isActiveBool.status !== "ok") return isActiveBool;

  return {
    status: "ok",
    value: {
      id: idNum.value,
      name: nameStr.value,
      email: emailStr.value,
      isActive: isActiveBool.value
    }
  };
}

function decodeUserListResponse(json: JsonValue): ParseResult<UserListResponse> {
  if (json.kind !== "object") {
    return { status: "error", error: "Expected object" };
  }

  const obj = json.value;

  const users = getField(obj, "users");
  if (users.status !== "ok") return users;
  const usersArray = decodeArray(decodeUser)(users.value);
  if (usersArray.status !== "ok") return usersArray;

  const total = getField(obj, "total");
  if (total.status !== "ok") return total;
  const totalNum = decodeNumber(total.value);
  if (totalNum.status !== "ok") return totalNum;

  const page = getField(obj, "page");
  if (page.status !== "ok") return page;
  const pageNum = decodeNumber(page.value);
  if (pageNum.status !== "ok") return pageNum;

  return {
    status: "ok",
    value: {
      users: usersArray.value,
      total: totalNum.value,
      page: pageNum.value
    }
  };
}

// Usage
async function fetchUsers(page: number): Task<ParseResult<UserListResponse>> {
  const response = await fetch(`/api/users?page=${page}`);
  const text = await response.text();

  const parseResult = parseJson(text);
  if (parseResult.status !== "ok") {
    return parseResult;
  }

  return decodeUserListResponse(parseResult.value);
}

// Main
async function main(): Task<void> {
  const result = await fetchUsers(1);

  switch (result.status) {
    case "ok":
      const data = result.value;
      console.log(`Page ${data.page} of ${data.total} users`);
      for (const user of data.users) {
        console.log(`- ${user.name} (${user.email})`);
      }
      break;
    case "error":
      console.error(`Error: ${result.error}`);
      break;
  }
}
```

### 7.2 Nested Objects

```ts
interface Address {
  street: string;
  city: string;
  zipCode: string;
}

interface Person {
  name: string;
  address: Address;
}

function decodeAddress(json: JsonValue): ParseResult<Address> {
  if (json.kind !== "object") {
    return { status: "error", error: "Expected object" };
  }

  const obj = json.value;

  const street = getField(obj, "street");
  if (street.status !== "ok") return street;
  const streetStr = decodeString(street.value);
  if (streetStr.status !== "ok") return streetStr;

  const city = getField(obj, "city");
  if (city.status !== "ok") return city;
  const cityStr = decodeString(city.value);
  if (cityStr.status !== "ok") return cityStr;

  const zipCode = getField(obj, "zipCode");
  if (zipCode.status !== "ok") return zipCode;
  const zipCodeStr = decodeString(zipCode.value);
  if (zipCodeStr.status !== "ok") return zipCodeStr;

  return {
    status: "ok",
    value: {
      street: streetStr.value,
      city: cityStr.value,
      zipCode: zipCodeStr.value
    }
  };
}

function decodePerson(json: JsonValue): ParseResult<Person> {
  if (json.kind !== "object") {
    return { status: "error", error: "Expected object" };
  }

  const obj = json.value;

  const name = getField(obj, "name");
  if (name.status !== "ok") return name;
  const nameStr = decodeString(name.value);
  if (nameStr.status !== "ok") return nameStr;

  const address = getField(obj, "address");
  if (address.status !== "ok") return address;
  const addressObj = decodeAddress(address.value);
  if (addressObj.status !== "ok") return addressObj;

  return {
    status: "ok",
    value: {
      name: nameStr.value,
      address: addressObj.value
    }
  };
}
```

---

## Summary

**Recommended Approach:**

Use the **Simple API** (Section 2) for most cases:

```ts
import { JSON } from "raya:json";

// Encoding
const result = JSON.encode(user);  // Result<string, Error>

// Decoding
const result = JSON.decode<User>(jsonString);  // Result<User, Error>
```

**When to Use Advanced API:**

Use **Manual Decoders** (Sections 3-5) when you need:
- Custom validation (e.g., age range, email format)
- Field name mapping
- Maximum performance (avoid reflection)
- Complex transformation logic

**Key Benefits:**

✅ **Go-like simplicity** — Single function call for encode/decode
✅ **Full type safety** — All values have explicit types
✅ **Compile-time guarantees** — Type errors caught before execution
✅ **Clear error messages** — Failed parsing returns descriptive errors
✅ **Flexible** — Choose simple API or manual decoders based on needs
✅ **Zero runtime overhead** — No type tags (when using compile-time codegen)

**This approach gives you Go-style ergonomics with Rust-level type safety!**
