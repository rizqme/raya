//! `raya remove` — Remove a dependency.

use raya_runtime::Runtime;

pub fn execute(package: String) -> anyhow::Result<()> {
    let rt = Runtime::new();
    let cwd = std::env::current_dir()?
        .display()
        .to_string()
        .replace('\\', "/")
        .replace('"', "\\\"");
    let pkg = package.replace('"', "\\\"");
    let script = format!(
        r#"pm.remove("{}", "{}")"#,
        cwd, pkg
    );
    match rt.eval(&script) {
        Ok(_) => Ok(()),
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}
