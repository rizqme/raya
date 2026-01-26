//! Diagnostic infrastructure for error reporting
//!
//! Provides structured error reporting with source code context,
//! helpful suggestions, and multiple output formats.

use codespan_reporting::diagnostic::{Diagnostic as CsDiagnostic, Label, Severity};
use codespan_reporting::files::{Files, SimpleFiles};
use codespan_reporting::term;
use codespan_reporting::term::termcolor::{ColorChoice, StandardStream};
use raya_parser::Span;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::{BindError, CheckError};

/// Error code for a diagnostic
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorCode(pub &'static str);

impl ErrorCode {
    pub fn as_str(&self) -> &str {
        self.0
    }
}

/// A diagnostic message with source code context
pub struct Diagnostic {
    /// The underlying codespan diagnostic
    inner: CsDiagnostic<usize>,
    /// Error code (e.g., "E2001")
    code: Option<ErrorCode>,
}

impl Diagnostic {
    /// Create a new diagnostic
    pub fn new(severity: Severity, message: impl Into<String>) -> Self {
        Diagnostic {
            inner: CsDiagnostic::new(severity).with_message(message),
            code: None,
        }
    }

    /// Create an error diagnostic
    pub fn error(message: impl Into<String>) -> Self {
        Self::new(Severity::Error, message)
    }

    /// Create a warning diagnostic
    pub fn warning(message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, message)
    }

    /// Create a note diagnostic
    pub fn note(message: impl Into<String>) -> Self {
        Self::new(Severity::Note, message)
    }

    /// Set the error code
    pub fn with_code(mut self, code: ErrorCode) -> Self {
        self.code = Some(code.clone());
        self.inner = self.inner.with_code(code.0);
        self
    }

    /// Add a primary label (main error location)
    pub fn with_primary_label(mut self, file_id: usize, span: Span, message: impl Into<String>) -> Self {
        let label = Label::primary(file_id, span.start as usize..span.end as usize)
            .with_message(message);
        self.inner = self.inner.with_labels(vec![label]);
        self
    }

    /// Add a secondary label (related location)
    pub fn with_secondary_label(mut self, file_id: usize, span: Span, message: impl Into<String>) -> Self {
        let label = Label::secondary(file_id, span.start as usize..span.end as usize)
            .with_message(message);
        // Clone the existing diagnostic and add the new label
        let existing_labels = std::mem::take(&mut self.inner.labels);
        let mut new_labels = existing_labels;
        new_labels.push(label);
        self.inner.labels = new_labels;
        self
    }

    /// Add a note (additional context)
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.inner.notes.push(note.into());
        self
    }

    /// Add a help suggestion
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.inner.notes.push(format!("help: {}", help.into()));
        self
    }

    /// Create diagnostic from a CheckError
    pub fn from_check_error(error: &CheckError, file_id: usize) -> Self {
        use CheckError::*;

        match error {
            TypeMismatch { expected, actual, span, note } => {
                let mut diag = Diagnostic::error(format!(
                    "Type '{}' is not assignable to type '{}'",
                    actual, expected
                ))
                .with_code(error_code(error))
                .with_primary_label(file_id, *span, format!("expected '{}', found '{}'", expected, actual));

                if let Some(note_text) = note {
                    diag = diag.with_note(note_text);
                }

                // Add helpful suggestion for common cases
                if actual.contains('|') && !expected.contains('|') {
                    diag = diag.with_help("Use typeof to narrow the union type");
                }

                diag
            }

            UndefinedVariable { name, span } => {
                Diagnostic::error(format!("Cannot find name '{}'", name))
                    .with_code(error_code(error))
                    .with_primary_label(file_id, *span, "not found in this scope")
            }

            NotCallable { ty, span } => {
                Diagnostic::error(format!("Type '{}' is not callable", ty))
                    .with_code(error_code(error))
                    .with_primary_label(file_id, *span, "cannot be called")
            }

            ArgumentCountMismatch { expected, actual, span } => {
                Diagnostic::error(format!(
                    "Expected {} argument{}, but got {}",
                    expected,
                    if *expected == 1 { "" } else { "s" },
                    actual
                ))
                .with_code(error_code(error))
                .with_primary_label(file_id, *span, "incorrect number of arguments")
            }

            PropertyNotFound { property, ty, span } => {
                Diagnostic::error(format!("Property '{}' does not exist on type '{}'", property, ty))
                    .with_code(error_code(error))
                    .with_primary_label(file_id, *span, "property not found")
            }

            NonExhaustiveMatch { missing, span } => {
                let mut diag = Diagnostic::error("Match is not exhaustive")
                    .with_code(error_code(error))
                    .with_primary_label(file_id, *span, "missing cases");

                if !missing.is_empty() {
                    diag = diag.with_note(format!("Missing cases: {}", missing.join(", ")));
                    diag = diag.with_help("Add cases for all variants or add a default case");
                }

                diag
            }

            ReturnTypeMismatch { expected, actual, span } => {
                Diagnostic::error(format!(
                    "Return type '{}' is not assignable to declared type '{}'",
                    actual, expected
                ))
                .with_code(error_code(error))
                .with_primary_label(file_id, *span, format!("expected '{}', found '{}'", expected, actual))
            }

            InvalidBinaryOp { op, left, right, span } => {
                Diagnostic::error(format!(
                    "Operator '{}' cannot be applied to types '{}' and '{}'",
                    op, left, right
                ))
                .with_code(error_code(error))
                .with_primary_label(file_id, *span, "invalid operation")
            }

            InvalidUnaryOp { op, ty, span } => {
                Diagnostic::error(format!(
                    "Operator '{}' cannot be applied to type '{}'",
                    op, ty
                ))
                .with_code(error_code(error))
                .with_primary_label(file_id, *span, "invalid operation")
            }

            BreakOutsideLoop { span } => {
                Diagnostic::error("'break' can only be used inside a loop")
                    .with_code(error_code(error))
                    .with_primary_label(file_id, *span, "not inside a loop")
            }

            ContinueOutsideLoop { span } => {
                Diagnostic::error("'continue' can only be used inside a loop")
                    .with_code(error_code(error))
                    .with_primary_label(file_id, *span, "not inside a loop")
            }

            ReturnOutsideFunction { span } => {
                Diagnostic::error("'return' can only be used inside a function")
                    .with_code(error_code(error))
                    .with_primary_label(file_id, *span, "not inside a function")
            }

            GenericInstantiationError { message, span } => {
                Diagnostic::error(format!("Generic instantiation failed: {}", message))
                    .with_code(error_code(error))
                    .with_primary_label(file_id, *span, "instantiation error")
            }

            ConstraintViolation { message, span } => {
                Diagnostic::error(format!("Type constraint violation: {}", message))
                    .with_code(error_code(error))
                    .with_primary_label(file_id, *span, "constraint not satisfied")
            }

            ForbiddenFieldAccess { field, span } => {
                Diagnostic::error(format!(
                    "Cannot access internal field '{}' on bare union",
                    field
                ))
                .with_code(error_code(error))
                .with_primary_label(file_id, *span, "forbidden field access")
                .with_note("Bare unions use typeof for type narrowing")
                .with_help(format!("Use typeof instead: typeof x === \"string\""))
            }
        }
    }

    /// Create diagnostic from a BindError
    pub fn from_bind_error(error: &BindError, file_id: usize) -> Self {
        use BindError::*;

        match error {
            DuplicateSymbol { name, original, duplicate } => {
                Diagnostic::error(format!("Duplicate identifier '{}'", name))
                    .with_code(ErrorCode("E3001"))
                    .with_primary_label(file_id, *duplicate, "duplicate declaration")
                    .with_secondary_label(file_id, *original, "first declaration here")
            }

            UndefinedType { name, span } => {
                Diagnostic::error(format!("Cannot find type '{}'", name))
                    .with_code(ErrorCode("E3002"))
                    .with_primary_label(file_id, *span, "type not found")
            }

            NotAType { name, span } => {
                Diagnostic::error(format!("'{}' refers to a value, but is being used as a type", name))
                    .with_code(ErrorCode("E3003"))
                    .with_primary_label(file_id, *span, "not a type")
            }

            InvalidTypeExpr { message, span } => {
                Diagnostic::error(format!("Invalid type expression: {}", message))
                    .with_code(ErrorCode("E3004"))
                    .with_primary_label(file_id, *span, "invalid type")
            }
        }
    }

    /// Emit the diagnostic to stderr with colors
    pub fn emit(&self, files: &SimpleFiles<String, String>) -> Result<(), codespan_reporting::files::Error> {
        let mut writer = StandardStream::stderr(ColorChoice::Auto);
        let config = codespan_reporting::term::Config::default();
        term::emit(&mut writer, &config, files, &self.inner)
    }

    /// Get the underlying codespan diagnostic (for testing/custom rendering)
    pub fn inner(&self) -> &CsDiagnostic<usize> {
        &self.inner
    }

    /// Convert to JSON representation for IDE integration
    pub fn to_json(&self, files: &SimpleFiles<String, String>) -> Result<String, serde_json::Error> {
        let json_diag = JsonDiagnostic::from_diagnostic(self, files);
        serde_json::to_string_pretty(&json_diag)
    }
}

