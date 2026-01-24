# Milestone 2.1: Lexer (Tokenization)

**Status:** ✅ Complete
**Priority:** High
**Dependencies:** Milestone 1.x (VM Core complete)
**Actual Duration:** Completed
**Completion Date:** 2026-01-24

**Scope:**
- Token type definitions for all Raya language constructs
- High-performance lexer implementation using logos
- Source location tracking for error reporting
- String template support (template literals)
- Comment handling (single-line and multi-line)
- Unicode identifier support
- Comprehensive test coverage

---

## Overview

Implement the lexical analysis phase of the Raya compiler. The lexer converts raw source code into a stream of tokens that will be consumed by the parser in Milestone 2.3.

**Architecture:**
```
Source Code (String)
    ↓
Lexer (logos-based)
    ↓
Token Stream (Vec<(Token, Span)>)
    ↓
Parser (Milestone 2.3)
```

**Key Design Principles:**
- **Performance:** Use logos for efficient tokenization with minimal allocations
- **Error Recovery:** Continue tokenizing after errors to report multiple issues
- **Source Tracking:** Precise span information for every token (line, column, byte offset)
- **Unicode Support:** Proper handling of UTF-8 identifiers and string literals
- **TypeScript Compatibility:** Token set matches Raya's TypeScript subset

---

## Goals & Checkboxes

### [ ] Phase 1: Token Type Definitions (Days 1-2)

**Core Token Types:**
- [ ] Define Token enum with all language constructs
- [ ] Keywords (function, class, interface, type, let, const, etc.)
- [ ] Operators (arithmetic, comparison, logical, bitwise, assignment)
- [ ] Delimiters (braces, brackets, parentheses, semicolons, commas)
- [ ] Literals (numbers, strings, booleans, null, undefined)
- [ ] Identifiers (variables, function names, type names)
- [ ] Comments (single-line //, multi-line /* */)
- [ ] Special tokens (EOF, Error, Newline)

**Token Categories:**
```rust
pub enum Token {
    // Keywords (41 tokens)
    // Core keywords
    Function, Class, Interface, Type, Enum,
    Let, Const, Var,

    // Control flow
    If, Else, Switch, Case, Default,
    For, While, Do, Break, Continue, Return,

    // Async/Error handling
    Async, Await, Try, Catch, Finally, Throw,

    // Modules
    Import, Export, From,

    // OOP keywords
    New, This, Super, Static, Extends, Implements,

    // Type operators
    Typeof, Instanceof, Void,

    // Utility/Debug
    Debugger,

    // Future reserved (for compatibility)
    Namespace, Private, Protected, Public, Yield, In,

    // Literals (7 tokens)
    IntLiteral(i64),           // 42, 0x1F, 0b1010, 0o77
    FloatLiteral(f64),         // 3.14, 1.0e10, .5
    StringLiteral(String),     // "hello", 'world'
    TemplateLiteral(Vec<TemplatePart>),  // `hello ${name}`
    True,
    False,
    Null,

    // Identifiers & Types
    Identifier(String),        // foo, myVar, _internal

    // Operators (43 tokens)
    // Arithmetic
    Plus, Minus, Star, Slash, Percent,
    PlusPlus, MinusMinus, StarStar,  // Exponentiation **

    // Comparison
    EqualEqual, BangEqual,
    Less, LessEqual, Greater, GreaterEqual,
    EqualEqualEqual, BangEqualEqual,

    // Logical
    AmpAmp, PipePipe, Bang,

    // Bitwise
    Amp, Pipe, Caret, Tilde,
    LessLess, GreaterGreater, GreaterGreaterGreater,

    // Assignment
    Equal,
    PlusEqual, MinusEqual, StarEqual, SlashEqual, PercentEqual,
    AmpEqual, PipeEqual, CaretEqual,
    LessLessEqual, GreaterGreaterEqual, GreaterGreaterGreaterEqual,

    // Other
    Question, QuestionQuestion, QuestionDot,  // Optional chaining
    Dot, Colon, Arrow,

    // Delimiters
    LeftParen, RightParen,
    LeftBrace, RightBrace,
    LeftBracket, RightBracket,
    Semicolon, Comma,

    // Special
    Eof,
    Error(String),
}
```

**Source Location:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,    // Byte offset in source
    pub end: usize,      // Byte offset (exclusive)
    pub line: u32,       // Line number (1-indexed)
    pub column: u32,     // Column number (1-indexed)
}

