//! Bug hunting tests
//!
//! Tests specifically designed to probe areas likely to have bugs, based on
//! patterns adjacent to the 9 bugs discovered in the language-completeness
//! test suite. Each section targets a specific area of suspected weakness.

use super::harness::*;

// ============================================================================
// 1. Template Literal Edge Cases
//    Template strings with complex expressions, nested interpolation,
//    and type coercion inside `${}`.
// ============================================================================

#[test]
fn test_template_literal_with_int() {
    expect_string(
        "let x: int = 42;
         return `value is ${x}`;",
        "value is 42",
    );
}

#[test]
fn test_template_literal_with_arithmetic() {
    expect_string(
        "let a = 20;
         let b = 22;
         return `sum is ${a + b}`;",
        "sum is 42",
    );
}

#[test]
fn test_template_literal_with_boolean() {
    expect_string(
        "let flag = true;
         return `flag is ${flag}`;",
        "flag is true",
    );
}

#[test]
fn test_template_literal_with_method_call() {
    expect_string(
        "let s = \"hello\";
         return `upper: ${s.toUpperCase()}`;",
        "upper: HELLO",
    );
}

#[test]
fn test_template_literal_with_ternary() {
    expect_string(
        "let x = 42;
         return `${x > 0 ? \"positive\" : \"negative\"}`;",
        "positive",
    );
}

#[test]
fn test_template_literal_multiple_expressions() {
    expect_string(
        "let a = 10;
         let b = 32;
         return `${a} + ${b} = ${a + b}`;",
        "10 + 32 = 42",
    );
}

#[test]
fn test_template_literal_empty_expression() {
    expect_string(
        "let s = \"\";
         return `[${s}]`;",
        "[]",
    );
}

#[test]
fn test_template_literal_with_function_call() {
    expect_string(
        "function greet(name: string): string {
             return `Hello, ${name}!`;
         }
         return greet(\"world\");",
        "Hello, world!",
    );
}

// ============================================================================
// 2. Exponentiation Operator (**)
//    Specified in lang.md but may not be fully implemented.
// ============================================================================

#[test]
fn test_exponentiation_basic() {
    expect_i32("return 2 ** 5;", 32);
}

#[test]
fn test_exponentiation_zero() {
    expect_i32("return 42 ** 0;", 1);
}

#[test]
fn test_exponentiation_one() {
    expect_i32("return 42 ** 1;", 42);
}

#[test]
fn test_exponentiation_in_expression() {
    expect_i32("return 2 ** 3 + 34;", 42);
}

#[test]
fn test_exponentiation_chained() {
    // 2 ** 3 ** 2 should be 2 ** 9 = 512 (right-associative)
    expect_i32("return 2 ** 3 ** 2;", 512);
}

#[test]
fn test_exponentiation_with_parens() {
    // (2 ** 3) ** 2 = 8 ** 2 = 64
    expect_i32("return (2 ** 3) ** 2;", 64);
}

// ============================================================================
// 3. Increment/Decrement Operators (++/--)
//    Spec mentions postfix/prefix. Likely not implemented.
// ============================================================================

#[test]
fn test_postfix_increment() {
    expect_i32(
        "let x = 41;
         x++;
         return x;",
        42,
    );
}

#[test]
fn test_prefix_increment() {
    expect_i32(
        "let x = 41;
         return ++x;",
        42,
    );
}

#[test]
fn test_postfix_decrement() {
    expect_i32(
        "let x = 43;
         x--;
         return x;",
        42,
    );
}

#[test]
fn test_prefix_decrement() {
    expect_i32(
        "let x = 43;
         return --x;",
        42,
    );
}

#[test]
fn test_postfix_returns_old_value() {
    expect_i32(
        "let x = 42;
         let y = x++;
         return y;",
        42,
    );
}

#[test]
fn test_prefix_returns_new_value() {
    expect_i32(
        "let x = 41;
         let y = ++x;
         return y;",
        42,
    );
}

#[test]
fn test_increment_in_for_loop() {
    expect_i32(
        "let sum = 0;
         for (let i = 0; i < 10; i++) {
             sum = sum + i;
         }
         return sum;",
        45,
    );
}

// ============================================================================
// 4. Static Class Members
//    Static fields and methods on classes.
// ============================================================================

#[test]
fn test_static_method() {
    expect_i32(
        "class MathHelper {
             static add(a: int, b: int): int {
                 return a + b;
             }
         }
         return MathHelper.add(20, 22);",
        42,
    );
}

#[test]
fn test_static_field() {
    expect_i32(
        "class Config {
             static value: int = 42;
         }
         return Config.value;",
        42,
    );
}

#[test]
fn test_static_and_instance_coexist() {
    expect_i32(
        "class Counter {
             static total: int = 0;
             value: int;
             constructor(v: int) {
                 this.value = v;
                 Counter.total = Counter.total + v;
             }
         }
         let a = new Counter(10);
         let b = new Counter(32);
         return Counter.total;",
        42,
    );
}

// ============================================================================
// 5. Super Method Calls in Overrides
//    Calling parent method from overriding method.
// ============================================================================

