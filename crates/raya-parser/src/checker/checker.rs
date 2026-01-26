//! Type checker - validates types for expressions and statements
//!
//! The type checker walks the AST and verifies that all operations are
//! type-safe. It uses the symbol table for name resolution and the type
//! context for type operations.

use super::error::CheckError;
use super::symbols::SymbolTable;
use super::type_guards::{extract_type_guard, TypeGuard};
use super::narrowing::{apply_type_guard, TypeEnv};
use super::exhaustiveness::{check_switch_exhaustiveness, ExhaustivenessResult};
use crate::ast::*;
use crate::{Interner, Symbol as ParserSymbol};
use crate::types::{AssignabilityContext, TypeContext, TypeId};
use rustc_hash::FxHashMap;

/// Get the variable name from a type guard
fn get_guard_var(guard: &TypeGuard) -> &String {
    match guard {
        TypeGuard::TypeOf { var, .. } => var,
        TypeGuard::Discriminant { var, .. } => var,
        TypeGuard::Nullish { var, .. } => var,
        TypeGuard::IsArray { var, .. } => var,
        TypeGuard::IsInteger { var, .. } => var,
        TypeGuard::IsNaN { var, .. } => var,
        TypeGuard::IsFinite { var, .. } => var,
        TypeGuard::TypePredicate { var, .. } => var,
    }
}

/// Negate a type guard
fn negate_guard(guard: &TypeGuard) -> TypeGuard {
    match guard {
        TypeGuard::TypeOf { var, type_name, negated } => TypeGuard::TypeOf {
            var: var.clone(),
            type_name: type_name.clone(),
            negated: !negated,
        },
        TypeGuard::Discriminant { var, field, variant, negated } => TypeGuard::Discriminant {
            var: var.clone(),
            field: field.clone(),
            variant: variant.clone(),
            negated: !negated,
        },
        TypeGuard::Nullish { var, negated } => TypeGuard::Nullish {
            var: var.clone(),
            negated: !negated,
        },
        TypeGuard::IsArray { var, negated } => TypeGuard::IsArray {
            var: var.clone(),
            negated: !negated,
        },
        TypeGuard::IsInteger { var, negated } => TypeGuard::IsInteger {
            var: var.clone(),
            negated: !negated,
        },
        TypeGuard::IsNaN { var, negated } => TypeGuard::IsNaN {
            var: var.clone(),
            negated: !negated,
        },
        TypeGuard::IsFinite { var, negated } => TypeGuard::IsFinite {
            var: var.clone(),
            negated: !negated,
        },
        TypeGuard::TypePredicate { var, predicate, negated } => TypeGuard::TypePredicate {
            var: var.clone(),
            predicate: predicate.clone(),
            negated: !negated,
        },
    }
}

/// Type checker
///
/// Performs type checking on the AST using the symbol table and type context.
pub struct TypeChecker<'a> {
    type_ctx: &'a mut TypeContext,
    symbols: &'a SymbolTable,
    interner: &'a Interner,

    /// Map from expression to its inferred type
    expr_types: FxHashMap<usize, TypeId>,

    /// Type checking errors
    errors: Vec<CheckError>,

    /// Current function return type (for checking return statements)
    current_function_return_type: Option<TypeId>,

    /// Type environment tracking narrowed types in current scope
    type_env: TypeEnv,

    /// Current scope ID for variable resolution
    /// The checker tracks its own scope position as it walks the AST
    current_scope: super::symbols::ScopeId,

    /// Next scope ID to be entered
    /// This mirrors the scope creation in the binder
    next_scope_id: u32,
}

impl<'a> TypeChecker<'a> {
    /// Create a new type checker
    pub fn new(type_ctx: &'a mut TypeContext, symbols: &'a SymbolTable, interner: &'a Interner) -> Self {
        TypeChecker {
            type_ctx,
            symbols,
            interner,
            expr_types: FxHashMap::default(),
            errors: Vec::new(),
            current_function_return_type: None,
            type_env: TypeEnv::new(),
            current_scope: super::symbols::ScopeId(0), // Start at global scope
            next_scope_id: 1, // Global is 0, next scope will be 1
        }
    }

