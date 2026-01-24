use raya_parser::ast::*;
use raya_parser::token::Span;

// ============================================================================
// Primitive Type Tests
// ============================================================================

#[test]
fn test_primitive_type_number() {
    let ty = Type::Primitive(PrimitiveType::Number);
    assert!(ty.is_primitive());
    assert_eq!(ty.as_primitive(), Some(PrimitiveType::Number));
}

#[test]
fn test_primitive_type_names() {
    assert_eq!(PrimitiveType::Number.name(), "number");
    assert_eq!(PrimitiveType::String.name(), "string");
    assert_eq!(PrimitiveType::Boolean.name(), "boolean");
    assert_eq!(PrimitiveType::Null.name(), "null");
    assert_eq!(PrimitiveType::Void.name(), "void");
}

#[test]
fn test_all_primitive_types() {
    let primitives = vec![
        PrimitiveType::Number,
        PrimitiveType::String,
        PrimitiveType::Boolean,
        PrimitiveType::Null,
        PrimitiveType::Void,
    ];

    for prim in primitives {
        let ty = Type::Primitive(prim);
        assert!(ty.is_primitive());
        assert!(!ty.is_union());
        assert!(!ty.is_function());
    }
}

// ============================================================================
// Type Reference Tests
// ============================================================================

#[test]
fn test_simple_type_reference() {
    let type_ref = TypeReference::simple(Identifier::new("Point".to_string(), Span::new(0, 5, 1, 1)));

    assert!(!type_ref.is_generic());
    assert_eq!(type_ref.name.name, "Point");
    assert!(type_ref.type_args.is_none());
}

#[test]
fn test_generic_type_reference() {
    let type_ref = TypeReference::generic(
        Identifier::new("Map".to_string(), Span::new(0, 3, 1, 1)),
        vec![
            TypeAnnotation {
                ty: Type::Primitive(PrimitiveType::String),
                span: Span::new(4, 10, 1, 5),
            },
            TypeAnnotation {
                ty: Type::Primitive(PrimitiveType::Number),
                span: Span::new(12, 18, 1, 13),
            },
        ],
    );

    assert!(type_ref.is_generic());
    assert_eq!(type_ref.name.name, "Map");
    assert_eq!(type_ref.type_args.as_ref().unwrap().len(), 2);
}

// ============================================================================
// Union Type Tests
// ============================================================================

#[test]
fn test_simple_union_type() {
    let union = UnionType::new(vec![
        TypeAnnotation {
            ty: Type::Primitive(PrimitiveType::String),
            span: Span::new(0, 6, 1, 1),
        },
        TypeAnnotation {
            ty: Type::Primitive(PrimitiveType::Number),
            span: Span::new(9, 15, 1, 10),
        },
    ]);

    assert_eq!(union.len(), 2);
    assert!(!union.is_empty());

    let ty = Type::Union(union);
    assert!(ty.is_union());
}

#[test]
fn test_bare_union_detection() {
    // Bare union: all primitives
    let bare_union = UnionType::new(vec![
        TypeAnnotation {
            ty: Type::Primitive(PrimitiveType::String),
            span: Span::new(0, 6, 1, 1),
        },
        TypeAnnotation {
            ty: Type::Primitive(PrimitiveType::Number),
            span: Span::new(9, 15, 1, 10),
        },
        TypeAnnotation {
            ty: Type::Primitive(PrimitiveType::Null),
            span: Span::new(18, 22, 1, 19),
        },
    ]);

    assert!(bare_union.is_bare_union());

    // Not a bare union: contains non-primitive
    let complex_union = UnionType::new(vec![
        TypeAnnotation {
            ty: Type::Primitive(PrimitiveType::String),
            span: Span::new(0, 6, 1, 1),
        },
        TypeAnnotation {
            ty: Type::Reference(TypeReference::simple(Identifier::new(
                "MyClass".to_string(),
                Span::new(9, 16, 1, 10),
            ))),
            span: Span::new(9, 16, 1, 10),
        },
    ]);

    assert!(!complex_union.is_bare_union());
}

// ============================================================================
// Function Type Tests
// ============================================================================

#[test]
fn test_nullary_function_type() {
    let func_type = FunctionType {
        params: vec![],
        return_type: Box::new(TypeAnnotation {
            ty: Type::Primitive(PrimitiveType::Void),
            span: Span::new(7, 11, 1, 8),
        }),
    };

    assert!(func_type.is_nullary());
    assert_eq!(func_type.param_count(), 0);
}

