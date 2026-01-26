//! Integration tests for typeof on bare unions
//!
//! Tests the complete workflow:
//! - Bare union detection and transformation
//! - typeof-based type narrowing
//! - Exhaustiveness checking
//! - Forbidden field access

use raya_checker::{Binder, TypeChecker};
use raya_parser::Parser;
use raya_types::TypeContext;

#[test]
fn test_bare_union_typeof_complete_workflow() {
    // Complete example: bare union with typeof narrowing
    let source = r#"
        let value: string | number = 42;

        if (typeof value === "number") {
            let n: number = value;
        } else {
            let s: string = value;
        }
    "#;

    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    let mut type_ctx = TypeContext::new();
    let binder = Binder::new(&mut type_ctx, &interner);
    let symbols = binder.bind_module(&module).unwrap();

    let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
    let result = checker.check_module(&module);

    assert!(
        result.is_ok(),
        "Expected no errors in typeof narrowing, got: {:?}",
        result
    );
}

#[test]
fn test_bare_union_three_primitives() {
    // Bare union with three primitive types
    let source = r#"
        let value: string | number | boolean = true;

        if (typeof value === "boolean") {
            let b: boolean = value;
        } else if (typeof value === "number") {
            let n: number = value;
        } else {
            let s: string = value;
        }
    "#;

    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    let mut type_ctx = TypeContext::new();
    let binder = Binder::new(&mut type_ctx, &interner);
    let symbols = binder.bind_module(&module).unwrap();

    let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
    let result = checker.check_module(&module);

    assert!(
        result.is_ok(),
        "Expected no errors with three-way typeof, got: {:?}",
        result
    );
}

#[test]
fn test_typeof_with_negation() {
    // typeof with !== operator
    let source = r#"
        let value: string | number = "hello";

        if (typeof value !== "string") {
            let n: number = value;
        } else {
            let s: string = value;
        }
    "#;

    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    let mut type_ctx = TypeContext::new();
    let binder = Binder::new(&mut type_ctx, &interner);
    let symbols = binder.bind_module(&module).unwrap();

    let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
    let result = checker.check_module(&module);

    assert!(
        result.is_ok(),
        "Expected no errors with negated typeof, got: {:?}",
        result
    );
}

// NOTE: This test is skipped because Array.isArray requires stdlib
// #[test]
// fn test_array_is_array_on_union() {
//     // Using Array.isArray on a union with array type
//     // Note: This tests call-based type guards, not bare unions per se
//     // Requires: Array to be defined in stdlib or as builtin
//     let source = r#"
//         let value: string = "hello";
//
//         if (!Array.isArray(value)) {
//             let s: string = value;
//         }
//     "#;
//
//     let parser = Parser::new(source).unwrap();
//     let (module, interner) = parser.parse().unwrap();
//
//     let mut type_ctx = TypeContext::new();
//     let binder = Binder::new(&mut type_ctx, &interner);
//     let symbols = binder.bind_module(&module).unwrap();
//
//     let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
//     let result = checker.check_module(&module);
//
//     assert!(
//         result.is_ok(),
//         "Expected no errors with Array.isArray, got: {:?}",
//         result
//     );
// }

// NOTE: This test is skipped because Number.isInteger requires stdlib
// #[test]
// fn test_number_is_integer_refinement() {
//     // Number.isInteger refines number type
//     // Requires: Number to be defined in stdlib or as builtin
//     let source = r#"
//         let value: number = 42;
//
//         if (Number.isInteger(value)) {
//             let n: number = value;
//         }
//     "#;
//
//     let parser = Parser::new(source).unwrap();
//     let (module, interner) = parser.parse().unwrap();
//
//     let mut type_ctx = TypeContext::new();
//     let binder = Binder::new(&mut type_ctx, &interner);
//     let symbols = binder.bind_module(&module).unwrap();
//
//     let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
//     let result = checker.check_module(&module);
//
//     assert!(
//         result.is_ok(),
//         "Expected no errors with Number.isInteger, got: {:?}",
//         result
//     );
// }

// NOTE: This test is skipped because it requires user-defined function
// #[test]
// fn test_custom_type_predicate() {
//     // Custom type predicate function
//     // Requires: isString function to be defined
//     let source = r#"
//         let value: string | number = "test";
//
//         if (isString(value)) {
//             let s: string | number = value;
//         }
//     "#;
//
//     let parser = Parser::new(source).unwrap();
//     let (module, interner) = parser.parse().unwrap();
//
//     let mut type_ctx = TypeContext::new();
//     let binder = Binder::new(&mut type_ctx, &interner);
//     let symbols = binder.bind_module(&module).unwrap();
//
//     let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
//     let result = checker.check_module(&module);
//
//     // Should work (isString is treated as a type predicate)
//     assert!(
//         result.is_ok(),
//         "Expected no errors with custom predicate, got: {:?}",
//         result
//     );
// }