/// JSON representation of a diagnostic for IDE integration
#[derive(Debug, Serialize, Deserialize)]
pub struct JsonDiagnostic {
    /// Error code (e.g., "E2001")
    pub code: Option<String>,
    /// Severity level
    pub severity: String,
    /// Main error message
    pub message: String,
    /// Source locations with labels
    pub labels: Vec<JsonLabel>,
    /// Additional notes and help
    pub notes: Vec<String>,
}

/// JSON representation of a diagnostic label
#[derive(Debug, Serialize, Deserialize)]
pub struct JsonLabel {
    /// File path
    pub file: String,
    /// Start line (1-indexed)
    pub start_line: usize,
    /// Start column (1-indexed)
    pub start_column: usize,
    /// End line (1-indexed)
    pub end_line: usize,
    /// End column (1-indexed)
    pub end_column: usize,
    /// Label message
    pub message: Option<String>,
    /// Label style (primary or secondary)
    pub style: String,
}

impl JsonDiagnostic {
    /// Convert a Diagnostic to JSON representation
    pub fn from_diagnostic(diag: &Diagnostic, files: &SimpleFiles<String, String>) -> Self {
        let severity = match diag.inner.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Note => "note",
            Severity::Help => "help",
            Severity::Bug => "bug",
        };

        let labels = diag.inner.labels.iter().filter_map(|label| {
            // Try to get file name and location info
            let file_id = label.file_id;
            let file_name = files.get(file_id).ok()?.name().to_string();

            // Get start and end locations
            let start = label.range.start;
            let end = label.range.end;

            // Get line and column information
            let start_location = files.get(file_id).ok()?.location((), start).ok()?;
            let end_location = files.get(file_id).ok()?.location((), end).ok()?;

            Some(JsonLabel {
                file: file_name,
                start_line: start_location.line_number,
                start_column: start_location.column_number,
                end_line: end_location.line_number,
                end_column: end_location.column_number,
                message: Some(label.message.clone()),
                style: match label.style {
                    codespan_reporting::diagnostic::LabelStyle::Primary => "primary",
                    codespan_reporting::diagnostic::LabelStyle::Secondary => "secondary",
                }.to_string(),
            })
        }).collect();

