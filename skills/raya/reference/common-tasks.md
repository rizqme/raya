# Common Tasks

Recipe-style guide for frequent Raya operations.

## Hello World

```typescript
import logger from "std:logger";

function main(): void {
  logger.info("Hello, Raya!");
}
```

## Read/Write Files

```typescript
import fs from "std:fs";

// Read text file
const content = fs.readTextFile("input.txt");

// Write text file
fs.writeTextFile("output.txt", "Hello, World!");

// Read binary file
const bytes = fs.readFile("image.png");

// Write binary file
fs.writeFile("copy.png", bytes);
```

## HTTP Server

```typescript
import logger from "std:logger";
import { HttpServer } from "std:http";

function main(): void {
  const server = new HttpServer("127.0.0.1", 8080);
  logger.info("Server listening on http://127.0.0.1:8080");
  
  server.serve((req) => {
    logger.info(req.method, req.path);
    
    return {
      status: 200,
      headers: { "Content-Type": "text/html" },
      body: "<h1>Hello from Raya!</h1>"
    };
  });
}
```

## HTTP Client

```typescript
import fetch from "std:fetch";
import logger from "std:logger";

function main(): void {
  const response = fetch("https://api.github.com/repos/rayalang/rayavm");
  logger.info("Status:", response.status);
  logger.info("Body:", response.body);
}
```

## JSON Parsing

```typescript
import { JSON } from "std:json";
import logger from "std:logger";

type User = {
  id: int;
  name: string;
  email: string;
};

function main(): void {
  const jsonStr = '{"id": 1, "name": "Alice", "email": "alice@example.com"}';
  
  const user = JSON.parse<User>(jsonStr);
  logger.info("User:", user.name, user.email);
  
  const output = JSON.stringify(user);
  logger.info("JSON:", output);
}
```

## Concurrent File Reads

```typescript
import fs from "std:fs";
import logger from "std:logger";

async function readFile(path: string): Task<string> {
  return fs.readTextFile(path);
}

function main(): void {
  // Start reading 3 files concurrently
  const t1 = readFile("file1.txt");
  const t2 = readFile("file2.txt");
  const t3 = readFile("file3.txt");
  
  // Collect results
  const c1 = await t1;
  const c2 = await t2;
  const c3 = await t3;
  
  logger.info("Total bytes:", c1.length + c2.length + c3.length);
}
```

## Process Arguments

```typescript
import args from "std:args";
import logger from "std:logger";

function main(): void {
  const parser = args.create()
    .option("--verbose", "-v", "Enable verbose output")
    .option("--output", "-o", "Output file", true)  // requires value
    .parse();
  
  if (parser.hasFlag("verbose")) {
    logger.info("Verbose mode enabled");
  }
  
  const output = parser.getValue("output") ?? "output.txt";
  logger.info("Output file:", output);
}
```

## Hash File

```typescript
import fs from "std:fs";
import crypto from "std:crypto";
import logger from "std:logger";

function main(): void {
  const content = fs.readTextFile("file.txt");
  const hash = crypto.hash("sha256", content);
  const hexHash = crypto.toHex(hash);
  
  logger.info("SHA-256:", hexHash);
}
```

## Measure Execution Time

```typescript
import time from "std:time";
import logger from "std:logger";

function main(): void {
  const start = time.monotonic();
  
  // Do work
  for (let i = 0; i < 1000000; i = i + 1) {
    // ...
  }
  
  const elapsed = time.elapsed(start);
  logger.info("Elapsed:", elapsed / 1000000, "ms");
}
```

## Create CLI Tool

```typescript
import args from "std:args";
import fs from "std:fs";
import logger from "std:logger";

function main(): void {
  const parser = args.create()
    .command("build", "Build project")
    .command("test", "Run tests")
    .option("--verbose", "-v", "Verbose output")
    .parse();
  
  const command = parser.getCommand();
  
  if (command == "build") {
    logger.info("Building project...");
    // Build logic
  } else if (command == "test") {
    logger.info("Running tests...");
    // Test logic
  } else {
    logger.error("Unknown command:", command);
  }
}
```

## Error Handling with Result Type

```typescript
import fs from "std:fs";
import logger from "std:logger";

type Result<T> =
  | { ok: true; value: T }
  | { ok: false; error: string };

function safeReadFile(path: string): Result<string> {
  try {
    const content = fs.readTextFile(path);
    return { ok: true, value: content };
  } catch (e) {
    return { ok: false, error: e.message };
  }
}

function main(): void {
  const result = safeReadFile("config.json");
  
  if (result.ok) {
    logger.info("Content:", result.value);
  } else {
    logger.error("Failed to read:", result.error);
  }
}
```

## Parallel Processing

```typescript
import logger from "std:logger";
import math from "std:math";

async function compute(id: int): Task<int> {
  // Simulate work
  let result = 0;
  for (let i = 0; i < 1000000; i = i + 1) {
    result = result + math.sqrt(i);
  }
  return id * 2;
}

function main(): void {
  const tasks: Task<int>[] = [];
  
  // Start 10 tasks
  for (let i = 0; i < 10; i = i + 1) {
    tasks.push(compute(i));
  }
  
  // Collect results
  let sum = 0;
  for (let i = 0; i < tasks.length; i = i + 1) {
    const result = await tasks[i];
    sum = sum + result;
  }
  
  logger.info("Sum:", sum);
}
```

## Compile & Run from String

```typescript
import { Compiler } from "std:runtime";
import logger from "std:logger";

function main(): void {
  const code = `
    function add(a: int, b: int): int {
      return a + b;
    }
    
    function main(): void {
      logger.info("Result:", add(2, 3));
    }
  `;
  
  const module = Compiler.compile(code);
  Compiler.execute(module);
}
```

## Related

- [Quick Reference](quick-reference.md) - FAQ and cheat sheet
- [Documentation](documentation.md) - Links to all docs
- [Examples](../language/examples.md) - Language examples
