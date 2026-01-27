//! Name binding - builds symbol tables from AST
//!
//! The binder walks the AST and creates symbol tables for name resolution.
//! It resolves type annotations to TypeId values and tracks all declarations.

use super::error::BindError;
use super::symbols::{Symbol, SymbolFlags, SymbolKind, SymbolTable, ScopeKind};
use crate::ast::*;
use crate::Interner;
use crate::types::{TypeContext, TypeId};

/// Binder - builds symbol tables from AST
///
/// The binder performs two main tasks:
/// 1. Creates symbol table entries for all declarations
/// 2. Resolves type annotations to TypeId values
pub struct Binder<'a> {
    symbols: SymbolTable,
    type_ctx: &'a mut TypeContext,
    interner: &'a Interner,
}

impl<'a> Binder<'a> {
    /// Create a new binder
    pub fn new(type_ctx: &'a mut TypeContext, interner: &'a Interner) -> Self {
        Binder {
            symbols: SymbolTable::new(),
            type_ctx,
            interner,
        }
    }

    /// Resolve a parser Symbol to a String
    #[inline]
    fn resolve(&self, sym: crate::Symbol) -> String {
        self.interner.resolve(sym).to_string()
    }

    /// Bind a module (entry point)
    ///
    /// Walks all top-level statements and builds the symbol table.
    pub fn bind_module(mut self, module: &Module) -> Result<SymbolTable, Vec<BindError>> {
        let mut errors = Vec::new();

        for stmt in &module.statements {
            if let Err(err) = self.bind_stmt(stmt) {
                errors.push(err);
            }
        }

        if errors.is_empty() {
            Ok(self.symbols)
        } else {
            Err(errors)
        }
    }

    /// Bind a statement
    fn bind_stmt(&mut self, stmt: &Statement) -> Result<(), BindError> {
        match stmt {
            Statement::VariableDecl(decl) => self.bind_var_decl(decl),
            Statement::FunctionDecl(func) => self.bind_function(func),
            Statement::ClassDecl(class) => self.bind_class(class),
            Statement::TypeAliasDecl(alias) => self.bind_type_alias(alias),
            Statement::Block(block) => self.bind_block(block),
            Statement::If(if_stmt) => self.bind_if(if_stmt),
            Statement::Switch(switch_stmt) => self.bind_switch(switch_stmt),
            Statement::While(while_stmt) => self.bind_while(while_stmt),
            Statement::For(for_stmt) => self.bind_for(for_stmt),
            Statement::ForOf(for_of) => self.bind_for_of(for_of),
            Statement::Try(try_stmt) => self.bind_try(try_stmt),
            // Other statements don't introduce bindings
            _ => Ok(()),
        }
    }

