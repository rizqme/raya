# Language Examples

Complete working examples demonstrating Raya's features.

## Hello World

```typescript
import logger from "std:logger";

function main(): void {
  logger.info("Hello, Raya!");
}
```

## Type System

### Discriminated Unions

```typescript
import logger from "std:logger";

type Result<T> =
  | { status: "ok"; value: T }
  | { status: "error"; error: string };

function divide(a: int, b: int): Result<int> {
  if (b == 0) {
    return { status: "error", error: "Division by zero" };
  }
  return { status: "ok", value: a / b };
}

function main(): void {
  const r1 = divide(10, 2);
  const r2 = divide(10, 0);
  
  if (r1.status == "ok") {
    logger.info("Result:", r1.value);  // 5
  }
  
  if (r2.status == "error") {
    logger.error("Error:", r2.error);  // Division by zero
  }
}
```

### Generic Classes

```typescript
import logger from "std:logger";

class Stack<T> {
  private items: T[];
  
  constructor() {
    this.items = [];
  }
  
  push(item: T): void {
    this.items.push(item);
  }
  
  pop(): T | null {
    if (this.items.length == 0) {
      return null;
    }
    return this.items.pop();
  }
  
  peek(): T | null {
    if (this.items.length == 0) {
      return null;
    }
    return this.items[this.items.length - 1];
  }
  
  isEmpty(): boolean {
    return this.items.length == 0;
  }
}

function main(): void {
  const stack = new Stack<int>();
  
  stack.push(1);
  stack.push(2);
  stack.push(3);
  
  while (!stack.isEmpty()) {
    const item = stack.pop();
    if (item != null) {
      logger.info(item);  // 3, 2, 1
    }
  }
}
```

## Concurrency

### Parallel Tasks

```typescript
import logger from "std:logger";
import time from "std:time";
import math from "std:math";

async function compute(id: int): Task<int> {
  const delay = math.randomInt(50, 200);
  time.sleep(delay);
  return id * id;
}

function main(): void {
  const start = time.monotonic();
  const tasks: Task<int>[] = [];
  
  // Start 10 tasks concurrently
  for (let i = 0; i < 10; i = i + 1) {
    tasks.push(compute(i));
  }
  
  // Collect results
  let sum = 0;
  for (let i = 0; i < tasks.length; i = i + 1) {
    const result = await tasks[i];
    logger.info("Task", i, "=", result);
    sum = sum + result;
  }
  
  const elapsed = time.elapsed(start);
  logger.info("Sum:", sum);
  logger.info("Time:", elapsed / 1000000, "ms");
}
```

### Concurrent File I/O

```typescript
import logger from "std:logger";
import fs from "std:fs";
import time from "std:time";

async function readFile(path: string): Task<string> {
  logger.info("Reading", path);
  const content = fs.readTextFile(path);
  logger.info("Finished", path);
  return content;
}

function main(): void {
  const start = time.monotonic();
  
  // Read 3 files concurrently
  const t1 = readFile("file1.txt");
  const t2 = readFile("file2.txt");
  const t3 = readFile("file3.txt");
  
  // All reads happen in parallel
  const c1 = await t1;
  const c2 = await t2;
  const c3 = await t3;
  
  const total = c1.length + c2.length + c3.length;
  const elapsed = time.elapsed(start);
  
  logger.info("Total bytes:", total);
  logger.info("Time:", elapsed / 1000000, "ms");
}
```

## HTTP Server

```typescript
import logger from "std:logger";
import { HttpServer, HttpRequest, HttpResponse } from "std:http";

function handleRequest(req: HttpRequest): HttpResponse {
  logger.info(req.method, req.path);
  
  if (req.path == "/") {
    return {
      status: 200,
      headers: { "Content-Type": "text/html" },
      body: "<h1>Hello, Raya!</h1>"
    };
  }
  
  if (req.path == "/api/users") {
    return {
      status: 200,
      headers: { "Content-Type": "application/json" },
      body: '{"users": [{"id": 1, "name": "Alice"}]}'
    };
  }
  
  return {
    status: 404,
    headers: { "Content-Type": "text/plain" },
    body: "Not Found"
  };
}

function main(): void {
  const server = new HttpServer("127.0.0.1", 8080);
  logger.info("Server listening on http://127.0.0.1:8080");
  
  server.serve(handleRequest);
}
```

## JSON Parsing

```typescript
import logger from "std:logger";
import { JSON } from "std:json";

type User = {
  id: int;
  name: string;
  email: string;
  active: boolean;
};

function main(): void {
  const jsonStr = '{"id": 1, "name": "Alice", "email": "alice@example.com", "active": true}';
  
  const user = JSON.parse<User>(jsonStr);
  
  logger.info("User ID:", user.id);
  logger.info("Name:", user.name);
  logger.info("Email:", user.email);
  logger.info("Active:", user.active);
  
  // Serialize back to JSON
  const output = JSON.stringify(user);
  logger.info("JSON:", output);
}
```

## Cryptography

