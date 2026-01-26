//! Type normalization utilities
//!
//! Provides functions to simplify and normalize types into canonical forms.

use super::context::TypeContext;
use super::ty::{Type, TypeId};

/// Normalize a type to its canonical form
///
/// Normalization includes:
/// - Flattening nested unions
/// - Removing duplicate union members
/// - Sorting union members for consistency
/// - Simplifying single-member unions
/// - Removing `never` from unions (unless it's the only member)
pub fn normalize_type(ctx: &mut TypeContext, ty: TypeId) -> TypeId {
    let ty_data = match ctx.get(ty) {
        Some(t) => t.clone(),
        None => return ty,
    };

    match ty_data {
        Type::Union(union) => normalize_union(ctx, &union.members),
        Type::Array(arr) => {
            let elem = normalize_type(ctx, arr.element);
            ctx.array_type(elem)
        }
        Type::Tuple(tuple) => {
            let elements: Vec<_> = tuple
                .elements
                .iter()
                .map(|&e| normalize_type(ctx, e))
                .collect();
            ctx.tuple_type(elements)
        }
        Type::Function(func) => {
            let params: Vec<_> = func
                .params
                .iter()
                .map(|&p| normalize_type(ctx, p))
                .collect();
            let return_type = normalize_type(ctx, func.return_type);
            ctx.function_type(params, return_type, func.is_async)
        }
        _ => ty,
    }
}

/// Normalize a union type
fn normalize_union(
    ctx: &mut TypeContext,
    members: &[TypeId],
) -> TypeId {
    let never = ctx.never_type();
    let mut normalized_members = Vec::new();

    // Flatten nested unions and collect all members
    for &member in members {
        let member_normalized = normalize_type(ctx, member);

        if let Some(Type::Union(nested)) = ctx.get(member_normalized) {
            // Flatten nested union
            for &nested_member in &nested.members {
                normalized_members.push(nested_member);
            }
        } else {
            normalized_members.push(member_normalized);
        }
    }

    // Remove `never` from union (unless it's the only member)
    if normalized_members.len() > 1 {
        normalized_members.retain(|&m| m != never);
    }

    // If empty after removing never, return never
    if normalized_members.is_empty() {
        return never;
    }

    // Sort for consistency
    normalized_members.sort_unstable_by_key(|id| id.0);

    // Remove duplicates
    normalized_members.dedup();

    // Single member union is just the member
    if normalized_members.len() == 1 {
        return normalized_members[0];
    }

    // Discriminant will be inferred automatically
    ctx.union_type(normalized_members)
}

/// Simplify a type by removing unnecessary complexity
///
/// This is more aggressive than normalization and may change semantics slightly.
pub fn simplify_type(ctx: &mut TypeContext, ty: TypeId) -> TypeId {
    let ty_data = match ctx.get(ty) {
        Some(t) => t.clone(),
        None => return ty,
    };

    match ty_data {
        Type::Union(union) => {
            let unknown = ctx.unknown_type();

            // If union contains unknown, the whole thing is unknown
            if union.members.contains(&unknown) {
                return unknown;
            }

            // Otherwise normalize
            normalize_union(ctx, &union.members)
        }
        _ => normalize_type(ctx, ty),
    }
}

/// Check if a type is a subtype of unknown (i.e., any type except never)
pub fn is_concrete_type(ctx: &TypeContext, ty: TypeId) -> bool {
    !matches!(ctx.get(ty), Some(Type::Never))
}

/// Check if a type contains type variables
pub fn contains_type_variables(ctx: &TypeContext, ty: TypeId) -> bool {
    let ty_data = match ctx.get(ty) {
        Some(t) => t,
        None => return false,
    };

    match ty_data {
        Type::TypeVar(_) => true,
        Type::Array(arr) => contains_type_variables(ctx, arr.element),
        Type::Tuple(tuple) => tuple.elements.iter().any(|&e| contains_type_variables(ctx, e)),
        Type::Function(func) => {
            func.params.iter().any(|&p| contains_type_variables(ctx, p))
                || contains_type_variables(ctx, func.return_type)
        }
        Type::Union(union) => union.members.iter().any(|&m| contains_type_variables(ctx, m)),
        Type::Generic(gen) => {
            contains_type_variables(ctx, gen.base)
                || gen.type_args.iter().any(|&a| contains_type_variables(ctx, a))
        }
        _ => false,
    }
}

/// Get the arity (number of parameters) of a function type
pub fn function_arity(ctx: &TypeContext, ty: TypeId) -> Option<usize> {
    match ctx.get(ty) {
        Some(Type::Function(func)) => Some(func.params.len()),
        _ => None,
    }
}

/// Check if a type is a function type
pub fn is_function_type(ctx: &TypeContext, ty: TypeId) -> bool {
    matches!(ctx.get(ty), Some(Type::Function(_)))
}

/// Check if a type is a union type
pub fn is_union_type(ctx: &TypeContext, ty: TypeId) -> bool {
    matches!(ctx.get(ty), Some(Type::Union(_)))
}

