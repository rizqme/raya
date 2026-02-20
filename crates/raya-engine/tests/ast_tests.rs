use raya_engine::parser::ast::*;
use raya_engine::parser::token::Span;
use raya_engine::parser::Interner;

// Helper to create Symbol from string for tests
fn intern(interner: &mut Interner, s: &str) -> raya_engine::parser::Symbol {
    interner.intern(s)
}

// Helper to create an Identifier with interned name
fn ident(interner: &mut Interner, name: &str, span: Span) -> Identifier {
    Identifier::new(interner.intern(name), span)
}

// ============================================================================
// Module Tests
// ============================================================================

#[test]
fn test_empty_module() {
    let module = Module::new(vec![], Span::new(0, 0, 1, 1));

    assert!(module.is_empty());
    assert_eq!(module.len(), 0);
}

#[test]
fn test_module_with_statements() {
    let stmt1 = Statement::Empty(Span::new(0, 1, 1, 1));
    let stmt2 = Statement::Empty(Span::new(2, 3, 2, 1));

    let module = Module::new(vec![stmt1, stmt2], Span::new(0, 3, 1, 1));

    assert!(!module.is_empty());
    assert_eq!(module.len(), 2);
}

// ============================================================================
// Variable Declaration Tests
// ============================================================================

#[test]
fn test_variable_decl_let() {
    let mut interner = Interner::new();

    let decl = VariableDecl {
        kind: VariableKind::Let,
        pattern: Pattern::Identifier(ident(&mut interner, "x", Span::new(4, 5, 1, 5))),
        type_annotation: None,
        initializer: Some(Expression::IntLiteral(IntLiteral {
            value: 42,
            span: Span::new(8, 10, 1, 9),
        })),
        span: Span::new(0, 11, 1, 1),
    };

    assert_eq!(decl.kind, VariableKind::Let);
    assert!(decl.initializer.is_some());
    assert_eq!(decl.span.start, 0);
}

#[test]
fn test_variable_decl_const() {
    let mut interner = Interner::new();

    let decl = VariableDecl {
        kind: VariableKind::Const,
        pattern: Pattern::Identifier(ident(&mut interner, "y", Span::new(6, 7, 1, 7))),
        type_annotation: Some(TypeAnnotation {
            ty: Type::Primitive(PrimitiveType::Number),
            span: Span::new(9, 15, 1, 10),
        }),
        initializer: Some(Expression::IntLiteral(IntLiteral {
            value: 10,
            span: Span::new(18, 20, 1, 19),
        })),
        span: Span::new(0, 21, 1, 1),
    };

    assert_eq!(decl.kind, VariableKind::Const);
    assert!(decl.type_annotation.is_some());
    assert!(decl.initializer.is_some());
}

// ============================================================================
// Function Declaration Tests
// ============================================================================

#[test]
fn test_function_decl_simple() {
    let mut interner = Interner::new();
    let add_name = intern(&mut interner, "add");

    let func = FunctionDecl {
        name: Identifier::new(add_name, Span::new(9, 12, 1, 10)),
        type_params: None,
        params: vec![
            Parameter {
                decorators: vec![],
                visibility: None,
                pattern: Pattern::Identifier(ident(&mut interner, "x", Span::new(13, 14, 1, 14))),
                type_annotation: Some(TypeAnnotation {
                    ty: Type::Primitive(PrimitiveType::Number),
                    span: Span::new(16, 22, 1, 17),
                }),
                default_value: None,
                span: Span::new(13, 22, 1, 14),
            },
            Parameter {
                decorators: vec![],
                visibility: None,
                pattern: Pattern::Identifier(ident(&mut interner, "y", Span::new(24, 25, 1, 25))),
                type_annotation: Some(TypeAnnotation {
                    ty: Type::Primitive(PrimitiveType::Number),
                    span: Span::new(27, 33, 1, 28),
                }),
                default_value: None,
                span: Span::new(24, 33, 1, 25),
            },
        ],
        return_type: Some(TypeAnnotation {
            ty: Type::Primitive(PrimitiveType::Number),
            span: Span::new(36, 42, 1, 37),
        }),
        body: BlockStatement {
            statements: vec![],
            span: Span::new(43, 45, 1, 44),
        },
        is_async: false,
        span: Span::new(0, 45, 1, 1),
    };

    assert_eq!(interner.resolve(func.name.name), "add");
    assert_eq!(func.params.len(), 2);
    assert!(func.return_type.is_some());
    assert!(!func.is_async);
}

