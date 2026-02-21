//! Lint rule trait and supporting types.
//!
//! Each lint rule implements `LintRule` and provides metadata (`RuleMeta`),
//! and one or more `check_*` methods that inspect AST nodes.

use crate::parser::ast;
use crate::parser::interner::Interner;
use crate::parser::token::Span;

/// Severity level for a lint diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Rule is disabled.
    Off,
    /// Reports as a warning (does not affect exit code).
    Warn,
    /// Reports as an error (causes non-zero exit code).
    Error,
}

/// Category of a lint rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// Likely bugs or logic errors.
    Correctness,
    /// Naming and formatting conventions.
    Style,
    /// Language idioms and recommended patterns.
    BestPractice,
}

/// Static metadata for a lint rule.
pub struct RuleMeta {
    /// Rule name, e.g. "no-empty-block".
    pub name: &'static str,
    /// Lint code, e.g. "L2002".
    pub code: &'static str,
    /// Human-readable description.
    pub description: &'static str,
    /// Category.
    pub category: Category,
    /// Default severity when no config override is set.
    pub default_severity: Severity,
    /// Whether the rule can provide auto-fixes.
    pub fixable: bool,
}

/// Context passed to each rule during lint checking.
pub struct LintContext<'a> {
    /// The original source code.
    pub source: &'a str,
    /// String interner (resolve Symbol → &str).
    pub interner: &'a Interner,
    /// Path of the file being linted.
    pub file_path: &'a str,
}

/// A suggested auto-fix: replace a span with new text.
#[derive(Debug, Clone)]
pub struct LintFix {
    /// The span to replace.
    pub span: Span,
    /// Replacement text.
    pub replacement: String,
}

/// A single lint diagnostic emitted by a rule.
#[derive(Debug, Clone)]
pub struct LintDiagnostic {
    /// Rule name (e.g. "no-empty-block").
    pub rule: &'static str,
    /// Lint code (e.g. "L2002").
    pub code: &'static str,
    /// Human-readable message.
    pub message: String,
    /// Source location.
    pub span: Span,
    /// Severity level.
    pub severity: Severity,
    /// Optional auto-fix.
    pub fix: Option<LintFix>,
    /// Additional notes.
    pub notes: Vec<String>,
}

/// Trait that every lint rule must implement.
///
/// Rules receive individual AST nodes and return diagnostics.
/// Default implementations return no diagnostics, so rules only
/// need to override the methods relevant to them.
pub trait LintRule: Send + Sync {
    /// Static metadata for this rule.
    fn meta(&self) -> &RuleMeta;

    /// Check a top-level module.
    fn check_module(&self, _module: &ast::Module, _ctx: &LintContext<'_>) -> Vec<LintDiagnostic> {
        vec![]
    }

    /// Check a statement node.
    fn check_statement(&self, _stmt: &ast::Statement, _ctx: &LintContext<'_>) -> Vec<LintDiagnostic> {
        vec![]
    }

    /// Check an expression node.
    fn check_expression(
        &self,
        _expr: &ast::Expression,
        _ctx: &LintContext<'_>,
    ) -> Vec<LintDiagnostic> {
        vec![]
    }

    /// Check a class member (field, method, constructor).
    fn check_class_member(
        &self,
        _member: &ast::ClassMember,
        _ctx: &LintContext<'_>,
    ) -> Vec<LintDiagnostic> {
        vec![]
    }
}
