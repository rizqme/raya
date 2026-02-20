//! Rule: no-throw-literal (L3001)
//!
//! Flags `throw "message"` or `throw 42` — you should throw an Error object
//! instead: `throw new Error("message")`.

use crate::linter::rule::*;
use crate::parser::ast;

pub struct NoThrowLiteral;

static META: RuleMeta = RuleMeta {
    name: "no-throw-literal",
    code: "L3001",
    description: "Disallow throwing literals (throw strings, numbers, etc.)",
    category: Category::BestPractice,
    default_severity: Severity::Warn,
    fixable: false,
};

impl LintRule for NoThrowLiteral {
    fn meta(&self) -> &RuleMeta {
        &META
    }

    fn check_statement(
        &self,
        stmt: &ast::Statement,
        _ctx: &LintContext,
    ) -> Vec<LintDiagnostic> {
        let throw_stmt = match stmt {
            ast::Statement::Throw(t) => t,
            _ => return vec![],
        };

        if is_literal(&throw_stmt.value) {
            vec![LintDiagnostic {
                rule: META.name,
                code: META.code,
                message: "Do not throw literals; use `throw new Error(...)` instead".to_string(),
                span: throw_stmt.span.clone(),
                severity: META.default_severity,
                fix: None,
                notes: vec![],
            }]
        } else {
            vec![]
        }
    }
}

fn is_literal(expr: &ast::Expression) -> bool {
    matches!(
        expr,
        ast::Expression::StringLiteral(_)
            | ast::Expression::IntLiteral(_)
            | ast::Expression::FloatLiteral(_)
            | ast::Expression::BooleanLiteral(_)
            | ast::Expression::NullLiteral(_)
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
    fn test_throw_string_flagged() {
        let diags = lint(r#"function f(): void { throw "error"; }"#);
        assert!(has_rule(&diags, "L3001"), "should flag throw string, got: {:?}", diags);
    }

    #[test]
    fn test_throw_new_error_ok() {
        let diags = lint(r#"function f(): void { throw new Error("msg"); }"#);
        assert!(!has_rule(&diags, "L3001"), "throw new Error should be ok");
    }

    #[test]
    fn test_throw_int_flagged() {
        let diags = lint("function f(): void { throw 42; }");
        assert!(has_rule(&diags, "L3001"), "should flag throw int");
    }

    #[test]
    fn test_throw_variable_ok() {
        let diags = lint("function f(): void { const e: Error = new Error(\"x\"); throw e; }");
        assert!(!has_rule(&diags, "L3001"), "throw variable should be ok");
    }
}