```typescript
import logger from "std:logger";
import crypto from "std:crypto";

function main(): void {
  // Hashing
  const data = "Hello, Raya!";
  const hash = crypto.hash("sha256", data);
  logger.info("SHA-256:", crypto.toHex(hash));
  
  // HMAC
  const key = "my-secret-key";
  const hmac = crypto.hmac("sha256", key, data);
  logger.info("HMAC:", crypto.toHex(hmac));
  
  // Random bytes
  const randomBytes = crypto.randomBytes(32);
  logger.info("Random:", crypto.toBase64(randomBytes));
  
  // Random UUID
  const uuid = crypto.randomUUID();
  logger.info("UUID:", uuid);
}
```

## File System Operations

```typescript
import logger from "std:logger";
import fs from "std:fs";
import path from "std:path";

function main(): void {
  const dir = "./data";
  
  // Create directory
  if (!fs.exists(dir)) {
    fs.createDir(dir);
    logger.info("Created directory:", dir);
  }
  
  // Write file
  const filePath = path.join(dir, "output.txt");
  fs.writeTextFile(filePath, "Hello from Raya!");
  logger.info("Wrote file:", filePath);
  
  // Read file
  const content = fs.readTextFile(filePath);
  logger.info("Content:", content);
  
  // List directory
  const entries = fs.readDir(dir);
  for (const entry of entries) {
    logger.info("Entry:", entry.name, entry.isFile ? "file" : "dir");
  }
  
  // File info
  const info = fs.stat(filePath);
  logger.info("Size:", info.size, "bytes");
  logger.info("Modified:", info.modified);
}
```

## Pattern Matching with Discriminated Unions

```typescript
import logger from "std:logger";

type Shape =
  | { type: "circle"; radius: number }
  | { type: "rectangle"; width: number; height: number }
  | { type: "triangle"; base: number; height: number };

function area(shape: Shape): number {
  if (shape.type == "circle") {
    return 3.14159 * shape.radius * shape.radius;
  } else if (shape.type == "rectangle") {
    return shape.width * shape.height;
  } else {  // triangle
    return 0.5 * shape.base * shape.height;
  }
}

function main(): void {
  const shapes: Shape[] = [
    { type: "circle", radius: 5 },
    { type: "rectangle", width: 4, height: 6 },
    { type: "triangle", base: 3, height: 4 }
  ];
  
  for (const shape of shapes) {
    logger.info("Area:", area(shape));
  }
}
```

## Class Inheritance

```typescript
import logger from "std:logger";

abstract class Animal {
  name: string;
  age: int;
  
  constructor(name: string, age: int) {
    this.name = name;
    this.age = age;
  }
  
  abstract makeSound(): void;
  
  describe(): void {
    logger.info(this.name, "is", this.age, "years old");
  }
}

class Dog extends Animal {
  breed: string;
  
  constructor(name: string, age: int, breed: string) {
    super(name, age);
    this.breed = breed;
  }
  
  makeSound(): void {
    logger.info(this.name, "barks: Woof!");
  }
}

class Cat extends Animal {
  indoor: boolean;
  
  constructor(name: string, age: int, indoor: boolean) {
    super(name, age);
    this.indoor = indoor;
  }
  
  makeSound(): void {
    logger.info(this.name, "meows: Meow!");
  }
}

function main(): void {
  const animals: Animal[] = [
    new Dog("Buddy", 3, "Golden Retriever"),
    new Cat("Whiskers", 5, true)
  ];
  
  for (const animal of animals) {
    animal.describe();
    animal.makeSound();
  }
}
```

## Error Handling

```typescript
import logger from "std:logger";
import fs from "std:fs";

type FileResult =
  | { ok: true; content: string }
  | { ok: false; error: string };

function safeReadFile(path: string): FileResult {
  try {
    const content = fs.readTextFile(path);
    return { ok: true, content: content };
  } catch (e) {
    return { ok: false, error: e.message };
  }
}

function main(): void {
  const paths = ["config.json", "nonexistent.txt", "data.txt"];
  
  for (const path of paths) {
    const result = safeReadFile(path);
    
    if (result.ok) {
      logger.info("Read", path, "-", result.content.length, "bytes");
    } else {
      logger.error("Failed to read", path, ":", result.error);
    }
  }
}
```

## Decorators Example

```typescript
import logger from "std:logger";

function Log(target: any, propertyKey: string, descriptor: PropertyDescriptor): void {
  const original = descriptor.value;
  
  descriptor.value = function(...args: any[]): any {
    logger.info("Calling", propertyKey, "with", args);
    const result = original.apply(this, args);
    logger.info("Result:", result);
    return result;
  };
}

class Calculator {
  @Log
  add(a: int, b: int): int {
    return a + b;
  }
  
  @Log
  multiply(a: int, b: int): int {
    return a * b;
  }
}

function main(): void {
  const calc = new Calculator();
  
  calc.add(5, 3);        // Logs: Calling add with [5, 3], Result: 8
  calc.multiply(4, 7);   // Logs: Calling multiply with [4, 7], Result: 28
}
```

## Related

- [Type System](type-system.md) - Type system rules
- [Concurrency](concurrency.md) - Concurrency patterns
- [Syntax](syntax.md) - Language syntax
- [Standard Library](../stdlib/overview.md) - Available APIs
