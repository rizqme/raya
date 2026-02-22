# Getting Started

## Installation

Install Raya directly:

```bash
curl -fsSL https://raya.land/install.sh | sh
```

**Other options:**

```bash
# Install specific version
curl -fsSL https://raya.land/install.sh | sh -s -- --version v0.1.0

# Install to custom directory
INSTALL_DIR=/usr/local/bin curl -fsSL https://raya.land/install.sh | sh
```

Verify install:

```bash
raya --version
```

**Build from source** (for contributors):

```bash
git clone https://github.com/rizqme/raya.git
cd raya
cargo build --release -p raya-cli
```

## Your First Program

Create a file `hello.raya`:

```typescript
import io from "std:io";

function main(): void {
  io.writeln("Hello from Raya!");
}
```

Run it:

```bash
raya run hello.raya
```

## Concurrency Example

Create `concurrent.raya`:

```typescript
import io from "std:io";
import time from "std:time";

async function fetchData(id: int): Task<string> {
  time.sleep(100_000_000); // 100ms in nanoseconds
  return `Data ${id}`;
}

function main(): void {
  const start = time.monotonic();
  
  // Start 10 concurrent tasks
  const tasks: Task<string>[] = [];
  for (let i = 0; i < 10; i++) {
    tasks.push(fetchData(i));
  }
  
  // Wait for all tasks
  for (const task of tasks) {
    io.writeln(await task);
  }
  
  const elapsed = time.elapsed(start) / 1_000_000;
  io.writeln(`Took ${elapsed}ms`);
}
```

Run it:

```bash
raya run concurrent.raya
```

All 10 tasks run concurrently on a work-stealing scheduler. On a multi-core machine, you'll see near-linear speedup.

## Type Safety Example

Raya's type system catches errors at compile time:

```typescript
import logger from "std:logger";

type Result<T> =
  | { status: "ok"; value: T }
  | { status: "error"; error: string };

function divide(a: number, b: number): Result<number> {
  if (b == 0) {
    return { status: "error", error: "Division by zero" };
  }
  return { status: "ok", value: a / b };
}

function main(): void {
  const result = divide(10, 2);
  
  // Compiler enforces exhaustive checking
  if (result.status == "ok") {
    logger.info("Result:", result.value);
  } else {
    logger.error(result.error);
  }
}
```

Try to access `result.value` without checking `status` - the compiler will reject it.

## CLI Commands

```bash
# Run a script
raya run script.raya

# Type-check without running
raya check script.raya

# Compile to bytecode
raya build script.raya -o output.rbc

# Start REPL
raya repl

# Evaluate expression
raya eval "1 + 2"

# Package manager
raya init              # Initialize project
raya add <package>     # Add dependency
raya install           # Install dependencies
```

## Project Structure

Initialize a new project:

```bash
raya init my-project
cd my-project
```

This creates:

```
my-project/
├── raya.toml          # Project config
├── src/
│   └── main.raya      # Entry point
└── tests/             # Test files
```

`raya.toml`:

```toml
[package]
name = "my-project"
version = "0.1.0"
entry = "src/main.raya"

[dependencies]
# Add packages here

[scripts]
dev = "run src/main.raya"
```

## Next Steps

- [Type System](/language/types) - Learn about Raya's static types
- [Concurrency](/language/concurrency) - Deep dive into Tasks and goroutines
- [Standard Library](https://github.com/rizqme/raya/tree/main/docs) - Explore available modules

## Getting Help

- **Issues:** [github.com/rizqme/raya/issues](https://github.com/rizqme/raya/issues)
- **Discussions:** [github.com/rizqme/raya/discussions](https://github.com/rizqme/raya/discussions)

::: warning Early Project
Raya is in active development. APIs may change. Not recommended for production use yet.
:::
