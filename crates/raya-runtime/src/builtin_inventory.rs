#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuiltinInventoryMode {
    RayaStrict,
    NodeCompat,
}

pub fn builtin_logical_paths(mode: BuiltinInventoryMode) -> &'static [&'static str] {
    match mode {
        BuiltinInventoryMode::RayaStrict => STRICT_BUILTIN_LOGICAL_PATHS,
        BuiltinInventoryMode::NodeCompat => NODE_COMPAT_BUILTIN_LOGICAL_PATHS,
    }
}

pub const STRICT_BUILTIN_LOGICAL_PATHS: &[&str] = &[
    "strict/object.raya",
    "strict/symbol.raya",
    "strict/globals.shared.raya",
    "strict/error.raya",
    "strict/array.raya",
    "strict/regexp.raya",
    "strict/map.raya",
    "strict/set.raya",
    "strict/buffer.raya",
    "strict/date.raya",
    "strict/channel.raya",
    "strict/mutex.raya",
    "strict/promise.raya",
    "strict/event_emitter.raya",
    "strict/iterator.raya",
    "strict/temporal.raya",
];

pub const NODE_COMPAT_BUILTIN_LOGICAL_PATHS: &[&str] = &[
    "node_compat/object.raya",
    "node_compat/symbol.raya",
    "node_compat/globals.shared.raya",
    "node_compat/error.raya",
    "node_compat/function_families.raya",
    "node_compat/globals.raya",
    "strict/array.raya",
    "strict/regexp.raya",
    "node_compat/map.raya",
    "node_compat/set.raya",
    "node_compat/buffer.raya",
    "node_compat/date.raya",
    "node_compat/channel.raya",
    "node_compat/mutex.raya",
    "node_compat/promise.raya",
    "node_compat/event_emitter.raya",
    "node_compat/iterator.raya",
    "node_compat/temporal.raya",
    "node_compat/typedarray.raya",
    "node_compat/atomics.raya",
    "node_compat/dataview.raya",
    "node_compat/disposal.raya",
    "node_compat/intl.raya",
    "node_compat/weak_collections.raya",
    "node_compat/weak_refs.raya",
];
