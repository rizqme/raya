//! Function and Class Specialization
//!
//! Creates specialized versions of generic functions and classes
//! for specific type argument combinations.

use super::collect::{GenericClassInfo, GenericFunctionInfo, InstantiationTracker};
use super::substitute::TypeSubstitution;
use super::{
    GenericId, InstantiationKind, MonoKey, MonomorphizationContext, MonomorphizationResult,
    PendingInstantiation,
};
use crate::compiler::ir::function::IrFunction;
use crate::compiler::ir::instr::{ClassId, FunctionId};
use crate::compiler::ir::module::{IrClass, IrField, IrModule};
use crate::parser::{Interner, TypeContext, TypeId};
use rustc_hash::FxHashMap;

/// The main monomorphizer that processes generic code
pub struct Monomorphizer<'a> {
    /// Type context for resolving types
    type_ctx: &'a TypeContext,
    /// String interner
    interner: &'a Interner,
    /// Monomorphization context tracking all specializations
    ctx: MonomorphizationContext,
    /// Generic function definitions
    generic_functions: FxHashMap<FunctionId, GenericFunctionInfo>,
    /// Generic class definitions
    generic_classes: FxHashMap<ClassId, GenericClassInfo>,
    /// Statistics
    functions_specialized: usize,
    classes_specialized: usize,
}

