//! Name binding - builds symbol tables from AST
//!
//! The binder walks the AST and creates symbol tables for name resolution.
//! It resolves type annotations to TypeId values and tracks all declarations.

use super::error::BindError;
use super::symbols::{Symbol, SymbolFlags, SymbolKind, SymbolTable, ScopeKind};
use super::builtins::BuiltinSignatures;
use crate::parser::ast::*;
use crate::parser::Interner;
use crate::parser::types::{TypeContext, TypeId};
use crate::parser::types::ty::{ClassType, PropertySignature, MethodSignature, Type};
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
    /// When true, builtin source files are prepended to user code, so duplicate
    /// class/function names between builtins and user code are expected (user shadows).
    detect_top_level_duplicates: bool,
    /// Tracks type parameter names for generic type aliases (e.g., Container<T> → ["T"])
    generic_type_alias_params: std::collections::HashMap<String, Vec<String>>,
}

impl<'a> Binder<'a> {
    /// Create a new binder
    pub fn new(type_ctx: &'a mut TypeContext, interner: &'a Interner) -> Self {
        Binder {
            symbols: SymbolTable::new(),
            type_ctx,
            interner,
            bound_classes: std::collections::HashMap::new(),
            bound_functions: std::collections::HashMap::new(),
            detect_top_level_duplicates: true,
            generic_type_alias_params: std::collections::HashMap::new(),
        }
    }

    /// Disable top-level duplicate class/function detection.
    /// Call this when builtin source files are prepended to user code,
    /// since user code may legitimately shadow builtin class names.
    pub fn skip_top_level_duplicate_detection(&mut self) {
        self.detect_top_level_duplicates = false;
    }

    /// Register an external class type so it can be referenced by name during binding.
    /// Used to pre-register builtin primitive types (e.g., RegExp, Array) before
    /// compiling a `.raya` file that cross-references them.
    pub fn register_external_class(&mut self, name: &str) {
        // Reuse existing TypeId if already registered (e.g., pre-interned primitives like
        // string=1, RegExp=8, Array=17). This preserves canonical TypeIds for dispatch.
        let type_id = if let Some(existing) = self.type_ctx.lookup_named_type(name) {
            existing
        } else {
            let stub_type = Type::Class(ClassType {
                name: name.to_string(),
                type_params: Vec::new(),
                properties: Vec::new(),
                methods: Vec::new(),
                static_properties: Vec::new(),
                static_methods: Vec::new(),
                extends: None,
                implements: Vec::new(),
                is_abstract: false,
            });
            let id = self.type_ctx.intern(stub_type);
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
            span: Span { start: 0, end: 0, line: 0, column: 0 },
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
    }

    /// Define an imported symbol
    ///
    /// Used to inject symbols from imported modules before binding.
    pub fn define_imported(&mut self, symbol: Symbol) -> Result<(), super::symbols::DuplicateSymbolError> {
        self.symbols.define_imported(symbol)
    }

    /// Register compiler intrinsics like __NATIVE_CALL and __OPCODE_CHANNEL_NEW
    ///
    /// These are special functions used in builtin .raya files to call VM opcodes.
    fn register_intrinsics(&mut self) {
        // __NATIVE_CALL(native_id: number, ...args): any
        // This is a variadic function that can return any type
        let any_ty = self.type_ctx.unknown_type();
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
            span: Span { start: 0, end: 0, line: 0, column: 0 },
            referenced: false,
        };
        let _ = self.symbols.define(symbol);

        // __OPCODE_CHANNEL_NEW(capacity: number): number
        let channel_new_ty = self.type_ctx.function_type(vec![number_ty], number_ty, false);
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
            span: Span { start: 0, end: 0, line: 0, column: 0 },
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
            span: Span { start: 0, end: 0, line: 0, column: 0 },
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
            span: Span { start: 0, end: 0, line: 0, column: 0 },
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
            span: Span { start: 0, end: 0, line: 0, column: 0 },
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
            span: Span { start: 0, end: 0, line: 0, column: 0 },
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
            span: Span { start: 0, end: 0, line: 0, column: 0 },
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
            span: Span { start: 0, end: 0, line: 0, column: 0 },
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
            span: Span { start: 0, end: 0, line: 0, column: 0 },
            referenced: false,
        };
        let _ = self.symbols.define(symbol);

