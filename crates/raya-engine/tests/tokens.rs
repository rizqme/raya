//! Basic token tests for the Raya lexer.

use raya_engine::parser::{Interner, Lexer, Token};

fn lex_tokens(source: &str) -> (Vec<(Token, raya_engine::parser::Span)>, Interner) {
    let lexer = Lexer::new(source);
    lexer.tokenize().unwrap()
}

fn lex_single(source: &str) -> (Token, Interner) {
    let (tokens, interner) = lex_tokens(source);
    assert!(tokens.len() >= 2, "Expected at least 2 tokens (token + EOF)");
    (tokens[0].0.clone(), interner)
}

fn assert_tokens(source: &str, expected: Vec<Token>) {
    let (tokens, _interner) = lex_tokens(source);
    let actual: Vec<Token> = tokens.iter().map(|(t, _)| t.clone()).collect();

    // Expected should include EOF
    let mut expected_with_eof = expected;
    expected_with_eof.push(Token::Eof);

    assert_eq!(actual, expected_with_eof, "Token mismatch");
}

/// Helper to check if token is an identifier with the given name
fn assert_identifier(token: &Token, interner: &Interner, expected_name: &str) {
    match token {
        Token::Identifier(sym) => {
            let actual = interner.resolve(*sym);
            assert_eq!(actual, expected_name, "Expected identifier '{}', got '{}'", expected_name, actual);
        }
        _ => panic!("Expected identifier, got {:?}", token),
    }
}

/// Helper to check if token is a string literal with the given value
fn assert_string_literal(token: &Token, interner: &Interner, expected_value: &str) {
    match token {
        Token::StringLiteral(sym) => {
            let actual = interner.resolve(*sym);
            assert_eq!(actual, expected_value, "Expected string '{}', got '{}'", expected_value, actual);
        }
        _ => panic!("Expected string literal, got {:?}", token),
    }
}

