//! Bug hunting tests — round 2
//!
//! More aggressive edge case tests targeting:
//! - Closure/mutation interactions with class fields
//! - Generic class with nullable fields
//! - Complex array manipulation chains
//! - Mixed control flow (try/catch + loop + switch)
//! - Method override with super + closure interaction
//! - String template literals in complex contexts
//! - Array.splice, Array.findIndex, Array.fill (if supported)
//! - Reassigning function references
//! - Deeply nested generics
//! - Interface-like patterns via abstract classes
//! - Complex nullish coalescing in method chains
//! - Closure that escapes via class field
//! - Multiple generic constraints

use super::harness::*;

// ============================================================================
// 1. Closure Stored in Class Field
//    Closures that escape their creation scope via class fields.
// ============================================================================

#[test]
fn test_closure_stored_in_class_field() {
    expect_i32(
        "class Handler {
             action: () => int;
             constructor(fn: () => int) { this.action = fn; }
         }
         let x = 42;
         let h = new Handler((): int => x);
         return h.action();",
        42,
    );
}

#[test]
fn test_closure_stored_in_class_mutates_outer() {
    expect_i32(
        "class Setter {
             set: (v: int) => void;
             constructor(fn: (v: int) => void) { this.set = fn; }
         }
         let value = 0;
         let s = new Setter((v: int): void => { value = v; });
         s.set(42);
         return value;",
        42,
    );
}

// ============================================================================
// 2. Generic Class with Nullable Field
// ============================================================================

#[test]
fn test_generic_class_nullable_field() {
    expect_i32(
        "class Maybe<T> {
             value: T | null;
             constructor(v: T | null) { this.value = v; }
             getOr(fallback: T): T {
                 if (this.value !== null) { return this.value; }
                 return fallback;
             }
         }
         let m = new Maybe<int>(null);
         return m.getOr(42);",
        42,
    );
}

#[test]
fn test_generic_class_nullable_field_present() {
    expect_i32(
        "class Maybe<T> {
             value: T | null;
             constructor(v: T | null) { this.value = v; }
             getOr(fallback: T): T {
                 if (this.value !== null) { return this.value; }
                 return fallback;
             }
         }
         let m = new Maybe<int>(42);
         return m.getOr(0);",
        42,
    );
}

// ============================================================================
// 3. Complex Control Flow: try/catch + loop + switch
// ============================================================================

#[test]
fn test_try_catch_in_switch_in_loop() {
    expect_i32_with_builtins(
        "let result = 0;
         let ops: int[] = [1, 2, 3];
         for (const op of ops) {
             switch (op) {
                 case 1:
                     try {
                         result = result + 10;
                     } catch (e) {
                         result = -1;
                     }
                     break;
                 case 2:
                     result = result + 12;
                     break;
                 case 3:
                     result = result + 20;
                     break;
             }
         }
         return result;",
        42,
    );
}

// ============================================================================
// 4. Method Override with Super Call + Closure
// ============================================================================

#[test]
fn test_method_override_super_plus_closure() {
    expect_i32(
        "class Base {
             compute(): int { return 20; }
         }
         class Child extends Base {
             compute(): int {
                 let base = super.compute();
                 let addExtra = (): int => base + 22;
                 return addExtra();
             }
         }
         return new Child().compute();",
        42,
    );
}

// ============================================================================
// 5. Reassigning Function References
// ============================================================================

#[test]
fn test_reassign_function_variable() {
    expect_i32(
        "let fn = (x: int): int => x + 1;
         fn = (x: int): int => x * 2;
         return fn(21);",
        42,
    );
}

#[test]
fn test_function_passed_as_argument_then_called() {
    expect_i32(
        "function apply(f: (x: int) => int, val: int): int {
             return f(val);
         }
         function double(x: int): int { return x * 2; }
         return apply(double, 21);",
        42,
    );
}

#[test]
fn test_function_stored_in_array_called() {
    expect_i32(
        "let fns: ((x: int) => int)[] = [];
         fns.push((x: int): int => x * 2);
         fns.push((x: int): int => x + 1);
         return fns[0](21);",
        42,
    );
}

// ============================================================================
// 6. Nested Generic Functions
// ============================================================================

