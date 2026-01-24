//! Tests for statement parsing

use raya_parser::ast::*;
use raya_parser::parser::Parser;

// ============================================================================
// Variable Declarations
// ============================================================================

#[test]
fn test_parse_let_declaration() {
    let source = "let x = 42;";
    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            assert!(matches!(decl.kind, VariableKind::Let));
            match &decl.pattern {
                Pattern::Identifier(id) => assert_eq!(id.name, "x"),
                _ => panic!("Expected identifier pattern"),
            }
            assert!(decl.initializer.is_some());
        }
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_const_declaration() {
    let source = "const y = 10;";
    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            assert!(matches!(decl.kind, VariableKind::Const));
            match &decl.pattern {
                Pattern::Identifier(id) => assert_eq!(id.name, "y"),
                _ => panic!("Expected identifier pattern"),
            }
            assert!(decl.initializer.is_some());
        }
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_let_with_type_annotation() {
    let source = "let x: number = 42;";
    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            assert!(matches!(decl.kind, VariableKind::Let));
            assert!(decl.type_annotation.is_some());
            assert!(decl.initializer.is_some());
        }
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_let_without_initializer() {
    let source = "let x;";
    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            assert!(matches!(decl.kind, VariableKind::Let));
            assert!(decl.initializer.is_none());
        }
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_const_requires_initializer() {
    let source = "const x;";
    let parser = Parser::new(source).unwrap();
    let result = parser.parse();

    assert!(result.is_err(), "const without initializer should fail");
}

// ============================================================================
// Function Declarations
// ============================================================================

#[test]
fn test_parse_function_declaration() {
    let source = "function add(a, b) { return a + b; }";
    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::FunctionDecl(func) => {
            assert_eq!(func.name.name, "add");
            assert_eq!(func.params.len(), 2);
            assert!(!func.is_async);
        }
        _ => panic!("Expected function declaration"),
    }
}

#[test]
fn test_parse_async_function() {
    let source = "async function fetchData() { return 42; }";
    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::FunctionDecl(func) => {
            assert_eq!(func.name.name, "fetchData");
            assert!(func.is_async);
        }
        _ => panic!("Expected function declaration"),
    }
}

#[test]
fn test_parse_function_with_return_type() {
    let source = "function getValue(): number { return 42; }";
    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::FunctionDecl(func) => {
            assert_eq!(func.name.name, "getValue");
            assert!(func.return_type.is_some());
        }
        _ => panic!("Expected function declaration"),
    }
}

#[test]
fn test_parse_function_with_type_parameters() {
    let source = "function identity<T>(value) { return value; }";
    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::FunctionDecl(func) => {
            assert_eq!(func.name.name, "identity");
            assert!(func.type_params.is_some());
            let type_params = func.type_params.as_ref().unwrap();
            assert_eq!(type_params.len(), 1);
            assert_eq!(type_params[0].name.name, "T");
        }
        _ => panic!("Expected function declaration"),
    }
}

// ============================================================================
// Control Flow - If Statements
// ============================================================================

#[test]
fn test_parse_if_statement() {
    let source = "if (x > 0) { return x; }";
    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::If(if_stmt) => {
            assert!(if_stmt.else_branch.is_none());
        }
        _ => panic!("Expected if statement"),
    }
}

#[test]
fn test_parse_if_else_statement() {
    let source = "if (x > 0) { return x; } else { return 0; }";
    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::If(if_stmt) => {
            assert!(if_stmt.else_branch.is_some());
        }
        _ => panic!("Expected if statement"),
    }
}

#[test]
fn test_parse_if_else_if_statement() {
    let source = "if (x > 0) { return 1; } else if (x < 0) { return -1; } else { return 0; }";
    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::If(if_stmt) => {
            assert!(if_stmt.else_branch.is_some());
            // The else branch should be another if statement
            match if_stmt.else_branch.as_ref().unwrap().as_ref() {
                Statement::If(else_if) => {
                    assert!(else_if.else_branch.is_some());
                }
                _ => panic!("Expected else if to be an if statement"),
            }
        }
        _ => panic!("Expected if statement"),
    }
}

// ============================================================================
// Control Flow - While Statements
// ============================================================================

#[test]
fn test_parse_while_statement() {
    let source = "while (x > 0) { x = x - 1; }";
    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::While(while_stmt) => {
            match &*while_stmt.body {
                Statement::Block(_) => (),
                _ => panic!("Expected block statement for while body"),
            }
        }
        _ => panic!("Expected while statement"),
    }
}

// ============================================================================
// Control Flow - For Statements
// ============================================================================

#[test]
fn test_parse_for_statement() {
    let source = "for (let i = 0; i < 10; i = i + 1) { console.log(i); }";
    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::For(for_stmt) => {
            assert!(for_stmt.init.is_some());
            assert!(for_stmt.test.is_some());
            assert!(for_stmt.update.is_some());
        }
        _ => panic!("Expected for statement"),
    }
}

#[test]
fn test_parse_for_statement_no_init() {
    let source = "for (; i < 10; i = i + 1) { console.log(i); }";
    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::For(for_stmt) => {
            assert!(for_stmt.init.is_none());
            assert!(for_stmt.test.is_some());
            assert!(for_stmt.update.is_some());
        }
        _ => panic!("Expected for statement"),
    }
}

#[test]
fn test_parse_for_statement_expression_init() {
    let source = "for (i = 0; i < 10; i = i + 1) { console.log(i); }";
    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::For(for_stmt) => {
            assert!(for_stmt.init.is_some());
            match for_stmt.init.as_ref().unwrap() {
                ForInit::Expression(_) => (),
                _ => panic!("Expected expression init"),
            }
        }
        _ => panic!("Expected for statement"),
    }
}

// ============================================================================
// Jump Statements
// ============================================================================

#[test]
fn test_parse_return_statement() {
    let source = "return 42;";
    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::Return(ret) => {
            assert!(ret.value.is_some());
        }
        _ => panic!("Expected return statement"),
    }
}

#[test]
fn test_parse_return_statement_no_value() {
    let source = "return;";
    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::Return(ret) => {
            assert!(ret.value.is_none());
        }
        _ => panic!("Expected return statement"),
    }
}

#[test]
fn test_parse_break_statement() {
    let source = "break;";
    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::Break(_) => (),
        _ => panic!("Expected break statement"),
    }
}

#[test]
fn test_parse_continue_statement() {
    let source = "continue;";
    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::Continue(_) => (),
        _ => panic!("Expected continue statement"),
    }
}

#[test]
fn test_parse_throw_statement() {
    let source = "throw new Error();";
    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::Throw(throw_stmt) => {
            match &throw_stmt.value {
                Expression::New(_) => (),
                _ => panic!("Expected new expression"),
            }
        }
        _ => panic!("Expected throw statement"),
    }
}

// ============================================================================
// Mixed Statements
// ============================================================================

#[test]
fn test_parse_multiple_statements() {
    let source = r#"
        let x = 42;
        const y = 10;
        function add(a, b) {
            return a + b;
        }
        if (x > y) {
            return x;
        }
    "#;
    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 4);
    assert!(matches!(module.statements[0], Statement::VariableDecl(_)));
    assert!(matches!(module.statements[1], Statement::VariableDecl(_)));
    assert!(matches!(module.statements[2], Statement::FunctionDecl(_)));
    assert!(matches!(module.statements[3], Statement::If(_)));
}
