//! Scope analysis, variable capture, and lexical scoping tests adapted from typescript-go.
//!
//! Tests that variables in nested scopes, closures, and loops are correctly
//! resolved and captured. Verifies block scoping rules and variable lifetime.
//!
//! Adapted from:
//!   - blockedScopeVariableNotUnused1.ts
//!   - unusedLocalsInForInOrOf1.ts
//!   - circularDestructuring.ts

use super::harness::*;

// ============================================================================
// 1. Variable Capture in Nested Closures
//    Adapted from: typescript-go/testdata/tests/cases/compiler/
//    blockedScopeVariableNotUnused1.ts
//    Tests that variables captured in nested arrow functions are correctly resolved
// ============================================================================

#[test]
fn test_variable_captured_in_nested_closure() {
    // Variable captured by nested arrow function should be accessible
    expect_i32(
        "let x: number = 42;
         let f = (): number => {
             let g = (): number => x;
             return g();
         };
         return f();",
        42,
    );
}

#[test]
fn test_variable_captured_across_two_levels() {
    // Variable captured across two levels of nesting
    expect_i32(
        "let val: number = 10;
         let outer = (): number => {
             let mid = (): number => {
                 let inner = (): number => val;
                 return inner();
             };
             return mid();
         };
         return outer();",
        10,
    );
}

#[test]
fn test_multiple_variables_captured() {
    // Multiple variables from different scopes captured in one closure
    expect_i32(
        "let a: number = 1;
         let b: number = 2;
         function outer(): number {
             let c: number = 3;
             let f = (): number => a + b + c;
             return f();
         }
         return outer();",
        6,
    );
}

#[test]
fn test_captured_variable_mutation() {
    // Closure captures mutable variable and sees updates
    expect_i32(
        "let counter: number = 0;
         let inc = (): void => { counter = counter + 1; };
         inc();
         inc();
         inc();
         return counter;",
        3,
    );
}

// ============================================================================
// 2. Loop Variable Capture
//    Adapted from: typescript-go/testdata/tests/cases/compiler/
//    unusedLocalsInForInOrOf1.ts
//    Tests that loop variables are correctly captured by closures
// ============================================================================

#[test]
fn test_for_loop_variable_capture() {
    // Variable from for loop captured by closure inside loop body
    expect_i32(
        "let result: number = 0;
         for (let i: number = 0; i < 5; i = i + 1) {
             let capture = (): number => i;
             result = result + capture();
         }
         return result;",
        10, // 0+1+2+3+4
    );
}

#[test]
fn test_loop_closure_captures_final_value() {
    // Closure created in loop captures the variable, not a snapshot
    expect_i32(
        "let x: number = 0;
         let f = (): number => 0;
         for (let i: number = 0; i < 3; i = i + 1) {
             x = i;
             f = (): number => x;
         }
         return f();",
        2,
    );
}

#[test]
fn test_while_loop_variable_capture() {
    // Variable captured from while loop scope
    expect_i32(
        "let sum: number = 0;
         let i: number = 1;
         while (i <= 4) {
             let current: number = i;
             let add = (): number => current;
             sum = sum + add();
             i = i + 1;
         }
         return sum;",
        10,
    );
}

// ============================================================================
// 3. Block Scoping
//    Tests that block-scoped variables are correctly isolated
// ============================================================================

#[test]
fn test_block_scope_isolation() {
    // let in inner block doesn't affect outer scope
    expect_i32(
        "let x: number = 10;
         if (true) {
             let x: number = 42;
         }
         return x;",
        10,
    );
}

#[test]
fn test_block_scope_in_loop() {
    // Each iteration of a for loop has its own block scope
    expect_i32(
        "let total: number = 0;
         for (let i: number = 0; i < 3; i = i + 1) {
             let x: number = i * 10;
             total = total + x;
         }
         return total;",
        30, // 0 + 10 + 20
    );
}

#[test]
fn test_nested_block_scopes() {
    // Variables in nested blocks are isolated
    expect_i32(
        "let result: number = 0;
         if (true) {
             let a: number = 1;
             if (true) {
                 let b: number = 2;
                 result = a + b;
             }
         }
         return result;",
        3,
    );
}

