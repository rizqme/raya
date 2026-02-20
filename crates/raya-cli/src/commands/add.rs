//! `raya add` — Add a dependency.

use raya_runtime::Runtime;

pub fn execute(
    package: String,
    dev: bool,
    exact: bool,
    no_install: bool,
) -> anyhow::Result<()> {
    let rt = Runtime::new();
    let cwd = std::env::current_dir()?
        .display()
        .to_string()
        .replace('\\', "/")
        .replace('"', "\\\"");
    let pkg = package.replace('"', "\\\"");
    let script = format!(
        r#"pm.add("{}", "{}", {}, {}, {})"#,
        cwd, pkg, dev, exact, no_install
    );
    match rt.eval(&script) {
        Ok(_) => {
            println!("\nPackage added successfully.");
            Ok(())
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}