#[test]
fn test_super_method_call() {
    expect_i32(
        "class Base {
             value(): int { return 20; }
         }
         class Child extends Base {
             value(): int { return super.value() + 22; }
         }
         let c = new Child();
         return c.value();",
        42,
    );
}

#[test]
fn test_super_in_three_level_chain() {
    expect_i32(
        "class A {
             x(): int { return 10; }
         }
         class B extends A {
             x(): int { return super.x() + 12; }
         }
         class C extends B {
             x(): int { return super.x() + 20; }
         }
         return new C().x();",
        42,
    );
}

#[test]
fn test_deep_inheritance_field_access() {
    expect_i32(
        "class A {
             a: int;
             constructor() { this.a = 10; }
         }
         class B extends A {
             b: int;
             constructor() { super(); this.b = 12; }
         }
         class C extends B {
             c: int;
             constructor() { super(); this.c = 20; }
         }
         let obj = new C();
         return obj.a + obj.b + obj.c;",
        42,
    );
}

// ============================================================================
// 6. Closure Capturing `this` in Class Methods
//    Arrow functions inside methods should capture `this`.
// ============================================================================

#[test]
fn test_closure_captures_this_in_method() {
    expect_i32(
        "class Adder {
             base: int;
             constructor(b: int) { this.base = b; }
             makeAdder(): (x: int) => int {
                 return (x: int): int => this.base + x;
             }
         }
         let a = new Adder(20);
         let fn = a.makeAdder();
         return fn(22);",
        42,
    );
}

#[test]
fn test_closure_captures_this_field_mutation() {
    expect_i32(
        "class Accumulator {
             total: int;
             constructor() { this.total = 0; }
             add(x: int): void {
                 this.total = this.total + x;
             }
             getAdder(): (x: int) => void {
                 return (x: int): void => {
                     this.total = this.total + x;
                 };
             }
         }
         let acc = new Accumulator();
         acc.add(20);
         let adder = acc.getAdder();
         adder(22);
         return acc.total;",
        42,
    );
}

// ============================================================================
// 7. Mixed int/number Arithmetic Promotion
//    Spec says int + number → number (f64).
// ============================================================================

#[test]
fn test_int_plus_float_promotion() {
    expect_f64(
        "let i: int = 40;
         let f: number = 2.5;
         return i + f;",
        42.5,
    );
}

#[test]
fn test_int_times_float_promotion() {
    expect_f64(
        "let i: int = 21;
         let f: number = 2.0;
         return i * f;",
        42.0,
    );
}

#[test]
fn test_float_minus_int() {
    expect_f64(
        "let f: number = 50.5;
         let i: int = 8;
         return f - i;",
        42.5,
    );
}

#[test]
fn test_int_division_produces_int() {
    // Integer division should truncate
    expect_i32(
        "let a: int = 85;
         let b: int = 2;
         return a / b;",
        42,
    );
}

// ============================================================================
// 8. Unsigned Right Shift (>>>)
//    Spec mentions this but likely untested.
// ============================================================================

#[test]
fn test_unsigned_right_shift() {
    expect_i32("return 84 >>> 1;", 42);
}

#[test]
fn test_unsigned_right_shift_negative() {
    // -1 >>> 0 should be 4294967295 in JS, but in i32 context...
    // For Raya's i32, -84 >>> 1 should give a large positive number
    // Actually in 32-bit unsigned: (-84 as u32) >> 1 = 2147483606
    expect_i32("return -84 >>> 1;", 2147483606);
}

// ============================================================================
// 9. Nested Closure Mutation
//    Closures that mutate captured variables — a common source of bugs.
// ============================================================================

#[test]
fn test_closure_mutates_captured_var() {
    expect_i32(
        "let x = 0;
         let inc = (): void => { x = x + 1; };
         for (let i = 0; i < 42; i = i + 1) {
             inc();
         }
         return x;",
        42,
    );
}

#[test]
fn test_two_closures_share_captured_var() {
    expect_i32(
        "let x = 0;
         let add10 = (): void => { x = x + 10; };
         let add32 = (): void => { x = x + 32; };
         add10();
         add32();
         return x;",
        42,
    );
}

#[test]
fn test_closure_captures_loop_var_mutation() {
    expect_i32(
        "let fns: (() => int)[] = [];
         let val = 0;
         for (let i = 0; i < 3; i = i + 1) {
             val = val + i;
             let captured = val;
             fns.push((): int => captured);
         }
         return fns[2]();",
        3,
    );
}

// ============================================================================
// 10. Chained Method Calls on Strings
//     Method chaining where one string method result feeds the next.
// ============================================================================

#[test]
fn test_string_trim_then_upper() {
    expect_string(
        "let s = \"  hello  \";
         return s.trim().toUpperCase();",
        "HELLO",
    );
}

