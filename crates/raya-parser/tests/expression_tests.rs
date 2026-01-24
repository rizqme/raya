use raya_parser::ast::*;
use raya_parser::token::Span;

// ============================================================================
// Literal Expression Tests
// ============================================================================

#[test]
fn test_int_literal() {
    let lit = IntLiteral {
        value: 42,
        span: Span::new(0, 2, 1, 1),
    };

    let expr = Expression::IntLiteral(lit);
    assert!(expr.is_literal());
    assert!(!expr.is_identifier());
    assert!(!expr.is_binary());
}

#[test]
fn test_float_literal() {
    let lit = FloatLiteral {
        value: 3.14,
        span: Span::new(0, 4, 1, 1),
    };

    let expr = Expression::FloatLiteral(lit);
    assert!(expr.is_literal());
}

#[test]
fn test_string_literal() {
    let lit = StringLiteral {
        value: "hello".to_string(),
        span: Span::new(0, 7, 1, 1),
    };

    let expr = Expression::StringLiteral(lit);
    assert!(expr.is_literal());
}

#[test]
fn test_boolean_literal() {
    let lit = BooleanLiteral {
        value: true,
        span: Span::new(0, 4, 1, 1),
    };

    let expr = Expression::BooleanLiteral(lit);
    assert!(expr.is_literal());
}

#[test]
fn test_null_literal() {
    let expr = Expression::NullLiteral(Span::new(0, 4, 1, 1));
    assert!(expr.is_literal());
}

#[test]
fn test_template_literal() {
    let lit = TemplateLiteral {
        parts: vec![
            TemplatePart::String("Hello, ".to_string()),
            TemplatePart::Expression(Box::new(Expression::Identifier(
                Identifier::new("name".to_string(), Span::new(9, 13, 1, 10))
            ))),
            TemplatePart::String("!".to_string()),
        ],
        span: Span::new(0, 16, 1, 1),
    };

    let expr = Expression::TemplateLiteral(lit);
    assert!(expr.is_literal());
}

// ============================================================================
// Array and Object Expression Tests
// ============================================================================

#[test]
fn test_array_expression() {
    let arr = ArrayExpression {
        elements: vec![
            Some(Expression::IntLiteral(IntLiteral {
                value: 1,
                span: Span::new(1, 2, 1, 2),
            })),
            Some(Expression::IntLiteral(IntLiteral {
                value: 2,
                span: Span::new(4, 5, 1, 5),
            })),
            None, // Sparse array: [1, 2, , 3]
            Some(Expression::IntLiteral(IntLiteral {
                value: 3,
                span: Span::new(9, 10, 1, 10),
            })),
        ],
        span: Span::new(0, 11, 1, 1),
    };

    let expr = Expression::Array(arr);
    assert!(expr.is_literal());
    assert_eq!(expr.span().start, 0);
}

#[test]
fn test_object_expression() {
    let obj = ObjectExpression {
        properties: vec![
            ObjectProperty::Property(Property {
                key: PropertyKey::Identifier(Identifier::new("x".to_string(), Span::new(2, 3, 1, 3))),
                value: Expression::IntLiteral(IntLiteral {
                    value: 1,
                    span: Span::new(5, 6, 1, 6),
                }),
                span: Span::new(2, 6, 1, 3),
            }),
            ObjectProperty::Property(Property {
                key: PropertyKey::StringLiteral(StringLiteral {
                    value: "y".to_string(),
                    span: Span::new(8, 11, 1, 9),
                }),
                value: Expression::IntLiteral(IntLiteral {
                    value: 2,
                    span: Span::new(13, 14, 1, 14),
                }),
                span: Span::new(8, 14, 1, 9),
            }),
        ],
        span: Span::new(0, 16, 1, 1),
    };

    let expr = Expression::Object(obj);
    assert!(expr.is_literal());
}

