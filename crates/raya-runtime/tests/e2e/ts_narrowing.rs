//! Advanced type narrowing tests
//!
//! Adapted from TypeScript conformance tests:
//!   - controlFlow/controlFlowTypeofObject.ts
//!   - controlFlow/controlFlowInstanceOfGuardPrimitives.ts
//!   - controlFlow/controlFlowTruthiness.ts
//!   - controlFlow/controlFlowGenericTypes.ts
//!   - controlFlow/dependentDestructuredVariables.ts
//!
//! Raya supports typeof (primitives) and instanceof (class hierarchy) narrowing.
//! Uses null instead of undefined.

use super::harness::*;

// ============================================================================
// 1. typeof Narrowing
//    Adapted from: controlFlowTypeofObject.ts
// ============================================================================

#[test]
fn test_typeof_string_narrowing() {
    expect_i32(
        "function process(x: string | number): number {
             if (typeof x == \"string\") {
                 return x.length;
             }
             return x;
         }
         return process(\"hello\");",
        5,
    );
}

#[test]
fn test_typeof_number_narrowing() {
    expect_i32(
        "function process(x: string | number): number {
             if (typeof x == \"string\") {
                 return x.length;
             }
             return x;
         }
         return process(42);",
        42,
    );
}

#[test]
fn test_typeof_boolean_narrowing() {
    expect_i32(
        "function toNum(x: boolean | number): number {
             if (typeof x == \"boolean\") {
                 if (x) { return 1; }
                 return 0;
             }
             return x;
         }
         return toNum(true) + toNum(41);",
        42,
    );
}

#[test]
fn test_typeof_in_else_branch() {
    expect_string(
        "function describe(x: string | number): string {
             if (typeof x == \"number\") {
                 return \"num:\" + x.toString();
             } else {
                 return \"str:\" + x;
             }
         }
         return describe(\"hello\");",
        "str:hello",
    );
}

// ============================================================================
// 2. instanceof Narrowing (class hierarchy)
//    Adapted from: controlFlowInstanceOfGuardPrimitives.ts
//    Note: instanceof works with base class reference, not union types
// ============================================================================

#[test]
fn test_instanceof_narrows_to_subclass() {
    // Using base class type variable with instanceof
    expect_i32(
        "class Animal {
             legs: number = 0;
         }
         class Dog extends Animal {
             tricks: number = 5;
             constructor() {
                 super();
                 this.legs = 4;
             }
         }
         let a: Animal = new Dog();
         if (a instanceof Dog) {
             return a.tricks;
         }
         return 0;",
        5,
    );
}

#[test]
fn test_instanceof_negative_branch() {
    // instanceof returns false for wrong subclass
    expect_i32(
        "class Animal {
             legs: number = 0;
         }
         class Dog extends Animal {
             tricks: number = 5;
             constructor() { super(); this.legs = 4; }
         }
         class Cat extends Animal {
             lives: number = 9;
             constructor() { super(); this.legs = 4; }
         }
         let a: Animal = new Cat();
         if (a instanceof Dog) {
             return 999;
         }
         return a.legs;",
        4,
    );
}

#[test]
fn test_instanceof_in_inheritance_chain() {
    expect_i32(
        "class Animal {
             legs: number;
             constructor(l: number) { this.legs = l; }
         }
         class Dog extends Animal {
             speed: number = 10;
             constructor() { super(4); }
         }
         class Bird extends Animal {
             wingspan: number = 20;
             constructor() { super(2); }
         }
         function score(a: Animal): number {
             if (a instanceof Dog) {
                 return a.speed;
             }
             if (a instanceof Bird) {
                 return a.wingspan;
             }
             return 0;
         }
         return score(new Dog()) + score(new Bird());",
        30,
    );
}

// ============================================================================
// 3. Null Narrowing (Raya uses null instead of undefined)
// ============================================================================

#[test]
fn test_null_check_narrowing() {
    expect_i32(
        "function safeLen(s: string | null): number {
             if (s == null) {
                 return 0;
             }
             return s.length;
         }
         return safeLen(\"hello\") + safeLen(null);",
        5,
    );
}

#[test]
fn test_null_check_not_null() {
    expect_i32(
        "function process(x: number | null): number {
             if (x != null) {
                 return x * 2;
             }
             return -1;
         }
         return process(21);",
        42,
    );
}

// ============================================================================
// 4. Truthiness-Based Narrowing
//    Adapted from: controlFlowTruthiness.ts
//    Status: Truthiness narrowing for string|null not yet supported
// ============================================================================

#[test]
fn test_truthiness_string_narrowing() {
    expect_i32(
        "function process(s: string | null): number {
             if (s) {
                 return s.length;
             }
             return 0;
         }
         return process(\"hello\");",
        5,
    );
}

#[test]
fn test_truthiness_null_falsy() {
    expect_i32(
        "function process(s: string | null): number {
             if (s) {
                 return s.length;
             }
             return 0;
         }
         return process(null);",
        0,
    );
}

// ============================================================================
// 5. Chained Narrowing
// ============================================================================

#[test]
fn test_chained_typeof_narrowing() {
    expect_i32(
        "function classify(x: string | number | boolean): number {
             if (typeof x == \"string\") {
                 return 1;
             }
             if (typeof x == \"number\") {
                 return 2;
             }
             return 3;
         }
         return classify(\"a\") + classify(10) + classify(true);",
        6,
    );
}

#[test]
fn test_narrowing_with_early_return() {
    expect_i32(
        "function process(x: number | null): number {
             if (x == null) {
                 return 0;
             }
             return x + 1;
         }
         return process(41);",
        42,
    );
}

// ============================================================================
// 6. Narrowing in Loops
// ============================================================================

#[test]
fn test_typeof_narrowing_in_array_loop() {
    expect_i32(
        "function sumNumbers(items: (string | number)[]): number {
             let total = 0;
             for (let i = 0; i < items.length; i = i + 1) {
                 let item = items[i];
                 if (typeof item == \"number\") {
                     total = total + item;
                 }
             }
             return total;
         }
         let mixed: (string | number)[] = [10, \"skip\", 20, \"ignore\", 12];
         return sumNumbers(mixed);",
        42,
    );
}
