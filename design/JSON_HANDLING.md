# JSON Handling in Raya

This document describes how to work with JSON data in Raya while maintaining **full static type safety**.

---

## Table of Contents

1. [Core Principles](#1-core-principles)
2. [JSON Type Definition](#2-json-type-definition)
3. [Parsing JSON](#3-parsing-json)
4. [Serializing to JSON](#4-serializing-to-json)
5. [Type-Safe Decoding](#5-type-safe-decoding)
6. [Validation](#6-validation)
7. [Common Patterns](#7-common-patterns)
8. [With Reflection](#8-with-reflection)
9. [Examples](#9-examples)

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

## 2. JSON Type Definition

### 2.1 JSON Value Type

Represent JSON as a discriminated union:

```ts
type JsonValue =
  | { kind: "null" }
  | { kind: "boolean"; value: boolean }
  | { kind: "number"; value: number }
  | { kind: "string"; value: string }
  | { kind: "array"; value: JsonValue[] }
  | { kind: "object"; value: Map<string, JsonValue> };
```

**Why this works:**
- Each JSON type has an explicit discriminant (`kind`)
- Compiler can verify exhaustiveness
- No runtime type tags needed (discriminant is a string literal)
- Type-safe pattern matching via `switch`

### 2.2 Alternative: Simpler Union

For performance-critical code:

```ts
type Json = null | boolean | number | string | Json[] | JsonObject;

interface JsonObject {
  [key: string]: Json;
}
```

**Trade-off:** Less explicit, but more ergonomic. Still type-safe.

---

## 3. Parsing JSON

### 3.1 Parse Function Signature

```ts
type ParseResult<T> =
  | { status: "ok"; value: T }
  | { status: "error"; error: string };

// Built-in JSON parser (returns discriminated union)
function parseJson(input: string): ParseResult<JsonValue>;
```

### 3.2 Example Usage

```ts
const input = '{"name": "Alice", "age": 30}';
const result = parseJson(input);

switch (result.status) {
  case "ok":
    const json = result.value;
    // json has type JsonValue
    processJson(json);
    break;
  case "error":
    console.error(`Parse error: ${result.error}`);
    break;
}
```

---

## 4. Serializing to JSON

### 4.1 Stringify Function

```ts
// Built-in JSON stringifier
function stringifyJson(value: JsonValue): string;
```

### 4.2 Example Usage

```ts
const json: JsonValue = {
  kind: "object",
  value: new Map([
    ["name", { kind: "string", value: "Alice" }],
    ["age", { kind: "number", value: 30 }]
  ])
};

const output = stringifyJson(json);
console.log(output); // {"name":"Alice","age":30}
```

---

## 5. Type-Safe Decoding

### 5.1 Decoder Pattern

**Problem:** We have `JsonValue`, but we want a strongly-typed Raya object.

**Solution:** Write **decoder functions** that validate and transform JSON.

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

### 5.2 Usage

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

## 6. Validation

### 6.1 Decoder Combinators

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

### 6.2 Field Extraction Helper

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

### 6.3 Improved User Decoder

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

## 7. Common Patterns

### 7.1 API Response Handling

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

### 7.2 Encoding to JSON

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

### 7.3 Array Handling

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

## 8. With Reflection

When compiled with `--emit-reflection`, you can use reflection for automatic serialization:

### 8.1 Auto-Serialization

```ts
import * as Reflect from "raya:reflect";

function autoEncode(value: any): JsonValue {
  const typeInfo = Reflect.typeOf(value);

  switch (typeInfo.kind) {
    case "primitive":
      if (typeInfo.name === "null") {
        return { kind: "null" };
      }
      if (typeInfo.name === "boolean") {
        return { kind: "boolean", value: value as boolean };
      }
      if (typeInfo.name === "number") {
        return { kind: "number", value: value as number };
      }
      if (typeInfo.name === "string") {
        return { kind: "string", value: value as string };
      }
      throw new Error(`Unsupported primitive: ${typeInfo.name}`);

    case "array":
      const arr = value as any[];
      const encodedArray = arr.map(item => autoEncode(item));
      return { kind: "array", value: encodedArray };

    case "class":
    case "interface":
      const props = Reflect.getProperties(value);
      const fields = new Map<string, JsonValue>();
      for (const prop of props) {
        const propValue = Reflect.getProperty(value, prop.name);
        fields.set(prop.name, autoEncode(propValue));
      }
      return { kind: "object", value: fields };

    default:
      throw new Error(`Cannot encode type: ${typeInfo.kind}`);
  }
}

// Usage
const user: User = { name: "Alice", age: 30, email: null };
const json = autoEncode(user);
const jsonString = stringifyJson(json);
```

### 8.2 Auto-Deserialization

```ts
function autoDecode<T>(json: JsonValue): ParseResult<T> {
  const typeInfo = Reflect.typeInfo<T>();

  if (typeInfo.kind === "class") {
    if (json.kind !== "object") {
      return { status: "error", error: "Expected object" };
    }

    const instance = Reflect.construct(typeInfo, []);
    const obj = json.value;

    if (!typeInfo.properties) {
      return { status: "ok", value: instance as T };
    }

    for (const prop of typeInfo.properties) {
      const jsonValue = obj.get(prop.name);
      if (!jsonValue) {
        return { status: "error", error: `Missing field: ${prop.name}` };
      }

      // Recursively decode based on property type
      const decodedValue = decodeByType(jsonValue, prop.type);
      if (decodedValue.status !== "ok") {
        return decodedValue;
      }

      Reflect.setProperty(instance, prop.name, decodedValue.value);
    }

    return { status: "ok", value: instance as T };
  }

  return { status: "error", error: "Auto-decode only supports classes" };
}

function decodeByType(
  json: JsonValue,
  typeInfo: TypeInfo
): ParseResult<any> {
  switch (typeInfo.kind) {
    case "primitive":
      if (typeInfo.name === "string") return decodeString(json);
      if (typeInfo.name === "number") return decodeNumber(json);
      if (typeInfo.name === "boolean") return decodeBoolean(json);
      break;
    // ... handle other types
  }
  return { status: "error", error: `Unsupported type: ${typeInfo.kind}` };
}
```

**Note:** Reflection-based approaches are convenient but have overhead. For performance-critical code, use explicit decoders.

---

## 9. Examples

### 9.1 Complete Example: User API

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

### 9.2 Nested Objects

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

**Key Takeaways:**

1. **JSON as Discriminated Union** — Represent JSON values using discriminated unions
2. **Explicit Decoders** — Write decoder functions that validate and transform JSON
3. **Result Types** — Use `Result<T, Error>` for all parsing operations
4. **Composable Validation** — Build complex decoders from simple primitives
5. **Type Safety** — Never bypass the type system; validate at boundaries
6. **Optional Reflection** — Use reflection for convenience when performance isn't critical

**Benefits:**

✅ **Compile-time guarantees** — Invalid JSON structures caught at runtime
✅ **No `any` type** — All values have explicit types
✅ **Exhaustive checking** — Compiler ensures all cases handled
✅ **Clear errors** — Failed parsing returns descriptive error messages
✅ **Zero overhead** — No runtime type tags (except optional reflection)

This approach gives you **TypeScript-style ergonomics** with **Rust-level safety**!
