//! Raya builtin types and handlers.
//!
//! This module provides:
//! - Rust-owned type signatures for builtin globals and types
//! - Native method handlers for builtins (arrays, strings, numbers, etc.)
//!
//! Builtin globals, constructors, prototypes, and namespace objects are now
//! materialized directly from Rust runtime bootstrap.
//!
//! # Usage
//!
//! ```rust,ignore
//! use raya_engine::vm::builtins::{builtin_names, get_all_signatures};
//!
//! // Get type signatures for type checking
//! let signatures = get_all_signatures();
//! ```

// Handler modules for built-in type methods
pub mod handlers;

use std::collections::HashMap;
use std::sync::Arc;

use crate::compiler::builtins::{BuiltinOpId, BuiltinRegistry, BuiltinSurfaceMemberDescriptor};
use crate::compiler::Module;
use crate::vm::interpreter::execution::OpcodeResult;
use crate::vm::interpreter::Interpreter;
use crate::vm::scheduler::Task;
use crate::vm::stack::Stack;
/// Type signature for a builtin class method
#[derive(Debug, Clone)]
pub struct MethodSig {
    /// Method name
    pub name: &'static str,
    /// Parameters: (name, type)
    pub params: &'static [(&'static str, &'static str)],
    /// Minimum number of required parameters (for optional params)
    pub min_params: usize,
    /// Return type
    pub return_type: &'static str,
    /// Is this a static method?
    pub is_static: bool,
}

/// Type signature for a builtin class property
#[derive(Debug, Clone)]
pub struct PropertySig {
    /// Property name
    pub name: &'static str,
    /// Property type
    pub ty: &'static str,
    /// Is this a static property?
    pub is_static: bool,
}

/// Type signature for a builtin class
#[derive(Debug, Clone)]
pub struct ClassSig {
    /// Class name
    pub name: &'static str,
    /// Type parameters
    pub type_params: &'static [&'static str],
    /// Properties
    pub properties: &'static [PropertySig],
    /// Methods
    pub methods: &'static [MethodSig],
    /// Constructor parameters (None if not constructible)
    pub constructor: Option<&'static [(&'static str, &'static str)]>,
}

/// Type signature for a builtin function
#[derive(Debug, Clone)]
pub struct FunctionSig {
    /// Function name
    pub name: &'static str,
    /// Type parameters
    pub type_params: &'static [&'static str],
    /// Parameters: (name, type)
    pub params: &'static [(&'static str, &'static str)],
    /// Return type
    pub return_type: &'static str,
}

/// All type signatures for a builtin module
#[derive(Debug, Clone)]
pub struct BuiltinSignatures {
    /// Module name
    pub name: &'static str,
    /// Classes in this module
    pub classes: &'static [ClassSig],
    /// Functions in this module
    pub functions: &'static [FunctionSig],
}

/// List all available builtin names
pub fn builtin_names() -> impl Iterator<Item = &'static str> {
    BuiltinRegistry::shared()
        .global_descriptors()
        .map(|(_, descriptor)| descriptor.global_name)
}

/// Get the number of available builtins
pub fn builtin_count() -> usize {
    BuiltinRegistry::shared().global_descriptors().count()
}

pub(crate) fn dispatch_builtin_kernel_call(
    interpreter: &mut Interpreter<'_>,
    stack: &mut Stack,
    module: &Module,
    task: &Arc<Task>,
    op_id: BuiltinOpId,
    arg_count: u8,
) -> OpcodeResult {
    crate::vm::interpreter::dispatch_builtin_kernel_call_impl(
        interpreter,
        stack,
        module,
        task,
        op_id,
        arg_count,
    )
}

// ============================================================================
// Builtin Type Signatures
// ============================================================================

/// Get all builtin type signatures for injection into the type checker
pub fn get_all_signatures() -> &'static [BuiltinSignatures] {
    BUILTIN_SIGS
}

pub fn builtin_visible_constructor_length(name: &str) -> Option<usize> {
    match name {
        "Array" | "Object" | "Function" | "Boolean" | "Number" | "String" | "Promise" => Some(1),
        "AggregateError" => Some(2),
        "SuppressedError" => Some(3),
        "Symbol" | "Map" | "Set" | "WeakMap" | "WeakSet" => Some(0),
        "Error" | "EvalError" | "RangeError" | "ReferenceError" | "SyntaxError" | "TypeError"
        | "URIError" | "InternalError" | "ChannelError" | "AssertionError" => Some(1),
        "RegExp" => Some(2),
        "Proxy" => Some(2),
        "ArrayBuffer" => Some(1),
        "DataView" => Some(3),
        _ => None,
    }
}

/// Get signatures for a specific builtin
pub fn get_signatures(name: &str) -> Option<&'static BuiltinSignatures> {
    BUILTIN_SIGS.iter().find(|s| s.name == name)
}

/// Convert static signatures to the checker's BuiltinSignatures format
///
/// This converts the static &'static str signatures to owned Strings
/// for use with the type checker.
pub fn to_checker_signatures() -> Vec<crate::parser::checker::BuiltinSignatures> {
    let mut signatures: Vec<_> = BUILTIN_SIGS
        .iter()
        .map(|sig| {
            crate::parser::checker::BuiltinSignatures {
                name: sig.name.to_string(),
                classes: sig
                    .classes
                    .iter()
                    .map(|c| {
                        crate::parser::checker::BuiltinClass {
                            name: c.name.to_string(),
                            type_params: c.type_params.iter().map(|s| s.to_string()).collect(),
                            properties: c
                                .properties
                                .iter()
                                .map(|p| crate::parser::checker::BuiltinProperty {
                                    name: p.name.to_string(),
                                    ty: p.ty.to_string(),
                                    is_static: p.is_static,
                                    descriptor: builtin_property_descriptor(c.name, p.name),
                                })
                                .collect(),
                            methods: c
                                .methods
                                .iter()
                                .map(|m| {
                                    crate::parser::checker::BuiltinMethod {
                                        name: m.name.to_string(),
                                        params: m
                                            .params
                                            .iter()
                                            .map(|(n, t)| (n.to_string(), t.to_string()))
                                            .collect(),
                                        min_params: m.min_params,
                                        return_type: m.return_type.to_string(),
                                        is_static: m.is_static,
                                        type_params: vec![], // VM builtins don't support method-level type params yet
                                    }
                                })
                                .collect(),
                            constructor_params: c.constructor.map(|params| {
                                params
                                    .iter()
                                    .map(|(n, t)| (n.to_string(), t.to_string()))
                                    .collect()
                            }),
                        }
                    })
                    .collect(),
                functions: sig
                    .functions
                    .iter()
                    .map(|f| crate::parser::checker::BuiltinFunction {
                        name: f.name.to_string(),
                        type_params: f.type_params.iter().map(|s| s.to_string()).collect(),
                        params: f
                            .params
                            .iter()
                            .map(|(n, t)| (n.to_string(), t.to_string()))
                            .collect(),
                        return_type: f.return_type.to_string(),
                    })
                    .collect(),
            }
        })
        .collect();

    overlay_checker_signatures_with_registry(&mut signatures);
    signatures
}

