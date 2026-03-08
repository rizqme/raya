//! Expression Lowering
//!
//! Converts AST expressions to IR instructions.

use super::{
    ClassFieldInfo, ConstantValue, Lowerer, BOOLEAN_TYPE_ID, CHANNEL_TYPE_ID, INT_TYPE_ID,
    JSON_OBJECT_TYPE_ID, JSON_TYPE_ID, MUTEX_TYPE_ID, NULL_TYPE_ID, NUMBER_TYPE_ID, REGEXP_TYPE_ID,
    STRING_TYPE_ID, TASK_TYPE_ID, UNKNOWN_TYPE_ID, UNRESOLVED, UNRESOLVED_TYPE_ID,
};
use crate::compiler::ir::{
    BinaryOp, NominalTypeId, FunctionId, IrConstant, IrInstr, IrValue, Register, Terminator,
    UnaryOp,
};
use crate::compiler::CompileError;
use crate::parser::ast::{self, AssignmentOperator, Expression, TemplatePart};
use crate::parser::interner::Symbol;
use crate::parser::types::ty::{PrimitiveType, Type};
use crate::parser::{TypeContext as TC, TypeId};
use rustc_hash::{FxHashMap, FxHashSet};

// Re-export VM builtin method IDs (canonical source of truth)
#[allow(unused_imports)]
use crate::vm::builtin::number as builtin_number;
use crate::vm::builtin::regexp as builtin_regexp;

const CAST_KIND_MASK_FLAG: u16 = 0x8000;
const CAST_TUPLE_LEN_FLAG: u16 = 0x4000;
const CAST_OBJECT_MIN_FIELDS_FLAG: u16 = 0x2000;
const CAST_ARRAY_ELEM_KIND_FLAG: u16 = 0x1000;
const CAST_KIND_NULL: u16 = 0x0001;
const CAST_KIND_BOOL: u16 = 0x0002;
const CAST_KIND_INT: u16 = 0x0004;
const CAST_KIND_NUMBER: u16 = 0x0008;
const CAST_KIND_STRING: u16 = 0x0010;
const CAST_KIND_ARRAY: u16 = 0x0020;
const CAST_KIND_OBJECT: u16 = 0x0040;
const CAST_KIND_FUNCTION: u16 = 0x0080;

enum SpreadSourceFields {
    Concrete(Vec<(String, u16)>),
    Shape { shape_id: u64, fields: Vec<(String, u16)> },
}

impl<'a> Lowerer<'a> {
    fn projection_layout_u16_from_type_id(&self, ty: TypeId) -> Option<Vec<(String, u16)>> {
        self.structural_projection_layout_from_type_id(ty).map(|layout| {
            layout
                .into_iter()
                .filter_map(|(field_name, field_idx)| {
                    u16::try_from(field_idx).ok().map(|slot| (field_name, slot))
                })
                .collect()
        })
    }

    pub(super) fn projected_structural_layout_from_alias_name(
        &self,
        alias_name: &str,
    ) -> Option<Vec<(String, u16)>> {
        self.type_alias_object_fields.get(alias_name).map(|fields| {
            fields
                .iter()
                .map(|(field_name, field_idx, _)| (field_name.clone(), *field_idx))
                .collect()
        })
    }

    fn prefers_structural_member_projection(&self, object_expr: &Expression) -> bool {
        self.projected_structural_layout_from_expr(object_expr)
            .is_some()
    }

    fn projected_structural_layout_from_expr(
        &self,
        object_expr: &Expression,
    ) -> Option<Vec<(String, u16)>> {
        match object_expr {
            Expression::Identifier(ident) => self
                .variable_structural_projection_fields
                .get(&ident.name)
                .map(|layout| {
                    layout
                        .iter()
                        .filter_map(|(field_name, field_idx)| {
                            u16::try_from(*field_idx).ok().map(|slot| (field_name.clone(), slot))
                        })
                        .collect()
                })
                .or_else(|| {
                    self.variable_object_type_aliases
                        .get(&ident.name)
                        .and_then(|alias| self.projected_structural_layout_from_alias_name(alias))
                })
                .or_else(|| self.projection_layout_u16_from_type_id(self.get_expr_type(object_expr))),
            Expression::TypeCast(cast) => {
                if self.try_extract_class_from_type(&cast.target_type).is_some() {
                    return None;
                }
                let target_ty = self.resolve_structural_slot_type_from_annotation(&cast.target_type);
                self.projection_layout_u16_from_type_id(target_ty)
            }
            Expression::Parenthesized(paren) => {
                self.projected_structural_layout_from_expr(&paren.expression)
            }
            _ => self.projection_layout_u16_from_type_id(self.get_expr_type(object_expr)),
        }
    }

    fn propagate_variable_projection_to_register(
        &mut self,
        var_name: crate::parser::Symbol,
        dest: &Register,
    ) {
        if let Some(fields) = self
            .variable_structural_projection_fields
            .get(&var_name)
            .cloned()
        {
            self.register_structural_projection_fields.insert(dest.id, fields);
        }
    }

    fn propagate_type_projection_to_register(&mut self, ty: TypeId, dest: &Register) {
        let _ = self.emit_projected_shape_registration_for_register_type(dest, ty);
    }

    fn lower_unresolved_poison(&mut self) -> Register {
        // Keep lowering progressing after a hard error, but preserve unresolved typing
        // so downstream dispatch does not misclassify this as a concrete null receiver.
        let dest = self.alloc_register(UNRESOLVED);
        self.poison_register(&dest);
        dest
    }

    fn poison_register(&mut self, dest: &Register) {
        self.emit(IrInstr::Assign {
            dest: dest.clone(),
            value: IrValue::Constant(IrConstant::Null),
        });
    }

    /// Lower an expression, returning the register holding its value
    pub fn lower_expr(&mut self, expr: &Expression) -> Register {
        // Track source span for sourcemap generation
        self.set_span(expr.span());

        match expr {
            Expression::IntLiteral(lit) => self.lower_int_literal(lit),
            Expression::FloatLiteral(lit) => self.lower_float_literal(lit),
            Expression::StringLiteral(lit) => self.lower_string_literal(lit),
            Expression::BooleanLiteral(lit) => self.lower_bool_literal(lit),
            Expression::NullLiteral(_) => self.lower_null_literal(),
            Expression::Identifier(ident) => self.lower_identifier(ident),
            Expression::Binary(binary) => self.lower_binary(binary),
            Expression::Unary(unary) => self.lower_unary(unary),
            Expression::Call(call) => self.lower_call(call, expr),
            Expression::Member(member) => self.lower_member(member),
            Expression::Index(index) => self.lower_index(index, expr),
            Expression::Array(array) => self.lower_array(array, expr),
            Expression::Object(object) => self.lower_object(object, expr),
            Expression::Assignment(assign) => self.lower_assignment(assign),
            Expression::Conditional(cond) => self.lower_conditional(cond),
            Expression::Arrow(arrow) => self.lower_arrow(arrow),
            Expression::Parenthesized(paren) => self.lower_expr(&paren.expression),
            Expression::Typeof(typeof_expr) => self.lower_typeof(typeof_expr),
            Expression::New(new_expr) => self.lower_new(new_expr),
            Expression::Await(await_expr) => self.lower_await(await_expr, expr),
            Expression::Logical(logical) => self.lower_logical(logical),
            Expression::TemplateLiteral(template) => self.lower_template_literal(template),
            Expression::This(_) => self.lower_this(),
            Expression::Super(_) => self.lower_super(),
            Expression::AsyncCall(async_call) => self.lower_async_call(async_call),
            Expression::InstanceOf(instanceof) => self.lower_instanceof(instanceof),
            Expression::TypeCast(cast) => self.lower_type_cast(cast),
            Expression::RegexLiteral(regex) => self.lower_regex_literal(regex),
            Expression::TaggedTemplate(tagged) => self.lower_tagged_template(tagged),
            Expression::DynamicImport(import) => {
                // Dynamic import() — evaluate the source expression, return unknown
                // Runtime dynamic module loading is not yet supported
                let _source = self.lower_expr(&import.source);
                self.alloc_register(TypeId::new(UNKNOWN_TYPE_ID))
            }
            Expression::JsxElement(jsx) => self.lower_jsx_element(jsx),
            Expression::JsxFragment(jsx) => self.lower_jsx_fragment(jsx),
        }
    }

    fn lower_int_literal(&mut self, lit: &ast::IntLiteral) -> Register {
        let ty = TypeId::new(INT_TYPE_ID);
        let dest = self.alloc_register(ty);
        self.emit(IrInstr::Assign {
            dest: dest.clone(),
            value: IrValue::Constant(IrConstant::I32(lit.value as i32)),
        });
        dest
    }

    fn lower_float_literal(&mut self, lit: &ast::FloatLiteral) -> Register {
        let ty = TypeId::new(NUMBER_TYPE_ID);
        let dest = self.alloc_register(ty);
        self.emit(IrInstr::Assign {
            dest: dest.clone(),
            value: IrValue::Constant(IrConstant::F64(lit.value)),
        });
        dest
    }

    fn lower_string_literal(&mut self, lit: &ast::StringLiteral) -> Register {
        let ty = TypeId::new(STRING_TYPE_ID);
        let dest = self.alloc_register(ty);
        let string_value = self.interner.resolve(lit.value).to_string();
        self.emit(IrInstr::Assign {
            dest: dest.clone(),
            value: IrValue::Constant(IrConstant::String(string_value)),
        });
        dest
    }

    fn lower_bool_literal(&mut self, lit: &ast::BooleanLiteral) -> Register {
        let ty = TypeId::new(BOOLEAN_TYPE_ID);
        let dest = self.alloc_register(ty);
        self.emit(IrInstr::Assign {
            dest: dest.clone(),
            value: IrValue::Constant(IrConstant::Boolean(lit.value)),
        });
        dest
    }

    pub(super) fn lower_null_literal(&mut self) -> Register {
        let ty = TypeId::new(NULL_TYPE_ID);
        let dest = self.alloc_register(ty);
        self.emit(IrInstr::Assign {
            dest: dest.clone(),
            value: IrValue::Constant(IrConstant::Null),
        });
        dest
    }

    fn lower_tagged_template(&mut self, tagged: &ast::TaggedTemplateExpression) -> Register {
        let string_ty = TypeId::new(STRING_TYPE_ID);
        let array_ty = TypeId::new(super::ARRAY_TYPE_ID);
        let dest = self.alloc_register(TypeId::new(UNKNOWN_TYPE_ID));

        // Build the strings array from template parts
        let mut string_regs = Vec::new();
        let mut expr_regs = Vec::new();

        for part in &tagged.template.parts {
            match part {
                TemplatePart::String(sym) => {
                    let s = self.interner.resolve(*sym).to_string();
                    let reg = self.alloc_register(string_ty);
                    self.emit(IrInstr::Assign {
                        dest: reg.clone(),
                        value: IrValue::Constant(IrConstant::String(s)),
                    });
                    string_regs.push(reg);
                }
                TemplatePart::Expression(expr) => {
                    expr_regs.push(self.lower_expr(expr));
                }
            }
        }

        // Create the strings array
        let len_reg = self.emit_i32_const(string_regs.len() as i32);
        let strings_arr = self.alloc_register(array_ty);
        self.emit(IrInstr::NewArray {
            dest: strings_arr.clone(),
            len: len_reg,
            elem_ty: string_ty,
        });
        for s_reg in &string_regs {
            self.emit(IrInstr::ArrayPush {
                array: strings_arr.clone(),
                element: s_reg.clone(),
            });
        }

        // Lower the tag expression and call it: tag(strings, ...exprs)
        // Enforce the same strict callability policy as regular calls.
        let tag_ty = self.get_expr_type(&tagged.tag);
        let tag_ty_raw = tag_ty.as_u32();
        if !self.type_is_callable(tag_ty) {
            self.errors
                .push(crate::compiler::CompileError::InternalError {
                    message: format!(
                        "unresolved tagged template call target: tag expression is not callable (type id {})",
                        tag_ty_raw
                    ),
                });
            self.poison_register(&dest);
            return dest;
        }
        let tag_reg = self.lower_expr(&tagged.tag);
        let mut args = vec![strings_arr];
        args.extend(expr_regs);

        self.emit(IrInstr::CallClosure {
            dest: Some(dest.clone()),
            closure: tag_reg,
            args,
        });
        dest
    }

    fn lower_regex_literal(&mut self, regex: &ast::RegexLiteral) -> Register {
        let pattern = self.interner.resolve(regex.pattern).to_string();
        let flags = self.interner.resolve(regex.flags).to_string();

        // Lower pattern string
        let pattern_reg = self.alloc_register(TypeId::new(STRING_TYPE_ID));
        self.emit(IrInstr::Assign {
            dest: pattern_reg.clone(),
            value: IrValue::Constant(IrConstant::String(pattern)),
        });

        // Lower flags string
        let flags_reg = self.alloc_register(TypeId::new(STRING_TYPE_ID));
        self.emit(IrInstr::Assign {
            dest: flags_reg.clone(),
            value: IrValue::Constant(IrConstant::String(flags)),
        });

        // Prefer class construction when RegExp class metadata exists so literals
        // produce proper objects (matching `new RegExp(...)`) rather than raw handles.
        let regexp_nominal_type_id = self.class_map.iter().find_map(|(sym, nominal_type_id)| {
            (self.interner.resolve(*sym) == TC::REGEXP_TYPE_NAME).then_some(*nominal_type_id)
        });

        if let Some(nominal_type_id) = regexp_nominal_type_id {
            let dest = self.alloc_register(UNRESOLVED);
            self.emit(IrInstr::NewType {
                dest: dest.clone(),
                nominal_type_id: nominal_type_id,
            });

            let all_fields = self.get_all_fields(nominal_type_id);
            for field in &all_fields {
                if let Some(ref init_expr) = field.initializer {
                    let value = self.lower_expr(init_expr);
                    self.emit(IrInstr::StoreFieldExact {
                        object: dest.clone(),
                        field: field.index,
                        value,
                    });
                }
            }

            if let Some(ctor_func_id) = self
                .class_info_map
                .get(&nominal_type_id)
                .and_then(|info| info.constructor)
            {
                self.emit(IrInstr::Call {
                    dest: None,
                    func: ctor_func_id,
                    args: vec![dest.clone(), pattern_reg, flags_reg],
                });
            }

            return dest;
        }

        // Fallback path when no RegExp class is present in scope.
        let dest = self.alloc_register(TypeId::new(REGEXP_TYPE_ID));
        self.emit(IrInstr::NativeCall {
            dest: Some(dest.clone()),
            native_id: builtin_regexp::NEW,
            args: vec![pattern_reg, flags_reg],
        });
        dest
    }

    /// Emit a compile-time constant value as an IR instruction
    /// Used for constant folding - inlines the constant directly
    pub(super) fn emit_constant_value(&mut self, const_val: &ConstantValue) -> Register {
        match const_val {
            ConstantValue::I64(v) => {
                let ty = TypeId::new(NUMBER_TYPE_ID); // Number type
                let dest = self.alloc_register(ty);
                self.emit(IrInstr::Assign {
                    dest: dest.clone(),
                    value: IrValue::Constant(IrConstant::I32(*v as i32)),
                });
                dest
            }
            ConstantValue::F64(v) => {
                let ty = TypeId::new(NUMBER_TYPE_ID); // Number type
                let dest = self.alloc_register(ty);
                self.emit(IrInstr::Assign {
                    dest: dest.clone(),
                    value: IrValue::Constant(IrConstant::F64(*v)),
                });
                dest
            }
            ConstantValue::String(s) => {
                let ty = TypeId::new(STRING_TYPE_ID); // String type
                let dest = self.alloc_register(ty);
                self.emit(IrInstr::Assign {
                    dest: dest.clone(),
                    value: IrValue::Constant(IrConstant::String(s.clone())),
                });
                dest
            }
            ConstantValue::Bool(v) => {
                let ty = TypeId::new(BOOLEAN_TYPE_ID); // Boolean type
                let dest = self.alloc_register(ty);
                self.emit(IrInstr::Assign {
                    dest: dest.clone(),
                    value: IrValue::Constant(IrConstant::Boolean(*v)),
                });
                dest
            }
        }
    }

    fn lower_identifier(&mut self, ident: &ast::Identifier) -> Register {
        // First, check if this is a compile-time constant (constant folding)
        // This takes precedence over local variables for const declarations
        if let Some(const_val) = self.constant_map.get(&ident.name).cloned() {
            return self.emit_constant_value(&const_val);
        }

        // Ambient strict globals available without local source injection.
        let name = self.interner.resolve(ident.name);
        match name {
            "Infinity" => {
                let dest = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
                self.emit(IrInstr::Assign {
                    dest: dest.clone(),
                    value: IrValue::Constant(IrConstant::F64(f64::INFINITY)),
                });
                return dest;
            }
            "NaN" => {
                let dest = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
                self.emit(IrInstr::Assign {
                    dest: dest.clone(),
                    value: IrValue::Constant(IrConstant::F64(f64::NAN)),
                });
                return dest;
            }
            "undefined" => {
                let dest = self.alloc_register(TypeId::new(NULL_TYPE_ID));
                self.emit(IrInstr::Assign {
                    dest: dest.clone(),
                    value: IrValue::Constant(IrConstant::Null),
                });
                return dest;
            }
            _ => {}
        }

        // Look up the variable in the local map (current function's locals)
        if let Some(&local_idx) = self.local_map.get(&ident.name) {
            // Check if this is a RefCell variable
            if self.refcell_registers.contains_key(&local_idx) {
                let inner_ty = self
                    .refcell_inner_types
                    .get(&local_idx)
                    .copied()
                    .unwrap_or(UNRESOLVED);
                // Load the RefCell pointer
                let refcell_reg = self.alloc_register(inner_ty);
                self.emit(IrInstr::LoadLocal {
                    dest: refcell_reg.clone(),
                    index: local_idx,
                });
                // Load the value from the RefCell
                let dest = self.alloc_register(inner_ty);
                self.emit(IrInstr::LoadRefCell {
                    dest: dest.clone(),
                    refcell: refcell_reg,
                });
                return dest;
            }

            let ty = self
                .local_registers
                .get(&local_idx)
                .map(|r| r.ty)
                .unwrap_or(UNRESOLVED);
            let dest = self.alloc_register(ty);
            self.emit(IrInstr::LoadLocal {
                dest: dest.clone(),
                index: local_idx,
            });
            // Propagate object field layout so destructuring can resolve field names
            if let Some(fields) = self.variable_object_fields.get(&ident.name).cloned() {
                self.register_object_fields.insert(dest.id, fields);
                if let Some(nested_fields) =
                    self.variable_nested_object_fields.get(&ident.name).cloned()
                {
                    for (field_idx, layout) in nested_fields {
                        self.register_nested_object_fields
                            .insert((dest.id, field_idx), layout);
                    }
                }
            }
            self.propagate_variable_projection_to_register(ident.name, &dest);
            return dest;
        }

        // Check if we've already captured this variable
        if let Some(idx) = self.captures.iter().position(|c| c.symbol == ident.name) {
            let ty = self.captures[idx].ty;
            let is_refcell = self.captures[idx].is_refcell;
            let capture_idx = self.captures[idx].capture_idx;

            if is_refcell {
                // Load the RefCell pointer from captured
                let refcell_ty = TypeId::new(NUMBER_TYPE_ID);
                let refcell_reg = self.alloc_register(refcell_ty);
                self.emit(IrInstr::LoadCaptured {
                    dest: refcell_reg.clone(),
                    index: capture_idx,
                });
                // Load the value from the RefCell
                let dest = self.alloc_register(ty);
                self.emit(IrInstr::LoadRefCell {
                    dest: dest.clone(),
                    refcell: refcell_reg,
                });
                return dest;
            } else {
                let dest = self.alloc_register(ty);
                self.emit(IrInstr::LoadCaptured {
                    dest: dest.clone(),
                    index: capture_idx,
                });
                self.propagate_variable_projection_to_register(ident.name, &dest);
                return dest;
            }
        }

        // Check ancestor variables (from enclosing scopes)
        if let Some(ref ancestors) = self.ancestor_variables {
            if let Some(ancestor_var) = ancestors.get(&ident.name) {
                // Variable is from an enclosing scope - capture it
                let ty = ancestor_var.ty;
                let is_refcell = ancestor_var.is_refcell;
                let capture_idx = self.next_capture_slot;
                self.next_capture_slot += 1;
                self.captures.push(super::CaptureInfo {
                    symbol: ident.name,
                    source: ancestor_var.source,
                    capture_idx,
                    ty,
                    is_refcell,
                });

                if is_refcell {
                    // Load the RefCell pointer from captured
                    let refcell_ty = TypeId::new(NUMBER_TYPE_ID);
                    let refcell_reg = self.alloc_register(refcell_ty);
                    self.emit(IrInstr::LoadCaptured {
                        dest: refcell_reg.clone(),
                        index: capture_idx,
                    });
                    // Load the value from the RefCell
                    let dest = self.alloc_register(ty);
                    self.emit(IrInstr::LoadRefCell {
                        dest: dest.clone(),
                        refcell: refcell_reg,
                    });
                    return dest;
                } else {
                    let dest = self.alloc_register(ty);
                    self.emit(IrInstr::LoadCaptured {
                        dest: dest.clone(),
                        index: capture_idx,
                    });
                    self.propagate_variable_projection_to_register(ident.name, &dest);
                    return dest;
                }
            }
        }

        // Check module-level variables (stored as globals)
        if let Some(&global_idx) = self.module_var_globals.get(&ident.name) {
            let ty = self
                .global_type_map
                .get(&global_idx)
                .copied()
                .unwrap_or(UNRESOLVED);
            let dest = self.alloc_register(ty);
            self.emit(IrInstr::LoadGlobal {
                dest: dest.clone(),
                index: global_idx,
            });
            // Propagate object field layout so destructuring can resolve field names
            if let Some(fields) = self.variable_object_fields.get(&ident.name).cloned() {
                self.register_object_fields.insert(dest.id, fields);
                if let Some(nested_fields) =
                    self.variable_nested_object_fields.get(&ident.name).cloned()
                {
                    for (field_idx, layout) in nested_fields {
                        self.register_nested_object_fields
                            .insert((dest.id, field_idx), layout);
                    }
                }
            }
            self.propagate_variable_projection_to_register(ident.name, &dest);
            return dest;
        }

        // Nested class method environment bridge:
        // std-wrapper class methods can reference enclosing wrapper locals.
        if let Some(env_globals) = &self.current_method_env_globals {
            if let Some(binding) = env_globals.get(&ident.name).copied() {
                let ty = self
                    .global_type_map
                    .get(&binding.global_idx)
                    .copied()
                    .unwrap_or(UNRESOLVED);
                let dest = if binding.is_refcell {
                    let refcell_reg = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
                    self.emit(IrInstr::LoadGlobal {
                        dest: refcell_reg.clone(),
                        index: binding.global_idx,
                    });
                    let value_reg = self.alloc_register(ty);
                    self.emit(IrInstr::LoadRefCell {
                        dest: value_reg.clone(),
                        refcell: refcell_reg,
                    });
                    value_reg
                } else {
                    let value_reg = self.alloc_register(ty);
                    self.emit(IrInstr::LoadGlobal {
                        dest: value_reg.clone(),
                        index: binding.global_idx,
                    });
                    value_reg
                };
                return dest;
            }
        }

        // Check if this is a named function used as a value (function reference)
        if let Some(&func_id) = self.function_map.get(&ident.name) {
            let dest = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
            self.emit(IrInstr::MakeClosure {
                dest: dest.clone(),
                func: func_id,
                captures: vec![],
            });
            return dest;
        }

        // Class identifiers may legitimately appear in value position
        // (e.g., class aliasing/import scaffolding). Class values are resolved
        // through class maps at use sites (new/static/member), not as plain locals.
        if let Some(&nominal_type_id) = self.class_map.get(&ident.name) {
            let dest = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
            self.emit(IrInstr::Assign {
                dest: dest.clone(),
                value: IrValue::Constant(IrConstant::I32(nominal_type_id.as_u32() as i32)),
            });
            return dest;
        }

        // Ambient builtin globals (seeded by declaration surfaces) are resolved at runtime
        // through a dedicated native lookup and do not require source-level declarations.
        if self.ambient_builtin_globals.contains(name) {
            let expr_ty = self
                .expr_types_by_span
                .get(&(ident.span.start, ident.span.end))
                .copied()
                .or_else(|| self.type_ctx.lookup_named_type(name))
                .unwrap_or(UNRESOLVED);
            let dest = self.alloc_register(expr_ty);
            let name_reg = self.alloc_register(TypeId::new(STRING_TYPE_ID));
            self.emit(IrInstr::Assign {
                dest: name_reg.clone(),
                value: IrValue::Constant(IrConstant::String(name.to_string())),
            });
            self.emit(IrInstr::NativeCall {
                dest: Some(dest.clone()),
                native_id: crate::compiler::native_id::OBJECT_GET_AMBIENT_GLOBAL,
                args: vec![name_reg],
            });
            return dest;
        }

        // Unknown identifier: do not silently lower to null.
        // Keep lowering moving, but surface a hard compile error.
        if std::env::var("RAYA_DEBUG_UNRESOLVED_IDENT").is_ok() {
            let fn_name = self
                .current_function
                .as_ref()
                .map(|f| f.name.clone())
                .unwrap_or_else(|| "<none>".to_string());
            let current_class = self
                .current_class
                .map(|cid| cid.as_u32().to_string())
                .unwrap_or_else(|| "none".to_string());
            let has_local = self.local_map.contains_key(&ident.name);
            let has_capture = self.captures.iter().any(|c| c.symbol == ident.name);
            let has_ancestor = self
                .ancestor_variables
                .as_ref()
                .is_some_and(|m| m.contains_key(&ident.name));
            let has_module_global = self.module_var_globals.contains_key(&ident.name);
            let has_function = self.function_map.contains_key(&ident.name);
            eprintln!(
                "[lower][unresolved-ident] name={} fn={} class={} local={} capture={} ancestor={} module_global={} function={} class_map={}",
                self.interner.resolve(ident.name),
                fn_name,
                current_class,
                has_local,
                has_capture,
                has_ancestor,
                has_module_global,
                has_function,
                self.class_map.contains_key(&ident.name)
            );
        }
        self.errors
            .push(crate::compiler::CompileError::InternalError {
                message: format!(
                    "unresolved identifier '{}': no local, captured, module-global, or function binding (class_map_contains={})",
                    self.interner.resolve(ident.name),
                    self.class_map.contains_key(&ident.name)
                ),
            });
        self.lower_unresolved_poison()
    }

    fn lower_binary(&mut self, binary: &ast::BinaryExpression) -> Register {
        let left = self.lower_expr(&binary.left);
        let right = self.lower_expr(&binary.right);

        let mut op = self.convert_binary_op(&binary.operator);
        if matches!(op, BinaryOp::Add)
            && (self.is_string_like_type(left.ty) || self.is_string_like_type(right.ty))
        {
            op = BinaryOp::Concat;
        }
        let result_ty = if matches!(op, BinaryOp::Concat) {
            TypeId::new(STRING_TYPE_ID)
        } else {
            self.infer_binary_result_type(&op, &left, &right)
        };
        let dest = self.alloc_register(result_ty);

        self.emit(IrInstr::BinaryOp {
            dest: dest.clone(),
            op,
            left,
            right,
        });
        dest
    }

    fn lower_unary(&mut self, unary: &ast::UnaryExpression) -> Register {
        // Handle increment/decrement operators specially — they need to
        // compute new_value = old ± 1 and store back to the variable
        match unary.operator {
            ast::UnaryOperator::PostfixIncrement
            | ast::UnaryOperator::PrefixIncrement
            | ast::UnaryOperator::PostfixDecrement
            | ast::UnaryOperator::PrefixDecrement => {
                return self.lower_increment_decrement(unary);
            }
            // void expr: evaluate for side-effects, return null
            ast::UnaryOperator::Void => {
                let _operand = self.lower_expr(&unary.operand);
                let dest = self.alloc_register(TypeId::new(NULL_TYPE_ID));
                self.emit(IrInstr::Assign {
                    dest: dest.clone(),
                    value: IrValue::Constant(IrConstant::Null),
                });
                return dest;
            }
            // delete obj.prop: remove field from object, return true
            // delete on non-member expressions is a no-op returning true (per spec)
            ast::UnaryOperator::Delete => {
                return self.lower_delete(unary);
            }
            _ => {}
        }

        let operand = self.lower_expr(&unary.operand);
        let op = self.convert_unary_op(&unary.operator);
        let dest = self.alloc_register(operand.ty);

        self.emit(IrInstr::UnaryOp {
            dest: dest.clone(),
            op,
            operand,
        });
        dest
    }

    fn lower_delete(&mut self, unary: &ast::UnaryExpression) -> Register {
        let bool_ty = TypeId::new(BOOLEAN_TYPE_ID);
        // delete on member expressions: set the property to null (pragmatic delete)
        if let Expression::Member(member) = &*unary.operand {
            let prop_name = self.interner.resolve(member.property.name).to_string();
            let nominal_type_id = if self.prefers_structural_member_projection(&member.object) {
                None
            } else {
                self.infer_nominal_type_id(&member.object)
            };
            let object = self.lower_expr(&member.object);
            let obj_ty_id = {
                let reg_ty = object.ty.as_u32();
                let checker_ty = self.get_expr_type(&member.object).as_u32();
                let reg_dispatch = self.normalize_type_for_dispatch(reg_ty);
                let checker_dispatch = self.normalize_type_for_dispatch(checker_ty);
                if reg_dispatch != UNRESOLVED_TYPE_ID && reg_dispatch != UNKNOWN_TYPE_ID {
                    reg_dispatch
                } else if checker_dispatch != UNRESOLVED_TYPE_ID
                    && checker_dispatch != UNKNOWN_TYPE_ID
                {
                    checker_dispatch
                } else {
                    UNRESOLVED_TYPE_ID
                }
            };

            // Create a null value to assign
            let null_reg = self.alloc_register(TypeId::new(NULL_TYPE_ID));
            self.emit(IrInstr::Assign {
                dest: null_reg.clone(),
                value: IrValue::Constant(IrConstant::Null),
            });

            // If we know the class, resolve field index and use StoreFieldExact
            if let Some(cid) = nominal_type_id {
                let all_fields = self.get_all_fields(cid);
                if let Some(field) = all_fields
                    .iter()
                    .rev()
                    .find(|f| self.interner.resolve(f.name) == prop_name)
                {
                    self.emit(IrInstr::StoreFieldExact {
                        object,
                        field: field.index,
                        value: null_reg,
                    });
                } else {
                    let class_name = self
                        .class_map
                        .iter()
                        .find_map(|(&sym, &id)| {
                            if id == cid {
                                Some(self.interner.resolve(sym).to_string())
                            } else {
                                None
                            }
                        })
                        .unwrap_or_else(|| format!("class#{}", cid.as_u32()));
                    self.errors
                        .push(crate::compiler::CompileError::InternalError {
                            message: format!(
                                "cannot delete unknown class field '{}.{}'",
                                class_name, prop_name
                            ),
                        });
                }
            } else if let Some((_, field_idx)) =
                self.structural_shape_slot_from_expr(&member.object, &prop_name)
            {
                self.emit_member_store(&member.object, object, field_idx, &prop_name, null_reg);
            } else if let Some(field_idx) = match &*member.object {
                Expression::Identifier(ident) => self
                    .variable_object_fields
                    .get(&ident.name)
                    .and_then(|fields| {
                        fields
                            .iter()
                            .find(|(name, _)| name == &prop_name)
                            .map(|(_, idx)| *idx as u16)
                    }),
                _ => self
                    .register_object_fields
                    .get(&object.id)
                    .and_then(|fields| {
                        fields
                            .iter()
                            .find(|(name, _)| name == &prop_name)
                            .map(|(_, idx)| *idx as u16)
                    }),
            } {
                self.emit_member_store(&member.object, object, field_idx, &prop_name, null_reg);
            } else if obj_ty_id == JSON_TYPE_ID || obj_ty_id == JSON_OBJECT_TYPE_ID {
                self.emit(IrInstr::DynSetProp {
                    object,
                    property: prop_name,
                    value: null_reg,
                });
            } else {
                self.errors
                    .push(crate::compiler::CompileError::InternalError {
                        message: format!(
                            "cannot delete unresolved member '{}': no class field or JSON receiver",
                            prop_name
                        ),
                    });
            }

            // Return true (delete always succeeds in our implementation)
            let dest = self.alloc_register(bool_ty);
            self.emit(IrInstr::Assign {
                dest: dest.clone(),
                value: IrValue::Constant(IrConstant::Boolean(true)),
            });
            return dest;
        }
        // delete on non-member expressions: evaluate for side-effects, return true
        let _operand = self.lower_expr(&unary.operand);
        let dest = self.alloc_register(bool_ty);
        self.emit(IrInstr::Assign {
            dest: dest.clone(),
            value: IrValue::Constant(IrConstant::Boolean(true)),
        });
        dest
    }

    fn lower_increment_decrement(&mut self, unary: &ast::UnaryExpression) -> Register {
        let is_increment = matches!(
            unary.operator,
            ast::UnaryOperator::PostfixIncrement | ast::UnaryOperator::PrefixIncrement
        );
        let is_prefix = matches!(
            unary.operator,
            ast::UnaryOperator::PrefixIncrement | ast::UnaryOperator::PrefixDecrement
        );

        // Lower operand to get current value
        let old_value = self.lower_expr(&unary.operand);

        // Create the ±1 constant based on operand type
        let int_ty = TypeId::new(INT_TYPE_ID);
        let one = if old_value.ty == int_ty {
            let r = self.alloc_register(int_ty);
            self.emit(IrInstr::Assign {
                dest: r.clone(),
                value: IrValue::Constant(IrConstant::I32(1)),
            });
            r
        } else {
            let num_ty = TypeId::new(NUMBER_TYPE_ID);
            let r = self.alloc_register(num_ty);
            self.emit(IrInstr::Assign {
                dest: r.clone(),
                value: IrValue::Constant(IrConstant::F64(1.0)),
            });
            r
        };

        // Compute new value: old ± 1
        let new_value = self.alloc_register(old_value.ty);
        let op = if is_increment {
            BinaryOp::Add
        } else {
            BinaryOp::Sub
        };
        self.emit(IrInstr::BinaryOp {
            dest: new_value.clone(),
            op,
            left: old_value.clone(),
            right: one,
        });

        // Store new value back to the variable
        if let Expression::Identifier(ident) = &*unary.operand {
            if let Some(&local_idx) = self.local_map.get(&ident.name) {
                self.emit(IrInstr::StoreLocal {
                    index: local_idx,
                    value: new_value.clone(),
                });
            } else if let Some(&global_idx) = self.module_var_globals.get(&ident.name) {
                self.emit(IrInstr::StoreGlobal {
                    index: global_idx,
                    value: new_value.clone(),
                });
            } else if let Some(binding) = self
                .current_method_env_globals
                .as_ref()
                .and_then(|m| m.get(&ident.name))
                .copied()
            {
                if binding.is_refcell {
                    let refcell_reg = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
                    self.emit(IrInstr::LoadGlobal {
                        dest: refcell_reg.clone(),
                        index: binding.global_idx,
                    });
                    self.emit(IrInstr::StoreRefCell {
                        refcell: refcell_reg,
                        value: new_value.clone(),
                    });
                } else {
                    self.emit(IrInstr::StoreGlobal {
                        index: binding.global_idx,
                        value: new_value.clone(),
                    });
                }
            }
        }

        // Return old value for postfix, new value for prefix
        if is_prefix {
            new_value
        } else {
            old_value
        }
    }