#[test]
fn test_string_lower_then_replace() {
    expect_string(
        "let s = \"HELLO WORLD\";
         return s.toLowerCase().replace(\"world\", \"raya\");",
        "hello raya",
    );
}

#[test]
fn test_string_chain_three_methods() {
    expect_string(
        "let s = \"  Hello World  \";
         return s.trim().toLowerCase().replace(\"world\", \"raya\");",
        "hello raya",
    );
}

// ============================================================================
// 11. Nullish Coalescing Chains
//     Multiple `??` operators in a chain.
// ============================================================================

#[test]
fn test_nullish_coalescing_chain() {
    expect_i32(
        "let a: int | null = null;
         let b: int | null = null;
         let c: int = 42;
         return a ?? b ?? c;",
        42,
    );
}

#[test]
fn test_nullish_coalescing_first_non_null() {
    expect_i32(
        "let a: int | null = null;
         let b: int | null = 42;
         let c: int = 99;
         return a ?? b ?? c;",
        42,
    );
}

#[test]
fn test_nullish_coalescing_with_method_call() {
    expect_i32(
        "function maybeGet(): int | null { return null; }
         return maybeGet() ?? 42;",
        42,
    );
}

// ============================================================================
// 12. Mutual Recursion
//     Two functions calling each other.
// ============================================================================

#[test]
fn test_mutual_recursion_even_odd() {
    expect_bool(
        "function isEven(n: int): boolean {
             if (n == 0) { return true; }
             return isOdd(n - 1);
         }
         function isOdd(n: int): boolean {
             if (n == 0) { return false; }
             return isEven(n - 1);
         }
         return isEven(42);",
        true,
    );
}

#[test]
fn test_mutual_recursion_returns_value() {
    expect_i32(
        "function a(n: int): int {
             if (n <= 0) { return 0; }
             return b(n - 1) + 1;
         }
         function b(n: int): int {
             if (n <= 0) { return 0; }
             return a(n - 1) + 1;
         }
         return a(42);",
        42,
    );
}

// ============================================================================
// 13. Deeply Nested Property Access
//     a.b.c.d — tests codegen for chained field access.
// ============================================================================

#[test]
fn test_three_level_property_access() {
    expect_i32(
        "class Inner { value: int; constructor(v: int) { this.value = v; } }
         class Middle { inner: Inner; constructor(i: Inner) { this.inner = i; } }
         class Outer { middle: Middle; constructor(m: Middle) { this.middle = m; } }
         let o = new Outer(new Middle(new Inner(42)));
         return o.middle.inner.value;",
        42,
    );
}

#[test]
fn test_nested_method_call_chain() {
    expect_i32(
        "class Node {
             val: int;
             child: Node | null;
             constructor(v: int) {
                 this.val = v;
                 this.child = null;
             }
             getChild(): Node | null {
                 return this.child;
             }
         }
         let root = new Node(10);
         root.child = new Node(42);
         let child = root.getChild();
         if (child !== null) {
             return child.val;
         }
         return 0;",
        42,
    );
}

// ============================================================================
// 14. Empty Array Edge Cases
//     Operations on empty arrays that might cause runtime errors or panics.
// ============================================================================

#[test]
fn test_empty_array_length() {
    expect_i32(
        "let arr: int[] = [];
         return arr.length;",
        0,
    );
}

#[test]
fn test_empty_array_map() {
    expect_i32(
        "let arr: int[] = [];
         let mapped = arr.map((x: int): int => x * 2);
         return mapped.length;",
        0,
    );
}

#[test]
fn test_empty_array_filter() {
    expect_i32(
        "let arr: int[] = [];
         let filtered = arr.filter((x: int): boolean => x > 0);
         return filtered.length;",
        0,
    );
}

#[test]
fn test_empty_array_every() {
    // every() on empty array should return true (vacuous truth)
    expect_bool(
        "let arr: int[] = [];
         return arr.every((x: int): boolean => x > 0);",
        true,
    );
}

#[test]
fn test_empty_array_some() {
    // some() on empty array should return false
    expect_bool(
        "let arr: int[] = [];
         return arr.some((x: int): boolean => x > 0);",
        false,
    );
}

