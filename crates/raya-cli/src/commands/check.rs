//! `raya check` — Type-check without building.

use raya_engine::parser::checker::{
    CheckError, CheckWarning, BindError, Diagnostic, SimpleFiles, WarningCode, WarningConfig,
};
use raya_engine::parser::Span;
use raya_runtime::Runtime;

use super::files::collect_raya_files;

pub fn execute(
    files: Vec<String>,
    _watch: bool,
    strict: bool,
    format: String,
    allow: Vec<String>,
    deny: Vec<String>,
    no_warnings: bool,
) -> anyhow::Result<()> {
    let raya_files = collect_raya_files(&files)?;

    if raya_files.is_empty() {
        eprintln!("No .raya files found.");
        std::process::exit(1);
    }

    let warning_config = build_warning_config(strict, &allow, &deny, no_warnings);
    let rt = Runtime::new();

    let mut total_errors = 0usize;
    let mut total_warnings = 0usize;
    let mut total_files = 0usize;

    for file_path in &raya_files {
        total_files += 1;

        let source = match std::fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading {}: {}", file_path.display(), e);
                total_errors += 1;
                continue;
            }
        };

        let diagnostics = match rt.check(&source) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("{}: {}", file_path.display(), e);
                total_errors += 1;
                continue;
            }
        };

        let offset = diagnostics.user_offset;

        // Register only the user's source for pretty printing
        let mut cs_files = SimpleFiles::new();
        let file_id = cs_files.add(file_path.display().to_string(), source.clone());

        // Emit bind errors
        for err in &diagnostics.bind_errors {
            if let Some(adjusted) = adjust_bind_error(err, offset) {
                let diag = Diagnostic::from_bind_error(&adjusted, file_id);
                emit_diagnostic(&diag, &cs_files, &format);
                total_errors += 1;
            }
        }

        // Emit check errors
        for err in &diagnostics.errors {
            if let Some(adjusted) = adjust_check_error(err, offset) {
                let diag = Diagnostic::from_check_error(&adjusted, file_id);
                emit_diagnostic(&diag, &cs_files, &format);
                total_errors += 1;
            }
        }

        // Emit warnings (filtered by config)
        if !no_warnings {
            for warn in &diagnostics.warnings {
                let code = warn.code();
                if !warning_config.is_enabled(code) {
                    continue;
                }

                if let Some(adjusted) = adjust_warning(warn, offset) {
                    if warning_config.is_denied(code) {
                        // Promoted to error — emit as error
                        let diag = Diagnostic::from_check_warning(&adjusted, file_id);
                        emit_diagnostic(&diag, &cs_files, &format);
                        total_errors += 1;
                    } else {
                        let diag = Diagnostic::from_check_warning(&adjusted, file_id);
                        emit_diagnostic(&diag, &cs_files, &format);
                        total_warnings += 1;
                    }
                }
            }
        }
    }

    // Summary
    if format != "json" {
        if total_errors == 0 && total_warnings == 0 {
            eprintln!(
                "Checked {} file{} — no errors.",
                total_files,
                if total_files == 1 { "" } else { "s" }
            );
        } else {
            let mut parts = Vec::new();
            if total_errors > 0 {
                parts.push(format!(
                    "{} error{}",
                    total_errors,
                    if total_errors == 1 { "" } else { "s" }
                ));
            }
            if total_warnings > 0 {
                parts.push(format!(
                    "{} warning{}",
                    total_warnings,
                    if total_warnings == 1 { "" } else { "s" }
                ));
            }
            eprintln!(
                "Checked {} file{} — {}.",
                total_files,
                if total_files == 1 { "" } else { "s" },
                parts.join(", ")
            );
        }
    }

    if total_errors > 0 {
        std::process::exit(1);
    }

    Ok(())
}

fn emit_diagnostic(
    diag: &Diagnostic,
    files: &SimpleFiles<String, String>,
    format: &str,
) {
    match format {
        "json" => {
            if let Ok(json) = diag.to_json(files) {
                println!("{}", json);
            }
        }
        _ => {
            let _ = diag.emit(files);
        }
    }
}

fn build_warning_config(
    strict: bool,
    allow: &[String],
    deny: &[String],
    no_warnings: bool,
) -> WarningConfig {
    let mut config = if strict {
        WarningConfig::strict()
    } else {
        WarningConfig::default()
    };

    if no_warnings {
        // Disable all known warning codes
        config.disabled.insert(WarningCode::UnusedVariable);
        config.disabled.insert(WarningCode::UnusedImport);
        config.disabled.insert(WarningCode::UnusedParameter);
        config.disabled.insert(WarningCode::UnreachableCode);
        config.disabled.insert(WarningCode::ShadowedVariable);
    }

    for name in allow {
        if let Some(code) = WarningCode::from_name(name) {
            config.disabled.insert(code);
        } else {
            eprintln!("Warning: unknown warning code '{}'", name);
        }
    }

    for name in deny {
        if let Some(code) = WarningCode::from_name(name) {
            config.deny.insert(code);
        } else {
            eprintln!("Warning: unknown warning code '{}'", name);
        }
    }

    config
}

