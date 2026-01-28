//! Tests for pattern parsing

use raya_engine::parser::ast::*;
use raya_engine::parser::parser::Parser;

// ============================================================================
// Simple Identifier Patterns
// ============================================================================

#[test]
fn test_parse_identifier_pattern() {
    let source = "let x = 42;";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::VariableDecl(decl) => match &decl.pattern {
            Pattern::Identifier(id) => assert_eq!(interner.resolve(id.name), "x"),
            _ => panic!("Expected identifier pattern"),
        },
        _ => panic!("Expected variable declaration"),
    }
}

// ============================================================================
// Array Destructuring Patterns
// ============================================================================

#[test]
fn test_parse_array_pattern_simple() {
    let source = "let [a, b, c] = [1, 2, 3];";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::VariableDecl(decl) => match &decl.pattern {
            Pattern::Array(array_pat) => {
                assert_eq!(array_pat.elements.len(), 3);

                // Check first element
                match &array_pat.elements[0] {
                    Some(elem) => match &elem.pattern {
                        Pattern::Identifier(id) => assert_eq!(interner.resolve(id.name), "a"),
                        _ => panic!("Expected identifier pattern"),
                    },
                    _ => panic!("Expected pattern element"),
                }

                // Check second element
                match &array_pat.elements[1] {
                    Some(elem) => match &elem.pattern {
                        Pattern::Identifier(id) => assert_eq!(interner.resolve(id.name), "b"),
                        _ => panic!("Expected identifier pattern"),
                    },
                    _ => panic!("Expected pattern element"),
                }

                // Check third element
                match &array_pat.elements[2] {
                    Some(elem) => match &elem.pattern {
                        Pattern::Identifier(id) => assert_eq!(interner.resolve(id.name), "c"),
                        _ => panic!("Expected identifier pattern"),
                    },
                    _ => panic!("Expected pattern element"),
                }
            }
            _ => panic!("Expected array pattern"),
        },
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_array_pattern_with_holes() {
    let source = "let [a, , c] = [1, 2, 3];";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::VariableDecl(decl) => match &decl.pattern {
            Pattern::Array(array_pat) => {
                assert_eq!(array_pat.elements.len(), 3);

                // Check first element
                match &array_pat.elements[0] {
                    Some(elem) => match &elem.pattern {
                        Pattern::Identifier(id) => assert_eq!(interner.resolve(id.name), "a"),
                        _ => panic!("Expected identifier pattern"),
                    },
                    _ => panic!("Expected pattern element"),
                }

                // Check second element (hole)
                assert!(array_pat.elements[1].is_none(), "Expected hole");

                // Check third element
                match &array_pat.elements[2] {
                    Some(elem) => match &elem.pattern {
                        Pattern::Identifier(id) => assert_eq!(interner.resolve(id.name), "c"),
                        _ => panic!("Expected identifier pattern"),
                    },
                    _ => panic!("Expected pattern element"),
                }
            }
            _ => panic!("Expected array pattern"),
        },
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_nested_array_pattern() {
    let source = "let [a, [b, c]] = [1, [2, 3]];";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::VariableDecl(decl) => match &decl.pattern {
            Pattern::Array(array_pat) => {
                assert_eq!(array_pat.elements.len(), 2);

                // Check first element
                match &array_pat.elements[0] {
                    Some(elem) => match &elem.pattern {
                        Pattern::Identifier(id) => assert_eq!(interner.resolve(id.name), "a"),
                        _ => panic!("Expected identifier pattern"),
                    },
                    _ => panic!("Expected pattern element"),
                }

                // Check second element (nested array)
                match &array_pat.elements[1] {
                    Some(elem) => match &elem.pattern {
                        Pattern::Array(nested) => {
                            assert_eq!(nested.elements.len(), 2);
                        }
                        _ => panic!("Expected nested array pattern"),
                    },
                    _ => panic!("Expected pattern element"),
                }
            }
            _ => panic!("Expected array pattern"),
        },
        _ => panic!("Expected variable declaration"),
    }
}

// ============================================================================
// Object Destructuring Patterns
// ============================================================================

#[test]
fn test_parse_object_pattern_simple() {
    let source = "let { x, y } = point;";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::VariableDecl(decl) => match &decl.pattern {
            Pattern::Object(obj_pat) => {
                assert_eq!(obj_pat.properties.len(), 2);

                // Check first property (shorthand)
                assert_eq!(interner.resolve(obj_pat.properties[0].key.name), "x");
                match &obj_pat.properties[0].value {
                    Pattern::Identifier(id) => assert_eq!(interner.resolve(id.name), "x"),
                    _ => panic!("Expected identifier pattern"),
                }

                // Check second property (shorthand)
                assert_eq!(interner.resolve(obj_pat.properties[1].key.name), "y");
                match &obj_pat.properties[1].value {
                    Pattern::Identifier(id) => assert_eq!(interner.resolve(id.name), "y"),
                    _ => panic!("Expected identifier pattern"),
                }
            }
            _ => panic!("Expected object pattern"),
        },
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_object_pattern_with_rename() {
    let source = "let { x: a, y: b } = point;";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::VariableDecl(decl) => match &decl.pattern {
            Pattern::Object(obj_pat) => {
                assert_eq!(obj_pat.properties.len(), 2);

                // Check first property (renamed)
                assert_eq!(interner.resolve(obj_pat.properties[0].key.name), "x");
                match &obj_pat.properties[0].value {
                    Pattern::Identifier(id) => assert_eq!(interner.resolve(id.name), "a"),
                    _ => panic!("Expected identifier pattern"),
                }

                // Check second property (renamed)
                assert_eq!(interner.resolve(obj_pat.properties[1].key.name), "y");
                match &obj_pat.properties[1].value {
                    Pattern::Identifier(id) => assert_eq!(interner.resolve(id.name), "b"),
                    _ => panic!("Expected identifier pattern"),
                }
            }
            _ => panic!("Expected object pattern"),
        },
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_nested_object_pattern() {
    let source = "let { point: { x, y } } = obj;";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::VariableDecl(decl) => match &decl.pattern {
            Pattern::Object(obj_pat) => {
                assert_eq!(obj_pat.properties.len(), 1);

                // Check first property (nested object)
                assert_eq!(interner.resolve(obj_pat.properties[0].key.name), "point");
                match &obj_pat.properties[0].value {
                    Pattern::Object(nested) => {
                        assert_eq!(nested.properties.len(), 2);
                        assert_eq!(interner.resolve(nested.properties[0].key.name), "x");
                        assert_eq!(interner.resolve(nested.properties[1].key.name), "y");
                    }
                    _ => panic!("Expected nested object pattern"),
                }
            }
            _ => panic!("Expected object pattern"),
        },
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_mixed_object_pattern() {
    let source = "let { x, y: newY } = point;";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::VariableDecl(decl) => match &decl.pattern {
            Pattern::Object(obj_pat) => {
                assert_eq!(obj_pat.properties.len(), 2);

                // Check first property (shorthand)
                assert_eq!(interner.resolve(obj_pat.properties[0].key.name), "x");
                match &obj_pat.properties[0].value {
                    Pattern::Identifier(id) => assert_eq!(interner.resolve(id.name), "x"),
                    _ => panic!("Expected identifier pattern"),
                }

                // Check second property (renamed)
                assert_eq!(interner.resolve(obj_pat.properties[1].key.name), "y");
                match &obj_pat.properties[1].value {
                    Pattern::Identifier(id) => assert_eq!(interner.resolve(id.name), "newY"),
                    _ => panic!("Expected identifier pattern"),
                }
            }
            _ => panic!("Expected object pattern"),
        },
        _ => panic!("Expected variable declaration"),
    }
}

// ============================================================================
// Complex Nested Patterns
// ============================================================================

#[test]
fn test_parse_array_of_objects_pattern() {
    let source = "let [{ x }, { y }] = points;";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::VariableDecl(decl) => match &decl.pattern {
            Pattern::Array(array_pat) => {
                assert_eq!(array_pat.elements.len(), 2);

                // Check first element (object pattern)
                match &array_pat.elements[0] {
                    Some(elem) => match &elem.pattern {
                        Pattern::Object(obj_pat) => {
                            assert_eq!(obj_pat.properties.len(), 1);
                            assert_eq!(interner.resolve(obj_pat.properties[0].key.name), "x");
                        }
                        _ => panic!("Expected object pattern"),
                    },
                    _ => panic!("Expected pattern element"),
                }

                // Check second element (object pattern)
                match &array_pat.elements[1] {
                    Some(elem) => match &elem.pattern {
                        Pattern::Object(obj_pat) => {
                            assert_eq!(obj_pat.properties.len(), 1);
                            assert_eq!(interner.resolve(obj_pat.properties[0].key.name), "y");
                        }
                        _ => panic!("Expected object pattern"),
                    },
                    _ => panic!("Expected pattern element"),
                }
            }
            _ => panic!("Expected array pattern"),
        },
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_object_with_array_pattern() {
    let source = "let { coords: [x, y] } = obj;";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::VariableDecl(decl) => match &decl.pattern {
            Pattern::Object(obj_pat) => {
                assert_eq!(obj_pat.properties.len(), 1);

                // Check property (array pattern)
                assert_eq!(interner.resolve(obj_pat.properties[0].key.name), "coords");
                match &obj_pat.properties[0].value {
                    Pattern::Array(array_pat) => {
                        assert_eq!(array_pat.elements.len(), 2);
                    }
                    _ => panic!("Expected array pattern"),
                }
            }
            _ => panic!("Expected object pattern"),
        },
        _ => panic!("Expected variable declaration"),
    }
}

// ============================================================================
// Patterns in Function Parameters
// ============================================================================

#[test]
fn test_parse_function_with_array_pattern_param() {
    let source = "function process([a, b]) { return a + b; }";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::FunctionDecl(func) => {
            assert_eq!(func.params.len(), 1);
            match &func.params[0].pattern {
                Pattern::Array(array_pat) => {
                    assert_eq!(array_pat.elements.len(), 2);
                }
                _ => panic!("Expected array pattern"),
            }
        }
        _ => panic!("Expected function declaration"),
    }
}

#[test]
fn test_parse_function_with_object_pattern_param() {
    let source = "function greet({ name, age }) { return name; }";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::FunctionDecl(func) => {
            assert_eq!(func.params.len(), 1);
            match &func.params[0].pattern {
                Pattern::Object(obj_pat) => {
                    assert_eq!(obj_pat.properties.len(), 2);
                    assert_eq!(interner.resolve(obj_pat.properties[0].key.name), "name");
                    assert_eq!(interner.resolve(obj_pat.properties[1].key.name), "age");
                }
                _ => panic!("Expected object pattern"),
            }
        }
        _ => panic!("Expected function declaration"),
    }
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_parse_empty_array_pattern() {
    let source = "let [] = empty;";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::VariableDecl(decl) => match &decl.pattern {
            Pattern::Array(array_pat) => {
                assert_eq!(array_pat.elements.len(), 0);
            }
            _ => panic!("Expected array pattern"),
        },
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_empty_object_pattern() {
    let source = "let {} = empty;";
    let parser = Parser::new(source).unwrap();
    let (module, interner) = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::VariableDecl(decl) => match &decl.pattern {
            Pattern::Object(obj_pat) => {
                assert_eq!(obj_pat.properties.len(), 0);
            }
            _ => panic!("Expected object pattern"),
        },
        _ => panic!("Expected variable declaration"),
    }
}
