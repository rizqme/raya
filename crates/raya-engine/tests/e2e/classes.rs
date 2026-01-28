//! Phase 9: Class tests
//!
//! Tests for class declarations, constructors, methods, and fields.

use super::harness::*;

// ============================================================================
// Simple Class Declarations
// ============================================================================

#[test]
fn test_class_empty() {
    expect_i32(
        "class Point {}
         let p = new Point();
         return 42;",
        42,
    );
}

// Test class with field but no access - to isolate if initialization is the problem
#[test]
fn test_class_with_field_no_access() {
    expect_i32(
        "class Counter {
             value: number = 0;
         }
         let c = new Counter();
         return 42;",
        42,
    );
}

// Test field access assign to variable
#[test]
fn test_class_field_to_variable() {
    expect_i32(
        "class Counter {
             value: number = 42;
         }
         let c = new Counter();
         let v = c.value;
         return v;",
        42,
    );
}

// Debug test for field access - prints bytecode
#[test]
fn test_class_field_access_debug() {
    let source = "class Counter {
             value: number = 42;
         }
         let c = new Counter();
         return c.value;";

    let debug_output = super::harness::debug_compile(source);
    println!("\n=== Debug Output ===\n{}", debug_output);

    // Try to run it
    match super::harness::compile_and_run(source) {
        Ok(v) => println!("Result: {:?}", v.as_i32()),
        Err(e) => println!("Error: {}", e),
    }
}

#[test]
fn test_class_with_field() {
    expect_i32(
        "class Counter {
             value: number = 0;
         }
         let c = new Counter();
         return c.value;",
        0,
    );
}

#[test]
fn test_class_field_initialized() {
    expect_i32(
        "class Counter {
             value: number = 42;
         }
         let c = new Counter();
         return c.value;",
        42,
    );
}

// ============================================================================
// Capturing Outer Variables in Class
// ============================================================================

#[test]
fn test_class_field_captures_outer_variable() {
    expect_i32(
        "let outer = 10;
         class Foo {
             value: number = outer;
         }
         let f = new Foo();
         return f.value;",
        10,
    );
}

#[test]
fn test_class_field_captures_outer_expression() {
    expect_i32(
        "let x = 5;
         let y = 7;
         class Foo {
             sum: number = x + y;
         }
         let f = new Foo();
         return f.sum;",
        12,
    );
}

#[test]
fn test_class_multiple_fields_first() {
    expect_i32(
        "let a = 10;
         let b = 20;
         class Pair {
             first: number = a;
             second: number = b;
         }
         let p = new Pair();
         return p.first;",
        10,
    );
}

#[test]
fn test_class_multiple_fields_second() {
    expect_i32(
        "let a = 10;
         let b = 20;
         class Pair {
             first: number = a;
             second: number = b;
         }
         let p = new Pair();
         return p.second;",
        20,
    );
}

// ============================================================================
// Constructors
// ============================================================================

#[test]
fn test_class_constructor_no_params() {
    expect_i32(
        "class Box {
             value: number;
             constructor() {
                 this.value = 100;
             }
         }
         let b = new Box();
         return b.value;",
        100,
    );
}

#[test]
fn test_class_constructor_with_params() {
    expect_i32(
        "class Point {
             x: number;
             y: number;
             constructor(x: number, y: number) {
                 this.x = x;
                 this.y = y;
             }
         }
         let p = new Point(10, 20);
         return p.x + p.y;",
        30,
    );
}

#[test]
fn test_class_constructor_default_values() {
    expect_i32(
        "class Config {
             debug: boolean;
             timeout: number;
             constructor(debug: boolean = false, timeout: number = 1000) {
                 this.debug = debug;
                 this.timeout = timeout;
             }
         }
         let c = new Config();
         return c.timeout;",
        1000,
    );
}

// ============================================================================
// Methods
// ============================================================================

#[test]
fn test_class_method_simple() {
    expect_i32(
        "class Calculator {
             add(a: number, b: number): number {
                 return a + b;
             }
         }
         let calc = new Calculator();
         return calc.add(10, 32);",
        42,
    );
}

#[test]
fn test_class_method_using_field() {
    expect_i32(
        "class Counter {
             value: number = 0;
             increment(): number {
                 this.value = this.value + 1;
                 return this.value;
             }
         }
         let c = new Counter();
         c.increment();
         c.increment();
         return c.value;",
        2,
    );
}

#[test]
fn test_class_method_chaining() {
    expect_i32(
        "class Builder {
             value: number = 0;
             add(n: number): Builder {
                 this.value = this.value + n;
                 return this;
             }
         }
         let b = new Builder();
         b.add(10).add(20).add(12);
         return b.value;",
        42,
    );
}

// ============================================================================
// Visibility Modifiers
// ============================================================================

#[test]
fn test_class_public_field() {
    expect_i32(
        "class Data {
             public value: number = 42;
         }
         let d = new Data();
         return d.value;",
        42,
    );
}

