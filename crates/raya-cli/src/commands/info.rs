//! `raya info` — Display environment and project info.

use std::path::Path;

pub fn execute() -> anyhow::Result<()> {
    // Version
    println!("Raya v{}", env!("CARGO_PKG_VERSION"));
    println!();

    // Platform
    println!("Platform:     {} ({})", std::env::consts::OS, std::env::consts::ARCH);

    // JIT availability
    println!("JIT:          available (Cranelift)");

    // Project info
    let manifest_path = Path::new("raya.toml");
    if manifest_path.exists() {
        if let Ok(manifest) = raya_pm::PackageManifest::from_file(manifest_path) {
            println!();
            println!("Project:      {} v{}", manifest.package.name, manifest.package.version);
            if let Some(ref main) = manifest.package.main {
                println!("Entry:        {}", main);
            }

            let dep_count = manifest.dependencies.len();
            let dev_dep_count = manifest.dev_dependencies.len();
            if dep_count > 0 || dev_dep_count > 0 {
                println!("Dependencies: {} ({} dev)", dep_count, dev_dep_count);
            }

            if !manifest.scripts.is_empty() {
                println!("Scripts:      {}", manifest.scripts.len());
            }

            if let Some(ref reg) = manifest.registry {
                println!("Registry:     {}", reg.url);
            }
        }
    }

    // Cache info
    let cache_dir = Path::new(".raya-cache");
    if cache_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(cache_dir) {
            let count = entries.count();
            println!("Cache:        .raya-cache/ ({} files)", count);
        }
    }

    // Environment
    println!();
    println!("Environment:");
    print_env("  RAYA_CACHE_DIR", "RAYA_CACHE_DIR");
    print_env("  RAYA_NUM_THREADS", "RAYA_NUM_THREADS");
    print_env("  RAYA_LOG", "RAYA_LOG");
    print_env("  RAYA_REGISTRY", "RAYA_REGISTRY");

    Ok(())
}

fn print_env(label: &str, var: &str) {
    match std::env::var(var) {
        Ok(val) => println!("{} = {}", label, val),
        Err(_) => println!("{} = (default)", label),
    }
}
