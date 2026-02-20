//! `raya lint` — Lint source files for style and correctness issues.

use std::path::PathBuf;

use crate::output::{resolve_color_choice, StyledOutput};
use raya_engine::linter::{LintConfig, LintDiagnostic, Linter, Severity};
use raya_engine::parser::checker::diagnostic::{Diagnostic, ErrorCode, SimpleFiles};

use super::files::collect_raya_files;

pub fn execute(
    files: Vec<String>,
    fix: bool,
    format: String,
    watch: bool,
) -> anyhow::Result<()> {
    let _ = watch; // TODO: watch mode

    // 1. Load lint config from raya.toml (if present)
    let config = load_lint_config();
    let linter = match config {
        Some(cfg) => Linter::with_config(cfg),
        None => Linter::new(),
    };

    // 2. Collect source files
    let source_files = collect_raya_files(&files)?;
    if source_files.is_empty() {
        eprintln!("No .raya files found.");
        std::process::exit(1);
    }

    // 3. Lint each file
    let mut total_errors = 0usize;
    let mut total_warnings = 0usize;
    let mut total_fixable = 0usize;
    let mut all_file_results: Vec<FileLintResult> = Vec::new();

    for path in &source_files {
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading {}: {}", path.display(), e);
                total_errors += 1;
                continue;
            }
        };
        let path_str = path.display().to_string();

        let result = linter.lint_source(&source, &path_str);

        let errors = result
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, Severity::Error))
            .count();
        let warnings = result
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, Severity::Warn))
            .count();

        total_errors += errors;
        total_warnings += warnings;
        total_fixable += result.fixable_count;

        if !result.diagnostics.is_empty() {
            all_file_results.push(FileLintResult {
                path: path.clone(),
                source,
                diagnostics: result.diagnostics,
            });
        }
    }

    // 4. Output diagnostics
    match format.as_str() {
        "json" => emit_json(&all_file_results),
        _ => emit_pretty(&all_file_results),
    }

    // 5. Apply fixes
    let mut out = StyledOutput::new(resolve_color_choice(None));

    if fix && total_fixable > 0 {
        let (fixed_issues, fixed_files) = apply_fixes(&all_file_results)?;
        out.newline();
        out.success(&format!(
            "Fixed {} issue(s) in {} file(s).",
            fixed_issues, fixed_files
        ));
        out.newline();
    } else if total_fixable > 0 {
        out.newline();
        out.info(&format!(
            "{} issue(s) are auto-fixable. Run `raya lint --fix` to apply.",
            total_fixable
        ));
        out.newline();
    }

    // 6. Summary
    if format != "json" {
        print_summary(
            &mut out,
            source_files.len(),
            total_errors,
            total_warnings,
        );
    }

    // 7. Exit code
    if total_errors > 0 {
        std::process::exit(1);
    }
    Ok(())
}

struct FileLintResult {
    path: PathBuf,
    source: String,
    diagnostics: Vec<LintDiagnostic>,
}

// ── Pretty output (codespan bridge) ────────────────────────────────────────

fn emit_pretty(results: &[FileLintResult]) {
    for file_result in results {
        let mut files = SimpleFiles::new();
        let file_id = files.add(
            file_result.path.display().to_string(),
            file_result.source.clone(),
        );

        for lint_diag in &file_result.diagnostics {
            let mut diag = match lint_diag.severity {
                Severity::Error => Diagnostic::error(&lint_diag.message),
                Severity::Warn => Diagnostic::warning(&lint_diag.message),
                Severity::Off => continue,
            };

            diag = diag
                .with_code(ErrorCode(lint_diag.code))
                .with_primary_label(file_id, lint_diag.span, lint_diag.rule);

            if let Some(fix) = &lint_diag.fix {
                diag = diag.with_help(format!("replace with '{}'", fix.replacement));
            }

            for note in &lint_diag.notes {
                diag = diag.with_note(note);
            }

            let _ = diag.emit(&files);
        }
    }
}

// ── JSON output ────────────────────────────────────────────────────────────

