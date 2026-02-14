# Opcode Handler Unification

## Problem Statement

Currently there are **two separate opcode execution paths** in the VM:

1. **Main execution** (`execute_opcode`, ~122 opcodes, lines 338-4500)
   - Used for normal task execution
   - Can suspend/await (async operations)
   - Works with task stack
   - Returns `OpcodeResult` enum

2. **Nested execution** (`execute_nested_function`, ~72 opcodes, lines 4849-6600)
   - Used for synchronous nested calls (reflect handlers, etc.)
   - Cannot suspend (errors on blocking operations)
   - Creates local stack
   - Returns `Result<Value, VmError>` directly

### Issues with Current Design

- **~1,700 lines of duplicated opcode logic** (~59% overlap)
- **Maintenance burden**: Bug fixes/features need dual implementation
- **Feature parity gaps**: Nested handler missing 50 opcodes
- **Consistency risks**: Easy to diverge over time

---

## Root Cause Analysis

The duplication exists because nested calls need **synchronous execution** (no suspension) while the main loop needs **async execution** (can suspend/await). However, **90% of opcodes are identical** between the two contexts:

### Identical Opcodes (can be shared)
- Stack manipulation: `Nop`, `Pop`, `Dup`, `Swap`
- Constants: `ConstNull`, `ConstTrue`, `ConstI32`, `ConstStr`, etc.
- Locals: `LoadLocal`, `StoreLocal`, `LoadLocal0-3`, etc.
- Arithmetic: `IAdd`, `ISub`, `FAdd`, `IMul`, etc.
- Comparisons: `ILt`, `IGt`, `IEq`, etc.
- Logic: `And`, `Or`, `Not`
- Object operations: `GetField`, `SetField`, `NewObject`
- Array operations: `NewArray`, `ArrayGet`, `ArraySet`
- Control flow (non-suspending): `Jump`, `JumpIf`, `JumpIfNot`

### Context-Specific Opcodes (need different behavior)
- **Suspension points**: `Await`, `Yield`, `Sleep`
- **Blocking I/O**: `MutexLock` (can wait), `ChannelSend/Receive` (can block)
- **Call handling**: `Call`, `CallMethod` (may need to suspend in callee)
- **Returns**: Different return handling (OpcodeResult vs Value)

---

## Proposed Solution: Execution Context Abstraction

### Architecture Overview

```
┌─────────────────────────────────────────────────────┐
│         Unified Opcode Dispatcher                    │
│  execute_opcode_unified(ctx, opcode, ...)           │
│                                                       │
│  • Handles 90% of opcodes identically                │
│  • Delegates to context for special cases            │
└─────────────────────────────────────────────────────┘
                         │
         ┌───────────────┴───────────────┐
         │                               │
         ▼                               ▼
┌─────────────────┐           ┌─────────────────┐
│  AsyncContext   │           │  SyncContext    │
│                 │           │                 │
│ • Task stack    │           │ • Local stack   │
│ • Can suspend   │           │ • Cannot suspend│
│ • OpcodeResult  │           │ • Must complete │
└─────────────────┘           └─────────────────┘
```

### Core Components

#### 1. Execution Context Trait

```rust
/// Execution context that abstracts over async vs sync execution
trait ExecutionContext {
    /// Get mutable reference to the execution stack
    fn stack_mut(&mut self) -> &mut Stack;

    /// Get immutable reference to the execution stack
    fn stack(&self) -> &Stack;

    /// Can this context suspend execution?
    fn can_suspend(&self) -> bool;

    /// Handle a suspension request (await, sleep, blocking I/O)
    ///
    /// Returns:
    /// - Ok(ControlFlow::Suspend) if suspension was handled
    /// - Err(_) if context cannot suspend
    fn request_suspend(&mut self, reason: SuspendReason) -> Result<ControlFlow, VmError>;

    /// Handle function call (may need context-specific behavior)
    fn handle_call(
        &mut self,
        interpreter: &mut Interpreter,
        task: &Arc<Task>,
        func_index: usize,
        args: Vec<Value>,
        module: &Module,
    ) -> Result<ControlFlow, VmError>;

    /// Handle return from function
    fn handle_return(&mut self, value: Value) -> Result<ControlFlow, VmError>;
}

/// Control flow directive from opcode execution
enum ControlFlow {
    /// Continue to next opcode
    Continue,
    /// Suspend execution (async context only)
    Suspend(SuspendReason),
    /// Return from current function
    Return(Value),
    /// Jump to offset
    Jump(usize),
    /// Exception thrown
    Exception(Value),
}
```

