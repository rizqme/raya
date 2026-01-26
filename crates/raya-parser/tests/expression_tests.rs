//! Tests for expression parsing

use raya_parser::ast::*;
use raya_parser::parser::Parser;

#[test]
fn test_parse_number_literal() {
    let source = "42";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::IntLiteral(lit) => assert_eq!(lit.value, 42),
            Expression::FloatLiteral(lit) => assert_eq!(lit.value, 42.0),
            _ => panic!("Expected number literal"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_string_literal() {
    let source = r#""hello""#;
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::StringLiteral(lit) => assert_eq!(interner.resolve(lit.value), "hello"),
            _ => panic!("Expected string literal"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_boolean_literals() {
    let source = "true";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::BooleanLiteral(lit) => assert!(lit.value),
            _ => panic!("Expected boolean literal"),
        },
        _ => panic!("Expected expression statement"),
    }

    let source = "false";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::BooleanLiteral(lit) => assert!(!lit.value),
            _ => panic!("Expected boolean literal"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_null_literal() {
    let source = "null";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::NullLiteral(_) => (),
            _ => panic!("Expected null literal"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_identifier() {
    let source = "myVariable";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::Identifier(id) => assert_eq!(interner.resolve(id.name), "myVariable"),
            _ => panic!("Expected identifier expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_binary_addition() {
    let source = "1 + 2";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::Binary(bin) => {
                assert!(matches!(bin.operator, BinaryOperator::Add));
                match &*bin.left {
                    Expression::IntLiteral(lit) => assert_eq!(lit.value, 1),
                    Expression::FloatLiteral(lit) => assert_eq!(lit.value, 1.0),
                    _ => panic!("Expected literal"),
                }
                match &*bin.right {
                    Expression::IntLiteral(lit) => assert_eq!(lit.value, 2),
                    Expression::FloatLiteral(lit) => assert_eq!(lit.value, 2.0),
                    _ => panic!("Expected literal"),
                }
            }
            _ => panic!("Expected binary expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_binary_precedence() {
    let source = "1 + 2 * 3";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    // Should parse as: 1 + (2 * 3)
    match &module.statements[0] {
        Statement::Expression(expr_stmt) => {
            // Debug: print what we actually got
            println!("Got expression: {:?}", expr_stmt.expression);

            match &expr_stmt.expression {
                Expression::Binary(bin) => {
                    println!("Binary operator: {:?}", bin.operator);
                    assert!(matches!(bin.operator, BinaryOperator::Add), "Expected Add, got {:?}", bin.operator);
                    // Left is 1
                    match &*bin.left {
                        Expression::IntLiteral(lit) => assert_eq!(lit.value, 1),
                        Expression::FloatLiteral(lit) => assert_eq!(lit.value, 1.0),
                        _ => panic!("Expected literal"),
                    }
                    // Right is (2 * 3)
                    match &*bin.right {
                        Expression::Binary(inner) => {
                            assert!(matches!(inner.operator, BinaryOperator::Multiply));
                        }
                        _ => panic!("Expected binary expression for 2 * 3"),
                    }
                }
                other => panic!("Expected binary expression, got {:?}", other),
            }
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_unary_negation() {
    let source = "-42";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::Unary(un) => {
                assert!(matches!(un.operator, UnaryOperator::Minus));
                match &*un.operand {
                    Expression::IntLiteral(lit) => assert_eq!(lit.value, 42),
                    Expression::FloatLiteral(lit) => assert_eq!(lit.value, 42.0),
                    _ => panic!("Expected literal"),
                }
            }
            _ => panic!("Expected unary expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_unary_not() {
    let source = "!true";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::Unary(un) => {
                assert!(matches!(un.operator, UnaryOperator::Not));
            }
            _ => panic!("Expected unary expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_array_literal() {
    let source = "[1, 2, 3]";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::Array(arr) => {
                assert_eq!(arr.elements.len(), 3);
                // Check first element is 1
                match &arr.elements[0] {
                    Some(ArrayElement::Expression(Expression::IntLiteral(lit))) => assert_eq!(lit.value, 1),
                    Some(ArrayElement::Expression(Expression::FloatLiteral(lit))) => assert_eq!(lit.value, 1.0),
                    _ => panic!("Expected element"),
                }
            }
            _ => panic!("Expected array expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_empty_array() {
    let source = "[]";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::Array(arr) => {
                assert_eq!(arr.elements.len(), 0);
            }
            _ => panic!("Expected array expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_object_literal() {
    let source = "{ x: 1, y: 2 }";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::Object(obj) => {
                assert_eq!(obj.properties.len(), 2);
                // Check first property
                match &obj.properties[0] {
                    ObjectProperty::Property(prop) => {
                        match &prop.key {
                            PropertyKey::Identifier(id) => assert_eq!(interner.resolve(id.name), "x"),
                            _ => panic!("Expected identifier key"),
                        }
                        match &prop.value {
                            Expression::IntLiteral(lit) => assert_eq!(lit.value, 1),
                            Expression::FloatLiteral(lit) => assert_eq!(lit.value, 1.0),
                            _ => panic!("Expected literal value"),
                        }
                    }
                    _ => panic!("Expected property"),
                }
            }
            _ => panic!("Expected object expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_empty_object() {
    let source = "{}";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::Object(obj) => {
                assert_eq!(obj.properties.len(), 0);
            }
            _ => panic!("Expected object expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_member_access() {
    let source = "obj.property";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::Member(mem) => {
                assert!(!mem.optional);
                match &*mem.object {
                    Expression::Identifier(id) => assert_eq!(interner.resolve(id.name), "obj"),
                    _ => panic!("Expected identifier"),
                }
                assert_eq!(interner.resolve(mem.property.name), "property");
            }
            _ => panic!("Expected member expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_computed_member_access() {
    let source = r#"obj["key"]"#;
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::Index(idx) => {
                match &*idx.index {
                    Expression::StringLiteral(lit) => assert_eq!(interner.resolve(lit.value), "key"),
                    _ => panic!("Expected string literal"),
                }
            }
            _ => panic!("Expected index expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_function_call() {
    let source = "foo()";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::Call(call) => {
                assert_eq!(call.arguments.len(), 0);
                match &*call.callee {
                    Expression::Identifier(id) => assert_eq!(interner.resolve(id.name), "foo"),
                    _ => panic!("Expected identifier"),
                }
            }
            _ => panic!("Expected call expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_function_call_with_args() {
    let source = "foo(1, 2)";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::Call(call) => {
                assert_eq!(call.arguments.len(), 2);
                match &call.arguments[0] {
                    Expression::IntLiteral(lit) => assert_eq!(lit.value, 1),
                    Expression::FloatLiteral(lit) => assert_eq!(lit.value, 1.0),
                    _ => panic!("Expected literal"),
                }
            }
            _ => panic!("Expected call expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_chained_member_access() {
    let source = "a.b.c";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::Member(mem) => {
                // Outermost is a.b accessing c
                assert_eq!(interner.resolve(mem.property.name), "c");
                // Object should be a.b
                match &*mem.object {
                    Expression::Member(inner) => {
                        assert_eq!(interner.resolve(inner.property.name), "b");
                    }
                    _ => panic!("Expected member expression"),
                }
            }
            _ => panic!("Expected member expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_assignment() {
    let source = "x = 42";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::Assignment(assign) => {
                assert!(matches!(assign.operator, AssignmentOperator::Assign));
                match &*assign.left {
                    Expression::Identifier(id) => assert_eq!(interner.resolve(id.name), "x"),
                    _ => panic!("Expected identifier"),
                }
                match &*assign.right {
                    Expression::IntLiteral(lit) => assert_eq!(lit.value, 42),
                    Expression::FloatLiteral(lit) => assert_eq!(lit.value, 42.0),
                    _ => panic!("Expected literal"),
                }
            }
            _ => panic!("Expected assignment expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_compound_assignment() {
    let source = "x += 5";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::Assignment(assign) => {
                assert!(matches!(assign.operator, AssignmentOperator::AddAssign));
            }
            _ => panic!("Expected assignment expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_conditional_expression() {
    let source = "x ? 1 : 2";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::Conditional(cond) => {
                match &*cond.test {
                    Expression::Identifier(id) => assert_eq!(interner.resolve(id.name), "x"),
                    _ => panic!("Expected identifier"),
                }
                match &*cond.consequent {
                    Expression::IntLiteral(lit) => assert_eq!(lit.value, 1),
                    Expression::FloatLiteral(lit) => assert_eq!(lit.value, 1.0),
                    _ => panic!("Expected literal"),
                }
                match &*cond.alternate {
                    Expression::IntLiteral(lit) => assert_eq!(lit.value, 2),
                    Expression::FloatLiteral(lit) => assert_eq!(lit.value, 2.0),
                    _ => panic!("Expected literal"),
                }
            }
            _ => panic!("Expected conditional expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_logical_operators() {
    let source = "a && b";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::Logical(logical) => {
                assert!(matches!(logical.operator, LogicalOperator::And));
            }
            _ => panic!("Expected logical expression"),
        },
        _ => panic!("Expected expression statement"),
    }

    let source = "a || b";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::Logical(logical) => {
                assert!(matches!(logical.operator, LogicalOperator::Or));
            }
            _ => panic!("Expected logical expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_nullish_coalescing() {
    let source = "a ?? b";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::Logical(logical) => {
                assert!(matches!(
                    logical.operator,
                    LogicalOperator::NullishCoalescing
                ));
            }
            _ => panic!("Expected logical expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_comparison_operators() {
    let tests = vec![
        ("a < b", BinaryOperator::LessThan),
        ("a <= b", BinaryOperator::LessEqual),
        ("a > b", BinaryOperator::GreaterThan),
        ("a >= b", BinaryOperator::GreaterEqual),
        ("a == b", BinaryOperator::Equal),
        ("a != b", BinaryOperator::NotEqual),
        ("a === b", BinaryOperator::StrictEqual),
        ("a !== b", BinaryOperator::StrictNotEqual),
    ];

    for (source, expected_op) in tests {
        let parser = Parser::new(source).unwrap();
        let (module, _interner) = parser.parse().unwrap();

        match &module.statements[0] {
            Statement::Expression(expr_stmt) => match &expr_stmt.expression {
                Expression::Binary(bin) => {
                    assert!(
                        std::mem::discriminant(&bin.operator)
                            == std::mem::discriminant(&expected_op),
                        "Failed for: {}",
                        source
                    );
                }
                _ => panic!("Expected binary expression for: {}", source),
            },
            _ => panic!("Expected expression statement for: {}", source),
        }
    }
}

#[test]
fn test_parse_grouped_expression() {
    let source = "(1 + 2)";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::Binary(bin) => {
                assert!(matches!(bin.operator, BinaryOperator::Add));
            }
            _ => panic!("Expected binary expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_complex_precedence() {
    let source = "a + b * c - d / e";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    // Should parse as: (a + (b * c)) - (d / e)
    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::Binary(bin) => {
                assert!(matches!(bin.operator, BinaryOperator::Subtract));
                // Left side: a + (b * c)
                match &*bin.left {
                    Expression::Binary(left_bin) => {
                        assert!(matches!(left_bin.operator, BinaryOperator::Add));
                    }
                    _ => panic!("Expected binary expression on left"),
                }
                // Right side: d / e
                match &*bin.right {
                    Expression::Binary(right_bin) => {
                        assert!(matches!(right_bin.operator, BinaryOperator::Divide));
                    }
                    _ => panic!("Expected binary expression on right"),
                }
            }
            _ => panic!("Expected binary expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}
#[test]
fn test_parse_left_associativity() {
    let source = "1 + 2 + 3";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    // Should parse as (1 + 2) + 3
    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::Binary(outer) => {
                // Outer is ... + 3
                assert!(matches!(outer.operator, BinaryOperator::Add));
                match &*outer.right {
                    Expression::IntLiteral(lit) => assert_eq!(lit.value, 3),
                    Expression::FloatLiteral(lit) => assert_eq!(lit.value, 3.0),
                    _ => panic!("Expected 3"),
                }
                // Left is (1 + 2)
                match &*outer.left {
                    Expression::Binary(inner) => {
                        assert!(matches!(inner.operator, BinaryOperator::Add));
                        match &*inner.left {
                            Expression::IntLiteral(lit) => assert_eq!(lit.value, 1),
                            Expression::FloatLiteral(lit) => assert_eq!(lit.value, 1.0),
                            _ => panic!("Expected 1"),
                        }
                        match &*inner.right {
                            Expression::IntLiteral(lit) => assert_eq!(lit.value, 2),
                            Expression::FloatLiteral(lit) => assert_eq!(lit.value, 2.0),
                            _ => panic!("Expected 2"),
                        }
                    }
                    _ => panic!("Expected binary expression for 1 + 2"),
                }
            }
            _ => panic!("Expected binary expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}

#[test]
fn test_parse_right_associativity() {
    let source = "2 ** 3 ** 4";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    // Should parse as 2 ** (3 ** 4)
    match &module.statements[0] {
        Statement::Expression(expr_stmt) => match &expr_stmt.expression {
            Expression::Binary(outer) => {
                // Outer is 2 ** ...
                assert!(matches!(outer.operator, BinaryOperator::Exponent));
                match &*outer.left {
                    Expression::IntLiteral(lit) => assert_eq!(lit.value, 2),
                    Expression::FloatLiteral(lit) => assert_eq!(lit.value, 2.0),
                    _ => panic!("Expected 2"),
                }
                // Right is (3 ** 4)
                match &*outer.right {
                    Expression::Binary(inner) => {
                        assert!(matches!(inner.operator, BinaryOperator::Exponent));
                        match &*inner.left {
                            Expression::IntLiteral(lit) => assert_eq!(lit.value, 3),
                            Expression::FloatLiteral(lit) => assert_eq!(lit.value, 3.0),
                            _ => panic!("Expected 3"),
                        }
                        match &*inner.right {
                            Expression::IntLiteral(lit) => assert_eq!(lit.value, 4),
                            Expression::FloatLiteral(lit) => assert_eq!(lit.value, 4.0),
                            _ => panic!("Expected 4"),
                        }
                    }
                    _ => panic!("Expected binary expression for 3 ** 4"),
                }
            }
            _ => panic!("Expected binary expression"),
        },
        _ => panic!("Expected expression statement"),
    }
}
