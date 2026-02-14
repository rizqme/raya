# Milestone 4.10: Opcode Handler Unification & Modular Breakdown

**Status**: Planning
**Design Doc**: [design/OPCODE_UNIFICATION.md](../design/OPCODE_UNIFICATION.md)

---

## Overview

Refactor the VM interpreter to eliminate opcode duplication and break down the monolithic 11,000-line `core.rs` into maintainable modules.

### Current Problems

1. **Opcode Duplication**: ~1,700 lines of duplicated logic
   - `execute_opcode` (122 opcodes, async execution)
   - `execute_nested_function` (72 opcodes, sync execution)

2. **Monolithic Files**:
   - `core.rs`: 11,021 lines (unmaintainable)
   - Single function with 122+ opcode cases

### Solution

1. **ExecutionContext Abstraction**: Unify async/sync execution via trait
2. **Modular Handlers**: Break opcodes into logical categories (~150-300 lines each)

### Expected Impact

- **Code reduction**: -30% opcode logic, -95% core.rs size
- **Maintainability**: Each module digestible, easy to locate code
- **Type safety**: Compile-time enforcement of async/sync semantics
- **Zero functional changes**: All 1,736 tests must pass

---

## Phase 1: Introduce Abstractions (Foundation)

**Goal**: Establish ExecutionContext trait and ControlFlow without changing behavior

### Tasks

- [x] Design ExecutionContext trait (design/OPCODE_UNIFICATION.md created)
- [ ] Create `vm/interpreter/context.rs`
  - [ ] Define `ExecutionContext` trait
    - `fn stack_mut(&mut self) -> &mut Stack`
    - `fn stack(&self) -> &Stack`
    - `fn can_suspend(&self) -> bool`
    - `fn request_suspend(&mut self, SuspendReason) -> Result<ControlFlow, VmError>`
    - `fn handle_call(...) -> Result<ControlFlow, VmError>`
    - `fn handle_return(Value) -> Result<ControlFlow, VmError>`
  - [ ] Implement `AsyncContext` struct
  - [ ] Implement `SyncContext` struct
- [ ] Create `vm/interpreter/execution.rs` (if not exists, or extend existing)
  - [ ] Define `ControlFlow` enum
    - `Continue`
    - `Suspend(SuspendReason)`
    - `Return(Value)`
    - `Jump(usize)`
    - `Exception(Value)`
  - [ ] Keep existing `ExecutionResult` enum
- [ ] Create `vm/interpreter/opcodes/` directory
  - [ ] Create `opcodes/mod.rs` with empty `dispatch_opcode` stub
- [ ] Update `vm/interpreter/mod.rs` to export new types
- [ ] Compile check (no functional changes yet)

**Deliverable**: New abstractions compile, existing code unchanged, all tests pass

**Tests**: 1,736 (no new tests, verification only)

---

## Phase 2: Extract Stack & Constant Handlers

**Goal**: Extract simplest opcodes to validate the pattern

### Tasks

- [ ] Create `vm/interpreter/opcodes/stack.rs`
  - [ ] `handle_nop() -> Result<ControlFlow, VmError>`
  - [ ] `handle_pop<C: ExecutionContext>(ctx) -> Result<ControlFlow, VmError>`
  - [ ] `handle_dup<C: ExecutionContext>(ctx) -> Result<ControlFlow, VmError>`
  - [ ] `handle_swap<C: ExecutionContext>(ctx) -> Result<ControlFlow, VmError>`
  - [ ] Add unit tests for each handler (mock context)
- [ ] Create `vm/interpreter/opcodes/constants.rs`
  - [ ] `handle_const_null<C: ExecutionContext>(ctx) -> Result<ControlFlow, VmError>`
  - [ ] `handle_const_true<C: ExecutionContext>(ctx) -> Result<ControlFlow, VmError>`
  - [ ] `handle_const_false<C: ExecutionContext>(ctx) -> Result<ControlFlow, VmError>`
  - [ ] `handle_const_i32<C: ExecutionContext>(ctx, code, ip) -> Result<ControlFlow, VmError>`
  - [ ] `handle_const_f64<C: ExecutionContext>(ctx, code, ip) -> Result<ControlFlow, VmError>`
  - [ ] `handle_const_str<C: ExecutionContext>(ctx, code, ip, module, interpreter) -> Result<ControlFlow, VmError>`
  - [ ] Add unit tests
