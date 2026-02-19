//! `raya run` — dual-mode: run scripts from raya.toml or execute files directly.

use anyhow::{anyhow, Context};
use rpkg::PackageManifest;
use std::path::Path;
use std::process::Command;

#[allow(dead_code)]
pub struct RunArgs {
    pub target: Option<String>,
    pub args: Vec<String>,
    pub watch: bool,
    pub inspect: bool,
    pub inspect_brk: bool,
    pub no_cache: bool,
    pub no_jit: bool,
    pub jit_threshold: u32,
    pub threads: usize,
    pub heap_limit: usize,
    pub timeout: u64,
    pub list: bool,
}

pub fn execute(args: RunArgs) -> anyhow::Result<()> {
    if args.list {
        return list_scripts();
    }

    match &args.target {
        None => run_default(&args),
        Some(target) if looks_like_file(target) => execute_file(target, &args.args),
        Some(script_name) => run_script(script_name, &args),
    }
}

/// Called from main.rs for implicit run: `raya ./file.raya`
pub fn execute_file(path: &str, extra_args: &[String]) -> anyhow::Result<()> {
    if !Path::new(path).exists() {
        anyhow::bail!("File not found: {}", path);
    }

    println!("Running: {}", path);
    if !extra_args.is_empty() {
        println!("Arguments: {:?}", extra_args);
    }

    // TODO: Wire up actual compilation + execution pipeline:
    // 1. Parse source
    // 2. Type-check
    // 3. Compile to bytecode
    // 4. Execute via Vm with StdNativeHandler
    // JIT is enabled by default.
    eprintln!("(Execution pipeline not yet wired — coming in Phase 1)");

    Ok(())
}

fn run_default(args: &RunArgs) -> anyhow::Result<()> {
    let manifest = load_manifest_optional();

    // Try [scripts].start first
    if let Some(ref manifest) = manifest {
        if let Some(cmd) = manifest.scripts.get("start") {
            println!("Running script: start → {}", cmd);
            return run_script_cmd(cmd);
        }

        // Fall back to [package].main
        if let Some(ref main_file) = manifest.package.main {
            return execute_file(main_file, &args.args);
        }
    }

    // No manifest at all — try src/main.raya
    if Path::new("src/main.raya").exists() {
        return execute_file("src/main.raya", &args.args);
    }

    Err(anyhow!(
        "No start script or main entry point defined.\n\
         Add [scripts].start to raya.toml, set [package].main, or create src/main.raya."
    ))
}

fn run_script(name: &str, _args: &RunArgs) -> anyhow::Result<()> {
    let manifest = load_manifest()
        .context("Cannot run scripts without a raya.toml in the project")?;

    let cmd = manifest
        .scripts
        .get(name)
        .ok_or_else(|| {
            let available: Vec<&String> = manifest.scripts.keys().collect();
            if available.is_empty() {
                anyhow!("Unknown script '{}'. No scripts defined in raya.toml.", name)
            } else {
                anyhow!(
                    "Unknown script '{}'. Available scripts: {}",
                    name,
                    available.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                )
            }
        })?;

    println!("Running script: {} → {}", name, cmd);
    run_script_cmd(cmd)
}

fn run_script_cmd(cmd: &str) -> anyhow::Result<()> {
    let first_word = cmd.split_whitespace().next().unwrap_or("");

    // If the command points to a .raya/.ryb file, run it directly
    if looks_like_file(first_word) {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let file = parts[0];
        let extra: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
        return execute_file(file, &extra);
    }

    // Otherwise, run as shell command
    let status = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .status()
        .context("Failed to execute shell command")?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(127));
    }

    Ok(())
}

fn list_scripts() -> anyhow::Result<()> {
    let manifest = load_manifest()
        .context("No raya.toml found in this directory or any parent")?;

    if manifest.scripts.is_empty() {
        println!("No scripts defined in raya.toml.");
        println!();
        println!("Add scripts to your raya.toml:");
        println!();
        println!("  [scripts]");
        println!("  dev = \"src/main.raya --watch\"");
        println!("  start = \"dist/main.ryb\"");
        return Ok(());
    }

    println!("Available scripts (from raya.toml):");
    println!();

    // Sort for consistent output
    let mut scripts: Vec<(&String, &String)> = manifest.scripts.iter().collect();
    scripts.sort_by(|(a, _), (b, _)| a.cmp(b));

    let max_name_len = scripts.iter().map(|(k, _)| k.len()).max().unwrap_or(0);

    for (name, cmd) in &scripts {
        println!("  {:<width$}  {}", name, cmd, width = max_name_len);
    }

    Ok(())
}

fn looks_like_file(s: &str) -> bool {
    s.ends_with(".raya")
        || s.ends_with(".ryb")
        || s.contains('/')
        || s.contains('\\')
        || s.starts_with('.')
}

fn load_manifest() -> anyhow::Result<PackageManifest> {
    let path = find_manifest()?;
    PackageManifest::from_file(&path).map_err(|e| anyhow!("{}", e))
}

fn load_manifest_optional() -> Option<PackageManifest> {
    load_manifest().ok()
}

fn find_manifest() -> anyhow::Result<std::path::PathBuf> {
    let mut dir = std::env::current_dir()?;
    loop {
        let candidate = dir.join("raya.toml");
        if candidate.exists() {
            return Ok(candidate);
        }
        if !dir.pop() {
            return Err(anyhow!("No raya.toml found"));
        }
    }
}
