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

use crate::compiler::bytecode::{GenericTemplateInfo, MonoDebugEntry, TemplateSymbolEntry};
use crate::compiler::ir::instr::IrInstr;
use crate::compiler::ir::{NominalTypeId, FunctionId, IrModule};
use crate::compiler::type_registry::TypeRegistry;
use crate::parser::{Interner, TypeContext, TypeId};
use rustc_hash::{FxHashMap, FxHashSet};
use sha2::{Digest, Sha256};
use std::hash::Hash;

/// Identifies whether a generic entity is a function or class
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GenericId {
    /// A generic function
    Function(FunctionId),
    /// A generic class
    Class(NominalTypeId),
    /// A method on a generic class (nominal_type_id, method_index)
    Method(NominalTypeId, usize),
    /// A constructor on a generic class
    Constructor(NominalTypeId),
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
    pub fn class(nominal_type_id: NominalTypeId, type_args: Vec<TypeId>) -> Self {
        Self::new(GenericId::Class(nominal_type_id), type_args)
    }

    /// Canonical serialization of concrete type arguments for deterministic keys.
    pub fn canonical_type_args(&self) -> String {
        self.type_args
            .iter()
            .map(|ty| ty.as_u32().to_string())
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Stable key hash used for specialization dedup/mangling.
    pub fn stable_hash_hex(&self) -> String {
        let mut hasher = Sha256::new();
        match self.generic_id {
            GenericId::Function(id) => hasher.update(format!("fn:{}", id.as_u32())),
            GenericId::Class(id) => hasher.update(format!("class:{}", id.as_u32())),
            GenericId::Method(nominal_type_id, method_idx) => {
                hasher.update(format!("method:{}:{}", nominal_type_id.as_u32(), method_idx));
            }
            GenericId::Constructor(nominal_type_id) => {
                hasher.update(format!("ctor:{}", nominal_type_id.as_u32()));
            }
        }
        hasher.update(self.canonical_type_args().as_bytes());
        let digest = hasher.finalize();
        hex::encode(digest)
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
    Class(NominalTypeId),
}

/// Context tracking all monomorphized instantiations
pub struct MonomorphizationContext {
    /// Map from mono key to specialized function ID
    functions: FxHashMap<MonoKey, FunctionId>,
    /// Map from mono key to specialized class ID
    classes: FxHashMap<MonoKey, NominalTypeId>,
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
    pub fn get_class(&self, key: &MonoKey) -> Option<NominalTypeId> {
        self.classes.get(key).copied()
    }

    /// Register a specialized function
    pub fn register_function(&mut self, key: MonoKey, func_id: FunctionId) {
        self.functions.insert(key, func_id);
    }

    /// Register a specialized class
    pub fn register_class(&mut self, key: MonoKey, nominal_type_id: NominalTypeId) {
        self.classes.insert(key, nominal_type_id);
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
    pub fn class_specializations(&self) -> impl Iterator<Item = (&MonoKey, &NominalTypeId)> {
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

fn collect_structural_slot_names_from_type(
    type_ctx: &TypeContext,
    ty_id: TypeId,
    names: &mut FxHashSet<String>,
    visited: &mut FxHashSet<TypeId>,
) -> bool {
    if !visited.insert(ty_id) {
        return false;
    }
    let Some(ty) = type_ctx.get(ty_id) else {
        return false;
    };

    match ty {
        crate::parser::types::Type::Object(obj) => {
            names.extend(obj.properties.iter().map(|property| property.name.clone()));
            true
        }
        crate::parser::types::Type::Interface(interface) => {
            names.extend(
                interface
                    .properties
                    .iter()
                    .map(|property| property.name.clone()),
            );
            names.extend(interface.methods.iter().map(|method| method.name.clone()));
            true
        }
        crate::parser::types::Type::Reference(reference) => type_ctx
            .lookup_named_type(&reference.name)
            .is_some_and(|named| {
                collect_structural_slot_names_from_type(type_ctx, named, names, visited)
            }),
        crate::parser::types::Type::TypeVar(type_var) => {
            type_var.constraint.is_some_and(|constraint| {
                collect_structural_slot_names_from_type(type_ctx, constraint, names, visited)
            }) || type_var.default.is_some_and(|default| {
                collect_structural_slot_names_from_type(type_ctx, default, names, visited)
            })
        }
        crate::parser::types::Type::Generic(generic) => {
            collect_structural_slot_names_from_type(type_ctx, generic.base, names, visited)
        }
        crate::parser::types::Type::Union(union) => {
            let mut any = false;
            for &member in &union.members {
                any |= collect_structural_slot_names_from_type(type_ctx, member, names, visited);
            }
            any
        }
        _ => false,
    }
}

fn structural_slot_index_for_property(
    type_ctx: &TypeContext,
    ty_id: TypeId,
    property: &str,
) -> Option<u16> {
    let mut names = FxHashSet::default();
    let mut visited = FxHashSet::default();
    if !collect_structural_slot_names_from_type(type_ctx, ty_id, &mut names, &mut visited) {
        return None;
    }

    let mut names: Vec<String> = names.into_iter().collect();
    names.sort_unstable();
    names.dedup();
    let idx = names.iter().position(|name| name == property)?;
    u16::try_from(idx).ok()
}

/// Resolve any `LateBoundMember` instructions in the IR module.
///
/// After monomorphization, TypeVar registers have been substituted with concrete types.
/// This pass replaces `LateBoundMember` instructions with the correct concrete opcodes
/// (e.g., ArrayLen, StringLen, LoadFieldExact) based on the now-known object type.
pub fn resolve_late_bound_members(
    ir_module: &mut IrModule,
    type_registry: &TypeRegistry,
    type_ctx: &TypeContext,
) {
    for func in &mut ir_module.functions {
        for block in &mut func.blocks {
            for instr in &mut block.instructions {
                if let IrInstr::LateBoundMember {
                    dest,
                    object,
                    property,
                } = instr
                {
                    let obj_ty = object.ty;
                    let dispatch_ty = type_registry
                        .normalize_type(obj_ty.as_u32(), type_ctx)
                        .unwrap_or(crate::compiler::type_registry::UNRESOLVED_TYPE_ID);

                    if dispatch_ty == crate::compiler::type_registry::UNRESOLVED_TYPE_ID {
                        if let Some(field) =
                            structural_slot_index_for_property(type_ctx, obj_ty, property)
                        {
                            *instr = IrInstr::LoadFieldExact {
                                dest: dest.clone(),
                                object: object.clone(),
                                field,
                                optional: false,
                            };
                            continue;
                        }

                        // Still unresolved after substitution: keep name-based dynamic lookup.
                        *instr = IrInstr::DynGetProp {
                            dest: dest.clone(),
                            object: object.clone(),
                            property: property.clone(),
                        };
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

                    if let Some(field) =
                        structural_slot_index_for_property(type_ctx, obj_ty, property)
                    {
                        *instr = IrInstr::LoadFieldExact {
                            dest: dest.clone(),
                            object: object.clone(),
                            field,
                            optional: false,
                        };
                        continue;
                    }

                    // Conservative dynamic fallback: preserve property-name dispatch.
                    // Avoid unsafe slot-0 field assumptions for unresolved layouts.
                    *instr = IrInstr::DynGetProp {
                        dest: dest.clone(),
                        object: object.clone(),
                        property: property.clone(),
                    };
                }
            }
        }
    }
}

/// Collect generic template metadata from IR module.
pub fn collect_generic_templates(ir_module: &IrModule) -> Vec<GenericTemplateInfo> {
    let mut templates = Vec::new();

    for (idx, func) in ir_module.functions.iter().enumerate() {
        if func.type_param_ids.is_empty() {
            continue;
        }
        let type_params = func
            .type_param_ids
            .iter()
            .map(|ty| format!("T{}", ty.as_u32()))
            .collect::<Vec<_>>();
        let template_id = format!("fn:{}:{}", idx, func.name);
        let mut fp_hasher = Sha256::new();
        fp_hasher.update(func.name.as_bytes());
        fp_hasher.update(func.type_param_ids.len().to_le_bytes());
        fp_hasher.update(func.instruction_count().to_le_bytes());
        let body_fingerprint = hex::encode(fp_hasher.finalize());
        templates.push(GenericTemplateInfo {
            template_id,
            symbol: func.name.clone(),
            type_params,
            constraints: Vec::new(),
            body_fingerprint,
        });
    }

    templates
}

/// Build template->symbol table from module generic templates.
pub fn collect_template_symbol_table(ir_module: &IrModule) -> Vec<TemplateSymbolEntry> {
    collect_generic_templates(ir_module)
        .into_iter()
        .map(|template| TemplateSymbolEntry {
            template_id: template.template_id,
            symbol: template.symbol,
        })
        .collect()
}

/// Collect monomorphization debug mapping entries from IR symbol names.
pub fn collect_mono_debug_map(ir_module: &IrModule) -> Vec<MonoDebugEntry> {
    let mut out = Vec::new();

    for func in &ir_module.functions {
        if let Some((template, _)) = func.name.split_once("__mono_") {
            out.push(MonoDebugEntry {
                specialized_symbol: func.name.clone(),
                template_id: format!("fn-template:{}", template),
                concrete_args: Vec::new(),
            });
        }
    }

    for class in &ir_module.classes {
        if let Some((template, _)) = class.name.split_once("__mono_") {
            out.push(MonoDebugEntry {
                specialized_symbol: class.name.clone(),
                template_id: format!("class-template:{}", template),
                concrete_args: Vec::new(),
            });
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::ir::block::{BasicBlock, BasicBlockId, Terminator};
    use crate::compiler::ir::function::IrFunction;
    use crate::compiler::ir::instr::IrInstr;
    use crate::compiler::ir::module::IrModule;
    use crate::compiler::ir::value::{Register, RegisterId};
    use crate::parser::ast::Visibility;
    use crate::parser::types::ty::PropertySignature;

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

    fn make_reg(id: u32, ty_id: u32) -> Register {
        Register::new(RegisterId::new(id), TypeId::new(ty_id))
    }

    #[test]
    fn test_resolve_late_bound_member_uses_structural_load_field() {
        let mut type_ctx = TypeContext::new();
        let number_ty = TypeId::new(TypeContext::NUMBER_TYPE_ID);
        let string_ty = TypeId::new(TypeContext::STRING_TYPE_ID);
        let object_ty = type_ctx.object_type(vec![
            PropertySignature {
                name: "b".to_string(),
                ty: string_ty,
                optional: false,
                readonly: false,
                visibility: Visibility::Public,
            },
            PropertySignature {
                name: "a".to_string(),
                ty: number_ty,
                optional: false,
                readonly: false,
                visibility: Visibility::Public,
            },
        ]);

        let object = make_reg(0, object_ty.as_u32());
        let dest = make_reg(1, number_ty.as_u32());
        let mut block = BasicBlock::new(BasicBlockId::new(0));
        block.add_instr(IrInstr::LateBoundMember {
            dest: dest.clone(),
            object: object.clone(),
            property: "a".to_string(),
        });
        block.set_terminator(Terminator::Return(Some(dest.clone())));

        let mut func = IrFunction::new("main", vec![], number_ty);
        func.add_block(block);

        let mut module = IrModule::new("test");
        module.add_function(func);

        let type_registry = TypeRegistry::new(&type_ctx);
        resolve_late_bound_members(&mut module, &type_registry, &type_ctx);

        match &module.functions[0].blocks[0].instructions[0] {
            IrInstr::LoadFieldExact {
                field, optional, ..
            } => {
                assert_eq!(
                    *field, 0,
                    "sorted structural layout should map 'a' to slot 0"
                );
                assert!(!optional);
            }
            other => panic!("expected LoadFieldExact after late-bound resolution, got {other:?}"),
        }
    }
}
