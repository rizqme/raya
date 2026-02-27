# raya-engine

_Verified against source on 2026-02-27._

The core Raya language engine containing the parser, compiler, and virtual machine.

## Module Structure

```
src/
├── lib.rs           # Crate entry point, re-exports, NativeHandler trait
├── parser/          # Lexer, parser, type checker
├── compiler/        # IR, optimizations, bytecode generation
├── vm/              # Interpreter, scheduler, GC, runtime
├── jit/             # JIT compilation (feature-gated: "jit")
├── aot/             # AOT native compilation (feature-gated: "aot")
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
- **scheduler/**: Unified reactor + VM/IO worker pools, task spawn optimization (lazy stacks, pooling)
- **gc/**: Garbage collector with per-task nursery allocator (64KB bump allocator)
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
- **pipeline/**: SSA lifter (stack→SSA with RPO, Phi insertion, loop support), optimization passes, pre-warming
- **backend/**: `CodegenBackend` trait, Cranelift backend (ABI, lowering), stub backend
- **runtime/**: Code cache, trampoline (JitEntryFn C-ABI)
- **profiling/**: Per-function counters, compilation policy (thresholds)
- **engine.rs**: Top-level `JitEngine` + `JitConfig`, pre-warm orchestration

### `aot/` - AOT Native Compilation (feature-gated: `#[cfg(feature = "aot")]`)
- **frame.rs**: `AotFrame`, `AotTaskContext`, `AotHelperTable` (25 entries), `AotEntryFn` C-ABI signature
- **abi.rs**: NaN-boxing constants and Cranelift IR emit helpers (box/unbox i32, f64, bool, null)
- **analysis.rs**: Suspension point analysis (`SuspensionKind`: Await, NativeCall, Preemption)
- **statemachine.rs**: State machine transform — splits at suspension points, inserts dispatch/save/restore
- **traits.rs**: `AotCompilable` trait, `compile_to_state_machine()` pipeline
- **ir_adapter.rs**: `IrFunctionAdapter` — translates IR to AOT blocks with type-aware arithmetic
- **bytecode_adapter.rs**: `LiftedFunction` — lifts bytecode to AOT form (stub: returns null)
- **lowering.rs**: Cranelift lowering from `StateMachineFunction` to native code (if-else dispatch chain)
- **codegen.rs**: `compile_functions()` — compiles each function independently via `Context::compile()`
- **helpers.rs**: 25 runtime helper implementations for `AotHelperTable` (frame, GC, values, native calls)
- **executor.rs**: `run_aot_function()` — bridges compiled functions with scheduler (`ExecutionResult`)
- **linker.rs**: Cross-module symbol resolution

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
    ├──▶ (optional, with --features jit)
    │   ┌─────────────────┐
    │   │   JIT Compile   │  → native code at runtime (Cranelift)
    │   └─────────────────┘
    │
    └──▶ (optional, with --features aot)
        ┌─────────────────┐
        │  State Machine  │  → suspension/resume transform
        └────────┬────────┘
                 │
                 ▼
        ┌─────────────────┐
        │  AOT Compile    │  → native code (Cranelift)
        └────────┬────────┘
                 │
                 ▼
        Bundle (.bundle)
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

- **Engine lib tests** (1,136): Unit tests across all modules
- **JIT tests** (147): 88 unit tests in `src/jit/` + 59 integration tests in `tests/jit_integration.rs` (requires `--features jit`)
- **AOT tests** (59+): Unit tests across `src/aot/` plus integration coverage in `tests/aot_integration.rs` (requires `--features aot`)
- **Runtime e2e tests** (2,450): In `raya-runtime/tests/e2e/` (62 test modules including bug hunting, edge cases)
- **Runtime lib tests** (30): In `raya-runtime/tests/`
- **Bundle tests** (15): In `raya-runtime/tests/bundle/`
- **CLI tests** (39): 26 integration + 13 REPL unit tests in `raya-cli/tests/`
- **Stdlib tests** (41): In `raya-stdlib/tests/`
- **Package manager tests** (204): In `raya-pm/tests/`

## Important Notes

