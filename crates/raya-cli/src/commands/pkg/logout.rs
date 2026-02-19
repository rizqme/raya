//! `raya pkg logout` — Remove stored credentials.

use std::path::PathBuf;

pub fn execute(registry: Option<String>, _scope: Option<String>) -> anyhow::Result<()> {
    let registry_url = registry.unwrap_or_else(|| "https://registry.raya.dev".to_string());
    let creds_path = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".raya")
        .join("credentials.toml");

    if !creds_path.exists() {
        println!("No credentials stored.");
        return Ok(());
    }

    let content = std::fs::read_to_string(&creds_path)?;
    let mut creds: toml::Value = toml::from_str(&content)?;

    if let Some(registries) = creds.as_table_mut().and_then(|t| t.get_mut("registries")) {
        if let Some(table) = registries.as_table_mut() {
            if table.remove(&registry_url).is_some() {
                let content = toml::to_string_pretty(&creds)?;
                std::fs::write(&creds_path, content)?;
                println!("Logged out from {}", registry_url);
                return Ok(());
            }
        }
    }

    println!("No credentials found for {}", registry_url);
    Ok(())
}
