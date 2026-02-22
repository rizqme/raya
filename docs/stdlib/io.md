# std:io

Standard input/output operations.

## Import

```typescript
import io from "std:io";
```

## Methods

### `writeln(text: string): void`
Write text to stdout with newline:

```typescript
io.writeln("Hello, World!");
io.writeln(`Count: ${count}`);
```

### `write(text: string): void`
Write text to stdout without newline:

```typescript
io.write("Loading");
io.write(".");
io.write(".");
io.writeln("done!");
```

### `readLine(): string`
Read a line from stdin (blocking):

```typescript
io.write("Enter name: ");
const name = io.readLine();
io.writeln(`Hello, ${name}!`);
```

### `readAll(): string`
Read all of stdin until EOF:

```typescript
const input = io.readAll();
io.writeln(`Read ${input.length} bytes`);
```

### `writeErr(text: string): void`
Write to stderr without newline:

```typescript
io.writeErr("Error: ");
```

### `writeErrln(text: string): void`
Write to stderr with newline:

```typescript
io.writeErrln("Fatal error occurred");
```

## Examples

### Interactive Input

```typescript
import io from "std:io";

function main(): void {
  io.write("What's your name? ");
  const name = io.readLine();
  
  io.write("How old are you? ");
  const age = io.readLine();
  
  io.writeln(`Hello ${name}, you are ${age} years old!`);
}
```

### Error Output

```typescript
import io from "std:io";

function processFile(path: string): void {
  if (!fs.exists(path)) {
    io.writeErrln(`Error: file not found: ${path}`);
    return;
  }
  // process file...
}
```

### Pipe Processing

```typescript
import io from "std:io";

// Read from stdin, process, write to stdout
function main(): void {
  const input = io.readAll();
  const lines = input.split("\n");
  
  for (const line of lines) {
    const processed = line.toUpperCase();
    io.writeln(processed);
  }
}
```

Run with: `echo "hello" | raya run process.raya`

## Notes

- All I/O operations are **blocking**
- Use `async` prefix for concurrent I/O
- stdin/stdout/stderr are buffered
- No formatting - use string interpolation
