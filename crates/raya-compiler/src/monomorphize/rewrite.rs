//! Call Site Rewriting
//!
//! Rewrites call sites to use specialized versions of generic functions and classes.

use super::{GenericId, MonoKey, MonomorphizationContext};
use crate::ir::function::IrFunction;
use crate::ir::instr::{ClassId, FunctionId, IrInstr};
use crate::ir::module::IrModule;
use raya_parser::TypeId;
use rustc_hash::FxHashMap;

/// Rewrites call sites to use specialized versions
pub struct CallSiteRewriter<'a> {
    /// The monomorphization context with all specializations
    mono_ctx: &'a MonomorphizationContext,
    /// Map from generic function to its type parameters
    generic_functions: FxHashMap<FunctionId, Vec<TypeId>>,
    /// Map from generic class to its type parameters
    generic_classes: FxHashMap<ClassId, Vec<TypeId>>,
    /// Statistics
    call_sites_rewritten: usize,
}

impl<'a> CallSiteRewriter<'a> {
    /// Create a new rewriter
    pub fn new(mono_ctx: &'a MonomorphizationContext) -> Self {
        Self {
            mono_ctx,
            generic_functions: FxHashMap::default(),
            generic_classes: FxHashMap::default(),
            call_sites_rewritten: 0,
        }
    }

    /// Register a generic function's type parameters
    pub fn register_generic_function(&mut self, func_id: FunctionId, type_params: Vec<TypeId>) {
        self.generic_functions.insert(func_id, type_params);
    }

    /// Register a generic class's type parameters
    pub fn register_generic_class(&mut self, class_id: ClassId, type_params: Vec<TypeId>) {
        self.generic_classes.insert(class_id, type_params);
    }

    /// Rewrite all call sites in the module
    pub fn rewrite(&mut self, module: &mut IrModule) -> usize {
        self.call_sites_rewritten = 0;

        // Collect function indices to iterate over (to avoid borrowing issues)
        let func_count = module.function_count();

        for func_idx in 0..func_count {
            let func_id = FunctionId::new(func_idx as u32);
            if let Some(func) = module.get_function_mut(func_id) {
                self.rewrite_function(func);
            }
        }

        self.call_sites_rewritten
    }

    /// Rewrite call sites in a single function
    fn rewrite_function(&mut self, func: &mut IrFunction) {
        for block in func.blocks_mut() {
            for instr in &mut block.instructions {
                if let Some(new_instr) = self.rewrite_instruction(instr) {
                    *instr = new_instr;
                    self.call_sites_rewritten += 1;
                }
            }
        }
    }

    /// Rewrite a single instruction if it's a generic call
    fn rewrite_instruction(&self, instr: &IrInstr) -> Option<IrInstr> {
        match instr {
            IrInstr::Call { dest, func, args } => {
                // Check if this is a call to a generic function
                if let Some(type_params) = self.generic_functions.get(func) {
                    // Infer type arguments from call arguments
                    let type_args: Vec<TypeId> = args.iter().map(|a| a.ty).collect();

                    // Look up specialized version
                    let key = MonoKey::function(*func, type_args);
                    if let Some(specialized_id) = self.mono_ctx.get_function(&key) {
                        return Some(IrInstr::Call {
                            dest: dest.clone(),
                            func: specialized_id,
                            args: args.clone(),
                        });
                    }
                }
                None
            }
            IrInstr::NewObject { dest, class } => {
                // Check if this is a generic class instantiation
                if let Some(type_params) = self.generic_classes.get(class) {
                    // Use the type params as the type args
                    let key = MonoKey::class(*class, type_params.clone());
                    if let Some(specialized_id) = self.mono_ctx.get_class(&key) {
                        return Some(IrInstr::NewObject {
                            dest: dest.clone(),
                            class: specialized_id,
                        });
                    }
                }
                None
            }
            IrInstr::CallMethod { dest, object, method, args } => {
                // Method calls on generic objects may need rewriting too
                // For now, we just pass through
                None
            }
            IrInstr::LoadField { dest, object, field } => {
                // Field access on generic objects - pass through for now
                None
            }
            IrInstr::StoreField { object, field, value } => {
                // Field stores on generic objects - pass through for now
                None
            }
            _ => None,
        }
    }

    /// Get the number of call sites rewritten
    pub fn rewritten_count(&self) -> usize {
        self.call_sites_rewritten
    }
}

/// A more sophisticated rewriter that tracks type arguments through the IR
pub struct TypeAwareRewriter<'a> {
    /// The monomorphization context
    mono_ctx: &'a MonomorphizationContext,
    /// Type arguments for each register (inferred from usage)
    register_type_args: FxHashMap<u32, Vec<TypeId>>,
    /// Statistics
    call_sites_rewritten: usize,
}

impl<'a> TypeAwareRewriter<'a> {
    /// Create a new type-aware rewriter
    pub fn new(mono_ctx: &'a MonomorphizationContext) -> Self {
        Self {
            mono_ctx,
            register_type_args: FxHashMap::default(),
            call_sites_rewritten: 0,
        }
    }

    /// Rewrite a module with type awareness
    pub fn rewrite(&mut self, module: &mut IrModule) -> usize {
        self.call_sites_rewritten = 0;

        let func_count = module.function_count();
        for func_idx in 0..func_count {
            let func_id = FunctionId::new(func_idx as u32);
            if let Some(func) = module.get_function_mut(func_id) {
                self.rewrite_function_with_types(func);
            }
        }

        self.call_sites_rewritten
    }

