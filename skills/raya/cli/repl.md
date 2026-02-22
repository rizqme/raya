# REPL (Interactive Shell)

Raya's REPL provides an interactive environment for experimenting with code.

## Starting the REPL

```bash
raya repl
```

## Features

### Persistent Session

Declarations accumulate across evaluations:

```typescript
raya> const x = 42
raya> const y = x * 2
raya> logger.info(y)  // 84
```

### Multi-Line Input

Press Enter on incomplete statements:

```typescript
raya> function add(a: int, b: int): int {
...     return a + b;
... }
```

### History

Command history saved to `~/.raya/repl_history`:
- Up/Down arrows to navigate
- Ctrl-R for reverse search
- Persistent across sessions

### Auto-Wrapping

Bare expressions are automatically wrapped:

```typescript
raya> 1 + 2
// Internally: return 1 + 2;
```

## REPL Commands

Commands are invoked without a dot prefix:

### help

Show REPL help.

```typescript
raya> help
```

### clear

Clear the session (reset all declarations).

```typescript
raya> clear
```

### load <file>

Load and evaluate a file.

```typescript
raya> load mylib.raya
```

### type <expr>

Show the type of an expression.

```typescript
raya> type 42
int
raya> type "hello"
string
raya> type [1, 2, 3]
int[]
```

### exit

Exit the REPL.

```typescript
raya> exit
```

Or press Ctrl-D.

## Examples

### Quick Math

```typescript
raya> import math from "std:math"
raya> math.sqrt(16)
4
raya> math.PI
3.141592653589793
```

### Define Functions

```typescript
raya> function factorial(n: int): int {
...     if (n <= 1) return 1;
...     return n * factorial(n - 1);
... }
raya> factorial(5)
120
```

### Work with Classes

```typescript
raya> class Point {
...     x: number;
...     y: number;
...     constructor(x: number, y: number) {
...         this.x = x;
...         this.y = y;
...     }
... }
raya> const p = new Point(3, 4)
raya> p.x
3
```

### Test APIs

```typescript
raya> import crypto from "std:crypto"
raya> const uuid = crypto.randomUUID()
raya> logger.info(uuid)
```

## Keyboard Shortcuts

- **Ctrl-C** - Cancel current line
- **Ctrl-D** - Exit REPL
- **Ctrl-L** - Clear screen
- **Up/Down** - History navigation
- **Ctrl-R** - Reverse history search
- **Tab** - Auto-completion (planned)

## Tips

1. **Import once** - Imports persist across evaluations
2. **Use `type`** - Check types when unsure
3. **Load files** - Test modules with `load`
4. **Clear when stuck** - Reset with `clear`
5. **Multi-line** - Don't fear newlines

## Limitations

- No top-level await (use functions)
- Imports can't be redefined
- Limited error recovery

## Related

- [Commands](commands.md) - CLI commands
- [Package Manager](package-manager.md) - Dependencies