fn emit_json(results: &[FileLintResult]) {
    print!("[");
    let mut first_file = true;
    for file_result in results {
        if !first_file {
            print!(",");
        }
        first_file = false;

        print!(
            r#"{{"file":"{}","diagnostics":["#,
            file_result.path.display()
        );

        let mut first_diag = true;
        for d in &file_result.diagnostics {
            if !first_diag {
                print!(",");
            }
            first_diag = false;

            let severity = match d.severity {
                Severity::Error => "error",
                Severity::Warn => "warn",
                Severity::Off => continue,
            };

            print!(
                r#"{{"code":"{}","rule":"{}","severity":"{}","message":"{}","span":{{"start":{},"end":{},"line":{},"column":{}}}"#,
                d.code,
                d.rule,
                severity,
                d.message.replace('"', "\\\""),
                d.span.start,
                d.span.end,
                d.span.line,
                d.span.column,
            );

            if let Some(fix) = &d.fix {
                print!(
                    r#","fix":{{"start":{},"end":{},"replacement":"{}"}}"#,
                    fix.span.start,
                    fix.span.end,
                    fix.replacement.replace('"', "\\\"")
                );
            }

            print!("}}");
        }

        print!("]}}");
    }
    println!("]");
}

// ── Auto-fix application ───────────────────────────────────────────────────

fn apply_fixes(results: &[FileLintResult]) -> anyhow::Result<(usize, usize)> {
    let mut total_fixed = 0usize;
    let mut files_fixed = 0usize;

    for file_result in results {
        let fixes: Vec<_> = file_result
            .diagnostics
            .iter()
            .filter_map(|d| d.fix.as_ref())
            .collect();

        if fixes.is_empty() {
            continue;
        }

        let mut source = file_result.source.clone();

        // Sort fixes by span start descending (apply from end to start
        // so earlier offsets aren't invalidated).
        let mut sorted_fixes = fixes.clone();
        sorted_fixes.sort_by(|a, b| b.span.start.cmp(&a.span.start));

        // Check for overlapping fixes and skip overlaps.
        let mut last_start = usize::MAX;
        let mut applied = 0;
        for fix in &sorted_fixes {
            let start = fix.span.start;
            let end = fix.span.end;

            // Skip if this fix overlaps with a previously applied one.
            if end > last_start {
                continue;
            }

            source.replace_range(start..end, &fix.replacement);
            last_start = start;
            applied += 1;
        }

        if applied > 0 {
            std::fs::write(&file_result.path, &source)?;
            total_fixed += applied;
            files_fixed += 1;
        }
    }

    Ok((total_fixed, files_fixed))
}

// ── Colored summary ────────────────────────────────────────────────────────

fn print_summary(out: &mut StyledOutput, file_count: usize, errors: usize, warnings: usize) {
    out.newline();
    if errors == 0 && warnings == 0 {
        out.plain(&format!(
            "Linted {} file{} — ",
            file_count,
            if file_count == 1 { "" } else { "s" }
        ));
        out.success("no issues found.");
        out.newline();
        return;
    }

    out.plain(&format!(
        "Linted {} file{}: ",
        file_count,
        if file_count == 1 { "" } else { "s" }
    ));
    if errors > 0 {
        out.error(&format!(
            "{} error{}",
            errors,
            if errors == 1 { "" } else { "s" }
        ));
    }
    if errors > 0 && warnings > 0 {
        out.plain(", ");
    }
    if warnings > 0 {
        out.warning(&format!(
            "{} warning{}",
            warnings,
            if warnings == 1 { "" } else { "s" }
        ));
    }
    out.plain(".");
    out.newline();
}

// ── Config loading ─────────────────────────────────────────────────────────

fn load_lint_config() -> Option<LintConfig> {
    let manifest_path = find_manifest()?;
    let manifest = raya_pm::PackageManifest::from_file(&manifest_path).ok()?;
    let lint_manifest = manifest.lint?;

    let mut config = LintConfig::new();
    for (rule_name, severity_str) in &lint_manifest.rules {
        let severity = match severity_str.as_str() {
            "off" => Severity::Off,
            "warn" | "warning" => Severity::Warn,
            "error" => Severity::Error,
            _ => continue,
        };
        config.set_severity(rule_name, severity);
    }
    Some(config)
}

/// Walk up from CWD to find `raya.toml`.
fn find_manifest() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let candidate = dir.join("raya.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}