    /// Bind variable declaration
    fn bind_var_decl(&mut self, decl: &VariableDecl) -> Result<(), BindError> {
        // Resolve type annotation or use unknown
        let ty = match &decl.type_annotation {
            Some(ty_annot) => self.resolve_type_annotation(ty_annot)?,
            None => self.type_ctx.unknown_type(),
        };

        // Extract identifier from pattern (simplified - only handles Identifier pattern)
        let (name, span) = match &decl.pattern {
            Pattern::Identifier(ident) => (self.resolve(ident.name), ident.span),
            _ => {
                // TODO: Handle destructuring patterns
                return Ok(());
            }
        };

        let symbol = Symbol {
            name,
            kind: SymbolKind::Variable,
            ty,
            flags: SymbolFlags {
                is_exported: false,
                is_const: matches!(decl.kind, VariableKind::Const),
                is_async: false,
                is_readonly: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span,
        };

        self.symbols.define(symbol).map_err(|err| BindError::DuplicateSymbol {
            name: err.name,
            original: err.original,
            duplicate: err.duplicate,
        })
    }

    /// Bind function declaration
    fn bind_function(&mut self, func: &FunctionDecl) -> Result<(), BindError> {
        // Get parent scope ID before pushing (for defining function symbol)
        let parent_scope_id = self.symbols.current_scope_id();

        // Push function scope - type parameters, parameters, and body all share this scope
        self.symbols.push_scope(ScopeKind::Function);

        // Bind type parameters first (before resolving parameter types)
        if let Some(ref type_params) = func.type_params {
            for type_param in type_params {
                let param_name = self.resolve(type_param.name.name);
                // Create a type variable for this type parameter
                let type_var = self.type_ctx.type_variable(param_name.clone());

                let tp_symbol = Symbol {
                    name: param_name,
                    kind: SymbolKind::TypeParameter,
                    ty: type_var,
                    flags: SymbolFlags::default(),
                    scope_id: self.symbols.current_scope_id(),
                    span: type_param.span,
                };

                self.symbols.define(tp_symbol).map_err(|err| BindError::DuplicateSymbol {
                    name: err.name,
                    original: err.original,
                    duplicate: err.duplicate,
                })?;
            }
        }

        // Build function type from parameters and return type
        // Type parameters are now in scope, so we can resolve them in param types
        let mut param_types = Vec::new();
        for param in &func.params {
            let param_ty = match &param.type_annotation {
                Some(ty_annot) => self.resolve_type_annotation(ty_annot)?,
                None => self.type_ctx.unknown_type(),
            };
            param_types.push(param_ty);
        }

        let return_ty = match &func.return_type {
            Some(ty_annot) => self.resolve_type_annotation(ty_annot)?,
            None => self.type_ctx.void_type(),
        };

        let func_ty = self.type_ctx.function_type(param_types.clone(), return_ty, func.is_async);

        // Define function symbol in parent scope (so it can be called recursively)
        let symbol = Symbol {
            name: self.resolve(func.name.name),
            kind: SymbolKind::Function,
            ty: func_ty,
            flags: SymbolFlags {
                is_exported: false,
                is_const: true,
                is_async: func.is_async,
                is_readonly: false,
            },
            scope_id: parent_scope_id,  // Define in parent scope
            span: func.name.span,
        };

        self.symbols.define_in_scope(parent_scope_id, symbol).map_err(|err| BindError::DuplicateSymbol {
            name: err.name,
            original: err.original,
            duplicate: err.duplicate,
        })?;

        // Bind parameters in the function scope
        // Note: param types are already resolved above, we use param_types[i]
        for (i, param) in func.params.iter().enumerate() {
            // Extract identifier from pattern (simplified)
            let (param_name, param_span) = match &param.pattern {
                Pattern::Identifier(ident) => (self.resolve(ident.name), ident.span),
                _ => continue, // Skip destructuring for now
            };

            let param_symbol = Symbol {
                name: param_name,
                kind: SymbolKind::Variable,
                ty: param_types[i],
                flags: SymbolFlags {
                    is_exported: false,
                    is_const: true,
                    is_async: false,
                    is_readonly: false,
                },
                scope_id: self.symbols.current_scope_id(),
                span: param_span,
            };

            self.symbols.define(param_symbol).map_err(|err| BindError::DuplicateSymbol {
                name: err.name,
                original: err.original,
                duplicate: err.duplicate,
            })?;
        }

        // Bind body statements
        for stmt in &func.body.statements {
            self.bind_stmt(stmt)?;
        }

        self.symbols.pop_scope();
        Ok(())
    }

    /// Bind class declaration
    fn bind_class(&mut self, class: &ClassDecl) -> Result<(), BindError> {
        use crate::types::ty::{ClassType, PropertySignature, MethodSignature, Type};

        let class_name = self.resolve(class.name.name);

        // First, create a placeholder class type and define the symbol
        // This allows the class name to be used as a return type in methods
        let placeholder_type = ClassType {
            name: class_name.clone(),
            type_params: vec![],
            properties: vec![],
            methods: vec![],
            extends: None,
            implements: vec![],
        };
        let class_ty = self.type_ctx.intern(Type::Class(placeholder_type));

        let symbol = Symbol {
            name: class_name.clone(),
            kind: SymbolKind::Class,
            ty: class_ty,
            flags: SymbolFlags {
                is_exported: false,
                is_const: true,
                is_async: false,
                is_readonly: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: class.name.span,
        };

        self.symbols.define(symbol).map_err(|err| BindError::DuplicateSymbol {
            name: err.name,
            original: err.original,
            duplicate: err.duplicate,
        })?;

        // Now collect properties and methods (class name is now resolvable)
        let mut properties = Vec::new();
        let mut methods = Vec::new();

        for member in &class.members {
            match member {
                ClassMember::Field(field) => {
                    let field_name = self.resolve(field.name.name);
                    let field_ty = if let Some(ref ann) = field.type_annotation {
                        self.resolve_type_annotation(ann)?
                    } else {
                        self.type_ctx.unknown_type()
                    };
                    properties.push(PropertySignature {
                        name: field_name,
                        ty: field_ty,
                        optional: false,
                        readonly: false,
                    });
                }
                ClassMember::Method(method) => {
                    let method_name = self.resolve(method.name.name);
                    // Create function type for the method
                    let mut params = Vec::new();
                    for p in &method.params {
                        let param_ty = if let Some(ref ann) = p.type_annotation {
                            self.resolve_type_annotation(ann)?
                        } else {
                            self.type_ctx.unknown_type()
                        };
                        params.push(param_ty);
                    }
                    // Placeholder for return type - will be fixed up below
                    let return_ty = if let Some(ref ann) = method.return_type {
                        self.resolve_type_annotation(ann)?
                    } else {
                        self.type_ctx.void_type()
                    };
                    methods.push((method_name, params, return_ty, method.is_async));
                }
                _ => {}
            }
        }

        // Create method signatures with proper return types
        // If return type equals the placeholder class_ty, we need to create a self-referential type
        // We'll create the full class type first, then fix up method return types that reference it

        // First pass: create method signatures (return types may reference placeholder)
        let method_sigs: Vec<MethodSignature> = methods
            .into_iter()
            .map(|(name, params, return_ty, is_async)| {
                // For now, use the return_ty as-is. Self-referential types will use placeholder.
                let func_ty = self.type_ctx.function_type(params, return_ty, is_async);
                MethodSignature { name, ty: func_ty }
            })
            .collect();

        // Create the full class type with properties and methods
        let full_class_type = ClassType {
            name: class_name.clone(),
            type_params: vec![],
            properties,
            methods: method_sigs,
            extends: None,
            implements: vec![],
        };
        let full_class_ty = self.type_ctx.intern(Type::Class(full_class_type));

        // Update the symbol's type with the full class type
        let scope_id = self.symbols.current_scope_id();
        self.symbols.update_type(scope_id, &class_name, full_class_ty);

        // Bind class members in class scope
        self.symbols.push_scope(ScopeKind::Class);

        for member in &class.members {
            match member {
                ClassMember::Method(method) => {
                    if let Some(ref body) = method.body {
                        self.symbols.push_scope(ScopeKind::Function);
                        for stmt in &body.statements {
                            self.bind_stmt(stmt)?;
                        }
                        self.symbols.pop_scope();
                    }
                }
                ClassMember::Constructor(ctor) => {
                    self.symbols.push_scope(ScopeKind::Function);
                    for stmt in &ctor.body.statements {
                        self.bind_stmt(stmt)?;
                    }
                    self.symbols.pop_scope();
                }
                _ => {}
            }
        }

        self.symbols.pop_scope();
        Ok(())
    }

    /// Bind type alias declaration
    fn bind_type_alias(&mut self, alias: &TypeAliasDecl) -> Result<(), BindError> {
        // Resolve the type annotation
        let ty = self.resolve_type_annotation(&alias.type_annotation)?;

        let symbol = Symbol {
            name: self.resolve(alias.name.name),
            kind: SymbolKind::TypeAlias,
            ty,
            flags: SymbolFlags::default(),
            scope_id: self.symbols.current_scope_id(),
            span: alias.name.span,
        };

        self.symbols.define(symbol).map_err(|err| BindError::DuplicateSymbol {
            name: err.name,
            original: err.original,
            duplicate: err.duplicate,
        })
    }

    /// Bind block statement
    fn bind_block(&mut self, block: &BlockStatement) -> Result<(), BindError> {
        self.symbols.push_scope(ScopeKind::Block);
        for stmt in &block.statements {
            self.bind_stmt(stmt)?;
        }
        self.symbols.pop_scope();
        Ok(())
    }

    /// Bind if statement
    fn bind_if(&mut self, if_stmt: &IfStatement) -> Result<(), BindError> {
        // Bind then branch
        self.bind_stmt(&if_stmt.then_branch)?;

        // Bind else branch if present
        if let Some(ref else_branch) = if_stmt.else_branch {
            self.bind_stmt(else_branch)?;
        }

        Ok(())
    }

    /// Bind switch statement
    fn bind_switch(&mut self, switch_stmt: &SwitchStatement) -> Result<(), BindError> {
        for case in &switch_stmt.cases {
            for stmt in &case.consequent {
                self.bind_stmt(stmt)?;
            }
        }
        Ok(())
    }

    /// Bind while loop
    fn bind_while(&mut self, while_stmt: &WhileStatement) -> Result<(), BindError> {
        self.symbols.push_scope(ScopeKind::Loop);
        self.bind_stmt(&while_stmt.body)?;
        self.symbols.pop_scope();
        Ok(())
    }

    /// Bind for loop
    fn bind_for(&mut self, for_stmt: &ForStatement) -> Result<(), BindError> {
        self.symbols.push_scope(ScopeKind::Loop);

        // Bind initializer if present
        if let Some(ref init) = for_stmt.init {
            match init {
                ForInit::VariableDecl(decl) => self.bind_var_decl(decl)?,
                ForInit::Expression(_) => {}
            }
        }

        // Bind body
        self.bind_stmt(&for_stmt.body)?;

        self.symbols.pop_scope();
        Ok(())
    }

    /// Bind for-of loop
    fn bind_for_of(&mut self, for_of: &ForOfStatement) -> Result<(), BindError> {
        self.symbols.push_scope(ScopeKind::Loop);

        // Bind the loop variable
        match &for_of.left {
            ForOfLeft::VariableDecl(decl) => self.bind_var_decl(decl)?,
            ForOfLeft::Pattern(_) => {
                // Existing variable - already bound in outer scope
            }
        }

        // Bind body
        self.bind_stmt(&for_of.body)?;

        self.symbols.pop_scope();
        Ok(())
    }

    /// Bind try-catch statement
    fn bind_try(&mut self, try_stmt: &TryStatement) -> Result<(), BindError> {
        // Bind try block
        for stmt in &try_stmt.body.statements {
            self.bind_stmt(stmt)?;
        }

        // Bind catch clause if present
        if let Some(ref catch) = try_stmt.catch_clause {
            self.symbols.push_scope(ScopeKind::Block);

            // Bind catch parameter
            if let Some(ref param) = catch.param {
                let (param_name, param_span) = match param {
                    Pattern::Identifier(ident) => (self.resolve(ident.name), ident.span),
                    _ => {
                        // TODO: Handle destructuring
                        ("error".to_string(), catch.body.span)
                    }
                };

                let param_symbol = Symbol {
                    name: param_name,
                    kind: SymbolKind::Variable,
                    ty: self.type_ctx.unknown_type(),
                    flags: SymbolFlags {
                        is_exported: false,
                        is_const: true,
                        is_async: false,
                        is_readonly: false,
                    },
                    scope_id: self.symbols.current_scope_id(),
                    span: param_span,
                };

                self.symbols.define(param_symbol).map_err(|err| BindError::DuplicateSymbol {
                    name: err.name,
                    original: err.original,
                    duplicate: err.duplicate,
                })?;
            }

            // Bind catch body
            for stmt in &catch.body.statements {
                self.bind_stmt(stmt)?;
            }

            self.symbols.pop_scope();
        }

        // Bind finally clause if present
        if let Some(ref finally_clause) = try_stmt.finally_clause {
            for stmt in &finally_clause.statements {
                self.bind_stmt(stmt)?;
            }
        }

        Ok(())
    }

    /// Resolve type annotation to TypeId
    fn resolve_type_annotation(&mut self, ty_annot: &TypeAnnotation) -> Result<TypeId, BindError> {
        self.resolve_type(&ty_annot.ty, ty_annot.span)
    }

    /// Resolve type to TypeId
    fn resolve_type(&mut self, ty: &crate::ast::Type, span: crate::Span) -> Result<TypeId, BindError> {
        use crate::ast::Type as AstType;

        match ty {
            AstType::Primitive(prim) => Ok(self.resolve_primitive(*prim)),

            AstType::Reference(type_ref) => {
                // Check if it's a user-defined type or type parameter
                let name = self.resolve(type_ref.name.name);

                // Handle built-in generic types
                if name == "Array" {
                    if let Some(ref type_args) = type_ref.type_args {
                        if type_args.len() == 1 {
                            let elem_ty = self.resolve_type_annotation(&type_args[0])?;
                            return Ok(self.type_ctx.array_type(elem_ty));
                        }
                    }
                    return Err(BindError::InvalidTypeArguments {
                        name,
                        expected: 1,
                        actual: type_ref.type_args.as_ref().map(|a| a.len()).unwrap_or(0),
                        span,
                    });
                }

                if let Some(symbol) = self.symbols.resolve(&name) {
                    if symbol.kind == SymbolKind::TypeAlias
                        || symbol.kind == SymbolKind::TypeParameter
                        || symbol.kind == SymbolKind::Class {
                        Ok(symbol.ty)
                    } else {
                        Err(BindError::NotAType {
                            name,
                            span,
                        })
                    }
                } else {
                    Err(BindError::UndefinedType {
                        name,
                        span,
                    })
                }
            }

            AstType::Array(arr) => {
                let elem_ty = self.resolve_type_annotation(&arr.element_type)?;
                Ok(self.type_ctx.array_type(elem_ty))
            }

            AstType::Tuple(tuple) => {
                let elem_tys: Result<Vec<_>, _> = tuple
                    .element_types
                    .iter()
                    .map(|e| self.resolve_type_annotation(e))
                    .collect();
                Ok(self.type_ctx.tuple_type(elem_tys?))
            }

            AstType::Union(union) => {
                let member_tys: Result<Vec<_>, _> = union
                    .types
                    .iter()
                    .map(|t| self.resolve_type_annotation(t))
                    .collect();
                Ok(self.type_ctx.union_type(member_tys?))
            }

            AstType::Function(func) => {
                let param_tys: Result<Vec<_>, _> = func
                    .params
                    .iter()
                    .map(|p| self.resolve_type_annotation(&p.ty))
                    .collect();

                let return_ty = self.resolve_type_annotation(&func.return_type)?;

                Ok(self.type_ctx.function_type(param_tys?, return_ty, false))
            }

            AstType::Object(obj) => {
                use crate::ast::ObjectTypeMember;
                use crate::types::ty::{ObjectType, PropertySignature};

                let mut properties = Vec::new();

                for member in &obj.members {
                    match member {
                        ObjectTypeMember::Property(prop) => {
                            let prop_type = self.resolve_type_annotation(&prop.ty)?;
                            properties.push(PropertySignature {
                                name: self.resolve(prop.name.name),
                                ty: prop_type,
                                optional: prop.optional,
                                readonly: false, // TODO: support readonly modifier
                            });
                        }
                        ObjectTypeMember::Method(method) => {
                            // Resolve method as a function type
                            let param_tys: Result<Vec<_>, _> = method
                                .params
                                .iter()
                                .map(|p| self.resolve_type_annotation(&p.ty))
                                .collect();

                            let return_ty = self.resolve_type_annotation(&method.return_type)?;
                            let func_ty = self.type_ctx.function_type(param_tys?, return_ty, false);

                            properties.push(PropertySignature {
                                name: self.resolve(method.name.name),
                                ty: func_ty,
                                optional: false,
                                readonly: false,
                            });
                        }
                    }
                }

                let object_type = ObjectType {
                    properties,
                    index_signature: None,
                };

                Ok(self.type_ctx.intern(crate::types::ty::Type::Object(object_type)))
            }

            AstType::Typeof(_) => {
                // typeof types are resolved during type checking
                Ok(self.type_ctx.unknown_type())
            }

            AstType::StringLiteral(s) => {
                Ok(self.type_ctx.string_literal(self.interner.resolve(*s).to_string()))
            }

            AstType::NumberLiteral(n) => {
                Ok(self.type_ctx.number_literal(*n))
            }

            AstType::BooleanLiteral(b) => {
                Ok(self.type_ctx.boolean_literal(*b))
            }

            AstType::Parenthesized(inner) => {
                self.resolve_type_annotation(inner)
            }
        }
    }

    /// Resolve primitive type to TypeId
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
    use crate::Parser;

    fn parse_and_bind(source: &str) -> (SymbolTable, TypeContext) {
        let parser = Parser::new(source).unwrap();
        let (module, interner) = parser.parse().unwrap();

        let mut ctx = TypeContext::new();
        let binder = Binder::new(&mut ctx, &interner);
        let symbols = binder.bind_module(&module).unwrap();
        (symbols, ctx)
    }

    #[test]
    fn test_bind_simple_variable() {
        let (symbols, _ctx) = parse_and_bind("let x: number = 42;");

        // Should be able to resolve x
        let symbol = symbols.resolve("x").unwrap();
        assert_eq!(symbol.name, "x");
        assert_eq!(symbol.kind, SymbolKind::Variable);
    }

    #[test]
    fn test_bind_function() {
        let (symbols, _ctx) = parse_and_bind(
            "function add(a: number, b: number): number { return a + b; }"
        );

        // Should be able to resolve add
        let symbol = symbols.resolve("add").unwrap();
        assert_eq!(symbol.name, "add");
        assert_eq!(symbol.kind, SymbolKind::Function);
    }
}
