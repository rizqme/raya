//! Expression Lowering
//!
//! Converts AST expressions to IR instructions.

use super::{ClassFieldInfo, ConstantValue, Lowerer};
use crate::compiler::ir::{BinaryOp, ClassId, FunctionId, IrConstant, IrInstr, IrValue, Register, UnaryOp};
use crate::parser::ast::{self, AssignmentOperator, Expression, TemplatePart};
use crate::parser::interner::Symbol;
use crate::parser::TypeId;

// ============================================================================
// Built-in Method IDs (must match raya-core/src/builtin.rs)
// ============================================================================

/// Built-in array method IDs (must match raya-core/src/builtin.rs)
mod builtin_array {
    pub const PUSH: u16 = 0x0100;
    pub const POP: u16 = 0x0101;
    pub const SHIFT: u16 = 0x0102;
    pub const UNSHIFT: u16 = 0x0103;
    pub const INDEX_OF: u16 = 0x0104;
    pub const INCLUDES: u16 = 0x0105;
    pub const SLICE: u16 = 0x0106;
    pub const CONCAT: u16 = 0x0107;
    pub const REVERSE: u16 = 0x0108;
    pub const JOIN: u16 = 0x0109;
    pub const FOR_EACH: u16 = 0x010A;
    pub const FILTER: u16 = 0x010B;
    pub const FIND: u16 = 0x010C;
    pub const FIND_INDEX: u16 = 0x010D;
    pub const EVERY: u16 = 0x010E;
    pub const SOME: u16 = 0x010F;
    pub const LAST_INDEX_OF: u16 = 0x0110;
    pub const SORT: u16 = 0x0111;
    pub const MAP: u16 = 0x0112;
    pub const REDUCE: u16 = 0x0113;
    pub const FILL: u16 = 0x0114;
    pub const FLAT: u16 = 0x0115;
}

/// Built-in string method IDs (must match raya-core/src/builtin.rs)
mod builtin_string {
    pub const CHAR_AT: u16 = 0x0200;
    pub const SUBSTRING: u16 = 0x0201;
    pub const TO_UPPER_CASE: u16 = 0x0202;
    pub const TO_LOWER_CASE: u16 = 0x0203;
    pub const TRIM: u16 = 0x0204;
    pub const INDEX_OF: u16 = 0x0205;
    pub const INCLUDES: u16 = 0x0206;
    pub const SPLIT: u16 = 0x0207;
    pub const STARTS_WITH: u16 = 0x0208;
    pub const ENDS_WITH: u16 = 0x0209;
    pub const REPLACE: u16 = 0x020A;
    pub const REPEAT: u16 = 0x020B;
    pub const PAD_START: u16 = 0x020C;
    pub const PAD_END: u16 = 0x020D;
    pub const CHAR_CODE_AT: u16 = 0x020E;
    pub const LAST_INDEX_OF: u16 = 0x020F;
    pub const TRIM_START: u16 = 0x0210;
    pub const TRIM_END: u16 = 0x0211;
    // String methods that take RegExp
    pub const MATCH: u16 = 0x0212;
    pub const MATCH_ALL: u16 = 0x0213;
    pub const SEARCH: u16 = 0x0214;
    pub const REPLACE_REGEXP: u16 = 0x0215;
    pub const SPLIT_REGEXP: u16 = 0x0216;
    pub const REPLACE_WITH_REGEXP: u16 = 0x0217;
}

/// Built-in Number method IDs (must match raya-core/src/builtin.rs)
mod builtin_number {
    pub const TO_FIXED: u16 = 0x0F00;
    pub const TO_PRECISION: u16 = 0x0F01;
    pub const TO_STRING_RADIX: u16 = 0x0F02;
}

/// Built-in RegExp method IDs (must match raya-core/src/builtin.rs)
mod builtin_regexp {
    pub const NEW: u16 = 0x0A00;
    pub const TEST: u16 = 0x0A01;
    pub const EXEC: u16 = 0x0A02;
    pub const EXEC_ALL: u16 = 0x0A03;
    pub const REPLACE: u16 = 0x0A04;
    pub const REPLACE_WITH: u16 = 0x0A05;
    pub const SPLIT: u16 = 0x0A06;
}

/// Look up built-in method ID by method name and object type
/// Pre-interned TypeIds: 0=Number, 1=String, 2=Boolean, 3=Null, 4=Void, 5=Never, 6=Unknown,
/// 7=Mutex, 8=RegExp, 9=Date, 10=Buffer, etc.
/// Array types are interned dynamically (TypeId >= 15 typically)
fn lookup_builtin_method(obj_type_id: u32, method_name: &str) -> Option<u16> {
    // Number type (TypeId 0)
    if obj_type_id == 0 {
        match method_name {
            "toFixed" => return Some(builtin_number::TO_FIXED),
            "toPrecision" => return Some(builtin_number::TO_PRECISION),
            "toString" => return Some(builtin_number::TO_STRING_RADIX),
            _ => {}
        }
    }

    // RegExp type (TypeId 8)
    if obj_type_id == 8 {
        match method_name {
            "test" => return Some(builtin_regexp::TEST),
            "exec" => return Some(builtin_regexp::EXEC),
            "execAll" => return Some(builtin_regexp::EXEC_ALL),
            "replace" => return Some(builtin_regexp::REPLACE),
            "replaceWith" => return Some(builtin_regexp::REPLACE_WITH),
            "split" => return Some(builtin_regexp::SPLIT),
            _ => {}
        }
    }

    // Array types (TypeId >= 7, but skip known non-array types) or unknown (TypeId 6)
    // Note: This is a heuristic - array types are interned dynamically
    if obj_type_id >= 7 || obj_type_id == 6 {
        match method_name {
            "push" => return Some(builtin_array::PUSH),
            "pop" => return Some(builtin_array::POP),
            "shift" => return Some(builtin_array::SHIFT),
            "unshift" => return Some(builtin_array::UNSHIFT),
            "indexOf" => return Some(builtin_array::INDEX_OF),
            "includes" => return Some(builtin_array::INCLUDES),
            "slice" => return Some(builtin_array::SLICE),
            "concat" => return Some(builtin_array::CONCAT),
            "join" => return Some(builtin_array::JOIN),
            "reverse" => return Some(builtin_array::REVERSE),
            "forEach" => return Some(builtin_array::FOR_EACH),
            "filter" => return Some(builtin_array::FILTER),
            "find" => return Some(builtin_array::FIND),
            "findIndex" => return Some(builtin_array::FIND_INDEX),
            "every" => return Some(builtin_array::EVERY),
            "some" => return Some(builtin_array::SOME),
            "lastIndexOf" => return Some(builtin_array::LAST_INDEX_OF),
            "sort" => return Some(builtin_array::SORT),
            "map" => return Some(builtin_array::MAP),
            "reduce" => return Some(builtin_array::REDUCE),
            "fill" => return Some(builtin_array::FILL),
            "flat" => return Some(builtin_array::FLAT),
            _ => {}
        }
    }

    // String type (TypeId 1)
    if obj_type_id == 1 {
        match method_name {
            "charAt" => return Some(builtin_string::CHAR_AT),
            "substring" => return Some(builtin_string::SUBSTRING),
            "toUpperCase" => return Some(builtin_string::TO_UPPER_CASE),
            "toLowerCase" => return Some(builtin_string::TO_LOWER_CASE),
            "trim" => return Some(builtin_string::TRIM),
            "indexOf" => return Some(builtin_string::INDEX_OF),
            "includes" => return Some(builtin_string::INCLUDES),
            "startsWith" => return Some(builtin_string::STARTS_WITH),
            "endsWith" => return Some(builtin_string::ENDS_WITH),
            "split" => return Some(builtin_string::SPLIT),
            "replace" => return Some(builtin_string::REPLACE),
            "repeat" => return Some(builtin_string::REPEAT),
            "padStart" => return Some(builtin_string::PAD_START),
            "padEnd" => return Some(builtin_string::PAD_END),
            "charCodeAt" => return Some(builtin_string::CHAR_CODE_AT),
            "lastIndexOf" => return Some(builtin_string::LAST_INDEX_OF),
            "trimStart" => return Some(builtin_string::TRIM_START),
            "trimEnd" => return Some(builtin_string::TRIM_END),
            "match" => return Some(builtin_string::MATCH),
            "matchAll" => return Some(builtin_string::MATCH_ALL),
            "search" => return Some(builtin_string::SEARCH),
            "replaceWith" => return Some(builtin_string::REPLACE_WITH_REGEXP),
            _ => {}
        }
    }

    None
}

impl<'a> Lowerer<'a> {
    /// Lower an expression, returning the register holding its value
    pub fn lower_expr(&mut self, expr: &Expression) -> Register {
        match expr {
            Expression::IntLiteral(lit) => self.lower_int_literal(lit),
            Expression::FloatLiteral(lit) => self.lower_float_literal(lit),
            Expression::StringLiteral(lit) => self.lower_string_literal(lit),
            Expression::BooleanLiteral(lit) => self.lower_bool_literal(lit),
            Expression::NullLiteral(_) => self.lower_null_literal(),
            Expression::Identifier(ident) => self.lower_identifier(ident),
            Expression::Binary(binary) => self.lower_binary(binary),
            Expression::Unary(unary) => self.lower_unary(unary),
            Expression::Call(call) => self.lower_call(call),
            Expression::Member(member) => self.lower_member(member),
            Expression::Index(index) => self.lower_index(index),
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
            _ => {
                // For unhandled expressions, emit a null placeholder
                self.lower_null_literal()
            }
        }
    }

