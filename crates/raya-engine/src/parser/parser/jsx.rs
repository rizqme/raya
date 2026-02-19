//! JSX parsing for declarative UI syntax
//!
//! This module implements parsing for JSX elements, fragments, and attributes.
//! JSX is parsed as expressions and compiled to function calls.

use super::{ParseError, Parser};
use crate::parser::ast::*;
use crate::parser::interner::Symbol;
use crate::parser::token::Token;

/// Parse a JSX element or fragment
/// Called when we encounter `<` in expression position
pub fn parse_jsx(parser: &mut Parser) -> Result<Expression, ParseError> {
    let start_span = parser.current_span();
    parser.expect(Token::Less)?;

    // Check for fragment: <>
    if parser.check(&Token::Greater) {
        return parse_jsx_fragment(parser, start_span);
    }

    // Check for closing tag (error case - unexpected </tag>)
    if parser.check(&Token::Slash) {
        return Err(ParseError::invalid_syntax(
            "Unexpected JSX closing tag",
            parser.current_span(),
        ));
    }

    // Parse opening element
    let opening = parse_jsx_opening_element(parser)?;

    // If self-closing, we're done
    if opening.self_closing {
        let span = parser.combine_spans(&start_span, &opening.span);
        return Ok(Expression::JsxElement(JsxElement {
            opening,
            children: vec![],
            closing: None,
            span,
        }));
    }

    // Parse children until we hit closing tag
    let children = parse_jsx_children(parser, &opening.name)?;

    // Parse closing tag
    let closing = parse_jsx_closing_element(parser, &opening.name)?;

    let end_span = closing.span.clone();
    let span = parser.combine_spans(&start_span, &end_span);

    Ok(Expression::JsxElement(JsxElement {
        opening,
        children,
        closing: Some(closing),
        span,
    }))
}

/// Parse JSX fragment: <>children</>
fn parse_jsx_fragment(parser: &mut Parser, start_span: crate::parser::token::Span) -> Result<Expression, ParseError> {
    // Opening: <>
    let opening_span = parser.current_span();
    parser.expect(Token::Greater)?;
    let opening = JsxOpeningFragment {
        span: opening_span,
    };

    // Parse children until we hit </>
    let children = parse_jsx_children_until_fragment_close(parser)?;

    // Closing: </>
    parser.expect(Token::Less)?;
    parser.expect(Token::Slash)?;
    let closing_span = parser.current_span();
    parser.expect(Token::Greater)?;
    let closing = JsxClosingFragment {
        span: closing_span,
    };

    let span = parser.combine_spans(&start_span, &parser.current_span());

    Ok(Expression::JsxFragment(JsxFragment {
        opening,
        children,
        closing,
        span,
    }))
}

/// Parse JSX opening element: <div className="foo"> or <div />
fn parse_jsx_opening_element(parser: &mut Parser) -> Result<JsxOpeningElement, ParseError> {
    let start_span = parser.current_span();

    // Parse element name
    let name = parse_jsx_element_name(parser)?;

    // Parse attributes
    let mut attributes = vec![];
    let mut guard = super::guards::LoopGuard::new("jsx_attributes");
    while !parser.check(&Token::Greater) && !parser.check(&Token::Slash) && !parser.at_eof() {
        guard.check()?;
        attributes.push(parse_jsx_attribute(parser)?);
    }

    // Check for self-closing
    let self_closing = if parser.check(&Token::Slash) {
        parser.advance();
        true
    } else {
        false
    };

    let end_span = parser.current_span();
    parser.expect(Token::Greater)?;

    let span = parser.combine_spans(&start_span, &end_span);

    Ok(JsxOpeningElement {
        name,
        attributes,
        self_closing,
        span,
    })
}

/// Parse JSX closing element: </div>
fn parse_jsx_closing_element(
    parser: &mut Parser,
    expected_name: &JsxElementName,
) -> Result<JsxClosingElement, ParseError> {
    let start_span = parser.current_span();

    parser.expect(Token::Less)?;
    parser.expect(Token::Slash)?;

    let name = parse_jsx_element_name(parser)?;

    // Verify name matches opening tag
    if name.to_string(&parser.interner) != expected_name.to_string(&parser.interner) {
        return Err(ParseError::invalid_syntax(
            format!(
                "Expected closing tag for '{}', found '{}'",
                expected_name.to_string(&parser.interner),
                name.to_string(&parser.interner)
            ),
            parser.current_span(),
        ));
    }

    let end_span = parser.current_span();
    parser.expect(Token::Greater)?;

    let span = parser.combine_spans(&start_span, &end_span);

    Ok(JsxClosingElement { name, span })
}

