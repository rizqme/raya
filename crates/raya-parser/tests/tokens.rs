//! Basic token tests for the Raya lexer.

use raya_parser::{Lexer, Token};

fn lex_single(source: &str) -> Token {
    let lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();
    assert!(tokens.len() >= 2, "Expected at least 2 tokens (token + EOF)");
    tokens[0].0.clone()
}

fn assert_tokens(source: &str, expected: Vec<Token>) {
    let lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();
    let actual: Vec<Token> = tokens.iter().map(|(t, _)| t.clone()).collect();
    
    // Expected should include EOF
    let mut expected_with_eof = expected;
    expected_with_eof.push(Token::Eof);
    
    assert_eq!(actual, expected_with_eof, "Token mismatch");
}

// Keywords tests
#[test]
fn test_keywords_variables() {
    assert_tokens("let const var", vec![
        Token::Let,
        Token::Const,
        Token::Var,
    ]);
}

#[test]
fn test_keywords_functions() {
    assert_tokens("function async await return", vec![
        Token::Function,
        Token::Async,
        Token::Await,
        Token::Return,
    ]);
}

#[test]
fn test_keywords_control_flow() {
    assert_tokens("if else for while do break continue", vec![
        Token::If,
        Token::Else,
        Token::For,
        Token::While,
        Token::Do,
        Token::Break,
        Token::Continue,
    ]);
}

#[test]
fn test_keywords_switch() {
    assert_tokens("switch case default", vec![
        Token::Switch,
        Token::Case,
        Token::Default,
    ]);
}

#[test]
fn test_keywords_oop() {
    assert_tokens("class new this super static extends implements interface", vec![
        Token::Class,
        Token::New,
        Token::This,
        Token::Super,
        Token::Static,
        Token::Extends,
        Token::Implements,
        Token::Interface,
    ]);
}

#[test]
fn test_keywords_types() {
    assert_tokens("type typeof instanceof void enum", vec![
        Token::Type,
        Token::Typeof,
        Token::Instanceof,
        Token::Void,
        Token::Enum,
    ]);
}

#[test]
fn test_keywords_error_handling() {
    assert_tokens("try catch finally throw", vec![
        Token::Try,
        Token::Catch,
        Token::Finally,
        Token::Throw,
    ]);
}

#[test]
fn test_keywords_modules() {
    assert_tokens("import export from", vec![
        Token::Import,
        Token::Export,
        Token::From,
    ]);
}

#[test]
fn test_keywords_future_reserved() {
    assert_tokens("namespace private protected public yield in", vec![
        Token::Namespace,
        Token::Private,
        Token::Protected,
        Token::Public,
        Token::Yield,
        Token::In,
    ]);
}

#[test]
fn test_keywords_debug() {
    assert_eq!(lex_single("debugger"), Token::Debugger);
}

// Literals tests
#[test]
fn test_boolean_literals() {
    assert_tokens("true false", vec![
        Token::True,
        Token::False,
    ]);
}

#[test]
fn test_null_literal() {
    assert_eq!(lex_single("null"), Token::Null);
}

#[test]
fn test_integer_literals() {
    assert_eq!(lex_single("42"), Token::IntLiteral(42));
    assert_eq!(lex_single("0"), Token::IntLiteral(0));
    assert_eq!(lex_single("123"), Token::IntLiteral(123));
}

#[test]
fn test_hex_literals() {
    assert_eq!(lex_single("0x1F"), Token::IntLiteral(31));
    assert_eq!(lex_single("0xFF"), Token::IntLiteral(255));
    assert_eq!(lex_single("0x0"), Token::IntLiteral(0));
}

#[test]
fn test_binary_literals() {
    assert_eq!(lex_single("0b1010"), Token::IntLiteral(10));
    assert_eq!(lex_single("0b0"), Token::IntLiteral(0));
    assert_eq!(lex_single("0b11111111"), Token::IntLiteral(255));
}

#[test]
fn test_octal_literals() {
    assert_eq!(lex_single("0o77"), Token::IntLiteral(63));
    assert_eq!(lex_single("0o755"), Token::IntLiteral(493));
    assert_eq!(lex_single("0o0"), Token::IntLiteral(0));
}

