//! `raya eval` — Evaluate an inline expression or statement.

use raya_runtime::{Runtime, RuntimeOptions, Value};

pub fn execute(
    code: String,
    print: bool,
    no_print: bool,
    no_jit: bool,
) -> anyhow::Result<()> {
    let rt = Runtime::with_options(RuntimeOptions {
        no_jit,
        ..Default::default()
    });

    // Wrap bare expressions in a return statement for convenience.
    // Full programs (with function/class/import) are passed through as-is.
    let source = if needs_wrapping(&code) {
        format!("return {};", code)
    } else {
        code
    };

    let value = rt.eval(&source).map_err(|e| anyhow::anyhow!("{}", e))?;

    // Print result unless --no-print, or if --print forces it
    if !no_print && (print || !value.is_null()) {
        println!("{}", format_value(&value));
    }

    Ok(())
}

/// Check if code is a bare expression that needs wrapping in `return ...;`
fn needs_wrapping(code: &str) -> bool {
    let trimmed = code.trim();
    !trimmed.starts_with("function ")
        && !trimmed.starts_with("class ")
        && !trimmed.starts_with("return ")
        && !trimmed.starts_with("import ")
        && !trimmed.starts_with("let ")
        && !trimmed.starts_with("const ")
        && !trimmed.contains('\n')
}

/// Format a VM Value for display.
fn format_value(value: &Value) -> String {
    if value.is_null() {
        return "null".to_string();
    }
    if let Some(b) = value.as_bool() {
        return b.to_string();
    }
    if let Some(i) = value.as_i32() {
        return i.to_string();
    }
    if let Some(f) = value.as_f64() {
        if f.fract() == 0.0 && f.abs() < i64::MAX as f64 {
            return format!("{}", f as i64);
        }
        return f.to_string();
    }
    format!("{:?}", value)
}