/// Parse JSX element name: div, Button, Foo.Bar
fn parse_jsx_element_name(parser: &mut Parser) -> Result<JsxElementName, ParseError> {
    if let Token::Identifier(name) = parser.current() {
        let id = Identifier {
            name: name.clone(),
            span: parser.current_span(),
        };
        parser.advance();

        // Check for namespaced name: svg:path
        if parser.check(&Token::Colon) {
            parser.advance();
            if let Token::Identifier(local_name) = parser.current() {
                let local = Identifier {
                    name: local_name.clone(),
                    span: parser.current_span(),
                };
                parser.advance();
                return Ok(JsxElementName::Namespaced {
                    namespace: id,
                    name: local,
                });
            } else {
                return Err(parser.unexpected_token(&[Token::Identifier(Symbol::dummy())]));
            }
        }

        // Check for member expression: Foo.Bar
        let mut result = JsxElementName::Identifier(id);
        while parser.check(&Token::Dot) {
            parser.advance();
            if let Token::Identifier(prop_name) = parser.current() {
                let property = Identifier {
                    name: prop_name.clone(),
                    span: parser.current_span(),
                };
                parser.advance();

                result = JsxElementName::MemberExpression {
                    object: Box::new(result),
                    property,
                };
            } else {
                return Err(parser.unexpected_token(&[Token::Identifier(Symbol::dummy())]));
            }
        }

        Ok(result)
    } else {
        Err(parser.unexpected_token(&[Token::Identifier(Symbol::dummy())]))
    }
}

/// Parse JSX attribute: className="foo" or onClick={handler} or {...props}
fn parse_jsx_attribute(parser: &mut Parser) -> Result<JsxAttribute, ParseError> {
    let start_span = parser.current_span();

    // Check for spread attribute: {...props}
    if parser.check(&Token::LeftBrace) {
        parser.advance();
        if parser.check(&Token::DotDotDot) {
            parser.advance();
            let argument = super::expr::parse_expression(parser)?;
            let end_span = parser.current_span();
            parser.expect(Token::RightBrace)?;
            let span = parser.combine_spans(&start_span, &end_span);
            return Ok(JsxAttribute::Spread { argument, span });
        } else {
            return Err(ParseError::invalid_syntax(
                "Expected spread operator after '{' in JSX attribute",
                parser.current_span(),
            ));
        }
    }

    // Regular attribute: name="value" or name={expr}
    let name = parse_jsx_attribute_name(parser)?;

    // Check for value
    let value = if parser.check(&Token::Equal) {
        parser.advance();
        Some(parse_jsx_attribute_value(parser)?)
    } else {
        // Boolean attribute: <input disabled />
        None
    };

    let span = parser.combine_spans(&start_span, &parser.current_span());

    Ok(JsxAttribute::Attribute { name, value, span })
}

/// Parse JSX attribute name: className, data-value, or xml:lang
fn parse_jsx_attribute_name(parser: &mut Parser) -> Result<JsxAttributeName, ParseError> {
    if let Token::Identifier(name_sym) = parser.current() {
        let start_span = parser.current_span();
        let mut full_name = parser.resolve(*name_sym).to_string();
        parser.advance();

        // Check for hyphenated attribute: data-value, aria-label, etc.
        while parser.check(&Token::Minus) {
            parser.advance();
            if let Token::Identifier(part_sym) = parser.current() {
                full_name.push('-');
                full_name.push_str(parser.resolve(*part_sym));
                parser.advance();
            } else {
                return Err(parser.unexpected_token(&[Token::Identifier(Symbol::dummy())]));
            }
        }

        // Intern the full hyphenated name
        let name = parser.intern(&full_name);
        let id = Identifier {
            name,
            span: start_span,
        };

        // Check for namespaced attribute: xml:lang
        if parser.check(&Token::Colon) {
            parser.advance();
            if let Token::Identifier(local_name) = parser.current() {
                let local_id = Identifier {
                    name: local_name.clone(),
                    span: parser.current_span(),
                };
                parser.advance();
                return Ok(JsxAttributeName::Namespaced {
                    namespace: id,
                    name: local_id,
                });
            } else {
                return Err(parser.unexpected_token(&[Token::Identifier(Symbol::dummy())]));
            }
        }

        Ok(JsxAttributeName::Identifier(id))
    } else {
        Err(parser.unexpected_token(&[Token::Identifier(Symbol::dummy())]))
    }
}

/// Parse JSX attribute value: "string" or {expression}
fn parse_jsx_attribute_value(parser: &mut Parser) -> Result<JsxAttributeValue, ParseError> {
    match parser.current() {
        Token::StringLiteral(s) => {
            let value = StringLiteral {
                value: s.clone(),
                span: parser.current_span(),
            };
            parser.advance();
            Ok(JsxAttributeValue::StringLiteral(value))
        }
        Token::LeftBrace => {
            parser.advance();
            let expr = super::expr::parse_expression(parser)?;
            parser.expect(Token::RightBrace)?;
            Ok(JsxAttributeValue::Expression(expr))
        }
        Token::Less => {
            // Nested JSX element
            let jsx = parse_jsx(parser)?;
            match jsx {
                Expression::JsxElement(elem) => Ok(JsxAttributeValue::JsxElement(Box::new(elem))),
                Expression::JsxFragment(frag) => Ok(JsxAttributeValue::JsxFragment(Box::new(frag))),
                _ => unreachable!(),
            }
        }
        _ => Err(parser.unexpected_token(&[
            Token::StringLiteral(Symbol::dummy()),
            Token::LeftBrace,
            Token::Less,
        ])),
    }
}