    fn lower_int_literal(&mut self, lit: &ast::IntLiteral) -> Register {
        // Pre-interned TypeIds: 0=Number, 1=String, 2=Boolean, 3=Null, 4=Void, 5=Never, 6=Unknown
        let ty = TypeId::new(0); // Number type (integers are numbers in Raya)
        let dest = self.alloc_register(ty);
        self.emit(IrInstr::Assign {
            dest: dest.clone(),
            value: IrValue::Constant(IrConstant::I32(lit.value as i32)),
        });
        dest
    }

    fn lower_float_literal(&mut self, lit: &ast::FloatLiteral) -> Register {
        // Pre-interned TypeIds: 0=Number, 1=String, 2=Boolean, 3=Null, 4=Void, 5=Never, 6=Unknown
        let ty = TypeId::new(0); // Number type
        let dest = self.alloc_register(ty);
        self.emit(IrInstr::Assign {
            dest: dest.clone(),
            value: IrValue::Constant(IrConstant::F64(lit.value)),
        });
        dest
    }

    fn lower_string_literal(&mut self, lit: &ast::StringLiteral) -> Register {
        // Pre-interned TypeIds: 0=Number, 1=String, 2=Boolean, 3=Null, 4=Void, 5=Never, 6=Unknown
        let ty = TypeId::new(1); // String type
        let dest = self.alloc_register(ty);
        let string_value = self.interner.resolve(lit.value).to_string();
        self.emit(IrInstr::Assign {
            dest: dest.clone(),
            value: IrValue::Constant(IrConstant::String(string_value)),
        });
        dest
    }

    fn lower_bool_literal(&mut self, lit: &ast::BooleanLiteral) -> Register {
        // Pre-interned TypeIds: 0=Number, 1=String, 2=Boolean, 3=Null, 4=Void, 5=Never, 6=Unknown
        let ty = TypeId::new(2); // Boolean type
        let dest = self.alloc_register(ty);
        self.emit(IrInstr::Assign {
            dest: dest.clone(),
            value: IrValue::Constant(IrConstant::Boolean(lit.value)),
        });
        dest
    }

    pub(super) fn lower_null_literal(&mut self) -> Register {
        // Pre-interned TypeIds: 0=Number, 1=String, 2=Boolean, 3=Null, 4=Void, 5=Never, 6=Unknown
        let ty = TypeId::new(3); // Null type
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
                let ty = TypeId::new(0); // Number type
                let dest = self.alloc_register(ty);
                self.emit(IrInstr::Assign {
                    dest: dest.clone(),
                    value: IrValue::Constant(IrConstant::I32(*v as i32)),
                });
                dest
            }
            ConstantValue::F64(v) => {
                let ty = TypeId::new(0); // Number type
                let dest = self.alloc_register(ty);
                self.emit(IrInstr::Assign {
                    dest: dest.clone(),
                    value: IrValue::Constant(IrConstant::F64(*v)),
                });
                dest
            }
            ConstantValue::String(s) => {
                let ty = TypeId::new(1); // String type
                let dest = self.alloc_register(ty);
                self.emit(IrInstr::Assign {
                    dest: dest.clone(),
                    value: IrValue::Constant(IrConstant::String(s.clone())),
                });
                dest
            }
            ConstantValue::Bool(v) => {
                let ty = TypeId::new(2); // Boolean type
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
                // Load the RefCell pointer
                let refcell_ty = TypeId::new(0);
                let refcell_reg = self.alloc_register(refcell_ty);
                self.emit(IrInstr::LoadLocal {
                    dest: refcell_reg.clone(),
                    index: local_idx,
                });
                // Load the value from the RefCell
                let value_ty = TypeId::new(0); // Would need to track the inner type
                let dest = self.alloc_register(value_ty);
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
                .unwrap_or(TypeId::new(0));
            let dest = self.alloc_register(ty);
            self.emit(IrInstr::LoadLocal {
                dest: dest.clone(),
                index: local_idx,
            });
            return dest;
        }

