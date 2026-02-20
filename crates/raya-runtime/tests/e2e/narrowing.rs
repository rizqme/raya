//! Control flow narrowing and type guard tests adapted from typescript-go.
//!
//! These tests verify that the Raya compiler and runtime correctly handle
//! type narrowing through control flow analysis, typeof checks, null checks,
//! and discriminated union pattern matching.
//!
//! Adapted from:
//!   - circularControlFlowNarrowingWithCurrentElement01.ts
//!   - switchExhaustiveNarrowing.ts
//!   - freshObjectLiteralSubtype.ts
//!   - implicitEmptyObjectType.ts
//!   - missingDiscriminants.ts

use super::harness::*;

// ============================================================================
// 1. Typeof Narrowing in Conditionals
//    Adapted from: typescript-go/testdata/tests/cases/compiler/implicitEmptyObjectType.ts
//    Tests that typeof checks correctly narrow union types at runtime
// ============================================================================

#[test]
fn test_typeof_narrows_number_from_union() {
    // typeof check narrows number | string to number in the matching branch
    expect_i32(
        "function extract(val: number | string): number {
             if (typeof val == \"number\") {
                 return val + 1;
             }
             return 0;
         }
         return extract(41);",
        42,
    );
}

#[test]
fn test_typeof_narrows_string_from_union() {
    // After typeof checks string, else branch should narrow to number
    expect_i32(
        "function extract(val: number | string): number {
             if (typeof val == \"string\") {
                 return 0;
             }
             return val;
         }
         return extract(42);",
        42,
    );
}

#[test]
fn test_typeof_narrows_boolean() {
    expect_bool(
        "function isBool(val: number | boolean): boolean {
             if (typeof val == \"boolean\") {
                 return val;
             }
             return false;
         }
         return isBool(true);",
        true,
    );
}

#[test]
fn test_typeof_in_else_branch() {
    // When typeof matches string, the explicit else branch narrows to number
    expect_i32(
        "function process(val: number | string): number {
             if (typeof val == \"string\") {
                 return -1;
             } else {
                 return val * 2;
             }
         }
         return process(21);",
        42,
    );
}

// ============================================================================
// 2. Null Narrowing
//    Tests null checks to narrow nullable types
// ============================================================================

#[test]
fn test_null_check_narrows_nullable() {
    expect_i32(
        "function safeGet(val: number | null): number {
             if (val != null) {
                 return val;
             }
             return -1;
         }
         return safeGet(42);",
        42,
    );
}

#[test]
fn test_null_check_null_case() {
    expect_i32(
        "function safeGet(val: number | null): number {
             if (val != null) {
                 return val;
             }
             return -1;
         }
         return safeGet(null);",
        -1,
    );
}

#[test]
fn test_null_check_equality() {
    // After `if (val == null) { return; }`, val should be narrowed to number
    expect_i32(
        "function handle(val: number | null): number {
             if (val == null) {
                 return 0;
             }
             return val + 10;
         }
         return handle(32);",
        42,
    );
}

#[test]
fn test_nullish_coalescing_as_narrowing() {
    expect_i32(
        "function getOrDefault(val: number | null): number {
             return val ?? 99;
         }
         return getOrDefault(42);",
        42,
    );
}

#[test]
fn test_nullish_coalescing_null_path() {
    expect_i32(
        "function getOrDefault(val: number | null): number {
             return val ?? 99;
         }
         return getOrDefault(null);",
        99,
    );
}

// ============================================================================
// 3. Control Flow Narrowing in Loops
//    Adapted from: typescript-go/testdata/tests/cases/compiler/
//    circularControlFlowNarrowingWithCurrentElement01.ts
//    Tests narrowing with linked-list traversal patterns
// ============================================================================

