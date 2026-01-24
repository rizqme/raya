# Raya Parser

Lexer and parser for the Raya programming language.

## Overview

This crate provides lexical analysis (tokenization) for Raya source code. It uses the `logos` library for high-performance tokenization with zero overhead.

## Features

- **Complete Token Set:** All 41 keywords and 43 operators from the Raya language specification
- **High Performance:** Logos-based DFA lexer with minimal allocations
- **Precise Source Tracking:** Line, column, and byte offset information for every token
- **Numeric Separators:** Support for `1_000_000`, `0xFF_FF`, `3.14_159`, etc.
- **Unicode Identifiers:** Full UTF-8 identifier support
- **Comment Handling:** Single-line (`//`) and multi-line (`/* */`) comments
- **Template Literals:** Backtick strings with expression interpolation (Phase 3)
- **Error Recovery:** Continue tokenizing after errors to report multiple issues

## Usage

```rust
use raya_parser::{Lexer, Token};

let source = r#"
    function add(a: number, b: number): number {
        return a + b;
    }
"#;

let lexer = Lexer::new(source);
match lexer.tokenize() {
    Ok(tokens) => {
        for (token, span) in tokens {
            println!("{:?} at {}:{}", token, span.line, span.column);
        }
    }
    Err(errors) => {
        for err in errors {
            eprintln!("{}", err);
        }
    }
}
```

## Token Types

### Keywords (41 total)

- **Variables:** `let`, `const`, `var`
- **Functions:** `function`, `async`, `await`, `return`
- **Control Flow:** `if`, `else`, `switch`, `case`, `default`, `for`, `while`, `do`, `break`, `continue`
- **OOP:** `class`, `new`, `this`, `super`, `static`, `extends`, `implements`, `interface`
- **Types:** `type`, `typeof`, `instanceof`, `void`, `enum`
- **Error Handling:** `try`, `catch`, `finally`, `throw`
- **Modules:** `import`, `export`, `from`
- **Future Reserved:** `namespace`, `private`, `protected`, `public`, `yield`, `in`
- **Debug:** `debugger`
- **Literals:** `true`, `false`, `null`

### Operators (43 total)

- **Arithmetic:** `+`, `-`, `*`, `/`, `%`, `**`
- **Unary:** `++`, `--`, `!`, `~`
- **Comparison:** `==`, `!=`, `===`, `!==`, `<`, `>`, `<=`, `>=`
- **Logical:** `&&`, `||`
- **Bitwise:** `&`, `|`, `^`, `~`, `<<`, `>>`, `>>>`
- **Assignment:** `=`, `+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`, `^=`, `<<=`, `>>=`, `>>>=`
- **Special:** `?.` (optional chaining), `??` (nullish coalescing), `=>` (arrow), `?:` (ternary)

### Literals

- **Numbers:** Integers, floats, hex, binary, octal, scientific notation
- **Strings:** Double-quoted, single-quoted, template literals
- **Booleans:** `true`, `false`
- **Null:** `null`

## Testing

Run tests:
```bash
cargo test -p raya-parser
```

Current test count: **39 tests** covering all token types and integration scenarios.

## Implementation Status

### ✅ Complete (Phase 1-2)

- [x] Token enum with all 41 keywords
- [x] All 43 operators
- [x] Span struct for source location tracking
- [x] Logos-based lexer
- [x] Numeric separator support
- [x] String escape sequences
- [x] Comment handling
- [x] 39 comprehensive tests

### 🚧 Pending (Phase 3-6)

- [ ] Template literal support with nested expressions
- [ ] Unicode escape sequences (\uXXXX, \u{XXXXXX})
- [ ] Error recovery and rich error messages
- [ ] Performance benchmarks
- [ ] Additional integration tests

## Performance

The lexer is designed for high performance:

- **Zero-copy:** Keywords use string slices where possible
- **DFA-based:** Logos generates optimized state machines
- **Minimal allocations:** Tokens are created only when needed
- **Fast path:** Most common tokens handled in single clock cycles

## References

- [Milestone 2.1 Documentation](../../plans/milestone-2.1.md)
- [Raya Language Specification](../../design/LANG.md)

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))
- MIT License ([LICENSE-MIT](../../LICENSE-MIT))

at your option.