        JsonDiagnostic {
            code: diag.code.as_ref().map(|c| c.0.to_string()),
            severity: severity.to_string(),
            message: diag.inner.message.clone(),
            labels,
            notes: diag.inner.notes.clone(),
        }
    }
}

/// Get error code for a CheckError
pub fn error_code(error: &CheckError) -> ErrorCode {
    use CheckError::*;

    match error {
        TypeMismatch { .. } => ErrorCode("E2001"),
        UndefinedVariable { .. } => ErrorCode("E2002"),
        ForbiddenFieldAccess { .. } => ErrorCode("E2003"),
        NotCallable { .. } => ErrorCode("E2004"),
        ArgumentCountMismatch { .. } => ErrorCode("E2005"),
        PropertyNotFound { .. } => ErrorCode("E2006"),
        NonExhaustiveMatch { .. } => ErrorCode("E2007"),
        ReturnTypeMismatch { .. } => ErrorCode("E2008"),
        InvalidBinaryOp { .. } => ErrorCode("E2009"),
        InvalidUnaryOp { .. } => ErrorCode("E2010"),
        BreakOutsideLoop { .. } => ErrorCode("E2011"),
        ContinueOutsideLoop { .. } => ErrorCode("E2012"),
        ReturnOutsideFunction { .. } => ErrorCode("E2013"),
        GenericInstantiationError { .. } => ErrorCode("E2014"),
        ConstraintViolation { .. } => ErrorCode("E2015"),
    }
}

