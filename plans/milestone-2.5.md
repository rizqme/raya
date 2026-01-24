# Milestone 2.5: Type Checker & Control Flow Analysis

**Duration:** 4-5 weeks
**Status:** 🔜 Not Started
**Dependencies:**
- Milestone 2.3 (Parser) ✅ Complete
- Milestone 2.4 (Type System) - To be implemented
**Next Milestone:** 2.6 (Discriminant Inference)

---

## Table of Contents

1. [Overview](#overview)
2. [Goals](#goals)
3. [Non-Goals](#non-goals)
4. [Architecture](#architecture)
5. [Phase 1: Symbol Tables & Name Resolution](#phase-1-symbol-tables--name-resolution-week-1)
6. [Phase 2: Basic Type Checking](#phase-2-basic-type-checking-week-2)
7. [Phase 3: Control Flow Analysis & Type Narrowing](#phase-3-control-flow-analysis--type-narrowing-week-3)
8. [Phase 4: Advanced Type Checking](#phase-4-advanced-type-checking-week-4)
9. [Testing Strategy](#testing-strategy)
10. [Success Criteria](#success-criteria)

---

## Overview

Implement a sound type checker for Raya that performs:
- Symbol resolution and scope management
- Type inference and checking
- **Control flow-based type narrowing** (TypeScript-style)
- Discriminated union validation
- Exhaustiveness checking
- **Implicit primitive coercions** (number → string)

### What is Type Checking?

Type checking verifies that a program follows the type system rules. It ensures:
- Variables have consistent types
- Function calls match signatures with implicit primitive coercions
- Operations are valid for their operand types
- **Control flow guards narrow union types correctly**
- **Implicit coercions for primitives** (number → string is automatic)

### Key Difference from TypeScript

**Raya has implicit primitive coercions:**
```typescript
// ✅ OK in Raya (number auto-casts to string):
let a: string | number = 42;
function fn(x: string): void { }
fn(a);  // OK: number in union is auto-cast to string

// ❌ ERROR in Raya (string cannot cast to number):
let b: string | number = "hello";
function gn(x: number): void { }
gn(b);  // ERROR: Cannot cast string to number

// ✅ OK: Subtype widening (Dog → Animal)
let dog: Dog = new Dog();
function handle(animal: Animal): void { }
handle(dog);  // OK: Dog is subtype of Animal

// ✅ OK: Structural subtyping (RaceCar → Car)
type Car = { honk(): void };
type RaceCar = Car & { speed(): void };
let raceCar: RaceCar = { honk() {}, speed() {} };
function drive(car: Car): void { car.honk(); }
drive(raceCar);  // OK: RaceCar has all properties of Car
```

Raya allows **implicit primitive coercions** (number → string), **structural subtyping** (objects with more properties can assign to types with fewer), and **union type coercion** (if all variants can coerce to target).

**Input:** AST from parser (Milestone 2.3)
```rust
IfStatement {
  condition: BinaryExpression {
    left: Typeof(Identifier("id")),
    op: EqualEqualEqual,
    right: StringLiteral("number"),
  },
  then_branch: /* ... */,
  else_branch: /* ... */,
}
```

**Output:** Typed AST with narrowing information
```rust
TypedIfStatement {
  condition: TypedExpression { type: Type::Boolean, /* ... */ },
  then_branch: /* id has type number here */,
  else_branch: /* id has type string here */,
  narrowing_info: NarrowingInfo {
    then_branch: [("id", Type::Number)],
    else_branch: [("id", Type::String)],
  },
}
```

---

## Goals

### Primary Goals

1. **Symbol Resolution**: Build symbol tables with proper scoping
2. **Type Inference**: Infer types for all expressions
3. **Type Checking**: Validate type correctness
4. **Control Flow Type Narrowing**: Narrow union types based on guards
   - `typeof` guards for bare unions
   - Discriminant guards for discriminated unions
   - Null checks
   - Truthiness checks
5. **Exhaustiveness Checking**: Ensure all union variants are handled
6. **Error Reporting**: Clear, actionable type errors

### Secondary Goals

1. **Generic Instantiation**: Track generic type parameters
2. **Type Widening**: Widen types at assignment boundaries
3. **Definite Assignment**: Track variable initialization
4. **Dead Code Detection**: Identify unreachable code after narrowing

---

## Non-Goals

1. **Code Generation**: Emitting bytecode (Milestone 3.1)
2. **Optimization**: Performance optimizations (later)
3. **Incremental Checking**: Re-check only changed code (future)
4. **IDE Features**: Autocompletion, hover info (Milestone 7.x)

---

## Architecture

### Type Checker Pipeline

```
AST → Binder → Type Inference → Control Flow Analysis → Type Validation → Typed AST
       ↓           ↓                    ↓                     ↓
   Symbol Table  Type Map      Narrowing Info         Error List
```

### Core Components

1. **Binder** - Creates symbol tables, resolves names
2. **Type Inferrer** - Assigns types to expressions
3. **Control Flow Analyzer** - Builds control flow graph, performs type narrowing
4. **Type Validator** - Checks type correctness, exhaustiveness

---

## Phase 1: Symbol Tables & Name Resolution (Week 1)

**Goal:** Build symbol tables and resolve all identifiers to their declarations.

### 1.1 Symbol Table Structure

**File:** `crates/raya-types/src/symbols.rs`

```rust
use raya_parser::ast::*;
use std::collections::HashMap;

/// Symbol table for name resolution
#[derive(Debug)]
pub struct SymbolTable {
    /// Scopes (stack of scopes)
    scopes: Vec<Scope>,

    /// Symbol information by ID
    symbols: HashMap<SymbolId, Symbol>,

    /// Next symbol ID
    next_id: SymbolId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SymbolId(u32);

#[derive(Debug)]
pub struct Scope {
    /// Parent scope (None for global)
    parent: Option<usize>,

    /// Symbols defined in this scope
    symbols: HashMap<String, SymbolId>,

    /// Kind of scope (function, block, class, etc.)
    kind: ScopeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    Global,
    Function,
    Block,
    Class,
    Loop,
}

#[derive(Debug)]
pub struct Symbol {
    pub id: SymbolId,
    pub name: String,
    pub kind: SymbolKind,
    pub type_annotation: Option<TypeAnnotation>,
    pub span: Span,
    pub scope: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Variable { is_const: bool },
    Function,
    Class,
    TypeAlias,
    Parameter,
    Field,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self {
            scopes: vec![Scope::new(None, ScopeKind::Global)],
            symbols: HashMap::new(),
            next_id: SymbolId(0),
        }
    }

    /// Push a new scope
    pub fn push_scope(&mut self, kind: ScopeKind) {
        let parent = self.scopes.len() - 1;
        self.scopes.push(Scope::new(Some(parent), kind));
    }

    /// Pop the current scope
    pub fn pop_scope(&mut self) {
        assert!(self.scopes.len() > 1, "Cannot pop global scope");
        self.scopes.pop();
    }

    /// Define a symbol in the current scope
    pub fn define(&mut self, name: String, kind: SymbolKind, span: Span) -> Result<SymbolId, BindError> {
        let current_scope = self.scopes.len() - 1;

        // Check for duplicate in current scope
        if self.scopes[current_scope].symbols.contains_key(&name) {
            return Err(BindError::DuplicateSymbol { name, span });
        }

        let id = self.next_id;
        self.next_id.0 += 1;

        let symbol = Symbol {
            id,
            name: name.clone(),
            kind,
            type_annotation: None,
            span,
            scope: current_scope,
        };

        self.symbols.insert(id, symbol);
        self.scopes[current_scope].symbols.insert(name, id);

        Ok(id)
    }

    /// Resolve a name to a symbol ID
    pub fn resolve(&self, name: &str) -> Option<SymbolId> {
        let mut scope_idx = self.scopes.len() - 1;

        loop {
            if let Some(&id) = self.scopes[scope_idx].symbols.get(name) {
                return Some(id);
            }

            match self.scopes[scope_idx].parent {
                Some(parent) => scope_idx = parent,
                None => return None,
            }
        }
    }

    /// Get symbol by ID
    pub fn get(&self, id: SymbolId) -> Option<&Symbol> {
        self.symbols.get(&id)
    }
}

#[derive(Debug, Clone)]
pub enum BindError {
    DuplicateSymbol { name: String, span: Span },
    UndefinedSymbol { name: String, span: Span },
}
```

### 1.2 Binder Implementation

**File:** `crates/raya-types/src/binder.rs`

```rust
use raya_parser::ast::*;
use crate::symbols::*;

/// Binder - builds symbol table and resolves names
pub struct Binder {
    symbols: SymbolTable,
    errors: Vec<BindError>,
}

impl Binder {
    pub fn new() -> Self {
        Self {
            symbols: SymbolTable::new(),
            errors: Vec::new(),
        }
    }

    pub fn bind_module(&mut self, module: &Module) -> Result<SymbolTable, Vec<BindError>> {
        for stmt in &module.statements {
            self.bind_statement(stmt);
        }

        if self.errors.is_empty() {
            Ok(std::mem::replace(&mut self.symbols, SymbolTable::new()))
        } else {
            Err(std::mem::take(&mut self.errors))
        }
    }

    fn bind_statement(&mut self, stmt: &Statement) {
        match stmt {
            Statement::VariableDecl(decl) => self.bind_variable_decl(decl),
            Statement::FunctionDecl(decl) => self.bind_function_decl(decl),
            Statement::ClassDecl(decl) => self.bind_class_decl(decl),
            Statement::TypeAliasDecl(decl) => self.bind_type_alias_decl(decl),
            Statement::Block(block) => self.bind_block(block),
            Statement::If(if_stmt) => self.bind_if_statement(if_stmt),
            // ... other statements
            _ => {}
        }
    }

    fn bind_variable_decl(&mut self, decl: &VariableDecl) {
        let kind = SymbolKind::Variable {
            is_const: matches!(decl.kind, VariableKind::Const),
        };

        // Extract name from pattern (simplified)
        if let Pattern::Identifier(ident) = &decl.pattern {
            if let Err(err) = self.symbols.define(ident.name.clone(), kind, ident.span) {
                self.errors.push(err);
            }
        }
    }

    fn bind_function_decl(&mut self, decl: &FunctionDecl) {
        // Define function in current scope
        if let Err(err) = self.symbols.define(
            decl.name.name.clone(),
            SymbolKind::Function,
            decl.name.span,
        ) {
            self.errors.push(err);
        }

        // Push function scope for parameters and body
        self.symbols.push_scope(ScopeKind::Function);

        // Bind parameters
        for param in &decl.params {
            if let Pattern::Identifier(ident) = &param.pattern {
                if let Err(err) = self.symbols.define(
                    ident.name.clone(),
                    SymbolKind::Parameter,
                    ident.span,
                ) {
                    self.errors.push(err);
                }
            }
        }

        // Bind body
        self.bind_block(&decl.body);

        self.symbols.pop_scope();
    }

    fn bind_block(&mut self, block: &BlockStatement) {
        self.symbols.push_scope(ScopeKind::Block);

        for stmt in &block.statements {
            self.bind_statement(stmt);
        }

        self.symbols.pop_scope();
    }

    // ... other binding methods
}
```

### 1.3 Testing

**File:** `crates/raya-types/tests/binder_tests.rs`

```rust
#[test]
fn test_simple_variable_binding() {
    let source = "let x: number = 42;";
    let ast = parse(source);
    let mut binder = Binder::new();
    let symbols = binder.bind_module(&ast).unwrap();

    assert!(symbols.resolve("x").is_some());
}

#[test]
fn test_duplicate_variable_error() {
    let source = "let x = 1; let x = 2;";
    let ast = parse(source);
    let mut binder = Binder::new();
    let result = binder.bind_module(&ast);

    assert!(result.is_err());
}

#[test]
fn test_scoping() {
    let source = r#"
        let x = 1;
        {
            let x = 2;  // shadows outer x
        }
    "#;
    // ... test that inner x doesn't conflict
}
```

---

## Phase 2: Basic Type Checking (Week 2)

**Goal:** Infer and check types for expressions and statements (without narrowing).

### 2.1 Type Representation

**File:** `crates/raya-types/src/types.rs`

```rust
use std::sync::Arc;

/// Runtime type representation
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    /// Primitives
    Number,
    String,
    Boolean,
    Null,
    Void,

    /// Union types
    Union(UnionType),

    /// Function types
    Function(FunctionType),

    /// Class types
    Class(ClassType),

    /// Type references
    TypeRef(String),

    /// Generic type parameter
    TypeParam(String),

    /// Array types
    Array(Box<Type>),

    /// Tuple types
    Tuple(Vec<Type>),

    /// Object types
    Object(ObjectType),
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnionType {
    pub variants: Vec<Type>,
    pub discriminant: Option<String>,  // Discriminant field name
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionType {
    pub params: Vec<Type>,
    pub return_type: Box<Type>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassType {
    pub name: String,
    pub fields: Vec<(String, Type)>,
    pub methods: Vec<(String, FunctionType)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ObjectType {
    pub properties: Vec<(String, Type, bool)>,  // name, type, optional
}

impl Type {
    /// Check if this is a bare union (primitives only)
    pub fn is_bare_union(&self) -> bool {
        match self {
            Type::Union(union) => union.variants.iter().all(|t| t.is_primitive()),
            _ => false,
        }
    }

    pub fn is_primitive(&self) -> bool {
        matches!(self, Type::Number | Type::String | Type::Boolean | Type::Null)
    }

    /// Check if type is assignable to target with implicit coercions
    ///
    /// Raya supports implicit primitive coercions:
    /// - number → string (auto-cast)
    /// - Subtype → Supertype (Dog → Animal)
    ///
    /// Examples:
    /// ```rust
    /// // ✅ OK: number in union auto-casts to string
    /// let a: string | number = 42;
    /// fn(a);  // where fn(x: string): void
    ///
    /// // ❌ ERROR: string cannot cast to number
    /// let b: string | number = "hello";
    /// gn(b);  // where gn(x: number): void
    ///
    /// // ✅ OK: Subtype widening
    /// let dog: Dog = new Dog();
    /// handle(dog);  // where handle(x: Animal): void
    /// ```
    pub fn is_assignable_to(&self, target: &Type) -> bool {
        match (self, target) {
            // Exact match
            _ if self == target => true,

            // Union → Target: Check if all variants can coerce to target
            (Type::Union(union), target) => {
                union.variants.iter().all(|variant| variant.is_assignable_to(target))
            }

            // Widening: concrete type to union
            (concrete, Type::Union(union)) => {
                union.variants.iter().any(|v| concrete.is_assignable_to(v))
            }

            // Primitive coercions
            (Type::Number, Type::String) => true,  // number → string auto-cast

            // Subtype to supertype (Dog → Animal)
            (Type::Class(subclass), Type::Class(superclass)) => {
                // Check inheritance chain
                self.is_subtype_of(superclass)
            }

            // Other subtyping rules
            _ => false,
        }
    }

    /// Check if this type can be coerced to target
    pub fn can_coerce_to(&self, target: &Type) -> bool {
        match (self, target) {
            (Type::Number, Type::String) => true,  // number → string
            _ => false,
        }
    }
}
```

### 2.2 Type Checker Core

**File:** `crates/raya-types/src/checker.rs`

```rust
use raya_parser::ast::*;
use crate::types::*;
use crate::symbols::*;
use std::collections::HashMap;

pub struct TypeChecker {
    symbols: SymbolTable,
    type_map: HashMap<SymbolId, Type>,
    errors: Vec<TypeError>,
}

#[derive(Debug, Clone)]
pub struct TypeError {
    pub kind: TypeErrorKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TypeErrorKind {
    TypeMismatch { expected: Type, found: Type },
    UndefinedVariable(String),
    CannotInferType,
    InvalidOperation { op: String, left: Type, right: Type },
}

impl TypeChecker {
    pub fn new(symbols: SymbolTable) -> Self {
        Self {
            symbols,
            type_map: HashMap::new(),
            errors: Vec::new(),
        }
    }

    pub fn check_module(&mut self, module: &Module) -> Result<(), Vec<TypeError>> {
        for stmt in &module.statements {
            self.check_statement(stmt);
        }

        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(std::mem::take(&mut self.errors))
        }
    }

    fn check_statement(&mut self, stmt: &Statement) {
        match stmt {
            Statement::VariableDecl(decl) => self.check_variable_decl(decl),
            Statement::FunctionDecl(decl) => self.check_function_decl(decl),
            Statement::Expression(expr_stmt) => {
                self.infer_expression(&expr_stmt.expression);
            }
            Statement::Return(ret) => {
                if let Some(value) = &ret.value {
                    self.infer_expression(value);
                }
            }
            // ... other statements
            _ => {}
        }
    }

    fn check_variable_decl(&mut self, decl: &VariableDecl) {
        let declared_type = decl.type_annotation.as_ref().map(|ann| self.resolve_type_annotation(ann));

        if let Some(init) = &decl.initializer {
            let inferred_type = self.infer_expression(init);

            if let (Some(declared), Some(inferred)) = (declared_type, &inferred_type) {
                if !inferred.is_assignable_to(declared) {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::TypeMismatch {
                            expected: declared.clone(),
                            found: inferred.clone(),
                        },
                        span: init.span(),
                    });
                }
            }
        }
    }

    fn infer_expression(&mut self, expr: &Expression) -> Option<Type> {
        match expr {
            Expression::IntLiteral(_) => Some(Type::Number),
            Expression::StringLiteral(_) => Some(Type::String),
            Expression::BoolLiteral(_) => Some(Type::Boolean),
            Expression::NullLiteral(_) => Some(Type::Null),

            Expression::Identifier(ident) => {
                if let Some(symbol_id) = self.symbols.resolve(&ident.name) {
                    self.type_map.get(&symbol_id).cloned()
                } else {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::UndefinedVariable(ident.name.clone()),
                        span: ident.span,
                    });
                    None
                }
            }

            Expression::Binary(bin) => self.infer_binary_expression(bin),

            // ... other expressions
            _ => None,
        }
    }

    fn infer_binary_expression(&mut self, expr: &BinaryExpression) -> Option<Type> {
        let left_type = self.infer_expression(&expr.left)?;
        let right_type = self.infer_expression(&expr.right)?;

        match expr.operator {
            BinaryOperator::Add | BinaryOperator::Sub | BinaryOperator::Mul | BinaryOperator::Div => {
                if left_type == Type::Number && right_type == Type::Number {
                    Some(Type::Number)
                } else {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::InvalidOperation {
                            op: format!("{:?}", expr.operator),
                            left: left_type,
                            right: right_type,
                        },
                        span: expr.span,
                    });
                    None
                }
            }

            BinaryOperator::EqualEqual | BinaryOperator::NotEqual => Some(Type::Boolean),

            // ... other operators
            _ => None,
        }
    }

    fn resolve_type_annotation(&self, ann: &TypeAnnotation) -> Type {
        // Convert AST type annotation to Type
        match ann {
            TypeAnnotation::Primitive(prim) => match prim.kind {
                PrimitiveKind::Number => Type::Number,
                PrimitiveKind::String => Type::String,
                PrimitiveKind::Boolean => Type::Boolean,
                PrimitiveKind::Null => Type::Null,
                PrimitiveKind::Void => Type::Void,
            },
            TypeAnnotation::Union(union) => Type::Union(UnionType {
                variants: union.types.iter().map(|t| self.resolve_type_annotation(t)).collect(),
                discriminant: None,  // Inferred later
            }),
            // ... other type annotations
            _ => Type::Void,  // Placeholder
        }
    }
}
```

---

## Phase 3: Control Flow Analysis & Type Narrowing (Week 3)

**Goal:** Implement TypeScript-style control flow-based type narrowing.

### 3.1 Control Flow Graph

**File:** `crates/raya-types/src/control_flow.rs`

```rust
use raya_parser::ast::*;
use crate::types::*;
use crate::symbols::*;
use std::collections::HashMap;

/// Control flow graph node
#[derive(Debug, Clone)]
pub struct CfgNode {
    pub id: NodeId,
    pub kind: NodeKind,
    pub predecessors: Vec<NodeId>,
    pub successors: Vec<NodeId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(usize);

#[derive(Debug, Clone)]
pub enum NodeKind {
    Entry,
    Exit,
    Statement(Statement),
    Expression(Expression),
    Branch {
        condition: Expression,
        then_node: NodeId,
        else_node: Option<NodeId>,
    },
}

/// Type narrowing information
#[derive(Debug, Clone)]
pub struct NarrowingInfo {
    /// Variables narrowed to specific types
    pub narrowed_types: HashMap<SymbolId, Type>,
}

/// Control flow analyzer
pub struct ControlFlowAnalyzer {
    nodes: Vec<CfgNode>,
    next_id: usize,

    /// Type narrowing at each CFG node
    narrowing_map: HashMap<NodeId, NarrowingInfo>,
}

impl ControlFlowAnalyzer {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            next_id: 0,
            narrowing_map: HashMap::new(),
        }
    }

    /// Build CFG for a function body
    pub fn build_cfg(&mut self, body: &BlockStatement) -> NodeId {
        let entry = self.new_node(NodeKind::Entry);
        let exit = self.new_node(NodeKind::Exit);

        // Build CFG for statements
        let mut current = entry;
        for stmt in &body.statements {
            current = self.build_statement_cfg(stmt, current, exit);
        }

        // Connect last statement to exit
        self.add_edge(current, exit);

        entry
    }

    fn build_statement_cfg(&mut self, stmt: &Statement, entry: NodeId, exit: NodeId) -> NodeId {
        match stmt {
            Statement::If(if_stmt) => self.build_if_cfg(if_stmt, entry, exit),
            Statement::While(while_stmt) => self.build_while_cfg(while_stmt, entry, exit),
            // ... other statements
            _ => {
                let node = self.new_node(NodeKind::Statement(stmt.clone()));
                self.add_edge(entry, node);
                node
            }
        }
    }

    fn build_if_cfg(&mut self, if_stmt: &IfStatement, entry: NodeId, exit: NodeId) -> NodeId {
        // Create branch node
        let then_entry = self.new_node(NodeKind::Entry);
        let else_entry = if_stmt.else_branch.is_some() {
            Some(self.new_node(NodeKind::Entry))
        } else {
            None
        };

        let branch = self.new_node(NodeKind::Branch {
            condition: if_stmt.condition.clone(),
            then_node: then_entry,
            else_node: else_entry,
        });

        self.add_edge(entry, branch);

        // Build then branch
        let then_exit = self.build_statement_cfg(&if_stmt.then_branch, then_entry, exit);

        // Build else branch
        let else_exit = if let Some(else_branch) = &if_stmt.else_branch {
            self.build_statement_cfg(else_branch, else_entry.unwrap(), exit)
        } else {
            else_entry.unwrap_or(branch)
        };

        // Merge point
        let merge = self.new_node(NodeKind::Entry);
        self.add_edge(then_exit, merge);
        self.add_edge(else_exit, merge);

        merge
    }

    fn new_node(&mut self, kind: NodeKind) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        self.nodes.push(CfgNode {
            id,
            kind,
            predecessors: Vec::new(),
            successors: Vec::new(),
        });
        id
    }

    fn add_edge(&mut self, from: NodeId, to: NodeId) {
        self.nodes[from.0].successors.push(to);
        self.nodes[to.0].predecessors.push(from);
    }
}
```

### 3.2 Type Guard Recognition

**File:** `crates/raya-types/src/type_guards.rs`

```rust
use raya_parser::ast::*;
use crate::types::*;
use crate::symbols::*;

/// Type guard information extracted from an expression
#[derive(Debug, Clone)]
pub enum TypeGuard {
    /// typeof x === "string"
    Typeof {
        variable: SymbolId,
        type_name: String,
        negated: bool,
    },

    /// x.kind === "ok"
    Discriminant {
        variable: SymbolId,
        field: String,
        value: String,
        negated: bool,
    },

    /// x !== null
    NullCheck {
        variable: SymbolId,
        negated: bool,
    },

    /// Truthiness check
    Truthy {
        variable: SymbolId,
        negated: bool,
    },
}

pub struct TypeGuardAnalyzer<'a> {
    symbols: &'a SymbolTable,
}

