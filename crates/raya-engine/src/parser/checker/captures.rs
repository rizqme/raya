//! Closure capture analysis
//!
//! Provides data structures and algorithms for analyzing which variables
//! are captured by arrow functions (closures).

use crate::parser::Span;
use crate::parser::types::TypeId;
use rustc_hash::{FxHashMap, FxHashSet};
use super::symbols::ScopeId;

/// Information about a single captured variable
#[derive(Debug, Clone)]
pub struct CaptureInfo {
    /// Name of the captured variable
    pub name: String,
    /// Type of the captured variable
    pub ty: TypeId,
    /// Scope where the variable was originally defined
    pub defining_scope: ScopeId,
    /// Whether this capture is mutated within the closure
    pub is_mutated: bool,
    /// Span where the capture was detected (first reference)
    pub capture_span: Span,
}

/// All captures for a single closure/arrow function
#[derive(Debug, Clone, Default)]
pub struct ClosureCaptures {
    /// List of captured variables in capture order
    pub captures: Vec<CaptureInfo>,
    /// Quick lookup by variable name
    capture_indices: FxHashMap<String, usize>,
}

impl ClosureCaptures {
    /// Create a new empty capture set
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a capture to the set
    pub fn add(&mut self, info: CaptureInfo) {
        if !self.capture_indices.contains_key(&info.name) {
            self.capture_indices.insert(info.name.clone(), self.captures.len());
            self.captures.push(info);
        }
    }

    /// Mark a capture as mutated
    pub fn mark_mutated(&mut self, name: &str) {
        if let Some(&idx) = self.capture_indices.get(name) {
            self.captures[idx].is_mutated = true;
        }
    }

    /// Check if a variable is captured
    pub fn is_captured(&self, name: &str) -> bool {
        self.capture_indices.contains_key(name)
    }

    /// Get capture info by name
    pub fn get(&self, name: &str) -> Option<&CaptureInfo> {
        self.capture_indices.get(name).map(|&idx| &self.captures[idx])
    }

    /// Get number of captures
    pub fn len(&self) -> usize {
        self.captures.len()
    }

    /// Check if there are no captures
    pub fn is_empty(&self) -> bool {
        self.captures.is_empty()
    }

    /// Get captures that are mutated
    pub fn mutated_captures(&self) -> impl Iterator<Item = &CaptureInfo> {
        self.captures.iter().filter(|c| c.is_mutated)
    }

    /// Get captures that are read-only
    pub fn readonly_captures(&self) -> impl Iterator<Item = &CaptureInfo> {
        self.captures.iter().filter(|c| !c.is_mutated)
    }
}

/// Unique identifier for a closure (using AST span as ID)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClosureId(pub Span);

/// Collection of capture information for all closures in a module
#[derive(Debug, Clone, Default)]
pub struct ModuleCaptureInfo {
    /// Map from closure (identified by span) to its captures
    closures: FxHashMap<ClosureId, ClosureCaptures>,
}

impl ModuleCaptureInfo {
    /// Create a new empty module capture info
    pub fn new() -> Self {
        Self::default()
    }

    /// Add capture info for a closure
    pub fn insert(&mut self, id: ClosureId, captures: ClosureCaptures) {
        self.closures.insert(id, captures);
    }

    /// Get captures for a closure
    pub fn get(&self, id: ClosureId) -> Option<&ClosureCaptures> {
        self.closures.get(&id)
    }

    /// Get all closures that capture a specific variable
    pub fn closures_capturing(&self, var_name: &str) -> Vec<ClosureId> {
        self.closures
            .iter()
            .filter(|(_, caps)| caps.is_captured(var_name))
            .map(|(&id, _)| id)
            .collect()
    }

    /// Get total number of closures analyzed
    pub fn len(&self) -> usize {
        self.closures.len()
    }

    /// Check if no closures were analyzed
    pub fn is_empty(&self) -> bool {
        self.closures.is_empty()
    }
}

/// Helper for collecting free variables during AST traversal
#[derive(Debug)]
pub struct FreeVariableCollector {
    /// Variables bound in the current scope (parameters + local lets)
    pub bound_vars: FxHashSet<String>,
    /// Free variables found (referenced but not bound)
    free_vars: FxHashSet<String>,
    /// Assignment targets (variables that are written to)
    assigned_vars: FxHashSet<String>,
}

impl FreeVariableCollector {
    /// Create a new collector
    pub fn new() -> Self {
        Self {
            bound_vars: FxHashSet::default(),
            free_vars: FxHashSet::default(),
            assigned_vars: FxHashSet::default(),
        }
    }

    /// Mark a variable as bound (defined in current scope)
    pub fn bind(&mut self, name: String) {
        self.bound_vars.insert(name);
    }

    /// Record a variable reference
    pub fn reference(&mut self, name: &str) {
        if !self.bound_vars.contains(name) {
            self.free_vars.insert(name.to_string());
        }
    }

    /// Record a variable assignment
    pub fn assign(&mut self, name: &str) {
        if !self.bound_vars.contains(name) {
            self.free_vars.insert(name.to_string());
            self.assigned_vars.insert(name.to_string());
        }
    }

    /// Get all free variables
    pub fn free_variables(&self) -> &FxHashSet<String> {
        &self.free_vars
    }

    /// Get assigned free variables
    pub fn assigned_variables(&self) -> &FxHashSet<String> {
        &self.assigned_vars
    }

    /// Check if a free variable is assigned
    pub fn is_assigned(&self, name: &str) -> bool {
        self.assigned_vars.contains(name)
    }
}

impl Default for FreeVariableCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_closure_captures() {
        let mut captures = ClosureCaptures::new();

        captures.add(CaptureInfo {
            name: "x".to_string(),
            ty: crate::parser::types::TypeId(0),
            defining_scope: ScopeId(0),
            is_mutated: false,
            capture_span: Span::new(0, 1, 1, 1),
        });

        assert!(captures.is_captured("x"));
        assert!(!captures.is_captured("y"));
        assert_eq!(captures.len(), 1);

        captures.mark_mutated("x");
        assert!(captures.get("x").unwrap().is_mutated);
    }

    #[test]
    fn test_free_variable_collector() {
        let mut collector = FreeVariableCollector::new();

        // Bind 'x' as a parameter
        collector.bind("x".to_string());

        // Reference 'x' (bound) and 'y' (free)
        collector.reference("x");
        collector.reference("y");

        // Assign to 'z' (free, mutated)
        collector.assign("z");

        assert!(!collector.free_variables().contains("x"));
        assert!(collector.free_variables().contains("y"));
        assert!(collector.free_variables().contains("z"));
        assert!(collector.is_assigned("z"));
        assert!(!collector.is_assigned("y"));
    }
}
