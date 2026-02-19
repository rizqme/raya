//! `raya remove` — Remove a dependency.

pub fn execute(package: String) -> anyhow::Result<()> {
    match raya_pm::commands::add::remove_package(&package, None) {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}