fn overlay_checker_signatures_with_registry(
    signatures: &mut Vec<crate::parser::checker::BuiltinSignatures>,
) {
    use crate::parser::checker::{
        BuiltinClass, BuiltinFunction, BuiltinMethod, BuiltinProperty, BuiltinSignatures,
    };

    fn member_return_type(binding: &BuiltinSurfaceMemberDescriptor) -> String {
        match binding {
            BuiltinSurfaceMemberDescriptor::SurfaceOnly => "unknown".to_string(),
            BuiltinSurfaceMemberDescriptor::Opcode(crate::compiler::type_registry::OpcodeKind::StringLen)
            | BuiltinSurfaceMemberDescriptor::Opcode(
                crate::compiler::type_registry::OpcodeKind::ArrayLen,
            ) => "number".to_string(),
            BuiltinSurfaceMemberDescriptor::Bound(binding) => binding
                .return_type_name()
                .unwrap_or("unknown")
                .to_string(),
        }
    }

    let mut class_locations: HashMap<String, (usize, usize)> = HashMap::new();
    for (module_index, module) in signatures.iter().enumerate() {
        for (class_index, class) in module.classes.iter().enumerate() {
            class_locations.insert(class.name.clone(), (module_index, class_index));
        }
    }

    let registry_module_index = if let Some(index) = signatures
        .iter()
        .position(|module| module.name == "__rust_builtins__")
    {
        index
    } else {
        signatures.push(BuiltinSignatures::new("__rust_builtins__"));
        signatures.len() - 1
    };

    for (type_name, descriptor) in BuiltinRegistry::shared().type_descriptors() {
        if type_name.eq("EventEmitter") || type_name.eq("RegExp") {
            continue;
        }
        let (module_index, class_index) = if let Some(&(module_index, class_index)) =
            class_locations.get(type_name)
        {
            (module_index, class_index)
        } else {
            signatures[registry_module_index]
                .classes
                .push(BuiltinClass::new(type_name));
            let class_index = signatures[registry_module_index].classes.len() - 1;
            class_locations.insert(type_name.to_string(), (registry_module_index, class_index));
            (registry_module_index, class_index)
        };

        let class = &mut signatures[module_index].classes[class_index];
        if class.constructor_params.is_none() && descriptor.constructor.is_some() {
            class.constructor_params = Some(Vec::new());
        }

        for (name, binding) in &descriptor.instance_methods {
            if class
                .methods
                .iter()
                .any(|method| !method.is_static && method.name == *name)
            {
                continue;
            }
            class.methods.push(BuiltinMethod {
                name: (*name).to_string(),
                params: Vec::new(),
                min_params: 0,
                return_type: member_return_type(binding),
                is_static: false,
                type_params: Vec::new(),
            });
        }

        for (name, binding) in &descriptor.static_methods {
            if class
                .methods
                .iter()
                .any(|method| method.is_static && method.name == *name)
            {
                continue;
            }
            class.methods.push(BuiltinMethod {
                name: (*name).to_string(),
                params: Vec::new(),
                min_params: 0,
                return_type: member_return_type(binding),
                is_static: true,
                type_params: Vec::new(),
            });
        }

        for (name, binding) in &descriptor.instance_properties {
            if class
                .properties
                .iter()
                .any(|property| !property.is_static && property.name == *name)
            {
                continue;
            }
            class.properties.push(BuiltinProperty {
                name: (*name).to_string(),
                ty: member_return_type(binding),
                is_static: false,
                descriptor: builtin_property_descriptor(type_name, name),
            });
        }

        for (name, binding) in &descriptor.static_properties {
            if class
                .properties
                .iter()
                .any(|property| property.is_static && property.name == *name)
            {
                continue;
            }
            class.properties.push(BuiltinProperty {
                name: (*name).to_string(),
                ty: member_return_type(binding),
                is_static: true,
                descriptor: builtin_property_descriptor(type_name, name),
            });
        }
    }

    for (global_name, descriptor) in BuiltinRegistry::shared().global_descriptors() {
        if descriptor.symbol_type != crate::compiler::SymbolType::Function {
            continue;
        }
        if signatures.iter().any(|module| {
            module
                .functions
                .iter()
                .any(|function| function.name == *global_name)
        }) {
            continue;
        }
        signatures[registry_module_index]
            .functions
            .push(BuiltinFunction {
                name: (*global_name).to_string(),
                type_params: Vec::new(),
                params: Vec::new(),
                return_type: descriptor
                    .binding
                    .and_then(|binding| binding.return_type_name())
                    .unwrap_or("unknown")
                    .to_string(),
            });
    }
}

fn builtin_property_descriptor(
    class_name: &str,
    property_name: &str,
) -> Option<crate::parser::checker::BuiltinPropertyDescriptor> {
    use crate::parser::checker::BuiltinPropertyDescriptor;

    let descriptor = match (class_name, property_name) {
        ("string", "length") => BuiltinPropertyDescriptor {
            writable: Some(false),
            enumerable: Some(false),
            configurable: Some(false),
            has_getter: true,
            has_setter: false,
        },
        ("Map", "size")
        | ("Set", "size")
        | ("Buffer", "length")
        | ("ArrayBuffer", "byteLength")
        | ("Uint8Array", "length")
        | ("Int8Array", "length")
        | ("Int32Array", "length")
        | ("Float64Array", "length")
        | ("DataView", "byteLength")
        | ("DataView", "byteOffset") => BuiltinPropertyDescriptor {
            writable: Some(false),
            enumerable: Some(false),
            configurable: Some(true),
            has_getter: true,
            has_setter: false,
        },
        (
            class_name,
            "message" | "name" | "stack" | "cause" | "code" | "errno" | "syscall" | "path",
        ) if matches!(
            class_name,
            "Error"
                | "TypeError"
                | "RangeError"
                | "ReferenceError"
                | "SyntaxError"
                | "URIError"
                | "EvalError"
                | "InternalError"
                | "AggregateError"
                | "SuppressedError"
                | "ChannelClosedError"
                | "AssertionError"
        ) =>
        {
            BuiltinPropertyDescriptor {
                writable: Some(true),
                enumerable: Some(false),
                configurable: Some(true),
                has_getter: false,
                has_setter: false,
            }
        }
        ("AggregateError", "errors") => BuiltinPropertyDescriptor {
            writable: Some(true),
            enumerable: Some(false),
            configurable: Some(true),
            has_getter: false,
            has_setter: false,
        },
        _ => return None,
    };
    Some(descriptor)
}

