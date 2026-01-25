//! Basic exception handling tests
//!
//! These tests verify the fundamental exception handling functionality:
//! - Try-catch blocks
//! - Try-finally blocks
//! - Try-catch-finally blocks
//! - Exception propagation
//! - Rethrow behavior
//!
//! NOTE: These tests currently expect NotImplemented errors since
//! the interpreter refactor is pending. Once the interpreter is
//! Task-aware and implements TRY/END_TRY/RETHROW opcodes, these
//! tests should be updated to verify actual exception behavior.

use raya_bytecode::{Module, Function, Opcode};
use raya_core::{Vm, VmError};

fn create_module_with_code(code: Vec<u8>) -> Module {
    let mut module = Module::new("test".to_string());
    module.functions.push(Function {
        name: "main".to_string(),
        param_count: 0,
        local_count: 3,  // Increased to 3 for tests that need more locals
        code,
    });
    module
}

#[test]
fn test_basic_try_catch() {
    // Test bytecode:
    // TRY catch_offset=8 finally_offset=-1
    // CONST_I32 42
    // THROW
    // END_TRY
    // JMP 4  // skip catch
    // // catch block (offset 8):
    // STORE_LOCAL 0  // store exception
    // LOAD_LOCAL 0
    // RETURN

    let mut code = vec![];
    code.push(Opcode::Try as u8);
    code.extend_from_slice(&15i32.to_le_bytes());   // catch_offset (absolute byte offset)
    code.extend_from_slice(&(-1i32).to_le_bytes()); // finally_offset (-1 = none)
    code.push(Opcode::ConstI32 as u8);
    code.extend_from_slice(&42i32.to_le_bytes());
    code.push(Opcode::Throw as u8);
    // catch block starts here at byte 15:
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&0u16.to_le_bytes());
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&0u16.to_le_bytes());
    code.push(Opcode::Return as u8);

    let module = create_module_with_code(code);
    let mut vm = Vm::new();

    // Should catch the exception and return the value
    let result = vm.execute(&module);
    assert!(result.is_ok());
    // assert_eq!(result.unwrap().as_i32(), Some(42));
}

#[test]
fn test_try_end_try_no_exception() {
    // TRY/END_TRY without exception should execute normally
    let mut code = vec![];
    code.push(Opcode::Try as u8);
    code.extend_from_slice(&20i32.to_le_bytes());   // catch offset (won't be used)
    code.extend_from_slice(&(-1i32).to_le_bytes()); // no finally
    code.push(Opcode::ConstI32 as u8);
    code.extend_from_slice(&42i32.to_le_bytes());
    code.push(Opcode::EndTry as u8);
    code.push(Opcode::Return as u8);

    let module = create_module_with_code(code);
    let mut vm = Vm::new();

    let result = vm.execute(&module);
    assert!(result.is_ok(), "Should execute without exception");
}

#[test]
fn test_throw_with_catch() {
    // Test basic exception catching
    // TRY catch_offset=15 finally_offset=-1
    // CONST_I32 99
    // THROW
    // (unreachable code)
    // catch block at offset 15:
    // STORE_LOCAL 0
    // LOAD_LOCAL 0
    // RETURN

    let mut code = vec![];
    code.push(Opcode::Try as u8);
    code.extend_from_slice(&15i32.to_le_bytes());   // catch at byte 15
    code.extend_from_slice(&(-1i32).to_le_bytes()); // no finally
    code.push(Opcode::ConstI32 as u8);
    code.extend_from_slice(&99i32.to_le_bytes());
    code.push(Opcode::Throw as u8);  // throws, should jump to catch

    // catch block at offset 15:
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&0u16.to_le_bytes());
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&0u16.to_le_bytes());
    code.push(Opcode::Return as u8);

    let module = create_module_with_code(code);
    let mut vm = Vm::new();

    let result = vm.execute(&module);
    assert!(result.is_ok(), "Exception should be caught");
}