impl Span {
    pub fn new(start: usize, end: usize, line: u32, column: u32) -> Self;
    pub fn len(&self) -> usize { self.end - self.start }
    pub fn slice<'a>(&self, source: &'a str) -> &'a str;
    pub fn merge(&self, other: &Span) -> Span;  // Combine two spans
}
```

**Complete Keyword List (41 keywords):**

| Category | Keywords |
|----------|----------|
| **Variables** | `let`, `const`, `var` (banned) |
| **Functions** | `function`, `async`, `await`, `return`, `yield` (future) |
| **Control Flow** | `if`, `else`, `switch`, `case`, `default`, `for`, `while`, `do`, `break`, `continue` |
| **OOP** | `class`, `new`, `this`, `super`, `static`, `extends`, `implements`, `interface` |
| **Types** | `type`, `typeof`, `instanceof`, `void`, `enum` (future) |
| **Error Handling** | `try`, `catch`, `finally`, `throw` |
| **Modules** | `import`, `export`, `from` |
| **Access Modifiers** (future) | `private`, `protected`, `public` |
| **Other** | `debugger`, `in`, `namespace` (future) |
| **Literals** | `true`, `false`, `null` |

**Complete Operator List (43 operators):**

| Category | Operators |
|----------|-----------|
| **Arithmetic** | `+`, `-`, `*`, `/`, `%`, `**` (exponentiation) |
| **Unary** | `++`, `--`, `!`, `~`, `+`, `-` |
| **Comparison** | `==`, `!=`, `===`, `!==`, `<`, `>`, `<=`, `>=` |
| **Logical** | `&&`, `\|\|`, `!` |
| **Bitwise** | `&`, `\|`, `^`, `~`, `<<`, `>>`, `>>>` |
| **Assignment** | `=`, `+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `\|=`, `^=`, `<<=`, `>>=`, `>>>=` |
| **Special** | `?.` (optional chain), `??` (nullish coalescing), `?:` (ternary), `=>` (arrow) |
| **Member** | `.`, `[`, `]` |

**Deliverables:**
- [ ] `raya-parser/src/token.rs` - Token enum and Span struct (41 keywords, 43 operators)
- [ ] Token display implementation (for debugging)
- [ ] Token classification helpers (is_keyword, is_operator, is_literal, etc.)
- [ ] Unit tests for Span operations
- [ ] Documentation of all keywords and their usage

---

### [ ] Phase 2: Lexer Implementation with Logos (Days 3-5)

