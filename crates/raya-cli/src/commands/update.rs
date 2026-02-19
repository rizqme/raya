//! `raya update` — Update dependencies.

pub fn execute(package: Option<String>) -> anyhow::Result<()> {
    if let Some(pkg) = package {
        println!("Updating package: {}", pkg);
        eprintln!("(Single package update not yet implemented)");
        return Ok(());
    }

    let options = rpkg::commands::install::InstallOptions {
        production: false,
        force: false,
        update: true,
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
