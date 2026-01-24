//! Expression parsing

use super::{ParseError, ParseErrorKind, Parser};
use crate::ast::*;
use crate::token::{Span, Token};
use super::precedence::{get_precedence, is_right_associative, Precedence};

/// Parse an expression (entry point).
pub fn parse_expression(parser: &mut Parser) -> Result<Expression, ParseError> {
    parse_expression_with_precedence(parser, Precedence::None)
}

/// Parse an expression with precedence climbing.
///
/// Standard precedence climbing algorithm:
/// - Parse left operand
/// - While next operator has precedence >= min_precedence:
///   - Consume operator
///   - Parse right operand with higher precedence (for left-assoc) or same (for right-assoc)
///   - Combine into binary expression
fn parse_expression_with_precedence(
    parser: &mut Parser,
    min_precedence: Precedence,
) -> Result<Expression, ParseError> {
    let mut left = parse_prefix(parser)?;

    loop {
        let current_precedence = get_precedence(parser.current());

        // Standard precedence climbing: continue while current_prec >= min_prec
        // Special case: allow postfix operators through even if precedence is None
        // (they will be handled in parse_infix -> parse_postfix)
        let is_postfix = matches!(
            parser.current(),
            Token::LeftParen | Token::Dot | Token::QuestionDot | Token::LeftBracket
                | Token::PlusPlus | Token::MinusMinus
        );

        if !is_postfix && (current_precedence == Precedence::None || current_precedence < min_precedence) {
            break;
        }

        left = parse_infix(parser, left, current_precedence)?;
    }

    Ok(left)
}

/// Parse a prefix expression (unary operators and primary expressions).
fn parse_prefix(parser: &mut Parser) -> Result<Expression, ParseError> {
    let start_span = parser.current_span();

    match parser.current() {
        // Unary operators
        Token::Bang | Token::Minus | Token::Plus | Token::Tilde => {
            let op_token = parser.advance();
            let operator = match op_token {
                Token::Bang => UnaryOperator::Not,
                Token::Minus => UnaryOperator::Minus,
                Token::Plus => UnaryOperator::Plus,
                Token::Tilde => UnaryOperator::BitwiseNot,
                _ => unreachable!(),
            };
            let operand = parse_expression_with_precedence(parser, Precedence::Unary)?;
            let span = parser.combine_spans(&start_span, operand.span());
            Ok(Expression::Unary(UnaryExpression {
                operator,
                operand: Box::new(operand),
                span,
            }))
        }

        // Prefix increment/decrement
        Token::PlusPlus | Token::MinusMinus => {
            let op_token = parser.advance();
            let operator = match op_token {
                Token::PlusPlus => UnaryOperator::PrefixIncrement,
                Token::MinusMinus => UnaryOperator::PrefixDecrement,
                _ => unreachable!(),
            };
            let operand = parse_expression_with_precedence(parser, Precedence::Unary)?;
            let span = parser.combine_spans(&start_span, operand.span());
            Ok(Expression::Unary(UnaryExpression {
                operator,
                operand: Box::new(operand),
                span,
            }))
        }

        // typeof expression
        Token::Typeof => {
            parser.advance();
            let argument = parse_expression_with_precedence(parser, Precedence::Unary)?;
            let span = parser.combine_spans(&start_span, argument.span());
            Ok(Expression::Typeof(TypeofExpression {
                argument: Box::new(argument),
                span,
            }))
        }

        // await expression
        Token::Await => {
            parser.advance();
            let argument = parse_expression_with_precedence(parser, Precedence::Unary)?;
            let span = parser.combine_spans(&start_span, argument.span());
            Ok(Expression::Await(AwaitExpression {
                argument: Box::new(argument),
                span,
            }))
        }

        // async call expression: async foo()
        // This wraps any function call in a Task, converting non-async calls to async
        Token::Async => {
            parser.advance();

            // Parse the function call expression
            let callee = parse_expression_with_precedence(parser, Precedence::Member)?;

            // Expect a call (function call, member call, etc)
            // The postfix parsing will handle the actual call syntax
            // But we need to ensure it's a call expression
            match &callee {
                Expression::Call(call_expr) => {
                    // Extract the parts from the CallExpression
                    let span = parser.combine_spans(&start_span, &call_expr.span);
                    Ok(Expression::AsyncCall(AsyncCallExpression {
                        callee: call_expr.callee.clone(),
                        type_args: call_expr.type_args.clone(),
                        arguments: call_expr.arguments.clone(),
                        span,
                    }))
                }
                _ => {
                    // Error: async must be followed by a function call
                    Err(ParseError {
                        kind: ParseErrorKind::InvalidSyntax {
                            reason: "async keyword must be followed by a function call".to_string(),
                        },
                        span: start_span,
                        message: "Expected function call after async".to_string(),
                        suggestion: Some("Use: async foo()".to_string()),
                    })
                }
            }
        }

        // delete and void (TODO: not yet implemented)
        Token::Delete | Token::Void => {
            return Err(ParseError {
                kind: ParseErrorKind::InvalidSyntax {
                    reason: "delete and void operators not yet implemented".to_string(),
                },
                span: start_span,
                message: "delete/void not supported yet".to_string(),
                suggestion: None,
            });
        }

        // new operator
        Token::New => {
            parser.advance();
            let callee = parse_expression_with_precedence(parser, Precedence::Member)?;

            let arguments = if parser.check(&Token::LeftParen) {
                parser.advance();
                parse_arguments(parser)?
            } else {
                vec![]
            };

            let span = if let Some(last_arg) = arguments.last() {
                parser.combine_spans(&start_span, last_arg.span())
            } else {
                parser.combine_spans(&start_span, callee.span())
            };

            Ok(Expression::New(NewExpression {
                callee: Box::new(callee),
                type_args: None,
                arguments,
                span,
            }))
        }

        // Primary expressions
        _ => parse_primary(parser),
    }
}

