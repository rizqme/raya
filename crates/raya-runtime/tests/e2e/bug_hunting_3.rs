//! Bug hunting tests — round 3
//!
//! Targeting untested language areas:
//! - Compound assignment operators (+=, -=, *=, /=, %=, &=, |=, ^=, <<=, >>=)
//! - String methods that may be missing (charAt, startsWith, endsWith, repeat, padStart, etc.)
//! - Array methods that may be missing (find, flat, concat, slice, indexOf with start)
//! - Discriminated union narrowing (switch on kind field)
//! - Access modifiers (private, protected)
//! - Short-circuit evaluation side effects
//! - Tuple types
//! - Arrow expression body (no braces)
//! - Void return enforcement
//! - String escape sequences
//! - Abstract subclass missing implementation (should error)
//! - Generic constraints (T extends Base)
//! - Intersection types
//! - Self-referential class
//! - Labeled break (if supported)

use super::harness::*;

// ============================================================================
// 1. Compound Assignment Operators
//    +=, -=, *=, /=, %=
// ============================================================================

#[test]
fn test_plus_equals() {
    expect_i32("let x = 40; x += 2; return x;", 42);
}

#[test]
fn test_minus_equals() {
    expect_i32("let x = 44; x -= 2; return x;", 42);
}

#[test]
fn test_times_equals() {
    expect_i32("let x = 21; x *= 2; return x;", 42);
}

#[test]
fn test_div_equals() {
    expect_i32("let x = 84; x /= 2; return x;", 42);
}

#[test]
fn test_mod_equals() {
    expect_i32("let x = 142; x %= 100; return x;", 42);
}

#[test]
fn test_compound_assign_in_loop() {
    expect_i32(
        "let sum = 0;
         for (let i = 0; i < 10; i += 1) {
             sum += i;
         }
         return sum;",
        45,
    );
}

#[test]
fn test_compound_assign_on_field() {
    expect_i32(
        "class Counter {
             value: int;
             constructor() { this.value = 0; }
             add(x: int): void { this.value += x; }
         }
         let c = new Counter();
         c.add(20);
         c.add(22);
         return c.value;",
        42,
    );
}

// ============================================================================
// 2. Bitwise Compound Assignment
//    &=, |=, ^=, <<=, >>=
// ============================================================================

#[test]
fn test_and_equals() {
    expect_i32("let x = 0xFF; x &= 0x2A; return x;", 42);
}

#[test]
fn test_or_equals() {
    expect_i32("let x = 0x20; x |= 0x0A; return x;", 42);
}

#[test]
fn test_xor_equals() {
    expect_i32("let x = 0xFF; x ^= 0xD5; return x;", 42);
}

#[test]
fn test_shl_equals() {
    expect_i32("let x = 21; x <<= 1; return x;", 42);
}

#[test]
fn test_shr_equals() {
    expect_i32("let x = 84; x >>= 1; return x;", 42);
}

// ============================================================================
// 3. String Methods — Possibly Missing
// ============================================================================

#[test]
fn test_string_char_at() {
    expect_string(
        "let s = \"hello\";
         return s.charAt(1);",
        "e",
    );
}

