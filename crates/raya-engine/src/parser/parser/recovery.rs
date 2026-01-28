//! Error recovery strategies for the parser.
//!
//! When the parser encounters an error, it uses these strategies to
//! resynchronize and continue parsing to find more errors.

use super::guards::LoopGuard;
use super::Parser;
use crate::parser::token::Token;

/// Synchronize to the next statement boundary.
///
/// This is used after encountering a parse error to skip tokens until
/// we reach a point where statement parsing can resume.
pub fn sync_to_statement_boundary(parser: &mut Parser) {
    // Loop guard to prevent infinite loops in recovery itself
    let mut guard = LoopGuard::new("statement_recovery");

    while !parser.at_eof() {
        // Emergency stop if recovery loops too long
        if guard.check().is_err() {
            return;
        }

        match parser.current() {
            // Statement-starting tokens
            Token::Function
            | Token::Class
            | Token::Type
            | Token::Let
            | Token::Const
            | Token::If
            | Token::While
            | Token::For
            | Token::Switch
            | Token::Try
            | Token::Return
            | Token::Break
            | Token::Continue
            | Token::Throw
            | Token::Import
            | Token::Export => {
                // Found a statement start, stop skipping
                return;
            }

            // Semicolon marks end of previous statement
            Token::Semicolon => {
                parser.advance();
                return;
            }

            // Closing brace might end a block - advance past it to avoid infinite loop
            Token::RightBrace => {
                parser.advance();
                return;
            }

            // Keep skipping
            _ => {
                parser.advance();
            }
        }
    }
}

/// Synchronize to the next expression boundary.
pub fn sync_to_expression_boundary(parser: &mut Parser) {
    // Loop guard to prevent infinite loops in recovery itself
    let mut guard = LoopGuard::new("expression_recovery");

    while !parser.at_eof() {
        // Emergency stop if recovery loops too long
        if guard.check().is_err() {
            return;
        }

        match parser.current() {
            // Expression delimiters
            Token::Semicolon
            | Token::Comma
            | Token::RightParen
            | Token::RightBrace
            | Token::RightBracket => {
                return;
            }

            // Keep skipping
            _ => {
                parser.advance();
            }
        }
    }
}

/// Skip tokens until we find one of the expected tokens.
pub fn skip_until(parser: &mut Parser, expected: &[Token]) {
    // Loop guard to prevent infinite loops
    let mut guard = LoopGuard::new("skip_until");

    while !parser.at_eof() {
        // Emergency stop if recovery loops too long
        if guard.check().is_err() {
            return;
        }

        // Check if current token matches any expected
        for t in expected {
            if parser.check(t) {
                return;
            }
        }

        parser.advance();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::lexer::Lexer;

    #[test]
    fn test_sync_to_statement_boundary() {
        let source = "invalid tokens let x = 42;";
        let mut parser = Parser::new(source).unwrap();

        // Skip "invalid"
        parser.advance();
        // Skip "tokens"
        parser.advance();

        sync_to_statement_boundary(&mut parser);

        // Should be at "let"
        assert!(matches!(parser.current(), Token::Let));
    }
}
