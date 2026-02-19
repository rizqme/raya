//! `raya add` — Add a dependency.

pub fn execute(
    package: String,
    dev: bool,
    exact: bool,
    no_install: bool,
) -> anyhow::Result<()> {
    let options = raya_pm::commands::add::AddOptions {
        dev,
        exact,
        no_install,
    };
    match raya_pm::commands::add::add_package(&package, None, options) {
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
