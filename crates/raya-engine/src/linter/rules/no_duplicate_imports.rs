//! Rule: no-duplicate-imports (L1001)
//!
//! Flags when the same module is imported more than once. Multiple import
//! statements from the same source should be merged into one.

use std::collections::HashMap;

use crate::linter::rule::*;
use crate::parser::ast;

pub struct NoDuplicateImports;

static META: RuleMeta = RuleMeta {
    name: "no-duplicate-imports",
    code: "L1001",
    description: "Disallow duplicate imports from the same module",
    category: Category::Correctness,
    default_severity: Severity::Warn,
    fixable: false,
};

impl LintRule for NoDuplicateImports {
    fn meta(&self) -> &RuleMeta {
        &META
    }

    fn check_module(
        &self,
        module: &ast::Module,
        ctx: &LintContext<'_>,
    ) -> Vec<LintDiagnostic> {
        let mut seen: HashMap<&str, usize> = HashMap::new();
        let mut diagnostics = Vec::new();

        for stmt in &module.statements {
            // Also check inside export declarations
            let import = match stmt {
                ast::Statement::ImportDecl(i) => i,
                _ => continue,
            };

            let source_str = ctx.interner.resolve(import.source.value);

            if let Some(&first_line) = seen.get(source_str) {
                diagnostics.push(LintDiagnostic {
                    rule: META.name,
                    code: META.code,
                    message: format!("'{}' is imported multiple times", source_str),
                    span: import.span,
                    severity: META.default_severity,
                    fix: None,
                    notes: vec![format!(
                        "First imported at line {}. Merge the imports into a single statement.",
                        first_line
                    )],
                });
            } else {
                seen.insert(source_str, import.span.line as usize);
            }
        }

        diagnostics
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
    fn test_duplicate_imports_flagged() {
        let source = r#"
import { foo } from "mod_a";
import { bar } from "mod_a";
"#;
        let diags = lint(source);
        assert!(has_rule(&diags, "L1001"), "should flag duplicate imports, got: {:?}", diags);
    }

    #[test]
    fn test_different_sources_ok() {
        let source = r#"
import { foo } from "mod_a";
import { bar } from "mod_b";
"#;
        let diags = lint(source);
        assert!(!has_rule(&diags, "L1001"), "different sources should be ok");
    }

    #[test]
    fn test_single_import_ok() {
        let source = r#"import { foo, bar } from "mod_a";"#;
        let diags = lint(source);
        assert!(!has_rule(&diags, "L1001"), "single import should be ok");
    }
}
