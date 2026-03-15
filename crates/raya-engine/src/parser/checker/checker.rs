//! Type checker - validates types for expressions and statements
//!
//! The type checker walks the AST and verifies that all operations are
//! type-safe. It uses the symbol table for name resolution and the type
//! context for type operations.

use super::captures::{
    CaptureInfo, ClosureCaptures, ClosureId, FreeVariableCollector, ModuleCaptureInfo,
};
use super::error::{CheckError, CheckWarning};
use super::exhaustiveness::{check_switch_exhaustiveness, ExhaustivenessResult};
use super::narrowing::{apply_type_guard, TypeEnv};
use super::symbols::{SymbolKind, SymbolTable};
use super::type_guards::{extract_all_type_guards, extract_type_guard, TypeGuard};
use super::{CheckerPolicy, TypeSystemMode};
use crate::parser::ast::*;
use crate::parser::token::Span;
use crate::parser::types::normalize::contains_type_variables;
use crate::parser::types::{AssignabilityContext, GenericContext, TypeContext, TypeId};
use crate::{Interner, Symbol as ParserSymbol};
use rustc_hash::{FxHashMap, FxHashSet};

/// Get the variable name from a type guard
fn get_guard_var(guard: &TypeGuard) -> &String {
    match guard {
        TypeGuard::TypeOf { var, .. } => var,
        TypeGuard::Discriminant { var, .. } => var,
        TypeGuard::Nullish { var, .. } => var,
        TypeGuard::IsArray { var, .. } => var,
        TypeGuard::IsInteger { var, .. } => var,
        TypeGuard::IsNaN { var, .. } => var,
        TypeGuard::IsFinite { var, .. } => var,
        TypeGuard::TypePredicate { var, .. } => var,
        TypeGuard::Truthiness { var, .. } => var,
    }
}

/// Inferred types for variables without type annotations
///
/// Maps (scope_id, variable_name) to the inferred TypeId.
/// These should be applied to the symbol table using `update_type`.
pub type InferredTypes = FxHashMap<(u32, String), TypeId>;

/// Result of type checking a module
#[derive(Debug)]
pub struct CheckResult {
    /// Inferred types for variables without type annotations
    pub inferred_types: InferredTypes,
    /// Capture information for all closures in the module
    pub captures: ModuleCaptureInfo,
    /// Expression types: maps expression ID (ptr as usize) to TypeId
    pub expr_types: FxHashMap<usize, TypeId>,
    /// Resolved type annotation types: maps TypeAnnotation node ID (ptr as usize) to TypeId
    pub type_annotation_types: FxHashMap<usize, TypeId>,
    /// Warnings collected during type checking
    pub warnings: Vec<CheckWarning>,
}

#[derive(Debug, Clone, Default)]
struct ClassAstSummary {
    is_abstract: bool,
    extends: Option<String>,
    abstract_methods: FxHashSet<String>,
    concrete_methods: FxHashSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HelperReceiverExpectation {
    Instance(TypeId),
    StaticClass(TypeId),
}

impl HelperReceiverExpectation {
    fn type_id(self) -> TypeId {
        match self {
            Self::Instance(ty) | Self::StaticClass(ty) => ty,
        }
    }
}

/// Negate a type guard
fn negate_guard(guard: &TypeGuard) -> TypeGuard {
    match guard {
        TypeGuard::TypeOf {
            var,
            type_name,
            negated,
        } => TypeGuard::TypeOf {
            var: var.clone(),
            type_name: type_name.clone(),
            negated: !negated,
        },
        TypeGuard::Discriminant {
            var,
            field,
            variant,
            negated,
        } => TypeGuard::Discriminant {
            var: var.clone(),
            field: field.clone(),
            variant: variant.clone(),
            negated: !negated,
        },
        TypeGuard::Nullish {
            var,
            field,
            negated,
        } => TypeGuard::Nullish {
            var: var.clone(),
            field: field.clone(),
            negated: !negated,
        },
        TypeGuard::IsArray { var, negated } => TypeGuard::IsArray {
            var: var.clone(),
            negated: !negated,
        },
        TypeGuard::IsInteger { var, negated } => TypeGuard::IsInteger {
            var: var.clone(),
            negated: !negated,
        },
        TypeGuard::IsNaN { var, negated } => TypeGuard::IsNaN {
            var: var.clone(),
            negated: !negated,
        },
        TypeGuard::IsFinite { var, negated } => TypeGuard::IsFinite {
            var: var.clone(),
            negated: !negated,
        },
        TypeGuard::TypePredicate {
            var,
            predicate,
            negated,
        } => TypeGuard::TypePredicate {
            var: var.clone(),
            predicate: predicate.clone(),
            negated: !negated,
        },
        TypeGuard::Truthiness { var, negated } => TypeGuard::Truthiness {
            var: var.clone(),
            negated: !negated,
        },
    }
}

/// Type checker
///
/// Performs type checking on the AST using the symbol table and type context.
pub struct TypeChecker<'a> {
    type_ctx: &'a mut TypeContext,
    symbols: &'a SymbolTable,
    interner: &'a Interner,

    /// Map from expression to its inferred type
    expr_types: FxHashMap<usize, TypeId>,
    /// Resolved types for type-annotation AST nodes (keyed by node pointer).
    type_annotation_types: FxHashMap<usize, TypeId>,

    /// Type checking errors
    errors: Vec<CheckError>,

    /// Current function return type (for checking return statements)
    current_function_return_type: Option<TypeId>,

    /// Type environment tracking narrowed types in current scope
    type_env: TypeEnv,

    /// Current scope ID for variable resolution
    /// The checker tracks its own scope position as it walks the AST
    current_scope: super::symbols::ScopeId,

    /// Next scope ID to be entered
    /// This mirrors the scope creation in the binder
    next_scope_id: u32,

    /// Stack of scope IDs for tracking parent scopes
    /// Used when inside expressions (arrow functions) where binder didn't create scopes
    scope_stack: Vec<super::symbols::ScopeId>,

    /// Inferred types for variables without type annotations
    /// Maps (scope_id, variable_name) -> inferred_type
    inferred_var_types: FxHashMap<(u32, String), TypeId>,
    /// Variables currently known to store extracted (unbound) method references.
    /// Keyed by (declaration_scope_id, variable_name).
    unbound_method_vars: FxHashSet<(u32, String)>,
    /// Expected receiver (`this`) shape for unbound extracted method variables.
    /// Keyed by (declaration_scope_id, variable_name).
    unbound_method_receiver_types: FxHashMap<(u32, String), HelperReceiverExpectation>,
    /// Variables currently known to alias constructible class values.
    /// Keyed by (declaration_scope_id, variable_name).
    constructible_vars: FxHashSet<(u32, String)>,

    /// Capture information for all closures
    capture_info: ModuleCaptureInfo,

    /// Method-level type parameters for the currently checked method body.
    /// Maps type parameter names (e.g. "K", "U") to their TypeVar TypeIds.
    method_type_params: FxHashMap<String, TypeId>,

    /// Current class type for checking `this` expressions
    current_class_type: Option<TypeId>,

    /// Whether we are inside a constructor body (readonly fields can be assigned)
    in_constructor: bool,

    /// Depth counter for arrow function bodies
    /// When > 0, scope enter/exit are no-ops because the binder never visits
    /// inside expressions, so arrow body scopes don't exist in the symbol table.
    arrow_depth: u32,

    /// Warnings collected during type checking
    warnings: Vec<CheckWarning>,

    /// Stack used to collect return expression types when inferring block return types
    /// (e.g. arrow functions without explicit return annotations).
    return_type_collector: Vec<Vec<TypeId>>,

    /// Type-system behavior mode.
    mode: TypeSystemMode,
    /// Effective checker policy.
    policy: CheckerPolicy,
    /// True while checking the left-hand side of an assignment expression.
    in_assignment_lhs: bool,
    /// AST-derived class summaries used for abstract-contract checks.
    class_ast_summaries: FxHashMap<String, ClassAstSummary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FallbackReason {
    RecoverableUnsupportedExpr,
    RecoverableInvalidIntrinsicContext,
    Unavoidable,
}

impl<'a> TypeChecker<'a> {
    fn js_builtin_call_uses_construct(name: &str) -> bool {
        matches!(
            name,
            "Array"
                | "Symbol"
                | "Function"
                | "Error"
                | "AggregateError"
                | "EvalError"
                | "RangeError"
                | "ReferenceError"
                | "SyntaxError"
                | "TypeError"
                | "URIError"
                | "InternalError"
                | "SuppressedError"
        )
    }

    /// Create a new type checker
    pub fn new(
        type_ctx: &'a mut TypeContext,
        symbols: &'a SymbolTable,
        interner: &'a Interner,
    ) -> Self {
        TypeChecker {
            type_ctx,
            symbols,
            interner,
            expr_types: FxHashMap::default(),
            type_annotation_types: FxHashMap::default(),
            errors: Vec::new(),
            current_function_return_type: None,
            type_env: TypeEnv::new(),
            current_scope: super::symbols::ScopeId(0), // Start at global scope
            next_scope_id: 1,                          // Global is 0, next scope will be 1
            scope_stack: vec![super::symbols::ScopeId(0)], // Start with global on stack
            inferred_var_types: FxHashMap::default(),
            unbound_method_vars: FxHashSet::default(),
            unbound_method_receiver_types: FxHashMap::default(),
            constructible_vars: FxHashSet::default(),
            capture_info: ModuleCaptureInfo::new(),
            method_type_params: FxHashMap::default(),
            current_class_type: None,
            in_constructor: false,
            arrow_depth: 0,
            warnings: Vec::new(),
            return_type_collector: Vec::new(),
            mode: TypeSystemMode::Raya,
            policy: CheckerPolicy::for_mode(TypeSystemMode::Raya),
            in_assignment_lhs: false,
            class_ast_summaries: FxHashMap::default(),
        }
    }

    /// Set checker behavior mode.
    pub fn with_mode(mut self, mode: TypeSystemMode) -> Self {
        self.mode = mode;
        self.policy = CheckerPolicy::for_mode(mode);
        self
    }

    /// Set explicit checker policy.
    pub fn with_policy(mut self, policy: CheckerPolicy) -> Self {
        self.policy = policy;
        self
    }

    #[inline]
    fn is_strict_mode(&self) -> bool {
        self.policy.strict_assignability
    }

    #[inline]
    fn allows_explicit_any(&self) -> bool {
        self.policy.allow_explicit_any
    }

    #[inline]
    fn allows_implicit_any(&self) -> bool {
        self.policy.allow_implicit_any
    }

    #[inline]
    fn allows_dynamic_any(&self) -> bool {
        self.policy.allow_explicit_any || self.policy.allow_js_dynamic_fallback
    }

    #[inline]
    fn is_js_mode(&self) -> bool {
        matches!(self.mode, TypeSystemMode::Js)
    }

    #[inline]
    fn enforce_call_arity(&self) -> bool {
        !self.is_js_mode()
    }

    #[inline]
    fn type_is_unknown(&mut self, ty: TypeId) -> bool {
        matches!(
            self.type_ctx.get(ty),
            Some(crate::parser::types::Type::Unknown)
        )
    }

    #[inline]
    fn type_is_dynamic_anyish(&self, ty: TypeId) -> bool {
        self.type_ctx.jsobject_inner(ty).is_some()
            || matches!(
                self.type_ctx.get(ty),
                Some(crate::parser::types::Type::Any)
                    | Some(crate::parser::types::Type::Unknown)
                    | Some(crate::parser::types::Type::JSObject)
            )
    }

    #[inline]
    fn is_string_concat_operand_type(&self, ty: TypeId) -> bool {
        use crate::parser::types::{PrimitiveType, Type};
        match self.type_ctx.get(ty) {
            Some(Type::Primitive(
                PrimitiveType::String
                | PrimitiveType::Number
                | PrimitiveType::Int
                | PrimitiveType::Boolean
                | PrimitiveType::Null,
            ))
            | Some(Type::StringLiteral(_))
            | Some(Type::NumberLiteral(_))
            | Some(Type::BooleanLiteral(_))
            | Some(Type::Any)
            | Some(Type::Json) => true,
            Some(Type::Union(u)) => u
                .members
                .iter()
                .all(|&member| self.is_string_concat_operand_type(member)),
            _ => false,
        }
    }

    fn check_unknown_actionable(&mut self, ty: TypeId, operation: &str, span: Span) {
        if self.policy.enforce_unknown_not_actionable && self.type_is_unknown(ty) {
            self.errors.push(CheckError::UnknownNotActionable {
                operation: operation.to_string(),
                span,
            });
        }
    }

    fn make_assignability_ctx(&self) -> AssignabilityContext<'_> {
        AssignabilityContext::with_strict_mode(self.type_ctx, self.is_strict_mode())
    }

    #[inline]
    fn inference_fallback_type(&mut self) -> TypeId {
        if self.is_js_mode() {
            self.type_ctx.any_type()
        } else {
            self.type_ctx.unknown_type()
        }
    }

    fn fallback_type(&mut self, span: Span, reason: FallbackReason, detail: &str) -> TypeId {
        if self.is_strict_mode() {
            match reason {
                FallbackReason::RecoverableUnsupportedExpr => {
                    self.errors
                        .push(CheckError::UnsupportedExpressionTypingPath {
                            expression: detail.to_string(),
                            span,
                        });
                }
                FallbackReason::RecoverableInvalidIntrinsicContext => {
                    self.errors
                        .push(CheckError::InvalidIntrinsicInferenceContext {
                            intrinsic: detail.to_string(),
                            span,
                        });
                }
                FallbackReason::Unavoidable => {}
            }
        }
        self.inference_fallback_type()
    }

    fn is_dynamic_seed_type(&mut self, ty: TypeId) -> bool {
        self.type_ctx.jsobject_inner(ty).is_some()
            || matches!(
                self.type_ctx.get(ty),
                Some(crate::parser::types::Type::Unknown)
                    | Some(crate::parser::types::Type::Any)
                    | Some(crate::parser::types::Type::JSObject)
            )
    }

    fn join_inferred_types(&mut self, left: TypeId, right: TypeId) -> TypeId {
        if left == right {
            return left;
        }
        if let Some(base) = self.type_ctx.jsobject_inner(left) {
            let merged = self.type_ctx.union_type(vec![base, right]);
            return self.type_ctx.jsobject_of(merged);
        }
        if let Some(base) = self.type_ctx.jsobject_inner(right) {
            let merged = self.type_ctx.union_type(vec![left, base]);
            return self.type_ctx.jsobject_of(merged);
        }
        if matches!(
            self.type_ctx.get(left),
            Some(crate::parser::types::Type::JSObject)
        ) || matches!(
            self.type_ctx.get(right),
            Some(crate::parser::types::Type::JSObject)
        ) {
            return self.type_ctx.jsobject_type();
        }
        self.type_ctx.union_type(vec![left, right])
    }

    fn is_predictable_index_expr(expr: &Expression) -> bool {
        matches!(
            expr,
            Expression::StringLiteral(_)
                | Expression::IntLiteral(_)
                | Expression::FloatLiteral(_)
                | Expression::BooleanLiteral(_)
        )
    }

    fn is_explicit_any_cast_expr(&self, expr: &Expression) -> bool {
        let Expression::TypeCast(cast) = expr else {
            return false;
        };
        use crate::parser::ast::Type as AstType;
        let is_any_ref = |type_ref: &crate::parser::ast::TypeReference| -> bool {
            self.resolve(type_ref.name.name) == "any"
        };
        match &cast.target_type.ty {
            AstType::Reference(type_ref) => is_any_ref(type_ref),
            AstType::Parenthesized(inner) => match &inner.ty {
                AstType::Reference(type_ref) => is_any_ref(type_ref),
                _ => false,
            },
            _ => false,
        }
    }

    fn maybe_escalate_identifier_to_jsobject(
        &mut self,
        expr: &Expression,
        index_expr: Option<&Expression>,
    ) {
        if !self.is_js_mode() {
            return;
        }
        if let Some(idx) = index_expr {
            if Self::is_predictable_index_expr(idx) {
                return;
            }
        }
        let Expression::Identifier(ident) = expr else {
            return;
        };
        let name = self.resolve(ident.name);
        let base_ty = self
            .type_env
            .get(&name)
            .or_else(|| {
                self.symbols
                    .resolve_from_scope(&name, self.current_scope)
                    .and_then(|symbol| {
                        self.inferred_var_types
                            .get(&(symbol.scope_id.0, name.clone()))
                            .copied()
                    })
            })
            .or_else(|| self.get_var_declared_type(&name))
            .or_else(|| {
                self.symbols
                    .resolve_from_scope(&name, self.current_scope)
                    .map(|s| s.ty)
            })
            .unwrap_or_else(|| self.type_ctx.unknown_type());
        let jsobject_ty = self.type_ctx.jsobject_of(base_ty);
        if let Some(symbol) = self.symbols.resolve_from_scope(&name, self.current_scope) {
            self.inferred_var_types
                .insert((symbol.scope_id.0, name.clone()), jsobject_ty);
        }
        self.type_env.set(name, jsobject_ty);
    }

    fn widen_identifier_with_monkeypatch_field(
        &mut self,
        object_expr: &Expression,
        field_name: &str,
        field_ty: TypeId,
    ) {
        if !self.policy.allow_js_dynamic_fallback {
            return;
        }
        let Expression::Identifier(ident) = object_expr else {
            return;
        };
        let name = self.resolve(ident.name);
        let base_ty = self
            .type_env
            .get(&name)
            .or_else(|| {
                self.symbols
                    .resolve_from_scope(&name, self.current_scope)
                    .and_then(|symbol| {
                        self.inferred_var_types
                            .get(&(symbol.scope_id.0, name.clone()))
                            .copied()
                    })
            })
            .or_else(|| self.get_var_declared_type(&name))
            .or_else(|| {
                self.symbols
                    .resolve_from_scope(&name, self.current_scope)
                    .map(|s| s.ty)
            })
            .unwrap_or_else(|| self.type_ctx.unknown_type());

        let inner_base = self.type_ctx.jsobject_inner(base_ty).unwrap_or(base_ty);
        let index_value_ty = if self.allows_dynamic_any() {
            self.type_ctx.any_type()
        } else {
            self.type_ctx.unknown_type()
        };
        let ext_obj_ty = self.type_ctx.intern(crate::parser::types::Type::Object(
            crate::parser::types::ty::ObjectType {
                properties: vec![crate::parser::types::ty::PropertySignature {
                    name: field_name.to_string(),
                    ty: field_ty,
                    optional: false,
                    readonly: false,
                    visibility: crate::parser::ast::Visibility::Public,
                }],
                index_signature: Some(("[key]".to_string(), index_value_ty)),
                call_signatures: vec![],
                construct_signatures: vec![],
            },
        ));
        let widened_inner = self.type_ctx.union_type(vec![inner_base, ext_obj_ty]);
        let wrapped = self.type_ctx.jsobject_of(widened_inner);
        if let Some(symbol) = self.symbols.resolve_from_scope(&name, self.current_scope) {
            self.inferred_var_types
                .insert((symbol.scope_id.0, name.clone()), wrapped);
        }
        self.type_env.set(name, wrapped);
    }

    fn lookup_method_in_class_hierarchy(
        &self,
        class: &crate::parser::types::ty::ClassType,
        method_name: &str,
    ) -> bool {
        if class.methods.iter().any(|m| m.name == method_name) {
            return true;
        }
        if let Some(parent_ty) = class.extends {
            if let Some(crate::parser::types::Type::Class(parent)) = self.type_ctx.get(parent_ty) {
                return self.lookup_method_in_class_hierarchy(parent, method_name);
            }
        }
        false
    }

    fn lookup_static_method_in_class_hierarchy(
        &self,
        class: &crate::parser::types::ty::ClassType,
        method_name: &str,
    ) -> bool {
        if class.static_methods.iter().any(|m| m.name == method_name) {
            return true;
        }
        if let Some(parent_ty) = class.extends {
            if let Some(crate::parser::types::Type::Class(parent)) = self.type_ctx.get(parent_ty) {
                return self.lookup_static_method_in_class_hierarchy(parent, method_name);
            }
        }
        false
    }

    fn infer_method_extract_object_type(&self, expr: &Expression) -> Option<TypeId> {
        match expr {
            Expression::Identifier(ident) => {
                let name = self.resolve(ident.name);
                self.get_var_type(&name)
            }
            Expression::This(_) => self.current_class_type,
            _ => None,
        }
    }

    fn infer_static_method_extract_object_type(&self, expr: &Expression) -> Option<TypeId> {
        let Expression::Identifier(ident) = expr else {
            return None;
        };
        let name = self.resolve(ident.name);
        self.symbols
            .resolve_from_scope(&name, self.current_scope)
            .filter(|symbol| symbol.kind == SymbolKind::Class)
            .map(|symbol| symbol.ty)
            .or_else(|| {
                if self.is_js_mode() {
                    self.type_ctx.lookup_named_type(&name)
                } else {
                    None
                }
            })
    }

