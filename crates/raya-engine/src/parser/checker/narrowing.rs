//! Type narrowing engine for control flow analysis
//!
//! This module provides type narrowing based on type guards detected
//! in conditional expressions. It tracks narrowed types through control
//! flow branches and merges environments at join points.

use super::type_guards::TypeGuard;
use crate::parser::types::{Type, TypeContext, TypeId};
use rustc_hash::FxHashMap;

/// Type environment tracking narrowed types for variables
///
/// In control flow branches, variables may have narrowed types based
/// on type guards. This environment tracks those narrowed types.
#[derive(Debug, Clone)]
pub struct TypeEnv {
    /// Map from variable name to narrowed type
    bindings: FxHashMap<String, TypeId>,
}

impl TypeEnv {
    /// Create a new empty type environment
    pub fn new() -> Self {
        TypeEnv {
            bindings: FxHashMap::default(),
        }
    }

    /// Get the narrowed type for a variable, if any
    pub fn get(&self, var: &str) -> Option<TypeId> {
        self.bindings.get(var).copied()
    }

    /// Set a narrowed type for a variable
    pub fn set(&mut self, var: String, ty: TypeId) {
        self.bindings.insert(var, ty);
    }

    /// Remove a narrowed type binding
    pub fn remove(&mut self, var: &str) {
        self.bindings.remove(var);
    }

    /// Merge two type environments at a control flow join point
    ///
    /// For variables present in both environments, creates a union type.
    /// Variables only in one environment are dropped (not guaranteed to be narrowed).
    pub fn merge(&self, other: &TypeEnv, ctx: &mut TypeContext) -> TypeEnv {
        let mut merged = TypeEnv::new();

        // For each variable in both environments, union the types
        for (var, ty1) in &self.bindings {
            if let Some(ty2) = other.bindings.get(var) {
                // Both branches narrowed this variable
                let union_ty = if ty1 == ty2 {
                    // Same type in both branches - no need for union
                    *ty1
                } else {
                    // Different types - create union
                    ctx.union_type(vec![*ty1, *ty2])
                };
                merged.set(var.clone(), union_ty);
            }
            // If variable only in one branch, don't add to merged
        }

        merged
    }
}

impl Default for TypeEnv {
    fn default() -> Self {
        Self::new()
    }
}

/// Apply a type guard to narrow a type
///
/// Returns the narrowed type based on the guard, or None if the guard
/// cannot narrow the type (e.g., type guard doesn't match the type).
pub fn apply_type_guard(ctx: &mut TypeContext, ty: TypeId, guard: &TypeGuard) -> Option<TypeId> {
    match guard {
        TypeGuard::TypeOf {
            type_name, negated, ..
        } => apply_typeof_guard(ctx, ty, type_name, *negated),
        TypeGuard::Discriminant {
            field,
            variant,
            negated,
            ..
        } => apply_discriminant_guard(ctx, ty, field, variant, *negated),
        TypeGuard::Nullish { field, negated, .. } => {
            if let Some(field_path) = field {
                apply_nullish_member_guard(ctx, ty, field_path, *negated)
            } else {
                apply_nullish_guard(ctx, ty, *negated)
            }
        }
        TypeGuard::IsArray { negated, .. } => apply_is_array_guard(ctx, ty, *negated),
        TypeGuard::IsInteger { negated, .. } => apply_is_integer_guard(ctx, ty, *negated),
        TypeGuard::IsNaN { negated, .. } => apply_is_nan_guard(ctx, ty, *negated),
        TypeGuard::IsFinite { negated, .. } => apply_is_finite_guard(ctx, ty, *negated),
        TypeGuard::TypePredicate {
            predicate, negated, ..
        } => apply_type_predicate_guard(ctx, ty, predicate, *negated),
        TypeGuard::Truthiness { negated, .. } => {
            // Truthiness narrows by removing null (and potentially other falsy types)
            let null_ty = ctx.null_type();
            if !negated {
                // Truthy branch: remove null from union
                remove_from_union(ctx, ty, null_ty)
            } else {
                // Falsy branch: narrow to null (or keep as-is for non-nullable)
                if let Some(Type::Union(union)) = ctx.get(ty) {
                    if union.members.contains(&null_ty) {
                        Some(null_ty)
                    } else {
                        Some(ty)
                    }
                } else {
                    Some(ty)
                }
            }
        }
    }
}

