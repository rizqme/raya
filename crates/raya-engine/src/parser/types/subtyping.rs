//! Subtyping rules for the Raya type system
//!
//! Implements the subtyping relation T <: U (T is a subtype of U).

use super::context::TypeContext;
use super::ty::{PrimitiveType, Type, TypeId};
use rustc_hash::FxHashMap;

/// Context for checking subtyping relationships
///
/// Maintains a substitution map for type variables during checking.
#[derive(Debug, Clone)]
pub struct SubtypingContext<'a> {
    /// Type context for resolving types
    type_ctx: &'a TypeContext,

    /// Current type variable substitutions
    type_vars: FxHashMap<String, TypeId>,
}

impl<'a> SubtypingContext<'a> {
    /// Create a new subtyping context
    pub fn new(type_ctx: &'a TypeContext) -> Self {
        SubtypingContext {
            type_ctx,
            type_vars: FxHashMap::default(),
        }
    }

    /// Check if `sub` is a subtype of `sup` (sub <: sup)
    ///
    /// Returns true if a value of type `sub` can be used where `sup` is expected.
    pub fn is_subtype(&mut self, sub: TypeId, sup: TypeId) -> bool {
        // Reflexivity: T <: T
        if sub == sup {
            return true;
        }

        let sub_ty = match self.type_ctx.get(sub) {
            Some(ty) => ty,
            None => return false,
        };

        let sup_ty = match self.type_ctx.get(sup) {
            Some(ty) => ty,
            None => return false,
        };

        // Bridge specialized builtin container/runtime types with their class-type names.
        // In checker/runtime-prelude flows we can see either side represented as:
        // - Type::Map/Set/Channel/Task/Array (specialized builtin types), or
        // - Type::Class { name: "Map" | "Set" | ... } (from class declarations).
        // They denote the same runtime entities and should be mutually compatible.
        if self.is_builtin_class_bridge(sub_ty, sup_ty)
            || self.is_builtin_class_bridge(sup_ty, sub_ty)
        {
            return true;
        }

        match (sub_ty, sup_ty) {
            // Never is subtype of everything
            (Type::Never, _) => true,

            // Everything is subtype of Unknown (Unknown is top type)
            (_, Type::Unknown) => true,

            // json is a dynamic duck-typed value from JSON.parse()/decode.
            // Allow bidirectional compatibility with other types to support
            // gradual typing and explicit target annotations.
            (Type::Json, _) => true,
            (_, Type::Json) => true,

            // Primitive subtyping (reflexive + int <-> number interop)
            (Type::Primitive(p1), Type::Primitive(p2)) => {
                p1 == p2
                    || matches!(
                        (p1, p2),
                        (PrimitiveType::Int, PrimitiveType::Number)
                            | (PrimitiveType::Number, PrimitiveType::Int)
                    )
            }

            // Literal type subtyping: "ok" <: string, 42 <: number, true <: boolean
            (Type::StringLiteral(_), Type::Primitive(PrimitiveType::String)) => true,
            (Type::NumberLiteral(_), Type::Primitive(PrimitiveType::Number)) => true,
            (Type::BooleanLiteral(_), Type::Primitive(PrimitiveType::Boolean)) => true,

            // Widening: string <: "ok" (allows assignment of string values to literal types)
            (Type::Primitive(PrimitiveType::String), Type::StringLiteral(_)) => true,
            (Type::Primitive(PrimitiveType::Number), Type::NumberLiteral(_)) => true,
            (Type::Primitive(PrimitiveType::Boolean), Type::BooleanLiteral(_)) => true,

            // Literal type reflexivity (same literal values)
            (Type::StringLiteral(a), Type::StringLiteral(b)) => a == b,
            (Type::NumberLiteral(a), Type::NumberLiteral(b)) => a == b,
            (Type::BooleanLiteral(a), Type::BooleanLiteral(b)) => a == b,

            // Union subtyping: T <: U1 | U2 | ... | Un if T <: Ui for some i
            (_, Type::Union(union)) => union
                .members
                .iter()
                .any(|&member| self.is_subtype(sub, member)),

            // Union subtyping: T1 | T2 | ... | Tn <: U if Ti <: U for all i
            (Type::Union(union), _) => union
                .members
                .iter()
                .all(|&member| self.is_subtype(member, sup)),

            // Function subtyping (contravariant in parameters, covariant in return type)
            // (P1, P2, ..., Pn) => R <: (Q1, Q2, ..., Qm) => S
            // if m = n, Qi <: Pi for all i (contravariant), and R <: S (covariant)
            (Type::Function(f1), Type::Function(f2)) => {
                if f1.params.len() != f2.params.len() {
                    return false;
                }

                // Parameters are contravariant: sup params <: sub params
                let params_match = f1
                    .params
                    .iter()
                    .zip(&f2.params)
                    .all(|(&p1, &p2)| self.is_subtype(p2, p1)); // Note: reversed!

                // Return type is covariant, comparing effective returns:
                // - async fn (...): T is treated as (... ) => Task<T>
                // - sync fn (...): R is treated as (... ) => R
                let return_match = self.is_function_return_subtype(
                    f1.return_type,
                    f1.is_async,
                    f2.return_type,
                    f2.is_async,
                );

                params_match && return_match
            }

            // Array subtyping: T[] <: U[] if T <: U
            (Type::Array(a1), Type::Array(a2)) => self.is_subtype(a1.element, a2.element),

            // Task subtyping: Task<T> <: Task<U> if T <: U (covariant)
            (Type::Task(t1), Type::Task(t2)) => self.is_subtype(t1.result, t2.result),

            // Tuple subtyping: [T1, T2, ..., Tn] <: [U1, U2, ..., Um]
            // if n = m and Ti <: Ui for all i
            (Type::Tuple(t1), Type::Tuple(t2)) => {
                if t1.elements.len() != t2.elements.len() {
                    return false;
                }

                t1.elements
                    .iter()
                    .zip(&t2.elements)
                    .all(|(&e1, &e2)| self.is_subtype(e1, e2))
            }

            // Object subtyping (structural): width and depth subtyping
            // { x: T, y: U } <: { x: T } (width)
            // { x: S } <: { x: T } if S <: T (depth)
            (Type::Object(o1), Type::Object(o2)) => {
                // All properties in o2 must be in o1 with subtypes
                o2.properties.iter().all(|p2| {
                    o1.properties.iter().any(|p1| {
                        p1.name == p2.name
                            && p1.optional == p2.optional
                            && (!p2.readonly || p1.readonly) // readonly in sup => readonly in sub
                            && self.is_subtype(p1.ty, p2.ty)
                    })
                })
            }

            // Class <: Object (structural): class instance <: object type
            // Optional properties in the target object type don't need to exist in the class
            (Type::Class(c), Type::Object(o)) => {
                o.properties.iter().all(|op| {
                    // If the target property is optional, the class doesn't need to have it
                    if op.optional {
                        return true;
                    }
                    // For required properties, check class properties
                    c.properties.iter().any(|cp| {
                        cp.name == op.name
                            && !cp.optional // Class property must not be optional for required target
                            && (!op.readonly || cp.readonly)
                            && self.is_subtype(cp.ty, op.ty)
                    })
                    // Also check class methods (methods are stored separately from properties)
                    || c.methods.iter().any(|cm| {
                        cm.name == op.name && self.is_subtype(cm.ty, op.ty)
                    })
                })
            }

            // Object <: Object-like Class (structural)
            // Optional properties in the class don't need to exist in the object
            (Type::Object(o), Type::Class(c)) => c.properties.iter().all(|cp| {
                // If the class property is optional, the object doesn't need to have it
                if cp.optional {
                    return true;
                }
                // For required class properties, the object must have them
                o.properties.iter().any(|op| {
                    op.name == cp.name
                        && !op.optional // Object property must not be optional for required class
                        && self.is_subtype(op.ty, cp.ty)
                })
            }),

            // Class subtyping (nominal): only through extends/implements
            (Type::Class(c1), Type::Class(c2)) => {
                if c1.name == c2.name {
                    return true;
                }

                // Check if c1 extends c2
                if let Some(parent) = c1.extends {
                    if self.is_subtype(parent, sup) {
                        return true;
                    }
                }

                // Check if c1 implements c2
                c1.implements
                    .iter()
                    .any(|&impl_id| self.is_subtype(impl_id, sup))
            }

            // Class <: Interface (structural subtyping for interfaces)
            (Type::Class(c), Type::Interface(i)) => {
                // Check if class implements all interface members
                i.properties.iter().all(|ip| {
                    // If the interface property is optional, the class doesn't need to have it
                    if ip.optional {
                        return true;
                    }
                    // For required properties, check class properties
                    c.properties.iter().any(|cp| {
                        cp.name == ip.name
                            && !cp.optional // Class property must not be optional for required interface
                            && (!ip.readonly || cp.readonly)
                            && self.is_subtype(cp.ty, ip.ty)
                    })
                }) && i.methods.iter().all(|im| {
                    c.methods
                        .iter()
                        .any(|cm| cm.name == im.name && self.is_subtype(cm.ty, im.ty))
                })
            }

            // Interface subtyping (structural)
            (Type::Interface(i1), Type::Interface(i2)) => {
                // Check properties
                let props_match = i2.properties.iter().all(|p2| {
                    i1.properties.iter().any(|p1| {
                        p1.name == p2.name
                            && p1.optional == p2.optional
                            && (!p2.readonly || p1.readonly)
                            && self.is_subtype(p1.ty, p2.ty)
                    })
                });

                // Check methods
                let methods_match = i2.methods.iter().all(|m2| {
                    i1.methods
                        .iter()
                        .any(|m1| m1.name == m2.name && self.is_subtype(m1.ty, m2.ty))
                });

                props_match && methods_match
            }

            // Type variable subtyping
            (Type::TypeVar(tv), _) => {
                // If we have a substitution, use it
                if let Some(&substitution) = self.type_vars.get(&tv.name) {
                    return self.is_subtype(substitution, sup);
                }

                // Check constraint
                if let Some(constraint) = tv.constraint {
                    return self.is_subtype(constraint, sup);
                }

                false
            }

            (_, Type::TypeVar(tv)) => {
                // If we have a substitution, use it
                if let Some(&substitution) = self.type_vars.get(&tv.name) {
                    return self.is_subtype(sub, substitution);
                }

                false
            }

            // Generic type subtyping (invariant)
            // Map<K1, V1> <: Map<K2, V2> if K1 = K2 and V1 = V2
            (Type::Generic(g1), Type::Generic(g2)) => {
                if g1.base != g2.base || g1.type_args.len() != g2.type_args.len() {
                    return false;
                }

                // Type arguments must be equal (invariant)
                g1.type_args
                    .iter()
                    .zip(&g2.type_args)
                    .all(|(&a1, &a2)| a1 == a2)
            }

            // Reference types
            (Type::Reference(r1), Type::Reference(r2)) => {
                if r1.name != r2.name {
                    return false;
                }

                // Check type arguments if present
                match (&r1.type_args, &r2.type_args) {
                    (Some(args1), Some(args2)) => {
                        if args1.len() != args2.len() {
                            return false;
                        }
                        args1.iter().zip(args2).all(|(&a1, &a2)| a1 == a2)
                    }
                    (None, None) => true,
                    _ => false,
                }
            }

            // No other subtyping relationships
            _ => false,
        }
    }

