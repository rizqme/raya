//! Expression Lowering
//!
//! Converts AST expressions to IR instructions.

use super::Lowerer;
use crate::ir::{BinaryOp, IrConstant, IrInstr, IrValue, Register, UnaryOp};
use raya_parser::ast::{self, Expression};
use raya_parser::TypeId;

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
            Expression::Array(array) => self.lower_array(array),
            Expression::Object(object) => self.lower_object(object),
            Expression::Assignment(assign) => self.lower_assignment(assign),
            Expression::Conditional(cond) => self.lower_conditional(cond),
            Expression::Arrow(arrow) => self.lower_arrow(arrow),
            Expression::Parenthesized(paren) => self.lower_expr(&paren.expression),
            Expression::Typeof(typeof_expr) => self.lower_typeof(typeof_expr),
            Expression::New(new_expr) => self.lower_new(new_expr),
            Expression::Await(await_expr) => self.lower_await(await_expr),
            Expression::Logical(logical) => self.lower_logical(logical),
            _ => {
                // For unhandled expressions, emit a null placeholder
                self.lower_null_literal()
            }
        }
    }

    fn lower_int_literal(&mut self, lit: &ast::IntLiteral) -> Register {
        let ty = TypeId::new(1); // i32 type
        let dest = self.alloc_register(ty);
        self.emit(IrInstr::Assign {
            dest: dest.clone(),
            value: IrValue::Constant(IrConstant::I32(lit.value as i32)),
        });
        dest
    }

    fn lower_float_literal(&mut self, lit: &ast::FloatLiteral) -> Register {
        let ty = TypeId::new(2); // f64 type
        let dest = self.alloc_register(ty);
        self.emit(IrInstr::Assign {
            dest: dest.clone(),
            value: IrValue::Constant(IrConstant::F64(lit.value)),
        });
        dest
    }

    fn lower_string_literal(&mut self, lit: &ast::StringLiteral) -> Register {
        let ty = TypeId::new(3); // string type
        let dest = self.alloc_register(ty);
        let string_value = self.interner.resolve(lit.value).to_string();
        self.emit(IrInstr::Assign {
            dest: dest.clone(),
            value: IrValue::Constant(IrConstant::String(string_value)),
        });
        dest
    }

    fn lower_bool_literal(&mut self, lit: &ast::BooleanLiteral) -> Register {
        let ty = TypeId::new(4); // boolean type
        let dest = self.alloc_register(ty);
        self.emit(IrInstr::Assign {
            dest: dest.clone(),
            value: IrValue::Constant(IrConstant::Boolean(lit.value)),
        });
        dest
    }

    pub(super) fn lower_null_literal(&mut self) -> Register {
        let ty = TypeId::new(0); // null type
        let dest = self.alloc_register(ty);
        self.emit(IrInstr::Assign {
            dest: dest.clone(),
            value: IrValue::Constant(IrConstant::Null),
        });
        dest
    }

    fn lower_identifier(&mut self, ident: &ast::Identifier) -> Register {
        // Look up the variable in the local map
        if let Some(&local_idx) = self.local_map.get(&ident.name) {
            // Get the type from the local register
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
            dest
        } else {
            // Unknown variable - could be a global or error
            // For now, return a null placeholder
            self.lower_null_literal()
        }
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

        if let Expression::Identifier(ident) = &*call.callee {
            // Direct function call
            if let Some(&func_id) = self.function_map.get(&ident.name) {
                self.emit(IrInstr::Call {
                    dest: Some(dest.clone()),
                    func: func_id,
                    args,
                });
                return dest;
            }
        }

        // For member calls or unknown callees, emit a method call placeholder
        if let Expression::Member(member) = &*call.callee {
            let object = self.lower_expr(&member.object);
            self.emit(IrInstr::CallMethod {
                dest: Some(dest.clone()),
                object,
                method: 0, // Would need to resolve method index
                args,
            });
            return dest;
        }

        // Fallback: call with function ID 0
        self.emit(IrInstr::Call {
            dest: Some(dest.clone()),
            func: crate::ir::FunctionId::new(0),
            args,
        });
        dest
    }

    fn lower_member(&mut self, member: &ast::MemberExpression) -> Register {
        let object = self.lower_expr(&member.object);
        let dest = self.alloc_register(TypeId::new(0));

        // For now, use field index 0 - would need type info to resolve actual field
        self.emit(IrInstr::LoadField {
            dest: dest.clone(),
            object,
            field: 0,
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

    fn lower_array(&mut self, array: &ast::ArrayExpression) -> Register {
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
        let dest = self.alloc_register(TypeId::new(0));

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
        let value = self.lower_expr(&assign.right);

        match &*assign.left {
            Expression::Identifier(ident) => {
                if let Some(&local_idx) = self.local_map.get(&ident.name) {
                    self.emit(IrInstr::StoreLocal {
                        index: local_idx,
                        value: value.clone(),
                    });
                }
            }
            Expression::Member(member) => {
                let object = self.lower_expr(&member.object);
                self.emit(IrInstr::StoreField {
                    object,
                    field: 0, // Would need to resolve actual field index
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
        self.set_terminator(crate::ir::Terminator::Jump(merge_block));

        // Else branch
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::new(else_block));
        self.current_block = else_block;
        let else_val = self.lower_expr(&cond.alternate);
        let else_result = else_val.clone();
        self.set_terminator(crate::ir::Terminator::Jump(merge_block));

        // Merge block with phi
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::new(merge_block));
        self.current_block = merge_block;

        let dest = self.alloc_register(then_result.ty);
        self.emit(IrInstr::Phi {
            dest: dest.clone(),
            sources: vec![(then_block, then_result), (else_block, else_result)],
        });

        dest
    }

    fn lower_arrow(&mut self, _arrow: &ast::ArrowFunction) -> Register {
        // Arrow functions create closures - for now just return null
        self.lower_null_literal()
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
        // For now, create a simple object
        let dest = self.alloc_register(TypeId::new(0));

        if let Expression::Identifier(_ident) = &*new_expr.callee {
            // Would need to resolve class ID from type context
            self.emit(IrInstr::NewObject {
                dest: dest.clone(),
                class: crate::ir::ClassId::new(0),
            });
        }

        dest
    }

    fn lower_await(&mut self, await_expr: &ast::AwaitExpression) -> Register {
        // Lower the awaited expression
        // In a real implementation, this would generate task suspension code
        self.lower_expr(&await_expr.argument)
    }

    fn lower_logical(&mut self, logical: &ast::LogicalExpression) -> Register {
        // Logical operators need short-circuit evaluation
        let left = self.lower_expr(&logical.left);

        let eval_right = self.alloc_block();
        let merge_block = self.alloc_block();

        let is_and = matches!(logical.operator, ast::LogicalOperator::And);

        // Short-circuit: if (left is false for &&) or (left is true for ||), skip right
        if is_and {
            self.set_terminator(crate::ir::Terminator::Branch {
                cond: left.clone(),
                then_block: eval_right,
                else_block: merge_block,
            });
        } else {
            self.set_terminator(crate::ir::Terminator::Branch {
                cond: left.clone(),
                then_block: merge_block,
                else_block: eval_right,
            });
        }

        let left_block = self.current_block;

        // Evaluate right side
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::new(eval_right));
        self.current_block = eval_right;
        let right = self.lower_expr(&logical.right);
        self.set_terminator(crate::ir::Terminator::Jump(merge_block));
        let right_block = self.current_block;

        // Merge
        self.current_function_mut()
            .add_block(crate::ir::BasicBlock::new(merge_block));
        self.current_block = merge_block;

        let dest = self.alloc_register(TypeId::new(4)); // boolean
        self.emit(IrInstr::Phi {
            dest: dest.clone(),
            sources: vec![(left_block, left), (right_block, right)],
        });

        dest
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
            ast::BinaryOperator::Exponent => BinaryOp::Add, // TODO: Add exponent to IR
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
        if op.is_comparison() || op.is_logical() {
            TypeId::new(4) // boolean
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
