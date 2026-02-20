//! Rule: explicit-return-type (L2004)
//!
//! Warns when exported/public functions don't have an explicit return type
//! annotation. Helps with readability and documentation.

use crate::linter::rule::*;
use crate::parser::ast;

pub struct ExplicitReturnType;

static META: RuleMeta = RuleMeta {
    name: "explicit-return-type",
    code: "L2004",
    description: "Exported functions should have explicit return types",
    category: Category::Style,
    default_severity: Severity::Warn,
    fixable: false,
};

impl LintRule for ExplicitReturnType {
    fn meta(&self) -> &RuleMeta {
        &META
    }

    fn check_statement(
        &self,
        stmt: &ast::Statement,
        ctx: &LintContext,
    ) -> Vec<LintDiagnostic> {
        // Check exported function declarations.
        if let ast::Statement::ExportDecl(export) = stmt {
            if let ast::ExportDecl::Declaration(inner) = export {
                if let ast::Statement::FunctionDecl(func) = inner.as_ref() {
                    return check_function(func, ctx);
                }
            }
        }

        // Also check top-level functions (they're implicitly module-public).
        if let ast::Statement::FunctionDecl(func) = stmt {
            return check_function(func, ctx);
        }

        vec![]
    }
}

fn check_function(func: &ast::FunctionDecl, ctx: &LintContext) -> Vec<LintDiagnostic> {
    if func.return_type.is_some() {
        return vec![];
    }

    let name = ctx.interner.resolve(func.name.name);

    // Skip private/internal functions (start with _).
    if name.starts_with('_') {
        return vec![];
    }

    vec![LintDiagnostic {
        rule: META.name,
        code: META.code,
        message: format!("Function '{}' is missing an explicit return type", name),
        span: func.name.span.clone(),
        severity: META.default_severity,
        fix: None,
        notes: vec!["Add a return type annotation for better readability".to_string()],
    }]
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
    fn test_missing_return_type_flagged() {
        let diags = lint("function add(a: int, b: int) { return a + b; }");
        assert!(has_rule(&diags, "L2004"), "should flag missing return type, got: {:?}", diags);
    }

    #[test]
    fn test_with_return_type_ok() {
        let diags = lint("function add(a: int, b: int): int { return a + b; }");
        assert!(!has_rule(&diags, "L2004"), "function with return type should be ok");
    }

    #[test]
    fn test_underscore_prefix_skipped() {
        let diags = lint("function _internal(x: int) { return x; }");
        assert!(!has_rule(&diags, "L2004"), "_ prefixed should be skipped");
    }
}
