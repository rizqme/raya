use raya_parser::ast::*;
use raya_parser::token::Span;

// ============================================================================
// JSX Element Tests
// ============================================================================

#[test]
fn test_jsx_self_closing_element() {
    let element = JsxElement {
        opening: JsxOpeningElement {
            name: JsxElementName::Identifier(Identifier::new("img".to_string(), Span::new(1, 4, 1, 2))),
            attributes: vec![
                JsxAttribute::Attribute {
                    name: JsxAttributeName::Identifier(Identifier::new("src".to_string(), Span::new(5, 8, 1, 6))),
                    value: Some(JsxAttributeValue::StringLiteral(StringLiteral {
                        value: "photo.jpg".to_string(),
                        span: Span::new(9, 20, 1, 10),
                    })),
                    span: Span::new(5, 20, 1, 6),
                },
            ],
            self_closing: true,
            span: Span::new(0, 23, 1, 1),
        },
        children: vec![],
        closing: None,
        span: Span::new(0, 23, 1, 1),
    };

    assert!(element.opening.self_closing);
    assert!(element.closing.is_none());
    assert_eq!(element.children.len(), 0);
}

#[test]
fn test_jsx_element_with_children() {
    let element = JsxElement {
        opening: JsxOpeningElement {
            name: JsxElementName::Identifier(Identifier::new("div".to_string(), Span::new(1, 4, 1, 2))),
            attributes: vec![],
            self_closing: false,
            span: Span::new(0, 5, 1, 1),
        },
        children: vec![JsxChild::Text(JsxText {
            value: "Hello World".to_string(),
            raw: "Hello World".to_string(),
            span: Span::new(5, 16, 1, 6),
        })],
        closing: Some(JsxClosingElement {
            name: JsxElementName::Identifier(Identifier::new("div".to_string(), Span::new(19, 22, 1, 20))),
            span: Span::new(16, 23, 1, 17),
        }),
        span: Span::new(0, 23, 1, 1),
    };

    assert!(!element.opening.self_closing);
    assert!(element.closing.is_some());
    assert_eq!(element.children.len(), 1);
}

#[test]
fn test_jsx_element_name_is_intrinsic() {
    let div_name = JsxElementName::Identifier(Identifier::new("div".to_string(), Span::new(0, 3, 1, 1)));
    let button_name = JsxElementName::Identifier(Identifier::new("Button".to_string(), Span::new(0, 6, 1, 1)));

    assert!(div_name.is_intrinsic());
    assert!(!button_name.is_intrinsic());
}

#[test]
fn test_jsx_element_name_to_string() {
    // Simple identifier
    let simple = JsxElementName::Identifier(Identifier::new("div".to_string(), Span::new(0, 3, 1, 1)));
    assert_eq!(simple.to_string(), "div");

    // Namespaced
    let namespaced = JsxElementName::Namespaced {
        namespace: Identifier::new("svg".to_string(), Span::new(0, 3, 1, 1)),
        name: Identifier::new("path".to_string(), Span::new(4, 8, 1, 5)),
    };
    assert_eq!(namespaced.to_string(), "svg:path");

    // Member expression
    let member = JsxElementName::MemberExpression {
        object: Box::new(JsxElementName::Identifier(Identifier::new(
            "React".to_string(),
            Span::new(0, 5, 1, 1),
        ))),
        property: Identifier::new("Fragment".to_string(), Span::new(6, 14, 1, 7)),
    };
    assert_eq!(member.to_string(), "React.Fragment");
}

#[test]
fn test_jsx_nested_member_expression() {
    // UI.Components.Button
    let nested = JsxElementName::MemberExpression {
        object: Box::new(JsxElementName::MemberExpression {
            object: Box::new(JsxElementName::Identifier(Identifier::new(
                "UI".to_string(),
                Span::new(0, 2, 1, 1),
            ))),
            property: Identifier::new("Components".to_string(), Span::new(3, 13, 1, 4)),
        }),
        property: Identifier::new("Button".to_string(), Span::new(14, 20, 1, 15)),
    };

    assert_eq!(nested.to_string(), "UI.Components.Button");
}

// ============================================================================
// JSX Attribute Tests
// ============================================================================

