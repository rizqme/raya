//! Symbol table implementation for name resolution
//!
//! Provides symbol tables with scope management for tracking declarations
//! and resolving identifiers during type checking.

use rustc_hash::FxHashMap;
use crate::parser::Span;
use crate::parser::types::TypeId;

/// Symbol kind (variable, function, class, type alias, etc.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    /// Variable binding
    Variable,
    /// Function declaration
    Function,
    /// Class declaration
    Class,
    /// Interface declaration
    Interface,
    /// Type alias
    TypeAlias,
    /// Type parameter (generic)
    TypeParameter,
    /// Enum member
    EnumMember,
}

/// Symbol flags for additional metadata
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SymbolFlags {
    /// Is this symbol exported from the module?
    pub is_exported: bool,
    /// Is this a const binding?
    pub is_const: bool,
    /// Is this an async function?
    pub is_async: bool,
    /// Is this a readonly property?
    pub is_readonly: bool,
}

impl Default for SymbolFlags {
    fn default() -> Self {
        SymbolFlags {
            is_exported: false,
            is_const: false,
            is_async: false,
            is_readonly: false,
        }
    }
}

/// Symbol information
#[derive(Debug, Clone)]
pub struct Symbol {
    /// Symbol name
    pub name: String,
    /// Symbol kind
    pub kind: SymbolKind,
    /// Type of this symbol
    pub ty: TypeId,
    /// Symbol flags
    pub flags: SymbolFlags,
    /// Scope where this symbol was defined
    pub scope_id: ScopeId,
    /// Source location
    pub span: Span,
}

/// Scope identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeId(pub u32);

/// Scope kind
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    /// Global scope
    Global,
    /// Function scope
    Function,
    /// Block scope
    Block,
    /// Class scope
    Class,
    /// Loop scope
    Loop,
}

/// Scope in the scope tree
#[derive(Debug, Clone)]
pub struct Scope {
    /// Scope ID
    pub id: ScopeId,
    /// Scope kind
    pub kind: ScopeKind,
    /// Parent scope (None for global scope)
    pub parent: Option<ScopeId>,
    /// Symbols defined in this scope
    pub symbols: FxHashMap<String, Symbol>,
}

impl Scope {
    /// Create a new scope
    pub fn new(id: ScopeId, kind: ScopeKind, parent: Option<ScopeId>) -> Self {
        Scope {
            id,
            kind,
            parent,
            symbols: FxHashMap::default(),
        }
    }
}

/// Symbol table with scope tree
///
/// Manages scopes and symbols for name resolution during type checking.
pub struct SymbolTable {
    /// All scopes (indexed by ScopeId)
    scopes: Vec<Scope>,
    /// Current scope ID
    current_scope: ScopeId,
}

impl SymbolTable {
    /// Create a new symbol table with a global scope
    pub fn new() -> Self {
        let global_scope = Scope::new(ScopeId(0), ScopeKind::Global, None);

        SymbolTable {
            scopes: vec![global_scope],
            current_scope: ScopeId(0),
        }
    }

    /// Get the number of scopes in the symbol table
    pub fn scope_count(&self) -> usize {
        self.scopes.len()
    }

    /// Push a new scope as a child of the current scope
    ///
    /// Returns the ID of the new scope and makes it current.
    pub fn push_scope(&mut self, kind: ScopeKind) -> ScopeId {
        let id = ScopeId(self.scopes.len() as u32);
        let scope = Scope::new(id, kind, Some(self.current_scope));
        self.scopes.push(scope);
        self.current_scope = id;
        id
    }

    /// Pop the current scope, returning to its parent
    ///
    /// Does nothing if already at global scope.
    pub fn pop_scope(&mut self) {
        if let Some(parent) = self.scopes[self.current_scope.0 as usize].parent {
            self.current_scope = parent;
        }
    }

    /// Define a symbol in the current scope
    ///
    /// Returns an error if a symbol with the same name already exists in this scope.
    pub fn define(&mut self, mut symbol: Symbol) -> Result<(), DuplicateSymbolError> {
        let scope = &mut self.scopes[self.current_scope.0 as usize];

        // Check for duplicate
        if let Some(existing) = scope.symbols.get(&symbol.name) {
            return Err(DuplicateSymbolError {
                name: symbol.name.clone(),
                original: existing.span,
                duplicate: symbol.span,
            });
        }

        // Set the scope ID
        symbol.scope_id = self.current_scope;

        // Insert symbol
        scope.symbols.insert(symbol.name.clone(), symbol);
        Ok(())
    }

