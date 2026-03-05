//! Assignability and implicit type coercions
//!
//! Implements the assignability relation T ~> U (T is assignable to U).
//! This includes both subtyping and implicit primitive coercions.

use super::context::TypeContext;
use super::subtyping::SubtypingContext;
use super::ty::{GenericType, PrimitiveType, Type, TypeId};
use rustc_hash::FxHashSet;

fn jsobject_generic_inner(type_ctx: &TypeContext, generic: &GenericType) -> Option<TypeId> {
    if generic.type_args.len() != 1 {
        return None;
    }
    match type_ctx.get(generic.base) {
        Some(Type::JSObject) => generic.type_args.first().copied(),
        _ => None,
    }
}

/// Context for checking assignability
#[derive(Debug)]
pub struct AssignabilityContext<'a> {
    /// Type context for resolving types
    type_ctx: &'a TypeContext,

    /// Subtyping context
    subtyping: SubtypingContext<'a>,
    /// Strict mode flag toggles TS-strict assignability behavior.
    strict_mode: bool,
    /// In-progress assignability checks for recursive structural types.
    active_pairs: FxHashSet<(u32, u32)>,
    /// Memoized assignability results for recursive structural comparisons.
    pair_cache: rustc_hash::FxHashMap<(u32, u32), bool>,
}

impl<'a> AssignabilityContext<'a> {
    /// Create a new assignability context
    pub fn new(type_ctx: &'a TypeContext) -> Self {
        AssignabilityContext {
            type_ctx,
            subtyping: SubtypingContext::new(type_ctx).with_relaxed_function_call_arity(true),
            strict_mode: false,
            active_pairs: FxHashSet::default(),
            pair_cache: rustc_hash::FxHashMap::default(),
        }
    }

    /// Create assignability context with explicit strict-mode behavior.
    pub fn with_strict_mode(type_ctx: &'a TypeContext, strict_mode: bool) -> Self {
        AssignabilityContext {
            type_ctx,
            subtyping: SubtypingContext::new(type_ctx).with_relaxed_function_call_arity(true),
            strict_mode,
            active_pairs: FxHashSet::default(),
            pair_cache: rustc_hash::FxHashMap::default(),
        }
    }

    /// Check if `source` is assignable to `target` (source ~> target)
    ///
    /// This includes both subtyping and implicit primitive coercions:
    /// - number ~> string (implicit toString)
    /// - boolean ~> string (implicit toString)
    /// - null ~> string (implicit toString)
    pub fn is_assignable(&mut self, source: TypeId, target: TypeId) -> bool {
        let pair = (source.0, target.0);
        if let Some(&cached) = self.pair_cache.get(&pair) {
            return cached;
        }
        if !self.active_pairs.insert(pair) {
            return true;
        }

        // First check subtyping
        if self.subtyping.is_subtype(source, target) {
            self.pair_cache.insert(pair, true);
            self.active_pairs.remove(&pair);
            return true;
        }

        // Check implicit primitive coercions
        let source_ty = match self.type_ctx.get(source) {
            Some(ty) => ty,
            None => {
                self.pair_cache.insert(pair, false);
                self.active_pairs.remove(&pair);
                return false;
            }
        };

        let target_ty = match self.type_ctx.get(target) {
            Some(ty) => ty,
            None => {
                self.pair_cache.insert(pair, false);
                self.active_pairs.remove(&pair);
                return false;
            }
        };

        let is_object_like = |ty: &Type| {
            matches!(
                ty,
                Type::Object(_)
                    | Type::Class(_)
                    | Type::Interface(_)
                    | Type::Map(_)
                    | Type::Set(_)
                    | Type::Array(_)
                    | Type::Tuple(_)
                    | Type::Json
                    | Type::JSObject
            )
        };

        let result =
            match (source_ty, target_ty) {
                // Node-compat dynamic `any`: assignable to/from everything.
                (Type::Any, _) | (_, Type::Any) => true,

                // TypeVar (unresolved generic) is compatible with any type
                // Raya uses monomorphization, so generics are resolved at compile time.
                // The checker runs before monomorphization and shouldn't reject
                // assignments involving unresolved type parameters.
                (Type::TypeVar(_), _) | (_, Type::TypeVar(_)) => true,

                // JSObject is a permissive object-ish fallback type used in node-compat.
                (Type::JSObject, t) | (t, Type::JSObject) if is_object_like(t) => true,
                (Type::Generic(g), t)
                    if jsobject_generic_inner(self.type_ctx, g).is_some() && is_object_like(t) =>
                {
                    true
                }
                (t, Type::Generic(g))
                    if jsobject_generic_inner(self.type_ctx, g).is_some() && is_object_like(t) =>
                {
                    true
                }

                // In non-strict mode, unknown acts like permissive top/bottom for compatibility.
                // In strict mode, this path is disabled and subtyping still allows T <: unknown
                // while blocking unknown -> concrete T without cast/narrowing.
                (Type::Unknown, _) | (_, Type::Unknown) if !self.strict_mode => true,

                // Implicit primitive-to-string coercions are non-strict only.
                (
                    Type::Primitive(PrimitiveType::Number),
                    Type::Primitive(PrimitiveType::String),
                ) if !self.strict_mode => true,

                // int ~> string
                (Type::Primitive(PrimitiveType::Int), Type::Primitive(PrimitiveType::String))
                    if !self.strict_mode =>
                {
                    true
                }

                // boolean ~> string
                (
                    Type::Primitive(PrimitiveType::Boolean),
                    Type::Primitive(PrimitiveType::String),
                ) if !self.strict_mode => true,

                // null ~> string
                (Type::Primitive(PrimitiveType::Null), Type::Primitive(PrimitiveType::String))
                    if !self.strict_mode =>
                {
                    true
                }

                // Union assignability: T1 | T2 | ... | Tn ~> U if Ti ~> U for all i
                (Type::Union(union), _) => union
                    .members
                    .iter()
                    .all(|&member| self.is_assignable(member, target)),

                // Assignability to union: T ~> U1 | U2 | ... | Un if T ~> Ui for some i
                (_, Type::Union(union)) => union
                    .members
                    .iter()
                    .any(|&member| self.is_assignable(source, member)),

                _ => false,
            };

        self.pair_cache.insert(pair, result);
        self.active_pairs.remove(&pair);
        result
    }

