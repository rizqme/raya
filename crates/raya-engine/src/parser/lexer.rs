//! Lexer for the Raya programming language.
//!
//! This module implements a high-performance lexer using the logos library.
//! It converts source code into a stream of tokens with precise source location information.

use crate::parser::token::{Span, TemplatePart, Token};
use crate::parser::interner::Interner;
use logos::Logos;

/// Logos-based token enum for lexing.
///
/// This enum is used internally by logos for efficient tokenization.
/// It's converted to our main Token enum after lexing.
#[derive(Logos, Debug, Clone, PartialEq)]
enum LogosToken {
    // Whitespace (skip)
    #[regex(r"[ \t\r\n]+", logos::skip)]
    Whitespace,

    // Line comments are handled in the manual whitespace loop (which checks for //@@)
    // This regex is kept for any edge cases but should rarely be hit
    #[regex(r"//[^@\n][^\n]*", logos::skip)]  // Skip // followed by non-@
    #[regex(r"//@[^@\n][^\n]*", logos::skip)]  // Skip //@ followed by non-@
    #[regex(r"//\n", logos::skip)]  // Skip empty line comment
    LineComment,

    #[regex(r"/\*", lex_block_comment)]
    BlockComment,

    // Compiler annotations: //@@tag or //@@tag value
    // Examples: //@@json, //@@json user_name, //@@json age,omitempty, //@@json -
    #[regex(r"//@@[a-zA-Z_][a-zA-Z0-9_]*( [^\n]*)?", parse_annotation)]
    Annotation(String),

    // Keywords (must come before identifiers)
    #[token("function")]
    Function,

    #[token("class")]
    Class,

    #[token("type")]
    Type,

    #[token("let")]
    Let,

    #[token("const")]
    Const,

    // Note: 'interface', 'enum', and 'var' are BANNED in Raya

    #[token("if")]
    If,

    #[token("else")]
    Else,

    #[token("switch")]
    Switch,

    #[token("case")]
    Case,

    #[token("default")]
    Default,

    #[token("for")]
    For,

    #[token("while")]
    While,

    #[token("do")]
    Do,

    #[token("break")]
    Break,

    #[token("continue")]
    Continue,

    #[token("return")]
    Return,

    #[token("async")]
    Async,

    #[token("await")]
    Await,

    #[token("try")]
    Try,

    #[token("catch")]
    Catch,

    #[token("finally")]
    Finally,

    #[token("throw")]
    Throw,

    #[token("import")]
    Import,

    #[token("export")]
    Export,

    #[token("from")]
    From,

    #[token("new")]
    New,

    #[token("this")]
    This,

    #[token("super")]
    Super,

    #[token("static")]
    Static,

    #[token("abstract")]
    Abstract,

    #[token("extends")]
    Extends,

    #[token("implements")]
    Implements,

    #[token("typeof")]
    Typeof,

    #[token("instanceof")]
    Instanceof,

    #[token("as")]
    As,

    #[token("delete")]
    Delete,

    #[token("void")]
    Void,

    #[token("debugger")]
    Debugger,

    #[token("namespace")]
    Namespace,

    #[token("private")]
    Private,

    #[token("protected")]
    Protected,

    #[token("public")]
    Public,

    #[token("yield")]
    Yield,

    #[token("in")]
    In,

    #[token("of")]
    Of,

    #[token("true")]
    True,

    #[token("false")]
    False,

    #[token("null")]
    Null,

    // Identifiers (must come after keywords)
    #[regex(r"[a-zA-Z_$][a-zA-Z0-9_$]*", |lex| lex.slice().to_string())]
    Identifier(String),

    // Numbers with numeric separator support
    #[regex(r"0x[0-9a-fA-F]+(_[0-9a-fA-F]+)*", parse_hex)]
    #[regex(r"0b[01]+(_[01]+)*", parse_binary)]
    #[regex(r"0o[0-7]+(_[0-7]+)*", parse_octal)]
    #[regex(r"[0-9]+(_[0-9]+)*", parse_int)]
    IntLiteral(i64),

    #[regex(r"[0-9]+(_[0-9]+)*\.[0-9]+(_[0-9]+)*([eE][+-]?[0-9]+(_[0-9]+)*)?", parse_float)]
    #[regex(r"[0-9]+(_[0-9]+)*[eE][+-]?[0-9]+(_[0-9]+)*", parse_float)]
    #[regex(r"\.[0-9]+(_[0-9]+)*([eE][+-]?[0-9]+(_[0-9]+)*)?", parse_float)]
    FloatLiteral(f64),

