//! Builtin source inventory.
//!
//! Builtins are tracked as individual source modules with stable logical paths so
//! the runtime can bind declarations first, then compile/load each builtin file
//! with preserved identity.

use crate::BuiltinMode;

/// Returns builtin source modules in deterministic load order, preserving file identity.
///
/// Tuple format: `(logical_path, source)`.
pub fn builtin_source_modules_for_mode(
    mode: BuiltinMode,
) -> &'static [(&'static str, &'static str)] {
    match mode {
        BuiltinMode::RayaStrict => strict_builtin_source_modules(),
        BuiltinMode::NodeCompat => node_compat_builtin_source_modules(),
    }
}

fn strict_builtin_source_modules() -> &'static [(&'static str, &'static str)] {
    &[
        (
            "strict/object.raya",
            include_str!("../../raya-engine/builtins/strict/object.raya"),
        ),
        (
            "strict/symbol.raya",
            include_str!("../../raya-engine/builtins/strict/symbol.raya"),
        ),
        (
            "strict/globals.shared.raya",
            include_str!("../../raya-engine/builtins/strict/globals.shared.raya"),
        ),
        (
            "strict/error.raya",
            include_str!("../../raya-engine/builtins/strict/error.raya"),
        ),
        (
            "strict/array.raya",
            include_str!("../../raya-engine/builtins/strict/array.raya"),
        ),
        (
            "strict/regexp.raya",
            include_str!("../../raya-engine/builtins/strict/regexp.raya"),
        ),
        (
            "strict/map.raya",
            include_str!("../../raya-engine/builtins/strict/map.raya"),
        ),
        (
            "strict/set.raya",
            include_str!("../../raya-engine/builtins/strict/set.raya"),
        ),
        (
            "strict/buffer.raya",
            include_str!("../../raya-engine/builtins/strict/buffer.raya"),
        ),
        (
            "strict/date.raya",
            include_str!("../../raya-engine/builtins/strict/date.raya"),
        ),
        (
            "strict/channel.raya",
            include_str!("../../raya-engine/builtins/strict/channel.raya"),
        ),
        (
            "strict/mutex.raya",
            include_str!("../../raya-engine/builtins/strict/mutex.raya"),
        ),
        (
            "strict/promise.raya",
            include_str!("../../raya-engine/builtins/strict/promise.raya"),
        ),
        (
            "strict/event_emitter.raya",
            include_str!("../../raya-engine/builtins/strict/event_emitter.raya"),
        ),
        (
            "strict/iterator.raya",
            include_str!("../../raya-engine/builtins/strict/iterator.raya"),
        ),
        (
            "strict/temporal.raya",
            include_str!("../../raya-engine/builtins/strict/temporal.raya"),
        ),
    ]
}

fn node_compat_builtin_source_modules() -> &'static [(&'static str, &'static str)] {
    &[
        (
            "node_compat/object.raya",
            include_str!("../../raya-engine/builtins/node_compat/object.raya"),
        ),
        (
            "node_compat/symbol.raya",
            include_str!("../../raya-engine/builtins/node_compat/symbol.raya"),
        ),
        (
            "node_compat/globals.shared.raya",
            include_str!("../../raya-engine/builtins/node_compat/globals.shared.raya"),
        ),
        (
            "node_compat/error.raya",
            include_str!("../../raya-engine/builtins/node_compat/error.raya"),
        ),
        (
            "node_compat/function_families.raya",
            include_str!("../../raya-engine/builtins/node_compat/function_families.raya"),
        ),
        (
            "node_compat/globals.raya",
            include_str!("../../raya-engine/builtins/node_compat/globals.raya"),
        ),
        (
            "strict/array.raya",
            include_str!("../../raya-engine/builtins/strict/array.raya"),
        ),
        (
            "strict/regexp.raya",
            include_str!("../../raya-engine/builtins/strict/regexp.raya"),
        ),
        (
            "node_compat/map.raya",
            include_str!("../../raya-engine/builtins/node_compat/map.raya"),
        ),
        (
            "node_compat/set.raya",
            include_str!("../../raya-engine/builtins/node_compat/set.raya"),
        ),
        (
            "node_compat/buffer.raya",
            include_str!("../../raya-engine/builtins/node_compat/buffer.raya"),
        ),
        (
            "node_compat/date.raya",
            include_str!("../../raya-engine/builtins/node_compat/date.raya"),
        ),
        (
            "node_compat/channel.raya",
            include_str!("../../raya-engine/builtins/node_compat/channel.raya"),
        ),
        (
            "node_compat/mutex.raya",
            include_str!("../../raya-engine/builtins/node_compat/mutex.raya"),
        ),
        (
            "node_compat/promise.raya",
            include_str!("../../raya-engine/builtins/node_compat/promise.raya"),
        ),
        (
            "node_compat/event_emitter.raya",
            include_str!("../../raya-engine/builtins/node_compat/event_emitter.raya"),
        ),
        (
            "node_compat/iterator.raya",
            include_str!("../../raya-engine/builtins/node_compat/iterator.raya"),
        ),
        (
            "node_compat/temporal.raya",
            include_str!("../../raya-engine/builtins/node_compat/temporal.raya"),
        ),
        (
            "node_compat/typedarray.raya",
            include_str!("../../raya-engine/builtins/node_compat/typedarray.raya"),
        ),
        (
            "node_compat/atomics.raya",
            include_str!("../../raya-engine/builtins/node_compat/atomics.raya"),
        ),
        (
            "node_compat/dataview.raya",
            include_str!("../../raya-engine/builtins/node_compat/dataview.raya"),
        ),
        (
            "node_compat/disposal.raya",
            include_str!("../../raya-engine/builtins/node_compat/disposal.raya"),
        ),
        (
            "node_compat/intl.raya",
            include_str!("../../raya-engine/builtins/node_compat/intl.raya"),
        ),
        (
            "node_compat/weak_collections.raya",
            include_str!("../../raya-engine/builtins/node_compat/weak_collections.raya"),
        ),
        (
            "node_compat/weak_refs.raya",
            include_str!("../../raya-engine/builtins/node_compat/weak_refs.raya"),
        ),
    ]
}
