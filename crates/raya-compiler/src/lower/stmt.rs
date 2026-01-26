//! Statement Lowering
//!
//! Converts AST statements to IR instructions.

use super::Lowerer;
use crate::ir::{IrInstr, Terminator};
use raya_parser::ast::{self, Statement};
use raya_parser::TypeId;

impl<'a> Lowerer<'a> {
    /// Lower a statement
    pub fn lower_stmt(&mut self, stmt: &Statement) {
        // Don't emit code after a terminator
        if self.current_block_is_terminated() {
            return;
        }

        match stmt {
            Statement::VariableDecl(decl) => self.lower_var_decl(decl),
            Statement::Expression(expr) => self.lower_expr_stmt(expr),
            Statement::Return(ret) => self.lower_return(ret),
            Statement::If(if_stmt) => self.lower_if(if_stmt),
            Statement::While(while_stmt) => self.lower_while(while_stmt),
            Statement::For(for_stmt) => self.lower_for(for_stmt),
            Statement::Block(block) => self.lower_block(block),
            Statement::Break(brk) => self.lower_break(brk),
            Statement::Continue(cont) => self.lower_continue(cont),
            Statement::Throw(throw) => self.lower_throw(throw),
            Statement::Try(try_stmt) => self.lower_try(try_stmt),
            Statement::Switch(switch) => self.lower_switch(switch),
            Statement::FunctionDecl(_) => {
                // Handled at module level
            }
            Statement::ClassDecl(_) => {
                // Handled at module level
            }
            Statement::TypeAliasDecl(_) => {
                // Type-only, no runtime code
            }
            Statement::ImportDecl(_) => {
                // Handled at module level
            }
            Statement::ExportDecl(_) => {
                // Handled at module level
            }
            Statement::Empty(_) => {
                // No code to emit
            }
            Statement::DoWhile(do_while) => self.lower_do_while(do_while),
            Statement::ForOf(for_of) => self.lower_for_of(for_of),
        }
    }

    fn lower_do_while(&mut self, do_while: &ast::DoWhileStatement) {
        let body_block = self.alloc_block();
        let cond_block = self.alloc_block();
        let exit_block = self.alloc_block();

        // Jump to body (do-while executes body first)
        self.set_terminator(Terminator::Jump(body_block));

        // Body block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(body_block, "dowhile.body"));
        self.current_block = body_block;
        self.lower_stmt(&do_while.body);
        if !self.current_block_is_terminated() {
            self.set_terminator(Terminator::Jump(cond_block));
        }

        // Condition block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(cond_block, "dowhile.cond"));
        self.current_block = cond_block;
        let cond = self.lower_expr(&do_while.condition);
        self.set_terminator(Terminator::Branch {
            cond,
            then_block: body_block,
            else_block: exit_block,
        });

        // Exit block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(exit_block, "dowhile.exit"));
        self.current_block = exit_block;
    }

    fn lower_for_of(&mut self, _for_of: &ast::ForOfStatement) {
        // For-of loops require iterator support
        // For now, emit an empty block
        // A full implementation would iterate over the iterable
    }

    fn lower_var_decl(&mut self, decl: &ast::VariableDecl) {
        // Extract the variable name from the pattern
        // For now, only handle simple identifier patterns
        let name = match &decl.pattern {
            ast::Pattern::Identifier(ident) => ident.name,
            _ => {
                // For complex patterns (destructuring), we'd need more sophisticated handling
                // For now, just skip
                return;
            }
        };

        // Allocate local slot
        let local_idx = self.allocate_local(name);

        // Get type from annotation or infer from initializer
        let ty = decl
            .type_annotation
            .as_ref()
            .map(|t| self.resolve_type_annotation(t))
            .unwrap_or(TypeId::new(0));

        // If there's an initializer, evaluate and store
        if let Some(init) = &decl.initializer {
            let value = self.lower_expr(init);
            self.local_registers.insert(local_idx, value.clone());
            self.emit(IrInstr::StoreLocal {
                index: local_idx,
                value,
            });
        } else {
            // Store null for uninitialized variables
            let null_reg = self.lower_null_literal();
            self.local_registers.insert(local_idx, null_reg.clone());
            self.emit(IrInstr::StoreLocal {
                index: local_idx,
                value: null_reg,
            });
        }
    }

