//! `raya install` — Install all dependencies.

pub fn execute(production: bool, _frozen: bool, force: bool) -> anyhow::Result<()> {
    let options = rpkg::commands::install::InstallOptions {
        production,
        force,
        update: false,
    };
    match rpkg::commands::install::install_dependencies(None, options) {
        Ok(result) => {
            println!(
                "\nDone! {} installed, {} from cache, {} updated.",
                result.installed, result.cached, result.updated
            );
            Ok(())
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}