#[test]
fn test_generic_function_called_from_generic() {
    expect_i32(
        "function wrap<T>(x: T): T { return x; }
         function doubleWrap<T>(x: T): T { return wrap<T>(x); }
         return doubleWrap<int>(42);",
        42,
    );
}

// ============================================================================
// 7. Complex String Template Literals
// ============================================================================

#[test]
fn test_template_with_nested_function_calls() {
    expect_string(
        "function getName(): string { return \"World\"; }
         function greet(name: string): string { return `Hello, ${name}!`; }
         return greet(getName());",
        "Hello, World!",
    );
}

#[test]
fn test_template_with_class_field() {
    expect_string(
        "class User {
             name: string;
             constructor(n: string) { this.name = n; }
             greet(): string {
                 return `Hello, ${this.name}!`;
             }
         }
         return new User(\"Raya\").greet();",
        "Hello, Raya!",
    );
}

#[test]
fn test_template_with_array_access() {
    expect_string(
        "let names: string[] = [\"Alice\", \"Bob\"];
         return `First: ${names[0]}, Second: ${names[1]}`;",
        "First: Alice, Second: Bob",
    );
}

// ============================================================================
// 8. Array.findIndex (if supported)
// ============================================================================

#[test]
fn test_array_find_index() {
    expect_i32(
        "let arr: int[] = [10, 20, 42, 30];
         return arr.findIndex((x: int): boolean => x == 42);",
        2,
    );
}

#[test]
fn test_array_find_index_not_found() {
    expect_i32(
        "let arr: int[] = [1, 2, 3];
         return arr.findIndex((x: int): boolean => x == 99);",
        -1,
    );
}

// ============================================================================
// 9. Array.fill (if supported)
// ============================================================================

#[test]
fn test_array_fill() {
    expect_i32(
        "let arr: int[] = [0, 0, 0];
         arr.fill(42);
         return arr[0] + arr[1] + arr[2];",
        126,
    );
}

// ============================================================================
// 10. Array.splice (if supported)
// ============================================================================

// BUG DISCOVERY: Array.splice() method is not recognized by the type checker.
// Error: NotCallable { ty: "TypeId(6)" } — the method doesn't exist on arrays.
// splice() is a standard TypeScript/JavaScript array method.
// #[test]
// fn test_array_splice_remove() {
//     expect_i32(
//         "let arr: int[] = [1, 2, 42, 4, 5];
//          arr.splice(0, 2);
//          return arr[0];",
//         42,
//     );
// }

// BUG DISCOVERY: Same splice issue.
// #[test]
// fn test_array_splice_insert() {
//     expect_i32(
//         "let arr: int[] = [1, 2, 3];
//          arr.splice(1, 0, 42);
//          return arr[1];",
//         42,
//     );
// }

// ============================================================================
// 11. Complex For Loop Patterns
// ============================================================================

#[test]
fn test_for_loop_no_init() {
    expect_i32(
        "let i = 0;
         for (; i < 42; i = i + 1) {}
         return i;",
        42,
    );
}

#[test]
fn test_for_loop_no_update() {
    expect_i32(
        "let result = 0;
         for (let i = 0; i < 42;) {
             result = result + 1;
             i = i + 1;
         }
         return result;",
        42,
    );
}

#[test]
fn test_for_loop_empty_body() {
    expect_i32(
        "let x = 0;
         for (let i = 0; i < 42; i = i + 1) {
             x = i + 1;
         }
         return x;",
        42,
    );
}

// ============================================================================
// 12. Multiple Levels of Inheritance — Method Resolution
// ============================================================================

#[test]
fn test_three_level_method_override() {
    expect_i32(
        "class A {
             val(): int { return 1; }
         }
         class B extends A {
             val(): int { return 2; }
         }
         class C extends B {
             val(): int { return 42; }
         }
         let obj: A = new C();
         return obj.val();",
        42,
    );
}

#[test]
fn test_middle_class_not_overriding() {
    expect_i32(
        "class A {
             val(): int { return 42; }
         }
         class B extends A {
             // does NOT override val
         }
         class C extends B {
             // does NOT override val either
         }
         let obj = new C();
         return obj.val();",
        42,
    );
}