/// Parse an infix (binary) expression.
fn parse_infix(
    parser: &mut Parser,
    left: Expression,
    precedence: Precedence,
) -> Result<Expression, ParseError> {
    let start_span = *left.span();

    // Check for assignment operators
    if matches!(
        parser.current(),
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
    ) {
        let op_token = parser.advance();
        let operator = match op_token {
            Token::Equal => AssignmentOperator::Assign,
            Token::PlusEqual => AssignmentOperator::AddAssign,
            Token::MinusEqual => AssignmentOperator::SubAssign,
            Token::StarEqual => AssignmentOperator::MulAssign,
            Token::SlashEqual => AssignmentOperator::DivAssign,
            Token::PercentEqual => AssignmentOperator::ModAssign,
            Token::AmpEqual => AssignmentOperator::AndAssign,
            Token::PipeEqual => AssignmentOperator::OrAssign,
            Token::CaretEqual => AssignmentOperator::XorAssign,
            Token::LessLessEqual => AssignmentOperator::LeftShiftAssign,
            Token::GreaterGreaterEqual => AssignmentOperator::RightShiftAssign,
            Token::GreaterGreaterGreaterEqual => AssignmentOperator::UnsignedRightShiftAssign,
            _ => unreachable!(),
        };

        let right = parse_expression_with_precedence(parser, Precedence::Assignment)?;
        let span = parser.combine_spans(&start_span, right.span());

        return Ok(Expression::Assignment(AssignmentExpression {
            left: Box::new(left),
            operator,
            right: Box::new(right),
            span,
        }));
    }

    // Ternary conditional (? :)
    if parser.check(&Token::Question) {
        parser.advance();
        let consequent = parse_expression(parser)?;
        parser.expect(Token::Colon)?;
        let alternate = parse_expression_with_precedence(parser, precedence)?;
        let span = parser.combine_spans(&start_span, alternate.span());

        return Ok(Expression::Conditional(ConditionalExpression {
            test: Box::new(left),
            consequent: Box::new(consequent),
            alternate: Box::new(alternate),
            span,
        }));
    }

    // Logical operators (&&, ||, ??)
    if matches!(
        parser.current(),
        Token::AmpAmp | Token::PipePipe | Token::QuestionQuestion
    ) {
        let op_token = parser.current().clone();
        let operator = match op_token {
            Token::AmpAmp => LogicalOperator::And,
            Token::PipePipe => LogicalOperator::Or,
            Token::QuestionQuestion => LogicalOperator::NullishCoalescing,
            _ => unreachable!(),
        };

        parser.advance();

        // Standard precedence climbing (same as binary operators)
        let next_precedence = if is_right_associative(&op_token) {
            // Right-associative: allow same precedence on right
            precedence
        } else {
            // Left-associative: require higher precedence on right
            Precedence::from(precedence as u8 + 1)
        };

        let right = parse_expression_with_precedence(parser, next_precedence)?;
        let span = parser.combine_spans(&start_span, right.span());

        let logical = Expression::Logical(LogicalExpression {
            left: Box::new(left),
            operator,
            right: Box::new(right),
            span,
        });

        return parse_postfix(parser, logical);
    }

    // Binary operators
    let op_token = parser.current().clone();
    let operator = match op_token {
        Token::Plus => BinaryOperator::Add,
        Token::Minus => BinaryOperator::Subtract,
        Token::Star => BinaryOperator::Multiply,
        Token::Slash => BinaryOperator::Divide,
        Token::Percent => BinaryOperator::Modulo,
        Token::StarStar => BinaryOperator::Exponent,
        Token::EqualEqual => BinaryOperator::Equal,
        Token::BangEqual => BinaryOperator::NotEqual,
        Token::EqualEqualEqual => BinaryOperator::StrictEqual,
        Token::BangEqualEqual => BinaryOperator::StrictNotEqual,
        Token::Less => BinaryOperator::LessThan,
        Token::LessEqual => BinaryOperator::LessEqual,
        Token::Greater => BinaryOperator::GreaterThan,
        Token::GreaterEqual => BinaryOperator::GreaterEqual,
        Token::Amp => BinaryOperator::BitwiseAnd,
        Token::Pipe => BinaryOperator::BitwiseOr,
        Token::Caret => BinaryOperator::BitwiseXor,
        Token::LessLess => BinaryOperator::LeftShift,
        Token::GreaterGreater => BinaryOperator::RightShift,
        Token::GreaterGreaterGreater => BinaryOperator::UnsignedRightShift,
        Token::Instanceof => return parse_postfix(parser, left), // instanceof not in AST
        Token::In => return parse_postfix(parser, left),         // in not in AST
        _ => {
            return parse_postfix(parser, left);
        }
    };

    parser.advance();

    // Standard precedence climbing:
    // For left-associative: use current_prec + 1 (prevent same-level ops from binding on right)
    // For right-associative: use current_prec (allow same-level ops to bind on right)
    let next_min_precedence = if is_right_associative(&op_token) {
        // Right-associative: allow same precedence on right (e.g., 2 ** 3 ** 4 = 2 ** (3 ** 4))
        precedence
    } else {
        // Left-associative: require higher precedence on right (e.g., 1 + 2 + 3 = (1 + 2) + 3)
        Precedence::from((precedence as u8) + 1)
    };

    let right = parse_expression_with_precedence(parser, next_min_precedence)?;
    let span = parser.combine_spans(&start_span, right.span());

    let binary = Expression::Binary(BinaryExpression {
        left: Box::new(left),
        operator,
        right: Box::new(right),
        span,
    });

    parse_postfix(parser, binary)
}

