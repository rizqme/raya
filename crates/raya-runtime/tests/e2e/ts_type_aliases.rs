//! Type alias and structural typing tests
//!
//! Adapted from TypeScript conformance tests:
//!   - interfaces/interfaceDeclarations/interfaceWithPropertyOfEveryType.ts
//!   - interfaces/interfaceDeclarations/interfaceWithOptionalProperty.ts
//!   - interfaces/interfaceDeclarations/interfaceWithMultipleBaseTypes.ts
//!   - interfaces/interfacesExtendingClasses/interfaceExtendingClass.ts
//!
//! Raya uses `type` aliases instead of `interface` (interfaces are banned).
//! Classes can `implements` type aliases for structural contracts.

use super::harness::*;

// ============================================================================
// 1. Basic Type Alias Object Shapes
//    Adapted from: interfaceWithPropertyOfEveryType.ts
//    Status: Object literal ↔ type alias assignment not yet supported
// ============================================================================

#[test]
fn test_type_alias_basic_object_shape() {
    expect_i32(
        "type Point = {
             x: number;
             y: number;
         };
         let p: Point = { x: 10, y: 32 };
         return p.x + p.y;",
        42,
    );
}

#[test]
fn test_type_alias_reordered_object_literal_access() {
    expect_i32(
        "type Point = {
             x: number;
             y: number;
         };
         let p: Point = { y: 32, x: 10 };
         return p.x + p.y;",
        42,
    );
}

#[test]
fn test_type_alias_with_string_property() {
    expect_string(
        "type Named = {
             name: string;
         };
         let n: Named = { name: \"hello\" };
         return n.name;",
        "hello",
    );
}

#[test]
fn test_type_alias_with_boolean_property() {
    expect_bool(
        "type Flags = {
             active: boolean;
             visible: boolean;
         };
         let f: Flags = { active: true, visible: false };
         return f.active;",
        true,
    );
}

#[test]
fn test_type_alias_with_nested_object() {
    expect_i32(
        "type Inner = {
             value: number;
         };
         type Outer = {
             inner: Inner;
             extra: number;
         };
         let o: Outer = { inner: { value: 40 }, extra: 2 };
         return o.inner.value + o.extra;",
        42,
    );
}

// ============================================================================
// 2. Type Alias with Method Signatures
//    Adapted from: interfaceWithCallSignature.ts
// ============================================================================

#[test]
fn test_type_alias_with_method_signature() {
    expect_i32(
        "type Computable = {
             compute(a: number, b: number): number;
         };
         class Adder {
             compute(a: number, b: number): number {
                 return a + b;
             }
         }
         let c: Computable = new Adder();
         return c.compute(20, 22);",
        42,
    );
}

#[test]
fn test_type_alias_function_type() {
    expect_i32(
        "type Transform = (x: number) => number;
         function doubler(x: number): number { return x * 2; }
         let t: Transform = doubler;
         return t(21);",
        42,
    );
}

// ============================================================================
// 3. Class Implements Type Alias
//    Adapted from: interfaceWithPropertyOfEveryType.ts
// ============================================================================

#[test]
fn test_class_implements_type_alias() {
    expect_i32(
        "type Measurable = {
             length(): number;
         };
         class StringWrapper implements Measurable {
             data: string;
             constructor(s: string) { this.data = s; }
             length(): number { return this.data.length; }
         }
         let sw = new StringWrapper(\"hello\");
         return sw.length();",
        5,
    );
}

#[test]
fn test_class_implements_type_with_fields_and_methods() {
    expect_i32(
        "type Entity = {
             id: number;
             name: string;
             getId(): number;
         };
         class User implements Entity {
             id: number;
             name: string;
             constructor(id: number, name: string) {
                 this.id = id;
                 this.name = name;
             }
             getId(): number { return this.id; }
         }
         let u = new User(42, \"Alice\");
         return u.getId();",
        42,
    );
}

#[test]
fn test_class_implements_multiple_types() {
    expect_string(
        "type Named = {
             name: string;
             getName(): string;
         };
         type Aged = {
             age: number;
             getAge(): number;
         };
         class Person implements Named, Aged {
             name: string;
             age: number;
             constructor(name: string, age: number) {
                 this.name = name;
                 this.age = age;
             }
             getName(): string { return this.name; }
             getAge(): number { return this.age; }
         }
         let p = new Person(\"Alice\", 30);
         return p.getName();",
        "Alice",
    );
}