/// Apply a typeof guard to narrow a type
fn apply_typeof_guard(
    ctx: &mut TypeContext,
    ty: TypeId,
    type_name: &str,
    negated: bool,
) -> Option<TypeId> {
    // Check if this is a bare union (for primitive type narrowing)
    if let Some(Type::Union(union)) = ctx.get(ty) {
        if union.is_bare {
            return apply_typeof_guard_bare_union(ctx, ty, type_name, negated);
        }
    }

    // Non-bare union: use standard typeof narrowing
    // Map type name to TypeId
    let target_ty = match type_name {
        "string" => ctx.string_type(),
        "number" => return narrow_by_predicate(ctx, ty, negated, is_number_like_type),
        "boolean" => ctx.boolean_type(),
        "function" => return narrow_by_predicate(ctx, ty, negated, is_function_type),
        "object" => return narrow_by_predicate(ctx, ty, negated, is_object_like_type),
        "undefined" => {
            return if negated {
                Some(ty)
            } else {
                Some(ctx.null_type())
            }
        }
        _ => return Some(ty), // Unknown type name, no narrowing
    };

    if negated {
        // typeof x !== "string" - remove string from union
        remove_from_union(ctx, ty, target_ty)
    } else {
        // typeof x === "string" - narrow to string
        Some(target_ty)
    }
}

fn narrow_by_predicate(
    ctx: &mut TypeContext,
    ty: TypeId,
    negated: bool,
    pred: fn(&Type) -> bool,
) -> Option<TypeId> {
    match ctx.get(ty).cloned() {
        Some(Type::Union(union)) => {
            let mut kept = Vec::new();
            for member in union.members {
                if let Some(member_ty) = ctx.get(member) {
                    let keep = if negated {
                        !pred(member_ty)
                    } else {
                        pred(member_ty)
                    };
                    if keep {
                        kept.push(member);
                    }
                }
            }
            if kept.is_empty() {
                Some(ctx.never_type())
            } else if kept.len() == 1 {
                Some(kept[0])
            } else {
                Some(ctx.union_type(kept))
            }
        }
        Some(t) => {
            let keep = if negated { !pred(&t) } else { pred(&t) };
            if keep {
                Some(ty)
            } else {
                Some(ctx.never_type())
            }
        }
        None => Some(ty),
    }
}

fn is_function_type(ty: &Type) -> bool {
    matches!(ty, Type::Function(_))
}

fn is_number_like_type(ty: &Type) -> bool {
    use crate::parser::types::PrimitiveType;
    matches!(
        ty,
        Type::Primitive(PrimitiveType::Number | PrimitiveType::Int)
            | Type::NumberLiteral(_)
    )
}

fn is_object_like_type(ty: &Type) -> bool {
    use crate::parser::types::PrimitiveType;
    match ty {
        Type::Object(_)
        | Type::Class(_)
        | Type::Interface(_)
        | Type::Array(_)
        | Type::Map(_)
        | Type::Set(_)
        | Type::Task(_)
        | Type::Channel(_)
        | Type::Generic(_)
        | Type::JSObject
        | Type::Json
        | Type::RegExp
        | Type::Mutex => true,
        Type::Primitive(PrimitiveType::Null) => false,
        Type::Primitive(_) => false,
        _ => false,
    }
}

/// Apply typeof guard to a bare primitive union
fn apply_typeof_guard_bare_union(
    ctx: &mut TypeContext,
    union_ty: TypeId,
    type_name: &str,
    negated: bool,
) -> Option<TypeId> {
    use crate::parser::types::PrimitiveType;

    // Map type name to PrimitiveType
    let target_prim = match type_name {
        "number" => {
            let union = match ctx.get(union_ty) {
                Some(Type::Union(u)) if u.is_bare => u.clone(),
                _ => return Some(union_ty),
            };

            let narrowed: Vec<TypeId> = union
                .members
                .iter()
                .copied()
                .filter(|&member| ctx.get(member).is_some_and(is_number_like_type))
                .collect();

            return if negated {
                remove_numeric_from_bare_union(ctx, union_ty)
            } else if narrowed.is_empty() {
                Some(ctx.never_type())
            } else if narrowed.len() == 1 {
                Some(narrowed[0])
            } else {
                Some(ctx.union_type(narrowed))
            };
        }
        "string" => PrimitiveType::String,
        "boolean" => PrimitiveType::Boolean,
        _ => return Some(union_ty), // Unknown type name, no narrowing
    };

    if negated {
        // typeof x !== "string" - remove string from union
        remove_primitive_from_bare_union(ctx, union_ty, target_prim)
    } else {
        // typeof x === "string" - narrow to string
        Some(ctx.intern(Type::Primitive(target_prim)))
    }
}

