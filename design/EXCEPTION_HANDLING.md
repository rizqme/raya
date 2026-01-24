# Exception Handling Implementation Status

**Status:** Partial Implementation (Foundation Complete)
**Date:** 2026-01-23

## Overview

This document tracks the implementation status of try-catch-finally exception handling for the Raya VM. Exception handling allows controlled error recovery through try-catch-finally blocks with proper stack unwinding and resource cleanup.

## Architecture

### Exception Handling Flow

```
1. TRY opcode: Install exception handler on Task's handler stack
2. Normal execution: Continue until exception thrown or block ends
3. THROW opcode: Begin unwinding process
4. Unwinding: Pop frames, execute finally blocks, search for catch handler
5. Catch handler: Execute catch block, clear exception
6. Finally handler: Always execute, even if no exception
7. END_TRY opcode: Remove handler from stack
8. RETHROW opcode: Re-raise exception from catch block
```

### Key Components

1. **ExceptionHandler struct** (in Task)
   - `catch_offset`: Bytecode offset to catch block (-1 if none)
   - `finally_offset`: Bytecode offset to finally block (-1 if none)
   - `stack_size`: Stack depth when handler installed (for unwinding)
   - `frame_count`: Call frame count when handler installed (for unwinding)

2. **Task fields** (for exception state)
   - `exception_handlers: Vec<ExceptionHandler>` - Handler stack
   - `current_exception: Option<Value>` - Active exception

3. **Opcodes** (0xFD-0xFF range)
   - `TRY = 0xFD` - Install handler (8-byte operands: i32 + i32)
   - `END_TRY = 0xFE` - Remove handler (no operands)
   - `RETHROW = 0xFF` - Re-raise exception (no operands)

## Implementation Status

### ✅ Completed

#### 1. Opcodes Defined
**File:** `crates/raya-bytecode/src/opcode.rs`
- [x] Added `Try`, `EndTry`, `Rethrow` opcodes (0xFD-0xFF)
- [x] Updated `from_u8()` method
- [x] Updated `name()` method
- [x] Added documentation comments

#### 2. Bytecode Verifier Updated
**File:** `crates/raya-bytecode/src/verify.rs`
- [x] Added operand sizes: Try=8 bytes, EndTry=0, Rethrow=0
- [x] Added stack effects: all have (0,0) - no direct stack manipulation

#### 3. Task Exception Handler Stack
**File:** `crates/raya-core/src/scheduler/task.rs`
- [x] Added `ExceptionHandler` struct with all fields
- [x] Added `exception_handlers` field to Task
- [x] Added `current_exception` field to Task
- [x] Implemented handler stack methods:
  - [x] `push_exception_handler()`
  - [x] `pop_exception_handler()`
  - [x] `peek_exception_handler()`
  - [x] `current_exception()`
  - [x] `set_exception()`
  - [x] `clear_exception()`
  - [x] `has_exception()`
  - [x] `exception_handler_count()`
- [x] Added comprehensive unit tests (4 tests)
- [x] All tests passing (280 total in raya-core)

#### 4. Interpreter Opcode Stubs
**File:** `crates/raya-core/src/vm/interpreter.rs`
- [x] Added Try opcode handler (reads operands, returns error)
- [x] Added EndTry opcode handler (returns error)
- [x] Added Rethrow opcode handler (returns error)
- [x] All handlers compile and test successfully

### ⏸️ Remaining Work

#### 5. Full Interpreter Implementation

**Problem:** Current interpreter in `execute_function()` is not Task-aware. Exception handling requires:
- Access to Task's exception handler stack
- Ability to unwind call frames
- Ability to jump to catch/finally blocks
- Integration with existing error propagation

**Required Changes:**

##### A. Refactor Interpreter Architecture

**Current:**
```rust
fn execute_function(&mut self, function: &Function, module: &Module) -> VmResult<Value> {
    // Uses self.stack directly
    // No Task reference
}
```

**Needed:**
```rust
fn execute_task(&mut self, task: &Arc<Task>, ip: &mut usize) -> VmResult<ExecutionState> {
    // Has access to task.push_exception_handler()
    // Has access to task.current_exception()
    // Can unwind task.stack()
}

enum ExecutionState {
    Continue,
    Suspended,
    Completed(Value),
    Exception(Value),
}
```

##### B. Implement TRY Opcode

```rust
Opcode::Try => {
    let catch_offset = self.read_i32(code, &mut ip)?;
    let finally_offset = self.read_i32(code, &mut ip)?;

    let handler = ExceptionHandler {
        catch_offset,
        finally_offset,
        stack_size: task.stack().lock().unwrap().depth(),
        frame_count: task.stack().lock().unwrap().frame_count(),
    };

    task.push_exception_handler(handler);
}
```