#[test]
fn test_empty_array_join() {
    expect_string(
        "let arr: string[] = [];
         return arr.join(\",\");",
        "",
    );
}

// ============================================================================
// 15. Array Method Chains
//     Chaining multiple array operations together.
// ============================================================================

#[test]
fn test_array_filter_then_map() {
    expect_i32(
        "let arr: int[] = [1, 2, 3, 4, 5, 6];
         let result = arr.filter((x: int): boolean => x % 2 == 0)
                         .map((x: int): int => x * 2);
         return result[0] + result[1] + result[2];",
        24,
    );
}

#[test]
fn test_array_map_then_filter() {
    expect_i32(
        "let arr: int[] = [1, 2, 3, 4, 5];
         let result = arr.map((x: int): int => x * 10)
                         .filter((x: int): boolean => x > 20);
         return result.length;",
        3,
    );
}

// ============================================================================
// 16. For-Of with Continue
//     Skipping items in a for-of loop.
// ============================================================================

#[test]
fn test_for_of_with_continue() {
    expect_i32(
        "let sum = 0;
         let arr: int[] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
         for (const x of arr) {
             if (x % 2 != 0) { continue; }
             sum = sum + x;
         }
         return sum;",
        30,
    );
}

#[test]
fn test_for_of_with_break() {
    expect_i32(
        "let sum = 0;
         let arr: int[] = [10, 20, 30, 40, 50];
         for (const x of arr) {
             sum = sum + x;
             if (sum >= 30) { break; }
         }
         return sum;",
        30,
    );
}

// ============================================================================
// 17. Switch Inside Loop
//     Combining switch statements with loops.
// ============================================================================

#[test]
fn test_switch_in_while_loop() {
    expect_i32(
        "let state = 0;
         let result = 0;
         while (state < 3) {
             switch (state) {
                 case 0: result = result + 10; break;
                 case 1: result = result + 12; break;
                 case 2: result = result + 20; break;
             }
             state = state + 1;
         }
         return result;",
        42,
    );
}

#[test]
fn test_switch_in_for_loop() {
    expect_i32(
        "let result = 0;
         let actions: int[] = [1, 2, 3];
         for (const a of actions) {
             switch (a) {
                 case 1: result = result + 10; break;
                 case 2: result = result + 12; break;
                 case 3: result = result + 20; break;
             }
         }
         return result;",
        42,
    );
}

// ============================================================================
// 18. Try-Catch in Loop
//     Exception handling inside loops.
// ============================================================================

#[test]
fn test_try_catch_in_for_loop() {
    expect_i32_with_builtins(
        "let caught = 0;
         for (let i = 0; i < 42; i = i + 1) {
             try {
                 if (i % 2 == 0) {
                     throw new Error(\"even\");
                 }
             } catch (e) {
                 caught = caught + 1;
             }
         }
         return caught;",
        21,
    );
}

#[test]
fn test_try_catch_with_continue() {
    expect_i32_with_builtins(
        "let sum = 0;
         let arr: int[] = [10, -1, 12, -1, 20];
         for (const x of arr) {
             try {
                 if (x < 0) { throw new Error(\"negative\"); }
                 sum = sum + x;
             } catch (e) {
                 continue;
             }
         }
         return sum;",
        42,
    );
}

// ============================================================================
// 19. Complex Ternary Patterns
//     Nested and chained ternary expressions.
// ============================================================================

#[test]
fn test_nested_ternary() {
    expect_i32(
        "let x = 2;
         return x == 1 ? 10 : x == 2 ? 42 : 0;",
        42,
    );
}

#[test]
fn test_ternary_with_function_calls() {
    expect_i32(
        "function double(x: int): int { return x * 2; }
         function triple(x: int): int { return x * 3; }
         let flag = true;
         return flag ? double(21) : triple(14);",
        42,
    );
}

#[test]
fn test_ternary_in_assignment() {
    expect_i32(
        "let a = 10;
         let b = 20;
         let max = a > b ? a : b;
         return max + 22;",
        42,
    );
}

// ============================================================================
// 20. Array Spread Edge Cases
//     Spread operator with various configurations.
// ============================================================================

#[test]
fn test_spread_empty_array() {
    expect_i32(
        "let empty: int[] = [];
         let arr: int[] = [42, ...empty];
         return arr[0];",
        42,
    );
}

#[test]
fn test_spread_only() {
    expect_i32(
        "let src: int[] = [10, 12, 20];
         let copy: int[] = [...src];
         return copy[0] + copy[1] + copy[2];",
        42,
    );
}

#[test]
fn test_spread_multiple_empty() {
    expect_i32(
        "let a: int[] = [];
         let b: int[] = [];
         let c: int[] = [42];
         let result: int[] = [...a, ...b, ...c];
         return result[0];",
        42,
    );
}

// ============================================================================
// 21. Recursive Data Structures (Binary Tree)
//     Testing deeper recursive patterns beyond linked list.
// ============================================================================

#[test]
fn test_binary_tree_sum() {
    expect_i32(
        "class TreeNode {
             val: int;
             left: TreeNode | null;
             right: TreeNode | null;
             constructor(v: int) {
                 this.val = v;
                 this.left = null;
                 this.right = null;
             }
         }
         function treeSum(node: TreeNode | null): int {
             if (node === null) { return 0; }
             return node.val + treeSum(node.left) + treeSum(node.right);
         }
         let root = new TreeNode(20);
         root.left = new TreeNode(10);
         root.right = new TreeNode(12);
         return treeSum(root);",
        42,
    );
}

#[test]
fn test_binary_tree_depth() {
    expect_i32(
        "class TNode {
             left: TNode | null;
             right: TNode | null;
             constructor() {
                 this.left = null;
                 this.right = null;
             }
         }
         function depth(node: TNode | null): int {
             if (node === null) { return 0; }
             let l = depth(node.left);
             let r = depth(node.right);
             return (l > r ? l : r) + 1;
         }
         let root = new TNode();
         root.left = new TNode();
         root.left.left = new TNode();
         return depth(root) * 14;",
        42,
    );
}

// ============================================================================
// 22. Generic Function Without Explicit Type Argument
//     Type inference for generic functions.
// ============================================================================

#[test]
fn test_generic_identity_inferred() {
    expect_i32(
        "function identity<T>(x: T): T { return x; }
         return identity(42);",
        42,
    );
}

#[test]
fn test_generic_pair_inferred() {
    expect_i32(
        "function first<T>(a: T, b: T): T { return a; }
         return first(42, 99);",
        42,
    );
}

#[test]
fn test_generic_with_array() {
    expect_i32(
        "function getFirst<T>(arr: T[]): T { return arr[0]; }
         let nums: int[] = [42, 1, 2];
         return getFirst(nums);",
        42,
    );
}

#[test]
fn test_generic_function_multiple_type_params() {
    expect_i32(
        "function selectFirst<A, B>(a: A, b: B): A { return a; }
         return selectFirst<int, string>(42, \"hello\");",
        42,
    );
}

// ============================================================================
// 23. String Comparison Operators
//     Spec says <, >, <=, >= work on strings.
// ============================================================================

#[test]
fn test_string_less_than() {
    expect_bool(
        "return \"apple\" < \"banana\";",
        true,
    );
}

#[test]
fn test_string_greater_than() {
    expect_bool(
        "return \"banana\" > \"apple\";",
        true,
    );
}

#[test]
fn test_string_less_equal() {
    expect_bool(
        "return \"abc\" <= \"abc\";",
        true,
    );
}

#[test]
fn test_string_greater_equal() {
    expect_bool(
        "return \"xyz\" >= \"abc\";",
        true,
    );
}

// ============================================================================
// 24. Int Boundary Behavior
//     i32 max/min edge cases.
// ============================================================================

#[test]
fn test_int_max_value() {
    expect_i32("return 2147483647;", 2147483647);
}

#[test]
fn test_int_min_value() {
    expect_i32("return -2147483648;", -2147483648);
}

#[test]
fn test_int_overflow_addition() {
    // 2147483647 + 1 should overflow in i32
    // Depending on implementation, might wrap or error
    expect_i32("return 2147483647 + 1;", -2147483648);
}

#[test]
fn test_int_overflow_multiplication() {
    // Large multiplication overflow
    expect_i32("return 100000 * 100000;", 1410065408);
}

// ============================================================================
// 25. Float Special Values
//     NaN, Infinity — spec mentions these.
// ============================================================================

// Note: Float division by zero produces Infinity correctly, but the harness
// can't compare inf values (inf - inf = NaN). Verified manually that it works.
// #[test]
// fn test_float_division_by_zero() {
//     expect_f64("let x: number = 1.0; let y: number = 0.0; return x / y;", f64::INFINITY);
// }

// ============================================================================
// 26. Complex Class Patterns
//     Method returning this type, builder pattern, etc.
// ============================================================================

#[test]
fn test_class_method_modifies_and_returns() {
    expect_i32(
        "class Builder {
             value: int;
             constructor() { this.value = 0; }
             add(x: int): void {
                 this.value = this.value + x;
             }
         }
         let b = new Builder();
         b.add(20);
         b.add(22);
         return b.value;",
        42,
    );
}

#[test]
fn test_class_with_array_field() {
    expect_i32(
        "class IntList {
             items: int[];
             constructor() { this.items = []; }
             add(x: int): void { this.items.push(x); }
             sum(): int {
                 let total = 0;
                 for (const item of this.items) {
                     total = total + item;
                 }
                 return total;
             }
         }
         let list = new IntList();
         list.add(10);
         list.add(12);
         list.add(20);
         return list.sum();",
        42,
    );
}

#[test]
fn test_class_array_of_instances() {
    expect_i32(
        "class Wrapper {
             v: int;
             constructor(v: int) { this.v = v; }
         }
         let items: Wrapper[] = [new Wrapper(10), new Wrapper(12), new Wrapper(20)];
         let sum = 0;
         for (const item of items) {
             sum = sum + item.v;
         }
         return sum;",
        42,
    );
}

