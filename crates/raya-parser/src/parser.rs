//! Parser for Raya language
//!
//! This module implements a recursive descent parser that transforms
//! a token stream from the lexer into an Abstract Syntax Tree (AST).

pub mod error;
pub mod expr;
pub mod pattern;
pub mod precedence;
pub mod recovery;
pub mod stmt;
pub mod types;

use crate::ast::*;
use crate::lexer::Lexer;
use crate::token::{Span, Token};

pub use error::{ParseError, ParseErrorKind};

/// Parser state for the Raya programming language.
///
/// This implements a recursive descent parser with 2-token lookahead (LL(2))
/// for handling ambiguous constructs like arrow functions vs parenthesized expressions.
pub struct Parser {
    /// Pre-tokenized input
    tokens: Vec<(Token, Span)>,

    /// Current position in token stream
    pos: usize,

    /// Accumulated parse errors (allows continuing after errors)
    errors: Vec<ParseError>,
}

impl Parser {
    /// Create a new parser from source code.
    pub fn new(source: &str) -> Result<Self, Vec<crate::lexer::LexError>> {
        // Tokenize the entire input first
        let lexer = Lexer::new(source);
        let mut tokens = lexer.tokenize()?;

        // Add EOF token if not present
        if tokens.is_empty() || !matches!(tokens.last().unwrap().0, Token::Eof) {
            let eof_span = if let Some((_, last_span)) = tokens.last() {
                Span::new(last_span.end, last_span.end, last_span.line, last_span.column)
            } else {
                Span::new(0, 0, 1, 1)
            };
            tokens.push((Token::Eof, eof_span));
        }

        Ok(Self {
            tokens,
            pos: 0,
            errors: Vec::new(),
        })
    }

    /// Parse the entire source file into a Module AST.
    ///
    /// Returns the Module on success, or all accumulated errors on failure.
    pub fn parse(mut self) -> Result<Module, Vec<ParseError>> {
        let start_span = self.current_span();
        let mut statements = Vec::new();

        // Parse top-level statements until EOF
        while !self.at_eof() {
            match self.parse_statement() {
                Ok(stmt) => statements.push(stmt),
                Err(err) => {
                    self.errors.push(err);
                    // Attempt recovery by synchronizing to next statement
                    self.sync_to_statement_boundary();
                }
            }
        }

        let span = if let Some(last) = statements.last() {
            self.combine_spans(&start_span, last.span())
        } else {
            start_span
        };

        // If any errors occurred, return them
        if !self.errors.is_empty() {
            return Err(self.errors);
        }

        Ok(Module { statements, span })
    }

    // ========================================================================
    // Token Management
    // ========================================================================

    /// Get the current token.
    #[inline]
    pub fn current(&self) -> &Token {
        &self.tokens[self.pos].0
    }

    /// Get the current token's span.
    #[inline]
    pub fn current_span(&self) -> Span {
        self.tokens[self.pos].1
    }

    /// Peek at the next token (lookahead).
    #[inline]
    pub fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos + 1).map(|(tok, _)| tok)
    }

    /// Peek at the next token's span.
    #[inline]
    pub fn peek_span(&self) -> Option<Span> {
        self.tokens.get(self.pos + 1).map(|(_, span)| *span)
    }

    /// Advance to the next token, returning the previous current token.
    pub fn advance(&mut self) -> Token {
        let tok = self.tokens[self.pos].0.clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        tok
    }

    /// Check if the current token matches the given kind.
    #[inline]
    pub fn check(&self, expected: &Token) -> bool {
        std::mem::discriminant(self.current()) == std::mem::discriminant(expected)
    }

    /// Check if the current token matches any of the given kinds.
    pub fn check_any(&self, expected: &[Token]) -> bool {
        expected.iter().any(|tok| self.check(tok))
    }

    /// Check if we've reached EOF.
    #[inline]
    pub fn at_eof(&self) -> bool {
        matches!(self.current(), Token::Eof)
    }

    /// Consume the current token if it matches the expected kind.
    ///
    /// Returns Ok(token) on match, or Err(ParseError) on mismatch.
    pub fn expect(&mut self, expected: Token) -> Result<Token, ParseError> {
        if self.check(&expected) {
            Ok(self.advance())
        } else {
            Err(self.unexpected_token(&[expected]))
        }
    }

    /// Consume the current token if it matches any of the expected kinds.
    pub fn expect_any(&mut self, expected: &[Token]) -> Result<Token, ParseError> {
        if self.check_any(expected) {
            Ok(self.advance())
        } else {
            Err(self.unexpected_token(expected))
        }
    }

    // ========================================================================
    // Error Handling
    // ========================================================================

    /// Record a parse error.
    pub fn error(&mut self, kind: ParseErrorKind, span: Span) {
        self.errors.push(ParseError {
            kind,
            span,
            message: String::new(), // Will be formatted later
            suggestion: None,
        });
    }

    /// Create an "unexpected token" error.
    fn unexpected_token(&self, expected: &[Token]) -> ParseError {
        let span = self.current_span();
        if self.at_eof() {
            ParseError {
                kind: ParseErrorKind::UnexpectedEof {
                    expected: expected.to_vec(),
                },
                span,
                message: format!("Unexpected end of file, expected one of: {:?}", expected),
                suggestion: None,
            }
        } else {
            ParseError {
                kind: ParseErrorKind::UnexpectedToken {
                    expected: expected.to_vec(),
                    found: self.current().clone(),
                },
                span,
                message: format!(
                    "Unexpected token {:?}, expected one of: {:?}",
                    self.current(),
                    expected
                ),
                suggestion: None,
            }
        }
    }

    // ========================================================================
    // Utilities
    // ========================================================================

    /// Combine two spans into a single span.
    pub fn combine_spans(&self, start: &Span, end: &Span) -> Span {
        Span {
            start: start.start,
            end: end.end,
            line: start.line,
            column: start.column,
        }
    }

    // ========================================================================
    // Placeholder parsing methods (to be implemented in submodules)
    // ========================================================================

    fn parse_statement(&mut self) -> Result<Statement, ParseError> {
        stmt::parse_statement(self)
    }

    /// Synchronize to the next statement boundary after an error.
    fn sync_to_statement_boundary(&mut self) {
        recovery::sync_to_statement_boundary(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser_new() {
        let source = "let x = 42;";
        let parser = Parser::new(source).unwrap();

        assert!(matches!(parser.current(), Token::Let));
    }

    #[test]
    fn test_parser_advance() {
        let source = "let x";
        let mut parser = Parser::new(source).unwrap();

        assert!(matches!(parser.current(), Token::Let));
        let tok = parser.advance();
        assert!(matches!(tok, Token::Let));
        assert!(matches!(parser.current(), Token::Identifier(_)));
    }

    #[test]
    fn test_parser_at_eof() {
        let source = "";
        let parser = Parser::new(source).unwrap();

        assert!(parser.at_eof());
    }

    #[test]
    fn test_parser_check() {
        let source = "let x";
        let parser = Parser::new(source).unwrap();

        assert!(parser.check(&Token::Let));
        assert!(!parser.check(&Token::Const));
    }

    #[test]
    fn test_parser_peek() {
        let source = "let x";
        let parser = Parser::new(source).unwrap();

        assert!(matches!(parser.current(), Token::Let));
        assert!(matches!(parser.peek(), Some(Token::Identifier(_))));
    }
}
