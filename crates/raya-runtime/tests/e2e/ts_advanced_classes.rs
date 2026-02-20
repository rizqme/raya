//! Advanced class pattern tests
//!
//! Adapted from TypeScript conformance tests:
//!   - classes/constructorDeclarations/constructorParameters/constructorParameterProperties.ts
//!   - classes/constructorDeclarations/superCalls/derivedClassParameterProperties.ts
//!   - classes/members/accessibility/classPropertyAsPrivate.ts
//!   - classes/members/accessibility/protectedClassPropertyAccessibleWithinClass.ts
//!   - classes/members/inheritanceAndOverriding/derivedClassOverridesPublicMembers.ts
//!
//! Tests constructor parameter properties, access modifiers, and method override semantics.

use super::harness::*;

// ============================================================================
// 1. Constructor Parameter Properties
//    Adapted from: constructorParameterProperties.ts
//    Status: Parser does not support `public` modifier in constructor params
// ============================================================================

#[test]
fn test_constructor_parameter_property_public() {
    expect_i32(
        "class Point {
             constructor(public x: number, public y: number) {}
         }
         let p = new Point(20, 22);
         return p.x + p.y;",
        42,
    );
}

#[test]
fn test_constructor_parameter_property_mixed() {
    expect_string(
        "class User {
             constructor(public name: string, public age: number) {}
         }
         let u = new User(\"Alice\", 30);
         return u.name;",
        "Alice",
    );
}

#[test]
fn test_constructor_param_props_with_body() {
    expect_i32(
        "class Counter {
             total: number;
             constructor(public start: number, public step: number) {
                 this.total = start;
             }
             advance(): number {
                 this.total = this.total + this.step;
                 return this.total;
             }
         }
         let c = new Counter(40, 1);
         c.advance();
         return c.advance();",
        42,
    );
}

// ============================================================================
// 2. Constructor Parameter Properties with Inheritance
//    Adapted from: derivedClassParameterProperties.ts
// ============================================================================

#[test]
fn test_param_props_with_inheritance() {
    expect_i32(
        "class Base {
             constructor(public value: number) {}
         }
         class Derived extends Base {
             constructor(value: number, public bonus: number) {
                 super(value);
             }
         }
         let d = new Derived(40, 2);
         return d.value + d.bonus;",
        42,
    );
}

// ============================================================================
// 3. Private Member Access
//    Adapted from: classPropertyAsPrivate.ts
// ============================================================================

#[test]
fn test_private_field_access_within_class() {
    expect_i32(
        "class Secret {
             private value: number;
             constructor(v: number) { this.value = v; }
             reveal(): number { return this.value; }
         }
         let s = new Secret(42);
         return s.reveal();",
        42,
    );
}

#[test]
fn test_private_method_access_within_class() {
    expect_i32(
        "class Calculator {
             private square(x: number): number { return x * x; }
             compute(x: number): number { return this.square(x) + 6; }
         }
         let c = new Calculator();
         return c.compute(6);",
        42,
    );
}

// ============================================================================
// 4. Protected Member Access
//    Adapted from: protectedClassPropertyAccessibleWithinClass.ts
// ============================================================================

#[test]
fn test_protected_field_access_in_subclass() {
    expect_i32(
        "class Base {
             protected secret: number;
             constructor(s: number) { this.secret = s; }
         }
         class Derived extends Base {
             constructor(s: number) { super(s); }
             getSecret(): number { return this.secret; }
         }
         let d = new Derived(42);
         return d.getSecret();",
        42,
    );
}

#[test]
fn test_protected_method_access_in_subclass() {
    expect_i32(
        "class Base {
             protected compute(): number { return 21; }
         }
         class Derived extends Base {
             result(): number { return this.compute() * 2; }
         }
         let d = new Derived();
         return d.result();",
        42,
    );
}

// ============================================================================
// 5. Method Override Patterns
//    Adapted from: derivedClassOverridesPublicMembers.ts
// ============================================================================

#[test]
fn test_method_override_basic() {
    expect_string(
        "class Animal {
             speak(): string { return \"...\"; }
         }
         class Dog extends Animal {
             speak(): string { return \"Woof\"; }
         }
         let d = new Dog();
         return d.speak();",
        "Woof",
    );
}

#[test]
fn test_method_override_calls_super() {
    expect_string(
        "class Base {
             greet(): string { return \"Hello\"; }
         }
         class Derived extends Base {
             greet(): string { return super.greet() + \" World\"; }
         }
         let d = new Derived();
         return d.greet();",
        "Hello World",
    );
}

#[test]
fn test_method_override_polymorphism() {
    expect_i32(
        "class Shape {
             area(): number { return 0; }
         }
         class Circle extends Shape {
             radius: number;
             constructor(r: number) {
                 super();
                 this.radius = r;
             }
             area(): number { return this.radius * this.radius * 3; }
         }
         let s: Shape = new Circle(10);
         return s.area();",
        300,
    );
}

// ============================================================================
// 6. Static Members
// ============================================================================

#[test]
fn test_static_field() {
    expect_i32(
        "class Config {
             static maxRetries: number = 42;
         }
         return Config.maxRetries;",
        42,
    );
}

#[test]
fn test_static_method() {
    expect_i32(
        "class MathHelper {
             static double(x: number): number { return x * 2; }
         }
         return MathHelper.double(21);",
        42,
    );
}

#[test]
fn test_static_and_instance_separation() {
    expect_i32(
        "class Counter {
             static total: number = 0;
             value: number;
             constructor(v: number) {
                 this.value = v;
                 Counter.total = Counter.total + v;
             }
         }
         let a = new Counter(20);
         let b = new Counter(22);
         return Counter.total;",
        42,
    );
}
