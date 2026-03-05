//! Subtyping rules for the Raya type system
//!
//! Implements the subtyping relation T <: U (T is a subtype of U).

use super::context::TypeContext;
use super::ty::{FunctionType, GenericType, PrimitiveType, Type, TypeId, TypeReference};
use crate::parser::ast::Visibility;
use rustc_hash::{FxHashMap, FxHashSet};

fn jsobject_generic_inner(type_ctx: &TypeContext, generic: &GenericType) -> Option<TypeId> {
    if generic.type_args.len() != 1 {
        return None;
    }
    match type_ctx.get(generic.base) {
        Some(Type::JSObject) => generic.type_args.first().copied(),
        _ => None,
    }
}

/// Context for checking subtyping relationships
///
/// Maintains a substitution map for type variables during checking.
#[derive(Debug, Clone)]
pub struct SubtypingContext<'a> {
    /// Type context for resolving types
    type_ctx: &'a TypeContext,

    /// Current type variable substitutions
    type_vars: FxHashMap<String, TypeId>,
    /// In-progress subtype relation checks used for coinductive recursion
    /// handling on recursive structural types.
    active_pairs: FxHashSet<(u32, u32)>,
    /// Memoized subtype results to avoid re-walking the same structural graph.
    pair_cache: FxHashMap<(u32, u32), bool>,
    /// Relax function arity compatibility to allow extra call arguments
    /// being ignored by the callee (used for runtime/link-time call-shape checks).
    relaxed_function_call_arity: bool,
}

impl<'a> SubtypingContext<'a> {
    /// Create a new subtyping context
    pub fn new(type_ctx: &'a TypeContext) -> Self {
        SubtypingContext {
            type_ctx,
            type_vars: FxHashMap::default(),
            active_pairs: FxHashSet::default(),
            pair_cache: FxHashMap::default(),
            relaxed_function_call_arity: false,
        }
    }

    /// Allow compatibility checks where callees may ignore additional call arguments.
    ///
    /// This is intentionally disabled by default to keep checker-level subtyping strict.
    pub fn with_relaxed_function_call_arity(mut self, enabled: bool) -> Self {
        self.relaxed_function_call_arity = enabled;
        self
    }