/// Helper to create a SimpleFiles instance from source code
pub fn create_files(path: impl Into<PathBuf>, source: impl Into<String>) -> SimpleFiles<String, String> {
    let mut files = SimpleFiles::new();
    files.add(path.into().display().to_string(), source.into());
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use raya_parser::Span;

    #[test]
    fn test_create_error_diagnostic() {
        let diag = Diagnostic::error("Test error message");
        assert_eq!(diag.inner.severity, Severity::Error);
    }

    #[test]
    fn test_diagnostic_with_code() {
        let diag = Diagnostic::error("Test error")
            .with_code(ErrorCode("E2001"));

        assert_eq!(diag.code, Some(ErrorCode("E2001")));
    }

    #[test]
    fn test_from_check_error_type_mismatch() {
        let error = CheckError::TypeMismatch {
            expected: "string".to_string(),
            actual: "number".to_string(),
            span: Span::new(10, 15, 1, 10),
            note: None,
        };

        let diag = Diagnostic::from_check_error(&error, 0);
        assert_eq!(diag.inner.severity, Severity::Error);
        assert_eq!(diag.code, Some(ErrorCode("E2001")));
    }

    #[test]
    fn test_from_check_error_forbidden_field() {
        let error = CheckError::ForbiddenFieldAccess {
            field: "$type".to_string(),
            span: Span::new(20, 25, 2, 5),
        };

        let diag = Diagnostic::from_check_error(&error, 0);
        assert_eq!(diag.code, Some(ErrorCode("E2003")));
        assert!(diag.inner.message.contains("$type"));
    }

    #[test]
    fn test_json_output() {
        let error = CheckError::TypeMismatch {
            expected: "string".to_string(),
            actual: "number".to_string(),
            span: Span::new(10, 15, 1, 10),
            note: None,
        };

        let diag = Diagnostic::from_check_error(&error, 0);
        let files = create_files("test.raya", "let x: string = 42;");

        let json = diag.to_json(&files).unwrap();

        // Verify JSON structure
        assert!(json.contains("\"code\""));
        assert!(json.contains("\"E2001\""));
        assert!(json.contains("\"severity\""));
        assert!(json.contains("\"error\""));
        assert!(json.contains("\"message\""));
    }

    #[test]
    fn test_json_labels() {
        let error = CheckError::UndefinedVariable {
            name: "foo".to_string(),
            span: Span::new(5, 8, 1, 5),
        };

        let diag = Diagnostic::from_check_error(&error, 0);
        let files = create_files("test.raya", "let x = foo;");

        let json = diag.to_json(&files).unwrap();

        // Verify label information is included
        assert!(json.contains("\"labels\""));
        assert!(json.contains("\"file\""));
        assert!(json.contains("\"start_line\""));
        assert!(json.contains("\"start_column\""));
    }
}