/// Get all primitive types from a union (if it's a bare primitive union)
pub fn extract_primitive_union_members(
    ctx: &TypeContext,
    ty: TypeId,
) -> Option<Vec<super::ty::PrimitiveType>> {
    match ctx.get(ty) {
        Some(Type::Union(union)) => {
            let mut primitives = Vec::new();
            for &member in &union.members {
                match ctx.get(member) {
                    Some(Type::Primitive(p)) => primitives.push(*p),
                    _ => return None, // Not a bare primitive union
                }
            }
            Some(primitives)
        }
        Some(Type::Primitive(p)) => Some(vec![*p]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ty::PrimitiveType;

    #[test]
    fn test_normalize_flat_union() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();
        let str = ctx.string_type();
        let union = ctx.union_type(vec![num, str]);

        let normalized = normalize_type(&mut ctx, union);
        assert_eq!(normalized, union);
    }

    #[test]
    fn test_normalize_nested_union() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();
        let str = ctx.string_type();
        let bool_ty = ctx.boolean_type();

        // Create (num | str) | bool
        let union1 = ctx.union_type(vec![num, str]);
        let union2 = ctx.union_type(vec![union1, bool_ty]);

        let normalized = normalize_type(&mut ctx, union2);

        // Should be flattened to num | str | bool
        match ctx.get(normalized) {
            Some(Type::Union(u)) => {
                assert_eq!(u.members.len(), 3);
                assert!(u.members.contains(&num));
                assert!(u.members.contains(&str));
                assert!(u.members.contains(&bool_ty));
            }
            _ => panic!("Expected union type"),
        }
    }

    #[test]
    fn test_normalize_removes_duplicates() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();
        let str = ctx.string_type();

        // Create num | str | num
        let union = ctx.union_type(vec![num, str, num]);

        let normalized = normalize_type(&mut ctx, union);

        match ctx.get(normalized) {
            Some(Type::Union(u)) => {
                assert_eq!(u.members.len(), 2);
                assert!(u.members.contains(&num));
                assert!(u.members.contains(&str));
            }
            _ => panic!("Expected union type"),
        }
    }

    #[test]
    fn test_normalize_single_member_union() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();

        let union = ctx.union_type(vec![num]);
        let normalized = normalize_type(&mut ctx, union);

        assert_eq!(normalized, num);
    }

    #[test]
    fn test_normalize_removes_never_from_union() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();
        let never = ctx.never_type();

        let union = ctx.union_type(vec![num, never]);
        let normalized = normalize_type(&mut ctx, union);

        // Should be just num
        assert_eq!(normalized, num);
    }

    #[test]
    fn test_normalize_never_only_union() {
        let mut ctx = TypeContext::new();
        let never = ctx.never_type();

        let union = ctx.union_type(vec![never]);
        let normalized = normalize_type(&mut ctx, union);

        // Should be never
        assert_eq!(normalized, never);
    }

    #[test]
    fn test_simplify_union_with_unknown() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();
        let unknown = ctx.unknown_type();

        let union = ctx.union_type(vec![num, unknown]);
        let simplified = simplify_type(&mut ctx, union);

        // Should be unknown
        assert_eq!(simplified, unknown);
    }

    #[test]
    fn test_contains_type_variables() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();

        assert!(!contains_type_variables(&ctx, num));

        let t_var = ctx.intern(Type::TypeVar(crate::types::ty::TypeVar {
            name: "T".to_string(),
            constraint: None,
            default: None,
        }));

        assert!(contains_type_variables(&ctx, t_var));

        let array_t = ctx.array_type(t_var);
        assert!(contains_type_variables(&ctx, array_t));
    }

    #[test]
    fn test_function_arity() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();
        let str = ctx.string_type();

        let func = ctx.function_type(vec![num, str], num, false);
        assert_eq!(function_arity(&ctx, func), Some(2));

        assert_eq!(function_arity(&ctx, num), None);
    }

    #[test]
    fn test_is_function_type() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();

        let func = ctx.function_type(vec![num], num, false);
        assert!(is_function_type(&ctx, func));
        assert!(!is_function_type(&ctx, num));
    }

    #[test]
    fn test_extract_primitive_union_members() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();
        let str = ctx.string_type();

        let union = ctx.union_type(vec![num, str]);
        let primitives = extract_primitive_union_members(&ctx, union);

        assert_eq!(
            primitives,
            Some(vec![PrimitiveType::Number, PrimitiveType::String])
        );
    }

    #[test]
    fn test_extract_primitive_single() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();

        let primitives = extract_primitive_union_members(&ctx, num);
        assert_eq!(primitives, Some(vec![PrimitiveType::Number]));
    }

    #[test]
    fn test_extract_non_primitive_union() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();
        let arr = ctx.array_type(num);

        let union = ctx.union_type(vec![num, arr]);
        let primitives = extract_primitive_union_members(&ctx, union);

        assert_eq!(primitives, None);
    }
}