    /// Check if an implicit coercion is needed
    ///
    /// Returns true if source is assignable to target but not a subtype
    /// (i.e., requires an implicit coercion)
    pub fn needs_coercion(&mut self, source: TypeId, target: TypeId) -> bool {
        self.is_assignable(source, target) && !self.subtyping.is_subtype(source, target)
    }

    /// Get the coercion kind if one is needed
    pub fn get_coercion(&mut self, source: TypeId, target: TypeId) -> Option<CoercionKind> {
        if !self.needs_coercion(source, target) {
            return None;
        }

        let source_ty = self.type_ctx.get(source)?;
        let target_ty = self.type_ctx.get(target)?;

        match (source_ty, target_ty) {
            (Type::Primitive(PrimitiveType::Number), Type::Primitive(PrimitiveType::String)) => {
                Some(CoercionKind::NumberToString)
            }
            (Type::Primitive(PrimitiveType::Int), Type::Primitive(PrimitiveType::String)) => {
                Some(CoercionKind::IntToString)
            }
            (Type::Primitive(PrimitiveType::Boolean), Type::Primitive(PrimitiveType::String)) => {
                Some(CoercionKind::BooleanToString)
            }
            (Type::Primitive(PrimitiveType::Null), Type::Primitive(PrimitiveType::String)) => {
                Some(CoercionKind::NullToString)
            }
            _ => None,
        }
    }
}

/// Kind of implicit coercion
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoercionKind {
    /// number → string
    NumberToString,
    /// int → string
    IntToString,
    /// boolean → string
    BooleanToString,
    /// null → string
    NullToString,
}

impl CoercionKind {
    /// Get the name of this coercion
    pub fn name(&self) -> &'static str {
        match self {
            CoercionKind::NumberToString => "number_to_string",
            CoercionKind::IntToString => "int_to_string",
            CoercionKind::BooleanToString => "boolean_to_string",
            CoercionKind::NullToString => "null_to_string",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::types::context::TypeContext;

    #[test]
    fn test_subtyping_is_assignable() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();
        let unknown = ctx.unknown_type();

        let mut assign_ctx = AssignabilityContext::new(&ctx);

        // Subtyping implies assignability
        assert!(assign_ctx.is_assignable(num, unknown));
    }

    #[test]
    fn test_number_to_string_coercion() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();
        let str = ctx.string_type();

        let mut assign_ctx = AssignabilityContext::new(&ctx);

        assert!(assign_ctx.is_assignable(num, str));
        assert!(assign_ctx.needs_coercion(num, str));
        assert_eq!(
            assign_ctx.get_coercion(num, str),
            Some(CoercionKind::NumberToString)
        );
    }

    #[test]
    fn test_boolean_to_string_coercion() {
        let mut ctx = TypeContext::new();
        let bool_ty = ctx.boolean_type();
        let str = ctx.string_type();

        let mut assign_ctx = AssignabilityContext::new(&ctx);

        assert!(assign_ctx.is_assignable(bool_ty, str));
        assert!(assign_ctx.needs_coercion(bool_ty, str));
        assert_eq!(
            assign_ctx.get_coercion(bool_ty, str),
            Some(CoercionKind::BooleanToString)
        );
    }

    #[test]
    fn test_null_to_string_coercion() {
        let mut ctx = TypeContext::new();
        let null = ctx.null_type();
        let str = ctx.string_type();

        let mut assign_ctx = AssignabilityContext::new(&ctx);

        assert!(assign_ctx.is_assignable(null, str));
        assert!(assign_ctx.needs_coercion(null, str));
        assert_eq!(
            assign_ctx.get_coercion(null, str),
            Some(CoercionKind::NullToString)
        );
    }

    #[test]
    fn test_no_string_to_number_coercion() {
        let mut ctx = TypeContext::new();
        let str = ctx.string_type();
        let num = ctx.number_type();

        let mut assign_ctx = AssignabilityContext::new(&ctx);

        // string is NOT assignable to number
        assert!(!assign_ctx.is_assignable(str, num));
    }

    #[test]
    fn test_union_assignability() {
        let mut ctx = TypeContext::new();

        let num = ctx.number_type();
        let str = ctx.string_type();
        let union = ctx.union_type(vec![num, str]);

        let mut assign_ctx = AssignabilityContext::new(&ctx);

        // number ~> number | string
        assert!(assign_ctx.is_assignable(num, union));

        // number | string ~> string (both members coerce to string)
        assert!(assign_ctx.is_assignable(union, str));
    }

    #[test]
    fn test_no_coercion_for_subtyping() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();

        let mut assign_ctx = AssignabilityContext::new(&ctx);

        assert!(assign_ctx.is_assignable(num, num));
        assert!(!assign_ctx.needs_coercion(num, num));
        assert_eq!(assign_ctx.get_coercion(num, num), None);
    }
}
