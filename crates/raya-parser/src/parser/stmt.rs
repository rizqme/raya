//! Statement parsing

use super::{ParseError, Parser};
use crate::ast::{ExpressionStatement, Statement};
use crate::token::Token;

/// Parse a statement.
pub fn parse_statement(parser: &mut Parser) -> Result<Statement, ParseError> {
    // Placeholder implementation
    match parser.current() {
        Token::Let | Token::Const => todo!("parse variable declaration"),
        Token::Function => todo!("parse function declaration"),
        Token::Class => todo!("parse class declaration"),
        Token::Type => todo!("parse type alias declaration"),
        Token::If => todo!("parse if statement"),
        Token::While => todo!("parse while statement"),
        Token::For => todo!("parse for statement"),
        Token::Switch => todo!("parse switch statement"),
        Token::Try => todo!("parse try statement"),
        Token::Return => todo!("parse return statement"),
        Token::Break => todo!("parse break statement"),
        Token::Continue => todo!("parse continue statement"),
        Token::Throw => todo!("parse throw statement"),
        Token::Import => todo!("parse import declaration"),
        Token::Export => todo!("parse export declaration"),
        Token::LeftBrace => todo!("parse block statement"),
        Token::Semicolon => {
            let span = parser.current_span();
            parser.advance();
            Ok(Statement::Empty(span))
        }
        _ => {
            // Parse expression statement
            let start_span = parser.current_span();
            let expression = super::expr::parse_expression(parser)?;

            // Optional semicolon
            if parser.check(&Token::Semicolon) {
                parser.advance();
            }

            let span = parser.combine_spans(&start_span, expression.span());

            Ok(Statement::Expression(ExpressionStatement {
                expression,
                span,
            }))
        }
    }
}