#[test]
fn test_function_type_with_params() {
    let func_type = FunctionType {
        params: vec![
            FunctionTypeParam {
                name: Some(Identifier::new("x".to_string(), Span::new(1, 2, 1, 2))),
                ty: TypeAnnotation {
                    ty: Type::Primitive(PrimitiveType::Number),
                    span: Span::new(4, 10, 1, 5),
                },
            },
            FunctionTypeParam {
                name: Some(Identifier::new("y".to_string(), Span::new(12, 13, 1, 13))),
                ty: TypeAnnotation {
                    ty: Type::Primitive(PrimitiveType::Number),
                    span: Span::new(15, 21, 1, 16),
                },
            },
        ],
        return_type: Box::new(TypeAnnotation {
            ty: Type::Primitive(PrimitiveType::Number),
            span: Span::new(26, 32, 1, 27),
        }),
    };

    assert!(!func_type.is_nullary());
    assert_eq!(func_type.param_count(), 2);

    let ty = Type::Function(func_type);
    assert!(ty.is_function());
}

#[test]
fn test_function_type_param_without_name() {
    // (number, string) => void
    let func_type = FunctionType {
        params: vec![
            FunctionTypeParam {
                name: None,
                ty: TypeAnnotation {
                    ty: Type::Primitive(PrimitiveType::Number),
                    span: Span::new(1, 7, 1, 2),
                },
            },
            FunctionTypeParam {
                name: None,
                ty: TypeAnnotation {
                    ty: Type::Primitive(PrimitiveType::String),
                    span: Span::new(9, 15, 1, 10),
                },
            },
        ],
        return_type: Box::new(TypeAnnotation {
            ty: Type::Primitive(PrimitiveType::Void),
            span: Span::new(20, 24, 1, 21),
        }),
    };

    assert_eq!(func_type.param_count(), 2);
    assert!(func_type.params[0].name.is_none());
    assert!(func_type.params[1].name.is_none());
}

// ============================================================================
// Array Type Tests
// ============================================================================

#[test]
fn test_array_type() {
    let array_type = ArrayType::new(TypeAnnotation {
        ty: Type::Primitive(PrimitiveType::Number),
        span: Span::new(0, 6, 1, 1),
    });

    if let Type::Primitive(PrimitiveType::Number) = array_type.element_type.ty {
        assert!(true);
    } else {
        panic!("Expected number element type");
    }
}

#[test]
fn test_nested_array_type() {
    // number[][]
    let array_type = ArrayType::new(TypeAnnotation {
        ty: Type::Array(ArrayType::new(TypeAnnotation {
            ty: Type::Primitive(PrimitiveType::Number),
            span: Span::new(0, 6, 1, 1),
        })),
        span: Span::new(0, 8, 1, 1),
    });

    // Verify it's an array of arrays
    if let Type::Array(_) = array_type.element_type.ty {
        assert!(true);
    } else {
        panic!("Expected array element type");
    }
}

// ============================================================================
// Tuple Type Tests
// ============================================================================

#[test]
fn test_simple_tuple_type() {
    let tuple = TupleType::new(vec![
        TypeAnnotation {
            ty: Type::Primitive(PrimitiveType::Number),
            span: Span::new(1, 7, 1, 2),
        },
        TypeAnnotation {
            ty: Type::Primitive(PrimitiveType::String),
            span: Span::new(9, 15, 1, 10),
        },
    ]);

    assert_eq!(tuple.len(), 2);
    assert!(!tuple.is_empty());
}

#[test]
fn test_empty_tuple() {
    let tuple = TupleType::new(vec![]);

    assert_eq!(tuple.len(), 0);
    assert!(tuple.is_empty());
}

#[test]
fn test_complex_tuple() {
    // [number, string, boolean[]]
    let tuple = TupleType::new(vec![
        TypeAnnotation {
            ty: Type::Primitive(PrimitiveType::Number),
            span: Span::new(1, 7, 1, 2),
        },
        TypeAnnotation {
            ty: Type::Primitive(PrimitiveType::String),
            span: Span::new(9, 15, 1, 10),
        },
        TypeAnnotation {
            ty: Type::Array(ArrayType::new(TypeAnnotation {
                ty: Type::Primitive(PrimitiveType::Boolean),
                span: Span::new(17, 24, 1, 18),
            })),
            span: Span::new(17, 26, 1, 18),
        },
    ]);

    assert_eq!(tuple.len(), 3);
}

