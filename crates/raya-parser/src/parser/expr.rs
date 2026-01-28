//! Expression parsing

use super::{ParseError, ParseErrorKind, Parser};
use crate::ast::*;
use crate::interner::Symbol;
use crate::token::{Span, Token};
use super::precedence::{get_precedence, is_right_associative, Precedence};

/// Check if a token is a keyword that can be used as a property/method name.
/// Returns the keyword string for keywords.
fn keyword_as_property_name(token: &Token) -> Option<&'static str> {
    match token {
        // Keywords that can be used as property/method names
        Token::Delete => Some("delete"),
        Token::New => Some("new"),
        Token::This => Some("this"),
        Token::Super => Some("super"),
        Token::Static => Some("static"),
        Token::Abstract => Some("abstract"),
        Token::Extends => Some("extends"),
        Token::Implements => Some("implements"),
        Token::Typeof => Some("typeof"),
        Token::Instanceof => Some("instanceof"),
        Token::As => Some("as"),
        Token::Void => Some("void"),
        Token::Namespace => Some("namespace"),
        Token::Private => Some("private"),
        Token::Protected => Some("protected"),
        Token::Public => Some("public"),
        Token::Yield => Some("yield"),
        Token::In => Some("in"),
        Token::Of => Some("of"),
        Token::From => Some("from"),
        Token::Import => Some("import"),
        Token::Export => Some("export"),
        Token::Default => Some("default"),
        Token::Class => Some("class"),
        Token::Function => Some("function"),
        Token::Return => Some("return"),
        Token::If => Some("if"),
        Token::Else => Some("else"),
        Token::While => Some("while"),
        Token::For => Some("for"),
        Token::Do => Some("do"),
        Token::Break => Some("break"),
        Token::Continue => Some("continue"),
        Token::Switch => Some("switch"),
        Token::Case => Some("case"),
        Token::Try => Some("try"),
        Token::Catch => Some("catch"),
        Token::Finally => Some("finally"),
        Token::Throw => Some("throw"),
        Token::Const => Some("const"),
        Token::Let => Some("let"),
        Token::Type => Some("type"),
        Token::Async => Some("async"),
        Token::Await => Some("await"),
        Token::True => Some("true"),
        Token::False => Some("false"),
        Token::Null => Some("null"),
        Token::Debugger => Some("debugger"),
        _ => None,
    }
}

/// Try to get a symbol for a property/method name from the current token.
/// Handles both identifiers and keywords used as property names.
fn get_property_name_symbol(parser: &mut Parser) -> Option<Symbol> {
    match parser.current() {
        Token::Identifier(sym) => Some(*sym),
        token => {
            keyword_as_property_name(token).map(|name| parser.intern(name))
        }
    }
}