#[test]
fn test_jsx_string_attribute() {
    let attr = JsxAttribute::Attribute {
        name: JsxAttributeName::Identifier(Identifier::new("className".to_string(), Span::new(0, 9, 1, 1))),
        value: Some(JsxAttributeValue::StringLiteral(StringLiteral {
            value: "container".to_string(),
            span: Span::new(10, 21, 1, 11),
        })),
        span: Span::new(0, 21, 1, 1),
    };

    if let JsxAttribute::Attribute { value, .. } = attr {
        assert!(value.is_some());
        if let Some(JsxAttributeValue::StringLiteral(s)) = value {
            assert_eq!(s.value, "container");
        }
    }
}

#[test]
fn test_jsx_expression_attribute() {
    let attr = JsxAttribute::Attribute {
        name: JsxAttributeName::Identifier(Identifier::new("value".to_string(), Span::new(0, 5, 1, 1))),
        value: Some(JsxAttributeValue::Expression(Expression::Identifier(
            Identifier::new("text".to_string(), Span::new(7, 11, 1, 8)),
        ))),
        span: Span::new(0, 12, 1, 1),
    };

    if let JsxAttribute::Attribute { value, .. } = attr {
        assert!(value.is_some());
    }
}

#[test]
fn test_jsx_boolean_attribute() {
    // Boolean attribute with no value: <button disabled />
    let attr = JsxAttribute::Attribute {
        name: JsxAttributeName::Identifier(Identifier::new("disabled".to_string(), Span::new(0, 8, 1, 1))),
        value: None,
        span: Span::new(0, 8, 1, 1),
    };

    if let JsxAttribute::Attribute { value, .. } = attr {
        assert!(value.is_none());
    }
}

#[test]
fn test_jsx_spread_attribute() {
    let attr = JsxAttribute::Spread {
        argument: Expression::Identifier(Identifier::new("props".to_string(), Span::new(4, 9, 1, 5))),
        span: Span::new(0, 10, 1, 1),
    };

    assert!(matches!(attr, JsxAttribute::Spread { .. }));
}

#[test]
fn test_jsx_namespaced_attribute() {
    let attr = JsxAttribute::Attribute {
        name: JsxAttributeName::Namespaced {
            namespace: Identifier::new("xml".to_string(), Span::new(0, 3, 1, 1)),
            name: Identifier::new("lang".to_string(), Span::new(4, 8, 1, 5)),
        },
        value: Some(JsxAttributeValue::StringLiteral(StringLiteral {
            value: "en".to_string(),
            span: Span::new(9, 13, 1, 10),
        })),
        span: Span::new(0, 13, 1, 1),
    };

    if let JsxAttribute::Attribute { name, .. } = attr {
        assert!(matches!(name, JsxAttributeName::Namespaced { .. }));
    }
}

// ============================================================================
// JSX Children Tests
// ============================================================================

#[test]
fn test_jsx_text_child() {
    let child = JsxChild::Text(JsxText {
        value: "Hello World".to_string(),
        raw: "Hello World".to_string(),
        span: Span::new(0, 11, 1, 1),
    });

    if let JsxChild::Text(text) = child {
        assert_eq!(text.value, "Hello World");
        assert_eq!(text.raw, "Hello World");
    }
}

#[test]
fn test_jsx_element_child() {
    let child = JsxChild::Element(JsxElement {
        opening: JsxOpeningElement {
            name: JsxElementName::Identifier(Identifier::new("span".to_string(), Span::new(1, 5, 1, 2))),
            attributes: vec![],
            self_closing: false,
            span: Span::new(0, 6, 1, 1),
        },
        children: vec![],
        closing: Some(JsxClosingElement {
            name: JsxElementName::Identifier(Identifier::new("span".to_string(), Span::new(9, 13, 1, 10))),
            span: Span::new(6, 14, 1, 7),
        }),
        span: Span::new(0, 14, 1, 1),
    });

    assert!(matches!(child, JsxChild::Element(_)));
}

#[test]
fn test_jsx_expression_child() {
    let child = JsxChild::Expression(JsxExpression {
        expression: Some(Expression::Identifier(Identifier::new(
            "name".to_string(),
            Span::new(1, 5, 1, 2),
        ))),
        span: Span::new(0, 6, 1, 1),
    });

    if let JsxChild::Expression(expr) = child {
        assert!(expr.expression.is_some());
    }
}

#[test]
fn test_jsx_empty_expression() {
    // Empty expression: {/* comment */} or just {}
    let child = JsxChild::Expression(JsxExpression {
        expression: None,
        span: Span::new(0, 2, 1, 1),
    });

    if let JsxChild::Expression(expr) = child {
        assert!(expr.expression.is_none());
    }
}

