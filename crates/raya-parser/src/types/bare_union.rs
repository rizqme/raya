//! Bare Union Detection and Transformation
//!
//! This module handles bare primitive unions like `string | number`, which are
//! automatically transformed to internal discriminated unions with `$type` and
//! `$value` fields.
//!
//! Only primitive types are allowed in bare unions:
//! - number (which represents int or float in the type system)
//! - string
//! - boolean
//! - null

use super::ty::{ObjectType, PropertySignature};
use super::{PrimitiveType, Type, TypeContext, TypeId};
use rustc_hash::{FxHashMap, FxHashSet};
use std::fmt;

/// Detector for bare primitive unions
pub struct BareUnionDetector<'a> {
    type_ctx: &'a TypeContext,
}

impl<'a> BareUnionDetector<'a> {
    pub fn new(type_ctx: &'a TypeContext) -> Self {
        Self { type_ctx }
    }

    /// Check if a union type is a bare primitive union
    ///
    /// A bare union contains only primitive types (number, string, boolean, null).
    /// Returns false if:
    /// - Union is empty
    /// - Any member is not a primitive type
    /// - Any primitive is not a valid bare union primitive (e.g., void is not allowed)
    pub fn is_bare_primitive_union(&self, members: &[TypeId]) -> bool {
        if members.is_empty() {
            return false;
        }

        // All members must be primitives (excluding void)
        members.iter().all(|&member_id| {
            if let Some(ty) = self.type_ctx.get(member_id) {
                matches!(
                    ty,
                    Type::Primitive(PrimitiveType::Number)
                        | Type::Primitive(PrimitiveType::String)
                        | Type::Primitive(PrimitiveType::Boolean)
                        | Type::Primitive(PrimitiveType::Null)
                )
            } else {
                false
            }
        })
    }

    /// Extract primitive types from union members
    ///
    /// Filters members to only include primitives, returning the PrimitiveType enum values.
    pub fn extract_primitives(&self, members: &[TypeId]) -> Vec<PrimitiveType> {
        members
            .iter()
            .filter_map(|&member_id| {
                if let Some(Type::Primitive(prim)) = self.type_ctx.get(member_id) {
                    Some(*prim)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Validate no duplicate primitive types
    ///
    /// Returns an error if the same primitive appears multiple times.
    /// Example: `string | string` is invalid.
    pub fn validate_no_duplicates(
        &self,
        primitives: &[PrimitiveType],
    ) -> Result<(), BareUnionError> {
        let mut seen = FxHashSet::default();
        for &prim in primitives {
            if !seen.insert(prim) {
                return Err(BareUnionError::DuplicatePrimitive { primitive: prim });
            }
        }
        Ok(())
    }
}

/// Errors during bare union processing
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BareUnionError {
    /// Union contains duplicate primitive types
    DuplicatePrimitive { primitive: PrimitiveType },

    /// Union contains non-primitive types (cannot be bare union)
    NonPrimitiveMembers { union_members: Vec<TypeId> },

    /// User attempted to access $type or $value fields
    ForbiddenFieldAccess { field_name: String },
}

impl fmt::Display for BareUnionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BareUnionError::DuplicatePrimitive { primitive } => {
                write!(
                    f,
                    "Bare union contains duplicate primitive type: {}",
                    primitive.type_name()
                )
            }
            BareUnionError::NonPrimitiveMembers { .. } => {
                write!(
                    f,
                    "Bare unions can only contain primitive types (int, float, number, string, boolean, null)"
                )
            }
            BareUnionError::ForbiddenFieldAccess { field_name } => {
                write!(
                    f,
                    "Cannot access internal field '{}' on bare union. Use typeof for type narrowing.",
                    field_name
                )
            }
        }
    }
}

impl std::error::Error for BareUnionError {}

/// Transforms bare primitive unions to internal discriminated unions
pub struct BareUnionTransform<'a> {
    type_ctx: &'a mut TypeContext,
}

impl<'a> BareUnionTransform<'a> {
    pub fn new(type_ctx: &'a mut TypeContext) -> Self {
        Self { type_ctx }
    }

    /// Transform a bare primitive union to internal representation
    ///
    /// Transforms `string | number` into:
    /// ```text
    /// { $type: "string", $value: string } | { $type: "number", $value: number }
    /// ```
    pub fn transform(&mut self, primitives: &[PrimitiveType]) -> TypeId {
        let variants: Vec<TypeId> = primitives
            .iter()
            .map(|&prim| self.create_variant(prim))
            .collect();

        // Create internal union with automatic discriminant inference
        // This will infer "$type" as the discriminant field
        self.type_ctx.union_type(variants)
    }

