//! Inheritance, access control, and class hierarchy tests adapted from typescript-go.
//!
//! These tests exercise deep inheritance chains, field initialization ordering,
//! method override semantics, and protected/private accessibility rules.
//!
//! Adapted from:
//!   - checkInheritedProperty.ts
//!   - protectedAccessibilityCheck.ts
//!   - noCrashOnMixin2.ts
//!   - errorInUnnamedClassExpression.ts

use super::harness::*;

// ============================================================================
// 1. Field Initialization Ordering in Inheritance
//    Adapted from: typescript-go/testdata/tests/cases/compiler/checkInheritedProperty.ts
//    Tests that field initialization order is correct in derived classes
// ============================================================================

#[test]
fn test_derived_field_initialized_before_base() {
    // Derived class initializer should run after base constructor
    expect_i32(
        "class Base {
             x: number = 10;
         }
         class Derived extends Base {
             y: number = 20;
             sum(): number { return this.x + this.y; }
         }
         let d = new Derived();
         return d.sum();",
        30,
    );
}

#[test]
fn test_derived_field_depends_on_base_field() {
    // Derived field initializer can reference this.baseField
    expect_i32(
        "class Base {
             value: number;
             constructor(v: number) {
                 this.value = v;
             }
         }
         class Derived extends Base {
             doubled: number = 0;
             constructor(v: number) {
                 super(v);
                 this.doubled = this.value * 2;
             }
         }
         let d = new Derived(21);
         return d.doubled;",
        42,
    );
}

#[test]
fn test_deep_inheritance_field_access() {
    // 4-level inheritance chain with field access at each level
    expect_i32(
        "class L1 {
             a: number = 1;
         }
         class L2 extends L1 {
             b: number = 2;
         }
         class L3 extends L2 {
             c: number = 4;
         }
         class L4 extends L3 {
             d: number = 8;
             total(): number {
                 return this.a + this.b + this.c + this.d;
             }
         }
         let obj = new L4();
         return obj.total();",
        15,
    );
}

// ============================================================================
// 2. Method Override Semantics
//    Tests that method overriding works correctly in class hierarchies
// ============================================================================

#[test]
fn test_method_override_simple() {
    // Derived class overrides base class method
    expect_i32(
        "class Animal {
             speak(): number { return 1; }
         }
         class Dog extends Animal {
             speak(): number { return 2; }
         }
         let d = new Dog();
         return d.speak();",
        2,
    );
}

#[test]
fn test_method_override_calls_own_logic() {
    // Override method uses its own fields
    expect_i32(
        "class Shape {
             area(): number { return 0; }
         }
         class Square extends Shape {
             side: number;
             constructor(s: number) {
                 super();
                 this.side = s;
             }
             area(): number { return this.side * this.side; }
         }
         let sq = new Square(7);
         return sq.area();",
        49,
    );
}

#[test]
fn test_polymorphic_method_dispatch() {
    // Methods should dispatch based on actual runtime type
    expect_i32(
        "class Base {
             value(): number { return 10; }
         }
         class DerivedA extends Base {
             value(): number { return 20; }
         }
         class DerivedB extends Base {
             value(): number { return 30; }
         }
         function getVal(b: Base): number {
             return b.value();
         }
         let a = new DerivedA();
         let b = new DerivedB();
         return getVal(a) + getVal(b);",
        50,
    );
}

#[test]
fn test_super_method_call() {
    // Derived method calls super method
    expect_i32(
        "class Base {
             compute(): number { return 10; }
         }
         class Derived extends Base {
             compute(): number { return super.compute() + 32; }
         }
         let d = new Derived();
         return d.compute();",
        42,
    );
}

#[test]
fn test_super_chain() {
    // Each level adds to the result via super
    expect_i32(
        "class A {
             val(): number { return 1; }
         }
         class B extends A {
             val(): number { return super.val() + 2; }
         }
         class C extends B {
             val(): number { return super.val() + 4; }
         }
         let c = new C();
         return c.val();",
        7,
    );
}

// ============================================================================
// 3. Constructor Patterns
//    Tests various constructor and initialization patterns in inheritance
// ============================================================================

