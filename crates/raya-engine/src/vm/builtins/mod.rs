//! Raya Builtin Types and Handlers
//!
//! This module provides:
//! - Precompiled bytecode and type signatures for Raya's builtin types
//! - Native method handlers for built-in types (arrays, strings, numbers, etc.)
//!
//! The builtins are compiled at build time and embedded in the library.
//! The handlers implement native operations called from the VM.
//!
//! # Usage
//!
//! ```rust,ignore
//! use raya_engine::vm::builtins::{get_builtin, builtin_names, get_all_signatures};
//!
//! // Get a specific builtin module
//! if let Some(module) = get_builtin("Map") {
//!     // Use the compiled bytecode module
//! }
//!
//! // Get type signatures for type checking
//! let signatures = get_all_signatures();
//! ```

// Handler modules for built-in type methods
pub mod handlers;

use crate::compiler::Module;
use std::sync::OnceLock;

/// A precompiled builtin module
pub struct BuiltinModule {
    /// Name of the builtin (e.g., "Map", "Set")
    pub name: &'static str,
    /// Raw bytecode bytes
    pub bytecode: &'static [u8],
}

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

// Include the generated index (from build.rs)
include!(concat!(env!("OUT_DIR"), "/builtins_index.rs"));

/// Cache for decoded modules
static DECODED_CACHE: OnceLock<Vec<(&'static str, Module)>> = OnceLock::new();

/// Get all decoded builtin modules
///
/// This decodes the bytecode on first access and caches the result.
pub fn get_all_builtins() -> &'static [(&'static str, Module)] {
    DECODED_CACHE.get_or_init(|| {
        let mut modules = Vec::new();

        for builtin in BUILTINS {
            match Module::decode(builtin.bytecode) {
                Ok(module) => {
                    modules.push((builtin.name, module));
                }
                Err(e) => {
                    eprintln!(
                        "Warning: Failed to decode builtin '{}': {:?}",
                        builtin.name, e
                    );
                }
            }
        }

        modules
    })
}

/// Get a specific builtin module by name
///
/// Returns `None` if the builtin doesn't exist or failed to decode.
pub fn get_builtin(name: &str) -> Option<&'static Module> {
    get_all_builtins()
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, m)| m)
}

/// Get the raw bytecode for a builtin (without decoding)
///
/// This is useful if you want to handle decoding yourself.
pub fn get_builtin_bytecode(name: &str) -> Option<&'static [u8]> {
    BUILTINS.iter().find(|b| b.name == name).map(|b| b.bytecode)
}

/// List all available builtin names
pub fn builtin_names() -> impl Iterator<Item = &'static str> {
    BUILTINS.iter().map(|b| b.name)
}

/// Get the number of available builtins
pub fn builtin_count() -> usize {
    BUILTINS.len()
}

// ============================================================================
// Builtin Type Signatures
// ============================================================================

/// Get all builtin type signatures for injection into the type checker
pub fn get_all_signatures() -> &'static [BuiltinSignatures] {
    BUILTIN_SIGS
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
    BUILTIN_SIGS
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
        .collect()
}

static BUILTIN_SIGS: &[BuiltinSignatures] = &[
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
            properties: &[],
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
                    name: "keyFor",
                    params: &[("sym", "Symbol")],
                    min_params: 1,
                    return_type: "string",
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
            ],
            constructor: Some(&[]),
        }],
        functions: &[
            FunctionSig {
                name: "dateNow",
                type_params: &[],
                params: &[],
                return_type: "number",
            },
            FunctionSig {
                name: "dateParse",
                type_params: &[],
                params: &[("str", "string")],
                return_type: "number",
            },
        ],
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
                    return_type: "T",
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
                    return_type: "T | null",
                    is_static: false,
                },
            ],
            constructor: Some(&[("value", "T")]),
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
                    params: &[("onFulfilled", "(T) => Object")],
                    min_params: 1,
                    return_type: "Promise<Object>",
                    is_static: false,
                },
                MethodSig {
                    name: "catch",
                    params: &[("onRejected", "(Object) => Object")],
                    min_params: 1,
                    return_type: "Promise<Object>",
                    is_static: false,
                },
                MethodSig {
                    name: "finally",
                    params: &[("onFinally", "() => void")],
                    min_params: 1,
                    return_type: "Promise<T>",
                    is_static: false,
                },
                MethodSig {
                    name: "resolve",
                    params: &[("value", "Object")],
                    min_params: 1,
                    return_type: "Promise<Object>",
                    is_static: true,
                },
                MethodSig {
                    name: "reject",
                    params: &[("reason", "Object")],
                    min_params: 1,
                    return_type: "Promise<Object>",
                    is_static: true,
                },
                MethodSig {
                    name: "all",
                    params: &[("values", "Array<Promise<Object>>")],
                    min_params: 1,
                    return_type: "Promise<Array<Object>>",
                    is_static: true,
                },
                MethodSig {
                    name: "race",
                    params: &[("values", "Array<Promise<Object>>")],
                    min_params: 1,
                    return_type: "Promise<Object>",
                    is_static: true,
                },
            ],
            constructor: None, // Promises are created via async keyword
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
                        name: "cause",
                        ty: "Object | null",
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
                        name: "errors",
                        ty: "Object[]",
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
                constructor: Some(&[("errors", "Object[]"), ("message", "string")]),
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
            properties: &[],
            methods: &[MethodSig {
                name: "toString",
                params: &[],
                min_params: 0,
                return_type: "string",
                is_static: false,
            }],
            constructor: Some(&[]),
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
        // Builtins may be empty during development (not precompiled yet)
        // This test verifies the API works, not that builtins are populated
        let _count = builtin_count(); // usize is always non-negative
    }

    #[test]
    fn test_get_builtin() {
        // Builtins may be empty during development
        let names: Vec<_> = builtin_names().collect();

        // If builtins are available, verify we can get them
        if let Some(name) = names.first() {
            let module = get_builtin(name);
            assert!(module.is_some(), "Should be able to get builtin '{}'", name);
        }
        // Otherwise, test passes (builtins not precompiled yet)
    }

    #[test]
    fn test_nonexistent_builtin() {
        assert!(get_builtin("NonExistent").is_none());
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
}