- [ ] Update `opcodes/mod.rs`
  - [ ] Add `mod stack;` and `mod constants;`
  - [ ] Implement `dispatch_opcode` for these opcodes
- [ ] Update `execute` and `execute_nested_function`
  - [ ] Call `dispatch_opcode` for stack/constant opcodes (experimental)
  - [ ] Keep old code paths as fallback
- [ ] Test: All 1,736 tests pass with new handlers

**Deliverable**: 27 opcodes extracted and working

**Tests**: 1,736 + ~10 unit tests for handlers

---

## Phase 3: Extract Arithmetic & Logic Handlers

**Goal**: Extract computational opcodes

### Tasks

- [ ] Create `vm/interpreter/opcodes/arithmetic.rs`
  - [ ] Integer arithmetic: `handle_iadd`, `handle_isub`, `handle_imul`, `handle_idiv`, `handle_imod`
  - [ ] Float arithmetic: `handle_fadd`, `handle_fsub`, `handle_fmul`, `handle_fdiv`
  - [ ] Number arithmetic: `handle_nadd`, `handle_nsub`, `handle_nmul`, `handle_ndiv`, `handle_nmod`
  - [ ] Unary: `handle_ineg`, `handle_fneg`, `handle_nneg`
  - [ ] Bitwise: `handle_band`, `handle_bor`, `handle_bxor`, `handle_bnot`, `handle_shl`, `handle_shr`
  - [ ] ~25 handlers total
  - [ ] Unit tests for each
- [ ] Create `vm/interpreter/opcodes/comparison.rs`
  - [ ] Integer: `handle_ilt`, `handle_igt`, `handle_ile`, `handle_ige`, `handle_ieq`, `handle_ine`
  - [ ] Float: `handle_flt`, `handle_fgt`, `handle_fle`, `handle_fge`, `handle_feq`, `handle_fne`
  - [ ] Number: `handle_nlt`, `handle_ngt`, `handle_nle`, `handle_nge`, `handle_neq`, `handle_nne`
  - [ ] ~18 handlers total
  - [ ] Unit tests
- [ ] Create `vm/interpreter/opcodes/logical.rs`
  - [ ] `handle_and<C: ExecutionContext>(ctx) -> Result<ControlFlow, VmError>`
  - [ ] `handle_or<C: ExecutionContext>(ctx) -> Result<ControlFlow, VmError>`
  - [ ] `handle_not<C: ExecutionContext>(ctx) -> Result<ControlFlow, VmError>`
  - [ ] Unit tests
- [ ] Update `opcodes/mod.rs` dispatcher
- [ ] Test: All 1,736 tests pass

**Deliverable**: 46 more opcodes extracted (73 total)

**Tests**: 1,736 + ~50 unit tests

---

## Phase 4: Extract Local Variable Handlers

**Goal**: Extract local variable access opcodes

### Tasks

- [ ] Create `vm/interpreter/opcodes/locals.rs`
  - [ ] `handle_load_local<C: ExecutionContext>(ctx, code, ip, locals_base) -> Result<ControlFlow, VmError>`
  - [ ] `handle_store_local<C: ExecutionContext>(ctx, code, ip, locals_base) -> Result<ControlFlow, VmError>`
  - [ ] `handle_load_local_0` through `handle_load_local_15` (fast locals)
  - [ ] `handle_store_local_0` through `handle_store_local_3` (fast stores)
  - [ ] ~20 handlers total
  - [ ] Unit tests
- [ ] Update `opcodes/mod.rs` dispatcher
- [ ] Test: All 1,736 tests pass

