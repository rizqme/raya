//! Pattern parsing (for destructuring and parameter bindings)

use super::{ParseError, Parser};
use crate::parser::ast::{ArrayPattern, Identifier, ObjectPattern, ObjectPatternProperty, Pattern, PatternElement, RestPattern};
use crate::parser::interner::Symbol;
use crate::parser::token::Token;

/// Parse a pattern (identifier or destructuring).
pub fn parse_pattern(parser: &mut Parser) -> Result<Pattern, ParseError> {
    // Check depth before entering (manual guard to avoid borrow issues)
    parser.depth += 1;
    if parser.depth > super::guards::MAX_PARSE_DEPTH {
        parser.depth -= 1;
        return Err(ParseError::parser_limit_exceeded(
            format!("Maximum nesting depth ({}) exceeded in pattern", super::guards::MAX_PARSE_DEPTH),
            parser.current_span(),
        ));
    }

    let start_span = parser.current_span();

    let result = match parser.current() {
        // Rest pattern: ...args (for function parameters)
        Token::DotDotDot => {
            parser.advance();
            let argument = parse_pattern(parser)?;
            let span = parser.combine_spans(&start_span, argument.span());
            Ok(Pattern::Rest(RestPattern {
                argument: Box::new(argument),
                span,
            }))
        }

        // Array destructuring: [a, b, c], [x, ...rest], [y = 10]
        Token::LeftBracket => parse_array_pattern(parser),

        // Object destructuring: { x, y }, { x: newX, y = 0 }, { a, ...rest }
        Token::LeftBrace => parse_object_pattern(parser),

        // Simple identifier: x
        Token::Identifier(name) => {
            let name = name.clone();
            parser.advance();
            Ok(Pattern::Identifier(Identifier {
                name,
                span: start_span,
            }))
        }

        _ => Err(parser.unexpected_token(&[
            Token::Identifier(Symbol::dummy()),
            Token::LeftBracket,
            Token::LeftBrace,
            Token::DotDotDot,
        ])),
    };

    parser.depth -= 1;
    result
}

/// Parse array destructuring pattern: [a, b], [x, , z], [first, ...rest], [y = 10]
fn parse_array_pattern(parser: &mut Parser) -> Result<Pattern, ParseError> {
    let start_span = parser.current_span();
    parser.expect(Token::LeftBracket)?;

    let mut elements = Vec::new();
    let mut rest = None;
    let mut guard = super::guards::LoopGuard::new("array_pattern_elements");

    while !parser.check(&Token::RightBracket) && !parser.at_eof() {
        guard.check()?;

        // Check for hole: [a, , c]
        if parser.check(&Token::Comma) {
            elements.push(None);
            parser.advance();
            continue;
        }

        // Check for rest element: ...rest
        if parser.check(&Token::DotDotDot) {
            parser.advance();
            rest = Some(Box::new(parse_pattern(parser)?));
            // Rest must be last element
            if parser.check(&Token::Comma) {
                parser.advance();
                if !parser.check(&Token::RightBracket) {
                    return Err(ParseError::invalid_syntax(
                        "Rest element must be last in array pattern",
                        parser.current_span(),
                    ));
                }
            }
            break;
        }

        // Parse pattern element
        let elem_start = parser.current_span();
        let pattern = parse_pattern(parser)?;

        // Check for default value: pattern = expr
        let default = if parser.check(&Token::Equal) {
            parser.advance();
            Some(super::expr::parse_expression(parser)?)
        } else {
            None
        };

        let elem_span = parser.combine_spans(&elem_start, &parser.current_span());
        elements.push(Some(PatternElement {
            pattern,
            default,
            span: elem_span,
        }));

        // Optional comma
        if parser.check(&Token::Comma) {
            parser.advance();
        } else if !parser.check(&Token::RightBracket) {
            return Err(parser.unexpected_token(&[Token::Comma, Token::RightBracket]));
        }
    }

    let end_span = parser.current_span();
    parser.expect(Token::RightBracket)?;
    let span = parser.combine_spans(&start_span, &end_span);

    Ok(Pattern::Array(ArrayPattern {
        elements,
        rest,
        span,
    }))
}

/// Parse object destructuring pattern: { x, y }, { x: newX, y = 0 }, { a, ...rest }
fn parse_object_pattern(parser: &mut Parser) -> Result<Pattern, ParseError> {
    let start_span = parser.current_span();
    parser.expect(Token::LeftBrace)?;

    let mut properties = Vec::new();
    let mut rest = None;
    let mut guard = super::guards::LoopGuard::new("object_pattern_properties");

    while !parser.check(&Token::RightBrace) && !parser.at_eof() {
        guard.check()?;

        let prop_start = parser.current_span();

        // Check for rest properties: ...rest
        if parser.check(&Token::DotDotDot) {
            parser.advance();
            if let Token::Identifier(name) = parser.current() {
                rest = Some(Identifier {
                    name: name.clone(),
                    span: parser.current_span(),
                });
                parser.advance();

                // Rest must be last property
                if parser.check(&Token::Comma) {
                    parser.advance();
                    if !parser.check(&Token::RightBrace) {
                        return Err(ParseError::invalid_syntax(
                            "Rest element must be last in object pattern",
                            parser.current_span(),
                        ));
                    }
                }
                break;
            } else {
                return Err(parser.unexpected_token(&[Token::Identifier(Symbol::dummy())]));
            }
        }

        // Parse key
        let key = if let Token::Identifier(name) = parser.current() {
            let id = Identifier {
                name: name.clone(),
                span: parser.current_span(),
            };
            parser.advance();
            id
        } else {
            return Err(parser.unexpected_token(&[Token::Identifier(Symbol::dummy())]));
        };

        // Check for renaming: { x: y }
        let value = if parser.check(&Token::Colon) {
            parser.advance();
            parse_pattern(parser)?
        } else {
            // Shorthand: { x } is equivalent to { x: x }
            Pattern::Identifier(Identifier {
                name: key.name.clone(),
                span: key.span.clone(),
            })
        };

        // Check for default value: { x = 10 } or { x: y = 10 }
        let default = if parser.check(&Token::Equal) {
            parser.advance();
            Some(super::expr::parse_expression(parser)?)
        } else {
            None
        };

        let prop_span = parser.combine_spans(&prop_start, &parser.current_span());

        properties.push(ObjectPatternProperty {
            key,
            value,
            default,
            span: prop_span,
        });

        // Optional comma or semicolon separator
        if parser.check(&Token::Comma) || parser.check(&Token::Semicolon) {
            parser.advance();
        } else if !parser.check(&Token::RightBrace) {
            return Err(parser.unexpected_token(&[Token::Comma, Token::RightBrace]));
        }
    }

    let end_span = parser.current_span();
    parser.expect(Token::RightBrace)?;
    let span = parser.combine_spans(&start_span, &end_span);

    Ok(Pattern::Object(ObjectPattern {
        properties,
        rest,
        span,
    }))
}