#[test]
fn test_grandchild_overrides_but_not_child() {
    expect_i32(
        "class A {
             val(): int { return 1; }
         }
         class B extends A {
             // does not override
         }
         class C extends B {
             val(): int { return 42; }
         }
         let obj: A = new C();
         return obj.val();",
        42,
    );
}

// ============================================================================
// 13. Complex Null Narrowing in Expressions
// ============================================================================

#[test]
fn test_null_check_with_and_operator() {
    expect_i32(
        "let x: int | null = 42;
         if (x !== null && x > 10) {
             return x;
         }
         return 0;",
        42,
    );
}

#[test]
fn test_null_check_or_default() {
    expect_i32(
        "function getOrDefault(x: int | null): int {
             if (x === null) { return 42; }
             return x;
         }
         return getOrDefault(null);",
        42,
    );
}

#[test]
fn test_null_in_ternary() {
    expect_i32(
        "let x: int | null = null;
         let result = x === null ? 42 : x;
         return result;",
        42,
    );
}

// ============================================================================
// 14. Recursive Generic Functions
// ============================================================================

#[test]
fn test_generic_identity_recursive() {
    expect_i32(
        "function repeatApply<T>(fn: (x: T) => T, x: T, n: int): T {
             if (n <= 0) { return x; }
             return repeatApply<T>(fn, fn(x), n - 1);
         }
         let addOne = (x: int): int => x + 1;
         return repeatApply<int>(addOne, 0, 42);",
        42,
    );
}

// ============================================================================
// 15. Array of Different Subclass Instances
// ============================================================================

#[test]
fn test_heterogeneous_array_virtual_dispatch() {
    expect_i32(
        "class Shape {
             area(): int { return 0; }
         }
         class Rect extends Shape {
             w: int;
             h: int;
             constructor(w: int, h: int) { super(); this.w = w; this.h = h; }
             area(): int { return this.w * this.h; }
         }
         class Triangle extends Shape {
             b: int;
             h: int;
             constructor(b: int, h: int) { super(); this.b = b; this.h = h; }
             area(): int { return this.b * this.h / 2; }
         }
         let shapes: Shape[] = [new Rect(6, 5), new Triangle(4, 6)];
         let total = 0;
         for (const s of shapes) {
             total = total + s.area();
         }
         return total;",
        42,
    );
}

// ============================================================================
// 16. String Concatenation with Numbers via Template
// ============================================================================

// FIXED: String concatenation now uses Display format instead of Debug
#[test]
fn test_string_concat_with_number_via_plus() {
    expect_string(
        "let x: int = 42;
         return \"answer: \" + x;",
        "answer: 42",
    );
}

#[test]
fn test_string_concat_multiple_types() {
    expect_string(
        "let n: int = 42;
         let b: boolean = true;
         return \"n=\" + n + \" b=\" + b;",
        "n=42 b=true",
    );
}

// ============================================================================
// 17. Complex While Loop Patterns
// ============================================================================

#[test]
fn test_while_with_multiple_exit_conditions() {
    expect_i32(
        "let i = 0;
         let found = false;
         while (i < 100 && !found) {
             if (i == 42) { found = true; }
             else { i = i + 1; }
         }
         return i;",
        42,
    );
}

#[test]
fn test_nested_while_with_break() {
    expect_i32(
        "let outer = 0;
         let result = 0;
         while (outer < 10) {
             let inner = 0;
             while (inner < 10) {
                 if (outer * inner == 42) {
                     result = outer * 10 + inner;
                     break;
                 }
                 inner = inner + 1;
             }
             if (result > 0) { break; }
             outer = outer + 1;
         }
         return result;",
        67, // 6 * 7 = 42, result = 67
    );
}

// ============================================================================
// 18. Boolean Coercion Patterns
// ============================================================================

#[test]
fn test_boolean_from_comparison_result() {
    expect_bool(
        "function isPositive(x: int): boolean { return x > 0; }
         return isPositive(42);",
        true,
    );
}

#[test]
fn test_boolean_as_int_gate() {
    expect_i32(
        "let flag = true;
         let x = 42;
         if (flag && x > 0) { return x; }
         return 0;",
        42,
    );
}

// ============================================================================
// 19. Complex Object / Class Initialization
// ============================================================================

