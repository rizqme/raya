//! Code generation from AST to bytecode

use crate::error::{CompileError, CompileResult};
use crate::module_builder::{FunctionBuilder, ModuleBuilder};
use crate::bytecode::{Module, Opcode};
use raya_parser::ast::*;
use raya_parser::{Interner, Symbol as ParserSymbol};
use raya_parser::TypeContext;
use rustc_hash::FxHashMap;

/// Main code generator
pub struct CodeGenerator<'a> {
    type_ctx: &'a TypeContext,
    interner: &'a Interner,
    module_builder: ModuleBuilder,
    current_function: Option<FunctionBuilder>,
    global_vars: FxHashMap<String, u16>,
}

impl<'a> CodeGenerator<'a> {
    pub fn new(type_ctx: &'a TypeContext, interner: &'a Interner) -> Self {
        Self {
            type_ctx,
            interner,
            module_builder: ModuleBuilder::new("main".to_string()),
            current_function: None,
            global_vars: FxHashMap::default(),
        }
    }

    /// Resolve a parser Symbol to a String
    #[inline]
    fn resolve(&self, sym: ParserSymbol) -> String {
        self.interner.resolve(sym).to_string()
    }

    /// Compile a complete module
    pub fn compile_program(&mut self, module: &raya_parser::ast::Module) -> CompileResult<Module> {
        // Create main function to hold top-level code
        let mut main_fn = FunctionBuilder::new("main".to_string(), 0);

        // Compile all statements in the main function body
        for stmt in &module.statements {
            self.compile_stmt(&mut main_fn, stmt)?;
        }

        // Add implicit return null at end of main
        main_fn.emit(Opcode::ConstNull);
        main_fn.emit(Opcode::Return);

        let main_function = main_fn.build();
        self.module_builder.add_function(main_function);

        // Take ownership and build the module
        let builder = std::mem::replace(&mut self.module_builder, ModuleBuilder::new(String::new()));
        Ok(builder.build())
    }

    /// Compile a statement
    fn compile_stmt(&mut self, func: &mut FunctionBuilder, stmt: &Statement) -> CompileResult<()> {
        match stmt {
            Statement::VariableDecl(decl) => self.compile_var_decl(func, decl),
            Statement::Expression(expr_stmt) => self.compile_expr_stmt(func, expr_stmt),
            Statement::Block(block) => self.compile_block(func, block),
            Statement::Return(ret) => self.compile_return(func, ret),
            _ => Err(CompileError::UnsupportedFeature {
                feature: format!("Statement: {:?}", stmt),
            }),
        }
    }

    /// Compile a variable declaration
    fn compile_var_decl(&mut self, func: &mut FunctionBuilder, decl: &VariableDecl) -> CompileResult<()> {
        // Only support simple identifier patterns for now
        let name = match &decl.pattern {
            Pattern::Identifier(ident) => self.resolve(ident.name),
            _ => {
                return Err(CompileError::UnsupportedFeature {
                    feature: "Destructuring patterns".to_string(),
                })
            }
        };

        // Compile initializer expression (or use null if none)
        if let Some(ref init) = decl.initializer {
            self.compile_expr(func, init)?;
        } else {
            func.emit(Opcode::ConstNull);
        }

        // Allocate local variable
        let local_index = func.add_local(name)?;

        // Store value in local
        func.emit(Opcode::StoreLocal);
        func.emit_u16(local_index);

        Ok(())
    }

    /// Compile an expression statement
    fn compile_expr_stmt(&mut self, func: &mut FunctionBuilder, expr_stmt: &ExpressionStatement) -> CompileResult<()> {
        self.compile_expr(func, &expr_stmt.expression)?;
        // Pop the result since expression statements don't use the value
        func.emit(Opcode::Pop);
        Ok(())
    }

    /// Compile a block statement
    fn compile_block(&mut self, func: &mut FunctionBuilder, block: &BlockStatement) -> CompileResult<()> {
        for stmt in &block.statements {
            self.compile_stmt(func, stmt)?;
        }
        Ok(())
    }

