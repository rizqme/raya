//! Integration tests for exhaustiveness checking
//!
//! NOTE: These tests currently serve as placeholders for exhaustiveness checking.
//! Full testing will be possible once the parser supports switch statements.

use raya_checker::{Binder, TypeChecker};
use raya_parser::Parser;
use raya_types::TypeContext;

#[test]
fn test_exhaustiveness_module_exists() {
    // This test verifies the exhaustiveness module compiles and is accessible
    use raya_checker::ExhaustivenessResult;

    let result = ExhaustivenessResult::Exhaustive;
    assert_eq!(result, ExhaustivenessResult::Exhaustive);
}

#[test]
fn test_basic_program_with_if_chains() {
    // Test that mimics exhaustiveness checking with if-else chains
    // Once switch statements are implemented, these should be converted
    let source = r#"
        let x: string = "hello";
        if (x === "hello") {
            let y: number = 1;
        } else if (x === "world") {
            let z: number = 2;
        } else {
            let w: number = 3;
        }
    "#;

    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    let mut type_ctx = TypeContext::new();
    let binder = Binder::new(&mut type_ctx);
    let symbols = binder.bind_module(&module).unwrap();

    let checker = TypeChecker::new(&mut type_ctx, &symbols);
    let result = checker.check_module(&module);

    // Should pass - all branches are covered with else
    assert!(result.is_ok(), "Expected no errors, got: {:?}", result);
}

#[test]
fn test_type_checking_still_works() {
    // Verify basic type checking still works after Phase 4 changes
    let source = r#"
        let x: number = 42;
        let y: string = "hello";
        let z: boolean = true;
    "#;

    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    let mut type_ctx = TypeContext::new();
    let binder = Binder::new(&mut type_ctx);
    let symbols = binder.bind_module(&module).unwrap();

    let checker = TypeChecker::new(&mut type_ctx, &symbols);
    let result = checker.check_module(&module);

    assert!(result.is_ok(), "Expected no errors, got: {:?}", result);
}

// TODO: Add full exhaustiveness checking tests once switch statements are implemented:
// - test_switch_with_default
// - test_switch_with_all_cases_covered
// - test_switch_missing_cases
// - test_discriminated_union_exhaustiveness