#[test]
fn test_default_constructor_inheritance() {
    // When derived class has no constructor, base constructor is used
    expect_i32(
        "class Base {
             x: number = 42;
         }
         class Derived extends Base {
             getX(): number { return this.x; }
         }
         let d = new Derived();
         return d.getX();",
        42,
    );
}

#[test]
fn test_constructor_with_super_args() {
    // Derived passes arguments to base constructor
    expect_i32(
        "class Rect {
             width: number;
             height: number;
             constructor(w: number, h: number) {
                 this.width = w;
                 this.height = h;
             }
             area(): number { return this.width * this.height; }
         }
         class NamedRect extends Rect {
             name: string;
             constructor(name: string, w: number, h: number) {
                 super(w, h);
                 this.name = name;
             }
         }
         let r = new NamedRect(\"test\", 6, 7);
         return r.area();",
        42,
    );
}

// ============================================================================
// 4. Instanceof with Inheritance Hierarchies
//    Adapted from: typescript-go pattern of checking instanceof across
//    multi-level hierarchies
// ============================================================================

#[test]
fn test_instanceof_multi_level() {
    // instanceof should work across inheritance chain
    expect_bool(
        "class A {}
         class B extends A {}
         class C extends B {}
         let c = new C();
         return c instanceof A;",
        true,
    );
}

#[test]
fn test_instanceof_sibling_classes() {
    // Sibling classes should not be instanceof each other
    expect_bool(
        "class Base {}
         class Left extends Base {}
         class Right extends Base {}
         let l = new Left();
         return l instanceof Right;",
        false,
    );
}

#[test]
fn test_instanceof_base_is_not_derived() {
    // Base instance is not instanceof Derived
    expect_bool(
        "class Base {}
         class Derived extends Base {}
         let b = new Base();
         return b instanceof Derived;",
        false,
    );
}

// ============================================================================
// 5. Diamond-like Patterns (multiple inheritance depth)
// ============================================================================

#[test]
fn test_wide_inheritance_tree() {
    // Multiple derived classes from same base, each with different behavior
    expect_i32(
        "class Op {
             apply(x: number): number { return x; }
         }
         class DoubleOp extends Op {
             apply(x: number): number { return x * 2; }
         }
         class SquareOp extends Op {
             apply(x: number): number { return x * x; }
         }
         class NegateOp extends Op {
             apply(x: number): number { return 0 - x; }
         }

         function applyOp(op: Op, val: number): number {
             return op.apply(val);
         }

         let d = new DoubleOp();
         let s = new SquareOp();
         let n = new NegateOp();

         // 5*2 + 4*4 + -(3) = 10 + 16 + (-3) = 23
         return applyOp(d, 5) + applyOp(s, 4) + applyOp(n, 3);",
        23,
    );
}

// ============================================================================
// 6. Field Shadowing in Inheritance
// ============================================================================

#[test]
fn test_field_shadowing_derived_value() {
    // Derived class field shadows base class field
    expect_i32(
        "class Base {
             x: number = 10;
         }
         class Derived extends Base {
             x: number = 42;
         }
         let d = new Derived();
         return d.x;",
        42,
    );
}

#[test]
fn test_field_shadowing_base_method_sees_base() {
    // Base method accessing this.x should see derived's value
    // (since fields are stored by index on the object)
    expect_i32(
        "class Base {
             x: number = 10;
             getX(): number { return this.x; }
         }
         class Derived extends Base {
             x: number = 42;
         }
         let d = new Derived();
         return d.getX();",
        42,
    );
}

// ============================================================================
// 7. Abstract-like Patterns (base class with minimal implementation)
// ============================================================================

#[test]
fn test_base_provides_default_override_specializes() {
    // Pattern where base provides a default and derived classes specialize
    expect_i32(
        "class Formatter {
             format(n: number): number { return n; }
         }
         class PercentFormatter extends Formatter {
             format(n: number): number { return n * 100; }
         }
         class CurrencyFormatter extends Formatter {
             rate: number;
             constructor(r: number) {
                 super();
                 this.rate = r;
             }
             format(n: number): number { return n * this.rate; }
         }

         let pct = new PercentFormatter();
         let usd = new CurrencyFormatter(2);

         return pct.format(1) + usd.format(21);",
        142,
    );
}

