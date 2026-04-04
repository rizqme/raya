use std::sync::OnceLock;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::compiler::builtins::{
    builtin_op_from_native_id, native_id_for_builtin_op, BuiltinOp, BuiltinRegistry,
};
use crate::compiler::module::BuiltinSurfaceMode;
use crate::compiler::type_registry::{DispatchAction, OpcodeKind};
use crate::parser::ast::{
    self, ClassMember, ExportDecl, Expression, ObjectProperty, Pattern, PropertyKey, PropertyKind,
    Statement, Type as AstType, TypeAnnotation,
};
use crate::parser::checker::SymbolKind;
use crate::parser::{Interner, Parser, TypeContext};
use crate::semantics::{SemanticProfile, SourceKind};

static STRICT_BUILTIN_SURFACE: OnceLock<BuiltinSurfaceManifest> = OnceLock::new();
static NODE_COMPAT_BUILTIN_SURFACE: OnceLock<BuiltinSurfaceManifest> = OnceLock::new();

pub(crate) fn builtin_surface_mode_for_profile(profile: SemanticProfile) -> BuiltinSurfaceMode {
    match profile.source_kind {
        SourceKind::Raya => BuiltinSurfaceMode::RayaStrict,
        SourceKind::Js | SourceKind::Ts => BuiltinSurfaceMode::NodeCompat,
    }
}

pub(crate) fn builtin_surface_manifest_for_mode(
    mode: BuiltinSurfaceMode,
) -> &'static BuiltinSurfaceManifest {
    let cache = match mode {
        BuiltinSurfaceMode::RayaStrict => &STRICT_BUILTIN_SURFACE,
        BuiltinSurfaceMode::NodeCompat => &NODE_COMPAT_BUILTIN_SURFACE,
    };
    cache.get_or_init(|| build_builtin_surface_manifest(mode))
}

pub(crate) fn builtin_class_method_sources() -> &'static [(&'static str, &'static str)] {
    &[]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuiltinGlobalKind {
    Namespace,
    ClassValue,
    StaticValue,
    Value,
}