    /// Define a symbol in a specific scope
    ///
    /// Returns an error if a symbol with the same name already exists in that scope.
    pub fn define_in_scope(&mut self, scope_id: ScopeId, mut symbol: Symbol) -> Result<(), DuplicateSymbolError> {
        let scope = &mut self.scopes[scope_id.0 as usize];

        // Check for duplicate
        if let Some(existing) = scope.symbols.get(&symbol.name) {
            return Err(DuplicateSymbolError {
                name: symbol.name.clone(),
                original: existing.span,
                duplicate: symbol.span,
            });
        }

        // Set the scope ID
        symbol.scope_id = scope_id;

        // Insert symbol
        scope.symbols.insert(symbol.name.clone(), symbol);
        Ok(())
    }

    /// Resolve a symbol by name, walking up the scope chain
    ///
    /// Searches from current scope to global scope, returning the first match.
    pub fn resolve(&self, name: &str) -> Option<&Symbol> {
        self.resolve_from_scope(name, self.current_scope)
    }

    /// Resolve a symbol by name from a specific scope, walking up the scope chain
    ///
    /// Searches from the given scope to global scope, returning the first match.
    pub fn resolve_from_scope(&self, name: &str, mut scope_id: ScopeId) -> Option<&Symbol> {
        // If scope ID is out of bounds (checker/binder scope mismatch),
        // fall back to searching from the last valid scope in the chain.
        if (scope_id.0 as usize) >= self.scopes.len() && !self.scopes.is_empty() {
            scope_id = ScopeId((self.scopes.len() - 1) as u32);
        }
        loop {
            let scope = match self.scopes.get(scope_id.0 as usize) {
                Some(s) => s,
                None => return None,
            };

            // Check if symbol exists in this scope
            if let Some(symbol) = scope.symbols.get(name) {
                return Some(symbol);
            }

            // Move to parent scope
            match scope.parent {
                Some(parent) => scope_id = parent,
                None => return None,
            }
        }
    }

    /// Get the current scope
    pub fn current(&self) -> &Scope {
        &self.scopes[self.current_scope.0 as usize]
    }

    /// Get the current scope ID
    pub fn current_scope_id(&self) -> ScopeId {
        self.current_scope
    }

    /// Get a scope by ID
    pub fn get_scope(&self, id: ScopeId) -> &Scope {
        &self.scopes[id.0 as usize]
    }

    /// Get the global scope
    pub fn global_scope(&self) -> &Scope {
        &self.scopes[0]
    }

    /// Get the parent scope ID of a given scope
    pub fn get_parent_scope_id(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.scopes[scope_id.0 as usize].parent
    }

    /// Update the type of a symbol in a specific scope
    ///
    /// This is used to update inferred types after type checking.
    /// Returns true if the symbol was found and updated.
    pub fn update_type(&mut self, scope_id: ScopeId, name: &str, new_ty: TypeId) -> bool {
        if let Some(scope) = self.scopes.get_mut(scope_id.0 as usize) {
            if let Some(symbol) = scope.symbols.get_mut(name) {
                symbol.ty = new_ty;
                return true;
            }
        }
        false
    }

    /// Mark a symbol as exported
    ///
    /// Searches from the current scope up to global scope and marks the first match as exported.
    /// Returns true if the symbol was found and marked.
    pub fn mark_exported(&mut self, name: &str) -> bool {
        let mut scope_id = self.current_scope;
        loop {
            let scope = &mut self.scopes[scope_id.0 as usize];

            if let Some(symbol) = scope.symbols.get_mut(name) {
                symbol.flags.is_exported = true;
                return true;
            }

            match scope.parent {
                Some(parent) => scope_id = parent,
                None => return false,
            }
        }
    }

    /// Get all exported symbols from the global scope
    ///
    /// Returns symbols that have `is_exported = true`.
    pub fn get_exported_symbols(&self) -> Vec<&Symbol> {
        self.scopes[0]
            .symbols
            .values()
            .filter(|s| s.flags.is_exported)
            .collect()
    }

    /// Define a symbol as imported (in the global scope)
    ///
    /// This is used to inject symbols from imported modules.
    pub fn define_imported(&mut self, symbol: Symbol) -> Result<(), DuplicateSymbolError> {
        self.define_in_scope(ScopeId(0), symbol)
    }
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Error indicating a duplicate symbol definition
#[derive(Debug, Clone)]
pub struct DuplicateSymbolError {
    /// Symbol name
    pub name: String,
    /// Location of original definition
    pub original: Span,
    /// Location of duplicate definition
    pub duplicate: Span,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::types::TypeContext;

    #[test]
    fn test_symbol_table_new() {
        let table = SymbolTable::new();
        assert_eq!(table.current_scope_id(), ScopeId(0));
        assert_eq!(table.current().kind, ScopeKind::Global);
    }

