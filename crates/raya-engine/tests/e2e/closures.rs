//! Phase 12: Closure tests
//!
//! Tests for arrow functions and closures (capturing outer variables).

use super::harness::*;

// ============================================================================
// Simple Arrow Functions
// ============================================================================

#[test]
fn test_arrow_expression_body() {
    expect_i32(
        "let double = (x: number): number => x * 2;
         return double(21);",
        42,
    );
}

#[test]
fn test_arrow_block_body() {
    expect_i32(
        "let add = (a: number, b: number): number => {
             return a + b;
         };
         return add(10, 32);",
        42,
    );
}

#[test]
fn test_arrow_no_params() {
    expect_i32(
        "let getAnswer = (): number => 42;
         return getAnswer();",
        42,
    );
}

#[test]
fn test_arrow_single_param() {
    expect_i32(
        "let double = (x: number): number => x * 2;
         return double(21);",
        42,
    );
}

// ============================================================================
// Closures - Capturing Variables
// ============================================================================

#[test]
fn test_closure_capture_single() {
    expect_i32(
        "let x = 10;
         let addX = (y: number): number => x + y;
         return addX(32);",
        42,
    );
}

#[test]
fn test_closure_capture_multiple() {
    expect_i32(
        "let a = 10;
         let b = 20;
         let sum = (): number => a + b + 12;
         return sum();",
        42,
    );
}

#[test]
fn test_closure_capture_and_modify() {
    // Closures capture by reference, so modifications are visible
    expect_i32(
        "let count = 0;
         let increment = (): number => {
             count = count + 1;
             return count;
         };
         increment();
         increment();
         return count;",
        2,
    );
}

#[test]
fn test_closure_capture_parameter() {
    expect_i32(
        "function makeAdder(x: number): (y: number) => number {
             return (y: number): number => x + y;
         }
         let add10 = makeAdder(10);
         return add10(32);",
        42,
    );
}

// ============================================================================
// Nested Closures
// ============================================================================

#[test]
fn test_closure_nested() {
    expect_i32(
        "let a = 10;
         let outer = (): number => {
             let b = 20;
             let inner = (): number => a + b + 12;
             return inner();
         };
         return outer();",
        42,
    );
}

#[test]
fn test_closure_nested_modification() {
    expect_i32(
        "let value = 0;
         let outer = (): number => {
             let inner = (): void => {
                 value = 42;
             };
             inner();
             return value;
         };
         return outer();",
        42,
    );
}

// ============================================================================
// Higher-Order Functions
// ============================================================================

#[test]
fn test_closure_as_parameter() {
    expect_i32(
        "function apply(f: (x: number) => number, x: number): number {
             return f(x);
         }
         let double = (x: number): number => x * 2;
         return apply(double, 21);",
        42,
    );
}

#[test]
fn test_closure_returned() {
    expect_i32(
        "function multiplier(n: number): (x: number) => number {
             return (x: number): number => x * n;
         }
         let times2 = multiplier(2);
         return times2(21);",
        42,
    );
}

#[test]
fn test_closure_compose() {
    expect_i32(
        "function compose<A, B, C>(f: (b: B) => C, g: (a: A) => B): (a: A) => C {
             return (a: A): C => f(g(a));
         }
         let double = (x: number): number => x * 2;
         let addOne = (x: number): number => x + 1;
         let doubleThenAddOne = compose(addOne, double);
         return doubleThenAddOne(20);",
        41,
    );
}

// ============================================================================
// Closures in Loops
// ============================================================================

// Simpler test to debug closure in loop capture behavior
#[test]
fn test_closure_capture_in_loop_simple() {
    // Create a single closure in a specific iteration
    expect_i32(
        "let closure: () => number = (): number => 0;
         for (let i = 0; i < 3; i = i + 1) {
             if (i == 2) {
                 closure = (): number => i;
             }
         }
         return closure();",
        2,
    );
}

// Test multiple closures without array to isolate the issue
#[test]
fn test_closure_capture_in_loop_multiple() {
    // Create three separate closures in different iterations
    expect_i32(
        "let c0: () => number = (): number => -1;
         let c1: () => number = (): number => -1;
         let c2: () => number = (): number => -1;
         for (let i = 0; i < 3; i = i + 1) {
             if (i == 0) { c0 = (): number => i; }
             if (i == 1) { c1 = (): number => i; }
             if (i == 2) { c2 = (): number => i; }
         }
         return c0() + c1() * 10 + c2() * 100;",
        210,  // 0 + 10 + 200
    );
}

