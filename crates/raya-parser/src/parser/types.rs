//! Type annotation parsing

use super::{ParseError, ParseErrorKind, Parser};
use crate::ast::*;
use crate::interner::Symbol;
use crate::token::Token;

/// Parse a type annotation.
/// This is the entry point for parsing any type expression.
pub fn parse_type_annotation(parser: &mut Parser) -> Result<TypeAnnotation, ParseError> {
    parse_union_type(parser)
}

/// Parse a union type (A | B | C) or a single type
fn parse_union_type(parser: &mut Parser) -> Result<TypeAnnotation, ParseError> {
    // Check depth before entering
    parser.depth += 1;
    if parser.depth > super::guards::MAX_PARSE_DEPTH {
        parser.depth -= 1;
        return Err(ParseError::parser_limit_exceeded(
            format!("Maximum nesting depth ({}) exceeded in type annotation", super::guards::MAX_PARSE_DEPTH),
            parser.current_span(),
        ));
    }

    let start_span = parser.current_span();
    let first_type = parse_primary_type(parser)?;

    // Check if this is a union type
    if parser.check(&Token::Pipe) {
        let mut types = vec![first_type];
        let mut guard = super::guards::LoopGuard::new("union_types");

        while parser.check(&Token::Pipe) {
            guard.check()?;
            parser.advance(); // consume |
            let next_type = parse_primary_type(parser)?;
            types.push(next_type);
        }

        let end_span = types.last().unwrap().span.clone();
        let span = parser.combine_spans(&start_span, &end_span);

        let result = TypeAnnotation {
            ty: Type::Union(UnionType { types }),
            span,
        };
        parser.depth -= 1;
        Ok(result)
    } else {
        parser.depth -= 1;
        Ok(first_type)
    }
}