    /// Compile a return statement
    fn compile_return(&mut self, func: &mut FunctionBuilder, ret: &ReturnStatement) -> CompileResult<()> {
        if let Some(ref value) = ret.value {
            self.compile_expr(func, value)?;
        } else {
            func.emit(Opcode::ConstNull);
        }
        func.emit(Opcode::Return);
        Ok(())
    }

    /// Compile an expression
    fn compile_expr(&mut self, func: &mut FunctionBuilder, expr: &Expression) -> CompileResult<()> {
        match expr {
            Expression::IntLiteral(lit) => self.compile_int_literal(func, lit),
            Expression::FloatLiteral(lit) => self.compile_float_literal(func, lit),
            Expression::StringLiteral(lit) => self.compile_string_literal(func, lit),
            Expression::BooleanLiteral(lit) => self.compile_boolean_literal(func, lit),
            Expression::NullLiteral(_) => {
                func.emit(Opcode::ConstNull);
                Ok(())
            }
            Expression::Identifier(ident) => self.compile_identifier(func, ident),
            Expression::Binary(binary) => self.compile_binary(func, binary),
            Expression::Unary(unary) => self.compile_unary(func, unary),
            Expression::Assignment(assign) => self.compile_assign(func, assign),
            _ => Err(CompileError::UnsupportedFeature {
                feature: format!("Expression: {:?}", expr),
            }),
        }
    }

    /// Compile an integer literal
    fn compile_int_literal(&mut self, func: &mut FunctionBuilder, lit: &IntLiteral) -> CompileResult<()> {
        if lit.value >= i32::MIN as i64 && lit.value <= i32::MAX as i64 {
            func.emit(Opcode::ConstI32);
            func.emit_i32(lit.value as i32);
        } else {
            // For larger integers, use f64 for now
            func.emit(Opcode::ConstF64);
            func.emit_f64(lit.value as f64);
        }
        Ok(())
    }

    /// Compile a float literal
    fn compile_float_literal(&mut self, func: &mut FunctionBuilder, lit: &FloatLiteral) -> CompileResult<()> {
        func.emit(Opcode::ConstF64);
        func.emit_f64(lit.value);
        Ok(())
    }

    /// Compile a string literal
    fn compile_string_literal(&mut self, func: &mut FunctionBuilder, lit: &StringLiteral) -> CompileResult<()> {
        // Add string to constant pool
        let const_index = self.module_builder.add_string(self.resolve(lit.value))?;
        func.emit(Opcode::ConstStr);
        func.emit_u16(const_index);
        Ok(())
    }

    /// Compile a boolean literal
    fn compile_boolean_literal(&mut self, func: &mut FunctionBuilder, lit: &BooleanLiteral) -> CompileResult<()> {
        if lit.value {
            func.emit(Opcode::ConstTrue);
        } else {
            func.emit(Opcode::ConstFalse);
        }
        Ok(())
    }

    /// Compile an identifier expression
    fn compile_identifier(&mut self, func: &mut FunctionBuilder, ident: &Identifier) -> CompileResult<()> {
        let name = self.resolve(ident.name);
        // Try to find as local variable
        if let Some(local_index) = func.get_local(&name) {
            func.emit(Opcode::LoadLocal);
            func.emit_u16(local_index);
            Ok(())
        } else {
            Err(CompileError::UndefinedVariable { name })
        }
    }

