# Milestone 2.11: Parser Feature Completion

**Status:** ✅ Complete
**Created:** 2026-01-26
**Completed:** 2026-01-26
**Dependencies:** Milestone 2.10 (Parser Hardening) ✅

---

## Overview

Complete the remaining parser features to achieve full LANG.md specification compliance. This milestone focuses on:

1. Access modifiers (private/protected/public) for class members
2. Static fields/methods support
3. Decorator parsing
4. Template literal interpolation
5. Rest patterns in destructuring

**Not Included (removed from spec):**
- Function expressions (`const add = function() {}`) - REMOVED from LANG.md
- Class expressions (`const Point = class {}`) - REMOVED from LANG.md
- Getter/setter syntax (`get size()`) - REMOVED from LANG.md

---

## Phase 1: Access Modifiers (Days 1-2)

**Goal:** Support `private`, `protected`, `public` modifiers for class members

### Task 1.1: Add Visibility Enum to AST

**Modify:** `crates/raya-parser/src/ast/statement.rs`

```rust
/// Visibility modifier for class members
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
    #[default]
    Public,
    Protected,
    Private,
}

/// Field declaration
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDecl {
    pub decorators: Vec<Decorator>,
    pub visibility: Visibility,  // ADD THIS
    pub name: Identifier,
    pub type_annotation: Option<TypeAnnotation>,
    pub initializer: Option<Expression>,
    pub is_static: bool,
    pub span: Span,
}

/// Method declaration
#[derive(Debug, Clone, PartialEq)]
pub struct MethodDecl {
    pub decorators: Vec<Decorator>,
    pub visibility: Visibility,  // ADD THIS
    pub is_abstract: bool,
    pub name: Identifier,
    pub type_params: Option<Vec<TypeParameter>>,
    pub params: Vec<Parameter>,
    pub return_type: Option<TypeAnnotation>,
    pub body: Option<BlockStatement>,
    pub is_static: bool,
    pub is_async: bool,
    pub span: Span,
}
```

### Task 1.2: Update Parser for Visibility Modifiers

**Modify:** `crates/raya-parser/src/parser/stmt.rs`

Update `parse_class_member()` to parse visibility:

```rust
fn parse_class_member(parser: &mut Parser) -> Result<ClassMember, ParseError> {
    let start_span = parser.current_span();
    let decorators = vec![]; // TODO: Parse decorators

    // Parse visibility modifier (private/protected/public)
    let visibility = match parser.current() {
        Token::Private => {
            parser.advance();
            Visibility::Private
        }
        Token::Protected => {
            parser.advance();
            Visibility::Protected
        }
        Token::Public => {
            parser.advance();
            Visibility::Public
        }
        _ => Visibility::Public, // Default
    };

    // Parse other modifiers (abstract, static, async)
    let is_abstract = if parser.check(&Token::Abstract) {
        parser.advance();
        true
    } else {
        false
    };

    // ... rest of existing code
}
```

### Task 1.3: Add Visibility Tests

**New file:** `crates/raya-parser/tests/visibility_test.rs`

```rust
#[test]
fn test_parse_private_field() {
    let source = "class Foo { private x: number; }";
    // Verify Visibility::Private
}

#[test]
fn test_parse_protected_method() {
    let source = "class Foo { protected bar(): void {} }";
    // Verify Visibility::Protected
}

#[test]
fn test_parse_public_explicit() {
    let source = "class Foo { public name: string; }";
    // Verify Visibility::Public
}

#[test]
fn test_parse_visibility_with_static() {
    let source = "class Foo { private static count: number; }";
    // Verify both visibility and is_static
}

#[test]
fn test_parse_visibility_with_abstract() {
    let source = "abstract class Foo { protected abstract bar(): void; }";
    // Verify visibility with abstract
}
```

---

## Phase 2: Static Fields/Methods (Days 3-4)

**Goal:** Verify and complete static member support

### Task 2.1: Verify Static Field Parsing

The parser already handles `static` keyword. Verify it works correctly:

```rust
#[test]
fn test_parse_static_field_with_initializer() {
    let source = "class Counter { static count: number = 0; }";
}

#[test]
fn test_parse_static_method() {
    let source = "class Math { static abs(x: number): number { return x; } }";
}

#[test]
fn test_parse_static_async_method() {
    let source = "class Api { static async fetch(): Task<string> {} }";
}
```

### Task 2.2: Static with Visibility

