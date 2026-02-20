//! Standard library module registry
//!
//! Maps `std:xxx` import specifiers to embedded module source code.
//! Standard library modules are built into the compiler and don't require
//! external files or package resolution.

use std::collections::HashMap;

/// Registry of standard library modules
///
/// Standard library modules use the `std:` namespace prefix and are
/// resolved by looking up their embedded source code rather than
/// going through the file system or package resolution.
pub struct StdModuleRegistry {
    /// Map from module name (without `std:` prefix) to source code
    modules: HashMap<&'static str, &'static str>,
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
        Self { modules }
    }

    /// Get the source code for a standard library module
    pub fn get(&self, name: &str) -> Option<&'static str> {
        self.modules.get(name).copied()
    }

    /// Check if an import specifier is a standard library import
    pub fn is_std_import(specifier: &str) -> bool {
        specifier.starts_with("std:")
    }

    /// Extract the module name from a `std:xxx` specifier
    pub fn module_name(specifier: &str) -> Option<&str> {
        specifier.strip_prefix("std:")
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
        assert!(!StdModuleRegistry::is_std_import("./local"));
        assert!(!StdModuleRegistry::is_std_import("https://example.com"));
        assert!(!StdModuleRegistry::is_std_import("package-name"));
    }

    #[test]
    fn test_module_name() {
        assert_eq!(StdModuleRegistry::module_name("std:logger"), Some("logger"));
        assert_eq!(StdModuleRegistry::module_name("std:math"), Some("math"));
        assert_eq!(StdModuleRegistry::module_name("./local"), None);
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
