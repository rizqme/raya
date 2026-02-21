//! Rule: no-invalid-typeof (L1011)
//!
//! Flags `typeof x == "..."` comparisons where the string is not a valid
//! Raya typeof result. Raya's `typeof` returns: `"int"`, `"float"`, `"string"`,
//! `"boolean"`, `"function"`, `"object"`, `"null"`.
//!
//! Common mistakes from JavaScript habits:
//! - `"number"` → use `"int"` or `"float"`
//! - `"undefined"` → use `"null"`

use crate::linter::rule::*;
use crate::parser::ast::{self, BinaryOperator};

pub struct NoInvalidTypeof;

static META: RuleMeta = RuleMeta {
    name: "no-invalid-typeof",
    code: "L1011",
    description: "Disallow invalid typeof comparison strings",
    category: Category::Correctness,
    default_severity: Severity::Error,
    fixable: true,
};

/// Valid strings that Raya's `typeof` operator can return.
const VALID_TYPEOF_RESULTS: &[&str] = &[
    "int", "float", "string", "boolean", "function", "object", "null",
];

impl LintRule for NoInvalidTypeof {
    fn meta(&self) -> &RuleMeta {
        &META
    }

    fn check_expression(
        &self,
        expr: &ast::Expression,
        ctx: &LintContext<'_>,
    ) -> Vec<LintDiagnostic> {
        let bin = match expr {
            ast::Expression::Binary(b) => b,
            _ => return vec![],
        };

        // Only check equality comparisons.
        if !matches!(
            bin.operator,
            BinaryOperator::Equal
                | BinaryOperator::NotEqual
                | BinaryOperator::StrictEqual
                | BinaryOperator::StrictNotEqual
        ) {
            return vec![];
        }

        // Find the (typeof, string) pair regardless of order.
        let string_lit = if is_typeof(&bin.left) {
            extract_string(&bin.right, ctx)
        } else if is_typeof(&bin.right) {
            extract_string(&bin.left, ctx)
        } else {
            return vec![];
        };

        let (value, str_span) = match string_lit {
            Some(v) => v,
            None => return vec![],
        };

        if VALID_TYPEOF_RESULTS.contains(&value.as_str()) {
            return vec![];
        }

        let (fix, suggestion) = suggest_replacement(&value);

        let mut notes = vec![
            format!(
                "Valid typeof results in Raya: {}",
                VALID_TYPEOF_RESULTS
                    .iter()
                    .map(|s| format!("\"{}\"", s))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ];
        if let Some(ref s) = suggestion {
            notes.push(s.clone());
        }

        vec![LintDiagnostic {
            rule: META.name,
            code: META.code,
            message: format!("Invalid typeof comparison value \"{}\"", value),
            span: str_span,
            severity: META.default_severity,
            fix: fix.map(|replacement| LintFix {
                span: str_span,
                replacement,
            }),
            notes,
        }]
    }
}

fn is_typeof(expr: &ast::Expression) -> bool {
    matches!(expr, ast::Expression::Typeof(_))
}

fn extract_string(expr: &ast::Expression, ctx: &LintContext<'_>) -> Option<(String, crate::parser::token::Span)> {
    if let ast::Expression::StringLiteral(s) = expr {
        Some((ctx.interner.resolve(s.value).to_string(), s.span))
    } else {
        None
    }
}

/// Returns (auto-fix replacement, human-readable suggestion) for common mistakes.
fn suggest_replacement(value: &str) -> (Option<String>, Option<String>) {
    match value {
        "number" => (
            Some("\"int\"".to_string()),
            Some("Did you mean \"int\" or \"float\"?".to_string()),
        ),
        "undefined" => (
            Some("\"null\"".to_string()),
            Some("Raya has no undefined — did you mean \"null\"?".to_string()),
        ),
        "bigint" => (
            None,
            Some("Raya has no bigint type".to_string()),
        ),
        "symbol" => (
            None,
            Some("Raya has no symbol type".to_string()),
        ),
        _ => (None, None),
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
    fn test_valid_typeof_string() {
        let diags = lint(r#"const x: boolean = typeof y == "int";"#);
        assert!(!has_rule(&diags, "L1011"));
    }

    #[test]
    fn test_valid_typeof_float() {
        let diags = lint(r#"const x: boolean = typeof y == "float";"#);
        assert!(!has_rule(&diags, "L1011"));
    }

    #[test]
    fn test_valid_typeof_null() {
        let diags = lint(r#"const x: boolean = typeof y == "null";"#);
        assert!(!has_rule(&diags, "L1011"));
    }

    #[test]
    fn test_invalid_number() {
        let diags = lint(r#"const x: boolean = typeof y == "number";"#);
        assert!(has_rule(&diags, "L1011"), "should flag 'number', got: {:?}", diags);
        assert!(diags.iter().any(|d| d.fix.is_some()), "should be fixable");
    }

    #[test]
    fn test_invalid_undefined() {
        let diags = lint(r#"const x: boolean = typeof y == "undefined";"#);
        assert!(has_rule(&diags, "L1011"), "should flag 'undefined'");
        assert!(diags.iter().any(|d| d.fix.is_some()), "should be fixable");
    }

    #[test]
    fn test_invalid_symbol() {
        let diags = lint(r#"const x: boolean = typeof y == "symbol";"#);
        assert!(has_rule(&diags, "L1011"), "should flag 'symbol'");
    }

    #[test]
    fn test_reversed_order() {
        let diags = lint(r#"const x: boolean = "number" == typeof y;"#);
        assert!(has_rule(&diags, "L1011"), "should detect reversed order");
    }

    #[test]
    fn test_strict_equality() {
        let diags = lint(r#"const x: boolean = typeof y === "number";"#);
        assert!(has_rule(&diags, "L1011"), "should work with ===");
    }

    #[test]
    fn test_not_equal() {
        let diags = lint(r#"const x: boolean = typeof y != "undefined";"#);
        assert!(has_rule(&diags, "L1011"), "should work with !=");
    }

    #[test]
    fn test_no_typeof_no_flag() {
        let diags = lint(r#"const x: boolean = y == "number";"#);
        assert!(!has_rule(&diags, "L1011"), "should not flag non-typeof comparison");
    }
}
