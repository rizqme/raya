//! Standard library module registry
//!
//! Maps `std:xxx` import specifiers to embedded module source code.
//! Standard library modules are built into the compiler and don't require
//! external files or package resolution.

use std::collections::HashMap;

const NODE_STD_ALIASES: [(&str, &str); 39] = [
    ("fs", "__node_fs"),
    ("fs/promises", "__node_fs_promises"),
    ("path", "__node_path"),
    ("os", "__node_os"),
    ("process", "__node_process"),
    ("dns", "dns"),
    ("net", "net"),
    ("http", "http"),
    ("https", "__node_https"),
    ("crypto", "__node_crypto"),
    ("url", "__node_url"),
    ("stream", "stream"),
    ("events", "__node_events"),
    ("assert", "__node_assert"),
    ("assert/strict", "__node_assert"),
    ("util", "__node_util"),
    ("module", "__node_module"),
    ("child_process", "__node_child_process"),
    ("test", "__node_test"),
    ("test/reporters", "__node_test_reporters"),
    ("timers", "__node_timers"),
    ("timers/promises", "__node_timers_promises"),
    ("buffer", "__node_buffer"),
    ("string_decoder", "__node_string_decoder"),
    ("stream/promises", "__node_stream_promises"),
    ("stream/web", "__node_stream_web"),
    ("worker_threads", "__node_worker_threads"),
    ("vm", "__node_vm"),
    ("http2", "http2"),
    ("inspector", "__node_inspector"),
    ("inspector/promises", "__node_inspector_promises"),
    ("async_hooks", "__node_async_hooks"),
    ("diagnostics_channel", "__node_diagnostics_channel"),
    ("v8", "__node_v8"),
    ("dgram", "__node_dgram"),
    ("cluster", "__node_cluster"),
    ("repl", "__node_repl"),
    ("perf_hooks", "__node_perf_hooks"),
    ("sqlite", "sqlite"),
];

/// Registry of standard library modules
///
/// Standard library modules use the `std:` namespace prefix and are
/// resolved by looking up their embedded source code rather than
/// going through the file system or package resolution.
pub struct StdModuleRegistry {
    /// Map from module name (without `std:` prefix) to source code
    modules: HashMap<&'static str, &'static str>,
    /// Map from Node core module names (without `node:` prefix) to canonical
    /// stdlib module names.
    node_aliases: HashMap<&'static str, &'static str>,
}

