//! Rule: no-duplicate-case (L1004)
//!
//! Flags switch statements with duplicate case values (e.g. two `case 1:`).
//! Only checks literal values (int, string, boolean).

use crate::linter::rule::*;
use crate::parser::ast;

pub struct NoDuplicateCase;

static META: RuleMeta = RuleMeta {
    name: "no-duplicate-case",
    code: "L1004",
    description: "Disallow duplicate case values in switch statements",
    category: Category::Correctness,
    default_severity: Severity::Error,
    fixable: false,
};

impl LintRule for NoDuplicateCase {
    fn meta(&self) -> &RuleMeta {
        &META
    }

    fn check_statement(
        &self,
        stmt: &ast::Statement,
        ctx: &LintContext<'_>,
    ) -> Vec<LintDiagnostic> {
        let switch = match stmt {
            ast::Statement::Switch(s) => s,
            _ => return vec![],
        };

        let mut diagnostics = Vec::new();
        let mut seen: Vec<(CaseKey, u32)> = Vec::new(); // (key, line)

        for case in &switch.cases {
            let test = match &case.test {
                Some(t) => t,
                None => continue, // default case
            };

            let key = match case_key(test, ctx) {
                Some(k) => k,
                None => continue, // non-literal, can't compare
            };

            if let Some((_, first_line)) = seen.iter().find(|(k, _)| k == &key) {
                diagnostics.push(LintDiagnostic {
                    rule: META.name,
                    code: META.code,
                    message: format!("Duplicate case value: {}", key.display()),
                    span: *test.span(),
                    severity: META.default_severity,
                    fix: None,
                    notes: vec![format!("First occurrence at line {}", first_line)],
                });
            } else {
                seen.push((key, case.span.line));
            }
        }

        diagnostics
    }
}

/// A comparable representation of a case literal.
#[derive(Debug, Clone, PartialEq)]
enum CaseKey {
    Int(i64),
    String(String),
    Bool(bool),
}

impl CaseKey {
    fn display(&self) -> String {
        match self {
            CaseKey::Int(v) => v.to_string(),
            CaseKey::String(v) => format!("\"{}\"", v),
            CaseKey::Bool(v) => v.to_string(),
        }
    }
}

fn case_key(expr: &ast::Expression, ctx: &LintContext<'_>) -> Option<CaseKey> {
    match expr {
        ast::Expression::IntLiteral(i) => Some(CaseKey::Int(i.value)),
        ast::Expression::StringLiteral(s) => {
            Some(CaseKey::String(ctx.interner.resolve(s.value).to_string()))
        }
        ast::Expression::BooleanLiteral(b) => Some(CaseKey::Bool(b.value)),
        ast::Expression::Parenthesized(p) => case_key(&p.expression, ctx),
        _ => None,
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
    fn test_duplicate_int_case() {
        let source = r#"
function f(x: int): void {
    switch (x) {
        case 1:
            break;
        case 1:
            break;
    }
}
"#;
        let diags = lint(source);
        assert!(has_rule(&diags, "L1004"), "should flag duplicate case 1, got: {:?}", diags);
    }

    #[test]
    fn test_unique_cases_ok() {
        let source = r#"
function f(x: int): void {
    switch (x) {
        case 1:
            break;
        case 2:
            break;
    }
}
"#;
        let diags = lint(source);
        assert!(!has_rule(&diags, "L1004"), "unique cases should be ok");
    }

    #[test]
    fn test_duplicate_string_case() {
        let source = r#"
function f(x: string): void {
    switch (x) {
        case "a":
            break;
        case "a":
            break;
    }
}
"#;
        let diags = lint(source);
        assert!(has_rule(&diags, "L1004"), "should flag duplicate string case");
    }
}