/// Parse postfix expressions (calls, member access, index access, postfix increment/decrement).
fn parse_postfix(parser: &mut Parser, mut expr: Expression) -> Result<Expression, ParseError> {
    loop {
        let start_span = *expr.span();

        match parser.current() {
            // Function call
            Token::LeftParen => {
                parser.advance();
                let arguments = parse_arguments(parser)?;
                let span = parser.combine_spans(&start_span, &parser.current_span());

                expr = Expression::Call(CallExpression {
                    callee: Box::new(expr),
                    type_args: None,
                    arguments,
                    span,
                });
            }

            // Member access (dot notation)
            Token::Dot => {
                parser.advance();
                if let Token::Identifier(name) = parser.current() {
                    let name = name.clone();
                    let end_span = parser.current_span();
                    parser.advance();
                    let span = parser.combine_spans(&start_span, &end_span);

                    expr = Expression::Member(MemberExpression {
                        object: Box::new(expr),
                        property: Identifier {
                            name,
                            span: end_span,
                        },
                        optional: false,
                        span,
                    });
                } else {
                    return Err(ParseError {
                        kind: ParseErrorKind::InvalidSyntax {
                            reason: "Expected property name after '.'".to_string(),
                        },
                        span: parser.current_span(),
                        message: "Expected identifier after '.'".to_string(),
                        suggestion: None,
                    });
                }
            }

            // Optional chaining (?.)
            Token::QuestionDot => {
                parser.advance();
                if let Token::Identifier(name) = parser.current() {
                    let name = name.clone();
                    let end_span = parser.current_span();
                    parser.advance();
                    let span = parser.combine_spans(&start_span, &end_span);

                    expr = Expression::Member(MemberExpression {
                        object: Box::new(expr),
                        property: Identifier {
                            name,
                            span: end_span,
                        },
                        optional: true,
                        span,
                    });
                } else {
                    return Err(ParseError {
                        kind: ParseErrorKind::InvalidSyntax {
                            reason: "Expected property name after '?.'".to_string(),
                        },
                        span: parser.current_span(),
                        message: "Expected identifier after '?.'".to_string(),
                        suggestion: None,
                    });
                }
            }

            // Index access
            Token::LeftBracket => {
                parser.advance();
                let index = parse_expression(parser)?;
                parser.expect(Token::RightBracket)?;
                let span = parser.combine_spans(&start_span, &parser.current_span());

                expr = Expression::Index(IndexExpression {
                    object: Box::new(expr),
                    index: Box::new(index),
                    span,
                });
            }

            // Postfix increment/decrement
            Token::PlusPlus | Token::MinusMinus => {
                let op_token = parser.advance();
                let operator = match op_token {
                    Token::PlusPlus => UnaryOperator::PostfixIncrement,
                    Token::MinusMinus => UnaryOperator::PostfixDecrement,
                    _ => unreachable!(),
                };
                let span = parser.combine_spans(&start_span, &parser.current_span());

                expr = Expression::Unary(UnaryExpression {
                    operator,
                    operand: Box::new(expr),
                    span,
                });
            }

            _ => break,
        }
    }

    Ok(expr)
}