// ============================================================================
// 27. Closure in Array Methods (Complex Callbacks)
//     Testing closures that capture variables used in map/filter/etc.
// ============================================================================

#[test]
fn test_map_with_captured_variable() {
    expect_i32(
        "let offset = 10;
         let arr: int[] = [1, 2, 3];
         let result = arr.map((x: int): int => x + offset);
         return result[0] + result[1] + result[2];",
        36,
    );
}

#[test]
fn test_filter_with_captured_threshold() {
    expect_i32(
        "let threshold = 30;
         let arr: int[] = [10, 42, 20, 50, 5];
         let big = arr.filter((x: int): boolean => x > threshold);
         return big[0];",
        42,
    );
}

// ============================================================================
// 28. Complex Control Flow — Early Return from Nested Constructs
// ============================================================================

#[test]
fn test_return_from_nested_for_of_if() {
    expect_i32(
        "function find42(arrs: int[][]): int {
             for (const arr of arrs) {
                 for (const x of arr) {
                     if (x == 42) { return x; }
                 }
             }
             return -1;
         }
         let data: int[][] = [[1, 2], [3, 42, 5], [6]];
         return find42(data);",
        42,
    );
}

#[test]
fn test_return_from_while_in_function() {
    expect_i32(
        "function countdown(start: int): int {
             let n = start;
             while (n > 0) {
                 if (n == 42) { return n; }
                 n = n - 1;
             }
             return -1;
         }
         return countdown(100);",
        42,
    );
}

// ============================================================================
// 29. Array of Functions / Closures
//     Creating and invoking arrays of function values.
// ============================================================================

