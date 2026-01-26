# Milestone 2.9: Advanced Parser Features

**Duration:** 2-3 weeks
**Status:** 🔴 Not Started
**Dependencies:**
- Milestone 2.3 (Parser) ✅ Complete
- Milestone 2.5 (Type Checker) ✅ Complete
**Next Milestone:** 3.1 (IR - Intermediate Representation)

---

## Table of Contents

1. [Overview](#overview)
2. [Goals](#goals)
3. [Non-Goals](#non-goals)
4. [Phase 1: Destructuring Patterns](#phase-1-destructuring-patterns-week-1)
5. [Phase 2: JSX/TSX Support](#phase-2-jsxtsx-support-week-2)
6. [Phase 3: Advanced Features](#phase-3-advanced-features-week-3)
7. [Testing Strategy](#testing-strategy)
8. [Success Criteria](#success-criteria)

---

## Overview

This milestone adds advanced parsing features that were deferred from Milestone 2.3. These features enhance developer ergonomics and enable UI framework integration but are not required for the core compiler pipeline.

### Features Covered

1. **Destructuring patterns** - Array and object destructuring in variable declarations
2. **JSX/TSX** - Declarative UI syntax for framework integration
3. **Spread/rest operators** - `...` syntax for arrays, objects, and function parameters
4. **Decorators** - Metadata and code generation hooks
5. **Computed property names** - Dynamic object keys `[expr]: value`

---

## Goals

### Primary Goals

1. **Destructuring Patterns**
   - Array destructuring: `let [a, b] = array`
   - Object destructuring: `let { x, y } = object`
   - Nested destructuring: `let { a: [b, c] } = obj`
   - Default values: `let [a = 10] = array`
   - Rest elements: `let [first, ...rest] = array`

2. **JSX/TSX Parsing**
   - JSX elements: `<div className="foo">content</div>`
   - JSX fragments: `<>content</>`
   - JSX expressions: `<div>{expr}</div>`
   - Self-closing tags: `<img src="..." />`
   - Spread attributes: `<Component {...props} />`

3. **Spread/Rest Operators**
   - Array spread: `[...arr1, ...arr2]`
   - Object spread: `{ ...obj1, ...obj2 }`
   - Rest parameters: `function foo(...args) {}`

4. **Decorator Parsing**
   - Class decorators: `@sealed class Foo {}`
   - Method decorators: `@memoized method() {}`
   - Property decorators: `@readonly prop: number`
   - Decorator factories: `@logged(level: "debug")`

### Secondary Goals

- Computed property names in object literals
- Template literal type annotations
- Better error messages for destructuring mistakes
- JSX-specific type checking integration

---

## Non-Goals

1. **Decorator Execution** - Runtime decorator logic (handled by compiler/runtime)
2. **JSX Transformation** - Converting JSX to function calls (handled by compiler)
3. **Type Checking** - Semantic analysis of these features (handled by type checker)
4. **React-Specific Features** - Hooks, effects, etc. (framework-level concerns)

---

## Phase 1: Destructuring Patterns (Week 1)

**Goal:** Parse all destructuring patterns in variable declarations and function parameters

### Task 1.1: Array Destructuring

**New/Modified files:** `crates/raya-parser/src/parser/pattern.rs`

Implement array destructuring patterns:

```typescript
// Basic array destructuring
let [a, b] = [1, 2];

// With defaults
let [x = 10, y = 20] = array;

// Nested destructuring
let [[a, b], [c, d]] = matrix;

// Rest elements
let [first, ...rest] = array;

// Skip elements
let [a, , c] = [1, 2, 3];
```

**AST Updates:**
```rust
// Already defined in crates/raya-parser/src/ast/pattern.rs
pub enum Pattern {
    Identifier(Identifier),
    Array(ArrayPattern),  // Implement this
    Object(ObjectPattern), // Implement this
}

pub struct ArrayPattern {
    pub elements: Vec<Option<Pattern>>, // None for skipped elements
    pub rest: Option<Box<Pattern>>,
    pub span: Span,
}
```

**Parser functions:**
```rust
fn parse_array_pattern(parser: &mut Parser) -> Result<ArrayPattern, ParseError> {
    // Parse [ element1, element2, ...rest ]
}

fn parse_pattern_with_default(parser: &mut Parser) -> Result<(Pattern, Option<Expression>), ParseError> {
    // Parse: pattern = defaultValue
}
```

### Task 1.2: Object Destructuring

```typescript
// Basic object destructuring
let { x, y } = point;

// Renaming
let { x: newX, y: newY } = point;

// With defaults
let { x = 0, y = 0 } = partial;

// Nested destructuring
let { position: { x, y } } = entity;

// Rest properties
let { x, ...rest } = object;
```

**AST Updates:**
```rust
pub struct ObjectPattern {
    pub properties: Vec<ObjectPatternProperty>,
    pub rest: Option<Identifier>,
    pub span: Span,
}

pub struct ObjectPatternProperty {
    pub key: Identifier,
    pub value: Pattern,
    pub default: Option<Expression>,
    pub span: Span,
}
```

**Parser functions:**
```rust
fn parse_object_pattern(parser: &mut Parser) -> Result<ObjectPattern, ParseError> {
    // Parse { prop1, prop2: renamed, ...rest }
}
```

### Task 1.3: Integration with Variable Declarations

**Modified files:** `crates/raya-parser/src/parser/stmt.rs`

Update `parse_variable_declaration` to support destructuring:

```rust
fn parse_variable_declaration(parser: &mut Parser) -> Result<Statement, ParseError> {
    // Already implemented, now call parse_pattern instead of just identifier
    let pattern = parse_pattern(parser)?;

    // For const, require initializer
    if kind == VariableKind::Const && initializer.is_none() {
        return Err(...);
    }
}
```

### Verification (Phase 1)

**Tests:** `crates/raya-parser/tests/pattern_test.rs`

```rust
#[test] fn test_array_destructuring();
#[test] fn test_array_destructuring_with_defaults();
#[test] fn test_array_destructuring_nested();
#[test] fn test_array_destructuring_with_rest();
#[test] fn test_object_destructuring();
#[test] fn test_object_destructuring_with_renaming();
#[test] fn test_object_destructuring_with_defaults();
#[test] fn test_object_destructuring_nested();
```

**Success Criteria:**
- ✅ Parse all destructuring forms
- ✅ Generate correct AST patterns
- ✅ Handle edge cases (empty arrays, missing properties)
- ✅ 15+ tests passing

---

## Phase 2: JSX/TSX Support (Week 2)

**Goal:** Parse JSX syntax for UI frameworks

### Task 2.1: JSX Infrastructure

**New file:** `crates/raya-parser/src/parser/jsx.rs`

**AST Updates:** `crates/raya-parser/src/ast/jsx.rs` (new file)

```rust
pub struct JsxElement {
    pub opening: JsxOpeningElement,
    pub children: Vec<JsxChild>,
    pub closing: Option<JsxClosingElement>,
    pub span: Span,
}

pub struct JsxOpeningElement {
    pub name: JsxName,
    pub attributes: Vec<JsxAttribute>,
    pub self_closing: bool,
    pub span: Span,
}

pub enum JsxName {
    Identifier(Identifier),
    MemberExpression(Box<Expression>), // For Foo.Bar
}

pub enum JsxAttribute {
    Attribute {
        name: Identifier,
        value: Option<JsxAttributeValue>,
    },
    SpreadAttribute {
        argument: Expression,
    },
}

pub enum JsxAttributeValue {
    StringLiteral(String),
    Expression(Expression),
    Element(Box<JsxElement>),
}

pub enum JsxChild {
    Element(JsxElement),
    Expression(Expression),
    Text(String),
}

pub struct JsxFragment {
    pub children: Vec<JsxChild>,
    pub span: Span,
}
```

### Task 2.2: JSX Parsing Functions

```rust
pub fn parse_jsx_element(parser: &mut Parser) -> Result<JsxElement, ParseError> {
    // Parse <Tag attr="value">children</Tag>
}

pub fn parse_jsx_opening_element(parser: &mut Parser) -> Result<JsxOpeningElement, ParseError> {
    // Parse <Tag attr="value"> or <Tag />
}

pub fn parse_jsx_closing_element(parser: &mut Parser) -> Result<JsxClosingElement, ParseError> {
    // Parse </Tag>
}

pub fn parse_jsx_attributes(parser: &mut Parser) -> Result<Vec<JsxAttribute>, ParseError> {
    // Parse attr="value" and {...spread}
}

pub fn parse_jsx_children(parser: &mut Parser) -> Result<Vec<JsxChild>, ParseError> {
    // Parse mixed content: text, {expressions}, <elements>
}

pub fn parse_jsx_fragment(parser: &mut Parser) -> Result<JsxFragment, ParseError> {
    // Parse <>children</>
}
```

### Task 2.3: Lexer Updates for JSX

**Modified file:** `crates/raya-parser/src/lexer.rs`

Add JSX-specific lexing modes:

```rust
enum LexerMode {
    Normal,
    JsxTag,      // Inside < ... >
    JsxChildren, // Between > and <
}

// In JSX children mode:
// - Text content is a single token
// - { switches to expression mode
// - < switches to tag mode
```

### Task 2.4: Integration with Expression Parser

**Modified file:** `crates/raya-parser/src/parser/expr.rs`

Update `parse_primary` to handle JSX:

```rust
fn parse_primary(parser: &mut Parser) -> Result<Expression, ParseError> {
    match parser.current() {
        Token::Less => {
            // Could be JSX or comparison operator
            // Look ahead to distinguish
            if looks_like_jsx_start(parser) {
                let jsx = parse_jsx_element(parser)?;
                Ok(Expression::Jsx(jsx))
            } else {
                // Parse as comparison
            }
        }
        // ... other cases
    }
}

fn looks_like_jsx_start(parser: &Parser) -> bool {
    // Heuristic: < followed by identifier or > (fragment)
    matches!(parser.peek(), Some(Token::Identifier(_)) | Some(Token::Greater))
}
```

### Verification (Phase 2)

**Tests:** `crates/raya-parser/tests/jsx_test.rs`

```rust
#[test] fn test_jsx_element_basic();
#[test] fn test_jsx_element_self_closing();
#[test] fn test_jsx_with_attributes();
#[test] fn test_jsx_with_spread_attributes();
#[test] fn test_jsx_with_children();
#[test] fn test_jsx_with_expressions();
#[test] fn test_jsx_nested_elements();
#[test] fn test_jsx_fragment();
#[test] fn test_jsx_member_expression(); // <Foo.Bar />
```

**Success Criteria:**
- ✅ Parse all JSX forms
- ✅ Handle self-closing tags
- ✅ Support spread attributes
- ✅ Parse JSX fragments
- ✅ 20+ tests passing

---

## Phase 3: Advanced Features (Week 3)

**Goal:** Implement spread/rest operators, decorators, and computed properties

### Task 3.1: Spread/Rest Operators

**Modified files:**
- `crates/raya-parser/src/parser/expr.rs` (array/object spread)
- `crates/raya-parser/src/parser/stmt.rs` (rest parameters)

**Array Spread:**
```typescript
let combined = [...arr1, ...arr2, extra];
```

```rust
// Update ArrayExpression
pub struct ArrayExpression {
    pub elements: Vec<ArrayElement>,
    pub span: Span,
}

pub enum ArrayElement {
    Expression(Expression),
    Spread(Expression),
}
```

**Object Spread:**
```typescript
let merged = { ...obj1, ...obj2, extra: value };
```

```rust
// Update ObjectExpression
pub enum ObjectProperty {
    Property { key: PropertyKey, value: Expression },
    Spread { argument: Expression },
}
```

**Rest Parameters:**
```typescript
function foo(first: number, ...rest: number[]) {}
```

```rust
// Update Parameter
pub struct Parameter {
    pub pattern: Pattern,
    pub type_annotation: Option<TypeAnnotation>,
    pub default: Option<Expression>,
    pub is_rest: bool,  // Add this field
    pub span: Span,
}
```

### Task 3.2: Decorator Parsing

**Modified files:** `crates/raya-parser/src/parser/stmt.rs`

Implement decorator parsing before class/method declarations:

```rust
fn parse_decorators(parser: &mut Parser) -> Result<Vec<Decorator>, ParseError> {
    let mut decorators = Vec::new();

    while parser.check(&Token::At) {
        parser.advance();

        let name = parse_identifier(parser)?;

        // Check for arguments: @decorator(arg1, arg2)
        let arguments = if parser.check(&Token::LeftParen) {
            parser.advance();
            let args = parse_argument_list(parser)?;
            parser.expect(Token::RightParen)?;
            Some(args)
        } else {
            None
        };

        decorators.push(Decorator { name, arguments, span });
    }

    Ok(decorators)
}
```

Update class and method parsing to call `parse_decorators()` first.

### Task 3.3: Computed Property Names

**Modified file:** `crates/raya-parser/src/parser/expr.rs`

Support `[expr]: value` in object literals:

```typescript
let key = "dynamicKey";
let obj = {
    [key]: value,
    [1 + 2]: "three",
};
```

```rust
pub enum PropertyKey {
    Identifier(Identifier),
    StringLiteral(String),
    NumberLiteral(f64),
    Computed(Expression),  // Add this variant
}
```

### Verification (Phase 3)

**Tests:** `crates/raya-parser/tests/advanced_test.rs`

```rust
#[test] fn test_array_spread();
#[test] fn test_object_spread();
#[test] fn test_rest_parameters();
#[test] fn test_decorator_simple();
#[test] fn test_decorator_with_args();
#[test] fn test_multiple_decorators();
#[test] fn test_computed_property_names();
```

**Success Criteria:**
- ✅ Spread operators work in arrays/objects
- ✅ Rest parameters parsed correctly
- ✅ Decorators parsed (execution deferred to compiler)
- ✅ Computed properties supported
- ✅ 10+ tests passing

---

## Testing Strategy

### Unit Tests

- Pattern parsing tests (15+)
- JSX parsing tests (20+)
- Spread/rest operator tests (10+)
- Total: 45+ new tests

### Integration Tests

Test complete programs using new features:

```typescript
// Destructuring + JSX
function Component({ name, age }: Props) {
    return <div>Hello {name}, age {age}</div>;
}

// Spread + decorators
@sealed
class Store {
    @readonly
    state = { ...initialState };

    update(...updates: Partial<State>[]) {
        this.state = updates.reduce((acc, u) => ({ ...acc, ...u }), this.state);
    }
}
```

### Error Handling Tests

- Invalid destructuring patterns
- Mismatched JSX tags
- Invalid decorator syntax
- Spread in invalid positions

---

## Success Criteria

### Must Have

- ✅ All destructuring patterns parse correctly
- ✅ JSX/TSX elements and fragments work
- ✅ Spread/rest operators in all contexts
- ✅ Decorator syntax parsed
- ✅ 45+ new tests passing
- ✅ No regressions in existing tests

### Should Have

- ✅ Helpful error messages for JSX mistakes
- ✅ Type annotations work with destructuring
- ✅ JSX integrates with type checker
- ✅ Performance: no degradation from JSX parsing

### Nice to Have

- JSX-specific error recovery
- Pretty-printing for JSX in AST
- Decorator composition validation

---

## Dependencies

### Required Crates

No new dependencies needed - all features use existing parser infrastructure.

### File Structure

```
crates/raya-parser/src/
├── parser/
│   ├── pattern.rs           # Enhanced with destructuring
│   ├── jsx.rs               # New: JSX parsing
│   └── stmt.rs              # Enhanced with decorators
└── ast/
    ├── pattern.rs           # Enhanced with destructuring AST
    ├── jsx.rs               # New: JSX AST nodes
    └── expression.rs        # Enhanced with spread/rest

tests/
├── pattern_test.rs          # New: destructuring tests
├── jsx_test.rs              # New: JSX tests
└── advanced_test.rs         # New: spread/rest/decorator tests
```

---

## Implementation Notes

### JSX Ambiguity with Generics

JSX `<` conflicts with generic type parameters:

```typescript
// JSX element
<Component />

// Generic function call
foo<T>(arg)
```

**Resolution strategy:**
1. Look ahead after `<`
2. If followed by identifier + `>` or `/`, treat as JSX
3. Otherwise treat as generic or comparison

### Decorator Execution Model

Decorators are **parsed** but not **executed** by the parser:
- Parser creates AST nodes with decorator metadata
- Compiler transforms decorated declarations
- Runtime applies decorator logic

### JSX Transformation

JSX elements transform to function calls during compilation:

```typescript
// Source
<div className="foo">{content}</div>

// Transforms to (compiler phase)
createElement("div", { className: "foo" }, content)
```

This transformation is **NOT** done by the parser.

---

## References

### Language Specification

- [design/LANG.md](../design/LANG.md) - Section 17 (JSX), Section 9 (Decorators)
- [plans/milestone-2.3.md](milestone-2.3.md) - Parser implementation foundation

### Related Milestones

- [Milestone 2.3](milestone-2.3.md) - Parser (✅ Complete)
- [Milestone 2.5](milestone-2.5.md) - Type Checker (✅ Complete)
- [Milestone 3.1](milestone-3.1.md) - IR & Compilation (Next)

### External References

- TypeScript Handbook: Decorators
- React JSX Documentation
- MDN: Destructuring Assignment
