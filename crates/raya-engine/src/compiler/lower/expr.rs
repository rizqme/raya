//! Expression Lowering
//!
//! Converts AST expressions to IR instructions.

use super::{
    ClassFieldInfo, ConstantValue, Lowerer, BOOLEAN_TYPE_ID, CHANNEL_TYPE_ID, INT_TYPE_ID,
    JSON_TYPE_ID, NULL_TYPE_ID, NUMBER_TYPE_ID, REGEXP_TYPE_ID, STRING_TYPE_ID, TASK_TYPE_ID,
    UNKNOWN_TYPE_ID, UNRESOLVED, UNRESOLVED_TYPE_ID,
};
use crate::compiler::ir::{
    BinaryOp, ClassId, FunctionId, IrConstant, IrInstr, IrValue, Register, Terminator, UnaryOp,
};
use crate::compiler::CompileError;
use crate::parser::ast::{self, AssignmentOperator, Expression, TemplatePart};
use crate::parser::interner::Symbol;
use crate::parser::{TypeContext as TC, TypeId};
use rustc_hash::FxHashMap;

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

impl<'a> Lowerer<'a> {
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
            Expression::Object(object) => self.lower_object(object),
            Expression::Assignment(assign) => self.lower_assignment(assign),
            Expression::Conditional(cond) => self.lower_conditional(cond),
            Expression::Arrow(arrow) => self.lower_arrow(arrow),
            Expression::Parenthesized(paren) => self.lower_expr(&paren.expression),
            Expression::Typeof(typeof_expr) => self.lower_typeof(typeof_expr),
            Expression::New(new_expr) => self.lower_new(new_expr),
            Expression::Await(await_expr) => self.lower_await(await_expr),
            Expression::Logical(logical) => self.lower_logical(logical),
            Expression::TemplateLiteral(template) => self.lower_template_literal(template),
            Expression::This(_) => self.lower_this(),
            Expression::Super(_) => self.lower_super(),
            Expression::AsyncCall(async_call) => self.lower_async_call(async_call),
            Expression::InstanceOf(instanceof) => self.lower_instanceof(instanceof),
            Expression::TypeCast(cast) => self.lower_type_cast(cast),
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

