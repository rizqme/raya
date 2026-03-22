//! Statement parsing

use super::expr::keyword_as_property_name;
use super::{ParseError, ParseErrorKind, Parser};
use crate::parser::ast::*;
use crate::parser::interner::Symbol;
use crate::parser::token::{Span, Token};

fn looks_like_type_alias_declaration(parser: &mut Parser) -> bool {
    if !parser.check(&Token::Type) {
        return false;
    }

    let checkpoint = parser.checkpoint();
    parser.advance();

    let mut ok = parser.check(&Token::Identifier(Symbol::dummy()));
    if ok {
        parser.advance();
        if parser.check(&Token::Less) {
            parser.advance();
            ok = parse_type_parameters(parser).is_ok() && parser.check(&Token::Equal);
        } else {
            ok = parser.check(&Token::Equal);
        }
    }

    parser.restore(checkpoint);
    ok
}

/// Parse a statement.
pub fn parse_statement(parser: &mut Parser) -> Result<Statement, ParseError> {
    // Check depth before entering
    parser.depth += 1;
    if parser.depth > super::guards::MAX_PARSE_DEPTH {
        parser.depth -= 1;
        return Err(ParseError::parser_limit_exceeded(
            format!(
                "Maximum nesting depth ({}) exceeded in statement",
                super::guards::MAX_PARSE_DEPTH
            ),
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
    if looks_like_type_alias_declaration(parser) {
        return parse_type_alias_declaration(parser, Vec::new());
    }

    match parser.current() {
        Token::Let | Token::Const | Token::Var => parse_variable_declaration(parser),
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

        Token::Class | Token::Abstract | Token::At => parse_class_declaration(parser),
        Token::Interface => parse_interface_declaration(parser, Vec::new()),
        Token::Annotation(_) => {
            // Annotations can appear before class or type declarations
            let annotations = parse_annotations(parser)?;
            if looks_like_type_alias_declaration(parser) {
                return parse_type_alias_declaration(parser, annotations);
            }

            match parser.current() {
                Token::Class | Token::Abstract | Token::At => {
                    parse_class_declaration_with_annotations(parser, annotations)
                }
                Token::Interface => parse_interface_declaration(parser, annotations),
                // Allow annotations before other statements (e.g., //@@builtin_primitive before const)
                // — annotations are discarded for non-class/type declarations
                _ => parse_statement(parser),
            }
        }
        Token::If => parse_if_statement(parser),
        Token::While => parse_while_statement(parser),
        Token::Do => parse_do_while_statement(parser),
        Token::For => parse_for_statement(parser),
        Token::Switch => parse_switch_statement(parser),
        Token::Try => parse_try_statement(parser),
        Token::Return => parse_return_statement(parser),
        Token::Yield => parse_yield_statement(parser),
        Token::Break => parse_break_statement(parser),
        Token::Continue => parse_continue_statement(parser),
        Token::Throw => parse_throw_statement(parser),
        Token::Import => parse_import_declaration(parser),
        Token::Export => parse_export_declaration(parser),

        // Standalone block statement or object literal expression
        // We use lookahead to distinguish:
        // - Block: { followed by statement-starting keywords, declarations, or }
        // - Object literal: { followed by property keys (identifiers, strings, etc.)
        Token::LeftBrace => parse_block_or_object_literal(parser),

        Token::Debugger => {
            let span = parser.current_span();
            parser.advance();
            // Optional semicolon
            if parser.check(&Token::Semicolon) {
                parser.advance();
            }
            Ok(Statement::Debugger(span))
        }

        Token::Semicolon => {
            let span = parser.current_span();
            parser.advance();
            Ok(Statement::Empty(span))
        }
        _ => {
            // Check for labeled statement: identifier followed by colon at statement level
            if parser.check_identifier_like() {
                if let Some(Token::Colon) = parser.peek() {
                    return parse_labeled_statement(parser);
                }
            }

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

    // Parse let, const, or var
    let kind = match parser.current() {
        Token::Let => VariableKind::Let,
        Token::Const => VariableKind::Const,
        Token::Var => VariableKind::Var,
        _ => unreachable!(),
    };
    parser.advance();

    let mut decls = Vec::new();
    loop {
        let pattern = super::pattern::parse_pattern(parser)?;

        let type_annotation = if parser.check(&Token::Colon) {
            parser.advance();
            Some(super::types::parse_type_annotation(parser)?)
        } else {
            None
        };

        let initializer = if parser.check(&Token::Equal) {
            parser.advance();
            Some(super::expr::parse_assignment_expression(parser)?)
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

        let span = if let Some(ref init) = initializer {
            parser.combine_spans(&start_span, init.span())
        } else if let Some(ref type_ann) = type_annotation {
            parser.combine_spans(&start_span, &type_ann.span)
        } else {
            parser.combine_spans(&start_span, pattern.span())
        };

        decls.push(VariableDecl {
            kind,
            pattern,
            type_annotation,
            initializer,
            span,
        });

        if !parser.check(&Token::Comma) {
            break;
        }
        parser.advance();
    }

    if parser.check(&Token::Semicolon) {
        parser.advance();
    }

    if decls.len() == 1 {
        return Ok(Statement::VariableDecl(decls.pop().expect("single decl")));
    }

    let end_span = decls.last().map(|decl| decl.span).unwrap_or(start_span);
    Ok(Statement::Block(BlockStatement {
        statements: decls.into_iter().map(Statement::VariableDecl).collect(),
        span: parser.combine_spans(&start_span, &end_span),
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
    let is_generator = if parser.check(&Token::Star) {
        parser.advance();
        true
    } else {
        false
    };

    // Parse function name
    let name = parser.expect_identifier_like()?;

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
        is_generator,
        span,
    }))
}

/// Parse function parameters
pub(super) fn parse_function_parameters(parser: &mut Parser) -> Result<Vec<Parameter>, ParseError> {
    let mut params = Vec::new();
    let mut guard = super::guards::LoopGuard::new("function_parameters");

    while !parser.check(&Token::RightParen) && !parser.at_eof() {
        guard.check()?;
        let start_span = parser.current_span();

        // Check for rest parameter: ...identifier
        let is_rest = if parser.check(&Token::DotDotDot) {
            parser.advance();
            true
        } else {
            false
        };

        // Parse parameter decorators (@Inject, @Validate, etc.)
        let decorators = parse_decorators(parser)?;

        // Parse optional visibility modifier for constructor parameter properties
        let visibility = match parser.current() {
            Token::Public => {
                parser.advance();
                Some(Visibility::Public)
            }
            Token::Private => {
                parser.advance();
                Some(Visibility::Private)
            }
            Token::Protected => {
                parser.advance();
                Some(Visibility::Protected)
            }
            _ => None,
        };

        // Parse parameter pattern
        let pattern = super::pattern::parse_pattern(parser)?;

        // Parse optional marker (param?: Type)
        // Rest parameters cannot be optional
        let optional = if parser.check(&Token::Question) {
            if is_rest {
                return Err(ParseError::invalid_syntax(
                    "Rest parameter cannot be optional",
                    parser.current_span(),
                ));
            }
            parser.advance();
            true
        } else {
            false
        };

        // Optional type annotation
        let type_annotation = if parser.check(&Token::Colon) {
            parser.advance();
            Some(super::types::parse_type_annotation(parser)?)
        } else {
            None
        };

        // Optional default value (e.g., `x: number = 10`)
        // Rest parameters cannot have default values
        let default_value = if parser.check(&Token::Equal) {
            if is_rest {
                return Err(ParseError::invalid_syntax(
                    "Rest parameter cannot have a default value",
                    parser.current_span(),
                ));
            }
            parser.advance();
            Some(super::expr::parse_assignment_expression(parser)?)
        } else {
            None
        };

        let span = if let Some(ref default) = default_value {
            parser.combine_spans(&start_span, default.span())
        } else if let Some(ref type_ann) = type_annotation {
            parser.combine_spans(&start_span, &type_ann.span)
        } else {
            parser.combine_spans(&start_span, pattern.span())
        };

        params.push(Parameter {
            decorators,
            visibility,
            pattern,
            type_annotation,
            optional,
            default_value,
            is_rest,
            span,
        });

        if !parser.check(&Token::RightParen) {
            parser.expect(Token::Comma)?;
        }
    }

    Ok(params)
}

/// Parse type parameters (generics)
pub(super) fn parse_type_parameters(parser: &mut Parser) -> Result<Vec<TypeParameter>, ParseError> {
    let mut type_params = Vec::new();
    let mut guard = super::guards::LoopGuard::new("type_parameters");

    while !parser.check(&Token::Greater) && !parser.at_eof() {
        guard.check()?;
        let start_span = parser.current_span();

        let name = if let Token::Identifier(name) = parser.current() {
            let name_str = *name;
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
pub(super) fn parse_block_statement(parser: &mut Parser) -> Result<BlockStatement, ParseError> {
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

/// Parse a block or a single statement for use as a control flow body.
/// Parse a standalone block statement or object literal expression.
/// Uses lookahead to distinguish between:
/// - Block statement: { const x = 1; return x; }
/// - Object literal expression: { x: 1, y: 2 }
fn parse_block_or_object_literal(parser: &mut Parser) -> Result<Statement, ParseError> {
    let start_span = parser.current_span();

    // Lookahead to determine if this is a block or object literal
    // A block starts with statement keywords or declarations.
    // An empty `{}` in statement position is a block (JS semantics).
    // An object literal starts with property keys (identifiers, strings, numbers, etc.)
    let is_likely_block = if let Some(next_token) = parser.peek() {
        matches!(
            next_token,
            Token::RightBrace       // Empty block: {}
            | Token::Let            // Block starts with let
            | Token::Const          // Block starts with const
            | Token::Var            // Block starts with var
            | Token::Return         // Block starts with return
            | Token::If             // Block starts with if
            | Token::While          // Block starts with while
            | Token::Do             // Block starts with do
            | Token::For            // Block starts with for
            | Token::Switch         // Block starts with switch
            | Token::Try            // Block starts with try
            | Token::Throw          // Block starts with throw
            | Token::Break          // Block starts with break
            | Token::Continue       // Block starts with continue
            | Token::Debugger       // Block starts with debugger
            | Token::Semicolon      // Empty statement
            | Token::Function       // Block starts with function
            | Token::Async          // Block starts with async
            | Token::Class          // Block starts with class
            | Token::Type           // Block starts with type
            | Token::Interface      // Block starts with interface
            | Token::Export         // Block starts with export
            | Token::Import         // Block starts with import
            | Token::At             // Block starts with annotation
            | Token::LeftBrace // Nested block: { { ... } }
        )
    } else {
        false
    };

    if is_likely_block {
        // Parse as block statement
        parser.expect(Token::LeftBrace)?;
        let block = parse_block_statement(parser)?;
        Ok(Statement::Block(block))
    } else {
        // Parse as object literal expression
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

/// Parse a block or a single statement for use as a control flow body.
/// Supports both `if (x) { ... }` and `if (x) return y;` syntax.
fn parse_block_or_statement(parser: &mut Parser) -> Result<Box<Statement>, ParseError> {
    if parser.check(&Token::LeftBrace) {
        parser.advance(); // consume '{'
        let block = parse_block_statement(parser)?;
        Ok(Box::new(Statement::Block(block)))
    } else {
        Ok(Box::new(parse_statement(parser)?))
    }
}

/// Parse if statement
fn parse_if_statement(parser: &mut Parser) -> Result<Statement, ParseError> {
    let start_span = parser.current_span();
    parser.expect(Token::If)?;

    // Parse condition (with parens)
    parser.expect(Token::LeftParen)?;
    let condition = super::expr::parse_expression(parser)?;
    parser.expect(Token::RightParen)?;

    // Parse then branch (block or single statement)
    let then_branch = parse_block_or_statement(parser)?;

    // Optional else branch
    let else_branch = if parser.check(&Token::Else) {
        parser.advance();
        if parser.check(&Token::If) {
            // else if - parse as nested if statement
            Some(Box::new(parse_if_statement(parser)?))
        } else {
            // else block or single statement
            Some(parse_block_or_statement(parser)?)
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

    // Parse body (block or single statement)
    let body = parse_block_or_statement(parser)?;

    let span = parser.combine_spans(&start_span, body.span());

    Ok(Statement::While(WhileStatement {
        condition,
        body,
        span,
    }))
}

/// Parse do-while statement: do { ... } while (condition);
fn parse_do_while_statement(parser: &mut Parser) -> Result<Statement, ParseError> {
    let start_span = parser.current_span();
    parser.expect(Token::Do)?;

    // Parse body (must be a block statement)
    parser.expect(Token::LeftBrace)?;
    let body_block = parse_block_statement(parser)?;
    let body = Box::new(Statement::Block(body_block));

    // Parse while keyword and condition
    parser.expect(Token::While)?;
    parser.expect(Token::LeftParen)?;
    let condition = super::expr::parse_expression(parser)?;
    parser.expect(Token::RightParen)?;

    // Optional semicolon
    if parser.check(&Token::Semicolon) {
        parser.advance();
    }

    let span = parser.combine_spans(&start_span, &parser.current_span());

    Ok(Statement::DoWhile(DoWhileStatement {
        body,
        condition,
        span,
    }))
}

/// Parse for statement (handles both traditional for and for-of)
fn parse_for_statement(parser: &mut Parser) -> Result<Statement, ParseError> {
    let start_span = parser.current_span();
    parser.expect(Token::For)?;
    parser.expect(Token::LeftParen)?;

    // Check if this is a for-of loop or traditional for loop
    // for (let/const pattern of iterable) { }
    // for (pattern of iterable) { }
    // for (init; test; update) { }

    if parser.check(&Token::Semicolon) {
        // for (; ...) - traditional for loop with no init
        parser.advance();
        return parse_traditional_for(parser, start_span, None);
    }

    if parser.check(&Token::Let) || parser.check(&Token::Const) || parser.check(&Token::Var) {
        // Could be for-of or traditional for with variable declaration
        let kind = match parser.current() {
            Token::Let => VariableKind::Let,
            Token::Const => VariableKind::Const,
            Token::Var => VariableKind::Var,
            _ => unreachable!(),
        };
        parser.advance();

        let pattern = super::pattern::parse_pattern(parser)?;

        // Check for 'of' keyword - this is a for-of loop
        if parser.check(&Token::Of) {
            parser.advance();
            let decl = VariableDecl {
                kind,
                pattern,
                type_annotation: None,
                initializer: None,
                span: start_span,
            };
            return parse_for_of(parser, start_span, ForOfLeft::VariableDecl(decl));
        }

        // Check for 'in' keyword - this is a for-in loop
        if parser.check(&Token::In) {
            parser.advance();
            let decl = VariableDecl {
                kind,
                pattern,
                type_annotation: None,
                initializer: None,
                span: start_span,
            };
            return parse_for_in(parser, start_span, ForOfLeft::VariableDecl(decl));
        }

        // Otherwise, this is a traditional for loop
        let type_annotation = if parser.check(&Token::Colon) {
            parser.advance();
            Some(super::types::parse_type_annotation(parser)?)
        } else {
            None
        };

        let initializer = if parser.check(&Token::Equal) {
            parser.advance();
            Some(parser.with_disallow_in(super::expr::parse_assignment_expression)?)
        } else {
            None
        };

        let span = *pattern.span();

        let decl = VariableDecl {
            kind,
            pattern,
            type_annotation,
            initializer,
            span,
        };

        parser.expect(Token::Semicolon)?;
        return parse_traditional_for(parser, start_span, Some(ForInit::VariableDecl(decl)));
    }

    // For traditional for loop with expression init OR for-of with existing variable
    // First, check if it's a simple identifier that could be for-of
    if parser.check_identifier_like() {
        // Parse pattern first to check for for-of
        let pattern = super::pattern::parse_pattern(parser)?;

        if parser.check(&Token::Of) {
            parser.advance();
            return parse_for_of(parser, start_span, ForOfLeft::Pattern(pattern));
        }

        if parser.check(&Token::In) {
            parser.advance();
            return parse_for_in(parser, start_span, ForOfLeft::Pattern(pattern));
        }

        // Not a for-of, so this pattern is part of an expression
        // Convert pattern back to expression and continue parsing the full expression
        let base_expr = pattern_to_expression(pattern)?;
        let expr = parse_expression_from_base(parser, base_expr)?;
        parser.expect(Token::Semicolon)?;
        return parse_traditional_for(parser, start_span, Some(ForInit::Expression(expr)));
    }

    // Traditional for loop with non-identifier expression init
    let expr = parser.with_disallow_in(super::expr::parse_expression)?;
    parser.expect(Token::Semicolon)?;
    parse_traditional_for(parser, start_span, Some(ForInit::Expression(expr)))
}

/// Convert a pattern to an expression (only works for simple identifier patterns)
fn pattern_to_expression(pattern: Pattern) -> Result<Expression, ParseError> {
    match pattern {
        Pattern::Identifier(id) => Ok(Expression::Identifier(id)),
        _ => {
            use super::ParseErrorKind;
            Err(ParseError {
                kind: ParseErrorKind::InvalidSyntax {
                    reason: "Cannot use destructuring pattern in for loop initializer expression"
                        .to_string(),
                },
                span: *pattern.span(),
                message: "Invalid for loop initializer".to_string(),
                suggestion: Some("Use a simple identifier or add a semicolon".to_string()),
            })
        }
    }
}

/// Continue parsing an expression starting from a base expression (identifier)
/// This handles assignment expressions like `i = 0`
fn parse_expression_from_base(
    parser: &mut Parser,
    base: Expression,
) -> Result<Expression, ParseError> {
    // Check for assignment operators
    let operator = match parser.current() {
        Token::Equal => Some(AssignmentOperator::Assign),
        Token::PlusEqual => Some(AssignmentOperator::AddAssign),
        Token::MinusEqual => Some(AssignmentOperator::SubAssign),
        Token::StarEqual => Some(AssignmentOperator::MulAssign),
        Token::SlashEqual => Some(AssignmentOperator::DivAssign),
        Token::PercentEqual => Some(AssignmentOperator::ModAssign),
        _ => None,
    };

    if let Some(op) = operator {
        let start_span = *base.span();
        parser.advance();
        let right = parser.with_disallow_in(super::expr::parse_expression)?;
        let span = parser.combine_spans(&start_span, right.span());
        return Ok(Expression::Assignment(AssignmentExpression {
            operator: op,
            left: Box::new(base),
            right: Box::new(right),
            span,
        }));
    }

    // No assignment, just return the base expression
    Ok(base)
}

/// Parse the rest of a traditional for loop after the init part
fn parse_traditional_for(
    parser: &mut Parser,
    start_span: Span,
    init: Option<ForInit>,
) -> Result<Statement, ParseError> {
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

    // Parse body (block or single statement)
    let body = parse_block_or_statement(parser)?;

    let span = parser.combine_spans(&start_span, body.span());

    Ok(Statement::For(ForStatement {
        init,
        test,
        update,
        body,
        span,
    }))
}

/// Parse the rest of a for-of loop after the 'of' keyword
fn parse_for_of(
    parser: &mut Parser,
    start_span: Span,
    left: ForOfLeft,
) -> Result<Statement, ParseError> {
    // Parse the iterable expression
    let right = super::expr::parse_expression(parser)?;
    parser.expect(Token::RightParen)?;

    // Parse body (block or single statement)
    let body = parse_block_or_statement(parser)?;

    let span = parser.combine_spans(&start_span, body.span());

    Ok(Statement::ForOf(ForOfStatement {
        left,
        right,
        body,
        span,
    }))
}

/// Parse for-in loop body (after 'in' keyword has been consumed)
fn parse_for_in(
    parser: &mut Parser,
    start_span: Span,
    left: ForOfLeft,
) -> Result<Statement, ParseError> {
    // Parse the object expression
    let right = super::expr::parse_expression(parser)?;
    parser.expect(Token::RightParen)?;

    // Parse body (block or single statement)
    let body = parse_block_or_statement(parser)?;

    let span = parser.combine_spans(&start_span, body.span());

    Ok(Statement::ForIn(ForInStatement {
        left,
        right,
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
    let value = if parser.can_insert_semicolon_before_current()
        || parser.has_line_terminator_before_current()
    {
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

/// Parse yield statement
fn parse_yield_statement(parser: &mut Parser) -> Result<Statement, ParseError> {
    let start_span = parser.current_span();
    parser.expect(Token::Yield)?;

    // Optional yield value
    let value = if parser.can_insert_semicolon_before_current()
        || parser.has_line_terminator_before_current()
    {
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

    Ok(Statement::Yield(YieldStatement { value, span }))
}

/// Parse an identifier token into an Identifier AST node
fn expect_identifier(parser: &mut Parser) -> Result<Identifier, ParseError> {
    parser.expect_identifier_like()
}

/// Parse break statement
fn parse_break_statement(parser: &mut Parser) -> Result<Statement, ParseError> {
    let start_span = parser.current_span();
    parser.expect(Token::Break)?;

    // Optional label: break myLabel;
    let label = if !parser.has_line_terminator_before_current() && parser.check_identifier_like() {
        Some(expect_identifier(parser)?)
    } else {
        None
    };

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

    // Optional label: continue myLabel;
    let label = if !parser.has_line_terminator_before_current() && parser.check_identifier_like() {
        Some(expect_identifier(parser)?)
    } else {
        None
    };

    // Optional semicolon
    if parser.check(&Token::Semicolon) {
        parser.advance();
    }

    Ok(Statement::Continue(ContinueStatement {
        label,
        span: start_span,
    }))
}

/// Parse labeled statement: label: statement
fn parse_labeled_statement(parser: &mut Parser) -> Result<Statement, ParseError> {
    let start_span = parser.current_span();

    // Parse the label identifier
    let label = expect_identifier(parser)?;

    // Consume the colon
    parser.expect(Token::Colon)?;

    // Parse the body statement
    let body = parse_statement(parser)?;
    let span = parser.combine_spans(&start_span, body.span());

    Ok(Statement::Labeled(LabeledStatement {
        label,
        body: Box::new(body),
        span,
    }))
}

/// Parse throw statement
fn parse_throw_statement(parser: &mut Parser) -> Result<Statement, ParseError> {
    let start_span = parser.current_span();
    parser.expect(Token::Throw)?;

    if parser.has_line_terminator_before_current() || parser.can_insert_semicolon_before_current() {
        return Err(ParseError {
            kind: ParseErrorKind::InvalidSyntax {
                reason: "Illegal newline after throw".to_string(),
            },
            span: parser.current_span(),
            message: "Line terminator is not allowed immediately after 'throw'".to_string(),
            suggestion: None,
        });
    }

    // Required expression
    let value = super::expr::parse_expression(parser)?;

    // Optional semicolon
    if parser.check(&Token::Semicolon) {
        parser.advance();
    }

    let span = parser.combine_spans(&start_span, value.span());

    Ok(Statement::Throw(ThrowStatement { value, span }))
}

// ============================================================================
// Type Declarations
// ============================================================================

/// Parse type alias declaration: type Foo = SomeType; or type Bar<T> = GenericType<T>;
/// Accepts pre-parsed annotations for JSON field mapping support.
fn parse_type_alias_declaration(
    parser: &mut Parser,
    annotations: Vec<Annotation>,
) -> Result<Statement, ParseError> {
    let start_span = parser.current_span();
    parser.expect(Token::Type)?;

    // Parse type name
    let name = if let Token::Identifier(name) = parser.current() {
        let name_str = *name;
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

    // Expect '='
    parser.expect(Token::Equal)?;

    // Parse the type annotation
    let type_annotation = super::types::parse_type_annotation(parser)?;

    // Optional semicolon
    if parser.check(&Token::Semicolon) {
        parser.advance();
    }

    let span = parser.combine_spans(&start_span, &type_annotation.span);

    Ok(Statement::TypeAliasDecl(TypeAliasDecl {
        name,
        type_params,
        type_annotation,
        annotations,
        span,
    }))
}

/// Parse interface declaration by lowering it into a type alias.
/// Example:
///   interface A extends B, C { x: number }
/// becomes:
///   type A = B & C & { x: number }
fn parse_interface_declaration(
    parser: &mut Parser,
    annotations: Vec<Annotation>,
) -> Result<Statement, ParseError> {
    let start_span = parser.current_span();
    parser.expect(Token::Interface)?;

    let name = if let Token::Identifier(name) = parser.current() {
        let ident = Identifier {
            name: *name,
            span: parser.current_span(),
        };
        parser.advance();
        ident
    } else {
        return Err(parser.unexpected_token(&[Token::Identifier(Symbol::dummy())]));
    };

    let type_params = if parser.check(&Token::Less) {
        parser.advance();
        Some(parse_type_parameters(parser)?)
    } else {
        None
    };

    let mut parts: Vec<TypeAnnotation> = Vec::new();
    if parser.check(&Token::Extends) {
        parser.advance();
        let mut guard = super::guards::LoopGuard::new("interface_extends");
        loop {
            guard.check()?;
            parts.push(super::types::parse_type_annotation(parser)?);
            if parser.check(&Token::Comma) {
                parser.advance();
                continue;
            }
            break;
        }
    }

    // Interface body uses object-type member syntax.
    let body = super::types::parse_type_annotation(parser)?;
    parts.push(body.clone());

    let type_annotation = if parts.len() == 1 {
        body
    } else {
        let span = parser.combine_spans(&parts[0].span, &parts[parts.len() - 1].span);
        TypeAnnotation {
            ty: Type::Intersection(IntersectionType { types: parts }),
            span,
        }
    };

    if parser.check(&Token::Semicolon) {
        parser.advance();
    }

    let span = parser.combine_spans(&start_span, &type_annotation.span);
    Ok(Statement::TypeAliasDecl(TypeAliasDecl {
        name,
        type_params,
        type_annotation,
        annotations,
        span,
    }))
}

// ============================================================================
// Switch Statement
// ============================================================================

/// Parse switch statement: switch (expr) { case value: ...; default: ...; }
fn parse_switch_statement(parser: &mut Parser) -> Result<Statement, ParseError> {
    let start_span = parser.current_span();
    parser.expect(Token::Switch)?;

    // Parse discriminant expression
    parser.expect(Token::LeftParen)?;
    let discriminant = super::expr::parse_expression(parser)?;
    parser.expect(Token::RightParen)?;

    // Parse cases
    parser.expect(Token::LeftBrace)?;

    let mut cases = Vec::new();
    let mut guard = super::guards::LoopGuard::new("switch_cases");

    while !parser.check(&Token::RightBrace) && !parser.at_eof() {
        guard.check()?;

        let case_start = parser.current_span();

        let test = if parser.check(&Token::Case) {
            parser.advance();
            // Parse the case test expression
            let test_expr = super::expr::parse_expression(parser)?;
            parser.expect(Token::Colon)?;
            Some(test_expr)
        } else if parser.check(&Token::Default) {
            parser.advance();
            parser.expect(Token::Colon)?;
            None
        } else {
            return Err(parser.unexpected_token(&[Token::Case, Token::Default]));
        };

        // Parse consequent statements until next case/default/end
        let mut consequent = Vec::new();
        let mut consequent_guard = super::guards::LoopGuard::new("switch_case_consequent");

        while !parser.check(&Token::Case)
            && !parser.check(&Token::Default)
            && !parser.check(&Token::RightBrace)
            && !parser.at_eof()
        {
            consequent_guard.check()?;
            consequent.push(parse_switch_case_statement(parser)?);
        }

        let case_end = if let Some(last) = consequent.last() {
            *last.span()
        } else {
            parser.current_span()
        };

        let case_span = parser.combine_spans(&case_start, &case_end);

        cases.push(SwitchCase {
            test,
            consequent,
            span: case_span,
        });
    }

    let end_span = parser.current_span();
    parser.expect(Token::RightBrace)?;

    let span = parser.combine_spans(&start_span, &end_span);

    Ok(Statement::Switch(SwitchStatement {
        discriminant,
        cases,
        span,
    }))
}

/// Parse a statement within a switch case.
///
/// Switch cases allow block statements `case X: { ... }` which is different
/// from the top-level where `{` starts an object literal. This helper function
/// handles that special case.
fn parse_switch_case_statement(parser: &mut Parser) -> Result<Statement, ParseError> {
    // Check for block statement - allowed in switch cases
    if parser.check(&Token::LeftBrace) {
        let start_span = parser.current_span();
        parser.advance();

        let mut statements = Vec::new();
        let mut guard = super::guards::LoopGuard::new("block");

        while !parser.check(&Token::RightBrace) && !parser.at_eof() {
            guard.check()?;
            statements.push(parse_statement(parser)?);
        }

        let end_span = parser.current_span();
        parser.expect(Token::RightBrace)?;
        let span = parser.combine_spans(&start_span, &end_span);

        return Ok(Statement::Block(BlockStatement { statements, span }));
    }

    // Otherwise parse as a normal statement
    parse_statement(parser)
}

// ============================================================================
// Try Statement
// ============================================================================

/// Parse try-catch-finally statement
fn parse_try_statement(parser: &mut Parser) -> Result<Statement, ParseError> {
    let start_span = parser.current_span();
    parser.expect(Token::Try)?;

    // Parse try block
    parser.expect(Token::LeftBrace)?;
    let body = parse_block_statement(parser)?;

    // Parse optional catch clause
    let catch_clause = if parser.check(&Token::Catch) {
        let catch_start = parser.current_span();
        parser.advance();

        // Optional catch parameter
        let param = if parser.check(&Token::LeftParen) {
            parser.advance();
            let pattern = super::pattern::parse_pattern(parser)?;
            parser.expect(Token::RightParen)?;
            Some(pattern)
        } else {
            None
        };

        // Parse catch block
        parser.expect(Token::LeftBrace)?;
        let catch_body = parse_block_statement(parser)?;

        let catch_span = parser.combine_spans(&catch_start, &catch_body.span);

        Some(CatchClause {
            param,
            body: catch_body,
            span: catch_span,
        })
    } else {
        None
    };

    // Parse optional finally clause
    let finally_clause = if parser.check(&Token::Finally) {
        parser.advance();
        parser.expect(Token::LeftBrace)?;
        Some(parse_block_statement(parser)?)
    } else {
        None
    };

    // Must have at least catch or finally
    if catch_clause.is_none() && finally_clause.is_none() {
        use super::ParseErrorKind;
        return Err(ParseError {
            kind: ParseErrorKind::InvalidSyntax {
                reason: "try statement must have a catch or finally clause".to_string(),
            },
            span: start_span,
            message: "Missing catch or finally clause".to_string(),
            suggestion: Some("Add a catch or finally clause: try { } catch (e) { }".to_string()),
        });
    }

    let end_span = if let Some(ref fin) = finally_clause {
        fin.span
    } else if let Some(ref catch) = catch_clause {
        catch.span
    } else {
        body.span
    };

    let span = parser.combine_spans(&start_span, &end_span);

    Ok(Statement::Try(TryStatement {
        body,
        catch_clause,
        finally_clause,
        span,
    }))
}

// ============================================================================
// Class Declaration
// ============================================================================

/// Parse class declaration
fn parse_class_declaration(parser: &mut Parser) -> Result<Statement, ParseError> {
    // Parse annotations (//@@tag)
    let annotations = parse_annotations(parser)?;
    parse_class_declaration_with_annotations(parser, annotations)
}

/// Parse class declaration with pre-parsed annotations
fn parse_class_declaration_with_annotations(
    parser: &mut Parser,
    annotations: Vec<Annotation>,
) -> Result<Statement, ParseError> {
    let start_span = parser.current_span();

    // Parse decorators (@decorator)
    let decorators = parse_decorators(parser)?;

    // Parse 'abstract' modifier
    let is_abstract = if parser.check(&Token::Abstract) {
        parser.advance();
        true
    } else {
        false
    };

    // Parse 'class' keyword
    parser.expect(Token::Class)?;

    // Parse class name
    let name = if let Token::Identifier(name) = parser.current() {
        let name_str = *name;
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

    // Optional extends clause
    let extends = if parser.check(&Token::Extends) {
        parser.advance();
        Some(super::types::parse_type_annotation(parser)?)
    } else {
        None
    };

    // Optional implements clause
    let mut implements = Vec::new();
    if parser.check(&Token::Implements) {
        parser.advance();
        let mut guard = super::guards::LoopGuard::new("implements_clause");
        loop {
            guard.check()?;
            implements.push(super::types::parse_type_annotation(parser)?);
            if parser.check(&Token::Comma) {
                parser.advance();
            } else {
                break;
            }
        }
    }

    // Parse class body
    parser.expect(Token::LeftBrace)?;
    let members = parse_class_members(parser)?;
    let end_span = parser.current_span();
    parser.expect(Token::RightBrace)?;

    let span = parser.combine_spans(&start_span, &end_span);

    Ok(Statement::ClassDecl(ClassDecl {
        decorators,
        annotations,
        is_abstract,
        name,
        type_params,
        extends,
        implements,
        members,
        span,
    }))
}

/// Parse class members (fields, methods, constructor)
pub(super) fn parse_class_members(parser: &mut Parser) -> Result<Vec<ClassMember>, ParseError> {
    let mut members = Vec::new();
    let mut guard = super::guards::LoopGuard::new("class_members");

    while !parser.check(&Token::RightBrace) && !parser.at_eof() {
        guard.check()?;
        members.push(parse_class_member(parser)?);
    }

    Ok(members)
}

/// Parse a single class member
fn parse_class_member(parser: &mut Parser) -> Result<ClassMember, ParseError> {
    let start_span = parser.current_span();

    // Parse annotations (//@@tag)
    let annotations = parse_annotations(parser)?;

    // Parse decorators (@decorator)
    let decorators = parse_decorators(parser)?;

    // Parse visibility modifier (private/protected/public)
    let mut visibility = match parser.current() {
        Token::Private => {
            parser.advance();
            Visibility::Private
        }
        Token::Protected => {
            parser.advance();
            Visibility::Protected
        }
        Token::Public => {
            parser.advance();
            Visibility::Public
        }
        _ => Visibility::Public, // Default is public
    };

    // Parse other modifiers
    let is_abstract = if parser.check(&Token::Abstract) {
        parser.advance();
        true
    } else {
        false
    };

    let is_static = if parser.check(&Token::Static) {
        parser.advance();
        // Static initializer block: static { ... }
        if parser.check(&Token::LeftBrace) {
            parser.advance(); // consume '{'
            let block = parse_block_statement(parser)?;
            return Ok(ClassMember::StaticBlock(block));
        }
        true
    } else {
        false
    };

    let is_readonly = if parser.check(&Token::Readonly) {
        parser.advance();
        true
    } else {
        false
    };

    let is_async = if parser.check(&Token::Async) {
        parser.advance();
        true
    } else {
        false
    };

    // Check for getter/setter: `get name()` or `set name(val)`
    // The `get`/`set` contextual keywords are parsed as identifiers, so check
    // if the current identifier is "get" or "set" and is followed by another identifier
    // (which would be the actual property name, not `:` for type annotations or `(` for calls).
    let mut method_kind = MethodKind::Normal;
    if let Token::Identifier(sym) = parser.current() {
        let name_str = parser.resolve(*sym);
        if (name_str == "get" || name_str == "set")
            && matches!(parser.peek(), Some(Token::Identifier(_)))
        {
            // This is a getter or setter - the next token is the actual property name
            if name_str == "get" {
                method_kind = MethodKind::Getter;
            } else {
                method_kind = MethodKind::Setter;
            }
            parser.advance(); // consume the get/set keyword
        }
    }

    // Parse member name - allow keywords that are valid as method names (like JavaScript/TypeScript)
    // Also handle private identifiers (#name) — strip the # and treat as private
    let name = if let Token::PrivateIdentifier(name) = parser.current() {
        // Private field/method: #name → treated as private, name stored without #
        let name_str = *name;
        let name_span = parser.current_span();
        parser.advance();
        visibility = Visibility::Private;
        Identifier {
            name: name_str,
            span: name_span,
        }
    } else if let Token::Identifier(name) = parser.current() {
        let name_str = *name;
        let name_span = parser.current_span();
        parser.advance();
        Identifier {
            name: name_str,
            span: name_span,
        }
    } else if let Some(kw_name) = keyword_as_property_name(parser.current()) {
        let name_str = parser.intern(kw_name);
        let name_span = parser.current_span();
        parser.advance();
        Identifier {
            name: name_str,
            span: name_span,
        }
    } else {
        return Err(parser.unexpected_token(&[Token::Identifier(Symbol::dummy())]));
    };

    // Check for constructor (identifier named "constructor")
    if parser.resolve(name.name) == "constructor" {
        return parse_constructor(parser, start_span, visibility);
    }

    // Check if this is a method (has type params or parens) or a field
    if parser.check(&Token::Less) || parser.check(&Token::LeftParen) {
        // Method
        let type_params = if parser.check(&Token::Less) {
            parser.advance();
            Some(parse_type_parameters(parser)?)
        } else {
            None
        };

        parser.expect(Token::LeftParen)?;
        let params = parse_function_parameters(parser)?;
        parser.expect(Token::RightParen)?;

        let return_type = if parser.check(&Token::Colon) {
            parser.advance();
            Some(super::types::parse_type_annotation(parser)?)
        } else {
            None
        };

        // Abstract methods have no body
        let body = if is_abstract {
            if parser.check(&Token::Semicolon) {
                parser.advance();
            }
            None
        } else {
            parser.expect(Token::LeftBrace)?;
            Some(parse_block_statement(parser)?)
        };

        let end_span = if let Some(ref b) = body {
            b.span
        } else if let Some(ref rt) = return_type {
            rt.span
        } else {
            parser.current_span()
        };

        let span = parser.combine_spans(&start_span, &end_span);

        Ok(ClassMember::Method(MethodDecl {
            decorators,
            annotations,
            visibility,
            is_abstract,
            kind: method_kind,
            name,
            type_params,
            params,
            return_type,
            body,
            is_static,
            is_async,
            span,
        }))
    } else {
        // Field
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

        if parser.check(&Token::Semicolon) {
            parser.advance();
        }

        let end_span = if let Some(ref init) = initializer {
            *init.span()
        } else if let Some(ref ta) = type_annotation {
            ta.span
        } else {
            name.span
        };

        let span = parser.combine_spans(&start_span, &end_span);

        Ok(ClassMember::Field(FieldDecl {
            decorators,
            annotations,
            visibility,
            name,
            type_annotation,
            initializer,
            is_static,
            is_readonly,
            span,
        }))
    }
}

/// Parse constructor
fn parse_constructor(
    parser: &mut Parser,
    start_span: Span,
    visibility: Visibility,
) -> Result<ClassMember, ParseError> {
    parser.expect(Token::LeftParen)?;
    let params = parse_function_parameters(parser)?;
    parser.expect(Token::RightParen)?;

    parser.expect(Token::LeftBrace)?;
    let body = parse_block_statement(parser)?;

    let span = parser.combine_spans(&start_span, &body.span);

    Ok(ClassMember::Constructor(ConstructorDecl {
        visibility,
        params,
        body,
        span,
    }))
}

// ============================================================================
// Decorator Parsing
// ============================================================================

/// Parse decorators: @name or @name(args)
pub(super) fn parse_decorators(parser: &mut Parser) -> Result<Vec<Decorator>, ParseError> {
    let mut decorators = Vec::new();
    let mut guard = super::guards::LoopGuard::new("decorators");

    while parser.check(&Token::At) {
        guard.check()?;
        decorators.push(parse_decorator(parser)?);
    }

    Ok(decorators)
}

/// Parse a single decorator: @name or @name(args)
fn parse_decorator(parser: &mut Parser) -> Result<Decorator, ParseError> {
    let start_span = parser.current_span();
    parser.expect(Token::At)?;

    // Parse decorator expression: identifier, member access, or call
    let mut expression = if let Token::Identifier(name) = parser.current() {
        let name_sym = *name;
        let ident_span = parser.current_span();
        parser.advance();

        Expression::Identifier(Identifier {
            name: name_sym,
            span: ident_span,
        })
    } else {
        return Err(parser.unexpected_token(&[Token::Identifier(Symbol::dummy())]));
    };

    // Handle member access: @module.decorator
    while parser.check(&Token::Dot) {
        parser.advance();
        if let Token::Identifier(name) = parser.current() {
            let name_sym = *name;
            let member_span = parser.current_span();
            parser.advance();

            let end_span = member_span;
            let span = parser.combine_spans(expression.span(), &end_span);

            expression = Expression::Member(MemberExpression {
                object: Box::new(expression),
                property: Identifier {
                    name: name_sym,
                    span: member_span,
                },
                optional: false,
                span,
            });
        } else {
            return Err(parser.unexpected_token(&[Token::Identifier(Symbol::dummy())]));
        }
    }

    // Check for call(s): @decorator(args) or chained @decorator(args1)(args2)...
    while parser.check(&Token::LeftParen) {
        let call_start = *expression.span();
        parser.advance();

        let mut arguments = Vec::new();
        let mut guard = super::guards::LoopGuard::new("decorator_args");

        if !parser.check(&Token::RightParen) {
            loop {
                guard.check()?;
                arguments.push(super::expr::parse_assignment_expression(parser)?);
                if parser.check(&Token::Comma) {
                    parser.advance();
                } else {
                    break;
                }
            }
        }

        let end_span = parser.current_span();
        parser.expect(Token::RightParen)?;

        let span = parser.combine_spans(&call_start, &end_span);
        expression = Expression::Call(CallExpression {
            callee: Box::new(expression),
            type_args: None,
            arguments,
            optional: false,
            span,
        });
    }

    let span = parser.combine_spans(&start_span, expression.span());
    Ok(Decorator { expression, span })
}

// ============================================================================
// Compiler Annotations
// ============================================================================

/// Parse compiler annotations (//@@tag or //@@tag value)
fn parse_annotations(parser: &mut Parser) -> Result<Vec<Annotation>, ParseError> {
    let mut annotations = Vec::new();
    let mut guard = super::guards::LoopGuard::new("annotations");

    while matches!(parser.current(), Token::Annotation(_)) {
        guard.check()?;
        annotations.push(parse_annotation(parser)?);
    }

    Ok(annotations)
}

/// Parse a single annotation: //@@tag or //@@tag value
fn parse_annotation(parser: &mut Parser) -> Result<Annotation, ParseError> {
    let span = parser.current_span();

    if let Token::Annotation(sym) = parser.current() {
        let content = parser.resolve(*sym).to_string();
        parser.advance();
        Ok(Annotation::from_content(&content, span))
    } else {
        Err(parser.unexpected_token(&[Token::Annotation(Symbol::dummy())]))
    }
}

// ============================================================================
// Import Declaration
// ============================================================================

/// Parse import declaration
fn parse_import_declaration(parser: &mut Parser) -> Result<Statement, ParseError> {
    let start_span = parser.current_span();
    parser.expect(Token::Import)?;

    let mut specifiers = Vec::new();

    // Check for different import forms
    if parser.check(&Token::LeftBrace) {
        // import { foo, bar } from "module"
        parser.advance();
        specifiers = parse_named_imports(parser)?;
        parser.expect(Token::RightBrace)?;
    } else if parser.check(&Token::Star) {
        // import * as foo from "module"
        parser.advance();
        parser.expect(Token::As)?;

        let alias = if let Token::Identifier(name) = parser.current() {
            let name_str = *name;
            let name_span = parser.current_span();
            parser.advance();
            Identifier {
                name: name_str,
                span: name_span,
            }
        } else {
            return Err(parser.unexpected_token(&[Token::Identifier(Symbol::dummy())]));
        };

        specifiers.push(ImportSpecifier::Namespace(alias));
    } else if let Token::Identifier(name) = parser.current() {
        // import foo from "module" (default import)
        let name_str = *name;
        let name_span = parser.current_span();
        parser.advance();

        specifiers.push(ImportSpecifier::Default(Identifier {
            name: name_str,
            span: name_span,
        }));

        // Check for additional named imports: import foo, { bar } from "module"
        if parser.check(&Token::Comma) {
            parser.advance();
            if parser.check(&Token::LeftBrace) {
                parser.advance();
                let mut named = parse_named_imports(parser)?;
                specifiers.append(&mut named);
                parser.expect(Token::RightBrace)?;
            }
        }
    } else {
        return Err(parser.unexpected_token(&[
            Token::LeftBrace,
            Token::Star,
            Token::Identifier(Symbol::dummy()),
        ]));
    }

    // Parse 'from' clause
    parser.expect(Token::From)?;

    // Parse module source
    let source = if let Token::StringLiteral(s) = parser.current() {
        let s_value = *s;
        let s_span = parser.current_span();
        parser.advance();
        StringLiteral {
            value: s_value,
            span: s_span,
        }
    } else {
        return Err(parser.unexpected_token(&[Token::StringLiteral(Symbol::dummy())]));
    };

    // Optional semicolon
    if parser.check(&Token::Semicolon) {
        parser.advance();
    }

    let span = parser.combine_spans(&start_span, &source.span);

    Ok(Statement::ImportDecl(ImportDecl {
        specifiers,
        source,
        span,
    }))
}

/// Parse named imports: foo, bar as baz, qux
fn parse_named_imports(parser: &mut Parser) -> Result<Vec<ImportSpecifier>, ParseError> {
    let mut specifiers = Vec::new();
    let mut guard = super::guards::LoopGuard::new("named_imports");

    while !parser.check(&Token::RightBrace) && !parser.at_eof() {
        guard.check()?;

        let name = if let Token::Identifier(name) = parser.current() {
            let name_str = *name;
            let name_span = parser.current_span();
            parser.advance();
            Identifier {
                name: name_str,
                span: name_span,
            }
        } else {
            return Err(parser.unexpected_token(&[Token::Identifier(Symbol::dummy())]));
        };

        // Optional 'as' alias
        let alias = if parser.check(&Token::As) {
            parser.advance();
            if let Token::Identifier(alias_name) = parser.current() {
                let alias_str = *alias_name;
                let alias_span = parser.current_span();
                parser.advance();
                Some(Identifier {
                    name: alias_str,
                    span: alias_span,
                })
            } else {
                return Err(parser.unexpected_token(&[Token::Identifier(Symbol::dummy())]));
            }
        } else {
            None
        };

        specifiers.push(ImportSpecifier::Named { name, alias });

        if parser.check(&Token::Comma) {
            parser.advance();
        } else {
            break;
        }
    }

    Ok(specifiers)
}

// ============================================================================
// Export Declaration
// ============================================================================

/// Parse export declaration
fn parse_export_declaration(parser: &mut Parser) -> Result<Statement, ParseError> {
    let start_span = parser.current_span();
    parser.expect(Token::Export)?;

    // Check for different export forms
    if parser.check(&Token::Star) {
        // export * from "module"
        parser.advance();
        parser.expect(Token::From)?;

        let source = if let Token::StringLiteral(s) = parser.current() {
            let s_value = *s;
            let s_span = parser.current_span();
            parser.advance();
            StringLiteral {
                value: s_value,
                span: s_span,
            }
        } else {
            return Err(parser.unexpected_token(&[Token::StringLiteral(Symbol::dummy())]));
        };

        if parser.check(&Token::Semicolon) {
            parser.advance();
        }

        let span = parser.combine_spans(&start_span, &source.span);

        Ok(Statement::ExportDecl(ExportDecl::All { source, span }))
    } else if parser.check(&Token::LeftBrace) {
        // export { foo, bar } or export { foo, bar } from "module"
        parser.advance();
        let specifiers = parse_export_specifiers(parser)?;
        parser.expect(Token::RightBrace)?;

        // Optional 'from' clause
        let source = if parser.check(&Token::From) {
            parser.advance();
            if let Token::StringLiteral(s) = parser.current() {
                let s_value = *s;
                let s_span = parser.current_span();
                parser.advance();
                Some(StringLiteral {
                    value: s_value,
                    span: s_span,
                })
            } else {
                return Err(parser.unexpected_token(&[Token::StringLiteral(Symbol::dummy())]));
            }
        } else {
            None
        };

        if parser.check(&Token::Semicolon) {
            parser.advance();
        }

        let end_span = if let Some(ref src) = source {
            src.span
        } else {
            parser.current_span()
        };

        let span = parser.combine_spans(&start_span, &end_span);

        Ok(Statement::ExportDecl(ExportDecl::Named {
            specifiers,
            source,
            span,
        }))
    } else if parser.check(&Token::Default) {
        // export default <expression>;
        parser.advance(); // consume 'default'

        let expr = super::expr::parse_expression(parser)?;

        if parser.check(&Token::Semicolon) {
            parser.advance();
        }

        let span = parser.combine_spans(&start_span, expr.span());

        Ok(Statement::ExportDecl(ExportDecl::Default {
            expression: Box::new(expr),
            span,
        }))
    } else {
        // export const/let/function/class declaration
        let declaration = match parser.current() {
            Token::Let | Token::Const | Token::Var => parse_variable_declaration(parser)?,
            Token::Function | Token::Async => parse_function_declaration(parser)?,
            Token::Class | Token::Abstract => parse_class_declaration(parser)?,
            Token::Type => parse_type_alias_declaration(parser, Vec::new())?,
            Token::Interface => parse_interface_declaration(parser, Vec::new())?,
            _ => {
                return Err(parser.unexpected_token(&[
                    Token::Let,
                    Token::Const,
                    Token::Function,
                    Token::Class,
                    Token::Type,
                    Token::Interface,
                    Token::LeftBrace,
                    Token::Star,
                    Token::Default,
                ]));
            }
        };

        Ok(Statement::ExportDecl(ExportDecl::Declaration(Box::new(
            declaration,
        ))))
    }
}

/// Parse export specifiers: foo, bar as baz, foo as default
fn parse_export_specifiers(parser: &mut Parser) -> Result<Vec<ExportSpecifier>, ParseError> {
    let mut specifiers = Vec::new();
    let mut guard = super::guards::LoopGuard::new("export_specifiers");

    while !parser.check(&Token::RightBrace) && !parser.at_eof() {
        guard.check()?;

        let name = match parser.current() {
            Token::Identifier(name) => {
                let name_str = *name;
                let name_span = parser.current_span();
                parser.advance();
                Identifier {
                    name: name_str,
                    span: name_span,
                }
            }
            Token::Default => {
                let name_span = parser.current_span();
                parser.advance();
                Identifier {
                    name: parser.intern("default"),
                    span: name_span,
                }
            }
            _ => {
                return Err(
                    parser.unexpected_token(&[Token::Identifier(Symbol::dummy()), Token::Default])
                )
            }
        };

        // Optional 'as' alias
        let alias = if parser.check(&Token::As) {
            parser.advance();
            match parser.current() {
                Token::Identifier(alias_name) => {
                    let alias_str = *alias_name;
                    let alias_span = parser.current_span();
                    parser.advance();
                    Some(Identifier {
                        name: alias_str,
                        span: alias_span,
                    })
                }
                Token::Default => {
                    let alias_span = parser.current_span();
                    parser.advance();
                    Some(Identifier {
                        name: parser.intern("default"),
                        span: alias_span,
                    })
                }
                _ => {
                    return Err(parser
                        .unexpected_token(&[Token::Identifier(Symbol::dummy()), Token::Default]))
                }
            }
        } else {
            None
        };

        specifiers.push(ExportSpecifier { name, alias });

        if parser.check(&Token::Comma) {
            parser.advance();
        } else {
            break;
        }
    }

    Ok(specifiers)
}

#[cfg(test)]
mod tests {
    use crate::parser::Parser;

    fn parse(source: &str) -> crate::parser::ast::Module {
        let parser = Parser::new(source).expect("should lex");
        let (module, _interner) = parser.parse().expect("should parse");
        module
    }

    #[test]
    fn test_class_annotation() {
        let source = r#"
//@@json
class User {
    name: string;
}
"#;
        let module = parse(source);
        if let crate::parser::ast::Statement::ClassDecl(class) = &module.statements[0] {
            assert_eq!(class.annotations.len(), 1);
            assert_eq!(class.annotations[0].tag, "json");
            assert!(class.annotations[0].value.is_none());
        } else {
            panic!("Expected ClassDecl");
        }
    }

    #[test]
    fn test_field_annotation_with_value() {
        let source = r#"
class User {
    //@@json user_name
    name: string;
}
"#;
        let module = parse(source);
        if let crate::parser::ast::Statement::ClassDecl(class) = &module.statements[0] {
            if let crate::parser::ast::ClassMember::Field(field) = &class.members[0] {
                assert_eq!(field.annotations.len(), 1);
                assert_eq!(field.annotations[0].tag, "json");
                assert_eq!(field.annotations[0].value.as_deref(), Some("user_name"));
            } else {
                panic!("Expected Field");
            }
        } else {
            panic!("Expected ClassDecl");
        }
    }

    #[test]
    fn test_field_annotation_skip() {
        let source = r#"
class User {
    //@@json -
    password: string;
}
"#;
        let module = parse(source);
        if let crate::parser::ast::Statement::ClassDecl(class) = &module.statements[0] {
            if let crate::parser::ast::ClassMember::Field(field) = &class.members[0] {
                assert_eq!(field.annotations.len(), 1);
                assert_eq!(field.annotations[0].tag, "json");
                assert!(field.annotations[0].is_skip());
            } else {
                panic!("Expected Field");
            }
        } else {
            panic!("Expected ClassDecl");
        }
    }

    #[test]
    fn test_field_annotation_with_options() {
        let source = r#"
class User {
    //@@json age,omitempty
    age: number;
}
"#;
        let module = parse(source);
        if let crate::parser::ast::Statement::ClassDecl(class) = &module.statements[0] {
            if let crate::parser::ast::ClassMember::Field(field) = &class.members[0] {
                assert_eq!(field.annotations.len(), 1);
                assert_eq!(field.annotations[0].tag, "json");
                // Annotation value is the raw text "age,omitempty"
                assert_eq!(field.annotations[0].value.as_deref(), Some("age,omitempty"));
            } else {
                panic!("Expected Field");
            }
        } else {
            panic!("Expected ClassDecl");
        }
    }

    #[test]
    fn test_multiple_annotations() {
        let source = r#"
//@@json
//@@validate
class User {
    //@@json user_name
    //@@validate required
    name: string;
}
"#;
        let module = parse(source);
        if let crate::parser::ast::Statement::ClassDecl(class) = &module.statements[0] {
            assert_eq!(class.annotations.len(), 2);
            assert_eq!(class.annotations[0].tag, "json");
            assert_eq!(class.annotations[1].tag, "validate");

            if let crate::parser::ast::ClassMember::Field(field) = &class.members[0] {
                assert_eq!(field.annotations.len(), 2);
                assert_eq!(field.annotations[0].tag, "json");
                assert_eq!(field.annotations[0].value.as_deref(), Some("user_name"));
                assert_eq!(field.annotations[1].tag, "validate");
                assert_eq!(field.annotations[1].value.as_deref(), Some("required"));
            } else {
                panic!("Expected Field");
            }
        } else {
            panic!("Expected ClassDecl");
        }
    }

    #[test]
    fn test_keyword_method_names() {
        // Keywords like 'type', 'null', 'class' should be allowed as method names
        let source = r#"
class Foo {
    type(): string { return "test"; }
    null(): number { return 0; }
    delete(): void { }
    in(): boolean { return true; }
    class(): void { }
    for(): void { }
    if(): void { }
}
"#;
        let module = parse(source);
        if let crate::parser::ast::Statement::ClassDecl(class) = &module.statements[0] {
            assert_eq!(class.members.len(), 7);
            let names: Vec<_> = class
                .members
                .iter()
                .filter_map(|m| {
                    if let crate::parser::ast::ClassMember::Method(method) = m {
                        Some(method.name.name)
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(names.len(), 7);
        } else {
            panic!("Expected ClassDecl");
        }
    }

    #[test]
    fn test_keyword_field_names() {
        // Keywords should also work as field names
        let source = r#"
class Foo {
    type: string;
    null: number;
    default: boolean;
}
"#;
        let module = parse(source);
        if let crate::parser::ast::Statement::ClassDecl(class) = &module.statements[0] {
            assert_eq!(class.members.len(), 3);
        } else {
            panic!("Expected ClassDecl");
        }
    }

    #[test]
    fn test_export_specifier_alias_to_default() {
        let source = r#"
const EventEmitterShim = EventEmitter;
export { EventEmitterShim as default, default as EventEmitter };
"#;
        let parser = Parser::new(source).expect("should lex");
        let (module, interner) = parser.parse().expect("should parse");
        assert_eq!(module.statements.len(), 2);
        let crate::parser::ast::Statement::ExportDecl(crate::parser::ast::ExportDecl::Named {
            specifiers,
            ..
        }) = &module.statements[1]
        else {
            panic!("Expected named export declaration");
        };

        assert_eq!(specifiers.len(), 2);
        assert_eq!(
            interner.resolve(specifiers[0].alias.as_ref().unwrap().name),
            "default"
        );
        assert_eq!(interner.resolve(specifiers[1].name.name), "default");
        assert_eq!(
            interner.resolve(specifiers[1].alias.as_ref().unwrap().name),
            "EventEmitter"
        );
    }

    #[test]
    fn test_return_line_terminator_uses_asi() {
        let module = parse("return\n42;");
        match &module.statements[0] {
            crate::parser::ast::Statement::Return(ret) => {
                assert!(ret.value.is_none());
            }
            _ => panic!("Expected Return"),
        }
    }

    #[test]
    fn test_break_line_terminator_drops_label() {
        let module = parse("break\nlabel;");
        match &module.statements[0] {
            crate::parser::ast::Statement::Break(stmt) => {
                assert!(stmt.label.is_none());
            }
            _ => panic!("Expected Break"),
        }
    }

    #[test]
    fn test_continue_line_terminator_drops_label() {
        let module = parse("continue\nlabel;");
        match &module.statements[0] {
            crate::parser::ast::Statement::Continue(stmt) => {
                assert!(stmt.label.is_none());
            }
            _ => panic!("Expected Continue"),
        }
    }

    #[test]
    fn test_throw_line_terminator_is_parse_error() {
        let parser = Parser::new("throw\nerr;").expect("should lex");
        let result = parser.parse();
        assert!(result.is_err(), "throw newline should be rejected");
    }
}