/// Parse a primary expression (literal, identifier, grouped expression, etc.).
pub fn parse_primary(parser: &mut Parser) -> Result<Expression, ParseError> {
    let start_span = parser.current_span();

    match parser.current() {
        // Boolean literals
        Token::True => {
            parser.advance();
            Ok(Expression::BooleanLiteral(BooleanLiteral {
                value: true,
                span: start_span,
            }))
        }

        Token::False => {
            parser.advance();
            Ok(Expression::BooleanLiteral(BooleanLiteral {
                value: false,
                span: start_span,
            }))
        }

        // Null literal
        Token::Null => {
            parser.advance();
            Ok(Expression::NullLiteral(start_span))
        }

        // Integer literal
        Token::IntLiteral(value) => {
            let value = *value;
            parser.advance();
            Ok(Expression::IntLiteral(IntLiteral {
                value,
                span: start_span,
            }))
        }

        // Float literal
        Token::FloatLiteral(value) => {
            let value = *value;
            parser.advance();
            Ok(Expression::FloatLiteral(FloatLiteral {
                value,
                span: start_span,
            }))
        }

        // String literal
        Token::StringLiteral(s) => {
            let value = s.clone();
            parser.advance();
            Ok(Expression::StringLiteral(StringLiteral {
                value,
                span: start_span,
            }))
        }

        // Template literal
        Token::TemplateLiteral(parts) => {
            let parts = convert_template_parts(parts.clone())?;
            parser.advance();
            Ok(Expression::TemplateLiteral(TemplateLiteral {
                parts,
                span: start_span,
            }))
        }

        // Arrow function (simplified - single parameter without parens): x => ...
        Token::Identifier(_) if matches!(parser.peek(), Some(Token::Arrow)) => {
            let param_name = if let Token::Identifier(name) = parser.current() {
                name.clone()
            } else {
                unreachable!()
            };
            parser.advance();
            parser.advance(); // consume =>

            let params = vec![Parameter {
                decorators: vec![],
                pattern: Pattern::Identifier(Identifier {
                    name: param_name,
                    span: start_span,
                }),
                type_annotation: None,
                span: start_span,
            }];

            parse_arrow_function_body(parser, params, start_span)
        }

        // Identifier
        Token::Identifier(name) => {
            let name = name.clone();
            parser.advance();
            Ok(Expression::Identifier(Identifier {
                name,
                span: start_span,
            }))
        }

        // Grouped expression
        Token::LeftParen => {
            parser.advance();

            // Check for arrow function with no parameters: () => ...
            if parser.check(&Token::RightParen) {
                if let Some(Token::Arrow) = parser.peek() {
                    parser.advance(); // consume )
                    parser.advance(); // consume =>
                    return parse_arrow_function_body(parser, vec![], start_span);
                }
            }

            let expr = parse_expression(parser)?;
            parser.expect(Token::RightParen)?;

            // Check for arrow function: (x) => ... or (x, y) => ...
            if parser.check(&Token::Arrow) {
                parser.advance();
                // Convert expression to parameter (simplified - real implementation would be more complex)
                return parse_arrow_function_body(parser, vec![], start_span);
            }

            Ok(expr)
        }

        // Array literal
        Token::LeftBracket => {
            parser.advance();
            let mut elements = Vec::new();

            while !parser.check(&Token::RightBracket) && !parser.at_eof() {
                if parser.check(&Token::Comma) {
                    // Hole in array
                    elements.push(None);
                    parser.advance();
                } else {
                    let elem = parse_expression(parser)?;
                    elements.push(Some(elem));

                    if !parser.check(&Token::RightBracket) {
                        parser.expect(Token::Comma)?;
                    }
                }
            }

            let end_span = parser.current_span();
            parser.expect(Token::RightBracket)?;
            let span = parser.combine_spans(&start_span, &end_span);

            Ok(Expression::Array(ArrayExpression { elements, span }))
        }

        // Object literal
        Token::LeftBrace => {
            parser.advance();
            let mut properties = Vec::new();

            while !parser.check(&Token::RightBrace) && !parser.at_eof() {
                let prop = parse_object_property(parser)?;
                properties.push(prop);

                if !parser.check(&Token::RightBrace) {
                    parser.expect(Token::Comma)?;
                }
            }

            let end_span = parser.current_span();
            parser.expect(Token::RightBrace)?;
            let span = parser.combine_spans(&start_span, &end_span);

            Ok(Expression::Object(ObjectExpression { properties, span }))
        }

        // Function expression (TODO: not in AST yet)
        Token::Function => {
            return Err(ParseError {
                kind: ParseErrorKind::InvalidSyntax {
                    reason: "Function expressions not yet implemented".to_string(),
                },
                span: start_span,
                message: "Function expressions not supported yet".to_string(),
                suggestion: Some("Use arrow functions instead".to_string()),
            });
        }

        _ => Err(ParseError {
            kind: ParseErrorKind::UnexpectedToken {
                expected: vec![
                    Token::Identifier("".to_string()),
                    Token::IntLiteral(0),
                    Token::FloatLiteral(0.0),
                    Token::StringLiteral("".to_string()),
                    Token::True,
                    Token::False,
                    Token::Null,
                    Token::LeftParen,
                    Token::LeftBracket,
                    Token::LeftBrace,
                ],
                found: parser.current().clone(),
            },
            span: parser.current_span(),
            message: format!("Unexpected token {:?}", parser.current()),
            suggestion: None,
        }),
    }
}