#[test]
fn test_object_with_spread() {
    let obj = ObjectExpression {
        properties: vec![
            ObjectProperty::Spread(SpreadProperty {
                argument: Expression::Identifier(Identifier::new("other".to_string(), Span::new(5, 10, 1, 6))),
                span: Span::new(2, 10, 1, 3),
            }),
            ObjectProperty::Property(Property {
                key: PropertyKey::Identifier(Identifier::new("x".to_string(), Span::new(12, 13, 1, 13))),
                value: Expression::IntLiteral(IntLiteral {
                    value: 42,
                    span: Span::new(15, 17, 1, 16),
                }),
                span: Span::new(12, 17, 1, 13),
            }),
        ],
        span: Span::new(0, 19, 1, 1),
    };

    assert_eq!(obj.properties.len(), 2);
}

// ============================================================================
// Identifier Tests
// ============================================================================

#[test]
fn test_identifier_expression() {
    let id = Identifier::new("foo".to_string(), Span::new(0, 3, 1, 1));
    let expr = Expression::Identifier(id);

    assert!(!expr.is_literal());
    assert!(expr.is_identifier());
    assert!(!expr.is_binary());
}

// ============================================================================
// Unary Expression Tests
// ============================================================================

#[test]
fn test_unary_not() {
    let unary = UnaryExpression {
        operator: UnaryOperator::Not,
        operand: Box::new(Expression::BooleanLiteral(BooleanLiteral {
            value: true,
            span: Span::new(1, 5, 1, 2),
        })),
        span: Span::new(0, 5, 1, 1),
    };

    let expr = Expression::Unary(unary);
    assert!(!expr.is_literal());
    assert!(!expr.is_binary());
}

#[test]
fn test_unary_minus() {
    let unary = UnaryExpression {
        operator: UnaryOperator::Minus,
        operand: Box::new(Expression::IntLiteral(IntLiteral {
            value: 42,
            span: Span::new(1, 3, 1, 2),
        })),
        span: Span::new(0, 3, 1, 1),
    };

    assert_eq!(unary.operator, UnaryOperator::Minus);
}

#[test]
fn test_unary_increment() {
    let prefix = UnaryExpression {
        operator: UnaryOperator::PrefixIncrement,
        operand: Box::new(Expression::Identifier(Identifier::new("x".to_string(), Span::new(2, 3, 1, 3)))),
        span: Span::new(0, 3, 1, 1),
    };

    let postfix = UnaryExpression {
        operator: UnaryOperator::PostfixIncrement,
        operand: Box::new(Expression::Identifier(Identifier::new("x".to_string(), Span::new(0, 1, 1, 1)))),
        span: Span::new(0, 3, 1, 1),
    };

    assert_eq!(prefix.operator, UnaryOperator::PrefixIncrement);
    assert_eq!(postfix.operator, UnaryOperator::PostfixIncrement);
}

// ============================================================================
// Binary Expression Tests
// ============================================================================

#[test]
fn test_binary_add() {
    let binary = BinaryExpression {
        operator: BinaryOperator::Add,
        left: Box::new(Expression::IntLiteral(IntLiteral {
            value: 1,
            span: Span::new(0, 1, 1, 1),
        })),
        right: Box::new(Expression::IntLiteral(IntLiteral {
            value: 2,
            span: Span::new(4, 5, 1, 5),
        })),
        span: Span::new(0, 5, 1, 1),
    };

    let expr = Expression::Binary(binary);
    assert!(expr.is_binary());
    assert!(!expr.is_literal());
}

#[test]
fn test_binary_comparison() {
    let binary = BinaryExpression {
        operator: BinaryOperator::LessThan,
        left: Box::new(Expression::Identifier(Identifier::new("x".to_string(), Span::new(0, 1, 1, 1)))),
        right: Box::new(Expression::IntLiteral(IntLiteral {
            value: 10,
            span: Span::new(4, 6, 1, 5),
        })),
        span: Span::new(0, 6, 1, 1),
    };

    assert_eq!(binary.operator, BinaryOperator::LessThan);
}