// ============================================================================
// Object Type Tests
// ============================================================================

#[test]
fn test_empty_object_type() {
    let obj = ObjectType::new(vec![]);

    assert_eq!(obj.len(), 0);
    assert!(obj.is_empty());
}

#[test]
fn test_object_type_with_properties() {
    let obj = ObjectType::new(vec![
        ObjectTypeMember::Property(ObjectTypeProperty {
            name: Identifier::new("x".to_string(), Span::new(2, 3, 1, 3)),
            ty: TypeAnnotation {
                ty: Type::Primitive(PrimitiveType::Number),
                span: Span::new(5, 11, 1, 6),
            },
            optional: false,
            span: Span::new(2, 11, 1, 3),
        }),
        ObjectTypeMember::Property(ObjectTypeProperty {
            name: Identifier::new("y".to_string(), Span::new(13, 14, 2, 3)),
            ty: TypeAnnotation {
                ty: Type::Primitive(PrimitiveType::String),
                span: Span::new(16, 22, 2, 6),
            },
            optional: false,
            span: Span::new(13, 22, 2, 3),
        }),
    ]);

    assert_eq!(obj.len(), 2);
    assert!(!obj.is_empty());
}

#[test]
fn test_object_type_with_optional_property() {
    let obj = ObjectType::new(vec![ObjectTypeMember::Property(ObjectTypeProperty {
        name: Identifier::new("optional".to_string(), Span::new(2, 10, 1, 3)),
        ty: TypeAnnotation {
            ty: Type::Primitive(PrimitiveType::String),
            span: Span::new(13, 19, 1, 14),
        },
        optional: true,
        span: Span::new(2, 19, 1, 3),
    })]);

    if let ObjectTypeMember::Property(prop) = &obj.members[0] {
        assert!(prop.optional);
    }
}

#[test]
fn test_object_type_with_method() {
    let obj = ObjectType::new(vec![ObjectTypeMember::Method(ObjectTypeMethod {
        name: Identifier::new("add".to_string(), Span::new(2, 5, 1, 3)),
        params: vec![
            FunctionTypeParam {
                name: Some(Identifier::new("x".to_string(), Span::new(6, 7, 1, 7))),
                ty: TypeAnnotation {
                    ty: Type::Primitive(PrimitiveType::Number),
                    span: Span::new(9, 15, 1, 10),
                },
            },
        ],
        return_type: TypeAnnotation {
            ty: Type::Primitive(PrimitiveType::Number),
            span: Span::new(20, 26, 1, 21),
        },
        span: Span::new(2, 26, 1, 3),
    })]);

    if let ObjectTypeMember::Method(method) = &obj.members[0] {
        assert_eq!(method.name.name, "add");
        assert_eq!(method.params.len(), 1);
    }
}

// ============================================================================
// Typeof Type Tests
// ============================================================================

#[test]
fn test_typeof_type() {
    let typeof_type = TypeofType {
        argument: Box::new(Expression::Identifier(Identifier::new(
            "value".to_string(),
            Span::new(7, 12, 1, 8),
        ))),
    };

    let ty = Type::Typeof(typeof_type);
    assert!(!ty.is_primitive());
    assert!(!ty.is_union());
    assert!(!ty.is_function());
}

// ============================================================================
// Type Parameter Tests
// ============================================================================

#[test]
fn test_simple_type_parameter() {
    let type_param = TypeParameter::simple(
        Identifier::new("T".to_string(), Span::new(0, 1, 1, 1)),
        Span::new(0, 1, 1, 1),
    );

    assert!(!type_param.is_constrained());
    assert!(!type_param.has_default());
    assert_eq!(type_param.name.name, "T");
}

#[test]
fn test_constrained_type_parameter() {
    let type_param = TypeParameter {
        name: Identifier::new("T".to_string(), Span::new(0, 1, 1, 1)),
        constraint: Some(TypeAnnotation {
            ty: Type::Primitive(PrimitiveType::String),
            span: Span::new(10, 16, 1, 11),
        }),
        default: None,
        span: Span::new(0, 16, 1, 1),
    };

    assert!(type_param.is_constrained());
    assert!(!type_param.has_default());
}

