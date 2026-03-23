//! Name binding - builds symbol tables from AST
//!
//! The binder walks the AST and creates symbol tables for name resolution.
//! It resolves type annotations to TypeId values and tracks all declarations.

use super::builtins::BuiltinSignatures;
use super::error::BindError;
use super::symbols::{ScopeId, ScopeKind, Symbol, SymbolFlags, SymbolKind, SymbolTable};
use super::{CheckerPolicy, TypeSystemMode};
use crate::parser::ast::*;
use crate::parser::types::try_hydrate_type_from_canonical_signature;
use crate::parser::types::ty::{
    ClassType, MethodSignature, PropertySignature, Type, TypeReference,
};
use crate::parser::types::{TypeContext, TypeId};
use crate::parser::Interner;
use crate::parser::Span;

/// Binder - builds symbol tables from AST
///
/// The binder performs two main tasks:
/// 1. Creates symbol table entries for all declarations
/// 2. Resolves type annotations to TypeId values
pub struct Binder<'a> {
    symbols: SymbolTable,
    type_ctx: &'a mut TypeContext,
    interner: &'a Interner,
    /// Tracks class names that have been fully bound (in bind_class).
    /// Used to detect duplicate class declarations in user code.
    bound_classes: std::collections::HashMap<String, crate::parser::Span>,
    /// Tracks function names that have been fully bound (in bind_function).
    /// Used to detect duplicate function declarations in user code.
    bound_functions: std::collections::HashMap<String, crate::parser::Span>,
    /// When true, duplicate top-level class/function declarations are rejected.
    /// Some helper/builtin compilation paths intentionally disable this.
    reject_duplicate_top_level_declarations: bool,
    /// Tracks type parameter names for generic type aliases (e.g., Container<T> → ["T"])
    generic_type_alias_params: rustc_hash::FxHashMap<String, Vec<String>>,
    /// Type system behavior mode.
    mode: TypeSystemMode,
    /// Effective checker policy.
    policy: CheckerPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BinderFallbackReason {
    UnresolvedTypeParse,
    PatternBinding,
}

impl<'a> Binder<'a> {
    fn has_symbol_in_current_scope(&self, name: &str) -> bool {
        self.symbols.current().symbols.contains_key(name)
    }

    fn symbol_in_scope(&self, scope_id: ScopeId, name: &str) -> Option<Symbol> {
        self.symbols.get_scope(scope_id).symbols.get(name).cloned()
    }

    /// Create a new binder
    pub fn new(type_ctx: &'a mut TypeContext, interner: &'a Interner) -> Self {
        Binder {
            symbols: SymbolTable::new(),
            type_ctx,
            interner,
            bound_classes: std::collections::HashMap::new(),
            bound_functions: std::collections::HashMap::new(),
            reject_duplicate_top_level_declarations: true,
            generic_type_alias_params: rustc_hash::FxHashMap::default(),
            mode: TypeSystemMode::Raya,
            policy: CheckerPolicy::for_mode(TypeSystemMode::Raya),
        }
    }

    /// Set checker/binder behavior mode.
    pub fn with_mode(mut self, mode: TypeSystemMode) -> Self {
        self.mode = mode;
        self.policy = CheckerPolicy::for_mode(mode);
        self
    }

    /// Set explicit checker/binder policy.
    pub fn with_policy(mut self, policy: CheckerPolicy) -> Self {
        self.policy = policy;
        self
    }

    #[inline]
    fn allows_explicit_any(&self) -> bool {
        self.policy.allow_explicit_any
    }

    #[inline]
    fn uses_js_dynamic_fallback(&self) -> bool {
        false
    }

    #[inline]
    fn is_js_mode(&self) -> bool {
        self.mode != TypeSystemMode::Raya
    }

    #[inline]
    fn inference_fallback_type(&mut self) -> TypeId {
        if self.uses_js_dynamic_fallback() {
            self.type_ctx.jsobject_type()
        } else {
            self.type_ctx.unknown_type()
        }
    }

    #[inline]
    fn fallback_type(&mut self, _reason: BinderFallbackReason) -> TypeId {
        self.inference_fallback_type()
    }

    fn binding_scope_for_variable_kind(&self, kind: VariableKind) -> ScopeId {
        if self.is_js_mode() && matches!(kind, VariableKind::Var) {
            return self.nearest_var_scope_id();
        }
        self.symbols.current_scope_id()
    }

    fn nearest_var_scope_id(&self) -> ScopeId {
        let mut scope_id = self.symbols.current_scope_id();
        loop {
            let scope = self.symbols.get_scope(scope_id);
            if matches!(
                scope.kind,
                ScopeKind::Function | ScopeKind::Module | ScopeKind::Global
            ) {
                return scope_id;
            }
            match scope.parent {
                Some(parent) => scope_id = parent,
                None => return scope_id,
            }
        }
    }

    /// Allow duplicate top-level class/function declarations for synthetic/helper builds.
    pub fn allow_duplicate_top_level_declarations(&mut self) {
        self.reject_duplicate_top_level_declarations = false;
    }

