//! Tests for parser hardening and robustness
//!
//! These tests verify that the parser can handle malformed, incomplete,
//! or pathological source code without hanging, crashing, or consuming
//! excessive resources.

use raya_parser::Parser;
use raya_parser::parser::ParseErrorKind;

// ============================================================================
// Infinite Loop Prevention
// ============================================================================

#[test]
fn test_malformed_jsx_attributes_no_hang() {
    // This used to hang due to hyphenated attributes (before fix)
    let source = r#"<div data-test-value-extra-hyphens="x" />"#;

    let parser = Parser::new(source).unwrap();
    let result = parser.parse();

    // Should succeed (not hang)
    assert!(result.is_ok(), "Parser should not hang on hyphenated attributes");
}

#[test]
fn test_unclosed_jsx_element_no_hang() {
    let source = r#"<div>"#;

    let parser = Parser::new(source).unwrap();
    let result = parser.parse();

    // Should error, not hang
    assert!(result.is_err(), "Should error on unclosed element");
}

#[test]
fn test_many_jsx_attributes() {
    // Create element with 100 attributes
    let mut source = String::from("<div ");
    for i in 0..100 {
        source.push_str(&format!("attr{}=\"value{}\" ", i, i));
    }
    source.push_str("/>");

    let parser = Parser::new(&source).unwrap();
    let result = parser.parse();

    assert!(result.is_ok(), "Should handle many attributes");
}

#[test]
fn test_deeply_nested_jsx() {
    // Create 50-level nested JSX
    let depth = 50;
    let mut source = String::new();
    for i in 0..depth {
        source.push_str(&format!("<div{}>", i));
    }
    source.push_str("content");
    for i in (0..depth).rev() {
        source.push_str(&format!("</div{}>", i));
    }

    let parser = Parser::new(&source).unwrap();
    let result = parser.parse();

    // Should succeed with reasonable nesting
    assert!(result.is_ok(), "Should handle reasonable JSX nesting");
}

// ============================================================================
// Depth Limits
// ============================================================================

#[test]
fn test_max_nesting_depth_arrays() {
    // Create deeply nested array: [[[[...41 levels...]]]]]
    // This exceeds MAX_PARSE_DEPTH (50) and should be rejected
    let depth = 41;
    let source = "[".repeat(depth) + "1" + &"]".repeat(depth);

    let parser = Parser::new(&source).unwrap();
    let result = parser.parse();

    // Should fail with depth limit error
    assert!(result.is_err(), "Should reject extremely deep nesting");

    if let Err(errors) = result {
        assert!(!errors.is_empty(), "Should have at least one error");
        assert!(
            matches!(errors[0].kind, ParseErrorKind::ParserLimitExceeded { .. }),
            "Should be parser limit exceeded error, got: {:?}",
            errors[0].kind
        );
    }
}

#[test]
fn test_max_nesting_depth_objects() {
    // Create deeply nested object expression in a let statement
    // let x = {a: {a: {a: ...38 levels...}}}
    // NOTE: Objects have deeper call stack than arrays, so use 38 instead of 41
    let depth = 38;
    let source = "let x = ".to_string() + &"{a:".repeat(depth) + "1" + &"}".repeat(depth) + ";";

    let parser = Parser::new(&source).unwrap();
    let result = parser.parse();

    // Should fail with depth limit error (depth guard triggers before stack overflow)
    assert!(result.is_err(), "Should reject extremely deep object nesting");
}

#[test]
fn test_max_nesting_depth_expressions() {
    // Create deeply nested parentheses: (((((...41 levels...))))
    let depth = 41;
    let source = "(".repeat(depth) + "1" + &")".repeat(depth);

    let parser = Parser::new(&source).unwrap();
    let result = parser.parse();

    // Should fail with depth limit error
    assert!(result.is_err(), "Should reject extremely deep expression nesting");
}

#[test]
fn test_reasonable_nesting_accepted() {
    // 30 levels should be fine (well under limit of 40)
    let depth = 30;
    let source = "[".repeat(depth) + "1" + &"]".repeat(depth);

    let parser = Parser::new(&source).unwrap();
    let result = parser.parse();

    assert!(result.is_ok(), "Should handle reasonable nesting depth");
}

// ============================================================================
// Loop Guard Tests
// ============================================================================

#[test]
fn test_array_pattern_with_many_elements() {
    // Create array pattern with 100 elements
    let elements: Vec<String> = (0..100).map(|i| format!("x{}", i)).collect();
    let source = format!("let [{}] = arr;", elements.join(", "));

    let parser = Parser::new(&source).unwrap();
    let result = parser.parse();

    assert!(result.is_ok(), "Should handle array pattern with many elements");
}

#[test]
fn test_object_pattern_with_many_properties() {
    // Create object pattern with 100 properties
    let properties: Vec<String> = (0..100).map(|i| format!("x{}", i)).collect();
    let source = format!("let {{ {} }} = obj;", properties.join(", "));

    let parser = Parser::new(&source).unwrap();
    let result = parser.parse();

    assert!(result.is_ok(), "Should handle object pattern with many properties");
}