    fn infer_extracted_unbound_method_receiver_expectation(
        &self,
        expr: &Expression,
    ) -> Option<HelperReceiverExpectation> {
        let Expression::Member(member) = expr else {
            return None;
        };
        let method_name = self.resolve(member.property.name);
        if let Some(object_ty) = self.infer_static_method_extract_object_type(&member.object) {
            if let Some(crate::parser::types::Type::Class(class)) = self.type_ctx.get(object_ty) {
                if self.lookup_static_method_in_class_hierarchy(class, &method_name) {
                    return Some(HelperReceiverExpectation::StaticClass(object_ty));
                }
            }
        }
        let Some(object_ty) = self.infer_method_extract_object_type(&member.object) else {
            return None;
        };
        match self.type_ctx.get(object_ty).cloned() {
            Some(crate::parser::types::Type::Class(class)) => {
                if self.lookup_method_in_class_hierarchy(&class, &method_name) {
                    Some(HelperReceiverExpectation::Instance(object_ty))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn is_bind_call_expr(&self, expr: &Expression) -> bool {
        let Expression::Call(call) = expr else {
            return false;
        };
        let Expression::Member(member) = call.callee.as_ref() else {
            return false;
        };
        self.resolve(member.property.name) == "bind"
    }

    fn set_unbound_method_var_state(&mut self, name: &str, expr: &Expression) {
        let symbol = self.symbols.resolve_from_scope(name, self.current_scope);
        let scope_id = symbol.map(|s| s.scope_id.0).unwrap_or(self.current_scope.0);
        let key = (scope_id, name.to_string());
        if let Some(receiver_expectation) =
            self.infer_extracted_unbound_method_receiver_expectation(expr)
        {
            if self.is_bind_call_expr(expr) {
                self.unbound_method_vars.remove(&key);
                self.unbound_method_receiver_types.remove(&key);
                return;
            }
            self.unbound_method_vars.insert(key);
            self.unbound_method_receiver_types
                .insert((scope_id, name.to_string()), receiver_expectation);
        } else {
            self.unbound_method_vars.remove(&key);
            self.unbound_method_receiver_types.remove(&key);
        }
    }

    fn is_unbound_method_var(&self, name: &str) -> bool {
        if let Some(symbol) = self.symbols.resolve_from_scope(name, self.current_scope) {
            return self
                .unbound_method_vars
                .contains(&(symbol.scope_id.0, name.to_string()));
        }
        self.unbound_method_vars
            .contains(&(self.current_scope.0, name.to_string()))
    }

    fn get_unbound_method_receiver_expectation_for_var(
        &self,
        name: &str,
    ) -> Option<HelperReceiverExpectation> {
        if let Some(symbol) = self.symbols.resolve_from_scope(name, self.current_scope) {
            return self
                .unbound_method_receiver_types
                .get(&(symbol.scope_id.0, name.to_string()))
                .copied();
        }
        self.unbound_method_receiver_types
            .get(&(self.current_scope.0, name.to_string()))
            .copied()
    }

    fn infer_constructible_alias_expr(&self, expr: &Expression) -> bool {
        match expr {
            Expression::Identifier(ident) => {
                let name = self.resolve(ident.name);
                if let Some(symbol) = self.symbols.resolve_from_scope(&name, self.current_scope) {
                    if symbol.kind == SymbolKind::Class
                        || (self.is_js_mode() && symbol.kind == SymbolKind::Function)
                    {
                        return true;
                    }
                }
                self.is_constructible_var(&name)
            }
            Expression::Function(func) => !func.is_generator,
            Expression::Member(_) => true,
            Expression::TypeCast(cast) => self.infer_constructible_alias_expr(&cast.object),
            Expression::Parenthesized(expr) => {
                self.infer_constructible_alias_expr(&expr.expression)
            }
            _ => false,
        }
    }

    fn type_is_constructible_alias(&self, ty: TypeId) -> bool {
        matches!(
            self.type_ctx.get(ty),
            Some(crate::parser::types::Type::Class(_))
                | Some(crate::parser::types::Type::Object(_))
                | Some(crate::parser::types::Type::Interface(_))
                | Some(crate::parser::types::Type::Function(_))
        )
    }

    fn type_is_js_constructible_callable(&self, ty: TypeId) -> bool {
        use crate::parser::types::Type;

        match self.type_ctx.get(ty) {
            Some(Type::Class(class_ty)) => !class_ty.is_abstract,
            Some(Type::Function(_)) => self.is_js_mode(),
            Some(Type::Object(obj)) => !obj.construct_signatures.is_empty(),
            Some(Type::Interface(iface)) => !iface.construct_signatures.is_empty(),
            Some(Type::Union(union)) => union
                .members
                .iter()
                .copied()
                .any(|member| self.type_is_js_constructible_callable(member)),
            Some(Type::Reference(type_ref)) => self
                .type_ctx
                .lookup_named_type(&type_ref.name)
                .is_some_and(|named| self.type_is_js_constructible_callable(named)),
            Some(Type::Generic(generic)) => self.type_is_js_constructible_callable(generic.base),
            Some(Type::TypeVar(tv)) => tv
                .constraint
                .is_some_and(|constraint| self.type_is_js_constructible_callable(constraint)),
            _ => false,
        }
    }

    fn set_constructible_var_state(&mut self, name: &str, expr: &Expression, ty: TypeId) {
        let symbol = self.symbols.resolve_from_scope(name, self.current_scope);
        let scope_id = symbol.map(|s| s.scope_id.0).unwrap_or(self.current_scope.0);
        let key = (scope_id, name.to_string());
        if self.type_is_constructible_alias(ty) && self.infer_constructible_alias_expr(expr) {
            self.constructible_vars.insert(key);
        } else {
            self.constructible_vars.remove(&key);
        }
    }

    fn inferred_var_scope_id(&self, name: &str) -> u32 {
        self.symbols
            .resolve_from_scope(name, self.current_scope)
            .map(|symbol| symbol.scope_id.0)
            .unwrap_or(self.current_scope.0)
    }

    fn is_constructible_var(&self, name: &str) -> bool {
        if let Some(symbol) = self.symbols.resolve_from_scope(name, self.current_scope) {
            return self
                .constructible_vars
                .contains(&(symbol.scope_id.0, name.to_string()));
        }
        self.constructible_vars
            .contains(&(self.current_scope.0, name.to_string()))
    }

    fn infer_helper_expected_this_type(
        &self,
        target_expr: &Expression,
    ) -> Option<HelperReceiverExpectation> {
        match target_expr {
            Expression::Identifier(ident) => {
                let name = self.resolve(ident.name);
                self.get_unbound_method_receiver_expectation_for_var(&name)
            }
            _ => self.infer_extracted_unbound_method_receiver_expectation(target_expr),
        }
    }

    fn check_helper_this_arg(
        &mut self,
        expected_this_ty: Option<HelperReceiverExpectation>,
        helper_args: &[(TypeId, crate::parser::Span)],
    ) {
        if self.is_js_mode() {
            return;
        }
        let Some(expected) = expected_this_ty else {
            return;
        };
        if let Some((actual_this_ty, actual_span)) = helper_args.first().copied() {
            self.check_assignable(actual_this_ty, expected.type_id(), actual_span);
        }
    }

    fn is_static_helper_receiver_compatible(
        &mut self,
        actual_this_ty: TypeId,
        expected_this_ty: TypeId,
    ) -> bool {
        let Some(expected_class) = self.resolve_class_type(expected_this_ty) else {
            return false;
        };
        let mut cursor = Some(actual_this_ty);
        while let Some(ty) = cursor {
            let Some(class) = self.resolve_class_type(ty) else {
                return false;
            };
            if class.name == expected_class.name {
                return true;
            }
            cursor = class.extends;
        }
        false
    }

    fn helper_call_preserves_return_type(
        &mut self,
        expected_this_ty: Option<HelperReceiverExpectation>,
        helper_args: &[(TypeId, crate::parser::Span)],
    ) -> bool {
        let Some(expected) = expected_this_ty else {
            return true;
        };
        let Some((actual_this_ty, _)) = helper_args.first().copied() else {
            return false;
        };
        match expected {
            HelperReceiverExpectation::Instance(expected_ty) => {
                let mut assign_ctx = self.make_assignability_ctx();
                assign_ctx.is_assignable(actual_this_ty, expected_ty)
            }
            HelperReceiverExpectation::StaticClass(expected_ty) => {
                self.is_static_helper_receiver_compatible(actual_this_ty, expected_ty)
            }
        }
    }

    fn helper_call_return_type(
        &mut self,
        target_fn: &crate::parser::types::ty::FunctionType,
        expected_this_ty: Option<HelperReceiverExpectation>,
        helper_args: &[(TypeId, crate::parser::Span)],
    ) -> TypeId {
        if self.helper_call_preserves_return_type(expected_this_ty, helper_args) {
            target_fn.return_type
        } else {
            self.inference_fallback_type()
        }
    }

    /// Resolve a parser Symbol to a String
    #[inline]
    fn resolve(&self, sym: ParserSymbol) -> String {
        self.interner.resolve(sym).to_string()
    }

    /// Enter a new scope (like entering a block or function)
    /// Mirrors what the binder does when it pushes a scope.
    /// No-op when inside arrow function bodies (arrow_depth > 0) because
    /// the binder never visits inside expressions, so those scopes don't
    /// exist in the symbol table.
    fn enter_scope(&mut self) {
        if self.arrow_depth > 0 {
            return;
        }
        self.scope_stack.push(self.current_scope);
        let mut candidate = self.next_scope_id as usize;
        while candidate < self.symbols.scope_count() {
            let scope_id = super::symbols::ScopeId(candidate as u32);
            let scope = self.symbols.get_scope(scope_id);
            if scope.parent == Some(self.current_scope) {
                self.current_scope = scope_id;
                self.next_scope_id = scope_id.0 + 1;
                return;
            }
            candidate += 1;
        }

        let scope_id = super::symbols::ScopeId(self.next_scope_id);
        self.next_scope_id += 1;
        self.current_scope = scope_id;
    }

    /// Exit the current scope, returning to parent
    fn exit_scope(&mut self) {
        if self.arrow_depth > 0 {
            return;
        }
        if let Some(parent) = self.scope_stack.pop() {
            self.current_scope = parent;
        }
    }

    /// Check a module
    ///
    /// Returns the check result containing inferred types and capture information.
    /// Inferred types should be applied to the symbol table using `update_type`.
    pub fn check_module(mut self, module: &Module) -> Result<CheckResult, Vec<CheckError>> {
        self.index_class_ast_summaries(module);
        // Mirror binder module scope so top-level resolution is local->global.
        self.enter_scope();
        for stmt in &module.statements {
            self.check_stmt(stmt);
        }
        self.exit_scope();

        // Collect unused variable warnings
        self.collect_unused_warnings();

        if self.errors.is_empty() {
            Ok(CheckResult {
                inferred_types: self.inferred_var_types,
                captures: self.capture_info,
                expr_types: self.expr_types,
                type_annotation_types: self.type_annotation_types,
                warnings: self.warnings,
            })
        } else {
            Err(self.errors)
        }
    }

    fn index_class_ast_summaries(&mut self, module: &Module) {
        self.class_ast_summaries.clear();
        for stmt in &module.statements {
            self.index_class_ast_summary_stmt(stmt);
        }
    }

    fn index_class_ast_summary_stmt(&mut self, stmt: &Statement) {
        match stmt {
            Statement::ClassDecl(class) => {
                let class_name = self.resolve(class.name.name);
                let extends = class.extends.as_ref().and_then(|ann| match &ann.ty {
                    crate::parser::ast::Type::Reference(reference) => {
                        Some(self.resolve(reference.name.name))
                    }
                    _ => None,
                });

                let mut abstract_methods = FxHashSet::default();
                let mut concrete_methods = FxHashSet::default();
                for member in &class.members {
                    if let ClassMember::Method(method) = member {
                        let method_name = self.resolve(method.name.name);
                        if method.is_abstract || method.body.is_none() {
                            abstract_methods.insert(method_name);
                        } else {
                            concrete_methods.insert(method_name);
                        }
                    }
                }

                self.class_ast_summaries.insert(
                    class_name,
                    ClassAstSummary {
                        is_abstract: class.is_abstract,
                        extends,
                        abstract_methods,
                        concrete_methods,
                    },
                );
            }
            Statement::ExportDecl(ExportDecl::Declaration(inner_stmt)) => {
                self.index_class_ast_summary_stmt(inner_stmt);
            }
            _ => {}
        }
    }

    fn required_abstract_methods_for_class(
        &self,
        class_name: &str,
        visiting: &mut FxHashSet<String>,
    ) -> FxHashSet<String> {
        if !visiting.insert(class_name.to_string()) {
            return FxHashSet::default();
        }

        let Some(summary) = self.class_ast_summaries.get(class_name) else {
            return FxHashSet::default();
        };

        let mut required = if let Some(parent_name) = summary.extends.as_deref() {
            self.required_abstract_methods_for_class(parent_name, visiting)
        } else {
            FxHashSet::default()
        };

        for concrete in &summary.concrete_methods {
            required.remove(concrete);
        }
        if summary.is_abstract || !summary.abstract_methods.is_empty() {
            for abstract_name in &summary.abstract_methods {
                required.insert(abstract_name.clone());
            }
        }

        required
    }

    fn check_abstract_class_contract(&mut self, class: &ClassDecl) {
        if class.is_abstract {
            return;
        }

        let Some(parent_name) = class.extends.as_ref().and_then(|ann| match &ann.ty {
            crate::parser::ast::Type::Reference(reference) => {
                Some(self.resolve(reference.name.name))
            }
            _ => None,
        }) else {
            return;
        };

        let mut visiting = FxHashSet::default();
        let mut required = self.required_abstract_methods_for_class(&parent_name, &mut visiting);
        if required.is_empty() {
            return;
        }

        for member in &class.members {
            if let ClassMember::Method(method) = member {
                if !method.is_abstract && method.body.is_some() {
                    required.remove(&self.resolve(method.name.name));
                }
            }
        }

        if !required.is_empty() {
            let mut missing: Vec<_> = required.into_iter().collect();
            missing.sort_unstable();
            self.errors.push(CheckError::ConstraintViolation {
                message: format!(
                    "Class '{}' must implement abstract member(s): {}",
                    self.resolve(class.name.name),
                    missing.join(", ")
                ),
                span: class.span,
            });
        }
    }

    /// Collect warnings for unused variables across all scopes
    fn collect_unused_warnings(&mut self) {
        use super::symbols::SymbolKind;

        for scope in self.symbols.all_scopes() {
            for symbol in scope.symbols.values() {
                // Only warn about variables (not functions, classes, types, etc.)
                if symbol.kind != SymbolKind::Variable {
                    continue;
                }

                // Skip if already referenced
                if symbol.referenced {
                    continue;
                }

                // Skip _-prefixed names (intentionally unused)
                if symbol.name.starts_with('_') {
                    continue;
                }

                // Skip exported symbols
                if symbol.flags.is_exported {
                    continue;
                }

                self.warnings.push(CheckWarning::UnusedVariable {
                    name: symbol.name.clone(),
                    span: symbol.span,
                });
            }
        }
    }

    /// Get the errors collected during checking
    pub fn errors(&self) -> &[CheckError] {
        &self.errors
    }

    /// Check statement
    fn check_stmt(&mut self, stmt: &Statement) {
        match stmt {
            Statement::VariableDecl(decl) => self.check_var_decl(decl),
            Statement::FunctionDecl(func) => self.check_function(func),
            Statement::Expression(expr_stmt) => {
                self.check_expr(&expr_stmt.expression);
            }
            Statement::Return(ret) => self.check_return(ret),
            Statement::Yield(yld) => self.check_yield(yld),
            Statement::If(if_stmt) => self.check_if(if_stmt),
            Statement::While(while_stmt) => self.check_while(while_stmt),
            Statement::For(for_stmt) => self.check_for(for_stmt),
            Statement::Block(block) => {
                // Enter block scope (mirrors binder's push_scope)
                self.enter_scope();
                for stmt in &block.statements {
                    self.check_stmt(stmt);
                }
                // Exit block scope (mirrors binder's pop_scope)
                self.exit_scope();
            }
            Statement::Switch(switch_stmt) => self.check_switch(switch_stmt),
            Statement::Try(try_stmt) => self.check_try(try_stmt),
            Statement::ForOf(for_of) => self.check_for_of(for_of),
            Statement::ForIn(for_in) => self.check_for_in(for_in),
            Statement::Labeled(labeled) => self.check_stmt(&labeled.body),
            Statement::ClassDecl(class) => {
                // Check class declaration including decorators
                self.check_class(class);
            }
            Statement::TypeAliasDecl(alias) => {
                // Sync scope for generic type aliases (binder creates a scope for type params)
                if alias.type_params.as_ref().is_some_and(|p| !p.is_empty()) {
                    self.enter_scope();
                    self.exit_scope();
                }
            }
            Statement::ExportDecl(ExportDecl::Declaration(inner_stmt)) => {
                self.check_stmt(inner_stmt)
            }
            Statement::ExportDecl(ExportDecl::Default { expression, .. }) => {
                let default_ty = self.check_expr(expression);
                self.inferred_var_types
                    .insert((self.current_scope.0, "default".to_string()), default_ty);
                self.type_env.set("default".to_string(), default_ty);
            }
            _ => {}
        }
    }

    /// Check variable declaration
    fn check_var_decl(&mut self, decl: &VariableDecl) {
        if !self.policy.allow_bare_let
            && decl.kind == VariableKind::Let
            && decl.type_annotation.is_none()
            && decl.initializer.is_none()
        {
            self.errors
                .push(CheckError::StrictBareLetForbidden { span: decl.span });
            return;
        }

        if let Some(ref init) = decl.initializer {
            let init_ty = self.check_expr(init);

            match &decl.pattern {
                Pattern::Identifier(ident) => {
                    let name = self.resolve(ident.name);
                    let inferred_scope_id = self.inferred_var_scope_id(&name);

                    // Determine the variable's type
                    let var_ty = if decl.type_annotation.is_some() {
                        // Resolve the declared annotation in checker scope. This is
                        // authoritative for variable declarations, even when binder
                        // prepass placeholders are still attached to the symbol entry.
                        let resolved_ty =
                            self.resolve_type_annotation(decl.type_annotation.as_ref().unwrap());
                        self.check_assignable(init_ty, resolved_ty, *init.span());
                        self.inferred_var_types
                            .insert((inferred_scope_id, name.clone()), resolved_ty);
                        resolved_ty
                    } else {
                        // No type annotation - infer type from initializer
                        // Store the inferred type for later lookups
                        self.inferred_var_types
                            .insert((inferred_scope_id, name.clone()), init_ty);
                        init_ty
                    };

                    // Also add to type_env so nested arrow functions can see it
                    self.set_unbound_method_var_state(&name, init);
                    self.set_constructible_var_state(&name, init, var_ty);
                    self.type_env.set(name, var_ty);
                }
                Pattern::Array(_) | Pattern::Object(_) => {
                    // For destructuring, infer element/property types from the
                    // initializer and register them for each binding.
                    self.check_destructure_pattern(&decl.pattern, init_ty);
                }
                _ => {}
            }
        }
    }

    /// Infer types for variables bound in a destructuring pattern.
    fn check_destructure_pattern(&mut self, pattern: &Pattern, value_ty: TypeId) {
        match pattern {
            Pattern::Identifier(ident) => {
                let name = self.resolve(ident.name);
                let inferred_scope_id = self.inferred_var_scope_id(&name);
                self.inferred_var_types
                    .insert((inferred_scope_id, name.clone()), value_ty);
                self.type_env.set(name, value_ty);
            }
            Pattern::Array(array_pat) => {
                if let Some(crate::parser::types::Type::Tuple(tuple_ty)) =
                    self.type_ctx.get(value_ty).cloned()
                {
                    for (idx, elem) in array_pat.elements.iter().enumerate() {
                        if let Some(elem) = elem {
                            let elem_ty = tuple_ty
                                .elements
                                .get(idx)
                                .copied()
                                .unwrap_or_else(|| self.inference_fallback_type());
                            self.check_destructure_pattern(&elem.pattern, elem_ty);
                        }
                    }
                    if let Some(rest) = &array_pat.rest {
                        let rest_members: Vec<TypeId> = tuple_ty
                            .elements
                            .iter()
                            .skip(array_pat.elements.len())
                            .copied()
                            .collect();
                        let rest_elem_ty = if rest_members.is_empty() {
                            self.inference_fallback_type()
                        } else if rest_members.len() == 1 {
                            rest_members[0]
                        } else {
                            self.type_ctx.union_type(rest_members)
                        };
                        let rest_ty = self.type_ctx.array_type(rest_elem_ty);
                        self.check_destructure_pattern(rest, rest_ty);
                    }
                } else {
                    // Extract element type from the array type
                    let elem_ty = if let Some(crate::parser::types::Type::Array(arr)) =
                        self.type_ctx.get(value_ty).cloned()
                    {
                        arr.element
                    } else {
                        self.inference_fallback_type()
                    };

                    for elem in array_pat.elements.iter().flatten() {
                        self.check_destructure_pattern(&elem.pattern, elem_ty);
                    }
                    if let Some(rest) = &array_pat.rest {
                        // Rest element gets the same array type
                        self.check_destructure_pattern(rest, value_ty);
                    }
                }
            }
            Pattern::Object(obj_pat) => {
                for prop in &obj_pat.properties {
                    let prop_name = self.resolve(prop.key.name);
                    let prop_ty = self.destructure_object_property_type(value_ty, &prop_name);
                    // If there's a default expression, use its type when property is missing
                    let final_ty = if let Some(ref default_expr) = prop.default {
                        let default_ty = self.check_expr(default_expr);
                        prop_ty.unwrap_or(default_ty)
                    } else {
                        prop_ty.unwrap_or_else(|| self.inference_fallback_type())
                    };
                    self.check_destructure_pattern(&prop.value, final_ty);
                }
                if let Some(rest_ident) = &obj_pat.rest {
                    let unknown = self.inference_fallback_type();
                    let name = self.resolve(rest_ident.name);
                    let inferred_scope_id = self.inferred_var_scope_id(&name);
                    self.inferred_var_types
                        .insert((inferred_scope_id, name.clone()), unknown);
                    self.type_env.set(name, unknown);
                }
            }
            Pattern::Rest(rest_pat) => {
                self.check_destructure_pattern(&rest_pat.argument, value_ty);
            }
        }
    }

    fn destructure_object_property_type(&mut self, ty: TypeId, prop_name: &str) -> Option<TypeId> {
        use crate::parser::types::Type;

        match self.type_ctx.get(ty).cloned() {
            Some(Type::Object(obj)) => obj
                .properties
                .iter()
                .find(|p| p.name == prop_name)
                .map(|p| p.ty)
                .or_else(|| obj.index_signature.map(|(_, sig_ty)| sig_ty)),
            Some(Type::Class(class_ty)) => self
                .lookup_class_member(&class_ty, prop_name)
                .map(|(member_ty, _)| member_ty),
            Some(Type::Interface(interface_ty)) => {
                self.lookup_interface_member(&interface_ty, prop_name)
            }
            Some(Type::Reference(type_ref)) => {
                let named_ty = self.type_ctx.lookup_named_type(&type_ref.name)?;
                let resolved_ty = match self.type_ctx.get(named_ty).cloned() {
                    Some(Type::Class(class_ty))
                        if type_ref
                            .type_args
                            .as_ref()
                            .is_some_and(|args| !args.is_empty())
                            && !class_ty.type_params.is_empty() =>
                    {
                        let args = type_ref.type_args.as_ref().expect("checked is_some");
                        self.instantiate_class_type(&class_ty, args)
                    }
                    Some(_) => named_ty,
                    None => return None,
                };
                self.destructure_object_property_type(resolved_ty, prop_name)
            }
            Some(Type::Generic(generic)) => {
                let resolved_ty = match self.type_ctx.get(generic.base).cloned() {
                    Some(Type::Class(class_ty)) if !generic.type_args.is_empty() => {
                        self.instantiate_class_type(&class_ty, &generic.type_args)
                    }
                    Some(_) => generic.base,
                    None => return None,
                };
                self.destructure_object_property_type(resolved_ty, prop_name)
            }
            Some(Type::TypeVar(tv)) => tv.constraint.and_then(|constraint_ty| {
                self.destructure_object_property_type(constraint_ty, prop_name)
            }),
            Some(Type::Union(union)) => {
                let mut member_types = Vec::new();
                for member in union.members {
                    if let Some(member_ty) =
                        self.destructure_object_property_type(member, prop_name)
                    {
                        if !member_types.contains(&member_ty) {
                            member_types.push(member_ty);
                        }
                    }
                }
                if member_types.is_empty() {
                    None
                } else if member_types.len() == 1 {
                    Some(member_types[0])
                } else {
                    Some(self.type_ctx.union_type(member_types))
                }
            }
            _ => None,
        }
    }

    /// Check function declaration
    fn check_function(&mut self, func: &FunctionDecl) {
        let saved_env = self.type_env.clone();
        self.type_env = TypeEnv::new();

        if !self.allows_implicit_any() {
            for param in &func.params {
                if param.type_annotation.is_none() {
                    self.errors
                        .push(CheckError::ImplicitAnyForbidden { span: param.span });
                }
            }
        }

        let func_name = self.resolve(func.name.name);
        let symbol_func_ty = self
            .symbols
            .resolve_from_scope(&func_name, self.current_scope)
            .and_then(|symbol| {
                // Duplicate-tolerant helper builds can admit multiple same-name declarations.
                // Only trust the symbol-table function type when this declaration is the active symbol.
                if symbol.span == func.name.span {
                    self.type_ctx.get(symbol.ty).and_then(|ty| match ty {
                        crate::parser::types::Type::Function(func_ty) => Some(func_ty.clone()),
                        _ => None,
                    })
                } else {
                    None
                }
            });

        let has_explicit_return_annotation = func.return_type.is_some();
        let declared_return_ty = if let Some(func_ty) = &symbol_func_ty {
            func_ty.return_type
        } else {
            func.return_type
                .as_ref()
                .map(|ann| self.resolve_type_annotation(ann))
                .unwrap_or_else(|| self.type_ctx.void_type())
        };
        let mut return_ty = declared_return_ty;

        // For async functions, the declared return type is Promise<T>,
        // but return statements should check against T (the inner type)
        if func.is_async {
            if let Some(crate::parser::types::Type::Task(task_ty)) = self.type_ctx.get(return_ty) {
                return_ty = task_ty.result;
            }
        }

        let param_types: Vec<TypeId> = if let Some(func_ty) = &symbol_func_ty {
            func_ty.params.iter().cloned().collect()
        } else {
            func.params
                .iter()
                .map(|param| {
                    param
                        .type_annotation
                        .as_ref()
                        .map(|ann| self.resolve_type_annotation(ann))
                        .unwrap_or_else(|| self.inference_fallback_type())
                })
                .collect()
        };

        // Keep annotation-type map complete even when param/return types are reused
        // from binder-resolved function symbols.
        if let Some(func_ty) = &symbol_func_ty {
            let mut positional_idx = 0usize;
            for param in &func.params {
                let Some(type_ann) = &param.type_annotation else {
                    continue;
                };
                let ann_id = type_ann as *const _ as usize;
                let ann_ty = if param.is_rest {
                    func_ty
                        .rest_param
                        .unwrap_or_else(|| self.type_ctx.unknown_type())
                } else {
                    let ty = func_ty
                        .params
                        .get(positional_idx)
                        .copied()
                        .unwrap_or_else(|| self.type_ctx.unknown_type());
                    positional_idx += 1;
                    ty
                };
                self.type_annotation_types.insert(ann_id, ann_ty);
            }

            if let Some(return_ann) = &func.return_type {
                let ann_id = return_ann as *const _ as usize;
                self.type_annotation_types
                    .insert(ann_id, declared_return_ty);
            }
        }

        // Set current function return type.
        // Unannotated function declarations infer from collected return statements.
        let prev_return_ty = self.current_function_return_type;
        self.current_function_return_type = if has_explicit_return_annotation {
            Some(return_ty)
        } else {
            None
        };
        if !has_explicit_return_annotation {
            self.return_type_collector.push(Vec::new());
        }

        // Enter function scope (mirrors binder's push_scope for function)
        self.enter_scope();

        // Register parameter types in type_env for destructuring patterns
        for (i, param) in func.params.iter().enumerate() {
            if let Some(&param_ty) = param_types.get(i) {
                self.check_destructure_pattern(&param.pattern, param_ty);
            }
        }

        // Check body
        for stmt in &func.body.statements {
            self.check_stmt(stmt);
        }

        let inferred_return_ty = if !has_explicit_return_annotation {
            let collected = self.return_type_collector.pop().unwrap_or_default();
            if collected.is_empty() {
                self.type_ctx.void_type()
            } else {
                let mut unique = Vec::new();
                for ty in collected {
                    if !unique.contains(&ty) {
                        unique.push(ty);
                    }
                }
                if unique.len() == 1 {
                    unique[0]
                } else {
                    self.type_ctx.union_type(unique)
                }
            }
        } else {
            self.type_ctx.void_type()
        };

        // Exit function scope
        self.exit_scope();

        // Restore previous return type
        self.current_function_return_type = prev_return_ty;

        if !has_explicit_return_annotation {
            let inferred_func_ty = if let Some(func_ty) = &symbol_func_ty {
                self.type_ctx.function_type_with_rest(
                    func_ty.params.clone(),
                    inferred_return_ty,
                    func_ty.is_async,
                    func_ty.min_params,
                    func_ty.rest_param,
                )
            } else {
                let min_params = if self.is_js_mode() {
                    0
                } else {
                    func.params
                        .iter()
                        .filter(|p| !p.is_rest)
                        .filter(|p| p.default_value.is_none() && !p.optional)
                        .count()
                };
                self.type_ctx.function_type_with_rest(
                    param_types,
                    inferred_return_ty,
                    func.is_async,
                    min_params,
                    None,
                )
            };

            if let Some(symbol) = self
                .symbols
                .resolve_from_scope(&func_name, self.current_scope)
            {
                if symbol.span == func.name.span {
                    self.inferred_var_types
                        .insert((symbol.scope_id.0, func_name.clone()), inferred_func_ty);
                }
            }
        }

        self.type_env = saved_env;
    }

    /// Sync scopes for class declaration to keep scope IDs in sync with binder
    /// This mirrors the binder's scope creation pattern without doing type checking
    fn sync_class_scopes(&mut self, class: &crate::parser::ast::ClassDecl) {
        // Enter class scope (mirrors binder's push_scope for class at line 687)
        self.enter_scope();

        // Mirror binder's temporary scope for methods with type params during type resolution
        // (binder lines 743-778)
        for member in &class.members {
            if let crate::parser::ast::ClassMember::Method(method) = member {
                if method
                    .type_params
                    .as_ref()
                    .is_some_and(|tps| !tps.is_empty())
                {
                    self.enter_scope();
                    self.exit_scope();
                }
            }
        }

        // Mirror binder's scope for method/constructor bodies (binder lines 833-871)
        for member in &class.members {
            match member {
                crate::parser::ast::ClassMember::Method(method) => {
                    if method.body.is_some() {
                        self.enter_scope();
                        // Recursively count nested scopes without type checking
                        if let Some(ref body) = method.body {
                            self.sync_stmts_scopes(&body.statements);
                        }
                        self.exit_scope();
                    }
                }
                crate::parser::ast::ClassMember::Constructor(ctor) => {
                    self.enter_scope();
                    self.sync_stmts_scopes(&ctor.body.statements);
                    self.exit_scope();
                }
                _ => {}
            }
        }

        // Exit class scope (mirrors binder's pop_scope at line 873)
        self.exit_scope();
    }

    /// Sync scopes for statements without doing type checking
    /// Just enters/exits scopes to keep scope IDs in sync with binder
    fn sync_stmts_scopes(&mut self, stmts: &[Statement]) {
        for stmt in stmts {
            self.sync_stmt_scopes(stmt);
        }
    }

    /// Sync scopes for a single statement without type checking
    fn sync_stmt_scopes(&mut self, stmt: &Statement) {
        match stmt {
            Statement::Block(block) => {
                self.enter_scope();
                self.sync_stmts_scopes(&block.statements);
                self.exit_scope();
            }
            Statement::FunctionDecl(func) => {
                self.enter_scope();
                self.sync_stmts_scopes(&func.body.statements);
                self.exit_scope();
            }
            Statement::While(while_stmt) => {
                self.enter_scope();
                self.sync_stmt_scopes(&while_stmt.body);
                self.exit_scope();
            }
            Statement::For(for_stmt) => {
                self.enter_scope();
                self.sync_stmt_scopes(&for_stmt.body);
                self.exit_scope();
            }
            Statement::ForOf(for_of) => {
                self.enter_scope();
                self.sync_stmt_scopes(&for_of.body);
                self.exit_scope();
            }
            Statement::ForIn(for_in) => {
                self.enter_scope();
                self.sync_stmt_scopes(&for_in.body);
                self.exit_scope();
            }
            Statement::Labeled(labeled) => {
                self.sync_stmt_scopes(&labeled.body);
            }
            Statement::If(if_stmt) => {
                // If body doesn't create scope unless it's a block
                self.sync_stmt_scopes(&if_stmt.then_branch);
                if let Some(ref alt) = if_stmt.else_branch {
                    self.sync_stmt_scopes(alt);
                }
            }
            Statement::Try(try_stmt) => {
                // Try body doesn't create a scope itself
                for s in &try_stmt.body.statements {
                    self.sync_stmt_scopes(s);
                }
                if let Some(ref catch) = try_stmt.catch_clause {
                    self.enter_scope();
                    self.sync_stmts_scopes(&catch.body.statements);
                    self.exit_scope();
                }
                if let Some(ref finally) = try_stmt.finally_clause {
                    for s in &finally.statements {
                        self.sync_stmt_scopes(s);
                    }
                }
            }
            Statement::Switch(switch_stmt) => {
                // Switch doesn't create scope, but cases might have blocks
                // Note: default case is included in cases with test: None
                for case in &switch_stmt.cases {
                    for s in &case.consequent {
                        self.sync_stmt_scopes(s);
                    }
                }
            }
            Statement::ClassDecl(class) => {
                self.sync_class_scopes(class);
            }
            Statement::TypeAliasDecl(alias) => {
                // Sync scope for generic type aliases (binder creates a scope for type params)
                if alias.type_params.as_ref().is_some_and(|p| !p.is_empty()) {
                    self.enter_scope();
                    self.exit_scope();
                }
            }
            Statement::ExportDecl(ExportDecl::Declaration(inner_stmt)) => {
                self.sync_stmt_scopes(inner_stmt)
            }
            // Other statements don't create scopes
            _ => {}
        }
    }

    /// Check class declaration
    ///
    /// This checks decorators on the class, methods, fields, and parameters,
    /// then syncs scopes with the binder for all nested code.
    fn check_class(&mut self, class: &crate::parser::ast::ClassDecl) {
        // Check class decorators first (before entering class scope)
        self.check_class_decorators(class);

        // Get the class type for 'this' checking inside methods
        let class_name = self.resolve(class.name.name);
        let prev_class_type = self.current_class_type;
        if let Some(symbol) = self
            .symbols
            .resolve_from_scope(&class_name, self.current_scope)
        {
            self.current_class_type = Some(symbol.ty);
        }

        // Enter class scope (mirrors binder's push_scope for class)
        self.enter_scope();

        // Mirror binder's temporary scope for methods with type params during type resolution.
        // This keeps scope IDs in sync before checking method/ctor bodies.
        for member in &class.members {
            if let crate::parser::ast::ClassMember::Method(method) = member {
                if method
                    .type_params
                    .as_ref()
                    .is_some_and(|tps| !tps.is_empty())
                {
                    self.enter_scope();
                    self.exit_scope();
                }
            }
        }

        // Check decorators + bodies for methods, constructors, and fields.
        for member in &class.members {
            match member {
                crate::parser::ast::ClassMember::Method(method) => {
                    // Register method-level type parameters so resolve_type can
                    // find them during build_method_type (param/return resolution).
                    self.method_type_params.clear();
                    if let Some(ref type_params) = method.type_params {
                        for tp in type_params {
                            let param_name = self.resolve(tp.name.name);
                            let constraint_ty = tp
                                .constraint
                                .as_ref()
                                .map(|c| self.resolve_type_annotation(c));
                            let type_var = self
                                .type_ctx
                                .type_variable_with_constraint(param_name.clone(), constraint_ty);
                            self.method_type_params.insert(param_name, type_var);
                        }
                    }

                    let method_ty = self.build_method_type(method);
                    self.check_method_decorators(method, method_ty);
                    for param in &method.params {
                        self.check_parameter_decorators(param);
                    }

                    if let Some(ref body) = method.body {
                        let saved_env = self.type_env.clone();
                        self.type_env = TypeEnv::new();

                        // Collect parameter types before entering scope (to avoid borrow issues)
                        let param_types: Option<Vec<TypeId>> =
                            if let Some(crate::parser::types::Type::Function(method_fn_ty)) =
                                self.type_ctx.get(method_ty)
                            {
                                Some(method_fn_ty.params.iter().cloned().collect())
                            } else {
                                None
                            };

                        // Enter method scope (binder creates one for every concrete method body)
                        self.enter_scope();

                        // Register parameter types in type_env for destructuring patterns
                        if let Some(ref pts) = param_types {
                            for (i, param) in method.params.iter().enumerate() {
                                if let Some(&param_ty) = pts.get(i) {
                                    self.check_destructure_pattern(&param.pattern, param_ty);
                                }
                            }
                        }

                        // Set return type for return statement checking.
                        // In JS mode, unannotated methods infer from explicit returns
                        // instead of defaulting to `void`.
                        let prev_return_ty = self.current_function_return_type;
                        let declared_return_ty = method
                            .return_type
                            .as_ref()
                            .map(|t| self.resolve_type_annotation(t));
                        let effective_return_ty = if method.is_async {
                            declared_return_ty.map(|ty| {
                                if let Some(crate::parser::types::Type::Task(task_ty)) =
                                    self.type_ctx.get(ty)
                                {
                                    task_ty.result
                                } else {
                                    ty
                                }
                            })
                        } else {
                            declared_return_ty
                        };
                        self.current_function_return_type = effective_return_ty;
                        if effective_return_ty.is_none() {
                            self.return_type_collector.push(Vec::new());
                        }

                        for stmt in &body.statements {
                            self.check_stmt(stmt);
                        }

                        if effective_return_ty.is_none() {
                            let _ = self.return_type_collector.pop().unwrap_or_default();
                        }
                        self.current_function_return_type = prev_return_ty;
                        self.method_type_params.clear();
                        self.exit_scope();
                        self.type_env = saved_env;
                    }
                }
                crate::parser::ast::ClassMember::Constructor(ctor) => {
                    let saved_env = self.type_env.clone();
                    self.type_env = TypeEnv::new();

                    for param in &ctor.params {
                        if !self.allows_implicit_any() && param.type_annotation.is_none() {
                            self.errors
                                .push(CheckError::ImplicitAnyForbidden { span: param.span });
                        }
                        self.check_parameter_decorators(param);
                    }

                    // Enter constructor scope (binder always creates one)
                    self.enter_scope();

                    // Register parameter types in type_env for destructuring patterns
                    for param in &ctor.params {
                        let param_ty = if let Some(ref ann) = param.type_annotation {
                            self.resolve_type_annotation(ann)
                        } else {
                            self.type_ctx.unknown_type()
                        };
                        self.check_destructure_pattern(&param.pattern, param_ty);
                    }

                    let prev_return_ty = self.current_function_return_type;
                    let prev_in_ctor = self.in_constructor;
                    self.current_function_return_type = Some(self.type_ctx.void_type());
                    self.in_constructor = true;

                    for stmt in &ctor.body.statements {
                        self.check_stmt(stmt);
                    }

                    self.in_constructor = prev_in_ctor;
                    self.current_function_return_type = prev_return_ty;
                    self.exit_scope();
                    self.type_env = saved_env;
                }
                crate::parser::ast::ClassMember::Field(field) => {
                    self.check_field_decorators(field);
                    if !self.allows_implicit_any()
                        && field.type_annotation.is_none()
                        && field.initializer.is_none()
                    {
                        self.errors
                            .push(CheckError::ImplicitAnyForbidden { span: field.span });
                    }

                    if let Some(ref init) = field.initializer {
                        let init_ty = self.check_expr(init);
                        if let Some(ref ann) = field.type_annotation {
                            let field_ty = self.resolve_type_annotation(ann);
                            self.check_assignable(init_ty, field_ty, *init.span());
                        }
                    }
                }
                crate::parser::ast::ClassMember::StaticBlock(block) => {
                    self.enter_scope();
                    for stmt in &block.statements {
                        self.check_stmt(stmt);
                    }
                    self.exit_scope();
                }
            }
        }

        // strictPropertyInitialization: non-static fields without initializer
        // must be definitely assigned in constructor.
        if self.policy.strict_property_initialization {
            let required_fields: Vec<(String, Span)> = class
                .members
                .iter()
                .filter_map(|m| {
                    if let crate::parser::ast::ClassMember::Field(field) = m {
                        if !field.is_static && field.initializer.is_none() {
                            return Some((self.resolve(field.name.name), field.span));
                        }
                    }
                    None
                })
                .collect();

            if !required_fields.is_empty() {
                let ctor_opt = class.members.iter().find_map(|m| {
                    if let crate::parser::ast::ClassMember::Constructor(ctor) = m {
                        Some(ctor)
                    } else {
                        None
                    }
                });

                let mut assigned = FxHashSet::default();
                if let Some(ctor) = ctor_opt {
                    for param in &ctor.params {
                        if param.visibility.is_some() {
                            if let Pattern::Identifier(id) = &param.pattern {
                                assigned.insert(self.resolve(id.name));
                            }
                        }
                    }
                    self.collect_this_assignments_block(&ctor.body, &mut assigned);
                }

                for (name, span) in required_fields {
                    if !assigned.contains(&name) {
                        self.errors.push(CheckError::StrictPropertyInitialization {
                            property: name,
                            span,
                        });
                    }
                }
            }
        }

        self.check_abstract_class_contract(class);

        // Exit class scope
        self.exit_scope();

        // Restore previous class type (for nested classes)
        self.current_class_type = prev_class_type;
    }

    fn collect_this_assignments_block(
        &self,
        block: &crate::parser::ast::BlockStatement,
        assigned: &mut FxHashSet<String>,
    ) {
        for stmt in &block.statements {
            self.collect_this_assignments_stmt(stmt, assigned);
        }
    }

    fn collect_this_assignments_stmt(
        &self,
        stmt: &crate::parser::ast::Statement,
        assigned: &mut FxHashSet<String>,
    ) {
        use crate::parser::ast::Statement;
        match stmt {
            Statement::Expression(expr_stmt) => {
                self.collect_this_assignments_expr(&expr_stmt.expression, assigned)
            }
            Statement::If(s) => {
                self.collect_this_assignments_expr(&s.condition, assigned);
                self.collect_this_assignments_stmt(&s.then_branch, assigned);
                if let Some(else_branch) = &s.else_branch {
                    self.collect_this_assignments_stmt(else_branch, assigned);
                }
            }
            Statement::While(s) => {
                self.collect_this_assignments_expr(&s.condition, assigned);
                self.collect_this_assignments_stmt(&s.body, assigned);
            }
            Statement::DoWhile(s) => {
                self.collect_this_assignments_stmt(&s.body, assigned);
                self.collect_this_assignments_expr(&s.condition, assigned);
            }
            Statement::For(s) => {
                if let Some(init) = &s.init {
                    match init {
                        crate::parser::ast::ForInit::Expression(e) => {
                            self.collect_this_assignments_expr(e, assigned)
                        }
                        crate::parser::ast::ForInit::VariableDecl(decl) => {
                            if let Some(init) = &decl.initializer {
                                self.collect_this_assignments_expr(init, assigned);
                            }
                        }
                    }
                }
                if let Some(test) = &s.test {
                    self.collect_this_assignments_expr(test, assigned);
                }
                if let Some(update) = &s.update {
                    self.collect_this_assignments_expr(update, assigned);
                }
                self.collect_this_assignments_stmt(&s.body, assigned);
            }
            Statement::ForOf(s) => {
                self.collect_this_assignments_expr(&s.right, assigned);
                self.collect_this_assignments_stmt(&s.body, assigned);
            }
            Statement::ForIn(s) => {
                self.collect_this_assignments_expr(&s.right, assigned);
                self.collect_this_assignments_stmt(&s.body, assigned);
            }
            Statement::Labeled(s) => {
                self.collect_this_assignments_stmt(&s.body, assigned);
            }
            Statement::Switch(s) => {
                self.collect_this_assignments_expr(&s.discriminant, assigned);
                for case in &s.cases {
                    if let Some(test) = &case.test {
                        self.collect_this_assignments_expr(test, assigned);
                    }
                    for cons in &case.consequent {
                        self.collect_this_assignments_stmt(cons, assigned);
                    }
                }
            }
            Statement::Try(s) => {
                self.collect_this_assignments_block(&s.body, assigned);
                if let Some(c) = &s.catch_clause {
                    self.collect_this_assignments_block(&c.body, assigned);
                }
                if let Some(f) = &s.finally_clause {
                    self.collect_this_assignments_block(f, assigned);
                }
            }
            Statement::Return(s) => {
                if let Some(v) = &s.value {
                    self.collect_this_assignments_expr(v, assigned);
                }
            }
            Statement::Throw(s) => self.collect_this_assignments_expr(&s.value, assigned),
            Statement::Block(b) => self.collect_this_assignments_block(b, assigned),
            _ => {}
        }
    }

    fn collect_this_assignments_expr(
        &self,
        expr: &crate::parser::ast::Expression,
        assigned: &mut FxHashSet<String>,
    ) {
        use crate::parser::ast::Expression;
        match expr {
            Expression::Assignment(a) => {
                if let Expression::Member(member) = &*a.left {
                    if matches!(&*member.object, Expression::This(_)) {
                        assigned.insert(self.resolve(member.property.name));
                    }
                }
                self.collect_this_assignments_expr(&a.left, assigned);
                self.collect_this_assignments_expr(&a.right, assigned);
            }
            Expression::Call(c) => {
                self.collect_this_assignments_expr(&c.callee, assigned);
                for a in &c.arguments {
                    self.collect_this_assignments_expr(a, assigned);
                }
            }
            Expression::AsyncCall(c) => {
                self.collect_this_assignments_expr(&c.callee, assigned);
                for a in &c.arguments {
                    self.collect_this_assignments_expr(a, assigned);
                }
            }
            Expression::Member(m) => self.collect_this_assignments_expr(&m.object, assigned),
            Expression::Index(i) => {
                self.collect_this_assignments_expr(&i.object, assigned);
                self.collect_this_assignments_expr(&i.index, assigned);
            }
            Expression::Unary(u) => self.collect_this_assignments_expr(&u.operand, assigned),
            Expression::Binary(b) => {
                self.collect_this_assignments_expr(&b.left, assigned);
                self.collect_this_assignments_expr(&b.right, assigned);
            }
            Expression::Logical(l) => {
                self.collect_this_assignments_expr(&l.left, assigned);
                self.collect_this_assignments_expr(&l.right, assigned);
            }
            Expression::Conditional(c) => {
                self.collect_this_assignments_expr(&c.test, assigned);
                self.collect_this_assignments_expr(&c.consequent, assigned);
                self.collect_this_assignments_expr(&c.alternate, assigned);
            }
            Expression::Parenthesized(p) => {
                self.collect_this_assignments_expr(&p.expression, assigned)
            }
            Expression::TypeCast(c) => self.collect_this_assignments_expr(&c.object, assigned),
            Expression::InstanceOf(i) => self.collect_this_assignments_expr(&i.object, assigned),
            Expression::Await(a) => self.collect_this_assignments_expr(&a.argument, assigned),
            Expression::Array(a) => {
                for e in &a.elements {
                    if let Some(elem) = e {
                        match elem {
                            crate::parser::ast::ArrayElement::Expression(expr)
                            | crate::parser::ast::ArrayElement::Spread(expr) => {
                                self.collect_this_assignments_expr(expr, assigned);
                            }
                        }
                    }
                }
            }
            Expression::Object(o) => {
                for p in &o.properties {
                    match p {
                        crate::parser::ast::ObjectProperty::Property(prop) => {
                            self.collect_this_assignments_expr(&prop.value, assigned);
                        }
                        crate::parser::ast::ObjectProperty::Spread(spread) => {
                            self.collect_this_assignments_expr(&spread.argument, assigned);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Check return statement
    fn check_return(&mut self, ret: &ReturnStatement) {
        if let Some(ref expr) = ret.value {
            let expr_ty = self.check_expr(expr);

            if let Some(expected_ty) = self.current_function_return_type {
                self.check_assignable(expr_ty, expected_ty, *expr.span());
            } else if let Some(collected) = self.return_type_collector.last_mut() {
                collected.push(expr_ty);
            }
        } else {
            // Return without value - check if function returns void
            if let Some(expected_ty) = self.current_function_return_type {
                let void_ty = self.type_ctx.void_type();
                if expected_ty != void_ty {
                    self.errors.push(CheckError::ReturnTypeMismatch {
                        expected: format!("{:?}", expected_ty),
                        actual: "void".to_string(),
                        span: ret.span,
                    });
                }
            } else if let Some(collected) = self.return_type_collector.last_mut() {
                collected.push(self.type_ctx.void_type());
            }
        }
    }

    /// Check yield statement
    fn check_yield(&mut self, yld: &YieldStatement) {
        if let Some(ref expr) = yld.value {
            self.check_expr(expr);
        }
    }

    /// Get the effective type of a variable, checking type_env, inferred_var_types, and symbol table
    fn get_var_type(&self, name: &str) -> Option<TypeId> {
        // First check for narrowed type in type environment
        if let Some(ty) = self.type_env.get(name) {
            return Some(ty);
        }

        // Look up in symbol table and check for inferred type
        if let Some(symbol) = self.symbols.resolve_from_scope(name, self.current_scope) {
            // Check if we have an inferred type for this variable
            let scope_id = symbol.scope_id.0;
            if let Some(&inferred_ty) = self.inferred_var_types.get(&(scope_id, name.to_string())) {
                return Some(inferred_ty);
            }
            return Some(symbol.ty);
        }

        None
    }

    /// Get the declared type of a variable, bypassing narrowing.
    /// Used for assignment targets where the original type (not narrowed) should be checked.
    fn get_var_declared_type(&self, name: &str) -> Option<TypeId> {
        if let Some(symbol) = self.symbols.resolve_from_scope(name, self.current_scope) {
            let scope_id = symbol.scope_id.0;
            if let Some(&inferred_ty) = self.inferred_var_types.get(&(scope_id, name.to_string())) {
                return Some(inferred_ty);
            }
            return Some(symbol.ty);
        }
        // Fallback: search inferred_var_types directly for variables declared inside
        // arrow bodies that the binder never visited (no symbol table entries)
        if self.arrow_depth > 0 {
            // Check inferred type at current scope (arrow bodies store at outer scope)
            if let Some(&ty) = self
                .inferred_var_types
                .get(&(self.current_scope.0, name.to_string()))
            {
                return Some(ty);
            }
            // Walk up the scope stack
            for scope_id in self.scope_stack.iter().rev() {
                if let Some(&ty) = self.inferred_var_types.get(&(scope_id.0, name.to_string())) {
                    return Some(ty);
                }
            }
        }
        None
    }

    fn type_supports_instanceof_guard_target(&self, ty: TypeId) -> bool {
        let Some(ty_def) = self.type_ctx.get(ty) else {
            return false;
        };

        match ty_def {
            crate::parser::types::Type::Class(_)
            | crate::parser::types::Type::Object(_)
            | crate::parser::types::Type::Interface(_) => true,
            crate::parser::types::Type::Union(union) => union
                .members
                .iter()
                .copied()
                .any(|member| self.type_supports_instanceof_guard_target(member)),
            crate::parser::types::Type::Reference(reference) => self
                .type_ctx
                .lookup_named_type(&reference.name)
                .is_some_and(|resolved| self.type_supports_instanceof_guard_target(resolved)),
            crate::parser::types::Type::Generic(generic) => {
                self.type_supports_instanceof_guard_target(generic.base)
            }
            crate::parser::types::Type::TypeVar(type_var) => type_var
                .constraint
                .or(type_var.default)
                .is_some_and(|inner| self.type_supports_instanceof_guard_target(inner)),
            _ => false,
        }
    }

    /// Try to extract an instanceof guard from a condition expression.
    /// Returns (variable_name, target_type_id) if the condition is `var instanceof T`
    /// and `T` is a supported nominal or structural runtime target.
    fn try_extract_instanceof_guard(&mut self, expr: &Expression) -> Option<(String, TypeId)> {
        let instanceof = match expr {
            Expression::InstanceOf(inst) => inst,
            _ => return None,
        };
        // Object must be an identifier
        let var_name = match &*instanceof.object {
            Expression::Identifier(ident) => self.resolve(ident.name),
            _ => return None,
        };

        if let crate::parser::ast::types::Type::Reference(type_ref) = &instanceof.type_name.ty {
            let target_name = self.resolve(type_ref.name.name);
            if let Some(target_ty) = self.type_ctx.lookup_named_type(&target_name) {
                if self.type_supports_instanceof_guard_target(target_ty) {
                    return Some((var_name.clone(), target_ty));
                }
            }

            if let Some(target_sym) = self
                .symbols
                .resolve_from_scope(&target_name, self.current_scope)
                .or_else(|| self.symbols.resolve(&target_name))
            {
                if self.type_supports_instanceof_guard_target(target_sym.ty) {
                    return Some((var_name.clone(), target_sym.ty));
                }
            }
        }

        let target_ty = self.resolve_type_annotation(&instanceof.type_name);
        if self.type_supports_instanceof_guard_target(target_ty) {
            Some((var_name, target_ty))
        } else {
            None
        }
    }

    /// Returns true if the statement definitely exits the current control-flow path
    /// (return/throw/break/continue).
    fn stmt_definitely_returns(stmt: &Statement) -> bool {
        match stmt {
            Statement::Return(_)
            | Statement::Throw(_)
            | Statement::Break(_)
            | Statement::Continue(_) => true,
            Statement::Block(block) => block
                .statements
                .last()
                .is_some_and(Self::stmt_definitely_returns),
            Statement::If(if_stmt) => {
                let then_returns = Self::stmt_definitely_returns(&if_stmt.then_branch);
                let else_returns = if_stmt
                    .else_branch
                    .as_ref()
                    .is_some_and(|e| Self::stmt_definitely_returns(e));
                then_returns && else_returns
            }
            _ => false,
        }
    }

    /// Check if statement
    fn check_if(&mut self, if_stmt: &IfStatement) {
        // Check condition — allow any type (truthiness), not just boolean
        let cond_ty = self.check_expr(&if_stmt.condition);
        let bool_ty = self.type_ctx.boolean_type();
        // Only enforce boolean for non-union, non-nullable types
        // TypeScript allows any expression in if-conditions (truthiness)
        let is_union_or_nullable = matches!(
            self.type_ctx.get(cond_ty),
            Some(crate::parser::types::Type::Union(_))
                | Some(crate::parser::types::Type::Primitive(
                    crate::parser::types::PrimitiveType::String
                ))
                | Some(crate::parser::types::Type::Primitive(
                    crate::parser::types::PrimitiveType::Number
                ))
        );
        if !is_union_or_nullable {
            self.check_assignable(cond_ty, bool_ty, *if_stmt.condition.span());
        }

        // Try to extract single type guard (used for else-branch / early-return narrowing)
        let type_guard = extract_type_guard(&if_stmt.condition, self.interner);

        // Extract all type guards including from && compound conditions (used for then-branch)
        let all_guards = extract_all_type_guards(&if_stmt.condition, self.interner);

        // Try to extract instanceof guard (needs symbol table, so done in checker)
        let instanceof_guard = self.try_extract_instanceof_guard(&if_stmt.condition);

        // Save current environment
        let saved_env = self.type_env.clone();

        // Apply all type guards for then branch (handles && compound conditions)
        if !all_guards.is_empty() {
            for guard in &all_guards {
                let var_name = get_guard_var(guard);
                if let Some(var_ty) = self.get_var_type(var_name) {
                    if let Some(narrowed_ty) = apply_type_guard(self.type_ctx, var_ty, guard) {
                        self.type_env.set(var_name.clone(), narrowed_ty);
                    }
                }
            }
        } else if let Some((ref var_name, class_ty)) = instanceof_guard {
            // instanceof narrows variable to the target class type
            self.type_env.set(var_name.clone(), class_ty);
        }

        // Check then branch
        self.check_stmt(&if_stmt.then_branch);
        let then_env = self.type_env.clone();

        // Check if the then-branch definitely exits (return/throw).
        // If so, code after the if can only be reached when condition was false.
        let then_returns = Self::stmt_definitely_returns(&if_stmt.then_branch);

        // Restore environment and apply negated guard for else branch
        self.type_env = saved_env.clone();

        let apply_negated_guard = |checker: &mut TypeChecker<'_>, guard: &TypeGuard| {
            let negated_guard = negate_guard(guard);
            let var_name = get_guard_var(&negated_guard);
            if let Some(var_ty) = checker.get_var_type(var_name) {
                if let Some(narrowed_ty) =
                    apply_type_guard(checker.type_ctx, var_ty, &negated_guard)
                {
                    checker.type_env.set(var_name.clone(), narrowed_ty);
                }
            }
        };

        if let Some(ref else_branch) = if_stmt.else_branch {
            if let Some(ref guard) = type_guard {
                apply_negated_guard(self, guard);
            }
            self.check_stmt(else_branch);
        } else if let Some(ref guard) = type_guard {
            // No else branch: the false path still reaches continuation,
            // so include negated-guard narrowing in the merge environment.
            apply_negated_guard(self, guard);
        }

        let else_env = self.type_env.clone();

        if then_returns && if_stmt.else_branch.is_none() {
            // Then-branch always exits, no else: continuation uses the narrowed env
            self.type_env = else_env;
        } else {
            // Normal merge of both branches
            self.type_env = then_env.merge(&else_env, self.type_ctx);
        }
    }

    /// Check while loop
    fn check_while(&mut self, while_stmt: &WhileStatement) {
        // Check condition is boolean
        let cond_ty = self.check_expr(&while_stmt.condition);
        let bool_ty = self.type_ctx.boolean_type();
        self.check_assignable(cond_ty, bool_ty, *while_stmt.condition.span());

        // Try to extract type guard from condition
        let type_guard = extract_type_guard(&while_stmt.condition, self.interner);

        // Save current environment
        let saved_env = self.type_env.clone();

        // Apply type guard for loop body
        if let Some(ref guard) = type_guard {
            let var_name = get_guard_var(guard);
            // Get the actual type of the variable (including inferred types)
            if let Some(var_ty) = self.get_var_type(var_name) {
                if let Some(narrowed_ty) = apply_type_guard(self.type_ctx, var_ty, guard) {
                    self.type_env.set(var_name.clone(), narrowed_ty);
                }
            }
        }

        // Binder creates a Loop scope, but the body statement (often a Block)
        // will create its own scope. We need to mirror binder's behavior.
        self.enter_scope(); // Loop scope

        // Check body - if it's a Block, it will enter another scope
        self.check_stmt(&while_stmt.body);

        self.exit_scope(); // Exit loop scope

        // Restore environment after loop
        self.type_env = saved_env;
    }

    /// Check for loop
    fn check_for(&mut self, for_stmt: &ForStatement) {
        // Enter loop scope (mirrors binder)
        self.enter_scope();

        // Check initializer if present
        if let Some(ref init) = for_stmt.init {
            match init {
                ForInit::VariableDecl(decl) => self.check_var_decl(decl),
                ForInit::Expression(expr) => {
                    self.check_expr(expr);
                }
            }
        }

        // Check test condition if present
        if let Some(ref test) = for_stmt.test {
            let cond_ty = self.check_expr(test);
            let bool_ty = self.type_ctx.boolean_type();
            self.check_assignable(cond_ty, bool_ty, *test.span());
        }

        // Check update expression if present
        if let Some(ref update) = for_stmt.update {
            self.check_expr(update);
        }

        // Check body
        self.check_stmt(&for_stmt.body);

        // Exit loop scope
        self.exit_scope();
    }

    /// Check for-of loop
    fn check_for_of(&mut self, for_of: &ForOfStatement) {
        // Enter loop scope
        self.enter_scope();

        // Check the iterable (right side) and get its type
        let iterable_ty = self.check_expr(&for_of.right);

        let elem_ty = self.for_of_element_type(iterable_ty, *for_of.right.span());

        // Handle the loop variable (left side)
        // The binder should have already registered the variable
        // We just need to ensure its type matches the element type
        match &for_of.left {
            ForOfLeft::VariableDecl(decl) => {
                // Variable declared in the for-of: `for (let x of arr)`
                // The type should be the element type of the array
                match &decl.pattern {
                    Pattern::Identifier(ident) => {
                        let name = self.resolve(ident.name);
                        // Store inferred type for the loop variable
                        self.inferred_var_types
                            .insert((self.current_scope.0, name), elem_ty);
                    }
                    Pattern::Array(_) | Pattern::Object(_) => {
                        self.check_destructure_pattern(&decl.pattern, elem_ty);
                    }
                    _ => {}
                }
            }
            ForOfLeft::Pattern(_) => {
                // Existing variable: `for (x of arr)` - type already bound
            }
        }

        // Check body
        self.check_stmt(&for_of.body);

        // Exit loop scope
        self.exit_scope();
    }

    /// Check for-in loop
    fn check_for_in(&mut self, for_in: &ForInStatement) {
        self.enter_scope();

        // Check the object expression (right side)
        self.check_expr(&for_in.right);

        // The loop variable is always a string (property key)
        let string_ty = self.type_ctx.string_type();

        match &for_in.left {
            ForOfLeft::VariableDecl(decl) => {
                if let Pattern::Identifier(ident) = &decl.pattern {
                    let name = self.resolve(ident.name);
                    self.inferred_var_types
                        .insert((self.current_scope.0, name), string_ty);
                }
            }
            ForOfLeft::Pattern(_) => {}
        }

        self.check_stmt(&for_in.body);
        self.exit_scope();
    }

    fn for_of_element_type(&mut self, iterable_ty: TypeId, span: Span) -> TypeId {
        if self.allows_dynamic_any() && self.type_is_dynamic_anyish(iterable_ty) {
            return self.type_ctx.any_type();
        }

        if let Some(elem_ty) = self.try_for_of_element_type(iterable_ty) {
            return elem_ty;
        }

        self.errors.push(CheckError::TypeMismatch {
            expected: "iterable (Array, Set, Map, or class with iterator())".to_string(),
            actual: self.format_type(iterable_ty),
            span,
            note: Some(
                "for-of loops require an iterable with iterator() returning an array".to_string(),
            ),
        });
        self.type_ctx.unknown_type()
    }

    fn try_for_of_element_type(&mut self, iterable_ty: TypeId) -> Option<TypeId> {
        use crate::parser::types::Type;

        let Some(ty) = self.type_ctx.get(iterable_ty).cloned() else {
            return None;
        };

        match ty {
            Type::Array(arr) => Some(arr.element),
            Type::Tuple(tuple_ty) => {
                if tuple_ty.elements.is_empty() {
                    Some(self.inference_fallback_type())
                } else {
                    Some(self.type_ctx.union_type(tuple_ty.elements))
                }
            }
            Type::Primitive(crate::parser::types::PrimitiveType::String) => {
                Some(self.type_ctx.string_type())
            }
            Type::StringLiteral(_) => Some(self.type_ctx.string_type()),
            Type::Set(set_ty) => Some(set_ty.element),
            Type::Map(map_ty) => Some(self.type_ctx.tuple_type(vec![map_ty.key, map_ty.value])),
            Type::Reference(reference) => match reference.name.as_str() {
                "Array" | "Set" => Some(
                    reference
                        .type_args
                        .and_then(|args| args.first().copied())
                        .unwrap_or_else(|| self.type_ctx.unknown_type()),
                ),
                "Map" => {
                    if let Some(args) = reference.type_args {
                        if args.len() >= 2 {
                            return Some(self.type_ctx.tuple_type(vec![args[0], args[1]]));
                        }
                    }
                    Some(self.type_ctx.unknown_type())
                }
                _ => self
                    .for_of_element_from_reference(&reference.name, reference.type_args.as_deref()),
            },
            Type::Generic(generic) => {
                let base_name = self.type_ctx.get(generic.base).and_then(|base| match base {
                    Type::Reference(reference) => Some(reference.name.clone()),
                    Type::Class(class_ty) => Some(class_ty.name.clone()),
                    _ => None,
                });
                match base_name.as_deref() {
                    Some("Array") | Some("Set") => Some(
                        generic
                            .type_args
                            .first()
                            .copied()
                            .unwrap_or_else(|| self.type_ctx.unknown_type()),
                    ),
                    Some("Map") => {
                        if generic.type_args.len() >= 2 {
                            Some(
                                self.type_ctx
                                    .tuple_type(vec![generic.type_args[0], generic.type_args[1]]),
                            )
                        } else {
                            Some(self.type_ctx.unknown_type())
                        }
                    }
                    Some(name) => {
                        self.for_of_element_from_reference(name, Some(&generic.type_args))
                    }
                    _ => None,
                }
            }
            Type::Class(class_ty) => self.for_of_element_from_class(&class_ty, None),
            Type::Union(union) => {
                let mut elem_types = Vec::new();
                for member in union.members {
                    let Some(elem_ty) = self.try_for_of_element_type(member) else {
                        return None;
                    };
                    elem_types.push(elem_ty);
                }
                if elem_types.is_empty() {
                    None
                } else if elem_types.len() == 1 {
                    Some(elem_types[0])
                } else {
                    Some(self.type_ctx.union_type(elem_types))
                }
            }
            _ => None,
        }
    }

    fn for_of_element_from_reference(
        &mut self,
        name: &str,
        type_args: Option<&[TypeId]>,
    ) -> Option<TypeId> {
        use crate::parser::types::Type;

        let named_ty = self.type_ctx.lookup_named_type(name)?;
        let class_ty = match self.type_ctx.get(named_ty).cloned()? {
            Type::Class(class_ty) => class_ty,
            _ => return None,
        };
        self.for_of_element_from_class(&class_ty, type_args)
    }

    fn for_of_element_from_class(
        &mut self,
        class_ty: &crate::parser::types::ty::ClassType,
        type_args: Option<&[TypeId]>,
    ) -> Option<TypeId> {
        use crate::parser::types::Type;

        let class_handle = if let Some(args) = type_args {
            self.instantiate_class_type(class_ty, args)
        } else {
            self.type_ctx.intern(Type::Class(class_ty.clone()))
        };

        let instantiated = match self.type_ctx.get(class_handle).cloned()? {
            Type::Class(class_ty) => class_ty,
            _ => return None,
        };
        let (method_ty, _) = self.lookup_class_member(&instantiated, "iterator")?;
        let method_fn = match self.type_ctx.get(method_ty).cloned()? {
            Type::Function(func) => func,
            _ => return None,
        };
        self.for_of_element_from_iterator_return(method_fn.return_type)
    }

    fn for_of_element_from_iterator_return(&mut self, return_ty: TypeId) -> Option<TypeId> {
        use crate::parser::types::Type;

        let ty = self.type_ctx.get(return_ty).cloned()?;
        match ty {
            Type::Array(arr) => Some(arr.element),
            Type::Reference(reference) if reference.name == "Array" => {
                reference.type_args.and_then(|args| args.first().copied())
            }
            Type::Generic(generic) => {
                let base_name = self.type_ctx.get(generic.base).and_then(|base| match base {
                    Type::Reference(reference) => Some(reference.name.as_str()),
                    Type::Class(class_ty) => Some(class_ty.name.as_str()),
                    _ => None,
                });
                if base_name == Some("Array") {
                    generic.type_args.first().copied()
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Check switch statement
    fn check_switch(&mut self, switch_stmt: &SwitchStatement) {
        // Check discriminant and get its type
        let discriminant_ty = self.check_expr(&switch_stmt.discriminant);

        // Check exhaustiveness for discriminated unions
        let exhaustiveness =
            check_switch_exhaustiveness(self.type_ctx, discriminant_ty, switch_stmt, self.interner);

        // Report non-exhaustive matches
        if let ExhaustivenessResult::NonExhaustive(missing) = exhaustiveness {
            self.errors.push(CheckError::NonExhaustiveMatch {
                missing,
                span: switch_stmt.span,
            });
        }

        enum SwitchNarrowingBase {
            TypeofVar(String),
            DiscriminantVar { var: String, field: String },
        }

        let narrowing_base = match &switch_stmt.discriminant {
            Expression::Typeof(typeof_expr) => match &*typeof_expr.argument {
                Expression::Identifier(ident) => {
                    Some(SwitchNarrowingBase::TypeofVar(self.resolve(ident.name)))
                }
                _ => None,
            },
            Expression::Member(member) => match &*member.object {
                Expression::Identifier(ident) => Some(SwitchNarrowingBase::DiscriminantVar {
                    var: self.resolve(ident.name),
                    field: self.resolve(member.property.name),
                }),
                _ => None,
            },
            _ => None,
        };

        // Check cases
        for case in &switch_stmt.cases {
            let saved_env = self.type_env.clone();

            if let Some(ref test) = case.test {
                self.check_expr(test);

                if let (Some(base), Expression::StringLiteral(lit)) =
                    (narrowing_base.as_ref(), test)
                {
                    let guard = match base {
                        SwitchNarrowingBase::TypeofVar(var) => TypeGuard::TypeOf {
                            var: var.clone(),
                            type_name: self.resolve(lit.value),
                            negated: false,
                        },
                        SwitchNarrowingBase::DiscriminantVar { var, field } => {
                            TypeGuard::Discriminant {
                                var: var.clone(),
                                field: field.clone(),
                                variant: self.resolve(lit.value),
                                negated: false,
                            }
                        }
                    };

                    let var_name = get_guard_var(&guard).clone();
                    if let Some(var_ty) = self.get_var_type(&var_name) {
                        if let Some(narrowed_ty) = apply_type_guard(self.type_ctx, var_ty, &guard) {
                            self.type_env.set(var_name, narrowed_ty);
                        }
                    }
                }
            }

            for stmt in &case.consequent {
                self.check_stmt(stmt);
            }

            // Cases are checked independently; don't leak branch narrowing into sibling cases.
            self.type_env = saved_env;
        }
    }

    /// Check try-catch statement
    fn check_try(&mut self, try_stmt: &TryStatement) {
        // Check try block
        for stmt in &try_stmt.body.statements {
            self.check_stmt(stmt);
        }

        // Check catch block
        if let Some(ref catch) = try_stmt.catch_clause {
            // Enter catch scope (mirrors binder's push_scope for catch block)
            self.enter_scope();

            // Check catch parameter pattern (handles destructuring)
            if let Some(ref param) = catch.param {
                let catch_ty = match param {
                    Pattern::Identifier(_) => {
                        if self.policy.use_unknown_in_catch_variables {
                            self.type_ctx.unknown_type()
                        } else {
                            self.type_ctx.jsobject_type()
                        }
                    }
                    // Destructuring catch params need to accept arbitrary thrown values.
                    _ => self.inference_fallback_type(),
                };

                // Register types for all variables bound in the pattern
                self.check_destructure_pattern(param, catch_ty);
            }

            // Check catch body statements
            for stmt in &catch.body.statements {
                self.check_stmt(stmt);
            }

            // Exit catch scope
            self.exit_scope();
        }

        // Check finally block
        if let Some(ref finally) = try_stmt.finally_clause {
            for stmt in &finally.statements {
                self.check_stmt(stmt);
            }
        }
    }

    /// Check expression (returns inferred type)
    fn check_expr(&mut self, expr: &Expression) -> TypeId {
        let ty = match expr {
            Expression::IntLiteral(_) | Expression::FloatLiteral(_) => self.type_ctx.number_type(),
            Expression::StringLiteral(_) => self.type_ctx.string_type(),
            Expression::TemplateLiteral(tpl) => {
                // Preserve type information/diagnostics for interpolated expressions.
                for part in &tpl.parts {
                    if let TemplatePart::Expression(expr) = part {
                        let _ = self.check_expr(expr);
                    }
                }
                self.type_ctx.string_type()
            }
            Expression::BooleanLiteral(_) => self.type_ctx.boolean_type(),
            Expression::NullLiteral(_) => self.type_ctx.null_type(),
            Expression::Identifier(ident) => self.check_identifier(ident),
            Expression::Binary(bin) => self.check_binary(bin),
            Expression::Logical(log) => self.check_logical(log),
            Expression::Unary(un) => self.check_unary(un),
            Expression::Call(call) => self.check_call(call),
            Expression::Member(member) => self.check_member(member),
            Expression::Array(arr) => self.check_array(arr),
            Expression::Object(obj) => self.check_object(obj),
            Expression::Conditional(cond) => self.check_conditional(cond),
            Expression::Assignment(assign) => self.check_assignment(assign),
            Expression::Typeof(_) => {
                // typeof always returns a string
                self.type_ctx.string_type()
            }
            Expression::Parenthesized(paren) => self.check_expr(&paren.expression),
            Expression::Arrow(arrow) => self.check_arrow(arrow),
            Expression::Function(func) => self.check_function_expression(func),
            Expression::Index(index) => self.check_index(index),
            Expression::New(new_expr) => self.check_new(new_expr),
            Expression::This(span) => self.check_this(*span),
            Expression::Await(await_expr) => self.check_await(await_expr),
            Expression::AsyncCall(async_call) => self.check_async_call(async_call),
            Expression::InstanceOf(instanceof) => self.check_instanceof(instanceof),
            Expression::TypeCast(cast) => self.check_type_cast(cast),
            Expression::RegexLiteral(_) => self.type_ctx.regexp_type(),
            Expression::TaggedTemplate(tagged) => self.check_tagged_template(tagged),
            Expression::DynamicImport(dynamic_import) => self.check_dynamic_import(dynamic_import),
            Expression::JsxElement(jsx) => self.fallback_type(
                jsx.span,
                FallbackReason::RecoverableUnsupportedExpr,
                "jsx-element",
            ),
            Expression::JsxFragment(jsx) => self.fallback_type(
                jsx.span,
                FallbackReason::RecoverableUnsupportedExpr,
                "jsx-fragment",
            ),
            Expression::Super(span) => {
                self.fallback_type(*span, FallbackReason::Unavoidable, "super-expression")
            }
        };

        // Store type for this expression (using pointer address as ID)
        let expr_id = expr as *const _ as usize;
        self.expr_types.insert(expr_id, ty);

        ty
    }

    fn check_function_expression(
        &mut self,
        func: &crate::parser::ast::FunctionExpression,
    ) -> TypeId {
        let arrow = crate::parser::ast::ArrowFunction {
            params: func.params.clone(),
            return_type: func.return_type.clone(),
            body: crate::parser::ast::ArrowBody::Block(func.body.clone()),
            is_async: func.is_async,
            span: func.span,
        };
        self.check_arrow(&arrow)
    }

    fn check_tagged_template(
        &mut self,
        tagged: &crate::parser::ast::TaggedTemplateExpression,
    ) -> TypeId {
        let tag_ty = self.check_expr(&tagged.tag);
        self.check_unknown_actionable(tag_ty, "tagged-template-call", *tagged.tag.span());
        let mut arg_types: Vec<(TypeId, Span)> = Vec::new();

        // First argument in JS/TS tagged templates is an array of cooked strings.
        let string_ty = self.type_ctx.string_type();
        let strings_arg_ty = self.type_ctx.array_type(string_ty);
        arg_types.push((strings_arg_ty, tagged.template.span));

        for part in &tagged.template.parts {
            if let crate::parser::ast::TemplatePart::Expression(expr) = part {
                arg_types.push((self.check_expr(expr), *expr.span()));
            }
        }

        if let Some(crate::parser::types::Type::Function(func)) = self.type_ctx.get(tag_ty).cloned()
        {
            for (idx, (arg_ty, arg_span)) in arg_types.iter().enumerate() {
                if let Some(&param_ty) = func.params.get(idx) {
                    self.check_assignable(*arg_ty, param_ty, *arg_span);
                }
            }
            return func.return_type;
        }

        if self.allows_dynamic_any() && self.type_is_dynamic_anyish(tag_ty) {
            return self.type_ctx.any_type();
        }

        if !self.type_is_unknown(tag_ty) {
            self.errors.push(CheckError::NotCallable {
                ty: self.format_type(tag_ty),
                span: tagged.span,
            });
        }

        // Mirror call-expression behavior: keep flow moving with unknown type
        // without forcing an additional fallback-path diagnostic.
        self.type_ctx.unknown_type()
    }

    fn check_dynamic_import(
        &mut self,
        dynamic_import: &crate::parser::ast::DynamicImportExpression,
    ) -> TypeId {
        let source_ty = self.check_expr(&dynamic_import.source);
        let string_ty = self.type_ctx.string_type();
        self.check_assignable(source_ty, string_ty, *dynamic_import.source.span());
        let unknown = self.fallback_type(
            dynamic_import.span,
            FallbackReason::Unavoidable,
            "dynamic-import-value",
        );
        self.type_ctx.task_type(unknown)
    }

    /// Check identifier
    fn check_identifier(&mut self, ident: &Identifier) -> TypeId {
        let name = self.resolve(ident.name);

        // Check for builtin functions
        if name == "sleep" {
            // sleep(ms: number): void
            let number_ty = self.type_ctx.number_type();
            let void_ty = self.type_ctx.void_type();
            return self.type_ctx.function_type(vec![number_ty], void_ty, false);
        }

        // Check for __OPCODE_* intrinsics
        if name.starts_with("__OPCODE_") || name == "__NATIVE_CALL" {
            // Return a callable type - the actual return type is handled in try_check_intrinsic
            let any_ty = self.type_ctx.unknown_type();
            return self.type_ctx.function_type(vec![], any_ty, false);
        }

        if self.is_js_mode() && name == "arguments" {
            let any_ty = self.type_ctx.any_type();
            return self.type_ctx.array_type(any_ty);
        }

        // First check for narrowed type in type environment
        if let Some(narrowed_ty) = self.type_env.get(&name) {
            return narrowed_ty;
        }

        // Look up in symbol table from current scope, walking up the scope chain
        match self.symbols.resolve_from_scope(&name, self.current_scope) {
            Some(symbol) => {
                // Check if we have an inferred type for this variable
                // (for variables declared without type annotations)
                let scope_id = symbol.scope_id.0;
                if let Some(&inferred_ty) = self.inferred_var_types.get(&(scope_id, name.clone())) {
                    return inferred_ty;
                }
                symbol.ty
            }
            None => {
                // Inside arrow bodies, variables may be declared in this body
                // but not in the binder's symbol table. Check inferred_var_types.
                if self.arrow_depth > 0 {
                    if let Some(&ty) = self
                        .inferred_var_types
                        .get(&(self.current_scope.0, name.clone()))
                    {
                        return ty;
                    }
                    for scope_id in self.scope_stack.iter().rev() {
                        if let Some(&ty) = self.inferred_var_types.get(&(scope_id.0, name.clone()))
                        {
                            return ty;
                        }
                    }
                }
                self.errors.push(CheckError::UndefinedVariable {
                    name,
                    span: ident.span,
                });
                self.type_ctx.unknown_type()
            }
        }
    }

    /// Check binary expression
    fn check_binary(&mut self, bin: &BinaryExpression) -> TypeId {
        let left_ty = self.check_expr(&bin.left);
        let right_ty = self.check_expr(&bin.right);
        self.check_unknown_actionable(left_ty, "binary", *bin.left.span());
        self.check_unknown_actionable(right_ty, "binary", *bin.right.span());

        match bin.operator {
            BinaryOperator::Add => {
                // Add can be either numeric or string concatenation
                let string_ty = self.type_ctx.string_type();
                let number_ty = self.type_ctx.number_type();
                let int_ty = self.type_ctx.int_type();

                // Check if either operand IS a string type (exact match or assignable)
                let left_is_string = left_ty == string_ty;
                let right_is_string = right_ty == string_ty;

                if left_is_string || right_is_string {
                    // JS-style string concatenation supports primitive operands.
                    if !self.is_string_concat_operand_type(left_ty) {
                        self.check_assignable(left_ty, string_ty, *bin.left.span());
                    }
                    if !self.is_string_concat_operand_type(right_ty) {
                        self.check_assignable(right_ty, string_ty, *bin.right.span());
                    }
                    string_ty
                } else if left_ty == int_ty && right_ty == int_ty {
                    // int + int = int
                    int_ty
                } else {
                    // Numeric addition (mixed int/number promotes to number)
                    self.check_assignable(left_ty, number_ty, *bin.left.span());
                    self.check_assignable(right_ty, number_ty, *bin.right.span());
                    number_ty
                }
            }

            BinaryOperator::Subtract
            | BinaryOperator::Multiply
            | BinaryOperator::Divide
            | BinaryOperator::Modulo
            | BinaryOperator::Exponent => {
                // Arithmetic operations require numeric operands
                let number_ty = self.type_ctx.number_type();
                let int_ty = self.type_ctx.int_type();
                if left_ty == int_ty && right_ty == int_ty {
                    // int op int = int
                    int_ty
                } else {
                    self.check_assignable(left_ty, number_ty, *bin.left.span());
                    self.check_assignable(right_ty, number_ty, *bin.right.span());
                    number_ty
                }
            }

            BinaryOperator::Equal
            | BinaryOperator::NotEqual
            | BinaryOperator::StrictEqual
            | BinaryOperator::StrictNotEqual
            | BinaryOperator::LessThan
            | BinaryOperator::LessEqual
            | BinaryOperator::GreaterThan
            | BinaryOperator::GreaterEqual => {
                // Comparison operations return boolean
                self.type_ctx.boolean_type()
            }

            BinaryOperator::BitwiseAnd
            | BinaryOperator::BitwiseOr
            | BinaryOperator::BitwiseXor
            | BinaryOperator::LeftShift
            | BinaryOperator::RightShift
            | BinaryOperator::UnsignedRightShift => {
                // Bitwise operations require numeric operands and produce int
                let number_ty = self.type_ctx.number_type();
                self.check_assignable(left_ty, number_ty, *bin.left.span());
                self.check_assignable(right_ty, number_ty, *bin.right.span());
                self.type_ctx.int_type()
            }
        }
    }

    /// Check logical expression
    fn check_logical(&mut self, log: &LogicalExpression) -> TypeId {
        match log.operator {
            LogicalOperator::NullishCoalescing => {
                let left_ty = self.check_expr(&log.left);
                let right_ty = self.check_expr(&log.right);
                // For x ?? y:
                // - x can be any type (typically T | null)
                // - y is the fallback value
                // - Result is the non-null part of x's type, or union with y

                // Get the non-null type from left operand
                let non_null_ty = self.get_non_null_type(left_ty);

                // Right side should be assignable to the non-null type
                // (or we take a union of both)
                if non_null_ty != right_ty {
                    // Check if right is assignable to non-null left type
                    let mut assign_ctx = self.make_assignability_ctx();
                    if !assign_ctx.is_assignable(right_ty, non_null_ty) {
                        // If not directly assignable, result is union of both
                        return self.type_ctx.union_type(vec![non_null_ty, right_ty]);
                    }
                }

                non_null_ty
            }
            LogicalOperator::And | LogicalOperator::Or => {
                let left_ty = self.check_expr(&log.left);
                let right_ty = if matches!(log.operator, LogicalOperator::And) {
                    let saved_env = self.type_env.clone();
                    for guard in extract_all_type_guards(&log.left, self.interner).iter() {
                        let var = get_guard_var(guard);
                        if let Some(var_ty) = self.type_env.get(var) {
                            if let Some(narrowed_ty) =
                                apply_type_guard(self.type_ctx, var_ty, guard)
                            {
                                self.type_env.set(var.clone(), narrowed_ty);
                            }
                        }
                    }
                    let ty = self.check_expr(&log.right);
                    self.type_env = saved_env;
                    ty
                } else {
                    self.check_expr(&log.right)
                };
                if self.is_js_mode() {
                    self.type_ctx.union_type(vec![left_ty, right_ty])
                } else {
                    // Logical AND/OR require boolean operands
                    let bool_ty = self.type_ctx.boolean_type();
                    self.check_assignable(left_ty, bool_ty, *log.left.span());
                    self.check_assignable(right_ty, bool_ty, *log.right.span());
                    bool_ty
                }
            }
        }
    }

    /// Get the non-null type from a type (removes null from unions)
    fn get_non_null_type(&mut self, ty: TypeId) -> TypeId {
        let null_ty = self.type_ctx.null_type();

        // If it's exactly null, return never (no non-null part)
        if ty == null_ty {
            return self.type_ctx.never_type();
        }

        // Check if it's a union type
        if let Some(union) = self.type_ctx.get(ty).and_then(|t| t.as_union()) {
            // Filter out null from the union members
            let non_null_members: Vec<TypeId> = union
                .members
                .iter()
                .filter(|&&m| m != null_ty)
                .copied()
                .collect();

            match non_null_members.len() {
                0 => self.type_ctx.never_type(),
                1 => non_null_members[0],
                _ => self.type_ctx.union_type(non_null_members),
            }
        } else {
            // Not a union, return as-is (already non-null)
            ty
        }
    }

    /// Check unary expression
    fn check_unary(&mut self, un: &UnaryExpression) -> TypeId {
        let operand_ty = self.check_expr(&un.operand);
        self.check_unknown_actionable(operand_ty, "unary", *un.operand.span());

        match un.operator {
            UnaryOperator::Not => {
                // Logical not requires boolean
                let bool_ty = self.type_ctx.boolean_type();
                self.check_assignable(operand_ty, bool_ty, *un.operand.span());
                bool_ty
            }
            UnaryOperator::Plus | UnaryOperator::Minus | UnaryOperator::BitwiseNot => {
                // Numeric operations require number
                let number_ty = self.type_ctx.number_type();
                self.check_assignable(operand_ty, number_ty, *un.operand.span());
                number_ty
            }
            UnaryOperator::PrefixIncrement
            | UnaryOperator::PrefixDecrement
            | UnaryOperator::PostfixIncrement
            | UnaryOperator::PostfixDecrement => {
                // Increment/decrement require number
                let number_ty = self.type_ctx.number_type();
                self.check_assignable(operand_ty, number_ty, *un.operand.span());
                number_ty
            }
            UnaryOperator::Void => {
                // void evaluates operand for side-effects and returns null
                self.type_ctx.null_type()
            }
            UnaryOperator::Delete => {
                // delete returns boolean (true if property was deleted)
                self.type_ctx.boolean_type()
            }
        }
    }

    /// Check function call
    fn check_call(&mut self, call: &CallExpression) -> TypeId {
        // super(...) constructor call
        if let Expression::Super(_) = call.callee.as_ref() {
            for arg in &call.arguments {
                self.check_expr(arg);
            }

            if self.in_constructor {
                if let Some(class_ty) = self.current_class_type {
                    if let Some(crate::parser::types::Type::Class(class)) =
                        self.type_ctx.get(class_ty)
                    {
                        if class.extends.is_some() {
                            return self.type_ctx.void_type();
                        }
                    }
                }
            }

            self.errors.push(CheckError::NotCallable {
                ty: "super".to_string(),
                span: call.span,
            });
            return self.type_ctx.unknown_type();
        }

        // Check for compiler intrinsics first.
        if let Expression::Identifier(ident) = call.callee.as_ref() {
            let name = self.resolve(ident.name);
            if let Some(opcode_name) = name.strip_prefix("__OPCODE_") {
                let arg_types: Vec<(TypeId, crate::parser::Span)> = call
                    .arguments
                    .iter()
                    .map(|arg| (self.check_expr(arg), *arg.span()))
                    .collect();
                return self.get_opcode_intrinsic_type(opcode_name, call, &arg_types);
            }
            if let Some(intrinsic_ty) = self.try_check_intrinsic(call) {
                // Still type-check the arguments for error detection
                for arg in &call.arguments {
                    self.check_expr(arg);
                }
                return intrinsic_ty;
            }
            if self.is_js_mode() && Self::js_builtin_call_uses_construct(&name) {
                let synthetic_new = crate::parser::ast::NewExpression {
                    callee: call.callee.clone(),
                    type_args: call.type_args.clone(),
                    arguments: call.arguments.clone(),
                    span: call.span,
                };
                return self.check_new(&synthetic_new);
            }
        }

        if let Expression::Identifier(ident) = call.callee.as_ref() {
            let name = self.resolve(ident.name);
            if !self.is_js_mode() && self.is_unbound_method_var(&name) {
                self.errors.push(CheckError::UnboundMethodCall {
                    name,
                    span: call.span,
                });
            }
        }

        let helper_name = if let Expression::Member(member) = call.callee.as_ref() {
            Some(self.resolve(member.property.name))
        } else {
            None
        };

        // Function helper calls: fn.call(...), fn.apply(...), fn.bind(...)
        if let Some(helper_ty) = self.try_check_function_helper_call(call) {
            return helper_ty;
        }
        if self.is_js_mode() && matches!(helper_name.as_deref(), Some("call") | Some("apply")) {
            // If helper-specific typing can't prove the target is a real
            // Function.prototype.call/apply usage, keep the result dynamic.
            // Treating these as ordinary method calls leaks direct-call return
            // types across rebound receivers and miscompiles later property access.
            for arg in &call.arguments {
                self.check_expr(arg);
            }
            return self.inference_fallback_type();
        }

        if let Expression::Index(idx) = call.callee.as_ref() {
            self.maybe_escalate_identifier_to_jsobject(&idx.object, Some(&idx.index));
        }

        let raw_callee_ty = self.check_expr(&call.callee);
        self.check_unknown_actionable(raw_callee_ty, "call", *call.callee.span());
        let callee_ty = if call.optional {
            self.get_non_null_type(raw_callee_ty)
        } else {
            raw_callee_ty
        };

        // Check all argument types first (before creating GenericContext)
        let arg_types: Vec<(TypeId, crate::parser::Span)> = call
            .arguments
            .iter()
            .map(|arg| (self.check_expr(arg), *arg.span()))
            .collect();

        // Clone the function type to avoid borrow checker issues. Structural
        // callable object/interface signatures are projected to function types.
        let mut func_ty_opt = self.type_ctx.get(callee_ty).cloned();
        if let Some(crate::parser::types::Type::Object(obj)) = func_ty_opt.as_ref() {
            if let Some(sig_ty) = obj.call_signatures.first() {
                func_ty_opt = self.type_ctx.get(*sig_ty).cloned();
            }
        } else if let Some(crate::parser::types::Type::Interface(iface)) = func_ty_opt.as_ref() {
            if let Some(sig_ty) = iface.call_signatures.first() {
                func_ty_opt = self.type_ctx.get(*sig_ty).cloned();
            }
        }

        // Check if callee is a function type
        match func_ty_opt {
            Some(crate::parser::types::Type::Function(func)) => {
                let fixed_param_len = func.params.len();
                let mut rest_tuple_elems: Option<Vec<TypeId>> = None;
                let mut rest_array_elem_ty: Option<TypeId> = None;
                if let Some(rest_ty) = func.rest_param {
                    match self.type_ctx.get(rest_ty) {
                        Some(crate::parser::types::Type::Tuple(t)) => {
                            rest_tuple_elems = Some(t.elements.clone());
                        }
                        Some(crate::parser::types::Type::Array(arr_ty)) => {
                            rest_array_elem_ty = Some(arr_ty.element);
                        }
                        _ => {}
                    }
                }

                // Check argument count (too many or too few required)
                let max_params = if let Some(ref tuple_elems) = rest_tuple_elems {
                    fixed_param_len + tuple_elems.len()
                } else if func.rest_param.is_some() {
                    usize::MAX
                } else {
                    fixed_param_len
                };
                let min_params = if let Some(ref tuple_elems) = rest_tuple_elems {
                    func.min_params + tuple_elems.len()
                } else {
                    func.min_params
                };

                if self.enforce_call_arity()
                    && (arg_types.len() > max_params || arg_types.len() < min_params)
                {
                    self.errors.push(CheckError::ArgumentCountMismatch {
                        expected: func.params.len(),
                        min_expected: min_params,
                        actual: arg_types.len(),
                        span: call.span,
                    });
                }

                // Check if this is a generic function (contains type variables)
                let is_generic = func
                    .params
                    .iter()
                    .any(|&p| contains_type_variables(self.type_ctx, p))
                    || func
                        .rest_param
                        .is_some_and(|rp| contains_type_variables(self.type_ctx, rp))
                    || contains_type_variables(self.type_ctx, func.return_type);

                if is_generic {
                    // Use type unification for generic functions
                    let (inferred_return, failed_unifications) = {
                        let mut failed_unifications: Vec<(TypeId, TypeId, crate::parser::Span)> =
                            Vec::new();

                        // Apply explicit type arguments first (e.g., fn<int, string>(...)).
                        let explicit_substitutions: Vec<(String, TypeId)> =
                            if let Some(type_args) = &call.type_args {
                                let mut type_param_names = Vec::new();
                                let mut seen = std::collections::HashSet::new();
                                for &param_ty in &func.params {
                                    self.collect_type_var_names(
                                        param_ty,
                                        &mut type_param_names,
                                        &mut seen,
                                    );
                                }
                                if let Some(rest_ty) = func.rest_param {
                                    self.collect_type_var_names(
                                        rest_ty,
                                        &mut type_param_names,
                                        &mut seen,
                                    );
                                }
                                self.collect_type_var_names(
                                    func.return_type,
                                    &mut type_param_names,
                                    &mut seen,
                                );
                                type_param_names
                                    .into_iter()
                                    .zip(type_args.iter())
                                    .map(|(name, arg)| (name, self.resolve_type_annotation(arg)))
                                    .collect()
                            } else {
                                Vec::new()
                            };

                        let mut gen_ctx = GenericContext::new(self.type_ctx);
                        for (name, resolved) in explicit_substitutions {
                            gen_ctx.add_substitution(name, resolved);
                        }

                        // Unify each argument type with parameter type.
                        for (i, (arg_ty, arg_span)) in arg_types.iter().enumerate() {
                            let target_param = if i < fixed_param_len {
                                Some(func.params[i])
                            } else if let Some(rest_ty) = func.rest_param {
                                let rest_idx = i - fixed_param_len;
                                gen_ctx
                                    .rest_param_element_type(rest_ty, rest_idx)
                                    .unwrap_or(None)
                            } else {
                                None
                            };

                            if let Some(param_ty) = target_param {
                                match gen_ctx.unify(param_ty, *arg_ty) {
                                    Ok(true) => {}
                                    Ok(false) | Err(_) => {
                                        // Apply substitutions accumulated so far so that
                                        // check_assignable sees the concrete expected type.
                                        // Example: `listener: (...args: E[K]) => void` becomes
                                        // `(...args: [number]) => void` after K is resolved from
                                        // a prior argument, enabling proper subtype checking.
                                        let resolved_param = gen_ctx
                                            .apply_substitution(param_ty)
                                            .unwrap_or(param_ty);
                                        failed_unifications.push((
                                            *arg_ty,
                                            resolved_param,
                                            *arg_span,
                                        ));
                                    }
                                }
                            }
                        }

                        let inferred_return = match gen_ctx.apply_substitution(func.return_type) {
                            Ok(substituted_return) => substituted_return,
                            Err(_) => func.return_type,
                        };
                        (inferred_return, failed_unifications)
                    };

                    for (arg_ty, param_ty, arg_span) in failed_unifications {
                        self.check_assignable(arg_ty, param_ty, arg_span);
                    }

                    if func.is_async {
                        match self.type_ctx.get(inferred_return) {
                            Some(crate::parser::types::Type::Task(_)) => inferred_return,
                            _ => self.type_ctx.task_type(inferred_return),
                        }
                    } else {
                        inferred_return
                    }
                } else {
                    // Non-generic function - use simple type checking
                    for (i, (arg_ty, arg_span)) in arg_types.iter().enumerate() {
                        if i < fixed_param_len {
                            self.check_assignable(*arg_ty, func.params[i], *arg_span);
                        } else if let Some(ref tuple_elems) = rest_tuple_elems {
                            let tuple_idx = i - fixed_param_len;
                            if let Some(&elem_ty) = tuple_elems.get(tuple_idx) {
                                self.check_assignable(*arg_ty, elem_ty, *arg_span);
                            }
                        } else if let Some(elem_ty) = rest_array_elem_ty {
                            self.check_assignable(*arg_ty, elem_ty, *arg_span);
                        }
                    }
                    if func.is_async {
                        match self.type_ctx.get(func.return_type) {
                            Some(crate::parser::types::Type::Task(_)) => func.return_type,
                            _ => self.type_ctx.task_type(func.return_type),
                        }
                    } else {
                        func.return_type
                    }
                }
            }
            // Union of function types: pick the first function member and call it.
            // This supports patterns like `(Object[] | null).push(...)` where the
            // member access resolves to a union of function types.
            Some(crate::parser::types::Type::Union(union)) => {
                for &member_id in &union.members {
                    let mut member_func = None;
                    match self.type_ctx.get(member_id).cloned() {
                        Some(crate::parser::types::Type::Function(func)) => {
                            member_func = Some(func);
                        }
                        Some(crate::parser::types::Type::Object(obj)) => {
                            if let Some(sig_ty) = obj.call_signatures.first() {
                                if let Some(crate::parser::types::Type::Function(func)) =
                                    self.type_ctx.get(*sig_ty).cloned()
                                {
                                    member_func = Some(func);
                                }
                            }
                        }
                        Some(crate::parser::types::Type::Interface(iface)) => {
                            if let Some(sig_ty) = iface.call_signatures.first() {
                                if let Some(crate::parser::types::Type::Function(func)) =
                                    self.type_ctx.get(*sig_ty).cloned()
                                {
                                    member_func = Some(func);
                                }
                            }
                        }
                        _ => {}
                    }

                    if let Some(func) = member_func {
                        // Re-dispatch with this function type
                        for (i, (arg_ty, arg_span)) in arg_types.iter().enumerate() {
                            if i < func.params.len() {
                                self.check_assignable(*arg_ty, func.params[i], *arg_span);
                            }
                        }
                        return if func.is_async {
                            match self.type_ctx.get(func.return_type) {
                                Some(crate::parser::types::Type::Task(_)) => func.return_type,
                                _ => self.type_ctx.task_type(func.return_type),
                            }
                        } else {
                            func.return_type
                        };
                    }
                }
                self.errors.push(CheckError::NotCallable {
                    ty: self.format_type(callee_ty),
                    span: call.span,
                });
                self.type_ctx.unknown_type()
            }
            _ => {
                if self.allows_dynamic_any() && self.type_is_dynamic_anyish(callee_ty) {
                    return self.type_ctx.any_type();
                }
                self.errors.push(CheckError::NotCallable {
                    ty: self.format_type(callee_ty),
                    span: call.span,
                });
                self.type_ctx.unknown_type()
            }
        }
    }

    fn try_check_function_helper_call(&mut self, call: &CallExpression) -> Option<TypeId> {
        let Expression::Member(member) = call.callee.as_ref() else {
            return None;
        };
        let helper = self.resolve(member.property.name);
        if helper != "call" && helper != "apply" && helper != "bind" {
            return None;
        }

        let target_ty = self.check_expr(&member.object);
        self.check_unknown_actionable(target_ty, "call", *member.object.span());
        let target_fn = match self.type_ctx.get(target_ty).cloned() {
            Some(crate::parser::types::Type::Function(func)) => func,
            _ => {
                if self.allows_dynamic_any() && self.type_is_dynamic_anyish(target_ty) {
                    for arg in &call.arguments {
                        self.check_expr(arg);
                    }
                    return Some(self.type_ctx.any_type());
                }
                // Not a Function.prototype helper call on this target; treat it as
                // a regular member call (e.g. object.apply(...)).
                return None;
            }
        };

        let arg_types: Vec<(TypeId, crate::parser::Span)> = call
            .arguments
            .iter()
            .map(|arg| (self.check_expr(arg), *arg.span()))
            .collect();
        let expected_this_ty = self.infer_helper_expected_this_type(&member.object);
        let debug_helper = std::env::var_os("RAYA_DEBUG_HELPER_CALL").is_some();
        if debug_helper {
            let expected_desc = expected_this_ty
                .map(|expectation| match expectation {
                    HelperReceiverExpectation::Instance(ty) => {
                        format!("instance {}", self.format_type(ty))
                    }
                    HelperReceiverExpectation::StaticClass(ty) => {
                        format!("static {}", self.format_type(ty))
                    }
                })
                .unwrap_or_else(|| "none".to_string());
            let actual_desc = arg_types
                .first()
                .map(|(ty, _)| self.format_type(*ty))
                .unwrap_or_else(|| "<missing>".to_string());
            eprintln!(
                "[check-helper] helper={} target={} expected_this={} actual_this={} return={}",
                helper,
                self.format_type(target_ty),
                expected_desc,
                actual_desc,
                self.format_type(target_fn.return_type),
            );
        }

        match helper.as_str() {
            "call" => {
                self.check_helper_this_arg(expected_this_ty, &arg_types);
                self.check_function_args_for_helper(&target_fn, &arg_types, 1, call.span);
                let helper_return =
                    self.helper_call_return_type(&target_fn, expected_this_ty, &arg_types);
                if debug_helper {
                    eprintln!(
                        "[check-helper] helper={} preserved={} resolved_return={}",
                        helper,
                        self.helper_call_preserves_return_type(expected_this_ty, &arg_types),
                        self.format_type(helper_return),
                    );
                }
                Some(helper_return)
            }
            "apply" => {
                self.check_helper_this_arg(expected_this_ty, &arg_types);
                self.check_apply_helper_args(&target_fn, &arg_types, call.span);
                let helper_return =
                    self.helper_call_return_type(&target_fn, expected_this_ty, &arg_types);
                if debug_helper {
                    eprintln!(
                        "[check-helper] helper={} preserved={} resolved_return={}",
                        helper,
                        self.helper_call_preserves_return_type(expected_this_ty, &arg_types),
                        self.format_type(helper_return),
                    );
                }
                Some(helper_return)
            }
            "bind" => {
                self.check_helper_this_arg(expected_this_ty, &arg_types);
                let bound_count = self.check_bind_helper_args(&target_fn, &arg_types, call.span);
                Some(self.make_bound_function_type(&target_fn, bound_count))
            }
            _ => None,
        }
    }

    fn compute_fn_arity_bounds(
        &self,
        func: &crate::parser::types::ty::FunctionType,
    ) -> (usize, usize) {
        let fixed_len = func.params.len();
        match func
            .rest_param
            .and_then(|rest| self.type_ctx.get(rest))
            .cloned()
        {
            Some(crate::parser::types::Type::Tuple(t)) => (
                func.min_params + t.elements.len(),
                fixed_len + t.elements.len(),
            ),
            Some(crate::parser::types::Type::Array(_)) => (func.min_params, usize::MAX),
            Some(_) => (func.min_params, usize::MAX),
            None => (func.min_params, fixed_len),
        }
    }

    fn helper_param_type_at(
        &self,
        func: &crate::parser::types::ty::FunctionType,
        index: usize,
    ) -> Option<TypeId> {
        if index < func.params.len() {
            return Some(func.params[index]);
        }
        let rest_index = index.saturating_sub(func.params.len());
        match func
            .rest_param
            .and_then(|rest| self.type_ctx.get(rest))
            .cloned()
        {
            Some(crate::parser::types::Type::Tuple(t)) => t.elements.get(rest_index).copied(),
            Some(crate::parser::types::Type::Array(arr)) => Some(arr.element),
            _ => None,
        }
    }

    /// Check args for fn.call/fn.bind style helpers.
    /// `skip` is the number of leading helper args ignored for target invocation (thisArg).
    /// Returns count of consumed function arguments after the skipped helper args.
    fn check_function_args_for_helper(
        &mut self,
        func: &crate::parser::types::ty::FunctionType,
        helper_args: &[(TypeId, crate::parser::Span)],
        skip: usize,
        span: crate::parser::Span,
    ) -> usize {
        if self.enforce_call_arity() && helper_args.len() < skip {
            self.errors.push(CheckError::ArgumentCountMismatch {
                expected: skip,
                min_expected: skip,
                actual: helper_args.len(),
                span,
            });
            return 0;
        }

        let invocation_count = helper_args.len().saturating_sub(skip);
        let (min_args, max_args) = self.compute_fn_arity_bounds(func);
        if self.enforce_call_arity() && (invocation_count < min_args || invocation_count > max_args)
        {
            self.errors.push(CheckError::ArgumentCountMismatch {
                expected: max_args,
                min_expected: min_args,
                actual: invocation_count,
                span,
            });
        }

        for (idx, &(arg_ty, arg_span)) in helper_args.iter().skip(skip).enumerate() {
            if let Some(param_ty) = self.helper_param_type_at(func, idx) {
                self.check_assignable(arg_ty, param_ty, arg_span);
            }
        }

        invocation_count
    }

    fn check_apply_helper_args(
        &mut self,
        func: &crate::parser::types::ty::FunctionType,
        helper_args: &[(TypeId, crate::parser::Span)],
        span: crate::parser::Span,
    ) {
        if self.enforce_call_arity() && (helper_args.is_empty() || helper_args.len() > 2) {
            self.errors.push(CheckError::ArgumentCountMismatch {
                expected: 2,
                min_expected: 1,
                actual: helper_args.len(),
                span,
            });
            return;
        }

        // fn.apply() and fn.apply(thisArg) are both valid and equivalent to an empty arg list.
        if helper_args.len() <= 1 {
            let (min_args, _max_args) = self.compute_fn_arity_bounds(func);
            if self.enforce_call_arity() && min_args > 0 {
                self.errors.push(CheckError::ArgumentCountMismatch {
                    expected: func.params.len(),
                    min_expected: min_args,
                    actual: 0,
                    span,
                });
            }
            return;
        }

        let (args_ty, args_span) = helper_args[1];
        match self.type_ctx.get(args_ty).cloned() {
            Some(crate::parser::types::Type::Tuple(t)) => {
                let tuple_args: Vec<(TypeId, crate::parser::Span)> = t
                    .elements
                    .iter()
                    .copied()
                    .map(|ty| (ty, args_span))
                    .collect();
                self.check_function_args_for_helper(func, &tuple_args, 0, span);
            }
            Some(crate::parser::types::Type::Array(arr)) => {
                // With homogeneous arrays, each function parameter must accept the element type.
                let elem_ty = arr.element;
                let (min_args, _max_args) = self.compute_fn_arity_bounds(func);
                if min_args > 0 {
                    for idx in 0..min_args {
                        if let Some(param_ty) = self.helper_param_type_at(func, idx) {
                            self.check_assignable(elem_ty, param_ty, args_span);
                        }
                    }
                }
            }
            _ if self.is_js_mode() && self.apply_helper_accepts_runtime_array_like(args_ty) => {}
            _ => {
                self.errors.push(CheckError::TypeMismatch {
                    expected: "tuple or array".to_string(),
                    actual: self.format_type(args_ty),
                    span: args_span,
                    note: Some(
                        "Function.prototype.apply expects argument list as tuple/array".to_string(),
                    ),
                });
            }
        }
    }

    fn apply_helper_accepts_runtime_array_like(&self, ty: TypeId) -> bool {
        match self.type_ctx.get(ty) {
            Some(crate::parser::types::Type::Any)
            | Some(crate::parser::types::Type::Unknown)
            | Some(crate::parser::types::Type::JSObject)
            | Some(crate::parser::types::Type::Object(_))
            | Some(crate::parser::types::Type::Interface(_))
            | Some(crate::parser::types::Type::Class(_))
            | Some(crate::parser::types::Type::Function(_)) => true,
            Some(crate::parser::types::Type::Reference(reference)) => self
                .type_ctx
                .lookup_named_type(&reference.name)
                .is_some_and(|resolved| self.apply_helper_accepts_runtime_array_like(resolved)),
            Some(crate::parser::types::Type::Generic(generic)) => {
                self.apply_helper_accepts_runtime_array_like(generic.base)
            }
            Some(crate::parser::types::Type::TypeVar(type_var)) => type_var
                .constraint
                .is_some_and(|constraint| self.apply_helper_accepts_runtime_array_like(constraint)),
            Some(crate::parser::types::Type::Union(union)) => union
                .members
                .iter()
                .copied()
                .any(|member| self.apply_helper_accepts_runtime_array_like(member)),
            _ => false,
        }
    }

    /// Check args for fn.bind(thisArg, ...boundArgs).
    /// Returns number of bound args (excluding thisArg).
    fn check_bind_helper_args(
        &mut self,
        func: &crate::parser::types::ty::FunctionType,
        helper_args: &[(TypeId, crate::parser::Span)],
        span: crate::parser::Span,
    ) -> usize {
        if self.enforce_call_arity() && helper_args.is_empty() {
            self.errors.push(CheckError::ArgumentCountMismatch {
                expected: 1,
                min_expected: 1,
                actual: 0,
                span,
            });
            return 0;
        }

        let bound_count = helper_args.len().saturating_sub(1);
        let (_min_args, max_args) = self.compute_fn_arity_bounds(func);
        if self.enforce_call_arity() && bound_count > max_args {
            self.errors.push(CheckError::ArgumentCountMismatch {
                expected: max_args,
                min_expected: 0,
                actual: bound_count,
                span,
            });
        }

        for (idx, &(arg_ty, arg_span)) in helper_args.iter().skip(1).enumerate() {
            if let Some(param_ty) = self.helper_param_type_at(func, idx) {
                self.check_assignable(arg_ty, param_ty, arg_span);
            }
        }

        bound_count
    }

    fn make_bound_function_type(
        &mut self,
        func: &crate::parser::types::ty::FunctionType,
        bound_arg_count: usize,
    ) -> TypeId {
        let mut remaining_params = func.params.clone();
        let mut remaining_rest = func.rest_param;
        let mut consumed = bound_arg_count;

        if consumed >= remaining_params.len() {
            consumed -= remaining_params.len();
            remaining_params.clear();
        } else {
            remaining_params = remaining_params[consumed..].to_vec();
            consumed = 0;
        }

        if consumed > 0 {
            if let Some(rest_ty) = remaining_rest {
                match self.type_ctx.get(rest_ty).cloned() {
                    Some(crate::parser::types::Type::Tuple(t)) => {
                        if consumed >= t.elements.len() {
                            remaining_rest = None;
                        } else {
                            remaining_rest =
                                Some(self.type_ctx.tuple_type(t.elements[consumed..].to_vec()));
                        }
                    }
                    Some(crate::parser::types::Type::Array(_)) => {
                        // Still variadic after consuming prefix arguments.
                    }
                    _ => {}
                }
            }
        }

        let min_params = func.min_params.saturating_sub(bound_arg_count);
        self.type_ctx.function_type_with_rest(
            remaining_params,
            func.return_type,
            func.is_async,
            min_params,
            remaining_rest,
        )
    }

    /// Collect free variables from an expression
    fn collect_free_vars_expr(&self, expr: &Expression, collector: &mut FreeVariableCollector) {
        match expr {
            Expression::Identifier(ident) => {
                let name = self.resolve(ident.name);
                collector.reference(&name);
            }
            Expression::Binary(bin) => {
                self.collect_free_vars_expr(&bin.left, collector);
                self.collect_free_vars_expr(&bin.right, collector);
            }
            Expression::Logical(log) => {
                self.collect_free_vars_expr(&log.left, collector);
                self.collect_free_vars_expr(&log.right, collector);
            }
            Expression::Unary(un) => {
                self.collect_free_vars_expr(&un.operand, collector);
            }
            Expression::Assignment(assign) => {
                // LHS is an assignment target
                if let Expression::Identifier(ident) = assign.left.as_ref() {
                    let name = self.resolve(ident.name);
                    collector.assign(&name);
                } else {
                    self.collect_free_vars_expr(&assign.left, collector);
                }
                self.collect_free_vars_expr(&assign.right, collector);
            }
            Expression::Call(call) => {
                self.collect_free_vars_expr(&call.callee, collector);
                for arg in &call.arguments {
                    self.collect_free_vars_expr(arg, collector);
                }
            }
            Expression::Member(member) => {
                self.collect_free_vars_expr(&member.object, collector);
            }
            Expression::Index(idx) => {
                self.collect_free_vars_expr(&idx.object, collector);
                self.collect_free_vars_expr(&idx.index, collector);
            }
            Expression::Array(arr) => {
                for elem in arr.elements.iter().flatten() {
                    match elem {
                        ArrayElement::Expression(e) => self.collect_free_vars_expr(e, collector),
                        ArrayElement::Spread(e) => self.collect_free_vars_expr(e, collector),
                    }
                }
            }
            Expression::Object(obj) => {
                for prop in &obj.properties {
                    match prop {
                        ObjectProperty::Property(p) => {
                            self.collect_free_vars_expr(&p.value, collector);
                        }
                        ObjectProperty::Spread(spread) => {
                            self.collect_free_vars_expr(&spread.argument, collector);
                        }
                    }
                }
            }
            Expression::Conditional(cond) => {
                self.collect_free_vars_expr(&cond.test, collector);
                self.collect_free_vars_expr(&cond.consequent, collector);
                self.collect_free_vars_expr(&cond.alternate, collector);
            }
            Expression::Arrow(inner_arrow) => {
                // Nested arrow: create new collector for its body
                // but note what it captures from our scope
                let mut inner_collector = FreeVariableCollector::new();
                for param in &inner_arrow.params {
                    if let crate::parser::ast::Pattern::Identifier(ident) = &param.pattern {
                        inner_collector.bind(self.resolve(ident.name));
                    }
                }
                match &inner_arrow.body {
                    ArrowBody::Expression(e) => {
                        self.collect_free_vars_expr(e, &mut inner_collector)
                    }
                    ArrowBody::Block(b) => self.collect_free_vars_block(b, &mut inner_collector),
                }
                // Free vars of inner closure that aren't bound locally are our free vars too
                for var in inner_collector.free_variables() {
                    if !collector.bound_vars.contains(var) {
                        if inner_collector.is_assigned(var) {
                            collector.assign(var);
                        } else {
                            collector.reference(var);
                        }
                    }
                }
            }
            Expression::Typeof(ty) => {
                self.collect_free_vars_expr(&ty.argument, collector);
            }
            Expression::TemplateLiteral(tpl) => {
                for part in &tpl.parts {
                    if let TemplatePart::Expression(expr) = part {
                        self.collect_free_vars_expr(expr, collector);
                    }
                }
            }
            // Literals don't have free variables
            Expression::IntLiteral(_)
            | Expression::FloatLiteral(_)
            | Expression::StringLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_) => {}
            // Other expressions - handle remaining cases
            _ => {}
        }
    }

    /// Collect free variables from a block
    fn collect_free_vars_block(
        &self,
        block: &BlockStatement,
        collector: &mut FreeVariableCollector,
    ) {
        for stmt in &block.statements {
            self.collect_free_vars_stmt(stmt, collector);
        }
    }

    /// Collect free variables from a statement
    fn collect_free_vars_stmt(&self, stmt: &Statement, collector: &mut FreeVariableCollector) {
        match stmt {
            Statement::Expression(expr_stmt) => {
                self.collect_free_vars_expr(&expr_stmt.expression, collector);
            }
            Statement::VariableDecl(decl) => {
                // Initializer is evaluated before binding
                if let Some(ref init) = decl.initializer {
                    self.collect_free_vars_expr(init, collector);
                }
                // Then bind the variable(s) - supports destructuring patterns
                self.bind_pattern_in_collector(&decl.pattern, collector);
            }
            Statement::Return(ret) => {
                if let Some(ref val) = ret.value {
                    self.collect_free_vars_expr(val, collector);
                }
            }
            Statement::Yield(yld) => {
                if let Some(ref val) = yld.value {
                    self.collect_free_vars_expr(val, collector);
                }
            }
            Statement::If(if_stmt) => {
                self.collect_free_vars_expr(&if_stmt.condition, collector);
                self.collect_free_vars_stmt(&if_stmt.then_branch, collector);
                if let Some(ref else_branch) = if_stmt.else_branch {
                    self.collect_free_vars_stmt(else_branch, collector);
                }
            }
            Statement::While(while_stmt) => {
                self.collect_free_vars_expr(&while_stmt.condition, collector);
                self.collect_free_vars_stmt(&while_stmt.body, collector);
            }
            Statement::For(for_stmt) => {
                if let Some(ref init) = for_stmt.init {
                    match init {
                        ForInit::VariableDecl(decl) => {
                            if let Some(ref init_expr) = decl.initializer {
                                self.collect_free_vars_expr(init_expr, collector);
                            }
                            self.bind_pattern_in_collector(&decl.pattern, collector);
                        }
                        ForInit::Expression(e) => self.collect_free_vars_expr(e, collector),
                    }
                }
                if let Some(ref test) = for_stmt.test {
                    self.collect_free_vars_expr(test, collector);
                }
                if let Some(ref update) = for_stmt.update {
                    self.collect_free_vars_expr(update, collector);
                }
                self.collect_free_vars_stmt(&for_stmt.body, collector);
            }
            Statement::ForOf(for_of) => {
                self.collect_free_vars_expr(&for_of.right, collector);
                match &for_of.left {
                    ForOfLeft::VariableDecl(decl) => {
                        self.bind_pattern_in_collector(&decl.pattern, collector);
                    }
                    ForOfLeft::Pattern(p) => {
                        self.bind_pattern_in_collector(p, collector);
                    }
                }
                self.collect_free_vars_stmt(&for_of.body, collector);
            }
            Statement::ForIn(for_in) => {
                self.collect_free_vars_expr(&for_in.right, collector);
                match &for_in.left {
                    ForOfLeft::VariableDecl(decl) => {
                        self.bind_pattern_in_collector(&decl.pattern, collector);
                    }
                    ForOfLeft::Pattern(p) => {
                        self.bind_pattern_in_collector(p, collector);
                    }
                }
                self.collect_free_vars_stmt(&for_in.body, collector);
            }
            Statement::Labeled(labeled) => {
                self.collect_free_vars_stmt(&labeled.body, collector);
            }
            Statement::Block(block) => {
                self.collect_free_vars_block(block, collector);
            }
            Statement::Switch(switch_stmt) => {
                self.collect_free_vars_expr(&switch_stmt.discriminant, collector);
                for case in &switch_stmt.cases {
                    if let Some(ref test) = case.test {
                        self.collect_free_vars_expr(test, collector);
                    }
                    for stmt in &case.consequent {
                        self.collect_free_vars_stmt(stmt, collector);
                    }
                }
            }
            Statement::Try(try_stmt) => {
                self.collect_free_vars_block(&try_stmt.body, collector);
                if let Some(ref catch) = try_stmt.catch_clause {
                    // Catch parameter variables are bound in catch block (supports destructuring)
                    if let Some(ref param) = catch.param {
                        match param {
                            Pattern::Identifier(ident) => {
                                collector.bind(self.resolve(ident.name));
                            }
                            Pattern::Array(arr) => {
                                for elem in arr.elements.iter().flatten() {
                                    self.bind_pattern_in_collector(&elem.pattern, collector);
                                }
                                if let Some(rest) = &arr.rest {
                                    self.bind_pattern_in_collector(rest, collector);
                                }
                            }
                            Pattern::Object(obj) => {
                                for prop in &obj.properties {
                                    self.bind_pattern_in_collector(&prop.value, collector);
                                }
                                if let Some(rest) = &obj.rest {
                                    collector.bind(self.resolve(rest.name));
                                }
                            }
                            Pattern::Rest(rest) => {
                                self.bind_pattern_in_collector(&rest.argument, collector);
                            }
                        }
                    }
                    self.collect_free_vars_block(&catch.body, collector);
                }
                if let Some(ref finally) = try_stmt.finally_clause {
                    self.collect_free_vars_block(finally, collector);
                }
            }
            _ => {}
        }
    }

    /// Collect parameter names from a destructuring pattern for binding
    fn collect_pattern_param_names(&self, pattern: &Pattern, names: &mut Vec<String>) {
        match pattern {
            Pattern::Identifier(ident) => {
                names.push(self.resolve(ident.name));
            }
            Pattern::Array(arr) => {
                for elem in arr.elements.iter().flatten() {
                    self.collect_pattern_param_names(&elem.pattern, names);
                }
                if let Some(rest) = &arr.rest {
                    self.collect_pattern_param_names(rest, names);
                }
            }
            Pattern::Object(obj) => {
                for prop in &obj.properties {
                    self.collect_pattern_param_names(&prop.value, names);
                }
                if let Some(rest) = &obj.rest {
                    names.push(self.resolve(rest.name));
                }
            }
            Pattern::Rest(rest) => {
                self.collect_pattern_param_names(&rest.argument, names);
            }
        }
    }

    /// Bind all variables in a pattern to the free variable collector
    fn bind_pattern_in_collector(&self, pattern: &Pattern, collector: &mut FreeVariableCollector) {
        match pattern {
            Pattern::Identifier(ident) => {
                collector.bind(self.resolve(ident.name));
            }
            Pattern::Array(arr) => {
                for elem in arr.elements.iter().flatten() {
                    self.bind_pattern_in_collector(&elem.pattern, collector);
                }
                if let Some(rest) = &arr.rest {
                    self.bind_pattern_in_collector(rest, collector);
                }
            }
            Pattern::Object(obj) => {
                for prop in &obj.properties {
                    self.bind_pattern_in_collector(&prop.value, collector);
                }
                if let Some(rest) = &obj.rest {
                    collector.bind(self.resolve(rest.name));
                }
            }
            Pattern::Rest(rest) => {
                self.bind_pattern_in_collector(&rest.argument, collector);
            }
        }
    }

    /// Check arrow function
    fn check_arrow(&mut self, arrow: &crate::parser::ast::ArrowFunction) -> TypeId {
        // Save current type environment - parameters are scoped to the arrow body
        let saved_env = self.type_env.clone();

        // Collect parameter names for binding
        let mut param_names = Vec::new();
        let mut param_types = Vec::new();

        for param in &arrow.params {
            if !self.allows_implicit_any() && param.type_annotation.is_none() {
                self.errors
                    .push(CheckError::ImplicitAnyForbidden { span: param.span });
            }
            let param_ty = param
                .type_annotation
                .as_ref()
                .map(|t| self.resolve_type_annotation(t))
                .unwrap_or_else(|| self.inference_fallback_type());

            // Add parameter to type environment so it can be resolved in body
            // (rest parameters are included in the type environment for the body)
            if let crate::parser::ast::Pattern::Identifier(ident) = &param.pattern {
                let name = self.resolve(ident.name);
                param_names.push(name.clone());
                self.type_env.set(name, param_ty);
            } else {
                // For destructuring patterns, register all bound variables
                self.check_destructure_pattern(&param.pattern, param_ty);
                // Collect names from the pattern by traversing it
                self.collect_pattern_param_names(&param.pattern, &mut param_names);
            }

            // Add to param_types only if it's NOT a rest parameter
            // (rest parameters are tracked separately in rest_param field)
            if !param.is_rest {
                param_types.push(param_ty);
            }
        }

        // Collect free variables from the arrow body
        let mut collector = FreeVariableCollector::new();
        for name in &param_names {
            collector.bind(name.clone());
        }

        match &arrow.body {
            ArrowBody::Expression(expr) => {
                self.collect_free_vars_expr(expr, &mut collector);
            }
            ArrowBody::Block(block) => {
                self.collect_free_vars_block(block, &mut collector);
            }
        }

        // Build capture info from free variables
        let mut closure_captures = ClosureCaptures::new();
        for var_name in collector.free_variables() {
            // Look up the variable in outer scopes
            if let Some(symbol) = self
                .symbols
                .resolve_from_scope(var_name, self.current_scope)
            {
                // Check if this is actually from an outer scope (not global built-in)
                if symbol.scope_id.0 < self.current_scope.0
                    || symbol.scope_id == super::symbols::ScopeId(0)
                {
                    // Get the type (prefer inferred type if available)
                    let ty = self
                        .inferred_var_types
                        .get(&(symbol.scope_id.0, var_name.clone()))
                        .copied()
                        .unwrap_or(symbol.ty);

                    closure_captures.add(CaptureInfo {
                        name: var_name.clone(),
                        ty,
                        defining_scope: symbol.scope_id,
                        is_mutated: collector.is_assigned(var_name),
                        capture_span: arrow.span, // Use arrow span as capture site
                    });
                }
            }
        }

        // Store capture info for this closure
        if !closure_captures.is_empty() {
            self.capture_info
                .insert(ClosureId(arrow.span), closure_captures);
        }

        // Determine return type
        // Determine declared return type (if any)
        let declared_return_ty = arrow
            .return_type
            .as_ref()
            .map(|t| self.resolve_type_annotation(t));

        // Arrow bodies may contain scope-creating constructs (while loops, blocks)
        // that the binder never visited. Increment arrow_depth to make enter_scope/
        // exit_scope no-ops, keeping the checker's scope IDs in sync with the binder.
        self.arrow_depth += 1;

        let return_ty = match &arrow.body {
            crate::parser::ast::ArrowBody::Expression(expr) => {
                // For expression body, the return type is the expression's type
                let expr_ty = self.check_expr(expr);
                declared_return_ty.unwrap_or(expr_ty)
            }
            crate::parser::ast::ArrowBody::Block(block) => {
                // Save and set return type for return statement checking
                let prev_return_ty = self.current_function_return_type;

                // For async arrows, unwrap Promise<T> → T so return statements
                // are checked against T (same logic as check_function)
                let effective_return_ty = if arrow.is_async {
                    declared_return_ty.map(|ty| {
                        if let Some(crate::parser::types::Type::Task(task_ty)) =
                            self.type_ctx.get(ty)
                        {
                            task_ty.result
                        } else {
                            ty
                        }
                    })
                } else {
                    declared_return_ty
                };

                self.current_function_return_type = effective_return_ty;

                if effective_return_ty.is_none() {
                    self.return_type_collector.push(Vec::new());
                }

                self.seed_expression_local_declarations(block);

                // Check block statements
                for stmt in &block.statements {
                    self.check_stmt(stmt);
                }

                let inferred_return_ty = if effective_return_ty.is_none() {
                    let collected = self.return_type_collector.pop().unwrap_or_default();
                    if collected.is_empty() {
                        self.type_ctx.void_type()
                    } else {
                        let mut unique = Vec::new();
                        for ty in collected {
                            if !unique.contains(&ty) {
                                unique.push(ty);
                            }
                        }
                        if unique.len() == 1 {
                            unique[0]
                        } else {
                            self.type_ctx.union_type(unique)
                        }
                    }
                } else {
                    self.type_ctx.void_type()
                };

                // Restore previous return type
                self.current_function_return_type = prev_return_ty;

                // Use the effective return type or infer void
                effective_return_ty.unwrap_or(inferred_return_ty)
            }
        };

        self.arrow_depth -= 1;

        // Restore type environment
        self.type_env = saved_env;

        // Check for rest parameter
        let rest_param = arrow.params.iter().find(|p| p.is_rest).map(|param| {
            // Get the rest parameter's type (array type)
            param
                .type_annotation
                .as_ref()
                .map(|t| self.resolve_type_annotation(t))
                .unwrap_or_else(|| self.inference_fallback_type())
        });

        // Create function type with min_params for optional/default params
        // If there's a rest parameter, min_params is the count of non-rest params
        let min_params = if self.is_js_mode() {
            0
        } else if rest_param.is_some() {
            arrow
                .params
                .iter()
                .filter(|p| !p.is_rest && p.default_value.is_none() && !p.optional)
                .count()
        } else {
            arrow
                .params
                .iter()
                .filter(|p| p.default_value.is_none() && !p.optional)
                .count()
        };

        let function_return_ty = if arrow.is_async {
            match self.type_ctx.get(return_ty) {
                Some(crate::parser::types::Type::Task(task_ty)) => task_ty.result,
                _ => return_ty,
            }
        } else {
            return_ty
        };

        self.type_ctx.function_type_with_rest(
            param_types,
            function_return_ty,
            arrow.is_async,
            min_params,
            rest_param,
        )
    }

    fn seed_expression_local_declarations(
        &mut self,
        block: &crate::parser::ast::BlockStatement,
    ) {
        let placeholder_ty = self.type_ctx.any_type();
        for stmt in &block.statements {
            self.seed_expression_local_declarations_from_stmt(stmt, placeholder_ty);
        }
    }

    fn seed_expression_local_declarations_from_stmt(
        &mut self,
        stmt: &crate::parser::ast::Statement,
        placeholder_ty: TypeId,
    ) {
        match stmt {
            crate::parser::ast::Statement::ClassDecl(class) => {
                let name = self.resolve(class.name.name);
                self.type_env.set(name, placeholder_ty);
            }
            crate::parser::ast::Statement::FunctionDecl(func) => {
                let name = self.resolve(func.name.name);
                self.type_env.set(name, placeholder_ty);
            }
            crate::parser::ast::Statement::Block(block) => {
                for stmt in &block.statements {
                    self.seed_expression_local_declarations_from_stmt(stmt, placeholder_ty);
                }
            }
            crate::parser::ast::Statement::If(if_stmt) => {
                self.seed_expression_local_declarations_from_stmt(
                    &if_stmt.then_branch,
                    placeholder_ty,
                );
                if let Some(else_branch) = &if_stmt.else_branch {
                    self.seed_expression_local_declarations_from_stmt(else_branch, placeholder_ty);
                }
            }
            crate::parser::ast::Statement::While(while_stmt) => {
                self.seed_expression_local_declarations_from_stmt(&while_stmt.body, placeholder_ty);
            }
            crate::parser::ast::Statement::DoWhile(do_while) => {
                self.seed_expression_local_declarations_from_stmt(&do_while.body, placeholder_ty);
            }
            crate::parser::ast::Statement::For(for_stmt) => {
                self.seed_expression_local_declarations_from_stmt(&for_stmt.body, placeholder_ty);
            }
            crate::parser::ast::Statement::ForOf(for_of) => {
                self.seed_expression_local_declarations_from_stmt(&for_of.body, placeholder_ty);
            }
            crate::parser::ast::Statement::ForIn(for_in) => {
                self.seed_expression_local_declarations_from_stmt(&for_in.body, placeholder_ty);
            }
            crate::parser::ast::Statement::Labeled(labeled) => {
                self.seed_expression_local_declarations_from_stmt(&labeled.body, placeholder_ty);
            }
            crate::parser::ast::Statement::Switch(switch_stmt) => {
                for case in &switch_stmt.cases {
                    for stmt in &case.consequent {
                        self.seed_expression_local_declarations_from_stmt(stmt, placeholder_ty);
                    }
                }
            }
            crate::parser::ast::Statement::Try(try_stmt) => {
                for stmt in &try_stmt.body.statements {
                    self.seed_expression_local_declarations_from_stmt(stmt, placeholder_ty);
                }
                if let Some(catch) = &try_stmt.catch_clause {
                    for stmt in &catch.body.statements {
                        self.seed_expression_local_declarations_from_stmt(stmt, placeholder_ty);
                    }
                }
                if let Some(finally) = &try_stmt.finally_clause {
                    for stmt in &finally.statements {
                        self.seed_expression_local_declarations_from_stmt(stmt, placeholder_ty);
                    }
                }
            }
            crate::parser::ast::Statement::ExportDecl(
                crate::parser::ast::ExportDecl::Declaration(inner),
            ) => {
                self.seed_expression_local_declarations_from_stmt(inner, placeholder_ty);
            }
            _ => {}
        }
    }

    /// Check index access
    fn check_index(&mut self, index: &crate::parser::ast::IndexExpression) -> TypeId {
        let raw_object_ty = self.check_expr(&index.object);
        let index_ty = self.check_expr(&index.index);
        self.check_unknown_actionable(raw_object_ty, "index", *index.object.span());
        self.maybe_escalate_identifier_to_jsobject(&index.object, Some(&index.index));

        let object_ty = if index.optional {
            self.get_non_null_type(raw_object_ty)
        } else {
            raw_object_ty
        };

        // JSObject<T> supports dynamic key lookup.
        if self.type_ctx.jsobject_inner(object_ty).is_some()
            || matches!(
                self.type_ctx.get(object_ty),
                Some(crate::parser::types::Type::JSObject) | Some(crate::parser::types::Type::Any)
            )
        {
            return if self.allows_dynamic_any() {
                self.type_ctx.any_type()
            } else {
                self.inference_fallback_type()
            };
        }

        if let Some(inferred) = self.index_access_from_type(object_ty, index_ty, &index.index) {
            return inferred;
        }

        if self.is_js_mode() && self.type_uses_dynamic_js_member_fallback(object_ty) {
            return self
                .js_dynamic_index_value_type()
                .unwrap_or_else(|| self.inference_fallback_type());
        }

        if let Some(key) = self.string_key_from_index_expr(&index.index) {
            if self.emit_missing_index_property_diagnostic(object_ty, &key, index.span) {
                // Unavoidable fallback allowlist:
                // literal key miss on object-like types still needs a value type to continue checking.
                return self.fallback_type(
                    index.span,
                    FallbackReason::Unavoidable,
                    "index-missing",
                );
            }
        }

        self.fallback_type(
            index.span,
            FallbackReason::RecoverableUnsupportedExpr,
            "index-access",
        )
    }

    fn expr_is_es_array_index_key(&self, expr: &Expression) -> bool {
        match expr {
            Expression::IntLiteral(lit) => lit.value >= 0 && lit.value < u32::MAX as i64,
            Expression::FloatLiteral(lit) => {
                lit.value.is_finite()
                    && lit.value.fract() == 0.0
                    && lit.value >= 0.0
                    && lit.value < u32::MAX as f64
            }
            Expression::StringLiteral(lit) => {
                let key = self.resolve(lit.value);
                if key.is_empty() {
                    return false;
                }
                if key != "0" && key.starts_with('0') {
                    return false;
                }
                let Ok(index) = key.parse::<u32>() else {
                    return false;
                };
                index != u32::MAX && index.to_string() == key
            }
            _ => false,
        }
    }

    fn js_dynamic_index_value_type(&mut self) -> Option<TypeId> {
        if self.is_js_mode() {
            Some(if self.allows_dynamic_any() {
                self.type_ctx.any_type()
            } else {
                self.inference_fallback_type()
            })
        } else {
            None
        }
    }

    fn type_uses_dynamic_js_member_fallback(&self, ty: TypeId) -> bool {
        use crate::parser::types::{PrimitiveType, Type};

        if let Some(inner) = self.type_ctx.jsobject_inner(ty) {
            return self.type_uses_dynamic_js_member_fallback(inner);
        }

        match self.type_ctx.get(ty) {
            Some(Type::Function(_))
            | Some(Type::Array(_))
            | Some(Type::Tuple(_))
            | Some(Type::Object(_))
            | Some(Type::Class(_))
            | Some(Type::Interface(_))
            | Some(Type::Primitive(PrimitiveType::String))
            | Some(Type::StringLiteral(_)) => true,
            Some(Type::Reference(_)) => true,
            Some(Type::Generic(generic)) => self.type_ctx.get(generic.base).is_some_and(|base| {
                matches!(
                    base,
                    Type::Reference(_)
                        | Type::Class(_)
                        | Type::Object(_)
                        | Type::Interface(_)
                        | Type::Array(_)
                )
            }),
            Some(Type::TypeVar(tv)) => tv
                .constraint
                .is_some_and(|constraint| self.type_uses_dynamic_js_member_fallback(constraint)),
            Some(Type::Union(union)) => union
                .members
                .iter()
                .copied()
                .any(|member| self.type_uses_dynamic_js_member_fallback(member)),
            _ => false,
        }
    }

    fn index_access_from_type(
        &mut self,
        object_ty: TypeId,
        index_ty: TypeId,
        index_expr: &Expression,
    ) -> Option<TypeId> {
        use crate::parser::types::{PrimitiveType, Type};

        let obj_data = self.type_ctx.get(object_ty).cloned()?;
        match obj_data {
            Type::Json => Some(self.type_ctx.json_type()),
            Type::Array(arr) => {
                if !self.expr_is_es_array_index_key(index_expr) {
                    if let Some(key) = self.string_key_from_index_expr(index_expr) {
                        if let Some(prop_ty) = self.get_array_method_type(&key, arr.element) {
                            return Some(prop_ty);
                        }
                    }
                    return self.js_dynamic_index_value_type();
                }
                Some(arr.element)
            }
            Type::Tuple(tuple_ty) => {
                if !self.expr_is_es_array_index_key(index_expr) {
                    if let Some(key) = self.string_key_from_index_expr(index_expr) {
                        let elem_ty = if tuple_ty.elements.is_empty() {
                            self.type_ctx.unknown_type()
                        } else {
                            self.type_ctx.union_type(tuple_ty.elements.clone())
                        };
                        if let Some(prop_ty) = self.get_array_method_type(&key, elem_ty) {
                            return Some(prop_ty);
                        }
                    }
                    return self.js_dynamic_index_value_type();
                }
                if let Expression::IntLiteral(int_lit) = index_expr {
                    if let Ok(idx) = usize::try_from(int_lit.value) {
                        return tuple_ty.elements.get(idx).copied();
                    }
                }
                if matches!(
                    self.type_ctx.get(index_ty),
                    Some(Type::Primitive(PrimitiveType::Number | PrimitiveType::Int))
                ) {
                    if tuple_ty.elements.is_empty() {
                        Some(self.inference_fallback_type())
                    } else {
                        Some(self.type_ctx.union_type(tuple_ty.elements))
                    }
                } else {
                    None
                }
            }
            Type::Primitive(PrimitiveType::String) | Type::StringLiteral(_) => {
                if !self.expr_is_es_array_index_key(index_expr) {
                    if let Some(key) = self.string_key_from_index_expr(index_expr) {
                        if let Some(prop_ty) = self.get_string_method_type(&key) {
                            return Some(prop_ty);
                        }
                    }
                    return self.js_dynamic_index_value_type();
                }
                if matches!(
                    self.type_ctx.get(index_ty),
                    Some(Type::Primitive(PrimitiveType::Number | PrimitiveType::Int))
                        | Some(Type::NumberLiteral(_))
                ) {
                    Some(self.type_ctx.string_type())
                } else {
                    None
                }
            }
            Type::Object(obj) => {
                if let Some(key) = self.string_key_from_index_expr(index_expr) {
                    if let Some(prop_ty) =
                        obj.properties.iter().find(|p| p.name == key).map(|p| p.ty)
                    {
                        return Some(prop_ty);
                    }
                    if let Some((_, sig_ty)) = obj.index_signature {
                        return Some(sig_ty);
                    }
                    return None;
                }
                match self.type_ctx.get(index_ty) {
                    Some(Type::Primitive(PrimitiveType::String)) | Some(Type::StringLiteral(_)) => {
                        let mut out: Vec<TypeId> = obj.properties.iter().map(|p| p.ty).collect();
                        if let Some((_, sig_ty)) = obj.index_signature {
                            out.push(sig_ty);
                        }
                        if out.is_empty() {
                            None
                        } else if out.len() == 1 {
                            Some(out[0])
                        } else {
                            Some(self.type_ctx.union_type(out))
                        }
                    }
                    Some(Type::Primitive(PrimitiveType::Number | PrimitiveType::Int))
                    | Some(Type::NumberLiteral(_)) => obj.index_signature.map(|(_, sig_ty)| sig_ty),
                    _ => None,
                }
            }
            Type::Class(class_ty) => {
                if let Some(key) = self.string_key_from_index_expr(index_expr) {
                    if let Some((member_ty, _)) = self.lookup_class_member(&class_ty, &key) {
                        return Some(member_ty);
                    }
                }
                match self.type_ctx.get(index_ty) {
                    Some(Type::Primitive(PrimitiveType::String)) | Some(Type::StringLiteral(_)) => {
                        let mut out: Vec<TypeId> =
                            class_ty.properties.iter().map(|p| p.ty).collect();
                        out.extend(class_ty.methods.iter().map(|m| m.ty));
                        if out.is_empty() {
                            None
                        } else if out.len() == 1 {
                            Some(out[0])
                        } else {
                            Some(self.type_ctx.union_type(out))
                        }
                    }
                    _ => None,
                }
            }
            Type::Interface(interface_ty) => {
                if let Some(key) = self.string_key_from_index_expr(index_expr) {
                    return self.lookup_interface_member(&interface_ty, &key);
                }
                match self.type_ctx.get(index_ty) {
                    Some(Type::Primitive(PrimitiveType::String)) | Some(Type::StringLiteral(_)) => {
                        let mut out = Vec::new();
                        let mut visited_parents = std::collections::HashSet::new();
                        self.collect_interface_member_types(
                            &interface_ty,
                            &mut out,
                            &mut visited_parents,
                        );
                        if out.is_empty() {
                            None
                        } else if out.len() == 1 {
                            Some(out[0])
                        } else {
                            Some(self.type_ctx.union_type(out))
                        }
                    }
                    _ => None,
                }
            }
            Type::TypeVar(tv) => tv.constraint.and_then(|constraint| {
                self.index_access_from_type(constraint, index_ty, index_expr)
            }),
            // IndexedAccess and Keyof types are unresolved type-level operations on
            // type variables.  At runtime they always produce concrete values, but the
            // checker cannot reduce them further statically.  Treat numeric index into
            // an unresolved indexed-access type as unknown rather than emitting an error.
            Type::IndexedAccess(_) | Type::Keyof(_) => Some(self.type_ctx.unknown_type()),
            Type::Union(union) => {
                let mut out = Vec::new();
                for member in union.members {
                    if let Some(member_ty) =
                        self.index_access_from_type(member, index_ty, index_expr)
                    {
                        if !out.contains(&member_ty) {
                            out.push(member_ty);
                        }
                    }
                }
                if out.is_empty() {
                    None
                } else if out.len() == 1 {
                    Some(out[0])
                } else {
                    Some(self.type_ctx.union_type(out))
                }
            }
            _ => None,
        }
    }

    fn string_key_from_index_expr(&self, expr: &Expression) -> Option<String> {
        match expr {
            Expression::StringLiteral(lit) => Some(self.resolve(lit.value)),
            Expression::IntLiteral(int_lit) => Some(int_lit.value.to_string()),
            _ => None,
        }
    }

    fn has_named_member_or_index_signature(&mut self, ty: TypeId, key: &str) -> bool {
        use crate::parser::types::Type;
        match self.type_ctx.get(ty).cloned() {
            Some(Type::Tuple(tuple_ty)) => key
                .parse::<usize>()
                .map(|idx| idx < tuple_ty.elements.len())
                .unwrap_or(false),
            Some(Type::Interface(interface_ty)) => {
                self.lookup_interface_member(&interface_ty, key).is_some()
            }
            Some(Type::Object(obj)) => {
                obj.properties.iter().any(|p| p.name == key) || obj.index_signature.is_some()
            }
            Some(Type::Class(class_ty)) => self.lookup_class_member(&class_ty, key).is_some(),
            Some(Type::TypeVar(tv)) => tv
                .constraint
                .map(|constraint| self.has_named_member_or_index_signature(constraint, key))
                .unwrap_or(false),
            Some(Type::Union(union)) => union
                .members
                .iter()
                .copied()
                .any(|member| self.has_named_member_or_index_signature(member, key)),
            _ => false,
        }
    }

    fn emit_missing_index_property_diagnostic(
        &mut self,
        object_ty: TypeId,
        key: &str,
        span: Span,
    ) -> bool {
        use crate::parser::types::Type;
        match self.type_ctx.get(object_ty).cloned() {
            Some(Type::Tuple(_))
            | Some(Type::Interface(_))
            | Some(Type::Object(_))
            | Some(Type::Class(_))
            | Some(Type::TypeVar(_)) => {
                if self.has_named_member_or_index_signature(object_ty, key) {
                    return false;
                }
                self.errors.push(CheckError::PropertyNotFound {
                    property: key.to_string(),
                    ty: self.format_type(object_ty),
                    span,
                });
                true
            }
            Some(Type::Union(union)) => {
                let null_ty = self.type_ctx.null_type();
                let mut object_like_members = 0usize;
                let mut found = false;
                for member in union.members {
                    if member == null_ty {
                        continue;
                    }
                    match self.type_ctx.get(member).cloned() {
                        Some(Type::Tuple(_))
                        | Some(Type::Interface(_))
                        | Some(Type::Object(_))
                        | Some(Type::Class(_))
                        | Some(Type::TypeVar(_)) => {
                            object_like_members += 1;
                            if self.has_named_member_or_index_signature(member, key) {
                                found = true;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                if object_like_members > 0 && !found {
                    self.errors.push(CheckError::PropertyNotFound {
                        property: key.to_string(),
                        ty: self.format_type(object_ty),
                        span,
                    });
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    /// Check new expression (class instantiation)
    fn check_new(&mut self, new_expr: &crate::parser::ast::NewExpression) -> TypeId {
        let mut deferred_non_class_name: Option<String> = None;
        // Get the callee type (should be a class)
        if let Expression::Identifier(ident) = &*new_expr.callee {
            let name = self.resolve(ident.name);

            if self.is_js_mode() && name == "Function" {
                for arg in &new_expr.arguments {
                    self.check_expr(arg);
                }
                let any_ty = self.type_ctx.any_type();
                return self.type_ctx.function_type(vec![], any_ty, false);
            }

            // Resolve type arguments if present
            let resolved_type_args: Vec<TypeId> = new_expr
                .type_args
                .as_ref()
                .map(|args| {
                    args.iter()
                        .map(|arg| self.resolve_type_annotation(arg))
                        .collect()
                })
                .unwrap_or_default();

            // Check for built-in types with type parameters
            // Note: Mutex is now a normal class from mutex.raya, not special-cased
            let builtin_type = match name.as_str() {
                "RegExp" => Some(self.type_ctx.regexp_type()),
                "Array" => {
                    // Array<T> - expect 1 type argument, defaults to unknown element type
                    if resolved_type_args.len() == 1 {
                        Some(self.type_ctx.array_type(resolved_type_args[0]))
                    } else {
                        let unknown = self.type_ctx.unknown_type();
                        Some(self.type_ctx.array_type(unknown))
                    }
                }
                "Map" => {
                    // Map<K, V> - expect 2 type arguments
                    if resolved_type_args.len() == 2 {
                        Some(
                            self.type_ctx
                                .map_type_with(resolved_type_args[0], resolved_type_args[1]),
                        )
                    } else {
                        Some(self.type_ctx.map_type())
                    }
                }
                "Set" => {
                    // Set<T> - expect 1 type argument
                    if resolved_type_args.len() == 1 {
                        Some(self.type_ctx.set_type_with(resolved_type_args[0]))
                    } else {
                        Some(self.type_ctx.set_type())
                    }
                }
                // Note: Buffer and Date are now normal classes from their .raya files
                "Channel" => {
                    // Channel<T> - expect 1 type argument
                    if resolved_type_args.len() == 1 {
                        Some(self.type_ctx.channel_type_with(resolved_type_args[0]))
                    } else {
                        Some(self.type_ctx.channel_type())
                    }
                }
                _ => None,
            };

            if let Some(ty) = builtin_type {
                // Check constructor arguments
                for arg in &new_expr.arguments {
                    self.check_expr(arg);
                }
                return ty;
            }

            // Look up the class symbol to get its type
            if let Some(symbol) = self.symbols.resolve_from_scope(&name, self.current_scope) {
                if symbol.kind == SymbolKind::Class {
                    // Check if the class is abstract (cannot be instantiated)
                    if let Some(crate::parser::types::Type::Class(class)) =
                        self.type_ctx.get(symbol.ty).cloned()
                    {
                        if class.is_abstract {
                            self.errors.push(CheckError::AbstractClassInstantiation {
                                name: name.clone(),
                                span: new_expr.span,
                            });
                        }
                        if let Some(ctor_sig) = class
                            .methods
                            .iter()
                            .find(|method| method.name == "constructor")
                        {
                            let allow_non_public_ctor = self
                                .current_class_type
                                .and_then(|ty| self.resolve_class_type(ty))
                                .map(|current| current.name == class.name)
                                .unwrap_or(false);
                            if ctor_sig.visibility != crate::parser::ast::Visibility::Public
                                && !allow_non_public_ctor
                            {
                                // Non-public constructors are not directly constructible.
                                self.errors.push(CheckError::NewNonClass {
                                    name: name.clone(),
                                    span: new_expr.span,
                                });
                            }
                        }
                    }

                    // Check constructor arguments (for now, just check them)
                    for arg in &new_expr.arguments {
                        self.check_expr(arg);
                    }

                    // If the class has type parameters and we have type arguments,
                    // create an instantiated class type with type vars substituted
                    if !resolved_type_args.is_empty() {
                        if let Some(crate::parser::types::Type::Class(class)) =
                            self.type_ctx.get(symbol.ty).cloned()
                        {
                            if class.type_params.len() == resolved_type_args.len() {
                                return self.instantiate_class_type(&class, &resolved_type_args);
                            }
                        }
                    }

                    return symbol.ty;
                } else if self.is_constructible_var(&name)
                    || self
                        .get_var_type(&name)
                        .is_some_and(|ty| self.type_is_js_constructible_callable(ty))
                {
                    let constructor_ty = self.get_var_type(&name);
                    // Support class aliases (e.g., const B = A; new B()) for imported or helper-bound constructors.
                    for arg in &new_expr.arguments {
                        self.check_expr(arg);
                    }
                    if let Some(ty) = constructor_ty {
                        if let Some(return_ty) = self.first_construct_signature_return_type(ty) {
                            return return_ty;
                        }
                        return ty;
                    }
                    return self.type_ctx.unknown_type();
                } else {
                    // Symbol exists but is not a nominal class. Defer reporting
                    // until structural construct-signature checks have a chance.
                    deferred_non_class_name = Some(name.clone());
                }
            }
        }

        let callee_ty = self.check_expr(&new_expr.callee);
        let mut ctor_fn: Option<crate::parser::types::ty::FunctionType> = None;
        match self.type_ctx.get(callee_ty).cloned() {
            Some(crate::parser::types::Type::Function(func)) => {
                if self.is_js_mode() {
                    ctor_fn = Some(func);
                }
            }
            Some(crate::parser::types::Type::Object(obj)) => {
                if let Some(sig_ty) = obj.construct_signatures.first() {
                    if let Some(crate::parser::types::Type::Function(func)) =
                        self.type_ctx.get(*sig_ty).cloned()
                    {
                        ctor_fn = Some(func);
                    }
                }
            }
            Some(crate::parser::types::Type::Interface(iface)) => {
                if let Some(sig_ty) = iface.construct_signatures.first() {
                    if let Some(crate::parser::types::Type::Function(func)) =
                        self.type_ctx.get(*sig_ty).cloned()
                    {
                        ctor_fn = Some(func);
                    }
                }
            }
            Some(crate::parser::types::Type::Union(union)) => {
                for member in union.members {
                    match self.type_ctx.get(member).cloned() {
                        Some(crate::parser::types::Type::Function(func)) => {
                            ctor_fn = Some(func);
                            break;
                        }
                        Some(crate::parser::types::Type::Object(obj)) => {
                            if let Some(sig_ty) = obj.construct_signatures.first() {
                                if let Some(crate::parser::types::Type::Function(func)) =
                                    self.type_ctx.get(*sig_ty).cloned()
                                {
                                    ctor_fn = Some(func);
                                    break;
                                }
                            }
                        }
                        Some(crate::parser::types::Type::Interface(iface)) => {
                            if let Some(sig_ty) = iface.construct_signatures.first() {
                                if let Some(crate::parser::types::Type::Function(func)) =
                                    self.type_ctx.get(*sig_ty).cloned()
                                {
                                    ctor_fn = Some(func);
                                    break;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }

        if let Some(func) = ctor_fn {
            let arg_types: Vec<(TypeId, crate::parser::Span)> = new_expr
                .arguments
                .iter()
                .map(|arg| (self.check_expr(arg), *arg.span()))
                .collect();
            if self.is_js_mode() {
                return self.type_ctx.unknown_type();
            }
            let (min_params, max_params) = self.compute_fn_arity_bounds(&func);
            if self.enforce_call_arity()
                && (arg_types.len() < min_params || arg_types.len() > max_params)
            {
                self.errors.push(CheckError::ArgumentCountMismatch {
                    expected: func.params.len(),
                    min_expected: min_params,
                    actual: arg_types.len(),
                    span: new_expr.span,
                });
            }
            for (index, (arg_ty, arg_span)) in arg_types.iter().enumerate() {
                if let Some(param_ty) = self.helper_param_type_at(&func, index) {
                    self.check_assignable(*arg_ty, param_ty, *arg_span);
                }
            }
            return func.return_type;
        }

        if self.is_js_mode() {
            for arg in &new_expr.arguments {
                self.check_expr(arg);
            }
            return self.type_ctx.unknown_type();
        }

        if let Some(name) = deferred_non_class_name {
            self.errors.push(CheckError::NewNonClass {
                name,
                span: new_expr.span,
            });
        }

        // Check arguments even if we can't determine the class
        for arg in &new_expr.arguments {
            self.check_expr(arg);
        }
        self.type_ctx.unknown_type()
    }

    fn first_construct_signature_return_type(&mut self, ty: TypeId) -> Option<TypeId> {
        match self.type_ctx.get(ty).cloned() {
            Some(crate::parser::types::Type::Function(func)) => Some(if self.is_js_mode() {
                self.type_ctx.unknown_type()
            } else {
                func.return_type
            }),
            Some(crate::parser::types::Type::Object(obj)) => obj
                .construct_signatures
                .first()
                .and_then(|sig_ty| match self.type_ctx.get(*sig_ty).cloned() {
                    Some(crate::parser::types::Type::Function(func)) => Some(func.return_type),
                    _ => None,
                }),
            Some(crate::parser::types::Type::Interface(iface)) => iface
                .construct_signatures
                .first()
                .and_then(|sig_ty| match self.type_ctx.get(*sig_ty).cloned() {
                    Some(crate::parser::types::Type::Function(func)) => Some(func.return_type),
                    _ => None,
                }),
            Some(crate::parser::types::Type::Union(union)) => union
                .members
                .into_iter()
                .find_map(|member| self.first_construct_signature_return_type(member)),
            Some(crate::parser::types::Type::Reference(type_ref)) => self
                .type_ctx
                .lookup_named_type(&type_ref.name)
                .and_then(|named| self.first_construct_signature_return_type(named)),
            _ => None,
        }
    }

    /// Check this expression
    fn check_this(&mut self, span: Span) -> TypeId {
        // Return the current class type if we're inside a class method
        if let Some(class_ty) = self.current_class_type {
            return class_ty;
        }
        if self.policy.no_implicit_this {
            self.errors.push(CheckError::ImplicitThisForbidden { span });
        }
        // Outside of a class, 'this' is unknown
        self.type_ctx.unknown_type()
    }

    /// Check await expression
    fn check_await(&mut self, await_expr: &crate::parser::ast::AwaitExpression) -> TypeId {
        // Check the argument expression
        let arg_ty = self.check_expr(&await_expr.argument);
        self.check_unknown_actionable(arg_ty, "await", *await_expr.argument.span());

        // If the argument is a Promise<T>, return T
        if let Some(crate::parser::types::Type::Task(task_ty)) = self.type_ctx.get(arg_ty) {
            return task_ty.result;
        }

        // If the argument is a Promise class instance (e.g., `await this` inside Promise class body),
        // resolve T from the class type parameter in the current scope.
        if let Some(crate::parser::types::Type::Class(class)) = self.type_ctx.get(arg_ty) {
            if class.name == "Promise" && !class.type_params.is_empty() {
                if let Some(symbol) = self
                    .symbols
                    .resolve_from_scope(&class.type_params[0], self.current_scope)
                {
                    return symbol.ty;
                }
            }
        }

        // If the argument is Promise<T>[], return T[] (parallel await)
        if let Some(crate::parser::types::Type::Array(arr_ty)) = self.type_ctx.get(arg_ty) {
            if let Some(crate::parser::types::Type::Task(task_ty)) =
                self.type_ctx.get(arr_ty.element)
            {
                return self.type_ctx.array_type(task_ty.result);
            }
            if let Some(crate::parser::types::Type::Class(class_ty)) =
                self.type_ctx.get(arr_ty.element)
            {
                if class_ty.name == "Promise" && !class_ty.type_params.is_empty() {
                    if let Some(symbol) = self
                        .symbols
                        .resolve_from_scope(&class_ty.type_params[0], self.current_scope)
                    {
                        return self.type_ctx.array_type(symbol.ty);
                    }
                }
            }
        }

        // Dynamic-anyish is allowed through in compatibility modes.
        if self.allows_dynamic_any() && self.type_is_dynamic_anyish(arg_ty) {
            return self.type_ctx.any_type();
        }

        // JS-compatible await semantics: non-Promise values resolve immediately.
        arg_ty
    }

    /// Try to check a compiler intrinsic call (__OPCODE_* or __NATIVE_CALL)
    /// Returns Some(type) if this is an intrinsic, None otherwise
    fn try_check_intrinsic(&mut self, call: &CallExpression) -> Option<TypeId> {
        // Check if callee is an identifier
        let ident = match &*call.callee {
            Expression::Identifier(id) => id,
            _ => return None,
        };

        let name = self.resolve(ident.name);

        // Check for __NATIVE_CALL intrinsic
        // Supports __NATIVE_CALL<T>(id, args...) to specify return type T
        if name == "__NATIVE_CALL" {
            if let Some(ref type_args) = call.type_args {
                if type_args.len() == 1 {
                    return Some(self.resolve_type_annotation(&type_args[0]));
                }
            }
            return Some(self.fallback_type(
                call.span,
                FallbackReason::Unavoidable,
                "__NATIVE_CALL",
            ));
        }

        None
    }

    /// Get the return type for an __OPCODE_* intrinsic
    fn get_opcode_intrinsic_type(
        &mut self,
        opcode_name: &str,
        call: &CallExpression,
        arg_types: &[(TypeId, Span)],
    ) -> TypeId {
        match opcode_name {
            // Mutex operations
            "MUTEX_NEW" => self.type_ctx.number_type(), // Returns native mutex handle
            "MUTEX_LOCK" | "MUTEX_UNLOCK" => self.type_ctx.void_type(),

            // Channel operations
            "CHANNEL_NEW" => {
                if let Some(type_args) = &call.type_args {
                    if type_args.len() == 1 {
                        let msg_ty = self.resolve_type_annotation(&type_args[0]);
                        return self.type_ctx.channel_type_with(msg_ty);
                    }
                }
                self.type_ctx.channel_type()
            }

            // Promise operations
            "TASK_CANCEL" => self.type_ctx.void_type(),
            "AWAIT" => {
                if let Some((operand_ty, _)) = arg_types.first() {
                    if let Some(crate::parser::types::Type::Task(task_ty)) =
                        self.type_ctx.get(*operand_ty)
                    {
                        return task_ty.result;
                    }
                    if let Some(crate::parser::types::Type::Class(class_ty)) =
                        self.type_ctx.get(*operand_ty)
                    {
                        if class_ty.name == "Promise" && !class_ty.type_params.is_empty() {
                            if let Some(sym) = self
                                .symbols
                                .resolve_from_scope(&class_ty.type_params[0], self.current_scope)
                            {
                                return sym.ty;
                            }
                        }
                    }
                    if self.allows_dynamic_any() && self.type_is_dynamic_anyish(*operand_ty) {
                        return self.type_ctx.any_type();
                    }
                }
                self.fallback_type(
                    call.span,
                    FallbackReason::RecoverableInvalidIntrinsicContext,
                    "__OPCODE_AWAIT",
                )
            }
            "AWAIT_ALL" => {
                if let Some((operand_ty, _)) = arg_types.first() {
                    if let Some(crate::parser::types::Type::Array(arr_ty)) =
                        self.type_ctx.get(*operand_ty)
                    {
                        if let Some(crate::parser::types::Type::Task(task_ty)) =
                            self.type_ctx.get(arr_ty.element)
                        {
                            return self.type_ctx.array_type(task_ty.result);
                        }
                        if let Some(crate::parser::types::Type::Class(class_ty)) =
                            self.type_ctx.get(arr_ty.element)
                        {
                            if class_ty.name == "Promise" && !class_ty.type_params.is_empty() {
                                if let Some(sym) = self.symbols.resolve_from_scope(
                                    &class_ty.type_params[0],
                                    self.current_scope,
                                ) {
                                    return self.type_ctx.array_type(sym.ty);
                                }
                            }
                        }
                    }
                    if self.allows_dynamic_any() && self.type_is_dynamic_anyish(*operand_ty) {
                        return self.type_ctx.any_type();
                    }
                }
                self.fallback_type(
                    call.span,
                    FallbackReason::RecoverableInvalidIntrinsicContext,
                    "__OPCODE_AWAIT_ALL",
                )
            }
            "YIELD" => self.type_ctx.void_type(),
            "SLEEP" => self.type_ctx.void_type(),

            // RefCell operations
            "REFCELL_NEW" => self.type_ctx.unknown_type(), // Returns RefCell
            "REFCELL_LOAD" => self.type_ctx.unknown_type(), // Returns the contained value
            "REFCELL_STORE" => self.type_ctx.void_type(),

            // Global operations
            "LOAD_GLOBAL" => {
                if let Some(type_args) = &call.type_args {
                    if type_args.len() == 1 {
                        return self.resolve_type_annotation(&type_args[0]);
                    }
                }
                self.fallback_type(
                    call.span,
                    FallbackReason::Unavoidable,
                    "__OPCODE_LOAD_GLOBAL",
                )
            }
            "STORE_GLOBAL" => self.type_ctx.void_type(),

            // Array/Object operations
            "ARRAY_LEN" => self.type_ctx.number_type(),
            "STRING_LEN" => self.type_ctx.number_type(),
            "TYPEOF" => self.type_ctx.string_type(),
            "TO_STRING" => self.type_ctx.string_type(),

            // Unknown opcode - return unknown
            _ => self.fallback_type(call.span, FallbackReason::Unavoidable, "opcode-unknown"),
        }
    }

    /// Check async call expression (async funcCall() syntax)
    fn check_async_call(&mut self, async_call: &crate::parser::ast::AsyncCallExpression) -> TypeId {
        // Check all arguments
        for arg in &async_call.arguments {
            self.check_expr(arg);
        }

        // Get the callee's return type
        let callee_ty = self.check_expr(&async_call.callee);
        self.check_unknown_actionable(callee_ty, "async-call", *async_call.callee.span());

        // If the callee is a function, get its return type and wrap in Promise
        if let Some(crate::parser::types::Type::Function(func_ty)) = self.type_ctx.get(callee_ty) {
            let return_ty = func_ty.return_type;
            if matches!(
                self.type_ctx.get(return_ty),
                Some(crate::parser::types::Type::Task(_))
            ) {
                return return_ty;
            }
            return self.type_ctx.task_type(return_ty);
        }

        // If callee already resolves to Promise<T>/Task<T>, preserve it.
        if matches!(
            self.type_ctx.get(callee_ty),
            Some(crate::parser::types::Type::Task(_))
        ) {
            return callee_ty;
        }

        // Non-callable async callee is a compile-time error.
        self.errors.push(CheckError::NotCallable {
            ty: self.format_type(callee_ty),
            span: async_call.span,
        });

        // Keep checking flow moving with Promise<unknown> fallback.
        let unknown = self.fallback_type(
            async_call.span,
            FallbackReason::Unavoidable,
            "async-call-unknown-return",
        );
        self.type_ctx.task_type(unknown)
    }

    /// Check instanceof expression: expr instanceof ClassName
    fn check_instanceof(
        &mut self,
        instanceof: &crate::parser::ast::InstanceOfExpression,
    ) -> TypeId {
        // Check the object expression
        self.check_expr(&instanceof.object);

        // The target type should be a class type
        // For now, just check that it's a valid type reference
        // TODO: Validate that the target is actually a class, not a primitive or interface

        // instanceof always returns boolean
        self.type_ctx.boolean_type()
    }

    /// Check type cast expression: expr as TypeName
    fn check_type_cast(&mut self, cast: &crate::parser::ast::TypeCastExpression) -> TypeId {
        // Check the object expression
        let _object_ty = self.check_expr(&cast.object);

        // Resolve the target type from the type annotation

        // TODO: Validate that the cast is safe (object type is related to target type)
        // For now, we allow all casts (like TypeScript's `as` keyword)

        // Return the target type
        self.resolve_type_annotation(&cast.target_type)
    }

    /// Check member access
    fn check_member(&mut self, member: &MemberExpression) -> TypeId {
        let property_name = self.resolve(member.property.name);

        // super.member access inside class methods
        if let Expression::Super(_) = &*member.object {
            let Some(class_ty) = self.current_class_type else {
                self.errors.push(CheckError::PropertyNotFound {
                    property: property_name,
                    ty: "super".to_string(),
                    span: member.span,
                });
                return self.type_ctx.unknown_type();
            };

            let Some(current_class) = self.resolve_class_type(class_ty) else {
                self.errors.push(CheckError::PropertyNotFound {
                    property: property_name,
                    ty: "super".to_string(),
                    span: member.span,
                });
                return self.type_ctx.unknown_type();
            };

            let Some(parent_ty) = current_class.extends else {
                self.errors.push(CheckError::PropertyNotFound {
                    property: property_name,
                    ty: format!("class {}", current_class.name),
                    span: member.span,
                });
                return self.type_ctx.unknown_type();
            };

            let Some(parent_class) = self.resolve_class_type(parent_ty) else {
                self.errors.push(CheckError::PropertyNotFound {
                    property: property_name,
                    ty: format!("class {}", current_class.name),
                    span: member.span,
                });
                return self.type_ctx.unknown_type();
            };

            if let Some((ty, vis, owner_name)) =
                self.lookup_class_member_with_owner(&parent_class, &property_name)
            {
                if vis == crate::parser::ast::Visibility::Private {
                    self.errors.push(CheckError::PropertyNotFound {
                        property: format!("private member '{}'", property_name),
                        ty: owner_name,
                        span: member.span,
                    });
                    return self.type_ctx.unknown_type();
                }
                return ty;
            }

            self.errors.push(CheckError::PropertyNotFound {
                property: property_name,
                ty: format!("class {}", parent_class.name),
                span: member.span,
            });
            return self.type_ctx.unknown_type();
        }

        let object_ty = self.check_expr(&member.object);
        self.check_unknown_actionable(object_ty, "member", *member.object.span());

        let lookup_object_ty = if member.optional {
            self.get_non_null_type(object_ty)
        } else {
            let non_null_object_ty = self.get_non_null_type(object_ty);
            if non_null_object_ty != object_ty {
                if self.is_js_mode() {
                    non_null_object_ty
                } else if !self.in_assignment_lhs {
                    self.errors.push(CheckError::TypeMismatch {
                        expected: "non-null object".to_string(),
                        actual: self.format_type(object_ty),
                        span: member.span,
                        note: Some(
                            "Use optional chaining (?.) or a null check before member access"
                                .to_string(),
                        ),
                    });
                    return self.type_ctx.unknown_type();
                } else {
                    non_null_object_ty
                }
            } else {
                object_ty
            }
        };

        // Check for forbidden access to $type/$value on bare unions
        if property_name == "$type" || property_name == "$value" {
            if let Some(crate::parser::types::Type::Union(union)) = self.type_ctx.get(object_ty) {
                if union.is_bare {
                    self.errors.push(CheckError::ForbiddenFieldAccess {
                        field: property_name,
                        span: member.span,
                    });
                    return self.type_ctx.unknown_type();
                }
            }
        }

        // Check if the object is a class name (static member access)
        // This happens when the object is an identifier that resolves to a class symbol
        if let Expression::Identifier(ident) = &*member.object {
            let class_name = self.resolve(ident.name);
            if self.is_js_mode() && class_name == "Object" {
                match property_name.as_str() {
                    "getOwnPropertyNames" => {
                        let any_ty = self.type_ctx.any_type();
                        let string_ty = self.type_ctx.string_type();
                        let string_array = self.type_ctx.array_type(string_ty);
                        return self
                            .type_ctx
                            .function_type(vec![any_ty], string_array, false);
                    }
                    "getOwnPropertyDescriptor" => {
                        let any_ty = self.type_ctx.any_type();
                        return self
                            .type_ctx
                            .function_type(vec![any_ty, any_ty], any_ty, false);
                    }
                    "defineProperty" => {
                        let any_ty = self.type_ctx.any_type();
                        return self.type_ctx.function_type(
                            vec![any_ty, any_ty, any_ty],
                            any_ty,
                            false,
                        );
                    }
                    "defineProperties" => {
                        let any_ty = self.type_ctx.any_type();
                        return self
                            .type_ctx
                            .function_type(vec![any_ty, any_ty], any_ty, false);
                    }
                    _ => {}
                }
            }
            if self.is_js_mode() && class_name == "Reflect" {
                match property_name.as_str() {
                    "has" => {
                        let any_ty = self.type_ctx.any_type();
                        let bool_ty = self.type_ctx.boolean_type();
                        return self
                            .type_ctx
                            .function_type(vec![any_ty, any_ty], bool_ty, false);
                    }
                    _ => {}
                }
            }
            let static_class_symbol_ty = self
                .symbols
                .resolve_from_scope(&class_name, self.current_scope)
                .filter(|symbol| symbol.kind == SymbolKind::Class)
                .map(|symbol| symbol.ty)
                .or_else(|| {
                    if self.is_js_mode() {
                        self.type_ctx.lookup_named_type(&class_name)
                    } else {
                        None
                    }
                });
            if let Some(static_ty) = static_class_symbol_ty {
                if let Some(class) = self.resolve_class_type(static_ty) {
                    if self.is_js_mode() && class.name == "Object" {
                        match property_name.as_str() {
                            "getOwnPropertyNames" => {
                                let any_ty = self.type_ctx.any_type();
                                let string_ty = self.type_ctx.string_type();
                                let string_array = self.type_ctx.array_type(string_ty);
                                return self.type_ctx.function_type(
                                    vec![any_ty],
                                    string_array,
                                    false,
                                );
                            }
                            "getOwnPropertyDescriptor" => {
                                let any_ty = self.type_ctx.any_type();
                                return self.type_ctx.function_type(
                                    vec![any_ty, any_ty],
                                    any_ty,
                                    false,
                                );
                            }
                            "defineProperty" => {
                                let any_ty = self.type_ctx.any_type();
                                return self.type_ctx.function_type(
                                    vec![any_ty, any_ty, any_ty],
                                    any_ty,
                                    false,
                                );
                            }
                            "defineProperties" => {
                                let any_ty = self.type_ctx.any_type();
                                return self.type_ctx.function_type(
                                    vec![any_ty, any_ty],
                                    any_ty,
                                    false,
                                );
                            }
                            _ => {}
                        }
                    }
                    if self.is_js_mode() && class.name == "Reflect" {
                        match property_name.as_str() {
                            "has" => {
                                let any_ty = self.type_ctx.any_type();
                                let bool_ty = self.type_ctx.boolean_type();
                                return self.type_ctx.function_type(
                                    vec![any_ty, any_ty],
                                    bool_ty,
                                    false,
                                );
                            }
                            _ => {}
                        }
                    }
                    if property_name == "prototype" {
                        if self.is_js_mode() {
                            return if self.allows_dynamic_any() {
                                self.type_ctx.any_type()
                            } else {
                                self.inference_fallback_type()
                            };
                        }
                        return static_ty;
                    }
                    if property_name == "name" {
                        return self.type_ctx.string_type();
                    }
                    if property_name == "length" {
                        return self.type_ctx.number_type();
                    }
                    for prop in &class.static_properties {
                        if prop.name == property_name {
                            return prop.ty;
                        }
                    }
                    for method in &class.static_methods {
                        if method.name == property_name {
                            return method.ty;
                        }
                    }
                    if self.is_js_mode() {
                        return if self.allows_dynamic_any() {
                            self.type_ctx.any_type()
                        } else {
                            self.inference_fallback_type()
                        };
                    }
                    self.errors.push(CheckError::UndefinedMember {
                        member: property_name.clone(),
                        span: member.span,
                    });
                    return self.type_ctx.unknown_type();
                }
            }
        }

        // Get the type for property lookup.
        // For JSObject<T>, use T to keep known member checks/typing as fast as normal T.
        let jsobject_inner = self.type_ctx.jsobject_inner(lookup_object_ty);
        let obj_type = if let Some(inner) = jsobject_inner {
            self.type_ctx.get(inner).cloned()
        } else {
            self.type_ctx.get(lookup_object_ty).cloned()
        };
        let obj_type = match obj_type {
            Some(crate::parser::types::Type::Reference(type_ref)) => {
                // Resolve through symbols first (scope-aware shadowing), then named types.
                let resolved_named_ty = self
                    .symbols
                    .resolve_from_scope(&type_ref.name, self.current_scope)
                    .map(|symbol| symbol.ty)
                    .or_else(|| self.type_ctx.lookup_named_type(&type_ref.name));
                if let Some(named_ty) = resolved_named_ty {
                    match self.type_ctx.get(named_ty).cloned() {
                        Some(crate::parser::types::Type::Class(class_ty))
                            if type_ref
                                .type_args
                                .as_ref()
                                .is_some_and(|args| !args.is_empty())
                                && !class_ty.type_params.is_empty() =>
                        {
                            let args = type_ref.type_args.as_ref().expect("checked is_some");
                            let instantiated_ty = self.instantiate_class_type(&class_ty, args);
                            self.type_ctx.get(instantiated_ty).cloned()
                        }
                        Some(resolved) => Some(resolved),
                        None => Some(crate::parser::types::Type::Reference(type_ref)),
                    }
                } else {
                    Some(crate::parser::types::Type::Reference(type_ref))
                }
            }
            other => other,
        };

        // Check for built-in array methods
        if let Some(crate::parser::types::Type::Array(arr)) = &obj_type {
            let elem_ty = arr.element;
            if let Some(method_type) = self.get_array_method_type(&property_name, elem_ty) {
                return method_type;
            }
        }

        // Check for built-in string methods
        if let Some(crate::parser::types::Type::Primitive(
            crate::parser::types::PrimitiveType::String,
        )) = &obj_type
        {
            if let Some(method_type) = self.get_string_method_type(&property_name) {
                return method_type;
            }
        }

        // Check for built-in number methods
        if let Some(crate::parser::types::Type::Primitive(
            crate::parser::types::PrimitiveType::Number,
        )) = &obj_type
        {
            if let Some(method_type) = self.get_number_method_type(&property_name) {
                return method_type;
            }
        }

        if self.is_js_mode() && matches!(&obj_type, Some(crate::parser::types::Type::Function(_))) {
            return match property_name.as_str() {
                "name" => self.type_ctx.string_type(),
                "length" => self.type_ctx.number_type(),
                _ => self.type_ctx.any_type(),
            };
        }

        if self.is_js_mode() && self.type_uses_dynamic_js_member_fallback(lookup_object_ty) {
            return if self.allows_dynamic_any() {
                self.type_ctx.any_type()
            } else {
                self.inference_fallback_type()
            };
        }

        // Note: Mutex methods are now resolved via normal class method lookup from mutex.raya

        // Check for built-in Promise methods
        if let Some(crate::parser::types::Type::Task(task_ty)) = &obj_type {
            if let Some(method_type) = self.get_task_method_type(&property_name, task_ty.result) {
                return method_type;
            }
            if let Some(ty) =
                self.resolve_builtin_class_member("Promise", &property_name, member.span)
            {
                return ty;
            }
        }

        // Check for built-in RegExp methods
        if let Some(crate::parser::types::Type::RegExp) = &obj_type {
            if let Some(method_type) = self.get_regexp_method_type(&property_name) {
                return method_type;
            }
        }

        // Check for built-in Map methods
        if let Some(crate::parser::types::Type::Map(map_ty)) = &obj_type {
            if let Some(method_type) =
                self.get_map_method_type(&property_name, map_ty.key, map_ty.value)
            {
                return method_type;
            }
            if let Some(ty) = self.resolve_builtin_class_member("Map", &property_name, member.span)
            {
                return ty;
            }
        }

        // Check for built-in Set methods
        if let Some(crate::parser::types::Type::Set(set_ty)) = &obj_type {
            if let Some(method_type) = self.get_set_method_type(&property_name, set_ty.element) {
                return method_type;
            }
            if let Some(ty) = self.resolve_builtin_class_member("Set", &property_name, member.span)
            {
                return ty;
            }
        }

        // Check for built-in Buffer methods
        if let Some(crate::parser::types::Type::Class(class)) = &obj_type {
            if class.name == "Buffer" {
                if let Some(method_type) = self.get_buffer_method_type(&property_name, object_ty) {
                    return method_type;
                }
            }
        }

        // Type::Buffer comes from type annotations; resolve via class definition.
        if matches!(&obj_type, Some(crate::parser::types::Type::Buffer)) {
            if let Some(method_type) = self.get_buffer_method_type(&property_name, object_ty) {
                return method_type;
            }
            if let Some(ty) =
                self.resolve_builtin_class_member("Buffer", &property_name, member.span)
            {
                return ty;
            }
        }

        // JSON values support duck-typed member access; property type remains json.
        if matches!(&obj_type, Some(crate::parser::types::Type::Json)) {
            return self.type_ctx.json_type();
        }

        // Note: Date methods are now resolved via normal class method lookup
        // from date.raya file definition

        // Check for built-in Channel methods
        if let Some(crate::parser::types::Type::Channel(chan_ty)) = &obj_type {
            if let Some(method_type) = self.get_channel_method_type(&property_name, chan_ty.message)
            {
                return method_type;
            }
            if let Some(ty) =
                self.resolve_builtin_class_member("Channel", &property_name, member.span)
            {
                return ty;
            }
        }

        if self.is_js_mode() {
            return if self.allows_dynamic_any() {
                self.type_ctx.any_type()
            } else {
                self.inference_fallback_type()
            };
        }

        // Check for class properties and methods (including inherited ones)
        if let Some(crate::parser::types::Type::Class(class)) = &obj_type {
            // If this is a placeholder class type (empty methods), look up the symbol to get the full type
            let actual_class = if class.methods.is_empty() && class.properties.is_empty() {
                // Look up class by name in symbol table
                if let Some(symbol) = self
                    .symbols
                    .resolve_from_scope(&class.name, self.current_scope)
                {
                    if symbol.kind == SymbolKind::Class {
                        self.resolve_class_type(symbol.ty)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            let class_to_use = actual_class.as_ref().unwrap_or(class);

            // Look up the member in the class hierarchy (including parent classes)
            if let Some((ty, vis, owner_name)) =
                self.lookup_class_member_with_owner(class_to_use, &property_name)
            {
                // Visibility:
                // - private: only declaring class
                // - protected: declaring class or subclasses
                if vis == crate::parser::ast::Visibility::Private
                    && !self.is_current_class_same_or_subclass_of(&owner_name, false)
                {
                    self.errors.push(CheckError::PropertyNotFound {
                        property: format!("private member '{}'", property_name),
                        ty: owner_name,
                        span: member.span,
                    });
                    return self.type_ctx.unknown_type();
                }
                if vis == crate::parser::ast::Visibility::Protected
                    && !self.is_current_class_same_or_subclass_of(&owner_name, true)
                {
                    self.errors.push(CheckError::PropertyNotFound {
                        property: format!("protected member '{}'", property_name),
                        ty: owner_name,
                        span: member.span,
                    });
                    return self.type_ctx.unknown_type();
                }
                return ty;
            }

            // If we have a class type and the member was not found, emit an error
            // (unless the class has no properties/methods, which means it's a placeholder)
            if !class_to_use.properties.is_empty() || !class_to_use.methods.is_empty() {
                if self.is_js_mode() && self.in_assignment_lhs {
                    self.maybe_escalate_identifier_to_jsobject(&member.object, None);
                    return self.type_ctx.any_type();
                }
                if !self.is_strict_mode() {
                    let object_is_anyish = self.type_is_dynamic_anyish(object_ty);
                    let explicit_any_cast = self.is_explicit_any_cast_expr(&member.object);
                    // Dot writes on class instances require explicit dynamic opt-in.
                    if !self.in_assignment_lhs || object_is_anyish || explicit_any_cast {
                        self.maybe_escalate_identifier_to_jsobject(&member.object, None);
                        return self.type_ctx.jsobject_of(object_ty);
                    }
                }
                self.errors.push(CheckError::PropertyNotFound {
                    property: property_name.clone(),
                    ty: format!("class {}", class_to_use.name),
                    span: member.span,
                });
                return self.type_ctx.unknown_type();
            }
        }

        // Check for object type properties (from type aliases like `type Point = { x: number }`)
        if let Some(crate::parser::types::Type::Object(obj)) = &obj_type {
            for prop in &obj.properties {
                if prop.name == property_name {
                    return prop.ty;
                }
            }
            if let Some((_, sig_ty)) = obj.index_signature {
                return sig_ty;
            }
        }

        // Check for interface properties and methods (including inherited interfaces)
        if let Some(crate::parser::types::Type::Interface(interface_ty)) = &obj_type {
            if let Some(member_ty) = self.lookup_interface_member(interface_ty, &property_name) {
                return member_ty;
            }
        }

        // Handle Union types: look up member on any union variant that has it
        if let Some(crate::parser::types::Type::Union(union)) = &obj_type {
            let null_ty = self.type_ctx.null_type();
            let mut found_types = Vec::new();
            let mut object_like_variants = 0usize;
            let mut missing_on_some_object_like = false;
            for &member_id in &union.members {
                if member_id == null_ty {
                    continue;
                }
                let mut variant_is_object_like = false;
                let mut variant_has_member = false;
                if let Some(member_ty) = self.type_ctx.get(member_id).cloned() {
                    let mut pending = vec![member_ty];
                    let mut visited_named_refs = FxHashSet::default();
                    while let Some(variant_ty) = pending.pop() {
                        match variant_ty {
                            crate::parser::types::Type::Object(obj) => {
                                variant_is_object_like = true;
                                for prop in &obj.properties {
                                    if prop.name == property_name {
                                        variant_has_member = true;
                                        if !found_types.contains(&prop.ty) {
                                            found_types.push(prop.ty);
                                        }
                                    }
                                }
                                if let Some((_, sig_ty)) = obj.index_signature {
                                    variant_has_member = true;
                                    if !found_types.contains(&sig_ty) {
                                        found_types.push(sig_ty);
                                    }
                                }
                            }
                            crate::parser::types::Type::Class(class) => {
                                if class.methods.is_empty() && class.properties.is_empty() {
                                    // Placeholder class-like surface (often from alias/reference
                                    // indirection). Resolve through symbols/named types before
                                    // treating it as a concrete class variant.
                                    if let Some(resolved_ty) = self
                                        .symbols
                                        .resolve_from_scope(&class.name, self.current_scope)
                                        .map(|sym| sym.ty)
                                        .or_else(|| self.type_ctx.lookup_named_type(&class.name))
                                        .and_then(|ty_id| self.type_ctx.get(ty_id).cloned())
                                    {
                                        pending.push(resolved_ty);
                                        continue;
                                    }
                                    continue;
                                }
                                variant_is_object_like = true;
                                if let Some((ty, _vis)) =
                                    self.lookup_class_member(&class, &property_name)
                                {
                                    variant_has_member = true;
                                    if !found_types.contains(&ty) {
                                        found_types.push(ty);
                                    }
                                }
                            }
                            crate::parser::types::Type::Interface(interface_ty) => {
                                variant_is_object_like = true;
                                if let Some(ty) =
                                    self.lookup_interface_member(&interface_ty, &property_name)
                                {
                                    variant_has_member = true;
                                    if !found_types.contains(&ty) {
                                        found_types.push(ty);
                                    }
                                }
                            }
                            crate::parser::types::Type::Array(arr) => {
                                variant_is_object_like = true;
                                if let Some(ty) =
                                    self.get_array_method_type(&property_name, arr.element)
                                {
                                    variant_has_member = true;
                                    if !found_types.contains(&ty) {
                                        found_types.push(ty);
                                    }
                                }
                            }
                            crate::parser::types::Type::Map(map_ty) => {
                                variant_is_object_like = true;
                                if let Some(ty) = self.get_map_method_type(
                                    &property_name,
                                    map_ty.key,
                                    map_ty.value,
                                ) {
                                    variant_has_member = true;
                                    if !found_types.contains(&ty) {
                                        found_types.push(ty);
                                    }
                                }
                            }
                            crate::parser::types::Type::Set(set_ty) => {
                                variant_is_object_like = true;
                                if let Some(ty) =
                                    self.get_set_method_type(&property_name, set_ty.element)
                                {
                                    variant_has_member = true;
                                    if !found_types.contains(&ty) {
                                        found_types.push(ty);
                                    }
                                }
                            }
                            crate::parser::types::Type::Reference(type_ref) => {
                                if !visited_named_refs.insert(type_ref.name.clone()) {
                                    continue;
                                }
                                if let Some(named_ty) =
                                    self.type_ctx.lookup_named_type(&type_ref.name)
                                {
                                    match self.type_ctx.get(named_ty).cloned() {
                                        Some(crate::parser::types::Type::Class(class_ty))
                                            if type_ref
                                                .type_args
                                                .as_ref()
                                                .is_some_and(|args| !args.is_empty())
                                                && !class_ty.type_params.is_empty() =>
                                        {
                                            let args = type_ref
                                                .type_args
                                                .as_ref()
                                                .expect("checked is_some");
                                            let instantiated_ty =
                                                self.instantiate_class_type(&class_ty, args);
                                            if let Some(inst_ty) =
                                                self.type_ctx.get(instantiated_ty).cloned()
                                            {
                                                pending.push(inst_ty);
                                            } else {
                                                pending.push(crate::parser::types::Type::Class(
                                                    class_ty,
                                                ));
                                            }
                                        }
                                        Some(resolved) => pending.push(resolved),
                                        None => {}
                                    }
                                }
                            }
                            crate::parser::types::Type::Generic(generic) => {
                                if let Some(base_ty) = self.type_ctx.get(generic.base).cloned() {
                                    pending.push(base_ty);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                if variant_is_object_like {
                    object_like_variants += 1;
                    if !variant_has_member {
                        missing_on_some_object_like = true;
                    }
                }
            }
            if missing_on_some_object_like {
                self.errors.push(CheckError::PropertyNotFound {
                    property: property_name.clone(),
                    ty: self.format_type(object_ty),
                    span: member.span,
                });
                return self.fallback_type(
                    member.span,
                    FallbackReason::Unavoidable,
                    "member-union-partial",
                );
            }
            if found_types.len() == 1 {
                return found_types[0];
            } else if found_types.len() > 1 {
                return self.type_ctx.union_type(found_types);
            }
            if object_like_variants > 0 {
                self.errors.push(CheckError::PropertyNotFound {
                    property: property_name.clone(),
                    ty: self.format_type(object_ty),
                    span: member.span,
                });
                // Unavoidable fallback allowlist:
                // union members are object-like but none provide the requested member.
                return self.fallback_type(
                    member.span,
                    FallbackReason::Unavoidable,
                    "member-union",
                );
            }
        }

        // Handle TypeVar with constraint: delegate member access to the constraint type
        if let Some(crate::parser::types::Type::TypeVar(tv)) = &obj_type {
            if let Some(constraint_id) = tv.constraint {
                let mut object_like_constraint = false;
                let mut pending = vec![constraint_id];
                let mut visited = FxHashSet::default();

                while let Some(ty_id) = pending.pop() {
                    if !visited.insert(ty_id) {
                        continue;
                    }
                    let Some(constraint_type) = self.type_ctx.get(ty_id).cloned() else {
                        continue;
                    };

                    // Look up member on the (possibly referenced/generic) constraint type.
                    match &constraint_type {
                        crate::parser::types::Type::Object(obj) => {
                            object_like_constraint = true;
                            for prop in &obj.properties {
                                if prop.name == property_name {
                                    return prop.ty;
                                }
                            }
                            if let Some((_, sig_ty)) = obj.index_signature {
                                return sig_ty;
                            }
                        }
                        crate::parser::types::Type::Class(class) => {
                            object_like_constraint = true;
                            if let Some((ty, _vis)) =
                                self.lookup_class_member(class, &property_name)
                            {
                                return ty;
                            }
                        }
                        crate::parser::types::Type::Interface(interface_ty) => {
                            object_like_constraint = true;
                            if let Some(ty) =
                                self.lookup_interface_member(interface_ty, &property_name)
                            {
                                return ty;
                            }
                        }
                        crate::parser::types::Type::Reference(type_ref) => {
                            if let Some(named_ty) = self.type_ctx.lookup_named_type(&type_ref.name)
                            {
                                pending.push(named_ty);
                            }
                        }
                        crate::parser::types::Type::Generic(generic) => {
                            pending.push(generic.base);
                        }
                        _ => {}
                    }
                }

                if object_like_constraint {
                    self.errors.push(CheckError::PropertyNotFound {
                        property: property_name.clone(),
                        ty: self.format_type(constraint_id),
                        span: member.span,
                    });
                    // Unavoidable fallback allowlist:
                    // constrained typevar has object-like constraint but requested member is absent.
                    return self.fallback_type(
                        member.span,
                        FallbackReason::Unavoidable,
                        "member-typevar-constraint",
                    );
                }
            }
        }

        // For JSObject<T>, unknown member falls back to index-signature value type.
        if jsobject_inner.is_some()
            || matches!(
                self.type_ctx.get(object_ty),
                Some(crate::parser::types::Type::JSObject) | Some(crate::parser::types::Type::Any)
            )
        {
            return if self.allows_dynamic_any() {
                self.type_ctx.any_type()
            } else {
                self.inference_fallback_type()
            };
        }

        self.fallback_type(
            member.span,
            FallbackReason::RecoverableUnsupportedExpr,
            "member-access",
        )
    }

    fn lookup_interface_member(
        &mut self,
        interface_ty: &crate::parser::types::ty::InterfaceType,
        name: &str,
    ) -> Option<TypeId> {
        for prop in &interface_ty.properties {
            if prop.name == name {
                return Some(prop.ty);
            }
        }
        for method in &interface_ty.methods {
            if method.name == name {
                return Some(method.ty);
            }
        }
        for parent in &interface_ty.extends {
            match self.type_ctx.get(*parent).cloned() {
                Some(crate::parser::types::Type::Interface(parent_interface)) => {
                    if let Some(ty) = self.lookup_interface_member(&parent_interface, name) {
                        return Some(ty);
                    }
                }
                Some(crate::parser::types::Type::Object(obj)) => {
                    if let Some(ty) = obj.properties.iter().find(|p| p.name == name).map(|p| p.ty) {
                        return Some(ty);
                    }
                    if let Some((_, sig_ty)) = obj.index_signature {
                        return Some(sig_ty);
                    }
                }
                Some(crate::parser::types::Type::Class(class_ty)) => {
                    if let Some((ty, _)) = self.lookup_class_member(&class_ty, name) {
                        return Some(ty);
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn collect_interface_member_types(
        &mut self,
        interface_ty: &crate::parser::types::ty::InterfaceType,
        out: &mut Vec<TypeId>,
        visited_parents: &mut std::collections::HashSet<TypeId>,
    ) {
        for prop in &interface_ty.properties {
            if !out.contains(&prop.ty) {
                out.push(prop.ty);
            }
        }
        for method in &interface_ty.methods {
            if !out.contains(&method.ty) {
                out.push(method.ty);
            }
        }
        for parent in &interface_ty.extends {
            if !visited_parents.insert(*parent) {
                continue;
            }
            match self.type_ctx.get(*parent).cloned() {
                Some(crate::parser::types::Type::Interface(parent_interface)) => {
                    self.collect_interface_member_types(&parent_interface, out, visited_parents);
                }
                Some(crate::parser::types::Type::Object(obj)) => {
                    for prop in &obj.properties {
                        if !out.contains(&prop.ty) {
                            out.push(prop.ty);
                        }
                    }
                    if let Some((_, sig_ty)) = obj.index_signature {
                        if !out.contains(&sig_ty) {
                            out.push(sig_ty);
                        }
                    }
                }
                Some(crate::parser::types::Type::Class(class_ty)) => {
                    for prop in &class_ty.properties {
                        if !out.contains(&prop.ty) {
                            out.push(prop.ty);
                        }
                    }
                    for method in &class_ty.methods {
                        if !out.contains(&method.ty) {
                            out.push(method.ty);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Look up a property or method in a class hierarchy, including parent classes
    /// Create an instantiated class type by substituting type parameters with concrete types.
    /// E.g., ReadableStream<T> with [number] → ReadableStream<number> with T→number in all methods.
    fn instantiate_class_type(
        &mut self,
        class: &crate::parser::types::ty::ClassType,
        type_args: &[TypeId],
    ) -> TypeId {
        use crate::parser::types::ty::{MethodSignature, PropertySignature};
        use crate::parser::types::GenericContext;

        // Build substitution map: type_param_name → concrete type
        let mut gen_ctx = GenericContext::new(self.type_ctx);
        for (param_name, &arg_ty) in class.type_params.iter().zip(type_args.iter()) {
            gen_ctx.add_substitution(param_name.clone(), arg_ty);
        }

        // Substitute in properties
        let properties: Vec<PropertySignature> = class
            .properties
            .iter()
            .map(|prop| {
                let ty = gen_ctx.apply_substitution(prop.ty).unwrap_or(prop.ty);
                PropertySignature {
                    name: prop.name.clone(),
                    ty,
                    optional: prop.optional,
                    readonly: prop.readonly,
                    visibility: prop.visibility,
                }
            })
            .collect();

        // Substitute in methods
        let methods: Vec<MethodSignature> = class
            .methods
            .iter()
            .map(|method| {
                let ty = gen_ctx.apply_substitution(method.ty).unwrap_or(method.ty);
                MethodSignature {
                    name: method.name.clone(),
                    ty,
                    type_params: method.type_params.clone(),
                    visibility: method.visibility,
                }
            })
            .collect();

        // Create the instantiated class type (clear type_params since they're resolved)
        let instantiated = crate::parser::types::ty::ClassType {
            name: class.name.clone(),
            type_params: vec![],
            properties,
            methods,
            static_properties: class.static_properties.clone(),
            static_methods: class.static_methods.clone(),
            extends: class.extends,
            implements: class.implements.clone(),
            is_abstract: class.is_abstract,
        };

        self.type_ctx
            .intern(crate::parser::types::Type::Class(instantiated))
    }

    fn instantiate_generic_type_alias(
        &mut self,
        template_ty: TypeId,
        param_names: &[String],
        type_args: &[TypeId],
    ) -> TypeId {
        let mut gen_ctx = GenericContext::new(self.type_ctx);
        for (param_name, &arg_ty) in param_names.iter().zip(type_args.iter()) {
            gen_ctx.add_substitution(param_name.clone(), arg_ty);
        }
        gen_ctx
            .apply_substitution(template_ty)
            .unwrap_or(template_ty)
    }

    fn lookup_class_member(
        &mut self,
        class: &crate::parser::types::ty::ClassType,
        property_name: &str,
    ) -> Option<(TypeId, crate::parser::ast::Visibility)> {
        self.lookup_class_member_with_owner(class, property_name)
            .map(|(ty, vis, _)| (ty, vis))
    }

    fn lookup_class_member_with_owner(
        &mut self,
        class: &crate::parser::types::ty::ClassType,
        property_name: &str,
    ) -> Option<(TypeId, crate::parser::ast::Visibility, String)> {
        // Check own properties first
        for prop in &class.properties {
            if prop.name == property_name {
                return Some((prop.ty, prop.visibility, class.name.clone()));
            }
        }
        // Check own methods
        for method in &class.methods {
            if method.name == property_name {
                return Some((method.ty, method.visibility, class.name.clone()));
            }
        }

        // If not found and class has a parent, check parent class
        if let Some(parent_ty) = class.extends {
            if let Some(parent_class) = self.resolve_class_type(parent_ty) {
                // Recursively check parent class
                return self.lookup_class_member_with_owner(&parent_class, property_name);
            }
        }

        // Not found in class hierarchy
        None
    }

    fn is_current_class_same_or_subclass_of(
        &mut self,
        target_class_name: &str,
        allow_subclass: bool,
    ) -> bool {
        let Some(current_ty) = self.current_class_type else {
            return false;
        };
        let mut cursor = Some(current_ty);
        while let Some(ty) = cursor {
            let Some(class) = self.resolve_class_type(ty) else {
                break;
            };
            if class.name == target_class_name {
                return true;
            }
            if !allow_subclass {
                return false;
            }
            cursor = class.extends;
        }
        false
    }

    fn resolve_class_type(&mut self, ty: TypeId) -> Option<crate::parser::types::ty::ClassType> {
        use crate::parser::types::Type;
        match self.type_ctx.get(ty).cloned()? {
            Type::Class(class_ty) => Some(class_ty),
            Type::Reference(type_ref) => {
                let named_ty = self.type_ctx.lookup_named_type(&type_ref.name)?;
                let named_class = match self.type_ctx.get(named_ty).cloned()? {
                    Type::Class(class_ty) => class_ty,
                    _ => return None,
                };
                if let Some(args) = type_ref
                    .type_args
                    .as_ref()
                    .filter(|args| !args.is_empty() && !named_class.type_params.is_empty())
                {
                    let instantiated = self.instantiate_class_type(&named_class, args);
                    match self.type_ctx.get(instantiated).cloned() {
                        Some(Type::Class(class_ty)) => Some(class_ty),
                        _ => Some(named_class),
                    }
                } else {
                    Some(named_class)
                }
            }
            Type::Generic(generic) => match self.type_ctx.get(generic.base).cloned() {
                Some(Type::Class(class_ty)) if !generic.type_args.is_empty() => {
                    let instantiated = self.instantiate_class_type(&class_ty, &generic.type_args);
                    match self.type_ctx.get(instantiated).cloned() {
                        Some(Type::Class(inst)) => Some(inst),
                        _ => Some(class_ty),
                    }
                }
                Some(Type::Class(class_ty)) => Some(class_ty),
                _ => None,
            },
            _ => None,
        }
    }

    /// Resolve a member on a built-in type by looking up its source-level class definition.
    /// Built-in types (Set, Map, Channel, Task/Promise) use specialised Type variants
    /// for core method resolution, but their `.raya` class definitions may declare
    /// additional members (e.g. internal fields like `setPtr`). This resolves those.
    fn resolve_builtin_class_member(
        &mut self,
        class_name: &str,
        property_name: &str,
        span: Span,
    ) -> Option<TypeId> {
        let symbol = self
            .symbols
            .resolve_from_scope(class_name, self.current_scope)?;
        if symbol.kind != SymbolKind::Class {
            return None;
        }
        let class = match self.type_ctx.get(symbol.ty).cloned() {
            Some(crate::parser::types::Type::Class(c)) => c,
            _ => return None,
        };
        let (ty, vis, owner_name) = self.lookup_class_member_with_owner(&class, property_name)?;
        if vis == crate::parser::ast::Visibility::Private
            && !self.is_current_class_same_or_subclass_of(&owner_name, false)
        {
            self.errors.push(CheckError::PropertyNotFound {
                property: format!("private member '{}'", property_name),
                ty: owner_name,
                span,
            });
            return Some(self.type_ctx.unknown_type());
        }
        if vis == crate::parser::ast::Visibility::Protected
            && !self.is_current_class_same_or_subclass_of(&owner_name, true)
        {
            self.errors.push(CheckError::PropertyNotFound {
                property: format!("protected member '{}'", property_name),
                ty: owner_name,
                span,
            });
            return Some(self.type_ctx.unknown_type());
        }
        Some(ty)
    }

    /// Get the type of a built-in array method
    fn get_array_method_type(&mut self, method_name: &str, elem_ty: TypeId) -> Option<TypeId> {
        let number_ty = self.type_ctx.number_type();
        let boolean_ty = self.type_ctx.boolean_type();
        let void_ty = self.type_ctx.void_type();
        let string_ty = self.type_ctx.string_type();
        let array_ty = self.type_ctx.array_type(elem_ty);
        let any_ty = self.type_ctx.any_type();
        let search_elem_ty = if self.is_js_mode() { any_ty } else { elem_ty };
        let mut callback_context_params = |return_ty: TypeId| {
            if self.is_js_mode() {
                self.type_ctx.function_type_with_min_params(
                    vec![any_ty, any_ty, any_ty],
                    return_ty,
                    false,
                    1,
                )
            } else {
                self.type_ctx.function_type(vec![elem_ty], return_ty, false)
            }
        };

        match method_name {
            // push(value: T) -> number
            "push" => Some(self.type_ctx.function_type(vec![elem_ty], number_ty, false)),
            // pop() -> T
            "pop" => Some(self.type_ctx.function_type(vec![], elem_ty, false)),
            // shift() -> T
            "shift" => Some(self.type_ctx.function_type(vec![], elem_ty, false)),
            // unshift(value: T) -> number
            "unshift" => Some(self.type_ctx.function_type(vec![elem_ty], number_ty, false)),
            // indexOf(value: T, fromIndex?: number) -> number
            "indexOf" => Some(self.type_ctx.function_type_with_min_params(
                vec![search_elem_ty, number_ty],
                number_ty,
                false,
                1,
            )),
            // includes(value: T) -> boolean
            "includes" => Some(
                self.type_ctx
                    .function_type(vec![search_elem_ty], boolean_ty, false),
            ),
            // slice(start: number, end?: number) -> Array<T>
            "slice" => Some(self.type_ctx.function_type_with_min_params(
                vec![number_ty, number_ty],
                array_ty,
                false,
                1,
            )),
            // concat(other: Array<T>) -> Array<T>
            "concat" => Some(self.type_ctx.function_type(vec![array_ty], array_ty, false)),
            // join(separator: string) -> string
            "join" => Some(
                self.type_ctx
                    .function_type(vec![string_ty], string_ty, false),
            ),
            // reverse() -> Array<T>
            "reverse" => Some(self.type_ctx.function_type(vec![], array_ty, false)),
            // forEach(fn: (elem: T, index?: number, array?: T[]) => void, thisArg?) -> void
            "forEach" => {
                let callback_ty = callback_context_params(void_ty);
                Some(self.type_ctx.function_type_with_min_params(
                    vec![callback_ty, any_ty],
                    void_ty,
                    false,
                    1,
                ))
            }
            // filter(predicate: (elem: T, index?: number, array?: T[]) => boolean, thisArg?) -> Array<T>
            "filter" => {
                let predicate_ty = callback_context_params(boolean_ty);
                Some(self.type_ctx.function_type_with_min_params(
                    vec![predicate_ty, any_ty],
                    array_ty,
                    false,
                    1,
                ))
            }
            // find(predicate: (elem: T, index?: number, array?: T[]) => boolean, thisArg?) -> T | null
            "find" => {
                let predicate_ty = callback_context_params(boolean_ty);
                let null_ty = self.type_ctx.null_type();
                let nullable_elem = self.type_ctx.union_type(vec![elem_ty, null_ty]);
                Some(self.type_ctx.function_type_with_min_params(
                    vec![predicate_ty, any_ty],
                    nullable_elem,
                    false,
                    1,
                ))
            }
            // findIndex(predicate: (elem: T, index?: number, array?: T[]) => boolean, thisArg?) -> number
            "findIndex" => {
                let predicate_ty = callback_context_params(boolean_ty);
                Some(self.type_ctx.function_type_with_min_params(
                    vec![predicate_ty, any_ty],
                    number_ty,
                    false,
                    1,
                ))
            }
            // every(predicate: (elem: T, index?: number, array?: T[]) => boolean, thisArg?) -> boolean
            "every" => {
                let predicate_ty = callback_context_params(boolean_ty);
                Some(self.type_ctx.function_type_with_min_params(
                    vec![predicate_ty, any_ty],
                    boolean_ty,
                    false,
                    1,
                ))
            }
            // some(predicate: (elem: T, index?: number, array?: T[]) => boolean, thisArg?) -> boolean
            "some" => {
                let predicate_ty = callback_context_params(boolean_ty);
                Some(self.type_ctx.function_type_with_min_params(
                    vec![predicate_ty, any_ty],
                    boolean_ty,
                    false,
                    1,
                ))
            }
            // length -> number (property, not method)
            "length" => Some(number_ty),
            // lastIndexOf(value: T, fromIndex?: number) -> number
            "lastIndexOf" => Some(self.type_ctx.function_type_with_min_params(
                vec![search_elem_ty, number_ty],
                number_ty,
                false,
                1,
            )),
            // sort(compareFn?: (a: T, b: T) => number) -> Array<T>
            "sort" => {
                let compare_fn_ty =
                    self.type_ctx
                        .function_type(vec![elem_ty, elem_ty], number_ty, false);
                Some(self.type_ctx.function_type_with_min_params(
                    vec![compare_fn_ty],
                    array_ty,
                    false,
                    0,
                ))
            }
            // map(fn: (elem: T) => T) -> Array<T> (simplified - without generic U)
            // map<U>(fn: (elem: T) => U) -> Array<U>
            "map" => {
                let map_result_ty = self.type_ctx.type_variable("__array_map_u");
                let callback_ty = if self.is_js_mode() {
                    self.type_ctx.function_type_with_min_params(
                        vec![any_ty, any_ty, any_ty],
                        map_result_ty,
                        false,
                        1,
                    )
                } else {
                    self.type_ctx
                        .function_type(vec![elem_ty], map_result_ty, false)
                };
                let mapped_array_ty = self.type_ctx.array_type(map_result_ty);
                Some(self.type_ctx.function_type_with_min_params(
                    vec![callback_ty, any_ty],
                    mapped_array_ty,
                    false,
                    1,
                ))
            }
            // reduce<U>(fn: (acc: U, elem: T) => U, initial: U) -> U
            "reduce" => {
                let acc_ty = self.type_ctx.type_variable("__array_reduce_u");
                let callback_ty = if self.is_js_mode() {
                    self.type_ctx.function_type_with_min_params(
                        vec![any_ty, any_ty, any_ty, any_ty],
                        acc_ty,
                        false,
                        2,
                    )
                } else {
                    self.type_ctx
                        .function_type(vec![acc_ty, elem_ty], acc_ty, false)
                };
                Some(
                    self.type_ctx
                        .function_type(vec![callback_ty, acc_ty], acc_ty, false),
                )
            }
            // fill(value: T, start?: number, end?: number) -> Array<T>
            "fill" => Some(self.type_ctx.function_type_with_min_params(
                vec![elem_ty, number_ty, number_ty],
                array_ty,
                false,
                1,
            )),
            // flat() -> Array<T> (simplified - single level flatten)
            "flat" => Some(self.type_ctx.function_type(vec![], array_ty, false)),
            // splice(start: number, deleteCount?: number, ...items: T[]) -> Array<T>
            // Note: simplified type signature - actual splice takes variable arguments
            "splice" => Some(self.type_ctx.function_type_with_min_params(
                vec![number_ty, number_ty, elem_ty, elem_ty, elem_ty, elem_ty],
                array_ty,
                false,
                1,
            )),
            _ => None,
        }
    }

    /// Get the type of a built-in string method
    fn get_string_method_type(&mut self, method_name: &str) -> Option<TypeId> {
        let number_ty = self.type_ctx.number_type();
        let string_ty = self.type_ctx.string_type();
        let boolean_ty = self.type_ctx.boolean_type();

        match method_name {
            // length property
            "length" => Some(number_ty),
            // charAt(index: number) -> string
            "charAt" => Some(
                self.type_ctx
                    .function_type(vec![number_ty], string_ty, false),
            ),
            // substring(start: number, end?: number) -> string
            "substring" => Some(self.type_ctx.function_type_with_min_params(
                vec![number_ty, number_ty],
                string_ty,
                false,
                1,
            )),
            // slice(start: number, end?: number) -> string
            "slice" => Some(self.type_ctx.function_type_with_min_params(
                vec![number_ty, number_ty],
                string_ty,
                false,
                1,
            )),
            // toUpperCase() -> string
            "toUpperCase" => Some(self.type_ctx.function_type(vec![], string_ty, false)),
            // toLowerCase() -> string
            "toLowerCase" => Some(self.type_ctx.function_type(vec![], string_ty, false)),
            // trim() -> string
            "trim" => Some(self.type_ctx.function_type(vec![], string_ty, false)),
            // indexOf(searchStr: string, fromIndex?: number) -> number
            "indexOf" => Some(self.type_ctx.function_type_with_min_params(
                vec![string_ty, number_ty],
                number_ty,
                false,
                1,
            )),
            // includes(searchStr: string) -> boolean
            "includes" => Some(
                self.type_ctx
                    .function_type(vec![string_ty], boolean_ty, false),
            ),
            // startsWith(prefix: string) -> boolean
            "startsWith" => Some(
                self.type_ctx
                    .function_type(vec![string_ty], boolean_ty, false),
            ),
            // endsWith(suffix: string) -> boolean
            "endsWith" => Some(
                self.type_ctx
                    .function_type(vec![string_ty], boolean_ty, false),
            ),
            // split(separator: string | RegExp, limit?: number) -> Array<string>
            "split" => {
                let regexp_ty = self.type_ctx.regexp_type();
                let search_ty = self.type_ctx.union_type(vec![string_ty, regexp_ty]);
                let arr_ty = self.type_ctx.array_type(string_ty);
                Some(self.type_ctx.function_type_with_min_params(
                    vec![search_ty, number_ty],
                    arr_ty,
                    false,
                    1,
                ))
            }
            // replace(search: string | RegExp, replacement: string) -> string
            "replace" => {
                let regexp_ty = self.type_ctx.regexp_type();
                let search_ty = self.type_ctx.union_type(vec![string_ty, regexp_ty]);
                Some(
                    self.type_ctx
                        .function_type(vec![search_ty, string_ty], string_ty, false),
                )
            }
            // repeat(count: number = 1) -> string
            "repeat" => Some(self.type_ctx.function_type_with_min_params(
                vec![number_ty],
                string_ty,
                false,
                0,
            )),
            // charCodeAt(index: number) -> number
            "charCodeAt" => Some(
                self.type_ctx
                    .function_type(vec![number_ty], number_ty, false),
            ),
            // lastIndexOf(searchStr: string, fromIndex?: number) -> number
            "lastIndexOf" => Some(self.type_ctx.function_type_with_min_params(
                vec![string_ty, number_ty],
                number_ty,
                false,
                1,
            )),
            // trimStart() -> string
            "trimStart" => Some(self.type_ctx.function_type(vec![], string_ty, false)),
            // trimEnd() -> string
            "trimEnd" => Some(self.type_ctx.function_type(vec![], string_ty, false)),
            // padStart(length: number, pad?: string) -> string
            "padStart" => Some(self.type_ctx.function_type_with_min_params(
                vec![number_ty, string_ty],
                string_ty,
                false,
                1,
            )),
            // padEnd(length: number, pad?: string) -> string
            "padEnd" => Some(self.type_ctx.function_type_with_min_params(
                vec![number_ty, string_ty],
                string_ty,
                false,
                1,
            )),
            // match(pattern: RegExp) -> string[] | null
            // Returns array of matches or null if no match
            "match" => {
                let regexp_ty = self.type_ctx.regexp_type();
                let arr_ty = self.type_ctx.array_type(string_ty);
                let null_ty = self.type_ctx.null_type();
                let result_ty = self.type_ctx.union_type(vec![arr_ty, null_ty]);
                Some(
                    self.type_ctx
                        .function_type(vec![regexp_ty], result_ty, false),
                )
            }
            // matchAll(pattern: RegExp) -> Array<string[]>
            "matchAll" => {
                let regexp_ty = self.type_ctx.regexp_type();
                let inner_arr_ty = self.type_ctx.array_type(string_ty);
                let arr_ty = self.type_ctx.array_type(inner_arr_ty);
                Some(self.type_ctx.function_type(vec![regexp_ty], arr_ty, false))
            }
            // search(pattern: RegExp) -> number
            // Returns index of first match, or -1 if no match
            "search" => {
                let regexp_ty = self.type_ctx.regexp_type();
                Some(
                    self.type_ctx
                        .function_type(vec![regexp_ty], number_ty, false),
                )
            }
            // replaceWith(pattern: RegExp, replacer: (match: Array<string | number>) => string) -> string
            // Callback receives [matchedText, index, ...groups] for each match
            "replaceWith" => {
                let regexp_ty = self.type_ctx.regexp_type();
                // Callback type: (Array<string | number>) => string
                let union_elem_ty = self.type_ctx.union_type(vec![string_ty, number_ty]);
                let match_arr_ty = self.type_ctx.array_type(union_elem_ty);
                let callback_ty = self
                    .type_ctx
                    .function_type(vec![match_arr_ty], string_ty, false);
                Some(
                    self.type_ctx
                        .function_type(vec![regexp_ty, callback_ty], string_ty, false),
                )
            }
            _ => None,
        }
    }

    /// Get the type of a built-in number method
    fn get_number_method_type(&mut self, method_name: &str) -> Option<TypeId> {
        let number_ty = self.type_ctx.number_type();
        let string_ty = self.type_ctx.string_type();

        match method_name {
            // toFixed(digits: number = 0) -> string
            "toFixed" => Some(self.type_ctx.function_type_with_min_params(
                vec![number_ty],
                string_ty,
                false,
                0,
            )),
            // toPrecision(precision?: number) -> string
            "toPrecision" => Some(self.type_ctx.function_type_with_min_params(
                vec![number_ty],
                string_ty,
                false,
                0,
            )),
            // toString(radix: number = 10) -> string
            "toString" => Some(self.type_ctx.function_type_with_min_params(
                vec![number_ty],
                string_ty,
                false,
                0,
            )),
            _ => None,
        }
    }

    // Note: Mutex methods are now resolved from mutex.raya class definition
    // (get_mutex_method_type removed - no longer needed)

    /// Get the type of a built-in Promise method
    fn get_task_method_type(&mut self, method_name: &str, result_ty: TypeId) -> Option<TypeId> {
        let void_ty = self.type_ctx.void_type();
        let bool_ty = self.type_ctx.boolean_type();
        let unknown_ty = self.type_ctx.unknown_type();
        let promise_unknown_ty = self.type_ctx.task_type(unknown_ty);
        let promise_result_ty = self.type_ctx.task_type(result_ty);

        match method_name {
            // cancel() -> void
            "cancel" => Some(self.type_ctx.function_type(vec![], void_ty, false)),
            // isDone() -> boolean
            "isDone" => Some(self.type_ctx.function_type(vec![], bool_ty, false)),
            // isCancelled() -> boolean
            "isCancelled" => Some(self.type_ctx.function_type(vec![], bool_ty, false)),
            // then<U>(onFulfilled: (value: T) => U, onRejected?: (reason) => U) -> Promise<U>
            "then" => {
                let on_fulfilled_ty =
                    self.type_ctx
                        .function_type(vec![result_ty], unknown_ty, false);
                // Use `never` for handler input to keep parameter position maximally permissive
                // under contravariant function parameter checking.
                let reason_ty = self.type_ctx.never_type();
                let on_rejected_ty =
                    self.type_ctx
                        .function_type(vec![reason_ty], unknown_ty, false);
                Some(self.type_ctx.function_type_with_min_params(
                    vec![on_fulfilled_ty, on_rejected_ty],
                    promise_unknown_ty,
                    false,
                    1,
                ))
            }
            // catch<U>(onRejected: (reason: Object) => U) -> Promise<T | U>
            "catch" => {
                // Use `never` for handler input to keep parameter position maximally permissive
                // under contravariant function parameter checking.
                let reason_ty = self.type_ctx.never_type();
                let on_rejected_ty =
                    self.type_ctx
                        .function_type(vec![reason_ty], unknown_ty, false);
                Some(
                    self.type_ctx
                        .function_type(vec![on_rejected_ty], promise_result_ty, false),
                )
            }
            // finally(onFinally: () => void) -> Promise<T>
            "finally" => {
                let on_finally_ty = self.type_ctx.function_type(vec![], void_ty, false);
                Some(
                    self.type_ctx
                        .function_type(vec![on_finally_ty], promise_result_ty, false),
                )
            }
            _ => None,
        }
    }

    /// Get the type of a built-in RegExp method
    fn get_regexp_method_type(&mut self, method_name: &str) -> Option<TypeId> {
        let string_ty = self.type_ctx.string_type();
        let boolean_ty = self.type_ctx.boolean_type();
        let number_ty = self.type_ctx.number_type();

        match method_name {
            // test(str: string) -> boolean
            "test" => Some(
                self.type_ctx
                    .function_type(vec![string_ty], boolean_ty, false),
            ),
            // exec(str: string) -> (string | number)[] | null
            // Runtime returns [matchText, matchIndex, ...captureGroups].
            "exec" => {
                let null_ty = self.type_ctx.null_type();
                let num_or_str = self.type_ctx.union_type(vec![string_ty, number_ty]);
                let match_array_ty = self.type_ctx.array_type(num_or_str);
                let result_ty = self.type_ctx.union_type(vec![match_array_ty, null_ty]);
                Some(
                    self.type_ctx
                        .function_type(vec![string_ty], result_ty, false),
                )
            }
            // execAll(str: string) -> string[]
            "execAll" => {
                let array_ty = self.type_ctx.array_type(string_ty);
                Some(
                    self.type_ctx
                        .function_type(vec![string_ty], array_ty, false),
                )
            }
            // replace(str: string, replacement: string) -> string
            "replace" => Some(self.type_ctx.function_type(
                vec![string_ty, string_ty],
                string_ty,
                false,
            )),
            // split(str: string, limit?: number) -> string[]
            "split" => {
                let array_ty = self.type_ctx.array_type(string_ty);
                Some(self.type_ctx.function_type_with_min_params(
                    vec![string_ty, number_ty],
                    array_ty,
                    false,
                    1,
                ))
            }
            // source property -> string
            "source" => Some(string_ty),
            // flags property -> string
            "flags" => Some(string_ty),
            // global property -> boolean
            "global" => Some(boolean_ty),
            // ignoreCase property -> boolean
            "ignoreCase" => Some(boolean_ty),
            // multiline property -> boolean
            "multiline" => Some(boolean_ty),
            // lastIndex property -> number (legacy, Raya RegExp is stateless)
            "lastIndex" => Some(number_ty),
            // replaceWith(str: string, replacer: (match: string) => string) -> string
            "replaceWith" => {
                let replacer_ty = self
                    .type_ctx
                    .function_type(vec![string_ty], string_ty, false);
                Some(
                    self.type_ctx
                        .function_type(vec![string_ty, replacer_ty], string_ty, false),
                )
            }
            // dotAll property -> boolean
            "dotAll" => Some(boolean_ty),
            // unicode property -> boolean
            "unicode" => Some(boolean_ty),
            _ => None,
        }
    }

    /// Get the type of a built-in Map method
    fn get_map_method_type(
        &mut self,
        method_name: &str,
        key_ty: TypeId,
        value_ty: TypeId,
    ) -> Option<TypeId> {
        let number_ty = self.type_ctx.number_type();
        let boolean_ty = self.type_ctx.boolean_type();
        let void_ty = self.type_ctx.void_type();
        let null_ty = self.type_ctx.null_type();

        match method_name {
            // size -> number
            "size" => Some(number_ty),
            // get(key: K) -> V | null
            "get" => {
                let result_ty = self.type_ctx.union_type(vec![value_ty, null_ty]);
                Some(self.type_ctx.function_type(vec![key_ty], result_ty, false))
            }
            // set(key: K, value: V) -> void
            "set" => Some(
                self.type_ctx
                    .function_type(vec![key_ty, value_ty], void_ty, false),
            ),
            // has(key: K) -> boolean
            "has" => Some(self.type_ctx.function_type(vec![key_ty], boolean_ty, false)),
            // delete(key: K) -> boolean
            "delete" => Some(self.type_ctx.function_type(vec![key_ty], boolean_ty, false)),
            // clear() -> void
            "clear" => Some(self.type_ctx.function_type(vec![], void_ty, false)),
            // keys() -> Array<K>
            "keys" => {
                let array_ty = self.type_ctx.array_type(key_ty);
                Some(self.type_ctx.function_type(vec![], array_ty, false))
            }
            // values() -> Array<V>
            "values" => {
                let array_ty = self.type_ctx.array_type(value_ty);
                Some(self.type_ctx.function_type(vec![], array_ty, false))
            }
            // entries() -> Array<[K, V]>
            "entries" => {
                let tuple_ty = self.type_ctx.tuple_type(vec![key_ty, value_ty]);
                let array_ty = self.type_ctx.array_type(tuple_ty);
                Some(self.type_ctx.function_type(vec![], array_ty, false))
            }
            // forEach(fn: (value: V, key: K) => void) -> void
            "forEach" => {
                let callback_ty =
                    self.type_ctx
                        .function_type(vec![value_ty, key_ty], void_ty, false);
                Some(
                    self.type_ctx
                        .function_type(vec![callback_ty], void_ty, false),
                )
            }
            _ => None,
        }
    }

    /// Get the type of a built-in Set method
    fn get_set_method_type(&mut self, method_name: &str, element_ty: TypeId) -> Option<TypeId> {
        let number_ty = self.type_ctx.number_type();
        let boolean_ty = self.type_ctx.boolean_type();
        let void_ty = self.type_ctx.void_type();

        match method_name {
            // size -> number
            "size" => Some(number_ty),
            // add(value: T) -> void
            "add" => Some(
                self.type_ctx
                    .function_type(vec![element_ty], void_ty, false),
            ),
            // has(value: T) -> boolean
            "has" => Some(
                self.type_ctx
                    .function_type(vec![element_ty], boolean_ty, false),
            ),
            // delete(value: T) -> boolean
            "delete" => Some(
                self.type_ctx
                    .function_type(vec![element_ty], boolean_ty, false),
            ),
            // clear() -> void
            "clear" => Some(self.type_ctx.function_type(vec![], void_ty, false)),
            // keys()/values() -> Array<T>
            "keys" | "values" => {
                let array_ty = self.type_ctx.array_type(element_ty);
                Some(self.type_ctx.function_type(vec![], array_ty, false))
            }
            // entries() -> Array<[T, T]>
            "entries" => {
                let tuple_ty = self.type_ctx.tuple_type(vec![element_ty, element_ty]);
                let array_ty = self.type_ctx.array_type(tuple_ty);
                Some(self.type_ctx.function_type(vec![], array_ty, false))
            }
            // forEach(fn: (value: T) => void) -> void
            "forEach" => {
                let callback_ty = self
                    .type_ctx
                    .function_type(vec![element_ty], void_ty, false);
                Some(
                    self.type_ctx
                        .function_type(vec![callback_ty], void_ty, false),
                )
            }
            // union(other: Set<T>) -> Set<T>
            "union" => {
                let set_ty = self.type_ctx.set_type_with(element_ty);
                Some(self.type_ctx.function_type(vec![set_ty], set_ty, false))
            }
            // intersection(other: Set<T>) -> Set<T>
            "intersection" => {
                let set_ty = self.type_ctx.set_type_with(element_ty);
                Some(self.type_ctx.function_type(vec![set_ty], set_ty, false))
            }
            // difference(other: Set<T>) -> Set<T>
            "difference" => {
                let set_ty = self.type_ctx.set_type_with(element_ty);
                Some(self.type_ctx.function_type(vec![set_ty], set_ty, false))
            }
            _ => None,
        }
    }

    /// Get the type of a built-in Buffer method
    fn get_buffer_method_type(&mut self, method_name: &str, buffer_ty: TypeId) -> Option<TypeId> {
        let number_ty = self.type_ctx.number_type();
        let string_ty = self.type_ctx.string_type();
        let void_ty = self.type_ctx.void_type();

        match method_name {
            // length -> number
            "length" => Some(number_ty),
            // getByte(index: number) -> number
            "getByte" => Some(
                self.type_ctx
                    .function_type(vec![number_ty], number_ty, false),
            ),
            // setByte(index: number, value: number) -> void
            "setByte" => Some(self.type_ctx.function_type(
                vec![number_ty, number_ty],
                void_ty,
                false,
            )),
            // getInt32(index: number) -> number
            "getInt32" => Some(
                self.type_ctx
                    .function_type(vec![number_ty], number_ty, false),
            ),
            // setInt32(index: number, value: number) -> void
            "setInt32" => Some(self.type_ctx.function_type(
                vec![number_ty, number_ty],
                void_ty,
                false,
            )),
            // getFloat64(index: number) -> number
            "getFloat64" => Some(
                self.type_ctx
                    .function_type(vec![number_ty], number_ty, false),
            ),
            // setFloat64(index: number, value: number) -> void
            "setFloat64" => Some(self.type_ctx.function_type(
                vec![number_ty, number_ty],
                void_ty,
                false,
            )),
            // slice(start: number, end?: number) -> Buffer
            "slice" => Some(self.type_ctx.function_type_with_min_params(
                vec![number_ty, number_ty],
                buffer_ty,
                false,
                1,
            )),
            // copy(target: Buffer, targetStart?: number, sourceStart?: number, sourceEnd?: number) -> number
            "copy" => Some(self.type_ctx.function_type_with_min_params(
                vec![buffer_ty, number_ty, number_ty, number_ty],
                number_ty,
                false,
                1,
            )),
            // toString(encoding?: string) -> string
            "toString" => Some(self.type_ctx.function_type_with_min_params(
                vec![string_ty],
                string_ty,
                false,
                0,
            )),
            _ => None,
        }
    }

    // Note: get_date_method_type removed
    // Date methods are now resolved from their .raya class definitions

    /// Get the type of a built-in Channel method
    fn get_channel_method_type(&mut self, method_name: &str, message_ty: TypeId) -> Option<TypeId> {
        let number_ty = self.type_ctx.number_type();
        let boolean_ty = self.type_ctx.boolean_type();
        let void_ty = self.type_ctx.void_type();
        let null_ty = self.type_ctx.null_type();

        match method_name {
            // send(value: T) -> void
            "send" => Some(
                self.type_ctx
                    .function_type(vec![message_ty], void_ty, false),
            ),
            // receive() -> T
            "receive" => Some(self.type_ctx.function_type(vec![], message_ty, false)),
            // trySend(value: T) -> boolean
            "trySend" => Some(
                self.type_ctx
                    .function_type(vec![message_ty], boolean_ty, false),
            ),
            // tryReceive() -> T | null
            "tryReceive" => {
                let result_ty = self.type_ctx.union_type(vec![message_ty, null_ty]);
                Some(self.type_ctx.function_type(vec![], result_ty, false))
            }
            // close() -> void
            "close" => Some(self.type_ctx.function_type(vec![], void_ty, false)),
            // isClosed() -> boolean
            "isClosed" => Some(self.type_ctx.function_type(vec![], boolean_ty, false)),
            // length() -> number
            "length" => Some(self.type_ctx.function_type(vec![], number_ty, false)),
            // capacity() -> number
            "capacity" => Some(self.type_ctx.function_type(vec![], number_ty, false)),
            _ => None,
        }
    }

    /// Check array literal
    fn check_array(&mut self, arr: &ArrayExpression) -> TypeId {
        if arr.elements.is_empty() {
            if self.is_js_mode() {
                let any_ty = self.type_ctx.any_type();
                return self.type_ctx.array_type(any_ty);
            }
            // Empty array - infer as never[]
            // never is the bottom type, so never[] <: T[] for any T
            // This allows empty arrays to be assigned to any typed array
            let never = self.type_ctx.never_type();
            return self.type_ctx.array_type(never);
        }

        let has_spread = arr
            .elements
            .iter()
            .flatten()
            .any(|elem| matches!(elem, ArrayElement::Spread(_)));
        let has_holes = arr.elements.iter().any(|elem| elem.is_none());

        // Collect all distinct element types to compute a unified element type
        let mut elem_types = Vec::new();
        let mut ordered_elem_types = Vec::new();
        for elem in arr.elements.iter().flatten() {
            let elem_ty = match elem {
                ArrayElement::Expression(expr) => self.check_expr(expr),
                ArrayElement::Spread(expr) => {
                    let spread_ty = self.check_expr(expr);
                    if let Some(crate::parser::types::Type::Array(arr_ty)) =
                        self.type_ctx.get(spread_ty).cloned()
                    {
                        arr_ty.element
                    } else {
                        spread_ty
                    }
                }
            };
            ordered_elem_types.push(elem_ty);
            if !elem_types.contains(&elem_ty) {
                elem_types.push(elem_ty);
            }
        }

        // Heterogeneous literals without spread/hole are tuple-like by default.
        // This preserves positional typing for declarations like `let x: [int, string] = [1, "a"]`.
        if !has_spread && !has_holes && elem_types.len() > 1 {
            return self.type_ctx.tuple_type(ordered_elem_types);
        }

        // If all elements are the same type, use that; otherwise create a union
        let unified_ty = if elem_types.len() == 1 {
            elem_types[0]
        } else if elem_types.is_empty() {
            self.type_ctx.unknown_type()
        } else {
            self.type_ctx.union_type(elem_types)
        };

        self.type_ctx.array_type(unified_ty)
    }

    fn insert_or_override_property(
        properties: &mut Vec<crate::parser::types::ty::PropertySignature>,
        prop: crate::parser::types::ty::PropertySignature,
    ) {
        if let Some(existing) = properties
            .iter_mut()
            .find(|existing| existing.name == prop.name)
        {
            *existing = prop;
        } else {
            properties.push(prop);
        }
    }

    fn spread_properties_from_type(
        &mut self,
        ty: TypeId,
    ) -> Option<Vec<crate::parser::types::ty::PropertySignature>> {
        use crate::parser::types::Type;

        match self.type_ctx.get(ty).cloned()? {
            Type::Object(obj) => Some(obj.properties),
            Type::Class(class) => Some(class.properties),
            Type::TypeVar(tv) => tv
                .constraint
                .and_then(|constraint| self.spread_properties_from_type(constraint)),
            Type::Union(union) => {
                let mut merged: Vec<crate::parser::types::ty::PropertySignature> = Vec::new();
                for member in union.members {
                    let member_props = self.spread_properties_from_type(member)?;
                    for prop in member_props {
                        if let Some(existing) = merged
                            .iter_mut()
                            .find(|existing| existing.name == prop.name)
                        {
                            if existing.ty != prop.ty {
                                existing.ty = self.type_ctx.union_type(vec![existing.ty, prop.ty]);
                            }
                        } else {
                            merged.push(prop);
                        }
                    }
                }
                Some(merged)
            }
            _ => None,
        }
    }

    /// Check object literal
    fn check_object(&mut self, obj: &ObjectExpression) -> TypeId {
        use crate::parser::types::ty::{ObjectType, PropertySignature};

        let mut properties = Vec::new();
        for prop in &obj.properties {
            match prop {
                ObjectProperty::Property(p) => {
                    let name = match &p.key {
                        PropertyKey::Identifier(ident) => self.resolve(ident.name),
                        PropertyKey::StringLiteral(lit) => self.resolve(lit.value),
                        PropertyKey::IntLiteral(lit) => lit.value.to_string(),
                        PropertyKey::Computed(_) => continue, // Skip computed keys
                    };
                    let value_ty = self.check_expr(&p.value);
                    Self::insert_or_override_property(
                        &mut properties,
                        PropertySignature {
                            name,
                            ty: value_ty,
                            optional: false,
                            readonly: false,
                            visibility: Default::default(),
                        },
                    );
                }
                ObjectProperty::Spread(spread) => {
                    let spread_ty = self.check_expr(&spread.argument);
                    if let Some(spread_props) = self.spread_properties_from_type(spread_ty) {
                        for spread_prop in spread_props {
                            Self::insert_or_override_property(&mut properties, spread_prop);
                        }
                    } else {
                        self.errors.push(CheckError::TypeMismatch {
                            expected: "object-like type".to_string(),
                            actual: self.format_type(spread_ty),
                            span: spread.span,
                            note: Some(
                                "Object spread requires an object, class instance, or compatible union"
                                    .to_string(),
                            ),
                        });
                    }
                }
            }
        }

        self.type_ctx
            .intern(crate::parser::types::Type::Object(ObjectType {
                properties,
                index_signature: None,
                call_signatures: vec![],
                construct_signatures: vec![],
            }))
    }

    /// Check conditional (ternary) expression
    fn check_conditional(&mut self, cond: &ConditionalExpression) -> TypeId {
        // Check test is boolean
        let test_ty = self.check_expr(&cond.test);
        if !self.is_js_mode() {
            let bool_ty = self.type_ctx.boolean_type();
            self.check_assignable(test_ty, bool_ty, *cond.test.span());
        }

        // Check both branches
        let then_ty = self.check_expr(&cond.consequent);
        let else_ty = self.check_expr(&cond.alternate);

        // Return union of both types
        self.type_ctx.union_type(vec![then_ty, else_ty])
    }

    /// Check assignment expression
    fn check_assignment(&mut self, assign: &AssignmentExpression) -> TypeId {
        // Check for readonly property assignment
        if let Expression::Member(member) = &*assign.left {
            let is_this = matches!(&*member.object, Expression::This(_));
            // Allow this.field = value inside constructors
            if !(is_this && self.in_constructor) {
                let prev = self.in_assignment_lhs;
                self.in_assignment_lhs = true;
                let object_ty = self.check_expr(&member.object);
                self.in_assignment_lhs = prev;
                let property_name = self.resolve(member.property.name);
                if self.is_readonly_property(object_ty, &property_name) {
                    self.errors.push(CheckError::ReadonlyAssignment {
                        property: property_name,
                        span: member.span,
                    });
                }
            }
        }

        // Check const reassignment for simple identifiers
        if let Expression::Identifier(ident) = &*assign.left {
            let name = self.resolve(ident.name);
            if let Some(symbol) = self.symbols.resolve_from_scope(&name, self.current_scope) {
                if symbol.flags.is_const {
                    self.errors.push(CheckError::ConstReassignment {
                        name: name.clone(),
                        span: assign.span,
                    });
                }
            }
        }

        // For simple identifier assignments, use the declared type (not narrowed)
        // so that reassignment back to the original wider type is allowed.
        // e.g., inside `while (val != null)`, `val = ch.tryReceive()` should work
        // even though `val` was narrowed from `T | null` to `T`.
        let (left_ty, clear_var) = if let Expression::Identifier(ident) = &*assign.left {
            let name = self.resolve(ident.name);
            let declared_ty = self
                .get_var_declared_type(&name)
                .unwrap_or_else(|| self.check_expr(&assign.left));
            (declared_ty, Some(name))
        } else {
            let is_member_lhs = matches!(&*assign.left, Expression::Member(_));
            let prev = self.in_assignment_lhs;
            if is_member_lhs {
                self.in_assignment_lhs = true;
            }
            let ty = self.check_expr(&assign.left);
            self.in_assignment_lhs = prev;
            (ty, None)
        };
        // Evaluate RHS before clearing narrowing so `current = current.next`
        // can use the narrowed type of `current` when evaluating `current.next`.
        let right_ty = self.check_expr(&assign.right);

        if let Expression::Member(member) = &*assign.left {
            let field_name = self.resolve(member.property.name);
            let lhs_object_ty = self.check_expr(&member.object);
            if self.type_is_dynamic_anyish(lhs_object_ty)
                || self.is_explicit_any_cast_expr(&member.object)
                || self.is_js_mode()
            {
                self.widen_identifier_with_monkeypatch_field(&member.object, &field_name, right_ty);
            }
        }

        // Clear narrowing after RHS evaluation since the variable is being reassigned.
        if let Some(name) = clear_var {
            self.type_env.remove(&name);
        }
        let mut target_ty = left_ty;

        // Deep fix: support mutable `let x = null; x = value;` by widening the
        // inferred declared type to `null | T` for unannotated variables.
        // This preserves strictness for annotated variables and const bindings.
        if let Expression::Identifier(ident) = &*assign.left {
            let name = self.resolve(ident.name);
            let resolved_symbol = self.symbols.resolve_from_scope(&name, self.current_scope);
            let resolved_scope_id = resolved_symbol.map(|symbol| symbol.scope_id.0);
            let symbol_ty = resolved_symbol
                .map(|symbol| symbol.ty)
                .unwrap_or_else(|| self.inference_fallback_type());
            let is_const = resolved_symbol.is_some_and(|symbol| symbol.flags.is_const);
            let is_dynamic_seed = resolved_symbol
                .map(|symbol| self.is_dynamic_seed_type(symbol.ty))
                .unwrap_or(self.is_js_mode());
            let inferred_key = resolved_scope_id
                .map(|scope_id| (scope_id, name.clone()))
                .or_else(|| {
                    self.scope_stack.iter().rev().find_map(|scope_id| {
                        let key = (scope_id.0, name.clone());
                        self.inferred_var_types.contains_key(&key).then_some(key)
                    })
                })
                .unwrap_or((self.current_scope.0, name.clone()));
            let inferred_current = self.inferred_var_types.get(&inferred_key).copied();
            let null_ty = self.type_ctx.null_type();
            let debug_js_assign = std::env::var("RAYA_DEBUG_JS_ASSIGN").is_ok();
            if debug_js_assign {
                eprintln!(
                    "[js-assign] name={} scope={} symbol_ty={} inferred_current={:?} right_ty={} is_dynamic_seed={} const={}",
                    name,
                    inferred_key.0,
                    self.format_type(symbol_ty),
                    inferred_current.map(|ty| self.format_type(ty)),
                    self.format_type(right_ty),
                    is_dynamic_seed,
                    is_const
                );
            }

            // Variable has no explicit annotation (binder stores dynamic seed type).
            if is_dynamic_seed && !is_const {
                match inferred_current {
                    // `let x = null; x = <T>;` => widen declaration to `null | T`
                    Some(inferred_ty) if inferred_ty == null_ty && right_ty != null_ty => {
                        let widened = self.type_ctx.union_type(vec![inferred_ty, right_ty]);
                        self.inferred_var_types
                            .insert(inferred_key.clone(), widened);
                        target_ty = widened;
                    }
                    // Node-compat auto-widen inference across contradictory assignments.
                    Some(inferred_ty) if self.is_js_mode() => {
                        // Use strict assignability here so non-strict coercions
                        // (e.g. number -> string) don't suppress union widening.
                        // In JS mode, widen whenever the new assignment is not
                        // assignable to the current inferred declaration type,
                        // even if the reverse direction happens to be assignable
                        // via structural width subtyping.
                        let mut strict_assign_ctx =
                            AssignabilityContext::with_strict_mode(self.type_ctx, true);
                        if !strict_assign_ctx.is_assignable(right_ty, inferred_ty) {
                            let widened = self.join_inferred_types(inferred_ty, right_ty);
                            if debug_js_assign {
                                eprintln!(
                                    "[js-assign] widening name={} from {} to {}",
                                    name,
                                    self.format_type(inferred_ty),
                                    self.format_type(widened)
                                );
                            }
                            self.inferred_var_types
                                .insert(inferred_key.clone(), widened);
                            target_ty = widened;
                        }
                    }
                    // `let x; x = <T>;` => first concrete assignment sets inferred declaration.
                    None => {
                        self.inferred_var_types
                            .insert(inferred_key.clone(), right_ty);
                        target_ty = right_ty;
                    }
                    _ => {}
                }
            }

            // Assignment updates current flow type (used by branch merges).
            self.set_unbound_method_var_state(&name, &assign.right);
            self.set_constructible_var_state(&name, &assign.right, right_ty);
            self.type_env.set(name, right_ty);
        } else if let Expression::Index(idx) = &*assign.left {
            self.maybe_escalate_identifier_to_jsobject(&idx.object, Some(&idx.index));
        }

        let skip_assignability = self.is_js_mode()
            && matches!(&*assign.left, Expression::Member(_) | Expression::Index(_));
        if !skip_assignability {
            self.check_assignable(right_ty, target_ty, *assign.right.span());
        }

        target_ty
    }

    /// Check if source type is assignable to target type
    fn check_assignable(&mut self, source: TypeId, target: TypeId, span: crate::parser::Span) {
        let mut assign_ctx = self.make_assignability_ctx();
        if !assign_ctx.is_assignable(source, target) {
            if std::env::var_os("RAYA_DEBUG_CHECK_ASSIGNABLE").is_some() {
                use crate::parser::types::Type;
                eprintln!(
                    "[check-assignable] mismatch at line {} col {}: source={} target={}",
                    span.line,
                    span.column,
                    self.format_type(source),
                    self.format_type(target),
                );
                if let Some(Type::Function(f)) = self.type_ctx.get(source) {
                    eprintln!(
                        "  source fn: params={:?} min={} rest={:?} async={}",
                        f.params, f.min_params, f.rest_param, f.is_async
                    );
                }
                if let Some(Type::Function(f)) = self.type_ctx.get(target) {
                    eprintln!(
                        "  target fn: params={:?} min={} rest={:?} async={}",
                        f.params, f.min_params, f.rest_param, f.is_async
                    );
                }
                if let Some(Type::TypeVar(tv)) = self.type_ctx.get(target) {
                    eprintln!(
                        "  target typevar: name={} constraint={:?}",
                        tv.name, tv.constraint
                    );
                }
            }
            self.errors.push(CheckError::TypeMismatch {
                expected: self.format_type(target),
                actual: self.format_type(source),
                span,
                note: None,
            });
        }
    }

    /// Check if a property is readonly on a given type
    fn is_readonly_property(&self, ty: TypeId, property_name: &str) -> bool {
        if let Some(resolved) = self.type_ctx.get(ty) {
            match resolved {
                crate::parser::types::Type::Class(class) => {
                    for prop in &class.properties {
                        if prop.name == property_name {
                            return prop.readonly;
                        }
                    }
                }
                crate::parser::types::Type::Object(obj) => {
                    for prop in &obj.properties {
                        if prop.name == property_name {
                            return prop.readonly;
                        }
                    }
                }
                crate::parser::types::Type::Interface(iface) => {
                    for prop in &iface.properties {
                        if prop.name == property_name {
                            return prop.readonly;
                        }
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// Format a type for display in error messages
    fn format_type(&self, ty: TypeId) -> String {
        self.type_ctx.display(ty)
    }

    /// Get type of expression (for external use)
    pub fn get_expr_type(&self, expr: &Expression) -> Option<TypeId> {
        let expr_id = expr as *const _ as usize;
        self.expr_types.get(&expr_id).copied()
    }

    /// Resolve a type annotation to a TypeId
    fn resolve_type_annotation(&mut self, ty_annot: &TypeAnnotation) -> TypeId {
        let ty = self.resolve_type(&ty_annot.ty);
        let ann_id = ty_annot as *const _ as usize;
        self.type_annotation_types.insert(ann_id, ty);
        ty
    }

    /// Resolve a type AST node to a TypeId
    fn resolve_type(&mut self, ty: &crate::parser::ast::Type) -> TypeId {
        use crate::parser::ast::Type as AstType;

        match ty {
            AstType::Primitive(prim) => self.resolve_primitive(*prim),

            AstType::Reference(type_ref) => {
                // Check if it's a user-defined type or type parameter
                let name = self.resolve(type_ref.name.name);
                if name == "any" {
                    if !self.allows_explicit_any() {
                        self.errors.push(CheckError::StrictAnyForbidden {
                            span: type_ref.name.span,
                        });
                        return self.type_ctx.unknown_type();
                    }
                    return self.type_ctx.any_type();
                }

                // Handle built-in generic types
                use crate::parser::TypeContext as TC;
                if name == TC::ARRAY_TYPE_NAME {
                    if let Some(ref type_args) = type_ref.type_args {
                        if type_args.len() == 1 {
                            let elem_ty = self.resolve_type_annotation(&type_args[0]);
                            return self.type_ctx.array_type(elem_ty);
                        }
                    }
                    let actual = type_ref.type_args.as_ref().map_or(0, |args| args.len());
                    self.errors.push(CheckError::InvalidTypeReferenceArity {
                        name: name.clone(),
                        expected: 1,
                        actual,
                        span: type_ref.name.span,
                    });
                    return self.fallback_type(
                        type_ref.name.span,
                        FallbackReason::RecoverableUnsupportedExpr,
                        "type-reference-array-arity",
                    );
                }

                // Handle Promise<T>/Promise<T> for async functions
                if name == TC::PROMISE_TYPE_NAME {
                    if let Some(ref type_args) = type_ref.type_args {
                        if type_args.len() == 1 {
                            let result_ty = self.resolve_type_annotation(&type_args[0]);
                            return self.type_ctx.task_type(result_ty);
                        }
                    }
                    let actual = type_ref.type_args.as_ref().map_or(0, |args| args.len());
                    self.errors.push(CheckError::InvalidTypeReferenceArity {
                        name: name.clone(),
                        expected: 1,
                        actual,
                        span: type_ref.name.span,
                    });
                    return self.fallback_type(
                        type_ref.name.span,
                        FallbackReason::RecoverableUnsupportedExpr,
                        "type-reference-promise-arity",
                    );
                }

                // Handle built-in types
                // Note: Mutex is now a normal class from mutex.raya
                if name == TC::REGEXP_TYPE_NAME {
                    return self.type_ctx.regexp_type();
                }
                if name == TC::CHANNEL_TYPE_NAME {
                    if let Some(ref type_args) = type_ref.type_args {
                        if type_args.len() != 1 {
                            self.errors.push(CheckError::InvalidTypeReferenceArity {
                                name: name.clone(),
                                expected: 1,
                                actual: type_args.len(),
                                span: type_ref.name.span,
                            });
                            return self.fallback_type(
                                type_ref.name.span,
                                FallbackReason::RecoverableUnsupportedExpr,
                                "type-reference-channel-arity",
                            );
                        }
                        let msg_ty = self.resolve_type_annotation(&type_args[0]);
                        return self.type_ctx.channel_type_with(msg_ty);
                    }
                    return self.type_ctx.channel_type();
                }
                if name == TC::MAP_TYPE_NAME {
                    if let Some(ref type_args) = type_ref.type_args {
                        if type_args.len() != 2 {
                            self.errors.push(CheckError::InvalidTypeReferenceArity {
                                name: name.clone(),
                                expected: 2,
                                actual: type_args.len(),
                                span: type_ref.name.span,
                            });
                            return self.fallback_type(
                                type_ref.name.span,
                                FallbackReason::RecoverableUnsupportedExpr,
                                "type-reference-map-arity",
                            );
                        }
                        let key_ty = self.resolve_type_annotation(&type_args[0]);
                        let value_ty = self.resolve_type_annotation(&type_args[1]);
                        return self.type_ctx.map_type_with(key_ty, value_ty);
                    }
                    return self.type_ctx.map_type();
                }
                if name == TC::SET_TYPE_NAME {
                    if let Some(ref type_args) = type_ref.type_args {
                        if type_args.len() != 1 {
                            self.errors.push(CheckError::InvalidTypeReferenceArity {
                                name: name.clone(),
                                expected: 1,
                                actual: type_args.len(),
                                span: type_ref.name.span,
                            });
                            return self.fallback_type(
                                type_ref.name.span,
                                FallbackReason::RecoverableUnsupportedExpr,
                                "type-reference-set-arity",
                            );
                        }
                        let elem_ty = self.resolve_type_annotation(&type_args[0]);
                        return self.type_ctx.set_type_with(elem_ty);
                    }
                    return self.type_ctx.set_type();
                }
                if name == "Record" {
                    if let Some(ref type_args) = type_ref.type_args {
                        if type_args.len() == 2 {
                            let value_ty = self.resolve_type_annotation(&type_args[1]);
                            let object_type = crate::parser::types::ty::ObjectType {
                                properties: vec![],
                                index_signature: Some(("[key]".to_string(), value_ty)),
                                call_signatures: vec![],
                                construct_signatures: vec![],
                            };
                            return self
                                .type_ctx
                                .intern(crate::parser::types::Type::Object(object_type));
                        }
                    }
                    let actual = type_ref.type_args.as_ref().map_or(0, |args| args.len());
                    self.errors.push(CheckError::InvalidTypeReferenceArity {
                        name: name.clone(),
                        expected: 2,
                        actual,
                        span: type_ref.name.span,
                    });
                    return self.fallback_type(
                        type_ref.name.span,
                        FallbackReason::RecoverableUnsupportedExpr,
                        "type-reference-record-arity",
                    );
                }
                // Note: Date and Buffer are now normal classes, looked up from symbol table

                // Check method-level type parameters (e.g. K, U from generic methods)
                if let Some(&type_var) = self.method_type_params.get(&name) {
                    return type_var;
                }

                if let Some(symbol) = self.symbols.resolve_from_scope(&name, self.current_scope) {
                    if let Some(ref type_args) = type_ref.type_args {
                        if !type_args.is_empty() {
                            let resolved_args: Vec<TypeId> = type_args
                                .iter()
                                .map(|arg| self.resolve_type_annotation(arg))
                                .collect();

                            if symbol.kind == SymbolKind::TypeAlias {
                                if let Some(param_names) =
                                    self.symbols.generic_type_alias_params(&name)
                                {
                                    if param_names.len() != resolved_args.len() {
                                        self.errors.push(CheckError::InvalidTypeReferenceArity {
                                            name: name.clone(),
                                            expected: param_names.len(),
                                            actual: resolved_args.len(),
                                            span: type_ref.name.span,
                                        });
                                        return self.fallback_type(
                                            type_ref.name.span,
                                            FallbackReason::RecoverableUnsupportedExpr,
                                            "type-reference-alias-arity",
                                        );
                                    }
                                    return self.instantiate_generic_type_alias(
                                        symbol.ty,
                                        param_names,
                                        &resolved_args,
                                    );
                                }
                            }

                            if let Some(crate::parser::types::Type::Class(class)) =
                                self.type_ctx.get(symbol.ty).cloned()
                            {
                                if class.type_params.len() == resolved_args.len() {
                                    return self.instantiate_class_type(&class, &resolved_args);
                                }
                            }
                        }
                    }
                    symbol.ty
                } else if let Some(named_ty) = self.type_ctx.lookup_named_type(&name) {
                    named_ty
                } else {
                    // Type not found - return unknown
                    self.type_ctx.unknown_type()
                }
            }

            AstType::Array(arr) => {
                let elem_ty = self.resolve_type_annotation(&arr.element_type);
                self.type_ctx.array_type(elem_ty)
            }

            AstType::Tuple(tuple) => {
                let elem_tys: Vec<_> = tuple
                    .element_types
                    .iter()
                    .map(|e| self.resolve_type_annotation(e))
                    .collect();
                self.type_ctx.tuple_type(elem_tys)
            }

            AstType::Union(union) => {
                let member_tys: Vec<_> = union
                    .types
                    .iter()
                    .map(|t| self.resolve_type_annotation(t))
                    .collect();
                self.type_ctx.union_type(member_tys)
            }

            AstType::Intersection(intersection) => {
                // Merge constituent types into a single Object type
                let mut merged_properties = Vec::new();
                let mut index_signature: Option<(String, TypeId)> = None;
                let mut call_signatures: Vec<TypeId> = Vec::new();
                let mut construct_signatures: Vec<TypeId> = Vec::new();
                for ty_annot in &intersection.types {
                    let ty_id = self.resolve_type_annotation(ty_annot);
                    match self.type_ctx.get(ty_id).cloned() {
                        Some(crate::parser::types::Type::Object(obj)) => {
                            for prop in &obj.properties {
                                if !merged_properties.iter().any(
                                    |p: &crate::parser::types::ty::PropertySignature| {
                                        p.name == prop.name
                                    },
                                ) {
                                    merged_properties.push(prop.clone());
                                }
                            }
                            if index_signature.is_none() {
                                index_signature = obj.index_signature;
                            }
                            for sig in obj.call_signatures {
                                if !call_signatures.contains(&sig) {
                                    call_signatures.push(sig);
                                }
                            }
                            for sig in obj.construct_signatures {
                                if !construct_signatures.contains(&sig) {
                                    construct_signatures.push(sig);
                                }
                            }
                        }
                        Some(crate::parser::types::Type::Interface(iface)) => {
                            for prop in &iface.properties {
                                if !merged_properties.iter().any(
                                    |p: &crate::parser::types::ty::PropertySignature| {
                                        p.name == prop.name
                                    },
                                ) {
                                    merged_properties.push(prop.clone());
                                }
                            }
                            for method in &iface.methods {
                                if !merged_properties.iter().any(
                                    |p: &crate::parser::types::ty::PropertySignature| {
                                        p.name == method.name
                                    },
                                ) {
                                    merged_properties.push(
                                        crate::parser::types::ty::PropertySignature {
                                            name: method.name.clone(),
                                            ty: method.ty,
                                            optional: false,
                                            readonly: false,
                                            visibility: crate::parser::ast::Visibility::Public,
                                        },
                                    );
                                }
                            }
                            for sig in iface.call_signatures {
                                if !call_signatures.contains(&sig) {
                                    call_signatures.push(sig);
                                }
                            }
                            for sig in iface.construct_signatures {
                                if !construct_signatures.contains(&sig) {
                                    construct_signatures.push(sig);
                                }
                            }
                        }
                        Some(crate::parser::types::Type::Class(class_ty)) => {
                            for prop in &class_ty.properties {
                                if prop.visibility != crate::parser::ast::Visibility::Public {
                                    continue;
                                }
                                if !merged_properties.iter().any(
                                    |p: &crate::parser::types::ty::PropertySignature| {
                                        p.name == prop.name
                                    },
                                ) {
                                    merged_properties.push(prop.clone());
                                }
                            }
                            for method in &class_ty.methods {
                                if method.visibility != crate::parser::ast::Visibility::Public {
                                    continue;
                                }
                                if !merged_properties.iter().any(
                                    |p: &crate::parser::types::ty::PropertySignature| {
                                        p.name == method.name
                                    },
                                ) {
                                    merged_properties.push(
                                        crate::parser::types::ty::PropertySignature {
                                            name: method.name.clone(),
                                            ty: method.ty,
                                            optional: false,
                                            readonly: false,
                                            visibility: crate::parser::ast::Visibility::Public,
                                        },
                                    );
                                }
                            }
                        }
                        _ => {}
                    }
                }
                self.type_ctx.intern(crate::parser::types::Type::Object(
                    crate::parser::types::ty::ObjectType {
                        properties: merged_properties,
                        index_signature,
                        call_signatures,
                        construct_signatures,
                    },
                ))
            }

            AstType::Function(func) => {
                let mut param_tys = Vec::new();
                let mut rest_param = None;
                let mut min_params = 0usize;

                for p in &func.params {
                    let p_ty = self.resolve_type_annotation(&p.ty);
                    if p.is_rest {
                        rest_param = Some(p_ty);
                    } else {
                        if !p.optional {
                            min_params += 1;
                        }
                        param_tys.push(p_ty);
                    }
                }

                let return_ty = self.resolve_type_annotation(&func.return_type);
                self.type_ctx
                    .function_type_with_rest(param_tys, return_ty, false, min_params, rest_param)
            }

            AstType::Object(obj) => {
                // Build an object type from the type annotation
                let mut properties = Vec::new();
                let mut index_signature: Option<(String, TypeId)> = None;
                let mut call_signatures: Vec<TypeId> = Vec::new();
                let mut construct_signatures: Vec<TypeId> = Vec::new();
                for member in &obj.members {
                    match member {
                        crate::parser::ast::ObjectTypeMember::Property(prop) => {
                            let prop_name = self.interner.resolve(prop.name.name).to_string();
                            let prop_ty = self.resolve_type_annotation(&prop.ty);
                            properties.push(crate::parser::types::ty::PropertySignature {
                                name: prop_name,
                                ty: prop_ty,
                                optional: prop.optional,
                                readonly: prop.readonly,
                                visibility: crate::parser::ast::Visibility::Public,
                            });
                        }
                        crate::parser::ast::ObjectTypeMember::Method(method) => {
                            let mut param_tys = Vec::new();
                            let mut rest_param = None;
                            let mut min_params = 0usize;
                            for param in &method.params {
                                let param_ty = self.resolve_type_annotation(&param.ty);
                                if param.is_rest {
                                    rest_param = Some(param_ty);
                                } else {
                                    if !param.optional {
                                        min_params += 1;
                                    }
                                    param_tys.push(param_ty);
                                }
                            }
                            let return_ty = self.resolve_type_annotation(&method.return_type);
                            let method_ty = self.type_ctx.function_type_with_rest(
                                param_tys, return_ty, false, min_params, rest_param,
                            );
                            properties.push(crate::parser::types::ty::PropertySignature {
                                name: self.interner.resolve(method.name.name).to_string(),
                                ty: method_ty,
                                optional: method.optional,
                                readonly: false,
                                visibility: crate::parser::ast::Visibility::Public,
                            });
                        }
                        crate::parser::ast::ObjectTypeMember::IndexSignature(index) => {
                            let key_name = self.interner.resolve(index.key_name.name).to_string();
                            let value_ty = self.resolve_type_annotation(&index.value_type);
                            index_signature = Some((key_name, value_ty));
                        }
                        crate::parser::ast::ObjectTypeMember::CallSignature(call_sig) => {
                            let mut param_tys = Vec::new();
                            let mut rest_param = None;
                            let mut min_params = 0usize;
                            for param in &call_sig.params {
                                let param_ty = self.resolve_type_annotation(&param.ty);
                                if param.is_rest {
                                    rest_param = Some(param_ty);
                                } else {
                                    if !param.optional {
                                        min_params += 1;
                                    }
                                    param_tys.push(param_ty);
                                }
                            }
                            let return_ty = self.resolve_type_annotation(&call_sig.return_type);
                            call_signatures.push(self.type_ctx.function_type_with_rest(
                                param_tys, return_ty, false, min_params, rest_param,
                            ));
                        }
                        crate::parser::ast::ObjectTypeMember::ConstructSignature(ctor_sig) => {
                            let mut param_tys = Vec::new();
                            let mut rest_param = None;
                            let mut min_params = 0usize;
                            for param in &ctor_sig.params {
                                let param_ty = self.resolve_type_annotation(&param.ty);
                                if param.is_rest {
                                    rest_param = Some(param_ty);
                                } else {
                                    if !param.optional {
                                        min_params += 1;
                                    }
                                    param_tys.push(param_ty);
                                }
                            }
                            let return_ty = self.resolve_type_annotation(&ctor_sig.return_type);
                            construct_signatures.push(self.type_ctx.function_type_with_rest(
                                param_tys, return_ty, false, min_params, rest_param,
                            ));
                        }
                    }
                }
                self.type_ctx.intern(crate::parser::types::Type::Object(
                    crate::parser::types::ty::ObjectType {
                        properties,
                        index_signature,
                        call_signatures,
                        construct_signatures,
                    },
                ))
            }

            AstType::Keyof(keyof_ty) => {
                let target_ty = self.resolve_type_annotation(&keyof_ty.target);
                match self.type_ctx.get(target_ty).cloned() {
                    Some(crate::parser::types::Type::Object(obj)) => {
                        let members: Vec<TypeId> = obj
                            .properties
                            .iter()
                            .map(|p| self.type_ctx.string_literal(p.name.clone()))
                            .collect();
                        if members.is_empty() {
                            self.type_ctx.string_type()
                        } else {
                            self.type_ctx.union_type(members)
                        }
                    }
                    Some(crate::parser::types::Type::Class(class)) => {
                        let members: Vec<TypeId> = class
                            .properties
                            .iter()
                            .map(|p| self.type_ctx.string_literal(p.name.clone()))
                            .collect();
                        if members.is_empty() {
                            self.type_ctx.string_type()
                        } else {
                            self.type_ctx.union_type(members)
                        }
                    }
                    Some(crate::parser::types::Type::TypeVar(tv)) => {
                        if let Some(constraint) = tv.constraint {
                            match self.type_ctx.get(constraint).cloned() {
                                Some(crate::parser::types::Type::Object(obj)) => {
                                    let members: Vec<TypeId> = obj
                                        .properties
                                        .iter()
                                        .map(|p| self.type_ctx.string_literal(p.name.clone()))
                                        .collect();
                                    if members.is_empty() {
                                        self.type_ctx.string_type()
                                    } else {
                                        self.type_ctx.union_type(members)
                                    }
                                }
                                Some(crate::parser::types::Type::Class(class)) => {
                                    let members: Vec<TypeId> = class
                                        .properties
                                        .iter()
                                        .map(|p| self.type_ctx.string_literal(p.name.clone()))
                                        .collect();
                                    if members.is_empty() {
                                        self.type_ctx.string_type()
                                    } else {
                                        self.type_ctx.union_type(members)
                                    }
                                }
                                _ => self.type_ctx.string_type(),
                            }
                        } else {
                            self.type_ctx.string_type()
                        }
                    }
                    _ => self.type_ctx.keyof_type(target_ty),
                }
            }

            AstType::IndexedAccess(indexed) => {
                let object_ty = self.resolve_type_annotation(&indexed.object);
                let index_ty = self.resolve_type_annotation(&indexed.index);

                let prop_for_key =
                    |obj: &crate::parser::types::ty::ObjectType, key: &str| -> Option<TypeId> {
                        obj.properties.iter().find(|p| p.name == key).map(|p| p.ty)
                    };

                let object_data = self.type_ctx.get(object_ty).cloned();
                let index_data = self.type_ctx.get(index_ty).cloned();

                if matches!(object_data, Some(crate::parser::types::Type::TypeVar(_)))
                    || matches!(index_data, Some(crate::parser::types::Type::TypeVar(_)))
                {
                    return self.type_ctx.indexed_access_type(object_ty, index_ty);
                }

                let object_data = match object_data {
                    Some(crate::parser::types::Type::TypeVar(tv)) => tv
                        .constraint
                        .and_then(|c| self.type_ctx.get(c).cloned())
                        .or(Some(crate::parser::types::Type::TypeVar(tv))),
                    other => other,
                };

                let index_data = match index_data {
                    Some(crate::parser::types::Type::TypeVar(tv)) => tv
                        .constraint
                        .and_then(|c| self.type_ctx.get(c).cloned())
                        .or(Some(crate::parser::types::Type::TypeVar(tv))),
                    other => other,
                };

                match (object_data, index_data) {
                    (
                        Some(crate::parser::types::Type::Object(obj)),
                        Some(crate::parser::types::Type::StringLiteral(s)),
                    ) => prop_for_key(&obj, &s)
                        .or(obj.index_signature.map(|(_, ty)| ty))
                        .unwrap_or_else(|| self.type_ctx.unknown_type()),
                    (
                        Some(crate::parser::types::Type::Object(obj)),
                        Some(crate::parser::types::Type::Union(u)),
                    ) => {
                        let mut out = Vec::new();
                        for member in &u.members {
                            if let Some(crate::parser::types::Type::StringLiteral(s)) =
                                self.type_ctx.get(*member).cloned()
                            {
                                if let Some(ty) = prop_for_key(&obj, &s) {
                                    out.push(ty);
                                }
                            }
                        }
                        if let Some((_, sig_ty)) = obj.index_signature {
                            out.push(sig_ty);
                        }
                        if out.is_empty() {
                            self.type_ctx.unknown_type()
                        } else {
                            self.type_ctx.union_type(out)
                        }
                    }
                    (
                        Some(crate::parser::types::Type::Tuple(t)),
                        Some(crate::parser::types::Type::NumberLiteral(n)),
                    ) => {
                        let idx = n as usize;
                        if idx < t.elements.len() {
                            t.elements[idx]
                        } else {
                            self.type_ctx.unknown_type()
                        }
                    }
                    (
                        Some(crate::parser::types::Type::Object(obj)),
                        Some(crate::parser::types::Type::Primitive(
                            crate::parser::types::PrimitiveType::String,
                        )),
                    ) => {
                        let mut out = Vec::new();
                        for p in &obj.properties {
                            out.push(p.ty);
                        }
                        if let Some((_, sig_ty)) = obj.index_signature {
                            out.push(sig_ty);
                        }
                        if out.is_empty() {
                            self.type_ctx.unknown_type()
                        } else {
                            self.type_ctx.union_type(out)
                        }
                    }
                    (
                        Some(crate::parser::types::Type::Object(obj)),
                        Some(crate::parser::types::Type::Primitive(
                            crate::parser::types::PrimitiveType::Number,
                        )),
                    )
                    | (
                        Some(crate::parser::types::Type::Object(obj)),
                        Some(crate::parser::types::Type::Primitive(
                            crate::parser::types::PrimitiveType::Int,
                        )),
                    )
                    | (
                        Some(crate::parser::types::Type::Object(obj)),
                        Some(crate::parser::types::Type::NumberLiteral(_)),
                    ) => {
                        if let Some((_, sig_ty)) = obj.index_signature {
                            sig_ty
                        } else {
                            self.type_ctx.unknown_type()
                        }
                    }
                    _ => self.type_ctx.indexed_access_type(object_ty, index_ty),
                }
            }

            AstType::Typeof(_) => {
                // typeof types are resolved during type checking
                self.type_ctx.unknown_type()
            }

            AstType::StringLiteral(s) => self
                .type_ctx
                .string_literal(self.interner.resolve(*s).to_string()),

            AstType::NumberLiteral(n) => self.type_ctx.number_literal(*n),

            AstType::BooleanLiteral(b) => self.type_ctx.boolean_literal(*b),

            AstType::Parenthesized(inner) => self.resolve_type_annotation(inner),
        }
    }

    /// Resolve a primitive type to TypeId
    fn resolve_primitive(&mut self, prim: crate::parser::ast::PrimitiveType) -> TypeId {
        use crate::parser::ast::PrimitiveType as AstPrim;

        match prim {
            AstPrim::Number => self.type_ctx.number_type(),
            AstPrim::Int => self.type_ctx.int_type(),
            AstPrim::String => self.type_ctx.string_type(),
            AstPrim::Boolean => self.type_ctx.boolean_type(),
            AstPrim::Null => self.type_ctx.null_type(),
            AstPrim::Void => self.type_ctx.void_type(),
        }
    }

    // ========================================================================
    // Decorator Type Checking
    // ========================================================================

    /// Check decorators on a class declaration
    fn check_class_decorators(&mut self, class: &crate::parser::ast::ClassDecl) {
        for decorator in &class.decorators {
            self.check_class_decorator(decorator, class);
        }
    }

    /// Check a single class decorator
    fn check_class_decorator(
        &mut self,
        decorator: &crate::parser::ast::Decorator,
        _class: &crate::parser::ast::ClassDecl,
    ) {
        // Get the type of the decorator expression
        let decorator_ty = self.check_expr(&decorator.expression);

        // Check if it's a function type
        let func_ty_opt = self.type_ctx.get(decorator_ty).cloned();

        match func_ty_opt {
            Some(crate::parser::types::Type::Function(func)) => {
                // Class decorator should take 1 parameter (the class)
                // and return the class type or void
                if func.params.len() != 1 {
                    self.errors.push(CheckError::InvalidDecorator {
                        ty: self.type_ctx.display(decorator_ty),
                        expected: "ClassDecorator<T> = (target: Class<T>) => Class<T> | void"
                            .to_string(),
                        span: decorator.span,
                    });
                }
                // Return type should be void or a class type
                // For now, we accept any return type as the type system
                // will validate at call site
            }
            Some(_) | None => {
                // Decorator expression must resolve to a callable decorator.
                // Call expressions are only valid if their return type is a function.
                self.errors.push(CheckError::InvalidDecorator {
                    ty: self.type_ctx.display(decorator_ty),
                    expected: "ClassDecorator<T> or decorator factory returning ClassDecorator<T>"
                        .to_string(),
                    span: decorator.span,
                });
            }
        }
    }

    /// Check decorators on a method declaration
    fn check_method_decorators(
        &mut self,
        method: &crate::parser::ast::MethodDecl,
        method_ty: TypeId,
    ) {
        for decorator in &method.decorators {
            self.check_method_decorator(decorator, method_ty);
        }
    }

    /// Check a single method decorator
    ///
    /// Raya supports two method decorator styles:
    /// 1. Metadata style: (target: unknown, methodName: string) => void
    /// 2. Type-constrained style: (method: F) => F (where F is a specific function type)
    ///
    /// The type-constrained style allows decorators to constrain which methods they
    /// can be applied to based on the method's signature.
    fn check_method_decorator(
        &mut self,
        decorator: &crate::parser::ast::Decorator,
        method_ty: TypeId,
    ) {
        // Get the type of the decorator expression
        let decorator_ty = self.check_expr(&decorator.expression);

        // Check if it's a function type
        let func_ty_opt = self.type_ctx.get(decorator_ty).cloned();

        match func_ty_opt {
            Some(crate::parser::types::Type::Function(func)) => {
                let str_ty = self.type_ctx.string_type();
                let void_ty = self.type_ctx.void_type();

                // Check for metadata-style method decorator: (target: unknown, methodName: string) => void
                if func.params.len() == 2 {
                    let mut assign_ctx = self.make_assignability_ctx();
                    if assign_ctx.is_assignable(str_ty, func.params[1])
                        && assign_ctx.is_assignable(func.return_type, void_ty)
                    {
                        // Valid metadata-style decorator - no method type constraint
                        return;
                    }
                }

                // Check for type-constrained decorator: (method: F) => F
                if func.params.len() == 1 {
                    let param_ty = func.params[0];

                    // Check if the parameter is a function type (type-constrained decorator)
                    if let Some(crate::parser::types::Type::Function(_)) =
                        self.type_ctx.get(param_ty)
                    {
                        // This is a type-constrained decorator
                        // Check if the method type is assignable to the parameter type
                        let mut assign_ctx = self.make_assignability_ctx();
                        if !assign_ctx.is_assignable(method_ty, param_ty) {
                            self.errors.push(CheckError::DecoratorSignatureMismatch {
                                expected_signature: self.type_ctx.display(param_ty),
                                actual_signature: self.type_ctx.display(method_ty),
                                span: decorator.span,
                            });
                            return;
                        }

                        // The return type should also match the method type
                        if !assign_ctx.is_assignable(func.return_type, method_ty)
                            && !assign_ctx.is_assignable(method_ty, func.return_type)
                        {
                            self.errors.push(CheckError::DecoratorReturnMismatch {
                                expected: self.type_ctx.display(method_ty),
                                actual: self.type_ctx.display(func.return_type),
                                span: decorator.span,
                            });
                        }
                    }
                }

                // Other function signatures are valid (e.g., field decorators applied to methods by mistake
                // will be caught elsewhere, or custom decorator patterns)
            }
            Some(_) | None => {
                self.errors.push(CheckError::InvalidDecorator {
                    ty: self.type_ctx.display(decorator_ty),
                    expected: "MethodDecorator or decorator factory returning MethodDecorator"
                        .to_string(),
                    span: decorator.span,
                });
            }
        }
    }

    /// Check decorators on a field declaration
    fn check_field_decorators(&mut self, field: &crate::parser::ast::FieldDecl) {
        for decorator in &field.decorators {
            self.check_field_decorator(decorator);
        }
    }

    /// Check a single field decorator
    ///
    /// Field decorators have the signature: (target: T, fieldName: string) => void
    fn check_field_decorator(&mut self, decorator: &crate::parser::ast::Decorator) {
        // Get the type of the decorator expression
        let decorator_ty = self.check_expr(&decorator.expression);

        // Check if it's a function type
        let func_ty_opt = self.type_ctx.get(decorator_ty).cloned();

        match func_ty_opt {
            Some(crate::parser::types::Type::Function(func)) => {
                // Field decorator should take 2 parameters (target, fieldName)
                if func.params.len() != 2 {
                    self.errors.push(CheckError::InvalidDecorator {
                        ty: self.type_ctx.display(decorator_ty),
                        expected: "FieldDecorator<T> = (target: T, fieldName: string) => void"
                            .to_string(),
                        span: decorator.span,
                    });
                    return;
                }

                // Second parameter should be string
                let string_ty = self.type_ctx.string_type();
                let mut assign_ctx = self.make_assignability_ctx();
                if !assign_ctx.is_assignable(string_ty, func.params[1]) {
                    self.errors.push(CheckError::InvalidDecorator {
                        ty: self.type_ctx.display(decorator_ty),
                        expected: "FieldDecorator<T> = (target: T, fieldName: string) => void"
                            .to_string(),
                        span: decorator.span,
                    });
                }

                // Return type should be void
                let void_ty = self.type_ctx.void_type();
                if func.return_type != void_ty {
                    self.errors.push(CheckError::DecoratorReturnMismatch {
                        expected: "void".to_string(),
                        actual: self.type_ctx.display(func.return_type),
                        span: decorator.span,
                    });
                }
            }
            Some(_) | None => {
                self.errors.push(CheckError::InvalidDecorator {
                    ty: self.type_ctx.display(decorator_ty),
                    expected: "FieldDecorator<T> or decorator factory returning FieldDecorator<T>"
                        .to_string(),
                    span: decorator.span,
                });
            }
        }
    }

    /// Check decorators on a parameter
    fn check_parameter_decorators(&mut self, param: &crate::parser::ast::Parameter) {
        for decorator in &param.decorators {
            self.check_parameter_decorator(decorator);
        }
    }

    /// Check a single parameter decorator
    ///
    /// Parameter decorators have the signature:
    /// (target: T, methodName: string, parameterIndex: number) => void
    fn check_parameter_decorator(&mut self, decorator: &crate::parser::ast::Decorator) {
        // Get the type of the decorator expression
        let decorator_ty = self.check_expr(&decorator.expression);

        // Check if it's a function type
        let func_ty_opt = self.type_ctx.get(decorator_ty).cloned();

        match func_ty_opt {
            Some(crate::parser::types::Type::Function(func)) => {
                // Parameter decorator should take 3 parameters
                if func.params.len() != 3 {
                    self.errors.push(CheckError::InvalidDecorator {
                        ty: self.type_ctx.display(decorator_ty),
                        expected: "ParameterDecorator<T> = (target: T, methodName: string, parameterIndex: number) => void".to_string(),
                        span: decorator.span,
                    });
                    return;
                }

                // Second parameter should be string
                let string_ty = self.type_ctx.string_type();
                let number_ty = self.type_ctx.number_type();
                let second_ok = {
                    let mut assign_ctx = self.make_assignability_ctx();
                    assign_ctx.is_assignable(string_ty, func.params[1])
                };
                if !second_ok {
                    self.errors.push(CheckError::InvalidDecorator {
                        ty: self.type_ctx.display(decorator_ty),
                        expected: "ParameterDecorator<T> - second param should be string"
                            .to_string(),
                        span: decorator.span,
                    });
                }

                // Third parameter should be number
                let third_ok = {
                    let mut assign_ctx = self.make_assignability_ctx();
                    assign_ctx.is_assignable(number_ty, func.params[2])
                };
                if !third_ok {
                    self.errors.push(CheckError::InvalidDecorator {
                        ty: self.type_ctx.display(decorator_ty),
                        expected: "ParameterDecorator<T> - third param should be number"
                            .to_string(),
                        span: decorator.span,
                    });
                }

                // Return type should be void
                let void_ty = self.type_ctx.void_type();
                if func.return_type != void_ty {
                    self.errors.push(CheckError::DecoratorReturnMismatch {
                        expected: "void".to_string(),
                        actual: self.type_ctx.display(func.return_type),
                        span: decorator.span,
                    });
                }
            }
            Some(_) | None => {
                self.errors.push(CheckError::InvalidDecorator {
                    ty: self.type_ctx.display(decorator_ty),
                    expected:
                        "ParameterDecorator<T> or decorator factory returning ParameterDecorator<T>"
                            .to_string(),
                    span: decorator.span,
                });
            }
        }
    }

    /// Build a function type for a method (used for decorator checking)
    fn build_method_type(&mut self, method: &crate::parser::ast::MethodDecl) -> TypeId {
        // Collect parameter types
        let param_types: Vec<TypeId> = method
            .params
            .iter()
            .map(|p| {
                if let Some(ref ann) = p.type_annotation {
                    self.resolve_type_annotation(ann)
                } else {
                    self.type_ctx.unknown_type()
                }
            })
            .collect();

        // Get return type
        let return_ty = if let Some(ref ann) = method.return_type {
            self.resolve_type_annotation(ann)
        } else {
            self.type_ctx.void_type()
        };

        let min_params = method
            .params
            .iter()
            .filter(|p| p.default_value.is_none() && !p.optional)
            .count();
        self.type_ctx.function_type_with_min_params(
            param_types,
            return_ty,
            method.is_async,
            min_params,
        )
    }

    fn collect_type_var_names(
        &self,
        ty: TypeId,
        out: &mut Vec<String>,
        seen: &mut std::collections::HashSet<String>,
    ) {
        use crate::parser::types::Type;
        let Some(ty_data) = self.type_ctx.get(ty) else {
            return;
        };
        match ty_data {
            Type::TypeVar(tv) => {
                if seen.insert(tv.name.clone()) {
                    out.push(tv.name.clone());
                }
            }
            Type::Array(arr) => self.collect_type_var_names(arr.element, out, seen),
            Type::Task(task) => self.collect_type_var_names(task.result, out, seen),
            Type::Tuple(tuple) => {
                for &elem in &tuple.elements {
                    self.collect_type_var_names(elem, out, seen);
                }
            }
            Type::Function(func) => {
                for &param in &func.params {
                    self.collect_type_var_names(param, out, seen);
                }
                if let Some(rest) = func.rest_param {
                    self.collect_type_var_names(rest, out, seen);
                }
                self.collect_type_var_names(func.return_type, out, seen);
            }
            Type::Union(union) => {
                for &member in &union.members {
                    self.collect_type_var_names(member, out, seen);
                }
            }
            Type::Generic(generic) => {
                self.collect_type_var_names(generic.base, out, seen);
                for &arg in &generic.type_args {
                    self.collect_type_var_names(arg, out, seen);
                }
            }
            Type::Reference(reference) => {
                if let Some(args) = &reference.type_args {
                    for &arg in args {
                        self.collect_type_var_names(arg, out, seen);
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::binder::Binder;
    use super::*;
    use crate::Parser;

    fn parse_and_check(source: &str) -> Result<(), Vec<CheckError>> {
        let parser = Parser::new(source).unwrap();
        let (module, interner) = parser.parse().unwrap();

        let mut type_ctx = TypeContext::new();
        let binder = Binder::new(&mut type_ctx, &interner);
        let symbols = binder.bind_module(&module).unwrap();

        let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
        checker.check_module(&module).map(|_| ())
    }

    #[test]
    fn test_checker_resolves_interface_call_signature_alias_reference() {
        let source = r#"
            interface Adder { (a: number, b: number): number }
            function unary(a: number): number { return a; }
            let f: Adder = unary;
        "#;
        let parser = Parser::new(source).unwrap();
        let (module, interner) = parser.parse().unwrap();

        let mut type_ctx = TypeContext::new();
        let binder = Binder::new(&mut type_ctx, &interner);
        let symbols = binder.bind_module(&module).unwrap();
        let mut checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);

        let ann = module
            .statements
            .iter()
            .find_map(|stmt| match stmt {
                Statement::VariableDecl(decl) => match &decl.pattern {
                    Pattern::Identifier(ident) if interner.resolve(ident.name) == "f" => {
                        decl.type_annotation.as_ref()
                    }
                    _ => None,
                },
                _ => None,
            })
            .expect("f annotation");

        checker.enter_scope();
        let ty = checker.resolve_type_annotation(ann);
        match checker.type_ctx.get(ty) {
            Some(crate::parser::types::Type::Object(obj)) => {
                assert_eq!(obj.call_signatures.len(), 1);
            }
            other => panic!("expected object type for Adder, got {other:?}"),
        }
        checker.exit_scope();
    }

    #[test]
    fn test_check_simple_arithmetic() {
        let result = parse_and_check("1 + 2;");
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_undefined_variable() {
        let result = parse_and_check("x;");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], CheckError::UndefinedVariable { .. }));
    }

    #[test]
    fn test_check_type_mismatch() {
        let result = parse_and_check(r#"let x: number = "hello";"#);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], CheckError::TypeMismatch { .. }));
    }

    #[test]
    fn test_null_initialized_let_can_widen_on_assignment() {
        let result = parse_and_check(
            r#"
            let x = null;
            x = 42;
            let y: number = x;
        "#,
        );
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }

    #[test]
    fn test_null_initialized_let_branch_assignments_flow_to_use() {
        let result = parse_and_check(
            r#"
            class Resp {
                status(): number { return 200; }
            }

            let res = null;
            if (true) {
                res = new Resp();
            } else {
                res = new Resp();
            }

            let s: number = res.status();
        "#,
        );
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }

    #[test]
    fn test_non_null_inferred_let_stays_strict_on_assignment() {
        let result = parse_and_check(
            r#"
            let x = 1;
            x = "oops";
        "#,
        );
        assert!(result.is_err(), "Expected type error, got {:?}", result);
    }

    #[test]
    fn test_new_allows_const_class_alias() {
        let result = parse_and_check(
            r#"
            class A {
                constructor() {}
                ok(): number { return 1; }
            }
            const B = A;
            const v = new B();
            const n: number = v.ok();
        "#,
        );
        assert!(
            result.is_ok(),
            "Expected class alias to be constructible, got {:?}",
            result
        );
    }

    #[test]
    fn test_new_rejects_object_alias() {
        let result = parse_and_check(
            r#"
            const obj = { x: 1 };
            new obj();
        "#,
        );
        assert!(
            result.is_err(),
            "Expected object alias to be non-constructible"
        );
        let errors = result.unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, CheckError::NewNonClass { .. })),
            "Expected NewNonClass error, got {:?}",
            errors
        );
    }

    #[test]
    fn test_new_allows_member_cast_alias() {
        let result = parse_and_check(
            r#"
            class A {
                constructor() {}
                ok(): number { return 1; }
            }
            const ns = { A: A };
            type TA = { ok: () => number };
            const B = (ns.A as TA);
            const v = new B();
            const n: number = v.ok();
        "#,
        );
        assert!(
            result.is_ok(),
            "Expected member cast alias to be constructible, got {:?}",
            result
        );
    }

    #[test]
    fn test_object_method_type_preserves_optional_params() {
        let result = parse_and_check(
            r#"
            type C = { f: (a: number, b?: number) => number };
            const c: C = { f: (a: number, b: number = 0): number => a + b };
            const x = c.f(1);
            const y: number = x;
        "#,
        );
        assert!(
            result.is_ok(),
            "Expected optional method param to allow omitted arg, got {:?}",
            result
        );
    }

    #[test]
    fn test_class_method_body_is_type_checked() {
        let result = parse_and_check(
            r#"
            class C {
                static run(): number {
                    return missingSymbol();
                }
            }
        "#,
        );
        assert!(
            result.is_err(),
            "Expected unresolved symbol error, got {:?}",
            result
        );
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, CheckError::UndefinedVariable { .. })));
    }

    #[test]
    fn test_class_field_initializer_type_checked() {
        let result = parse_and_check(
            r#"
            class C {
                value: number = "bad";
            }
        "#,
        );
        assert!(
            result.is_err(),
            "Expected field type mismatch, got {:?}",
            result
        );
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, CheckError::TypeMismatch { .. })));
    }

    #[test]
    fn test_super_constructor_call_is_callable() {
        let result = parse_and_check(
            r#"
            class Base {
                constructor(message: string) {}
            }

            class Derived extends Base {
                constructor() {
                    super("ok");
                }
            }
        "#,
        );
        assert!(
            result.is_ok(),
            "Expected super(...) call to type-check, got {:?}",
            result
        );
    }

    #[test]
    fn test_loop_guard_narrows_after_break() {
        let result = parse_and_check(
            r#"
            class TcpStream {}

            function useStream(stream: TcpStream): void {}

            function serve(stream: TcpStream | null): void {
                while (true) {
                    if (stream == null) {
                        break;
                    }
                    useStream(stream);
                    break;
                }
            }
        "#,
        );
        assert!(
            result.is_ok(),
            "Expected stream to be narrowed after break-guard, got {:?}",
            result
        );
    }

    #[test]
    fn test_task_is_cancelled_method_type_checked() {
        let result = parse_and_check(
            r#"
            async function job(): Promise<number> { return 1; }
            async function main(): Promise<boolean> {
                const t = job();
                t.cancel();
                return t.isCancelled();
            }
        "#,
        );
        assert!(
            result.is_ok(),
            "Expected Promise.isCancelled() to type-check, got {:?}",
            result
        );
    }

    #[test]
    fn test_unannotated_async_function_return_is_inferred() {
        let result = parse_and_check(
            r#"
            async function ok() { return 1; }
            function main() {
                let v = await [ok()];
                return v[0];
            }
            return main();
        "#,
        );
        assert!(
            result.is_ok(),
            "Expected unannotated async function return to infer as number, got {:?}",
            result
        );
    }

    // ========================================================================
    // Decorator Type Checking Tests
    // ========================================================================

    #[test]
    fn test_class_decorator_valid() {
        // A valid class decorator is a function that takes Class<T> and returns Class<T> | void
        let result = parse_and_check(
            r#"
            function Injectable<T>(target: T): void {}

            @Injectable
            class Service {}
        "#,
        );
        // Should pass - decorator function is valid
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }

    #[test]
    fn test_class_decorator_factory_valid() {
        // Decorator factory is a function that returns a decorator
        // Use arrow function since function expressions are not supported
        let result = parse_and_check(
            r#"
            function Controller<T>(prefix: string): (target: T) => void {
                return (target: T): void => {};
            }

            @Controller("/api")
            class ApiController {}
        "#,
        );
        // Should pass - decorator factory is valid
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }

    #[test]
    fn test_method_decorator_valid() {
        // A valid method decorator takes a function and returns a function
        let result = parse_and_check(
            r#"
            function Logged<F>(method: F): F {
                return method;
            }

            class Service {
                @Logged
                doWork(): void {}
            }
        "#,
        );
        // Should pass - decorator matches method signature
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }

    #[test]
    fn test_field_decorator_valid() {
        // A valid field decorator takes (target, fieldName) and returns void
        let result = parse_and_check(
            r#"
            function Column<T>(target: T, fieldName: string): void {}

            class User {
                @Column
                name: string = "guest";
            }
        "#,
        );
        // Should pass - decorator signature is valid
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }

    #[test]
    fn test_parameter_decorator_valid() {
        // A valid parameter decorator takes (target, methodName, index) and returns void
        // Note: Parameter decorators on constructor params may not be fully supported by parser
        // So we test on method parameters instead
        let result = parse_and_check(
            r#"
            function Inject<T>(target: T, methodName: string, parameterIndex: number): void {}

            class Service {
                doWork(@Inject dep: number): void {}
            }
        "#,
        );
        // Should pass - decorator signature is valid
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }

    #[test]
    fn test_decorator_not_a_function() {
        // Non-function as decorator should error
        let result = parse_and_check(
            r#"
            let notAFunction: number = 42;

            @notAFunction
            class Service {}
        "#,
        );
        // Should fail - decorator is not a function
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, CheckError::InvalidDecorator { .. })));
    }

    #[test]
    fn test_field_decorator_wrong_param_count() {
        // Field decorator with wrong parameter count should error
        let result = parse_and_check(
            r#"
            function BadDecorator<T>(target: T): void {}

            class User {
                @BadDecorator
                name: string;
            }
        "#,
        );
        // Should fail - field decorator expects 2 params
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, CheckError::InvalidDecorator { .. })));
    }

    #[test]
    fn test_field_decorator_wrong_return_type() {
        // Field decorator must return void
        let result = parse_and_check(
            r#"
            function BadReturn<T>(target: T, fieldName: string): string {
                return fieldName;
            }

            class User {
                @BadReturn
                name: string;
            }
        "#,
        );
        // Should fail - return type is not void
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, CheckError::DecoratorReturnMismatch { .. })));
    }

    #[test]
    fn test_multiple_decorators_on_class() {
        // Multiple decorators on a class
        let result = parse_and_check(
            r#"
            function Dec1<T>(target: T): void {}
            function Dec2<T>(target: T): void {}

            @Dec1
            @Dec2
            class Service {}
        "#,
        );
        // Should pass - both decorators are valid
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }

    #[test]
    fn test_multiple_decorators_on_method() {
        // Multiple decorators on a method
        let result = parse_and_check(
            r#"
            function Log<F>(method: F): F { return method; }
            function Measure<F>(method: F): F { return method; }

            class Service {
                @Log
                @Measure
                doWork(): void {}
            }
        "#,
        );
        // Should pass - both decorators are valid
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }

    // ========================================================================
    // Decorator Type Alias Tests (Milestone 3.9 Phase 2)
    // ========================================================================

    #[test]
    fn test_class_decorator_type_alias_registered() {
        // Verify ClassDecorator type alias is registered and can be referenced
        let result = parse_and_check(
            r#"
            // Use ClassDecorator type alias in function declaration
            function makeSealed<T>(target: T): T | void {
                return target;
            }

            @makeSealed
            class MyClass {}
        "#,
        );
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }

    #[test]
    fn test_method_decorator_type_alias_registered() {
        // Verify MethodDecorator type alias concept works
        let result = parse_and_check(
            r#"
            // Method decorator function that takes function and returns function
            function log<F>(method: F): F {
                return method;
            }

            class Service {
                @log
                process(): void {}
            }
        "#,
        );
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }

    #[test]
    fn test_field_decorator_type_alias_registered() {
        // Verify FieldDecorator signature works
        let result = parse_and_check(
            r#"
            // Field decorator with correct signature
            function validate<T>(target: T, fieldName: string): void {}

            class Entity {
                @validate
                name: string = "";
            }
        "#,
        );
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }

    #[test]
    fn test_class_interface_registered() {
        // Verify Class<T> is registered as a type
        // This test uses Class-like pattern
        let result = parse_and_check(
            r#"
            class Foo {}

            // Function that accepts class-like object
            function getClassName<T>(cls: T): string {
                return "name";
            }

            let name: string = getClassName(Foo);
        "#,
        );
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }

    #[test]
    fn test_decorator_with_generic_constraint() {
        // Verify decorator with generic type parameter works
        let result = parse_and_check(
            r#"
            function Injectable<T>(target: T): void {}

            @Injectable
            class UserService {}

            @Injectable
            class ProductService {}
        "#,
        );
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }

    #[test]
    fn test_decorator_factory_with_generic() {
        // Verify decorator factory with generic works
        let result = parse_and_check(
            r#"
            function Route<T>(path: string): (target: T) => void {
                return (target: T): void => {};
            }

            @Route("/users")
            class UserController {}
        "#,
        );
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }

    #[test]
    fn test_chained_and_preserves_null_narrowing_for_member_access() {
        let result = parse_and_check(
            r#"
            function main(): boolean {
                let got: { value: string, writable: boolean } | null = null;
                got = { value: "locked", writable: false };
                return got != null && got.value == "locked" && got.writable == false;
            }
        "#,
        );
        assert!(
            result.is_ok(),
            "Expected chained && to preserve null narrowing across member accesses, got {:?}",
            result
        );
    }
}
