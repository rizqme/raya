# ast module

Abstract Syntax Tree definitions for Raya.

## Module Structure

```
ast/
├── mod.rs          # Re-exports, Module struct
├── statement.rs    # Statement AST nodes
├── expression.rs   # Expression AST nodes
├── types.rs        # Type annotation AST nodes
├── pattern.rs      # Pattern matching AST nodes
└── visitor.rs      # Visitor trait for AST traversal
```

## Key Types

### Module (Root)
```rust
pub struct Module {
    pub statements: Vec<Statement>,
    pub span: Span,
}
```

### Statements (`statement.rs`)
```rust
pub enum Statement {
    VariableDecl(VariableDecl),
    FunctionDecl(FunctionDecl),
    ClassDecl(ClassDecl),
    TypeAliasDecl(TypeAliasDecl),
    ImportDecl(ImportDecl),
    ExportDecl(ExportDecl),
    Expression(ExpressionStatement),
    If(IfStatement),
    While(WhileStatement),
    For(ForStatement),
    ForOf(ForOfStatement),
    DoWhile(DoWhileStatement),
    Return(ReturnStatement),
    Break(BreakStatement),
    Continue(ContinueStatement),
    Throw(ThrowStatement),
    Try(TryStatement),
    Switch(SwitchStatement),
    Block(BlockStatement),
    Empty(EmptyStatement),
}
```

### Expressions (`expression.rs`)
```rust
pub enum Expression {
    Identifier(Identifier),
    IntLiteral(IntLiteral),
    FloatLiteral(FloatLiteral),
    StringLiteral(StringLiteral),
    BooleanLiteral(BooleanLiteral),
    NullLiteral(NullLiteral),
    ArrayLiteral(ArrayLiteral),
    ObjectLiteral(ObjectLiteral),
    Binary(BinaryExpression),
    Unary(UnaryExpression),
    Assignment(AssignmentExpression),
    Call(CallExpression),
    Member(MemberExpression),
    Index(IndexExpression),
    Conditional(ConditionalExpression),
    Arrow(ArrowFunction),
    New(NewExpression),
    This(ThisExpression),
    Await(AwaitExpression),
    TypeOf(TypeOfExpression),
    InstanceOf(InstanceOfExpression),
    As(AsExpression),
    Parenthesized(ParenthesizedExpression),
    Template(TemplateExpression),
}
```

### Type Annotations (`types.rs`)
```rust
pub enum Type {
    Primitive(PrimitiveType),     // number, string, boolean, null, void
    Reference(TypeReference),      // MyClass, Array<T>
    Array(ArrayType),              // T[]
    Tuple(TupleType),              // [T, U, V]
    Function(FunctionType),        // (a: T) => U
    Object(ObjectType),            // { x: T; y: U }
    Union(UnionType),              // T | U
    Intersection(IntersectionType),// T & U
    Conditional(ConditionalType),  // T extends U ? V : W
}
```

### Patterns (`pattern.rs`)
```rust
pub enum Pattern {
    Identifier(Identifier),
    Object(ObjectPattern),
    Array(ArrayPattern),
    Rest(RestPattern),
}
```

## Class Field Modifiers

`FieldDecl` supports these modifiers:
- `visibility`: `Public` (default), `Protected`, `Private`
- `is_static`: `static` keyword
- `is_readonly`: `readonly` keyword — field can only be assigned in the constructor

`ObjectTypeProperty` supports `readonly: bool` for readonly properties in object type annotations:
```typescript
type Config = { readonly host: string; port: number; }
```

## Annotations

AST nodes can have annotations (for features like JSON field mapping):
```rust
pub struct Annotation {
    pub tag: String,      // e.g., "json"
    pub args: Vec<String>, // e.g., ["user_name", "omitEmpty"]
}

// Usage: //@@json user_name omitEmpty
```

## Visitor Pattern

```rust
pub trait Visitor {
    fn visit_module(&mut self, module: &Module);
    fn visit_statement(&mut self, stmt: &Statement);
    fn visit_expression(&mut self, expr: &Expression);
    // ... etc
}
```

## For AI Assistants

- All nodes include `span: Span` for error reporting
- Identifiers use `Symbol` (interned string reference)
- Box is used for recursive structures to avoid infinite size
- Annotations are parsed from special comments `//@@tag args...`
- Visitor trait enables AST traversal without modifying nodes
