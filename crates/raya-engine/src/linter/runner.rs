//! Lint runner — single-pass AST visitor that dispatches to all enabled rules.

use crate::parser::ast::{self, visitor::{self, Visitor}};

use super::rule::{LintContext, LintDiagnostic, LintRule};

/// Runs all enabled lint rules over an AST in a single traversal.
pub struct LintRunner<'a> {
    rules: &'a [Box<dyn LintRule>],
    ctx: LintContext<'a>,
    diagnostics: Vec<LintDiagnostic>,
}

impl<'a> LintRunner<'a> {
    /// Create a new runner with the given rules and context.
    pub fn new(rules: &'a [Box<dyn LintRule>], ctx: LintContext<'a>) -> Self {
        Self {
            rules,
            ctx,
            diagnostics: Vec::new(),
        }
    }

    /// Run all rules over the module and return collected diagnostics.
    pub fn run(mut self, module: &ast::Module) -> Vec<LintDiagnostic> {
        // First, let rules inspect the module as a whole.
        for rule in self.rules {
            self.diagnostics.extend(rule.check_module(module, &self.ctx));
        }

        // Then walk the AST, dispatching statement/expression/class_member checks.
        self.visit_module(module);

        self.diagnostics
    }
}

impl<'a> Visitor for LintRunner<'a> {
    fn visit_statement(&mut self, stmt: &ast::Statement) {
        for rule in self.rules {
            self.diagnostics.extend(rule.check_statement(stmt, &self.ctx));
        }
        visitor::walk_statement(self, stmt);
    }

    fn visit_expression(&mut self, expr: &ast::Expression) {
        for rule in self.rules {
            self.diagnostics.extend(rule.check_expression(expr, &self.ctx));
        }
        visitor::walk_expression(self, expr);
    }

    fn visit_class_decl(&mut self, decl: &ast::ClassDecl) {
        // Dispatch class members to rules before walking into them.
        for member in &decl.members {
            for rule in self.rules {
                self.diagnostics
                    .extend(rule.check_class_member(member, &self.ctx));
            }
        }
        visitor::walk_class_decl(self, decl);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linter::rule::{Category, RuleMeta, Severity};
    use crate::parser::token::Span;
    use crate::parser::interner::Interner;

    /// A trivial test rule that flags every statement.
    struct FlagAllStatements;

    static FLAG_ALL_META: RuleMeta = RuleMeta {
        name: "flag-all",
        code: "T0001",
        description: "Flags every statement (test only)",
        category: Category::Correctness,
        default_severity: Severity::Warn,
        fixable: false,
    };

    impl LintRule for FlagAllStatements {
        fn meta(&self) -> &RuleMeta {
            &FLAG_ALL_META
        }

        fn check_statement(
            &self,
            stmt: &ast::Statement,
            _ctx: &LintContext,
        ) -> Vec<LintDiagnostic> {
            vec![LintDiagnostic {
                rule: self.meta().name,
                code: self.meta().code,
                message: "flagged".to_string(),
                span: stmt.span().clone(),
                severity: Severity::Warn,
                fix: None,
                notes: vec![],
            }]
        }
    }

    #[test]
    fn test_runner_dispatches_to_rules() {
        let mut interner = Interner::new();
        let source = "const x: int = 1;";
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(FlagAllStatements)];

        let sym_x = interner.intern("x");

        let ctx = LintContext {
            source,
            interner: &interner,
            file_path: "test.raya",
        };

        // Build a minimal module with one statement
        let module = ast::Module::new(
            vec![ast::Statement::VariableDecl(ast::VariableDecl {
                kind: ast::VariableKind::Const,
                pattern: ast::Pattern::Identifier(ast::Identifier::new(
                    sym_x,
                    Span::new(6, 7, 1, 7),
                )),
                type_annotation: None,
                initializer: Some(ast::Expression::IntLiteral(ast::IntLiteral {
                    value: 1,
                    span: Span::new(15, 16, 1, 16),
                })),
                span: Span::new(0, 17, 1, 1),
            })],
            Span::new(0, 17, 1, 1),
        );

        let runner = LintRunner::new(&rules, ctx);
        let diags = runner.run(&module);

        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, "T0001");
        assert_eq!(diags[0].message, "flagged");
    }

    #[test]
    fn test_runner_empty_rules() {
        let interner = Interner::new();
        let source = "";
        let rules: Vec<Box<dyn LintRule>> = vec![];
        let ctx = LintContext {
            source,
            interner: &interner,
            file_path: "empty.raya",
        };
        let module = ast::Module::new(vec![], Span::new(0, 0, 1, 1));

        let runner = LintRunner::new(&rules, ctx);
        let diags = runner.run(&module);

        assert!(diags.is_empty());
    }
}
