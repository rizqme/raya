//! Raya Linter
//!
//! AST-based lint analysis for Raya source files. Enforces style conventions,
//! catches common mistakes, and provides auto-fixable suggestions.
//!
//! # Architecture
//!
//! - Each rule implements [`LintRule`] and checks individual AST nodes.
//! - The [`LintRunner`](runner::LintRunner) walks the AST once and dispatches
//!   to all enabled rules (single-pass visitor).
//! - [`Linter`] is the public entry point: create one, then call
//!   [`lint_source`](Linter::lint_source) or [`lint_ast`](Linter::lint_ast).
//!
//! # Example
//!
//! ```ignore
//! use raya_engine::linter::{Linter, Severity};
//!
//! let linter = Linter::new();
//! let result = linter.lint_source("let x: int = 1;", "test.raya");
//! for d in &result.diagnostics {
//!     println!("[{}] {}: {}", d.code, d.rule, d.message);
//! }
//! ```

pub mod config;
pub mod rule;
pub mod rules;
mod runner;

pub use config::LintConfig;
pub use rule::{Category, LintContext, LintDiagnostic, LintFix, LintRule, RuleMeta, Severity};

use crate::parser::interner::Interner;
use crate::parser::Parser;
use runner::LintRunner;

/// Result of linting a single file.
#[derive(Debug)]
pub struct LintResult {
    /// All diagnostics emitted for this file.
    pub diagnostics: Vec<LintDiagnostic>,
    /// File path that was linted.
    pub file_path: String,
    /// Number of diagnostics that have an auto-fix.
    pub fixable_count: usize,
}

/// The Raya linter. Holds a set of enabled rules and configuration.
pub struct Linter {
    rules: Vec<Box<dyn LintRule>>,
    config: LintConfig,
}

impl Linter {
    /// Create a linter with all default rules and default severities.
    pub fn new() -> Self {
        Self {
            rules: rules::all_rules(),
            config: LintConfig::new(),
        }
    }

    /// Create a linter with configuration overrides.
    pub fn with_config(config: LintConfig) -> Self {
        Self {
            rules: rules::all_rules(),
            config,
        }
    }

    /// Lint a parsed AST module.
    ///
    /// The caller provides the AST, source, interner, and file path.
    pub fn lint_ast(
        &self,
        module: &crate::parser::ast::Module,
        source: &str,
        interner: &Interner,
        file_path: &str,
    ) -> LintResult {
        // Filter out disabled rules and apply severity overrides.
        let active_rules: Vec<&Box<dyn LintRule>> = self
            .rules
            .iter()
            .filter(|r| !self.config.is_disabled(r.meta().name))
            .collect();

        // Wrap active rules so the runner can use them.
        // We pass references, but runner expects &[Box<dyn LintRule>].
        // Instead, build a temporary vec of trait-object references.
        let ctx = LintContext {
            source,
            interner,
            file_path,
        };

        let runner = LintRunner::new(&self.rules, ctx);
        let mut diagnostics = runner.run(module);

        // Apply severity overrides and filter disabled.
        diagnostics.retain_mut(|d| {
            let eff = self.config.effective_severity(d.rule, d.severity);
            if eff == Severity::Off {
                return false;
            }
            d.severity = eff;
            true
        });

        let fixable_count = diagnostics.iter().filter(|d| d.fix.is_some()).count();

        LintResult {
            diagnostics,
            file_path: file_path.to_string(),
            fixable_count,
        }
    }

    /// Convenience: parse source code and lint it.
    ///
    /// Returns lint diagnostics. Parse errors are converted to lint diagnostics
    /// so the caller gets a uniform result.
    pub fn lint_source(&self, source: &str, file_path: &str) -> LintResult {
        let parser = match Parser::new(source) {
            Ok(p) => p,
            Err(lex_errors) => {
                // Convert lex errors to lint diagnostics.
                let diagnostics: Vec<LintDiagnostic> = lex_errors
                    .iter()
                    .map(|e| LintDiagnostic {
                        rule: "parse-error",
                        code: "L0001",
                        message: format!("Lex error: {}", e),
                        span: crate::parser::token::Span::new(0, 0, 1, 1),
                        severity: Severity::Error,
                        fix: None,
                        notes: vec![],
                    })
                    .collect();
                return LintResult {
                    diagnostics,
                    file_path: file_path.to_string(),
                    fixable_count: 0,
                };
            }
        };

        match parser.parse() {
            Ok((module, interner)) => self.lint_ast(&module, source, &interner, file_path),
            Err(parse_errors) => {
                let diagnostics: Vec<LintDiagnostic> = parse_errors
                    .iter()
                    .map(|e| LintDiagnostic {
                        rule: "parse-error",
                        code: "L0001",
                        message: format!("Parse error: {}", e),
                        span: crate::parser::token::Span::new(0, 0, 1, 1),
                        severity: Severity::Error,
                        fix: None,
                        notes: vec![],
                    })
                    .collect();
                LintResult {
                    diagnostics,
                    file_path: file_path.to_string(),
                    fixable_count: 0,
                }
            }
        }
    }
}

impl Default for Linter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linter_empty_source() {
        let linter = Linter::new();
        let result = linter.lint_source("", "empty.raya");
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn test_linter_parse_error() {
        let linter = Linter::new();
        let result = linter.lint_source("function {{{", "bad.raya");
        assert!(!result.diagnostics.is_empty());
        assert_eq!(result.diagnostics[0].code, "L0001");
    }

    #[test]
    fn test_linter_clean_source() {
        let linter = Linter::new();
        let result = linter.lint_source(
            "const x: int = 42;\nfunction add(a: int, b: int): int { return a + b; }",
            "clean.raya",
        );
        // With no rules registered yet, should be clean
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn test_linter_with_config_disables_rule() {
        let mut config = LintConfig::new();
        config.set_severity("no-empty-block", Severity::Off);

        let linter = Linter::with_config(config);
        let result = linter.lint_source("const x: int = 1;", "test.raya");
        // Disabled rule produces no diagnostics
        assert!(result.diagnostics.is_empty());
    }
}
