use std::sync::OnceLock;

use rustc_hash::FxHashMap;

use crate::compiler::SymbolType;
use crate::compiler::type_registry::OpcodeKind;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinSurfaceMemberDescriptor {
    SurfaceOnly,
    Opcode(OpcodeKind),
    Bound(BuiltinBindingDescriptor),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BuiltinLiteral {
    String(&'static str),
    Bool(bool),
    I32(i32),
    F64(f64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct BuiltinGlobalDescriptor {
    pub global_name: &'static str,
    pub backing_type_name: &'static str,
    pub symbol_type: SymbolType,
    pub binding: Option<BuiltinBindingDescriptor>,
    pub literal: Option<BuiltinLiteral>,
}

#[derive(Debug, Clone, Default)]
pub struct BuiltinTypeDescriptor {
    pub builtin_primitive: bool,
    pub wrapper_method_surface: bool,
    pub constructor: Option<BuiltinSurfaceMemberDescriptor>,
    pub instance_methods: FxHashMap<&'static str, BuiltinSurfaceMemberDescriptor>,
    pub instance_properties: FxHashMap<&'static str, BuiltinSurfaceMemberDescriptor>,
    pub static_methods: FxHashMap<&'static str, BuiltinSurfaceMemberDescriptor>,
    pub static_properties: FxHashMap<&'static str, BuiltinSurfaceMemberDescriptor>,
}

#[derive(Debug, Default)]
pub struct BuiltinRegistry {
    globals: FxHashMap<&'static str, BuiltinGlobalDescriptor>,
    types: FxHashMap<&'static str, BuiltinTypeDescriptor>,
}

static BUILTIN_REGISTRY: OnceLock<BuiltinRegistry> = OnceLock::new();
static BUILTIN_NATIVE_OP_TABLE: OnceLock<BuiltinNativeOpTable> = OnceLock::new();

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

#[derive(Debug)]
struct BuiltinNativeOpTable {
    ordered_ids: Vec<u16>,
    indices: FxHashMap<u16, BuiltinOpId>,
}

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
            "values",
            "keys",
            "entries",
        ],
    );
    register_runtime_methods_explicit(
        &mut registry,
        "Array",
        &[("push", BuiltinOp::Native(builtin::array::PUSH), Some("number"))],
    );
    register_runtime_static_methods_explicit(
        &mut registry,
        "Array",
        &[("from", BuiltinOp::Native(builtin::array::FROM), Some("Array"))],
    );
    register_opcode_properties(&mut registry, "Array", &[("length", OpcodeKind::ArrayLen)]);
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
            "match",
            "matchAll",
            "search",
            "trimStart",
            "trimEnd",
        ],
    );
    register_opcode_properties(&mut registry, "string", &[("length", OpcodeKind::StringLen)]);
    register_runtime_static_methods_explicit(
        &mut registry,
        "string",
        &[(
            "fromCharCode",
            BuiltinOp::Native(crate::compiler::native_id::OBJECT_STRING_FROM_CHAR_CODE),
            Some("string"),
        )],
    );
    register_exported_function(
        &mut registry,
        "parseInt",
        BuiltinOp::Native(builtin::number::PARSE_INT),
        Some("number"),
    );
    register_exported_function(
        &mut registry,
        "parseFloat",
        BuiltinOp::Native(builtin::number::PARSE_FLOAT),
        Some("number"),
    );
    register_exported_function(
        &mut registry,
        "encodeURI",
        BuiltinOp::Native(builtin::url::ENCODE),
        Some("string"),
    );
    register_exported_function(
        &mut registry,
        "decodeURI",
        BuiltinOp::Native(builtin::url::DECODE),
        Some("string"),
    );
    register_exported_function(
        &mut registry,
        "encodeURIComponent",
        BuiltinOp::Native(builtin::url::ENCODE),
        Some("string"),
    );
    register_exported_function(
        &mut registry,
        "decodeURIComponent",
        BuiltinOp::Native(builtin::url::DECODE),
        Some("string"),
    );
    register_exported_function(
        &mut registry,
        "escape",
        BuiltinOp::Native(builtin::url::ENCODE),
        Some("string"),
    );
    register_exported_function(
        &mut registry,
        "unescape",
        BuiltinOp::Native(builtin::url::DECODE),
        Some("string"),
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
    register_exported_function(
        &mut registry,
        "isNaN",
        BuiltinOp::Native(builtin::number::IS_NAN),
        Some("boolean"),
    );
    register_exported_function(
        &mut registry,
        "isFinite",
        BuiltinOp::Native(builtin::number::IS_FINITE),
        Some("boolean"),
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
    register_runtime_type(
        &mut registry,
        "Object",
        false,
        Some(BuiltinOp::Native(crate::compiler::native_id::OBJECT_NEW)),
        &["toString", "hashCode", "equals"],
    );
    register_runtime_methods_explicit(
        &mut registry,
        "Object",
        &[
            (
                "hasOwnProperty",
                BuiltinOp::Native(crate::compiler::native_id::OBJECT_HAS_OWN_PROPERTY),
                Some("boolean"),
            ),
            (
                "propertyIsEnumerable",
                BuiltinOp::Native(crate::compiler::native_id::OBJECT_PROPERTY_IS_ENUMERABLE),
                Some("boolean"),
            ),
        ],
    );
    register_runtime_static_methods(
        &mut registry,
        "Object",
        &[
            (
                "create",
                BuiltinOp::Native(crate::compiler::native_id::OBJECT_CREATE),
            ),
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
                "keys",
                BuiltinOp::Native(crate::compiler::native_id::OBJECT_KEYS),
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
    register_exported_class(&mut registry, "WeakMap", "WeakMap");
    register_runtime_type_explicit(
        &mut registry,
        "WeakMap",
        false,
        Some(BuiltinOp::Native(builtin::weak_map::NEW)),
        &[
            ("get", BuiltinOp::Native(builtin::weak_map::GET), None),
            (
                "set",
                BuiltinOp::Native(builtin::weak_map::SET),
                Some("WeakMap"),
            ),
            (
                "has",
                BuiltinOp::Native(builtin::weak_map::HAS),
                Some("boolean"),
            ),
            (
                "delete",
                BuiltinOp::Native(builtin::weak_map::DELETE),
                Some("boolean"),
            ),
        ],
        &[],
    );
    register_exported_class(&mut registry, "WeakSet", "WeakSet");
    register_runtime_type_explicit(
        &mut registry,
        "WeakSet",
        false,
        Some(BuiltinOp::Native(builtin::weak_set::NEW)),
        &[
            (
                "add",
                BuiltinOp::Native(builtin::weak_set::ADD),
                Some("WeakSet"),
            ),
            (
                "has",
                BuiltinOp::Native(builtin::weak_set::HAS),
                Some("boolean"),
            ),
            (
                "delete",
                BuiltinOp::Native(builtin::weak_set::DELETE),
                Some("boolean"),
            ),
        ],
        &[],
    );
    register_exported_class(&mut registry, "WeakRef", "WeakRef");
    register_runtime_type_explicit(
        &mut registry,
        "WeakRef",
        false,
        Some(BuiltinOp::Native(builtin::weak_ref::NEW)),
        &[("deref", BuiltinOp::Native(builtin::weak_ref::DEREF), Some("Object"))],
        &[],
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
    register_exported_function(
        &mut registry,
        "createRangeError",
        BuiltinOp::Native(builtin::error::CREATE_RANGE_ERROR),
        Some("RangeError"),
    );
    register_exported_constant_string(&mut registry, "ERR_OUT_OF_RANGE", "ERR_OUT_OF_RANGE");
    register_exported_constant_string(
        &mut registry,
        "E_UNIMPLEMENTED_BUILTIN_BEHAVIOR",
        "E_UNIMPLEMENTED_BUILTIN_BEHAVIOR",
    );
    register_runtime_type_explicit(
        &mut registry,
        "ArrayBuffer",
        false,
        Some(BuiltinOp::Native(builtin::array_buffer::NEW)),
        &[(
            "slice",
            BuiltinOp::Native(builtin::array_buffer::SLICE),
            Some("ArrayBuffer"),
        )],
        &[
            (
                "byteLength",
                BuiltinOp::Native(builtin::array_buffer::BYTE_LENGTH),
                Some("int"),
            ),
        ],
    );
    register_runtime_type_explicit(
        &mut registry,
        "SharedArrayBuffer",
        false,
        Some(BuiltinOp::Native(builtin::array_buffer::SHARED_NEW)),
        &[],
        &[
            (
                "byteLength",
                BuiltinOp::Native(builtin::array_buffer::BYTE_LENGTH),
                Some("int"),
            ),
        ],
    );
    register_runtime_type_explicit(
        &mut registry,
        "DataView",
        false,
        Some(BuiltinOp::Native(builtin::data_view::NEW)),
        &[
            (
                "getUint8",
                BuiltinOp::Native(builtin::data_view::GET_UINT8),
                Some("int"),
            ),
            (
                "setUint8",
                BuiltinOp::Native(builtin::data_view::SET_UINT8),
                Some("void"),
            ),
            (
                "getInt8",
                BuiltinOp::Native(builtin::data_view::GET_INT8),
                Some("int"),
            ),
            (
                "setInt8",
                BuiltinOp::Native(builtin::data_view::SET_INT8),
                Some("void"),
            ),
            (
                "getInt32",
                BuiltinOp::Native(builtin::data_view::GET_INT32),
                Some("int"),
            ),
            (
                "setInt32",
                BuiltinOp::Native(builtin::data_view::SET_INT32),
                Some("void"),
            ),
            (
                "getUint32",
                BuiltinOp::Native(builtin::data_view::GET_UINT32),
                Some("int"),
            ),
            (
                "setUint32",
                BuiltinOp::Native(builtin::data_view::SET_UINT32),
                Some("void"),
            ),
            (
                "getFloat64",
                BuiltinOp::Native(builtin::data_view::GET_FLOAT64),
                Some("number"),
            ),
            (
                "setFloat64",
                BuiltinOp::Native(builtin::data_view::SET_FLOAT64),
                Some("void"),
            ),
        ],
        &[
            (
                "buffer",
                BuiltinOp::Native(builtin::data_view::BUFFER),
                Some("ArrayBuffer"),
            ),
            (
                "byteLength",
                BuiltinOp::Native(builtin::data_view::BYTE_LENGTH),
                Some("int"),
            ),
            (
                "byteOffset",
                BuiltinOp::Native(builtin::data_view::BYTE_OFFSET),
                Some("int"),
            ),
        ],
    );
    register_runtime_type_explicit(
        &mut registry,
        "TypedArray",
        false,
        Some(BuiltinOp::Native(builtin::typed_array::GENERIC_NEW)),
        &[
            ("get", BuiltinOp::Native(builtin::typed_array::GET), Some("int")),
            ("set", BuiltinOp::Native(builtin::typed_array::SET), Some("void")),
        ],
        &[
            (
                "buffer",
                BuiltinOp::Native(builtin::typed_array::BUFFER),
                Some("ArrayBuffer"),
            ),
            (
                "byteLength",
                BuiltinOp::Native(builtin::typed_array::BYTE_LENGTH),
                Some("int"),
            ),
            (
                "byteOffset",
                BuiltinOp::Native(builtin::typed_array::BYTE_OFFSET),
                Some("int"),
            ),
            (
                "length",
                BuiltinOp::Native(builtin::typed_array::LENGTH),
                Some("int"),
            ),
        ],
    );
    register_runtime_type_explicit(
        &mut registry,
        "Uint8Array",
        false,
        Some(BuiltinOp::Native(builtin::typed_array::UINT8_NEW)),
        &[
            ("get", BuiltinOp::Native(builtin::typed_array::GET), Some("int")),
            ("set", BuiltinOp::Native(builtin::typed_array::SET), Some("void")),
        ],
        &[
            (
                "buffer",
                BuiltinOp::Native(builtin::typed_array::BUFFER),
                Some("ArrayBuffer"),
            ),
            (
                "byteLength",
                BuiltinOp::Native(builtin::typed_array::BYTE_LENGTH),
                Some("int"),
            ),
            (
                "byteOffset",
                BuiltinOp::Native(builtin::typed_array::BYTE_OFFSET),
                Some("int"),
            ),
            (
                "length",
                BuiltinOp::Native(builtin::typed_array::LENGTH),
                Some("int"),
            ),
        ],
    );
    register_runtime_type_explicit(
        &mut registry,
        "Uint8ClampedArray",
        false,
        Some(BuiltinOp::Native(builtin::typed_array::UINT8_CLAMPED_NEW)),
        &[
            ("get", BuiltinOp::Native(builtin::typed_array::GET), Some("int")),
            ("set", BuiltinOp::Native(builtin::typed_array::SET), Some("void")),
        ],
        &[
            (
                "buffer",
                BuiltinOp::Native(builtin::typed_array::BUFFER),
                Some("ArrayBuffer"),
            ),
            (
                "byteLength",
                BuiltinOp::Native(builtin::typed_array::BYTE_LENGTH),
                Some("int"),
            ),
            (
                "byteOffset",
                BuiltinOp::Native(builtin::typed_array::BYTE_OFFSET),
                Some("int"),
            ),
            (
                "length",
                BuiltinOp::Native(builtin::typed_array::LENGTH),
                Some("int"),
            ),
        ],
    );
    register_runtime_type_explicit(
        &mut registry,
        "Int8Array",
        false,
        Some(BuiltinOp::Native(builtin::typed_array::INT8_NEW)),
        &[
            ("get", BuiltinOp::Native(builtin::typed_array::GET), Some("int")),
            ("set", BuiltinOp::Native(builtin::typed_array::SET), Some("void")),
        ],
        &[
            (
                "buffer",
                BuiltinOp::Native(builtin::typed_array::BUFFER),
                Some("ArrayBuffer"),
            ),
            (
                "byteLength",
                BuiltinOp::Native(builtin::typed_array::BYTE_LENGTH),
                Some("int"),
            ),
            (
                "byteOffset",
                BuiltinOp::Native(builtin::typed_array::BYTE_OFFSET),
                Some("int"),
            ),
            (
                "length",
                BuiltinOp::Native(builtin::typed_array::LENGTH),
                Some("int"),
            ),
        ],
    );
    register_runtime_type_explicit(
        &mut registry,
        "Uint16Array",
        false,
        Some(BuiltinOp::Native(builtin::typed_array::UINT16_NEW)),
        &[
            ("get", BuiltinOp::Native(builtin::typed_array::GET), Some("int")),
            ("set", BuiltinOp::Native(builtin::typed_array::SET), Some("void")),
        ],
        &[
            (
                "buffer",
                BuiltinOp::Native(builtin::typed_array::BUFFER),
                Some("ArrayBuffer"),
            ),
            (
                "byteLength",
                BuiltinOp::Native(builtin::typed_array::BYTE_LENGTH),
                Some("int"),
            ),
            (
                "byteOffset",
                BuiltinOp::Native(builtin::typed_array::BYTE_OFFSET),
                Some("int"),
            ),
            (
                "length",
                BuiltinOp::Native(builtin::typed_array::LENGTH),
                Some("int"),
            ),
        ],
    );
    register_runtime_type_explicit(
        &mut registry,
        "Int16Array",
        false,
        Some(BuiltinOp::Native(builtin::typed_array::INT16_NEW)),
        &[
            ("get", BuiltinOp::Native(builtin::typed_array::GET), Some("int")),
            ("set", BuiltinOp::Native(builtin::typed_array::SET), Some("void")),
        ],
        &[
            (
                "buffer",
                BuiltinOp::Native(builtin::typed_array::BUFFER),
                Some("ArrayBuffer"),
            ),
            (
                "byteLength",
                BuiltinOp::Native(builtin::typed_array::BYTE_LENGTH),
                Some("int"),
            ),
            (
                "byteOffset",
                BuiltinOp::Native(builtin::typed_array::BYTE_OFFSET),
                Some("int"),
            ),
            (
                "length",
                BuiltinOp::Native(builtin::typed_array::LENGTH),
                Some("int"),
            ),
        ],
    );
    register_runtime_type_explicit(
        &mut registry,
        "Int32Array",
        false,
        Some(BuiltinOp::Native(builtin::typed_array::INT32_NEW)),
        &[
            ("get", BuiltinOp::Native(builtin::typed_array::GET), Some("int")),
            ("set", BuiltinOp::Native(builtin::typed_array::SET), Some("void")),
        ],
        &[
            (
                "buffer",
                BuiltinOp::Native(builtin::typed_array::BUFFER),
                Some("ArrayBuffer"),
            ),
            (
                "byteLength",
                BuiltinOp::Native(builtin::typed_array::BYTE_LENGTH),
                Some("int"),
            ),
            (
                "byteOffset",
                BuiltinOp::Native(builtin::typed_array::BYTE_OFFSET),
                Some("int"),
            ),
            (
                "length",
                BuiltinOp::Native(builtin::typed_array::LENGTH),
                Some("int"),
            ),
        ],
    );
    register_runtime_type_explicit(
        &mut registry,
        "Uint32Array",
        false,
        Some(BuiltinOp::Native(builtin::typed_array::UINT32_NEW)),
        &[
            (
                "get",
                BuiltinOp::Native(builtin::typed_array::GET),
                Some("number"),
            ),
            ("set", BuiltinOp::Native(builtin::typed_array::SET), Some("void")),
        ],
        &[
            (
                "buffer",
                BuiltinOp::Native(builtin::typed_array::BUFFER),
                Some("ArrayBuffer"),
            ),
            (
                "byteLength",
                BuiltinOp::Native(builtin::typed_array::BYTE_LENGTH),
                Some("int"),
            ),
            (
                "byteOffset",
                BuiltinOp::Native(builtin::typed_array::BYTE_OFFSET),
                Some("int"),
            ),
            (
                "length",
                BuiltinOp::Native(builtin::typed_array::LENGTH),
                Some("int"),
            ),
        ],
    );
    register_runtime_type_explicit(
        &mut registry,
        "Float32Array",
        false,
        Some(BuiltinOp::Native(builtin::typed_array::FLOAT32_NEW)),
        &[
            (
                "get",
                BuiltinOp::Native(builtin::typed_array::GET),
                Some("number"),
            ),
            ("set", BuiltinOp::Native(builtin::typed_array::SET), Some("void")),
        ],
        &[
            (
                "buffer",
                BuiltinOp::Native(builtin::typed_array::BUFFER),
                Some("ArrayBuffer"),
            ),
            (
                "byteLength",
                BuiltinOp::Native(builtin::typed_array::BYTE_LENGTH),
                Some("int"),
            ),
            (
                "byteOffset",
                BuiltinOp::Native(builtin::typed_array::BYTE_OFFSET),
                Some("int"),
            ),
            (
                "length",
                BuiltinOp::Native(builtin::typed_array::LENGTH),
                Some("int"),
            ),
        ],
    );
    register_runtime_type_explicit(
        &mut registry,
        "Float16Array",
        false,
        Some(BuiltinOp::Native(builtin::typed_array::FLOAT16_NEW)),
        &[
            (
                "get",
                BuiltinOp::Native(builtin::typed_array::GET),
                Some("number"),
            ),
            ("set", BuiltinOp::Native(builtin::typed_array::SET), Some("void")),
        ],
        &[
            (
                "buffer",
                BuiltinOp::Native(builtin::typed_array::BUFFER),
                Some("ArrayBuffer"),
            ),
            (
                "byteLength",
                BuiltinOp::Native(builtin::typed_array::BYTE_LENGTH),
                Some("int"),
            ),
            (
                "byteOffset",
                BuiltinOp::Native(builtin::typed_array::BYTE_OFFSET),
                Some("int"),
            ),
            (
                "length",
                BuiltinOp::Native(builtin::typed_array::LENGTH),
                Some("int"),
            ),
        ],
    );
    register_runtime_type_explicit(
        &mut registry,
        "Float64Array",
        false,
        Some(BuiltinOp::Native(builtin::typed_array::FLOAT64_NEW)),
        &[
            (
                "get",
                BuiltinOp::Native(builtin::typed_array::GET),
                Some("number"),
            ),
            ("set", BuiltinOp::Native(builtin::typed_array::SET), Some("void")),
        ],
        &[
            (
                "buffer",
                BuiltinOp::Native(builtin::typed_array::BUFFER),
                Some("ArrayBuffer"),
            ),
            (
                "byteLength",
                BuiltinOp::Native(builtin::typed_array::BYTE_LENGTH),
                Some("int"),
            ),
            (
                "byteOffset",
                BuiltinOp::Native(builtin::typed_array::BYTE_OFFSET),
                Some("int"),
            ),
            (
                "length",
                BuiltinOp::Native(builtin::typed_array::LENGTH),
                Some("int"),
            ),
        ],
    );
    register_runtime_type_explicit(
        &mut registry,
        "BigInt64Array",
        false,
        Some(BuiltinOp::Native(builtin::typed_array::BIGINT64_NEW)),
        &[
            (
                "get",
                BuiltinOp::Native(builtin::typed_array::GET),
                Some("number"),
            ),
            ("set", BuiltinOp::Native(builtin::typed_array::SET), Some("void")),
        ],
        &[
            (
                "buffer",
                BuiltinOp::Native(builtin::typed_array::BUFFER),
                Some("ArrayBuffer"),
            ),
            (
                "byteLength",
                BuiltinOp::Native(builtin::typed_array::BYTE_LENGTH),
                Some("int"),
            ),
            (
                "byteOffset",
                BuiltinOp::Native(builtin::typed_array::BYTE_OFFSET),
                Some("int"),
            ),
            (
                "length",
                BuiltinOp::Native(builtin::typed_array::LENGTH),
                Some("int"),
            ),
        ],
    );
    register_runtime_type_explicit(
        &mut registry,
        "BigUint64Array",
        false,
        Some(BuiltinOp::Native(builtin::typed_array::BIGUINT64_NEW)),
        &[
            (
                "get",
                BuiltinOp::Native(builtin::typed_array::GET),
                Some("number"),
            ),
            ("set", BuiltinOp::Native(builtin::typed_array::SET), Some("void")),
        ],
        &[
            (
                "buffer",
                BuiltinOp::Native(builtin::typed_array::BUFFER),
                Some("ArrayBuffer"),
            ),
            (
                "byteLength",
                BuiltinOp::Native(builtin::typed_array::BYTE_LENGTH),
                Some("int"),
            ),
            (
                "byteOffset",
                BuiltinOp::Native(builtin::typed_array::BYTE_OFFSET),
                Some("int"),
            ),
            (
                "length",
                BuiltinOp::Native(builtin::typed_array::LENGTH),
                Some("int"),
            ),
        ],
    );
    for typed_array_name in [
        "ArrayBuffer",
        "DataView",
        "TypedArray",
        "Uint8Array",
        "Uint8ClampedArray",
        "Int8Array",
        "Int16Array",
        "Uint16Array",
        "Int32Array",
        "Uint32Array",
        "Float32Array",
        "Float16Array",
        "Float64Array",
        "BigInt64Array",
        "BigUint64Array",
        "SharedArrayBuffer",
    ] {
        register_exported_class(&mut registry, typed_array_name, typed_array_name);
    }
    register_exported_class(&mut registry, "Atomics", "Atomics");
    register_runtime_static_methods_explicit(
        &mut registry,
        "Atomics",
        &[
            ("load", BuiltinOp::Native(builtin::atomics::LOAD), Some("number")),
            ("store", BuiltinOp::Native(builtin::atomics::STORE), Some("number")),
            ("add", BuiltinOp::Native(builtin::atomics::ADD), Some("number")),
            (
                "compareExchange",
                BuiltinOp::Native(builtin::atomics::COMPARE_EXCHANGE),
                Some("number"),
            ),
            (
                "wait",
                BuiltinOp::Native(builtin::atomics::WAIT),
                Some("string"),
            ),
        ],
    );
    register_exported_class(&mut registry, "Date", "Date");
    register_runtime_type(
        &mut registry,
        "Date",
        false,
        Some(BuiltinOp::Native(builtin::date::CONSTRUCT)),
        &[
            "getTime",
            "getFullYear",
            "getMonth",
            "getDate",
            "getDay",
            "getHours",
            "getMinutes",
            "getSeconds",
            "getMilliseconds",
            "getTimezoneOffset",
            "setTime",
            "setFullYear",
            "setMonth",
            "setDate",
            "setHours",
            "setMinutes",
            "setSeconds",
            "setMilliseconds",
            "toString",
            "toISOString",
            "toDateString",
            "toTimeString",
        ],
    );
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
    register_exported_class(&mut registry, "Iterator", "Iterator");
    register_runtime_static_methods_explicit(
        &mut registry,
        "Iterator",
        &[(
            "fromArray",
            BuiltinOp::Native(crate::compiler::native_id::ITERATOR_FROM_ARRAY),
            Some("Iterator"),
        )],
    );
    register_runtime_methods_explicit(
        &mut registry,
        "Iterator",
        &[(
            "toArray",
            BuiltinOp::Native(crate::compiler::native_id::ITERATOR_TO_ARRAY),
            Some("Array"),
        )],
    );
    register_exported_nominal_class(&mut registry, "InternalError", "InternalError");
    register_exported_class(&mut registry, "Reflect", "Reflect");
    register_exported_class(&mut registry, "Intl", "Intl");
    register_exported_class(&mut registry, "Temporal", "Temporal");
    register_runtime_static_methods_explicit(
        &mut registry,
        "Temporal",
        &[
            (
                "Instant",
                BuiltinOp::Native(crate::compiler::native_id::TEMPORAL_INSTANT),
                Some("TemporalInstant"),
            ),
            (
                "PlainDate",
                BuiltinOp::Native(crate::compiler::native_id::TEMPORAL_PLAIN_DATE),
                Some("TemporalPlainDate"),
            ),
            (
                "PlainTime",
                BuiltinOp::Native(crate::compiler::native_id::TEMPORAL_PLAIN_TIME),
                Some("TemporalPlainTime"),
            ),
            (
                "ZonedDateTime",
                BuiltinOp::Native(crate::compiler::native_id::TEMPORAL_ZONED_DATE_TIME),
                Some("TemporalZonedDateTime"),
            ),
        ],
    );
    register_runtime_type_explicit(
        &mut registry,
        "TemporalInstant",
        false,
        None,
        &[(
            "toString",
            BuiltinOp::Native(crate::compiler::native_id::TEMPORAL_INSTANT_TO_STRING),
            Some("string"),
        )],
        &[],
    );
    register_runtime_type_explicit(
        &mut registry,
        "TemporalPlainDate",
        false,
        None,
        &[(
            "toString",
            BuiltinOp::Native(crate::compiler::native_id::TEMPORAL_PLAIN_DATE_TO_STRING),
            Some("string"),
        )],
        &[],
    );
    register_runtime_type_explicit(
        &mut registry,
        "TemporalPlainTime",
        false,
        None,
        &[(
            "toString",
            BuiltinOp::Native(crate::compiler::native_id::TEMPORAL_PLAIN_TIME_TO_STRING),
            Some("string"),
        )],
        &[],
    );
    register_runtime_type_explicit(
        &mut registry,
        "TemporalZonedDateTime",
        false,
        None,
        &[(
            "toString",
            BuiltinOp::Native(crate::compiler::native_id::TEMPORAL_ZONED_DATE_TIME_TO_STRING),
            Some("string"),
        )],
        &[],
    );
    register_runtime_static_methods_explicit(
        &mut registry,
        "Intl",
        &[
            (
                "NumberFormat",
                BuiltinOp::Native(crate::compiler::native_id::INTL_NUMBER_FORMAT),
                Some("Object"),
            ),
            (
                "DateTimeFormat",
                BuiltinOp::Native(crate::compiler::native_id::INTL_DATE_TIME_FORMAT),
                Some("Object"),
            ),
        ],
    );
    register_runtime_static_methods_explicit(
        &mut registry,
        "Reflect",
        &[
            (
                "apply",
                BuiltinOp::Native(crate::compiler::native_id::FUNCTION_APPLY_HELPER),
                Some("unknown"),
            ),
            (
                "get",
                BuiltinOp::Native(crate::compiler::native_id::REFLECT_GET),
                Some("unknown"),
            ),
            (
                "set",
                BuiltinOp::Native(crate::compiler::native_id::REFLECT_SET),
                Some("boolean"),
            ),
            (
                "has",
                BuiltinOp::Native(crate::compiler::native_id::REFLECT_HAS),
                Some("boolean"),
            ),
            (
                "getFieldNames",
                BuiltinOp::Native(crate::compiler::native_id::REFLECT_GET_FIELD_NAMES),
                Some("Array"),
            ),
            (
                "getFieldInfo",
                BuiltinOp::Native(crate::compiler::native_id::REFLECT_GET_FIELD_INFO),
                Some("Object"),
            ),
            (
                "hasMethod",
                BuiltinOp::Native(crate::compiler::native_id::REFLECT_HAS_METHOD),
                Some("boolean"),
            ),
            (
                "isProxy",
                BuiltinOp::Native(builtin::reflect::IS_PROXY),
                Some("boolean"),
            ),
            (
                "getProxyTarget",
                BuiltinOp::Native(builtin::reflect::GET_PROXY_TARGET),
                Some("Object"),
            ),
            (
                "getProxyHandler",
                BuiltinOp::Native(builtin::reflect::GET_PROXY_HANDLER),
                Some("Object"),
            ),
            (
                "revokeProxy",
                BuiltinOp::Native(builtin::reflect::REVOKE_PROXY),
                Some("void"),
            ),
        ],
    );
    register_exported_class(&mut registry, "Proxy", "Proxy");
    register_exported_class(&mut registry, "Symbol", "Symbol");
    for class_name in [
        "AsyncIterator",
        "Generator",
        "AsyncGenerator",
        "GeneratorFunction",
        "AsyncGeneratorFunction",
        "AsyncFunction",
    ] {
        register_exported_nominal_class(&mut registry, class_name, class_name);
    }
    register_exported_nominal_class(
        &mut registry,
        "FinalizationRegistry",
        "FinalizationRegistry",
    );
    register_runtime_type_explicit(
        &mut registry,
        "FinalizationRegistry",
        false,
        Some(BuiltinOp::Native(
            crate::compiler::native_id::FINALIZATION_REGISTRY_NEW,
        )),
        &[
            (
                "register",
                BuiltinOp::Native(crate::compiler::native_id::FINALIZATION_REGISTRY_REGISTER),
                Some("void"),
            ),
            (
                "unregister",
                BuiltinOp::Native(crate::compiler::native_id::FINALIZATION_REGISTRY_UNREGISTER),
                Some("boolean"),
            ),
            (
                "cleanupSome",
                BuiltinOp::Native(crate::compiler::native_id::FINALIZATION_REGISTRY_CLEANUP_SOME),
                Some("void"),
            ),
        ],
        &[],
    );
    register_exported_nominal_class(&mut registry, "DisposableStack", "DisposableStack");
    register_runtime_type_explicit(
        &mut registry,
        "DisposableStack",
        false,
        Some(BuiltinOp::Native(
            crate::compiler::native_id::DISPOSABLE_STACK_NEW,
        )),
        &[
            (
                "defer",
                BuiltinOp::Native(crate::compiler::native_id::DISPOSABLE_STACK_DEFER),
                Some("DisposableStack"),
            ),
            (
                "dispose",
                BuiltinOp::Native(crate::compiler::native_id::DISPOSABLE_STACK_DISPOSE),
                Some("void"),
            ),
            (
                "move",
                BuiltinOp::Native(crate::compiler::native_id::DISPOSABLE_STACK_MOVE),
                Some("DisposableStack"),
            ),
        ],
        &[],
    );
    register_exported_nominal_class(
        &mut registry,
        "AsyncDisposableStack",
        "AsyncDisposableStack",
    );
    register_runtime_type_explicit(
        &mut registry,
        "AsyncDisposableStack",
        false,
        Some(BuiltinOp::Native(
            crate::compiler::native_id::ASYNC_DISPOSABLE_STACK_NEW,
        )),
        &[
            (
                "defer",
                BuiltinOp::Native(crate::compiler::native_id::ASYNC_DISPOSABLE_STACK_DEFER),
                Some("AsyncDisposableStack"),
            ),
            (
                "disposeAsync",
                BuiltinOp::Native(crate::compiler::native_id::ASYNC_DISPOSABLE_STACK_DISPOSE_ASYNC),
                Some("Promise"),
            ),
        ],
        &[],
    );
    {
        let descriptor = registry.types.entry("Symbol").or_default();
        descriptor.static_properties.insert(
            "iterator",
            BuiltinSurfaceMemberDescriptor::Bound(BuiltinBindingDescriptor {
                op: BuiltinOp::Native(crate::compiler::native_id::SYMBOL_ITERATOR),
                return_type_name: Some("Symbol"),
            }),
        );
        descriptor.static_properties.insert(
            "toStringTag",
            BuiltinSurfaceMemberDescriptor::Bound(BuiltinBindingDescriptor {
                op: BuiltinOp::Native(crate::compiler::native_id::SYMBOL_TO_STRING_TAG),
                return_type_name: Some("Symbol"),
            }),
        );
    }
    register_runtime_methods_explicit(
        &mut registry,
        "Symbol",
        &[
            (
                "toString",
                BuiltinOp::Native(crate::compiler::native_id::SYMBOL_TO_STRING),
                Some("string"),
            ),
            (
                "valueOf",
                BuiltinOp::Native(crate::compiler::native_id::SYMBOL_VALUE_OF),
                Some("string"),
            ),
        ],
    );
    register_runtime_static_methods_explicit(
        &mut registry,
        "Symbol",
        &[
            (
                "iterator",
                BuiltinOp::Native(crate::compiler::native_id::SYMBOL_ITERATOR),
                Some("Symbol"),
            ),
            (
                "toStringTag",
                BuiltinOp::Native(crate::compiler::native_id::SYMBOL_TO_STRING_TAG),
                Some("Symbol"),
            ),
            (
                "for",
                BuiltinOp::Native(crate::compiler::native_id::SYMBOL_FOR),
                Some("Symbol"),
            ),
            (
                "keyFor",
                BuiltinOp::Native(crate::compiler::native_id::SYMBOL_KEY_FOR),
                Some("string"),
            ),
        ],
    );
    register_exported_class(&mut registry, "Boolean", "boolean");
    register_exported_class(&mut registry, "Error", "Error");
    register_exported_class(&mut registry, "AggregateError", "AggregateError");
    register_exported_class(&mut registry, "SuppressedError", "SuppressedError");
    register_exported_class(&mut registry, "TypeError", "TypeError");
    register_exported_class(&mut registry, "RangeError", "RangeError");
    register_exported_class(&mut registry, "ReferenceError", "ReferenceError");
    register_exported_class(&mut registry, "SyntaxError", "SyntaxError");
    register_exported_class(&mut registry, "URIError", "URIError");
    register_exported_class(&mut registry, "EvalError", "EvalError");
    for error_type_name in [
        "Error",
        "TypeError",
        "RangeError",
        "ReferenceError",
        "SyntaxError",
        "URIError",
        "EvalError",
        "InternalError",
        "AggregateError",
        "SuppressedError",
    ] {
        register_runtime_type_explicit(
            &mut registry,
            error_type_name,
            false,
            None,
            &[(
                "toString",
                BuiltinOp::Native(crate::compiler::native_id::ERROR_TO_STRING),
                Some("string"),
            )],
            &[(
                "stack",
                BuiltinOp::Native(crate::compiler::native_id::ERROR_STACK),
                Some("string"),
            )],
        );
    }
    register_exported_class(&mut registry, "Function", "Function");
    register_runtime_type(
        &mut registry,
        "Function",
        false,
        Some(BuiltinOp::Native(
            crate::compiler::native_id::FUNCTION_CONSTRUCTOR_HELPER,
        )),
        &[],
    );
    register_runtime_methods_explicit(
        &mut registry,
        "Function",
        &[
            (
                "call",
                BuiltinOp::Native(crate::compiler::native_id::FUNCTION_CALL_HELPER),
                Some("unknown"),
            ),
            (
                "apply",
                BuiltinOp::Native(crate::compiler::native_id::FUNCTION_APPLY_HELPER),
                Some("unknown"),
            ),
            (
                "bind",
                BuiltinOp::Native(crate::compiler::native_id::FUNCTION_BIND_HELPER),
                Some("Function"),
            ),
        ],
    );
    register_runtime_type(
        &mut registry,
        "Proxy",
        false,
        Some(BuiltinOp::Native(builtin::reflect::CREATE_PROXY)),
        &[],
    );
    register_runtime_methods_explicit(
        &mut registry,
        "Proxy",
        &[
            (
                "isProxy",
                BuiltinOp::Native(builtin::reflect::IS_PROXY),
                Some("boolean"),
            ),
            (
                "getTarget",
                BuiltinOp::Native(builtin::reflect::GET_PROXY_TARGET),
                Some("Object"),
            ),
            (
                "getHandler",
                BuiltinOp::Native(builtin::reflect::GET_PROXY_HANDLER),
                Some("Object"),
            ),
            (
                "revoke",
                BuiltinOp::Native(builtin::reflect::REVOKE_PROXY),
                Some("void"),
            ),
        ],
    );
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
    register_runtime_methods_explicit(
        &mut registry,
        "EventEmitter",
        &[
            (
                "on",
                BuiltinOp::Native(builtin::event_emitter::ON),
                Some("EventEmitter"),
            ),
            (
                "once",
                BuiltinOp::Native(builtin::event_emitter::ONCE),
                Some("EventEmitter"),
            ),
            (
                "off",
                BuiltinOp::Native(builtin::event_emitter::OFF),
                Some("EventEmitter"),
            ),
            (
                "addListener",
                BuiltinOp::Native(builtin::event_emitter::ADD_LISTENER),
                Some("EventEmitter"),
            ),
            (
                "removeListener",
                BuiltinOp::Native(builtin::event_emitter::REMOVE_LISTENER),
                Some("EventEmitter"),
            ),
            (
                "emit",
                BuiltinOp::Native(builtin::event_emitter::EMIT),
                Some("boolean"),
            ),
            (
                "listeners",
                BuiltinOp::Native(builtin::event_emitter::LISTENERS),
                Some("Array"),
            ),
            (
                "listenerCount",
                BuiltinOp::Native(builtin::event_emitter::LISTENER_COUNT),
                Some("number"),
            ),
            (
                "eventNames",
                BuiltinOp::Native(builtin::event_emitter::EVENT_NAMES),
                Some("Array"),
            ),
            (
                "setMaxListeners",
                BuiltinOp::Native(builtin::event_emitter::SET_MAX_LISTENERS),
                Some("EventEmitter"),
            ),
            (
                "getMaxListeners",
                BuiltinOp::Native(builtin::event_emitter::GET_MAX_LISTENERS),
                Some("number"),
            ),
            (
                "removeAllListeners",
                BuiltinOp::Native(builtin::event_emitter::REMOVE_ALL_LISTENERS),
                Some("EventEmitter"),
            ),
        ],
    );

    seed_surface_only_entries_from_signatures(&mut registry);

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
            binding: None,
            literal: None,
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
            binding: None,
            literal: None,
        },
    );
}

fn register_exported_function(
    registry: &mut BuiltinRegistry,
    global_name: &'static str,
    op: BuiltinOp,
    return_type_name: Option<&'static str>,
) {
    registry.globals.insert(
        global_name,
        BuiltinGlobalDescriptor {
            global_name,
            backing_type_name: "Function",
            symbol_type: SymbolType::Function,
            binding: Some(BuiltinBindingDescriptor {
                op,
                return_type_name,
            }),
            literal: None,
        },
    );
}

fn register_exported_constant_string(
    registry: &mut BuiltinRegistry,
    global_name: &'static str,
    value: &'static str,
) {
    registry.globals.insert(
        global_name,
        BuiltinGlobalDescriptor {
            global_name,
            backing_type_name: "string",
            symbol_type: SymbolType::Constant,
            binding: None,
            literal: Some(BuiltinLiteral::String(value)),
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
    descriptor.constructor = constructor.map(|op| {
        BuiltinSurfaceMemberDescriptor::Bound(BuiltinBindingDescriptor {
            op,
            return_type_name: None,
        })
    });
    for &method_name in methods {
        let native_id = builtin::lookup_builtin_method(type_name, method_name)
            .unwrap_or_else(|| panic!("missing runtime builtin method: {type_name}.{method_name}"));
        let op = builtin_op_from_native_id(native_id).unwrap_or(BuiltinOp::Native(native_id));
        descriptor.instance_methods.insert(
            method_name,
            BuiltinSurfaceMemberDescriptor::Bound(BuiltinBindingDescriptor {
                op,
                return_type_name: builtin_member_return_type(type_name, method_name, false),
            }),
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
    descriptor.constructor = Some(BuiltinSurfaceMemberDescriptor::Bound(BuiltinBindingDescriptor {
        op: constructor,
        return_type_name: None,
    }));
    for &(method_name, op) in methods {
        descriptor.instance_methods.insert(
            method_name,
            BuiltinSurfaceMemberDescriptor::Bound(BuiltinBindingDescriptor {
                op,
                return_type_name: builtin_member_return_type(type_name, method_name, false),
            }),
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
            BuiltinSurfaceMemberDescriptor::Bound(BuiltinBindingDescriptor {
                op,
                return_type_name: builtin_member_return_type(type_name, property_name, false),
            }),
        );
    }
}

fn register_opcode_properties(
    registry: &mut BuiltinRegistry,
    type_name: &'static str,
    properties: &[(&'static str, OpcodeKind)],
) {
    let descriptor = registry.types.entry(type_name).or_default();
    for &(property_name, opcode) in properties {
        descriptor
            .instance_properties
            .insert(property_name, BuiltinSurfaceMemberDescriptor::Opcode(opcode));
    }
}

fn register_runtime_type_explicit(
    registry: &mut BuiltinRegistry,
    type_name: &'static str,
    builtin_primitive: bool,
    constructor: Option<BuiltinOp>,
    methods: &[(&'static str, BuiltinOp, Option<&'static str>)],
    properties: &[(&'static str, BuiltinOp, Option<&'static str>)],
) {
    let descriptor = registry.types.entry(type_name).or_default();
    descriptor.builtin_primitive = builtin_primitive;
    descriptor.constructor = constructor.map(|op| {
        BuiltinSurfaceMemberDescriptor::Bound(BuiltinBindingDescriptor {
            op,
            return_type_name: None,
        })
    });
    for &(method_name, op, return_type_name) in methods {
        descriptor.instance_methods.insert(
            method_name,
            BuiltinSurfaceMemberDescriptor::Bound(BuiltinBindingDescriptor {
                op,
                return_type_name,
            }),
        );
    }
    for &(property_name, op, return_type_name) in properties {
        descriptor.instance_properties.insert(
            property_name,
            BuiltinSurfaceMemberDescriptor::Bound(BuiltinBindingDescriptor {
                op,
                return_type_name,
            }),
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
            BuiltinSurfaceMemberDescriptor::Bound(BuiltinBindingDescriptor {
                op,
                return_type_name,
            }),
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
            BuiltinSurfaceMemberDescriptor::Bound(BuiltinBindingDescriptor {
                op,
                return_type_name: builtin_member_return_type(type_name, method_name, true),
            }),
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
            BuiltinSurfaceMemberDescriptor::Bound(BuiltinBindingDescriptor {
                op,
                return_type_name,
            }),
        );
    }
}

fn seed_surface_only_entries_from_signatures(registry: &mut BuiltinRegistry) {
    for signatures in builtins::get_all_signatures() {
        for class in signatures.classes {
            if class.name.starts_with("__") {
                continue;
            }

            let descriptor = registry.types.entry(class.name).or_default();
            if descriptor.constructor.is_none() && class.constructor.is_some() {
                descriptor.constructor = Some(BuiltinSurfaceMemberDescriptor::SurfaceOnly);
            }

            for property in class.properties {
                let target = if property.is_static {
                    &mut descriptor.static_properties
                } else {
                    &mut descriptor.instance_properties
                };
                target
                    .entry(property.name)
                    .or_insert(BuiltinSurfaceMemberDescriptor::SurfaceOnly);
            }

            for method in class.methods {
                let target = if method.is_static {
                    &mut descriptor.static_methods
                } else {
                    &mut descriptor.instance_methods
                };
                target
                    .entry(method.name)
                    .or_insert(BuiltinSurfaceMemberDescriptor::SurfaceOnly);
            }

            registry.globals.entry(class.name).or_insert(BuiltinGlobalDescriptor {
                global_name: class.name,
                backing_type_name: class.name,
                symbol_type: if class.constructor.is_some() {
                    SymbolType::Class
                } else {
                    SymbolType::Constant
                },
                binding: None,
                literal: None,
            });
        }

        for function in signatures.functions {
            registry
                .globals
                .entry(function.name)
                .or_insert(BuiltinGlobalDescriptor {
                    global_name: function.name,
                    backing_type_name: "Function",
                    symbol_type: SymbolType::Function,
                    binding: None,
                    literal: None,
                });
        }
    }
}

fn builtin_member_return_type(
    type_name: &str,
    member_name: &str,
    is_static: bool,
) -> Option<&'static str> {
    for signatures in builtins::get_all_signatures() {
        let Some(class) = signatures
            .classes
            .iter()
            .find(|class| class.name.eq_ignore_ascii_case(type_name))
        else {
            continue;
        };
        if let Some(method) = class
            .methods
            .iter()
            .find(|method| method.name == member_name && method.is_static == is_static)
        {
            return Some(method.return_type);
        }
        if let Some(property) = class
            .properties
            .iter()
            .find(|property| property.name == member_name && property.is_static == is_static)
        {
            return Some(property.ty);
        }
    }
    None
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

fn collect_builtin_native_ids_from_member(
    member: BuiltinSurfaceMemberDescriptor,
    ids: &mut Vec<u16>,
) {
    if let BuiltinSurfaceMemberDescriptor::Bound(binding) = member {
        if let BuiltinOp::Native(native_id) = binding.op {
            ids.push(native_id);
        }
    }
}

fn builtin_native_op_table() -> &'static BuiltinNativeOpTable {
    BUILTIN_NATIVE_OP_TABLE.get_or_init(|| {
        let registry = BuiltinRegistry::shared();
        let mut ordered_ids = Vec::new();

        for (_, descriptor) in registry.global_descriptors() {
            if let Some(binding) = descriptor.binding {
                if let BuiltinOp::Native(native_id) = binding.op {
                    ordered_ids.push(native_id);
                }
            }
        }

        for (_, descriptor) in registry.type_descriptors() {
            if let Some(constructor) = descriptor.constructor {
                collect_builtin_native_ids_from_member(constructor, &mut ordered_ids);
            }
            for member in descriptor.instance_methods.values().copied() {
                collect_builtin_native_ids_from_member(member, &mut ordered_ids);
            }
            for member in descriptor.instance_properties.values().copied() {
                collect_builtin_native_ids_from_member(member, &mut ordered_ids);
            }
            for member in descriptor.static_methods.values().copied() {
                collect_builtin_native_ids_from_member(member, &mut ordered_ids);
            }
            for member in descriptor.static_properties.values().copied() {
                collect_builtin_native_ids_from_member(member, &mut ordered_ids);
            }
        }

        ordered_ids.extend([
            crate::compiler::native_id::STRING_REPLACE_REGEXP,
            crate::compiler::native_id::STRING_SPLIT_REGEXP,
            crate::compiler::native_id::STRING_REPLACE_WITH_REGEXP,
        ]);

        ordered_ids.sort_unstable();
        ordered_ids.dedup();

        let indices = ordered_ids
            .iter()
            .enumerate()
            .map(|(index, native_id)| {
                (
                    *native_id,
                    BuiltinOpId::try_from(index)
                        .expect("builtin native op table exceeded u16 capacity"),
                )
            })
            .collect();

        BuiltinNativeOpTable {
            ordered_ids,
            indices,
        }
    })
}

pub fn encode_builtin_op_id(op: BuiltinOp) -> BuiltinOpId {
    match op {
        BuiltinOp::Native(native_id) => {
            let compact_index = builtin_native_op_table()
                .indices
                .get(&native_id)
                .copied()
                .unwrap_or_else(|| {
                    panic!("unregistered builtin native op id: {native_id:#06x}");
                });
            NATIVE_BASE + compact_index
        }
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
        let compact_index = usize::from(id - NATIVE_BASE);
        return builtin_native_op_table()
            .ordered_ids
            .get(compact_index)
            .copied()
            .map(BuiltinOp::Native);
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
