//! Tests for module-level function access to module-level variables.
//!
//! Module-level `function` declarations should be able to read and write
//! module-level `let` variables. These are stored as globals (LoadGlobal/StoreGlobal)
//! so both the implicit main function and standalone module functions can access them.

use super::harness::*;

// === Basic Read Access ===

#[test]
fn test_module_fn_reads_module_let() {
    expect_i32(
        "let x = 42;
         function getX(): number { return x; }
         return getX();",
        42,
    );
}

#[test]
fn test_module_fn_reads_module_string() {
    expect_string(
        r#"let greeting = "hello";
         function getGreeting(): string { return greeting; }
         return getGreeting();"#,
        "hello",
    );
}

#[test]
fn test_module_fn_reads_module_bool() {
    expect_bool(
        "let flag = true;
         function getFlag(): boolean { return flag; }
         return getFlag();",
        true,
    );
}

// === Basic Write Access ===

#[test]
fn test_module_fn_writes_module_let() {
    expect_i32(
        "let counter = 0;
         function increment(): void { counter = counter + 1; }
         increment();
         increment();
         increment();
         return counter;",
        3,
    );
}

#[test]
fn test_module_fn_toggles_module_bool() {
    expect_bool(
        "let flag = false;
         function toggle(): void { flag = !flag; }
         toggle();
         return flag;",
        true,
    );
}

// === Multiple Functions Sharing Variables ===

#[test]
fn test_multiple_fns_share_module_var() {
    expect_i32(
        "let value = 10;
         function add(n: number): void { value = value + n; }
         function mul(n: number): void { value = value * n; }
         function get(): number { return value; }
         add(5);
         mul(3);
         return get();",
        45,
    );
}

#[test]
fn test_module_fn_calls_module_fn_with_module_var() {
    expect_i32(
        "let state = 1;
         function doubleState(): void { state = state * 2; }
         function tripleDouble(): void {
             doubleState();
             state = state + state + state;
         }
         tripleDouble();
         return state;",
        6,
    );
}

// === Multiple Module Variables ===

#[test]
fn test_module_fn_accesses_multiple_module_vars() {
    expect_i32(
        "let a = 10;
         let b = 20;
         let c = 30;
         function sumAll(): number { return a + b + c; }
         return sumAll();",
        60,
    );
}

// === Module Function + Top-Level Code Sharing ===

#[test]
fn test_module_fn_and_toplevel_share_var() {
    expect_i32(
        "let total = 0;
         function addToTotal(n: number): void { total = total + n; }
         total = 5;
         addToTotal(10);
         total = total + 1;
         return total;",
        16,
    );
}

// === Array Access ===

#[test]
fn test_module_fn_array_push_and_length() {
    expect_i32(
        "let items: number[] = [];
         function addItem(n: number): void { items.push(n); }
         function count(): number { return items.length; }
         addItem(1);
         addItem(2);
         addItem(3);
         return count();",
        3,
    );
}

#[test]
fn test_module_fn_reads_array_elements() {
    expect_i32(
        "let nums: number[] = [10, 20, 30];
         function sum(): number { return nums[0] + nums[1] + nums[2]; }
         return sum();",
        60,
    );
}

// === Compound Assignment ===

#[test]
fn test_module_fn_compound_assignment() {
    expect_i32(
        "let x = 10;
         function addFive(): void { x += 5; }
         addFive();
         addFive();
         return x;",
        20,
    );
}

// === Recursion with Module State ===

#[test]
fn test_recursive_fn_with_module_accumulator() {
    expect_i32(
        "let result = 0;
         function sumUpTo(n: number): void {
             if (n <= 0) { return; }
             result = result + n;
             sumUpTo(n - 1);
         }
         sumUpTo(5);
         return result;",
        15,
    );
}

// === Conditional Access ===

#[test]
fn test_module_fn_conditional_module_var() {
    expect_i32(
        "let mode = 1;
         function compute(x: number): number {
             if (mode == 1) { return x * 2; }
             return x * 3;
         }
         let a = compute(5);
         mode = 2;
         let b = compute(5);
         return a + b;",
        25,
    );
}

// === Loop in Module Function ===

#[test]
fn test_module_fn_loop_with_module_var() {
    expect_i32(
        "let total = 0;
         function addRange(start: number, end: number): void {
             let i = start;
             while (i < end) {
                 total = total + i;
                 i = i + 1;
             }
         }
         addRange(1, 6);
         return total;",
        15,
    );
}

// === Expression Initializers ===

#[test]
fn test_module_var_initialized_with_expr() {
    expect_i32(
        "let base = 3 + 4;
         function doubled(): number { return base * 2; }
         return doubled();",
        14,
    );
}

// === Variable Declared Before Function ===

#[test]
fn test_var_declared_before_fn() {
    expect_i32(
        "let myVal = 99;
         function getVal(): number { return myVal; }
         return getVal();",
        99,
    );
}

// === Set by Function ===

#[test]
fn test_module_fn_sets_var() {
    expect_i32(
        "let result: number = 0;
         function setResult(n: number): void { result = n; }
         setResult(77);
         return result;",
        77,
    );
}

// === Const Still Constant-Folded ===