impl<'a> TypeGuardAnalyzer<'a> {
    pub fn new(symbols: &'a SymbolTable) -> Self {
        Self { symbols }
    }

    /// Analyze an expression to extract type guard
    pub fn analyze(&self, expr: &Expression) -> Option<TypeGuard> {
        match expr {
            Expression::Binary(bin) => self.analyze_binary(bin),
            Expression::Unary(unary) if matches!(unary.operator, UnaryOperator::Not) => {
                // Negate the guard
                let mut guard = self.analyze(&unary.operand)?;
                guard.negate();
                Some(guard)
            }
            _ => None,
        }
    }

    fn analyze_binary(&self, expr: &BinaryExpression) -> Option<TypeGuard> {
        match expr.operator {
            BinaryOperator::EqualEqualEqual | BinaryOperator::NotEqualEqual => {
                let negated = matches!(expr.operator, BinaryOperator::NotEqualEqual);

                // Check for typeof guard: typeof x === "string"
                if let Some(guard) = self.try_typeof_guard(&expr.left, &expr.right, negated) {
                    return Some(guard);
                }
                if let Some(guard) = self.try_typeof_guard(&expr.right, &expr.left, negated) {
                    return Some(guard);
                }

                // Check for discriminant guard: x.kind === "ok"
                if let Some(guard) = self.try_discriminant_guard(&expr.left, &expr.right, negated) {
                    return Some(guard);
                }
                if let Some(guard) = self.try_discriminant_guard(&expr.right, &expr.left, negated) {
                    return Some(guard);
                }

                // Check for null check: x === null
                if let Some(guard) = self.try_null_check(&expr.left, &expr.right, negated) {
                    return Some(guard);
                }
                if let Some(guard) = self.try_null_check(&expr.right, &expr.left, negated) {
                    return Some(guard);
                }

                None
            }
            _ => None,
        }
    }