        // __OPCODE_ARRAY_PUSH(arr: any, elem: any): void
        let array_push_ty = self.type_ctx.function_type(vec![any_ty, any_ty], void_ty, false);
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
            span: Span { start: 0, end: 0, line: 0, column: 0 },
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
            span: Span { start: 0, end: 0, line: 0, column: 0 },
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
    /// - JSON.encode<T>(value: T): string - Compile-time codegen (simplified return type for now)
    /// - JSON.decode<T>(json: string): T - Compile-time codegen (returns typed value)
    ///
    /// JSON.parse and JSON.decode (without type args) return the `json` type.
    /// The `json` type supports duck typing - property access returns json values.
    fn register_json_global(&mut self) {
        let string_ty = self.type_ctx.string_type();
        let any_ty = self.type_ctx.unknown_type();
        let json_ty = self.type_ctx.json_type();

        // Build static methods for JSON object
        // JSON.stringify takes any value and returns string
        // JSON.parse returns json type (supports duck typing)
        // JSON.encode<T> returns string
        // JSON.decode<T> returns T (or json if no type arg)
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
            MethodSignature {
                name: "encode".to_string(),
                ty: self.type_ctx.function_type(vec![any_ty], string_ty, false),
                type_params: vec!["T".to_string()],
                visibility: Default::default(),
            },
            MethodSignature {
                name: "decode".to_string(),
                ty: self.type_ctx.function_type(vec![string_ty], json_ty, false),
                type_params: vec!["T".to_string()],
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
            span: Span { start: 0, end: 0, line: 0, column: 0 },
            referenced: false,
        };
        let _ = self.symbols.define(symbol);
    }

