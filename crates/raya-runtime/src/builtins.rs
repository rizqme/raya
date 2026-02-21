//! Builtin and standard library source loading.
//!
//! Embeds the Raya builtin class sources and standard library module sources
//! at compile time, so the runtime can prepend them to user code.

/// Returns the source code for all builtin classes (Object, Error, Map, Set, etc.)
pub fn builtin_sources() -> &'static str {
    concat!(
        include_str!("../../raya-engine/builtins/Object.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/Error.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/Map.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/Set.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/Buffer.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/Date.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/Channel.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/Mutex.raya"),
        "\n",
        include_str!("../../raya-engine/builtins/Task.raya"),
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
        include_str!("../../raya-stdlib-posix/raya/glob.raya"),
        "\n",
        // Package manager
        include_str!("../../raya-stdlib-posix/raya/pm.raya"),
        "\n",
    )
}