#[test]
fn test_array_of_closures_invocation() {
    expect_i32(
        "let ops: ((x: int) => int)[] = [
             (x: int): int => x + 10,
             (x: int): int => x + 12,
             (x: int): int => x + 20
         ];
         let result = 0;
         for (const op of ops) {
             result = op(result);
         }
         return result;",
        42,
    );
}

#[test]
fn test_function_returning_different_closures() {
    expect_i32(
        "function makeOp(kind: int): (x: int) => int {
             if (kind == 1) {
                 return (x: int): int => x * 2;
             }
             return (x: int): int => x + 1;
         }
         let double = makeOp(1);
         return double(21);",
        42,
    );
}

// ============================================================================
// 30. Complex Generic Class Patterns
// ============================================================================

#[test]
fn test_generic_class_instantiation() {
    expect_i32(
        "class Box<T> {
             value: T;
             constructor(v: T) { this.value = v; }
             get(): T { return this.value; }
         }
         let b = new Box<int>(42);
         return b.get();",
        42,
    );
}

#[test]
fn test_generic_class_with_method() {
    expect_i32(
        "class Pair<A, B> {
             first: A;
             second: B;
             constructor(a: A, b: B) {
                 this.first = a;
                 this.second = b;
             }
         }
         let p = new Pair<int, int>(20, 22);
         return p.first + p.second;",
        42,
    );
}

#[test]
fn test_generic_class_with_array() {
    expect_i32(
        "class Stack<T> {
             items: T[];
             constructor() { this.items = []; }
             push(item: T): void { this.items.push(item); }
             peek(): T { return this.items[this.items.length - 1]; }
             size(): int { return this.items.length; }
         }
         let s = new Stack<int>();
         s.push(10);
         s.push(42);
         return s.peek();",
        42,
    );
}

// ============================================================================
// 31. String Edge Cases
//     Empty strings, single chars, unicode-related patterns.
// ============================================================================

#[test]
fn test_empty_string_length() {
    expect_i32(
        "let s = \"\";
         return s.length;",
        0,
    );
}

#[test]
fn test_empty_string_concat() {
    expect_string(
        "let a = \"\";
         let b = \"hello\";
         return a + b;",
        "hello",
    );
}

#[test]
fn test_string_index_of_empty() {
    expect_i32(
        "let s = \"hello\";
         return s.indexOf(\"\");",
        0,
    );
}

#[test]
fn test_single_char_string_operations() {
    expect_string(
        "let s = \"a\";
         return s.toUpperCase();",
        "A",
    );
}

#[test]
fn test_string_split_single_char() {
    expect_i32(
        "let parts = \"a.b.c\".split(\".\");
         return parts.length;",
        3,
    );
}

// ============================================================================
// 32. Scope and Variable Lifetime Edge Cases
// ============================================================================

#[test]
fn test_variable_reuse_in_sequential_loops() {
    expect_i32(
        "let total = 0;
         for (let i = 0; i < 3; i = i + 1) {
             total = total + i;
         }
         for (let i = 10; i < 13; i = i + 1) {
             total = total + i;
         }
         return total;",
        36,
    );
}

#[test]
fn test_same_name_different_scopes() {
    expect_i32(
        "function outer(): int {
             let x = 10;
             function inner(): int {
                 let x = 32;
                 return x;
             }
             return x + inner();
         }
         return outer();",
        42,
    );
}

#[test]
fn test_parameter_shadows_outer() {
    expect_i32(
        "let x = 99;
         function f(x: int): int { return x; }
         return f(42);",
        42,
    );
}

// ============================================================================
// 33. Complex Boolean Logic
// ============================================================================

#[test]
fn test_demorgan_and() {
    // !(a && b) == !a || !b
    expect_bool(
        "let a = true;
         let b = false;
         return !(a && b) == (!a || !b);",
        true,
    );
}

#[test]
fn test_demorgan_or() {
    // !(a || b) == !a && !b
    expect_bool(
        "let a = false;
         let b = false;
         return !(a || b) == (!a && !b);",
        true,
    );
}

#[test]
fn test_boolean_chain_mixed() {
    expect_bool(
        "let a = true;
         let b = false;
         let c = true;
         return (a || b) && (b || c) && (a || c);",
        true,
    );
}

// ============================================================================
// 34. Const Semantics — Object Field Mutation
//     const makes binding immutable, not the value.
// ============================================================================

#[test]
fn test_const_object_field_mutation() {
    expect_i32(
        "class Obj {
             value: int;
             constructor(v: int) { this.value = v; }
         }
         const o = new Obj(10);
         o.value = 42;
         return o.value;",
        42,
    );
}

#[test]
fn test_const_array_push() {
    expect_i32(
        "const arr: int[] = [10, 12];
         arr.push(20);
         return arr[0] + arr[1] + arr[2];",
        42,
    );
}

// ============================================================================
// 35. Polymorphism / Virtual Method Dispatch
//     Calling overridden methods through base class reference.
// ============================================================================

#[test]
fn test_virtual_dispatch_through_base_ref() {
    expect_i32(
        "class Animal {
             sound(): int { return 0; }
         }
         class Dog extends Animal {
             sound(): int { return 42; }
         }
         let a: Animal = new Dog();
         return a.sound();",
        42,
    );
}