/// Parse a primary type (not a union)
fn parse_primary_type(parser: &mut Parser) -> Result<TypeAnnotation, ParseError> {
    let start_span = parser.current_span();

    let mut base_type = match parser.current() {
        // Void keyword as a type
        Token::Void => {
            parser.advance();
            TypeAnnotation {
                ty: Type::Primitive(PrimitiveType::Void),
                span: start_span,
            }
        }

        // Null keyword as a type
        Token::Null => {
            parser.advance();
            TypeAnnotation {
                ty: Type::Primitive(PrimitiveType::Null),
                span: start_span,
            }
        }

        // Primitive types via identifiers
        Token::Identifier(name) => {
            let name_sym = *name;
            parser.advance();

            match parser.resolve(name_sym) {
                "number" => TypeAnnotation {
                    ty: Type::Primitive(PrimitiveType::Number),
                    span: start_span,
                },
                "string" => TypeAnnotation {
                    ty: Type::Primitive(PrimitiveType::String),
                    span: start_span,
                },
                "boolean" => TypeAnnotation {
                    ty: Type::Primitive(PrimitiveType::Boolean),
                    span: start_span,
                },
                "null" => TypeAnnotation {
                    ty: Type::Primitive(PrimitiveType::Null),
                    span: start_span,
                },
                "void" => TypeAnnotation {
                    ty: Type::Primitive(PrimitiveType::Void),
                    span: start_span,
                },
                // Otherwise, it's a type reference
                _ => {
                    // Check for type arguments
                    let type_args = if parser.check(&Token::Less) {
                        parser.advance();
                        Some(parse_type_arguments(parser)?)
                    } else {
                        None
                    };

                    TypeAnnotation {
                        ty: Type::Reference(TypeReference {
                            name: Identifier {
                                name: name_sym,
                                span: start_span,
                            },
                            type_args,
                        }),
                        span: start_span,
                    }
                }
            }
        }

        // Parenthesized type: (T)
        Token::LeftParen => {
            parser.advance();

            // Check for function type: (x: number) => string
            // vs parenthesized type: (number | string)
            // We distinguish by looking for : after an identifier
            let is_function_type = if let Token::Identifier(_) = parser.current() {
                if let Some(Token::Colon) = parser.peek() {
                    true
                } else if let Some(Token::Comma) = parser.peek() {
                    true
                } else if let Some(Token::RightParen) = parser.peek() {
                    // Could be () => or (T)
                    false
                } else {
                    false
                }
            } else if parser.check(&Token::RightParen) {
                // () - could be empty function type
                true
            } else {
                false
            };

            if is_function_type {
                // Parse as function type
                let params = parse_function_type_params(parser)?;
                parser.expect(Token::RightParen)?;
                parser.expect(Token::Arrow)?;
                let return_type = Box::new(parse_type_annotation(parser)?);
                let end_span = return_type.span.clone();
                let span = parser.combine_spans(&start_span, &end_span);

                TypeAnnotation {
                    ty: Type::Function(FunctionType {
                        params,
                        return_type,
                    }),
                    span,
                }
            } else {
                // Parse as parenthesized type
                let inner = parse_type_annotation(parser)?;
                parser.expect(Token::RightParen)?;
                let span = parser.combine_spans(&start_span, &inner.span);

                TypeAnnotation {
                    ty: Type::Parenthesized(Box::new(inner)),
                    span,
                }
            }
        }

        // Array/Tuple types: [T] or [T, U, V]
        Token::LeftBracket => {
            parser.advance();

            if parser.check(&Token::RightBracket) {
                // Empty array type: []
                // This is actually invalid in TypeScript, but we'll parse it as an error
                return Err(ParseError {
                    kind: ParseErrorKind::InvalidSyntax {
                        reason: "Array type must specify element type".to_string(),
                    },
                    span: start_span,
                    message: "Use T[] for array types".to_string(),
                    suggestion: Some("Example: number[]".to_string()),
                });
            }

            let first_element = parse_type_annotation(parser)?;

            if parser.check(&Token::Comma) {
                // Tuple type: [T, U, V]
                let mut element_types = vec![first_element];
                let mut guard = super::guards::LoopGuard::new("tuple_elements");

                while parser.check(&Token::Comma) {
                    guard.check()?;
                    parser.advance();
                    // Allow trailing comma
                    if parser.check(&Token::RightBracket) {
                        break;
                    }
                    element_types.push(parse_type_annotation(parser)?);
                }

                let end_span = parser.current_span();
                parser.expect(Token::RightBracket)?;
                let span = parser.combine_spans(&start_span, &end_span);

                TypeAnnotation {
                    ty: Type::Tuple(TupleType { element_types }),
                    span,
                }
            } else {
                // Single element - but this is unusual syntax
                // In TypeScript, you'd use T[] not [T]
                // We'll treat it as a tuple with one element
                parser.expect(Token::RightBracket)?;
                let span = parser.combine_spans(&start_span, &first_element.span);

                TypeAnnotation {
                    ty: Type::Tuple(TupleType {
                        element_types: vec![first_element],
                    }),
                    span,
                }
            }
        }

        // Object type: { x: number; y: string }
        Token::LeftBrace => {
            parser.advance();
            let members = parse_object_type_members(parser)?;
            let end_span = parser.current_span();
            parser.expect(Token::RightBrace)?;
            let span = parser.combine_spans(&start_span, &end_span);

            TypeAnnotation {
                ty: Type::Object(ObjectType { members }),
                span,
            }
        }

        // typeof expression
        Token::Typeof => {
            parser.advance();
            let argument = Box::new(super::expr::parse_primary(parser)?);
            let span = parser.combine_spans(&start_span, argument.span());

            TypeAnnotation {
                ty: Type::Typeof(TypeofType { argument }),
                span,
            }
        }

        // String literal type: "foo"
        Token::StringLiteral(s) => {
            let value = *s;
            parser.advance();

            TypeAnnotation {
                ty: Type::StringLiteral(value),
                span: start_span,
            }
        }

        // Number literal type: 42
        Token::IntLiteral(_) => {
            let value = if let Token::IntLiteral(i) = parser.current() {
                *i as f64
            } else {
                unreachable!()
            };
            parser.advance();

            TypeAnnotation {
                ty: Type::NumberLiteral(value),
                span: start_span,
            }
        }

        Token::FloatLiteral(_) => {
            let value = if let Token::FloatLiteral(f) = parser.current() {
                *f
            } else {
                unreachable!()
            };
            parser.advance();

            TypeAnnotation {
                ty: Type::NumberLiteral(value),
                span: start_span,
            }
        }

        // Boolean literal type: true | false
        Token::True => {
            parser.advance();

            TypeAnnotation {
                ty: Type::BooleanLiteral(true),
                span: start_span,
            }
        }

        Token::False => {
            parser.advance();

            TypeAnnotation {
                ty: Type::BooleanLiteral(false),
                span: start_span,
            }
        }

        _ => {
            return Err(parser.unexpected_token(&[
                Token::Identifier(Symbol::dummy()),
                Token::LeftParen,
                Token::LeftBracket,
                Token::LeftBrace,
            ]));
        }
    };

    // Handle postfix array type: T[]
    let mut guard = super::guards::LoopGuard::new("array_type_postfix");
    while parser.check(&Token::LeftBracket) {
        guard.check()?;
        parser.advance();
        parser.expect(Token::RightBracket)?;
        let span = parser.combine_spans(&start_span, &parser.current_span());

        base_type = TypeAnnotation {
            ty: Type::Array(ArrayType {
                element_type: Box::new(base_type),
            }),
            span,
        };
    }

    Ok(base_type)
}

