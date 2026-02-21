//! Monomorphization - Generic Specialization
//!
//! This module implements monomorphization, the process of generating specialized
//! versions of generic functions and classes for each concrete type instantiation.
//!
//! # Overview
//!
//! Monomorphization eliminates generic dispatch at runtime by creating specialized
//! versions of generic code for each unique type argument combination.
//!
//! # Example
//!
//! ```typescript
//! // Source
//! function identity<T>(x: T): T { return x; }
//! let a = identity(42);       // identity<number>
//! let b = identity("hello");  // identity<string>
//!
//! // After monomorphization
//! function identity_number(x: number): number { return x; }
//! function identity_string(x: string): string { return x; }
//! let a = identity_number(42);
//! let b = identity_string("hello");
//! ```

pub mod collect;
mod rewrite;
mod specialize;
mod substitute;

pub use collect::{GenericClassInfo, GenericFunctionInfo, InstantiationCollector};
pub use rewrite::CallSiteRewriter;
pub use specialize::Monomorphizer;
pub use substitute::TypeSubstitution;

use crate::compiler::ir::{ClassId, FunctionId, IrModule};
use crate::compiler::ir::instr::IrInstr;
use crate::compiler::type_registry::TypeRegistry;
use crate::parser::{Interner, TypeContext, TypeId};
use rustc_hash::FxHashMap;
use std::hash::Hash;

/// Identifies whether a generic entity is a function or class
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GenericId {
    /// A generic function
    Function(FunctionId),
    /// A generic class
    Class(ClassId),
    /// A method on a generic class (class_id, method_index)
    Method(ClassId, usize),
    /// A constructor on a generic class
    Constructor(ClassId),
}

/// A unique key identifying a specific monomorphization
///
/// Each unique combination of generic entity + type arguments produces
/// a distinct specialized version.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MonoKey {
    /// The generic function or class being specialized
    pub generic_id: GenericId,
    /// Concrete type arguments for this instantiation
    pub type_args: Vec<TypeId>,
}

impl MonoKey {
    /// Create a new monomorphization key
    pub fn new(generic_id: GenericId, type_args: Vec<TypeId>) -> Self {
        Self {
            generic_id,
            type_args,
        }
    }

    /// Create a key for a function instantiation
    pub fn function(func_id: FunctionId, type_args: Vec<TypeId>) -> Self {
        Self::new(GenericId::Function(func_id), type_args)
    }

    /// Create a key for a class instantiation
    pub fn class(class_id: ClassId, type_args: Vec<TypeId>) -> Self {
        Self::new(GenericId::Class(class_id), type_args)
    }
}

/// A pending instantiation that needs to be processed
#[derive(Debug, Clone)]
pub struct PendingInstantiation {
    /// The unique key for this instantiation
    pub key: MonoKey,
    /// What kind of instantiation this is
    pub kind: InstantiationKind,
}

/// The kind of generic instantiation
#[derive(Debug, Clone, Copy)]
pub enum InstantiationKind {
    /// A generic function call
    Function(FunctionId),
    /// A generic class constructor
    Class(ClassId),
}

/// Context tracking all monomorphized instantiations
pub struct MonomorphizationContext {
    /// Map from mono key to specialized function ID
    functions: FxHashMap<MonoKey, FunctionId>,
    /// Map from mono key to specialized class ID
    classes: FxHashMap<MonoKey, ClassId>,
    /// Work queue of pending instantiations
    pending: Vec<PendingInstantiation>,
    /// Instantiations currently being processed (cycle detection)
    in_progress: FxHashMap<MonoKey, ()>,
}

impl MonomorphizationContext {
    /// Create a new empty context
    pub fn new() -> Self {
        Self {
            functions: FxHashMap::default(),
            classes: FxHashMap::default(),
            pending: Vec::new(),
            in_progress: FxHashMap::default(),
        }
    }

    /// Check if a function instantiation already exists
    pub fn has_function(&self, key: &MonoKey) -> bool {
        self.functions.contains_key(key)
    }

    /// Check if a class instantiation already exists
    pub fn has_class(&self, key: &MonoKey) -> bool {
        self.classes.contains_key(key)
    }

    /// Get a specialized function ID
    pub fn get_function(&self, key: &MonoKey) -> Option<FunctionId> {
        self.functions.get(key).copied()
    }

    /// Get a specialized class ID
    pub fn get_class(&self, key: &MonoKey) -> Option<ClassId> {
        self.classes.get(key).copied()
    }

    /// Register a specialized function
    pub fn register_function(&mut self, key: MonoKey, func_id: FunctionId) {
        self.functions.insert(key, func_id);
    }

    /// Register a specialized class
    pub fn register_class(&mut self, key: MonoKey, class_id: ClassId) {
        self.classes.insert(key, class_id);
    }

    /// Add a pending instantiation to process
    pub fn add_pending(&mut self, pending: PendingInstantiation) {
        // Only add if not already processed or in progress
        let key = &pending.key;
        if !self.has_function(key) && !self.has_class(key) && !self.in_progress.contains_key(key) {
            self.pending.push(pending);
        }
    }

    /// Get the next pending instantiation
    pub fn pop_pending(&mut self) -> Option<PendingInstantiation> {
        self.pending.pop()
    }

    /// Mark an instantiation as in progress (for cycle detection)
    pub fn mark_in_progress(&mut self, key: &MonoKey) {
        self.in_progress.insert(key.clone(), ());
    }

