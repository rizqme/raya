//! `raya pkg login` — Authenticate with a package registry.

use anyhow::anyhow;
use std::io::{self, Write};
use std::path::PathBuf;

const DEFAULT_REGISTRY: &str = "https://registry.raya.dev";

pub fn execute(
    registry: Option<String>,
    token: Option<String>,
    _scope: Option<String>,
) -> anyhow::Result<()> {
    let registry_url = registry.unwrap_or_else(|| resolve_registry());

    if let Some(token) = token {
        // Non-interactive: token provided directly
        save_credentials(&registry_url, &token, None, None)?;
        println!("Credentials saved for {}", registry_url);
        return Ok(());
    }

    // Interactive flow
    println!("  Registry: {}", registry_url);
    println!();
    print!("  Enter token: ");
    io::stdout().flush()?;

    let mut token = String::new();
    io::stdin().read_line(&mut token)?;
    let token = token.trim();

    if token.is_empty() {
        return Err(anyhow!("Token cannot be empty"));
    }

    save_credentials(&registry_url, token, None, None)?;
    println!();
    println!("  Credentials saved to {}", credentials_path().display());

    Ok(())
}

fn resolve_registry() -> String {
    // Check RAYA_REGISTRY env var
    if let Ok(url) = std::env::var("RAYA_REGISTRY") {
        return url;
    }

    // Check project raya.toml
    if let Ok(manifest) = rpkg::PackageManifest::from_file(&std::path::Path::new("raya.toml")) {
        if let Some(reg) = manifest.registry {
            return reg.url;
        }
    }

    // Check global config
    if let Some(url) = read_global_registry() {
        return url;
    }

    DEFAULT_REGISTRY.to_string()
}

fn read_global_registry() -> Option<String> {
    let config_path = dirs_path().join("config.toml");
    let content = std::fs::read_to_string(&config_path).ok()?;
    // Simple TOML parsing for [registry].url
    let parsed: toml::Value = toml::from_str(&content).ok()?;
    parsed.get("registry")?.get("url")?.as_str().map(|s| s.to_string())
}

fn credentials_path() -> PathBuf {
    dirs_path().join("credentials.toml")
}

fn dirs_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".raya")
}

fn save_credentials(
    registry_url: &str,
    token: &str,
    user: Option<&str>,
    email: Option<&str>,
) -> anyhow::Result<()> {
    let creds_path = credentials_path();

    // Ensure directory exists
    if let Some(parent) = creds_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Read existing or create new
    let mut creds: toml::Value = if creds_path.exists() {
        let content = std::fs::read_to_string(&creds_path)?;
        toml::from_str(&content).unwrap_or(toml::Value::Table(toml::map::Map::new()))
    } else {
        toml::Value::Table(toml::map::Map::new())
    };

    // Build registry entry
    let mut entry = toml::map::Map::new();
    entry.insert("token".to_string(), toml::Value::String(token.to_string()));
    if let Some(u) = user {
        entry.insert("user".to_string(), toml::Value::String(u.to_string()));
    }
    if let Some(e) = email {
        entry.insert("email".to_string(), toml::Value::String(e.to_string()));
    }

    // Set under [registries."<url>"]
    let registries = creds
        .as_table_mut()
        .unwrap()
        .entry("registries")
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));

    registries
        .as_table_mut()
        .unwrap()
        .insert(registry_url.to_string(), toml::Value::Table(entry));

    let content = toml::to_string_pretty(&creds)?;
    std::fs::write(&creds_path, content)?;

    Ok(())
}