##### C. Implement THROW Opcode Unwinding

**Current (immediate error):**
```rust
Opcode::Throw => {
    return Err(VmError::RuntimeError("Exception thrown".to_string()));
}
```

**Needed (controlled unwinding):**
```rust
Opcode::Throw => {
    let exception = stack.pop()?;
    task.set_exception(exception);

    // Begin unwinding
    loop {
        if let Some(handler) = task.peek_exception_handler() {
            // Unwind stack to handler's saved state
            self.unwind_to_handler(&task, &handler)?;

            // Execute finally block (if present)
            if handler.finally_offset != -1 {
                ip = handler.finally_offset as usize;
                // Mark that we're in finally, not catch
                break;
            }

            // Jump to catch block (if present)
            if handler.catch_offset != -1 {
                stack.push(exception)?;
                ip = handler.catch_offset as usize;
                task.pop_exception_handler();
                task.clear_exception();
                break;
            }

            // No catch, remove handler and continue unwinding
            task.pop_exception_handler();
        } else {
            // No handler found, propagate to caller
            return Err(VmError::Exception(exception));
        }
    }
}
```

##### D. Implement Unwinding Helper

```rust
fn unwind_to_handler(&mut self, task: &Arc<Task>, handler: &ExceptionHandler) -> VmResult<()> {
    let mut stack = task.stack().lock().unwrap();

    // Pop stack to saved size
    while stack.depth() > handler.stack_size {
        stack.pop()?;
    }

    // Pop call frames to saved count
    while stack.frame_count() > handler.frame_count {
        stack.pop_frame()?;
    }

    Ok(())
}
```

##### E. Implement END_TRY Opcode

```rust
Opcode::EndTry => {
    task.pop_exception_handler();
}
```

##### F. Implement RETHROW Opcode

```rust
Opcode::Rethrow => {
    if let Some(exception) = task.current_exception() {
        // Same unwinding logic as THROW
        // But skip the current handler (already handled)
        task.pop_exception_handler();
        // Continue unwinding...
    } else {
        return Err(VmError::RuntimeError(
            "RETHROW with no active exception".to_string()
        ));
    }
}
```

#### 6. Mutex Integration

**File:** `crates/raya-core/src/vm/sync/mutex.rs`

**Problem:** When exception thrown while holding mutex, must unlock automatically.

**Required:**

1. Track mutexes held by Task:
```rust
pub struct Task {
    // ... existing fields ...
    held_mutexes: Mutex<Vec<MutexId>>,
}
```

2. Register mutex on lock:
```rust
pub fn lock(&self, task: &Arc<Task>) -> Result<MutexGuard, MutexError> {
    // ... existing lock logic ...
    task.add_held_mutex(self.id);
    Ok(guard)
}
```

3. Auto-unlock on unwind:
```rust
fn unwind_to_handler(&mut self, task: &Arc<Task>, handler: &ExceptionHandler) -> VmResult<()> {
    // Unlock all mutexes acquired since this handler was installed
    let mutexes = task.take_mutexes_since(handler.mutex_count);
    for mutex_id in mutexes {
        self.mutex_registry.unlock(mutex_id, task)?;
    }

    // ... existing unwinding logic ...
}
```

4. Update ExceptionHandler:
```rust
pub struct ExceptionHandler {
    pub catch_offset: i32,
    pub finally_offset: i32,
    pub stack_size: usize,
    pub frame_count: usize,
    pub mutex_count: usize,  // NEW: Count of held mutexes when handler installed
}
```

#### 7. Language Specification Update

**File:** `design/LANG.md`

**Changes needed:**

1. Update Section 19.3 from "Future feature" to "Implemented"
2. Add syntax specification:
```typescript
// Try-catch
try {
    // protected code
} catch (error) {
    // error handling
}

// Try-finally
try {
    // protected code
} finally {
    // cleanup code
}

// Try-catch-finally
try {
    // protected code
} catch (error) {
    // error handling
} finally {
    // cleanup always executes
}
```

3. Add semantics:
- Catch block receives error as Value
- Finally always executes (even if no error)
- Rethrow with: `throw error;` in catch block
- Stack unwinding behavior
- Mutex auto-unlock guarantee

4. Add compilation examples:
```typescript
// Source:
try {
    riskyOperation();
} catch (e) {
    console.log("Error: " + e);
}

// Bytecode:
TRY catch_offset=8 finally_offset=-1
CALL riskyOperation
END_TRY
JMP end_offset
// catch block (offset 8):
STORE_LOCAL 0  // Store exception in local 0
CONST_STR "Error: "
LOAD_LOCAL 0
SCONCAT
CALL console.log
// end:
```