    /// Resolve a parser Symbol to a String
    #[inline]
    fn resolve(&self, sym: ParserSymbol) -> String {
        self.interner.resolve(sym).to_string()
    }

    /// Enter a new scope (like entering a block or function)
    /// Mirrors what the binder does when it pushes a scope
    fn enter_scope(&mut self) {
        let scope_id = super::symbols::ScopeId(self.next_scope_id);
        self.next_scope_id += 1;
        self.current_scope = scope_id;
    }

    /// Exit the current scope, returning to parent
    fn exit_scope(&mut self) {
        if let Some(parent) = self.symbols.get_parent_scope_id(self.current_scope) {
            self.current_scope = parent;
        }
    }

    /// Check a module
    pub fn check_module(mut self, module: &Module) -> Result<(), Vec<CheckError>> {
        for stmt in &module.statements {
            self.check_stmt(stmt);
        }

        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors)
        }
    }

    /// Get the errors collected during checking
    pub fn errors(&self) -> &[CheckError] {
        &self.errors
    }

    /// Check statement
    fn check_stmt(&mut self, stmt: &Statement) {
        match stmt {
            Statement::VariableDecl(decl) => self.check_var_decl(decl),
            Statement::FunctionDecl(func) => self.check_function(func),
            Statement::Expression(expr_stmt) => {
                self.check_expr(&expr_stmt.expression);
            }
            Statement::Return(ret) => self.check_return(ret),
            Statement::If(if_stmt) => self.check_if(if_stmt),
            Statement::While(while_stmt) => self.check_while(while_stmt),
            Statement::For(for_stmt) => self.check_for(for_stmt),
            Statement::Block(block) => {
                // Enter block scope (mirrors binder's push_scope)
                self.enter_scope();
                for stmt in &block.statements {
                    self.check_stmt(stmt);
                }
                // Exit block scope (mirrors binder's pop_scope)
                self.exit_scope();
            }
            Statement::Switch(switch_stmt) => self.check_switch(switch_stmt),
            Statement::Try(try_stmt) => self.check_try(try_stmt),
            _ => {}
        }
    }

    /// Check variable declaration
    fn check_var_decl(&mut self, decl: &VariableDecl) {
        if let Some(ref init) = decl.initializer {
            let init_ty = self.check_expr(init);

            // If there's a type annotation, check that initializer is assignable
            if let Some(ref _ty_annot) = decl.type_annotation {
                // Get the declared type from symbol table
                if let Pattern::Identifier(ident) = &decl.pattern {
                    let name = self.resolve(ident.name);
                    if let Some(symbol) = self.symbols.resolve_from_scope(&name, self.current_scope) {
                        self.check_assignable(init_ty, symbol.ty, *init.span());
                    }
                }
            }
        }
    }

    /// Check function declaration
    fn check_function(&mut self, func: &FunctionDecl) {
        // Get return type from symbol table
        let func_name = self.resolve(func.name.name);
        if let Some(symbol) = self.symbols.resolve_from_scope(&func_name, self.current_scope) {
            if let Some(crate::types::Type::Function(func_ty)) = self.type_ctx.get(symbol.ty) {
                let return_ty = func_ty.return_type;

                // Set current function return type
                let prev_return_ty = self.current_function_return_type;
                self.current_function_return_type = Some(return_ty);

                // Enter function scope (mirrors binder's push_scope for function)
                self.enter_scope();

                // Check body
                for stmt in &func.body.statements {
                    self.check_stmt(stmt);
                }

                // Exit function scope
                self.exit_scope();

                // Restore previous return type
                self.current_function_return_type = prev_return_ty;
            }
        }
    }

    /// Check return statement
    fn check_return(&mut self, ret: &ReturnStatement) {
        if let Some(ref expr) = ret.value {
            let expr_ty = self.check_expr(expr);

            if let Some(expected_ty) = self.current_function_return_type {
                self.check_assignable(expr_ty, expected_ty, *expr.span());
            }
        } else {
            // Return without value - check if function returns void
            if let Some(expected_ty) = self.current_function_return_type {
                let void_ty = self.type_ctx.void_type();
                if expected_ty != void_ty {
                    self.errors.push(CheckError::ReturnTypeMismatch {
                        expected: format!("{:?}", expected_ty),
                        actual: "void".to_string(),
                        span: ret.span,
                    });
                }
            }
        }
    }

    /// Check if statement
    fn check_if(&mut self, if_stmt: &IfStatement) {
        // Check condition is boolean
        let cond_ty = self.check_expr(&if_stmt.condition);
        let bool_ty = self.type_ctx.boolean_type();
        self.check_assignable(cond_ty, bool_ty, *if_stmt.condition.span());

        // Try to extract type guard from condition
        let type_guard = extract_type_guard(&if_stmt.condition, self.interner);

        // Save current environment
        let saved_env = self.type_env.clone();

        // Apply type guard for then branch
        if let Some(ref guard) = type_guard {
            if let Some(symbol) = self.symbols.resolve_from_scope(get_guard_var(guard), self.current_scope) {
                if let Some(narrowed_ty) = apply_type_guard(self.type_ctx, symbol.ty, guard) {
                    self.type_env.set(get_guard_var(guard).clone(), narrowed_ty);
                }
            }
        }

        // Check then branch
        self.check_stmt(&if_stmt.then_branch);
        let then_env = self.type_env.clone();

        // Restore environment and apply negated guard for else branch
        self.type_env = saved_env.clone();

        if let Some(ref else_branch) = if_stmt.else_branch {
            if let Some(ref guard) = type_guard {
                // Apply negated guard
                let negated_guard = negate_guard(guard);
                if let Some(symbol) = self.symbols.resolve_from_scope(get_guard_var(&negated_guard), self.current_scope) {
                    if let Some(narrowed_ty) = apply_type_guard(self.type_ctx, symbol.ty, &negated_guard) {
                        self.type_env.set(get_guard_var(&negated_guard).clone(), narrowed_ty);
                    }
                }
            }

            self.check_stmt(else_branch);
        }

        let else_env = self.type_env.clone();

        // Merge environments from both branches
        self.type_env = then_env.merge(&else_env, self.type_ctx);
    }

    /// Check while loop
    fn check_while(&mut self, while_stmt: &WhileStatement) {
        // Check condition is boolean
        let cond_ty = self.check_expr(&while_stmt.condition);
        let bool_ty = self.type_ctx.boolean_type();
        self.check_assignable(cond_ty, bool_ty, *while_stmt.condition.span());

        // Try to extract type guard from condition
        let type_guard = extract_type_guard(&while_stmt.condition, self.interner);

        // Save current environment
        let saved_env = self.type_env.clone();

        // Apply type guard for loop body
        if let Some(ref guard) = type_guard {
            if let Some(symbol) = self.symbols.resolve_from_scope(get_guard_var(guard), self.current_scope) {
                if let Some(narrowed_ty) = apply_type_guard(self.type_ctx, symbol.ty, guard) {
                    self.type_env.set(get_guard_var(guard).clone(), narrowed_ty);
                }
            }
        }

        // Binder creates a Loop scope, but the body statement (often a Block)
        // will create its own scope. We need to mirror binder's behavior.
        self.enter_scope(); // Loop scope

        // Check body - if it's a Block, it will enter another scope
        self.check_stmt(&while_stmt.body);

        self.exit_scope(); // Exit loop scope

        // Restore environment after loop
        self.type_env = saved_env;
    }

    /// Check for loop
    fn check_for(&mut self, for_stmt: &ForStatement) {
        // Enter loop scope (mirrors binder)
        self.enter_scope();

        // Check test condition if present
        if let Some(ref test) = for_stmt.test {
            let cond_ty = self.check_expr(test);
            let bool_ty = self.type_ctx.boolean_type();
            self.check_assignable(cond_ty, bool_ty, *test.span());
        }

        // Check update expression if present
        if let Some(ref update) = for_stmt.update {
            self.check_expr(update);
        }

        // Check body
        self.check_stmt(&for_stmt.body);

        // Exit loop scope
        self.exit_scope();
    }

    /// Check switch statement
    fn check_switch(&mut self, switch_stmt: &SwitchStatement) {
        // Check discriminant and get its type
        let discriminant_ty = self.check_expr(&switch_stmt.discriminant);

        // Check exhaustiveness for discriminated unions
        let exhaustiveness = check_switch_exhaustiveness(
            self.type_ctx,
            discriminant_ty,
            switch_stmt,
            self.interner,
        );

        // Report non-exhaustive matches
        if let ExhaustivenessResult::NonExhaustive(missing) = exhaustiveness {
            self.errors.push(CheckError::NonExhaustiveMatch {
                missing,
                span: switch_stmt.span,
            });
        }

        // Check cases
        for case in &switch_stmt.cases {
            if let Some(ref test) = case.test {
                self.check_expr(test);
            }

            for stmt in &case.consequent {
                self.check_stmt(stmt);
            }
        }
    }

    /// Check try-catch statement
    fn check_try(&mut self, try_stmt: &TryStatement) {
        // Check try block
        for stmt in &try_stmt.body.statements {
            self.check_stmt(stmt);
        }

        // Check catch block
        if let Some(ref catch) = try_stmt.catch_clause {
            for stmt in &catch.body.statements {
                self.check_stmt(stmt);
            }
        }

        // Check finally block
        if let Some(ref finally) = try_stmt.finally_clause {
            for stmt in &finally.statements {
                self.check_stmt(stmt);
            }
        }
    }

    /// Check expression (returns inferred type)
    fn check_expr(&mut self, expr: &Expression) -> TypeId {
        let ty = match expr {
            Expression::IntLiteral(_) | Expression::FloatLiteral(_) => {
                self.type_ctx.number_type()
            }
            Expression::StringLiteral(_) | Expression::TemplateLiteral(_) => {
                self.type_ctx.string_type()
            }
            Expression::BooleanLiteral(_) => self.type_ctx.boolean_type(),
            Expression::NullLiteral(_) => self.type_ctx.null_type(),
            Expression::Identifier(ident) => self.check_identifier(ident),
            Expression::Binary(bin) => self.check_binary(bin),
            Expression::Logical(log) => self.check_logical(log),
            Expression::Unary(un) => self.check_unary(un),
            Expression::Call(call) => self.check_call(call),
            Expression::Member(member) => self.check_member(member),
            Expression::Array(arr) => self.check_array(arr),
            Expression::Object(obj) => self.check_object(obj),
            Expression::Conditional(cond) => self.check_conditional(cond),
            Expression::Assignment(assign) => self.check_assignment(assign),
            Expression::Typeof(_) => {
                // typeof always returns a string
                self.type_ctx.string_type()
            }
            _ => self.type_ctx.unknown_type(),
        };

        // Store type for this expression (using pointer address as ID)
        let expr_id = expr as *const _ as usize;
        self.expr_types.insert(expr_id, ty);

        ty
    }

    /// Check identifier
    fn check_identifier(&mut self, ident: &Identifier) -> TypeId {
        let name = self.resolve(ident.name);

        // First check for narrowed type in type environment
        if let Some(narrowed_ty) = self.type_env.get(&name) {
            return narrowed_ty;
        }

        // Otherwise look up in symbol table from current scope
        match self.symbols.resolve_from_scope(&name, self.current_scope) {
            Some(symbol) => symbol.ty,
            None => {
                self.errors.push(CheckError::UndefinedVariable {
                    name,
                    span: ident.span,
                });
                self.type_ctx.unknown_type()
            }
        }
    }

    /// Check binary expression
    fn check_binary(&mut self, bin: &BinaryExpression) -> TypeId {
        let left_ty = self.check_expr(&bin.left);
        let right_ty = self.check_expr(&bin.right);

        match bin.operator {
            BinaryOperator::Add
            | BinaryOperator::Subtract
            | BinaryOperator::Multiply
            | BinaryOperator::Divide
            | BinaryOperator::Modulo
            | BinaryOperator::Exponent => {
                // Arithmetic operations require number operands
                let number_ty = self.type_ctx.number_type();
                self.check_assignable(left_ty, number_ty, *bin.left.span());
                self.check_assignable(right_ty, number_ty, *bin.right.span());
                number_ty
            }

            BinaryOperator::Equal
            | BinaryOperator::NotEqual
            | BinaryOperator::StrictEqual
            | BinaryOperator::StrictNotEqual
            | BinaryOperator::LessThan
            | BinaryOperator::LessEqual
            | BinaryOperator::GreaterThan
            | BinaryOperator::GreaterEqual => {
                // Comparison operations return boolean
                self.type_ctx.boolean_type()
            }

            BinaryOperator::BitwiseAnd
            | BinaryOperator::BitwiseOr
            | BinaryOperator::BitwiseXor
            | BinaryOperator::LeftShift
            | BinaryOperator::RightShift
            | BinaryOperator::UnsignedRightShift => {
                // Bitwise operations require number operands
                let number_ty = self.type_ctx.number_type();
                self.check_assignable(left_ty, number_ty, *bin.left.span());
                self.check_assignable(right_ty, number_ty, *bin.right.span());
                number_ty
            }
        }
    }

    /// Check logical expression
    fn check_logical(&mut self, log: &LogicalExpression) -> TypeId {
        let left_ty = self.check_expr(&log.left);
        let right_ty = self.check_expr(&log.right);

        // Logical operations require boolean operands
        let bool_ty = self.type_ctx.boolean_type();
        self.check_assignable(left_ty, bool_ty, *log.left.span());
        self.check_assignable(right_ty, bool_ty, *log.right.span());

        bool_ty
    }

    /// Check unary expression
    fn check_unary(&mut self, un: &UnaryExpression) -> TypeId {
        let operand_ty = self.check_expr(&un.operand);

        match un.operator {
            UnaryOperator::Not => {
                // Logical not requires boolean
                let bool_ty = self.type_ctx.boolean_type();
                self.check_assignable(operand_ty, bool_ty, *un.operand.span());
                bool_ty
            }
            UnaryOperator::Plus | UnaryOperator::Minus | UnaryOperator::BitwiseNot => {
                // Numeric operations require number
                let number_ty = self.type_ctx.number_type();
                self.check_assignable(operand_ty, number_ty, *un.operand.span());
                number_ty
            }
            UnaryOperator::PrefixIncrement
            | UnaryOperator::PrefixDecrement
            | UnaryOperator::PostfixIncrement
            | UnaryOperator::PostfixDecrement => {
                // Increment/decrement require number
                let number_ty = self.type_ctx.number_type();
                self.check_assignable(operand_ty, number_ty, *un.operand.span());
                number_ty
            }
        }
    }

    /// Check function call
    fn check_call(&mut self, call: &CallExpression) -> TypeId {
        let callee_ty = self.check_expr(&call.callee);

        // Clone the function type to avoid borrow checker issues
        let func_ty_opt = self.type_ctx.get(callee_ty).cloned();

        // Check if callee is a function type
        match func_ty_opt {
            Some(crate::types::Type::Function(func)) => {
                // Check argument count
                if call.arguments.len() != func.params.len() {
                    self.errors.push(CheckError::ArgumentCountMismatch {
                        expected: func.params.len(),
                        actual: call.arguments.len(),
                        span: call.span,
                    });
                }

                // Check argument types
                for (i, arg) in call.arguments.iter().enumerate() {
                    if let Some(&param_ty) = func.params.get(i) {
                        let arg_ty = self.check_expr(arg);
                        self.check_assignable(arg_ty, param_ty, *arg.span());
                    }
                }

                func.return_type
            }
            _ => {
                self.errors.push(CheckError::NotCallable {
                    ty: format!("{:?}", callee_ty),
                    span: call.span,
                });
                self.type_ctx.unknown_type()
            }
        }
    }

    /// Check member access
    fn check_member(&mut self, member: &MemberExpression) -> TypeId {
        let object_ty = self.check_expr(&member.object);

        // Check for forbidden access to $type/$value on bare unions
        let property_name = self.resolve(member.property.name);
        if property_name == "$type" || property_name == "$value" {
            if let Some(crate::types::Type::Union(union)) = self.type_ctx.get(object_ty) {
                if union.is_bare {
                    self.errors.push(CheckError::ForbiddenFieldAccess {
                        field: property_name,
                        span: member.span,
                    });
                    return self.type_ctx.unknown_type();
                }
            }
        }

        // For now, return unknown for member access
        // TODO: Implement property type lookup for objects/classes
        self.type_ctx.unknown_type()
    }

    /// Check array literal
    fn check_array(&mut self, arr: &ArrayExpression) -> TypeId {
        if arr.elements.is_empty() {
            // Empty array - infer as unknown[]
            let unknown = self.type_ctx.unknown_type();
            return self.type_ctx.array_type(unknown);
        }

        // Find first non-None element to infer type
        let first_ty = arr.elements.iter()
            .find_map(|elem_opt| {
                elem_opt.as_ref().map(|elem| match elem {
                    ArrayElement::Expression(expr) => self.check_expr(expr),
                    ArrayElement::Spread(expr) => {
                        // For spread, the type should be element type of the array
                        let spread_ty = self.check_expr(expr);
                        // TODO: Extract element type from array type
                        spread_ty
                    }
                })
            })
            .unwrap_or_else(|| self.type_ctx.unknown_type());

        // Check all elements have compatible types
        for elem_opt in &arr.elements {
            if let Some(elem) = elem_opt {
                let (elem_ty, elem_span) = match elem {
                    ArrayElement::Expression(expr) => (self.check_expr(expr), *expr.span()),
                    ArrayElement::Spread(expr) => (self.check_expr(expr), *expr.span()),
                };
                // TODO: Compute union type instead of requiring exact match
                self.check_assignable(elem_ty, first_ty, elem_span);
            }
        }

        self.type_ctx.array_type(first_ty)
    }

    /// Check object literal
    fn check_object(&mut self, _obj: &ObjectExpression) -> TypeId {
        // TODO: Build object type from properties
        self.type_ctx.unknown_type()
    }

    /// Check conditional (ternary) expression
    fn check_conditional(&mut self, cond: &ConditionalExpression) -> TypeId {
        // Check test is boolean
        let test_ty = self.check_expr(&cond.test);
        let bool_ty = self.type_ctx.boolean_type();
        self.check_assignable(test_ty, bool_ty, *cond.test.span());

        // Check both branches
        let then_ty = self.check_expr(&cond.consequent);
        let else_ty = self.check_expr(&cond.alternate);

        // Return union of both types
        self.type_ctx.union_type(vec![then_ty, else_ty])
    }

    /// Check assignment expression
    fn check_assignment(&mut self, assign: &AssignmentExpression) -> TypeId {
        let left_ty = self.check_expr(&assign.left);
        let right_ty = self.check_expr(&assign.right);

        self.check_assignable(right_ty, left_ty, *assign.right.span());

        left_ty
    }

    /// Check if source type is assignable to target type
    fn check_assignable(&mut self, source: TypeId, target: TypeId, span: crate::Span) {
        let mut assign_ctx = AssignabilityContext::new(self.type_ctx);
        if !assign_ctx.is_assignable(source, target) {
            self.errors.push(CheckError::TypeMismatch {
                expected: self.format_type(target),
                actual: self.format_type(source),
                span,
                note: None,
            });
        }
    }

    /// Format a type for display in error messages
    fn format_type(&self, ty: TypeId) -> String {
        // For now, use Debug formatting
        // TODO: Implement proper type formatting (e.g., "string | number" instead of "TypeId(...)")
        format!("{:?}", ty)
    }

    /// Get type of expression (for external use)
    pub fn get_expr_type(&self, expr: &Expression) -> Option<TypeId> {
        let expr_id = expr as *const _ as usize;
        self.expr_types.get(&expr_id).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::binder::Binder;
    use crate::Parser;

    fn parse_and_check(source: &str) -> Result<(), Vec<CheckError>> {
        let parser = Parser::new(source).unwrap();
        let (module, interner) = parser.parse().unwrap();

        let mut type_ctx = TypeContext::new();
        let binder = Binder::new(&mut type_ctx, &interner);
        let symbols = binder.bind_module(&module).unwrap();

        let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
        checker.check_module(&module)
    }

    #[test]
    fn test_check_simple_arithmetic() {
        let result = parse_and_check("1 + 2;");
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_undefined_variable() {
        let result = parse_and_check("x;");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], CheckError::UndefinedVariable { .. }));
    }

    #[test]
    fn test_check_type_mismatch() {
        let result = parse_and_check(r#"let x: number = "hello";"#);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], CheckError::TypeMismatch { .. }));
    }
}