- Runtime type checks exist at explicit cast boundaries (`as T`) and selected dynamic interop operations
- **Monomorphization**: Generics are specialized per concrete type
- **Task-based concurrency**: `async` creates Tasks, `await` suspends; optimized spawn (lazy stacks, pooling)
- **Nursery allocator**: Per-task 64KB bump allocator reduces GC lock contention
- **Typed opcodes**: `IADD` (int), `FADD` (float/number)
- **Rest parameters**: `...args` syntax fully supported
- **Optional parameters**: `param?` syntax with ordering validation
- **Builtin classes**: lowercase filenames (array.raya, string.raya), centralized TypeRegistry dispatch
- **NativeHandler trait**: Engine defines this trait for stdlib decoupling; `raya-runtime` binds implementations via `Runtime` API
- **Reflection always enabled**: No compiler flag needed, metadata always emitted
- **JIT is feature-gated**: `cargo build --features jit` pulls in Cranelift; without the flag, no JIT deps
- **JIT stability default**: adaptive on-the-fly compilation is currently disabled by default (`JitConfig::default().adaptive_compilation = false`), while static-analysis prewarm is enabled with a conservative cap (`max_prewarm_functions = 4`).
- **JIT exit ABI (phase 4 metadata bridge)**: `JitEntryFn` receives `JitExitInfo*` out-parameter (kind/suspend/deopt/frame snapshot). `CallNative` and `CheckPreemption` now carry/lower bytecode offsets into `JitExitInfo.bytecode_offset` for resume/deopt mapping; `CallNative` exits `Suspended` (native-call boundary handoff to interpreter loop), `CheckPreemption` exits `Suspended` when helper reports preemption, and `GcSafepoint`/`CheckPreemption` run via runtime helper table from interpreter JIT dispatch. Full native continuation resume back into machine code is still staged work.
- **JIT loop/control-flow hardening**: lifter now preserves `JmpIfFalse` semantics (edge inversion for `brif`), integer arithmetic/comparisons coerce `Value` locals via `UnboxI32`, and `StoreLocal` re-boxes typed regs (`I32/F64/Bool`) before writing local slots. Added regression coverage for compiler-style i16 branch loops.
- **JIT fallback stack materialization (phase 1)**: interpreter fallback now restores JIT-mutated non-arg locals (`locals_buf`) before resuming at a native-call suspension offset. Resume guard expanded beyond entry-only but remains conservative: only native boundaries with statically-empty operand stack prefixes are eligible.
- **JIT fallback stack materialization (phase 2)**: `JitExitInfo` now carries a bounded native-call operand snapshot (`native_arg_count` + `native_args[8]`) at `NativeCallBoundary`. Interpreter fallback validates expected arg count from bytecode and re-pushes materialized operands before resuming.
- **JIT fallback stack materialization (phase 2.1)**: native-boundary resume operand extraction is now centralized via `materialize_native_resume_operands()` (count/limit validation + value reconstruction), with dedicated unit tests for match/mismatch behavior.
- **JIT resume telemetry**: interpreter now tracks resume outcomes (`resume_native_ok/reject`, `resume_preemption_ok/reject`) in `JitTelemetry`/`JitTelemetrySnapshot` for visibility into fallback-resume safety decisions.
- **JIT/AOT native bridge parity (helpers)**: JIT runtime now carries a `JitRuntimeBridgeContext` (safepoint/task + VM refs) in `RuntimeContext.shared_state`; `helper_native_call_dispatch` attempts real resolved-native dispatch (`EngineContext` conversion, IO submit on suspend) with conservative null fallback, and `helper_safepoint_poll` now dereferences through the bridge context.
- **JIT native-call fast-path parity (phase 4)**: `JitInstr::CallNative` with `arg_count == 0` now has a helper-backed fast path when `RuntimeContext` is present: it calls `RuntimeHelperTable.native_call_dispatch`, continues on normal values, and suspends only when helper returns `JIT_NATIVE_SUSPEND_SENTINEL`. Arg-carrying native calls still materialize operands and exit through `NativeCallBoundary`. Integration tests cover both zero-arg value return and zero-arg sentinel suspend behavior.
- **JIT native-call fast-path parity (phase 4.1)**: arg-carrying `JitInstr::CallNative` now mirrors zero-arg behavior when runtime context is present: lowering marshals boxed args into a temporary contiguous buffer, calls `native_call_dispatch`, continues on normal values, and only exits `NativeCallBoundary` on sentinel (or null ctx fallback). Added integration coverage for arg fast-return and arg sentinel-suspend, while preserving operand materialization in `JitExitInfo` on suspend.
- **JIT fallback stack materialization (phase 4)**: preemption resume now supports a conservative boundary (`Jmp` with statically-empty stack prefix), restoring JIT-mutated locals and resuming interpreter at bytecode offset when safe.
- **Interpreter fallback refinement (phase 4.5)**: on JIT suspended exits at a zero-arg native boundary (`suspend_reason=NativeCallBoundary`), interpreter fallback may resume from `bytecode_offset` only under a strict guard (currently function-entry only). This avoids unsound mid-function resumes until full operand-stack/frame materialization exists.
- **Typed suspend reasons**: JIT exit metadata now uses `JitSuspendReason` (`None`, `Preemption`, `NativeCallBoundary`) to avoid magic-number handling across lowering/interpreter/tests.
- **JIT benchmark coverage expanded**: `examples/jit_bench.rs` now includes a complex mixed workload benchmark (nested loops + branch + modulo + local traffic) and JIT-vs-interpreter speedup reporting alongside existing branch-loop and pipeline benchmarks.
- **AOT is feature-gated**: `cargo build --features aot` enables ahead-of-time native compilation; shares Cranelift deps with JIT
- **AOT/JIT suspend parity (native boundaries)**: AOT now carries an explicit `SuspendReason::NativeCallBoundary` (value 9). State-machine lowering maps `SuspensionKind::NativeCall` to this reason, and executor conversion routes it through immediate scheduler re-entry (`Sleep { wake_at: now }`) so native dispatch still round-trips through the VM thread loop before resuming compiled code.
- **AOT helper parity step (thread-loop handoff)**: default AOT helpers now implement boundary semantics for suspension flow: `helper_native_call` marks `NativeCallBoundary` + returns `AOT_SUSPEND`, `helper_is_native_suspend` checks the sentinel, and `helper_check_preemption` reads `ctx.preempt_requested`. This keeps suspend/control behavior aligned with JIT even before full native dispatch wiring.
- **AOT suspend-reason hardening**: state-machine lowering now derives suspend reason encodings from `SuspendReason` enum casts (instead of raw literals), and executor coverage includes an end-to-end `NativeCallBoundary` suspend case (`run_aot_function` → `Suspended(Sleep)` handoff).
- **AOT suspension transform correctness**: `split_block_at_suspensions` now treats may-suspend points conditionally (`AotCall`/`NativeCall` via `IsSuspend`, synthetic `PreemptionCheck` via `HelperCall::CheckPreemption` branch) instead of always-suspending, and correctly handles end-of-block synthetic suspension indices without slicing past block length.
- **AOT transform regression coverage**: state-machine tests now assert `NativeCall` suspension sites generate conditional save/continue branches and that malformed may-suspend analysis input degrades conservatively to a save jump (no panic).
- **AOT native suspend check path**: state-machine lowering now distinguishes suspend detection by call kind — `AotCall` uses `IsSuspend` (AOT sentinel), while `NativeCall` uses `HelperCall::IsNativeSuspend`, preparing a clean fast-path/suspend split for runtime-specific native tokens.
- **AOT native helper fast/suspend split (stub phase)**: default `helper_native_call` now models both paths — immediate completion for normal IDs (fast path) and boundary suspension only for `STUB_NATIVE_SUSPEND_ID` (`u16::MAX`) with `NativeCallBoundary` + `AOT_SUSPEND`. This keeps transform/executor logic exercised for both branches before full runtime native dispatch wiring.
- **AOT executor parity coverage**: executor tests now include helper-driven native fast/suspend end-to-end paths (`run_aot_function` completes on fast path, preserves frame + suspends on boundary path), validating behavior beyond direct synthetic suspend-reason injection.
- **AOT helper native dispatch bridge (partial wiring)**: `helper_native_call` now attempts real ModuleNative-style dispatch when `AotTaskContext.shared_state` is populated (resolved natives + `EngineContext` conversion + IO submit on suspend -> `SuspendReason::IoWait` + `AOT_SUSPEND`). It retains stub fallback behavior for null/partial contexts and tests.
- **AOT native-call argument marshalling parity**: `HelperCall::NativeCall` lowering now marshals adapter args as `[native_id_reg, arg0..argN]` into ABI form `(ctx, native_id:i16, args_ptr, argc:u8)` via an explicit stack buffer, boxing typed primitives to NaN-boxed `u64` as needed before helper dispatch.
- **AOT lowering regression coverage (native arg marshalling)**: `aot::lowering::tests::test_lower_native_call_helper_marshals_args_via_stack_buffer` guards the `HelperCall::NativeCall` path to ensure lowered IR includes stack-addressed argument buffer materialization for arg-carrying native calls.
- **AOT executor e2e integration coverage**: `tests/aot_integration.rs` validates public AOT execution flow end-to-end (completion, native arg fast-path, native-boundary suspend handoff, suspend/resume roundtrip) via `allocate_initial_frame` + `run_aot_function`.
- **JIT native boundary snapshot bounds**: `jit_native_call_materializes_operands_truncated_to_exit_cap` verifies `JitExitInfo.native_args` materialization is capped to `JIT_EXIT_MAX_NATIVE_ARGS` for oversized native call argument lists.
- **AOT helper integration tests (shared state path)**: helper tests now cover shared-state dispatch end-to-end for both immediate native value returns and suspend submissions (including `IoSubmission` emission + `SuspendReason::IoWait`), so runtime-integrated and stub fallback paths are both exercised.

See submodule CLAUDE.md files for detailed guidance on each component.
