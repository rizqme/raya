//! Expression Lowering
//!
//! Converts AST expressions to IR instructions.

use super::{ClassFieldInfo, ConstantValue, Lowerer};
use crate::ir::{BinaryOp, ClassId, FunctionId, IrConstant, IrInstr, IrValue, Register, UnaryOp};
use raya_parser::ast::{self, AssignmentOperator, Expression, TemplatePart};
use raya_parser::interner::Symbol;
use raya_parser::TypeId;

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
                    self.emit(IrInstr::Spawn {
                        dest: dest.clone(),
                        func: func_id,
                        args,
                    });
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

            // Check if this is a static method call (e.g., Utils.double(21))
            if let Expression::Identifier(ident) = &*member.object {
                if let Some(&class_id) = self.class_map.get(&ident.name) {
                    // This is a class identifier, check for static methods
                    if let Some(&func_id) = self.static_method_map.get(&(class_id, method_name_symbol)) {
                        // Static method call - no 'this' parameter
                        // Check if async method - emit Spawn instead of Call
                        if self.async_functions.contains(&func_id) {
                            self.emit(IrInstr::Spawn {
                                dest: dest.clone(),
                                func: func_id,
                                args,
                            });
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
            let class_id = self.infer_class_id(&member.object);

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
                        self.emit(IrInstr::Spawn {
                            dest: dest.clone(),
                            func: func_id,
                            args: method_args,
                        });
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
            (0, TypeId::new(0))
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

        // Destination for the Task handle
        let dest = self.alloc_register(TypeId::new(0)); // Task type

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
        use raya_parser::ast::types::Type;

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
            // Method call: if we know the class and method, check if it returns the same class
            Expression::Call(call) => {
                if let Expression::Member(member) = &*call.callee {
                    let obj_class_id = self.infer_class_id(&member.object)?;
                    let method_name = member.property.name;
                    // If the method exists on the class, assume it returns 'this' (same class)
                    // This is a simplification - ideally we'd check the return type
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
    use raya_parser::{Interner, Parser};

    fn parse_expr(source: &str) -> (ast::Module, Interner) {
        let parser = Parser::new(source).expect("lexer error");
        parser.parse().expect("parse error")
    }

    #[test]
    fn test_lower_integer_literal() {
        let (module, interner) = parse_expr("42;");
        let type_ctx = raya_parser::TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let ir = lowerer.lower_module(&module);

        // Should have a main function with the expression
        assert!(ir.function_count() >= 1);
    }

    #[test]
    fn test_lower_binary_expression() {
        let (module, interner) = parse_expr("1 + 2;");
        let type_ctx = raya_parser::TypeContext::new();
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
        let type_ctx = raya_parser::TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let ir = lowerer.lower_module(&module);

        assert!(ir.get_function_by_name("add").is_some());
    }
}