/// Parse function call arguments.
fn parse_arguments(parser: &mut Parser) -> Result<Vec<Expression>, ParseError> {
    let mut arguments = Vec::new();

    while !parser.check(&Token::RightParen) && !parser.at_eof() {
        let arg = parse_expression(parser)?;
        arguments.push(arg);

        if !parser.check(&Token::RightParen) {
            parser.expect(Token::Comma)?;
        }
    }

    parser.expect(Token::RightParen)?;
    Ok(arguments)
}

/// Parse an object property (simplified).
fn parse_object_property(parser: &mut Parser) -> Result<ObjectProperty, ParseError> {
    let start_span = parser.current_span();

    // TODO: Handle spread properties: { ...obj }
    // For now, spread properties are not supported

    // Property key
    let key = if let Token::Identifier(name) = parser.current() {
        let name = name.clone();
        parser.advance();
        PropertyKey::Identifier(Identifier {
            name,
            span: start_span,
        })
    } else if let Token::StringLiteral(s) = parser.current() {
        let s = s.clone();
        parser.advance();
        PropertyKey::StringLiteral(StringLiteral {
            value: s,
            span: start_span,
        })
    } else if let Token::IntLiteral(n) = parser.current() {
        let n = *n;
        parser.advance();
        PropertyKey::IntLiteral(IntLiteral {
            value: n,
            span: start_span,
        })
    } else if parser.check(&Token::LeftBracket) {
        // Computed property: [expr]: value
        parser.advance();
        let expr = parse_expression(parser)?;
        parser.expect(Token::RightBracket)?;

        // For now, we need to handle computed properties
        // TODO: This is a simplification - real implementation needs computed property support
        return Err(ParseError {
            kind: ParseErrorKind::InvalidSyntax {
                reason: "Computed properties not yet supported".to_string(),
            },
            span: parser.current_span(),
            message: "Computed properties not implemented".to_string(),
            suggestion: None,
        });
    } else {
        return Err(parser.unexpected_token(&[
            Token::Identifier("".to_string()),
            Token::StringLiteral("".to_string()),
        ]));
    };

    // Check for shorthand property: { x } instead of { x: x }
    if parser.check(&Token::Comma) || parser.check(&Token::RightBrace) {
        // Shorthand is only valid for identifiers
        let value = if let PropertyKey::Identifier(ref ident) = key {
            Expression::Identifier(ident.clone())
        } else {
            return Err(ParseError {
                kind: ParseErrorKind::InvalidSyntax {
                    reason: "Shorthand properties must use identifiers".to_string(),
                },
                span: start_span,
                message: "Invalid shorthand property".to_string(),
                suggestion: None,
            });
        };

        let span = parser.combine_spans(&start_span, value.span());
        return Ok(ObjectProperty::Property(Property {
            key,
            value,
            span,
        }));
    }

    parser.expect(Token::Colon)?;
    let value = parse_expression(parser)?;
    let span = parser.combine_spans(&start_span, value.span());

    Ok(ObjectProperty::Property(Property {
        key,
        value,
        span,
    }))
}