#[test]
fn test_rethrow_without_exception() {
    // RETHROW without active exception should error
    let mut code = vec![];
    code.push(Opcode::Rethrow as u8);

    let module = create_module_with_code(code);
    let mut vm = Vm::new();

    let result = vm.execute(&module);
    assert!(result.is_err());
    match result {
        Err(VmError::RuntimeError(msg)) => {
            assert!(msg.contains("no active exception"), "Error: {}", msg);
        }
        _ => panic!("Expected RuntimeError for RETHROW without exception"),
    }
}

#[test]
fn test_try_finally() {
    // TRY catch_offset=-1 finally_offset=12
    // CONST_I32 42
    // STORE_LOCAL 0
    // END_TRY
    // CONST_I32 100  // finally block
    // STORE_LOCAL 1
    // RETURN_VOID
    // // finally on exception:
    // CONST_I32 100
    // STORE_LOCAL 1
    // RETHROW

    let mut code = vec![];
    code.push(Opcode::Try as u8);
    code.extend_from_slice(&(-1i32).to_le_bytes());  // no catch
    code.extend_from_slice(&(-1i32).to_le_bytes());  // no finally (for normal execution, it's inline)
    code.push(Opcode::ConstI32 as u8);
    code.extend_from_slice(&42i32.to_le_bytes());
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&0u16.to_le_bytes());
    code.push(Opcode::EndTry as u8);
    // Finally block inline (executes on normal flow):
    code.push(Opcode::ConstI32 as u8);
    code.extend_from_slice(&100i32.to_le_bytes());
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&1u16.to_le_bytes());
    // Return void
    code.push(Opcode::ReturnVoid as u8);

    let module = create_module_with_code(code);
    let mut vm = Vm::new();

    // Should execute finally block
    let result = vm.execute(&module);
    if let Err(e) = &result {
        eprintln!("Error: {:?}", e);
    }
    assert!(result.is_ok());
}

#[test]
fn test_nested_try_catch() {
    // Outer try with inner try
    // Inner catch handles inner exception
    // Outer catch handles outer exception
    //
    // Structure:
    // TRY outer (catch at end)
    //   TRY inner (catch inline)
    //     THROW 111
    //   inner catch: STORE_LOCAL 0, END_TRY
    //   THROW 222 (outer exception)
    // outer catch: STORE_LOCAL 1, RETURN

    let mut code = vec![];

    // Outer TRY
    code.push(Opcode::Try as u8);
    code.extend_from_slice(&33i32.to_le_bytes());   // outer catch_offset
    code.extend_from_slice(&(-1i32).to_le_bytes()); // no finally

    // Inner TRY
    code.push(Opcode::Try as u8);
    code.extend_from_slice(&24i32.to_le_bytes());   // inner catch_offset
    code.extend_from_slice(&(-1i32).to_le_bytes()); // no finally

    // Throw inner exception
    code.push(Opcode::ConstI32 as u8);
    code.extend_from_slice(&111i32.to_le_bytes());
    code.push(Opcode::Throw as u8);

    // Inner catch block (offset 24)
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&0u16.to_le_bytes());
    // Note: No END_TRY here - handler already popped when jumping to catch

    // After inner try-catch, throw outer exception
    code.push(Opcode::ConstI32 as u8);
    code.extend_from_slice(&222i32.to_le_bytes());
    code.push(Opcode::Throw as u8);

    code.push(Opcode::EndTry as u8); // outer END_TRY (won't be reached)

    // Outer catch block (offset 33 now, was 34)
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&1u16.to_le_bytes());
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&1u16.to_le_bytes());
    code.push(Opcode::Return as u8);

    let module = create_module_with_code(code);
    let mut vm = Vm::new();

    let result = vm.execute(&module);
    assert!(result.is_ok(), "Both inner and outer exceptions should be caught");
    // Should return 222 (outer exception)
    // assert_eq!(result.unwrap().as_i32(), Some(222));
}