    /// Compile a binary expression
    fn compile_binary(&mut self, func: &mut FunctionBuilder, binary: &BinaryExpression) -> CompileResult<()> {
        // Compile left and right operands
        self.compile_expr(func, &binary.left)?;
        self.compile_expr(func, &binary.right)?;

        // Emit the appropriate opcode based on operator
        match binary.operator {
            BinaryOperator::Add => func.emit(Opcode::Iadd),
            BinaryOperator::Subtract => func.emit(Opcode::Isub),
            BinaryOperator::Multiply => func.emit(Opcode::Imul),
            BinaryOperator::Divide => func.emit(Opcode::Idiv),
            BinaryOperator::Modulo => func.emit(Opcode::Imod),
            BinaryOperator::LessThan => func.emit(Opcode::Ilt),
            BinaryOperator::LessEqual => func.emit(Opcode::Ile),
            BinaryOperator::GreaterThan => func.emit(Opcode::Igt),
            BinaryOperator::GreaterEqual => func.emit(Opcode::Ige),
            BinaryOperator::Equal => func.emit(Opcode::Ieq),
            BinaryOperator::NotEqual => func.emit(Opcode::Ine),
            _ => {
                return Err(CompileError::UnsupportedFeature {
                    feature: format!("Binary operator: {:?}", binary.operator),
                })
            }
        }

        Ok(())
    }

    /// Compile a logical expression (handled separately from binary)
    fn compile_logical(&mut self, func: &mut FunctionBuilder, logical: &LogicalExpression) -> CompileResult<()> {
        match logical.operator {
            LogicalOperator::And => {
                // Compile left operand
                self.compile_expr(func, &logical.left)?;

                // Duplicate for test
                func.emit(Opcode::Dup);

                // Jump to end if false (short-circuit)
                func.emit(Opcode::JmpIfFalse);
                let jump_pos = func.current_position();
                func.emit_i16(0); // Placeholder

                // Pop the duplicated value and evaluate right
                func.emit(Opcode::Pop);
                self.compile_expr(func, &logical.right)?;

                // Patch jump offset
                let offset = (func.current_position() as isize - (jump_pos + 2) as isize) as i16;
                func.patch_jump(jump_pos, offset);
            }
            LogicalOperator::Or => {
                // Compile left operand
                self.compile_expr(func, &logical.left)?;

                // Duplicate for test
                func.emit(Opcode::Dup);

                // Jump to end if true (short-circuit)
                func.emit(Opcode::JmpIfTrue);
                let jump_pos = func.current_position();
                func.emit_i16(0); // Placeholder

                // Pop the duplicated value and evaluate right
                func.emit(Opcode::Pop);
                self.compile_expr(func, &logical.right)?;

                // Patch jump offset
                let offset = (func.current_position() as isize - (jump_pos + 2) as isize) as i16;
                func.patch_jump(jump_pos, offset);
            }
            LogicalOperator::NullishCoalescing => {
                return Err(CompileError::UnsupportedFeature {
                    feature: "Nullish coalescing operator (??)".to_string(),
                })
            }
        }

        Ok(())
    }

    /// Compile a unary expression
    fn compile_unary(&mut self, func: &mut FunctionBuilder, unary: &UnaryExpression) -> CompileResult<()> {
        // Compile the operand
        self.compile_expr(func, &unary.operand)?;

        // Emit the appropriate opcode
        match unary.operator {
            UnaryOperator::Minus => func.emit(Opcode::Ineg),
            UnaryOperator::Not => func.emit(Opcode::Not),
            _ => {
                return Err(CompileError::UnsupportedFeature {
                    feature: format!("Unary operator: {:?}", unary.operator),
                })
            }
        }

        Ok(())
    }

    /// Compile an assignment expression
    fn compile_assign(&mut self, func: &mut FunctionBuilder, assign: &AssignmentExpression) -> CompileResult<()> {
        // Compile the value expression
        self.compile_expr(func, &assign.right)?;

        // Handle different assignment targets
        match &*assign.left {
            Expression::Identifier(ident) => {
                let name = self.resolve(ident.name);
                // Store to local variable
                if let Some(local_index) = func.get_local(&name) {
                    // Duplicate value on stack (assignment is an expression)
                    func.emit(Opcode::Dup);
                    func.emit(Opcode::StoreLocal);
                    func.emit_u16(local_index);
                    Ok(())
                } else {
                    Err(CompileError::UndefinedVariable { name })
                }
            }
            _ => Err(CompileError::UnsupportedFeature {
                feature: "Assignment to non-identifier".to_string(),
            }),
        }
    }
}
