//! Builtin and standard library source loading.
//!
//! Embeds the Raya builtin class sources and standard library module sources
//! at compile time, so the runtime can prepend them to user code.

/// Returns the source code for all builtin classes (Object, Error, Map, Set, etc.)
pub fn builtin_sources() -> &'static str {
    concat!(
        include_str!("../../raya-engine/builtins/object.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/error.raya"),
        "\n",
        // Map, Set, Buffer are //@@builtin_native — still prepended for type
        // definitions (used by stdlib), but dispatch goes through CallMethod
        include_str!("../../raya-engine/builtins/map.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/set.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/buffer.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/date.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/channel.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/mutex.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/task.raya"),
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
        // POSIX stdlib
        include_str!("../../raya-stdlib-posix/raya/fs.raya"),
        "\n",
        include_str!("../../raya-stdlib-posix/raya/net.raya"),
        "\n",
        include_str!("../../raya-stdlib-posix/raya/http.raya"),
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
        // Additional stdlib (needed by pm)
        include_str!("../../raya-stdlib/raya/encoding.raya"),
        "\n",
        include_str!("../../raya-stdlib/raya/semver.raya"),
        "\n",
        include_str!("../../raya-stdlib-posix/raya/archive.raya"),
        "\n",
        // Package manager
        include_str!("../../raya-stdlib-posix/raya/pm.raya"),
        "\n",
    )
}