#[test]
fn test_jsx_fragment_child() {
    let child = JsxChild::Fragment(JsxFragment {
        opening: JsxOpeningFragment {
            span: Span::new(0, 2, 1, 1),
        },
        children: vec![],
        closing: JsxClosingFragment {
            span: Span::new(2, 5, 1, 3),
        },
        span: Span::new(0, 5, 1, 1),
    });

    assert!(matches!(child, JsxChild::Fragment(_)));
}

// ============================================================================
// JSX Fragment Tests
// ============================================================================

#[test]
fn test_jsx_empty_fragment() {
    let fragment = JsxFragment {
        opening: JsxOpeningFragment {
            span: Span::new(0, 2, 1, 1),
        },
        children: vec![],
        closing: JsxClosingFragment {
            span: Span::new(2, 5, 1, 3),
        },
        span: Span::new(0, 5, 1, 1),
    };

    assert_eq!(fragment.children.len(), 0);
}

#[test]
fn test_jsx_fragment_with_children() {
    let fragment = JsxFragment {
        opening: JsxOpeningFragment {
            span: Span::new(0, 2, 1, 1),
        },
        children: vec![
            JsxChild::Element(JsxElement {
                opening: JsxOpeningElement {
                    name: JsxElementName::Identifier(Identifier::new("div".to_string(), Span::new(3, 6, 2, 2))),
                    attributes: vec![],
                    self_closing: false,
                    span: Span::new(2, 7, 2, 1),
                },
                children: vec![],
                closing: Some(JsxClosingElement {
                    name: JsxElementName::Identifier(Identifier::new("div".to_string(), Span::new(10, 13, 2, 9))),
                    span: Span::new(7, 14, 2, 8),
                }),
                span: Span::new(2, 14, 2, 1),
            }),
            JsxChild::Element(JsxElement {
                opening: JsxOpeningElement {
                    name: JsxElementName::Identifier(Identifier::new("div".to_string(), Span::new(16, 19, 3, 2))),
                    attributes: vec![],
                    self_closing: false,
                    span: Span::new(15, 20, 3, 1),
                },
                children: vec![],
                closing: Some(JsxClosingElement {
                    name: JsxElementName::Identifier(Identifier::new("div".to_string(), Span::new(23, 26, 3, 9))),
                    span: Span::new(20, 27, 3, 8),
                }),
                span: Span::new(15, 27, 3, 1),
            }),
        ],
        closing: JsxClosingFragment {
            span: Span::new(27, 30, 4, 1),
        },
        span: Span::new(0, 30, 1, 1),
    };

    assert_eq!(fragment.children.len(), 2);
}

// ============================================================================
// Complex JSX Tests
// ============================================================================

#[test]
fn test_jsx_nested_elements() {
    let element = JsxElement {
        opening: JsxOpeningElement {
            name: JsxElementName::Identifier(Identifier::new("div".to_string(), Span::new(1, 4, 1, 2))),
            attributes: vec![],
            self_closing: false,
            span: Span::new(0, 5, 1, 1),
        },
        children: vec![
            JsxChild::Element(JsxElement {
                opening: JsxOpeningElement {
                    name: JsxElementName::Identifier(Identifier::new("h1".to_string(), Span::new(7, 9, 2, 3))),
                    attributes: vec![],
                    self_closing: false,
                    span: Span::new(6, 10, 2, 2),
                },
                children: vec![JsxChild::Text(JsxText {
                    value: "Title".to_string(),
                    raw: "Title".to_string(),
                    span: Span::new(10, 15, 2, 6),
                })],
                closing: Some(JsxClosingElement {
                    name: JsxElementName::Identifier(Identifier::new("h1".to_string(), Span::new(18, 20, 2, 14))),
                    span: Span::new(15, 21, 2, 11),
                }),
                span: Span::new(6, 21, 2, 2),
            }),
            JsxChild::Element(JsxElement {
                opening: JsxOpeningElement {
                    name: JsxElementName::Identifier(Identifier::new("p".to_string(), Span::new(23, 24, 3, 3))),
                    attributes: vec![],
                    self_closing: false,
                    span: Span::new(22, 25, 3, 2),
                },
                children: vec![JsxChild::Text(JsxText {
                    value: "Content".to_string(),
                    raw: "Content".to_string(),
                    span: Span::new(25, 32, 3, 5),
                })],
                closing: Some(JsxClosingElement {
                    name: JsxElementName::Identifier(Identifier::new("p".to_string(), Span::new(35, 36, 3, 15))),
                    span: Span::new(32, 37, 3, 12),
                }),
                span: Span::new(22, 37, 3, 2),
            }),
        ],
        closing: Some(JsxClosingElement {
            name: JsxElementName::Identifier(Identifier::new("div".to_string(), Span::new(41, 44, 4, 4))),
            span: Span::new(37, 45, 4, 1),
        }),
        span: Span::new(0, 45, 1, 1),
    };

    assert_eq!(element.children.len(), 2);
}