impl StdModuleRegistry {
    /// Create a new registry with all standard library modules
    pub fn new() -> Self {
        let mut modules = HashMap::new();

        // Register standard library modules
        modules.insert(
            "logger",
            include_str!("../../../../raya-stdlib/raya/logger.raya"),
        );
        modules.insert(
            "math",
            include_str!("../../../../raya-stdlib/raya/math.raya"),
        );
        modules.insert(
            "reflect",
            include_str!("../../../../raya-stdlib/raya/reflect.raya"),
        );
        modules.insert(
            "runtime",
            include_str!("../../../../raya-stdlib/raya/runtime.raya"),
        );
        modules.insert(
            "crypto",
            include_str!("../../../../raya-stdlib/raya/crypto.raya"),
        );
        modules.insert(
            "time",
            include_str!("../../../../raya-stdlib/raya/time.raya"),
        );
        modules.insert(
            "path",
            include_str!("../../../../raya-stdlib/raya/path.raya"),
        );
        modules.insert(
            "stream",
            include_str!("../../../../raya-stdlib/raya/stream.raya"),
        );
        modules.insert(
            "compress",
            include_str!("../../../../raya-stdlib/raya/compress.raya"),
        );
        modules.insert(
            "url",
            include_str!("../../../../raya-stdlib/raya/url.raya"),
        );
        modules.insert(
            "args",
            include_str!("../../../../raya-stdlib/raya/args.raya"),
        );
        modules.insert(
            "encoding",
            include_str!("../../../../raya-stdlib/raya/encoding.raya"),
        );
        modules.insert(
            "template",
            include_str!("../../../../raya-stdlib/raya/template.raya"),
        );
        modules.insert(
            "semver",
            include_str!("../../../../raya-stdlib/raya/semver.raya"),
        );
        modules.insert(
            "test",
            include_str!("../../../../raya-stdlib/raya/test.raya"),
        );

        // POSIX stdlib modules (raya-stdlib-posix)
        modules.insert(
            "fs",
            include_str!("../../../../raya-stdlib-posix/raya/fs.raya"),
        );
        modules.insert(
            "net",
            include_str!("../../../../raya-stdlib-posix/raya/net.raya"),
        );
        modules.insert(
            "http",
            include_str!("../../../../raya-stdlib-posix/raya/http.raya"),
        );
        modules.insert(
            "http2",
            include_str!("../../../../raya-stdlib-posix/raya/http2.raya"),
        );
        modules.insert(
            "fetch",
            include_str!("../../../../raya-stdlib-posix/raya/fetch.raya"),
        );
        modules.insert(
            "env",
            include_str!("../../../../raya-stdlib-posix/raya/env.raya"),
        );
        modules.insert(
            "process",
            include_str!("../../../../raya-stdlib-posix/raya/process.raya"),
        );
        modules.insert(
            "os",
            include_str!("../../../../raya-stdlib-posix/raya/os.raya"),
        );
        modules.insert(
            "io",
            include_str!("../../../../raya-stdlib-posix/raya/io.raya"),
        );
        modules.insert(
            "dns",
            include_str!("../../../../raya-stdlib-posix/raya/dns.raya"),
        );
        modules.insert(
            "terminal",
            include_str!("../../../../raya-stdlib-posix/raya/terminal.raya"),
        );
        modules.insert(
            "ws",
            include_str!("../../../../raya-stdlib-posix/raya/ws.raya"),
        );
        modules.insert(
            "readline",
            include_str!("../../../../raya-stdlib-posix/raya/readline.raya"),
        );
        modules.insert(
            "glob",
            include_str!("../../../../raya-stdlib-posix/raya/glob.raya"),
        );
        modules.insert(
            "archive",
            include_str!("../../../../raya-stdlib-posix/raya/archive.raya"),
        );
        modules.insert(
            "watch",
            include_str!("../../../../raya-stdlib-posix/raya/watch.raya"),
        );
        modules.insert(
            "pm",
            include_str!("../../../../raya-stdlib-posix/raya/pm.raya"),
        );
        modules.insert(
            "sqlite",
            include_str!("../../../../raya-stdlib-posix/raya/sqlite.raya"),
        );
        modules.insert(
            "__node_events",
            include_str!("../../../../raya-stdlib-node/raya/events.raya"),
        );
        modules.insert(
            "__node_assert",
            include_str!("../../../../raya-stdlib-node/raya/assert.raya"),
        );
        modules.insert(
            "__node_util",
            include_str!("../../../../raya-stdlib-node/raya/util.raya"),
        );
        modules.insert(
            "__node_module",
            include_str!("../../../../raya-stdlib-node/raya/module.raya"),
        );
        modules.insert(
            "__node_child_process",
            include_str!("../../../../raya-stdlib-node/raya/child_process.raya"),
        );
        modules.insert(
            "__node_https",
            include_str!("../../../../raya-stdlib-node/raya/https.raya"),
        );
        modules.insert(
            "__node_fs_promises",
            include_str!("../../../../raya-stdlib-node/raya/fs_promises.raya"),
        );
        modules.insert(
            "__node_test",
            include_str!("../../../../raya-stdlib-node/raya/test.raya"),
        );
        modules.insert(
            "__node_test_reporters",
            include_str!("../../../../raya-stdlib-node/raya/test_reporters.raya"),
        );
        modules.insert(
            "__node_timers",
            include_str!("../../../../raya-stdlib-node/raya/timers.raya"),
        );
        modules.insert(
            "__node_timers_promises",
            include_str!("../../../../raya-stdlib-node/raya/timers_promises.raya"),
        );
        modules.insert(
            "__node_buffer",
            include_str!("../../../../raya-stdlib-node/raya/buffer.raya"),
        );
        modules.insert(
            "__node_string_decoder",
            include_str!("../../../../raya-stdlib-node/raya/string_decoder.raya"),
        );
        modules.insert(
            "__node_stream_promises",
            include_str!("../../../../raya-stdlib-node/raya/stream_promises.raya"),
        );
        modules.insert(
            "__node_stream_web",
            include_str!("../../../../raya-stdlib-node/raya/stream_web.raya"),
        );
        modules.insert(
            "__node_inspector",
            include_str!("../../../../raya-stdlib-node/raya/inspector.raya"),
        );
        modules.insert(
            "__node_inspector_promises",
            include_str!("../../../../raya-stdlib-node/raya/inspector_promises.raya"),
        );
        modules.insert(
            "__node_async_hooks",
            include_str!("../../../../raya-stdlib-node/raya/async_hooks.raya"),
        );
        modules.insert(
            "__node_diagnostics_channel",
            include_str!("../../../../raya-stdlib-node/raya/diagnostics_channel.raya"),
        );
        modules.insert(
            "__node_v8",
            include_str!("../../../../raya-stdlib-node/raya/v8.raya"),
        );
        modules.insert(
            "__node_dgram",
            include_str!("../../../../raya-stdlib-node/raya/dgram.raya"),
        );
        modules.insert(
            "__node_repl",
            include_str!("../../../../raya-stdlib-node/raya/repl.raya"),
        );
        modules.insert(
            "__node_perf_hooks",
            include_str!("../../../../raya-stdlib-node/raya/perf_hooks.raya"),
        );

        // Node-compat wrappers (previously direct-mapped to std:*)
        modules.insert(
            "__node_fs",
            include_str!("../../../../raya-stdlib-node/raya/fs.raya"),
        );
        modules.insert(
            "__node_path",
            include_str!("../../../../raya-stdlib-node/raya/path.raya"),
        );
        modules.insert(
            "__node_crypto",
            include_str!("../../../../raya-stdlib-node/raya/crypto.raya"),
        );
        modules.insert(
            "__node_os",
            include_str!("../../../../raya-stdlib-node/raya/os.raya"),
        );
        modules.insert(
            "__node_process",
            include_str!("../../../../raya-stdlib-node/raya/process.raya"),
        );
        modules.insert(
            "__node_dns",
            include_str!("../../../../raya-stdlib-node/raya/dns.raya"),
        );
        modules.insert(
            "__node_net",
            include_str!("../../../../raya-stdlib-node/raya/net.raya"),
        );
        modules.insert(
            "__node_http",
            include_str!("../../../../raya-stdlib-node/raya/http.raya"),
        );
        modules.insert(
            "__node_url",
            include_str!("../../../../raya-stdlib-node/raya/url.raya"),
        );
        modules.insert(
            "__node_stream",
            include_str!("../../../../raya-stdlib-node/raya/stream.raya"),
        );
        modules.insert(
            "__node_worker_threads",
            include_str!("../../../../raya-stdlib-node/raya/worker_threads.raya"),
        );
        modules.insert(
            "__node_vm",
            include_str!("../../../../raya-stdlib-node/raya/vm.raya"),
        );
        modules.insert(
            "__node_cluster",
            include_str!("../../../../raya-stdlib-node/raya/cluster.raya"),
        );
        modules.insert(
            "__node_http2",
            include_str!("../../../../raya-stdlib-node/raya/http2.raya"),
        );
        modules.insert(
            "__node_sqlite",
            include_str!("../../../../raya-stdlib-node/raya/sqlite.raya"),
        );

        let node_aliases = NODE_STD_ALIASES.into_iter().collect();

        Self {
            modules,
            node_aliases,
        }
    }