**Deliverable**: 20 more opcodes extracted (93 total)

**Tests**: 1,736 + ~20 unit tests

---

## Phase 5: Extract Object & Array Handlers

**Goal**: Extract object and array operation opcodes

### Tasks

- [ ] Create `vm/interpreter/opcodes/objects.rs`
  - [ ] `handle_new_object<C: ExecutionContext>(ctx, code, ip, module, interpreter) -> Result<ControlFlow, VmError>`
  - [ ] `handle_get_field<C: ExecutionContext>(ctx, code, ip, interpreter) -> Result<ControlFlow, VmError>`
  - [ ] `handle_set_field<C: ExecutionContext>(ctx, code, ip, interpreter) -> Result<ControlFlow, VmError>`
  - [ ] `handle_get_field_checked<C: ExecutionContext>(...) -> Result<ControlFlow, VmError>`
  - [ ] `handle_set_field_checked<C: ExecutionContext>(...) -> Result<ControlFlow, VmError>`
  - [ ] ~10 handlers total
  - [ ] Unit tests
- [ ] Create `vm/interpreter/opcodes/arrays.rs`
  - [ ] `handle_new_array<C: ExecutionContext>(ctx, code, ip, module, interpreter) -> Result<ControlFlow, VmError>`
  - [ ] `handle_array_get<C: ExecutionContext>(ctx, interpreter) -> Result<ControlFlow, VmError>`
  - [ ] `handle_array_set<C: ExecutionContext>(ctx, interpreter) -> Result<ControlFlow, VmError>`
  - [ ] `handle_array_len<C: ExecutionContext>(ctx, interpreter) -> Result<ControlFlow, VmError>`
  - [ ] ~8 handlers total
  - [ ] Unit tests
- [ ] Update `opcodes/mod.rs` dispatcher
- [ ] Test: All 1,736 tests pass

**Deliverable**: 18 more opcodes extracted (111 total)

**Tests**: 1,736 + ~18 unit tests

---

## Phase 6: Extract Control Flow Handlers (Complex)

**Goal**: Extract control flow opcodes with context-aware behavior

### Tasks

- [ ] Create `vm/interpreter/opcodes/control_flow.rs`
  - [ ] `handle_jump(code, ip) -> Result<ControlFlow, VmError>`
  - [ ] `handle_jump_if<C: ExecutionContext>(ctx, code, ip) -> Result<ControlFlow, VmError>`
  - [ ] `handle_jump_if_not<C: ExecutionContext>(ctx, code, ip) -> Result<ControlFlow, VmError>`
  - [ ] `handle_call<C: ExecutionContext>(ctx, task, code, ip, module, interpreter) -> Result<ControlFlow, VmError>`
    - Uses `ctx.handle_call()` for context-specific behavior
  - [ ] `handle_call_method<C: ExecutionContext>(...) -> Result<ControlFlow, VmError>`
  - [ ] `handle_return<C: ExecutionContext>(ctx) -> Result<ControlFlow, VmError>`
    - Uses `ctx.handle_return()` for context-specific behavior
  - [ ] `handle_return_void<C: ExecutionContext>(ctx) -> Result<ControlFlow, VmError>`
  - [ ] `handle_throw<C: ExecutionContext>(ctx) -> Result<ControlFlow, VmError>`
  - [ ] ~15 handlers total
  - [ ] Unit tests (mock context for call/return behavior)
- [ ] Implement `handle_call` and `handle_return` in contexts
  - [ ] `AsyncContext::handle_call` - push frame, continue execution
  - [ ] `SyncContext::handle_call` - execute recursively
  - [ ] `AsyncContext::handle_return` - return to scheduler
  - [ ] `SyncContext::handle_return` - return value directly
- [ ] Update `opcodes/mod.rs` dispatcher
- [ ] Test: All 1,736 tests pass

**Deliverable**: 15 more opcodes extracted (126 total)

**Tests**: 1,736 + ~15 unit tests

---