#### 8. Comprehensive Testing

**New test files needed:**

##### A. Unit Tests
**File:** `crates/raya-core/tests/exception_handling.rs`

- [ ] test_try_catch_basic - throw and catch
- [ ] test_try_finally_basic - finally executes
- [ ] test_try_catch_finally - all three blocks
- [ ] test_no_exception - normal flow through try
- [ ] test_rethrow - catch and rethrow
- [ ] test_nested_try - nested try blocks
- [ ] test_catch_specific_error - different error types
- [ ] test_finally_after_return - finally runs before return
- [ ] test_finally_after_throw - finally runs during unwind

##### B. Stack Unwinding Tests
**File:** `crates/raya-core/tests/exception_unwinding.rs`

- [ ] test_unwind_single_frame
- [ ] test_unwind_multiple_frames
- [ ] test_unwind_deep_call_stack
- [ ] test_stack_restored_correctly
- [ ] test_locals_cleared_during_unwind

##### C. Mutex Integration Tests
**File:** `crates/raya-core/tests/exception_mutex.rs`

- [ ] test_mutex_unlock_on_exception
- [ ] test_multiple_mutex_unlock
- [ ] test_nested_mutex_exception
- [ ] test_mutex_no_deadlock_after_exception

##### D. Concurrency Tests
**File:** `crates/raya-core/tests/exception_tasks.rs`

- [ ] test_exception_in_spawned_task
- [ ] test_exception_propagate_through_await
- [ ] test_exception_doesnt_affect_other_tasks
- [ ] test_parent_catches_child_exception

##### E. Integration Tests
**File:** `crates/raya-core/tests/exception_integration.rs`

- [ ] test_real_world_error_handling - API calls, file I/O
- [ ] test_resource_cleanup - files, network sockets
- [ ] test_transaction_rollback - database-like operations

## Implementation Roadmap

### Phase 1: Interpreter Refactoring (2-3 days)
1. Refactor execute_function to be Task-aware
2. Add ExecutionState enum
3. Update all existing opcodes to work with new architecture
4. Ensure all existing tests pass

### Phase 2: Exception Unwinding (2-3 days)
1. Implement TRY opcode handler
2. Implement unwinding logic
3. Implement END_TRY opcode handler
4. Implement RETHROW opcode handler
5. Add unit tests for each opcode

### Phase 3: Mutex Integration (1-2 days)
1. Add mutex tracking to Task
2. Implement auto-unlock on unwind
3. Add mutex_count to ExceptionHandler
4. Add mutex integration tests

### Phase 4: Documentation (1 day)
1. Update LANG.md specification
2. Add compilation examples
3. Document semantics and guarantees

### Phase 5: Comprehensive Testing (2-3 days)
1. Write all test suites
2. Test edge cases
3. Performance testing
4. Stress testing with concurrent exceptions

**Total Estimated Time:** 8-12 days

## Current Blockers

1. **Interpreter Architecture:** The current `execute_function()` method is not Task-aware, preventing access to exception handler stack. This is the primary blocker.

2. **Stack Unwinding:** Need to implement proper call frame unwinding with local variable cleanup.

3. **IP Management:** Need to manage instruction pointer (ip) across exception unwinding and catch/finally jumps.

## Testing Strategy

### Unit Tests
- Each opcode individually
- Handler stack operations
- Unwinding logic

### Integration Tests
- End-to-end exception handling
- Multiple nested try blocks
- Complex control flow

### Stress Tests
- Many concurrent exceptions
- Deep call stacks
- High exception rate

### Edge Cases
- Exception in finally block
- Multiple rethrows
- Empty catch blocks
- Exception during mutex lock

## Success Criteria

- [ ] All exception opcodes fully implemented
- [ ] Stack unwinding works correctly
- [ ] Finally blocks always execute
- [ ] Mutexes auto-unlock on exception
- [ ] LANG.md updated
- [ ] 25+ comprehensive tests passing
- [ ] No regressions in existing tests (280+ tests)
- [ ] Documentation complete

## Notes

1. **Backward Compatibility:** Exception handling is additive - existing code without try-catch continues to work (exceptions propagate as errors).

2. **Performance:** Try-catch should have minimal overhead when no exception thrown (just handler stack push/pop).

3. **Semver:** This is a significant feature addition but doesn't break existing code.

4. **Future Work:** Consider adding typed exceptions, exception hierarchies, and stack traces.

---

**Last Updated:** 2026-01-23
**Status:** Foundation complete, interpreter refactoring needed before full implementation
