//! `raya init` — Initialize a new Raya project.

use std::path::PathBuf;

pub fn execute(
    path: PathBuf,
    name: Option<String>,
    _template: String,
    _yes: bool,
) -> anyhow::Result<()> {
    match raya_pm::commands::init::init_project(&path, name.as_deref()) {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}
