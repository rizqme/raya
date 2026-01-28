//! Discriminant inference for discriminated unions
//!
//! This module implements automatic discriminant field detection for union types.
//! The discriminant is a field with literal types that appears in all variants
//! and has distinct values for each variant.

use super::{Type, TypeContext, TypeId};
use rustc_hash::{FxHashMap, FxHashSet};
use std::fmt;
use std::hash::{Hash, Hasher};

/// Information about a discriminant field
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discriminant {
    /// Field name that serves as discriminant
    pub field_name: String,

    /// Map from discriminant value to variant index
    /// e.g., "ok" -> 0, "error" -> 1
    pub value_map: FxHashMap<String, usize>,
}

impl Hash for Discriminant {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Hash only field_name for performance
        // value_map is deterministic given the variants
        self.field_name.hash(state);
    }
}

impl Discriminant {
    /// Get the variant index for a discriminant value
    pub fn get_variant_index(&self, value: &str) -> Option<usize> {
        self.value_map.get(value).copied()
    }

    /// Get all discriminant values
    pub fn all_values(&self) -> Vec<&String> {
        self.value_map.keys().collect()
    }

    /// Check if a value is valid for this discriminant
    pub fn is_valid_value(&self, value: &str) -> bool {
        self.value_map.contains_key(value)
    }
}

/// Errors that can occur during discriminant inference
#[derive(Debug, Clone, PartialEq)]
pub enum DiscriminantError {
    /// Union has no common fields with literal types
    NoCommonLiteralFields { variants: Vec<TypeId> },

    /// Discriminant values are not unique across variants
    DuplicateValues {
        field: String,
        duplicate_value: String,
        variants: Vec<TypeId>,
    },

    /// Variant is missing the discriminant field
    MissingField {
        variant: TypeId,
        field: String,
    },

    /// Field type is not a literal
    NotALiteral { variant: TypeId, field: String },

    /// Discriminant field has inconsistent types (e.g., string vs number)
    InconsistentTypes {
        field: String,
        expected: String,
        found: String,
        variant: TypeId,
    },

    /// Union cannot be empty
    EmptyUnion,

    /// Variant is not an object type
    NonObjectVariant { variant: TypeId },
}

impl fmt::Display for DiscriminantError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            DiscriminantError::NoCommonLiteralFields { variants } => {
                write!(
                    f,
                    "Cannot infer discriminant: union with {} variants has no common fields with literal types",
                    variants.len()
                )
            }
            DiscriminantError::DuplicateValues { field, duplicate_value, .. } => {
                write!(
                    f,
                    "Discriminant field '{}' has duplicate value '{}'",
                    field, duplicate_value
                )
            }
            DiscriminantError::MissingField { field, .. } => {
                write!(f, "Variant is missing discriminant field '{}'", field)
            }
            DiscriminantError::NotALiteral { field, .. } => {
                write!(f, "Field '{}' is not a literal type", field)
            }
            DiscriminantError::InconsistentTypes {
                field,
                expected,
                found,
                ..
            } => {
                write!(
                    f,
                    "Discriminant field '{}' has inconsistent types: expected {}, found {}",
                    field, expected, found
                )
            }
            DiscriminantError::EmptyUnion => {
                write!(f, "Union cannot be empty")
            }
            DiscriminantError::NonObjectVariant { .. } => {
                write!(f, "All union variants must be object types for discriminant inference")
            }
        }
    }
}

impl std::error::Error for DiscriminantError {}

/// Discriminant inference engine
pub struct DiscriminantInference<'a> {
    type_ctx: &'a TypeContext,
}

impl<'a> DiscriminantInference<'a> {
    pub fn new(type_ctx: &'a TypeContext) -> Self {
        Self { type_ctx }
    }