## Phase 7: Extract Concurrency & I/O Handlers (Context-Specific)

**Goal**: Extract async/blocking opcodes with suspension logic

### Tasks

- [ ] Create `vm/interpreter/opcodes/concurrency.rs`
  - [ ] `handle_await<C: ExecutionContext>(ctx) -> Result<ControlFlow, VmError>`
    - Calls `ctx.request_suspend(SuspendReason::AwaitTask(...))`
    - AsyncContext: returns Ok(Suspend)
    - SyncContext: returns Err
  - [ ] `handle_yield<C: ExecutionContext>(ctx) -> Result<ControlFlow, VmError>`
  - [ ] `handle_spawn<C: ExecutionContext>(ctx, code, ip, module, interpreter) -> Result<ControlFlow, VmError>`
  - [ ] `handle_sleep<C: ExecutionContext>(ctx) -> Result<ControlFlow, VmError>`
  - [ ] ~6 handlers total
  - [ ] Unit tests (verify sync context rejects suspension)
- [ ] Create `vm/interpreter/opcodes/io.rs`
  - [ ] `handle_mutex_lock<C: ExecutionContext>(ctx, interpreter) -> Result<ControlFlow, VmError>`
    - Try lock first, suspend if needed (async only)
  - [ ] `handle_mutex_unlock<C: ExecutionContext>(ctx, interpreter) -> Result<ControlFlow, VmError>`
  - [ ] `handle_channel_send<C: ExecutionContext>(ctx, interpreter) -> Result<ControlFlow, VmError>`
  - [ ] `handle_channel_receive<C: ExecutionContext>(ctx, interpreter) -> Result<ControlFlow, VmError>`
  - [ ] ~8 handlers total
  - [ ] Unit tests
- [ ] Update `opcodes/mod.rs` dispatcher
- [ ] Test: All 1,736 tests pass

**Deliverable**: 14 more opcodes extracted (140 total, all opcodes covered)

**Tests**: 1,736 + ~14 unit tests

---

## Phase 8: Replace Old Implementations

**Goal**: Remove duplicate opcode handling, use unified dispatcher exclusively

### Tasks

- [ ] Refactor `execute` method in `core.rs`
  - [ ] Create `AsyncContext` for task execution
  - [ ] Replace `execute_opcode` calls with `opcodes::dispatch_opcode`
  - [ ] Delete old `execute_opcode` method (~2,500 lines)
  - [ ] Simplify main loop to ~50 lines
- [ ] Refactor `execute_nested_function` in `core.rs`
  - [ ] Create `SyncContext` with local stack
  - [ ] Replace nested opcode loop with `opcodes::dispatch_opcode`
  - [ ] Delete old nested opcode handling (~1,600 lines)
  - [ ] Simplify to ~50 lines
- [ ] Verify no references to old methods
  - [ ] Search for `execute_opcode` (should only be in tests)
  - [ ] Remove any helper methods only used by old code
- [ ] Test: All 1,736 tests pass
- [ ] Measure code reduction
  - [ ] `core.rs` before: 11,021 lines
  - [ ] `core.rs` after: ~500 lines
  - [ ] Deleted: ~4,100 lines of duplicate opcode logic

**Deliverable**: Old implementations deleted, unified dispatcher in use

**Tests**: 1,736 (no regressions)

---

## Phase 9: Cleanup & Optimization

**Goal**: Polish, document, and optimize the refactored code

### Tasks

- [ ] Performance optimization
  - [ ] Add `#[inline]` to hot path handlers (stack, arithmetic, locals)
  - [ ] Profile execution to identify bottlenecks
  - [ ] Benchmark: ensure < 5% regression (ideally 0%)
- [ ] Code cleanup
  - [ ] Remove any remaining duplicate helper methods
  - [ ] Ensure consistent error messages
  - [ ] Run `cargo clippy` and fix warnings
  - [ ] Run `cargo fmt` on all changed files
