//! `raya install` — Install all dependencies.

use raya_runtime::Runtime;

pub fn execute(production: bool, _frozen: bool, force: bool) -> anyhow::Result<()> {
    let rt = Runtime::new();
    let cwd = std::env::current_dir()?
        .display()
        .to_string()
        .replace('\\', "/")
        .replace('"', "\\\"");
    let script = format!(
        r#"const result = pm.install("{}", {}, {}, false);
io.writeln("Done! " + result.installed.toString() + " installed, " + result.cached.toString() + " from cache.")"#,
        cwd, production, force
    );
    match rt.eval(&script) {
        Ok(_) => Ok(()),
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}
