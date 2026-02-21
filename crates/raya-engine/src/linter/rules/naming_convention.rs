//! Rule: naming-convention (L2001)
//!
//! Enforces naming conventions:
//! - Functions and variables: camelCase
//! - Classes and type aliases: PascalCase

use crate::linter::rule::*;
use crate::parser::ast;

pub struct NamingConvention;

static META: RuleMeta = RuleMeta {
    name: "naming-convention",
    code: "L2001",
    description: "Enforce naming conventions (camelCase functions/vars, PascalCase classes/types)",
    category: Category::Style,
    default_severity: Severity::Warn,
    fixable: false,
};

impl LintRule for NamingConvention {
    fn meta(&self) -> &RuleMeta {
        &META
    }

    fn check_statement(
        &self,
        stmt: &ast::Statement,
        ctx: &LintContext<'_>,
    ) -> Vec<LintDiagnostic> {
        match stmt {
            // Variable: camelCase (or UPPER_SNAKE_CASE for const)
            ast::Statement::VariableDecl(decl) => {
                let name_sym = match &decl.pattern {
                    ast::Pattern::Identifier(id) => id.name,
                    _ => return vec![],
                };
                let name = ctx.interner.resolve(name_sym);
                // Skip _ prefixed
                if name.starts_with('_') || name == "_" {
                    return vec![];
                }
                // Allow UPPER_SNAKE_CASE for const
                if decl.kind == ast::VariableKind::Const && is_upper_snake_case(name) {
                    return vec![];
                }
                if !is_camel_case(name) {
                    vec![LintDiagnostic {
                        rule: META.name,
                        code: META.code,
                        message: format!("Variable '{}' should be camelCase", name),
                        span: decl.span,
                        severity: META.default_severity,
                        fix: None,
                        notes: vec![],
                    }]
                } else {
                    vec![]
                }
            }

            // Function: camelCase
            ast::Statement::FunctionDecl(decl) => {
                let name = ctx.interner.resolve(decl.name.name);
                if !is_camel_case(name) {
                    vec![LintDiagnostic {
                        rule: META.name,
                        code: META.code,
                        message: format!("Function '{}' should be camelCase", name),
                        span: decl.name.span,
                        severity: META.default_severity,
                        fix: None,
                        notes: vec![],
                    }]
                } else {
                    vec![]
                }
            }

            // Class: PascalCase
            ast::Statement::ClassDecl(decl) => {
                let name = ctx.interner.resolve(decl.name.name);
                if !is_pascal_case(name) {
                    vec![LintDiagnostic {
                        rule: META.name,
                        code: META.code,
                        message: format!("Class '{}' should be PascalCase", name),
                        span: decl.name.span,
                        severity: META.default_severity,
                        fix: None,
                        notes: vec![],
                    }]
                } else {
                    vec![]
                }
            }

            // Type alias: PascalCase
            ast::Statement::TypeAliasDecl(decl) => {
                let name = ctx.interner.resolve(decl.name.name);
                if !is_pascal_case(name) {
                    vec![LintDiagnostic {
                        rule: META.name,
                        code: META.code,
                        message: format!("Type '{}' should be PascalCase", name),
                        span: decl.name.span,
                        severity: META.default_severity,
                        fix: None,
                        notes: vec![],
                    }]
                } else {
                    vec![]
                }
            }

            _ => vec![],
        }
    }
}

/// Check if a name starts with a lowercase letter (camelCase).
fn is_camel_case(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase())
}

/// Check if a name starts with an uppercase letter (PascalCase).
fn is_pascal_case(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_uppercase())
}

/// Check if a name is UPPER_SNAKE_CASE (e.g. MAX_SIZE).
fn is_upper_snake_case(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
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
    fn test_camel_case_function_ok() {
        let diags = lint("function myFunc(): void {}");
        assert!(!has_rule(&diags, "L2001"), "camelCase function should be ok");
    }

    #[test]
    fn test_pascal_case_function_flagged() {
        let diags = lint("function MyFunc(): void {}");
        assert!(has_rule(&diags, "L2001"), "PascalCase function should be flagged, got: {:?}", diags);
    }

    #[test]
    fn test_pascal_case_class_ok() {
        let diags = lint("class MyClass {}");
        assert!(!has_rule(&diags, "L2001"), "PascalCase class should be ok");
    }

    #[test]
    fn test_camel_case_class_flagged() {
        let diags = lint("class myClass {}");
        assert!(has_rule(&diags, "L2001"), "camelCase class should be flagged");
    }

    #[test]
    fn test_upper_snake_case_const_ok() {
        let diags = lint("const MAX_SIZE: int = 100;");
        // Should not get naming convention warning (UPPER_SNAKE_CASE allowed for const)
        assert!(!has_rule(&diags, "L2001"), "UPPER_SNAKE_CASE const should be ok");
    }

    #[test]
    fn test_type_alias_pascal_case() {
        let diags = lint("type myType = int;");
        assert!(has_rule(&diags, "L2001"), "camelCase type alias should be flagged");
    }
}
