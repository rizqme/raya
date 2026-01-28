//! Token definitions for the Raya programming language.
//!
//! This module defines all tokens that can appear in Raya source code,
//! including keywords, operators, literals, and special tokens.

use std::fmt;
use crate::parser::interner::Symbol;

/// A token in the Raya programming language.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Keywords (41 total)
    // Core keywords
    Function,
    Class,
    Type,
    Let,
    Const,
    // Note: 'interface' and 'enum' are BANNED in Raya (LANG.md §10, §19.2)
    // Note: 'var' is BANNED in Raya (LANG.md §19.1)

    // Control flow
    If,
    Else,
    Switch,
    Case,
    Default,
    For,
    While,
    Do,
    Break,
    Continue,
    Return,

    // Async/Error handling
    Async,
    Await,
    Try,
    Catch,
    Finally,
    Throw,

    // Modules
    Import,
    Export,
    From,

    // OOP keywords
    New,
    This,
    Super,
    Static,
    Abstract,
    Extends,
    Implements,

    // Type operators
    Typeof,
    Instanceof,
    As,
    Delete,
    Void,

    // Utility/Debug
    Debugger,

    // Future reserved (for compatibility)
    Namespace,
    Private,
    Protected,
    Public,
    Yield,
    In,
    Of,

    // Literals
    IntLiteral(i64),
    FloatLiteral(f64),
    StringLiteral(Symbol),  // Interned string
    TemplateLiteral(Vec<TemplatePart>),
    True,
    False,
    Null,

    // Identifiers
    Identifier(Symbol),  // Interned identifier

    // Operators
    // Arithmetic
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    StarStar, // Exponentiation **

    // Unary
    PlusPlus,
    MinusMinus,
    Bang,
    Tilde,

    // Comparison
    EqualEqual,
    BangEqual,
    EqualEqualEqual,
    BangEqualEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,

    // Logical
    AmpAmp,
    PipePipe,

    // Bitwise
    Amp,
    Pipe,
    Caret,
    LessLess,
    GreaterGreater,
    GreaterGreaterGreater,

    // Assignment
    Equal,
    PlusEqual,
    MinusEqual,
    StarEqual,
    SlashEqual,
    PercentEqual,
    AmpEqual,
    PipeEqual,
    CaretEqual,
    LessLessEqual,
    GreaterGreaterEqual,
    GreaterGreaterGreaterEqual,

    // Other
    Question,
    QuestionQuestion,
    QuestionDot,
    DotDotDot, // ... for spread/rest
    Dot,
    Colon,
    Arrow,
    At, // @ for decorators

    // Delimiters
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    Semicolon,
    Comma,

    // Special
    Eof,
    Error(String),
}

/// A part of a template literal.
#[derive(Debug, Clone, PartialEq)]
pub enum TemplatePart {
    String(Symbol),  // Interned string part
    Expression(Vec<(Token, Span)>),
}

/// Source location information for a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: u32,
    pub column: u32,
}

impl Span {
    pub fn new(start: usize, end: usize, line: u32, column: u32) -> Self {
        Self {
            start,
            end,
            line,
            column,
        }
    }