    /// Clear the in-progress marker
    pub fn clear_in_progress(&mut self, key: &MonoKey) {
        self.in_progress.remove(key);
    }

    /// Check if an instantiation is currently being processed
    pub fn is_in_progress(&self, key: &MonoKey) -> bool {
        self.in_progress.contains_key(key)
    }

    /// Get all registered function specializations
    pub fn function_specializations(&self) -> impl Iterator<Item = (&MonoKey, &FunctionId)> {
        self.functions.iter()
    }

    /// Get all registered class specializations
    pub fn class_specializations(&self) -> impl Iterator<Item = (&MonoKey, &ClassId)> {
        self.classes.iter()
    }

    /// Get the number of specialized functions
    pub fn specialized_function_count(&self) -> usize {
        self.functions.len()
    }

    /// Get the number of specialized classes
    pub fn specialized_class_count(&self) -> usize {
        self.classes.len()
    }
}

impl Default for MonomorphizationContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of monomorphization
#[derive(Debug)]
pub struct MonomorphizationResult {
    /// Number of functions specialized
    pub functions_specialized: usize,
    /// Number of classes specialized
    pub classes_specialized: usize,
    /// Number of call sites rewritten
    pub call_sites_rewritten: usize,
}

/// Perform monomorphization on an IR module
///
/// This is the main entry point for the monomorphization pass.
pub fn monomorphize(
    ir_module: &mut IrModule,
    type_ctx: &TypeContext,
    interner: &Interner,
) -> MonomorphizationResult {
    let mut monomorphizer = Monomorphizer::new(type_ctx, interner);
    monomorphizer.monomorphize(ir_module)
}

/// Resolve any `LateBoundMember` instructions in the IR module.
///
/// After monomorphization, TypeVar registers have been substituted with concrete types.
/// This pass replaces `LateBoundMember` instructions with the correct concrete opcodes
/// (e.g., ArrayLen, StringLen, LoadField) based on the now-known object type.
pub fn resolve_late_bound_members(
    ir_module: &mut IrModule,
    type_registry: &TypeRegistry,
    type_ctx: &TypeContext,
) {
    for func in &mut ir_module.functions {
        for block in &mut func.blocks {
            for instr in &mut block.instructions {
                if let IrInstr::LateBoundMember { dest, object, property } = instr {
                    let obj_ty = object.ty.as_u32();
                    let dispatch_ty = type_registry.normalize_type(obj_ty, type_ctx)
                        .unwrap_or(crate::compiler::type_registry::UNRESOLVED_TYPE_ID);

                    if dispatch_ty == crate::compiler::type_registry::UNRESOLVED_TYPE_ID {
                        // Still unresolved — leave as-is (will panic at codegen)
                        continue;
                    }

                    // Try property dispatch (e.g., .length → ArrayLen/StringLen)
                    if let Some(action) = type_registry.lookup_property(dispatch_ty, property) {
                        use crate::compiler::type_registry::{DispatchAction, OpcodeKind};
                        match action {
                            DispatchAction::Opcode(OpcodeKind::ArrayLen) => {
                                *instr = IrInstr::ArrayLen {
                                    dest: dest.clone(),
                                    array: object.clone(),
                                };
                                continue;
                            }
                            DispatchAction::Opcode(OpcodeKind::StringLen) => {
                                *instr = IrInstr::StringLen {
                                    dest: dest.clone(),
                                    string: object.clone(),
                                };
                                continue;
                            }
                            _ => {}
                        }
                    }

                    // Fall back to LoadField with field index 0
                    // (for object types where the property is a regular field)
                    *instr = IrInstr::LoadField {
                        dest: dest.clone(),
                        object: object.clone(),
                        field: 0,
                    };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mono_key_equality() {
        let key1 = MonoKey::function(FunctionId::new(0), vec![TypeId::new(1), TypeId::new(2)]);
        let key2 = MonoKey::function(FunctionId::new(0), vec![TypeId::new(1), TypeId::new(2)]);
        let key3 = MonoKey::function(FunctionId::new(0), vec![TypeId::new(1), TypeId::new(3)]);

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_mono_context_registration() {
        let mut ctx = MonomorphizationContext::new();
        let key = MonoKey::function(FunctionId::new(0), vec![TypeId::new(1)]);

        assert!(!ctx.has_function(&key));

        ctx.register_function(key.clone(), FunctionId::new(5));

        assert!(ctx.has_function(&key));
        assert_eq!(ctx.get_function(&key), Some(FunctionId::new(5)));
    }

    #[test]
    fn test_pending_deduplication() {
        let mut ctx = MonomorphizationContext::new();
        let key = MonoKey::function(FunctionId::new(0), vec![TypeId::new(1)]);

        // Add pending
        ctx.add_pending(PendingInstantiation {
            key: key.clone(),
            kind: InstantiationKind::Function(FunctionId::new(0)),
        });
        assert_eq!(ctx.pending.len(), 1);

        // Mark as in progress
        ctx.mark_in_progress(&key);

        // Try to add again - should be ignored
        ctx.add_pending(PendingInstantiation {
            key: key.clone(),
            kind: InstantiationKind::Function(FunctionId::new(0)),
        });
        assert_eq!(ctx.pending.len(), 1);
    }
}