    /// Check if `sub` is a subtype of `sup` (sub <: sup)
    ///
    /// Returns true if a value of type `sub` can be used where `sup` is expected.
    pub fn is_subtype(&mut self, sub: TypeId, sup: TypeId) -> bool {
        let pair = (sub.0, sup.0);
        if let Some(&cached) = self.pair_cache.get(&pair) {
            return cached;
        }
        // Coinductive guard: if we're already proving this pair, treat it as
        // provisionally true to prevent infinite descent on recursive types.
        if !self.active_pairs.insert(pair) {
            return true;
        }

        // Reflexivity: T <: T
        if sub == sup {
            self.pair_cache.insert(pair, true);
            self.active_pairs.remove(&pair);
            return true;
        }

        let sub_ty = match self.type_ctx.get(sub) {
            Some(ty) => ty,
            None => {
                self.pair_cache.insert(pair, false);
                self.active_pairs.remove(&pair);
                return false;
            }
        };

        let sup_ty = match self.type_ctx.get(sup) {
            Some(ty) => ty,
            None => {
                self.pair_cache.insert(pair, false);
                self.active_pairs.remove(&pair);
                return false;
            }
        };

        // Bridge specialized builtin container/runtime types with their class-type names.
        // In checker/runtime-prelude flows we can see either side represented as:
        // - Type::Map/Set/Channel/Promise/Array (specialized builtin types), or
        // - Type::Class { name: "Map" | "Set" | ... } (from class declarations).
        // They denote the same runtime entities and should be mutually compatible.
        if self.is_builtin_class_bridge(sub_ty, sup_ty)
            || self.is_builtin_class_bridge(sup_ty, sub_ty)
        {
            self.pair_cache.insert(pair, true);
            self.active_pairs.remove(&pair);
            return true;
        }

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

        let result = match (sub_ty, sup_ty) {
            // Never is subtype of everything
            (Type::Never, _) => true,

            // Any is both top and bottom in node-compat style typing.
            (Type::Any, _) | (_, Type::Any) => true,

            // Everything is subtype of Unknown (Unknown is top type)
            (_, Type::Unknown) => true,

            // JSObject is a structural object-ish top/bottom for dynamic object values.
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

            // Union-to-union subtyping:
            // S1 | ... | Sn <: T1 | ... | Tm iff every Si is a subtype of some Tj.
            (Type::Union(sub_union), Type::Union(sup_union)) => {
                sub_union.members.iter().all(|&sub_member| {
                    sup_union
                        .members
                        .iter()
                        .any(|&sup_member| self.is_subtype(sub_member, sup_member))
                })
            }

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
                #[derive(Clone)]
                enum RestSpec {
                    None,
                    Tuple(Vec<TypeId>),
                    Array(TypeId),
                    // Variadic but unresolved to a concrete tuple/array element shape.
                    // This appears in generic signatures like (...args: E[K]).
                    Unresolved,
                }

                #[derive(Clone, Copy)]
                enum ParamSlot {
                    Absent,
                    Ty(TypeId),
                    Wildcard,
                }

                let rest_spec = |f: &FunctionType, this: &TypeContext| -> RestSpec {
                    let Some(rest_ty) = f.rest_param else {
                        return RestSpec::None;
                    };
                    match this.get(rest_ty) {
                        Some(Type::Tuple(t)) => RestSpec::Tuple(t.elements.clone()),
                        Some(Type::Array(arr)) => RestSpec::Array(arr.element),
                        Some(Type::IndexedAccess(_))
                        | Some(Type::Keyof(_))
                        | Some(Type::TypeVar(_))
                        | Some(Type::Union(_))
                        | Some(Type::Unknown) => RestSpec::Unresolved,
                        _ => RestSpec::Unresolved,
                    }
                };

                let arity_bounds = |f: &FunctionType, rest: &RestSpec| -> (usize, usize) {
                    match rest {
                        RestSpec::None => (f.min_params, f.params.len()),
                        RestSpec::Tuple(t) => (f.min_params + t.len(), f.params.len() + t.len()),
                        RestSpec::Array(_) | RestSpec::Unresolved => (f.min_params, usize::MAX),
                    }
                };

                let finite_prefix_len = |f: &FunctionType, rest: &RestSpec| -> usize {
                    match rest {
                        RestSpec::None => f.params.len(),
                        RestSpec::Tuple(t) => f.params.len() + t.len(),
                        RestSpec::Array(_) | RestSpec::Unresolved => f.params.len(),
                    }
                };

                let slot_at = |f: &FunctionType, rest: &RestSpec, idx: usize| -> ParamSlot {
                    if idx < f.params.len() {
                        return ParamSlot::Ty(f.params[idx]);
                    }
                    let rest_idx = idx - f.params.len();
                    match rest {
                        RestSpec::None => ParamSlot::Absent,
                        RestSpec::Tuple(t) => t
                            .get(rest_idx)
                            .copied()
                            .map(ParamSlot::Ty)
                            .unwrap_or(ParamSlot::Absent),
                        RestSpec::Array(elem) => ParamSlot::Ty(*elem),
                        RestSpec::Unresolved => ParamSlot::Wildcard,
                    }
                };

                let r1 = rest_spec(f1, self.type_ctx);
                let r2 = rest_spec(f2, self.type_ctx);
                let (min1, max1) = arity_bounds(f1, &r1);
                let (min2, max2) = arity_bounds(f2, &r2);

                // f1 <: f2 requires f1 to accept every call shape accepted by f2.
                let has_unresolved_arity =
                    matches!(r1, RestSpec::Unresolved) || matches!(r2, RestSpec::Unresolved);
                if !has_unresolved_arity {
                    if min1 > min2 {
                        return false;
                    }
                    if !self.relaxed_function_call_arity && max1 < max2 {
                        return false;
                    }
                }

                let p1_prefix = finite_prefix_len(f1, &r1);
                let p2_prefix = finite_prefix_len(f2, &r2);
                let compare_len = if max2 == usize::MAX {
                    // Compare finite prefixes and one extra slot for variadic element compatibility.
                    p1_prefix.max(p2_prefix).saturating_add(1)
                } else {
                    max2
                };

                let mut params_match = true;
                for i in 0..compare_len {
                    let sub_slot = slot_at(f1, &r1, i);
                    let sup_slot = slot_at(f2, &r2, i);
                    match (sub_slot, sup_slot) {
                        // Unresolved variadic generic slots are checked at call-site unification;
                        // treat as wildcard here to avoid rejecting valid generic callbacks.
                        (ParamSlot::Wildcard, _) | (_, ParamSlot::Wildcard) => {}
                        (ParamSlot::Absent, ParamSlot::Absent) => {}
                        (ParamSlot::Absent, _) => {
                            if !self.relaxed_function_call_arity {
                                params_match = false;
                                break;
                            }
                        }
                        (_, ParamSlot::Absent) => {}
                        (ParamSlot::Ty(p1), ParamSlot::Ty(p2)) => {
                            if !self.is_subtype(p2, p1) {
                                params_match = false;
                                break;
                            }
                        }
                    }
                }

                // Return type is covariant, comparing effective returns:
                // - async fn (...): T is treated as (... ) => Promise<T>
                // - sync fn (...): R is treated as (... ) => R
                let return_match = self.is_function_return_subtype(
                    f1.return_type,
                    f1.is_async,
                    f2.return_type,
                    f2.is_async,
                );

                params_match && return_match
            }

            // Function <: Object: in JavaScript, all functions are objects.
            // This allows storing function references in Object-typed containers.
            (Type::Function(_), Type::Class(class)) if class.name == "Object" => true,

            // Function <: Object with structural call signatures.
            (Type::Function(_), Type::Object(o)) => {
                let props_match = o.properties.iter().all(|p| p.optional);
                let methods_match = o.call_signatures.iter().all(|sup_sig| {
                    self.is_subtype(sub, *sup_sig)
                });
                let construct_match = o.construct_signatures.is_empty();
                props_match && methods_match && construct_match
            }

            // Function <: Interface with structural call signatures.
            (Type::Function(_), Type::Interface(i)) => {
                let props_match = i.properties.iter().all(|p| p.optional);
                let methods_empty = i.methods.is_empty();
                let call_match = i.call_signatures.iter().all(|sup_sig| {
                    self.is_subtype(sub, *sup_sig)
                });
                let construct_match = i.construct_signatures.is_empty();
                props_match && methods_empty && call_match && construct_match
            }

            // Array subtyping: T[] <: U[] if T <: U
            (Type::Array(a1), Type::Array(a2)) => self.is_subtype(a1.element, a2.element),

            // Promise subtyping: Promise<T> <: Promise<U> if T <: U (covariant)
            (Type::Task(t1), Type::Task(t2)) => self.is_subtype(t1.result, t2.result),

            // Array-to-tuple subtyping: T[] <: [U1, U2, ..., Un] if T <: Ui for all i.
            // This allows array literals (typed as T[]) to satisfy tuple expectations.
            (Type::Array(arr), Type::Tuple(tup)) => tup
                .elements
                .iter()
                .all(|&elem| self.is_subtype(arr.element, elem)),

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
                // All required properties in o2 must be in o1 with compatible variance.
                // Optional properties in o2 may be absent in o1.
                let props_match = o2.properties.iter().all(|p2| {
                    match o1.properties.iter().find(|p1| p1.name == p2.name) {
                        Some(p1) => {
                            (p2.optional || !p1.optional)
                                && (!p2.readonly || p1.readonly) // readonly in sup => readonly in sub
                                && self.is_subtype(p1.ty, p2.ty)
                        }
                        None => p2.optional,
                    }
                });
                let call_match = o2.call_signatures.iter().all(|sup_sig| {
                    o1.call_signatures
                        .iter()
                        .any(|sub_sig| self.is_subtype(*sub_sig, *sup_sig))
                });
                let construct_match = o2.construct_signatures.iter().all(|sup_sig| {
                    o1.construct_signatures
                        .iter()
                        .any(|sub_sig| self.is_subtype(*sub_sig, *sup_sig))
                });
                props_match && call_match && construct_match
            }

