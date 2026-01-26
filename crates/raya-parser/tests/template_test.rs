//! Tests for template literal parsing

use raya_parser::ast::{Expression, Statement, TemplatePart};
use raya_parser::Parser;

// ============================================================================
// Basic Template Literals
// ============================================================================

#[test]
fn test_parse_simple_template() {
    let source = r#"let s = `hello`;"#;
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            assert!(decl.initializer.is_some());
            match decl.initializer.as_ref().unwrap() {
                Expression::TemplateLiteral(template) => {
                    // Single string part
                    assert_eq!(template.parts.len(), 1);
                    assert!(matches!(template.parts[0], TemplatePart::String(_)));
                }
                _ => panic!("Expected template literal"),
            }
        }
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_template_with_simple_interpolation() {
    let source = r#"let s = `Hello, ${name}!`;"#;
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            match decl.initializer.as_ref().unwrap() {
                Expression::TemplateLiteral(template) => {
                    // "Hello, " + ${name} + "!"
                    assert_eq!(template.parts.len(), 3);
                    assert!(matches!(template.parts[0], TemplatePart::String(_)));
                    match &template.parts[1] {
                        TemplatePart::Expression(expr) => {
                            match expr.as_ref() {
                                Expression::Identifier(id) => {
                                    assert_eq!(interner.resolve(id.name), "name");
                                }
                                _ => panic!("Expected identifier"),
                            }
                        }
                        _ => panic!("Expected expression"),
                    }
                    assert!(matches!(template.parts[2], TemplatePart::String(_)));
                }
                _ => panic!("Expected template literal"),
            }
        }
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_template_with_multiple_interpolations() {
    let source = r#"let s = `${a} + ${b} = ${c}`;"#;
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            match decl.initializer.as_ref().unwrap() {
                Expression::TemplateLiteral(template) => {
                    // Count expression parts
                    let expr_count = template
                        .parts
                        .iter()
                        .filter(|p| matches!(p, TemplatePart::Expression(_)))
                        .count();
                    assert_eq!(expr_count, 3);
                }
                _ => panic!("Expected template literal"),
            }
        }
        _ => panic!("Expected variable declaration"),
    }
}

// ============================================================================
// Complex Expressions in Templates
// ============================================================================

#[test]
fn test_parse_template_with_member_access() {
    let source = r#"let s = `User: ${user.name}, Age: ${user.age}`;"#;
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            match decl.initializer.as_ref().unwrap() {
                Expression::TemplateLiteral(template) => {
                    // Count expression parts that are member expressions
                    let member_count = template.parts.iter().filter(|p| {
                        matches!(p, TemplatePart::Expression(e) if matches!(e.as_ref(), Expression::Member(_)))
                    }).count();
                    assert_eq!(member_count, 2);
                }
                _ => panic!("Expected template literal"),
            }
        }
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_template_with_function_call() {
    let source = r#"let s = `Result: ${calculate(x, y)}`;"#;
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            match decl.initializer.as_ref().unwrap() {
                Expression::TemplateLiteral(template) => {
                    // Find call expression
                    let has_call = template.parts.iter().any(|p| {
                        matches!(p, TemplatePart::Expression(e) if matches!(e.as_ref(), Expression::Call(_)))
                    });
                    assert!(has_call, "Expected call expression in template");
                }
                _ => panic!("Expected template literal"),
            }
        }
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_template_with_binary_expression() {
    let source = r#"let s = `Sum: ${a + b}`;"#;
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            match decl.initializer.as_ref().unwrap() {
                Expression::TemplateLiteral(template) => {
                    let has_binary = template.parts.iter().any(|p| {
                        matches!(p, TemplatePart::Expression(e) if matches!(e.as_ref(), Expression::Binary(_)))
                    });
                    assert!(has_binary, "Expected binary expression in template");
                }
                _ => panic!("Expected template literal"),
            }
        }
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_template_with_ternary() {
    let source = r#"let s = `Status: ${isActive ? "Active" : "Inactive"}`;"#;
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            match decl.initializer.as_ref().unwrap() {
                Expression::TemplateLiteral(template) => {
                    let has_conditional = template.parts.iter().any(|p| {
                        matches!(p, TemplatePart::Expression(e) if matches!(e.as_ref(), Expression::Conditional(_)))
                    });
                    assert!(has_conditional, "Expected conditional expression in template");
                }
                _ => panic!("Expected template literal"),
            }
        }
        _ => panic!("Expected variable declaration"),
    }
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_parse_template_starting_with_interpolation() {
    let source = r#"let s = `${prefix}suffix`;"#;
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            match decl.initializer.as_ref().unwrap() {
                Expression::TemplateLiteral(template) => {
                    // Parts: ${prefix} + "suffix" (may or may not have empty string at start)
                    let expr_count = template
                        .parts
                        .iter()
                        .filter(|p| matches!(p, TemplatePart::Expression(_)))
                        .count();
                    assert_eq!(expr_count, 1);
                }
                _ => panic!("Expected template literal"),
            }
        }
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_template_ending_with_interpolation() {
    let source = r#"let s = `prefix${suffix}`;"#;
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            match decl.initializer.as_ref().unwrap() {
                Expression::TemplateLiteral(template) => {
                    let expr_count = template
                        .parts
                        .iter()
                        .filter(|p| matches!(p, TemplatePart::Expression(_)))
                        .count();
                    assert_eq!(expr_count, 1);
                }
                _ => panic!("Expected template literal"),
            }
        }
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_template_consecutive_interpolations() {
    let source = r#"let s = `${a}${b}${c}`;"#;
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            match decl.initializer.as_ref().unwrap() {
                Expression::TemplateLiteral(template) => {
                    let expr_count = template
                        .parts
                        .iter()
                        .filter(|p| matches!(p, TemplatePart::Expression(_)))
                        .count();
                    assert_eq!(expr_count, 3);
                }
                _ => panic!("Expected template literal"),
            }
        }
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_template_with_nested_object() {
    let source = r#"let s = `Data: ${{ key: value }}`;"#;
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            match decl.initializer.as_ref().unwrap() {
                Expression::TemplateLiteral(template) => {
                    let has_object = template.parts.iter().any(|p| {
                        matches!(p, TemplatePart::Expression(e) if matches!(e.as_ref(), Expression::Object(_)))
                    });
                    assert!(has_object, "Expected object expression in template");
                }
                _ => panic!("Expected template literal"),
            }
        }
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_template_with_array() {
    let source = r#"let s = `Items: ${[1, 2, 3]}`;"#;
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            match decl.initializer.as_ref().unwrap() {
                Expression::TemplateLiteral(template) => {
                    let has_array = template.parts.iter().any(|p| {
                        matches!(p, TemplatePart::Expression(e) if matches!(e.as_ref(), Expression::Array(_)))
                    });
                    assert!(has_array, "Expected array expression in template");
                }
                _ => panic!("Expected template literal"),
            }
        }
        _ => panic!("Expected variable declaration"),
    }
}
