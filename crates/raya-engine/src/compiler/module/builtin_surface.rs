use std::sync::OnceLock;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::compiler::module::BuiltinSurfaceMode;
use crate::compiler::type_registry::{DispatchAction, OpcodeKind};
use crate::parser::ast::{
    self, ClassMember, ExportDecl, Expression, ObjectProperty, Pattern, PropertyKind, PropertyKey,
    Statement, Type as AstType, TypeAnnotation,
};
use crate::parser::checker::SymbolKind;
use crate::parser::{Interner, Parser, TypeContext};
use crate::semantics::{SemanticProfile, SourceKind};

use super::builtin_contract::builtin_source_modules_for_mode;

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
    &[
        ("string", include_str!("../../../builtins/strict/string.raya")),
        ("number", include_str!("../../../builtins/strict/number.raya")),
        ("Array", include_str!("../../../builtins/strict/array.raya")),
        ("RegExp", include_str!("../../../builtins/strict/regexp.raya")),
        ("Set", include_str!("../../../builtins/strict/set.raya")),
        (
            "Promise",
            include_str!("../../../builtins/strict/promise.raya"),
        ),
    ]
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
    pub(crate) constructor_native_id: Option<u16>,
    pub(crate) instance_methods: FxHashMap<String, BuiltinDispatchBinding>,
    pub(crate) instance_properties: FxHashMap<String, BuiltinDispatchBinding>,
    pub(crate) static_methods: FxHashMap<String, BuiltinDispatchBinding>,
    pub(crate) static_properties: FxHashMap<String, BuiltinDispatchBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BuiltinDispatchBinding {
    SourceDefined,
    Opcode(OpcodeKind),
    VmNative {
        native_id: u16,
        return_type_name: Option<String>,
    },
    DeclaredField(Option<String>),
    ClassMethod {
        type_name: String,
        method_name: String,
    },
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
            .and_then(|surface| surface.constructor_native_id)
    }
}

impl BuiltinDispatchBinding {
    pub(crate) fn to_dispatch_action(&self, type_ctx: &TypeContext) -> Option<DispatchAction> {
        match self {
            Self::SourceDefined => None,
            Self::Opcode(kind) => Some(DispatchAction::Opcode(*kind)),
            Self::VmNative { native_id, .. } => Some(DispatchAction::VmNative(*native_id)),
            Self::DeclaredField(field_type_name) => Some(DispatchAction::DeclaredField(
                field_type_name
                    .as_deref()
                    .and_then(|name| resolve_type_name(type_ctx, name)),
            )),
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
            Self::VmNative {
                return_type_name: Some(name),
                ..
            } => Some(name.as_str()),
            _ => None,
        }
    }
}