#[test]
fn test_block_scope_function_inside_if() {
    // Function declared inside if block captures block-scoped variables
    expect_i32(
        "let result: number = 0;
         if (true) {
             let x: number = 42;
             let f = (): number => x;
             result = f();
         }
         return result;",
        42,
    );
}

// ============================================================================
// 4. Closure Over Mutable State
//    Tests closures that read and write shared state
// ============================================================================

#[test]
fn test_counter_closure_pair() {
    // Two closures sharing the same mutable variable
    expect_i32(
        "let count: number = 0;
         let increment = (): void => { count = count + 1; };
         let getCount = (): number => count;
         increment();
         increment();
         increment();
         return getCount();",
        3,
    );
}

#[test]
fn test_accumulator_closure() {
    // Closure that accumulates values
    expect_i32(
        "let total: number = 0;
         let add = (n: number): void => { total = total + n; };
         add(10);
         add(20);
         add(12);
         return total;",
        42,
    );
}

// ============================================================================
// 5. Variable Shadowing
//    Tests that variable shadowing works correctly across scopes
// ============================================================================

#[test]
fn test_parameter_shadows_outer() {
    // Function parameter shadows outer variable
    expect_i32(
        "let x: number = 10;
         function f(x: number): number {
             return x;
         }
         return f(42);",
        42,
    );
}

#[test]
fn test_outer_unchanged_after_shadow() {
    // Outer variable remains unchanged after shadowing in function
    expect_i32(
        "let x: number = 42;
         function f(x: number): number {
             return x + 1;
         }
         f(100);
         return x;",
        42,
    );
}

#[test]
fn test_nested_function_shadow() {
    // Nested function shadows outer variable at each level
    expect_i32(
        "let x: number = 1;
         function outer(): number {
             let x: number = 2;
             function inner(): number {
                 let x: number = 3;
                 return x;
             }
             return inner() + x;
         }
         return outer() + x;",
        6, // inner returns 3, outer x=2, outer returns 5, global x=1, result=6
    );
}

// ============================================================================
// 6. Closure in Class Methods
//    Tests closures created within class methods
// ============================================================================

#[test]
fn test_closure_in_method_captures_this() {
    // Arrow function inside method captures 'this' correctly
    expect_i32(
        "class Counter {
             value: number = 0;
             incrementBy(n: number): void {
                 let doIt = (): void => {
                     this.value = this.value + n;
                 };
                 doIt();
             }
         }
         let c = new Counter();
         c.incrementBy(10);
         c.incrementBy(32);
         return c.value;",
        42,
    );
}

#[test]
fn test_closure_captures_method_parameter() {
    // Closure inside method captures both 'this' and method parameter
    expect_i32(
        "class Multiplier {
             factor: number;
             constructor(f: number) {
                 this.factor = f;
             }
             apply(x: number): number {
                 let compute = (): number => x * this.factor;
                 return compute();
             }
         }
         let m = new Multiplier(6);
         return m.apply(7);",
        42,
    );
}

// ============================================================================
// 7. Complex Capture Scenarios
// ============================================================================

#[test]
fn test_closure_returned_from_function() {
    // Function returns a closure that captures local variable
    expect_i32(
        "function makeAdder(n: number): (x: number) => number {
             return (x: number): number => x + n;
         }
         let add10 = makeAdder(10);
         return add10(32);",
        42,
    );
}

#[test]
fn test_multiple_closures_from_same_scope() {
    // Multiple closures from the same function share the same captured scope
    expect_i32(
        "function makePair(): number {
             let shared: number = 0;
             let inc = (): void => { shared = shared + 1; };
             let get = (): number => shared;
             inc();
             inc();
             inc();
             return get();
         }
         return makePair();",
        3,
    );
}

#[test]
fn test_closure_chain() {
    // Chain of closures, each wrapping the previous
    expect_i32(
        "function chain(start: number): number {
             let a = (): number => start + 1;
             let b = (): number => a() + 2;
             let c = (): number => b() + 3;
             return c();
         }
         return chain(36);",
        42, // 36+1+2+3
    );
}