            // Class <: Object (structural): class instance <: object type
            // Optional properties in the target object type don't need to exist in the class
            (Type::Class(c), Type::Object(o)) => {
                let construct_match = o.construct_signatures.iter().all(|sig| {
                    match self.type_ctx.get(*sig) {
                        Some(Type::Function(func)) => self.is_subtype(sub, func.return_type),
                        _ => false,
                    }
                });
                o.call_signatures.is_empty()
                    && construct_match
                    && o.properties.iter().all(|op| {
                    // If the target property is optional, the class doesn't need to have it
                    if op.optional {
                        return true;
                    }
                    // For required properties, check class properties
                    c.properties
                        .iter()
                        .filter(|cp| cp.visibility == Visibility::Public)
                        .any(|cp| {
                            cp.name == op.name
                                && !cp.optional // Class property must not be optional for required target
                                && (!op.readonly || cp.readonly)
                                && self.is_subtype(cp.ty, op.ty)
                        })
                    // Also check class methods (methods are stored separately from properties)
                    || c.methods
                        .iter()
                        .filter(|cm| cm.visibility == Visibility::Public)
                        .any(|cm| cm.name == op.name && self.is_subtype(cm.ty, op.ty))
                    })
            }

            // Object <: Object-like Class (structural)
            // Optional properties in the class don't need to exist in the object
            (Type::Object(o), Type::Class(c)) => c
                .properties
                .iter()
                .filter(|cp| cp.visibility == Visibility::Public)
                .all(|cp| {
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

            // Class subtyping (structural public surface):
            // width/depth compatibility on instance + static members.
            // Fallback to extends/implements chain for explicit nominal ancestry.
            (Type::Class(c1), Type::Class(c2)) => {
                if c1.name == c2.name {
                    return true;
                }

                let props_match = c2
                    .properties
                    .iter()
                    .filter(|p2| p2.visibility == Visibility::Public)
                    .all(|p2| {
                        match c1
                            .properties
                            .iter()
                            .filter(|p1| p1.visibility == Visibility::Public)
                            .find(|p1| p1.name == p2.name)
                        {
                            Some(p1) => {
                                (p2.optional || !p1.optional)
                                    && (!p2.readonly || p1.readonly)
                                    && self.is_subtype(p1.ty, p2.ty)
                            }
                            None => p2.optional,
                        }
                    });

                let methods_match = c2
                    .methods
                    .iter()
                    .filter(|m2| m2.visibility == Visibility::Public)
                    .all(|m2| {
                        c1.methods
                            .iter()
                            .filter(|m1| m1.visibility == Visibility::Public)
                            .any(|m1| m1.name == m2.name && self.is_subtype(m1.ty, m2.ty))
                    });

                let static_props_match = c2
                    .static_properties
                    .iter()
                    .filter(|p2| p2.visibility == Visibility::Public)
                    .all(|p2| {
                        match c1
                            .static_properties
                            .iter()
                            .filter(|p1| p1.visibility == Visibility::Public)
                            .find(|p1| p1.name == p2.name)
                        {
                            Some(p1) => {
                                (p2.optional || !p1.optional)
                                    && (!p2.readonly || p1.readonly)
                                    && self.is_subtype(p1.ty, p2.ty)
                            }
                            None => p2.optional,
                        }
                    });

                let static_methods_match = c2
                    .static_methods
                    .iter()
                    .filter(|m2| m2.visibility == Visibility::Public)
                    .all(|m2| {
                        c1.static_methods
                            .iter()
                            .filter(|m1| m1.visibility == Visibility::Public)
                            .any(|m1| m1.name == m2.name && self.is_subtype(m1.ty, m2.ty))
                    });

                if props_match && methods_match && static_props_match && static_methods_match {
                    return true;
                }

                // Check explicit ancestry
                if let Some(parent) = c1.extends {
                    if self.is_subtype(parent, sup) {
                        return true;
                    }
                }

                c1.implements
                    .iter()
                    .any(|&impl_id| self.is_subtype(impl_id, sup))
            }

            // Class <: Interface (structural subtyping for interfaces)
            (Type::Class(c), Type::Interface(i)) => {
                // Check if class implements all interface members
                let construct_match = i.construct_signatures.iter().all(|sig| {
                    match self.type_ctx.get(*sig) {
                        Some(Type::Function(func)) => self.is_subtype(sub, func.return_type),
                        _ => false,
                    }
                });
                i.call_signatures.is_empty()
                    && construct_match
                    && i.properties.iter().all(|ip| {
                    // If the interface property is optional, the class doesn't need to have it
                    if ip.optional {
                        return true;
                    }
                    // For required properties, check class properties
                    c.properties
                        .iter()
                        .filter(|cp| cp.visibility == Visibility::Public)
                        .any(|cp| {
                            cp.name == ip.name
                                && !cp.optional // Class property must not be optional for required interface
                                && (!ip.readonly || cp.readonly)
                                && self.is_subtype(cp.ty, ip.ty)
                        })
                }) && i.methods.iter().all(|im| {
                    c.methods
                        .iter()
                        .filter(|cm| cm.visibility == Visibility::Public)
                        .any(|cm| cm.name == im.name && self.is_subtype(cm.ty, im.ty))
                    })
            }

            // Interface subtyping (structural)
            (Type::Interface(i1), Type::Interface(i2)) => {
                // Check properties
                let props_match = i2.properties.iter().all(|p2| {
                    match i1.properties.iter().find(|p1| p1.name == p2.name) {
                        Some(p1) => {
                            (p2.optional || !p1.optional)
                                && (!p2.readonly || p1.readonly)
                                && self.is_subtype(p1.ty, p2.ty)
                        }
                        None => p2.optional,
                    }
                });

                // Check methods
                let methods_match = i2.methods.iter().all(|m2| {
                    i1.methods
                        .iter()
                        .any(|m1| m1.name == m2.name && self.is_subtype(m1.ty, m2.ty))
                });

                let call_match = i2.call_signatures.iter().all(|sup_sig| {
                    i1.call_signatures
                        .iter()
                        .any(|sub_sig| self.is_subtype(*sub_sig, *sup_sig))
                });
                let construct_match = i2.construct_signatures.iter().all(|sup_sig| {
                    i1.construct_signatures
                        .iter()
                        .any(|sub_sig| self.is_subtype(*sub_sig, *sup_sig))
                });

                props_match && methods_match && call_match && construct_match
            }

            // Object <: Interface (structural)
            (Type::Object(o), Type::Interface(i)) => {
                let props_match = i.properties.iter().all(|ip| {
                    match o.properties.iter().find(|op| op.name == ip.name) {
                        Some(op) => {
                            (ip.optional || !op.optional)
                                && (!ip.readonly || op.readonly)
                                && self.is_subtype(op.ty, ip.ty)
                        }
                        None => ip.optional,
                    }
                });
                let methods_match = i.methods.iter().all(|im| {
                    o.properties
                        .iter()
                        .find(|op| op.name == im.name)
                        .is_some_and(|op| self.is_subtype(op.ty, im.ty))
                });
                let call_match = i.call_signatures.iter().all(|sup_sig| {
                    o.call_signatures
                        .iter()
                        .any(|sub_sig| self.is_subtype(*sub_sig, *sup_sig))
                });
                let construct_match = i.construct_signatures.iter().all(|sup_sig| {
                    o.construct_signatures
                        .iter()
                        .any(|sub_sig| self.is_subtype(*sub_sig, *sup_sig))
                });
                props_match && methods_match && call_match && construct_match
            }

            // Interface <: Object (structural)
            (Type::Interface(i), Type::Object(o)) => {
                let props_match = o.properties.iter().all(|op| {
                    match i.properties.iter().find(|ip| ip.name == op.name) {
                        Some(ip) => {
                            (op.optional || !ip.optional)
                                && (!op.readonly || ip.readonly)
                                && self.is_subtype(ip.ty, op.ty)
                        }
                        None => op.optional,
                    }
                });
                let call_match = o.call_signatures.iter().all(|sup_sig| {
                    i.call_signatures
                        .iter()
                        .any(|sub_sig| self.is_subtype(*sub_sig, *sup_sig))
                });
                let construct_match = o.construct_signatures.iter().all(|sup_sig| {
                    i.construct_signatures
                        .iter()
                        .any(|sub_sig| self.is_subtype(*sub_sig, *sup_sig))
                });
                props_match && call_match && construct_match
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

            // Reference types are symbolic type names. Resolve them to named types when possible.
            (Type::Reference(r1), Type::Reference(r2)) => {
                if r1.name == r2.name {
                    match (&r1.type_args, &r2.type_args) {
                        (Some(args1), Some(args2)) => {
                            if args1.len() != args2.len() {
                                false
                            } else {
                                args1.iter().zip(args2.iter()).all(|(&a1, &a2)| {
                                    // Structural equality on type args.
                                    self.is_subtype(a1, a2) && self.is_subtype(a2, a1)
                                })
                            }
                        }
                        // Unspecified args are treated as compatible with specialized references.
                        (None, None) | (Some(_), None) | (None, Some(_)) => true,
                    }
                } else if let (Some(left), Some(right)) = (
                    self.resolve_named_type_ref(r1),
                    self.resolve_named_type_ref(r2),
                ) {
                    self.is_subtype(left, right)
                } else {
                    false
                }
            }
            (Type::Reference(reference), _) => self.reference_subtype_of(reference, sup),
            (_, Type::Reference(reference)) => self.subtype_of_reference(sub, reference),

            // No other subtyping relationships
            _ => false,
        };

        self.pair_cache.insert(pair, result);
        self.active_pairs.remove(&pair);
        result
    }

    fn is_builtin_class_bridge(&self, lhs: &Type, rhs: &Type) -> bool {
        match (lhs, rhs) {
            (Type::Array(_), Type::Class(c)) => c.name == "Array",
            (Type::Task(_), Type::Class(c)) => c.name == "Promise",
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

    fn resolve_named_type_ref(&self, reference: &TypeReference) -> Option<TypeId> {
        self.type_ctx.lookup_named_type(&reference.name)
    }

    fn reference_subtype_of(&mut self, reference: &TypeReference, sup: TypeId) -> bool {
        if let Some(named_ty) = self.resolve_named_type_ref(reference) {
            return self.is_subtype(named_ty, sup);
        }
        false
    }

    fn subtype_of_reference(&mut self, sub: TypeId, reference: &TypeReference) -> bool {
        if let Some(Type::Class(sub_class)) = self.type_ctx.get(sub) {
            if sub_class.name == reference.name {
                return true;
            }
        }

        if let Some(named_ty) = self.resolve_named_type_ref(reference) {
            return self.is_subtype(sub, named_ty);
        }
        false
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
            // async (...) => T  <:  (... ) => Promise<U>  iff  T <: U
            (true, false) => match self.type_ctx.get(sup_return) {
                Some(Type::Task(task_sup)) => self.is_subtype(sub_return, task_sup.result),
                _ => false,
            },
            // (... ) => Promise<T>  <:  async (...) => U  iff  T <: U
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

        // async (number) => void  (effective return: Promise<void>)
        let async_fn = ctx.function_type(vec![num], void, true);
        // (number) => Promise<void>
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

        // async (number) => void  (effective return: Promise<void>)
        let async_fn = ctx.function_type(vec![num], void, true);
        // (number) => void
        let plain_callback = ctx.function_type(vec![num], void, false);

        let mut sub_ctx = SubtypingContext::new(&ctx);
        assert!(!sub_ctx.is_subtype(async_fn, plain_callback));
    }

    #[test]
    fn test_relaxed_function_subtyping_allows_ignored_extra_call_args() {
        let mut ctx = TypeContext::new();
        let num = ctx.number_type();

        // actual export: (number) => number
        let actual = ctx.function_type(vec![num], num, false);
        // expected import contract: (number, number) => number
        let expected = ctx.function_type(vec![num, num], num, false);

        let mut strict = SubtypingContext::new(&ctx);
        assert!(!strict.is_subtype(actual, expected));

        let mut relaxed = SubtypingContext::new(&ctx).with_relaxed_function_call_arity(true);
        assert!(relaxed.is_subtype(actual, expected));
    }
}
