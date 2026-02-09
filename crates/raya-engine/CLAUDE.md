# raya-engine

The core Raya language engine containing the parser, compiler, and virtual machine.

## Module Structure

```
src/
├── lib.rs           # Crate entry point, re-exports, NativeHandler trait
├── parser/          # Lexer, parser, type checker
├── compiler/        # IR, optimizations, bytecode generation
├── vm/              # Interpreter, scheduler, GC, runtime
└── builtins/        # Precompiled builtin type signatures
```

## Key Modules

### `parser/` - Frontend
- **lexer.rs**: Tokenization (hand-written, not generated)
- **parser/**: Recursive descent parser → AST
- **ast/**: Complete AST node definitions
- **checker/**: Type checking, inference, narrowing
- **types/**: Type system representation, assignability

### `compiler/` - Middle-end & Backend
- **lower/**: AST → IR lowering
- **ir/**: Three-address code intermediate representation
- **monomorphize/**: Generic specialization
- **optimize/**: Constant folding, DCE, inlining
- **codegen/**: IR → Bytecode generation
- **bytecode/**: Opcode definitions, module format
- **module/**: Multi-module compilation, std: module registry

### `vm/` - Runtime
- **vm/**: Core interpreter, context management
- **scheduler/**: Work-stealing task scheduler
- **gc/**: Garbage collector (currently placeholder)
- **stack.rs**: Call frames, operand stack
- **object.rs**: Object model (Class, Array, String)
- **sync/**: Mutex, synchronization primitives
- **snapshot/**: VM state serialization
- **ffi/**: Native module interface
- **reflect/**: Reflection API runtime (metadata, introspection, proxies, dynamic code) - see `vm/reflect/CLAUDE.md`
- **module/**: Module loading and linking
- **json/**: JSON parsing and serialization

## Compilation Pipeline

```
Source (.raya)
    │
    ▼
┌─────────────────┐
│     Lexer       │  → tokens
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│     Parser      │  → AST
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Type Checker   │  → typed AST + errors
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│    Lowering     │  → IR (three-address code)
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Monomorphization│  → specialized IR
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Optimizations  │  → optimized IR
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│    Codegen      │  → bytecode
└────────┬────────┘
         │
         ▼
Binary (.ryb)
```

## Key Types

```rust
// Parser
Parser::parse() -> (ast::Module, Interner)

// Type Checker
TypeChecker::check(&Module) -> CheckResult

// Compiler
Compiler::compile_via_ir(&ast::Module) -> CompileResult<Module>

// VM
Vm::execute(&Module) -> VmResult<Value>
```

## Common Tasks

### Adding a New Opcode
1. Add to `compiler/bytecode/opcode.rs`
2. Add encoding in `compiler/codegen/emit.rs`
3. Add execution in `vm/vm/interpreter.rs`
4. Update `design/OPCODE.md`

### Adding a Builtin Method
1. Add signature to `builtins/*.raya`
2. Add native ID in `compiler/native_id.rs`
3. Add lowering in `compiler/lower/expr.rs`
4. Add execution in `vm/vm/interpreter.rs` (NativeCall dispatch)

### Adding a New AST Node
1. Define in `parser/ast/` (statement.rs or expression.rs)
2. Parse in `parser/parser/`
3. Type check in `parser/checker/`
4. Lower in `compiler/lower/`

### Adding a Reflect API Method
1. Add native ID in `vm/builtin.rs` (0x0Dxx range)
2. Add handler in `vm/vm/handlers/reflect.rs`
3. Add type declaration in `raya-stdlib/reflect.d.raya`
4. Update milestone-3.8.md

### Adding a New Stdlib Module
1. Create `.raya` source in `crates/raya-stdlib/` (e.g., `Math.raya`)
2. Define native IDs in `vm/builtin.rs`
3. Add to std registry in `compiler/module/std_modules.rs`
4. Implement Rust functions in `crates/raya-stdlib/src/`
5. Route in `StdNativeHandler` in `raya-runtime/src/lib.rs`

## Test Files

- **Engine unit tests** (827): Colocated `#[cfg(test)]` blocks in each module
- **Engine integration tests** (897): `tests/*.rs` files (codegen, IR, concurrency, etc.)
- **E2E tests** (594): Moved to `raya-runtime/tests/` in M4.2 (require `StdNativeHandler`)
- `tests/reflect_phase8_tests.rs`: Reflect API integration tests
- `tests/opcode_tests.rs`: Individual opcode tests

## Important Notes

- **No runtime type checks**: All types verified at compile time
- **Monomorphization**: Generics are specialized per concrete type
- **Task-based concurrency**: `async` creates Tasks, `await` suspends
- **Typed opcodes**: `IADD` (int), `FADD` (float), `NADD` (number)
- **NativeHandler trait**: Engine defines this trait for stdlib decoupling; `raya-runtime` binds implementations
- **Reflection always enabled**: No compiler flag needed, metadata always emitted

See submodule CLAUDE.md files for detailed guidance on each component.
