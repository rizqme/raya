//! Comprehensive tests for Milestone 2.9 features
//! Tests destructuring patterns, JSX parsing, spread operators, and computed properties

use raya_engine::parser::Parser;
use raya_engine::parser::ast::*;

// ============================================================================
// Destructuring Pattern Tests
// ============================================================================

#[test]
fn test_array_destructuring_basic() {
    let source = "let [a, b, c] = arr;";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    assert_eq!(module.statements.len(), 1);
    assert!(matches!(module.statements[0], Statement::VariableDecl(_)));

    if let Statement::VariableDecl(decl) = &module.statements[0] {
        assert!(matches!(decl.pattern, Pattern::Array(_)));
    }
}

#[test]
fn test_array_destructuring_with_rest() {
    let source = "let [first, ...rest] = arr;";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    if let Statement::VariableDecl(decl) = &module.statements[0] {
        if let Pattern::Array(pattern) = &decl.pattern {
            assert!(pattern.rest.is_some());
        } else {
            panic!("Expected array pattern");
        }
    }
}

#[test]
fn test_array_destructuring_with_defaults() {
    let source = "let [x = 10, y = 20] = arr;";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    if let Statement::VariableDecl(decl) = &module.statements[0] {
        if let Pattern::Array(pattern) = &decl.pattern {
            // Verify elements have default values
            for elem in &pattern.elements {
                if let Some(elem) = elem {
                    assert!(elem.default.is_some());
                }
            }
        } else {
            panic!("Expected array pattern");
        }
    }
}

#[test]
fn test_object_destructuring_basic() {
    let source = "let { x, y } = point;";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    if let Statement::VariableDecl(decl) = &module.statements[0] {
        assert!(matches!(decl.pattern, Pattern::Object(_)));
    }
}

#[test]
fn test_object_destructuring_with_rest() {
    let source = "let { x, ...rest } = object;";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    if let Statement::VariableDecl(decl) = &module.statements[0] {
        if let Pattern::Object(pattern) = &decl.pattern {
            assert!(pattern.rest.is_some());
        } else {
            panic!("Expected object pattern");
        }
    }
}

#[test]
fn test_object_destructuring_with_defaults() {
    let source = "let { x = 0, y = 0 } = partial;";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    if let Statement::VariableDecl(decl) = &module.statements[0] {
        if let Pattern::Object(pattern) = &decl.pattern {
            for prop in &pattern.properties {
                assert!(prop.default.is_some());
            }
        } else {
            panic!("Expected object pattern");
        }
    }
}

// ============================================================================
// JSX Tests
// ============================================================================

#[test]
fn test_jsx_element_basic() {
    let source = r#"let elem = <div>Hello</div>;"#;
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    if let Statement::VariableDecl(decl) = &module.statements[0] {
        assert!(matches!(decl.initializer, Some(Expression::JsxElement(_))));
    }
}

#[test]
fn test_jsx_self_closing() {
    let source = r#"let elem = <img src="photo.jpg" />;"#;
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    if let Statement::VariableDecl(decl) = &module.statements[0] {
        if let Some(Expression::JsxElement(jsx)) = &decl.initializer {
            assert!(jsx.opening.self_closing);
            assert!(jsx.closing.is_none());
        } else {
            panic!("Expected JSX element");
        }
    }
}

#[test]
fn test_jsx_with_attributes() {
    let source = r#"let elem = <div className="container" />;"#;
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    if let Statement::VariableDecl(decl) = &module.statements[0] {
        if let Some(Expression::JsxElement(jsx)) = &decl.initializer {
            assert!(jsx.opening.attributes.len() > 0);
        } else {
            panic!("Expected JSX element");
        }
    }
}

#[test]
fn test_jsx_spread_attribute() {
    let source = r#"let elem = <Component {...props} />;"#;
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    if let Statement::VariableDecl(decl) = &module.statements[0] {
        if let Some(Expression::JsxElement(jsx)) = &decl.initializer {
            assert!(jsx.opening.attributes.len() > 0);
            assert!(matches!(
                jsx.opening.attributes[0],
                JsxAttribute::Spread { .. }
            ));
        } else {
            panic!("Expected JSX element");
        }
    }
}

#[test]
fn test_jsx_fragment() {
    let source = r#"let elem = <>content</>;"#;
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    if let Statement::VariableDecl(decl) = &module.statements[0] {
        assert!(matches!(decl.initializer, Some(Expression::JsxFragment(_))));
    }
}