    fn lower_expr_stmt(&mut self, stmt: &ast::ExpressionStatement) {
        // Evaluate expression for side effects, discard result
        self.lower_expr(&stmt.expression);
    }

    fn lower_return(&mut self, ret: &ast::ReturnStatement) {
        let value = ret.value.as_ref().map(|e| self.lower_expr(e));
        self.set_terminator(Terminator::Return(value));
    }

    fn lower_if(&mut self, if_stmt: &ast::IfStatement) {
        let cond = self.lower_expr(&if_stmt.condition);

        let then_block = self.alloc_block();
        let else_block = if if_stmt.else_branch.is_some() {
            Some(self.alloc_block())
        } else {
            None
        };
        let merge_block = self.alloc_block();

        // Branch to then or else/merge
        let else_target = else_block.unwrap_or(merge_block);
        self.set_terminator(Terminator::Branch {
            cond,
            then_block,
            else_block: else_target,
        });

        // Lower then branch
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::new(then_block));
        self.current_block = then_block;
        self.lower_stmt(&if_stmt.then_branch);
        if !self.current_block_is_terminated() {
            self.set_terminator(Terminator::Jump(merge_block));
        }

        // Lower else branch if exists
        if let Some(else_stmt) = &if_stmt.else_branch {
            let else_id = else_block.unwrap();
            self.current_function_mut()
                .add_block(crate::ir::BasicBlock::new(else_id));
            self.current_block = else_id;
            self.lower_stmt(else_stmt);
            if !self.current_block_is_terminated() {
                self.set_terminator(Terminator::Jump(merge_block));
            }
        }