    /// Register an external class type so it can be referenced by name during binding.
    /// Used to pre-register builtin primitive types (e.g., RegExp, Array) before
    /// compiling a `.raya` file that cross-references them.
    pub fn register_external_class(&mut self, name: &str) {
        let implicit_object_base = self.implicit_object_base_type(name);
        // Preserve primitive named types (`string`, `number`, ...) as primitives.
        // Builtin wrapper classes for those names need their own class TypeId so
        // previously interned primitive references are not rewritten into classes.
        let type_id = if let Some(existing) = self.type_ctx.lookup_named_type(name) {
            match self.type_ctx.get(existing) {
                Some(Type::Class(_)) => existing,
                _ => self.type_ctx.intern(Type::Class(ClassType {
                    name: name.to_string(),
                    type_params: Vec::new(),
                    properties: Vec::new(),
                    methods: Vec::new(),
                    static_properties: Vec::new(),
                    static_methods: Vec::new(),
                    extends: implicit_object_base,
                    implements: Vec::new(),
                    is_abstract: false,
                })),
            }
        } else {
            let id = self.type_ctx.intern(Type::Class(ClassType {
                name: name.to_string(),
                type_params: Vec::new(),
                properties: Vec::new(),
                methods: Vec::new(),
                static_properties: Vec::new(),
                static_methods: Vec::new(),
                extends: implicit_object_base,
                implements: Vec::new(),
                is_abstract: false,
            }));
            self.type_ctx.register_named_type(name.to_string(), id);
            id
        };
        let symbol = Symbol {
            name: name.to_string(),
            kind: SymbolKind::Class,
            ty: type_id,
            flags: SymbolFlags {
                is_exported: false,
                is_const: true,
                is_async: false,
                is_readonly: false,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        };
        let _ = self.symbols.define(symbol);
    }

    /// Register builtin type signatures
    ///
    /// This registers classes and functions from builtin signatures so they
    /// are available during type checking.
    pub fn register_builtins(&mut self, builtins: &[BuiltinSignatures]) {
        // Register compiler intrinsics first
        self.register_intrinsics();

        // Register decorator type aliases (ClassDecorator<T>, etc.)
        self.register_decorator_types();

        // Predeclare all builtin class symbols/types so builtin property/method
        // signatures can reference classes that are declared later in BUILTIN_SIGS
        // (including self-references like `Error | null`).
        for sig in builtins {
            for class_sig in &sig.classes {
                self.register_external_class(&class_sig.name);
            }
        }

        for sig in builtins {
            // Register each class from this builtin module
            for class_sig in &sig.classes {
                self.register_builtin_class(class_sig);
            }

            // Register each function from this builtin module
            for func_sig in &sig.functions {
                self.register_builtin_function(func_sig);
            }
        }

        self.register_builtin_event_emitter();
    }

    fn register_builtin_event_emitter(&mut self) {
        if self.symbols.resolve("EventEmitter").is_some() {
            return;
        }

        let type_param_name = "E".to_string();
        let event_map_ty = self.type_ctx.type_variable(type_param_name.clone());
        let event_key_ty = self.type_ctx.string_type();
        let event_index_ty = self.type_ctx.keyof_type(event_map_ty);
        let event_payload_ty = self
            .type_ctx
            .indexed_access_type(event_map_ty, event_index_ty);
        let void_ty = self.type_ctx.void_type();
        let bool_ty = self.type_ctx.boolean_type();
        let number_ty = self.type_ctx.number_type();
        let string_ty = self.type_ctx.string_type();

        let listener_ty = self.type_ctx.function_type_with_rest(
            vec![],
            void_ty,
            false,
            0,
            Some(event_payload_ty),
        );
        let this_ref_ty = self.type_ctx.intern(Type::Reference(TypeReference {
            name: "EventEmitter".to_string(),
            type_args: Some(vec![event_map_ty]),
        }));
        let listener_array_ty = self.type_ctx.array_type(listener_ty);
        let string_array_ty = self.type_ctx.array_type(string_ty);

        let methods = vec![
            MethodSignature {
                name: "on".to_string(),
                ty: self.type_ctx.function_type(
                    vec![event_key_ty, listener_ty],
                    this_ref_ty,
                    false,
                ),
                type_params: Vec::new(),
                visibility: Default::default(),
            },
            MethodSignature {
                name: "once".to_string(),
                ty: self.type_ctx.function_type(
                    vec![event_key_ty, listener_ty],
                    this_ref_ty,
                    false,
                ),
                type_params: Vec::new(),
                visibility: Default::default(),
            },
            MethodSignature {
                name: "off".to_string(),
                ty: self.type_ctx.function_type(
                    vec![event_key_ty, listener_ty],
                    this_ref_ty,
                    false,
                ),
                type_params: Vec::new(),
                visibility: Default::default(),
            },
            MethodSignature {
                name: "addListener".to_string(),
                ty: self.type_ctx.function_type(
                    vec![event_key_ty, listener_ty],
                    this_ref_ty,
                    false,
                ),
                type_params: Vec::new(),
                visibility: Default::default(),
            },
            MethodSignature {
                name: "removeListener".to_string(),
                ty: self.type_ctx.function_type(
                    vec![event_key_ty, listener_ty],
                    this_ref_ty,
                    false,
                ),
                type_params: Vec::new(),
                visibility: Default::default(),
            },
            MethodSignature {
                name: "emit".to_string(),
                ty: self.type_ctx.function_type_with_rest(
                    vec![event_key_ty],
                    bool_ty,
                    false,
                    1,
                    Some(event_payload_ty),
                ),
                type_params: Vec::new(),
                visibility: Default::default(),
            },
            MethodSignature {
                name: "listeners".to_string(),
                ty: self
                    .type_ctx
                    .function_type(vec![event_key_ty], listener_array_ty, false),
                type_params: Vec::new(),
                visibility: Default::default(),
            },
            MethodSignature {
                name: "listenerCount".to_string(),
                ty: self
                    .type_ctx
                    .function_type(vec![string_ty], number_ty, false),
                type_params: Vec::new(),
                visibility: Default::default(),
            },
            MethodSignature {
                name: "eventNames".to_string(),
                ty: self.type_ctx.function_type(vec![], string_array_ty, false),
                type_params: Vec::new(),
                visibility: Default::default(),
            },
            MethodSignature {
                name: "setMaxListeners".to_string(),
                ty: self
                    .type_ctx
                    .function_type(vec![number_ty], this_ref_ty, false),
                type_params: Vec::new(),
                visibility: Default::default(),
            },
            MethodSignature {
                name: "getMaxListeners".to_string(),
                ty: self.type_ctx.function_type(vec![], number_ty, false),
                type_params: Vec::new(),
                visibility: Default::default(),
            },
            MethodSignature {
                name: "removeAllListeners".to_string(),
                ty: self.type_ctx.function_type_with_min_params(
                    vec![string_ty],
                    this_ref_ty,
                    false,
                    0,
                ),
                type_params: Vec::new(),
                visibility: Default::default(),
            },
        ];

        let class_ty = self.type_ctx.intern(Type::Class(ClassType {
            name: "EventEmitter".to_string(),
            type_params: vec![type_param_name],
            properties: Vec::new(),
            methods,
            static_properties: Vec::new(),
            static_methods: Vec::new(),
            extends: None,
            implements: Vec::new(),
            is_abstract: false,
        }));
        self.type_ctx
            .register_named_type("EventEmitter".to_string(), class_ty);
        let _ = self.symbols.define(Symbol {
            name: "EventEmitter".to_string(),
            kind: SymbolKind::Class,
            ty: class_ty,
            flags: SymbolFlags {
                is_exported: false,
                is_const: true,
                is_async: false,
                is_readonly: false,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span::new(0, 0, 0, 0),
            referenced: false,
        });
    }

    /// Define an imported symbol
    ///
    /// Used to inject symbols from imported modules before binding.
    pub fn define_imported(
        &mut self,
        symbol: Symbol,
    ) -> Result<(), super::symbols::DuplicateSymbolError> {
        self.symbols.define_imported(symbol)
    }

    /// Hydrate a canonical structural signature into this module's `TypeContext`.
    pub fn hydrate_imported_signature_type(&mut self, signature: &str) -> TypeId {
        // Use `unknown` (not `any`) on parse failure so strict import paths
        // can deterministically reject unresolved structural signatures.
        try_hydrate_type_from_canonical_signature(signature, self.type_ctx)
            .unwrap_or_else(|| self.type_ctx.unknown_type())
    }

    /// Check whether a type ID currently resolves to `unknown`.
    pub fn is_unknown_type_id(&self, ty: TypeId) -> bool {
        matches!(self.type_ctx.get(ty), Some(Type::Unknown))
    }

    /// Returns true when imported signature hydration produced a type that is
    /// not directly actionable for value-member calls in strict mode.
    pub fn needs_import_namespace_fallback(&self, ty: TypeId) -> bool {
        matches!(
            self.type_ctx.get(ty),
            Some(Type::Unknown) | Some(Type::Reference(_))
        )
    }

    /// Return the canonical `any` type ID.
    pub fn any_type_id(&mut self) -> TypeId {
        self.type_ctx.any_type()
    }

    /// Build a structural object type from named namespace members.
    pub fn object_type_from_members(&mut self, members: Vec<(String, TypeId)>) -> TypeId {
        let properties = members
            .into_iter()
            .map(|(name, ty)| PropertySignature {
                name,
                ty,
                optional: false,
                readonly: true,
                visibility: crate::parser::ast::Visibility::Public,
            })
            .collect();
        self.type_ctx
            .intern(Type::Object(crate::parser::types::ty::ObjectType {
                properties,
                index_signature: None,
                call_signatures: vec![],
                construct_signatures: vec![],
            }))
    }

    /// Format a type ID for diagnostics/debugging.
    pub fn format_type_id(&self, ty: TypeId) -> String {
        self.type_ctx.format_type(ty)
    }

    /// Register a named type alias for imported symbols so hydrated reference
    /// types (e.g. `ReadableStream<T>`) can resolve during member checking.
    pub fn register_imported_named_type(&mut self, name: &str, ty: TypeId) {
        if self.type_ctx.lookup_named_type(name).is_none() {
            self.type_ctx.register_named_type(name.to_string(), ty);
        }
    }

    /// Override a named type mapping in the binder type context.
    ///
    /// Binary declaration surfaces are the authoritative module-boundary contract.
    /// They must be able to replace older builtin checker stubs when the richer
    /// canonical declaration shape differs.
    pub fn override_imported_named_type(&mut self, name: &str, ty: TypeId) {
        self.type_ctx.register_named_type(name.to_string(), ty);
    }

    /// Look up a named type in the binder type context.
    pub fn lookup_named_type(&self, name: &str) -> Option<TypeId> {
        self.type_ctx.lookup_named_type(name)
    }

    /// Return whether a symbol already exists in global scope.
    pub fn has_global_symbol(&self, name: &str) -> bool {
        self.symbols.resolve_from_scope(name, ScopeId(0)).is_some()
    }

    /// Register compiler intrinsics like __NATIVE_CALL and __OPCODE_CHANNEL_NEW
    ///
    /// These are special functions used in builtin .raya files to call VM opcodes.
    fn register_intrinsics(&mut self) {
        // __NATIVE_CALL(native_id: number, ...args): any
        // This is a variadic function that can return any type
        let any_ty = self.type_ctx.any_type();
        let number_ty = self.type_ctx.number_type();

        // Create a function type: (number) -> any
        // The type checker will allow additional arguments
        let native_call_ty = self.type_ctx.function_type(vec![number_ty], any_ty, false);
        let symbol = Symbol {
            name: "__NATIVE_CALL".to_string(),
            kind: SymbolKind::Function,
            ty: native_call_ty,
            flags: SymbolFlags {
                is_exported: false,
                is_const: true,
                is_async: false,
                is_readonly: false,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        };
        let _ = self.symbols.define(symbol);

        // __OPCODE_CHANNEL_NEW(capacity: number): number
        let channel_new_ty = self
            .type_ctx
            .function_type(vec![number_ty], number_ty, false);
        let symbol = Symbol {
            name: "__OPCODE_CHANNEL_NEW".to_string(),
            kind: SymbolKind::Function,
            ty: channel_new_ty,
            flags: SymbolFlags {
                is_exported: false,
                is_const: true,
                is_async: false,
                is_readonly: false,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        };
        let _ = self.symbols.define(symbol);

        let void_ty = self.type_ctx.void_type();

        // __OPCODE_MUTEX_NEW(): number
        let mutex_new_ty = self.type_ctx.function_type(vec![], number_ty, false);
        let symbol = Symbol {
            name: "__OPCODE_MUTEX_NEW".to_string(),
            kind: SymbolKind::Function,
            ty: mutex_new_ty,
            flags: SymbolFlags {
                is_exported: false,
                is_const: true,
                is_async: false,
                is_readonly: false,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        };
        let _ = self.symbols.define(symbol);

        // __OPCODE_MUTEX_LOCK(handle: number): void
        let mutex_lock_ty = self.type_ctx.function_type(vec![number_ty], void_ty, false);
        let symbol = Symbol {
            name: "__OPCODE_MUTEX_LOCK".to_string(),
            kind: SymbolKind::Function,
            ty: mutex_lock_ty,
            flags: SymbolFlags {
                is_exported: false,
                is_const: true,
                is_async: false,
                is_readonly: false,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        };
        let _ = self.symbols.define(symbol);

        // __OPCODE_MUTEX_UNLOCK(handle: number): void
        let mutex_unlock_ty = self.type_ctx.function_type(vec![number_ty], void_ty, false);
        let symbol = Symbol {
            name: "__OPCODE_MUTEX_UNLOCK".to_string(),
            kind: SymbolKind::Function,
            ty: mutex_unlock_ty,
            flags: SymbolFlags {
                is_exported: false,
                is_const: true,
                is_async: false,
                is_readonly: false,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        };
        let _ = self.symbols.define(symbol);

        // __OPCODE_TASK_CANCEL(handle: number): void
        let task_cancel_ty = self.type_ctx.function_type(vec![number_ty], void_ty, false);
        let symbol = Symbol {
            name: "__OPCODE_TASK_CANCEL".to_string(),
            kind: SymbolKind::Function,
            ty: task_cancel_ty,
            flags: SymbolFlags {
                is_exported: false,
                is_const: true,
                is_async: false,
                is_readonly: false,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        };
        let _ = self.symbols.define(symbol);

        // __OPCODE_YIELD(): void
        let yield_ty = self.type_ctx.function_type(vec![], void_ty, false);
        let symbol = Symbol {
            name: "__OPCODE_YIELD".to_string(),
            kind: SymbolKind::Function,
            ty: yield_ty,
            flags: SymbolFlags {
                is_exported: false,
                is_const: true,
                is_async: false,
                is_readonly: false,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        };
        let _ = self.symbols.define(symbol);

        // __OPCODE_SLEEP(durationMs: number): void
        let sleep_ty = self.type_ctx.function_type(vec![number_ty], void_ty, false);
        let symbol = Symbol {
            name: "__OPCODE_SLEEP".to_string(),
            kind: SymbolKind::Function,
            ty: sleep_ty,
            flags: SymbolFlags {
                is_exported: false,
                is_const: true,
                is_async: false,
                is_readonly: false,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        };
        let _ = self.symbols.define(symbol);

        // __OPCODE_ARRAY_LEN(arr: any): number
        let array_len_ty = self.type_ctx.function_type(vec![any_ty], number_ty, false);
        let symbol = Symbol {
            name: "__OPCODE_ARRAY_LEN".to_string(),
            kind: SymbolKind::Function,
            ty: array_len_ty,
            flags: SymbolFlags {
                is_exported: false,
                is_const: true,
                is_async: false,
                is_readonly: false,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        };
        let _ = self.symbols.define(symbol);

        // __OPCODE_ARRAY_PUSH(arr: any, elem: any): void
        let array_push_ty = self
            .type_ctx
            .function_type(vec![any_ty, any_ty], void_ty, false);
        let symbol = Symbol {
            name: "__OPCODE_ARRAY_PUSH".to_string(),
            kind: SymbolKind::Function,
            ty: array_push_ty,
            flags: SymbolFlags {
                is_exported: false,
                is_const: true,
                is_async: false,
                is_readonly: false,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        };
        let _ = self.symbols.define(symbol);

        // __OPCODE_ARRAY_POP(arr: any): any
        let array_pop_ty = self.type_ctx.function_type(vec![any_ty], any_ty, false);
        let symbol = Symbol {
            name: "__OPCODE_ARRAY_POP".to_string(),
            kind: SymbolKind::Function,
            ty: array_pop_ty,
            flags: SymbolFlags {
                is_exported: false,
                is_const: true,
                is_async: false,
                is_readonly: false,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        };
        let _ = self.symbols.define(symbol);

        // undefined: any
        let symbol = Symbol {
            name: "undefined".to_string(),
            kind: SymbolKind::Variable,
            ty: any_ty,
            flags: SymbolFlags {
                is_exported: false,
                is_const: true,
                is_async: false,
                is_readonly: true,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        };
        let _ = self.symbols.define(symbol);

        // Infinity: number
        let symbol = Symbol {
            name: "Infinity".to_string(),
            kind: SymbolKind::Variable,
            ty: number_ty,
            flags: SymbolFlags {
                is_exported: false,
                is_const: true,
                is_async: false,
                is_readonly: true,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        };
        let _ = self.symbols.define(symbol);

        // NaN: number
        let symbol = Symbol {
            name: "NaN".to_string(),
            kind: SymbolKind::Variable,
            ty: number_ty,
            flags: SymbolFlags {
                is_exported: false,
                is_const: true,
                is_async: false,
                is_readonly: true,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        };
        let _ = self.symbols.define(symbol);

        // Register JSON global object with static methods
        self.register_json_global();
    }

    /// Register the JSON global object with static methods
    ///
    /// JSON is a built-in global object (like JavaScript's JSON) with:
    /// - JSON.stringify(value: any): string - Runtime serialization
    /// - JSON.parse(json: string): any - Runtime parsing
    ///
    /// JSON.parse returns the `json` type, which supports duck typing:
    /// property access returns json values.
    fn register_json_global(&mut self) {
        let string_ty = self.type_ctx.string_type();
        let any_ty = self.type_ctx.any_type();
        let json_ty = self.type_ctx.json_type();

        // Build static methods for JSON object
        // JSON.stringify takes any value and returns string
        // JSON.parse returns json type (supports duck typing)
        let static_methods = vec![
            MethodSignature {
                name: "stringify".to_string(),
                ty: self.type_ctx.function_type(vec![any_ty], string_ty, false),
                type_params: vec![],
                visibility: Default::default(),
            },
            MethodSignature {
                name: "parse".to_string(),
                ty: self.type_ctx.function_type(vec![string_ty], json_ty, false),
                type_params: vec![],
                visibility: Default::default(),
            },
        ];

        // Create JSON as a class type with only static methods
        let json_class = ClassType {
            name: "JSON".to_string(),
            type_params: vec![],
            properties: vec![],
            methods: vec![],
            static_properties: vec![],
            static_methods,
            extends: None,
            implements: vec![],
            is_abstract: false,
        };

        let json_ty = self.type_ctx.intern(Type::Class(json_class));

        // Register JSON as a global symbol
        let symbol = Symbol {
            name: "JSON".to_string(),
            kind: SymbolKind::Class,
            ty: json_ty,
            flags: SymbolFlags {
                is_exported: false,
                is_const: true,
                is_async: false,
                is_readonly: true,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        };
        let _ = self.symbols.define(symbol);
    }

    /// Register decorator-related built-in types
    ///
    /// This registers:
    /// - Ctor<T>: strict constructor alias (approximation of class constructor type)
    /// - *DecoratorContext interfaces (TS-like context payload types)
    /// - Ts*Decorator aliases (TS-like decorator signatures without `any`)
    /// - Legacy ClassDecorator/MethodDecorator/FieldDecorator/ParameterDecorator for compatibility
    /// - Class<T>: Interface representing a class constructor
    /// - ClassDecorator<T>: (target: Class<T>) => Class<T> | void
    /// - MethodDecorator<F>: (method: F) => F
    /// - FieldDecorator<T>: (target: T, fieldName: string) => void
    /// - ParameterDecorator<T>: (target: T, methodName: string, parameterIndex: number) => void
    fn register_decorator_types(&mut self) {
        let string_ty = self.type_ctx.string_type();
        let number_ty = self.type_ctx.number_type();
        let boolean_ty = self.type_ctx.boolean_type();
        let void_ty = self.type_ctx.void_type();

        // Register Class<T> interface
        // Class<T> has: name: string, prototype: T, and is callable as constructor
        let t_var = self.type_ctx.type_variable("T".to_string());
        let class_interface = ClassType {
            name: "Class".to_string(),
            type_params: vec!["T".to_string()],
            properties: vec![
                PropertySignature {
                    name: "name".to_string(),
                    ty: string_ty,
                    optional: false,
                    readonly: true,
                    visibility: Default::default(),
                },
                PropertySignature {
                    name: "prototype".to_string(),
                    ty: t_var,
                    optional: false,
                    readonly: true,
                    visibility: Default::default(),
                },
            ],
            methods: vec![],
            static_properties: vec![],
            static_methods: vec![],
            extends: None,
            implements: vec![],
            is_abstract: false,
        };
        let class_ty = self.type_ctx.intern(Type::Class(class_interface));
        let class_symbol = Symbol {
            name: "Class".to_string(),
            kind: SymbolKind::Class,
            ty: class_ty,
            flags: SymbolFlags {
                is_exported: true,
                is_const: true,
                is_async: false,
                is_readonly: true,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        };
        let _ = self.symbols.define(class_symbol);
        self.type_ctx
            .register_named_type("Class".to_string(), class_ty);

        // Ctor<T> = Class<T> (strict approximation without `any`)
        let ctor_symbol = Symbol {
            name: "Ctor".to_string(),
            kind: SymbolKind::TypeAlias,
            ty: class_ty,
            flags: SymbolFlags {
                is_exported: true,
                is_const: true,
                is_async: false,
                is_readonly: true,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        };
        let _ = self.symbols.define(ctor_symbol);
        self.type_ctx
            .register_named_type("Ctor".to_string(), class_ty);

        // ClassDecoratorContext
        let class_decorator_context_ty = self.type_ctx.intern(Type::Class(ClassType {
            name: "ClassDecoratorContext".to_string(),
            type_params: vec![],
            properties: vec![
                PropertySignature {
                    name: "kind".to_string(),
                    ty: string_ty,
                    optional: false,
                    readonly: true,
                    visibility: Default::default(),
                },
                PropertySignature {
                    name: "name".to_string(),
                    ty: string_ty,
                    optional: false,
                    readonly: true,
                    visibility: Default::default(),
                },
            ],
            methods: vec![],
            static_properties: vec![],
            static_methods: vec![],
            extends: None,
            implements: vec![],
            is_abstract: false,
        }));
        let _ = self.symbols.define(Symbol {
            name: "ClassDecoratorContext".to_string(),
            kind: SymbolKind::TypeAlias,
            ty: class_decorator_context_ty,
            flags: SymbolFlags {
                is_exported: true,
                is_const: true,
                is_async: false,
                is_readonly: true,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        });
        self.type_ctx.register_named_type(
            "ClassDecoratorContext".to_string(),
            class_decorator_context_ty,
        );

        // MethodDecoratorContext<This>
        let method_decorator_context_ty = self.type_ctx.intern(Type::Class(ClassType {
            name: "MethodDecoratorContext".to_string(),
            type_params: vec!["This".to_string()],
            properties: vec![
                PropertySignature {
                    name: "kind".to_string(),
                    ty: string_ty,
                    optional: false,
                    readonly: true,
                    visibility: Default::default(),
                },
                PropertySignature {
                    name: "name".to_string(),
                    ty: string_ty,
                    optional: false,
                    readonly: true,
                    visibility: Default::default(),
                },
                PropertySignature {
                    name: "static".to_string(),
                    ty: boolean_ty,
                    optional: false,
                    readonly: true,
                    visibility: Default::default(),
                },
                PropertySignature {
                    name: "class".to_string(),
                    ty: class_ty,
                    optional: false,
                    readonly: true,
                    visibility: Default::default(),
                },
            ],
            methods: vec![],
            static_properties: vec![],
            static_methods: vec![],
            extends: None,
            implements: vec![],
            is_abstract: false,
        }));
        let _ = self.symbols.define(Symbol {
            name: "MethodDecoratorContext".to_string(),
            kind: SymbolKind::TypeAlias,
            ty: method_decorator_context_ty,
            flags: SymbolFlags {
                is_exported: true,
                is_const: true,
                is_async: false,
                is_readonly: true,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        });
        self.type_ctx.register_named_type(
            "MethodDecoratorContext".to_string(),
            method_decorator_context_ty,
        );

        // FieldDecoratorContext<This, V>
        let field_decorator_context_ty = self.type_ctx.intern(Type::Class(ClassType {
            name: "FieldDecoratorContext".to_string(),
            type_params: vec!["This".to_string(), "V".to_string()],
            properties: vec![
                PropertySignature {
                    name: "kind".to_string(),
                    ty: string_ty,
                    optional: false,
                    readonly: true,
                    visibility: Default::default(),
                },
                PropertySignature {
                    name: "name".to_string(),
                    ty: string_ty,
                    optional: false,
                    readonly: true,
                    visibility: Default::default(),
                },
                PropertySignature {
                    name: "static".to_string(),
                    ty: boolean_ty,
                    optional: false,
                    readonly: true,
                    visibility: Default::default(),
                },
                PropertySignature {
                    name: "class".to_string(),
                    ty: class_ty,
                    optional: false,
                    readonly: true,
                    visibility: Default::default(),
                },
            ],
            methods: vec![],
            static_properties: vec![],
            static_methods: vec![],
            extends: None,
            implements: vec![],
            is_abstract: false,
        }));
        let _ = self.symbols.define(Symbol {
            name: "FieldDecoratorContext".to_string(),
            kind: SymbolKind::TypeAlias,
            ty: field_decorator_context_ty,
            flags: SymbolFlags {
                is_exported: true,
                is_const: true,
                is_async: false,
                is_readonly: true,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        });
        self.type_ctx.register_named_type(
            "FieldDecoratorContext".to_string(),
            field_decorator_context_ty,
        );

        // ParameterDecoratorContext<This>
        let parameter_decorator_context_ty = self.type_ctx.intern(Type::Class(ClassType {
            name: "ParameterDecoratorContext".to_string(),
            type_params: vec!["This".to_string()],
            properties: vec![
                PropertySignature {
                    name: "kind".to_string(),
                    ty: string_ty,
                    optional: false,
                    readonly: true,
                    visibility: Default::default(),
                },
                PropertySignature {
                    name: "name".to_string(),
                    ty: string_ty,
                    optional: false,
                    readonly: true,
                    visibility: Default::default(),
                },
                PropertySignature {
                    name: "index".to_string(),
                    ty: number_ty,
                    optional: false,
                    readonly: true,
                    visibility: Default::default(),
                },
                PropertySignature {
                    name: "class".to_string(),
                    ty: class_ty,
                    optional: false,
                    readonly: true,
                    visibility: Default::default(),
                },
            ],
            methods: vec![],
            static_properties: vec![],
            static_methods: vec![],
            extends: None,
            implements: vec![],
            is_abstract: false,
        }));
        let _ = self.symbols.define(Symbol {
            name: "ParameterDecoratorContext".to_string(),
            kind: SymbolKind::TypeAlias,
            ty: parameter_decorator_context_ty,
            flags: SymbolFlags {
                is_exported: true,
                is_const: true,
                is_async: false,
                is_readonly: true,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        });
        self.type_ctx.register_named_type(
            "ParameterDecoratorContext".to_string(),
            parameter_decorator_context_ty,
        );

        // TS-style strict decorator aliases (without `any`)
        let ts_class_t_var = self.type_ctx.type_variable("T".to_string());
        let ts_class_decorator_return = self.type_ctx.union_type(vec![ts_class_t_var, void_ty]);
        let ts_class_decorator_ty = self.type_ctx.function_type(
            vec![ts_class_t_var, class_decorator_context_ty],
            ts_class_decorator_return,
            false,
        );
        let _ = self.symbols.define(Symbol {
            name: "TsClassDecorator".to_string(),
            kind: SymbolKind::TypeAlias,
            ty: ts_class_decorator_ty,
            flags: SymbolFlags {
                is_exported: true,
                is_const: true,
                is_async: false,
                is_readonly: true,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        });
        self.type_ctx
            .register_named_type("TsClassDecorator".to_string(), ts_class_decorator_ty);

        let ts_f_var = self.type_ctx.type_variable("F".to_string());
        let ts_method_decorator_return = self.type_ctx.union_type(vec![ts_f_var, void_ty]);
        let ts_method_decorator_ty = self.type_ctx.function_type(
            vec![ts_f_var, method_decorator_context_ty],
            ts_method_decorator_return,
            false,
        );
        let _ = self.symbols.define(Symbol {
            name: "TsMethodDecorator".to_string(),
            kind: SymbolKind::TypeAlias,
            ty: ts_method_decorator_ty,
            flags: SymbolFlags {
                is_exported: true,
                is_const: true,
                is_async: false,
                is_readonly: true,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        });
        self.type_ctx
            .register_named_type("TsMethodDecorator".to_string(), ts_method_decorator_ty);

        let ts_field_t_var = self.type_ctx.type_variable("This".to_string());
        let ts_field_decorator_ty = self.type_ctx.function_type(
            vec![ts_field_t_var, field_decorator_context_ty],
            void_ty,
            false,
        );
        let _ = self.symbols.define(Symbol {
            name: "TsFieldDecorator".to_string(),
            kind: SymbolKind::TypeAlias,
            ty: ts_field_decorator_ty,
            flags: SymbolFlags {
                is_exported: true,
                is_const: true,
                is_async: false,
                is_readonly: true,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        });
        self.type_ctx
            .register_named_type("TsFieldDecorator".to_string(), ts_field_decorator_ty);

        let ts_param_t_var = self.type_ctx.type_variable("This".to_string());
        let ts_param_decorator_ty = self.type_ctx.function_type(
            vec![ts_param_t_var, parameter_decorator_context_ty],
            void_ty,
            false,
        );
        let _ = self.symbols.define(Symbol {
            name: "TsParameterDecorator".to_string(),
            kind: SymbolKind::TypeAlias,
            ty: ts_param_decorator_ty,
            flags: SymbolFlags {
                is_exported: true,
                is_const: true,
                is_async: false,
                is_readonly: true,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        });
        self.type_ctx
            .register_named_type("TsParameterDecorator".to_string(), ts_param_decorator_ty);

        // ClassDecorator<T> = (target: Class<T>) => Class<T> | void
        // For simplicity, we register it as a function type with type variable
        let class_t_var = self.type_ctx.type_variable("T".to_string());
        let class_decorator_return = self.type_ctx.union_type(vec![class_t_var, void_ty]);
        let class_decorator_ty = self.type_ctx.function_type(
            vec![class_t_var], // target: Class<T> - using T as approximation
            class_decorator_return,
            false,
        );
        let class_decorator_symbol = Symbol {
            name: "ClassDecorator".to_string(),
            kind: SymbolKind::TypeAlias,
            ty: class_decorator_ty,
            flags: SymbolFlags {
                is_exported: true,
                is_const: true,
                is_async: false,
                is_readonly: true,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        };
        let _ = self.symbols.define(class_decorator_symbol);

        // MethodDecorator<F> = (method: F) => F
        let f_var = self.type_ctx.type_variable("F".to_string());
        let method_decorator_ty = self.type_ctx.function_type(vec![f_var], f_var, false);
        let method_decorator_symbol = Symbol {
            name: "MethodDecorator".to_string(),
            kind: SymbolKind::TypeAlias,
            ty: method_decorator_ty,
            flags: SymbolFlags {
                is_exported: true,
                is_const: true,
                is_async: false,
                is_readonly: true,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        };
        let _ = self.symbols.define(method_decorator_symbol);

        // FieldDecorator<T> = (target: T, fieldName: string) => void
        let field_t_var = self.type_ctx.type_variable("T".to_string());
        let field_decorator_ty =
            self.type_ctx
                .function_type(vec![field_t_var, string_ty], void_ty, false);
        let field_decorator_symbol = Symbol {
            name: "FieldDecorator".to_string(),
            kind: SymbolKind::TypeAlias,
            ty: field_decorator_ty,
            flags: SymbolFlags {
                is_exported: true,
                is_const: true,
                is_async: false,
                is_readonly: true,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        };
        let _ = self.symbols.define(field_decorator_symbol);

        // ParameterDecorator<T> = (target: T, methodName: string, parameterIndex: number) => void
        let param_t_var = self.type_ctx.type_variable("T".to_string());
        let param_decorator_ty =
            self.type_ctx
                .function_type(vec![param_t_var, string_ty, number_ty], void_ty, false);
        let param_decorator_symbol = Symbol {
            name: "ParameterDecorator".to_string(),
            kind: SymbolKind::TypeAlias,
            ty: param_decorator_ty,
            flags: SymbolFlags {
                is_exported: true,
                is_const: true,
                is_async: false,
                is_readonly: true,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        };
        let _ = self.symbols.define(param_decorator_symbol);
    }

    /// Register a single builtin class
    fn register_builtin_class(&mut self, class_sig: &super::builtins::BuiltinClass) {
        // Create type parameters map for resolving generic types
        let type_params: Vec<String> = class_sig.type_params.clone();

        // Create property signatures
        let properties: Vec<PropertySignature> = class_sig
            .properties
            .iter()
            .filter(|p| !p.is_static)
            .map(|p| PropertySignature {
                name: p.name.clone(),
                ty: self.parse_type_string(&p.ty, &type_params),
                optional: false,
                readonly: p
                    .descriptor
                    .as_ref()
                    .and_then(|d| d.writable)
                    .is_some_and(|w| !w),
                visibility: Default::default(),
            })
            .collect();

        let static_properties: Vec<PropertySignature> = class_sig
            .properties
            .iter()
            .filter(|p| p.is_static)
            .map(|p| PropertySignature {
                name: p.name.clone(),
                ty: self.parse_type_string(&p.ty, &type_params),
                optional: false,
                readonly: p
                    .descriptor
                    .as_ref()
                    .and_then(|d| d.writable)
                    .is_some_and(|w| !w),
                visibility: Default::default(),
            })
            .collect();

        // Create method signatures
        let methods: Vec<MethodSignature> = class_sig
            .methods
            .iter()
            .filter(|m| !m.is_static)
            .map(|m| {
                // Combine class type params with method type params for parsing
                let all_type_params: Vec<String> = type_params
                    .iter()
                    .chain(m.type_params.iter())
                    .cloned()
                    .collect();
                let param_types: Vec<TypeId> = m
                    .params
                    .iter()
                    .map(|(_, ty)| self.parse_type_string(ty, &all_type_params))
                    .collect();
                let return_ty = self.parse_type_string(&m.return_type, &all_type_params);
                let func_ty = self.type_ctx.function_type_with_min_params(
                    param_types,
                    return_ty,
                    false,
                    m.min_params,
                );
                MethodSignature {
                    name: m.name.clone(),
                    ty: func_ty,
                    type_params: m.type_params.clone(),
                    visibility: Default::default(),
                }
            })
            .collect();

        let static_methods: Vec<MethodSignature> = class_sig
            .methods
            .iter()
            .filter(|m| m.is_static)
            .map(|m| {
                // Combine class type params with method type params for parsing
                let all_type_params: Vec<String> = type_params
                    .iter()
                    .chain(m.type_params.iter())
                    .cloned()
                    .collect();
                let param_types: Vec<TypeId> = m
                    .params
                    .iter()
                    .map(|(_, ty)| self.parse_type_string(ty, &all_type_params))
                    .collect();
                let return_ty = self.parse_type_string(&m.return_type, &all_type_params);
                let func_ty = self.type_ctx.function_type_with_min_params(
                    param_types,
                    return_ty,
                    false,
                    m.min_params,
                );
                MethodSignature {
                    name: m.name.clone(),
                    ty: func_ty,
                    type_params: m.type_params.clone(),
                    visibility: Default::default(),
                }
            })
            .collect();

        let extends = self
            .builtin_parent_type_name(&class_sig.name)
            .and_then(|parent| self.type_ctx.lookup_named_type(parent));

        // Create the class type
        let class_type = ClassType {
            name: class_sig.name.clone(),
            type_params: type_params.clone(),
            properties,
            methods,
            static_properties,
            static_methods,
            extends,
            implements: vec![],
            is_abstract: false,
        };

        let class_ty = if let Some(symbol) = self.symbols.resolve(&class_sig.name) {
            if symbol.kind == SymbolKind::Class {
                self.type_ctx
                    .replace_type(symbol.ty, Type::Class(class_type));
                symbol.ty
            } else if let Some(existing) = self.type_ctx.lookup_named_type(&class_sig.name) {
                self.type_ctx
                    .replace_type(existing, Type::Class(class_type));
                existing
            } else {
                let id = self.type_ctx.intern(Type::Class(class_type));
                self.type_ctx
                    .register_named_type(class_sig.name.clone(), id);
                id
            }
        } else if let Some(existing) = self.type_ctx.lookup_named_type(&class_sig.name) {
            match self.type_ctx.get(existing) {
                Some(Type::Class(_)) => {
                    self.type_ctx
                        .replace_type(existing, Type::Class(class_type));
                    existing
                }
                _ => self.type_ctx.intern(Type::Class(class_type)),
            }
        } else {
            let id = self.type_ctx.intern(Type::Class(class_type));
            self.type_ctx
                .register_named_type(class_sig.name.clone(), id);
            id
        };

        // Register the class symbol
        let symbol = Symbol {
            name: class_sig.name.clone(),
            kind: SymbolKind::Class,
            ty: class_ty,
            flags: SymbolFlags {
                is_exported: true,
                is_const: true,
                is_async: false,
                is_readonly: false,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        };

        // Ignore errors for duplicate symbols (builtins might override each other)
        let _ = self.symbols.define(symbol);
    }

    fn builtin_parent_type_name(&self, class_name: &str) -> Option<&'static str> {
        // Runtime builtin signatures currently model many classes as flat declarations.
        // Preserve expected inheritance surface for error hierarchy in checker types.
        if class_name != "Error" && class_name.ends_with("Error") {
            return Some("Error");
        }
        if class_name != "Object" {
            return Some("Object");
        }
        None
    }

    fn implicit_object_base_type(&self, class_name: &str) -> Option<TypeId> {
        (class_name != "Object")
            .then(|| self.type_ctx.lookup_named_type("Object"))
            .flatten()
    }

    /// Register a single builtin function
    fn register_builtin_function(&mut self, func_sig: &super::builtins::BuiltinFunction) {
        let param_types: Vec<TypeId> = func_sig
            .params
            .iter()
            .map(|(_, ty)| self.parse_type_string(ty, &func_sig.type_params))
            .collect();
        let return_ty = self.parse_type_string(&func_sig.return_type, &func_sig.type_params);
        let func_ty = self.type_ctx.function_type(param_types, return_ty, false);

        let symbol = Symbol {
            name: func_sig.name.clone(),
            kind: SymbolKind::Function,
            ty: func_ty,
            flags: SymbolFlags {
                is_exported: true,
                is_const: true,
                is_async: false,
                is_readonly: false,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                column: 0,
            },
            referenced: false,
        };

        let _ = self.symbols.define(symbol);
    }

    /// Parse a type string into a TypeId
    ///
    /// Handles common type patterns:
    /// - Primitives: number, string, boolean, void, null
    /// - Type parameters: K, V, T (from type_params)
    /// - Arrays: Array<T>
    /// - Unions: T | null
    fn parse_type_string(&mut self, ty_str: &str, type_params: &[String]) -> TypeId {
        let ty_str = ty_str.trim();

        // Check for union types (e.g., "T | null")
        if ty_str.contains(" | ") {
            let parts: Vec<&str> = ty_str.split(" | ").collect();
            let type_ids: Vec<TypeId> = parts
                .iter()
                .map(|p| self.parse_type_string(p.trim(), type_params))
                .collect();
            return self.type_ctx.union_type(type_ids);
        }

        // Check for array types (e.g., "Array<T>")
        if ty_str.starts_with("Array<") && ty_str.ends_with('>') {
            let inner = &ty_str[6..ty_str.len() - 1];
            let elem_ty = self.parse_type_string(inner, type_params);
            return self.type_ctx.array_type(elem_ty);
        }

        // Check for tuple types (e.g., "[K, V]")
        if ty_str.starts_with('[') && ty_str.ends_with(']') {
            let inner = &ty_str[1..ty_str.len() - 1];
            let elem_types: Vec<TypeId> = inner
                .split(',')
                .map(|p| self.parse_type_string(p.trim(), type_params))
                .collect();
            return self.type_ctx.tuple_type(elem_types);
        }

        // Check for generic class types (e.g., "Set<T>", "Map<K, V>")
        if let Some(idx) = ty_str.find('<') {
            let class_name = &ty_str[..idx];
            let args_str = &ty_str[idx + 1..ty_str.len() - 1];
            let args: Vec<TypeId> = args_str
                .split(',')
                .map(|p| self.parse_type_string(p.trim(), type_params))
                .collect();
            use crate::parser::TypeContext as TC;
            return match class_name {
                TC::ARRAY_TYPE_NAME if args.len() == 1 => self.type_ctx.array_type(args[0]),
                TC::PROMISE_TYPE_NAME if args.len() == 1 => self.type_ctx.task_type(args[0]),
                "Task" if args.len() == 1 => self.type_ctx.task_type(args[0]),
                TC::CHANNEL_TYPE_NAME if args.len() == 1 => {
                    self.type_ctx.channel_type_with(args[0])
                }
                TC::SET_TYPE_NAME if args.len() == 1 => self.type_ctx.set_type_with(args[0]),
                TC::MAP_TYPE_NAME if args.len() == 2 => {
                    self.type_ctx.map_type_with(args[0], args[1])
                }
                TC::ARRAY_TYPE_NAME
                | TC::PROMISE_TYPE_NAME
                | TC::CHANNEL_TYPE_NAME
                | TC::SET_TYPE_NAME
                | TC::MAP_TYPE_NAME
                | "Task" => self.fallback_type(BinderFallbackReason::UnresolvedTypeParse),
                _ => {
                    if let Some(base_ty) = self.type_ctx.lookup_named_type(class_name) {
                        self.type_ctx
                            .intern(Type::Generic(crate::parser::types::ty::GenericType {
                                base: base_ty,
                                type_args: args,
                            }))
                    } else {
                        self.fallback_type(BinderFallbackReason::UnresolvedTypeParse)
                    }
                }
            };
        }

        // Check for type parameters
        if type_params.contains(&ty_str.to_string()) {
            return self.type_ctx.type_variable(ty_str.to_string());
        }

        // Primitive types
        match ty_str {
            "number" | "float" => self.type_ctx.number_type(),
            "int" => self.type_ctx.int_type(),
            "string" => self.type_ctx.string_type(),
            "boolean" => self.type_ctx.boolean_type(),
            "void" => self.type_ctx.void_type(),
            "null" => self.type_ctx.null_type(),
            _ => {
                // Could be a class name - look it up or return unknown
                if let Some(sym) = self.symbols.resolve(ty_str) {
                    sym.ty
                } else {
                    self.fallback_type(BinderFallbackReason::UnresolvedTypeParse)
                }
            }
        }
    }

    /// Resolve a parser Symbol to a String
    #[inline]
    fn resolve(&self, sym: crate::parser::Symbol) -> String {
        self.interner.resolve(sym).to_string()
    }

    /// Bind a module (entry point)
    ///
    /// Uses a two-pass approach like TypeScript:
    /// 1. Pre-pass: register all top-level class/function names as placeholder symbols
    ///    so forward references between classes work (e.g., ReadableStream referencing WritableStream)
    /// 2. Main pass: full binding with type resolution
    pub fn bind_module(mut self, module: &Module) -> Result<SymbolTable, Vec<BindError>> {
        let mut errors = Vec::new();
        // All file-level declarations live in a module scope under global (builtins).
        self.symbols.push_scope(ScopeKind::Module);

        // Pre-pass: collect all top-level declarations for forward references
        for stmt in &module.statements {
            if let Err(err) = self.prepass_stmt(stmt) {
                errors.push(err);
            }
        }

        // Main pass: full binding
        for stmt in &module.statements {
            if let Err(err) = self.bind_stmt(stmt) {
                errors.push(err);
            }
        }
        self.symbols.pop_scope();

        if errors.is_empty() {
            self.symbols
                .set_generic_type_alias_params(self.generic_type_alias_params);
            Ok(self.symbols)
        } else {
            Err(errors)
        }
    }

    /// Pre-pass: register top-level class, function, and type alias names as placeholder symbols.
    /// This enables forward references between declarations.
    fn prepass_stmt(&mut self, stmt: &Statement) -> Result<(), BindError> {
        match stmt {
            Statement::ClassDecl(class) => self.prepass_class(class),
            Statement::FunctionDecl(func) => self.prepass_function(func),
            Statement::TypeAliasDecl(alias) => self.prepass_type_alias(alias),
            Statement::ExportDecl(ExportDecl::Declaration(inner_stmt)) => {
                self.prepass_stmt(inner_stmt)
            }
            _ => Ok(()),
        }
    }

    /// Pre-pass nested declarations inside a function body scope.
    /// This enables forward references between function-local classes/functions/type aliases.
    fn prepass_stmt_nested(&mut self, stmt: &Statement) -> Result<(), BindError> {
        match stmt {
            Statement::ClassDecl(class) => self.prepass_class(class),
            Statement::FunctionDecl(func) => self.prepass_function(func),
            Statement::TypeAliasDecl(alias) => self.prepass_type_alias(alias),
            Statement::Block(block) => {
                for s in &block.statements {
                    self.prepass_stmt_nested(s)?;
                }
                Ok(())
            }
            Statement::If(if_stmt) => {
                self.prepass_stmt_nested(&if_stmt.then_branch)?;
                if let Some(else_branch) = &if_stmt.else_branch {
                    self.prepass_stmt_nested(else_branch)?;
                }
                Ok(())
            }
            Statement::While(while_stmt) => self.prepass_stmt_nested(&while_stmt.body),
            Statement::DoWhile(do_while) => self.prepass_stmt_nested(&do_while.body),
            Statement::For(for_stmt) => self.prepass_stmt_nested(&for_stmt.body),
            Statement::ForOf(for_of) => self.prepass_stmt_nested(&for_of.body),
            Statement::ForIn(for_in) => self.prepass_stmt_nested(&for_in.body),
            Statement::Labeled(labeled) => self.prepass_stmt_nested(&labeled.body),
            Statement::Switch(switch_stmt) => {
                for case in &switch_stmt.cases {
                    for s in &case.consequent {
                        self.prepass_stmt_nested(s)?;
                    }
                }
                Ok(())
            }
            Statement::Try(try_stmt) => {
                for s in &try_stmt.body.statements {
                    self.prepass_stmt_nested(s)?;
                }
                if let Some(catch) = &try_stmt.catch_clause {
                    for s in &catch.body.statements {
                        self.prepass_stmt_nested(s)?;
                    }
                }
                if let Some(finally) = &try_stmt.finally_clause {
                    for s in &finally.statements {
                        self.prepass_stmt_nested(s)?;
                    }
                }
                Ok(())
            }
            Statement::ExportDecl(ExportDecl::Declaration(inner_stmt)) => {
                self.prepass_stmt_nested(inner_stmt)
            }
            _ => Ok(()),
        }
    }

    /// Pre-pass: register a class name with a placeholder type
    fn prepass_class(&mut self, class: &ClassDecl) -> Result<(), BindError> {
        let class_name = self.resolve(class.name.name);

        // Skip only when the declaration already exists in THIS scope.
        // Module/local scopes may intentionally shadow global builtins.
        if self.has_symbol_in_current_scope(&class_name) {
            return Ok(());
        }

        let type_param_names: Vec<String> = class
            .type_params
            .as_ref()
            .map(|params| params.iter().map(|p| self.resolve(p.name.name)).collect())
            .unwrap_or_default();

        let placeholder = ClassType {
            name: class_name.clone(),
            type_params: type_param_names,
            properties: vec![],
            methods: vec![],
            static_properties: vec![],
            static_methods: vec![],
            extends: self.implicit_object_base_type(&class_name),
            implements: vec![],
            is_abstract: class.is_abstract,
        };
        let class_ty = self.type_ctx.intern(Type::Class(placeholder));
        // Expose class name for reference-based annotations (including forward refs).
        self.type_ctx
            .register_named_type(class_name.clone(), class_ty);

        let symbol = Symbol {
            name: class_name.clone(),
            kind: SymbolKind::Class,
            ty: class_ty,
            flags: SymbolFlags {
                is_exported: false,
                is_const: true,
                is_async: false,
                is_readonly: false,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: class.name.span,
            referenced: false,
        };

        self.symbols
            .define(symbol)
            .map_err(|err| BindError::DuplicateSymbol {
                name: err.name,
                original: err.original,
                duplicate: err.duplicate,
            })
    }

    /// Pre-pass: register a function name with a placeholder type
    fn prepass_function(&mut self, func: &FunctionDecl) -> Result<(), BindError> {
        let func_name = self.resolve(func.name.name);

        // Skip only when this scope already has the declaration.
        // In JS mode, we allow redeclaration by replacing the existing symbol.
        if self.has_symbol_in_current_scope(&func_name) {
            if self.is_js_mode() {
                // Replace the existing symbol so the last declaration wins.
                let unknown_ty = self.type_ctx.unknown_type();
                let symbol = Symbol {
                    name: func_name.clone(),
                    kind: SymbolKind::Function,
                    ty: unknown_ty,
                    flags: SymbolFlags {
                        is_exported: false,
                        is_const: true,
                        is_async: func.is_async,
                        is_readonly: false,
                        is_imported: false,
                    },
                    scope_id: self.symbols.current_scope_id(),
                    span: func.name.span,
                    referenced: false,
                };
                let scope_id = self.symbols.current_scope_id();
                self.symbols.replace_in_scope(scope_id, symbol);
            }
            return Ok(());
        }

        let unknown_ty = self.type_ctx.unknown_type();
        let symbol = Symbol {
            name: func_name.clone(),
            kind: SymbolKind::Function,
            ty: unknown_ty,
            flags: SymbolFlags {
                is_exported: false,
                is_const: true,
                is_async: func.is_async,
                is_readonly: false,
                is_imported: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: func.name.span,
            referenced: false,
        };

        self.symbols
            .define(symbol)
            .map_err(|err| BindError::DuplicateSymbol {
                name: err.name,
                original: err.original,
                duplicate: err.duplicate,
            })
    }

    /// Pre-pass: register a type alias name with a placeholder type.
    /// This enables self-referential type aliases (e.g., JsonValue that references itself)
    /// and forward references between type aliases.
    fn prepass_type_alias(&mut self, alias: &TypeAliasDecl) -> Result<(), BindError> {
        let alias_name = self.resolve(alias.name.name);

        // Skip only when this scope already has the declaration.
        if self.has_symbol_in_current_scope(&alias_name) {
            return Ok(());
        }

        // Create a UNIQUE placeholder ObjectType for this type alias (like classes
        // use unique ClassType placeholders with the class name). A named marker
        // property ensures each placeholder is distinct — without this, intern()
        // would dedup all empty ObjectTypes to the same TypeId.
        use crate::parser::types::ty::PropertySignature;
        let unknown_ty = self.type_ctx.unknown_type();
        let placeholder_ty = self.type_ctx.object_type(vec![PropertySignature {
            name: format!("__placeholder_{}", alias_name),
            ty: unknown_ty,
            optional: false,
            readonly: false,
            visibility: crate::parser::ast::Visibility::Public,
        }]);
        let symbol = Symbol {
            name: alias_name.clone(),
            kind: SymbolKind::TypeAlias,
            ty: placeholder_ty,
            flags: SymbolFlags::default(),
            scope_id: self.symbols.current_scope_id(),
            span: alias.name.span,
            referenced: false,
        };

        self.symbols
            .define(symbol)
            .map_err(|err| BindError::DuplicateSymbol {
                name: err.name,
                original: err.original,
                duplicate: err.duplicate,
            })
    }

    /// Bind a statement
    fn bind_stmt(&mut self, stmt: &Statement) -> Result<(), BindError> {
        match stmt {
            Statement::VariableDecl(decl) => self.bind_var_decl(decl),
            Statement::FunctionDecl(func) => self.bind_function(func),
            Statement::ClassDecl(class) => self.bind_class(class),
            Statement::TypeAliasDecl(alias) => self.bind_type_alias(alias),
            Statement::Block(block) => self.bind_block(block),
            Statement::If(if_stmt) => self.bind_if(if_stmt),
            Statement::Switch(switch_stmt) => self.bind_switch(switch_stmt),
            Statement::While(while_stmt) => self.bind_while(while_stmt),
            Statement::For(for_stmt) => self.bind_for(for_stmt),
            Statement::ForOf(for_of) => self.bind_for_of(for_of),
            Statement::ForIn(for_in) => self.bind_for_in(for_in),
            Statement::Labeled(labeled) => self.bind_stmt(&labeled.body),
            Statement::Try(try_stmt) => self.bind_try(try_stmt),
            Statement::ExportDecl(export) => self.bind_export(export),
            // ImportDecl is handled during pre-binding phase
            Statement::ImportDecl(_) => Ok(()),
            // Other statements don't introduce bindings
            _ => Ok(()),
        }
    }

    /// Bind an export declaration
    fn bind_export(&mut self, export: &ExportDecl) -> Result<(), BindError> {
        match export {
            ExportDecl::Declaration(stmt) => {
                // First bind the inner statement
                self.bind_stmt(stmt)?;

                // Then mark the declared symbol as exported
                // Extract the name from the statement
                if let Some(name) = self.get_declaration_name(stmt) {
                    self.symbols.mark_exported(&name);
                }
                Ok(())
            }
            ExportDecl::Named { specifiers, .. } => {
                // Mark each named export as exported
                for spec in specifiers {
                    let name = self.resolve(spec.name.name);
                    // The symbol should already be defined; just mark it as exported
                    self.symbols.mark_exported(&name);
                }
                Ok(())
            }
            ExportDecl::All { .. } => {
                // Re-exports are handled at module linking time, not binding time
                Ok(())
            }
            ExportDecl::Default { expression, span } => {
                // export default <expr> — create a "default" symbol with the expression's type
                // For identifier expressions (e.g., `export default logger`), copy the symbol's type
                if let Expression::Identifier(ident) = expression.as_ref() {
                    let name = self.interner.resolve(ident.name).to_string();
                    if let Some(sym) = self.symbols.resolve(&name) {
                        let default_sym = Symbol {
                            name: "default".to_string(),
                            kind: sym.kind,
                            ty: sym.ty,
                            flags: SymbolFlags {
                                is_exported: true,
                                is_const: sym.flags.is_const,
                                is_async: sym.flags.is_async,
                                is_readonly: sym.flags.is_readonly,
                                is_imported: false,
                            },
                            scope_id: self.symbols.current_scope_id(),
                            span: *span,
                            referenced: false,
                        };
                        let _ = self.symbols.define(default_sym);
                    }
                } else {
                    // For non-identifier expressions (e.g., `export default new Logger()`),
                    // create a default symbol with unknown type (type checker will infer)
                    let unknown_ty = self.type_ctx.unknown_type();
                    let default_sym = Symbol {
                        name: "default".to_string(),
                        kind: SymbolKind::Variable,
                        ty: unknown_ty,
                        flags: SymbolFlags {
                            is_exported: true,
                            is_const: true,
                            is_async: false,
                            is_readonly: false,
                            is_imported: false,
                        },
                        scope_id: self.symbols.current_scope_id(),
                        span: *span,
                        referenced: false,
                    };
                    let _ = self.symbols.define(default_sym);
                }
                Ok(())
            }
        }
    }

    /// Get the name of a declaration statement (for export marking)
    fn get_declaration_name(&self, stmt: &Statement) -> Option<String> {
        match stmt {
            Statement::VariableDecl(decl) => {
                if let Pattern::Identifier(ident) = &decl.pattern {
                    Some(self.interner.resolve(ident.name).to_string())
                } else {
                    None
                }
            }
            Statement::FunctionDecl(func) => {
                Some(self.interner.resolve(func.name.name).to_string())
            }
            Statement::ClassDecl(class) => Some(self.interner.resolve(class.name.name).to_string()),
            Statement::TypeAliasDecl(alias) => {
                Some(self.interner.resolve(alias.name.name).to_string())
            }
            _ => None,
        }
    }

    fn array_element_type_for_pattern(&mut self, ty: TypeId) -> TypeId {
        match self.type_ctx.get(ty).cloned() {
            Some(Type::Array(arr)) => arr.element,
            Some(Type::TypeVar(tv)) => tv
                .constraint
                .map(|c| self.array_element_type_for_pattern(c))
                .unwrap_or_else(|| self.fallback_type(BinderFallbackReason::PatternBinding)),
            Some(Type::Union(union)) => {
                let mut elements = Vec::new();
                for member in union.members {
                    let elem_ty = self.array_element_type_for_pattern(member);
                    if !elements.contains(&elem_ty) {
                        elements.push(elem_ty);
                    }
                }
                if elements.is_empty() {
                    self.fallback_type(BinderFallbackReason::PatternBinding)
                } else if elements.len() == 1 {
                    elements[0]
                } else {
                    self.type_ctx.union_type(elements)
                }
            }
            _ => self.fallback_type(BinderFallbackReason::PatternBinding),
        }
    }

    fn object_property_type_for_pattern(&mut self, ty: TypeId, prop_name: &str) -> TypeId {
        match self.type_ctx.get(ty).cloned() {
            Some(Type::Object(obj)) => obj
                .properties
                .iter()
                .find(|p| p.name == prop_name)
                .map(|p| p.ty)
                .or_else(|| obj.index_signature.map(|(_, sig_ty)| sig_ty))
                .unwrap_or_else(|| self.fallback_type(BinderFallbackReason::PatternBinding)),
            Some(Type::Class(cls)) => cls
                .properties
                .iter()
                .find(|p| p.name == prop_name)
                .map(|p| p.ty)
                .unwrap_or_else(|| self.fallback_type(BinderFallbackReason::PatternBinding)),
            Some(Type::TypeVar(tv)) => tv
                .constraint
                .map(|c| self.object_property_type_for_pattern(c, prop_name))
                .unwrap_or_else(|| self.fallback_type(BinderFallbackReason::PatternBinding)),
            Some(Type::Union(union)) => {
                let mut out = Vec::new();
                for member in union.members {
                    let member_ty = self.object_property_type_for_pattern(member, prop_name);
                    if !out.contains(&member_ty) {
                        out.push(member_ty);
                    }
                }
                if out.is_empty() {
                    self.fallback_type(BinderFallbackReason::PatternBinding)
                } else if out.len() == 1 {
                    out[0]
                } else {
                    self.type_ctx.union_type(out)
                }
            }
            _ => self.fallback_type(BinderFallbackReason::PatternBinding),
        }
    }

    /// Recursively register all identifiers in a pattern as variable symbols.
    fn bind_pattern_names(
        &mut self,
        pattern: &Pattern,
        ty: TypeId,
        is_const: bool,
        is_imported: bool,
    ) -> Result<(), BindError> {
        let scope_id = self.symbols.current_scope_id();
        self.bind_pattern_names_in_scope(pattern, ty, is_const, is_imported, scope_id, false)
    }

    fn bind_pattern_names_in_scope(
        &mut self,
        pattern: &Pattern,
        ty: TypeId,
        is_const: bool,
        is_imported: bool,
        scope_id: ScopeId,
        allow_duplicate_var: bool,
    ) -> Result<(), BindError> {
        match pattern {
            Pattern::Identifier(ident) => {
                let name = self.resolve(ident.name);
                if let Some(existing) = self.symbols.resolve_from_scope(&name, scope_id) {
                    if existing.scope_id == scope_id && existing.kind == SymbolKind::TypeAlias {
                        // Allow value binding to coexist with a same-name type alias in this scope.
                        // The checker resolves identifiers by TypeId, so the type alias symbol
                        // can service both type and value references for helper-generated shims.
                        return Ok(());
                    }
                    if allow_duplicate_var
                        && existing.scope_id == scope_id
                        && existing.kind == SymbolKind::Variable
                        && !existing.flags.is_const
                    {
                        return Ok(());
                    }
                }
                let symbol = Symbol {
                    name,
                    kind: SymbolKind::Variable,
                    ty,
                    flags: SymbolFlags {
                        is_exported: false,
                        is_const,
                        is_async: false,
                        is_readonly: false,
                        is_imported,
                    },
                    scope_id,
                    span: ident.span,
                    referenced: false,
                };
                self.symbols
                    .define_in_scope(scope_id, symbol)
                    .map_err(|err| BindError::DuplicateSymbol {
                        name: err.name,
                        original: err.original,
                        duplicate: err.duplicate,
                    })?;
            }
            Pattern::Array(array_pat) => {
                // Extract element type from array type annotation
                let elem_ty = self.array_element_type_for_pattern(ty);
                for elem in array_pat.elements.iter().flatten() {
                    self.bind_pattern_names_in_scope(
                        &elem.pattern,
                        elem_ty,
                        is_const,
                        is_imported,
                        scope_id,
                        allow_duplicate_var,
                    )?;
                }
                if let Some(rest) = &array_pat.rest {
                    self.bind_pattern_names_in_scope(
                        rest,
                        ty,
                        is_const,
                        is_imported,
                        scope_id,
                        allow_duplicate_var,
                    )?;
                }
            }
            Pattern::Object(obj_pat) => {
                // Extract property types from object type annotation
                for prop in &obj_pat.properties {
                    let prop_ty = match &prop.key {
                        crate::parser::ast::PropertyKey::Identifier(id) => {
                            let prop_name = self.resolve(id.name);
                            self.object_property_type_for_pattern(ty, &prop_name)
                        }
                        crate::parser::ast::PropertyKey::StringLiteral(lit) => {
                            let prop_name = self.resolve(lit.value);
                            self.object_property_type_for_pattern(ty, &prop_name)
                        }
                        crate::parser::ast::PropertyKey::IntLiteral(lit) => {
                            self.object_property_type_for_pattern(ty, &lit.value.to_string())
                        }
                        crate::parser::ast::PropertyKey::Computed(_) => {
                            self.inference_fallback_type()
                        }
                    };
                    self.bind_pattern_names_in_scope(
                        &prop.value,
                        prop_ty,
                        is_const,
                        is_imported,
                        scope_id,
                        allow_duplicate_var,
                    )?;
                }
                if let Some(rest_ident) = &obj_pat.rest {
                    let name = self.resolve(rest_ident.name);
                    if allow_duplicate_var {
                        if let Some(existing) = self.symbols.resolve_from_scope(&name, scope_id) {
                            if existing.scope_id == scope_id
                                && existing.kind == SymbolKind::Variable
                                && !existing.flags.is_const
                            {
                                return Ok(());
                            }
                        }
                    }
                    let symbol = Symbol {
                        name,
                        kind: SymbolKind::Variable,
                        ty,
                        flags: SymbolFlags {
                            is_exported: false,
                            is_const,
                            is_async: false,
                            is_readonly: false,
                            is_imported,
                        },
                        scope_id,
                        span: rest_ident.span,
                        referenced: false,
                    };
                    self.symbols
                        .define_in_scope(scope_id, symbol)
                        .map_err(|err| BindError::DuplicateSymbol {
                            name: err.name,
                            original: err.original,
                            duplicate: err.duplicate,
                        })?;
                }
            }
            Pattern::Rest(rest_pat) => {
                self.bind_pattern_names_in_scope(
                    &rest_pat.argument,
                    ty,
                    is_const,
                    is_imported,
                    scope_id,
                    allow_duplicate_var,
                )?;
            }
        }
        Ok(())
    }

    fn expression_uses_linker_dep_binding(&self, expr: &Expression) -> bool {
        match expr {
            Expression::Identifier(ident) => self.resolve(ident.name).starts_with("__raya_dep_"),
            Expression::Array(arr) => arr.elements.iter().flatten().any(|elem| match elem {
                crate::parser::ast::ArrayElement::Expression(e)
                | crate::parser::ast::ArrayElement::Spread(e) => {
                    self.expression_uses_linker_dep_binding(e)
                }
            }),
            Expression::Object(obj) => obj.properties.iter().any(|prop| match prop {
                crate::parser::ast::ObjectProperty::Property(p) => {
                    self.expression_uses_linker_dep_binding(&p.value)
                }
                crate::parser::ast::ObjectProperty::Spread(s) => {
                    self.expression_uses_linker_dep_binding(&s.argument)
                }
            }),
            Expression::Unary(un) => self.expression_uses_linker_dep_binding(&un.operand),
            Expression::Binary(bin) => {
                self.expression_uses_linker_dep_binding(&bin.left)
                    || self.expression_uses_linker_dep_binding(&bin.right)
            }
            Expression::Assignment(assign) => {
                self.expression_uses_linker_dep_binding(&assign.left)
                    || self.expression_uses_linker_dep_binding(&assign.right)
            }
            Expression::Logical(logical) => {
                self.expression_uses_linker_dep_binding(&logical.left)
                    || self.expression_uses_linker_dep_binding(&logical.right)
            }
            Expression::Conditional(cond) => {
                self.expression_uses_linker_dep_binding(&cond.test)
                    || self.expression_uses_linker_dep_binding(&cond.consequent)
                    || self.expression_uses_linker_dep_binding(&cond.alternate)
            }
            Expression::Call(call) => {
                self.expression_uses_linker_dep_binding(&call.callee)
                    || call
                        .arguments
                        .iter()
                        .any(|arg| self.expression_uses_linker_dep_binding(arg.expression()))
            }
            Expression::AsyncCall(call) => {
                self.expression_uses_linker_dep_binding(&call.callee)
                    || call
                        .arguments
                        .iter()
                        .any(|arg| self.expression_uses_linker_dep_binding(arg.expression()))
            }
            Expression::Member(member) => self.expression_uses_linker_dep_binding(&member.object),
            Expression::Index(index) => {
                self.expression_uses_linker_dep_binding(&index.object)
                    || self.expression_uses_linker_dep_binding(&index.index)
            }
            Expression::New(new_expr) => {
                self.expression_uses_linker_dep_binding(&new_expr.callee)
                    || new_expr
                        .arguments
                        .iter()
                        .any(|arg| self.expression_uses_linker_dep_binding(arg.expression()))
            }
            Expression::Arrow(arrow) => match &arrow.body {
                crate::parser::ast::ArrowBody::Expression(expr) => {
                    self.expression_uses_linker_dep_binding(expr)
                }
                crate::parser::ast::ArrowBody::Block(_) => false,
            },
            Expression::Function(_) => false,
            Expression::Await(await_expr) => {
                self.expression_uses_linker_dep_binding(&await_expr.argument)
            }
            Expression::Typeof(typeof_expr) => {
                self.expression_uses_linker_dep_binding(&typeof_expr.argument)
            }
            Expression::Parenthesized(paren) => {
                self.expression_uses_linker_dep_binding(&paren.expression)
            }
            Expression::InstanceOf(instanceof) => {
                self.expression_uses_linker_dep_binding(&instanceof.object)
            }
            Expression::In(in_expr) => {
                self.expression_uses_linker_dep_binding(&in_expr.property)
                    || self.expression_uses_linker_dep_binding(&in_expr.object)
            }
            Expression::TypeCast(cast) => self.expression_uses_linker_dep_binding(&cast.object),
            Expression::TemplateLiteral(tpl) => tpl.parts.iter().any(|part| match part {
                crate::parser::ast::TemplatePart::Expression(expr) => {
                    self.expression_uses_linker_dep_binding(expr)
                }
                crate::parser::ast::TemplatePart::String(_) => false,
            }),
            Expression::TaggedTemplate(tagged) => {
                self.expression_uses_linker_dep_binding(&tagged.tag)
                    || tagged.template.parts.iter().any(|part| match part {
                        crate::parser::ast::TemplatePart::Expression(expr) => {
                            self.expression_uses_linker_dep_binding(expr)
                        }
                        crate::parser::ast::TemplatePart::String(_) => false,
                    })
            }
            Expression::DynamicImport(import_expr) => {
                self.expression_uses_linker_dep_binding(&import_expr.source)
            }
            Expression::JsxElement(_)
            | Expression::JsxFragment(_)
            | Expression::IntLiteral(_)
            | Expression::FloatLiteral(_)
            | Expression::StringLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::This(_)
            | Expression::Super(_)
            | Expression::RegexLiteral(_) => false,
        }
    }

    /// Bind variable declaration
    fn bind_var_decl(&mut self, decl: &VariableDecl) -> Result<(), BindError> {
        // Resolve type annotation or use unknown
        let ty = match &decl.type_annotation {
            Some(ty_annot) => self.resolve_type_annotation(ty_annot)?,
            None => self.inference_fallback_type(),
        };

        let is_const = matches!(decl.kind, VariableKind::Const);
        let is_imported = decl
            .initializer
            .as_ref()
            .is_some_and(|init| self.expression_uses_linker_dep_binding(init));
        let scope_id = self.binding_scope_for_variable_kind(decl.kind);
        let allow_duplicate_var = self.is_js_mode() && matches!(decl.kind, VariableKind::Var);
        self.bind_pattern_names_in_scope(
            &decl.pattern,
            ty,
            is_const,
            is_imported,
            scope_id,
            allow_duplicate_var,
        )
    }

    /// Bind function declaration
    fn bind_function(&mut self, func: &FunctionDecl) -> Result<(), BindError> {
        let func_name = self.resolve(func.name.name);

        // Detect duplicate function declarations
        // In JS mode, function redeclaration in the same scope is allowed (ES spec).
        if self.reject_duplicate_top_level_declarations && !self.is_js_mode() {
            if let Some(&original_span) = self.bound_functions.get(&func_name) {
                return Err(BindError::DuplicateSymbol {
                    name: func_name,
                    original: original_span,
                    duplicate: func.name.span,
                });
            }
            self.bound_functions
                .insert(func_name.clone(), func.name.span);
        }

        // Get parent scope ID before pushing (for defining function symbol)
        let parent_scope_id = self.symbols.current_scope_id();

        // Push function scope - type parameters, parameters, and body all share this scope
        self.symbols.push_scope(ScopeKind::Function);

        // Pre-pass function-local declarations for forward references inside this scope.
        for stmt in &func.body.statements {
            self.prepass_stmt_nested(stmt)?;
        }

        // Bind type parameters first (before resolving parameter types)
        if let Some(ref type_params) = func.type_params {
            for type_param in type_params {
                let param_name = self.resolve(type_param.name.name);
                // Resolve constraint if present (e.g., T extends HasLength)
                let constraint_ty = if let Some(ref constraint) = type_param.constraint {
                    self.resolve_type_annotation(constraint).ok()
                } else {
                    None
                };
                // Create a type variable for this type parameter
                let type_var = self
                    .type_ctx
                    .type_variable_with_constraint(param_name.clone(), constraint_ty);

                let tp_symbol = Symbol {
                    name: param_name,
                    kind: SymbolKind::TypeParameter,
                    ty: type_var,
                    flags: SymbolFlags::default(),
                    scope_id: self.symbols.current_scope_id(),
                    span: type_param.span,
                    referenced: false,
                };

                self.symbols
                    .define(tp_symbol)
                    .map_err(|err| BindError::DuplicateSymbol {
                        name: err.name,
                        original: err.original,
                        duplicate: err.duplicate,
                    })?;
            }
        }

        // Build function type from parameters and return type
        // Type parameters are now in scope, so we can resolve them in param types
        let mut param_types = Vec::new();
        let mut rest_param_ty = None;

        for param in &func.params {
            if param.is_rest {
                // Validate and extract rest parameter type
                if let Some(ref type_ann) = param.type_annotation {
                    let param_ty = self.resolve_type_annotation(type_ann)?;
                    if self.is_valid_rest_param_type(param_ty) {
                        rest_param_ty = Some(param_ty);
                    } else {
                        return Err(BindError::InvalidRestParameter {
                            message:
                                "Rest parameter type must be an array/tuple (e.g., string[] or [number, string])"
                                    .to_string(),
                            span: type_ann.span,
                        });
                    }
                } else {
                    return Err(BindError::InvalidRestParameter {
                        message:
                            "Rest parameter must have a type annotation (e.g., ...args: string[])"
                                .to_string(),
                        span: param.span,
                    });
                }
                // Skip rest parameter from regular params
                continue;
            }

            let param_ty = match &param.type_annotation {
                Some(ty_annot) => self.resolve_type_annotation(ty_annot)?,
                None => {
                    if self.policy.allow_implicit_any {
                        self.type_ctx.any_type()
                    } else {
                        self.type_ctx.unknown_type()
                    }
                }
            };
            param_types.push(param_ty);
        }

        let declared_return_ty = match &func.return_type {
            Some(ty_annot) => self.resolve_type_annotation(ty_annot)?,
            None => {
                if self.mode == TypeSystemMode::Js {
                    self.type_ctx.unknown_type()
                } else {
                    self.type_ctx.void_type()
                }
            }
        };
        // Async functions are represented as `is_async = true` with inner return type `T`.
        // If user annotates `Promise<T>`, unwrap it here to avoid `Promise<Promise<T>>` function types.
        let return_ty = if func.is_async {
            match self.type_ctx.get(declared_return_ty) {
                Some(Type::Task(task_ty)) => task_ty.result,
                _ => declared_return_ty,
            }
        } else {
            declared_return_ty
        };

        // Validate parameter ordering: required params must come before optional/default params
        self.validate_param_order(&func.params)?;

        // Validate rest parameter is last and only one
        if let Some(rest_idx) = func.params.iter().position(|p| p.is_rest) {
            // Check rest is last
            if rest_idx < func.params.len() - 1 {
                return Err(BindError::InvalidRestParameter {
                    message: "Rest parameter must be last".to_string(),
                    span: func.params[rest_idx].span,
                });
            }
        }

        // Count required params (those without default values and not optional)
        // Exclude rest parameter from count
        let min_params = if self.mode == TypeSystemMode::Js {
            0
        } else {
            func.params
                .iter()
                .filter(|p| !p.is_rest)
                .filter(|p| p.default_value.is_none() && !p.optional)
                .count()
        };

        // Create function type with rest parameter
        let func_ty = self.type_ctx.function_type_with_rest(
            param_types.clone(),
            return_ty,
            func.is_async,
            min_params,
            rest_param_ty,
        );

        let symbol = Symbol {
            name: func_name.clone(),
            kind: SymbolKind::Function,
            ty: func_ty,
            flags: SymbolFlags {
                is_exported: false,
                is_const: true,
                is_async: func.is_async,
                is_readonly: false,
                is_imported: false,
            },
            scope_id: parent_scope_id,
            span: func.name.span,
            referenced: false,
        };

        // Define function symbol in parent scope (so it can be called recursively).
        // If pre-registered by pre-pass, replace it so span/flags match the active declaration.
        if let Some(existing) = self.symbol_in_scope(parent_scope_id, &func_name) {
            if existing.kind == SymbolKind::Function && !existing.flags.is_imported {
                self.symbols.replace_in_scope(parent_scope_id, symbol);
            } else {
                if let Err(err) = self
                    .symbols
                    .define_in_scope(parent_scope_id, symbol.clone())
                {
                    if self.is_js_mode() {
                        self.symbols.replace_in_scope(parent_scope_id, symbol);
                    } else {
                        return Err(BindError::DuplicateSymbol {
                            name: err.name,
                            original: err.original,
                            duplicate: err.duplicate,
                        });
                    }
                }
            }
        } else {
            if let Err(err) = self
                .symbols
                .define_in_scope(parent_scope_id, symbol.clone())
            {
                if self.is_js_mode() {
                    self.symbols.replace_in_scope(parent_scope_id, symbol);
                } else {
                    return Err(BindError::DuplicateSymbol {
                        name: err.name,
                        original: err.original,
                        duplicate: err.duplicate,
                    });
                }
            }
        }

        // Bind parameters in the function scope
        // Note: param types are already resolved above, we use param_types[i]
        // For rest parameters, we use the rest_param_ty
        let mut non_rest_idx = 0;
        for param in func.params.iter() {
            // Extract identifier from pattern (simplified)
            let (param_name, param_span) = match &param.pattern {
                Pattern::Identifier(ident) => (self.resolve(ident.name), ident.span),
                _ => continue, // Skip destructuring for now
            };

            // Get the type for this parameter
            let param_ty = if param.is_rest {
                // Use the rest parameter type
                match rest_param_ty {
                    Some(ty) => ty,
                    None => self.inference_fallback_type(),
                }
            } else {
                // Use the regular parameter type
                if non_rest_idx >= param_types.len() {
                    return Err(BindError::InvalidRestParameter {
                        message: "Parameter type index out of bounds".to_string(),
                        span: param.span,
                    });
                }
                let ty = param_types[non_rest_idx];
                non_rest_idx += 1;
                ty
            };

            let param_symbol = Symbol {
                name: param_name,
                kind: SymbolKind::Variable,
                ty: param_ty,
                flags: SymbolFlags {
                    is_exported: false,
                    is_const: false, // function parameters are mutable
                    is_async: false,
                    is_readonly: false,
                    is_imported: false,
                },
                scope_id: self.symbols.current_scope_id(),
                span: param_span,
                referenced: false,
            };

            self.symbols
                .define(param_symbol)
                .map_err(|err| BindError::DuplicateSymbol {
                    name: err.name,
                    original: err.original,
                    duplicate: err.duplicate,
                })?;
        }

        // Bind body statements
        for stmt in &func.body.statements {
            self.bind_stmt(stmt)?;
        }

        self.symbols.pop_scope();
        Ok(())
    }

    /// Bind class declaration
    fn bind_class(&mut self, class: &ClassDecl) -> Result<(), BindError> {
        use crate::parser::types::ty::{ClassType, MethodSignature, PropertySignature, Type};

        let class_name = self.resolve(class.name.name);

        // Detect duplicate class declarations using the bound_classes set.
        if self.reject_duplicate_top_level_declarations {
            if let Some(&original_span) = self.bound_classes.get(&class_name) {
                return Err(BindError::DuplicateSymbol {
                    name: class_name,
                    original: original_span,
                    duplicate: class.name.span,
                });
            }
            self.bound_classes
                .insert(class_name.clone(), class.name.span);
        }

        // Collect type parameters (K, V, T, etc.)
        let type_param_names: Vec<String> = class
            .type_params
            .as_ref()
            .map(|params| params.iter().map(|p| self.resolve(p.name.name)).collect())
            .unwrap_or_default();

        // Create a placeholder class type for self-references in methods
        let placeholder_type = ClassType {
            name: class_name.clone(),
            type_params: type_param_names.clone(),
            properties: vec![],
            methods: vec![],
            static_properties: vec![],
            static_methods: vec![],
            extends: self.implicit_object_base_type(&class_name),
            implements: vec![],
            is_abstract: class.is_abstract,
        };
        let class_ty = self.type_ctx.intern(Type::Class(placeholder_type));

        // Store the scope ID where the class is defined (for later update)
        let class_definition_scope = self.symbols.current_scope_id();

        // If the class was already registered by the pre-pass, replace it;
        // otherwise define it now (handles non-top-level classes)
        if let Some(existing) = self.symbol_in_scope(class_definition_scope, &class_name) {
            if existing.kind == SymbolKind::Class && !existing.flags.is_imported {
                self.symbols.replace_in_scope(
                    class_definition_scope,
                    Symbol {
                        name: class_name.clone(),
                        kind: SymbolKind::Class,
                        ty: class_ty,
                        flags: SymbolFlags {
                            is_exported: false,
                            is_const: true,
                            is_async: false,
                            is_readonly: false,
                            is_imported: false,
                        },
                        scope_id: class_definition_scope,
                        span: class.name.span,
                        referenced: false,
                    },
                );
            } else {
                let symbol = Symbol {
                    name: class_name.clone(),
                    kind: SymbolKind::Class,
                    ty: class_ty,
                    flags: SymbolFlags {
                        is_exported: false,
                        is_const: true,
                        is_async: false,
                        is_readonly: false,
                        is_imported: false,
                    },
                    scope_id: class_definition_scope,
                    span: class.name.span,
                    referenced: false,
                };
                self.symbols
                    .define(symbol)
                    .map_err(|err| BindError::DuplicateSymbol {
                        name: err.name,
                        original: err.original,
                        duplicate: err.duplicate,
                    })?;
            }
        } else {
            let symbol = Symbol {
                name: class_name.clone(),
                kind: SymbolKind::Class,
                ty: class_ty,
                flags: SymbolFlags {
                    is_exported: false,
                    is_const: true,
                    is_async: false,
                    is_readonly: false,
                    is_imported: false,
                },
                scope_id: class_definition_scope,
                span: class.name.span,
                referenced: false,
            };
            self.symbols
                .define(symbol)
                .map_err(|err| BindError::DuplicateSymbol {
                    name: err.name,
                    original: err.original,
                    duplicate: err.duplicate,
                })?;
        }

        // Enter class scope for type parameters
        self.symbols.push_scope(ScopeKind::Class);

        // Register type parameters (with constraints) in class scope
        if let Some(ref class_type_params) = class.type_params {
            for type_param in class_type_params {
                let type_param_name = self.resolve(type_param.name.name);
                let constraint_ty = if let Some(ref constraint) = type_param.constraint {
                    self.resolve_type_annotation(constraint).ok()
                } else {
                    None
                };
                let type_var = self
                    .type_ctx
                    .type_variable_with_constraint(type_param_name.clone(), constraint_ty);
                let symbol = Symbol {
                    name: type_param_name,
                    kind: SymbolKind::TypeAlias,
                    ty: type_var,
                    flags: SymbolFlags::default(),
                    scope_id: self.symbols.current_scope_id(),
                    span: type_param.span,
                    referenced: false,
                };
                let _ = self.symbols.define(symbol);
            }
        } else {
            for type_param_name in &type_param_names {
                let type_var = self.type_ctx.type_variable(type_param_name.clone());
                let symbol = Symbol {
                    name: type_param_name.clone(),
                    kind: SymbolKind::TypeAlias,
                    ty: type_var,
                    flags: SymbolFlags::default(),
                    scope_id: self.symbols.current_scope_id(),
                    span: Span {
                        start: 0,
                        end: 0,
                        line: 0,
                        column: 0,
                    },
                    referenced: false,
                };
                let _ = self.symbols.define(symbol);
            }
        }

        // Now collect properties and methods (class name is now resolvable)
        // Separate instance and static members
        let mut properties = Vec::new();
        let mut methods = Vec::new();
        let mut static_properties = Vec::new();
        let mut static_methods = Vec::new();

        // Track seen field/method names for duplicate detection
        let mut seen_fields: std::collections::HashMap<String, Span> =
            std::collections::HashMap::new();
        let mut seen_methods: std::collections::HashMap<String, Span> =
            std::collections::HashMap::new();

        for member in &class.members {
            match member {
                ClassMember::Field(field) => {
                    let field_name = self.resolve(field.name.name);

                    // Check for duplicate field names
                    if let Some(original_span) = seen_fields.get(&field_name) {
                        return Err(BindError::DuplicateSymbol {
                            name: field_name,
                            original: *original_span,
                            duplicate: field.name.span,
                        });
                    }
                    seen_fields.insert(field_name.clone(), field.name.span);
                    let field_ty = if let Some(ref ann) = field.type_annotation {
                        self.resolve_type_annotation(ann)?
                    } else {
                        self.type_ctx.unknown_type()
                    };
                    let prop = PropertySignature {
                        name: field_name,
                        ty: field_ty,
                        optional: false,
                        readonly: field.is_readonly,
                        visibility: field.visibility,
                    };
                    if field.is_static {
                        static_properties.push(prop);
                    } else {
                        properties.push(prop);
                    }
                }
                ClassMember::Method(method) => {
                    let method_name = self.resolve(method.name.name);

                    // Check for duplicate method names
                    if let Some(original_span) = seen_methods.get(&method_name) {
                        return Err(BindError::DuplicateSymbol {
                            name: method_name,
                            original: *original_span,
                            duplicate: method.name.span,
                        });
                    }
                    seen_methods.insert(method_name.clone(), method.name.span);

                    // Extract method-level type parameters (e.g., withLock<R>)
                    let method_type_params: Vec<String> = method
                        .type_params
                        .as_ref()
                        .map(|tps| tps.iter().map(|tp| self.resolve(tp.name.name)).collect())
                        .unwrap_or_default();

                    // If method has type parameters, push a temporary scope and register them
                    let has_method_type_params = !method_type_params.is_empty();
                    if has_method_type_params {
                        self.symbols.push_scope(ScopeKind::Function);
                        if let Some(ref method_type_params_ast) = method.type_params {
                            for type_param in method_type_params_ast {
                                let type_param_name = self.resolve(type_param.name.name);
                                let constraint_ty =
                                    if let Some(ref constraint) = type_param.constraint {
                                        self.resolve_type_annotation(constraint).ok()
                                    } else {
                                        None
                                    };
                                let type_var = self.type_ctx.type_variable_with_constraint(
                                    type_param_name.clone(),
                                    constraint_ty,
                                );
                                let symbol = Symbol {
                                    name: type_param_name,
                                    kind: SymbolKind::TypeAlias,
                                    ty: type_var,
                                    flags: SymbolFlags::default(),
                                    scope_id: self.symbols.current_scope_id(),
                                    span: type_param.span,
                                    referenced: false,
                                };
                                let _ = self.symbols.define(symbol);
                            }
                        }
                    }

                    // Create function type for the method
                    let mut params = Vec::new();
                    let mut rest_param_ty = None;

                    for p in &method.params {
                        if p.is_rest {
                            // Validate and extract rest parameter type
                            if let Some(ref type_ann) = p.type_annotation {
                                let param_ty = self.resolve_type_annotation(type_ann)?;
                                if self.is_valid_rest_param_type(param_ty) {
                                    rest_param_ty = Some(param_ty);
                                } else {
                                    return Err(BindError::InvalidRestParameter {
                                        message: "Rest parameter type must be an array/tuple (e.g., string[] or [number, string])".to_string(),
                                        span: type_ann.span,
                                    });
                                }
                            } else {
                                return Err(BindError::InvalidRestParameter {
                                    message: "Rest parameter must have a type annotation (e.g., ...args: string[])".to_string(),
                                    span: p.span,
                                });
                            }
                            // Skip rest parameter from regular params
                            continue;
                        }

                        let param_ty = if let Some(ref ann) = p.type_annotation {
                            self.resolve_type_annotation(ann)?
                        } else {
                            self.type_ctx.unknown_type()
                        };
                        params.push(param_ty);
                    }
                    // Placeholder for return type - will be fixed up below
                    let declared_return_ty = if let Some(ref ann) = method.return_type {
                        self.resolve_type_annotation(ann)?
                    } else {
                        if self.mode == TypeSystemMode::Js {
                            self.type_ctx.unknown_type()
                        } else {
                            self.type_ctx.void_type()
                        }
                    };
                    // Async methods are represented as `is_async = true` with inner return type `T`.
                    // If user annotates `Promise<T>`, unwrap it here to avoid `Promise<Promise<T>>`.
                    let return_ty = if method.is_async {
                        match self.type_ctx.get(declared_return_ty) {
                            Some(Type::Task(task_ty)) => task_ty.result,
                            _ => declared_return_ty,
                        }
                    } else {
                        declared_return_ty
                    };

                    // Pop the temporary scope for method type parameters
                    if has_method_type_params {
                        self.symbols.pop_scope();
                    }

                    // Validate parameter ordering
                    self.validate_param_order(&method.params)?;

                    // Validate rest parameter is last and only one
                    if let Some(rest_idx) = method.params.iter().position(|p| p.is_rest) {
                        // Check rest is last
                        if rest_idx < method.params.len() - 1 {
                            return Err(BindError::InvalidRestParameter {
                                message: "Rest parameter must be last".to_string(),
                                span: method.params[rest_idx].span,
                            });
                        }
                    }

                    // Count required params (excluding rest parameter)
                    let min_params = if self.mode == TypeSystemMode::Js {
                        0
                    } else {
                        method
                            .params
                            .iter()
                            .filter(|p| !p.is_rest)
                            .filter(|p| p.default_value.is_none() && !p.optional)
                            .count()
                    };

                    if method.is_static {
                        static_methods.push((
                            method_name,
                            params,
                            return_ty,
                            method.is_async,
                            method_type_params.clone(),
                            method.visibility,
                            min_params,
                            rest_param_ty,
                        ));
                    } else {
                        methods.push((
                            method_name,
                            params,
                            return_ty,
                            method.is_async,
                            method_type_params,
                            method.visibility,
                            min_params,
                            rest_param_ty,
                        ));
                    }
                }
                ClassMember::Constructor(ctor) => {
                    // Register constructor parameter properties (e.g., `constructor(public x: number)`)
                    for param in &ctor.params {
                        if let Some(vis) = param.visibility {
                            if let crate::parser::ast::Pattern::Identifier(ident) = &param.pattern {
                                let field_name = self.resolve(ident.name);
                                let field_ty = if let Some(ref ann) = param.type_annotation {
                                    self.resolve_type_annotation(ann)?
                                } else {
                                    self.type_ctx.unknown_type()
                                };
                                properties.push(PropertySignature {
                                    name: field_name,
                                    ty: field_ty,
                                    optional: false,
                                    readonly: false,
                                    visibility: vis,
                                });
                            }
                        }
                    }

                    // Track constructor signature + visibility so checker can
                    // enforce constructor accessibility on `new`.
                    let mut ctor_params = Vec::new();
                    let mut ctor_rest_param_ty = None;
                    for p in &ctor.params {
                        if p.is_rest {
                            if let Some(ref type_ann) = p.type_annotation {
                                let param_ty = self.resolve_type_annotation(type_ann)?;
                                if self.is_valid_rest_param_type(param_ty) {
                                    ctor_rest_param_ty = Some(param_ty);
                                } else {
                                    return Err(BindError::InvalidRestParameter {
                                        message: "Rest parameter type must be an array/tuple (e.g., string[] or [number, string])".to_string(),
                                        span: type_ann.span,
                                    });
                                }
                            } else {
                                return Err(BindError::InvalidRestParameter {
                                    message: "Rest parameter must have a type annotation (e.g., ...args: string[])".to_string(),
                                    span: p.span,
                                });
                            }
                            continue;
                        }
                        let param_ty = if let Some(ref ann) = p.type_annotation {
                            self.resolve_type_annotation(ann)?
                        } else {
                            self.type_ctx.unknown_type()
                        };
                        ctor_params.push(param_ty);
                    }
                    self.validate_param_order(&ctor.params)?;
                    if let Some(rest_idx) = ctor.params.iter().position(|p| p.is_rest) {
                        if rest_idx < ctor.params.len() - 1 {
                            return Err(BindError::InvalidRestParameter {
                                message: "Rest parameter must be last".to_string(),
                                span: ctor.params[rest_idx].span,
                            });
                        }
                    }
                    let ctor_min_params = ctor
                        .params
                        .iter()
                        .filter(|p| !p.is_rest)
                        .filter(|p| p.default_value.is_none() && !p.optional)
                        .count();
                    methods.push((
                        "constructor".to_string(),
                        ctor_params,
                        self.type_ctx.void_type(),
                        false, // constructors are never async
                        Vec::new(),
                        ctor.visibility,
                        ctor_min_params,
                        ctor_rest_param_ty,
                    ));
                }
                ClassMember::StaticBlock(_) => {
                    // Static initializer blocks don't contribute to the type signature
                }
            }
        }

        // Create method signatures with proper return types
        // If return type equals the placeholder class_ty, we need to create a self-referential type
        // We'll create the full class type first, then fix up method return types that reference it

        // First pass: create instance method signatures
        let method_sigs: Vec<MethodSignature> = methods
            .into_iter()
            .map(
                |(
                    name,
                    params,
                    return_ty,
                    is_async,
                    method_type_params,
                    vis,
                    min_params,
                    rest_param,
                )| {
                    let func_ty = self.type_ctx.function_type_with_rest(
                        params, return_ty, is_async, min_params, rest_param,
                    );
                    MethodSignature {
                        name,
                        ty: func_ty,
                        type_params: method_type_params,
                        visibility: vis,
                    }
                },
            )
            .collect();

        // Create static method signatures
        let static_method_sigs: Vec<MethodSignature> = static_methods
            .into_iter()
            .map(
                |(
                    name,
                    params,
                    return_ty,
                    is_async,
                    method_type_params,
                    vis,
                    min_params,
                    rest_param,
                )| {
                    let func_ty = self.type_ctx.function_type_with_rest(
                        params, return_ty, is_async, min_params, rest_param,
                    );
                    MethodSignature {
                        name,
                        ty: func_ty,
                        type_params: method_type_params,
                        visibility: vis,
                    }
                },
            )
            .collect();

        // Resolve the extends clause if present
        let extends_ty = if let Some(ref extends_ann) = class.extends {
            Some(self.resolve_type_annotation(extends_ann)?)
        } else {
            self.implicit_object_base_type(&class_name)
        };

        // Create the full class type with properties and methods
        let full_class_type = ClassType {
            name: class_name.clone(),
            type_params: type_param_names.clone(),
            properties,
            methods: method_sigs,
            static_properties,
            static_methods: static_method_sigs,
            extends: extends_ty,
            implements: vec![],
            is_abstract: class.is_abstract,
        };
        // Replace the placeholder type in-place so that all existing references
        // (e.g., self-referential fields like `next: Node | null`) automatically
        // see the full class type without needing to update every TypeId.
        self.type_ctx
            .replace_type(class_ty, Type::Class(full_class_type));
        // Keep named-type mapping pinned to the canonical class TypeId.
        self.type_ctx
            .register_named_type(class_name.clone(), class_ty);

        // Bind class members in the already-entered class scope
        // (scope was pushed earlier for type parameters)

        for member in &class.members {
            match member {
                ClassMember::Method(method) => {
                    if let Some(ref body) = method.body {
                        self.symbols.push_scope(ScopeKind::Function);

                        // Register method-level type parameters in the method scope
                        if let Some(ref type_params) = method.type_params {
                            for tp in type_params {
                                let type_param_name = self.resolve(tp.name.name);
                                let constraint_ty = if let Some(ref constraint) = tp.constraint {
                                    self.resolve_type_annotation(constraint).ok()
                                } else {
                                    None
                                };
                                let type_var = self.type_ctx.type_variable_with_constraint(
                                    type_param_name.clone(),
                                    constraint_ty,
                                );
                                let symbol = Symbol {
                                    name: type_param_name,
                                    kind: SymbolKind::TypeAlias,
                                    ty: type_var,
                                    flags: SymbolFlags::default(),
                                    scope_id: self.symbols.current_scope_id(),
                                    span: tp.span,
                                    referenced: false,
                                };
                                let _ = self.symbols.define(symbol);
                            }
                        }

                        // Bind method parameters in method scope
                        for param in &method.params {
                            let param_ty = if let Some(ref ann) = param.type_annotation {
                                self.resolve_type_annotation(ann)?
                            } else {
                                self.type_ctx.unknown_type()
                            };
                            // Method parameters are mutable (same semantics as function params)
                            self.bind_pattern_names(&param.pattern, param_ty, false, false)?;
                        }

                        for stmt in &body.statements {
                            self.bind_stmt(stmt)?;
                        }
                        self.symbols.pop_scope();
                    }
                }
                ClassMember::Constructor(ctor) => {
                    self.symbols.push_scope(ScopeKind::Function);

                    // Bind constructor parameters in constructor scope
                    for param in &ctor.params {
                        let param_ty = if let Some(ref ann) = param.type_annotation {
                            self.resolve_type_annotation(ann)?
                        } else {
                            self.type_ctx.unknown_type()
                        };
                        // Constructor parameters are mutable
                        self.bind_pattern_names(&param.pattern, param_ty, false, false)?;
                    }

                    for stmt in &ctor.body.statements {
                        self.bind_stmt(stmt)?;
                    }
                    self.symbols.pop_scope();
                }
                _ => {}
            }
        }

        self.symbols.pop_scope();
        Ok(())
    }

    /// Bind type alias declaration
    fn bind_type_alias(&mut self, alias: &TypeAliasDecl) -> Result<(), BindError> {
        let alias_name = self.resolve(alias.name.name);

        // If the type alias has type parameters, register them in a nested scope
        let has_type_params = alias.type_params.as_ref().is_some_and(|p| !p.is_empty());
        let mut type_param_names = Vec::new();

        if has_type_params {
            self.symbols.push_scope(ScopeKind::Function);
            for type_param in alias.type_params.as_ref().unwrap() {
                let param_name = self.resolve(type_param.name.name);
                let constraint_ty = if let Some(ref constraint) = type_param.constraint {
                    self.resolve_type_annotation(constraint).ok()
                } else {
                    None
                };
                let type_var = self
                    .type_ctx
                    .type_variable_with_constraint(param_name.clone(), constraint_ty);
                let sym = Symbol {
                    name: param_name.clone(),
                    kind: SymbolKind::TypeAlias,
                    ty: type_var,
                    flags: SymbolFlags::default(),
                    scope_id: self.symbols.current_scope_id(),
                    span: Span {
                        start: 0,
                        end: 0,
                        line: 0,
                        column: 0,
                    },
                    referenced: false,
                };
                let _ = self.symbols.define(sym);
                type_param_names.push(param_name);
            }
        }

        // Resolve the type annotation (TypeVars will be resolved from the nested scope)
        let ty = self.resolve_type_annotation(&alias.type_annotation)?;

        if has_type_params {
            self.symbols.pop_scope();
            // Store type param names for later substitution during type reference resolution
            self.generic_type_alias_params
                .insert(alias_name.clone(), type_param_names);
        }

        let definition_scope = self.symbols.current_scope_id();
        let symbol = Symbol {
            name: alias_name.clone(),
            kind: SymbolKind::TypeAlias,
            ty,
            flags: SymbolFlags::default(),
            scope_id: definition_scope,
            span: alias.name.span,
            referenced: false,
        };

        // If pre-registered by the prepass, use replace_type() to fill the
        // placeholder ObjectType with the resolved type data. This ensures
        // self-referential type aliases work: during resolution above, any
        // reference to this alias resolved to the placeholder TypeId, and
        // replace_type() mutates it in-place so all references see the update.
        if let Some(existing) = self.symbol_in_scope(definition_scope, &alias_name) {
            if existing.kind == SymbolKind::TypeAlias && !existing.flags.is_imported {
                let placeholder_ty = existing.ty;
                // Copy the resolved type's data into the placeholder TypeId
                if let Some(resolved_type) = self.type_ctx.get(ty).cloned() {
                    self.type_ctx.replace_type(placeholder_ty, resolved_type);
                }
                // Keep symbol pointing to placeholder_ty (which now has real data)
                let symbol = Symbol {
                    ty: placeholder_ty,
                    ..symbol
                };
                self.symbols.replace_in_scope(definition_scope, symbol);
                Ok(())
            } else {
                self.symbols
                    .define(symbol)
                    .map_err(|err| BindError::DuplicateSymbol {
                        name: err.name,
                        original: err.original,
                        duplicate: err.duplicate,
                    })
            }
        } else {
            self.symbols
                .define(symbol)
                .map_err(|err| BindError::DuplicateSymbol {
                    name: err.name,
                    original: err.original,
                    duplicate: err.duplicate,
                })
        }
    }

    /// Bind block statement
    fn bind_block(&mut self, block: &BlockStatement) -> Result<(), BindError> {
        self.symbols.push_scope(ScopeKind::Block);
        for stmt in &block.statements {
            self.bind_stmt(stmt)?;
        }
        self.symbols.pop_scope();
        Ok(())
    }

    /// Bind if statement
    fn bind_if(&mut self, if_stmt: &IfStatement) -> Result<(), BindError> {
        // Bind then branch
        self.bind_stmt(&if_stmt.then_branch)?;

        // Bind else branch if present
        if let Some(ref else_branch) = if_stmt.else_branch {
            self.bind_stmt(else_branch)?;
        }

        Ok(())
    }

    /// Bind switch statement
    fn bind_switch(&mut self, switch_stmt: &SwitchStatement) -> Result<(), BindError> {
        for case in &switch_stmt.cases {
            for stmt in &case.consequent {
                self.bind_stmt(stmt)?;
            }
        }
        Ok(())
    }

    /// Bind while loop
    fn bind_while(&mut self, while_stmt: &WhileStatement) -> Result<(), BindError> {
        self.symbols.push_scope(ScopeKind::Loop);
        self.bind_stmt(&while_stmt.body)?;
        self.symbols.pop_scope();
        Ok(())
    }

    /// Bind for loop
    fn bind_for(&mut self, for_stmt: &ForStatement) -> Result<(), BindError> {
        self.symbols.push_scope(ScopeKind::Loop);

        // Bind initializer if present
        if let Some(ref init) = for_stmt.init {
            match init {
                ForInit::VariableDecl(decl) => self.bind_var_decl(decl)?,
                ForInit::Expression(_) => {}
            }
        }

        // Bind body
        self.bind_stmt(&for_stmt.body)?;

        self.symbols.pop_scope();
        Ok(())
    }

    /// Bind for-of loop
    fn bind_for_of(&mut self, for_of: &ForOfStatement) -> Result<(), BindError> {
        self.symbols.push_scope(ScopeKind::Loop);

        // Bind the loop variable
        match &for_of.left {
            ForOfLeft::VariableDecl(decl) => self.bind_var_decl(decl)?,
            ForOfLeft::Pattern(_) => {
                // Existing variable - already bound in outer scope
            }
        }

        // Bind body
        self.bind_stmt(&for_of.body)?;

        self.symbols.pop_scope();
        Ok(())
    }

    /// Bind for-in loop
    fn bind_for_in(&mut self, for_in: &ForInStatement) -> Result<(), BindError> {
        self.symbols.push_scope(ScopeKind::Loop);

        // Bind the loop variable
        match &for_in.left {
            ForOfLeft::VariableDecl(decl) => self.bind_var_decl(decl)?,
            ForOfLeft::Pattern(_) => {
                // Existing variable - already bound in outer scope
            }
        }

        // Bind body
        self.bind_stmt(&for_in.body)?;

        self.symbols.pop_scope();
        Ok(())
    }

    /// Bind try-catch statement
    fn bind_try(&mut self, try_stmt: &TryStatement) -> Result<(), BindError> {
        // Bind try block
        for stmt in &try_stmt.body.statements {
            self.bind_stmt(stmt)?;
        }

        // Bind catch clause if present
        if let Some(ref catch) = try_stmt.catch_clause {
            self.symbols.push_scope(ScopeKind::Block);

            // Bind catch parameter
            if let Some(ref param) = catch.param {
                let error_ty = if self.policy.use_unknown_in_catch_variables {
                    self.type_ctx.unknown_type()
                } else {
                    self.type_ctx.jsobject_type()
                };

                // Bind all names in the pattern (handles destructuring)
                self.bind_pattern_names(param, error_ty, true, false)?;
            }

            // Bind catch body
            for stmt in &catch.body.statements {
                self.bind_stmt(stmt)?;
            }

            self.symbols.pop_scope();
        }

        // Bind finally clause if present
        if let Some(ref finally_clause) = try_stmt.finally_clause {
            for stmt in &finally_clause.statements {
                self.bind_stmt(stmt)?;
            }
        }

        Ok(())
    }

    /// Validate that required parameters come before optional/default parameters
    fn validate_param_order(
        &self,
        params: &[crate::parser::ast::Parameter],
    ) -> Result<(), BindError> {
        let mut seen_optional = false;
        for param in params {
            let is_optional = param.optional;
            if is_optional {
                seen_optional = true;
            } else if seen_optional {
                let name = if let crate::parser::ast::Pattern::Identifier(ident) = &param.pattern {
                    self.resolve(ident.name)
                } else {
                    "<unknown>".to_string()
                };
                return Err(BindError::RequiredAfterOptional {
                    name,
                    span: param.span,
                });
            }
        }
        Ok(())
    }

    /// Resolve type annotation to TypeId
    fn resolve_type_annotation(&mut self, ty_annot: &TypeAnnotation) -> Result<TypeId, BindError> {
        self.resolve_type(&ty_annot.ty, ty_annot.span)
    }

    fn is_valid_rest_param_type(&self, ty: TypeId) -> bool {
        match self.type_ctx.get(ty) {
            Some(Type::Array(_))
            | Some(Type::Tuple(_))
            | Some(Type::TypeVar(_))
            | Some(Type::IndexedAccess(_))
            | Some(Type::Keyof(_))
            | Some(Type::Unknown) => true,
            Some(Type::Union(u)) => u.members.iter().all(|m| self.is_valid_rest_param_type(*m)),
            _ => false,
        }
    }

    fn instantiate_generic_type_alias(
        &mut self,
        template_ty: TypeId,
        param_names: &[String],
        resolved_args: &[TypeId],
    ) -> TypeId {
        let mut gen_ctx = crate::parser::types::GenericContext::new(self.type_ctx);
        for (param_name, &arg_ty) in param_names.iter().zip(resolved_args.iter()) {
            gen_ctx.add_substitution(param_name.clone(), arg_ty);
        }
        gen_ctx
            .apply_substitution(template_ty)
            .unwrap_or(template_ty)
    }

    /// Resolve type to TypeId
    fn resolve_type(
        &mut self,
        ty: &crate::parser::ast::Type,
        span: crate::parser::Span,
    ) -> Result<TypeId, BindError> {
        use crate::parser::ast::Type as AstType;

        match ty {
            AstType::Primitive(prim) => Ok(self.resolve_primitive(*prim)),

            AstType::Reference(type_ref) => {
                // Check if it's a user-defined type or type parameter
                let name = self.resolve(type_ref.name.name);
                if name == "any" {
                    return if self.allows_explicit_any() {
                        Ok(self.type_ctx.any_type())
                    } else {
                        Err(BindError::InvalidTypeExpr {
                            message:
                                "E_STRICT_ANY_FORBIDDEN: `any` is not allowed in Raya strict mode"
                                    .to_string(),
                            span,
                        })
                    };
                }

                // Handle built-in generic types
                use crate::parser::TypeContext as TC;
                if name == TC::ARRAY_TYPE_NAME {
                    if let Some(ref type_args) = type_ref.type_args {
                        if type_args.len() == 1 {
                            let elem_ty = self.resolve_type_annotation(&type_args[0])?;
                            return Ok(self.type_ctx.array_type(elem_ty));
                        }
                    }
                    return Err(BindError::InvalidTypeArguments {
                        name,
                        expected: 1,
                        actual: type_ref.type_args.as_ref().map(|a| a.len()).unwrap_or(0),
                        span,
                    });
                }

                // Handle Promise<T> and legacy Task<T> alias for async functions.
                if name == TC::PROMISE_TYPE_NAME || name == "Task" {
                    if let Some(ref type_args) = type_ref.type_args {
                        if type_args.is_empty() {
                            let unknown = self.type_ctx.unknown_type();
                            return Ok(self.type_ctx.task_type(unknown));
                        }
                        if type_args.len() == 1 {
                            let result_ty = self.resolve_type_annotation(&type_args[0])?;
                            return Ok(self.type_ctx.task_type(result_ty));
                        }
                    } else {
                        let unknown = self.type_ctx.unknown_type();
                        return Ok(self.type_ctx.task_type(unknown));
                    }
                    return Err(BindError::InvalidTypeArguments {
                        name,
                        expected: 1,
                        actual: type_ref.type_args.as_ref().map(|a| a.len()).unwrap_or(0),
                        span,
                    });
                }

                // Handle Channel<T> for channel communication
                if name == TC::CHANNEL_TYPE_NAME {
                    if let Some(ref type_args) = type_ref.type_args {
                        if type_args.len() == 1 {
                            let message_ty = self.resolve_type_annotation(&type_args[0])?;
                            return Ok(self.type_ctx.channel_type_with(message_ty));
                        }
                        return Err(BindError::InvalidTypeArguments {
                            name,
                            expected: 1,
                            actual: type_args.len(),
                            span,
                        });
                    }
                    return Ok(self.type_ctx.channel_type());
                }

                if name == TC::MAP_TYPE_NAME {
                    if let Some(ref type_args) = type_ref.type_args {
                        if type_args.len() == 2 {
                            let key_ty = self.resolve_type_annotation(&type_args[0])?;
                            let value_ty = self.resolve_type_annotation(&type_args[1])?;
                            return Ok(self.type_ctx.map_type_with(key_ty, value_ty));
                        }
                        return Err(BindError::InvalidTypeArguments {
                            name,
                            expected: 2,
                            actual: type_args.len(),
                            span,
                        });
                    }
                    return Ok(self.type_ctx.map_type());
                }

                if name == TC::SET_TYPE_NAME {
                    if let Some(ref type_args) = type_ref.type_args {
                        if type_args.len() == 1 {
                            let elem_ty = self.resolve_type_annotation(&type_args[0])?;
                            return Ok(self.type_ctx.set_type_with(elem_ty));
                        }
                        return Err(BindError::InvalidTypeArguments {
                            name,
                            expected: 1,
                            actual: type_args.len(),
                            span,
                        });
                    }
                    return Ok(self.type_ctx.set_type());
                }

                // Handle Record<K, V> utility type as an indexed object shape.
                if name == "Record" {
                    if let Some(ref type_args) = type_ref.type_args {
                        if type_args.len() == 2 {
                            let value_ty = self.resolve_type_annotation(&type_args[1])?;
                            let object_type = crate::parser::types::ty::ObjectType {
                                properties: vec![],
                                index_signature: Some(("[key]".to_string(), value_ty)),
                                call_signatures: vec![],
                                construct_signatures: vec![],
                            };
                            return Ok(self.type_ctx.intern(Type::Object(object_type)));
                        }
                    }
                    return Err(BindError::InvalidTypeArguments {
                        name,
                        expected: 2,
                        actual: type_ref.type_args.as_ref().map(|a| a.len()).unwrap_or(0),
                        span,
                    });
                }

                if let Some(symbol) = self
                    .symbols
                    .resolve_from_scope(&name, self.symbols.current_scope_id())
                {
                    match symbol.kind {
                        SymbolKind::Class => {
                            // Keep class references symbolic during binding so forward refs and
                            // cross-class generics don't capture pre-pass placeholder layouts.
                            let type_args = if let Some(type_args) = &type_ref.type_args {
                                let mut resolved = Vec::with_capacity(type_args.len());
                                for arg in type_args {
                                    resolved.push(self.resolve_type_annotation(arg)?);
                                }
                                if resolved.is_empty() {
                                    None
                                } else {
                                    Some(resolved)
                                }
                            } else {
                                None
                            };
                            Ok(self.type_ctx.intern(Type::Reference(
                                crate::parser::types::ty::TypeReference { name, type_args },
                            )))
                        }
                        SymbolKind::TypeAlias | SymbolKind::TypeParameter => {
                            let template_ty = symbol.ty;

                            // Check if this is a generic type alias with type arguments.
                            if let Some(ref type_args) = type_ref.type_args {
                                if let Some(param_names) =
                                    self.generic_type_alias_params.get(&name).cloned()
                                {
                                    if type_args.len() == param_names.len() {
                                        // Resolve each type argument.
                                        let mut resolved_args = Vec::with_capacity(type_args.len());
                                        for arg in type_args {
                                            resolved_args.push(self.resolve_type_annotation(arg)?);
                                        }
                                        return Ok(self.instantiate_generic_type_alias(
                                            template_ty,
                                            &param_names,
                                            &resolved_args,
                                        ));
                                    }
                                }
                            }

                            Ok(template_ty)
                        }
                        _ => Err(BindError::NotAType { name, span }),
                    }
                } else {
                    // Fall back to globally registered named types (primitive/system aliases)
                    // only when no in-scope symbol matches. This preserves shadowing semantics.
                    if let Some(named_ty) = self.type_ctx.lookup_named_type(&name) {
                        if let Some(type_args) = &type_ref.type_args {
                            let mut resolved_args = Vec::with_capacity(type_args.len());
                            for arg in type_args {
                                resolved_args.push(self.resolve_type_annotation(arg)?);
                            }
                            if !resolved_args.is_empty() {
                                return Ok(self.type_ctx.intern(Type::Reference(
                                    crate::parser::types::ty::TypeReference {
                                        name,
                                        type_args: Some(resolved_args),
                                    },
                                )));
                            }
                        }
                        Ok(named_ty)
                    } else {
                        Err(BindError::UndefinedType { name, span })
                    }
                }
            }

            AstType::Array(arr) => {
                let elem_ty = self.resolve_type_annotation(&arr.element_type)?;
                Ok(self.type_ctx.array_type(elem_ty))
            }

            AstType::Tuple(tuple) => {
                let elem_tys: Result<Vec<_>, _> = tuple
                    .element_types
                    .iter()
                    .map(|e| self.resolve_type_annotation(e))
                    .collect();
                Ok(self.type_ctx.tuple_type(elem_tys?))
            }

            AstType::Union(union) => {
                let member_tys: Result<Vec<_>, _> = union
                    .types
                    .iter()
                    .map(|t| self.resolve_type_annotation(t))
                    .collect();
                Ok(self.type_ctx.union_type(member_tys?))
            }

            AstType::Intersection(intersection) => {
                // Resolve all constituent types and merge their structural members into
                // a single object type. This supports TS-compatible interface/class
                // extension forms lowered to intersections.
                let mut merged_properties = Vec::new();
                let mut index_signature: Option<(String, TypeId)> = None;
                let mut call_signatures: Vec<TypeId> = Vec::new();
                let mut construct_signatures: Vec<TypeId> = Vec::new();
                let mut pending = Vec::new();
                let mut visited = std::collections::HashSet::new();

                for ty_annot in &intersection.types {
                    pending.push(self.resolve_type_annotation(ty_annot)?);
                }

                while let Some(ty_id) = pending.pop() {
                    if !visited.insert(ty_id) {
                        continue;
                    }

                    let Some(ty) = self.type_ctx.get(ty_id).cloned() else {
                        continue;
                    };

                    match ty {
                        crate::parser::types::Type::Object(obj) => {
                            for prop in obj.properties {
                                if !merged_properties.iter().any(
                                    |p: &crate::parser::types::ty::PropertySignature| {
                                        p.name == prop.name
                                    },
                                ) {
                                    merged_properties.push(prop);
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
                        crate::parser::types::Type::Class(class_ty) => {
                            for prop in class_ty.properties.into_iter().filter(|prop| {
                                prop.visibility == crate::parser::ast::Visibility::Public
                            }) {
                                if !merged_properties.iter().any(
                                    |existing: &crate::parser::types::ty::PropertySignature| {
                                        existing.name == prop.name
                                    },
                                ) {
                                    merged_properties.push(prop);
                                }
                            }
                            for method in class_ty.methods.into_iter().filter(|method| {
                                method.visibility == crate::parser::ast::Visibility::Public
                            }) {
                                if !merged_properties.iter().any(
                                    |existing: &crate::parser::types::ty::PropertySignature| {
                                        existing.name == method.name
                                    },
                                ) {
                                    merged_properties.push(
                                        crate::parser::types::ty::PropertySignature {
                                            name: method.name,
                                            ty: method.ty,
                                            optional: false,
                                            readonly: false,
                                            visibility: crate::parser::ast::Visibility::Public,
                                        },
                                    );
                                }
                            }
                        }
                        crate::parser::types::Type::Interface(interface_ty) => {
                            for prop in interface_ty.properties {
                                if !merged_properties.iter().any(
                                    |existing: &crate::parser::types::ty::PropertySignature| {
                                        existing.name == prop.name
                                    },
                                ) {
                                    merged_properties.push(prop);
                                }
                            }
                            for method in interface_ty.methods {
                                if !merged_properties.iter().any(
                                    |existing: &crate::parser::types::ty::PropertySignature| {
                                        existing.name == method.name
                                    },
                                ) {
                                    merged_properties.push(
                                        crate::parser::types::ty::PropertySignature {
                                            name: method.name,
                                            ty: method.ty,
                                            optional: false,
                                            readonly: false,
                                            visibility: crate::parser::ast::Visibility::Public,
                                        },
                                    );
                                }
                            }
                            for sig in interface_ty.call_signatures {
                                if !call_signatures.contains(&sig) {
                                    call_signatures.push(sig);
                                }
                            }
                            for sig in interface_ty.construct_signatures {
                                if !construct_signatures.contains(&sig) {
                                    construct_signatures.push(sig);
                                }
                            }
                            pending.extend(interface_ty.extends);
                        }
                        crate::parser::types::Type::Reference(type_ref) => {
                            if let Some(named_ty) = self.type_ctx.lookup_named_type(&type_ref.name)
                            {
                                pending.push(named_ty);
                            }
                        }
                        crate::parser::types::Type::Generic(generic_ty) => {
                            pending.push(generic_ty.base);
                        }
                        _ => {}
                    }
                }

                Ok(self.type_ctx.intern(crate::parser::types::Type::Object(
                    crate::parser::types::ty::ObjectType {
                        properties: merged_properties,
                        index_signature,
                        call_signatures,
                        construct_signatures,
                    },
                )))
            }

            AstType::Function(func) => {
                let mut param_tys = Vec::new();
                let mut rest_param = None;
                let mut min_params = 0usize;

                for p in &func.params {
                    let p_ty = self.resolve_type_annotation(&p.ty)?;
                    if p.is_rest {
                        rest_param = Some(p_ty);
                    } else {
                        if !p.optional {
                            min_params += 1;
                        }
                        param_tys.push(p_ty);
                    }
                }

                let return_ty = self.resolve_type_annotation(&func.return_type)?;
                Ok(self
                    .type_ctx
                    .function_type_with_rest(param_tys, return_ty, false, min_params, rest_param))
            }

            AstType::Object(obj) => {
                use crate::parser::ast::ObjectTypeMember;
                use crate::parser::types::ty::{ObjectType, PropertySignature};

                let mut properties = Vec::new();
                let mut index_signature: Option<(String, TypeId)> = None;
                let mut call_signatures = Vec::new();
                let mut construct_signatures = Vec::new();

                for member in &obj.members {
                    match member {
                        ObjectTypeMember::Property(prop) => {
                            let prop_type = self.resolve_type_annotation(&prop.ty)?;
                            properties.push(PropertySignature {
                                name: self.resolve(prop.name.name),
                                ty: prop_type,
                                optional: prop.optional,
                                readonly: prop.readonly,
                                visibility: Default::default(),
                            });
                        }
                        ObjectTypeMember::Method(method) => {
                            // Resolve method as a function type
                            let mut param_tys = Vec::new();
                            let mut rest_param = None;
                            let mut min_params = 0usize;
                            for p in &method.params {
                                let p_ty = self.resolve_type_annotation(&p.ty)?;
                                if p.is_rest {
                                    rest_param = Some(p_ty);
                                } else {
                                    if !p.optional {
                                        min_params += 1;
                                    }
                                    param_tys.push(p_ty);
                                }
                            }
                            let return_ty = self.resolve_type_annotation(&method.return_type)?;
                            let func_ty = self.type_ctx.function_type_with_rest(
                                param_tys, return_ty, false, min_params, rest_param,
                            );

                            properties.push(PropertySignature {
                                name: self.resolve(method.name.name),
                                ty: func_ty,
                                optional: method.optional,
                                readonly: false,
                                visibility: Default::default(),
                            });
                        }
                        ObjectTypeMember::IndexSignature(index) => {
                            let key_name = self.resolve(index.key_name.name);
                            let value_ty = self.resolve_type_annotation(&index.value_type)?;
                            index_signature = Some((key_name, value_ty));
                        }
                        ObjectTypeMember::CallSignature(call_sig) => {
                            let mut param_tys = Vec::new();
                            let mut rest_param = None;
                            let mut min_params = 0usize;
                            for p in &call_sig.params {
                                let p_ty = self.resolve_type_annotation(&p.ty)?;
                                if p.is_rest {
                                    rest_param = Some(p_ty);
                                } else {
                                    if !p.optional {
                                        min_params += 1;
                                    }
                                    param_tys.push(p_ty);
                                }
                            }
                            let return_ty = self.resolve_type_annotation(&call_sig.return_type)?;
                            let call_ty = self.type_ctx.function_type_with_rest(
                                param_tys, return_ty, false, min_params, rest_param,
                            );
                            call_signatures.push(call_ty);
                        }
                        ObjectTypeMember::ConstructSignature(ctor_sig) => {
                            let mut param_tys = Vec::new();
                            let mut rest_param = None;
                            let mut min_params = 0usize;
                            for p in &ctor_sig.params {
                                let p_ty = self.resolve_type_annotation(&p.ty)?;
                                if p.is_rest {
                                    rest_param = Some(p_ty);
                                } else {
                                    if !p.optional {
                                        min_params += 1;
                                    }
                                    param_tys.push(p_ty);
                                }
                            }
                            let return_ty = self.resolve_type_annotation(&ctor_sig.return_type)?;
                            let ctor_ty = self.type_ctx.function_type_with_rest(
                                param_tys, return_ty, false, min_params, rest_param,
                            );
                            construct_signatures.push(ctor_ty);
                        }
                    }
                }

                let object_type = ObjectType {
                    properties,
                    index_signature,
                    call_signatures,
                    construct_signatures,
                };

                Ok(self
                    .type_ctx
                    .intern(crate::parser::types::ty::Type::Object(object_type)))
            }

            AstType::Keyof(keyof_ty) => {
                let target_ty = self.resolve_type_annotation(&keyof_ty.target)?;
                match self.type_ctx.get(target_ty).cloned() {
                    Some(Type::Object(obj)) => {
                        let members: Vec<TypeId> = obj
                            .properties
                            .iter()
                            .map(|p| self.type_ctx.string_literal(p.name.clone()))
                            .collect();
                        if members.is_empty() {
                            Ok(self.type_ctx.string_type())
                        } else {
                            Ok(self.type_ctx.union_type(members))
                        }
                    }
                    Some(Type::Class(class)) => {
                        let members: Vec<TypeId> = class
                            .properties
                            .iter()
                            .map(|p| self.type_ctx.string_literal(p.name.clone()))
                            .collect();
                        if members.is_empty() {
                            Ok(self.type_ctx.string_type())
                        } else {
                            Ok(self.type_ctx.union_type(members))
                        }
                    }
                    Some(Type::TypeVar(tv)) => {
                        if let Some(constraint) = tv.constraint {
                            match self.type_ctx.get(constraint).cloned() {
                                Some(Type::Object(obj)) => {
                                    let members: Vec<TypeId> = obj
                                        .properties
                                        .iter()
                                        .map(|p| self.type_ctx.string_literal(p.name.clone()))
                                        .collect();
                                    if members.is_empty() {
                                        Ok(self.type_ctx.string_type())
                                    } else {
                                        Ok(self.type_ctx.union_type(members))
                                    }
                                }
                                Some(Type::Class(class)) => {
                                    let members: Vec<TypeId> = class
                                        .properties
                                        .iter()
                                        .map(|p| self.type_ctx.string_literal(p.name.clone()))
                                        .collect();
                                    if members.is_empty() {
                                        Ok(self.type_ctx.string_type())
                                    } else {
                                        Ok(self.type_ctx.union_type(members))
                                    }
                                }
                                _ => Ok(self.type_ctx.string_type()),
                            }
                        } else {
                            Ok(self.type_ctx.string_type())
                        }
                    }
                    _ => Ok(self.type_ctx.keyof_type(target_ty)),
                }
            }

            AstType::IndexedAccess(indexed) => {
                let object_ty = self.resolve_type_annotation(&indexed.object)?;
                let index_ty = self.resolve_type_annotation(&indexed.index)?;

                let prop_for_key =
                    |obj: &crate::parser::types::ty::ObjectType, key: &str| -> Option<TypeId> {
                        obj.properties.iter().find(|p| p.name == key).map(|p| p.ty)
                    };

                let object_data = self.type_ctx.get(object_ty).cloned();
                let index_data = self.type_ctx.get(index_ty).cloned();

                if matches!(object_data, Some(Type::TypeVar(_)))
                    || matches!(index_data, Some(Type::TypeVar(_)))
                {
                    return Ok(self.type_ctx.indexed_access_type(object_ty, index_ty));
                }

                let object_data = match object_data {
                    Some(Type::TypeVar(tv)) => tv
                        .constraint
                        .and_then(|c| self.type_ctx.get(c).cloned())
                        .or(Some(Type::TypeVar(tv))),
                    other => other,
                };

                let index_data = match index_data {
                    Some(Type::TypeVar(tv)) => tv
                        .constraint
                        .and_then(|c| self.type_ctx.get(c).cloned())
                        .or(Some(Type::TypeVar(tv))),
                    other => other,
                };

                match (object_data, index_data) {
                    (Some(Type::Object(obj)), Some(Type::StringLiteral(s))) => {
                        Ok(prop_for_key(&obj, &s)
                            .or(obj.index_signature.map(|(_, ty)| ty))
                            .unwrap_or_else(|| self.type_ctx.unknown_type()))
                    }
                    (Some(Type::Object(obj)), Some(Type::Union(u))) => {
                        let mut out = Vec::new();
                        for member in &u.members {
                            if let Some(Type::StringLiteral(s)) =
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
                            Ok(self.type_ctx.unknown_type())
                        } else {
                            Ok(self.type_ctx.union_type(out))
                        }
                    }
                    (Some(Type::Tuple(t)), Some(Type::NumberLiteral(n))) => {
                        let idx = n as usize;
                        if idx < t.elements.len() {
                            Ok(t.elements[idx])
                        } else {
                            Ok(self.type_ctx.unknown_type())
                        }
                    }
                    (
                        Some(Type::Object(obj)),
                        Some(Type::Primitive(crate::parser::types::PrimitiveType::String)),
                    ) => {
                        let mut out = Vec::new();
                        for p in &obj.properties {
                            out.push(p.ty);
                        }
                        if let Some((_, sig_ty)) = obj.index_signature {
                            out.push(sig_ty);
                        }
                        if out.is_empty() {
                            Ok(self.type_ctx.unknown_type())
                        } else {
                            Ok(self.type_ctx.union_type(out))
                        }
                    }
                    (
                        Some(Type::Object(obj)),
                        Some(Type::Primitive(crate::parser::types::PrimitiveType::Number)),
                    )
                    | (
                        Some(Type::Object(obj)),
                        Some(Type::Primitive(crate::parser::types::PrimitiveType::Int)),
                    )
                    | (Some(Type::Object(obj)), Some(Type::NumberLiteral(_))) => {
                        if let Some((_, sig_ty)) = obj.index_signature {
                            Ok(sig_ty)
                        } else {
                            Ok(self.type_ctx.unknown_type())
                        }
                    }
                    _ => Ok(self.type_ctx.indexed_access_type(object_ty, index_ty)),
                }
            }

            AstType::Typeof(_) => {
                // typeof types are resolved during type checking
                Ok(self.type_ctx.unknown_type())
            }

            AstType::StringLiteral(s) => Ok(self
                .type_ctx
                .string_literal(self.interner.resolve(*s).to_string())),

            AstType::NumberLiteral(n) => Ok(self.type_ctx.number_literal(*n)),

            AstType::BooleanLiteral(b) => Ok(self.type_ctx.boolean_literal(*b)),

            AstType::Parenthesized(inner) => self.resolve_type_annotation(inner),
        }
    }

    /// Resolve primitive type to TypeId
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Parser;

    fn parse_and_bind(source: &str) -> (SymbolTable, TypeContext) {
        let parser = Parser::new(source).unwrap();
        let (module, interner) = parser.parse().unwrap();

        let mut ctx = TypeContext::new();
        let binder = Binder::new(&mut ctx, &interner);
        let symbols = binder.bind_module(&module).unwrap();
        (symbols, ctx)
    }

    #[test]
    fn test_bind_simple_variable() {
        let (symbols, _ctx) = parse_and_bind("let x: number = 42;");

        // Should be able to resolve x
        let symbol = symbols.resolve("x").unwrap();
        assert_eq!(symbol.name, "x");
        assert_eq!(symbol.kind, SymbolKind::Variable);
    }

    #[test]
    fn test_bind_function() {
        let (symbols, _ctx) =
            parse_and_bind("function add(a: number, b: number): number { return a + b; }");

        // Should be able to resolve add
        let symbol = symbols.resolve("add").unwrap();
        assert_eq!(symbol.name, "add");
        assert_eq!(symbol.kind, SymbolKind::Function);
    }

    #[test]
    fn test_class_type_name_shadows_global_named_type() {
        let source = r#"
            class Json {
                parse(input: string): number { return 1; }
            }
            class Encoding {
                json: Json;
                constructor() {
                    this.json = new Json();
                }
            }
            const encoding = new Encoding();
        "#;
        let (symbols, ctx) = parse_and_bind(source);
        let encoding = symbols.resolve("Encoding").expect("Encoding symbol");
        let encoding_ty = ctx.get(encoding.ty).expect("Encoding type");
        let field_ty = match encoding_ty {
            Type::Class(class_ty) => class_ty
                .properties
                .iter()
                .find(|p| p.name == "json")
                .map(|p| p.ty)
                .expect("json field"),
            other => panic!("expected class type, got {other:?}"),
        };
        match ctx.get(field_ty) {
            Some(Type::Class(class_ty)) => assert_eq!(class_ty.name, "Json"),
            Some(Type::Reference(reference)) => assert_eq!(reference.name, "Json"),
            Some(other) => panic!("expected json field to be class Json, got {other:?}"),
            None => panic!("missing field type"),
        }
    }

    #[test]
    fn test_function_local_class_forward_type_reference_binds() {
        let source = r#"
            function wrap(): void {
                class A {
                    next(): B | null { return null; }
                }
                class B {}
                let _a = new A();
                let _b = new B();
            }
        "#;
        let _ = parse_and_bind(source);
    }

    #[test]
    fn test_promise_without_type_args_defaults_to_unknown_task() {
        let source = r#"
            async function f(): Promise {
                return 1;
            }
        "#;
        let _ = parse_and_bind(source);
    }

    #[test]
    fn test_interface_call_and_construct_signatures_bind_into_object_shape() {
        let source = r#"
            interface Adder { (a: number, b: number): number }
            interface BoxCtor { new (value: number): { value: number } }
            function unary(a: number): number { return a; }
            let f: Adder = unary;
        "#;
        let (symbols, ctx) = parse_and_bind(source);

        let adder = symbols.resolve("Adder").expect("Adder symbol");
        match ctx.get(adder.ty) {
            Some(Type::Object(obj)) => {
                assert_eq!(obj.call_signatures.len(), 1, "Adder call signatures");
                assert_eq!(
                    obj.construct_signatures.len(),
                    0,
                    "Adder construct signatures"
                );
            }
            other => panic!("expected Adder to bind as object type, got {other:?}"),
        }

        let box_ctor = symbols.resolve("BoxCtor").expect("BoxCtor symbol");
        match ctx.get(box_ctor.ty) {
            Some(Type::Object(obj)) => {
                assert_eq!(obj.call_signatures.len(), 0, "BoxCtor call signatures");
                assert_eq!(
                    obj.construct_signatures.len(),
                    1,
                    "BoxCtor construct signatures"
                );
            }
            other => panic!("expected BoxCtor to bind as object type, got {other:?}"),
        }
    }
}
