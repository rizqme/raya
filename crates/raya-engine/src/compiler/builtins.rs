use std::sync::OnceLock;

use rustc_hash::FxHashMap;

use crate::compiler::SymbolType;
use crate::semantics::{HostHandleOpKind, IteratorOpKind, JsOpKind, MetaobjectOpKind};
use crate::vm::{builtin, builtins};

pub type BuiltinOpId = u16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinOp {
    Native(u16),
    Metaobject(MetaobjectOpKind),
    Iterator(IteratorOpKind),
    HostHandle(HostHandleOpKind),
    Js(JsOpKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinSurfaceKind {
    Constructor,
    InstanceMethod,
    StaticMethod,
    PropertyGet,
    PropertySet,
    NamespaceCall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HandleKind {
    Mutex,
    Channel,
    Task,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinReceiverModel {
    None,
    Value,
    Object,
    Handle(HandleKind),
    TaskHandle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinExecutionKind {
    PureOpcode,
    RuntimeBuiltin,
    ResumableRuntimeBuiltin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BuiltinBindingDescriptor {
    pub op: BuiltinOp,
    pub return_type_name: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinGlobalDescriptor {
    pub global_name: &'static str,
    pub backing_type_name: &'static str,
    pub symbol_type: SymbolType,
}

#[derive(Debug, Clone, Default)]
pub struct BuiltinTypeDescriptor {
    pub builtin_primitive: bool,
    pub wrapper_method_surface: bool,
    pub constructor: Option<BuiltinBindingDescriptor>,
    pub instance_methods: FxHashMap<&'static str, BuiltinBindingDescriptor>,
    pub instance_properties: FxHashMap<&'static str, BuiltinBindingDescriptor>,
    pub static_methods: FxHashMap<&'static str, BuiltinBindingDescriptor>,
}

#[derive(Debug, Default)]
pub struct BuiltinRegistry {
    globals: FxHashMap<&'static str, BuiltinGlobalDescriptor>,
    types: FxHashMap<&'static str, BuiltinTypeDescriptor>,
}

static BUILTIN_REGISTRY: OnceLock<BuiltinRegistry> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BuiltinDescriptor {
    pub op: BuiltinOp,
    pub name: &'static str,
    pub surface: BuiltinSurfaceKind,
    pub receiver: BuiltinReceiverModel,
    pub execution: BuiltinExecutionKind,
}

const METAOBJECT_COUNT: BuiltinOpId = 13;
const ITERATOR_COUNT: BuiltinOpId = 12;
const HOST_HANDLE_COUNT: BuiltinOpId = 17;
const JS_COUNT: BuiltinOpId = 30;

const METAOBJECT_BASE: BuiltinOpId = 0;
const ITERATOR_BASE: BuiltinOpId = METAOBJECT_BASE + METAOBJECT_COUNT;
const HOST_HANDLE_BASE: BuiltinOpId = ITERATOR_BASE + ITERATOR_COUNT;
const JS_BASE: BuiltinOpId = HOST_HANDLE_BASE + HOST_HANDLE_COUNT;
const NATIVE_BASE: BuiltinOpId = JS_BASE + JS_COUNT;

impl From<MetaobjectOpKind> for BuiltinOp {
    fn from(value: MetaobjectOpKind) -> Self {
        Self::Metaobject(value)
    }
}

impl From<IteratorOpKind> for BuiltinOp {
    fn from(value: IteratorOpKind) -> Self {
        Self::Iterator(value)
    }
}

impl From<HostHandleOpKind> for BuiltinOp {
    fn from(value: HostHandleOpKind) -> Self {
        Self::HostHandle(value)
    }
}

impl From<JsOpKind> for BuiltinOp {
    fn from(value: JsOpKind) -> Self {
        Self::Js(value)
    }
}

impl BuiltinRegistry {
    pub fn shared() -> &'static Self {
        BUILTIN_REGISTRY.get_or_init(build_builtin_registry)
    }

    pub fn type_descriptor(&self, type_name: &str) -> Option<&BuiltinTypeDescriptor> {
        self.types.get(canonical_runtime_type_name(type_name))
    }

    pub fn has_dispatch_type(&self, type_name: &str) -> bool {
        self.type_descriptor(type_name).is_some()
    }

    pub fn type_descriptors(
        &self,
    ) -> impl Iterator<Item = (&'static str, &BuiltinTypeDescriptor)> + '_ {
        self.types.iter().map(|(name, descriptor)| (*name, descriptor))
    }

    pub fn global_descriptor(&self, global_name: &str) -> Option<&BuiltinGlobalDescriptor> {
        self.globals.get(global_name)
    }

    pub fn global_descriptors(
        &self,
    ) -> impl Iterator<Item = (&'static str, &BuiltinGlobalDescriptor)> + '_ {
        self.globals
            .iter()
            .map(|(name, descriptor)| (*name, descriptor))
    }
}

fn build_builtin_registry() -> BuiltinRegistry {
    let mut registry = BuiltinRegistry::default();

    register_exported_class(&mut registry, "Array", "Array");
    register_runtime_type(
        &mut registry,
        "Array",
        false,
        Some(BuiltinOp::Native(builtin::array::NEW)),
        &[
            "push",
            "pop",
            "shift",
            "unshift",
            "indexOf",
            "includes",
            "slice",
            "splice",
            "concat",
            "reverse",
            "join",
            "forEach",
            "filter",
            "find",
            "findIndex",
            "every",
            "some",
            "lastIndexOf",
            "sort",
            "map",
            "reduce",
            "fill",
            "flat",
        ],
    );
    register_exported_class(&mut registry, "String", "string");
    register_runtime_type(
        &mut registry,
        "string",
        true,
        None,
        &[
            "charAt",
            "substring",
            "toUpperCase",
            "toLowerCase",
            "trim",
            "indexOf",
            "includes",
            "split",
            "startsWith",
            "endsWith",
            "replace",
            "replaceWith",
            "repeat",
            "padStart",
            "padEnd",
            "charCodeAt",
            "lastIndexOf",
            "trimStart",
            "trimEnd",
        ],
    );
    register_runtime_static_methods_explicit(
        &mut registry,
        "string",
        &[(
            "fromCharCode",
            BuiltinOp::Native(crate::compiler::native_id::OBJECT_STRING_FROM_CHAR_CODE),
            Some("string"),
        )],
    );
    register_exported_class(&mut registry, "Number", "number");
    register_runtime_type(
        &mut registry,
        "number",
        true,
        None,
        &["toFixed", "toPrecision", "toString"],
    );
    register_runtime_static_methods_explicit(
        &mut registry,
        "number",
        &[
            (
                "isNaN",
                BuiltinOp::Native(builtin::number::IS_NAN),
                Some("boolean"),
            ),
            (
                "isFinite",
                BuiltinOp::Native(builtin::number::IS_FINITE),
                Some("boolean"),
            ),
        ],
    );
    register_exported_class(&mut registry, "JSON", "JSON");
    register_runtime_static_methods_explicit(
        &mut registry,
        "JSON",
        &[
            (
                "parse",
                BuiltinOp::Native(crate::compiler::native_id::JSON_PARSE),
                Some("Json"),
            ),
            (
                "stringify",
                BuiltinOp::Native(crate::compiler::native_id::JSON_STRINGIFY),
                Some("string"),
            ),
        ],
    );
    register_exported_class(&mut registry, "Object", "Object");
    register_runtime_static_methods(
        &mut registry,
        "Object",
        &[
            (
                "is",
                BuiltinOp::Native(crate::compiler::native_id::OBJECT_SAME_VALUE),
            ),
            (
                "defineProperty",
                BuiltinOp::Native(crate::compiler::native_id::OBJECT_DEFINE_PROPERTY),
            ),
            (
                "getOwnPropertyDescriptor",
                BuiltinOp::Native(crate::compiler::native_id::OBJECT_GET_OWN_PROPERTY_DESCRIPTOR),
            ),
            (
                "getOwnPropertyNames",
                BuiltinOp::Native(crate::compiler::native_id::REFLECT_GET_FIELD_NAMES),
            ),
            (
                "getOwnPropertySymbols",
                BuiltinOp::Native(crate::compiler::native_id::OBJECT_GET_OWN_PROPERTY_SYMBOLS),
            ),
            (
                "getPrototypeOf",
                BuiltinOp::Native(crate::compiler::native_id::OBJECT_GET_PROTOTYPE_OF),
            ),
            (
                "setPrototypeOf",
                BuiltinOp::Native(crate::compiler::native_id::OBJECT_SET_PROTOTYPE_OF),
            ),
            (
                "isExtensible",
                BuiltinOp::Native(crate::compiler::native_id::OBJECT_IS_EXTENSIBLE),
            ),
            (
                "preventExtensions",
                BuiltinOp::Native(crate::compiler::native_id::OBJECT_PREVENT_EXTENSIONS),
            ),
            (
                "defineProperties",
                BuiltinOp::Native(crate::compiler::native_id::OBJECT_DEFINE_PROPERTIES),
            ),
        ],
    );
    register_exported_class(&mut registry, "Map", "Map");
    register_runtime_type(
        &mut registry,
        "Map",
        false,
        Some(BuiltinOp::Native(builtin::map::NEW)),
        &[
            "get",
            "set",
            "has",
            "delete",
            "clear",
            "keys",
            "values",
            "entries",
            "forEach",
        ],
    );
    register_runtime_properties(
        &mut registry,
        "Map",
        &[("size", BuiltinOp::Native(builtin::map::SIZE))],
    );
    register_exported_class(&mut registry, "Set", "Set");
    register_runtime_type(
        &mut registry,
        "Set",
        false,
        Some(BuiltinOp::Native(builtin::set::NEW)),
        &[
            "add",
            "has",
            "delete",
            "clear",
            "keys",
            "values",
            "entries",
            "forEach",
            "union",
            "intersection",
            "difference",
        ],
    );
    register_runtime_properties(
        &mut registry,
        "Set",
        &[("size", BuiltinOp::Native(builtin::set::SIZE))],
    );
    register_exported_class(&mut registry, "RegExp", "RegExp");
    register_runtime_type(
        &mut registry,
        "RegExp",
        false,
        Some(BuiltinOp::Native(builtin::regexp::NEW)),
        &["test", "exec", "execAll", "replace", "replaceWith", "split"],
    );
    register_exported_class(&mut registry, "Buffer", "Buffer");
    register_runtime_type(
        &mut registry,
        "Buffer",
        false,
        Some(BuiltinOp::Native(builtin::buffer::NEW)),
        &[
            "getByte",
            "setByte",
            "getInt32",
            "setInt32",
            "getFloat64",
            "setFloat64",
            "slice",
            "copy",
            "toString",
        ],
    );
    register_runtime_properties(
        &mut registry,
        "Buffer",
        &[("length", BuiltinOp::Native(builtin::buffer::LENGTH))],
    );
    register_exported_class(&mut registry, "Date", "Date");
    register_runtime_static_methods(
        &mut registry,
        "Date",
        &[
            ("now", BuiltinOp::Native(builtin::date::NOW)),
            ("parse", BuiltinOp::Native(builtin::date::PARSE)),
        ],
    );
    register_exported_class(&mut registry, "Mutex", "Mutex");
    register_wrapper_type(
        &mut registry,
        "Mutex",
        BuiltinOp::HostHandle(HostHandleOpKind::MutexConstructor),
        &[
            ("lock", BuiltinOp::HostHandle(HostHandleOpKind::MutexLock)),
            ("unlock", BuiltinOp::HostHandle(HostHandleOpKind::MutexUnlock)),
            ("tryLock", BuiltinOp::HostHandle(HostHandleOpKind::MutexTryLock)),
            (
                "isLocked",
                BuiltinOp::HostHandle(HostHandleOpKind::MutexIsLocked),
            ),
        ],
    );
    register_exported_class(&mut registry, "Channel", "Channel");
    register_wrapper_type(
        &mut registry,
        "Channel",
        BuiltinOp::HostHandle(HostHandleOpKind::ChannelConstructor),
        &[
            ("send", BuiltinOp::HostHandle(HostHandleOpKind::ChannelSend)),
            (
                "receive",
                BuiltinOp::HostHandle(HostHandleOpKind::ChannelReceive),
            ),
            (
                "trySend",
                BuiltinOp::HostHandle(HostHandleOpKind::ChannelTrySend),
            ),
            (
                "tryReceive",
                BuiltinOp::HostHandle(HostHandleOpKind::ChannelTryReceive),
            ),
            ("close", BuiltinOp::HostHandle(HostHandleOpKind::ChannelClose)),
            (
                "isClosed",
                BuiltinOp::HostHandle(HostHandleOpKind::ChannelIsClosed),
            ),
            (
                "length",
                BuiltinOp::HostHandle(HostHandleOpKind::ChannelLength),
            ),
            (
                "capacity",
                BuiltinOp::HostHandle(HostHandleOpKind::ChannelCapacity),
            ),
        ],
    );
    register_exported_class(&mut registry, "Reflect", "Reflect");
    register_exported_class(&mut registry, "Symbol", "Symbol");
    register_exported_class(&mut registry, "Boolean", "boolean");
    register_exported_class(&mut registry, "Error", "Error");
    register_exported_class(&mut registry, "AggregateError", "AggregateError");
    register_exported_class(&mut registry, "TypeError", "TypeError");
    register_exported_class(&mut registry, "Function", "Function");
    register_exported_class(&mut registry, "Promise", "Promise");
    register_runtime_methods_explicit(
        &mut registry,
        "Promise",
        &[
            (
                "cancel",
                BuiltinOp::HostHandle(HostHandleOpKind::TaskCancel),
                Some("void"),
            ),
            (
                "isDone",
                BuiltinOp::HostHandle(HostHandleOpKind::TaskIsDone),
                Some("boolean"),
            ),
            (
                "isCancelled",
                BuiltinOp::HostHandle(HostHandleOpKind::TaskIsCancelled),
                Some("boolean"),
            ),
            (
                "then",
                BuiltinOp::Native(builtin::promise::CHAIN),
                Some("Promise"),
            ),
            (
                "catch",
                BuiltinOp::Native(builtin::promise::CHAIN),
                Some("Promise"),
            ),
            (
                "finally",
                BuiltinOp::Native(builtin::promise::FINALLY),
                Some("Promise"),
            ),
        ],
    );
    register_runtime_static_methods_explicit(
        &mut registry,
        "Promise",
        &[
            (
                "resolve",
                BuiltinOp::Native(builtin::promise::ADOPT),
                Some("Promise"),
            ),
            (
                "reject",
                BuiltinOp::Native(builtin::promise::REJECT_NOW),
                Some("Promise"),
            ),
            (
                "all",
                BuiltinOp::Native(builtin::promise::ALL),
                Some("Promise"),
            ),
            (
                "race",
                BuiltinOp::Native(builtin::promise::RACE),
                Some("Promise"),
            ),
        ],
    );
    register_exported_class(&mut registry, "Math", "Math");
    register_runtime_static_methods_explicit(
        &mut registry,
        "Math",
        &[
            ("abs", BuiltinOp::Native(builtin::math::ABS), Some("number")),
            ("sign", BuiltinOp::Native(builtin::math::SIGN), Some("number")),
            ("floor", BuiltinOp::Native(builtin::math::FLOOR), Some("number")),
            ("ceil", BuiltinOp::Native(builtin::math::CEIL), Some("number")),
            ("round", BuiltinOp::Native(builtin::math::ROUND), Some("number")),
            ("trunc", BuiltinOp::Native(builtin::math::TRUNC), Some("number")),
            ("min", BuiltinOp::Native(builtin::math::MIN), Some("number")),
            ("max", BuiltinOp::Native(builtin::math::MAX), Some("number")),
            ("pow", BuiltinOp::Native(builtin::math::POW), Some("number")),
            ("sqrt", BuiltinOp::Native(builtin::math::SQRT), Some("number")),
            ("sin", BuiltinOp::Native(builtin::math::SIN), Some("number")),
            ("cos", BuiltinOp::Native(builtin::math::COS), Some("number")),
            ("tan", BuiltinOp::Native(builtin::math::TAN), Some("number")),
            ("asin", BuiltinOp::Native(builtin::math::ASIN), Some("number")),
            ("acos", BuiltinOp::Native(builtin::math::ACOS), Some("number")),
            ("atan", BuiltinOp::Native(builtin::math::ATAN), Some("number")),
            ("atan2", BuiltinOp::Native(builtin::math::ATAN2), Some("number")),
            ("exp", BuiltinOp::Native(builtin::math::EXP), Some("number")),
            ("log", BuiltinOp::Native(builtin::math::LOG), Some("number")),
            ("log10", BuiltinOp::Native(builtin::math::LOG10), Some("number")),
            ("random", BuiltinOp::Native(builtin::math::RANDOM), Some("number")),
        ],
    );
    register_exported_nominal_class(&mut registry, "EventEmitter", "EventEmitter");

    registry
}

fn register_exported_class(
    registry: &mut BuiltinRegistry,
    global_name: &'static str,
    backing_type_name: &'static str,
) {
    registry.globals.insert(
        global_name,
        BuiltinGlobalDescriptor {
            global_name,
            backing_type_name,
            symbol_type: SymbolType::Constant,
        },
    );
}

fn register_exported_nominal_class(
    registry: &mut BuiltinRegistry,
    global_name: &'static str,
    backing_type_name: &'static str,
) {
    registry.globals.insert(
        global_name,
        BuiltinGlobalDescriptor {
            global_name,
            backing_type_name,
            symbol_type: SymbolType::Class,
        },
    );
}

fn register_runtime_type(
    registry: &mut BuiltinRegistry,
    type_name: &'static str,
    builtin_primitive: bool,
    constructor: Option<BuiltinOp>,
    methods: &[&'static str],
) {
    let descriptor = registry.types.entry(type_name).or_default();
    descriptor.builtin_primitive = builtin_primitive;
    descriptor.constructor = constructor.map(|op| BuiltinBindingDescriptor {
        op,
        return_type_name: None,
    });
    for &method_name in methods {
        let native_id = builtin::lookup_builtin_method(type_name, method_name)
            .unwrap_or_else(|| panic!("missing runtime builtin method: {type_name}.{method_name}"));
        let op = builtin_op_from_native_id(native_id).unwrap_or(BuiltinOp::Native(native_id));
        descriptor.instance_methods.insert(
            method_name,
            BuiltinBindingDescriptor {
                op,
                return_type_name: builtin_member_return_type(type_name, method_name, false),
            },
        );
    }
}

fn register_wrapper_type(
    registry: &mut BuiltinRegistry,
    type_name: &'static str,
    constructor: BuiltinOp,
    methods: &[(&'static str, BuiltinOp)],
) {
    let descriptor = registry.types.entry(type_name).or_default();
    descriptor.wrapper_method_surface = true;
    descriptor.constructor = Some(BuiltinBindingDescriptor {
        op: constructor,
        return_type_name: None,
    });
    for &(method_name, op) in methods {
        descriptor.instance_methods.insert(
            method_name,
            BuiltinBindingDescriptor {
                op,
                return_type_name: builtin_member_return_type(type_name, method_name, false),
            },
        );
    }
}

fn register_runtime_properties(
    registry: &mut BuiltinRegistry,
    type_name: &'static str,
    properties: &[(&'static str, BuiltinOp)],
) {
    let descriptor = registry.types.entry(type_name).or_default();
    for &(property_name, op) in properties {
        descriptor.instance_properties.insert(
            property_name,
            BuiltinBindingDescriptor {
                op,
                return_type_name: builtin_member_return_type(type_name, property_name, false),
            },
        );
    }
}

fn register_runtime_methods_explicit(
    registry: &mut BuiltinRegistry,
    type_name: &'static str,
    methods: &[(&'static str, BuiltinOp, Option<&'static str>)],
) {
    let descriptor = registry.types.entry(type_name).or_default();
    for &(method_name, op, return_type_name) in methods {
        descriptor.instance_methods.insert(
            method_name,
            BuiltinBindingDescriptor {
                op,
                return_type_name,
            },
        );
    }
}

fn register_runtime_static_methods(
    registry: &mut BuiltinRegistry,
    type_name: &'static str,
    methods: &[(&'static str, BuiltinOp)],
) {
    let descriptor = registry.types.entry(type_name).or_default();
    for &(method_name, op) in methods {
        descriptor.static_methods.insert(
            method_name,
            BuiltinBindingDescriptor {
                op,
                return_type_name: builtin_member_return_type(type_name, method_name, true),
            },
        );
    }
}

fn register_runtime_static_methods_explicit(
    registry: &mut BuiltinRegistry,
    type_name: &'static str,
    methods: &[(&'static str, BuiltinOp, Option<&'static str>)],
) {
    let descriptor = registry.types.entry(type_name).or_default();
    for &(method_name, op, return_type_name) in methods {
        descriptor.static_methods.insert(
            method_name,
            BuiltinBindingDescriptor {
                op,
                return_type_name,
            },
        );
    }
}

fn builtin_member_return_type(
    type_name: &str,
    member_name: &str,
    is_static: bool,
) -> Option<&'static str> {
    let signatures = builtins::get_signatures(signature_lookup_name(type_name))?;
    let class = signatures
        .classes
        .iter()
        .find(|class| class.name.eq_ignore_ascii_case(type_name))?;
    if let Some(method) = class
        .methods
        .iter()
        .find(|method| method.name == member_name && method.is_static == is_static)
    {
        return Some(method.return_type);
    }
    class
        .properties
        .iter()
        .find(|property| property.name == member_name && property.is_static == is_static)
        .map(|property| property.ty)
}

fn signature_lookup_name(type_name: &str) -> &str {
    match type_name {
        "String" | "string" => "string",
        other => canonical_runtime_type_name(other),
    }
}

fn canonical_runtime_type_name(type_name: &str) -> &str {
    match type_name {
        "array" => "Array",
        "map" => "Map",
        "set" => "Set",
        "regexp" => "RegExp",
        "mutex" => "Mutex",
        "channel" => "Channel",
        "String" => "string",
        other => other,
    }
}

pub fn builtin_descriptor(op: BuiltinOp) -> BuiltinDescriptor {
    match op {
        BuiltinOp::Native(native_id) => BuiltinDescriptor {
            op,
            name: if native_id == 0 {
                "native.0"
            } else {
                "native.runtime"
            },
            surface: BuiltinSurfaceKind::NamespaceCall,
            receiver: BuiltinReceiverModel::None,
            execution: BuiltinExecutionKind::RuntimeBuiltin,
        },
        BuiltinOp::Metaobject(kind) => BuiltinDescriptor {
            op,
            name: match kind {
                MetaobjectOpKind::DefineProperty => "metaobject.defineProperty",
                MetaobjectOpKind::GetOwnPropertyDescriptor => "metaobject.getOwnPropertyDescriptor",
                MetaobjectOpKind::DefineProperties => "metaobject.defineProperties",
                MetaobjectOpKind::DeleteProperty => "metaobject.deleteProperty",
                MetaobjectOpKind::GetPrototypeOf => "metaobject.getPrototypeOf",
                MetaobjectOpKind::SetPrototypeOf => "metaobject.setPrototypeOf",
                MetaobjectOpKind::PreventExtensions => "metaobject.preventExtensions",
                MetaobjectOpKind::IsExtensible => "metaobject.isExtensible",
                MetaobjectOpKind::ReflectGet => "metaobject.reflectGet",
                MetaobjectOpKind::ReflectSet => "metaobject.reflectSet",
                MetaobjectOpKind::ReflectHas => "metaobject.reflectHas",
                MetaobjectOpKind::ReflectOwnKeys => "metaobject.reflectOwnKeys",
                MetaobjectOpKind::ReflectConstruct => "metaobject.reflectConstruct",
            },
            surface: BuiltinSurfaceKind::NamespaceCall,
            receiver: BuiltinReceiverModel::Object,
            execution: BuiltinExecutionKind::RuntimeBuiltin,
        },
        BuiltinOp::Iterator(kind) => BuiltinDescriptor {
            op,
            name: match kind {
                IteratorOpKind::GetIterator => "iterator.get",
                IteratorOpKind::GetAsyncIterator => "iterator.getAsync",
                IteratorOpKind::Step => "iterator.step",
                IteratorOpKind::Done => "iterator.done",
                IteratorOpKind::Value => "iterator.value",
                IteratorOpKind::ResumeNext => "iterator.resumeNext",
                IteratorOpKind::ResumeReturn => "iterator.resumeReturn",
                IteratorOpKind::ResumeThrow => "iterator.resumeThrow",
                IteratorOpKind::Close => "iterator.close",
                IteratorOpKind::CloseOnThrow => "iterator.closeOnThrow",
                IteratorOpKind::CloseCompletion => "iterator.closeCompletion",
                IteratorOpKind::AppendToArray => "iterator.appendToArray",
            },
            surface: BuiltinSurfaceKind::NamespaceCall,
            receiver: BuiltinReceiverModel::Value,
            execution: BuiltinExecutionKind::RuntimeBuiltin,
        },
        BuiltinOp::HostHandle(kind) => BuiltinDescriptor {
            op,
            name: match kind {
                HostHandleOpKind::ChannelConstructor => "channel.constructor",
                HostHandleOpKind::ChannelSend => "channel.send",
                HostHandleOpKind::ChannelReceive => "channel.receive",
                HostHandleOpKind::ChannelTrySend => "channel.trySend",
                HostHandleOpKind::ChannelTryReceive => "channel.tryReceive",
                HostHandleOpKind::ChannelClose => "channel.close",
                HostHandleOpKind::ChannelIsClosed => "channel.isClosed",
                HostHandleOpKind::ChannelLength => "channel.length",
                HostHandleOpKind::ChannelCapacity => "channel.capacity",
                HostHandleOpKind::MutexConstructor => "mutex.constructor",
                HostHandleOpKind::MutexLock => "mutex.lock",
                HostHandleOpKind::MutexUnlock => "mutex.unlock",
                HostHandleOpKind::MutexTryLock => "mutex.tryLock",
                HostHandleOpKind::MutexIsLocked => "mutex.isLocked",
                HostHandleOpKind::TaskCancel => "promise.cancel",
                HostHandleOpKind::TaskIsDone => "promise.isDone",
                HostHandleOpKind::TaskIsCancelled => "promise.isCancelled",
            },
            surface: match kind {
                HostHandleOpKind::ChannelConstructor | HostHandleOpKind::MutexConstructor => {
                    BuiltinSurfaceKind::Constructor
                }
                _ => BuiltinSurfaceKind::InstanceMethod,
            },
            receiver: match kind {
                HostHandleOpKind::ChannelConstructor | HostHandleOpKind::MutexConstructor => {
                    BuiltinReceiverModel::None
                }
                HostHandleOpKind::ChannelSend
                | HostHandleOpKind::ChannelReceive
                | HostHandleOpKind::ChannelTrySend
                | HostHandleOpKind::ChannelTryReceive
                | HostHandleOpKind::ChannelClose
                | HostHandleOpKind::ChannelIsClosed
                | HostHandleOpKind::ChannelLength
                | HostHandleOpKind::ChannelCapacity => BuiltinReceiverModel::Handle(HandleKind::Channel),
                HostHandleOpKind::MutexLock
                | HostHandleOpKind::MutexUnlock
                | HostHandleOpKind::MutexTryLock
                | HostHandleOpKind::MutexIsLocked => BuiltinReceiverModel::Handle(HandleKind::Mutex),
                HostHandleOpKind::TaskCancel
                | HostHandleOpKind::TaskIsDone
                | HostHandleOpKind::TaskIsCancelled => BuiltinReceiverModel::TaskHandle,
            },
            execution: BuiltinExecutionKind::RuntimeBuiltin,
        },
        BuiltinOp::Js(kind) => BuiltinDescriptor {
            op,
            name: match kind {
                JsOpKind::GetNamed => "js.getNamed",
                JsOpKind::GetKeyed => "js.getKeyed",
                JsOpKind::SetNamed { strict: false } => "js.setNamed",
                JsOpKind::SetNamed { strict: true } => "js.setNamedStrict",
                JsOpKind::SetKeyed { strict: false } => "js.setKeyed",
                JsOpKind::SetKeyed { strict: true } => "js.setKeyedStrict",
                JsOpKind::BindMethod => "js.bindMethod",
                JsOpKind::ResolveIdentifier { non_throwing: false } => "js.resolveIdentifier",
                JsOpKind::ResolveIdentifier { non_throwing: true } => {
                    "js.resolveIdentifierNonThrowing"
                }
                JsOpKind::HasIdentifier => "js.hasIdentifier",
                JsOpKind::AssignIdentifier { strict: false } => "js.assignIdentifier",
                JsOpKind::AssignIdentifier { strict: true } => "js.assignIdentifierStrict",
                JsOpKind::DeleteIdentifier => "js.deleteIdentifier",
                JsOpKind::DeclareVar => "js.declareVar",
                JsOpKind::DeclareFunction => "js.declareFunction",
                JsOpKind::DeclareLexical => "js.declareLexical",
                JsOpKind::CallValue => "js.callValue",
                JsOpKind::CallMemberNamed => "js.callMemberNamed",
                JsOpKind::CallMemberKeyed => "js.callMemberKeyed",
                JsOpKind::ConstructValue => "js.constructValue",
                JsOpKind::EnterActivationEnv => "js.enterActivationEnv",
                JsOpKind::LeaveActivationEnv => "js.leaveActivationEnv",
                JsOpKind::PushWithEnv => "js.pushWithEnv",
                JsOpKind::PopWithEnv => "js.popWithEnv",
                JsOpKind::PushDeclarativeEnv => "js.pushDeclarativeEnv",
                JsOpKind::PopDeclarativeEnv => "js.popDeclarativeEnv",
                JsOpKind::ReplaceDeclarativeEnv => "js.replaceDeclarativeEnv",
                JsOpKind::DirectEval => "js.directEval",
                JsOpKind::EvalGetCompletion => "js.evalGetCompletion",
                JsOpKind::EvalSetCompletion => "js.evalSetCompletion",
            },
            surface: BuiltinSurfaceKind::NamespaceCall,
            receiver: match kind {
                JsOpKind::GetNamed
                | JsOpKind::GetKeyed
                | JsOpKind::SetNamed { .. }
                | JsOpKind::SetKeyed { .. }
                | JsOpKind::BindMethod
                | JsOpKind::CallMemberNamed
                | JsOpKind::CallMemberKeyed => BuiltinReceiverModel::Object,
                JsOpKind::CallValue | JsOpKind::ConstructValue => BuiltinReceiverModel::Value,
                _ => BuiltinReceiverModel::None,
            },
            execution: BuiltinExecutionKind::RuntimeBuiltin,
        },
    }
}

pub fn encode_builtin_op_id(op: BuiltinOp) -> BuiltinOpId {
    match op {
        BuiltinOp::Native(native_id) => NATIVE_BASE + native_id,
        BuiltinOp::Metaobject(kind) => METAOBJECT_BASE
            + match kind {
                MetaobjectOpKind::DefineProperty => 0,
                MetaobjectOpKind::GetOwnPropertyDescriptor => 1,
                MetaobjectOpKind::DefineProperties => 2,
                MetaobjectOpKind::DeleteProperty => 3,
                MetaobjectOpKind::GetPrototypeOf => 4,
                MetaobjectOpKind::SetPrototypeOf => 5,
                MetaobjectOpKind::PreventExtensions => 6,
                MetaobjectOpKind::IsExtensible => 7,
                MetaobjectOpKind::ReflectGet => 8,
                MetaobjectOpKind::ReflectSet => 9,
                MetaobjectOpKind::ReflectHas => 10,
                MetaobjectOpKind::ReflectOwnKeys => 11,
                MetaobjectOpKind::ReflectConstruct => 12,
            },
        BuiltinOp::Iterator(kind) => ITERATOR_BASE
            + match kind {
                IteratorOpKind::GetIterator => 0,
                IteratorOpKind::GetAsyncIterator => 1,
                IteratorOpKind::Step => 2,
                IteratorOpKind::Done => 3,
                IteratorOpKind::Value => 4,
                IteratorOpKind::ResumeNext => 5,
                IteratorOpKind::ResumeReturn => 6,
                IteratorOpKind::ResumeThrow => 7,
                IteratorOpKind::Close => 8,
                IteratorOpKind::CloseOnThrow => 9,
                IteratorOpKind::CloseCompletion => 10,
                IteratorOpKind::AppendToArray => 11,
            },
        BuiltinOp::HostHandle(kind) => HOST_HANDLE_BASE
            + match kind {
                HostHandleOpKind::ChannelConstructor => 0,
                HostHandleOpKind::ChannelSend => 1,
                HostHandleOpKind::ChannelReceive => 2,
                HostHandleOpKind::ChannelTrySend => 3,
                HostHandleOpKind::ChannelTryReceive => 4,
                HostHandleOpKind::ChannelClose => 5,
                HostHandleOpKind::ChannelIsClosed => 6,
                HostHandleOpKind::ChannelLength => 7,
                HostHandleOpKind::ChannelCapacity => 8,
                HostHandleOpKind::MutexConstructor => 9,
                HostHandleOpKind::MutexLock => 10,
                HostHandleOpKind::MutexUnlock => 11,
                HostHandleOpKind::MutexTryLock => 12,
                HostHandleOpKind::MutexIsLocked => 13,
                HostHandleOpKind::TaskCancel => 14,
                HostHandleOpKind::TaskIsDone => 15,
                HostHandleOpKind::TaskIsCancelled => 16,
            },
        BuiltinOp::Js(kind) => JS_BASE
            + match kind {
                JsOpKind::GetNamed => 0,
                JsOpKind::GetKeyed => 1,
                JsOpKind::SetNamed { strict: false } => 2,
                JsOpKind::SetNamed { strict: true } => 3,
                JsOpKind::SetKeyed { strict: false } => 4,
                JsOpKind::SetKeyed { strict: true } => 5,
                JsOpKind::BindMethod => 6,
                JsOpKind::ResolveIdentifier { non_throwing: false } => 7,
                JsOpKind::ResolveIdentifier { non_throwing: true } => 8,
                JsOpKind::AssignIdentifier { strict: false } => 9,
                JsOpKind::AssignIdentifier { strict: true } => 10,
                JsOpKind::CallValue => 11,
                JsOpKind::CallMemberNamed => 12,
                JsOpKind::CallMemberKeyed => 13,
                JsOpKind::ConstructValue => 14,
                JsOpKind::PushWithEnv => 15,
                JsOpKind::PopWithEnv => 16,
                JsOpKind::PushDeclarativeEnv => 17,
                JsOpKind::PopDeclarativeEnv => 18,
                JsOpKind::ReplaceDeclarativeEnv => 19,
                JsOpKind::DirectEval => 20,
                JsOpKind::EvalGetCompletion => 21,
                JsOpKind::EvalSetCompletion => 22,
                JsOpKind::HasIdentifier => 23,
                JsOpKind::DeleteIdentifier => 24,
                JsOpKind::DeclareVar => 25,
                JsOpKind::DeclareFunction => 26,
                JsOpKind::DeclareLexical => 27,
                JsOpKind::EnterActivationEnv => 28,
                JsOpKind::LeaveActivationEnv => 29,
            },
    }
}

pub fn decode_builtin_op_id(id: BuiltinOpId) -> Option<BuiltinOp> {
    if id >= NATIVE_BASE {
        return Some(BuiltinOp::Native(id - NATIVE_BASE));
    }
    if id >= JS_BASE {
        return Some(BuiltinOp::Js(match id - JS_BASE {
            0 => JsOpKind::GetNamed,
            1 => JsOpKind::GetKeyed,
            2 => JsOpKind::SetNamed { strict: false },
            3 => JsOpKind::SetNamed { strict: true },
            4 => JsOpKind::SetKeyed { strict: false },
            5 => JsOpKind::SetKeyed { strict: true },
            6 => JsOpKind::BindMethod,
            7 => JsOpKind::ResolveIdentifier { non_throwing: false },
            8 => JsOpKind::ResolveIdentifier { non_throwing: true },
            9 => JsOpKind::AssignIdentifier { strict: false },
            10 => JsOpKind::AssignIdentifier { strict: true },
            11 => JsOpKind::CallValue,
            12 => JsOpKind::CallMemberNamed,
            13 => JsOpKind::CallMemberKeyed,
            14 => JsOpKind::ConstructValue,
            15 => JsOpKind::PushWithEnv,
            16 => JsOpKind::PopWithEnv,
            17 => JsOpKind::PushDeclarativeEnv,
            18 => JsOpKind::PopDeclarativeEnv,
            19 => JsOpKind::ReplaceDeclarativeEnv,
            20 => JsOpKind::DirectEval,
            21 => JsOpKind::EvalGetCompletion,
            22 => JsOpKind::EvalSetCompletion,
            23 => JsOpKind::HasIdentifier,
            24 => JsOpKind::DeleteIdentifier,
            25 => JsOpKind::DeclareVar,
            26 => JsOpKind::DeclareFunction,
            27 => JsOpKind::DeclareLexical,
            28 => JsOpKind::EnterActivationEnv,
            29 => JsOpKind::LeaveActivationEnv,
            _ => unreachable!(),
        }));
    }
    if id >= HOST_HANDLE_BASE {
        return Some(BuiltinOp::HostHandle(match id - HOST_HANDLE_BASE {
            0 => HostHandleOpKind::ChannelConstructor,
            1 => HostHandleOpKind::ChannelSend,
            2 => HostHandleOpKind::ChannelReceive,
            3 => HostHandleOpKind::ChannelTrySend,
            4 => HostHandleOpKind::ChannelTryReceive,
            5 => HostHandleOpKind::ChannelClose,
            6 => HostHandleOpKind::ChannelIsClosed,
            7 => HostHandleOpKind::ChannelLength,
            8 => HostHandleOpKind::ChannelCapacity,
            9 => HostHandleOpKind::MutexConstructor,
            10 => HostHandleOpKind::MutexLock,
            11 => HostHandleOpKind::MutexUnlock,
            12 => HostHandleOpKind::MutexTryLock,
            13 => HostHandleOpKind::MutexIsLocked,
            14 => HostHandleOpKind::TaskCancel,
            15 => HostHandleOpKind::TaskIsDone,
            16 => HostHandleOpKind::TaskIsCancelled,
            _ => unreachable!(),
        }));
    }
    if id >= ITERATOR_BASE {
        return Some(BuiltinOp::Iterator(match id - ITERATOR_BASE {
            0 => IteratorOpKind::GetIterator,
            1 => IteratorOpKind::GetAsyncIterator,
            2 => IteratorOpKind::Step,
            3 => IteratorOpKind::Done,
            4 => IteratorOpKind::Value,
            5 => IteratorOpKind::ResumeNext,
            6 => IteratorOpKind::ResumeReturn,
            7 => IteratorOpKind::ResumeThrow,
            8 => IteratorOpKind::Close,
            9 => IteratorOpKind::CloseOnThrow,
            10 => IteratorOpKind::CloseCompletion,
            11 => IteratorOpKind::AppendToArray,
            _ => unreachable!(),
        }));
    }
    Some(BuiltinOp::Metaobject(match id - METAOBJECT_BASE {
        0 => MetaobjectOpKind::DefineProperty,
        1 => MetaobjectOpKind::GetOwnPropertyDescriptor,
        2 => MetaobjectOpKind::DefineProperties,
        3 => MetaobjectOpKind::DeleteProperty,
        4 => MetaobjectOpKind::GetPrototypeOf,
        5 => MetaobjectOpKind::SetPrototypeOf,
        6 => MetaobjectOpKind::PreventExtensions,
        7 => MetaobjectOpKind::IsExtensible,
        8 => MetaobjectOpKind::ReflectGet,
        9 => MetaobjectOpKind::ReflectSet,
        10 => MetaobjectOpKind::ReflectHas,
        11 => MetaobjectOpKind::ReflectOwnKeys,
        12 => MetaobjectOpKind::ReflectConstruct,
        _ => unreachable!(),
    }))
}

pub fn builtin_op_from_native_id(native_id: u16) -> Option<BuiltinOp> {
    Some(match native_id {
        crate::compiler::native_id::OBJECT_DEFINE_PROPERTY => {
            BuiltinOp::Metaobject(MetaobjectOpKind::DefineProperty)
        }
        crate::compiler::native_id::OBJECT_GET_OWN_PROPERTY_DESCRIPTOR => {
            BuiltinOp::Metaobject(MetaobjectOpKind::GetOwnPropertyDescriptor)
        }
        crate::compiler::native_id::OBJECT_DEFINE_PROPERTIES => {
            BuiltinOp::Metaobject(MetaobjectOpKind::DefineProperties)
        }
        crate::compiler::native_id::OBJECT_DELETE_PROPERTY => {
            BuiltinOp::Metaobject(MetaobjectOpKind::DeleteProperty)
        }
        crate::compiler::native_id::OBJECT_GET_PROTOTYPE_OF => {
            BuiltinOp::Metaobject(MetaobjectOpKind::GetPrototypeOf)
        }
        crate::compiler::native_id::OBJECT_SET_PROTOTYPE_OF => {
            BuiltinOp::Metaobject(MetaobjectOpKind::SetPrototypeOf)
        }
        crate::compiler::native_id::OBJECT_PREVENT_EXTENSIONS => {
            BuiltinOp::Metaobject(MetaobjectOpKind::PreventExtensions)
        }
        crate::compiler::native_id::OBJECT_IS_EXTENSIBLE => {
            BuiltinOp::Metaobject(MetaobjectOpKind::IsExtensible)
        }
        crate::compiler::native_id::REFLECT_GET => BuiltinOp::Metaobject(MetaobjectOpKind::ReflectGet),
        crate::compiler::native_id::REFLECT_SET => BuiltinOp::Metaobject(MetaobjectOpKind::ReflectSet),
        crate::compiler::native_id::REFLECT_HAS => BuiltinOp::Metaobject(MetaobjectOpKind::ReflectHas),
        crate::compiler::native_id::REFLECT_OWN_KEYS => {
            BuiltinOp::Metaobject(MetaobjectOpKind::ReflectOwnKeys)
        }
        crate::compiler::native_id::REFLECT_CONSTRUCT => {
            BuiltinOp::Metaobject(MetaobjectOpKind::ReflectConstruct)
        }
        crate::compiler::native_id::OBJECT_ITERATOR_GET => {
            BuiltinOp::Iterator(IteratorOpKind::GetIterator)
        }
        crate::compiler::native_id::OBJECT_ASYNC_ITERATOR_GET => {
            BuiltinOp::Iterator(IteratorOpKind::GetAsyncIterator)
        }
        crate::compiler::native_id::OBJECT_ITERATOR_STEP => {
            BuiltinOp::Iterator(IteratorOpKind::Step)
        }
        crate::compiler::native_id::OBJECT_ITERATOR_DONE => {
            BuiltinOp::Iterator(IteratorOpKind::Done)
        }
        crate::compiler::native_id::OBJECT_ITERATOR_VALUE => {
            BuiltinOp::Iterator(IteratorOpKind::Value)
        }
        crate::compiler::native_id::OBJECT_ITERATOR_RESUME_NEXT => {
            BuiltinOp::Iterator(IteratorOpKind::ResumeNext)
        }
        crate::compiler::native_id::OBJECT_ITERATOR_RESUME_RETURN => {
            BuiltinOp::Iterator(IteratorOpKind::ResumeReturn)
        }
        crate::compiler::native_id::OBJECT_ITERATOR_RESUME_THROW => {
            BuiltinOp::Iterator(IteratorOpKind::ResumeThrow)
        }
        crate::compiler::native_id::OBJECT_ITERATOR_CLOSE => {
            BuiltinOp::Iterator(IteratorOpKind::Close)
        }
        crate::compiler::native_id::OBJECT_ITERATOR_CLOSE_ON_THROW => {
            BuiltinOp::Iterator(IteratorOpKind::CloseOnThrow)
        }
        crate::compiler::native_id::OBJECT_ITERATOR_CLOSE_COMPLETION => {
            BuiltinOp::Iterator(IteratorOpKind::CloseCompletion)
        }
        crate::compiler::native_id::OBJECT_ITERATOR_APPEND_TO_ARRAY => {
            BuiltinOp::Iterator(IteratorOpKind::AppendToArray)
        }
        crate::compiler::native_id::CHANNEL_NEW => {
            BuiltinOp::HostHandle(HostHandleOpKind::ChannelConstructor)
        }
        crate::compiler::native_id::CHANNEL_SEND => {
            BuiltinOp::HostHandle(HostHandleOpKind::ChannelSend)
        }
        crate::compiler::native_id::CHANNEL_RECEIVE => {
            BuiltinOp::HostHandle(HostHandleOpKind::ChannelReceive)
        }
        crate::compiler::native_id::CHANNEL_TRY_SEND => {
            BuiltinOp::HostHandle(HostHandleOpKind::ChannelTrySend)
        }
        crate::compiler::native_id::CHANNEL_TRY_RECEIVE => {
            BuiltinOp::HostHandle(HostHandleOpKind::ChannelTryReceive)
        }
        crate::compiler::native_id::CHANNEL_CLOSE => {
            BuiltinOp::HostHandle(HostHandleOpKind::ChannelClose)
        }
        crate::compiler::native_id::CHANNEL_IS_CLOSED => {
            BuiltinOp::HostHandle(HostHandleOpKind::ChannelIsClosed)
        }
        crate::compiler::native_id::CHANNEL_LENGTH => {
            BuiltinOp::HostHandle(HostHandleOpKind::ChannelLength)
        }
        crate::compiler::native_id::CHANNEL_CAPACITY => {
            BuiltinOp::HostHandle(HostHandleOpKind::ChannelCapacity)
        }
        crate::compiler::native_id::MUTEX_TRY_LOCK => {
            BuiltinOp::HostHandle(HostHandleOpKind::MutexTryLock)
        }
        crate::compiler::native_id::MUTEX_IS_LOCKED => {
            BuiltinOp::HostHandle(HostHandleOpKind::MutexIsLocked)
        }
        crate::compiler::native_id::TASK_IS_DONE => {
            BuiltinOp::HostHandle(HostHandleOpKind::TaskIsDone)
        }
        crate::compiler::native_id::TASK_IS_CANCELLED => {
            BuiltinOp::HostHandle(HostHandleOpKind::TaskIsCancelled)
        }
        _ => return None,
    })
}

pub fn native_id_for_builtin_op(op: BuiltinOp) -> Option<u16> {
    Some(match op {
        BuiltinOp::Native(native_id) => native_id,
        BuiltinOp::Metaobject(kind) => match kind {
            MetaobjectOpKind::DefineProperty => crate::compiler::native_id::OBJECT_DEFINE_PROPERTY,
            MetaobjectOpKind::GetOwnPropertyDescriptor => {
                crate::compiler::native_id::OBJECT_GET_OWN_PROPERTY_DESCRIPTOR
            }
            MetaobjectOpKind::DefineProperties => crate::compiler::native_id::OBJECT_DEFINE_PROPERTIES,
            MetaobjectOpKind::DeleteProperty => crate::compiler::native_id::OBJECT_DELETE_PROPERTY,
            MetaobjectOpKind::GetPrototypeOf => crate::compiler::native_id::OBJECT_GET_PROTOTYPE_OF,
            MetaobjectOpKind::SetPrototypeOf => crate::compiler::native_id::OBJECT_SET_PROTOTYPE_OF,
            MetaobjectOpKind::PreventExtensions => {
                crate::compiler::native_id::OBJECT_PREVENT_EXTENSIONS
            }
            MetaobjectOpKind::IsExtensible => crate::compiler::native_id::OBJECT_IS_EXTENSIBLE,
            MetaobjectOpKind::ReflectGet => crate::compiler::native_id::REFLECT_GET,
            MetaobjectOpKind::ReflectSet => crate::compiler::native_id::REFLECT_SET,
            MetaobjectOpKind::ReflectHas => crate::compiler::native_id::REFLECT_HAS,
            MetaobjectOpKind::ReflectOwnKeys => crate::compiler::native_id::REFLECT_OWN_KEYS,
            MetaobjectOpKind::ReflectConstruct => crate::compiler::native_id::REFLECT_CONSTRUCT,
        },
        BuiltinOp::Iterator(kind) => match kind {
            IteratorOpKind::GetIterator => crate::compiler::native_id::OBJECT_ITERATOR_GET,
            IteratorOpKind::GetAsyncIterator => crate::compiler::native_id::OBJECT_ASYNC_ITERATOR_GET,
            IteratorOpKind::Step => crate::compiler::native_id::OBJECT_ITERATOR_STEP,
            IteratorOpKind::Done => crate::compiler::native_id::OBJECT_ITERATOR_DONE,
            IteratorOpKind::Value => crate::compiler::native_id::OBJECT_ITERATOR_VALUE,
            IteratorOpKind::ResumeNext => crate::compiler::native_id::OBJECT_ITERATOR_RESUME_NEXT,
            IteratorOpKind::ResumeReturn => crate::compiler::native_id::OBJECT_ITERATOR_RESUME_RETURN,
            IteratorOpKind::ResumeThrow => crate::compiler::native_id::OBJECT_ITERATOR_RESUME_THROW,
            IteratorOpKind::Close => crate::compiler::native_id::OBJECT_ITERATOR_CLOSE,
            IteratorOpKind::CloseOnThrow => crate::compiler::native_id::OBJECT_ITERATOR_CLOSE_ON_THROW,
            IteratorOpKind::CloseCompletion => {
                crate::compiler::native_id::OBJECT_ITERATOR_CLOSE_COMPLETION
            }
            IteratorOpKind::AppendToArray => {
                crate::compiler::native_id::OBJECT_ITERATOR_APPEND_TO_ARRAY
            }
        },
        BuiltinOp::HostHandle(kind) => match kind {
            HostHandleOpKind::ChannelConstructor => crate::compiler::native_id::CHANNEL_NEW,
            HostHandleOpKind::ChannelSend => crate::compiler::native_id::CHANNEL_SEND,
            HostHandleOpKind::ChannelReceive => crate::compiler::native_id::CHANNEL_RECEIVE,
            HostHandleOpKind::ChannelTrySend => crate::compiler::native_id::CHANNEL_TRY_SEND,
            HostHandleOpKind::ChannelTryReceive => crate::compiler::native_id::CHANNEL_TRY_RECEIVE,
            HostHandleOpKind::ChannelClose => crate::compiler::native_id::CHANNEL_CLOSE,
            HostHandleOpKind::ChannelIsClosed => crate::compiler::native_id::CHANNEL_IS_CLOSED,
            HostHandleOpKind::ChannelLength => crate::compiler::native_id::CHANNEL_LENGTH,
            HostHandleOpKind::ChannelCapacity => crate::compiler::native_id::CHANNEL_CAPACITY,
            HostHandleOpKind::MutexTryLock => crate::compiler::native_id::MUTEX_TRY_LOCK,
            HostHandleOpKind::MutexIsLocked => crate::compiler::native_id::MUTEX_IS_LOCKED,
            HostHandleOpKind::TaskIsDone => crate::compiler::native_id::TASK_IS_DONE,
            HostHandleOpKind::TaskIsCancelled => crate::compiler::native_id::TASK_IS_CANCELLED,
            HostHandleOpKind::MutexConstructor
            | HostHandleOpKind::MutexLock
            | HostHandleOpKind::MutexUnlock
            | HostHandleOpKind::TaskCancel => return None,
        },
        BuiltinOp::Js(_) => return None,
    })
}
