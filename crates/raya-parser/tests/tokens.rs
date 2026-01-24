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