Ensure `private static`, `protected static`, `public static` all work:

```rust
#[test]
fn test_parse_private_static_field() {
    let source = "class Singleton { private static instance: Singleton | null = null; }";
}
```

---

## Phase 3: Decorator Parsing (Days 5-7)

**Goal:** Parse decorators on classes, methods, fields, and parameters

### Task 3.1: Update Decorator Parsing

The AST already has `Decorator` type. Implement parsing:

**Modify:** `crates/raya-parser/src/parser/stmt.rs`

```rust
/// Parse decorators: @name or @name(args)
fn parse_decorators(parser: &mut Parser) -> Result<Vec<Decorator>, ParseError> {
    let mut decorators = Vec::new();
    let mut guard = LoopGuard::new("decorators");

    while parser.check(&Token::At) {
        guard.check()?;
        decorators.push(parse_decorator(parser)?);
    }

    Ok(decorators)
}

fn parse_decorator(parser: &mut Parser) -> Result<Decorator, ParseError> {
    let start_span = parser.current_span();
    parser.expect(Token::At)?;

    // Parse decorator expression: identifier or call
    let expression = if parser.check(&Token::Identifier(_)) {
        let ident = parse_identifier(parser)?;

        // Check for call: @decorator(args)
        if parser.check(&Token::LeftParen) {
            parser.advance();
            let args = parse_arguments(parser)?;
            parser.expect(Token::RightParen)?;

            Expression::Call(CallExpression {
                callee: Box::new(Expression::Identifier(ident)),
                type_args: None,
                arguments: args,
                span: parser.combine_spans(&start_span, &parser.current_span()),
            })
        } else {
            Expression::Identifier(ident)
        }
    } else {
        return Err(parser.unexpected_token(&[Token::Identifier(Symbol::dummy())]));
    };

    let span = parser.combine_spans(&start_span, &parser.current_span());
    Ok(Decorator { expression, span })
}
```

### Task 3.2: Apply Decorators in Parsing

Update class/method/field/parameter parsing to call `parse_decorators()`:

```rust
fn parse_class_member(parser: &mut Parser) -> Result<ClassMember, ParseError> {
    let start_span = parser.current_span();

    // Parse decorators first
    let decorators = parse_decorators(parser)?;

    // Then visibility, abstract, static, async...
    // ... rest
}

fn parse_class_declaration(parser: &mut Parser) -> Result<Statement, ParseError> {
    let start_span = parser.current_span();

    // Parse class decorators
    let decorators = parse_decorators(parser)?;

    // Parse 'abstract' keyword if present
    // ...
}
```

### Task 3.3: Decorator Tests

```rust
#[test]
fn test_parse_class_decorator() {
    let source = "@sealed class Foo {}";
}

#[test]
fn test_parse_decorator_with_args() {
    let source = "@logged(\"debug\") class Foo {}";
}

#[test]
fn test_parse_method_decorator() {
    let source = "class Foo { @memoized calculate(): number { return 42; } }";
}

#[test]
fn test_parse_field_decorator() {
    let source = "class Foo { @validate name: string; }";
}

#[test]
fn test_parse_multiple_decorators() {
    let source = "@sealed @logged class Foo {}";
}

#[test]
fn test_parse_parameter_decorator() {
    let source = "class Foo { method(@inject dep: Service): void {} }";
}
```

---

## Phase 4: Template Literal Interpolation (Days 8-9)

**Goal:** Properly parse `${expression}` in template literals

### Task 4.1: Verify Template Parsing

Check if the lexer/parser properly handles template interpolation:

```rust
#[test]
fn test_parse_template_simple_interpolation() {
    let source = "`Hello, ${name}!`;";
    // Should have TemplateLiteral with quasis and expressions
}

#[test]
fn test_parse_template_multiple_interpolations() {
    let source = "`${a} + ${b} = ${a + b}`;";
}

#[test]
fn test_parse_template_nested_object() {
    let source = "`User: ${user.name}, Age: ${user.age}`;";
}

#[test]
fn test_parse_template_function_call() {
    let source = "`Result: ${calculate(x, y)}`;";
}
```

### Task 4.2: Fix Template Interpolation if Needed

If tests fail, update the lexer to properly tokenize template parts and the parser to handle embedded expressions.

---

## Phase 5: Rest Patterns (Day 10)

**Goal:** Support `...rest` in array destructuring patterns

### Task 5.1: Add Rest Pattern to AST

