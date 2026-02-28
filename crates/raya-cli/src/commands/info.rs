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
    let package_json_path = Path::new("package.json");
    let manifest_path = Path::new("raya.toml");
    if package_json_path.exists() {
        if let Ok(content) = std::fs::read_to_string(package_json_path) {
            if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
                let name = pkg.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
                let version = pkg.get("version").and_then(|v| v.as_str()).unwrap_or("0.0.0");
                println!();
                println!("Project:      {} v{}", name, version);
                if let Some(entry) = pkg
                    .get("raya")
                    .and_then(|v| v.get("entry"))
                    .and_then(|v| v.as_str())
                {
                    println!("Entry:        {}", entry);
                }
                let dep_count = pkg
                    .get("dependencies")
                    .and_then(|v| v.as_object())
                    .map(|m| m.len())
                    .unwrap_or(0);
                let dev_dep_count = pkg
                    .get("devDependencies")
                    .and_then(|v| v.as_object())
                    .map(|m| m.len())
                    .unwrap_or(0);
                if dep_count > 0 || dev_dep_count > 0 {
                    println!("Dependencies: {} ({} dev)", dep_count, dev_dep_count);
                }
                let scripts_count = pkg
                    .get("scripts")
                    .and_then(|v| v.as_object())
                    .map(|m| m.len())
                    .unwrap_or(0);
                if scripts_count > 0 {
                    println!("Scripts:      {}", scripts_count);
                }
                if let Some(registry) = pkg
                    .get("raya")
                    .and_then(|v| v.get("registry"))
                    .and_then(|v| v.get("url"))
                    .and_then(|v| v.as_str())
                {
                    println!("Registry:     {}", registry);
                }
            }
        }
    } else if manifest_path.exists() {
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
    let build_dir = Path::new(".raya/build");
    if build_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(build_dir) {
            let count = entries.count();
            println!("Artifacts:    .raya/build/ ({} entries)", count);
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
