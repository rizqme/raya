//! Rule: no-floating-task (L1007)
//!
//! Flags `async foo()` used as an expression statement where the Task result
//! is silently discarded. You can't await, cancel, or check a discarded Task.
//!
//! **Excluded:** `async { ... }` blocks (desugared to `async (() => { ... })()`)
//! are intentional fire-and-forget patterns and are not flagged.

use crate::linter::rule::*;
use crate::parser::ast;

pub struct NoFloatingTask;

static META: RuleMeta = RuleMeta {
    name: "no-floating-task",
    code: "L1007",
    description: "Disallow discarding Task results from async calls",
    category: Category::Correctness,
    default_severity: Severity::Warn,
    fixable: false,
};

impl LintRule for NoFloatingTask {
    fn meta(&self) -> &RuleMeta {
        &META
    }

    fn check_statement(
        &self,
        stmt: &ast::Statement,
        _ctx: &LintContext<'_>,
    ) -> Vec<LintDiagnostic> {
        let expr_stmt = match stmt {
            ast::Statement::Expression(e) => e,
            _ => return vec![],
        };

        let async_call = match &expr_stmt.expression {
            ast::Expression::AsyncCall(ac) => ac,
            _ => return vec![],
        };

        // `async { ... }` desugars to `async (() => { ... })()` — skip these.
        // The callee is an arrow function (possibly parenthesized).
        if is_fire_and_forget_block(&async_call.callee) {
            return vec![];
        }

        vec![LintDiagnostic {
            rule: META.name,
            code: META.code,
            message: "Task result from 'async' call is discarded".to_string(),
            span: async_call.span,
            severity: META.default_severity,
            fix: None,
            notes: vec![
                "The spawned Task cannot be awaited, cancelled, or checked".to_string(),
                "Store in a variable: let task = async foo()".to_string(),
                "Or await immediately: await async foo()".to_string(),
                "Or use async { ... } for intentional fire-and-forget".to_string(),
            ],
        }]
    }
}

/// Returns true if the callee is an arrow function (the `async { ... }` block pattern).
fn is_fire_and_forget_block(callee: &ast::Expression) -> bool {
    match callee {
        ast::Expression::Arrow(_) => true,
        ast::Expression::Parenthesized(p) => is_fire_and_forget_block(&p.expression),
        _ => false,
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
    fn test_discarded_async_call_flagged() {
        let diags = lint("function foo(): void {}\nasync foo();");
        assert!(has_rule(&diags, "L1007"), "should flag discarded async call, got: {:?}", diags);
    }

    #[test]
    fn test_assigned_async_call_ok() {
        let diags = lint("function foo(): void {}\nlet task: Task<void> = async foo();");
        assert!(!has_rule(&diags, "L1007"), "assigned async call should be ok");
    }

    #[test]
    fn test_awaited_async_call_ok() {
        let diags = lint("function foo(): int { return 1; }\nlet x: int = await async foo();");
        assert!(!has_rule(&diags, "L1007"), "awaited async call should be ok");
    }

    #[test]
    fn test_async_block_ok() {
        // async { ... } is a fire-and-forget pattern — should not be flagged.
        let diags = lint("async { const x: int = 1; }");
        assert!(!has_rule(&diags, "L1007"), "async block should not be flagged, got: {:?}", diags);
    }

    #[test]
    fn test_async_call_in_variable_ok() {
        let diags = lint("function work(): void {}\nconst t: Task<void> = async work();");
        assert!(!has_rule(&diags, "L1007"));
    }
}