    /// Register decorator-related built-in types
    ///
    /// This registers:
    /// - Class<T>: Interface representing a class constructor
    /// - ClassDecorator<T>: (target: Class<T>) => Class<T> | void
    /// - MethodDecorator<F>: (method: F) => F
    /// - FieldDecorator<T>: (target: T, fieldName: string) => void
    /// - ParameterDecorator<T>: (target: T, methodName: string, parameterIndex: number) => void
    fn register_decorator_types(&mut self) {
        let string_ty = self.type_ctx.string_type();
        let number_ty = self.type_ctx.number_type();
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
            span: Span { start: 0, end: 0, line: 0, column: 0 },
            referenced: false,
        };
        let _ = self.symbols.define(class_symbol);

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
            span: Span { start: 0, end: 0, line: 0, column: 0 },
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
            span: Span { start: 0, end: 0, line: 0, column: 0 },
            referenced: false,
        };
        let _ = self.symbols.define(method_decorator_symbol);

        // FieldDecorator<T> = (target: T, fieldName: string) => void
        let field_t_var = self.type_ctx.type_variable("T".to_string());
        let field_decorator_ty = self.type_ctx.function_type(
            vec![field_t_var, string_ty],
            void_ty,
            false,
        );
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
            span: Span { start: 0, end: 0, line: 0, column: 0 },
            referenced: false,
        };
        let _ = self.symbols.define(field_decorator_symbol);

        // ParameterDecorator<T> = (target: T, methodName: string, parameterIndex: number) => void
        let param_t_var = self.type_ctx.type_variable("T".to_string());
        let param_decorator_ty = self.type_ctx.function_type(
            vec![param_t_var, string_ty, number_ty],
            void_ty,
            false,
        );
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
            span: Span { start: 0, end: 0, line: 0, column: 0 },
            referenced: false,
        };
        let _ = self.symbols.define(param_decorator_symbol);
    }

    /// Register a single builtin class
    fn register_builtin_class(&mut self, class_sig: &super::builtins::BuiltinClass) {
        // Create type parameters map for resolving generic types
        let type_params: Vec<String> = class_sig.type_params.clone();

        // Create property signatures
        let properties: Vec<PropertySignature> = class_sig.properties.iter()
            .filter(|p| !p.is_static)
            .map(|p| PropertySignature {
                name: p.name.clone(),
                ty: self.parse_type_string(&p.ty, &type_params),
                optional: false,
                readonly: false,
                visibility: Default::default(),
            })
            .collect();

        let static_properties: Vec<PropertySignature> = class_sig.properties.iter()
            .filter(|p| p.is_static)
            .map(|p| PropertySignature {
                name: p.name.clone(),
                ty: self.parse_type_string(&p.ty, &type_params),
                optional: false,
                readonly: false,
                visibility: Default::default(),
            })
            .collect();

        // Create method signatures
        let methods: Vec<MethodSignature> = class_sig.methods.iter()
            .filter(|m| !m.is_static)
            .map(|m| {
                let param_types: Vec<TypeId> = m.params.iter()
                    .map(|(_, ty)| self.parse_type_string(ty, &type_params))
                    .collect();
                let return_ty = self.parse_type_string(&m.return_type, &type_params);
                let func_ty = self.type_ctx.function_type(param_types, return_ty, false);
                MethodSignature {
                    name: m.name.clone(),
                    ty: func_ty,
                    type_params: vec![], // Builtin methods don't have method-level type params
                    visibility: Default::default(),
                }
            })
            .collect();

        let static_methods: Vec<MethodSignature> = class_sig.methods.iter()
            .filter(|m| m.is_static)
            .map(|m| {
                let param_types: Vec<TypeId> = m.params.iter()
                    .map(|(_, ty)| self.parse_type_string(ty, &type_params))
                    .collect();
                let return_ty = self.parse_type_string(&m.return_type, &type_params);
                let func_ty = self.type_ctx.function_type(param_types, return_ty, false);
                MethodSignature {
                    name: m.name.clone(),
                    ty: func_ty,
                    type_params: vec![], // Builtin methods don't have method-level type params
                    visibility: Default::default(),
                }
            })
            .collect();

        // Create the class type
        let class_type = ClassType {
            name: class_sig.name.clone(),
            type_params: type_params.clone(),
            properties,
            methods,
            static_properties,
            static_methods,
            extends: None,
            implements: vec![],
            is_abstract: false,
        };

        let class_ty = self.type_ctx.intern(Type::Class(class_type));

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
            span: Span { start: 0, end: 0, line: 0, column: 0 },
            referenced: false,
        };

        // Ignore errors for duplicate symbols (builtins might override each other)
        let _ = self.symbols.define(symbol);
    }

    /// Register a single builtin function
    fn register_builtin_function(&mut self, func_sig: &super::builtins::BuiltinFunction) {
        let param_types: Vec<TypeId> = func_sig.params.iter()
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
            span: Span { start: 0, end: 0, line: 0, column: 0 },
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
            let type_ids: Vec<TypeId> = parts.iter()
                .map(|p| self.parse_type_string(p.trim(), type_params))
                .collect();
            return self.type_ctx.union_type(type_ids);
        }

        // Check for array types (e.g., "Array<T>")
        if ty_str.starts_with("Array<") && ty_str.ends_with('>') {
            let inner = &ty_str[6..ty_str.len()-1];
            let elem_ty = self.parse_type_string(inner, type_params);
            return self.type_ctx.array_type(elem_ty);
        }

        // Check for tuple types (e.g., "[K, V]")
        if ty_str.starts_with('[') && ty_str.ends_with(']') {
            let inner = &ty_str[1..ty_str.len()-1];
            let elem_types: Vec<TypeId> = inner.split(',')
                .map(|p| self.parse_type_string(p.trim(), type_params))
                .collect();
            return self.type_ctx.tuple_type(elem_types);
        }

        // Check for generic class types (e.g., "Set<T>", "Map<K, V>")
        if let Some(idx) = ty_str.find('<') {
            let _class_name = &ty_str[..idx];
            let args_str = &ty_str[idx+1..ty_str.len()-1];
            let _args: Vec<TypeId> = args_str.split(',')
                .map(|p| self.parse_type_string(p.trim(), type_params))
                .collect();
            // For now, just return unknown for complex generic types
            // The checker will handle instantiation
            return self.type_ctx.unknown_type();
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
                    self.type_ctx.unknown_type()
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

        if errors.is_empty() {
            Ok(self.symbols)
        } else {
            Err(errors)
        }
    }

    /// Pre-pass: register top-level class and function names as placeholder symbols.
    /// This enables forward references between declarations.
    fn prepass_stmt(&mut self, stmt: &Statement) -> Result<(), BindError> {
        match stmt {
            Statement::ClassDecl(class) => self.prepass_class(class),
            Statement::FunctionDecl(func) => self.prepass_function(func),
            Statement::ExportDecl(ExportDecl::Declaration(inner_stmt)) => self.prepass_stmt(inner_stmt),
            _ => Ok(()),
        }
    }

    /// Pre-pass: register a class name with a placeholder type
    fn prepass_class(&mut self, class: &ClassDecl) -> Result<(), BindError> {
        let class_name = self.resolve(class.name.name);

        // If a symbol with this name already exists (from builtins or forward declaration),
        // skip re-registration. Duplicate detection happens in bind_class.
        if self.symbols.resolve(&class_name).is_some() {
            return Ok(());
        }

        let type_param_names: Vec<String> = class.type_params
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
            extends: None,
            implements: vec![],
            is_abstract: class.is_abstract,
        };
        let class_ty = self.type_ctx.intern(Type::Class(placeholder));

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

        self.symbols.define(symbol).map_err(|err| BindError::DuplicateSymbol {
            name: err.name,
            original: err.original,
            duplicate: err.duplicate,
        })
    }

    /// Pre-pass: register a function name with a placeholder type
    fn prepass_function(&mut self, func: &FunctionDecl) -> Result<(), BindError> {
        let func_name = self.resolve(func.name.name);

        // If a symbol with this name already exists (from builtins or forward declaration),
        // skip re-registration. Duplicate detection happens in bind_function.
        if self.symbols.resolve(&func_name).is_some() {
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

        self.symbols.define(symbol).map_err(|err| BindError::DuplicateSymbol {
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
            Statement::ClassDecl(class) => {
                Some(self.interner.resolve(class.name.name).to_string())
            }
            Statement::TypeAliasDecl(alias) => {
                Some(self.interner.resolve(alias.name.name).to_string())
            }
            _ => None,
        }
    }

    /// Recursively register all identifiers in a pattern as variable symbols.
    fn bind_pattern_names(
        &mut self,
        pattern: &Pattern,
        ty: TypeId,
        is_const: bool,
    ) -> Result<(), BindError> {
        match pattern {
            Pattern::Identifier(ident) => {
                let name = self.resolve(ident.name);
                let symbol = Symbol {
                    name,
                    kind: SymbolKind::Variable,
                    ty,
                    flags: SymbolFlags {
                        is_exported: false,
                        is_const,
                        is_async: false,
                        is_readonly: false,
                        is_imported: false,
                    },
                    scope_id: self.symbols.current_scope_id(),
                    span: ident.span,
                    referenced: false,
                };
                self.symbols.define(symbol).map_err(|err| BindError::DuplicateSymbol {
                    name: err.name,
                    original: err.original,
                    duplicate: err.duplicate,
                })?;
            }
            Pattern::Array(array_pat) => {
                let elem_ty = self.type_ctx.unknown_type();
                for elem in array_pat.elements.iter().flatten() {
                    self.bind_pattern_names(&elem.pattern, elem_ty, is_const)?;
                }
                if let Some(rest) = &array_pat.rest {
                    self.bind_pattern_names(rest, ty, is_const)?;
                }
            }
            Pattern::Object(obj_pat) => {
                let prop_ty = self.type_ctx.unknown_type();
                for prop in &obj_pat.properties {
                    self.bind_pattern_names(&prop.value, prop_ty, is_const)?;
                }
                if let Some(rest_ident) = &obj_pat.rest {
                    let name = self.resolve(rest_ident.name);
                    let symbol = Symbol {
                        name,
                        kind: SymbolKind::Variable,
                        ty,
                        flags: SymbolFlags {
                            is_exported: false,
                            is_const,
                            is_async: false,
                            is_readonly: false,
                            is_imported: false,
                        },
                        scope_id: self.symbols.current_scope_id(),
                        span: rest_ident.span,
                        referenced: false,
                    };
                    self.symbols.define(symbol).map_err(|err| BindError::DuplicateSymbol {
                        name: err.name,
                        original: err.original,
                        duplicate: err.duplicate,
                    })?;
                }
            }
            Pattern::Rest(rest_pat) => {
                self.bind_pattern_names(&rest_pat.argument, ty, is_const)?;
            }
        }
        Ok(())
    }

    /// Bind variable declaration
    fn bind_var_decl(&mut self, decl: &VariableDecl) -> Result<(), BindError> {
        // Resolve type annotation or use unknown
        let ty = match &decl.type_annotation {
            Some(ty_annot) => self.resolve_type_annotation(ty_annot)?,
            None => self.type_ctx.unknown_type(),
        };

        let is_const = matches!(decl.kind, VariableKind::Const);
        self.bind_pattern_names(&decl.pattern, ty, is_const)
    }

    /// Bind function declaration
    fn bind_function(&mut self, func: &FunctionDecl) -> Result<(), BindError> {
        let func_name = self.resolve(func.name.name);

        // Detect duplicate function declarations
        if self.detect_top_level_duplicates {
            if let Some(&original_span) = self.bound_functions.get(&func_name) {
                return Err(BindError::DuplicateSymbol {
                    name: func_name,
                    original: original_span,
                    duplicate: func.name.span,
                });
            }
            self.bound_functions.insert(func_name.clone(), func.name.span);
        }

        // Get parent scope ID before pushing (for defining function symbol)
        let parent_scope_id = self.symbols.current_scope_id();

        // Push function scope - type parameters, parameters, and body all share this scope
        self.symbols.push_scope(ScopeKind::Function);

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
                let type_var = self.type_ctx.type_variable_with_constraint(param_name.clone(), constraint_ty);

                let tp_symbol = Symbol {
                    name: param_name,
                    kind: SymbolKind::TypeParameter,
                    ty: type_var,
                    flags: SymbolFlags::default(),
                    scope_id: self.symbols.current_scope_id(),
                    span: type_param.span,
                    referenced: false,
                };

                self.symbols.define(tp_symbol).map_err(|err| BindError::DuplicateSymbol {
                    name: err.name,
                    original: err.original,
                    duplicate: err.duplicate,
                })?;
            }
        }

        // Build function type from parameters and return type
        // Type parameters are now in scope, so we can resolve them in param types
        let mut param_types = Vec::new();
        for param in &func.params {
            let param_ty = match &param.type_annotation {
                Some(ty_annot) => self.resolve_type_annotation(ty_annot)?,
                None => self.type_ctx.unknown_type(),
            };
            param_types.push(param_ty);
        }

        let return_ty = match &func.return_type {
            Some(ty_annot) => self.resolve_type_annotation(ty_annot)?,
            None => self.type_ctx.void_type(),
        };

        // Validate parameter ordering: required params must come before optional/default params
        self.validate_param_order(&func.params)?;

        // Count required params (those without default values and not optional)
        let min_params = func.params.iter().filter(|p| p.default_value.is_none() && !p.optional).count();
        let func_ty = self.type_ctx.function_type_with_min_params(param_types.clone(), return_ty, func.is_async, min_params);

        // Define function symbol in parent scope (so it can be called recursively)
        // If pre-registered by the pre-pass, update the type instead of re-defining
        if self.symbols.resolve(&func_name).is_some() {
            self.symbols.update_type(parent_scope_id, &func_name, func_ty);
        } else {
            let symbol = Symbol {
                name: func_name,
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
            self.symbols.define_in_scope(parent_scope_id, symbol).map_err(|err| BindError::DuplicateSymbol {
                name: err.name,
                original: err.original,
                duplicate: err.duplicate,
            })?;
        }

        // Bind parameters in the function scope
        // Note: param types are already resolved above, we use param_types[i]
        for (i, param) in func.params.iter().enumerate() {
            // Extract identifier from pattern (simplified)
            let (param_name, param_span) = match &param.pattern {
                Pattern::Identifier(ident) => (self.resolve(ident.name), ident.span),
                _ => continue, // Skip destructuring for now
            };

            let param_symbol = Symbol {
                name: param_name,
                kind: SymbolKind::Variable,
                ty: param_types[i],
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

            self.symbols.define(param_symbol).map_err(|err| BindError::DuplicateSymbol {
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
        use crate::parser::types::ty::{ClassType, PropertySignature, MethodSignature, Type};

        let class_name = self.resolve(class.name.name);

        // Detect duplicate class declarations using the bound_classes set.
        if self.detect_top_level_duplicates {
            if let Some(&original_span) = self.bound_classes.get(&class_name) {
                return Err(BindError::DuplicateSymbol {
                    name: class_name,
                    original: original_span,
                    duplicate: class.name.span,
                });
            }
            self.bound_classes.insert(class_name.clone(), class.name.span);
        }

        // Collect type parameters (K, V, T, etc.)
        let type_param_names: Vec<String> = class.type_params
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
            extends: None,
            implements: vec![],
            is_abstract: class.is_abstract,
        };
        let class_ty = self.type_ctx.intern(Type::Class(placeholder_type));

        // Store the scope ID where the class is defined (for later update)
        let class_definition_scope = self.symbols.current_scope_id();

        // If the class was already registered by the pre-pass, update its type;
        // otherwise define it now (handles non-top-level classes)
        if self.symbols.resolve(&class_name).is_some() {
            self.symbols.update_type(class_definition_scope, &class_name, class_ty);
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
            self.symbols.define(symbol).map_err(|err| BindError::DuplicateSymbol {
                name: err.name,
                original: err.original,
                duplicate: err.duplicate,
            })?;
        }

        // Enter class scope for type parameters
        self.symbols.push_scope(ScopeKind::Class);

        // Register type parameters as type aliases in class scope
        for type_param_name in &type_param_names {
            let type_var = self.type_ctx.type_variable(type_param_name.clone());
            let symbol = Symbol {
                name: type_param_name.clone(),
                kind: SymbolKind::TypeAlias,
                ty: type_var,
                flags: SymbolFlags::default(),
                scope_id: self.symbols.current_scope_id(),
                span: Span { start: 0, end: 0, line: 0, column: 0 },
                referenced: false,
            };
            let _ = self.symbols.define(symbol);
        }

        // Now collect properties and methods (class name is now resolvable)
        // Separate instance and static members
        let mut properties = Vec::new();
        let mut methods = Vec::new();
        let mut static_properties = Vec::new();
        let mut static_methods = Vec::new();

        // Track seen field/method names for duplicate detection
        let mut seen_fields: std::collections::HashMap<String, Span> = std::collections::HashMap::new();
        let mut seen_methods: std::collections::HashMap<String, Span> = std::collections::HashMap::new();

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
                        for type_param_name in &method_type_params {
                            let type_var = self.type_ctx.type_variable(type_param_name.clone());
                            let symbol = Symbol {
                                name: type_param_name.clone(),
                                kind: SymbolKind::TypeAlias,
                                ty: type_var,
                                flags: SymbolFlags::default(),
                                scope_id: self.symbols.current_scope_id(),
                                span: Span { start: 0, end: 0, line: 0, column: 0 },
                                referenced: false,
                            };
                            let _ = self.symbols.define(symbol);
                        }
                    }

                    // Create function type for the method
                    let mut params = Vec::new();
                    for p in &method.params {
                        let param_ty = if let Some(ref ann) = p.type_annotation {
                            self.resolve_type_annotation(ann)?
                        } else {
                            self.type_ctx.unknown_type()
                        };
                        params.push(param_ty);
                    }
                    // Placeholder for return type - will be fixed up below
                    let return_ty = if let Some(ref ann) = method.return_type {
                        self.resolve_type_annotation(ann)?
                    } else {
                        self.type_ctx.void_type()
                    };

                    // Pop the temporary scope for method type parameters
                    if has_method_type_params {
                        self.symbols.pop_scope();
                    }

                    // Validate parameter ordering
                    self.validate_param_order(&method.params)?;

                    let min_params = method.params.iter().filter(|p| p.default_value.is_none() && !p.optional).count();
                    if method.is_static {
                        static_methods.push((method_name, params, return_ty, method.is_async, method_type_params.clone(), method.visibility, min_params));
                    } else {
                        methods.push((method_name, params, return_ty, method.is_async, method_type_params, method.visibility, min_params));
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
                }
            }
        }

        // Create method signatures with proper return types
        // If return type equals the placeholder class_ty, we need to create a self-referential type
        // We'll create the full class type first, then fix up method return types that reference it

        // First pass: create instance method signatures
        let method_sigs: Vec<MethodSignature> = methods
            .into_iter()
            .map(|(name, params, return_ty, is_async, method_type_params, vis, min_params)| {
                let func_ty = self.type_ctx.function_type_with_min_params(params, return_ty, is_async, min_params);
                MethodSignature { name, ty: func_ty, type_params: method_type_params, visibility: vis }
            })
            .collect();

        // Create static method signatures
        let static_method_sigs: Vec<MethodSignature> = static_methods
            .into_iter()
            .map(|(name, params, return_ty, is_async, method_type_params, vis, min_params)| {
                let func_ty = self.type_ctx.function_type_with_min_params(params, return_ty, is_async, min_params);
                MethodSignature { name, ty: func_ty, type_params: method_type_params, visibility: vis }
            })
            .collect();

        // Resolve the extends clause if present
        let extends_ty = if let Some(ref extends_ann) = class.extends {
            Some(self.resolve_type_annotation(extends_ann)?)
        } else {
            None
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
        self.type_ctx.replace_type(class_ty, Type::Class(full_class_type));

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
                                let type_var = self.type_ctx.type_variable(type_param_name.clone());
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

                        for stmt in &body.statements {
                            self.bind_stmt(stmt)?;
                        }
                        self.symbols.pop_scope();
                    }
                }
                ClassMember::Constructor(ctor) => {
                    self.symbols.push_scope(ScopeKind::Function);
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
                let type_var = self.type_ctx.type_variable(param_name.clone());
                let sym = Symbol {
                    name: param_name.clone(),
                    kind: SymbolKind::TypeAlias,
                    ty: type_var,
                    flags: SymbolFlags::default(),
                    scope_id: self.symbols.current_scope_id(),
                    span: Span { start: 0, end: 0, line: 0, column: 0 },
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
            self.generic_type_alias_params.insert(alias_name.clone(), type_param_names);
        }

        let symbol = Symbol {
            name: alias_name,
            kind: SymbolKind::TypeAlias,
            ty,
            flags: SymbolFlags::default(),
            scope_id: self.symbols.current_scope_id(),
            span: alias.name.span,
            referenced: false,
        };

        self.symbols.define(symbol).map_err(|err| BindError::DuplicateSymbol {
            name: err.name,
            original: err.original,
            duplicate: err.duplicate,
        })
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
                let (param_name, param_span) = match param {
                    Pattern::Identifier(ident) => (self.resolve(ident.name), ident.span),
                    _ => {
                        // TODO: Handle destructuring
                        ("error".to_string(), catch.body.span)
                    }
                };

                // Look up Error class type for catch parameter
                // If Error class isn't registered, fall back to unknown
                let error_ty = self.symbols.resolve("Error")
                    .map(|s| s.ty)
                    .unwrap_or_else(|| self.type_ctx.unknown_type());

                let param_symbol = Symbol {
                    name: param_name,
                    kind: SymbolKind::Variable,
                    ty: error_ty,
                    flags: SymbolFlags {
                        is_exported: false,
                        is_const: true,
                        is_async: false,
                        is_readonly: false,
                        is_imported: false,
                    },
                    scope_id: self.symbols.current_scope_id(),
                    span: param_span,
                    referenced: false,
                };

                self.symbols.define(param_symbol).map_err(|err| BindError::DuplicateSymbol {
                    name: err.name,
                    original: err.original,
                    duplicate: err.duplicate,
                })?;
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
    fn validate_param_order(&self, params: &[crate::parser::ast::Parameter]) -> Result<(), BindError> {
        let mut seen_optional = false;
        for param in params {
            let is_optional = param.optional || param.default_value.is_some();
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

    /// Recursively substitute TypeVars in a type according to a substitution map
    fn substitute_type_vars(&mut self, ty: TypeId, subs: &std::collections::HashMap<String, TypeId>) -> TypeId {
        let type_info = self.type_ctx.get(ty).cloned();
        match type_info {
            Some(Type::TypeVar(tv)) => {
                if let Some(&sub) = subs.get(&tv.name) {
                    sub
                } else {
                    ty
                }
            }
            Some(Type::Object(obj)) => {
                let new_props: Vec<_> = obj.properties.iter().map(|p| {
                    PropertySignature {
                        name: p.name.clone(),
                        ty: self.substitute_type_vars(p.ty, subs),
                        optional: p.optional,
                        readonly: p.readonly,
                        visibility: p.visibility,
                    }
                }).collect();
                self.type_ctx.object_type(new_props)
            }
            Some(Type::Union(union)) => {
                let new_members: Vec<_> = union.members.iter().map(|&m| {
                    self.substitute_type_vars(m, subs)
                }).collect();
                self.type_ctx.union_type(new_members)
            }
            Some(Type::Function(func)) => {
                let new_params: Vec<_> = func.params.iter().map(|&p| {
                    self.substitute_type_vars(p, subs)
                }).collect();
                let new_ret = self.substitute_type_vars(func.return_type, subs);
                self.type_ctx.function_type(new_params, new_ret, func.is_async)
            }
            Some(Type::Array(arr)) => {
                let new_elem = self.substitute_type_vars(arr.element, subs);
                self.type_ctx.array_type(new_elem)
            }
            Some(Type::Class(class)) => {
                let new_props: Vec<_> = class.properties.iter().map(|p| {
                    PropertySignature {
                        name: p.name.clone(),
                        ty: self.substitute_type_vars(p.ty, subs),
                        optional: p.optional,
                        readonly: p.readonly,
                        visibility: p.visibility,
                    }
                }).collect();
                let new_methods: Vec<_> = class.methods.iter().map(|m| {
                    MethodSignature {
                        name: m.name.clone(),
                        ty: self.substitute_type_vars(m.ty, subs),
                        type_params: m.type_params.clone(),
                        visibility: m.visibility,
                    }
                }).collect();
                let new_extends = class.extends.map(|e| self.substitute_type_vars(e, subs));
                let new_class = ClassType {
                    name: class.name.clone(),
                    type_params: vec![], // Specialized class has no type params
                    properties: new_props,
                    methods: new_methods,
                    static_properties: class.static_properties.clone(),
                    static_methods: class.static_methods.clone(),
                    extends: new_extends,
                    implements: class.implements.clone(),
                    is_abstract: class.is_abstract,
                };
                self.type_ctx.intern(Type::Class(new_class))
            }
            _ => ty,
        }
    }

    /// Resolve type to TypeId
    fn resolve_type(&mut self, ty: &crate::parser::ast::Type, span: crate::parser::Span) -> Result<TypeId, BindError> {
        use crate::parser::ast::Type as AstType;

        match ty {
            AstType::Primitive(prim) => Ok(self.resolve_primitive(*prim)),

            AstType::Reference(type_ref) => {
                // Check if it's a user-defined type or type parameter
                let name = self.resolve(type_ref.name.name);

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

                // Handle Task<T> for async functions
                if name == TC::TASK_TYPE_NAME {
                    if let Some(ref type_args) = type_ref.type_args {
                        if type_args.len() == 1 {
                            let result_ty = self.resolve_type_annotation(&type_args[0])?;
                            return Ok(self.type_ctx.task_type(result_ty));
                        }
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
                    }
                    return Err(BindError::InvalidTypeArguments {
                        name,
                        expected: 1,
                        actual: type_ref.type_args.as_ref().map(|a| a.len()).unwrap_or(0),
                        span,
                    });
                }

                // Mutex is a normal class from Mutex.raya, no special handling needed

                if let Some(symbol) = self.symbols.resolve(&name) {
                    if symbol.kind == SymbolKind::TypeAlias
                        || symbol.kind == SymbolKind::TypeParameter
                        || symbol.kind == SymbolKind::Class {
                        let template_ty = symbol.ty;

                        // Check if this is a generic type with type arguments
                        if let Some(ref type_args) = type_ref.type_args {
                            // Try type alias params first
                            let param_names = if let Some(names) = self.generic_type_alias_params.get(&name).cloned() {
                                Some(names)
                            } else if let Some(Type::Class(class_ty)) = self.type_ctx.get(template_ty).cloned() {
                                // For classes, read type_params from the ClassType
                                if !class_ty.type_params.is_empty() {
                                    Some(class_ty.type_params.clone())
                                } else {
                                    None
                                }
                            } else {
                                None
                            };

                            if let Some(param_names) = param_names {
                                if type_args.len() == param_names.len() {
                                    // Resolve each type argument
                                    let mut resolved_args = Vec::new();
                                    for arg in type_args {
                                        resolved_args.push(self.resolve_type_annotation(arg)?);
                                    }
                                    // Build substitution map: param_name → concrete type
                                    let mut subs = std::collections::HashMap::new();
                                    for (param_name, arg_ty) in param_names.iter().zip(resolved_args.iter()) {
                                        subs.insert(param_name.clone(), *arg_ty);
                                    }
                                    // Apply substitution to the template type
                                    return Ok(self.substitute_type_vars(template_ty, &subs));
                                }
                            }
                        }

                        Ok(template_ty)
                    } else {
                        Err(BindError::NotAType {
                            name,
                            span,
                        })
                    }
                } else {
                    Err(BindError::UndefinedType {
                        name,
                        span,
                    })
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
                // Resolve all constituent types and merge their properties into a single Object type
                let mut merged_properties = Vec::new();
                for ty_annot in &intersection.types {
                    let ty_id = self.resolve_type_annotation(ty_annot)?;
                    if let Some(crate::parser::types::Type::Object(obj)) = self.type_ctx.get(ty_id).cloned() {
                        for prop in &obj.properties {
                            if !merged_properties.iter().any(|p: &crate::parser::types::ty::PropertySignature| p.name == prop.name) {
                                merged_properties.push(prop.clone());
                            }
                        }
                    }
                }
                Ok(self.type_ctx.object_type(merged_properties))
            }

            AstType::Function(func) => {
                let param_tys: Result<Vec<_>, _> = func
                    .params
                    .iter()
                    .map(|p| self.resolve_type_annotation(&p.ty))
                    .collect();

                let return_ty = self.resolve_type_annotation(&func.return_type)?;

                Ok(self.type_ctx.function_type(param_tys?, return_ty, false))
            }

            AstType::Object(obj) => {
                use crate::parser::ast::ObjectTypeMember;
                use crate::parser::types::ty::{ObjectType, PropertySignature};

                let mut properties = Vec::new();

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
                            let param_tys: Result<Vec<_>, _> = method
                                .params
                                .iter()
                                .map(|p| self.resolve_type_annotation(&p.ty))
                                .collect();

                            let return_ty = self.resolve_type_annotation(&method.return_type)?;
                            let func_ty = self.type_ctx.function_type(param_tys?, return_ty, false);

                            properties.push(PropertySignature {
                                name: self.resolve(method.name.name),
                                ty: func_ty,
                                optional: false,
                                readonly: false,
                                visibility: Default::default(),
                            });
                        }
                    }
                }

                let object_type = ObjectType {
                    properties,
                    index_signature: None,
                };

                Ok(self.type_ctx.intern(crate::parser::types::ty::Type::Object(object_type)))
            }

            AstType::Typeof(_) => {
                // typeof types are resolved during type checking
                Ok(self.type_ctx.unknown_type())
            }

            AstType::StringLiteral(s) => {
                Ok(self.type_ctx.string_literal(self.interner.resolve(*s).to_string()))
            }

            AstType::NumberLiteral(n) => {
                Ok(self.type_ctx.number_literal(*n))
            }

            AstType::BooleanLiteral(b) => {
                Ok(self.type_ctx.boolean_literal(*b))
            }

            AstType::Parenthesized(inner) => {
                self.resolve_type_annotation(inner)
            }
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
        let (symbols, _ctx) = parse_and_bind(
            "function add(a: number, b: number): number { return a + b; }"
        );

        // Should be able to resolve add
        let symbol = symbols.resolve("add").unwrap();
        assert_eq!(symbol.name, "add");
        assert_eq!(symbol.kind, SymbolKind::Function);
    }
}
