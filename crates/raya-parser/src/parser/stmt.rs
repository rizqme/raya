//! Statement parsing

use super::{ParseError, Parser};
use crate::ast::*;
use crate::interner::Symbol;
use crate::token::Token;

/// Parse a statement.
pub fn parse_statement(parser: &mut Parser) -> Result<Statement, ParseError> {
    // Check depth before entering
    parser.depth += 1;
    if parser.depth > super::guards::MAX_PARSE_DEPTH {
        parser.depth -= 1;
        return Err(ParseError::parser_limit_exceeded(
            format!("Maximum nesting depth ({}) exceeded in statement", super::guards::MAX_PARSE_DEPTH),
            parser.current_span(),
        ));
    }

    // Use inner function so `?` can be used freely while ensuring depth is always decremented
    let result = parse_statement_inner(parser);

    parser.depth -= 1;
    result
}

/// Inner statement parsing logic - allows use of `?` operator
fn parse_statement_inner(parser: &mut Parser) -> Result<Statement, ParseError> {
    match parser.current() {
        Token::Let | Token::Const => parse_variable_declaration(parser),
        Token::Function => parse_function_declaration(parser),

        // Distinguish between async function declaration and async call expression
        Token::Async => {
            // Look ahead to see if this is "async function" or "async foo()"
            if let Some(Token::Function) = parser.peek() {
                // async function declaration
                parse_function_declaration(parser)
            } else {
                // async call expression - parse as expression statement
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

        Token::Class => todo!("parse class declaration"),
        Token::Type => todo!("parse type alias declaration"),
        Token::If => parse_if_statement(parser),
        Token::While => parse_while_statement(parser),
        Token::For => parse_for_statement(parser),
        Token::Switch => todo!("parse switch statement"),
        Token::Try => todo!("parse try statement"),
        Token::Return => parse_return_statement(parser),
        Token::Break => parse_break_statement(parser),
        Token::Continue => parse_continue_statement(parser),
        Token::Throw => parse_throw_statement(parser),
        Token::Import => todo!("parse import declaration"),
        Token::Export => todo!("parse export declaration"),

        // IMPORTANT: Raya does NOT support standalone block statements
        // The { } syntax is ONLY used for:
        // 1. Function bodies (handled in function declaration parsing)
        // 2. Object literals (handled here as expression statements)
        // 3. Control flow bodies (if, while, for - handled in their respective parsers)
        // At the statement level, { always starts an object literal expression
        Token::LeftBrace => {
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

// ============================================================================
// Variable Declarations
// ============================================================================

/// Parse variable declaration: let x = 1; or const y: number = 2;
fn parse_variable_declaration(parser: &mut Parser) -> Result<Statement, ParseError> {
    let start_span = parser.current_span();

    // Parse let or const
    let kind = match parser.current() {
        Token::Let => VariableKind::Let,
        Token::Const => VariableKind::Const,
        _ => unreachable!(),
    };
    parser.advance();

    // Parse pattern (for now, just identifier - destructuring later)
    let pattern = super::pattern::parse_pattern(parser)?;

    // Optional type annotation
    let type_annotation = if parser.check(&Token::Colon) {
        parser.advance();
        Some(super::types::parse_type_annotation(parser)?)
    } else {
        None
    };

    // Initializer (required for const, optional for let)
    let initializer = if parser.check(&Token::Equal) {
        parser.advance();
        Some(super::expr::parse_expression(parser)?)
    } else {
        if kind == VariableKind::Const {
            use super::ParseErrorKind;
            return Err(ParseError {
                kind: ParseErrorKind::InvalidSyntax {
                    reason: "const declarations must have an initializer".to_string(),
                },
                span: start_span,
                message: "Missing initializer for const declaration".to_string(),
                suggestion: Some("Add an initializer: const x = value;".to_string()),
            });
        }
        None
    };

    // Optional semicolon
    if parser.check(&Token::Semicolon) {
        parser.advance();
    }

    let span = if let Some(ref init) = initializer {
        parser.combine_spans(&start_span, init.span())
    } else if let Some(ref type_ann) = type_annotation {
        parser.combine_spans(&start_span, &type_ann.span)
    } else {
        parser.combine_spans(&start_span, pattern.span())
    };

    Ok(Statement::VariableDecl(VariableDecl {
        kind,
        pattern,
        type_annotation,
        initializer,
        span,
    }))
}

// ============================================================================
// Function Declarations
// ============================================================================

/// Parse function declaration
fn parse_function_declaration(parser: &mut Parser) -> Result<Statement, ParseError> {
    let start_span = parser.current_span();

    // Parse 'async' modifier
    let is_async = if parser.check(&Token::Async) {
        parser.advance();
        true
    } else {
        false
    };

    // Parse 'function' keyword
    parser.expect(Token::Function)?;

    // Parse function name
    let name = if let Token::Identifier(name) = parser.current() {
        let name_str = name.clone();
        let name_span = parser.current_span();
        parser.advance();
        Identifier {
            name: name_str,
            span: name_span,
        }
    } else {
        return Err(parser.unexpected_token(&[Token::Identifier(Symbol::dummy())]));
    };

    // Optional type parameters
    let type_params = if parser.check(&Token::Less) {
        parser.advance();
        Some(parse_type_parameters(parser)?)
    } else {
        None
    };

    // Parse parameters
    parser.expect(Token::LeftParen)?;
    let params = parse_function_parameters(parser)?;
    parser.expect(Token::RightParen)?;

    // Optional return type
    let return_type = if parser.check(&Token::Colon) {
        parser.advance();
        Some(super::types::parse_type_annotation(parser)?)
    } else {
        None
    };

    // Parse body
    parser.expect(Token::LeftBrace)?;
    let body = parse_block_statement(parser)?;

    let span = parser.combine_spans(&start_span, &body.span);

    Ok(Statement::FunctionDecl(FunctionDecl {
        name,
        type_params,
        params,
        return_type,
        body,
        is_async,
        span,
    }))
}

/// Parse function parameters
fn parse_function_parameters(parser: &mut Parser) -> Result<Vec<Parameter>, ParseError> {
    let mut params = Vec::new();
    let mut guard = super::guards::LoopGuard::new("function_parameters");

    while !parser.check(&Token::RightParen) && !parser.at_eof() {
        guard.check()?;
        let start_span = parser.current_span();

        // TODO: Parse decorators when implemented
        let decorators = vec![];

        // Parse parameter pattern
        let pattern = super::pattern::parse_pattern(parser)?;

        // Optional type annotation
        let type_annotation = if parser.check(&Token::Colon) {
            parser.advance();
            Some(super::types::parse_type_annotation(parser)?)
        } else {
            None
        };

        let span = if let Some(ref type_ann) = type_annotation {
            parser.combine_spans(&start_span, &type_ann.span)
        } else {
            parser.combine_spans(&start_span, pattern.span())
        };

        params.push(Parameter {
            decorators,
            pattern,
            type_annotation,
            span,
        });

        if !parser.check(&Token::RightParen) {
            parser.expect(Token::Comma)?;
        }
    }

    Ok(params)
}

/// Parse type parameters (generics)
fn parse_type_parameters(parser: &mut Parser) -> Result<Vec<TypeParameter>, ParseError> {
    let mut type_params = Vec::new();
    let mut guard = super::guards::LoopGuard::new("type_parameters");

    while !parser.check(&Token::Greater) && !parser.at_eof() {
        guard.check()?;
        let start_span = parser.current_span();

        let name = if let Token::Identifier(name) = parser.current() {
            let name_str = name.clone();
            parser.advance();
            Identifier {
                name: name_str,
                span: start_span,
            }
        } else {
            return Err(parser.unexpected_token(&[Token::Identifier(Symbol::dummy())]));
        };

        // Optional constraint: T extends Foo
        let constraint = if parser.check(&Token::Extends) {
            parser.advance();
            Some(super::types::parse_type_annotation(parser)?)
        } else {
            None
        };

        // Optional default: T = DefaultType
        let default = if parser.check(&Token::Equal) {
            parser.advance();
            Some(super::types::parse_type_annotation(parser)?)
        } else {
            None
        };

        let span = if let Some(ref d) = default {
            parser.combine_spans(&start_span, &d.span)
        } else if let Some(ref c) = constraint {
            parser.combine_spans(&start_span, &c.span)
        } else {
            start_span
        };

        type_params.push(TypeParameter {
            name,
            constraint,
            default,
            span,
        });

        if !parser.check(&Token::Greater) {
            parser.expect(Token::Comma)?;
        }
    }

    parser.expect(Token::Greater)?;
    Ok(type_params)
}

/// Parse block statement (sequence of statements in { })
fn parse_block_statement(parser: &mut Parser) -> Result<BlockStatement, ParseError> {
    let start_span = parser.current_span();
    let mut statements = Vec::new();
    let mut guard = super::guards::LoopGuard::new("block_statements");

    while !parser.check(&Token::RightBrace) && !parser.at_eof() {
        guard.check()?;
        let stmt = parse_statement(parser)?;
        statements.push(stmt);
    }

    let end_span = parser.current_span();
    parser.expect(Token::RightBrace)?;
    let span = parser.combine_spans(&start_span, &end_span);

    Ok(BlockStatement { statements, span })
}

// ============================================================================
// Control Flow Statements
// ============================================================================

/// Parse if statement
fn parse_if_statement(parser: &mut Parser) -> Result<Statement, ParseError> {
    let start_span = parser.current_span();
    parser.expect(Token::If)?;

    // Parse condition (with parens)
    parser.expect(Token::LeftParen)?;
    let condition = super::expr::parse_expression(parser)?;
    parser.expect(Token::RightParen)?;

    // Parse then branch (must be a block statement in Raya)
    parser.expect(Token::LeftBrace)?;
    let then_block = parse_block_statement(parser)?;
    let then_branch = Box::new(Statement::Block(then_block));

    // Optional else branch
    let else_branch = if parser.check(&Token::Else) {
        parser.advance();
        if parser.check(&Token::If) {
            // else if - parse as nested if statement
            Some(Box::new(parse_if_statement(parser)?))
        } else {
            // else block
            parser.expect(Token::LeftBrace)?;
            let else_block = parse_block_statement(parser)?;
            Some(Box::new(Statement::Block(else_block)))
        }
    } else {
        None
    };

    let span = if let Some(ref else_b) = else_branch {
        parser.combine_spans(&start_span, else_b.span())
    } else {
        parser.combine_spans(&start_span, then_branch.span())
    };

    Ok(Statement::If(IfStatement {
        condition,
        then_branch,
        else_branch,
        span,
    }))
}

/// Parse while statement
fn parse_while_statement(parser: &mut Parser) -> Result<Statement, ParseError> {
    let start_span = parser.current_span();
    parser.expect(Token::While)?;

    // Parse condition (with parens)
    parser.expect(Token::LeftParen)?;
    let condition = super::expr::parse_expression(parser)?;
    parser.expect(Token::RightParen)?;

    // Parse body (must be a block statement)
    parser.expect(Token::LeftBrace)?;
    let body_block = parse_block_statement(parser)?;
    let body = Box::new(Statement::Block(body_block));

    let span = parser.combine_spans(&start_span, body.span());

    Ok(Statement::While(WhileStatement {
        condition,
        body,
        span,
    }))
}

/// Parse for statement
fn parse_for_statement(parser: &mut Parser) -> Result<Statement, ParseError> {
    let start_span = parser.current_span();
    parser.expect(Token::For)?;
    parser.expect(Token::LeftParen)?;

    // Parse init
    let init = if parser.check(&Token::Semicolon) {
        parser.advance(); // consume semicolon
        None
    } else if parser.check(&Token::Let) || parser.check(&Token::Const) {
        // Variable declaration (don't parse the semicolon, we'll do it after)
        let kind = match parser.current() {
            Token::Let => VariableKind::Let,
            Token::Const => VariableKind::Const,
            _ => unreachable!(),
        };
        parser.advance();

        let pattern = super::pattern::parse_pattern(parser)?;

        let type_annotation = if parser.check(&Token::Colon) {
            parser.advance();
            Some(super::types::parse_type_annotation(parser)?)
        } else {
            None
        };

        let initializer = if parser.check(&Token::Equal) {
            parser.advance();
            Some(super::expr::parse_expression(parser)?)
        } else {
            None
        };

        let span = pattern.span().clone();

        let decl = VariableDecl {
            kind,
            pattern,
            type_annotation,
            initializer,
            span,
        };

        parser.expect(Token::Semicolon)?; // consume semicolon after variable declaration
        Some(ForInit::VariableDecl(decl))
    } else {
        // Expression
        let expr = super::expr::parse_expression(parser)?;
        parser.expect(Token::Semicolon)?; // consume semicolon after expression
        Some(ForInit::Expression(expr))
    };

    // Parse test
    let test = if parser.check(&Token::Semicolon) {
        None
    } else {
        Some(super::expr::parse_expression(parser)?)
    };
    parser.expect(Token::Semicolon)?;

    // Parse update
    let update = if parser.check(&Token::RightParen) {
        None
    } else {
        Some(super::expr::parse_expression(parser)?)
    };
    parser.expect(Token::RightParen)?;

    // Parse body (must be a block statement)
    parser.expect(Token::LeftBrace)?;
    let body_block = parse_block_statement(parser)?;
    let body = Box::new(Statement::Block(body_block));

    let span = parser.combine_spans(&start_span, body.span());

    Ok(Statement::For(ForStatement {
        init,
        test,
        update,
        body,
        span,
    }))
}

// ============================================================================
// Jump Statements
// ============================================================================

/// Parse return statement
fn parse_return_statement(parser: &mut Parser) -> Result<Statement, ParseError> {
    let start_span = parser.current_span();
    parser.expect(Token::Return)?;

    // Optional return value
    let value = if parser.check(&Token::Semicolon) || parser.at_eof() {
        None
    } else {
        Some(super::expr::parse_expression(parser)?)
    };

    // Optional semicolon
    if parser.check(&Token::Semicolon) {
        parser.advance();
    }

    let span = if let Some(ref val) = value {
        parser.combine_spans(&start_span, val.span())
    } else {
        start_span
    };

    Ok(Statement::Return(ReturnStatement { value, span }))
}

/// Parse break statement
fn parse_break_statement(parser: &mut Parser) -> Result<Statement, ParseError> {
    let start_span = parser.current_span();
    parser.expect(Token::Break)?;

    // Optional label (TODO: labels not yet supported)
    let label = None;

    // Optional semicolon
    if parser.check(&Token::Semicolon) {
        parser.advance();
    }

    Ok(Statement::Break(BreakStatement {
        label,
        span: start_span,
    }))
}

/// Parse continue statement
fn parse_continue_statement(parser: &mut Parser) -> Result<Statement, ParseError> {
    let start_span = parser.current_span();
    parser.expect(Token::Continue)?;

    // Optional label (TODO: labels not yet supported)
    let label = None;

    // Optional semicolon
    if parser.check(&Token::Semicolon) {
        parser.advance();
    }

    Ok(Statement::Continue(ContinueStatement {
        label,
        span: start_span,
    }))
}

/// Parse throw statement
fn parse_throw_statement(parser: &mut Parser) -> Result<Statement, ParseError> {
    let start_span = parser.current_span();
    parser.expect(Token::Throw)?;

    // Required expression
    let value = super::expr::parse_expression(parser)?;

    // Optional semicolon
    if parser.check(&Token::Semicolon) {
        parser.advance();
    }

    let span = parser.combine_spans(&start_span, value.span());

    Ok(Statement::Throw(ThrowStatement { value, span }))
}