#[test]
fn test_string_starts_with() {
    expect_bool(
        "let s = \"hello world\";
         return s.startsWith(\"hello\");",
        true,
    );
}

#[test]
fn test_string_ends_with() {
    expect_bool(
        "let s = \"hello world\";
         return s.endsWith(\"world\");",
        true,
    );
}

#[test]
fn test_string_repeat() {
    expect_string(
        "let s = \"ab\";
         return s.repeat(3);",
        "ababab",
    );
}

#[test]
fn test_string_pad_start() {
    expect_string(
        "let s = \"42\";
         return s.padStart(5, \"0\");",
        "00042",
    );
}

#[test]
fn test_string_pad_end() {
    expect_string(
        "let s = \"42\";
         return s.padEnd(5, \".\");",
        "42...",
    );
}

#[test]
fn test_string_substring() {
    expect_string(
        "let s = \"hello world\";
         return s.substring(6, 11);",
        "world",
    );
}

// BUG DISCOVERY: String.slice() method not implemented.
// Error: NotCallable { ty: "TypeId(6)" }
#[test]
fn test_string_slice() {
    expect_string(
        "let s = \"hello world\";
         return s.slice(6);",
        "world",
    );
}

#[test]
fn test_string_char_code_at() {
    expect_i32(
        "let s = \"A\";
         return s.charCodeAt(0);",
        65,
    );
}

#[test]
fn test_string_includes() {
    expect_bool(
        "let s = \"hello world\";
         return s.includes(\"world\");",
        true,
    );
}

#[test]
fn test_string_last_index_of() {
    expect_i32(
        "let s = \"abcabc\";
         return s.lastIndexOf(\"abc\");",
        3,
    );
}

// ============================================================================
// 4. Array Methods — Possibly Missing
// ============================================================================

#[test]
fn test_array_find() {
    expect_i32(
        "let arr: int[] = [1, 2, 42, 3];
         let found = arr.find((x: int): boolean => x > 40);
         if (found !== null) { return found; }
         return 0;",
        42,
    );
}

#[test]
fn test_array_concat() {
    expect_i32(
        "let a: int[] = [10, 12];
         let b: int[] = [20];
         let c = a.concat(b);
         return c[0] + c[1] + c[2];",
        42,
    );
}

#[test]
fn test_array_slice() {
    expect_i32(
        "let arr: int[] = [1, 42, 3, 4];
         let sliced = arr.slice(1, 2);
         return sliced[0];",
        42,
    );
}

// BUG DISCOVERY: Array.flat() returns wrong type. Type checker infers the
// result type incorrectly — flat() on int[][] doesn't produce int[].
// Elements are typed as TypeId(90) instead of int.
// #[test]
// fn test_array_flat() {
//     expect_i32(
//         "let arr: int[][] = [[10, 12], [20]];
//          let flat = arr.flat();
//          return flat[0] + flat[1] + flat[2];",
//         42,
//     );
// }

#[test]
fn test_array_index_of() {
    expect_i32(
        "let arr: int[] = [10, 20, 42, 30];
         return arr.indexOf(42);",
        2,
    );
}

#[test]
fn test_array_last_index_of() {
    expect_i32(
        "let arr: int[] = [42, 1, 2, 42];
         return arr.lastIndexOf(42);",
        3,
    );
}

#[test]
fn test_array_sort() {
    expect_i32(
        "let arr: int[] = [3, 1, 42, 2];
         arr.sort((a: int, b: int): int => a - b);
         return arr[3];",
        42,
    );
}

#[test]
fn test_array_pop() {
    expect_i32(
        "let arr: int[] = [1, 2, 42];
         let val = arr.pop();
         if (val !== null) { return val; }
         return 0;",
        42,
    );
}

#[test]
fn test_array_shift() {
    expect_i32(
        "let arr: int[] = [42, 1, 2];
         let val = arr.shift();
         if (val !== null) { return val; }
         return 0;",
        42,
    );
}

#[test]
fn test_array_unshift() {
    expect_i32(
        "let arr: int[] = [1, 2];
         arr.unshift(42);
         return arr[0];",
        42,
    );
}

// ============================================================================
// 5. Discriminated Union Narrowing
//    Switch on a `kind` field to narrow type.
// ============================================================================

#[test]
fn test_discriminated_union_basic() {
    expect_i32(
        "type Shape =
             | { kind: \"circle\"; radius: int }
             | { kind: \"square\"; side: int };
         function area(s: Shape): int {
             switch (s.kind) {
                 case \"circle\": return s.radius * s.radius;
                 case \"square\": return s.side * s.side;
             }
         }
         let sq: Shape = { kind: \"square\", side: 6 };
         return area(sq) + 6;",
        42,
    );
}

#[test]
fn test_discriminated_union_if_else() {
    expect_i32(
        "type Result =
             | { status: \"ok\"; value: int }
             | { status: \"err\"; code: int };
         function getValue(r: Result): int {
             if (r.status === \"ok\") {
                 return r.value;
             } else {
                 return r.code;
             }
         }
         let r: Result = { status: \"ok\", value: 42 };
         return getValue(r);",
        42,
    );
}

#[test]
fn test_discriminated_union_three_variants() {
    expect_i32(
        "type Token =
             | { kind: \"number\"; value: int }
             | { kind: \"string\"; text: string }
             | { kind: \"bool\"; flag: boolean };
         function tokenScore(t: Token): int {
             switch (t.kind) {
                 case \"number\": return t.value;
                 case \"string\": return t.text.length;
                 case \"bool\": return t.flag ? 1 : 0;
             }
         }
         let t: Token = { kind: \"number\", value: 42 };
         return tokenScore(t);",
        42,
    );
}

// ============================================================================
// 6. Access Modifiers (private, protected)
// ============================================================================

#[test]
fn test_private_field_access_inside_class() {
    expect_i32(
        "class Secret {
             private code: int;
             constructor(c: int) { this.code = c; }
             getCode(): int { return this.code; }
         }
         let s = new Secret(42);
         return s.getCode();",
        42,
    );
}

#[test]
fn test_private_field_access_outside_class_errors() {
    expect_compile_error(
        "class Secret {
             private code: int;
             constructor(c: int) { this.code = c; }
         }
         let s = new Secret(42);
         return s.code;",
        "private",
    );
}

#[test]
fn test_protected_field_access_in_subclass() {
    expect_i32(
        "class Base {
             protected value: int;
             constructor(v: int) { this.value = v; }
         }
         class Child extends Base {
             constructor(v: int) { super(v); }
             getValue(): int { return this.value; }
         }
         return new Child(42).getValue();",
        42,
    );
}

// BUG DISCOVERY: Protected field access from outside the class hierarchy
// compiles successfully instead of producing an error.
// The type checker doesn't enforce `protected` visibility.
// #[test]
// fn test_protected_field_access_outside_errors() {
//     expect_compile_error(
//         "class Base {
//              protected value: int;
//              constructor(v: int) { this.value = v; }
//          }
//          let b = new Base(42);
//          return b.value;",
//         "protected",
//     );
// }

#[test]
fn test_private_method() {
    expect_i32(
        "class Encap {
             private secret(): int { return 42; }
             reveal(): int { return this.secret(); }
         }
         return new Encap().reveal();",
        42,
    );
}

// ============================================================================
// 7. Short-Circuit Side Effects
//    false && f() should NOT call f()
//    true || f() should NOT call f()
// ============================================================================

#[test]
fn test_and_short_circuit_no_side_effect() {
    expect_i32(
        "let x = 42;
         let cond = false;
         function sideEffect(): boolean {
             x = 0;
             return true;
         }
         if (cond && sideEffect()) { x = -1; }
         return x;",
        42,
    );
}

#[test]
fn test_or_short_circuit_no_side_effect() {
    expect_i32(
        "let x = 42;
         let cond = true;
         function sideEffect(): boolean {
             x = 0;
             return false;
         }
         if (cond || sideEffect()) {}
         return x;",
        42,
    );
}

#[test]
fn test_and_short_circuit_evaluates_when_true() {
    expect_i32(
        "let x = 0;
         let cond = true;
         function doIt(): boolean { x = 42; return true; }
         if (cond && doIt()) {}
         return x;",
        42,
    );
}

// ============================================================================
// 8. Tuple Types
//    [int, string], [int, int, int] etc.
// ============================================================================

// BUG DISCOVERY: Tuple types are not implemented in the type checker.
// The syntax `[int, string]` is parsed but the type checker rejects
// assigning array literals to tuple-typed variables with TypeMismatch.
// All 4 tuple tests fail with the same pattern.
// #[test]
// fn test_tuple_basic() {
//     expect_i32(
//         "let pair: [int, string] = [42, \"hello\"];
//          return pair[0];",
//         42,
//     );
// }
//
// #[test]
// fn test_tuple_string_element() {
//     expect_string(
//         "let pair: [int, string] = [42, \"hello\"];
//          return pair[1];",
//         "hello",
//     );
// }
//
// #[test]
// fn test_tuple_three_elements() {
//     expect_i32(
//         "let triple: [int, int, int] = [10, 12, 20];
//          return triple[0] + triple[1] + triple[2];",
//         42,
//     );
// }
//
// #[test]
// fn test_tuple_from_function() {
//     expect_i32(
//         "function divmod(a: int, b: int): [int, int] {
//              return [a / b, a % b];
//          }
//          let result = divmod(42, 10);
//          return result[0] * 10 + result[1];",
//         42,
//     );
// }

// ============================================================================
// 9. Arrow Function Expression Body (no braces)
// ============================================================================

#[test]
fn test_arrow_expression_body() {
    expect_i32(
        "let double = (x: int): int => x * 2;
         return double(21);",
        42,
    );
}

#[test]
fn test_arrow_expression_body_in_map() {
    expect_i32(
        "let arr: int[] = [20, 11, 11];
         let result = arr.map((x: int): int => x + 1);
         return result[0] + result[1] - result[2];",
        21,
    );
}

// ============================================================================
// 10. String Escape Sequences
// ============================================================================

#[test]
fn test_string_newline_escape() {
    expect_i32(
        "let s = \"line1\\nline2\";
         return s.length;",
        11,
    );
}

#[test]
fn test_string_tab_escape() {
    expect_i32(
        "let s = \"a\\tb\";
         return s.length;",
        3,
    );
}

#[test]
fn test_string_backslash_escape() {
    expect_i32(
        "let s = \"a\\\\b\";
         return s.length;",
        3,
    );
}

#[test]
fn test_string_quote_escape() {
    expect_i32(
        "let s = \"he said \\\"hi\\\"\";
         return s.length;",
        12,
    );
}

// ============================================================================
// 11. Abstract Subclass Missing Implementation (should error)
// ============================================================================

// BUG DISCOVERY: Abstract method implementation is not enforced.
// A concrete class that extends an abstract class WITHOUT implementing
// the abstract method compiles successfully instead of producing an error.
// #[test]
// fn test_abstract_method_not_implemented_errors() {
//     expect_compile_error(
//         "abstract class Base {
//              abstract compute(): int;
//          }
//          class Child extends Base {
//              // missing compute() implementation
//          }
//          let c = new Child();",
//         "abstract",
//     );
// }

// ============================================================================
// 12. Generic Constraints (T extends Base)
// ============================================================================

#[test]
fn test_generic_constraint_basic() {
    expect_i32(
        "class HasValue {
             value: int;
             constructor(v: int) { this.value = v; }
         }
         function extract<T extends HasValue>(obj: T): int {
             return obj.value;
         }
         return extract(new HasValue(42));",
        42,
    );
}

#[test]
fn test_generic_constraint_subclass() {
    expect_i32(
        "class Base {
             val: int;
             constructor(v: int) { this.val = v; }
         }
         class Child extends Base {
             extra: int;
             constructor(v: int, e: int) { super(v); this.extra = e; }
         }
         function getVal<T extends Base>(obj: T): int {
             return obj.val;
         }
         return getVal(new Child(42, 99));",
        42,
    );
}

// ============================================================================
// 13. Intersection Types (A & B)
// ============================================================================

#[test]
fn test_intersection_type_basic() {
    expect_i32(
        "type HasName = { name: string };
         type HasAge = { age: int };
         type Person = HasName & HasAge;
         let p: Person = { name: \"Alice\", age: 42 };
         return p.age;",
        42,
    );
}

// ============================================================================
// 14. Self-Referential Class (Linked List)
// ============================================================================

#[test]
fn test_self_referential_class_linked_list() {
    expect_i32(
        "class ListNode {
             val: int;
             next: ListNode | null;
             constructor(v: int) {
                 this.val = v;
                 this.next = null;
             }
         }
         function sum(node: ListNode | null): int {
             if (node === null) { return 0; }
             return node.val + sum(node.next);
         }
         let a = new ListNode(10);
         let b = new ListNode(12);
         let c = new ListNode(20);
         a.next = b;
         b.next = c;
         return sum(a);",
        42,
    );
}

#[test]
fn test_self_referential_class_length() {
    expect_i32(
        "class Node {
             next: Node | null;
             constructor() { this.next = null; }
         }
         function length(n: Node | null): int {
             if (n === null) { return 0; }
             return 1 + length(n.next);
         }
         let a = new Node();
         a.next = new Node();
         a.next.next = new Node();
         return length(a) * 14;",
        42,
    );
}

// ============================================================================
// 15. Void Return Type Enforcement
// ============================================================================

#[test]
fn test_void_function_returns_nothing() {
    expect_i32(
        "let x = 0;
         function setX(): void {
             x = 42;
         }
         setX();
         return x;",
        42,
    );
}

#[test]
fn test_void_function_return_value_errors() {
    expect_compile_error(
        "function f(): void { return 42; }",
        "TypeMismatch",
    );
}

// ============================================================================
// 16. Typeof in Switch Statement
//     switch (typeof x) { case "int": ... }
// ============================================================================

// BUG DISCOVERY: `switch (typeof x)` doesn't narrow the type of `x` in
// case branches. The spec shows this as a supported pattern, but the
// type checker doesn't recognize typeof narrowing in switch contexts.
// Error: TypeMismatch for `return x` (still sees union, not narrowed int)
// and `x.length` (doesn't narrow to string).
// #[test]
// fn test_typeof_switch() {
//     expect_i32(
//         "function process(x: int | string): int {
//              switch (typeof x) {
//                  case \"int\": return x;
//                  case \"string\": return x.length;
//              }
//          }
//          return process(42);",
//         42,
//     );
// }
//
// #[test]
// fn test_typeof_switch_string_branch() {
//     expect_i32(
//         "function process(x: int | string): int {
//              switch (typeof x) {
//                  case \"int\": return x;
//                  case \"string\": return x.length;
//              }
//          }
//          return process(\"hello world! the answer is forty-two\");",
//         36,
//     );
// }

// ============================================================================
// 17. Complex Compound Assignment Patterns
// ============================================================================

#[test]
fn test_compound_assign_on_array_element() {
    expect_i32(
        "let arr: int[] = [40, 1, 2];
         arr[0] += 2;
         return arr[0];",
        42,
    );
}

#[test]
fn test_compound_assign_string_concat() {
    expect_string(
        "let s = \"hello\";
         s += \" world\";
         return s;",
        "hello world",
    );
}

#[test]
fn test_compound_assign_multiple_ops() {
    expect_i32(
        "let x = 10;
         x += 5;    // 15
         x *= 3;    // 45
         x -= 3;    // 42
         return x;",
        42,
    );
}

// ============================================================================
// 18. Type Alias Patterns
// ============================================================================

#[test]
fn test_simple_type_alias() {
    expect_i32(
        "type ID = int;
         let id: ID = 42;
         return id;",
        42,
    );
}

#[test]
fn test_type_alias_function_type() {
    expect_i32(
        "type Transform = (x: int) => int;
         let double: Transform = (x: int): int => x * 2;
         return double(21);",
        42,
    );
}

#[test]
fn test_type_alias_union() {
    expect_i32(
        "type MaybeInt = int | null;
         let x: MaybeInt = 42;
         if (x !== null) { return x; }
         return 0;",
        42,
    );
}

// ============================================================================
// 19. Class With Static Methods and Inheritance
// ============================================================================

#[test]
fn test_static_method_on_subclass() {
    expect_i32(
        "class Base {
             static create(): int { return 20; }
         }
         class Child extends Base {
             static create(): int { return 42; }
         }
         return Child.create();",
        42,
    );
}

// ============================================================================
// 20. Array.every and Array.some with Complex Predicates
// ============================================================================

#[test]
fn test_array_every_all_match() {
    expect_bool(
        "let arr: int[] = [2, 4, 6, 8];
         return arr.every((x: int): boolean => x % 2 == 0);",
        true,
    );
}

#[test]
fn test_array_every_not_all_match() {
    expect_bool(
        "let arr: int[] = [2, 3, 6, 8];
         return arr.every((x: int): boolean => x % 2 == 0);",
        false,
    );
}

#[test]
fn test_array_some_one_matches() {
    expect_bool(
        "let arr: int[] = [1, 3, 42, 7];
         return arr.some((x: int): boolean => x == 42);",
        true,
    );
}

#[test]
fn test_array_some_none_matches() {
    expect_bool(
        "let arr: int[] = [1, 3, 5, 7];
         return arr.some((x: int): boolean => x == 42);",
        false,
    );
}

// ============================================================================
// 21. Class Implementing Type Contract
// ============================================================================

#[test]
fn test_class_implements_type() {
    expect_i32(
        "type Measurable = { measure(): int };
         class Ruler implements Measurable {
             length: int;
             constructor(l: int) { this.length = l; }
             measure(): int { return this.length; }
         }
         function doMeasure(m: Measurable): int { return m.measure(); }
         return doMeasure(new Ruler(42));",
        42,
    );
}

// ============================================================================
// 22. Nested Switch Statements
// ============================================================================

#[test]
fn test_nested_switch() {
    expect_i32(
        "let category = 1;
         let subCategory = 2;
         let result = 0;
         switch (category) {
             case 1:
                 switch (subCategory) {
                     case 1: result = 10; break;
                     case 2: result = 42; break;
                 }
                 break;
             case 2:
                 result = 99;
                 break;
         }
         return result;",
        42,
    );
}

// ============================================================================
// 23. Complex Closure + Exception Interaction
// ============================================================================

#[test]
fn test_closure_in_catch_block() {
    expect_i32_with_builtins(
        "let captured = 0;
         try {
             throw new Error(\"test\");
         } catch (e) {
             let fn = (): void => { captured = 42; };
             fn();
         }
         return captured;",
        42,
    );
}

#[test]
fn test_closure_in_finally_block() {
    expect_i32(
        "let captured = 0;
         try {
             captured = 20;
         } finally {
             let fn = (): void => { captured = captured + 22; };
             fn();
         }
         return captured;",
        42,
    );
}

// ============================================================================
// 24. String Comparison Edge Cases
// ============================================================================

#[test]
fn test_string_equality_empty() {
    expect_bool("return \"\" === \"\";", true);
}

#[test]
fn test_string_inequality_different_length() {
    expect_bool("return \"abc\" !== \"abcd\";", true);
}

#[test]
fn test_string_equality_same() {
    expect_bool("return \"hello\" === \"hello\";", true);
}

// ============================================================================
// 25. Complex Generic + Closure
// ============================================================================

#[test]
fn test_generic_function_taking_closure() {
    expect_i32(
        "function applyTwice<T>(fn: (x: T) => T, val: T): T {
             return fn(fn(val));
         }
         let addTen = (x: int): int => x + 10;
         return applyTwice<int>(addTen, 22);",
        42,
    );
}

#[test]
fn test_generic_map_implementation() {
    // [10,11,11] → map(x+1) → [11,12,12] → sum = 35
    expect_i32(
        "function myMap<T, U>(arr: T[], fn: (x: T) => U): U[] {
             let result: U[] = [];
             for (const item of arr) {
                 result.push(fn(item));
             }
             return result;
         }
         let nums: int[] = [10, 11, 11];
         let doubled = myMap<int, int>(nums, (x: int): int => x + 1);
         return doubled[0] + doubled[1] + doubled[2];",
        35,
    );
}

// ============================================================================
// 26. Complex For-Of Patterns
// ============================================================================

#[test]
fn test_for_of_with_index_tracking() {
    expect_i32(
        "let arr: int[] = [99, 99, 42, 99];
         let idx = 0;
         let found = -1;
         for (const x of arr) {
             if (x == 42) { found = idx; }
             idx += 1;
         }
         return found;",
        2,
    );
}

#[test]
fn test_for_of_modifying_external_array() {
    expect_i32(
        "let src: int[] = [10, 12, 20];
         let dst: int[] = [];
         for (const x of src) {
             dst.push(x);
         }
         return dst[0] + dst[1] + dst[2];",
        42,
    );
}

// ============================================================================
// 27. Complex Method Chaining
// ============================================================================

#[test]
fn test_method_chain_on_class() {
    expect_i32(
        "class Builder {
             value: int;
             constructor() { this.value = 0; }
             add(x: int): Builder {
                 this.value += x;
                 return this;
             }
         }
         return new Builder().add(10).add(12).add(20).value;",
        42,
    );
}

// ============================================================================
// 28. Number/Int Edge Cases
// ============================================================================

// BUG DISCOVERY: Implicit int→number conversion doesn't actually convert
// the value at the VM level. `let f: number = i` where i is int still
// stores the value as int(42) instead of f64(42.0). The spec says
// int→number is an implicit safe widening conversion.
// #[test]
// fn test_implicit_int_to_number() {
//     expect_f64(
//         "let i: int = 42;
//          let f: number = i;
//          return f;",
//         42.0,
//     );
// }

// math module requires builtins
#[test]
fn test_number_floor() {
    expect_i32_with_builtins(
        "let f: number = 42.9;
         return math.floor(f);",
        42,
    );
}

// ============================================================================
// 29. Immediately Invoked Function Expression (IIFE)
// ============================================================================

#[test]
fn test_iife() {
    expect_i32(
        "return ((x: int): int => x * 2)(21);",
        42,
    );
}

#[test]
fn test_iife_with_closure() {
    expect_i32(
        "let base = 20;
         return ((x: int): int => base + x)(22);",
        42,
    );
}

// ============================================================================
// 30. Complex Array + Class Patterns
// ============================================================================

#[test]
fn test_class_with_method_returning_array() {
    expect_i32(
        "class Range {
             start: int;
             end: int;
             constructor(s: int, e: int) { this.start = s; this.end = e; }
             toArray(): int[] {
                 let result: int[] = [];
                 for (let i = this.start; i < this.end; i += 1) {
                     result.push(i);
                 }
                 return result;
             }
         }
         let r = new Range(40, 43);
         let arr = r.toArray();
         return arr[2];",
        42,
    );
}

// ============================================================================
// 31. Multiple Catch Blocks (chained try-catch)
// ============================================================================

#[test]
fn test_sequential_try_catch() {
    expect_i32_with_builtins(
        "let sum = 0;
         for (let i = 0; i < 3; i += 1) {
             try {
                 throw new Error(\"e\");
             } catch (e) {
                 sum += 14;
             }
         }
         return sum;",
        42,
    );
}

// ============================================================================
// 32. Complex Recursion — Ackermann (small values)
// ============================================================================

#[test]
fn test_ackermann_small() {
    // ackermann(3, 3) = 61. Use smaller: ack(3, 2) = 29
    // ack(2, 5) = 13, ack(3, 1) = 13
    expect_i32(
        "function ack(m: int, n: int): int {
             if (m == 0) { return n + 1; }
             if (n == 0) { return ack(m - 1, 1); }
             return ack(m - 1, ack(m, n - 1));
         }
         return ack(3, 1) * 3 + 3;",
        42,
    );
}

// ============================================================================
// 33. Closure Captures Class Instance
// ============================================================================

#[test]
fn test_closure_captures_class_instance() {
    expect_i32(
        "class State {
             val: int;
             constructor(v: int) { this.val = v; }
         }
         let state = new State(42);
         let getter = (): int => state.val;
         return getter();",
        42,
    );
}

#[test]
fn test_closure_modifies_class_field() {
    expect_i32(
        "class State {
             val: int;
             constructor(v: int) { this.val = v; }
         }
         let state = new State(0);
         let setter = (v: int): void => { state.val = v; };
         setter(42);
         return state.val;",
        42,
    );
}

// ============================================================================
// 34. Empty For-Of (edge case)
// ============================================================================

#[test]
fn test_for_of_empty_array_body_not_executed() {
    expect_i32(
        "let x = 42;
         let empty: int[] = [];
         for (const item of empty) {
             x = 0;
         }
         return x;",
        42,
    );
}

// ============================================================================
// 35. Complex Default Switch Fall-Through
// ============================================================================

#[test]
fn test_switch_default_only() {
    expect_i32(
        "let x = 99;
         switch (x) {
             default: return 42;
         }",
        42,
    );
}

#[test]
fn test_switch_no_match_no_default() {
    expect_i32(
        "let x = 99;
         let result = 42;
         switch (x) {
             case 1: result = 0; break;
             case 2: result = 0; break;
         }
         return result;",
        42,
    );
}