    /// Get the source code for a standard library module
    pub fn get(&self, name: &str) -> Option<&'static str> {
        self.modules.get(name).copied()
    }

    /// Resolve an import specifier to canonical module name + source.
    pub fn resolve_specifier(&self, specifier: &str) -> Option<(String, &'static str)> {
        let canonical_name = if let Some(name) = specifier.strip_prefix(super::STD_MODULE_PREFIX) {
            name.to_string()
        } else if let Some(node_name) = specifier.strip_prefix(super::NODE_MODULE_PREFIX) {
            self.node_aliases.get(node_name)?.to_string()
        } else {
            return None;
        };

        self.get(&canonical_name)
            .map(|source| (canonical_name, source))
    }

    /// Check if an import specifier is a standard-library namespace import
    /// (`std:` or `node:`).
    pub fn is_std_import(specifier: &str) -> bool {
        specifier.starts_with(super::STD_MODULE_PREFIX)
            || specifier.starts_with(super::NODE_MODULE_PREFIX)
    }

    /// Check if an import specifier is a Node-core `node:` namespace import.
    pub fn is_node_import(specifier: &str) -> bool {
        specifier.starts_with(super::NODE_MODULE_PREFIX)
    }

    /// Extract the canonical module name from a `std:xxx` or supported
    /// `node:xxx` specifier.
    pub fn module_name(specifier: &str) -> Option<&str> {
        if let Some(name) = specifier.strip_prefix(super::STD_MODULE_PREFIX) {
            return Some(name);
        }
        if let Some(node_name) = specifier.strip_prefix(super::NODE_MODULE_PREFIX) {
            return NODE_STD_ALIASES
                .iter()
                .find_map(|(alias, target)| (*alias == node_name).then_some(*target));
        }
        None
    }

    /// Check if a `node:` module specifier maps to a supported module.
    pub fn is_supported_node_import(specifier: &str) -> bool {
        specifier
            .strip_prefix(super::NODE_MODULE_PREFIX)
            .and_then(|node_name| {
                NODE_STD_ALIASES
                    .iter()
                    .find_map(|(alias, _)| (*alias == node_name).then_some(()))
            })
            .is_some()
    }

    /// Supported v1 node core module names (without `node:` prefix).
    pub fn supported_node_module_names() -> impl Iterator<Item = &'static str> {
        NODE_STD_ALIASES.iter().map(|(name, _)| *name)
    }

    /// Get all registered module names
    pub fn module_names(&self) -> impl Iterator<Item = &&'static str> {
        self.modules.keys()
    }
}

