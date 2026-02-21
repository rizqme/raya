//! Rule: no-self-assign (L1002)
//!
//! Flags assignments where the left and right sides are the same identifier,
//! e.g. `x = x`. These have no effect and are likely a mistake.

use crate::linter::rule::*;
use crate::parser::ast;

pub struct NoSelfAssign;

static META: RuleMeta = RuleMeta {
    name: "no-self-assign",
    code: "L1002",
    description: "Disallow assignments where both sides are the same",
    category: Category::Correctness,
    default_severity: Severity::Warn,
    fixable: false,
};

impl LintRule for NoSelfAssign {
    fn meta(&self) -> &RuleMeta {
        &META
    }

    fn check_expression(
        &self,
        expr: &ast::Expression,
        ctx: &LintContext<'_>,
    ) -> Vec<LintDiagnostic> {
        let assign = match expr {
            ast::Expression::Assignment(a) => a,
            _ => return vec![],
        };

        // Only flag plain `=` (not +=, -=, etc.)
        if assign.operator != ast::AssignmentOperator::Assign {
            return vec![];
        }

        // Both sides must be simple identifiers with the same symbol.
        let left_sym = match assign.left.as_ref() {
            ast::Expression::Identifier(id) => id.name,
            _ => return vec![],
        };
        let right_sym = match assign.right.as_ref() {
            ast::Expression::Identifier(id) => id.name,
            _ => return vec![],
        };

        if left_sym == right_sym {
            let name = ctx.interner.resolve(left_sym);
            vec![LintDiagnostic {
                rule: META.name,
                code: META.code,
                message: format!("'{}' is assigned to itself", name),
                span: assign.span,
                severity: META.default_severity,
                fix: None,
                notes: vec![],
            }]
        } else {
            vec![]
        }
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
    fn test_self_assign_flagged() {
        let diags = lint("function f(): void { let x: int = 1; x = x; }");
        assert!(has_rule(&diags, "L1002"), "should flag x = x, got: {:?}", diags);
    }

    #[test]
    fn test_different_assign_ok() {
        let diags = lint("function f(): void { let x: int = 1; let y: int = 2; x = y; }");
        assert!(!has_rule(&diags, "L1002"), "different vars should be ok");
    }

    #[test]
    fn test_compound_assign_ok() {
        let diags = lint("function f(): void { let x: int = 1; x += x; }");
        assert!(!has_rule(&diags, "L1002"), "+= self should not be flagged");
    }
}