#[test]
fn test_function_decl_async() {
    let mut interner = Interner::new();

    let func = FunctionDecl {
        name: ident(&mut interner, "fetch", Span::new(15, 20, 1, 16)),
        type_params: None,
        params: vec![],
        return_type: None,
        body: BlockStatement {
            statements: vec![],
            span: Span::new(23, 25, 1, 24),
        },
        is_async: true,
        span: Span::new(0, 25, 1, 1),
    };

    assert!(func.is_async);
    assert_eq!(interner.resolve(func.name.name), "fetch");
}

// ============================================================================
// Class Declaration Tests
// ============================================================================

#[test]
fn test_class_decl_simple() {
    let mut interner = Interner::new();

    let class = ClassDecl {
        decorators: vec![],
        is_abstract: false,
        name: ident(&mut interner, "Point", Span::new(6, 11, 1, 7)),
        type_params: None,
        extends: None,
        implements: vec![],
        members: vec![
            ClassMember::Field(FieldDecl {
                decorators: vec![],
                visibility: Visibility::Public,
                name: ident(&mut interner, "x", Span::new(18, 19, 2, 5)),
                type_annotation: Some(TypeAnnotation {
                    ty: Type::Primitive(PrimitiveType::Number),
                    span: Span::new(21, 27, 2, 8),
                }),
                initializer: None,
                is_static: false,
                is_readonly: false,
                annotations: vec![],
                span: Span::new(18, 28, 2, 5),
            }),
            ClassMember::Field(FieldDecl {
                decorators: vec![],
                visibility: Visibility::Public,
                name: ident(&mut interner, "y", Span::new(33, 34, 3, 5)),
                type_annotation: Some(TypeAnnotation {
                    ty: Type::Primitive(PrimitiveType::Number),
                    span: Span::new(36, 42, 3, 8),
                }),
                initializer: None,
                is_static: false,
                is_readonly: false,
                annotations: vec![],
                span: Span::new(33, 43, 3, 5),
            }),
        ],
        annotations: vec![],
        span: Span::new(0, 45, 1, 1),
    };

    assert_eq!(interner.resolve(class.name.name), "Point");
    assert_eq!(class.members.len(), 2);
    assert!(class.extends.is_none());
}

#[test]
fn test_class_with_extends() {
    let mut interner = Interner::new();

    let class = ClassDecl {
        decorators: vec![],
        is_abstract: false,
        name: ident(&mut interner, "Square", Span::new(6, 12, 1, 7)),
        type_params: None,
        extends: Some(TypeAnnotation {
            ty: Type::Reference(TypeReference {
                name: ident(&mut interner, "Shape", Span::new(21, 26, 1, 22)),
                type_args: None,
            }),
            span: Span::new(21, 26, 1, 22),
        }),
        implements: vec![],
        members: vec![],
        annotations: vec![],
        span: Span::new(0, 29, 1, 1),
    };

    assert!(class.extends.is_some());
}

// ============================================================================
// Control Flow Statement Tests
// ============================================================================

#[test]
fn test_if_statement() {
    let if_stmt = IfStatement {
        condition: Expression::BooleanLiteral(BooleanLiteral {
            value: true,
            span: Span::new(4, 8, 1, 5),
        }),
        then_branch: Box::new(Statement::Empty(Span::new(10, 11, 1, 11))),
        else_branch: None,
        span: Span::new(0, 11, 1, 1),
    };

    assert!(if_stmt.else_branch.is_none());
}

#[test]
fn test_if_else_statement() {
    let if_stmt = IfStatement {
        condition: Expression::BooleanLiteral(BooleanLiteral {
            value: false,
            span: Span::new(4, 9, 1, 5),
        }),
        then_branch: Box::new(Statement::Empty(Span::new(11, 12, 1, 12))),
        else_branch: Some(Box::new(Statement::Empty(Span::new(18, 19, 1, 19)))),
        span: Span::new(0, 19, 1, 1),
    };

    assert!(if_stmt.else_branch.is_some());
}

