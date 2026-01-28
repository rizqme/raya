//! Instantiation Collection
//!
//! Collects all generic instantiations from the IR module.
//! This is the first phase of monomorphization.

use super::{GenericId, InstantiationKind, MonoKey, PendingInstantiation};
use crate::compiler::ir::instr::{ClassId, FunctionId, IrInstr};
use crate::compiler::ir::module::IrModule;
use crate::parser::{Interner, TypeContext, TypeId};
use rustc_hash::{FxHashMap, FxHashSet};

/// Information about a generic function
#[derive(Debug, Clone)]
pub struct GenericFunctionInfo {
    /// Function ID
    pub func_id: FunctionId,
    /// Type parameter IDs
    pub type_params: Vec<TypeId>,
    /// Original function name
    pub name: String,
}

/// Information about a generic class
#[derive(Debug, Clone)]
pub struct GenericClassInfo {
    /// Class ID
    pub class_id: ClassId,
    /// Type parameter IDs
    pub type_params: Vec<TypeId>,
    /// Original class name
    pub name: String,
}

/// Collects all generic instantiations from IR
///
/// This collector walks through the IR module and finds all places where
/// generic functions or classes are used with concrete type arguments.
pub struct InstantiationCollector<'a> {
    /// Type context for resolving types
    type_ctx: &'a TypeContext,
    /// String interner
    interner: &'a Interner,
    /// Registered generic functions
    generic_functions: FxHashMap<FunctionId, GenericFunctionInfo>,
    /// Registered generic classes
    generic_classes: FxHashMap<ClassId, GenericClassInfo>,
    /// Collected instantiations
    instantiations: Vec<PendingInstantiation>,
    /// Already seen instantiations (deduplication)
    seen: FxHashSet<MonoKey>,
}

impl<'a> InstantiationCollector<'a> {
    /// Create a new collector
    pub fn new(type_ctx: &'a TypeContext, interner: &'a Interner) -> Self {
        Self {
            type_ctx,
            interner,
            generic_functions: FxHashMap::default(),
            generic_classes: FxHashMap::default(),
            instantiations: Vec::new(),
            seen: FxHashSet::default(),
        }
    }

    /// Register a generic function
    pub fn register_generic_function(&mut self, info: GenericFunctionInfo) {
        self.generic_functions.insert(info.func_id, info);
    }

    /// Register a generic class
    pub fn register_generic_class(&mut self, info: GenericClassInfo) {
        self.generic_classes.insert(info.class_id, info);
    }

    /// Check if a function is generic
    pub fn is_generic_function(&self, func_id: FunctionId) -> bool {
        self.generic_functions.contains_key(&func_id)
    }

    /// Check if a class is generic
    pub fn is_generic_class(&self, class_id: ClassId) -> bool {
        self.generic_classes.contains_key(&class_id)
    }

    /// Get generic function info
    pub fn get_generic_function(&self, func_id: FunctionId) -> Option<&GenericFunctionInfo> {
        self.generic_functions.get(&func_id)
    }

    /// Get generic class info
    pub fn get_generic_class(&self, class_id: ClassId) -> Option<&GenericClassInfo> {
        self.generic_classes.get(&class_id)
    }

    /// Collect all instantiations from the module
    pub fn collect(&mut self, module: &IrModule) -> Vec<PendingInstantiation> {
        // Walk through all functions
        for func in module.functions() {
            self.collect_from_function(func);
        }

        std::mem::take(&mut self.instantiations)
    }

    /// Collect instantiations from a single function
    fn collect_from_function(&mut self, func: &crate::ir::function::IrFunction) {
        for block in func.blocks() {
            for instr in &block.instructions {
                self.collect_from_instr(instr);
            }
        }
    }

    /// Collect instantiations from a single instruction
    fn collect_from_instr(&mut self, instr: &IrInstr) {
        match instr {
            IrInstr::Call { func, args, .. } => {
                // Check if this is a call to a generic function
                if let Some(info) = self.generic_functions.get(func) {
                    // Infer type arguments from argument types
                    let type_args = self.infer_type_args_from_call(info, args);
                    if !type_args.is_empty() {
                        self.add_function_instantiation(*func, type_args);
                    }
                }
            }
            IrInstr::NewObject { class, .. } => {
                // Check if this is a generic class instantiation
                if let Some(info) = self.generic_classes.get(class) {
                    // For now, use placeholder type inference
                    // In a real implementation, we'd get type args from the AST or type annotations
                    let type_args = info.type_params.clone();
                    if !type_args.is_empty() {
                        self.add_class_instantiation(*class, type_args);
                    }
                }
            }
            _ => {}
        }
    }

    /// Infer type arguments from call arguments
    fn infer_type_args_from_call(
        &self,
        info: &GenericFunctionInfo,
        args: &[crate::ir::value::Register],
    ) -> Vec<TypeId> {
        // Simple inference: use the types of the arguments
        // In a real implementation, this would be more sophisticated
        args.iter().map(|a| a.ty).collect()
    }

    /// Add a function instantiation
    fn add_function_instantiation(&mut self, func_id: FunctionId, type_args: Vec<TypeId>) {
        let key = MonoKey::function(func_id, type_args);
        if self.seen.insert(key.clone()) {
            self.instantiations.push(PendingInstantiation {
                key,
                kind: InstantiationKind::Function(func_id),
            });
        }
    }

    /// Add a class instantiation
    fn add_class_instantiation(&mut self, class_id: ClassId, type_args: Vec<TypeId>) {
        let key = MonoKey::class(class_id, type_args);
        if self.seen.insert(key.clone()) {
            self.instantiations.push(PendingInstantiation {
                key,
                kind: InstantiationKind::Class(class_id),
            });
        }
    }

