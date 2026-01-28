//! Precompiled Raya Builtin Types
//!
//! This crate provides precompiled bytecode and type signatures for Raya's builtin types.
//! The builtins are compiled at build time and embedded in the library.
//!
//! # Usage
//!
//! ```rust,ignore
//! use raya_builtins::{get_builtin, builtin_names, get_builtin_signatures};
//!
//! // Get a specific builtin module
//! if let Some(module) = get_builtin("Map") {
//!     // Use the compiled bytecode module
//! }
//!
//! // Get type signatures for type checking
//! let signatures = get_builtin_signatures();
//! ```

use raya_compiler::Module;
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
    pub name: &'static str,
    pub params: &'static [(&'static str, &'static str)], // (name, type)
    pub return_type: &'static str,
    pub is_static: bool,
}

/// Type signature for a builtin class property
#[derive(Debug, Clone)]
pub struct PropertySig {
    pub name: &'static str,
    pub ty: &'static str,
    pub is_static: bool,
}

/// Type signature for a builtin class
#[derive(Debug, Clone)]
pub struct ClassSig {
    pub name: &'static str,
    pub type_params: &'static [&'static str],
    pub properties: &'static [PropertySig],
    pub methods: &'static [MethodSig],
    pub constructor: Option<&'static [(&'static str, &'static str)]>, // params
}

/// Type signature for a builtin function
#[derive(Debug, Clone)]
pub struct FunctionSig {
    pub name: &'static str,
    pub type_params: &'static [&'static str],
    pub params: &'static [(&'static str, &'static str)],
    pub return_type: &'static str,
}

/// All type signatures for a builtin module
#[derive(Debug, Clone)]
pub struct BuiltinSignatures {
    pub name: &'static str,
    pub classes: &'static [ClassSig],
    pub functions: &'static [FunctionSig],
}

// Include the generated index
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
    BUILTINS
        .iter()
        .find(|b| b.name == name)
        .map(|b| b.bytecode)
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
    &BUILTIN_SIGS
}

/// Get signatures for a specific builtin
pub fn get_signatures(name: &str) -> Option<&'static BuiltinSignatures> {
    BUILTIN_SIGS.iter().find(|s| s.name == name)
}

/// Convert static signatures to the checker's BuiltinSignatures format
///
/// This converts the static &'static str signatures to owned Strings
/// for use with the raya-parser type checker.
pub fn to_checker_signatures() -> Vec<raya_parser::checker::BuiltinSignatures> {
    BUILTIN_SIGS.iter().map(|sig| {
        raya_parser::checker::BuiltinSignatures {
            name: sig.name.to_string(),
            classes: sig.classes.iter().map(|c| {
                raya_parser::checker::BuiltinClass {
                    name: c.name.to_string(),
                    type_params: c.type_params.iter().map(|s| s.to_string()).collect(),
                    properties: c.properties.iter().map(|p| {
                        raya_parser::checker::BuiltinProperty {
                            name: p.name.to_string(),
                            ty: p.ty.to_string(),
                            is_static: p.is_static,
                        }
                    }).collect(),
                    methods: c.methods.iter().map(|m| {
                        raya_parser::checker::BuiltinMethod {
                            name: m.name.to_string(),
                            params: m.params.iter().map(|(n, t)| (n.to_string(), t.to_string())).collect(),
                            return_type: m.return_type.to_string(),
                            is_static: m.is_static,
                        }
                    }).collect(),
                    constructor_params: c.constructor.map(|params| {
                        params.iter().map(|(n, t)| (n.to_string(), t.to_string())).collect()
                    }),
                }
            }).collect(),
            functions: sig.functions.iter().map(|f| {
                raya_parser::checker::BuiltinFunction {
                    name: f.name.to_string(),
                    type_params: f.type_params.iter().map(|s| s.to_string()).collect(),
                    params: f.params.iter().map(|(n, t)| (n.to_string(), t.to_string())).collect(),
                    return_type: f.return_type.to_string(),
                }
            }).collect(),
        }
    }).collect()
}