    #[test]
    fn test_push_pop_scope() {
        let mut table = SymbolTable::new();

        // Push function scope
        let func_scope = table.push_scope(ScopeKind::Function);
        assert_eq!(func_scope, ScopeId(1));
        assert_eq!(table.current_scope_id(), ScopeId(1));
        assert_eq!(table.current().kind, ScopeKind::Function);

        // Push block scope
        let block_scope = table.push_scope(ScopeKind::Block);
        assert_eq!(block_scope, ScopeId(2));
        assert_eq!(table.current_scope_id(), ScopeId(2));

        // Pop back to function scope
        table.pop_scope();
        assert_eq!(table.current_scope_id(), ScopeId(1));

        // Pop back to global scope
        table.pop_scope();
        assert_eq!(table.current_scope_id(), ScopeId(0));
    }

    #[test]
    fn test_define_and_resolve() {
        let mut table = SymbolTable::new();
        let mut ctx = TypeContext::new();
        let num_ty = ctx.number_type();

        // Define variable in global scope
        let symbol = Symbol {
            name: "x".to_string(),
            kind: SymbolKind::Variable,
            ty: num_ty,
            flags: SymbolFlags::default(),
            scope_id: ScopeId(0),
            span: Span::new(0, 1, 1, 1),
        };

        table.define(symbol).unwrap();

        // Resolve in same scope
        let resolved = table.resolve("x").unwrap();
        assert_eq!(resolved.name, "x");
        assert_eq!(resolved.ty, num_ty);
    }

    #[test]
    fn test_resolve_in_parent_scope() {
        let mut table = SymbolTable::new();
        let mut ctx = TypeContext::new();
        let num_ty = ctx.number_type();

        // Define in global scope
        let symbol = Symbol {
            name: "x".to_string(),
            kind: SymbolKind::Variable,
            ty: num_ty,
            flags: SymbolFlags::default(),
            scope_id: ScopeId(0),
            span: Span::new(0, 1, 1, 1),
        };
        table.define(symbol).unwrap();

        // Push new scope
        table.push_scope(ScopeKind::Function);

        // Should still resolve x from parent scope
        let resolved = table.resolve("x").unwrap();
        assert_eq!(resolved.name, "x");
    }

    #[test]
    fn test_shadow_in_nested_scope() {
        let mut table = SymbolTable::new();
        let mut ctx = TypeContext::new();
        let num_ty = ctx.number_type();
        let str_ty = ctx.string_type();

        // Define x as number in global scope
        let symbol1 = Symbol {
            name: "x".to_string(),
            kind: SymbolKind::Variable,
            ty: num_ty,
            flags: SymbolFlags::default(),
            scope_id: ScopeId(0),
            span: Span::new(0, 1, 1, 1),
        };
        table.define(symbol1).unwrap();

        // Push new scope
        table.push_scope(ScopeKind::Function);

        // Define x as string in function scope (shadowing)
        let symbol2 = Symbol {
            name: "x".to_string(),
            kind: SymbolKind::Variable,
            ty: str_ty,
            flags: SymbolFlags::default(),
            scope_id: ScopeId(1),
            span: Span::new(10, 11, 1, 10),
        };
        table.define(symbol2).unwrap();

        // Should resolve to string type (inner scope)
        let resolved = table.resolve("x").unwrap();
        assert_eq!(resolved.ty, str_ty);

        // Pop scope
        table.pop_scope();

        // Should resolve to number type (outer scope)
        let resolved = table.resolve("x").unwrap();
        assert_eq!(resolved.ty, num_ty);
    }

    #[test]
    fn test_duplicate_symbol_error() {
        let mut table = SymbolTable::new();
        let mut ctx = TypeContext::new();
        let num_ty = ctx.number_type();

        // Define x
        let symbol1 = Symbol {
            name: "x".to_string(),
            kind: SymbolKind::Variable,
            ty: num_ty,
            flags: SymbolFlags::default(),
            scope_id: ScopeId(0),
            span: Span::new(0, 1, 1, 1),
        };
        table.define(symbol1).unwrap();

        // Try to define x again in same scope
        let symbol2 = Symbol {
            name: "x".to_string(),
            kind: SymbolKind::Variable,
            ty: num_ty,
            flags: SymbolFlags::default(),
            scope_id: ScopeId(0),
            span: Span::new(10, 11, 1, 10),
        };

        let result = table.define(symbol2);
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert_eq!(err.name, "x");
        assert_eq!(err.original, Span::new(0, 1, 1, 1));
        assert_eq!(err.duplicate, Span::new(10, 11, 1, 10));
    }

    #[test]
    fn test_resolve_nonexistent() {
        let table = SymbolTable::new();
        let resolved = table.resolve("nonexistent");
        assert!(resolved.is_none());
    }
}