    fn try_typeof_guard(&self, left: &Expression, right: &Expression, negated: bool) -> Option<TypeGuard> {
        // Check for: typeof x === "string"
        if let Expression::Unary(unary) = left {
            if matches!(unary.operator, UnaryOperator::Typeof) {
                if let Expression::Identifier(ident) = &*unary.operand {
                    if let Expression::StringLiteral(lit) = right {
                        let symbol_id = self.symbols.resolve(&ident.name)?;
                        return Some(TypeGuard::Typeof {
                            variable: symbol_id,
                            type_name: lit.value.clone(),
                            negated,
                        });
                    }
                }
            }
        }
        None
    }

    fn try_discriminant_guard(&self, left: &Expression, right: &Expression, negated: bool) -> Option<TypeGuard> {
        // Check for: x.kind === "ok"
        if let Expression::Member(member) = left {
            if let Expression::Identifier(base) = &*member.object {
                if let Expression::StringLiteral(lit) = right {
                    let symbol_id = self.symbols.resolve(&base.name)?;
                    return Some(TypeGuard::Discriminant {
                        variable: symbol_id,
                        field: member.property.name.clone(),
                        value: lit.value.clone(),
                        negated,
                    });
                }
            }
        }
        None
    }

    fn try_null_check(&self, left: &Expression, right: &Expression, negated: bool) -> Option<TypeGuard> {
        if let Expression::NullLiteral(_) = right {
            if let Expression::Identifier(ident) = left {
                let symbol_id = self.symbols.resolve(&ident.name)?;
                return Some(TypeGuard::NullCheck {
                    variable: symbol_id,
                    negated,
                });
            }
        }
        None
    }
}

