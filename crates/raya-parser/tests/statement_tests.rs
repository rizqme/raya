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
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            assert!(matches!(decl.kind, VariableKind::Let));
            match &decl.pattern {
                Pattern::Identifier(id) => assert_eq!(interner.resolve(id.name), "x"),
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
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            assert!(matches!(decl.kind, VariableKind::Const));
            match &decl.pattern {
                Pattern::Identifier(id) => assert_eq!(interner.resolve(id.name), "y"),
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
    let (module, _interner) = parser.parse().unwrap();

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
    let (module, _interner) = parser.parse().unwrap();

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
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::FunctionDecl(func) => {
            assert_eq!(interner.resolve(func.name.name), "add");
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
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::FunctionDecl(func) => {
            assert_eq!(interner.resolve(func.name.name), "fetchData");
            assert!(func.is_async);
        }
        _ => panic!("Expected function declaration"),
    }
}

#[test]
fn test_parse_function_with_return_type() {
    let source = "function getValue(): number { return 42; }";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::FunctionDecl(func) => {
            assert_eq!(interner.resolve(func.name.name), "getValue");
            assert!(func.return_type.is_some());
        }
        _ => panic!("Expected function declaration"),
    }
}

#[test]
fn test_parse_function_with_type_parameters() {
    let source = "function identity<T>(value) { return value; }";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::FunctionDecl(func) => {
            assert_eq!(interner.resolve(func.name.name), "identity");
            assert!(func.type_params.is_some());
            let type_params = func.type_params.as_ref().unwrap();
            assert_eq!(type_params.len(), 1);
            assert_eq!(interner.resolve(type_params[0].name.name), "T");
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
    let (module, _interner) = parser.parse().unwrap();

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
    let (module, _interner) = parser.parse().unwrap();

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
    let (module, _interner) = parser.parse().unwrap();

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
    let (module, _interner) = parser.parse().unwrap();

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
    let (module, _interner) = parser.parse().unwrap();

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
    let (module, _interner) = parser.parse().unwrap();

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
    let (module, _interner) = parser.parse().unwrap();

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
    let (module, _interner) = parser.parse().unwrap();

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
    let (module, _interner) = parser.parse().unwrap();

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
    let (module, _interner) = parser.parse().unwrap();

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
    let (module, _interner) = parser.parse().unwrap();

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
    let (module, _interner) = parser.parse().unwrap();

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
// Type Alias Declarations
// ============================================================================

#[test]
fn test_parse_type_alias_simple() {
    let source = "type ID = string;";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::TypeAliasDecl(decl) => {
            assert_eq!(interner.resolve(decl.name.name), "ID");
            assert!(decl.type_params.is_none());
        }
        _ => panic!("Expected type alias declaration"),
    }
}

#[test]
fn test_parse_type_alias_union() {
    let source = "type StringOrNumber = string | number;";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::TypeAliasDecl(decl) => {
            assert_eq!(interner.resolve(decl.name.name), "StringOrNumber");
            match &decl.type_annotation.ty {
                Type::Union(_) => (),
                _ => panic!("Expected union type"),
            }
        }
        _ => panic!("Expected type alias declaration"),
    }
}

#[test]
fn test_parse_type_alias_generic() {
    let source = "type Result<T, E> = { ok: T } | { error: E };";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::TypeAliasDecl(decl) => {
            assert_eq!(interner.resolve(decl.name.name), "Result");
            assert!(decl.type_params.is_some());
            let type_params = decl.type_params.as_ref().unwrap();
            assert_eq!(type_params.len(), 2);
            assert_eq!(interner.resolve(type_params[0].name.name), "T");
            assert_eq!(interner.resolve(type_params[1].name.name), "E");
        }
        _ => panic!("Expected type alias declaration"),
    }
}

// ============================================================================
// Class Declarations
// ============================================================================

#[test]
fn test_parse_class_empty() {
    let source = "class Point {}";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(interner.resolve(decl.name.name), "Point");
            assert!(!decl.is_abstract);
            assert!(decl.extends.is_none());
            assert!(decl.implements.is_empty());
            assert!(decl.members.is_empty());
        }
        _ => panic!("Expected class declaration"),
    }
}