static BUILTIN_SIGS: &[BuiltinSignatures] = &[
    BuiltinSignatures {
        name: "GlobalConstructors",
        classes: &[ClassSig {
            name: "Array",
            type_params: &["T"],
            properties: &[],
            methods: &[MethodSig {
                name: "from",
                params: &[("items", "unknown")],
                min_params: 1,
                return_type: "Array<unknown>",
                is_static: true,
            }],
            constructor: Some(&[("length", "number")]),
        }],
        functions: &[
            FunctionSig {
                name: "Boolean",
                type_params: &[],
                params: &[("value", "any")],
                return_type: "boolean",
            },
            FunctionSig {
                name: "Number",
                type_params: &[],
                params: &[("value", "any")],
                return_type: "number",
            },
            FunctionSig {
                name: "String",
                type_params: &[],
                params: &[("value", "any")],
                return_type: "string",
            },
            FunctionSig {
                name: "encodeURI",
                type_params: &[],
                params: &[("value", "string")],
                return_type: "string",
            },
            FunctionSig {
                name: "decodeURI",
                type_params: &[],
                params: &[("value", "string")],
                return_type: "string",
            },
            FunctionSig {
                name: "encodeURIComponent",
                type_params: &[],
                params: &[("value", "string")],
                return_type: "string",
            },
            FunctionSig {
                name: "decodeURIComponent",
                type_params: &[],
                params: &[("value", "string")],
                return_type: "string",
            },
            FunctionSig {
                name: "escape",
                type_params: &[],
                params: &[("value", "string")],
                return_type: "string",
            },
            FunctionSig {
                name: "unescape",
                type_params: &[],
                params: &[("value", "string")],
                return_type: "string",
            },
        ],
    },
    BuiltinSignatures {
        name: "Iterator",
        classes: &[
            ClassSig {
                name: "IteratorResult",
                type_params: &["T"],
                properties: &[
                    PropertySig {
                        name: "done",
                        ty: "boolean",
                        is_static: false,
                    },
                    PropertySig {
                        name: "value",
                        ty: "T",
                        is_static: false,
                    },
                ],
                methods: &[],
                constructor: None,
            },
            ClassSig {
                name: "Iterator",
                type_params: &["T"],
                properties: &[],
                methods: &[
                    MethodSig {
                        name: "next",
                        params: &[],
                        min_params: 0,
                        return_type: "IteratorResult<T>",
                        is_static: false,
                    },
                    MethodSig {
                        name: "fromArray",
                        params: &[("items", "T[]")],
                        min_params: 1,
                        return_type: "Iterator<T>",
                        is_static: true,
                    },
                    MethodSig {
                        name: "toArray",
                        params: &[],
                        min_params: 0,
                        return_type: "Array<T>",
                        is_static: false,
                    },
                ],
                constructor: None,
            },
        ],
        functions: &[],
    },
    BuiltinSignatures {
        name: "Temporal",
        classes: &[
            ClassSig {
                name: "TemporalInstant",
                type_params: &[],
                properties: &[],
                methods: &[MethodSig {
                    name: "toString",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                }],
                constructor: None,
            },
            ClassSig {
                name: "TemporalPlainDate",
                type_params: &[],
                properties: &[],
                methods: &[MethodSig {
                    name: "toString",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                }],
                constructor: None,
            },
            ClassSig {
                name: "TemporalPlainTime",
                type_params: &[],
                properties: &[],
                methods: &[MethodSig {
                    name: "toString",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                }],
                constructor: None,
            },
            ClassSig {
                name: "TemporalZonedDateTime",
                type_params: &[],
                properties: &[],
                methods: &[MethodSig {
                    name: "toString",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                }],
                constructor: None,
            },
            ClassSig {
                name: "Temporal",
                type_params: &[],
                properties: &[],
                methods: &[
                    MethodSig {
                        name: "Instant",
                        params: &[("epochMilliseconds", "number")],
                        min_params: 1,
                        return_type: "TemporalInstant",
                        is_static: true,
                    },
                    MethodSig {
                        name: "PlainDate",
                        params: &[("year", "number"), ("month", "number"), ("day", "number")],
                        min_params: 3,
                        return_type: "TemporalPlainDate",
                        is_static: true,
                    },
                    MethodSig {
                        name: "PlainTime",
                        params: &[
                            ("hour", "number"),
                            ("minute", "number"),
                            ("second", "number"),
                            ("millisecond", "number"),
                        ],
                        min_params: 4,
                        return_type: "TemporalPlainTime",
                        is_static: true,
                    },
                    MethodSig {
                        name: "ZonedDateTime",
                        params: &[("epochNanoseconds", "number"), ("timeZone", "string")],
                        min_params: 2,
                        return_type: "TemporalZonedDateTime",
                        is_static: true,
                    },
                ],
                constructor: None,
            },
        ],
        functions: &[],
    },
    // string (primitive)
    BuiltinSignatures {
        name: "string",
        classes: &[ClassSig {
            name: "string",
            type_params: &[],
            properties: &[PropertySig {
                name: "length",
                ty: "number",
                is_static: false,
            }],
            methods: &[
                MethodSig {
                    name: "charAt",
                    params: &[("index", "number")],
                    min_params: 1,
                    return_type: "string",
                    is_static: false,
                },
                MethodSig {
                    name: "charCodeAt",
                    params: &[("index", "number")],
                    min_params: 1,
                    return_type: "int",
                    is_static: false,
                },
                MethodSig {
                    name: "substring",
                    params: &[("start", "number"), ("end", "number")],
                    min_params: 1,
                    return_type: "string",
                    is_static: false,
                },
                MethodSig {
                    name: "toUpperCase",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                },
                MethodSig {
                    name: "toLowerCase",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                },
                MethodSig {
                    name: "trim",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                },
                MethodSig {
                    name: "trimStart",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                },
                MethodSig {
                    name: "trimEnd",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                },
                MethodSig {
                    name: "indexOf",
                    params: &[("searchStr", "string"), ("fromIndex", "number")],
                    min_params: 1,
                    return_type: "int",
                    is_static: false,
                },
                MethodSig {
                    name: "lastIndexOf",
                    params: &[("searchStr", "string"), ("fromIndex", "number")],
                    min_params: 1,
                    return_type: "int",
                    is_static: false,
                },
                MethodSig {
                    name: "includes",
                    params: &[("searchStr", "string")],
                    min_params: 1,
                    return_type: "boolean",
                    is_static: false,
                },
                MethodSig {
                    name: "startsWith",
                    params: &[("prefix", "string")],
                    min_params: 1,
                    return_type: "boolean",
                    is_static: false,
                },
                MethodSig {
                    name: "endsWith",
                    params: &[("suffix", "string")],
                    min_params: 1,
                    return_type: "boolean",
                    is_static: false,
                },
                MethodSig {
                    name: "split",
                    params: &[("separator", "string"), ("limit", "number")],
                    min_params: 1,
                    return_type: "string[]",
                    is_static: false,
                },
                MethodSig {
                    name: "replace",
                    params: &[("search", "string"), ("replacement", "string")],
                    min_params: 2,
                    return_type: "string",
                    is_static: false,
                },
                MethodSig {
                    name: "repeat",
                    params: &[("count", "number")],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                },
                MethodSig {
                    name: "padStart",
                    params: &[("length", "number"), ("pad", "string")],
                    min_params: 1,
                    return_type: "string",
                    is_static: false,
                },
                MethodSig {
                    name: "padEnd",
                    params: &[("length", "number"), ("pad", "string")],
                    min_params: 1,
                    return_type: "string",
                    is_static: false,
                },
                MethodSig {
                    name: "match",
                    params: &[("pattern", "RegExp")],
                    min_params: 1,
                    return_type: "string[] | null",
                    is_static: false,
                },
                MethodSig {
                    name: "matchAll",
                    params: &[("pattern", "RegExp")],
                    min_params: 1,
                    return_type: "string[][]",
                    is_static: false,
                },
                MethodSig {
                    name: "search",
                    params: &[("pattern", "RegExp")],
                    min_params: 1,
                    return_type: "int",
                    is_static: false,
                },
                MethodSig {
                    name: "slice",
                    params: &[("start", "number"), ("end", "number")],
                    min_params: 1,
                    return_type: "string",
                    is_static: false,
                },
            ],
            constructor: None,
        }],
        functions: &[],
    },
    // number (primitive)
    BuiltinSignatures {
        name: "number",
        classes: &[ClassSig {
            name: "number",
            type_params: &[],
            properties: &[],
            methods: &[
                MethodSig {
                    name: "toFixed",
                    params: &[("digits", "number")],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                },
                MethodSig {
                    name: "toPrecision",
                    params: &[("precision", "number")],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                },
                MethodSig {
                    name: "toString",
                    params: &[("radix", "number")],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                },
            ],
            constructor: None,
        }],
        functions: &[],
    },
    // Symbol
    BuiltinSignatures {
        name: "Symbol",
        classes: &[ClassSig {
            name: "Symbol",
            type_params: &[],
            properties: &[
                PropertySig {
                    name: "iterator",
                    ty: "Symbol",
                    is_static: true,
                },
                PropertySig {
                    name: "toStringTag",
                    ty: "Symbol",
                    is_static: true,
                },
            ],
            methods: &[
                MethodSig {
                    name: "toString",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                },
                MethodSig {
                    name: "valueOf",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                },
                MethodSig {
                    name: "for",
                    params: &[("key", "string")],
                    min_params: 1,
                    return_type: "Symbol",
                    is_static: true,
                },
                MethodSig {
                    name: "iterator",
                    params: &[],
                    min_params: 0,
                    return_type: "Symbol",
                    is_static: true,
                },
                MethodSig {
                    name: "toStringTag",
                    params: &[],
                    min_params: 0,
                    return_type: "Symbol",
                    is_static: true,
                },
                MethodSig {
                    name: "keyFor",
                    params: &[("sym", "Symbol")],
                    min_params: 1,
                    return_type: "string",
                    is_static: true,
                },
            ],
            constructor: None,
        }],
        functions: &[],
    },
    // Map<K, V>
    BuiltinSignatures {
        name: "Map",
        classes: &[ClassSig {
            name: "Map",
            type_params: &["K", "V"],
            properties: &[PropertySig {
                name: "size",
                ty: "number",
                is_static: false,
            }],
            methods: &[
                MethodSig {
                    name: "get",
                    params: &[("key", "K")],
                    min_params: 1,
                    return_type: "V | null",
                    is_static: false,
                },
                MethodSig {
                    name: "set",
                    params: &[("key", "K"), ("value", "V")],
                    min_params: 2,
                    return_type: "void",
                    is_static: false,
                },
                MethodSig {
                    name: "has",
                    params: &[("key", "K")],
                    min_params: 1,
                    return_type: "boolean",
                    is_static: false,
                },
                MethodSig {
                    name: "delete",
                    params: &[("key", "K")],
                    min_params: 1,
                    return_type: "boolean",
                    is_static: false,
                },
                MethodSig {
                    name: "clear",
                    params: &[],
                    min_params: 0,
                    return_type: "void",
                    is_static: false,
                },
                MethodSig {
                    name: "keys",
                    params: &[],
                    min_params: 0,
                    return_type: "Array<K>",
                    is_static: false,
                },
                MethodSig {
                    name: "values",
                    params: &[],
                    min_params: 0,
                    return_type: "Array<V>",
                    is_static: false,
                },
                MethodSig {
                    name: "entries",
                    params: &[],
                    min_params: 0,
                    return_type: "Array<[K, V]>",
                    is_static: false,
                },
                MethodSig {
                    name: "iterator",
                    params: &[],
                    min_params: 0,
                    return_type: "Array<[K, V]>",
                    is_static: false,
                },
                MethodSig {
                    name: "forEach",
                    params: &[
                        ("callback", "(V, K, Map<K, V>) => void"),
                        ("thisArg", "unknown"),
                    ],
                    min_params: 1,
                    return_type: "void",
                    is_static: false,
                },
            ],
            constructor: Some(&[]),
        }],
        functions: &[],
    },
    // Set<T>
    BuiltinSignatures {
        name: "Set",
        classes: &[ClassSig {
            name: "Set",
            type_params: &["T"],
            properties: &[PropertySig {
                name: "size",
                ty: "number",
                is_static: false,
            }],
            methods: &[
                MethodSig {
                    name: "add",
                    params: &[("value", "T")],
                    min_params: 1,
                    return_type: "void",
                    is_static: false,
                },
                MethodSig {
                    name: "has",
                    params: &[("value", "T")],
                    min_params: 1,
                    return_type: "boolean",
                    is_static: false,
                },
                MethodSig {
                    name: "delete",
                    params: &[("value", "T")],
                    min_params: 1,
                    return_type: "boolean",
                    is_static: false,
                },
                MethodSig {
                    name: "clear",
                    params: &[],
                    min_params: 0,
                    return_type: "void",
                    is_static: false,
                },
                MethodSig {
                    name: "values",
                    params: &[],
                    min_params: 0,
                    return_type: "Array<T>",
                    is_static: false,
                },
                MethodSig {
                    name: "keys",
                    params: &[],
                    min_params: 0,
                    return_type: "Array<T>",
                    is_static: false,
                },
                MethodSig {
                    name: "entries",
                    params: &[],
                    min_params: 0,
                    return_type: "Array<[T, T]>",
                    is_static: false,
                },
                MethodSig {
                    name: "forEach",
                    params: &[("callback", "(T, T, Set<T>) => void"), ("thisArg", "unknown")],
                    min_params: 1,
                    return_type: "void",
                    is_static: false,
                },
                MethodSig {
                    name: "iterator",
                    params: &[],
                    min_params: 0,
                    return_type: "Array<T>",
                    is_static: false,
                },
                MethodSig {
                    name: "union",
                    params: &[("other", "Set<T>")],
                    min_params: 1,
                    return_type: "Set<T>",
                    is_static: false,
                },
                MethodSig {
                    name: "intersection",
                    params: &[("other", "Set<T>")],
                    min_params: 1,
                    return_type: "Set<T>",
                    is_static: false,
                },
                MethodSig {
                    name: "difference",
                    params: &[("other", "Set<T>")],
                    min_params: 1,
                    return_type: "Set<T>",
                    is_static: false,
                },
            ],
            constructor: Some(&[]),
        }],
        functions: &[],
    },
    // Buffer
    BuiltinSignatures {
        name: "Buffer",
        classes: &[ClassSig {
            name: "Buffer",
            type_params: &[],
            properties: &[PropertySig {
                name: "length",
                ty: "number",
                is_static: false,
            }],
            methods: &[
                MethodSig {
                    name: "getByte",
                    params: &[("offset", "number")],
                    min_params: 1,
                    return_type: "number",
                    is_static: false,
                },
                MethodSig {
                    name: "setByte",
                    params: &[("offset", "number"), ("value", "number")],
                    min_params: 2,
                    return_type: "void",
                    is_static: false,
                },
                MethodSig {
                    name: "getInt32",
                    params: &[("offset", "number")],
                    min_params: 1,
                    return_type: "number",
                    is_static: false,
                },
                MethodSig {
                    name: "setInt32",
                    params: &[("offset", "number"), ("value", "number")],
                    min_params: 2,
                    return_type: "void",
                    is_static: false,
                },
                MethodSig {
                    name: "getFloat64",
                    params: &[("offset", "number")],
                    min_params: 1,
                    return_type: "number",
                    is_static: false,
                },
                MethodSig {
                    name: "setFloat64",
                    params: &[("offset", "number"), ("value", "number")],
                    min_params: 2,
                    return_type: "void",
                    is_static: false,
                },
                MethodSig {
                    name: "slice",
                    params: &[("start", "number"), ("end", "number")],
                    min_params: 1,
                    return_type: "Buffer",
                    is_static: false,
                },
                MethodSig {
                    name: "copy",
                    params: &[
                        ("target", "Buffer"),
                        ("targetStart", "number"),
                        ("sourceStart", "number"),
                        ("sourceEnd", "number"),
                    ],
                    min_params: 1,
                    return_type: "number",
                    is_static: false,
                },
                MethodSig {
                    name: "toString",
                    params: &[("encoding", "string")],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                },
            ],
            constructor: Some(&[("size", "number")]),
        }],
        functions: &[
            FunctionSig {
                name: "bufferFromString",
                type_params: &[],
                params: &[("str", "string"), ("encoding", "string")],
                return_type: "Buffer",
            },
            FunctionSig {
                name: "bufferFromUtf8",
                type_params: &[],
                params: &[("str", "string")],
                return_type: "Buffer",
            },
        ],
    },
    // Typed arrays + DataView
    BuiltinSignatures {
        name: "TypedArray",
        classes: &[
            ClassSig {
                name: "ArrayBuffer",
                type_params: &[],
                properties: &[PropertySig {
                    name: "byteLength",
                    ty: "int",
                    is_static: false,
                }],
                methods: &[MethodSig {
                    name: "slice",
                    params: &[("begin", "int"), ("end", "int")],
                    min_params: 0,
                    return_type: "ArrayBuffer",
                    is_static: false,
                }],
                constructor: Some(&[("byteLength", "int")]),
            },
            ClassSig {
                name: "Uint8Array",
                type_params: &[],
                properties: &[
                    PropertySig {
                        name: "length",
                        ty: "int",
                        is_static: false,
                    },
                    PropertySig {
                        name: "byteLength",
                        ty: "int",
                        is_static: false,
                    },
                    PropertySig {
                        name: "byteOffset",
                        ty: "int",
                        is_static: false,
                    },
                ],
                methods: &[
                    MethodSig {
                        name: "get",
                        params: &[("index", "int")],
                        min_params: 1,
                        return_type: "int",
                        is_static: false,
                    },
                    MethodSig {
                        name: "set",
                        params: &[("index", "int"), ("value", "int")],
                        min_params: 2,
                        return_type: "void",
                        is_static: false,
                    },
                ],
                constructor: Some(&[("source", "int | ArrayBuffer")]),
            },
            ClassSig {
                name: "Int8Array",
                type_params: &[],
                properties: &[
                    PropertySig {
                        name: "length",
                        ty: "int",
                        is_static: false,
                    },
                    PropertySig {
                        name: "byteLength",
                        ty: "int",
                        is_static: false,
                    },
                    PropertySig {
                        name: "byteOffset",
                        ty: "int",
                        is_static: false,
                    },
                ],
                methods: &[
                    MethodSig {
                        name: "get",
                        params: &[("index", "int")],
                        min_params: 1,
                        return_type: "int",
                        is_static: false,
                    },
                    MethodSig {
                        name: "set",
                        params: &[("index", "int"), ("value", "int")],
                        min_params: 2,
                        return_type: "void",
                        is_static: false,
                    },
                ],
                constructor: Some(&[("source", "int | ArrayBuffer")]),
            },
            ClassSig {
                name: "Int32Array",
                type_params: &[],
                properties: &[
                    PropertySig {
                        name: "length",
                        ty: "int",
                        is_static: false,
                    },
                    PropertySig {
                        name: "byteLength",
                        ty: "int",
                        is_static: false,
                    },
                    PropertySig {
                        name: "byteOffset",
                        ty: "int",
                        is_static: false,
                    },
                ],
                methods: &[
                    MethodSig {
                        name: "get",
                        params: &[("index", "int")],
                        min_params: 1,
                        return_type: "int",
                        is_static: false,
                    },
                    MethodSig {
                        name: "set",
                        params: &[("index", "int"), ("value", "int")],
                        min_params: 2,
                        return_type: "void",
                        is_static: false,
                    },
                ],
                constructor: Some(&[("source", "int | ArrayBuffer")]),
            },
            ClassSig {
                name: "Float64Array",
                type_params: &[],
                properties: &[
                    PropertySig {
                        name: "length",
                        ty: "int",
                        is_static: false,
                    },
                    PropertySig {
                        name: "byteLength",
                        ty: "int",
                        is_static: false,
                    },
                    PropertySig {
                        name: "byteOffset",
                        ty: "int",
                        is_static: false,
                    },
                ],
                methods: &[
                    MethodSig {
                        name: "get",
                        params: &[("index", "int")],
                        min_params: 1,
                        return_type: "number",
                        is_static: false,
                    },
                    MethodSig {
                        name: "set",
                        params: &[("index", "int"), ("value", "number")],
                        min_params: 2,
                        return_type: "void",
                        is_static: false,
                    },
                ],
                constructor: Some(&[("source", "int | ArrayBuffer")]),
            },
            ClassSig {
                name: "DataView",
                type_params: &[],
                properties: &[
                    PropertySig {
                        name: "byteLength",
                        ty: "int",
                        is_static: false,
                    },
                    PropertySig {
                        name: "byteOffset",
                        ty: "int",
                        is_static: false,
                    },
                ],
                methods: &[
                    MethodSig {
                        name: "getUint8",
                        params: &[("offset", "int")],
                        min_params: 1,
                        return_type: "int",
                        is_static: false,
                    },
                    MethodSig {
                        name: "setUint8",
                        params: &[("offset", "int"), ("value", "int")],
                        min_params: 2,
                        return_type: "void",
                        is_static: false,
                    },
                    MethodSig {
                        name: "getInt8",
                        params: &[("offset", "int")],
                        min_params: 1,
                        return_type: "int",
                        is_static: false,
                    },
                    MethodSig {
                        name: "setInt8",
                        params: &[("offset", "int"), ("value", "int")],
                        min_params: 2,
                        return_type: "void",
                        is_static: false,
                    },
                    MethodSig {
                        name: "getInt32",
                        params: &[("offset", "int"), ("littleEndian", "boolean")],
                        min_params: 1,
                        return_type: "int",
                        is_static: false,
                    },
                    MethodSig {
                        name: "setInt32",
                        params: &[
                            ("offset", "int"),
                            ("value", "int"),
                            ("littleEndian", "boolean"),
                        ],
                        min_params: 2,
                        return_type: "void",
                        is_static: false,
                    },
                    MethodSig {
                        name: "getUint32",
                        params: &[("offset", "int"), ("littleEndian", "boolean")],
                        min_params: 1,
                        return_type: "int",
                        is_static: false,
                    },
                    MethodSig {
                        name: "setUint32",
                        params: &[
                            ("offset", "int"),
                            ("value", "int"),
                            ("littleEndian", "boolean"),
                        ],
                        min_params: 2,
                        return_type: "void",
                        is_static: false,
                    },
                    MethodSig {
                        name: "getFloat64",
                        params: &[("offset", "int"), ("littleEndian", "boolean")],
                        min_params: 1,
                        return_type: "number",
                        is_static: false,
                    },
                    MethodSig {
                        name: "setFloat64",
                        params: &[
                            ("offset", "int"),
                            ("value", "number"),
                            ("littleEndian", "boolean"),
                        ],
                        min_params: 2,
                        return_type: "void",
                        is_static: false,
                    },
                ],
                constructor: Some(&[("buffer", "ArrayBuffer")]),
            },
        ],
        functions: &[],
    },
    // Date
    BuiltinSignatures {
        name: "Date",
        classes: &[ClassSig {
            name: "Date",
            type_params: &[],
            properties: &[],
            methods: &[
                MethodSig {
                    name: "getTime",
                    params: &[],
                    min_params: 0,
                    return_type: "number",
                    is_static: false,
                },
                MethodSig {
                    name: "getFullYear",
                    params: &[],
                    min_params: 0,
                    return_type: "number",
                    is_static: false,
                },
                MethodSig {
                    name: "getMonth",
                    params: &[],
                    min_params: 0,
                    return_type: "number",
                    is_static: false,
                },
                MethodSig {
                    name: "getDate",
                    params: &[],
                    min_params: 0,
                    return_type: "number",
                    is_static: false,
                },
                MethodSig {
                    name: "getDay",
                    params: &[],
                    min_params: 0,
                    return_type: "number",
                    is_static: false,
                },
                MethodSig {
                    name: "getHours",
                    params: &[],
                    min_params: 0,
                    return_type: "number",
                    is_static: false,
                },
                MethodSig {
                    name: "getMinutes",
                    params: &[],
                    min_params: 0,
                    return_type: "number",
                    is_static: false,
                },
                MethodSig {
                    name: "getSeconds",
                    params: &[],
                    min_params: 0,
                    return_type: "number",
                    is_static: false,
                },
                MethodSig {
                    name: "getMilliseconds",
                    params: &[],
                    min_params: 0,
                    return_type: "number",
                    is_static: false,
                },
                MethodSig {
                    name: "setFullYear",
                    params: &[("year", "number")],
                    min_params: 1,
                    return_type: "void",
                    is_static: false,
                },
                MethodSig {
                    name: "setTime",
                    params: &[("value", "number")],
                    min_params: 1,
                    return_type: "number",
                    is_static: false,
                },
                MethodSig {
                    name: "setMonth",
                    params: &[("month", "number")],
                    min_params: 1,
                    return_type: "void",
                    is_static: false,
                },
                MethodSig {
                    name: "setDate",
                    params: &[("date", "number")],
                    min_params: 1,
                    return_type: "void",
                    is_static: false,
                },
                MethodSig {
                    name: "setHours",
                    params: &[("hours", "number")],
                    min_params: 1,
                    return_type: "void",
                    is_static: false,
                },
                MethodSig {
                    name: "setMinutes",
                    params: &[("minutes", "number")],
                    min_params: 1,
                    return_type: "void",
                    is_static: false,
                },
                MethodSig {
                    name: "setSeconds",
                    params: &[("seconds", "number")],
                    min_params: 1,
                    return_type: "void",
                    is_static: false,
                },
                MethodSig {
                    name: "setMilliseconds",
                    params: &[("ms", "number")],
                    min_params: 1,
                    return_type: "void",
                    is_static: false,
                },
                MethodSig {
                    name: "toString",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                },
                MethodSig {
                    name: "toISOString",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                },
                MethodSig {
                    name: "toDateString",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                },
                MethodSig {
                    name: "toTimeString",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                },
                MethodSig {
                    name: "now",
                    params: &[],
                    min_params: 0,
                    return_type: "number",
                    is_static: true,
                },
                MethodSig {
                    name: "parse",
                    params: &[("str", "string")],
                    min_params: 1,
                    return_type: "number",
                    is_static: true,
                },
            ],
            constructor: Some(&[]),
        }],
        functions: &[],
    },
    // Channel<T>
    BuiltinSignatures {
        name: "Channel",
        classes: &[ClassSig {
            name: "Channel",
            type_params: &["T"],
            properties: &[],
            methods: &[
                MethodSig {
                    name: "send",
                    params: &[("value", "T")],
                    min_params: 1,
                    return_type: "void",
                    is_static: false,
                },
                MethodSig {
                    name: "receive",
                    params: &[],
                    min_params: 0,
                    return_type: "T",
                    is_static: false,
                },
                MethodSig {
                    name: "trySend",
                    params: &[("value", "T")],
                    min_params: 1,
                    return_type: "boolean",
                    is_static: false,
                },
                MethodSig {
                    name: "tryReceive",
                    params: &[],
                    min_params: 0,
                    return_type: "T | null",
                    is_static: false,
                },
                MethodSig {
                    name: "close",
                    params: &[],
                    min_params: 0,
                    return_type: "void",
                    is_static: false,
                },
                MethodSig {
                    name: "isClosed",
                    params: &[],
                    min_params: 0,
                    return_type: "boolean",
                    is_static: false,
                },
                MethodSig {
                    name: "length",
                    params: &[],
                    min_params: 0,
                    return_type: "number",
                    is_static: false,
                },
                MethodSig {
                    name: "capacity",
                    params: &[],
                    min_params: 0,
                    return_type: "number",
                    is_static: false,
                },
            ],
            constructor: Some(&[("capacity", "number")]),
        }],
        functions: &[],
    },
    // Mutex<T>
    BuiltinSignatures {
        name: "Mutex",
        classes: &[ClassSig {
            name: "Mutex",
            type_params: &["T"],
            properties: &[],
            methods: &[
                MethodSig {
                    name: "lock",
                    params: &[],
                    min_params: 0,
                    return_type: "void",
                    is_static: false,
                },
                MethodSig {
                    name: "unlock",
                    params: &[],
                    min_params: 0,
                    return_type: "void",
                    is_static: false,
                },
                MethodSig {
                    name: "tryLock",
                    params: &[],
                    min_params: 0,
                    return_type: "boolean",
                    is_static: false,
                },
                MethodSig {
                    name: "isLocked",
                    params: &[],
                    min_params: 0,
                    return_type: "boolean",
                    is_static: false,
                },
            ],
            constructor: Some(&[]),
        }],
        functions: &[],
    },
    // Promise<T> (public async contract; internally scheduler-backed)
    BuiltinSignatures {
        name: "Promise",
        classes: &[ClassSig {
            name: "Promise",
            type_params: &["T"],
            properties: &[],
            methods: &[
                MethodSig {
                    name: "cancel",
                    params: &[],
                    min_params: 0,
                    return_type: "void",
                    is_static: false,
                },
                MethodSig {
                    name: "isDone",
                    params: &[],
                    min_params: 0,
                    return_type: "boolean",
                    is_static: false,
                },
                MethodSig {
                    name: "isCancelled",
                    params: &[],
                    min_params: 0,
                    return_type: "boolean",
                    is_static: false,
                },
                MethodSig {
                    name: "then",
                    params: &[("onFulfilled", "unknown"), ("onRejected", "unknown")],
                    min_params: 1,
                    return_type: "Promise<T>",
                    is_static: false,
                },
                MethodSig {
                    name: "catch",
                    params: &[("onRejected", "unknown")],
                    min_params: 1,
                    return_type: "Promise<T>",
                    is_static: false,
                },
                MethodSig {
                    name: "finally",
                    params: &[("onFinally", "unknown")],
                    min_params: 1,
                    return_type: "Promise<T>",
                    is_static: false,
                },
                MethodSig {
                    name: "resolve",
                    params: &[("value", "T")],
                    min_params: 1,
                    return_type: "Promise<T>",
                    is_static: true,
                },
                MethodSig {
                    name: "reject",
                    params: &[("reason", "unknown")],
                    min_params: 1,
                    return_type: "Promise<T>",
                    is_static: true,
                },
                MethodSig {
                    name: "all",
                    params: &[("values", "unknown")],
                    min_params: 1,
                    return_type: "Promise<Array<T>>",
                    is_static: true,
                },
                MethodSig {
                    name: "race",
                    params: &[("values", "unknown")],
                    min_params: 1,
                    return_type: "Promise<T>",
                    is_static: true,
                },
            ],
            constructor: None, // Promises are created via async keyword
        }],
        functions: &[],
    },
    // Symbol
    BuiltinSignatures {
        name: "Symbol",
        classes: &[ClassSig {
            name: "Symbol",
            type_params: &[],
            properties: &[
                PropertySig {
                    name: "iterator",
                    ty: "Symbol",
                    is_static: true,
                },
                PropertySig {
                    name: "toStringTag",
                    ty: "Symbol",
                    is_static: true,
                },
            ],
            methods: &[
                MethodSig {
                    name: "toString",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                },
                MethodSig {
                    name: "valueOf",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                },
                MethodSig {
                    name: "for",
                    params: &[("key", "string")],
                    min_params: 1,
                    return_type: "Symbol",
                    is_static: true,
                },
                MethodSig {
                    name: "iterator",
                    params: &[],
                    min_params: 0,
                    return_type: "Symbol",
                    is_static: true,
                },
                MethodSig {
                    name: "toStringTag",
                    params: &[],
                    min_params: 0,
                    return_type: "Symbol",
                    is_static: true,
                },
                MethodSig {
                    name: "keyFor",
                    params: &[("sym", "Symbol")],
                    min_params: 1,
                    return_type: "string",
                    is_static: true,
                },
            ],
            constructor: None,
        }],
        functions: &[],
    },
    // Error classes
    BuiltinSignatures {
        name: "Error",
        classes: &[
            ClassSig {
                name: "Error",
                type_params: &[],
                properties: &[
                    PropertySig {
                        name: "message",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "name",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "stack",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "cause",
                        ty: "unknown",
                        is_static: false,
                    },
                    PropertySig {
                        name: "code",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "errno",
                        ty: "number",
                        is_static: false,
                    },
                    PropertySig {
                        name: "syscall",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "path",
                        ty: "string",
                        is_static: false,
                    },
                ],
                methods: &[MethodSig {
                    name: "toString",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                }],
                constructor: Some(&[("message", "string"), ("options", "Object")]),
            },
            ClassSig {
                name: "TypeError",
                type_params: &[],
                properties: &[
                    PropertySig {
                        name: "message",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "name",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "stack",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "cause",
                        ty: "unknown",
                        is_static: false,
                    },
                    PropertySig {
                        name: "code",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "errno",
                        ty: "number",
                        is_static: false,
                    },
                    PropertySig {
                        name: "syscall",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "path",
                        ty: "string",
                        is_static: false,
                    },
                ],
                methods: &[MethodSig {
                    name: "toString",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                }],
                constructor: Some(&[("message", "string")]),
            },
            ClassSig {
                name: "RangeError",
                type_params: &[],
                properties: &[
                    PropertySig {
                        name: "message",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "name",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "stack",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "cause",
                        ty: "unknown",
                        is_static: false,
                    },
                    PropertySig {
                        name: "code",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "errno",
                        ty: "number",
                        is_static: false,
                    },
                    PropertySig {
                        name: "syscall",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "path",
                        ty: "string",
                        is_static: false,
                    },
                ],
                methods: &[MethodSig {
                    name: "toString",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                }],
                constructor: Some(&[("message", "string")]),
            },
            ClassSig {
                name: "ReferenceError",
                type_params: &[],
                properties: &[
                    PropertySig {
                        name: "message",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "name",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "stack",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "cause",
                        ty: "unknown",
                        is_static: false,
                    },
                    PropertySig {
                        name: "code",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "errno",
                        ty: "number",
                        is_static: false,
                    },
                    PropertySig {
                        name: "syscall",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "path",
                        ty: "string",
                        is_static: false,
                    },
                ],
                methods: &[MethodSig {
                    name: "toString",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                }],
                constructor: Some(&[("message", "string")]),
            },
            ClassSig {
                name: "SyntaxError",
                type_params: &[],
                properties: &[
                    PropertySig {
                        name: "message",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "name",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "stack",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "cause",
                        ty: "unknown",
                        is_static: false,
                    },
                    PropertySig {
                        name: "code",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "errno",
                        ty: "number",
                        is_static: false,
                    },
                    PropertySig {
                        name: "syscall",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "path",
                        ty: "string",
                        is_static: false,
                    },
                ],
                methods: &[MethodSig {
                    name: "toString",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                }],
                constructor: Some(&[("message", "string")]),
            },
            ClassSig {
                name: "URIError",
                type_params: &[],
                properties: &[
                    PropertySig {
                        name: "message",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "name",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "stack",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "cause",
                        ty: "unknown",
                        is_static: false,
                    },
                    PropertySig {
                        name: "code",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "errno",
                        ty: "number",
                        is_static: false,
                    },
                    PropertySig {
                        name: "syscall",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "path",
                        ty: "string",
                        is_static: false,
                    },
                ],
                methods: &[MethodSig {
                    name: "toString",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                }],
                constructor: Some(&[("message", "string")]),
            },
            ClassSig {
                name: "EvalError",
                type_params: &[],
                properties: &[
                    PropertySig {
                        name: "message",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "name",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "stack",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "cause",
                        ty: "unknown",
                        is_static: false,
                    },
                    PropertySig {
                        name: "code",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "errno",
                        ty: "number",
                        is_static: false,
                    },
                    PropertySig {
                        name: "syscall",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "path",
                        ty: "string",
                        is_static: false,
                    },
                ],
                methods: &[MethodSig {
                    name: "toString",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                }],
                constructor: Some(&[("message", "string")]),
            },
            ClassSig {
                name: "InternalError",
                type_params: &[],
                properties: &[
                    PropertySig {
                        name: "message",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "name",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "stack",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "cause",
                        ty: "unknown",
                        is_static: false,
                    },
                    PropertySig {
                        name: "code",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "errno",
                        ty: "number",
                        is_static: false,
                    },
                    PropertySig {
                        name: "syscall",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "path",
                        ty: "string",
                        is_static: false,
                    },
                ],
                methods: &[MethodSig {
                    name: "toString",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                }],
                constructor: Some(&[("message", "string")]),
            },
            ClassSig {
                name: "AggregateError",
                type_params: &[],
                properties: &[
                    PropertySig {
                        name: "message",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "name",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "stack",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "cause",
                        ty: "unknown",
                        is_static: false,
                    },
                    PropertySig {
                        name: "code",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "errno",
                        ty: "number",
                        is_static: false,
                    },
                    PropertySig {
                        name: "syscall",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "path",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "errors",
                        ty: "Array<Error>",
                        is_static: false,
                    },
                ],
                methods: &[MethodSig {
                    name: "toString",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                }],
                constructor: Some(&[
                    ("errors", "Array<Error>"),
                    ("message", "string"),
                    ("options", "Object"),
                ]),
            },
            ClassSig {
                name: "SuppressedError",
                type_params: &[],
                properties: &[
                    PropertySig {
                        name: "name",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "error",
                        ty: "Object | string | number | boolean | null",
                        is_static: false,
                    },
                    PropertySig {
                        name: "suppressed",
                        ty: "Object | string | number | boolean | null",
                        is_static: false,
                    },
                    PropertySig {
                        name: "message",
                        ty: "string",
                        is_static: false,
                    },
                ],
                methods: &[MethodSig {
                    name: "toString",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                }],
                constructor: Some(&[
                    ("error", "Object | string | number | boolean | null"),
                    ("suppressed", "Object | string | number | boolean | null"),
                    ("message", "string"),
                ]),
            },
            ClassSig {
                name: "ChannelClosedError",
                type_params: &[],
                properties: &[
                    PropertySig {
                        name: "message",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "name",
                        ty: "string",
                        is_static: false,
                    },
                ],
                methods: &[MethodSig {
                    name: "toString",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                }],
                constructor: Some(&[("message", "string")]),
            },
            ClassSig {
                name: "AssertionError",
                type_params: &[],
                properties: &[
                    PropertySig {
                        name: "message",
                        ty: "string",
                        is_static: false,
                    },
                    PropertySig {
                        name: "name",
                        ty: "string",
                        is_static: false,
                    },
                ],
                methods: &[MethodSig {
                    name: "toString",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                }],
                constructor: Some(&[("message", "string")]),
            },
        ],
        functions: &[],
    },
    // Object
    BuiltinSignatures {
        name: "Object",
        classes: &[ClassSig {
            name: "Object",
            type_params: &[],
            properties: &[
                PropertySig {
                    name: "value",
                    ty: "Object | string | number | boolean | null",
                    is_static: false,
                },
                PropertySig {
                    name: "writable",
                    ty: "boolean",
                    is_static: false,
                },
                PropertySig {
                    name: "configurable",
                    ty: "boolean",
                    is_static: false,
                },
                PropertySig {
                    name: "enumerable",
                    ty: "boolean",
                    is_static: false,
                },
                PropertySig {
                    name: "get",
                    ty: "(() => Object) | null",
                    is_static: false,
                },
                PropertySig {
                    name: "set",
                    ty: "((Object) => void) | null",
                    is_static: false,
                },
            ],
            methods: &[
                MethodSig {
                    name: "toString",
                    params: &[],
                    min_params: 0,
                    return_type: "string",
                    is_static: false,
                },
                MethodSig {
                    name: "hashCode",
                    params: &[],
                    min_params: 0,
                    return_type: "number",
                    is_static: false,
                },
                MethodSig {
                    name: "equals",
                    params: &[("other", "Object")],
                    min_params: 1,
                    return_type: "boolean",
                    is_static: false,
                },
                MethodSig {
                    name: "hasOwnProperty",
                    params: &[("key", "string")],
                    min_params: 1,
                    return_type: "boolean",
                    is_static: false,
                },
                MethodSig {
                    name: "create",
                    params: &[("proto", "Object | null"), ("descriptors", "Object")],
                    min_params: 1,
                    return_type: "Object",
                    is_static: true,
                },
                MethodSig {
                    name: "defineProperty",
                    params: &[
                        ("obj", "Object"),
                        ("key", "string"),
                        ("descriptor", "Object"),
                    ],
                    min_params: 3,
                    return_type: "Object",
                    is_static: true,
                },
                MethodSig {
                    name: "getOwnPropertyDescriptor",
                    params: &[("obj", "Object"), ("key", "string")],
                    min_params: 2,
                    return_type: "Object | null",
                    is_static: true,
                },
                MethodSig {
                    name: "keys",
                    params: &[("obj", "Object")],
                    min_params: 1,
                    return_type: "string[]",
                    is_static: true,
                },
                MethodSig {
                    name: "defineProperties",
                    params: &[("obj", "Object"), ("descriptors", "Object")],
                    min_params: 2,
                    return_type: "Object",
                    is_static: true,
                },
                MethodSig {
                    name: "getPrototypeOf",
                    params: &[("obj", "unknown")],
                    min_params: 1,
                    return_type: "Object | null",
                    is_static: true,
                },
                MethodSig {
                    name: "setPrototypeOf",
                    params: &[("obj", "Object"), ("proto", "Object | null")],
                    min_params: 2,
                    return_type: "Object",
                    is_static: true,
                },
                MethodSig {
                    name: "isExtensible",
                    params: &[("obj", "Object")],
                    min_params: 1,
                    return_type: "boolean",
                    is_static: true,
                },
                MethodSig {
                    name: "preventExtensions",
                    params: &[("obj", "Object")],
                    min_params: 1,
                    return_type: "Object",
                    is_static: true,
                },
                MethodSig {
                    name: "is",
                    params: &[("left", "Object"), ("right", "Object")],
                    min_params: 2,
                    return_type: "boolean",
                    is_static: true,
                },
            ],
            constructor: Some(&[]),
        }],
        functions: &[],
    },
    // JSON
    BuiltinSignatures {
        name: "JSON",
        classes: &[ClassSig {
            name: "JSON",
            type_params: &[],
            properties: &[],
            methods: &[
                MethodSig {
                    name: "parse",
                    params: &[("source", "string")],
                    min_params: 1,
                    return_type: "Json",
                    is_static: true,
                },
                MethodSig {
                    name: "stringify",
                    params: &[("value", "unknown")],
                    min_params: 1,
                    return_type: "string",
                    is_static: true,
                },
            ],
            constructor: None,
        }],
        functions: &[],
    },
    // Reflect
    BuiltinSignatures {
        name: "Reflect",
        classes: &[ClassSig {
            name: "Reflect",
            type_params: &[],
            properties: &[],
            methods: &[
                MethodSig {
                    name: "get",
                    params: &[("target", "Object"), ("key", "string")],
                    min_params: 2,
                    return_type: "unknown",
                    is_static: true,
                },
                MethodSig {
                    name: "set",
                    params: &[
                        ("target", "Object"),
                        ("key", "string"),
                        ("value", "unknown"),
                    ],
                    min_params: 3,
                    return_type: "boolean",
                    is_static: true,
                },
                MethodSig {
                    name: "has",
                    params: &[("target", "Object"), ("key", "string")],
                    min_params: 2,
                    return_type: "boolean",
                    is_static: true,
                },
                MethodSig {
                    name: "getFieldNames",
                    params: &[("target", "Object")],
                    min_params: 1,
                    return_type: "string[]",
                    is_static: true,
                },
                MethodSig {
                    name: "getFieldInfo",
                    params: &[("target", "Object"), ("key", "string")],
                    min_params: 2,
                    return_type: "Object | null",
                    is_static: true,
                },
                MethodSig {
                    name: "hasMethod",
                    params: &[("target", "Object"), ("methodName", "string")],
                    min_params: 2,
                    return_type: "boolean",
                    is_static: true,
                },
                MethodSig {
                    name: "isProxy",
                    params: &[("target", "Object")],
                    min_params: 1,
                    return_type: "boolean",
                    is_static: true,
                },
                MethodSig {
                    name: "getProxyTarget",
                    params: &[("proxy", "Object")],
                    min_params: 1,
                    return_type: "Object | null",
                    is_static: true,
                },
                MethodSig {
                    name: "getProxyHandler",
                    params: &[("proxy", "Object")],
                    min_params: 1,
                    return_type: "Object | null",
                    is_static: true,
                },
                MethodSig {
                    name: "revokeProxy",
                    params: &[("proxy", "Object")],
                    min_params: 1,
                    return_type: "void",
                    is_static: true,
                },
            ],
            constructor: None,
        }],
        functions: &[],
    },
    // Proxy
    BuiltinSignatures {
        name: "Proxy",
        classes: &[ClassSig {
            name: "Proxy",
            type_params: &["T"],
            properties: &[],
            methods: &[
                MethodSig {
                    name: "isProxy",
                    params: &[],
                    min_params: 0,
                    return_type: "boolean",
                    is_static: false,
                },
                MethodSig {
                    name: "getTarget",
                    params: &[],
                    min_params: 0,
                    return_type: "T | null",
                    is_static: false,
                },
                MethodSig {
                    name: "getHandler",
                    params: &[],
                    min_params: 0,
                    return_type: "Object | null",
                    is_static: false,
                },
                MethodSig {
                    name: "revoke",
                    params: &[],
                    min_params: 0,
                    return_type: "void",
                    is_static: false,
                },
            ],
            constructor: Some(&[("target", "T"), ("handler", "Object")]),
        }],
        functions: &[],
    },
    // Number utility functions (global)
    BuiltinSignatures {
        name: "NumberUtils",
        classes: &[],
        functions: &[
            FunctionSig {
                name: "parseInt",
                type_params: &[],
                params: &[("value", "string")],
                return_type: "number",
            },
            FunctionSig {
                name: "parseFloat",
                type_params: &[],
                params: &[("value", "string")],
                return_type: "number",
            },
            FunctionSig {
                name: "isNaN",
                type_params: &[],
                params: &[("value", "number")],
                return_type: "boolean",
            },
            FunctionSig {
                name: "isFinite",
                type_params: &[],
                params: &[("value", "number")],
                return_type: "boolean",
            },
        ],
    },
    // RegExpMatch
    BuiltinSignatures {
        name: "RegExpMatch",
        classes: &[ClassSig {
            name: "RegExpMatch",
            type_params: &[],
            properties: &[
                PropertySig {
                    name: "match",
                    ty: "string",
                    is_static: false,
                },
                PropertySig {
                    name: "index",
                    ty: "number",
                    is_static: false,
                },
                PropertySig {
                    name: "input",
                    ty: "string",
                    is_static: false,
                },
            ],
            methods: &[
                MethodSig {
                    name: "group",
                    params: &[("index", "number")],
                    min_params: 1,
                    return_type: "string | null",
                    is_static: false,
                },
                MethodSig {
                    name: "groupCount",
                    params: &[],
                    min_params: 0,
                    return_type: "number",
                    is_static: false,
                },
            ],
            constructor: None,
        }],
        functions: &[],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtins_count() {
        assert!(builtin_count() > 0);
    }

    #[test]
    fn test_builtin_names_include_map() {
        assert!(builtin_names().any(|name| name == "Map"));
    }

    #[test]
    fn test_signatures_available() {
        // Signatures are always available (hardcoded)
        let sigs = get_all_signatures();
        assert!(
            !sigs.is_empty(),
            "Type signatures should always be available"
        );
    }

    #[test]
    fn test_descriptor_metadata_exported_for_dynamic_properties() {
        let sigs = to_checker_signatures();
        let map_sig = sigs
            .iter()
            .find(|s| s.name == "Map")
            .expect("Map signature");
        let map_class = map_sig
            .classes
            .iter()
            .find(|c| c.name == "Map")
            .expect("Map class signature");
        let size_prop = map_class
            .properties
            .iter()
            .find(|p| p.name == "size")
            .expect("Map.size property");
        let descriptor = size_prop
            .descriptor
            .as_ref()
            .expect("Map.size should include descriptor metadata");
        assert_eq!(descriptor.writable, Some(false));
        assert!(descriptor.has_getter);
        assert!(!descriptor.has_setter);
    }
}