/// Parse type arguments: <T, U, V>
/// Expects the opening `<` to already have been consumed.
pub fn parse_type_arguments(parser: &mut Parser) -> Result<Vec<TypeAnnotation>, ParseError> {
    let mut type_args = Vec::new();
    let mut guard = super::guards::LoopGuard::new("type_arguments");

    while !parser.check(&Token::Greater) && !parser.at_eof() {
        guard.check()?;
        type_args.push(parse_type_annotation(parser)?);

        if !parser.check(&Token::Greater) {
            parser.expect(Token::Comma)?;
        }
    }

    parser.expect(Token::Greater)?;
    Ok(type_args)
}

/// Parse function type parameters: (x: number, y: string)
fn parse_function_type_params(parser: &mut Parser) -> Result<Vec<FunctionTypeParam>, ParseError> {
    let mut params = Vec::new();
    let mut guard = super::guards::LoopGuard::new("function_type_params");

    while !parser.check(&Token::RightParen) && !parser.at_eof() {
        guard.check()?;
        let name = if let Token::Identifier(n) = parser.current() {
            let identifier = Identifier {
                name: n.clone(),
                span: parser.current_span(),
            };
            parser.advance();

            if parser.check(&Token::Colon) {
                parser.advance();
                Some(identifier)
            } else {
                // No colon means this is just a type, not a named parameter
                // Backtrack - treat the identifier as a type reference
                return Err(ParseError {
                    kind: ParseErrorKind::InvalidSyntax {
                        reason: "Function type parameters must have type annotations".to_string(),
                    },
                    span: identifier.span,
                    message: "Expected : after parameter name".to_string(),
                    suggestion: Some("Use (name: type) syntax".to_string()),
                });
            }
        } else {
            None
        };

        let ty = parse_type_annotation(parser)?;

        params.push(FunctionTypeParam { name, ty });

        if !parser.check(&Token::RightParen) {
            parser.expect(Token::Comma)?;
        }
    }

    Ok(params)
}

/// Parse object type members: x: number; y: string;
fn parse_object_type_members(parser: &mut Parser) -> Result<Vec<ObjectTypeMember>, ParseError> {
    let mut members = Vec::new();
    let mut guard = super::guards::LoopGuard::new("object_type_members");

    while !parser.check(&Token::RightBrace) && !parser.at_eof() {
        guard.check()?;
        let start_span = parser.current_span();

        // Parse property/method name
        let name = if let Token::Identifier(n) = parser.current() {
            let id = Identifier {
                name: n.clone(),
                span: parser.current_span(),
            };
            parser.advance();
            id
        } else {
            return Err(parser.unexpected_token(&[Token::Identifier(Symbol::dummy())]));
        };

        // Check for optional marker: x?: number
        let optional = if parser.check(&Token::Question) {
            parser.advance();
            true
        } else {
            false
        };

        parser.expect(Token::Colon)?;

        // Check if this is a method type: foo(): number
        // vs property type: foo: number
        // We need to check if the type is a function type
        if parser.check(&Token::LeftParen) {
            // Method type
            parser.advance();
            let params = parse_function_type_params(parser)?;
            parser.expect(Token::RightParen)?;
            parser.expect(Token::Arrow)?;
            let return_type = parse_type_annotation(parser)?;
            let span = parser.combine_spans(&start_span, &return_type.span);

            members.push(ObjectTypeMember::Method(ObjectTypeMethod {
                name,
                params,
                return_type,
                span,
            }));
        } else {
            // Property type
            let ty = parse_type_annotation(parser)?;
            let span = parser.combine_spans(&start_span, &ty.span);

            members.push(ObjectTypeMember::Property(ObjectTypeProperty {
                name,
                ty,
                optional,
                span,
            }));
        }

        // Optional semicolon or comma separator
        if parser.check(&Token::Semicolon) || parser.check(&Token::Comma) {
            parser.advance();
        }
    }

    Ok(members)
}