#[derive(Debug, Clone)]
pub(crate) struct BuiltinSurfaceManifest {
    pub(crate) mode: BuiltinSurfaceMode,
    pub(crate) globals: FxHashMap<String, BuiltinGlobalSurface>,
    pub(crate) types: FxHashMap<String, BuiltinTypeSurface>,
    builtin_global_names: FxHashSet<String>,
    builtin_namespace_names: FxHashSet<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct BuiltinGlobalSurface {
    pub(crate) kind: BuiltinGlobalKind,
    pub(crate) backing_type_name: Option<String>,
    pub(crate) static_methods: FxHashMap<String, BuiltinDispatchBinding>,
    pub(crate) static_properties: FxHashMap<String, BuiltinDispatchBinding>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct BuiltinTypeSurface {
    pub(crate) builtin_primitive: bool,
    pub(crate) wrapper_method_surface: bool,
    pub(crate) constructor_binding: Option<BuiltinDispatchBinding>,
    pub(crate) instance_methods: FxHashMap<String, BuiltinDispatchBinding>,
    pub(crate) instance_properties: FxHashMap<String, BuiltinDispatchBinding>,
    pub(crate) static_methods: FxHashMap<String, BuiltinDispatchBinding>,
    pub(crate) static_properties: FxHashMap<String, BuiltinDispatchBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BuiltinDispatchBinding {
    SourceDefined,
    Opcode(OpcodeKind),
    Builtin {
        op: BuiltinOp,
        return_type_name: Option<String>,
    },
    DeclaredField {
        field_type_name: Option<String>,
        field_index: Option<u16>,
    },
    ClassMethod {
        type_name: String,
        method_name: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeWrapperMode {
    ExplicitArgsOnly,
    AllowReceiverThis,
}

#[derive(Debug, Clone)]
enum LocalBuiltinBindingKind {
    Class(String),
    NamespaceInstance(String),
    Function,
    Value,
}

#[derive(Debug, Clone)]
struct ParsedFunctionBinding {
    binding: BuiltinDispatchBinding,
    callable: bool,
}

#[derive(Default)]
struct StaticSurfacePatch {
    methods: FxHashMap<String, BuiltinDispatchBinding>,
    properties: FxHashMap<String, BuiltinDispatchBinding>,
}

impl BuiltinSurfaceManifest {
    pub(crate) fn global_surface(&self, name: &str) -> Option<&BuiltinGlobalSurface> {
        self.globals.get(name)
    }

    pub(crate) fn type_surface(&self, name: &str) -> Option<&BuiltinTypeSurface> {
        self.types.get(name)
    }

    pub(crate) fn is_builtin_global(&self, name: &str) -> bool {
        self.builtin_global_names.contains(name)
    }

    pub(crate) fn is_namespace_global(&self, name: &str) -> bool {
        self.builtin_namespace_names.contains(name)
    }

    pub(crate) fn global_kind(&self, name: &str) -> Option<BuiltinGlobalKind> {
        self.globals.get(name).map(|surface| surface.kind)
    }

    pub(crate) fn backing_type_name(&self, global_name: &str) -> Option<&str> {
        self.globals
            .get(global_name)
            .and_then(|surface| surface.backing_type_name.as_deref())
    }

    pub(crate) fn global_uses_static_surface(&self, name: &str) -> bool {
        self.globals.get(name).is_some_and(|surface| {
            matches!(
                surface.kind,
                BuiltinGlobalKind::Namespace
                    | BuiltinGlobalKind::ClassValue
                    | BuiltinGlobalKind::StaticValue
            )
        })
    }

    pub(crate) fn has_dispatch_type(&self, type_name: &str) -> bool {
        self.types.contains_key(type_name)
    }

    pub(crate) fn static_method_binding(
        &self,
        global_name: &str,
        member_name: &str,
    ) -> Option<&BuiltinDispatchBinding> {
        self.globals
            .get(global_name)
            .and_then(|surface| surface.static_methods.get(member_name))
    }

    pub(crate) fn static_property_binding(
        &self,
        global_name: &str,
        property_name: &str,
    ) -> Option<&BuiltinDispatchBinding> {
        self.globals
            .get(global_name)
            .and_then(|surface| surface.static_properties.get(property_name))
    }

    pub(crate) fn instance_method_binding(
        &self,
        type_name: &str,
        member_name: &str,
    ) -> Option<&BuiltinDispatchBinding> {
        self.types
            .get(type_name)
            .and_then(|surface| surface.instance_methods.get(member_name))
    }

    pub(crate) fn instance_property_binding(
        &self,
        type_name: &str,
        property_name: &str,
    ) -> Option<&BuiltinDispatchBinding> {
        self.types
            .get(type_name)
            .and_then(|surface| surface.instance_properties.get(property_name))
    }

    pub(crate) fn has_static_method(&self, global_name: &str, member_name: &str) -> bool {
        self.globals
            .get(global_name)
            .is_some_and(|surface| surface.static_methods.contains_key(member_name))
    }

    pub(crate) fn has_static_property(&self, global_name: &str, property_name: &str) -> bool {
        self.globals
            .get(global_name)
            .is_some_and(|surface| surface.static_properties.contains_key(property_name))
    }

    pub(crate) fn lookup_static_method(
        &self,
        global_name: &str,
        member_name: &str,
        type_ctx: &TypeContext,
    ) -> Option<DispatchAction> {
        self.globals
            .get(global_name)
            .and_then(|surface| surface.static_methods.get(member_name))
            .and_then(|binding| binding.to_dispatch_action(type_ctx))
    }

    pub(crate) fn lookup_static_property(
        &self,
        global_name: &str,
        property_name: &str,
        type_ctx: &TypeContext,
    ) -> Option<DispatchAction> {
        self.globals
            .get(global_name)
            .and_then(|surface| surface.static_properties.get(property_name))
            .and_then(|binding| binding.to_dispatch_action(type_ctx))
    }

    pub(crate) fn lookup_instance_method(
        &self,
        type_name: &str,
        member_name: &str,
        type_ctx: &TypeContext,
    ) -> Option<DispatchAction> {
        self.types
            .get(type_name)
            .and_then(|surface| surface.instance_methods.get(member_name))
            .and_then(|binding| binding.to_dispatch_action(type_ctx))
    }

    pub(crate) fn lookup_instance_property(
        &self,
        type_name: &str,
        property_name: &str,
        type_ctx: &TypeContext,
    ) -> Option<DispatchAction> {
        self.types
            .get(type_name)
            .and_then(|surface| surface.instance_properties.get(property_name))
            .and_then(|binding| binding.to_dispatch_action(type_ctx))
    }

    pub(crate) fn constructor_native_id(&self, type_name: &str) -> Option<u16> {
        self.types
            .get(type_name)
            .and_then(|surface| {
                surface
                    .constructor_binding
                    .as_ref()
                    .and_then(BuiltinDispatchBinding::native_id)
            })
    }

    pub(crate) fn constructor_binding(&self, type_name: &str) -> Option<&BuiltinDispatchBinding> {
        self.types
            .get(type_name)
            .and_then(|surface| surface.constructor_binding.as_ref())
    }
}

impl BuiltinDispatchBinding {
    pub(crate) fn to_dispatch_action(&self, type_ctx: &TypeContext) -> Option<DispatchAction> {
        match self {
            Self::SourceDefined => None,
            Self::Opcode(kind) => Some(DispatchAction::Opcode(*kind)),
            Self::Builtin { op, .. } => Some(DispatchAction::Builtin(*op)),
            Self::DeclaredField {
                field_type_name,
                field_index,
            } => Some(DispatchAction::DeclaredField {
                field_type: field_type_name
                    .as_deref()
                    .and_then(|name| resolve_type_name(type_ctx, name)),
                field_index: *field_index,
            }),
            Self::ClassMethod {
                type_name,
                method_name,
            } => Some(DispatchAction::ClassMethod(
                type_name.clone(),
                method_name.clone(),
            )),
        }
    }

    pub(crate) fn return_type_name(&self) -> Option<&str> {
        match self {
            Self::Builtin {
                return_type_name: Some(name),
                ..
            } => Some(name.as_str()),
            _ => None,
        }
    }

    pub(crate) fn native_id(&self) -> Option<u16> {
        match self {
            Self::Builtin { op, .. } => native_id_for_builtin_op(*op),
            _ => None,
        }
    }

    pub(crate) fn builtin_op(&self) -> Option<BuiltinOp> {
        match self {
            Self::Builtin { op, .. } => Some(*op),
            _ => None,
        }
    }
}

fn build_builtin_surface_manifest(mode: BuiltinSurfaceMode) -> BuiltinSurfaceManifest {
    let mut classes = FxHashMap::<String, BuiltinTypeSurface>::default();
    let mut globals = FxHashMap::<String, BuiltinGlobalSurface>::default();
    let mut exported_names = FxHashSet::<String>::default();
    let mut builtin_global_names = FxHashSet::default();
    let mut builtin_namespace_names = FxHashSet::default();
    seed_builtin_surface_from_signatures(
        mode,
        &mut classes,
        &mut globals,
        &mut exported_names,
        &mut builtin_global_names,
    );
    if matches!(mode, BuiltinSurfaceMode::NodeCompat) {
        builtin_global_names.insert("globalThis".to_string());
        exported_names.insert("globalThis".to_string());
        globals
            .entry("globalThis".to_string())
            .or_insert_with(|| BuiltinGlobalSurface {
                kind: BuiltinGlobalKind::Value,
                backing_type_name: None,
                static_methods: FxHashMap::default(),
                static_properties: FxHashMap::default(),
            });
    }

    apply_builtin_registry_overlays(mode, &mut classes);

    apply_builtin_registry_global_overlays(mode, &mut globals, &mut builtin_global_names);

    BuiltinSurfaceManifest {
        mode,
        globals,
        types: classes,
        builtin_global_names,
        builtin_namespace_names,
    }
}

fn seed_builtin_surface_from_signatures(
    mode: BuiltinSurfaceMode,
    classes: &mut FxHashMap<String, BuiltinTypeSurface>,
    globals: &mut FxHashMap<String, BuiltinGlobalSurface>,
    exported_names: &mut FxHashSet<String>,
    builtin_global_names: &mut FxHashSet<String>,
) {
    for signatures in crate::vm::builtins::get_all_signatures() {
        for class in signatures.classes {
            if class.name.starts_with("__") || !builtin_root_visible_in_mode(class.name, mode) {
                continue;
            }

            let surface = classes.entry(class.name.to_string()).or_default();
            if class.constructor.is_some() {
                surface
                    .constructor_binding
                    .get_or_insert(BuiltinDispatchBinding::SourceDefined);
            }

            for property in class.properties {
                if property.is_static
                    && !builtin_static_member_visible_in_mode(class.name, property.name, mode)
                {
                    continue;
                }
                let target = if property.is_static {
                    &mut surface.static_properties
                } else {
                    &mut surface.instance_properties
                };
                target
                    .entry(property.name.to_string())
                    .or_insert(BuiltinDispatchBinding::SourceDefined);
            }

            for method in class.methods {
                if method.is_static
                    && !builtin_static_member_visible_in_mode(class.name, method.name, mode)
                {
                    continue;
                }
                let target = if method.is_static {
                    &mut surface.static_methods
                } else {
                    &mut surface.instance_methods
                };
                target
                    .entry(method.name.to_string())
                    .or_insert(BuiltinDispatchBinding::SourceDefined);
            }

            exported_names.insert(class.name.to_string());
            builtin_global_names.insert(class.name.to_string());
            let has_static_surface =
                class.methods.iter().any(|method| method.is_static)
                    || class.properties.iter().any(|property| property.is_static);
            let kind = if class.constructor.is_some() {
                BuiltinGlobalKind::ClassValue
            } else if has_static_surface {
                BuiltinGlobalKind::StaticValue
            } else {
                BuiltinGlobalKind::Value
            };
            let global = globals
                .entry(class.name.to_string())
                .or_insert_with(|| BuiltinGlobalSurface {
                    kind,
                    backing_type_name: Some(class.name.to_string()),
                    static_methods: FxHashMap::default(),
                    static_properties: FxHashMap::default(),
                });
            if global.backing_type_name.is_none() {
                global.backing_type_name = Some(class.name.to_string());
            }
            for (member_name, binding) in &surface.static_methods {
                global
                    .static_methods
                    .entry(member_name.clone())
                    .or_insert_with(|| binding.clone());
            }
            for (property_name, binding) in &surface.static_properties {
                global
                    .static_properties
                    .entry(property_name.clone())
                    .or_insert_with(|| binding.clone());
            }
        }

        for function in signatures.functions {
            if !builtin_root_visible_in_mode(function.name, mode) {
                continue;
            }
            exported_names.insert(function.name.to_string());
            builtin_global_names.insert(function.name.to_string());
            globals
                .entry(function.name.to_string())
                .or_insert_with(|| BuiltinGlobalSurface {
                    kind: BuiltinGlobalKind::Value,
                    backing_type_name: None,
                    static_methods: FxHashMap::default(),
                    static_properties: FxHashMap::default(),
                });
        }
    }
}

fn builtin_root_visible_in_mode(name: &str, mode: BuiltinSurfaceMode) -> bool {
    if matches!(mode, BuiltinSurfaceMode::NodeCompat) {
        return true;
    }
    !matches!(
        name,
        "ArrayBuffer"
            | "DataView"
            | "Uint8Array"
            | "Uint8ClampedArray"
            | "Int8Array"
            | "Int16Array"
            | "Int32Array"
            | "Uint16Array"
            | "Uint32Array"
            | "Float32Array"
            | "Float16Array"
            | "Float64Array"
            | "BigInt"
            | "BigInt64Array"
            | "BigUint64Array"
            | "TypedArray"
            | "SharedArrayBuffer"
            | "Atomics"
            | "parseInt"
            | "parseFloat"
            | "isNaN"
            | "isFinite"
            | "eval"
            | "Function"
            | "AsyncFunction"
            | "Generator"
            | "GeneratorFunction"
            | "AsyncGenerator"
            | "AsyncGeneratorFunction"
            | "AsyncIterator"
            | "Proxy"
            | "Reflect"
            | "WeakMap"
            | "WeakSet"
            | "WeakRef"
            | "FinalizationRegistry"
            | "DisposableStack"
            | "AsyncDisposableStack"
            | "Intl"
            | "globalThis"
            | "escape"
            | "unescape"
    )
}

fn builtin_static_member_visible_in_mode(
    type_name: &str,
    member_name: &str,
    mode: BuiltinSurfaceMode,
) -> bool {
    if matches!(mode, BuiltinSurfaceMode::NodeCompat) {
        return true;
    }
    !matches!(
        (type_name, member_name),
        ("Object", "defineProperty")
            | ("Object", "getOwnPropertyDescriptor")
            | ("Object", "defineProperties")
    )
}

fn apply_builtin_registry_global_overlays(
    mode: BuiltinSurfaceMode,
    globals: &mut FxHashMap<String, BuiltinGlobalSurface>,
    builtin_global_names: &mut FxHashSet<String>,
) {
    for (global_name, descriptor) in BuiltinRegistry::shared().global_descriptors() {
        if !builtin_root_visible_in_mode(global_name, mode) {
            continue;
        }
        builtin_global_names.insert(global_name.to_string());
        let mut registry_static_methods = FxHashMap::default();
        if let Some(type_descriptor) =
            BuiltinRegistry::shared().type_descriptor(descriptor.backing_type_name)
        {
            for (method_name, binding) in &type_descriptor.static_methods {
                if !builtin_static_member_visible_in_mode(global_name, method_name, mode) {
                    continue;
                }
                registry_static_methods.insert(
                    (*method_name).to_string(),
                    BuiltinDispatchBinding::Builtin {
                        op: binding.op,
                        return_type_name: binding.return_type_name.map(str::to_string),
                    },
                );
            }
        }
        let entry = globals
            .entry(global_name.to_string())
            .or_insert_with(|| BuiltinGlobalSurface {
                kind: BuiltinGlobalKind::ClassValue,
                backing_type_name: Some(descriptor.backing_type_name.to_string()),
                static_methods: FxHashMap::default(),
                static_properties: FxHashMap::default(),
            });
        if entry.backing_type_name.is_none() {
            entry.backing_type_name = Some(descriptor.backing_type_name.to_string());
        }
        for (method_name, binding) in registry_static_methods {
            entry.static_methods.insert(method_name, binding);
        }
    }
}

fn apply_builtin_registry_overlays(
    mode: BuiltinSurfaceMode,
    classes: &mut FxHashMap<String, BuiltinTypeSurface>,
) {
    for (type_name, descriptor) in BuiltinRegistry::shared().type_descriptors() {
        if !builtin_root_visible_in_mode(type_name, mode) {
            continue;
        }
        let surface = classes.entry(type_name.to_string()).or_default();
        surface.builtin_primitive |= descriptor.builtin_primitive;
        surface.wrapper_method_surface |= descriptor.wrapper_method_surface;
        if let Some(constructor) = descriptor.constructor {
            surface.constructor_binding = Some(BuiltinDispatchBinding::Builtin {
                op: constructor.op,
                return_type_name: constructor.return_type_name.map(str::to_string),
            });
        }
        for (method_name, binding) in &descriptor.instance_methods {
            surface.instance_methods.insert(
                (*method_name).to_string(),
                BuiltinDispatchBinding::Builtin {
                    op: binding.op,
                    return_type_name: binding.return_type_name.map(str::to_string),
                },
            );
        }
        for (method_name, binding) in &descriptor.static_methods {
            if !builtin_static_member_visible_in_mode(type_name, method_name, mode) {
                continue;
            }
            surface.static_methods.insert(
                (*method_name).to_string(),
                BuiltinDispatchBinding::Builtin {
                    op: binding.op,
                    return_type_name: binding.return_type_name.map(str::to_string),
                },
            );
        }
        for (property_name, binding) in &descriptor.instance_properties {
            surface.instance_properties.insert(
                (*property_name).to_string(),
                BuiltinDispatchBinding::Builtin {
                    op: binding.op,
                    return_type_name: binding.return_type_name.map(str::to_string),
                },
            );
        }
    }
}

fn merge_surface_maps(
    base: Option<&FxHashMap<String, BuiltinDispatchBinding>>,
    patch: &FxHashMap<String, BuiltinDispatchBinding>,
) -> FxHashMap<String, BuiltinDispatchBinding> {
    let mut merged = base.cloned().unwrap_or_default();
    for (name, binding) in patch {
        merged.insert(name.clone(), binding.clone());
    }
    merged
}

fn collect_export_names(
    statements: &[Statement],
    interner: &Interner,
    export_names: &mut FxHashSet<String>,
) {
    for stmt in statements {
        match stmt {
            Statement::ExportDecl(ExportDecl::Declaration(inner)) => match inner.as_ref() {
                Statement::ClassDecl(class_decl) => {
                    export_names.insert(interner.resolve(class_decl.name.name).to_string());
                }
                Statement::FunctionDecl(function) => {
                    export_names.insert(interner.resolve(function.name.name).to_string());
                }
                Statement::VariableDecl(var) => {
                    if let Pattern::Identifier(ident) = &var.pattern {
                        export_names.insert(interner.resolve(ident.name).to_string());
                    }
                }
                _ => {}
            },
            Statement::ExportDecl(ExportDecl::Named {
                specifiers,
                source: None,
                ..
            }) => {
                for specifier in specifiers {
                    export_names.insert(
                        specifier
                            .alias
                            .as_ref()
                            .map(|alias| interner.resolve(alias.name).to_string())
                            .unwrap_or_else(|| interner.resolve(specifier.name.name).to_string()),
                    );
                }
            }
            _ => {}
        }
    }
}

fn collect_top_level_bindings(
    statements: &[Statement],
    source: &str,
    interner: &Interner,
    constants: &FxHashMap<String, u16>,
    locals: &mut FxHashMap<String, LocalBuiltinBindingKind>,
    functions: &mut FxHashMap<String, ParsedFunctionBinding>,
) {
    for stmt in statements {
        match stmt {
            Statement::ClassDecl(class_decl) => {
                locals.insert(
                    interner.resolve(class_decl.name.name).to_string(),
                    LocalBuiltinBindingKind::Class(
                        interner.resolve(class_decl.name.name).to_string(),
                    ),
                );
            }
            Statement::FunctionDecl(function) => {
                let name = interner.resolve(function.name.name).to_string();
                let binding = direct_wrapper_binding_from_block(
                    &function.body,
                    function.return_type.as_ref(),
                    source,
                    interner,
                    constants,
                    functions,
                    NativeWrapperMode::ExplicitArgsOnly,
                );
                locals.insert(name.clone(), LocalBuiltinBindingKind::Function);
                functions.insert(
                    name,
                    ParsedFunctionBinding {
                        binding,
                        callable: true,
                    },
                );
            }
            Statement::VariableDecl(var) => {
                if let Pattern::Identifier(ident) = &var.pattern {
                    let name = interner.resolve(ident.name).to_string();
                    locals.insert(
                        name.clone(),
                        namespace_instance_kind(var.initializer.as_ref(), interner)
                            .unwrap_or(LocalBuiltinBindingKind::Value),
                    );
                }
            }
            Statement::ExportDecl(ExportDecl::Declaration(inner)) => {
                collect_top_level_bindings(
                    std::slice::from_ref(inner.as_ref()),
                    source,
                    interner,
                    constants,
                    locals,
                    functions,
                );
            }
            _ => {}
        }
    }
}

fn namespace_instance_kind(
    initializer: Option<&Expression>,
    interner: &Interner,
) -> Option<LocalBuiltinBindingKind> {
    let Expression::New(new_expr) = initializer? else {
        return None;
    };
    let Expression::Identifier(ident) = &*new_expr.callee else {
        return None;
    };
    Some(LocalBuiltinBindingKind::NamespaceInstance(
        interner.resolve(ident.name).to_string(),
    ))
}

fn collect_class_surfaces(
    statements: &[Statement],
    source: &str,
    interner: &Interner,
    constants: &FxHashMap<String, u16>,
    classes: &mut FxHashMap<String, BuiltinTypeSurface>,
) {
    for stmt in statements {
        match stmt {
            Statement::ClassDecl(class_decl) => {
                let class_name = interner.resolve(class_decl.name.name).to_string();
                let mut surface = BuiltinTypeSurface {
                    builtin_primitive: class_decl
                        .annotations
                        .iter()
                        .any(|annotation| annotation.tag == "builtin_primitive"),
                    wrapper_method_surface: class_decl
                        .annotations
                        .iter()
                        .any(|annotation| annotation.tag == "wrapper_method_surface"),
                    ..BuiltinTypeSurface::default()
                };
                let mut next_instance_field_index: u16 = 0;
                let mut next_static_field_index: u16 = 0;

                for member in &class_decl.members {
                    match member {
                        ClassMember::Field(field) => {
                            let Some(name) = property_key_name(&field.name, interner) else {
                                continue;
                            };
                            let field_index = if field.is_static {
                                let index = next_static_field_index;
                                next_static_field_index = next_static_field_index.saturating_add(1);
                                index
                            } else {
                                let index = next_instance_field_index;
                                next_instance_field_index =
                                    next_instance_field_index.saturating_add(1);
                                index
                            };
                            let binding = field
                                .annotations
                                .iter()
                                .find_map(|annotation| opcode_kind_from_annotation(annotation))
                                .map(BuiltinDispatchBinding::Opcode)
                                .unwrap_or_else(|| {
                                    BuiltinDispatchBinding::DeclaredField {
                                        field_type_name: type_name_from_annotation(
                                            field.type_annotation.as_ref(),
                                            interner,
                                        ),
                                        field_index: Some(field_index),
                                    }
                                });
                            if field.is_static {
                                surface.static_properties.insert(name, binding);
                            } else {
                                surface.instance_properties.insert(name, binding);
                            }
                        }
                        ClassMember::Method(method) => {
                            let Some(name) = property_key_name(&method.name, interner) else {
                                continue;
                            };
                            let binding = method_binding_from_decl(
                                &class_name,
                                &name,
                                method,
                                source,
                                interner,
                                constants,
                            );
                            let binding = if surface.wrapper_method_surface
                                && !method.is_static
                                && method.kind != ast::MethodKind::Getter
                            {
                                // Wrapper-surface builtins still execute as direct handle/native
                                // dispatch at runtime. Keep the body-derived binding here so
                                // methods like Channel.send/receive and Mutex.tryLock/isLocked
                                // preserve their explicit backend targets instead of degrading to
                                // a source-only method surface.
                                binding
                            } else {
                                binding
                            };
                            let target = if method.kind == ast::MethodKind::Getter {
                                if method.is_static {
                                    &mut surface.static_properties
                                } else {
                                    &mut surface.instance_properties
                                }
                            } else if method.is_static {
                                &mut surface.static_methods
                            } else {
                                &mut surface.instance_methods
                            };
                            target.insert(name, binding);
                        }
                        ClassMember::Constructor(ctor) => {
                            surface.constructor_binding = match direct_wrapper_binding_from_block(
                                &ctor.body,
                                None,
                                source,
                                interner,
                                constants,
                                &FxHashMap::default(),
                                NativeWrapperMode::ExplicitArgsOnly,
                            ) {
                                BuiltinDispatchBinding::SourceDefined => None,
                                binding => Some(binding),
                            };
                        }
                        ClassMember::StaticBlock(_) => {}
                    }
                }

                classes.insert(class_name, surface);
            }
            Statement::ExportDecl(ExportDecl::Declaration(inner)) => {
                collect_class_surfaces(
                    std::slice::from_ref(inner.as_ref()),
                    source,
                    interner,
                    constants,
                    classes,
                );
            }
            _ => {}
        }
    }
}

fn collect_static_surface_patches(
    statements: &[Statement],
    source: &str,
    interner: &Interner,
    constants: &FxHashMap<String, u16>,
    functions: &FxHashMap<String, ParsedFunctionBinding>,
    static_patches: &mut FxHashMap<String, StaticSurfacePatch>,
) {
    for stmt in statements {
        match stmt {
            Statement::Expression(expr_stmt) => {
                collect_static_surface_patch_from_expr(
                    &expr_stmt.expression,
                    source,
                    interner,
                    constants,
                    functions,
                    static_patches,
                );
            }
            Statement::ExportDecl(ExportDecl::Declaration(inner)) => {
                collect_static_surface_patches(
                    std::slice::from_ref(inner.as_ref()),
                    source,
                    interner,
                    constants,
                    functions,
                    static_patches,
                );
            }
            _ => {}
        }
    }
}

fn collect_static_surface_patch_from_expr(
    expr: &Expression,
    source: &str,
    interner: &Interner,
    constants: &FxHashMap<String, u16>,
    functions: &FxHashMap<String, ParsedFunctionBinding>,
    static_patches: &mut FxHashMap<String, StaticSurfacePatch>,
) {
    if let Some((target_name, member_name, binding, callable)) =
        object_define_property_patch(expr, source, interner, constants, functions)
    {
        let patch = static_patches.entry(target_name).or_default();
        if callable {
            patch.methods.insert(member_name, binding);
        } else {
            patch.properties.insert(member_name, binding);
        }
        return;
    }

    if let Some((target_name, member_name, binding, callable)) =
        assignment_patch(expr, source, interner, constants, functions)
    {
        let patch = static_patches.entry(target_name).or_default();
        if callable {
            patch.methods.insert(member_name, binding);
        } else {
            patch.properties.insert(member_name, binding);
        }
    }
}

fn object_define_property_patch(
    expr: &Expression,
    source: &str,
    interner: &Interner,
    constants: &FxHashMap<String, u16>,
    functions: &FxHashMap<String, ParsedFunctionBinding>,
) -> Option<(String, String, BuiltinDispatchBinding, bool)> {
    let Expression::Call(call) = expr else {
        return None;
    };
    let Expression::Member(member) = &*call.callee else {
        return None;
    };
    let Expression::Identifier(object_ident) = &*member.object else {
        return None;
    };
    if interner.resolve(object_ident.name) != "Object"
        || interner.resolve(member.property.name) != "defineProperty"
    {
        return None;
    }
    let target_name = target_identifier_name(call.argument_expression(0)?, interner)?;
    let member_name = literal_property_name(call.argument_expression(1)?, interner)?;
    let descriptor = call.argument_expression(2)?;
    let Expression::Object(object) = descriptor else {
        return None;
    };
    let value_expr = object
        .properties
        .iter()
        .find_map(|property| match property {
            ObjectProperty::Property(property)
                if property.kind == PropertyKind::Init
                    && property_key_name(&property.key, interner).as_deref() == Some("value") =>
            {
                Some(&property.value)
            }
            _ => None,
        });
    let Some(value_expr) = value_expr else {
        return Some((
            target_name,
            member_name,
            BuiltinDispatchBinding::SourceDefined,
            false,
        ));
    };
    let (binding, callable) =
        binding_from_expression(value_expr, source, interner, constants, functions);
    Some((target_name, member_name, binding, callable))
}

fn assignment_patch(
    expr: &Expression,
    source: &str,
    interner: &Interner,
    constants: &FxHashMap<String, u16>,
    functions: &FxHashMap<String, ParsedFunctionBinding>,
) -> Option<(String, String, BuiltinDispatchBinding, bool)> {
    let Expression::Assignment(assign) = expr else {
        return None;
    };
    let (target_name, member_name) = match &*assign.left {
        Expression::Member(member) => (
            target_identifier_name(&member.object, interner)?,
            interner.resolve(member.property.name).to_string(),
        ),
        Expression::Index(index) => (
            target_identifier_name(&index.object, interner)?,
            literal_property_name(&index.index, interner)?,
        ),
        _ => return None,
    };
    let (binding, callable) =
        binding_from_expression(&assign.right, source, interner, constants, functions);
    Some((target_name, member_name, binding, callable))
}

fn target_identifier_name(expr: &Expression, interner: &Interner) -> Option<String> {
    match expr {
        Expression::Identifier(ident) => Some(interner.resolve(ident.name).to_string()),
        Expression::TypeCast(cast) => target_identifier_name(&cast.object, interner),
        Expression::Parenthesized(paren) => target_identifier_name(&paren.expression, interner),
        _ => None,
    }
}

fn literal_property_name(expr: &Expression, interner: &Interner) -> Option<String> {
    match expr {
        Expression::StringLiteral(lit) => Some(interner.resolve(lit.value).to_string()),
        Expression::IntLiteral(lit) => Some(lit.value.to_string()),
        Expression::Identifier(ident) => Some(interner.resolve(ident.name).to_string()),
        Expression::Parenthesized(paren) => literal_property_name(&paren.expression, interner),
        _ => None,
    }
}

fn binding_from_expression(
    expr: &Expression,
    source: &str,
    interner: &Interner,
    constants: &FxHashMap<String, u16>,
    functions: &FxHashMap<String, ParsedFunctionBinding>,
) -> (BuiltinDispatchBinding, bool) {
    match expr {
        Expression::Identifier(ident) => functions
            .get(interner.resolve(ident.name))
            .map(|parsed| (parsed.binding.clone(), parsed.callable))
            .unwrap_or((BuiltinDispatchBinding::SourceDefined, false)),
        Expression::Function(function) => (
            function_binding(
                &function.body,
                function.return_type.as_ref(),
                source,
                interner,
                constants,
                functions,
                NativeWrapperMode::ExplicitArgsOnly,
            ),
            true,
        ),
        Expression::Arrow(arrow) => match &arrow.body {
            ast::ArrowBody::Block(block) => (
                function_binding(
                    block,
                    arrow.return_type.as_ref(),
                    source,
                    interner,
                    constants,
                    functions,
                    NativeWrapperMode::ExplicitArgsOnly,
                ),
                true,
            ),
            ast::ArrowBody::Expression(_) => (BuiltinDispatchBinding::SourceDefined, true),
        },
        _ => (BuiltinDispatchBinding::SourceDefined, false),
    }
}

fn function_binding(
    body: &ast::BlockStatement,
    return_type: Option<&TypeAnnotation>,
    source: &str,
    interner: &Interner,
    constants: &FxHashMap<String, u16>,
    functions: &FxHashMap<String, ParsedFunctionBinding>,
    wrapper_mode: NativeWrapperMode,
) -> BuiltinDispatchBinding {
    direct_wrapper_binding_from_block(
        body,
        return_type,
        source,
        interner,
        constants,
        functions,
        wrapper_mode,
    )
}

fn method_binding_from_decl(
    owner_type_name: &str,
    method_name: &str,
    method: &ast::MethodDecl,
    source: &str,
    interner: &Interner,
    constants: &FxHashMap<String, u16>,
) -> BuiltinDispatchBinding {
    let annotated_class_method = method
        .annotations
        .iter()
        .any(|annotation| annotation.tag == "class_method");

    let Some(body) = &method.body else {
        return BuiltinDispatchBinding::SourceDefined;
    };
    let wrapper_mode = if !method.is_static {
        NativeWrapperMode::AllowReceiverThis
    } else {
        NativeWrapperMode::ExplicitArgsOnly
    };
    let binding = function_binding(
        body,
        method.return_type.as_ref(),
        source,
        interner,
        constants,
        &FxHashMap::default(),
        wrapper_mode,
    );
    if !matches!(binding, BuiltinDispatchBinding::SourceDefined) {
        return binding;
    }
    if annotated_class_method {
        return BuiltinDispatchBinding::ClassMethod {
            type_name: owner_type_name.to_string(),
            method_name: method_name.to_string(),
        };
    }
    BuiltinDispatchBinding::SourceDefined
}

fn direct_wrapper_binding_from_block(
    body: &ast::BlockStatement,
    return_type: Option<&TypeAnnotation>,
    source: &str,
    interner: &Interner,
    constants: &FxHashMap<String, u16>,
    functions: &FxHashMap<String, ParsedFunctionBinding>,
    wrapper_mode: NativeWrapperMode,
) -> BuiltinDispatchBinding {
    let [stmt] = body.statements.as_slice() else {
        return BuiltinDispatchBinding::SourceDefined;
    };
    let expr = match stmt {
        Statement::Return(ret) => ret.value.as_ref(),
        Statement::Expression(expr_stmt) => Some(&expr_stmt.expression),
        _ => None,
    };
    let Some(expr) = expr else {
        return BuiltinDispatchBinding::SourceDefined;
    };
    direct_wrapper_binding_from_expression(
        expr,
        return_type,
        source,
        interner,
        constants,
        functions,
        wrapper_mode,
    )
}

fn direct_wrapper_binding_from_expression(
    expr: &Expression,
    return_type: Option<&TypeAnnotation>,
    source: &str,
    interner: &Interner,
    constants: &FxHashMap<String, u16>,
    functions: &FxHashMap<String, ParsedFunctionBinding>,
    wrapper_mode: NativeWrapperMode,
) -> BuiltinDispatchBinding {
    match expr {
        Expression::Parenthesized(paren) => direct_wrapper_binding_from_expression(
            &paren.expression,
            return_type,
            source,
            interner,
            constants,
            functions,
            wrapper_mode,
        ),
        Expression::TypeCast(cast) => direct_wrapper_binding_from_expression(
            &cast.object,
            return_type,
            source,
            interner,
            constants,
            functions,
            wrapper_mode,
        ),
        Expression::Assignment(assign) => direct_wrapper_binding_from_expression(
            &assign.right,
            return_type,
            source,
            interner,
            constants,
            functions,
            wrapper_mode,
        ),
        Expression::Call(call) => {
            let Expression::Identifier(callee) = &*call.callee else {
                return BuiltinDispatchBinding::SourceDefined;
            };
            let callee_name = interner.resolve(callee.name);
            if let Some(binding) = opcode_wrapper_binding_from_call(
                callee_name,
                call,
                return_type,
                interner,
                wrapper_mode,
            ) {
                return binding;
            }
            if callee_name == "__NATIVE_CALL" {
                return native_wrapper_binding_from_call(
                    call,
                    return_type,
                    interner,
                    constants,
                    wrapper_mode,
                )
                    .unwrap_or(BuiltinDispatchBinding::SourceDefined);
            }
            functions
                .get(callee_name)
                .map(|parsed| parsed.binding.clone())
                .unwrap_or(BuiltinDispatchBinding::SourceDefined)
        }
        Expression::Identifier(ident) => functions
            .get(interner.resolve(ident.name))
            .map(|parsed| parsed.binding.clone())
            .unwrap_or(BuiltinDispatchBinding::SourceDefined),
        _ => BuiltinDispatchBinding::SourceDefined,
    }
}

fn native_wrapper_binding_from_call(
    call: &ast::CallExpression,
    return_type: Option<&TypeAnnotation>,
    interner: &Interner,
    constants: &FxHashMap<String, u16>,
    wrapper_mode: NativeWrapperMode,
) -> Option<BuiltinDispatchBinding> {
    if !call
        .arguments
        .iter()
        .skip(1)
        .all(|arg: &ast::CallArgument| {
            native_wrapper_arg_is_passthrough(arg.expression(), wrapper_mode)
        })
    {
        return None;
    }

    let first_arg = call.argument_expression(0)?;
    native_wrapper_native_id(first_arg, interner, constants).map(|native_id| {
        BuiltinDispatchBinding::Builtin {
            op: builtin_op_from_native_id(native_id).unwrap_or(BuiltinOp::Native(native_id)),
            return_type_name: type_name_from_annotation(return_type, interner),
        }
    })
}

fn opcode_wrapper_binding_from_call(
    callee_name: &str,
    call: &ast::CallExpression,
    return_type: Option<&TypeAnnotation>,
    interner: &Interner,
    wrapper_mode: NativeWrapperMode,
) -> Option<BuiltinDispatchBinding> {
    if !call
        .arguments
        .iter()
        .all(|arg| native_wrapper_arg_is_passthrough(arg.expression(), wrapper_mode))
    {
        return None;
    }

    let op = match callee_name {
        "__OPCODE_CHANNEL_NEW" => BuiltinOp::HostHandle(crate::semantics::HostHandleOpKind::ChannelConstructor),
        "__OPCODE_MUTEX_NEW" => BuiltinOp::HostHandle(crate::semantics::HostHandleOpKind::MutexConstructor),
        "__OPCODE_MUTEX_LOCK" => BuiltinOp::HostHandle(crate::semantics::HostHandleOpKind::MutexLock),
        "__OPCODE_MUTEX_UNLOCK" => BuiltinOp::HostHandle(crate::semantics::HostHandleOpKind::MutexUnlock),
        _ => return None,
    };
    Some(BuiltinDispatchBinding::Builtin {
        op,
        return_type_name: type_name_from_annotation(return_type, interner),
    })
}

fn native_wrapper_native_id(
    expr: &Expression,
    interner: &Interner,
    constants: &FxHashMap<String, u16>,
) -> Option<u16> {
    match expr {
        Expression::IntLiteral(lit) => Some(lit.value as u16),
        Expression::Identifier(ident) => constants
            .get(interner.resolve(ident.name))
            .copied()
            .or_else(|| parse_number_literal(interner.resolve(ident.name)).map(|value| value as u16)),
        Expression::Parenthesized(paren) => {
            native_wrapper_native_id(&paren.expression, interner, constants)
        }
        Expression::TypeCast(cast) => native_wrapper_native_id(&cast.object, interner, constants),
        _ => None,
    }
}

fn native_wrapper_arg_is_passthrough(expr: &Expression, wrapper_mode: NativeWrapperMode) -> bool {
    match expr {
        Expression::Identifier(_)
        | Expression::IntLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_) => true,
        Expression::This(_) => wrapper_mode == NativeWrapperMode::AllowReceiverThis,
        Expression::Member(member) => {
            wrapper_mode == NativeWrapperMode::AllowReceiverThis
                && matches!(&*member.object, Expression::This(_))
        }
        Expression::Parenthesized(paren) => {
            native_wrapper_arg_is_passthrough(&paren.expression, wrapper_mode)
        }
        Expression::TypeCast(cast) => {
            native_wrapper_arg_is_passthrough(&cast.object, wrapper_mode)
        }
        _ => false,
    }
}

fn property_key_name(key: &PropertyKey, interner: &Interner) -> Option<String> {
    match key {
        PropertyKey::Identifier(id) => Some(interner.resolve(id.name).to_string()),
        PropertyKey::StringLiteral(lit) => Some(interner.resolve(lit.value).to_string()),
        PropertyKey::IntLiteral(lit) => Some(lit.value.to_string()),
        PropertyKey::Computed(_) => None,
    }
}

fn opcode_kind_from_annotation(annotation: &ast::Annotation) -> Option<OpcodeKind> {
    if annotation.tag != "opcode" {
        return None;
    }
    match annotation.value.as_deref() {
        Some("StringLen") => Some(OpcodeKind::StringLen),
        Some("ArrayLen") => Some(OpcodeKind::ArrayLen),
        _ => None,
    }
}

fn type_name_from_annotation(
    annotation: Option<&TypeAnnotation>,
    interner: &Interner,
) -> Option<String> {
    let annotation = annotation?;
    match &annotation.ty {
        AstType::Primitive(primitive) => Some(primitive.name().to_string()),
        AstType::Reference(reference) => Some(interner.resolve(reference.name.name).to_string()),
        AstType::Array(_) | AstType::Tuple(_) => Some("Array".to_string()),
        AstType::Parenthesized(inner) => type_name_from_annotation(Some(inner.as_ref()), interner),
        AstType::Union(union) => {
            let mut candidate = None;
            for ty in &union.types {
                let Some(name) = type_name_from_annotation(Some(ty), interner) else {
                    continue;
                };
                if name == "null" || name == "void" {
                    continue;
                }
                match &candidate {
                    Some(existing) if existing != &name => return None,
                    None => candidate = Some(name),
                    _ => {}
                }
            }
            candidate
        }
        _ => None,
    }
}

fn resolve_type_name(type_ctx: &TypeContext, name: &str) -> Option<u32> {
    match name {
        "Array" => Some(TypeContext::ARRAY_TYPE_ID),
        _ => type_ctx
            .lookup_named_type(name)
            .map(|type_id| type_id.as_u32()),
    }
}

fn span_slice(source: &str, start: usize, end: usize) -> &str {
    source.get(start..end).unwrap_or("")
}

fn extract_constants(source: &str) -> FxHashMap<String, u16> {
    let mut constants = FxHashMap::default();
    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("const ") {
            if let Some(colon_idx) = rest.find(':') {
                let name = rest[..colon_idx].trim();
                if let Some(eq_idx) = rest.find('=') {
                    let value_str = rest[eq_idx + 1..].trim().trim_end_matches(';').trim();
                    if let Some(value) = parse_number_literal(value_str) {
                        constants.insert(name.to_string(), value as u16);
                    }
                }
            }
        }
    }
    constants
}

fn parse_number_literal(s: &str) -> Option<u64> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else {
        s.parse::<u64>().ok()
    }
}

fn extract_native_call_id(body: &str, constants: &FxHashMap<String, u16>) -> Option<u16> {
    let marker = "__NATIVE_CALL";
    let start = body.find(marker)?;
    let after_marker = &body[start + marker.len()..];
    let after_generic = if let Some(lt_idx) = after_marker.find('<') {
        let gt_idx = after_marker[lt_idx + 1..].find('>')?;
        &after_marker[lt_idx + gt_idx + 2..]
    } else {
        after_marker
    };
    let open_paren = after_generic.find('(')?;
    let after_paren = &after_generic[open_paren + 1..];
    let arg_end = after_paren
        .find(',')
        .or_else(|| after_paren.find(')'))
        .unwrap_or(after_paren.len());
    let native_token = after_paren[..arg_end].trim();
    constants
        .get(native_token)
        .copied()
        .or_else(|| parse_number_literal(native_token).map(|value| value as u16))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strict_builtin_manifest() -> &'static BuiltinSurfaceManifest {
        builtin_surface_manifest_for_mode(BuiltinSurfaceMode::RayaStrict)
    }

    #[test]
    fn test_registry_overlay_exposes_date_static_methods() {
        let manifest = strict_builtin_manifest();
        let type_ctx = TypeContext::new();

        assert!(matches!(
            manifest.lookup_static_method("Date", "now", &type_ctx),
            Some(DispatchAction::Builtin(_))
        ));
        assert!(matches!(
            manifest.lookup_static_method("Date", "parse", &type_ctx),
            Some(DispatchAction::Builtin(_))
        ));
        assert!(matches!(
            manifest.lookup_static_method("Number", "isNaN", &type_ctx),
            Some(DispatchAction::Builtin(_))
        ));
        assert!(matches!(
            manifest.lookup_static_method("Number", "isFinite", &type_ctx),
            Some(DispatchAction::Builtin(_))
        ));
        assert!(matches!(
            manifest.lookup_static_method("String", "fromCharCode", &type_ctx),
            Some(DispatchAction::Builtin(_))
        ));
        assert!(matches!(
            manifest.lookup_static_method("JSON", "parse", &type_ctx),
            Some(DispatchAction::Builtin(_))
        ));
        assert!(matches!(
            manifest.lookup_static_method("JSON", "stringify", &type_ctx),
            Some(DispatchAction::Builtin(_))
        ));
        assert!(matches!(
            manifest.lookup_static_method("Object", "is", &type_ctx),
            Some(DispatchAction::Builtin(_))
        ));
        assert!(matches!(
            manifest.lookup_static_method("Object", "getPrototypeOf", &type_ctx),
            Some(DispatchAction::Builtin(_))
        ));
        assert!(matches!(
            manifest.lookup_static_method("Object", "defineProperty", &type_ctx),
            Some(DispatchAction::Builtin(_))
        ));
        assert!(matches!(
            manifest.lookup_static_method("Math", "abs", &type_ctx),
            Some(DispatchAction::Builtin(_))
        ));
        assert!(matches!(
            manifest.lookup_static_method("Math", "random", &type_ctx),
            Some(DispatchAction::Builtin(_))
        ));
        assert!(matches!(
            manifest.lookup_static_method("Promise", "resolve", &type_ctx),
            Some(DispatchAction::Builtin(_))
        ));
        assert!(matches!(
            manifest.lookup_static_method("Promise", "all", &type_ctx),
            Some(DispatchAction::Builtin(_))
        ));
    }
}