**Lexer Structure:**
```rust
use logos::Logos;

#[derive(Logos, Debug, Clone, PartialEq)]
pub enum Token {
    // Whitespace (skip)
    #[regex(r"[ \t\r\n]+", logos::skip)]
    _Whitespace,

    // Comments (skip or preserve for doc comments)
    #[regex(r"//[^\n]*", logos::skip)]
    _LineComment,

    #[regex(r"/\*[^*]*\*+(?:[^/*][^*]*\*+)*/", logos::skip)]
    _BlockComment,

    // Keywords
    #[token("function")]
    Function,

    #[token("class")]
    Class,

    #[token("let")]
    Let,

    #[token("const")]
    Const,

    #[token("switch")]
    Switch,

    #[token("case")]
    Case,

    #[token("default")]
    Default,

    #[token("new")]
    New,

    #[token("this")]
    This,

    #[token("super")]
    Super,

    #[token("static")]
    Static,

    #[token("extends")]
    Extends,

    #[token("implements")]
    Implements,

    #[token("typeof")]
    Typeof,

    #[token("instanceof")]
    Instanceof,

    #[token("void")]
    Void,

    #[token("debugger")]
    Debugger,

    #[token("do")]
    Do,

    #[token("in")]
    In,

    #[token("yield")]
    Yield,

    #[token("var")]
    Var,

    #[token("enum")]
    Enum,

    #[token("namespace")]
    Namespace,

    #[token("private")]
    Private,

    #[token("protected")]
    Protected,

    #[token("public")]
    Public,

    // Identifiers (must come after keywords)
    #[regex(r"[a-zA-Z_$][a-zA-Z0-9_$]*", |lex| lex.slice().to_string())]
    Identifier(String),

    // Numbers (with numeric separator support: 1_000_000)
    #[regex(r"0x[0-9a-fA-F]+(_[0-9a-fA-F]+)*", parse_hex)]
    #[regex(r"0b[01]+(_[01]+)*", parse_binary)]
    #[regex(r"0o[0-7]+(_[0-7]+)*", parse_octal)]
    #[regex(r"[0-9]+(_[0-9]+)*", parse_int)]
    IntLiteral(i64),

    #[regex(r"[0-9]+(_[0-9]+)*\.[0-9]+(_[0-9]+)*([eE][+-]?[0-9]+(_[0-9]+)*)?", parse_float)]
    #[regex(r"[0-9]+(_[0-9]+)*[eE][+-]?[0-9]+(_[0-9]+)*", parse_float)]
    #[regex(r"\.[0-9]+(_[0-9]+)*([eE][+-]?[0-9]+(_[0-9]+)*)?", parse_float)]
    FloatLiteral(f64),

    // Strings
    #[regex(r#""([^"\\]|\\.)*""#, parse_string)]
    #[regex(r#"'([^'\\]|\\.)*'"#, parse_string)]
    StringLiteral(String),

    // Template literals (handled specially)
    #[token("`")]
    BacktickStart,

    // Operators (2-char and 3-char must come before 1-char)
    #[token("===")]
    EqualEqualEqual,

    #[token("!==")]
    BangEqualEqual,

    #[token("**")]
    StarStar,

    #[token("==")]
    EqualEqual,

    #[token("!=")]
    BangEqual,

    #[token("<=")]
    LessEqual,

    #[token(">=")]
    GreaterEqual,

    #[token("&&")]
    AmpAmp,

    #[token("||")]
    PipePipe,

    #[token("++")]
    PlusPlus,

    #[token("--")]
    MinusMinus,

    #[token("<<")]
    LessLess,

    #[token(">>")]
    GreaterGreater,

    #[token(">>>")]
    GreaterGreaterGreater,

    #[token("?.")]
    QuestionDot,

    #[token("??")]
    QuestionQuestion,

    #[token("=>")]
    Arrow,

    // ... all compound operators

    // Single-character tokens
    #[token("+")]
    Plus,

    #[token("-")]
    Minus,

    #[token("*")]
    Star,

    #[token("/")]
    Slash,

    // ... all single-char operators

    #[token("(")]
    LeftParen,

    #[token(")")]
    RightParen,

    // ... all delimiters

    // Error handling
    #[error]
    Error,
}

// Helper parsing functions
fn parse_hex(lex: &mut logos::Lexer<Token>) -> Option<i64> {
    let s = lex.slice()[2..].replace('_', "");  // Remove underscores
    i64::from_str_radix(&s, 16).ok()
}

fn parse_binary(lex: &mut logos::Lexer<Token>) -> Option<i64> {
    let s = lex.slice()[2..].replace('_', "");
    i64::from_str_radix(&s, 2).ok()
}

fn parse_octal(lex: &mut logos::Lexer<Token>) -> Option<i64> {
    let s = lex.slice()[2..].replace('_', "");
    i64::from_str_radix(&s, 8).ok()
}

fn parse_int(lex: &mut logos::Lexer<Token>) -> Option<i64> {
    lex.slice().replace('_', "").parse().ok()
}

fn parse_float(lex: &mut logos::Lexer<Token>) -> Option<f64> {
    lex.slice().replace('_', "").parse().ok()
}

fn parse_string(lex: &mut logos::Lexer<Token>) -> Option<String> {
    let s = lex.slice();
    let inner = &s[1..s.len()-1];  // Remove quotes
    Some(unescape_string(inner))
}

