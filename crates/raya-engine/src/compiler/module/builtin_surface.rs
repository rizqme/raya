use std::sync::OnceLock;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::compiler::builtins::{
    native_id_for_builtin_op, BuiltinOp, BuiltinRegistry, BuiltinSurfaceMemberDescriptor,
};
use crate::compiler::module::BuiltinSurfaceMode;
use crate::compiler::type_registry::{DispatchAction, OpcodeKind};
use crate::parser::TypeContext;
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
    SurfaceOnly,
    Opcode(OpcodeKind),
    Builtin {
        op: BuiltinOp,
        return_type_name: Option<String>,
    },
    DeclaredField {
        field_type_name: Option<String>,
        field_index: Option<u16>,
    },
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
            Self::SurfaceOnly => None,
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
    let mut builtin_global_names = FxHashSet::default();

    if matches!(mode, BuiltinSurfaceMode::NodeCompat) {
        builtin_global_names.insert("globalThis".to_string());
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

    let builtin_namespace_names = globals
        .iter()
        .filter_map(|(name, surface)| {
            (surface.kind == BuiltinGlobalKind::Namespace).then(|| name.clone())
        })
        .collect();

    BuiltinSurfaceManifest {
        mode,
        globals,
        types: classes,
        builtin_global_names,
        builtin_namespace_names,
    }
}

fn builtin_global_kind_for_name(
    name: &str,
    has_constructor: bool,
    has_static_surface: bool,
) -> BuiltinGlobalKind {
    if matches!(
        name,
        "JSON" | "Math" | "Reflect" | "Temporal" | "Atomics" | "Intl"
    ) {
        return BuiltinGlobalKind::Namespace;
    }
    if has_constructor {
        BuiltinGlobalKind::ClassValue
    } else if has_static_surface {
        BuiltinGlobalKind::StaticValue
    } else {
        BuiltinGlobalKind::Value
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
    let _ = (type_name, member_name, mode);
    true
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
        let mut registry_static_properties = FxHashMap::default();
        let kind = match descriptor.symbol_type {
            crate::compiler::SymbolType::Function => BuiltinGlobalKind::Value,
            _ => match global_name {
                "JSON" | "Math" | "Reflect" | "Atomics" | "Intl" | "Temporal" => {
                    BuiltinGlobalKind::Namespace
                }
                _ => BuiltinGlobalKind::ClassValue,
            },
        };
        if let Some(type_descriptor) =
            BuiltinRegistry::shared().type_descriptor(descriptor.backing_type_name)
        {
            for (method_name, binding) in &type_descriptor.static_methods {
                if !builtin_static_member_visible_in_mode(global_name, method_name, mode) {
                    continue;
                }
                registry_static_methods
                    .insert((*method_name).to_string(), member_binding_to_dispatch(binding));
            }
            for (property_name, binding) in &type_descriptor.static_properties {
                if !builtin_static_member_visible_in_mode(global_name, property_name, mode) {
                    continue;
                }
                registry_static_properties.insert(
                    (*property_name).to_string(),
                    member_binding_to_dispatch(binding),
                );
            }
        }
        let entry = globals
            .entry(global_name.to_string())
            .or_insert_with(|| BuiltinGlobalSurface {
                kind,
                backing_type_name: Some(descriptor.backing_type_name.to_string()),
                static_methods: FxHashMap::default(),
                static_properties: FxHashMap::default(),
            });
        if entry.backing_type_name.is_none() {
            entry.backing_type_name = Some(descriptor.backing_type_name.to_string());
        }
        entry.kind = kind;
        for (method_name, binding) in registry_static_methods {
            entry.static_methods.insert(method_name, binding);
        }
        for (property_name, binding) in registry_static_properties {
            entry.static_properties.insert(property_name, binding);
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
            surface.constructor_binding = Some(member_binding_to_dispatch(&constructor));
        }
        for (method_name, binding) in &descriptor.instance_methods {
            surface.instance_methods.insert(
                (*method_name).to_string(),
                member_binding_to_dispatch(binding),
            );
        }
        for (method_name, binding) in &descriptor.static_methods {
            if !builtin_static_member_visible_in_mode(type_name, method_name, mode) {
                continue;
            }
            surface.static_methods.insert(
                (*method_name).to_string(),
                member_binding_to_dispatch(binding),
            );
        }
        for (property_name, binding) in &descriptor.instance_properties {
            surface.instance_properties.insert(
                (*property_name).to_string(),
                member_binding_to_dispatch(binding),
            );
        }
        for (property_name, binding) in &descriptor.static_properties {
            if !builtin_static_member_visible_in_mode(type_name, property_name, mode) {
                continue;
            }
            surface.static_properties.insert(
                (*property_name).to_string(),
                member_binding_to_dispatch(binding),
            );
        }
    }
}

fn member_binding_to_dispatch(binding: &BuiltinSurfaceMemberDescriptor) -> BuiltinDispatchBinding {
    match binding {
        BuiltinSurfaceMemberDescriptor::SurfaceOnly => BuiltinDispatchBinding::SurfaceOnly,
        BuiltinSurfaceMemberDescriptor::Opcode(kind) => BuiltinDispatchBinding::Opcode(*kind),
        BuiltinSurfaceMemberDescriptor::Bound(binding) => BuiltinDispatchBinding::Builtin {
            op: binding.op,
            return_type_name: binding.return_type_name.map(str::to_string),
        },
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
