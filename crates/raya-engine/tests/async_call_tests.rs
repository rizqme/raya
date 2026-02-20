//! Tests for async call expression parsing

use raya_engine::parser::ast::*;
use raya_engine::parser::parser::Parser;

// ============================================================================
// Basic Async Call
// ============================================================================

#[test]
fn test_parse_async_call_simple() {
    let source = "async myFn()";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::AsyncCall(async_call) => {
                // Check that callee is myFn
                match &*async_call.callee {
                    Expression::Identifier(id) => assert_eq!(interner.resolve(id.name), "myFn"),
                    _ => panic!("Expected identifier callee"),
                }
                // Check no arguments
                assert_eq!(async_call.arguments.len(), 0);
            }
            _ => panic!("Expected async call expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_async_call_with_args() {
    let source = "async myFn(1, 2, 3)";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::AsyncCall(async_call) => {
                // Check that callee is myFn
                match &*async_call.callee {
                    Expression::Identifier(id) => assert_eq!(interner.resolve(id.name), "myFn"),
                    _ => panic!("Expected identifier callee"),
                }
                // Check arguments
                assert_eq!(async_call.arguments.len(), 3);
                match &async_call.arguments[0] {
                    Expression::IntLiteral(lit) => assert_eq!(lit.value, 1),
                    Expression::FloatLiteral(lit) => assert_eq!(lit.value, 1.0),
                    _ => panic!("Expected number literal"),
                }
            }
            _ => panic!("Expected async call expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}

// ============================================================================
// Async Call with Member Access
// ============================================================================

#[test]
fn test_parse_async_call_member() {
    let source = "async obj.method()";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::AsyncCall(async_call) => {
                // Check that callee is obj.method
                match &*async_call.callee {
                    Expression::Member(member) => {
                        assert_eq!(interner.resolve(member.property.name), "method");
                        match &*member.object {
                            Expression::Identifier(id) => assert_eq!(interner.resolve(id.name), "obj"),
                            _ => panic!("Expected identifier object"),
                        }
                    }
                    _ => panic!("Expected member expression callee"),
                }
            }
            _ => panic!("Expected async call expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_async_call_chained_member() {
    let source = "async obj.nested.method()";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::AsyncCall(async_call) => {
                // Check that callee is obj.nested.method
                match &*async_call.callee {
                    Expression::Member(outer_member) => {
                        assert_eq!(interner.resolve(outer_member.property.name), "method");
                        match &*outer_member.object {
                            Expression::Member(inner_member) => {
                                assert_eq!(interner.resolve(inner_member.property.name), "nested");
                            }
                            _ => panic!("Expected nested member expression"),
                        }
                    }
                    _ => panic!("Expected member expression callee"),
                }
            }
            _ => panic!("Expected async call expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}

// ============================================================================
// Async Call in Assignments
// ============================================================================

#[test]
fn test_parse_async_call_in_assignment() {
    let source = "let task = async myFn();";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            match &decl.initializer {
                Some(Expression::AsyncCall(async_call)) => {
                    match &*async_call.callee {
                        Expression::Identifier(id) => assert_eq!(interner.resolve(id.name), "myFn"),
                        _ => panic!("Expected identifier callee"),
                    }
                }
                _ => panic!("Expected async call initializer"),
            }
        }
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_async_call_assigned_to_const() {
    let source = "const task = async fetchData(url);";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            assert!(matches!(decl.kind, VariableKind::Const));
            match &decl.initializer {
                Some(Expression::AsyncCall(async_call)) => {
                    match &*async_call.callee {
                        Expression::Identifier(id) => assert_eq!(interner.resolve(id.name), "fetchData"),
                        _ => panic!("Expected identifier callee"),
                    }
                    assert_eq!(async_call.arguments.len(), 1);
                }
                _ => panic!("Expected async call initializer"),
            }
        }
        _ => panic!("Expected variable declaration"),
    }
}

// ============================================================================
// Async Call with Type Arguments
// ============================================================================

#[test]
fn test_parse_async_call_with_type_args() {
    let source = "async genericFn<number>()";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::AsyncCall(async_call) => {
                // Check that callee is genericFn
                match &*async_call.callee {
                    Expression::Identifier(id) => assert_eq!(interner.resolve(id.name), "genericFn"),
                    _ => panic!("Expected identifier callee"),
                }
                // Check type arguments
                assert!(async_call.type_args.is_some());
                let type_args = async_call.type_args.as_ref().unwrap();
                assert_eq!(type_args.len(), 1);
            }
            _ => panic!("Expected async call expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}

// ============================================================================
// Async Call vs Regular Call
// ============================================================================

#[test]
fn test_parse_regular_call_without_async() {
    let source = "myFn()";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::Call(_) => {
                // This is correct - regular call without async
            }
            Expression::AsyncCall(_) => panic!("Should not be async call"),
            _ => panic!("Expected call expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_async_vs_await() {
    // async foo() creates a Task immediately
    let source1 = "async foo()";
    let parser1 = Parser::new(source1).unwrap();
    let (module1, _interner1) = parser1.parse().unwrap();

    match &module1.statements[0] {
        Statement::Expression(expr_stmt) => {
            assert!(matches!(expr_stmt.expression, Expression::AsyncCall(_)));
        }
        _ => panic!("Expected expression statement"),
    }

    // await foo() waits for a Task/Promise to complete
    let source2 = "await foo()";
    let parser2 = Parser::new(source2).unwrap();
    let (module2, _interner2) = parser2.parse().unwrap();

    match &module2.statements[0] {
        Statement::Expression(expr_stmt) => {
            assert!(matches!(expr_stmt.expression, Expression::Await(_)));
        }
        _ => panic!("Expected expression statement"),
    }
}

// ============================================================================
// Error Cases
// ============================================================================

#[test]
fn test_async_without_call_fails() {
    let source = "async myVar";
    let parser = Parser::new(source).unwrap();
    let result = parser.parse();

    assert!(result.is_err(), "async without call should fail");
}

#[test]
fn test_async_with_non_call_expression_fails() {
    let source = "async 42";
    let parser = Parser::new(source).unwrap();
    let result = parser.parse();

    assert!(result.is_err(), "async with non-call should fail");
}

// ============================================================================
// Complex Expressions
// ============================================================================

#[test]
fn test_parse_async_call_in_binary_expression() {
    let source = "let x = async foo() + bar()";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::VariableDecl(decl) => match &decl.initializer {
            Some(Expression::Binary(bin)) => {
                assert!(matches!(bin.operator, BinaryOperator::Add));
                // Left side is async call
                assert!(matches!(&*bin.left, Expression::AsyncCall(_)));
                // Right side is regular call
                assert!(matches!(&*bin.right, Expression::Call(_)));
            }
            _ => panic!("Expected binary expression"),
        },
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_multiple_async_calls() {
    let source = r#"
        let task1 = async fetch();
        let task2 = async process();
        let task3 = async save();
    "#;
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 3);

    for stmt in &module.statements {
        match stmt {
            Statement::VariableDecl(decl) => {
                assert!(matches!(
                    decl.initializer,
                    Some(Expression::AsyncCall(_))
                ));
            }
            _ => panic!("Expected variable declaration"),
        }
    }
}

#[test]
fn test_parse_async_call_as_argument() {
    let source = "processTask(async compute())";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::Call(call) => {
                assert_eq!(call.arguments.len(), 1);
                // Argument should be an async call
                match &call.arguments[0] {
                    Expression::AsyncCall(async_call) => {
                        match &*async_call.callee {
                            Expression::Identifier(id) => assert_eq!(interner.resolve(id.name), "compute"),
                            _ => panic!("Expected identifier"),
                        }
                    }
                    _ => panic!("Expected async call argument"),
                }
            }
            _ => panic!("Expected call expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}
