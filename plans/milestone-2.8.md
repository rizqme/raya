# Milestone 2.8: Error Reporting & Diagnostics

**Duration:** 1-2 weeks
**Status:** 🔴 Not Started
**Dependencies:**
- Milestone 2.3 (Parser) ✅ Complete
- Milestone 2.5 (Type Checker) ✅ Complete
**Next Milestone:** 3.1 (IR - Intermediate Representation)

---

## Table of Contents

1. [Overview](#overview)
2. [Goals](#goals)
3. [Non-Goals](#non-goals)
4. [Architecture](#architecture)
5. [Phase 1: Error Formatting & Display](#phase-1-error-formatting--display-week-1)
6. [Phase 2: Advanced Diagnostics](#phase-2-advanced-diagnostics-week-2)
7. [Testing Strategy](#testing-strategy)
8. [Success Criteria](#success-criteria)

---

## Overview

Implement comprehensive error reporting and diagnostics for the Raya compiler. The goal is to provide clear, actionable error messages with source code context, suggestions for fixes, and support for multiple output formats.

### What is Error Reporting?

Error reporting transforms internal compiler errors into human-readable messages that help developers:
- Understand what went wrong
- Locate the problem in source code
- Fix the issue with suggested solutions
- Learn best practices through helpful hints

### Design Philosophy

**TypeScript-inspired diagnostics:**
```
error TS2322: Type 'string | number' is not assignable to type 'string'.
  Type 'number' is not assignable to type 'string'.

  src/main.raya:10:5

   8 | let value: string | number = 42;
   9 | function fn(x: string): void { }
> 10 | fn(value);
     |    ^^^^^
  11 |

  💡 Hint: Use 'typeof' to narrow the union type:
     if (typeof value === "string") { fn(value); }
```

**Key Features:**
- Clear error messages with context
- Source code snippets with highlighting
- Helpful suggestions and hints
- Support for multiple output formats (human-readable, JSON, LSP)
- Color-coded output (errors in red, warnings in yellow, hints in blue)

---

## Goals

### Primary Goals

1. **Readable Error Messages**
   - Clear, concise error descriptions
   - Avoid compiler jargon when possible
   - Explain what went wrong and why

2. **Source Code Context**
   - Show relevant lines of code
   - Highlight error location with underlines/carets
   - Display line numbers for easy navigation

3. **Actionable Suggestions**
   - Suggest fixes for common errors
   - Provide code examples where helpful
   - Link to documentation for complex issues

4. **Multiple Output Formats**
   - Human-readable (default CLI output)
   - JSON (for IDE integration)
   - LSP (Language Server Protocol) format

5. **Error Categories**
   - Parse errors (syntax issues)
   - Bind errors (undefined types, duplicate symbols)
   - Type errors (type mismatches, invalid operations)
   - Warnings (unused variables, deprecated features)

### Secondary Goals

- Color-coded output (optional, controlled by flag)
- Error recovery suggestions (what to try next)
- Link to relevant documentation sections
- Aggregate multiple related errors
- Show error statistics (X errors, Y warnings)

---

## Non-Goals

- **Not implementing error recovery** - Parser/checker already handle this
- **Not adding new error types** - Focus on presentation, not detection
- **Not building an IDE** - Just provide good diagnostic data
- **Not internationalization** - English-only for now

---

## Architecture

### Component Overview

```
Error Detection (Parser/Checker)
    ↓
Error Storage (Error types)
    ↓
Error Formatter
    ├── Human-Readable Formatter
    ├── JSON Formatter
    └── LSP Formatter
    ↓
Output (Terminal/File/IDE)
```

### Error Data Flow

```rust
// 1. Error is created during checking
CheckError::TypeMismatch {
    expected: "string",
    actual: "string | number",
    span: Span { start: 145, end: 150, ... },
    note: Some("Use typeof to narrow the type"),
}

// 2. Error is formatted with source context
let diagnostic = Diagnostic::from_error(error, source_code);

// 3. Diagnostic is rendered to output
match output_format {
    Format::Human => diagnostic.render_human(&mut stdout),
    Format::Json => diagnostic.render_json(&mut stdout),
    Format::Lsp => diagnostic.render_lsp(&mut stdout),
}
```

### Key Types

```rust
// Error severity levels
pub enum Severity {
    Error,    // Compilation fails
    Warning,  // Compilation succeeds, but code may be problematic
    Hint,     // Optional suggestion
}

// Diagnostic with context
pub struct Diagnostic {
    severity: Severity,
    message: String,
    labels: Vec<Label>,     // Source code annotations
    notes: Vec<String>,     // Additional context
    suggestions: Vec<Suggestion>,
}

// Source code annotation
pub struct Label {
    span: Span,
    message: Option<String>,
    style: LabelStyle,  // Primary, Secondary
}

// Fix suggestion
pub struct Suggestion {
    message: String,
    replacements: Vec<Replacement>,  // Code edits
}
```

---

## Phase 1: Error Formatting & Display (Week 1)

**Goal:** Implement basic error formatting with source code context.

### Task 1.1: Diagnostic Infrastructure

**New file:** `crates/raya-checker/src/diagnostic.rs`

Create core diagnostic types:
```rust
pub struct Diagnostic {
    pub severity: Severity,
    pub code: Option<String>,  // E.g., "E0001"
    pub message: String,
    pub labels: Vec<Label>,
    pub notes: Vec<String>,
    pub help: Option<String>,
}

impl Diagnostic {
    /// Create diagnostic from CheckError
    pub fn from_check_error(error: &CheckError, source: &SourceFile) -> Self;

    /// Create diagnostic from BindError
    pub fn from_bind_error(error: &BindError, source: &SourceFile) -> Self;

    /// Create diagnostic from ParseError
    pub fn from_parse_error(error: &ParseError, source: &SourceFile) -> Self;
}
```

### Task 1.2: Human-Readable Formatter

**New file:** `crates/raya-checker/src/diagnostic/human.rs`

Implement pretty-printing for terminal output:
```rust
pub struct HumanFormatter {
    use_color: bool,
    show_line_numbers: bool,
}

impl HumanFormatter {
    pub fn format(&self, diagnostic: &Diagnostic, source: &SourceFile) -> String {
        // Format: error[E0001]: Type mismatch
        //   --> src/main.raya:10:5
        //    |
        // 10 | fn(value);
        //    |    ^^^^^ expected string, found string | number
        //    |
        //    = help: Use typeof to narrow the type
    }
}
```

**Key Features:**
- Line numbers with padding
- Source code snippets (3 lines context)
- Caret/underline highlighting
- Color coding (red for errors, yellow for warnings)
- Multi-span support (show multiple locations)

### Task 1.3: Source File Management

**New file:** `crates/raya-checker/src/source.rs`

Manage source code for error reporting:
```rust
pub struct SourceFile {
    path: PathBuf,
    content: String,
    lines: Vec<LineInfo>,  // Precomputed line boundaries
}

impl SourceFile {
    pub fn new(path: PathBuf, content: String) -> Self;

    /// Get line and column from byte offset
    pub fn position(&self, offset: usize) -> (usize, usize);

    /// Get text snippet for span
    pub fn snippet(&self, span: Span) -> &str;

    /// Get line at index
    pub fn line(&self, line: usize) -> &str;
}
```

### Task 1.4: Error Code Mapping

Create error code registry:
```rust
// Map error types to error codes
pub fn error_code(error: &CheckError) -> &str {
    match error {
        CheckError::TypeMismatch { .. } => "E2001",
        CheckError::UndefinedVariable { .. } => "E2002",
        CheckError::ForbiddenFieldAccess { .. } => "E2003",
        // ... etc
    }
}
```

### Verification (Phase 1)

**Tests:** `crates/raya-checker/tests/diagnostic_test.rs`

```rust
#[test]
fn test_format_type_mismatch_error() {
    let source = r#"
        let x: string = 42;
    "#;

    // Trigger error and format it
    let diagnostic = // ... create diagnostic
    let formatted = HumanFormatter::new().format(&diagnostic, &source_file);

    assert!(formatted.contains("Type mismatch"));
    assert!(formatted.contains("expected string"));
    assert!(formatted.contains("found number"));
}

#[test]
fn test_multiline_error_context() {
    // Test error spanning multiple lines
}

#[test]
fn test_color_output() {
    // Test ANSI color codes
}
```

**Success Criteria:**
- ✅ Basic diagnostic formatting working
- ✅ Source code snippets displayed
- ✅ Line numbers and highlighting
- ✅ 3+ tests passing

---

## Phase 2: Advanced Diagnostics (Week 2)

**Goal:** Add suggestions, multiple formats, and enhanced error messages.

### Task 2.1: Fix Suggestions

Add automatic fix suggestions:
```rust
pub struct Suggestion {
    pub message: String,
    pub replacements: Vec<Replacement>,
}

pub struct Replacement {
    pub span: Span,
    pub text: String,
}

impl Diagnostic {
    /// Add suggestion for typeof narrowing
    pub fn suggest_typeof_narrowing(&mut self, var: &str, ty: &str);

    /// Add suggestion for discriminated union
    pub fn suggest_discriminated_union(&mut self, types: &[String]);
}
```

**Common suggestions:**
- `ForbiddenFieldAccess` → "Use typeof instead of accessing $type"
- `TypeMismatch` with union → "Narrow the type with typeof or discriminant check"
- `UndefinedVariable` → "Did you mean: <similar_name>?"
- `NonExhaustiveMatch` → "Add missing cases: <variants>"

### Task 2.2: JSON Output Format

**New file:** `crates/raya-checker/src/diagnostic/json.rs`

```rust
#[derive(Serialize)]
pub struct JsonDiagnostic {
    severity: String,
    code: Option<String>,
    message: String,
    spans: Vec<JsonSpan>,
    notes: Vec<String>,
    suggestions: Vec<JsonSuggestion>,
}

impl JsonFormatter {
    pub fn format(&self, diagnostics: &[Diagnostic]) -> String {
        // Output JSON array of diagnostics
        serde_json::to_string_pretty(&json_diagnostics)
    }
}
```

### Task 2.3: Error Aggregation

Group related errors:
```rust
pub struct DiagnosticBatch {
    diagnostics: Vec<Diagnostic>,
}

impl DiagnosticBatch {
    /// Group errors by file
    pub fn by_file(&self) -> HashMap<PathBuf, Vec<Diagnostic>>;

    /// Count errors vs warnings
    pub fn summary(&self) -> (usize, usize);

    /// Sort by severity and location
    pub fn sorted(&self) -> Vec<&Diagnostic>;
}
```

### Task 2.4: Enhanced Error Messages

Improve specific error messages:

**Type Mismatch:**
```
error[E2001]: Type 'string | number' is not assignable to type 'string'
  --> src/main.raya:10:5
   |
10 | fn(value);
   |    ^^^^^ expected 'string', found 'string | number'
   |
   = note: Type 'number' is not assignable to type 'string'
   = help: Use typeof to narrow the union type:
           if (typeof value === "string") { fn(value); }
```

**Forbidden Field Access:**
```
error[E2003]: Cannot access internal field '$type' on bare union
  --> src/main.raya:15:9
   |
15 | let t = value.$type;
   |         ^^^^^^^^^^^
   |
   = note: Bare unions use typeof for type narrowing
   = help: Use typeof instead:
           typeof value === "string"
```

**Undefined Variable:**
```
error[E2002]: Cannot find name 'vlaue'
  --> src/main.raya:8:5
   |
 8 | logger.info(vlaue);
   |             ^^^^^ not found in this scope
   |
   = help: Did you mean 'value'?
```

### Verification (Phase 2)

**Tests:** `crates/raya-checker/tests/diagnostic_advanced_test.rs`

```rust
#[test]
fn test_suggestion_typeof_narrowing();

#[test]
fn test_json_output_format();

#[test]
fn test_error_aggregation();

#[test]
fn test_did_you_mean_suggestion();
```

**Success Criteria:**
- ✅ Fix suggestions working
- ✅ JSON output format implemented
- ✅ Error aggregation and sorting
- ✅ Enhanced error messages
- ✅ 4+ tests passing

---

## Testing Strategy

### Unit Tests

Test individual formatters:
```rust
#[test]
fn test_human_formatter_basic();
#[test]
fn test_human_formatter_multiline();
#[test]
fn test_json_formatter();
#[test]
fn test_label_formatting();
```

### Integration Tests

Test end-to-end error reporting:
```rust
#[test]
fn test_type_error_full_diagnostic() {
    let source = r#"
        let x: string | number = 42;
        function fn(s: string): void {}
        fn(x);
    "#;

    // Parse, check, and collect errors
    let errors = check_source(source);

    // Format errors
    let diagnostics: Vec<_> = errors.iter()
        .map(|e| Diagnostic::from_check_error(e, &source_file))
        .collect();

    // Verify formatting
    let formatted = HumanFormatter::new().format(&diagnostics[0], &source_file);
    assert!(formatted.contains("Type mismatch"));
    assert!(formatted.contains("Use typeof"));
}
```

### Visual Tests

Create snapshot tests for error output:
```rust
#[test]
fn test_error_snapshots() {
    // Compare formatted output against saved snapshots
    // Useful for catching formatting regressions
}
```

---

## Success Criteria

**Phase 1:**
- ✅ Basic diagnostic formatting with source context
- ✅ Human-readable output with line numbers
- ✅ Color-coded terminal output
- ✅ 3+ unit tests passing

**Phase 2:**
- ✅ Automatic fix suggestions
- ✅ JSON output format
- ✅ Error aggregation and summary
- ✅ Enhanced messages for all error types
- ✅ 4+ advanced tests passing

**Overall:**
- ✅ All parser errors have good diagnostics
- ✅ All bind errors have good diagnostics
- ✅ All type check errors have good diagnostics
- ✅ Support for human-readable and JSON formats
- ✅ Helpful suggestions for common mistakes
- ✅ 10+ total tests passing
- ✅ No regressions in existing tests

---

## Implementation Notes

### Libraries to Consider

- **codespan-reporting** - Mature diagnostic library (https://crates.io/crates/codespan-reporting)
  - Pros: Battle-tested, feature-rich, used by many compilers
  - Cons: May be overkill, requires adaptation

- **annotate-snippets** - Lightweight snippet formatter (https://crates.io/crates/annotate-snippets)
  - Pros: Simple, focused on source snippets
  - Cons: Less flexible than codespan

- **Custom implementation** - Build from scratch
  - Pros: Full control, tailored to Raya
  - Cons: More work, potential bugs

**Recommendation:** Start with **codespan-reporting** for Phase 1, can switch to custom if needed.

### Color Output

Use `termcolor` crate for cross-platform color support:
```rust
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

let mut stdout = StandardStream::stdout(ColorChoice::Auto);
stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red)))?;
write!(&mut stdout, "error")?;
stdout.reset()?;
```

### Error Codes

Assign unique codes to each error type:
- **E1xxx** - Parse errors
- **E2xxx** - Type errors
- **E3xxx** - Bind errors
- **E4xxx** - Warnings

This allows documentation to reference specific error codes.

---

## Dependencies

### Required Crates

```toml
[dependencies]
codespan-reporting = "0.11"  # Diagnostic formatting
termcolor = "1.4"             # Color output
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

### File Structure

```
crates/raya-checker/src/
├── diagnostic.rs              # Core diagnostic types
├── diagnostic/
│   ├── human.rs              # Human-readable formatter
│   ├── json.rs               # JSON formatter
│   └── suggestions.rs        # Fix suggestions
├── source.rs                 # Source file management
└── error_codes.rs            # Error code registry

crates/raya-checker/tests/
├── diagnostic_test.rs        # Basic formatting tests
└── diagnostic_advanced_test.rs  # Advanced feature tests
```

---

## Future Enhancements (Post-Milestone)

- **IDE Integration:** LSP-compatible diagnostic protocol
- **Internationalization:** Error messages in multiple languages
- **Error Recovery:** Show partial type information even with errors
- **Interactive Fixes:** Apply suggestions automatically
- **Documentation Links:** Link error codes to online docs
- **Error Statistics:** Track common errors for documentation improvements

---

## References

- TypeScript Compiler: https://github.com/microsoft/TypeScript/tree/main/src/compiler
- Rust Compiler Errors: https://doc.rust-lang.org/error-index.html
- Elm Compiler Messages: https://elm-lang.org/news/compiler-errors-for-humans
- codespan-reporting: https://docs.rs/codespan-reporting/