// Test single closure in array (no loop) to verify array + closure works
#[test]
fn test_closure_in_array_simple() {
    expect_i32(
        "let closures: Array<() => number> = [];
         let x = 42;
         closures.push((): number => x);
         return closures[0]();",
        42,
    );
}

// Test multiple closures in array capturing different variables (no loop)
#[test]
fn test_closure_in_array_multiple() {
    expect_i32(
        "let closures: Array<() => number> = [];
         let a = 1;
         let b = 2;
         let c = 3;
         closures.push((): number => a);
         closures.push((): number => b);
         closures.push((): number => c);
         return closures[0]() + closures[1]() * 10 + closures[2]() * 100;",
        321,  // 1 + 20 + 300
    );
}

// Test multiple closures in loop capturing the same variable
#[test]
fn test_closure_in_array_loop_same_var() {
    // This is the problematic case - capturing same loop var
    expect_i32(
        "let closures: Array<() => number> = [];
         for (let i = 0; i < 3; i = i + 1) {
             closures.push((): number => i);
         }
         return closures[0]() + closures[1]() * 10 + closures[2]() * 100;",
        210,  // 0 + 10 + 200
    );
}

// Test with intermediate variable
#[test]
fn test_closure_in_array_loop_with_temp() {
    expect_i32(
        "let closures: Array<() => number> = [];
         for (let i = 0; i < 3; i = i + 1) {
             let closure = (): number => i;
             closures.push(closure);
         }
         return closures[0]() + closures[1]() * 10 + closures[2]() * 100;",
        210,  // 0 + 10 + 200
    );
}

// Test unconditional closure creation without array (always overwrite same var)
#[test]
fn test_closure_in_loop_unconditional() {
    // Every iteration overwrites the closure - should capture last value
    expect_i32(
        "let closure: () => number = (): number => -1;
         for (let i = 0; i < 3; i = i + 1) {
             closure = (): number => i;
         }
         return closure();",
        2,  // Last iteration: i=2
    );
}

#[test]
fn test_closure_in_loop_let() {
    // With 'let', each iteration gets its own binding
    expect_i32(
        "let closures: Array<() => number> = [];
         for (let i = 0; i < 3; i = i + 1) {
             closures.push((): number => i);
         }
         return closures[2]();",
        2,
    );
}

#[test]
fn test_closure_capture_loop_variable() {
    expect_i32(
        "let sum = 0;
         let adders: Array<() => void> = [];
         for (let i = 1; i <= 3; i = i + 1) {
             adders.push((): void => {
                 sum = sum + i;
             });
         }
         for (let adder of adders) {
             adder();
         }
         return sum;",
        6, // 1 + 2 + 3
    );
}

// ============================================================================
// Closure with this
// ============================================================================

#[test]
fn test_arrow_preserves_this() {
    // Arrow functions capture 'this' from enclosing scope
    expect_i32(
        "class Counter {
             value: number = 0;

             delayedIncrement(): void {
                 let increment = (): void => {
                     this.value = this.value + 1;
                 };
                 increment();
             }
         }
         let c = new Counter();
         c.delayedIncrement();
         return c.value;",
        1,
    );
}

// ============================================================================
// Immediately Invoked Function Expression (IIFE)
// ============================================================================

#[test]
fn test_iife_simple() {
    expect_i32(
        "let result = ((): number => {
             let x = 40;
             let y = 2;
             return x + y;
         })();
         return result;",
        42,
    );
}

#[test]
fn test_iife_with_args() {
    expect_i32(
        "let result = ((x: number, y: number): number => x + y)(10, 32);
         return result;",
        42,
    );
}

// ============================================================================
// Closures and Recursion
// ============================================================================

#[test]
fn test_closure_recursive() {
    expect_i32(
        "let factorial: (n: number) => number;
         factorial = (n: number): number => {
             if (n <= 1) { return 1; }
             return n * factorial(n - 1);
         };
         return factorial(5);",
        120,
    );
}
