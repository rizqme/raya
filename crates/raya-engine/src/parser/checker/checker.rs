//! Type checker - validates types for expressions and statements
//!
//! The type checker walks the AST and verifies that all operations are
//! type-safe. It uses the symbol table for name resolution and the type
//! context for type operations.

use super::error::{CheckError, CheckWarning};
use super::symbols::{SymbolKind, SymbolTable};
use super::type_guards::{extract_type_guard, TypeGuard};
use super::narrowing::{apply_type_guard, TypeEnv};
use super::exhaustiveness::{check_switch_exhaustiveness, ExhaustivenessResult};
use super::captures::{CaptureInfo, ClosureCaptures, ClosureId, ModuleCaptureInfo, FreeVariableCollector};
use crate::parser::ast::*;
use crate::{Interner, Symbol as ParserSymbol};
use crate::parser::types::{AssignabilityContext, GenericContext, TypeContext, TypeId};
use crate::parser::types::normalize::contains_type_variables;
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
        TypeGuard::Truthiness { var, .. } => var,
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
    /// Expression types: maps expression ID (ptr as usize) to TypeId
    pub expr_types: FxHashMap<usize, TypeId>,
    /// Warnings collected during type checking
    pub warnings: Vec<CheckWarning>,
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
        TypeGuard::Truthiness { var, negated } => TypeGuard::Truthiness {
            var: var.clone(),
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

    /// Current class type for checking `this` expressions
    current_class_type: Option<TypeId>,

    /// Whether we are inside a constructor body (readonly fields can be assigned)
    in_constructor: bool,

    /// Depth counter for arrow function bodies
    /// When > 0, scope enter/exit are no-ops because the binder never visits
    /// inside expressions, so arrow body scopes don't exist in the symbol table.
    arrow_depth: u32,

    /// Warnings collected during type checking
    warnings: Vec<CheckWarning>,
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
            current_class_type: None,
            in_constructor: false,
            arrow_depth: 0,
            warnings: Vec::new(),
        }
    }

    /// Resolve a parser Symbol to a String
    #[inline]
    fn resolve(&self, sym: ParserSymbol) -> String {
        self.interner.resolve(sym).to_string()
    }

    /// Enter a new scope (like entering a block or function)
    /// Mirrors what the binder does when it pushes a scope.
    /// No-op when inside arrow function bodies (arrow_depth > 0) because
    /// the binder never visits inside expressions, so those scopes don't
    /// exist in the symbol table.
    fn enter_scope(&mut self) {
        if self.arrow_depth > 0 {
            return;
        }
        self.scope_stack.push(self.current_scope);
        let scope_id = super::symbols::ScopeId(self.next_scope_id);
        self.next_scope_id += 1;
        self.current_scope = scope_id;
    }

    /// Exit the current scope, returning to parent
    fn exit_scope(&mut self) {
        if self.arrow_depth > 0 {
            return;
        }
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

        // Collect unused variable warnings
        self.collect_unused_warnings();

        if self.errors.is_empty() {
            Ok(CheckResult {
                inferred_types: self.inferred_var_types,
                captures: self.capture_info,
                expr_types: self.expr_types,
                warnings: self.warnings,
            })
        } else {
            Err(self.errors)
        }
    }

    /// Collect warnings for unused variables across all scopes
    fn collect_unused_warnings(&mut self) {
        use super::symbols::SymbolKind;

        for scope in self.symbols.all_scopes() {
            for symbol in scope.symbols.values() {
                // Only warn about variables (not functions, classes, types, etc.)
                if symbol.kind != SymbolKind::Variable {
                    continue;
                }

                // Skip if already referenced
                if symbol.referenced {
                    continue;
                }

                // Skip _-prefixed names (intentionally unused)
                if symbol.name.starts_with('_') {
                    continue;
                }

                // Skip exported symbols
                if symbol.flags.is_exported {
                    continue;
                }

                self.warnings.push(CheckWarning::UnusedVariable {
                    name: symbol.name.clone(),
                    span: symbol.span,
                });
            }
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
            Statement::ClassDecl(class) => {
                // Check class declaration including decorators
                self.check_class(class);
            }
            Statement::TypeAliasDecl(alias) => {
                // Sync scope for generic type aliases (binder creates a scope for type params)
                if alias.type_params.as_ref().map_or(false, |p| !p.is_empty()) {
                    self.enter_scope();
                    self.exit_scope();
                }
            }
            _ => {}
        }
    }

    /// Check variable declaration
    fn check_var_decl(&mut self, decl: &VariableDecl) {
        if let Some(ref init) = decl.initializer {
            let init_ty = self.check_expr(init);

            match &decl.pattern {
                Pattern::Identifier(ident) => {
                    let name = self.resolve(ident.name);

                    // Determine the variable's type
                    let var_ty = if decl.type_annotation.is_some() {
                        // Get the declared type from symbol table
                        if let Some(symbol) = self.symbols.resolve_from_scope(&name, self.current_scope) {
                            self.check_assignable(init_ty, symbol.ty, *init.span());
                            symbol.ty
                        } else {
                            // Inside arrow bodies, the binder never visited — resolve
                            // the annotation and store in inferred_var_types so
                            // subsequent references can find this variable
                            let resolved_ty = self.resolve_type_annotation(decl.type_annotation.as_ref().unwrap());
                            self.check_assignable(init_ty, resolved_ty, *init.span());
                            self.inferred_var_types.insert(
                                (self.current_scope.0, name.clone()),
                                resolved_ty,
                            );
                            resolved_ty
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
                Pattern::Array(_) | Pattern::Object(_) => {
                    // For destructuring, infer element/property types from the
                    // initializer and register them for each binding.
                    self.check_destructure_pattern(&decl.pattern, init_ty);
                }
                _ => {}
            }
        }
    }

    /// Infer types for variables bound in a destructuring pattern.
    fn check_destructure_pattern(&mut self, pattern: &Pattern, value_ty: TypeId) {
        match pattern {
            Pattern::Identifier(ident) => {
                let name = self.resolve(ident.name);
                self.inferred_var_types.insert(
                    (self.current_scope.0, name.clone()),
                    value_ty,
                );
                self.type_env.set(name, value_ty);
            }
            Pattern::Array(array_pat) => {
                // Extract element type from the array type
                let elem_ty = if let Some(crate::parser::types::Type::Array(arr)) = self.type_ctx.get(value_ty).cloned() {
                    arr.element
                } else {
                    self.type_ctx.unknown_type()
                };

                for elem_opt in &array_pat.elements {
                    if let Some(elem) = elem_opt {
                        self.check_destructure_pattern(&elem.pattern, elem_ty);
                    }
                }
                if let Some(rest) = &array_pat.rest {
                    // Rest element gets the same array type
                    self.check_destructure_pattern(rest, value_ty);
                }
            }
            Pattern::Object(obj_pat) => {
                // Look up class fields from value type
                let class_props: Option<Vec<crate::parser::types::ty::PropertySignature>> =
                    if let Some(crate::parser::types::Type::Class(class)) = self.type_ctx.get(value_ty).cloned() {
                        Some(class.properties.clone())
                    } else {
                        None
                    };

                for prop in &obj_pat.properties {
                    let prop_name = self.resolve(prop.key.name);
                    let prop_ty = class_props.as_ref()
                        .and_then(|props| props.iter().find(|p| p.name == prop_name))
                        .map(|p| p.ty);
                    // If there's a default expression, use its type when property is missing
                    let final_ty = if let Some(ref default_expr) = prop.default {
                        let default_ty = self.check_expr(default_expr);
                        prop_ty.unwrap_or(default_ty)
                    } else {
                        prop_ty.unwrap_or_else(|| self.type_ctx.unknown_type())
                    };
                    self.check_destructure_pattern(&prop.value, final_ty);
                }
                if let Some(rest_ident) = &obj_pat.rest {
                    let unknown = self.type_ctx.unknown_type();
                    let name = self.resolve(rest_ident.name);
                    self.inferred_var_types.insert(
                        (self.current_scope.0, name.clone()),
                        unknown,
                    );
                    self.type_env.set(name, unknown);
                }
            }
            Pattern::Rest(rest_pat) => {
                self.check_destructure_pattern(&rest_pat.argument, value_ty);
            }
        }
    }

    /// Check function declaration
    fn check_function(&mut self, func: &FunctionDecl) {
        // Get return type from symbol table
        let func_name = self.resolve(func.name.name);
        if let Some(symbol) = self.symbols.resolve_from_scope(&func_name, self.current_scope) {
            if let Some(crate::parser::types::Type::Function(func_ty)) = self.type_ctx.get(symbol.ty) {
                let mut return_ty = func_ty.return_type;

                // For async functions, the declared return type is Task<T>,
                // but return statements should check against T (the inner type)
                if func.is_async {
                    if let Some(crate::parser::types::Type::Task(task_ty)) = self.type_ctx.get(return_ty) {
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

    /// Sync scopes for class declaration to keep scope IDs in sync with binder
    /// This mirrors the binder's scope creation pattern without doing type checking
    fn sync_class_scopes(&mut self, class: &crate::parser::ast::ClassDecl) {
        // Enter class scope (mirrors binder's push_scope for class at line 687)
        self.enter_scope();

        // Mirror binder's temporary scope for methods with type params during type resolution
        // (binder lines 743-778)
        for member in &class.members {
            if let crate::parser::ast::ClassMember::Method(method) = member {
                if method.type_params.as_ref().map_or(false, |tps| !tps.is_empty()) {
                    self.enter_scope();
                    self.exit_scope();
                }
            }
        }

        // Mirror binder's scope for method/constructor bodies (binder lines 833-871)
        for member in &class.members {
            match member {
                crate::parser::ast::ClassMember::Method(method) => {
                    if method.body.is_some() {
                        self.enter_scope();
                        // Recursively count nested scopes without type checking
                        if let Some(ref body) = method.body {
                            self.sync_stmts_scopes(&body.statements);
                        }
                        self.exit_scope();
                    }
                }
                crate::parser::ast::ClassMember::Constructor(ctor) => {
                    self.enter_scope();
                    self.sync_stmts_scopes(&ctor.body.statements);
                    self.exit_scope();
                }
                _ => {}
            }
        }

        // Exit class scope (mirrors binder's pop_scope at line 873)
        self.exit_scope();
    }

    /// Sync scopes for statements without doing type checking
    /// Just enters/exits scopes to keep scope IDs in sync with binder
    fn sync_stmts_scopes(&mut self, stmts: &[Statement]) {
        for stmt in stmts {
            self.sync_stmt_scopes(stmt);
        }
    }

    /// Sync scopes for a single statement without type checking
    fn sync_stmt_scopes(&mut self, stmt: &Statement) {
        match stmt {
            Statement::Block(block) => {
                self.enter_scope();
                self.sync_stmts_scopes(&block.statements);
                self.exit_scope();
            }
            Statement::FunctionDecl(func) => {
                self.enter_scope();
                self.sync_stmts_scopes(&func.body.statements);
                self.exit_scope();
            }
            Statement::While(while_stmt) => {
                self.enter_scope();
                self.sync_stmt_scopes(&while_stmt.body);
                self.exit_scope();
            }
            Statement::For(for_stmt) => {
                self.enter_scope();
                self.sync_stmt_scopes(&for_stmt.body);
                self.exit_scope();
            }
            Statement::ForOf(for_of) => {
                self.enter_scope();
                self.sync_stmt_scopes(&for_of.body);
                self.exit_scope();
            }
            Statement::If(if_stmt) => {
                // If body doesn't create scope unless it's a block
                self.sync_stmt_scopes(&if_stmt.then_branch);
                if let Some(ref alt) = if_stmt.else_branch {
                    self.sync_stmt_scopes(alt);
                }
            }
            Statement::Try(try_stmt) => {
                // Try body doesn't create a scope itself
                for s in &try_stmt.body.statements {
                    self.sync_stmt_scopes(s);
                }
                if let Some(ref catch) = try_stmt.catch_clause {
                    self.enter_scope();
                    self.sync_stmts_scopes(&catch.body.statements);
                    self.exit_scope();
                }
                if let Some(ref finally) = try_stmt.finally_clause {
                    for s in &finally.statements {
                        self.sync_stmt_scopes(s);
                    }
                }
            }
            Statement::Switch(switch_stmt) => {
                // Switch doesn't create scope, but cases might have blocks
                // Note: default case is included in cases with test: None
                for case in &switch_stmt.cases {
                    for s in &case.consequent {
                        self.sync_stmt_scopes(s);
                    }
                }
            }
            Statement::ClassDecl(class) => {
                self.sync_class_scopes(class);
            }
            Statement::TypeAliasDecl(alias) => {
                // Sync scope for generic type aliases (binder creates a scope for type params)
                if alias.type_params.as_ref().map_or(false, |p| !p.is_empty()) {
                    self.enter_scope();
                    self.exit_scope();
                }
            }
            // Other statements don't create scopes
            _ => {}
        }
    }

    /// Check class declaration
    ///
    /// This checks decorators on the class, methods, fields, and parameters,
    /// then syncs scopes with the binder for all nested code.
    fn check_class(&mut self, class: &crate::parser::ast::ClassDecl) {
        // Check class decorators first (before entering class scope)
        self.check_class_decorators(class);

        // Get the class type for 'this' checking inside methods
        let class_name = self.resolve(class.name.name);
        let prev_class_type = self.current_class_type;
        if let Some(symbol) = self.symbols.resolve_from_scope(&class_name, self.current_scope) {
            self.current_class_type = Some(symbol.ty);
        }

        // Check decorators on methods, fields, and constructors
        for member in &class.members {
            match member {
                crate::parser::ast::ClassMember::Method(method) => {
                    // Build method type for decorator checking
                    let method_ty = self.build_method_type(method);
                    // Check method decorators
                    self.check_method_decorators(method, method_ty);
                    // Check parameter decorators
                    for param in &method.params {
                        self.check_parameter_decorators(param);
                    }
                }
                crate::parser::ast::ClassMember::Constructor(ctor) => {
                    // Check parameter decorators
                    for param in &ctor.params {
                        self.check_parameter_decorators(param);
                    }
                }
                crate::parser::ast::ClassMember::Field(field) => {
                    self.check_field_decorators(field);
                }
                _ => {}
            }
        }

        // Restore previous class type (for nested classes)
        self.current_class_type = prev_class_type;

        // Now sync all scopes to keep scope IDs in sync with binder
        // This uses the existing sync_class_scopes logic
        self.sync_class_scopes(class);
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

    /// Get the effective type of a variable, checking type_env, inferred_var_types, and symbol table
    fn get_var_type(&self, name: &str) -> Option<TypeId> {
        // First check for narrowed type in type environment
        if let Some(ty) = self.type_env.get(name) {
            return Some(ty);
        }

        // Look up in symbol table and check for inferred type
        if let Some(symbol) = self.symbols.resolve_from_scope(name, self.current_scope) {
            // Check if we have an inferred type for this variable
            let scope_id = symbol.scope_id.0;
            if let Some(&inferred_ty) = self.inferred_var_types.get(&(scope_id, name.to_string())) {
                return Some(inferred_ty);
            }
            return Some(symbol.ty);
        }

        None
    }

    /// Get the declared type of a variable, bypassing narrowing.
    /// Used for assignment targets where the original type (not narrowed) should be checked.
    fn get_var_declared_type(&self, name: &str) -> Option<TypeId> {
        if let Some(symbol) = self.symbols.resolve_from_scope(name, self.current_scope) {
            let scope_id = symbol.scope_id.0;
            if let Some(&inferred_ty) = self.inferred_var_types.get(&(scope_id, name.to_string())) {
                return Some(inferred_ty);
            }
            return Some(symbol.ty);
        }
        // Fallback: search inferred_var_types directly for variables declared inside
        // arrow bodies that the binder never visited (no symbol table entries)
        if self.arrow_depth > 0 {
            // Check inferred type at current scope (arrow bodies store at outer scope)
            if let Some(&ty) = self.inferred_var_types.get(&(self.current_scope.0, name.to_string())) {
                return Some(ty);
            }
            // Walk up the scope stack
            for scope_id in self.scope_stack.iter().rev() {
                if let Some(&ty) = self.inferred_var_types.get(&(scope_id.0, name.to_string())) {
                    return Some(ty);
                }
            }
        }
        None
    }

    /// Try to extract an instanceof guard from a condition expression.
    /// Returns (variable_name, class_type_id) if the condition is `var instanceof ClassName`.
    fn try_extract_instanceof_guard(&self, expr: &Expression) -> Option<(String, TypeId)> {
        let instanceof = match expr {
            Expression::InstanceOf(inst) => inst,
            _ => return None,
        };
        // Object must be an identifier
        let var_name = match &*instanceof.object {
            Expression::Identifier(ident) => self.resolve(ident.name),
            _ => return None,
        };
        // Resolve class name from type annotation
        let class_name = match &instanceof.type_name.ty {
            crate::parser::ast::types::Type::Reference(type_ref) => self.resolve(type_ref.name.name),
            _ => return None,
        };
        // Look up the class type in the symbol table
        let class_sym = self.symbols.resolve(&class_name)?;
        Some((var_name, class_sym.ty))
    }

    /// Returns true if the statement definitely exits (return/throw).
    fn stmt_definitely_returns(stmt: &Statement) -> bool {
        match stmt {
            Statement::Return(_) | Statement::Throw(_) => true,
            Statement::Block(block) => {
                block.statements.last().map_or(false, Self::stmt_definitely_returns)
            }
            Statement::If(if_stmt) => {
                let then_returns = Self::stmt_definitely_returns(&if_stmt.then_branch);
                let else_returns = if_stmt.else_branch.as_ref()
                    .map_or(false, |e| Self::stmt_definitely_returns(e));
                then_returns && else_returns
            }
            _ => false,
        }
    }

    /// Check if statement
    fn check_if(&mut self, if_stmt: &IfStatement) {
        // Check condition — allow any type (truthiness), not just boolean
        let cond_ty = self.check_expr(&if_stmt.condition);
        let bool_ty = self.type_ctx.boolean_type();
        // Only enforce boolean for non-union, non-nullable types
        // TypeScript allows any expression in if-conditions (truthiness)
        let is_union_or_nullable = matches!(
            self.type_ctx.get(cond_ty),
            Some(crate::parser::types::Type::Union(_))
                | Some(crate::parser::types::Type::Primitive(crate::parser::types::PrimitiveType::String))
                | Some(crate::parser::types::Type::Primitive(crate::parser::types::PrimitiveType::Number))
        );
        if !is_union_or_nullable {
            self.check_assignable(cond_ty, bool_ty, *if_stmt.condition.span());
        }

        // Try to extract type guard from condition
        let type_guard = extract_type_guard(&if_stmt.condition, self.interner);

        // Try to extract instanceof guard (needs symbol table, so done in checker)
        let instanceof_guard = self.try_extract_instanceof_guard(&if_stmt.condition);

        // Save current environment
        let saved_env = self.type_env.clone();

        // Apply type guard for then branch
        if let Some(ref guard) = type_guard {
            let var_name = get_guard_var(guard);
            // Get the actual type of the variable (including inferred types)
            if let Some(var_ty) = self.get_var_type(var_name) {
                if let Some(narrowed_ty) = apply_type_guard(self.type_ctx, var_ty, guard) {
                    self.type_env.set(var_name.clone(), narrowed_ty);
                }
            }
        } else if let Some((ref var_name, class_ty)) = instanceof_guard {
            // instanceof narrows variable to the target class type
            self.type_env.set(var_name.clone(), class_ty);
        }

        // Check then branch
        self.check_stmt(&if_stmt.then_branch);
        let then_env = self.type_env.clone();

        // Check if the then-branch definitely exits (return/throw).
        // If so, code after the if can only be reached when condition was false.
        let then_returns = Self::stmt_definitely_returns(&if_stmt.then_branch);

        // Restore environment and apply negated guard for else branch
        self.type_env = saved_env.clone();

        if let Some(ref else_branch) = if_stmt.else_branch {
            if let Some(ref guard) = type_guard {
                // Apply negated guard
                let negated_guard = negate_guard(guard);
                let var_name = get_guard_var(&negated_guard);
                if let Some(var_ty) = self.get_var_type(var_name) {
                    if let Some(narrowed_ty) = apply_type_guard(self.type_ctx, var_ty, &negated_guard) {
                        self.type_env.set(var_name.clone(), narrowed_ty);
                    }
                }
            }

            self.check_stmt(else_branch);
        } else if then_returns {
            // No else branch but then-branch always returns.
            // Code after the if-statement can only run when the condition was false,
            // so apply the negated guard to narrow the continuation.
            if let Some(ref guard) = type_guard {
                let negated_guard = negate_guard(guard);
                let var_name = get_guard_var(&negated_guard);
                if let Some(var_ty) = self.get_var_type(var_name) {
                    if let Some(narrowed_ty) = apply_type_guard(self.type_ctx, var_ty, &negated_guard) {
                        self.type_env.set(var_name.clone(), narrowed_ty);
                    }
                }
            }
        }

        let else_env = self.type_env.clone();

        if then_returns && if_stmt.else_branch.is_none() {
            // Then-branch always exits, no else: continuation uses the narrowed env
            self.type_env = else_env;
        } else {
            // Normal merge of both branches
            self.type_env = then_env.merge(&else_env, self.type_ctx);
        }
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
            let var_name = get_guard_var(guard);
            // Get the actual type of the variable (including inferred types)
            if let Some(var_ty) = self.get_var_type(var_name) {
                if let Some(narrowed_ty) = apply_type_guard(self.type_ctx, var_ty, guard) {
                    self.type_env.set(var_name.clone(), narrowed_ty);
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
        let elem_ty = if let Some(crate::parser::types::Type::Array(arr)) = self.type_ctx.get(iterable_ty) {
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
                match &decl.pattern {
                    Pattern::Identifier(ident) => {
                        let name = self.resolve(ident.name);
                        // Store inferred type for the loop variable
                        self.inferred_var_types.insert(
                            (self.current_scope.0, name),
                            elem_ty
                        );
                    }
                    Pattern::Array(_) | Pattern::Object(_) => {
                        self.check_destructure_pattern(&decl.pattern, elem_ty);
                    }
                    _ => {}
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
            Expression::InstanceOf(instanceof) => self.check_instanceof(instanceof),
            Expression::TypeCast(cast) => self.check_type_cast(cast),
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

        // Check for builtin functions
        if name == "sleep" {
            // sleep(ms: number): void
            let number_ty = self.type_ctx.number_type();
            let void_ty = self.type_ctx.void_type();
            return self.type_ctx.function_type(vec![number_ty], void_ty, false);
        }

        // Check for __OPCODE_* intrinsics
        if name.starts_with("__OPCODE_") || name == "__NATIVE_CALL" {
            // Return a callable type - the actual return type is handled in try_check_intrinsic
            let any_ty = self.type_ctx.unknown_type();
            return self.type_ctx.function_type(vec![], any_ty, false);
        }

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
                // Inside arrow bodies, variables may be declared in this body
                // but not in the binder's symbol table. Check inferred_var_types.
                if self.arrow_depth > 0 {
                    if let Some(&ty) = self.inferred_var_types.get(&(self.current_scope.0, name.clone())) {
                        return ty;
                    }
                    for scope_id in self.scope_stack.iter().rev() {
                        if let Some(&ty) = self.inferred_var_types.get(&(scope_id.0, name.clone())) {
                            return ty;
                        }
                    }
                }
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
                let int_ty = self.type_ctx.int_type();

                // Check if either operand IS a string type (exact match or assignable)
                let left_is_string = left_ty == string_ty;
                let right_is_string = right_ty == string_ty;

                if left_is_string || right_is_string {
                    // String concatenation - both operands should be strings
                    self.check_assignable(left_ty, string_ty, *bin.left.span());
                    self.check_assignable(right_ty, string_ty, *bin.right.span());
                    string_ty
                } else if left_ty == int_ty && right_ty == int_ty {
                    // int + int = int
                    int_ty
                } else {
                    // Numeric addition (mixed int/number promotes to number)
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
                // Arithmetic operations require numeric operands
                let number_ty = self.type_ctx.number_type();
                let int_ty = self.type_ctx.int_type();
                if left_ty == int_ty && right_ty == int_ty {
                    // int op int = int
                    int_ty
                } else {
                    self.check_assignable(left_ty, number_ty, *bin.left.span());
                    self.check_assignable(right_ty, number_ty, *bin.right.span());
                    number_ty
                }
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
                // Bitwise operations require numeric operands and produce int
                let number_ty = self.type_ctx.number_type();
                self.check_assignable(left_ty, number_ty, *bin.left.span());
                self.check_assignable(right_ty, number_ty, *bin.right.span());
                self.type_ctx.int_type()
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
        // Check for compiler intrinsics first
        if let Some(intrinsic_ty) = self.try_check_intrinsic(call) {
            // Still type-check the arguments for error detection
            for arg in &call.arguments {
                self.check_expr(arg);
            }
            return intrinsic_ty;
        }

        let callee_ty = self.check_expr(&call.callee);

        // Check all argument types first (before creating GenericContext)
        let arg_types: Vec<(TypeId, crate::parser::Span)> = call.arguments.iter()
            .map(|arg| (self.check_expr(arg), *arg.span()))
            .collect();

        // Clone the function type to avoid borrow checker issues
        let func_ty_opt = self.type_ctx.get(callee_ty).cloned();

        // Check if callee is a function type
        match func_ty_opt {
            Some(crate::parser::types::Type::Function(func)) => {
                // Check argument count (too many or too few required)
                if arg_types.len() > func.params.len() || arg_types.len() < func.min_params {
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
                    if let crate::parser::ast::Pattern::Identifier(ident) = &param.pattern {
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
    fn check_arrow(&mut self, arrow: &crate::parser::ast::ArrowFunction) -> TypeId {
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
            if let crate::parser::ast::Pattern::Identifier(ident) = &param.pattern {
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
        // Determine declared return type (if any)
        let declared_return_ty = arrow
            .return_type
            .as_ref()
            .map(|t| self.resolve_type_annotation(t));

        // Arrow bodies may contain scope-creating constructs (while loops, blocks)
        // that the binder never visited. Increment arrow_depth to make enter_scope/
        // exit_scope no-ops, keeping the checker's scope IDs in sync with the binder.
        self.arrow_depth += 1;

        let return_ty = match &arrow.body {
            crate::parser::ast::ArrowBody::Expression(expr) => {
                // For expression body, the return type is the expression's type
                let expr_ty = self.check_expr(expr);
                declared_return_ty.unwrap_or(expr_ty)
            }
            crate::parser::ast::ArrowBody::Block(block) => {
                // Save and set return type for return statement checking
                let prev_return_ty = self.current_function_return_type;

                // For async arrows, unwrap Task<T> → T so return statements
                // are checked against T (same logic as check_function)
                let effective_return_ty = if arrow.is_async {
                    declared_return_ty.and_then(|ty| {
                        if let Some(crate::parser::types::Type::Task(task_ty)) =
                            self.type_ctx.get(ty)
                        {
                            Some(task_ty.result)
                        } else {
                            Some(ty)
                        }
                    })
                } else {
                    declared_return_ty
                };

                self.current_function_return_type = effective_return_ty;

                // Check block statements
                for stmt in &block.statements {
                    self.check_stmt(stmt);
                }

                // Restore previous return type
                self.current_function_return_type = prev_return_ty;

                // Use the effective return type or infer void
                effective_return_ty.unwrap_or_else(|| self.type_ctx.void_type())
            }
        };

        self.arrow_depth -= 1;

        // Restore type environment
        self.type_env = saved_env;

        // Create function type
        self.type_ctx
            .function_type(param_types, return_ty, arrow.is_async)
    }

    /// Check index access
    fn check_index(&mut self, index: &crate::parser::ast::IndexExpression) -> TypeId {
        let object_ty = self.check_expr(&index.object);
        let _index_ty = self.check_expr(&index.index);

        // Get element type if object is an array
        if let Some(crate::parser::types::Type::Array(arr)) = self.type_ctx.get(object_ty) {
            arr.element
        } else {
            // For other types (objects with index signature), return unknown
            self.type_ctx.unknown_type()
        }
    }

    /// Check new expression (class instantiation)
    fn check_new(&mut self, new_expr: &crate::parser::ast::NewExpression) -> TypeId {
        // Get the callee type (should be a class)
        if let Expression::Identifier(ident) = &*new_expr.callee {
            let name = self.resolve(ident.name);

            // Resolve type arguments if present
            let resolved_type_args: Vec<TypeId> = new_expr.type_args.as_ref()
                .map(|args| args.iter().map(|arg| self.resolve_type_annotation(arg)).collect())
                .unwrap_or_default();

            // Check for built-in types with type parameters
            // Note: Mutex is now a normal class from Mutex.raya, not special-cased
            let builtin_type = match name.as_str() {
                "RegExp" => Some(self.type_ctx.regexp_type()),
                "Map" => {
                    // Map<K, V> - expect 2 type arguments
                    if resolved_type_args.len() == 2 {
                        Some(self.type_ctx.map_type_with(resolved_type_args[0], resolved_type_args[1]))
                    } else {
                        Some(self.type_ctx.map_type())
                    }
                }
                "Set" => {
                    // Set<T> - expect 1 type argument
                    if resolved_type_args.len() == 1 {
                        Some(self.type_ctx.set_type_with(resolved_type_args[0]))
                    } else {
                        Some(self.type_ctx.set_type())
                    }
                }
                // Note: Buffer and Date are now normal classes from their .raya files
                "Channel" => {
                    // Channel<T> - expect 1 type argument
                    if resolved_type_args.len() == 1 {
                        Some(self.type_ctx.channel_type_with(resolved_type_args[0]))
                    } else {
                        Some(self.type_ctx.channel_type())
                    }
                }
                _ => None,
            };

            if let Some(ty) = builtin_type {
                // Check constructor arguments
                for arg in &new_expr.arguments {
                    self.check_expr(arg);
                }
                return ty;
            }

            // Look up the class symbol to get its type
            if let Some(symbol) = self.symbols.resolve_from_scope(&name, self.current_scope) {
                if symbol.kind == SymbolKind::Class {
                    // Check if the class is abstract (cannot be instantiated)
                    if let Some(crate::parser::types::Type::Class(class)) = self.type_ctx.get(symbol.ty).cloned() {
                        if class.is_abstract {
                            self.errors.push(CheckError::AbstractClassInstantiation {
                                name: name.clone(),
                                span: new_expr.span,
                            });
                        }
                    }

                    // Check constructor arguments (for now, just check them)
                    for arg in &new_expr.arguments {
                        self.check_expr(arg);
                    }

                    // If the class has type parameters and we have type arguments,
                    // create an instantiated class type with type vars substituted
                    if !resolved_type_args.is_empty() {
                        if let Some(crate::parser::types::Type::Class(class)) = self.type_ctx.get(symbol.ty).cloned() {
                            if class.type_params.len() == resolved_type_args.len() {
                                return self.instantiate_class_type(&class, &resolved_type_args);
                            }
                        }
                    }

                    return symbol.ty;
                } else {
                    // Symbol exists but is not a class — cannot use 'new' on it
                    self.errors.push(CheckError::NewNonClass {
                        name: name.clone(),
                        span: new_expr.span,
                    });
                    for arg in &new_expr.arguments {
                        self.check_expr(arg);
                    }
                    return self.type_ctx.unknown_type();
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
        if let Some(class_ty) = self.current_class_type {
            return class_ty;
        }
        // Outside of a class, 'this' is unknown
        self.type_ctx.unknown_type()
    }

    /// Check await expression
    fn check_await(&mut self, await_expr: &crate::parser::ast::AwaitExpression) -> TypeId {
        // Check the argument expression
        let arg_ty = self.check_expr(&await_expr.argument);

        // If the argument is a Task<T>, return T
        if let Some(crate::parser::types::Type::Task(task_ty)) = self.type_ctx.get(arg_ty) {
            return task_ty.result;
        }

        // If the argument is Task<T>[], return T[] (parallel await)
        if let Some(crate::parser::types::Type::Array(arr_ty)) = self.type_ctx.get(arg_ty) {
            if let Some(crate::parser::types::Type::Task(task_ty)) = self.type_ctx.get(arr_ty.element) {
                return self.type_ctx.array_type(task_ty.result);
            }
        }

        // Otherwise return the argument type (for compatibility)
        arg_ty
    }

    /// Try to check a compiler intrinsic call (__OPCODE_* or __NATIVE_CALL)
    /// Returns Some(type) if this is an intrinsic, None otherwise
    fn try_check_intrinsic(&mut self, call: &CallExpression) -> Option<TypeId> {
        // Check if callee is an identifier
        let ident = match &*call.callee {
            Expression::Identifier(id) => id,
            _ => return None,
        };

        let name = self.resolve(ident.name);

        // Check for __OPCODE_* intrinsics
        if let Some(opcode_name) = name.strip_prefix("__OPCODE_") {
            return Some(self.get_opcode_intrinsic_type(opcode_name));
        }

        // Check for __NATIVE_CALL intrinsic
        // Supports __NATIVE_CALL<T>(id, args...) to specify return type T
        if name == "__NATIVE_CALL" {
            if let Some(ref type_args) = call.type_args {
                if type_args.len() == 1 {
                    return Some(self.resolve_type_annotation(&type_args[0]));
                }
            }
            return Some(self.type_ctx.unknown_type());
        }

        None
    }

    /// Get the return type for an __OPCODE_* intrinsic
    fn get_opcode_intrinsic_type(&mut self, opcode_name: &str) -> TypeId {
        match opcode_name {
            // Mutex operations
            "MUTEX_NEW" => self.type_ctx.unknown_type(), // Returns Mutex type
            "MUTEX_LOCK" | "MUTEX_UNLOCK" => self.type_ctx.void_type(),

            // Channel operations
            "CHANNEL_NEW" => self.type_ctx.unknown_type(), // Returns Channel type

            // Task operations
            "TASK_CANCEL" => self.type_ctx.void_type(),
            "AWAIT" => self.type_ctx.unknown_type(), // Returns the awaited value type
            "AWAIT_ALL" => self.type_ctx.unknown_type(), // Returns array of results
            "YIELD" => self.type_ctx.void_type(),
            "SLEEP" => self.type_ctx.void_type(),

            // RefCell operations
            "REFCELL_NEW" => self.type_ctx.unknown_type(), // Returns RefCell
            "REFCELL_LOAD" => self.type_ctx.unknown_type(), // Returns the contained value
            "REFCELL_STORE" => self.type_ctx.void_type(),

            // Global operations
            "LOAD_GLOBAL" => self.type_ctx.unknown_type(),
            "STORE_GLOBAL" => self.type_ctx.void_type(),

            // Array/Object operations
            "ARRAY_LEN" => self.type_ctx.number_type(),
            "STRING_LEN" => self.type_ctx.number_type(),
            "TYPEOF" => self.type_ctx.string_type(),
            "TO_STRING" => self.type_ctx.string_type(),

            // Unknown opcode - return unknown
            _ => self.type_ctx.unknown_type(),
        }
    }

    /// Check async call expression (async funcCall() syntax)
    fn check_async_call(&mut self, async_call: &crate::parser::ast::AsyncCallExpression) -> TypeId {
        // Check all arguments
        for arg in &async_call.arguments {
            self.check_expr(arg);
        }

        // Get the callee's return type
        let callee_ty = self.check_expr(&async_call.callee);

        // If the callee is a function, get its return type and wrap in Task
        if let Some(crate::parser::types::Type::Function(func_ty)) = self.type_ctx.get(callee_ty) {
            let return_ty = func_ty.return_type;
            return self.type_ctx.task_type(return_ty);
        }

        // Otherwise just return Task<unknown>
        let unknown = self.type_ctx.unknown_type();
        self.type_ctx.task_type(unknown)
    }

    /// Check instanceof expression: expr instanceof ClassName
    fn check_instanceof(&mut self, instanceof: &crate::parser::ast::InstanceOfExpression) -> TypeId {
        // Check the object expression
        self.check_expr(&instanceof.object);

        // The target type should be a class type
        // For now, just check that it's a valid type reference
        // TODO: Validate that the target is actually a class, not a primitive or interface

        // instanceof always returns boolean
        self.type_ctx.boolean_type()
    }

    /// Check type cast expression: expr as TypeName
    fn check_type_cast(&mut self, cast: &crate::parser::ast::TypeCastExpression) -> TypeId {
        // Check the object expression
        let object_ty = self.check_expr(&cast.object);

        // Resolve the target type from the type annotation
        let target_ty = self.resolve_type_annotation(&cast.target_type);

        // TODO: Validate that the cast is safe (object type is related to target type)
        // For now, we allow all casts (like TypeScript's `as` keyword)

        // Return the target type
        target_ty
    }

    /// Check member access
    fn check_member(&mut self, member: &MemberExpression) -> TypeId {
        let object_ty = self.check_expr(&member.object);

        // Check for forbidden access to $type/$value on bare unions
        let property_name = self.resolve(member.property.name);
        if property_name == "$type" || property_name == "$value" {
            if let Some(crate::parser::types::Type::Union(union)) = self.type_ctx.get(object_ty) {
                if union.is_bare {
                    self.errors.push(CheckError::ForbiddenFieldAccess {
                        field: property_name,
                        span: member.span,
                    });
                    return self.type_ctx.unknown_type();
                }
            }
        }

        // Check if the object is a class name (static member access)
        // This happens when the object is an identifier that resolves to a class symbol
        if let Expression::Identifier(ident) = &*member.object {
            let class_name = self.resolve(ident.name);
            if let Some(symbol) = self.symbols.resolve_from_scope(&class_name, self.current_scope) {
                if symbol.kind == SymbolKind::Class {
                    // This is static member access (e.g., Date.now())
                    if let Some(crate::parser::types::Type::Class(class)) = self.type_ctx.get(symbol.ty) {
                        // Check static properties
                        for prop in &class.static_properties {
                            if prop.name == property_name {
                                return prop.ty;
                            }
                        }
                        // Check static methods
                        for method in &class.static_methods {
                            if method.name == property_name {
                                return method.ty;
                            }
                        }
                        // Static member not found
                        self.errors.push(CheckError::UndefinedMember {
                            member: property_name.clone(),
                            span: member.span,
                        });
                        return self.type_ctx.unknown_type();
                    }
                }
            }
        }

        // Get the type for property lookup
        let obj_type = self.type_ctx.get(object_ty).cloned();

        // Check for built-in array methods
        if let Some(crate::parser::types::Type::Array(arr)) = &obj_type {
            let elem_ty = arr.element;
            if let Some(method_type) = self.get_array_method_type(&property_name, elem_ty) {
                return method_type;
            }
        }

        // Check for built-in string methods
        if let Some(crate::parser::types::Type::Primitive(crate::parser::types::PrimitiveType::String)) = &obj_type {
            if let Some(method_type) = self.get_string_method_type(&property_name) {
                return method_type;
            }
        }

        // Check for built-in number methods
        if let Some(crate::parser::types::Type::Primitive(crate::parser::types::PrimitiveType::Number)) = &obj_type {
            if let Some(method_type) = self.get_number_method_type(&property_name) {
                return method_type;
            }
        }

        // Note: Mutex methods are now resolved via normal class method lookup from Mutex.raya

        // Check for built-in Task methods
        if let Some(crate::parser::types::Type::Task(_)) = &obj_type {
            if let Some(method_type) = self.get_task_method_type(&property_name) {
                return method_type;
            }
        }

        // Check for built-in RegExp methods
        if let Some(crate::parser::types::Type::RegExp) = &obj_type {
            if let Some(method_type) = self.get_regexp_method_type(&property_name) {
                return method_type;
            }
        }

        // Check for built-in Map methods
        if let Some(crate::parser::types::Type::Map(map_ty)) = &obj_type {
            if let Some(method_type) = self.get_map_method_type(&property_name, map_ty.key, map_ty.value) {
                return method_type;
            }
        }

        // Check for built-in Set methods
        if let Some(crate::parser::types::Type::Set(set_ty)) = &obj_type {
            if let Some(method_type) = self.get_set_method_type(&property_name, set_ty.element) {
                return method_type;
            }
        }

        // Note: Buffer and Date methods are now resolved via normal class method lookup
        // from their respective .raya file definitions

        // Check for built-in Channel methods
        if let Some(crate::parser::types::Type::Channel(chan_ty)) = &obj_type {
            if let Some(method_type) = self.get_channel_method_type(&property_name, chan_ty.message) {
                return method_type;
            }
        }

        // Check for class properties and methods (including inherited ones)
        if let Some(crate::parser::types::Type::Class(class)) = &obj_type {
            // If this is a placeholder class type (empty methods), look up the symbol to get the full type
            let actual_class = if class.methods.is_empty() && class.properties.is_empty() {
                // Look up class by name in symbol table
                if let Some(symbol) = self.symbols.resolve_from_scope(&class.name, self.current_scope) {
                    if symbol.kind == SymbolKind::Class {
                        self.type_ctx.get(symbol.ty).and_then(|t| {
                            if let crate::parser::types::Type::Class(c) = t {
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

            // Look up the member in the class hierarchy (including parent classes)
            if let Some((ty, vis)) = self.lookup_class_member(class_to_use, &property_name) {
                // Check visibility: private members can only be accessed from within the same class
                if vis == crate::parser::ast::Visibility::Private {
                    // Check if we're inside the same class
                    let accessing_own_class = self.current_class_type.map_or(false, |ct| {
                        if let Some(crate::parser::types::Type::Class(cur_class)) = self.type_ctx.get(ct) {
                            cur_class.name == class_to_use.name
                        } else {
                            false
                        }
                    });
                    if !accessing_own_class {
                        self.errors.push(CheckError::PropertyNotFound {
                            property: format!("private member '{}'", property_name),
                            ty: class_to_use.name.clone(),
                            span: member.span,
                        });
                        return self.type_ctx.unknown_type();
                    }
                }
                return ty;
            }

            // If we have a class type and the member was not found, emit an error
            // (unless the class has no properties/methods, which means it's a placeholder)
            if !class_to_use.properties.is_empty() || !class_to_use.methods.is_empty() {
                self.errors.push(CheckError::PropertyNotFound {
                    property: property_name.clone(),
                    ty: format!("class {}", class_to_use.name),
                    span: member.span,
                });
                return self.type_ctx.unknown_type();
            }
        }

        // Check for object type properties (from type aliases like `type Point = { x: number }`)
        if let Some(crate::parser::types::Type::Object(obj)) = &obj_type {
            for prop in &obj.properties {
                if prop.name == property_name {
                    return prop.ty;
                }
            }
        }

        // Handle Union types: look up member on any union variant that has it
        if let Some(crate::parser::types::Type::Union(union)) = &obj_type {
            let null_ty = self.type_ctx.null_type();

            // Try Class members first (nullable class unions)
            let class_members: Vec<_> = union.members.iter()
                .filter(|&&m| m != null_ty)
                .filter_map(|&m| {
                    self.type_ctx.get(m).and_then(|t| {
                        if let crate::parser::types::Type::Class(c) = t { Some(c.clone()) } else { None }
                    })
                })
                .collect();
            if class_members.len() == 1 {
                if let Some((ty, _vis)) = self.lookup_class_member(&class_members[0], &property_name) {
                    return ty;
                }
            }

            // Try Object members (for discriminated unions: type X = | { a: T } | { b: U })
            let mut found_types = Vec::new();
            for &member_id in &union.members {
                if let Some(crate::parser::types::Type::Object(obj)) = self.type_ctx.get(member_id) {
                    for prop in &obj.properties {
                        if prop.name == property_name && !found_types.contains(&prop.ty) {
                            found_types.push(prop.ty);
                        }
                    }
                }
            }
            if found_types.len() == 1 {
                return found_types[0];
            } else if found_types.len() > 1 {
                return self.type_ctx.union_type(found_types);
            }
        }

        // Handle TypeVar with constraint: delegate member access to the constraint type
        if let Some(crate::parser::types::Type::TypeVar(tv)) = &obj_type {
            if let Some(constraint_id) = tv.constraint {
                if let Some(constraint_type) = self.type_ctx.get(constraint_id).cloned() {
                    // Look up member on the constraint type (Object or Class)
                    match &constraint_type {
                        crate::parser::types::Type::Object(obj) => {
                            for prop in &obj.properties {
                                if prop.name == property_name {
                                    return prop.ty;
                                }
                            }
                        }
                        crate::parser::types::Type::Class(class) => {
                            if let Some((ty, _vis)) = self.lookup_class_member(class, &property_name) {
                                return ty;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // For now, return unknown for other member access
        self.type_ctx.unknown_type()
    }

    /// Look up a property or method in a class hierarchy, including parent classes
    /// Create an instantiated class type by substituting type parameters with concrete types.
    /// E.g., ReadableStream<T> with [number] → ReadableStream<number> with T→number in all methods.
    fn instantiate_class_type(&mut self, class: &crate::parser::types::ty::ClassType, type_args: &[TypeId]) -> TypeId {
        use crate::parser::types::GenericContext;
        use crate::parser::types::ty::{PropertySignature, MethodSignature};

        // Build substitution map: type_param_name → concrete type
        let mut gen_ctx = GenericContext::new(self.type_ctx);
        for (param_name, &arg_ty) in class.type_params.iter().zip(type_args.iter()) {
            gen_ctx.add_substitution(param_name.clone(), arg_ty);
        }

        // Substitute in properties
        let properties: Vec<PropertySignature> = class.properties.iter().map(|prop| {
            let ty = gen_ctx.apply_substitution(prop.ty).unwrap_or(prop.ty);
            PropertySignature { name: prop.name.clone(), ty, optional: prop.optional, readonly: prop.readonly, visibility: prop.visibility }
        }).collect();

        // Substitute in methods
        let methods: Vec<MethodSignature> = class.methods.iter().map(|method| {
            let ty = gen_ctx.apply_substitution(method.ty).unwrap_or(method.ty);
            MethodSignature { name: method.name.clone(), ty, type_params: method.type_params.clone(), visibility: method.visibility }
        }).collect();

        // Create the instantiated class type (clear type_params since they're resolved)
        let instantiated = crate::parser::types::ty::ClassType {
            name: class.name.clone(),
            type_params: vec![],
            properties,
            methods,
            static_properties: class.static_properties.clone(),
            static_methods: class.static_methods.clone(),
            extends: class.extends,
            implements: class.implements.clone(),
            is_abstract: class.is_abstract,
        };

        self.type_ctx.intern(crate::parser::types::Type::Class(instantiated))
    }

    fn lookup_class_member(&self, class: &crate::parser::types::ty::ClassType, property_name: &str) -> Option<(TypeId, crate::parser::ast::Visibility)> {
        // Check own properties first
        for prop in &class.properties {
            if prop.name == property_name {
                return Some((prop.ty, prop.visibility));
            }
        }
        // Check own methods
        for method in &class.methods {
            if method.name == property_name {
                return Some((method.ty, method.visibility));
            }
        }

        // If not found and class has a parent, check parent class
        if let Some(parent_ty) = class.extends {
            if let Some(crate::parser::types::Type::Class(parent_class)) = self.type_ctx.get(parent_ty) {
                // Recursively check parent class
                return self.lookup_class_member(parent_class, property_name);
            }
        }

        // Not found in class hierarchy
        None
    }

    /// Get the type of a built-in array method
    fn get_array_method_type(&mut self, method_name: &str, elem_ty: TypeId) -> Option<TypeId> {
        let number_ty = self.type_ctx.number_type();
        let boolean_ty = self.type_ctx.boolean_type();
        let void_ty = self.type_ctx.void_type();
        let string_ty = self.type_ctx.string_type();
        let array_ty = self.type_ctx.array_type(elem_ty);

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
            // slice(start: number, end: number) -> Array<T>
            "slice" => Some(self.type_ctx.function_type(vec![number_ty, number_ty], array_ty, false)),
            // concat(other: Array<T>) -> Array<T>
            "concat" => Some(self.type_ctx.function_type(vec![array_ty], array_ty, false)),
            // join(separator: string) -> string
            "join" => Some(self.type_ctx.function_type(vec![string_ty], string_ty, false)),
            // reverse() -> Array<T>
            "reverse" => Some(self.type_ctx.function_type(vec![], array_ty, false)),
            // forEach(fn: (elem: T) => void) -> void
            "forEach" => {
                let callback_ty = self.type_ctx.function_type(vec![elem_ty], void_ty, false);
                Some(self.type_ctx.function_type(vec![callback_ty], void_ty, false))
            }
            // filter(predicate: (elem: T) => boolean) -> Array<T>
            "filter" => {
                let predicate_ty = self.type_ctx.function_type(vec![elem_ty], boolean_ty, false);
                Some(self.type_ctx.function_type(vec![predicate_ty], array_ty, false))
            }
            // find(predicate: (elem: T) => boolean) -> T | null
            "find" => {
                let predicate_ty = self.type_ctx.function_type(vec![elem_ty], boolean_ty, false);
                let null_ty = self.type_ctx.null_type();
                let nullable_elem = self.type_ctx.union_type(vec![elem_ty, null_ty]);
                Some(self.type_ctx.function_type(vec![predicate_ty], nullable_elem, false))
            }
            // findIndex(predicate: (elem: T) => boolean) -> number
            "findIndex" => {
                let predicate_ty = self.type_ctx.function_type(vec![elem_ty], boolean_ty, false);
                Some(self.type_ctx.function_type(vec![predicate_ty], number_ty, false))
            }
            // every(predicate: (elem: T) => boolean) -> boolean
            "every" => {
                let predicate_ty = self.type_ctx.function_type(vec![elem_ty], boolean_ty, false);
                Some(self.type_ctx.function_type(vec![predicate_ty], boolean_ty, false))
            }
            // some(predicate: (elem: T) => boolean) -> boolean
            "some" => {
                let predicate_ty = self.type_ctx.function_type(vec![elem_ty], boolean_ty, false);
                Some(self.type_ctx.function_type(vec![predicate_ty], boolean_ty, false))
            }
            // length -> number (property, not method)
            "length" => Some(number_ty),
            // lastIndexOf(value: T) -> number
            "lastIndexOf" => Some(self.type_ctx.function_type(vec![elem_ty], number_ty, false)),
            // sort(compareFn?: (a: T, b: T) => number) -> Array<T>
            "sort" => {
                let compare_fn_ty = self.type_ctx.function_type(vec![elem_ty, elem_ty], number_ty, false);
                Some(self.type_ctx.function_type_with_min_params(vec![compare_fn_ty], array_ty, false, 0))
            }
            // map(fn: (elem: T) => T) -> Array<T> (simplified - without generic U)
            "map" => {
                let callback_ty = self.type_ctx.function_type(vec![elem_ty], elem_ty, false);
                Some(self.type_ctx.function_type(vec![callback_ty], array_ty, false))
            }
            // reduce(fn: (acc: T, elem: T) => T, initial: T) -> T (simplified)
            "reduce" => {
                let callback_ty = self.type_ctx.function_type(vec![elem_ty, elem_ty], elem_ty, false);
                Some(self.type_ctx.function_type(vec![callback_ty, elem_ty], elem_ty, false))
            }
            // fill(value: T, start?: number, end?: number) -> Array<T>
            "fill" => Some(self.type_ctx.function_type_with_min_params(vec![elem_ty, number_ty, number_ty], array_ty, false, 1)),
            // flat() -> Array<T> (simplified - single level flatten)
            "flat" => Some(self.type_ctx.function_type(vec![], array_ty, false)),
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
            // substring(start: number, end?: number) -> string
            "substring" => Some(self.type_ctx.function_type_with_min_params(vec![number_ty, number_ty], string_ty, false, 1)),
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
            // startsWith(prefix: string) -> boolean
            "startsWith" => Some(self.type_ctx.function_type(vec![string_ty], boolean_ty, false)),
            // endsWith(suffix: string) -> boolean
            "endsWith" => Some(self.type_ctx.function_type(vec![string_ty], boolean_ty, false)),
            // split(separator: string | RegExp, limit?: number) -> Array<string>
            "split" => {
                let regexp_ty = self.type_ctx.regexp_type();
                let search_ty = self.type_ctx.union_type(vec![string_ty, regexp_ty]);
                let arr_ty = self.type_ctx.array_type(string_ty);
                Some(self.type_ctx.function_type_with_min_params(vec![search_ty, number_ty], arr_ty, false, 1))
            }
            // replace(search: string | RegExp, replacement: string) -> string
            "replace" => {
                let regexp_ty = self.type_ctx.regexp_type();
                let search_ty = self.type_ctx.union_type(vec![string_ty, regexp_ty]);
                Some(self.type_ctx.function_type(vec![search_ty, string_ty], string_ty, false))
            }
            // repeat(count: number) -> string
            "repeat" => Some(self.type_ctx.function_type(vec![number_ty], string_ty, false)),
            // charCodeAt(index: number) -> number
            "charCodeAt" => Some(self.type_ctx.function_type(vec![number_ty], number_ty, false)),
            // lastIndexOf(searchStr: string) -> number
            "lastIndexOf" => Some(self.type_ctx.function_type(vec![string_ty], number_ty, false)),
            // trimStart() -> string
            "trimStart" => Some(self.type_ctx.function_type(vec![], string_ty, false)),
            // trimEnd() -> string
            "trimEnd" => Some(self.type_ctx.function_type(vec![], string_ty, false)),
            // padStart(length: number, pad?: string) -> string
            "padStart" => Some(self.type_ctx.function_type_with_min_params(vec![number_ty, string_ty], string_ty, false, 1)),
            // padEnd(length: number, pad?: string) -> string
            "padEnd" => Some(self.type_ctx.function_type_with_min_params(vec![number_ty, string_ty], string_ty, false, 1)),
            // match(pattern: RegExp) -> string[] | null
            // Returns array of matches or null if no match
            "match" => {
                let regexp_ty = self.type_ctx.regexp_type();
                let arr_ty = self.type_ctx.array_type(string_ty);
                let null_ty = self.type_ctx.null_type();
                let result_ty = self.type_ctx.union_type(vec![arr_ty, null_ty]);
                Some(self.type_ctx.function_type(vec![regexp_ty], result_ty, false))
            }
            // matchAll(pattern: RegExp) -> Array<string[]>
            "matchAll" => {
                let regexp_ty = self.type_ctx.regexp_type();
                let inner_arr_ty = self.type_ctx.array_type(string_ty);
                let arr_ty = self.type_ctx.array_type(inner_arr_ty);
                Some(self.type_ctx.function_type(vec![regexp_ty], arr_ty, false))
            }
            // search(pattern: RegExp) -> number
            // Returns index of first match, or -1 if no match
            "search" => {
                let regexp_ty = self.type_ctx.regexp_type();
                Some(self.type_ctx.function_type(vec![regexp_ty], number_ty, false))
            }
            // replaceWith(pattern: RegExp, replacer: (match: Array<string | number>) => string) -> string
            // Callback receives [matchedText, index, ...groups] for each match
            "replaceWith" => {
                let regexp_ty = self.type_ctx.regexp_type();
                // Callback type: (Array<string | number>) => string
                let union_elem_ty = self.type_ctx.union_type(vec![string_ty, number_ty]);
                let match_arr_ty = self.type_ctx.array_type(union_elem_ty);
                let callback_ty = self.type_ctx.function_type(vec![match_arr_ty], string_ty, false);
                Some(self.type_ctx.function_type(vec![regexp_ty, callback_ty], string_ty, false))
            }
            _ => None,
        }
    }

    /// Get the type of a built-in number method
    fn get_number_method_type(&mut self, method_name: &str) -> Option<TypeId> {
        let number_ty = self.type_ctx.number_type();
        let string_ty = self.type_ctx.string_type();

        match method_name {
            // toFixed(digits: number) -> string
            "toFixed" => Some(self.type_ctx.function_type(vec![number_ty], string_ty, false)),
            // toPrecision(precision: number) -> string
            "toPrecision" => Some(self.type_ctx.function_type(vec![number_ty], string_ty, false)),
            // toString(radix?: number) -> string
            "toString" => Some(self.type_ctx.function_type_with_min_params(vec![number_ty], string_ty, false, 0)),
            _ => None,
        }
    }

    // Note: Mutex methods are now resolved from Mutex.raya class definition
    // (get_mutex_method_type removed - no longer needed)

    /// Get the type of a built-in Task method
    fn get_task_method_type(&mut self, method_name: &str) -> Option<TypeId> {
        let void_ty = self.type_ctx.void_type();

        match method_name {
            // cancel() -> void
            "cancel" => Some(self.type_ctx.function_type(vec![], void_ty, false)),
            _ => None,
        }
    }

    /// Get the type of a built-in RegExp method
    fn get_regexp_method_type(&mut self, method_name: &str) -> Option<TypeId> {
        let string_ty = self.type_ctx.string_type();
        let boolean_ty = self.type_ctx.boolean_type();
        let number_ty = self.type_ctx.number_type();

        match method_name {
            // test(str: string) -> boolean
            "test" => Some(self.type_ctx.function_type(vec![string_ty], boolean_ty, false)),
            // exec(str: string) -> string | null (simplified - returns matched string or null)
            "exec" => {
                let null_ty = self.type_ctx.null_type();
                let result_ty = self.type_ctx.union_type(vec![string_ty, null_ty]);
                Some(self.type_ctx.function_type(vec![string_ty], result_ty, false))
            }
            // execAll(str: string) -> string[]
            "execAll" => {
                let array_ty = self.type_ctx.array_type(string_ty);
                Some(self.type_ctx.function_type(vec![string_ty], array_ty, false))
            }
            // replace(str: string, replacement: string) -> string
            "replace" => Some(self.type_ctx.function_type(vec![string_ty, string_ty], string_ty, false)),
            // split(str: string, limit?: number) -> string[]
            "split" => {
                let array_ty = self.type_ctx.array_type(string_ty);
                Some(self.type_ctx.function_type_with_min_params(vec![string_ty, number_ty], array_ty, false, 1))
            }
            // source property -> string
            "source" => Some(string_ty),
            // flags property -> string
            "flags" => Some(string_ty),
            // global property -> boolean
            "global" => Some(boolean_ty),
            // ignoreCase property -> boolean
            "ignoreCase" => Some(boolean_ty),
            // multiline property -> boolean
            "multiline" => Some(boolean_ty),
            // lastIndex property -> number (legacy, Raya RegExp is stateless)
            "lastIndex" => Some(number_ty),
            // replaceWith(str: string, replacer: (match: string) => string) -> string
            "replaceWith" => {
                let replacer_ty = self.type_ctx.function_type(vec![string_ty], string_ty, false);
                Some(self.type_ctx.function_type(vec![string_ty, replacer_ty], string_ty, false))
            }
            // dotAll property -> boolean
            "dotAll" => Some(boolean_ty),
            // unicode property -> boolean
            "unicode" => Some(boolean_ty),
            _ => None,
        }
    }

    /// Get the type of a built-in Map method
    fn get_map_method_type(&mut self, method_name: &str, key_ty: TypeId, value_ty: TypeId) -> Option<TypeId> {
        let number_ty = self.type_ctx.number_type();
        let boolean_ty = self.type_ctx.boolean_type();
        let void_ty = self.type_ctx.void_type();
        let null_ty = self.type_ctx.null_type();

        match method_name {
            // size() -> number
            "size" => Some(self.type_ctx.function_type(vec![], number_ty, false)),
            // get(key: K) -> V | null
            "get" => {
                let result_ty = self.type_ctx.union_type(vec![value_ty, null_ty]);
                Some(self.type_ctx.function_type(vec![key_ty], result_ty, false))
            }
            // set(key: K, value: V) -> void
            "set" => Some(self.type_ctx.function_type(vec![key_ty, value_ty], void_ty, false)),
            // has(key: K) -> boolean
            "has" => Some(self.type_ctx.function_type(vec![key_ty], boolean_ty, false)),
            // delete(key: K) -> boolean
            "delete" => Some(self.type_ctx.function_type(vec![key_ty], boolean_ty, false)),
            // clear() -> void
            "clear" => Some(self.type_ctx.function_type(vec![], void_ty, false)),
            // keys() -> Array<K>
            "keys" => {
                let array_ty = self.type_ctx.array_type(key_ty);
                Some(self.type_ctx.function_type(vec![], array_ty, false))
            }
            // values() -> Array<V>
            "values" => {
                let array_ty = self.type_ctx.array_type(value_ty);
                Some(self.type_ctx.function_type(vec![], array_ty, false))
            }
            // entries() -> Array<[K, V]>
            "entries" => {
                let tuple_ty = self.type_ctx.tuple_type(vec![key_ty, value_ty]);
                let array_ty = self.type_ctx.array_type(tuple_ty);
                Some(self.type_ctx.function_type(vec![], array_ty, false))
            }
            // forEach(fn: (value: V, key: K) => void) -> void
            "forEach" => {
                let callback_ty = self.type_ctx.function_type(vec![value_ty, key_ty], void_ty, false);
                Some(self.type_ctx.function_type(vec![callback_ty], void_ty, false))
            }
            _ => None,
        }
    }

    /// Get the type of a built-in Set method
    fn get_set_method_type(&mut self, method_name: &str, element_ty: TypeId) -> Option<TypeId> {
        let number_ty = self.type_ctx.number_type();
        let boolean_ty = self.type_ctx.boolean_type();
        let void_ty = self.type_ctx.void_type();

        match method_name {
            // size() -> number
            "size" => Some(self.type_ctx.function_type(vec![], number_ty, false)),
            // add(value: T) -> void
            "add" => Some(self.type_ctx.function_type(vec![element_ty], void_ty, false)),
            // has(value: T) -> boolean
            "has" => Some(self.type_ctx.function_type(vec![element_ty], boolean_ty, false)),
            // delete(value: T) -> boolean
            "delete" => Some(self.type_ctx.function_type(vec![element_ty], boolean_ty, false)),
            // clear() -> void
            "clear" => Some(self.type_ctx.function_type(vec![], void_ty, false)),
            // values() -> Array<T>
            "values" => {
                let array_ty = self.type_ctx.array_type(element_ty);
                Some(self.type_ctx.function_type(vec![], array_ty, false))
            }
            // forEach(fn: (value: T) => void) -> void
            "forEach" => {
                let callback_ty = self.type_ctx.function_type(vec![element_ty], void_ty, false);
                Some(self.type_ctx.function_type(vec![callback_ty], void_ty, false))
            }
            // union(other: Set<T>) -> Set<T>
            "union" => {
                let set_ty = self.type_ctx.set_type_with(element_ty);
                Some(self.type_ctx.function_type(vec![set_ty], set_ty, false))
            }
            // intersection(other: Set<T>) -> Set<T>
            "intersection" => {
                let set_ty = self.type_ctx.set_type_with(element_ty);
                Some(self.type_ctx.function_type(vec![set_ty], set_ty, false))
            }
            // difference(other: Set<T>) -> Set<T>
            "difference" => {
                let set_ty = self.type_ctx.set_type_with(element_ty);
                Some(self.type_ctx.function_type(vec![set_ty], set_ty, false))
            }
            _ => None,
        }
    }

    // Note: get_buffer_method_type and get_date_method_type removed
    // Buffer and Date methods are now resolved from their .raya class definitions

    /// Get the type of a built-in Channel method
    fn get_channel_method_type(&mut self, method_name: &str, message_ty: TypeId) -> Option<TypeId> {
        let number_ty = self.type_ctx.number_type();
        let boolean_ty = self.type_ctx.boolean_type();
        let void_ty = self.type_ctx.void_type();
        let null_ty = self.type_ctx.null_type();

        match method_name {
            // send(value: T) -> void
            "send" => Some(self.type_ctx.function_type(vec![message_ty], void_ty, false)),
            // receive() -> T
            "receive" => Some(self.type_ctx.function_type(vec![], message_ty, false)),
            // trySend(value: T) -> boolean
            "trySend" => Some(self.type_ctx.function_type(vec![message_ty], boolean_ty, false)),
            // tryReceive() -> T | null
            "tryReceive" => {
                let result_ty = self.type_ctx.union_type(vec![message_ty, null_ty]);
                Some(self.type_ctx.function_type(vec![], result_ty, false))
            }
            // close() -> void
            "close" => Some(self.type_ctx.function_type(vec![], void_ty, false)),
            // isClosed() -> boolean
            "isClosed" => Some(self.type_ctx.function_type(vec![], boolean_ty, false)),
            // length() -> number
            "length" => Some(self.type_ctx.function_type(vec![], number_ty, false)),
            // capacity() -> number
            "capacity" => Some(self.type_ctx.function_type(vec![], number_ty, false)),
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

        // Collect all distinct element types to compute a unified element type
        let mut elem_types = Vec::new();
        for elem_opt in &arr.elements {
            if let Some(elem) = elem_opt {
                let elem_ty = match elem {
                    ArrayElement::Expression(expr) => self.check_expr(expr),
                    ArrayElement::Spread(expr) => {
                        let spread_ty = self.check_expr(expr);
                        if let Some(crate::parser::types::Type::Array(arr_ty)) = self.type_ctx.get(spread_ty).cloned() {
                            arr_ty.element
                        } else {
                            spread_ty
                        }
                    }
                };
                if !elem_types.contains(&elem_ty) {
                    elem_types.push(elem_ty);
                }
            }
        }

        // If all elements are the same type, use that; otherwise create a union
        let unified_ty = if elem_types.len() == 1 {
            elem_types[0]
        } else if elem_types.is_empty() {
            self.type_ctx.unknown_type()
        } else {
            self.type_ctx.union_type(elem_types)
        };

        self.type_ctx.array_type(unified_ty)
    }

    /// Check object literal
    fn check_object(&mut self, obj: &ObjectExpression) -> TypeId {
        use crate::parser::types::ty::{PropertySignature, ClassType};

        let mut properties = Vec::new();
        for prop in &obj.properties {
            match prop {
                ObjectProperty::Property(p) => {
                    let name = match &p.key {
                        PropertyKey::Identifier(ident) => self.resolve(ident.name),
                        PropertyKey::StringLiteral(lit) => self.resolve(lit.value),
                        PropertyKey::IntLiteral(lit) => lit.value.to_string(),
                        PropertyKey::Computed(_) => continue, // Skip computed keys
                    };
                    let value_ty = self.check_expr(&p.value);
                    properties.push(PropertySignature {
                        name,
                        ty: value_ty,
                        optional: false,
                        readonly: false,
                        visibility: Default::default(),
                    });
                }
                ObjectProperty::Spread(_) => {
                    // Spread properties are complex — skip for now
                }
            }
        }

        let class_type = ClassType {
            name: "<anonymous>".to_string(),
            type_params: vec![],
            properties,
            methods: vec![],
            static_properties: vec![],
            static_methods: vec![],
            extends: None,
            implements: vec![],
            is_abstract: false,
        };
        self.type_ctx.intern(crate::parser::types::Type::Class(class_type))
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
        // Check for readonly property assignment
        if let Expression::Member(member) = &*assign.left {
            let is_this = matches!(&*member.object, Expression::This(_));
            // Allow this.field = value inside constructors
            if !(is_this && self.in_constructor) {
                let object_ty = self.check_expr(&member.object);
                let property_name = self.resolve(member.property.name);
                if self.is_readonly_property(object_ty, &property_name) {
                    self.errors.push(CheckError::ReadonlyAssignment {
                        property: property_name,
                        span: member.span,
                    });
                }
            }
        }

        // Check const reassignment for simple identifiers
        if let Expression::Identifier(ident) = &*assign.left {
            let name = self.resolve(ident.name);
            if let Some(symbol) = self.symbols.resolve_from_scope(&name, self.current_scope) {
                if symbol.flags.is_const {
                    self.errors.push(CheckError::ConstReassignment {
                        name: name.clone(),
                        span: assign.span,
                    });
                }
            }
        }

        // For simple identifier assignments, use the declared type (not narrowed)
        // so that reassignment back to the original wider type is allowed.
        // e.g., inside `while (val != null)`, `val = ch.tryReceive()` should work
        // even though `val` was narrowed from `T | null` to `T`.
        let (left_ty, clear_var) = if let Expression::Identifier(ident) = &*assign.left {
            let name = self.resolve(ident.name);
            let declared_ty = self.get_var_declared_type(&name)
                .unwrap_or_else(|| self.check_expr(&assign.left));
            (declared_ty, Some(name))
        } else {
            (self.check_expr(&assign.left), None)
        };
        // Evaluate RHS before clearing narrowing so `current = current.next`
        // can use the narrowed type of `current` when evaluating `current.next`.
        let right_ty = self.check_expr(&assign.right);
        // Clear narrowing after RHS evaluation since the variable is being reassigned.
        if let Some(name) = clear_var {
            self.type_env.remove(&name);
        }

        self.check_assignable(right_ty, left_ty, *assign.right.span());

        left_ty
    }

    /// Check if source type is assignable to target type
    fn check_assignable(&mut self, source: TypeId, target: TypeId, span: crate::parser::Span) {
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

    /// Check if a property is readonly on a given type
    fn is_readonly_property(&self, ty: TypeId, property_name: &str) -> bool {
        if let Some(resolved) = self.type_ctx.get(ty) {
            match resolved {
                crate::parser::types::Type::Class(class) => {
                    for prop in &class.properties {
                        if prop.name == property_name {
                            return prop.readonly;
                        }
                    }
                }
                crate::parser::types::Type::Object(obj) => {
                    for prop in &obj.properties {
                        if prop.name == property_name {
                            return prop.readonly;
                        }
                    }
                }
                crate::parser::types::Type::Interface(iface) => {
                    for prop in &iface.properties {
                        if prop.name == property_name {
                            return prop.readonly;
                        }
                    }
                }
                _ => {}
            }
        }
        false
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
    fn resolve_type(&mut self, ty: &crate::parser::ast::Type) -> TypeId {
        use crate::parser::ast::Type as AstType;

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

                // Handle built-in types
                // Note: Mutex is now a normal class from Mutex.raya
                if name == "RegExp" {
                    return self.type_ctx.regexp_type();
                }
                if name == "Channel" {
                    return self.type_ctx.channel_type();
                }
                if name == "Map" {
                    return self.type_ctx.map_type();
                }
                if name == "Set" {
                    return self.type_ctx.set_type();
                }
                // Note: Date and Buffer are now normal classes, looked up from symbol table

                if let Some(symbol) = self.symbols.resolve_from_scope(&name, self.current_scope) {
                    // If this is a generic class with type arguments, instantiate it
                    if let Some(ref type_args) = type_ref.type_args {
                        if !type_args.is_empty() {
                            let resolved_args: Vec<TypeId> = type_args.iter()
                                .map(|arg| self.resolve_type_annotation(arg))
                                .collect();
                            if let Some(crate::parser::types::Type::Class(class)) = self.type_ctx.get(symbol.ty).cloned() {
                                if class.type_params.len() == resolved_args.len() {
                                    return self.instantiate_class_type(&class, &resolved_args);
                                }
                            }
                        }
                    }
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

            AstType::Intersection(intersection) => {
                // Merge constituent types into a single Object type
                let mut merged_properties = Vec::new();
                for ty_annot in &intersection.types {
                    let ty_id = self.resolve_type_annotation(ty_annot);
                    if let Some(ty) = self.type_ctx.get(ty_id).cloned() {
                        if let crate::parser::types::Type::Object(obj) = ty {
                            for prop in &obj.properties {
                                if !merged_properties.iter().any(|p: &crate::parser::types::ty::PropertySignature| p.name == prop.name) {
                                    merged_properties.push(prop.clone());
                                }
                            }
                        }
                    }
                }
                self.type_ctx.object_type(merged_properties)
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
    fn resolve_primitive(&mut self, prim: crate::parser::ast::PrimitiveType) -> TypeId {
        use crate::parser::ast::PrimitiveType as AstPrim;

        match prim {
            AstPrim::Number => self.type_ctx.number_type(),
            AstPrim::Int => self.type_ctx.int_type(),
            AstPrim::String => self.type_ctx.string_type(),
            AstPrim::Boolean => self.type_ctx.boolean_type(),
            AstPrim::Null => self.type_ctx.null_type(),
            AstPrim::Void => self.type_ctx.void_type(),
        }
    }

    // ========================================================================
    // Decorator Type Checking
    // ========================================================================

    /// Check decorators on a class declaration
    fn check_class_decorators(&mut self, class: &crate::parser::ast::ClassDecl) {
        for decorator in &class.decorators {
            self.check_class_decorator(decorator, class);
        }
    }

    /// Check a single class decorator
    fn check_class_decorator(
        &mut self,
        decorator: &crate::parser::ast::Decorator,
        _class: &crate::parser::ast::ClassDecl,
    ) {
        // Get the type of the decorator expression
        let decorator_ty = self.check_expr(&decorator.expression);

        // Check if it's a function type
        let func_ty_opt = self.type_ctx.get(decorator_ty).cloned();

        match func_ty_opt {
            Some(crate::parser::types::Type::Function(func)) => {
                // Class decorator should take 1 parameter (the class)
                // and return the class type or void
                if func.params.len() != 1 {
                    self.errors.push(CheckError::InvalidDecorator {
                        ty: self.type_ctx.display(decorator_ty),
                        expected: "ClassDecorator<T> = (target: Class<T>) => Class<T> | void"
                            .to_string(),
                        span: decorator.span,
                    });
                }
                // Return type should be void or a class type
                // For now, we accept any return type as the type system
                // will validate at call site
            }
            Some(_) | None => {
                // Not a function - might be a decorator factory (call expression)
                // The call expression would have already been type-checked
                // and its result type stored
                if !matches!(decorator.expression, Expression::Call(_)) {
                    self.errors.push(CheckError::InvalidDecorator {
                        ty: self.type_ctx.display(decorator_ty),
                        expected: "ClassDecorator<T> or decorator factory".to_string(),
                        span: decorator.span,
                    });
                }
            }
        }
    }

    /// Check decorators on a method declaration
    fn check_method_decorators(
        &mut self,
        method: &crate::parser::ast::MethodDecl,
        method_ty: TypeId,
    ) {
        for decorator in &method.decorators {
            self.check_method_decorator(decorator, method_ty);
        }
    }

    /// Check a single method decorator
    ///
    /// Raya supports two method decorator styles:
    /// 1. Metadata style: (classId: number, methodName: string) => void
    /// 2. Type-constrained style: (method: F) => F (where F is a specific function type)
    ///
    /// The type-constrained style allows decorators to constrain which methods they
    /// can be applied to based on the method's signature.
    fn check_method_decorator(&mut self, decorator: &crate::parser::ast::Decorator, method_ty: TypeId) {
        // Get the type of the decorator expression
        let decorator_ty = self.check_expr(&decorator.expression);

        // Check if it's a function type
        let func_ty_opt = self.type_ctx.get(decorator_ty).cloned();

        match func_ty_opt {
            Some(crate::parser::types::Type::Function(func)) => {
                let num_ty = self.type_ctx.number_type();
                let str_ty = self.type_ctx.string_type();
                let void_ty = self.type_ctx.void_type();

                // Check for metadata-style method decorator: (classId: number, methodName: string) => void
                if func.params.len() == 2 {
                    let mut assign_ctx = AssignabilityContext::new(self.type_ctx);
                    if assign_ctx.is_assignable(num_ty, func.params[0])
                        && assign_ctx.is_assignable(str_ty, func.params[1])
                        && assign_ctx.is_assignable(func.return_type, void_ty)
                    {
                        // Valid metadata-style decorator - no method type constraint
                        return;
                    }
                }

                // Check for type-constrained decorator: (method: F) => F
                if func.params.len() == 1 {
                    let param_ty = func.params[0];

                    // Check if the parameter is a function type (type-constrained decorator)
                    if let Some(crate::parser::types::Type::Function(_)) = self.type_ctx.get(param_ty) {
                        // This is a type-constrained decorator
                        // Check if the method type is assignable to the parameter type
                        let mut assign_ctx = AssignabilityContext::new(self.type_ctx);
                        if !assign_ctx.is_assignable(method_ty, param_ty) {
                            self.errors.push(CheckError::DecoratorSignatureMismatch {
                                expected_signature: self.type_ctx.display(param_ty),
                                actual_signature: self.type_ctx.display(method_ty),
                                span: decorator.span,
                            });
                            return;
                        }

                        // The return type should also match the method type
                        if !assign_ctx.is_assignable(func.return_type, method_ty)
                            && !assign_ctx.is_assignable(method_ty, func.return_type)
                        {
                            self.errors.push(CheckError::DecoratorReturnMismatch {
                                expected: self.type_ctx.display(method_ty),
                                actual: self.type_ctx.display(func.return_type),
                                span: decorator.span,
                            });
                        }
                        return;
                    }
                }

                // Other function signatures are valid (e.g., field decorators applied to methods by mistake
                // will be caught elsewhere, or custom decorator patterns)
            }
            Some(_) | None => {
                // Not a function - might be a decorator factory (call expression)
                if !matches!(decorator.expression, Expression::Call(_)) {
                    self.errors.push(CheckError::InvalidDecorator {
                        ty: self.type_ctx.display(decorator_ty),
                        expected: "MethodDecorator or decorator factory".to_string(),
                        span: decorator.span,
                    });
                }
            }
        }
    }

    /// Check decorators on a field declaration
    fn check_field_decorators(&mut self, field: &crate::parser::ast::FieldDecl) {
        for decorator in &field.decorators {
            self.check_field_decorator(decorator);
        }
    }

    /// Check a single field decorator
    ///
    /// Field decorators have the signature: (target: T, fieldName: string) => void
    fn check_field_decorator(&mut self, decorator: &crate::parser::ast::Decorator) {
        // Get the type of the decorator expression
        let decorator_ty = self.check_expr(&decorator.expression);

        // Check if it's a function type
        let func_ty_opt = self.type_ctx.get(decorator_ty).cloned();

        match func_ty_opt {
            Some(crate::parser::types::Type::Function(func)) => {
                // Field decorator should take 2 parameters (target, fieldName)
                if func.params.len() != 2 {
                    self.errors.push(CheckError::InvalidDecorator {
                        ty: self.type_ctx.display(decorator_ty),
                        expected: "FieldDecorator<T> = (target: T, fieldName: string) => void"
                            .to_string(),
                        span: decorator.span,
                    });
                    return;
                }

                // Second parameter should be string
                let string_ty = self.type_ctx.string_type();
                let mut assign_ctx = AssignabilityContext::new(self.type_ctx);
                if !assign_ctx.is_assignable(string_ty, func.params[1]) {
                    self.errors.push(CheckError::InvalidDecorator {
                        ty: self.type_ctx.display(decorator_ty),
                        expected: "FieldDecorator<T> = (target: T, fieldName: string) => void"
                            .to_string(),
                        span: decorator.span,
                    });
                }

                // Return type should be void
                let void_ty = self.type_ctx.void_type();
                if func.return_type != void_ty {
                    self.errors.push(CheckError::DecoratorReturnMismatch {
                        expected: "void".to_string(),
                        actual: self.type_ctx.display(func.return_type),
                        span: decorator.span,
                    });
                }
            }
            Some(_) | None => {
                // Not a function - might be a decorator factory
                if !matches!(decorator.expression, Expression::Call(_)) {
                    self.errors.push(CheckError::InvalidDecorator {
                        ty: self.type_ctx.display(decorator_ty),
                        expected: "FieldDecorator<T> or decorator factory".to_string(),
                        span: decorator.span,
                    });
                }
            }
        }
    }

    /// Check decorators on a parameter
    fn check_parameter_decorators(&mut self, param: &crate::parser::ast::Parameter) {
        for decorator in &param.decorators {
            self.check_parameter_decorator(decorator);
        }
    }

    /// Check a single parameter decorator
    ///
    /// Parameter decorators have the signature:
    /// (target: T, methodName: string, parameterIndex: number) => void
    fn check_parameter_decorator(&mut self, decorator: &crate::parser::ast::Decorator) {
        // Get the type of the decorator expression
        let decorator_ty = self.check_expr(&decorator.expression);

        // Check if it's a function type
        let func_ty_opt = self.type_ctx.get(decorator_ty).cloned();

        match func_ty_opt {
            Some(crate::parser::types::Type::Function(func)) => {
                // Parameter decorator should take 3 parameters
                if func.params.len() != 3 {
                    self.errors.push(CheckError::InvalidDecorator {
                        ty: self.type_ctx.display(decorator_ty),
                        expected: "ParameterDecorator<T> = (target: T, methodName: string, parameterIndex: number) => void".to_string(),
                        span: decorator.span,
                    });
                    return;
                }

                // Second parameter should be string
                let string_ty = self.type_ctx.string_type();
                let number_ty = self.type_ctx.number_type();
                let mut assign_ctx = AssignabilityContext::new(self.type_ctx);

                if !assign_ctx.is_assignable(string_ty, func.params[1]) {
                    self.errors.push(CheckError::InvalidDecorator {
                        ty: self.type_ctx.display(decorator_ty),
                        expected: "ParameterDecorator<T> - second param should be string".to_string(),
                        span: decorator.span,
                    });
                }

                // Third parameter should be number
                if !assign_ctx.is_assignable(number_ty, func.params[2]) {
                    self.errors.push(CheckError::InvalidDecorator {
                        ty: self.type_ctx.display(decorator_ty),
                        expected: "ParameterDecorator<T> - third param should be number".to_string(),
                        span: decorator.span,
                    });
                }

                // Return type should be void
                let void_ty = self.type_ctx.void_type();
                if func.return_type != void_ty {
                    self.errors.push(CheckError::DecoratorReturnMismatch {
                        expected: "void".to_string(),
                        actual: self.type_ctx.display(func.return_type),
                        span: decorator.span,
                    });
                }
            }
            Some(_) | None => {
                // Not a function - might be a decorator factory
                if !matches!(decorator.expression, Expression::Call(_)) {
                    self.errors.push(CheckError::InvalidDecorator {
                        ty: self.type_ctx.display(decorator_ty),
                        expected: "ParameterDecorator<T> or decorator factory".to_string(),
                        span: decorator.span,
                    });
                }
            }
        }
    }

    /// Build a function type for a method (used for decorator checking)
    fn build_method_type(&mut self, method: &crate::parser::ast::MethodDecl) -> TypeId {
        // Collect parameter types
        let param_types: Vec<TypeId> = method
            .params
            .iter()
            .map(|p| {
                if let Some(ref ann) = p.type_annotation {
                    self.resolve_type_annotation(ann)
                } else {
                    self.type_ctx.unknown_type()
                }
            })
            .collect();

        // Get return type
        let return_ty = if let Some(ref ann) = method.return_type {
            self.resolve_type_annotation(ann)
        } else {
            self.type_ctx.void_type()
        };

        self.type_ctx.function_type(param_types, return_ty, method.is_async)
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

    // ========================================================================
    // Decorator Type Checking Tests
    // ========================================================================

    #[test]
    fn test_class_decorator_valid() {
        // A valid class decorator is a function that takes Class<T> and returns Class<T> | void
        let result = parse_and_check(r#"
            function Injectable<T>(target: T): void {}

            @Injectable
            class Service {}
        "#);
        // Should pass - decorator function is valid
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }

    #[test]
    fn test_class_decorator_factory_valid() {
        // Decorator factory is a function that returns a decorator
        // Use arrow function since function expressions are not supported
        let result = parse_and_check(r#"
            function Controller<T>(prefix: string): (target: T) => void {
                return (target: T): void => {};
            }

            @Controller("/api")
            class ApiController {}
        "#);
        // Should pass - decorator factory is valid
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }

    #[test]
    fn test_method_decorator_valid() {
        // A valid method decorator takes a function and returns a function
        let result = parse_and_check(r#"
            function Logged<F>(method: F): F {
                return method;
            }

            class Service {
                @Logged
                doWork(): void {}
            }
        "#);
        // Should pass - decorator matches method signature
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }

    #[test]
    fn test_field_decorator_valid() {
        // A valid field decorator takes (target, fieldName) and returns void
        let result = parse_and_check(r#"
            function Column<T>(target: T, fieldName: string): void {}

            class User {
                @Column
                name: string;
            }
        "#);
        // Should pass - decorator signature is valid
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }

    #[test]
    fn test_parameter_decorator_valid() {
        // A valid parameter decorator takes (target, methodName, index) and returns void
        // Note: Parameter decorators on constructor params may not be fully supported by parser
        // So we test on method parameters instead
        let result = parse_and_check(r#"
            function Inject<T>(target: T, methodName: string, parameterIndex: number): void {}

            class Service {
                doWork(@Inject dep: number): void {}
            }
        "#);
        // Should pass - decorator signature is valid
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }

    #[test]
    fn test_decorator_not_a_function() {
        // Non-function as decorator should error
        let result = parse_and_check(r#"
            let notAFunction: number = 42;

            @notAFunction
            class Service {}
        "#);
        // Should fail - decorator is not a function
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.iter().any(|e| matches!(e, CheckError::InvalidDecorator { .. })));
    }

    #[test]
    fn test_field_decorator_wrong_param_count() {
        // Field decorator with wrong parameter count should error
        let result = parse_and_check(r#"
            function BadDecorator<T>(target: T): void {}

            class User {
                @BadDecorator
                name: string;
            }
        "#);
        // Should fail - field decorator expects 2 params
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.iter().any(|e| matches!(e, CheckError::InvalidDecorator { .. })));
    }

    #[test]
    fn test_field_decorator_wrong_return_type() {
        // Field decorator must return void
        let result = parse_and_check(r#"
            function BadReturn<T>(target: T, fieldName: string): string {
                return fieldName;
            }

            class User {
                @BadReturn
                name: string;
            }
        "#);
        // Should fail - return type is not void
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.iter().any(|e| matches!(e, CheckError::DecoratorReturnMismatch { .. })));
    }

    #[test]
    fn test_multiple_decorators_on_class() {
        // Multiple decorators on a class
        let result = parse_and_check(r#"
            function Dec1<T>(target: T): void {}
            function Dec2<T>(target: T): void {}

            @Dec1
            @Dec2
            class Service {}
        "#);
        // Should pass - both decorators are valid
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }

    #[test]
    fn test_multiple_decorators_on_method() {
        // Multiple decorators on a method
        let result = parse_and_check(r#"
            function Log<F>(method: F): F { return method; }
            function Measure<F>(method: F): F { return method; }

            class Service {
                @Log
                @Measure
                doWork(): void {}
            }
        "#);
        // Should pass - both decorators are valid
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }

    // ========================================================================
    // Decorator Type Alias Tests (Milestone 3.9 Phase 2)
    // ========================================================================

    #[test]
    fn test_class_decorator_type_alias_registered() {
        // Verify ClassDecorator type alias is registered and can be referenced
        let result = parse_and_check(r#"
            // Use ClassDecorator type alias in function declaration
            function makeSealed<T>(target: T): T | void {
                return target;
            }

            @makeSealed
            class MyClass {}
        "#);
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }

    #[test]
    fn test_method_decorator_type_alias_registered() {
        // Verify MethodDecorator type alias concept works
        let result = parse_and_check(r#"
            // Method decorator function that takes function and returns function
            function log<F>(method: F): F {
                return method;
            }

            class Service {
                @log
                process(): void {}
            }
        "#);
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }

    #[test]
    fn test_field_decorator_type_alias_registered() {
        // Verify FieldDecorator signature works
        let result = parse_and_check(r#"
            // Field decorator with correct signature
            function validate<T>(target: T, fieldName: string): void {}

            class Entity {
                @validate
                name: string;
            }
        "#);
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }

    #[test]
    fn test_class_interface_registered() {
        // Verify Class<T> is registered as a type
        // This test uses Class-like pattern
        let result = parse_and_check(r#"
            class Foo {}

            // Function that accepts class-like object
            function getClassName<T>(cls: T): string {
                return "name";
            }

            let name: string = getClassName(Foo);
        "#);
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }

    #[test]
    fn test_decorator_with_generic_constraint() {
        // Verify decorator with generic type parameter works
        let result = parse_and_check(r#"
            function Injectable<T>(target: T): void {}

            @Injectable
            class UserService {}

            @Injectable
            class ProductService {}
        "#);
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }

    #[test]
    fn test_decorator_factory_with_generic() {
        // Verify decorator factory with generic works
        let result = parse_and_check(r#"
            function Route<T>(path: string): (target: T) => void {
                return (target: T): void => {};
            }

            @Route("/users")
            class UserController {}
        "#);
        assert!(result.is_ok(), "Expected ok, got {:?}", result);
    }
}