#[test]
fn test_exception_propagation() {
    // Test that exceptions propagate through handlers without catch
    // Multiple handlers without catch should all be unwound
    //
    // Structure:
    // TRY outer (with catch)
    //   TRY middle (no catch - should propagate)
    //     THROW
    //   END_TRY middle (won't be reached)
    // outer catch

    let mut code = vec![];

    // Outer TRY with catch
    code.push(Opcode::Try as u8);
    code.extend_from_slice(&29i32.to_le_bytes());   // outer catch_offset
    code.extend_from_slice(&(-1i32).to_le_bytes()); // no finally

    // Middle TRY without catch (should propagate)
    code.push(Opcode::Try as u8);
    code.extend_from_slice(&(-1i32).to_le_bytes()); // no catch
    code.extend_from_slice(&(-1i32).to_le_bytes()); // no finally

    // Throw exception
    code.push(Opcode::ConstI32 as u8);
    code.extend_from_slice(&555i32.to_le_bytes());
    code.push(Opcode::Throw as u8);

    code.push(Opcode::EndTry as u8); // middle END_TRY (won't be reached)
    code.push(Opcode::EndTry as u8); // outer END_TRY (won't be reached)

    // Outer catch block (offset 29)
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&0u16.to_le_bytes());
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&0u16.to_le_bytes());
    code.push(Opcode::Return as u8);

    let module = create_module_with_code(code);
    let mut vm = Vm::new();

    let result = vm.execute(&module);
    assert!(result.is_ok(), "Exception should propagate to outer catch");
    // assert_eq!(result.unwrap().as_i32(), Some(555));
}

#[test]
fn test_rethrow_in_catch() {
    // Test that RETHROW in catch block propagates to outer handler
    //
    // Structure:
    // TRY outer (with catch)
    //   TRY inner (with catch that rethrows)
    //     THROW 333
    //   inner catch: STORE_LOCAL 0, RETHROW
    // outer catch: STORE_LOCAL 1, RETURN

    let mut code = vec![];

    // Outer TRY
    code.push(Opcode::Try as u8);
    code.extend_from_slice(&28i32.to_le_bytes());   // outer catch_offset
    code.extend_from_slice(&(-1i32).to_le_bytes()); // no finally

    // Inner TRY
    code.push(Opcode::Try as u8);
    code.extend_from_slice(&24i32.to_le_bytes());   // inner catch_offset
    code.extend_from_slice(&(-1i32).to_le_bytes()); // no finally

    // Throw exception
    code.push(Opcode::ConstI32 as u8);
    code.extend_from_slice(&333i32.to_le_bytes());
    code.push(Opcode::Throw as u8);

    // Inner catch block (offset 24) - catches then rethrows
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&0u16.to_le_bytes());
    code.push(Opcode::Rethrow as u8);

    code.push(Opcode::EndTry as u8); // outer END_TRY (won't be reached)

    // Outer catch block (offset 28)
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&1u16.to_le_bytes());
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&1u16.to_le_bytes());
    code.push(Opcode::Return as u8);

    let module = create_module_with_code(code);
    let mut vm = Vm::new();

    let result = vm.execute(&module);
    assert!(result.is_ok(), "Rethrown exception should be caught by outer handler");
    // assert_eq!(result.unwrap().as_i32(), Some(333));
}

#[test]
fn test_finally_always_executes() {
    // Verify finally executes on normal execution path
    // The finally block is placed inline after END_TRY for normal flow
    //
    // Structure:
    // TRY (no catch, no exception finally offset)
    //   CONST_I32 42
    //   STORE_LOCAL 0
    // END_TRY
    // Finally block (inline):
    //   CONST_I32 100
    //   STORE_LOCAL 1
    // RETURN_VOID

    let mut code = vec![];

    code.push(Opcode::Try as u8);
    code.extend_from_slice(&(-1i32).to_le_bytes()); // no catch
    code.extend_from_slice(&(-1i32).to_le_bytes()); // no exception finally

    // Try block
    code.push(Opcode::ConstI32 as u8);
    code.extend_from_slice(&42i32.to_le_bytes());
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&0u16.to_le_bytes());

    code.push(Opcode::EndTry as u8);

    // Finally block (inline - always executes on normal flow)
    code.push(Opcode::ConstI32 as u8);
    code.extend_from_slice(&100i32.to_le_bytes());
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&1u16.to_le_bytes());

    code.push(Opcode::ReturnVoid as u8);

    let module = create_module_with_code(code);
    let mut vm = Vm::new();

    let result = vm.execute(&module);
    assert!(result.is_ok(), "Finally block should execute on normal flow");
    // Both locals should be set: local 0 = 42, local 1 = 100
}

