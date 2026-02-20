//! Abstract class tests
//!
//! Adapted from TypeScript conformance tests:
//!   - classes/classDeclarations/classAbstractKeyword/classAbstractDeclarations.ts
//!   - classes/classDeclarations/classAbstractKeyword/classAbstractExtends.ts
//!   - classes/classDeclarations/classAbstractKeyword/classAbstractGeneric.ts
//!   - classes/classDeclarations/classAbstractKeyword/classAbstractMethodWithImplementation.ts
//!   - classes/classDeclarations/classAbstractKeyword/classAbstractUsingAbstractMethods1.ts
//!
//! Raya supports abstract classes with abstract methods that subclasses must implement.

use super::harness::*;

// ============================================================================
// 1. Abstract Class Cannot Be Instantiated
//    Adapted from: classAbstractDeclarations.ts
// ============================================================================

#[test]
fn test_abstract_class_instantiation_error() {
    expect_compile_error(
        "abstract class Shape {
             abstract area(): number;
         }
         let s = new Shape();
         return 0;",
        "AbstractClassInstantiation",
    );
}

// ============================================================================
// 2. Abstract Class with Concrete Subclass
//    Adapted from: classAbstractExtends.ts
// ============================================================================

#[test]
fn test_abstract_concrete_subclass() {
    expect_i32(
        "abstract class Shape {
             abstract area(): number;
         }
         class Circle extends Shape {
             radius: number;
             constructor(r: number) {
                 super();
                 this.radius = r;
             }
             area(): number { return this.radius * this.radius * 3; }
         }
         let c = new Circle(10);
         return c.area();",
        300,
    );
}

#[test]

fn test_abstract_with_concrete_method() {
    expect_string(
        "abstract class Animal {
             name: string;
             constructor(name: string) {
                 this.name = name;
             }
             abstract sound(): string;
             describe(): string {
                 return this.name + \" says \" + this.sound();
             }
         }
         class Dog extends Animal {
             constructor() { super(\"Dog\"); }
             sound(): string { return \"Woof\"; }
         }
         let d = new Dog();
         return d.describe();",
        "Dog says Woof",
    );
}

#[test]

fn test_abstract_with_fields() {
    expect_i32(
        "abstract class Counter {
             count: number = 0;
             abstract step(): number;
             advance(): number {
                 this.count = this.count + this.step();
                 return this.count;
             }
         }
         class DoubleCounter extends Counter {
             step(): number { return 2; }
         }
         let dc = new DoubleCounter();
         dc.advance();
         dc.advance();
         return dc.count;",
        4,
    );
}

// ============================================================================
// 3. Multi-Level Abstract Inheritance
//    Adapted from: classAbstractUsingAbstractMethods1.ts
// ============================================================================

#[test]
fn test_abstract_multi_level_inheritance() {
    expect_i32(
        "abstract class Base {
             abstract value(): number;
         }
         abstract class Middle extends Base {
             multiplier(): number { return 2; }
         }
         class Concrete extends Middle {
             value(): number { return 21 * this.multiplier(); }
         }
         let c = new Concrete();
         return c.value();",
        42,
    );
}

#[test]
fn test_abstract_chain_override() {
    expect_i32(
        "abstract class A {
             abstract compute(): number;
             bonus(): number { return 0; }
         }
         class B extends A {
             compute(): number { return 40; }
             bonus(): number { return 2; }
         }
         let b = new B();
         return b.compute() + b.bonus();",
        42,
    );
}

// ============================================================================
// 4. Abstract Class with Generic Types
//    Adapted from: classAbstractGeneric.ts
// ============================================================================

#[test]
fn test_abstract_generic_class() {
    expect_i32(
        "abstract class Repository<T> {
             abstract find(id: number): T;
         }
         class NumberRepo extends Repository<number> {
             find(id: number): number { return id * 2; }
         }
         let repo = new NumberRepo();
         return repo.find(21);",
        42,
    );
}

#[test]

fn test_abstract_generic_with_concrete_method() {
    expect_string(
        "abstract class Transformer<T> {
             abstract transform(input: T): string;
             process(input: T): string {
                 return \"Result: \" + this.transform(input);
             }
         }
         class NumToStr extends Transformer<number> {
             transform(input: number): string {
                 return input.toString();
             }
         }
         let t = new NumToStr();
         return t.process(42);",
        "Result: 42",
    );
}

// ============================================================================
// 5. Abstract Class Polymorphism
// ============================================================================

#[test]
fn test_abstract_polymorphic_call() {
    expect_i32(
        "abstract class Shape {
             abstract area(): number;
         }
         class Rect extends Shape {
             w: number;
             h: number;
             constructor(w: number, h: number) {
                 super();
                 this.w = w;
                 this.h = h;
             }
             area(): number { return this.w * this.h; }
         }
         class Square extends Shape {
             side: number;
             constructor(s: number) {
                 super();
                 this.side = s;
             }
             area(): number { return this.side * this.side; }
         }
         function totalArea(shapes: Shape[]): number {
             let sum = 0;
             for (let i = 0; i < shapes.length; i = i + 1) {
                 sum = sum + shapes[i].area();
             }
             return sum;
         }
         let shapes: Shape[] = [new Rect(3, 4), new Square(5)];
         return totalArea(shapes);",
        37,
    );
}

// ============================================================================
// 6. Abstract Class with Constructor Parameters
// ============================================================================

#[test]
fn test_abstract_constructor_params() {
    expect_string(
        "abstract class Named {
             name: string;
             constructor(name: string) {
                 this.name = name;
             }
             abstract greet(): string;
         }
         class Greeter extends Named {
             constructor(name: string) { super(name); }
             greet(): string { return \"Hello, \" + this.name; }
         }
         let g = new Greeter(\"World\");
         return g.greet();",
        "Hello, World",
    );
}
