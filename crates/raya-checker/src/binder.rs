//! Name binding - builds symbol tables from AST
//!
//! The binder walks the AST and creates symbol tables for name resolution.
//! It resolves type annotations to TypeId values and tracks all declarations.

use crate::error::BindError;
use crate::symbols::{Symbol, SymbolFlags, SymbolKind, SymbolTable, ScopeKind};
use raya_parser::ast::*;
use raya_types::{TypeContext, TypeId};

/// Binder - builds symbol tables from AST
///
/// The binder performs two main tasks:
/// 1. Creates symbol table entries for all declarations
/// 2. Resolves type annotations to TypeId values
pub struct Binder<'a> {
    symbols: SymbolTable,
    type_ctx: &'a mut TypeContext,
}

impl<'a> Binder<'a> {
    /// Create a new binder
    pub fn new(type_ctx: &'a mut TypeContext) -> Self {
        Binder {
            symbols: SymbolTable::new(),
            type_ctx,
        }
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
            Pattern::Identifier(ident) => (ident.name.clone(), ident.span),
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
        // Build function type from parameters and return type
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

        let func_ty = self.type_ctx.function_type(param_types, return_ty, func.is_async);

        let symbol = Symbol {
            name: func.name.name.clone(),
            kind: SymbolKind::Function,
            ty: func_ty,
            flags: SymbolFlags {
                is_exported: false,
                is_const: true,
                is_async: func.is_async,
                is_readonly: false,
            },
            scope_id: self.symbols.current_scope_id(),
            span: func.name.span,
        };

        self.symbols.define(symbol).map_err(|err| BindError::DuplicateSymbol {
            name: err.name,
            original: err.original,
            duplicate: err.duplicate,
        })?;

        // Bind function body in new scope
        self.symbols.push_scope(ScopeKind::Function);

        // Bind parameters
        for param in &func.params {
            let param_ty = match &param.type_annotation {
                Some(ty_annot) => self.resolve_type_annotation(ty_annot)?,
                None => self.type_ctx.unknown_type(),
            };

            // Extract identifier from pattern (simplified)
            let (param_name, param_span) = match &param.pattern {
                Pattern::Identifier(ident) => (ident.name.clone(), ident.span),
                _ => continue, // Skip destructuring for now
            };

            let param_symbol = Symbol {
                name: param_name,
                kind: SymbolKind::Variable,
                ty: param_ty,
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
        // For now, create a simple class type
        // TODO: Build full class type with properties and methods
        let class_ty = self.type_ctx.unknown_type();

        let symbol = Symbol {
            name: class.name.name.clone(),
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
            name: alias.name.name.clone(),
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
                    Pattern::Identifier(ident) => (ident.name.clone(), ident.span),
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
    fn resolve_type(&mut self, ty: &raya_parser::ast::Type, span: raya_parser::Span) -> Result<TypeId, BindError> {
        use raya_parser::ast::Type as AstType;

        match ty {
            AstType::Primitive(prim) => Ok(self.resolve_primitive(*prim)),

            AstType::Reference(type_ref) => {
                // Check if it's a user-defined type
                if let Some(symbol) = self.symbols.resolve(&type_ref.name.name) {
                    if symbol.kind == SymbolKind::TypeAlias {
                        Ok(symbol.ty)
                    } else {
                        Err(BindError::NotAType {
                            name: type_ref.name.name.clone(),
                            span,
                        })
                    }
                } else {
                    Err(BindError::UndefinedType {
                        name: type_ref.name.name.clone(),
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
                Ok(self.type_ctx.union_type(member_tys?, None))
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

            AstType::Object(_) => {
                // TODO: Build object type from properties
                Ok(self.type_ctx.unknown_type())
            }

            AstType::Typeof(_) => {
                // typeof types are resolved during type checking
                Ok(self.type_ctx.unknown_type())
            }

            AstType::Parenthesized(inner) => {
                self.resolve_type_annotation(inner)
            }
        }
    }

    /// Resolve primitive type to TypeId
    fn resolve_primitive(&mut self, prim: raya_parser::ast::PrimitiveType) -> TypeId {
        use raya_parser::ast::PrimitiveType as AstPrim;

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
    use raya_parser::Span;
    use raya_parser::ast::{Type as AstType, PrimitiveType as AstPrim};

    fn make_ident(name: &str) -> Identifier {
        Identifier {
            name: name.to_string(),
            span: Span::new(0, 0, 1, 1),
        }
    }

    fn make_span() -> Span {
        Span::new(0, 0, 1, 1)
    }

    #[test]
    fn test_bind_simple_variable() {
        let mut ctx = TypeContext::new();
        let binder = Binder::new(&mut ctx);

        let decl = VariableDecl {
            kind: VariableKind::Let,
            pattern: Pattern::Identifier(make_ident("x")),
            type_annotation: Some(TypeAnnotation {
                ty: AstType::Primitive(AstPrim::Number),
                span: make_span(),
            }),
            initializer: None,
            span: make_span(),
        };

        let module = Module {
            statements: vec![Statement::VariableDecl(decl)],
            span: make_span(),
        };

        let symbols = binder.bind_module(&module).unwrap();

        // Should be able to resolve x
        let symbol = symbols.resolve("x").unwrap();
        assert_eq!(symbol.name, "x");
        assert_eq!(symbol.kind, SymbolKind::Variable);
    }

    #[test]
    fn test_bind_function() {
        let mut ctx = TypeContext::new();
        let binder = Binder::new(&mut ctx);

        let func = FunctionDecl {
            name: make_ident("add"),
            type_params: None,
            params: vec![
                Parameter {
                    decorators: vec![],
                    pattern: Pattern::Identifier(make_ident("a")),
                    type_annotation: Some(TypeAnnotation {
                        ty: AstType::Primitive(AstPrim::Number),
                        span: make_span(),
                    }),
                    span: make_span(),
                },
                Parameter {
                    decorators: vec![],
                    pattern: Pattern::Identifier(make_ident("b")),
                    type_annotation: Some(TypeAnnotation {
                        ty: AstType::Primitive(AstPrim::Number),
                        span: make_span(),
                    }),
                    span: make_span(),
                },
            ],
            return_type: Some(TypeAnnotation {
                ty: AstType::Primitive(AstPrim::Number),
                span: make_span(),
            }),
            body: BlockStatement {
                statements: vec![],
                span: make_span(),
            },
            is_async: false,
            span: make_span(),
        };

        let module = Module {
            statements: vec![Statement::FunctionDecl(func)],
            span: make_span(),
        };

        let symbols = binder.bind_module(&module).unwrap();

        // Should be able to resolve add
        let symbol = symbols.resolve("add").unwrap();
        assert_eq!(symbol.name, "add");
        assert_eq!(symbol.kind, SymbolKind::Function);
    }
}