impl<'a> Monomorphizer<'a> {
    /// Create a new monomorphizer
    pub fn new(type_ctx: &'a TypeContext, interner: &'a Interner) -> Self {
        Self {
            type_ctx,
            interner,
            ctx: MonomorphizationContext::new(),
            generic_functions: FxHashMap::default(),
            generic_classes: FxHashMap::default(),
            functions_specialized: 0,
            classes_specialized: 0,
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

    /// Perform monomorphization on the IR module
    ///
    /// This is the main entry point that processes all pending instantiations.
    pub fn monomorphize(&mut self, module: &mut IrModule) -> MonomorphizationResult {
        // Phase 1: Identify generic entities
        self.identify_generics(module);

        // Phase 2: Collect initial instantiations
        let initial_pending = self.collect_instantiations(module);
        for pending in initial_pending {
            self.ctx.add_pending(pending);
        }

        // Phase 3: Process all pending instantiations (including transitively discovered ones)
        while let Some(pending) = self.ctx.pop_pending() {
            self.process_instantiation(module, pending);
        }

        // Phase 4: Rewrite call sites (handled by rewrite module)
        // This will be done separately

        MonomorphizationResult {
            functions_specialized: self.functions_specialized,
            classes_specialized: self.classes_specialized,
            call_sites_rewritten: 0, // Will be updated by rewriter
        }
    }

    /// Identify generic functions and classes in the module
    fn identify_generics(&mut self, module: &IrModule) {
        // For now, we consider functions with type parameters as generic
        // In the current IR, we don't have explicit type parameters yet,
        // so we'll use a heuristic or rely on external registration
    }

    /// Collect initial instantiations from the module
    fn collect_instantiations(&self, module: &IrModule) -> Vec<PendingInstantiation> {
        let mut instantiations = Vec::new();

        // Walk through all functions looking for generic calls
        for func in module.functions() {
            for block in func.blocks() {
                for instr in &block.instructions {
                    if let Some(pending) = self.check_instruction_for_instantiation(instr) {
                        instantiations.push(pending);
                    }
                }
            }
        }

        instantiations
    }

    /// Check if an instruction requires instantiation
    fn check_instruction_for_instantiation(
        &self,
        instr: &crate::ir::instr::IrInstr,
    ) -> Option<PendingInstantiation> {
        use crate::compiler::ir::instr::IrInstr;

        match instr {
            IrInstr::Call { func, args, .. } => {
                if let Some(info) = self.generic_functions.get(func) {
                    // Infer type arguments from call
                    let type_args: Vec<TypeId> = args.iter().map(|a| a.ty).collect();
                    if !type_args.is_empty() {
                        return Some(PendingInstantiation {
                            key: MonoKey::function(*func, type_args),
                            kind: InstantiationKind::Function(*func),
                        });
                    }
                }
            }
            IrInstr::NewObject { class, .. } => {
                if let Some(info) = self.generic_classes.get(class) {
                    // Use registered type parameters
                    if !info.type_params.is_empty() {
                        return Some(PendingInstantiation {
                            key: MonoKey::class(*class, info.type_params.clone()),
                            kind: InstantiationKind::Class(*class),
                        });
                    }
                }
            }
            _ => {}
        }

        None
    }

    /// Process a single pending instantiation
    fn process_instantiation(&mut self, module: &mut IrModule, pending: PendingInstantiation) {
        // Check for cycles
        if self.ctx.is_in_progress(&pending.key) {
            return;
        }

        // Mark as in progress
        self.ctx.mark_in_progress(&pending.key);

        match pending.kind {
            InstantiationKind::Function(func_id) => {
                self.specialize_function(module, &pending.key, func_id);
            }
            InstantiationKind::Class(class_id) => {
                self.specialize_class(module, &pending.key, class_id);
            }
        }

        // Clear in progress marker
        self.ctx.clear_in_progress(&pending.key);
    }

    /// Specialize a generic function
    fn specialize_function(
        &mut self,
        module: &mut IrModule,
        key: &MonoKey,
        func_id: FunctionId,
    ) {
        // Check if already specialized
        if self.ctx.has_function(key) {
            return;
        }

        // Get the generic function
        let generic_func = match module.get_function(func_id) {
            Some(f) => f.clone(),
            None => return,
        };

        // Get type parameters for this function
        let type_params = self
            .generic_functions
            .get(&func_id)
            .map(|info| &info.type_params)
            .cloned()
            .unwrap_or_default();

        // Create type substitution
        let substitution = TypeSubstitution::from_params_and_args(&type_params, &key.type_args);

        // Apply substitution to create specialized function
        let mut specialized = substitution.apply_function(&generic_func);

        // Generate mangled name
        specialized.name = self.mangle_function_name(&generic_func.name, &key.type_args);

        // Add to module
        let new_id = module.add_function(specialized);

        // Register the specialization
        self.ctx.register_function(key.clone(), new_id);
        self.functions_specialized += 1;

        // Check for any nested generic calls in the specialized function
        self.discover_nested_instantiations(module, new_id);
    }

    /// Specialize a generic class
    fn specialize_class(
        &mut self,
        module: &mut IrModule,
        key: &MonoKey,
        class_id: ClassId,
    ) {
        // Check if already specialized
        if self.ctx.has_class(key) {
            return;
        }

        // Get the generic class
        let generic_class = match module.get_class(class_id) {
            Some(c) => c.clone(),
            None => return,
        };

        // Get type parameters for this class
        let type_params = self
            .generic_classes
            .get(&class_id)
            .map(|info| &info.type_params)
            .cloned()
            .unwrap_or_default();

        // Create type substitution
        let substitution = TypeSubstitution::from_params_and_args(&type_params, &key.type_args);

        // Create specialized class
        let mut specialized = IrClass::new(self.mangle_class_name(&generic_class.name, &key.type_args));

        // Substitute field types
        for field in &generic_class.fields {
            let new_field = IrField::new(
                field.name.clone(),
                substitution.apply(field.ty),
                field.index,
            );
            specialized.add_field(new_field);
        }

        // Copy and specialize methods
        for method_id in &generic_class.methods {
            // The method specialization would create a new specialized method
            // For now, we just copy the method ID
            specialized.add_method(*method_id);
        }

        // Copy constructor reference
        specialized.constructor = generic_class.constructor;
        specialized.parent = generic_class.parent;

        // Add to module
        let new_id = module.add_class(specialized);

        // Register the specialization
        self.ctx.register_class(key.clone(), new_id);
        self.classes_specialized += 1;
    }

    /// Discover nested generic instantiations in a specialized function
    fn discover_nested_instantiations(&mut self, module: &IrModule, func_id: FunctionId) {
        if let Some(func) = module.get_function(func_id) {
            for block in func.blocks() {
                for instr in &block.instructions {
                    if let Some(pending) = self.check_instruction_for_instantiation(instr) {
                        self.ctx.add_pending(pending);
                    }
                }
            }
        }
    }

    /// Generate a mangled name for a specialized function
    fn mangle_function_name(&self, base_name: &str, type_args: &[TypeId]) -> String {
        let mut name = base_name.to_string();
        for ty in type_args {
            name.push('_');
            name.push_str(&self.type_name(*ty));
        }
        name
    }

    /// Generate a mangled name for a specialized class
    fn mangle_class_name(&self, base_name: &str, type_args: &[TypeId]) -> String {
        let mut name = base_name.to_string();
        for ty in type_args {
            name.push('_');
            name.push_str(&self.type_name(*ty));
        }
        name
    }

    /// Get a string representation of a type for name mangling
    fn type_name(&self, ty: TypeId) -> String {
        // Use simple type ID for now
        // In a real implementation, this would resolve to actual type names
        match ty.as_u32() {
            0 => "null".to_string(),
            1 => "i32".to_string(),
            2 => "f64".to_string(),
            3 => "string".to_string(),
            4 => "bool".to_string(),
            n => format!("type{}", n),
        }
    }

    /// Get the specialized function ID for a key
    pub fn get_specialized_function(&self, key: &MonoKey) -> Option<FunctionId> {
        self.ctx.get_function(key)
    }

    /// Get the specialized class ID for a key
    pub fn get_specialized_class(&self, key: &MonoKey) -> Option<ClassId> {
        self.ctx.get_class(key)
    }

    /// Get the monomorphization context
    pub fn context(&self) -> &MonomorphizationContext {
        &self.ctx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::ir::block::{BasicBlock, BasicBlockId, Terminator};
    use crate::compiler::ir::instr::IrInstr;
    use crate::compiler::ir::value::{IrConstant, IrValue, Register, RegisterId};

    fn make_reg(id: u32, ty: u32) -> Register {
        Register::new(RegisterId::new(id), TypeId::new(ty))
    }

    fn make_simple_function(name: &str, param_ty: TypeId, return_ty: TypeId) -> IrFunction {
        let mut func = IrFunction::new(name, vec![make_reg(0, param_ty.as_u32())], return_ty);
        let mut block = BasicBlock::new(BasicBlockId(0));
        block.set_terminator(Terminator::Return(Some(make_reg(0, param_ty.as_u32()))));
        func.add_block(block);
        func
    }

    #[test]
    fn test_mangle_function_name() {
        let type_ctx = TypeContext::new();
        let interner = Interner::new();
        let mono = Monomorphizer::new(&type_ctx, &interner);

        let name = mono.mangle_function_name("identity", &[TypeId::new(1)]);
        assert_eq!(name, "identity_i32");

        let name2 = mono.mangle_function_name("pair", &[TypeId::new(1), TypeId::new(3)]);
        assert_eq!(name2, "pair_i32_string");
    }

    #[test]
    fn test_mangle_class_name() {
        let type_ctx = TypeContext::new();
        let interner = Interner::new();
        let mono = Monomorphizer::new(&type_ctx, &interner);

        let name = mono.mangle_class_name("Box", &[TypeId::new(1)]);
        assert_eq!(name, "Box_i32");
    }

    #[test]
    fn test_specialize_function() {
        let type_ctx = TypeContext::new();
        let interner = Interner::new();
        let mut mono = Monomorphizer::new(&type_ctx, &interner);

        // Create a "generic" function with type parameter (represented as TypeId 100)
        let mut module = IrModule::new("test");
        let generic_func = make_simple_function("identity", TypeId::new(100), TypeId::new(100));
        let func_id = module.add_function(generic_func);

        // Register it as generic
        mono.register_generic_function(GenericFunctionInfo {
            func_id,
            type_params: vec![TypeId::new(100)],
            name: "identity".to_string(),
        });

        // Create specialization key for identity<i32>
        let key = MonoKey::function(func_id, vec![TypeId::new(1)]);

        // Specialize
        mono.specialize_function(&mut module, &key, func_id);

        // Verify specialization was created
        assert!(mono.ctx.has_function(&key));

        // The specialized function should exist
        let specialized_id = mono.ctx.get_function(&key).unwrap();
        let specialized = module.get_function(specialized_id).unwrap();

        assert_eq!(specialized.name, "identity_i32");
        assert_eq!(specialized.return_ty, TypeId::new(1));
        assert_eq!(specialized.params[0].ty, TypeId::new(1));
    }

    #[test]
    fn test_multiple_specializations() {
        let type_ctx = TypeContext::new();
        let interner = Interner::new();
        let mut mono = Monomorphizer::new(&type_ctx, &interner);

        let mut module = IrModule::new("test");
        let generic_func = make_simple_function("identity", TypeId::new(100), TypeId::new(100));
        let func_id = module.add_function(generic_func);

        mono.register_generic_function(GenericFunctionInfo {
            func_id,
            type_params: vec![TypeId::new(100)],
            name: "identity".to_string(),
        });

        // Specialize for i32
        let key1 = MonoKey::function(func_id, vec![TypeId::new(1)]);
        mono.specialize_function(&mut module, &key1, func_id);

        // Specialize for string
        let key2 = MonoKey::function(func_id, vec![TypeId::new(3)]);
        mono.specialize_function(&mut module, &key2, func_id);

        // Both should exist
        assert!(mono.ctx.has_function(&key1));
        assert!(mono.ctx.has_function(&key2));

        // They should have different IDs
        let id1 = mono.ctx.get_function(&key1).unwrap();
        let id2 = mono.ctx.get_function(&key2).unwrap();
        assert_ne!(id1, id2);

        // Check names
        assert_eq!(module.get_function(id1).unwrap().name, "identity_i32");
        assert_eq!(module.get_function(id2).unwrap().name, "identity_string");
    }

    #[test]
    fn test_specialize_class() {
        let type_ctx = TypeContext::new();
        let interner = Interner::new();
        let mut mono = Monomorphizer::new(&type_ctx, &interner);

        let mut module = IrModule::new("test");

        // Create a generic class with a field of type T
        let mut generic_class = IrClass::new("Box");
        generic_class.add_field(IrField::new("value", TypeId::new(100), 0));
        let class_id = module.add_class(generic_class);

        mono.register_generic_class(GenericClassInfo {
            class_id,
            type_params: vec![TypeId::new(100)],
            name: "Box".to_string(),
        });

        // Specialize for i32
        let key = MonoKey::class(class_id, vec![TypeId::new(1)]);
        mono.specialize_class(&mut module, &key, class_id);

        // Verify
        assert!(mono.ctx.has_class(&key));

        let specialized_id = mono.ctx.get_class(&key).unwrap();
        let specialized = module.get_class(specialized_id).unwrap();

        assert_eq!(specialized.name, "Box_i32");
        assert_eq!(specialized.fields[0].ty, TypeId::new(1));
    }
}