impl TypeGuard {
    fn negate(&mut self) {
        match self {
            TypeGuard::Typeof { negated, .. } => *negated = !*negated,
            TypeGuard::Discriminant { negated, .. } => *negated = !*negated,
            TypeGuard::NullCheck { negated, .. } => *negated = !*negated,
            TypeGuard::Truthy { negated, .. } => *negated = !*negated,
        }
    }
}
```

### 3.3 Type Narrowing

**File:** `crates/raya-types/src/narrowing.rs`

```rust
use crate::types::*;
use crate::type_guards::*;
use crate::symbols::*;
use std::collections::HashMap;

/// Type narrowing engine
pub struct TypeNarrower {
    /// Current type environment
    type_env: HashMap<SymbolId, Type>,
}

impl TypeNarrower {
    pub fn new() -> Self {
        Self {
            type_env: HashMap::new(),
        }
    }

    /// Apply type guard to narrow types
    pub fn apply_guard(&mut self, guard: &TypeGuard, original_type: &Type) -> Type {
        match guard {
            TypeGuard::Typeof { variable, type_name, negated } => {
                self.narrow_typeof(*variable, type_name, *negated, original_type)
            }

            TypeGuard::Discriminant { variable, field, value, negated } => {
                self.narrow_discriminant(*variable, field, value, *negated, original_type)
            }

            TypeGuard::NullCheck { variable, negated } => {
                self.narrow_null_check(*variable, *negated, original_type)
            }

            TypeGuard::Truthy { .. } => original_type.clone(),
        }
    }