    /// Rewrite a function, tracking type information
    fn rewrite_function_with_types(&mut self, func: &mut IrFunction) {
        // Clear register type args for new function
        self.register_type_args.clear();

        for block in func.blocks_mut() {
            for instr in &mut block.instructions {
                // Track type arguments from NewObject
                if let IrInstr::NewObject { dest, class } = instr {
                    // Store type args for this register if we have specialization info
                    // This would be populated during the specialization phase
                }

                // Rewrite calls
                if let Some(new_instr) = self.try_rewrite_instruction(instr) {
                    *instr = new_instr;
                    self.call_sites_rewritten += 1;
                }
            }
        }
    }

    /// Try to rewrite an instruction
    fn try_rewrite_instruction(&self, instr: &IrInstr) -> Option<IrInstr> {
        match instr {
            IrInstr::Call { dest, func, args } => {
                // Infer type arguments from argument types
                let type_args: Vec<TypeId> = args.iter().map(|a| a.ty).collect();

                if !type_args.is_empty() {
                    let key = MonoKey::function(*func, type_args);
                    if let Some(specialized_id) = self.mono_ctx.get_function(&key) {
                        return Some(IrInstr::Call {
                            dest: dest.clone(),
                            func: specialized_id,
                            args: args.clone(),
                        });
                    }
                }
                None
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::block::{BasicBlock, BasicBlockId, Terminator};
    use crate::ir::value::{Register, RegisterId};

    fn make_reg(id: u32, ty: u32) -> Register {
        Register::new(RegisterId::new(id), TypeId::new(ty))
    }

    #[test]
    fn test_rewrite_function_call() {
        // Set up context with a specialization
        let mut ctx = MonomorphizationContext::new();
        let key = MonoKey::function(FunctionId::new(0), vec![TypeId::new(1)]);
        ctx.register_function(key.clone(), FunctionId::new(5)); // Specialized to func 5

        let mut rewriter = CallSiteRewriter::new(&ctx);
        rewriter.register_generic_function(FunctionId::new(0), vec![TypeId::new(100)]);

        // Create instruction calling generic func with i32 arg
        let instr = IrInstr::Call {
            dest: Some(make_reg(0, 1)),
            func: FunctionId::new(0),
            args: vec![make_reg(1, 1)], // arg with type i32
        };

        let result = rewriter.rewrite_instruction(&instr);
        assert!(result.is_some());

        if let Some(IrInstr::Call { func, .. }) = result {
            assert_eq!(func, FunctionId::new(5)); // Should be rewritten to specialized
        } else {
            panic!("Expected Call instruction");
        }
    }

    #[test]
    fn test_rewrite_new_object() {
        let mut ctx = MonomorphizationContext::new();
        let key = MonoKey::class(ClassId::new(0), vec![TypeId::new(1)]);
        ctx.register_class(key.clone(), ClassId::new(3)); // Specialized to class 3

        let mut rewriter = CallSiteRewriter::new(&ctx);
        rewriter.register_generic_class(ClassId::new(0), vec![TypeId::new(1)]);

        let instr = IrInstr::NewObject {
            dest: make_reg(0, 0),
            class: ClassId::new(0),
        };

        let result = rewriter.rewrite_instruction(&instr);
        assert!(result.is_some());

        if let Some(IrInstr::NewObject { class, .. }) = result {
            assert_eq!(class, ClassId::new(3));
        } else {
            panic!("Expected NewObject instruction");
        }
    }

    #[test]
    fn test_no_rewrite_for_non_generic() {
        let ctx = MonomorphizationContext::new();
        let rewriter = CallSiteRewriter::new(&ctx);

        // Non-generic call should not be rewritten
        let instr = IrInstr::Call {
            dest: Some(make_reg(0, 1)),
            func: FunctionId::new(0),
            args: vec![make_reg(1, 1)],
        };

        let result = rewriter.rewrite_instruction(&instr);
        assert!(result.is_none());
    }

    #[test]
    fn test_rewrite_module() {
        let mut ctx = MonomorphizationContext::new();
        let key = MonoKey::function(FunctionId::new(0), vec![TypeId::new(1)]);
        ctx.register_function(key.clone(), FunctionId::new(5));

        let mut rewriter = CallSiteRewriter::new(&ctx);
        rewriter.register_generic_function(FunctionId::new(0), vec![TypeId::new(100)]);

        // Create a module with a function that calls the generic
        let mut module = IrModule::new("test");

        let mut caller = IrFunction::new("caller", vec![], TypeId::new(0));
        let mut block = BasicBlock::new(BasicBlockId(0));
        block.add_instr(IrInstr::Call {
            dest: Some(make_reg(0, 1)),
            func: FunctionId::new(0),
            args: vec![make_reg(1, 1)],
        });
        block.set_terminator(Terminator::Return(None));
        caller.add_block(block);
        module.add_function(caller);

        // Also add a placeholder for the generic function
        let mut generic = IrFunction::new("identity", vec![make_reg(0, 100)], TypeId::new(100));
        let mut gblock = BasicBlock::new(BasicBlockId(0));
        gblock.set_terminator(Terminator::Return(Some(make_reg(0, 100))));
        generic.add_block(gblock);
        module.add_function(generic);

        let rewritten = rewriter.rewrite(&mut module);
        assert_eq!(rewritten, 1);
    }
}