- [ ] Documentation
  - [ ] Add module-level docs to each `opcodes/*.rs` file
  - [ ] Document `ExecutionContext` trait with examples
  - [ ] Document `ControlFlow` enum variants
  - [ ] Update `vm/interpreter/CLAUDE.md` with new structure
  - [ ] Update root `CLAUDE.md` with phase completion
- [ ] Verification
  - [ ] Run full test suite: `cargo test --workspace`
  - [ ] Run with sanitizers (if applicable)
  - [ ] Test coverage report (should maintain or improve)

**Deliverable**: Clean, optimized, documented codebase

**Tests**: 1,736 + ~120 unit tests

---

## Success Criteria

### Correctness
- ✅ All 1,736 existing tests passing
- ✅ No behavioral regressions
- ✅ ~120 new unit tests for opcode handlers

### Performance
- ✅ Benchmark shows < 5% regression (ideally 0%)
- ✅ No performance degradation in interpreter hot paths

### Code Quality
- ✅ `core.rs` reduced from 11,021 lines to ~500 lines (**-95%**)
- ✅ Total opcode logic reduced by ~1,250 lines (**-30%**)
- ✅ No file in `opcodes/` directory > 500 lines
- ✅ Single implementation for each opcode (no duplication)
- ✅ Zero async/sync path duplication

### Architecture
- ✅ Clean `ExecutionContext` trait abstraction
- ✅ Modular opcode handlers by category
- ✅ All opcodes use unified dispatcher
- ✅ Compile-time enforcement of async/sync semantics

---

## File Structure (After Completion)

```
vm/interpreter/
├── mod.rs                    # Exports (updated)
├── core.rs                   # Main execution loops (~500 lines, was 11,021)
├── context.rs                # ExecutionContext trait + impls (~300 lines)
├── execution.rs              # ControlFlow enum (merged with existing)
├── opcodes/                  # Opcode handler modules (NEW)
│   ├── mod.rs                # Dispatcher (~200 lines)
│   ├── stack.rs              # Stack ops (~150 lines, 15 opcodes)
│   ├── constants.rs          # Constants (~180 lines, 12 opcodes)
│   ├── locals.rs             # Locals (~220 lines, 20 opcodes)
│   ├── arithmetic.rs         # Arithmetic (~250 lines, 25 opcodes)
│   ├── comparison.rs         # Comparisons (~200 lines, 18 opcodes)
│   ├── logical.rs            # Logic (~80 lines, 3 opcodes)
│   ├── control_flow.rs       # Control flow (~400 lines, 15 opcodes)
│   ├── objects.rs            # Objects (~300 lines, 10 opcodes)
│   ├── arrays.rs             # Arrays (~250 lines, 8 opcodes)
│   ├── concurrency.rs        # Async/sync (~200 lines, 6 opcodes)
│   └── io.rs                 # I/O (~220 lines, 8 opcodes)
├── capabilities.rs           # Unchanged
├── class_registry.rs         # Unchanged
├── lifecycle.rs              # Unchanged
├── marshal.rs                # Unchanged
├── module_registry.rs        # Unchanged
├── native_module_registry.rs # Unchanged
├── safepoint.rs              # Unchanged
├── shared_state.rs           # Unchanged
└── vm_facade.rs              # Unchanged
```

**Total opcodes coverage**: 140 opcodes across 11 category modules

---

## Notes

- Each phase must pass all tests before proceeding to next
- Can rollback any phase independently if issues arise
- Priority: correctness > performance > code size
- ExecutionContext trait enables future extensions (e.g., debugging context)
- Modular structure enables parallel development across team

---

## References

- Design doc: [design/OPCODE_UNIFICATION.md](../design/OPCODE_UNIFICATION.md)
- Current interpreter: [crates/raya-engine/src/vm/interpreter/core.rs](../crates/raya-engine/src/vm/interpreter/core.rs)
- Opcode definitions: [crates/raya-engine/src/compiler/bytecode/opcodes.rs](../crates/raya-engine/src/compiler/bytecode/opcodes.rs)