#[test]
fn test_binary_exponent() {
    let binary = BinaryExpression {
        operator: BinaryOperator::Exponent,
        left: Box::new(Expression::IntLiteral(IntLiteral {
            value: 2,
            span: Span::new(0, 1, 1, 1),
        })),
        right: Box::new(Expression::IntLiteral(IntLiteral {
            value: 3,
            span: Span::new(5, 6, 1, 6),
        })),
        span: Span::new(0, 6, 1, 1),
    };

    assert_eq!(binary.operator, BinaryOperator::Exponent);
}

// ============================================================================
// Logical Expression Tests
// ============================================================================

#[test]
fn test_logical_and() {
    let logical = LogicalExpression {
        operator: LogicalOperator::And,
        left: Box::new(Expression::BooleanLiteral(BooleanLiteral {
            value: true,
            span: Span::new(0, 4, 1, 1),
        })),
        right: Box::new(Expression::BooleanLiteral(BooleanLiteral {
            value: false,
            span: Span::new(8, 13, 1, 9),
        })),
        span: Span::new(0, 13, 1, 1),
    };

    let expr = Expression::Logical(logical);
    assert!(expr.is_binary());
}

#[test]
fn test_logical_or() {
    let logical = LogicalExpression {
        operator: LogicalOperator::Or,
        left: Box::new(Expression::Identifier(Identifier::new("a".to_string(), Span::new(0, 1, 1, 1)))),
        right: Box::new(Expression::Identifier(Identifier::new("b".to_string(), Span::new(5, 6, 1, 6)))),
        span: Span::new(0, 6, 1, 1),
    };

    assert_eq!(logical.operator, LogicalOperator::Or);
}

#[test]
fn test_nullish_coalescing() {
    let logical = LogicalExpression {
        operator: LogicalOperator::NullishCoalescing,
        left: Box::new(Expression::Identifier(Identifier::new("x".to_string(), Span::new(0, 1, 1, 1)))),
        right: Box::new(Expression::IntLiteral(IntLiteral {
            value: 0,
            span: Span::new(5, 6, 1, 6),
        })),
        span: Span::new(0, 6, 1, 1),
    };

    assert_eq!(logical.operator, LogicalOperator::NullishCoalescing);
}

// ============================================================================
// Assignment Expression Tests
// ============================================================================

#[test]
fn test_assignment_simple() {
    let assign = AssignmentExpression {
        operator: AssignmentOperator::Assign,
        left: Box::new(Expression::Identifier(Identifier::new("x".to_string(), Span::new(0, 1, 1, 1)))),
        right: Box::new(Expression::IntLiteral(IntLiteral {
            value: 42,
            span: Span::new(4, 6, 1, 5),
        })),
        span: Span::new(0, 6, 1, 1),
    };

    assert_eq!(assign.operator, AssignmentOperator::Assign);
}

#[test]
fn test_assignment_compound() {
    let assign = AssignmentExpression {
        operator: AssignmentOperator::AddAssign,
        left: Box::new(Expression::Identifier(Identifier::new("x".to_string(), Span::new(0, 1, 1, 1)))),
        right: Box::new(Expression::IntLiteral(IntLiteral {
            value: 1,
            span: Span::new(5, 6, 1, 6),
        })),
        span: Span::new(0, 6, 1, 1),
    };

    assert_eq!(assign.operator, AssignmentOperator::AddAssign);
}

// ============================================================================
// Conditional Expression Tests
// ============================================================================

#[test]
fn test_conditional_ternary() {
    let cond = ConditionalExpression {
        test: Box::new(Expression::BooleanLiteral(BooleanLiteral {
            value: true,
            span: Span::new(0, 4, 1, 1),
        })),
        consequent: Box::new(Expression::IntLiteral(IntLiteral {
            value: 1,
            span: Span::new(7, 8, 1, 8),
        })),
        alternate: Box::new(Expression::IntLiteral(IntLiteral {
            value: 2,
            span: Span::new(11, 12, 1, 12),
        })),
        span: Span::new(0, 12, 1, 1),
    };

    let expr = Expression::Conditional(cond);
    assert_eq!(expr.span().start, 0);
}

// ============================================================================
// Call Expression Tests
// ============================================================================

