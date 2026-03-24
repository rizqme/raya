//! Lexer for the Raya programming language.
//!
//! This module implements a high-performance lexer using the logos library.
//! It converts source code into a stream of tokens with precise source location information.

use crate::parser::interner::Interner;
use crate::parser::token::{LexedToken, Span, TemplatePart, Token};
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
    #[regex(r"//[^@\n][^\n]*", logos::skip)] // Skip // followed by non-@
    #[regex(r"//@[^@\n][^\n]*", logos::skip)] // Skip //@ followed by non-@
    #[regex(r"//\n", logos::skip)] // Skip empty line comment
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

    #[token("interface")]
    Interface,

    #[token("let")]
    Let,

    #[token("const")]
    Const,

    #[token("var")]
    Var,

    // Note: 'interface' and 'enum' are BANNED in Raya
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

    #[token("with")]
    With,

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

    #[token("readonly")]
    Readonly,

    #[token("keyof")]
    Keyof,

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

    // Private identifiers (#name) for private class fields/methods
    #[regex(r"#[a-zA-Z_$][a-zA-Z0-9_$]*", |lex| lex.slice()[1..].to_string())]
    PrivateIdentifier(String),

    // Numbers with numeric separator support
    #[regex(r"0x[0-9a-fA-F]+(_[0-9a-fA-F]+)*n", parse_bigint_hex)]
    #[regex(r"0b[01]+(_[01]+)*n", parse_bigint_binary)]
    #[regex(r"0o[0-7]+(_[0-7]+)*n", parse_bigint_octal)]
    #[regex(r"[0-9]+(_[0-9]+)*n", parse_bigint_int)]
    #[regex(r"0x[0-9a-fA-F]+(_[0-9a-fA-F]+)*", parse_hex)]
    #[regex(r"0b[01]+(_[01]+)*", parse_binary)]
    #[regex(r"0o[0-7]+(_[0-7]+)*", parse_octal)]
    #[regex(r"[0-9]+(_[0-9]+)*", parse_int)]
    IntLiteral(i64),

    #[regex(
        r"[0-9]+(_[0-9]+)*\.[0-9]+(_[0-9]+)*([eE][+-]?[0-9]+(_[0-9]+)*)?",
        parse_float
    )]
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

    #[token("||=")]
    PipePipeEqual,

    #[token("&&=")]
    AmpersandAmpersandEqual,

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
fn lex_block_comment(lex: &mut logos::Lexer<'_, LogosToken>) -> logos::Skip {
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

fn parse_hex(lex: &mut logos::Lexer<'_, LogosToken>) -> Option<i64> {
    let s = lex.slice()[2..].replace('_', "");
    i64::from_str_radix(&s, 16).ok()
}

fn parse_bigint_hex(lex: &mut logos::Lexer<'_, LogosToken>) -> Option<i64> {
    let s = lex.slice();
    let s = s[..s.len() - 1][2..].replace('_', "");
    i64::from_str_radix(&s, 16).ok()
}

fn parse_binary(lex: &mut logos::Lexer<'_, LogosToken>) -> Option<i64> {
    let s = lex.slice()[2..].replace('_', "");
    i64::from_str_radix(&s, 2).ok()
}

fn parse_bigint_binary(lex: &mut logos::Lexer<'_, LogosToken>) -> Option<i64> {
    let s = lex.slice();
    let s = s[..s.len() - 1][2..].replace('_', "");
    i64::from_str_radix(&s, 2).ok()
}

fn parse_octal(lex: &mut logos::Lexer<'_, LogosToken>) -> Option<i64> {
    let s = lex.slice()[2..].replace('_', "");
    i64::from_str_radix(&s, 8).ok()
}

fn parse_bigint_octal(lex: &mut logos::Lexer<'_, LogosToken>) -> Option<i64> {
    let s = lex.slice();
    let s = s[..s.len() - 1][2..].replace('_', "");
    i64::from_str_radix(&s, 8).ok()
}

fn parse_int(lex: &mut logos::Lexer<'_, LogosToken>) -> Option<i64> {
    lex.slice().replace('_', "").parse().ok()
}

fn parse_bigint_int(lex: &mut logos::Lexer<'_, LogosToken>) -> Option<i64> {
    let s = lex.slice();
    s[..s.len() - 1].replace('_', "").parse().ok()
}

fn parse_float(lex: &mut logos::Lexer<'_, LogosToken>) -> Option<f64> {
    lex.slice().replace('_', "").parse().ok()
}

fn parse_string(lex: &mut logos::Lexer<'_, LogosToken>) -> Option<String> {
    let s = lex.slice();
    let inner = &s[1..s.len() - 1]; // Remove quotes
    Some(unescape_string(inner))
}

fn parse_annotation(lex: &mut logos::Lexer<'_, LogosToken>) -> Option<String> {
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
                Some('\n') => {}
                Some('\r') => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                }
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
    tokens: Vec<LexedToken>,
    errors: Vec<LexError>,
    interner: Interner,
}

