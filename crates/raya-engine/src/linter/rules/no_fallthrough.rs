//! Rule: no-fallthrough (L1005)
//!
//! Flags non-empty switch cases that don't end with `break`, `return`,
//! `throw`, or `continue`. Fallthrough is often a mistake.
//! Empty cases (used for grouping) are intentionally allowed.

use crate::linter::rule::*;
use crate::parser::ast;

pub struct NoFallthrough;

static META: RuleMeta = RuleMeta {
    name: "no-fallthrough",
    code: "L1005",
    description: "Disallow switch case fallthrough without break",
    category: Category::Correctness,
    default_severity: Severity::Warn,
    fixable: false,
};

impl LintRule for NoFallthrough {
    fn meta(&self) -> &RuleMeta {
        &META
    }

    fn check_statement(
        &self,
        stmt: &ast::Statement,
        _ctx: &LintContext<'_>,
    ) -> Vec<LintDiagnostic> {
        let switch = match stmt {
            ast::Statement::Switch(s) => s,
            _ => return vec![],
        };

        let mut diagnostics = Vec::new();
        let case_count = switch.cases.len();

        for (i, case) in switch.cases.iter().enumerate() {
            // Skip the last case (no fallthrough possible) and empty cases
            // (empty cases are used for grouping: `case A: case B: ...`).
            if i == case_count - 1 || case.consequent.is_empty() {
                continue;
            }

            // Check if the last statement is a terminator.
            if let Some(last) = case.consequent.last() {
                if !is_terminator(last) {
                    diagnostics.push(LintDiagnostic {
                        rule: META.name,
                        code: META.code,
                        message: "Switch case falls through to the next case".to_string(),
                        span: case.span,
                        severity: META.default_severity,
                        fix: None,
                        notes: vec!["Add 'break', 'return', or 'throw' to prevent fallthrough".to_string()],
                    });
                }
            }
        }

        diagnostics
    }
}

fn is_terminator(stmt: &ast::Statement) -> bool {
    matches!(
        stmt,
        ast::Statement::Break(_)
            | ast::Statement::Return(_)
            | ast::Statement::Throw(_)
            | ast::Statement::Continue(_)
    )
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
    fn test_fallthrough_flagged() {
        let source = r#"
function f(x: int): void {
    switch (x) {
        case 1:
            const a: int = 1;
        case 2:
            break;
    }
}
"#;
        let diags = lint(source);
        assert!(has_rule(&diags, "L1005"), "should flag fallthrough, got: {:?}", diags);
    }

    #[test]
    fn test_with_break_ok() {
        let source = r#"
function f(x: int): void {
    switch (x) {
        case 1:
            const a: int = 1;
            break;
        case 2:
            break;
    }
}
"#;
        let diags = lint(source);
        assert!(!has_rule(&diags, "L1005"), "cases with break should be ok");
    }

    #[test]
    fn test_empty_case_grouping_ok() {
        let source = r#"
function f(x: int): void {
    switch (x) {
        case 1:
        case 2:
            break;
    }
}
"#;
        let diags = lint(source);
        assert!(!has_rule(&diags, "L1005"), "empty case grouping should be ok");
    }
}
