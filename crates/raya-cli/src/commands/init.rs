//! `raya init` — Initialize a new Raya project.

use anyhow::Context;
use raya_runtime::Runtime;
use std::io::{self, Write};
use std::path::PathBuf;

pub fn execute(
    path: PathBuf,
    name: Option<String>,
    template: String,
    yes: bool,
    interactive: bool,
    node: bool,
    npm: bool,
) -> anyhow::Result<()> {
    let default_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("raya-app")
        .to_string();

    let use_interactive = interactive || (!yes && name.is_none() && template == "basic");
    let (resolved_name, resolved_template) = if use_interactive {
        prompt_init_config(&default_name, &template)?
    } else {
        let n = name.unwrap_or(default_name);
        let t = normalize_template(&template);
        (n, t)
    };

    let rt = Runtime::new();
    let dir_str = path
        .display()
        .to_string()
        .replace('\\', "/")
        .replace('"', "\\\"");
    let name_arg = format!("\"{}\"", resolved_name.replace('"', "\\\""));
    let script = format!(
        r#"import pm from "std:pm";
pm.init("{}", {}, {}, {})"#,
        dir_str, name_arg, node, npm
    );
    match rt
        .eval(&script)
        .with_context(|| "failed to initialize project via pm.init")
    {
        Ok(_) => {
            apply_template(&path, &resolved_template, node)?;
            println!(
                "Initialized Raya project '{}' at {} (template: {})",
                resolved_name,
                path.display(),
                resolved_template
            );
            Ok(())
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

fn normalize_template(t: &str) -> String {
    match t.trim().to_lowercase().as_str() {
        "lib" | "library" => "lib".to_string(),
        _ => "basic".to_string(),
    }
}

fn prompt_line(prompt: &str, default: &str) -> anyhow::Result<String> {
    print!("{prompt} [{default}]: ");
    io::stdout().flush().context("flush stdout")?;
    let mut input = String::new();
    io::stdin().read_line(&mut input).context("read stdin")?;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

fn prompt_init_config(
    default_name: &str,
    current_template: &str,
) -> anyhow::Result<(String, String)> {
    println!("This utility will walk you through creating a Raya project.");
    let name = prompt_line("package name", default_name)?;
    let default_template = normalize_template(current_template);
    let template_raw = prompt_line("template (basic/lib)", &default_template)?;
    Ok((name, normalize_template(&template_raw)))
}

fn apply_template(path: &PathBuf, template: &str, node: bool) -> anyhow::Result<()> {
    if template != "lib" {
        return Ok(());
    }

    let src_dir = path.join("src");
    std::fs::create_dir_all(&src_dir).with_context(|| "create src directory")?;
    let lib_path = src_dir.join("lib.raya");
    if !lib_path.exists() {
        std::fs::write(
            &lib_path,
            "export function hello(name: string): string {\n    return \"Hello, \" + name + \"!\";\n}\n",
        )
        .with_context(|| "write src/lib.raya")?;
    }

    if node {
        let package_json_path = path.join("package.json");
        if package_json_path.exists() {
            let content =
                std::fs::read_to_string(&package_json_path).with_context(|| "read package.json")?;
            let mut json: serde_json::Value =
                serde_json::from_str(&content).with_context(|| "parse package.json")?;
            if !json.get("raya").map(|v| v.is_object()).unwrap_or(false) {
                json["raya"] = serde_json::json!({});
            }
            json["raya"]["entry"] = serde_json::json!("src/lib.raya");
            let updated =
                serde_json::to_string_pretty(&json).with_context(|| "serialize package.json")?;
            std::fs::write(&package_json_path, updated).with_context(|| "update package.json")?;
        }
    } else {
        let manifest_path = path.join("raya.toml");
        if manifest_path.exists() {
            let manifest =
                std::fs::read_to_string(&manifest_path).with_context(|| "read raya.toml")?;
            let updated = manifest.replace("main = \"src/main.raya\"", "main = \"src/lib.raya\"");
            std::fs::write(&manifest_path, updated).with_context(|| "update raya.toml")?;
        }
    }

    Ok(())
}