#[test]
fn test_class_with_computed_field() {
    expect_i32(
        "class Computed {
             doubled: int;
             constructor(x: int) {
                 this.doubled = x * 2;
             }
         }
         return new Computed(21).doubled;",
        42,
    );
}

#[test]
fn test_class_with_method_called_in_constructor() {
    expect_i32(
        "class Init {
             value: int;
             constructor(x: int) {
                 this.value = this.compute(x);
             }
             compute(x: int): int { return x * 2; }
         }
         return new Init(21).value;",
        42,
    );
}

// ============================================================================
// 20. Edge Cases in Array Index Operations
// ============================================================================

#[test]
fn test_array_set_by_computed_index() {
    expect_i32(
        "let arr: int[] = [0, 0, 0];
         let idx = 1;
         arr[idx] = 42;
         return arr[1];",
        42,
    );
}

#[test]
fn test_array_swap() {
    expect_i32(
        "let arr: int[] = [42, 1];
         let tmp = arr[0];
         arr[0] = arr[1];
         arr[1] = tmp;
         return arr[1];",
        42,
    );
}

// ============================================================================
// 21. Complex Closure Scenarios — Closure Over Loop Variables
// ============================================================================

#[test]
fn test_closure_over_for_loop_var_correct_capture() {
    // Each closure should capture its own `i` value
    expect_i32(
        "let fns: (() => int)[] = [];
         for (let i = 0; i < 5; i = i + 1) {
             let captured = i;
             fns.push((): int => captured);
         }
         // Sum: 0+1+2+3+4 = 10
         let sum = 0;
         for (const fn of fns) {
             sum = sum + fn();
         }
         return sum;",
        10,
    );
}

#[test]
fn test_closure_in_for_of_captures_element() {
    expect_i32(
        "let fns: (() => int)[] = [];
         let items: int[] = [10, 12, 20];
         for (const item of items) {
             let captured = item;
             fns.push((): int => captured);
         }
         return fns[0]() + fns[1]() + fns[2]();",
        42,
    );
}

// ============================================================================
// 22. Abstract Class with Constructor
// ============================================================================

#[test]
fn test_abstract_class_constructor_with_fields() {
    expect_i32(
        "abstract class Vehicle {
             speed: int;
             constructor(s: int) { this.speed = s; }
             abstract fuelCost(): int;
             totalCost(): int { return this.speed + this.fuelCost(); }
         }
         class Car extends Vehicle {
             constructor() { super(30); }
             fuelCost(): int { return 12; }
         }
         return new Car().totalCost();",
        42,
    );
}

// ============================================================================
// 23. Recursive Fibonacci (Deep Recursion Stress)
// ============================================================================

#[test]
fn test_fibonacci_recursive() {
    expect_i32(
        "function fib(n: int): int {
             if (n <= 1) { return n; }
             return fib(n - 1) + fib(n - 2);
         }
         return fib(10);",
        55,
    );
}

#[test]
fn test_fibonacci_iterative() {
    expect_i32(
        "function fib(n: int): int {
             let a = 0;
             let b = 1;
             for (let i = 0; i < n; i = i + 1) {
                 let tmp = a + b;
                 a = b;
                 b = tmp;
             }
             return a;
         }
         return fib(10);",
        55,
    );
}

// ============================================================================
// 24. GCD / Mathematical Algorithms
// ============================================================================

#[test]
fn test_gcd_euclidean() {
    expect_i32(
        "function gcd(a: int, b: int): int {
             while (b != 0) {
                 let t = b;
                 b = a % b;
                 a = t;
             }
             return a;
         }
         return gcd(84, 42);",
        42,
    );
}

// ============================================================================
// 25. Complex Array Manipulation — Building Arrays Dynamically
// ============================================================================

#[test]
fn test_build_array_in_loop() {
    expect_i32(
        "let arr: int[] = [];
         for (let i = 0; i < 10; i = i + 1) {
             arr.push(i * i);
         }
         return arr[6] + arr[1] + arr[2];",
        41,
    );
}