/// Convert token template parts to AST template parts.
fn convert_template_parts(
    token_parts: Vec<crate::token::TemplatePart>,
) -> Result<Vec<TemplatePart>, ParseError> {
    let mut result = Vec::new();

    for part in token_parts {
        match part {
            crate::token::TemplatePart::String(s) => {
                result.push(TemplatePart::String(s));
            }
            crate::token::TemplatePart::Expression(_tokens) => {
                // TODO: Parse the token sequence into an expression
                // For now, return an error
                return Err(ParseError {
                    kind: ParseErrorKind::InvalidSyntax {
                        reason: "Template literal expressions not yet implemented".to_string(),
                    },
                    span: Span::new(0, 0, 0, 0),
                    message: "Template expressions not supported yet".to_string(),
                    suggestion: None,
                });
            }
        }
    }

    Ok(result)
}

/// Parse arrow function body.
fn parse_arrow_function_body(
    parser: &mut Parser,
    params: Vec<Parameter>,
    start_span: Span,
) -> Result<Expression, ParseError> {
    let body = if parser.check(&Token::LeftBrace) {
        parser.advance();
        ArrowBody::Block(parse_block_statement(parser)?)
    } else {
        // Expression body
        let expr = parse_expression(parser)?;
        ArrowBody::Expression(Box::new(expr))
    };

    let body_span = match &body {
        ArrowBody::Expression(expr) => expr.span(),
        ArrowBody::Block(block) => &block.span,
    };

    let span = parser.combine_spans(&start_span, body_span);

    Ok(Expression::Arrow(ArrowFunction {
        params,
        return_type: None,
        body,
        is_async: false,
        span,
    }))
}

