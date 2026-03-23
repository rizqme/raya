//! Parser for Raya language
//!
//! This module implements a recursive descent parser that transforms
//! a token stream from the lexer into an Abstract Syntax Tree (AST).

pub mod error;
pub mod expr;
pub mod guards;
pub mod jsx;
pub mod pattern;
pub mod precedence;
pub mod recovery;
pub mod stmt;
pub mod types;

use crate::parser::ast::*;
use crate::parser::interner::Interner;
use crate::parser::interner::Symbol;
use crate::parser::lexer::Lexer;
use crate::parser::token::{LexedToken, Span, Token};

pub use error::{ParseError, ParseErrorKind};

/// Parser state for the Raya programming language.
///
/// This implements a recursive descent parser with 2-token lookahead (LL(2))
/// for handling ambiguous constructs like arrow functions vs parenthesized expressions.
pub struct Parser {
    /// Pre-tokenized input
    tokens: Vec<LexedToken>,

    /// String interner for resolving symbols
    interner: Interner,

    /// Current position in token stream
    pos: usize,

    /// Accumulated parse errors (allows continuing after errors)
    errors: Vec<ParseError>,

    /// Current recursion depth (for preventing stack overflow)
    depth: usize,

    /// Nesting counter for contexts that must not consume `in` as a binary operator.
    disallow_in: usize,
}

/// Backtracking snapshot for speculative parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParserCheckpoint {
    pos: usize,
    depth: usize,
    disallow_in: usize,
}

impl Parser {
    /// Create a new parser from source code.
    pub fn new(source: &str) -> Result<Self, Vec<crate::parser::lexer::LexError>> {
        // Tokenize the entire input first
        let lexer = Lexer::new(source);
        let (mut tokens, interner) = lexer.tokenize()?;

        // Add EOF token if not present
        if tokens.is_empty() || !matches!(tokens.last().unwrap().token, Token::Eof) {
            let eof_span = if let Some(last) = tokens.last() {
                Span::new(
                    last.span.end,
                    last.span.end,
                    last.span.line,
                    last.span.column,
                )
            } else {
                Span::new(0, 0, 1, 1)
            };
            tokens.push(LexedToken::new(Token::Eof, eof_span, false));
        }

        Ok(Self {
            tokens,
            interner,
            pos: 0,
            errors: Vec::new(),
            depth: 0,
            disallow_in: 0,
        })
    }

    /// Create a new parser from pre-tokenized input.
    ///
    /// This is used internally for parsing template literal expressions
    /// where we already have tokens from the lexer.
    pub(crate) fn from_tokens(mut tokens: Vec<LexedToken>, interner: Interner) -> Self {
        // Add EOF token if not present
        if tokens.is_empty() || !matches!(tokens.last().unwrap().token, Token::Eof) {
            let eof_span = if let Some(last) = tokens.last() {
                Span::new(
                    last.span.end,
                    last.span.end,
                    last.span.line,
                    last.span.column,
                )
            } else {
                Span::new(0, 0, 1, 1)
            };
            tokens.push(LexedToken::new(Token::Eof, eof_span, false));
        }

        Self {
            tokens,
            interner,
            pos: 0,
            errors: Vec::new(),
            depth: 0,
            disallow_in: 0,
        }
    }

    /// Parse a single expression from this parser.
    ///
    /// Used for parsing template literal expressions.
    pub(crate) fn parse_single_expression(&mut self) -> Result<Expression, ParseError> {
        expr::parse_expression(self)
    }