#[test]
fn test_jsx_member_expression() {
    let source = r#"let elem = <UI.Button />;"#;
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    if let Statement::VariableDecl(decl) = &module.statements[0] {
        if let Some(Expression::JsxElement(jsx)) = &decl.initializer {
            assert!(matches!(
                jsx.opening.name,
                JsxElementName::MemberExpression { .. }
            ));
        } else {
            panic!("Expected JSX element");
        }
    }
}

// ============================================================================
// Spread Operator Tests
// ============================================================================

#[test]
fn test_array_spread() {
    let source = "let combined = [...arr1, ...arr2];";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    if let Statement::VariableDecl(decl) = &module.statements[0] {
        if let Some(Expression::Array(arr)) = &decl.initializer {
            // Verify we have spread elements
            let has_spread = arr.elements.iter().any(|elem| {
                matches!(elem, Some(ArrayElement::Spread(_)))
            });
            assert!(has_spread);
        } else {
            panic!("Expected array expression");
        }
    }
}

#[test]
fn test_object_spread() {
    let source = "let merged = { ...obj1, ...obj2 };";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    if let Statement::VariableDecl(decl) = &module.statements[0] {
        if let Some(Expression::Object(obj)) = &decl.initializer {
            // Verify we have spread properties
            let has_spread = obj.properties.iter().any(|prop| {
                matches!(prop, ObjectProperty::Spread(_))
            });
            assert!(has_spread);
        } else {
            panic!("Expected object expression");
        }
    }
}

// ============================================================================
// Computed Property Tests
// ============================================================================

#[test]
fn test_computed_property() {
    let source = "let obj = { [key]: value };";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    if let Statement::VariableDecl(decl) = &module.statements[0] {
        if let Some(Expression::Object(obj)) = &decl.initializer {
            if let ObjectProperty::Property(prop) = &obj.properties[0] {
                assert!(matches!(prop.key, PropertyKey::Computed(_)));
            } else {
                panic!("Expected property");
            }
        } else {
            panic!("Expected object expression");
        }
    }
}

#[test]
fn test_computed_property_with_expression() {
    let source = "let obj = { [1 + 2]: 'three' };";
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    if let Statement::VariableDecl(decl) = &module.statements[0] {
        if let Some(Expression::Object(obj)) = &decl.initializer {
            if let ObjectProperty::Property(prop) = &obj.properties[0] {
                if let PropertyKey::Computed(expr) = &prop.key {
                    assert!(matches!(expr, Expression::Binary(_)));
                } else {
                    panic!("Expected computed property");
                }
            } else {
                panic!("Expected property");
            }
        } else {
            panic!("Expected object expression");
        }
    }
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_complex_destructuring_and_spread() {
    let source = r#"
        let { items: [first, ...rest], ...other } = data;
    "#;
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    // Just verify it parses successfully
    assert_eq!(module.statements.len(), 1);
    assert!(matches!(module.statements[0], Statement::VariableDecl(_)));
}

#[test]
fn test_complex_object_with_all_features() {
    let source = r#"
        let obj = {
            ...defaults,
            name: "test",
            [computedKey]: value,
            nested: { ...nestedDefaults, x: 1 },
            ...overrides
        };
    "#;
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    if let Statement::VariableDecl(decl) = &module.statements[0] {
        if let Some(Expression::Object(obj)) = &decl.initializer {
            // Verify mix of spread, regular, and computed properties
            assert!(obj.properties.len() >= 3);

            let has_spread = obj.properties.iter().any(|p| matches!(p, ObjectProperty::Spread(_)));
            let has_computed = obj.properties.iter().any(|p| {
                matches!(p, ObjectProperty::Property(prop) if matches!(prop.key, PropertyKey::Computed(_)))
            });

            assert!(has_spread);
            assert!(has_computed);
        } else {
            panic!("Expected object expression");
        }
    }
}

#[test]
fn test_jsx_with_spread_and_computed() {
    let source = r#"let elem = <Component {...props} data-value={computed} />;"#;
    let parser = Parser::new(source).unwrap();
    let (module, _interner) = parser.parse().unwrap();

    if let Statement::VariableDecl(decl) = &module.statements[0] {
        if let Some(Expression::JsxElement(jsx)) = &decl.initializer {
            assert!(jsx.opening.attributes.len() >= 2);

            let has_spread = jsx.opening.attributes.iter().any(|attr| {
                matches!(attr, JsxAttribute::Spread { .. })
            });
            assert!(has_spread);
        } else {
            panic!("Expected JSX element");
        }
    }
}