    /// Add an instantiation explicitly (for external use)
    pub fn add_instantiation(&mut self, key: MonoKey, kind: InstantiationKind) {
        if self.seen.insert(key.clone()) {
            self.instantiations.push(PendingInstantiation { key, kind });
        }
    }
}

/// A simpler approach: track generic instantiations during IR generation
///
/// This is used when we have type information available during lowering.
#[derive(Debug, Clone)]
pub struct InstantiationTracker {
    /// Function instantiations: (generic_func_id, type_args) -> specialized_func_id
    function_instantiations: FxHashMap<MonoKey, FunctionId>,
    /// Class instantiations: (generic_class_id, type_args) -> specialized_class_id
    class_instantiations: FxHashMap<MonoKey, ClassId>,
    /// Pending instantiations to process
    pending: Vec<PendingInstantiation>,
}

impl InstantiationTracker {
    /// Create a new tracker
    pub fn new() -> Self {
        Self {
            function_instantiations: FxHashMap::default(),
            class_instantiations: FxHashMap::default(),
            pending: Vec::new(),
        }
    }

    /// Record a function instantiation
    ///
    /// Returns the specialized function ID if already instantiated,
    /// otherwise adds to pending and returns None.
    pub fn record_function_instantiation(
        &mut self,
        func_id: FunctionId,
        type_args: Vec<TypeId>,
    ) -> Option<FunctionId> {
        let key = MonoKey::function(func_id, type_args.clone());

        if let Some(&specialized) = self.function_instantiations.get(&key) {
            return Some(specialized);
        }

        // Add to pending
        self.pending.push(PendingInstantiation {
            key,
            kind: InstantiationKind::Function(func_id),
        });

        None
    }

    /// Record a class instantiation
    pub fn record_class_instantiation(
        &mut self,
        class_id: ClassId,
        type_args: Vec<TypeId>,
    ) -> Option<ClassId> {
        let key = MonoKey::class(class_id, type_args.clone());

        if let Some(&specialized) = self.class_instantiations.get(&key) {
            return Some(specialized);
        }

        // Add to pending
        self.pending.push(PendingInstantiation {
            key,
            kind: InstantiationKind::Class(class_id),
        });

        None
    }

    /// Register a completed function instantiation
    pub fn complete_function_instantiation(&mut self, key: MonoKey, specialized_id: FunctionId) {
        self.function_instantiations.insert(key, specialized_id);
    }

    /// Register a completed class instantiation
    pub fn complete_class_instantiation(&mut self, key: MonoKey, specialized_id: ClassId) {
        self.class_instantiations.insert(key, specialized_id);
    }

    /// Get pending instantiations
    pub fn pending(&self) -> &[PendingInstantiation] {
        &self.pending
    }

    /// Take all pending instantiations
    pub fn take_pending(&mut self) -> Vec<PendingInstantiation> {
        std::mem::take(&mut self.pending)
    }

    /// Get the specialized function ID for an instantiation
    pub fn get_specialized_function(&self, key: &MonoKey) -> Option<FunctionId> {
        self.function_instantiations.get(key).copied()
    }

    /// Get the specialized class ID for an instantiation
    pub fn get_specialized_class(&self, key: &MonoKey) -> Option<ClassId> {
        self.class_instantiations.get(key).copied()
    }
}

impl Default for InstantiationTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracker_record_function() {
        let mut tracker = InstantiationTracker::new();

        // First instantiation should return None and add to pending
        let result = tracker.record_function_instantiation(
            FunctionId::new(0),
            vec![TypeId::new(1)],
        );
        assert!(result.is_none());
        assert_eq!(tracker.pending().len(), 1);

        // Complete the instantiation
        let key = MonoKey::function(FunctionId::new(0), vec![TypeId::new(1)]);
        tracker.complete_function_instantiation(key.clone(), FunctionId::new(5));

        // Second instantiation with same args should return the specialized ID
        let result2 = tracker.record_function_instantiation(
            FunctionId::new(0),
            vec![TypeId::new(1)],
        );
        assert_eq!(result2, Some(FunctionId::new(5)));
    }

    #[test]
    fn test_collector_deduplication() {
        let type_ctx = TypeContext::new();
        let interner = Interner::new();
        let mut collector = InstantiationCollector::new(&type_ctx, &interner);

        // Add the same instantiation twice
        collector.add_instantiation(
            MonoKey::function(FunctionId::new(0), vec![TypeId::new(1)]),
            InstantiationKind::Function(FunctionId::new(0)),
        );
        collector.add_instantiation(
            MonoKey::function(FunctionId::new(0), vec![TypeId::new(1)]),
            InstantiationKind::Function(FunctionId::new(0)),
        );

        // Should only have one
        assert_eq!(collector.instantiations.len(), 1);
    }

    #[test]
    fn test_collector_different_type_args() {
        let type_ctx = TypeContext::new();
        let interner = Interner::new();
        let mut collector = InstantiationCollector::new(&type_ctx, &interner);

        // Add with different type args
        collector.add_instantiation(
            MonoKey::function(FunctionId::new(0), vec![TypeId::new(1)]),
            InstantiationKind::Function(FunctionId::new(0)),
        );
        collector.add_instantiation(
            MonoKey::function(FunctionId::new(0), vec![TypeId::new(2)]),
            InstantiationKind::Function(FunctionId::new(0)),
        );

        // Should have both
        assert_eq!(collector.instantiations.len(), 2);
    }
}
