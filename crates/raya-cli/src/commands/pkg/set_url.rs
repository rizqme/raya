//! `raya pkg set-url` — Set the default package registry URL.

use anyhow::anyhow;
use rpkg::{PackageManifest, RegistryConfig};
use std::path::{Path, PathBuf};

pub fn execute(url: Option<String>, global: bool, show: bool) -> anyhow::Result<()> {
    if show || url.is_none() {
        return show_current();
    }

    let url = url.unwrap();

    if global {
        set_global(&url)
    } else {
        set_project(&url)
    }
}

fn show_current() -> anyhow::Result<()> {
    // Resolution order: RAYA_REGISTRY env > project raya.toml > global config > default
    if let Ok(url) = std::env::var("RAYA_REGISTRY") {
        println!("Registry: {} (from RAYA_REGISTRY)", url);
        return Ok(());
    }

    if let Ok(manifest) = PackageManifest::from_file(Path::new("raya.toml")) {
        if let Some(reg) = manifest.registry {
            println!("Registry: {} (from raya.toml)", reg.url);
            return Ok(());
        }
    }

    let global_config = global_config_path();
    if global_config.exists() {
        let content = std::fs::read_to_string(&global_config)?;
        if let Ok(parsed) = content.parse::<toml::Value>() {
            if let Some(url) = parsed.get("registry").and_then(|r| r.get("url")).and_then(|u| u.as_str()) {
                println!("Registry: {} (from ~/.raya/config.toml)", url);
                return Ok(());
            }
        }
    }

    println!("Registry: https://registry.raya.dev (default)");
    Ok(())
}

fn set_project(url: &str) -> anyhow::Result<()> {
    let manifest_path = Path::new("raya.toml");
    if !manifest_path.exists() {
        return Err(anyhow!(
            "No raya.toml found. Use --global to set the registry globally, \
             or run `raya init` first."
        ));
    }

    let mut manifest = PackageManifest::from_file(manifest_path)
        .map_err(|e| anyhow!("{}", e))?;

    manifest.registry = Some(RegistryConfig {
        url: url.to_string(),
    });

    manifest.to_file(manifest_path).map_err(|e| anyhow!("{}", e))?;
    println!("Registry set to {} (in raya.toml)", url);
    Ok(())
}

fn set_global(url: &str) -> anyhow::Result<()> {
    let config_path = global_config_path();

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut config: toml::Value = if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)?;
        toml::from_str(&content).unwrap_or(toml::Value::Table(toml::map::Map::new()))
    } else {
        toml::Value::Table(toml::map::Map::new())
    };

    let mut reg = toml::map::Map::new();
    reg.insert("url".to_string(), toml::Value::String(url.to_string()));
    config
        .as_table_mut()
        .unwrap()
        .insert("registry".to_string(), toml::Value::Table(reg));

    let content = toml::to_string_pretty(&config)?;
    std::fs::write(&config_path, content)?;
    println!("Registry set to {} (in ~/.raya/config.toml)", url);
    Ok(())
}

fn global_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".raya")
        .join("config.toml")
}