    // Strings
    #[regex(r#""([^"\\]|\\.)*""#, parse_string)]
    #[regex(r"'([^'\\]|\\.)*'", parse_string)]
    StringLiteral(String),

    // Template literal start
    #[token("`")]
    Backtick,

    // Operators (3-char must come before 2-char, 2-char before 1-char)
    #[token("===")]
    EqualEqualEqual,

    #[token("!==")]
    BangEqualEqual,

    #[token(">>>")]
    GreaterGreaterGreater,

    #[token(">>>=")]
    GreaterGreaterGreaterEqual,

    #[token("**")]
    StarStar,

    #[token("==")]
    EqualEqual,

    #[token("!=")]
    BangEqual,

    #[token("<=")]
    LessEqual,

    #[token(">=")]
    GreaterEqual,

    #[token("&&")]
    AmpAmp,

    #[token("||")]
    PipePipe,

    #[token("++")]
    PlusPlus,

    #[token("--")]
    MinusMinus,

    #[token("<<")]
    LessLess,

    #[token(">>")]
    GreaterGreater,

    #[token("<<=")]
    LessLessEqual,

    #[token(">>=")]
    GreaterGreaterEqual,

    #[token("?.")]
    QuestionDot,

    #[token("??=")]
    QuestionQuestionEqual,

    #[token("??")]
    QuestionQuestion,

    #[token("=>")]
    Arrow,

    #[token("+=")]
    PlusEqual,

    #[token("-=")]
    MinusEqual,

    #[token("*=")]
    StarEqual,

    #[token("/=")]
    SlashEqual,

    #[token("%=")]
    PercentEqual,

    #[token("&=")]
    AmpEqual,

    #[token("|=")]
    PipeEqual,

    #[token("^=")]
    CaretEqual,

    // Single-character tokens
    #[token("+")]
    Plus,

    #[token("-")]
    Minus,

    #[token("*")]
    Star,

    #[token("/")]
    Slash,

    #[token("%")]
    Percent,

    #[token("!")]
    Bang,

    #[token("~")]
    Tilde,

    #[token("<")]
    Less,

    #[token(">")]
    Greater,

    #[token("&")]
    Amp,

    #[token("|")]
    Pipe,

    #[token("^")]
    Caret,

    #[token("=")]
    Equal,

    #[token("?")]
    Question,

    #[token("...")]
    DotDotDot,

    #[token(".")]
    Dot,

    #[token(":")]
    Colon,

    #[token("@")]
    At,

    #[token("(")]
    LeftParen,

    #[token(")")]
    RightParen,

    #[token("{")]
    LeftBrace,

    #[token("}")]
    RightBrace,

    #[token("[")]
    LeftBracket,

    #[token("]")]
    RightBracket,

    #[token(";")]
    Semicolon,

    #[token(",")]
    Comma,
}

// Helper parsing functions
fn lex_block_comment(lex: &mut logos::Lexer<LogosToken>) -> logos::Skip {
    // We've already consumed "/*", now find "*/"
    let remainder = lex.remainder();

    if let Some(end) = remainder.find("*/") {
        // Consume everything up to and including "*/"
        lex.bump(end + 2);
    } else {
        // Unterminated comment - consume to end
        lex.bump(remainder.len());
    }

    logos::Skip
}

fn parse_hex(lex: &mut logos::Lexer<LogosToken>) -> Option<i64> {
    let s = lex.slice()[2..].replace('_', "");
    i64::from_str_radix(&s, 16).ok()
}

fn parse_binary(lex: &mut logos::Lexer<LogosToken>) -> Option<i64> {
    let s = lex.slice()[2..].replace('_', "");
    i64::from_str_radix(&s, 2).ok()
}

fn parse_octal(lex: &mut logos::Lexer<LogosToken>) -> Option<i64> {
    let s = lex.slice()[2..].replace('_', "");
    i64::from_str_radix(&s, 8).ok()
}

fn parse_int(lex: &mut logos::Lexer<LogosToken>) -> Option<i64> {
    lex.slice().replace('_', "").parse().ok()
}

fn parse_float(lex: &mut logos::Lexer<LogosToken>) -> Option<f64> {
    lex.slice().replace('_', "").parse().ok()
}

fn parse_string(lex: &mut logos::Lexer<LogosToken>) -> Option<String> {
    let s = lex.slice();
    let inner = &s[1..s.len() - 1]; // Remove quotes
    Some(unescape_string(inner))
}