/// Parse parameter list (simplified stub - will be implemented in pattern parsing).
fn parse_parameter_list(parser: &mut Parser) -> Result<Vec<Parameter>, ParseError> {
    let mut params = Vec::new();

    while !parser.check(&Token::RightParen) && !parser.at_eof() {
        let start_span = parser.current_span();

        if let Token::Identifier(name) = parser.current() {
            let name = name.clone();
            parser.advance();

            let type_annotation = if parser.check(&Token::Colon) {
                parser.advance();
                Some(super::types::parse_type_annotation(parser)?)
            } else {
                None
            };

            params.push(Parameter {
                decorators: vec![],
                pattern: Pattern::Identifier(Identifier {
                    name,
                    span: start_span,
                }),
                type_annotation,
                span: start_span,
            });
        } else {
            return Err(parser.unexpected_token(&[Token::Identifier("".to_string())]));
        }

        if !parser.check(&Token::RightParen) {
            parser.expect(Token::Comma)?;
        }
    }

    Ok(params)
}

/// Parse block statement for function bodies and control flow constructs.
/// NOTE: BlockStatement is NOT a standalone statement in Raya - it's only used
/// as part of functions, if/while/for/try statements, and arrow function bodies.
fn parse_block_statement(parser: &mut Parser) -> Result<BlockStatement, ParseError> {
    let start_span = parser.current_span();
    let mut statements = Vec::new();

    while !parser.check(&Token::RightBrace) && !parser.at_eof() {
        let stmt = super::stmt::parse_statement(parser)?;
        statements.push(stmt);
    }

    let end_span = parser.current_span();
    parser.expect(Token::RightBrace)?;
    let span = parser.combine_spans(&start_span, &end_span);

    Ok(BlockStatement { statements, span })
}

// Implement From<u8> for Precedence to allow arithmetic
impl From<u8> for Precedence {
    fn from(val: u8) -> Self {
        match val {
            0 => Precedence::None,
            1 => Precedence::Assignment,
            2 => Precedence::Conditional,
            3 => Precedence::NullCoalescing,
            4 => Precedence::LogicalOr,
            5 => Precedence::LogicalAnd,
            6 => Precedence::BitwiseOr,
            7 => Precedence::BitwiseXor,
            8 => Precedence::BitwiseAnd,
            9 => Precedence::Equality,
            10 => Precedence::Relational,
            11 => Precedence::Shift,
            12 => Precedence::Additive,
            13 => Precedence::Multiplicative,
            14 => Precedence::Exponentiation,
            15 => Precedence::Unary,
            16 => Precedence::Postfix,
            17 => Precedence::Call,
            18 => Precedence::Member,
            19 => Precedence::Primary,
            _ => Precedence::None,
        }
    }
}