fn unescape_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('r') => result.push('\r'),
                Some('t') => result.push('\t'),
                Some('\\') => result.push('\\'),
                Some('"') => result.push('"'),
                Some('\'') => result.push('\''),
                Some('0') => result.push('\0'),
                Some('x') => { /* hex escape */ }
                Some('u') => { /* unicode escape */ }
                Some(c) => result.push(c),
                None => break,
            }
        } else {
            result.push(c);
        }
    }

    result
}
```

**Lexer Wrapper:**
```rust
pub struct Lexer<'a> {
    source: &'a str,
    lexer: logos::Lexer<'a, Token>,
    line: u32,
    column: u32,
    byte_offset: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            lexer: Token::lexer(source),
            line: 1,
            column: 1,
            byte_offset: 0,
        }
    }

    pub fn tokenize(mut self) -> Result<Vec<(Token, Span)>, LexError> {
        let mut tokens = Vec::new();

        while let Some(token) = self.lexer.next() {
            let span = self.current_span();

            match token {
                Ok(tok) => tokens.push((tok, span)),
                Err(_) => {
                    let slice = self.lexer.slice();
                    return Err(LexError::UnexpectedCharacter {
                        char: slice.chars().next().unwrap(),
                        span,
                    });
                }
            }

            self.update_position();
        }

        // Add EOF token
        let eof_span = Span::new(
            self.byte_offset,
            self.byte_offset,
            self.line,
            self.column,
        );
        tokens.push((Token::Eof, eof_span));

        Ok(tokens)
    }

    fn current_span(&self) -> Span {
        let range = self.lexer.span();
        Span::new(range.start, range.end, self.line, self.column)
    }

    fn update_position(&mut self) {
        let slice = self.lexer.slice();
        for c in slice.chars() {
            if c == '\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
        }
        self.byte_offset = self.lexer.span().end;
    }
}

#[derive(Debug)]
pub enum LexError {
    UnexpectedCharacter { char: char, span: Span },
    UnterminatedString { span: Span },
    InvalidEscape { escape: String, span: Span },
    InvalidNumber { text: String, span: Span },
}
```

**Tasks:**
- [ ] Implement Token enum with logos macros
- [ ] Implement all parsing helper functions
- [ ] Implement string escape sequence handling
- [ ] Implement Lexer wrapper with position tracking
- [ ] Handle edge cases (EOF, empty source, etc.)
- [ ] Comprehensive error messages

**Deliverables:**
- [ ] `raya-parser/src/lexer.rs` - Complete lexer implementation
- [ ] Line/column tracking for error reporting
- [ ] Support for all Raya token types
- [ ] 50+ unit tests covering all token types

---

### [ ] Phase 3: Template Literal Support (Days 6-7)

**Template Literal Structure:**
```rust
#[derive(Debug, Clone, PartialEq)]
pub enum TemplatePart {
    String(String),        // Raw string part
    Expression(Vec<Token>), // Tokenized ${...} expression
}

pub struct TemplateLexer<'a> {
    source: &'a str,
    pos: usize,
}

impl<'a> TemplateLexer<'a> {
    pub fn lex_template(&mut self) -> Result<Vec<TemplatePart>, LexError> {
        let mut parts = Vec::new();
        let mut current_string = String::new();

        while self.pos < self.source.len() {
            match self.current_char() {
                '`' => {
                    // End of template
                    if !current_string.is_empty() {
                        parts.push(TemplatePart::String(current_string));
                    }
                    break;
                }
                '$' if self.peek_char() == Some('{') => {
                    // Start of expression
                    if !current_string.is_empty() {
                        parts.push(TemplatePart::String(current_string.clone()));
                        current_string.clear();
                    }

                    self.pos += 2; // Skip ${
                    let expr = self.lex_expression()?;
                    parts.push(TemplatePart::Expression(expr));
                }
                '\\' => {
                    // Escape sequence
                    self.pos += 1;
                    let escaped = self.parse_escape()?;
                    current_string.push(escaped);
                }
                c => {
                    current_string.push(c);
                    self.pos += 1;
                }
            }
        }

        Ok(parts)
    }

    fn lex_expression(&mut self) -> Result<Vec<Token>, LexError> {
        let start = self.pos;
        let mut depth = 1;

        // Find matching }
        while depth > 0 && self.pos < self.source.len() {
            match self.current_char() {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
            self.pos += 1;
        }

        if depth != 0 {
            return Err(LexError::UnterminatedExpression);
        }

        // Tokenize the expression
        let expr_source = &self.source[start..self.pos - 1];
        let lexer = Lexer::new(expr_source);
        lexer.tokenize()
            .map(|tokens| tokens.into_iter().map(|(tok, _)| tok).collect())
    }
}
```

**Example:**
```typescript
const name = "World";
const greeting = `Hello, ${name}!`;
// Tokens: TemplateLiteral([
//   String("Hello, "),
//   Expression([Identifier("name")]),
//   String("!")
// ])
```

**Tasks:**
- [ ] Implement TemplateLexer for backtick strings
- [ ] Handle nested braces in expressions
- [ ] Support escape sequences in templates
- [ ] Handle multi-line templates
- [ ] Integrate with main lexer

**Deliverables:**
- [ ] Template literal tokenization
- [ ] 20+ tests for template literals
- [ ] Support for nested expressions

---

### [ ] Phase 4: Unicode & Identifier Support (Day 8)

**Unicode Identifiers:**
```rust
// Support Unicode identifiers (like JavaScript)
// ✅ Valid: café, 你好, Привет, _test, $var
// ❌ Invalid: 123abc, -var, @name

fn is_identifier_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '$'
}

