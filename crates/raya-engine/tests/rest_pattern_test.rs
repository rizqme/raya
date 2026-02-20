//! Tests for rest pattern parsing

use raya_engine::parser::ast::{Pattern, Statement};
use raya_engine::parser::Parser;

// ============================================================================
// Array Rest Patterns
// ============================================================================

#[test]
fn test_parse_rest_pattern_array() {
    let source = "let [first, ...rest] = arr;";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            match &decl.pattern {
                Pattern::Array(arr) => {
                    // One element plus rest
                    assert_eq!(arr.elements.len(), 1);
                    assert!(arr.rest.is_some());
                }
                _ => panic!("Expected array pattern"),
            }
        }
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_rest_pattern_only() {
    let source = "let [...items] = arr;";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            match &decl.pattern {
                Pattern::Array(arr) => {
                    // No elements, just rest
                    assert_eq!(arr.elements.len(), 0);
                    assert!(arr.rest.is_some());
                }
                _ => panic!("Expected array pattern"),
            }
        }
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_rest_pattern_multiple_elements() {
    let source = "let [a, b, c, ...others] = arr;";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            match &decl.pattern {
                Pattern::Array(arr) => {
                    // Three elements plus rest
                    assert_eq!(arr.elements.len(), 3);
                    assert!(arr.rest.is_some());
                }
                _ => panic!("Expected array pattern"),
            }
        }
        _ => panic!("Expected variable declaration"),
    }
}

// ============================================================================
// Object Rest Patterns
// ============================================================================

#[test]
fn test_parse_rest_pattern_object() {
    let source = "let { name, ...rest } = obj;";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            match &decl.pattern {
                Pattern::Object(obj) => {
                    // One property plus rest
                    assert_eq!(obj.properties.len(), 1);
                    assert!(obj.rest.is_some());
                }
                _ => panic!("Expected object pattern"),
            }
        }
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_rest_pattern_object_only() {
    let source = "let { ...all } = obj;";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            match &decl.pattern {
                Pattern::Object(obj) => {
                    // No properties, just rest
                    assert_eq!(obj.properties.len(), 0);
                    assert!(obj.rest.is_some());
                }
                _ => panic!("Expected object pattern"),
            }
        }
        _ => panic!("Expected variable declaration"),
    }
}

// ============================================================================
// Function Parameter Rest Patterns
// ============================================================================

#[test]
fn test_parse_rest_parameter() {
    let source = "function sum(...nums: number[]): number { return 0; }";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::FunctionDecl(func) => {
            // Should have one rest parameter
            assert_eq!(func.params.len(), 1);
            match &func.params[0].pattern {
                Pattern::Rest(rest) => {
                    match rest.argument.as_ref() {
                        Pattern::Identifier(id) => {
                            assert!(_interner.resolve(id.name) == "nums");
                        }
                        _ => panic!("Expected identifier in rest pattern"),
                    }
                }
                _ => panic!("Expected rest pattern"),
            }
        }
        _ => panic!("Expected function declaration"),
    }
}

#[test]
fn test_parse_rest_parameter_with_other_params() {
    let source = "function log(prefix: string, ...args: string[]): void {}";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::FunctionDecl(func) => {
            // Should have two parameters
            assert_eq!(func.params.len(), 2);
            // First should be identifier pattern
            match &func.params[0].pattern {
                Pattern::Identifier(_) => {}
                _ => panic!("Expected identifier pattern for first param"),
            }
            // Second should be rest pattern
            match &func.params[1].pattern {
                Pattern::Rest(_) => {}
                _ => panic!("Expected rest pattern for second param"),
            }
        }
        _ => panic!("Expected function declaration"),
    }
}

// ============================================================================
// Error Cases
// ============================================================================

#[test]
fn test_parse_rest_pattern_must_be_last_in_array() {
    let source = "let [...rest, last] = arr;";
    let parser = Parser::new(source).unwrap();
    let result = parser.parse();
    assert!(result.is_err(), "Rest element must be last in array pattern");
}

#[test]
fn test_parse_rest_pattern_must_be_last_in_object() {
    let source = "let { ...rest, last } = obj;";
    let parser = Parser::new(source).unwrap();
    let result = parser.parse();
    assert!(result.is_err(), "Rest element must be last in object pattern");
}
