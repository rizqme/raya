//! Rule: no-constant-condition (L1003)
//!
//! Flags `if`, `while`, `do-while`, and ternary conditions that are always
//! the same value (literal `true`, `false`, integer, string, or `null`).

use crate::linter::rule::*;
use crate::parser::ast;

pub struct NoConstantCondition;

static META: RuleMeta = RuleMeta {
    name: "no-constant-condition",
    code: "L1003",
    description: "Disallow constant conditions in control flow",
    category: Category::Correctness,
    default_severity: Severity::Warn,
    fixable: false,
};

impl LintRule for NoConstantCondition {
    fn meta(&self) -> &RuleMeta {
        &META
    }

    fn check_statement(
        &self,
        stmt: &ast::Statement,
        _ctx: &LintContext<'_>,
    ) -> Vec<LintDiagnostic> {
        match stmt {
            ast::Statement::If(if_stmt) => check_condition(&if_stmt.condition, "if"),
            ast::Statement::While(while_stmt) => check_condition(&while_stmt.condition, "while"),
            ast::Statement::DoWhile(do_stmt) => check_condition(&do_stmt.condition, "do-while"),
            _ => vec![],
        }
    }

    fn check_expression(
        &self,
        expr: &ast::Expression,
        _ctx: &LintContext<'_>,
    ) -> Vec<LintDiagnostic> {
        // Ternary: `true ? a : b`
        if let ast::Expression::Conditional(cond) = expr {
            return check_condition(&cond.test, "ternary");
        }
        vec![]
    }
}

fn is_constant(expr: &ast::Expression) -> bool {
    matches!(
        expr,
        ast::Expression::BooleanLiteral(_)
            | ast::Expression::IntLiteral(_)
            | ast::Expression::FloatLiteral(_)
            | ast::Expression::StringLiteral(_)
            | ast::Expression::NullLiteral(_)
    )
}

fn check_condition(condition: &ast::Expression, context: &str) -> Vec<LintDiagnostic> {
    // Unwrap parenthesized expressions
    let inner = unwrap_parens(condition);
    if is_constant(inner) {
        vec![LintDiagnostic {
            rule: META.name,
            code: META.code,
            message: format!("Constant condition in '{}' statement", context),
            span: *condition.span(),
            severity: META.default_severity,
            fix: None,
            notes: vec![format!("This condition will always evaluate to the same value")],
        }]
    } else {
        vec![]
    }
}

fn unwrap_parens(expr: &ast::Expression) -> &ast::Expression {
    match expr {
        ast::Expression::Parenthesized(p) => unwrap_parens(&p.expression),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linter::Linter;

    fn lint(source: &str) -> Vec<LintDiagnostic> {
        let linter = Linter::new();
        linter.lint_source(source, "test.raya").diagnostics
    }

    fn has_rule(diags: &[LintDiagnostic], code: &str) -> bool {
        diags.iter().any(|d| d.code == code)
    }

    #[test]
    fn test_if_true() {
        let diags = lint("function f(): void { if (true) { const x: int = 1; } }");
        assert!(has_rule(&diags, "L1003"), "should flag if(true)");
    }

    #[test]
    fn test_while_false() {
        let diags = lint("function f(): void { while (false) { const x: int = 1; } }");
        assert!(has_rule(&diags, "L1003"), "should flag while(false)");
    }

    #[test]
    fn test_if_variable_ok() {
        let diags = lint("function f(x: bool): void { if (x) { const y: int = 1; } }");
        assert!(!has_rule(&diags, "L1003"), "should not flag variable condition");
    }

    #[test]
    fn test_if_literal_int() {
        let diags = lint("function f(): void { if (0) { const x: int = 1; } }");
        assert!(has_rule(&diags, "L1003"), "should flag if(0)");
    }
}