    fn narrow_typeof(&mut self, var: SymbolId, type_name: &str, negated: bool, original: &Type) -> Type {
        if let Type::Union(union) = original {
            if union.is_bare_union() {
                // Narrow bare union based on typeof
                let target_type = match type_name {
                    "number" => Type::Number,
                    "string" => Type::String,
                    "boolean" => Type::Boolean,
                    _ => return original.clone(),
                };

                if negated {
                    // Keep all types EXCEPT target_type
                    let remaining: Vec<_> = union.variants.iter()
                        .filter(|t| *t != &target_type)
                        .cloned()
                        .collect();

                    if remaining.len() == 1 {
                        remaining[0].clone()
                    } else {
                        Type::Union(UnionType {
                            variants: remaining,
                            discriminant: None,
                        })
                    }
                } else {
                    // Narrow to target_type
                    target_type
                }
            } else {
                original.clone()
            }
        } else {
            original.clone()
        }
    }

    fn narrow_discriminant(&mut self, var: SymbolId, field: &str, value: &str, negated: bool, original: &Type) -> Type {
        if let Type::Union(union) = original {
            if let Some(discriminant) = &union.discriminant {
                if discriminant == field {
                    if negated {
                        // Keep all variants EXCEPT the one with discriminant = value
                        let remaining: Vec<_> = union.variants.iter()
                            .filter(|variant| !self.matches_discriminant(variant, field, value))
                            .cloned()
                            .collect();

                        if remaining.len() == 1 {
                            remaining[0].clone()
                        } else {
                            Type::Union(UnionType {
                                variants: remaining,
                                discriminant: Some(discriminant.clone()),
                            })
                        }
                    } else {
                        // Narrow to the variant with discriminant = value
                        union.variants.iter()
                            .find(|variant| self.matches_discriminant(variant, field, value))
                            .cloned()
                            .unwrap_or_else(|| original.clone())
                    }
                } else {
                    original.clone()
                }
            } else {
                original.clone()
            }
        } else {
            original.clone()
        }
    }

