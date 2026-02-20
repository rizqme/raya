//! Generic type system tests
//!
//! Adapted from TypeScript conformance tests:
//!   - types/typeParameters/typeArgumentLists/typeParameterAsTypeParameterConstraint.ts
//!   - types/typeParameters/typeArgumentLists/instantiateGenericClassWithZeroTypeArguments.ts
//!   - types/typeRelationships/typeInference/genericCallWithObjectTypeArgs2.ts
//!   - types/typeRelationships/typeInference/genericCallWithConstraintsTypeArgumentInference2.ts
//!   - types/typeRelationships/assignmentCompatibility/assignmentCompatWithGenericCallSignatures.ts
//!
//! Raya uses monomorphization (generics specialized at compile time).

use super::harness::*;

// ============================================================================
// 1. Generic Function Basics
//    Adapted from: typeParameterAsTypeParameterConstraint.ts
// ============================================================================

#[test]
fn test_generic_function_single_param() {
    expect_i32(
        "function identity<T>(x: T): T {
             return x;
         }
         return identity<number>(42);",
        42,
    );
}

#[test]
fn test_generic_function_single_param_string() {
    expect_string(
        "function identity<T>(x: T): T {
             return x;
         }
         return identity<string>(\"hello\");",
        "hello",
    );
}

#[test]
fn test_generic_function_two_params() {
    expect_i32(
        "function first<A, B>(a: A, b: B): A {
             return a;
         }
         return first<number, string>(42, \"ignored\");",
        42,
    );
}

#[test]
fn test_generic_function_two_params_second() {
    expect_string(
        "function second<A, B>(a: A, b: B): B {
             return b;
         }
         return second<number, string>(0, \"result\");",
        "result",
    );
}

#[test]
fn test_generic_function_with_array_param() {
    expect_i32(
        "function firstElement<T>(arr: T[]): T {
             return arr[0];
         }
         let nums: number[] = [42, 10, 20];
         return firstElement<number>(nums);",
        42,
    );
}

#[test]
fn test_generic_function_array_length() {
    expect_i32(
        "function len<T>(arr: T[]): number {
             return arr.length;
         }
         let items: string[] = [\"a\", \"b\", \"c\"];
         return len<string>(items);",
        3,
    );
}

// ============================================================================
// 2. Generic Class Patterns
//    Adapted from: instantiateGenericClassWithZeroTypeArguments.ts
// ============================================================================

#[test]
fn test_generic_class_number() {
    expect_i32(
        "class Box<T> {
             value: T;
             constructor(v: T) { this.value = v; }
             get(): T { return this.value; }
         }
         let b = new Box<number>(42);
         return b.get();",
        42,
    );
}

#[test]
fn test_generic_class_string() {
    expect_string(
        "class Box<T> {
             value: T;
             constructor(v: T) { this.value = v; }
             get(): T { return this.value; }
         }
         let b = new Box<string>(\"world\");
         return b.get();",
        "world",
    );
}

#[test]
fn test_generic_class_with_method() {
    expect_bool(
        "class Container<T> {
             items: T[];
             constructor() { this.items = []; }
             add(item: T): void { this.items.push(item); }
             isEmpty(): boolean { return this.items.length == 0; }
         }
         let c = new Container<number>();
         return c.isEmpty();",
        true,
    );
}

#[test]
fn test_generic_class_after_add() {
    expect_i32(
        "class Container<T> {
             items: T[];
             constructor() { this.items = []; }
             add(item: T): void { this.items.push(item); }
             count(): number { return this.items.length; }
         }
         let c = new Container<number>();
         c.add(1);
         c.add(2);
         c.add(3);
         return c.count();",
        3,
    );
}

#[test]
fn test_generic_class_two_params() {
    expect_i32(
        "class Pair<A, B> {
             first: A;
             second: B;
             constructor(a: A, b: B) {
                 this.first = a;
                 this.second = b;
             }
         }
         let p = new Pair<number, string>(42, \"hello\");
         return p.first;",
        42,
    );
}

