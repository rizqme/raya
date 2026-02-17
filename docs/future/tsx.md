---
title: "TSX/JSX Support"
---

# TSX Support in Raya

> **Status:** Future (Not Implemented)
> **Version:** 1.0
> **Related:** [Language Spec](../language/lang.md)

---

## Table of Contents

1. [Overview](#overview)
2. [Design Principles](#design-principles)
3. [JSX Syntax](#jsx-syntax)
4. [AST Extensions](#ast-extensions)
5. [Lexer Extensions](#lexer-extensions)
6. [Type System Integration](#type-system-integration)
7. [Compilation Strategy](#compilation-strategy)
8. [Examples](#examples)
9. [Implementation Plan](#implementation-plan)

---

## Overview

**TSX** (TypeScript + JSX) support enables Raya to parse and compile JSX syntax for building user interfaces. JSX is a syntax extension that allows embedding XML-like markup directly in code.

### What is JSX?

JSX is a declarative syntax for describing UI component trees:

```tsx
const element = <div className="container">Hello, {name}!</div>;

const component = (
  <Button onClick={handleClick} disabled={isLoading}>
    {isLoading ? <Spinner /> : "Submit"}
  </Button>
);
```

### Goals

1. **Full JSX Compatibility**: Support React/Preact-compatible JSX syntax
2. **Type Safety**: Full type checking for JSX elements and props
3. **Zero Runtime Cost**: Compile to regular function calls
4. **Configurable**: Support different JSX factories (React.createElement, h, etc.)

### Non-Goals

1. **React Runtime**: No bundled React/Preact (bring your own)
2. **Custom JSX Extensions**: Stick to standard JSX syntax
3. **JSX Transformations**: No special optimizations beyond standard compilation

---

## Design Principles

### 1. JSX is Syntactic Sugar

JSX compiles to regular function calls:

```tsx
// Source
<div className="hello">World</div>

// Compiles to
createElement("div", { className: "hello" }, "World")
```

### 2. Type-Safe Props

Every JSX element type is checked:

```tsx
// ✅ Valid: Button has onClick and children props
<Button onClick={() => logger.info("clicked")}>Click me</Button>

// ❌ Error: Button doesn't have invalidProp
<Button invalidProp="value">Click me</Button>
```

### 3. Components are Functions or Classes

```tsx
// Function component
function Welcome(props: { name: string }) {
  return <h1>Hello, {props.name}</h1>;
}

// Class component
class Counter {
  render() {
    return <div>{this.state.count}</div>;
  }
}
```

### 4. Fragments

Support React-style fragments:

```tsx
// Fragment shorthand
<>
  <div>First</div>
  <div>Second</div>
</>

// Named fragment
<Fragment>
  <div>First</div>
  <div>Second</div>
</Fragment>
```

---

## JSX Syntax

### Elements

```tsx
// Self-closing
<img src="photo.jpg" alt="Photo" />

// With children
<div>
  <h1>Title</h1>
  <p>Content</p>
</div>

// With expressions
<div className={isActive ? "active" : "inactive"}>
  {user.name}
</div>
```

### Attributes

```tsx
// String literals
<div id="main" className="container" />

// Expressions
<input value={text} onChange={handleChange} />

// Spread attributes
<Component {...props} />

// Boolean attributes
<button disabled />
<button disabled={true} />
```

### Children

```tsx
// Text
<div>Hello World</div>

// Expressions
<div>{2 + 2}</div>

// Nested elements
<div>
  <span>Nested</span>
</div>

// Mixed
<div>
  Text and {expression} and <span>element</span>
</div>

// Arrays
<ul>
  {items.map(item => <li key={item.id}>{item.name}</li>)}
</ul>
```

### Fragments

```tsx
// Short syntax
<>
  <td>First</td>
  <td>Second</td>
</>

// Long syntax (allows keys)
<Fragment key={id}>
  <td>First</td>
  <td>Second</td>
</Fragment>
```

### Comments

```tsx
<div>
  {/* This is a comment */}
  <span>Content</span>
</div>
```

---

## AST Extensions

### New Expression Nodes

Add to `crates/raya-parser/src/ast/expression.rs`:

```rust
/// Expression enum - add these variants
pub enum Expression {
    // ... existing variants ...

    /// JSX element: <div>content</div>
    JsxElement(JsxElement),

    /// JSX fragment: <>content</>
    JsxFragment(JsxFragment),
}

// ============================================================================
// JSX AST Nodes
// ============================================================================

/// JSX element: <div className="foo">Hello</div>
#[derive(Debug, Clone, PartialEq)]
pub struct JsxElement {
    /// Opening tag with name and attributes
    pub opening: JsxOpeningElement,

    /// Children elements, text, or expressions
    pub children: Vec<JsxChild>,

    /// Optional closing tag (None for self-closing)
    pub closing: Option<JsxClosingElement>,

    pub span: Span,
}

/// JSX opening tag: <div className="foo">
#[derive(Debug, Clone, PartialEq)]
pub struct JsxOpeningElement {
    /// Element name (div, Button, etc.)
    pub name: JsxElementName,

    /// Attributes
    pub attributes: Vec<JsxAttribute>,

    /// Self-closing? <div />
    pub self_closing: bool,

    pub span: Span,
}

/// JSX closing tag: </div>
#[derive(Debug, Clone, PartialEq)]
pub struct JsxClosingElement {
    pub name: JsxElementName,
    pub span: Span,
}

/// JSX element name
#[derive(Debug, Clone, PartialEq)]
pub enum JsxElementName {
    /// Simple identifier: div, span, Button
    Identifier(Identifier),

    /// Namespaced: svg:path
    Namespaced {
        namespace: Identifier,
        name: Identifier,
    },

    /// Member expression: React.Fragment, UI.Button
    MemberExpression {
        object: Box<JsxElementName>,
        property: Identifier,
    },
}

/// JSX attribute
#[derive(Debug, Clone, PartialEq)]
pub enum JsxAttribute {
    /// Regular attribute: className="foo"
    Attribute {
        name: JsxAttributeName,
        value: Option<JsxAttributeValue>,
        span: Span,
    },

    /// Spread attribute: {...props}
    Spread {
        argument: Expression,
        span: Span,
    },
}

/// JSX attribute name
#[derive(Debug, Clone, PartialEq)]
pub enum JsxAttributeName {
    /// Simple: className
    Identifier(Identifier),

    /// Namespaced: xml:lang
    Namespaced {
        namespace: Identifier,
        name: Identifier,
    },
}

/// JSX attribute value
#[derive(Debug, Clone, PartialEq)]
pub enum JsxAttributeValue {
    /// String literal: "value"
    StringLiteral(StringLiteral),

    /// Expression: {value}
    Expression(Expression),

    /// Nested element: <Component prop={<div />} />
    JsxElement(Box<JsxElement>),

    /// Fragment: <Component prop={<>...</>} />
    JsxFragment(Box<JsxFragment>),
}

/// JSX child node
#[derive(Debug, Clone, PartialEq)]
pub enum JsxChild {
    /// Text content
    Text(JsxText),

    /// Element: <div />
    Element(JsxElement),

    /// Fragment: <>...</>
    Fragment(JsxFragment),

    /// Expression: {value}
    Expression(JsxExpression),
}

/// JSX text content
#[derive(Debug, Clone, PartialEq)]
pub struct JsxText {
    pub value: String,
    pub raw: String, // Preserves whitespace
    pub span: Span,
}

/// JSX expression: {value}
#[derive(Debug, Clone, PartialEq)]
pub struct JsxExpression {
    pub expression: Option<Expression>, // None for empty {}
    pub span: Span,
}

/// JSX fragment: <>children</>
#[derive(Debug, Clone, PartialEq)]
pub struct JsxFragment {
    /// Opening: <>
    pub opening: JsxOpeningFragment,

    /// Children
    pub children: Vec<JsxChild>,

    /// Closing: </>
    pub closing: JsxClosingFragment,

    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JsxOpeningFragment {
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JsxClosingFragment {
    pub span: Span,
}
```

---

## Lexer Extensions

### New Tokens

Add to `crates/raya-parser/src/token.rs`:

```rust
pub enum Token {
    // ... existing tokens ...

    // JSX-specific tokens
    JsxTagStart,        // <
    JsxTagEnd,          // >
    JsxSelfClose,       // />
    JsxCloseTagStart,   // </
    JsxText(String),    // Text between tags
    JsxExprStart,       // {
    JsxExprEnd,         // }
}
```

### Context-Aware Lexing

The lexer must switch modes when entering/exiting JSX:

```rust
enum LexerMode {
    Normal,      // Regular TypeScript
    JsxTag,      // Inside <tag ...>
    JsxChildren, // Between <tag> and </tag>
}
```

**Example:**

```tsx
const x = <div className="foo">Hello {name}</div>;
         ^                             ^     ^
         |                             |     |
    Switch to JSX                   Expr   Normal
```

### Whitespace Handling

JSX preserves whitespace differently:

```tsx
// Multiple spaces/newlines collapsed to single space
<div>
  Hello    World
</div>
// → "Hello World"

// Leading/trailing whitespace trimmed
<div>
  Content
</div>
// → "Content"
```

---

## Type System Integration

### JSX Element Types

Every JSX element has an inferred type based on its tag:

```tsx
// Intrinsic element (HTML tag)
const div: JSX.Element = <div />;

// Component element
function Button(props: { text: string }): JSX.Element {
  return <button>{props.text}</button>;
}

const btn: JSX.Element = <Button text="Click" />;
```

### Intrinsic Elements

Define global namespace for HTML elements:

```tsx
namespace JSX {
  interface IntrinsicElements {
    div: HTMLAttributes<HTMLDivElement>;
    span: HTMLAttributes<HTMLSpanElement>;
    button: HTMLAttributes<HTMLButtonElement>;
    // ... all HTML elements
  }

  interface HTMLAttributes<T> {
    className?: string;
    id?: string;
    onClick?: (event: MouseEvent) => void;
    // ... common HTML attributes
  }
}
```

### Component Props

Function components define props via parameter types:

```tsx
// Props are inferred from function parameter
function Welcome(props: { name: string; age?: number }) {
  return <h1>Hello, {props.name}</h1>;
}

// ✅ Valid
<Welcome name="Alice" />
<Welcome name="Bob" age={30} />

// ❌ Type error: missing required prop 'name'
<Welcome age={25} />
```

### Children Type

```tsx
interface PropsWithChildren<P = {}> {
  children?: JSX.Element | JSX.Element[] | string;
}

function Container(props: PropsWithChildren<{ title: string }>) {
  return (
    <div>
      <h1>{props.title}</h1>
      {props.children}
    </div>
  );
}
```

---

## Compilation Strategy

### JSX Factory

JSX compiles to function calls. The factory is configurable:

```tsx
// raya.toml
[jsx]
factory = "createElement"       // Function name
fragment = "Fragment"           // Fragment component
```

### Compilation Examples

#### Simple Element

```tsx
// Source
<div className="container">Hello</div>

// Compiled
createElement("div", { className: "container" }, "Hello")
```

#### Component with Props

```tsx
// Source
<Button onClick={handleClick} disabled>
  Submit
</Button>

// Compiled
createElement(
  Button,
  { onClick: handleClick, disabled: true },
  "Submit"
)
```

#### Fragment

```tsx
// Source
<>
  <div>First</div>
  <div>Second</div>
</>

// Compiled
createElement(
  Fragment,
  null,
  createElement("div", null, "First"),
  createElement("div", null, "Second")
)
```

#### Spread Attributes

```tsx
// Source
<Button {...props} disabled />

// Compiled
createElement(
  Button,
  { ...props, disabled: true }
)
```

#### Nested Elements

```tsx
// Source
<div>
  <h1>Title</h1>
  <p>Content: {text}</p>
</div>

// Compiled
createElement(
  "div",
  null,
  createElement("h1", null, "Title"),
  createElement("p", null, "Content: ", text)
)
```

### Children Flattening

Arrays are flattened:

```tsx
// Source
<ul>
  {items.map(item => <li>{item}</li>)}
</ul>

// Compiled
createElement(
  "ul",
  null,
  ...items.map(item => createElement("li", null, item))
)
```

---

## Examples

### Counter Component

```tsx
interface CounterProps {
  initialCount?: number;
}

function Counter(props: CounterProps) {
  const [count, setCount] = useState(props.initialCount ?? 0);

  return (
    <div className="counter">
      <h1>Count: {count}</h1>
      <button onClick={() => setCount(count + 1)}>
        Increment
      </button>
      <button onClick={() => setCount(count - 1)}>
        Decrement
      </button>
    </div>
  );
}
```

### List Rendering

```tsx
interface Item {
  id: string;
  text: string;
}

function ItemList(props: { items: Item[] }) {
  return (
    <ul className="item-list">
      {props.items.map(item => (
        <li key={item.id}>{item.text}</li>
      ))}
    </ul>
  );
}
```

### Conditional Rendering

```tsx
function Greeting(props: { user?: { name: string } }) {
  if (props.user) {
    return <h1>Welcome, {props.user.name}!</h1>;
  } else {
    return <h1>Please sign in</h1>;
  }
}

// Inline ternary
function Status(props: { isLoading: boolean }) {
  return (
    <div>
      {props.isLoading ? <Spinner /> : <Content />}
    </div>
  );
}
```

### Complex Composition

```tsx
function App() {
  const [todos, setTodos] = useState<Todo[]>([]);

  const addTodo = (text: string) => {
    setTodos([...todos, { id: crypto.randomUUID(), text, done: false }]);
  };

  const toggleTodo = (id: string) => {
    setTodos(todos.map(todo =>
      todo.id === id ? { ...todo, done: !todo.done } : todo
    ));
  };

  return (
    <div className="app">
      <Header>
        <h1>Todo App</h1>
      </Header>

      <AddTodoForm onSubmit={addTodo} />

      <TodoList
        todos={todos}
        onToggle={toggleTodo}
      />

      <Footer>
        {todos.filter(t => !t.done).length} items left
      </Footer>
    </div>
  );
}
```

---

## Implementation Plan

### Phase 1: AST Extensions (Week 1)

**Tasks:**
- [ ] Add JSX AST nodes to `expression.rs`
- [ ] Update `Expression` enum with `JsxElement` and `JsxFragment`
- [ ] Add helper methods for JSX nodes
- [ ] Write unit tests for AST construction

**Deliverables:**
- Complete JSX AST definitions
- 20+ unit tests for JSX nodes

### Phase 2: Lexer Extensions (Week 2)

**Tasks:**
- [ ] Add JSX token types to `token.rs`
- [ ] Implement context-aware lexing (Normal/JsxTag/JsxChildren modes)
- [ ] Handle JSX whitespace rules
- [ ] Handle JSX entity escaping
- [ ] Write lexer tests for JSX

**Deliverables:**
- JSX-aware lexer
- 30+ lexer tests for JSX syntax

### Phase 3: Parser Extensions (Week 3)

**Tasks:**
- [ ] Parse JSX elements
- [ ] Parse JSX fragments
- [ ] Parse JSX attributes (including spread)
- [ ] Parse JSX children (text, elements, expressions)
- [ ] Validate matching opening/closing tags
- [ ] Write parser tests

**Deliverables:**
- Complete JSX parser
- 40+ parser tests

### Phase 4: Type Checker Integration (Week 4)

**Tasks:**
- [ ] Define `JSX` namespace types
- [ ] Implement intrinsic element checking
- [ ] Implement component prop checking
- [ ] Check JSX expression types
- [ ] Write type checker tests

**Deliverables:**
- Type-safe JSX
- 30+ type checker tests

### Phase 5: Code Generation (Week 5)

**Tasks:**
- [ ] Compile JSX to `createElement` calls
- [ ] Handle fragment compilation
- [ ] Handle spread attributes
- [ ] Handle children flattening
- [ ] Optimize static elements
- [ ] Write codegen tests

**Deliverables:**
- JSX → bytecode compilation
- 25+ codegen tests

---

## Configuration

Add to `raya.toml`:

```toml
[jsx]
# JSX factory function name (default: createElement)
factory = "createElement"

# Fragment component name (default: Fragment)
fragment = "Fragment"

# JSX factory module (if imported)
# factory_module = "react"

# Development mode (adds __source and __self props)
development = false
```

Usage in `.raya` files:

```tsx
// Automatically recognized by .tsx or .raya.tsx extension
const element = <div>Hello</div>;
```

---

## Success Criteria

- ✅ Parse all valid JSX syntax
- ✅ Reject invalid JSX with clear errors
- ✅ Type-check JSX element props
- ✅ Compile JSX to function calls
- ✅ 145+ tests passing (20 AST + 30 lexer + 40 parser + 30 types + 25 codegen)
- ✅ Full compatibility with React/Preact patterns

---

## References

- **JSX Specification**: https://facebook.github.io/jsx/
- **TypeScript JSX**: https://www.typescriptlang.org/docs/handbook/jsx.html
- **React JSX**: https://react.dev/learn/writing-markup-with-jsx
- **Babel JSX Transform**: https://babeljs.io/docs/en/babel-plugin-transform-react-jsx

---

**Implementation Status:** 🔄 Not Started
**Target Completion:** 5 weeks (1 week per phase)
