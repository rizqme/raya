//! `raya new` — Create a new project (alias for init with directory).

use std::path::PathBuf;

pub fn execute(name: String, _template: String) -> anyhow::Result<()> {
    let path = PathBuf::from(&name);
    match raya_pm::commands::init::init_project(&path, Some(&name)) {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}
