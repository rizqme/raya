use raya_types::{
    BareUnionDetector, BareUnionError, BareUnionTransform, PrimitiveType, Type, TypeContext,
};

#[test]
fn test_primitive_type_name() {
    assert_eq!(PrimitiveType::String.type_name(), "string");
    assert_eq!(PrimitiveType::Number.type_name(), "number");
    assert_eq!(PrimitiveType::Boolean.type_name(), "boolean");
    assert_eq!(PrimitiveType::Null.type_name(), "null");
    assert_eq!(PrimitiveType::Void.type_name(), "void");
}

#[test]
fn test_is_bare_union_primitive() {
    assert!(PrimitiveType::String.is_bare_union_primitive());
    assert!(PrimitiveType::Number.is_bare_union_primitive());
    assert!(PrimitiveType::Boolean.is_bare_union_primitive());
    assert!(PrimitiveType::Null.is_bare_union_primitive());
    assert!(!PrimitiveType::Void.is_bare_union_primitive());
}

#[test]
fn test_detect_string_number_union() {
    let mut ctx = TypeContext::new();
    let string = ctx.string_type();
    let number = ctx.number_type();

    let detector = BareUnionDetector::new(&ctx);
    assert!(
        detector.is_bare_primitive_union(&[string, number]),
        "string | number should be detected as bare union"
    );
}

#[test]
fn test_detect_string_boolean_null_union() {
    let mut ctx = TypeContext::new();
    let string = ctx.string_type();
    let boolean = ctx.boolean_type();
    let null = ctx.null_type();

    let detector = BareUnionDetector::new(&ctx);
    assert!(
        detector.is_bare_primitive_union(&[string, boolean, null]),
        "string | boolean | null should be detected as bare union"
    );
}

#[test]
fn test_reject_empty_union() {
    let ctx = TypeContext::new();
    let detector = BareUnionDetector::new(&ctx);

    assert!(
        !detector.is_bare_primitive_union(&[]),
        "Empty union should not be bare union"
    );
}

#[test]
fn test_reject_object_union() {
    use raya_types::ty::ObjectType;

    let mut ctx = TypeContext::new();
    let string = ctx.string_type();
    let obj = ctx.intern(Type::Object(ObjectType {
        properties: vec![],
        index_signature: None,
    }));

    let detector = BareUnionDetector::new(&ctx);
    assert!(
        !detector.is_bare_primitive_union(&[string, obj]),
        "string | object should not be bare union"
    );
}

#[test]
fn test_reject_array_union() {
    use raya_types::ty::ArrayType;

    let mut ctx = TypeContext::new();
    let string = ctx.string_type();
    let number = ctx.number_type();
    let array = ctx.intern(Type::Array(ArrayType { element: number }));

    let detector = BareUnionDetector::new(&ctx);
    assert!(
        !detector.is_bare_primitive_union(&[string, array]),
        "string | array should not be bare union"
    );
}

#[test]
fn test_reject_void_union() {
    let mut ctx = TypeContext::new();
    let string = ctx.string_type();
    let void = ctx.void_type();

    let detector = BareUnionDetector::new(&ctx);
    assert!(
        !detector.is_bare_primitive_union(&[string, void]),
        "string | void should not be bare union (void is not a value type)"
    );
}

#[test]
fn test_extract_primitives() {
    let mut ctx = TypeContext::new();
    let string = ctx.string_type();
    let number = ctx.number_type();
    let boolean = ctx.boolean_type();

    let detector = BareUnionDetector::new(&ctx);
    let prims = detector.extract_primitives(&[string, number, boolean]);

    assert_eq!(prims.len(), 3);
    assert!(prims.contains(&PrimitiveType::String));
    assert!(prims.contains(&PrimitiveType::Number));
    assert!(prims.contains(&PrimitiveType::Boolean));
}

#[test]
fn test_extract_primitives_with_non_primitives() {
    use raya_types::ty::ObjectType;

    let mut ctx = TypeContext::new();
    let string = ctx.string_type();
    let number = ctx.number_type();
    let obj = ctx.intern(Type::Object(ObjectType {
        properties: vec![],
        index_signature: None,
    }));

    let detector = BareUnionDetector::new(&ctx);
    let prims = detector.extract_primitives(&[string, obj, number]);

    // Should only extract the primitives
    assert_eq!(prims.len(), 2);
    assert!(prims.contains(&PrimitiveType::String));
    assert!(prims.contains(&PrimitiveType::Number));
}