    fn is_builtin_class_bridge(&self, lhs: &Type, rhs: &Type) -> bool {
        match (lhs, rhs) {
            (Type::Array(_), Type::Class(c)) => c.name == "Array",
            (Type::Task(_), Type::Class(c)) => c.name == "Task",
            (Type::Channel(_), Type::Class(c)) => c.name == "Channel",
            (Type::Map(_), Type::Class(c)) => c.name == "Map",
            (Type::Set(_), Type::Class(c)) => c.name == "Set",
            (Type::RegExp, Type::Class(c)) => c.name == "RegExp",
            (Type::Date, Type::Class(c)) => c.name == "Date",
            (Type::Buffer, Type::Class(c)) => c.name == "Buffer",
            (Type::Mutex, Type::Class(c)) => c.name == "Mutex",
            _ => false,
        }
    }

    /// Add a type variable substitution
    pub fn add_substitution(&mut self, name: String, ty: TypeId) {
        self.type_vars.insert(name, ty);
    }

    /// Clear all type variable substitutions
    pub fn clear_substitutions(&mut self) {
        self.type_vars.clear();
    }

    fn is_function_return_subtype(
        &mut self,
        sub_return: TypeId,
        sub_async: bool,
        sup_return: TypeId,
        sup_async: bool,
    ) -> bool {
        match (sub_async, sup_async) {
            (false, false) | (true, true) => self.is_subtype(sub_return, sup_return),
            // async (...) => T  <:  (... ) => Task<U>  iff  T <: U
            (true, false) => match self.type_ctx.get(sup_return) {
                Some(Type::Task(task_sup)) => self.is_subtype(sub_return, task_sup.result),
                _ => false,
            },
            // (... ) => Task<T>  <:  async (...) => U  iff  T <: U
            (false, true) => match self.type_ctx.get(sub_return) {
                Some(Type::Task(task_sub)) => self.is_subtype(task_sub.result, sup_return),
                _ => false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::types::context::TypeContext;

    #[test]
    fn test_reflexivity() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();
        let mut sub_ctx = SubtypingContext::new(&ctx);

        assert!(sub_ctx.is_subtype(num, num));
    }

    #[test]
    fn test_never_is_bottom() {
        let mut ctx = TypeContext::new();
        let never = ctx.never_type();
        let num = ctx.number_type();
        let str = ctx.string_type();
        let mut sub_ctx = SubtypingContext::new(&ctx);

        assert!(sub_ctx.is_subtype(never, num));
        assert!(sub_ctx.is_subtype(never, str));
        assert!(!sub_ctx.is_subtype(num, never));
    }

    #[test]
    fn test_unknown_is_top() {
        let mut ctx = TypeContext::new();
        let unknown = ctx.unknown_type();
        let num = ctx.number_type();
        let str = ctx.string_type();
        let mut sub_ctx = SubtypingContext::new(&ctx);

        assert!(sub_ctx.is_subtype(num, unknown));
        assert!(sub_ctx.is_subtype(str, unknown));
        assert!(!sub_ctx.is_subtype(unknown, num));
    }

    #[test]
    fn test_primitive_subtyping() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();
        let str = ctx.string_type();
        let mut sub_ctx = SubtypingContext::new(&ctx);

        assert!(sub_ctx.is_subtype(num, num));
        assert!(!sub_ctx.is_subtype(num, str));
    }

    #[test]
    fn test_union_subtyping() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();
        let str = ctx.string_type();
        let union = ctx.union_type(vec![num, str]);
        let mut sub_ctx = SubtypingContext::new(&ctx);

        // number <: number | string
        assert!(sub_ctx.is_subtype(num, union));
        // string <: number | string
        assert!(sub_ctx.is_subtype(str, union));
        // number | string <: number | string
        assert!(sub_ctx.is_subtype(union, union));
        // !(number | string <: number)
        assert!(!sub_ctx.is_subtype(union, num));
    }