/// Lexer error types.
#[derive(Debug, Clone, PartialEq)]
pub enum LexError {
    UnexpectedCharacter { char: char, span: Span },
    UnterminatedComment { span: Span },
    UnterminatedString { span: Span },
    UnterminatedTemplate { span: Span },
    InvalidNumber { text: String, span: Span },
    InvalidEscape { escape: String, span: Span },
}

fn is_js_whitespace(ch: char) -> bool {
    matches!(
        ch,
        '\u{0009}'
            | '\u{000B}'
            | '\u{000C}'
            | '\u{0020}'
            | '\u{00A0}'
            | '\u{FEFF}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
    )
}

fn line_terminator_width(source: &str, pos: usize) -> Option<(usize, bool)> {
    let ch = source[pos..].chars().next()?;
    match ch {
        '\n' => Some((1, true)),
        '\r' => {
            if source[pos + 1..].starts_with('\n') {
                Some((2, true))
            } else {
                Some((1, true))
            }
        }
        '\u{2028}' | '\u{2029}' => Some((ch.len_utf8(), true)),
        _ => None,
    }
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

    // Compound return carries token stream + interner together; splitting would add indirection.
    #[allow(clippy::type_complexity)]
    pub fn tokenize(mut self) -> Result<(Vec<LexedToken>, Interner), Vec<LexError>> {
        let mut pos = 0;
        let mut line = 1u32;
        let mut column = 1u32;
        let mut line_break_before = false;

        while pos < self.source.len() {
            // Skip whitespace and comments manually before checking for template literal
            // This is needed because logos skips whitespace internally, but we need to
            // check for backticks BEFORE logos processes them
            while pos < self.source.len() {
                if pos == 0 && self.source[pos..].starts_with("#!") {
                    pos += 2;
                    column += 2;
                    while pos < self.source.len() {
                        if let Some((width, _)) = line_terminator_width(self.source, pos) {
                            pos += width;
                            line += 1;
                            column = 1;
                            line_break_before = true;
                            break;
                        }
                        let ch = match self.source[pos..].chars().next() {
                            Some(ch) => ch,
                            None => break,
                        };
                        pos += ch.len_utf8();
                        column += 1;
                    }
                    continue;
                }

                if let Some((width, _)) = line_terminator_width(self.source, pos) {
                    pos += width;
                    line += 1;
                    column = 1;
                    line_break_before = true;
                    continue;
                }

                let ch = match self.source[pos..].chars().next() {
                    Some(ch) => ch,
                    None => break,
                };

                if is_js_whitespace(ch) {
                    pos += ch.len_utf8();
                    column += 1;
                    continue;
                }

                if self.source[pos..].starts_with("//") {
                    // Check for //@@annotation - don't skip, let logos handle it
                    let bytes = self.source.as_bytes();
                    if pos + 3 < bytes.len() && bytes[pos + 2] == b'@' && bytes[pos + 3] == b'@' {
                        break;
                    }
                    pos += 2;
                    column += 2;
                    while pos < self.source.len() {
                        if let Some((_, _)) = line_terminator_width(self.source, pos) {
                            break;
                        }
                        let ch = match self.source[pos..].chars().next() {
                            Some(ch) => ch,
                            None => break,
                        };
                        pos += ch.len_utf8();
                        column += 1;
                    }
                    continue;
                }

                if self.source[pos..].starts_with("/*") {
                    let comment_start = pos;
                    pos += 2;
                    column += 2;
                    let mut terminated = false;
                    while pos < self.source.len() {
                        if self.source[pos..].starts_with("*/") {
                            pos += 2;
                            column += 2;
                            terminated = true;
                            break;
                        }
                        if let Some((width, _)) = line_terminator_width(self.source, pos) {
                            pos += width;
                            line += 1;
                            column = 1;
                            line_break_before = true;
                            continue;
                        }
                        let ch = match self.source[pos..].chars().next() {
                            Some(ch) => ch,
                            None => break,
                        };
                        pos += ch.len_utf8();
                        column += 1;
                    }
                    if !terminated {
                        self.errors.push(LexError::UnterminatedComment {
                            span: Span::new(comment_start, pos, line, column),
                        });
                        pos = self.source.len();
                    }
                    continue;
                }

                break;
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
                        self.tokens.push(LexedToken::new(
                            Token::TemplateLiteral(template),
                            start_span,
                            line_break_before,
                        ));
                        line_break_before = false;

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

            // Check for regex literal: /pattern/flags
            // A `/` starts a regex when NOT preceded by a value-producing token
            // and NOT followed by `/` or `*` (which are comments)
            if self.source.as_bytes()[pos] == b'/'
                && (pos + 1 >= self.source.len()
                    || (self.source.as_bytes()[pos + 1] != b'/'
                        && self.source.as_bytes()[pos + 1] != b'*'))
            {
                // In JSX closing tags (e.g. `</div>`), `/` must remain a slash token.
                // Guard this specific context before regex-literal detection.
                let jsx_closing_tag_slash = self
                    .tokens
                    .last()
                    .map_or(false, |tok| matches!(tok.token, Token::Less))
                    && (pos + 1 < self.source.len()
                        && (self.source.as_bytes()[pos + 1].is_ascii_alphabetic()
                            || self.source.as_bytes()[pos + 1] == b'>'));

                let is_division = self.tokens.last().map_or(false, |tok| {
                    matches!(
                        tok.token,
                        Token::Identifier(_)
                            | Token::IntLiteral(_)
                            | Token::FloatLiteral(_)
                            | Token::StringLiteral(_)
                            | Token::TemplateLiteral(_)
                            | Token::RegexLiteral(_, _)
                            | Token::True
                            | Token::False
                            | Token::Null
                            | Token::This
                            | Token::Super
                            | Token::RightParen
                            | Token::RightBracket
                            | Token::RightBrace
                            | Token::PlusPlus
                            | Token::MinusMinus
                    )
                });

                if !is_division && !jsx_closing_tag_slash {
                    // Scan regex pattern
                    let start = pos;
                    pos += 1; // skip opening /
                    let pattern_start = pos;
                    let mut in_char_class = false;

                    while pos < self.source.len() {
                        let ch = self.source.as_bytes()[pos];
                        if ch == b'\\' && pos + 1 < self.source.len() {
                            pos += 2; // skip escaped char
                        } else if ch == b'[' {
                            in_char_class = true;
                            pos += 1;
                        } else if ch == b']' {
                            in_char_class = false;
                            pos += 1;
                        } else if ch == b'/' && !in_char_class {
                            break;
                        } else if ch == b'\n' {
                            break; // unterminated regex
                        } else {
                            pos += 1;
                        }
                    }

                    if pos < self.source.len() && self.source.as_bytes()[pos] == b'/' {
                        let pattern = &self.source[pattern_start..pos];
                        pos += 1; // skip closing /

                        // Scan flags (gimsuvy)
                        let flags_start = pos;
                        while pos < self.source.len() {
                            let ch = self.source.as_bytes()[pos];
                            if ch.is_ascii_alphabetic() {
                                pos += 1;
                            } else {
                                break;
                            }
                        }
                        let flags = &self.source[flags_start..pos];

                        let pattern_sym = self.interner.intern(pattern);
                        let flags_sym = self.interner.intern(flags);
                        let span = Span::new(start, pos, line, column);
                        self.tokens.push(LexedToken::new(
                            Token::RegexLiteral(pattern_sym, flags_sym),
                            span,
                            line_break_before,
                        ));
                        line_break_before = false;

                        // Update column for the consumed regex
                        column += (pos - start) as u32;
                        continue;
                    }
                    // If we didn't find closing /, reset and fall through to logos
                    pos = start;
                }
            }

            if let Some((next_pos, next_line, next_column)) =
                self.scan_string_literal(pos, line, column, line_break_before)
            {
                pos = next_pos;
                line = next_line;
                column = next_column;
                line_break_before = false;
                continue;
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
                        self.tokens
                            .push(LexedToken::new(token, span, line_break_before));
                        line_break_before = false;
                    }
                    Err(_) => {
                        let char = self.source[abs_start..].chars().next().unwrap_or('\0');
                        self.errors
                            .push(LexError::UnexpectedCharacter { char, span });
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
        self.tokens
            .push(LexedToken::new(Token::Eof, eof_span, line_break_before));

        if self.errors.is_empty() {
            Ok((self.tokens, self.interner))
        } else {
            Err(self.errors)
        }
    }

    fn scan_string_literal(
        &mut self,
        pos: usize,
        line: u32,
        column: u32,
        line_break_before: bool,
    ) -> Option<(usize, u32, u32)> {
        let quote = match self.source.as_bytes().get(pos).copied() {
            Some(b'\'') => '\'',
            Some(b'"') => '"',
            _ => return None,
        };

        let start = pos;
        let mut cursor = pos + 1;
        let mut current_line = line;
        let mut current_column = column + 1;

        while cursor < self.source.len() {
            let ch = self.source[cursor..].chars().next()?;
            let ch_len = ch.len_utf8();

            match ch {
                c if c == quote => {
                    let end = cursor + ch_len;
                    let inner = &self.source[start + 1..cursor];
                    let token =
                        Token::StringLiteral(self.interner.intern(&unescape_string(inner)));
                    let span = Span::new(start, end, line, column);
                    let mut lexed = LexedToken::new(token, span, line_break_before);
                    lexed.raw_string_literal = !inner.contains('\\');
                    self.tokens.push(lexed);
                    return Some((end, current_line, current_column + 1));
                }
                '\\' => {
                    cursor += ch_len;
                    current_column += 1;
                    if cursor >= self.source.len() {
                        let span = Span::new(start, cursor, line, column);
                        self.errors.push(LexError::UnterminatedString { span });
                        return Some((cursor, current_line, current_column));
                    }

                    let next = self.source[cursor..].chars().next()?;
                    let next_len = next.len_utf8();
                    match next {
                        '\n' => {
                            cursor += next_len;
                            current_line += 1;
                            current_column = 1;
                        }
                        '\r' => {
                            cursor += next_len;
                            if cursor < self.source.len()
                                && self.source[cursor..].chars().next() == Some('\n')
                            {
                                cursor += '\n'.len_utf8();
                            }
                            current_line += 1;
                            current_column = 1;
                        }
                        _ => {
                            cursor += next_len;
                            current_column += 1;
                        }
                    }
                }
                '\n' | '\r' => {
                    let span = Span::new(start, cursor, line, column);
                    self.errors.push(LexError::UnterminatedString { span });
                    return Some((cursor, current_line, current_column));
                }
                _ => {
                    cursor += ch_len;
                    current_column += 1;
                }
            }
        }

        let span = Span::new(start, self.source.len(), line, column);
        self.errors.push(LexError::UnterminatedString { span });
        Some((self.source.len(), current_line, current_column))
    }

    fn convert_token(&mut self, logos_token: LogosToken) -> Token {
        match logos_token {
            LogosToken::Function => Token::Function,
            LogosToken::Class => Token::Class,
            LogosToken::Type => Token::Type,
            LogosToken::Interface => Token::Interface,
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
            LogosToken::With => Token::With,
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
            LogosToken::Readonly => Token::Readonly,
            LogosToken::Keyof => Token::Keyof,
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
            LogosToken::PrivateIdentifier(s) => Token::PrivateIdentifier(self.interner.intern(&s)),
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
            LogosToken::PipePipeEqual => Token::PipePipeEqual,
            LogosToken::AmpersandAmpersandEqual => Token::AmpersandAmpersandEqual,
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
                                    if pos < bytes.len() && (bytes[pos] as char).is_ascii_hexdigit()
                                    {
                                        hex.push(bytes[pos] as char);
                                        pos += 1;
                                    } else {
                                        break;
                                    }
                                }

                                if hex.len() == 4 {
                                    if let Ok(code_point) = u16::from_str_radix(&hex, 16) {
                                        string_part
                                            .push(char::from_u32(code_point as u32).unwrap());
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
                                .filter(|token| !matches!(token.token, Token::Eof))
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
            | LexError::UnterminatedComment { span }
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
            LexError::UnterminatedComment { .. } => "Unterminated block comment".to_string(),
            LexError::UnterminatedString { .. } => "Unterminated string literal".to_string(),
            LexError::UnterminatedTemplate { .. } => "Unterminated template literal".to_string(),
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
            LexError::UnterminatedComment { .. } => {
                Some("Add a closing */ to terminate the block comment".to_string())
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
            result.push_str("  |\n");
            result.push_str(&format!("{:3} | {}\n", span.line, error_line));
            result.push_str(&format!(
                "  | {}{}\n",
                " ".repeat(span.column as usize - 1),
                "^"
            ));
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
        if let Token::Annotation(sym) = &tokens[0].token {
            assert_eq!(interner.resolve(*sym), "json");
        } else {
            panic!("Expected Annotation token, got {:?}", tokens[0].token);
        }
    }

    #[test]
    fn test_annotation_with_value() {
        let source = "//@@json user_name";
        let lexer = Lexer::new(source);
        let (tokens, interner) = lexer.tokenize().expect("should lex");
        assert_eq!(tokens.len(), 2); // Annotation + EOF
        if let Token::Annotation(sym) = &tokens[0].token {
            assert_eq!(interner.resolve(*sym), "json user_name");
        } else {
            panic!("Expected Annotation token, got {:?}", tokens[0].token);
        }
    }

    #[test]
    fn test_annotation_with_options() {
        let source = "//@@json age,omitempty";
        let lexer = Lexer::new(source);
        let (tokens, interner) = lexer.tokenize().expect("should lex");
        assert_eq!(tokens.len(), 2); // Annotation + EOF
        if let Token::Annotation(sym) = &tokens[0].token {
            assert_eq!(interner.resolve(*sym), "json age,omitempty");
        } else {
            panic!("Expected Annotation token, got {:?}", tokens[0].token);
        }
    }

    #[test]
    fn test_annotation_skip() {
        let source = "//@@json -";
        let lexer = Lexer::new(source);
        let (tokens, interner) = lexer.tokenize().expect("should lex");
        assert_eq!(tokens.len(), 2); // Annotation + EOF
        if let Token::Annotation(sym) = &tokens[0].token {
            assert_eq!(interner.resolve(*sym), "json -");
        } else {
            panic!("Expected Annotation token, got {:?}", tokens[0].token);
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
        let annotations: Vec<_> = tokens
            .iter()
            .filter_map(|token| {
                if let Token::Annotation(sym) = &token.token {
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
        let has_annotation = tokens
            .iter()
            .any(|token| matches!(token.token, Token::Annotation(_)));
        assert!(
            !has_annotation,
            "Regular comments should not produce Annotation tokens"
        );

        // Should have the let statement tokens
        assert!(tokens.iter().any(|token| matches!(token.token, Token::Let)));
    }

    #[test]
    fn test_single_at_comment_skipped() {
        let source = "//@not an annotation\nlet x = 1;";
        let lexer = Lexer::new(source);
        let (tokens, _) = lexer.tokenize().expect("should lex");

        // Should not have any Annotation tokens (single @ is not an annotation)
        let has_annotation = tokens
            .iter()
            .any(|token| matches!(token.token, Token::Annotation(_)));
        assert!(
            !has_annotation,
            "//@... should not produce Annotation tokens"
        );
    }

    #[test]
    fn test_string_line_continuation_double_quote() {
        let source = "\"a\\\nb\"";
        let lexer = Lexer::new(source);
        let (tokens, interner) = lexer.tokenize().expect("should lex");
        match &tokens[0].token {
            Token::StringLiteral(sym) => assert_eq!(interner.resolve(*sym), "ab"),
            other => panic!("expected string literal, got {other:?}"),
        }
    }

    #[test]
    fn test_string_line_continuation_single_quote() {
        let source = "'a\\\r\nb'";
        let lexer = Lexer::new(source);
        let (tokens, interner) = lexer.tokenize().expect("should lex");
        match &tokens[0].token {
            Token::StringLiteral(sym) => assert_eq!(interner.resolve(*sym), "ab"),
            other => panic!("expected string literal, got {other:?}"),
        }
    }
}