        // Continue at merge block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::new(merge_block));
        self.current_block = merge_block;
    }

    fn lower_while(&mut self, while_stmt: &ast::WhileStatement) {
        let header_block = self.alloc_block();
        let body_block = self.alloc_block();
        let exit_block = self.alloc_block();

        // Jump to header
        self.set_terminator(Terminator::Jump(header_block));

        // Header block: evaluate condition
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(header_block, "while.header"));
        self.current_block = header_block;
        let cond = self.lower_expr(&while_stmt.condition);
        self.set_terminator(Terminator::Branch {
            cond,
            then_block: body_block,
            else_block: exit_block,
        });

        // Body block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(body_block, "while.body"));
        self.current_block = body_block;
        self.lower_stmt(&while_stmt.body);
        if !self.current_block_is_terminated() {
            self.set_terminator(Terminator::Jump(header_block));
        }

        // Exit block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(exit_block, "while.exit"));
        self.current_block = exit_block;
    }

    fn lower_for(&mut self, for_stmt: &ast::ForStatement) {
        // Lower initializer
        if let Some(init) = &for_stmt.init {
            match init {
                ast::ForInit::VariableDecl(decl) => self.lower_var_decl(decl),
                ast::ForInit::Expression(expr) => {
                    self.lower_expr(expr);
                }
            }
        }

        let header_block = self.alloc_block();
        let body_block = self.alloc_block();
        let update_block = self.alloc_block();
        let exit_block = self.alloc_block();

        // Jump to header
        self.set_terminator(Terminator::Jump(header_block));

        // Header block: evaluate condition
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(header_block, "for.header"));
        self.current_block = header_block;

        if let Some(cond) = &for_stmt.test {
            let cond_reg = self.lower_expr(cond);
            self.set_terminator(Terminator::Branch {
                cond: cond_reg,
                then_block: body_block,
                else_block: exit_block,
            });
        } else {
            // No condition = infinite loop (until break)
            self.set_terminator(Terminator::Jump(body_block));
        }

        // Body block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(body_block, "for.body"));
        self.current_block = body_block;
        self.lower_stmt(&for_stmt.body);
        if !self.current_block_is_terminated() {
            self.set_terminator(Terminator::Jump(update_block));
        }

        // Update block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(update_block, "for.update"));
        self.current_block = update_block;
        if let Some(update) = &for_stmt.update {
            self.lower_expr(update);
        }
        self.set_terminator(Terminator::Jump(header_block));

        // Exit block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(exit_block, "for.exit"));
        self.current_block = exit_block;
    }

    fn lower_block(&mut self, block: &ast::BlockStatement) {
        for stmt in &block.statements {
            self.lower_stmt(stmt);
            if self.current_block_is_terminated() {
                break;
            }
        }
    }

    fn lower_break(&mut self, _brk: &ast::BreakStatement) {
        // In a real implementation, we'd need to track the loop exit block
        // For now, emit unreachable (break handling requires loop context)
        self.set_terminator(Terminator::Unreachable);
    }

    fn lower_continue(&mut self, _cont: &ast::ContinueStatement) {
        // In a real implementation, we'd need to track the loop header block
        // For now, emit unreachable (continue handling requires loop context)
        self.set_terminator(Terminator::Unreachable);
    }

    fn lower_throw(&mut self, throw: &ast::ThrowStatement) {
        let value = self.lower_expr(&throw.value);
        self.set_terminator(Terminator::Throw(value));
    }

    fn lower_try(&mut self, try_stmt: &ast::TryStatement) {
        // Lower try block (simplified - no exception handling in IR yet)
        self.lower_block(&try_stmt.body);

        // In a real implementation, we'd emit exception handling instructions
        // For now, just lower the catch block if the try completes normally
        if let Some(_catch) = &try_stmt.catch_clause {
            // Catch block would be jumped to on exception
        }

        // Lower finally block
        if let Some(finally) = &try_stmt.finally_clause {
            self.lower_block(finally);
        }
    }

    fn lower_switch(&mut self, switch: &ast::SwitchStatement) {
        let discriminant = self.lower_expr(&switch.discriminant);
        let exit_block = self.alloc_block();

        // Collect case blocks and values
        let mut cases = Vec::new();
        let mut default_block = None;

        for case in &switch.cases {
            let case_block = self.alloc_block();

            if let Some(test) = &case.test {
                // Extract integer value from test expression (simplified)
                // In a real implementation, we'd need to handle more complex patterns
                if let ast::Expression::IntLiteral(lit) = test {
                    cases.push((lit.value as i32, case_block));
                }
            } else {
                // Default case
                default_block = Some(case_block);
            }

            // Lower case body
            self.current_function_mut()
                .add_block(crate::ir::BasicBlock::new(case_block));
            self.current_block = case_block;
            for stmt in &case.consequent {
                self.lower_stmt(stmt);
            }
            if !self.current_block_is_terminated() {
                // Fall through to next case or exit
                self.set_terminator(Terminator::Jump(exit_block));
            }
        }

        // Set switch terminator at the original block
        // Note: This is simplified and would need proper block management
        let default = default_block.unwrap_or(exit_block);

        // Continue at exit block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::new(exit_block));
        self.current_block = exit_block;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lower::Lowerer;
    use raya_parser::{Parser, TypeContext};

    fn lower(source: &str) -> crate::ir::IrModule {
        let parser = Parser::new(source).expect("lexer error");
        let (module, interner) = parser.parse().expect("parse error");
        let type_ctx = TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        lowerer.lower_module(&module)
    }

    #[test]
    fn test_lower_var_decl() {
        let ir = lower("let x = 42;");
        assert!(ir.function_count() >= 1);
    }

    #[test]
    fn test_lower_if_statement() {
        let ir = lower("if (true) { let x = 1; } else { let x = 2; }");
        assert!(ir.function_count() >= 1);
    }

    #[test]
    fn test_lower_while_loop() {
        let ir = lower("while (true) { let x = 1; }");
        assert!(ir.function_count() >= 1);
    }

    #[test]
    fn test_lower_return_statement() {
        let source = r#"
            function foo(): number {
                return 42;
            }
        "#;
        let ir = lower(source);
        assert!(ir.get_function_by_name("foo").is_some());
    }
}
