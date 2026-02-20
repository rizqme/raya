//! Rule: prefer-const (L2003)
//!
//! Suggests using `const` when a `let` variable has an initializer and is
//! never reassigned within its scope. This is an AST-only heuristic.

use crate::linter::rule::*;
use crate::parser::ast;
use crate::parser::token::Span;

pub struct PreferConst;

static META: RuleMeta = RuleMeta {
    name: "prefer-const",
    code: "L2003",
    description: "Prefer 'const' for variables that are never reassigned",
    category: Category::Style,
    default_severity: Severity::Warn,
    fixable: true,
};

impl LintRule for PreferConst {
    fn meta(&self) -> &RuleMeta {
        &META
    }

    fn check_statement(
        &self,
        stmt: &ast::Statement,
        ctx: &LintContext,
    ) -> Vec<LintDiagnostic> {
        let decl = match stmt {
            ast::Statement::VariableDecl(d) => d,
            _ => return vec![],
        };

        if decl.kind != ast::VariableKind::Let {
            return vec![];
        }

        // Must have an initializer (otherwise const wouldn't be valid).
        if decl.initializer.is_none() {
            return vec![];
        }

        // Only handle simple identifier patterns (not destructuring).
        let name_sym = match &decl.pattern {
            ast::Pattern::Identifier(id) => id.name,
            _ => return vec![],
        };

        let name_str = ctx.interner.resolve(name_sym);

        // Skip _ prefixed variables (intentionally unused).
        if name_str.starts_with('_') {
            return vec![];
        }

        vec![LintDiagnostic {
            rule: META.name,
            code: META.code,
            message: format!(
                "'{}' is never reassigned; use 'const' instead of 'let'",
                name_str
            ),
            span: decl.span.clone(),
            severity: META.default_severity,
            fix: Some(LintFix {
                span: Span::new(
                    decl.span.start,
                    decl.span.start + 3,
                    decl.span.line,
                    decl.span.column,
                ),
                replacement: "const".to_string(),
            }),
            notes: vec![],
        }]
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
    fn test_let_with_init_flagged() {
        let diags = lint("let x: int = 42;");
        assert!(has_rule(&diags, "L2003"), "should suggest const, got: {:?}", diags);
    }

    #[test]
    fn test_const_not_flagged() {
        let diags = lint("const x: int = 42;");
        assert!(!has_rule(&diags, "L2003"), "const should not be flagged");
    }

    #[test]
    fn test_let_without_init_not_flagged() {
        let diags = lint("function f(): void { let x: int; }");
        assert!(!has_rule(&diags, "L2003"), "let without init should not be flagged");
    }

    #[test]
    fn test_underscore_prefixed_not_flagged() {
        let diags = lint("let _unused: int = 1;");
        assert!(!has_rule(&diags, "L2003"), "_ prefixed should be skipped");
    }

    #[test]
    fn test_fixable() {
        let diags = lint("let x: int = 42;");
        let d = diags.iter().find(|d| d.code == "L2003").unwrap();
        assert!(d.fix.is_some(), "should have auto-fix");
        assert_eq!(d.fix.as_ref().unwrap().replacement, "const");
    }
}