impl Default for StdModuleRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_std_import() {
        assert!(StdModuleRegistry::is_std_import("std:logger"));
        assert!(StdModuleRegistry::is_std_import("std:math"));
        assert!(StdModuleRegistry::is_std_import("node:fs"));
        assert!(!StdModuleRegistry::is_std_import("./local"));
        assert!(!StdModuleRegistry::is_std_import("https://example.com"));
        assert!(!StdModuleRegistry::is_std_import("package-name"));
    }

    #[test]
    fn test_module_name() {
        assert_eq!(StdModuleRegistry::module_name("std:logger"), Some("logger"));
        assert_eq!(StdModuleRegistry::module_name("std:math"), Some("math"));
        assert_eq!(StdModuleRegistry::module_name("node:fs"), Some("__node_fs"));
        assert_eq!(
            StdModuleRegistry::module_name("node:fs/promises"),
            Some("__node_fs_promises")
        );
        assert_eq!(
            StdModuleRegistry::module_name("node:events"),
            Some("__node_events")
        );
        assert_eq!(
            StdModuleRegistry::module_name("node:assert"),
            Some("__node_assert")
        );
        assert_eq!(
            StdModuleRegistry::module_name("node:test/reporters"),
            Some("__node_test_reporters")
        );
        assert_eq!(
            StdModuleRegistry::module_name("node:worker_threads"),
            Some("__node_worker_threads")
        );
        assert_eq!(StdModuleRegistry::module_name("./local"), None);
    }

    #[test]
    fn test_supported_node_imports() {
        assert!(StdModuleRegistry::is_supported_node_import("node:fs"));
        assert!(StdModuleRegistry::is_supported_node_import("node:events"));
        assert!(StdModuleRegistry::is_supported_node_import("node:assert"));
        assert!(StdModuleRegistry::is_supported_node_import(
            "node:timers/promises"
        ));
        assert!(StdModuleRegistry::is_supported_node_import("node:sqlite"));
        assert!(!StdModuleRegistry::is_supported_node_import("std:fs"));
    }

    #[test]
    fn test_get_logger() {
        let registry = StdModuleRegistry::new();
        let source = registry.get("logger");
        assert!(source.is_some(), "std:logger module should be registered");
        assert!(
            source.unwrap().contains("Logger"),
            "Logger source should contain 'Logger'"
        );
    }
}
