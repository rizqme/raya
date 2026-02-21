//! Rule: no-typeof-class (L1010)
//!
//! Flags `typeof x == "MyClass"` or any string that looks like a class name.
//! Raya's `typeof` only returns primitive type strings (`"int"`, `"string"`, etc.),
//! not class names. Use `instanceof` for class checks.

use crate::linter::rule::*;
use crate::parser::ast::{self, BinaryOperator};

pub struct NoTypeofClass;

static META: RuleMeta = RuleMeta {
    name: "no-typeof-class",
    code: "L1010",
    description: "Disallow typeof compared to class names",
    category: Category::Correctness,
    default_severity: Severity::Warn,
    fixable: false,
};

/// Valid strings that Raya's `typeof` operator can return.
const VALID_TYPEOF_RESULTS: &[&str] = &[
    "int", "float", "string", "boolean", "function", "object", "null",
];

impl LintRule for NoTypeofClass {
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

        if !matches!(
            bin.operator,
            BinaryOperator::Equal
                | BinaryOperator::NotEqual
                | BinaryOperator::StrictEqual
                | BinaryOperator::StrictNotEqual
        ) {
            return vec![];
        }

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

        // Skip values handled by no-invalid-typeof or that are valid.
        if VALID_TYPEOF_RESULTS.contains(&value.as_str()) {
            return vec![];
        }

        // Detect class names: starts with uppercase letter.
        if !value.starts_with(|c: char| c.is_ascii_uppercase()) {
            return vec![];
        }

        vec![LintDiagnostic {
            rule: META.name,
            code: META.code,
            message: format!(
                "typeof does not return class names — typeof x == \"{}\" is always false",
                value
            ),
            span: str_span,
            severity: META.default_severity,
            fix: None,
            notes: vec![
                format!("Use 'x instanceof {}' to check for class instances", value),
                "typeof only returns: \"int\", \"float\", \"string\", \"boolean\", \"function\", \"object\", \"null\"".to_string(),
            ],
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
    fn test_typeof_class_name_flagged() {
        let diags = lint(r#"const x: boolean = typeof y == "MyClass";"#);
        assert!(has_rule(&diags, "L1010"), "should flag class name, got: {:?}", diags);
    }

    #[test]
    fn test_typeof_class_name_not_equal() {
        let diags = lint(r#"const x: boolean = typeof y != "Error";"#);
        assert!(has_rule(&diags, "L1010"), "should flag with !=");
    }

    #[test]
    fn test_typeof_valid_string_ok() {
        let diags = lint(r#"const x: boolean = typeof y == "string";"#);
        assert!(!has_rule(&diags, "L1010"), "valid typeof string should not be flagged");
    }

    #[test]
    fn test_typeof_object_ok() {
        let diags = lint(r#"const x: boolean = typeof y == "object";"#);
        assert!(!has_rule(&diags, "L1010"));
    }

    #[test]
    fn test_typeof_lowercase_not_class() {
        // Lowercase non-valid strings are handled by no-invalid-typeof, not this rule.
        let diags = lint(r#"const x: boolean = typeof y == "number";"#);
        assert!(!has_rule(&diags, "L1010"), "lowercase strings should not trigger this rule");
    }

    #[test]
    fn test_non_typeof_comparison_ok() {
        let diags = lint(r#"const x: boolean = y == "MyClass";"#);
        assert!(!has_rule(&diags, "L1010"));
    }

    #[test]
    fn test_reversed_order() {
        let diags = lint(r#"const x: boolean = "HttpClient" == typeof y;"#);
        assert!(has_rule(&diags, "L1010"), "should detect reversed order");
    }
}
