//! Statement Lowering
//!
//! Converts AST statements to IR instructions.

use super::{is_module_wrapper_function_name, Lowerer, UNRESOLVED, UNRESOLVED_TYPE_ID};
use crate::compiler::ir::block::BasicBlockId;
use crate::compiler::ir::{
    BinaryOp, IrConstant, IrInstr, IrValue, Register, StringCompareMode, Terminator,
};
use crate::parser::ast::{self, Statement};
use crate::parser::TypeId;
use rustc_hash::{FxHashMap, FxHashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ForOfIterableKind {
    Array,
    ClassIterator,
    Unknown,
}

impl<'a> Lowerer<'a> {
    fn materialize_current_locals_for_method_env(
        &mut self,
    ) -> FxHashMap<crate::parser::Symbol, super::MethodEnvBinding> {
        let mut env_globals = FxHashMap::default();
        let locals: Vec<(crate::parser::Symbol, u16)> =
            self.local_map.iter().map(|(s, i)| (*s, *i)).collect();

        for (sym, local_idx) in locals {
            let Some(reg) = self.local_registers.get(&local_idx).cloned() else {
                continue;
            };

            let local_val = self.alloc_register(reg.ty);
            self.emit(IrInstr::LoadLocal {
                dest: local_val.clone(),
                index: local_idx,
            });

            let global_idx = self.next_global_index;
            self.next_global_index += 1;
            self.global_type_map.insert(global_idx, reg.ty);
            self.emit(IrInstr::StoreGlobal {
                index: global_idx,
                value: local_val,
            });
            env_globals.insert(
                sym,
                super::MethodEnvBinding {
                    global_idx,
                    is_refcell: self.refcell_registers.contains_key(&local_idx),
                },
            );
        }

        env_globals
    }

    fn classify_for_of_iterable(
        &self,
        iterable: &ast::Expression,
    ) -> (ForOfIterableKind, TypeId, Option<String>) {
        use crate::parser::types::Type;

        let iterable_ty = self.get_expr_type(iterable);
        let Some(ty) = self.type_ctx.get(iterable_ty) else {
            return (ForOfIterableKind::Unknown, UNRESOLVED, None);
        };

        match ty {
            Type::Array(arr) => (ForOfIterableKind::Array, arr.element, None),
            Type::Set(set_ty) => (
                ForOfIterableKind::ClassIterator,
                set_ty.element,
                Some("Set".to_string()),
            ),
            Type::Map(_) => (
                ForOfIterableKind::ClassIterator,
                UNRESOLVED,
                Some("Map".to_string()),
            ),
            Type::Reference(reference) => match reference.name.as_str() {
                "Array" => (
                    ForOfIterableKind::Array,
                    reference
                        .type_args
                        .as_ref()
                        .and_then(|args| args.first().copied())
                        .unwrap_or(UNRESOLVED),
                    None,
                ),
                _ => (
                    ForOfIterableKind::ClassIterator,
                    reference
                        .type_args
                        .as_ref()
                        .and_then(|args| args.first().copied())
                        .unwrap_or(UNRESOLVED),
                    Some(reference.name.clone()),
                ),
            },
            Type::Generic(generic) => {
                let base_name = self.type_ctx.get(generic.base).and_then(|base| match base {
                    Type::Reference(reference) => Some(reference.name.as_str()),
                    Type::Class(class_ty) => Some(class_ty.name.as_str()),
                    _ => None,
                });
                match base_name {
                    Some("Array") => (
                        ForOfIterableKind::Array,
                        *generic.type_args.first().unwrap_or(&UNRESOLVED),
                        None,
                    ),
                    Some(name) => (
                        ForOfIterableKind::ClassIterator,
                        *generic.type_args.first().unwrap_or(&UNRESOLVED),
                        Some(name.to_string()),
                    ),
                    _ => (ForOfIterableKind::Unknown, UNRESOLVED, None),
                }
            }
            Type::Class(class_ty) => match class_ty.name.as_str() {
                "Array" => (ForOfIterableKind::Array, UNRESOLVED, None),
                _ => (
                    ForOfIterableKind::ClassIterator,
                    UNRESOLVED,
                    Some(class_ty.name.clone()),
                ),
            },
            _ => (ForOfIterableKind::Unknown, UNRESOLVED, None),
        }
    }

    fn nominal_type_id_by_name(&self, class_name: &str) -> Option<crate::compiler::ir::NominalTypeId> {
        self.nominal_type_id_from_type_name(class_name)
    }

    /// Lower a statement
    pub fn lower_stmt(&mut self, stmt: &Statement) {
        // Don't emit code after a terminator
        if self.current_block_is_terminated() {
            return;
        }

        // Track source span for sourcemap generation
        self.set_span(stmt.span());

        match stmt {
            Statement::VariableDecl(decl) => self.lower_var_decl(decl),
            Statement::Expression(expr) => self.lower_expr_stmt(expr),
            Statement::Return(ret) => self.lower_return(ret),
            Statement::Yield(yld) => self.lower_yield(yld),
            Statement::If(if_stmt) => self.lower_if(if_stmt),
            Statement::While(while_stmt) => self.lower_while(while_stmt),
            Statement::For(for_stmt) => self.lower_for(for_stmt),
            Statement::Block(block) => self.lower_block(block),
            Statement::Break(brk) => self.lower_break(brk),
            Statement::Continue(cont) => self.lower_continue(cont),
            Statement::Throw(throw) => self.lower_throw(throw),
            Statement::Try(try_stmt) => self.lower_try(try_stmt),
            Statement::Switch(switch) => self.lower_switch(switch),
            Statement::FunctionDecl(func_decl) => {
                let is_pre_registered = self.function_id_for_decl(func_decl).is_some();
                if !self.function_map.contains_key(&func_decl.name.name) || is_pre_registered {
                    // Nested function declaration — treat as closure with captures
                    self.lower_nested_function_decl(func_decl);
                }
                // Module-level declarations handled in lower_module first pass
            }
            Statement::ClassDecl(class) => {
                let Some(nominal_type_id) = self.nominal_type_id_for_decl(class) else {
                    self.errors
                        .push(crate::compiler::CompileError::InternalError {
                            message: format!(
                                "class declaration '{}' at span {} missing NominalTypeId registration",
                                self.interner.resolve(class.name.name),
                                class.span.start
                            ),
                        });
                    return;
                };

                if !self.lowered_nominal_type_ids.contains(&nominal_type_id) {
                    // Nested class declaration — lower once by declaration identity.
                    // lower_class resets per-function state (registers, blocks, locals)
                    // for each method/constructor, so save and restore enclosing state.
                    let in_module_wrapper = self
                        .current_function
                        .as_ref()
                        .is_some_and(|f| is_module_wrapper_function_name(&f.name));
                    let saved_pending_method_env = self.pending_class_method_env_globals.take();
                    if in_module_wrapper {
                        self.pending_class_method_env_globals =
                            Some(self.materialize_current_locals_for_method_env());
                    }

                    let saved_register = self.next_register;
                    let saved_block = self.next_block;
                    let saved_local_map = self.local_map.clone();
                    let saved_local_registers = self.local_registers.clone();
                    let saved_refcell_registers = self.refcell_registers.clone();
                    let saved_refcell_inner_types = self.refcell_inner_types.clone();
                    let saved_refcell_vars = self.refcell_vars.clone();
                    let saved_loop_captured_vars = self.loop_captured_vars.clone();
                    let saved_next_local = self.next_local;
                    let saved_function = self.current_function.take();
                    let saved_current_block = self.current_block;
                    let saved_current_class = self.current_class.take();
                    let saved_this_register = self.this_register.take();

                    self.lower_class_declaration(class);

                    // Restore per-function state
                    self.next_register = saved_register;
                    self.next_block = saved_block;
                    self.local_map = saved_local_map;
                    self.local_registers = saved_local_registers;
                    self.refcell_registers = saved_refcell_registers;
                    self.refcell_inner_types = saved_refcell_inner_types;
                    self.refcell_vars = saved_refcell_vars;
                    self.loop_captured_vars = saved_loop_captured_vars;
                    self.next_local = saved_next_local;
                    self.current_function = saved_function;
                    self.current_block = saved_current_block;
                    self.current_class = saved_current_class;
                    self.this_register = saved_this_register;
                    self.pending_class_method_env_globals = saved_pending_method_env;
                } else {
                    // Already-lowered declaration: emit static blocks at declaration position
                    // so execution order matches source semantics.
                    let static_blocks: Vec<ast::BlockStatement> = self
                        .class_info_map
                        .get(&nominal_type_id)
                        .map(|info| info.static_blocks.clone())
                        .unwrap_or_default();
                    for block in static_blocks {
                        for s in &block.statements {
                            self.lower_stmt(s);
                        }
                    }
                }
            }
            Statement::TypeAliasDecl(_) => {
                // Type-only, no runtime code
            }
            Statement::ImportDecl(_) => {
                // Handled at module level
            }
            Statement::ExportDecl(export) => {
                match export {
                    ast::ExportDecl::Declaration(inner) => self.lower_stmt(inner),
                    ast::ExportDecl::Default { expression, .. } => {
                        // Materialize default exports into a module-global slot so
                        // binary export metadata can reference a concrete runtime index.
                        let default_value = self.lower_expr(expression);
                        if let Some(global_idx) = self.default_export_global {
                            self.global_type_map.insert(global_idx, default_value.ty);
                            self.emit(IrInstr::StoreGlobal {
                                index: global_idx,
                                value: default_value,
                            });
                        }
                    }
                    _ => {} // Named/All exports are module-level metadata only
                }
            }
            Statement::Debugger(_) => {
                self.emit(IrInstr::Debugger);
            }
            Statement::Empty(_) => {
                // No code to emit
            }
            Statement::DoWhile(do_while) => self.lower_do_while(do_while),
            Statement::ForOf(for_of) => self.lower_for_of(for_of),
            Statement::ForIn(for_in) => self.lower_for_in(for_in),
            Statement::Labeled(labeled) => {
                // Set the pending label so the next loop picks it up
                self.pending_label = Some(labeled.label.name);
                self.lower_stmt(&labeled.body);
                // Clear pending label if not consumed (e.g., label on non-loop statement)
                self.pending_label = None;
            }
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
            label: self.pending_label.take(),
            break_target: exit_block,
            continue_target: cond_block,
            try_finally_depth: self.try_finally_stack.len(),
        });

        // Body block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(
                body_block,
                "dowhile.body",
            ));
        self.current_block = body_block;
        self.lower_stmt(&do_while.body);
        if !self.current_block_is_terminated() {
            self.set_terminator(Terminator::Jump(cond_block));
        }

        // Pop loop context
        self.loop_stack.pop();

        // Condition block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(
                cond_block,
                "dowhile.cond",
            ));
        self.current_block = cond_block;
        let cond = self.lower_expr(&do_while.condition);
        self.set_terminator(Terminator::Branch {
            cond,
            then_block: body_block,
            else_block: exit_block,
        });

        // Exit block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(
                exit_block,
                "dowhile.exit",
            ));
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

        let (iter_kind, elem_ty, iter_class_name) = self.classify_for_of_iterable(&for_of.right);

        // Normalize iterable to an indexable array for loop lowering.
        let array_reg = match iter_kind {
            ForOfIterableKind::Array | ForOfIterableKind::Unknown => self.lower_expr(&for_of.right),
            ForOfIterableKind::ClassIterator => {
                let source_reg = self.lower_expr(&for_of.right);
                let class_name = match iter_class_name.as_deref() {
                    Some(name) => name,
                    None => {
                        self.errors
                            .push(crate::compiler::CompileError::InternalError {
                                message: "for-of iterable class name not available".to_string(),
                            });
                        return;
                    }
                };
                if class_name == "Map" {
                    let iter_array = self.alloc_register(UNRESOLVED);
                    let mut lowered = false;
                    if let Some(nominal_type_id) = self.nominal_type_id_by_name("Map") {
                        if let Some(entries_sym) = self.interner.lookup("entries") {
                            if let Some(slot) = self.find_method_slot(nominal_type_id, entries_sym) {
                                self.emit(IrInstr::CallMethodExact {
                                    dest: Some(iter_array.clone()),
                                    object: source_reg.clone(),
                                    method: slot,
                                    args: vec![],
                                    optional: false,
                                });
                                lowered = true;
                            }
                        }
                    }
                    if !lowered {
                        self.emit(IrInstr::NativeCall {
                            dest: Some(iter_array.clone()),
                            native_id: crate::compiler::native_id::MAP_ENTRIES,
                            args: vec![source_reg],
                        });
                    }
                    iter_array
                } else if class_name == "Set" {
                    let iter_array = self.alloc_register(UNRESOLVED);
                    let mut lowered = false;
                    if let Some(nominal_type_id) = self.nominal_type_id_by_name("Set") {
                        if let Some(values_sym) = self.interner.lookup("values") {
                            if let Some(slot) = self.find_method_slot(nominal_type_id, values_sym) {
                                self.emit(IrInstr::CallMethodExact {
                                    dest: Some(iter_array.clone()),
                                    object: source_reg.clone(),
                                    method: slot,
                                    args: vec![],
                                    optional: false,
                                });
                                lowered = true;
                            }
                        }
                    }
                    if !lowered {
                        self.emit(IrInstr::NativeCall {
                            dest: Some(iter_array.clone()),
                            native_id: crate::compiler::native_id::SET_VALUES,
                            args: vec![source_reg],
                        });
                    }
                    iter_array
                } else {
                    let nominal_type_id = match self.nominal_type_id_by_name(class_name) {
                        Some(id) => id,
                        None => {
                            let mut known: Vec<String> = self
                                .class_map
                                .keys()
                                .map(|sym| self.interner.resolve(*sym).to_string())
                                .collect();
                            known.sort();
                            known.dedup();
                            self.errors
                                .push(crate::compiler::CompileError::InternalError {
                                    message: format!(
                                    "for-of iterable class '{}' not registered (known classes: {})",
                                    class_name,
                                    known.join(", ")
                                ),
                                });
                            return;
                        }
                    };
                    let symbol_iterator_sym = self.interner.lookup("Symbol.iterator");
                    let iterator_sym = self.interner.lookup("iterator");
                    let slot = match symbol_iterator_sym
                        .and_then(|sym| self.find_method_slot(nominal_type_id, sym))
                        .or_else(|| {
                            iterator_sym.and_then(|sym| self.find_method_slot(nominal_type_id, sym))
                        }) {
                        Some(slot) => slot,
                        None => {
                            self.errors
                            .push(crate::compiler::CompileError::InternalError {
                                message: format!(
                                    "for-of iterator method not found on {} (expected Symbol.iterator/iterator)",
                                    class_name
                                ),
                            });
                            return;
                        }
                    };
                    let iter_array = self.alloc_register(UNRESOLVED);
                    self.emit(IrInstr::CallMethodExact {
                        dest: Some(iter_array.clone()),
                        object: source_reg,
                        method: slot,
                        args: vec![],
                        optional: false,
                    });
                    iter_array
                }
            }
        };

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
            .add_block(crate::ir::BasicBlock::with_label(
                header_block,
                "forof.header",
            ));
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
            label: self.pending_label.take(),
            break_target: exit_block,
            continue_target: update_block,
            try_finally_depth: self.try_finally_stack.len(),
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
        let elem_reg = self.alloc_register(elem_ty);
        self.emit(IrInstr::LoadElement {
            dest: elem_reg.clone(),
            array: array_reg.clone(),
            index: body_idx,
        });

        // Determine loop variable name and check if captured
        let loop_var_name = match &for_of.left {
            ast::ForOfLeft::VariableDecl(decl) => {
                if let ast::Pattern::Identifier(ident) = &decl.pattern {
                    Some(ident.name)
                } else {
                    None
                }
            }
            ast::ForOfLeft::Pattern(ast::Pattern::Identifier(ident)) => Some(ident.name),
            _ => None,
        };

        let is_captured = loop_var_name
            .map(|n| self.loop_captured_vars.contains(&n))
            .unwrap_or(false);

        // If captured, mark for RefCell treatment
        if is_captured {
            if let Some(name) = loop_var_name {
                self.refcell_vars.insert(name);
            }
        }

        // Infer element class type from the iterable for field access resolution
        if let Some(var_name) = loop_var_name {
            // Check if iterable is a variable with known array element class type
            if let ast::Expression::Identifier(iter_ident) = &for_of.right {
                if let Some(&elem_nominal_type_id) = self.array_element_class_map.get(&iter_ident.name) {
                    self.variable_class_map.insert(var_name, elem_nominal_type_id);
                }
            }
            // Also check if the for-of variable has a type annotation
            if let ast::ForOfLeft::VariableDecl(decl) = &for_of.left {
                if let Some(type_ann) = &decl.type_annotation {
                    if let ast::Type::Reference(type_ref) = &type_ann.ty {
                        if let Some(&nominal_type_id) = self.class_map.get(&type_ref.name.name) {
                            self.variable_class_map.insert(var_name, nominal_type_id);
                        }
                    }
                }
            }
        }

        // Bind the loop variable (supports destructuring patterns)
        // Clone elem_reg before binding so we can use it for RefCell wrapping
        let elem_for_refcell = if is_captured {
            Some(elem_reg.clone())
        } else {
            None
        };

        match &for_of.left {
            ast::ForOfLeft::VariableDecl(decl) => {
                self.bind_pattern(&decl.pattern, elem_reg);
            }
            ast::ForOfLeft::Pattern(pattern) => match pattern {
                ast::Pattern::Identifier(ident) => {
                    if let Some(local_idx) = self.lookup_local(ident.name) {
                        self.emit(IrInstr::StoreLocal {
                            index: local_idx,
                            value: elem_reg,
                        });
                    }
                }
                _ => {
                    self.bind_pattern(pattern, elem_reg);
                }
            },
        }

        // Per-iteration RefCell binding for captured loop variables
        if is_captured {
            if let Some(var_name) = loop_var_name {
                if let (Some(&local_idx), Some(value_reg)) =
                    (self.local_map.get(&var_name), elem_for_refcell)
                {
                    let inner_ty = value_reg.ty;
                    let refcell_ty = TypeId::new(0);
                    let refcell_reg = self.alloc_register(refcell_ty);
                    self.emit(IrInstr::NewRefCell {
                        dest: refcell_reg.clone(),
                        initial_value: value_reg,
                    });
                    self.local_registers.insert(local_idx, refcell_reg.clone());
                    self.refcell_registers
                        .insert(local_idx, refcell_reg.clone());
                    self.refcell_inner_types.insert(local_idx, inner_ty);
                    self.emit(IrInstr::StoreLocal {
                        index: local_idx,
                        value: refcell_reg,
                    });
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
            .add_block(crate::ir::BasicBlock::with_label(
                update_block,
                "forof.update",
            ));
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

    fn lower_for_in(&mut self, for_in: &ast::ForInStatement) {
        // For-in loops are desugared to:
        //   let _keys = Reflect.getFieldNames(obj);
        //   let _idx = 0;
        //   let _len = _keys.length;
        //   while (_idx < _len) {
        //       let key = _keys[_idx];
        //       body;
        //       _idx = _idx + 1;
        //   }

        let number_ty = TypeId::new(2); // number type
        let string_ty = TypeId::new(1); // string type

        // Evaluate the object expression
        let obj_reg = self.lower_expr(&for_in.right);

        // Call Reflect.getFieldNames(obj) to get keys array
        let keys_reg = self.alloc_register(UNRESOLVED);
        self.emit(IrInstr::NativeCall {
            dest: Some(keys_reg.clone()),
            native_id: crate::vm::builtin::reflect::GET_FIELD_NAMES,
            args: vec![obj_reg],
        });

        // Get keys array length
        let len_reg = self.alloc_register(number_ty);
        self.emit(IrInstr::ArrayLen {
            dest: len_reg.clone(),
            array: keys_reg.clone(),
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
            .add_block(crate::ir::BasicBlock::with_label(
                header_block,
                "forin.header",
            ));
        self.current_block = header_block;

        let current_idx = self.alloc_register(number_ty);
        self.emit(IrInstr::LoadLocal {
            dest: current_idx.clone(),
            index: idx_local,
        });

        let cond_reg = self.alloc_register(TypeId::new(4)); // boolean type
        self.emit(IrInstr::BinaryOp {
            dest: cond_reg.clone(),
            op: crate::ir::BinaryOp::Less,
            left: current_idx.clone(),
            right: len_reg.clone(),
        });

        self.set_terminator(Terminator::Branch {
            cond: cond_reg,
            then_block: body_block,
            else_block: exit_block,
        });

        // Push loop context for break/continue
        self.loop_stack.push(super::LoopContext {
            label: self.pending_label.take(),
            break_target: exit_block,
            continue_target: update_block,
            try_finally_depth: self.try_finally_stack.len(),
        });

        // Body block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(body_block, "forin.body"));
        self.current_block = body_block;

        // Load current index
        let body_idx = self.alloc_register(number_ty);
        self.emit(IrInstr::LoadLocal {
            dest: body_idx.clone(),
            index: idx_local,
        });

        // Load key: key = _keys[_idx]
        let key_reg = self.alloc_register(string_ty);
        self.emit(IrInstr::LoadElement {
            dest: key_reg.clone(),
            array: keys_reg.clone(),
            index: body_idx,
        });

        // Bind the loop variable
        match &for_in.left {
            ast::ForOfLeft::VariableDecl(decl) => {
                self.bind_pattern(&decl.pattern, key_reg);
            }
            ast::ForOfLeft::Pattern(pattern) => match pattern {
                ast::Pattern::Identifier(ident) => {
                    if let Some(local_idx) = self.lookup_local(ident.name) {
                        self.emit(IrInstr::StoreLocal {
                            index: local_idx,
                            value: key_reg,
                        });
                    }
                }
                _ => {
                    self.bind_pattern(pattern, key_reg);
                }
            },
        }

        // Lower the body
        self.lower_stmt(&for_in.body);

        // Jump to update block if not terminated
        if !self.current_block_is_terminated() {
            self.set_terminator(Terminator::Jump(update_block));
        }

        // Pop loop context
        self.loop_stack.pop();

        // Update block: _idx = _idx + 1
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(
                update_block,
                "forin.update",
            ));
        self.current_block = update_block;

        let update_idx = self.alloc_register(number_ty);
        self.emit(IrInstr::LoadLocal {
            dest: update_idx.clone(),
            index: idx_local,
        });

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

        self.emit(IrInstr::StoreLocal {
            index: idx_local,
            value: new_idx,
        });

        // Jump back to header
        self.set_terminator(Terminator::Jump(header_block));

        // Exit block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(exit_block, "forin.exit"));
        self.current_block = exit_block;
    }

    /// Bind a destructuring pattern to a value register.
    /// Recursively handles nested array/object patterns.
    fn object_layout_from_type(&self, value_ty: TypeId) -> Option<Vec<(String, usize)>> {
        use crate::parser::types::Type;

        match self.type_ctx.get(value_ty)? {
            Type::Object(obj) => {
                let mut names: Vec<String> = obj
                    .properties
                    .iter()
                    .map(|prop| prop.name.clone())
                    .collect();
                names.sort_unstable();
                names.dedup();
                Some(
                    names
                        .into_iter()
                        .enumerate()
                        .map(|(idx, name)| (name, idx))
                        .collect(),
                )
            }
            Type::Reference(reference) => self
                .type_alias_object_fields
                .get(&reference.name)
                .map(|fields| {
                    fields
                        .iter()
                        .map(|(name, idx, _)| (name.clone(), *idx as usize))
                        .collect()
                })
                .or_else(|| {
                    self.type_ctx
                        .lookup_named_type(&reference.name)
                        .and_then(|named| self.object_layout_from_type(named))
                }),
            Type::Class(_) => {
                let nominal_type_id = self.nominal_type_id_from_type_id(value_ty)?;
                let mut fields = self.get_all_fields(nominal_type_id);
                fields.sort_by_key(|f| f.index);
                Some(
                    fields
                        .into_iter()
                        .map(|f| (self.interner.resolve(f.name).to_string(), f.index as usize))
                        .collect(),
                )
            }
            Type::TypeVar(tv) => tv
                .constraint
                .and_then(|constraint| self.object_layout_from_type(constraint)),
            Type::Generic(generic) => self.object_layout_from_type(generic.base),
            Type::Union(union) => {
                let mut merged_names: FxHashSet<String> = FxHashSet::default();
                let mut found = false;
                for member in &union.members {
                    let Some(layout) = self.object_layout_from_type(*member) else {
                        continue;
                    };
                    found = true;
                    merged_names.extend(layout.into_iter().map(|(name, _)| name));
                }
                if !found {
                    return None;
                }
                let mut names: Vec<String> = merged_names.into_iter().collect();
                names.sort_unstable();
                names.dedup();
                Some(
                    names
                        .into_iter()
                        .enumerate()
                        .map(|(idx, name)| (name, idx))
                        .collect(),
                )
            }
            _ => None,
        }
    }

    fn object_property_type_from_value_type(
        &self,
        value_ty: TypeId,
        property_name: &str,
    ) -> Option<TypeId> {
        use crate::parser::types::Type;

        match self.type_ctx.get(value_ty)? {
            Type::Object(obj) => obj
                .properties
                .iter()
                .find(|prop| prop.name == property_name)
                .map(|prop| prop.ty),
            Type::Reference(reference) => self
                .type_alias_field_lookup(&reference.name, property_name)
                .map(|(_, ty)| ty)
                .or_else(|| {
                    self.type_ctx
                        .lookup_named_type(&reference.name)
                        .and_then(|named| {
                            self.object_property_type_from_value_type(named, property_name)
                        })
                }),
            Type::Class(_) => {
                let nominal_type_id = self.nominal_type_id_from_type_id(value_ty)?;
                self.get_all_fields(nominal_type_id)
                    .into_iter()
                    .find(|field| self.interner.resolve(field.name) == property_name)
                    .map(|field| field.ty)
            }
            Type::TypeVar(tv) => tv.constraint.and_then(|constraint| {
                self.object_property_type_from_value_type(constraint, property_name)
            }),
            Type::Generic(generic) => {
                self.object_property_type_from_value_type(generic.base, property_name)
            }
            Type::Union(union) => {
                let mut found: Option<TypeId> = None;
                for member in &union.members {
                    let Some(member_ty) =
                        self.object_property_type_from_value_type(*member, property_name)
                    else {
                        continue;
                    };
                    match found {
                        None => found = Some(member_ty),
                        Some(existing) if existing == member_ty => {}
                        Some(_) => return None,
                    }
                }
                found
            }
            _ => None,
        }
    }

    fn array_element_object_layout_from_type(
        &self,
        value_ty: TypeId,
    ) -> Option<Vec<(String, usize)>> {
        use crate::parser::types::Type;

        match self.type_ctx.get(value_ty)? {
            Type::Array(arr) => self.object_layout_from_type(arr.element),
            Type::Tuple(tuple) => tuple
                .elements
                .first()
                .and_then(|elem_ty| self.object_layout_from_type(*elem_ty)),
            Type::TypeVar(tv) => tv
                .constraint
                .and_then(|constraint| self.array_element_object_layout_from_type(constraint)),
            Type::Generic(generic) => self.array_element_object_layout_from_type(generic.base),
            Type::Union(union) => {
                let mut found: Option<Vec<(String, usize)>> = None;
                for member in &union.members {
                    let Some(layout) = self.array_element_object_layout_from_type(*member) else {
                        continue;
                    };
                    match &found {
                        None => found = Some(layout),
                        Some(existing) if *existing == layout => {}
                        Some(_) => return None,
                    }
                }
                found
            }
            _ => None,
        }
    }

    pub fn bind_pattern(&mut self, pattern: &ast::Pattern, value_reg: Register) {
        match pattern {
            ast::Pattern::Identifier(ident) => {
                // Module-top-level bindings must use globals so module functions can see them.
                if self.function_depth == 0 && self.block_depth == 0 {
                    if let Some(&global_idx) = self.module_var_globals.get(&ident.name) {
                        self.global_type_map.insert(global_idx, value_reg.ty);
                        if let Some(fields) =
                            self.register_object_fields.get(&value_reg.id).cloned()
                        {
                            self.variable_object_fields.insert(ident.name, fields);
                            let nested_fields: FxHashMap<u16, Vec<(String, usize)>> = self
                                .register_nested_object_fields
                                .iter()
                                .filter_map(|(&(obj_reg, field_idx), layout)| {
                                    (obj_reg == value_reg.id).then_some((field_idx, layout.clone()))
                                })
                                .collect();
                            if !nested_fields.is_empty() {
                                self.variable_nested_object_fields
                                    .insert(ident.name, nested_fields);
                            }
                        }
                        self.emit(IrInstr::StoreGlobal {
                            index: global_idx,
                            value: value_reg,
                        });
                        return;
                    }
                }

                let local_idx = self.allocate_local(ident.name);
                if self.refcell_vars.contains(&ident.name) {
                    // Wrap in RefCell for capture-by-reference semantics
                    let refcell_reg = self.alloc_register(TypeId::new(0));
                    self.emit(IrInstr::NewRefCell {
                        dest: refcell_reg.clone(),
                        initial_value: value_reg.clone(),
                    });
                    self.local_registers.insert(local_idx, refcell_reg.clone());
                    self.refcell_registers
                        .insert(local_idx, refcell_reg.clone());
                    self.refcell_inner_types.insert(local_idx, value_reg.ty);
                    self.emit(IrInstr::StoreLocal {
                        index: local_idx,
                        value: refcell_reg,
                    });
                } else {
                    self.local_registers.insert(local_idx, value_reg.clone());
                    self.emit(IrInstr::StoreLocal {
                        index: local_idx,
                        value: value_reg,
                    });
                }
            }
            ast::Pattern::Array(array_pat) => {
                let element_layout_hint = self
                    .register_array_element_object_fields
                    .get(&value_reg.id)
                    .cloned()
                    .or_else(|| self.array_element_object_layout_from_type(value_reg.ty));
                for (i, elem_opt) in array_pat.elements.iter().enumerate() {
                    if let Some(elem) = elem_opt {
                        if let Some(default_expr) = &elem.default {
                            // With default: check bounds first, use default if OOB or null
                            let idx_reg = self.emit_i32_const(i as i32);
                            let len_reg = self.alloc_register(TypeId::new(0));
                            self.emit(IrInstr::ArrayLen {
                                dest: len_reg.clone(),
                                array: value_reg.clone(),
                            });

                            let in_bounds = self.alloc_register(TypeId::new(2));
                            self.emit(IrInstr::BinaryOp {
                                dest: in_bounds.clone(),
                                op: BinaryOp::Less,
                                left: idx_reg.clone(),
                                right: len_reg,
                            });

                            let load_block = self.alloc_block();
                            let default_block = self.alloc_block();
                            let merge_block = self.alloc_block();
                            let final_val = self.alloc_register(TypeId::new(0));

                            self.set_terminator(Terminator::Branch {
                                cond: in_bounds,
                                then_block: load_block,
                                else_block: default_block,
                            });

                            // In-bounds path: load element, then check for null
                            self.current_function_mut().add_block(
                                crate::ir::BasicBlock::with_label(load_block, "destr.load"),
                            );
                            self.current_block = load_block;
                            let elem_reg = self.alloc_register(TypeId::new(0));
                            self.emit(IrInstr::LoadElement {
                                dest: elem_reg.clone(),
                                array: value_reg.clone(),
                                index: idx_reg,
                            });

                            // Also check if the loaded value is null
                            let not_null_block = self.alloc_block();
                            self.set_terminator(Terminator::BranchIfNull {
                                value: elem_reg.clone(),
                                null_block: default_block,
                                not_null_block,
                            });

                            self.current_function_mut().add_block(
                                crate::ir::BasicBlock::with_label(not_null_block, "destr.hasval"),
                            );
                            self.current_block = not_null_block;
                            self.emit(IrInstr::Assign {
                                dest: final_val.clone(),
                                value: IrValue::Register(elem_reg),
                            });
                            self.set_terminator(Terminator::Jump(merge_block));

                            // Default path: evaluate default expression
                            self.current_function_mut().add_block(
                                crate::ir::BasicBlock::with_label(default_block, "destr.default"),
                            );
                            self.current_block = default_block;
                            let default_val = self.lower_expr(default_expr);
                            self.emit(IrInstr::Assign {
                                dest: final_val.clone(),
                                value: IrValue::Register(default_val),
                            });
                            self.set_terminator(Terminator::Jump(merge_block));

                            // Merge
                            self.current_function_mut().add_block(
                                crate::ir::BasicBlock::with_label(merge_block, "destr.merge"),
                            );
                            self.current_block = merge_block;

                            self.bind_pattern(&elem.pattern, final_val);
                        } else {
                            // No default: just load element directly
                            let idx_reg = self.emit_i32_const(i as i32);
                            let elem_reg = self.alloc_register(TypeId::new(0));
                            self.emit(IrInstr::LoadElement {
                                dest: elem_reg.clone(),
                                array: value_reg.clone(),
                                index: idx_reg,
                            });
                            if let Some(layout) = &element_layout_hint {
                                self.register_object_fields
                                    .insert(elem_reg.id, layout.clone());
                            }
                            self.bind_pattern(&elem.pattern, elem_reg);
                        }
                    }
                }

                // Handle rest pattern: ...rest = arr.slice(elements.len())
                if let Some(rest_pat) = &array_pat.rest {
                    let start_idx = self.emit_i32_const(array_pat.elements.len() as i32);
                    let len_reg = self.alloc_register(TypeId::new(0));
                    self.emit(IrInstr::ArrayLen {
                        dest: len_reg.clone(),
                        array: value_reg.clone(),
                    });

                    // Build rest array: for i in start..len { rest.push(arr[i]) }
                    let zero = self.emit_i32_const(0);
                    let rest_arr = self.alloc_register(TypeId::new(super::ARRAY_TYPE_ID));
                    self.emit(IrInstr::NewArray {
                        dest: rest_arr.clone(),
                        len: zero,
                        elem_ty: TypeId::new(0),
                    });

                    let i = self.alloc_register(TypeId::new(0));
                    self.emit(IrInstr::Assign {
                        dest: i.clone(),
                        value: IrValue::Register(start_idx),
                    });

                    let header = self.alloc_block();
                    let body = self.alloc_block();
                    let exit = self.alloc_block();

                    self.set_terminator(Terminator::Jump(header));

                    self.current_function_mut()
                        .add_block(crate::ir::BasicBlock::with_label(header, "rest.hdr"));
                    self.current_block = header;
                    let cond = self.alloc_register(TypeId::new(2));
                    self.emit(IrInstr::BinaryOp {
                        dest: cond.clone(),
                        op: BinaryOp::Less,
                        left: i.clone(),
                        right: len_reg,
                    });
                    self.set_terminator(Terminator::Branch {
                        cond,
                        then_block: body,
                        else_block: exit,
                    });

                    self.current_function_mut()
                        .add_block(crate::ir::BasicBlock::with_label(body, "rest.body"));
                    self.current_block = body;
                    let elem = self.alloc_register(TypeId::new(0));
                    self.emit(IrInstr::LoadElement {
                        dest: elem.clone(),
                        array: value_reg.clone(),
                        index: i.clone(),
                    });
                    self.emit(IrInstr::ArrayPush {
                        array: rest_arr.clone(),
                        element: elem,
                    });
                    let one = self.emit_i32_const(1);
                    self.emit(IrInstr::BinaryOp {
                        dest: i.clone(),
                        op: BinaryOp::Add,
                        left: i.clone(),
                        right: one,
                    });
                    self.set_terminator(Terminator::Jump(header));

                    self.current_function_mut()
                        .add_block(crate::ir::BasicBlock::with_label(exit, "rest.exit"));
                    self.current_block = exit;

                    self.bind_pattern(rest_pat, rest_arr);
                }
            }
            ast::Pattern::Object(obj_pat) => {
                // Object destructuring
                // Prefer statically known field layout; otherwise use Reflect.get by property name.
                let field_layout: Option<Vec<(String, usize)>> = self
                    .register_object_fields
                    .get(&value_reg.id)
                    .cloned()
                    .or_else(|| self.object_layout_from_type(value_reg.ty));
                let projected_layout: Option<Vec<(String, usize)>> = self
                    .register_structural_projection_fields
                    .get(&value_reg.id)
                    .cloned()
                    .or_else(|| self.structural_projection_layout_from_type_id(value_reg.ty));
                let projected_shape_id = projected_layout.as_ref().map(|layout| {
                    let names = layout
                        .iter()
                        .map(|(name, _)| name.clone())
                        .collect::<Vec<_>>();
                    crate::vm::object::shape_id_from_member_names(&names)
                });

                for property in &obj_pat.properties {
                    let prop_name = self.interner.resolve(property.key.name).to_string();
                    let inferred_field_ty = self
                        .object_property_type_from_value_type(value_reg.ty, &prop_name)
                        .unwrap_or(TypeId::new(0));

                    let field_reg = if let Some(ref layout) = field_layout {
                        // Statically known layout: use direct field slot when present.
                        let Some(field_index) = layout
                            .iter()
                            .find(|(name, _)| name == &prop_name)
                            .map(|(_, idx)| *idx as u16)
                        else {
                            if let Some(default_expr) = &property.default {
                                let default_val = self.lower_expr(default_expr);
                                self.bind_pattern(&property.value, default_val);
                            } else {
                                let null_reg = self.lower_null_literal();
                                self.bind_pattern(&property.value, null_reg);
                            }
                            continue;
                        };
                        let loaded = self.alloc_register(inferred_field_ty);
                        if let Some(shape_id) = projected_shape_id {
                            self.emit(IrInstr::LoadFieldShape {
                                dest: loaded.clone(),
                                object: value_reg.clone(),
                                shape_id,
                                field: field_index,
                                optional: false,
                            });
                        } else {
                            self.emit(IrInstr::LoadFieldExact {
                                dest: loaded.clone(),
                                object: value_reg.clone(),
                                field: field_index,
                                optional: false,
                            });
                        }
                        if let Some(nested_layout) = self
                            .register_nested_object_fields
                            .get(&(value_reg.id, field_index))
                            .cloned()
                        {
                            self.register_object_fields.insert(loaded.id, nested_layout);
                        }
                        if let Some(elem_layout) = self
                            .register_nested_array_element_object_fields
                            .get(&(value_reg.id, field_index))
                            .cloned()
                        {
                            self.register_array_element_object_fields
                                .insert(loaded.id, elem_layout);
                        }
                        loaded
                    } else {
                        // Dynamic layout: read by property name instead of positional slot fallback.
                        let key_reg = self.alloc_register(TypeId::new(super::STRING_TYPE_ID));
                        self.emit(IrInstr::Assign {
                            dest: key_reg.clone(),
                            value: IrValue::Constant(IrConstant::String(prop_name.clone())),
                        });
                        let loaded = self.alloc_register(inferred_field_ty);
                        self.emit(IrInstr::NativeCall {
                            dest: Some(loaded.clone()),
                            native_id: crate::compiler::native_id::REFLECT_GET,
                            args: vec![value_reg.clone(), key_reg],
                        });
                        loaded
                    };

                    // Handle default values
                    if let Some(default_expr) = &property.default {
                        let not_null_block = self.alloc_block();
                        let default_block = self.alloc_block();
                        let merge_block = self.alloc_block();
                        let final_val = self.alloc_register(TypeId::new(0));

                        self.set_terminator(Terminator::BranchIfNull {
                            value: field_reg.clone(),
                            null_block: default_block,
                            not_null_block,
                        });

                        self.current_function_mut()
                            .add_block(crate::ir::BasicBlock::with_label(
                                not_null_block,
                                "objd.hasval",
                            ));
                        self.current_block = not_null_block;
                        self.emit(IrInstr::Assign {
                            dest: final_val.clone(),
                            value: IrValue::Register(field_reg),
                        });
                        self.set_terminator(Terminator::Jump(merge_block));

                        self.current_function_mut()
                            .add_block(crate::ir::BasicBlock::with_label(
                                default_block,
                                "objd.default",
                            ));
                        self.current_block = default_block;
                        let default_val = self.lower_expr(default_expr);
                        self.emit(IrInstr::Assign {
                            dest: final_val.clone(),
                            value: IrValue::Register(default_val),
                        });
                        self.set_terminator(Terminator::Jump(merge_block));

                        self.current_function_mut()
                            .add_block(crate::ir::BasicBlock::with_label(
                                merge_block,
                                "objd.merge",
                            ));
                        self.current_block = merge_block;

                        self.bind_pattern(&property.value, final_val);
                    } else {
                        self.bind_pattern(&property.value, field_reg);
                    }
                }
            }
            ast::Pattern::Rest(rest_pat) => {
                // Rest pattern at top level — just bind the value
                self.bind_pattern(&rest_pat.argument, value_reg);
            }
        }
    }

    fn collect_spread_target_fields_from_type(&self, ty: TypeId) -> Option<FxHashSet<String>> {
        let resolved_ty = self.type_ctx.get(ty)?;
        match resolved_ty {
            crate::parser::types::Type::Object(obj) => {
                Some(obj.properties.iter().map(|p| p.name.clone()).collect())
            }
            crate::parser::types::Type::Class(_) => {
                if let Some(nominal_type_id) = self.nominal_type_id_from_type_id(ty) {
                    let names = self
                        .get_all_fields(nominal_type_id)
                        .into_iter()
                        .map(|f| self.interner.resolve(f.name).to_string())
                        .collect();
                    Some(names)
                } else {
                    None
                }
            }
            crate::parser::types::Type::TypeVar(tv) => tv
                .constraint
                .and_then(|constraint| self.collect_spread_target_fields_from_type(constraint)),
            crate::parser::types::Type::Union(union) => {
                let mut merged: FxHashSet<String> = FxHashSet::default();
                let mut found = false;
                for member in &union.members {
                    if let Some(fields) = self.collect_spread_target_fields_from_type(*member) {
                        found = true;
                        merged.extend(fields);
                    }
                }
                found.then_some(merged)
            }
            _ => None,
        }
    }

    fn object_spread_filter_from_annotation(
        &self,
        type_ann: &ast::TypeAnnotation,
    ) -> Option<FxHashSet<String>> {
        let ty = self.resolve_type_annotation(type_ann);
        self.collect_spread_target_fields_from_type(ty)
    }

    fn lower_expr_with_object_spread_filter(
        &mut self,
        expr: &ast::Expression,
        type_ann: Option<&ast::TypeAnnotation>,
    ) -> Register {
        let prev_filter = self.object_spread_target_filter.clone();
        let prev_layout = self.object_literal_target_layout.clone();
        if matches!(expr, ast::Expression::Object(_)) {
            self.object_spread_target_filter =
                type_ann.and_then(|ann| self.object_spread_filter_from_annotation(ann));
            self.object_literal_target_layout = type_ann.and_then(|ann| {
                let mut resolved_ty = self.resolve_type_annotation(ann);
                let mut layout_from_alias: Option<Vec<(String, usize)>> = None;
                if resolved_ty == super::UNRESOLVED {
                    if let ast::Type::Reference(type_ref) = &ann.ty {
                        let name = self.interner.resolve(type_ref.name.name);
                        if let Some(fields) = self.type_alias_object_fields.get(name) {
                            layout_from_alias = Some(
                                fields
                                    .iter()
                                    .map(|(field_name, idx, _)| (field_name.clone(), *idx as usize))
                                    .collect(),
                            );
                        }
                        if let Some(named) = self.type_ctx.lookup_named_type(name) {
                            resolved_ty = named;
                        }
                    }
                }
                layout_from_alias
                    .or_else(|| self.object_layout_from_type(resolved_ty))
                    .map(|layout| {
                        let mut names: Vec<String> =
                            layout.into_iter().map(|(name, _)| name).collect();
                        names.sort_unstable();
                        names.dedup();
                        names
                    })
            });
        } else {
            self.object_spread_target_filter = None;
            self.object_literal_target_layout = None;
        }
        let value = self.lower_expr(expr);
        self.object_spread_target_filter = prev_filter;
        self.object_literal_target_layout = prev_layout;
        value
    }

    fn track_variable_object_alias_from_annotation(
        &mut self,
        name: crate::parser::Symbol,
        type_ann: &ast::TypeAnnotation,
    ) {
        if let ast::Type::Reference(type_ref) = &type_ann.ty {
            let alias_name = self.interner.resolve(type_ref.name.name).to_string();
            if self.type_alias_object_fields.contains_key(&alias_name) {
                self.variable_object_type_aliases.insert(name, alias_name);
            }
        }
    }

    fn type_allows_structural_projection(&self, ty: TypeId) -> bool {
        let Some(ty_def) = self.type_ctx.get(ty) else {
            return false;
        };

        match ty_def {
            crate::parser::types::Type::Object(_)
            | crate::parser::types::Type::Interface(_) => true,
            crate::parser::types::Type::Class(class_ty) => {
                self.nominal_type_id_from_type_name(&class_ty.name).is_none()
            }
            crate::parser::types::Type::Union(union) => union
                .members
                .iter()
                .copied()
                .any(|member| self.type_allows_structural_projection(member)),
            crate::parser::types::Type::Reference(type_ref) => self
                .type_ctx
                .lookup_named_type(&type_ref.name)
                .is_some_and(|resolved| self.type_allows_structural_projection(resolved)),
            crate::parser::types::Type::Generic(generic) => {
                self.type_allows_structural_projection(generic.base)
            }
            crate::parser::types::Type::TypeVar(type_var) => type_var
                .constraint
                .or(type_var.default)
                .is_some_and(|inner| self.type_allows_structural_projection(inner)),
            _ => false,
        }
    }

    pub(super) fn structural_projection_layout_from_type_id(
        &self,
        ty: TypeId,
    ) -> Option<Vec<(String, usize)>> {
        if !self.type_allows_structural_projection(ty) {
            return None;
        }
        self.structural_slot_layout_from_type(ty).map(|layout| {
            layout
                .into_iter()
                .map(|(field_name, field_idx)| (field_name, field_idx as usize))
                .collect()
        })
    }

    fn track_variable_structural_projection_from_annotation(
        &mut self,
        name: crate::parser::Symbol,
        type_ann: &ast::TypeAnnotation,
    ) {
        if self.try_extract_class_from_type(type_ann).is_some() {
            self.variable_structural_projection_fields.remove(&name);
            return;
        }

        let ty = self.resolve_structural_slot_type_from_annotation(type_ann);
        if let Some(layout) = self.structural_projection_layout_from_type_id(ty) {
            self.variable_structural_projection_fields.insert(name, layout);
        } else {
            self.variable_structural_projection_fields.remove(&name);
        }
    }

    fn track_variable_object_alias_from_initializer(
        &mut self,
        name: crate::parser::Symbol,
        init: &ast::Expression,
    ) {
        let (call, cast_alias_name) = match init {
            ast::Expression::Call(call) => (Some(call), None),
            ast::Expression::TypeCast(cast) => {
                let alias_name = match &cast.target_type.ty {
                    ast::Type::Reference(type_ref) => {
                        Some(self.interner.resolve(type_ref.name.name).to_string())
                    }
                    _ => None,
                };
                if let ast::Expression::Call(call) = &*cast.object {
                    (Some(call), alias_name)
                } else {
                    (None, alias_name)
                }
            }
            _ => (None, None),
        };

        if let Some(alias_name) = cast_alias_name.as_ref() {
            if self.type_alias_object_fields.contains_key(alias_name) {
                self.variable_object_type_aliases
                    .insert(name, alias_name.clone());
            }
        }

        if let Some(call) = call {
            if let ast::Expression::Identifier(func_ident) = &*call.callee {
                let inferred_alias = self
                    .function_return_type_alias_map
                    .get(&func_ident.name)
                    .cloned();
                let alias_name = cast_alias_name.or(inferred_alias);
                if let Some(alias_name) = alias_name {
                    if self.type_alias_object_fields.contains_key(&alias_name) {
                        self.variable_object_type_aliases.insert(name, alias_name);
                    }
                }
            }
        }

        if !self.variable_object_type_aliases.contains_key(&name) {
            if let ast::Expression::Await(await_expr) = init {
                if let ast::Expression::Identifier(task_ident) = &*await_expr.argument {
                    if let Some(alias_name) =
                        self.task_result_type_aliases.get(&task_ident.name).cloned()
                    {
                        self.variable_object_type_aliases.insert(name, alias_name);
                    }
                }
            }
        }

        // Fallback: infer alias directly from the expression type (covers await/async-call
        // assignments where there is no direct function return-alias metadata on initializer AST).
        if !self.variable_object_type_aliases.contains_key(&name) {
            let init_ty = self.get_expr_type(init);
            if let Some(alias_name) = self.find_object_alias_for_type_id(init_ty) {
                self.variable_object_type_aliases.insert(name, alias_name);
            }
        }
    }

    fn track_variable_structural_projection_from_initializer(
        &mut self,
        name: crate::parser::Symbol,
        init: &ast::Expression,
    ) {
        let projected_layout = match init {
            ast::Expression::TypeCast(cast) => {
                if self.try_extract_class_from_type(&cast.target_type).is_some() {
                    None
                } else {
                    let target_ty = self.resolve_structural_slot_type_from_annotation(&cast.target_type);
                    self.structural_projection_layout_from_type_id(target_ty)
                }
            }
            _ => None,
        };

        if let Some(layout) = projected_layout {
            self.variable_structural_projection_fields.insert(name, layout);
        }
    }

    fn track_task_result_alias_from_initializer(
        &mut self,
        name: crate::parser::Symbol,
        init: &ast::Expression,
    ) {
        let alias = match init {
            ast::Expression::AsyncCall(async_call) => {
                self.find_return_alias_for_callee(&async_call.callee, &async_call.arguments)
            }
            _ => None,
        };
        if let Some(alias_name) = alias {
            self.task_result_type_aliases.insert(name, alias_name);
        } else {
            self.task_result_type_aliases.remove(&name);
        }
    }

    fn find_return_alias_for_callee(
        &self,
        callee: &ast::Expression,
        _args: &[ast::Expression],
    ) -> Option<String> {
        let class_name_from_id = |nominal_type_id: crate::compiler::ir::NominalTypeId| {
            self.class_map.iter().find_map(|(&sym, &cid)| {
                (cid == nominal_type_id).then_some(self.interner.resolve(sym).to_string())
            })
        };

        let direct = match callee {
            ast::Expression::Identifier(ident) => self
                .function_return_type_alias_map
                .get(&ident.name)
                .filter(|name| {
                    self.type_alias_object_fields.contains_key(*name)
                        || self.nominal_type_id_from_type_name(name).is_some()
                })
                .cloned()
                .or_else(|| {
                    self.function_return_class_map
                        .get(&ident.name)
                        .and_then(|cid| class_name_from_id(*cid))
                }),
            ast::Expression::Member(member) => {
                let nominal_type_id = self.infer_nominal_type_id(&member.object);
                nominal_type_id.and_then(|cid| {
                    self.method_return_type_alias_map
                        .get(&(cid, member.property.name))
                        .filter(|name| {
                            self.type_alias_object_fields.contains_key(*name)
                                || self.nominal_type_id_from_type_name(name).is_some()
                        })
                        .cloned()
                        .or_else(|| {
                            self.method_return_class_map
                                .get(&(cid, member.property.name))
                                .and_then(|ret_cid| class_name_from_id(*ret_cid))
                        })
                })
            }
            _ => None,
        };
        if direct.is_some() {
            return direct;
        }

        // Fallback: use checker function type of the callee expression.
        let callee_ty = self.get_expr_type(callee);
        if let Some(crate::parser::types::ty::Type::Function(func_ty)) =
            self.type_ctx.get(callee_ty)
        {
            return self.find_object_alias_for_type_id(func_ty.return_type);
        }
        None
    }

    fn find_object_alias_for_type_id(&self, ty: TypeId) -> Option<String> {
        if ty == super::UNRESOLVED {
            return None;
        }

        let mut subtype_ctx = crate::parser::types::subtyping::SubtypingContext::new(self.type_ctx);
        for alias_name in self.type_alias_object_fields.keys() {
            let alias_ty = self
                .type_alias_resolved_type_map
                .get(alias_name)
                .copied()
                .filter(|candidate| *candidate != super::UNRESOLVED)
                .or_else(|| self.type_ctx.lookup_named_type(alias_name));
            let Some(alias_ty) = alias_ty else {
                continue;
            };
            if ty == alias_ty
                || (subtype_ctx.is_subtype(ty, alias_ty) && subtype_ctx.is_subtype(alias_ty, ty))
            {
                return Some(alias_name.clone());
            }
        }
        None
    }

    fn lower_var_decl(&mut self, decl: &ast::VariableDecl) {
        // Handle destructuring patterns
        let name = match &decl.pattern {
            ast::Pattern::Identifier(ident) => ident.name,
            ast::Pattern::Array(_) | ast::Pattern::Object(_) => {
                // Destructuring: evaluate initializer, then bind pattern
                if let Some(init) = &decl.initializer {
                    let value = self
                        .lower_expr_with_object_spread_filter(init, decl.type_annotation.as_ref());
                    self.bind_pattern(&decl.pattern, value);
                }
                return;
            }
            ast::Pattern::Rest(_) => return,
        };

        // Reset stale per-name inference from other scopes before rebinding this declaration.
        self.variable_class_map.remove(&name);
        self.array_element_class_map.remove(&name);
        self.bound_method_vars.remove(&name);
        self.variable_object_fields.remove(&name);
        self.variable_nested_object_fields.remove(&name);
        self.variable_object_type_aliases.remove(&name);
        self.variable_structural_projection_fields.remove(&name);
        self.task_result_type_aliases.remove(&name);
        self.callable_symbol_hints.remove(&name);
        self.clear_late_bound_object_binding(name);

        // Re-populate object field layout for __std_exports_<tag> variables.
        // These are declared as `const __std_exports_<tag> = __std_module_<tag>()`.
        // Doing this here (after the stale-entry removal) ensures has_concrete_layout=true
        // in lower_member when the next sibling decl accesses a field of this variable.
        if let Some(init) = &decl.initializer {
            let (call_expr, cast_alias_name) = match init {
                ast::Expression::Call(call_expr) => (Some(call_expr), None),
                ast::Expression::TypeCast(cast) => {
                    let alias_name = match &cast.target_type.ty {
                        ast::Type::Reference(type_ref) => {
                            Some(self.interner.resolve(type_ref.name.name).to_string())
                        }
                        _ => None,
                    };
                    if let ast::Expression::Call(call_expr) = &*cast.object {
                        (Some(call_expr), alias_name)
                    } else {
                        (None, alias_name)
                    }
                }
                _ => (None, None),
            };

            if let Some(call_expr) = call_expr {
                if let ast::Expression::Identifier(func_ident) = &*call_expr.callee {
                    let func_name = self.interner.resolve(func_ident.name).to_string();
                    if let Some(tag) = func_name.strip_prefix("__std_module_") {
                        let alias_name = cast_alias_name
                            .unwrap_or_else(|| format!("__std_exports_type_{}", tag));
                        if let Some(fields) =
                            self.type_alias_object_fields.get(&alias_name).cloned()
                        {
                            let field_layout: Vec<(String, usize)> = fields
                                .iter()
                                .map(|(n, idx, _)| (n.clone(), *idx as usize))
                                .collect();
                            self.variable_object_fields.insert(name, field_layout);
                            self.variable_object_type_aliases.insert(name, alias_name);
                        }
                    }
                }
            }
        }

        // Populate field layout from inline object type annotation (e.g. `const x: { a: T } = ...`).
        // This gives has_concrete_layout=true so LoadFieldExact is emitted instead of LateBoundMember.
        if !self.variable_object_fields.contains_key(&name) {
            if let Some(type_ann) = &decl.type_annotation {
                if let ast::Type::Object(obj_type) = &type_ann.ty {
                    let mut member_names: Vec<String> = obj_type
                        .members
                        .iter()
                        .filter_map(|member| match member {
                            ast::ObjectTypeMember::Property(prop) => {
                                Some(self.interner.resolve(prop.name.name).to_string())
                            }
                            ast::ObjectTypeMember::Method(method) => {
                                Some(self.interner.resolve(method.name.name).to_string())
                            }
                            ast::ObjectTypeMember::IndexSignature(_) => None,
                            ast::ObjectTypeMember::CallSignature(_) => None,
                            ast::ObjectTypeMember::ConstructSignature(_) => None,
                        })
                        .collect();
                    if !member_names.is_empty() {
                        member_names.sort_unstable();
                        member_names.dedup();
                        let fields: Vec<(String, usize)> = member_names
                            .into_iter()
                            .enumerate()
                            .map(|(idx, name)| (name, idx))
                            .collect();
                        self.variable_object_fields.insert(name, fields);
                    }
                }
            }
        }

        // Check for compile-time constant: const with literal initializer.
        // Most are folded away, but module-scope globals still need runtime materialization
        // so binary export/import hydration can read a stable global slot.
        if decl.kind == crate::parser::ast::VariableKind::Const {
            if let Some(init) = &decl.initializer {
                if let Some(const_val) = self.try_eval_constant(init) {
                    // Preserve compile-time constant folding for local use sites, but
                    // still materialize module-scope globals so binary exports/imports
                    // can hydrate named constants from stable runtime slots.
                    self.constant_map.insert(name, const_val.clone());
                    if self.function_depth == 0 && self.block_depth == 0 {
                        if let Some(&global_idx) = self.module_var_globals.get(&name) {
                            let value = self.emit_constant_value(&const_val);
                            self.global_type_map.insert(global_idx, value.ty);
                            self.emit(IrInstr::StoreGlobal {
                                index: global_idx,
                                value,
                            });
                        }
                    }
                    return;
                }
            }
        }

        // Module-level variable: use global storage (not local) so module-level
        // functions can access them via LoadGlobal/StoreGlobal.
        // Only at module scope (depth 0) — inside function bodies, `let x` creates a local
        // even if a module-level `x` exists (shadowing).
        if self.function_depth == 0 && self.block_depth == 0 {
            if let Some(&global_idx) = self.module_var_globals.get(&name) {
                if let Some(init) = &decl.initializer {
                    let explicit_dynamic_any_annotation = decl.type_annotation.as_ref().is_some_and(|type_ann| {
                        self.type_is_dynamic_any_like(self.resolve_structural_slot_type_from_annotation(type_ann))
                    });
                    if explicit_dynamic_any_annotation {
                        self.dynamic_any_vars.insert(name);
                    } else {
                        self.dynamic_any_vars.remove(&name);
                    }
                    // Track class type from type annotation (same as local path)
                    if let Some(type_ann) = &decl.type_annotation {
                        if let Some(nominal_type_id) = self.try_extract_class_from_type(type_ann) {
                            self.variable_class_map.insert(name, nominal_type_id);
                            self.clear_late_bound_object_binding(name);
                        }
                        self.track_variable_object_alias_from_annotation(name, type_ann);
                        self.track_variable_structural_projection_from_annotation(name, type_ann);
                        if self.variable_structural_projection_fields.contains_key(&name) {
                            self.variable_class_map.remove(&name);
                        }
                        if let ast::Type::Array(arr_ty) = &type_ann.ty {
                            if let ast::Type::Reference(elem_ref) = &arr_ty.element_type.ty {
                                if let Some(&nominal_type_id) = self.class_map.get(&elem_ref.name.name) {
                                    self.array_element_class_map.insert(name, nominal_type_id);
                                }
                            }
                        }
                    }
                    let callable_hint = decl
                        .type_annotation
                        .as_ref()
                        .is_some_and(|t| self.type_annotation_is_callable(t))
                        || self.expression_is_callable_hint(init);
                    if callable_hint {
                        self.callable_symbol_hints.insert(name);
                    }

                    // Track class type from new expression (e.g., `let x = new MyClass()`).
                    // Always override stale mappings from previous scopes/methods.
                    if let ast::Expression::New(new_expr) = init {
                        if let ast::Expression::Identifier(ident) = &*new_expr.callee {
                            let nominal_type_id = self
                                .class_map
                                .get(&ident.name)
                                .copied()
                                .or_else(|| self.variable_class_map.get(&ident.name).copied())
                                .or_else(|| {
                                    self.nominal_type_id_from_type_name(self.interner.resolve(ident.name))
                                });
                            if let Some(nominal_type_id) = nominal_type_id {
                                self.variable_class_map.insert(name, nominal_type_id);
                                self.clear_late_bound_object_binding(name);
                            } else if self.import_bindings.contains(&ident.name)
                                || self
                                    .ambient_builtin_globals
                                    .contains(self.interner.resolve(ident.name))
                            {
                                let ctor_ty = self
                                    .get_expr_type(&new_expr.callee)
                                    .as_u32()
                                    .ne(&UNRESOLVED_TYPE_ID)
                                    .then(|| self.get_expr_type(&new_expr.callee))
                                    .or_else(|| {
                                        self.type_ctx
                                            .lookup_named_type(self.interner.resolve(ident.name))
                                    });
                                self.mark_late_bound_object_binding(name, ident.name, ctor_ty);
                            }
                        }
                    }
                    if let ast::Expression::Identifier(ident) = init {
                        let ident_name = self.interner.resolve(ident.name);
                        let constructor_type = self
                            .get_expr_type(init)
                            .as_u32()
                            .ne(&UNRESOLVED_TYPE_ID)
                            .then(|| self.get_expr_type(init))
                            .or_else(|| self.type_ctx.lookup_named_type(ident_name));
                        if self.class_map.contains_key(&ident.name)
                            || self.import_bindings.contains(&ident.name)
                            || self.ambient_builtin_globals.contains(ident_name)
                            || constructor_type.is_some_and(|ty| self.type_has_construct_signature(ty))
                        {
                            self.mark_constructor_value_binding(name, ident.name, constructor_type);
                        } else {
                            self.clear_constructor_value_binding(name);
                        }
                    } else {
                        self.clear_constructor_value_binding(name);
                    }

                    // Infer class type from method call return types
                    if let Some(nominal_type_id) = self.infer_nominal_type_id(init) {
                        self.variable_class_map.insert(name, nominal_type_id);
                        self.clear_late_bound_object_binding(name);
                    }
                    if explicit_dynamic_any_annotation {
                        self.variable_class_map.remove(&name);
                        self.clear_late_bound_object_binding(name);
                    }
                    self.track_task_result_alias_from_initializer(name, init);
                    self.track_variable_object_alias_from_initializer(name, init);
                    self.track_variable_structural_projection_from_initializer(name, init);
                    if self.variable_structural_projection_fields.contains_key(&name) {
                        self.variable_class_map.remove(&name);
                    }

                    // Track bound method variables (e.g., `let f = obj.method`)
                    if !self.js_this_binding_compat && matches!(init, ast::Expression::Member(_)) {
                        let ast::Expression::Member(member) = init else {
                            unreachable!()
                        };
                        if let Some(nominal_type_id) = self.infer_nominal_type_id(&member.object) {
                            if self
                                .find_method_slot(nominal_type_id, member.property.name)
                                .is_some()
                            {
                                self.bound_method_vars
                                    .insert(name, (nominal_type_id, member.property.name));
                            }
                        }
                    }

                    // Track if this is an async arrow function assigned to a global
                    let is_async_arrow = if let ast::Expression::Arrow(arrow) = init {
                        arrow.is_async
                    } else {
                        false
                    };

                    let value = self
                        .lower_expr_with_object_spread_filter(init, decl.type_annotation.as_ref());

                    if let Some(type_ann) = &decl.type_annotation {
                        let expected_ty =
                            self.resolve_structural_slot_type_from_annotation(type_ann);
                        if !self
                            .emit_projected_shape_registration_for_register_type(&value, expected_ty)
                        {
                            self.emit_structural_slot_registration_for_type(value.clone(), expected_ty);
                        }
                    }

                    // Fallback class capture from lowered value type.
                    // This is critical for imported/default-exported factories where
                    // pre-lowering AST inference may miss the concrete class, but
                    // the checker/lowered register type is already precise.
                    if !self.variable_class_map.contains_key(&name)
                        && !self.late_bound_object_vars.contains(&name)
                    {
                        if let Some(nominal_type_id) = self.nominal_type_id_from_type_id(value.ty) {
                            self.variable_class_map.insert(name, nominal_type_id);
                            self.clear_late_bound_object_binding(name);
                        }
                    }
                    if explicit_dynamic_any_annotation {
                        self.variable_class_map.remove(&name);
                        self.clear_late_bound_object_binding(name);
                    }

                    // Track the global's type so LoadGlobal preserves it
                    self.global_type_map.insert(global_idx, value.ty);

                    // Transfer object field layout from register to variable
                    if let Some(fields) = self.register_object_fields.get(&value.id).cloned() {
                        self.variable_object_fields.insert(name, fields);
                        let nested_fields: FxHashMap<u16, Vec<(String, usize)>> = self
                            .register_nested_object_fields
                            .iter()
                            .filter_map(|(&(obj_reg, field_idx), layout)| {
                                (obj_reg == value.id).then_some((field_idx, layout.clone()))
                            })
                            .collect();
                        if !nested_fields.is_empty() {
                            self.variable_nested_object_fields
                                .insert(name, nested_fields);
                        }
                    }
                    if !self.variable_object_fields.contains_key(&name) {
                        if let Some(alias_name) = self.variable_object_type_aliases.get(&name) {
                            if let Some(fields) = self.type_alias_object_fields.get(alias_name) {
                                let field_layout: Vec<(String, usize)> = fields
                                    .iter()
                                    .map(|(n, idx, _)| (n.clone(), *idx as usize))
                                    .collect();
                                self.variable_object_fields.insert(name, field_layout);
                            }
                        }
                    }

                    self.emit(IrInstr::StoreGlobal {
                        index: global_idx,
                        value,
                    });

                    // Track async global closures so call lowering can emit SpawnClosure
                    if is_async_arrow {
                        if let Some(func_id) = self.last_arrow_func_id.take() {
                            if self.async_closures.contains(&func_id) {
                                self.closure_globals.insert(global_idx, func_id);
                            }
                        }
                    }
                } else if let Some(type_ann) = &decl.type_annotation {
                    if self.type_is_dynamic_any_like(
                        self.resolve_structural_slot_type_from_annotation(type_ann),
                    ) {
                        self.dynamic_any_vars.insert(name);
                        self.variable_class_map.remove(&name);
                        self.clear_late_bound_object_binding(name);
                    } else {
                        self.dynamic_any_vars.remove(&name);
                    }
                } else {
                    self.dynamic_any_vars.remove(&name);
                }
                // No local allocation — resolved via LoadGlobal/StoreGlobal
                return;
            }
        }

        // Allocate local slot (only for non-constant or non-literal variables)
        let local_idx = self.allocate_local(name);

        // Check if this variable needs RefCell wrapping (captured by closure)
        let needs_refcell = self.refcell_vars.contains(&name);

        // If there's an initializer, evaluate and store
        // The register from lowering the expression will have the correct inferred type
        if let Some(init) = &decl.initializer {
            let explicit_dynamic_any_annotation = decl.type_annotation.as_ref().is_some_and(|type_ann| {
                self.type_is_dynamic_any_like(self.resolve_structural_slot_type_from_annotation(type_ann))
            });
            if explicit_dynamic_any_annotation {
                self.dynamic_any_vars.insert(name);
            } else {
                self.dynamic_any_vars.remove(&name);
            }
            // Track class type from explicit type annotation FIRST (highest priority).
            // This must come before other inference to override stale entries from other scopes
            // (variable_class_map is a flat map without scope tracking).
            if let Some(type_ann) = &decl.type_annotation {
                if let Some(nominal_type_id) = self.try_extract_class_from_type(type_ann) {
                    self.variable_class_map.insert(name, nominal_type_id);
                    self.clear_late_bound_object_binding(name);
                }
                self.track_variable_object_alias_from_annotation(name, type_ann);
                self.track_variable_structural_projection_from_annotation(name, type_ann);
                if self.variable_structural_projection_fields.contains_key(&name) {
                    self.variable_class_map.remove(&name);
                }
                // Track array element class type (e.g., `let items: Item[] = [...]`)
                if let ast::Type::Array(arr_ty) = &type_ann.ty {
                    if let ast::Type::Reference(elem_ref) = &arr_ty.element_type.ty {
                        if let Some(&nominal_type_id) = self.class_map.get(&elem_ref.name.name) {
                            self.array_element_class_map.insert(name, nominal_type_id);
                        }
                    }
                }
            }
            let callable_hint = decl
                .type_annotation
                .as_ref()
                .is_some_and(|t| self.type_annotation_is_callable(t))
                || self.expression_is_callable_hint(init);
            if callable_hint {
                self.callable_local_hints.insert(local_idx);
                self.callable_symbol_hints.insert(name);
            }

            // Track class type from New expression (e.g., `let x = new MyClass()`).
            // Always override stale mappings from previous scopes/methods.
            if let ast::Expression::New(new_expr) = init {
                if let ast::Expression::Identifier(ident) = &*new_expr.callee {
                    let nominal_type_id = self
                        .class_map
                        .get(&ident.name)
                        .copied()
                        .or_else(|| self.variable_class_map.get(&ident.name).copied())
                        .or_else(|| {
                            self.nominal_type_id_from_type_name(self.interner.resolve(ident.name))
                        });
                    if let Some(nominal_type_id) = nominal_type_id {
                        self.variable_class_map.insert(name, nominal_type_id);
                        self.clear_late_bound_object_binding(name);
                    } else if self.import_bindings.contains(&ident.name)
                        || self
                            .ambient_builtin_globals
                            .contains(self.interner.resolve(ident.name))
                    {
                        let ctor_ty = self
                            .get_expr_type(&new_expr.callee)
                            .as_u32()
                            .ne(&UNRESOLVED_TYPE_ID)
                            .then(|| self.get_expr_type(&new_expr.callee))
                            .or_else(|| {
                                self.type_ctx
                                    .lookup_named_type(self.interner.resolve(ident.name))
                            });
                        self.mark_late_bound_object_binding(name, ident.name, ctor_ty);
                    }
                }
            }
            if let ast::Expression::Identifier(ident) = init {
                let ident_name = self.interner.resolve(ident.name);
                let constructor_type = self
                    .get_expr_type(init)
                    .as_u32()
                    .ne(&UNRESOLVED_TYPE_ID)
                    .then(|| self.get_expr_type(init))
                    .or_else(|| self.type_ctx.lookup_named_type(ident_name));
                if self.class_map.contains_key(&ident.name)
                    || self.import_bindings.contains(&ident.name)
                    || self.ambient_builtin_globals.contains(ident_name)
                    || constructor_type.is_some_and(|ty| self.type_has_construct_signature(ty))
                {
                    self.mark_constructor_value_binding(name, ident.name, constructor_type);
                } else {
                    self.clear_constructor_value_binding(name);
                }
            } else {
                self.clear_constructor_value_binding(name);
            }

            // Infer class type from method call return types
            // e.g., `let output = source.pipeThrough(x)` → infer ReadableStream from return type
            if let Some(nominal_type_id) = self.infer_nominal_type_id(init) {
                self.variable_class_map.insert(name, nominal_type_id);
                self.clear_late_bound_object_binding(name);
            }
            if explicit_dynamic_any_annotation {
                self.variable_class_map.remove(&name);
                self.clear_late_bound_object_binding(name);
            }
            self.track_task_result_alias_from_initializer(name, init);
            self.track_variable_object_alias_from_initializer(name, init);
            self.track_variable_structural_projection_from_initializer(name, init);
            if self.variable_structural_projection_fields.contains_key(&name) {
                self.variable_class_map.remove(&name);
            }

            // Track bound method variables (e.g., `let f = obj.method`)
            if !self.js_this_binding_compat && matches!(init, ast::Expression::Member(_)) {
                let ast::Expression::Member(member) = init else {
                    unreachable!()
                };
                if let Some(nominal_type_id) = self.infer_nominal_type_id(&member.object) {
                    if self
                        .find_method_slot(nominal_type_id, member.property.name)
                        .is_some()
                    {
                        self.bound_method_vars
                            .insert(name, (nominal_type_id, member.property.name));
                    }
                }
            }

            // Track if this is an arrow function for async closure detection
            let is_async_arrow = if let ast::Expression::Arrow(arrow) = init {
                arrow.is_async
            } else {
                false
            };

            let value =
                self.lower_expr_with_object_spread_filter(init, decl.type_annotation.as_ref());

            if let Some(type_ann) = &decl.type_annotation {
                let expected_ty = self.resolve_structural_slot_type_from_annotation(type_ann);
                if !self.emit_projected_shape_registration_for_register_type(&value, expected_ty) {
                    self.emit_structural_slot_registration_for_type(value.clone(), expected_ty);
                }
            }

            // Fallback class capture from lowered value type.
            // Helps preserve receiver typing for chained calls on values returned
            // from imports/factories when AST-only inference was inconclusive.
            if !self.variable_class_map.contains_key(&name)
                && !self.late_bound_object_vars.contains(&name)
            {
                if let Some(nominal_type_id) = self.nominal_type_id_from_type_id(value.ty) {
                    self.variable_class_map.insert(name, nominal_type_id);
                    self.clear_late_bound_object_binding(name);
                }
            }
            if explicit_dynamic_any_annotation {
                self.variable_class_map.remove(&name);
                self.clear_late_bound_object_binding(name);
            }

            // Transfer object field layout from register to variable
            // (for decode<T> results so property access resolves correctly)
            if let Some(fields) = self.register_object_fields.get(&value.id).cloned() {
                self.variable_object_fields.insert(name, fields);
                let nested_fields: FxHashMap<u16, Vec<(String, usize)>> = self
                    .register_nested_object_fields
                    .iter()
                    .filter_map(|(&(obj_reg, field_idx), layout)| {
                        (obj_reg == value.id).then_some((field_idx, layout.clone()))
                    })
                    .collect();
                if !nested_fields.is_empty() {
                    self.variable_nested_object_fields
                        .insert(name, nested_fields);
                }
            }
            if !self.variable_object_fields.contains_key(&name) {
                if let Some(alias_name) = self.variable_object_type_aliases.get(&name) {
                    if let Some(fields) = self.type_alias_object_fields.get(alias_name) {
                        let field_layout: Vec<(String, usize)> = fields
                            .iter()
                            .map(|(n, idx, _)| (n.clone(), *idx as usize))
                            .collect();
                        self.variable_object_fields.insert(name, field_layout);
                    }
                }
            }

            // Module-wrapper top-scope arrow bindings (e.g., `const sign = (...) => ...`)
            // are referenced from nested class methods. Class methods are lowered as
            // standalone IR functions, so resolve these helpers via function_map.
            let in_module_wrapper = self
                .current_function
                .as_ref()
                .is_some_and(|f| is_module_wrapper_function_name(&f.name));
            if in_module_wrapper && matches!(init, ast::Expression::Arrow(_)) {
                if let Some(func_id) = self.last_arrow_func_id {
                    self.function_map.insert(name, func_id);
                }
            }

            // Track closure locals for async closure detection
            // Use last_arrow_func_id which is set by lower_arrow (reliable even with nested closures)
            if is_async_arrow {
                if let Some(func_id) = self.last_arrow_func_id.take() {
                    if self.async_closures.contains(&func_id) {
                        self.closure_locals.insert(local_idx, func_id);
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
                self.refcell_registers
                    .insert(local_idx, refcell_reg.clone());
                self.refcell_inner_types.insert(local_idx, value.ty);
                self.emit(IrInstr::StoreLocal {
                    index: local_idx,
                    value: refcell_reg,
                });
            } else {
                // If there's a type annotation for a numeric type, use it for the register type
                // so that operations on this variable use the correct opcodes.
                // This handles cases like `let result: number = 1` where `1` infers as int
                // but the variable should be typed as number for correct codegen (Fadd vs Iadd).
                let typed_value = if let Some(type_ann) = &decl.type_annotation {
                    let ann_ty = self.resolve_type_annotation(type_ann);
                    // If annotation resolves to UNRESOLVED (common for generic placeholders
                    // in precompiled builtin class methods), keep inferred initializer type.
                    if ann_ty != value.ty && ann_ty.as_u32() != super::UNRESOLVED_TYPE_ID {
                        use crate::compiler::ir::Register;
                        Register {
                            id: value.id,
                            ty: ann_ty,
                        }
                    } else {
                        value.clone()
                    }
                } else {
                    value.clone()
                };
                self.local_registers.insert(local_idx, typed_value);
                self.emit(IrInstr::StoreLocal {
                    index: local_idx,
                    value,
                });
            }
        } else {
            // No initializer: still honor type-annotation hints for later dispatch.
            // Without this, `let x: SomeClass; ... x.method()` can lose class-based
            // method lowering (especially across try/catch assignment paths).
            if let Some(type_ann) = &decl.type_annotation {
                if self.type_is_dynamic_any_like(self.resolve_structural_slot_type_from_annotation(type_ann)) {
                    self.dynamic_any_vars.insert(name);
                } else {
                    self.dynamic_any_vars.remove(&name);
                }
                if let Some(nominal_type_id) = self.try_extract_class_from_type(type_ann) {
                    self.variable_class_map.insert(name, nominal_type_id);
                }
                self.track_variable_object_alias_from_annotation(name, type_ann);
                self.track_variable_structural_projection_from_annotation(name, type_ann);
                if self.variable_structural_projection_fields.contains_key(&name) {
                    self.variable_class_map.remove(&name);
                }
                if let ast::Type::Array(arr_ty) = &type_ann.ty {
                    if let ast::Type::Reference(elem_ref) = &arr_ty.element_type.ty {
                        if let Some(&nominal_type_id) = self.class_map.get(&elem_ref.name.name) {
                            self.array_element_class_map.insert(name, nominal_type_id);
                        }
                    }
                }
                if self.type_annotation_is_callable(type_ann) {
                    self.callable_local_hints.insert(local_idx);
                    self.callable_symbol_hints.insert(name);
                }
                if self.type_is_dynamic_any_like(self.resolve_structural_slot_type_from_annotation(type_ann)) {
                    self.variable_class_map.remove(&name);
                    self.clear_late_bound_object_binding(name);
                }
            }
            else {
                self.dynamic_any_vars.remove(&name);
            }

            // No initializer - get type from annotation or UNRESOLVED
            let ty = decl
                .type_annotation
                .as_ref()
                .map(|t| self.resolve_type_annotation(t))
                .unwrap_or(UNRESOLVED);
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
                let typed_reg = Register {
                    id: refcell_reg.id,
                    ty: refcell_ty,
                };
                self.local_registers.insert(local_idx, typed_reg.clone());
                self.refcell_registers
                    .insert(local_idx, refcell_reg.clone());
                self.refcell_inner_types.insert(local_idx, UNRESOLVED);
                self.emit(IrInstr::StoreLocal {
                    index: local_idx,
                    value: refcell_reg,
                });
            } else {
                // Create a typed register for the local
                let typed_reg = Register {
                    id: null_reg.id,
                    ty,
                };
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

    fn lower_nested_function_decl(&mut self, func_decl: &ast::FunctionDecl) {
        use crate::parser::ast::{ArrowBody, ArrowFunction};
        use crate::parser::token::Span;

        // Build a synthetic ArrowFunction from the FunctionDecl
        let arrow = ArrowFunction {
            params: func_decl.params.clone(),
            body: ArrowBody::Block(func_decl.body.clone()),
            return_type: func_decl.return_type.clone(),
            is_async: func_decl.is_async,
            span: Span::new(0, 0, 0, 0),
        };

        // Lower as arrow (handles capture analysis, MakeClosure, etc.).
        // Std wrapper nested functions may have a pre-assigned function ID.
        let preassigned_func_id = self.function_id_for_decl(func_decl);
        if let Some(func_id) = preassigned_func_id {
            // Register before lowering body so recursive calls resolve through
            // direct-call lowering instead of falling back to non-callable locals.
            self.function_map.insert(func_decl.name.name, func_id);
        }
        let closure_reg = self.lower_arrow_with_preassigned_id(&arrow, preassigned_func_id);
        if let Some(func_id) = preassigned_func_id.or(self.last_arrow_func_id) {
            let declared_name = self.interner.resolve(func_decl.name.name).to_string();
            if let Some((_, ir_func)) = self
                .pending_arrow_functions
                .iter_mut()
                .find(|(id, _)| *id == func_id.as_u32())
            {
                // Preserve declared function names in stack traces/debug output.
                ir_func.name = declared_name;
            }
        }

        // Module-wrapper functions rely on sibling helper functions from class methods
        // (e.g., EnvNamespace.cwd() calling local `cwd()`), so expose wrapper-local
        // function declarations in function_map for direct identifier calls.
        let in_module_wrapper = self
            .current_function
            .as_ref()
            .is_some_and(|f| is_module_wrapper_function_name(&f.name));
        if in_module_wrapper {
            if let Some(func_id) = self.last_arrow_func_id {
                self.function_map.insert(func_decl.name.name, func_id);
                if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                    eprintln!(
                        "[lower] wrapper helper fn '{}' -> func_id={} preassigned={}",
                        self.interner.resolve(func_decl.name.name),
                        func_id.as_u32(),
                        preassigned_func_id
                            .map(|id| id.as_u32().to_string())
                            .unwrap_or_else(|| "none".to_string())
                    );
                }
            }
        }

        self.record_function_return_mappings(func_decl.name.name, func_decl.return_type.as_ref());

        // Assign to a local variable with the function's name
        let local_idx = self.allocate_local(func_decl.name.name);
        // Nested function declarations are callable closures; mark both the symbol
        // and local slot so captured/ancestor call lowering treats them as callable.
        self.callable_local_hints.insert(local_idx);
        self.callable_symbol_hints.insert(func_decl.name.name);
        self.local_registers.insert(local_idx, closure_reg.clone());
        self.emit(IrInstr::StoreLocal {
            index: local_idx,
            value: closure_reg,
        });

        // Async function declarations lowered through the closure path still need
        // closure-locals metadata so call lowering emits SpawnClosure, not CallClosure.
        if func_decl.is_async {
            if let Some(func_id) = preassigned_func_id.or(self.last_arrow_func_id) {
                if self.async_closures.contains(&func_id) {
                    self.closure_locals.insert(local_idx, func_id);
                }
            }
        }
    }

    fn lower_return(&mut self, ret: &ast::ReturnStatement) {
        let value = ret.value.as_ref().map(|e| self.lower_expr(e));

        // Inline finally blocks from innermost to outermost.
        // Drain the stack to prevent recursive re-inlining: if a finally block
        // itself contains a return, that nested lower_return sees an empty stack.
        let entries: Vec<super::TryFinallyEntry> = self.try_finally_stack.drain(..).rev().collect();
        for entry in &entries {
            if entry.in_try_body {
                self.emit(IrInstr::EndTry);
            }
            self.lower_block(&entry.finally_body);
            if self.current_block_is_terminated() {
                // Finally body contained its own return/throw — it takes precedence
                return;
            }
        }

        self.set_terminator(Terminator::Return(value));
    }

    fn lower_yield(&mut self, yld: &ast::YieldStatement) {
        if let Some(value) = &yld.value {
            self.lower_expr(value);
        }
        self.emit(IrInstr::Yield);
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

        // If condition is `a instanceof T`, temporarily update the narrowed runtime
        // view for `a` in the then-branch. Nominal targets use variable_class_map;
        // structural targets use variable_structural_projection_fields.
        let instanceof_nominal_save = if let ast::Expression::InstanceOf(inst) = &if_stmt.condition {
            if let ast::Expression::Identifier(ident) = &*inst.object {
                if let ast::types::Type::Reference(type_ref) = &inst.type_name.ty {
                    if let Some(&nominal_type_id) = self.class_map.get(&type_ref.name.name) {
                        let old = self.variable_class_map.insert(ident.name, nominal_type_id);
                        Some((ident.name, old))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let instanceof_shape_save = if let ast::Expression::InstanceOf(inst) = &if_stmt.condition {
            if let ast::Expression::Identifier(ident) = &*inst.object {
                let target_ty = self.resolve_structural_slot_type_from_annotation(&inst.type_name);
                let layout = self
                    .structural_projection_layout_from_type_id(target_ty)
                    .or_else(|| {
                        self.try_extract_object_alias_name_from_type(&inst.type_name)
                            .and_then(|alias_name| {
                                self.projected_structural_layout_from_alias_name(&alias_name)
                                    .map(|layout| {
                                        layout
                                            .into_iter()
                                            .map(|(field_name, field_idx)| {
                                                (field_name, field_idx as usize)
                                            })
                                            .collect::<Vec<(String, usize)>>()
                                    })
                            })
                    });
                if let Some(layout) = layout {
                    let old = self
                        .variable_structural_projection_fields
                        .insert(ident.name, layout);
                    Some((ident.name, old))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        // Lower then branch
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::new(then_block));
        self.current_block = then_block;
        self.lower_stmt(&if_stmt.then_branch);
        if !self.current_block_is_terminated() {
            self.set_terminator(Terminator::Jump(merge_block));
        }

        // Restore variable_class_map after then-branch
        if let Some((name, old_class)) = instanceof_nominal_save {
            if let Some(old) = old_class {
                self.variable_class_map.insert(name, old);
            } else {
                self.variable_class_map.remove(&name);
            }
        }
        if let Some((name, old_projection)) = instanceof_shape_save {
            if let Some(old) = old_projection {
                self.variable_structural_projection_fields.insert(name, old);
            } else {
                self.variable_structural_projection_fields.remove(&name);
            }
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
            .add_block(crate::ir::BasicBlock::with_label(
                header_block,
                "while.header",
            ));
        self.current_block = header_block;
        let cond = self.lower_expr(&while_stmt.condition);
        self.set_terminator(Terminator::Branch {
            cond,
            then_block: body_block,
            else_block: exit_block,
        });

        // Push loop context for break/continue
        self.loop_stack.push(super::LoopContext {
            label: self.pending_label.take(),
            break_target: exit_block,
            continue_target: header_block,
            try_finally_depth: self.try_finally_stack.len(),
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
        let mut loop_var_info: Option<(crate::parser::Symbol, u16)> = None;

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
            .add_block(crate::ir::BasicBlock::with_label(
                header_block,
                "for.header",
            ));
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
            label: self.pending_label.take(),
            break_target: exit_block,
            continue_target: update_block,
            try_finally_depth: self.try_finally_stack.len(),
        });

        // Body block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(body_block, "for.body"));
        self.current_block = body_block;

        // Per-iteration binding setup: if the loop variable is captured,
        // create a fresh RefCell for this iteration and copy the current value into it
        let original_refcell: Option<(u16, Register)> =
            if let Some((_sym, local_idx)) = &loop_var_info {
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
                    self.refcell_registers
                        .insert(*local_idx, iter_refcell.clone());
                    self.local_registers
                        .insert(*local_idx, iter_refcell.clone());
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
                self.refcell_registers
                    .insert(*local_idx, orig_refcell.clone());
                self.local_registers
                    .insert(*local_idx, orig_refcell.clone());
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
            .add_block(crate::ir::BasicBlock::with_label(
                update_block,
                "for.update",
            ));
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
        self.block_depth += 1;

        for stmt in &block.statements {
            self.lower_stmt(stmt);
            if self.current_block_is_terminated() {
                break;
            }
        }

        // Restore local_map to exit the block scope
        // This ensures outer variables are accessible again after the block
        self.local_map = saved_local_map;
        self.block_depth = self.block_depth.saturating_sub(1);
    }

    fn lower_break(&mut self, brk: &ast::BreakStatement) {
        // Labeled break: search loop stack for matching label
        if let Some(ref label_ident) = brk.label {
            let label_sym = label_ident.name;
            if let Some(loop_ctx) = self
                .loop_stack
                .iter()
                .rev()
                .find(|ctx| ctx.label == Some(label_sym))
                .cloned()
            {
                let depth = loop_ctx.try_finally_depth;
                let entries: Vec<super::TryFinallyEntry> =
                    self.try_finally_stack.drain(depth..).rev().collect();
                for entry in &entries {
                    if entry.in_try_body {
                        self.emit(IrInstr::EndTry);
                    }
                    self.lower_block(&entry.finally_body);
                    if self.current_block_is_terminated() {
                        return;
                    }
                }
                self.set_terminator(Terminator::Jump(loop_ctx.break_target));
                return;
            }
        }

        // Unlabeled break: if inside switch, target switch exit
        if let Some(&switch_exit) = self.switch_stack.last() {
            self.set_terminator(Terminator::Jump(switch_exit));
            return;
        }
        if let Some(loop_ctx) = self.loop_stack.last().cloned() {
            let depth = loop_ctx.try_finally_depth;
            let entries: Vec<super::TryFinallyEntry> =
                self.try_finally_stack.drain(depth..).rev().collect();
            for entry in &entries {
                if entry.in_try_body {
                    self.emit(IrInstr::EndTry);
                }
                self.lower_block(&entry.finally_body);
                if self.current_block_is_terminated() {
                    return;
                }
            }
            self.set_terminator(Terminator::Jump(loop_ctx.break_target));
        } else {
            self.set_terminator(Terminator::Unreachable);
        }
    }

    fn lower_continue(&mut self, cont: &ast::ContinueStatement) {
        // Find the target loop context (labeled or innermost)
        let loop_ctx = if let Some(ref label_ident) = cont.label {
            let label_sym = label_ident.name;
            self.loop_stack
                .iter()
                .rev()
                .find(|ctx| ctx.label == Some(label_sym))
                .cloned()
        } else {
            self.loop_stack.last().cloned()
        };

        if let Some(loop_ctx) = loop_ctx {
            let depth = loop_ctx.try_finally_depth;
            let entries: Vec<super::TryFinallyEntry> =
                self.try_finally_stack.drain(depth..).rev().collect();
            for entry in &entries {
                if entry.in_try_body {
                    self.emit(IrInstr::EndTry);
                }
                self.lower_block(&entry.finally_body);
                if self.current_block_is_terminated() {
                    return;
                }
            }
            self.set_terminator(Terminator::Jump(loop_ctx.continue_target));
        } else {
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

        // Push finally context so return/break/continue in try/catch bodies
        // will inline the finally block before exiting
        if let Some(finally_clause) = &try_stmt.finally_clause {
            self.try_finally_stack.push(super::TryFinallyEntry {
                finally_body: finally_clause.clone(),
                in_try_body: true,
            });
        }

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

        // Mark that we're now in the catch body (handler already consumed, no EndTry needed)
        if has_finally {
            if let Some(entry) = self.try_finally_stack.last_mut() {
                entry.in_try_body = false;
            }
        }

        // Create catch block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(catch_block, "catch"));
        self.current_block = catch_block;

        if let Some(catch_clause) = &try_stmt.catch_clause {
            // Bind the exception parameter if present
            // The VM pushes the exception value onto the stack when jumping to catch
            if let Some(ref param) = catch_clause.param {
                match param {
                    ast::Pattern::Identifier(ident) => {
                        // Simple identifier: pop exception directly into local
                        let local_idx = self.allocate_local(ident.name);
                        self.emit(IrInstr::PopToLocal { index: local_idx });
                        let exc_ty = TypeId::new(0); // Exception type (unknown)
                        let exc_reg = self.alloc_register(exc_ty);
                        self.local_registers.insert(local_idx, exc_reg);

                        // Add catch parameter to variable_class_map for method resolution
                        for (&symbol, &nominal_type_id) in &self.class_map {
                            if self.interner.resolve(symbol) == "Error" {
                                self.variable_class_map.insert(ident.name, nominal_type_id);
                                break;
                            }
                        }
                    }
                    _ => {
                        // Destructuring pattern: pop exception into temp local, load into register, then bind pattern
                        let exc_ty = TypeId::new(0); // Exception type (unknown)

                        // Use next local slot as temporary (don't add to local_map since it's a temp)
                        let temp_local = self.next_local;
                        self.next_local += 1;

                        // Pop exception into temporary local
                        self.emit(IrInstr::PopToLocal { index: temp_local });

                        // Load exception from local into register
                        let exc_reg = self.alloc_register(exc_ty);
                        self.emit(IrInstr::LoadLocal {
                            index: temp_local,
                            dest: exc_reg.clone(),
                        });

                        // Bind the pattern to the exception value
                        self.bind_pattern(param, exc_reg);
                    }
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

        // Pop the finally context before lowering the actual finally block
        // (the finally block should not see itself on the stack)
        if has_finally {
            self.try_finally_stack.pop();
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
        let entry_block = self.current_block;
        let exit_block = self.alloc_block();

        // Push switch exit so break inside case bodies targets this switch
        self.switch_stack.push(exit_block);

        // Pre-allocate all case blocks so we know the next block for fall-through
        let mut int_cases = Vec::new();
        let mut string_cases: Vec<(String, BasicBlockId)> = Vec::new();
        let mut default_block = None;
        let mut case_blocks = Vec::new();

        for case in &switch.cases {
            let case_block = self.alloc_block();
            case_blocks.push(case_block);

            if let Some(test) = &case.test {
                match test {
                    ast::Expression::IntLiteral(lit) => {
                        int_cases.push((lit.value as i32, case_block));
                    }
                    ast::Expression::StringLiteral(lit) => {
                        let resolved = self.interner.resolve(lit.value).to_string();
                        string_cases.push((resolved, case_block));
                    }
                    _ => {}
                }
            } else {
                default_block = Some(case_block);
            }
        }

        // Lower case bodies with fall-through support
        for (i, case) in switch.cases.iter().enumerate() {
            let case_block = case_blocks[i];
            self.current_function_mut()
                .add_block(crate::ir::BasicBlock::new(case_block));
            self.current_block = case_block;
            for stmt in &case.consequent {
                self.lower_stmt(stmt);
            }
            if !self.current_block_is_terminated() {
                // No break: fall through to next case body, or exit if last case
                let target = case_blocks.get(i + 1).copied().unwrap_or(exit_block);
                self.set_terminator(Terminator::Jump(target));
            }
        }

        self.switch_stack.pop();

        let default = default_block.unwrap_or(exit_block);

        if !string_cases.is_empty() {
            // String switch: emit if-else chain of string equality comparisons
            self.current_block = entry_block;
            for (string_val, target_block) in &string_cases {
                let const_reg = self.alloc_register(TypeId::new(1));
                self.emit(IrInstr::Assign {
                    dest: const_reg.clone(),
                    value: IrValue::Constant(IrConstant::String(string_val.clone())),
                });
                let eq_reg = self.alloc_register(TypeId::new(2));
                self.emit(IrInstr::StringCompare {
                    dest: eq_reg.clone(),
                    left: discriminant.clone(),
                    right: const_reg,
                    mode: StringCompareMode::Full,
                    negate: false,
                });
                let next_check = self.alloc_block();
                self.set_terminator(Terminator::Branch {
                    cond: eq_reg,
                    then_block: *target_block,
                    else_block: next_check,
                });
                self.current_function_mut()
                    .add_block(crate::ir::BasicBlock::new(next_check));
                self.current_block = next_check;
            }
            // After all string checks, jump to default
            self.set_terminator(Terminator::Jump(default));
        } else {
            // Integer switch: use Switch terminator
            self.current_block = entry_block;
            self.set_terminator(Terminator::Switch {
                value: discriminant,
                cases: int_cases,
                default,
            });
        }

        // Continue at exit block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::new(exit_block));
        self.current_block = exit_block;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::lower::{NominalTypeId, Lowerer};
    use crate::parser::{Parser, TypeContext};
    use crate::parser::types::ty::{ClassType, MethodSignature, PropertySignature, Type};

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

    #[test]
    fn imported_class_type_uses_structural_projection_layout() {
        let parser = Parser::new("let x = 1;").expect("lexer error");
        let (_module, mut interner) = parser.parse().expect("parse error");
        let class_name = interner.intern("RemoteBox");
        let mut type_ctx = TypeContext::new();
        let number_ty = type_ctx.number_type();
        let get_value_ty = type_ctx.function_type(vec![], number_ty, false);
        let remote_ty = type_ctx.intern(Type::Class(ClassType {
            name: "RemoteBox".to_string(),
            type_params: vec![],
            properties: vec![PropertySignature {
                name: "value".to_string(),
                ty: number_ty,
                optional: false,
                readonly: false,
                visibility: Default::default(),
            }],
            methods: vec![MethodSignature {
                name: "getValue".to_string(),
                ty: get_value_ty,
                type_params: vec![],
                visibility: Default::default(),
            }],
            static_properties: vec![],
            static_methods: vec![],
            extends: None,
            implements: vec![],
            is_abstract: false,
        }));
        let lowerer_type_ctx = Box::leak(Box::new(type_ctx));
        let interner = Box::leak(Box::new(interner));
        let mut lowerer = Lowerer::new(lowerer_type_ctx, interner);
        assert!(
            lowerer
                .structural_projection_layout_from_type_id(remote_ty)
                .is_some(),
            "late-bound/imported class public surface should project structurally"
        );

        lowerer.class_map.insert(class_name, NominalTypeId::new(1));
        assert!(
            lowerer
                .structural_projection_layout_from_type_id(remote_ty)
                .is_none(),
            "local concrete classes should stay nominal in lowering"
        );
    }
}
