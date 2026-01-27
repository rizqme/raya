//! Statement Lowering
//!
//! Converts AST statements to IR instructions.

use super::Lowerer;
use crate::ir::{BinaryOp, IrInstr, Register, Terminator};
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

        // Push loop context for break/continue
        // For continue in do-while, jump to condition block
        self.loop_stack.push(super::LoopContext {
            break_target: exit_block,
            continue_target: cond_block,
        });

        // Body block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(body_block, "dowhile.body"));
        self.current_block = body_block;
        self.lower_stmt(&do_while.body);
        if !self.current_block_is_terminated() {
            self.set_terminator(Terminator::Jump(cond_block));
        }

        // Pop loop context
        self.loop_stack.pop();

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

    fn lower_for_of(&mut self, for_of: &ast::ForOfStatement) {
        // For-of loops are desugared to index-based iteration:
        // for (let x of arr) { body }
        // becomes:
        // let _idx = 0;
        // let _len = arr.length;
        // while (_idx < _len) {
        //     let x = arr[_idx];
        //     body;
        //     _idx = _idx + 1;
        // }

        let number_ty = TypeId::new(2); // number type

        // Lower the iterable (array) expression
        let array_reg = self.lower_expr(&for_of.right);

        // Get array length: _len = arr.length
        let len_reg = self.alloc_register(number_ty);
        self.emit(IrInstr::ArrayLen {
            dest: len_reg.clone(),
            array: array_reg.clone(),
        });

        // Initialize index: _idx = 0
        let idx_local = self.allocate_anonymous_local();
        let idx_reg = self.alloc_register(number_ty);
        self.emit(IrInstr::Assign {
            dest: idx_reg.clone(),
            value: crate::ir::IrValue::Constant(crate::ir::IrConstant::I32(0)),
        });
        self.emit(IrInstr::StoreLocal {
            index: idx_local,
            value: idx_reg.clone(),
        });

        // Create blocks
        let header_block = self.alloc_block();
        let body_block = self.alloc_block();
        let update_block = self.alloc_block();
        let exit_block = self.alloc_block();

        // Jump to header
        self.set_terminator(Terminator::Jump(header_block));

        // Header block: compare index < length
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(header_block, "forof.header"));
        self.current_block = header_block;

        // Load current index
        let current_idx = self.alloc_register(number_ty);
        self.emit(IrInstr::LoadLocal {
            dest: current_idx.clone(),
            index: idx_local,
        });

        // Compare: _idx < _len
        let cond_reg = self.alloc_register(TypeId::new(4)); // boolean type
        self.emit(IrInstr::BinaryOp {
            dest: cond_reg.clone(),
            op: crate::ir::BinaryOp::Less,
            left: current_idx.clone(),
            right: len_reg.clone(),
        });

        // Branch: if condition is true, go to body; else go to exit
        self.set_terminator(Terminator::Branch {
            cond: cond_reg,
            then_block: body_block,
            else_block: exit_block,
        });

        // Push loop context for break/continue
        self.loop_stack.push(super::LoopContext {
            break_target: exit_block,
            continue_target: update_block,
        });

        // Body block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(body_block, "forof.body"));
        self.current_block = body_block;

        // Load current index again (might be different register after block switch)
        let body_idx = self.alloc_register(number_ty);
        self.emit(IrInstr::LoadLocal {
            dest: body_idx.clone(),
            index: idx_local,
        });

        // Load element: x = arr[_idx]
        // Get the element type from the array type
        let elem_ty = if array_reg.ty.as_u32() >= 5 {
            // Array types start at TypeId 5+, element type is encoded
            // For simplicity, use number type as default
            number_ty
        } else {
            number_ty
        };

        let elem_reg = self.alloc_register(elem_ty);
        self.emit(IrInstr::LoadElement {
            dest: elem_reg.clone(),
            array: array_reg.clone(),
            index: body_idx,
        });

        // Bind the loop variable
        match &for_of.left {
            ast::ForOfLeft::VariableDecl(decl) => {
                if let ast::Pattern::Identifier(ident) = &decl.pattern {
                    let local_idx = self.allocate_local(ident.name);
                    self.local_registers.insert(local_idx, elem_reg.clone());
                    self.emit(IrInstr::StoreLocal {
                        index: local_idx,
                        value: elem_reg,
                    });
                }
            }
            ast::ForOfLeft::Pattern(pattern) => {
                // For existing variable pattern
                if let ast::Pattern::Identifier(ident) = pattern {
                    if let Some(local_idx) = self.lookup_local(ident.name) {
                        self.emit(IrInstr::StoreLocal {
                            index: local_idx,
                            value: elem_reg,
                        });
                    }
                }
            }
        }

        // Lower the body
        self.lower_stmt(&for_of.body);

        // Jump to update block if not terminated
        if !self.current_block_is_terminated() {
            self.set_terminator(Terminator::Jump(update_block));
        }

        // Pop loop context
        self.loop_stack.pop();

        // Update block: _idx = _idx + 1
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(update_block, "forof.update"));
        self.current_block = update_block;

        // Load current index
        let update_idx = self.alloc_register(number_ty);
        self.emit(IrInstr::LoadLocal {
            dest: update_idx.clone(),
            index: idx_local,
        });

        // Increment: _idx + 1
        let one_reg = self.alloc_register(number_ty);
        self.emit(IrInstr::Assign {
            dest: one_reg.clone(),
            value: crate::ir::IrValue::Constant(crate::ir::IrConstant::I32(1)),
        });

        let new_idx = self.alloc_register(number_ty);
        self.emit(IrInstr::BinaryOp {
            dest: new_idx.clone(),
            op: crate::ir::BinaryOp::Add,
            left: update_idx,
            right: one_reg,
        });

        // Store incremented index
        self.emit(IrInstr::StoreLocal {
            index: idx_local,
            value: new_idx,
        });

        // Jump back to header
        self.set_terminator(Terminator::Jump(header_block));

        // Exit block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(exit_block, "forof.exit"));
        self.current_block = exit_block;
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

        // Check if this variable needs RefCell wrapping (captured by closure)
        let needs_refcell = self.refcell_vars.contains(&name);

        // If there's an initializer, evaluate and store
        // The register from lowering the expression will have the correct inferred type
        if let Some(init) = &decl.initializer {
            // Track class type for field access resolution
            if let ast::Expression::New(new_expr) = init {
                if let ast::Expression::Identifier(ident) = &*new_expr.callee {
                    if let Some(&class_id) = self.class_map.get(&ident.name) {
                        self.variable_class_map.insert(name, class_id);
                    }
                }
            }

            // Track if this is an arrow function for async closure detection
            let is_async_arrow = if let ast::Expression::Arrow(arrow) = init {
                arrow.is_async
            } else {
                false
            };

            let value = self.lower_expr(init);

            // Track closure locals for async closure detection
            // After lowering an arrow, last_closure_info has the function ID
            if is_async_arrow {
                if let Some((_, _)) = &self.last_closure_info {
                    // Find the function ID from async_closures that was just created
                    // The most recently added function ID is the one we just created
                    let last_func_id = crate::ir::FunctionId::new(self.next_function_id.saturating_sub(1));
                    if self.async_closures.contains(&last_func_id) {
                        self.closure_locals.insert(local_idx, last_func_id);
                    }
                }
            }

            if needs_refcell {
                // Wrap the value in a RefCell
                let refcell_ty = TypeId::new(0); // RefCell type
                let refcell_reg = self.alloc_register(refcell_ty);
                self.emit(IrInstr::NewRefCell {
                    dest: refcell_reg.clone(),
                    initial_value: value.clone(),
                });
                // Store the RefCell pointer as the local
                self.local_registers.insert(local_idx, refcell_reg.clone());
                self.refcell_registers.insert(local_idx, refcell_reg.clone());
                self.emit(IrInstr::StoreLocal {
                    index: local_idx,
                    value: refcell_reg,
                });
            } else {
                // Use the type from the initializer expression (already inferred during lowering)
                // This correctly handles cases like `let x = 42;` where x should be number
                self.local_registers.insert(local_idx, value.clone());
                self.emit(IrInstr::StoreLocal {
                    index: local_idx,
                    value,
                });
            }
        } else {
            // No initializer - get type from annotation or default to number
            let ty = decl
                .type_annotation
                .as_ref()
                .map(|t| self.resolve_type_annotation(t))
                .unwrap_or(TypeId::new(0));
            // Store null for uninitialized variables
            let null_reg = self.lower_null_literal();

            if needs_refcell {
                // Wrap null in a RefCell
                let refcell_ty = TypeId::new(0);
                let refcell_reg = self.alloc_register(refcell_ty);
                self.emit(IrInstr::NewRefCell {
                    dest: refcell_reg.clone(),
                    initial_value: null_reg,
                });
                // Create a typed register for the local
                let typed_reg = Register { id: refcell_reg.id, ty: refcell_ty };
                self.local_registers.insert(local_idx, typed_reg.clone());
                self.refcell_registers.insert(local_idx, refcell_reg.clone());
                self.emit(IrInstr::StoreLocal {
                    index: local_idx,
                    value: refcell_reg,
                });
            } else {
                // Create a typed register for the local
                let typed_reg = Register { id: null_reg.id, ty };
                self.local_registers.insert(local_idx, typed_reg.clone());
                self.emit(IrInstr::StoreLocal {
                    index: local_idx,
                    value: null_reg,
                });
            }
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

        // Push loop context for break/continue
        self.loop_stack.push(super::LoopContext {
            break_target: exit_block,
            continue_target: header_block,
        });

        // Body block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(body_block, "while.body"));
        self.current_block = body_block;
        self.lower_stmt(&while_stmt.body);
        if !self.current_block_is_terminated() {
            self.set_terminator(Terminator::Jump(header_block));
        }

        // Pop loop context
        self.loop_stack.pop();

        // Exit block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(exit_block, "while.exit"));
        self.current_block = exit_block;
    }

    fn lower_for(&mut self, for_stmt: &ast::ForStatement) {
        // Track if we need per-iteration binding for a captured loop variable
        // This implements JavaScript/TypeScript `let` semantics where each iteration
        // gets a fresh binding, so closures capture the value from their iteration.
        let mut loop_var_info: Option<(raya_parser::Symbol, u16)> = None;

        // Lower initializer
        if let Some(init) = &for_stmt.init {
            match init {
                ast::ForInit::VariableDecl(decl) => {
                    // Check if this is a captured variable (needs per-iteration binding)
                    // Use loop_captured_vars which tracks ALL captured variables (read or write)
                    if let ast::Pattern::Identifier(ident) = &decl.pattern {
                        let is_captured = self.loop_captured_vars.contains(&ident.name);
                        if is_captured {
                            // This variable is captured by a closure - we'll need per-iteration binding
                            // Ensure it gets RefCell treatment even for read-only captures
                            self.refcell_vars.insert(ident.name);
                            // Get the local index after lowering
                            self.lower_var_decl(decl);
                            if let Some(&local_idx) = self.local_map.get(&ident.name) {
                                loop_var_info = Some((ident.name, local_idx));
                            }
                        } else {
                            self.lower_var_decl(decl);
                        }
                    } else {
                        self.lower_var_decl(decl);
                    }
                }
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

        // Push loop context for break/continue
        // For continue, jump to update_block to execute the update expression
        self.loop_stack.push(super::LoopContext {
            break_target: exit_block,
            continue_target: update_block,
        });

        // Body block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(body_block, "for.body"));
        self.current_block = body_block;

        // Per-iteration binding setup: if the loop variable is captured,
        // create a fresh RefCell for this iteration and copy the current value into it
        let original_refcell: Option<(u16, Register)> = if let Some((_sym, local_idx)) = &loop_var_info {
            if let Some(orig_refcell) = self.refcell_registers.get(local_idx).cloned() {
                // Load current value from loop variable's RefCell
                let refcell_ty = TypeId::new(0);
                let value_reg = self.alloc_register(refcell_ty);
                self.emit(IrInstr::LoadRefCell {
                    dest: value_reg.clone(),
                    refcell: orig_refcell.clone(),
                });

                // Create per-iteration RefCell with this value
                let iter_refcell = self.alloc_register(refcell_ty);
                self.emit(IrInstr::NewRefCell {
                    dest: iter_refcell.clone(),
                    initial_value: value_reg,
                });

                // Replace mappings so closures in the body capture the per-iteration RefCell
                self.refcell_registers.insert(*local_idx, iter_refcell.clone());
                self.local_registers.insert(*local_idx, iter_refcell.clone());
                self.emit(IrInstr::StoreLocal {
                    index: *local_idx,
                    value: iter_refcell,
                });

                Some((*local_idx, orig_refcell))
            } else {
                None
            }
        } else {
            None
        };

        self.lower_stmt(&for_stmt.body);

        // Before jumping to update, copy back from per-iteration RefCell to original
        // so the update expression (i = i + 1) operates on the loop counter
        if let Some((local_idx, orig_refcell)) = &original_refcell {
            if !self.current_block_is_terminated() {
                // Load value from per-iteration RefCell
                if let Some(iter_refcell) = self.refcell_registers.get(local_idx).cloned() {
                    let refcell_ty = TypeId::new(0);
                    let value = self.alloc_register(refcell_ty);
                    self.emit(IrInstr::LoadRefCell {
                        dest: value.clone(),
                        refcell: iter_refcell,
                    });
                    // Store to original RefCell
                    self.emit(IrInstr::StoreRefCell {
                        refcell: orig_refcell.clone(),
                        value,
                    });
                }

                // Restore original RefCell mapping for update expression
                self.refcell_registers.insert(*local_idx, orig_refcell.clone());
                self.local_registers.insert(*local_idx, orig_refcell.clone());
                self.emit(IrInstr::StoreLocal {
                    index: *local_idx,
                    value: orig_refcell.clone(),
                });
            }
        }

        if !self.current_block_is_terminated() {
            self.set_terminator(Terminator::Jump(update_block));
        }

        // Pop loop context
        self.loop_stack.pop();

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
        // Save current local_map state for scope management
        // This allows nested scopes to shadow outer variables without
        // overwriting the outer variable's slot mapping
        let saved_local_map = self.local_map.clone();

        for stmt in &block.statements {
            self.lower_stmt(stmt);
            if self.current_block_is_terminated() {
                break;
            }
        }

        // Restore local_map to exit the block scope
        // This ensures outer variables are accessible again after the block
        self.local_map = saved_local_map;
    }

    fn lower_break(&mut self, _brk: &ast::BreakStatement) {
        if let Some(loop_ctx) = self.loop_stack.last() {
            self.set_terminator(Terminator::Jump(loop_ctx.break_target));
        } else {
            // Break outside of loop - should be caught by type checker
            self.set_terminator(Terminator::Unreachable);
        }
    }

    fn lower_continue(&mut self, _cont: &ast::ContinueStatement) {
        if let Some(loop_ctx) = self.loop_stack.last() {
            self.set_terminator(Terminator::Jump(loop_ctx.continue_target));
        } else {
            // Continue outside of loop - should be caught by type checker
            self.set_terminator(Terminator::Unreachable);
        }
    }

    fn lower_throw(&mut self, throw: &ast::ThrowStatement) {
        let value = self.lower_expr(&throw.value);
        self.set_terminator(Terminator::Throw(value));
    }

    fn lower_try(&mut self, try_stmt: &ast::TryStatement) {
        // Allocate blocks for catch, finally, and exit
        let has_catch = try_stmt.catch_clause.is_some();
        let has_finally = try_stmt.finally_clause.is_some();

        let catch_block = if has_catch {
            self.alloc_block()
        } else {
            // If no catch, we still need a block for the exception handler to jump to
            // This will just rethrow or jump to finally
            self.alloc_block()
        };

        let finally_block = if has_finally {
            Some(self.alloc_block())
        } else {
            None
        };

        let exit_block = self.alloc_block();

        // Emit SetupTry instruction
        self.emit(IrInstr::SetupTry {
            catch_block,
            finally_block,
        });

        // Lower try body
        self.lower_block(&try_stmt.body);

        // If try body completes normally (not terminated by return/throw)
        if !self.current_block_is_terminated() {
            // Remove exception handler
            self.emit(IrInstr::EndTry);
            // Jump to finally or exit
            if let Some(finally) = finally_block {
                self.set_terminator(Terminator::Jump(finally));
            } else {
                self.set_terminator(Terminator::Jump(exit_block));
            }
        }

        // Create catch block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(catch_block, "catch"));
        self.current_block = catch_block;

        if let Some(catch_clause) = &try_stmt.catch_clause {
            // Bind the exception parameter if present
            // The VM pushes the exception value onto the stack when jumping to catch
            if let Some(param) = &catch_clause.param {
                if let ast::Pattern::Identifier(ident) = param {
                    // Allocate local for exception parameter
                    let local_idx = self.allocate_local(ident.name);
                    // Pop exception from stack directly into local
                    // The VM pushes the exception before jumping to catch block
                    self.emit(IrInstr::PopToLocal { index: local_idx });
                    // Create a register for subsequent uses of the catch parameter
                    let exc_ty = TypeId::new(0); // Exception type (unknown)
                    let exc_reg = self.alloc_register(exc_ty);
                    self.local_registers.insert(local_idx, exc_reg);
                }
            }

            // Lower catch body
            self.lower_block(&catch_clause.body);
        }

        // After catch, jump to finally or exit
        if !self.current_block_is_terminated() {
            if let Some(finally) = finally_block {
                self.set_terminator(Terminator::Jump(finally));
            } else {
                self.set_terminator(Terminator::Jump(exit_block));
            }
        }

        // Create finally block if present
        if let Some(finally) = finally_block {
            self.current_function_mut()
                .add_block(crate::ir::BasicBlock::with_label(finally, "finally"));
            self.current_block = finally;

            if let Some(finally_clause) = &try_stmt.finally_clause {
                self.lower_block(finally_clause);
            }

            // Jump to exit after finally
            if !self.current_block_is_terminated() {
                self.set_terminator(Terminator::Jump(exit_block));
            }
        }

        // Continue at exit block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(exit_block, "try.exit"));
        self.current_block = exit_block;
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