fn remove_numeric_from_bare_union(ctx: &mut TypeContext, union_id: TypeId) -> Option<TypeId> {
    let union = match ctx.get(union_id) {
        Some(Type::Union(u)) if u.is_bare => u.clone(),
        _ => return Some(union_id),
    };

    let remaining: Vec<TypeId> = union
        .members
        .iter()
        .filter(|&&member| !ctx.get(member).is_some_and(is_number_like_type))
        .copied()
        .collect();

    if remaining.is_empty() {
        Some(ctx.never_type())
    } else if remaining.len() == 1 {
        Some(remaining[0])
    } else {
        Some(ctx.union_type(remaining))
    }
}

/// Remove a primitive type from a bare union
fn remove_primitive_from_bare_union(
    ctx: &mut TypeContext,
    union_id: TypeId,
    to_remove: crate::parser::types::PrimitiveType,
) -> Option<TypeId> {
    let union = match ctx.get(union_id) {
        Some(Type::Union(u)) if u.is_bare => u.clone(),
        _ => return Some(union_id),
    };

    // Filter out the primitive to remove
    let remaining: Vec<TypeId> = union
        .members
        .iter()
        .filter(|&&member| {
            if let Some(Type::Primitive(prim)) = ctx.get(member) {
                *prim != to_remove
            } else {
                true
            }
        })
        .copied()
        .collect();

    if remaining.is_empty() {
        // No members left - unreachable
        Some(ctx.never_type())
    } else if remaining.len() == 1 {
        // Single member - return it directly
        Some(remaining[0])
    } else {
        // Multiple members - return new union
        Some(ctx.union_type(remaining))
    }
}

/// Apply a discriminant guard to narrow a discriminated union
fn apply_discriminant_guard(
    ctx: &mut TypeContext,
    ty: TypeId,
    field: &str,
    variant: &str,
    negated: bool,
) -> Option<TypeId> {
    let path: Vec<&str> = field
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect();
    if path.is_empty() {
        return Some(ty);
    }

    let is_union = matches!(ctx.get(ty), Some(Type::Union(_)));
    match narrow_type_by_discriminant_path(ctx, ty, &path, variant, negated) {
        Some(narrowed) => Some(narrowed),
        None if is_union => Some(ctx.never_type()),
        None => Some(ty),
    }
}

fn narrow_type_by_discriminant_path(
    ctx: &mut TypeContext,
    ty: TypeId,
    path: &[&str],
    variant: &str,
    negated: bool,
) -> Option<TypeId> {
    let ty_def = ctx.get(ty)?.clone();
    match ty_def {
        Type::Reference(type_ref) => {
            let named = ctx.lookup_named_type(&type_ref.name)?;
            narrow_type_by_discriminant_path(ctx, named, path, variant, negated)
        }
        Type::Generic(generic) => {
            narrow_type_by_discriminant_path(ctx, generic.base, path, variant, negated)
        }
        Type::Union(union) => {
            let mut narrowed_members = Vec::new();
            for member in union.members {
                if let Some(narrowed_member) =
                    narrow_type_by_discriminant_path(ctx, member, path, variant, negated)
                {
                    narrowed_members.push(narrowed_member);
                }
            }
            if narrowed_members.is_empty() {
                None
            } else if narrowed_members.len() == 1 {
                Some(narrowed_members[0])
            } else {
                Some(ctx.union_type(narrowed_members))
            }
        }
        Type::Object(obj) => narrow_object_discriminant_path(ctx, ty, obj, path, variant, negated),
        _ => {
            if negated {
                Some(ty)
            } else {
                None
            }
        }
    }
}