        // Check if we've already captured this variable
        if let Some(idx) = self.captures.iter().position(|c| c.symbol == ident.name) {
            let ty = self.captures[idx].ty;
            let is_refcell = self.captures[idx].is_refcell;
            let capture_idx = idx as u16;

            if is_refcell {
                // Load the RefCell pointer from captured
                let refcell_ty = TypeId::new(0);
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
                let capture_idx = self.captures.len() as u16;
                self.captures.push(super::CaptureInfo {
                    symbol: ident.name,
                    source: ancestor_var.source,
                    capture_idx,
                    ty,
                    is_refcell,
                });

                if is_refcell {
                    // Load the RefCell pointer from captured
                    let refcell_ty = TypeId::new(0);
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

    fn lower_call(&mut self, call: &ast::CallExpression) -> Register {
        // Lower arguments first
        let args: Vec<Register> = call.arguments.iter().map(|a| self.lower_expr(a)).collect();

        // Try to resolve the callee
        let dest = self.alloc_register(TypeId::new(0));

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

        if let Expression::Identifier(ident) = &*call.callee {
            // Check for builtin functions/intrinsics first
            let name = self.interner.resolve(ident.name);

            // Handle __NATIVE_CALL intrinsic: __NATIVE_CALL(native_id, args...)
            if name == "__NATIVE_CALL" {
                // First argument must be the native ID (integer literal or constant)
                if let Some(first_arg) = call.arguments.first() {
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
                                        eprintln!("Warning: __NATIVE_CALL constant '{}' is not a number", name);
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
                            eprintln!("Warning: __NATIVE_CALL first argument must be an integer literal or constant");
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
                    return dest;
                }
            }

            // Handle __OPCODE_CHANNEL_NEW intrinsic: __OPCODE_CHANNEL_NEW(capacity)
            if name == "__OPCODE_CHANNEL_NEW" {
                let capacity = if !call.arguments.is_empty() {
                    self.lower_expr(&call.arguments[0])
                } else {
                    let zero_reg = self.alloc_register(TypeId::new(1));
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
                self.emit(IrInstr::NewMutex {
                    dest: dest.clone(),
                });
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

            // Check if it's a direct function call
            if let Some(&func_id) = self.function_map.get(&ident.name) {
                // Check if this is an async function - emit Spawn instead of Call
                if self.async_functions.contains(&func_id) {
                    // Use proper Task type for the destination register
                    let task_ty = self.type_ctx.generic_task_type().unwrap_or(TypeId::new(11));
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

            // Otherwise, it might be a closure stored in a variable
            if let Some(&local_idx) = self.local_map.get(&ident.name) {
                // Load the closure from the local variable
                let closure_ty = self
                    .local_registers
                    .get(&local_idx)
                    .map(|r| r.ty)
                    .unwrap_or(TypeId::new(0));
                let closure = self.alloc_register(closure_ty);
                self.emit(IrInstr::LoadLocal {
                    dest: closure.clone(),
                    index: local_idx,
                });

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
                    use crate::compiler::native_id::{JSON_STRINGIFY, JSON_PARSE};

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
                                const JSON_TYPE_ID: u32 = 15;
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
                                use crate::compiler::native_id::JSON_DECODE_OBJECT;

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
                                const JSON_TYPE_ID: u32 = 15;
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

                // Check for Msgpack type-safe codec methods
                if obj_name == "Msgpack" {
                    match method_name {
                        "encode" => {
                            // Msgpack.encode<T>(obj) -> typed encoder with field metadata
                            use crate::compiler::native_id::MSGPACK_ENCODE_OBJECT;
                            if let Some(type_args) = &call.type_args {
                                if let Some(first_type) = type_args.first() {
                                    if let Some(field_info) = self.get_json_field_info(&first_type.ty) {
                                        return self.emit_codec_encode_with_fields(
                                            dest.clone(), args, field_info, MSGPACK_ENCODE_OBJECT,
                                        );
                                    }
                                }
                            }
                        }
                        "decode" => {
                            // Msgpack.decode<T>(bytes) -> typed decoder with field metadata
                            use crate::compiler::native_id::MSGPACK_DECODE_OBJECT;
                            if let Some(type_args) = &call.type_args {
                                if let Some(first_type) = type_args.first() {
                                    if let Some(field_info) = self.get_json_field_info(&first_type.ty) {
                                        return self.emit_codec_decode_with_fields(
                                            dest.clone(), args, field_info, MSGPACK_DECODE_OBJECT,
                                        );
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }

                // Check for Cbor type-safe codec methods
                if obj_name == "Cbor" {
                    match method_name {
                        "encode" => {
                            // Cbor.encode<T>(obj) -> typed encoder with field metadata
                            use crate::compiler::native_id::CBOR_ENCODE_OBJECT;
                            if let Some(type_args) = &call.type_args {
                                if let Some(first_type) = type_args.first() {
                                    if let Some(field_info) = self.get_json_field_info(&first_type.ty) {
                                        return self.emit_codec_encode_with_fields(
                                            dest.clone(), args, field_info, CBOR_ENCODE_OBJECT,
                                        );
                                    }
                                }
                            }
                        }
                        "decode" => {
                            // Cbor.decode<T>(bytes) -> typed decoder with field metadata
                            use crate::compiler::native_id::CBOR_DECODE_OBJECT;
                            if let Some(type_args) = &call.type_args {
                                if let Some(first_type) = type_args.first() {
                                    if let Some(field_info) = self.get_json_field_info(&first_type.ty) {
                                        return self.emit_codec_decode_with_fields(
                                            dest.clone(), args, field_info, CBOR_DECODE_OBJECT,
                                        );
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }

                // Check for Protobuf type-safe codec methods
                if obj_name == "Protobuf" {
                    match method_name {
                        "encode" => {
                            // Protobuf.encode<T>(obj) -> typed encoder with proto field metadata
                            use crate::compiler::native_id::PROTO_ENCODE_OBJECT;
                            if let Some(type_args) = &call.type_args {
                                if let Some(first_type) = type_args.first() {
                                    if let Some(field_info) = self.get_proto_field_info(&first_type.ty) {
                                        return self.emit_proto_encode_with_fields(
                                            dest.clone(), args, field_info, PROTO_ENCODE_OBJECT,
                                        );
                                    }
                                }
                            }
                        }
                        "decode" => {
                            // Protobuf.decode<T>(bytes) -> typed decoder with proto field metadata
                            use crate::compiler::native_id::PROTO_DECODE_OBJECT;
                            if let Some(type_args) = &call.type_args {
                                if let Some(first_type) = type_args.first() {
                                    if let Some(field_info) = self.get_proto_field_info(&first_type.ty) {
                                        return self.emit_proto_decode_with_fields(
                                            dest.clone(), args, field_info, PROTO_DECODE_OBJECT,
                                        );
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            // Check if this is a Task method call (e.g., task.cancel())
            // Task<T> is a special type that holds a raw task_id (u64), not an object
            if let Expression::Identifier(ident) = &*member.object {
                if let Some(&local_idx) = self.local_map.get(&ident.name) {
                    if let Some(reg) = self.local_registers.get(&local_idx) {
                        if self.type_ctx.is_task_type(reg.ty) {
                            // Load the task handle (raw task_id u64)
                            let task_reg = self.alloc_register(reg.ty);
                            self.emit(IrInstr::LoadLocal {
                                dest: task_reg.clone(),
                                index: local_idx,
                            });

                            match method_name {
                                "cancel" => {
                                    // Emit TaskCancel opcode
                                    self.emit(IrInstr::TaskCancel { task: task_reg });
                                    return dest; // void return
                                }
                                "isDone" | "isCancelled" => {
                                    // These are native calls that take the task handle
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
                                _ => {}
                            }
                        }
                    }
                }
            }

            // Check if this is a static method call (e.g., Utils.double(21))
            if let Expression::Identifier(ident) = &*member.object {
                if let Some(&class_id) = self.class_map.get(&ident.name) {
                    // This is a class identifier, check for static methods
                    if let Some(&func_id) = self.static_method_map.get(&(class_id, method_name_symbol)) {
                        // Static method call - no 'this' parameter
                        // Check if async method - emit Spawn instead of Call
                        if self.async_functions.contains(&func_id) {
                            let task_ty = self.type_ctx.generic_task_type().unwrap_or(TypeId::new(11));
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
            // Parameters with Channel<T> type annotation get TypeId(100) but aren't in variable_class_map
            if class_id.is_none() {
                if let Expression::Identifier(ident) = &*member.object {
                    // Check if this identifier is a local variable with Channel type (TypeId 100)
                    if let Some(&local_idx) = self.local_map.get(&ident.name) {
                        if let Some(reg) = self.local_registers.get(&local_idx) {
                            if reg.ty.as_u32() == 100 {
                                // This is a Channel type - look up Channel class by finding it in class_map
                                for (&sym, &cid) in &self.class_map {
                                    if self.interner.resolve(sym) == "Channel" {
                                        class_id = Some(cid);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Check if this is a user-defined class method (including inherited methods)
            if let Some(class_id) = class_id {
                if let Some(func_id) = self.find_method(class_id, method_name_symbol) {
                    // Lower the object (receiver) first
                    let object = self.lower_expr(&member.object);

                    // Build args with 'this' as first argument
                    let mut method_args = vec![object];
                    method_args.extend(args);

                    // Check if async method - emit Spawn instead of Call
                    if self.async_functions.contains(&func_id) {
                        let task_ty = self.type_ctx.generic_task_type().unwrap_or(TypeId::new(11));
                        let task_dest = self.alloc_register(task_ty);
                        self.emit(IrInstr::Spawn {
                            dest: task_dest.clone(),
                            func: func_id,
                            args: method_args,
                        });
                        return task_dest;
                    } else {
                        // Call the method function
                        self.emit(IrInstr::Call {
                            dest: Some(dest.clone()),
                            func: func_id,
                            args: method_args,
                        });
                    }
                    return dest;
                }
            }

            // Fall back to builtin method handling
            let object = self.lower_expr(&member.object);
            let obj_type_id = object.ty.as_u32();

            // Note: Channel methods are NOT handled here via NativeCall.
            // Channel methods go through normal vtable dispatch to Channel class methods,
            // which internally use __NATIVE_CALL with the channel ID stored in the object.

            // Look up builtin method ID, with special handling for string RegExp overloads
            let method_id = if obj_type_id == 1 && !args.is_empty() && args[0].ty.as_u32() == 8 {
                // String method with RegExp first argument - use RegExp variant
                match method_name {
                    "replace" => builtin_string::REPLACE_REGEXP,
                    "split" => builtin_string::SPLIT_REGEXP,
                    _ => lookup_builtin_method(obj_type_id, method_name).unwrap_or(0),
                }
            } else {
                lookup_builtin_method(obj_type_id, method_name).unwrap_or(0)
            };

            // Array callback methods are inlined as compiler intrinsics
            // instead of emitting CallMethod (which would need nested execution)
            const ARRAY_CALLBACK_METHODS: &[u16] = &[
                builtin_array::MAP,
                builtin_array::FILTER,
                builtin_array::REDUCE,
                builtin_array::FOR_EACH,
                builtin_array::FIND,
                builtin_array::FIND_INDEX,
                builtin_array::SOME,
                builtin_array::EVERY,
                builtin_array::SORT,
            ];
            if ARRAY_CALLBACK_METHODS.contains(&method_id) {
                return self.lower_array_intrinsic(dest, method_id, object, args);
            }

            self.emit(IrInstr::CallMethod {
                dest: Some(dest.clone()),
                object,
                method: method_id,
                args,
            });
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

    /// Lower array callback methods as inline loops (compiler intrinsics).
    ///
    /// Instead of emitting CallMethod that requires nested execution in the VM,
    /// we emit the iteration loop directly with CallClosure for the callback.
    /// The callback executes on the main interpreter stack via normal frame push/pop.
    fn lower_array_intrinsic(
        &mut self,
        dest: Register,
        method_id: u16,
        array: Register,
        args: Vec<Register>,
    ) -> Register {
        match method_id {
            builtin_array::MAP => self.lower_array_map(dest, array, args),
            builtin_array::FILTER => self.lower_array_filter(dest, array, args),
            builtin_array::FOR_EACH => self.lower_array_foreach(dest, array, args),
            builtin_array::REDUCE => self.lower_array_reduce(dest, array, args),
            builtin_array::FIND => self.lower_array_find(dest, array, args),
            builtin_array::FIND_INDEX => self.lower_array_find_index(dest, array, args),
            builtin_array::SOME => self.lower_array_some(dest, array, args),
            builtin_array::EVERY => self.lower_array_every(dest, array, args),
            builtin_array::SORT => self.lower_array_sort(dest, array, args),
            _ => unreachable!("Not an array callback method: 0x{:04X}", method_id),
        }
    }

    /// Helper: emit integer constant into a register
    fn emit_i32_const(&mut self, value: i32) -> Register {
        let reg = self.alloc_register(TypeId::new(0)); // number type
        self.emit(IrInstr::Assign {
            dest: reg.clone(),
            value: IrValue::Constant(IrConstant::I32(value)),
        });
        reg
    }

    /// Helper: emit boolean constant into a register
    fn emit_bool_const(&mut self, value: bool) -> Register {
        let reg = self.alloc_register(TypeId::new(2)); // boolean type
        self.emit(IrInstr::Assign {
            dest: reg.clone(),
            value: IrValue::Constant(IrConstant::Boolean(value)),
        });
        reg
    }

    /// Helper: emit null constant into a register
    fn emit_null_const(&mut self) -> Register {
        let reg = self.alloc_register(TypeId::new(3)); // null type
        self.emit(IrInstr::Assign {
            dest: reg.clone(),
            value: IrValue::Constant(IrConstant::Null),
        });
        reg
    }

    /// Helper: create a new block and switch to it
    fn enter_new_block(&mut self, label: &str) -> crate::compiler::ir::BasicBlockId {
        let block = self.alloc_block();
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(block, label));
        self.current_block = block;
        block
    }

    // arr.map(callback) → inline loop with CallClosure
    fn lower_array_map(
        &mut self,
        dest: Register,
        array: Register,
        args: Vec<Register>,
    ) -> Register {
        let callback = args.into_iter().next().expect("map requires a callback argument");

        // Get array length
        let len = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::ArrayLen {
            dest: len.clone(),
            array: array.clone(),
        });

        // Create result array
        let result = self.alloc_register(array.ty);
        self.emit(IrInstr::ArrayLiteral {
            dest: result.clone(),
            elements: vec![],
            elem_ty: TypeId::new(0),
        });

        // i = 0
        let i = self.emit_i32_const(0);

        // Allocate blocks
        let header_block = self.alloc_block();
        let body_block = self.alloc_block();
        let exit_block = self.alloc_block();

        // Jump to header
        self.set_terminator(crate::compiler::ir::Terminator::Jump(header_block));

        // Header: if i < len → body, else → exit
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(header_block, "map.header"));
        self.current_block = header_block;
        let cond = self.alloc_register(TypeId::new(2));
        self.emit(IrInstr::BinaryOp {
            dest: cond.clone(),
            op: BinaryOp::Less,
            left: i.clone(),
            right: len.clone(),
        });
        self.set_terminator(crate::compiler::ir::Terminator::Branch {
            cond,
            then_block: body_block,
            else_block: exit_block,
        });

        // Body: elem = arr[i]; mapped = callback(elem); result.push(mapped); i++
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(body_block, "map.body"));
        self.current_block = body_block;

        let elem = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::LoadElement {
            dest: elem.clone(),
            array: array.clone(),
            index: i.clone(),
        });

        let mapped = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::CallClosure {
            dest: Some(mapped.clone()),
            closure: callback.clone(),
            args: vec![elem],
        });

        self.emit(IrInstr::ArrayPush {
            array: result.clone(),
            element: mapped,
        });

        // i = i + 1
        let one = self.emit_i32_const(1);
        self.emit(IrInstr::BinaryOp {
            dest: i.clone(),
            op: BinaryOp::Add,
            left: i.clone(),
            right: one,
        });
        self.set_terminator(crate::compiler::ir::Terminator::Jump(header_block));

        // Exit block
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(exit_block, "map.exit"));
        self.current_block = exit_block;

        // Copy result to dest
        self.emit(IrInstr::Assign {
            dest: dest.clone(),
            value: IrValue::Register(result),
        });
        dest
    }

    // arr.filter(predicate) → inline loop with conditional push
    fn lower_array_filter(
        &mut self,
        dest: Register,
        array: Register,
        args: Vec<Register>,
    ) -> Register {
        let callback = args.into_iter().next().expect("filter requires a callback argument");

        let len = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::ArrayLen {
            dest: len.clone(),
            array: array.clone(),
        });

        let result = self.alloc_register(array.ty);
        self.emit(IrInstr::ArrayLiteral {
            dest: result.clone(),
            elements: vec![],
            elem_ty: TypeId::new(0),
        });

        let i = self.emit_i32_const(0);
        let one = self.emit_i32_const(1);

        let header_block = self.alloc_block();
        let body_block = self.alloc_block();
        let keep_block = self.alloc_block();
        let skip_block = self.alloc_block();
        let exit_block = self.alloc_block();

        self.set_terminator(crate::compiler::ir::Terminator::Jump(header_block));

        // Header
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(header_block, "filter.header"));
        self.current_block = header_block;
        let cond = self.alloc_register(TypeId::new(2));
        self.emit(IrInstr::BinaryOp {
            dest: cond.clone(),
            op: BinaryOp::Less,
            left: i.clone(),
            right: len.clone(),
        });
        self.set_terminator(crate::compiler::ir::Terminator::Branch {
            cond,
            then_block: body_block,
            else_block: exit_block,
        });

        // Body: test = callback(elem)
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(body_block, "filter.body"));
        self.current_block = body_block;

        let elem = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::LoadElement {
            dest: elem.clone(),
            array: array.clone(),
            index: i.clone(),
        });

        let test = self.alloc_register(TypeId::new(2));
        self.emit(IrInstr::CallClosure {
            dest: Some(test.clone()),
            closure: callback.clone(),
            args: vec![elem.clone()],
        });

        self.set_terminator(crate::compiler::ir::Terminator::Branch {
            cond: test,
            then_block: keep_block,
            else_block: skip_block,
        });

        // Keep: push elem to result
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(keep_block, "filter.keep"));
        self.current_block = keep_block;
        self.emit(IrInstr::ArrayPush {
            array: result.clone(),
            element: elem,
        });
        self.set_terminator(crate::compiler::ir::Terminator::Jump(skip_block));

        // Skip: i++
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(skip_block, "filter.skip"));
        self.current_block = skip_block;
        self.emit(IrInstr::BinaryOp {
            dest: i.clone(),
            op: BinaryOp::Add,
            left: i.clone(),
            right: one,
        });
        self.set_terminator(crate::compiler::ir::Terminator::Jump(header_block));

        // Exit
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(exit_block, "filter.exit"));
        self.current_block = exit_block;
        self.emit(IrInstr::Assign {
            dest: dest.clone(),
            value: IrValue::Register(result),
        });
        dest
    }

    // arr.forEach(callback) → inline loop, discard results
    fn lower_array_foreach(
        &mut self,
        dest: Register,
        array: Register,
        args: Vec<Register>,
    ) -> Register {
        let callback = args.into_iter().next().expect("forEach requires a callback argument");

        let len = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::ArrayLen {
            dest: len.clone(),
            array: array.clone(),
        });

        let i = self.emit_i32_const(0);
        let one = self.emit_i32_const(1);

        let header_block = self.alloc_block();
        let body_block = self.alloc_block();
        let exit_block = self.alloc_block();

        self.set_terminator(crate::compiler::ir::Terminator::Jump(header_block));

        // Header
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(header_block, "forEach.header"));
        self.current_block = header_block;
        let cond = self.alloc_register(TypeId::new(2));
        self.emit(IrInstr::BinaryOp {
            dest: cond.clone(),
            op: BinaryOp::Less,
            left: i.clone(),
            right: len.clone(),
        });
        self.set_terminator(crate::compiler::ir::Terminator::Branch {
            cond,
            then_block: body_block,
            else_block: exit_block,
        });

        // Body
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(body_block, "forEach.body"));
        self.current_block = body_block;

        let elem = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::LoadElement {
            dest: elem.clone(),
            array: array.clone(),
            index: i.clone(),
        });

        self.emit(IrInstr::CallClosure {
            dest: None,
            closure: callback.clone(),
            args: vec![elem],
        });

        self.emit(IrInstr::BinaryOp {
            dest: i.clone(),
            op: BinaryOp::Add,
            left: i.clone(),
            right: one,
        });
        self.set_terminator(crate::compiler::ir::Terminator::Jump(header_block));

        // Exit
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(exit_block, "forEach.exit"));
        self.current_block = exit_block;
        dest
    }

    // arr.reduce(callback, initial) → accumulator loop
    fn lower_array_reduce(
        &mut self,
        dest: Register,
        array: Register,
        args: Vec<Register>,
    ) -> Register {
        let mut args_iter = args.into_iter();
        let callback = args_iter.next().expect("reduce requires a callback argument");
        let initial = args_iter.next();

        let len = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::ArrayLen {
            dest: len.clone(),
            array: array.clone(),
        });

        // Set up accumulator and start index based on whether initial value provided
        let acc = self.alloc_register(TypeId::new(0));
        let i = self.alloc_register(TypeId::new(0));

        if let Some(init_val) = initial {
            // acc = initial, i = 0
            self.emit(IrInstr::Assign {
                dest: acc.clone(),
                value: IrValue::Register(init_val),
            });
            self.emit(IrInstr::Assign {
                dest: i.clone(),
                value: IrValue::Constant(IrConstant::I32(0)),
            });
        } else {
            // acc = arr[0], i = 1
            let zero = self.emit_i32_const(0);
            self.emit(IrInstr::LoadElement {
                dest: acc.clone(),
                array: array.clone(),
                index: zero,
            });
            self.emit(IrInstr::Assign {
                dest: i.clone(),
                value: IrValue::Constant(IrConstant::I32(1)),
            });
        }

        let one = self.emit_i32_const(1);

        let header_block = self.alloc_block();
        let body_block = self.alloc_block();
        let exit_block = self.alloc_block();

        self.set_terminator(crate::compiler::ir::Terminator::Jump(header_block));

        // Header
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(header_block, "reduce.header"));
        self.current_block = header_block;
        let cond = self.alloc_register(TypeId::new(2));
        self.emit(IrInstr::BinaryOp {
            dest: cond.clone(),
            op: BinaryOp::Less,
            left: i.clone(),
            right: len.clone(),
        });
        self.set_terminator(crate::compiler::ir::Terminator::Branch {
            cond,
            then_block: body_block,
            else_block: exit_block,
        });

        // Body: acc = callback(acc, elem)
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(body_block, "reduce.body"));
        self.current_block = body_block;

        let elem = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::LoadElement {
            dest: elem.clone(),
            array: array.clone(),
            index: i.clone(),
        });

        self.emit(IrInstr::CallClosure {
            dest: Some(acc.clone()),
            closure: callback.clone(),
            args: vec![acc.clone(), elem],
        });

        self.emit(IrInstr::BinaryOp {
            dest: i.clone(),
            op: BinaryOp::Add,
            left: i.clone(),
            right: one,
        });
        self.set_terminator(crate::compiler::ir::Terminator::Jump(header_block));

        // Exit
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(exit_block, "reduce.exit"));
        self.current_block = exit_block;
        self.emit(IrInstr::Assign {
            dest: dest.clone(),
            value: IrValue::Register(acc),
        });
        dest
    }

    // arr.find(predicate) → returns first matching element or null
    fn lower_array_find(
        &mut self,
        dest: Register,
        array: Register,
        args: Vec<Register>,
    ) -> Register {
        let callback = args.into_iter().next().expect("find requires a callback argument");

        let len = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::ArrayLen {
            dest: len.clone(),
            array: array.clone(),
        });

        let result = self.emit_null_const();
        let i = self.emit_i32_const(0);
        let one = self.emit_i32_const(1);

        let header_block = self.alloc_block();
        let body_block = self.alloc_block();
        let found_block = self.alloc_block();
        let next_block = self.alloc_block();
        let exit_block = self.alloc_block();

        self.set_terminator(crate::compiler::ir::Terminator::Jump(header_block));

        // Header
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(header_block, "find.header"));
        self.current_block = header_block;
        let cond = self.alloc_register(TypeId::new(2));
        self.emit(IrInstr::BinaryOp {
            dest: cond.clone(),
            op: BinaryOp::Less,
            left: i.clone(),
            right: len.clone(),
        });
        self.set_terminator(crate::compiler::ir::Terminator::Branch {
            cond,
            then_block: body_block,
            else_block: exit_block,
        });

        // Body
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(body_block, "find.body"));
        self.current_block = body_block;

        let elem = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::LoadElement {
            dest: elem.clone(),
            array: array.clone(),
            index: i.clone(),
        });

        let test = self.alloc_register(TypeId::new(2));
        self.emit(IrInstr::CallClosure {
            dest: Some(test.clone()),
            closure: callback.clone(),
            args: vec![elem.clone()],
        });

        self.set_terminator(crate::compiler::ir::Terminator::Branch {
            cond: test,
            then_block: found_block,
            else_block: next_block,
        });

        // Found
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(found_block, "find.found"));
        self.current_block = found_block;
        self.emit(IrInstr::Assign {
            dest: result.clone(),
            value: IrValue::Register(elem),
        });
        self.set_terminator(crate::compiler::ir::Terminator::Jump(exit_block));

        // Next: i++
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(next_block, "find.next"));
        self.current_block = next_block;
        self.emit(IrInstr::BinaryOp {
            dest: i.clone(),
            op: BinaryOp::Add,
            left: i.clone(),
            right: one,
        });
        self.set_terminator(crate::compiler::ir::Terminator::Jump(header_block));

        // Exit
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(exit_block, "find.exit"));
        self.current_block = exit_block;
        self.emit(IrInstr::Assign {
            dest: dest.clone(),
            value: IrValue::Register(result),
        });
        dest
    }

    // arr.findIndex(predicate) → returns first matching index or -1
    fn lower_array_find_index(
        &mut self,
        dest: Register,
        array: Register,
        args: Vec<Register>,
    ) -> Register {
        let callback = args.into_iter().next().expect("findIndex requires a callback argument");

        let len = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::ArrayLen {
            dest: len.clone(),
            array: array.clone(),
        });

        let result = self.emit_i32_const(-1);
        let i = self.emit_i32_const(0);
        let one = self.emit_i32_const(1);

        let header_block = self.alloc_block();
        let body_block = self.alloc_block();
        let found_block = self.alloc_block();
        let next_block = self.alloc_block();
        let exit_block = self.alloc_block();

        self.set_terminator(crate::compiler::ir::Terminator::Jump(header_block));

        // Header
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(header_block, "findIndex.header"));
        self.current_block = header_block;
        let cond = self.alloc_register(TypeId::new(2));
        self.emit(IrInstr::BinaryOp {
            dest: cond.clone(),
            op: BinaryOp::Less,
            left: i.clone(),
            right: len.clone(),
        });
        self.set_terminator(crate::compiler::ir::Terminator::Branch {
            cond,
            then_block: body_block,
            else_block: exit_block,
        });

        // Body
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(body_block, "findIndex.body"));
        self.current_block = body_block;

        let elem = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::LoadElement {
            dest: elem.clone(),
            array: array.clone(),
            index: i.clone(),
        });

        let test = self.alloc_register(TypeId::new(2));
        self.emit(IrInstr::CallClosure {
            dest: Some(test.clone()),
            closure: callback.clone(),
            args: vec![elem],
        });

        self.set_terminator(crate::compiler::ir::Terminator::Branch {
            cond: test,
            then_block: found_block,
            else_block: next_block,
        });

        // Found: result = i
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(found_block, "findIndex.found"));
        self.current_block = found_block;
        self.emit(IrInstr::Assign {
            dest: result.clone(),
            value: IrValue::Register(i.clone()),
        });
        self.set_terminator(crate::compiler::ir::Terminator::Jump(exit_block));

        // Next: i++
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(next_block, "findIndex.next"));
        self.current_block = next_block;
        self.emit(IrInstr::BinaryOp {
            dest: i.clone(),
            op: BinaryOp::Add,
            left: i.clone(),
            right: one,
        });
        self.set_terminator(crate::compiler::ir::Terminator::Jump(header_block));

        // Exit
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(exit_block, "findIndex.exit"));
        self.current_block = exit_block;
        self.emit(IrInstr::Assign {
            dest: dest.clone(),
            value: IrValue::Register(result),
        });
        dest
    }