**Check/Modify:** `crates/raya-parser/src/ast/pattern.rs`

Ensure `RestPattern` exists:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Identifier(IdentifierPattern),
    Array(ArrayPattern),
    Object(ObjectPattern),
    Rest(RestPattern),  // Ensure this exists
    // ...
}

#[derive(Debug, Clone, PartialEq)]
pub struct RestPattern {
    pub argument: Box<Pattern>,  // Usually an identifier
    pub span: Span,
}
```

### Task 5.2: Parse Rest in Array Patterns

**Modify:** `crates/raya-parser/src/parser/pattern.rs`

```rust
fn parse_array_pattern(parser: &mut Parser) -> Result<Pattern, ParseError> {
    // ...
    while !parser.check(&Token::RightBracket) {
        // Check for rest element: ...rest
        if parser.check(&Token::DotDotDot) {
            parser.advance();
            let rest_pattern = parse_pattern(parser)?;
            elements.push(ArrayPatternElement::Rest(RestPattern {
                argument: Box::new(rest_pattern),
                span: parser.combine_spans(&start, &parser.current_span()),
            }));
            break; // Rest must be last
        }

        // Regular element
        elements.push(parse_array_pattern_element(parser)?);

        if !parser.check(&Token::RightBracket) {
            parser.expect(Token::Comma)?;
        }
    }
    // ...
}
```

### Task 5.3: Rest Pattern Tests

```rust
#[test]
fn test_parse_rest_pattern_array() {
    let source = "let [first, ...rest] = arr;";
}

#[test]
fn test_parse_rest_pattern_function_param() {
    let source = "function sum(...nums: number[]): number { return 0; }";
}

#[test]
fn test_parse_rest_pattern_middle_error() {
    // Rest must be last
    let source = "let [...rest, last] = arr;";
    // Should error
}
```

---

## Phase 6: Verification & Testing (Days 11-12)

### Task 6.1: Integration Tests

Create comprehensive tests combining all features:

```rust
#[test]
fn test_full_class_with_all_features() {
    let source = r#"
        @sealed
        abstract class BaseService {
            private static instances: number = 0;
            protected readonly name: string;

            constructor(name: string) {
                this.name = name;
                BaseService.instances += 1;
            }

            @logged
            protected abstract process(): Task<void>;

            public static getInstanceCount(): number {
                return BaseService.instances;
            }
        }
    "#;
}
```

### Task 6.2: Run All Tests

```bash
cargo test --workspace
cargo test -p raya-parser
```

### Task 6.3: Update Documentation

- Update README.md with new features
- Update any API documentation

---

## Success Criteria

- ✅ Access modifiers (private/protected/public) parsed correctly
- ✅ Visibility stored in FieldDecl and MethodDecl
- ✅ Static fields/methods work with all visibility modifiers
- ✅ Decorators parsed on classes, methods, fields, parameters
- ✅ Template literal interpolation handles complex expressions
- ✅ Rest patterns work in array destructuring
- ✅ All new tests passing
- ✅ No regressions in existing tests

---

## Files to Modify

### AST Changes
- `crates/raya-parser/src/ast/statement.rs` - Add Visibility enum, update FieldDecl/MethodDecl
- `crates/raya-parser/src/ast/pattern.rs` - Verify RestPattern exists

### Parser Changes
- `crates/raya-parser/src/parser/stmt.rs` - Parse visibility, decorators
- `crates/raya-parser/src/parser/pattern.rs` - Parse rest patterns

### Visitor Changes
- `crates/raya-parser/src/ast/visitor.rs` - Update if AST changes

### New Test Files
- `crates/raya-parser/tests/visibility_test.rs`
- `crates/raya-parser/tests/decorator_test.rs`
- `crates/raya-parser/tests/template_test.rs`
- `crates/raya-parser/tests/rest_pattern_test.rs`

---

## Implementation Order

1. **Days 1-2:** Access modifiers (Visibility enum + parsing)
2. **Days 3-4:** Static verification and testing
3. **Days 5-7:** Decorator parsing
4. **Days 8-9:** Template literal verification/fixes
5. **Day 10:** Rest patterns
6. **Days 11-12:** Integration tests and verification

---

## Related Milestones

- Milestone 2.3 (Parser Foundation) ✅
- Milestone 2.9 (Advanced Parser Features) ✅
- Milestone 2.10 (Parser Hardening) ✅
- Milestone 3.1 (Checker Integration) - Depends on this