fn narrow_object_discriminant_path(
    ctx: &mut TypeContext,
    object_ty: TypeId,
    mut obj: crate::parser::types::ty::ObjectType,
    path: &[&str],
    variant: &str,
    negated: bool,
) -> Option<TypeId> {
    let (head, tail) = path.split_first()?;
    let prop_idx = obj.properties.iter().position(|prop| prop.name == *head);

    let Some(prop_idx) = prop_idx else {
        return if negated { Some(object_ty) } else { None };
    };

    if tail.is_empty() {
        let prop_ty = obj.properties[prop_idx].ty;
        let matches_variant = match ctx.get(prop_ty) {
            Some(Type::StringLiteral(lit_val)) => lit_val == variant,
            _ => true,
        };
        if (!negated && matches_variant) || (negated && !matches_variant) {
            return Some(object_ty);
        }
        return None;
    }

    let prop_ty = obj.properties[prop_idx].ty;
    let narrowed_prop_ty = narrow_type_by_discriminant_path(ctx, prop_ty, tail, variant, negated)?;
    if narrowed_prop_ty == prop_ty {
        return Some(object_ty);
    }
    obj.properties[prop_idx].ty = narrowed_prop_ty;
    Some(ctx.intern(Type::Object(obj)))
}

fn apply_nullish_member_guard(
    ctx: &mut TypeContext,
    ty: TypeId,
    field_path: &str,
    negated: bool,
) -> Option<TypeId> {
    let path: Vec<&str> = field_path
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect();
    if path.is_empty() {
        return apply_nullish_guard(ctx, ty, negated);
    }
    narrow_type_by_nullish_path(ctx, ty, &path, negated).or_else(|| Some(ty))
}

fn narrow_type_by_nullish_path(
    ctx: &mut TypeContext,
    ty: TypeId,
    path: &[&str],
    negated: bool,
) -> Option<TypeId> {
    let ty_def = ctx.get(ty)?.clone();
    match ty_def {
        Type::Reference(type_ref) => {
            let named = ctx.lookup_named_type(&type_ref.name)?;
            narrow_type_by_nullish_path(ctx, named, path, negated)
        }
        Type::Generic(generic) => narrow_type_by_nullish_path(ctx, generic.base, path, negated),
        Type::Union(union) => {
            let mut narrowed = Vec::new();
            for member in union.members {
                if let Some(member_ty) = narrow_type_by_nullish_path(ctx, member, path, negated) {
                    narrowed.push(member_ty);
                }
            }
            if narrowed.is_empty() {
                None
            } else if narrowed.len() == 1 {
                Some(narrowed[0])
            } else {
                Some(ctx.union_type(narrowed))
            }
        }
        Type::Object(obj) => narrow_object_nullish_path(ctx, ty, obj, path, negated),
        Type::Class(class_ty) => narrow_class_nullish_path(ctx, ty, class_ty, path, negated),
        _ => None,
    }
}

fn narrow_object_nullish_path(
    ctx: &mut TypeContext,
    object_ty: TypeId,
    mut obj: crate::parser::types::ty::ObjectType,
    path: &[&str],
    negated: bool,
) -> Option<TypeId> {
    let (head, tail) = path.split_first()?;
    let Some(prop_idx) = obj.properties.iter().position(|prop| prop.name == *head) else {
        return None;
    };

    let prop_ty = obj.properties[prop_idx].ty;
    let narrowed_prop = if tail.is_empty() {
        apply_nullish_guard(ctx, prop_ty, negated)?
    } else {
        narrow_type_by_nullish_path(ctx, prop_ty, tail, negated)?
    };
    if narrowed_prop == prop_ty {
        return Some(object_ty);
    }
    obj.properties[prop_idx].ty = narrowed_prop;
    Some(ctx.intern(Type::Object(obj)))
}

