//! Error recovery strategies for the parser.
//!
//! When the parser encounters an error, it uses these strategies to
//! resynchronize and continue parsing to find more errors.

use super::Parser;
use crate::token::Token;

/// Synchronize to the next statement boundary.
///
/// This is used after encountering a parse error to skip tokens until
/// we reach a point where statement parsing can resume.
pub fn sync_to_statement_boundary(parser: &mut Parser) {
    while !parser.at_eof() {
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

            // Closing brace might end a block
            Token::RightBrace => {
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
    while !parser.at_eof() {
        match parser.current() {
            // Expression delimiters
            Token::Semicolon | Token::Comma | Token::RightParen | Token::RightBrace | Token::RightBracket => {
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
    while !parser.at_eof() && !parser.check_any(expected) {
        parser.advance();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

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
