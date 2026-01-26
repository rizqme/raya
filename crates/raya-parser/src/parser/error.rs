//! Parse error types and error reporting

use crate::token::{Span, Token};
use std::fmt;

/// A parse error with location and contextual information.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    /// The kind of error that occurred
    pub kind: ParseErrorKind,

    /// Source location of the error
    pub span: Span,

    /// Human-readable error message
    pub message: String,

    /// Optional suggestion for fixing the error
    pub suggestion: Option<String>,
}

/// The kind of parse error.
#[derive(Debug, Clone, PartialEq)]
pub enum ParseErrorKind {
    /// Unexpected token found
    UnexpectedToken {
        expected: Vec<Token>,
        found: Token,
    },

    /// Unexpected end of file
    UnexpectedEof {
        expected: Vec<Token>,
    },

    /// Invalid syntax
    InvalidSyntax {
        reason: String,
    },

    /// Duplicate declaration
    DuplicateDeclaration {
        name: String,
    },

    /// Invalid number literal
    InvalidNumber {
        value: String,
    },

    /// Invalid string literal
    InvalidString {
        reason: String,
    },

    /// Missing semicolon
    MissingSemicolon,

    /// Missing closing delimiter
    UnclosedDelimiter {
        open: Token,
        expected_close: Token,
    },

    /// Banned feature used
    BannedFeature {
        feature: String,
        reason: String,
    },

    /// Context-dependent operator used incorrectly
    InvalidOperatorContext {
        operator: String,
        context: String,
    },

    /// Parser exceeded iteration/depth/size limit
    ParserLimitExceeded {
        message: String,
    },

    /// Parser got stuck (position didn't advance)
    ParserStuck {
        message: String,
    },

    /// Error was recovered, parsing continued
    Recovered,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Parse error at {}:{}: {}",
            self.span.line, self.span.column, self.message
        )?;

        if let Some(suggestion) = &self.suggestion {
            write!(f, "\n  Suggestion: {}", suggestion)?;
        }

        Ok(())
    }
}

impl std::error::Error for ParseError {}

impl ParseError {
    /// Create an "unexpected token" error.
    pub fn unexpected_token(expected: Vec<Token>, found: Token, span: Span) -> Self {
        let message = if expected.len() == 1 {
            format!("Expected {:?}, found {:?}", expected[0], found)
        } else {
            format!("Expected one of {:?}, found {:?}", expected, found)
        };

        Self {
            kind: ParseErrorKind::UnexpectedToken { expected, found },
            span,
            message,
            suggestion: None,
        }
    }

    /// Create an "unexpected EOF" error.
    pub fn unexpected_eof(expected: Vec<Token>, span: Span) -> Self {
        let message = if expected.len() == 1 {
            format!("Unexpected end of file, expected {:?}", expected[0])
        } else {
            format!("Unexpected end of file, expected one of {:?}", expected)
        };

        Self {
            kind: ParseErrorKind::UnexpectedEof { expected },
            span,
            message,
            suggestion: None,
        }
    }

    /// Create an "invalid syntax" error.
    pub fn invalid_syntax(reason: impl Into<String>, span: Span) -> Self {
        let reason = reason.into();
        Self {
            kind: ParseErrorKind::InvalidSyntax {
                reason: reason.clone(),
            },
            span,
            message: format!("Invalid syntax: {}", reason),
            suggestion: None,
        }
    }

    /// Create a "banned feature" error.
    pub fn banned_feature(feature: impl Into<String>, reason: impl Into<String>, span: Span) -> Self {
        let feature = feature.into();
        let reason = reason.into();

        Self {
            kind: ParseErrorKind::BannedFeature {
                feature: feature.clone(),
                reason: reason.clone(),
            },
            span,
            message: format!("Feature '{}' is banned in Raya: {}", feature, reason),
            suggestion: None,
        }
    }

    /// Add a suggestion to this error.
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }

    /// Create a "parser limit exceeded" error.
    pub fn parser_limit_exceeded(message: impl Into<String>, span: Span) -> Self {
        let message = message.into();
        Self {
            kind: ParseErrorKind::ParserLimitExceeded {
                message: message.clone(),
            },
            span,
            message: format!("Parser limit exceeded: {}", message),
            suggestion: None,
        }
    }

    /// Create a "parser stuck" error.
    pub fn parser_stuck(message: impl Into<String>, span: Span) -> Self {
        let message = message.into();
        Self {
            kind: ParseErrorKind::ParserStuck {
                message: message.clone(),
            },
            span,
            message: format!("Parser stuck: {}", message),
            suggestion: None,
        }
    }

    /// Create a "recovered" marker error.
    pub fn recovered() -> Self {
        Self {
            kind: ParseErrorKind::Recovered,
            span: Span::new(0, 0, 0, 0),
            message: "Error recovered".to_string(),
            suggestion: None,
        }
    }
}
