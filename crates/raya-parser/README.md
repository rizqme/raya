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
- **Template Literals:** Backtick strings with `${expression}` interpolation
- **Unicode Escapes:** `\uXXXX` (4-digit) and `\u{XXXXXX}` (variable-length) support
- **Hex Escapes:** `\xXX` (2-digit) support
- **Error Recovery:** Continue tokenizing after errors to report multiple issues
- **Rich Error Messages:** Contextual error messages with source snippets and hints
- **Performance Benchmarks:** Comprehensive criterion-based benchmarks

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

Current test count: **64 tests** covering all token types, template literals, Unicode escapes, error recovery, and integration scenarios.

## Implementation Status

### ✅ Complete (Phases 1-6)

- [x] Token enum with all 41 keywords
- [x] All 43 operators
- [x] Span struct for source location tracking
- [x] Logos-based lexer
- [x] Numeric separator support (`1_000_000`)
- [x] String escape sequences (`\n`, `\t`, `\r`, `\\`, etc.)
- [x] Comment handling (`//` and `/* */`)
- [x] Template literal support with `${expression}` interpolation
- [x] Unicode escape sequences (`\uXXXX`, `\u{XXXXXX}`)
- [x] Hex escape sequences (`\xXX`)
- [x] Error recovery (continues after errors)
- [x] Rich error messages with source context and hints
- [x] Performance benchmarks (criterion)
- [x] 64 comprehensive tests

## Performance

The lexer is designed for high performance:

- **Zero-copy:** Keywords use string slices where possible
- **DFA-based:** Logos generates optimized state machines
- **Minimal allocations:** Tokens are created only when needed
- **Fast path:** Most common tokens handled in single clock cycles

Run benchmarks:
```bash
cargo bench -p raya-parser
```

Typical performance (on modern hardware):
- Keywords: ~230 ns
- Numbers: ~200-320 ns (depending on type)
- Strings: ~125-350 ns (depending on escapes)
- Template literals: ~400-600 ns (depending on expressions)
- Real code (100+ tokens): ~2-5 µs

## References

- [Milestone 2.1 Documentation](../../plans/milestone-2.1.md)
- [Raya Language Specification](../../design/LANG.md)

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))
- MIT License ([LICENSE-MIT](../../LICENSE-MIT))

at your option.