#[test]
fn test_array_of_arrays_dynamic() {
    // matrix = [[1,2,3],[4,5,6],[7,8,9]]
    // matrix[2][2] = 9
    expect_i32(
        "let matrix: int[][] = [];
         for (let i = 0; i < 3; i = i + 1) {
             let row: int[] = [];
             for (let j = 0; j < 3; j = j + 1) {
                 row.push(i * 3 + j + 1);
             }
             matrix.push(row);
         }
         return matrix[2][2];",
        9,
    );
}

// ============================================================================
// 26. Switch with String Discriminant
// ============================================================================

#[test]
fn test_switch_on_string() {
    expect_i32(
        "function eval(op: string, a: int, b: int): int {
             switch (op) {
                 case \"add\": return a + b;
                 case \"mul\": return a * b;
                 default: return 0;
             }
         }
         return eval(\"mul\", 6, 7);",
        42,
    );
}

#[test]
fn test_switch_on_string_default() {
    expect_i32(
        "function eval(op: string): int {
             switch (op) {
                 case \"yes\": return 1;
                 case \"no\": return 0;
                 default: return 42;
             }
         }
         return eval(\"maybe\");",
        42,
    );
}

// ============================================================================
// 27. Closure Capturing Multiple Variables from Different Scopes
// ============================================================================

#[test]
fn test_closure_captures_from_multiple_scopes() {
    expect_i32(
        "let a = 10;
         function outer(): () => int {
             let b = 12;
             function inner(): () => int {
                 let c = 20;
                 return (): int => a + b + c;
             }
             return inner();
         }
         return outer()();",
        42,
    );
}

// ============================================================================
// 28. Complex Ternary + Null Interaction
// ============================================================================

#[test]
fn test_ternary_chain_with_null_checks() {
    expect_i32(
        "let a: int | null = null;
         let b: int | null = null;
         let c: int | null = 42;
         let result = a !== null ? a : (b !== null ? b : (c !== null ? c : 0));
         return result;",
        42,
    );
}

// ============================================================================
// 29. Class with Generic Method
// ============================================================================

#[test]
fn test_class_with_generic_method() {
    expect_i32(
        "class Transformer {
             apply<T>(x: T, fn: (v: T) => T): T {
                 return fn(x);
             }
         }
         let t = new Transformer();
         return t.apply<int>(21, (v: int): int => v * 2);",
        42,
    );
}

// ============================================================================
// 30. Edge Case: Return from Finally Block
// ============================================================================

#[test]
fn test_return_value_with_finally_side_effect() {
    expect_i32(
        "let sideEffect = 0;
         function test(): int {
             try {
                 return 42;
             } finally {
                 sideEffect = 99;
             }
         }
         let r = test();
         return r;",
        42,
    );
}

// ============================================================================
// 31. Complex Conditional Expressions
// ============================================================================

#[test]
fn test_deeply_nested_ternary() {
    expect_i32(
        "let x = 4;
         return x == 1 ? 10 : x == 2 ? 20 : x == 3 ? 30 : x == 4 ? 42 : 0;",
        42,
    );
}

// ============================================================================
// 32. Iterator-like Pattern (State Machine via Class)
// ============================================================================

#[test]
fn test_stateful_iterator_pattern() {
    expect_i32(
        "class Counter {
             current: int;
             limit: int;
             constructor(start: int, limit: int) {
                 this.current = start;
                 this.limit = limit;
             }
             hasNext(): boolean { return this.current < this.limit; }
             next(): int {
                 let val = this.current;
                 this.current = this.current + 1;
                 return val;
             }
         }
         let iter = new Counter(0, 42);
         let last = 0;
         while (iter.hasNext()) {
             last = iter.next();
         }
         return last + 1;",
        42,
    );
}

// ============================================================================
// 33. Enum-like Pattern via Constants
// ============================================================================

#[test]
fn test_enum_like_constants() {
    expect_i32(
        "const RED = 0;
         const GREEN = 1;
         const BLUE = 2;
         function colorValue(c: int): int {
             switch (c) {
                 case 0: return 10;
                 case 1: return 42;
                 case 2: return 20;
                 default: return 0;
             }
         }
         return colorValue(GREEN);",
        42,
    );
}

// ============================================================================
// 34. Complex String Building
// ============================================================================

