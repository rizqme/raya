//! Pattern parsing (for destructuring and parameter bindings)

use super::{ParseError, Parser};
use crate::ast::{ArrayPattern, Identifier, ObjectPattern, ObjectPatternProperty, Pattern};
use crate::token::Token;

/// Parse a pattern (identifier or destructuring).
pub fn parse_pattern(parser: &mut Parser) -> Result<Pattern, ParseError> {
    let start_span = parser.current_span();

    match parser.current() {
        // Array destructuring: [a, b, c]
        Token::LeftBracket => {
            parser.advance();
            let elements = parse_array_pattern_elements(parser)?;
            let end_span = parser.current_span();
            parser.expect(Token::RightBracket)?;
            let span = parser.combine_spans(&start_span, &end_span);

            Ok(Pattern::Array(ArrayPattern { elements, span }))
        }

        // Object destructuring: { x, y }
        Token::LeftBrace => {
            parser.advance();
            let properties = parse_object_pattern_properties(parser)?;
            let end_span = parser.current_span();
            parser.expect(Token::RightBrace)?;
            let span = parser.combine_spans(&start_span, &end_span);

            Ok(Pattern::Object(ObjectPattern { properties, span }))
        }

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
            Token::Identifier("".to_string()),
            Token::LeftBracket,
            Token::LeftBrace,
        ])),
    }
}

/// Parse array pattern elements: a, b, c
fn parse_array_pattern_elements(parser: &mut Parser) -> Result<Vec<Option<Pattern>>, ParseError> {
    let mut elements = Vec::new();

    while !parser.check(&Token::RightBracket) && !parser.at_eof() {
        // Check for hole: [a, , c]
        if parser.check(&Token::Comma) {
            elements.push(None);
            parser.advance();
            continue;
        }

        // Parse pattern element
        let pattern = parse_pattern(parser)?;
        elements.push(Some(pattern));

        // Optional comma
        if !parser.check(&Token::RightBracket) {
            parser.expect(Token::Comma)?;
        }
    }

    Ok(elements)
}

/// Parse object pattern properties: x, y: z
fn parse_object_pattern_properties(
    parser: &mut Parser,
) -> Result<Vec<ObjectPatternProperty>, ParseError> {
    let mut properties = Vec::new();

    while !parser.check(&Token::RightBrace) && !parser.at_eof() {
        let start_span = parser.current_span();

        // Parse key
        let key = if let Token::Identifier(name) = parser.current() {
            let id = Identifier {
                name: name.clone(),
                span: parser.current_span(),
            };
            parser.advance();
            id
        } else {
            return Err(parser.unexpected_token(&[Token::Identifier("".to_string())]));
        };

        // Check for shorthand: { x } vs full: { x: y }
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

        let span = parser.combine_spans(&start_span, value.span());

        properties.push(ObjectPatternProperty { key, value, span });

        // Optional comma or semicolon separator
        if parser.check(&Token::Comma) || parser.check(&Token::Semicolon) {
            parser.advance();
        }
    }

    Ok(properties)
}