// ── Span adjustment ─────────────────────────────────────────────────────────
//
// The checker receives the full source (builtins + stdlib + user code).
// All spans are relative to the full source. We subtract `user_offset`
// so spans are relative to the user's file only. Diagnostics whose spans
// fall within the builtin/stdlib prefix are skipped (return None).

fn adjust_span(span: Span, offset: usize) -> Option<Span> {
    let start = span.start as usize;
    let end = span.end as usize;

    if end <= offset {
        return None; // Entirely in builtin/stdlib
    }

    let new_start = if start >= offset { start - offset } else { 0 };
    let new_end = end - offset;

    Some(Span::new(new_start, new_end, span.line, span.column))
}

fn adjust_check_error(err: &CheckError, offset: usize) -> Option<CheckError> {
    let span = err.span();
    if (span.end as usize) <= offset {
        return None;
    }

    let mut adjusted = err.clone();
    adjust_check_error_spans(&mut adjusted, offset);
    Some(adjusted)
}

fn adjust_check_error_spans(err: &mut CheckError, offset: usize) {
    match err {
        CheckError::TypeMismatch { span, .. }
        | CheckError::UndefinedVariable { span, .. }
        | CheckError::NotCallable { span, .. }
        | CheckError::ArgumentCountMismatch { span, .. }
        | CheckError::NonExhaustiveMatch { span, .. }
        | CheckError::PropertyNotFound { span, .. }
        | CheckError::ReturnTypeMismatch { span, .. }
        | CheckError::InvalidBinaryOp { span, .. }
        | CheckError::InvalidUnaryOp { span, .. }
        | CheckError::BreakOutsideLoop { span }
        | CheckError::ContinueOutsideLoop { span }
        | CheckError::ReturnOutsideFunction { span }
        | CheckError::GenericInstantiationError { span, .. }
        | CheckError::ConstraintViolation { span, .. }
        | CheckError::ForbiddenFieldAccess { span, .. }
        | CheckError::AbstractClassInstantiation { span, .. }
        | CheckError::UndefinedMember { span, .. }
        | CheckError::InvalidDecorator { span, .. }
        | CheckError::DecoratorSignatureMismatch { span, .. }
        | CheckError::DecoratorReturnMismatch { span, .. }
        | CheckError::ReadonlyAssignment { span, .. }
        | CheckError::ConstReassignment { span, .. }
        | CheckError::NewNonClass { span, .. } => {
            if let Some(new_span) = adjust_span(*span, offset) {
                *span = new_span;
            }
        }
    }
}

fn adjust_bind_error(err: &BindError, offset: usize) -> Option<BindError> {
    let span = err.span();
    if (span.end as usize) <= offset {
        return None;
    }

    let mut adjusted = err.clone();
    match &mut adjusted {
        BindError::DuplicateSymbol { original, duplicate, .. } => {
            if let Some(s) = adjust_span(*duplicate, offset) { *duplicate = s; }
            if let Some(s) = adjust_span(*original, offset) { *original = s; }
        }
        BindError::UndefinedType { span, .. }
        | BindError::NotAType { span, .. }
        | BindError::InvalidTypeExpr { span, .. }
        | BindError::InvalidTypeArguments { span, .. } => {
            if let Some(s) = adjust_span(*span, offset) { *span = s; }
        }
    }
    Some(adjusted)
}

fn adjust_warning(warn: &CheckWarning, offset: usize) -> Option<CheckWarning> {
    let span = warn.span();
    if (span.end as usize) <= offset {
        return None;
    }

    let mut adjusted = warn.clone();
    match &mut adjusted {
        CheckWarning::UnusedVariable { span, .. } => {
            if let Some(s) = adjust_span(*span, offset) { *span = s; }
        }
        CheckWarning::UnreachableCode { span } => {
            if let Some(s) = adjust_span(*span, offset) { *span = s; }
        }
        CheckWarning::ShadowedVariable { original, shadow, .. } => {
            if let Some(s) = adjust_span(*shadow, offset) { *shadow = s; }
            if let Some(s) = adjust_span(*original, offset) { *original = s; }
        }
    }
    Some(adjusted)
}