// ============================================================================
// 3. Generic Class Inheritance
//    Adapted from: genericCallWithObjectTypeArgs2.ts
// ============================================================================

#[test]
fn test_generic_class_extends() {
    expect_i32(
        "class Base<T> {
             value: T;
             constructor(v: T) { this.value = v; }
         }
         class Derived<T> extends Base<T> {
             extra: number;
             constructor(v: T, e: number) {
                 super(v);
                 this.extra = e;
             }
         }
         let d = new Derived<number>(40, 2);
         return d.value + d.extra;",
        42,
    );
}

#[test]
#[ignore = "generic class specialization in extends not yet supported"]
fn test_generic_class_extends_specialized() {
    expect_string(
        "class Base<T> {
             value: T;
             constructor(v: T) { this.value = v; }
             get(): T { return this.value; }
         }
         class StringBox extends Base<string> {
             constructor(s: string) { super(s); }
             upper(): string { return this.value.toUpperCase(); }
         }
         let sb = new StringBox(\"hello\");
         return sb.upper();",
        "HELLO",
    );
}

// ============================================================================
// 4. Generic Functions with Multiple Instantiations
//    Tests monomorphization - same function with different types
// ============================================================================

#[test]
fn test_generic_multiple_instantiations() {
    expect_i32(
        "function wrap<T>(x: T): T {
             return x;
         }
         let a = wrap<number>(40);
         let b = wrap<number>(2);
         return a + b;",
        42,
    );
}

#[test]
fn test_generic_class_multiple_instantiations() {
    expect_i32(
        "class Holder<T> {
             val: T;
             constructor(v: T) { this.val = v; }
         }
         let numH = new Holder<number>(42);
         let strH = new Holder<string>(\"hello\");
         return numH.val;",
        42,
    );
}

// ============================================================================
// 5. Generic Constraints
//    Adapted from: genericCallWithConstraintsTypeArgumentInference2.ts
//    Status: Generic constraints not yet supported
// ============================================================================

#[test]
fn test_generic_constraint_extends() {
    expect_i32(
        "type HasLength = { length: number; };
         function getLength<T extends HasLength>(x: T): number {
             return x.length;
         }
         let arr: number[] = [1, 2, 3];
         return getLength<number[]>(arr);",
        3,
    );
}

#[test]
fn test_generic_constraint_string() {
    expect_i32(
        "type HasLength = { length: number; };
         function getLength<T extends HasLength>(x: T): number {
             return x.length;
         }
         return getLength<string>(\"hello\");",
        5,
    );
}

#[test]
fn test_generic_constraint_class() {
    expect_i32(
        "class Animal {
             legs: number;
             constructor(l: number) { this.legs = l; }
         }
         class Dog extends Animal {
             constructor() { super(4); }
         }
         function countLegs<T extends Animal>(a: T): number {
             return a.legs;
         }
         let d = new Dog();
         return countLegs<Dog>(d);",
        4,
    );
}

// ============================================================================
// 6. Generic Method in Class
// ============================================================================

#[test]
fn test_generic_method_on_nongeneric_class() {
    expect_i32(
        "class Util {
             wrap<T>(x: T): T { return x; }
         }
         let u = new Util();
         return u.wrap<number>(42);",
        42,
    );
}

#[test]
fn test_generic_class_with_generic_method() {
    expect_string(
        "class Store<T> {
             items: T[];
             constructor() { this.items = []; }
             add(item: T): void { this.items.push(item); }
             transform<U>(fn: (item: T) => U): U[] {
                 let result: U[] = [];
                 for (let i = 0; i < this.items.length; i = i + 1) {
                     result.push(fn(this.items[i]));
                 }
                 return result;
             }
         }
         let store = new Store<number>();
         store.add(1);
         store.add(2);
         store.add(3);
         function toStr(n: number): string { return n.toString(); }
         let strs = store.transform<string>(toStr);
         return strs[0];",
        "1",
    );
}