fn parse_annotation(lex: &mut logos::Lexer<LogosToken>) -> Option<String> {
    let s = lex.slice();
    // Skip "//@@" prefix to get the annotation content (tag + optional value)
    // e.g., "//@@json user_name" -> "json user_name"
    // e.g., "//@@json" -> "json"
    Some(s[4..].trim_end().to_string())
}

fn unescape_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('r') => result.push('\r'),
                Some('t') => result.push('\t'),
                Some('\\') => result.push('\\'),
                Some('"') => result.push('"'),
                Some('\'') => result.push('\''),
                Some('0') => result.push('\0'),
                Some('u') => {
                    // Unicode escape sequences: \uXXXX or \u{XXXXXX}
                    if chars.peek() == Some(&'{') {
                        // Variable-length: \u{XXXXXX}
                        chars.next(); // consume '{'
                        let mut hex = String::new();
                        while let Some(&ch) = chars.peek() {
                            if ch == '}' {
                                chars.next(); // consume '}'
                                break;
                            }
                            if ch.is_ascii_hexdigit() {
                                hex.push(ch);
                                chars.next();
                            } else {
                                break;
                            }
                        }

                        if let Ok(code_point) = u32::from_str_radix(&hex, 16) {
                            if let Some(unicode_char) = char::from_u32(code_point) {
                                result.push(unicode_char);
                            } else {
                                // Invalid code point, keep as-is
                                result.push('\\');
                                result.push('u');
                                result.push('{');
                                result.push_str(&hex);
                                result.push('}');
                            }
                        } else {
                            // Invalid hex, keep as-is
                            result.push('\\');
                            result.push('u');
                            result.push('{');
                            result.push_str(&hex);
                        }
                    } else {
                        // Fixed-length: \uXXXX (4 hex digits)
                        let mut hex = String::new();
                        for _ in 0..4 {
                            if let Some(&ch) = chars.peek() {
                                if ch.is_ascii_hexdigit() {
                                    hex.push(ch);
                                    chars.next();
                                } else {
                                    break;
                                }
                            }
                        }

                        if hex.len() == 4 {
                            if let Ok(code_point) = u16::from_str_radix(&hex, 16) {
                                result.push(char::from_u32(code_point as u32).unwrap());
                            } else {
                                // Invalid hex (shouldn't happen)
                                result.push('\\');
                                result.push('u');
                                result.push_str(&hex);
                            }
                        } else {
                            // Not enough hex digits, keep as-is
                            result.push('\\');
                            result.push('u');
                            result.push_str(&hex);
                        }
                    }
                }
                Some('x') => {
                    // Hex escape sequences: \xXX (2 hex digits)
                    let mut hex = String::new();
                    for _ in 0..2 {
                        if let Some(&ch) = chars.peek() {
                            if ch.is_ascii_hexdigit() {
                                hex.push(ch);
                                chars.next();
                            } else {
                                break;
                            }
                        }
                    }

                    if hex.len() == 2 {
                        if let Ok(code_point) = u8::from_str_radix(&hex, 16) {
                            result.push(code_point as char);
                        } else {
                            result.push('\\');
                            result.push('x');
                            result.push_str(&hex);
                        }
                    } else {
                        // Not enough hex digits
                        result.push('\\');
                        result.push('x');
                        result.push_str(&hex);
                    }
                }
                Some(c) => result.push(c),
                None => break,
            }
        } else {
            result.push(c);
        }
    }

    result
}

/// Main lexer structure.
pub struct Lexer<'a> {
    source: &'a str,
    tokens: Vec<(Token, Span)>,
    errors: Vec<LexError>,
    interner: Interner,
}

