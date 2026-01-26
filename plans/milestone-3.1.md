# Milestone 3.1: IR (Intermediate Representation)

**Duration:** 2-3 weeks
**Status:** 🔴 Not Started
**Dependencies:**
- Milestone 2.3 (Parser) ✅ Complete
- Milestone 2.4 (Type System) ✅ Complete
- Milestone 2.5 (Type Checker) ✅ Complete
**Next Milestone:** 3.2 (Monomorphization)

---

## Table of Contents

1. [Overview](#overview)
2. [Goals](#goals)
3. [Non-Goals](#non-goals)
4. [Architecture](#architecture)
5. [Phase 1: IR Design & Structure](#phase-1-ir-design--structure-week-1)
6. [Phase 2: AST Lowering](#phase-2-ast-lowering-week-2)
7. [Phase 3: Basic Optimizations](#phase-3-basic-optimizations-week-3)
8. [Testing Strategy](#testing-strategy)
9. [Success Criteria](#success-criteria)

---

## Overview

Design and implement an **Intermediate Representation (IR)** layer between the type-checked AST and bytecode generation. The IR serves as:
- A simplified representation for optimization passes
- A target for AST lowering (high-level → low-level)
- An input for bytecode generation (in Milestone 3.3)

### Why IR?

**Without IR (Direct AST → Bytecode):**
- Complex AST structures complicate optimization
- Hard to maintain type information during codegen
- Difficult to implement advanced optimizations
- Each optimization pass needs to understand AST nuances

**With IR (AST → IR → Bytecode):**
- ✅ Clean separation of concerns (lowering vs optimization vs codegen)
- ✅ Single representation for all optimization passes
- ✅ Easier to reason about program transformations
- ✅ Type information explicitly tracked in IR
- ✅ Foundation for advanced optimizations (inlining, escape analysis, etc.)

### IR Design Choice

We'll use **Three-Address Code (TAC)** with Basic Blocks, which is:
- Simpler than full SSA (Static Single Assignment)
- Sufficient for our optimization needs
- Easy to convert to bytecode
- Similar to LLVM IR but lighter weight

---

## Goals

### Primary Goals

1. **IR Structure**: Define IR instruction set and data structures
2. **AST Lowering**: Convert type-checked AST to IR
3. **Type Preservation**: Maintain type information in IR
4. **Basic Blocks**: Organize instructions into basic blocks
5. **Control Flow Graph**: Build CFG for control flow analysis
6. **Basic Optimizations**: Implement constant folding and dead code elimination

### Secondary Goals

1. **IR Validation**: Verify IR correctness (type consistency, CFG validity)
2. **IR Visualization**: Pretty-print IR for debugging
3. **IR Serialization**: Save/load IR for debugging

---

## Non-Goals

1. **Full SSA Form**: Not implementing φ-nodes (future milestone)
2. **Advanced Optimizations**: No inlining, loop unrolling, etc. (Milestone 3.7)
3. **Bytecode Generation**: That's Milestone 3.3
4. **Monomorphization**: That's Milestone 3.2

---

## Architecture

### IR Components

```
IR Module
├── Functions (IrFunction)
│   ├── Parameters (Register + Type)
│   ├── Locals (Register + Type)
│   ├── Basic Blocks (BasicBlock)
│   └── CFG (Control Flow Graph)
├── Classes (IrClass)
├── Constants (IrConstant)
└── Type Table (TypeId → Type)
```

### IR Instruction Set

**Three-Address Code Instructions:**
```rust
pub enum IrInstr {
    // Assignment
    Assign { dest: Register, value: IrValue },

    // Arithmetic
    BinaryOp { dest: Register, op: BinOp, left: Register, right: Register },
    UnaryOp { dest: Register, op: UnOp, operand: Register },

    // Memory
    LoadLocal { dest: Register, index: u16 },
    StoreLocal { index: u16, value: Register },
    LoadField { dest: Register, object: Register, field: u16 },
    StoreField { object: Register, field: u16, value: Register },

    // Calls
    Call { dest: Option<Register>, func: FunctionId, args: Vec<Register> },
    CallMethod { dest: Option<Register>, object: Register, method: u16, args: Vec<Register> },

    // Control flow (terminators)
    Jump { target: BasicBlockId },
    Branch { cond: Register, then_block: BasicBlockId, else_block: BasicBlockId },
    Return { value: Option<Register> },

    // Objects
    NewObject { dest: Register, class: ClassId },
    NewArray { dest: Register, len: Register, elem_ty: TypeId },
}
```

### Basic Block Structure

```rust
pub struct BasicBlock {
    id: BasicBlockId,
    instructions: Vec<IrInstr>,
    terminator: Terminator,  // Jump, Branch, or Return
}

pub enum Terminator {
    Jump(BasicBlockId),
    Branch { cond: Register, then_block: BasicBlockId, else_block: BasicBlockId },
    Return(Option<Register>),
}
```

### Module Organization

```
crates/raya-compiler/src/
├── lib.rs                 // Public API
├── ir/
│   ├── mod.rs            // IR types
│   ├── instr.rs          // IrInstr enum
│   ├── value.rs          // IrValue, Register
│   ├── function.rs       // IrFunction
│   ├── block.rs          // BasicBlock
│   ├── module.rs         // IrModule
│   └── pretty.rs         // Pretty-printing
├── lower/
│   ├── mod.rs            // AST → IR lowering
│   ├── expr.rs           // Expression lowering
│   ├── stmt.rs           // Statement lowering
│   ├── function.rs       // Function lowering
│   └── control_flow.rs   // If/while/for lowering
├── optimize/
│   ├── mod.rs            // Optimization framework
│   ├── constant_fold.rs  // Constant folding
│   └── dce.rs            // Dead code elimination
└── error.rs              // Compilation errors

tests/
├── ir_tests.rs           // IR construction tests
├── lower_tests.rs        // AST lowering tests
└── optimize_tests.rs     // Optimization tests
```

---

## Phase 1: IR Design & Structure (Week 1)

**Duration:** 5-7 days
**Goal:** Define IR data structures and basic infrastructure

### Task 1.1: Create raya-compiler Crate

**Setup:**
```toml
# Cargo.toml
[package]
name = "raya-compiler"
version = "0.1.0"
edition = "2021"

[dependencies]
raya-bytecode = { path = "../raya-bytecode" }
raya-parser = { path = "../raya-parser" }
raya-types = { path = "../raya-types" }
raya-checker = { path = "../raya-checker" }
rustc-hash = "2.0"
thiserror = "2.0"

[dev-dependencies]
raya-parser = { path = "../raya-parser" }
```

### Task 1.2: Define IR Types

**File:** `crates/raya-compiler/src/ir/mod.rs`

```rust
pub use instr::IrInstr;
pub use value::{IrValue, Register, RegisterId};
pub use function::IrFunction;
pub use block::{BasicBlock, BasicBlockId, Terminator};
pub use module::IrModule;

pub mod instr;
pub mod value;
pub mod function;
pub mod block;
pub mod module;
pub mod pretty;
```

**File:** `crates/raya-compiler/src/ir/value.rs`

```rust
use raya_types::TypeId;

/// Virtual register identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegisterId(pub u32);

/// Register with type information
#[derive(Debug, Clone)]
pub struct Register {
    pub id: RegisterId,
    pub ty: TypeId,
}

/// IR values (right-hand side of assignments)
#[derive(Debug, Clone)]
pub enum IrValue {
    Register(Register),
    Constant(IrConstant),
}

#[derive(Debug, Clone)]
pub enum IrConstant {
    I32(i32),
    F64(f64),
    String(String),
    Boolean(bool),
    Null,
}
```

**File:** `crates/raya-compiler/src/ir/instr.rs`

```rust
use super::*;

#[derive(Debug, Clone)]
pub enum IrInstr {
    // Assignment: dest = value
    Assign {
        dest: Register,
        value: IrValue,
    },

    // Binary operation: dest = left op right
    BinaryOp {
        dest: Register,
        op: BinaryOp,
        left: Register,
        right: Register,
    },

    // Unary operation: dest = op operand
    UnaryOp {
        dest: Register,
        op: UnaryOp,
        operand: Register,
    },

    // Function call: dest = func(args)
    Call {
        dest: Option<Register>,
        func: FunctionId,
        args: Vec<Register>,
    },

    // Method call: dest = object.method(args)
    CallMethod {
        dest: Option<Register>,
        object: Register,
        method: u16,
        args: Vec<Register>,
    },

    // Load from local variable
    LoadLocal {
        dest: Register,
        index: u16,
    },

    // Store to local variable
    StoreLocal {
        index: u16,
        value: Register,
    },

    // Load object field
    LoadField {
        dest: Register,
        object: Register,
        field: u16,
    },

    // Store object field
    StoreField {
        object: Register,
        field: u16,
        value: Register,
    },

    // Create new object
    NewObject {
        dest: Register,
        class: ClassId,
    },

    // Create new array
    NewArray {
        dest: Register,
        len: Register,
        elem_ty: TypeId,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum BinaryOp {
    // Arithmetic
    Add, Sub, Mul, Div, Mod,

    // Comparison
    Equal, NotEqual, Less, LessEqual, Greater, GreaterEqual,

    // Logical
    And, Or,
}

#[derive(Debug, Clone, Copy)]
pub enum UnaryOp {
    Neg,    // Numeric negation
    Not,    // Logical not
    Typeof, // typeof operator
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FunctionId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClassId(pub u32);
```

### Task 1.3: Basic Blocks and CFG

**File:** `crates/raya-compiler/src/ir/block.rs`

```rust
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BasicBlockId(pub u32);

/// A basic block: sequence of instructions with single entry and exit
#[derive(Debug, Clone)]
pub struct BasicBlock {
    pub id: BasicBlockId,
    pub instructions: Vec<IrInstr>,
    pub terminator: Terminator,
}

/// Control flow terminator (ends a basic block)
#[derive(Debug, Clone)]
pub enum Terminator {
    /// Unconditional jump to target block
    Jump(BasicBlockId),

    /// Conditional branch based on condition register
    Branch {
        cond: Register,
        then_block: BasicBlockId,
        else_block: BasicBlockId,
    },

    /// Return from function
    Return(Option<Register>),
}

impl BasicBlock {
    pub fn new(id: BasicBlockId) -> Self {
        Self {
            id,
            instructions: Vec::new(),
            terminator: Terminator::Return(None),
        }
    }

    pub fn add_instr(&mut self, instr: IrInstr) {
        self.instructions.push(instr);
    }

    pub fn set_terminator(&mut self, term: Terminator) {
        self.terminator = term;
    }

    /// Get successor blocks
    pub fn successors(&self) -> Vec<BasicBlockId> {
        match &self.terminator {
            Terminator::Jump(target) => vec![*target],
            Terminator::Branch { then_block, else_block, .. } => {
                vec![*then_block, *else_block]
            }
            Terminator::Return(_) => vec![],
        }
    }
}
```

### Task 1.4: IR Function and Module

**File:** `crates/raya-compiler/src/ir/function.rs`

```rust
use super::*;
use rustc_hash::FxHashMap;

#[derive(Debug, Clone)]
pub struct IrFunction {
    pub name: String,
    pub params: Vec<Register>,
    pub return_ty: TypeId,
    pub locals: Vec<Register>,
    pub blocks: Vec<BasicBlock>,
    pub entry_block: BasicBlockId,
}

impl IrFunction {
    pub fn new(name: String, params: Vec<Register>, return_ty: TypeId) -> Self {
        Self {
            name,
            params,
            return_ty,
            locals: Vec::new(),
            blocks: Vec::new(),
            entry_block: BasicBlockId(0),
        }
    }

    pub fn add_block(&mut self, block: BasicBlock) -> BasicBlockId {
        let id = block.id;
        self.blocks.push(block);
        id
    }

    pub fn get_block(&self, id: BasicBlockId) -> Option<&BasicBlock> {
        self.blocks.iter().find(|b| b.id == id)
    }

    pub fn get_block_mut(&mut self, id: BasicBlockId) -> Option<&mut BasicBlock> {
        self.blocks.iter_mut().find(|b| b.id == id)
    }
}
```

**File:** `crates/raya-compiler/src/ir/module.rs`

```rust
use super::*;

#[derive(Debug, Clone)]
pub struct IrModule {
    pub name: String,
    pub functions: Vec<IrFunction>,
    pub classes: Vec<IrClass>,
}

#[derive(Debug, Clone)]
pub struct IrClass {
    pub name: String,
    pub fields: Vec<IrField>,
    pub methods: Vec<FunctionId>,
}

#[derive(Debug, Clone)]
pub struct IrField {
    pub name: String,
    pub ty: TypeId,
    pub index: u16,
}

impl IrModule {
    pub fn new(name: String) -> Self {
        Self {
            name,
            functions: Vec::new(),
            classes: Vec::new(),
        }
    }

    pub fn add_function(&mut self, func: IrFunction) -> FunctionId {
        let id = FunctionId(self.functions.len() as u32);
        self.functions.push(func);
        id
    }
}
```

### Verification (Phase 1)

**Tests:** `crates/raya-compiler/tests/ir_tests.rs`

```rust
#[test]
fn test_create_basic_block() {
    let block = BasicBlock::new(BasicBlockId(0));
    assert_eq!(block.id, BasicBlockId(0));
    assert!(block.instructions.is_empty());
}

#[test]
fn test_add_instruction_to_block() {
    let mut block = BasicBlock::new(BasicBlockId(0));
    let dest = Register { id: RegisterId(0), ty: TypeId(0) };
    block.add_instr(IrInstr::Assign {
        dest: dest.clone(),
        value: IrValue::Constant(IrConstant::I32(42)),
    });
    assert_eq!(block.instructions.len(), 1);
}

#[test]
fn test_block_successors() {
    let mut block = BasicBlock::new(BasicBlockId(0));
    block.set_terminator(Terminator::Jump(BasicBlockId(1)));
    assert_eq!(block.successors(), vec![BasicBlockId(1)]);
}
```

**Success Criteria:**
- ✅ IR types compile without errors
- ✅ Basic blocks can be created and manipulated
- ✅ Terminators correctly identify successors
- ✅ 5+ tests passing

---

## Phase 2: AST Lowering (Week 2)

**Duration:** 5-7 days
**Goal:** Convert type-checked AST to IR

### Task 2.1: Lowering Framework

**File:** `crates/raya-compiler/src/lower/mod.rs`

```rust
use crate::ir::*;
use raya_checker::SymbolTable;
use raya_parser::ast;
use raya_types::TypeContext;
use rustc_hash::FxHashMap;

pub struct Lowerer<'a> {
    type_ctx: &'a TypeContext,
    symbols: &'a SymbolTable,
    current_function: Option<IrFunction>,
    current_block: BasicBlockId,
    next_register: u32,
    next_block: u32,
    local_map: FxHashMap<String, u16>,
}

impl<'a> Lowerer<'a> {
    pub fn new(type_ctx: &'a TypeContext, symbols: &'a SymbolTable) -> Self {
        Self {
            type_ctx,
            symbols,
            current_function: None,
            current_block: BasicBlockId(0),
            next_register: 0,
            next_block: 0,
            local_map: FxHashMap::default(),
        }
    }

    pub fn lower_module(&mut self, module: &ast::Module) -> IrModule {
        let mut ir_module = IrModule::new(module.name.clone());

        for stmt in &module.statements {
            if let ast::Statement::FunctionDecl(func) = stmt {
                let ir_func = self.lower_function(func);
                ir_module.add_function(ir_func);
            }
        }

        ir_module
    }

    fn alloc_register(&mut self, ty: TypeId) -> Register {
        let id = RegisterId(self.next_register);
        self.next_register += 1;
        Register { id, ty }
    }

    fn alloc_block(&mut self) -> BasicBlockId {
        let id = BasicBlockId(self.next_block);
        self.next_block += 1;
        id
    }

    fn current_block_mut(&mut self) -> &mut BasicBlock {
        let func = self.current_function.as_mut().unwrap();
        func.get_block_mut(self.current_block).unwrap()
    }
}
```

### Task 2.2: Expression Lowering

**File:** `crates/raya-compiler/src/lower/expr.rs`

```rust
impl<'a> Lowerer<'a> {
    /// Lower an expression, returning the register holding its value
    pub fn lower_expr(&mut self, expr: &ast::Expression) -> Register {
        match expr {
            ast::Expression::IntLiteral(n) => {
                let ty = self.type_ctx.get_i32_type();
                let dest = self.alloc_register(ty);
                self.current_block_mut().add_instr(IrInstr::Assign {
                    dest: dest.clone(),
                    value: IrValue::Constant(IrConstant::I32(*n)),
                });
                dest
            }

            ast::Expression::BinaryExpression(binary) => {
                let left = self.lower_expr(&binary.left);
                let right = self.lower_expr(&binary.right);
                let ty = self.infer_binary_result_type(&binary.operator, &left, &right);
                let dest = self.alloc_register(ty);

                self.current_block_mut().add_instr(IrInstr::BinaryOp {
                    dest: dest.clone(),
                    op: self.convert_binop(&binary.operator),
                    left,
                    right,
                });
                dest
            }

            ast::Expression::Identifier(name) => {
                // Look up variable and generate LoadLocal
                let local_idx = self.local_map.get(name).copied().unwrap();
                let ty = self.get_local_type(name);
                let dest = self.alloc_register(ty);

                self.current_block_mut().add_instr(IrInstr::LoadLocal {
                    dest: dest.clone(),
                    index: local_idx,
                });
                dest
            }

            // ... other expression types
            _ => todo!("Lower other expression types"),
        }
    }

    fn convert_binop(&self, op: &ast::BinaryOperator) -> BinaryOp {
        match op {
            ast::BinaryOperator::Plus => BinaryOp::Add,
            ast::BinaryOperator::Minus => BinaryOp::Sub,
            ast::BinaryOperator::Star => BinaryOp::Mul,
            ast::BinaryOperator::Slash => BinaryOp::Div,
            ast::BinaryOperator::Equal => BinaryOp::Equal,
            ast::BinaryOperator::Less => BinaryOp::Less,
            // ... others
            _ => todo!(),
        }
    }
}
```

### Task 2.3: Statement Lowering

**File:** `crates/raya-compiler/src/lower/stmt.rs`

```rust
impl<'a> Lowerer<'a> {
    pub fn lower_stmt(&mut self, stmt: &ast::Statement) {
        match stmt {
            ast::Statement::VariableDecl(decl) => {
                if let Some(init) = &decl.initializer {
                    let value = self.lower_expr(init);
                    let local_idx = self.allocate_local(&decl.name);

                    self.current_block_mut().add_instr(IrInstr::StoreLocal {
                        index: local_idx,
                        value,
                    });
                }
            }

            ast::Statement::Return(ret) => {
                let value = ret.value.as_ref().map(|expr| self.lower_expr(expr));
                self.current_block_mut().set_terminator(Terminator::Return(value));
            }

            ast::Statement::Expression(expr) => {
                self.lower_expr(expr);
                // Result discarded
            }

            // ... other statements
            _ => todo!("Lower other statement types"),
        }
    }

    fn allocate_local(&mut self, name: &str) -> u16 {
        let idx = self.local_map.len() as u16;
        self.local_map.insert(name.to_string(), idx);
        idx
    }
}
```

### Task 2.4: Control Flow Lowering

**File:** `crates/raya-compiler/src/lower/control_flow.rs`

```rust
impl<'a> Lowerer<'a> {
    pub fn lower_if(&mut self, if_stmt: &ast::IfStatement) {
        // Allocate blocks
        let then_block = self.alloc_block();
        let else_block = if_stmt.else_branch.is_some() {
            Some(self.alloc_block())
        } else {
            None
        };
        let merge_block = self.alloc_block();

        // Lower condition
        let cond = self.lower_expr(&if_stmt.condition);

        // Branch terminator
        let else_target = else_block.unwrap_or(merge_block);
        self.current_block_mut().set_terminator(Terminator::Branch {
            cond,
            then_block,
            else_block: else_target,
        });

        // Lower then branch
        self.current_block = then_block;
        let then_bb = BasicBlock::new(then_block);
        self.current_function.as_mut().unwrap().add_block(then_bb);

        for stmt in &if_stmt.then_branch {
            self.lower_stmt(stmt);
        }
        self.current_block_mut().set_terminator(Terminator::Jump(merge_block));

        // Lower else branch if exists
        if let Some(else_stmts) = &if_stmt.else_branch {
            self.current_block = else_block.unwrap();
            let else_bb = BasicBlock::new(else_block.unwrap());
            self.current_function.as_mut().unwrap().add_block(else_bb);

            for stmt in else_stmts {
                self.lower_stmt(stmt);
            }
            self.current_block_mut().set_terminator(Terminator::Jump(merge_block));
        }

        // Continue at merge block
        self.current_block = merge_block;
        let merge_bb = BasicBlock::new(merge_block);
        self.current_function.as_mut().unwrap().add_block(merge_bb);
    }
}
```

### Verification (Phase 2)

**Tests:** `crates/raya-compiler/tests/lower_tests.rs`

```rust
#[test]
fn test_lower_integer_literal() {
    let source = "42";
    // Parse → Lower → Check IR
}

#[test]
fn test_lower_binary_expression() {
    let source = "10 + 32";
    // Should produce: BinaryOp { Add, r0, r1 }
}

#[test]
fn test_lower_variable_declaration() {
    let source = "let x = 42;";
    // Should produce: Assign + StoreLocal
}

#[test]
fn test_lower_if_statement() {
    let source = "if (x > 0) { return 1; } else { return 0; }";
    // Should produce 3 basic blocks
}
```

**Success Criteria:**
- ✅ Expressions lower to correct IR instructions
- ✅ Control flow creates proper basic blocks
- ✅ Type information preserved in registers
- ✅ 10+ tests passing

---

## Phase 3: Basic Optimizations (Week 3)

**Duration:** 5-7 days
**Goal:** Implement constant folding and dead code elimination

### Task 3.1: Constant Folding

**File:** `crates/raya-compiler/src/optimize/constant_fold.rs`

```rust
use crate::ir::*;

pub struct ConstantFolder;

impl ConstantFolder {
    pub fn fold(&self, module: &mut IrModule) {
        for func in &mut module.functions {
            self.fold_function(func);
        }
    }

    fn fold_function(&self, func: &mut IrFunction) {
        for block in &mut func.blocks {
            self.fold_block(block);
        }
    }

    fn fold_block(&self, block: &mut BasicBlock) {
        let mut new_instrs = Vec::new();

        for instr in &block.instructions {
            match instr {
                IrInstr::BinaryOp { dest, op, left, right } => {
                    // If both operands are constants, fold
                    if let (Some(lval), Some(rval)) = (self.get_constant(left), self.get_constant(right)) {
                        if let Some(result) = self.eval_binop(*op, lval, rval) {
                            new_instrs.push(IrInstr::Assign {
                                dest: dest.clone(),
                                value: IrValue::Constant(result),
                            });
                            continue;
                        }
                    }
                    new_instrs.push(instr.clone());
                }
                _ => new_instrs.push(instr.clone()),
            }
        }

        block.instructions = new_instrs;
    }

    fn eval_binop(&self, op: BinaryOp, left: IrConstant, right: IrConstant) -> Option<IrConstant> {
        match (op, left, right) {
            (BinaryOp::Add, IrConstant::I32(a), IrConstant::I32(b)) => {
                Some(IrConstant::I32(a + b))
            }
            (BinaryOp::Sub, IrConstant::I32(a), IrConstant::I32(b)) => {
                Some(IrConstant::I32(a - b))
            }
            // ... other operations
            _ => None,
        }
    }
}
```

### Task 3.2: Dead Code Elimination

**File:** `crates/raya-compiler/src/optimize/dce.rs`

```rust
use crate::ir::*;
use rustc_hash::FxHashSet;

pub struct DeadCodeEliminator;

impl DeadCodeEliminator {
    pub fn eliminate(&self, module: &mut IrModule) {
        for func in &mut module.functions {
            self.eliminate_function(func);
        }
    }

    fn eliminate_function(&self, func: &mut IrFunction) {
        // Mark all used registers
        let used = self.collect_used_registers(func);

        // Remove instructions that define unused registers
        for block in &mut func.blocks {
            block.instructions.retain(|instr| {
                match instr {
                    IrInstr::Assign { dest, .. } |
                    IrInstr::BinaryOp { dest, .. } |
                    IrInstr::UnaryOp { dest, .. } => {
                        used.contains(&dest.id)
                    }
                    _ => true, // Keep side-effecting instructions
                }
            });
        }
    }

    fn collect_used_registers(&self, func: &IrFunction) -> FxHashSet<RegisterId> {
        let mut used = FxHashSet::default();

        for block in &func.blocks {
            for instr in &block.instructions {
                self.collect_uses(instr, &mut used);
            }
            self.collect_terminator_uses(&block.terminator, &mut used);
        }

        used
    }

    fn collect_uses(&self, instr: &IrInstr, used: &mut FxHashSet<RegisterId>) {
        match instr {
            IrInstr::BinaryOp { left, right, .. } => {
                used.insert(left.id);
                used.insert(right.id);
            }
            IrInstr::UnaryOp { operand, .. } => {
                used.insert(operand.id);
            }
            // ... other instructions
            _ => {}
        }
    }
}
```

### Verification (Phase 3)

**Tests:** `crates/raya-compiler/tests/optimize_tests.rs`

```rust
#[test]
fn test_constant_folding_add() {
    // Input IR: r0 = 10 + 32
    // Output IR: r0 = 42
}

#[test]
fn test_dead_code_elimination() {
    // Input: r0 = 42; r1 = 100; return r0;
    // Output: r0 = 42; return r0; (r1 eliminated)
}
```

**Success Criteria:**
- ✅ Constant folding eliminates compile-time computations
- ✅ DCE removes unused instructions
- ✅ Optimizations preserve semantics
- ✅ 5+ tests passing

---

## Testing Strategy

### Unit Tests

**Coverage:**
- IR construction (blocks, instructions, terminators)
- AST lowering (expressions, statements, control flow)
- Optimizations (constant folding, DCE)

### Integration Tests

**End-to-end lowering:**
```rust
#[test]
fn test_lower_simple_function() {
    let source = r#"
        function add(a: number, b: number): number {
            return a + b;
        }
    "#;

    // Parse → Type Check → Lower to IR
    let ir = compile_to_ir(source);

    // Verify IR structure
    assert_eq!(ir.functions.len(), 1);
    let func = &ir.functions[0];
    assert_eq!(func.params.len(), 2);
    assert_eq!(func.blocks.len(), 1);
}
```

---

## Success Criteria

### Must Have

- [ ] IR types fully defined and documented
- [ ] Basic blocks and CFG implemented
- [ ] AST lowering for expressions and statements
- [ ] Control flow lowering (if, while, for)
- [ ] Constant folding optimization
- [ ] Dead code elimination
- [ ] 30+ comprehensive tests passing

### Should Have

- [ ] IR pretty-printing for debugging
- [ ] IR validation (type checking, CFG validity)
- [ ] Optimization framework extensible for future passes

### Nice to Have

- [ ] IR serialization/deserialization
- [ ] Graphical CFG visualization
- [ ] Performance benchmarks

---

## References

### Related Documents

- [plans/PLAN.md](PLAN.md) - Overall implementation plan
- [design/LANG.md](../design/LANG.md) - Language specification
- [design/ARCHITECTURE.md](../design/ARCHITECTURE.md) - VM architecture

### Related Milestones

- [Milestone 2.5](milestone-2.5.md) - Type Checker (provides typed AST)
- [Milestone 3.2](milestone-3.2.md) - Monomorphization (consumes IR)
- [Milestone 3.3](milestone-3.3.md) - Code Generation (consumes optimized IR)

### External References

- **LLVM IR** - Inspiration for IR design
- **Three-Address Code** - Compiler design textbooks
- **SSA Form** - For future optimizations

---

## Notes

1. **Why Three-Address Code?**
   - Simpler than SSA (no φ-nodes)
   - Each instruction has at most 3 addresses (dest, src1, src2)
   - Easy to convert to bytecode (one-to-one or one-to-few mapping)

2. **Basic Blocks**
   - Single entry point (no jumps into middle)
   - Single exit point (terminator)
   - All instructions execute sequentially

3. **CFG (Control Flow Graph)**
   - Nodes = Basic Blocks
   - Edges = Control flow (jumps, branches)
   - Used for data flow analysis and optimization

4. **Type Preservation**
   - Every register has a TypeId
   - Enables type-driven optimizations
   - Facilitates bytecode generation (typed opcodes)

---

**End of Milestone 3.1 Specification**