#[test]
fn test_call_expression() {
    let call = CallExpression {
        callee: Box::new(Expression::Identifier(Identifier::new("foo".to_string(), Span::new(0, 3, 1, 1)))),
        type_args: None,
        arguments: vec![
            Expression::IntLiteral(IntLiteral {
                value: 1,
                span: Span::new(4, 5, 1, 5),
            }),
            Expression::IntLiteral(IntLiteral {
                value: 2,
                span: Span::new(7, 8, 1, 8),
            }),
        ],
        span: Span::new(0, 9, 1, 1),
    };

    assert_eq!(call.arguments.len(), 2);
}

#[test]
fn test_call_with_type_args() {
    let call = CallExpression {
        callee: Box::new(Expression::Identifier(Identifier::new("foo".to_string(), Span::new(0, 3, 1, 1)))),
        type_args: Some(vec![
            TypeAnnotation {
                ty: Type::Primitive(PrimitiveType::Number),
                span: Span::new(4, 10, 1, 5),
            },
        ]),
        arguments: vec![],
        span: Span::new(0, 13, 1, 1),
    };

    assert!(call.type_args.is_some());
}

// ============================================================================
// Member Expression Tests
// ============================================================================

#[test]
fn test_member_access() {
    let member = MemberExpression {
        object: Box::new(Expression::Identifier(Identifier::new("obj".to_string(), Span::new(0, 3, 1, 1)))),
        property: Identifier::new("prop".to_string(), Span::new(4, 8, 1, 5)),
        optional: false,
        span: Span::new(0, 8, 1, 1),
    };

    assert!(!member.optional);
}

#[test]
fn test_optional_member_access() {
    let member = MemberExpression {
        object: Box::new(Expression::Identifier(Identifier::new("obj".to_string(), Span::new(0, 3, 1, 1)))),
        property: Identifier::new("prop".to_string(), Span::new(5, 9, 1, 6)),
        optional: true,
        span: Span::new(0, 9, 1, 1),
    };

    assert!(member.optional);
}

// ============================================================================
// Index Expression Tests
// ============================================================================

#[test]
fn test_index_access() {
    let index = IndexExpression {
        object: Box::new(Expression::Identifier(Identifier::new("arr".to_string(), Span::new(0, 3, 1, 1)))),
        index: Box::new(Expression::IntLiteral(IntLiteral {
            value: 0,
            span: Span::new(4, 5, 1, 5),
        })),
        span: Span::new(0, 6, 1, 1),
    };

    let expr = Expression::Index(index);
    assert_eq!(expr.span().start, 0);
}

// ============================================================================
// New Expression Tests
// ============================================================================

#[test]
fn test_new_expression() {
    let new_expr = NewExpression {
        callee: Box::new(Expression::Identifier(Identifier::new("Point".to_string(), Span::new(4, 9, 1, 5)))),
        type_args: None,
        arguments: vec![
            Expression::IntLiteral(IntLiteral {
                value: 1,
                span: Span::new(10, 11, 1, 11),
            }),
            Expression::IntLiteral(IntLiteral {
                value: 2,
                span: Span::new(13, 14, 1, 14),
            }),
        ],
        span: Span::new(0, 15, 1, 1),
    };

    assert_eq!(new_expr.arguments.len(), 2);
}

// ============================================================================
// Arrow Function Tests
// ============================================================================

#[test]
fn test_arrow_function_expression() {
    let arrow = ArrowFunction {
        params: vec![
            Parameter {
                pattern: Pattern::Identifier(Identifier::new("x".to_string(), Span::new(1, 2, 1, 2))),
                type_annotation: Some(TypeAnnotation {
                    ty: Type::Primitive(PrimitiveType::Number),
                    span: Span::new(4, 10, 1, 5),
                }),
                span: Span::new(1, 10, 1, 2),
            },
        ],
        return_type: None,
        body: ArrowBody::Expression(Box::new(Expression::Binary(BinaryExpression {
            operator: BinaryOperator::Add,
            left: Box::new(Expression::Identifier(Identifier::new("x".to_string(), Span::new(15, 16, 1, 16)))),
            right: Box::new(Expression::IntLiteral(IntLiteral {
                value: 1,
                span: Span::new(19, 20, 1, 20),
            })),
            span: Span::new(15, 20, 1, 16),
        }))),
        is_async: false,
        span: Span::new(0, 20, 1, 1),
    };

    assert_eq!(arrow.params.len(), 1);
    assert!(!arrow.is_async);
}

