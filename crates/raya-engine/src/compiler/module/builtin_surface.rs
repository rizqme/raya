use std::sync::OnceLock;

use rustc_hash::FxHashMap;

use crate::compiler::builtins::{
    builtin_native_alias_id, BuiltinOpId, BuiltinRegistry, BuiltinSurfaceMemberDescriptor,
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
    cache.get_or_init(|| BuiltinSurfaceManifest { mode })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuiltinGlobalKind {
    Namespace,
    ClassValue,
    StaticValue,
    Value,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct BuiltinSurfaceManifest {
    pub(crate) mode: BuiltinSurfaceMode,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuiltinDispatchBinding {
    SurfaceOnly,
    Opcode(OpcodeKind),
    Builtin {
        op: BuiltinOpId,
        return_type_name: Option<&'static str>,
    },
    DeclaredField {
        field_type_name: Option<&'static str>,
        field_index: Option<u16>,
    },
}

impl BuiltinSurfaceManifest {
    pub(crate) fn global_surface(&self, name: &str) -> Option<BuiltinGlobalSurface> {
        if name == "globalThis" && matches!(self.mode, BuiltinSurfaceMode::NodeCompat) {
            return Some(BuiltinGlobalSurface {
                kind: BuiltinGlobalKind::Value,
                backing_type_name: None,
                static_methods: FxHashMap::default(),
                static_properties: FxHashMap::default(),
            });
        }

        if !builtin_root_visible_in_mode(name, self.mode) {
            return None;
        }

        let descriptor = BuiltinRegistry::shared().global_descriptor(name)?;
        let mut static_methods = FxHashMap::default();
        let mut static_properties = FxHashMap::default();

        if let Some(type_descriptor) =
            BuiltinRegistry::shared().type_descriptor(descriptor.backing_type_name)
        {
            for (member_name, binding) in &type_descriptor.static_methods {
                if builtin_static_member_visible_in_mode(name, member_name, self.mode) {
                    static_methods.insert((*member_name).to_string(), member_binding_to_dispatch(*binding));
                }
            }
            for (property_name, binding) in &type_descriptor.static_properties {
                if builtin_static_member_visible_in_mode(name, property_name, self.mode) {
                    static_properties
                        .insert((*property_name).to_string(), member_binding_to_dispatch(*binding));
                }
            }
        }

        let has_constructor = BuiltinRegistry::shared()
            .type_descriptor(descriptor.backing_type_name)
            .is_some_and(|surface| surface.constructor.is_some());
        let has_static_surface = !static_methods.is_empty() || !static_properties.is_empty();

        Some(BuiltinGlobalSurface {
            kind: builtin_global_kind_for_name(name, has_constructor, has_static_surface),
            backing_type_name: Some(descriptor.backing_type_name.to_string()),
            static_methods,
            static_properties,
        })
    }

    pub(crate) fn type_surface(&self, name: &str) -> Option<BuiltinTypeSurface> {
        if !builtin_root_visible_in_mode(name, self.mode) {
            return None;
        }

        let descriptor = BuiltinRegistry::shared().type_descriptor(name)?;
        let mut surface = BuiltinTypeSurface {
            builtin_primitive: descriptor.builtin_primitive,
            wrapper_method_surface: descriptor.wrapper_method_surface,
            constructor_binding: descriptor.constructor.map(member_binding_to_dispatch),
            instance_methods: FxHashMap::default(),
            instance_properties: FxHashMap::default(),
            static_methods: FxHashMap::default(),
            static_properties: FxHashMap::default(),
        };

        for (method_name, binding) in &descriptor.instance_methods {
            surface.instance_methods.insert(
                (*method_name).to_string(),
                member_binding_to_dispatch(*binding),
            );
        }
        for (property_name, binding) in &descriptor.instance_properties {
            surface.instance_properties.insert(
                (*property_name).to_string(),
                member_binding_to_dispatch(*binding),
            );
        }
        for (method_name, binding) in &descriptor.static_methods {
            if builtin_static_member_visible_in_mode(name, method_name, self.mode) {
                surface.static_methods.insert(
                    (*method_name).to_string(),
                    member_binding_to_dispatch(*binding),
                );
            }
        }
        for (property_name, binding) in &descriptor.static_properties {
            if builtin_static_member_visible_in_mode(name, property_name, self.mode) {
                surface.static_properties.insert(
                    (*property_name).to_string(),
                    member_binding_to_dispatch(*binding),
                );
            }
        }

        Some(surface)
    }

    pub(crate) fn is_builtin_global(&self, name: &str) -> bool {
        self.global_surface(name).is_some()
    }

    pub(crate) fn is_namespace_global(&self, name: &str) -> bool {
        self.global_kind(name) == Some(BuiltinGlobalKind::Namespace)
    }

    pub(crate) fn global_kind(&self, name: &str) -> Option<BuiltinGlobalKind> {
        self.global_surface(name).map(|surface| surface.kind)
    }

    pub(crate) fn backing_type_name(&self, global_name: &str) -> Option<&str> {
        if global_name == "globalThis" && matches!(self.mode, BuiltinSurfaceMode::NodeCompat) {
            return None;
        }
        if !builtin_root_visible_in_mode(global_name, self.mode) {
            return None;
        }
        BuiltinRegistry::shared()
            .global_descriptor(global_name)
            .map(|descriptor| descriptor.backing_type_name)
    }

    pub(crate) fn global_uses_static_surface(&self, name: &str) -> bool {
        self.global_kind(name).is_some_and(|kind| {
            matches!(
                kind,
                BuiltinGlobalKind::Namespace
                    | BuiltinGlobalKind::ClassValue
                    | BuiltinGlobalKind::StaticValue
            )
        })
    }

    pub(crate) fn has_dispatch_type(&self, type_name: &str) -> bool {
        self.type_surface(type_name).is_some()
    }

    pub(crate) fn static_method_binding(
        &self,
        global_name: &str,
        member_name: &str,
    ) -> Option<BuiltinDispatchBinding> {
        self.global_surface(global_name)
            .and_then(|surface| surface.static_methods.get(member_name).copied())
    }

    pub(crate) fn static_property_binding(
        &self,
        global_name: &str,
        property_name: &str,
    ) -> Option<BuiltinDispatchBinding> {
        self.global_surface(global_name)
            .and_then(|surface| surface.static_properties.get(property_name).copied())
    }

    pub(crate) fn instance_method_binding(
        &self,
        type_name: &str,
        member_name: &str,
    ) -> Option<BuiltinDispatchBinding> {
        self.type_surface(type_name)
            .and_then(|surface| surface.instance_methods.get(member_name).copied())
    }

    pub(crate) fn instance_property_binding(
        &self,
        type_name: &str,
        property_name: &str,
    ) -> Option<BuiltinDispatchBinding> {
        self.type_surface(type_name)
            .and_then(|surface| surface.instance_properties.get(property_name).copied())
    }

    pub(crate) fn has_static_method(&self, global_name: &str, member_name: &str) -> bool {
        self.static_method_binding(global_name, member_name).is_some()
    }

    pub(crate) fn has_static_property(&self, global_name: &str, property_name: &str) -> bool {
        self.static_property_binding(global_name, property_name)
            .is_some()
    }

    pub(crate) fn lookup_static_method(
        &self,
        global_name: &str,
        member_name: &str,
        type_ctx: &TypeContext,
    ) -> Option<DispatchAction> {
        self.static_method_binding(global_name, member_name)
            .and_then(|binding| binding.to_dispatch_action(type_ctx))
    }

    pub(crate) fn lookup_static_property(
        &self,
        global_name: &str,
        property_name: &str,
        type_ctx: &TypeContext,
    ) -> Option<DispatchAction> {
        self.static_property_binding(global_name, property_name)
            .and_then(|binding| binding.to_dispatch_action(type_ctx))
    }

    pub(crate) fn lookup_instance_method(
        &self,
        type_name: &str,
        member_name: &str,
        type_ctx: &TypeContext,
    ) -> Option<DispatchAction> {
        self.instance_method_binding(type_name, member_name)
            .and_then(|binding| binding.to_dispatch_action(type_ctx))
    }

    pub(crate) fn lookup_instance_property(
        &self,
        type_name: &str,
        property_name: &str,
        type_ctx: &TypeContext,
    ) -> Option<DispatchAction> {
        self.instance_property_binding(type_name, property_name)
            .and_then(|binding| binding.to_dispatch_action(type_ctx))
    }

    pub(crate) fn constructor_native_id(&self, type_name: &str) -> Option<u16> {
        self.constructor_binding(type_name)
            .and_then(|binding| binding.native_id())
    }

    pub(crate) fn constructor_binding(&self, type_name: &str) -> Option<BuiltinDispatchBinding> {
        self.type_surface(type_name)
            .and_then(|surface| surface.constructor_binding)
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
            } => Some(*name),
            _ => None,
        }
    }

    pub(crate) fn native_id(&self) -> Option<u16> {
        match self {
            Self::Builtin { op, .. } => builtin_native_alias_id(*op),
            _ => None,
        }
    }

    pub(crate) fn builtin_op(&self) -> Option<BuiltinOpId> {
        match self {
            Self::Builtin { op, .. } => Some(*op),
            _ => None,
        }
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

pub(crate) fn builtin_root_visible_in_mode(name: &str, mode: BuiltinSurfaceMode) -> bool {
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

pub(crate) fn builtin_static_member_visible_in_mode(
    type_name: &str,
    member_name: &str,
    mode: BuiltinSurfaceMode,
) -> bool {
    let _ = (type_name, member_name, mode);
    true
}

fn member_binding_to_dispatch(binding: BuiltinSurfaceMemberDescriptor) -> BuiltinDispatchBinding {
    match binding {
        BuiltinSurfaceMemberDescriptor::SurfaceOnly => BuiltinDispatchBinding::SurfaceOnly,
        BuiltinSurfaceMemberDescriptor::Opcode(kind) => BuiltinDispatchBinding::Opcode(kind),
        BuiltinSurfaceMemberDescriptor::Bound(binding) => BuiltinDispatchBinding::Builtin {
            op: binding.op_id(),
            return_type_name: binding.return_type_name(),
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