#[test]
fn test_parse_class_with_fields() {
    let source = r#"
        class Point {
            x: number;
            y: number;
        }
    "#;
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(interner.resolve(decl.name.name), "Point");
            assert_eq!(decl.members.len(), 2);
            for member in &decl.members {
                match member {
                    ClassMember::Field(field) => {
                        assert!(field.type_annotation.is_some());
                    }
                    _ => panic!("Expected field member"),
                }
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

#[test]
fn test_parse_class_with_method() {
    let source = r#"
        class Calculator {
            add(a: number, b: number): number {
                return a + b;
            }
        }
    "#;
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(interner.resolve(decl.name.name), "Calculator");
            assert_eq!(decl.members.len(), 1);
            match &decl.members[0] {
                ClassMember::Method(method) => {
                    assert_eq!(interner.resolve(method.name.name), "add");
                    assert_eq!(method.params.len(), 2);
                    assert!(method.return_type.is_some());
                    assert!(method.body.is_some());
                }
                _ => panic!("Expected method member"),
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

#[test]
fn test_parse_class_with_constructor() {
    let source = r#"
        class Point {
            x: number;
            y: number;
            constructor(x: number, y: number) {
                this.x = x;
                this.y = y;
            }
        }
    "#;
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(interner.resolve(decl.name.name), "Point");
            assert_eq!(decl.members.len(), 3);
            // Third member should be constructor
            match &decl.members[2] {
                ClassMember::Constructor(ctor) => {
                    assert_eq!(ctor.params.len(), 2);
                }
                _ => panic!("Expected constructor member"),
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

#[test]
fn test_parse_class_extends() {
    let source = "class Square extends Shape {}";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(interner.resolve(decl.name.name), "Square");
            assert!(decl.extends.is_some());
        }
        _ => panic!("Expected class declaration"),
    }
}

#[test]
fn test_parse_abstract_class() {
    let source = r#"
        abstract class Shape {
            abstract area(): number;
        }
    "#;
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(interner.resolve(decl.name.name), "Shape");
            assert!(decl.is_abstract);
            assert_eq!(decl.members.len(), 1);
            match &decl.members[0] {
                ClassMember::Method(method) => {
                    assert!(method.is_abstract);
                    assert!(method.body.is_none());
                }
                _ => panic!("Expected method member"),
            }
        }
        _ => panic!("Expected class declaration"),
    }
}

#[test]
fn test_parse_class_generic() {
    let source = "class Container<T> {}";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ClassDecl(decl) => {
            assert_eq!(interner.resolve(decl.name.name), "Container");
            assert!(decl.type_params.is_some());
            let type_params = decl.type_params.as_ref().unwrap();
            assert_eq!(type_params.len(), 1);
            assert_eq!(interner.resolve(type_params[0].name.name), "T");
        }
        _ => panic!("Expected class declaration"),
    }
}

// ============================================================================
// Try-Catch-Finally Statements
// ============================================================================

#[test]
fn test_parse_try_catch() {
    let source = r#"
        try {
            throw new Error();
        } catch (e) {
            console.log(e);
        }
    "#;
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::Try(try_stmt) => {
            assert!(try_stmt.catch_clause.is_some());
            assert!(try_stmt.finally_clause.is_none());
            let catch = try_stmt.catch_clause.as_ref().unwrap();
            assert!(catch.param.is_some());
        }
        _ => panic!("Expected try statement"),
    }
}

#[test]
fn test_parse_try_finally() {
    let source = r#"
        try {
            doSomething();
        } finally {
            cleanup();
        }
    "#;
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::Try(try_stmt) => {
            assert!(try_stmt.catch_clause.is_none());
            assert!(try_stmt.finally_clause.is_some());
        }
        _ => panic!("Expected try statement"),
    }
}

#[test]
fn test_parse_try_catch_finally() {
    let source = r#"
        try {
            doSomething();
        } catch (e) {
            handleError(e);
        } finally {
            cleanup();
        }
    "#;
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::Try(try_stmt) => {
            assert!(try_stmt.catch_clause.is_some());
            assert!(try_stmt.finally_clause.is_some());
        }
        _ => panic!("Expected try statement"),
    }
}

#[test]
fn test_parse_try_without_catch_or_finally_fails() {
    let source = "try { doSomething(); }";
    let parser = Parser::new(source).unwrap();
    let result = parser.parse();

    assert!(result.is_err(), "try without catch or finally should fail");
}

// ============================================================================
// Import Declarations
// ============================================================================

#[test]
fn test_parse_import_named() {
    let source = r#"import { foo, bar } from "module";"#;
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ImportDecl(decl) => {
            assert_eq!(decl.specifiers.len(), 2);
            match &decl.specifiers[0] {
                ImportSpecifier::Named { name, alias } => {
                    assert_eq!(interner.resolve(name.name), "foo");
                    assert!(alias.is_none());
                }
                _ => panic!("Expected named import"),
            }
            match &decl.specifiers[1] {
                ImportSpecifier::Named { name, alias } => {
                    assert_eq!(interner.resolve(name.name), "bar");
                    assert!(alias.is_none());
                }
                _ => panic!("Expected named import"),
            }
            assert_eq!(interner.resolve(decl.source.value), "module");
        }
        _ => panic!("Expected import declaration"),
    }
}

#[test]
fn test_parse_import_with_alias() {
    let source = r#"import { foo as f } from "module";"#;
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ImportDecl(decl) => {
            assert_eq!(decl.specifiers.len(), 1);
            match &decl.specifiers[0] {
                ImportSpecifier::Named { name, alias } => {
                    assert_eq!(interner.resolve(name.name), "foo");
                    assert!(alias.is_some());
                    assert_eq!(interner.resolve(alias.as_ref().unwrap().name), "f");
                }
                _ => panic!("Expected named import"),
            }
        }
        _ => panic!("Expected import declaration"),
    }
}