fn is_identifier_continue(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

// Updated regex for identifiers
#[regex(r"[\p{L}_$][\p{L}\p{N}_$]*", |lex| lex.slice().to_string())]
Identifier(String),
```

**Unicode Escape Sequences:**
```rust
// Support \uXXXX and \u{XXXXXX} in strings
fn parse_unicode_escape(chars: &mut impl Iterator<Item = char>) -> Result<char, LexError> {
    if chars.next() == Some('{') {
        // \u{XXXXXX} format
        let mut hex = String::new();
        loop {
            match chars.next() {
                Some('}') => break,
                Some(c) if c.is_ascii_hexdigit() => hex.push(c),
                _ => return Err(LexError::InvalidUnicodeEscape),
            }
        }
        let code = u32::from_str_radix(&hex, 16)
            .map_err(|_| LexError::InvalidUnicodeEscape)?;
        char::from_u32(code).ok_or(LexError::InvalidUnicodeEscape)
    } else {
        // \uXXXX format (4 hex digits)
        let hex: String = chars.take(4).collect();
        if hex.len() != 4 {
            return Err(LexError::InvalidUnicodeEscape);
        }
        let code = u16::from_str_radix(&hex, 16)
            .map_err(|_| LexError::InvalidUnicodeEscape)?;
        Ok(char::from(code))
    }
}
```

**Tasks:**
- [ ] Support Unicode identifiers (using Unicode regex \p{L})
- [ ] Implement \uXXXX escape sequences
- [ ] Implement \u{XXXXXX} escape sequences
- [ ] Handle invalid UTF-8 gracefully
- [ ] Test with non-ASCII source files

**Deliverables:**
- [ ] Full Unicode identifier support
- [ ] Unicode escape sequences in strings
- [ ] 15+ tests with Unicode characters

---

### [ ] Phase 5: Error Recovery & Reporting (Day 9)

**Error Recovery:**
```rust
pub struct LexerWithRecovery<'a> {
    lexer: Lexer<'a>,
    errors: Vec<LexError>,
}

impl<'a> LexerWithRecovery<'a> {
    pub fn tokenize_with_recovery(mut self) -> (Vec<(Token, Span)>, Vec<LexError>) {
        let mut tokens = Vec::new();

        while let Some(result) = self.lexer.next() {
            match result {
                Ok(token) => tokens.push((token, self.lexer.current_span())),
                Err(err) => {
                    self.errors.push(err);
                    // Skip the problematic character and continue
                    self.lexer.skip_one();
                }
            }
        }

        (tokens, self.errors)
    }
}
```

**Error Messages:**
```rust
impl LexError {
    pub fn format(&self, source: &str) -> String {
        match self {
            LexError::UnexpectedCharacter { char, span } => {
                format!(
                    "Unexpected character '{}' at {}:{}\n{}",
                    char,
                    span.line,
                    span.column,
                    self.format_source_context(source, span)
                )
            }
            LexError::UnterminatedString { span } => {
                format!(
                    "Unterminated string literal at {}:{}\n{}",
                    span.line,
                    span.column,
                    self.format_source_context(source, span)
                )
            }
            // ... other error types
        }
    }

    fn format_source_context(&self, source: &str, span: &Span) -> String {
        let line = source.lines().nth((span.line - 1) as usize).unwrap();
        format!(
            "{}\n{}^\n",
            line,
            " ".repeat((span.column - 1) as usize)
        )
    }
}
```

**Tasks:**
- [ ] Implement error recovery (continue after errors)
- [ ] Collect multiple errors instead of failing fast
- [ ] Format error messages with source context
- [ ] Add suggestions for common mistakes
- [ ] Test error reporting with invalid input

**Deliverables:**
- [ ] Error recovery mechanism
- [ ] Rich error messages with source context
- [ ] 20+ error handling tests

---

### [ ] Phase 6: Testing & Documentation (Day 10)

**Test Categories:**

**1. Basic Tokens:**
```rust
#[test]
fn test_keywords() {
    assert_tokens("function class let const", vec![
        Token::Function,
        Token::Class,
        Token::Let,
        Token::Const,
        Token::Eof,
    ]);
}

