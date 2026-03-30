//! Statement Lowering
//!
//! Converts AST statements to IR instructions.

use super::{
    is_module_wrapper_function_name, Lowerer, ARRAY_TYPE_ID, INT_TYPE_ID, UNKNOWN_TYPE_ID,
    UNRESOLVED, UNRESOLVED_TYPE_ID,
};
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
    String,
    ClassIterator,
    Unknown,
}

impl<'a> Lowerer<'a> {
    pub(super) fn emit_is_js_undefined(&mut self, value: Register) -> Register {
        let value_any = self.alloc_register(TypeId::new(UNKNOWN_TYPE_ID));
        self.emit(IrInstr::Assign {
            dest: value_any.clone(),
            value: IrValue::Register(value),
        });

        let undefined_reg = self.alloc_register(TypeId::new(UNKNOWN_TYPE_ID));
        self.emit(IrInstr::Assign {
            dest: undefined_reg.clone(),
            value: IrValue::Constant(crate::ir::IrConstant::Undefined),
        });

        let is_undefined = self.alloc_register(TypeId::new(super::BOOLEAN_TYPE_ID));
        self.emit(IrInstr::BinaryOp {
            dest: is_undefined.clone(),
            op: crate::ir::BinaryOp::StrictEqual,
            left: value_any,
            right: undefined_reg,
        });
        is_undefined
    }

    fn ensure_class_prototype_target(
        &mut self,
        class_value: &Register,
        prototype_value: &mut Option<Register>,
    ) -> Register {
        if let Some(prototype) = prototype_value.clone() {
            return prototype;
        }

        let prototype = self.alloc_register(TypeId::new(UNKNOWN_TYPE_ID));
        self.emit_dyn_get_named(prototype.clone(), class_value.clone(), "prototype");
        *prototype_value = Some(prototype.clone());
        prototype
    }

    fn emit_runtime_method_publication(
        &mut self,
        member_idx: usize,
        method: &ast::MethodDecl,
        runtime_method: &super::RuntimeClassMethodElement,
        class_value: &Register,
        prototype_value: &mut Option<Register>,
    ) {
        let target = if method.is_static {
            class_value.clone()
        } else {
            self.ensure_class_prototype_target(class_value, prototype_value)
        };

        let key_reg = self.lower_class_property_key(&runtime_method.key, member_idx);
        let func_id_reg = self.alloc_register(TypeId::new(INT_TYPE_ID));
        self.emit(IrInstr::Assign {
            dest: func_id_reg.clone(),
            value: IrValue::Constant(IrConstant::I32(runtime_method.func_id.as_u32() as i32)),
        });
        let kind_reg = self.alloc_register(TypeId::new(INT_TYPE_ID));
        let kind = match runtime_method.kind {
            ast::MethodKind::Normal => 0,
            ast::MethodKind::Getter => 1,
            ast::MethodKind::Setter => 2,
        };
        self.emit(IrInstr::Assign {
            dest: kind_reg.clone(),
            value: IrValue::Constant(IrConstant::I32(kind)),
        });

        self.emit(IrInstr::NativeCall {
            dest: None,
            native_id: crate::compiler::native_id::OBJECT_DEFINE_CLASS_PROPERTY,
            args: vec![target, key_reg, func_id_reg, kind_reg],
        });
    }

    fn emit_static_elements_for_class(
        &mut self,
        class: &ast::ClassDecl,
        nominal_type_id: crate::compiler::ir::NominalTypeId,
        class_value: Register,
    ) {
        let publish_runtime_instance_methods = self
            .class_info_map
            .get(&nominal_type_id)
            .is_some_and(|info| info.publish_runtime_instance_methods);
        let publish_runtime_static_methods = self
            .class_info_map
            .get(&nominal_type_id)
            .is_some_and(|info| info.publish_runtime_static_methods);
        let mut prototype_value = None;
        if self.js_this_binding_compat {
            if let Some(extends_expr) = &class.extends_expr {
                let parent_constructor = self.lower_expr(extends_expr);
                self.emit(IrInstr::NativeCall {
                    dest: None,
                    native_id: crate::compiler::native_id::OBJECT_SET_PROTOTYPE_OF,
                    args: vec![class_value.clone(), parent_constructor.clone()],
                });

                let class_prototype =
                    self.ensure_class_prototype_target(&class_value, &mut prototype_value);
                let parent_prototype = self.alloc_register(TypeId::new(UNKNOWN_TYPE_ID));
                self.emit_dyn_get_named(parent_prototype.clone(), parent_constructor, "prototype");
                self.emit(IrInstr::NativeCall {
                    dest: None,
                    native_id: crate::compiler::native_id::OBJECT_SET_PROTOTYPE_OF,
                    args: vec![class_prototype, parent_prototype],
                });
            }
        }
        for (member_idx, member) in class.members.iter().enumerate() {
            match member {
                ast::ClassMember::Method(method)
                    if method.body.is_some()
                        && ((method.is_static && publish_runtime_static_methods)
                            || (!method.is_static && publish_runtime_instance_methods)) =>
                {
                    let Some(runtime_method) = self
                        .class_info_map
                        .get(&nominal_type_id)
                        .and_then(|info| {
                            info.runtime_method_elements
                                .iter()
                                .find(|elem| elem.order == member_idx)
                        })
                        .cloned()
                    else {
                        continue;
                    };
                    self.emit_runtime_method_publication(
                        member_idx,
                        method,
                        &runtime_method,
                        &class_value,
                        &mut prototype_value,
                    );
                }
                ast::ClassMember::Field(field) if field.is_static => {
                    let Some(initializer) = &field.initializer else {
                        continue;
                    };
                    let field_symbol = self.known_class_member_symbol(&field.name);
                    let global_index = field_symbol.and_then(|field_symbol| {
                        self.class_info_map.get(&nominal_type_id).and_then(|info| {
                            info.static_fields
                                .iter()
                                .find(|static_field| static_field.name == field_symbol)
                                .map(|static_field| static_field.global_index)
                        })
                    });
                    let value_reg = self.lower_expr(initializer);
                    if let Some(global_index) = global_index {
                        self.emit(IrInstr::StoreGlobal {
                            index: global_index,
                            value: value_reg.clone(),
                        });
                    }

                    let key_reg = self.lower_class_property_key(&field.name, member_idx);
                    self.emit(IrInstr::NativeCall {
                        dest: None,
                        native_id: crate::compiler::native_id::REFLECT_SET,
                        args: vec![class_value.clone(), key_reg, value_reg],
                    });
                }
                ast::ClassMember::StaticBlock(block) => {
                    for stmt in &block.statements {
                        self.lower_stmt(stmt);
                    }
                }
                _ => {}
            }
        }
    }