#[test]
fn test_parse_import_default() {
    let source = r#"import React from "react";"#;
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ImportDecl(decl) => {
            assert_eq!(decl.specifiers.len(), 1);
            match &decl.specifiers[0] {
                ImportSpecifier::Default(name) => {
                    assert_eq!(interner.resolve(name.name), "React");
                }
                _ => panic!("Expected default import"),
            }
        }
        _ => panic!("Expected import declaration"),
    }
}

#[test]
fn test_parse_import_namespace() {
    let source = r#"import * as utils from "utils";"#;
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ImportDecl(decl) => {
            assert_eq!(decl.specifiers.len(), 1);
            match &decl.specifiers[0] {
                ImportSpecifier::Namespace(name) => {
                    assert_eq!(interner.resolve(name.name), "utils");
                }
                _ => panic!("Expected namespace import"),
            }
        }
        _ => panic!("Expected import declaration"),
    }
}

#[test]
fn test_parse_import_default_and_named() {
    let source = r#"import React, { useState } from "react";"#;
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ImportDecl(decl) => {
            assert_eq!(decl.specifiers.len(), 2);
            match &decl.specifiers[0] {
                ImportSpecifier::Default(name) => {
                    assert_eq!(interner.resolve(name.name), "React");
                }
                _ => panic!("Expected default import"),
            }
            match &decl.specifiers[1] {
                ImportSpecifier::Named { name, .. } => {
                    assert_eq!(interner.resolve(name.name), "useState");
                }
                _ => panic!("Expected named import"),
            }
        }
        _ => panic!("Expected import declaration"),
    }
}

// ============================================================================
// Export Declarations
// ============================================================================

#[test]
fn test_parse_export_named() {
    let source = "export { foo, bar };";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ExportDecl(ExportDecl::Named { specifiers, source, .. }) => {
            assert_eq!(specifiers.len(), 2);
            assert_eq!(interner.resolve(specifiers[0].name.name), "foo");
            assert_eq!(interner.resolve(specifiers[1].name.name), "bar");
            assert!(source.is_none());
        }
        _ => panic!("Expected named export declaration"),
    }
}

#[test]
fn test_parse_export_reexport() {
    let source = r#"export { foo } from "module";"#;
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ExportDecl(ExportDecl::Named { specifiers, source, .. }) => {
            assert_eq!(specifiers.len(), 1);
            assert_eq!(interner.resolve(specifiers[0].name.name), "foo");
            assert!(source.is_some());
            assert_eq!(interner.resolve(source.as_ref().unwrap().value), "module");
        }
        _ => panic!("Expected named export declaration"),
    }
}

#[test]
fn test_parse_export_all() {
    let source = r#"export * from "module";"#;
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ExportDecl(ExportDecl::All { source, .. }) => {
            assert_eq!(interner.resolve(source.value), "module");
        }
        _ => panic!("Expected export all declaration"),
    }
}

#[test]
fn test_parse_export_const() {
    let source = "export const x = 42;";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ExportDecl(ExportDecl::Declaration(decl)) => {
            match decl.as_ref() {
                Statement::VariableDecl(var) => {
                    assert!(matches!(var.kind, VariableKind::Const));
                    match &var.pattern {
                        Pattern::Identifier(id) => assert_eq!(interner.resolve(id.name), "x"),
                        _ => panic!("Expected identifier pattern"),
                    }
                }
                _ => panic!("Expected variable declaration"),
            }
        }
        _ => panic!("Expected export declaration"),
    }
}

#[test]
fn test_parse_export_function() {
    let source = "export function add(a, b) { return a + b; }";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ExportDecl(ExportDecl::Declaration(decl)) => {
            match decl.as_ref() {
                Statement::FunctionDecl(func) => {
                    assert_eq!(interner.resolve(func.name.name), "add");
                }
                _ => panic!("Expected function declaration"),
            }
        }
        _ => panic!("Expected export declaration"),
    }
}

#[test]
fn test_parse_export_class() {
    let source = "export class Point {}";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ExportDecl(ExportDecl::Declaration(decl)) => {
            match decl.as_ref() {
                Statement::ClassDecl(class) => {
                    assert_eq!(interner.resolve(class.name.name), "Point");
                }
                _ => panic!("Expected class declaration"),
            }
        }
        _ => panic!("Expected export declaration"),
    }
}

#[test]
fn test_parse_export_type_alias() {
    let source = "export type ID = string;";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::ExportDecl(ExportDecl::Declaration(decl)) => {
            match decl.as_ref() {
                Statement::TypeAliasDecl(type_alias) => {
                    assert_eq!(interner.resolve(type_alias.name.name), "ID");
                }
                _ => panic!("Expected type alias declaration"),
            }
        }
        _ => panic!("Expected export declaration"),
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
    let (module, _interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 4);
    assert!(matches!(module.statements[0], Statement::VariableDecl(_)));
    assert!(matches!(module.statements[1], Statement::VariableDecl(_)));
    assert!(matches!(module.statements[2], Statement::FunctionDecl(_)));
    assert!(matches!(module.statements[3], Statement::If(_)));
}
