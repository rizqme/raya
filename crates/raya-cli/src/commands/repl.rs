//! `raya repl` — Interactive REPL.
//!
//! Persistent evaluation session with line editing, history, and multi-line
//! input support. State (variables, functions, classes, imports) is maintained
//! across inputs.

use raya_runtime::{RuntimeOptions, Session};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

const PROMPT: &str = "raya> ";
const CONTINUATION_PROMPT: &str = "  ... ";

pub fn execute(no_jit: bool) -> anyhow::Result<()> {
    let options = RuntimeOptions {
        no_jit,
        ..Default::default()
    };

    let mut session = Session::new(&options);
    let mut editor = DefaultEditor::new()?;

    // Load history if it exists
    let history_path = dirs::home_dir().map(|h| h.join(".raya").join("repl_history"));
    if let Some(ref path) = history_path {
        let _ = editor.load_history(path);
    }

    println!("Raya v{} REPL", env!("CARGO_PKG_VERSION"));
    println!("Type help for help, exit to quit\n");

    let mut buffer = String::new();

    loop {
        let prompt = if buffer.is_empty() {
            PROMPT
        } else {
            CONTINUATION_PROMPT
        };

        match editor.readline(prompt) {
            Ok(line) => {
                let trimmed = line.trim();

                // Handle empty line
                if trimmed.is_empty() {
                    if !buffer.is_empty() {
                        // In multi-line mode, empty line is appended
                        buffer.push('\n');
                    }
                    continue;
                }

                // Handle REPL commands (only when not in multi-line mode)
                if buffer.is_empty() && is_command(trimmed) {
                    let _ = editor.add_history_entry(&line);
                    if handle_command(trimmed, &mut session, &options) {
                        break; // .exit
                    }
                    continue;
                }

                // Accumulate input
                if buffer.is_empty() {
                    buffer = line.clone();
                } else {
                    buffer.push('\n');
                    buffer.push_str(&line);
                }

                // Check for incomplete input (unclosed braces, etc.)
                if is_incomplete(&buffer) {
                    continue;
                }

                let code = std::mem::take(&mut buffer);
                let _ = editor.add_history_entry(&code);

                // Auto-wrap bare expressions
                let source = if needs_wrapping(&code) {
                    format!("return {};", code)
                } else {
                    code
                };

                match session.eval(&source) {
                    Ok(value) => {
                        if !value.is_null() {
                            print_value(&session.format_value(&value));
                        }
                    }
                    Err(e) => {
                        print_error(&format!("{}", e));
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl-C: discard multi-line buffer or hint exit
                if !buffer.is_empty() {
                    buffer.clear();
                    println!();
                } else {
                    println!("\n(To exit, press Ctrl+D or type exit)");
                }
                continue;
            }
            Err(ReadlineError::Eof) => {
                // Ctrl-D: exit
                break;
            }
            Err(e) => {
                print_error(&format!("{}", e));
                break;
            }
        }
    }

    // Save history
    if let Some(ref path) = history_path {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = editor.save_history(path);
    }

    Ok(())
}

/// Check if input looks like a REPL command.
fn is_command(input: &str) -> bool {
    matches!(
        input.split_whitespace().next(),
        Some("exit" | "quit" | "help" | "clear" | "load" | "type")
    )
}

/// Handle REPL commands. Returns true if REPL should exit.
fn handle_command(cmd: &str, session: &mut Session, options: &RuntimeOptions) -> bool {
    match cmd {
        "exit" | "quit" => return true,
        "help" => {
            println!("Commands:");
            println!("  help            Show this help");
            println!("  clear           Reset session (discard all state)");
            println!("  load <file>     Load and execute a Raya file");
            println!("  type <expr>     Show the type of an expression");
            println!("  exit            Exit the REPL (also Ctrl-D)");
        }
        "clear" => {
            session.reset(options);
            println!("Session cleared.");
        }
        _ if cmd.starts_with("load ") => {
            let path = cmd.strip_prefix("load ").unwrap().trim();
            if path.is_empty() {
                print_error("Usage: load <file.raya>");
            } else {
                match std::fs::read_to_string(path) {
                    Ok(source) => match session.eval(&source) {
                        Ok(_) => println!("Loaded: {}", path),
                        Err(e) => print_error(&format!("{}", e)),
                    },
                    Err(e) => print_error(&format!("Cannot read {}: {}", path, e)),
                }
            }
        }
        _ if cmd.starts_with("type ") => {
            let expr = cmd.strip_prefix("type ").unwrap().trim();
            if expr.is_empty() {
                print_error("Usage: type <expression>");
            } else {
                let code = format!("return {};", expr);
                match session.eval(&code) {
                    Ok(value) => println!("{}", describe_type(&value)),
                    Err(e) => print_error(&format!("{}", e)),
                }
            }
        }
        _ => {
            print_error(&format!("Unknown command: {}", cmd));
            eprintln!("Type help for available commands.");
        }
    }
    false
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

/// Count open delimiters, skipping those inside strings and comments.
/// Returns true if there are unclosed delimiters.
fn is_incomplete(code: &str) -> bool {
    let mut depth = 0i32;
    let mut chars = code.chars().peekable();
    let mut in_string = false;
    let mut string_char = '"';
    let mut in_line_comment = false;
    let mut in_block_comment = false;

    while let Some(c) = chars.next() {
        if in_line_comment {
            if c == '\n' {
                in_line_comment = false;
            }
            continue;
        }
        if in_block_comment {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block_comment = false;
            }
            continue;
        }
        if in_string {
            if c == '\\' {
                chars.next();
                continue;
            }
            if c == string_char {
                in_string = false;
            }
            continue;
        }

        match c {
            '"' | '\'' | '`' => {
                in_string = true;
                string_char = c;
            }
            '/' if chars.peek() == Some(&'/') => {
                chars.next();
                in_line_comment = true;
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                in_block_comment = true;
            }
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => depth -= 1,
            _ => {}
        }
    }

    depth > 0 || in_string || in_block_comment
}

/// Describe the runtime type of a value.
fn describe_type(value: &raya_runtime::Value) -> String {
    if value.is_null() {
        "null".into()
    } else if value.is_bool() {
        "boolean".into()
    } else if value.is_i32() {
        "int".into()
    } else if value.is_f64() {
        "number".into()
    } else if value.is_ptr() {
        "object".into()
    } else {
        "unknown".into()
    }
}

fn print_error(msg: &str) {
    eprintln!("\x1b[31m{}\x1b[0m", msg);
}

fn print_value(formatted: &str) {
    println!("\x1b[36m{}\x1b[0m", formatted);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_expression() {
        assert!(!is_incomplete("1 + 2"));
    }

    #[test]
    fn incomplete_brace() {
        assert!(is_incomplete("function f() {"));
    }

    #[test]
    fn complete_brace() {
        assert!(!is_incomplete("function f() { return 1; }"));
    }

    #[test]
    fn incomplete_string() {
        assert!(is_incomplete("let s = \"hello"));
    }

    #[test]
    fn braces_in_string_ignored() {
        assert!(!is_incomplete("let s = \"{\""));
    }

    #[test]
    fn nested_braces() {
        assert!(is_incomplete("if (true) { if (false) {"));
    }

    #[test]
    fn block_comment_incomplete() {
        assert!(is_incomplete("/* comment"));
    }

    #[test]
    fn block_comment_complete() {
        assert!(!is_incomplete("/* comment */"));
    }

    #[test]
    fn line_comment_does_not_affect() {
        assert!(!is_incomplete("let x = 1 // comment"));
    }

    #[test]
    fn escaped_quote_in_string() {
        assert!(!is_incomplete("let s = \"he said \\\"hi\\\"\""));
    }

    #[test]
    fn needs_wrapping_bare_expression() {
        assert!(needs_wrapping("1 + 2"));
        assert!(needs_wrapping("x * 2"));
        assert!(needs_wrapping("\"hello\""));
    }

    #[test]
    fn needs_wrapping_declarations() {
        assert!(!needs_wrapping("let x = 1"));
        assert!(!needs_wrapping("const y = 2"));
        assert!(!needs_wrapping("function f() {}"));
        assert!(!needs_wrapping("class Foo {}"));
        assert!(!needs_wrapping("import { Math } from \"std:math\""));
        assert!(!needs_wrapping("return 42"));
    }

    #[test]
    fn needs_wrapping_multiline() {
        assert!(!needs_wrapping("function f() {\nreturn 1;\n}"));
    }
}