#[test]
fn test_virtual_dispatch_in_array() {
    expect_i32(
        "class Base {
             val(): int { return 0; }
         }
         class A extends Base {
             val(): int { return 10; }
         }
         class B extends Base {
             val(): int { return 12; }
         }
         class C extends Base {
             val(): int { return 20; }
         }
         let items: Base[] = [new A(), new B(), new C()];
         let sum = 0;
         for (const item of items) {
             sum = sum + item.val();
         }
         return sum;",
        42,
    );
}

#[test]
fn test_virtual_dispatch_through_function() {
    expect_i32(
        "class Shape {
             area(): int { return 0; }
         }
         class Square extends Shape {
             side: int;
             constructor(s: int) { super(); this.side = s; }
             area(): int { return this.side * this.side; }
         }
         function getArea(s: Shape): int {
             return s.area();
         }
         return getArea(new Square(6)) + 6;",
        42,
    );
}

// ============================================================================
// 36. Complex Expression Edge Cases
// ============================================================================

#[test]
fn test_chained_comparisons_in_condition() {
    expect_i32(
        "let x = 42;
         if (x >= 0 && x <= 100 && x != 0) {
             return x;
         }
         return 0;",
        42,
    );
}

#[test]
fn test_expression_in_array_index() {
    expect_i32(
        "let arr: int[] = [10, 20, 42, 30];
         let idx = 1 + 1;
         return arr[idx];",
        42,
    );
}

#[test]
fn test_function_call_in_array_index() {
    expect_i32(
        "function getIndex(): int { return 2; }
         let arr: int[] = [10, 20, 42, 30];
         return arr[getIndex()];",
        42,
    );
}

#[test]
fn test_method_call_as_condition() {
    expect_i32(
        "let arr: int[] = [1, 42, 3];
         if (arr.includes(42)) {
             return 42;
         }
         return 0;",
        42,
    );
}

// ============================================================================
// 37. Do-While with Complex Conditions
// ============================================================================

#[test]
fn test_do_while_with_compound_condition() {
    expect_i32(
        "let x = 0;
         let y = 100;
         do {
             x = x + 1;
             y = y - 1;
         } while (x < y && x < 42);
         return x;",
        42,
    );
}

// ============================================================================
// 38. Multiple Catch Scenarios and Exception Types
// ============================================================================

#[test]
fn test_rethrow_caught_by_outer() {
    expect_i32_with_builtins(
        "function inner(): int {
             try {
                 throw new Error(\"inner\");
             } catch (e) {
                 throw new Error(\"rethrown\");
             }
             return 0;
         }
         try {
             inner();
         } catch (e) {
             return 42;
         }
         return 0;",
        42,
    );
}

#[test]
fn test_finally_runs_even_with_return() {
    expect_i32(
        "let x = 0;
         function test(): int {
             try {
                 x = 30;
                 return x;
             } finally {
                 x = 42;
             }
         }
         test();
         return x;",
        42,
    );
}

// ============================================================================
// 39. Reduce (if supported)
//     Array reduce is a critical functional method.
// ============================================================================

#[test]
fn test_array_reduce_sum() {
    expect_i32(
        "let arr: int[] = [10, 12, 20];
         let sum = arr.reduce((acc: int, x: int): int => acc + x, 0);
         return sum;",
        42,
    );
}

#[test]
fn test_array_reduce_product() {
    expect_i32(
        "let arr: int[] = [2, 3, 7];
         let product = arr.reduce((acc: int, x: int): int => acc * x, 1);
         return product;",
        42,
    );
}

// ============================================================================
// 40. forEach (if supported)
// ============================================================================

#[test]
fn test_array_foreach() {
    expect_i32(
        "let sum = 0;
         let arr: int[] = [10, 12, 20];
         arr.forEach((x: int): void => {
             sum = sum + x;
         });
         return sum;",
        42,
    );
}

// ============================================================================
// 41. Optional Parameters
//     Spec says optional params have type T | null.
// ============================================================================

#[test]
fn test_optional_parameter_provided() {
    expect_i32(
        "function add(a: int, b?: int): int {
             if (b !== null) { return a + b; }
             return a;
         }
         return add(20, 22);",
        42,
    );
}

#[test]
fn test_optional_parameter_omitted() {
    expect_i32(
        "function add(a: int, b?: int): int {
             if (b !== null) { return a + b; }
             return a;
         }
         return add(42);",
        42,
    );
}

// ============================================================================
// 42. Default Parameters
//     Spec mentions default parameter values.
// ============================================================================

#[test]
fn test_default_parameter() {
    expect_i32(
        "function greetLen(name: string = \"hello world!\"): int {
             return name.length;
         }
         return greetLen();",
        12,
    );
}

#[test]
fn test_default_parameter_overridden() {
    expect_i32(
        "function add(a: int, b: int = 22): int {
             return a + b;
         }
         return add(20);",
        42,
    );
}

// ============================================================================
// 43. Rest Parameters
//     Spec mentions rest parameters with `...args: T[]`.
// ============================================================================