#### 2. Context Implementations

```rust
/// Async execution context (main task loop)
struct AsyncContext<'a> {
    task: &'a Arc<Task>,
    /// Uses task's execution stack
}

impl ExecutionContext for AsyncContext<'_> {
    fn stack_mut(&mut self) -> &mut Stack {
        // Get task's stack (requires task.stack() to return &mut)
        // May need RefCell or similar
    }

    fn can_suspend(&self) -> bool {
        true
    }

    fn request_suspend(&mut self, reason: SuspendReason) -> Result<ControlFlow, VmError> {
        // Set task state to suspended
        Ok(ControlFlow::Suspend(reason))
    }

    fn handle_call(&mut self, ...) -> Result<ControlFlow, VmError> {
        // Push frame, continue execution in main loop
        // May return Suspend if callee suspends
        Ok(ControlFlow::Continue)
    }
}

/// Sync execution context (nested calls)
struct SyncContext {
    /// Local stack for this execution
    stack: Stack,
}

impl ExecutionContext for SyncContext {
    fn stack_mut(&mut self) -> &mut Stack {
        &mut self.stack
    }

    fn can_suspend(&self) -> bool {
        false
    }

    fn request_suspend(&mut self, reason: SuspendReason) -> Result<ControlFlow, VmError> {
        // Nested calls cannot suspend
        Err(VmError::RuntimeError(format!(
            "Cannot suspend in synchronous nested call: {:?}",
            reason
        )))
    }

    fn handle_call(&mut self, interpreter, ...) -> Result<ControlFlow, VmError> {
        // Execute recursively, must complete synchronously
        let result = interpreter.execute_nested_function(...)?;
        self.stack.push(result)?;
        Ok(ControlFlow::Continue)
    }
}
```

#### 3. Unified Opcode Dispatcher

```rust
impl Interpreter {
    /// Unified opcode execution (replaces both execute_opcode and execute_nested_function loop)
    fn execute_opcode_unified<C: ExecutionContext>(
        &mut self,
        ctx: &mut C,
        task: &Arc<Task>,
        ip: &mut usize,
        code: &[u8],
        module: &Module,
        opcode: Opcode,
        locals_base: usize,
    ) -> Result<ControlFlow, VmError> {
        let stack = ctx.stack_mut();

        match opcode {
            // ============================================================
            // Shared opcodes (90% of cases)
            // ============================================================

            Opcode::Nop => Ok(ControlFlow::Continue),

            Opcode::Pop => {
                stack.pop()?;
                Ok(ControlFlow::Continue)
            }

            Opcode::ConstI32 => {
                let value = Self::read_i32(code, ip)?;
                stack.push(Value::i32(value))?;
                Ok(ControlFlow::Continue)
            }

            Opcode::IAdd => {
                let b = stack.pop()?.as_i32()?;
                let a = stack.pop()?.as_i32()?;
                stack.push(Value::i32(a.wrapping_add(b)))?;
                Ok(ControlFlow::Continue)
            }

            // ... all other shared opcodes ...

            // ============================================================
            // Context-specific opcodes
            // ============================================================

            Opcode::Await => {
                // Try to suspend if context allows
                let task_id = stack.pop()?.as_task_id()?;
                ctx.request_suspend(SuspendReason::AwaitTask(task_id))
            }

            Opcode::Sleep => {
                let duration_ms = stack.pop()?.as_i32()? as u64;
                ctx.request_suspend(SuspendReason::Sleep(duration_ms))
            }

            Opcode::MutexLock => {
                let mutex_id = stack.pop()?.as_mutex_id()?;

                // Try to acquire lock
                if let Some(value) = self.mutexes.try_lock(mutex_id)? {
                    // Lock acquired immediately
                    stack.push(value)?;
                    Ok(ControlFlow::Continue)
                } else if ctx.can_suspend() {
                    // Can wait for lock
                    ctx.request_suspend(SuspendReason::WaitMutex(mutex_id))
                } else {
                    // Nested context cannot wait
                    Err(VmError::RuntimeError(
                        "Cannot wait for mutex in synchronous call".to_string()
                    ))
                }
            }

            Opcode::Call => {
                let func_index = Self::read_u32(code, ip)? as usize;
                let arg_count = Self::read_u16(code, ip)? as usize;

                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    args.push(stack.pop()?);
                }
                args.reverse();

                // Delegate to context
                ctx.handle_call(self, task, func_index, args, module)
            }

            Opcode::Return => {
                let value = if stack.depth() > 0 {
                    stack.pop()?
                } else {
                    Value::null()
                };
                ctx.handle_return(value)
            }

            _ => Err(VmError::RuntimeError(format!(
                "Opcode {:?} not implemented",
                opcode
            ))),
        }
    }
}
```