fn build_builtin_surface_manifest(mode: BuiltinSurfaceMode) -> BuiltinSurfaceManifest {
    let mut classes = FxHashMap::<String, BuiltinTypeSurface>::default();
    let mut locals = FxHashMap::<String, LocalBuiltinBindingKind>::default();
    let mut static_patches = FxHashMap::<String, StaticSurfacePatch>::default();
    let mut exported_names = FxHashSet::<String>::default();

    let mut source_modules = Vec::new();
    source_modules.extend(builtin_class_method_sources().iter().copied());
    source_modules.extend(builtin_source_modules_for_mode(mode).iter().copied());

    for (_logical_path, source) in source_modules {
        let parser = Parser::new(source)
            .unwrap_or_else(|errors| panic!("failed to lex builtin surface source: {errors:?}"));
        let (module, interner) = parser
            .parse()
            .unwrap_or_else(|errors| panic!("failed to parse builtin surface source: {errors:?}"));
        let constants = extract_constants(source);
        let mut functions = FxHashMap::<String, ParsedFunctionBinding>::default();

        collect_top_level_bindings(&module.statements, &interner, &mut locals, &mut functions);
        collect_export_names(&module.statements, &interner, &mut exported_names);
        collect_class_surfaces(&module.statements, source, &interner, &constants, &mut classes);
        collect_static_surface_patches(
            &module.statements,
            source,
            &interner,
            &constants,
            &functions,
            &mut static_patches,
        );
    }

    let mut globals = FxHashMap::default();
    let mut builtin_global_names = FxHashSet::default();
    let mut builtin_namespace_names = FxHashSet::default();

    for exported_name in &exported_names {
        builtin_global_names.insert(exported_name.clone());
        let local_name = exported_name.as_str();
        let patch = static_patches.remove(local_name).unwrap_or_default();
        let global_surface = match locals.get(local_name) {
            Some(LocalBuiltinBindingKind::Class(type_name)) => {
                let type_surface = classes.get(type_name);
                BuiltinGlobalSurface {
                    kind: BuiltinGlobalKind::ClassValue,
                    backing_type_name: Some(type_name.clone()),
                    static_methods: merge_surface_maps(
                        type_surface.map(|surface| &surface.static_methods),
                        &patch.methods,
                    ),
                    static_properties: merge_surface_maps(
                        type_surface.map(|surface| &surface.static_properties),
                        &patch.properties,
                    ),
                }
            }
            Some(LocalBuiltinBindingKind::NamespaceInstance(type_name)) => {
                builtin_namespace_names.insert(exported_name.clone());
                let type_surface = classes.get(type_name);
                BuiltinGlobalSurface {
                    kind: BuiltinGlobalKind::Namespace,
                    backing_type_name: Some(type_name.clone()),
                    static_methods: merge_surface_maps(
                        type_surface.map(|surface| &surface.instance_methods),
                        &patch.methods,
                    ),
                    static_properties: merge_surface_maps(
                        type_surface.map(|surface| &surface.instance_properties),
                        &patch.properties,
                    ),
                }
            }
            Some(LocalBuiltinBindingKind::Function) if !patch.methods.is_empty() || !patch.properties.is_empty() => {
                BuiltinGlobalSurface {
                    kind: BuiltinGlobalKind::StaticValue,
                    backing_type_name: None,
                    static_methods: patch.methods,
                    static_properties: patch.properties,
                }
            }
            Some(LocalBuiltinBindingKind::Function) => BuiltinGlobalSurface {
                kind: BuiltinGlobalKind::Value,
                backing_type_name: None,
                static_methods: FxHashMap::default(),
                static_properties: FxHashMap::default(),
            },
            _ if !patch.methods.is_empty() || !patch.properties.is_empty() => BuiltinGlobalSurface {
                kind: BuiltinGlobalKind::StaticValue,
                backing_type_name: None,
                static_methods: patch.methods,
                static_properties: patch.properties,
            },
            _ => BuiltinGlobalSurface {
                kind: BuiltinGlobalKind::Value,
                backing_type_name: None,
                static_methods: FxHashMap::default(),
                static_properties: FxHashMap::default(),
            },
        };
        globals.insert(exported_name.clone(), global_surface);
    }

    BuiltinSurfaceManifest {
        mode,
        globals,
        types: classes,
        builtin_global_names,
        builtin_namespace_names,
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
    interner: &Interner,
    locals: &mut FxHashMap<String, LocalBuiltinBindingKind>,
    functions: &mut FxHashMap<String, ParsedFunctionBinding>,
) {
    for stmt in statements {
        match stmt {
            Statement::ClassDecl(class_decl) => {
                locals.insert(
                    interner.resolve(class_decl.name.name).to_string(),
                    LocalBuiltinBindingKind::Class(interner.resolve(class_decl.name.name).to_string()),
                );
            }
            Statement::FunctionDecl(function) => {
                let name = interner.resolve(function.name.name).to_string();
                locals.insert(name.clone(), LocalBuiltinBindingKind::Function);
                functions.insert(
                    name,
                    ParsedFunctionBinding {
                        binding: BuiltinDispatchBinding::SourceDefined,
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
                collect_top_level_bindings(std::slice::from_ref(inner.as_ref()), interner, locals, functions);
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

                for member in &class_decl.members {
                    match member {
                        ClassMember::Field(field) => {
                            let Some(name) = property_key_name(&field.name, interner) else {
                                continue;
                            };
                            let binding = field
                                .annotations
                                .iter()
                                .find_map(|annotation| opcode_kind_from_annotation(annotation))
                                .map(BuiltinDispatchBinding::Opcode)
                                .unwrap_or_else(|| {
                                    BuiltinDispatchBinding::DeclaredField(
                                        type_name_from_annotation(field.type_annotation.as_ref(), interner),
                                    )
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
                            let body = span_slice(source, ctor.body.span.start, ctor.body.span.end);
                            surface.constructor_native_id =
                                extract_native_call_id(body, constants);
                        }
                        ClassMember::StaticBlock(_) => {}
                    }
                }

                classes.insert(class_name, surface);
            }
            Statement::ExportDecl(ExportDecl::Declaration(inner)) => {
                collect_class_surfaces(std::slice::from_ref(inner.as_ref()), source, interner, constants, classes);
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
    let value_expr = object.properties.iter().find_map(|property| match property {
        ObjectProperty::Property(property)
            if property.kind == PropertyKind::Init
                && property_key_name(&property.key, interner).as_deref() == Some("value") =>
        {
            Some(&property.value)
        }
        _ => None,
    });
    let Some(value_expr) = value_expr else {
        return Some((target_name, member_name, BuiltinDispatchBinding::SourceDefined, false));
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
            function_binding(function.body.span.start, function.body.span.end, function.return_type.as_ref(), source, interner, constants),
            true,
        ),
        Expression::Arrow(arrow) => match &arrow.body {
            ast::ArrowBody::Block(block) => (
                function_binding(
                    block.span.start,
                    block.span.end,
                    arrow.return_type.as_ref(),
                    source,
                    interner,
                    constants,
                ),
                true,
            ),
            ast::ArrowBody::Expression(_) => (BuiltinDispatchBinding::SourceDefined, true),
        },
        _ => (BuiltinDispatchBinding::SourceDefined, false),
    }
}

fn function_binding(
    span_start: usize,
    span_end: usize,
    return_type: Option<&TypeAnnotation>,
    source: &str,
    interner: &Interner,
    constants: &FxHashMap<String, u16>,
) -> BuiltinDispatchBinding {
    let body = span_slice(source, span_start, span_end);
    extract_native_call_id(body, constants)
        .map(|native_id| BuiltinDispatchBinding::VmNative {
            native_id,
            return_type_name: type_name_from_annotation(return_type, interner),
        })
        .unwrap_or(BuiltinDispatchBinding::SourceDefined)
}

fn method_binding_from_decl(
    owner_type_name: &str,
    method_name: &str,
    method: &ast::MethodDecl,
    source: &str,
    interner: &Interner,
    constants: &FxHashMap<String, u16>,
) -> BuiltinDispatchBinding {
    if method
        .annotations
        .iter()
        .any(|annotation| annotation.tag == "class_method")
    {
        return BuiltinDispatchBinding::ClassMethod {
            type_name: owner_type_name.to_string(),
            method_name: method_name.to_string(),
        };
    }

    let Some(body) = &method.body else {
        return BuiltinDispatchBinding::SourceDefined;
    };
    function_binding(
        body.span.start,
        body.span.end,
        method.return_type.as_ref(),
        source,
        interner,
        constants,
    )
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
        _ => type_ctx.lookup_named_type(name).map(|type_id| type_id.as_u32()),
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
