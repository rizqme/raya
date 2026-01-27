//! Type checker - validates types for expressions and statements
//!
//! The type checker walks the AST and verifies that all operations are
//! type-safe. It uses the symbol table for name resolution and the type
//! context for type operations.

use super::error::CheckError;
use super::symbols::{SymbolKind, SymbolTable};
use super::type_guards::{extract_type_guard, TypeGuard};
use super::narrowing::{apply_type_guard, TypeEnv};
use super::exhaustiveness::{check_switch_exhaustiveness, ExhaustivenessResult};
use super::captures::{CaptureInfo, ClosureCaptures, ClosureId, ModuleCaptureInfo, FreeVariableCollector};
use crate::ast::*;
use crate::{Interner, Symbol as ParserSymbol};
use crate::types::{AssignabilityContext, GenericContext, TypeContext, TypeId};
use crate::types::normalize::contains_type_variables;
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

/// Inferred types for variables without type annotations
///
/// Maps (scope_id, variable_name) to the inferred TypeId.
/// These should be applied to the symbol table using `update_type`.
pub type InferredTypes = FxHashMap<(u32, String), TypeId>;

/// Result of type checking a module
#[derive(Debug)]
pub struct CheckResult {
    /// Inferred types for variables without type annotations
    pub inferred_types: InferredTypes,
    /// Capture information for all closures in the module
    pub captures: ModuleCaptureInfo,
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

    /// Stack of scope IDs for tracking parent scopes
    /// Used when inside expressions (arrow functions) where binder didn't create scopes
    scope_stack: Vec<super::symbols::ScopeId>,

    /// Inferred types for variables without type annotations
    /// Maps (scope_id, variable_name) -> inferred_type
    inferred_var_types: FxHashMap<(u32, String), TypeId>,