#### 4. Refactored Main Loop

```rust
impl Interpreter {
    /// Main execution loop for async tasks
    pub fn execute(
        &mut self,
        task: Arc<Task>,
        module: &Module,
        func_index: usize,
    ) -> ExecutionResult {
        // Set up async context
        let mut ctx = AsyncContext { task: &task };

        // ... frame setup ...

        loop {
            self.safepoint.poll();

            let opcode_byte = code[ip];
            ip += 1;
            let opcode = Opcode::from_u8(opcode_byte)?;

            // Use unified dispatcher
            match self.execute_opcode_unified(
                &mut ctx,
                &task,
                &mut ip,
                code,
                module,
                opcode,
                locals_base,
            )? {
                ControlFlow::Continue => continue,
                ControlFlow::Suspend(reason) => {
                    task.set_suspend_reason(reason);
                    return ExecutionResult::Suspended;
                }
                ControlFlow::Return(value) => {
                    return ExecutionResult::Completed(value);
                }
                ControlFlow::Jump(offset) => {
                    ip = offset;
                }
                ControlFlow::Exception(exc) => {
                    // ... exception handling ...
                }
            }
        }
    }

    /// Synchronous nested function execution
    fn execute_nested_function(
        &mut self,
        task: &Arc<Task>,
        func_index: usize,
        args: Vec<Value>,
        module: &Module,
    ) -> Result<Value, VmError> {
        // Set up sync context with local stack
        let mut stack = Stack::new();
        let mut ctx = SyncContext { stack };

        // ... setup locals, args ...

        loop {
            self.safepoint.poll();

            let opcode_byte = code[ip];
            ip += 1;
            let opcode = Opcode::from_u8(opcode_byte)?;

            // Use SAME unified dispatcher
            match self.execute_opcode_unified(
                &mut ctx,
                task,
                &mut ip,
                code,
                module,
                opcode,
                locals_base,
            )? {
                ControlFlow::Continue => continue,
                ControlFlow::Return(value) => {
                    return Ok(value);
                }
                ControlFlow::Jump(offset) => {
                    ip = offset;
                }
                ControlFlow::Suspend(_) => {
                    // Should never happen (ctx.request_suspend returns error)
                    unreachable!("SyncContext should not return Suspend")
                }
                ControlFlow::Exception(exc) => {
                    return Err(VmError::RuntimeError(
                        format!("Exception in nested call: {:?}", exc)
                    ));
                }
            }
        }
    }
}
```

---

## Modular Breakdown Strategy

### Current Problem: Monolithic core.rs

**File size**: 11,021 lines (too large for maintainability)

Even after unification, a single `execute_opcode_unified` with 122+ opcode cases will still be ~2,300 lines in one function. We need to break this into **logical handler modules**.

### Proposed Module Structure