static BUILTIN_SIGS: &[BuiltinSignatures] = &[
    // Map<K, V>
    BuiltinSignatures {
        name: "Map",
        classes: &[ClassSig {
            name: "Map",
            type_params: &["K", "V"],
            properties: &[],
            methods: &[
                MethodSig { name: "size", params: &[], return_type: "number", is_static: false },
                MethodSig { name: "get", params: &[("key", "K")], return_type: "V | null", is_static: false },
                MethodSig { name: "set", params: &[("key", "K"), ("value", "V")], return_type: "void", is_static: false },
                MethodSig { name: "has", params: &[("key", "K")], return_type: "boolean", is_static: false },
                MethodSig { name: "delete", params: &[("key", "K")], return_type: "boolean", is_static: false },
                MethodSig { name: "clear", params: &[], return_type: "void", is_static: false },
                MethodSig { name: "keys", params: &[], return_type: "Array<K>", is_static: false },
                MethodSig { name: "values", params: &[], return_type: "Array<V>", is_static: false },
                MethodSig { name: "entries", params: &[], return_type: "Array<[K, V]>", is_static: false },
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
            properties: &[],
            methods: &[
                MethodSig { name: "size", params: &[], return_type: "number", is_static: false },
                MethodSig { name: "add", params: &[("value", "T")], return_type: "void", is_static: false },
                MethodSig { name: "has", params: &[("value", "T")], return_type: "boolean", is_static: false },
                MethodSig { name: "delete", params: &[("value", "T")], return_type: "boolean", is_static: false },
                MethodSig { name: "clear", params: &[], return_type: "void", is_static: false },
                MethodSig { name: "values", params: &[], return_type: "Array<T>", is_static: false },
                MethodSig { name: "union", params: &[("other", "Set<T>")], return_type: "Set<T>", is_static: false },
                MethodSig { name: "intersection", params: &[("other", "Set<T>")], return_type: "Set<T>", is_static: false },
                MethodSig { name: "difference", params: &[("other", "Set<T>")], return_type: "Set<T>", is_static: false },
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
            properties: &[],
            methods: &[
                MethodSig { name: "length", params: &[], return_type: "number", is_static: false },
                MethodSig { name: "getByte", params: &[("offset", "number")], return_type: "number", is_static: false },
                MethodSig { name: "setByte", params: &[("offset", "number"), ("value", "number")], return_type: "void", is_static: false },
                MethodSig { name: "getInt32", params: &[("offset", "number")], return_type: "number", is_static: false },
                MethodSig { name: "setInt32", params: &[("offset", "number"), ("value", "number")], return_type: "void", is_static: false },
                MethodSig { name: "getFloat64", params: &[("offset", "number")], return_type: "number", is_static: false },
                MethodSig { name: "setFloat64", params: &[("offset", "number"), ("value", "number")], return_type: "void", is_static: false },
                MethodSig { name: "slice", params: &[("start", "number"), ("end", "number")], return_type: "Buffer", is_static: false },
                MethodSig { name: "copy", params: &[("target", "Buffer"), ("targetStart", "number"), ("sourceStart", "number"), ("sourceEnd", "number")], return_type: "number", is_static: false },
                MethodSig { name: "toString", params: &[("encoding", "string")], return_type: "string", is_static: false },
            ],
            constructor: Some(&[("size", "number")]),
        }],
        functions: &[
            FunctionSig { name: "bufferFromString", type_params: &[], params: &[("str", "string"), ("encoding", "string")], return_type: "Buffer" },
            FunctionSig { name: "bufferFromUtf8", type_params: &[], params: &[("str", "string")], return_type: "Buffer" },
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
                MethodSig { name: "getTime", params: &[], return_type: "number", is_static: false },
                MethodSig { name: "getFullYear", params: &[], return_type: "number", is_static: false },
                MethodSig { name: "getMonth", params: &[], return_type: "number", is_static: false },
                MethodSig { name: "getDate", params: &[], return_type: "number", is_static: false },
                MethodSig { name: "getDay", params: &[], return_type: "number", is_static: false },
                MethodSig { name: "getHours", params: &[], return_type: "number", is_static: false },
                MethodSig { name: "getMinutes", params: &[], return_type: "number", is_static: false },
                MethodSig { name: "getSeconds", params: &[], return_type: "number", is_static: false },
                MethodSig { name: "getMilliseconds", params: &[], return_type: "number", is_static: false },
                MethodSig { name: "setFullYear", params: &[("year", "number")], return_type: "void", is_static: false },
                MethodSig { name: "setMonth", params: &[("month", "number")], return_type: "void", is_static: false },
                MethodSig { name: "setDate", params: &[("date", "number")], return_type: "void", is_static: false },
                MethodSig { name: "setHours", params: &[("hours", "number")], return_type: "void", is_static: false },
                MethodSig { name: "setMinutes", params: &[("minutes", "number")], return_type: "void", is_static: false },
                MethodSig { name: "setSeconds", params: &[("seconds", "number")], return_type: "void", is_static: false },
                MethodSig { name: "setMilliseconds", params: &[("ms", "number")], return_type: "void", is_static: false },
                MethodSig { name: "toString", params: &[], return_type: "string", is_static: false },
                MethodSig { name: "toISOString", params: &[], return_type: "string", is_static: false },
                MethodSig { name: "toDateString", params: &[], return_type: "string", is_static: false },
                MethodSig { name: "toTimeString", params: &[], return_type: "string", is_static: false },
            ],
            constructor: Some(&[]),
        }],
        functions: &[
            FunctionSig { name: "dateNow", type_params: &[], params: &[], return_type: "number" },
            FunctionSig { name: "dateParse", type_params: &[], params: &[("str", "string")], return_type: "number" },
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
                MethodSig { name: "send", params: &[("value", "T")], return_type: "void", is_static: false },
                MethodSig { name: "receive", params: &[], return_type: "T", is_static: false },
                MethodSig { name: "trySend", params: &[("value", "T")], return_type: "boolean", is_static: false },
                MethodSig { name: "tryReceive", params: &[], return_type: "T | null", is_static: false },
                MethodSig { name: "close", params: &[], return_type: "void", is_static: false },
                MethodSig { name: "isClosed", params: &[], return_type: "boolean", is_static: false },
                MethodSig { name: "length", params: &[], return_type: "number", is_static: false },
                MethodSig { name: "capacity", params: &[], return_type: "number", is_static: false },
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
                MethodSig { name: "lock", params: &[], return_type: "T", is_static: false },
                MethodSig { name: "unlock", params: &[], return_type: "void", is_static: false },
                MethodSig { name: "tryLock", params: &[], return_type: "T | null", is_static: false },
            ],
            constructor: Some(&[("value", "T")]),
        }],
        functions: &[],
    },
    // Task<T>
    BuiltinSignatures {
        name: "Task",
        classes: &[ClassSig {
            name: "Task",
            type_params: &["T"],
            properties: &[],
            methods: &[
                MethodSig { name: "cancel", params: &[], return_type: "void", is_static: false },
            ],
            constructor: None, // Tasks are created via async keyword
        }],
        functions: &[
            FunctionSig { name: "taskYield", type_params: &[], params: &[], return_type: "void" },
            FunctionSig { name: "taskSleep", type_params: &[], params: &[("durationMs", "number")], return_type: "void" },
        ],
    },
    // Error classes
    BuiltinSignatures {
        name: "Error",
        classes: &[
            ClassSig {
                name: "Error",
                type_params: &[],
                properties: &[
                    PropertySig { name: "message", ty: "string", is_static: false },
                ],
                methods: &[
                    MethodSig { name: "toString", params: &[], return_type: "string", is_static: false },
                ],
                constructor: Some(&[("message", "string")]),
            },
            ClassSig {
                name: "TypeError",
                type_params: &[],
                properties: &[
                    PropertySig { name: "message", ty: "string", is_static: false },
                ],
                methods: &[
                    MethodSig { name: "toString", params: &[], return_type: "string", is_static: false },
                ],
                constructor: Some(&[("message", "string")]),
            },
            ClassSig {
                name: "RangeError",
                type_params: &[],
                properties: &[
                    PropertySig { name: "message", ty: "string", is_static: false },
                ],
                methods: &[
                    MethodSig { name: "toString", params: &[], return_type: "string", is_static: false },
                ],
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
            methods: &[
                MethodSig { name: "toString", params: &[], return_type: "string", is_static: false },
            ],
            constructor: Some(&[]),
        }],
        functions: &[],
    },
    // RegExpMatch
    BuiltinSignatures {
        name: "RegExpMatch",
        classes: &[ClassSig {
            name: "RegExpMatch",
            type_params: &[],
            properties: &[
                PropertySig { name: "match", ty: "string", is_static: false },
                PropertySig { name: "index", ty: "number", is_static: false },
                PropertySig { name: "input", ty: "string", is_static: false },
            ],
            methods: &[
                MethodSig { name: "group", params: &[("index", "number")], return_type: "string | null", is_static: false },
                MethodSig { name: "groupCount", params: &[], return_type: "number", is_static: false },
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
    fn test_builtins_available() {
        assert!(builtin_count() > 0, "Should have at least one builtin");
    }

    #[test]
    fn test_get_builtin() {
        // At least some builtins should be available
        let names: Vec<_> = builtin_names().collect();
        assert!(!names.is_empty());

        // Try to get the first available builtin
        if let Some(name) = names.first() {
            let module = get_builtin(name);
            assert!(module.is_some(), "Should be able to get builtin '{}'", name);
        }
    }

    #[test]
    fn test_nonexistent_builtin() {
        assert!(get_builtin("NonExistent").is_none());
    }
}
