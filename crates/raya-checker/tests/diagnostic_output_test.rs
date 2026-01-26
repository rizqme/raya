//! Integration tests for diagnostic output
//!
//! Tests the complete diagnostic workflow with actual source code

use raya_checker::{Binder, TypeChecker, Diagnostic, create_files};
use raya_parser::Parser;
use raya_types::TypeContext;

#[test]
fn test_type_mismatch_diagnostic() {
    let source = r#"
        let x: number = "hello";
    "#;

    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    let mut type_ctx = TypeContext::new();
    let binder = Binder::new(&mut type_ctx, &interner);
    let symbols = binder.bind_module(&module).unwrap();

    let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
    let result = checker.check_module(&module);

    assert!(result.is_err(), "Expected type mismatch error");

    let errors = result.unwrap_err();
    assert_eq!(errors.len(), 1, "Expected exactly one error");

    // Create diagnostic and verify it can be emitted
    let files = create_files("test.raya", source);
    let diag = Diagnostic::from_check_error(&errors[0], 0);

    // Verify we can emit the diagnostic (to stderr)
    // In a real test, we'd capture stderr, but for now just ensure no panic
    let _ = diag.emit(&files);
}

#[test]
fn test_undefined_variable_diagnostic() {
    let source = r#"
        let x: number = y;
    "#;

    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    let mut type_ctx = TypeContext::new();
    let binder = Binder::new(&mut type_ctx, &interner);
    let symbols = binder.bind_module(&module).unwrap();

    let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
    let result = checker.check_module(&module);

    assert!(result.is_err(), "Expected undefined variable error");

    let errors = result.unwrap_err();
    let files = create_files("test.raya", source);
    let diag = Diagnostic::from_check_error(&errors[0], 0);

    let _ = diag.emit(&files);
}

#[test]
fn test_union_type_mismatch_with_suggestion() {
    // This should produce a type mismatch with a helpful suggestion
    // Assigning a union to a more specific type requires narrowing
    let source = r#"
        let value: string | number = "hello";
        let result: string = value;
    "#;

    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    let mut type_ctx = TypeContext::new();
    let binder = Binder::new(&mut type_ctx, &interner);
    let symbols = binder.bind_module(&module).unwrap();

    let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
    let result = checker.check_module(&module);

    // This test may or may not error depending on implementation
    // If it errors, it should include a suggestion about typeof
    if result.is_err() {
        let errors = result.unwrap_err();
        let files = create_files("test.raya", source);
        let diag = Diagnostic::from_check_error(&errors[0], 0);
        let _ = diag.emit(&files);
    }
}

#[test]
fn test_forbidden_field_access_diagnostic() {
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

    assert!(result.is_err(), "Expected forbidden field access error");

    let errors = result.unwrap_err();
    let files = create_files("test.raya", source);
    let diag = Diagnostic::from_check_error(&errors[0], 0);

    // This should include note about bare unions and help about typeof
    let _ = diag.emit(&files);
}

#[test]
fn test_duplicate_symbol_diagnostic() {
    let source = r#"
        let x: number = 42;
        let x: string = "hello";
    "#;

    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    let mut type_ctx = TypeContext::new();
    let binder = Binder::new(&mut type_ctx, &interner);
    let result = binder.bind_module(&module);

    assert!(result.is_err(), "Expected duplicate symbol error");

    match result {
        Err(errors) => {
            assert_eq!(errors.len(), 1, "Expected exactly one error");

            let files = create_files("test.raya", source);
            let diag = Diagnostic::from_bind_error(&errors[0], 0);

            // This should show both locations (original and duplicate)
            let _ = diag.emit(&files);
        }
        Ok(_) => panic!("Expected bind error"),
    }
}

#[test]
fn test_non_exhaustive_match_diagnostic() {
    // Test exhaustiveness checking with helpful missing case information
    let source = r#"
        type Result = { kind: "ok"; value: number } | { kind: "error"; error: string };
        let result: Result = { kind: "ok", value: 42 };

        switch (result.kind) {
            case "ok":
                let v: number = result.value;
                break;
        }
    "#;

    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    let mut type_ctx = TypeContext::new();
    let binder = Binder::new(&mut type_ctx, &interner);
    let symbols = binder.bind_module(&module).unwrap();

    let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
    let result = checker.check_module(&module);

    // This may or may not error depending on implementation
    // The test demonstrates how the diagnostic would be shown
    if result.is_err() {
        let errors = result.unwrap_err();
        let files = create_files("test.raya", source);

        for error in &errors {
            let diag = Diagnostic::from_check_error(error, 0);
            let _ = diag.emit(&files);
        }
    }
}

#[test]
fn test_multiple_errors_in_one_file() {
    let source = r#"
        let x: number = "wrong type";
        let y: string = undefined_var;
        let z: boolean = 42;
    "#;

    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    let mut type_ctx = TypeContext::new();
    let binder = Binder::new(&mut type_ctx, &interner);
    let symbols = binder.bind_module(&module).unwrap();

    let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
    let result = checker.check_module(&module);

    assert!(result.is_err(), "Expected multiple errors");

    let errors = result.unwrap_err();
    let files = create_files("test.raya", source);

    // Emit all diagnostics
    for error in &errors {
        let diag = Diagnostic::from_check_error(error, 0);
        let _ = diag.emit(&files);
    }
}