    #[test]
    fn test_array_subtyping() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();
        let str = ctx.string_type();
        let num_arr = ctx.array_type(num);
        let str_arr = ctx.array_type(str);
        let mut sub_ctx = SubtypingContext::new(&ctx);

        assert!(sub_ctx.is_subtype(num_arr, num_arr));
        assert!(!sub_ctx.is_subtype(num_arr, str_arr));
    }

    #[test]
    fn test_tuple_subtyping() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();
        let str = ctx.string_type();
        let tuple1 = ctx.tuple_type(vec![num, str]);
        let tuple2 = ctx.tuple_type(vec![num, str]);
        let tuple3 = ctx.tuple_type(vec![str, num]);
        let mut sub_ctx = SubtypingContext::new(&ctx);

        assert!(sub_ctx.is_subtype(tuple1, tuple2));
        assert!(!sub_ctx.is_subtype(tuple1, tuple3));
    }

    #[test]
    fn test_function_subtyping_contravariance() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();
        let str = ctx.string_type();
        let unknown = ctx.unknown_type();

        // (unknown) => number
        let f1 = ctx.function_type(vec![unknown], num, false);
        // (string) => number
        let f2 = ctx.function_type(vec![str], num, false);

        let mut sub_ctx = SubtypingContext::new(&ctx);

        // (unknown) => number <: (string) => number
        // because string <: unknown (contravariant in parameters)
        assert!(sub_ctx.is_subtype(f1, f2));
        assert!(!sub_ctx.is_subtype(f2, f1));
    }

    #[test]
    fn test_function_subtyping_covariance() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();
        let never = ctx.never_type();
        let unknown = ctx.unknown_type();

        // (number) => never
        let f1 = ctx.function_type(vec![num], never, false);
        // (number) => unknown
        let f2 = ctx.function_type(vec![num], unknown, false);

        let mut sub_ctx = SubtypingContext::new(&ctx);

        // (number) => never <: (number) => unknown
        // because never <: unknown (covariant in return type)
        assert!(sub_ctx.is_subtype(f1, f2));
        assert!(!sub_ctx.is_subtype(f2, f1));
    }

    #[test]
    fn test_function_subtyping_async_matches_task_return() {
        let mut ctx = TypeContext::new();
        let void = ctx.void_type();
        let task_void = ctx.task_type(void);
        let num = ctx.number_type();

        // async (number) => void  (effective return: Task<void>)
        let async_fn = ctx.function_type(vec![num], void, true);
        // (number) => Task<void>
        let task_callback = ctx.function_type(vec![num], task_void, false);

        let mut sub_ctx = SubtypingContext::new(&ctx);
        assert!(sub_ctx.is_subtype(async_fn, task_callback));
        assert!(sub_ctx.is_subtype(task_callback, async_fn));
    }

    #[test]
    fn test_function_subtyping_async_not_assignable_to_plain_void_callback() {
        let mut ctx = TypeContext::new();
        let void = ctx.void_type();
        let num = ctx.number_type();

        // async (number) => void  (effective return: Task<void>)
        let async_fn = ctx.function_type(vec![num], void, true);
        // (number) => void
        let plain_callback = ctx.function_type(vec![num], void, false);

        let mut sub_ctx = SubtypingContext::new(&ctx);
        assert!(!sub_ctx.is_subtype(async_fn, plain_callback));
    }
}