#[test]
fn test_validate_no_duplicates_success() {
    let ctx = TypeContext::new();
    let detector = BareUnionDetector::new(&ctx);

    let prims = vec![PrimitiveType::String, PrimitiveType::Number];
    let result = detector.validate_no_duplicates(&prims);
    assert!(result.is_ok());
}

#[test]
fn test_validate_no_duplicates_all_unique() {
    let ctx = TypeContext::new();
    let detector = BareUnionDetector::new(&ctx);

    let prims = vec![
        PrimitiveType::String,
        PrimitiveType::Number,
        PrimitiveType::Boolean,
        PrimitiveType::Null,
    ];
    let result = detector.validate_no_duplicates(&prims);
    assert!(result.is_ok());
}

#[test]
fn test_validate_rejects_duplicate_string() {
    let ctx = TypeContext::new();
    let detector = BareUnionDetector::new(&ctx);

    let prims = vec![PrimitiveType::String, PrimitiveType::String];
    let result = detector.validate_no_duplicates(&prims);

    assert!(result.is_err());
    match result.unwrap_err() {
        BareUnionError::DuplicatePrimitive { primitive } => {
            assert_eq!(primitive, PrimitiveType::String);
        }
        _ => panic!("Expected DuplicatePrimitive error"),
    }
}

#[test]
fn test_validate_rejects_duplicate_number() {
    let ctx = TypeContext::new();
    let detector = BareUnionDetector::new(&ctx);

    let prims = vec![
        PrimitiveType::String,
        PrimitiveType::Number,
        PrimitiveType::Number,
    ];
    let result = detector.validate_no_duplicates(&prims);

    assert!(result.is_err());
    match result.unwrap_err() {
        BareUnionError::DuplicatePrimitive { primitive } => {
            assert_eq!(primitive, PrimitiveType::Number);
        }
        _ => panic!("Expected DuplicatePrimitive error"),
    }
}

#[test]
fn test_error_message_duplicate_primitive() {
    let error = BareUnionError::DuplicatePrimitive {
        primitive: PrimitiveType::String,
    };

    let msg = error.to_string();
    assert!(msg.contains("duplicate"));
    assert!(msg.contains("string"));
}

#[test]
fn test_error_message_non_primitive_members() {
    let error = BareUnionError::NonPrimitiveMembers {
        union_members: vec![],
    };

    let msg = error.to_string();
    assert!(msg.contains("primitive types"));
}

#[test]
fn test_error_message_forbidden_field_access() {
    let error = BareUnionError::ForbiddenFieldAccess {
        field_name: "$type".to_string(),
    };

    let msg = error.to_string();
    assert!(msg.contains("$type"));
    assert!(msg.contains("typeof"));
}

// ============================================================================
// Phase 2: Transformation Tests
// ============================================================================

#[test]
fn test_transform_string_number_union() {
    let mut ctx = TypeContext::new();

    let primitives = vec![PrimitiveType::String, PrimitiveType::Number];
    let mut transform = BareUnionTransform::new(&mut ctx);
    let internal = transform.transform(&primitives);

    // Verify internal union has correct structure
    if let Some(Type::Union(union)) = ctx.get(internal) {
        assert_eq!(union.members.len(), 2, "Should have 2 variants");

        // Check discriminant was inferred as "$type"
        assert!(union.discriminant.is_some(), "Should have discriminant");
        let disc = union.discriminant.as_ref().unwrap();
        assert_eq!(disc.field_name, "$type");

        // Check value_map has correct entries
        assert_eq!(disc.get_variant_index("string"), Some(0));
        assert_eq!(disc.get_variant_index("number"), Some(1));
    } else {
        panic!("Expected union type");
    }
}

#[test]
fn test_create_variant_structure() {
    let mut ctx = TypeContext::new();
    let mut transform = BareUnionTransform::new(&mut ctx);

    let variant = transform.create_variant(PrimitiveType::String);

    if let Some(Type::Object(obj)) = ctx.get(variant) {
        assert_eq!(obj.properties.len(), 2, "Should have 2 properties");

        // Check $type field
        let type_prop = &obj.properties[0];
        assert_eq!(type_prop.name, "$type");
        assert!(type_prop.readonly, "$type should be readonly");

        // Verify it's a string literal
        if let Some(Type::StringLiteral(lit)) = ctx.get(type_prop.ty) {
            assert_eq!(lit, "string");
        } else {
            panic!("$type field should be a string literal");
        }

        // Check $value field
        let value_prop = &obj.properties[1];
        assert_eq!(value_prop.name, "$value");
        assert!(!value_prop.readonly, "$value should not be readonly");

        // Verify it's a string type
        assert!(
            matches!(ctx.get(value_prop.ty), Some(Type::Primitive(PrimitiveType::String))),
            "$value field should be string type"
        );
    } else {
        panic!("Expected object type");
    }
}