    pub fn len(&self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    pub fn slice<'a>(&self, source: &'a str) -> &'a str {
        &source[self.start..self.end]
    }

    pub fn merge(&self, other: &Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
            line: self.line.min(other.line),
            column: self.column.min(other.column),
        }
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Function => write!(f, "function"),
            Token::Class => write!(f, "class"),
            Token::Type => write!(f, "type"),
            Token::Let => write!(f, "let"),
            Token::Const => write!(f, "const"),
            Token::If => write!(f, "if"),
            Token::Else => write!(f, "else"),
            Token::Switch => write!(f, "switch"),
            Token::Case => write!(f, "case"),
            Token::Default => write!(f, "default"),
            Token::For => write!(f, "for"),
            Token::While => write!(f, "while"),
            Token::Do => write!(f, "do"),
            Token::Break => write!(f, "break"),
            Token::Continue => write!(f, "continue"),
            Token::Return => write!(f, "return"),
            Token::Async => write!(f, "async"),
            Token::Await => write!(f, "await"),
            Token::Try => write!(f, "try"),
            Token::Catch => write!(f, "catch"),
            Token::Finally => write!(f, "finally"),
            Token::Throw => write!(f, "throw"),
            Token::Import => write!(f, "import"),
            Token::Export => write!(f, "export"),
            Token::From => write!(f, "from"),
            Token::New => write!(f, "new"),
            Token::This => write!(f, "this"),
            Token::Super => write!(f, "super"),
            Token::Static => write!(f, "static"),
            Token::Abstract => write!(f, "abstract"),
            Token::Extends => write!(f, "extends"),
            Token::Implements => write!(f, "implements"),
            Token::Typeof => write!(f, "typeof"),
            Token::Instanceof => write!(f, "instanceof"),
            Token::As => write!(f, "as"),
            Token::Delete => write!(f, "delete"),
            Token::Void => write!(f, "void"),
            Token::Debugger => write!(f, "debugger"),
            Token::Namespace => write!(f, "namespace"),
            Token::Private => write!(f, "private"),
            Token::Protected => write!(f, "protected"),
            Token::Public => write!(f, "public"),
            Token::Yield => write!(f, "yield"),
            Token::In => write!(f, "in"),
            Token::Of => write!(f, "of"),
            Token::IntLiteral(n) => write!(f, "{}", n),
            Token::FloatLiteral(n) => write!(f, "{}", n),
            Token::StringLiteral(_) => write!(f, "\"<string>\""),
            Token::TemplateLiteral(_) => write!(f, "`...`"),
            Token::True => write!(f, "true"),
            Token::False => write!(f, "false"),
            Token::Null => write!(f, "null"),
            Token::Identifier(_) => write!(f, "<identifier>"),
            Token::Plus => write!(f, "+"),
            Token::Minus => write!(f, "-"),
            Token::Star => write!(f, "*"),
            Token::Slash => write!(f, "/"),
            Token::Percent => write!(f, "%"),
            Token::StarStar => write!(f, "**"),
            Token::PlusPlus => write!(f, "++"),
            Token::MinusMinus => write!(f, "--"),
            Token::Bang => write!(f, "!"),
            Token::Tilde => write!(f, "~"),
            Token::EqualEqual => write!(f, "=="),
            Token::BangEqual => write!(f, "!="),
            Token::EqualEqualEqual => write!(f, "==="),
            Token::BangEqualEqual => write!(f, "!=="),
            Token::Less => write!(f, "<"),
            Token::LessEqual => write!(f, "<="),
            Token::Greater => write!(f, ">"),
            Token::GreaterEqual => write!(f, ">="),
            Token::AmpAmp => write!(f, "&&"),
            Token::PipePipe => write!(f, "||"),
            Token::Amp => write!(f, "&"),
            Token::Pipe => write!(f, "|"),
            Token::Caret => write!(f, "^"),
            Token::LessLess => write!(f, "<<"),
            Token::GreaterGreater => write!(f, ">>"),
            Token::GreaterGreaterGreater => write!(f, ">>>"),
            Token::Equal => write!(f, "="),
            Token::PlusEqual => write!(f, "+="),
            Token::MinusEqual => write!(f, "-="),
            Token::StarEqual => write!(f, "*="),
            Token::SlashEqual => write!(f, "/="),
            Token::PercentEqual => write!(f, "%="),
            Token::AmpEqual => write!(f, "&="),
            Token::PipeEqual => write!(f, "|="),
            Token::CaretEqual => write!(f, "^="),
            Token::LessLessEqual => write!(f, "<<="),
            Token::GreaterGreaterEqual => write!(f, ">>="),
            Token::GreaterGreaterGreaterEqual => write!(f, ">>>="),
            Token::Question => write!(f, "?"),
            Token::QuestionQuestion => write!(f, "??"),
            Token::QuestionDot => write!(f, "?."),
            Token::DotDotDot => write!(f, "..."),
            Token::Dot => write!(f, "."),
            Token::Colon => write!(f, ":"),
            Token::Arrow => write!(f, "=>"),
            Token::At => write!(f, "@"),
            Token::LeftParen => write!(f, "("),
            Token::RightParen => write!(f, ")"),
            Token::LeftBrace => write!(f, "{{"),
            Token::RightBrace => write!(f, "}}"),
            Token::LeftBracket => write!(f, "["),
            Token::RightBracket => write!(f, "]"),
            Token::Semicolon => write!(f, ";"),
            Token::Comma => write!(f, ","),
            Token::Eof => write!(f, "EOF"),
            Token::Error(msg) => write!(f, "ERROR: {}", msg),
        }
    }
}

impl Token {
    /// Returns true if this token is a keyword.
    pub fn is_keyword(&self) -> bool {
        matches!(
            self,
            Token::Function
                | Token::Class
                | Token::Type
                | Token::Let
                | Token::Const
                | Token::If
                | Token::Else
                | Token::Switch
                | Token::Case
                | Token::Default
                | Token::For
                | Token::While
                | Token::Do
                | Token::Break
                | Token::Continue
                | Token::Return
                | Token::Async
                | Token::Await
                | Token::Try
                | Token::Catch
                | Token::Finally
                | Token::Throw
                | Token::Import
                | Token::Export
                | Token::From
                | Token::New
                | Token::This
                | Token::Super
                | Token::Static
                | Token::Abstract
                | Token::Extends
                | Token::Implements
                | Token::Typeof
                | Token::Instanceof
                | Token::As
                | Token::Delete
                | Token::Void
                | Token::Debugger
                | Token::Namespace
                | Token::Private
                | Token::Protected
                | Token::Public
                | Token::Yield
                | Token::In
                | Token::Of
        )
    }

    /// Returns true if this token is a literal.
    pub fn is_literal(&self) -> bool {
        matches!(
            self,
            Token::IntLiteral(_)
                | Token::FloatLiteral(_)
                | Token::StringLiteral(_)
                | Token::TemplateLiteral(_)
                | Token::True
                | Token::False
                | Token::Null
        )
    }

    /// Returns true if this token is an operator.
    pub fn is_operator(&self) -> bool {
        matches!(
            self,
            Token::Plus
                | Token::Minus
                | Token::Star
                | Token::Slash
                | Token::Percent
                | Token::StarStar
                | Token::PlusPlus
                | Token::MinusMinus
                | Token::Bang
                | Token::Tilde
                | Token::EqualEqual
                | Token::BangEqual
                | Token::EqualEqualEqual
                | Token::BangEqualEqual
                | Token::Less
                | Token::LessEqual
                | Token::Greater
                | Token::GreaterEqual
                | Token::AmpAmp
                | Token::PipePipe
                | Token::Amp
                | Token::Pipe
                | Token::Caret
                | Token::LessLess
                | Token::GreaterGreater
                | Token::GreaterGreaterGreater
                | Token::Equal
                | Token::PlusEqual
                | Token::MinusEqual
                | Token::StarEqual
                | Token::SlashEqual
                | Token::PercentEqual
                | Token::AmpEqual
                | Token::PipeEqual
                | Token::CaretEqual
                | Token::LessLessEqual
                | Token::GreaterGreaterEqual
                | Token::GreaterGreaterGreaterEqual
                | Token::Question
                | Token::QuestionQuestion
                | Token::QuestionDot
                | Token::Dot
                | Token::Arrow
        )
    }
}
