//! `raya pkg whoami` — Show the currently authenticated user.

use std::path::PathBuf;

pub fn execute() -> anyhow::Result<()> {
    let creds_path = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".raya")
        .join("credentials.toml");

    if !creds_path.exists() {
        println!("Not logged in. Run `raya pkg login` to authenticate.");
        return Ok(());
    }

    let content = std::fs::read_to_string(&creds_path)?;
    let creds: toml::Value = toml::from_str(&content)?;

    let registries = match creds.get("registries").and_then(|r| r.as_table()) {
        Some(r) => r,
        None => {
            println!("Not logged in. Run `raya pkg login` to authenticate.");
            return Ok(());
        }
    };

    if registries.is_empty() {
        println!("Not logged in. Run `raya pkg login` to authenticate.");
        return Ok(());
    }

    for (registry, entry) in registries {
        let user = entry.get("user").and_then(|v| v.as_str()).unwrap_or("(unknown user)");
        let email = entry.get("email").and_then(|v| v.as_str());

        if let Some(email) = email {
            println!("  {} ({}) on {}", user, email, registry);
        } else {
            println!("  {} on {}", user, registry);
        }
    }

    Ok(())
}