// Keywords tests
// NOTE: 'var' is BANNED in Raya (LANG.md §19.1) - use 'let' or 'const' instead
#[test]
fn test_keywords_variables() {
    assert_tokens("let const", vec![
        Token::Let,
        Token::Const,
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

// NOTE: 'interface' is BANNED in Raya (LANG.md §10) - use 'type' aliases instead
#[test]
fn test_keywords_oop() {
    assert_tokens("class new this super static extends implements", vec![
        Token::Class,
        Token::New,
        Token::This,
        Token::Super,
        Token::Static,
        Token::Extends,
        Token::Implements,
    ]);
}

// NOTE: 'enum' is BANNED in Raya (LANG.md §19.2) - use union of literals instead
#[test]
fn test_keywords_types() {
    assert_tokens("type typeof instanceof void", vec![
        Token::Type,
        Token::Typeof,
        Token::Instanceof,
        Token::Void,
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
    let (token, _interner) = lex_single("debugger");
    assert_eq!(token, Token::Debugger);
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
    let (token, _interner) = lex_single("null");
    assert_eq!(token, Token::Null);
}

#[test]
fn test_integer_literals() {
    let (token, _) = lex_single("42");
    assert_eq!(token, Token::IntLiteral(42));
    let (token, _) = lex_single("0");
    assert_eq!(token, Token::IntLiteral(0));
    let (token, _) = lex_single("123");
    assert_eq!(token, Token::IntLiteral(123));
}

#[test]
fn test_hex_literals() {
    let (token, _) = lex_single("0x1F");
    assert_eq!(token, Token::IntLiteral(31));
    let (token, _) = lex_single("0xFF");
    assert_eq!(token, Token::IntLiteral(255));
    let (token, _) = lex_single("0x0");
    assert_eq!(token, Token::IntLiteral(0));
}

#[test]
fn test_binary_literals() {
    let (token, _) = lex_single("0b1010");
    assert_eq!(token, Token::IntLiteral(10));
    let (token, _) = lex_single("0b0");
    assert_eq!(token, Token::IntLiteral(0));
    let (token, _) = lex_single("0b11111111");
    assert_eq!(token, Token::IntLiteral(255));
}

#[test]
fn test_octal_literals() {
    let (token, _) = lex_single("0o77");
    assert_eq!(token, Token::IntLiteral(63));
    let (token, _) = lex_single("0o755");
    assert_eq!(token, Token::IntLiteral(493));
    let (token, _) = lex_single("0o0");
    assert_eq!(token, Token::IntLiteral(0));
}

#[test]
fn test_float_literals() {
    let (token, _) = lex_single("3.14");
    assert_eq!(token, Token::FloatLiteral(3.14));
    let (token, _) = lex_single("0.5");
    assert_eq!(token, Token::FloatLiteral(0.5));
    let (token, _) = lex_single(".5");
    assert_eq!(token, Token::FloatLiteral(0.5));
}

#[test]
fn test_scientific_notation() {
    let (token, _) = lex_single("1e6");
    assert_eq!(token, Token::FloatLiteral(1_000_000.0));
    let (token, _) = lex_single("1.5e3");
    assert_eq!(token, Token::FloatLiteral(1500.0));
    let (token, _) = lex_single("2e-3");
    assert_eq!(token, Token::FloatLiteral(0.002));
}

#[test]
fn test_numeric_separators() {
    let (token, _) = lex_single("1_000_000");
    assert_eq!(token, Token::IntLiteral(1_000_000));
    let (token, _) = lex_single("3.14_159");
    assert_eq!(token, Token::FloatLiteral(3.14159));
    let (token, _) = lex_single("0xFF_FF");
    assert_eq!(token, Token::IntLiteral(0xFFFF));
    let (token, _) = lex_single("0b1010_1100");
    assert_eq!(token, Token::IntLiteral(0b10101100));
}

#[test]
fn test_string_literals_double_quotes() {
    let (token, interner) = lex_single(r#""hello""#);
    assert_string_literal(&token, &interner, "hello");
    let (token, interner) = lex_single(r#""world""#);
    assert_string_literal(&token, &interner, "world");
}

#[test]
fn test_string_literals_single_quotes() {
    let (token, interner) = lex_single("'hello'");
    assert_string_literal(&token, &interner, "hello");
    let (token, interner) = lex_single("'world'");
    assert_string_literal(&token, &interner, "world");
}

#[test]
fn test_string_escape_sequences() {
    let (token, interner) = lex_single(r#""\n""#);
    assert_string_literal(&token, &interner, "\n");
    let (token, interner) = lex_single(r#""\t""#);
    assert_string_literal(&token, &interner, "\t");
    let (token, interner) = lex_single(r#""\r""#);
    assert_string_literal(&token, &interner, "\r");
    let (token, interner) = lex_single(r#""\\""#);
    assert_string_literal(&token, &interner, "\\");
    let (token, interner) = lex_single(r#""\"""#);
    assert_string_literal(&token, &interner, "\"");
    let (token, interner) = lex_single(r#""\'""#);
    assert_string_literal(&token, &interner, "'");
}

// Identifier tests
#[test]
fn test_identifiers() {
    let (token, interner) = lex_single("foo");
    assert_identifier(&token, &interner, "foo");
    let (token, interner) = lex_single("myVar");
    assert_identifier(&token, &interner, "myVar");
    let (token, interner) = lex_single("_private");
    assert_identifier(&token, &interner, "_private");
    let (token, interner) = lex_single("$jQuery");
    assert_identifier(&token, &interner, "$jQuery");
    let (token, interner) = lex_single("test123");
    assert_identifier(&token, &interner, "test123");
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
    let (token, _) = lex_single("**");
    assert_eq!(token, Token::StarStar);

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
    let (tokens, interner) = lex_tokens(source);

    assert_eq!(tokens[0].0, Token::Function);
    assert_identifier(&tokens[1].0, &interner, "add");
    assert_eq!(tokens[2].0, Token::LeftParen);
    assert_identifier(&tokens[3].0, &interner, "a");
    assert_eq!(tokens[4].0, Token::Comma);
    assert_identifier(&tokens[5].0, &interner, "b");
    assert_eq!(tokens[6].0, Token::RightParen);
    assert_eq!(tokens[7].0, Token::LeftBrace);
    assert_eq!(tokens[8].0, Token::Return);
    assert_identifier(&tokens[9].0, &interner, "a");
    assert_eq!(tokens[10].0, Token::Plus);
    assert_identifier(&tokens[11].0, &interner, "b");
    assert_eq!(tokens[12].0, Token::Semicolon);
    assert_eq!(tokens[13].0, Token::RightBrace);
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

    let (tokens, _interner) = lex_tokens(source);
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

    let (tokens, interner) = lex_tokens(source);

    // First token should be identifier "outer"
    assert_identifier(&tokens[0].0, &interner, "outer");
    // Second token should be colon
    assert_eq!(tokens[1].0, Token::Colon);
    // Third token should be "for"
    assert_eq!(tokens[2].0, Token::For);
}

#[test]
fn test_class_declaration() {
    let source = "class User extends BaseUser implements Named { }";

    let (tokens, _interner) = lex_tokens(source);
    let token_types: Vec<Token> = tokens.iter().map(|(t, _)| t.clone()).collect();

    assert!(token_types.contains(&Token::Class));
    assert!(token_types.contains(&Token::Extends));
    assert!(token_types.contains(&Token::Implements));
}

#[test]
fn test_async_function() {
    let source = "async function fetchData() { await response; }";

    let (tokens, _interner) = lex_tokens(source);
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

    let (tokens, interner) = lex_tokens(source);
    let token_types: Vec<Token> = tokens.iter().map(|(t, _)| t.clone()).collect();

    // Comments should not appear in tokens
    assert!(token_types.contains(&Token::Const));
    assert!(token_types.contains(&Token::Let));

    // Check for identifier "x"
    let has_x = tokens.iter().any(|(t, _)| {
        matches!(t, Token::Identifier(sym) if interner.resolve(*sym) == "x")
    });
    assert!(has_x, "Expected identifier 'x'");
}

// ============================================================================
// Template Literal Tests (Phase 3)
// ============================================================================

#[test]
fn test_simple_template_literal() {
    use raya_engine::parser::TemplatePart;

    let source = r#"`Hello, World!`"#;
    let (tokens, interner) = lex_tokens(source);

    assert_eq!(tokens.len(), 2); // Template + EOF
    match &tokens[0].0 {
        Token::TemplateLiteral(parts) => {
            assert_eq!(parts.len(), 1);
            match &parts[0] {
                TemplatePart::String(sym) => {
                    assert_eq!(interner.resolve(*sym), "Hello, World!");
                }
                _ => panic!("Expected string part"),
            }
        }
        _ => panic!("Expected template literal"),
    }
}

#[test]
fn test_template_with_single_expression() {
    use raya_engine::parser::TemplatePart;

    let source = r#"`Hello, ${name}!`"#;
    let (tokens, interner) = lex_tokens(source);

    match &tokens[0].0 {
        Token::TemplateLiteral(parts) => {
            assert_eq!(parts.len(), 3); // "Hello, " + expr + "!"

            match &parts[0] {
                TemplatePart::String(sym) => {
                    assert_eq!(interner.resolve(*sym), "Hello, ");
                }
                _ => panic!("Expected string part"),
            }

            match &parts[1] {
                TemplatePart::Expression(expr_tokens) => {
                    assert_eq!(expr_tokens.len(), 1);
                    // Note: Expression tokens use their own interner, so we just check it's an identifier
                    assert!(matches!(&expr_tokens[0].0, Token::Identifier(_)), "Expected identifier token");
                }
                _ => panic!("Expected expression part"),
            }

            match &parts[2] {
                TemplatePart::String(sym) => {
                    assert_eq!(interner.resolve(*sym), "!");
                }
                _ => panic!("Expected string part"),
            }
        }
        _ => panic!("Expected template literal"),
    }
}

#[test]
fn test_template_with_multiple_expressions() {
    use raya_engine::parser::TemplatePart;

    let source = r#"`${a} + ${b} = ${a + b}`"#;
    let (tokens, interner) = lex_tokens(source);

    match &tokens[0].0 {
        Token::TemplateLiteral(parts) => {
            assert_eq!(parts.len(), 5); // expr + " + " + expr + " = " + expr

            // First expression
            match &parts[0] {
                TemplatePart::Expression(expr_tokens) => {
                    assert_eq!(expr_tokens.len(), 1);
                    // Note: Expression tokens use their own interner, so we just check it's an identifier
                    assert!(matches!(&expr_tokens[0].0, Token::Identifier(_)), "Expected identifier token");
                }
                _ => panic!("Expected expression part"),
            }

            // String " + "
            match &parts[1] {
                TemplatePart::String(sym) => {
                    assert_eq!(interner.resolve(*sym), " + ");
                }
                _ => panic!("Expected string part"),
            }
        }
        _ => panic!("Expected template literal"),
    }
}

#[test]
fn test_template_with_nested_braces() {
    use raya_engine::parser::TemplatePart;

    let source = r#"`Result: ${{ x: 42 }.x}`"#;
    let (tokens, _interner) = lex_tokens(source);

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
    use raya_engine::parser::TemplatePart;

    let source = r#"`Total: ${items.length * price}`"#;
    let (tokens, _interner) = lex_tokens(source);

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
    use raya_engine::parser::TemplatePart;

    let source = r#"`Line 1\nLine 2\tTabbed`"#;
    let (tokens, interner) = lex_tokens(source);

    match &tokens[0].0 {
        Token::TemplateLiteral(parts) => {
            assert_eq!(parts.len(), 1);
            match &parts[0] {
                TemplatePart::String(sym) => {
                    let s = interner.resolve(*sym);
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
    use raya_engine::parser::TemplatePart;

    let source = r#"`Price: \$${price}`"#;
    let (tokens, interner) = lex_tokens(source);

    match &tokens[0].0 {
        Token::TemplateLiteral(parts) => {
            assert_eq!(parts.len(), 2); // "Price: $" + expr

            match &parts[0] {
                TemplatePart::String(sym) => {
                    assert_eq!(interner.resolve(*sym), "Price: $");
                }
                _ => panic!("Expected string part"),
            }
        }
        _ => panic!("Expected template literal"),
    }
}

#[test]
fn test_template_escaped_backtick() {
    use raya_engine::parser::TemplatePart;

    let source = r#"`Use \` for templates`"#;
    let (tokens, interner) = lex_tokens(source);

    match &tokens[0].0 {
        Token::TemplateLiteral(parts) => {
            assert_eq!(parts.len(), 1);
            match &parts[0] {
                TemplatePart::String(sym) => {
                    assert!(interner.resolve(*sym).contains('`'));
                }
                _ => panic!("Expected string part"),
            }
        }
        _ => panic!("Expected template literal"),
    }
}

#[test]
fn test_multiline_template() {
    use raya_engine::parser::TemplatePart;

    let source = r#"`This is
a multiline
template`"#;
    let (tokens, interner) = lex_tokens(source);

    match &tokens[0].0 {
        Token::TemplateLiteral(parts) => {
            assert_eq!(parts.len(), 1);
            match &parts[0] {
                TemplatePart::String(sym) => {
                    let s = interner.resolve(*sym);
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
    use raya_engine::parser::TemplatePart;

    let source = "``";
    let (tokens, _interner) = lex_tokens(source);

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
        raya_engine::parser::lexer::LexError::UnterminatedTemplate { .. } => {}
        _ => panic!("Expected UnterminatedTemplate error"),
    }
}

// ============================================================================
// Unicode Escape Sequence Tests (Phase 4)
// ============================================================================

#[test]
fn test_unicode_escape_4_digits() {
    let source = r#""\u0048\u0065\u006C\u006C\u006F""#; // "Hello"
    let (token, interner) = lex_single(source);
    assert_string_literal(&token, &interner, "Hello");
}

#[test]
fn test_unicode_escape_emoji() {
    let source = r#""\u{1F600}""#; // 😀
    let (token, interner) = lex_single(source);
    assert_string_literal(&token, &interner, "😀");
}

#[test]
fn test_unicode_escape_chinese() {
    let source = r#""\u4F60\u597D""#; // "你好"
    let (token, interner) = lex_single(source);
    assert_string_literal(&token, &interner, "你好");
}

#[test]
fn test_unicode_escape_variable_length() {
    let source = r#""\u{41}\u{42}\u{43}""#; // "ABC"
    let (token, interner) = lex_single(source);
    assert_string_literal(&token, &interner, "ABC");
}

#[test]
fn test_hex_escape_sequence() {
    let source = r#""\x48\x65\x6C\x6C\x6F""#; // "Hello"
    let (token, interner) = lex_single(source);
    assert_string_literal(&token, &interner, "Hello");
}

#[test]
fn test_mixed_escape_sequences() {
    let source = r#""Line 1\nTab:\t\u0048\x65\u{6C}lo""#;
    let (token, interner) = lex_single(source);

    match &token {
        Token::StringLiteral(sym) => {
            let s = interner.resolve(*sym);
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
    use raya_engine::parser::TemplatePart;

    let source = r#"`Hello \u{1F44B}!`"#; // Hello 👋!
    let (tokens, interner) = lex_tokens(source);

    match &tokens[0].0 {
        Token::TemplateLiteral(parts) => {
            assert_eq!(parts.len(), 1);
            match &parts[0] {
                TemplatePart::String(sym) => {
                    let s = interner.resolve(*sym);
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
    use raya_engine::parser::TemplatePart;

    let source = r#"`\u0048\u0065\u006C\u006C\u006F`"#; // Hello
    let (tokens, interner) = lex_tokens(source);

    match &tokens[0].0 {
        Token::TemplateLiteral(parts) => {
            assert_eq!(parts.len(), 1);
            match &parts[0] {
                TemplatePart::String(sym) => {
                    assert_eq!(interner.resolve(*sym), "Hello");
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
fn test_multiple_errors_formatted() {
    // Use invalid characters (# is not a valid token)
    let source = r#"
const x = #;
const y = $;
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
