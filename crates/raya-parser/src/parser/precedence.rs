//! Operator precedence table for expression parsing.
//!
//! This module defines the precedence and associativity of all operators
//! in Raya, following JavaScript/TypeScript precedence rules.

use crate::token::Token;

/// Operator precedence level (higher = tighter binding).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Precedence {
    None = 0,
    Assignment = 1,      // =, +=, -=, etc.
    Conditional = 2,     // ?:
    NullCoalescing = 3,  // ??
    LogicalOr = 4,       // ||
    LogicalAnd = 5,      // &&
    BitwiseOr = 6,       // |
    BitwiseXor = 7,      // ^
    BitwiseAnd = 8,      // &
    Equality = 9,        // ==, !=, ===, !==
    Relational = 10,     // <, >, <=, >=, instanceof, in
    Shift = 11,          // <<, >>, >>>
    Additive = 12,       // +, -
    Multiplicative = 13, // *, /, %
    Exponentiation = 14, // **
    Unary = 15,          // !, ~, +, -, ++, --, typeof, void, delete, await
    Postfix = 16,        // ++, --
    Call = 17,           // (), [], ., ?.
    Member = 18,         // . (member access)
    Primary = 19,        // Literals, identifiers, ()
}

/// Get the precedence of a binary operator token.
pub fn get_precedence(token: &Token) -> Precedence {
    match token {
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
        | Token::GreaterGreaterGreaterEqual => Precedence::Assignment,

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
        Token::Less | Token::LessEqual | Token::Greater | Token::GreaterEqual | Token::Instanceof | Token::In => {
            Precedence::Relational
        }

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
            | Token::StarStar // ** is right-associative
            | Token::Question // ?: is right-associative
    )
}