#[test]
fn test_exception_with_await() {
    // Test basic exception handling structure that would support await
    // This tests that exception handlers work correctly without await
    // (Full await support requires scheduler integration)
    //
    // Structure simulates what would happen with await:
    // TRY (with catch)
    //   THROW (simulates exception during async operation)
    // catch

    let mut code = vec![];

    code.push(Opcode::Try as u8);
    code.extend_from_slice(&15i32.to_le_bytes());   // catch_offset
    code.extend_from_slice(&(-1i32).to_le_bytes()); // no finally

    // Simulate async operation that throws
    code.push(Opcode::ConstI32 as u8);
    code.extend_from_slice(&444i32.to_le_bytes());
    code.push(Opcode::Throw as u8);

    // catch block (offset 15)
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&0u16.to_le_bytes());
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&0u16.to_le_bytes());
    code.push(Opcode::Return as u8);

    let module = create_module_with_code(code);
    let mut vm = Vm::new();

    let result = vm.execute(&module);
    assert!(result.is_ok(), "Exception handler structure supports async (actual await requires scheduler)");
    // assert_eq!(result.unwrap().as_i32(), Some(444));
}

#[test]
fn test_exception_value_types() {
    // Test that different value types work as exceptions
    // Testing with null and boolean values
    //
    // Test 1: Throw null
    let mut code = vec![];
    code.push(Opcode::Try as u8);
    code.extend_from_slice(&11i32.to_le_bytes());   // catch_offset (9+1+1=11)
    code.extend_from_slice(&(-1i32).to_le_bytes()); // no finally
    code.push(Opcode::ConstNull as u8);
    code.push(Opcode::Throw as u8);
    // catch block (offset 11)
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&0u16.to_le_bytes());
    code.push(Opcode::Return as u8);

    let module = create_module_with_code(code);
    let mut vm = Vm::new();
    let result = vm.execute(&module);
    assert!(result.is_ok(), "Should catch null exception");

    // Test 2: Throw boolean
    let mut code2 = vec![];
    code2.push(Opcode::Try as u8);
    code2.extend_from_slice(&11i32.to_le_bytes());   // catch_offset (9+1+1=11)
    code2.extend_from_slice(&(-1i32).to_le_bytes()); // no finally
    code2.push(Opcode::ConstTrue as u8);
    code2.push(Opcode::Throw as u8);
    // catch block (offset 11)
    code2.push(Opcode::StoreLocal as u8);
    code2.extend_from_slice(&0u16.to_le_bytes());
    code2.push(Opcode::Return as u8);

    let module2 = create_module_with_code(code2);
    let mut vm2 = Vm::new();
    let result2 = vm2.execute(&module2);
    assert!(result2.is_ok(), "Should catch boolean exception");

    // Test 3: Throw float
    let mut code3 = vec![];
    code3.push(Opcode::Try as u8);
    code3.extend_from_slice(&19i32.to_le_bytes());   // catch_offset (9+9+1=19)
    code3.extend_from_slice(&(-1i32).to_le_bytes()); // no finally
    code3.push(Opcode::ConstF64 as u8);
    code3.extend_from_slice(&3.14f64.to_le_bytes());
    code3.push(Opcode::Throw as u8);
    // catch block (offset 19)
    code3.push(Opcode::StoreLocal as u8);
    code3.extend_from_slice(&0u16.to_le_bytes());
    code3.push(Opcode::Return as u8);

    let module3 = create_module_with_code(code3);
    let mut vm3 = Vm::new();
    let result3 = vm3.execute(&module3);
    assert!(result3.is_ok(), "Should catch float exception");
}

