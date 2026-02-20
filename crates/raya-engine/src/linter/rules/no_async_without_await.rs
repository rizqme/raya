//! Rule: no-async-without-await (L3002)
//!
//! Flags `async` functions that never use `await`. The `async` keyword adds
//! overhead (Task wrapping) for no benefit if await is never used.

use crate::linter::rule::*;
use crate::parser::ast::{self, visitor::{Visitor, walk_block_statement, walk_expression}};

pub struct NoAsyncWithoutAwait;

static META: RuleMeta = RuleMeta {
    name: "no-async-without-await",
    code: "L3002",
    description: "Disallow async functions that don't use await",
    category: Category::BestPractice,
    default_severity: Severity::Warn,
    fixable: false,
};

impl LintRule for NoAsyncWithoutAwait {
    fn meta(&self) -> &RuleMeta {
        &META
    }

    fn check_statement(
        &self,
        stmt: &ast::Statement,
        ctx: &LintContext,
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
                span: func.span.clone(),
                severity: META.default_severity,
                fix: None,
                notes: vec!["Remove the 'async' keyword if await is not needed".to_string()],
            }]
        } else {
            vec![]
        }
    }

    fn check_expression(
        &self,
        expr: &ast::Expression,
        _ctx: &LintContext,
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
                span: arrow.span.clone(),
                severity: META.default_severity,
                fix: None,
                notes: vec!["Remove the 'async' keyword if await is not needed".to_string()],
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
            ast::Expression::Arrow(_) => return,
            _ => walk_expression(self, expr),
        }
    }

    fn visit_function_decl(&mut self, _decl: &ast::FunctionDecl) {
        // Don't descend into nested functions.
    }
}

#[cfg(test)]
mod tests {
    use crate::linter::rule::LintDiagnostic;
    use crate::linter::Linter;

    fn lint(source: &str) -> Vec<LintDiagnostic> {
        let linter = Linter::new();
        linter.lint_source(source, "test.raya").diagnostics
    }

    fn has_rule(diags: &[LintDiagnostic], code: &str) -> bool {
        diags.iter().any(|d| d.code == code)
    }

    #[test]
    fn test_async_without_await_flagged() {
        let diags = lint("async function f(): void { const x: int = 1; }");
        assert!(has_rule(&diags, "L3002"), "should flag async without await, got: {:?}", diags);
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