/// Parse JSX children until we hit closing tag
fn parse_jsx_children(
    parser: &mut Parser,
    _parent_name: &JsxElementName,
) -> Result<Vec<JsxChild>, ParseError> {
    let mut children = vec![];
    let mut guard = super::guards::LoopGuard::new("jsx_children");

    loop {
        guard.check()?;

        // Check for closing tag
        if parser.check(&Token::Less) {
            // Peek ahead to see if it's a closing tag
            let next_is_slash = parser.peek() == Some(&Token::Slash);
            if next_is_slash {
                break;
            }
        }

        if parser.at_eof() {
            return Err(ParseError::unexpected_eof(
                vec![Token::Less],
                parser.current_span(),
            ));
        }

        children.push(parse_jsx_child(parser)?);
    }

    Ok(children)
}

/// Parse JSX children for fragments until we hit </>
fn parse_jsx_children_until_fragment_close(parser: &mut Parser) -> Result<Vec<JsxChild>, ParseError> {
    let mut children = vec![];
    let mut guard = super::guards::LoopGuard::new("jsx_fragment_children");

    loop {
        guard.check()?;

        // Check for closing fragment: </>
        if parser.check(&Token::Less) {
            let next_is_slash = parser.peek() == Some(&Token::Slash);
            if next_is_slash {
                break;
            }
        }

        if parser.at_eof() {
            return Err(ParseError::unexpected_eof(
                vec![Token::Less],
                parser.current_span(),
            ));
        }

        children.push(parse_jsx_child(parser)?);
    }

    Ok(children)
}

/// Parse a single JSX child: text, {expression}, or <element>
fn parse_jsx_child(parser: &mut Parser) -> Result<JsxChild, ParseError> {
    match parser.current() {
        Token::LeftBrace => {
            // JSX expression: {value}
            let start_span = parser.current_span();
            parser.advance();

            // Check for empty expression: {}
            let expression = if parser.check(&Token::RightBrace) {
                None
            } else {
                Some(super::expr::parse_expression(parser)?)
            };

            let end_span = parser.current_span();
            parser.expect(Token::RightBrace)?;
            let span = parser.combine_spans(&start_span, &end_span);

            Ok(JsxChild::Expression(JsxExpression { expression, span }))
        }
        Token::Less => {
            // Nested element or fragment
            let jsx = parse_jsx(parser)?;
            match jsx {
                Expression::JsxElement(elem) => Ok(JsxChild::Element(elem)),
                Expression::JsxFragment(frag) => Ok(JsxChild::Fragment(frag)),
                _ => unreachable!(),
            }
        }
        _ => {
            // Text content - for now, we'll parse until we hit < or {
            // In a full implementation, this would need proper whitespace handling
            parse_jsx_text(parser)
        }
    }
}

/// Parse JSX text content
/// This is a simplified implementation - a full version would need to handle
/// whitespace properly and use a different lexer mode
fn parse_jsx_text(parser: &mut Parser) -> Result<JsxChild, ParseError> {
    let start_span = parser.current_span();
    let mut text = String::new();
    let mut guard = super::guards::LoopGuard::new("jsx_text");

    // Collect tokens until we hit < or {
    while !parser.check(&Token::Less) && !parser.check(&Token::LeftBrace) && !parser.at_eof() {
        guard.check()?;

        // Extract actual text content from the token
        let token_text = match parser.current() {
            Token::Identifier(sym) => parser.resolve(*sym).to_string(),
            Token::StringLiteral(sym) => parser.resolve(*sym).to_string(),
            Token::IntLiteral(n) => n.to_string(),
            Token::FloatLiteral(n) => n.to_string(),
            other => format!("{}", other),
        };
        text.push_str(&token_text);
        text.push(' ');
        parser.advance();
    }

    let span = parser.combine_spans(&start_span, &parser.current_span());

    Ok(JsxChild::Text(JsxText {
        value: text.trim().to_string(),
        raw: text,
        span,
    }))
}

/// Check if we're looking at the start of a JSX element
/// This helps disambiguate < in expression context
pub fn looks_like_jsx(parser: &Parser) -> bool {
    // JSX starts with < followed by:
    // - An identifier (element name): <div
    // - > (fragment): <>
    // - / (closing tag, but this would be an error in expression position): </
    match parser.peek() {
        Some(Token::Identifier(_)) => true,
        Some(Token::Greater) => true,  // Fragment
        _ => false,
    }
}