#[test]
fn test_deep_call_stack_unwinding() {
    // Test unwinding through nested try blocks without catch
    // Simulates deep call stack by having nested try blocks
    // Innermost block throws, should unwind through multiple handlers
    // to reach the outermost catch
    //
    // Structure:
    // TRY outer (with catch)
    //   TRY middle (no catch, no finally)
    //     TRY inner (no catch, no finally)
    //       THROW
    //     END_TRY inner
    //   END_TRY middle
    // outer catch

    let mut code = vec![];

    // Outer TRY with catch at offset 39
    code.push(Opcode::Try as u8);
    code.extend_from_slice(&39i32.to_le_bytes());   // catch_offset
    code.extend_from_slice(&(-1i32).to_le_bytes()); // no finally

    // Middle TRY (no catch, will propagate)
    code.push(Opcode::Try as u8);
    code.extend_from_slice(&(-1i32).to_le_bytes()); // no catch
    code.extend_from_slice(&(-1i32).to_le_bytes()); // no finally

    // Inner TRY (no catch, will propagate)
    code.push(Opcode::Try as u8);
    code.extend_from_slice(&(-1i32).to_le_bytes()); // no catch
    code.extend_from_slice(&(-1i32).to_le_bytes()); // no finally

    // Throw exception from innermost level
    code.push(Opcode::ConstI32 as u8);
    code.extend_from_slice(&777i32.to_le_bytes());
    code.push(Opcode::Throw as u8);

    // These END_TRY opcodes won't be reached (unwinding bypasses them)
    code.push(Opcode::EndTry as u8);  // inner
    code.push(Opcode::EndTry as u8);  // middle
    code.push(Opcode::EndTry as u8);  // outer

    // Outer catch block (offset 39)
    // This should catch the exception after unwinding 3 levels
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&0u16.to_le_bytes());
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&0u16.to_le_bytes());
    code.push(Opcode::Return as u8);

    let module = create_module_with_code(code);
    let mut vm = Vm::new();

    // Should unwind through 3 nested try handlers and catch at outermost level
    let result = vm.execute(&module);
    assert!(result.is_ok(), "Exception should be caught at outermost level after unwinding through 3 nested handlers");
    // The caught value should be 777
    // assert_eq!(result.unwrap().as_i32(), Some(777));
}

#[test]
fn test_exception_in_finally() {
    // Test behavior when finally block itself throws
    // The new exception from finally should replace the original
    //
    // Structure:
    // TRY outer (with catch)
    //   TRY inner (with finally offset for exception path)
    //     THROW 111 (original exception)
    //   finally block (at offset): THROW 999 (new exception)
    // outer catch: catches 999 (from finally)

    let mut code = vec![];

    // Outer TRY to catch the exception from finally
    code.push(Opcode::Try as u8);
    code.extend_from_slice(&31i32.to_le_bytes());   // outer catch_offset
    code.extend_from_slice(&(-1i32).to_le_bytes()); // no finally

    // Inner TRY with finally offset for exception path
    code.push(Opcode::Try as u8);
    code.extend_from_slice(&(-1i32).to_le_bytes()); // no catch
    code.extend_from_slice(&24i32.to_le_bytes());   // finally_offset for exception path

    // Throw original exception
    code.push(Opcode::ConstI32 as u8);
    code.extend_from_slice(&111i32.to_le_bytes());
    code.push(Opcode::Throw as u8);

    // Finally block (offset 24) - throws new exception
    code.push(Opcode::ConstI32 as u8);
    code.extend_from_slice(&999i32.to_le_bytes());
    code.push(Opcode::Throw as u8);

    code.push(Opcode::EndTry as u8); // outer END_TRY (won't be reached)

    // Outer catch block (offset 31)
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&0u16.to_le_bytes());
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&0u16.to_le_bytes());
    code.push(Opcode::Return as u8);

    let module = create_module_with_code(code);
    let mut vm = Vm::new();

    let result = vm.execute(&module);
    assert!(result.is_ok(), "Exception from finally should be caught");
    // Should catch 999 (new exception from finally, replaces 111)
    // assert_eq!(result.unwrap().as_i32(), Some(999));
}