    fn narrow_null_check(&mut self, var: SymbolId, negated: bool, original: &Type) -> Type {
        if let Type::Union(union) = original {
            if negated {
                // x !== null => remove null from union
                let remaining: Vec<_> = union.variants.iter()
                    .filter(|t| !matches!(t, Type::Null))
                    .cloned()
                    .collect();

                if remaining.len() == 1 {
                    remaining[0].clone()
                } else {
                    Type::Union(UnionType {
                        variants: remaining,
                        discriminant: union.discriminant.clone(),
                    })
                }
            } else {
                // x === null => type is null
                Type::Null
            }
        } else {
            original.clone()
        }
    }

    fn matches_discriminant(&self, variant: &Type, field: &str, value: &str) -> bool {
        // Check if variant matches discriminant field = value
        // Simplified - full implementation would inspect object type
        false  // Placeholder
    }
}
```

### 3.4 Integration with Type Checker

Update `checker.rs` to use control flow analysis:

```rust
impl TypeChecker {
    fn check_if_statement(&mut self, if_stmt: &IfStatement) {
        // Infer condition type
        let cond_type = self.infer_expression(&if_stmt.condition);

        // Analyze type guard
        let guard_analyzer = TypeGuardAnalyzer::new(&self.symbols);
        if let Some(guard) = guard_analyzer.analyze(&if_stmt.condition) {
            // Apply narrowing in then branch
            self.push_narrowing_scope();
            self.apply_type_guard(&guard, false);  // false = not negated
            self.check_statement(&if_stmt.then_branch);
            self.pop_narrowing_scope();

            // Apply narrowing in else branch
            if let Some(else_branch) = &if_stmt.else_branch {
                self.push_narrowing_scope();
                self.apply_type_guard(&guard, true);  // true = negated
                self.check_statement(else_branch);
                self.pop_narrowing_scope();
            }
        } else {
            // No narrowing, just check branches
            self.check_statement(&if_stmt.then_branch);
            if let Some(else_branch) = &if_stmt.else_branch {
                self.check_statement(else_branch);
            }
        }
    }