#[test]
fn test_null_narrowing_in_while_loop() {
    // Linked-list style traversal with null narrowing
    expect_i32(
        "class Node {
             value: number;
             next: Node | null;
             constructor(v: number) {
                 this.value = v;
                 this.next = null;
             }
         }
         let a = new Node(1);
         let b = new Node(2);
         let c = new Node(3);
         a.next = b;
         b.next = c;

         let sum: number = 0;
         let current: Node | null = a;
         while (current != null) {
             sum = sum + current.value;
             current = current.next;
         }
         return sum;",
        6,
    );
}

#[test]
fn test_null_narrowing_linked_list_length() {
    expect_i32(
        "class Node {
             value: number;
             next: Node | null;
             constructor(v: number) {
                 this.value = v;
                 this.next = null;
             }
         }
         let a = new Node(10);
         let b = new Node(20);
         let c = new Node(30);
         let d = new Node(40);
         a.next = b;
         b.next = c;
         c.next = d;

         let count: number = 0;
         let current: Node | null = a;
         while (current != null) {
             count = count + 1;
             current = current.next;
         }
         return count;",
        4,
    );
}

#[test]
fn test_null_narrowing_find_in_list() {
    expect_bool(
        "class Node {
             value: number;
             next: Node | null;
             constructor(v: number) {
                 this.value = v;
                 this.next = null;
             }
         }
         let a = new Node(1);
         let b = new Node(2);
         let c = new Node(3);
         a.next = b;
         b.next = c;

         function find(head: Node | null, target: number): boolean {
             let current: Node | null = head;
             while (current != null) {
                 if (current.value == target) {
                     return true;
                 }
                 current = current.next;
             }
             return false;
         }
         return find(a, 2);",
        true,
    );
}

#[test]
fn test_null_narrowing_find_not_found() {
    expect_bool(
        "class Node {
             value: number;
             next: Node | null;
             constructor(v: number) {
                 this.value = v;
                 this.next = null;
             }
         }
         let a = new Node(1);
         let b = new Node(2);
         a.next = b;

         function find(head: Node | null, target: number): boolean {
             let current: Node | null = head;
             while (current != null) {
                 if (current.value == target) {
                     return true;
                 }
                 current = current.next;
             }
             return false;
         }
         return find(a, 99);",
        false,
    );
}

// ============================================================================
// 4. Narrowing with Early Returns
//    Adapted from: reachabilityChecks patterns
//    Tests that early returns correctly narrow the remaining code path
// ============================================================================

#[test]
fn test_early_return_narrows_remaining() {
    expect_i32(
        "function process(val: number | null): number {
             if (val == null) {
                 return -1;
             }
             // After the null return, val is narrowed to number
             return val * 2;
         }
         return process(21);",
        42,
    );
}

#[test]
fn test_early_return_chain() {
    // Multiple early returns narrowing progressively (no union types here)
    expect_i32(
        "function classify(x: number): number {
             if (x < 0) {
                 return -1;
             }
             if (x == 0) {
                 return 0;
             }
             return 1;
         }
         return classify(42);",
        1,
    );
}

#[test]
fn test_early_return_negative() {
    expect_i32(
        "function classify(x: number): number {
             if (x < 0) {
                 return -1;
             }
             if (x == 0) {
                 return 0;
             }
             return 1;
         }
         return classify(-5);",
        -1,
    );
}

// ============================================================================
// 5. Switch Statement Exhaustiveness
//    Adapted from: typescript-go/testdata/tests/cases/compiler/
//    exhaustiveSwitchStatementsGeneric1.ts, switchExhaustiveNarrowing.ts
// ============================================================================

#[test]
fn test_switch_exhaustive_all_cases() {
    expect_i32(
        "function toNum(s: string): number {
             switch (s) {
                 case \"one\": return 1;
                 case \"two\": return 2;
                 default: return 0;
             }
         }
         return toNum(\"two\");",
        2,
    );
}

#[test]
fn test_switch_default_branch() {
    expect_i32(
        "function toNum(s: string): number {
             switch (s) {
                 case \"one\": return 1;
                 case \"two\": return 2;
                 default: return -1;
             }
         }
         return toNum(\"other\");",
        -1,
    );
}

