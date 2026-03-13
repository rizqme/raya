//! Operator precedence table for expression parsing.
//!
//! This module defines the precedence and associativity of all operators
//! in Raya, following JavaScript/TypeScript precedence rules.

use crate::parser::token::Token;

/// Operator precedence level (higher = tighter binding).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Precedence {
    None = 0,
    Comma = 1,           // ,
    Assignment = 2,      // =, +=, -=, etc.
    Conditional = 3,     // ?:
    NullCoalescing = 4,  // ??
    LogicalOr = 5,       // ||
    LogicalAnd = 6,      // &&
    BitwiseOr = 7,       // |
    BitwiseXor = 8,      // ^
    BitwiseAnd = 9,      // &
    Equality = 10,       // ==, !=, ===, !==
    Relational = 11,     // <, >, <=, >=, instanceof, in
    Shift = 12,          // <<, >>, >>>
    Additive = 13,       // +, -
    Multiplicative = 14, // *, /, %
    Exponentiation = 15, // **
    Unary = 16,          // !, ~, +, -, ++, --, typeof, void, delete, await
    Postfix = 17,        // ++, --
    Call = 18,           // (), [], ., ?.
    Member = 19,         // . (member access)
    Primary = 20,        // Literals, identifiers, ()
}

/// Get the precedence of a binary operator token.
pub fn get_precedence(token: &Token) -> Precedence {
    match token {
        Token::Comma => Precedence::Comma,
        // Assignment
        Token::Equal
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
        | Token::PipePipeEqual
        | Token::AmpersandAmpersandEqual
        | Token::QuestionQuestionEqual => Precedence::Assignment,

        // Conditional
        Token::Question => Precedence::Conditional,

        // Null coalescing
        Token::QuestionQuestion => Precedence::NullCoalescing,

        // Logical OR
        Token::PipePipe => Precedence::LogicalOr,

        // Logical AND
        Token::AmpAmp => Precedence::LogicalAnd,

        // Bitwise OR
        Token::Pipe => Precedence::BitwiseOr,

        // Bitwise XOR
        Token::Caret => Precedence::BitwiseXor,

        // Bitwise AND
        Token::Amp => Precedence::BitwiseAnd,

        // Equality
        Token::EqualEqual | Token::BangEqual | Token::EqualEqualEqual | Token::BangEqualEqual => {
            Precedence::Equality
        }

        // Relational
        Token::Less
        | Token::LessEqual
        | Token::Greater
        | Token::GreaterEqual
        | Token::Instanceof
        | Token::In
        | Token::As => Precedence::Relational,

        // Shift
        Token::LessLess | Token::GreaterGreater | Token::GreaterGreaterGreater => Precedence::Shift,

        // Additive
        Token::Plus | Token::Minus => Precedence::Additive,

        // Multiplicative
        Token::Star | Token::Slash | Token::Percent => Precedence::Multiplicative,

        // Exponentiation (right-associative)
        Token::StarStar => Precedence::Exponentiation,

        _ => Precedence::None,
    }
}

/// Check if an operator is right-associative.
pub fn is_right_associative(token: &Token) -> bool {
    matches!(
        token,
        Token::Equal
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
            | Token::PipePipeEqual
            | Token::AmpersandAmpersandEqual
            | Token::QuestionQuestionEqual
            | Token::StarStar // ** is right-associative
            | Token::Question // ?: is right-associative
    )
}
