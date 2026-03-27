//! `raya run` — dual-mode: run scripts from raya.toml or execute files directly.

use anyhow::{anyhow, Context};
use raya_engine::semantics::{SemanticProfile, SourceKind};
use raya_pm::PackageManifest;
use raya_runtime::{BuiltinMode, Runtime, RuntimeOptions};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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
    pub node_compat: bool,
    pub semantic_profile: SemanticProfile,
}

impl RunArgs {
    fn to_runtime_options(&self) -> anyhow::Result<RuntimeOptions> {
        if matches!(
            self.semantic_profile.source_kind,
            SourceKind::Ts | SourceKind::Js
        ) && !self.node_compat
        {
            anyhow::bail!("--mode ts/js requires --node-compat");
        }
        Ok(RuntimeOptions {
            threads: self.threads,
            heap_limit: self.heap_limit * 1024 * 1024, // MB → bytes
            timeout: self.timeout,
            no_jit: self.no_jit,
            jit_threshold: self.jit_threshold,
            cpu_prof: self.cpu_prof.clone(),
            prof_interval_us: self.prof_interval,
            builtin_mode: if self.node_compat {
                BuiltinMode::NodeCompat
            } else {
                BuiltinMode::RayaStrict
            },
            semantic_profile: Some(self.semantic_profile),
            ts_options: None,
            ..Default::default()
        })
    }
}

pub fn execute(args: RunArgs) -> anyhow::Result<()> {
    if args.list {
        return list_scripts();
    }

    // Delegate to debug command when --inspect or --inspect-brk is used
    if args.inspect || args.inspect_brk {
        let target = args.target.clone().unwrap_or_default();
        if target.is_empty() {
            return Err(anyhow!("--inspect requires a target file"));
        }
        return super::debug::execute(target, args.inspect_brk, None, false);
    }

    let rt = Runtime::with_options(args.to_runtime_options()?);

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

    let exit_code = rt.run_file(Path::new(path)).map_err(|e| anyhow!("{}", e))?;

    if exit_code != 0 {
        std::process::exit(exit_code);
    }

    Ok(())
}

fn run_default(rt: &Runtime, _args: &RunArgs) -> anyhow::Result<()> {
    let manifest = load_project_manifest_optional();

    // Try [scripts].start first
    if let Some(ref manifest) = manifest {
        if let Some(cmd) = manifest.scripts().get("start") {
            println!("Running script: start → {}", cmd);
            return run_script_cmd(cmd, rt);
        }

        // Fall back to main entry
        if let Some(main_file) = manifest.main() {
            return run_file(rt, &main_file);
        }
    }

    // No manifest at all — try src/main.raya
    if Path::new("src/main.raya").exists() {
        return run_file(rt, "src/main.raya");
    }

    Err(anyhow!(
        "No start script or main entry point defined.\n\
         Add scripts.start and raya.entry (package.json) or [scripts]/[package].main (raya.toml), or create src/main.raya."
    ))
}

fn run_script(name: &str, rt: &Runtime) -> anyhow::Result<()> {
    let manifest = load_project_manifest()
        .context("Cannot run scripts without a package.json or raya.toml in the project")?;

    let scripts = manifest.scripts();
    let cmd = scripts.get(name).ok_or_else(|| {
        let available: Vec<&String> = scripts.keys().collect();
        if available.is_empty() {
            anyhow!(
                "Unknown script '{}'. No scripts defined in project manifest.",
                name
            )
        } else {
            anyhow!(
                "Unknown script '{}'. Available scripts: {}",
                name,
                available
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
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
    let manifest = load_project_manifest()
        .context("No package.json or raya.toml found in this directory or any parent")?;
    let scripts = manifest.scripts();

    if scripts.is_empty() {
        println!("No scripts defined in project manifest.");
        println!();
        println!("Add scripts to your manifest:");
        println!();
        if matches!(manifest, ProjectManifest::PackageJson(_)) {
            println!("  \"scripts\": {{");
            println!("    \"dev\": \"src/main.raya --watch\",");
            println!("    \"start\": \"dist/main.ryb\"");
            println!("  }}");
        } else {
            println!("  [scripts]");
            println!("  dev = \"src/main.raya --watch\"");
            println!("  start = \"dist/main.ryb\"");
        }
        return Ok(());
    }

    println!("Available scripts:");
    println!();

    // Sort for consistent output
    let mut script_pairs: Vec<(&String, &String)> = scripts.iter().collect();
    script_pairs.sort_by(|(a, _), (b, _)| a.cmp(b));

    let max_name_len = script_pairs.iter().map(|(k, _)| k.len()).max().unwrap_or(0);

    for (name, cmd) in &script_pairs {
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

#[derive(Debug, Clone)]
struct PackageJsonManifest {
    scripts: HashMap<String, String>,
    main: Option<String>,
}

#[derive(Debug, Clone)]
enum ProjectManifest {
    RayaToml(PackageManifest),
    PackageJson(PackageJsonManifest),
}

impl ProjectManifest {
    fn scripts(&self) -> &HashMap<String, String> {
        match self {
            ProjectManifest::RayaToml(m) => &m.scripts,
            ProjectManifest::PackageJson(m) => &m.scripts,
        }
    }

    fn main(&self) -> Option<String> {
        match self {
            ProjectManifest::RayaToml(m) => m.package.main.clone(),
            ProjectManifest::PackageJson(m) => m.main.clone(),
        }
    }
}

fn load_project_manifest() -> anyhow::Result<ProjectManifest> {
    let (path, is_package_json) = find_manifest()?;
    if is_package_json {
        let content = std::fs::read_to_string(&path)?;
        let value: serde_json::Value = serde_json::from_str(&content)?;
        let mut scripts = HashMap::new();
        if let Some(obj) = value.get("scripts").and_then(|v| v.as_object()) {
            for (k, v) in obj {
                if let Some(s) = v.as_str() {
                    scripts.insert(k.clone(), s.to_string());
                }
            }
        }
        let main = value
            .get("raya")
            .and_then(|v| v.get("entry"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                value
                    .get("main")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            });
        Ok(ProjectManifest::PackageJson(PackageJsonManifest {
            scripts,
            main,
        }))
    } else {
        PackageManifest::from_file(&path)
            .map(ProjectManifest::RayaToml)
            .map_err(|e| anyhow!("{}", e))
    }
}

fn load_project_manifest_optional() -> Option<ProjectManifest> {
    load_project_manifest().ok()
}

fn find_manifest() -> anyhow::Result<(PathBuf, bool)> {
    let mut dir = std::env::current_dir()?;
    loop {
        let package_candidate = dir.join("package.json");
        if package_candidate.exists() {
            return Ok((package_candidate, true));
        }
        let toml_candidate = dir.join("raya.toml");
        if toml_candidate.exists() {
            return Ok((toml_candidate, false));
        }
        if !dir.pop() {
            return Err(anyhow!("No package.json or raya.toml found"));
        }
    }
}
