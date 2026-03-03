//! Builtin and standard library source loading.
//!
//! Embeds the Raya builtin class sources and standard library module sources
//! at compile time, so the runtime can prepend them to user code.

use crate::BuiltinMode;

/// Returns the source code for all builtin classes (Object, Error, Map, Set, etc.)
pub fn builtin_sources() -> &'static str {
    builtin_sources_for_mode(BuiltinMode::RayaStrict)
}

/// Returns builtin sources for a specific compatibility mode.
pub fn builtin_sources_for_mode(mode: BuiltinMode) -> &'static str {
    match mode {
        BuiltinMode::RayaStrict => strict_builtin_sources(),
        BuiltinMode::NodeCompat => node_compat_builtin_sources(),
    }
}

fn strict_builtin_sources() -> &'static str {
    concat!(
        include_str!("../../raya-engine/builtins/strict/array.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/strict/regexp.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/strict/object.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/strict/error.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/strict/symbol.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/strict/globals.shared.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/strict/map.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/strict/set.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/strict/buffer.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/strict/date.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/strict/channel.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/strict/mutex.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/strict/promise.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/strict/event_emitter.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/strict/iterator.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/strict/temporal.raya"),
        "\n",
    )
}

fn node_compat_builtin_sources() -> &'static str {
    concat!(
        include_str!("../../raya-engine/builtins/strict/array.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/strict/regexp.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/node_compat/object.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/node_compat/error.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/node_compat/symbol.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/node_compat/globals.shared.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/node_compat/map.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/node_compat/set.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/node_compat/buffer.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/node_compat/date.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/node_compat/channel.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/node_compat/mutex.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/node_compat/promise.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/node_compat/event_emitter.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/node_compat/iterator.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/node_compat/temporal.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/node_compat/typedarray.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/node_compat/atomics.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/node_compat/dataview.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/node_compat/globals.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/node_compat/function_families.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/node_compat/disposal.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/node_compat/intl.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/node_compat/weak_collections.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/node_compat/weak_refs.raya"),
        "\n",
    )
}

/// Returns the source code for all standard library modules.
pub fn std_sources() -> &'static str {
    concat!(
        // Core stdlib
        include_str!("../../raya-stdlib/raya/logger.raya"),
        "\n",
        include_str!("../../raya-stdlib/raya/math.raya"),
        "\n",
        include_str!("../../raya-stdlib/raya/reflect.raya"),
        "\n",
        include_str!("../../raya-stdlib/raya/runtime.raya"),
        "\n",
        include_str!("../../raya-stdlib/raya/crypto.raya"),
        "\n",
        include_str!("../../raya-stdlib/raya/time.raya"),
        "\n",
        include_str!("../../raya-stdlib/raya/path.raya"),
        "\n",
        include_str!("../../raya-stdlib/raya/stream.raya"),
        "\n",
        include_str!("../../raya-stdlib/raya/compress.raya"),
        "\n",
        include_str!("../../raya-stdlib/raya/url.raya"),
        "\n",
        include_str!("../../raya-stdlib/raya/args.raya"),
        "\n",
        include_str!("../../raya-stdlib/raya/encoding.raya"),
        "\n",
        include_str!("../../raya-stdlib/raya/semver.raya"),
        "\n",
        include_str!("../../raya-stdlib/raya/template.raya"),
        "\n",
        // POSIX stdlib
        include_str!("../../raya-stdlib-posix/raya/fs.raya"),
        "\n",
        include_str!("../../raya-stdlib-posix/raya/net.raya"),
        "\n",
        include_str!("../../raya-stdlib-posix/raya/http.raya"),
        "\n",
        include_str!("../../raya-stdlib-posix/raya/http2.raya"),
        "\n",
        include_str!("../../raya-stdlib-posix/raya/fetch.raya"),
        "\n",
        include_str!("../../raya-stdlib-posix/raya/env.raya"),
        "\n",
        include_str!("../../raya-stdlib-posix/raya/process.raya"),
        "\n",
        include_str!("../../raya-stdlib-posix/raya/os.raya"),
        "\n",
        include_str!("../../raya-stdlib-posix/raya/io.raya"),
        "\n",
        include_str!("../../raya-stdlib-posix/raya/dns.raya"),
        "\n",
        include_str!("../../raya-stdlib-posix/raya/terminal.raya"),
        "\n",
        include_str!("../../raya-stdlib-posix/raya/ws.raya"),
        "\n",
        include_str!("../../raya-stdlib-posix/raya/readline.raya"),
        "\n",
        include_str!("../../raya-stdlib-posix/raya/glob.raya"),
        "\n",
        include_str!("../../raya-stdlib-posix/raya/archive.raya"),
        "\n",
        include_str!("../../raya-stdlib-posix/raya/watch.raya"),
        "\n",
        // Package manager
        include_str!("../../raya-stdlib-posix/raya/pm.raya"),
        "\n",
    )
}