#[test]
fn test_switch_with_break() {
    expect_i32(
        "let x: string = \"b\";
         let result: number = 0;
         switch (x) {
             case \"a\":
                 result = 1;
                 break;
             case \"b\":
                 result = 2;
                 break;
             case \"c\":
                 result = 3;
                 break;
         }
         return result;",
        2,
    );
}

#[test]
fn test_switch_fall_through() {
    // Switch without break falls through to next case
    expect_i32(
        "let x: string = \"a\";
         let result: number = 0;
         switch (x) {
             case \"a\":
                 result = result + 1;
             case \"b\":
                 result = result + 10;
                 break;
             case \"c\":
                 result = result + 100;
                 break;
         }
         return result;",
        11,
    );
}

// ============================================================================
// 6. Instanceof Narrowing
// ============================================================================

#[test]
fn test_instanceof_narrows_to_derived() {
    // instanceof check correctly identifies derived class
    // (using distinct field names to avoid field shadowing issues)
    expect_i32(
        "class Animal {
             legs: number = 4;
         }
         class Bird extends Animal {
             wingSpan: number = 2;
         }
         let a: Animal = new Bird();
         if (a instanceof Bird) {
             return a.wingSpan;
         }
         return 0;",
        2,
    );
}

#[test]
fn test_instanceof_negative_case() {
    // instanceof returns false for unrelated derived class
    expect_i32(
        "class Animal {
             legs: number = 4;
         }
         class Bird extends Animal {
             wingSpan: number = 10;
         }
         class Fish extends Animal {
             finCount: number = 2;
         }
         let a: Animal = new Fish();
         if (a instanceof Bird) {
             return 999;
         }
         return a.legs;",
        4,
    );
}

// ============================================================================
// 7. Complex Narrowing Patterns
// ============================================================================

#[test]
fn test_narrowing_with_boolean_flag() {
    expect_i32(
        "function firstPositive(a: number, b: number, c: number): number {
             let found: boolean = false;
             let result: number = -1;
             if (a > 0 && !found) {
                 result = a;
                 found = true;
             }
             if (b > 0 && !found) {
                 result = b;
                 found = true;
             }
             if (c > 0 && !found) {
                 result = c;
                 found = true;
             }
             return result;
         }
         return firstPositive(-1, -2, 42);",
        42,
    );
}

#[test]
fn test_narrowing_nested_conditions() {
    expect_i32(
        "function nested(a: number | null, b: number | null): number {
             if (a != null) {
                 if (b != null) {
                     return a + b;
                 }
                 return a;
             }
             if (b != null) {
                 return b;
             }
             return -1;
         }
         return nested(20, 22);",
        42,
    );
}

#[test]
fn test_narrowing_nested_null_a_only() {
    expect_i32(
        "function nested(a: number | null, b: number | null): number {
             if (a != null) {
                 if (b != null) {
                     return a + b;
                 }
                 return a;
             }
             if (b != null) {
                 return b;
             }
             return -1;
         }
         return nested(42, null);",
        42,
    );
}

#[test]
fn test_narrowing_nested_both_null() {
    expect_i32(
        "function nested(a: number | null, b: number | null): number {
             if (a != null) {
                 if (b != null) {
                     return a + b;
                 }
                 return a;
             }
             if (b != null) {
                 return b;
             }
             return -1;
         }
         return nested(null, null);",
        -1,
    );
}

// ============================================================================
// 8. Typeof with Multiple Types
// ============================================================================

#[test]
fn test_typeof_chain_three_types() {
    expect_i32(
        "function classify(val: number | string | boolean): number {
             if (typeof val == \"number\") {
                 return 1;
             }
             if (typeof val == \"string\") {
                 return 2;
             }
             return 3;
         }
         return classify(true);",
        3,
    );
}

#[test]
fn test_typeof_chain_number_path() {
    expect_i32(
        "function classify(val: number | string | boolean): number {
             if (typeof val == \"number\") {
                 return 1;
             }
             if (typeof val == \"string\") {
                 return 2;
             }
             return 3;
         }
         return classify(42);",
        1,
    );
}