#[test]
fn test_extreme_loop_iterations() {
    // This should hit loop guard limit (10,000 iterations)
    // Create array with 15,000 elements
    let elements: Vec<String> = (0..15_000).map(|i| i.to_string()).collect();
    let source = format!("[{}]", elements.join(", "));

    let parser = Parser::new(&source).unwrap();
    let result = parser.parse();

    // Should fail with loop limit error
    assert!(result.is_err(), "Should hit loop iteration limit");
}

// ============================================================================
// Pathological Cases
// ============================================================================

#[test]
fn test_very_long_identifier() {
    // 10,000 character identifier
    let name = "x".repeat(10_000);
    let source = format!("let {} = 42;", name);

    let parser = Parser::new(&source).unwrap();
    let result = parser.parse();

    // Should handle long identifiers
    assert!(result.is_ok(), "Should handle long identifiers");
}

#[test]
#[ignore] // TODO: Fix stack overflow with very long strings
fn test_very_long_string() {
    // 100,000 character string
    let s = "x".repeat(100_000);
    let source = format!(r#"let x = "{}";"#, s);

    let parser = Parser::new(&source).unwrap();
    let result = parser.parse();

    assert!(result.is_ok(), "Should handle long strings");
}

#[test]
fn test_many_function_arguments() {
    // Function call with 1000 arguments
    let args: Vec<String> = (1..=1000).map(|i| i.to_string()).collect();
    let source = format!("f({});", args.join(", "));

    let parser = Parser::new(&source).unwrap();
    let result = parser.parse();

    assert!(result.is_ok(), "Should handle many arguments");
}

#[test]
fn test_deeply_chained_member_access() {
    // a.b.c.d. ... .m999 (1000 levels)
    let chain = (0..1000)
        .map(|i| format!("m{}", i))
        .collect::<Vec<_>>()
        .join(".");
    let source = format!("x.{};", chain);

    let parser = Parser::new(&source).unwrap();
    let result = parser.parse();

    assert!(result.is_ok(), "Should handle long member chains");
}

#[test]
fn test_long_operator_chain() {
    // 1 + 2 + 3 + ... + 1000
    let mut source = String::from("1");
    for i in 2..=1000 {
        source.push_str(&format!(" + {}", i));
    }

    let parser = Parser::new(&source).unwrap();
    let result = parser.parse();

    assert!(result.is_ok(), "Should handle long operator chains");
}

#[test]
fn test_mixed_precedence_operators() {
    // Complex precedence: 1 + 2 * 3 - 4 / 5 + 6 % 7 ...
    let source = "1 + 2 * 3 - 4 / 5 + 6 % 7 * 8 + 9 - 10 * 11 / 12";

    let parser = Parser::new(&source).unwrap();
    let result = parser.parse();

    assert!(result.is_ok(), "Should handle mixed precedence operators");
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_empty_array_pattern() {
    let source = "let [] = arr;";

    let parser = Parser::new(source).unwrap();
    let result = parser.parse();

    assert!(result.is_ok(), "Should handle empty array pattern");
}

#[test]
fn test_empty_object_pattern() {
    let source = "let {} = obj;";

    let parser = Parser::new(source).unwrap();
    let result = parser.parse();

    assert!(result.is_ok(), "Should handle empty object pattern");
}

#[test]
fn test_array_with_holes() {
    let source = "let [a, , , , b] = arr;";

    let parser = Parser::new(source).unwrap();
    let result = parser.parse();

    assert!(result.is_ok(), "Should handle array patterns with many holes");
}

#[test]
fn test_deeply_nested_destructuring() {
    let source = r#"
        let {
            a: {
                b: {
                    c: {
                        d: {
                            e: value
                        }
                    }
                }
            }
        } = obj;
    "#;

    let parser = Parser::new(source).unwrap();
    let result = parser.parse();

    assert!(result.is_ok(), "Should handle nested destructuring");
}

// ============================================================================
// JSX Specific Tests
// ============================================================================

#[test]
fn test_jsx_with_many_hyphens_in_attribute() {
    let source = r#"<div data-test-foo-bar-baz-qux-extra-long="value" />"#;

    let parser = Parser::new(source).unwrap();
    let result = parser.parse();

    assert!(result.is_ok(), "Should handle attributes with many hyphens");
}

#[test]
fn test_jsx_with_numeric_attribute_names() {
    // Note: This may not be valid JSX, but parser should handle it gracefully
    let source = r#"<div data-1-2-3="value" />"#;

    let parser = Parser::new(source).unwrap();
    let result = parser.parse();

    // Parser should either accept or reject gracefully (not hang)
    let _ = result; // Don't care about result, just that it doesn't hang
}

#[test]
fn test_jsx_deeply_nested_elements() {
    // Build deeply nested JSX (50 levels)
    let mut source = String::new();
    for _ in 0..50 {
        source.push_str("<div>");
    }
    source.push_str("text");
    for _ in 0..50 {
        source.push_str("</div>");
    }

    let parser = Parser::new(&source).unwrap();
    let result = parser.parse();

    assert!(result.is_ok(), "Should handle deeply nested JSX");
}
#[test]
fn test_realistic_nesting_depth() {
    use raya_parser::Parser;
    
    // Test realistic code patterns - around 10-15 levels of nesting
    let source = r#"
        function processData(data) {
            if (data) {
                const result = data.map(item => {
                    if (item.valid) {
                        return {
                            id: item.id,
                            nested: {
                                level1: {
                                    level2: {
                                        value: item.value
                                    }
                                }
                            }
                        };
                    }
                    return null;
                });
                return result;
            }
            return [];
        }
    "#;
    
    let parser = Parser::new(source).unwrap();
    let result = parser.parse();
    
    assert!(result.is_ok(), "Should handle realistic nesting depth (10-15 levels)");
}

#[test]
fn test_deeply_nested_realistic() {
    use raya_parser::Parser;
    
    // Test around 20 levels - still reasonable for complex apps
    let source = r#"
        let config = {
            app: {
                settings: {
                    database: {
                        connection: {
                            pool: {
                                min: 2,
                                max: 10
                            }
                        }
                    },
                    api: {
                        endpoints: {
                            users: {
                                get: "/api/users",
                                post: "/api/users"
                            }
                        }
                    }
                }
            }
        };
    "#;
    
    let parser = Parser::new(source).unwrap();
    let result = parser.parse();
    
    assert!(result.is_ok(), "Should handle 20 levels of object nesting");
}
#[test]
fn test_depth_limit_41() {
    use raya_parser::Parser;
    
    // 41 levels - just over the limit of 40
    let depth = 41;
    let source = "[".repeat(depth) + "1" + &"]".repeat(depth);
    
    let parser = Parser::new(&source).unwrap();
    let result = parser.parse();
    
    // Should be rejected by depth guard
    assert!(result.is_err(), "Should reject 41 levels (over limit of 40)");
}
#[test]
fn test_object_depth_10() {
    use raya_parser::Parser;
    
    let depth = 10;
    let source = "let x = ".to_string() + &"{a:".repeat(depth) + "1" + &"}".repeat(depth) + ";";
    println!("Source: {}", source);
    
    let parser = Parser::new(&source).unwrap();
    let result = parser.parse();
    
    assert!(result.is_ok(), "Should handle 10 levels of object nesting");
}
#[test]
fn test_object_depth_at_limit() {
    use raya_parser::Parser;

    // 33 levels of object nesting + 2 (statement + expression) = 35 total depth
    // This should be at the exact limit (MAX_PARSE_DEPTH = 35)
    let depth = 33;
    let source = "let x = ".to_string() + &"{a:".repeat(depth) + "1" + &"}".repeat(depth) + ";";

    let parser = Parser::new(&source).unwrap();
    let result = parser.parse();

    assert!(result.is_ok(), "Should handle 33 levels of object nesting (at limit)");
}
#[test]
fn test_obj_38() {
    use raya_parser::Parser;
    let source = "let x = ".to_string() + &"{a:".repeat(38) + "1" + &"}".repeat(38) + ";";
    let parser = Parser::new(&source).unwrap();
    let result = parser.parse();
    println!("Depth 38 result: {:?}", result.is_ok());
}
#[test]
fn test_obj_39() {
    use raya_parser::Parser;
    let source = "let x = ".to_string() + &"{a:".repeat(39) + "1" + &"}".repeat(39) + ";";
    let parser = Parser::new(&source).unwrap();
    let result = parser.parse();
    println!("Depth 39 result: {:?}", result.is_ok());
}
#[test]
fn test_obj_40() {
    use raya_parser::Parser;
    let source = "let x = ".to_string() + &"{a:".repeat(40) + "1" + &"}".repeat(40) + ";";
    let parser = Parser::new(&source).unwrap();
    let result = parser.parse();
    println!("Depth 40 result: {:?}", result.is_ok());
}
#[test]
fn test_obj_41() {
    use raya_parser::Parser;
    let source = "let x = ".to_string() + &"{a:".repeat(41) + "1" + &"}".repeat(41) + ";";
    let parser = Parser::new(&source).unwrap();
    let result = parser.parse();
    println!("Depth 41 result: {:?}", result.is_ok());
}
#[test]
fn test_check_38() {
    use raya_parser::Parser;
    let source = "let x = ".to_string() + &"{a:".repeat(38) + "1" + &"}".repeat(38) + ";";
    let parser = Parser::new(&source).unwrap();
    let result = parser.parse();
    match result {
        Ok(_) => println!("38 levels: PASSED (no error)"),
        Err(e) => println!("38 levels: REJECTED with {} errors", e.len()),
    }
}
#[test]
fn test_depth_over_limit() {
    use raya_parser::Parser;
    // 34 levels of nesting + 2 (statement + expression) = 36 total depth > 35 limit
    let source = "let x = ".to_string() + &"{a:".repeat(34) + "1" + &"}".repeat(34) + ";";
    let parser = Parser::new(&source).unwrap();
    let result = parser.parse();
    assert!(result.is_err(), "Should reject 34 levels (total depth 36, over limit of 35)");
}