    fn apply_type_guard(&mut self, guard: &TypeGuard, negate: bool) {
        let mut guard = guard.clone();
        if negate {
            guard.negate();
        }

        // Get original type and narrow it
        match &guard {
            TypeGuard::Typeof { variable, .. } |
            TypeGuard::Discriminant { variable, .. } |
            TypeGuard::NullCheck { variable, .. } => {
                if let Some(original_type) = self.type_map.get(variable).cloned() {
                    let mut narrower = TypeNarrower::new();
                    let narrowed = narrower.apply_guard(&guard, &original_type);
                    self.narrowed_types.insert(*variable, narrowed);
                }
            }
            _ => {}
        }
    }
}
```

---

## Phase 4: Advanced Type Checking (Week 4)

### 4.1 Exhaustiveness Checking

**File:** `crates/raya-types/src/exhaustiveness.rs`

```rust
use crate::types::*;

pub struct ExhaustivenessChecker;

impl ExhaustivenessChecker {
    /// Check if switch/match covers all union variants
    pub fn check_exhaustive(union: &UnionType, covered: &[String]) -> Vec<String> {
        let mut missing = Vec::new();

        for variant in &union.variants {
            // Extract discriminant value from variant
            if let Some(disc_value) = Self::get_discriminant_value(variant, &union.discriminant) {
                if !covered.contains(&disc_value) {
                    missing.push(disc_value);
                }
            }
        }

        missing
    }

