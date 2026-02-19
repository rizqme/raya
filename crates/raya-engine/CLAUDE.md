# raya-engine

The core Raya language engine containing the parser, compiler, and virtual machine.

## Module Structure

```
src/
├── lib.rs           # Crate entry point, re-exports, NativeHandler trait
├── parser/          # Lexer, parser, type checker
├── compiler/        # IR, optimizations, bytecode generation
├── vm/              # Interpreter, scheduler, GC, runtime
├── jit/             # JIT compilation (feature-gated: "jit")
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
- **interpreter/**: Single-executor bytecode interpreter (opcodes/ + handlers/ modules)
- **scheduler/**: Work-stealing task scheduler
- **gc/**: Garbage collector
- **stack.rs**: Call frames, operand stack
- **object.rs**: Object model (Class, Array, String, Closure, Buffer, Map, Set)
- **builtin.rs**: Native ID constants (all ranges: 0x01xx-0x6000+)
- **native_handler.rs**: NativeHandler trait, NativeCallResult
- **native_registry.rs**: NativeFunctionRegistry, ResolvedNatives
- **abi.rs**: Internal ABI (NativeContext, NativeValue)
- **sync/**: Mutex, synchronization primitives
- **snapshot/**: VM state serialization
- **ffi/**: Native module interface
- **reflect/**: Reflection API runtime (metadata, introspection, proxies, dynamic code) - see `vm/reflect/CLAUDE.md`
- **module/**: Module loading and linking
- **json/**: JSON parsing and serialization

### `jit/` - JIT Compilation (feature-gated: `#[cfg(feature = "jit")]`)
- **analysis/**: Bytecode decoder, control-flow graph, heuristics-based candidate selection
- **ir/**: Backend-agnostic SSA-form IR (types, instructions, builder, display)
- **pipeline/**: SSA lifter (stack→SSA), optimization passes, pre-warming
- **backend/**: `CodegenBackend` trait, Cranelift backend (ABI, lowering), stub backend
- **runtime/**: Code cache, trampoline (JitEntryFn C-ABI)
- **profiling/**: Per-function counters, compilation policy (thresholds)
- **engine.rs**: Top-level `JitEngine` + `JitConfig`, pre-warm orchestration

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
    │
    ▼ (optional, with --features jit)
┌─────────────────┐
│   JIT Compile   │  → native code (Cranelift)
└─────────────────┘
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
3. Add handler in appropriate `vm/interpreter/opcodes/*.rs` module
4. Update `docs/compiler/opcode.md`

### Adding a Builtin Method
1. Add signature to `builtins/*.raya`
2. Add native ID in `compiler/native_id.rs`
3. Add lowering in `compiler/lower/expr.rs`
4. Add handler in `vm/interpreter/opcodes/native.rs` or `vm/interpreter/handlers/`

### Adding a New AST Node
1. Define in `parser/ast/` (statement.rs or expression.rs)
2. Parse in `parser/parser/`
3. Type check in `parser/checker/`
4. Lower in `compiler/lower/`

### Adding a Reflect API Method
1. Add native ID in `vm/builtin.rs` (0x0Dxx range)
2. Add handler in `vm/interpreter/handlers/reflect.rs`
3. Add type declaration in `raya-stdlib/raya/reflect.d.raya`

### Adding a New Stdlib Module
1. Create `.raya` + `.d.raya` in `crates/raya-stdlib/raya/`
2. Define native IDs in `vm/builtin.rs`
3. Add to std registry in `compiler/module/std_modules.rs`
4. Implement Rust functions in `crates/raya-stdlib/src/`
5. Route in `StdNativeHandler` in `raya-stdlib/src/handler.rs`

## Test Files

- **Engine unit tests** (835): Colocated `#[cfg(test)]` blocks in each module
- **Engine integration tests** (920+): `tests/*.rs` files (codegen, IR, concurrency, etc.)
- **JIT tests** (142): 83 unit tests in `src/jit/` + 59 integration tests in `tests/jit_integration.rs` (requires `--features jit`)
- **E2E tests** (883+): In `raya-runtime/tests/` (require `StdNativeHandler`)
- `tests/reflect_phase8_tests.rs`: Reflect API integration tests
- `tests/opcode_tests.rs`: Individual opcode tests

## Important Notes

- **No runtime type checks**: All types verified at compile time
- **Monomorphization**: Generics are specialized per concrete type
- **Task-based concurrency**: `async` creates Tasks, `await` suspends
- **Typed opcodes**: `IADD` (int), `FADD` (float/number)
- **NativeHandler trait**: Engine defines this trait for stdlib decoupling; `raya-runtime` binds implementations
- **Reflection always enabled**: No compiler flag needed, metadata always emitted
- **JIT is feature-gated**: `cargo build --features jit` pulls in Cranelift; without the flag, no JIT deps

See submodule CLAUDE.md files for detailed guidance on each component.
