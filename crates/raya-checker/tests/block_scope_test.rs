//! Tests for block-scoped variable declarations

use raya_checker::{Binder, TypeChecker};
use raya_parser::Parser;
use raya_types::TypeContext;

#[test]
fn test_simple_block_scope() {
    let source = r#"
        let x: number = 42;
        let y: number = x;
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
        "Expected no errors with simple block scope, got: {:?}",
        result
    );
}

#[test]
fn test_nested_block_with_variable() {
    let source = r#"
        let x: number = 42;
        let cond: boolean = true;
        if (cond) {
            let y: number = x;
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
        "Expected no errors with nested block, got: {:?}",
        result
    );
}

#[test]
fn test_variable_declared_in_block_used_in_same_block() {
    // This tests the actual issue: can we use a variable declared earlier in the same block?
    let source = r#"
        let cond: boolean = true;
        if (cond) {
            let n: number = 42;
            let i: number = n;
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
        "Expected no errors when using variable declared in same block, got: {:?}",
        result
    );
}

#[test]
fn test_while_loop_with_assignment() {
    // Test assignment to outer scope variable from within while loop
    let source = r#"
        let count: number = 0;
        let cond: boolean = true;

        while (cond) {
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
        "Expected no errors with while loop assignment, got: {:?}",
        result
    );
}

#[test]
fn test_while_loop_with_typeof_and_assignment() {
    // This is closer to the failing test
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

    if result.is_err() {
        eprintln!("Errors: {:?}", result);
    }

    assert!(
        result.is_ok(),
        "Expected no errors with typeof in while, got: {:?}",
        result
    );
}
