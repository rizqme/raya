use raya_engine::parser::ast::*;
use raya_engine::parser::token::Span;
use raya_engine::parser::Interner;

// Helper to create a test Interner with common symbols
fn test_interner() -> Interner {
    Interner::with_capacity(64)
}

// Helper to create an Identifier
fn ident(interner: &mut Interner, name: &str, span: Span) -> Identifier {
    Identifier::new(interner.intern(name), span)
}

// Helper to create a StringLiteral
fn string_lit(interner: &mut Interner, value: &str, span: Span) -> StringLiteral {
    StringLiteral {
        value: interner.intern(value),
        span,
    }
}

// ============================================================================
// JSX Element Tests
// ============================================================================

#[test]
fn test_jsx_self_closing_element() {
    let mut interner = test_interner();
    let element = JsxElement {
        opening: JsxOpeningElement {
            name: JsxElementName::Identifier(ident(&mut interner, "img", Span::new(1, 4, 1, 2))),
            attributes: vec![
                JsxAttribute::Attribute {
                    name: JsxAttributeName::Identifier(ident(&mut interner, "src", Span::new(5, 8, 1, 6))),
                    value: Some(JsxAttributeValue::StringLiteral(string_lit(&mut interner, "photo.jpg", Span::new(9, 20, 1, 10)))),
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
    let mut interner = test_interner();
    let element = JsxElement {
        opening: JsxOpeningElement {
            name: JsxElementName::Identifier(ident(&mut interner, "div", Span::new(1, 4, 1, 2))),
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
            name: JsxElementName::Identifier(ident(&mut interner, "div", Span::new(19, 22, 1, 20))),
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
    let mut interner = test_interner();
    let div_name = JsxElementName::Identifier(ident(&mut interner, "div", Span::new(0, 3, 1, 1)));
    let button_name = JsxElementName::Identifier(ident(&mut interner, "Button", Span::new(0, 6, 1, 1)));

    assert!(div_name.is_intrinsic(&interner));
    assert!(!button_name.is_intrinsic(&interner));
}

#[test]
fn test_jsx_element_name_to_string() {
    let mut interner = test_interner();

    // Simple identifier
    let simple = JsxElementName::Identifier(ident(&mut interner, "div", Span::new(0, 3, 1, 1)));
    assert_eq!(simple.to_string(&interner), "div");

    // Namespaced
    let namespaced = JsxElementName::Namespaced {
        namespace: ident(&mut interner, "svg", Span::new(0, 3, 1, 1)),
        name: ident(&mut interner, "path", Span::new(4, 8, 1, 5)),
    };
    assert_eq!(namespaced.to_string(&interner), "svg:path");

    // Member expression
    let member = JsxElementName::MemberExpression {
        object: Box::new(JsxElementName::Identifier(ident(&mut interner, "React", Span::new(0, 5, 1, 1)))),
        property: ident(&mut interner, "Fragment", Span::new(6, 14, 1, 7)),
    };
    assert_eq!(member.to_string(&interner), "React.Fragment");
}

#[test]
fn test_jsx_nested_member_expression() {
    let mut interner = test_interner();

    // UI.Components.Button
    let nested = JsxElementName::MemberExpression {
        object: Box::new(JsxElementName::MemberExpression {
            object: Box::new(JsxElementName::Identifier(ident(&mut interner, "UI", Span::new(0, 2, 1, 1)))),
            property: ident(&mut interner, "Components", Span::new(3, 13, 1, 4)),
        }),
        property: ident(&mut interner, "Button", Span::new(14, 20, 1, 15)),
    };

    assert_eq!(nested.to_string(&interner), "UI.Components.Button");
}

// ============================================================================
// JSX Attribute Tests
// ============================================================================

#[test]
fn test_jsx_string_attribute() {
    let mut interner = test_interner();
    let attr = JsxAttribute::Attribute {
        name: JsxAttributeName::Identifier(ident(&mut interner, "className", Span::new(0, 9, 1, 1))),
        value: Some(JsxAttributeValue::StringLiteral(string_lit(&mut interner, "container", Span::new(10, 21, 1, 11)))),
        span: Span::new(0, 21, 1, 1),
    };

    if let JsxAttribute::Attribute { value, .. } = attr {
        assert!(value.is_some());
        if let Some(JsxAttributeValue::StringLiteral(s)) = value {
            assert_eq!(interner.resolve(s.value), "container");
        }
    }
}

#[test]
fn test_jsx_expression_attribute() {
    let mut interner = test_interner();
    let attr = JsxAttribute::Attribute {
        name: JsxAttributeName::Identifier(ident(&mut interner, "value", Span::new(0, 5, 1, 1))),
        value: Some(JsxAttributeValue::Expression(Expression::Identifier(
            ident(&mut interner, "text", Span::new(7, 11, 1, 8)),
        ))),
        span: Span::new(0, 12, 1, 1),
    };

    if let JsxAttribute::Attribute { value, .. } = attr {
        assert!(value.is_some());
    }
}

#[test]
fn test_jsx_boolean_attribute() {
    // A boolean attribute like "disabled" has no value
    let mut interner = test_interner();
    let attr = JsxAttribute::Attribute {
        name: JsxAttributeName::Identifier(ident(&mut interner, "disabled", Span::new(0, 8, 1, 1))),
        value: None,
        span: Span::new(0, 8, 1, 1),
    };

    if let JsxAttribute::Attribute { value, .. } = attr {
        assert!(value.is_none());
    }
}

#[test]
fn test_jsx_namespaced_attribute() {
    let mut interner = test_interner();
    let attr = JsxAttribute::Attribute {
        name: JsxAttributeName::Namespaced {
            namespace: ident(&mut interner, "xlink", Span::new(0, 5, 1, 1)),
            name: ident(&mut interner, "href", Span::new(6, 10, 1, 7)),
        },
        value: Some(JsxAttributeValue::StringLiteral(string_lit(&mut interner, "#target", Span::new(11, 20, 1, 12)))),
        span: Span::new(0, 20, 1, 1),
    };

    if let JsxAttribute::Attribute { name, .. } = attr {
        match name {
            JsxAttributeName::Namespaced { namespace, name } => {
                assert_eq!(interner.resolve(namespace.name), "xlink");
                assert_eq!(interner.resolve(name.name), "href");
            }
            _ => panic!("Expected namespaced attribute"),
        }
    }
}

#[test]
fn test_jsx_spread_attribute() {
    let mut interner = test_interner();
    let attr = JsxAttribute::Spread {
        argument: Expression::Identifier(ident(&mut interner, "props", Span::new(5, 10, 1, 6))),
        span: Span::new(0, 11, 1, 1),
    };

    if let JsxAttribute::Spread { argument, .. } = attr {
        if let Expression::Identifier(id) = argument {
            assert_eq!(interner.resolve(id.name), "props");
        } else {
            panic!("Expected identifier");
        }
    }
}

// ============================================================================
// JSX Children Tests
// ============================================================================

#[test]
fn test_jsx_text_child() {
    let text = JsxChild::Text(JsxText {
        value: "Hello World".to_string(),
        raw: "Hello World".to_string(),
        span: Span::new(0, 11, 1, 1),
    });

    if let JsxChild::Text(t) = text {
        assert_eq!(t.value, "Hello World");
        assert_eq!(t.raw, "Hello World");
    }
}

#[test]
fn test_jsx_expression_child() {
    let mut interner = test_interner();
    let child = JsxChild::Expression(JsxExpression {
        expression: Some(Expression::Identifier(ident(&mut interner, "content", Span::new(1, 8, 1, 2)))),
        span: Span::new(0, 9, 1, 1),
    });

    if let JsxChild::Expression(expr) = child {
        assert!(expr.expression.is_some());
    }
}

#[test]
fn test_jsx_empty_expression_child() {
    // {/* comment */} results in empty expression
    let child = JsxChild::Expression(JsxExpression {
        expression: None,
        span: Span::new(0, 15, 1, 1),
    });

    if let JsxChild::Expression(expr) = child {
        assert!(expr.expression.is_none());
    }
}

#[test]
fn test_jsx_element_child() {
    let mut interner = test_interner();
    let child = JsxChild::Element(JsxElement {
        opening: JsxOpeningElement {
            name: JsxElementName::Identifier(ident(&mut interner, "span", Span::new(1, 5, 1, 2))),
            attributes: vec![],
            self_closing: true,
            span: Span::new(0, 8, 1, 1),
        },
        children: vec![],
        closing: None,
        span: Span::new(0, 8, 1, 1),
    });

    if let JsxChild::Element(e) = child {
        assert!(e.opening.self_closing);
    }
}

// ============================================================================
// JSX Fragment Tests
// ============================================================================

#[test]
fn test_jsx_fragment() {
    let fragment = JsxFragment {
        opening: JsxOpeningFragment {
            span: Span::new(0, 2, 1, 1),
        },
        children: vec![JsxChild::Text(JsxText {
            value: "Content".to_string(),
            raw: "Content".to_string(),
            span: Span::new(2, 9, 1, 3),
        })],
        closing: JsxClosingFragment {
            span: Span::new(9, 12, 1, 10),
        },
        span: Span::new(0, 12, 1, 1),
    };

    assert_eq!(fragment.children.len(), 1);
}

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

// ============================================================================
// Complex JSX Tests
// ============================================================================

#[test]
fn test_jsx_nested_elements() {
    let mut interner = test_interner();

    // <div><span>Text</span></div>
    let element = JsxElement {
        opening: JsxOpeningElement {
            name: JsxElementName::Identifier(ident(&mut interner, "div", Span::new(1, 4, 1, 2))),
            attributes: vec![],
            self_closing: false,
            span: Span::new(0, 5, 1, 1),
        },
        children: vec![JsxChild::Element(JsxElement {
            opening: JsxOpeningElement {
                name: JsxElementName::Identifier(ident(&mut interner, "span", Span::new(6, 10, 1, 7))),
                attributes: vec![],
                self_closing: false,
                span: Span::new(5, 11, 1, 6),
            },
            children: vec![JsxChild::Text(JsxText {
                value: "Text".to_string(),
                raw: "Text".to_string(),
                span: Span::new(11, 15, 1, 12),
            })],
            closing: Some(JsxClosingElement {
                name: JsxElementName::Identifier(ident(&mut interner, "span", Span::new(18, 22, 1, 19))),
                span: Span::new(15, 23, 1, 16),
            }),
            span: Span::new(5, 23, 1, 6),
        })],
        closing: Some(JsxClosingElement {
            name: JsxElementName::Identifier(ident(&mut interner, "div", Span::new(26, 29, 1, 27))),
            span: Span::new(23, 30, 1, 24),
        }),
        span: Span::new(0, 30, 1, 1),
    };

    assert_eq!(element.children.len(), 1);
    if let JsxChild::Element(inner) = &element.children[0] {
        assert_eq!(inner.children.len(), 1);
    }
}

#[test]
fn test_jsx_multiple_attributes() {
    let mut interner = test_interner();

    let element = JsxElement {
        opening: JsxOpeningElement {
            name: JsxElementName::Identifier(ident(&mut interner, "input", Span::new(1, 6, 1, 2))),
            attributes: vec![
                JsxAttribute::Attribute {
                    name: JsxAttributeName::Identifier(ident(&mut interner, "type", Span::new(7, 11, 1, 8))),
                    value: Some(JsxAttributeValue::StringLiteral(string_lit(&mut interner, "text", Span::new(12, 18, 1, 13)))),
                    span: Span::new(7, 18, 1, 8),
                },
                JsxAttribute::Attribute {
                    name: JsxAttributeName::Identifier(ident(&mut interner, "value", Span::new(19, 24, 1, 20))),
                    value: Some(JsxAttributeValue::Expression(Expression::Identifier(
                        ident(&mut interner, "input", Span::new(26, 31, 1, 27)),
                    ))),
                    span: Span::new(19, 32, 1, 20),
                },
                JsxAttribute::Attribute {
                    name: JsxAttributeName::Identifier(ident(&mut interner, "onChange", Span::new(33, 41, 1, 34))),
                    value: Some(JsxAttributeValue::Expression(Expression::Identifier(
                        ident(&mut interner, "handleChange", Span::new(43, 55, 1, 44)),
                    ))),
                    span: Span::new(33, 56, 1, 34),
                },
            ],
            self_closing: true,
            span: Span::new(0, 59, 1, 1),
        },
        children: vec![],
        closing: None,
        span: Span::new(0, 59, 1, 1),
    };

    assert_eq!(element.opening.attributes.len(), 3);
}

#[test]
fn test_jsx_mixed_children() {
    let mut interner = test_interner();

    // <div>Text {expr} more text</div>
    let element = JsxElement {
        opening: JsxOpeningElement {
            name: JsxElementName::Identifier(ident(&mut interner, "div", Span::new(1, 4, 1, 2))),
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
                expression: Some(Expression::Identifier(ident(&mut interner, "expr", Span::new(11, 15, 1, 12)))),
                span: Span::new(10, 16, 1, 11),
            }),
            JsxChild::Text(JsxText {
                value: " more text".to_string(),
                raw: " more text".to_string(),
                span: Span::new(16, 26, 1, 17),
            }),
        ],
        closing: Some(JsxClosingElement {
            name: JsxElementName::Identifier(ident(&mut interner, "div", Span::new(29, 32, 1, 30))),
            span: Span::new(26, 33, 1, 27),
        }),
        span: Span::new(0, 33, 1, 1),
    };

    assert_eq!(element.children.len(), 3);
}

#[test]
fn test_jsx_component_with_props() {
    let mut interner = test_interner();

    let element = JsxElement {
        opening: JsxOpeningElement {
            name: JsxElementName::Identifier(ident(&mut interner, "Button", Span::new(1, 7, 1, 2))),
            attributes: vec![
                JsxAttribute::Attribute {
                    name: JsxAttributeName::Identifier(ident(&mut interner, "onClick", Span::new(8, 15, 1, 9))),
                    value: Some(JsxAttributeValue::Expression(Expression::Identifier(
                        ident(&mut interner, "handleClick", Span::new(17, 28, 1, 18)),
                    ))),
                    span: Span::new(8, 29, 1, 9),
                },
                JsxAttribute::Attribute {
                    name: JsxAttributeName::Identifier(ident(&mut interner, "disabled", Span::new(30, 38, 1, 31))),
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
            name: JsxElementName::Identifier(ident(&mut interner, "Button", Span::new(48, 54, 1, 49))),
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
    let mut interner = test_interner();

    let expr = Expression::JsxElement(JsxElement {
        opening: JsxOpeningElement {
            name: JsxElementName::Identifier(ident(&mut interner, "div", Span::new(1, 4, 1, 2))),
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
