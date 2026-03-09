//! `raya eval` — Evaluate an inline expression or statement.

use raya_runtime::{BuiltinMode, RuntimeOptions, Session, TypeMode};

pub fn execute(
    code: String,
    print: bool,
    no_print: bool,
    no_jit: bool,
    jit_threshold: u32,
    node_compat: bool,
    type_mode: TypeMode,
) -> anyhow::Result<()> {
    if matches!(type_mode, TypeMode::Ts | TypeMode::Js) && !node_compat {
        anyhow::bail!("--mode ts/js requires --node-compat");
    }
    let options = RuntimeOptions {
        no_jit,
        jit_threshold,
        builtin_mode: if node_compat {
            BuiltinMode::NodeCompat
        } else {
            BuiltinMode::RayaStrict
        },
        type_mode: Some(type_mode),
        ts_options: None,
        ..Default::default()
    };
    let mut session = Session::new(&options);

    // Wrap bare expressions in a return statement for convenience.
    // Full programs (with function/class/import) are passed through as-is.
    let source = if needs_wrapping(&code) {
        format!("return {};", code)
    } else {
        code
    };

    let value = session
        .eval(&source)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // Print result unless --no-print, or if --print forces it
    if !no_print && (print || !value.is_null()) {
        println!("{}", session.format_value(&value));
    }

    Ok(())
}

/// Check if code is a bare expression that needs wrapping in `return ...;`
fn needs_wrapping(code: &str) -> bool {
    let trimmed = code.trim();
    !trimmed.starts_with("function ")
        && !trimmed.starts_with("async function ")
        && !trimmed.starts_with("class ")
        && !trimmed.starts_with("interface ")
        && !trimmed.starts_with("enum ")
        && !trimmed.starts_with("type ")
        && !trimmed.starts_with("abstract ")
        && !trimmed.starts_with("export ")
        && !trimmed.starts_with("return ")
        && !trimmed.starts_with("import ")
        && !trimmed.starts_with("let ")
        && !trimmed.starts_with("var ")
        && !trimmed.starts_with("const ")
        && !trimmed.starts_with("try ")
        && !trimmed.starts_with("if ")
        && !trimmed.starts_with("for ")
        && !trimmed.starts_with("while ")
        && !trimmed.starts_with("switch ")
        && !trimmed.starts_with("throw ")
        && !trimmed.contains('\n')
}

#[cfg(test)]
mod tests {
    use super::needs_wrapping;

    #[test]
    fn does_not_wrap_async_function_declaration() {
        assert!(!needs_wrapping(
            "async function ok() { return 1; } return ok();"
        ));
    }

    #[test]
    fn wraps_simple_expression() {
        assert!(needs_wrapping("1 + 2 * 3"));
    }

    #[test]
    fn does_not_wrap_interface_declaration() {
        assert!(!needs_wrapping("interface Box { value: number }"));
    }

    #[test]
    fn does_not_wrap_export_statement() {
        assert!(!needs_wrapping("export const x = 1;"));
    }
}