#[test]
fn test_operators() {
    assert_tokens("+ - * / == != < > && ||", vec![
        Token::Plus,
        Token::Minus,
        Token::Star,
        Token::Slash,
        Token::EqualEqual,
        Token::BangEqual,
        Token::Less,
        Token::Greater,
        Token::AmpAmp,
        Token::PipePipe,
        Token::Eof,
    ]);
}
```

**2. Literals:**
```rust
#[test]
fn test_numbers() {
    assert_tokens("42 3.14 0x1F 0b1010 0o77", vec![
        Token::IntLiteral(42),
        Token::FloatLiteral(3.14),
        Token::IntLiteral(31),
        Token::IntLiteral(10),
        Token::IntLiteral(63),
        Token::Eof,
    ]);
}

#[test]
fn test_strings() {
    assert_tokens(r#""hello" 'world' "\n\t""#, vec![
        Token::StringLiteral("hello".to_string()),
        Token::StringLiteral("world".to_string()),
        Token::StringLiteral("\n\t".to_string()),
        Token::Eof,
    ]);
}
```

**3. Complex Cases:**
```rust
#[test]
fn test_template_literals() {
    let source = r#"`Hello, ${name}!`"#;
    let tokens = lex(source);
    // ... verify TemplateLiteral structure
}

#[test]
fn test_unicode_identifiers() {
    assert_tokens("café 你好 Привет", vec![
        Token::Identifier("café".to_string()),
        Token::Identifier("你好".to_string()),
        Token::Identifier("Привет".to_string()),
        Token::Eof,
    ]);
}

