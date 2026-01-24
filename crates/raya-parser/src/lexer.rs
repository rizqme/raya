//! Lexer for the Raya programming language.
//!
//! This module implements a high-performance lexer using the logos library.
//! It converts source code into a stream of tokens with precise source location information.

use crate::token::{Span, TemplatePart, Token};
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

    // Comments (skip)
    #[regex(r"//[^\n]*", logos::skip)]
    LineComment,

    #[regex(r"/\*", lex_block_comment)]
    BlockComment,

    // Keywords (must come before identifiers)
    #[token("function")]
    Function,

    #[token("class")]
    Class,

    #[token("interface")]
    Interface,

    #[token("type")]
    Type,

    #[token("enum")]
    Enum,

    #[token("let")]
    Let,

    #[token("const")]
    Const,

    #[token("var")]
    Var,

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

    #[token("extends")]
    Extends,

    #[token("implements")]
    Implements,

    #[token("typeof")]
    Typeof,

    #[token("instanceof")]
    Instanceof,

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

    #[token(".")]
    Dot,

    #[token(":")]
    Colon,

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

fn unescape_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();

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
                    // Unicode escape sequences handled in Phase 4
                    result.push('u');
                }
                Some('x') => {
                    // Hex escape sequences
                    result.push('x');
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
        }
    }

    pub fn tokenize(mut self) -> Result<Vec<(Token, Span)>, Vec<LexError>> {
        let mut logos_lexer = LogosToken::lexer(self.source);
        let mut line = 1u32;
        let mut column = 1u32;
        let mut last_end = 0;

        while let Some(token_result) = logos_lexer.next() {
            let range = logos_lexer.span();
            
            // Update line and column based on consumed text
            for c in self.source[last_end..range.start].chars() {
                if c == '\n' {
                    line += 1;
                    column = 1;
                } else {
                    column += 1;
                }
            }

            let span = Span::new(range.start, range.end, line, column);

            match token_result {
                Ok(logos_token) => {
                    // Handle template literals specially
                    if matches!(logos_token, LogosToken::Backtick) {
                        match self.lex_template(range.end) {
                            Ok((template, end_pos)) => {
                                self.tokens.push((Token::TemplateLiteral(template), span));
                                // Update position
                                last_end = end_pos;
                                continue;
                            }
                            Err(err) => {
                                self.errors.push(err);
                                last_end = range.end;
                                continue;
                            }
                        }
                    }

                    let token = self.convert_token(logos_token);
                    self.tokens.push((token, span));
                }
                Err(_) => {
                    let char = self.source[range.start..].chars().next().unwrap_or('\0');
                    self.errors.push(LexError::UnexpectedCharacter { char, span });
                }
            }

            // Update column for this token
            for c in self.source[range.start..range.end].chars() {
                if c == '\n' {
                    line += 1;
                    column = 1;
                } else {
                    column += 1;
                }
            }

            last_end = range.end;
        }

        // Add EOF token
        let eof_span = Span::new(self.source.len(), self.source.len(), line, column);
        self.tokens.push((Token::Eof, eof_span));

        if self.errors.is_empty() {
            Ok(self.tokens)
        } else {
            Err(self.errors)
        }
    }

    fn convert_token(&self, logos_token: LogosToken) -> Token {
        match logos_token {
            LogosToken::Function => Token::Function,
            LogosToken::Class => Token::Class,
            LogosToken::Interface => Token::Interface,
            LogosToken::Type => Token::Type,
            LogosToken::Enum => Token::Enum,
            LogosToken::Let => Token::Let,
            LogosToken::Const => Token::Const,
            LogosToken::Var => Token::Var,
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
            LogosToken::Extends => Token::Extends,
            LogosToken::Implements => Token::Implements,
            LogosToken::Typeof => Token::Typeof,
            LogosToken::Instanceof => Token::Instanceof,
            LogosToken::Void => Token::Void,
            LogosToken::Debugger => Token::Debugger,
            LogosToken::Namespace => Token::Namespace,
            LogosToken::Private => Token::Private,
            LogosToken::Protected => Token::Protected,
            LogosToken::Public => Token::Public,
            LogosToken::Yield => Token::Yield,
            LogosToken::In => Token::In,
            LogosToken::True => Token::True,
            LogosToken::False => Token::False,
            LogosToken::Null => Token::Null,
            LogosToken::Identifier(s) => Token::Identifier(s),
            LogosToken::IntLiteral(n) => Token::IntLiteral(n),
            LogosToken::FloatLiteral(n) => Token::FloatLiteral(n),
            LogosToken::StringLiteral(s) => Token::StringLiteral(s),
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
            LogosToken::Dot => Token::Dot,
            LogosToken::Colon => Token::Colon,
            LogosToken::LeftParen => Token::LeftParen,
            LogosToken::RightParen => Token::RightParen,
            LogosToken::LeftBrace => Token::LeftBrace,
            LogosToken::RightBrace => Token::RightBrace,
            LogosToken::LeftBracket => Token::LeftBracket,
            LogosToken::RightBracket => Token::RightBracket,
            LogosToken::Semicolon => Token::Semicolon,
            LogosToken::Comma => Token::Comma,
            LogosToken::Whitespace | LogosToken::LineComment | LogosToken::BlockComment => {
                unreachable!("Whitespace and comments should be skipped")
            }
            LogosToken::Backtick => unreachable!("Backtick handled separately"),
        }
    }

    fn lex_template(&self, start: usize) -> Result<(Vec<TemplatePart>, usize), LexError> {
        // Template literal lexing will be implemented in Phase 3
        // For now, return a simple placeholder
        Ok((vec![], start))
    }
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LexError::UnexpectedCharacter { char, span } => {
                write!(f, "Unexpected character '{}' at {}:{}", char, span.line, span.column)
            }
            LexError::UnterminatedString { span } => {
                write!(f, "Unterminated string at {}:{}", span.line, span.column)
            }
            LexError::UnterminatedTemplate { span } => {
                write!(f, "Unterminated template literal at {}:{}", span.line, span.column)
            }
            LexError::InvalidNumber { text, span } => {
                write!(f, "Invalid number '{}' at {}:{}", text, span.line, span.column)
            }
            LexError::InvalidEscape { escape, span } => {
                write!(f, "Invalid escape sequence '{}' at {}:{}", escape, span.line, span.column)
            }
        }
    }
}

impl std::error::Error for LexError {}