#[test]
fn test_union_type_auto_detection_and_transformation() {
    let mut ctx = TypeContext::new();
    let string = ctx.string_type();
    let number = ctx.number_type();

    let union = ctx.union_type(vec![string, number]);

    // Verify it was detected as bare union
    if let Some(Type::Union(u)) = ctx.get(union) {
        assert!(u.is_bare, "Should be detected as bare union");
        assert!(
            u.internal_union.is_some(),
            "Should have internal representation"
        );

        // Verify internal union has correct structure
        let internal = u.internal_union.unwrap();
        if let Some(Type::Union(internal_u)) = ctx.get(internal) {
            assert_eq!(internal_u.members.len(), 2);
            assert!(internal_u.discriminant.is_some());
            let disc = internal_u.discriminant.as_ref().unwrap();
            assert_eq!(disc.field_name, "$type");
        } else {
            panic!("Internal union should be a union type");
        }
    } else {
        panic!("Expected bare union");
    }
}

#[test]
fn test_transform_all_primitive_types() {
    let mut ctx = TypeContext::new();
    let primitives = vec![
        PrimitiveType::String,
        PrimitiveType::Number,
        PrimitiveType::Boolean,
        PrimitiveType::Null,
    ];

    let mut transform = BareUnionTransform::new(&mut ctx);
    let internal = transform.transform(&primitives);

    if let Some(Type::Union(union)) = ctx.get(internal) {
        assert_eq!(union.members.len(), 4);

        // Verify discriminant
        let disc = union.discriminant.as_ref().unwrap();
        assert_eq!(disc.field_name, "$type");

        // Verify all variants are present
        assert_eq!(disc.get_variant_index("string"), Some(0));
        assert_eq!(disc.get_variant_index("number"), Some(1));
        assert_eq!(disc.get_variant_index("boolean"), Some(2));
        assert_eq!(disc.get_variant_index("null"), Some(3));
    }
}

#[test]
fn test_get_bare_union_internal() {
    let mut ctx = TypeContext::new();
    let string = ctx.string_type();
    let number = ctx.number_type();

    let union = ctx.union_type(vec![string, number]);

    // Should return internal union for bare unions
    let internal = ctx.get_bare_union_internal(union);
    assert!(internal.is_some(), "Should have internal representation");

    // Verify internal structure
    let internal_id = internal.unwrap();
    if let Some(Type::Union(u)) = ctx.get(internal_id) {
        assert_eq!(u.members.len(), 2);
        assert!(u.discriminant.is_some());
    } else {
        panic!("Internal should be a union");
    }
}

#[test]
fn test_non_bare_union_has_no_internal() {
    use raya_types::ty::ObjectType;

    let mut ctx = TypeContext::new();
    let string = ctx.string_type();
    let obj = ctx.intern(Type::Object(ObjectType {
        properties: vec![],
        index_signature: None,
    }));

    let union = ctx.union_type(vec![string, obj]);

    // Non-bare union should not have internal representation
    if let Some(Type::Union(u)) = ctx.get(union) {
        assert!(!u.is_bare, "Should not be bare union");
        assert!(
            u.internal_union.is_none(),
            "Should not have internal representation"
        );
    }

    // get_bare_union_internal should return None
    assert!(ctx.get_bare_union_internal(union).is_none());
}

#[test]
fn test_variant_properties_order() {
    let mut ctx = TypeContext::new();
    let mut transform = BareUnionTransform::new(&mut ctx);

    let variant = transform.create_variant(PrimitiveType::Number);

    if let Some(Type::Object(obj)) = ctx.get(variant) {
        // Verify order: $type comes first, then $value
        assert_eq!(obj.properties[0].name, "$type");
        assert_eq!(obj.properties[1].name, "$value");
    } else {
        panic!("Expected object type");
    }
}