#[test]
fn test_real_program() {
    let source = r#"
        function add(a: number, b: number): number {
            return a + b;
        }
    "#;
    // Verify all tokens are correct
}
```

**4. Error Cases:**
```rust
#[test]
fn test_unterminated_string() {
    let result = lex(r#""hello"#);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), LexError::UnterminatedString { .. }));
}

#[test]
fn test_invalid_number() {
    let result = lex("0x");  // Invalid hex literal
    assert!(result.is_err());
}
```

**Documentation:**
- [ ] Module-level documentation explaining lexer design
- [ ] Examples of token usage
- [ ] Error handling guide
- [ ] Performance characteristics
- [ ] Unicode support documentation

**Benchmarks:**
```rust
#[bench]
fn bench_lex_small_file(b: &mut Bencher) {
    let source = include_str!("../benches/small.raya");
    b.iter(|| {
        let lexer = Lexer::new(source);
        lexer.tokenize()
    });
}

#[bench]
fn bench_lex_large_file(b: &mut Bencher) {
    let source = include_str!("../benches/large.raya");
    b.iter(|| {
        let lexer = Lexer::new(source);
        lexer.tokenize()
    });
}
```

**Additional Test Cases:**

**Switch Statements:**
```rust
#[test]
fn test_switch_statement() {
    let source = r#"
        switch (value) {
            case 1: break;
            case 2: break;
            default: break;
        }
    "#;
    let tokens = lex(source);
    // Verify: Switch, LeftParen, Identifier, RightParen, LeftBrace,
    //         Case, IntLiteral(1), Colon, Break, Semicolon, ...
}
```

**Labels:**
```rust
#[test]
fn test_labeled_statement() {
    let source = "outer: for (const i of items) { break outer; }";
    let tokens = lex(source);
    // Verify: Identifier("outer"), Colon, For, ...
    // Note: Labels are just identifiers followed by colon
}
```

**Exponentiation:**
```rust
#[test]
fn test_exponentiation() {
    assert_tokens("2 ** 3", vec![
        Token::IntLiteral(2),
        Token::StarStar,
        Token::IntLiteral(3),
        Token::Eof,
    ]);
}
```

**Numeric Separators:**
```rust
#[test]
fn test_numeric_separators() {
    assert_tokens("1_000_000 3.14_159", vec![
        Token::IntLiteral(1000000),
        Token::FloatLiteral(3.14159),
        Token::Eof,
    ]);
}
```

**Tasks:**
- [ ] Write 120+ unit tests covering all token types (up from 100)
- [ ] Test error cases and edge cases
- [ ] Test all 41 keywords
- [ ] Test all 43 operators
- [ ] Test switch/case/default statements
- [ ] Test labeled statements
- [ ] Test numeric separators
- [ ] Test exponentiation operator
- [ ] Write integration tests with real Raya programs
- [ ] Add benchmarks for performance tracking
- [ ] Document all public APIs
- [ ] Write user guide for lexer usage

**Deliverables:**
- [ ] 100+ passing tests (>95% code coverage)
- [ ] Comprehensive documentation
- [ ] Performance benchmarks
- [ ] Example programs demonstrating lexer usage

---

## File Structure

```
crates/raya-parser/
├── Cargo.toml
├── src/
│   ├── lib.rs              # Re-exports
│   ├── token.rs            # Token enum, Span struct
│   ├── lexer.rs            # Main lexer implementation
│   ├── template.rs         # Template literal handling
│   ├── unicode.rs          # Unicode utilities
│   └── error.rs            # Error types and formatting
├── tests/
│   ├── tokens.rs           # Basic token tests
│   ├── literals.rs         # Literal tests
│   ├── templates.rs        # Template literal tests
│   ├── unicode.rs          # Unicode tests
│   ├── errors.rs           # Error handling tests
│   └── integration.rs      # End-to-end tests
├── benches/
│   ├── lexer.rs            # Performance benchmarks
│   ├── small.raya          # Small test file
│   └── large.raya          # Large test file
└── README.md               # Lexer documentation
```

---

## Dependencies

```toml
[dependencies]
logos = "0.13"              # Lexer generator
unicode-xid = "0.2"         # Unicode identifier support

[dev-dependencies]
criterion = "0.5"           # Benchmarking
```

---

## Success Criteria

### Must Have

- [ ] Complete Token enum with ALL Raya language constructs (41 keywords, 43 operators)
- [ ] Lexer using logos for efficient tokenization
- [ ] Precise source location tracking (line, column, byte offset)
- [ ] **All 41 keyword tokens:**
  - [ ] Variables: `let`, `const`, `var`
  - [ ] Functions: `function`, `async`, `await`, `return`
  - [ ] Control flow: `if`, `else`, `switch`, `case`, `default`, `for`, `while`, `do`, `break`, `continue`
  - [ ] OOP: `class`, `new`, `this`, `super`, `static`, `extends`, `implements`, `interface`
  - [ ] Types: `type`, `typeof`, `instanceof`, `void`
  - [ ] Error handling: `try`, `catch`, `finally`, `throw`
  - [ ] Modules: `import`, `export`, `from`
  - [ ] Future reserved: `enum`, `namespace`, `private`, `protected`, `public`, `yield`, `in`
  - [ ] Literals: `true`, `false`, `null`
  - [ ] Debug: `debugger`
- [ ] **All 43 operator tokens:**
  - [ ] Arithmetic: `+`, `-`, `*`, `/`, `%`, `**` (exponentiation)
  - [ ] Unary: `++`, `--`, `!`, `~`
  - [ ] Comparison: `==`, `!=`, `===`, `!==`, `<`, `>`, `<=`, `>=`
  - [ ] Logical: `&&`, `||`, `!`
  - [ ] Bitwise: `&`, `|`, `^`, `~`, `<<`, `>>`, `>>>`
  - [ ] Assignment: `=`, `+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`, `^=`, `<<=`, `>>=`, `>>>=`
  - [ ] Special: `?.`, `??`, `?:`, `=>`
- [ ] All literal tokens (numbers, strings, booleans, null)
- [ ] Numeric separator support (`1_000_000`)
- [ ] Identifier tokenization (Unicode support)
- [ ] Comment handling (single-line `//` and multi-line `/* */`)
- [ ] String literal parsing with escape sequences
- [ ] Template literal support with nested expressions
- [ ] Unicode identifier support (café, 你好, etc.)
- [ ] Error recovery (continue after errors)
- [ ] Rich error messages with source context
- [ ] 120+ comprehensive tests (>95% coverage)
- [ ] Documentation for all public APIs

### Should Have

- [ ] Unicode escape sequences (\uXXXX, \u{XXXXXX})
- [ ] Performance benchmarks
- [ ] Fuzzing tests for robustness
- [ ] Integration tests with real Raya programs
- [ ] Support for different number formats (hex, binary, octal)
- [ ] Proper handling of edge cases (EOF, empty files, etc.)

### Nice to Have

- [ ] Syntax highlighting hints (token categories)
- [ ] Pretty-printer for token streams
- [ ] Token stream serialization/deserialization
- [ ] Incremental lexing for IDE support

---

## Performance Targets

- **Small files (<1KB):** <100μs
- **Medium files (<100KB):** <10ms
- **Large files (<1MB):** <100ms
- **Memory overhead:** <2x source size
- **Zero-copy:** String slices where possible (identifiers, keywords)

---

## Testing Strategy

### Unit Tests (80 tests)
- Token type definitions (10 tests)
- Keyword tokenization (27 tests)
- Operator tokenization (40 tests)
- Literal tokenization (15 tests)
- Identifier tokenization (5 tests)
- Comment handling (3 tests)

### Integration Tests (30 tests)
- Template literals (10 tests)
- Unicode identifiers (5 tests)
- Error recovery (10 tests)
- Real Raya programs (5 tests)

### Benchmark Tests (5 benchmarks)
- Small file lexing
- Large file lexing
- Template literal parsing
- Unicode identifier parsing
- Error recovery overhead

---

## Design Decisions

### Why logos?

**Pros:**
- **Performance:** Generates DFA-based lexers with zero overhead
- **Ergonomics:** Declarative macro-based API
- **Safety:** Compile-time checked regex patterns
- **Zero-copy:** Returns slices when possible

**Cons:**
- Compile-time overhead (acceptable for our use case)
- Less flexible than hand-written lexers (not an issue for Raya)

**Alternative Considered:** Hand-written lexer
- More flexible but significantly more code
- Higher maintenance burden
- Not faster for our token set

**Decision:** Use logos for simplicity and performance.

---

### Why preserve source locations?

**Critical for:**
- Precise error messages in parser/type checker
- IDE features (go-to-definition, hover info)
- Debugger integration (breakpoints)
- Source maps for bytecode

**Cost:** ~16 bytes per token (4 usizes)
**Benefit:** Essential for developer experience

---

### Why skip comments?

**Rationale:**
- Comments don't affect program semantics
- Parser doesn't need them
- Reduces token stream size

**Exception:** Doc comments (///, /** */)
- Needed for documentation generation
- Can be preserved as special tokens if needed in future

---

## Risks and Mitigations

### Risk 1: Unicode Edge Cases
**Impact:** Medium
**Probability:** Medium
**Mitigation:**
- Comprehensive Unicode tests
- Use battle-tested unicode-xid crate
- Test with real-world multilingual code

### Risk 2: Template Literal Complexity
**Impact:** Medium
**Probability:** Low
**Mitigation:**
- Separate TemplateLexer for clarity
- Extensive tests for nested expressions
- Clear error messages for unterminated templates

### Risk 3: Performance Regression
**Impact:** Low
**Probability:** Low
**Mitigation:**
- Benchmark suite to track performance
- Logos generates highly optimized code
- Profile hot paths if needed

### Risk 4: Tokenization Ambiguity
**Impact:** High
**Probability:** Low
**Mitigation:**
- Precedence rules in logos (longer matches first)
- Comprehensive tests for operator combinations
- Clear documentation of token precedence

---

## Future Enhancements

### Milestone 2.2+ Features:
- Incremental lexing for IDE support
- Token stream caching
- Parallel lexing for large files
- Syntax highlighting metadata
- Source map generation

---

## Summary

Milestone 2.1 implements a complete, high-performance lexer for Raya using logos, with full Unicode support, template literals, and comprehensive error handling.

**Key Features:**
- [ ] Complete token set for all Raya language constructs
- [ ] Logos-based lexer for optimal performance
- [ ] Precise source location tracking
- [ ] Template literal support with nested expressions
- [ ] Full Unicode identifier support
- [ ] Error recovery and rich error messages
- [ ] 100+ comprehensive tests

**Design Priorities:**
1. **Correctness:** All tokens correctly identified
2. **Performance:** Sub-millisecond for typical files
3. **Usability:** Clear error messages with source context
4. **Maintainability:** Declarative token definitions with logos

**Target:** Production-ready lexer with excellent performance and developer experience.

**Timeline:** 1-2 weeks (10 days)

**Risk Level:** Low (well-understood problem, proven tools)