fn narrow_class_nullish_path(
    ctx: &mut TypeContext,
    class_ty_id: TypeId,
    mut class_ty: crate::parser::types::ty::ClassType,
    path: &[&str],
    negated: bool,
) -> Option<TypeId> {
    let (head, tail) = path.split_first()?;
    let Some(prop_idx) = class_ty
        .properties
        .iter()
        .position(|prop| prop.name == *head)
    else {
        if let Some(parent_ty) = class_ty.extends {
            let narrowed_parent = narrow_type_by_nullish_path(ctx, parent_ty, path, negated)?;
            if narrowed_parent == parent_ty {
                return Some(class_ty_id);
            }
            class_ty.extends = Some(narrowed_parent);
            return Some(ctx.intern(Type::Class(class_ty)));
        }
        return None;
    };

    let prop_ty = class_ty.properties[prop_idx].ty;
    let narrowed_prop = if tail.is_empty() {
        apply_nullish_guard(ctx, prop_ty, negated)?
    } else {
        narrow_type_by_nullish_path(ctx, prop_ty, tail, negated)?
    };
    if narrowed_prop == prop_ty {
        return Some(class_ty_id);
    }
    class_ty.properties[prop_idx].ty = narrowed_prop;
    Some(ctx.intern(Type::Class(class_ty)))
}

/// Apply a nullish guard (x !== null or x === null)
fn apply_nullish_guard(ctx: &mut TypeContext, ty: TypeId, negated: bool) -> Option<TypeId> {
    let null_ty = ctx.null_type();

    if negated {
        // x !== null - remove null from union
        remove_from_union(ctx, ty, null_ty)
    } else {
        // x === null - narrow to null
        Some(null_ty)
    }
}

