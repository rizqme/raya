//! `raya run` — dual-mode: run scripts from raya.toml or execute files directly.

use anyhow::{anyhow, Context};
use raya_runtime::{Runtime, RuntimeOptions};
use raya_pm::PackageManifest;
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
    pub cpu_prof: Option<std::path::PathBuf>,
    pub prof_interval: u64,
}

impl RunArgs {
    fn to_runtime_options(&self) -> RuntimeOptions {
        RuntimeOptions {
            threads: self.threads,
            heap_limit: self.heap_limit * 1024 * 1024, // MB → bytes
            timeout: self.timeout,
            no_jit: self.no_jit,
            jit_threshold: self.jit_threshold,
            cpu_prof: self.cpu_prof.clone(),
            prof_interval_us: self.prof_interval,
        }
    }
}

pub fn execute(args: RunArgs) -> anyhow::Result<()> {
    if args.list {
        return list_scripts();
    }

    let rt = Runtime::with_options(args.to_runtime_options());

    match &args.target {
        None => run_default(&rt, &args),
        Some(target) if looks_like_file(target) => run_file(&rt, target),
        Some(script_name) => run_script(script_name, &rt),
    }
}

/// Called from main.rs for implicit run: `raya ./file.raya`
pub fn execute_file(path: &str, _extra_args: &[String]) -> anyhow::Result<()> {
    let rt = Runtime::new();
    run_file(&rt, path)
}

/// Run a .raya or .ryb file through the runtime.
fn run_file(rt: &Runtime, path: &str) -> anyhow::Result<()> {
    if !Path::new(path).exists() {
        anyhow::bail!("File not found: {}", path);
    }

    let exit_code = rt
        .run_file(Path::new(path))
        .map_err(|e| anyhow!("{}", e))?;

    if exit_code != 0 {
        std::process::exit(exit_code);
    }

    Ok(())
}

fn run_default(rt: &Runtime, _args: &RunArgs) -> anyhow::Result<()> {
    let manifest = load_manifest_optional();

    // Try [scripts].start first
    if let Some(ref manifest) = manifest {
        if let Some(cmd) = manifest.scripts.get("start") {
            println!("Running script: start → {}", cmd);
            return run_script_cmd(cmd, rt);
        }

        // Fall back to [package].main
        if let Some(ref main_file) = manifest.package.main {
            return run_file(rt, main_file);
        }
    }

    // No manifest at all — try src/main.raya
    if Path::new("src/main.raya").exists() {
        return run_file(rt, "src/main.raya");
    }

    Err(anyhow!(
        "No start script or main entry point defined.\n\
         Add [scripts].start to raya.toml, set [package].main, or create src/main.raya."
    ))
}

fn run_script(name: &str, rt: &Runtime) -> anyhow::Result<()> {
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
    run_script_cmd(cmd, rt)
}

fn run_script_cmd(cmd: &str, rt: &Runtime) -> anyhow::Result<()> {
    let first_word = cmd.split_whitespace().next().unwrap_or("");

    // If the command points to a .raya/.ryb file, run it directly
    if looks_like_file(first_word) {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let file = parts[0];
        return run_file(rt, file);
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
