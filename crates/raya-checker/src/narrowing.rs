//! Type narrowing engine for control flow analysis
//!
//! This module provides type narrowing based on type guards detected
//! in conditional expressions. It tracks narrowed types through control
//! flow branches and merges environments at join points.

use rustc_hash::FxHashMap;
use raya_types::{Type, TypeContext, TypeId};
use crate::type_guards::TypeGuard;

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
    pub fn merge(
        &self,
        other: &TypeEnv,
        ctx: &mut TypeContext,
    ) -> TypeEnv {
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
                    ctx.union_type(vec![*ty1, *ty2], None)
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
pub fn apply_type_guard(
    ctx: &mut TypeContext,
    ty: TypeId,
    guard: &TypeGuard,
) -> Option<TypeId> {
    match guard {
        TypeGuard::TypeOf { type_name, negated, .. } => {
            apply_typeof_guard(ctx, ty, type_name, *negated)
        }
        TypeGuard::Discriminant { field, variant, negated, .. } => {
            apply_discriminant_guard(ctx, ty, field, variant, *negated)
        }
        TypeGuard::Nullish { negated, .. } => {
            apply_nullish_guard(ctx, ty, *negated)
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
    // Map type name to TypeId
    let target_ty = match type_name {
        "string" => ctx.string_type(),
        "number" => ctx.number_type(),
        "boolean" => ctx.boolean_type(),
        "function" => return Some(ty), // TODO: filter to function types only
        "object" => return Some(ty),   // TODO: filter to object types only
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

/// Apply a discriminant guard to narrow a discriminated union
fn apply_discriminant_guard(
    ctx: &mut TypeContext,
    ty: TypeId,
    field: &str,
    variant: &str,
    negated: bool,
) -> Option<TypeId> {
    // Get the type definition
    let type_def = ctx.get(ty)?.clone();

    match type_def {
        Type::Union(union_ty) => {
            // Filter union members based on discriminant
            let mut matching_members = Vec::new();

            for member_id in &union_ty.members {
                if let Some(Type::Object(obj)) = ctx.get(*member_id) {
                    // Check if this member has the discriminant field
                    if let Some(prop) = obj.properties.iter().find(|p| p.name == field) {
                        // For now, we assume discriminant fields match by property name
                        // In a full implementation, we'd check the actual value
                        // Since we don't have string literal types yet, we just check field existence
                        if !negated {
                            matching_members.push(*member_id);
                        }
                    } else if negated {
                        // Doesn't have the field - matches negated check
                        matching_members.push(*member_id);
                    }
                } else if negated {
                    // Non-object members pass negated checks
                    matching_members.push(*member_id);
                }
            }

            if matching_members.is_empty() {
                // No matching variants - unreachable code
                Some(ctx.never_type())
            } else if matching_members.len() == 1 {
                // Single variant - return it directly
                Some(matching_members[0])
            } else {
                // Multiple variants - return union
                Some(ctx.union_type(matching_members, None))
            }
        }
        Type::Object(obj) => {
            // Single object type - check discriminant
            if obj.properties.iter().any(|p| p.name == field) {
                // Has the discriminant field
                if !negated {
                    return Some(ty); // Matches, keep type
                } else {
                    return Some(ctx.never_type()); // Doesn't match, unreachable
                }
            }
            Some(ty) // No discriminant field, no narrowing
        }
        _ => {
            // Not a discriminated union
            Some(ty)
        }
    }
}

/// Apply a nullish guard (x !== null or x === null)
fn apply_nullish_guard(
    ctx: &mut TypeContext,
    ty: TypeId,
    negated: bool,
) -> Option<TypeId> {
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
fn remove_from_union(
    ctx: &mut TypeContext,
    ty: TypeId,
    remove_ty: TypeId,
) -> Option<TypeId> {
    let type_def = ctx.get(ty)?.clone();

    match type_def {
        Type::Union(union_ty) => {
            // Filter out the type to remove
            let remaining: Vec<TypeId> = union_ty.members
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
                Some(ctx.union_type(remaining, None))
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
        let union_ty = ctx.union_type(vec![num_ty, str_ty], None);

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
        let union_ty = ctx.union_type(vec![num_ty, str_ty], None);

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
        let union_ty = ctx.union_type(vec![str_ty, null_ty], None);

        let guard = TypeGuard::Nullish {
            var: "x".to_string(),
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
        let union_ty = ctx.union_type(vec![str_ty, null_ty], None);

        let guard = TypeGuard::Nullish {
            var: "x".to_string(),
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
        let union_ty = ctx.union_type(vec![num_ty, str_ty, bool_ty], None);

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
