//! Intersection type tests
//!
//! Adapted from TypeScript conformance tests:
//!   - types/ (intersection type patterns)
//!   - interfaces/interfaceDeclarations/interfaceWithMultipleBaseTypes.ts
//!
//! Raya supports intersection types (`A & B`) to combine multiple type shapes.
//! However, object literal assignment to intersection types is not yet implemented.

use super::harness::*;

// ============================================================================
// 1. Basic Intersection Types
// ============================================================================

#[test]
#[ignore = "object literal assignment to intersection type not yet implemented"]
fn test_intersection_two_object_types() {
    expect_i32(
        "type HasX = { x: number };
         type HasY = { y: number };
         type Point = HasX & HasY;
         let p: Point = { x: 20, y: 22 };
         return p.x + p.y;",
        42,
    );
}

#[test]
#[ignore = "object literal assignment to intersection type not yet implemented"]
fn test_intersection_three_types() {
    expect_i32(
        "type A = { a: number };
         type B = { b: number };
         type C = { c: number };
         type ABC = A & B & C;
         let obj: ABC = { a: 10, b: 20, c: 12 };
         return obj.a + obj.b + obj.c;",
        42,
    );
}

// ============================================================================
// 2. Intersection with Methods
// ============================================================================

#[test]
#[ignore = "class implements intersection type not yet implemented"]
fn test_intersection_with_methods() {
    expect_string(
        "type Printable = {
             print(): string;
         };
         type Serializable = {
             serialize(): string;
         };
         type Document = Printable & Serializable;
         class Report implements Document {
             data: string;
             constructor(d: string) { this.data = d; }
             print(): string { return \"Print: \" + this.data; }
             serialize(): string { return \"JSON: \" + this.data; }
         }
         let r: Document = new Report(\"test\");
         return r.print();",
        "Print: test",
    );
}

// ============================================================================
// 3. Intersection Extending Base Type
// ============================================================================

#[test]
#[ignore = "object literal assignment to intersection type not yet implemented"]
fn test_intersection_extend_base() {
    expect_i32(
        "type Base = {
             id: number;
         };
         type Extended = Base & {
             name: string;
             score: number;
         };
         let e: Extended = { id: 1, name: \"test\", score: 42 };
         return e.score;",
        42,
    );
}

#[test]
#[ignore = "object literal assignment to intersection type not yet implemented"]
fn test_intersection_as_function_param() {
    expect_string(
        "type Named = { name: string };
         type Aged = { age: number };
         function greet(person: Named & Aged): string {
             return person.name + \" is \" + person.age.toString();
         }
         return greet({ name: \"Alice\", age: 30 });",
        "Alice is 30",
    );
}

// ============================================================================
// 4. Intersection with Class Implements
// ============================================================================

#[test]
#[ignore = "class implements multiple types not yet implemented"]
fn test_intersection_class_implements() {
    expect_i32(
        "type Readable = {
             read(): number;
         };
         type Writable = {
             write(v: number): void;
         };
         class Buffer implements Readable, Writable {
             data: number = 0;
             read(): number { return this.data; }
             write(v: number): void { this.data = v; }
         }
         let buf = new Buffer();
         buf.write(42);
         return buf.read();",
        42,
    );
}