    /// Infer discriminant for a union type
    ///
    /// Algorithm (from LANG.md Section 17.6):
    /// 1. Find all fields with literal types that exist in ALL variants
    /// 2. If multiple candidates, use priority order:
    ///    kind > type > tag > variant > alphabetical
    /// 3. If no common field with literal types exists, return error
    pub fn infer(&self, variants: &[TypeId]) -> Result<Discriminant, DiscriminantError> {
        if variants.is_empty() {
            return Err(DiscriminantError::EmptyUnion);
        }

        // Validate all variants are object types
        self.validate_object_variants(variants)?;

        // Step 1: Find common fields with literal types
        let candidates = self.find_common_literal_fields(variants)?;

        if candidates.is_empty() {
            return Err(DiscriminantError::NoCommonLiteralFields {
                variants: variants.to_vec(),
            });
        }

        // Step 2: Select discriminant using priority order
        let discriminant_field = self.select_by_priority(&candidates);

        // Step 3: Validate type consistency
        self.validate_literal_type_consistency(variants, &discriminant_field)?;

        // Step 4: Build value map
        let value_map = self.build_value_map(variants, &discriminant_field)?;

        // Step 5: Validate distinct values
        self.validate_distinct_values(&value_map, variants, &discriminant_field)?;

        Ok(Discriminant {
            field_name: discriminant_field,
            value_map,
        })
    }

    /// Validate that all variants are object types
    fn validate_object_variants(&self, variants: &[TypeId]) -> Result<(), DiscriminantError> {
        for &variant_id in variants {
            let variant = self.type_ctx.get(variant_id)
                .expect("Invalid variant TypeId in discriminant inference");
            if !matches!(variant, Type::Object(_)) {
                return Err(DiscriminantError::NonObjectVariant {
                    variant: variant_id,
                });
            }
        }
        Ok(())
    }

    /// Find fields that:
    /// - Exist in ALL variants
    /// - Have literal types (string literals, number literals, boolean literals)
    fn find_common_literal_fields(
        &self,
        variants: &[TypeId],
    ) -> Result<Vec<String>, DiscriminantError> {
        // Get fields from first variant
        let first_variant = self.type_ctx.get(variants[0])
            .expect("Invalid variant TypeId");
        let mut common_fields = self.get_literal_fields(first_variant);

        // Intersect with fields from other variants
        for &variant_id in &variants[1..] {
            let variant = self.type_ctx.get(variant_id)
                .expect("Invalid variant TypeId");
            let variant_fields = self.get_literal_fields(variant);

            common_fields.retain(|field| variant_fields.contains(field));
        }

        Ok(common_fields.into_iter().collect())
    }

    /// Extract fields with literal types from an object type
    fn get_literal_fields(&self, ty: &Type) -> FxHashSet<String> {
        let mut fields = FxHashSet::default();

        if let Type::Object(obj) = ty {
            for prop in &obj.properties {
                let field_ty = self.type_ctx.get(prop.ty)
                    .expect("Invalid field type TypeId");
                if self.is_literal_type(field_ty) {
                    fields.insert(prop.name.clone());
                }
            }
        }

        fields
    }

    /// Check if a type is a literal type
    fn is_literal_type(&self, ty: &Type) -> bool {
        matches!(
            ty,
            Type::StringLiteral(_) | Type::NumberLiteral(_) | Type::BooleanLiteral(_)
        )
    }

    /// Get property TypeId by name from an ObjectType
    fn get_property_type(&self, obj: &super::ty::ObjectType, name: &str) -> Option<TypeId> {
        obj.properties
            .iter()
            .find(|prop| prop.name == name)
            .map(|prop| prop.ty)
    }

    /// Select discriminant field using priority order
    ///
    /// Priority: kind > type > tag > variant > alphabetical
    fn select_by_priority(&self, candidates: &[String]) -> String {
        const PRIORITY: &[&str] = &["kind", "type", "tag", "variant"];

        // Check priority order
        for &preferred in PRIORITY {
            if candidates.iter().any(|c| c == preferred) {
                return preferred.to_string();
            }
        }

        // Fall back to alphabetical
        let mut sorted = candidates.to_vec();
        sorted.sort();
        sorted[0].clone()
    }

    /// Validate that discriminant field has consistent types across variants
    fn validate_literal_type_consistency(
        &self,
        variants: &[TypeId],
        discriminant_field: &str,
    ) -> Result<(), DiscriminantError> {
        let mut literal_kind: Option<&str> = None;

        for &variant_id in variants {
            let variant = self.type_ctx.get(variant_id)
                .expect("Invalid variant TypeId");
            if let Type::Object(obj) = variant {
                if let Some(field_ty_id) = self.get_property_type(obj, discriminant_field) {
                    let field_ty = self.type_ctx.get(field_ty_id)
                    .expect("Invalid field type TypeId");
                    let kind = self.get_literal_kind(field_ty, variant_id, discriminant_field)?;

                    if let Some(expected) = literal_kind {
                        if expected != kind {
                            return Err(DiscriminantError::InconsistentTypes {
                                field: discriminant_field.to_string(),
                                expected: expected.to_string(),
                                found: kind.to_string(),
                                variant: variant_id,
                            });
                        }
                    } else {
                        literal_kind = Some(kind);
                    }
                } else {
                    return Err(DiscriminantError::MissingField {
                        variant: variant_id,
                        field: discriminant_field.to_string(),
                    });
                }
            }
        }

        Ok(())
    }

