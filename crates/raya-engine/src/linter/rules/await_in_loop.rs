//! Rule: await-in-loop (L1006)
//!
//! Flags `await` expressions inside `for`, `while`, `do-while`, and `for-of`
//! loops. Sequential awaits in a loop are usually a performance mistake —
//! consider collecting promises and awaiting them all at once.

use crate::linter::rule::*;
use crate::parser::ast::{self, visitor::{Visitor, walk_expression}};

pub struct AwaitInLoop;

static META: RuleMeta = RuleMeta {
    name: "await-in-loop",
    code: "L1006",
    description: "Disallow await inside loops (sequential execution)",
    category: Category::Correctness,
    default_severity: Severity::Warn,
    fixable: false,
};

impl LintRule for AwaitInLoop {
    fn meta(&self) -> &RuleMeta {
        &META
    }

    fn check_statement(
        &self,
        stmt: &ast::Statement,
        _ctx: &LintContext,
    ) -> Vec<LintDiagnostic> {
        // Check if this statement is a loop; if so, scan its body for awaits.
        let body: &ast::Statement = match stmt {
            ast::Statement::While(w) => &w.body,
            ast::Statement::DoWhile(d) => &d.body,
            ast::Statement::For(f) => &f.body,
            ast::Statement::ForOf(f) => &f.body,
            _ => return vec![],
        };

        let mut finder = AwaitInBodyFinder {
            diagnostics: Vec::new(),
        };
        finder.visit_statement(body);
        finder.diagnostics
    }
}

/// Walks a loop body looking for `await` expressions, but does NOT
/// descend into nested functions/arrows (they have their own scope).
struct AwaitInBodyFinder {
    diagnostics: Vec<LintDiagnostic>,
}

impl Visitor for AwaitInBodyFinder {
    fn visit_expression(&mut self, expr: &ast::Expression) {
        if let ast::Expression::Await(await_expr) = expr {
            self.diagnostics.push(LintDiagnostic {
                rule: META.name,
                code: META.code,
                message: "'await' inside a loop runs sequentially; consider batching".to_string(),
                span: await_expr.span.clone(),
                severity: META.default_severity,
                fix: None,
                notes: vec![],
            });
        }

        // Don't descend into nested functions/arrows.
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
    fn test_await_in_for_flagged() {
        let source = r#"
async function f(): void {
    for (let i: int = 0; i < 10; i += 1) {
        await async fetch();
    }
}
"#;
        let diags = lint(source);
        assert!(has_rule(&diags, "L1006"), "should flag await in for loop, got: {:?}", diags);
    }

    #[test]
    fn test_await_outside_loop_ok() {
        let source = "async function f(): int { return await async getNum(); }";
        let diags = lint(source);
        assert!(!has_rule(&diags, "L1006"), "await outside loop should be ok");
    }
}