    // arr.some(predicate) → returns true if any element matches
    fn lower_array_some(
        &mut self,
        dest: Register,
        array: Register,
        args: Vec<Register>,
    ) -> Register {
        let callback = args.into_iter().next().expect("some requires a callback argument");

        let len = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::ArrayLen {
            dest: len.clone(),
            array: array.clone(),
        });

        let result = self.emit_bool_const(false);
        let i = self.emit_i32_const(0);
        let one = self.emit_i32_const(1);

        let header_block = self.alloc_block();
        let body_block = self.alloc_block();
        let found_block = self.alloc_block();
        let next_block = self.alloc_block();
        let exit_block = self.alloc_block();

        self.set_terminator(crate::compiler::ir::Terminator::Jump(header_block));

        // Header
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(header_block, "some.header"));
        self.current_block = header_block;
        let cond = self.alloc_register(TypeId::new(2));
        self.emit(IrInstr::BinaryOp {
            dest: cond.clone(),
            op: BinaryOp::Less,
            left: i.clone(),
            right: len.clone(),
        });
        self.set_terminator(crate::compiler::ir::Terminator::Branch {
            cond,
            then_block: body_block,
            else_block: exit_block,
        });

        // Body
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(body_block, "some.body"));
        self.current_block = body_block;

        let elem = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::LoadElement {
            dest: elem.clone(),
            array: array.clone(),
            index: i.clone(),
        });

        let test = self.alloc_register(TypeId::new(2));
        self.emit(IrInstr::CallClosure {
            dest: Some(test.clone()),
            closure: callback.clone(),
            args: vec![elem],
        });

        self.set_terminator(crate::compiler::ir::Terminator::Branch {
            cond: test,
            then_block: found_block,
            else_block: next_block,
        });

        // Found: result = true
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(found_block, "some.found"));
        self.current_block = found_block;
        self.emit(IrInstr::Assign {
            dest: result.clone(),
            value: IrValue::Constant(IrConstant::Boolean(true)),
        });
        self.set_terminator(crate::compiler::ir::Terminator::Jump(exit_block));

        // Next: i++
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(next_block, "some.next"));
        self.current_block = next_block;
        self.emit(IrInstr::BinaryOp {
            dest: i.clone(),
            op: BinaryOp::Add,
            left: i.clone(),
            right: one,
        });
        self.set_terminator(crate::compiler::ir::Terminator::Jump(header_block));

        // Exit
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(exit_block, "some.exit"));
        self.current_block = exit_block;
        self.emit(IrInstr::Assign {
            dest: dest.clone(),
            value: IrValue::Register(result),
        });
        dest
    }

    // arr.every(predicate) → returns false if any element fails
    fn lower_array_every(
        &mut self,
        dest: Register,
        array: Register,
        args: Vec<Register>,
    ) -> Register {
        let callback = args.into_iter().next().expect("every requires a callback argument");

        let len = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::ArrayLen {
            dest: len.clone(),
            array: array.clone(),
        });

        let result = self.emit_bool_const(true);
        let i = self.emit_i32_const(0);
        let one = self.emit_i32_const(1);

        let header_block = self.alloc_block();
        let body_block = self.alloc_block();
        let failed_block = self.alloc_block();
        let next_block = self.alloc_block();
        let exit_block = self.alloc_block();

        self.set_terminator(crate::compiler::ir::Terminator::Jump(header_block));

        // Header
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(header_block, "every.header"));
        self.current_block = header_block;
        let cond = self.alloc_register(TypeId::new(2));
        self.emit(IrInstr::BinaryOp {
            dest: cond.clone(),
            op: BinaryOp::Less,
            left: i.clone(),
            right: len.clone(),
        });
        self.set_terminator(crate::compiler::ir::Terminator::Branch {
            cond,
            then_block: body_block,
            else_block: exit_block,
        });

        // Body
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(body_block, "every.body"));
        self.current_block = body_block;

        let elem = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::LoadElement {
            dest: elem.clone(),
            array: array.clone(),
            index: i.clone(),
        });

        let test = self.alloc_register(TypeId::new(2));
        self.emit(IrInstr::CallClosure {
            dest: Some(test.clone()),
            closure: callback.clone(),
            args: vec![elem],
        });

        // Branch: if test → next, else → failed
        self.set_terminator(crate::compiler::ir::Terminator::Branch {
            cond: test,
            then_block: next_block,
            else_block: failed_block,
        });

        // Failed: result = false
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(failed_block, "every.failed"));
        self.current_block = failed_block;
        self.emit(IrInstr::Assign {
            dest: result.clone(),
            value: IrValue::Constant(IrConstant::Boolean(false)),
        });
        self.set_terminator(crate::compiler::ir::Terminator::Jump(exit_block));

        // Next: i++
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(next_block, "every.next"));
        self.current_block = next_block;
        self.emit(IrInstr::BinaryOp {
            dest: i.clone(),
            op: BinaryOp::Add,
            left: i.clone(),
            right: one,
        });
        self.set_terminator(crate::compiler::ir::Terminator::Jump(header_block));

        // Exit
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(exit_block, "every.exit"));
        self.current_block = exit_block;
        self.emit(IrInstr::Assign {
            dest: dest.clone(),
            value: IrValue::Register(result),
        });
        dest
    }

    // arr.sort(compareFn) → bubble sort in-place, returns array
    fn lower_array_sort(
        &mut self,
        dest: Register,
        array: Register,
        args: Vec<Register>,
    ) -> Register {
        let compare_fn = args.into_iter().next().expect("sort requires a compareFn argument");

        let len = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::ArrayLen {
            dest: len.clone(),
            array: array.clone(),
        });

        let one = self.emit_i32_const(1);
        let zero_f64 = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::Assign {
            dest: zero_f64.clone(),
            value: IrValue::Constant(IrConstant::F64(0.0)),
        });

        // limit = len - 1
        let limit = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::BinaryOp {
            dest: limit.clone(),
            op: BinaryOp::Sub,
            left: len.clone(),
            right: one.clone(),
        });

        let swapped = self.emit_bool_const(false);
        let i = self.emit_i32_const(0);

        let outer_header = self.alloc_block();
        let inner_header = self.alloc_block();
        let inner_body = self.alloc_block();
        let swap_block = self.alloc_block();
        let no_swap_block = self.alloc_block();
        let inner_exit = self.alloc_block();
        let done_block = self.alloc_block();

        self.set_terminator(crate::compiler::ir::Terminator::Jump(outer_header));

        // Outer header: swapped = false, i = 0
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(outer_header, "sort.outer"));
        self.current_block = outer_header;
        self.emit(IrInstr::Assign {
            dest: swapped.clone(),
            value: IrValue::Constant(IrConstant::Boolean(false)),
        });
        self.emit(IrInstr::Assign {
            dest: i.clone(),
            value: IrValue::Constant(IrConstant::I32(0)),
        });
        self.set_terminator(crate::compiler::ir::Terminator::Jump(inner_header));

        // Inner header: if i < limit → inner_body, else → inner_exit
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(inner_header, "sort.inner.header"));
        self.current_block = inner_header;
        let inner_cond = self.alloc_register(TypeId::new(2));
        self.emit(IrInstr::BinaryOp {
            dest: inner_cond.clone(),
            op: BinaryOp::Less,
            left: i.clone(),
            right: limit.clone(),
        });
        self.set_terminator(crate::compiler::ir::Terminator::Branch {
            cond: inner_cond,
            then_block: inner_body,
            else_block: inner_exit,
        });

        // Inner body: compare arr[i] and arr[i+1]
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(inner_body, "sort.inner.body"));
        self.current_block = inner_body;

        let j = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::BinaryOp {
            dest: j.clone(),
            op: BinaryOp::Add,
            left: i.clone(),
            right: one.clone(),
        });

        let a = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::LoadElement {
            dest: a.clone(),
            array: array.clone(),
            index: i.clone(),
        });

        let b = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::LoadElement {
            dest: b.clone(),
            array: array.clone(),
            index: j.clone(),
        });

        let cmp = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::CallClosure {
            dest: Some(cmp.clone()),
            closure: compare_fn.clone(),
            args: vec![a.clone(), b.clone()],
        });

        let should_swap = self.alloc_register(TypeId::new(2));
        self.emit(IrInstr::BinaryOp {
            dest: should_swap.clone(),
            op: BinaryOp::Greater,
            left: cmp,
            right: zero_f64.clone(),
        });

        self.set_terminator(crate::compiler::ir::Terminator::Branch {
            cond: should_swap,
            then_block: swap_block,
            else_block: no_swap_block,
        });

        // Swap: arr[i] = b, arr[j] = a, swapped = true
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(swap_block, "sort.swap"));
        self.current_block = swap_block;
        self.emit(IrInstr::StoreElement {
            array: array.clone(),
            index: i.clone(),
            value: b,
        });
        self.emit(IrInstr::StoreElement {
            array: array.clone(),
            index: j.clone(),
            value: a,
        });
        self.emit(IrInstr::Assign {
            dest: swapped.clone(),
            value: IrValue::Constant(IrConstant::Boolean(true)),
        });
        self.set_terminator(crate::compiler::ir::Terminator::Jump(no_swap_block));

        // No swap: i++
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(no_swap_block, "sort.no_swap"));
        self.current_block = no_swap_block;
        self.emit(IrInstr::BinaryOp {
            dest: i.clone(),
            op: BinaryOp::Add,
            left: i.clone(),
            right: one.clone(),
        });
        self.set_terminator(crate::compiler::ir::Terminator::Jump(inner_header));

        // Inner exit: if swapped → outer_header, else → done
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(inner_exit, "sort.inner.exit"));
        self.current_block = inner_exit;
        self.set_terminator(crate::compiler::ir::Terminator::Branch {
            cond: swapped.clone(),
            then_block: outer_header,
            else_block: done_block,
        });

        // Done: dest = arr
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(done_block, "sort.done"));
        self.current_block = done_block;
        self.emit(IrInstr::Assign {
            dest: dest.clone(),
            value: IrValue::Register(array),
        });
        dest
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
                    let dest = self.alloc_register(TypeId::new(0));
                    self.emit(IrInstr::LoadGlobal {
                        dest: dest.clone(),
                        index,
                    });
                    return dest;
                }
            }
        }

        // Try to determine the class type of the object for field resolution
        let class_id = match &*member.object {
            // Handle 'this.field' - use current class context
            Expression::This(_) => self.current_class,
            // Handle 'obj.field' where obj is a variable
            Expression::Identifier(ident) => self.variable_class_map.get(&ident.name).copied(),
            _ => None,
        };

        let object = self.lower_expr(&member.object);

        // Check for JSON type - use duck typing with dynamic property access
        // JSON type is pre-interned at TypeId 15
        // (0=Number, 1=String, 2=Boolean, 3=Null, 4=Void, 5=Never, 6=Unknown,
        //  7=Mutex, 8=RegExp, 9=Date, 10=Buffer, 11=Task, 12=Channel, 13=Map, 14=Set, 15=Json)
        const JSON_TYPE_ID: u32 = 15;
        if object.ty.as_u32() == JSON_TYPE_ID {
            // JSON duck typing - emit JsonLoadProperty which does runtime string lookup
            let json_type = TypeId::new(JSON_TYPE_ID);
            let dest = self.alloc_register(json_type); // Result is also json
            self.emit(IrInstr::JsonLoadProperty {
                dest: dest.clone(),
                object,
                property: prop_name.to_string(),
            });
            return dest;
        }

        // Check for built-in properties on primitive types
        // Pre-interned TypeIds: 0=Number, 1=String, 2=Boolean, 3=Null, 4=Void, 5=Never, 6=Unknown
        if prop_name == "length" {
            let obj_ty = object.ty.as_u32();

            // String type (TypeId 1)
            if obj_ty == 1 {
                let dest = self.alloc_register(TypeId::new(0)); // Number result
                self.emit(IrInstr::StringLen {
                    dest: dest.clone(),
                    string: object,
                });
                return dest;
            }

            // Array types: TypeId > 6 (after pre-interned primitives)
            // OR TypeId 0 (Number) because arrays are currently typed as Number in IR
            // Numbers don't have .length, so if we're accessing .length on TypeId 0,
            // it's most likely an array
            if obj_ty > 6 || obj_ty == 0 {
                let dest = self.alloc_register(TypeId::new(0)); // Number result
                self.emit(IrInstr::ArrayLen {
                    dest: dest.clone(),
                    array: object,
                });
                return dest;
            }
        }

        // Look up field index and type by name if we know the class type (including inherited fields)
        let (field_index, field_ty) = if let Some(class_id) = class_id {
            // Get all fields including parent fields
            let all_fields = self.get_all_fields(class_id);
            if let Some(field) = all_fields
                .iter()
                .find(|f| self.interner.resolve(f.name) == prop_name)
            {
                (field.index, field.ty)
            } else {
                (0, TypeId::new(0))
            }
        } else {
            // Check variable_object_fields for decoded object field layout
            let obj_field_idx = match &*member.object {
                Expression::Identifier(ident) => {
                    self.variable_object_fields
                        .get(&ident.name)
                        .and_then(|fields| {
                            fields
                                .iter()
                                .find(|(name, _)| name == prop_name)
                                .map(|(_, idx)| *idx as u16)
                        })
                }
                _ => None,
            };
            (obj_field_idx.unwrap_or(0), TypeId::new(0))
        };

        // Fall back to field access for objects
        let dest = self.alloc_register(field_ty);
        self.emit(IrInstr::LoadField {
            dest: dest.clone(),
            object,
            field: field_index,
        });
        dest
    }

    fn lower_index(&mut self, index: &ast::IndexExpression) -> Register {
        let array = self.lower_expr(&index.object);
        let idx = self.lower_expr(&index.index);
        let dest = self.alloc_register(TypeId::new(0));

        self.emit(IrInstr::LoadElement {
            dest: dest.clone(),
            array,
            index: idx,
        });
        dest
    }

    fn lower_array(&mut self, array: &ast::ArrayExpression, full_expr: &Expression) -> Register {
        // ArrayExpression.elements is Vec<Option<ArrayElement>>
        // ArrayElement can be Expression or Spread
        let mut elements = Vec::new();
        for elem_opt in &array.elements {
            if let Some(elem) = elem_opt {
                match elem {
                    ast::ArrayElement::Expression(expr) => {
                        elements.push(self.lower_expr(expr));
                    }
                    ast::ArrayElement::Spread(spread_expr) => {
                        // For now, just lower the spread expression as a single element
                        // A full implementation would need to handle spread at runtime
                        elements.push(self.lower_expr(spread_expr));
                    }
                }
            }
        }
        let elem_ty = elements.first().map(|r| r.ty).unwrap_or(TypeId::new(0));
        // Get the array type from the type checker, or use a default array TypeId
        let array_ty = self.get_expr_type(full_expr);
        let dest = self.alloc_register(array_ty);

        self.emit(IrInstr::ArrayLiteral {
            dest: dest.clone(),
            elements,
            elem_ty,
        });
        dest
    }

    fn lower_object(&mut self, object: &ast::ObjectExpression) -> Register {
        let dest = self.alloc_register(TypeId::new(0));
        let mut fields = Vec::new();

        for (idx, prop) in object.properties.iter().enumerate() {
            match prop {
                ast::ObjectProperty::Property(p) => {
                    let value = self.lower_expr(&p.value);
                    fields.push((idx as u16, value));
                }
                ast::ObjectProperty::Spread(_spread) => {
                    // Spread properties would need runtime handling
                    // For now, skip them
                }
            }
        }

        self.emit(IrInstr::ObjectLiteral {
            dest: dest.clone(),
            class: crate::ir::ClassId::new(0),
            fields,
        });
        dest
    }

    fn lower_assignment(&mut self, assign: &ast::AssignmentExpression) -> Register {
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
        };

        let rhs = self.lower_expr(&assign.right);

        // Compute the final value to store
        let value = if let Some(op) = binary_op {
            // Compound assignment: load current value, apply operation
            let current = self.lower_expr(&assign.left);
            let dest = self.alloc_register(TypeId::new(0));
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
                        let refcell_ty = TypeId::new(0);
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

                        // Check for self-recursive closure: if we just assigned a closure
                        // that captured this variable, patch the closure's capture
                        if let Some((closure_reg, ref captures)) = self.last_closure_info.take() {
                            if let Some(&(_, capture_idx)) = captures.iter().find(|(sym, _)| *sym == ident.name) {
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
                } else if let Some(idx) = self.captures.iter().position(|c| c.symbol == ident.name) {
                    // Variable is captured - handle assignment to captured variable
                    let is_refcell = self.captures[idx].is_refcell;
                    let capture_idx = idx as u16;

                    if is_refcell {
                        // Load the RefCell pointer from captured
                        let refcell_ty = TypeId::new(0);
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
                        let capture_idx = self.captures.len() as u16;
                        self.captures.push(super::CaptureInfo {
                            symbol: ident.name,
                            source: ancestor_var.source,
                            capture_idx,
                            ty,
                            is_refcell,
                        });

                        if is_refcell {
                            // Load the RefCell pointer from captured
                            let refcell_ty = TypeId::new(0);
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
                    }
                }
            }
            Expression::Member(member) => {
                let prop_name = self.interner.resolve(member.property.name);

                // Try to determine the class type of the object for field resolution
                let class_id = match &*member.object {
                    // Handle 'this.field' - use current class context
                    Expression::This(_) => self.current_class,
                    // Handle 'obj.field' where obj is a variable
                    Expression::Identifier(ident) => self.variable_class_map.get(&ident.name).copied(),
                    _ => None,
                };

                // Look up field index by name if we know the class type
                // Use get_all_fields to include inherited fields from parent classes
                let field_index = if let Some(class_id) = class_id {
                    self.get_all_fields(class_id)
                        .iter()
                        .find(|f| self.interner.resolve(f.name) == prop_name)
                        .map(|f| f.index)
                        .unwrap_or(0)
                } else {
                    0
                };

                let object = self.lower_expr(&member.object);
                self.emit(IrInstr::StoreField {
                    object,
                    field: field_index,
                    value: value.clone(),
                });
            }
            Expression::Index(index) => {
                let array = self.lower_expr(&index.object);
                let idx = self.lower_expr(&index.index);
                self.emit(IrInstr::StoreElement {
                    array,
                    index: idx,
                    value: value.clone(),
                });
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
            sources: vec![(then_exit_block, then_result), (else_exit_block, else_result)],
        });

        dest
    }

    fn lower_arrow(&mut self, arrow: &ast::ArrowFunction) -> Register {
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
                .unwrap_or(TypeId::new(0));
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
        self.next_local = 0;

        // Create parameter registers
        let mut params = Vec::new();
        for param in &arrow.params {
            let ty = param
                .type_annotation
                .as_ref()
                .map(|t| self.resolve_type_annotation(t))
                .unwrap_or(TypeId::new(0));
            let reg = self.alloc_register(ty);

            if let ast::Pattern::Identifier(ident) = &param.pattern {
                let local_idx = self.allocate_local(ident.name);
                self.local_registers.insert(local_idx, reg.clone());
            }
            params.push(reg);
        }

        // Get return type
        let return_ty = arrow
            .return_type
            .as_ref()
            .map(|t| self.resolve_type_annotation(t))
            .unwrap_or_else(|| TypeId::new(0));

        // Create the arrow function
        let ir_func = crate::ir::IrFunction::new(&arrow_name, params, return_ty);
        self.current_function = Some(ir_func);

        // Create entry block
        let entry_block = self.alloc_block();
        self.current_block = entry_block;
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::with_label(entry_block, "entry"));

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
        self.pending_arrow_functions.push((func_id.as_u32(), arrow_func));

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
        self.this_register = saved_this_register;
        self.this_ancestor_info = saved_this_ancestor_info;
        self.this_captured_idx = saved_this_captured_idx;

        // Load captured variables and build captures list for MakeClosure
        let mut capture_regs = Vec::new();
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
                            let (source, is_refcell) = if let Some(ref ancestors) = child_ancestor_variables {
                                if let Some(ancestor_var) = ancestors.get(&cap.symbol) {
                                    (ancestor_var.source, ancestor_var.is_refcell)
                                } else {
                                    // Variable not in child's ancestors - should not happen
                                    // Fall back to loading from locals if available
                                    if let Some(&local_idx) = self.local_map.get(&cap.symbol) {
                                        (super::AncestorSource::ImmediateParentLocal(local_idx), cap.is_refcell)
                                    } else {
                                        (super::AncestorSource::Ancestor, cap.is_refcell)
                                    }
                                }
                            } else {
                                // No child ancestors - check our own locals
                                if let Some(&local_idx) = self.local_map.get(&cap.symbol) {
                                    // Check if it's a RefCell
                                    let is_refcell = self.refcell_registers.contains_key(&local_idx);
                                    (super::AncestorSource::ImmediateParentLocal(local_idx), is_refcell)
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

        // Handle `this` capture if the arrow function used `this`
        if child_this_captured_idx.is_some() {
            let this_reg = self.alloc_register(TypeId::new(0)); // Object type

            // Check where `this` comes from
            if let Some(ref parent_this) = self.this_register {
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
                        // `this` was in immediate parent's locals
                        self.emit(IrInstr::LoadLocal {
                            dest: this_reg.clone(),
                            index: local_idx,
                        });
                    }
                    super::AncestorSource::Ancestor => {
                        // `this` is from further ancestor - we need to capture it too
                        // Check if parent already captured `this`
                        if let Some(parent_this_capture) = self.this_captured_idx {
                            self.emit(IrInstr::LoadCaptured {
                                dest: this_reg.clone(),
                                index: parent_this_capture,
                            });
                        } else {
                            // Parent needs to capture `this` now
                            // (This case shouldn't normally happen if we're tracking correctly)
                            // Fall back to loading from local 0
                            self.emit(IrInstr::LoadLocal {
                                dest: this_reg.clone(),
                                index: 0,
                            });
                        }
                    }
                }
            }

            capture_regs.push(this_reg);
        }

        // Create closure: emit MakeClosure instruction with captures
        let closure_ty = TypeId::new(0); // Generic function type
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

        dest
    }

    fn lower_typeof(&mut self, typeof_expr: &ast::TypeofExpression) -> Register {
        let operand = self.lower_expr(&typeof_expr.argument);
        let dest = self.alloc_register(TypeId::new(3)); // string type

        self.emit(IrInstr::Typeof {
            dest: dest.clone(),
            operand,
        });
        dest
    }

    fn lower_new(&mut self, new_expr: &ast::NewExpression) -> Register {
        let dest = self.alloc_register(TypeId::new(0));

        if let Expression::Identifier(ident) = &*new_expr.callee {
            // Handle built-in primitive constructors
            let name = self.interner.resolve(ident.name);
            if name == "RegExp" {
                // new RegExp(pattern, flags?) -> NativeCall(0x0A00)
                // Use TypeId 8 for RegExp
                let regexp_dest = self.alloc_register(TypeId::new(8));
                let mut args = Vec::new();
                for arg in &new_expr.arguments {
                    args.push(self.lower_expr(arg));
                }
                // If flags not provided, pass empty string
                if args.len() == 1 {
                    let empty_flags = self.alloc_register(TypeId::new(1)); // String type
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
            let elements: Vec<Register> = arr.elements.iter().filter_map(|e| {
                match e {
                    Some(ast::ArrayElement::Expression(expr)) => Some(self.lower_expr(expr)),
                    _ => None, // Skip spread elements and holes for now
                }
            }).collect();

            // Create the array of tasks
            let tasks_array = self.alloc_register(TypeId::new(0)); // Task[] type
            self.emit(IrInstr::ArrayLiteral {
                dest: tasks_array.clone(),
                elements,
                elem_ty: TypeId::new(0), // Task type
            });

            // Emit await_all instruction
            let dest = self.alloc_register(TypeId::new(0)); // Result array type
            self.emit(IrInstr::AwaitAll {
                dest: dest.clone(),
                tasks: tasks_array,
            });
            return dest;
        }

        // Lower the awaited expression (should be a Task)
        let task = self.lower_expr(&await_expr.argument);

        // Emit await instruction
        let dest = self.alloc_register(TypeId::new(0)); // Result type
        self.emit(IrInstr::Await {
            dest: dest.clone(),
            task,
        });
        dest
    }

    fn lower_async_call(&mut self, async_call: &ast::AsyncCallExpression) -> Register {
        // Lower arguments first
        let args: Vec<Register> = async_call.arguments.iter().map(|a| self.lower_expr(a)).collect();

        // Destination for the Task handle - use proper Task type
        let task_ty = self.type_ctx.generic_task_type().unwrap_or(TypeId::new(11));
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
                    .unwrap_or(TypeId::new(0));
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
                    if let Some(&func_id) = self.static_method_map.get(&(class_id, method_name_symbol)) {
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

            // Instance method - need to find the method and spawn with 'this'
            // For now, we'll look up the method in the class hierarchy
            // This is a simplified version - in practice we'd need proper method resolution
            let mut method_args = vec![object];
            method_args.extend(args);

            // Try to find the method - for now emit a placeholder
            // In a full implementation, we'd resolve the method like in lower_call
            // and emit Spawn with the method's function ID
        }

        // Fallback: return null for unhandled cases
        self.lower_null_literal()
    }

    fn lower_this(&mut self) -> Register {
        // Return the 'this' register if we're directly inside a method
        if let Some(ref this_reg) = self.this_register {
            return this_reg.clone();
        }

        // Check if we've already captured `this`
        if let Some(capture_idx) = self.this_captured_idx {
            let dest = self.alloc_register(TypeId::new(0));
            self.emit(IrInstr::LoadCaptured {
                dest: dest.clone(),
                index: capture_idx,
            });
            return dest;
        }

        // Check if `this` is available from ancestor scope (we're inside an arrow in a method)
        if let Some(ref _ancestor_info) = self.this_ancestor_info {
            // Record that we need to capture `this`
            // The capture index will be after all regular captures
            let capture_idx = self.captures.len() as u16;
            self.this_captured_idx = Some(capture_idx);

            let dest = self.alloc_register(TypeId::new(0));
            self.emit(IrInstr::LoadCaptured {
                dest: dest.clone(),
                index: capture_idx,
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
        let class_id = self.resolve_class_from_type(&instanceof.type_name);

        // Allocate register for boolean result
        let dest = self.alloc_register(TypeId::new(2)); // Boolean type

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

        // Resolve the class ID from the type annotation
        let class_id = self.resolve_class_from_type(&cast.target_type);

        // Allocate register for the casted object (same type as target)
        let dest = self.alloc_register(TypeId::new(6)); // Unknown type - will be refined by type checker

        self.emit(IrInstr::Cast {
            dest: dest.clone(),
            object,
            class_id,
        });

        dest
    }

    /// Resolve a ClassId from a type annotation
    fn resolve_class_from_type(&self, type_ann: &ast::TypeAnnotation) -> ClassId {
        use crate::parser::ast::types::Type;

        match &type_ann.ty {
            Type::Reference(type_ref) => {
                // Look up the class by name
                if let Some(&class_id) = self.class_map.get(&type_ref.name.name) {
                    return class_id;
                }
                // Unknown class - return class ID 0 as fallback
                ClassId::new(0)
            }
            _ => {
                // Non-reference types (primitives, unions, etc.) - not valid for instanceof/as
                // Return class ID 0 as fallback
                ClassId::new(0)
            }
        }
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

    /// Get all fields for a class, including inherited fields from parent classes.
    /// Returns fields in order: parent fields first, then child fields.
    fn get_all_fields(&self, class_id: ClassId) -> Vec<ClassFieldInfo> {
        let mut all_fields = Vec::new();

        // First, get parent fields (recursively)
        if let Some(class_info) = self.class_info_map.get(&class_id) {
            if let Some(parent_id) = class_info.parent_class {
                all_fields.extend(self.get_all_fields(parent_id));
            }
            // Then add this class's fields
            all_fields.extend(class_info.fields.clone());
        }

        all_fields
    }

    /// Infer the class ID of an expression (for method call resolution)
    fn infer_class_id(&self, expr: &Expression) -> Option<ClassId> {
        match expr {
            // 'this' uses current class context
            Expression::This(_) => self.current_class,
            // Variable lookup
            Expression::Identifier(ident) => self.variable_class_map.get(&ident.name).copied(),
            // Field access: look up the field's type in the class definition
            Expression::Member(member) => {
                // Get the class of the object
                let obj_class_id = self.infer_class_id(&member.object)?;
                // Look up the field type
                let field_name = self.interner.resolve(member.property.name);
                let all_fields = self.get_all_fields(obj_class_id);
                for field in all_fields {
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
                None
            }
            // Method call: check if the method has a known return class type
            Expression::Call(call) => {
                if let Expression::Member(member) = &*call.callee {
                    let obj_class_id = self.infer_class_id(&member.object)?;
                    let method_name = member.property.name;
                    // Check if method has an explicit return class type
                    if let Some(&ret_class_id) = self.method_return_class_map.get(&(obj_class_id, method_name)) {
                        return Some(ret_class_id);
                    }
                    // Otherwise, if the method exists, assume it returns the same class
                    if self.method_map.contains_key(&(obj_class_id, method_name)) {
                        return Some(obj_class_id);
                    }
                }
                None
            }
            // New expression: return the class being instantiated
            Expression::New(new_expr) => {
                if let Expression::Identifier(ident) = &*new_expr.callee {
                    return self.class_map.get(&ident.name).copied();
                }
                None
            }
            _ => None,
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
        let string_ty = TypeId::new(3); // string type

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
        let all_strings = template.parts.iter().all(|p| matches!(p, TemplatePart::String(_)));

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
                    if expr_reg.ty.as_u32() == 3 {
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
    fn infer_binary_result_type(&self, op: &BinaryOp, left: &Register, _right: &Register) -> TypeId {
        // Pre-interned TypeIds: 0=Number, 1=String, 2=Boolean, 3=Null, 4=Void, 5=Never, 6=Unknown
        if op.is_comparison() || op.is_logical() {
            TypeId::new(2) // Boolean type
        } else {
            left.ty // Same as left operand for arithmetic
        }
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