    fn get_discriminant_value(variant: &Type, discriminant: &Option<String>) -> Option<String> {
        // Extract discriminant value from object type
        None  // Placeholder
    }
}
```

### 4.2 Generic Type Checking

**File:** `crates/raya-types/src/generics.rs`

```rust
use crate::types::*;
use std::collections::HashMap;

pub struct GenericInstantiator {
    /// Type parameter substitutions
    substitutions: HashMap<String, Type>,
}

impl GenericInstantiator {
    pub fn instantiate(&self, ty: &Type) -> Type {
        match ty {
            Type::TypeParam(name) => {
                self.substitutions.get(name).cloned().unwrap_or_else(|| ty.clone())
            }
            Type::Union(union) => Type::Union(UnionType {
                variants: union.variants.iter().map(|t| self.instantiate(t)).collect(),
                discriminant: union.discriminant.clone(),
            }),
            Type::Function(func) => Type::Function(FunctionType {
                params: func.params.iter().map(|t| self.instantiate(t)).collect(),
                return_type: Box::new(self.instantiate(&func.return_type)),
            }),
            _ => ty.clone(),
        }
    }
}
```

---

## Testing Strategy

### Unit Tests

**File:** `crates/raya-types/tests/narrowing_tests.rs`

```rust
#[test]
fn test_typeof_narrowing_bare_union() {
    let source = r#"
        type ID = string | number;

        function processID(id: ID): void {
            if (typeof id === "number") {
                // id should be narrowed to number here
                const x: number = id;
            } else {
                // id should be narrowed to string here
                const y: string = id;
            }
        }
    "#;

    let result = type_check(source);
    assert!(result.is_ok());
}

#[test]
fn test_discriminant_narrowing() {
    let source = r#"
        type Result =
            | { status: "ok"; value: number }
            | { status: "error"; error: string };

        function handle(result: Result): number {
            if (result.status === "ok") {
                return result.value;  // Should work
            } else {
                console.log(result.error);  // Should work
                return 0;
            }
        }
    "#;

    let result = type_check(source);
    assert!(result.is_ok());
}

#[test]
fn test_null_narrowing() {
    let source = r#"
        function process(x: string | null): number {
            if (x !== null) {
                return x.length;  // Should work
            }
            return 0;
        }
    "#;

    let result = type_check(source);
    assert!(result.is_ok());
}

#[test]
fn test_implicit_number_to_string_coercion() {
    // Raya allows number → string auto-cast
    let source = r#"
        function fn(x: string): void { }

        let a: string | number = 42;
        fn(a);  // OK: number auto-casts to string
    "#;

    let result = type_check(source);
    assert!(result.is_ok());
}

#[test]
fn test_no_string_to_number_coercion() {
    // string → number is NOT allowed
    let source = r#"
        function gn(x: number): void { }

        let b: string | number = "hello";
        gn(b);  // ERROR: Cannot cast string to number
    "#;

    let result = type_check(source);
    assert!(result.is_err());

    // Verify error message
    let errors = result.unwrap_err();
    assert!(errors.iter().any(|e| matches!(e.kind, TypeErrorKind::TypeMismatch { .. })));
}

#[test]
fn test_subtype_widening() {
    // Dog → Animal (subtype to supertype) is OK
    let source = r#"
        class Animal { }
        class Dog extends Animal { }

        function handle(animal: Animal): void { }

        let dog: Dog = new Dog();
        handle(dog);  // OK: Dog is subtype of Animal
    "#;

    let result = type_check(source);
    assert!(result.is_ok());
}
```

### Integration Tests

```rust
#[test]
fn test_nested_narrowing() {
    let source = r#"
        type Value = string | number | null;

        function process(val: Value): void {
            if (val !== null) {
                if (typeof val === "string") {
                    console.log(val.toUpperCase());
                } else {
                    console.log(val + 1);
                }
            }
        }
    "#;

    let result = type_check(source);
    assert!(result.is_ok());
}
```

---

## Success Criteria

- ✅ Symbol table correctly resolves all names
- ✅ Type inference works for all expression types
- ✅ **Implicit primitive coercions: number → string**
- ✅ **Subtype widening: Dog → Animal**
- ✅ **No invalid coercions: string ↛ number**
- ✅ `typeof` guards narrow bare unions (string | number)
- ✅ Discriminant guards narrow discriminated unions
- ✅ Null checks remove null from unions
- ✅ Nested narrowing works correctly
- ✅ Exhaustiveness checking detects missing cases
- ✅ 50+ type checking tests passing
- ✅ Clear error messages with source locations

---

## References

- [LANG.md §4](../design/LANG.md) - Type System
- [LANG.md §4.7](../design/LANG.md) - Discriminated Unions
- [LANG.md §6.13](../design/LANG.md) - typeof operator
- TypeScript Handbook - Type Narrowing
- Flow - Control Flow Analysis
