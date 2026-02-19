//! `raya add` — Add a dependency.

pub fn execute(
    package: String,
    dev: bool,
    exact: bool,
    no_install: bool,
) -> anyhow::Result<()> {
    let options = rpkg::commands::add::AddOptions {
        dev,
        exact,
        no_install,
    };
    match rpkg::commands::add::add_package(&package, None, options) {
        Ok(()) => {
            println!("\nPackage added successfully.");
            Ok(())
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}