#[test]
fn test_type_parameter_with_default() {
    let type_param = TypeParameter {
        name: Identifier::new("T".to_string(), Span::new(0, 1, 1, 1)),
        constraint: None,
        default: Some(TypeAnnotation {
            ty: Type::Primitive(PrimitiveType::String),
            span: Span::new(4, 10, 1, 5),
        }),
        span: Span::new(0, 10, 1, 1),
    };

    assert!(!type_param.is_constrained());
    assert!(type_param.has_default());
}

#[test]
fn test_type_parameter_with_constraint_and_default() {
    let type_param = TypeParameter {
        name: Identifier::new("T".to_string(), Span::new(0, 1, 1, 1)),
        constraint: Some(TypeAnnotation {
            ty: Type::Primitive(PrimitiveType::Number),
            span: Span::new(10, 16, 1, 11),
        }),
        default: Some(TypeAnnotation {
            ty: Type::Primitive(PrimitiveType::Number),
            span: Span::new(19, 25, 1, 20),
        }),
        span: Span::new(0, 25, 1, 1),
    };

    assert!(type_param.is_constrained());
    assert!(type_param.has_default());
}

// ============================================================================
// Complex Type Tests
// ============================================================================

#[test]
fn test_parenthesized_type() {
    let ty = Type::Parenthesized(Box::new(TypeAnnotation {
        ty: Type::Union(UnionType::new(vec![
            TypeAnnotation {
                ty: Type::Primitive(PrimitiveType::String),
                span: Span::new(1, 7, 1, 2),
            },
            TypeAnnotation {
                ty: Type::Primitive(PrimitiveType::Number),
                span: Span::new(10, 16, 1, 11),
            },
        ])),
        span: Span::new(1, 16, 1, 2),
    }));

    assert!(!ty.is_primitive());
    assert!(!ty.is_union()); // The parenthesized wrapper itself is not a union
}

#[test]
fn test_generic_array_type() {
    // Array<number>
    let generic_array = TypeReference::generic(
        Identifier::new("Array".to_string(), Span::new(0, 5, 1, 1)),
        vec![TypeAnnotation {
            ty: Type::Primitive(PrimitiveType::Number),
            span: Span::new(6, 12, 1, 7),
        }],
    );

    assert!(generic_array.is_generic());
    assert_eq!(generic_array.type_args.as_ref().unwrap().len(), 1);
}

#[test]
fn test_function_returning_union() {
    // () => string | number
    let func_type = FunctionType {
        params: vec![],
        return_type: Box::new(TypeAnnotation {
            ty: Type::Union(UnionType::new(vec![
                TypeAnnotation {
                    ty: Type::Primitive(PrimitiveType::String),
                    span: Span::new(6, 12, 1, 7),
                },
                TypeAnnotation {
                    ty: Type::Primitive(PrimitiveType::Number),
                    span: Span::new(15, 21, 1, 16),
                },
            ])),
            span: Span::new(6, 21, 1, 7),
        }),
    };

    assert!(func_type.is_nullary());
}

#[test]
fn test_union_of_object_types() {
    // { x: number } | { y: string }
    let union = UnionType::new(vec![
        TypeAnnotation {
            ty: Type::Object(ObjectType::new(vec![ObjectTypeMember::Property(
                ObjectTypeProperty {
                    name: Identifier::new("x".to_string(), Span::new(2, 3, 1, 3)),
                    ty: TypeAnnotation {
                        ty: Type::Primitive(PrimitiveType::Number),
                        span: Span::new(5, 11, 1, 6),
                    },
                    optional: false,
                    span: Span::new(2, 11, 1, 3),
                },
            )])),
            span: Span::new(0, 13, 1, 1),
        },
        TypeAnnotation {
            ty: Type::Object(ObjectType::new(vec![ObjectTypeMember::Property(
                ObjectTypeProperty {
                    name: Identifier::new("y".to_string(), Span::new(18, 19, 1, 19)),
                    ty: TypeAnnotation {
                        ty: Type::Primitive(PrimitiveType::String),
                        span: Span::new(21, 27, 1, 22),
                    },
                    optional: false,
                    span: Span::new(18, 27, 1, 19),
                },
            )])),
            span: Span::new(16, 29, 1, 17),
        },
    ]);

    assert!(!union.is_bare_union()); // Contains objects, not primitives
    assert_eq!(union.len(), 2);
}