/// Lexer error types.
#[derive(Debug, Clone, PartialEq)]
pub enum LexError {
    UnexpectedCharacter { char: char, span: Span },
    UnterminatedString { span: Span },
    UnterminatedTemplate { span: Span },
    InvalidNumber { text: String, span: Span },
    InvalidEscape { escape: String, span: Span },
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            tokens: Vec::new(),
            errors: Vec::new(),
            interner: Interner::with_capacity(256), // Preallocate for typical file
        }
    }

    /// Create a new lexer with an existing interner.
    ///
    /// This is used internally for lexing template literal expressions
    /// where we need to share the interner with the parent lexer.
    fn with_interner(source: &'a str, interner: Interner) -> Self {
        Self {
            source,
            tokens: Vec::new(),
            errors: Vec::new(),
            interner,
        }
    }

    /// Format all errors with source context
    pub fn format_errors(errors: &[LexError], source: &str) -> String {
        errors
            .iter()
            .map(|e| e.format_with_source(source))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn tokenize(mut self) -> Result<(Vec<(Token, Span)>, Interner), Vec<LexError>> {
        let mut pos = 0;
        let mut line = 1u32;
        let mut column = 1u32;

        while pos < self.source.len() {
            // Skip whitespace and comments manually before checking for template literal
            // This is needed because logos skips whitespace internally, but we need to
            // check for backticks BEFORE logos processes them
            let bytes = self.source.as_bytes();
            while pos < bytes.len() {
                let ch = bytes[pos];
                match ch {
                    b' ' | b'\t' | b'\r' => {
                        column += 1;
                        pos += 1;
                    }
                    b'\n' => {
                        line += 1;
                        column = 1;
                        pos += 1;
                    }
                    b'/' if pos + 1 < bytes.len() => {
                        // Check for comments
                        match bytes[pos + 1] {
                            b'/' => {
                                // Check for //@@annotation - don't skip, let logos handle it
                                if pos + 3 < bytes.len()
                                    && bytes[pos + 2] == b'@'
                                    && bytes[pos + 3] == b'@'
                                {
                                    break; // Not a comment, let logos tokenize
                                }
                                // Line comment - skip to end of line
                                pos += 2;
                                column += 2;
                                while pos < bytes.len() && bytes[pos] != b'\n' {
                                    pos += 1;
                                    column += 1;
                                }
                            }
                            b'*' => {
                                // Block comment - skip to */
                                pos += 2;
                                column += 2;
                                while pos + 1 < bytes.len() {
                                    if bytes[pos] == b'*' && bytes[pos + 1] == b'/' {
                                        pos += 2;
                                        column += 2;
                                        break;
                                    }
                                    if bytes[pos] == b'\n' {
                                        line += 1;
                                        column = 1;
                                    } else {
                                        column += 1;
                                    }
                                    pos += 1;
                                }
                            }
                            _ => break, // Not a comment, stop skipping
                        }
                    }
                    _ => break, // Not whitespace, stop skipping
                }
            }

            // Check if we reached the end after skipping whitespace
            if pos >= self.source.len() {
                break;
            }

            // Check for template literal first
            if self.source.as_bytes()[pos] == b'`' {
                let start_span = Span::new(pos, pos + 1, line, column);
                pos += 1; // Skip opening backtick
                column += 1;

                match self.lex_template(pos) {
                    Ok((template, end_pos)) => {
                        self.tokens.push((Token::TemplateLiteral(template), start_span));

                        // Update line/column for consumed template
                        for c in self.source[pos..end_pos].chars() {
                            if c == '\n' {
                                line += 1;
                                column = 1;
                            } else {
                                column += 1;
                            }
                        }
                        pos = end_pos;
                        continue;
                    }
                    Err(err) => {
                        self.errors.push(err);
                        // Skip to end of line or next backtick for error recovery
                        while pos < self.source.len() {
                            let ch = self.source.as_bytes()[pos];
                            if ch == b'\n' || ch == b'`' {
                                break;
                            }
                            if ch == b'\n' {
                                line += 1;
                                column = 1;
                            } else {
                                column += 1;
                            }
                            pos += 1;
                        }
                        continue;
                    }
                }
            }

            // Use logos for regular tokens
            let mut logos_lexer = LogosToken::lexer(&self.source[pos..]);

            if let Some(token_result) = logos_lexer.next() {
                let range = logos_lexer.span();
                let abs_start = pos + range.start;
                let abs_end = pos + range.end;

                let span = Span::new(abs_start, abs_end, line, column);

                match token_result {
                    Ok(logos_token) => {
                        let token = self.convert_token(logos_token);
                        self.tokens.push((token, span));
                    }
                    Err(_) => {
                        let char = self.source[abs_start..].chars().next().unwrap_or('\0');
                        self.errors.push(LexError::UnexpectedCharacter { char, span });
                    }
                }

                // Update line and column
                for c in self.source[abs_start..abs_end].chars() {
                    if c == '\n' {
                        line += 1;
                        column = 1;
                    } else {
                        column += 1;
                    }
                }

                pos = abs_end;
            } else {
                break;
            }
        }

        // Add EOF token
        let eof_span = Span::new(self.source.len(), self.source.len(), line, column);
        self.tokens.push((Token::Eof, eof_span));

        if self.errors.is_empty() {
            Ok((self.tokens, self.interner))
        } else {
            Err(self.errors)
        }
    }

    fn convert_token(&mut self, logos_token: LogosToken) -> Token {
        match logos_token {
            LogosToken::Function => Token::Function,
            LogosToken::Class => Token::Class,
            LogosToken::Type => Token::Type,
            LogosToken::Let => Token::Let,
            LogosToken::Const => Token::Const,
            LogosToken::If => Token::If,
            LogosToken::Else => Token::Else,
            LogosToken::Switch => Token::Switch,
            LogosToken::Case => Token::Case,
            LogosToken::Default => Token::Default,
            LogosToken::For => Token::For,
            LogosToken::While => Token::While,
            LogosToken::Do => Token::Do,
            LogosToken::Break => Token::Break,
            LogosToken::Continue => Token::Continue,
            LogosToken::Return => Token::Return,
            LogosToken::Async => Token::Async,
            LogosToken::Await => Token::Await,
            LogosToken::Try => Token::Try,
            LogosToken::Catch => Token::Catch,
            LogosToken::Finally => Token::Finally,
            LogosToken::Throw => Token::Throw,
            LogosToken::Import => Token::Import,
            LogosToken::Export => Token::Export,
            LogosToken::From => Token::From,
            LogosToken::New => Token::New,
            LogosToken::This => Token::This,
            LogosToken::Super => Token::Super,
            LogosToken::Static => Token::Static,
            LogosToken::Abstract => Token::Abstract,
            LogosToken::Extends => Token::Extends,
            LogosToken::Implements => Token::Implements,
            LogosToken::Typeof => Token::Typeof,
            LogosToken::Instanceof => Token::Instanceof,
            LogosToken::As => Token::As,
            LogosToken::Delete => Token::Delete,
            LogosToken::Void => Token::Void,
            LogosToken::Debugger => Token::Debugger,
            LogosToken::Namespace => Token::Namespace,
            LogosToken::Private => Token::Private,
            LogosToken::Protected => Token::Protected,
            LogosToken::Public => Token::Public,
            LogosToken::Yield => Token::Yield,
            LogosToken::In => Token::In,
            LogosToken::Of => Token::Of,
            LogosToken::True => Token::True,
            LogosToken::False => Token::False,
            LogosToken::Null => Token::Null,
            LogosToken::Identifier(s) => Token::Identifier(self.interner.intern(&s)),
            LogosToken::IntLiteral(n) => Token::IntLiteral(n),
            LogosToken::FloatLiteral(n) => Token::FloatLiteral(n),
            LogosToken::StringLiteral(s) => Token::StringLiteral(self.interner.intern(&s)),
            LogosToken::EqualEqualEqual => Token::EqualEqualEqual,
            LogosToken::BangEqualEqual => Token::BangEqualEqual,
            LogosToken::GreaterGreaterGreater => Token::GreaterGreaterGreater,
            LogosToken::GreaterGreaterGreaterEqual => Token::GreaterGreaterGreaterEqual,
            LogosToken::StarStar => Token::StarStar,
            LogosToken::EqualEqual => Token::EqualEqual,
            LogosToken::BangEqual => Token::BangEqual,
            LogosToken::LessEqual => Token::LessEqual,
            LogosToken::GreaterEqual => Token::GreaterEqual,
            LogosToken::AmpAmp => Token::AmpAmp,
            LogosToken::PipePipe => Token::PipePipe,
            LogosToken::PlusPlus => Token::PlusPlus,
            LogosToken::MinusMinus => Token::MinusMinus,
            LogosToken::LessLess => Token::LessLess,
            LogosToken::GreaterGreater => Token::GreaterGreater,
            LogosToken::LessLessEqual => Token::LessLessEqual,
            LogosToken::GreaterGreaterEqual => Token::GreaterGreaterEqual,
            LogosToken::QuestionDot => Token::QuestionDot,
            LogosToken::QuestionQuestion => Token::QuestionQuestion,
            LogosToken::QuestionQuestionEqual => Token::QuestionQuestionEqual,
            LogosToken::Arrow => Token::Arrow,
            LogosToken::PlusEqual => Token::PlusEqual,
            LogosToken::MinusEqual => Token::MinusEqual,
            LogosToken::StarEqual => Token::StarEqual,
            LogosToken::SlashEqual => Token::SlashEqual,
            LogosToken::PercentEqual => Token::PercentEqual,
            LogosToken::AmpEqual => Token::AmpEqual,
            LogosToken::PipeEqual => Token::PipeEqual,
            LogosToken::CaretEqual => Token::CaretEqual,
            LogosToken::Plus => Token::Plus,
            LogosToken::Minus => Token::Minus,
            LogosToken::Star => Token::Star,
            LogosToken::Slash => Token::Slash,
            LogosToken::Percent => Token::Percent,
            LogosToken::Bang => Token::Bang,
            LogosToken::Tilde => Token::Tilde,
            LogosToken::Less => Token::Less,
            LogosToken::Greater => Token::Greater,
            LogosToken::Amp => Token::Amp,
            LogosToken::Pipe => Token::Pipe,
            LogosToken::Caret => Token::Caret,
            LogosToken::Equal => Token::Equal,
            LogosToken::Question => Token::Question,
            LogosToken::DotDotDot => Token::DotDotDot,
            LogosToken::Dot => Token::Dot,
            LogosToken::Colon => Token::Colon,
            LogosToken::At => Token::At,
            LogosToken::LeftParen => Token::LeftParen,
            LogosToken::RightParen => Token::RightParen,
            LogosToken::LeftBrace => Token::LeftBrace,
            LogosToken::RightBrace => Token::RightBrace,
            LogosToken::LeftBracket => Token::LeftBracket,
            LogosToken::RightBracket => Token::RightBracket,
            LogosToken::Semicolon => Token::Semicolon,
            LogosToken::Comma => Token::Comma,
            LogosToken::Annotation(s) => Token::Annotation(self.interner.intern(&s)),
            LogosToken::Whitespace | LogosToken::LineComment | LogosToken::BlockComment => {
                unreachable!("Whitespace and comments should be skipped")
            }
            LogosToken::Backtick => unreachable!("Backtick handled separately"),
        }
    }

    fn lex_template(&mut self, start: usize) -> Result<(Vec<TemplatePart>, usize), LexError> {
        let mut parts = Vec::new();
        let mut string_part = String::new();
        let bytes = self.source.as_bytes();
        let mut pos = start;

        while pos < bytes.len() {
            let ch = bytes[pos] as char;

            match ch {
                '`' => {
                    // End of template literal
                    if !string_part.is_empty() {
                        let sym = self.interner.intern(&string_part);
                        parts.push(TemplatePart::String(sym));
                    }
                    return Ok((parts, pos + 1));
                }
                '\\' if pos + 1 < bytes.len() => {
                    // Escape sequence
                    pos += 1;
                    match bytes[pos] as char {
                        'n' => {
                            string_part.push('\n');
                            pos += 1;
                        }
                        'r' => {
                            string_part.push('\r');
                            pos += 1;
                        }
                        't' => {
                            string_part.push('\t');
                            pos += 1;
                        }
                        '\\' => {
                            string_part.push('\\');
                            pos += 1;
                        }
                        '`' => {
                            string_part.push('`');
                            pos += 1;
                        }
                        '"' => {
                            string_part.push('"');
                            pos += 1;
                        }
                        '\'' => {
                            string_part.push('\'');
                            pos += 1;
                        }
                        '0' => {
                            string_part.push('\0');
                            pos += 1;
                        }
                        '$' => {
                            string_part.push('$');
                            pos += 1;
                        }
                        'u' => {
                            // Unicode escape sequences
                            pos += 1;
                            if pos < bytes.len() && bytes[pos] as char == '{' {
                                // Variable-length \u{XXXXXX}
                                pos += 1;
                                let mut hex = String::new();
                                while pos < bytes.len() && bytes[pos] as char != '}' {
                                    hex.push(bytes[pos] as char);
                                    pos += 1;
                                }
                                if pos < bytes.len() {
                                    pos += 1; // skip '}'
                                }

                                if let Ok(code_point) = u32::from_str_radix(&hex, 16) {
                                    if let Some(unicode_char) = char::from_u32(code_point) {
                                        string_part.push(unicode_char);
                                    }
                                }
                            } else {
                                // Fixed-length \uXXXX
                                let mut hex = String::new();
                                for _ in 0..4 {
                                    if pos < bytes.len() && (bytes[pos] as char).is_ascii_hexdigit() {
                                        hex.push(bytes[pos] as char);
                                        pos += 1;
                                    } else {
                                        break;
                                    }
                                }

                                if hex.len() == 4 {
                                    if let Ok(code_point) = u16::from_str_radix(&hex, 16) {
                                        string_part.push(char::from_u32(code_point as u32).unwrap());
                                    }
                                }
                            }
                        }
                        'x' => {
                            // Hex escape \xXX
                            pos += 1;
                            let mut hex = String::new();
                            for _ in 0..2 {
                                if pos < bytes.len() && (bytes[pos] as char).is_ascii_hexdigit() {
                                    hex.push(bytes[pos] as char);
                                    pos += 1;
                                } else {
                                    break;
                                }
                            }

                            if hex.len() == 2 {
                                if let Ok(code_point) = u8::from_str_radix(&hex, 16) {
                                    string_part.push(code_point as char);
                                }
                            }
                        }
                        _ => {
                            string_part.push('\\');
                            string_part.push(bytes[pos] as char);
                            pos += 1;
                        }
                    }
                }
                '$' if pos + 1 < bytes.len() && bytes[pos + 1] as char == '{' => {
                    // Expression interpolation
                    if !string_part.is_empty() {
                        let sym = self.interner.intern(&string_part);
                        parts.push(TemplatePart::String(sym));
                        string_part.clear();
                    }

                    // Skip ${
                    pos += 2;
                    let expr_start = pos;
                    let mut brace_depth = 1;

                    // Find matching closing brace
                    while pos < bytes.len() && brace_depth > 0 {
                        match bytes[pos] as char {
                            '{' => brace_depth += 1,
                            '}' => {
                                brace_depth -= 1;
                                if brace_depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        pos += 1;
                    }

                    if brace_depth != 0 {
                        let span = Span::new(expr_start - 2, pos, 0, 0);
                        return Err(LexError::UnterminatedTemplate { span });
                    }

                    // Extract and tokenize the expression
                    // We pass the current interner to the sub-lexer and take back
                    // the updated interner (with any new symbols from the expression)
                    let expr_str = &self.source[expr_start..pos];
                    let current_interner = std::mem::take(&mut self.interner);
                    let expr_lexer = Lexer::with_interner(expr_str, current_interner);
                    match expr_lexer.tokenize() {
                        Ok((tokens, updated_interner)) => {
                            // Put the updated interner back
                            self.interner = updated_interner;
                            // Remove EOF token from the end
                            let tokens_without_eof: Vec<_> = tokens
                                .into_iter()
                                .filter(|(t, _)| !matches!(t, Token::Eof))
                                .collect();
                            parts.push(TemplatePart::Expression(tokens_without_eof));
                        }
                        Err(_) => {
                            let span = Span::new(expr_start - 2, pos, 0, 0);
                            return Err(LexError::UnterminatedTemplate { span });
                        }
                    }

                    pos += 1; // Skip the closing }
                }
                _ => {
                    string_part.push(ch);
                    pos += 1;
                }
            }
        }

        // Reached end without finding closing backtick
        let span = Span::new(start, self.source.len(), 0, 0);
        Err(LexError::UnterminatedTemplate { span })
    }
}

impl LexError {
    /// Get the span of this error
    pub fn span(&self) -> &Span {
        match self {
            LexError::UnexpectedCharacter { span, .. }
            | LexError::UnterminatedString { span }
            | LexError::UnterminatedTemplate { span }
            | LexError::InvalidNumber { span, .. }
            | LexError::InvalidEscape { span, .. } => span,
        }
    }

    /// Get a description of this error
    pub fn description(&self) -> String {
        match self {
            LexError::UnexpectedCharacter { char, .. } => {
                format!("Unexpected character '{}'", char)
            }
            LexError::UnterminatedString { .. } => {
                "Unterminated string literal".to_string()
            }
            LexError::UnterminatedTemplate { .. } => {
                "Unterminated template literal".to_string()
            }
            LexError::InvalidNumber { text, .. } => {
                format!("Invalid number '{}'", text)
            }
            LexError::InvalidEscape { escape, .. } => {
                format!("Invalid escape sequence '{}'", escape)
            }
        }
    }

    /// Get a hint for fixing this error
    pub fn hint(&self) -> Option<String> {
        match self {
            LexError::UnterminatedString { .. } => {
                Some("Add a closing quote to terminate the string".to_string())
            }
            LexError::UnterminatedTemplate { .. } => {
                Some("Add a closing backtick (`) to terminate the template literal".to_string())
            }
            LexError::InvalidEscape { escape, .. } => {
                Some(format!(
                    "Valid escape sequences are: \\n \\r \\t \\\\ \\\" \\' \\0 \\xXX \\uXXXX \\u{{XXXXXX}}, but found '{}'",
                    escape
                ))
            }
            _ => None,
        }
    }

    /// Format the error with source context
    pub fn format_with_source(&self, source: &str) -> String {
        let span = self.span();
        let mut result = String::new();

        // Error header
        result.push_str(&format!(
            "Error at {}:{}: {}\n",
            span.line,
            span.column,
            self.description()
        ));

        // Source context: show the line with the error
        if let Some(error_line) = source.lines().nth((span.line - 1) as usize) {
            result.push_str(&format!("  |\n"));
            result.push_str(&format!("{:3} | {}\n", span.line, error_line));
            result.push_str(&format!("  | {}{}\n", " ".repeat(span.column as usize - 1), "^"));
        }

        // Hint if available
        if let Some(hint) = self.hint() {
            result.push_str(&format!("\nHint: {}\n", hint));
        }

        result
    }
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} at {}:{}",
            self.description(),
            self.span().line,
            self.span().column
        )
    }
}