#[test]
fn test_const_still_folded() {
    expect_i32(
        "const MULTIPLIER = 10;
         function compute(x: number): number { return x * MULTIPLIER; }
         return compute(4);",
        40,
    );
}

// === Static Fields Coexist ===

#[test]
fn test_static_fields_coexist_with_module_globals() {
    expect_i32_with_builtins(
        "let moduleVar = 100;
         class Counter {
             static count: number = 0;
             static increment(): void { Counter.count = Counter.count + 1; }
         }
         function getTotal(): number { return moduleVar + Counter.count; }
         Counter.increment();
         Counter.increment();
         return getTotal();",
        102,
    );
}

// === Arrow in Top-Level Still Works ===

#[test]
fn test_arrow_in_toplevel_accesses_module_global() {
    expect_i32(
        "let x = 10;
         let f = (): number => x + 5;
         return f();",
        15,
    );
}

// === Arrow Mutates Module Global ===

#[test]
fn test_arrow_mutates_module_global() {
    expect_i32(
        "let x = 10;
         let inc = (): void => { x = x + 1; };
         inc();
         inc();
         return x;",
        12,
    );
}

// === Mixed: Function + Arrow Both Access Module Var ===

// Diagnostic test A: arrow returning constant, with function decl present
#[test]
fn test_diag_arrow_const_with_fn_decl() {
    expect_i32(
        "function dummy(): void { }
         let getVal = (): number => 42;
         return getVal();",
        42,
    );
}

// Diagnostic test B: arrow reading module global, with function decl present (no mutation)
#[test]
fn test_diag_arrow_reads_global_with_fn_decl() {
    expect_i32(
        "let val = 42;
         function dummy(): void { }
         let getVal = (): number => val;
         return getVal();",
        42,
    );
}

// Diagnostic test C: arrow reading module global, function mutates, but don't call function
#[test]
fn test_diag_arrow_with_fn_decl_no_call() {
    expect_i32(
        "let val = 42;
         function setVal(n: number): void { val = n; }
         let getVal = (): number => val;
         return getVal();",
        42,
    );
}

// Diagnostic test D: call setVal, return global directly (no arrow call)
#[test]
fn test_diag_call_fn_return_global() {
    expect_i32(
        "let val = 0;
         function setVal(n: number): void { val = n; }
         let getVal = (): number => val;
         setVal(42);
         return val;",
        42,
    );
}

// Diagnostic test E: call setVal, then call arrow - the full failing case
#[test]
fn test_function_and_arrow_both_access_module_var() {
    expect_i32(
        "let val = 0;
         function setVal(n: number): void { val = n; }
         let getVal = (): number => val;
         setVal(42);
         return getVal();",
        42,
    );
}

// Diagnostic test F: call setVal BEFORE arrow decl
#[test]
fn test_diag_call_fn_before_arrow_decl() {
    expect_i32(
        "let val = 0;
         function setVal(n: number): void { val = n; }
         setVal(42);
         let getVal = (): number => val;
         return getVal();",
        42,
    );
}

// Diagnostic test G: No function decl, just arrow + direct global mutation
#[test]
fn test_diag_no_fn_decl_mutate_global() {
    expect_i32(
        "let val = 0;
         let getVal = (): number => val;
         val = 42;
         return getVal();",
        42,
    );
}

// Diagnostic test H: No arrow, just two globals + function call
#[test]
fn test_diag_no_arrow_two_globals_fn_call() {
    expect_i32(
        "let val = 0;
         function setVal(n: number): void { val = n; }
         setVal(42);
         return val;",
        42,
    );
}

// Diagnostic test I: Three globals, no arrow, function call
#[test]
fn test_diag_three_globals_fn_call() {
    expect_i32(
        "let val = 0;
         function setVal(n: number): void { val = n; }
         let other: number = 99;
         setVal(42);
         return val;",
        42,
    );
}

// Diagnostic test J: Two globals with arrow, return first global WITHOUT fn call
#[test]
fn test_diag_two_globals_arrow_no_fn_call() {
    expect_i32(
        "let val = 42;
         function dummy(): void { }
         let getVal = (): number => val;
         return val;",
        42,
    );
}

// Diagnostic test K: Return val BEFORE fn call (with two globals)
#[test]
fn test_diag_return_before_fn_call() {
    expect_i32(
        "let val = 42;
         function setVal(n: number): void { val = n; }
         let other: number = 99;
         return val;",
        42,
    );
}

// Diagnostic test L: Direct assignment instead of fn call
#[test]
fn test_diag_direct_assign_two_globals() {
    expect_i32(
        "let val = 0;
         function setVal(n: number): void { val = n; }
         let other: number = 99;
         val = 42;
         return val;",
        42,
    );
}

// Diagnostic test M: Return other after fn call (check if globals swapped)
#[test]
fn test_diag_return_other_after_fn_call() {
    expect_i32(
        "let val = 0;
         function setVal(n: number): void { val = n; }
         let other: number = 99;
         setVal(42);
         return other;",
        99,
    );
}

// Diagnostic test N: Simple two globals, fn call with no mutation
#[test]
fn test_diag_two_globals_fn_no_mutation() {
    expect_i32(
        "let a = 42;
         let b = 99;
         function noop(): void { }
         noop();
         return a;",
        42,
    );
}