    fn lower_call(&mut self, call: &ast::CallExpression, full_expr: &Expression) -> Register {
        // Lower arguments first
        let args: Vec<Register> = call.arguments.iter().map(|a| self.lower_expr(a)).collect();

        // Use the type checker's computed return type for this call expression
        let call_ty = self.get_expr_type(full_expr);
        let mut dest = self.alloc_register(call_ty);

        // Handle super() constructor call
        if let Expression::Super(_) = &*call.callee {
            if let Some(current_nominal_type_id) = self.current_class {
                // Get parent class
                if let Some(parent_id) = self
                    .class_info_map
                    .get(&current_nominal_type_id)
                    .and_then(|info| info.parent_class)
                {
                    // Get parent's constructor
                    if let Some(parent_ctor) = self
                        .class_info_map
                        .get(&parent_id)
                        .and_then(|info| info.constructor)
                    {
                        // Call parent constructor with 'this' as first argument
                        let mut ctor_args = vec![self.lower_this()];
                        ctor_args.extend(args);
                        self.emit(IrInstr::Call {
                            dest: None, // Constructor doesn't return
                            func: parent_ctor,
                            args: ctor_args,
                        });
                        self.emit_pending_constructor_prologue_if_needed();
                    }
                }
            }
            return dest;
        }

        // Handle super.method() call
        if let Expression::Member(member) = &*call.callee {
            if let Expression::Super(_) = &*member.object {
                let method_name_symbol = member.property.name;
                if let Some(current_nominal_type_id) = self.current_class {
                    // Get parent class
                    if let Some(parent_id) = self
                        .class_info_map
                        .get(&current_nominal_type_id)
                        .and_then(|info| info.parent_class)
                    {
                        // Look up method in parent class
                        if let Some(&parent_method_id) =
                            self.method_map.get(&(parent_id, method_name_symbol))
                        {
                            // Call parent method with 'this' as first argument
                            let mut method_args = vec![self.lower_this()];
                            method_args.extend(args);
                            self.emit(IrInstr::Call {
                                dest: Some(dest.clone()),
                                func: parent_method_id,
                                args: method_args,
                            });
                            return dest;
                        }
                    }
                }
            }
        }

        // Node-compat JS semantics: explicit method binding via `.bind(thisArg)`.
        // Example: `let f = obj.method.bind(obj);`
        if self.js_this_binding_compat {
            if let Expression::Member(bind_member) = &*call.callee {
                let bind_name = self.interner.resolve(bind_member.property.name);
                if bind_name == "bind" {
                    if let Expression::Member(target_member) = &*bind_member.object {
                        if let Some(nominal_type_id) = self.infer_nominal_type_id(&target_member.object) {
                            if let Some(&slot) = self
                                .method_slot_map
                                .get(&(nominal_type_id, target_member.property.name))
                            {
                                // Only handle receiver binding here.
                                // Partial argument binding still follows existing generic path.
                                if args.len() > 1 {
                                    // Fall through to normal lowering.
                                } else {
                                    let receiver = if let Some(first) = args.first() {
                                        first.clone()
                                    } else {
                                        self.lower_expr(&target_member.object)
                                    };
                                    self.emit(IrInstr::BindMethod {
                                        dest: dest.clone(),
                                        object: receiver,
                                        method: slot,
                                    });
                                    return dest;
                                }
                            }
                        }
                    }
                }
            }
        }

        if let Expression::Identifier(ident) = &*call.callee {
            // Check for builtin functions/intrinsics first
            let name = self.interner.resolve(ident.name);

            // Primitive/global coercion helpers available in strict mode.
            if name == "Boolean" {
                if let Some(value) = args.first().cloned() {
                    let not_val = self.alloc_register(TypeId::new(BOOLEAN_TYPE_ID));
                    self.emit(IrInstr::UnaryOp {
                        dest: not_val.clone(),
                        op: UnaryOp::Not,
                        operand: value,
                    });
                    self.emit(IrInstr::UnaryOp {
                        dest: dest.clone(),
                        op: UnaryOp::Not,
                        operand: not_val,
                    });
                } else {
                    self.emit(IrInstr::Assign {
                        dest: dest.clone(),
                        value: IrValue::Constant(IrConstant::Boolean(false)),
                    });
                }
                return dest;
            }
            if name == "Number" {
                if let Some(value) = args.first().cloned() {
                    self.emit(IrInstr::NativeCall {
                        dest: Some(dest.clone()),
                        native_id: crate::vm::builtin::number::PARSE_FLOAT,
                        args: vec![value],
                    });
                } else {
                    self.emit(IrInstr::Assign {
                        dest: dest.clone(),
                        value: IrValue::Constant(IrConstant::I32(0)),
                    });
                }
                return dest;
            }
            if name == "String" {
                if let Some(value) = args.first().cloned() {
                    self.emit(IrInstr::ToString {
                        dest: dest.clone(),
                        operand: value,
                    });
                } else {
                    self.emit(IrInstr::Assign {
                        dest: dest.clone(),
                        value: IrValue::Constant(IrConstant::String(String::new())),
                    });
                }
                return dest;
            }
            if name == "encodeURI" || name == "encodeURIComponent" {
                self.emit(IrInstr::NativeCall {
                    dest: Some(dest.clone()),
                    native_id: crate::vm::builtin::url::ENCODE,
                    args,
                });
                return dest;
            }
            if name == "decodeURI" || name == "decodeURIComponent" {
                self.emit(IrInstr::NativeCall {
                    dest: Some(dest.clone()),
                    native_id: crate::vm::builtin::url::DECODE,
                    args,
                });
                return dest;
            }

            // Handle __NATIVE_CALL intrinsic: __NATIVE_CALL(native_id, args...)
            // First argument can be:
            //   - StringLiteral: symbolic name → ModuleNativeCall (stdlib)
            //   - IntLiteral/Identifier: numeric ID → NativeCall (engine-internal)
            if name == "__NATIVE_CALL" {
                if let Some(first_arg) = call.arguments.first() {
                    // Check for string literal first → ModuleNativeCall
                    if let Expression::StringLiteral(lit) = first_arg {
                        let fn_name = self.interner.resolve(lit.value);
                        let local_idx = self.resolve_native_name(fn_name);

                        let native_args: Vec<Register> = call.arguments[1..]
                            .iter()
                            .map(|a| self.lower_expr(a))
                            .collect();

                        self.emit(IrInstr::ModuleNativeCall {
                            dest: Some(dest.clone()),
                            local_idx,
                            args: native_args,
                        });
                        // Apply type argument to dest register if provided
                        self.apply_native_call_type_args(call, &mut dest);
                        return dest;
                    }

                    // Numeric ID → NativeCall (engine-internal: reflect, runtime, builtins)
                    let native_id = match first_arg {
                        Expression::IntLiteral(lit) => lit.value as u16,
                        Expression::Identifier(id_expr) => {
                            // Look up compile-time constant value
                            if let Some(const_val) = self.constant_map.get(&id_expr.name) {
                                match const_val {
                                    ConstantValue::I64(v) => *v as u16,
                                    ConstantValue::F64(v) => *v as u16,
                                    _ => {
                                        let name = self.interner.resolve(id_expr.name);
                                        eprintln!(
                                            "Warning: __NATIVE_CALL constant '{}' is not a number",
                                            name
                                        );
                                        0
                                    }
                                }
                            } else {
                                let name = self.interner.resolve(id_expr.name);
                                eprintln!("Warning: __NATIVE_CALL identifier '{}' is not a compile-time constant", name);
                                0
                            }
                        }
                        _ => {
                            eprintln!("Warning: __NATIVE_CALL first argument must be a string literal, integer literal, or constant");
                            0
                        }
                    };

                    // Lower remaining arguments (skip the native_id)
                    let native_args: Vec<Register> = call.arguments[1..]
                        .iter()
                        .map(|a| self.lower_expr(a))
                        .collect();

                    self.emit(IrInstr::NativeCall {
                        dest: Some(dest.clone()),
                        native_id,
                        args: native_args,
                    });
                    // Apply type argument to dest register if provided
                    self.apply_native_call_type_args(call, &mut dest);
                    return dest;
                }
            }

            // Handle __OPCODE_CHANNEL_NEW intrinsic: __OPCODE_CHANNEL_NEW(capacity)
            if name == "__OPCODE_CHANNEL_NEW" {
                let capacity = if !call.arguments.is_empty() {
                    self.lower_expr(&call.arguments[0])
                } else {
                    let zero_reg = self.alloc_register(TypeId::new(STRING_TYPE_ID));
                    self.emit(IrInstr::Assign {
                        dest: zero_reg.clone(),
                        value: IrValue::Constant(IrConstant::I32(0)),
                    });
                    zero_reg
                };
                self.emit(IrInstr::NewChannel {
                    dest: dest.clone(),
                    capacity,
                });
                return dest;
            }

            // Handle __OPCODE_MUTEX_NEW intrinsic: creates a new mutex handle
            if name == "__OPCODE_MUTEX_NEW" {
                self.emit(IrInstr::NewMutex { dest: dest.clone() });
                return dest;
            }

            // Handle __OPCODE_MUTEX_LOCK intrinsic: acquires mutex lock (blocking)
            if name == "__OPCODE_MUTEX_LOCK" {
                if !call.arguments.is_empty() {
                    let mutex = self.lower_expr(&call.arguments[0]);
                    self.emit(IrInstr::MutexLock { mutex });
                }
                return dest;
            }

            // Handle __OPCODE_MUTEX_UNLOCK intrinsic: releases mutex lock
            if name == "__OPCODE_MUTEX_UNLOCK" {
                if !call.arguments.is_empty() {
                    let mutex = self.lower_expr(&call.arguments[0]);
                    self.emit(IrInstr::MutexUnlock { mutex });
                }
                return dest;
            }

            // Handle __OPCODE_TASK_CANCEL intrinsic: cancels a running task
            if name == "__OPCODE_TASK_CANCEL" {
                if !call.arguments.is_empty() {
                    let task = self.lower_expr(&call.arguments[0]);
                    self.emit(IrInstr::TaskCancel { task });
                }
                return dest;
            }

            // Handle __OPCODE_YIELD intrinsic: yields execution to scheduler
            if name == "__OPCODE_YIELD" {
                self.emit(IrInstr::Yield);
                return dest;
            }

            // Handle __OPCODE_SLEEP intrinsic: sleeps for specified milliseconds
            if name == "__OPCODE_SLEEP" {
                if !call.arguments.is_empty() {
                    let duration_ms = self.lower_expr(&call.arguments[0]);
                    self.emit(IrInstr::Sleep { duration_ms });
                }
                return dest;
            }

            // Handle __OPCODE_ARRAY_LEN intrinsic: gets array length
            if name == "__OPCODE_ARRAY_LEN" {
                if !call.arguments.is_empty() {
                    let array = self.lower_expr(&call.arguments[0]);
                    self.emit(IrInstr::ArrayLen {
                        dest: dest.clone(),
                        array,
                    });
                }
                return dest;
            }

            // Handle __OPCODE_ARRAY_PUSH intrinsic: pushes element to array
            if name == "__OPCODE_ARRAY_PUSH" {
                if call.arguments.len() >= 2 {
                    let array = self.lower_expr(&call.arguments[0]);
                    let element = self.lower_expr(&call.arguments[1]);
                    self.emit(IrInstr::ArrayPush { array, element });
                }
                return dest;
            }

            // Handle __OPCODE_ARRAY_POP intrinsic: pops element from array
            if name == "__OPCODE_ARRAY_POP" {
                if !call.arguments.is_empty() {
                    let array = self.lower_expr(&call.arguments[0]);
                    self.emit(IrInstr::ArrayPop {
                        dest: dest.clone(),
                        array,
                    });
                }
                return dest;
            }

            // Handle __OPCODE_AWAIT intrinsic variants.
            // The checker may emit specialized names (e.g. __OPCODE_AWAIT_Promise).
            if name == "__OPCODE_AWAIT" || name.starts_with("__OPCODE_AWAIT_") {
                if !args.is_empty() {
                    self.emit(IrInstr::Await {
                        dest: dest.clone(),
                        task: args[0].clone(),
                    });
                }
                return dest;
            }

            // Handle __OPCODE_AWAIT_ALL intrinsic variants.
            if name == "__OPCODE_AWAIT_ALL" || name.starts_with("__OPCODE_AWAIT_ALL_") {
                if !args.is_empty() {
                    self.emit(IrInstr::AwaitAll {
                        dest: dest.clone(),
                        tasks: args[0].clone(),
                    });
                }
                return dest;
            }

            if name == "sleep" {
                // sleep(ms) - emit Sleep instruction
                if !args.is_empty() {
                    self.emit(IrInstr::Sleep {
                        duration_ms: args[0].clone(),
                    });
                }
                return dest;
            }

            // Global number utility functions
            if name == "parseInt" || name == "parseFloat" || name == "isNaN" || name == "isFinite" {
                let native_id = match name {
                    "parseInt" => crate::vm::builtin::number::PARSE_INT,
                    "parseFloat" => crate::vm::builtin::number::PARSE_FLOAT,
                    "isNaN" => crate::vm::builtin::number::IS_NAN,
                    "isFinite" => crate::vm::builtin::number::IS_FINITE,
                    _ => unreachable!(),
                };
                self.emit(IrInstr::NativeCall {
                    dest: Some(dest.clone()),
                    native_id,
                    args,
                });
                return dest;
            }

            let is_captured = self.captures.iter().any(|c| c.symbol == ident.name);
            let is_ancestor = self
                .ancestor_variables
                .as_ref()
                .is_some_and(|m| m.contains_key(&ident.name));

            // Check if it's a direct function call. Prefer closure/capture paths when
            // the identifier is locally bound or captured so lexical environments stay intact.
            if !self.local_map.contains_key(&ident.name)
                && !is_captured
                && !is_ancestor
                && !self.module_var_globals.contains_key(&ident.name)
                && !self
                    .current_method_env_globals
                    .as_ref()
                    .is_some_and(|m| m.contains_key(&ident.name))
            {
                if let Some(&func_id) = self.function_map.get(&ident.name) {
                    if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                        eprintln!(
                            "[lower] direct call '{}' -> func_id={}",
                            self.interner.resolve(ident.name),
                            func_id.as_u32()
                        );
                    }
                    // Call-site specialization: only for generic functions with constrained type
                    // parameters (e.g., T extends HasLength). Unconstrained generics are handled
                    // correctly by the normal monomorphization pipeline.
                    let effective_func_id = if call.type_args.is_some() {
                        let needs_specialization =
                            self.generic_function_asts
                                .get(&ident.name)
                                .map(|func_ast| {
                                    func_ast.type_params.as_ref().is_some_and(|tps| {
                                        tps.iter().any(|tp| tp.constraint.is_some())
                                    })
                                })
                                .unwrap_or(false);
                        if needs_specialization {
                            self.specialize_generic_function(ident.name, call)
                                .unwrap_or(func_id)
                        } else {
                            func_id
                        }
                    } else {
                        func_id
                    };

                    // Check if this is an async function - emit Spawn instead of Call
                    if self.async_functions.contains(&effective_func_id) {
                        // Use proper Task type for the destination register
                        let task_ty = self
                            .type_ctx
                            .generic_task_type()
                            .unwrap_or(TypeId::new(TASK_TYPE_ID));
                        let task_dest = self.alloc_register(task_ty);
                        self.emit(IrInstr::Spawn {
                            dest: task_dest.clone(),
                            func: effective_func_id,
                            args,
                        });
                        return task_dest;
                    } else {
                        self.emit(IrInstr::Call {
                            dest: Some(dest.clone()),
                            func: effective_func_id,
                            args,
                        });
                        self.propagate_type_projection_to_register(call_ty, &dest);
                    }
                    return dest;
                }
            }

            // Otherwise, it might be a closure stored in a variable
            if let Some(&local_idx) = self.local_map.get(&ident.name) {
                if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                    eprintln!(
                        "[lower] closure call '{}' via local idx={}",
                        self.interner.resolve(ident.name),
                        local_idx
                    );
                }
                // Load the closure from the local variable.
                // Do not silently coerce missing local type metadata to number; fail loudly.
                let closure_ty = if let Some(reg) = self.local_registers.get(&local_idx) {
                    reg.ty
                } else {
                    self.errors
                        .push(crate::compiler::CompileError::InternalError {
                            message: format!(
                                "internal error: missing local register metadata for callable '{}'",
                                self.interner.resolve(ident.name)
                            ),
                        });
                    self.poison_register(&dest);
                    return dest;
                };
                let callee_ty = self.get_expr_type(&call.callee);
                let callee_ty_raw = callee_ty.as_u32();
                let known_callable = self.closure_locals.contains_key(&local_idx)
                    || self.callable_local_hints.contains(&local_idx)
                    || self.callable_symbol_hints.contains(&ident.name)
                    || self.bound_method_vars.contains_key(&ident.name);
                if !known_callable && !self.type_is_callable(callee_ty) {
                    self.errors
                        .push(crate::compiler::CompileError::InternalError {
                            message: format!(
                            "unresolved call target '{}': local value is not callable (type id {})",
                            self.interner.resolve(ident.name),
                            callee_ty_raw
                        ),
                        });
                    self.poison_register(&dest);
                    return dest;
                }
                let closure_raw = self.alloc_register(closure_ty);
                self.emit(IrInstr::LoadLocal {
                    dest: closure_raw.clone(),
                    index: local_idx,
                });

                // Unwrap RefCell if the variable is captured and externally modified
                let closure = if self.refcell_registers.contains_key(&local_idx) {
                    let val = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
                    self.emit(IrInstr::LoadRefCell {
                        dest: val.clone(),
                        refcell: closure_raw,
                    });
                    val
                } else {
                    closure_raw
                };

                // Check if this is an async closure (spawns a Task)
                if let Some(&func_id) = self.closure_locals.get(&local_idx) {
                    if self.async_closures.contains(&func_id) {
                        // Emit SpawnClosure instead of CallClosure for async closures
                        if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                            eprintln!(
                                "[lower] SpawnClosure[local-async] '{}' local_idx={} func_id={}",
                                self.interner.resolve(ident.name),
                                local_idx,
                                func_id.as_u32()
                            );
                        }
                        self.emit(IrInstr::SpawnClosure {
                            dest: dest.clone(),
                            closure,
                            args,
                        });
                        return dest;
                    }
                }