#[test]
fn test_class_private_field_access() {
    // This should compile and work (accessing from within class)
    expect_i32(
        "class Secret {
             private code: number = 42;
             getCode(): number {
                 return this.code;
             }
         }
         let s = new Secret();
         return s.getCode();",
        42,
    );
}

// ============================================================================
// Static Members
// ============================================================================

#[test]
fn test_class_static_field() {
    expect_i32(
        "class Math {
             static PI: number = 3;
         }
         return Math.PI;",
        3,
    );
}

#[test]
fn test_class_static_method() {
    expect_i32(
        "class Utils {
             static double(x: number): number {
                 return x * 2;
             }
         }
         return Utils.double(21);",
        42,
    );
}

// ============================================================================
// Inheritance
// ============================================================================

#[test]
fn test_class_extends() {
    expect_i32(
        "class Animal {
             legs: number = 4;
         }
         class Dog extends Animal {
             bark(): number {
                 return 1;
             }
         }
         let d = new Dog();
         return d.legs;",
        4,
    );
}

#[test]
fn test_class_override_method() {
    expect_i32(
        "class Shape {
             area(): number {
                 return 0;
             }
         }
         class Square extends Shape {
             size: number;
             constructor(size: number) {
                 super();
                 this.size = size;
             }
             area(): number {
                 return this.size * this.size;
             }
         }
         let s = new Square(5);
         return s.area();",
        25,
    );
}

#[test]
fn test_class_super_call() {
    expect_i32(
        "class Parent {
             value: number = 10;
             getValue(): number {
                 return this.value;
             }
         }
         class Child extends Parent {
             getValue(): number {
                 return super.getValue() * 2;
             }
         }
         let c = new Child();
         return c.getValue();",
        20,
    );
}

// ============================================================================
// This Reference
// ============================================================================

#[test]
fn test_class_this_in_method() {
    expect_i32(
        "class Self {
             value: number = 42;
             getSelf(): number {
                 return this.value;
             }
         }
         let s = new Self();
         return s.getSelf();",
        42,
    );
}

#[test]
fn test_class_this_assignment() {
    expect_i32(
        "class Mutable {
             value: number = 0;
             setValue(v: number): void {
                 this.value = v;
             }
         }
         let m = new Mutable();
         m.setValue(42);
         return m.value;",
        42,
    );
}

// ============================================================================
// Type Operators: instanceof and as
// ============================================================================

#[test]
fn test_instanceof_same_class() {
    expect_i32(
        "class Animal {}
         let a = new Animal();
         if (a instanceof Animal) {
             return 1;
         }
         return 0;",
        1,
    );
}

#[test]
fn test_instanceof_inheritance() {
    expect_i32(
        "class Animal {}
         class Dog extends Animal {}
         let d = new Dog();
         if (d instanceof Animal) {
             return 1;
         }
         return 0;",
        1,
    );
}

#[test]
fn test_instanceof_returns_false() {
    expect_i32(
        "class Cat {}
         class Dog {}
         let c = new Cat();
         if (c instanceof Dog) {
             return 1;
         }
         return 0;",
        0,
    );
}

#[test]
fn test_cast_basic() {
    expect_i32(
        "class Animal {
             age: number = 5;
         }
         class Dog extends Animal {
             name: string = \"Rex\";
         }
         let d = new Dog();
         let a = d as Animal;
         return a.age;",
        5,
    );
}

// ============================================================================
// instanceof with Generics
// ============================================================================

#[test]
fn test_instanceof_with_generics() {
    expect_i32(
        "class Container<T> {
             value: T;
             constructor(v: T) {
                 this.value = v;
             }
         }
         let c = new Container<number>(42);
         if (c instanceof Container) {
             return 1;
         }
         return 0;",
        1,
    );
}

#[test]
fn test_instanceof_with_generics_inheritance() {
    expect_i32(
        "class Base<T> {
             value: T;
             constructor(v: T) {
                 this.value = v;
             }
         }
         class Derived<T> extends Base<T> {
             constructor(v: T) {
                 super(v);
             }
         }
         let d = new Derived<number>(42);
         if (d instanceof Base) {
             return 1;
         }
         return 0;",
        1,
    );
}

// ============================================================================
// Cast Error Handling
// ============================================================================

#[test]
fn test_cast_invalid_throws() {
    // Casting to an unrelated class should throw Type error
    super::harness::expect_runtime_error(
        "class Cat {
             meow(): number { return 1; }
         }
         class Dog {
             bark(): number { return 2; }
         }
         let c = new Cat();
         let d = c as Dog;
         return d.bark();",
        "Cannot cast",
    );
}

#[test]
fn test_cast_upcast_always_succeeds() {
    // Upcasting (child to parent) should always succeed
    expect_i32(
        "class Animal {
             legs: number = 4;
         }
         class Dog extends Animal {
             name: string = \"Rex\";
         }
         let d = new Dog();
         let a = d as Animal;
         return a.legs;",
        4,
    );
}