    fn ensure_js_nested_class_binding(&mut self, name: crate::parser::Symbol) {
        if !self.js_this_binding_compat
            || self.function_depth == 0
            || self.local_map.contains_key(&name)
        {
            return;
        }

        let local_idx = self.allocate_local(name);
        let undefined = self.alloc_register(UNRESOLVED);
        self.emit(IrInstr::Assign {
            dest: undefined.clone(),
            value: IrValue::Constant(IrConstant::Undefined),
        });

        let refcell_reg = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::NewRefCell {
            dest: refcell_reg.clone(),
            initial_value: undefined,
        });
        self.local_registers.insert(local_idx, refcell_reg.clone());
        self.refcell_registers
            .insert(local_idx, refcell_reg.clone());
        self.refcell_inner_types.insert(local_idx, UNRESOLVED);
        self.emit(IrInstr::StoreLocal {
            index: local_idx,
            value: refcell_reg,
        });
    }

    fn existing_refcell_local(&self, local_idx: u16) -> Option<Register> {
        self.refcell_registers
            .get(&local_idx)
            .cloned()
            .or_else(|| self.local_registers.get(&local_idx).cloned())
    }

    fn lower_class_property_key(
        &mut self,
        key: &ast::PropertyKey,
        _fallback_idx: usize,
    ) -> Register {
        match key {
            ast::PropertyKey::Identifier(id) => {
                self.emit_named_key_register(self.interner.resolve(id.name))
            }
            ast::PropertyKey::StringLiteral(lit) => self.lower_string_literal(lit),
            ast::PropertyKey::IntLiteral(lit) => self.lower_int_literal(lit),
            ast::PropertyKey::Computed(ast::Expression::StringLiteral(lit)) => {
                self.lower_string_literal(lit)
            }
            ast::PropertyKey::Computed(ast::Expression::IntLiteral(lit)) => {
                self.lower_int_literal(lit)
            }
            ast::PropertyKey::Computed(ast::Expression::Parenthesized(expr)) => {
                self.lower_expr(&expr.expression)
            }
            ast::PropertyKey::Computed(expr) => self.lower_expr(expr),
        }
    }

    fn coerce_value_to_annotation_type(
        &mut self,
        value: Register,
        type_ann: &ast::TypeAnnotation,
    ) -> Register {
        let ann_ty = self.resolve_type_annotation(type_ann);
        if ann_ty.as_u32() == super::NUMBER_TYPE_ID && value.ty.as_u32() == super::INT_TYPE_ID {
            let zero = self.alloc_register(TypeId::new(super::NUMBER_TYPE_ID));
            self.emit(IrInstr::Assign {
                dest: zero.clone(),
                value: IrValue::Constant(IrConstant::F64(0.0)),
            });
            let dest = self.alloc_register(TypeId::new(super::NUMBER_TYPE_ID));
            self.emit(IrInstr::BinaryOp {
                dest: dest.clone(),
                op: BinaryOp::Add,
                left: value,
                right: zero,
            });
            return dest;
        }
        value
    }

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
        use crate::parser::types::{PrimitiveType, Type};

        let iterable_ty = self.get_expr_type(iterable);
        let Some(ty) = self.type_ctx.get(iterable_ty) else {
            return (ForOfIterableKind::Unknown, UNRESOLVED, None);
        };

        match ty {
            Type::Array(arr) => (ForOfIterableKind::Array, arr.element, None),
            Type::Primitive(PrimitiveType::String) | Type::StringLiteral(_) => (
                ForOfIterableKind::String,
                TypeId::new(super::STRING_TYPE_ID),
                None,
            ),
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

    fn nominal_type_id_by_name(
        &self,
        class_name: &str,
    ) -> Option<crate::compiler::ir::NominalTypeId> {
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
            Statement::With(with_stmt) => self.lower_with(with_stmt),
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
                self.ensure_js_nested_class_binding(class.name.name);
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
                    let saved_pending_method_env = self.pending_class_method_env_globals.take();
                    if self.current_function.is_some() {
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
                    let saved_immutable_bindings = self.immutable_bindings.clone();
                    let saved_captured_read_vars = self.captured_read_vars.clone();
                    let saved_next_local = self.next_local;
                    let saved_function = self.current_function.take();
                    let saved_current_block = self.current_block;
                    let saved_current_class = self.current_class.take();
                    let saved_this_register = self.this_register.take();
                    let saved_generator_yield_array_local = self.generator_yield_array_local.take();

                    self.lower_class_declaration(class);

                    // Restore per-function state
                    self.next_register = saved_register;
                    self.next_block = saved_block;
                    self.local_map = saved_local_map;
                    self.local_registers = saved_local_registers;
                    self.refcell_registers = saved_refcell_registers;
                    self.refcell_inner_types = saved_refcell_inner_types;
                    self.refcell_vars = saved_refcell_vars;
                    self.immutable_bindings = saved_immutable_bindings;
                    self.captured_read_vars = saved_captured_read_vars;
                    self.next_local = saved_next_local;
                    self.current_function = saved_function;
                    self.current_block = saved_current_block;
                    self.current_class = saved_current_class;
                    self.this_register = saved_this_register;
                    self.generator_yield_array_local = saved_generator_yield_array_local;
                    self.pending_class_method_env_globals = saved_pending_method_env;
                }

                let class_value = self.load_class_value_for_nominal_type(nominal_type_id);
                if self.js_this_binding_compat && self.function_depth == 0 && self.block_depth == 0
                {
                    if let Some(&global_idx) = self.js_script_lexical_globals.get(&class.name.name)
                    {
                        self.global_type_map.insert(global_idx, class_value.ty);
                        self.emit(IrInstr::StoreGlobal {
                            index: global_idx,
                            value: class_value.clone(),
                        });
                        self.mark_js_script_lexical_initialized(class.name.name);
                    }
                }
                if self.in_direct_eval_function {
                    self.emit_direct_eval_binding_declare_lexical(
                        self.interner.resolve(class.name.name),
                    );
                    self.emit_direct_eval_binding_set(
                        self.interner.resolve(class.name.name),
                        class_value.clone(),
                    );
                } else if self.js_this_binding_compat && self.function_depth > 0 {
                    self.store_identifier_value_at_span(
                        class.name.name,
                        class.name.span.start,
                        class_value.clone(),
                    );
                }

                self.emit_static_elements_for_class(class, nominal_type_id, class_value);
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
                let is_iteration = matches!(
                    labeled.body.as_ref(),
                    Statement::While(_)
                        | Statement::DoWhile(_)
                        | Statement::For(_)
                        | Statement::ForOf(_)
                        | Statement::ForIn(_)
                );
                if is_iteration {
                    // Set the pending label so the next loop picks it up.
                    self.pending_label = Some(labeled.label.name);
                    self.lower_stmt(&labeled.body);
                    self.pending_label = None;
                } else {
                    let exit_block = self.alloc_block();
                    self.label_stack.push(super::LabelContext {
                        label: labeled.label.name,
                        break_target: exit_block,
                        try_finally_depth: self.try_finally_stack.len(),
                    });
                    self.lower_stmt(&labeled.body);
                    self.label_stack.pop();
                    if !self.current_block_is_terminated() {
                        self.set_terminator(Terminator::Jump(exit_block));
                    }
                    self.current_function_mut()
                        .add_block(crate::ir::BasicBlock::with_label(exit_block, "label.exit"));
                    self.current_block = exit_block;
                }
            }
        }
    }

    fn lower_with(&mut self, with_stmt: &ast::WithStatement) {
        let object = self.lower_expr(&with_stmt.object);
        self.emit(IrInstr::NativeCall {
            dest: None,
            native_id: crate::compiler::native_id::OBJECT_PUSH_WITH_ENV,
            args: vec![object],
        });
        self.lower_stmt(&with_stmt.body);
        if !self.current_block_is_terminated() {
            self.emit(IrInstr::NativeCall {
                dest: None,
                native_id: crate::compiler::native_id::OBJECT_POP_WITH_ENV,
                args: vec![],
            });
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
            iterator_record: None,
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

    fn loop_scope_plan(&self, span_start: usize) -> Option<crate::semantics::LoopScopePlan> {
        self.semantic_plan.loop_scope_plan_at_span(span_start).cloned()
    }

    fn with_runtime_loop_declaration_bindings<T>(
        &mut self,
        binding_names: &[String],
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let saved = std::mem::take(&mut self.active_runtime_declaration_bindings);
        self.active_runtime_declaration_bindings = saved.clone();
        for binding_name in binding_names {
            self.active_runtime_declaration_bindings
                .insert(binding_name.clone());
        }
        let result = f(self);
        self.active_runtime_declaration_bindings = saved;
        result
    }

    fn declare_runtime_loop_bindings(&mut self, binding_names: &[String]) {
        self.emit_push_declarative_env();
        for binding_name in binding_names {
            self.emit_direct_eval_binding_declare_lexical(binding_name);
        }
    }

    fn lower_runtime_loop_initializer_decl(
        &mut self,
        decl: &ast::VariableDecl,
        binding_names: &[String],
    ) {
        self.with_runtime_loop_declaration_bindings(binding_names, |this| {
            let value = if let Some(init) = &decl.initializer {
                this.lower_expr(init)
            } else {
                let undefined = this.alloc_register(UNRESOLVED);
                this.emit(IrInstr::Assign {
                    dest: undefined.clone(),
                    value: crate::ir::IrValue::Constant(crate::ir::IrConstant::Undefined),
                });
                undefined
            };
            this.bind_pattern(&decl.pattern, value);
        });
    }

    fn lower_for_of(&mut self, for_of: &ast::ForOfStatement) {
        let loop_plan = self.loop_scope_plan(for_of.span.start);
        let runtime_loop_env = self.js_this_binding_compat
            && loop_plan
                .as_ref()
                .is_some_and(|plan| plan.creates_per_iteration_env && !plan.binding_names.is_empty());
        let runtime_loop_binding_names = loop_plan
            .as_ref()
            .map(|plan| plan.binding_names.clone())
            .unwrap_or_default();

        if runtime_loop_env {
            self.declare_runtime_loop_bindings(&runtime_loop_binding_names);
        }

        let (_, elem_ty, _) = self.classify_for_of_iterable(&for_of.right);
        let iterable_reg = self.lower_expr(&for_of.right);
        let iterator_reg = self.emit_iterator_get_helper(iterable_reg);

        let header_block = self.alloc_block();
        let body_block = self.alloc_block();
        let next_iter_block = self.alloc_block();
        let exit_block = self.alloc_block();

        self.set_terminator(Terminator::Jump(header_block));

        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(
                header_block,
                "forof.header",
            ));
        self.current_block = header_block;

        let step_result = self.emit_iterator_step_helper(iterator_reg.clone());
        self.set_terminator(Terminator::BranchIfNull {
            value: step_result.clone(),
            null_block: exit_block,
            not_null_block: body_block,
        });

        self.loop_stack.push(super::LoopContext {
            label: self.pending_label.take(),
            break_target: exit_block,
            continue_target: if runtime_loop_env {
                next_iter_block
            } else {
                header_block
            },
            iterator_record: Some(iterator_reg.clone()),
            try_finally_depth: self.try_finally_stack.len(),
        });

        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(body_block, "forof.body"));
        self.current_block = body_block;

        let raw_elem_reg = self.emit_iterator_value_helper(step_result);
        let elem_reg = if elem_ty.as_u32() == UNRESOLVED_TYPE_ID {
            raw_elem_reg
        } else {
            let typed_elem = self.alloc_register(elem_ty);
            self.emit(IrInstr::Assign {
                dest: typed_elem.clone(),
                value: crate::ir::IrValue::Register(raw_elem_reg),
            });
            typed_elem
        };

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

        // Bind the loop variable (supports destructuring patterns)

        match &for_of.left {
            ast::ForOfLeft::VariableDecl(decl) => {
                if runtime_loop_env {
                    self.with_runtime_loop_declaration_bindings(
                        &runtime_loop_binding_names,
                        |this| this.bind_pattern(&decl.pattern, elem_reg),
                    );
                } else {
                    self.bind_pattern(&decl.pattern, elem_reg);
                }
            }
            ast::ForOfLeft::Pattern(pattern) => match pattern {
                ast::Pattern::Identifier(ident) => {
                    if runtime_loop_env
                        && runtime_loop_binding_names
                            .iter()
                            .any(|binding| binding == self.interner.resolve(ident.name))
                    {
                        self.with_runtime_loop_declaration_bindings(
                            &runtime_loop_binding_names,
                            |this| this.bind_pattern(pattern, elem_reg),
                        );
                    } else if let Some(local_idx) = self.lookup_local(ident.name) {
                        self.emit(IrInstr::StoreLocal {
                            index: local_idx,
                            value: elem_reg,
                        });
                    }
                }
                _ => {
                    if runtime_loop_env {
                        self.with_runtime_loop_declaration_bindings(
                            &runtime_loop_binding_names,
                            |this| this.bind_pattern(pattern, elem_reg),
                        );
                    } else {
                        self.bind_pattern(pattern, elem_reg);
                    }
                }
            },
        }

        // Lower the body
        self.lower_stmt(&for_of.body);

        if !self.current_block_is_terminated() {
            self.set_terminator(Terminator::Jump(if runtime_loop_env {
                next_iter_block
            } else {
                header_block
            }));
        }

        self.loop_stack.pop();

        if runtime_loop_env {
            self.current_function_mut()
                .add_block(crate::ir::BasicBlock::with_label(
                    next_iter_block,
                    "forof.next",
                ));
            self.current_block = next_iter_block;
            self.emit_replace_declarative_env(&runtime_loop_binding_names);
            self.set_terminator(Terminator::Jump(header_block));
        }

        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(exit_block, "forof.exit"));
        self.current_block = exit_block;
        if runtime_loop_env {
            self.emit_pop_declarative_env();
        }
    }

    fn lower_for_in(&mut self, for_in: &ast::ForInStatement) {
        // For-in loops are desugared to:
        //   let _keys = Reflect.getEnumerableKeys(obj);
        //   let _idx = 0;
        //   let _len = _keys.length;
        //   while (_idx < _len) {
        //       let key = _keys[_idx];
        //       body;
        //       _idx = _idx + 1;
        //   }

        let number_ty = TypeId::new(2); // number type
        let string_ty = TypeId::new(1); // string type

        let loop_plan = self.loop_scope_plan(for_in.span.start);
        let runtime_loop_env = self.js_this_binding_compat
            && loop_plan
                .as_ref()
                .is_some_and(|plan| plan.creates_per_iteration_env && !plan.binding_names.is_empty());
        let runtime_loop_binding_names = loop_plan
            .as_ref()
            .map(|plan| plan.binding_names.clone())
            .unwrap_or_default();

        if runtime_loop_env {
            self.declare_runtime_loop_bindings(&runtime_loop_binding_names);
        }

        // Evaluate the object expression
        let obj_reg = self.lower_expr(&for_in.right);

        // Call Reflect.getEnumerableKeys(obj) to get keys array
        let keys_reg = self.alloc_register(UNRESOLVED);
        self.emit(IrInstr::NativeCall {
            dest: Some(keys_reg.clone()),
            native_id: crate::vm::builtin::reflect::GET_ENUMERABLE_KEYS,
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
            iterator_record: None,
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
                if runtime_loop_env {
                    self.with_runtime_loop_declaration_bindings(
                        &runtime_loop_binding_names,
                        |this| this.bind_pattern(&decl.pattern, key_reg),
                    );
                } else {
                    self.bind_pattern(&decl.pattern, key_reg);
                }
            }
            ast::ForOfLeft::Pattern(pattern) => match pattern {
                ast::Pattern::Identifier(ident) => {
                    if runtime_loop_env
                        && runtime_loop_binding_names
                            .iter()
                            .any(|binding| binding == self.interner.resolve(ident.name))
                    {
                        self.with_runtime_loop_declaration_bindings(
                            &runtime_loop_binding_names,
                            |this| this.bind_pattern(pattern, key_reg),
                        );
                    } else if let Some(local_idx) = self.lookup_local(ident.name) {
                        self.emit(IrInstr::StoreLocal {
                            index: local_idx,
                            value: key_reg,
                        });
                    }
                }
                _ => {
                    if runtime_loop_env {
                        self.with_runtime_loop_declaration_bindings(
                            &runtime_loop_binding_names,
                            |this| this.bind_pattern(pattern, key_reg),
                        );
                    } else {
                        self.bind_pattern(pattern, key_reg);
                    }
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

        if runtime_loop_env {
            self.emit_replace_declarative_env(&runtime_loop_binding_names);
        }

        // Jump back to header
        self.set_terminator(Terminator::Jump(header_block));

        // Exit block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(exit_block, "forin.exit"));
        self.current_block = exit_block;
        if runtime_loop_env {
            self.emit_pop_declarative_env();
        }
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

    pub(super) fn emit_require_object_coercible(&mut self, value: Register) {
        self.emit(IrInstr::NativeCall {
            dest: None,
            native_id: crate::compiler::native_id::OBJECT_REQUIRE_OBJECT_COERCIBLE,
            args: vec![value],
        });
    }

    fn is_anonymous_binding_initializer(&self, expr: &ast::Expression) -> bool {
        match expr {
            ast::Expression::Arrow(_) => true,
            ast::Expression::Function(func) => func.name.is_none(),
            ast::Expression::Call(call) if call.arguments.is_empty() => {
                let ast::Expression::Function(func) = call.callee.as_ref() else {
                    return false;
                };
                if func.name.is_some() || !func.params.is_empty() {
                    return false;
                }
                let [ast::Statement::ClassDecl(class_decl), ast::Statement::Return(ret)] =
                    func.body.statements.as_slice()
                else {
                    return false;
                };
                let Some(ast::Expression::Identifier(ret_ident)) = ret.value.as_ref() else {
                    return false;
                };
                self.interner
                    .resolve(class_decl.name.name)
                    .starts_with("__class_expr_")
                    && ret_ident.name == class_decl.name.name
            }
            ast::Expression::Parenthesized(paren) => {
                self.is_anonymous_binding_initializer(&paren.expression)
            }
            _ => false,
        }
    }

    fn property_key_is_static_name(&self, key: &ast::PropertyKey) -> bool {
        match key {
            ast::PropertyKey::Identifier(ident) => self.interner.resolve(ident.name) == "name",
            ast::PropertyKey::StringLiteral(lit) => self.interner.resolve(lit.value) == "name",
            ast::PropertyKey::Computed(ast::Expression::StringLiteral(lit)) => {
                self.interner.resolve(lit.value) == "name"
            }
            _ => false,
        }
    }

    fn anonymous_initializer_declares_own_name(&self, expr: &ast::Expression) -> bool {
        let ast::Expression::Call(call) = expr else {
            return false;
        };
        if !call.arguments.is_empty() {
            return false;
        }
        let ast::Expression::Function(func) = call.callee.as_ref() else {
            return false;
        };
        let [ast::Statement::ClassDecl(class_decl), ast::Statement::Return(_)] =
            func.body.statements.as_slice()
        else {
            return false;
        };

        class_decl.members.iter().any(|member| match member {
            ast::ClassMember::Field(field) => {
                field.is_static && self.property_key_is_static_name(&field.name)
            }
            ast::ClassMember::Method(method) => {
                method.is_static && self.property_key_is_static_name(&method.name)
            }
            _ => false,
        })
    }

    pub(super) fn maybe_assign_anonymous_binding_name(
        &mut self,
        pattern: &ast::Pattern,
        initializer: &ast::Expression,
        value: &Register,
    ) {
        let ast::Pattern::Identifier(ident) = pattern else {
            return;
        };
        if !self.is_anonymous_binding_initializer(initializer) {
            return;
        }
        if self.anonymous_initializer_declares_own_name(initializer) {
            return;
        }

        let binding_name = self.emit_named_key_register(self.interner.resolve(ident.name));
        self.emit(IrInstr::NativeCall {
            dest: None,
            native_id: crate::compiler::native_id::OBJECT_ASSIGN_BINDING_NAME_IF_MISSING,
            args: vec![value.clone(), binding_name],
        });
    }

    fn emit_binding_default_value(
        &mut self,
        pattern: &ast::Pattern,
        current_value: Register,
        default_expr: &ast::Expression,
    ) -> Register {
        let present_block = self.alloc_block();
        let default_block = self.alloc_block();
        let merge_block = self.alloc_block();
        let final_val = self.alloc_register(TypeId::new(0));
        let is_undefined = self.emit_is_js_undefined(current_value.clone());
        self.set_terminator(Terminator::Branch {
            cond: is_undefined,
            then_block: default_block,
            else_block: present_block,
        });

        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(
                present_block,
                "destr.present",
            ));
        self.current_block = present_block;
        self.emit(IrInstr::Assign {
            dest: final_val.clone(),
            value: IrValue::Register(current_value),
        });
        self.set_terminator(Terminator::Jump(merge_block));

        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(
                default_block,
                "destr.default",
            ));
        self.current_block = default_block;
        let default_val = self.lower_expr(default_expr);
        self.maybe_assign_anonymous_binding_name(pattern, default_expr, &default_val);
        self.emit(IrInstr::Assign {
            dest: final_val.clone(),
            value: IrValue::Register(default_val),
        });
        self.set_terminator(Terminator::Jump(merge_block));

        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(
                merge_block,
                "destr.merge",
            ));
        self.current_block = merge_block;
        final_val
    }

    pub(super) fn emit_destructuring_property_load(
        &mut self,
        object: Register,
        key: Register,
        ty: TypeId,
    ) -> Register {
        let loaded = self.alloc_register(ty);
        self.emit(IrInstr::NativeCall {
            dest: Some(loaded.clone()),
            native_id: crate::compiler::native_id::OBJECT_GET_DESTRUCTURING_PROPERTY,
            args: vec![object, key],
        });
        loaded
    }

    pub(super) fn emit_property_key_coercion(&mut self, key: Register) -> Register {
        let coerced = self.alloc_register(TypeId::new(super::STRING_TYPE_ID));
        self.emit(IrInstr::NativeCall {
            dest: Some(coerced.clone()),
            native_id: crate::compiler::native_id::OBJECT_COERCE_PROPERTY_KEY,
            args: vec![key],
        });
        coerced
    }

    pub fn bind_pattern(&mut self, pattern: &ast::Pattern, value_reg: Register) {
        match pattern {
            ast::Pattern::Identifier(ident) => {
                if self
                    .active_runtime_declaration_bindings
                    .contains(self.interner.resolve(ident.name))
                {
                    let binding = super::ResolvedBinding::RuntimeIdentifier {
                        env: self.env_handle_for_binding(false, false, false),
                        symbol: ident.name,
                    };
                    let _ = self.emit_store_identifier_binding(binding, value_reg);
                    return;
                }

                // Module-top-level bindings must use globals so module functions can see them.
                if self.function_depth == 0 && self.block_depth == 0 {
                    if self.js_this_binding_compat {
                        if let Some(&global_idx) = self.js_script_lexical_globals.get(&ident.name) {
                            self.global_type_map.insert(global_idx, value_reg.ty);
                            if let Some(fields) =
                                self.register_object_fields.get(&value_reg.id).cloned()
                            {
                                self.variable_object_fields.insert(ident.name, fields);
                                let nested_fields: FxHashMap<u16, Vec<(String, usize)>> = self
                                    .register_nested_object_fields
                                    .iter()
                                    .filter_map(|(&(obj_reg, field_idx), layout)| {
                                        (obj_reg == value_reg.id)
                                            .then_some((field_idx, layout.clone()))
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

                let reuses_preallocated_capture = self.js_this_binding_compat
                    && self.block_depth == 0
                    && self.refcell_vars.contains(&ident.name)
                    && self.local_map.contains_key(&ident.name);
                let reuses_preallocated_parameter =
                    self.parameter_binding_mode && self.local_map.contains_key(&ident.name);
                let local_idx = if reuses_preallocated_capture || reuses_preallocated_parameter {
                    self.lookup_local(ident.name)
                        .expect("preallocated local must exist")
                } else {
                    self.allocate_local(ident.name)
                };
                if self.refcell_vars.contains(&ident.name) {
                    if let Some(existing_refcell) = self.existing_refcell_local(local_idx) {
                        self.emit(IrInstr::StoreRefCell {
                            refcell: existing_refcell,
                            value: value_reg.clone(),
                        });
                        self.refcell_inner_types.insert(local_idx, value_reg.ty);
                    } else {
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
                    }
                } else {
                    self.local_registers.insert(local_idx, value_reg.clone());
                    self.emit(IrInstr::StoreLocal {
                        index: local_idx,
                        value: value_reg,
                    });
                }
            }
            ast::Pattern::Array(array_pat) => {
                if array_pat.elements.is_empty() && array_pat.rest.is_none() {
                    return;
                }
                let element_layout_hint = self
                    .register_array_element_object_fields
                    .get(&value_reg.id)
                    .cloned()
                    .or_else(|| self.array_element_object_layout_from_type(value_reg.ty));
                let iterator_reg = self.emit_iterator_get_helper(value_reg.clone());
                let mut last_step_result: Option<Register> = None;
                for (i, elem_opt) in array_pat.elements.iter().enumerate() {
                    let step_result = self.emit_iterator_step_helper(iterator_reg.clone());
                    last_step_result = Some(step_result.clone());

                    if elem_opt.is_none() {
                        continue;
                    }

                    let elem = elem_opt.as_ref().expect("checked some");
                    if let Some(default_expr) = &elem.default {
                        let value_block = self.alloc_block();
                        let default_block = self.alloc_block();
                        let merge_block = self.alloc_block();
                        let final_val = self.alloc_register(TypeId::new(0));

                        self.set_terminator(Terminator::BranchIfNull {
                            value: step_result.clone(),
                            null_block: default_block,
                            not_null_block: value_block,
                        });

                        self.current_function_mut()
                            .add_block(crate::ir::BasicBlock::with_label(
                                value_block,
                                "destr.iter.value",
                            ));
                        self.current_block = value_block;
                        let elem_reg = self.emit_iterator_value_helper(step_result);
                        let present_block = self.alloc_block();
                        let is_undefined = self.emit_is_js_undefined(elem_reg.clone());
                        self.set_terminator(Terminator::Branch {
                            cond: is_undefined,
                            then_block: default_block,
                            else_block: present_block,
                        });

                        self.current_function_mut()
                            .add_block(crate::ir::BasicBlock::with_label(
                                present_block,
                                "destr.iter.hasval",
                            ));
                        self.current_block = present_block;
                        self.emit(IrInstr::Assign {
                            dest: final_val.clone(),
                            value: IrValue::Register(elem_reg),
                        });
                        self.set_terminator(Terminator::Jump(merge_block));

                        self.current_function_mut()
                            .add_block(crate::ir::BasicBlock::with_label(
                                default_block,
                                "destr.iter.default",
                            ));
                        self.current_block = default_block;
                        let default_val = self.lower_expr(default_expr);
                        self.maybe_assign_anonymous_binding_name(
                            &elem.pattern,
                            default_expr,
                            &default_val,
                        );
                        self.emit(IrInstr::Assign {
                            dest: final_val.clone(),
                            value: IrValue::Register(default_val),
                        });
                        self.set_terminator(Terminator::Jump(merge_block));

                        self.current_function_mut()
                            .add_block(crate::ir::BasicBlock::with_label(
                                merge_block,
                                "destr.iter.merge",
                            ));
                        self.current_block = merge_block;
                        self.bind_pattern(&elem.pattern, final_val);
                    } else {
                        let value_block = self.alloc_block();
                        let done_block = self.alloc_block();
                        let merge_block = self.alloc_block();
                        let final_val = self.alloc_register(TypeId::new(0));

                        self.set_terminator(Terminator::BranchIfNull {
                            value: step_result.clone(),
                            null_block: done_block,
                            not_null_block: value_block,
                        });

                        self.current_function_mut()
                            .add_block(crate::ir::BasicBlock::with_label(
                                value_block,
                                "destr.iter.load",
                            ));
                        self.current_block = value_block;
                        let elem_reg = self.emit_iterator_value_helper(step_result);
                        self.emit(IrInstr::Assign {
                            dest: final_val.clone(),
                            value: IrValue::Register(elem_reg),
                        });
                        self.set_terminator(Terminator::Jump(merge_block));

                        self.current_function_mut()
                            .add_block(crate::ir::BasicBlock::with_label(
                                done_block,
                                "destr.iter.done",
                            ));
                        self.current_block = done_block;
                        self.emit(IrInstr::Assign {
                            dest: final_val.clone(),
                            value: IrValue::Constant(crate::ir::IrConstant::Undefined),
                        });
                        self.set_terminator(Terminator::Jump(merge_block));

                        self.current_function_mut()
                            .add_block(crate::ir::BasicBlock::with_label(
                                merge_block,
                                "destr.iter.bound",
                            ));
                        self.current_block = merge_block;
                        if let Some(layout) = &element_layout_hint {
                            self.register_object_fields
                                .insert(final_val.id, layout.clone());
                        }
                        self.bind_pattern(&elem.pattern, final_val);
                    }
                }

                // Handle rest pattern by draining the remaining iterator values.
                if let Some(rest_pat) = &array_pat.rest {
                    let zero = self.emit_i32_const(0);
                    let rest_arr = self.alloc_register(TypeId::new(super::ARRAY_TYPE_ID));
                    self.emit(IrInstr::NewArray {
                        dest: rest_arr.clone(),
                        len: zero,
                        elem_ty: TypeId::new(0),
                    });

                    let header = self.alloc_block();
                    let body = self.alloc_block();
                    let exit = self.alloc_block();

                    self.set_terminator(Terminator::Jump(header));

                    self.current_function_mut()
                        .add_block(crate::ir::BasicBlock::with_label(header, "rest.hdr"));
                    self.current_block = header;
                    let step_result = self.emit_iterator_step_helper(iterator_reg.clone());
                    self.set_terminator(Terminator::BranchIfNull {
                        value: step_result.clone(),
                        null_block: exit,
                        not_null_block: body,
                    });

                    self.current_function_mut()
                        .add_block(crate::ir::BasicBlock::with_label(body, "rest.body"));
                    self.current_block = body;
                    let elem = self.emit_iterator_value_helper(step_result);
                    self.emit(IrInstr::ArrayPush {
                        array: rest_arr.clone(),
                        element: elem,
                    });
                    self.set_terminator(Terminator::Jump(header));

                    self.current_function_mut()
                        .add_block(crate::ir::BasicBlock::with_label(exit, "rest.exit"));
                    self.current_block = exit;

                    self.bind_pattern(rest_pat, rest_arr);
                } else if let Some(step_result) = last_step_result {
                    let close_block = self.alloc_block();
                    let exit_block = self.alloc_block();
                    self.set_terminator(Terminator::BranchIfNull {
                        value: step_result,
                        null_block: exit_block,
                        not_null_block: close_block,
                    });

                    self.current_function_mut()
                        .add_block(crate::ir::BasicBlock::with_label(close_block, "iter.close"));
                    self.current_block = close_block;
                    self.emit_iterator_close_helper(iterator_reg.clone());
                    self.set_terminator(Terminator::Jump(exit_block));

                    self.current_function_mut()
                        .add_block(crate::ir::BasicBlock::with_label(exit_block, "iter.done"));
                    self.current_block = exit_block;
                }
            }
            ast::Pattern::Object(obj_pat) => {
                self.emit_require_object_coercible(value_reg.clone());
                // Object destructuring
                // Prefer statically known field layout; otherwise use Reflect.get by property name.
                let mut excluded_keys = Vec::new();
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
                    let key_reg = match &property.key {
                        ast::PropertyKey::Identifier(id) => {
                            self.emit_named_key_register(self.interner.resolve(id.name))
                        }
                        ast::PropertyKey::StringLiteral(lit) => {
                            self.emit_named_key_register(self.interner.resolve(lit.value))
                        }
                        ast::PropertyKey::IntLiteral(lit) => {
                            let key_reg = self.alloc_register(TypeId::new(super::STRING_TYPE_ID));
                            self.emit(IrInstr::Assign {
                                dest: key_reg.clone(),
                                value: IrValue::Constant(IrConstant::String(lit.value.to_string())),
                            });
                            key_reg
                        }
                        ast::PropertyKey::Computed(expr) => {
                            let raw_key = self.lower_expr(expr);
                            self.emit_property_key_coercion(raw_key)
                        }
                    };
                    excluded_keys.push(key_reg.clone());

                    if self.js_this_binding_compat {
                        if let ast::Pattern::Identifier(ident) = &property.value {
                            let _ =
                                self.emit_direct_eval_binding_has(self.interner.resolve(ident.name));
                        }
                    }

                    let static_prop_name = match &property.key {
                        ast::PropertyKey::Identifier(id) => {
                            Some(self.interner.resolve(id.name).to_string())
                        }
                        ast::PropertyKey::StringLiteral(lit) => {
                            Some(self.interner.resolve(lit.value).to_string())
                        }
                        ast::PropertyKey::IntLiteral(lit) => Some(lit.value.to_string()),
                        ast::PropertyKey::Computed(_) => None,
                    };
                    let inferred_field_ty = static_prop_name
                        .as_ref()
                        .and_then(|prop_name| {
                            self.object_property_type_from_value_type(value_reg.ty, prop_name)
                        })
                        .unwrap_or(UNRESOLVED);
                    let destructuring_field_ty = UNRESOLVED;

                    let field_reg = if property.default.is_none() {
                        if let (Some(layout), Some(prop_name)) =
                            (field_layout.as_ref(), static_prop_name.as_ref())
                        {
                        // Statically known layout: use direct field slot when present.
                            let Some(field_index) = layout
                                .iter()
                                .find(|(name, _)| name == prop_name)
                                .map(|(_, idx)| *idx as u16)
                            else {
                                if let Some(default_expr) = &property.default {
                                    let default_val = self.lower_expr(default_expr);
                                    self.maybe_assign_anonymous_binding_name(
                                        &property.value,
                                        default_expr,
                                        &default_val,
                                    );
                                    self.bind_pattern(&property.value, default_val);
                                } else {
                                    let undefined_reg = self.alloc_register(TypeId::new(0));
                                    self.emit(IrInstr::Assign {
                                        dest: undefined_reg.clone(),
                                        value: IrValue::Constant(crate::ir::IrConstant::Undefined),
                                    });
                                    self.bind_pattern(&property.value, undefined_reg);
                                }
                                continue;
                            };
                            let loaded = self.alloc_register(destructuring_field_ty);
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
                            // Dynamic layout or computed key: read through keyed property access.
                            self.emit_destructuring_property_load(
                                value_reg.clone(),
                                key_reg.clone(),
                                destructuring_field_ty,
                            )
                        }
                    } else {
                        self.emit_destructuring_property_load(
                            value_reg.clone(),
                            key_reg.clone(),
                            destructuring_field_ty,
                        )
                    };

                    // Handle default values
                    if let Some(default_expr) = &property.default {
                        let final_val = self.emit_binding_default_value(
                            &property.value,
                            field_reg,
                            default_expr,
                        );
                        self.bind_pattern(&property.value, final_val);
                    } else {
                        self.bind_pattern(&property.value, field_reg);
                    }
                }

                if let Some(rest_ident) = &obj_pat.rest {
                    let rest_obj = self.alloc_register(TypeId::new(super::NUMBER_TYPE_ID));
                    let type_index = crate::vm::object::layout_id_from_ordered_names(&[]);
                    self.module_structural_layouts
                        .entry(type_index)
                        .or_insert_with(Vec::new);
                    self.emit(IrInstr::ObjectLiteral {
                        dest: rest_obj.clone(),
                        type_index,
                        fields: vec![],
                    });
                    self.register_object_fields.insert(rest_obj.id, Vec::new());

                    let mut args = Vec::with_capacity(excluded_keys.len() + 2);
                    args.push(rest_obj.clone());
                    args.push(value_reg.clone());
                    args.extend(excluded_keys);
                    self.emit(IrInstr::NativeCall {
                        dest: Some(rest_obj.clone()),
                        native_id:
                            crate::compiler::native_id::OBJECT_COPY_DATA_PROPERTIES_EXCLUDING,
                        args,
                    });
                    self.bind_pattern(&ast::Pattern::Identifier(rest_ident.clone()), rest_obj);
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
            crate::parser::types::Type::Object(_) | crate::parser::types::Type::Interface(_) => {
                true
            }
            crate::parser::types::Type::Class(class_ty) => {
                self.nominal_type_id_from_type_name(&class_ty.name)
                    .is_none()
                    && !self.type_registry.has_builtin_dispatch_type(&class_ty.name)
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
            self.projection_layout_cache.remove(&name);
            return;
        }

        let ty = self.resolve_structural_slot_type_from_annotation(type_ann);
        if let Some(layout) = self.structural_projection_layout_from_type_id(ty) {
            self.projection_layout_cache
                .insert(name, layout);
        } else {
            self.projection_layout_cache.remove(&name);
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
                if self
                    .try_extract_class_from_type(&cast.target_type)
                    .is_some()
                {
                    None
                } else {
                    let target_ty =
                        self.resolve_structural_slot_type_from_annotation(&cast.target_type);
                    self.structural_projection_layout_from_type_id(target_ty)
                }
            }
            _ => None,
        };

        if let Some(layout) = projected_layout {
            self.projection_layout_cache
                .insert(name, layout);
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
        _args: &[ast::CallArgument],
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
                .cloned(),
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
        let is_js_lexical = self.js_this_binding_compat
            && decl.kind != crate::parser::ast::VariableKind::Var;
        if decl.kind == crate::parser::ast::VariableKind::Const {
            super::collect_pattern_names(&decl.pattern, &mut self.immutable_bindings);
        }

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
        self.nominal_binding_cache.remove(&name);
        self.array_element_nominal_cache.remove(&name);
        self.variable_object_fields.remove(&name);
        self.variable_nested_object_fields.remove(&name);
        self.variable_object_type_aliases.remove(&name);
        self.projection_layout_cache.remove(&name);
        self.task_result_type_aliases.remove(&name);
        self.callable_symbol_hints.remove(&name);
        self.clear_runtime_dispatch_hint(name);

        if self.in_direct_eval_function && decl.kind == crate::parser::ast::VariableKind::Var {
            if let Some(init) = &decl.initializer {
                let mut value =
                    self.lower_expr_with_object_spread_filter(init, decl.type_annotation.as_ref());
                if let Some(type_ann) = &decl.type_annotation {
                    value = self.coerce_value_to_annotation_type(value, type_ann);
                }
                self.emit_direct_eval_binding_set(self.interner.resolve(name), value);
            }
            return;
        }

        if self.in_direct_eval_function && decl.kind != crate::parser::ast::VariableKind::Var {
            self.emit_direct_eval_binding_declare_lexical(self.interner.resolve(name));
            let mut value = if let Some(init) = &decl.initializer {
                self.lower_expr_with_object_spread_filter(init, decl.type_annotation.as_ref())
            } else {
                let undefined = self.alloc_register(UNRESOLVED);
                self.emit(IrInstr::Assign {
                    dest: undefined.clone(),
                    value: IrValue::Constant(IrConstant::Undefined),
                });
                undefined
            };
            if let Some(type_ann) = &decl.type_annotation {
                value = self.coerce_value_to_annotation_type(value, type_ann);
            }
            self.emit_direct_eval_binding_set(self.interner.resolve(name), value);
            return;
        }

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
                        if let Some(&global_idx) = self
                            .js_script_lexical_globals
                            .get(&name)
                            .or_else(|| self.module_var_globals.get(&name))
                        {
                            let value = self.emit_constant_value(&const_val);
                            self.global_type_map.insert(global_idx, value.ty);
                            self.emit(IrInstr::StoreGlobal {
                                index: global_idx,
                                value,
                            });
                            if self.js_script_lexical_globals.contains_key(&name) {
                                self.mark_js_script_lexical_initialized(name);
                            }
                        }
                    } else {
                        let value = self.emit_constant_value(&const_val);
                        let local_idx = self.allocate_local(name);
                        self.local_registers.insert(local_idx, value.clone());
                        self.emit(IrInstr::StoreLocal {
                            index: local_idx,
                            value,
                        });
                    }
                    return;
                }
            }
        }

        // Module-level variable: use global storage (not local) so module-level
        // functions can access them via LoadGlobal/StoreGlobal.
        // Only at module scope (depth 0) — inside function bodies, `let x` creates a local
        // even if a module-level `x` exists (shadowing).
        let uses_hoisted_script_global = self.js_this_binding_compat
            && decl.kind == crate::parser::ast::VariableKind::Var
            && self.function_depth == 0;
        let uses_top_level_script_lexical = self.js_this_binding_compat
            && self.function_depth == 0
            && self.block_depth == 0
            && decl.kind != crate::parser::ast::VariableKind::Var
            && self.js_script_lexical_globals.contains_key(&name);
        if uses_top_level_script_lexical {
            let global_idx = *self
                .js_script_lexical_globals
                .get(&name)
                .expect("top-level JS lexical slot must exist");
            if let Some(init) = &decl.initializer {
                let mut value =
                    self.lower_expr_with_object_spread_filter(init, decl.type_annotation.as_ref());
                if let Some(type_ann) = &decl.type_annotation {
                    value = self.coerce_value_to_annotation_type(value, type_ann);
                }
                self.global_type_map.insert(global_idx, value.ty);
                self.emit(IrInstr::StoreGlobal {
                    index: global_idx,
                    value: value.clone(),
                });
                self.mark_js_script_lexical_initialized(name);
            } else {
                let undefined = self.alloc_register(UNRESOLVED);
                self.emit(IrInstr::Assign {
                    dest: undefined.clone(),
                    value: IrValue::Constant(IrConstant::Undefined),
                });
                self.global_type_map.insert(global_idx, undefined.ty);
                self.emit(IrInstr::StoreGlobal {
                    index: global_idx,
                    value: undefined,
                });
                self.mark_js_script_lexical_initialized(name);
            }
            return;
        }

        if self.function_depth == 0
            && (self.block_depth == 0 || uses_hoisted_script_global)
            && !uses_top_level_script_lexical
        {
            if let Some(&global_idx) = self.module_var_globals.get(&name) {
                if let Some(init) = &decl.initializer {
                    let explicit_dynamic_any_annotation =
                        decl.type_annotation.as_ref().is_some_and(|type_ann| {
                            self.type_is_dynamic_any_like(
                                self.resolve_structural_slot_type_from_annotation(type_ann),
                            )
                        });
                    if explicit_dynamic_any_annotation {
                        self.dynamic_any_vars.insert(name);
                    } else {
                        self.dynamic_any_vars.remove(&name);
                    }
                    // Track class type from type annotation (same as local path)
                    if let Some(type_ann) = &decl.type_annotation {
                        if let Some(nominal_type_id) = self.try_extract_class_from_type(type_ann) {
                            self.nominal_binding_cache.insert(name, nominal_type_id);
                            self.clear_runtime_dispatch_hint(name);
                        }
                        self.track_variable_object_alias_from_annotation(name, type_ann);
                        self.track_variable_structural_projection_from_annotation(name, type_ann);
                        if self
                            .projection_layout_cache
                            .contains_key(&name)
                        {
                            self.nominal_binding_cache.remove(&name);
                        }
                        if let ast::Type::Array(arr_ty) = &type_ann.ty {
                            if let ast::Type::Reference(elem_ref) = &arr_ty.element_type.ty {
                                if let Some(&nominal_type_id) =
                                    self.class_map.get(&elem_ref.name.name)
                                {
                                    self.array_element_nominal_cache.insert(name, nominal_type_id);
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
                            let ctor_ty = self
                                .get_expr_type(&new_expr.callee)
                                .as_u32()
                                .ne(&UNRESOLVED_TYPE_ID)
                                .then(|| self.get_expr_type(&new_expr.callee))
                                .or_else(|| {
                                    self.type_ctx
                                        .lookup_named_type(self.interner.resolve(ident.name))
                                });
                            let nominal_type_id = self.resolve_runtime_bound_new_nominal_type(
                                ident.name,
                                ctor_ty,
                                self.get_expr_type(init)
                                    .as_u32()
                                    .ne(&UNRESOLVED_TYPE_ID)
                                    .then(|| self.get_expr_type(init)),
                            );
                            if let Some(nominal_type_id) = nominal_type_id {
                                self.nominal_binding_cache.insert(name, nominal_type_id);
                                self.clear_runtime_dispatch_hint(name);
                            } else if let Some((ctor_symbol, ctor_ty)) =
                                self.new_expr_runtime_dispatch_binding_hint(new_expr)
                            {
                                self.set_runtime_dispatch_hint(name, ctor_symbol, ctor_ty);
                            }
                        }
                    }
                    if let ast::Expression::Identifier(ident) = init {
                        if let Some((ctor_symbol, constructor_type)) =
                            self.identifier_constructor_binding_hint(ident)
                        {
                            self.mark_constructor_value_binding(name, ctor_symbol, constructor_type);
                        } else {
                            self.clear_constructor_value_binding(name);
                        }
                    } else if let Some(class_symbol) = self.class_expression_name_symbol(init) {
                        self.mark_constructor_value_binding(name, class_symbol, None);
                    } else {
                        self.clear_constructor_value_binding(name);
                    }

                    // Infer class type from method call return types
                    if let Some(nominal_type_id) = self
                        .infer_nominal_type_id(init)
                        .or_else(|| self.nominal_type_id_for_class_expression(init))
                    {
                        self.nominal_binding_cache.insert(name, nominal_type_id);
                        self.clear_runtime_dispatch_hint(name);
                    }
                    if explicit_dynamic_any_annotation {
                        self.nominal_binding_cache.remove(&name);
                        self.clear_runtime_dispatch_hint(name);
                    }
                    self.track_task_result_alias_from_initializer(name, init);
                    self.track_variable_object_alias_from_initializer(name, init);
                    self.track_variable_structural_projection_from_initializer(name, init);
                    if self
                        .projection_layout_cache
                        .contains_key(&name)
                    {
                        self.nominal_binding_cache.remove(&name);
                    }

                    // Track if this is an async arrow function assigned to a global
                    let is_async_arrow = if let ast::Expression::Arrow(arrow) = init {
                        let callable_kind = self.callable_kind_for_span(
                            arrow.span.start,
                            arrow.is_async,
                            false,
                            false,
                        );
                        Self::callable_spawns_task(callable_kind)
                    } else {
                        false
                    };

                    let mut value = self
                        .lower_expr_with_object_spread_filter(init, decl.type_annotation.as_ref());
                    if let Some(type_ann) = &decl.type_annotation {
                        value = self.coerce_value_to_annotation_type(value, type_ann);
                    }

                    if let Some(type_ann) = &decl.type_annotation {
                        let expected_ty =
                            self.resolve_structural_slot_type_from_annotation(type_ann);
                        if !self.emit_projected_shape_registration_for_register_type(
                            &value,
                            expected_ty,
                        ) {
                            self.emit_structural_slot_registration_for_type(
                                value.clone(),
                                expected_ty,
                            );
                        }
                    }

                    // Fallback class capture from lowered value type.
                    // This is critical for imported/default-exported factories where
                    // pre-lowering AST inference may miss the concrete class, but
                    // the checker/lowered register type is already precise.
                    if let Some(nominal_type_id) = self.nominal_type_id_from_type_id(value.ty) {
                        self.nominal_binding_cache.insert(name, nominal_type_id);
                        self.clear_runtime_dispatch_hint(name);
                    }
                    if self.type_is_handle_dispatch_builtin(value.ty) {
                        self.clear_runtime_dispatch_hint(name);
                    }
                    let keep_late_bound_builtin_dispatch =
                        self.binding_uses_builtin_dispatch_hint(name);
                    if self.type_has_checker_validated_class_members(value.ty)
                        && !keep_late_bound_builtin_dispatch
                    {
                        self.clear_runtime_dispatch_hint(name);
                    }
                    if !keep_late_bound_builtin_dispatch {
                        if let Some(layout) =
                            self.structural_projection_layout_from_type_id(value.ty)
                        {
                            self.projection_layout_cache
                                .insert(name, layout);
                            self.nominal_binding_cache.remove(&name);
                        }
                    }
                    if explicit_dynamic_any_annotation {
                        self.nominal_binding_cache.remove(&name);
                        self.clear_runtime_dispatch_hint(name);
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
                        value: value.clone(),
                    });
                    if decl.kind == crate::parser::ast::VariableKind::Var {
                        self.emit_js_script_global_binding(name, value);
                    }

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
                        self.nominal_binding_cache.remove(&name);
                        self.clear_runtime_dispatch_hint(name);
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
        let reuses_hoisted_local = self.js_this_binding_compat
            && decl.kind == crate::parser::ast::VariableKind::Var
            && self.function_depth > 0
            && self.local_map.contains_key(&name);
        let reuses_preallocated_capture = self.js_this_binding_compat
            && self.block_depth == 0
            && self.refcell_vars.contains(&name)
            && self.local_map.contains_key(&name);
        let local_idx = if reuses_hoisted_local || reuses_preallocated_capture {
            self.lookup_local(name)
                .expect("existing hoisted/preallocated JS local must be present")
        } else {
            self.allocate_local(name)
        };

        // Check if this variable needs RefCell wrapping (captured by closure).
        // JS lexical bindings with function-valued initializers can become observable
        // from nested direct eval during their own initialization, so preallocate
        // them as live cells rather than snapshotting a pre-init value.
        let lexical_initializer_refcell =
            is_js_lexical && self.function_depth > 0 && decl.initializer.is_some();
        let needs_refcell = self.refcell_vars.contains(&name) || lexical_initializer_refcell;

        if lexical_initializer_refcell
            && !reuses_hoisted_local
            && !reuses_preallocated_capture
            && !self.refcell_registers.contains_key(&local_idx)
        {
            let undefined = self.alloc_register(UNRESOLVED);
            self.emit(IrInstr::Assign {
                dest: undefined.clone(),
                value: IrValue::Constant(IrConstant::Undefined),
            });
            let refcell_ty = TypeId::new(0);
            let refcell_reg = self.alloc_register(refcell_ty);
            self.emit(IrInstr::NewRefCell {
                dest: refcell_reg.clone(),
                initial_value: undefined,
            });
            self.local_registers.insert(local_idx, refcell_reg.clone());
            self.refcell_registers
                .insert(local_idx, refcell_reg.clone());
            self.refcell_inner_types.insert(local_idx, UNRESOLVED);
            self.emit(IrInstr::StoreLocal {
                index: local_idx,
                value: refcell_reg,
            });
        }

        // If there's an initializer, evaluate and store
        // The register from lowering the expression will have the correct inferred type
        if let Some(init) = &decl.initializer {
            let explicit_dynamic_any_annotation =
                decl.type_annotation.as_ref().is_some_and(|type_ann| {
                    self.type_is_dynamic_any_like(
                        self.resolve_structural_slot_type_from_annotation(type_ann),
                    )
                });
            if explicit_dynamic_any_annotation {
                self.dynamic_any_vars.insert(name);
            } else {
                self.dynamic_any_vars.remove(&name);
            }
            // Track class type from explicit type annotation FIRST (highest priority).
            // This must come before other inference to override stale entries from other scopes
            // (nominal_binding_cache is a flat map without scope tracking).
            if let Some(type_ann) = &decl.type_annotation {
                if let Some(nominal_type_id) = self.try_extract_class_from_type(type_ann) {
                    self.nominal_binding_cache.insert(name, nominal_type_id);
                    self.clear_runtime_dispatch_hint(name);
                }
                self.track_variable_object_alias_from_annotation(name, type_ann);
                self.track_variable_structural_projection_from_annotation(name, type_ann);
                if self
                    .projection_layout_cache
                    .contains_key(&name)
                {
                    self.nominal_binding_cache.remove(&name);
                }
                // Track array element class type (e.g., `let items: Item[] = [...]`)
                if let ast::Type::Array(arr_ty) = &type_ann.ty {
                    if let ast::Type::Reference(elem_ref) = &arr_ty.element_type.ty {
                        if let Some(&nominal_type_id) = self.class_map.get(&elem_ref.name.name) {
                            self.array_element_nominal_cache.insert(name, nominal_type_id);
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
                    let ctor_ty = self
                        .get_expr_type(&new_expr.callee)
                        .as_u32()
                        .ne(&UNRESOLVED_TYPE_ID)
                        .then(|| self.get_expr_type(&new_expr.callee))
                        .or_else(|| {
                            self.type_ctx
                                .lookup_named_type(self.interner.resolve(ident.name))
                        });
                    let nominal_type_id = self.resolve_runtime_bound_new_nominal_type(
                        ident.name,
                        ctor_ty,
                        self.get_expr_type(init)
                            .as_u32()
                            .ne(&UNRESOLVED_TYPE_ID)
                            .then(|| self.get_expr_type(init)),
                    );
                    if let Some(nominal_type_id) = nominal_type_id {
                        self.nominal_binding_cache.insert(name, nominal_type_id);
                        self.clear_runtime_dispatch_hint(name);
                    } else if let Some((ctor_symbol, ctor_ty)) =
                        self.new_expr_runtime_dispatch_binding_hint(new_expr)
                    {
                        self.set_runtime_dispatch_hint(name, ctor_symbol, ctor_ty);
                    }
                }
            }
            if let ast::Expression::Identifier(ident) = init {
                if let Some((ctor_symbol, constructor_type)) =
                    self.identifier_constructor_binding_hint(ident)
                {
                    self.mark_constructor_value_binding(name, ctor_symbol, constructor_type);
                } else {
                    self.clear_constructor_value_binding(name);
                }
            } else if let Some(class_symbol) = self.class_expression_name_symbol(init) {
                self.mark_constructor_value_binding(name, class_symbol, None);
            } else {
                self.clear_constructor_value_binding(name);
            }

            // Infer class type from method call return types
            // e.g., `let output = source.pipeThrough(x)` → infer ReadableStream from return type
            if let Some(nominal_type_id) = self
                .infer_nominal_type_id(init)
                .or_else(|| self.nominal_type_id_for_class_expression(init))
            {
                self.nominal_binding_cache.insert(name, nominal_type_id);
                self.clear_runtime_dispatch_hint(name);
            }
            if explicit_dynamic_any_annotation {
                self.nominal_binding_cache.remove(&name);
                self.clear_runtime_dispatch_hint(name);
            }
            self.track_task_result_alias_from_initializer(name, init);
            self.track_variable_object_alias_from_initializer(name, init);
            self.track_variable_structural_projection_from_initializer(name, init);
            if self
                .projection_layout_cache
                .contains_key(&name)
            {
                self.nominal_binding_cache.remove(&name);
            }

            // Track if this is an arrow function for async closure detection
            let is_async_arrow = if let ast::Expression::Arrow(arrow) = init {
                let callable_kind =
                    self.callable_kind_for_span(arrow.span.start, arrow.is_async, false, false);
                Self::callable_spawns_task(callable_kind)
            } else {
                false
            };

            let mut value =
                self.lower_expr_with_object_spread_filter(init, decl.type_annotation.as_ref());
            if let Some(type_ann) = &decl.type_annotation {
                value = self.coerce_value_to_annotation_type(value, type_ann);
            }
            let rebinding_call_result =
                self.js_this_binding_compat && self.js_receiver_rebinding_call_expr(init);

            if let Some(type_ann) = &decl.type_annotation {
                let expected_ty = self.resolve_structural_slot_type_from_annotation(type_ann);
                if !self.emit_projected_shape_registration_for_register_type(&value, expected_ty) {
                    self.emit_structural_slot_registration_for_type(value.clone(), expected_ty);
                }
            }

            // Fallback class capture from lowered value type.
            // Helps preserve receiver typing for chained calls on values returned
            // from imports/factories when AST-only inference was inconclusive.
            if let Some(nominal_type_id) = self.nominal_type_id_from_type_id(value.ty) {
                self.nominal_binding_cache.insert(name, nominal_type_id);
                self.clear_runtime_dispatch_hint(name);
            }
            if self.type_is_handle_dispatch_builtin(value.ty) {
                self.clear_runtime_dispatch_hint(name);
            }
            let keep_late_bound_builtin_dispatch =
                self.binding_uses_builtin_dispatch_hint(name);
            if self.type_has_checker_validated_class_members(value.ty)
                && !keep_late_bound_builtin_dispatch
            {
                self.clear_runtime_dispatch_hint(name);
            }
            if !keep_late_bound_builtin_dispatch {
                if let Some(layout) = self.structural_projection_layout_from_type_id(value.ty) {
                    self.projection_layout_cache
                        .insert(name, layout);
                    self.nominal_binding_cache.remove(&name);
                }
            }
            if explicit_dynamic_any_annotation {
                self.nominal_binding_cache.remove(&name);
                self.clear_runtime_dispatch_hint(name);
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
                if reuses_hoisted_local || reuses_preallocated_capture {
                    if let Some(refcell_reg) = self.existing_refcell_local(local_idx) {
                        self.refcell_inner_types.insert(local_idx, value.ty);
                        self.emit(IrInstr::StoreRefCell {
                            refcell: refcell_reg,
                            value,
                        });
                    } else {
                        let refcell_ty = TypeId::new(0);
                        let refcell_reg = self.alloc_register(refcell_ty);
                        self.emit(IrInstr::NewRefCell {
                            dest: refcell_reg.clone(),
                            initial_value: value.clone(),
                        });
                        self.local_registers.insert(local_idx, refcell_reg.clone());
                        self.refcell_registers
                            .insert(local_idx, refcell_reg.clone());
                        self.refcell_inner_types.insert(local_idx, value.ty);
                        self.emit(IrInstr::StoreLocal {
                            index: local_idx,
                            value: refcell_reg,
                        });
                    }
                } else {
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
                }
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
                } else if rebinding_call_result {
                    use crate::compiler::ir::Register;
                    Register {
                        id: value.id,
                        ty: UNRESOLVED,
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
                if self.type_is_dynamic_any_like(
                    self.resolve_structural_slot_type_from_annotation(type_ann),
                ) {
                    self.dynamic_any_vars.insert(name);
                } else {
                    self.dynamic_any_vars.remove(&name);
                }
                if let Some(nominal_type_id) = self.try_extract_class_from_type(type_ann) {
                    self.nominal_binding_cache.insert(name, nominal_type_id);
                }
                self.track_variable_object_alias_from_annotation(name, type_ann);
                self.track_variable_structural_projection_from_annotation(name, type_ann);
                if self
                    .projection_layout_cache
                    .contains_key(&name)
                {
                    self.nominal_binding_cache.remove(&name);
                }
                if let ast::Type::Array(arr_ty) = &type_ann.ty {
                    if let ast::Type::Reference(elem_ref) = &arr_ty.element_type.ty {
                        if let Some(&nominal_type_id) = self.class_map.get(&elem_ref.name.name) {
                            self.array_element_nominal_cache.insert(name, nominal_type_id);
                        }
                    }
                }
                if self.type_annotation_is_callable(type_ann) {
                    self.callable_local_hints.insert(local_idx);
                    self.callable_symbol_hints.insert(name);
                }
                if self.type_is_dynamic_any_like(
                    self.resolve_structural_slot_type_from_annotation(type_ann),
                ) {
                    self.nominal_binding_cache.remove(&name);
                    self.clear_runtime_dispatch_hint(name);
                }
            } else {
                self.dynamic_any_vars.remove(&name);
            }

            // No initializer - get type from annotation or UNRESOLVED
            let ty = decl
                .type_annotation
                .as_ref()
                .map(|t| self.resolve_type_annotation(t))
                .unwrap_or(UNRESOLVED);
            if (reuses_hoisted_local
                && self.js_this_binding_compat
                && decl.kind == crate::parser::ast::VariableKind::Var)
                || reuses_preallocated_capture
            {
                return;
            }

            let uninitialized_reg = if self.js_this_binding_compat {
                let undefined = self.alloc_register(UNRESOLVED);
                self.emit(IrInstr::Assign {
                    dest: undefined.clone(),
                    value: IrValue::Constant(IrConstant::Undefined),
                });
                undefined
            } else {
                self.lower_null_literal()
            };

            if needs_refcell {
                // Wrap null in a RefCell
                let refcell_ty = TypeId::new(0);
                let refcell_reg = self.alloc_register(refcell_ty);
                self.emit(IrInstr::NewRefCell {
                    dest: refcell_reg.clone(),
                    initial_value: uninitialized_reg.clone(),
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
                    id: uninitialized_reg.id,
                    ty,
                };
                self.local_registers.insert(local_idx, typed_reg.clone());
                self.emit(IrInstr::StoreLocal {
                    index: local_idx,
                    value: uninitialized_reg,
                });
            }
        }
    }

    fn lower_expr_stmt(&mut self, stmt: &ast::ExpressionStatement) {
        let value = self.lower_expr(&stmt.expression);
        self.record_eval_completion(value);
    }

    fn lower_nested_function_decl(&mut self, func_decl: &ast::FunctionDecl) {
        if self
            .hoisted_function_decl_spans
            .contains(&func_decl.span.start)
        {
            return;
        }
        self.materialize_nested_function_decl(func_decl, false);
    }

    pub(super) fn lower_nested_function_decl_hoist(&mut self, func_decl: &ast::FunctionDecl) {
        self.materialize_nested_function_decl(func_decl, true);
    }

    fn materialize_nested_function_decl(
        &mut self,
        func_decl: &ast::FunctionDecl,
        reuse_existing_local: bool,
    ) {
        use crate::parser::ast::FunctionExpression;
        use crate::parser::token::Span;

        // Lower nested JS/Raya function declarations as real function objects,
        // not synthetic arrows, so they keep their own `this`, `arguments`,
        // constructibility, and declaration semantics.
        let function_expr = FunctionExpression {
            name: Some(func_decl.name.clone()),
            type_params: func_decl.type_params.clone(),
            params: func_decl.params.clone(),
            body: func_decl.body.clone(),
            return_type: func_decl.return_type.clone(),
            is_method: false,
            is_async: func_decl.is_async,
            is_generator: func_decl.is_generator,
            span: Span::new(0, 0, 0, 0),
        };

        // Ordinary nested declarations should lower as fresh closures. Reusing
        // declaration-registration IDs here can point block-scoped functions at
        // the wrong callable body shape, especially for async declarations.
        let in_module_wrapper = self
            .current_function
            .as_ref()
            .is_some_and(|f| is_module_wrapper_function_name(&f.name));
        let preassigned_func_id = in_module_wrapper
            .then(|| self.function_id_for_decl(func_decl))
            .flatten();
        let closure_reg =
            self.lower_function_expression_with_preassigned_id(&function_expr, preassigned_func_id);
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

        let publish_to_direct_eval_env =
            self.in_direct_eval_function && !(self.js_strict_context && self.block_depth > 0);
        if publish_to_direct_eval_env {
            self.emit_direct_eval_binding_declare_function(
                self.interner.resolve(func_decl.name.name),
                closure_reg,
            );
            return;
        }

        // Assign to a local variable with the function's name
        let local_idx = if reuse_existing_local {
            self.lookup_local(func_decl.name.name)
                .unwrap_or_else(|| self.allocate_local(func_decl.name.name))
        } else {
            self.allocate_local(func_decl.name.name)
        };
        // Nested function declarations are callable closures; mark both the symbol
        // and local slot so captured/ancestor call lowering treats them as callable.
        self.callable_local_hints.insert(local_idx);
        self.callable_symbol_hints.insert(func_decl.name.name);
        self.local_registers.insert(local_idx, closure_reg.clone());
        self.emit(IrInstr::StoreLocal {
            index: local_idx,
            value: closure_reg.clone(),
        });

        // Async function declarations lowered through the closure path still need
        // closure-locals metadata so call lowering emits SpawnClosure, not CallClosure.
        let callable_kind = self.callable_kind_for_span(
            func_decl.span.start,
            func_decl.is_async,
            func_decl.is_generator,
            false,
        );
        if Self::callable_spawns_task(callable_kind) {
            if let Some(func_id) = preassigned_func_id.or(self.last_arrow_func_id) {
                if self.async_closures.contains(&func_id) {
                    self.closure_locals.insert(local_idx, func_id);
                }
            }
        }
    }

    fn lower_return(&mut self, ret: &ast::ReturnStatement) {
        let value = if let Some(expr) = &ret.value {
            Some(self.lower_expr(expr))
        } else if let Some(this_reg) = &self.constructor_return_this {
            Some(this_reg.clone())
        } else if self.js_this_binding_compat {
            let undefined = self.alloc_register(UNRESOLVED);
            self.emit(IrInstr::Assign {
                dest: undefined.clone(),
                value: IrValue::Constant(IrConstant::Undefined),
            });
            Some(undefined)
        } else {
            None
        };

        self.close_active_loop_iterators();

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

        self.emit_function_return(value);
    }

    fn lower_yield(&mut self, yld: &ast::YieldStatement) {
        if self.generator_yield_array_local.is_some() {
            let Some(yield_array) = self.load_generator_yield_array() else {
                return;
            };

            if yld.is_delegate {
                if let Some(value) = &yld.value {
                    let iterable = self.lower_expr(value);
                    self.emit_iterator_append_to_array_helper(yield_array, iterable);
                }
            } else {
                let yielded = if let Some(value) = &yld.value {
                    self.lower_expr(value)
                } else {
                    let undefined = self.alloc_register(TypeId::new(UNRESOLVED_TYPE_ID));
                    self.emit(IrInstr::Assign {
                        dest: undefined.clone(),
                        value: IrValue::Constant(IrConstant::Undefined),
                    });
                    undefined
                };
                self.emit(IrInstr::ArrayPush {
                    array: yield_array,
                    element: yielded,
                });
            }
            return;
        }

        let yielded = if let Some(value) = &yld.value {
            self.lower_expr(value)
        } else {
            let undefined = self.alloc_register(TypeId::new(UNRESOLVED_TYPE_ID));
            self.emit(IrInstr::Assign {
                dest: undefined.clone(),
                value: IrValue::Constant(IrConstant::Undefined),
            });
            undefined
        };
        self.emit(IrInstr::GeneratorYield { value: yielded });

        let temp_local = self.next_local;
        self.next_local += 1;
        self.emit(IrInstr::PopToLocal { index: temp_local });

        let resumed_payload = self.alloc_register(UNRESOLVED);
        self.emit(IrInstr::LoadLocal {
            index: temp_local,
            dest: resumed_payload.clone(),
        });
        let resumed = self.alloc_register(UNRESOLVED);
        self.emit(IrInstr::NativeCall {
            dest: Some(resumed),
            native_id: crate::compiler::native_id::OBJECT_HANDLE_GENERATOR_RESUME,
            args: vec![resumed_payload],
        });
    }

    fn close_active_loop_iterators(&mut self) {
        let iterator_records: Vec<Register> = self
            .loop_stack
            .iter()
            .rev()
            .filter_map(|ctx| ctx.iterator_record.clone())
            .collect();
        for iterator in iterator_records {
            self.emit_iterator_close_helper(iterator);
        }
    }

    fn close_loop_iterators_from_target(&mut self, target_index: usize, include_target: bool) {
        let iterator_records: Vec<Register> = self
            .loop_stack
            .iter()
            .enumerate()
            .rev()
            .filter_map(|(index, ctx)| {
                if index < target_index || (!include_target && index == target_index) {
                    None
                } else {
                    ctx.iterator_record.clone()
                }
            })
            .collect();
        for iterator in iterator_records {
            self.emit_iterator_close_helper(iterator);
        }
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
            iterator_record: None,
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
        let loop_plan = self.loop_scope_plan(for_stmt.span.start);
        let runtime_loop_env = self.js_this_binding_compat
            && loop_plan
                .as_ref()
                .is_some_and(|plan| plan.creates_per_iteration_env && !plan.binding_names.is_empty());
        let runtime_loop_binding_names = loop_plan
            .as_ref()
            .map(|plan| plan.binding_names.clone())
            .unwrap_or_default();

        if runtime_loop_env {
            self.declare_runtime_loop_bindings(&runtime_loop_binding_names);
        }

        // Lower initializer
        if let Some(init) = &for_stmt.init {
            match init {
                ast::ForInit::VariableDecl(decl) => {
                    if runtime_loop_env {
                        self.lower_runtime_loop_initializer_decl(decl, &runtime_loop_binding_names);
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
            iterator_record: None,
            try_finally_depth: self.try_finally_stack.len(),
        });

        // Body block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(body_block, "for.body"));
        self.current_block = body_block;

        self.lower_stmt(&for_stmt.body);

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
        if runtime_loop_env {
            self.emit_replace_declarative_env(&runtime_loop_binding_names);
        }
        if let Some(update) = &for_stmt.update {
            self.lower_expr(update);
        }
        self.set_terminator(Terminator::Jump(header_block));

        // Exit block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(exit_block, "for.exit"));
        self.current_block = exit_block;
        if runtime_loop_env {
            self.emit_pop_declarative_env();
        }
    }

    fn lower_block(&mut self, block: &ast::BlockStatement) {
        // Save current local_map state for scope management
        // This allows nested scopes to shadow outer variables without
        // overwriting the outer variable's slot mapping
        let saved_local_map = self.local_map.clone();
        let saved_constant_map = self.constant_map.clone();
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
        self.constant_map = saved_constant_map;
        self.block_depth = self.block_depth.saturating_sub(1);
    }

    fn lower_break(&mut self, brk: &ast::BreakStatement) {
        // Labeled break: search loop stack for matching label
        if let Some(ref label_ident) = brk.label {
            let label_sym = label_ident.name;
            if let Some((target_index, loop_ctx)) = self
                .loop_stack
                .iter()
                .enumerate()
                .rev()
                .find(|(_, ctx)| ctx.label == Some(label_sym))
                .map(|(index, ctx)| (index, ctx.clone()))
            {
                self.close_loop_iterators_from_target(target_index, true);
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
            if let Some(label_ctx) = self
                .label_stack
                .iter()
                .rev()
                .find(|ctx| ctx.label == label_sym)
                .cloned()
            {
                let depth = label_ctx.try_finally_depth;
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
                self.set_terminator(Terminator::Jump(label_ctx.break_target));
                return;
            }
        }

        // Unlabeled break: if inside switch, target switch exit
        if let Some(&switch_exit) = self.switch_stack.last() {
            self.set_terminator(Terminator::Jump(switch_exit));
            return;
        }
        if let Some((target_index, loop_ctx)) = self
            .loop_stack
            .iter()
            .enumerate()
            .last()
            .map(|(index, ctx)| (index, ctx.clone()))
        {
            self.close_loop_iterators_from_target(target_index, true);
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
                .enumerate()
                .rev()
                .find(|(_, ctx)| ctx.label == Some(label_sym))
                .map(|(index, ctx)| (index, ctx.clone()))
        } else {
            self.loop_stack
                .iter()
                .enumerate()
                .last()
                .map(|(index, ctx)| (index, ctx.clone()))
        };

        if let Some((target_index, loop_ctx)) = loop_ctx {
            self.close_loop_iterators_from_target(target_index, false);
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
        self.close_active_loop_iterators();
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
            let saved_local_map = self.local_map.clone();
            let saved_constant_map = self.constant_map.clone();
            self.block_depth += 1;

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

            // Lower catch body inside the same lexical scope as the catch parameter.
            for stmt in &catch_clause.body.statements {
                self.lower_stmt(stmt);
                if self.current_block_is_terminated() {
                    break;
                }
            }

            self.local_map = saved_local_map;
            self.constant_map = saved_constant_map;
            self.block_depth = self.block_depth.saturating_sub(1);
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
    use crate::compiler::lower::{Lowerer, NominalTypeId};
    use crate::parser::types::ty::{ClassType, MethodSignature, PropertySignature, Type};
    use crate::parser::{Parser, TypeContext};

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
