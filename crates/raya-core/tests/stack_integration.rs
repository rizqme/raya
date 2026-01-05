//! Integration tests for Stack & Frame Management (Milestone 1.4)
//!
//! Tests cover:
//! - Function call simulation
//! - Frame push/pop
//! - Local variable access across frames
//! - Stack depth tracking

use raya_core::stack::Stack;
use raya_core::value::Value;

#[test]
fn test_function_call_simulation() {
    let mut stack = Stack::new();

    // Simulate: main() calls foo(42, 100)

    // Main frame
    stack.push_frame(0, 0, 1, 0).unwrap();
    stack.store_local(0, Value::i32(999)).unwrap();

    // Call foo (2 locals)
    stack.push_frame(1, 5, 2, 2).unwrap();

    // Set up arguments as locals
    stack.store_local(0, Value::i32(42)).unwrap();
    stack.store_local(1, Value::i32(100)).unwrap();

    // In foo: use arguments as locals
    assert_eq!(stack.load_local(0).unwrap(), Value::i32(42));
    assert_eq!(stack.load_local(1).unwrap(), Value::i32(100));

    // Compute result
    let a = stack.load_local(0).unwrap().as_i32().unwrap();
    let b = stack.load_local(1).unwrap().as_i32().unwrap();
    let result = Value::i32(a + b);

    // Return
    stack.pop_frame().unwrap();

    // Push result on caller's stack
    stack.push(result).unwrap();

    // Verify
    assert_eq!(stack.pop().unwrap(), Value::i32(142));
    assert_eq!(stack.load_local(0).unwrap(), Value::i32(999));
}

#[test]
fn test_nested_function_calls() {
    let mut stack = Stack::new();

    // Simulate: main() calls foo() calls bar()

    // Main frame
    stack.push_frame(0, 0, 1, 0).unwrap();
    stack.store_local(0, Value::i32(1)).unwrap();

    // Foo frame
    stack.push_frame(1, 5, 1, 1).unwrap();
    stack.store_local(0, Value::i32(10)).unwrap();

    // Bar frame
    stack.push_frame(2, 10, 1, 1).unwrap();
    stack.store_local(0, Value::i32(20)).unwrap();

    // Bar returns 200
    stack.pop_frame().unwrap();
    stack.push(Value::i32(200)).unwrap();

    // Foo receives result and returns 210
    let bar_result = stack.pop().unwrap().as_i32().unwrap();
    let foo_arg = stack.load_local(0).unwrap().as_i32().unwrap();
    stack.pop_frame().unwrap();
    stack.push(Value::i32(bar_result + foo_arg)).unwrap();

    // Main receives result
    let result = stack.pop().unwrap();
    assert_eq!(result, Value::i32(210));
    assert_eq!(stack.load_local(0).unwrap(), Value::i32(1));
}

#[test]
fn test_local_variables_isolation() {
    let mut stack = Stack::new();

    // Frame 1 with local 0 = 100
    stack.push_frame(0, 0, 1, 0).unwrap();
    stack.store_local(0, Value::i32(100)).unwrap();

    // Frame 2 with local 0 = 200
    stack.push_frame(1, 5, 1, 1).unwrap();
    stack.store_local(0, Value::i32(200)).unwrap();

    // Frame 2's local 0 should be 200
    assert_eq!(stack.load_local(0).unwrap(), Value::i32(200));

    // Pop frame 2
    stack.pop_frame().unwrap();

    // Frame 1's local 0 should still be 100
    assert_eq!(stack.load_local(0).unwrap(), Value::i32(100));
}

#[test]
fn test_stack_depth_tracking() {
    let mut stack = Stack::new();

    // Initial depth
    assert_eq!(stack.depth(), 0);

    // Push frame
    stack.push_frame(0, 0, 2, 0).unwrap();
    assert_eq!(stack.depth(), 2); // Space for 2 locals

    // Push values
    stack.push(Value::i32(10)).unwrap();
    assert_eq!(stack.depth(), 3);

    stack.push(Value::i32(20)).unwrap();
    assert_eq!(stack.depth(), 4);

    // Pop values
    stack.pop().unwrap();
    assert_eq!(stack.depth(), 3);

    stack.pop().unwrap();
    assert_eq!(stack.depth(), 2);

    // Pop frame
    stack.pop_frame().unwrap();
    assert_eq!(stack.depth(), 0);
}

#[test]
fn test_multiple_locals() {
    let mut stack = Stack::new();

    // Frame with 5 locals
    stack.push_frame(0, 0, 5, 0).unwrap();

    // Store to all locals
    stack.store_local(0, Value::i32(10)).unwrap();
    stack.store_local(1, Value::i32(20)).unwrap();
    stack.store_local(2, Value::i32(30)).unwrap();
    stack.store_local(3, Value::i32(40)).unwrap();
    stack.store_local(4, Value::i32(50)).unwrap();

    // Verify all locals
    assert_eq!(stack.load_local(0).unwrap(), Value::i32(10));
    assert_eq!(stack.load_local(1).unwrap(), Value::i32(20));
    assert_eq!(stack.load_local(2).unwrap(), Value::i32(30));
    assert_eq!(stack.load_local(3).unwrap(), Value::i32(40));
    assert_eq!(stack.load_local(4).unwrap(), Value::i32(50));
}

#[test]
fn test_frame_info_access() {
    let mut stack = Stack::new();

    // Main frame
    stack.push_frame(0, 0, 2, 0).unwrap();

    // Foo frame
    stack.push(Value::i32(42)).unwrap();
    stack.push_frame(1, 100, 1, 1).unwrap();

    // Check frame count
    assert_eq!(stack.frame_count(), 2);

    // Pop foo
    stack.pop_frame().unwrap();
    assert_eq!(stack.frame_count(), 1);

    // Pop main
    stack.pop_frame().unwrap();
    assert_eq!(stack.frame_count(), 0);
}

#[test]
fn test_peek_operations() {
    let mut stack = Stack::new();

    stack.push_frame(0, 0, 1, 0).unwrap();

    // Push some values
    stack.push(Value::i32(10)).unwrap();
    stack.push(Value::i32(20)).unwrap();
    stack.push(Value::i32(30)).unwrap();

    // Peek at different positions
    assert_eq!(stack.peek_at(stack.depth() - 1).unwrap(), Value::i32(30));
    assert_eq!(stack.peek_at(stack.depth() - 2).unwrap(), Value::i32(20));
    assert_eq!(stack.peek_at(stack.depth() - 3).unwrap(), Value::i32(10));

    // Depth should be unchanged
    assert_eq!(stack.depth(), 4); // 1 local + 3 pushed values
}

#[test]
fn test_return_value_passing() {
    let mut stack = Stack::new();

    // Main frame
    stack.push_frame(0, 0, 1, 0).unwrap();

    // Call function
    stack.push_frame(1, 10, 1, 1).unwrap();
    stack.store_local(0, Value::i32(5)).unwrap();

    // Function computes result
    let arg = stack.load_local(0).unwrap().as_i32().unwrap();
    let result = Value::i32(arg * arg); // Square

    // Return
    stack.pop_frame().unwrap();

    // Push return value
    stack.push(result).unwrap();

    // Verify
    assert_eq!(stack.pop().unwrap(), Value::i32(25));
}