#[test]
fn test_while_statement() {
    let while_stmt = WhileStatement {
        condition: Expression::BooleanLiteral(BooleanLiteral {
            value: true,
            span: Span::new(7, 11, 1, 8),
        }),
        body: Box::new(Statement::Empty(Span::new(13, 14, 1, 14))),
        span: Span::new(0, 14, 1, 1),
    };

    assert_eq!(while_stmt.span.start, 0);
}

#[test]
fn test_switch_statement() {
    let switch_stmt = SwitchStatement {
        discriminant: Expression::IntLiteral(IntLiteral {
            value: 1,
            span: Span::new(8, 9, 1, 9),
        }),
        cases: vec![
            SwitchCase {
                test: Some(Expression::IntLiteral(IntLiteral {
                    value: 1,
                    span: Span::new(21, 22, 2, 10),
                })),
                consequent: vec![Statement::Break(BreakStatement {
                    label: None,
                    span: Span::new(24, 30, 2, 13),
                })],
                span: Span::new(16, 31, 2, 5),
            },
            SwitchCase {
                test: None, // default case
                consequent: vec![],
                span: Span::new(36, 45, 3, 5),
            },
        ],
        span: Span::new(0, 47, 1, 1),
    };

    assert_eq!(switch_stmt.cases.len(), 2);
    assert!(switch_stmt.cases[1].test.is_none()); // default case
}

// ============================================================================
// Statement Helper Tests
// ============================================================================

#[test]
fn test_statement_is_declaration() {
    let mut interner = Interner::new();

    let var_decl = Statement::VariableDecl(VariableDecl {
        kind: VariableKind::Let,
        pattern: Pattern::Identifier(ident(&mut interner, "x", Span::new(0, 1, 1, 1))),
        type_annotation: None,
        initializer: None,
        span: Span::new(0, 1, 1, 1),
    });

    let empty_stmt = Statement::Empty(Span::new(0, 1, 1, 1));

    assert!(var_decl.is_declaration());
    assert!(!empty_stmt.is_declaration());
}

#[test]
fn test_expression_is_literal() {
    let mut interner = Interner::new();

    let int_lit = Expression::IntLiteral(IntLiteral {
        value: 42,
        span: Span::new(0, 2, 1, 1),
    });

    let id = Expression::Identifier(ident(&mut interner, "x", Span::new(0, 1, 1, 1)));

    assert!(int_lit.is_literal());
    assert!(!id.is_literal());
}

// ============================================================================
// Import/Export Tests
// ============================================================================

#[test]
fn test_import_named() {
    let mut interner = Interner::new();

    let import = ImportDecl {
        specifiers: vec![
            ImportSpecifier::Named {
                name: ident(&mut interner, "foo", Span::new(9, 12, 1, 10)),
                alias: None,
            },
            ImportSpecifier::Named {
                name: ident(&mut interner, "bar", Span::new(14, 17, 1, 15)),
                alias: Some(ident(&mut interner, "baz", Span::new(21, 24, 1, 22))),
            },
        ],
        source: StringLiteral {
            value: intern(&mut interner, "./module"),
            span: Span::new(31, 41, 1, 32),
        },
        span: Span::new(0, 42, 1, 1),
    };

    assert_eq!(import.specifiers.len(), 2);
    assert_eq!(interner.resolve(import.source.value), "./module");
}

#[test]
fn test_export_named() {
    let mut interner = Interner::new();

    let export = ExportDecl::Named {
        specifiers: vec![ExportSpecifier {
            name: ident(&mut interner, "foo", Span::new(9, 12, 1, 10)),
            alias: None,
        }],
        source: None,
        span: Span::new(0, 15, 1, 1),
    };

    if let ExportDecl::Named { specifiers, .. } = export {
        assert_eq!(specifiers.len(), 1);
    } else {
        panic!("Expected Named export");
    }
}
