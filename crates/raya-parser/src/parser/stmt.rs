//! Statement parsing

use super::{ParseError, Parser};
use crate::ast::Statement;
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
        Token::Semicolon => todo!("parse empty statement"),
        _ => todo!("parse expression statement"),
    }
}