    pub(crate) fn with_disallow_in<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, ParseError>,
    ) -> Result<T, ParseError> {
        self.disallow_in += 1;
        let result = f(self);
        self.disallow_in -= 1;
        result
    }

    #[inline(always)]
    pub(crate) fn disallow_in_context(&self) -> bool {
        self.disallow_in > 0
    }

    /// Get a clone of the interner (for template expression parsing).
    pub(crate) fn interner_clone(&self) -> Interner {
        self.interner.clone()
    }

    /// Parse the entire source file into a Module AST.
    ///
    /// Returns the Module and Interner on success, or all accumulated errors on failure.
    /// The Interner is needed to resolve Symbol values back to strings.
    pub fn parse(mut self) -> Result<(Module, crate::parser::interner::Interner), Vec<ParseError>> {
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

        Ok((Module { statements, span }, self.interner))
    }

    // ========================================================================
    // Token Management
    // ========================================================================

    /// Get the current token.
    #[inline(always)]
    pub fn current(&self) -> &Token {
        &self.tokens[self.pos].token
    }

    /// Get the current token's span.
    #[inline(always)]
    pub fn current_span(&self) -> Span {
        self.tokens[self.pos].span
    }

    /// Returns true when the current token is separated from the previous token by
    /// at least one line terminator in the original source.
    ///
    #[inline]
    pub(crate) fn has_line_terminator_before_current(&self) -> bool {
        if self.pos == 0 {
            return false;
        }
        self.tokens[self.pos].line_break_before
    }

    #[inline]
    pub(crate) fn can_insert_semicolon_before_current(&self) -> bool {
        self.at_eof() || self.check(&Token::Semicolon) || self.check(&Token::RightBrace)
    }

    /// Peek at the next token (lookahead).
    #[inline(always)]
    pub fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos + 1).map(|token| &token.token)
    }

    /// Peek at the next token's span.
    #[inline]
    pub fn peek_span(&self) -> Option<Span> {
        self.tokens.get(self.pos + 1).map(|token| token.span)
    }

    #[inline]
    pub(crate) fn has_line_terminator_before_peek(&self) -> bool {
        self.tokens
            .get(self.pos + 1)
            .is_some_and(|token| token.line_break_before)
    }

    /// Peek two tokens ahead (lookahead 2).
    #[inline(always)]
    pub fn peek2(&self) -> Option<&Token> {
        self.tokens.get(self.pos + 2).map(|token| &token.token)
    }

    /// Advance to the next token, returning the current token.
    ///
    /// Note: This clones the token. Consider using current() + advance_without_return() if you don't need the value.
    #[inline]
    pub fn advance(&mut self) -> Token {
        let tok = self.tokens[self.pos].token.clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        tok
    }

    /// Advance without returning the token (avoids clone).
    #[inline(always)]
    pub fn advance_without_return(&mut self) {
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
    }

    /// Save the current parser position for potential backtracking.
    #[inline(always)]
    pub fn checkpoint(&self) -> ParserCheckpoint {
        ParserCheckpoint {
            pos: self.pos,
            depth: self.depth,
            disallow_in: self.disallow_in,
        }
    }

    /// Restore the parser to a previously saved position.
    #[inline(always)]
    pub fn restore(&mut self, checkpoint: ParserCheckpoint) {
        self.pos = checkpoint.pos;
        self.depth = checkpoint.depth;
        self.disallow_in = checkpoint.disallow_in;
    }

    /// Check if the current token matches the given kind.
    #[inline(always)]
    pub fn check(&self, expected: &Token) -> bool {
        std::mem::discriminant(self.current()) == std::mem::discriminant(expected)
    }

    /// Check if the current token matches any of the given kinds.
    #[inline]
    pub fn check_any(&self, expected: &[Token]) -> bool {
        expected.iter().any(|tok| self.check(tok))
    }

    /// Check if we've reached EOF.
    #[inline(always)]
    pub fn at_eof(&self) -> bool {
        matches!(self.current(), Token::Eof)
    }

    /// Resolve a symbol to its string representation.
    #[inline]
    pub fn resolve(&self, symbol: crate::parser::interner::Symbol) -> &str {
        self.interner.resolve(symbol)
    }

    /// Check whether the current token can behave like an identifier in JS value/binding syntax.
    #[inline]
    pub(crate) fn check_identifier_like(&self) -> bool {
        matches!(
            self.current(),
            Token::Identifier(_) | Token::Async | Token::From | Token::Type
        )
    }

    /// Resolve the current token to an identifier-like symbol in JS value/binding syntax.
    pub(crate) fn current_identifier_like_symbol(&mut self) -> Option<Symbol> {
        match self.current().clone() {
            Token::Identifier(name) => Some(name),
            Token::Async => Some(self.intern("async")),
            Token::From => Some(self.intern("from")),
            Token::Type => Some(self.intern("type")),
            _ => None,
        }
    }

    /// Consume the current token as an identifier-like token in JS value/binding syntax.
    pub(crate) fn expect_identifier_like(&mut self) -> Result<Identifier, ParseError> {
        if let Some(name) = self.current_identifier_like_symbol() {
            let span = self.current_span();
            self.advance();
            Ok(Identifier { name, span })
        } else {
            Err(self.unexpected_token(&[Token::Identifier(Symbol::dummy())]))
        }
    }

    /// Intern a new string, returning its symbol.
    #[inline]
    pub fn intern(&mut self, s: &str) -> crate::parser::interner::Symbol {
        self.interner.intern(s)
    }

    /// Consume the current token if it matches the expected kind.
    ///
    /// Returns Ok(token) on match, or Err(ParseError) on mismatch.
    #[inline]
    pub fn expect(&mut self, expected: Token) -> Result<Token, ParseError> {
        if self.check(&expected) {
            Ok(self.advance())
        } else {
            Err(self.unexpected_token(&[expected]))
        }
    }

    /// Consume the current token if it matches any of the expected kinds.
    #[inline]
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
    // Parser Guards (Loop & Depth Protection)
    // ========================================================================

    /// Enter a recursive parsing context
    ///
    /// Returns a RAII guard that automatically decrements depth on drop.
    /// Returns error if maximum depth exceeded.
    #[inline]
    pub fn enter_depth(
        &mut self,
        name: &'static str,
    ) -> Result<guards::DepthGuard<'_>, ParseError> {
        guards::DepthGuard::new(&mut self.depth, name)
    }

    /// Assert that parser position advanced
    ///
    /// Prevents silent infinite loops where position doesn't change
    #[inline]
    pub fn assert_progress(&self, old_pos: usize) -> Result<(), ParseError> {
        if self.pos == old_pos {
            return Err(ParseError::parser_stuck(
                "Parser position did not advance",
                self.current_span(),
            ));
        }
        Ok(())
    }

    /// Get current parser position (for progress tracking)
    #[inline]
    pub fn position(&self) -> usize {
        self.pos
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

    #[test]
    fn test_type_args_in_call() {
        use crate::parser::ast::{Expression, Statement};

        let source = "foo<number>()";
        let parser = Parser::new(source).unwrap();
        let (module, _) = parser.parse().unwrap();

        match &module.statements[0] {
            Statement::Expression(expr_stmt) => match &expr_stmt.expression {
                Expression::Call(call) => {
                    assert!(call.type_args.is_some(), "Should have type arguments");
                    let type_args = call.type_args.as_ref().unwrap();
                    assert_eq!(type_args.len(), 1, "Should have 1 type argument");
                }
                other => panic!("Expected Call, got {:?}", std::mem::discriminant(other)),
            },
            other => panic!(
                "Expected Expression statement, got {:?}",
                std::mem::discriminant(other)
            ),
        }
    }

    #[test]
    fn test_less_than_still_works() {
        use crate::parser::ast::{BinaryOperator, Expression, Statement};

        let source = "a < b";
        let parser = Parser::new(source).unwrap();
        let (module, _) = parser.parse().unwrap();

        match &module.statements[0] {
            Statement::Expression(expr_stmt) => match &expr_stmt.expression {
                Expression::Binary(bin) => {
                    assert!(matches!(bin.operator, BinaryOperator::LessThan));
                }
                other => panic!("Expected Binary, got {:?}", std::mem::discriminant(other)),
            },
            other => panic!(
                "Expected Expression statement, got {:?}",
                std::mem::discriminant(other)
            ),
        }
    }

    #[test]
    fn test_additive_binds_tighter_than_relational() {
        use crate::parser::ast::{BinaryOperator, Expression, Statement};

        let source = "0 + 1 < 3";
        let parser = Parser::new(source).unwrap();
        let (module, _) = parser.parse().unwrap();

        match &module.statements[0] {
            Statement::Expression(expr_stmt) => match &expr_stmt.expression {
                Expression::Binary(outer) => {
                    assert!(matches!(outer.operator, BinaryOperator::LessThan));
                    match &*outer.left {
                        Expression::Binary(inner) => {
                            assert!(matches!(inner.operator, BinaryOperator::Add));
                        }
                        other => panic!(
                            "Expected left side to be additive binary, got {:?}",
                            std::mem::discriminant(other)
                        ),
                    }
                }
                other => panic!("Expected Binary, got {:?}", std::mem::discriminant(other)),
            },
            other => panic!(
                "Expected Expression statement, got {:?}",
                std::mem::discriminant(other)
            ),
        }
    }

    #[test]
    fn test_type_args_multiple() {
        use crate::parser::ast::{Expression, Statement};

        let source = "map<string, number>()";
        let parser = Parser::new(source).unwrap();
        let (module, _) = parser.parse().unwrap();

        match &module.statements[0] {
            Statement::Expression(expr_stmt) => match &expr_stmt.expression {
                Expression::Call(call) => {
                    assert!(call.type_args.is_some(), "Should have type arguments");
                    let type_args = call.type_args.as_ref().unwrap();
                    assert_eq!(type_args.len(), 2, "Should have 2 type arguments");
                }
                other => panic!("Expected Call, got {:?}", std::mem::discriminant(other)),
            },
            other => panic!(
                "Expected Expression statement, got {:?}",
                std::mem::discriminant(other)
            ),
        }
    }

    #[test]
    fn test_detects_line_terminator_between_tokens() {
        let source = "return\n42;";
        let mut parser = Parser::new(source).unwrap();

        assert!(matches!(parser.current(), Token::Return));
        parser.advance();
        assert!(matches!(parser.current(), Token::IntLiteral(42)));
        assert!(parser.has_line_terminator_before_current());
    }

    #[test]
    fn test_detects_line_terminator_through_block_comment() {
        let source = "return /*\n*/ 42;";
        let mut parser = Parser::new(source).unwrap();

        assert!(matches!(parser.current(), Token::Return));
        parser.advance();
        assert!(matches!(parser.current(), Token::IntLiteral(42)));
        assert!(parser.has_line_terminator_before_current());
    }

    #[test]
    fn test_postfix_operator_respects_line_terminator() {
        use crate::parser::ast::Expression;

        let source = "x\n++";
        let mut parser = Parser::new(source).unwrap();
        let expr = parser.parse_single_expression().unwrap();

        match expr {
            Expression::Identifier(_) => {}
            other => panic!("Expected identifier expression, got {:?}", other),
        }
    }

    #[test]
    fn test_postfix_line_terminator_splits_statements() {
        use crate::parser::ast::{Expression, Statement, UnaryOperator};

        let source = "x\n++y;";
        let parser = Parser::new(source).unwrap();
        let (module, _) = parser.parse().unwrap();

        assert_eq!(module.statements.len(), 2);

        match &module.statements[0] {
            Statement::Expression(expr_stmt) => match &expr_stmt.expression {
                Expression::Identifier(_) => {}
                other => panic!("Expected identifier expression, got {:?}", other),
            },
            other => panic!(
                "Expected first statement to be expression, got {:?}",
                std::mem::discriminant(other)
            ),
        }

        match &module.statements[1] {
            Statement::Expression(expr_stmt) => match &expr_stmt.expression {
                Expression::Unary(unary) => {
                    assert_eq!(unary.operator, UnaryOperator::PrefixIncrement);
                    assert!(matches!(&*unary.operand, Expression::Identifier(_)));
                }
                other => panic!("Expected prefix increment expression, got {:?}", other),
            },
            other => panic!(
                "Expected second statement to be expression, got {:?}",
                std::mem::discriminant(other)
            ),
        }
    }

    #[test]
    fn test_parenthesized_object_expression_stays_grouped() {
        use crate::parser::ast::Expression;

        let mut parser = Parser::new("({ a: 1 })").unwrap();
        let expr = parser.parse_single_expression().unwrap();

        match expr {
            Expression::Object(_) => {}
            other => panic!("Expected object expression, got {:?}", other),
        }
    }

    #[test]
    fn test_parenthesized_array_expression_stays_grouped() {
        use crate::parser::ast::Expression;

        let mut parser = Parser::new("([1, 2])").unwrap();
        let expr = parser.parse_single_expression().unwrap();

        match expr {
            Expression::Array(_) => {}
            other => panic!("Expected array expression, got {:?}", other),
        }
    }

    #[test]
    fn test_destructured_object_arrow_still_parses() {
        use crate::parser::ast::Expression;

        let mut parser = Parser::new("({ a }) => a").unwrap();
        let expr = parser.parse_single_expression().unwrap();

        match expr {
            Expression::Arrow(_) => {}
            other => panic!("Expected arrow function, got {:?}", other),
        }
    }

    #[test]
    fn test_destructured_array_arrow_still_parses() {
        use crate::parser::ast::Expression;

        let mut parser = Parser::new("([a]) => a").unwrap();
        let expr = parser.parse_single_expression().unwrap();

        match expr {
            Expression::Arrow(_) => {}
            other => panic!("Expected arrow function, got {:?}", other),
        }
    }

    #[test]
    fn test_parenthesized_assignment_expression_stays_grouped() {
        use crate::parser::ast::Expression;

        let mut parser = Parser::new("(x = 1)").unwrap();
        let expr = parser.parse_single_expression().unwrap();

        match expr {
            Expression::Assignment(_) => {}
            other => panic!("Expected assignment expression, got {:?}", other),
        }
    }

    #[test]
    fn test_defaulted_parenthesized_arrow_parses() {
        use crate::parser::ast::Expression;

        let mut parser = Parser::new("(p = eval(\"1\"), arguments) => 0").unwrap();
        let expr = parser.parse_single_expression().unwrap();

        match expr {
            Expression::Arrow(_) => {}
            other => panic!("Expected arrow function, got {:?}", other),
        }
    }

    #[test]
    fn test_async_generator_function_expression_parses() {
        use crate::parser::ast::Expression;

        let mut parser = Parser::new("async function* f() { yield 1; }").unwrap();
        let expr = parser.parse_single_expression().unwrap();

        match expr {
            Expression::Function(func) => {
                assert!(func.is_async);
                assert!(func.is_generator);
            }
            other => panic!("Expected async generator function expression, got {:?}", other),
        }
    }

    #[test]
    fn test_parenthesized_comma_expression_stays_grouped() {
        use crate::parser::ast::{BinaryOperator, Expression};

        let mut parser = Parser::new("(p = eval(\"1\"), arguments)").unwrap();
        let expr = parser.parse_single_expression().unwrap();

        let Expression::Binary(binary) = expr else {
            panic!("Expected comma expression");
        };
        assert!(matches!(binary.operator, BinaryOperator::Comma));
    }

    #[test]
    fn test_call_expression_tracks_spread_arguments() {
        use crate::parser::ast::{CallArgument, Expression};

        let mut parser = Parser::new("fn(a, ...rest, b)").unwrap();
        let expr = parser.parse_single_expression().unwrap();

        let Expression::Call(call) = expr else {
            panic!("Expected call expression");
        };
        assert_eq!(call.arguments.len(), 3);
        assert!(matches!(call.arguments[0], CallArgument::Expression(_)));
        assert!(matches!(call.arguments[1], CallArgument::Spread(_)));
        assert!(matches!(call.arguments[2], CallArgument::Expression(_)));
    }

    #[test]
    fn test_new_expression_tracks_spread_arguments() {
        use crate::parser::ast::{CallArgument, Expression};

        let mut parser = Parser::new("new Foo(...args)").unwrap();
        let expr = parser.parse_single_expression().unwrap();

        let Expression::New(new_expr) = expr else {
            panic!("Expected new expression");
        };
        assert_eq!(new_expr.arguments.len(), 1);
        assert!(matches!(new_expr.arguments[0], CallArgument::Spread(_)));
    }
}