// ============================================================================
// 4. Generic Type Aliases
// ============================================================================

#[test]
fn test_generic_type_alias_object() {
    expect_i32(
        "type Container<T> = {
             value: T;
         };
         let c: Container<number> = { value: 42 };
         return c.value;",
        42,
    );
}

#[test]
fn test_generic_type_alias_with_method() {
    expect_i32(
        "type Wrapper<T> = {
             get(): T;
         };
         class NumberWrapper implements Wrapper<number> {
             val: number;
             constructor(v: number) { this.val = v; }
             get(): number { return this.val; }
         }
         let w = new NumberWrapper(42);
         return w.get();",
        42,
    );
}

// ============================================================================
// 5. Type Alias Assignability
// ============================================================================

#[test]
fn test_type_alias_structural_compatibility() {
    expect_i32(
        "type HasX = {
             x: number;
         };
         let obj: HasX = { x: 42 };
         return obj.x;",
        42,
    );
}

#[test]
fn test_type_alias_as_parameter_type() {
    expect_i32(
        "type Pair = {
             first: number;
             second: number;
         };
         function sum(p: Pair): number {
             return p.first + p.second;
         }
         return sum({ first: 20, second: 22 });",
        42,
    );
}

#[test]
fn test_type_alias_as_return_type() {
    expect_i32(
        "type Result = {
             value: number;
             ok: boolean;
         };
         function makeResult(v: number): Result {
             return { value: v, ok: true };
         }
         let r = makeResult(42);
         return r.value;",
        42,
    );
}

#[test]
fn test_structural_multi_view_projection_preserves_slot_mapping() {
    expect_i32(
        "let full = { a: 1, b: 2, c: 3 };
         let ab = full as { a: number; b: number };
         let bc = full as { b: number; c: number };
         return ab.a + bc.b + bc.c;",
        6,
    );
}

#[test]
fn test_structural_spread_uses_projected_shape_slots() {
    expect_i32(
        "class Box {
             c: number = 3;
             b: number = 2;
         }
         let view = new Box() as { b: number; c: number };
         let copy = { ...view };
         return copy.b + copy.c;",
        5,
    );
}

#[test]
fn test_structural_class_projection_method_and_fields() {
    expect_i32(
        "type PairView = {
             b: number;
             c: number;
             sum(): number;
         };
         class Box {
             c: number = 3;
             b: number = 2;
             sum(): number {
                 return this.b + this.c;
             }
         }
         let view: PairView = new Box();
         return view.b + view.sum();",
        7,
    );
}

#[test]
fn test_structural_parameter_projection_uses_shape_slots() {
    expect_i32(
        "class Box {
             a: number = 1;
             b: number = 2;
             sum(x: number): number {
                 return this.a + this.b + x;
             }
         }
         function run(view: { b: number; sum(x: number): number }): number {
             view.b = 5;
             return view.sum(1) + view.b;
         }
         return run(new Box());",
        12,
    );
}

#[test]
fn test_structural_field_load_projection_uses_shape_slots() {
    expect_i32(
        "type View = {
             b: number;
             c: number;
         };
         class Box {
             c: number = 3;
             b: number = 2;
         }
         class Holder {
             view: View;
             constructor() {
                 this.view = new Box();
             }
         }
         let holder = new Holder();
         let view = holder.view;
         return view.b + view.c;",
        5,
    );
}

#[test]
fn test_structural_call_result_destructuring_uses_shape_slots() {
    expect_i32(
        "class Box {
             c: number = 3;
             b: number = 2;
         }
         function makeView(): { b: number; c: number } {
             return new Box();
         }
         let { b, c } = makeView();
         return b + c;",
        5,
    );
}

#[test]
fn test_structural_conditional_result_destructuring_uses_shape_slots() {
    expect_i32(
        "class Box {
             c: number = 3;
             b: number = 2;
         }
         let fallback = { b: 1, c: 1 };
         let { b, c } = true
             ? (new Box() as { b: number; c: number })
             : fallback;
         return b + c;",
        5,
    );
}

#[test]
fn test_structural_parameter_destructuring_uses_projected_shape_slots() {
    expect_i32(
        "class Box {
             c: number = 3;
             b: number = 2;
         }
         function run(view: { b: number; c: number }): number {
             let { b, c } = view;
             return b + c;
         }
         return run(new Box());",
        5,
    );
}
