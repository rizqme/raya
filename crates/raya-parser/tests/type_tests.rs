//! Tests for type annotation parsing

use raya_parser::ast::*;
use raya_parser::parser::Parser;

// ===========================================================================
// Primitive Types
// ============================================================================

#[test]
fn test_parse_number_type() {
    let source = "let x: number;";
    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            let ty = decl.type_annotation.as_ref().unwrap();
            assert!(matches!(ty.ty, Type::Primitive(PrimitiveType::Number)));
        }
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_union_type() {
    let source = "let x: number | string;";
    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            let ty = decl.type_annotation.as_ref().unwrap();
            match &ty.ty {
                Type::Union(union_ty) => {
                    assert_eq!(union_ty.types.len(), 2);
                }
                _ => panic!("Expected union type"),
            }
        }
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_array_type() {
    let source = "let x: number[];";
    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            let ty = decl.type_annotation.as_ref().unwrap();
            match &ty.ty {
                Type::Array(array_ty) => {
                    match &array_ty.element_type.ty {
                        Type::Primitive(PrimitiveType::Number) => (),
                        _ => panic!("Expected number element type"),
                    }
                }
                _ => panic!("Expected array type"),
            }
        }
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_tuple_type() {
    let source = "let x: [number, string];";
    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            let ty = decl.type_annotation.as_ref().unwrap();
            match &ty.ty {
                Type::Tuple(tuple_ty) => {
                    assert_eq!(tuple_ty.element_types.len(), 2);
                }
                _ => panic!("Expected tuple type"),
            }
        }
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_function_type() {
    let source = "let x: (x: number) => string;";
    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            let ty = decl.type_annotation.as_ref().unwrap();
            match &ty.ty {
                Type::Function(func_ty) => {
                    assert_eq!(func_ty.params.len(), 1);
                }
                _ => panic!("Expected function type"),
            }
        }
        _ => panic!("Expected variable declaration"),
    }
}

#[test]
fn test_parse_object_type() {
    let source = "let x: { x: number; y: string };";
    let parser = Parser::new(source).unwrap();
    let module = parser.parse().unwrap();

    match &module.statements[0] {
        Statement::VariableDecl(decl) => {
            let ty = decl.type_annotation.as_ref().unwrap();
            match &ty.ty {
                Type::Object(obj_ty) => {
                    assert_eq!(obj_ty.members.len(), 2);
                }
                _ => panic!("Expected object type"),
            }
        }
        _ => panic!("Expected variable declaration"),
    }
}