    /// Capture information for all closures
    capture_info: ModuleCaptureInfo,
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
            scope_stack: vec![super::symbols::ScopeId(0)], // Start with global on stack
            inferred_var_types: FxHashMap::default(),
            capture_info: ModuleCaptureInfo::new(),
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
        self.scope_stack.push(self.current_scope);
        let scope_id = super::symbols::ScopeId(self.next_scope_id);
        self.next_scope_id += 1;
        self.current_scope = scope_id;
    }

    /// Exit the current scope, returning to parent
    fn exit_scope(&mut self) {
        // Use our own stack instead of querying symbol table
        // This handles cases where we're inside expressions (arrow functions)
        // that the binder didn't create scopes for
        if let Some(parent) = self.scope_stack.pop() {
            self.current_scope = parent;
        }
    }

    /// Check a module
    ///
    /// Returns the check result containing inferred types and capture information.
    /// Inferred types should be applied to the symbol table using `update_type`.
    pub fn check_module(mut self, module: &Module) -> Result<CheckResult, Vec<CheckError>> {
        for stmt in &module.statements {
            self.check_stmt(stmt);
        }

        if self.errors.is_empty() {
            Ok(CheckResult {
                inferred_types: self.inferred_var_types,
                captures: self.capture_info,
            })
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
            Statement::ForOf(for_of) => self.check_for_of(for_of),
            _ => {}
        }
    }

    /// Check variable declaration
    fn check_var_decl(&mut self, decl: &VariableDecl) {
        if let Some(ref init) = decl.initializer {
            let init_ty = self.check_expr(init);

            if let Pattern::Identifier(ident) = &decl.pattern {
                let name = self.resolve(ident.name);

                // Determine the variable's type
                let var_ty = if decl.type_annotation.is_some() {
                    // Get the declared type from symbol table
                    if let Some(symbol) = self.symbols.resolve_from_scope(&name, self.current_scope) {
                        self.check_assignable(init_ty, symbol.ty, *init.span());
                        symbol.ty
                    } else {
                        init_ty
                    }
                } else {
                    // No type annotation - infer type from initializer
                    // Store the inferred type for later lookups
                    self.inferred_var_types.insert(
                        (self.current_scope.0, name.clone()),
                        init_ty
                    );
                    init_ty
                };

                // Also add to type_env so nested arrow functions can see it
                self.type_env.set(name, var_ty);
            }
        }
    }

    /// Check function declaration
    fn check_function(&mut self, func: &FunctionDecl) {
        // Get return type from symbol table
        let func_name = self.resolve(func.name.name);
        if let Some(symbol) = self.symbols.resolve_from_scope(&func_name, self.current_scope) {
            if let Some(crate::types::Type::Function(func_ty)) = self.type_ctx.get(symbol.ty) {
                let mut return_ty = func_ty.return_type;

                // For async functions, the declared return type is Task<T>,
                // but return statements should check against T (the inner type)
                if func.is_async {
                    if let Some(crate::types::Type::Task(task_ty)) = self.type_ctx.get(return_ty) {
                        return_ty = task_ty.result;
                    }
                }

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

        // Check initializer if present
        if let Some(ref init) = for_stmt.init {
            match init {
                ForInit::VariableDecl(decl) => self.check_var_decl(decl),
                ForInit::Expression(expr) => {
                    self.check_expr(expr);
                }
            }
        }

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

    /// Check for-of loop
    fn check_for_of(&mut self, for_of: &ForOfStatement) {
        // Enter loop scope
        self.enter_scope();

        // Check the iterable (right side) and get its type
        let iterable_ty = self.check_expr(&for_of.right);

        // Get the element type from the array
        // For now, we only support arrays
        let elem_ty = if let Some(crate::types::Type::Array(arr)) = self.type_ctx.get(iterable_ty) {
            arr.element
        } else {
            // Not an array - report error and use unknown type
            self.errors.push(CheckError::TypeMismatch {
                expected: "array".to_string(),
                actual: self.format_type(iterable_ty),
                span: *for_of.right.span(),
                note: Some("for-of loops require an iterable (array)".to_string()),
            });
            self.type_ctx.unknown_type()
        };

        // Handle the loop variable (left side)
        // The binder should have already registered the variable
        // We just need to ensure its type matches the element type
        match &for_of.left {
            ForOfLeft::VariableDecl(decl) => {
                // Variable declared in the for-of: `for (let x of arr)`
                // The type should be the element type of the array
                if let Pattern::Identifier(ident) = &decl.pattern {
                    let name = self.resolve(ident.name);
                    // Store inferred type for the loop variable
                    self.inferred_var_types.insert(
                        (self.current_scope.0, name),
                        elem_ty
                    );
                }
            }
            ForOfLeft::Pattern(_) => {
                // Existing variable: `for (x of arr)` - type already bound
            }
        }

        // Check body
        self.check_stmt(&for_of.body);

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
            // Enter catch scope (mirrors binder's push_scope for catch block)
            self.enter_scope();

            // Check catch body statements
            for stmt in &catch.body.statements {
                self.check_stmt(stmt);
            }

            // Exit catch scope
            self.exit_scope();
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
            Expression::Arrow(arrow) => self.check_arrow(arrow),
            Expression::Index(index) => self.check_index(index),
            Expression::New(new_expr) => self.check_new(new_expr),
            Expression::This(_) => self.check_this(),
            Expression::Await(await_expr) => self.check_await(await_expr),
            Expression::AsyncCall(async_call) => self.check_async_call(async_call),
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

        // Look up in symbol table from current scope, walking up the scope chain
        match self.symbols.resolve_from_scope(&name, self.current_scope) {
            Some(symbol) => {
                // Check if we have an inferred type for this variable
                // (for variables declared without type annotations)
                let scope_id = symbol.scope_id.0;
                if let Some(&inferred_ty) = self.inferred_var_types.get(&(scope_id, name.clone())) {
                    return inferred_ty;
                }
                symbol.ty
            }
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
            BinaryOperator::Add => {
                // Add can be either numeric or string concatenation
                let string_ty = self.type_ctx.string_type();
                let number_ty = self.type_ctx.number_type();

                // Check if either operand IS a string type (exact match or assignable)
                let left_is_string = left_ty == string_ty;
                let right_is_string = right_ty == string_ty;

                if left_is_string || right_is_string {
                    // String concatenation - both operands should be strings
                    self.check_assignable(left_ty, string_ty, *bin.left.span());
                    self.check_assignable(right_ty, string_ty, *bin.right.span());
                    string_ty
                } else {
                    // Numeric addition
                    self.check_assignable(left_ty, number_ty, *bin.left.span());
                    self.check_assignable(right_ty, number_ty, *bin.right.span());
                    number_ty
                }
            }

            BinaryOperator::Subtract
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

        match log.operator {
            LogicalOperator::NullishCoalescing => {
                // For x ?? y:
                // - x can be any type (typically T | null)
                // - y is the fallback value
                // - Result is the non-null part of x's type, or union with y

                // Get the non-null type from left operand
                let non_null_ty = self.get_non_null_type(left_ty);

                // Right side should be assignable to the non-null type
                // (or we take a union of both)
                if non_null_ty != right_ty {
                    // Check if right is assignable to non-null left type
                    let mut assign_ctx = AssignabilityContext::new(self.type_ctx);
                    if !assign_ctx.is_assignable(right_ty, non_null_ty) {
                        // If not directly assignable, result is union of both
                        return self.type_ctx.union_type(vec![non_null_ty, right_ty]);
                    }
                }

                non_null_ty
            }
            LogicalOperator::And | LogicalOperator::Or => {
                // Logical AND/OR require boolean operands
                let bool_ty = self.type_ctx.boolean_type();
                self.check_assignable(left_ty, bool_ty, *log.left.span());
                self.check_assignable(right_ty, bool_ty, *log.right.span());
                bool_ty
            }
        }
    }

    /// Get the non-null type from a type (removes null from unions)
    fn get_non_null_type(&mut self, ty: TypeId) -> TypeId {
        let null_ty = self.type_ctx.null_type();

        // If it's exactly null, return never (no non-null part)
        if ty == null_ty {
            return self.type_ctx.never_type();
        }

        // Check if it's a union type
        if let Some(union) = self.type_ctx.get(ty).and_then(|t| t.as_union()) {
            // Filter out null from the union members
            let non_null_members: Vec<TypeId> = union.members.iter()
                .filter(|&&m| m != null_ty)
                .copied()
                .collect();

            match non_null_members.len() {
                0 => self.type_ctx.never_type(),
                1 => non_null_members[0],
                _ => self.type_ctx.union_type(non_null_members),
            }
        } else {
            // Not a union, return as-is (already non-null)
            ty
        }
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

        // Check all argument types first (before creating GenericContext)
        let arg_types: Vec<(TypeId, crate::Span)> = call.arguments.iter()
            .map(|arg| (self.check_expr(arg), *arg.span()))
            .collect();

        // Clone the function type to avoid borrow checker issues
        let func_ty_opt = self.type_ctx.get(callee_ty).cloned();

        // Check if callee is a function type
        match func_ty_opt {
            Some(crate::types::Type::Function(func)) => {
                // Check argument count
                if arg_types.len() != func.params.len() {
                    self.errors.push(CheckError::ArgumentCountMismatch {
                        expected: func.params.len(),
                        actual: arg_types.len(),
                        span: call.span,
                    });
                }

                // Check if this is a generic function (contains type variables)
                let is_generic = func.params.iter().any(|&p| contains_type_variables(self.type_ctx, p))
                    || contains_type_variables(self.type_ctx, func.return_type);

                if is_generic {
                    // Use type unification for generic functions
                    let mut gen_ctx = GenericContext::new(self.type_ctx);

                    // Unify each argument type with parameter type
                    for (i, (arg_ty, arg_span)) in arg_types.iter().enumerate() {
                        if let Some(&param_ty) = func.params.get(i) {
                            // Attempt unification
                            let _ = gen_ctx.unify(param_ty, *arg_ty);
                        }
                    }

                    // Apply substitutions to return type
                    match gen_ctx.apply_substitution(func.return_type) {
                        Ok(substituted_return) => substituted_return,
                        Err(_) => func.return_type,
                    }
                } else {
                    // Non-generic function - use simple type checking
                    for (i, (arg_ty, arg_span)) in arg_types.iter().enumerate() {
                        if let Some(&param_ty) = func.params.get(i) {
                            self.check_assignable(*arg_ty, param_ty, *arg_span);
                        }
                    }
                    func.return_type
                }
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

    /// Collect free variables from an expression
    fn collect_free_vars_expr(&self, expr: &Expression, collector: &mut FreeVariableCollector) {
        match expr {
            Expression::Identifier(ident) => {
                let name = self.resolve(ident.name);
                collector.reference(&name);
            }
            Expression::Binary(bin) => {
                self.collect_free_vars_expr(&bin.left, collector);
                self.collect_free_vars_expr(&bin.right, collector);
            }
            Expression::Logical(log) => {
                self.collect_free_vars_expr(&log.left, collector);
                self.collect_free_vars_expr(&log.right, collector);
            }
            Expression::Unary(un) => {
                self.collect_free_vars_expr(&un.operand, collector);
            }
            Expression::Assignment(assign) => {
                // LHS is an assignment target
                if let Expression::Identifier(ident) = assign.left.as_ref() {
                    let name = self.resolve(ident.name);
                    collector.assign(&name);
                } else {
                    self.collect_free_vars_expr(&assign.left, collector);
                }
                self.collect_free_vars_expr(&assign.right, collector);
            }
            Expression::Call(call) => {
                self.collect_free_vars_expr(&call.callee, collector);
                for arg in &call.arguments {
                    self.collect_free_vars_expr(arg, collector);
                }
            }
            Expression::Member(member) => {
                self.collect_free_vars_expr(&member.object, collector);
            }
            Expression::Index(idx) => {
                self.collect_free_vars_expr(&idx.object, collector);
                self.collect_free_vars_expr(&idx.index, collector);
            }
            Expression::Array(arr) => {
                for elem_opt in &arr.elements {
                    if let Some(elem) = elem_opt {
                        match elem {
                            ArrayElement::Expression(e) => self.collect_free_vars_expr(e, collector),
                            ArrayElement::Spread(e) => self.collect_free_vars_expr(e, collector),
                        }
                    }
                }
            }
            Expression::Object(obj) => {
                for prop in &obj.properties {
                    match prop {
                        ObjectProperty::Property(p) => {
                            self.collect_free_vars_expr(&p.value, collector);
                        }
                        ObjectProperty::Spread(spread) => {
                            self.collect_free_vars_expr(&spread.argument, collector);
                        }
                    }
                }
            }
            Expression::Conditional(cond) => {
                self.collect_free_vars_expr(&cond.test, collector);
                self.collect_free_vars_expr(&cond.consequent, collector);
                self.collect_free_vars_expr(&cond.alternate, collector);
            }
            Expression::Arrow(inner_arrow) => {
                // Nested arrow: create new collector for its body
                // but note what it captures from our scope
                let mut inner_collector = FreeVariableCollector::new();
                for param in &inner_arrow.params {
                    if let crate::ast::Pattern::Identifier(ident) = &param.pattern {
                        inner_collector.bind(self.resolve(ident.name));
                    }
                }
                match &inner_arrow.body {
                    ArrowBody::Expression(e) => self.collect_free_vars_expr(e, &mut inner_collector),
                    ArrowBody::Block(b) => self.collect_free_vars_block(b, &mut inner_collector),
                }
                // Free vars of inner closure that aren't bound locally are our free vars too
                for var in inner_collector.free_variables() {
                    if !collector.bound_vars.contains(var) {
                        if inner_collector.is_assigned(var) {
                            collector.assign(var);
                        } else {
                            collector.reference(var);
                        }
                    }
                }
            }
            Expression::Typeof(ty) => {
                self.collect_free_vars_expr(&ty.argument, collector);
            }
            Expression::TemplateLiteral(tpl) => {
                for part in &tpl.parts {
                    if let TemplatePart::Expression(expr) = part {
                        self.collect_free_vars_expr(expr, collector);
                    }
                }
            }
            // Literals don't have free variables
            Expression::IntLiteral(_)
            | Expression::FloatLiteral(_)
            | Expression::StringLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_) => {}
            // Other expressions - handle remaining cases
            _ => {}
        }
    }

    /// Collect free variables from a block
    fn collect_free_vars_block(&self, block: &BlockStatement, collector: &mut FreeVariableCollector) {
        for stmt in &block.statements {
            self.collect_free_vars_stmt(stmt, collector);
        }
    }

    /// Collect free variables from a statement
    fn collect_free_vars_stmt(&self, stmt: &Statement, collector: &mut FreeVariableCollector) {
        match stmt {
            Statement::Expression(expr_stmt) => {
                self.collect_free_vars_expr(&expr_stmt.expression, collector);
            }
            Statement::VariableDecl(decl) => {
                // Initializer is evaluated before binding
                if let Some(ref init) = decl.initializer {
                    self.collect_free_vars_expr(init, collector);
                }
                // Then bind the variable
                if let Pattern::Identifier(ident) = &decl.pattern {
                    collector.bind(self.resolve(ident.name));
                }
            }
            Statement::Return(ret) => {
                if let Some(ref val) = ret.value {
                    self.collect_free_vars_expr(val, collector);
                }
            }
            Statement::If(if_stmt) => {
                self.collect_free_vars_expr(&if_stmt.condition, collector);
                self.collect_free_vars_stmt(&if_stmt.then_branch, collector);
                if let Some(ref else_branch) = if_stmt.else_branch {
                    self.collect_free_vars_stmt(else_branch, collector);
                }
            }
            Statement::While(while_stmt) => {
                self.collect_free_vars_expr(&while_stmt.condition, collector);
                self.collect_free_vars_stmt(&while_stmt.body, collector);
            }
            Statement::For(for_stmt) => {
                if let Some(ref init) = for_stmt.init {
                    match init {
                        ForInit::VariableDecl(decl) => {
                            if let Some(ref init_expr) = decl.initializer {
                                self.collect_free_vars_expr(init_expr, collector);
                            }
                            if let Pattern::Identifier(ident) = &decl.pattern {
                                collector.bind(self.resolve(ident.name));
                            }
                        }
                        ForInit::Expression(e) => self.collect_free_vars_expr(e, collector),
                    }
                }
                if let Some(ref test) = for_stmt.test {
                    self.collect_free_vars_expr(test, collector);
                }
                if let Some(ref update) = for_stmt.update {
                    self.collect_free_vars_expr(update, collector);
                }
                self.collect_free_vars_stmt(&for_stmt.body, collector);
            }
            Statement::ForOf(for_of) => {
                self.collect_free_vars_expr(&for_of.right, collector);
                match &for_of.left {
                    ForOfLeft::VariableDecl(decl) => {
                        if let Pattern::Identifier(ident) = &decl.pattern {
                            collector.bind(self.resolve(ident.name));
                        }
                    }
                    ForOfLeft::Pattern(p) => {
                        if let Pattern::Identifier(ident) = p {
                            collector.assign(&self.resolve(ident.name));
                        }
                    }
                }
                self.collect_free_vars_stmt(&for_of.body, collector);
            }
            Statement::Block(block) => {
                self.collect_free_vars_block(block, collector);
            }
            Statement::Switch(switch_stmt) => {
                self.collect_free_vars_expr(&switch_stmt.discriminant, collector);
                for case in &switch_stmt.cases {
                    if let Some(ref test) = case.test {
                        self.collect_free_vars_expr(test, collector);
                    }
                    for stmt in &case.consequent {
                        self.collect_free_vars_stmt(stmt, collector);
                    }
                }
            }
            Statement::Try(try_stmt) => {
                self.collect_free_vars_block(&try_stmt.body, collector);
                if let Some(ref catch) = try_stmt.catch_clause {
                    // Catch variable is bound in catch block
                    if let Some(ref param) = catch.param {
                        if let Pattern::Identifier(ident) = param {
                            collector.bind(self.resolve(ident.name));
                        }
                    }
                    self.collect_free_vars_block(&catch.body, collector);
                }
                if let Some(ref finally) = try_stmt.finally_clause {
                    self.collect_free_vars_block(finally, collector);
                }
            }
            _ => {}
        }
    }

    /// Check arrow function
    fn check_arrow(&mut self, arrow: &crate::ast::ArrowFunction) -> TypeId {
        // Save current type environment - parameters are scoped to the arrow body
        let saved_env = self.type_env.clone();

        // Collect parameter names for binding
        let mut param_names = Vec::new();
        let mut param_types = Vec::new();

        for param in &arrow.params {
            let param_ty = param
                .type_annotation
                .as_ref()
                .map(|t| self.resolve_type_annotation(t))
                .unwrap_or_else(|| self.type_ctx.unknown_type());
            param_types.push(param_ty);

            // Add parameter to type environment so it can be resolved in body
            if let crate::ast::Pattern::Identifier(ident) = &param.pattern {
                let name = self.resolve(ident.name);
                param_names.push(name.clone());
                self.type_env.set(name, param_ty);
            }
        }

        // Collect free variables from the arrow body
        let mut collector = FreeVariableCollector::new();
        for name in &param_names {
            collector.bind(name.clone());
        }

        match &arrow.body {
            ArrowBody::Expression(expr) => {
                self.collect_free_vars_expr(expr, &mut collector);
            }
            ArrowBody::Block(block) => {
                self.collect_free_vars_block(block, &mut collector);
            }
        }

        // Build capture info from free variables
        let mut closure_captures = ClosureCaptures::new();
        for var_name in collector.free_variables() {
            // Look up the variable in outer scopes
            if let Some(symbol) = self.symbols.resolve_from_scope(var_name, self.current_scope) {
                // Check if this is actually from an outer scope (not global built-in)
                if symbol.scope_id.0 < self.current_scope.0 || symbol.scope_id == super::symbols::ScopeId(0) {
                    // Get the type (prefer inferred type if available)
                    let ty = self.inferred_var_types
                        .get(&(symbol.scope_id.0, var_name.clone()))
                        .copied()
                        .unwrap_or(symbol.ty);

                    closure_captures.add(CaptureInfo {
                        name: var_name.clone(),
                        ty,
                        defining_scope: symbol.scope_id,
                        is_mutated: collector.is_assigned(var_name),
                        capture_span: arrow.span, // Use arrow span as capture site
                    });
                }
            }
        }

        // Store capture info for this closure
        if !closure_captures.is_empty() {
            self.capture_info.insert(ClosureId(arrow.span), closure_captures);
        }

        // Determine return type
        let return_ty = match &arrow.body {
            crate::ast::ArrowBody::Expression(expr) => {
                // For expression body, the return type is the expression's type
                let expr_ty = self.check_expr(expr);
                arrow
                    .return_type
                    .as_ref()
                    .map(|t| self.resolve_type_annotation(t))
                    .unwrap_or(expr_ty)
            }
            crate::ast::ArrowBody::Block(block) => {
                // Check block statements
                for stmt in &block.statements {
                    self.check_stmt(stmt);
                }
                // Use the return type annotation or infer void
                arrow
                    .return_type
                    .as_ref()
                    .map(|t| self.resolve_type_annotation(t))
                    .unwrap_or_else(|| self.type_ctx.void_type())
            }
        };

        // Restore type environment
        self.type_env = saved_env;

        // Create function type
        self.type_ctx
            .function_type(param_types, return_ty, arrow.is_async)
    }

    /// Check index access
    fn check_index(&mut self, index: &crate::ast::IndexExpression) -> TypeId {
        let object_ty = self.check_expr(&index.object);
        let _index_ty = self.check_expr(&index.index);

        // Get element type if object is an array
        if let Some(crate::types::Type::Array(arr)) = self.type_ctx.get(object_ty) {
            arr.element
        } else {
            // For other types (objects with index signature), return unknown
            self.type_ctx.unknown_type()
        }
    }

    /// Check new expression (class instantiation)
    fn check_new(&mut self, new_expr: &crate::ast::NewExpression) -> TypeId {
        // Get the callee type (should be a class)
        if let Expression::Identifier(ident) = &*new_expr.callee {
            let name = self.resolve(ident.name);
            // Look up the class symbol to get its type
            if let Some(symbol) = self.symbols.resolve(&name) {
                if symbol.kind == SymbolKind::Class {
                    // Check constructor arguments (for now, just check them)
                    for arg in &new_expr.arguments {
                        self.check_expr(arg);
                    }
                    return symbol.ty;
                }
            }
        }
        // Check arguments even if we can't determine the class
        for arg in &new_expr.arguments {
            self.check_expr(arg);
        }
        self.type_ctx.unknown_type()
    }

    /// Check this expression
    fn check_this(&mut self) -> TypeId {
        // Return the current class type if we're inside a class method
        // For now, return unknown - this will be enhanced with class context
        self.type_ctx.unknown_type()
    }

    /// Check await expression
    fn check_await(&mut self, await_expr: &crate::ast::AwaitExpression) -> TypeId {
        // Check the argument expression
        let arg_ty = self.check_expr(&await_expr.argument);

        // If the argument is a Task<T>, return T
        if let Some(crate::types::Type::Task(task_ty)) = self.type_ctx.get(arg_ty) {
            return task_ty.result;
        }

        // If the argument is Task<T>[], return T[] (parallel await)
        if let Some(crate::types::Type::Array(arr_ty)) = self.type_ctx.get(arg_ty) {
            if let Some(crate::types::Type::Task(task_ty)) = self.type_ctx.get(arr_ty.element) {
                return self.type_ctx.array_type(task_ty.result);
            }
        }

        // Otherwise return the argument type (for compatibility)
        arg_ty
    }

    /// Check async call expression (async funcCall() syntax)
    fn check_async_call(&mut self, async_call: &crate::ast::AsyncCallExpression) -> TypeId {
        // Check all arguments
        for arg in &async_call.arguments {
            self.check_expr(arg);
        }

        // Get the callee's return type
        let callee_ty = self.check_expr(&async_call.callee);

        // If the callee is a function, get its return type and wrap in Task
        if let Some(crate::types::Type::Function(func_ty)) = self.type_ctx.get(callee_ty) {
            let return_ty = func_ty.return_type;
            return self.type_ctx.task_type(return_ty);
        }

        // Otherwise just return Task<unknown>
        let unknown = self.type_ctx.unknown_type();
        self.type_ctx.task_type(unknown)
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

        // Get the type for property lookup
        let obj_type = self.type_ctx.get(object_ty).cloned();

        // Check for built-in array methods
        if let Some(crate::types::Type::Array(arr)) = &obj_type {
            let elem_ty = arr.element;
            if let Some(method_type) = self.get_array_method_type(&property_name, elem_ty) {
                return method_type;
            }
        }

        // Check for built-in string methods
        if let Some(crate::types::Type::Primitive(crate::types::PrimitiveType::String)) = &obj_type {
            if let Some(method_type) = self.get_string_method_type(&property_name) {
                return method_type;
            }
        }

        // Check for class properties and methods
        if let Some(crate::types::Type::Class(class)) = &obj_type {
            // If this is a placeholder class type (empty methods), look up the symbol to get the full type
            let actual_class = if class.methods.is_empty() && class.properties.is_empty() {
                // Look up class by name in symbol table
                if let Some(symbol) = self.symbols.resolve(&class.name) {
                    if symbol.kind == SymbolKind::Class {
                        self.type_ctx.get(symbol.ty).and_then(|t| {
                            if let crate::types::Type::Class(c) = t {
                                Some(c.clone())
                            } else {
                                None
                            }
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            let class_to_use = actual_class.as_ref().unwrap_or(class);

            // Check properties
            for prop in &class_to_use.properties {
                if prop.name == property_name {
                    return prop.ty;
                }
            }
            // Check methods
            for method in &class_to_use.methods {
                if method.name == property_name {
                    return method.ty;
                }
            }
        }

        // For now, return unknown for other member access
        // TODO: Implement property type lookup for interfaces and other types
        self.type_ctx.unknown_type()
    }

    /// Get the type of a built-in array method
    fn get_array_method_type(&mut self, method_name: &str, elem_ty: TypeId) -> Option<TypeId> {
        let number_ty = self.type_ctx.number_type();
        let boolean_ty = self.type_ctx.boolean_type();

        match method_name {
            // push(value: T) -> number
            "push" => Some(self.type_ctx.function_type(vec![elem_ty], number_ty, false)),
            // pop() -> T
            "pop" => Some(self.type_ctx.function_type(vec![], elem_ty, false)),
            // shift() -> T
            "shift" => Some(self.type_ctx.function_type(vec![], elem_ty, false)),
            // unshift(value: T) -> number
            "unshift" => Some(self.type_ctx.function_type(vec![elem_ty], number_ty, false)),
            // indexOf(value: T) -> number
            "indexOf" => Some(self.type_ctx.function_type(vec![elem_ty], number_ty, false)),
            // includes(value: T) -> boolean
            "includes" => Some(self.type_ctx.function_type(vec![elem_ty], boolean_ty, false)),
            // length property (not a method, but handled here for convenience)
            "length" => Some(number_ty),
            _ => None,
        }
    }

    /// Get the type of a built-in string method
    fn get_string_method_type(&mut self, method_name: &str) -> Option<TypeId> {
        let number_ty = self.type_ctx.number_type();
        let string_ty = self.type_ctx.string_type();
        let boolean_ty = self.type_ctx.boolean_type();

        match method_name {
            // length property
            "length" => Some(number_ty),
            // charAt(index: number) -> string
            "charAt" => Some(self.type_ctx.function_type(vec![number_ty], string_ty, false)),
            // substring(start: number, end: number) -> string
            "substring" => Some(self.type_ctx.function_type(vec![number_ty, number_ty], string_ty, false)),
            // toUpperCase() -> string
            "toUpperCase" => Some(self.type_ctx.function_type(vec![], string_ty, false)),
            // toLowerCase() -> string
            "toLowerCase" => Some(self.type_ctx.function_type(vec![], string_ty, false)),
            // trim() -> string
            "trim" => Some(self.type_ctx.function_type(vec![], string_ty, false)),
            // indexOf(searchStr: string) -> number
            "indexOf" => Some(self.type_ctx.function_type(vec![string_ty], number_ty, false)),
            // includes(searchStr: string) -> boolean
            "includes" => Some(self.type_ctx.function_type(vec![string_ty], boolean_ty, false)),
            _ => None,
        }
    }

    /// Check array literal
    fn check_array(&mut self, arr: &ArrayExpression) -> TypeId {
        if arr.elements.is_empty() {
            // Empty array - infer as never[]
            // never is the bottom type, so never[] <: T[] for any T
            // This allows empty arrays to be assigned to any typed array
            let never = self.type_ctx.never_type();
            return self.type_ctx.array_type(never);
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

    /// Resolve a type annotation to a TypeId
    fn resolve_type_annotation(&mut self, ty_annot: &TypeAnnotation) -> TypeId {
        self.resolve_type(&ty_annot.ty)
    }

    /// Resolve a type AST node to a TypeId
    fn resolve_type(&mut self, ty: &crate::ast::Type) -> TypeId {
        use crate::ast::Type as AstType;

        match ty {
            AstType::Primitive(prim) => self.resolve_primitive(*prim),

            AstType::Reference(type_ref) => {
                // Check if it's a user-defined type or type parameter
                let name = self.resolve(type_ref.name.name);

                // Handle built-in generic types
                if name == "Array" {
                    if let Some(ref type_args) = type_ref.type_args {
                        if type_args.len() == 1 {
                            let elem_ty = self.resolve_type_annotation(&type_args[0]);
                            return self.type_ctx.array_type(elem_ty);
                        }
                    }
                    // Invalid Array usage - return unknown
                    return self.type_ctx.unknown_type();
                }

                // Handle Task<T> for async functions
                if name == "Task" {
                    if let Some(ref type_args) = type_ref.type_args {
                        if type_args.len() == 1 {
                            let result_ty = self.resolve_type_annotation(&type_args[0]);
                            return self.type_ctx.task_type(result_ty);
                        }
                    }
                    // Invalid Task usage - return unknown
                    return self.type_ctx.unknown_type();
                }

                if let Some(symbol) = self.symbols.resolve_from_scope(&name, self.current_scope) {
                    symbol.ty
                } else {
                    // Type not found - return unknown
                    self.type_ctx.unknown_type()
                }
            }

            AstType::Array(arr) => {
                let elem_ty = self.resolve_type_annotation(&arr.element_type);
                self.type_ctx.array_type(elem_ty)
            }

            AstType::Tuple(tuple) => {
                let elem_tys: Vec<_> = tuple
                    .element_types
                    .iter()
                    .map(|e| self.resolve_type_annotation(e))
                    .collect();
                self.type_ctx.tuple_type(elem_tys)
            }

            AstType::Union(union) => {
                let member_tys: Vec<_> = union
                    .types
                    .iter()
                    .map(|t| self.resolve_type_annotation(t))
                    .collect();
                self.type_ctx.union_type(member_tys)
            }

            AstType::Function(func) => {
                let param_tys: Vec<_> = func
                    .params
                    .iter()
                    .map(|p| self.resolve_type_annotation(&p.ty))
                    .collect();

                let return_ty = self.resolve_type_annotation(&func.return_type);

                self.type_ctx.function_type(param_tys, return_ty, false)
            }

            AstType::Object(_obj) => {
                // TODO: Implement object type resolution
                self.type_ctx.unknown_type()
            }

            AstType::Typeof(_) => {
                // typeof types are resolved during type checking
                self.type_ctx.unknown_type()
            }

            AstType::StringLiteral(s) => {
                self.type_ctx.string_literal(self.interner.resolve(*s).to_string())
            }

            AstType::NumberLiteral(n) => {
                self.type_ctx.number_literal(*n)
            }

            AstType::BooleanLiteral(b) => {
                self.type_ctx.boolean_literal(*b)
            }

            AstType::Parenthesized(inner) => {
                self.resolve_type_annotation(inner)
            }
        }
    }

    /// Resolve a primitive type to TypeId
    fn resolve_primitive(&mut self, prim: crate::ast::PrimitiveType) -> TypeId {
        use crate::ast::PrimitiveType as AstPrim;

        match prim {
            AstPrim::Number => self.type_ctx.number_type(),
            AstPrim::String => self.type_ctx.string_type(),
            AstPrim::Boolean => self.type_ctx.boolean_type(),
            AstPrim::Null => self.type_ctx.null_type(),
            AstPrim::Void => self.type_ctx.void_type(),
        }
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
        checker.check_module(&module).map(|_| ())
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