    /// Emit a compile-time constant value as an IR instruction
    /// Used for constant folding - inlines the constant directly
    fn emit_constant_value(&mut self, const_val: &ConstantValue) -> Register {
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
            }
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
            }
            return dest;
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

        // Unknown variable - could be a global or error
        // For now, return a null placeholder
        self.lower_null_literal()
    }

    fn lower_binary(&mut self, binary: &ast::BinaryExpression) -> Register {
        let left = self.lower_expr(&binary.left);
        let right = self.lower_expr(&binary.right);

        let op = self.convert_binary_op(&binary.operator);
        let result_ty = self.infer_binary_result_type(&op, &left, &right);
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
            ast::UnaryOperator::Void => {
                let _ = self.lower_expr(&unary.operand);
                let dest = self.alloc_register(TypeId::new(NULL_TYPE_ID));
                self.emit(IrInstr::Assign {
                    dest: dest.clone(),
                    value: IrValue::Constant(IrConstant::Null),
                });
                return dest;
            }
            ast::UnaryOperator::Delete => {
                let null_reg = self.lower_null_literal();
                match unary.operand.as_ref() {
                    Expression::Member(member) => {
                        let prop_name = self.interner.resolve(member.property.name);

                        // Static field delete: ClassName.field
                        if let Expression::Identifier(ident) = &*member.object {
                            if let Some(&class_id) = self.class_map.get(&ident.name) {
                                let global_index =
                                    self.class_info_map.get(&class_id).and_then(|info| {
                                        info.static_fields
                                            .iter()
                                            .find(|f| self.interner.resolve(f.name) == prop_name)
                                            .map(|sf| sf.global_index)
                                    });
                                if let Some(index) = global_index {
                                    self.emit(IrInstr::StoreGlobal {
                                        index,
                                        value: null_reg.clone(),
                                    });
                                } else {
                                    let object = self.lower_expr(&member.object);
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
                                        args: vec![object, prop_reg, null_reg.clone()],
                                        });
                                }
                            } else {
                                let class_id = self.infer_class_id(&member.object);
                                let object = self.lower_expr(&member.object);
                                let field_index = if let Some(class_id) = class_id {
                                    self.get_all_fields(class_id)
                                        .iter()
                                        .rev()
                                        .find(|f| self.interner.resolve(f.name) == prop_name)
                                        .map(|f| f.index)
                                } else {
                                    self.variable_object_fields
                                        .get(&ident.name)
                                        .and_then(|fields| {
                                            fields
                                                .iter()
                                                .find(|(name, _)| name == prop_name)
                                                .map(|(_, idx)| *idx as u16)
                                        })
                                };

                                if let Some(field) = field_index {
                                    self.emit(IrInstr::StoreField {
                                        object,
                                        field,
                                        value: null_reg.clone(),
                                    });
                                } else {
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
                                        args: vec![object, prop_reg, null_reg.clone()],
                                    });
                                }
                            }
                        } else {
                            let class_id = self.infer_class_id(&member.object);
                            let object = self.lower_expr(&member.object);
                            if let Some(field) = class_id.and_then(|cid| {
                                self.get_all_fields(cid)
                                    .iter()
                                    .rev()
                                    .find(|f| self.interner.resolve(f.name) == prop_name)
                                    .map(|f| f.index)
                            }) {
                                self.emit(IrInstr::StoreField {
                                    object,
                                    field,
                                    value: null_reg.clone(),
                                });
                            } else {
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
                                    args: vec![object, prop_reg, null_reg.clone()],
                                });
                            }
                        }
                    }
                    Expression::Index(index) => {
                        let array = self.lower_expr(&index.object);
                        let idx = self.lower_expr(&index.index);
                        self.emit(IrInstr::StoreElement {
                            array,
                            index: idx,
                            value: null_reg.clone(),
                        });
                    }
                    _ => {
                        let _ = self.lower_expr(&unary.operand);
                    }
                }
                let dest = self.alloc_register(TypeId::new(BOOLEAN_TYPE_ID));
                self.emit(IrInstr::Assign {
                    dest: dest.clone(),
                    value: IrValue::Constant(IrConstant::Boolean(true)),
                });
                return dest;
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
            if let Some(current_class_id) = self.current_class {
                // Get parent class
                if let Some(parent_id) = self
                    .class_info_map
                    .get(&current_class_id)
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
                    }
                }
            }
            return dest;
        }

        // Handle super.method() call
        if let Expression::Member(member) = &*call.callee {
            if let Expression::Super(_) = &*member.object {
                let method_name_symbol = member.property.name;
                if let Some(current_class_id) = self.current_class {
                    // Get parent class
                    if let Some(parent_id) = self
                        .class_info_map
                        .get(&current_class_id)
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
                        if let Some(class_id) = self.infer_class_id(&target_member.object) {
                            if let Some(&slot) =
                                self.method_slot_map.get(&(class_id, target_member.property.name))
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

            // Check if it's a direct function call
            if let Some(&func_id) = self.function_map.get(&ident.name) {
                // Call-site specialization: only for generic functions with constrained type
                // parameters (e.g., T extends HasLength). Unconstrained generics are handled
                // correctly by the normal monomorphization pipeline.
                let effective_func_id = if call.type_args.is_some() {
                    let needs_specialization = self
                        .generic_function_asts
                        .get(&ident.name)
                        .map(|func_ast| {
                            func_ast
                                .type_params
                                .as_ref()
                                .is_some_and(|tps| tps.iter().any(|tp| tp.constraint.is_some()))
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
                }
                return dest;
            }

            // Otherwise, it might be a closure stored in a variable
            if let Some(&local_idx) = self.local_map.get(&ident.name) {
                // Load the closure from the local variable
                let closure_ty = self
                    .local_registers
                    .get(&local_idx)
                    .map(|r| r.ty)
                    .unwrap_or(TypeId::new(NUMBER_TYPE_ID));
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
                        self.emit(IrInstr::SpawnClosure {
                            dest: dest.clone(),
                            closure,
                            args,
                        });
                        return dest;
                    }
                }

                // Regular closure call
                self.emit(IrInstr::CallClosure {
                    dest: Some(dest.clone()),
                    closure,
                    args,
                });

                // Propagate return type for bound method calls
                if let Some(&(class_id, method_name)) = self.bound_method_vars.get(&ident.name) {
                    if let Some(&ret_ty) = self.method_return_type_map.get(&(class_id, method_name))
                    {
                        if ret_ty != UNRESOLVED {
                            dest.ty = ret_ty;
                        }
                    }
                }

                return dest;
            }

            // Check for closure stored in a module-level global variable
            if let Some(&global_idx) = self.module_var_globals.get(&ident.name) {
                let closure = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
                self.emit(IrInstr::LoadGlobal {
                    dest: closure.clone(),
                    index: global_idx,
                });
                if let Some(&func_id) = self.closure_globals.get(&global_idx) {
                    if self.async_closures.contains(&func_id) {
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
                if let Some(&(class_id, method_name)) = self.bound_method_vars.get(&ident.name) {
                    if let Some(&ret_ty) = self.method_return_type_map.get(&(class_id, method_name))
                    {
                        if ret_ty != UNRESOLVED {
                            dest.ty = ret_ty;
                        }
                    }
                }

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
                            "encode" => {
                                // JSON.encode<T>(value) -> for now, same as stringify
                                // TODO: Generate specialized encoder based on type T
                                self.emit(IrInstr::NativeCall {
                                    dest: Some(dest.clone()),
                                    native_id: JSON_STRINGIFY,
                                    args,
                                });
                                return dest;
                            }
                            "decode" => {
                                // JSON.decode<T>(json) -> typed decoder
                                // If type argument is provided, use specialized decoder

                                if let Some(type_args) = &call.type_args {
                                    if let Some(first_type) = type_args.first() {
                                        // Try to get field info from the type
                                        if let Some(field_info) =
                                            self.get_json_field_info(&first_type.ty)
                                        {
                                            // Generate specialized decode with field info
                                            return self.emit_json_decode_with_fields(
                                                dest.clone(),
                                                args,
                                                field_info,
                                            );
                                        }
                                    }
                                }

                                // Fallback to generic parse if no type info available
                                // Returns json type (TypeId 15) for duck typing support

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
            let is_promise_like = self.type_ctx.is_task_type(object_ty)
                || matches!(
                    self.type_ctx.get(object_ty),
                    Some(crate::parser::types::Type::Class(class)) if class.name == "Promise"
                );
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

            // Check if this is a static method call (e.g., Utils.double(21))
            if let Expression::Identifier(ident) = &*member.object {
                if let Some(&class_id) = self.class_map.get(&ident.name) {
                    // This is a class identifier, check for static methods
                    if let Some(&func_id) =
                        self.static_method_map.get(&(class_id, method_name_symbol))
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
                        }
                        return dest;
                    }
                }
            }

            // Try to determine the class type of the object for method resolution
            let mut class_id = self.infer_class_id(&member.object);

            // If class_id is not found, check if this is a Channel type parameter
            // Parameters with Channel<T> type annotation aren't in variable_class_map
            if class_id.is_none() {
                if let Expression::Identifier(ident) = &*member.object {
                    // Check if this identifier is a local variable with Channel type
                    if let Some(&local_idx) = self.local_map.get(&ident.name) {
                        if let Some(reg) = self.local_registers.get(&local_idx) {
                            if reg.ty.as_u32() == CHANNEL_TYPE_ID {
                                // This is a Channel type - look up Channel class by finding it in class_map
                                for (&sym, &cid) in &self.class_map {
                                    if self.interner.resolve(sym) == TC::CHANNEL_TYPE_NAME {
                                        class_id = Some(cid);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Skip class dispatch for builtin primitive types — their methods
            // are dispatched via the type registry (native calls / class methods)
            if let Some(cid) = class_id {
                let is_builtin = self.class_map.iter().any(|(&sym, &id)| {
                    id == cid
                        && matches!(
                            self.interner.resolve(sym),
                            "string" | "number" | "Array" | "RegExp"
                        )
                });
                if is_builtin {
                    class_id = None;
                }
            }

            // Check if this is a user-defined class method (including inherited methods)
            if let Some(class_id) = class_id {
                // Check if this is a function-typed FIELD (not a method)
                // Fields should be loaded via GetField + CallClosure, not CallMethod
                let all_fields = self.get_all_fields(class_id);
                let is_field = all_fields
                    .iter()
                    .any(|f| self.interner.resolve(f.name) == method_name);
                let is_method = self.find_method(class_id, method_name_symbol).is_some();

                if is_field && !is_method {
                    // Function-typed field: emit GetField + CallClosure
                    let object = self.lower_expr(&member.object);
                    let field_info = all_fields
                        .iter()
                        .rev()
                        .find(|f| self.interner.resolve(f.name) == method_name)
                        .unwrap();
                    let field_reg = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
                    self.emit(IrInstr::LoadField {
                        dest: field_reg.clone(),
                        object,
                        field: field_info.index,
                        optional: member.optional,
                    });
                    self.emit(IrInstr::CallClosure {
                        dest: Some(dest.clone()),
                        closure: field_reg,
                        args,
                    });
                    return dest;
                }

                if let Some(func_id) = self.find_method(class_id, method_name_symbol) {
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
                    if let Some(&slot) = self.method_slot_map.get(&(class_id, method_name_symbol)) {
                        self.emit(IrInstr::CallMethod {
                            dest: Some(dest.clone()),
                            object,
                            method: slot,
                            args,
                            optional: member.optional,
                        });
                    } else {
                        // Fallback: static call (shouldn't happen for instance methods)
                        let mut method_args = vec![object];
                        method_args.extend(args);
                        self.emit(IrInstr::Call {
                            dest: Some(dest.clone()),
                            func: func_id,
                            args: method_args,
                        });
                    }

                    // Preserve precise method return typing for user-defined class methods.
                    // This is especially important when checker call typing is unresolved
                    // (e.g. precompiled stdlib class dispatch), so downstream property/method
                    // access can still select typed opcodes (ArrayLen/StringLen/etc).
                    if dest.ty.as_u32() == super::UNRESOLVED_TYPE_ID {
                        if let Some(&ret_ty) = self
                            .method_return_type_map
                            .get(&(class_id, method_name_symbol))
                        {
                            if ret_ty.as_u32() != super::UNRESOLVED_TYPE_ID {
                                dest.ty = ret_ty;
                            }
                        }
                    }

                    // Propagate generic return type for Map/Set methods
                    self.propagate_container_return_type(
                        &mut dest,
                        class_id,
                        method_name,
                        &member.object,
                    );

                    return dest;
                } else if let Some(&slot) =
                    self.method_slot_map.get(&(class_id, method_name_symbol))
                {
                    // Abstract method with vtable slot - use virtual dispatch.
                    // The actual implementation is provided by a derived class.
                    let object = self.lower_expr(&member.object);
                    self.emit(IrInstr::CallMethod {
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
                            .get(&(class_id, method_name_symbol))
                        {
                            if ret_ty.as_u32() != super::UNRESOLVED_TYPE_ID {
                                dest.ty = ret_ty;
                            }
                        }
                    }
                    return dest;
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
                            if obj_type_id == STRING_TYPE_ID
                                && !args.is_empty()
                                && args[0].ty.as_u32() == REGEXP_TYPE_ID
                            {
                                use crate::vm::builtin::string as bs;
                                match method_name {
                                    "replace" => id = bs::REPLACE_REGEXP,
                                    "split" => id = bs::SPLIT_REGEXP,
                                    _ => {}
                                }
                            }
                            id
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
                        crate::compiler::type_registry::DispatchAction::Opcode(_) => 0, // Properties handled above
                    }
                } else {
                    0 // Not in registry — fall through to vtable dispatch at runtime
                }
            } else {
                0 // UNRESOLVED type — fall through to vtable dispatch at runtime
            };

            self.emit(IrInstr::CallMethod {
                dest: Some(dest.clone()),
                object,
                method: method_id,
                args,
                optional: member.optional,
            });

            // Propagate return type for builtin methods so subsequent operations
            // use the correct typed opcodes (e.g., Iadd vs Fadd, Seq vs Feq).
            // Return types are extracted from .raya builtin file method signatures.
            if method_id != 0 {
                if let Some(ret_type) = self.type_registry.lookup_return_type(method_id) {
                    dest.ty = TypeId::new(ret_type);
                }
            }

            return dest;
        }

        // Fallback: callee is an expression (e.g., (getFunc())())
        // Lower the callee as an expression, then call it as a closure
        let closure = self.lower_expr(&call.callee);
        self.emit(IrInstr::CallClosure {
            dest: Some(dest.clone()),
            closure,
            args,
        });
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
            if let Some(&class_id) = self.class_map.get(&ident.name) {
                // This is a class identifier, check for static fields
                // Extract global_index first to avoid borrow conflict
                let global_index = self.class_info_map.get(&class_id).and_then(|class_info| {
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

        // Try to determine the class type of the object for field resolution
        let class_id = self.infer_class_id(&member.object);

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

        // Check for JSON type - use duck typing with dynamic property access
        if obj_ty_id == JSON_TYPE_ID {
            let json_type = TypeId::new(JSON_TYPE_ID);
            let dest = self.alloc_register(json_type);
            self.emit(IrInstr::JsonLoadProperty {
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
                    crate::compiler::type_registry::DispatchAction::NativeCall(_method_id) => {
                        // Properties shouldn't use NativeCall, but handle for completeness
                    }
                    crate::compiler::type_registry::DispatchAction::ClassMethod(_, _) => {
                        // Properties shouldn't use ClassMethod
                    }
                }
            }
        }

        // Look up field index and type by name if we know the class type (including inherited fields)
        let (field_index, field_ty) = if let Some(class_id) = class_id {
            // Get all fields including parent fields
            let all_fields = self.get_all_fields(class_id);
            // Use .rev() so child fields shadow parent fields with the same name
                if let Some(field) = all_fields
                    .iter()
                    .rev()
                    .find(|f| self.interner.resolve(f.name) == prop_name)
                {
                    (field.index, field.ty)
                } else {
                    // Field not found — check if it's a method (bound method extraction)
                    if let Some(&slot) = self.method_slot_map.get(&(class_id, member.property.name)) {
                        if self.js_this_binding_compat {
                            if let Some(func_id) = self.find_method(class_id, member.property.name) {
                                let dest = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
                                self.emit(IrInstr::MakeClosure {
                                    dest: dest.clone(),
                                    func: func_id,
                                    captures: vec![],
                                });
                                return dest;
                            }
                        }
                        let dest = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
                        self.emit(IrInstr::BindMethod {
                            dest: dest.clone(),
                            object,
                            method: slot,
                        });
                        return dest;
                    }
                    // Not a field or method — fall through to the non-class path below.
                    (0, UNRESOLVED)
                }
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
                _ => None,
            };

            if let Some(idx) = obj_field_idx {
                (idx, UNRESOLVED)
            } else {
                // Fall back to type-based field resolution (for function parameters typed as object types)
                let expr_ty = self.get_expr_type(&member.object);
                let type_field_idx = self.type_ctx.get(expr_ty).and_then(|ty| {
                    if let crate::parser::types::ty::Type::Object(obj) = ty {
                        obj.properties.iter().enumerate().find_map(|(i, p)| {
                            if p.name == prop_name {
                                Some((i as u16, p.ty))
                            } else {
                                None
                            }
                        })
                    } else if let crate::parser::types::ty::Type::Union(union) = ty {
                        // Search union members for the property
                        for &member_id in &union.members {
                            if let Some(crate::parser::types::ty::Type::Object(obj)) =
                                self.type_ctx.get(member_id)
                            {
                                if let Some(result) =
                                    obj.properties.iter().enumerate().find_map(|(i, p)| {
                                        if p.name == prop_name {
                                            Some((i as u16, p.ty))
                                        } else {
                                            None
                                        }
                                    })
                                {
                                    return Some(result);
                                }
                            }
                        }
                        None
                    } else {
                        None
                    }
                });
                type_field_idx.unwrap_or((0, UNRESOLVED))
            }
        };

        // Check if the object is a TypeVar (generic parameter) — emit LateBoundMember
        // so the post-monomorphization pass can resolve to the correct opcode.
        if obj_ty_id == UNRESOLVED_TYPE_ID && class_id.is_none() {
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

        // Fall back to field access for objects
        let dest = self.alloc_register(field_ty);
        self.emit(IrInstr::LoadField {
            dest: dest.clone(),
            object,
            field: field_index,
            optional: member.optional,
        });
        dest
    }

    fn lower_index(&mut self, index: &ast::IndexExpression, full_expr: &Expression) -> Register {
        let array = self.lower_expr(&index.object);
        let idx = self.lower_expr(&index.index);
        let elem_ty = self.get_expr_type(full_expr);
        let dest = self.alloc_register(elem_ty);

        self.emit(IrInstr::LoadElement {
            dest: dest.clone(),
            array,
            index: idx,
        });
        dest
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
            let dest = self.alloc_register(array_ty);
            self.emit(IrInstr::ArrayLiteral {
                dest: dest.clone(),
                elements,
                elem_ty,
            });
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

    fn spread_source_fields_from_type(&self, ty: TypeId) -> Option<Vec<(String, u16)>> {
        match self.type_ctx.get(ty)? {
            crate::parser::types::Type::Object(obj) => Some(
                obj.properties
                    .iter()
                    .enumerate()
                    .map(|(i, p)| (p.name.clone(), i as u16))
                    .collect(),
            ),
            crate::parser::types::Type::Class(_) => {
                let class_id = self.class_id_from_type_id(ty)?;
                Some(
                    self.get_all_fields(class_id)
                        .into_iter()
                        .map(|f| (self.interner.resolve(f.name).to_string(), f.index))
                        .collect(),
                )
            }
            crate::parser::types::Type::TypeVar(tv) => tv
                .constraint
                .and_then(|constraint| self.spread_source_fields_from_type(constraint)),
            _ => None,
        }
    }

    fn resolve_spread_source_fields(
        &self,
        spread_expr: &ast::Expression,
        spread_reg: Option<&Register>,
    ) -> Option<Vec<(String, u16)>> {
        if let Some(reg) = spread_reg {
            if let Some(fields) = self.register_object_fields.get(&reg.id) {
                let mut ordered: Vec<(String, u16)> = fields
                    .iter()
                    .map(|(name, idx)| (name.clone(), *idx as u16))
                    .collect();
                ordered.sort_by_key(|(_, idx)| *idx);
                return Some(ordered);
            }
        }

        if let ast::Expression::Identifier(ident) = spread_expr {
            if let Some(fields) = self.variable_object_fields.get(&ident.name) {
                let mut ordered: Vec<(String, u16)> = fields
                    .iter()
                    .map(|(name, idx)| (name.clone(), *idx as u16))
                    .collect();
                ordered.sort_by_key(|(_, idx)| *idx);
                return Some(ordered);
            }
        }

        if let Some(class_id) = self.infer_class_id(spread_expr) {
            return Some(
                self.get_all_fields(class_id)
                    .into_iter()
                    .map(|f| (self.interner.resolve(f.name).to_string(), f.index))
                    .collect(),
            );
        }

        let spread_ty = self.get_expr_type(spread_expr);
        self.spread_source_fields_from_type(spread_ty)
    }

    fn lower_object(&mut self, object: &ast::ObjectExpression) -> Register {
        let dest = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
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
                        for (field_name, _) in source_fields {
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

        let null_value = self.lower_null_literal();
        let initial_fields: Vec<(u16, Register)> = (0..field_names.len())
            .map(|i| (i as u16, null_value.clone()))
            .collect();

        self.emit(IrInstr::ObjectLiteral {
            dest: dest.clone(),
            class: ClassId::new(0),
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
                    self.emit(IrInstr::StoreField {
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
                    for (field_name, src_field_idx) in source_fields {
                        if !self.include_spread_field(&field_name) {
                            continue;
                        }
                        let Some(&dest_idx) = field_index_map.get(&field_name) else {
                            continue;
                        };
                        let field_val = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
                        self.emit(IrInstr::LoadField {
                            dest: field_val.clone(),
                            object: spread_reg.clone(),
                            field: src_field_idx,
                            optional: false,
                        });
                        self.emit(IrInstr::StoreField {
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
                    if let Some(&local_idx) = self.local_map.get(&ident.name) {
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
                        // Module-level variable — store via global slot
                        self.emit(IrInstr::StoreGlobal {
                            index: global_idx,
                            value: rhs,
                        });
                    } else if let Some(idx) =
                        self.captures.iter().position(|c| c.symbol == ident.name)
                    {
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
                            self.emit(IrInstr::StoreGlobal {
                                index: global_idx,
                                value: rhs,
                            });
                        }
                    }
                }
                Expression::Member(member) => {
                    let prop_name = self.interner.resolve(member.property.name);
                    let class_id = self.infer_class_id(&member.object);
                    let object = self.lower_expr(&member.object);
                    if let Some(class_id) = class_id {
                        if let Some(field) = self
                            .get_all_fields(class_id)
                            .iter()
                            .rev()
                            .find(|f| self.interner.resolve(f.name) == prop_name)
                        {
                            self.emit(IrInstr::StoreField {
                                object,
                                field: field.index,
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
                    let array = self.lower_expr(&index.object);
                    if self.is_append_index_pattern(&index.object, &index.index) {
                        self.emit(IrInstr::ArrayPush {
                            array,
                            element: rhs,
                        });
                    } else {
                        let idx = self.lower_expr(&index.index);
                        self.emit(IrInstr::StoreElement {
                            array,
                            index: idx,
                            value: rhs,
                        });
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

        match &*assign.left {
            Expression::Identifier(ident) => {
                if let Some(&local_idx) = self.local_map.get(&ident.name) {
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
                        // Module-level variable inside arrow — store via global slot
                        self.emit(IrInstr::StoreGlobal {
                            index: global_idx,
                            value: value.clone(),
                        });
                    }
                } else if let Some(&global_idx) = self.module_var_globals.get(&ident.name) {
                    // Module-level variable — store via global slot
                    self.emit(IrInstr::StoreGlobal {
                        index: global_idx,
                        value: value.clone(),
                    });
                }
            }
            Expression::Member(member) => {
                let prop_name = self.interner.resolve(member.property.name);

                // Check for static field write: ClassName.staticField = value
                if let Expression::Identifier(ident) = &*member.object {
                    if let Some(&class_id) = self.class_map.get(&ident.name) {
                        let global_index = self.class_info_map.get(&class_id).and_then(|info| {
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

                // Instance field write
                let class_id = self.infer_class_id(&member.object);
                let object = self.lower_expr(&member.object);
                if let Some(class_id) = class_id {
                    if let Some(field) = self
                        .get_all_fields(class_id)
                        .iter()
                        .rev()
                        .find(|f| self.interner.resolve(f.name) == prop_name)
                    {
                        self.emit(IrInstr::StoreField {
                            object,
                            field: field.index,
                            value: value.clone(),
                        });
                    } else {
                        // Dynamic fallback for unresolved field names (e.g. `any`/monkeypatch flows).
                        let prop_reg = self.alloc_register(TypeId::new(STRING_TYPE_ID));
                        self.emit(IrInstr::Assign {
                            dest: prop_reg.clone(),
                            value: IrValue::Constant(IrConstant::String(prop_name.to_string())),
                        });
                        self.emit(IrInstr::NativeCall {
                            dest: None,
                            native_id: crate::compiler::native_id::REFLECT_SET,
                            args: vec![object, prop_reg, value.clone()],
                        });
                    }
                } else {
                    // Dynamic fallback for non-class/static-unknown object writes.
                    let prop_reg = self.alloc_register(TypeId::new(STRING_TYPE_ID));
                    self.emit(IrInstr::Assign {
                        dest: prop_reg.clone(),
                        value: IrValue::Constant(IrConstant::String(prop_name.to_string())),
                    });
                    self.emit(IrInstr::NativeCall {
                        dest: None,
                        native_id: crate::compiler::native_id::REFLECT_SET,
                        args: vec![object, prop_reg, value.clone()],
                    });
                }
            }
            Expression::Index(index) => {
                let array = self.lower_expr(&index.object);
                if self.is_append_index_pattern(&index.object, &index.index) {
                    self.emit(IrInstr::ArrayPush {
                        array,
                        element: value.clone(),
                    });
                } else {
                    let idx = self.lower_expr(&index.index);
                    self.emit(IrInstr::StoreElement {
                        array,
                        index: idx,
                        value: value.clone(),
                    });
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

        dest
    }

    pub(super) fn lower_arrow(&mut self, arrow: &ast::ArrowFunction) -> Register {
        // Generate unique name for the arrow function
        let arrow_name = format!("__arrow_{}", self.arrow_counter);
        self.arrow_counter += 1;

        // Allocate function ID for the arrow
        let func_id = crate::ir::FunctionId::new(self.next_function_id);
        self.next_function_id += 1;

        // Track async closures for SpawnClosure emission
        if arrow.is_async {
            self.async_closures.insert(func_id);
        }

        // Save current lowerer state
        let saved_register = self.next_register;
        let saved_block = self.next_block;
        let saved_local_map = self.local_map.clone();
        let saved_local_registers = self.local_registers.clone();
        let saved_refcell_registers = self.refcell_registers.clone();
        let saved_next_local = self.next_local;
        let saved_function = self.current_function.take();
        let saved_current_block = self.current_block;
        let saved_ancestor_variables = self.ancestor_variables.take();
        let saved_captures = std::mem::take(&mut self.captures);
        let saved_next_capture_slot = self.next_capture_slot;
        let saved_this_register = self.this_register.take();
        let saved_this_ancestor_info = self.this_ancestor_info.take();
        let saved_this_captured_idx = self.this_captured_idx.take();

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

        for param in &arrow.params {
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

                // Track class type for parameters with class type annotations
                // so method calls can be statically resolved
                if let Some(type_ann) = &param.type_annotation {
                    if let Some(class_id) = self.try_extract_class_from_type(type_ann) {
                        self.variable_class_map.insert(ident.name, class_id);
                    }
                }
            } else {
                // Destructuring pattern: track for later binding after entry block
                destructure_params.push((params.len(), &param.pattern, reg.clone()));
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
        self.refcell_registers = saved_refcell_registers;
        self.next_local = saved_next_local;
        self.current_function = saved_function;
        self.current_block = saved_current_block;
        self.ancestor_variables = saved_ancestor_variables;
        self.captures = saved_captures;
        self.next_capture_slot = saved_next_capture_slot;
        self.this_register = saved_this_register;
        self.this_ancestor_info = saved_this_ancestor_info;
        self.this_captured_idx = saved_this_captured_idx;

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
        let dest = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));

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
                // new RegExp(pattern, flags?) -> NativeCall(0x0A00)
                // Use TypeId 8 for RegExp
                let regexp_dest = self.alloc_register(TypeId::new(REGEXP_TYPE_ID));
                let mut args = Vec::new();
                for arg in &new_expr.arguments {
                    args.push(self.lower_expr(arg));
                }
                // If flags not provided, pass empty string
                if args.len() == 1 {
                    let empty_flags = self.alloc_register(TypeId::new(STRING_TYPE_ID)); // String type
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

            // Look up class ID from class_map
            if let Some(&class_id) = self.class_map.get(&ident.name) {
                // Create the object
                self.emit(IrInstr::NewObject {
                    dest: dest.clone(),
                    class: class_id,
                });

                // Initialize all fields (including inherited parent fields) with default values
                let all_fields = self.get_all_fields(class_id);
                for field in &all_fields {
                    if let Some(ref init_expr) = field.initializer {
                        // Lower the initializer expression
                        let value = self.lower_expr(init_expr);
                        // Store it to the field
                        self.emit(IrInstr::StoreField {
                            object: dest.clone(),
                            field: field.index,
                            value,
                        });
                    }
                }

                let constructor_func_id = self
                    .class_info_map
                    .get(&class_id)
                    .and_then(|info| info.constructor);

                // Call the constructor if one exists
                if let Some(ctor_func_id) = constructor_func_id {
                    // Get constructor parameter info for default values
                    let ctor_params = self
                        .class_info_map
                        .get(&class_id)
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

                    // Call the constructor (it doesn't return a value we care about)
                    self.emit(IrInstr::Call {
                        dest: None, // Constructor return value is discarded
                        func: ctor_func_id,
                        args,
                    });
                }

                return dest;
            }
        }

        // Unknown class or not an identifier - emit NewObject with class ID 0 as fallback
        self.emit(IrInstr::NewObject {
            dest: dest.clone(),
            class: crate::ir::ClassId::new(0),
        });

        dest
    }

    fn lower_await(&mut self, await_expr: &ast::AwaitExpression) -> Register {
        // Check if the argument is an array literal (await [task1, task2, ...])
        if let Expression::Array(arr) = &*await_expr.argument {
            // Lower all elements (each should be a Task)
            // We only handle simple expressions (no spread, no holes)
            let elements: Vec<Register> = arr
                .elements
                .iter()
                .filter_map(|e| {
                    match e {
                        Some(ast::ArrayElement::Expression(expr)) => Some(self.lower_expr(expr)),
                        _ => None, // Skip spread elements and holes for now
                    }
                })
                .collect();

            // Create the array of tasks - Task IDs are stored as u64 values
            let task_ty = TypeId::new(TASK_TYPE_ID); // For type tracking
            let tasks_array = self.alloc_register(task_ty);
            self.emit(IrInstr::ArrayLiteral {
                dest: tasks_array.clone(),
                elements,
                elem_ty: task_ty, // Element type is Task (u64 internally)
            });

            // Emit await_all instruction
            // Result is an array (use generic ARRAY_TYPE_ID)
            let dest = self.alloc_register(TypeId::new(super::ARRAY_TYPE_ID));
            self.emit(IrInstr::AwaitAll {
                dest: dest.clone(),
                tasks: tasks_array,
            });
            return dest;
        }

        // Lower the awaited expression
        let task_or_array = self.lower_expr(&await_expr.argument);

        // Check if the expression type is an array - if so, use AwaitAll
        let expr_type = self.get_expr_type(&await_expr.argument);
        if matches!(
            self.type_ctx.get(expr_type),
            Some(crate::parser::types::ty::Type::Array(_))
        ) {
            // Awaiting an array variable - emit AwaitAll
            // Result is an array (use generic ARRAY_TYPE_ID)
            let dest = self.alloc_register(TypeId::new(super::ARRAY_TYPE_ID));
            self.emit(IrInstr::AwaitAll {
                dest: dest.clone(),
                tasks: task_or_array,
            });
            return dest;
        }

        // Extract the result type from Task<T>
        let result_type = if let Some(crate::parser::types::ty::Type::Task(task_ty)) =
            self.type_ctx.get(expr_type)
        {
            task_ty.result
        } else {
            TypeId::new(NUMBER_TYPE_ID) // Fallback to number if not a Task type
        };

        // Emit await instruction for single task
        let dest = self.alloc_register(result_type);
        self.emit(IrInstr::Await {
            dest: dest.clone(),
            task: task_or_array,
        });
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
                let closure_ty = self
                    .local_registers
                    .get(&local_idx)
                    .map(|r| r.ty)
                    .unwrap_or(TypeId::new(NUMBER_TYPE_ID));
                let closure = self.alloc_register(closure_ty);
                self.emit(IrInstr::LoadLocal {
                    dest: closure.clone(),
                    index: local_idx,
                });

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

            // Lower the object
            let object = self.lower_expr(&member.object);

            // Check if it's a static method call
            if let Expression::Identifier(ident) = &*member.object {
                if let Some(&class_id) = self.class_map.get(&ident.name) {
                    if let Some(&func_id) =
                        self.static_method_map.get(&(class_id, method_name_symbol))
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
            let class_id = self.infer_class_id(&member.object);
            if let Some(class_id) = class_id {
                if let Some(func_id) = self.find_method(class_id, method_name_symbol) {
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
        }

        // Fallback: treat callee as a closure/expression and spawn it
        let callee_reg = self.lower_expr(&async_call.callee);
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

        // If not in a method context and no ancestor has `this`, return null
        self.lower_null_literal()
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

        // Resolve the class ID from the type annotation
        let class_id = self
            .resolve_class_from_type(&instanceof.type_name)
            .unwrap_or(ClassId::new(0));

        // Allocate register for boolean result
        let dest = self.alloc_register(TypeId::new(BOOLEAN_TYPE_ID)); // Boolean type

        self.emit(IrInstr::InstanceOf {
            dest: dest.clone(),
            object,
            class_id,
        });

        dest
    }

    /// Lower type cast expression: expr as TypeName
    fn lower_type_cast(&mut self, cast: &ast::TypeCastExpression) -> Register {
        // Lower the object expression
        let object = self.lower_expr(&cast.object);

        // Allocate register for the casted value. Type checker treats this expression
        // as target type; lowering keeps runtime value flow here.
        let dest = self.alloc_register(TypeId::new(UNKNOWN_TYPE_ID));

        // Runtime cast checks are currently supported for class-reference targets.
        // Non-class targets use compile-time cast typing only.
        if let Some(class_id) = self.resolve_class_from_type(&cast.target_type) {
            self.emit(IrInstr::Cast {
                dest: dest.clone(),
                object,
                class_id,
            });
        } else if let Some(class_id) = self.resolve_nullable_class_from_union(&cast.target_type) {
            self.emit(IrInstr::Cast {
                dest: dest.clone(),
                object,
                class_id,
            });
        } else if let Some(tuple_len_encoded) = self.resolve_runtime_tuple_len_cast_target(&cast.target_type) {
            self.emit(IrInstr::Cast {
                dest: dest.clone(),
                object,
                class_id: ClassId::new(tuple_len_encoded as u32),
            });
        } else if let Some(object_min_fields_encoded) =
            self.resolve_runtime_object_min_fields_cast_target(&cast.target_type)
        {
            self.emit(IrInstr::Cast {
                dest: dest.clone(),
                object,
                class_id: ClassId::new(object_min_fields_encoded as u32),
            });
        } else if let Some(array_elem_kind_encoded) =
            self.resolve_runtime_array_element_kind_cast_target(&cast.target_type)
        {
            self.emit(IrInstr::Cast {
                dest: dest.clone(),
                object,
                class_id: ClassId::new(array_elem_kind_encoded as u32),
            });
        } else if let Some(kind_mask) = self.resolve_runtime_cast_kind_mask(&cast.target_type) {
            self.emit(IrInstr::Cast {
                dest: dest.clone(),
                object,
                class_id: ClassId::new((CAST_KIND_MASK_FLAG | kind_mask) as u32),
            });
        } else {
            self.emit(IrInstr::Assign {
                dest: dest.clone(),
                value: IrValue::Register(object),
            });
        }

        dest
    }

    /// Resolve a class runtime cast target from a type annotation.
    /// Returns None for non-class targets (primitive/union/object/etc).
    fn resolve_class_from_type(&self, type_ann: &ast::TypeAnnotation) -> Option<ClassId> {
        use crate::parser::ast::types::Type;

        match &type_ann.ty {
            Type::Reference(type_ref) => {
                // Look up the class by name
                if let Some(&class_id) = self.class_map.get(&type_ref.name.name) {
                    return Some(class_id);
                }
                None
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
                if combined == 0 { None } else { Some(combined) }
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

    fn resolve_nullable_class_from_union(&self, type_ann: &ast::TypeAnnotation) -> Option<ClassId> {
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
    fn find_method(&self, class_id: ClassId, method_name: Symbol) -> Option<FunctionId> {
        // First check this class
        if let Some(&func_id) = self.method_map.get(&(class_id, method_name)) {
            return Some(func_id);
        }

        // Check parent class recursively
        if let Some(parent_id) = self
            .class_info_map
            .get(&class_id)
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
        class_id: ClassId,
        method_name: &str,
        object_expr: &Expression,
    ) {
        // Check if class is Map or Set
        let class_name = self
            .class_map
            .iter()
            .find(|(&_sym, &id)| id == class_id)
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

    /// Look up the generic value type for a Map/Set field expression.
    /// For `this.adj` where `adj: Map<K, V>`, returns V's TypeId.
    fn get_container_value_type(&self, expr: &Expression) -> Option<TypeId> {
        if let Expression::Member(member) = expr {
            let obj_class_id = self.infer_class_id(&member.object)?;
            let field_name = self.interner.resolve(member.property.name);
            let all_fields = self.get_all_fields(obj_class_id);
            for field in all_fields.into_iter().rev() {
                if self.interner.resolve(field.name) == field_name {
                    return field.value_type;
                }
            }
        }
        None
    }

    /// When a child extends a generic parent (e.g., `extends Base<string>`),
    /// parent field types are substituted with concrete type arguments.
    pub(super) fn get_all_fields(&self, class_id: ClassId) -> Vec<ClassFieldInfo> {
        let mut all_fields = Vec::new();

        if let Some(class_info) = self.class_info_map.get(&class_id) {
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
                all_fields.extend(parent_fields);
            }
            // Then add this class's fields
            all_fields.extend(class_info.fields.clone());
        }

        all_fields
    }

    /// Infer the class ID of an expression (for method call resolution)
    pub(super) fn infer_class_id(&self, expr: &Expression) -> Option<ClassId> {
        match expr {
            // 'this' uses current class context
            Expression::This(_) => self.current_class,
            // Variable lookup
            Expression::Identifier(ident) => self
                .variable_class_map
                .get(&ident.name)
                .copied()
                .or_else(|| self.class_id_from_type_id(self.get_expr_type(expr))),
            // Field access: look up the field's type in the class definition
            Expression::Member(member) => {
                // Get the class of the object
                let obj_class_id = self.infer_class_id(&member.object)?;
                // Look up the field type
                let field_name = self.interner.resolve(member.property.name);
                let all_fields = self.get_all_fields(obj_class_id);
                for field in all_fields.into_iter().rev() {
                    let fname = self.interner.resolve(field.name);
                    if fname == field_name {
                        // Check if the field has a known class type
                        if let Some(field_class_id) = field.class_type {
                            return Some(field_class_id);
                        }
                        // Otherwise, check if we have a type name we can look up
                        if let Some(ref type_name) = field.type_name {
                            // Look up the class by name
                            for (&sym, &cid) in &self.class_map {
                                if self.interner.resolve(sym) == type_name {
                                    return Some(cid);
                                }
                            }
                        }
                        break;
                    }
                }
                self.class_id_from_type_id(self.get_expr_type(expr))
            }
            // Method/function call: check if the call has a known return class type
            Expression::Call(call) => {
                if let Expression::Member(member) = &*call.callee {
                    let obj_class_id = self.infer_class_id(&member.object)?;
                    let method_name = member.property.name;
                    // Only return a class if there's an explicit return class mapping.
                    // Don't assume methods return the same class (e.g., Map.get() returns
                    // the value type, not Map).
                    if let Some(&ret_class_id) = self
                        .method_return_class_map
                        .get(&(obj_class_id, method_name))
                    {
                        return Some(ret_class_id);
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
                    if let Some(&(class_id, method_name)) = self.bound_method_vars.get(&ident.name)
                    {
                        if let Some(&ret_class_id) =
                            self.method_return_class_map.get(&(class_id, method_name))
                        {
                            return Some(ret_class_id);
                        }
                    }
                    // Check function return class
                    if let Some(&ret_class_id) = self.function_return_class_map.get(&ident.name) {
                        return Some(ret_class_id);
                    }
                }
                self.class_id_from_type_id(self.get_expr_type(expr))
            }
            // New expression: return the class being instantiated
            Expression::New(new_expr) => {
                if let Expression::Identifier(ident) = &*new_expr.callee {
                    return self.class_map.get(&ident.name).copied();
                }
                self.class_id_from_type_id(self.get_expr_type(expr))
            }
            // Index access over array/tuple containers can preserve the element class.
            // Important: do NOT return container class ID directly (e.g. Array), because
            // that misroutes primitive element calls to object/vtable dispatch.
            Expression::Index(index) => {
                let object_ty = self.get_expr_type(&index.object);
                if let Some(ty) = self.type_ctx.get(object_ty) {
                    match ty {
                        crate::parser::types::ty::Type::Array(arr) => {
                            return self.class_id_from_type_id(arr.element);
                        }
                        crate::parser::types::ty::Type::Tuple(tuple) => {
                            // Conservative: if all tuple members that are class-typed agree
                            // on one class, use it; otherwise keep unresolved.
                            let mut found: Option<ClassId> = None;
                            for member_ty in &tuple.elements {
                                if let Some(cid) = self.class_id_from_type_id(*member_ty) {
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
                            return self.infer_class_id(&index.object);
                        }
                    }
                }
                self.infer_class_id(&index.object)
            }
            _ => self.class_id_from_type_id(self.get_expr_type(expr)),
        }
    }

    pub(super) fn class_id_from_type_id(&self, ty_id: TypeId) -> Option<ClassId> {
        use crate::parser::types::ty::Type;

        let ty = self.type_ctx.get(ty_id)?;
        match ty {
            Type::Class(class_ty) => self.class_map.iter().find_map(|(&sym, &cid)| {
                if self.interner.resolve(sym) == class_ty.name {
                    Some(cid)
                } else {
                    None
                }
            }),
            Type::Reference(type_ref) => self.class_map.iter().find_map(|(&sym, &cid)| {
                if self.interner.resolve(sym) == type_ref.name {
                    Some(cid)
                } else {
                    None
                }
            }),
            Type::Generic(generic) => {
                if let Some(Type::JSObject) = self.type_ctx.get(generic.base) {
                    if let Some(&inner) = generic.type_args.first() {
                        return self.class_id_from_type_id(inner);
                    }
                }
                self.class_id_from_type_id(generic.base)
            }
            Type::TypeVar(tv) => tv
                .constraint
                .and_then(|constraint| self.class_id_from_type_id(constraint)),
            Type::Union(union) => {
                // Prefer the single concrete class member if present.
                let mut found: Option<ClassId> = None;
                for member in &union.members {
                    if let Some(cid) = self.class_id_from_type_id(*member) {
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
        let string_ty = TypeId::new(NULL_TYPE_ID); // string type

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
        self.emit(IrInstr::JsonLoadProperty {
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
        self.emit(IrInstr::ObjectLiteral {
            dest: dest.clone(),
            class: ClassId::new(0),
            fields,
        });

        // Register field layout for destructuring support
        self.register_object_fields.insert(dest.id, field_layout);

        dest
    }

    /// Lower JSX props that include spread attributes.
    ///
    /// Uses JsonStoreProperty to build the object incrementally,
    /// preserving attribute evaluation order.
    fn lower_jsx_props_with_spread(&mut self, attributes: &[ast::JsxAttribute]) -> Register {
        // Start with an empty object
        let dest = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
        self.emit(IrInstr::ObjectLiteral {
            dest: dest.clone(),
            class: ClassId::new(0),
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
                    self.emit(IrInstr::JsonStoreProperty {
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

        // Factory not found in scope — emit a CallClosure with a null callee
        // This will produce a runtime error, which is the expected behavior
        // when the factory function hasn't been imported/defined
        let null_reg = self.lower_null_literal();
        self.emit(IrInstr::CallClosure {
            dest: Some(dest.clone()),
            closure: null_reg,
            args,
        });
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
}