#[test]
fn test_forbidden_type_field_access() {
    // Accessing $type on bare union should fail
    let source = r#"
        let value: string | number = 42;
        let t = value.$type;
    "#;

    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    let mut type_ctx = TypeContext::new();
    let binder = Binder::new(&mut type_ctx, &interner);
    let symbols = binder.bind_module(&module).unwrap();

    let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
    let result = checker.check_module(&module);

    assert!(
        result.is_err(),
        "Expected error for $type access on bare union"
    );

    let errors = result.unwrap_err();
    assert!(
        errors.iter().any(|e| matches!(
            e,
            raya_checker::error::CheckError::ForbiddenFieldAccess { field, .. } if field == "$type"
        )),
        "Expected ForbiddenFieldAccess error"
    );
}

#[test]
fn test_forbidden_value_field_access() {
    // Accessing $value on bare union should fail
    let source = r#"
        let value: string | number = "hello";
        let v = value.$value;
    "#;

    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    let mut type_ctx = TypeContext::new();
    let binder = Binder::new(&mut type_ctx, &interner);
    let symbols = binder.bind_module(&module).unwrap();

    let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
    let result = checker.check_module(&module);

    assert!(
        result.is_err(),
        "Expected error for $value access on bare union"
    );

    let errors = result.unwrap_err();
    assert!(
        errors.iter().any(|e| matches!(
            e,
            raya_checker::error::CheckError::ForbiddenFieldAccess { field, .. } if field == "$value"
        )),
        "Expected ForbiddenFieldAccess error"
    );
}

#[test]
fn test_nested_typeof_checks() {
    // Nested typeof checks for complex narrowing
    let source = r#"
        let value: string | number | boolean = 42;

        if (typeof value === "number") {
            let n: number = value;
        } else {
            let sb: string | boolean = value;
        }
    "#;

    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    let mut type_ctx = TypeContext::new();
    let binder = Binder::new(&mut type_ctx, &interner);
    let symbols = binder.bind_module(&module).unwrap();

    let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
    let result = checker.check_module(&module);

    assert!(
        result.is_ok(),
        "Expected no errors with typeof narrowing, got: {:?}",
        result
    );
}

#[test]
fn test_typeof_in_while_loop() {
    // typeof guard in while loop
    let source = r#"
        let value: string | number = 42;
        let count: number = 0;

        while (typeof value === "number") {
            let n: number = value;
            count = count + 1;
        }
    "#;

    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    let mut type_ctx = TypeContext::new();
    let binder = Binder::new(&mut type_ctx, &interner);
    let symbols = binder.bind_module(&module).unwrap();

    let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
    let result = checker.check_module(&module);

    assert!(
        result.is_ok(),
        "Expected no errors with typeof in while, got: {:?}",
        result
    );
}

#[test]
fn test_typeof_pattern_matching_style() {
    // Pattern matching style with typeof
    let source = r#"
        let value: string | number | boolean = "test";

        if (typeof value === "string") {
            let s: string = value;
        } else if (typeof value === "number") {
            let n: number = value;
        } else if (typeof value === "boolean") {
            let b: boolean = value;
        }
    "#;

    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    let mut type_ctx = TypeContext::new();
    let binder = Binder::new(&mut type_ctx, &interner);
    let symbols = binder.bind_module(&module).unwrap();

    let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
    let result = checker.check_module(&module);

    assert!(
        result.is_ok(),
        "Expected no errors with pattern matching style, got: {:?}",
        result
    );
}

#[test]
fn test_multiple_type_guards() {
    // Multiple type guards in sequence
    let source = r#"
        let value: string | number = 42;

        if (typeof value === "number") {
            // After typeof narrows to number, we can use it as number
            let i: number = value;
        }
    "#;

    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    let mut type_ctx = TypeContext::new();
    let binder = Binder::new(&mut type_ctx, &interner);
    let symbols = binder.bind_module(&module).unwrap();

    let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
    let result = checker.check_module(&module);

    assert!(
        result.is_ok(),
        "Expected no errors with typeof guard, got: {:?}",
        result
    );
}