/// Remove a type from a union
///
/// If `ty` is a union containing `remove_ty`, returns a new union without it.
/// If `ty` is not a union or doesn't contain `remove_ty`, returns `ty` unchanged.
fn remove_from_union(ctx: &mut TypeContext, ty: TypeId, remove_ty: TypeId) -> Option<TypeId> {
    let type_def = ctx.get(ty)?.clone();

    match type_def {
        Type::Union(union_ty) => {
            // Filter out the type to remove
            let remaining: Vec<TypeId> = union_ty
                .members
                .into_iter()
                .filter(|member| *member != remove_ty)
                .collect();

            if remaining.is_empty() {
                // All members removed - unreachable
                Some(ctx.never_type())
            } else if remaining.len() == 1 {
                // Single member - return it directly
                Some(remaining[0])
            } else {
                // Multiple members - return union
                Some(ctx.union_type(remaining))
            }
        }
        _ => {
            // Not a union - check if it's the type to remove
            if ty == remove_ty {
                Some(ctx.never_type())
            } else {
                Some(ty)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_env_new() {
        let env = TypeEnv::new();
        assert!(env.get("x").is_none());
    }

    #[test]
    fn test_type_env_set_get() {
        let mut env = TypeEnv::new();
        let mut ctx = TypeContext::new();
        let num_ty = ctx.number_type();

        env.set("x".to_string(), num_ty);
        assert_eq!(env.get("x"), Some(num_ty));
    }

    #[test]
    fn test_type_env_merge_same_type() {
        let mut ctx = TypeContext::new();
        let num_ty = ctx.number_type();

        let mut env1 = TypeEnv::new();
        env1.set("x".to_string(), num_ty);

        let mut env2 = TypeEnv::new();
        env2.set("x".to_string(), num_ty);

        let merged = env1.merge(&env2, &mut ctx);
        assert_eq!(merged.get("x"), Some(num_ty));
    }

    #[test]
    fn test_type_env_merge_different_types() {
        let mut ctx = TypeContext::new();
        let num_ty = ctx.number_type();
        let str_ty = ctx.string_type();

        let mut env1 = TypeEnv::new();
        env1.set("x".to_string(), num_ty);

        let mut env2 = TypeEnv::new();
        env2.set("x".to_string(), str_ty);

        let merged = env1.merge(&env2, &mut ctx);
        let merged_ty = merged.get("x").unwrap();

        // Should be a union of number and string
        match ctx.get(merged_ty).unwrap() {
            Type::Union(union_ty) => {
                assert!(union_ty.members.contains(&num_ty));
                assert!(union_ty.members.contains(&str_ty));
            }
            _ => panic!("Expected union type"),
        }
    }

    #[test]
    fn test_type_env_merge_only_in_one() {
        let mut ctx = TypeContext::new();
        let num_ty = ctx.number_type();

        let mut env1 = TypeEnv::new();
        env1.set("x".to_string(), num_ty);

        let env2 = TypeEnv::new();

        let merged = env1.merge(&env2, &mut ctx);
        // x only in env1, so not in merged
        assert!(merged.get("x").is_none());
    }

    #[test]
    fn test_apply_typeof_guard_string() {
        let mut ctx = TypeContext::new();
        let num_ty = ctx.number_type();
        let str_ty = ctx.string_type();
        let union_ty = ctx.union_type(vec![num_ty, str_ty]);

        let guard = TypeGuard::TypeOf {
            var: "x".to_string(),
            type_name: "string".to_string(),
            negated: false,
        };

        let narrowed = apply_type_guard(&mut ctx, union_ty, &guard).unwrap();
        assert_eq!(narrowed, str_ty);
    }

    #[test]
    fn test_apply_typeof_guard_negated() {
        let mut ctx = TypeContext::new();
        let num_ty = ctx.number_type();
        let str_ty = ctx.string_type();
        let union_ty = ctx.union_type(vec![num_ty, str_ty]);

        let guard = TypeGuard::TypeOf {
            var: "x".to_string(),
            type_name: "string".to_string(),
            negated: true,
        };

        let narrowed = apply_type_guard(&mut ctx, union_ty, &guard).unwrap();
        assert_eq!(narrowed, num_ty);
    }

    #[test]
    fn test_apply_nullish_guard() {
        let mut ctx = TypeContext::new();
        let str_ty = ctx.string_type();
        let null_ty = ctx.null_type();
        let union_ty = ctx.union_type(vec![str_ty, null_ty]);

        let guard = TypeGuard::Nullish {
            var: "x".to_string(),
            field: None,
            negated: true, // x !== null
        };

        let narrowed = apply_type_guard(&mut ctx, union_ty, &guard).unwrap();
        assert_eq!(narrowed, str_ty);
    }

    #[test]
    fn test_apply_nullish_guard_non_negated() {
        let mut ctx = TypeContext::new();
        let str_ty = ctx.string_type();
        let null_ty = ctx.null_type();
        let union_ty = ctx.union_type(vec![str_ty, null_ty]);

        let guard = TypeGuard::Nullish {
            var: "x".to_string(),
            field: None,
            negated: false, // x === null
        };

        let narrowed = apply_type_guard(&mut ctx, union_ty, &guard).unwrap();
        assert_eq!(narrowed, null_ty);
    }

    #[test]
    fn test_remove_from_union() {
        let mut ctx = TypeContext::new();
        let num_ty = ctx.number_type();
        let str_ty = ctx.string_type();
        let bool_ty = ctx.boolean_type();
        let union_ty = ctx.union_type(vec![num_ty, str_ty, bool_ty]);

        let result = remove_from_union(&mut ctx, union_ty, str_ty).unwrap();

        // Should be union of number and boolean
        match ctx.get(result).unwrap() {
            Type::Union(union_ty) => {
                assert_eq!(union_ty.members.len(), 2);
                assert!(union_ty.members.contains(&num_ty));
                assert!(union_ty.members.contains(&bool_ty));
            }
            _ => panic!("Expected union type"),
        }
    }
}

/// Apply Array.isArray guard
fn apply_is_array_guard(ctx: &mut TypeContext, ty: TypeId, negated: bool) -> Option<TypeId> {
    if negated {
        // !Array.isArray(x) - remove array types from union
        if let Some(Type::Union(union)) = ctx.get(ty) {
            let remaining: Vec<TypeId> = union
                .members
                .iter()
                .filter(|&&member| !matches!(ctx.get(member), Some(Type::Array(_))))
                .copied()
                .collect();

            if remaining.is_empty() {
                return Some(ctx.never_type());
            } else if remaining.len() == 1 {
                return Some(remaining[0]);
            } else {
                return Some(ctx.union_type(remaining));
            }
        }
        Some(ty)
    } else {
        // Array.isArray(x) - narrow to array type only
        if let Some(Type::Array(_)) = ctx.get(ty) {
            Some(ty)
        } else if let Some(Type::Union(union)) = ctx.get(ty) {
            // Find array members
            let arrays: Vec<TypeId> = union
                .members
                .iter()
                .filter(|&&member| matches!(ctx.get(member), Some(Type::Array(_))))
                .copied()
                .collect();

            if arrays.is_empty() {
                Some(ctx.never_type())
            } else if arrays.len() == 1 {
                Some(arrays[0])
            } else {
                Some(ctx.union_type(arrays))
            }
        } else {
            // Non-array type - narrowed to never
            Some(ctx.never_type())
        }
    }
}

/// Apply Number.isInteger guard
///
/// Note: In Raya, we don't distinguish int/float at runtime for bare unions,
/// but this can still be useful for documentation and future optimizations.
fn apply_is_integer_guard(ctx: &mut TypeContext, ty: TypeId, negated: bool) -> Option<TypeId> {
    use crate::parser::types::PrimitiveType;

    // For now, Number.isInteger just validates it's a number
    // In the future, we could track integer-ness more precisely
    if negated {
        // !Number.isInteger(x) - could be float, string, etc.
        // For bare unions containing number, this doesn't narrow much
        Some(ty)
    } else {
        // Number.isInteger(x) - narrow to number type
        if matches!(ctx.get(ty), Some(Type::Primitive(PrimitiveType::Number))) {
            Some(ty)
        } else if let Some(Type::Union(union)) = ctx.get(ty) {
            // Find number members
            let numbers: Vec<TypeId> = union
                .members
                .iter()
                .filter(|&&member| {
                    matches!(
                        ctx.get(member),
                        Some(Type::Primitive(PrimitiveType::Number))
                    )
                })
                .copied()
                .collect();

            if numbers.is_empty() {
                Some(ctx.never_type())
            } else if numbers.len() == 1 {
                Some(numbers[0])
            } else {
                Some(ctx.union_type(numbers))
            }
        } else {
            Some(ctx.never_type())
        }
    }
}

/// Apply Number.isNaN guard
fn apply_is_nan_guard(ctx: &mut TypeContext, ty: TypeId, negated: bool) -> Option<TypeId> {
    // NaN is a special number value
    // For negated (!Number.isNaN), we know it's not NaN but still could be number
    // For non-negated, we know it IS NaN

    if negated {
        // !Number.isNaN(x) - not NaN, but could be any other value
        Some(ty)
    } else {
        // Number.isNaN(x) - it's NaN (a number value)
        // In practice, this narrows to number type
        Some(ctx.number_type())
    }
}

/// Apply Number.isFinite guard
fn apply_is_finite_guard(ctx: &mut TypeContext, ty: TypeId, negated: bool) -> Option<TypeId> {
    use crate::parser::types::PrimitiveType;

    if negated {
        // !Number.isFinite(x) - could be Infinity, NaN, or non-number
        Some(ty)
    } else {
        // Number.isFinite(x) - finite number (excludes Infinity and NaN)
        // Narrows to number type
        if matches!(ctx.get(ty), Some(Type::Primitive(PrimitiveType::Number))) {
            Some(ty)
        } else if let Some(Type::Union(union)) = ctx.get(ty) {
            let numbers: Vec<TypeId> = union
                .members
                .iter()
                .filter(|&&member| {
                    matches!(
                        ctx.get(member),
                        Some(Type::Primitive(PrimitiveType::Number))
                    )
                })
                .copied()
                .collect();

            if numbers.is_empty() {
                Some(ctx.never_type())
            } else if numbers.len() == 1 {
                Some(numbers[0])
            } else {
                Some(ctx.union_type(numbers))
            }
        } else {
            Some(ctx.never_type())
        }
    }
}

/// Apply custom type predicate guard (isString, isObject, etc.)
fn apply_type_predicate_guard(
    ctx: &mut TypeContext,
    ty: TypeId,
    predicate: &str,
    negated: bool,
) -> Option<TypeId> {
    // Map predicate names to types
    let target_ty = match predicate {
        "isString" => Some(ctx.string_type()),
        "isNumber" => Some(ctx.number_type()),
        "isBoolean" => Some(ctx.boolean_type()),
        "isNull" => Some(ctx.null_type()),
        "isObject" => return narrow_by_predicate(ctx, ty, negated, is_object_like_type),
        "isFunction" => return narrow_by_predicate(ctx, ty, negated, is_function_type),
        _ => None,
    };

    let target_ty = target_ty?;

    if negated {
        // !isString(x) - remove string from union
        remove_from_union(ctx, ty, target_ty)
    } else {
        // isString(x) - narrow to string
        Some(target_ty)
    }
}