#[test]
fn test_jsx_mixed_children() {
    // <div>Text {expr} more text</div>
    let element = JsxElement {
        opening: JsxOpeningElement {
            name: JsxElementName::Identifier(Identifier::new("div".to_string(), Span::new(1, 4, 1, 2))),
            attributes: vec![],
            self_closing: false,
            span: Span::new(0, 5, 1, 1),
        },
        children: vec![
            JsxChild::Text(JsxText {
                value: "Text ".to_string(),
                raw: "Text ".to_string(),
                span: Span::new(5, 10, 1, 6),
            }),
            JsxChild::Expression(JsxExpression {
                expression: Some(Expression::Identifier(Identifier::new(
                    "expr".to_string(),
                    Span::new(11, 15, 1, 12),
                ))),
                span: Span::new(10, 16, 1, 11),
            }),
            JsxChild::Text(JsxText {
                value: " more text".to_string(),
                raw: " more text".to_string(),
                span: Span::new(16, 26, 1, 17),
            }),
        ],
        closing: Some(JsxClosingElement {
            name: JsxElementName::Identifier(Identifier::new("div".to_string(), Span::new(29, 32, 1, 30))),
            span: Span::new(26, 33, 1, 27),
        }),
        span: Span::new(0, 33, 1, 1),
    };

    assert_eq!(element.children.len(), 3);
}

#[test]
fn test_jsx_component_with_props() {
    let element = JsxElement {
        opening: JsxOpeningElement {
            name: JsxElementName::Identifier(Identifier::new("Button".to_string(), Span::new(1, 7, 1, 2))),
            attributes: vec![
                JsxAttribute::Attribute {
                    name: JsxAttributeName::Identifier(Identifier::new("onClick".to_string(), Span::new(8, 15, 1, 9))),
                    value: Some(JsxAttributeValue::Expression(Expression::Identifier(
                        Identifier::new("handleClick".to_string(), Span::new(17, 28, 1, 18)),
                    ))),
                    span: Span::new(8, 29, 1, 9),
                },
                JsxAttribute::Attribute {
                    name: JsxAttributeName::Identifier(Identifier::new("disabled".to_string(), Span::new(30, 38, 1, 31))),
                    value: None,
                    span: Span::new(30, 38, 1, 31),
                },
            ],
            self_closing: false,
            span: Span::new(0, 39, 1, 1),
        },
        children: vec![JsxChild::Text(JsxText {
            value: "Submit".to_string(),
            raw: "Submit".to_string(),
            span: Span::new(39, 45, 1, 40),
        })],
        closing: Some(JsxClosingElement {
            name: JsxElementName::Identifier(Identifier::new("Button".to_string(), Span::new(48, 54, 1, 49))),
            span: Span::new(45, 55, 1, 46),
        }),
        span: Span::new(0, 55, 1, 1),
    };

    assert_eq!(element.opening.attributes.len(), 2);
    assert_eq!(element.children.len(), 1);
}

// ============================================================================
// Expression Integration Tests
// ============================================================================

#[test]
fn test_jsx_element_as_expression() {
    let expr = Expression::JsxElement(JsxElement {
        opening: JsxOpeningElement {
            name: JsxElementName::Identifier(Identifier::new("div".to_string(), Span::new(1, 4, 1, 2))),
            attributes: vec![],
            self_closing: true,
            span: Span::new(0, 7, 1, 1),
        },
        children: vec![],
        closing: None,
        span: Span::new(0, 7, 1, 1),
    });

    assert_eq!(expr.span().start, 0);
    assert_eq!(expr.span().end, 7);
}

#[test]
fn test_jsx_fragment_as_expression() {
    let expr = Expression::JsxFragment(JsxFragment {
        opening: JsxOpeningFragment {
            span: Span::new(0, 2, 1, 1),
        },
        children: vec![],
        closing: JsxClosingFragment {
            span: Span::new(2, 5, 1, 3),
        },
        span: Span::new(0, 5, 1, 1),
    });

    assert_eq!(expr.span().start, 0);
    assert_eq!(expr.span().end, 5);
}