#[test]
fn test_arrow_function_block() {
    let arrow = ArrowFunction {
        params: vec![],
        return_type: None,
        body: ArrowBody::Block(BlockStatement {
            statements: vec![],
            span: Span::new(6, 8, 1, 7),
        }),
        is_async: false,
        span: Span::new(0, 8, 1, 1),
    };

    assert!(matches!(arrow.body, ArrowBody::Block(_)));
}

// ============================================================================
// Await Expression Tests
// ============================================================================

#[test]
fn test_await_expression() {
    let await_expr = AwaitExpression {
        argument: Box::new(Expression::Identifier(Identifier::new("promise".to_string(), Span::new(6, 13, 1, 7)))),
        span: Span::new(0, 13, 1, 1),
    };

    let expr = Expression::Await(await_expr);
    assert_eq!(expr.span().start, 0);
}

// ============================================================================
// Typeof Expression Tests
// ============================================================================

#[test]
fn test_typeof_expression() {
    let typeof_expr = TypeofExpression {
        argument: Box::new(Expression::Identifier(Identifier::new("value".to_string(), Span::new(7, 12, 1, 8)))),
        span: Span::new(0, 12, 1, 1),
    };

    let expr = Expression::Typeof(typeof_expr);
    assert_eq!(expr.span().start, 0);
}

// ============================================================================
// Parenthesized Expression Tests
// ============================================================================

#[test]
fn test_parenthesized_expression() {
    let paren = ParenthesizedExpression {
        expression: Box::new(Expression::IntLiteral(IntLiteral {
            value: 42,
            span: Span::new(1, 3, 1, 2),
        })),
        span: Span::new(0, 4, 1, 1),
    };

    let expr = Expression::Parenthesized(paren);
    assert_eq!(expr.span().start, 0);
}

// ============================================================================
// Expression Helper Methods Tests
// ============================================================================

#[test]
fn test_is_literal() {
    assert!(Expression::IntLiteral(IntLiteral {
        value: 42,
        span: Span::new(0, 2, 1, 1),
    }).is_literal());

    assert!(Expression::NullLiteral(Span::new(0, 4, 1, 1)).is_literal());

    assert!(!Expression::Identifier(Identifier::new("x".to_string(), Span::new(0, 1, 1, 1))).is_literal());
}

#[test]
fn test_is_identifier() {
    assert!(Expression::Identifier(Identifier::new("foo".to_string(), Span::new(0, 3, 1, 1))).is_identifier());

    assert!(!Expression::IntLiteral(IntLiteral {
        value: 42,
        span: Span::new(0, 2, 1, 1),
    }).is_identifier());
}

#[test]
fn test_is_binary() {
    assert!(Expression::Binary(BinaryExpression {
        operator: BinaryOperator::Add,
        left: Box::new(Expression::IntLiteral(IntLiteral {
            value: 1,
            span: Span::new(0, 1, 1, 1),
        })),
        right: Box::new(Expression::IntLiteral(IntLiteral {
            value: 2,
            span: Span::new(4, 5, 1, 5),
        })),
        span: Span::new(0, 5, 1, 1),
    }).is_binary());

    assert!(Expression::Logical(LogicalExpression {
        operator: LogicalOperator::And,
        left: Box::new(Expression::BooleanLiteral(BooleanLiteral {
            value: true,
            span: Span::new(0, 4, 1, 1),
        })),
        right: Box::new(Expression::BooleanLiteral(BooleanLiteral {
            value: false,
            span: Span::new(8, 13, 1, 9),
        })),
        span: Span::new(0, 13, 1, 1),
    }).is_binary());

    assert!(!Expression::IntLiteral(IntLiteral {
        value: 42,
        span: Span::new(0, 2, 1, 1),
    }).is_binary());
}