                // Regular closure call
                if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                    eprintln!(
                        "[lower] CallClosure[local] '{}'",
                        self.interner.resolve(ident.name)
                    );
                }
                self.emit(IrInstr::CallClosure {
                    dest: Some(dest.clone()),
                    closure,
                    args,
                });

                // Propagate return type for bound method calls
                if let Some(&(nominal_type_id, method_name)) = self.bound_method_vars.get(&ident.name) {
                    if let Some(&ret_ty) = self.method_return_type_map.get(&(nominal_type_id, method_name))
                    {
                        if ret_ty != UNRESOLVED {
                            dest.ty = ret_ty;
                        }
                    }
                }
                self.propagate_type_projection_to_register(dest.ty, &dest);

                return dest;
            }

            // Check for closure stored in a module-level global variable
            if let Some(&global_idx) = self.module_var_globals.get(&ident.name) {
                let closure_ty = self.get_expr_type(&call.callee);
                let closure_ty_raw = closure_ty.as_u32();
                let known_callable = self.closure_globals.contains_key(&global_idx)
                    || self.callable_symbol_hints.contains(&ident.name)
                    || self.bound_method_vars.contains_key(&ident.name);
                if !known_callable && !self.type_is_callable(closure_ty) {
                    self.errors
                        .push(crate::compiler::CompileError::InternalError {
                            message: format!(
                                "unresolved call target '{}': module/global value is not callable (type id {})",
                                self.interner.resolve(ident.name),
                                closure_ty_raw
                            ),
                        });
                    self.poison_register(&dest);
                    return dest;
                }
                let closure = self.alloc_register(closure_ty);
                self.emit(IrInstr::LoadGlobal {
                    dest: closure.clone(),
                    index: global_idx,
                });
                if let Some(&func_id) = self.closure_globals.get(&global_idx) {
                    if self.async_closures.contains(&func_id) {
                        if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                            eprintln!(
                                "[lower] SpawnClosure[global-async] '{}' global_idx={} func_id={}",
                                self.interner.resolve(ident.name),
                                global_idx,
                                func_id.as_u32()
                            );
                        }
                        self.emit(IrInstr::SpawnClosure {
                            dest: dest.clone(),
                            closure,
                            args,
                        });
                    } else {
                        self.emit(IrInstr::CallClosure {
                            dest: Some(dest.clone()),
                            closure,
                            args,
                        });
                    }
                } else {
                    self.emit(IrInstr::CallClosure {
                        dest: Some(dest.clone()),
                        closure,
                        args,
                    });
                }

                // Propagate return type for bound method calls (global variable path)
                if let Some(&(nominal_type_id, method_name)) = self.bound_method_vars.get(&ident.name) {
                    if let Some(&ret_ty) = self.method_return_type_map.get(&(nominal_type_id, method_name))
                    {
                        if ret_ty != UNRESOLVED {
                            dest.ty = ret_ty;
                        }
                    }
                }
                self.propagate_type_projection_to_register(dest.ty, &dest);

                return dest;
            }

            // Nested class-method environment bridge call path.
            // std-wrapper class methods can call enclosing wrapper locals/functions
            // that were materialized into dedicated globals.
            if let Some(binding) = self
                .current_method_env_globals
                .as_ref()
                .and_then(|m| m.get(&ident.name))
                .copied()
            {
                let closure_ty = self.get_expr_type(&call.callee);
                let closure_ty_raw = closure_ty.as_u32();
                let known_callable = self.closure_globals.contains_key(&binding.global_idx)
                    || self.callable_symbol_hints.contains(&ident.name)
                    || self.bound_method_vars.contains_key(&ident.name)
                    || self.function_map.contains_key(&ident.name);
                if !known_callable && !self.type_is_callable(closure_ty) {
                    self.errors
                        .push(crate::compiler::CompileError::InternalError {
                            message: format!(
                                "unresolved call target '{}': bridged method-env value is not callable (type id {})",
                                self.interner.resolve(ident.name),
                                closure_ty_raw
                            ),
                        });
                    self.poison_register(&dest);
                    return dest;
                }

                let mut closure = self.alloc_register(closure_ty);
                self.emit(IrInstr::LoadGlobal {
                    dest: closure.clone(),
                    index: binding.global_idx,
                });
                if binding.is_refcell {
                    let value = self.alloc_register(closure_ty);
                    self.emit(IrInstr::LoadRefCell {
                        dest: value.clone(),
                        refcell: closure,
                    });
                    closure = value;
                }

                if let Some(&func_id) = self.closure_globals.get(&binding.global_idx) {
                    if self.async_closures.contains(&func_id) {
                        self.emit(IrInstr::SpawnClosure {
                            dest: dest.clone(),
                            closure,
                            args,
                        });
                        return dest;
                    }
                }

                self.emit(IrInstr::CallClosure {
                    dest: Some(dest.clone()),
                    closure,
                    args,
                });
                self.propagate_type_projection_to_register(call_ty, &dest);
                return dest;
            }

            // Captured/ancestor identifier call (e.g. method param used inside arrow):
            // resolve through identifier lowering so capture slots are honored.
            if is_captured || is_ancestor {
                let closure = self.lower_identifier(ident);
                let callee_ty = self.get_expr_type(&call.callee);
                let callee_ty_raw = callee_ty.as_u32();
                let known_callable = self.bound_method_vars.contains_key(&ident.name)
                    || self.callable_symbol_hints.contains(&ident.name)
                    || self.function_map.contains_key(&ident.name);
                if !known_callable && !self.type_is_callable(callee_ty) {
                    self.errors
                        .push(crate::compiler::CompileError::InternalError {
                            message: format!(
                                "unresolved call target '{}': captured/ancestor value is not callable (type id {})",
                                self.interner.resolve(ident.name),
                                callee_ty_raw
                            ),
                        });
                    self.poison_register(&dest);
                    return dest;
                }
                // Captured/ancestor async function declarations are represented as closures.
                // Spawn them to preserve Task semantics required by await/await-all.
                if let Some(&func_id) = self.function_map.get(&ident.name) {
                    if self.async_functions.contains(&func_id)
                        || self.async_closures.contains(&func_id)
                    {
                        if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                            eprintln!(
                                "[lower] SpawnClosure[captured-async] '{}' func_id={} is_captured={} is_ancestor={}",
                                self.interner.resolve(ident.name),
                                func_id.as_u32(),
                                is_captured,
                                is_ancestor
                            );
                        }
                        self.emit(IrInstr::SpawnClosure {
                            dest: dest.clone(),
                            closure,
                            args,
                        });
                        return dest;
                    }
                }
                self.emit(IrInstr::CallClosure {
                    dest: Some(dest.clone()),
                    closure,
                    args,
                });
                self.propagate_type_projection_to_register(call_ty, &dest);
                return dest;
            }
        }

        // For member calls, resolve method to builtin ID or user-defined method
        if let Expression::Member(member) = &*call.callee {
            let method_name_symbol = member.property.name;
            let method_name = self.interner.resolve(method_name_symbol);

            // Check for JSON global object methods
            if let Expression::Identifier(ident) = &*member.object {
                let obj_name = self.interner.resolve(ident.name);
                if obj_name == "JSON" {
                    use crate::compiler::intrinsic::JsonIntrinsic;
                    use crate::compiler::native_id::{JSON_PARSE, JSON_STRINGIFY};

                    if let Some(intrinsic) = JsonIntrinsic::detect_intrinsic("JSON", method_name) {
                        match intrinsic {
                            "stringify" => {
                                // JSON.stringify(value) -> native call
                                self.emit(IrInstr::NativeCall {
                                    dest: Some(dest.clone()),
                                    native_id: JSON_STRINGIFY,
                                    args,
                                });
                                return dest;
                            }
                            "parse" => {
                                // JSON.parse(json) -> native call returning json type
                                // JSON type is TypeId 15 (pre-interned in context.rs)

                                let json_dest = self.alloc_register(TypeId::new(JSON_TYPE_ID));
                                self.emit(IrInstr::NativeCall {
                                    dest: Some(json_dest.clone()),
                                    native_id: JSON_PARSE,
                                    args,
                                });
                                return json_dest;
                            }
                            _ => {}
                        }
                    }
                }
            }

            // Promise<T> is represented by a raw task handle internally.
            // Intercept promise-like methods before object dispatch.
            let object_ty = self.get_expr_type(&member.object);
            let inferred_is_promise_class =
                self.infer_nominal_type_id(&member.object).is_some_and(|cid| {
                    self.class_map
                        .iter()
                        .any(|(&sym, &id)| id == cid && self.interner.resolve(sym) == "Promise")
                });
            let is_promise_like = self.type_ctx.is_task_type(object_ty)
                || matches!(
                    self.type_ctx.get(object_ty),
                    Some(crate::parser::types::Type::Class(class)) if class.name == "Promise"
                )
                || inferred_is_promise_class;
            if is_promise_like {
                let task_reg = self.lower_expr(&member.object);
                match method_name {
                    "cancel" => {
                        self.emit(IrInstr::TaskCancel { task: task_reg });
                        return dest;
                    }
                    "isDone" | "isCancelled" => {
                        let native_id = match method_name {
                            "isDone" => 0x0500,      // TASK_IS_DONE
                            "isCancelled" => 0x0501, // TASK_IS_CANCELLED
                            _ => unreachable!(),
                        };
                        self.emit(IrInstr::NativeCall {
                            dest: Some(dest.clone()),
                            native_id,
                            args: vec![task_reg],
                        });
                        return dest;
                    }
                    "then" | "catch" | "finally" => {
                        let awaited_ty =
                            if let Some(crate::parser::types::ty::Type::Task(task_ty)) =
                                self.type_ctx.get(object_ty)
                            {
                                task_ty.result
                            } else {
                                TypeId::new(UNRESOLVED_TYPE_ID)
                            };

                        match method_name {
                            "then" => {
                                self.emit(IrInstr::NativeCall {
                                    dest: None,
                                    native_id: 0x0504, // TASK_MARK_OBSERVED
                                    args: vec![task_reg.clone()],
                                });

                                let check_block = self.alloc_block();
                                let sleep_block = self.alloc_block();
                                let settled_block = self.alloc_block();
                                self.set_terminator(Terminator::Jump(check_block));

                                self.current_function_mut()
                                    .add_block(crate::ir::BasicBlock::new(check_block));
                                self.current_block = check_block;
                                let bool_ty = TypeId::new(BOOLEAN_TYPE_ID);
                                let is_done = self.alloc_register(bool_ty);
                                self.emit(IrInstr::NativeCall {
                                    dest: Some(is_done.clone()),
                                    native_id: 0x0500, // TASK_IS_DONE
                                    args: vec![task_reg.clone()],
                                });
                                self.set_terminator(Terminator::Branch {
                                    cond: is_done,
                                    then_block: settled_block,
                                    else_block: sleep_block,
                                });

                                self.current_function_mut()
                                    .add_block(crate::ir::BasicBlock::new(sleep_block));
                                self.current_block = sleep_block;
                                let zero = self.emit_i32_const(0);
                                self.emit(IrInstr::Sleep { duration_ms: zero });
                                self.set_terminator(Terminator::Jump(check_block));

                                self.current_function_mut()
                                    .add_block(crate::ir::BasicBlock::new(settled_block));
                                self.current_block = settled_block;
                                let is_failed = self.alloc_register(bool_ty);
                                self.emit(IrInstr::NativeCall {
                                    dest: Some(is_failed.clone()),
                                    native_id: 0x0502, // TASK_IS_FAILED
                                    args: vec![task_reg.clone()],
                                });

                                let failed_block = self.alloc_block();
                                let success_block = self.alloc_block();
                                let merge_block = self.alloc_block();
                                self.set_terminator(Terminator::Branch {
                                    cond: is_failed,
                                    then_block: failed_block,
                                    else_block: success_block,
                                });

                                self.current_function_mut()
                                    .add_block(crate::ir::BasicBlock::new(failed_block));
                                self.current_block = failed_block;
                                let failed_result = self.alloc_register(dest.ty);
                                self.emit(IrInstr::Assign {
                                    dest: failed_result.clone(),
                                    value: IrValue::Register(task_reg.clone()),
                                });
                                let failed_exit = self.current_block;
                                self.set_terminator(Terminator::Jump(merge_block));

                                self.current_function_mut()
                                    .add_block(crate::ir::BasicBlock::new(success_block));
                                self.current_block = success_block;
                                let success_result = self.alloc_register(dest.ty);
                                if let Some(callback) = args.first().cloned() {
                                    let awaited = self.alloc_register(awaited_ty);
                                    self.emit(IrInstr::Await {
                                        dest: awaited.clone(),
                                        task: task_reg,
                                    });
                                    self.emit(IrInstr::SpawnClosure {
                                        dest: success_result.clone(),
                                        closure: callback,
                                        args: vec![awaited],
                                    });
                                } else {
                                    self.emit(IrInstr::Assign {
                                        dest: success_result.clone(),
                                        value: IrValue::Register(task_reg),
                                    });
                                }
                                let success_exit = self.current_block;
                                self.set_terminator(Terminator::Jump(merge_block));

                                self.current_function_mut()
                                    .add_block(crate::ir::BasicBlock::new(merge_block));
                                self.current_block = merge_block;
                                self.emit(IrInstr::Phi {
                                    dest: dest.clone(),
                                    sources: vec![
                                        (failed_exit, failed_result),
                                        (success_exit, success_result),
                                    ],
                                });
                                return dest;
                            }
                            "finally" => {
                                self.emit(IrInstr::NativeCall {
                                    dest: None,
                                    native_id: 0x0504, // TASK_MARK_OBSERVED
                                    args: vec![task_reg.clone()],
                                });

                                let check_block = self.alloc_block();
                                let sleep_block = self.alloc_block();
                                let settled_block = self.alloc_block();
                                self.set_terminator(Terminator::Jump(check_block));

                                self.current_function_mut()
                                    .add_block(crate::ir::BasicBlock::new(check_block));
                                self.current_block = check_block;
                                let bool_ty = TypeId::new(BOOLEAN_TYPE_ID);
                                let is_done = self.alloc_register(bool_ty);
                                self.emit(IrInstr::NativeCall {
                                    dest: Some(is_done.clone()),
                                    native_id: 0x0500, // TASK_IS_DONE
                                    args: vec![task_reg.clone()],
                                });
                                self.set_terminator(Terminator::Branch {
                                    cond: is_done,
                                    then_block: settled_block,
                                    else_block: sleep_block,
                                });

                                self.current_function_mut()
                                    .add_block(crate::ir::BasicBlock::new(sleep_block));
                                self.current_block = sleep_block;
                                let zero = self.emit_i32_const(0);
                                self.emit(IrInstr::Sleep { duration_ms: zero });
                                self.set_terminator(Terminator::Jump(check_block));

                                self.current_function_mut()
                                    .add_block(crate::ir::BasicBlock::new(settled_block));
                                self.current_block = settled_block;
                                let is_failed = self.alloc_register(bool_ty);
                                self.emit(IrInstr::NativeCall {
                                    dest: Some(is_failed.clone()),
                                    native_id: 0x0502, // TASK_IS_FAILED
                                    args: vec![task_reg.clone()],
                                });

                                let failed_block = self.alloc_block();
                                let success_block = self.alloc_block();
                                let merge_block = self.alloc_block();
                                self.set_terminator(Terminator::Branch {
                                    cond: is_failed,
                                    then_block: failed_block,
                                    else_block: success_block,
                                });

                                self.current_function_mut()
                                    .add_block(crate::ir::BasicBlock::new(failed_block));
                                self.current_block = failed_block;
                                if let Some(callback) = args.first().cloned() {
                                    self.emit(IrInstr::CallClosure {
                                        dest: None,
                                        closure: callback,
                                        args: vec![],
                                    });
                                }
                                let failed_result = self.alloc_register(dest.ty);
                                self.emit(IrInstr::Assign {
                                    dest: failed_result.clone(),
                                    value: IrValue::Register(task_reg.clone()),
                                });
                                let failed_exit = self.current_block;
                                self.set_terminator(Terminator::Jump(merge_block));

                                self.current_function_mut()
                                    .add_block(crate::ir::BasicBlock::new(success_block));
                                self.current_block = success_block;
                                if let Some(callback) = args.first().cloned() {
                                    self.emit(IrInstr::CallClosure {
                                        dest: None,
                                        closure: callback,
                                        args: vec![],
                                    });
                                }
                                let success_result = self.alloc_register(dest.ty);
                                self.emit(IrInstr::Assign {
                                    dest: success_result.clone(),
                                    value: IrValue::Register(task_reg),
                                });
                                let success_exit = self.current_block;
                                self.set_terminator(Terminator::Jump(merge_block));

                                self.current_function_mut()
                                    .add_block(crate::ir::BasicBlock::new(merge_block));
                                self.current_block = merge_block;
                                self.emit(IrInstr::Phi {
                                    dest: dest.clone(),
                                    sources: vec![
                                        (failed_exit, failed_result),
                                        (success_exit, success_result),
                                    ],
                                });
                                return dest;
                            }
                            "catch" => {
                                self.emit(IrInstr::NativeCall {
                                    dest: None,
                                    native_id: 0x0504, // TASK_MARK_OBSERVED
                                    args: vec![task_reg.clone()],
                                });

                                let check_block = self.alloc_block();
                                let sleep_block = self.alloc_block();
                                let settled_block = self.alloc_block();
                                self.set_terminator(Terminator::Jump(check_block));

                                self.current_function_mut()
                                    .add_block(crate::ir::BasicBlock::new(check_block));
                                self.current_block = check_block;
                                let bool_ty = TypeId::new(BOOLEAN_TYPE_ID);
                                let is_done = self.alloc_register(bool_ty);
                                self.emit(IrInstr::NativeCall {
                                    dest: Some(is_done.clone()),
                                    native_id: 0x0500, // TASK_IS_DONE
                                    args: vec![task_reg.clone()],
                                });
                                self.set_terminator(Terminator::Branch {
                                    cond: is_done,
                                    then_block: settled_block,
                                    else_block: sleep_block,
                                });

                                self.current_function_mut()
                                    .add_block(crate::ir::BasicBlock::new(sleep_block));
                                self.current_block = sleep_block;
                                let zero = self.emit_i32_const(0);
                                self.emit(IrInstr::Sleep { duration_ms: zero });
                                self.set_terminator(Terminator::Jump(check_block));

                                self.current_function_mut()
                                    .add_block(crate::ir::BasicBlock::new(settled_block));
                                self.current_block = settled_block;
                                let is_failed = self.alloc_register(bool_ty);
                                self.emit(IrInstr::NativeCall {
                                    dest: Some(is_failed.clone()),
                                    native_id: 0x0502, // TASK_IS_FAILED
                                    args: vec![task_reg.clone()],
                                });

                                let failed_block = self.alloc_block();
                                let success_block = self.alloc_block();
                                let merge_block = self.alloc_block();
                                self.set_terminator(Terminator::Branch {
                                    cond: is_failed,
                                    then_block: failed_block,
                                    else_block: success_block,
                                });

                                self.current_function_mut()
                                    .add_block(crate::ir::BasicBlock::new(failed_block));
                                self.current_block = failed_block;
                                let reason = self.alloc_register(TypeId::new(UNRESOLVED_TYPE_ID));
                                self.emit(IrInstr::NativeCall {
                                    dest: Some(reason.clone()),
                                    native_id: 0x0503, // TASK_GET_ERROR
                                    args: vec![task_reg.clone()],
                                });
                                let failed_result = self.alloc_register(dest.ty);
                                if let Some(callback) = args.first().cloned() {
                                    self.emit(IrInstr::SpawnClosure {
                                        dest: failed_result.clone(),
                                        closure: callback,
                                        args: vec![reason],
                                    });
                                } else {
                                    self.emit(IrInstr::Assign {
                                        dest: failed_result.clone(),
                                        value: IrValue::Register(task_reg.clone()),
                                    });
                                }
                                let failed_exit = self.current_block;
                                self.set_terminator(Terminator::Jump(merge_block));

                                self.current_function_mut()
                                    .add_block(crate::ir::BasicBlock::new(success_block));
                                self.current_block = success_block;
                                let success_result = self.alloc_register(dest.ty);
                                self.emit(IrInstr::Assign {
                                    dest: success_result.clone(),
                                    value: IrValue::Register(task_reg),
                                });
                                let success_exit = self.current_block;
                                self.set_terminator(Terminator::Jump(merge_block));

                                self.current_function_mut()
                                    .add_block(crate::ir::BasicBlock::new(merge_block));
                                self.current_block = merge_block;
                                self.emit(IrInstr::Phi {
                                    dest: dest.clone(),
                                    sources: vec![
                                        (failed_exit, failed_result),
                                        (success_exit, success_result),
                                    ],
                                });
                                return dest;
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }

            // Method extraction bind fast-path:
            // `obj.method.bind(receiver)` can be lowered directly to BindMethod
            // when the source method slot is statically known.
            if method_name == "bind" {
                if let (Expression::Member(inner_member), Some(bound_receiver)) =
                    (&*member.object, args.first().cloned())
                {
                    if let Some(nominal_type_id) = self.infer_nominal_type_id(&inner_member.object) {
                        if let Some(method_slot) =
                            self.find_method_slot(nominal_type_id, inner_member.property.name)
                        {
                            self.emit(IrInstr::BindMethod {
                                dest: dest.clone(),
                                object: bound_receiver,
                                method: method_slot,
                            });
                            return dest;
                        }
                    }
                }
            }

            // Check if this is a static method call (e.g., Utils.double(21))
            if let Expression::Identifier(ident) = &*member.object {
                let class_name = self.interner.resolve(ident.name);
                let static_native_id = match (class_name, method_name) {
                    ("Object", "defineProperty") => {
                        Some(crate::compiler::native_id::OBJECT_DEFINE_PROPERTY)
                    }
                    ("Object", "getOwnPropertyDescriptor") => {
                        Some(crate::compiler::native_id::OBJECT_GET_OWN_PROPERTY_DESCRIPTOR)
                    }
                    ("Object", "defineProperties") => {
                        Some(crate::compiler::native_id::OBJECT_DEFINE_PROPERTIES)
                    }
                    _ => None,
                };
                if let Some(native_id) = static_native_id {
                    self.emit(IrInstr::NativeCall {
                        dest: Some(dest.clone()),
                        native_id,
                        args,
                    });
                    if native_id == crate::compiler::native_id::OBJECT_GET_OWN_PROPERTY_DESCRIPTOR {
                        self.register_object_fields.insert(
                            dest.id,
                            vec![
                                ("value".to_string(), 0),
                                ("writable".to_string(), 1),
                                ("configurable".to_string(), 2),
                                ("enumerable".to_string(), 3),
                                ("get".to_string(), 4),
                                ("set".to_string(), 5),
                            ],
                        );
                    }
                    return dest;
                }

                if let Some(&nominal_type_id) = self.class_map.get(&ident.name) {
                    // This is a class identifier, check for static methods
                    if let Some(&func_id) =
                        self.static_method_map.get(&(nominal_type_id, method_name_symbol))
                    {
                        // Static method call - no 'this' parameter
                        // Check if async method - emit Spawn instead of Call
                        if self.async_functions.contains(&func_id) {
                            let task_ty = self
                                .type_ctx
                                .generic_task_type()
                                .unwrap_or(TypeId::new(TASK_TYPE_ID));
                            let task_dest = self.alloc_register(task_ty);
                            self.emit(IrInstr::Spawn {
                                dest: task_dest.clone(),
                                func: func_id,
                                args,
                            });
                            return task_dest;
                        } else {
                            self.emit(IrInstr::Call {
                                dest: Some(dest.clone()),
                                func: func_id,
                                args,
                            });
                            self.propagate_type_projection_to_register(call_ty, &dest);
                        }
                        return dest;
                    }
                }
            }

            if self.prefers_structural_member_projection(&member.object)
                && self
                    .structural_shape_slot_from_expr(&member.object, method_name)
                    .is_some()
            {
                if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                if let Expression::Identifier(obj_ident) = &*member.object {
                        eprintln!(
                            "[lower] structural member call '{}.{}(...)' via shape projection",
                            self.interner.resolve(obj_ident.name),
                            method_name
                        );
                    }
                }
                let async_call =
                    self.late_bound_member_call_is_async(&member.object, &call.callee, method_name);
                if !async_call {
                    if let Some((shape_id, slot)) =
                        self.structural_shape_slot_from_expr(&member.object, method_name)
                    {
                        let object = self.lower_expr(&member.object);
                        self.emit_structural_shape_name_registration_for_expr(&member.object);
                        self.emit(IrInstr::CallMethodShape {
                            dest: Some(dest.clone()),
                            object,
                            shape_id,
                            method: slot,
                            args,
                            optional: member.optional,
                        });
                        self.propagate_type_projection_to_register(call_ty, &dest);
                        return dest;
                    }
                }
                let closure = self.lower_member(member);
                if async_call {
                    self.emit(IrInstr::SpawnClosure {
                        dest: dest.clone(),
                        closure,
                        args,
                    });
                } else {
                    self.emit(IrInstr::CallClosure {
                        dest: Some(dest.clone()),
                        closure,
                        args,
                    });
                    self.propagate_type_projection_to_register(call_ty, &dest);
                }
                return dest;
            }

            let constrained_typevar_shape_call = self
                .type_ctx
                .get(self.get_expr_type(&member.object))
                .is_some_and(|ty| {
                    matches!(ty, crate::parser::types::ty::Type::TypeVar(tv) if tv.constraint.is_some())
                });

            if constrained_typevar_shape_call
                && self
                    .structural_shape_slot_from_expr(&member.object, method_name)
                    .is_some()
            {
                if let Some((shape_id, slot)) =
                    self.structural_shape_slot_from_expr(&member.object, method_name)
                {
                    let object = self.lower_expr(&member.object);
                    self.emit_structural_shape_name_registration_for_expr(&member.object);
                    self.emit(IrInstr::CallMethodShape {
                        dest: Some(dest.clone()),
                        object,
                        shape_id,
                        method: slot,
                        args,
                        optional: member.optional,
                    });
                    self.propagate_type_projection_to_register(call_ty, &dest);
                    return dest;
                }
            }

            // Try to determine the class type of the object for method resolution
            let member_field_metadata = self.resolve_member_field_metadata(&member.object);
            let mut nominal_type_id = self
                .infer_nominal_type_id(&member.object)
                .or_else(|| member_field_metadata.and_then(|(_, nominal_type_id)| nominal_type_id));

            // If nominal_type_id is not found, check if this is a Channel type parameter
            // Parameters with Channel<T> type annotation aren't in variable_class_map
            if nominal_type_id.is_none() {
                if let Expression::Identifier(ident) = &*member.object {
                    // Check if this identifier is a local variable with Channel type
                    if let Some(&local_idx) = self.local_map.get(&ident.name) {
                        if let Some(reg) = self.local_registers.get(&local_idx) {
                            if reg.ty.as_u32() == CHANNEL_TYPE_ID {
                                // This is a Channel type - look up Channel class by finding it in class_map
                                for (&sym, &cid) in &self.class_map {
                                    if self.interner.resolve(sym) == TC::CHANNEL_TYPE_NAME {
                                        nominal_type_id = Some(cid);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            let inferred_nominal_type_id = nominal_type_id;

            // Skip class dispatch for builtin primitive types — their methods
            // are dispatched via the type registry (native calls / class methods)
            if let Some(cid) = nominal_type_id {
                let is_builtin = self.class_map.iter().any(|(&sym, &id)| {
                    id == cid && matches!(self.interner.resolve(sym), "string" | "number" | "Array")
                });
                if is_builtin {
                    nominal_type_id = None;
                }
            }

            // Check if this is a user-defined class method (including inherited methods)
            if let Some(nominal_type_id) = nominal_type_id {
                // Check if this is a function-typed FIELD (not a method)
                // Fields should be loaded via GetField + CallClosure, not CallMethodExact
                let all_fields = self.get_all_fields(nominal_type_id);
                let is_field = all_fields
                    .iter()
                    .any(|f| self.interner.resolve(f.name) == method_name);
                let is_method = self.find_method(nominal_type_id, method_name_symbol).is_some();

                if is_field && !is_method {
                    // Function-typed field: emit GetField + CallClosure
                    let object = self.lower_expr(&member.object);
                    let field_info = all_fields
                        .iter()
                        .rev()
                        .find(|f| self.interner.resolve(f.name) == method_name)
                        .unwrap();
                    let field_reg = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
                    self.emit(IrInstr::LoadFieldExact {
                        dest: field_reg.clone(),
                        object,
                        field: field_info.index,
                        optional: member.optional,
                    });
                    if self.type_id_is_async_callable(field_info.ty) {
                        self.emit(IrInstr::SpawnClosure {
                            dest: dest.clone(),
                            closure: field_reg,
                            args,
                        });
                    } else {
                        self.emit(IrInstr::CallClosure {
                            dest: Some(dest.clone()),
                            closure: field_reg,
                            args,
                        });
                        self.propagate_type_projection_to_register(call_ty, &dest);
                    }
                    return dest;
                }

                if let Some(func_id) = self.find_method(nominal_type_id, method_name_symbol) {
                    if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                        if let Expression::Identifier(obj_ident) = &*member.object {
                            eprintln!(
                                "[lower] class member call '{}.{}(...)' nominal_type_id={} func_id={}",
                                self.interner.resolve(obj_ident.name),
                                method_name,
                                nominal_type_id.as_u32(),
                                func_id.as_u32()
                            );
                        }
                    }
                    // Lower the object (receiver) first
                    let object = self.lower_expr(&member.object);

                    // Check if async method - emit Spawn (stays static, no vtable)
                    if self.async_functions.contains(&func_id) {
                        let mut method_args = vec![object];
                        method_args.extend(args);
                        let task_ty = self
                            .type_ctx
                            .generic_task_type()
                            .unwrap_or(TypeId::new(TASK_TYPE_ID));
                        let task_dest = self.alloc_register(task_ty);
                        self.emit(IrInstr::Spawn {
                            dest: task_dest.clone(),
                            func: func_id,
                            args: method_args,
                        });
                        return task_dest;
                    }

                    // Use virtual dispatch via vtable slot
                    if let Some(slot) = self.find_method_slot(nominal_type_id, method_name_symbol) {
                        self.emit(IrInstr::CallMethodExact {
                            dest: Some(dest.clone()),
                            object,
                            method: slot,
                            args,
                            optional: member.optional,
                        });
                    } else {
                        let class_name = self
                            .class_map
                            .iter()
                            .find_map(|(&sym, &id)| {
                                if id == nominal_type_id {
                                    Some(self.interner.resolve(sym).to_string())
                                } else {
                                    None
                                }
                            })
                            .unwrap_or_else(|| format!("class#{}", nominal_type_id.as_u32()));
                        self.errors
                            .push(crate::compiler::CompileError::InternalError {
                                message: format!(
                                    "missing vtable slot for instance method '{}.{}' (func_id={})",
                                    class_name,
                                    method_name,
                                    func_id.as_u32()
                                ),
                            });
                        self.poison_register(&dest);
                        return dest;
                    }

                    // Preserve precise method return typing for user-defined class methods.
                    // This is especially important when checker call typing is unresolved
                    // (e.g. precompiled stdlib class dispatch), so downstream property/method
                    // access can still select typed opcodes (ArrayLen/StringLen/etc).
                    if dest.ty.as_u32() == super::UNRESOLVED_TYPE_ID {
                        if let Some(&ret_ty) = self
                            .method_return_type_map
                            .get(&(nominal_type_id, method_name_symbol))
                        {
                            if ret_ty.as_u32() != super::UNRESOLVED_TYPE_ID {
                                dest.ty = ret_ty;
                            }
                        }
                    }

                    // Propagate generic return type for Map/Set methods
                    self.propagate_container_return_type(
                        &mut dest,
                        nominal_type_id,
                        method_name,
                        &member.object,
                    );
                    self.propagate_type_projection_to_register(dest.ty, &dest);

                    return dest;
                } else if let Some(slot) = self.find_method_slot(nominal_type_id, method_name_symbol) {
                    // Abstract method with vtable slot - use virtual dispatch.
                    // The actual implementation is provided by a derived class.
                    let object = self.lower_expr(&member.object);
                    self.emit(IrInstr::CallMethodExact {
                        dest: Some(dest.clone()),
                        object,
                        method: slot,
                        args,
                        optional: member.optional,
                    });

                    // If checker type is unresolved, still try to carry declared return type.
                    if dest.ty.as_u32() == super::UNRESOLVED_TYPE_ID {
                        if let Some(&ret_ty) = self
                            .method_return_type_map
                            .get(&(nominal_type_id, method_name_symbol))
                        {
                            if ret_ty.as_u32() != super::UNRESOLVED_TYPE_ID {
                                dest.ty = ret_ty;
                            }
                        }
                    }
                    self.propagate_type_projection_to_register(dest.ty, &dest);
                    return dest;
                }
            }

            // Structural object-call path: `obj.f(...)` where `f` is function-typed.
            // Keep slot-based lowering; runtime structural slot views map interface
            // slots to concrete field/method bindings.
            if nominal_type_id.is_none()
                && self.is_structural_object_type(self.get_expr_type(&member.object))
                && self.type_is_callable(self.get_expr_type(&call.callee))
            {
                if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                    let obj_type = self.get_expr_type(&member.object);
                    let obj_type_str = format!("{:?}", obj_type);
                    if let Expression::Identifier(obj_ident) = &*member.object {
                        let in_vcm = self.variable_class_map.contains_key(&obj_ident.name);
                        eprintln!(
                            "[lower] structural member call '{}.{}(...)' — obj type_id={} in_variable_class_map={} (WILL USE LoadFieldExact+CallClosure)",
                            self.interner.resolve(obj_ident.name),
                            method_name,
                            obj_type_str,
                            in_vcm
                        );
                    } else {
                        eprintln!(
                            "[lower] structural member call '<expr>.{}(...)' — obj type_id={} (WILL USE LoadFieldExact+CallClosure)",
                            method_name,
                            obj_type_str
                        );
                    }
                }
                let async_call =
                    self.late_bound_member_call_is_async(&member.object, &call.callee, method_name);
                if !async_call {
                    if let Some((shape_id, slot)) =
                        self.structural_shape_slot_from_expr(&member.object, method_name)
                    {
                        let object = self.lower_expr(&member.object);
                        self.emit_structural_shape_name_registration_for_expr(&member.object);
                        self.emit(IrInstr::CallMethodShape {
                            dest: Some(dest.clone()),
                            object,
                            shape_id,
                            method: slot,
                            args,
                            optional: member.optional,
                        });
                        self.propagate_type_projection_to_register(call_ty, &dest);
                        return dest;
                    }
                }
                let closure = self.lower_member(member);
                if async_call {
                    self.emit(IrInstr::SpawnClosure {
                        dest: dest.clone(),
                        closure,
                        args,
                    });
                } else {
                    self.emit(IrInstr::CallClosure {
                        dest: Some(dest.clone()),
                        closure,
                        args,
                    });
                    self.propagate_type_projection_to_register(call_ty, &dest);
                }
                return dest;
            }

            // Free-variable concrete-layout call path: `obj.method(...)` where `obj` is an
            // identifier with a known field layout (variable_object_fields) but the checker
            // type is unresolved (e.g. captured from an outer scope inside a nested function).
            // This avoids falling into JsonGet when the object is a stdlib module default export.
            if nominal_type_id.is_none() {
                if let Expression::Identifier(obj_ident) = &*member.object {
                    if let Some(fields) = self.variable_object_fields.get(&obj_ident.name) {
                        if fields.iter().any(|(n, _)| n == method_name) {
                            if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                                eprintln!(
                                    "[lower] concrete-layout member call '{}.{}(...)' via variable_object_fields (WILL USE LoadFieldExact+CallClosure)",
                                    self.interner.resolve(obj_ident.name),
                                    method_name
                                );
                            }
                            let closure = self.lower_member(member);
                            if self.late_bound_member_call_is_async(
                                &member.object,
                                &call.callee,
                                method_name,
                            ) {
                                self.emit(IrInstr::SpawnClosure {
                                    dest: dest.clone(),
                                    closure,
                                    args,
                                });
                            } else {
                                self.emit(IrInstr::CallClosure {
                                    dest: Some(dest.clone()),
                                    closure,
                                    args,
                                });
                                self.propagate_type_projection_to_register(call_ty, &dest);
                            }
                            return dest;
                        }
                    }
                }
            }

            // Fall back to registry-based dispatch
            let object = self.lower_expr(&member.object);

            // Resolve dispatch type: prefer register type (canonical IDs from lowerer),
            // normalize checker type as fallback (may have dynamic union/generic IDs).
            // Unknown (6) is treated as useless for dispatch.
            let obj_type_id = {
                let reg_ty = object.ty.as_u32();
                let checker_ty = self.get_expr_type(&member.object).as_u32();
                let field_ty = member_field_metadata
                    .map(|(field_ty, _)| field_ty.as_u32())
                    .unwrap_or(UNRESOLVED_TYPE_ID);
                let reg_dispatch = self.normalize_type_for_dispatch(reg_ty);
                let checker_dispatch = self.normalize_type_for_dispatch(checker_ty);
                let field_dispatch = self.normalize_type_for_dispatch(field_ty);

                if reg_dispatch != UNRESOLVED_TYPE_ID && reg_dispatch != 6 {
                    reg_dispatch
                } else if checker_dispatch != UNRESOLVED_TYPE_ID && checker_dispatch != 6 {
                    checker_dispatch
                } else if field_dispatch != UNRESOLVED_TYPE_ID && field_dispatch != 6 {
                    field_dispatch
                } else {
                    UNRESOLVED_TYPE_ID
                }
            };
            if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                if !matches!(&*member.object, Expression::Identifier(_)) {
                    eprintln!(
                        "[lower] member-call receiver method='{}' reg_ty={} checker_ty={} field_ty={} nominal_type_id={:?} obj_type_id={}",
                        method_name,
                        object.ty.as_u32(),
                        self.get_expr_type(&member.object).as_u32(),
                        member_field_metadata
                            .map(|(field_ty, _)| field_ty.as_u32())
                            .unwrap_or(UNRESOLVED_TYPE_ID),
                        nominal_type_id.map(|id| id.as_u32()),
                        obj_type_id
                    );
                }
            }
            if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                if let Expression::Identifier(obj_ident) = &*member.object {
                    eprintln!(
                        "[lower] registry member call fallback '{}.{}(...)' obj_ty_id={}",
                        self.interner.resolve(obj_ident.name),
                        method_name,
                        obj_type_id
                    );
                }
            }

            let receiver_requires_late_bound = if let Expression::Identifier(obj_ident) =
                &*member.object
            {
                self.identifier_requires_late_bound_dispatch(obj_ident.name)
            } else {
                false
            } || (nominal_type_id.is_none()
                && (self.type_requires_late_bound_dispatch(object.ty)
                    || self.type_requires_late_bound_dispatch(self.get_expr_type(&member.object))));

            // Handle-backed Mutex<T> methods that map to dedicated bytecode opcodes.
            if obj_type_id == MUTEX_TYPE_ID && args.is_empty() {
                match method_name {
                    "lock" => {
                        self.emit(IrInstr::MutexLock {
                            mutex: object.clone(),
                        });
                        self.emit(IrInstr::Assign {
                            dest: dest.clone(),
                            value: IrValue::Constant(IrConstant::Null),
                        });
                        return dest;
                    }
                    "unlock" => {
                        self.emit(IrInstr::MutexUnlock {
                            mutex: object.clone(),
                        });
                        self.emit(IrInstr::Assign {
                            dest: dest.clone(),
                            value: IrValue::Constant(IrConstant::Null),
                        });
                        return dest;
                    }
                    _ => {}
                }
            }

            // Imported-constructor objects (without local class metadata) must use
            // late-bound member lookup regardless of primitive checker fallbacks.
            if nominal_type_id.is_none() && receiver_requires_late_bound {
                if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                    if let Expression::Identifier(obj_ident) = &*member.object {
                        eprintln!(
                            "[lower] late-bound member call '{}.{}(...)' via late_bound_object_vars",
                            self.interner.resolve(obj_ident.name),
                            method_name
                        );
                    }
                }
                let closure = self.alloc_register(UNRESOLVED);
                self.emit(IrInstr::LateBoundMember {
                    dest: closure.clone(),
                    object,
                    property: method_name.to_string(),
                });
                if self.late_bound_member_call_is_async(&member.object, &call.callee, method_name) {
                    self.emit(IrInstr::SpawnClosure {
                        dest: dest.clone(),
                        closure,
                        args,
                    });
                } else {
                    self.emit(IrInstr::CallClosure {
                        dest: Some(dest.clone()),
                        closure,
                        args,
                    });
                    self.propagate_type_projection_to_register(call_ty, &dest);
                }
                return dest;
            }

            // For no-arg calls like length(), check registry properties (opcode dispatch)
            if args.is_empty() && obj_type_id != UNRESOLVED_TYPE_ID {
                if let Some(crate::compiler::type_registry::DispatchAction::Opcode(kind)) =
                    self.type_registry.lookup_property(obj_type_id, method_name)
                {
                    let len_dest = self.alloc_register(TypeId::new(INT_TYPE_ID));
                    match kind {
                        crate::compiler::type_registry::OpcodeKind::StringLen => {
                            self.emit(IrInstr::StringLen {
                                dest: len_dest.clone(),
                                string: object,
                            });
                        }
                        crate::compiler::type_registry::OpcodeKind::ArrayLen => {
                            self.emit(IrInstr::ArrayLen {
                                dest: len_dest.clone(),
                                array: object,
                            });
                        }
                    }
                    return len_dest;
                }
            }

            // Look up method in type registry for native dispatch
            let method_id = if obj_type_id != UNRESOLVED_TYPE_ID {
                if let Some(action) = self.type_registry.lookup_method(obj_type_id, method_name) {
                    match action {
                        crate::compiler::type_registry::DispatchAction::NativeCall(mut id) => {
                            // Special handling: string methods with RegExp argument
                            let first_arg_is_regexp = if !args.is_empty() {
                                let reg_ty = self.normalize_type_for_dispatch(args[0].ty.as_u32());
                                let checker_ty = self.normalize_type_for_dispatch(
                                    self.get_expr_type(&call.arguments[0]).as_u32(),
                                );
                                reg_ty == REGEXP_TYPE_ID || checker_ty == REGEXP_TYPE_ID
                            } else {
                                false
                            };
                            if obj_type_id == STRING_TYPE_ID && first_arg_is_regexp {
                                use crate::vm::builtin::string as bs;
                                match method_name {
                                    "replace" => id = bs::REPLACE_REGEXP,
                                    "split" => id = bs::SPLIT_REGEXP,
                                    _ => {}
                                }
                            }
                            Some(id)
                        }
                        crate::compiler::type_registry::DispatchAction::ClassMethod(
                            ref cm_type,
                            ref cm_method,
                        ) => {
                            // Build or retrieve the pre-compiled class method function
                            let func_id = self.get_or_build_class_method(cm_type, cm_method);
                            // Emit Call with object as first arg (function takes `this` as param[0])
                            let mut call_args = vec![object];
                            call_args.extend(args);
                            self.emit(IrInstr::Call {
                                dest: Some(dest.clone()),
                                func: func_id,
                                args: call_args,
                            });
                            return dest;
                        }
                        crate::compiler::type_registry::DispatchAction::Opcode(_) => None,
                    }
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(method_id) = method_id {
                // Registry DispatchAction::NativeCall entries are VM native IDs,
                // not class vtable slots. Always lower these as NativeCall with
                // receiver as arg0 to support primitive receivers (number/string)
                // and handle-backed builtins (Map/Set/Channel/Buffer/Date/Mutex).
                // Optional member calls on builtin native methods are currently
                // lowered as eager native calls (same as non-optional).
                let mut native_args = Vec::with_capacity(args.len() + 1);
                native_args.push(object);
                native_args.extend(args);
                self.emit(IrInstr::NativeCall {
                    dest: Some(dest.clone()),
                    native_id: method_id,
                    args: native_args,
                });

                // Propagate return type for builtin methods so subsequent operations
                // use the correct typed opcodes (e.g., Iadd vs Fadd, Seq vs Feq).
                // Return types are extracted from .raya builtin file method signatures.
                if let Some(ret_type) = self.type_registry.lookup_return_type(method_id) {
                    dest.ty = TypeId::new(ret_type);
                }
                return dest;
            }

            // Last structural fallback: if the receiver is a known object-literal
            // with a concrete field layout, treat `obj.m(...)` as loading a
            // function-valued field then invoking it as a closure.
            if let Expression::Identifier(ident) = &*member.object {
                let field_index = self
                    .variable_object_fields
                    .get(&ident.name)
                    .and_then(|fields| {
                        fields
                            .iter()
                            .find(|(name, _)| name == method_name)
                            .map(|(_, idx)| *idx as u16)
                    });
                if let Some(field_index) = field_index {
                    let object = self.lower_expr(&member.object);
                    let closure = self.alloc_register(UNRESOLVED);
                    self.emit(IrInstr::LoadFieldExact {
                        dest: closure.clone(),
                        object,
                        field: field_index,
                        optional: member.optional,
                    });
                    if self.late_bound_member_call_is_async(
                        &member.object,
                        &call.callee,
                        method_name,
                    ) {
                        self.emit(IrInstr::SpawnClosure {
                            dest: dest.clone(),
                            closure,
                            args,
                        });
                    } else {
                        self.emit(IrInstr::CallClosure {
                            dest: Some(dest.clone()),
                            closure,
                            args,
                        });
                    }
                    return dest;
                }
            }

            // Unknown/dynamic receiver fallback: lower `obj.m(...)` as
            // `CallClosure(LateBoundMember(obj, "m"), args)` so strict mode
            // can still compile unresolved-but-valid dynamic patterns.
            if nominal_type_id.is_none() && (obj_type_id == UNRESOLVED_TYPE_ID || obj_type_id == 6) {
                let closure = self.alloc_register(UNRESOLVED);
                self.emit(IrInstr::LateBoundMember {
                    dest: closure.clone(),
                    object,
                    property: method_name.to_string(),
                });
                if self.late_bound_member_call_is_async(&member.object, &call.callee, method_name) {
                    self.emit(IrInstr::SpawnClosure {
                        dest: dest.clone(),
                        closure,
                        args,
                    });
                } else {
                    self.emit(IrInstr::CallClosure {
                        dest: Some(dest.clone()),
                        closure,
                        args,
                    });
                }
                return dest;
            }

            let class_name = inferred_nominal_type_id.and_then(|cid| {
                self.class_map.iter().find_map(|(&sym, &id)| {
                    if id == cid {
                        Some(self.interner.resolve(sym).to_string())
                    } else {
                        None
                    }
                })
            });
            let object_type = class_name
                .map(|name| format!("class {}", name))
                .or_else(|| {
                    if obj_type_id != UNRESOLVED_TYPE_ID {
                        Some(format!("type id {}", obj_type_id))
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "unknown receiver type".to_string());

            self.errors
                .push(crate::compiler::CompileError::InternalError {
                    message: format!(
                        "unresolved member call '{}()' on {}: no class or registry dispatch path",
                        method_name, object_type
                    ),
                });
            self.poison_register(&dest);
            return dest;
        }

        // Fallback: callee is an expression (e.g., (getFunc())()).
        // Require callability; do not emit an unsafe call to an arbitrary value.
        let callee_ty = self.get_expr_type(&call.callee);
        let callee_ty_raw = callee_ty.as_u32();
        let ambient_runtime_callable = matches!(
            &*call.callee,
            Expression::Identifier(ident)
                if self
                    .ambient_builtin_globals
                    .contains(self.interner.resolve(ident.name))
                    && !self.class_map.contains_key(&ident.name)
        );
        if std::env::var("RAYA_DEBUG_CALL_FALLBACK").is_ok() {
            let callee_desc = match &*call.callee {
                Expression::Identifier(id) => {
                    format!("Identifier({})", self.interner.resolve(id.name))
                }
                Expression::Member(member) => {
                    format!("Member(.{})", self.interner.resolve(member.property.name))
                }
                Expression::Call(_) => "Call(...)".to_string(),
                Expression::TypeCast(_) => "TypeCast(...)".to_string(),
                Expression::Parenthesized(_) => "Parenthesized(...)".to_string(),
                _ => format!("{:?}", &*call.callee),
            };
            eprintln!(
                "[lower][call-fallback] callee={} ty={} unresolved={} unknown={}",
                callee_desc,
                callee_ty_raw,
                callee_ty_raw == UNRESOLVED_TYPE_ID,
                callee_ty_raw == UNKNOWN_TYPE_ID
            );
        }
        if !self.type_is_callable(callee_ty)
            && !self.expression_is_callable_hint(&call.callee)
            && !ambient_runtime_callable
        {
            self.errors
                .push(crate::compiler::CompileError::InternalError {
                    message: format!(
                        "unresolved call target: callee expression is not callable (type id {})",
                        callee_ty.as_u32()
                    ),
                });
            self.poison_register(&dest);
            return dest;
        }

        let closure = self.lower_expr(&call.callee);
        if self.type_id_is_async_callable(callee_ty) {
            self.emit(IrInstr::SpawnClosure {
                dest: dest.clone(),
                closure,
                args,
            });
        } else {
            self.emit(IrInstr::CallClosure {
                dest: Some(dest.clone()),
                closure,
                args,
            });
            self.propagate_type_projection_to_register(call_ty, &dest);
        }
        dest
    }

    /// Get or build a class method IR function. Caches by "TypeName_methodName".
    fn get_or_build_class_method(&mut self, type_name: &str, method_name: &str) -> FunctionId {
        let key = format!("{}_{}", type_name, method_name);
        if let Some(&func_id) = self.class_method_cache.get(&key) {
            return func_id;
        }

        let ir_func = super::class_methods::build_class_method_ir(type_name, method_name)
            .unwrap_or_else(|| panic!("Unknown class method: {}.{}", type_name, method_name));

        let func_id = FunctionId::new(self.next_function_id);
        self.next_function_id += 1;

        self.pending_arrow_functions
            .push((func_id.as_u32(), ir_func));
        self.class_method_cache.insert(key, func_id);

        func_id
    }

    /// Helper: emit an i32 constant into a register.
    pub(super) fn emit_i32_const(&mut self, value: i32) -> Register {
        let reg = self.alloc_register(TypeId::new(INT_TYPE_ID)); // int type
        self.emit(IrInstr::Assign {
            dest: reg.clone(),
            value: IrValue::Constant(IrConstant::I32(value)),
        });
        reg
    }

    fn lower_member(&mut self, member: &ast::MemberExpression) -> Register {
        let prop_name = self.interner.resolve(member.property.name);

        // Check if this is a static field access (e.g., Math.PI where Math is a class)
        if let Expression::Identifier(ident) = &*member.object {
            if let Some(&nominal_type_id) = self.class_map.get(&ident.name) {
                // This is a class identifier, check for static fields
                // Extract global_index first to avoid borrow conflict
                let global_index = self.class_info_map.get(&nominal_type_id).and_then(|class_info| {
                    class_info
                        .static_fields
                        .iter()
                        .find(|f| self.interner.resolve(f.name) == prop_name)
                        .map(|sf| sf.global_index)
                });

                if let Some(index) = global_index {
                    // Found a static field - emit LoadGlobal
                    let dest = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
                    self.emit(IrInstr::LoadGlobal {
                        dest: dest.clone(),
                        index,
                    });
                    return dest;
                }
            }
        }

        // Try to determine the class type of the object for field resolution.
        // Explicit structural alias projections must not reuse provider field indices.
        let nominal_type_id = if self.prefers_structural_member_projection(&member.object) {
            None
        } else {
            self.infer_nominal_type_id(&member.object)
        };

        let object = self.lower_expr(&member.object);

        // Resolve dispatch type: prefer register type (set by lowerer with canonical IDs),
        // normalize checker type as fallback (may have dynamic union/generic IDs).
        // Unknown (6) is treated as useless for dispatch — it has no registry entries.
        let obj_ty_id = {
            let reg_ty = object.ty.as_u32();
            let checker_ty = self.get_expr_type(&member.object).as_u32();
            let reg_dispatch = self.normalize_type_for_dispatch(reg_ty);
            let checker_dispatch = self.normalize_type_for_dispatch(checker_ty);

            if reg_dispatch != UNRESOLVED_TYPE_ID && reg_dispatch != 6 {
                reg_dispatch
            } else if checker_dispatch != UNRESOLVED_TYPE_ID && checker_dispatch != 6 {
                checker_dispatch
            } else {
                UNRESOLVED_TYPE_ID
            }
        };

        // Check for JSON/JsonObject types - use duck typing with dynamic property access
        if obj_ty_id == JSON_TYPE_ID || obj_ty_id == JSON_OBJECT_TYPE_ID {
            let json_type = TypeId::new(JSON_TYPE_ID);
            let dest = self.alloc_register(json_type);
            self.emit(IrInstr::DynGetProp {
                dest: dest.clone(),
                object,
                property: prop_name.to_string(),
            });
            return dest;
        }

        let receiver_expr_ty = self.get_expr_type(&member.object);
        let receiver_is_dynamic_object = matches!(
            self.type_ctx.get(receiver_expr_ty),
            Some(Type::JSObject) | Some(Type::Any) | Some(Type::Unknown)
        ) || self.type_ctx.jsobject_inner(receiver_expr_ty).is_some()
            || matches!(
                self.type_ctx.get(object.ty),
                Some(Type::JSObject) | Some(Type::Any) | Some(Type::Unknown)
            )
            || self.type_ctx.jsobject_inner(object.ty).is_some();

        if nominal_type_id.is_none() && receiver_is_dynamic_object {
            let member_ty = {
                let member_expr = Expression::Member(member.clone());
                let inferred = self.get_expr_type(&member_expr);
                if inferred.as_u32() == UNRESOLVED_TYPE_ID {
                    UNRESOLVED
                } else {
                    inferred
                }
            };
            let dest = self.alloc_register(member_ty);
            self.emit(IrInstr::DynGetProp {
                dest: dest.clone(),
                object,
                property: prop_name.to_string(),
            });
            return dest;
        }

        // Registry-based property dispatch (replaces hardcoded .length checks)
        if obj_ty_id != UNRESOLVED_TYPE_ID {
            if let Some(action) = self.type_registry.lookup_property(obj_ty_id, prop_name) {
                match action {
                    crate::compiler::type_registry::DispatchAction::Opcode(kind) => {
                        let dest = self.alloc_register(TypeId::new(INT_TYPE_ID)); // length returns int
                        match kind {
                            crate::compiler::type_registry::OpcodeKind::StringLen => {
                                self.emit(IrInstr::StringLen {
                                    dest: dest.clone(),
                                    string: object,
                                });
                            }
                            crate::compiler::type_registry::OpcodeKind::ArrayLen => {
                                self.emit(IrInstr::ArrayLen {
                                    dest: dest.clone(),
                                    array: object,
                                });
                            }
                        }
                        return dest;
                    }
                    crate::compiler::type_registry::DispatchAction::NativeCall(method_id) => {
                        let mut dest = self.alloc_register(TypeId::new(UNRESOLVED_TYPE_ID));
                        if let Some(ret_type) = self.type_registry.lookup_return_type(method_id) {
                            dest.ty = TypeId::new(ret_type);
                        }
                        self.emit(IrInstr::NativeCall {
                            dest: Some(dest.clone()),
                            native_id: method_id,
                            args: vec![object],
                        });
                        return dest;
                    }
                    crate::compiler::type_registry::DispatchAction::ClassMethod(_, _) => {
                        // Properties shouldn't use ClassMethod
                    }
                }
            }
        }

        // Look up field index and type by name if we know the class type (including inherited fields)
        enum ResolvedMemberSlot {
            Concrete {
                field_index: u16,
                field_ty: TypeId,
            },
            Shape {
                shape_id: u64,
                field_index: u16,
                field_ty: TypeId,
            },
        }

        let resolved_field = if let Some(nominal_type_id) = nominal_type_id {
            // Get all fields including parent fields
            let all_fields = self.get_all_fields(nominal_type_id);
            // Use .rev() so child fields shadow parent fields with the same name
            if let Some(field) = all_fields
                .iter()
                .rev()
                .find(|f| self.interner.resolve(f.name) == prop_name)
            {
                Some(ResolvedMemberSlot::Concrete {
                    field_index: field.index,
                    field_ty: field.ty,
                })
            } else {
                if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                    let class_name = self
                        .class_map
                        .iter()
                        .find_map(|(&sym, &id)| {
                            (id == nominal_type_id).then(|| self.interner.resolve(sym))
                        })
                        .unwrap_or("<unknown>");
                    let field_list = all_fields
                        .iter()
                        .map(|f| self.interner.resolve(f.name).to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    eprintln!(
                        "[lower] member miss on class {} property '{}' fields=[{}]",
                        class_name, prop_name, field_list
                    );
                }
                // Field not found — check if it's a method (bound method extraction)
                if let Some(slot) = self.find_method_slot(nominal_type_id, member.property.name) {
                    let member_ty = {
                        let member_expr = Expression::Member(member.clone());
                        let inferred = self.get_expr_type(&member_expr);
                        if inferred.as_u32() == UNRESOLVED_TYPE_ID {
                            UNRESOLVED
                        } else {
                            inferred
                        }
                    };
                    if self.js_this_binding_compat {
                        if let Some(func_id) = self.find_method(nominal_type_id, member.property.name) {
                            let dest = self.alloc_register(member_ty);
                            self.emit(IrInstr::MakeClosure {
                                dest: dest.clone(),
                                func: func_id,
                                captures: vec![],
                            });
                            return dest;
                        }
                    }
                    let dest = self.alloc_register(member_ty);
                    self.emit(IrInstr::BindMethod {
                        dest: dest.clone(),
                        object,
                        method: slot,
                    });
                    return dest;
                }
                // Not a field or method — fall through to the non-class path below.
                None
            }
        } else {
            let infer_field_from_expr_type = |this: &Self| {
                if let Some((shape_id, slot)) =
                    this.structural_shape_slot_from_expr(&member.object, prop_name)
                {
                    let field_ty = this
                        .structural_field_type_from_type(this.get_expr_type(&member.object), prop_name)
                        .unwrap_or(UNRESOLVED);
                    return Some(ResolvedMemberSlot::Shape {
                        shape_id,
                        field_index: slot,
                        field_ty,
                    });
                }
                let expr_ty = if object.ty.as_u32() != UNKNOWN_TYPE_ID
                    && object.ty.as_u32() != UNRESOLVED_TYPE_ID
                {
                    object.ty
                } else {
                    this.get_expr_type(&member.object)
                };
                let shape_id = this.structural_shape_id_from_type(expr_ty)?;
                let slot = this.structural_slot_index_from_type(expr_ty, prop_name)?;
                let field_ty = this
                    .structural_field_type_from_type(expr_ty, prop_name)
                    .unwrap_or(UNRESOLVED);
                Some(ResolvedMemberSlot::Shape {
                    shape_id,
                    field_index: slot,
                    field_ty,
                })
            };

            let alias_field_idx = match &*member.object {
                Expression::Identifier(ident) => self
                    .variable_object_type_aliases
                    .get(&ident.name)
                    .and_then(|alias| self.type_alias_field_lookup(alias, prop_name)),
                _ => None,
            };

            if let Some((field_index, field_ty)) = alias_field_idx {
                self.structural_shape_id_from_type(self.get_expr_type(&member.object))
                    .map(|shape_id| ResolvedMemberSlot::Shape {
                        shape_id,
                        field_index,
                        field_ty,
                    })
            } else {
                // Check variable_object_fields for decoded object field layout
                let obj_field_idx = match &*member.object {
                    Expression::Identifier(ident) => self
                        .variable_object_fields
                        .get(&ident.name)
                        .and_then(|fields| {
                            fields
                                .iter()
                                .find(|(name, _)| name == prop_name)
                                .map(|(_, idx)| *idx as u16)
                        }),
                    _ => self
                        .register_object_fields
                        .get(&object.id)
                        .and_then(|fields| {
                            fields
                                .iter()
                                .find(|(name, _)| name == prop_name)
                                .map(|(_, idx)| *idx as u16)
                        }),
                };

                if let Some(idx) = obj_field_idx {
                    let field_ty = infer_field_from_expr_type(self)
                        .map(|resolved| match resolved {
                            ResolvedMemberSlot::Concrete { field_ty, .. }
                            | ResolvedMemberSlot::Shape { field_ty, .. } => field_ty,
                        })
                        .unwrap_or(UNRESOLVED);
                    if let Some((shape_id, slot)) =
                        self.structural_shape_slot_from_expr(&member.object, prop_name)
                    {
                        Some(ResolvedMemberSlot::Shape {
                            shape_id,
                            field_index: slot,
                            field_ty,
                        })
                    } else {
                        Some(ResolvedMemberSlot::Concrete {
                            field_index: idx,
                            field_ty,
                        })
                    }
                } else {
                    // Fall back to type-based field resolution (for function parameters typed as object types)
                    infer_field_from_expr_type(self)
                }
            }
        };

        // Check if the object is a TypeVar (generic parameter) — emit LateBoundMember
        // so the post-monomorphization pass can resolve to the correct opcode.
        if obj_ty_id == UNRESOLVED_TYPE_ID && nominal_type_id.is_none() {
            let expr_ty = self.get_expr_type(&member.object);
            let is_typevar = self
                .type_ctx
                .get(expr_ty)
                .is_some_and(|ty| matches!(ty, crate::parser::types::ty::Type::TypeVar(_)));
            // Also check register type for TypeVar
            let is_typevar = is_typevar
                || self
                    .type_ctx
                    .get(object.ty)
                    .is_some_and(|ty| matches!(ty, crate::parser::types::ty::Type::TypeVar(_)));

            if is_typevar {
                // Resolve dest type from the constraint's property if possible
                let constraint_prop_ty =
                    self.resolve_typevar_property_type(&member.object, prop_name);
                let dest_ty = constraint_prop_ty.unwrap_or(UNRESOLVED);
                let dest = self.alloc_register(dest_ty);
                self.emit(IrInstr::LateBoundMember {
                    dest: dest.clone(),
                    object,
                    property: prop_name.to_string(),
                });
                return dest;
            }
        }

        if let Some(resolved_field) = resolved_field {
            let object_id = object.id;
            let dest = self.alloc_register(match resolved_field {
                ResolvedMemberSlot::Concrete { field_ty, .. }
                | ResolvedMemberSlot::Shape { field_ty, .. } => field_ty,
            });
            match resolved_field {
                ResolvedMemberSlot::Concrete {
                    field_index,
                    field_ty,
                } => {
                if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                    let obj_name = match &*member.object {
                        Expression::Identifier(i) => self.interner.resolve(i.name).to_string(),
                        _ => "<expr>".to_string(),
                    };
                    eprintln!(
                        "[lower] LoadFieldExact: {}.{} field_index={}",
                        obj_name, prop_name, field_index
                    );
                }
                self.emit(IrInstr::LoadFieldExact {
                    dest: dest.clone(),
                    object,
                    field: field_index,
                    optional: member.optional,
                });
                if !self.emit_projected_shape_registration_for_register_type(&dest, field_ty) {
                    self.emit_structural_slot_registration_for_type(dest.clone(), field_ty);
                }
                if let Some(nested_layout) = self
                    .register_nested_object_fields
                    .get(&(object_id, field_index))
                    .cloned()
                {
                    self.register_object_fields.insert(dest.id, nested_layout);
                }
                }
                ResolvedMemberSlot::Shape {
                    shape_id,
                    field_index,
                    ..
                } => {
                if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                    let obj_name = match &*member.object {
                        Expression::Identifier(i) => self.interner.resolve(i.name).to_string(),
                        _ => "<expr>".to_string(),
                    };
                    eprintln!(
                        "[lower] LoadFieldShape: {}.{} field_index={} shape={:016x}",
                        obj_name, prop_name, field_index, shape_id
                    );
                }
                self.emit_structural_shape_name_registration_for_expr(&member.object);
                self.emit(IrInstr::LoadFieldShape {
                    dest: dest.clone(),
                    object,
                    shape_id,
                    field: field_index,
                    optional: member.optional,
                });
                }
            }
            return dest;
        }

        if nominal_type_id.is_none() && std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
            eprintln!(
                "[lower] unresolved member object AST: {:?}",
                &*member.object
            );
        }

        let class_name = nominal_type_id.and_then(|cid| {
            self.class_map.iter().find_map(|(&sym, &id)| {
                if id == cid {
                    Some(self.interner.resolve(sym).to_string())
                } else {
                    None
                }
            })
        });
        let object_type = class_name
            .map(|name| format!("class {}", name))
            .or_else(|| {
                if obj_ty_id != UNRESOLVED_TYPE_ID {
                    Some(format!("type id {}", obj_ty_id))
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "unknown receiver type".to_string());

        let receiver_requires_late_bound = match &*member.object {
            Expression::Identifier(obj_ident) => {
                self.identifier_requires_late_bound_dispatch(obj_ident.name)
            }
            _ => false,
        } || (nominal_type_id.is_none()
            && (self.type_requires_late_bound_dispatch(object.ty)
                || self.type_requires_late_bound_dispatch(self.get_expr_type(&member.object))));
        if nominal_type_id.is_none() && receiver_requires_late_bound {
            let dest = self.alloc_register(UNRESOLVED);
            self.emit(IrInstr::LateBoundMember {
                dest: dest.clone(),
                object,
                property: prop_name.to_string(),
            });
            return dest;
        }

        // Dynamic/member-latebound fallback: when receiver typing remains unresolved
        // (unknown/any/jsobject/etc), preserve runtime behavior instead of hard ICE.
        if nominal_type_id.is_none() && (obj_ty_id == UNRESOLVED_TYPE_ID || obj_ty_id == 6) {
            let dest = self.alloc_register(UNRESOLVED);
            self.emit(IrInstr::LateBoundMember {
                dest: dest.clone(),
                object,
                property: prop_name.to_string(),
            });
            return dest;
        }

        self.errors
            .push(crate::compiler::CompileError::InternalError {
                message: format!(
                    "unresolved member property '{}.{}': no class field, registry property, or object layout",
                    object_type, prop_name
                ),
            });
        self.lower_unresolved_poison()
    }

    fn lower_index(&mut self, index: &ast::IndexExpression, full_expr: &Expression) -> Register {
        let object = self.lower_expr(&index.object);
        let idx = self.lower_expr(&index.index);
        let elem_ty = self.get_expr_type(full_expr);
        let dest = self.alloc_register(elem_ty);

        if self.index_uses_dynamic_keyed_access(self.get_expr_type(&index.object)) {
            self.emit(IrInstr::DynGetKeyed {
                dest: dest.clone(),
                object,
                key: idx,
            });
        } else {
            self.emit(IrInstr::LoadElement {
                dest: dest.clone(),
                array: object,
                index: idx,
            });
        }
        dest
    }

    fn index_uses_dynamic_keyed_access(&self, object_ty: TypeId) -> bool {
        use crate::parser::types::{PrimitiveType, Type};

        match self.type_ctx.get(object_ty) {
            Some(Type::Array(_)) | Some(Type::Tuple(_)) => false,
            Some(Type::Primitive(PrimitiveType::String)) => false,
            Some(Type::Reference(type_ref)) => self
                .type_ctx
                .lookup_named_type(&type_ref.name)
                .map(|inner| self.index_uses_dynamic_keyed_access(inner))
                .unwrap_or(true),
            Some(Type::Union(union)) => union
                .members
                .iter()
                .copied()
                .any(|member| self.index_uses_dynamic_keyed_access(member)),
            Some(Type::Generic(generic)) => self.index_uses_dynamic_keyed_access(generic.base),
            Some(Type::TypeVar(type_var)) => type_var
                .constraint
                .or(type_var.default)
                .map(|inner| self.index_uses_dynamic_keyed_access(inner))
                .unwrap_or(true),
            Some(Type::JSObject)
            | Some(Type::Any)
            | Some(Type::Object(_))
            | Some(Type::Class(_)) => true,
            _ => true,
        }
    }

    fn is_append_index_pattern(&self, target: &Expression, index: &Expression) -> bool {
        let Expression::Member(member) = index else {
            return false;
        };

        let prop = self.interner.resolve(member.property.name);
        if prop != "length" {
            return false;
        }

        self.same_access_path(target, &member.object)
    }

    fn same_access_path(&self, a: &Expression, b: &Expression) -> bool {
        match (a, b) {
            (Expression::Identifier(ai), Expression::Identifier(bi)) => ai.name == bi.name,
            (Expression::This(_), Expression::This(_)) => true,
            (Expression::Parenthesized(ap), _) => self.same_access_path(&ap.expression, b),
            (_, Expression::Parenthesized(bp)) => self.same_access_path(a, &bp.expression),
            (Expression::Member(am), Expression::Member(bm)) => {
                am.property.name == bm.property.name
                    && self.same_access_path(&am.object, &bm.object)
            }
            _ => false,
        }
    }

    fn lower_array(&mut self, array: &ast::ArrayExpression, full_expr: &Expression) -> Register {
        // Check if any element is a spread
        let has_spread = array
            .elements
            .iter()
            .any(|elem_opt| matches!(elem_opt, Some(ast::ArrayElement::Spread(_))));

        let checker_ty = self.get_expr_type(full_expr);
        // NewArray always creates an array — use ARRAY_TYPE_ID when checker type is unknown
        let array_ty = if checker_ty.as_u32() == UNRESOLVED_TYPE_ID {
            TypeId::new(super::ARRAY_TYPE_ID)
        } else {
            checker_ty
        };

        if has_spread {
            // Spread present: build array imperatively with NewArray + push/loop
            let zero = self.emit_i32_const(0);
            let dest = self.alloc_register(array_ty);
            self.emit(IrInstr::NewArray {
                dest: dest.clone(),
                len: zero,
                elem_ty: TypeId::new(NUMBER_TYPE_ID),
            });

            for elem in array.elements.iter().flatten() {
                match elem {
                    ast::ArrayElement::Expression(expr) => {
                        let val = self.lower_expr(expr);
                        self.emit(IrInstr::ArrayPush {
                            array: dest.clone(),
                            element: val,
                        });
                    }
                    ast::ArrayElement::Spread(spread_expr) => {
                        let src_arr = self.lower_expr(spread_expr);
                        // Inline for-loop: for i in 0..src_arr.length { dest.push(src_arr[i]) }
                        let len = self.alloc_register(TypeId::new(INT_TYPE_ID));
                        self.emit(IrInstr::ArrayLen {
                            dest: len.clone(),
                            array: src_arr.clone(),
                        });
                        let i = self.emit_i32_const(0);

                        let header = self.alloc_block();
                        let body = self.alloc_block();
                        let exit = self.alloc_block();

                        self.set_terminator(crate::compiler::ir::Terminator::Jump(header));

                        // Header: i < len?
                        self.current_function_mut()
                            .add_block(crate::ir::BasicBlock::with_label(header, "spread.hdr"));
                        self.current_block = header;
                        let cond = self.alloc_register(TypeId::new(BOOLEAN_TYPE_ID));
                        self.emit(IrInstr::BinaryOp {
                            dest: cond.clone(),
                            op: BinaryOp::Less,
                            left: i.clone(),
                            right: len.clone(),
                        });
                        self.set_terminator(crate::compiler::ir::Terminator::Branch {
                            cond,
                            then_block: body,
                            else_block: exit,
                        });

                        // Body: elem = src_arr[i]; dest.push(elem); i++
                        self.current_function_mut()
                            .add_block(crate::ir::BasicBlock::with_label(body, "spread.body"));
                        self.current_block = body;
                        let elem = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
                        self.emit(IrInstr::LoadElement {
                            dest: elem.clone(),
                            array: src_arr.clone(),
                            index: i.clone(),
                        });
                        self.emit(IrInstr::ArrayPush {
                            array: dest.clone(),
                            element: elem,
                        });
                        let one = self.emit_i32_const(1);
                        self.emit(IrInstr::BinaryOp {
                            dest: i.clone(),
                            op: BinaryOp::Add,
                            left: i.clone(),
                            right: one,
                        });
                        self.set_terminator(crate::compiler::ir::Terminator::Jump(header));

                        // Exit
                        self.current_function_mut()
                            .add_block(crate::ir::BasicBlock::with_label(exit, "spread.exit"));
                        self.current_block = exit;
                    }
                }
            }
            dest
        } else {
            // No spread: use efficient ArrayLiteral path
            let mut elements = Vec::new();
            for elem in array.elements.iter().flatten() {
                match elem {
                    ast::ArrayElement::Expression(expr) => {
                        elements.push(self.lower_expr(expr));
                    }
                    ast::ArrayElement::Spread(_) => unreachable!(),
                }
            }
            let elem_ty = elements
                .first()
                .map(|r| r.ty)
                .unwrap_or(TypeId::new(NUMBER_TYPE_ID));
            let element_layout = if elements.is_empty() {
                None
            } else {
                let mut expected: Option<Vec<(String, usize)>> = None;
                let mut consistent = true;
                for reg in &elements {
                    let Some(layout) = self.register_object_fields.get(&reg.id).cloned() else {
                        consistent = false;
                        break;
                    };
                    match &expected {
                        None => expected = Some(layout),
                        Some(existing) if *existing == layout => {}
                        Some(_) => {
                            consistent = false;
                            break;
                        }
                    }
                }
                if consistent {
                    expected
                } else {
                    None
                }
            };
            let dest = self.alloc_register(array_ty);
            self.emit(IrInstr::ArrayLiteral {
                dest: dest.clone(),
                elements,
                elem_ty,
            });
            if let Some(layout) = element_layout {
                self.register_array_element_object_fields
                    .insert(dest.id, layout);
            }
            dest
        }
    }

    fn object_property_name(&self, key: &ast::PropertyKey, fallback_idx: usize) -> String {
        match key {
            ast::PropertyKey::Identifier(ident) => self.interner.resolve(ident.name).to_string(),
            ast::PropertyKey::StringLiteral(lit) => self.interner.resolve(lit.value).to_string(),
            ast::PropertyKey::IntLiteral(lit) => lit.value.to_string(),
            ast::PropertyKey::Computed(_) => fallback_idx.to_string(),
        }
    }

    fn include_spread_field(&self, name: &str) -> bool {
        match &self.object_spread_target_filter {
            Some(filter) => filter.contains(name),
            None => true,
        }
    }

    fn canonical_object_slot_index(
        &self,
        obj: &crate::parser::types::ty::ObjectType,
        prop_name: &str,
    ) -> Option<u16> {
        let mut names: Vec<&str> = obj.properties.iter().map(|p| p.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        let idx = names.iter().position(|name| *name == prop_name)?;
        u16::try_from(idx).ok()
    }

    fn canonical_object_slot_layout(
        &self,
        obj: &crate::parser::types::ty::ObjectType,
    ) -> Option<Vec<(String, u16)>> {
        let mut names: Vec<String> = obj.properties.iter().map(|p| p.name.clone()).collect();
        names.sort_unstable();
        names.dedup();
        let mut layout = Vec::with_capacity(names.len());
        for (idx, name) in names.into_iter().enumerate() {
            let slot = u16::try_from(idx).ok()?;
            layout.push((name, slot));
        }
        Some(layout)
    }

    fn collect_structural_slot_names_from_type(
        &self,
        ty_id: TypeId,
        names: &mut FxHashSet<String>,
        visited: &mut FxHashSet<TypeId>,
    ) -> bool {
        if !visited.insert(ty_id) {
            return false;
        }
        let Some(ty) = self.type_ctx.get(ty_id) else {
            return false;
        };
        match ty {
            crate::parser::types::Type::Object(obj) => {
                names.extend(obj.properties.iter().map(|property| property.name.clone()));
                true
            }
            crate::parser::types::Type::Class(class) => {
                let mut found = false;
                for property in &class.properties {
                    if property.visibility == crate::parser::ast::Visibility::Public {
                        names.insert(property.name.clone());
                        found = true;
                    }
                }
                for method in &class.methods {
                    if method.visibility == crate::parser::ast::Visibility::Public {
                        names.insert(method.name.clone());
                        found = true;
                    }
                }
                if let Some(parent) = class.extends {
                    found |= self.collect_structural_slot_names_from_type(parent, names, visited);
                }
                found
            }
            crate::parser::types::Type::Interface(interface) => {
                names.extend(
                    interface
                        .properties
                        .iter()
                        .map(|property| property.name.clone()),
                );
                names.extend(interface.methods.iter().map(|method| method.name.clone()));
                let mut found = true;
                for &parent in &interface.extends {
                    found |= self.collect_structural_slot_names_from_type(parent, names, visited);
                }
                found
            }
            crate::parser::types::Type::Reference(reference) => {
                let mut found = false;
                if let Some(fields) = self.type_alias_object_fields.get(&reference.name) {
                    names.extend(fields.iter().map(|(name, _, _)| name.clone()));
                    found = true;
                }
                if let Some(named_ty) = self.type_ctx.lookup_named_type(&reference.name) {
                    found |= self.collect_structural_slot_names_from_type(named_ty, names, visited);
                }
                found
            }
            crate::parser::types::Type::TypeVar(type_var) => {
                let mut found = false;
                if let Some(constraint) = type_var.constraint {
                    found |=
                        self.collect_structural_slot_names_from_type(constraint, names, visited);
                }
                if let Some(default) = type_var.default {
                    found |= self.collect_structural_slot_names_from_type(default, names, visited);
                }
                found
            }
            crate::parser::types::Type::Generic(generic) => {
                self.collect_structural_slot_names_from_type(generic.base, names, visited)
            }
            crate::parser::types::Type::Union(union) => {
                let mut found = false;
                for &member in &union.members {
                    found |= self.collect_structural_slot_names_from_type(member, names, visited);
                }
                found
            }
            _ => false,
        }
    }

    fn resolve_concrete_class_type_for_runtime_slots(
        &self,
        ty_id: TypeId,
        visited: &mut FxHashSet<TypeId>,
    ) -> Option<TypeId> {
        if !visited.insert(ty_id) {
            return None;
        }
        let ty = self.type_ctx.get(ty_id)?;
        match ty {
            crate::parser::types::Type::Class(class_ty) => self
                .nominal_type_id_from_type_name(&class_ty.name)
                .map(|_| ty_id),
            crate::parser::types::Type::Reference(reference) => self
                .type_ctx
                .lookup_named_type(&reference.name)
                .and_then(|named| {
                    self.resolve_concrete_class_type_for_runtime_slots(named, visited)
                }),
            crate::parser::types::Type::Generic(generic) => {
                self.resolve_concrete_class_type_for_runtime_slots(generic.base, visited)
            }
            crate::parser::types::Type::TypeVar(type_var) => {
                type_var.constraint.or(type_var.default).and_then(|inner| {
                    self.resolve_concrete_class_type_for_runtime_slots(inner, visited)
                })
            }
            crate::parser::types::Type::Union(union) => {
                let mut resolved: Option<TypeId> = None;
                for &member in &union.members {
                    let Some(member_ty) = self.type_ctx.get(member) else {
                        continue;
                    };
                    if matches!(
                        member_ty,
                        crate::parser::types::Type::Primitive(
                            crate::parser::types::PrimitiveType::Null
                        )
                    ) {
                        continue;
                    }
                    let member_class =
                        self.resolve_concrete_class_type_for_runtime_slots(member, visited)?;
                    if let Some(existing) = resolved {
                        if existing != member_class {
                            return None;
                        }
                    } else {
                        resolved = Some(member_class);
                    }
                }
                resolved
            }
            _ => None,
        }
    }

    fn collect_ordered_public_class_slot_names(
        &self,
        ty_id: TypeId,
        names: &mut Vec<String>,
        seen: &mut FxHashSet<String>,
        visited: &mut FxHashSet<TypeId>,
    ) -> bool {
        if !visited.insert(ty_id) {
            return false;
        }
        let Some(ty) = self.type_ctx.get(ty_id) else {
            return false;
        };
        match ty {
            crate::parser::types::Type::Class(class) => {
                let mut found = false;
                if let Some(parent) = class.extends {
                    found |=
                        self.collect_ordered_public_class_slot_names(parent, names, seen, visited);
                }
                for property in &class.properties {
                    if property.visibility == crate::parser::ast::Visibility::Public
                        && seen.insert(property.name.clone())
                    {
                        names.push(property.name.clone());
                        found = true;
                    }
                }
                for method in &class.methods {
                    if method.visibility == crate::parser::ast::Visibility::Public
                        && seen.insert(method.name.clone())
                    {
                        names.push(method.name.clone());
                        found = true;
                    }
                }
                found
            }
            crate::parser::types::Type::Reference(reference) => self
                .type_ctx
                .lookup_named_type(&reference.name)
                .is_some_and(|named| {
                    self.collect_ordered_public_class_slot_names(named, names, seen, visited)
                }),
            crate::parser::types::Type::Generic(generic) => {
                self.collect_ordered_public_class_slot_names(generic.base, names, seen, visited)
            }
            crate::parser::types::Type::TypeVar(type_var) => type_var
                .constraint
                .or(type_var.default)
                .is_some_and(|inner| {
                    self.collect_ordered_public_class_slot_names(inner, names, seen, visited)
                }),
            _ => false,
        }
    }

    pub(super) fn ordered_slot_names_for_concrete_classish_type(
        &self,
        ty_id: TypeId,
    ) -> Option<Vec<String>> {
        let mut visited = FxHashSet::default();
        let class_ty = self.resolve_concrete_class_type_for_runtime_slots(ty_id, &mut visited)?;
        let mut names = Vec::new();
        let mut seen = FxHashSet::default();
        let mut ordered_visited = FxHashSet::default();
        if !self.collect_ordered_public_class_slot_names(
            class_ty,
            &mut names,
            &mut seen,
            &mut ordered_visited,
        ) {
            return None;
        }
        if names.is_empty() {
            None
        } else {
            Some(names)
        }
    }

    pub(super) fn structural_slot_layout_from_type(
        &self,
        ty_id: TypeId,
    ) -> Option<Vec<(String, u16)>> {
        let mut names = FxHashSet::default();
        let mut visited = FxHashSet::default();
        if !self.collect_structural_slot_names_from_type(ty_id, &mut names, &mut visited) {
            return None;
        }
        let mut names: Vec<String> = names.into_iter().collect();
        names.sort_unstable();
        names.dedup();
        let mut layout = Vec::with_capacity(names.len());
        for (idx, name) in names.into_iter().enumerate() {
            let slot = u16::try_from(idx).ok()?;
            layout.push((name, slot));
        }
        Some(layout)
    }

    fn structural_layout_id_from_ordered_names(&mut self, ordered_names: &[String]) -> u32 {
        let layout_id = crate::vm::object::layout_id_from_ordered_names(ordered_names);
        self.module_structural_layouts
            .entry(layout_id)
            .or_insert_with(|| ordered_names.to_vec());
        layout_id
    }

    fn structural_slot_index_from_type(&self, ty_id: TypeId, prop_name: &str) -> Option<u16> {
        self.structural_slot_layout_from_type(ty_id)?
            .into_iter()
            .find(|(name, _)| name == prop_name)
            .map(|(_, slot)| slot)
    }

    fn structural_shape_id_from_type(&self, ty_id: TypeId) -> Option<u64> {
        let names = self
            .structural_slot_layout_from_type(ty_id)?
            .into_iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>();
        if names.is_empty() {
            return None;
        }
        Some(crate::vm::object::shape_id_from_member_names(&names))
    }

    fn structural_shape_slot_from_expr(
        &self,
        object_expr: &Expression,
        prop_name: &str,
    ) -> Option<(u64, u16)> {
        if let Some(layout) = self.projected_structural_layout_from_expr(object_expr) {
            let slot = layout
                .iter()
                .find(|(name, _)| name == prop_name)
                .map(|(_, slot)| *slot)?;
            let names = layout
                .iter()
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>();
            let shape_id = crate::vm::object::shape_id_from_member_names(&names);
            return Some((shape_id, slot));
        }

        let expr_ty = self.get_expr_type(object_expr);
        let shape_id = self.structural_shape_id_from_type(expr_ty)?;
        let slot = self.structural_slot_index_from_type(expr_ty, prop_name)?;
        Some((shape_id, slot))
    }

    fn emit_structural_shape_name_registration_for_expr(
        &mut self,
        object_expr: &Expression,
    ) -> bool {
        if let Some(layout) = self.projected_structural_layout_from_expr(object_expr) {
            let names = layout
                .into_iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>();
            self.emit_structural_shape_name_registration_for_ordered_names(names);
            return true;
        }

        let expr_ty = self.get_expr_type(object_expr);
        let Some(layout) = self.structural_slot_layout_from_type(expr_ty) else {
            return false;
        };
        self.emit_structural_shape_name_registration_for_ordered_names(
            layout.into_iter().map(|(name, _)| name).collect(),
        );
        true
    }

    fn emit_member_store(
        &mut self,
        object_expr: &Expression,
        object: Register,
        fallback_field: u16,
        prop_name: &str,
        value: Register,
    ) {
        if let Some((shape_id, slot)) = self.structural_shape_slot_from_expr(object_expr, prop_name)
        {
            if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                let obj_name = match object_expr {
                    Expression::Identifier(i) => self.interner.resolve(i.name).to_string(),
                    _ => "<expr>".to_string(),
                };
                eprintln!(
                    "[lower] StoreFieldShape: {}.{} field_index={} shape={:016x}",
                    obj_name, prop_name, slot, shape_id
                );
            }
            self.emit_structural_shape_name_registration_for_expr(object_expr);
            self.emit(IrInstr::StoreFieldShape {
                object,
                shape_id,
                field: slot,
                value,
            });
        } else {
            self.emit(IrInstr::StoreFieldExact {
                object,
                field: fallback_field,
                value,
            });
        }
    }

    fn structural_field_type_from_type_inner(
        &self,
        ty_id: TypeId,
        prop_name: &str,
        visited: &mut FxHashSet<TypeId>,
    ) -> Option<TypeId> {
        if !visited.insert(ty_id) {
            return None;
        }
        let Some(ty) = self.type_ctx.get(ty_id) else {
            return None;
        };
        match ty {
            crate::parser::types::Type::Object(obj) => obj
                .properties
                .iter()
                .find(|property| property.name == prop_name)
                .map(|property| property.ty),
            crate::parser::types::Type::Class(class) => class
                .properties
                .iter()
                .find(|property| {
                    property.name == prop_name
                        && property.visibility == crate::parser::ast::Visibility::Public
                })
                .map(|property| property.ty)
                .or_else(|| {
                    class
                        .methods
                        .iter()
                        .find(|method| {
                            method.name == prop_name
                                && method.visibility == crate::parser::ast::Visibility::Public
                        })
                        .map(|method| method.ty)
                })
                .or_else(|| {
                    class.extends.and_then(|parent| {
                        self.structural_field_type_from_type_inner(parent, prop_name, visited)
                    })
                }),
            crate::parser::types::Type::Interface(interface) => interface
                .properties
                .iter()
                .find(|property| property.name == prop_name)
                .map(|property| property.ty)
                .or_else(|| {
                    interface
                        .methods
                        .iter()
                        .find(|method| method.name == prop_name)
                        .map(|method| method.ty)
                })
                .or_else(|| {
                    for &parent in &interface.extends {
                        if let Some(ty) =
                            self.structural_field_type_from_type_inner(parent, prop_name, visited)
                        {
                            return Some(ty);
                        }
                    }
                    None
                }),
            crate::parser::types::Type::Reference(reference) => {
                if let Some((_, field_ty)) =
                    self.type_alias_field_lookup(&reference.name, prop_name)
                {
                    return Some(field_ty);
                }
                self.type_ctx
                    .lookup_named_type(&reference.name)
                    .and_then(|named| {
                        self.structural_field_type_from_type_inner(named, prop_name, visited)
                    })
            }
            crate::parser::types::Type::TypeVar(type_var) => type_var
                .constraint
                .and_then(|constraint| {
                    self.structural_field_type_from_type_inner(constraint, prop_name, visited)
                })
                .or_else(|| {
                    type_var.default.and_then(|default| {
                        self.structural_field_type_from_type_inner(default, prop_name, visited)
                    })
                }),
            crate::parser::types::Type::Generic(generic) => {
                self.structural_field_type_from_type_inner(generic.base, prop_name, visited)
            }
            crate::parser::types::Type::Union(union) => {
                let mut found = None;
                for &member in &union.members {
                    let Some(member_ty) =
                        self.structural_field_type_from_type_inner(member, prop_name, visited)
                    else {
                        continue;
                    };
                    match found {
                        None => found = Some(member_ty),
                        Some(existing) if existing == member_ty => {}
                        Some(_) => return Some(UNRESOLVED),
                    }
                }
                found
            }
            _ => None,
        }
    }

    fn structural_field_type_from_type(&self, ty_id: TypeId, prop_name: &str) -> Option<TypeId> {
        let mut visited = FxHashSet::default();
        self.structural_field_type_from_type_inner(ty_id, prop_name, &mut visited)
    }

    fn spread_source_fields_from_type(&self, ty: TypeId) -> Option<SpreadSourceFields> {
        match self.type_ctx.get(ty)? {
            crate::parser::types::Type::Object(obj) => {
                let fields = self.canonical_object_slot_layout(obj)?;
                let shape_id = self.structural_shape_id_from_type(ty)?;
                Some(SpreadSourceFields::Shape { shape_id, fields })
            }
            crate::parser::types::Type::Class(_) => {
                let nominal_type_id = self.nominal_type_id_from_type_id(ty)?;
                Some(SpreadSourceFields::Concrete(
                    self.get_all_fields(nominal_type_id)
                        .into_iter()
                        .map(|f| (self.interner.resolve(f.name).to_string(), f.index))
                        .collect(),
                ))
            }
            crate::parser::types::Type::TypeVar(tv) => tv
                .constraint
                .and_then(|constraint| self.spread_source_fields_from_type(constraint)),
            crate::parser::types::Type::Reference(_)
            | crate::parser::types::Type::Interface(_)
            | crate::parser::types::Type::Generic(_)
            | crate::parser::types::Type::Union(_) => {
                let fields = self.structural_slot_layout_from_type(ty)?;
                let shape_id = self.structural_shape_id_from_type(ty)?;
                Some(SpreadSourceFields::Shape { shape_id, fields })
            }
            _ => None,
        }
    }

    fn resolve_spread_source_fields(
        &self,
        spread_expr: &ast::Expression,
        spread_reg: Option<&Register>,
    ) -> Option<SpreadSourceFields> {
        if let Some(reg) = spread_reg {
            if let Some(fields) = self.register_object_fields.get(&reg.id) {
                let mut ordered: Vec<(String, u16)> = fields
                    .iter()
                    .map(|(name, idx)| (name.clone(), *idx as u16))
                    .collect();
                ordered.sort_by_key(|(_, idx)| *idx);
                return Some(SpreadSourceFields::Concrete(ordered));
            }
        }

        if let ast::Expression::Identifier(ident) = spread_expr {
            if let Some(fields) = self.variable_object_fields.get(&ident.name) {
                let mut ordered: Vec<(String, u16)> = fields
                    .iter()
                    .map(|(name, idx)| (name.clone(), *idx as u16))
                    .collect();
                ordered.sort_by_key(|(_, idx)| *idx);
                return Some(SpreadSourceFields::Concrete(ordered));
            }
        }

        if let Some(nominal_type_id) = self.infer_nominal_type_id(spread_expr) {
            return Some(SpreadSourceFields::Concrete(
                self.get_all_fields(nominal_type_id)
                    .into_iter()
                    .map(|f| (self.interner.resolve(f.name).to_string(), f.index))
                    .collect(),
            ));
        }

        let spread_ty = self.get_expr_type(spread_expr);
        self.spread_source_fields_from_type(spread_ty)
    }

    fn lower_object(&mut self, object: &ast::ObjectExpression, full_expr: &Expression) -> Register {
        let checker_ty = self.get_expr_type(full_expr);
        let object_ty = if checker_ty.as_u32() == UNRESOLVED_TYPE_ID {
            TypeId::new(NUMBER_TYPE_ID)
        } else {
            checker_ty
        };
        let dest = self.alloc_register(object_ty);
        let mut field_names = Vec::<String>::new();
        let mut field_index_map = FxHashMap::<String, usize>::default();

        for (idx, prop) in object.properties.iter().enumerate() {
            match prop {
                ast::ObjectProperty::Property(p) => {
                    let name = self.object_property_name(&p.key, idx);
                    if !field_index_map.contains_key(&name) {
                        let next_idx = field_names.len();
                        field_names.push(name.clone());
                        field_index_map.insert(name, next_idx);
                    }
                }
                ast::ObjectProperty::Spread(spread) => {
                    if let Some(source_fields) =
                        self.resolve_spread_source_fields(&spread.argument, None)
                    {
                        let fields = match source_fields {
                            SpreadSourceFields::Concrete(fields)
                            | SpreadSourceFields::Shape { fields, .. } => fields,
                        };
                        for (field_name, _) in fields {
                            if !self.include_spread_field(&field_name) {
                                continue;
                            }
                            if !field_index_map.contains_key(&field_name) {
                                let next_idx = field_names.len();
                                field_names.push(field_name.clone());
                                field_index_map.insert(field_name, next_idx);
                            }
                        }
                    } else {
                        self.errors.push(CompileError::UnsupportedFeature {
                            feature: "object spread requires statically known source fields"
                                .to_string(),
                        });
                    }
                }
            }
        }
        if let Some(target_names) = self.object_literal_target_layout.clone() {
            for name in target_names {
                if !field_index_map.contains_key(&name) {
                    let next_idx = field_names.len();
                    field_names.push(name.clone());
                    field_index_map.insert(name, next_idx);
                }
            }
        }

        // If checker type carries a structural/union layout, use it as the
        // canonical slot space so all variants share stable slots.
        if let Some(target_layout) = self.structural_slot_layout_from_type(checker_ty) {
            for (name, _) in target_layout {
                if !field_index_map.contains_key(&name) {
                    let next_idx = field_names.len();
                    field_names.push(name.clone());
                    field_index_map.insert(name, next_idx);
                }
            }
        }

        // Canonicalize object literal slot layout by key so structural signatures
        // map to stable runtime slots across modules.
        field_names.sort_unstable();
        field_names.dedup();
        field_index_map.clear();
        for (idx, name) in field_names.iter().enumerate() {
            field_index_map.insert(name.clone(), idx);
        }

        let null_value = self.lower_null_literal();
        let initial_fields: Vec<(u16, Register)> = (0..field_names.len())
            .map(|i| (i as u16, null_value.clone()))
            .collect();
        let type_index = self.structural_layout_id_from_ordered_names(&field_names);

        self.emit(IrInstr::ObjectLiteral {
            dest: dest.clone(),
            type_index,
            fields: initial_fields,
        });

        for (idx, prop) in object.properties.iter().enumerate() {
            match prop {
                ast::ObjectProperty::Property(p) => {
                    let field_name = self.object_property_name(&p.key, idx);
                    let Some(&field_index) = field_index_map.get(&field_name) else {
                        continue;
                    };
                    let value = self.lower_expr(&p.value);
                    if let Some(nested_layout) = self.register_object_fields.get(&value.id).cloned()
                    {
                        self.register_nested_object_fields
                            .insert((dest.id, field_index as u16), nested_layout);
                    }
                    if let Some(elem_layout) = self
                        .register_array_element_object_fields
                        .get(&value.id)
                        .cloned()
                    {
                        self.register_nested_array_element_object_fields
                            .insert((dest.id, field_index as u16), elem_layout);
                    }
                    self.emit(IrInstr::StoreFieldExact {
                        object: dest.clone(),
                        field: field_index as u16,
                        value,
                    });
                }
                ast::ObjectProperty::Spread(spread) => {
                    let spread_reg = self.lower_expr(&spread.argument);
                    let Some(source_fields) =
                        self.resolve_spread_source_fields(&spread.argument, Some(&spread_reg))
                    else {
                        self.errors.push(CompileError::UnsupportedFeature {
                            feature: "object spread requires statically known source fields"
                                .to_string(),
                        });
                        continue;
                    };
                    let (shape_id, fields) = match source_fields {
                        SpreadSourceFields::Concrete(fields) => (None, fields),
                        SpreadSourceFields::Shape { shape_id, fields } => (Some(shape_id), fields),
                    };
                    for (field_name, src_field_idx) in fields {
                        if !self.include_spread_field(&field_name) {
                            continue;
                        }
                        let Some(&dest_idx) = field_index_map.get(&field_name) else {
                            continue;
                        };
                        let field_val = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
                        if let Some(shape_id) = shape_id {
                            self.emit(IrInstr::LoadFieldShape {
                                dest: field_val.clone(),
                                object: spread_reg.clone(),
                                shape_id,
                                field: src_field_idx,
                                optional: false,
                            });
                        } else {
                            self.emit(IrInstr::LoadFieldExact {
                                dest: field_val.clone(),
                                object: spread_reg.clone(),
                                field: src_field_idx,
                                optional: false,
                            });
                        }
                        if let Some(nested_layout) = self
                            .register_nested_object_fields
                            .get(&(spread_reg.id, src_field_idx))
                            .cloned()
                        {
                            self.register_nested_object_fields
                                .insert((dest.id, dest_idx as u16), nested_layout);
                        }
                        if let Some(elem_layout) = self
                            .register_nested_array_element_object_fields
                            .get(&(spread_reg.id, src_field_idx))
                            .cloned()
                        {
                            self.register_nested_array_element_object_fields
                                .insert((dest.id, dest_idx as u16), elem_layout);
                        }
                        self.emit(IrInstr::StoreFieldExact {
                            object: dest.clone(),
                            field: dest_idx as u16,
                            value: field_val,
                        });
                    }
                }
            }
        }

        let field_layout: Vec<(String, usize)> = field_names
            .iter()
            .enumerate()
            .map(|(idx, name)| (name.clone(), idx))
            .collect();
        self.register_object_fields.insert(dest.id, field_layout);
        self.emit_structural_shape_name_registration_for_ordered_names(field_names);

        dest
    }

    fn lower_assignment(&mut self, assign: &ast::AssignmentExpression) -> Register {
        // ??= is short-circuiting: only evaluate and assign RHS if LHS is null
        if assign.operator == AssignmentOperator::NullCoalesceAssign {
            let current = self.lower_expr(&assign.left);
            let assign_block = self.alloc_block();
            let merge_block = self.alloc_block();

            self.set_terminator(Terminator::BranchIfNull {
                value: current.clone(),
                null_block: assign_block,
                not_null_block: merge_block,
            });

            // Null path: evaluate RHS and assign to LHS
            self.current_function_mut()
                .add_block(crate::ir::BasicBlock::with_label(
                    assign_block,
                    "nca.assign",
                ));
            self.current_block = assign_block;
            let rhs = self.lower_expr(&assign.right);
            // Store to LHS
            match &*assign.left {
                Expression::Identifier(ident) => {
                    let mut assigned_symbol = false;
                    let mut assigned_local_idx: Option<u16> = None;
                    if let Some(&local_idx) = self.local_map.get(&ident.name) {
                        assigned_symbol = true;
                        assigned_local_idx = Some(local_idx);
                        if self.refcell_registers.contains_key(&local_idx) {
                            let refcell_reg = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
                            self.emit(IrInstr::LoadLocal {
                                dest: refcell_reg.clone(),
                                index: local_idx,
                            });
                            self.emit(IrInstr::StoreRefCell {
                                refcell: refcell_reg,
                                value: rhs,
                            });
                        } else {
                            self.emit(IrInstr::StoreLocal {
                                index: local_idx,
                                value: rhs,
                            });
                        }
                    } else if let Some(&global_idx) = self.module_var_globals.get(&ident.name) {
                        assigned_symbol = true;
                        // Module-level variable — store via global slot
                        self.emit(IrInstr::StoreGlobal {
                            index: global_idx,
                            value: rhs,
                        });
                    } else if let Some(binding) = self
                        .current_method_env_globals
                        .as_ref()
                        .and_then(|m| m.get(&ident.name))
                        .copied()
                    {
                        assigned_symbol = true;
                        if binding.is_refcell {
                            let refcell_reg = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
                            self.emit(IrInstr::LoadGlobal {
                                dest: refcell_reg.clone(),
                                index: binding.global_idx,
                            });
                            self.emit(IrInstr::StoreRefCell {
                                refcell: refcell_reg,
                                value: rhs,
                            });
                        } else {
                            self.emit(IrInstr::StoreGlobal {
                                index: binding.global_idx,
                                value: rhs,
                            });
                        }
                    } else if let Some(idx) =
                        self.captures.iter().position(|c| c.symbol == ident.name)
                    {
                        assigned_symbol = true;
                        // Captured variable from outer scope
                        let is_refcell = self.captures[idx].is_refcell;
                        let capture_idx = self.captures[idx].capture_idx;
                        if is_refcell {
                            let refcell_reg = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
                            self.emit(IrInstr::LoadCaptured {
                                dest: refcell_reg.clone(),
                                index: capture_idx,
                            });
                            self.emit(IrInstr::StoreRefCell {
                                refcell: refcell_reg,
                                value: rhs,
                            });
                        } else {
                            self.emit(IrInstr::StoreCaptured {
                                index: capture_idx,
                                value: rhs,
                            });
                        }
                    } else if let Some(ref ancestors) = self.ancestor_variables.clone() {
                        // Lazy ancestor capture (mirror from regular assignment)
                        if let Some(ancestor_var) = ancestors.get(&ident.name) {
                            assigned_symbol = true;
                            let ty = ancestor_var.ty;
                            let is_refcell = ancestor_var.is_refcell;
                            let capture_idx = self.next_capture_slot;
                            self.next_capture_slot += 1;
                            self.captures.push(super::CaptureInfo {
                                symbol: ident.name,
                                source: ancestor_var.source,
                                capture_idx,
                                ty,
                                is_refcell,
                            });
                            if is_refcell {
                                let refcell_reg = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
                                self.emit(IrInstr::LoadCaptured {
                                    dest: refcell_reg.clone(),
                                    index: capture_idx,
                                });
                                self.emit(IrInstr::StoreRefCell {
                                    refcell: refcell_reg,
                                    value: rhs,
                                });
                            } else {
                                self.emit(IrInstr::StoreCaptured {
                                    index: capture_idx,
                                    value: rhs,
                                });
                            }
                        } else if let Some(&global_idx) = self.module_var_globals.get(&ident.name) {
                            assigned_symbol = true;
                            self.emit(IrInstr::StoreGlobal {
                                index: global_idx,
                                value: rhs,
                            });
                        } else if let Some(binding) = self
                            .current_method_env_globals
                            .as_ref()
                            .and_then(|m| m.get(&ident.name))
                            .copied()
                        {
                            assigned_symbol = true;
                            if binding.is_refcell {
                                let refcell_reg = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
                                self.emit(IrInstr::LoadGlobal {
                                    dest: refcell_reg.clone(),
                                    index: binding.global_idx,
                                });
                                self.emit(IrInstr::StoreRefCell {
                                    refcell: refcell_reg,
                                    value: rhs,
                                });
                            } else {
                                self.emit(IrInstr::StoreGlobal {
                                    index: binding.global_idx,
                                    value: rhs,
                                });
                            }
                        }
                    }

                    // `??=` may assign RHS; keep callable hints conservative.
                    // If RHS isn't callable, drop callable/bound-method hints.
                    if assigned_symbol && !self.expression_is_callable_hint(&assign.right) {
                        self.callable_symbol_hints.remove(&ident.name);
                        self.bound_method_vars.remove(&ident.name);
                        if let Some(local_idx) = assigned_local_idx {
                            self.callable_local_hints.remove(&local_idx);
                        }
                    }
                }
                Expression::Member(member) => {
                    let prop_name = self.interner.resolve(member.property.name);
                    let nominal_type_id = if self.prefers_structural_member_projection(&member.object) {
                        None
                    } else {
                        self.infer_nominal_type_id(&member.object)
                    };
                    let object = self.lower_expr(&member.object);
                    let checker_obj_ty = self.get_expr_type(&member.object);
                    let allow_dynamic_any_write = self.type_is_dynamic_any_like(checker_obj_ty)
                        || self.type_is_dynamic_any_like(object.ty);
                    let obj_ty_id = {
                        let reg_ty = object.ty.as_u32();
                        let checker_ty = self.get_expr_type(&member.object).as_u32();
                        let reg_dispatch = self.normalize_type_for_dispatch(reg_ty);
                        let checker_dispatch = self.normalize_type_for_dispatch(checker_ty);
                        if reg_dispatch != UNRESOLVED_TYPE_ID && reg_dispatch != UNKNOWN_TYPE_ID {
                            reg_dispatch
                        } else if checker_dispatch != UNRESOLVED_TYPE_ID
                            && checker_dispatch != UNKNOWN_TYPE_ID
                        {
                            checker_dispatch
                        } else {
                            UNRESOLVED_TYPE_ID
                        }
                    };
                    if let Some(nominal_type_id) = nominal_type_id {
                        if let Some(field) = self
                            .get_all_fields(nominal_type_id)
                            .iter()
                            .rev()
                            .find(|f| self.interner.resolve(f.name) == prop_name)
                        {
                            self.emit(IrInstr::StoreFieldExact {
                                object,
                                field: field.index,
                                value: rhs,
                            });
                        } else {
                            if allow_dynamic_any_write {
                                let prop_reg = self.alloc_register(TypeId::new(STRING_TYPE_ID));
                                self.emit(IrInstr::Assign {
                                    dest: prop_reg.clone(),
                                    value: IrValue::Constant(IrConstant::String(
                                        prop_name.to_string(),
                                    )),
                                });
                                self.emit(IrInstr::NativeCall {
                                    dest: None,
                                    native_id: crate::compiler::native_id::REFLECT_SET,
                                    args: vec![object, prop_reg, rhs],
                                });
                            } else {
                                let class_name = self
                                    .class_map
                                    .iter()
                                    .find_map(|(&sym, &id)| {
                                        if id == nominal_type_id {
                                            Some(self.interner.resolve(sym).to_string())
                                        } else {
                                            None
                                        }
                                    })
                                    .unwrap_or_else(|| format!("class#{}", nominal_type_id.as_u32()));
                                self.errors
                                    .push(crate::compiler::CompileError::InternalError {
                                        message: format!(
                                            "unresolved member assignment '{}.{}': class field not found",
                                            class_name, prop_name
                                        ),
                                    });
                            }
                        }
                    } else if let Some(field_idx) = match &*member.object {
                        Expression::Identifier(ident) => self
                            .variable_object_fields
                            .get(&ident.name)
                            .and_then(|fields| {
                                fields
                                    .iter()
                                    .find(|(name, _)| name == prop_name)
                                    .map(|(_, idx)| *idx as u16)
                            }),
                        _ => self
                            .register_object_fields
                            .get(&object.id)
                            .and_then(|fields| {
                                fields
                                    .iter()
                                    .find(|(name, _)| name == prop_name)
                                    .map(|(_, idx)| *idx as u16)
                            }),
                    } {
                        self.emit_member_store(&member.object, object, field_idx, prop_name, rhs);
                    } else if obj_ty_id == JSON_TYPE_ID || obj_ty_id == JSON_OBJECT_TYPE_ID {
                        self.emit(IrInstr::DynSetProp {
                            object,
                            property: prop_name.to_string(),
                            value: rhs,
                        });
                    } else {
                        let prop_reg = self.alloc_register(TypeId::new(STRING_TYPE_ID));
                        self.emit(IrInstr::Assign {
                            dest: prop_reg.clone(),
                            value: IrValue::Constant(IrConstant::String(prop_name.to_string())),
                        });
                        self.emit(IrInstr::NativeCall {
                            dest: None,
                            native_id: crate::compiler::native_id::REFLECT_SET,
                            args: vec![object, prop_reg, rhs],
                        });
                    }
                }
                Expression::Index(index) => {
                    let object = self.lower_expr(&index.object);
                    if self.is_append_index_pattern(&index.object, &index.index) {
                        self.emit(IrInstr::ArrayPush {
                            array: object,
                            element: rhs,
                        });
                    } else {
                        let idx = self.lower_expr(&index.index);
                        if self.index_uses_dynamic_keyed_access(self.get_expr_type(&index.object)) {
                            self.emit(IrInstr::DynSetKeyed {
                                object,
                                key: idx,
                                value: rhs,
                            });
                        } else {
                            self.emit(IrInstr::StoreElement {
                                array: object,
                                index: idx,
                                value: rhs,
                            });
                        }
                    }
                }
                _ => {}
            }
            self.set_terminator(Terminator::Jump(merge_block));

            // Merge
            self.current_function_mut()
                .add_block(crate::ir::BasicBlock::with_label(merge_block, "nca.merge"));
            self.current_block = merge_block;

            return current;
        }

        // For compound assignment, we need to load the current value first
        let binary_op = match assign.operator {
            AssignmentOperator::Assign => None,
            AssignmentOperator::AddAssign => Some(BinaryOp::Add),
            AssignmentOperator::SubAssign => Some(BinaryOp::Sub),
            AssignmentOperator::MulAssign => Some(BinaryOp::Mul),
            AssignmentOperator::DivAssign => Some(BinaryOp::Div),
            AssignmentOperator::ModAssign => Some(BinaryOp::Mod),
            AssignmentOperator::AndAssign => Some(BinaryOp::BitAnd),
            AssignmentOperator::OrAssign => Some(BinaryOp::BitOr),
            AssignmentOperator::XorAssign => Some(BinaryOp::BitXor),
            AssignmentOperator::LeftShiftAssign => Some(BinaryOp::ShiftLeft),
            AssignmentOperator::RightShiftAssign => Some(BinaryOp::ShiftRight),
            AssignmentOperator::UnsignedRightShiftAssign => Some(BinaryOp::UnsignedShiftRight),
            AssignmentOperator::LogicalOrAssign => Some(BinaryOp::Or),
            AssignmentOperator::LogicalAndAssign => Some(BinaryOp::And),
            AssignmentOperator::NullCoalesceAssign => unreachable!(),
        };

        let rhs = self.lower_expr(&assign.right);

        // Compute the final value to store
        let value = if let Some(op) = binary_op {
            // Compound assignment: load current value, apply operation
            let current = self.lower_expr(&assign.left);
            let dest = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
            self.emit(IrInstr::BinaryOp {
                dest: dest.clone(),
                op,
                left: current,
                right: rhs,
            });
            dest
        } else {
            rhs
        };
        let callable_assign_hint = assign.operator == AssignmentOperator::Assign
            && self.expression_is_callable_hint(&assign.right);

        match &*assign.left {
            Expression::Identifier(ident) => {
                let mut assigned_local_idx: Option<u16> = None;
                let mut assigned_symbol = false;
                if let Some(&local_idx) = self.local_map.get(&ident.name) {
                    assigned_local_idx = Some(local_idx);
                    assigned_symbol = true;
                    // Check if this is a RefCell variable
                    if self.refcell_registers.contains_key(&local_idx) {
                        // Load the RefCell pointer
                        let refcell_ty = TypeId::new(NUMBER_TYPE_ID);
                        let refcell_reg = self.alloc_register(refcell_ty);
                        self.emit(IrInstr::LoadLocal {
                            dest: refcell_reg.clone(),
                            index: local_idx,
                        });
                        // Store the value to the RefCell
                        self.emit(IrInstr::StoreRefCell {
                            refcell: refcell_reg,
                            value: value.clone(),
                        });
                    } else {
                        self.emit(IrInstr::StoreLocal {
                            index: local_idx,
                            value: value.clone(),
                        });

                        // Update local register type on reassignment so subsequent
                        // member access dispatches correctly (e.g., neighbors = [] → array type)
                        if value.ty.as_u32() != UNRESOLVED_TYPE_ID {
                            if let Some(entry) = self.local_registers.get_mut(&local_idx) {
                                entry.ty = value.ty;
                            }
                        }

                        // Check for self-recursive closure: if we just assigned a closure
                        // that captured this variable, patch the closure's capture
                        if let Some((closure_reg, ref captures)) = self.last_closure_info.take() {
                            if let Some(&(_, capture_idx)) =
                                captures.iter().find(|(sym, _)| *sym == ident.name)
                            {
                                // This closure captured the variable we're assigning to
                                // Emit SetClosureCapture to patch the closure with itself
                                self.emit(IrInstr::SetClosureCapture {
                                    closure: closure_reg,
                                    index: capture_idx,
                                    value: value.clone(),
                                });
                            }
                        }
                    }
                } else if let Some(idx) = self.captures.iter().position(|c| c.symbol == ident.name)
                {
                    assigned_symbol = true;
                    // Variable is captured - handle assignment to captured variable
                    let is_refcell = self.captures[idx].is_refcell;
                    let capture_idx = self.captures[idx].capture_idx;

                    if is_refcell {
                        // Load the RefCell pointer from captured
                        let refcell_ty = TypeId::new(NUMBER_TYPE_ID);
                        let refcell_reg = self.alloc_register(refcell_ty);
                        self.emit(IrInstr::LoadCaptured {
                            dest: refcell_reg.clone(),
                            index: capture_idx,
                        });
                        // Store the value to the RefCell
                        self.emit(IrInstr::StoreRefCell {
                            refcell: refcell_reg,
                            value: value.clone(),
                        });
                    } else {
                        // Non-RefCell captured variable - use StoreCaptured
                        self.emit(IrInstr::StoreCaptured {
                            index: capture_idx,
                            value: value.clone(),
                        });
                    }
                } else if let Some(ref ancestors) = self.ancestor_variables.clone() {
                    // Variable not captured yet but exists in ancestor scope - add to captures
                    if let Some(ancestor_var) = ancestors.get(&ident.name) {
                        assigned_symbol = true;
                        let ty = ancestor_var.ty;
                        let is_refcell = ancestor_var.is_refcell;
                        let capture_idx = self.next_capture_slot;
                        self.next_capture_slot += 1;
                        self.captures.push(super::CaptureInfo {
                            symbol: ident.name,
                            source: ancestor_var.source,
                            capture_idx,
                            ty,
                            is_refcell,
                        });

                        if is_refcell {
                            // Load the RefCell pointer from captured
                            let refcell_ty = TypeId::new(NUMBER_TYPE_ID);
                            let refcell_reg = self.alloc_register(refcell_ty);
                            self.emit(IrInstr::LoadCaptured {
                                dest: refcell_reg.clone(),
                                index: capture_idx,
                            });
                            // Store the value to the RefCell
                            self.emit(IrInstr::StoreRefCell {
                                refcell: refcell_reg,
                                value: value.clone(),
                            });
                        } else {
                            // Non-RefCell captured variable - use StoreCaptured
                            self.emit(IrInstr::StoreCaptured {
                                index: capture_idx,
                                value: value.clone(),
                            });
                        }
                    } else if let Some(&global_idx) = self.module_var_globals.get(&ident.name) {
                        assigned_symbol = true;
                        // Module-level variable inside arrow — store via global slot
                        self.emit(IrInstr::StoreGlobal {
                            index: global_idx,
                            value: value.clone(),
                        });
                    } else if let Some(binding) = self
                        .current_method_env_globals
                        .as_ref()
                        .and_then(|m| m.get(&ident.name))
                        .copied()
                    {
                        assigned_symbol = true;
                        if binding.is_refcell {
                            let refcell_reg = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
                            self.emit(IrInstr::LoadGlobal {
                                dest: refcell_reg.clone(),
                                index: binding.global_idx,
                            });
                            self.emit(IrInstr::StoreRefCell {
                                refcell: refcell_reg,
                                value: value.clone(),
                            });
                        } else {
                            self.emit(IrInstr::StoreGlobal {
                                index: binding.global_idx,
                                value: value.clone(),
                            });
                        }
                    }
                } else if let Some(&global_idx) = self.module_var_globals.get(&ident.name) {
                    assigned_symbol = true;
                    // Module-level variable — store via global slot
                    self.emit(IrInstr::StoreGlobal {
                        index: global_idx,
                        value: value.clone(),
                    });
                } else if let Some(binding) = self
                    .current_method_env_globals
                    .as_ref()
                    .and_then(|m| m.get(&ident.name))
                    .copied()
                {
                    assigned_symbol = true;
                    if binding.is_refcell {
                        let refcell_reg = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
                        self.emit(IrInstr::LoadGlobal {
                            dest: refcell_reg.clone(),
                            index: binding.global_idx,
                        });
                        self.emit(IrInstr::StoreRefCell {
                            refcell: refcell_reg,
                            value: value.clone(),
                        });
                    } else {
                        self.emit(IrInstr::StoreGlobal {
                            index: binding.global_idx,
                            value: value.clone(),
                        });
                    }
                }

                // Keep callable-hint state in sync for identifier reassignments.
                if assigned_symbol {
                    if assign.operator == AssignmentOperator::Assign {
                        if callable_assign_hint {
                            self.callable_symbol_hints.insert(ident.name);
                        } else {
                            self.callable_symbol_hints.remove(&ident.name);
                            self.bound_method_vars.remove(&ident.name);
                        }
                        if let Some(local_idx) = assigned_local_idx {
                            if callable_assign_hint {
                                self.callable_local_hints.insert(local_idx);
                            } else {
                                self.callable_local_hints.remove(&local_idx);
                            }
                        }
                    } else {
                        // Compound assignments produce non-callable values.
                        self.callable_symbol_hints.remove(&ident.name);
                        self.bound_method_vars.remove(&ident.name);
                        if let Some(local_idx) = assigned_local_idx {
                            self.callable_local_hints.remove(&local_idx);
                        }
                    }
                }
            }
            Expression::Member(member) => {
                let prop_name = self.interner.resolve(member.property.name).to_string();

                // Check for static field write: ClassName.staticField = value
                if let Expression::Identifier(ident) = &*member.object {
                    if let Some(&nominal_type_id) = self.class_map.get(&ident.name) {
                        let global_index = self.class_info_map.get(&nominal_type_id).and_then(|info| {
                            info.static_fields
                                .iter()
                                .find(|f| self.interner.resolve(f.name) == prop_name)
                                .map(|sf| sf.global_index)
                        });
                        if let Some(index) = global_index {
                            self.emit(IrInstr::StoreGlobal {
                                index,
                                value: value.clone(),
                            });
                            return value;
                        }
                    }
                }

                // Instance/object field write
                let nominal_type_id = if self.prefers_structural_member_projection(&member.object) {
                    None
                } else {
                    self.infer_nominal_type_id(&member.object)
                };
                let object = self.lower_expr(&member.object);
                let checker_obj_ty = self.get_expr_type(&member.object);
                let allow_dynamic_any_write = self.type_is_dynamic_any_like(checker_obj_ty)
                    || self.type_is_dynamic_any_like(object.ty);
                let obj_ty_id = {
                    let reg_ty = object.ty.as_u32();
                    let checker_ty = self.get_expr_type(&member.object).as_u32();
                    let reg_dispatch = self.normalize_type_for_dispatch(reg_ty);
                    let checker_dispatch = self.normalize_type_for_dispatch(checker_ty);
                    if reg_dispatch != UNRESOLVED_TYPE_ID && reg_dispatch != UNKNOWN_TYPE_ID {
                        reg_dispatch
                    } else if checker_dispatch != UNRESOLVED_TYPE_ID
                        && checker_dispatch != UNKNOWN_TYPE_ID
                    {
                        checker_dispatch
                    } else {
                        UNRESOLVED_TYPE_ID
                    }
                };

                if let Some(nominal_type_id) = nominal_type_id {
                    if let Some(field) = self
                        .get_all_fields(nominal_type_id)
                        .iter()
                        .rev()
                        .find(|f| self.interner.resolve(f.name) == prop_name)
                    {
                        self.emit(IrInstr::StoreFieldExact {
                            object,
                            field: field.index,
                            value: value.clone(),
                        });
                    } else {
                        if allow_dynamic_any_write {
                            let prop_reg = self.alloc_register(TypeId::new(STRING_TYPE_ID));
                            self.emit(IrInstr::Assign {
                                dest: prop_reg.clone(),
                                value: IrValue::Constant(IrConstant::String(prop_name.clone())),
                            });
                            self.emit(IrInstr::NativeCall {
                                dest: None,
                                native_id: crate::compiler::native_id::REFLECT_SET,
                                args: vec![object, prop_reg, value.clone()],
                            });
                        } else {
                            let class_name = self
                                .class_map
                                .iter()
                                .find_map(|(&sym, &id)| {
                                    if id == nominal_type_id {
                                        Some(self.interner.resolve(sym).to_string())
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or_else(|| format!("class#{}", nominal_type_id.as_u32()));
                            self.errors
                                .push(crate::compiler::CompileError::InternalError {
                                    message: format!(
                                    "unresolved member assignment '{}.{}': class field not found",
                                    class_name, prop_name
                                ),
                                });
                        }
                    }
                } else {
                    let resolved_field_idx = match &*member.object {
                        Expression::Identifier(ident) => self
                            .variable_object_fields
                            .get(&ident.name)
                            .and_then(|fields| {
                                fields
                                    .iter()
                                    .find(|(name, _)| name == &prop_name)
                                    .map(|(_, idx)| *idx as u16)
                            }),
                        _ => self
                            .register_object_fields
                            .get(&object.id)
                            .and_then(|fields| {
                                fields
                                    .iter()
                                    .find(|(name, _)| name == &prop_name)
                                    .map(|(_, idx)| *idx as u16)
                            }),
                    };

                    if let Some(field_idx) = resolved_field_idx {
                        self.emit_member_store(
                            &member.object,
                            object,
                            field_idx,
                            &prop_name,
                            value.clone(),
                        );
                    } else if obj_ty_id == JSON_TYPE_ID || obj_ty_id == JSON_OBJECT_TYPE_ID {
                        self.emit(IrInstr::DynSetProp {
                            object,
                            property: prop_name.clone(),
                            value: value.clone(),
                        });
                    } else {
                        let prop_reg = self.alloc_register(TypeId::new(STRING_TYPE_ID));
                        self.emit(IrInstr::Assign {
                            dest: prop_reg.clone(),
                            value: IrValue::Constant(IrConstant::String(prop_name.clone())),
                        });
                        self.emit(IrInstr::NativeCall {
                            dest: None,
                            native_id: crate::compiler::native_id::REFLECT_SET,
                            args: vec![object, prop_reg, value.clone()],
                        });
                    }
                }
            }
            Expression::Index(index) => {
                let object = self.lower_expr(&index.object);
                if self.is_append_index_pattern(&index.object, &index.index) {
                    self.emit(IrInstr::ArrayPush {
                        array: object,
                        element: value.clone(),
                    });
                } else {
                    let idx = self.lower_expr(&index.index);
                    if self.index_uses_dynamic_keyed_access(self.get_expr_type(&index.object)) {
                        self.emit(IrInstr::DynSetKeyed {
                            object,
                            key: idx,
                            value: value.clone(),
                        });
                    } else {
                        self.emit(IrInstr::StoreElement {
                            array: object,
                            index: idx,
                            value: value.clone(),
                        });
                    }
                }
            }
            _ => {}
        }

        value
    }

    fn lower_conditional(&mut self, cond: &ast::ConditionalExpression) -> Register {
        // Lower using control flow
        let condition = self.lower_expr(&cond.test);

        let then_block = self.alloc_block();
        let else_block = self.alloc_block();
        let merge_block = self.alloc_block();

        self.set_terminator(crate::ir::Terminator::Branch {
            cond: condition,
            then_block,
            else_block,
        });

        // Then branch
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::new(then_block));
        self.current_block = then_block;
        let then_val = self.lower_expr(&cond.consequent);
        let then_result = then_val.clone();
        // Capture actual block after lowering (may differ if expression has control flow)
        let then_exit_block = self.current_block;
        self.set_terminator(crate::ir::Terminator::Jump(merge_block));

        // Else branch
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::new(else_block));
        self.current_block = else_block;
        let else_val = self.lower_expr(&cond.alternate);
        let else_result = else_val.clone();
        // Capture actual block after lowering (may differ if expression has control flow)
        let else_exit_block = self.current_block;
        self.set_terminator(crate::ir::Terminator::Jump(merge_block));

        // Merge block with phi
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::new(merge_block));
        self.current_block = merge_block;

        let dest = self.alloc_register(then_result.ty);
        self.emit(IrInstr::Phi {
            dest: dest.clone(),
            sources: vec![
                (then_exit_block, then_result),
                (else_exit_block, else_result),
            ],
        });
        self.propagate_type_projection_to_register(dest.ty, &dest);

        dest
    }

    pub(super) fn lower_arrow(&mut self, arrow: &ast::ArrowFunction) -> Register {
        self.lower_arrow_with_preassigned_id(arrow, None)
    }

    pub(super) fn lower_arrow_with_preassigned_id(
        &mut self,
        arrow: &ast::ArrowFunction,
        preassigned_func_id: Option<crate::ir::FunctionId>,
    ) -> Register {
        // Generate unique name for the arrow function
        let arrow_name = format!("__arrow_{}", self.arrow_counter);
        self.arrow_counter += 1;

        // Allocate or reuse function ID for the arrow.
        // Reuse is required for pre-registered nested std-wrapper helpers so
        // forward sibling calls target the exact emitted function.
        let func_id = if let Some(func_id) = preassigned_func_id {
            if func_id.as_u32() >= self.next_function_id {
                self.next_function_id = func_id.as_u32() + 1;
            }
            func_id
        } else {
            let func_id = crate::ir::FunctionId::new(self.next_function_id);
            self.next_function_id += 1;
            func_id
        };

        // Track async closures for SpawnClosure emission
        if arrow.is_async {
            self.async_closures.insert(func_id);
        }

        // Save current lowerer state
        let saved_register = self.next_register;
        let saved_block = self.next_block;
        let saved_local_map = self.local_map.clone();
        let saved_local_registers = self.local_registers.clone();
        let saved_callable_local_hints = self.callable_local_hints.clone();
        let saved_callable_symbol_hints = self.callable_symbol_hints.clone();
        let saved_bound_method_vars = self.bound_method_vars.clone();
        let saved_refcell_registers = self.refcell_registers.clone();
        let saved_next_local = self.next_local;
        let saved_function = self.current_function.take();
        let saved_current_block = self.current_block;
        let saved_ancestor_variables = self.ancestor_variables.take();
        let saved_captures = std::mem::take(&mut self.captures);
        let saved_next_capture_slot = self.next_capture_slot;
        let saved_this_register = self.this_register.take();
        let saved_pending_constructor_prologue = self.pending_constructor_prologue.take();
        let saved_this_ancestor_info = self.this_ancestor_info.take();
        let saved_this_captured_idx = self.this_captured_idx.take();
        // closure_locals maps local-slot indices to async func IDs; it is
        // per-scope, so it must be cleared on entry and restored on exit to
        // prevent stale entries from an outer (or sibling) function bleeding
        // into this function's local-slot numbering.
        let saved_closure_locals = std::mem::take(&mut self.closure_locals);

        // Build ancestor_variables for the child arrow:
        // 1. Current local_map becomes ImmediateParentLocal for the child
        // 2. Current ancestor_variables become Ancestor for the child
        let mut new_ancestor_vars = rustc_hash::FxHashMap::default();

        // Add current locals as immediate parent locals
        for (sym, &local_idx) in &saved_local_map {
            let ty = saved_local_registers
                .get(&local_idx)
                .map(|r| r.ty)
                .unwrap_or(TypeId::new(NUMBER_TYPE_ID));
            let is_refcell = saved_refcell_registers.contains_key(&local_idx);
            new_ancestor_vars.insert(
                *sym,
                super::AncestorVar {
                    source: super::AncestorSource::ImmediateParentLocal(local_idx),
                    ty,
                    is_refcell,
                },
            );
        }

        // Add existing ancestor variables (they stay as Ancestor for nested arrows)
        if let Some(ref existing) = saved_ancestor_variables {
            for (sym, var) in existing {
                // Don't override if already in locals (shadowing)
                if !new_ancestor_vars.contains_key(sym) {
                    new_ancestor_vars.insert(
                        *sym,
                        super::AncestorVar {
                            source: super::AncestorSource::Ancestor,
                            ty: var.ty,
                            is_refcell: var.is_refcell,
                        },
                    );
                }
            }
        }

        self.ancestor_variables = Some(new_ancestor_vars);
        self.captures.clear();
        self.next_capture_slot = 0;

        // Set up this_ancestor_info for the arrow function
        // If parent has this_register, `this` is at local slot 0 (implicit first param in methods)
        // If parent was also in an arrow with this_ancestor_info, propagate as Ancestor
        self.this_ancestor_info = if saved_this_register.is_some() {
            // Parent is a method - `this` is effectively at local 0
            Some(super::AncestorThisInfo {
                source: super::AncestorSource::ImmediateParentLocal(0),
            })
        } else if saved_this_ancestor_info.is_some() {
            // Parent is an arrow that had access to `this` - becomes Ancestor for us
            Some(super::AncestorThisInfo {
                source: super::AncestorSource::Ancestor,
            })
        } else {
            None
        };
        self.this_captured_idx = None;

        // Reset per-function state
        self.next_register = 0;
        self.next_block = 0;
        self.local_map.clear();
        self.local_registers.clear();
        self.callable_local_hints.clear();
        self.refcell_registers.clear();

        // Check if any parameters use destructuring
        let has_destructuring_params = arrow.params.iter().any(|p| {
            !matches!(
                p.pattern,
                ast::Pattern::Identifier(_) | ast::Pattern::Rest(_)
            )
        });

        // IMPORTANT: If there are destructuring parameters, start local allocation AFTER parameter slots
        if has_destructuring_params {
            let fixed_param_count = arrow.params.iter().filter(|p| !p.is_rest).count();
            self.next_local = fixed_param_count as u16;
        } else {
            self.next_local = 0;
        }

        // Create parameter registers (excluding rest parameters)
        let mut params = Vec::new();
        let mut rest_param_info = None;
        let mut fixed_param_count = 0;
        // Track parameters with destructuring patterns for later binding
        let mut destructure_params: Vec<(usize, &ast::Pattern, Register)> = Vec::new();

        for (decl_param_idx, param) in arrow.params.iter().enumerate() {
            // Skip rest parameters - they're handled separately
            if param.is_rest {
                // Extract rest parameter info for later processing
                let rest_ident = match &param.pattern {
                    ast::Pattern::Identifier(ident) => Some(ident.name),
                    ast::Pattern::Rest(rest) => match rest.argument.as_ref() {
                        ast::Pattern::Identifier(ident) => Some(ident.name),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(rest_name) = rest_ident {
                    let ty = param
                        .type_annotation
                        .as_ref()
                        .map(|t| self.resolve_type_annotation(t))
                        .unwrap_or(TypeId::new(crate::parser::TypeContext::ARRAY_TYPE_ID));
                    rest_param_info = Some((rest_name, ty));
                }
                continue;
            }

            fixed_param_count += 1;

            let ty = param
                .type_annotation
                .as_ref()
                .map(|t| self.resolve_type_annotation(t))
                .unwrap_or(TypeId::new(NUMBER_TYPE_ID));
            let reg = self.alloc_register(ty);

            if let ast::Pattern::Identifier(ident) = &param.pattern {
                let local_idx = self.allocate_local(ident.name);
                self.local_registers.insert(local_idx, reg.clone());
                // Shadowing is by symbol name; ensure arrow params override outer callable hints.
                self.callable_symbol_hints.remove(&ident.name);

                // Track class type for parameters with class type annotations
                // so method calls can be statically resolved
                if let Some(type_ann) = &param.type_annotation {
                    if let Some(nominal_type_id) = self.try_extract_class_from_type(type_ann) {
                        self.variable_class_map.insert(ident.name, nominal_type_id);
                    }
                    self.register_variable_type_hints_from_annotation(ident.name, type_ann);
                    if self.type_annotation_is_callable(type_ann) {
                        self.callable_local_hints.insert(local_idx);
                        self.callable_symbol_hints.insert(ident.name);
                    }
                }
            } else {
                // Destructuring pattern: track for later binding after entry block
                destructure_params.push((decl_param_idx, &param.pattern, reg.clone()));
            }
            params.push(reg);
        }

        // Get return type
        let return_ty = arrow
            .return_type
            .as_ref()
            .map(|t| self.resolve_type_annotation(t))
            .unwrap_or_else(|| TypeId::new(NUMBER_TYPE_ID));

        // Create the arrow function
        let mut ir_func = crate::ir::IrFunction::new(&arrow_name, params, return_ty);
        if self.emit_sourcemap {
            ir_func.source_span = arrow.span;
        }
        self.current_function = Some(ir_func);

        // Create entry block
        let entry_block = self.alloc_block();
        self.current_block = entry_block;
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(entry_block, "entry"));

        // Bind destructuring patterns in arrow parameters
        // This must happen after entry block is created so we can emit instructions
        for (param_idx, pattern, value_reg) in destructure_params {
            // Register object field layout for destructuring
            if let ast::Pattern::Object(_) = pattern {
                if let Some(type_ann) = arrow
                    .params
                    .get(param_idx)
                    .and_then(|p| p.type_annotation.as_ref())
                {
                    if let Some(field_layout) = self.extract_field_names_from_type(type_ann) {
                        self.register_object_fields
                            .insert(value_reg.id, field_layout);
                    }
                    if let Some(nested_array_layouts) =
                        self.extract_array_element_object_layouts_from_type(type_ann)
                    {
                        for (field_idx, layout) in nested_array_layouts {
                            self.register_nested_array_element_object_fields
                                .insert((value_reg.id, field_idx), layout);
                        }
                    }
                }
            }
            self.bind_pattern(pattern, value_reg);
        }

        // Emit rest array collection code if present
        if let Some((rest_name, rest_ty)) = rest_param_info {
            self.emit_rest_array_collection(rest_name, rest_ty, fixed_param_count);
        }

        // Emit null-check + default-value for parameters with defaults
        self.emit_default_params(&arrow.params);

        // Lower arrow body
        match &arrow.body {
            ast::ArrowBody::Expression(expr) => {
                let result = self.lower_expr(expr);
                self.set_terminator(crate::ir::Terminator::Return(Some(result)));
            }
            ast::ArrowBody::Block(block) => {
                for stmt in &block.statements {
                    self.lower_stmt(stmt);
                }
                // Ensure the function ends with a return
                if !self.current_block_is_terminated() {
                    self.set_terminator(crate::ir::Terminator::Return(None));
                }
            }
        }

        // Collect captures discovered during lowering
        let captured_vars: Vec<_> = self.captures.clone();
        // Propagate callable-hint invalidations for captured outer symbols.
        // Child scopes may clear callable hints after assigning non-callable
        // values to captured identifiers; reflect that in the parent.
        let mut propagate_callable_invalidations: FxHashSet<Symbol> = FxHashSet::default();
        for cap in &captured_vars {
            let sym = cap.symbol;
            if saved_callable_symbol_hints.contains(&sym)
                && !self.callable_symbol_hints.contains(&sym)
            {
                propagate_callable_invalidations.insert(sym);
            }
        }
        // Save the child's ancestor_variables for capture propagation
        let child_ancestor_variables = self.ancestor_variables.take();
        // Save child's this capture info
        let child_this_captured_idx = self.this_captured_idx;
        let child_this_ancestor_info = self.this_ancestor_info;

        // Take the completed arrow function and add to pending with its func_id
        let arrow_func = self.current_function.take().unwrap();
        self.pending_arrow_functions
            .push((func_id.as_u32(), arrow_func));

        // Restore saved state
        self.next_register = saved_register;
        self.next_block = saved_block;
        self.local_map = saved_local_map;
        self.local_registers = saved_local_registers;
        self.callable_local_hints = saved_callable_local_hints;
        self.callable_symbol_hints = saved_callable_symbol_hints;
        self.bound_method_vars = saved_bound_method_vars;
        self.refcell_registers = saved_refcell_registers;
        self.next_local = saved_next_local;
        self.current_function = saved_function;
        self.current_block = saved_current_block;
        self.ancestor_variables = saved_ancestor_variables;
        self.captures = saved_captures;
        self.next_capture_slot = saved_next_capture_slot;
        self.this_register = saved_this_register;
        self.pending_constructor_prologue = saved_pending_constructor_prologue;
        self.this_ancestor_info = saved_this_ancestor_info;
        self.this_captured_idx = saved_this_captured_idx;
        self.closure_locals = saved_closure_locals;

        for sym in propagate_callable_invalidations {
            self.callable_symbol_hints.remove(&sym);
            self.bound_method_vars.remove(&sym);
            if let Some(&local_idx) = self.local_map.get(&sym) {
                self.callable_local_hints.remove(&local_idx);
            }
        }

        // Load captured variables and build captures list for MakeClosure
        // `this` (if captured) is inserted at its assigned slot index
        let mut capture_regs = Vec::new();
        let this_reg_for_closure = if child_this_captured_idx.is_some() {
            let this_reg = self.alloc_register(TypeId::new(NUMBER_TYPE_ID)); // Object type

            // Check where `this` comes from
            if let Some(ref _parent_this) = self.this_register {
                // Parent is a method - load `this` from parent's register
                // In methods, `this` is passed as local slot 0
                self.emit(IrInstr::LoadLocal {
                    dest: this_reg.clone(),
                    index: 0,
                });
            } else if let Some(ref ancestor_info) = child_this_ancestor_info {
                // Parent was also an arrow that had access to `this`
                match ancestor_info.source {
                    super::AncestorSource::ImmediateParentLocal(local_idx) => {
                        self.emit(IrInstr::LoadLocal {
                            dest: this_reg.clone(),
                            index: local_idx,
                        });
                    }
                    super::AncestorSource::Ancestor => {
                        if let Some(parent_this_capture) = self.this_captured_idx {
                            self.emit(IrInstr::LoadCaptured {
                                dest: this_reg.clone(),
                                index: parent_this_capture,
                            });
                        } else {
                            self.emit(IrInstr::LoadLocal {
                                dest: this_reg.clone(),
                                index: 0,
                            });
                        }
                    }
                }
            }

            Some(this_reg)
        } else {
            None
        };
        for cap in &captured_vars {
            let ty = cap.ty;
            let cap_reg = self.alloc_register(ty);

            match cap.source {
                super::AncestorSource::ImmediateParentLocal(local_idx) => {
                    // Variable is in immediate parent's locals - load directly
                    self.emit(IrInstr::LoadLocal {
                        dest: cap_reg.clone(),
                        index: local_idx,
                    });
                }
                super::AncestorSource::Ancestor => {
                    // Variable is from a further ancestor
                    // First check if it's actually available as a local in current scope
                    if let Some(&local_idx) = self.local_map.get(&cap.symbol) {
                        // It's a local in current scope - load directly
                        self.emit(IrInstr::LoadLocal {
                            dest: cap_reg.clone(),
                            index: local_idx,
                        });
                    } else {
                        // Not in our locals - we must capture it too
                        // Check if parent already captured it
                        let parent_capture_idx = self
                            .captures
                            .iter()
                            .position(|c| c.symbol == cap.symbol)
                            .map(|i| i as u16);

                        let capture_idx = if let Some(idx) = parent_capture_idx {
                            idx
                        } else {
                            // Add to parent's captures (propagate up)
                            // Look up where the CURRENT (parent) function gets this variable from
                            // using the child's ancestor_variables (which describes the parent's sources)
                            let (source, is_refcell) = if let Some(ref ancestors) =
                                child_ancestor_variables
                            {
                                if let Some(ancestor_var) = ancestors.get(&cap.symbol) {
                                    (ancestor_var.source, ancestor_var.is_refcell)
                                } else {
                                    // Variable not in child's ancestors - should not happen
                                    // Fall back to loading from locals if available
                                    if let Some(&local_idx) = self.local_map.get(&cap.symbol) {
                                        (
                                            super::AncestorSource::ImmediateParentLocal(local_idx),
                                            cap.is_refcell,
                                        )
                                    } else {
                                        (super::AncestorSource::Ancestor, cap.is_refcell)
                                    }
                                }
                            } else {
                                // No child ancestors - check our own locals
                                if let Some(&local_idx) = self.local_map.get(&cap.symbol) {
                                    // Check if it's a RefCell
                                    let is_refcell =
                                        self.refcell_registers.contains_key(&local_idx);
                                    (
                                        super::AncestorSource::ImmediateParentLocal(local_idx),
                                        is_refcell,
                                    )
                                } else {
                                    (super::AncestorSource::Ancestor, cap.is_refcell)
                                }
                            };

                            let idx = self.captures.len() as u16;
                            self.captures.push(super::CaptureInfo {
                                symbol: cap.symbol,
                                source,
                                capture_idx: idx,
                                ty: cap.ty,
                                is_refcell,
                            });
                            // Keep lazy-capture index allocation monotonic.
                            // Without this, subsequent direct captures in the same function
                            // can reuse slot 0 and alias unrelated captured symbols.
                            self.next_capture_slot = self.next_capture_slot.max(idx + 1);
                            idx
                        };

                        self.emit(IrInstr::LoadCaptured {
                            dest: cap_reg.clone(),
                            index: capture_idx,
                        });
                    }
                }
            }
            capture_regs.push(cap_reg);
        }

        // Insert `this` at its assigned capture slot index
        if let Some(this_reg) = this_reg_for_closure {
            let idx = child_this_captured_idx.unwrap() as usize;
            capture_regs.insert(idx, this_reg);
        }

        // Create closure: emit MakeClosure instruction with captures
        let closure_ty = TypeId::new(NUMBER_TYPE_ID); // Generic function type
        let dest = self.alloc_register(closure_ty);
        self.emit(IrInstr::MakeClosure {
            dest: dest.clone(),
            func: func_id,
            captures: capture_regs,
        });

        // Store info about this closure for self-recursive detection
        let capture_info: Vec<_> = captured_vars
            .iter()
            .enumerate()
            .map(|(i, cap)| (cap.symbol, i as u16))
            .collect();
        self.last_closure_info = Some((dest.clone(), capture_info));
        self.last_arrow_func_id = Some(func_id);

        dest
    }

    fn lower_typeof(&mut self, typeof_expr: &ast::TypeofExpression) -> Register {
        // Preserve explicit int-literal behavior: `typeof 42` should be "int".
        // For non-literals, runtime TYPEOF handles value-based classification.
        if let Expression::IntLiteral(_) = &*typeof_expr.argument {
            let dest = self.alloc_register(TypeId::new(STRING_TYPE_ID));
            self.emit(IrInstr::Assign {
                dest: dest.clone(),
                value: IrValue::Constant(IrConstant::String("int".to_string())),
            });
            return dest;
        }

        let operand = self.lower_expr(&typeof_expr.argument);
        let dest = self.alloc_register(TypeId::new(STRING_TYPE_ID)); // String type (TypeId 1)

        self.emit(IrInstr::Typeof {
            dest: dest.clone(),
            operand,
        });
        dest
    }

    fn lower_new(&mut self, new_expr: &ast::NewExpression) -> Register {
        // Constructor results are object-like by default. Keep unresolved until a
        // concrete class/type path assigns a precise type.
        let dest = self.alloc_register(UNRESOLVED);

        if let Expression::Identifier(ident) = &*new_expr.callee {
            // Handle built-in primitive constructors
            let name = self.interner.resolve(ident.name);
            if name == TC::ARRAY_TYPE_NAME {
                // Lower `new Array(...)` directly to array IR so it compiles to bytecode
                // without relying on the legacy ARRAY_NEW native constructor path.
                let array_dest = self.alloc_register(TypeId::new(super::ARRAY_TYPE_ID));
                match new_expr.arguments.len() {
                    0 => {
                        let zero = self.emit_i32_const(0);
                        self.emit(IrInstr::NewArray {
                            dest: array_dest.clone(),
                            len: zero,
                            elem_ty: TypeId::new(NUMBER_TYPE_ID),
                        });
                    }
                    1 => match &new_expr.arguments[0] {
                        // JS-compatible pragmatic subset: numeric single arg = length.
                        Expression::IntLiteral(_) | Expression::FloatLiteral(_) => {
                            let len = self.lower_expr(&new_expr.arguments[0]);
                            self.emit(IrInstr::NewArray {
                                dest: array_dest.clone(),
                                len,
                                elem_ty: TypeId::new(NUMBER_TYPE_ID),
                            });
                        }
                        // Non-numeric single arg = array with one element.
                        _ => {
                            let zero = self.emit_i32_const(0);
                            self.emit(IrInstr::NewArray {
                                dest: array_dest.clone(),
                                len: zero,
                                elem_ty: TypeId::new(NUMBER_TYPE_ID),
                            });
                            let element = self.lower_expr(&new_expr.arguments[0]);
                            self.emit(IrInstr::ArrayPush {
                                array: array_dest.clone(),
                                element,
                            });
                        }
                    },
                    _ => {
                        // Two or more args become initial elements.
                        let zero = self.emit_i32_const(0);
                        self.emit(IrInstr::NewArray {
                            dest: array_dest.clone(),
                            len: zero,
                            elem_ty: TypeId::new(NUMBER_TYPE_ID),
                        });
                        for arg in &new_expr.arguments {
                            let element = self.lower_expr(arg);
                            self.emit(IrInstr::ArrayPush {
                                array: array_dest.clone(),
                                element,
                            });
                        }
                    }
                }
                return array_dest;
            }

            if name == TC::REGEXP_TYPE_NAME {
                // RegExp may not be declared as a local class in this module.
                // Fall back to direct native constructor only in that case.
                let has_regexp_class = self.class_map.contains_key(&ident.name)
                    || self.variable_class_map.contains_key(&ident.name);
                if !has_regexp_class {
                    let regexp_dest = self.alloc_register(TypeId::new(REGEXP_TYPE_ID));
                    let mut args = Vec::new();
                    for arg in &new_expr.arguments {
                        args.push(self.lower_expr(arg));
                    }
                    // If flags not provided, pass empty string
                    if args.len() == 1 {
                        let empty_flags = self.alloc_register(TypeId::new(STRING_TYPE_ID));
                        self.emit(IrInstr::Assign {
                            dest: empty_flags.clone(),
                            value: IrValue::Constant(IrConstant::String(String::new())),
                        });
                        args.push(empty_flags);
                    }
                    self.emit(IrInstr::NativeCall {
                        dest: Some(regexp_dest.clone()),
                        native_id: builtin_regexp::NEW,
                        args,
                    });
                    return regexp_dest;
                }
            }

            if name == TC::CHANNEL_TYPE_NAME {
                // When the Channel class definition is unavailable, lower directly to opcode IR.
                // If the Channel class exists in this module, use normal class
                // construction so methods dispatch through wrapper methods (`channelId` field).
                let has_channel_class = self.class_map.contains_key(&ident.name)
                    || self.variable_class_map.contains_key(&ident.name);
                if !has_channel_class {
                    let channel_dest = self.alloc_register(TypeId::new(CHANNEL_TYPE_ID));
                    let capacity = if let Some(first_arg) = new_expr.arguments.first() {
                        self.lower_expr(first_arg)
                    } else {
                        self.emit_i32_const(0)
                    };
                    self.emit(IrInstr::NewChannel {
                        dest: channel_dest.clone(),
                        capacity,
                    });
                    return channel_dest;
                }
            }

            if name == TC::MUTEX_TYPE_NAME {
                // Ambient builtin path: mutex values are handle-backed and constructed via opcode.
                // If a concrete Mutex class is present in this module, regular class
                // construction should take precedence.
                let has_mutex_class = self.class_map.contains_key(&ident.name)
                    || self.variable_class_map.contains_key(&ident.name);
                if !has_mutex_class {
                    let mutex_dest = self.alloc_register(TypeId::new(MUTEX_TYPE_ID));
                    self.emit(IrInstr::NewMutex {
                        dest: mutex_dest.clone(),
                    });
                    return mutex_dest;
                }
            }

            // Look up class ID from known class symbols or class-typed aliases.
            let nominal_type_id_opt = self
                .class_map
                .get(&ident.name)
                .copied()
                .or_else(|| self.variable_class_map.get(&ident.name).copied());
            if let Some(nominal_type_id) = nominal_type_id_opt {
                if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                    eprintln!("[lower] new {} -> nominal_type_id={}", name, nominal_type_id.as_u32());
                }
                // Create the object
                self.emit(IrInstr::NewType {
                    dest: dest.clone(),
                    nominal_type_id: nominal_type_id,
                });

                let constructor_func_id = self
                    .class_info_map
                    .get(&nominal_type_id)
                    .and_then(|info| info.constructor);

                if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                    eprintln!(
                        "[lower] new {} ctor_func_id={:?}",
                        name,
                        constructor_func_id.map(|id| id.as_u32())
                    );
                }

                // Call the constructor if one exists
                if let Some(ctor_func_id) = constructor_func_id {
                    // Get constructor parameter info for default values
                    let ctor_params = self
                        .class_info_map
                        .get(&nominal_type_id)
                        .map(|info| info.constructor_params.clone())
                        .unwrap_or_default();

                    // Lower constructor arguments
                    let mut args = Vec::new();
                    args.push(dest.clone()); // Pass 'this' as first argument

                    // Add provided arguments
                    for arg in &new_expr.arguments {
                        args.push(self.lower_expr(arg));
                    }

                    // Fill in default values for missing arguments
                    let provided_count = new_expr.arguments.len();
                    for (i, param_info) in ctor_params.iter().enumerate() {
                        if i >= provided_count {
                            if let Some(ref default_expr) = param_info.default_value {
                                args.push(self.lower_expr(default_expr));
                            }
                        }
                    }

                    self.emit(IrInstr::ConstructType {
                        dest: dest.clone(),
                        object: dest.clone(),
                        nominal_type_id,
                        args: args.into_iter().skip(1).collect(),
                    });
                    if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                        eprintln!(
                            "[lower] emitted construct_type nominal_type_id={} ctor_func_id={}",
                            nominal_type_id.as_u32(),
                            ctor_func_id.as_u32()
                        );
                    }
                }

                return dest;
            }

            let ctor_source_symbol = self
                .constructor_value_ctor_map
                .get(&ident.name)
                .copied()
                .unwrap_or(ident.name);
            let ctor_source_name = self.interner.resolve(ctor_source_symbol);
            let mut callee_ty = self.get_expr_type(&new_expr.callee);
            if callee_ty.as_u32() == UNRESOLVED_TYPE_ID {
                callee_ty = self
                    .type_ctx
                    .lookup_named_type(name)
                    .or_else(|| self.type_ctx.lookup_named_type(ctor_source_name))
                    .unwrap_or(callee_ty);
            }
            let runtime_bound_constructor = self.import_bindings.contains(&ident.name)
                || self.constructor_value_ctor_map.contains_key(&ident.name)
                || (self.ambient_builtin_globals.contains(name)
                    && !self.class_map.contains_key(&ident.name)
                    && !self.variable_class_map.contains_key(&ident.name)
                    && self.type_has_construct_signature(callee_ty));
            if runtime_bound_constructor {
                let return_ty = self
                    .first_construct_signature_return_type(callee_ty)
                    .unwrap_or(UNRESOLVED);
                let ctor_dest = self.alloc_register(return_ty);
                let nominal_type_id = self.lower_expr(&new_expr.callee);
                let mut native_args = Vec::with_capacity(new_expr.arguments.len() + 1);
                native_args.push(nominal_type_id);
                for arg in &new_expr.arguments {
                    native_args.push(self.lower_expr(arg));
                }
                self.emit(IrInstr::NativeCall {
                    dest: Some(ctor_dest.clone()),
                    native_id: crate::compiler::native_id::OBJECT_CONSTRUCT_DYNAMIC_CLASS,
                    args: native_args,
                });
                return ctor_dest;
            }

            // Builtin class constructors (Map/Set/Buffer/Date/etc.) are resolved via
            // native constructor IDs in the type registry when no runtime-bound class
            // value is available in the current module environment.
            if let Some(native_id) = self.type_registry.constructor_native_id(name) {
                let ctor_ty = self
                    .type_ctx
                    .lookup_named_type(name)
                    .unwrap_or(TypeId::new(UNRESOLVED_TYPE_ID));
                let ctor_dest = self.alloc_register(ctor_ty);
                let mut args = Vec::with_capacity(new_expr.arguments.len());
                for arg in &new_expr.arguments {
                    args.push(self.lower_expr(arg));
                }
                self.emit(IrInstr::NativeCall {
                    dest: Some(ctor_dest.clone()),
                    native_id,
                    args,
                });
                if name == "Object" {
                    self.register_object_fields.insert(
                        ctor_dest.id,
                        vec![
                            ("value".to_string(), 0),
                            ("writable".to_string(), 1),
                            ("configurable".to_string(), 2),
                            ("enumerable".to_string(), 3),
                            ("get".to_string(), 4),
                            ("set".to_string(), 5),
                        ],
                    );
                }
                return ctor_dest;
            }

            // Fallback for builtin error constructors in compilation modes where
            // builtin classes are type-known but not lowered as concrete class IR.
            if matches!(
                name,
                "Error"
                    | "TypeError"
                    | "RangeError"
                    | "ReferenceError"
                    | "SyntaxError"
                    | "URIError"
                    | "EvalError"
                    | "AggregateError"
                    | "ChannelClosedError"
                    | "AssertionError"
            ) {
                let is_aggregate_error = name == "AggregateError";

                let message_arg = if is_aggregate_error {
                    new_expr.arguments.get(1)
                } else {
                    new_expr.arguments.first()
                };
                let message = if let Some(message_arg) = message_arg {
                    self.lower_expr(message_arg)
                } else {
                    let reg = self.alloc_register(TypeId::new(STRING_TYPE_ID));
                    self.emit(IrInstr::Assign {
                        dest: reg.clone(),
                        value: IrValue::Constant(IrConstant::String(String::new())),
                    });
                    reg
                };

                let cause_arg = if is_aggregate_error {
                    new_expr.arguments.get(2)
                } else {
                    new_expr.arguments.get(1)
                };
                let cause_reg = if let Some(cause_arg) = cause_arg {
                    let options = self.lower_expr(cause_arg);
                    let extracted = self.alloc_register(TypeId::new(UNRESOLVED_TYPE_ID));
                    self.emit(IrInstr::DynGetProp {
                        dest: extracted.clone(),
                        object: options,
                        property: "cause".to_string(),
                    });
                    extracted
                } else {
                    self.lower_null_literal()
                };

                let name_reg = self.alloc_register(TypeId::new(STRING_TYPE_ID));
                self.emit(IrInstr::Assign {
                    dest: name_reg.clone(),
                    value: IrValue::Constant(IrConstant::String(name.to_string())),
                });
                let stack_reg = self.alloc_register(TypeId::new(STRING_TYPE_ID));
                self.emit(IrInstr::Assign {
                    dest: stack_reg.clone(),
                    value: IrValue::Constant(IrConstant::String(String::new())),
                });
                let code_reg = self.alloc_register(TypeId::new(STRING_TYPE_ID));
                self.emit(IrInstr::Assign {
                    dest: code_reg.clone(),
                    value: IrValue::Constant(IrConstant::String(String::new())),
                });
                let errno_reg = self.emit_i32_const(0);
                let syscall_reg = self.alloc_register(TypeId::new(STRING_TYPE_ID));
                self.emit(IrInstr::Assign {
                    dest: syscall_reg.clone(),
                    value: IrValue::Constant(IrConstant::String(String::new())),
                });
                let path_reg = self.alloc_register(TypeId::new(STRING_TYPE_ID));
                self.emit(IrInstr::Assign {
                    dest: path_reg.clone(),
                    value: IrValue::Constant(IrConstant::String(String::new())),
                });

                let mut fields = vec![
                    (0, message),
                    (1, name_reg),
                    (2, stack_reg),
                    (3, cause_reg),
                    (4, code_reg),
                    (5, errno_reg),
                    (6, syscall_reg),
                    (7, path_reg),
                ];

                if is_aggregate_error {
                    let errors_reg = if let Some(arg0) = new_expr.arguments.first() {
                        self.lower_expr(arg0)
                    } else {
                        let len = self.emit_i32_const(0);
                        let arr = self.alloc_register(TypeId::new(super::ARRAY_TYPE_ID));
                        self.emit(IrInstr::NewArray {
                            dest: arr.clone(),
                            len,
                            elem_ty: TypeId::new(UNRESOLVED_TYPE_ID),
                        });
                        arr
                    };
                    fields.push((8, errors_reg));
                }
                let mut ordered_names = vec![
                    "message".to_string(),
                    "name".to_string(),
                    "stack".to_string(),
                    "cause".to_string(),
                    "code".to_string(),
                    "errno".to_string(),
                    "syscall".to_string(),
                    "path".to_string(),
                ];
                if name == "AggregateError" {
                    ordered_names.push("errors".to_string());
                }

                let type_index = self.structural_layout_id_from_ordered_names(&ordered_names);
                self.emit(IrInstr::ObjectLiteral {
                    dest: dest.clone(),
                    type_index,
                    fields,
                });

                let mut named_fields = vec![
                    ("message".to_string(), 0),
                    ("name".to_string(), 1),
                    ("stack".to_string(), 2),
                    ("cause".to_string(), 3),
                    ("code".to_string(), 4),
                    ("errno".to_string(), 5),
                    ("syscall".to_string(), 6),
                    ("path".to_string(), 7),
                ];
                if name == "AggregateError" {
                    named_fields.push(("errors".to_string(), 8));
                }
                self.register_object_fields.insert(dest.id, named_fields);
                self.emit_structural_shape_name_registration_for_ordered_names(ordered_names);

                return dest;
            }
            if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                eprintln!(
                    "[lower] new {} ctor_source={} callee_ty={} has_construct_sig={} ambient_builtin={}",
                    name,
                    ctor_source_name,
                    callee_ty.as_u32(),
                    self.type_has_construct_signature(callee_ty),
                    self.ambient_builtin_globals.contains(name)
                );
            }
            if self.type_has_construct_signature(callee_ty) {
                let return_ty = self
                    .first_construct_signature_return_type(callee_ty)
                    .unwrap_or(UNRESOLVED);
                let ctor_dest = self.alloc_register(return_ty);
                let nominal_type_id = self.lower_expr(&new_expr.callee);
                let mut native_args = Vec::with_capacity(new_expr.arguments.len() + 1);
                native_args.push(nominal_type_id);
                for arg in &new_expr.arguments {
                    native_args.push(self.lower_expr(arg));
                }
                self.emit(IrInstr::NativeCall {
                    dest: Some(ctor_dest.clone()),
                    native_id: crate::compiler::native_id::OBJECT_CONSTRUCT_DYNAMIC_CLASS,
                    args: native_args,
                });
                return ctor_dest;
            }
        }

        let ctor_name = if let Expression::Identifier(ident) = &*new_expr.callee {
            self.interner.resolve(ident.name).to_string()
        } else {
            "<dynamic>".to_string()
        };
        self.errors
            .push(crate::compiler::CompileError::InternalError {
                message: format!(
                    "unresolved constructor target in `new` expression: {}",
                    ctor_name
                ),
            });
        self.lower_unresolved_poison()
    }

    fn lower_await(
        &mut self,
        await_expr: &ast::AwaitExpression,
        await_node: &Expression,
    ) -> Register {
        use crate::parser::types::ty::Type;

        #[derive(Clone, Copy, PartialEq, Eq)]
        enum Awaitability {
            None,
            Maybe,
            Definite,
        }

        fn merge_awaitability(current: Awaitability, next: Awaitability) -> Awaitability {
            match (current, next) {
                (Awaitability::Definite, Awaitability::Definite) => Awaitability::Definite,
                (Awaitability::None, Awaitability::None) => Awaitability::None,
                (Awaitability::Maybe, _) | (_, Awaitability::Maybe) => Awaitability::Maybe,
                _ => Awaitability::Maybe,
            }
        }

        fn value_awaitability(type_ctx: &TC, ty: TypeId) -> Awaitability {
            match type_ctx.get(ty) {
                Some(Type::Task(_)) => Awaitability::Definite,
                Some(Type::Class(class)) if class.name == "Promise" => Awaitability::Definite,
                Some(Type::Union(union)) => union
                    .members
                    .iter()
                    .copied()
                    .fold(Awaitability::None, |acc, member| {
                        merge_awaitability(acc, value_awaitability(type_ctx, member))
                    }),
                Some(Type::TypeVar(tv)) => {
                    tv.constraint.map_or(Awaitability::Maybe, |constraint| {
                        value_awaitability(type_ctx, constraint)
                    })
                }
                Some(Type::Any) | Some(Type::Unknown) | Some(Type::JSObject) => Awaitability::Maybe,
                None => Awaitability::Maybe,
                _ => Awaitability::None,
            }
        }

        fn array_awaitability(type_ctx: &TC, ty: TypeId) -> Option<Awaitability> {
            match type_ctx.get(ty) {
                Some(Type::Array(arr)) => Some(value_awaitability(type_ctx, arr.element)),
                Some(Type::Tuple(tuple)) => Some(
                    tuple
                        .elements
                        .iter()
                        .copied()
                        .fold(Awaitability::None, |acc, elem| {
                            merge_awaitability(acc, value_awaitability(type_ctx, elem))
                        }),
                ),
                _ => None,
            }
        }

        // Lower the awaited expression first; decisions below are based on checker + lowered type.
        let awaited_value = self.lower_expr(&await_expr.argument);
        let checker_arg_ty = self.get_expr_type(&await_expr.argument);
        let lowered_arg_ty = awaited_value.ty;

        let checker_array_awaitability = array_awaitability(self.type_ctx, checker_arg_ty);
        let lowered_array_awaitability = array_awaitability(self.type_ctx, lowered_arg_ty);

        // Parallel await is an extension; only use it when element type is definitely awaitable.
        if checker_array_awaitability == Some(Awaitability::Definite)
            || lowered_array_awaitability == Some(Awaitability::Definite)
        {
            let result_ty = self.get_expr_type(await_node);
            let dest = self.alloc_register(if result_ty == UNRESOLVED {
                TypeId::new(super::ARRAY_TYPE_ID)
            } else {
                result_ty
            });
            self.emit(IrInstr::AwaitAll {
                dest: dest.clone(),
                tasks: awaited_value,
            });
            self.propagate_type_projection_to_register(dest.ty, &dest);
            return dest;
        }

        // Non-task arrays resolve immediately with no runtime WaitAll path.
        if checker_array_awaitability.is_some() || lowered_array_awaitability.is_some() {
            return awaited_value;
        }

        let checker_awaitability = value_awaitability(self.type_ctx, checker_arg_ty);
        let lowered_awaitability = value_awaitability(self.type_ctx, lowered_arg_ty);
        let should_emit_await = checker_awaitability != Awaitability::None
            || lowered_awaitability != Awaitability::None;

        // JS-compatible await semantics: non-awaitables resolve immediately.
        if !should_emit_await {
            return awaited_value;
        }

        let result_ty = self.get_expr_type(await_node);
        let dest = self.alloc_register(if result_ty == UNRESOLVED {
            if let Some(Type::Task(task_ty)) = self.type_ctx.get(checker_arg_ty) {
                task_ty.result
            } else if let Some(Type::Task(task_ty)) = self.type_ctx.get(lowered_arg_ty) {
                task_ty.result
            } else {
                UNRESOLVED
            }
        } else {
            result_ty
        });

        self.emit(IrInstr::Await {
            dest: dest.clone(),
            task: awaited_value,
        });
        self.propagate_type_projection_to_register(dest.ty, &dest);
        dest
    }

    fn lower_async_call(&mut self, async_call: &ast::AsyncCallExpression) -> Register {
        // Lower arguments first
        let args: Vec<Register> = async_call
            .arguments
            .iter()
            .map(|a| self.lower_expr(a))
            .collect();

        // Destination for the Task handle - use proper Task type
        let task_ty = self
            .type_ctx
            .generic_task_type()
            .unwrap_or(TypeId::new(TASK_TYPE_ID));
        let dest = self.alloc_register(task_ty);

        // Handle different callee types
        if let Expression::Identifier(ident) = &*async_call.callee {
            // Direct function call: async myFn()
            if let Some(&func_id) = self.function_map.get(&ident.name) {
                self.emit(IrInstr::Spawn {
                    dest: dest.clone(),
                    func: func_id,
                    args,
                });
                return dest;
            }

            // Closure call: async closureVar()
            if let Some(&local_idx) = self.local_map.get(&ident.name) {
                let closure_ty = if let Some(reg) = self.local_registers.get(&local_idx) {
                    reg.ty
                } else {
                    self.errors
                        .push(crate::compiler::CompileError::InternalError {
                            message: format!(
                                    "internal error: missing local register metadata for async callable '{}'",
                                    self.interner.resolve(ident.name)
                            ),
                        });
                    self.poison_register(&dest);
                    return dest;
                };
                let callee_ty = self.get_expr_type(&async_call.callee);
                let callee_ty_raw = callee_ty.as_u32();
                let known_callable = self.closure_locals.contains_key(&local_idx)
                    || self.callable_local_hints.contains(&local_idx)
                    || self.callable_symbol_hints.contains(&ident.name);
                if !known_callable && !self.type_is_callable(callee_ty) {
                    self.errors
                        .push(crate::compiler::CompileError::InternalError {
                            message: format!(
                                "unresolved async call target '{}': local value is not callable (type id {})",
                                self.interner.resolve(ident.name),
                                callee_ty_raw
                            ),
                        });
                    self.poison_register(&dest);
                    return dest;
                }
                let closure = self.alloc_register(closure_ty);
                self.emit(IrInstr::LoadLocal {
                    dest: closure.clone(),
                    index: local_idx,
                });

                if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                    eprintln!(
                        "[lower] SpawnClosure[lower_async_call-local] '{}' local_idx={}",
                        self.interner.resolve(ident.name),
                        local_idx
                    );
                }
                self.emit(IrInstr::SpawnClosure {
                    dest: dest.clone(),
                    closure,
                    args,
                });
                return dest;
            }
        }

        // Handle member access: async obj.method()
        if let Expression::Member(member) = &*async_call.callee {
            let method_name_symbol = member.property.name;

            // Check if it's a static method call
            if let Expression::Identifier(ident) = &*member.object {
                if let Some(&nominal_type_id) = self.class_map.get(&ident.name) {
                    if let Some(&func_id) =
                        self.static_method_map.get(&(nominal_type_id, method_name_symbol))
                    {
                        // Spawn static method
                        self.emit(IrInstr::Spawn {
                            dest: dest.clone(),
                            func: func_id,
                            args,
                        });
                        return dest;
                    }
                }
            }

            // Instance method - find the method and spawn with 'this'
            let nominal_type_id = self.infer_nominal_type_id(&member.object);
            if let Some(nominal_type_id) = nominal_type_id {
                if let Some(func_id) = self.find_method(nominal_type_id, method_name_symbol) {
                    let object = self.lower_expr(&member.object);
                    let mut method_args = vec![object];
                    method_args.extend(args);
                    self.emit(IrInstr::Spawn {
                        dest: dest.clone(),
                        func: func_id,
                        args: method_args,
                    });
                    return dest;
                }
            }

            // Dynamic/late-bound member fallback:
            // lower member access and spawn the resulting callable.
            let closure = self.lower_member(member);
            self.emit(IrInstr::SpawnClosure {
                dest: dest.clone(),
                closure,
                args,
            });
            return dest;
        }

        // Non-member async callee fallback: closure/expression path only when callable.
        let callee_ty = self.get_expr_type(&async_call.callee);
        let callee_ty_raw = callee_ty.as_u32();
        if std::env::var("RAYA_DEBUG_CALL_FALLBACK").is_ok() {
            let callee_desc = match &*async_call.callee {
                Expression::Identifier(id) => {
                    format!("Identifier({})", self.interner.resolve(id.name))
                }
                Expression::Member(member) => {
                    format!("Member(.{})", self.interner.resolve(member.property.name))
                }
                Expression::Call(_) => "Call(...)".to_string(),
                Expression::TypeCast(_) => "TypeCast(...)".to_string(),
                Expression::Parenthesized(_) => "Parenthesized(...)".to_string(),
                _ => format!("{:?}", &*async_call.callee),
            };
            eprintln!(
                "[lower][async-call-fallback] callee={} ty={} unresolved={} unknown={}",
                callee_desc,
                callee_ty_raw,
                callee_ty_raw == UNRESOLVED_TYPE_ID,
                callee_ty_raw == UNKNOWN_TYPE_ID
            );
        }
        if !self.type_is_callable(callee_ty) {
            self.errors
                .push(crate::compiler::CompileError::InternalError {
                    message: format!(
                    "unresolved async call target: callee expression is not callable (type id {})",
                    callee_ty.as_u32()
                ),
                });
            self.poison_register(&dest);
            return dest;
        }

        let callee_reg = self.lower_expr(&async_call.callee);
        if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
            eprintln!(
                "[lower] SpawnClosure[lower_async_call-generic] callee_ty={}",
                callee_ty.as_u32()
            );
        }
        self.emit(IrInstr::SpawnClosure {
            dest: dest.clone(),
            closure: callee_reg,
            args,
        });
        dest
    }

    fn lower_this(&mut self) -> Register {
        // Return the 'this' register if we're directly inside a method
        if let Some(ref this_reg) = self.this_register {
            return this_reg.clone();
        }

        // Check if we've already captured `this`
        if let Some(capture_idx) = self.this_captured_idx {
            let dest = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
            self.emit(IrInstr::LoadCaptured {
                dest: dest.clone(),
                index: capture_idx,
            });
            return dest;
        }

        // Check if `this` is available from ancestor scope (we're inside an arrow in a method)
        if let Some(ref _ancestor_info) = self.this_ancestor_info {
            // Record that we need to capture `this` - claim next available slot
            let idx = self.next_capture_slot;
            self.next_capture_slot += 1;
            self.this_captured_idx = Some(idx);

            let dest = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
            self.emit(IrInstr::LoadCaptured {
                dest: dest.clone(),
                index: idx,
            });
            return dest;
        }

        // `this` outside class/arrow-method context is unresolved.
        self.errors
            .push(crate::compiler::CompileError::InternalError {
                message: "unresolved 'this' outside method/arrow-method context".to_string(),
            });
        self.lower_unresolved_poison()
    }

    fn lower_super(&mut self) -> Register {
        // 'super' refers to the same object as 'this', just with parent class semantics
        // The actual parent method dispatch is handled in lower_call
        self.lower_this()
    }

    /// Lower instanceof expression: expr instanceof ClassName
    fn lower_instanceof(&mut self, instanceof: &ast::InstanceOfExpression) -> Register {
        // Lower the object expression
        let object = self.lower_expr(&instanceof.object);
        let dest = self.alloc_register(TypeId::new(BOOLEAN_TYPE_ID));

        let mut target_ty = self.resolve_structural_slot_type_from_annotation(&instanceof.type_name);
        if target_ty == UNRESOLVED {
            if let ast::Type::Reference(type_ref) = &instanceof.type_name.ty {
                let name = self.interner.resolve(type_ref.name.name);
                if let Some(named) = self.type_ctx.lookup_named_type(name) {
                    target_ty = named;
                }
            }
        }

        if let Some(nominal_type_id) = self.resolve_class_from_type(&instanceof.type_name) {
            self.emit(IrInstr::IsNominal {
                dest: dest.clone(),
                object,
                nominal_type_id,
            });
            return dest;
        }

        let structural_layout = self
            .structural_slot_layout_from_type(target_ty)
            .or_else(|| {
                self.try_extract_object_alias_name_from_type(&instanceof.type_name)
                    .and_then(|alias_name| self.projected_structural_layout_from_alias_name(&alias_name))
            });
        if let Some(layout) = structural_layout {
            let shape_id = crate::vm::object::shape_id_from_member_names(
                &layout.iter().map(|(name, _)| name.clone()).collect::<Vec<_>>(),
            );
            self.emit_structural_shape_name_registration_for_ordered_names(
                layout.into_iter().map(|(name, _)| name).collect(),
            );
            self.emit(IrInstr::ImplementsShape {
                dest: dest.clone(),
                object,
                shape_id,
            });
            return dest;
        }

        if let ast::Type::Reference(type_ref) = &instanceof.type_name.ty {
            let runtime_name = self.interner.resolve(type_ref.name.name);
            let runtime_bound_class = self.import_bindings.contains(&type_ref.name.name)
                || (self.ambient_builtin_globals.contains(runtime_name)
                    && !self.class_map.contains_key(&type_ref.name.name)
                    && !self.variable_class_map.contains_key(&type_ref.name.name));
            if runtime_bound_class {
                let class_value = self.lower_identifier(&type_ref.name);
                self.emit(IrInstr::NativeCall {
                    dest: Some(dest.clone()),
                    native_id: crate::compiler::native_id::OBJECT_INSTANCE_OF_DYNAMIC_CLASS,
                    args: vec![object, class_value],
                });
                return dest;
            }
        }

        self.errors
            .push(crate::compiler::CompileError::InternalError {
                message: "unresolved nominal or structural target in `instanceof` type annotation"
                    .to_string(),
            });
        self.emit(IrInstr::Assign {
            dest: dest.clone(),
            value: IrValue::Constant(IrConstant::Boolean(false)),
        });
        dest
    }

    /// Lower type cast expression: expr as TypeName
    fn lower_type_cast(&mut self, cast: &ast::TypeCastExpression) -> Register {
        let mut target_ty = self.resolve_type_annotation(&cast.target_type);
        if target_ty == UNRESOLVED {
            if let ast::Type::Reference(type_ref) = &cast.target_type.ty {
                let name = self.interner.resolve(type_ref.name.name);
                if let Some(named) = self.type_ctx.lookup_named_type(name) {
                    target_ty = named;
                }
            }
        }

        // Lower the object expression. For object literals, use target-type
        // structural layout as contextual slot space.
        let prev_layout = self.object_literal_target_layout.clone();
        if matches!(&*cast.object, Expression::Object(_)) {
            self.object_literal_target_layout = self
                .structural_slot_layout_from_type(target_ty)
                .map(|layout| {
                    let mut names: Vec<String> = layout.into_iter().map(|(name, _)| name).collect();
                    names.sort_unstable();
                    names.dedup();
                    names
                });
        }
        let object = self.lower_expr(&cast.object);
        self.object_literal_target_layout = prev_layout;
        let object_id = object.id;

        // Allocate register for the casted value using the projected static target type
        // when available so downstream member lowering sees the cast surface.
        let dest_ty = if target_ty.as_u32() == UNRESOLVED_TYPE_ID {
            TypeId::new(UNKNOWN_TYPE_ID)
        } else {
            target_ty
        };
        let dest = self.alloc_register(dest_ty);

        // Runtime cast checks are currently supported for class-reference targets.
        // Non-class targets use compile-time cast typing only.
        if let Some(nominal_type_id) = self.resolve_class_from_type(&cast.target_type) {
            if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                match &*cast.object {
                    Expression::Member(member) => {
                        if let Expression::Identifier(obj_ident) = &*member.object {
                            eprintln!(
                                "[lower] cast member '{}.{}' -> nominal_type_id={}",
                                self.interner.resolve(obj_ident.name),
                                self.interner.resolve(member.property.name),
                                nominal_type_id.as_u32()
                            );
                        } else {
                            eprintln!(
                                "[lower] cast <member expr> -> nominal_type_id={}",
                                nominal_type_id.as_u32()
                            );
                        }
                    }
                    Expression::Identifier(ident) => eprintln!(
                        "[lower] cast ident '{}' -> nominal_type_id={}",
                        self.interner.resolve(ident.name),
                        nominal_type_id.as_u32()
                    ),
                    _ => eprintln!(
                        "[lower] cast expr -> nominal_type_id={}",
                        nominal_type_id.as_u32()
                    ),
                }
            }
            self.emit(IrInstr::CastNominal {
                dest: dest.clone(),
                object,
                nominal_type_id,
            });
        } else if let Some(nominal_type_id) =
            self.resolve_nullable_class_from_union(&cast.target_type)
        {
            self.emit(IrInstr::CastNominal {
                dest: dest.clone(),
                object,
                nominal_type_id,
            });
        } else if let Some(shape_id) = self.structural_shape_id_from_type(target_ty) {
            if let Some(layout) = self.structural_slot_layout_from_type(target_ty) {
                let names = layout
                    .into_iter()
                    .map(|(name, _)| name)
                    .collect::<Vec<_>>();
                self.emit_structural_shape_name_registration_for_ordered_names(names);
            }
            self.emit(IrInstr::CastShape {
                dest: dest.clone(),
                object,
                shape_id,
            });
        } else if let Some(tuple_len_encoded) =
            self.resolve_runtime_tuple_len_cast_target(&cast.target_type)
        {
            self.emit(IrInstr::CastTupleLen {
                dest: dest.clone(),
                object,
                expected_len: tuple_len_encoded & 0x3FFF,
            });
        } else if let Some(object_min_fields_encoded) =
            self.resolve_runtime_object_min_fields_cast_target(&cast.target_type)
        {
            self.emit(IrInstr::CastObjectMinFields {
                dest: dest.clone(),
                object,
                required_fields: object_min_fields_encoded & 0x1FFF,
            });
        } else if let Some(array_elem_kind_encoded) =
            self.resolve_runtime_array_element_kind_cast_target(&cast.target_type)
        {
            self.emit(IrInstr::CastArrayElemKind {
                dest: dest.clone(),
                object,
                expected_elem_mask: array_elem_kind_encoded & 0x00FF,
            });
        } else if let Some(kind_mask) = self.resolve_runtime_cast_kind_mask(&cast.target_type) {
            self.emit(IrInstr::CastKindMask {
                dest: dest.clone(),
                object,
                expected_kind_mask: kind_mask,
            });
        } else {
            self.emit(IrInstr::Assign {
                dest: dest.clone(),
                value: IrValue::Register(object),
            });
        }

        // Preserve inferred object/array slot layouts across cast boundaries so
        // subsequent member lowering can keep using slot-based access.
        if let Some(fields) = self.register_object_fields.get(&object_id).cloned() {
            self.register_object_fields.insert(dest.id, fields);
        }
        if let Some(fields) = self
            .register_array_element_object_fields
            .get(&object_id)
            .cloned()
        {
            self.register_array_element_object_fields
                .insert(dest.id, fields);
        }
        let nested_object_layouts: Vec<(u16, Vec<(String, usize)>)> = self
            .register_nested_object_fields
            .iter()
            .filter_map(|(&(obj_reg, field_idx), layout)| {
                (obj_reg == object_id).then_some((field_idx, layout.clone()))
            })
            .collect();
        for (field_idx, layout) in nested_object_layouts {
            self.register_nested_object_fields
                .insert((dest.id, field_idx), layout);
        }
        let nested_array_layouts: Vec<(u16, Vec<(String, usize)>)> = self
            .register_nested_array_element_object_fields
            .iter()
            .filter_map(|(&(obj_reg, field_idx), layout)| {
                (obj_reg == object_id).then_some((field_idx, layout.clone()))
            })
            .collect();
        for (field_idx, layout) in nested_array_layouts {
            self.register_nested_array_element_object_fields
                .insert((dest.id, field_idx), layout);
        }
        if let Some(layout) = self.structural_projection_layout_from_type_id(target_ty) {
            self.register_structural_projection_fields
                .insert(dest.id, layout);
        } else if let ast::Type::Reference(type_ref) = &cast.target_type.ty {
            let alias_name = self.interner.resolve(type_ref.name.name);
            if let Some(layout) = self
                .projected_structural_layout_from_alias_name(alias_name)
                .map(|layout| {
                    layout
                        .into_iter()
                        .map(|(field_name, field_idx)| (field_name, field_idx as usize))
                        .collect::<Vec<_>>()
                })
            {
                self.register_structural_projection_fields
                    .insert(dest.id, layout);
            }
        }

        // If lowering already propagated a concrete field layout for this value,
        // slot-based member access can stay local without runtime remap registration.
        // This avoids overriding concrete object-literal slots with metadata-based
        // remaps when the source value is structurally known.
        if let Some(layout) = self.register_structural_projection_fields.get(&dest.id).cloned() {
            let mut names = layout.into_iter().map(|(name, _)| name).collect::<Vec<_>>();
            names.sort_unstable();
            names.dedup();
            self.emit_structural_shape_name_registration_for_ordered_names(names);
        } else if !self.register_object_fields.contains_key(&dest.id) {
            self.emit_structural_slot_registration_for_type(dest.clone(), target_ty);
        }

        dest
    }

    /// Resolve a class runtime cast target from a type annotation.
    /// Returns None for non-class targets (primitive/union/object/etc).
    fn resolve_class_from_type(&self, type_ann: &ast::TypeAnnotation) -> Option<NominalTypeId> {
        use crate::parser::ast::types::Type;

        match &type_ann.ty {
            Type::Reference(type_ref) => {
                let type_name = self.interner.resolve(type_ref.name.name);
                // Internal linker aliases (`__t_*`) model structural instance shapes.
                // Treat casts to these aliases as compile-time only (no runtime class cast),
                // otherwise exported class constructor values trigger invalid runtime Cast.
                if type_name.starts_with("__t_") {
                    None
                } else {
                    self.nominal_type_id_from_type_name(type_name)
                }
            }
            _ => None,
        }
    }

    fn resolve_runtime_cast_kind_mask(&self, type_ann: &ast::TypeAnnotation) -> Option<u16> {
        use crate::parser::ast::types::{PrimitiveType, Type};
        match &type_ann.ty {
            Type::Primitive(prim) => match prim {
                PrimitiveType::Null => Some(CAST_KIND_NULL),
                PrimitiveType::Boolean => Some(CAST_KIND_BOOL),
                PrimitiveType::Int => Some(CAST_KIND_INT),
                PrimitiveType::Number => Some(CAST_KIND_INT | CAST_KIND_NUMBER),
                PrimitiveType::String => Some(CAST_KIND_STRING),
                PrimitiveType::Void => None,
            },
            Type::Array(_) | Type::Tuple(_) => Some(CAST_KIND_ARRAY),
            Type::Object(_) => Some(CAST_KIND_OBJECT),
            Type::Function(_) => Some(CAST_KIND_FUNCTION),
            Type::StringLiteral(_) => Some(CAST_KIND_STRING),
            Type::NumberLiteral(n) => {
                if n.fract() == 0.0 {
                    Some(CAST_KIND_INT)
                } else {
                    Some(CAST_KIND_NUMBER)
                }
            }
            Type::BooleanLiteral(_) => Some(CAST_KIND_BOOL),
            Type::Parenthesized(inner) => self.resolve_runtime_cast_kind_mask(inner),
            Type::Union(union) => {
                let mut combined = 0u16;
                for ty in &union.types {
                    let mask = self.resolve_runtime_cast_kind_mask(ty)?;
                    combined |= mask;
                }
                if combined == 0 {
                    None
                } else {
                    Some(combined)
                }
            }
            // For class references/intersections/indexed access/keyof/typeof we currently
            // use either class-id casts or compile-time-only casts.
            _ => None,
        }
    }

    fn resolve_runtime_tuple_len_cast_target(&self, type_ann: &ast::TypeAnnotation) -> Option<u16> {
        use crate::parser::ast::types::Type;
        let tuple = match &type_ann.ty {
            Type::Tuple(t) => t,
            Type::Parenthesized(inner) => return self.resolve_runtime_tuple_len_cast_target(inner),
            _ => return None,
        };
        let len = u16::try_from(tuple.element_types.len()).ok()?;
        if len > 0x3FFF {
            return None;
        }
        Some(CAST_KIND_MASK_FLAG | CAST_TUPLE_LEN_FLAG | len)
    }

    fn resolve_runtime_object_min_fields_cast_target(
        &self,
        type_ann: &ast::TypeAnnotation,
    ) -> Option<u16> {
        use crate::parser::ast::types::{ObjectTypeMember, Type};
        let object = match &type_ann.ty {
            Type::Object(o) => o,
            Type::Parenthesized(inner) => {
                return self.resolve_runtime_object_min_fields_cast_target(inner);
            }
            _ => return None,
        };

        let required_props = object
            .members
            .iter()
            .filter(|m| {
                matches!(
                    m,
                    ObjectTypeMember::Property(p) if !p.optional
                )
            })
            .count();
        let required_props = u16::try_from(required_props).ok()?;
        if required_props > 0x1FFF {
            return None;
        }
        Some(CAST_KIND_MASK_FLAG | CAST_OBJECT_MIN_FIELDS_FLAG | required_props)
    }

    fn resolve_runtime_array_element_kind_cast_target(
        &self,
        type_ann: &ast::TypeAnnotation,
    ) -> Option<u16> {
        use crate::parser::ast::types::Type;
        let array = match &type_ann.ty {
            Type::Array(a) => a,
            Type::Parenthesized(inner) => {
                return self.resolve_runtime_array_element_kind_cast_target(inner);
            }
            _ => return None,
        };
        let elem_mask = self.resolve_runtime_cast_kind_mask(&array.element_type)?;
        Some(CAST_KIND_MASK_FLAG | CAST_ARRAY_ELEM_KIND_FLAG | elem_mask)
    }

    fn resolve_nullable_class_from_union(&self, type_ann: &ast::TypeAnnotation) -> Option<NominalTypeId> {
        use crate::parser::ast::types::{PrimitiveType, Type};
        let Type::Union(union) = &type_ann.ty else {
            return None;
        };
        if union.types.len() != 2 {
            return None;
        }
        let first = &union.types[0];
        let second = &union.types[1];
        let (class_candidate, null_candidate) = match (&first.ty, &second.ty) {
            (Type::Reference(_), Type::Primitive(PrimitiveType::Null)) => (first, second),
            (Type::Primitive(PrimitiveType::Null), Type::Reference(_)) => (second, first),
            _ => return None,
        };
        if !matches!(null_candidate.ty, Type::Primitive(PrimitiveType::Null)) {
            return None;
        }
        self.resolve_class_from_type(class_candidate)
    }

    /// Find a method in a class or its parent classes.
    /// Returns the function ID if found.
    fn find_method(&self, nominal_type_id: NominalTypeId, method_name: Symbol) -> Option<FunctionId> {
        // First check this class
        if let Some(&func_id) = self.method_map.get(&(nominal_type_id, method_name)) {
            return Some(func_id);
        }

        // Check parent class recursively
        if let Some(parent_id) = self
            .class_info_map
            .get(&nominal_type_id)
            .and_then(|info| info.parent_class)
        {
            return self.find_method(parent_id, method_name);
        }

        None
    }

    /// Specialize a generic function for a call with concrete type arguments.
    /// Creates a new specialized copy of the function with type params substituted.
    fn specialize_generic_function(
        &mut self,
        func_name: Symbol,
        call: &ast::CallExpression,
    ) -> Option<FunctionId> {
        let type_args = call.type_args.as_ref()?;
        let func_ast = self.generic_function_asts.get(&func_name)?.clone();
        let type_params = func_ast.type_params.as_ref()?;

        if type_args.len() != type_params.len() {
            return None;
        }

        // Build substitution map: param_name → concrete TypeId
        let mut substitutions = FxHashMap::default();
        let mut mangled_parts = Vec::new();
        for (param, arg) in type_params.iter().zip(type_args.iter()) {
            let param_name = self.interner.resolve(param.name.name).to_string();
            let concrete_ty = self.resolve_type_annotation(arg);
            substitutions.insert(param_name.clone(), concrete_ty);
            mangled_parts.push(format!("{}", concrete_ty.as_u32()));
        }

        // Check cache
        let original_name = self.interner.resolve(func_name).to_string();
        let mangled_name = format!("{}${}", original_name, mangled_parts.join("_"));
        if let Some(&cached_id) = self.specialized_function_cache.get(&mangled_name) {
            return Some(cached_id);
        }

        // Allocate a new function ID
        let specialized_id = FunctionId::new(self.next_function_id);
        self.next_function_id += 1;

        // Preserve async marker from original function
        if func_ast.is_async {
            self.async_functions.insert(specialized_id);
        }

        // Cache before lowering (prevents infinite recursion for recursive generics)
        self.specialized_function_cache
            .insert(mangled_name.clone(), specialized_id);

        // Save per-function lowering state (we're interrupting another function's lowering)
        let saved_substitutions =
            std::mem::replace(&mut self.type_param_substitutions, substitutions);
        let saved_current_function = self.current_function.take();
        let saved_current_block = self.current_block;
        let saved_next_register = self.next_register;
        let saved_next_block = self.next_block;
        let saved_local_map = std::mem::take(&mut self.local_map);
        let saved_local_registers = std::mem::take(&mut self.local_registers);
        let saved_next_local = self.next_local;
        let saved_refcell_vars = std::mem::take(&mut self.refcell_vars);
        let saved_refcell_registers = std::mem::take(&mut self.refcell_registers);
        let saved_loop_captured_vars = std::mem::take(&mut self.loop_captured_vars);

        // Lower the specialized function
        let mut ir_func = self.lower_function(&func_ast);
        ir_func.name = mangled_name;

        // Restore per-function lowering state
        self.type_param_substitutions = saved_substitutions;
        self.current_function = saved_current_function;
        self.current_block = saved_current_block;
        self.next_register = saved_next_register;
        self.next_block = saved_next_block;
        self.local_map = saved_local_map;
        self.local_registers = saved_local_registers;
        self.next_local = saved_next_local;
        self.refcell_vars = saved_refcell_vars;
        self.refcell_registers = saved_refcell_registers;
        self.loop_captured_vars = saved_loop_captured_vars;

        // Add to pending functions
        self.pending_arrow_functions
            .push((specialized_id.as_u32(), ir_func));

        Some(specialized_id)
    }

    /// Get all fields for a class, including inherited fields from parent classes.
    /// Returns fields in order: parent fields first, then child fields.
    /// Set the return register type for Map/Set method calls based on generic type args.
    fn propagate_container_return_type(
        &self,
        dest: &mut Register,
        nominal_type_id: NominalTypeId,
        method_name: &str,
        object_expr: &Expression,
    ) {
        // Check if class is Map or Set
        let class_name = self
            .class_map
            .iter()
            .find(|(&_sym, &id)| id == nominal_type_id)
            .map(|(&sym, _)| self.interner.resolve(sym).to_string());
        let class_name = match class_name {
            Some(name) if name == TC::MAP_TYPE_NAME || name == TC::SET_TYPE_NAME => name,
            _ => return,
        };

        let value_type = match self.get_container_value_type(object_expr) {
            Some(vt) => vt,
            None => return,
        };

        match class_name.as_str() {
            "Map" => match method_name {
                "get" => {
                    dest.ty = value_type;
                }
                "keys" | "values" | "entries" => {
                    dest.ty = TypeId::new(super::ARRAY_TYPE_ID);
                }
                _ => {}
            },
            "Set" => match method_name {
                "keys" | "values" | "entries" => {
                    dest.ty = TypeId::new(super::ARRAY_TYPE_ID);
                }
                "has" => {
                    dest.ty = TypeId::new(BOOLEAN_TYPE_ID);
                } // boolean
                _ => {}
            },
            _ => {}
        }
    }

    /// Resolve a property type from a TypeVar's constraint.
    /// For `x: T extends { length: number }`, resolves `x.length` → number TypeId.
    fn resolve_typevar_property_type(
        &self,
        object_expr: &Expression,
        prop_name: &str,
    ) -> Option<TypeId> {
        let expr_ty = self.get_expr_type(object_expr);
        let ty = self.type_ctx.get(expr_ty)?;
        let constraint_id = match ty {
            crate::parser::types::ty::Type::TypeVar(tv) => tv.constraint?,
            _ => {
                // Try register type
                let obj_reg_ty = match object_expr {
                    Expression::Identifier(ident) => self
                        .local_map
                        .get(&ident.name)
                        .and_then(|&idx| self.local_registers.get(&idx))
                        .map(|r| r.ty),
                    _ => None,
                };
                let reg_ty = obj_reg_ty?;
                match self.type_ctx.get(reg_ty)? {
                    crate::parser::types::ty::Type::TypeVar(tv) => tv.constraint?,
                    _ => return None,
                }
            }
        };
        // Look up the property in the constraint type (which should be an object type)
        let constraint_ty = self.type_ctx.get(constraint_id)?;
        match constraint_ty {
            crate::parser::types::ty::Type::Object(obj) => obj
                .properties
                .iter()
                .find(|p| p.name == prop_name)
                .map(|p| p.ty),
            _ => None,
        }
    }

    fn resolve_member_field_metadata(
        &self,
        expr: &Expression,
    ) -> Option<(TypeId, Option<NominalTypeId>)> {
        let Expression::Member(member) = expr else {
            return None;
        };
        let owner_nominal_type_id = self.infer_nominal_type_id(&member.object)?;
        let field_name = self.interner.resolve(member.property.name);
        self.get_all_fields(owner_nominal_type_id)
            .into_iter()
            .rev()
            .find_map(|field| {
                (self.interner.resolve(field.name) == field_name).then(|| {
                    let nominal_type_id = field
                        .class_type
                        .or_else(|| {
                            field
                                .type_name
                                .as_deref()
                                .and_then(|name| self.nominal_type_id_from_type_name(name))
                        })
                        .or_else(|| self.nominal_type_id_from_type_id(field.ty));
                    (field.ty, nominal_type_id)
                })
            })
    }

    /// Look up the generic value type for a Map/Set field expression.
    /// For `this.adj` where `adj: Map<K, V>`, returns V's TypeId.
    fn get_container_value_type(&self, expr: &Expression) -> Option<TypeId> {
        if let Expression::Member(member) = expr {
            let obj_nominal_type_id = self.infer_nominal_type_id(&member.object)?;
            let field_name = self.interner.resolve(member.property.name);
            let all_fields = self.get_all_fields(obj_nominal_type_id);
            for field in all_fields.into_iter().rev() {
                if self.interner.resolve(field.name) == field_name {
                    return field.value_type;
                }
            }
        }
        None
    }

    pub(super) fn type_is_callable(&self, ty_id: TypeId) -> bool {
        use crate::parser::types::ty::Type;

        match self.type_ctx.get(ty_id) {
            Some(Type::Function(_)) => true,
            Some(Type::Object(obj)) => !obj.call_signatures.is_empty(),
            Some(Type::Interface(iface)) => !iface.call_signatures.is_empty(),
            Some(Type::Reference(type_ref)) => self
                .type_ctx
                .lookup_named_type(&type_ref.name)
                .is_some_and(|named| self.type_is_callable(named)),
            Some(Type::TypeVar(tv)) => tv
                .constraint
                .is_some_and(|constraint| self.type_is_callable(constraint)),
            Some(Type::Union(union)) => union
                .members
                .iter()
                .copied()
                .any(|member| self.type_is_callable(member)),
            Some(Type::Generic(generic)) => self.type_is_callable(generic.base),
            _ => false,
        }
    }

    pub(crate) fn type_has_construct_signature(&self, ty_id: TypeId) -> bool {
        use crate::parser::types::ty::Type;

        match self.type_ctx.get(ty_id) {
            Some(Type::Class(class_ty)) => !class_ty.is_abstract,
            Some(Type::Object(obj)) => !obj.construct_signatures.is_empty(),
            Some(Type::Interface(iface)) => !iface.construct_signatures.is_empty(),
            Some(Type::Reference(type_ref)) => self
                .type_ctx
                .lookup_named_type(&type_ref.name)
                .is_some_and(|named| self.type_has_construct_signature(named)),
            Some(Type::TypeVar(tv)) => tv
                .constraint
                .is_some_and(|constraint| self.type_has_construct_signature(constraint)),
            Some(Type::Union(union)) => union
                .members
                .iter()
                .copied()
                .any(|member| self.type_has_construct_signature(member)),
            Some(Type::Generic(generic)) => self.type_has_construct_signature(generic.base),
            _ => false,
        }
    }

    fn first_construct_signature_return_type(&self, ty_id: TypeId) -> Option<TypeId> {
        use crate::parser::types::ty::Type;

        match self.type_ctx.get(ty_id) {
            Some(Type::Class(_)) => Some(ty_id),
            Some(Type::Function(func)) => Some(func.return_type),
            Some(Type::Object(obj)) => {
                obj.construct_signatures.iter().find_map(|sig_ty| {
                    match self.type_ctx.get(*sig_ty) {
                        Some(Type::Function(func)) => Some(func.return_type),
                        _ => None,
                    }
                })
            }
            Some(Type::Interface(iface)) => iface.construct_signatures.iter().find_map(|sig_ty| {
                match self.type_ctx.get(*sig_ty) {
                    Some(Type::Function(func)) => Some(func.return_type),
                    _ => None,
                }
            }),
            Some(Type::Reference(type_ref)) => self
                .type_ctx
                .lookup_named_type(&type_ref.name)
                .and_then(|named| self.first_construct_signature_return_type(named)),
            Some(Type::TypeVar(tv)) => tv
                .constraint
                .and_then(|constraint| self.first_construct_signature_return_type(constraint)),
            Some(Type::Union(union)) => union
                .members
                .iter()
                .copied()
                .find_map(|member| self.first_construct_signature_return_type(member)),
            Some(Type::Generic(generic)) => {
                self.first_construct_signature_return_type(generic.base)
            }
            _ => None,
        }
    }

    fn type_is_dynamic_any_like(&self, ty_id: TypeId) -> bool {
        use crate::parser::types::ty::Type;

        self.type_ctx
            .get(ty_id)
            .is_some_and(|ty| matches!(ty, Type::Any | Type::Unknown))
    }

    fn is_structural_object_type(&self, ty_id: TypeId) -> bool {
        use crate::parser::types::ty::Type;

        match self.type_ctx.get(ty_id) {
            Some(Type::Object(_) | Type::Interface(_)) => true,
            Some(Type::Union(union)) => union
                .members
                .iter()
                .copied()
                .any(|member| self.is_structural_object_type(member)),
            Some(Type::TypeVar(tv)) => tv
                .constraint
                .is_some_and(|constraint| self.is_structural_object_type(constraint)),
            _ => false,
        }
    }

    /// When a child extends a generic parent (e.g., `extends Base<string>`),
    /// parent field types are substituted with concrete type arguments.
    pub(super) fn get_all_fields(&self, nominal_type_id: NominalTypeId) -> Vec<ClassFieldInfo> {
        let mut all_fields = Vec::new();

        if let Some(class_info) = self.class_info_map.get(&nominal_type_id) {
            if let Some(parent_id) = class_info.parent_class {
                let mut parent_fields = self.get_all_fields(parent_id);
                // Apply type substitutions for generic parent classes
                // Uses field.type_name (original type annotation name) since
                // the lowerer maps unknown type refs to TypeId(7), not TypeVar
                if let Some(ref subs) = class_info.extends_type_subs {
                    for field in &mut parent_fields {
                        if let Some(ref name) = field.type_name {
                            if let Some(&concrete_ty) = subs.get(name.as_str()) {
                                field.ty = concrete_ty;
                            }
                        }
                    }
                }
                for field in &mut parent_fields {
                    if field.ty == UNRESOLVED {
                        if let Some(ref type_name) = field.type_name {
                            if let Some(resolved_ty) = self.type_ctx.lookup_named_type(type_name) {
                                field.ty = resolved_ty;
                            }
                        }
                    }
                    if field.class_type.is_none() {
                        if let Some(ref type_name) = field.type_name {
                            field.class_type = self.nominal_type_id_from_type_name(type_name);
                        }
                    }
                }
                all_fields.extend(parent_fields);
            }
            // Then add this class's fields
            let mut class_fields = class_info.fields.clone();
            for field in &mut class_fields {
                if field.ty == UNRESOLVED {
                    if let Some(ref type_name) = field.type_name {
                        if let Some(resolved_ty) = self.type_ctx.lookup_named_type(type_name) {
                            field.ty = resolved_ty;
                        }
                    }
                }
                if field.class_type.is_none() {
                    if let Some(ref type_name) = field.type_name {
                        field.class_type = self.nominal_type_id_from_type_name(type_name);
                    }
                }
            }
            all_fields.extend(class_fields);
        }

        all_fields
    }

    /// Infer the class ID of an expression (for method call resolution)
    pub(super) fn infer_nominal_type_id(&self, expr: &Expression) -> Option<NominalTypeId> {
        match expr {
            // 'this' uses current class context
            Expression::This(_) => {
                let resolved = self.current_class.or_else(|| {
                    self.this_register
                        .as_ref()
                        .and_then(|reg| self.nominal_type_id_from_type_id(reg.ty))
                });
                if resolved.is_none() && std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                    let this_ty = self
                        .this_register
                        .as_ref()
                        .map(|reg| reg.ty.as_u32())
                        .unwrap_or(u32::MAX);
                    eprintln!(
                        "[lower] infer_nominal_type_id(this)=None current_class={:?} this_ty={}",
                        self.current_class.map(|id| id.as_u32()),
                        this_ty
                    );
                }
                resolved
            }
            // Variable lookup
            Expression::Identifier(ident) => {
                if self.interner.resolve(ident.name) == "this" {
                    return self.current_class.or_else(|| {
                        self.this_register
                            .as_ref()
                            .and_then(|reg| self.nominal_type_id_from_type_id(reg.ty))
                    });
                }
                if self
                    .variable_structural_projection_fields
                    .contains_key(&ident.name)
                {
                    return None;
                }
                // Prefer explicit variable/class maps, then local register typing.
                // Local register type is often more precise than checker fallback
                // (e.g. primitive parameters should not degrade to Object class).
                if let Some(nominal_type_id) = self.variable_class_map.get(&ident.name).copied() {
                    return Some(nominal_type_id);
                }
                if let Some(nominal_type_id) =
                    self.nominal_type_id_from_type_name(self.interner.resolve(ident.name))
                {
                    return Some(nominal_type_id);
                }
                if let Some(nominal_type_id) = self
                    .variable_object_type_aliases
                    .get(&ident.name)
                    .and_then(|alias| self.nominal_type_id_from_type_name(alias))
                {
                    return Some(nominal_type_id);
                }
                if let Some(&local_idx) = self.local_map.get(&ident.name) {
                    if let Some(local_reg) = self.local_registers.get(&local_idx) {
                        return self.nominal_type_id_from_type_id(local_reg.ty);
                    }
                }
                self.nominal_type_id_from_type_id(self.get_expr_type(expr))
            }
            Expression::TypeCast(cast) => {
                if self
                    .structural_projection_layout_from_type_id(
                        self.resolve_structural_slot_type_from_annotation(&cast.target_type),
                    )
                    .is_some()
                {
                    None
                } else {
                    self.try_extract_class_from_type(&cast.target_type)
                        .or_else(|| self.infer_nominal_type_id(&cast.object))
                        .or_else(|| self.nominal_type_id_from_type_id(self.get_expr_type(expr)))
                }
            }
            Expression::Conditional(cond) => {
                let then_class = self.infer_nominal_type_id(&cond.consequent);
                let else_class = self.infer_nominal_type_id(&cond.alternate);
                match (then_class, else_class) {
                    (Some(a), Some(b)) if a == b => Some(a),
                    (Some(a), None) => Some(a),
                    (None, Some(b)) => Some(b),
                    _ => self.nominal_type_id_from_type_id(self.get_expr_type(expr)),
                }
            }
            Expression::Parenthesized(inner) => self.infer_nominal_type_id(&inner.expression),
            // Field access: look up the field's type in the class definition
            Expression::Member(member) => {
                // Get the class of the object
                let obj_nominal_type_id = self.infer_nominal_type_id(&member.object)?;
                // Look up the field type
                let field_name = self.interner.resolve(member.property.name);
                let all_fields = self.get_all_fields(obj_nominal_type_id);
                for field in all_fields.into_iter().rev() {
                    let fname = self.interner.resolve(field.name);
                    if fname == field_name {
                        if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                            eprintln!(
                                "[lower] infer member '{}.{}' field_ty={} class_type={:?} type_name={:?}",
                                "<member>",
                                field_name,
                                field.ty.as_u32(),
                                field.class_type.map(|id| id.as_u32()),
                                field.type_name,
                            );
                        }
                        // Check if the field has a known class type
                        if let Some(field_nominal_type_id) = field.class_type {
                            return Some(field_nominal_type_id);
                        }
                        // Generic and substituted field types often have the correct
                        // concrete nominal type even when `class_type`/`type_name`
                        // were not preserved during lowering metadata construction.
                        if let Some(field_nominal_type_id) =
                            self.nominal_type_id_from_type_id(field.ty)
                        {
                            return Some(field_nominal_type_id);
                        }
                        // Otherwise, check if we have a type name we can look up
                        if let Some(ref type_name) = field.type_name {
                            if let Some(cid) = self.nominal_type_id_from_type_name(type_name) {
                                return Some(cid);
                            }
                        }
                        break;
                    }
                }
                self.nominal_type_id_from_type_id(self.get_expr_type(expr))
            }
            // Method/function call: check if the call has a known return class type
            Expression::Call(call) => {
                if let Expression::Member(member) = &*call.callee {
                    let obj_nominal_type_id = self.infer_nominal_type_id(&member.object)?;
                    let method_name = member.property.name;
                    // Only return a class if there's an explicit return class mapping.
                    // Don't assume methods return the same class (e.g., Map.get() returns
                    // the value type, not Map).
                    if let Some(&ret_nominal_type_id) = self
                        .method_return_class_map
                        .get(&(obj_nominal_type_id, method_name))
                    {
                        return Some(ret_nominal_type_id);
                    }
                    if let Some(ret_class_name) = self
                        .method_return_type_alias_map
                        .get(&(obj_nominal_type_id, method_name))
                    {
                        if let Some(ret_nominal_type_id) = self.nominal_type_id_from_type_name(ret_class_name) {
                            return Some(ret_nominal_type_id);
                        }
                    }
                }
                // Standalone function/bound method call: check return class
                if let Expression::Identifier(ident) = &*call.callee {
                    // Check __NATIVE_CALL<Type> type arguments
                    let name = self.interner.resolve(ident.name);
                    if name == "__NATIVE_CALL" {
                        if let Some(type_args) = &call.type_args {
                            if let Some(first_ty) = type_args.first() {
                                return self.try_extract_class_from_type(first_ty);
                            }
                        }
                    }
                    // Check if callee is a bound method variable
                    if let Some(&(nominal_type_id, method_name)) = self.bound_method_vars.get(&ident.name)
                    {
                        if let Some(&ret_nominal_type_id) =
                            self.method_return_class_map.get(&(nominal_type_id, method_name))
                        {
                            return Some(ret_nominal_type_id);
                        }
                    }
                    // Check function return class
                    if let Some(&ret_nominal_type_id) = self.function_return_class_map.get(&ident.name) {
                        return Some(ret_nominal_type_id);
                    }
                }
                self.nominal_type_id_from_type_id(self.get_expr_type(expr))
            }
            // New expression: return the class being instantiated
            Expression::New(new_expr) => {
                if let Expression::Identifier(ident) = &*new_expr.callee {
                    return self
                        .class_map
                        .get(&ident.name)
                        .copied()
                        .or_else(|| self.variable_class_map.get(&ident.name).copied())
                        .or_else(|| {
                            self.nominal_type_id_from_type_name(self.interner.resolve(ident.name))
                        });
                }
                self.nominal_type_id_from_type_id(self.get_expr_type(expr))
            }
            // Index access over array/tuple containers can preserve the element class.
            // Important: do NOT return container class ID directly (e.g. Array), because
            // that misroutes primitive element calls to object/vtable dispatch.
            Expression::Index(index) => {
                let object_ty = self.get_expr_type(&index.object);
                if let Some(ty) = self.type_ctx.get(object_ty) {
                    match ty {
                        crate::parser::types::ty::Type::Array(arr) => {
                            return self.nominal_type_id_from_type_id(arr.element);
                        }
                        crate::parser::types::ty::Type::Tuple(tuple) => {
                            // Conservative: if all tuple members that are class-typed agree
                            // on one class, use it; otherwise keep unresolved.
                            let mut found: Option<NominalTypeId> = None;
                            for member_ty in &tuple.elements {
                                if let Some(cid) = self.nominal_type_id_from_type_id(*member_ty) {
                                    match found {
                                        None => found = Some(cid),
                                        Some(existing) if existing == cid => {}
                                        Some(_) => {
                                            found = None;
                                            break;
                                        }
                                    }
                                }
                            }
                            if found.is_some() {
                                return found;
                            }
                            return None;
                        }
                        _ => {
                            // Non-array indexed containers (or custom indexable classes):
                            // fall back to the container class inference behavior.
                            return self.infer_nominal_type_id(&index.object);
                        }
                    }
                }
                self.infer_nominal_type_id(&index.object)
            }
            _ => self.nominal_type_id_from_type_id(self.get_expr_type(expr)),
        }
    }

    fn type_id_is_promise_like(&self, ty_id: TypeId) -> bool {
        use crate::parser::types::ty::Type;

        if ty_id.as_u32() == UNRESOLVED_TYPE_ID {
            return false;
        }

        match self.type_ctx.get(ty_id) {
            Some(Type::Task(_)) => true,
            Some(Type::Class(class_ty)) if class_ty.name == "Promise" => true,
            Some(Type::Reference(type_ref)) => {
                if type_ref.name == "Promise" {
                    return true;
                }
                self.type_ctx
                    .lookup_named_type(&type_ref.name)
                    .is_some_and(|named| self.type_id_is_promise_like(named))
            }
            Some(Type::TypeVar(tv)) => tv
                .constraint
                .is_some_and(|constraint| self.type_id_is_promise_like(constraint)),
            Some(Type::Union(union)) => union
                .members
                .iter()
                .copied()
                .any(|member| self.type_id_is_promise_like(member)),
            Some(Type::Generic(generic)) => self.type_id_is_promise_like(generic.base),
            _ => false,
        }
    }

    fn type_id_is_async_callable(&self, ty_id: TypeId) -> bool {
        use crate::parser::types::ty::Type;

        if ty_id.as_u32() == UNRESOLVED_TYPE_ID {
            return false;
        }

        match self.type_ctx.get(ty_id) {
            Some(Type::Function(func)) => {
                func.is_async || self.type_id_is_promise_like(func.return_type)
            }
            Some(Type::Object(obj)) => obj
                .call_signatures
                .iter()
                .copied()
                .any(|sig| self.type_id_is_async_callable(sig)),
            Some(Type::Interface(iface)) => iface
                .call_signatures
                .iter()
                .copied()
                .any(|sig| self.type_id_is_async_callable(sig)),
            Some(Type::Reference(type_ref)) => self
                .type_ctx
                .lookup_named_type(&type_ref.name)
                .is_some_and(|named| self.type_id_is_async_callable(named)),
            Some(Type::TypeVar(tv)) => tv
                .constraint
                .is_some_and(|constraint| self.type_id_is_async_callable(constraint)),
            Some(Type::Union(union)) => union
                .members
                .iter()
                .copied()
                .any(|member| self.type_id_is_async_callable(member)),
            Some(Type::Generic(generic)) => self.type_id_is_async_callable(generic.base),
            _ => false,
        }
    }

    fn class_type_has_async_method(&self, ty_id: TypeId, method_name: &str) -> bool {
        use crate::parser::types::ty::Type;

        match self.type_ctx.get(ty_id) {
            Some(Type::Class(class_ty)) => class_ty.methods.iter().any(|method| {
                method.name == method_name && self.type_id_is_async_callable(method.ty)
            }),
            Some(Type::Reference(type_ref)) => self
                .type_ctx
                .lookup_named_type(&type_ref.name)
                .is_some_and(|named| self.class_type_has_async_method(named, method_name)),
            Some(Type::TypeVar(tv)) => tv.constraint.is_some_and(|constraint| {
                self.class_type_has_async_method(constraint, method_name)
            }),
            Some(Type::Union(union)) => union
                .members
                .iter()
                .copied()
                .any(|member| self.class_type_has_async_method(member, method_name)),
            Some(Type::Generic(generic)) => {
                self.class_type_has_async_method(generic.base, method_name)
            }
            _ => false,
        }
    }

    fn late_bound_member_call_is_async(
        &self,
        receiver: &Expression,
        callee: &Expression,
        method_name: &str,
    ) -> bool {
        let callee_ty = self.get_expr_type(callee);
        if self.type_id_is_async_callable(callee_ty) {
            return true;
        }

        let Expression::Identifier(receiver_ident) = receiver else {
            return false;
        };

        if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
            eprintln!(
                "[lower] late-bound async check '{}.{}' callee_ty={}",
                self.interner.resolve(receiver_ident.name),
                method_name,
                callee_ty.as_u32()
            );
        }

        if let Some(&receiver_ty) = self.late_bound_object_type_map.get(&receiver_ident.name) {
            if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                eprintln!(
                    "[lower] late-bound async check via ctor ty id {}",
                    receiver_ty.as_u32()
                );
            }
            if self.class_type_has_async_method(receiver_ty, method_name) {
                return true;
            }
        }

        if let Some(&ctor_symbol) = self.late_bound_object_ctor_map.get(&receiver_ident.name) {
            let ctor_name = self.interner.resolve(ctor_symbol);
            if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                eprintln!(
                    "[lower] late-bound async check via ctor name '{}'",
                    ctor_name
                );
            }
            if let Some(ctor_ty) = self.type_ctx.lookup_named_type(ctor_name) {
                if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                    eprintln!(
                        "[lower] late-bound async ctor '{}' resolved ty id {}",
                        ctor_name,
                        ctor_ty.as_u32()
                    );
                }
                if self.class_type_has_async_method(ctor_ty, method_name) {
                    return true;
                }
            }
        }

        false
    }

    pub(super) fn nominal_type_id_from_type_id(&self, ty_id: TypeId) -> Option<NominalTypeId> {
        use crate::parser::types::ty::Type;

        if let Some(&cid) = self.type_alias_object_class_map.get(&ty_id) {
            return Some(cid);
        }
        // Some checker paths materialize structural object TypeIds that are equivalent
        // to a named wrapper alias (e.g. __t_m0_TcpStream). Resolve those back to class IDs.
        if !self.type_alias_class_map.is_empty() {
            let mut subtype_ctx =
                crate::parser::types::subtyping::SubtypingContext::new(self.type_ctx);
            for (alias_name, &cid) in &self.type_alias_class_map {
                let alias_ty = self
                    .type_alias_resolved_type_map
                    .get(alias_name)
                    .copied()
                    .filter(|ty| *ty != UNRESOLVED)
                    .or_else(|| self.type_ctx.lookup_named_type(alias_name));
                let Some(alias_ty) = alias_ty else {
                    continue;
                };
                if ty_id == alias_ty
                    || (subtype_ctx.is_subtype(ty_id, alias_ty)
                        && subtype_ctx.is_subtype(alias_ty, ty_id))
                {
                    return Some(cid);
                }
            }
        }

        let ty = self.type_ctx.get(ty_id)?;
        match ty {
            Type::Class(class_ty) => self.nominal_type_id_from_type_name(&class_ty.name),
            Type::Reference(type_ref) => self.nominal_type_id_from_type_name(&type_ref.name),
            Type::Generic(generic) => {
                if let Some(Type::JSObject) = self.type_ctx.get(generic.base) {
                    if let Some(&inner) = generic.type_args.first() {
                        return self.nominal_type_id_from_type_id(inner);
                    }
                }
                self.nominal_type_id_from_type_id(generic.base)
            }
            Type::TypeVar(tv) => tv
                .constraint
                .and_then(|constraint| self.nominal_type_id_from_type_id(constraint)),
            Type::Union(union) => {
                // Prefer the single concrete class member if present.
                let mut found: Option<NominalTypeId> = None;
                for member in &union.members {
                    if let Some(cid) = self.nominal_type_id_from_type_id(*member) {
                        match found {
                            None => found = Some(cid),
                            Some(existing) if existing == cid => {}
                            Some(_) => return None,
                        }
                    }
                }
                found
            }
            _ => None,
        }
    }

    /// Apply type arguments from __NATIVE_CALL<Type> to the dest register.
    /// This lets the compiler know the return type of native calls, enabling
    /// subsequent method dispatch on the result (e.g., Buffer.length()).
    fn apply_native_call_type_args(&mut self, call: &ast::CallExpression, dest: &mut Register) {
        if let Some(type_args) = &call.type_args {
            if let Some(first_ty) = type_args.first() {
                let resolved_ty = self.resolve_type_annotation(first_ty);
                if resolved_ty != UNRESOLVED {
                    dest.ty = resolved_ty;
                }
            }
        }
    }

    fn lower_logical(&mut self, logical: &ast::LogicalExpression) -> Register {
        // Logical operators need short-circuit evaluation
        let left = self.lower_expr(&logical.left);

        let eval_right = self.alloc_block();
        let merge_block = self.alloc_block();

        let left_block = self.current_block;

        match logical.operator {
            ast::LogicalOperator::And => {
                // &&: if left is falsy, return left; else evaluate and return right
                self.set_terminator(crate::ir::Terminator::Branch {
                    cond: left.clone(),
                    then_block: eval_right,
                    else_block: merge_block,
                });
            }
            ast::LogicalOperator::Or => {
                // ||: if left is truthy, return left; else evaluate and return right
                self.set_terminator(crate::ir::Terminator::Branch {
                    cond: left.clone(),
                    then_block: merge_block,
                    else_block: eval_right,
                });
            }
            ast::LogicalOperator::NullishCoalescing => {
                // ??: if left is null, evaluate and return right; else return left
                self.set_terminator(crate::ir::Terminator::BranchIfNull {
                    value: left.clone(),
                    null_block: eval_right,
                    not_null_block: merge_block,
                });
            }
        }

        // Evaluate right side
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::new(eval_right));
        self.current_block = eval_right;
        let right = self.lower_expr(&logical.right);
        self.set_terminator(crate::ir::Terminator::Jump(merge_block));
        let right_block = self.current_block;

        // Merge - use left type for PHI since nullish coalescing can return left's value
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::new(merge_block));
        self.current_block = merge_block;

        let dest = self.alloc_register(left.ty);
        self.emit(IrInstr::Phi {
            dest: dest.clone(),
            sources: vec![(left_block, left), (right_block, right)],
        });

        dest
    }

    fn lower_template_literal(&mut self, template: &ast::TemplateLiteral) -> Register {
        let string_ty = TypeId::new(STRING_TYPE_ID);

        // If no parts, return empty string
        if template.parts.is_empty() {
            let dest = self.alloc_register(string_ty);
            self.emit(IrInstr::Assign {
                dest: dest.clone(),
                value: IrValue::Constant(IrConstant::String(String::new())),
            });
            return dest;
        }

        // Check if all parts are strings - we can concatenate at compile time
        let all_strings = template
            .parts
            .iter()
            .all(|p| matches!(p, TemplatePart::String(_)));

        if all_strings {
            // Compile-time concatenation
            let mut result = String::new();
            for part in &template.parts {
                if let TemplatePart::String(sym) = part {
                    result.push_str(self.interner.resolve(*sym));
                }
            }
            let dest = self.alloc_register(string_ty);
            self.emit(IrInstr::Assign {
                dest: dest.clone(),
                value: IrValue::Constant(IrConstant::String(result)),
            });
            return dest;
        }

        // Mixed parts - need runtime concatenation
        // Convert each part to a string register, then concatenate
        let mut part_registers: Vec<Register> = Vec::new();

        for part in &template.parts {
            match part {
                TemplatePart::String(sym) => {
                    let s = self.interner.resolve(*sym).to_string();
                    let reg = self.alloc_register(string_ty);
                    self.emit(IrInstr::Assign {
                        dest: reg.clone(),
                        value: IrValue::Constant(IrConstant::String(s)),
                    });
                    part_registers.push(reg);
                }
                TemplatePart::Expression(expr) => {
                    let expr_reg = self.lower_expr(expr);
                    // Convert to string if not already a string
                    if expr_reg.ty.as_u32() == STRING_TYPE_ID {
                        // Already a string
                        part_registers.push(expr_reg);
                    } else {
                        // Need to convert to string
                        let str_reg = self.alloc_register(string_ty);
                        self.emit(IrInstr::ToString {
                            dest: str_reg.clone(),
                            operand: expr_reg,
                        });
                        part_registers.push(str_reg);
                    }
                }
            }
        }

        // Concatenate all parts
        if part_registers.len() == 1 {
            return part_registers.into_iter().next().unwrap();
        }

        // Chain concatenation: ((a + b) + c) + d ...
        let mut result = part_registers.remove(0);
        for part in part_registers {
            let concat_result = self.alloc_register(string_ty);
            self.emit(IrInstr::BinaryOp {
                dest: concat_result.clone(),
                op: BinaryOp::Concat,
                left: result,
                right: part,
            });
            result = concat_result;
        }

        result
    }

    /// Convert AST binary operator to IR binary operator
    fn convert_binary_op(&self, op: &ast::BinaryOperator) -> BinaryOp {
        match op {
            ast::BinaryOperator::Add => BinaryOp::Add,
            ast::BinaryOperator::Subtract => BinaryOp::Sub,
            ast::BinaryOperator::Multiply => BinaryOp::Mul,
            ast::BinaryOperator::Divide => BinaryOp::Div,
            ast::BinaryOperator::Modulo => BinaryOp::Mod,
            ast::BinaryOperator::Equal => BinaryOp::Equal,
            ast::BinaryOperator::StrictEqual => BinaryOp::Equal,
            ast::BinaryOperator::NotEqual => BinaryOp::NotEqual,
            ast::BinaryOperator::StrictNotEqual => BinaryOp::NotEqual,
            ast::BinaryOperator::LessThan => BinaryOp::Less,
            ast::BinaryOperator::LessEqual => BinaryOp::LessEqual,
            ast::BinaryOperator::GreaterThan => BinaryOp::Greater,
            ast::BinaryOperator::GreaterEqual => BinaryOp::GreaterEqual,
            ast::BinaryOperator::BitwiseAnd => BinaryOp::BitAnd,
            ast::BinaryOperator::BitwiseOr => BinaryOp::BitOr,
            ast::BinaryOperator::BitwiseXor => BinaryOp::BitXor,
            ast::BinaryOperator::LeftShift => BinaryOp::ShiftLeft,
            ast::BinaryOperator::RightShift => BinaryOp::ShiftRight,
            ast::BinaryOperator::UnsignedRightShift => BinaryOp::UnsignedShiftRight,
            ast::BinaryOperator::Exponent => BinaryOp::Pow,
        }
    }

    /// Convert AST unary operator to IR unary operator
    fn convert_unary_op(&self, op: &ast::UnaryOperator) -> UnaryOp {
        match op {
            ast::UnaryOperator::Minus => UnaryOp::Neg,
            ast::UnaryOperator::Not => UnaryOp::Not,
            ast::UnaryOperator::BitwiseNot => UnaryOp::BitNot,
            _ => UnaryOp::Neg, // Fallback
        }
    }

    /// Infer result type for binary operation
    fn infer_binary_result_type(&self, op: &BinaryOp, left: &Register, right: &Register) -> TypeId {
        if op.is_comparison() || op.is_logical() {
            TypeId::new(BOOLEAN_TYPE_ID)
        } else {
            let l = left.ty.as_u32();
            let r = right.ty.as_u32();
            // String concatenation: if either operand is a string, result is string
            if matches!(op, BinaryOp::Add) && (l == STRING_TYPE_ID || r == STRING_TYPE_ID) {
                TypeId::new(STRING_TYPE_ID)
            } else if l == NUMBER_TYPE_ID || r == NUMBER_TYPE_ID {
                // Mixed int+number promotes to number (f64)
                TypeId::new(NUMBER_TYPE_ID)
            } else {
                left.ty
            }
        }
    }

    fn is_string_like_type(&self, ty: TypeId) -> bool {
        match self.type_ctx.get(ty) {
            Some(Type::Primitive(PrimitiveType::String)) => true,
            Some(Type::Reference(type_ref)) => type_ref.name == "string",
            Some(Type::Class(class_ty)) => class_ty.name == "string",
            _ => false,
        }
    }

    // ============================================================================
    // JSX Lowering
    // ============================================================================
    //
    // JSX elements and fragments are desugared to createElement() calls:
    //   <div className="x">{name}</div>
    //   →  createElement("div", { className: "x" }, name)
    //
    //   <>A B</>
    //   →  createElement(Fragment, null, "A", "B")

    /// Lower a JSX element to a createElement call
    fn lower_jsx_element(&mut self, jsx: &ast::JsxElement) -> Register {
        let jsx_options = match &self.jsx_options {
            Some(opts) => opts.clone(),
            None => {
                // JSX not enabled — emit null
                return self.lower_null_literal();
            }
        };

        // 1. Lower the tag argument
        let tag_reg = self.lower_jsx_tag(&jsx.opening.name);

        // 2. Lower props (attributes → object literal, or null)
        let props_reg = self.lower_jsx_props(&jsx.opening.attributes);

        // 3. Lower children
        let child_regs = self.lower_jsx_children(&jsx.children);

        // 4. Emit factory call: createElement(tag, props, ...children)
        self.emit_jsx_factory_call(&jsx_options.factory, tag_reg, props_reg, child_regs)
    }

    /// Lower a JSX fragment to a createElement call
    fn lower_jsx_fragment(&mut self, jsx: &ast::JsxFragment) -> Register {
        let jsx_options = match &self.jsx_options {
            Some(opts) => opts.clone(),
            None => {
                return self.lower_null_literal();
            }
        };

        // 1. Fragment identifier as tag
        let tag_reg = self.lower_jsx_fragment_tag(&jsx_options.fragment);

        // 2. Null props
        let props_reg = self.lower_null_literal();

        // 3. Lower children
        let child_regs = self.lower_jsx_children(&jsx.children);

        // 4. Emit factory call
        self.emit_jsx_factory_call(&jsx_options.factory, tag_reg, props_reg, child_regs)
    }

    /// Lower a JSX element name to a register.
    ///
    /// - Intrinsic elements (lowercase: div, span) → string literal
    /// - Component elements (uppercase: Button) → identifier reference
    /// - Member expressions (UI.Button) → member access chain
    /// - Namespaced (svg:path) → string literal
    fn lower_jsx_tag(&mut self, name: &ast::JsxElementName) -> Register {
        match name {
            ast::JsxElementName::Identifier(ident) if name.is_intrinsic(self.interner) => {
                // Intrinsic HTML element → string: "div", "span"
                let tag_name = self.interner.resolve(ident.name).to_string();
                let dest = self.alloc_register(TypeId::new(STRING_TYPE_ID)); // String
                self.emit(IrInstr::Assign {
                    dest: dest.clone(),
                    value: IrValue::Constant(IrConstant::String(tag_name)),
                });
                dest
            }
            ast::JsxElementName::Identifier(ident) => {
                // Component → identifier reference
                self.lower_identifier(ident)
            }
            ast::JsxElementName::MemberExpression { object, property } => {
                // UI.Button → member access chain
                self.lower_jsx_member_name(object, property)
            }
            ast::JsxElementName::Namespaced { namespace, name } => {
                // svg:path → string "svg:path"
                let ns = self.interner.resolve(namespace.name);
                let n = self.interner.resolve(name.name);
                let tag_name = format!("{}:{}", ns, n);
                let dest = self.alloc_register(TypeId::new(STRING_TYPE_ID)); // String
                self.emit(IrInstr::Assign {
                    dest: dest.clone(),
                    value: IrValue::Constant(IrConstant::String(tag_name)),
                });
                dest
            }
        }
    }

    /// Lower a JSX member expression name (e.g., UI.Components.Button)
    fn lower_jsx_member_name(
        &mut self,
        object: &ast::JsxElementName,
        property: &ast::Identifier,
    ) -> Register {
        let obj_reg = match object {
            ast::JsxElementName::Identifier(ident) => self.lower_identifier(ident),
            ast::JsxElementName::MemberExpression {
                object: inner_obj,
                property: inner_prop,
            } => self.lower_jsx_member_name(inner_obj, inner_prop),
            ast::JsxElementName::Namespaced { .. } => self.lower_null_literal(),
        };

        // Emit field access: obj.property
        let dest = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
        let field_name = self.interner.resolve(property.name).to_string();
        self.emit(IrInstr::DynGetProp {
            dest: dest.clone(),
            object: obj_reg,
            property: field_name,
        });
        dest
    }

    /// Lower a Fragment identifier tag
    fn lower_jsx_fragment_tag(&mut self, fragment_name: &str) -> Register {
        // Try to resolve as an existing identifier in scope
        if let Some(sym) = self.interner.lookup(fragment_name) {
            if let Some(&local_idx) = self.local_map.get(&sym) {
                let dest = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
                self.emit(IrInstr::LoadLocal {
                    dest: dest.clone(),
                    index: local_idx,
                });
                return dest;
            }
        }
        // Fragment not in scope — emit as a null (framework should provide it)
        self.lower_null_literal()
    }

    /// Lower JSX attributes into a props object (or null if empty)
    fn lower_jsx_props(&mut self, attributes: &[ast::JsxAttribute]) -> Register {
        if attributes.is_empty() {
            return self.lower_null_literal();
        }

        // Check for spread attributes
        let has_spread = attributes
            .iter()
            .any(|a| matches!(a, ast::JsxAttribute::Spread { .. }));

        if has_spread {
            return self.lower_jsx_props_with_spread(attributes);
        }

        // Simple case: all regular attributes → object literal
        let mut fields = Vec::new();
        let mut field_layout = Vec::new();

        for (idx, attr) in attributes.iter().enumerate() {
            if let ast::JsxAttribute::Attribute { name, value, .. } = attr {
                let value_reg = self.lower_jsx_attr_value(value);
                fields.push((idx as u16, value_reg));

                let key = self.jsx_attr_name_string(name);
                field_layout.push((key, idx));
            }
        }

        let dest = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
        let ordered_names: Vec<String> = field_layout.iter().map(|(key, _)| key.clone()).collect();
        let type_index = self.structural_layout_id_from_ordered_names(&ordered_names);
        self.emit(IrInstr::ObjectLiteral {
            dest: dest.clone(),
            type_index,
            fields,
        });

        // Register field layout for destructuring support
        self.register_object_fields.insert(dest.id, field_layout);

        dest
    }

    /// Lower JSX props that include spread attributes.
    ///
    /// Uses DynSetProp to build the object incrementally,
    /// preserving attribute evaluation order.
    fn lower_jsx_props_with_spread(&mut self, attributes: &[ast::JsxAttribute]) -> Register {
        // Start with an empty object
        let dest = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
        let type_index = self.structural_layout_id_from_ordered_names(&[]);
        self.emit(IrInstr::ObjectLiteral {
            dest: dest.clone(),
            type_index,
            fields: vec![],
        });

        for attr in attributes {
            match attr {
                ast::JsxAttribute::Spread { argument, .. } => {
                    // Lower the spread source and merge its properties into the dest object
                    let spread_reg = self.lower_expr(argument);
                    self.emit(IrInstr::NativeCall {
                        dest: Some(dest.clone()),
                        native_id: crate::compiler::native_id::JSON_MERGE,
                        args: vec![dest.clone(), spread_reg],
                    });
                }
                ast::JsxAttribute::Attribute { name, value, .. } => {
                    let key = self.jsx_attr_name_string(name);
                    let value_reg = self.lower_jsx_attr_value(value);
                    self.emit(IrInstr::DynSetProp {
                        object: dest.clone(),
                        property: key,
                        value: value_reg,
                    });
                }
            }
        }

        dest
    }

    /// Lower a single JSX attribute value
    fn lower_jsx_attr_value(&mut self, value: &Option<ast::JsxAttributeValue>) -> Register {
        match value {
            Some(ast::JsxAttributeValue::StringLiteral(lit)) => self.lower_string_literal(lit),
            Some(ast::JsxAttributeValue::Expression(expr)) => self.lower_expr(expr),
            Some(ast::JsxAttributeValue::JsxElement(jsx)) => self.lower_jsx_element(jsx),
            Some(ast::JsxAttributeValue::JsxFragment(jsx)) => self.lower_jsx_fragment(jsx),
            None => {
                // Boolean attribute: <input disabled /> → true
                let dest = self.alloc_register(TypeId::new(BOOLEAN_TYPE_ID)); // Boolean
                self.emit(IrInstr::Assign {
                    dest: dest.clone(),
                    value: IrValue::Constant(IrConstant::Boolean(true)),
                });
                dest
            }
        }
    }

    /// Get the string key name for a JSX attribute
    fn jsx_attr_name_string(&self, name: &ast::JsxAttributeName) -> String {
        match name {
            ast::JsxAttributeName::Identifier(ident) => {
                self.interner.resolve(ident.name).to_string()
            }
            ast::JsxAttributeName::Namespaced { namespace, name } => {
                format!(
                    "{}:{}",
                    self.interner.resolve(namespace.name),
                    self.interner.resolve(name.name)
                )
            }
        }
    }

    /// Lower JSX children, filtering out whitespace-only text nodes
    fn lower_jsx_children(&mut self, children: &[ast::JsxChild]) -> Vec<Register> {
        let mut regs = Vec::new();

        for child in children {
            match child {
                ast::JsxChild::Text(text) => {
                    // Skip whitespace-only text nodes
                    let trimmed = text.value.trim();
                    if !trimmed.is_empty() {
                        let dest = self.alloc_register(TypeId::new(STRING_TYPE_ID)); // String
                        self.emit(IrInstr::Assign {
                            dest: dest.clone(),
                            value: IrValue::Constant(IrConstant::String(trimmed.to_string())),
                        });
                        regs.push(dest);
                    }
                }
                ast::JsxChild::Element(jsx_elem) => {
                    regs.push(self.lower_jsx_element(jsx_elem));
                }
                ast::JsxChild::Fragment(jsx_frag) => {
                    regs.push(self.lower_jsx_fragment(jsx_frag));
                }
                ast::JsxChild::Expression(jsx_expr) => {
                    if let Some(ref expr) = jsx_expr.expression {
                        regs.push(self.lower_expr(expr));
                    }
                    // Empty expressions {} are skipped
                }
            }
        }

        regs
    }

    /// Emit the JSX factory function call: factory(tag, props, ...children)
    ///
    /// Resolves the factory function in scope (function_map or local_map)
    /// and emits the appropriate call instruction.
    fn emit_jsx_factory_call(
        &mut self,
        factory_name: &str,
        tag: Register,
        props: Register,
        children: Vec<Register>,
    ) -> Register {
        // Build argument list: (tag, props, ...children)
        let mut args = vec![tag, props];
        args.extend(children);

        let dest = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));

        // Try to resolve factory by symbol in the interner
        if let Some(factory_sym) = self.interner.lookup(factory_name) {
            // Check if it's a known function (declared in this module)
            if let Some(&func_id) = self.function_map.get(&factory_sym) {
                self.emit(IrInstr::Call {
                    dest: Some(dest.clone()),
                    func: func_id,
                    args,
                });
                return dest;
            }

            // Check if it's a local variable (imported function / closure)
            if let Some(&local_idx) = self.local_map.get(&factory_sym) {
                let closure = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
                self.emit(IrInstr::LoadLocal {
                    dest: closure.clone(),
                    index: local_idx,
                });
                self.emit(IrInstr::CallClosure {
                    dest: Some(dest.clone()),
                    closure,
                    args,
                });
                return dest;
            }
        }

        // Factory must resolve at compile time; do not lower an unsafe null-closure call.
        self.errors
            .push(crate::compiler::CompileError::InternalError {
                message: format!(
                    "unresolved JSX factory '{}': no function or local closure binding in scope",
                    factory_name
                ),
            });
        self.poison_register(&dest);
        dest
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{Interner, Parser};

    fn parse_expr(source: &str) -> (ast::Module, Interner) {
        let parser = Parser::new(source).expect("lexer error");
        parser.parse().expect("parse error")
    }

    #[test]
    fn test_lower_integer_literal() {
        let (module, interner) = parse_expr("42;");
        let type_ctx = crate::parser::TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let ir = lowerer.lower_module(&module);

        // Should have a main function with the expression
        assert!(ir.function_count() >= 1);
    }

    #[test]
    fn test_lower_binary_expression() {
        let (module, interner) = parse_expr("1 + 2;");
        let type_ctx = crate::parser::TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let ir = lowerer.lower_module(&module);

        assert!(ir.function_count() >= 1);
    }

    #[test]
    fn test_lower_function() {
        let source = r#"
            function add(a: number, b: number): number {
                return a + b;
            }
        "#;
        let (module, interner) = parse_expr(source);
        let type_ctx = crate::parser::TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let ir = lowerer.lower_module(&module);

        assert!(ir.get_function_by_name("add").is_some());
    }

    #[test]
    fn unresolved_member_call_reports_compile_error() {
        let (module, interner) = parse_expr(
            r#"
            let n = 1;
            n.missing();
            "#,
        );
        let type_ctx = crate::parser::TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let _ = lowerer.lower_module(&module);

        let has_unresolved_call = lowerer.errors().iter().any(|err| {
            err.to_string()
                .contains("unresolved member call 'missing()'")
        });
        assert!(
            has_unresolved_call,
            "expected unresolved member call error, got: {:?}",
            lowerer.errors()
        );
    }

    #[test]
    fn unresolved_member_call_emits_poison_value_instead_of_uninitialized_dest() {
        let (module, interner) = parse_expr(
            r#"
            let n = 1;
            n.missing();
            "#,
        );
        let type_ctx = crate::parser::TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let ir = lowerer.lower_module(&module);

        let has_null_assign = ir.functions.iter().any(|func| {
            func.blocks.iter().any(|block| {
                block.instructions.iter().any(|instr| {
                    matches!(
                        instr,
                        IrInstr::Assign {
                            value: IrValue::Constant(IrConstant::Null),
                            ..
                        }
                    )
                })
            })
        });
        assert!(
            has_null_assign,
            "expected poison null assignment for unresolved member call"
        );
    }

    #[test]
    fn unresolved_member_property_reports_compile_error() {
        let (module, interner) = parse_expr(
            r#"
            let n = 1;
            n.missing;
            "#,
        );
        let type_ctx = crate::parser::TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let _ = lowerer.lower_module(&module);

        let has_unresolved_property = lowerer.errors().iter().any(|err| {
            err.to_string().contains("unresolved member property")
                && err.to_string().contains(".missing")
        });
        assert!(
            has_unresolved_property,
            "expected unresolved member property error, got: {:?}",
            lowerer.errors()
        );
    }

    #[test]
    fn unresolved_member_property_does_not_emit_reflect_get_fallback() {
        let (module, interner) = parse_expr(
            r#"
            let n = 1;
            n.missing;
            "#,
        );
        let type_ctx = crate::parser::TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let ir = lowerer.lower_module(&module);

        let has_reflect_get = ir.functions.iter().any(|func| {
            func.blocks.iter().any(|block| {
                block.instructions.iter().any(|instr| {
                    matches!(
                        instr,
                        IrInstr::NativeCall {
                            native_id: crate::compiler::native_id::REFLECT_GET,
                            ..
                        }
                    )
                })
            })
        });
        assert!(
            !has_reflect_get,
            "did not expect Reflect.get fallback for unresolved member property"
        );
    }

    #[test]
    fn unresolved_async_member_call_reports_compile_error() {
        let (module, interner) = parse_expr(
            r#"
            let n = 1;
            async n.missing();
            "#,
        );
        let type_ctx = crate::parser::TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let _ = lowerer.lower_module(&module);

        let has_unresolved_async_call = lowerer.errors().iter().any(|err| {
            err.to_string()
                .contains("unresolved async member call 'missing()'")
                || (err.to_string().contains("unresolved member property")
                    && err.to_string().contains(".missing"))
        });
        assert!(
            has_unresolved_async_call,
            "expected unresolved async member call error, got: {:?}",
            lowerer.errors()
        );
    }

    #[test]
    fn unresolved_local_call_without_callable_type_reports_compile_error() {
        let (module, interner) = parse_expr(
            r#"
            let x = 1;
            x();
            "#,
        );
        let type_ctx = crate::parser::TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let _ = lowerer.lower_module(&module);

        let has_unresolved_call = lowerer.errors().iter().any(|err| {
            err.to_string().contains("unresolved call target")
                && err.to_string().contains("not callable")
        });
        assert!(
            has_unresolved_call,
            "expected unresolved local-call error, got: {:?}",
            lowerer.errors()
        );
    }

    #[test]
    fn unresolved_local_async_call_without_callable_type_reports_compile_error() {
        let (module, interner) = parse_expr(
            r#"
            let x = 1;
            async x();
            "#,
        );
        let type_ctx = crate::parser::TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let _ = lowerer.lower_module(&module);

        let has_unresolved_async_call = lowerer.errors().iter().any(|err| {
            err.to_string().contains("unresolved async call target")
                && err.to_string().contains("not callable")
        });
        assert!(
            has_unresolved_async_call,
            "expected unresolved local-async-call error, got: {:?}",
            lowerer.errors()
        );
    }

    #[test]
    fn unresolved_local_async_call_emits_poison_value_instead_of_uninitialized_dest() {
        let (module, interner) = parse_expr(
            r#"
            let x = 1;
            async x();
            "#,
        );
        let type_ctx = crate::parser::TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let ir = lowerer.lower_module(&module);

        let has_null_assign = ir.functions.iter().any(|func| {
            func.blocks.iter().any(|block| {
                block.instructions.iter().any(|instr| {
                    matches!(
                        instr,
                        IrInstr::Assign {
                            value: IrValue::Constant(IrConstant::Null),
                            ..
                        }
                    )
                })
            })
        });
        assert!(
            has_null_assign,
            "expected poison null assignment for unresolved async local call"
        );
    }

    #[test]
    fn unresolved_parenthesized_call_without_callable_type_reports_compile_error() {
        let (module, interner) = parse_expr(
            r#"
            let x = 1;
            (x)();
            "#,
        );
        let type_ctx = crate::parser::TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let _ = lowerer.lower_module(&module);

        let has_unresolved_call = lowerer.errors().iter().any(|err| {
            err.to_string().contains("unresolved call target")
                && err.to_string().contains("not callable")
        });
        assert!(
            has_unresolved_call,
            "expected unresolved parenthesized-call error, got: {:?}",
            lowerer.errors()
        );
    }

    #[test]
    fn unresolved_tagged_template_non_callable_emits_compile_error_and_poison() {
        let (module, interner) = parse_expr(
            r#"
            let n = 1;
            n`x`;
            "#,
        );
        let type_ctx = crate::parser::TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let ir = lowerer.lower_module(&module);

        let has_error = lowerer.errors().iter().any(|err| {
            err.to_string()
                .contains("unresolved tagged template call target")
        });
        assert!(
            has_error,
            "expected unresolved tagged-template non-callable error, got: {:?}",
            lowerer.errors()
        );

        let has_null_assign = ir.functions.iter().any(|func| {
            func.blocks.iter().any(|block| {
                block.instructions.iter().any(|instr| {
                    matches!(
                        instr,
                        IrInstr::Assign {
                            value: IrValue::Constant(IrConstant::Null),
                            ..
                        }
                    )
                })
            })
        });
        assert!(
            has_null_assign,
            "expected poison null assignment for unresolved tagged-template call"
        );
    }

    #[test]
    fn callable_symbol_hints_do_not_leak_between_functions() {
        let (module, interner) = parse_expr(
            r#"
            function acceptsCb(cb: (n: number) => number): number {
                return cb(1);
            }
            function bad(cb: number): number {
                return cb();
            }
            "#,
        );
        let type_ctx = crate::parser::TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let _ = lowerer.lower_module(&module);

        let has_non_callable_error = lowerer.errors().iter().any(|err| {
            err.to_string().contains("unresolved call target 'cb'")
                && err.to_string().contains("not callable")
        });
        assert!(
            has_non_callable_error,
            "expected non-callable error for second function cb(), got: {:?}",
            lowerer.errors()
        );
    }

    #[test]
    fn callable_cast_local_variable_allows_call_lowering() {
        let (module, interner) = parse_expr(
            r#"
            function run(value: number): number {
                let fn = (value as ((n: number) => number));
                return fn(1);
            }
            "#,
        );
        let type_ctx = crate::parser::TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let _ = lowerer.lower_module(&module);

        let has_non_callable_error = lowerer.errors().iter().any(|err| {
            err.to_string().contains("unresolved call target 'fn'")
                && err.to_string().contains("not callable")
        });
        assert!(
            !has_non_callable_error,
            "did not expect non-callable error for casted callable local, got: {:?}",
            lowerer.errors()
        );
    }

    #[test]
    fn arrow_param_shadowing_callable_symbol_reports_non_callable_error() {
        let (module, interner) = parse_expr(
            r#"
            function outer(cb: (n: number) => number): number {
                let f = (cb: number): number => cb();
                return f(1);
            }
            "#,
        );
        let type_ctx = crate::parser::TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let _ = lowerer.lower_module(&module);

        let has_non_callable_error = lowerer.errors().iter().any(|err| {
            err.to_string().contains("unresolved call target 'cb'")
                && err.to_string().contains("not callable")
        });
        assert!(
            has_non_callable_error,
            "expected non-callable error for shadowed arrow param, got: {:?}",
            lowerer.errors()
        );
    }

    #[test]
    fn reassigned_local_from_callable_to_number_reports_non_callable_error() {
        let (module, interner) = parse_expr(
            r#"
            function run(): number {
                let fn = (x: number): number => x;
                fn = 1;
                return fn();
            }
            "#,
        );
        let type_ctx = crate::parser::TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let _ = lowerer.lower_module(&module);

        let has_non_callable_error = lowerer.errors().iter().any(|err| {
            err.to_string().contains("unresolved call target 'fn'")
                && err.to_string().contains("not callable")
        });
        assert!(
            has_non_callable_error,
            "expected non-callable error after fn reassignment, got: {:?}",
            lowerer.errors()
        );
    }

    #[test]
    fn compound_assignment_invalidates_callable_hint() {
        let (module, interner) = parse_expr(
            r#"
            function run(): number {
                let fn = (x: number): number => x;
                fn += 1;
                return fn();
            }
            "#,
        );
        let type_ctx = crate::parser::TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let _ = lowerer.lower_module(&module);

        let has_non_callable_error = lowerer.errors().iter().any(|err| {
            err.to_string().contains("unresolved call target 'fn'")
                && err.to_string().contains("not callable")
        });
        assert!(
            has_non_callable_error,
            "expected non-callable error after compound assignment, got: {:?}",
            lowerer.errors()
        );
    }

    #[test]
    fn null_coalesce_assignment_invalidates_callable_hint_when_rhs_not_callable() {
        let (module, interner) = parse_expr(
            r#"
            function run(): number {
                let fn: ((x: number) => number) | number | null = null;
                fn ??= 1;
                return fn();
            }
            "#,
        );
        let type_ctx = crate::parser::TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let _ = lowerer.lower_module(&module);

        let has_non_callable_error = lowerer.errors().iter().any(|err| {
            err.to_string().contains("unresolved call target 'fn'")
                && err.to_string().contains("not callable")
        });
        assert!(
            has_non_callable_error,
            "expected non-callable error after ??= non-callable rhs, got: {:?}",
            lowerer.errors()
        );
    }

    #[test]
    fn captured_callable_reassignment_in_arrow_invalidates_callable_hint() {
        let (module, interner) = parse_expr(
            r#"
            function outer(): number {
                let fn = (x: number): number => x;
                let mutate = (): number => {
                    fn = 1;
                    return 0;
                };
                mutate();
                return fn();
            }
            "#,
        );
        let type_ctx = crate::parser::TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let _ = lowerer.lower_module(&module);

        let has_non_callable_error = lowerer.errors().iter().any(|err| {
            err.to_string().contains("unresolved call target 'fn'")
                && err.to_string().contains("not callable")
        });
        assert!(
            has_non_callable_error,
            "expected non-callable error after captured reassignment, got: {:?}",
            lowerer.errors()
        );
    }

    #[test]
    fn captured_callable_null_coalesce_assignment_in_arrow_invalidates_hint() {
        let (module, interner) = parse_expr(
            r#"
            function outer(): number {
                let fn: ((x: number) => number) | number | null = null;
                let mutate = (): number => {
                    fn ??= 1;
                    return 0;
                };
                mutate();
                return fn();
            }
            "#,
        );
        let type_ctx = crate::parser::TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let _ = lowerer.lower_module(&module);

        let has_non_callable_error = lowerer.errors().iter().any(|err| {
            err.to_string().contains("unresolved call target 'fn'")
                && err.to_string().contains("not callable")
        });
        assert!(
            has_non_callable_error,
            "expected non-callable error after captured ??= reassignment, got: {:?}",
            lowerer.errors()
        );
    }

    #[test]
    fn captured_callable_reassignment_in_arrow_invalidates_async_call_hint() {
        let (module, interner) = parse_expr(
            r#"
            function outer(): number {
                let fn = (x: number): number => x;
                let mutate = (): number => {
                    fn = 1;
                    return 0;
                };
                mutate();
                async fn();
                return 0;
            }
            "#,
        );
        let type_ctx = crate::parser::TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let _ = lowerer.lower_module(&module);

        let has_non_callable_error = lowerer.errors().iter().any(|err| {
            err.to_string()
                .contains("unresolved async call target 'fn'")
                && err.to_string().contains("not callable")
        });
        assert!(
            has_non_callable_error,
            "expected non-callable async error after captured reassignment, got: {:?}",
            lowerer.errors()
        );
    }

    #[test]
    fn nested_arrow_async_call_on_captured_callable_param_does_not_false_error() {
        let (module, interner) = parse_expr(
            r#"
            function outer(cb: (x: number) => number): number {
                let run = (): number => {
                    async cb();
                    return 0;
                };
                return run();
            }
            "#,
        );
        let type_ctx = crate::parser::TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let _ = lowerer.lower_module(&module);

        let has_unresolved_async_cb_error = lowerer.errors().iter().any(|err| {
            err.to_string()
                .contains("unresolved async call target 'cb'")
                && err.to_string().contains("not callable")
        });
        assert!(
            !has_unresolved_async_cb_error,
            "did not expect unresolved async-call error for captured callable param, got: {:?}",
            lowerer.errors()
        );
    }

    #[test]
    fn arrow_local_shadow_does_not_erase_parent_bound_method_mapping() {
        let (module, interner) = parse_expr(
            r#"
            class A {
                m(): number { return 1; }
            }
            function outer(): number {
                let a = new A();
                let f = a.m;
                let g = (): number => {
                    let f = 1;
                    return f;
                };
                g();
                return f();
            }
            "#,
        );
        let type_ctx = crate::parser::TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let _ = lowerer.lower_module(&module);

        let has_non_callable_f_error = lowerer.errors().iter().any(|err| {
            err.to_string().contains("unresolved call target 'f'")
                && err.to_string().contains("not callable")
        });
        assert!(
            !has_non_callable_f_error,
            "did not expect parent bound-method mapping to be erased by child shadow, got: {:?}",
            lowerer.errors()
        );
    }

    #[test]
    fn parser_produces_tagged_template_expression_shape() {
        let (module, _interner) = parse_expr(
            r#"
            let n: number = 1;
            n`x`;
            "#,
        );

        let mut found = false;
        for stmt in &module.statements {
            if let crate::parser::ast::Statement::Expression(expr_stmt) = stmt {
                if matches!(
                    expr_stmt.expression,
                    crate::parser::ast::Expression::TaggedTemplate(_)
                ) {
                    found = true;
                    break;
                }
            }
        }
        assert!(found, "expected tagged template expression in parsed AST");
    }
}