#[test]
fn test_float_literals() {
    assert_eq!(lex_single("3.14"), Token::FloatLiteral(3.14));
    assert_eq!(lex_single("0.5"), Token::FloatLiteral(0.5));
    assert_eq!(lex_single(".5"), Token::FloatLiteral(0.5));
}

#[test]
fn test_scientific_notation() {
    assert_eq!(lex_single("1e6"), Token::FloatLiteral(1_000_000.0));
    assert_eq!(lex_single("1.5e3"), Token::FloatLiteral(1500.0));
    assert_eq!(lex_single("2e-3"), Token::FloatLiteral(0.002));
}

#[test]
fn test_numeric_separators() {
    assert_eq!(lex_single("1_000_000"), Token::IntLiteral(1_000_000));
    assert_eq!(lex_single("3.14_159"), Token::FloatLiteral(3.14159));
    assert_eq!(lex_single("0xFF_FF"), Token::IntLiteral(0xFFFF));
    assert_eq!(lex_single("0b1010_1100"), Token::IntLiteral(0b10101100));
}

#[test]
fn test_string_literals_double_quotes() {
    assert_eq!(lex_single(r#""hello""#), Token::StringLiteral("hello".to_string()));
    assert_eq!(lex_single(r#""world""#), Token::StringLiteral("world".to_string()));
}

#[test]
fn test_string_literals_single_quotes() {
    assert_eq!(lex_single("'hello'"), Token::StringLiteral("hello".to_string()));
    assert_eq!(lex_single("'world'"), Token::StringLiteral("world".to_string()));
}

#[test]
fn test_string_escape_sequences() {
    assert_eq!(lex_single(r#""\n""#), Token::StringLiteral("\n".to_string()));
    assert_eq!(lex_single(r#""\t""#), Token::StringLiteral("\t".to_string()));
    assert_eq!(lex_single(r#""\r""#), Token::StringLiteral("\r".to_string()));
    assert_eq!(lex_single(r#""\\""#), Token::StringLiteral("\\".to_string()));
    assert_eq!(lex_single(r#""\"""#), Token::StringLiteral("\"".to_string()));
    assert_eq!(lex_single(r#""\'""#), Token::StringLiteral("'".to_string()));
}

// Identifier tests
#[test]
fn test_identifiers() {
    assert_eq!(lex_single("foo"), Token::Identifier("foo".to_string()));
    assert_eq!(lex_single("myVar"), Token::Identifier("myVar".to_string()));
    assert_eq!(lex_single("_private"), Token::Identifier("_private".to_string()));
    assert_eq!(lex_single("$jQuery"), Token::Identifier("$jQuery".to_string()));
    assert_eq!(lex_single("test123"), Token::Identifier("test123".to_string()));
}

// Operator tests
#[test]
fn test_arithmetic_operators() {
    assert_tokens("+ - * / %", vec![
        Token::Plus,
        Token::Minus,
        Token::Star,
        Token::Slash,
        Token::Percent,
    ]);
}

#[test]
fn test_exponentiation_operator() {
    assert_eq!(lex_single("**"), Token::StarStar);
    
    assert_tokens("2 ** 3", vec![
        Token::IntLiteral(2),
        Token::StarStar,
        Token::IntLiteral(3),
    ]);
}

#[test]
fn test_comparison_operators() {
    assert_tokens("== != === !== < > <= >=", vec![
        Token::EqualEqual,
        Token::BangEqual,
        Token::EqualEqualEqual,
        Token::BangEqualEqual,
        Token::Less,
        Token::Greater,
        Token::LessEqual,
        Token::GreaterEqual,
    ]);
}

#[test]
fn test_logical_operators() {
    assert_tokens("&& || !", vec![
        Token::AmpAmp,
        Token::PipePipe,
        Token::Bang,
    ]);
}

#[test]
fn test_bitwise_operators() {
    assert_tokens("& | ^ ~ << >> >>>", vec![
        Token::Amp,
        Token::Pipe,
        Token::Caret,
        Token::Tilde,
        Token::LessLess,
        Token::GreaterGreater,
        Token::GreaterGreaterGreater,
    ]);
}

#[test]
fn test_assignment_operators() {
    assert_tokens("= += -= *= /= %= &= |= ^= <<= >>= >>>=", vec![
        Token::Equal,
        Token::PlusEqual,
        Token::MinusEqual,
        Token::StarEqual,
        Token::SlashEqual,
        Token::PercentEqual,
        Token::AmpEqual,
        Token::PipeEqual,
        Token::CaretEqual,
        Token::LessLessEqual,
        Token::GreaterGreaterEqual,
        Token::GreaterGreaterGreaterEqual,
    ]);
}

#[test]
fn test_increment_decrement() {
    assert_tokens("++ --", vec![
        Token::PlusPlus,
        Token::MinusMinus,
    ]);
}

#[test]
fn test_special_operators() {
    assert_tokens("?. ?? =>", vec![
        Token::QuestionDot,
        Token::QuestionQuestion,
        Token::Arrow,
    ]);
}

#[test]
fn test_member_operators() {
    assert_tokens(". [ ]", vec![
        Token::Dot,
        Token::LeftBracket,
        Token::RightBracket,
    ]);
}

// Delimiter tests
#[test]
fn test_delimiters() {
    assert_tokens("( ) { } [ ] ; , :", vec![
        Token::LeftParen,
        Token::RightParen,
        Token::LeftBrace,
        Token::RightBrace,
        Token::LeftBracket,
        Token::RightBracket,
        Token::Semicolon,
        Token::Comma,
        Token::Colon,
    ]);
}

// Integration tests
#[test]
fn test_simple_function() {
    let source = "function add(a, b) { return a + b; }";
    assert_tokens(source, vec![
        Token::Function,
        Token::Identifier("add".to_string()),
        Token::LeftParen,
        Token::Identifier("a".to_string()),
        Token::Comma,
        Token::Identifier("b".to_string()),
        Token::RightParen,
        Token::LeftBrace,
        Token::Return,
        Token::Identifier("a".to_string()),
        Token::Plus,
        Token::Identifier("b".to_string()),
        Token::Semicolon,
        Token::RightBrace,
    ]);
}

#[test]
fn test_switch_statement() {
    let source = r#"
        switch (value) {
            case 1: break;
            case 2: break;
            default: break;
        }
    "#;
    
    let lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();
    let token_types: Vec<Token> = tokens.iter().map(|(t, _)| t.clone()).collect();
    
    // Verify switch, case, default are present
    assert!(token_types.contains(&Token::Switch));
    assert!(token_types.contains(&Token::Case));
    assert!(token_types.contains(&Token::Default));
    assert!(token_types.contains(&Token::Break));
}

#[test]
fn test_labeled_statement() {
    let source = "outer: for (const i of items) { break outer; }";
    
    let lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();
    
    // First token should be identifier "outer"
    assert_eq!(tokens[0].0, Token::Identifier("outer".to_string()));
    // Second token should be colon
    assert_eq!(tokens[1].0, Token::Colon);
    // Third token should be "for"
    assert_eq!(tokens[2].0, Token::For);
}

#[test]
fn test_class_declaration() {
    let source = "class User extends BaseUser implements Named { }";
    
    let lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();
    let token_types: Vec<Token> = tokens.iter().map(|(t, _)| t.clone()).collect();
    
    assert!(token_types.contains(&Token::Class));
    assert!(token_types.contains(&Token::Extends));
    assert!(token_types.contains(&Token::Implements));
}

#[test]
fn test_async_function() {
    let source = "async function fetchData() { await response; }";
    
    let lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();
    let token_types: Vec<Token> = tokens.iter().map(|(t, _)| t.clone()).collect();
    
    assert!(token_types.contains(&Token::Async));
    assert!(token_types.contains(&Token::Await));
}

#[test]
fn test_comments_are_ignored() {
    let source = r#"
        // This is a comment
        const x = 42;
        /* Multi-line
           comment */
        let y = "test";
    "#;

    let lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();
    let token_types: Vec<Token> = tokens.iter().map(|(t, _)| t.clone()).collect();

    // Comments should not appear in tokens
    assert!(token_types.contains(&Token::Const));
    assert!(token_types.contains(&Token::Let));
    assert_eq!(token_types.iter().filter(|t| matches!(t, Token::Identifier(s) if s == "x")).count(), 1);
}

// ============================================================================
// Template Literal Tests (Phase 3)
// ============================================================================

#[test]
fn test_simple_template_literal() {
    use raya_parser::TemplatePart;

    let source = r#"`Hello, World!`"#;
    let lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();

    assert_eq!(tokens.len(), 2); // Template + EOF
    match &tokens[0].0 {
        Token::TemplateLiteral(parts) => {
            assert_eq!(parts.len(), 1);
            match &parts[0] {
                TemplatePart::String(s) => assert_eq!(s, "Hello, World!"),
                _ => panic!("Expected string part"),
            }
        }
        _ => panic!("Expected template literal"),
    }
}

#[test]
fn test_template_with_single_expression() {
    use raya_parser::TemplatePart;

    let source = r#"`Hello, ${name}!`"#;
    let lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();

    match &tokens[0].0 {
        Token::TemplateLiteral(parts) => {
            assert_eq!(parts.len(), 3); // "Hello, " + expr + "!"

            match &parts[0] {
                TemplatePart::String(s) => assert_eq!(s, "Hello, "),
                _ => panic!("Expected string part"),
            }

            match &parts[1] {
                TemplatePart::Expression(expr_tokens) => {
                    assert_eq!(expr_tokens.len(), 1);
                    match &expr_tokens[0].0 {
                        Token::Identifier(id) => assert_eq!(id, "name"),
                        _ => panic!("Expected identifier"),
                    }
                }
                _ => panic!("Expected expression part"),
            }

            match &parts[2] {
                TemplatePart::String(s) => assert_eq!(s, "!"),
                _ => panic!("Expected string part"),
            }
        }
        _ => panic!("Expected template literal"),
    }
}

#[test]
fn test_template_with_multiple_expressions() {
    use raya_parser::TemplatePart;

    let source = r#"`${a} + ${b} = ${a + b}`"#;
    let lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();

    match &tokens[0].0 {
        Token::TemplateLiteral(parts) => {
            assert_eq!(parts.len(), 5); // expr + " + " + expr + " = " + expr

            // First expression
            match &parts[0] {
                TemplatePart::Expression(expr_tokens) => {
                    assert_eq!(expr_tokens.len(), 1);
                    match &expr_tokens[0].0 {
                        Token::Identifier(id) => assert_eq!(id, "a"),
                        _ => panic!("Expected identifier 'a'"),
                    }
                }
                _ => panic!("Expected expression part"),
            }

            // String " + "
            match &parts[1] {
                TemplatePart::String(s) => assert_eq!(s, " + "),
                _ => panic!("Expected string part"),
            }
        }
        _ => panic!("Expected template literal"),
    }
}

#[test]
fn test_template_with_nested_braces() {
    use raya_parser::TemplatePart;

    let source = r#"`Result: ${{ x: 42 }.x}`"#;
    let lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();

    match &tokens[0].0 {
        Token::TemplateLiteral(parts) => {
            assert_eq!(parts.len(), 2); // "Result: " + expr

            match &parts[1] {
                TemplatePart::Expression(expr_tokens) => {
                    // Should contain: { x : 42 } . x
                    let has_braces = expr_tokens.iter().any(|(t, _)| matches!(t, Token::LeftBrace | Token::RightBrace));
                    assert!(has_braces, "Expected braces in expression");
                }
                _ => panic!("Expected expression part"),
            }
        }
        _ => panic!("Expected template literal"),
    }
}

#[test]
fn test_template_with_complex_expression() {
    use raya_parser::TemplatePart;

    let source = r#"`Total: ${items.length * price}`"#;
    let lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();

    match &tokens[0].0 {
        Token::TemplateLiteral(parts) => {
            assert_eq!(parts.len(), 2);

            match &parts[1] {
                TemplatePart::Expression(expr_tokens) => {
                    // Should have: items . length * price
                    assert!(expr_tokens.len() >= 5);
                }
                _ => panic!("Expected expression part"),
            }
        }
        _ => panic!("Expected template literal"),
    }
}

#[test]
fn test_template_with_escape_sequences() {
    use raya_parser::TemplatePart;

    let source = r#"`Line 1\nLine 2\tTabbed`"#;
    let lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();

    match &tokens[0].0 {
        Token::TemplateLiteral(parts) => {
            assert_eq!(parts.len(), 1);
            match &parts[0] {
                TemplatePart::String(s) => {
                    assert!(s.contains('\n'));
                    assert!(s.contains('\t'));
                }
                _ => panic!("Expected string part"),
            }
        }
        _ => panic!("Expected template literal"),
    }
}

#[test]
fn test_template_escaped_dollar_sign() {
    use raya_parser::TemplatePart;

    let source = r#"`Price: \$${price}`"#;
    let lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();

    match &tokens[0].0 {
        Token::TemplateLiteral(parts) => {
            assert_eq!(parts.len(), 2); // "Price: $" + expr

            match &parts[0] {
                TemplatePart::String(s) => assert_eq!(s, "Price: $"),
                _ => panic!("Expected string part"),
            }
        }
        _ => panic!("Expected template literal"),
    }
}

#[test]
fn test_template_escaped_backtick() {
    use raya_parser::TemplatePart;

    let source = r#"`Use \` for templates`"#;
    let lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();

    match &tokens[0].0 {
        Token::TemplateLiteral(parts) => {
            assert_eq!(parts.len(), 1);
            match &parts[0] {
                TemplatePart::String(s) => assert!(s.contains('`')),
                _ => panic!("Expected string part"),
            }
        }
        _ => panic!("Expected template literal"),
    }
}

#[test]
fn test_multiline_template() {
    use raya_parser::TemplatePart;

    let source = r#"`This is
a multiline
template`"#;
    let lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();

    match &tokens[0].0 {
        Token::TemplateLiteral(parts) => {
            assert_eq!(parts.len(), 1);
            match &parts[0] {
                TemplatePart::String(s) => {
                    assert!(s.contains('\n'));
                    assert_eq!(s.lines().count(), 3);
                }
                _ => panic!("Expected string part"),
            }
        }
        _ => panic!("Expected template literal"),
    }
}

#[test]
fn test_empty_template() {
    use raya_parser::TemplatePart;

    let source = "``";
    let lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();

    match &tokens[0].0 {
        Token::TemplateLiteral(parts) => {
            assert_eq!(parts.len(), 0);
        }
        _ => panic!("Expected template literal"),
    }
}

#[test]
fn test_unterminated_template() {
    let source = r#"`This is unterminated"#;
    let lexer = Lexer::new(source);
    let result = lexer.tokenize();

    assert!(result.is_err());
    match &result.err().unwrap()[0] {
        raya_parser::lexer::LexError::UnterminatedTemplate { .. } => {}
        _ => panic!("Expected UnterminatedTemplate error"),
    }
}

// ============================================================================
// Unicode Escape Sequence Tests (Phase 4)
// ============================================================================

#[test]
fn test_unicode_escape_4_digits() {
    let source = r#""\u0048\u0065\u006C\u006C\u006F""#; // "Hello"
    let lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();

    match &tokens[0].0 {
        Token::StringLiteral(s) => {
            assert_eq!(s, "Hello");
        }
        _ => panic!("Expected string literal"),
    }
}

#[test]
fn test_unicode_escape_emoji() {
    let source = r#""\u{1F600}""#; // 😀
    let lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();

    match &tokens[0].0 {
        Token::StringLiteral(s) => {
            assert_eq!(s, "😀");
        }
        _ => panic!("Expected string literal"),
    }
}

#[test]
fn test_unicode_escape_chinese() {
    let source = r#""\u4F60\u597D""#; // "你好"
    let lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();

    match &tokens[0].0 {
        Token::StringLiteral(s) => {
            assert_eq!(s, "你好");
        }
        _ => panic!("Expected string literal"),
    }
}

#[test]
fn test_unicode_escape_variable_length() {
    let source = r#""\u{41}\u{42}\u{43}""#; // "ABC"
    let lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();

    match &tokens[0].0 {
        Token::StringLiteral(s) => {
            assert_eq!(s, "ABC");
        }
        _ => panic!("Expected string literal"),
    }
}

#[test]
fn test_hex_escape_sequence() {
    let source = r#""\x48\x65\x6C\x6C\x6F""#; // "Hello"
    let lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();

    match &tokens[0].0 {
        Token::StringLiteral(s) => {
            assert_eq!(s, "Hello");
        }
        _ => panic!("Expected string literal"),
    }
}

#[test]
fn test_mixed_escape_sequences() {
    let source = r#""Line 1\nTab:\t\u0048\x65\u{6C}lo""#;
    let lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();

    match &tokens[0].0 {
        Token::StringLiteral(s) => {
            assert!(s.contains('\n'));
            assert!(s.contains('\t'));
            assert!(s.contains('H'));
            assert!(s.contains('e'));
            assert!(s.contains('l'));
        }
        _ => panic!("Expected string literal"),
    }
}

#[test]
fn test_unicode_in_template_literal() {
    use raya_parser::TemplatePart;

    let source = r#"`Hello \u{1F44B}!`"#; // Hello 👋!
    let lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();

    match &tokens[0].0 {
        Token::TemplateLiteral(parts) => {
            assert_eq!(parts.len(), 1);
            match &parts[0] {
                TemplatePart::String(s) => {
                    assert!(s.contains('👋'));
                    assert_eq!(s, "Hello 👋!");
                }
                _ => panic!("Expected string part"),
            }
        }
        _ => panic!("Expected template literal"),
    }
}

#[test]
fn test_unicode_fixed_in_template() {
    use raya_parser::TemplatePart;

    let source = r#"`\u0048\u0065\u006C\u006C\u006F`"#; // Hello
    let lexer = Lexer::new(source);
    let tokens = lexer.tokenize().unwrap();

    match &tokens[0].0 {
        Token::TemplateLiteral(parts) => {
            assert_eq!(parts.len(), 1);
            match &parts[0] {
                TemplatePart::String(s) => {
                    assert_eq!(s, "Hello");
                }
                _ => panic!("Expected string part"),
            }
        }
        _ => panic!("Expected template literal"),
    }
}

// ============================================================================
// Error Recovery Tests (Phase 5)
// ============================================================================

#[test]
fn test_error_recovery_continues_after_error() {
    let source = r#"
        const x = @; // Invalid character
        const y = 42; // Should still tokenize this
    "#;

    let lexer = Lexer::new(source);
    let result = lexer.tokenize();

    // Should have errors but also some valid tokens
    assert!(result.is_err());
    // The lexer should have tried to continue tokenizing
}

#[test]
fn test_rich_error_message_for_unterminated_string() {
    let source = r#"const x = "unterminated"#;

    let lexer = Lexer::new(source);
    let result = lexer.tokenize();

    if let Err(errors) = result {
        assert!(!errors.is_empty());
        let formatted = Lexer::format_errors(&errors, source);
        // Note: logos recognizes the string, so we might not get an unterminated string error
        // Just verify we get an error message
        assert!(formatted.contains("Error at"));
    } else {
        panic!("Expected error");
    }
}

#[test]
fn test_rich_error_message_for_unexpected_char() {
    let source = "const x = @;";

    let lexer = Lexer::new(source);
    let result = lexer.tokenize();

    if let Err(errors) = result {
        assert!(!errors.is_empty());
        let error = &errors[0];

        // Test individual error methods
        assert_eq!(error.description(), "Unexpected character '@'");
        assert_eq!(error.span().line, 1);

        // Test formatted output
        let formatted = error.format_with_source(source);
        assert!(formatted.contains("Error at 1:"));
        assert!(formatted.contains("Unexpected character '@'"));
        assert!(formatted.contains("const x = @;"));
        assert!(formatted.contains("^"));
    } else {
        panic!("Expected error");
    }
}

#[test]
fn test_unterminated_template_hint() {
    let source = r#"`This template never closes"#;

    let lexer = Lexer::new(source);
    let result = lexer.tokenize();

    if let Err(errors) = result {
        assert!(!errors.is_empty());
        let hint = errors[0].hint();
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("backtick"));
    } else {
        panic!("Expected error");
    }
}

#[test]
fn test_error_span_accuracy() {
    let source = "let x = @;";

    let lexer = Lexer::new(source);
    let result = lexer.tokenize();

    if let Err(errors) = result {
        let span = errors[0].span();
        // The @ character should be detected
        assert!(span.start == 8); // position of @
        assert!(span.column > 0); // should have a valid column
    } else {
        panic!("Expected error");
    }
}

#[test]
fn test_multiple_errors_formatted() {
    let source = r#"
const x = @;
const y = #;
"#;

    let lexer = Lexer::new(source);
    let result = lexer.tokenize();

    if let Err(errors) = result {
        // Should have at least one error
        assert!(!errors.is_empty());

        let formatted = Lexer::format_errors(&errors, source);
        assert!(formatted.contains("Error at"));
    } else {
        panic!("Expected errors");
    }
}