    /// Create a variant object for a primitive type
    ///
    /// For PrimitiveType::String, creates:
    /// ```text
    /// { $type: "string", $value: string }
    /// ```
    ///
    /// This is public for testing purposes.
    pub fn create_variant(&mut self, prim: PrimitiveType) -> TypeId {
        // Create literal type for $type field
        let type_literal = self.type_ctx.string_literal(prim.type_name());

        // Create primitive type for $value field
        let value_type = match prim {
            PrimitiveType::Number => self.type_ctx.number_type(),
            PrimitiveType::String => self.type_ctx.string_type(),
            PrimitiveType::Boolean => self.type_ctx.boolean_type(),
            PrimitiveType::Null => self.type_ctx.null_type(),
            PrimitiveType::Void => {
                // Void should never appear in bare unions, but handle it anyway
                self.type_ctx.void_type()
            }
        };

        // Create object type with $type and $value fields
        self.type_ctx.intern(Type::Object(ObjectType {
            properties: vec![
                PropertySignature {
                    name: "$type".to_string(),
                    ty: type_literal,
                    optional: false,
                    readonly: true, // $type is immutable
                },
                PropertySignature {
                    name: "$value".to_string(),
                    ty: value_type,
                    optional: false,
                    readonly: false,
                },
            ],
            index_signature: None,
        }))
    }
}

/// Information about a bare union's internal representation
#[derive(Debug, Clone)]
pub struct BareUnionInfo {
    /// Mapping from primitive type to variant index
    pub variant_map: FxHashMap<PrimitiveType, usize>,

    /// The internal discriminated union TypeId
    pub internal_union: TypeId,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ty::ObjectType;

    #[test]
    fn test_detect_bare_primitive_union() {
        let mut ctx = TypeContext::new();
        let string = ctx.string_type();
        let number = ctx.number_type();

        let detector = BareUnionDetector::new(&ctx);
        assert!(detector.is_bare_primitive_union(&[string, number]));
    }

    #[test]
    fn test_detect_all_primitives() {
        let mut ctx = TypeContext::new();
        let number = ctx.number_type();
        let string = ctx.string_type();
        let boolean = ctx.boolean_type();
        let null = ctx.null_type();

        let detector = BareUnionDetector::new(&ctx);

        // Test all combinations
        assert!(detector.is_bare_primitive_union(&[number, string]));
        assert!(detector.is_bare_primitive_union(&[string, boolean]));
        assert!(detector.is_bare_primitive_union(&[string, null]));
        assert!(detector.is_bare_primitive_union(&[number, string, boolean, null]));
    }

    #[test]
    fn test_reject_object_in_bare_union() {
        let mut ctx = TypeContext::new();
        let string = ctx.string_type();
        let obj = ctx.intern(Type::Object(ObjectType {
            properties: vec![],
            index_signature: None,
        }));

        let detector = BareUnionDetector::new(&ctx);
        assert!(!detector.is_bare_primitive_union(&[string, obj]));
    }

    #[test]
    fn test_reject_empty_union() {
        let ctx = TypeContext::new();
        let detector = BareUnionDetector::new(&ctx);
        assert!(!detector.is_bare_primitive_union(&[]));
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
    fn test_validate_no_duplicates_success() {
        let ctx = TypeContext::new();
        let detector = BareUnionDetector::new(&ctx);
        let prims = vec![PrimitiveType::String, PrimitiveType::Number];

        let result = detector.validate_no_duplicates(&prims);
        assert!(result.is_ok());
    }

    #[test]
    fn test_reject_duplicate_primitives() {
        let ctx = TypeContext::new();
        let detector = BareUnionDetector::new(&ctx);
        let prims = vec![PrimitiveType::String, PrimitiveType::String];

        let result = detector.validate_no_duplicates(&prims);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            BareUnionError::DuplicatePrimitive { .. }
        ));
    }

    #[test]
    fn test_reject_multiple_duplicates() {
        let ctx = TypeContext::new();
        let detector = BareUnionDetector::new(&ctx);
        let prims = vec![
            PrimitiveType::String,
            PrimitiveType::Number,
            PrimitiveType::String,
        ];

        let result = detector.validate_no_duplicates(&prims);
        assert!(result.is_err());
    }
}