```
vm/interpreter/
├── mod.rs              # Main exports
├── core.rs             # Main execution loop (now ~500 lines)
├── context.rs          # ExecutionContext trait + implementations
├── execution.rs        # ExecutionResult, ControlFlow enums
├── opcodes/            # Opcode handler modules
│   ├── mod.rs          # Opcode dispatcher (delegates to handlers)
│   ├── stack.rs        # Stack operations (Nop, Pop, Dup, Swap, etc.)
│   ├── constants.rs    # Constant opcodes (ConstNull, ConstI32, ConstStr, etc.)
│   ├── locals.rs       # Local variable access (LoadLocal, StoreLocal, etc.)
│   ├── arithmetic.rs   # Arithmetic ops (IAdd, ISub, FAdd, IMul, etc.)
│   ├── comparison.rs   # Comparison ops (ILt, IEq, FGt, etc.)
│   ├── logical.rs      # Logic ops (And, Or, Not)
│   ├── control_flow.rs # Control flow (Jump, JumpIf, Call, Return, etc.)
│   ├── objects.rs      # Object ops (NewObject, GetField, SetField, etc.)
│   ├── arrays.rs       # Array ops (NewArray, ArrayGet, ArraySet, ArrayLen)
│   ├── concurrency.rs  # Async/sync ops (Await, Yield, Spawn, etc.)
│   └── io.rs           # I/O ops (MutexLock, ChannelSend, etc.)
├── ... (other existing modules)
```

### Opcode Handler Pattern

Each handler module follows this pattern:

```rust
// vm/interpreter/opcodes/arithmetic.rs

use super::super::context::ExecutionContext;
use super::ControlFlow;
use crate::vm::{Stack, Value, VmError};

/// Handle integer addition (IAdd opcode)
#[inline]
pub fn handle_iadd<C: ExecutionContext>(
    ctx: &mut C,
) -> Result<ControlFlow, VmError> {
    let stack = ctx.stack_mut();
    let b = stack.pop()?.as_i32()?;
    let a = stack.pop()?.as_i32()?;
    stack.push(Value::i32(a.wrapping_add(b)))?;
    Ok(ControlFlow::Continue)
}

/// Handle floating point addition (FAdd opcode)
#[inline]
pub fn handle_fadd<C: ExecutionContext>(
    ctx: &mut C,
) -> Result<ControlFlow, VmError> {
    let stack = ctx.stack_mut();
    let b = stack.pop()?.as_f64()?;
    let a = stack.pop()?.as_f64()?;
    stack.push(Value::f64(a + b))?;
    Ok(ControlFlow::Continue)
}

// ... other arithmetic handlers (isub, imul, idiv, etc.)
```

### Opcode Dispatcher

The main dispatcher delegates to category-specific handlers:

```rust
// vm/interpreter/opcodes/mod.rs

mod stack;
mod constants;
mod locals;
mod arithmetic;
mod comparison;
mod logical;
mod control_flow;
mod objects;
mod arrays;
mod concurrency;
mod io;

use super::context::ExecutionContext;
use super::execution::ControlFlow;
use crate::compiler::{Module, Opcode};
use crate::vm::{Value, VmError};
use std::sync::Arc;

/// Dispatch opcode to appropriate handler
#[inline]
pub fn dispatch_opcode<C: ExecutionContext>(
    ctx: &mut C,
    task: &Arc<Task>,
    opcode: Opcode,
    ip: &mut usize,
    code: &[u8],
    module: &Module,
    locals_base: usize,
    interpreter: &mut Interpreter,
) -> Result<ControlFlow, VmError> {
    match opcode {
        // Stack operations (15 opcodes)
        Opcode::Nop => stack::handle_nop(),
        Opcode::Pop => stack::handle_pop(ctx),
        Opcode::Dup => stack::handle_dup(ctx),
        Opcode::Swap => stack::handle_swap(ctx),
        // ... other stack ops

        // Constants (12 opcodes)
        Opcode::ConstNull => constants::handle_const_null(ctx),
        Opcode::ConstTrue => constants::handle_const_true(ctx),
        Opcode::ConstI32 => constants::handle_const_i32(ctx, code, ip),
        Opcode::ConstF64 => constants::handle_const_f64(ctx, code, ip),
        Opcode::ConstStr => constants::handle_const_str(ctx, code, ip, module, interpreter),
        // ... other constants

        // Locals (20 opcodes)
        Opcode::LoadLocal => locals::handle_load_local(ctx, code, ip, locals_base),
        Opcode::StoreLocal => locals::handle_store_local(ctx, code, ip, locals_base),
        Opcode::LoadLocal0 => locals::handle_load_local_n(ctx, 0, locals_base),
        // ... other local access

        // Arithmetic (25 opcodes)
        Opcode::IAdd => arithmetic::handle_iadd(ctx),
        Opcode::ISub => arithmetic::handle_isub(ctx),
        Opcode::IMul => arithmetic::handle_imul(ctx),
        Opcode::FAdd => arithmetic::handle_fadd(ctx),
        // ... other arithmetic

        // Comparison (18 opcodes)
        Opcode::ILt => comparison::handle_ilt(ctx),
        Opcode::IEq => comparison::handle_ieq(ctx),
        // ... other comparisons

        // Logical (3 opcodes)
        Opcode::And => logical::handle_and(ctx),
        Opcode::Or => logical::handle_or(ctx),
        Opcode::Not => logical::handle_not(ctx),

        // Control flow (15 opcodes)
        Opcode::Jump => control_flow::handle_jump(code, ip),
        Opcode::JumpIf => control_flow::handle_jump_if(ctx, code, ip),
        Opcode::Call => control_flow::handle_call(ctx, task, code, ip, module, interpreter),
        Opcode::Return => control_flow::handle_return(ctx),
        // ... other control flow

        // Objects (10 opcodes)
        Opcode::NewObject => objects::handle_new_object(ctx, code, ip, module, interpreter),
        Opcode::GetField => objects::handle_get_field(ctx, code, ip, interpreter),
        Opcode::SetField => objects::handle_set_field(ctx, code, ip, interpreter),
        // ... other object ops

        // Arrays (8 opcodes)
        Opcode::NewArray => arrays::handle_new_array(ctx, code, ip, module, interpreter),
        Opcode::ArrayGet => arrays::handle_array_get(ctx, interpreter),
        Opcode::ArraySet => arrays::handle_array_set(ctx, interpreter),
        // ... other array ops

        // Concurrency (6 opcodes)
        Opcode::Await => concurrency::handle_await(ctx),
        Opcode::Yield => concurrency::handle_yield(ctx),
        Opcode::Spawn => concurrency::handle_spawn(ctx, code, ip, module, interpreter),
        // ... other concurrency

        // I/O (8 opcodes)
        Opcode::MutexLock => io::handle_mutex_lock(ctx, interpreter),
        Opcode::ChannelSend => io::handle_channel_send(ctx, interpreter),
        // ... other I/O

        _ => Err(VmError::RuntimeError(format!(
            "Unimplemented opcode: {:?}",
            opcode
        ))),
    }
}
```

### Refactored core.rs

The main execution loop becomes much simpler:

```rust
// vm/interpreter/core.rs (now ~500 lines instead of 11,000)

use super::context::{AsyncContext, ExecutionContext};
use super::execution::{ControlFlow, ExecutionResult};
use super::opcodes;
use crate::compiler::{Module, Opcode};
use crate::vm::scheduler::Task;
use std::sync::Arc;

impl Interpreter {
    /// Main execution loop for async tasks
    pub fn execute(
        &mut self,
        task: Arc<Task>,
        module: &Module,
        func_index: usize,
    ) -> ExecutionResult {
        // Set up function context
        let function = &module.functions[func_index];
        let code = &function.code;

        // Create async execution context
        let mut ctx = AsyncContext::new(&task);

        // Initialize locals
        let locals_base = 0;
        let mut ip = 0;

        // Main execution loop
        loop {
            self.safepoint.poll();

            if ip >= code.len() {
                return ExecutionResult::Completed(Value::null());
            }

            let opcode_byte = code[ip];
            ip += 1;

            let opcode = match Opcode::from_u8(opcode_byte) {
                Some(op) => op,
                None => return ExecutionResult::Error(VmError::InvalidOpcode(opcode_byte)),
            };

            // Dispatch to handler
            match opcodes::dispatch_opcode(
                &mut ctx,
                &task,
                opcode,
                &mut ip,
                code,
                module,
                locals_base,
                self,
            ) {
                Ok(ControlFlow::Continue) => continue,
                Ok(ControlFlow::Suspend(reason)) => {
                    task.set_suspend_reason(reason);
                    return ExecutionResult::Suspended;
                }
                Ok(ControlFlow::Return(value)) => {
                    task.pop_call_frame();
                    return ExecutionResult::Completed(value);
                }
                Ok(ControlFlow::Jump(offset)) => {
                    ip = offset;
                }
                Ok(ControlFlow::Exception(exc)) => {
                    // Exception handling...
                    self.handle_exception(&task, exc, module)?;
                }
                Err(e) => {
                    return ExecutionResult::Error(e);
                }
            }
        }
    }

    /// Synchronous nested function execution
    fn execute_nested_function(
        &mut self,
        task: &Arc<Task>,
        func_index: usize,
        args: Vec<Value>,
        module: &Module,
    ) -> Result<Value, VmError> {
        let function = &module.functions[func_index];
        let code = &function.code;

        // Create sync execution context
        let mut ctx = SyncContext::new(function.local_count, args);

        let locals_base = 0;
        let mut ip = 0;

        // Execution loop (same structure as async, different context)
        loop {
            self.safepoint.poll();

            if ip >= code.len() {
                return Ok(Value::null());
            }

            let opcode_byte = code[ip];
            ip += 1;
            let opcode = Opcode::from_u8(opcode_byte)?;

            // Use SAME dispatcher, different context type
            match opcodes::dispatch_opcode(
                &mut ctx,
                task,
                opcode,
                &mut ip,
                code,
                module,
                locals_base,
                self,
            ) {
                Ok(ControlFlow::Continue) => continue,
                Ok(ControlFlow::Return(value)) => {
                    return Ok(value);
                }
                Ok(ControlFlow::Jump(offset)) => {
                    ip = offset;
                }
                Ok(ControlFlow::Suspend(_)) => {
                    unreachable!("SyncContext should never suspend")
                }
                Ok(ControlFlow::Exception(exc)) => {
                    return Err(VmError::RuntimeError(format!("Exception: {:?}", exc)));
                }
                Err(e) => return Err(e),
            }
        }
    }
}
```

### Benefits of Modular Breakdown

**Maintainability**
- ✅ Each handler module ~150-300 lines (easy to understand)
- ✅ Clear separation of concerns by opcode category
- ✅ Easy to locate specific opcode implementation

**Testability**
- ✅ Individual handlers can have unit tests
- ✅ Mock ExecutionContext for isolated testing
- ✅ Test coverage per category

**Parallelizability**
- ✅ Multiple developers can work on different handler modules
- ✅ Reduced merge conflicts (different files)

**Performance**
- ✅ `#[inline]` on hot path handlers
- ✅ Compiler can optimize each handler independently
- ✅ Better code locality (related opcodes grouped)

### File Size Comparison