    /// Get the kind of literal type (string, number, boolean)
    fn get_literal_kind(
        &self,
        ty: &Type,
        variant_id: TypeId,
        field: &str,
    ) -> Result<&'static str, DiscriminantError> {
        match ty {
            Type::StringLiteral(_) => Ok("string"),
            Type::NumberLiteral(_) => Ok("number"),
            Type::BooleanLiteral(_) => Ok("boolean"),
            _ => Err(DiscriminantError::NotALiteral {
                variant: variant_id,
                field: field.to_string(),
            }),
        }
    }

    /// Build map from discriminant value to variant index
    fn build_value_map(
        &self,
        variants: &[TypeId],
        discriminant_field: &str,
    ) -> Result<FxHashMap<String, usize>, DiscriminantError> {
        let mut value_map = FxHashMap::default();

        for (idx, &variant_id) in variants.iter().enumerate() {
            let variant = self.type_ctx.get(variant_id)
                .expect("Invalid variant TypeId");

            if let Type::Object(obj) = variant {
                if let Some(field_ty_id) = self.get_property_type(obj, discriminant_field) {
                    let field_ty = self.type_ctx.get(field_ty_id)
                    .expect("Invalid field type TypeId");
                    let value = self.extract_literal_value(field_ty)?;

                    value_map.insert(value, idx);
                } else {
                    return Err(DiscriminantError::MissingField {
                        variant: variant_id,
                        field: discriminant_field.to_string(),
                    });
                }
            }
        }

        Ok(value_map)
    }

    /// Extract string value from a literal type
    fn extract_literal_value(&self, ty: &Type) -> Result<String, DiscriminantError> {
        match ty {
            Type::StringLiteral(s) => Ok(s.clone()),
            Type::NumberLiteral(n) => Ok(n.to_string()),
            Type::BooleanLiteral(b) => Ok(b.to_string()),
            _ => {
                // This shouldn't happen if we validated correctly
                Err(DiscriminantError::EmptyUnion) // Placeholder error
            }
        }
    }

    /// Validate that all discriminant values are distinct
    fn validate_distinct_values(
        &self,
        value_map: &FxHashMap<String, usize>,
        variants: &[TypeId],
        field: &str,
    ) -> Result<(), DiscriminantError> {
        if value_map.len() != variants.len() {
            // Find the duplicate value
            let mut seen = FxHashSet::default();
            for &variant_id in variants.iter() {
                let variant = self.type_ctx.get(variant_id)
                .expect("Invalid variant TypeId");
                if let Type::Object(obj) = variant {
                    if let Some(field_ty_id) = self.get_property_type(obj, field) {
                        let field_ty = self.type_ctx.get(field_ty_id)
                    .expect("Invalid field type TypeId");
                        if let Ok(value) = self.extract_literal_value(field_ty) {
                            if !seen.insert(value.clone()) {
                                return Err(DiscriminantError::DuplicateValues {
                                    field: field.to_string(),
                                    duplicate_value: value,
                                    variants: variants.to_vec(),
                                });
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_selection() {
        let ctx = TypeContext::new();
        let inference = DiscriminantInference::new(&ctx);

        // Test "kind" has highest priority
        assert_eq!(
            inference.select_by_priority(&["type".to_string(), "kind".to_string()]),
            "kind"
        );

        // Test "type" preferred over "tag"
        assert_eq!(
            inference.select_by_priority(&["tag".to_string(), "type".to_string()]),
            "type"
        );

        // Test alphabetical fallback
        assert_eq!(
            inference.select_by_priority(&["zebra".to_string(), "apple".to_string()]),
            "apple"
        );
    }

    #[test]
    fn test_is_literal_type() {
        let mut ctx = TypeContext::new();

        // Create all types first
        let str_lit_id = ctx.string_literal("test");
        let num_lit_id = ctx.number_literal(42.0);
        let bool_lit_id = ctx.boolean_literal(true);
        let number_id = ctx.number_type();

        // Now create inference with immutable borrow
        let inference = DiscriminantInference::new(&ctx);

        // String literal
        let str_lit = ctx.get(str_lit_id).unwrap();
        assert!(inference.is_literal_type(str_lit));

        // Number literal
        let num_lit = ctx.get(num_lit_id).unwrap();
        assert!(inference.is_literal_type(num_lit));

        // Boolean literal
        let bool_lit = ctx.get(bool_lit_id).unwrap();
        assert!(inference.is_literal_type(bool_lit));

        // Not a literal
        let number = ctx.get(number_id).unwrap();
        assert!(!inference.is_literal_type(number));
    }

    #[test]
    fn test_infer_simple_discriminated_union() {
        use crate::parser::types::ty::{ObjectType, PropertySignature};

        let mut ctx = TypeContext::new();

        // Create { kind: "ok", value: number }
        let ok_kind = ctx.string_literal("ok");
        let number = ctx.number_type();
        let ok_variant = ctx.intern(Type::Object(ObjectType {
            properties: vec![
                PropertySignature {
                    name: "kind".to_string(),
                    ty: ok_kind,
                    optional: false,
                    readonly: false,
                },
                PropertySignature {
                    name: "value".to_string(),
                    ty: number,
                    optional: false,
                    readonly: false,
                },
            ],
            index_signature: None,
        }));

        // Create { kind: "error", error: string }
        let error_kind = ctx.string_literal("error");
        let string = ctx.string_type();
        let error_variant = ctx.intern(Type::Object(ObjectType {
            properties: vec![
                PropertySignature {
                    name: "kind".to_string(),
                    ty: error_kind,
                    optional: false,
                    readonly: false,
                },
                PropertySignature {
                    name: "error".to_string(),
                    ty: string,
                    optional: false,
                    readonly: false,
                },
            ],
            index_signature: None,
        }));

        let inference = DiscriminantInference::new(&ctx);
        let result = inference.infer(&[ok_variant, error_variant]);

        assert!(result.is_ok());
        let discriminant = result.unwrap();
        assert_eq!(discriminant.field_name, "kind");
        assert_eq!(discriminant.get_variant_index("ok"), Some(0));
        assert_eq!(discriminant.get_variant_index("error"), Some(1));
    }

    #[test]
    fn test_infer_with_priority_order() {
        use crate::parser::types::ty::{ObjectType, PropertySignature};

        let mut ctx = TypeContext::new();

        // Variant with both "kind" and "type" fields (kind should win)
        let kind_lit = ctx.string_literal("a");
        let type_lit = ctx.string_literal("x");
        let variant1 = ctx.intern(Type::Object(ObjectType {
            properties: vec![
                PropertySignature {
                    name: "kind".to_string(),
                    ty: kind_lit,
                    optional: false,
                    readonly: false,
                },
                PropertySignature {
                    name: "type".to_string(),
                    ty: type_lit,
                    optional: false,
                    readonly: false,
                },
            ],
            index_signature: None,
        }));

        let kind_lit2 = ctx.string_literal("b");
        let type_lit2 = ctx.string_literal("y");
        let variant2 = ctx.intern(Type::Object(ObjectType {
            properties: vec![
                PropertySignature {
                    name: "kind".to_string(),
                    ty: kind_lit2,
                    optional: false,
                    readonly: false,
                },
                PropertySignature {
                    name: "type".to_string(),
                    ty: type_lit2,
                    optional: false,
                    readonly: false,
                },
            ],
            index_signature: None,
        }));

        let inference = DiscriminantInference::new(&ctx);
        let result = inference.infer(&[variant1, variant2]);

        assert!(result.is_ok());
        let discriminant = result.unwrap();
        // "kind" has higher priority than "type"
        assert_eq!(discriminant.field_name, "kind");
    }

    #[test]
    fn test_no_common_literal_fields() {
        use crate::parser::types::ty::{ObjectType, PropertySignature};

        let mut ctx = TypeContext::new();

        // Variant 1 has "kind" field
        let kind_lit = ctx.string_literal("a");
        let variant1 = ctx.intern(Type::Object(ObjectType {
            properties: vec![
                PropertySignature {
                    name: "kind".to_string(),
                    ty: kind_lit,
                    optional: false,
                    readonly: false,
                },
            ],
            index_signature: None,
        }));

        // Variant 2 has "type" field (different name)
        let type_lit = ctx.string_literal("b");
        let variant2 = ctx.intern(Type::Object(ObjectType {
            properties: vec![
                PropertySignature {
                    name: "type".to_string(),
                    ty: type_lit,
                    optional: false,
                    readonly: false,
                },
            ],
            index_signature: None,
        }));

        let inference = DiscriminantInference::new(&ctx);
        let result = inference.infer(&[variant1, variant2]);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DiscriminantError::NoCommonLiteralFields { .. }
        ));
    }

    #[test]
    fn test_duplicate_discriminant_values() {
        use crate::parser::types::ty::{ObjectType, PropertySignature};

        let mut ctx = TypeContext::new();

        // Both variants have same "kind" value
        let kind_lit = ctx.string_literal("same");
        let variant1 = ctx.intern(Type::Object(ObjectType {
            properties: vec![
                PropertySignature {
                    name: "kind".to_string(),
                    ty: kind_lit,
                    optional: false,
                    readonly: false,
                },
            ],
            index_signature: None,
        }));

        let kind_lit2 = ctx.string_literal("same");
        let variant2 = ctx.intern(Type::Object(ObjectType {
            properties: vec![
                PropertySignature {
                    name: "kind".to_string(),
                    ty: kind_lit2,
                    optional: false,
                    readonly: false,
                },
            ],
            index_signature: None,
        }));

        let inference = DiscriminantInference::new(&ctx);
        let result = inference.infer(&[variant1, variant2]);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DiscriminantError::DuplicateValues { .. }
        ));
    }

    #[test]
    fn test_inconsistent_literal_types() {
        use crate::parser::types::ty::{ObjectType, PropertySignature};

        let mut ctx = TypeContext::new();

        // Variant 1 has string literal for "kind"
        let kind_str = ctx.string_literal("a");
        let variant1 = ctx.intern(Type::Object(ObjectType {
            properties: vec![
                PropertySignature {
                    name: "kind".to_string(),
                    ty: kind_str,
                    optional: false,
                    readonly: false,
                },
            ],
            index_signature: None,
        }));

        // Variant 2 has number literal for "kind"
        let kind_num = ctx.number_literal(1.0);
        let variant2 = ctx.intern(Type::Object(ObjectType {
            properties: vec![
                PropertySignature {
                    name: "kind".to_string(),
                    ty: kind_num,
                    optional: false,
                    readonly: false,
                },
            ],
            index_signature: None,
        }));

        let inference = DiscriminantInference::new(&ctx);
        let result = inference.infer(&[variant1, variant2]);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DiscriminantError::InconsistentTypes { .. }
        ));
    }

    #[test]
    fn test_union_type_auto_inference() {
        use crate::parser::types::ty::{ObjectType, PropertySignature};

        let mut ctx = TypeContext::new();

        // Create discriminated union variants
        let ok_kind = ctx.string_literal("ok");
        let number = ctx.number_type();
        let ok_variant = ctx.intern(Type::Object(ObjectType {
            properties: vec![
                PropertySignature {
                    name: "status".to_string(),
                    ty: ok_kind,
                    optional: false,
                    readonly: false,
                },
                PropertySignature {
                    name: "value".to_string(),
                    ty: number,
                    optional: false,
                    readonly: false,
                },
            ],
            index_signature: None,
        }));

        let error_kind = ctx.string_literal("error");
        let string = ctx.string_type();
        let error_variant = ctx.intern(Type::Object(ObjectType {
            properties: vec![
                PropertySignature {
                    name: "status".to_string(),
                    ty: error_kind,
                    optional: false,
                    readonly: false,
                },
                PropertySignature {
                    name: "message".to_string(),
                    ty: string,
                    optional: false,
                    readonly: false,
                },
            ],
            index_signature: None,
        }));

        // union_type should automatically infer discriminant
        let union = ctx.union_type(vec![ok_variant, error_variant]);

        // Check that discriminant was inferred
        let discriminant = ctx.get_discriminant(union);
        assert!(discriminant.is_some());
        let discriminant = discriminant.unwrap();
        assert_eq!(discriminant.field_name, "status");
        assert_eq!(discriminant.get_variant_index("ok"), Some(0));
        assert_eq!(discriminant.get_variant_index("error"), Some(1));
    }
}
