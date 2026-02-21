//! Rule: no-async-without-await (L3002)
//!
//! Flags `async` functions that never use `await`. In Raya, `async` means
//! "spawn a green thread," so an async function without await is still valid
//! for concurrency. This rule is off by default — enable it if you prefer
//! explicit separation of concurrent vs. suspending functions.

use crate::linter::rule::*;
use crate::parser::ast::{self, visitor::{Visitor, walk_expression}};

pub struct NoAsyncWithoutAwait;

static META: RuleMeta = RuleMeta {
    name: "no-async-without-await",
    code: "L3002",
    description: "Disallow async functions that don't use await (opt-in)",
    category: Category::BestPractice,
    default_severity: Severity::Off,
    fixable: false,
};

impl LintRule for NoAsyncWithoutAwait {
    fn meta(&self) -> &RuleMeta {
        &META
    }

    fn check_statement(
        &self,
        stmt: &ast::Statement,
        ctx: &LintContext<'_>,
    ) -> Vec<LintDiagnostic> {
        let func = match stmt {
            ast::Statement::FunctionDecl(f) => f,
            _ => return vec![],
        };

        if !func.is_async {
            return vec![];
        }

        let mut finder = AwaitFinder { found: false };
        finder.visit_block_statement(&func.body);

        if !finder.found {
            let name = ctx.interner.resolve(func.name.name);
            vec![LintDiagnostic {
                rule: META.name,
                code: META.code,
                message: format!("Async function '{}' does not use 'await'", name),
                span: func.span,
                severity: META.default_severity,
                fix: None,
                notes: vec!["In Raya, async functions run as concurrent tasks even without await. Remove 'async' if concurrency is not intended".to_string()],
            }]
        } else {
            vec![]
        }
    }

    fn check_expression(
        &self,
        expr: &ast::Expression,
        _ctx: &LintContext<'_>,
    ) -> Vec<LintDiagnostic> {
        // Also check async arrow functions
        let arrow = match expr {
            ast::Expression::Arrow(a) => a,
            _ => return vec![],
        };

        if !arrow.is_async {
            return vec![];
        }

        let mut finder = AwaitFinder { found: false };
        match &arrow.body {
            ast::ArrowBody::Block(block) => finder.visit_block_statement(block),
            ast::ArrowBody::Expression(expr) => finder.visit_expression(expr),
        }

        if !finder.found {
            vec![LintDiagnostic {
                rule: META.name,
                code: META.code,
                message: "Async arrow function does not use 'await'".to_string(),
                span: arrow.span,
                severity: META.default_severity,
                fix: None,
                notes: vec!["In Raya, async functions run as concurrent tasks even without await. Remove 'async' if concurrency is not intended".to_string()],
            }]
        } else {
            vec![]
        }
    }
}

/// Visitor that searches for any `await` expression.
struct AwaitFinder {
    found: bool,
}

impl Visitor for AwaitFinder {
    fn visit_expression(&mut self, expr: &ast::Expression) {
        if self.found {
            return;
        }
        if matches!(expr, ast::Expression::Await(_)) {
            self.found = true;
            return;
        }
        // Don't descend into nested function/arrow (they have their own async scope).
        match expr {
            ast::Expression::Arrow(_) => (),
            _ => walk_expression(self, expr),
        }
    }

    fn visit_function_decl(&mut self, _decl: &ast::FunctionDecl) {
        // Don't descend into nested functions.
    }
}

#[cfg(test)]
mod tests {
    use crate::linter::config::LintConfig;
    use crate::linter::rule::{LintDiagnostic, Severity};
    use crate::linter::Linter;

    fn lint(source: &str) -> Vec<LintDiagnostic> {
        // Rule is Off by default; explicitly enable for testing.
        let mut config = LintConfig::default();
        config.set_severity("no-async-without-await", Severity::Warn);
        let linter = Linter::with_config(config);
        linter.lint_source(source, "test.raya").diagnostics
    }

    fn has_rule(diags: &[LintDiagnostic], code: &str) -> bool {
        diags.iter().any(|d| d.code == code)
    }

    #[test]
    fn test_off_by_default() {
        let linter = Linter::new();
        let diags = linter.lint_source("async function f(): void { const x: int = 1; }", "test.raya").diagnostics;
        assert!(!has_rule(&diags, "L3002"), "should NOT fire when Off by default");
    }

    #[test]
    fn test_async_without_await_flagged() {
        let diags = lint("async function f(): void { const x: int = 1; }");
        assert!(has_rule(&diags, "L3002"), "should flag async without await when enabled, got: {:?}", diags);
    }

    #[test]
    fn test_async_with_await_ok() {
        let diags = lint("async function f(): int { return await async getNum(); }");
        assert!(!has_rule(&diags, "L3002"), "async with await should be ok");
    }

    #[test]
    fn test_non_async_ok() {
        let diags = lint("function f(): void { const x: int = 1; }");
        assert!(!has_rule(&diags, "L3002"), "non-async should not be flagged");
    }
}
