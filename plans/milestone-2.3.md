# Milestone 2.3: Parser Implementation

**Duration:** 3-4 weeks
**Status:** 🔄 Not Started
**Dependencies:** Milestone 2.2 (AST Definition) ✅ Complete
**Next Milestone:** 2.4 (Type Checker)

---

## Table of Contents

1. [Overview](#overview)
2. [Goals](#goals)
3. [Non-Goals](#non-goals)
4. [Design Principles](#design-principles)
5. [Architecture](#architecture)
6. [Phase 1: Core Parser Infrastructure](#phase-1-core-parser-infrastructure-days-1-3)
7. [Phase 2: Expression Parsing](#phase-2-expression-parsing-days-4-7)
8. [Phase 3: Statement Parsing](#phase-3-statement-parsing-days-8-11)
9. [Phase 4: Type Annotation Parsing](#phase-4-type-annotation-parsing-days-12-14)
10. [Phase 5: Advanced Features & Error Recovery](#phase-5-advanced-features--error-recovery-days-15-18)
11. [Testing Strategy](#testing-strategy)
12. [Success Criteria](#success-criteria)
13. [References](#references)

---

## Overview

Implement a complete recursive descent parser for the Raya programming language that transforms a stream of tokens (from Milestone 2.1 Lexer) into an Abstract Syntax Tree (from Milestone 2.2 AST).

### What is a Parser?

A parser analyzes the syntactic structure of source code and builds a tree representation (AST). It ensures the code follows the grammar rules and provides meaningful error messages when it doesn't.

**Input:** Token stream from lexer
```rust
[Let, Identifier("x"), Colon, Identifier("number"), Equal, IntLiteral(42), Semicolon]
```

**Output:** AST node
```rust
VariableDecl {
    kind: VariableKind::Let,
    pattern: Pattern::Identifier("x"),
    type_annotation: Some(Type::Primitive(Number)),
    initializer: Some(Expression::IntLiteral(42)),
}
```

### Parser Approach

This milestone implements a **recursive descent parser** with **operator precedence climbing** for expressions. This approach is:
- **Simple to implement** - Each grammar rule maps to a parsing function
- **Fast** - Single-pass, no backtracking for most constructs
- **Maintainable** - Easy to extend with new language features
- **Error-friendly** - Natural place for error recovery

---

## Goals

### Primary Goals

1. **Complete Grammar Coverage**: Parse all Raya constructs from LANG.md
2. **Correct AST Generation**: Produce semantically correct AST nodes
3. **Comprehensive Error Reporting**:
   - Precise error locations (line, column)
   - Helpful error messages
   - Recovery from common mistakes
4. **Performance**: Parse 10,000+ LOC/second on modern hardware
5. **Memory Efficiency**: Minimize allocations during parsing
6. **Operator Precedence**: Correctly handle all operators per JS/TS rules
7. **JSX Support**: Parse JSX/TSX syntax for UI components

### Secondary Goals

1. **Panic Mode Recovery**: Continue parsing after errors
2. **Suggested Fixes**: Hint at corrections for common errors
3. **Extensive Test Coverage**: 200+ parser tests
4. **Fuzzing Support**: Handle malformed input gracefully

---

## Non-Goals

1. **Type Checking**: Validating types (Milestone 2.4)
2. **Name Resolution**: Resolving identifiers (Milestone 2.4)
3. **Semantic Analysis**: Checking control flow, exhaustiveness (Milestone 2.4)
4. **Code Generation**: Emitting bytecode (Milestone 3.1)
5. **Macro Expansion**: Compile-time metaprogramming
6. **Incremental Parsing**: Re-parsing only changed portions

---

## Design Principles

### 1. One Function Per Grammar Rule

Each grammar production maps to a parsing function:

**Grammar:**
```
VariableDecl ::= ("let" | "const") Pattern (":" Type)? ("=" Expression)? ";"
```

**Function:**
```rust
fn parse_variable_decl(&mut self) -> Result<VariableDecl, ParseError>
```

### 2. Recursive Descent with Precedence Climbing

- **Statements**: Recursive descent (top-down)
- **Expressions**: Precedence climbing (handles operator priority)
- **Types**: Recursive descent

### 3. Synchronization Points for Error Recovery

When an error occurs, skip to known synchronization points:
- Statement boundaries (`;`, `}`, keywords)
- Expression boundaries (`,`, `)`, `]`)
- Continue parsing to find more errors

### 4. Panic-Free Parsing

Never `panic!` on invalid input:
- Return `Result<T, ParseError>` from all parsing functions
- Collect all errors, don't stop at first error
- Gracefully handle EOF, unexpected tokens, malformed syntax

### 5. Zero-Copy Where Possible

Reuse token data without cloning:
- String interning for identifiers
- Reference token positions in spans
- Clone only when building AST nodes

### 6. Lookahead Minimization

Minimize lookahead to 1 token (LL(1) where possible):
- Most constructs determined by first token
- Use 2-token lookahead only when necessary (arrow functions vs parenthesized expressions)

---

## Architecture

### Parser Structure

```
Parser
├── lexer: Lexer           // Token source
├── current: Token         // Current token
├── peek: Option<Token>    // Lookahead (LL(2))
├── errors: Vec<ParseError> // Accumulated errors
└── context: ParseContext  // State tracking
```

### Module Organization

```
crates/raya-parser/src/
├── lib.rs                 // Public API
├── parser/
│   ├── mod.rs            // Parser struct & core logic
│   ├── expr.rs           // Expression parsing
│   ├── stmt.rs           // Statement parsing
│   ├── types.rs          // Type annotation parsing
│   ├── pattern.rs        // Pattern parsing
│   ├── jsx.rs            // JSX/TSX parsing
│   ├── precedence.rs     // Operator precedence table
│   ├── error.rs          // Error types & reporting
│   └── recovery.rs       // Error recovery strategies
└── tests/
    ├── parser_tests.rs   // Integration tests
    ├── expr_tests.rs     // Expression parsing tests
    ├── stmt_tests.rs     // Statement parsing tests
    ├── type_tests.rs     // Type parsing tests
    ├── jsx_tests.rs      // JSX parsing tests
    └── error_tests.rs    // Error handling tests
```

### Error Handling

**ParseError Structure:**
```rust
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub span: Span,
    pub message: String,
    pub suggestion: Option<String>,
}

pub enum ParseErrorKind {
    UnexpectedToken { expected: Vec<TokenKind>, found: TokenKind },
    UnexpectedEof { expected: Vec<TokenKind> },
    InvalidSyntax { reason: String },
    DuplicateDeclaration { name: String },
    // ... more variants
}
```

---

## Phase 1: Core Parser Infrastructure (Days 1-3)

### Goal
Establish the foundation: Parser struct, token consumption, error handling, and basic utilities.

### Deliverables

#### 1.1 Parser Core (`parser/mod.rs`)

**Parser struct:**
- Token stream management
- Current/peek token tracking
- Error accumulation
- Span combination utilities

**Core methods:**
- `new(lexer: Lexer) -> Self` - Initialize parser
- `parse() -> Result<Module, Vec<ParseError>>` - Main entry point
- `current() -> &Token` - Get current token
- `peek() -> Option<&Token>` - Lookahead 1 token
- `advance() -> Token` - Consume current token
- `expect(kind: TokenKind) -> Result<Token, ParseError>` - Consume expected token
- `check(kind: TokenKind) -> bool` - Check without consuming
- `check_any(&[TokenKind]) -> bool` - Check multiple kinds
- `at_eof() -> bool` - Check for end of input
- `combine_spans(start: Span, end: Span) -> Span` - Merge spans
- `error(&mut self, kind: ParseErrorKind, span: Span)` - Record error

#### 1.2 Error Handling (`parser/error.rs`)

**Error types:**
- Complete `ParseError` and `ParseErrorKind` definitions
- Error formatting with source snippets
- Colorized terminal output (optional)
- JSON error output for IDEs

**Error utilities:**
- `expected_one_of(expected: &[TokenKind], found: TokenKind) -> ParseError`
- `unexpected_eof(expected: &[TokenKind]) -> ParseError`
- `invalid_syntax(message: &str, span: Span) -> ParseError`

#### 1.3 Recovery Strategies (`parser/recovery.rs`)

**Synchronization:**
- `sync_to_statement_boundary()` - Skip to next statement
- `sync_to_expression_boundary()` - Skip to next expression
- `skip_until(tokens: &[TokenKind])` - Skip until specific token

**Recovery helpers:**
- `try_insert_semicolon()` - Suggest missing semicolon
- `try_close_delimiter(open: TokenKind)` - Suggest closing bracket/paren
- `suggest_did_you_mean(identifier: &str)` - Typo suggestions

### Tasks

- [ ] Implement Parser struct with token management
- [ ] Implement core token consumption methods
- [ ] Define ParseError and ParseErrorKind enums
- [ ] Implement error formatting with source context
- [ ] Implement basic error recovery (sync to statement boundary)
- [ ] Add 10+ unit tests for error handling
- [ ] Test EOF handling
- [ ] Test unexpected token handling

### Testing Focus

**Unit tests:**
- Token advance/peek/expect mechanics
- Error accumulation (multiple errors)
- Span combination correctness
- Recovery to synchronization points

**Example tests:**
- Parse empty source → empty Module
- Parse single token → error
- Parse unexpected EOF → descriptive error
- Multiple syntax errors → all reported

---

## Phase 2: Expression Parsing (Days 4-7)

### Goal
Parse all expression types using precedence climbing for binary/unary operators.

### Deliverables

#### 2.1 Operator Precedence Table (`parser/precedence.rs`)

**Precedence levels (highest to lowest):**
1. Primary: Literals, identifiers, `()`, `[]`, `{}`
2. Postfix: `++`, `--`, `()` (call), `.`, `[]` (index)
3. Prefix: `+`, `-`, `!`, `~`, `++`, `--`, `typeof`, `await`
4. Exponentiation: `**`
5. Multiplicative: `*`, `/`, `%`
6. Additive: `+`, `-`
7. Shift: `<<`, `>>`, `>>>`
8. Relational: `<`, `>`, `<=`, `>=`
9. Equality: `==`, `!=`, `===`, `!==`
10. Bitwise AND: `&`
11. Bitwise XOR: `^`
12. Bitwise OR: `|`
13. Logical AND: `&&`
14. Logical OR: `||`
15. Nullish Coalescing: `??`
16. Conditional: `? :`
17. Assignment: `=`, `+=`, `-=`, etc.

**Implementation:**
```rust
pub enum Precedence {
    Lowest,
    Assignment,      // =, +=, -=, ...
    Conditional,     // ? :
    NullishCoalescing, // ??
    LogicalOr,       // ||
    LogicalAnd,      // &&
    BitwiseOr,       // |
    BitwiseXor,      // ^
    BitwiseAnd,      // &
    Equality,        // ==, !=, ===, !==
    Relational,      // <, >, <=, >=
    Shift,           // <<, >>, >>>
    Additive,        // +, -
    Multiplicative,  // *, /, %
    Exponentiation,  // **
    Prefix,          // !, ~, +, -, ++, --, typeof, await
    Postfix,         // ++, --, call, member, index
    Primary,         // literals, identifiers, ()
}

impl TokenKind {
    pub fn precedence(&self) -> Precedence { /* ... */ }
    pub fn is_binary_op(&self) -> bool { /* ... */ }
    pub fn is_unary_op(&self) -> bool { /* ... */ }
    pub fn is_assignment_op(&self) -> bool { /* ... */ }
}
```

#### 2.2 Primary Expressions (`parser/expr.rs`)

**Parsing functions:**
- `parse_expression()` - Entry point (lowest precedence)
- `parse_primary_expression()` - Literals, identifiers, parens, arrays, objects
- `parse_literal()` - Int, float, string, boolean, null, template
- `parse_identifier()` - Simple identifier
- `parse_array_literal()` - `[1, 2, 3]`
- `parse_object_literal()` - `{ x: 1, y: 2 }`
- `parse_template_literal()` - `` `Hello, ${name}!` ``
- `parse_parenthesized_or_arrow()` - Disambiguate `(x)` vs `(x) => x`

**Special cases:**
- **Array holes:** `[1, , 3]` → `Some(1), None, Some(3)`
- **Object shorthand:** `{ x }` → `{ x: x }`
- **Computed properties:** `{ [key]: value }`
- **Spread:** `{ ...obj }`

#### 2.3 Binary & Unary Expressions

**Precedence climbing algorithm:**
```rust
fn parse_binary_expression(&mut self, min_prec: Precedence) -> Result<Expression> {
    let mut left = parse_unary_expression()?;

    while current_precedence() >= min_prec {
        let op = consume_operator();
        let right = parse_binary_expression(op.precedence() + 1)?;
        left = BinaryExpression { op, left, right };
    }

    left
}
```

**Parsing functions:**
- `parse_binary_expression(min_prec: Precedence)` - Handle all binary ops
- `parse_unary_expression()` - Prefix operators
- `parse_postfix_expression()` - Postfix operators, calls, members
- `parse_assignment_expression()` - All assignment operators
- `parse_logical_expression()` - `&&`, `||`, `??`
- `parse_conditional_expression()` - Ternary `? :`

#### 2.4 Postfix & Call Expressions

**Parsing functions:**
- `parse_call_expression(callee: Expression)` - `foo(1, 2)`, `foo<T>()`
- `parse_member_expression(object: Expression)` - `obj.prop`, `obj?.prop`
- `parse_index_expression(object: Expression)` - `arr[0]`
- `parse_new_expression()` - `new Point(1, 2)`
- `parse_postfix_operator(operand: Expression)` - `x++`, `x--`

**Special cases:**
- **Optional chaining:** `obj?.prop?.method?.()`
- **Generic call:** `foo<number>(42)`
- **Spread arguments:** `func(...args)`

#### 2.5 Arrow Functions

**Parsing function:**
- `parse_arrow_function(params: Vec<Pattern>)` - `(x) => x + 1`, `x => x * 2`

**Challenges:**
- Disambiguate `(x)` vs `(x) => x` (requires 2-token lookahead)
- Parse both expression and block bodies
- Handle async arrows: `async (x) => await x`

**Implementation strategy:**
```rust
fn parse_parenthesized_or_arrow() -> Result<Expression> {
    let start = current_span();
    consume(LParen);

    // Try to parse as parameter list
    let params = parse_parameter_list_or_expression()?;
    consume(RParen);

    // Check for arrow
    if check(Arrow) {
        consume(Arrow);
        return parse_arrow_body(params);
    }

    // It's a parenthesized expression
    Ok(Expression::Parenthesized(params.into_expression()))
}
```

### Tasks

- [ ] Implement precedence table for all operators
- [ ] Implement primary expression parsing
- [ ] Implement binary expression parsing with precedence
- [ ] Implement unary/postfix expression parsing
- [ ] Implement call/member/index expression parsing
- [ ] Implement arrow function parsing
- [ ] Implement template literal parsing
- [ ] Implement array/object literal parsing
- [ ] Add 50+ expression parsing tests
- [ ] Test operator precedence exhaustively
- [ ] Test edge cases (holes, spread, optional chaining)

### Testing Focus

**Expression categories:**
- Literals: All types (int, float, string, boolean, null, template)
- Binary operators: All precedence levels
- Unary operators: Prefix and postfix
- Calls: Regular, generic, spread arguments
- Member access: Dot notation, optional chaining
- Arrays: Empty, holes, nested, spread
- Objects: Empty, shorthand, computed, spread
- Arrows: Expression/block body, async, parameters
- Templates: Simple, nested expressions, escapes

**Precedence tests:**
```typescript
// Test all operator combinations
x + y * z        → (x + (y * z))
x * y + z        → ((x * y) + z)
x && y || z      → ((x && y) || z)
x = y = z        → (x = (y = z))  // Right-associative
a ? b : c ? d : e → (a ? b : (c ? d : e))
```

**Error cases:**
- Unexpected operator: `+ +`
- Missing operand: `x +`
- Unmatched parens: `(x + y`
- Invalid property name: `obj.123`

---

## Phase 3: Statement Parsing (Days 8-11)

### Goal
Parse all statement types including declarations, control flow, and blocks.

### Deliverables

#### 3.1 Variable Declarations (`parser/stmt.rs`)

**Parsing functions:**
- `parse_statement()` - Entry point, dispatch by first token
- `parse_variable_declaration()` - `let x = 42;`, `const y: string = "hi";`
- `parse_pattern()` - Identifier, array destructuring, object destructuring

**Patterns:**
- **Identifier:** `let x = 42;`
- **Array destructuring:** `let [x, y] = [1, 2];`
- **Object destructuring:** `let { x, y } = obj;`
- **Nested destructuring:** `let { a: [b, c] } = obj;`

**Validation:**
- `const` requires initializer
- Check valid pattern structure
- Detect duplicate bindings in pattern

#### 3.2 Function Declarations

**Parsing function:**
- `parse_function_declaration()` - `function foo(x: number): number { return x; }`

**Components:**
- Function name (required for declarations)
- Type parameters: `<T, K extends string>`
- Parameters: `(x: number, y?: string)`
- Return type annotation
- Body (block statement)
- `async` modifier

**Special cases:**
- Generic functions: `function map<T, U>(arr: T[], fn: (T) => U): U[] { }`
- Async functions: `async function fetchData(): Task<Data> { }`
- Optional parameters: `function foo(x?: number) { }`

#### 3.3 Class Declarations

**Parsing function:**
- `parse_class_declaration()` - Complete class syntax

**Components:**
- Class name
- Type parameters
- Extends clause
- Implements clauses (multiple interfaces)
- Members: fields, methods, constructor

**Member parsing:**
- `parse_class_member()` - Dispatch to field/method/constructor
- `parse_field_declaration()` - `x: number = 0;`
- `parse_method_declaration()` - `foo(x: number): void { }`
- `parse_constructor_declaration()` - `constructor(x: number) { }`

**Modifiers:**
- `static` - Class-level vs instance-level
- `async` - Async methods

#### 3.4 Interface & Type Alias

**Parsing functions:**
- `parse_interface_declaration()` - Interface definitions
- `parse_type_alias_declaration()` - Type aliases

**Interface members:**
- Properties: `x: number;`, `y?: string;`
- Methods: `foo(x: number): void;`

**Type aliases:**
- Simple: `type Point = { x: number; y: number };`
- Generic: `type Result<T, E> = { status: "ok"; value: T } | { status: "error"; error: E };`

#### 3.5 Control Flow Statements

**Parsing functions:**
- `parse_if_statement()` - `if`, `else if`, `else`
- `parse_switch_statement()` - `switch`, `case`, `default`
- `parse_while_statement()` - `while` loop
- `parse_do_while_statement()` - `do-while` loop
- `parse_for_statement()` - `for` loop
- `parse_break_statement()` - `break`, `break label;`
- `parse_continue_statement()` - `continue`, `continue label;`
- `parse_return_statement()` - `return`, `return expr;`
- `parse_throw_statement()` - `throw expr;`
- `parse_try_statement()` - `try-catch-finally`

**If statement:**
- Single `if`
- `if-else` chains
- Nested `if` statements

**Switch statement:**
- Multiple `case` clauses
- Optional `default` clause
- Fall-through behavior (no automatic break)

**For loop variants:**
- C-style: `for (let i = 0; i < 10; i++) { }`
- Infinite: `for (;;) { }`
- No init: `for (; condition; update) { }`

**Try-catch-finally:**
- `try` block (required)
- `catch` clause (optional, with/without parameter)
- `finally` block (optional)
- At least one of `catch` or `finally` required

#### 3.6 Block & Expression Statements

**Parsing functions:**
- `parse_block_statement()` - `{ stmt1; stmt2; }`
- `parse_expression_statement()` - Expression followed by semicolon

**Automatic Semicolon Insertion (ASI):**
- Insert semicolon at newline before `}`, EOF, or incompatible token
- Don't insert in middle of expression
- Follow TypeScript/JavaScript ASI rules

#### 3.7 Module System

**Parsing functions:**
- `parse_import_declaration()` - All import forms
- `parse_export_declaration()` - All export forms

**Import forms:**
- Named: `import { foo, bar } from "./mod";`
- Default: `import foo from "./mod";`
- Namespace: `import * as mod from "./mod";`
- Combined: `import foo, { bar } from "./mod";`

**Export forms:**
- Named: `export { foo, bar };`
- Re-export: `export { foo } from "./mod";`
- Export all: `export * from "./mod";`
- Export declaration: `export const x = 42;`
- Default export: `export default class Foo { }`

### Tasks

- [ ] Implement statement dispatch (parse_statement)
- [ ] Implement variable declarations with patterns
- [ ] Implement function declarations
- [ ] Implement class declarations with all members
- [ ] Implement interface and type alias declarations
- [ ] Implement all control flow statements
- [ ] Implement block and expression statements
- [ ] Implement import/export declarations
- [ ] Add 60+ statement parsing tests
- [ ] Test destructuring patterns
- [ ] Test nested control flow
- [ ] Test ASI edge cases

### Testing Focus

**Declaration tests:**
- Variables: `let`, `const`, with/without initializers
- Functions: Regular, async, generic
- Classes: Simple, generic, inheritance, implements
- Interfaces: Properties, methods, extends
- Type aliases: Simple, generic, unions

**Control flow tests:**
- All statement types with valid syntax
- Nested statements (if inside while, etc.)
- Edge cases (empty blocks, missing braces)
- Labels on loops

**Pattern tests:**
- Identifier patterns
- Array destructuring (simple, nested, holes)
- Object destructuring (simple, nested, shorthand)

**Module tests:**
- All import forms
- All export forms
- Mixed import/export
- Re-exports

---

## Phase 4: Type Annotation Parsing (Days 12-14)

### Goal
Parse all type annotation syntax including primitives, unions, functions, generics, and objects.

### Deliverables

#### 4.1 Type Parsing Core (`parser/types.rs`)

**Parsing functions:**
- `parse_type_annotation()` - Entry point
- `parse_type()` - Dispatch to specific type parsers
- `parse_primary_type()` - Primitives, references, parens

**Type contexts:**
- Variable declarations: `let x: number;`
- Function parameters: `(x: number)`
- Function return types: `(): number`
- Type aliases: `type Foo = number;`
- Class fields: `x: number;`
- Interface members: `foo(): number;`

#### 4.2 Primitive & Reference Types

**Parsing functions:**
- `parse_primitive_type()` - `number`, `string`, `boolean`, `null`, `void`
- `parse_type_reference()` - `Foo`, `Map<K, V>`

**Type references:**
- Simple: `Point`
- Generic: `Array<number>`, `Map<string, number>`
- Nested generics: `Promise<Result<T, E>>`

#### 4.3 Union & Intersection Types

**Parsing functions:**
- `parse_union_type()` - `number | string | null`
- `parse_intersection_type()` - `A & B & C` (if supported)

**Union types:**
- Bare unions (primitives): `string | number | boolean | null`
- Discriminated unions (objects): `{ kind: "a" } | { kind: "b" }`

**Precedence:**
- Intersection binds tighter than union
- `A & B | C & D` → `(A & B) | (C & D)`

#### 4.4 Function Types

**Parsing function:**
- `parse_function_type()` - `(x: number) => number`, `() => void`

**Components:**
- Parameters: Named or anonymous
- Return type
- Optional parameters: `(x?: number) => void`

**Examples:**
- Simple: `(x: number) => number`
- Multiple params: `(x: number, y: string) => boolean`
- No params: `() => void`
- Anonymous params: `(number, string) => boolean`

#### 4.5 Array & Tuple Types

**Parsing functions:**
- `parse_array_type()` - `number[]`, `Array<number>`
- `parse_tuple_type()` - `[number, string]`

**Array types:**
- Postfix syntax: `number[]`, `string[][]`
- Generic syntax: `Array<number>`

**Tuple types:**
- Fixed length: `[number, string]`
- Empty: `[]`
- Nested: `[number, [string, boolean]]`

#### 4.6 Object Types

**Parsing function:**
- `parse_object_type()` - `{ x: number; y: string }`

**Members:**
- Properties: `x: number;`
- Optional properties: `y?: string;`
- Methods: `foo(x: number): void;`

**Examples:**
- Empty: `{}`
- Single property: `{ x: number }`
- Multiple properties: `{ x: number; y: string; z: boolean }`
- Methods: `{ getValue(): number; setValue(x: number): void }`

#### 4.7 Typeof & Special Types

**Parsing functions:**
- `parse_typeof_type()` - `typeof value` (for bare unions)
- `parse_parenthesized_type()` - `(number | string)`

**Typeof usage:**
- Only valid for bare unions (primitives)
- Used for type narrowing in control flow

#### 4.8 Type Parameters

**Parsing functions:**
- `parse_type_parameters()` - `<T, K extends string>`
- `parse_type_parameter()` - Single parameter with constraints/defaults

**Components:**
- Name: Identifier
- Constraint: `extends` clause
- Default: `= Type`

**Examples:**
- Simple: `<T>`
- Multiple: `<T, U, V>`
- Constrained: `<T extends string>`
- Default: `<T = number>`
- Combined: `<T extends string = "default">`

### Tasks

- [ ] Implement type annotation parsing
- [ ] Implement primitive and reference types
- [ ] Implement union types
- [ ] Implement function types
- [ ] Implement array and tuple types
- [ ] Implement object types
- [ ] Implement typeof types
- [ ] Implement type parameters with constraints
- [ ] Add 40+ type parsing tests
- [ ] Test generic type nesting
- [ ] Test complex union types

### Testing Focus

**Type categories:**
- Primitives: All 5 primitive types
- References: Simple and generic
- Unions: 2-way, 3-way, nested
- Functions: Various parameter configurations
- Arrays: Postfix and generic syntax
- Tuples: Empty, single, multiple elements
- Objects: Empty, properties, methods, optional
- Typeof: Bare union types
- Type parameters: Simple, constrained, defaults

**Complex types:**
```typescript
// Nested generics
Result<Array<Map<string, number>>, Error>

// Union of function types
((x: number) => string) | ((x: string) => number)

// Object with method
{ getValue(): Promise<Result<T, E>> }

// Generic function type
<T>(x: T) => Array<T>
```

---

## Phase 5: Advanced Features & Error Recovery (Days 15-18)

### Goal
Implement JSX parsing, comprehensive error recovery, and polish the parser.

### Deliverables

#### 5.1 JSX/TSX Parsing (`parser/jsx.rs`)

**Parsing functions:**
- `parse_jsx_element()` - `<div>content</div>`
- `parse_jsx_self_closing_element()` - `<img src="..." />`
- `parse_jsx_fragment()` - `<>content</>`
- `parse_jsx_opening_element()` - `<div className="foo">`
- `parse_jsx_closing_element()` - `</div>`
- `parse_jsx_attributes()` - All attribute forms
- `parse_jsx_children()` - Elements, text, expressions

**Element names:**
- Intrinsic (lowercase): `div`, `span`, `button`
- Component (uppercase): `Button`, `MyComponent`
- Namespaced: `svg:path`, `xml:lang`
- Member expression: `React.Fragment`, `UI.Button`

**Attributes:**
- Simple: `className="foo"`
- Expression: `value={expr}`
- Boolean: `disabled` (implicitly `true`)
- Spread: `{...props}`

**Children:**
- Text: `<div>Hello</div>`
- Elements: `<div><span>Hi</span></div>`
- Expressions: `<div>{value}</div>`
- Mixed: `<div>Hello {name}!</div>`

**Special cases:**
- Empty elements: `<div />`
- Fragments: `<>content</>`
- Nested elements: `<div><p><span>Hi</span></p></div>`
- Tag mismatch detection: `<div></span>` → error

#### 5.2 Enhanced Error Recovery

**Recovery strategies:**

**1. Statement-level recovery:**
- Skip to next statement boundary (`;`, `}`, keyword)
- Continue parsing subsequent statements
- Collect all errors before failing

**2. Expression-level recovery:**
- Insert dummy expression on error
- Skip to expression boundary (`,`, `)`, `]`, `;`)
- Continue parsing parent construct

**3. Missing token insertion:**
- Missing semicolons (ASI)
- Missing closing delimiters
- Missing keywords

**4. Common mistakes:**
- `=` instead of `==` or `===`
- `fun` instead of `function`
- Missing `async` keyword before await
- Wrong arrow syntax `=> ` vs `=>`

**Error message quality:**
- Show source code snippet with highlight
- Suggest corrections when obvious
- Explain why syntax is invalid
- Provide links to documentation

#### 5.3 Parser Performance Optimization

**Optimizations:**
- String interning for identifiers
- Reuse token allocations
- Minimize clones during parsing
- Preallocate common AST node types

**Profiling:**
- Benchmark on large files (10k+ LOC)
- Identify bottlenecks
- Optimize hot paths

#### 5.4 Public API & Integration

**Public API (`lib.rs`):**
```rust
pub fn parse_module(source: &str) -> Result<Module, Vec<ParseError>>
pub fn parse_expression(source: &str) -> Result<Expression, Vec<ParseError>>
pub fn parse_type(source: &str) -> Result<TypeAnnotation, Vec<ParseError>>
```

**Integration:**
- Accept source code as input
- Create lexer internally
- Return AST or detailed errors
- Support streaming errors (incremental reporting)

#### 5.5 Documentation & Examples

**Documentation:**
- Public API docs with examples
- Internal parser architecture docs
- Grammar reference (EBNF or similar)
- Error recovery strategies explained

**Examples:**
- Parse simple programs
- Parse complex expressions
- Handle errors gracefully
- Use visitor pattern on AST

### Tasks

- [ ] Implement JSX element parsing
- [ ] Implement JSX attribute parsing
- [ ] Implement JSX children parsing
- [ ] Enhance error recovery for statements
- [ ] Enhance error recovery for expressions
- [ ] Add helpful error messages
- [ ] Implement missing token insertion
- [ ] Optimize parser performance
- [ ] Create public API
- [ ] Write comprehensive documentation
- [ ] Add 30+ JSX tests
- [ ] Add 20+ error recovery tests
- [ ] Benchmark parser on large files

### Testing Focus

**JSX tests:**
- Simple elements
- Self-closing elements
- Nested elements
- Fragments
- Attributes (all forms)
- Children (all forms)
- Edge cases (tag mismatches, empty content)

**Error recovery tests:**
- Missing semicolons
- Unmatched braces/parens
- Invalid tokens in expressions
- Multiple errors in sequence
- Recovery and continue parsing

**Integration tests:**
- Parse complete Raya programs
- Parse stdlib modules
- Parse examples from LANG.md
- Round-trip test (parse → unparse → parse)

---

## Testing Strategy

### Test Organization

**Unit Tests (`tests/parser_tests.rs`):**
- Test individual parsing functions
- Isolated expression/statement/type parsing
- Edge cases and corner cases

**Integration Tests:**
- Parse complete programs
- Multi-file parsing
- Error scenarios

**Corpus Tests:**
- Parse all examples from LANG.md
- Parse standard library source
- Parse example programs

**Fuzzing:**
- Random token sequences
- Malformed input
- Stress test error recovery

### Test Categories

#### 1. Expression Tests (50+ tests)

**Literals:**
- Integers: Decimal, hex, binary, octal
- Floats: Standard, scientific notation
- Strings: Single quote, double quote, escapes
- Templates: Simple, with expressions
- Booleans: `true`, `false`
- Null: `null`

**Binary operators (all precedence levels):**
- Arithmetic: `+`, `-`, `*`, `/`, `%`, `**`
- Comparison: `==`, `!=`, `===`, `!==`, `<`, `>`, `<=`, `>=`
- Logical: `&&`, `||`, `??`
- Bitwise: `&`, `|`, `^`, `<<`, `>>`, `>>>`

**Unary operators:**
- Prefix: `+`, `-`, `!`, `~`, `++`, `--`
- Postfix: `++`, `--`

**Complex expressions:**
- Calls: `foo()`, `foo(1, 2)`, `foo<T>(x)`
- Member: `obj.prop`, `obj?.prop`
- Index: `arr[0]`, `arr[x + 1]`
- New: `new Foo()`, `new Bar(1, 2)`
- Arrows: `x => x`, `(x, y) => x + y`, `async x => await x`
- Conditional: `x ? y : z`
- Assignment: All assignment operators

**Arrays & Objects:**
- Empty: `[]`, `{}`
- Simple: `[1, 2, 3]`, `{ x: 1, y: 2 }`
- Nested: `[[1, 2], [3, 4]]`, `{ a: { b: 1 } }`
- Holes: `[1, , 3]`
- Spread: `[...arr]`, `{ ...obj }`

#### 2. Statement Tests (60+ tests)

**Variable declarations:**
- Let: `let x;`, `let x = 42;`
- Const: `const x = 42;`
- Typed: `let x: number = 42;`
- Destructuring: `let [x, y] = arr;`, `let { a, b } = obj;`

**Function declarations:**
- Simple: `function foo() { }`
- Parameters: `function foo(x: number, y: string) { }`
- Return type: `function foo(): number { }`
- Generic: `function map<T, U>(arr: T[]): U[] { }`
- Async: `async function fetch(): Task<Data> { }`

**Class declarations:**
- Simple: `class Foo { }`
- Fields: `class Foo { x: number; }`
- Methods: `class Foo { bar() { } }`
- Constructor: `class Foo { constructor(x: number) { } }`
- Extends: `class Foo extends Bar { }`
- Implements: `class Foo implements IFoo { }`
- Generic: `class Box<T> { value: T; }`
- Static members: `class Foo { static count: number; }`

**Interfaces:**
- Simple: `interface IFoo { }`
- Properties: `interface IFoo { x: number; }`
- Methods: `interface IFoo { bar(): void; }`
- Extends: `interface IFoo extends IBar { }`
- Generic: `interface Result<T, E> { }`

**Control flow:**
- If: All forms (`if`, `if-else`, `if-else if-else`)
- Switch: With multiple cases and default
- While: `while (condition) { }`
- Do-while: `do { } while (condition);`
- For: All forms (C-style, infinite)
- Break/Continue: With and without labels
- Return: With and without value
- Throw: `throw error;`
- Try-catch-finally: All combinations

**Blocks:**
- Empty: `{ }`
- Single statement: `{ stmt; }`
- Multiple statements: `{ stmt1; stmt2; }`
- Nested blocks

**Imports/Exports:**
- All import forms
- All export forms
- Re-exports
- Default imports/exports

#### 3. Type Tests (40+ tests)

**Primitives:**
- All 5 primitives: `number`, `string`, `boolean`, `null`, `void`

**References:**
- Simple: `Foo`
- Generic: `Array<number>`, `Map<K, V>`

**Unions:**
- Two types: `number | string`
- Three+ types: `number | string | boolean | null`
- Nested: `(A | B) | C`

**Functions:**
- Simple: `(x: number) => number`
- Multiple params: `(x: number, y: string) => boolean`
- No params: `() => void`
- Optional params: `(x?: number) => void`

**Arrays & Tuples:**
- Arrays: `number[]`, `Array<string>`
- Tuples: `[number, string]`, `[]`

**Objects:**
- Empty: `{}`
- Properties: `{ x: number; y: string }`
- Optional: `{ x?: number }`
- Methods: `{ foo(): void }`

**Type parameters:**
- Simple: `<T>`
- Constrained: `<T extends string>`
- Default: `<T = number>`

**Complex types:**
- Nested generics: `Result<Array<T>, Error>`
- Union of functions: `((x: number) => string) | ((x: string) => number)`
- Generic function types: `<T>(x: T) => T`

#### 4. JSX Tests (30+ tests)

**Elements:**
- Simple: `<div />`
- With attributes: `<div className="foo" />`
- With children: `<div>Hello</div>`
- Nested: `<div><span>Hi</span></div>`

**Fragments:**
- Empty: `<></>`
- With children: `<>Hello</>`

**Attributes:**
- String: `className="foo"`
- Expression: `value={expr}`
- Boolean: `disabled`
- Spread: `{...props}`

**Children:**
- Text: `<div>Hello</div>`
- Element: `<div><span /></div>`
- Expression: `<div>{value}</div>`
- Mixed: `<div>Hello {name}!</div>`

**Element names:**
- Intrinsic: `<div />`
- Component: `<Button />`
- Namespaced: `<svg:path />`
- Member: `<React.Fragment />`

#### 5. Error Tests (20+ tests)

**Expected errors:**
- Unexpected token
- Unexpected EOF
- Missing semicolon
- Unmatched delimiter
- Invalid syntax

**Error recovery:**
- Continue after error
- Multiple errors reported
- Helpful suggestions
- Source context shown

**Common mistakes:**
- `=` vs `==`
- Missing `async` before `await`
- Wrong arrow syntax
- Tag mismatch in JSX

### Test Metrics

**Target metrics:**
- **200+ total tests**
- **80%+ code coverage**
- **All examples from LANG.md parse correctly**
- **Performance:** 10,000+ LOC/second

### Test Utilities

**Helper functions:**
```rust
fn parse_expr(source: &str) -> Expression
fn parse_stmt(source: &str) -> Statement
fn parse_type(source: &str) -> TypeAnnotation
fn expect_error(source: &str) -> ParseError
fn assert_ast_eq(source: &str, expected: Statement)
```

**Snapshot testing:**
- Generate AST snapshots for visual inspection
- Detect unintended AST changes
- Review AST structure for correctness

---

## Success Criteria

### Must Have

- [ ] Parse all Raya language constructs from LANG.md
- [ ] Generate correct AST nodes for valid input
- [ ] Report precise errors with line/column
- [ ] Recover from errors and continue parsing
- [ ] Support JSX/TSX syntax
- [ ] 200+ comprehensive tests
- [ ] All LANG.md examples parse successfully
- [ ] Performance: 10,000+ LOC/second
- [ ] Comprehensive documentation

### Should Have

- [ ] Helpful error messages with suggestions
- [ ] Missing token insertion (ASI)
- [ ] Common mistake detection
- [ ] Fuzzing test harness
- [ ] Benchmark suite

### Nice to Have

- [ ] Parallel parsing for multiple files
- [ ] Incremental parsing (reparse changed portions)
- [ ] Error recovery quality metrics
- [ ] Parser visualization tools

---

## References

### Language Specification

- [design/LANG.md](../design/LANG.md) - Complete language specification
  - Section 3: Lexical Structure (tokens from lexer)
  - Section 4: Type System (type annotations)
  - Section 6: Expressions (all expression forms)
  - Section 7: Statements (all statement forms)
  - Section 8: Functions
  - Section 9: Classes
  - Section 10: Interfaces
  - Section 16: Module System

### Related Milestones

- [Milestone 2.1](milestone-2.1.md) - Lexer (✅ Complete)
- [Milestone 2.2](milestone-2.2.md) - AST Definition (✅ Complete)
- [Milestone 2.4](milestone-2.4.md) - Type Checker (Next)

### External References

- **Recursive Descent Parsing:**
  - https://en.wikipedia.org/wiki/Recursive_descent_parser
  - Crafting Interpreters (Chapter 6): https://craftinginterpreters.com/parsing-expressions.html

- **Precedence Climbing:**
  - https://en.wikipedia.org/wiki/Operator-precedence_parser
  - https://eli.thegreenplace.net/2012/08/02/parsing-expressions-by-precedence-climbing

- **Error Recovery:**
  - Dragon Book (Compilers: Principles, Techniques, and Tools) - Chapter 4
  - Panic Mode Recovery: https://en.wikipedia.org/wiki/Panic_mode_recovery

- **TypeScript Parser:**
  - https://github.com/microsoft/TypeScript/tree/main/src/compiler

- **JSX Specification:**
  - https://facebook.github.io/jsx/

---

## Notes

### 1. Parser vs Lexer Separation

The parser receives tokens from the lexer and builds the AST. It doesn't deal with:
- Character-level processing
- String escaping
- Number parsing
- Comment handling

These are all handled by the lexer (Milestone 2.1).

### 2. Automatic Semicolon Insertion (ASI)

Follow TypeScript/JavaScript ASI rules:
- Insert semicolon at newline before `}`, EOF, or incompatible token
- Don't insert in middle of valid expression
- Special handling for `return`, `break`, `continue`, `throw`

### 3. Lookahead Strategy

Most parsing is LL(1) (1-token lookahead):
- Statement dispatch: First token determines type
- Expression dispatch: First token determines primary
- Type dispatch: First token determines type

LL(2) needed for:
- Arrow functions vs parenthesized expressions: `(x)` vs `(x) => x`
- Generic calls vs comparisons: `foo<T>()` vs `foo < T >`

### 4. Error Recovery Philosophy

**Goal:** Find as many errors as possible in one parse
- Don't stop at first error
- Synchronize to known boundaries
- Insert dummy nodes to continue
- Collect all errors for batch reporting

### 5. Performance Considerations

**Bottlenecks:**
- Token allocation (minimize clones)
- AST node allocation (use arena allocator if needed)
- String processing (use string interning)

**Optimization:**
- Profile on large files
- Optimize hot paths (expression parsing)
- Preallocate common structures

### 6. Future Extensions

This parser design supports:
- Decorators (future syntax)
- Advanced pattern matching
- Additional type syntax
- Macro expansion (preprocessing)

---

**End of Milestone 2.3 Specification**