/// Parse an expression (entry point).
pub fn parse_expression(parser: &mut Parser) -> Result<Expression, ParseError> {
    // Check depth before entering (manual guard to avoid borrow issues)
    parser.depth += 1;
    if parser.depth > super::guards::MAX_PARSE_DEPTH {
        parser.depth -= 1;
        return Err(ParseError::parser_limit_exceeded(
            format!("Maximum nesting depth ({}) exceeded in expression", super::guards::MAX_PARSE_DEPTH),
            parser.current_span(),
        ));
    }

    let result = parse_expression_with_precedence(parser, Precedence::None);

    parser.depth -= 1;
    result
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
        // Note: Token::Less is included for type arguments in calls (foo<T>())
        let is_postfix = matches!(
            parser.current(),
            Token::LeftParen | Token::Dot | Token::QuestionDot | Token::LeftBracket
                | Token::PlusPlus | Token::MinusMinus | Token::Less
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

        // async - can be:
        // 1. async call expression: async foo()
        // 2. async arrow function: async () => expr or async x => expr
        Token::Async => {
            parser.advance();

            // Check if this is an async arrow function
            // Pattern: async () => ... or async (params) => ... or async x => ...
            if parser.check(&Token::LeftParen) {
                // Could be async () => ... or async (params) => ...
                // Need to look ahead to see if there's a => after the parentheses
                // Save position and try to parse as arrow
                let paren_start = parser.current_span();
                parser.advance(); // consume (

                // Parse parameters
                let params = try_parse_arrow_params(parser)?;

                // Check for closing paren
                parser.expect(Token::RightParen)?;

                // Check for optional return type annotation
                let return_type = if parser.check(&Token::Colon) {
                    parser.advance();
                    Some(super::types::parse_type_annotation(parser)?)
                } else {
                    None
                };

                // Check for arrow
                if parser.check(&Token::Arrow) {
                    parser.advance(); // consume =>

                    // Parse body - could be expression or block
                    let (body, body_span) = if parser.check(&Token::LeftBrace) {
                        let block = parse_block_statement(parser)?;
                        let span = block.span;
                        (crate::ast::ArrowBody::Block(block), span)
                    } else {
                        let expr = parse_expression(parser)?;
                        let span = *expr.span();
                        (crate::ast::ArrowBody::Expression(Box::new(expr)), span)
                    };

                    let span = parser.combine_spans(&start_span, &body_span);
                    return Ok(Expression::Arrow(ArrowFunction {
                        params,
                        return_type,
                        body,
                        is_async: true,
                        span,
                    }));
                } else {
                    // Not an arrow function - this is a parse error since we consumed the parens
                    return Err(ParseError {
                        kind: ParseErrorKind::InvalidSyntax {
                            reason: "Expected => after async function parameters".to_string(),
                        },
                        span: parser.current_span(),
                        message: "Expected arrow (=>) for async arrow function".to_string(),
                        suggestion: Some("Use: async () => expression".to_string()),
                    });
                }
            } else if matches!(parser.current(), Token::Identifier(_)) {
                // Could be: async x => ... or async foo()
                // Look ahead to see if there's a => after the identifier
                let ident_token = parser.current().clone();
                let ident_span = parser.current_span();

                // Peek ahead: if next token is =>, it's an arrow function
                // Otherwise, it's an async call
                parser.advance(); // consume identifier

                if parser.check(&Token::Arrow) {
                    // It's an async arrow function with single parameter
                    parser.advance(); // consume =>

                    // Create parameter from identifier
                    let param_name = if let Token::Identifier(name) = ident_token {
                        name
                    } else {
                        unreachable!()
                    };

                    let param = crate::ast::Parameter {
                        decorators: vec![],
                        pattern: crate::ast::Pattern::Identifier(crate::ast::Identifier {
                            name: param_name,
                            span: ident_span,
                        }),
                        type_annotation: None,
                        default_value: None,
                        span: ident_span,
                    };

                    // Parse body
                    let (body, body_span) = if parser.check(&Token::LeftBrace) {
                        let block = parse_block_statement(parser)?;
                        let span = block.span;
                        (crate::ast::ArrowBody::Block(block), span)
                    } else {
                        let expr = parse_expression(parser)?;
                        let span = *expr.span();
                        (crate::ast::ArrowBody::Expression(Box::new(expr)), span)
                    };

                    let span = parser.combine_spans(&start_span, &body_span);
                    return Ok(Expression::Arrow(ArrowFunction {
                        params: vec![param],
                        return_type: None,
                        body,
                        is_async: true,
                        span,
                    }));
                } else {
                    // Not an arrow - need to "put back" the identifier and parse as call
                    // Since we already advanced, we need to construct the identifier expr
                    // and continue parsing it as a call
                    let ident_name = if let Token::Identifier(name) = ident_token {
                        name
                    } else {
                        unreachable!()
                    };

                    let ident_expr = Expression::Identifier(crate::ast::Identifier {
                        name: ident_name,
                        span: ident_span,
                    });

                    // Continue parsing postfix (call, member access, etc)
                    let callee = parse_postfix(parser, ident_expr)?;

                    // Expect a call
                    match &callee {
                        Expression::Call(call_expr) => {
                            let span = parser.combine_spans(&start_span, &call_expr.span);
                            return Ok(Expression::AsyncCall(AsyncCallExpression {
                                callee: call_expr.callee.clone(),
                                type_args: call_expr.type_args.clone(),
                                arguments: call_expr.arguments.clone(),
                                span,
                            }));
                        }
                        _ => {
                            return Err(ParseError {
                                kind: ParseErrorKind::InvalidSyntax {
                                    reason: "async keyword must be followed by a function call or arrow function".to_string(),
                                },
                                span: start_span,
                                message: "Expected function call or arrow function after async".to_string(),
                                suggestion: Some("Use: async foo() or async () => expr".to_string()),
                            });
                        }
                    }
                }
            } else {
                // Parse the function call expression
                // Use Precedence::Call to stop at binary operators while allowing postfix parsing
                let callee = parse_expression_with_precedence(parser, Precedence::Call)?;

                // Expect a call (function call, member call, etc)
                match &callee {
                    Expression::Call(call_expr) => {
                        let span = parser.combine_spans(&start_span, &call_expr.span);
                        Ok(Expression::AsyncCall(AsyncCallExpression {
                            callee: call_expr.callee.clone(),
                            type_args: call_expr.type_args.clone(),
                            arguments: call_expr.arguments.clone(),
                            span,
                        }))
                    }
                    _ => {
                        Err(ParseError {
                            kind: ParseErrorKind::InvalidSyntax {
                                reason: "async keyword must be followed by a function call or arrow function".to_string(),
                            },
                            span: start_span,
                            message: "Expected function call or arrow function after async".to_string(),
                            suggestion: Some("Use: async foo() or async () => expr".to_string()),
                        })
                    }
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
            // Parse callee - only allow identifiers and member access, not calls
            // The parentheses belong to `new`, not a function call
            let callee = parse_new_callee(parser)?;

            // Try to parse type arguments (e.g., new Map<string, number>())
            let type_args = if parser.check(&Token::Less) {
                // Save position to backtrack if this isn't type arguments
                let checkpoint = parser.checkpoint();
                parser.advance(); // consume '<'
                match super::types::parse_type_arguments(parser) {
                    Ok(args) => {
                        // Type arguments must be followed by '(' for new expression
                        if parser.check(&Token::LeftParen) {
                            Some(args)
                        } else {
                            // Not type args (probably a comparison), backtrack
                            parser.restore(checkpoint);
                            None
                        }
                    }
                    Err(_) => {
                        // Failed to parse type args, backtrack
                        parser.restore(checkpoint);
                        None
                    }
                }
            } else {
                None
            };

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
                type_args,
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

    // Special case: `<` might be type arguments for a call, not less-than operator
    // Try to parse type arguments speculatively
    if matches!(op_token, Token::Less) {
        let checkpoint = parser.checkpoint();
        parser.advance(); // consume '<'

        // Try to parse type arguments
        if let Ok(type_args) = super::types::parse_type_arguments(parser) {
            // Type arguments parsed successfully
            // Now check if followed by '(' for a call
            if parser.check(&Token::LeftParen) {
                parser.advance(); // consume '('
                let arguments = parse_arguments(parser)?;
                let span = parser.combine_spans(&start_span, &parser.current_span());

                let call = Expression::Call(CallExpression {
                    callee: Box::new(left),
                    type_args: Some(type_args),
                    arguments,
                    span,
                });
                return parse_postfix(parser, call);
            }
        }
        // Not a generic call, backtrack and treat as less-than
        parser.restore(checkpoint);
    }

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
        Token::Instanceof => {
            // Parse: expr instanceof TypeName
            parser.advance();
            let type_name = super::types::parse_type_annotation(parser)?;
            let span = parser.combine_spans(&start_span, &type_name.span);
            let instanceof = Expression::InstanceOf(InstanceOfExpression {
                object: Box::new(left),
                type_name,
                span,
            });
            return parse_postfix(parser, instanceof);
        }
        Token::As => {
            // Parse: expr as TypeName
            parser.advance();
            let target_type = super::types::parse_type_annotation(parser)?;
            let span = parser.combine_spans(&start_span, &target_type.span);
            let cast = Expression::TypeCast(TypeCastExpression {
                object: Box::new(left),
                target_type,
                span,
            });
            return parse_postfix(parser, cast);
        }
        Token::In => return parse_postfix(parser, left), // in not in AST
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
                if let Some(name) = get_property_name_symbol(parser) {
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
                if let Some(name) = get_property_name_symbol(parser) {
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

            // Type arguments before call: foo<T>() or foo<T, U>()
            Token::Less => {
                // Speculatively try to parse type arguments
                let checkpoint = parser.checkpoint();
                parser.advance(); // consume '<'

                // Try to parse type arguments
                match super::types::parse_type_arguments(parser) {
                    Ok(type_args) => {
                        // Type arguments parsed successfully
                        // Now check if followed by '(' for a call
                        if parser.check(&Token::LeftParen) {
                            parser.advance(); // consume '('
                            let arguments = parse_arguments(parser)?;
                            let span = parser.combine_spans(&start_span, &parser.current_span());

                            expr = Expression::Call(CallExpression {
                                callee: Box::new(expr),
                                type_args: Some(type_args),
                                arguments,
                                span,
                            });
                        } else {
                            // Not a call, backtrack - this is a comparison operator
                            parser.restore(checkpoint);
                            break;
                        }
                    }
                    Err(_) => {
                        // Failed to parse type arguments, backtrack
                        parser.restore(checkpoint);
                        break;
                    }
                }
            }

            _ => break,
        }
    }

    Ok(expr)
}

/// Parse the callee of a `new` expression.
///
/// This is a restricted form of expression parsing that allows:
/// - Identifiers: `new Foo()`
/// - Member access: `new Foo.Bar()`, `new a.b.c()`
/// - Index access: `new classes[0]()`
/// - Nested new: `new new Foo()()`
///
/// But NOT function calls - the `()` belongs to `new`, not a call expression.
fn parse_new_callee(parser: &mut Parser) -> Result<Expression, ParseError> {
    let start_span = parser.current_span();

    // Start with an identifier (or nested new)
    let mut expr = if parser.check(&Token::New) {
        // Nested new: `new new Foo()`
        parser.advance();
        let inner_callee = parse_new_callee(parser)?;
        let arguments = if parser.check(&Token::LeftParen) {
            parser.advance();
            parse_arguments(parser)?
        } else {
            vec![]
        };
        let span = parser.combine_spans(&start_span, &parser.current_span());
        Expression::New(NewExpression {
            callee: Box::new(inner_callee),
            type_args: None,
            arguments,
            span,
        })
    } else if let Token::Identifier(name) = parser.current() {
        let name = name.clone();
        parser.advance();
        Expression::Identifier(Identifier {
            name,
            span: start_span,
        })
    } else {
        return Err(ParseError {
            kind: ParseErrorKind::UnexpectedToken {
                expected: vec![Token::Identifier(Symbol::dummy())],
                found: parser.current().clone(),
            },
            span: parser.current_span(),
            message: "Expected class name after 'new'".to_string(),
            suggestion: None,
        });
    };

    // Allow member access (dot notation) and index access, but NOT calls
    loop {
        match parser.current() {
            Token::Dot => {
                parser.advance();
                if let Some(name) = get_property_name_symbol(parser) {
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

        // This expression
        Token::This => {
            parser.advance();
            Ok(Expression::This(start_span))
        }

        // Super expression (for parent class access)
        Token::Super => {
            parser.advance();
            Ok(Expression::Super(start_span))
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
            let parts = convert_template_parts(parser, parts.clone())?;
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
                default_value: None,
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

        // Grouped expression or arrow function
        Token::LeftParen => {
            parser.advance();

            // Check for arrow function with no parameters: () => ... or (): type => ...
            if parser.check(&Token::RightParen) {
                parser.advance(); // consume )

                // Check for return type annotation: (): type => ...
                let return_type = if parser.check(&Token::Colon) {
                    parser.advance(); // consume :
                    Some(super::types::parse_type_annotation(parser)?)
                } else {
                    None
                };

                if parser.check(&Token::Arrow) {
                    parser.advance(); // consume =>
                    return parse_arrow_function_body_with_type(parser, vec![], return_type, start_span);
                }

                // Not an arrow function - error (empty parens not valid as expression)
                return Err(ParseError {
                    kind: ParseErrorKind::UnexpectedToken {
                        expected: vec![Token::Arrow],
                        found: parser.current().clone(),
                    },
                    span: parser.current_span(),
                    message: "Expected '=>' after empty parameter list".to_string(),
                    suggestion: None,
                });
            }

            // Use lookahead to determine if this is arrow function parameters or expression
            // Arrow params: (x: type, y) => ... or (x, y): type => ...
            // Expression: (x + y), (x), etc.
            if looks_like_arrow_params(parser) {
                // Parse as arrow function parameters
                let params = try_parse_arrow_params(parser)?;
                parser.expect(Token::RightParen)?;

                // Check for return type annotation
                let return_type = if parser.check(&Token::Colon) {
                    parser.advance(); // consume :
                    Some(super::types::parse_type_annotation(parser)?)
                } else {
                    None
                };

                // Must have arrow
                if parser.check(&Token::Arrow) {
                    parser.advance();
                    return parse_arrow_function_body_with_type(parser, params, return_type, start_span);
                }

                return Err(ParseError {
                    kind: ParseErrorKind::UnexpectedToken {
                        expected: vec![Token::Arrow],
                        found: parser.current().clone(),
                    },
                    span: parser.current_span(),
                    message: "Expected '=>' after parameter list".to_string(),
                    suggestion: None,
                });
            }

            // Parse as regular expression (parentheses just group, don't wrap)
            let expr = parse_expression(parser)?;
            parser.expect(Token::RightParen)?;

            Ok(expr)
        }

        // Array literal: [1, 2, 3], [...arr1, ...arr2]
        Token::LeftBracket => {
            parser.advance();
            let mut elements = Vec::with_capacity(8); // Most arrays < 8 elements
            let mut guard = super::guards::LoopGuard::new("array_elements");

            while !parser.check(&Token::RightBracket) && !parser.at_eof() {
                guard.check()?;
                if parser.check(&Token::Comma) {
                    // Hole in array
                    elements.push(None);
                    parser.advance();
                } else if parser.check(&Token::DotDotDot) {
                    // Spread element: ...arr
                    parser.advance();
                    let expr = parse_expression(parser)?;
                    elements.push(Some(ArrayElement::Spread(expr)));

                    if !parser.check(&Token::RightBracket) {
                        parser.expect(Token::Comma)?;
                    }
                } else {
                    // Regular element
                    let elem = parse_expression(parser)?;
                    elements.push(Some(ArrayElement::Expression(elem)));

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
            let mut properties = Vec::with_capacity(8); // Most objects < 8 properties
            let mut guard = super::guards::LoopGuard::new("object_properties");

            while !parser.check(&Token::RightBrace) && !parser.at_eof() {
                guard.check()?;
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

        // JSX element or fragment: <div>...</div> or <>...</>
        Token::Less if super::jsx::looks_like_jsx(parser) => {
            super::jsx::parse_jsx(parser)
        }

        _ => Err(ParseError {
            kind: ParseErrorKind::UnexpectedToken {
                expected: vec![
                    Token::Identifier(Symbol::dummy()),
                    Token::IntLiteral(0),
                    Token::FloatLiteral(0.0),
                    Token::StringLiteral(Symbol::dummy()),
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
    let mut arguments = Vec::with_capacity(4); // Most calls have < 4 arguments
    let mut guard = super::guards::LoopGuard::new("call_arguments");

    while !parser.check(&Token::RightParen) && !parser.at_eof() {
        guard.check()?;
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

    // Spread property: { ...obj }
    if parser.check(&Token::DotDotDot) {
        parser.advance();
        let argument = parse_expression(parser)?;
        let span = parser.combine_spans(&start_span, &parser.current_span());
        return Ok(ObjectProperty::Spread(SpreadProperty { argument, span }));
    }

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
        PropertyKey::Computed(expr)
    } else {
        return Err(parser.unexpected_token(&[
            Token::Identifier(Symbol::dummy()),
            Token::StringLiteral(Symbol::dummy()),
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
    parser: &Parser,
    token_parts: Vec<crate::token::TemplatePart>,
) -> Result<Vec<TemplatePart>, ParseError> {
    let mut result = Vec::new();

    for part in token_parts {
        match part {
            crate::token::TemplatePart::String(s) => {
                result.push(TemplatePart::String(s));
            }
            crate::token::TemplatePart::Expression(tokens) => {
                // Parse the token sequence into an expression using a sub-parser
                let interner = parser.interner_clone();
                let mut sub_parser = Parser::from_tokens(tokens, interner);
                let expr = sub_parser.parse_single_expression()?;
                result.push(TemplatePart::Expression(Box::new(expr)));
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
    parse_arrow_function_body_with_type(parser, params, None, start_span)
}

/// Parse arrow function body with optional return type.
fn parse_arrow_function_body_with_type(
    parser: &mut Parser,
    params: Vec<Parameter>,
    return_type: Option<TypeAnnotation>,
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
        return_type,
        body,
        is_async: false,
        span,
    }))
}

/// Check if the current position looks like arrow function parameters.
/// Uses lookahead without consuming tokens.
/// Arrow params pattern: identifier followed by `:`, `,`, or `)`
/// Expression pattern: identifier followed by operators (+, -, *, /, etc.)
fn looks_like_arrow_params(parser: &Parser) -> bool {
    // First token must be an identifier
    if !matches!(parser.current(), Token::Identifier(_)) {
        return false;
    }

    // Check what follows the identifier
    match parser.peek() {
        // Type annotation - definitely parameters
        Some(Token::Colon) => true,
        // Comma - multiple params, definitely parameters
        Some(Token::Comma) => true,
        // Closing paren - could be either (x) expr or (x) =>
        // Need more context - we'll parse as expression and handle single-ident case specially
        Some(Token::RightParen) => {
            // Lookahead further: if followed by `:` or `=>`, it's arrow params
            // We can only look one token ahead, so assume it's expression by default
            // The expression parsing will handle (x) correctly
            false
        }
        // Any operator means it's an expression
        _ => false,
    }
}

/// Try to parse arrow function parameters.
/// Returns Err if the content doesn't look like parameters (but is a valid expression).
fn try_parse_arrow_params(parser: &mut Parser) -> Result<Vec<Parameter>, ParseError> {
    let mut params = Vec::with_capacity(4);
    let mut guard = super::guards::LoopGuard::new("arrow_params");

    while !parser.check(&Token::RightParen) && !parser.at_eof() {
        guard.check()?;
        let start_span = parser.current_span();

        // Parameter must start with an identifier
        if let Token::Identifier(name) = parser.current().clone() {
            parser.advance();

            // Optional type annotation
            let type_annotation = if parser.check(&Token::Colon) {
                parser.advance();
                Some(super::types::parse_type_annotation(parser)?)
            } else {
                None
            };

            // Optional default value
            let default_value = if parser.check(&Token::Equal) {
                parser.advance();
                Some(parse_expression(parser)?)
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
                default_value,
                span: start_span,
            });

            // Comma or end
            if parser.check(&Token::Comma) {
                parser.advance();
            } else if !parser.check(&Token::RightParen) {
                // Not a comma and not closing paren - not valid params
                return Err(ParseError {
                    kind: ParseErrorKind::UnexpectedToken {
                        expected: vec![Token::Comma, Token::RightParen],
                        found: parser.current().clone(),
                    },
                    span: parser.current_span(),
                    message: "Expected ',' or ')' in parameter list".to_string(),
                    suggestion: None,
                });
            }
        } else {
            // Doesn't start with identifier - not valid params
            return Err(ParseError {
                kind: ParseErrorKind::UnexpectedToken {
                    expected: vec![Token::Identifier(Symbol::dummy())],
                    found: parser.current().clone(),
                },
                span: parser.current_span(),
                message: "Expected parameter name".to_string(),
                suggestion: None,
            });
        }
    }

    Ok(params)
}

/// Parse parameter list (simplified stub - will be implemented in pattern parsing).
pub(super) fn parse_parameter_list(parser: &mut Parser) -> Result<Vec<Parameter>, ParseError> {
    let mut params = Vec::with_capacity(4); // Most functions have < 4 parameters
    let mut guard = super::guards::LoopGuard::new("function_parameters");

    while !parser.check(&Token::RightParen) && !parser.at_eof() {
        guard.check()?;
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

            // Parse default value if present (e.g., `x: number = 10`)
            let default_value = if parser.check(&Token::Equal) {
                parser.advance();
                Some(parse_expression(parser)?)
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
                default_value,
                span: start_span,
            });
        } else {
            return Err(parser.unexpected_token(&[Token::Identifier(Symbol::dummy())]));
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
    let mut guard = super::guards::LoopGuard::new("block_statements");

    while !parser.check(&Token::RightBrace) && !parser.at_eof() {
        guard.check()?;
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