#[test]
fn test_string_building_in_loop() {
    expect_string(
        "let result = \"\";
         let chars: string[] = [\"h\", \"e\", \"l\", \"l\", \"o\"];
         for (const c of chars) {
             result = result + c;
         }
         return result;",
        "hello",
    );
}

#[test]
fn test_string_repeat_via_loop() {
    expect_string(
        "let result = \"\";
         for (let i = 0; i < 3; i = i + 1) {
             result = result + \"ab\";
         }
         return result;",
        "ababab",
    );
}

// ============================================================================
// 35. Complex Map/Filter with Class Instances
// ============================================================================

#[test]
fn test_filter_array_of_objects() {
    expect_i32(
        "class Item {
             value: int;
             active: boolean;
             constructor(v: int, a: boolean) {
                 this.value = v;
                 this.active = a;
             }
         }
         let items: Item[] = [
             new Item(10, true),
             new Item(99, false),
             new Item(12, true),
             new Item(88, false),
             new Item(20, true)
         ];
         let sum = 0;
         for (const item of items) {
             if (item.active) {
                 sum = sum + item.value;
             }
         }
         return sum;",
        42,
    );
}

// ============================================================================
// 36. Stress: Many Nested Closures
// ============================================================================

#[test]
fn test_five_level_nested_closures() {
    expect_i32(
        "let f1 = (a: int): (b: int) => (c: int) => int => {
             return (b: int): (c: int) => int => {
                 return (c: int): int => a + b + c;
             };
         };
         return f1(10)(12)(20);",
        42,
    );
}

// ============================================================================
// 37. Try-Catch-Finally Complex Interactions
// ============================================================================

#[test]
fn test_finally_after_caught_exception() {
    expect_i32_with_builtins(
        "let x = 0;
         try {
             throw new Error(\"test\");
         } catch (e) {
             x = 30;
         } finally {
             x = x + 12;
         }
         return x;",
        42,
    );
}

#[test]
fn test_nested_try_finally() {
    expect_i32(
        "let x = 0;
         try {
             try {
                 x = 20;
             } finally {
                 x = x + 10;
             }
         } finally {
             x = x + 12;
         }
         return x;",
        42,
    );
}

// ============================================================================
// 38. Arithmetic Edge Cases
// ============================================================================

#[test]
fn test_modulo_negative() {
    // In most languages, -7 % 3 = -1
    expect_i32("return -7 % 3;", -1);
}

#[test]
fn test_integer_division_truncation() {
    // 7 / 2 should be 3 for integers
    expect_i32("return 7 / 2;", 3);
}

#[test]
fn test_negative_integer_division() {
    // -7 / 2 should be -3 (truncation toward zero)
    expect_i32("return -7 / 2;", -3);
}

#[test]
fn test_large_addition_chain() {
    expect_i32(
        "return 1+2+3+4+5+6+7+8+9+10+11+12+13+14+15+16+17+18+19+20+21+22+23+24+25+26+27+28+29+30+31+32+33+34+35+36+37+38+39+40+41+42 - 861;",
        42,
    );
}

// ============================================================================
// 39. Interface-Like Pattern via Type Alias (if supported)
// ============================================================================

#[test]
fn test_type_alias_with_class_implementing() {
    expect_i32(
        "type HasValue = { value: int };
         class Wrapper {
             value: int;
             constructor(v: int) { this.value = v; }
         }
         function extract(h: HasValue): int { return h.value; }
         return extract(new Wrapper(42));",
        42,
    );
}

// ============================================================================
// 40. Early Return Stress
// ============================================================================

#[test]
fn test_many_early_returns() {
    expect_i32(
        "function classify(x: int): int {
             if (x < 0) { return -1; }
             if (x == 0) { return 0; }
             if (x < 10) { return 1; }
             if (x < 20) { return 2; }
             if (x < 30) { return 3; }
             if (x < 40) { return 4; }
             if (x < 50) { return 42; }
             return 99;
         }
         return classify(42);",
        42,
    );
}

#[test]
fn test_early_return_from_nested_loops() {
    expect_i32(
        "function search(grid: int[][], target: int): int {
             for (const row of grid) {
                 for (const cell of row) {
                     if (cell == target) {
                         return cell;
                     }
                 }
             }
             return -1;
         }
         let g: int[][] = [[1, 2], [3, 42], [5, 6]];
         return search(g, 42);",
        42,
    );
}
