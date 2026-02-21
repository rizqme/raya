//! Rule: explicit-visibility (L2006)
//!
//! Flags class fields and methods that don't have an explicit `public`,
//! `private`, or `protected` modifier. Default visibility in Raya is `public`,
//! but being explicit improves readability and prevents accidental exposure.
//!
//! Default: Off (opt-in style rule).

use crate::linter::rule::*;
use crate::parser::ast;

pub struct ExplicitVisibility;

static META: RuleMeta = RuleMeta {
    name: "explicit-visibility",
    code: "L2006",
    description: "Require explicit visibility modifiers on class members (opt-in)",
    category: Category::Style,
    default_severity: Severity::Off,
    fixable: false,
};

/// Visibility keywords to search for in source text.
const VISIBILITY_KEYWORDS: &[&str] = &["public", "private", "protected"];

impl LintRule for ExplicitVisibility {
    fn meta(&self) -> &RuleMeta {
        &META
    }

    fn check_class_member(
        &self,
        member: &ast::ClassMember,
        ctx: &LintContext<'_>,
    ) -> Vec<LintDiagnostic> {
        match member {
            ast::ClassMember::Field(f) => {
                if !has_explicit_visibility(ctx.source, &f.span, &f.name.span) {
                    let name = ctx.interner.resolve(f.name.name);
                    vec![LintDiagnostic {
                        rule: META.name,
                        code: META.code,
                        message: format!("Field '{}' is missing an explicit visibility modifier", name),
                        span: f.name.span,
                        severity: META.default_severity,
                        fix: None,
                        notes: vec!["Add 'public', 'private', or 'protected' before the field".to_string()],
                    }]
                } else {
                    vec![]
                }
            }
            ast::ClassMember::Method(m) => {
                if !has_explicit_visibility(ctx.source, &m.span, &m.name.span) {
                    let name = ctx.interner.resolve(m.name.name);
                    vec![LintDiagnostic {
                        rule: META.name,
                        code: META.code,
                        message: format!("Method '{}' is missing an explicit visibility modifier", name),
                        span: m.name.span,
                        severity: META.default_severity,
                        fix: None,
                        notes: vec!["Add 'public', 'private', or 'protected' before the method".to_string()],
                    }]
                } else {
                    vec![]
                }
            }
            // Constructors don't need visibility annotations.
            ast::ClassMember::Constructor(_) => vec![],
        }
    }
}

/// Checks if the source text between the member start and the name contains
/// an explicit visibility keyword. The parser sets `Visibility::Public` both
/// for explicit `public` and for omitted visibility, so we inspect the source.
fn has_explicit_visibility(
    source: &str,
    member_span: &crate::parser::token::Span,
    name_span: &crate::parser::token::Span,
) -> bool {
    let start = member_span.start;
    let end = name_span.start;
    if end > source.len() {
        return true; // Can't determine, assume explicit.
    }
    if start >= end {
        // No prefix before the name — no visibility keyword was written.
        return false;
    }
    let prefix = &source[start..end];
    VISIBILITY_KEYWORDS.iter().any(|kw| prefix.contains(kw))
}

#[cfg(test)]
mod tests {
    use crate::linter::config::LintConfig;
    use crate::linter::rule::{LintDiagnostic, Severity};
    use crate::linter::Linter;

    fn lint(source: &str) -> Vec<LintDiagnostic> {
        let mut config = LintConfig::default();
        config.set_severity("explicit-visibility", Severity::Warn);
        let linter = Linter::with_config(config);
        linter.lint_source(source, "test.raya").diagnostics
    }

    fn has_rule(diags: &[LintDiagnostic], code: &str) -> bool {
        diags.iter().any(|d| d.code == code)
    }

    #[test]
    fn test_off_by_default() {
        let linter = Linter::new();
        let diags = linter.lint_source("class Foo { name: string = \"x\"; }", "test.raya").diagnostics;
        assert!(!has_rule(&diags, "L2006"), "should not fire when Off by default");
    }

    #[test]
    fn test_field_without_visibility_flagged() {
        let diags = lint("class Foo { name: string = \"x\"; }");
        assert!(has_rule(&diags, "L2006"), "should flag field without visibility, got: {:?}", diags);
    }

    #[test]
    fn test_field_with_public_ok() {
        let diags = lint("class Foo { public name: string = \"x\"; }");
        assert!(!has_rule(&diags, "L2006"), "should not flag explicit public");
    }

    #[test]
    fn test_field_with_private_ok() {
        let diags = lint("class Foo { private name: string = \"x\"; }");
        assert!(!has_rule(&diags, "L2006"), "should not flag explicit private");
    }

    #[test]
    fn test_field_with_protected_ok() {
        let diags = lint("class Foo { protected name: string = \"x\"; }");
        assert!(!has_rule(&diags, "L2006"), "should not flag explicit protected");
    }

    #[test]
    fn test_method_without_visibility_flagged() {
        let diags = lint("class Foo { greet(): void {} }");
        assert!(has_rule(&diags, "L2006"), "should flag method without visibility, got: {:?}", diags);
    }

    #[test]
    fn test_method_with_visibility_ok() {
        let diags = lint("class Foo { public greet(): void {} }");
        assert!(!has_rule(&diags, "L2006"));
    }

    #[test]
    fn test_constructor_ok() {
        let diags = lint("class Foo { constructor() {} }");
        assert!(!has_rule(&diags, "L2006"), "constructors should not require visibility");
    }
}