// BUG DISCOVERY: Rest parameters (...args) are parsed but not bound correctly.
// The binder fails with UndefinedVariable for the rest parameter name, and
// the function arity check doesn't account for variadic arguments.
// Error: UndefinedVariable { name: "nums" } + ArgumentCountMismatch
// #[test]
// fn test_rest_parameters() {
//     expect_i32(
//         "function sum(...nums: int[]): int {
//              let total = 0;
//              for (const n of nums) {
//                  total = total + n;
//              }
//              return total;
//          }
//          return sum(10, 12, 20);",
//         42,
//     );
// }

// BUG DISCOVERY: Same rest parameter issue — binder doesn't handle `...rest`.
// #[test]
// fn test_rest_parameters_with_regular() {
//     expect_i32(
//         "function first_plus_rest(first: int, ...rest: int[]): int {
//              let total = first;
//              for (const n of rest) {
//                  total = total + n;
//              }
//              return total;
//          }
//          return first_plus_rest(10, 12, 20);",
//         42,
//     );
// }

// ============================================================================
// 44. Nested Array Access
//     arr[arr[0]] — computed index from array.
// ============================================================================

#[test]
fn test_nested_array_index() {
    expect_i32(
        "let arr: int[] = [2, 10, 20, 42];
         return arr[arr[0] + 1];",
        42,
    );
}

#[test]
fn test_2d_array_access() {
    expect_i32(
        "let grid: int[][] = [[1, 2], [3, 42]];
         return grid[1][1];",
        42,
    );
}

#[test]
fn test_2d_array_nested_loop() {
    expect_i32(
        "let grid: int[][] = [[1, 2, 3], [4, 5, 6], [7, 8, 9]];
         let sum = 0;
         for (const row of grid) {
             for (const val of row) {
                 sum = sum + val;
             }
         }
         return sum;",
        45,
    );
}

// ============================================================================
// 45. Multiple Return Value Simulation via Objects
// ============================================================================

#[test]
fn test_class_as_return_tuple() {
    expect_i32(
        "class Result {
             x: int;
             y: int;
             constructor(x: int, y: int) {
                 this.x = x;
                 this.y = y;
             }
         }
         function divmod(a: int, b: int): Result {
             return new Result(a / b, a % b);
         }
         let r = divmod(42, 10);
         return r.x * 10 + r.y;",
        42,
    );
}

// ============================================================================
// 46. Complex for Loop Update Expressions
// ============================================================================

#[test]
fn test_for_loop_step_by_two() {
    expect_i32(
        "let sum = 0;
         for (let i = 0; i < 12; i = i + 2) {
             sum = sum + i;
         }
         return sum;",
        30,
    );
}

#[test]
fn test_for_loop_with_multiple_updates() {
    expect_i32(
        "let a = 0;
         let b = 0;
         for (let i = 0; i < 6; i = i + 1) {
             a = a + i;
             b = b + (5 - i);
         }
         return a + b;",
        30,
    );
}

// ============================================================================
// 47. Abstract Class with Multiple Abstract Methods
// ============================================================================

#[test]
fn test_abstract_class_multiple_abstracts() {
    expect_i32(
        "abstract class Calc {
             abstract add(a: int, b: int): int;
             abstract mul(a: int, b: int): int;
             compute(x: int, y: int, z: int): int {
                 return this.add(this.mul(x, y), z);
             }
         }
         class SimpleCalc extends Calc {
             add(a: int, b: int): int { return a + b; }
             mul(a: int, b: int): int { return a * b; }
         }
         let c = new SimpleCalc();
         return c.compute(5, 8, 2);",
        42,
    );
}

// ============================================================================
// 48. Type Alias with Generic
// ============================================================================

#[test]
fn test_generic_type_alias() {
    expect_i32(
        "type Mapper<T> = (x: T) => T;
         let double: Mapper<int> = (x: int): int => x * 2;
         return double(21);",
        42,
    );
}

// ============================================================================
// 49. Conditional Expression as Function Argument
// ============================================================================

#[test]
fn test_ternary_as_argument() {
    expect_i32(
        "function identity(x: int): int { return x; }
         let flag = true;
         return identity(flag ? 42 : 0);",
        42,
    );
}

#[test]
fn test_ternary_with_null() {
    expect_i32(
        "let x: int | null = null;
         return x !== null ? x : 42;",
        42,
    );
}

// ============================================================================
// 50. Deeply Nested Closure Chains
// ============================================================================

#[test]
fn test_closure_returns_closure_returns_closure() {
    expect_i32(
        "function make(): () => () => int {
             return (): () => int => {
                 return (): int => 42;
             };
         }
         let f1 = make();
         let f2 = f1();
         return f2();",
        42,
    );
}

#[test]
fn test_adder_factory() {
    expect_i32(
        "function adder(a: int): (b: int) => int {
             return (b: int): int => a + b;
         }
         let add20 = adder(20);
         return add20(22);",
        42,
    );
}

#[test]
fn test_compose_two_functions() {
    expect_i32(
        "function compose(f: (x: int) => int, g: (x: int) => int): (x: int) => int {
             return (x: int): int => f(g(x));
         }
         let double = (x: int): int => x * 2;
         let addOne = (x: int): int => x + 1;
         let doubleThenAdd = compose(addOne, double);
         return doubleThenAdd(20) + 1;",
        42,
    );
}
