# parser module

Frontend of the Raya compiler: lexer, parser, type system, and type checker.

## Module Structure

```
parser/
├── mod.rs         # Module entry, re-exports
├── token.rs       # Token definitions, Span
├── lexer.rs       # Tokenization
├── interner.rs    # String interning (Symbol)
├── ast/           # Abstract Syntax Tree
├── parser/        # Recursive descent parser
├── types/         # Type system representation
└── checker/       # Type checking and inference
```

## Submodules

### `token.rs`
Token definitions and source spans.
```rust
pub enum Token { ... }
pub struct Span { start, end, line, column }
```

### `lexer.rs`
Hand-written lexer (not generated).
```rust
Lexer::new(source) -> Lexer
lexer.tokenize() -> Result<(Vec<Token>, Interner), Vec<LexError>>
```

### `interner.rs`
String interning for memory efficiency.
```rust
interner.intern("foo") -> Symbol
interner.resolve(symbol) -> &str
```

### `ast/`
Complete AST definitions. See [ast/CLAUDE.md](ast/CLAUDE.md).

### `parser/`
Recursive descent parser. See [parser/CLAUDE.md](parser/CLAUDE.md).

### `types/`
Type system representation. See [types/CLAUDE.md](types/CLAUDE.md).

### `checker/`
Type checking and inference. See [checker/CLAUDE.md](checker/CLAUDE.md).

## Key Types

```rust
// Parsing
Parser::new(source) -> Result<Parser, LexError>
parser.parse() -> Result<(ast::Module, Interner), ParseError>

// Type checking
TypeChecker::new(&interner)
checker.check(&module) -> CheckResult
```

## Usage Flow

```rust
// 1. Parse source code
let parser = Parser::new(source)?;
let (module, interner) = parser.parse()?;

// 2. Type check
let mut checker = TypeChecker::new(&interner);
let result = checker.check(&module);

// 3. Handle errors
if result.has_errors() {
    for error in result.errors() {
        eprintln!("{}", error);
    }
}

// 4. Get type context for compiler
let type_ctx = result.type_context();
```

## For AI Assistants

- Lexer is hand-written for precise control
- Parser is recursive descent (not parser generator)
- Interner is used for all identifiers/strings
- Type checker produces `CheckResult` with errors + type context
- AST nodes include `Span` for error reporting