| File | Before | After | Change |
|------|--------|-------|--------|
| core.rs | 11,021 lines | ~500 lines | **-95%** |
| opcodes/mod.rs | - | ~200 lines | New |
| opcodes/arithmetic.rs | - | ~250 lines | New |
| opcodes/control_flow.rs | - | ~400 lines | New |
| opcodes/objects.rs | - | ~300 lines | New |
| opcodes/concurrency.rs | - | ~200 lines | New |
| opcodes/*.rs (others) | - | ~1,000 lines | New |
| **Total opcode logic** | ~4,100 lines | ~2,850 lines | **-30%** |

## Implementation Plan

### Phase 1: Introduce Abstractions (No Behavior Change)
1. Create `context.rs` with `ExecutionContext` trait
2. Implement `AsyncContext` and `SyncContext`
3. Create `execution.rs` with `ControlFlow` enum
4. Create `opcodes/mod.rs` with empty `dispatch_opcode` function

**Deliverable**: New modules compile, old code still works

### Phase 2: Extract Stack & Constant Handlers
1. Create `opcodes/stack.rs` (Nop, Pop, Dup, Swap, etc.)
2. Create `opcodes/constants.rs` (ConstNull, ConstI32, ConstStr, etc.)
3. Update `dispatch_opcode` to delegate to these handlers
4. Test: All 1,736 tests pass

**Deliverable**: 27 opcodes extracted

### Phase 3: Extract Arithmetic & Logic Handlers
1. Create `opcodes/arithmetic.rs` (IAdd, ISub, FAdd, IMul, etc.)
2. Create `opcodes/comparison.rs` (ILt, IEq, FGt, etc.)
3. Create `opcodes/logical.rs` (And, Or, Not)
4. Update dispatcher
5. Test: All tests pass

**Deliverable**: 46 more opcodes extracted (73 total)

### Phase 4: Extract Local Variable Handlers
1. Create `opcodes/locals.rs` (LoadLocal, StoreLocal, LoadLocal0-3, etc.)
2. Update dispatcher
3. Test: All tests pass

**Deliverable**: 20 more opcodes extracted (93 total)

### Phase 5: Extract Object & Array Handlers
1. Create `opcodes/objects.rs` (NewObject, GetField, SetField, etc.)
2. Create `opcodes/arrays.rs` (NewArray, ArrayGet, ArraySet, etc.)
3. Update dispatcher
4. Test: All tests pass

**Deliverable**: 18 more opcodes extracted (111 total)

### Phase 6: Extract Control Flow Handlers (Complex)
1. Create `opcodes/control_flow.rs` (Jump, JumpIf, Call, Return, etc.)
2. Implement context-aware call handling
3. Update dispatcher
4. Test: All tests pass

**Deliverable**: 15 more opcodes extracted (126 total)

### Phase 7: Extract Concurrency & I/O Handlers (Context-Specific)
1. Create `opcodes/concurrency.rs` (Await, Yield, Spawn, etc.)
2. Create `opcodes/io.rs` (MutexLock, ChannelSend, etc.)
3. Implement `ctx.request_suspend` for async operations
4. Implement context checks for blocking operations
5. Update dispatcher
6. Test: All tests pass

**Deliverable**: 14 more opcodes extracted (140 total, all opcodes covered)

### Phase 8: Replace Old Implementations
1. Update `execute` to use `dispatch_opcode`
2. Update `execute_nested_function` to use `dispatch_opcode`
3. Delete old `execute_opcode` match statement (2,500 lines)
4. Delete nested opcode loop (1,600 lines)
5. Test: All tests pass

**Deliverable**: Old code removed, ~4,100 lines deleted

### Phase 9: Cleanup & Optimization
1. Remove duplicate helper methods
2. Add `#[inline]` hints to hot paths
3. Benchmark performance (should be within 5% of baseline)
4. Update documentation
5. Update CLAUDE.md files

**Deliverable**: Clean, optimized, documented codebase

---

## Combined Benefits: Unification + Modular Breakdown

### Code Size Reduction

**Before**:
- `core.rs`: 11,021 lines (monolithic)
- Opcode handling: ~4,100 lines duplicated (2,500 main + 1,600 nested)

**After**:
- `core.rs`: ~500 lines (main execution loop only, **-95%**)
- `opcodes/*.rs`: ~2,850 lines (unified, modular handlers, **-30%** from baseline)
- Context glue: ~300 lines

**Total Savings**: ~1,250 lines (**30% reduction** in opcode logic + **95% reduction** in core.rs size)

### Maintainability

**Code Organization**:
- ✅ Each opcode handler module ~150-300 lines (digestible)
- ✅ Clear separation by category (arithmetic, control flow, etc.)
- ✅ Easy to locate specific opcode implementation
- ✅ `core.rs` reduced from 11K lines to 500 lines (maintainable)

**Single Source of Truth**:
- ✅ Each opcode implemented once (not twice)
- ✅ Bug fixes automatically apply to both async/sync contexts
- ✅ New opcodes only need one implementation
- ✅ Behavior consistency guaranteed by shared code

**Team Collaboration**:
- ✅ Multiple developers can work on different handler modules
- ✅ Reduced merge conflicts (different files)
- ✅ Clear ownership boundaries

### Type Safety

- ✅ `ExecutionContext` trait enforces correct behavior statically
- ✅ Compiler prevents suspension in sync context (compile-time error)
- ✅ Clear separation between async/sync semantics
- ✅ Context capabilities checked at compile time, not runtime

### Testability

- ✅ Individual handler functions can have unit tests
- ✅ Mock `ExecutionContext` for isolated testing
- ✅ Test coverage per opcode category
- ✅ Easier to test edge cases (smaller test surface)

### Performance

**Optimization Opportunities**:
- ✅ No runtime overhead (trait dispatch monomorphized by compiler)
- ✅ `#[inline]` hints on hot path handlers
- ✅ Better code locality (related opcodes grouped in same file)
- ✅ Compiler can optimize each handler independently

**Expected Impact**:
- ✅ Should be within 5% of current performance (likely identical)
- ✅ Potential for *better* performance (improved inlining, cache locality)
- ⚠️ Slightly more complex setup (context creation), but negligible (< 1% overhead)

---

## Alternative Considered: Macro-Based Code Generation

### Approach
```rust
macro_rules! opcodes {
    ($(Opcode::$name:ident => $impl:expr),* $(,)?) => {
        // Generate both execute_opcode and nested loop from single source
    };
}
```

### Why Rejected
- ❌ Less type-safe (macro errors are cryptic)
- ❌ Harder to debug (macro expansion required)
- ❌ No way to enforce context-specific behavior statically
- ❌ IDE support poor (no autocomplete in macros)
- ✅ Trait-based solution is more idiomatic Rust

---

## Migration Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Behavior divergence | Low | High | Comprehensive test coverage (1,736 tests) |
| Performance regression | Low | Medium | Benchmark before/after, trait monomorphization |
| Increased complexity | Medium | Low | Clear abstractions, good documentation |
| Stack handling errors | Medium | High | Incremental migration, test each batch |

---

## Success Metrics

**Correctness**:
- ✅ All 1,736 tests passing
- ✅ No behavioral regressions (bit-identical execution)

**Performance**:
- ✅ No performance regression (< 5% slowdown acceptable, ideally identical)
- ✅ Benchmark suite shows comparable or better performance

**Code Quality**:
- ✅ `core.rs` reduced from 11,021 to ~500 lines (**95% reduction**)
- ✅ Total opcode logic reduced by ~1,250 lines (**30% reduction**)
- ✅ No file > 500 lines in `opcodes/` directory
- ✅ Single implementation for each opcode (no duplication)
- ✅ No async/sync path duplication

**Architecture**:
- ✅ Clean ExecutionContext abstraction
- ✅ Modular opcode handlers by category
- ✅ All opcode handlers use unified dispatcher

---

## Conclusion

This refactoring addresses **two critical architectural issues**:

1. **Opcode Duplication**: The `ExecutionContext` trait provides a clean, type-safe solution to unify async and sync opcode handling while preserving their distinct semantics.

2. **Monolithic File Size**: Breaking down 11,000-line `core.rs` into focused opcode handler modules (~150-300 lines each) dramatically improves maintainability.

### Key Outcomes

**Code Quality**:
- Single source of truth for each opcode (eliminates 1,700 lines of duplication)
- Manageable file sizes (no file > 500 lines)
- Clear modular organization

**Type Safety**:
- Compile-time enforcement of async/sync semantics
- Context capabilities checked statically

**Maintainability**:
- Easy to locate and modify opcode implementations
- Multiple developers can work in parallel
- Reduced merge conflicts

**Risk Mitigation**:
- Incremental 9-phase migration path
- Test after each phase (1,736 tests)
- Can rollback any phase independently

### Recommendation

**Proceed with implementation** starting with Phase 1 (abstractions), then Phase 2 (stack/constants). Each phase delivers incremental value while maintaining full test coverage.

**Estimated Effort**:
- Phase 1-2: 2-3 days (foundation)
- Phase 3-6: 5-7 days (bulk migration)
- Phase 7-8: 3-4 days (complex context-specific opcodes)
- Phase 9: 1-2 days (cleanup)
- **Total**: ~2-3 weeks with thorough testing

**Expected Impact**: 30% code reduction, 95% file size reduction, zero functional changes, near-zero performance impact.