impl std::error::Error for LexError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_annotation_simple() {
        let source = "//@@json";
        let lexer = Lexer::new(source);
        let (tokens, interner) = lexer.tokenize().expect("should lex");
        assert_eq!(tokens.len(), 2); // Annotation + EOF
        if let Token::Annotation(sym) = &tokens[0].0 {
            assert_eq!(interner.resolve(*sym), "json");
        } else {
            panic!("Expected Annotation token, got {:?}", tokens[0].0);
        }
    }

    #[test]
    fn test_annotation_with_value() {
        let source = "//@@json user_name";
        let lexer = Lexer::new(source);
        let (tokens, interner) = lexer.tokenize().expect("should lex");
        assert_eq!(tokens.len(), 2); // Annotation + EOF
        if let Token::Annotation(sym) = &tokens[0].0 {
            assert_eq!(interner.resolve(*sym), "json user_name");
        } else {
            panic!("Expected Annotation token, got {:?}", tokens[0].0);
        }
    }

    #[test]
    fn test_annotation_with_options() {
        let source = "//@@json age,omitempty";
        let lexer = Lexer::new(source);
        let (tokens, interner) = lexer.tokenize().expect("should lex");
        assert_eq!(tokens.len(), 2); // Annotation + EOF
        if let Token::Annotation(sym) = &tokens[0].0 {
            assert_eq!(interner.resolve(*sym), "json age,omitempty");
        } else {
            panic!("Expected Annotation token, got {:?}", tokens[0].0);
        }
    }

    #[test]
    fn test_annotation_skip() {
        let source = "//@@json -";
        let lexer = Lexer::new(source);
        let (tokens, interner) = lexer.tokenize().expect("should lex");
        assert_eq!(tokens.len(), 2); // Annotation + EOF
        if let Token::Annotation(sym) = &tokens[0].0 {
            assert_eq!(interner.resolve(*sym), "json -");
        } else {
            panic!("Expected Annotation token, got {:?}", tokens[0].0);
        }
    }

    #[test]
    fn test_annotation_in_class() {
        let source = r#"
//@@json
class User {
    //@@json user_name
    name: string;
}
"#;
        let lexer = Lexer::new(source);
        let (tokens, interner) = lexer.tokenize().expect("should lex");

        // Find annotation tokens
        let annotations: Vec<_> = tokens.iter()
            .filter_map(|(t, _)| {
                if let Token::Annotation(sym) = t {
                    Some(interner.resolve(*sym).to_string())
                } else {
                    None
                }
            })
            .collect();

        assert_eq!(annotations.len(), 2);
        assert_eq!(annotations[0], "json");
        assert_eq!(annotations[1], "json user_name");
    }

    #[test]
    fn test_regular_comment_skipped() {
        let source = "// this is a comment\nlet x = 1;";
        let lexer = Lexer::new(source);
        let (tokens, _) = lexer.tokenize().expect("should lex");

        // Should not have any Annotation tokens
        let has_annotation = tokens.iter().any(|(t, _)| matches!(t, Token::Annotation(_)));
        assert!(!has_annotation, "Regular comments should not produce Annotation tokens");

        // Should have the let statement tokens
        assert!(tokens.iter().any(|(t, _)| matches!(t, Token::Let)));
    }

    #[test]
    fn test_single_at_comment_skipped() {
        let source = "//@not an annotation\nlet x = 1;";
        let lexer = Lexer::new(source);
        let (tokens, _) = lexer.tokenize().expect("should lex");

        // Should not have any Annotation tokens (single @ is not an annotation)
        let has_annotation = tokens.iter().any(|(t, _)| matches!(t, Token::Annotation(_)));
        assert!(!has_annotation, "//@... should not produce Annotation tokens");
    }
}
