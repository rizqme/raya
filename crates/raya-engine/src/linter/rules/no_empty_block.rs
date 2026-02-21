//! Rule: no-empty-block (L2002)
//!
//! Warns on empty block statements `{}` in control flow. Empty catch blocks
//! are intentionally allowed (common pattern for swallowing errors).

use crate::linter::rule::*;
use crate::parser::ast;

pub struct NoEmptyBlock;

static META: RuleMeta = RuleMeta {
    name: "no-empty-block",
    code: "L2002",
    description: "Disallow empty block statements",
    category: Category::Style,
    default_severity: Severity::Warn,
    fixable: false,
};

impl LintRule for NoEmptyBlock {
    fn meta(&self) -> &RuleMeta {
        &META
    }

    fn check_statement(
        &self,
        stmt: &ast::Statement,
        _ctx: &LintContext<'_>,
    ) -> Vec<LintDiagnostic> {
        match stmt {
            // if (...) {}
            ast::Statement::If(if_stmt) => {
                let mut diags = check_empty_body(&if_stmt.then_branch, &META);
                if let Some(else_branch) = &if_stmt.else_branch {
                    diags.extend(check_empty_body(else_branch, &META));
                }
                diags
            }
            // while (...) {}
            ast::Statement::While(while_stmt) => {
                check_empty_body(&while_stmt.body, &META)
            }
            // do {} while (...)
            ast::Statement::DoWhile(do_stmt) => {
                check_empty_body(&do_stmt.body, &META)
            }
            // for (...) {}
            ast::Statement::For(for_stmt) => {
                check_empty_body(&for_stmt.body, &META)
            }
            // for ... of ... {}
            ast::Statement::ForOf(for_of_stmt) => {
                check_empty_body(&for_of_stmt.body, &META)
            }
            // function foo() {} — empty function body
            ast::Statement::FunctionDecl(func) if func.body.statements.is_empty() => {
                vec![LintDiagnostic {
                    rule: META.name,
                    code: META.code,
                    message: "Empty function body".to_string(),
                    span: func.body.span,
                    severity: META.default_severity,
                    fix: None,
                    notes: vec![],
                }]
            }
            // try {} catch(e) {} — only flag empty try body, NOT empty catch
            ast::Statement::Try(try_stmt) => {
                let mut diags = Vec::new();
                if try_stmt.body.statements.is_empty() {
                    diags.push(LintDiagnostic {
                        rule: META.name,
                        code: META.code,
                        message: "Empty try body".to_string(),
                        span: try_stmt.body.span,
                        severity: META.default_severity,
                        fix: None,
                        notes: vec![],
                    });
                }
                // Empty catch is intentionally allowed
                diags
            }
            _ => vec![],
        }
    }
}

/// Check if a statement is an empty block (used for if/while/for bodies).
fn check_empty_body(stmt: &ast::Statement, meta: &RuleMeta) -> Vec<LintDiagnostic> {
    // The body of if/while/for is always a Block statement wrapping a BlockStatement.
    if let ast::Statement::Block(block) = stmt {
        if block.statements.is_empty() {
            return vec![LintDiagnostic {
                rule: meta.name,
                code: meta.code,
                message: "Empty block statement".to_string(),
                span: block.span,
                severity: meta.default_severity,
                fix: None,
                notes: vec![],
            }];
        }
    }
    vec![]
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
    fn test_empty_if_block() {
        let diags = lint("function f(): void { if (true) {} }");
        assert!(has_rule(&diags, "L2002"), "should flag empty if block, got: {:?}", diags);
    }

    #[test]
    fn test_nonempty_if_block() {
        let diags = lint("function f(): void { if (true) { const x: int = 1; } }");
        // Should only have the no-constant-condition hit, not no-empty-block
        assert!(!has_rule(&diags, "L2002"), "should not flag non-empty if block");
    }

    #[test]
    fn test_empty_catch_allowed() {
        let diags = lint("function f(): void { try { const x: int = 1; } catch (e) {} }");
        assert!(!has_rule(&diags, "L2002"), "empty catch should be allowed");
    }

    #[test]
    fn test_empty_function_body() {
        let diags = lint("function noop(): void {}");
        assert!(has_rule(&diags, "L2002"), "should flag empty function body");
    }
}
