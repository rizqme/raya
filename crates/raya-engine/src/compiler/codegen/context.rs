//! Code Generator Context
//!
//! Manages state during bytecode generation from IR.

use crate::compiler::bytecode::{
    ClassReflectionData, FieldReflectionData, Function, Module, Opcode, ReflectionData,
};
use crate::compiler::bytecode::flags;
use crate::compiler::error::{CompileError, CompileResult};
use crate::compiler::ir::{
    BasicBlock, BasicBlockId, BinaryOp, ClassId, FunctionId, IrConstant, IrFunction, IrInstr,
    IrModule, IrValue, Register, StringCompareMode, Terminator, UnaryOp,
};
use crate::compiler::module_builder::{FunctionBuilder, ModuleBuilder};
use rustc_hash::FxHashMap;

/// Code generator that transforms IR to bytecode
pub struct IrCodeGenerator {
    /// Module builder for constructing the output
    module_builder: ModuleBuilder,
    /// Current function being compiled
    current_func: Option<FunctionContext>,
}

/// Context for compiling a single function
struct FunctionContext {
    /// Function builder
    builder: FunctionBuilder,
    /// Register to local slot mapping
    register_slots: FxHashMap<u32, u16>,
    /// Next available local slot
    next_slot: u16,
    /// Block start positions for jump patching
    block_positions: FxHashMap<BasicBlockId, usize>,
    /// Pending jumps that need patching (source position, target block)
    pending_jumps: Vec<(usize, BasicBlockId)>,
    /// Pending i32 jumps for try blocks (source position, target block)
    pending_try_jumps: Vec<(usize, BasicBlockId)>,
}

impl FunctionContext {
    fn new(name: String, param_count: u8) -> Self {
        Self {
            builder: FunctionBuilder::new(name, param_count),
            register_slots: FxHashMap::default(),
            next_slot: param_count as u16,
            block_positions: FxHashMap::default(),
            pending_jumps: Vec::new(),
            pending_try_jumps: Vec::new(),
        }
    }

    /// Get or allocate a local slot for a register
    fn get_or_alloc_slot(&mut self, reg: &Register) -> u16 {
        let id = reg.id.as_u32();
        if let Some(&slot) = self.register_slots.get(&id) {
            slot
        } else {
            let slot = self.next_slot;
            self.next_slot += 1;
            self.register_slots.insert(id, slot);
            slot
        }
    }

    /// Emit an opcode
    fn emit(&mut self, opcode: Opcode) {
        self.builder.emit(opcode);
    }

    /// Emit a u8 operand
    fn emit_u8(&mut self, value: u8) {
        self.builder.emit_u8(value);
    }

    /// Emit a u16 operand
    fn emit_u16(&mut self, value: u16) {
        self.builder.emit_u16(value);
    }

    /// Emit an i16 operand
    fn emit_i16(&mut self, value: i16) {
        self.builder.emit_i16(value);
    }

    /// Emit an i32 operand
    fn emit_i32(&mut self, value: i32) {
        self.builder.emit_i32(value);
    }

    /// Emit an f64 operand
    fn emit_f64(&mut self, value: f64) {
        self.builder.emit_f64(value);
    }

    /// Emit a u32 operand
    fn emit_u32(&mut self, value: u32) {
        self.builder.code_mut().extend_from_slice(&value.to_le_bytes());
    }

    /// Get current code position
    fn current_position(&self) -> usize {
        self.builder.current_position()
    }

    /// Record block start position
    fn record_block_position(&mut self, block_id: BasicBlockId) {
        self.block_positions.insert(block_id, self.current_position());
    }

    /// Record a pending jump to be patched
    fn record_pending_jump(&mut self, target: BasicBlockId) {
        let pos = self.current_position();
        self.pending_jumps.push((pos, target));
        // Emit placeholder (i16 to match VM's read_i16)
        self.emit_i16(0);
    }

    /// Record a pending i32 try jump to be patched (for try/catch/finally offsets)
    fn record_pending_try_jump(&mut self, target: BasicBlockId) {
        let pos = self.current_position();
        self.pending_try_jumps.push((pos, target));
        // Emit placeholder (i32 for try block offsets)
        self.emit_i32(0);
    }

    /// Patch all pending jumps
    fn patch_jumps(&mut self) {
        // Patch i16 jumps (regular jumps)
        for (source_pos, target_block) in &self.pending_jumps {
            if let Some(&target_pos) = self.block_positions.get(target_block) {
                // Calculate relative offset
                // Jump is relative to the instruction AFTER the offset (2 bytes for i16)
                let offset = target_pos as i32 - (*source_pos as i32 + 2);
                let offset_i16 = offset as i16;
                let bytes = offset_i16.to_le_bytes();
                let code = self.builder.code_mut();
                code[*source_pos] = bytes[0];
                code[*source_pos + 1] = bytes[1];
            }
        }

        // Patch i32 try jumps (for exception handling)
        for (source_pos, target_block) in &self.pending_try_jumps {
            if let Some(&target_pos) = self.block_positions.get(target_block) {
                // Calculate relative offset
                // Jump is relative to the instruction AFTER the offset (4 bytes for i32)
                let offset = target_pos as i32 - (*source_pos as i32 + 4);
                let bytes = offset.to_le_bytes();
                let code = self.builder.code_mut();
                code[*source_pos] = bytes[0];
                code[*source_pos + 1] = bytes[1];
                code[*source_pos + 2] = bytes[2];
                code[*source_pos + 3] = bytes[3];
            }
        }
    }

    /// Build the final function
    fn build(mut self) -> Function {
        self.patch_jumps();
        // Update local count
        self.builder.set_local_count(self.next_slot);
        self.builder.build()
    }
}

impl IrCodeGenerator {
    /// Create a new code generator
    ///
    /// Reflection metadata is always emitted to support runtime introspection.
    pub fn new(module_name: &str) -> Self {
        Self {
            module_builder: ModuleBuilder::new(module_name.to_string()),
            current_func: None,
        }
    }

    /// Generate bytecode from an IR module
    ///
    /// Reflection data (class/field/method names) is always included.
    pub fn generate(&mut self, module: &IrModule) -> CompileResult<Module> {
        // Generate bytecode for each function
        for func in module.functions() {
            let bytecode_func = self.generate_function(func)?;
            self.module_builder.add_function(bytecode_func);
        }

        // Always generate reflection data for runtime introspection
        let reflection_data = Some(self.generate_reflection_data(module));

        // Generate class definitions with method vtable entries
        for class in module.classes() {
            let methods: Vec<crate::compiler::bytecode::Method> = class.methods.iter()
                .zip(class.method_slots.iter())
                .filter_map(|(&method_id, &slot)| {
                    module.get_function(method_id).map(|func| {
                        crate::compiler::bytecode::Method {
                            name: func.name.clone(),
                            function_id: method_id.as_u32() as usize,
                            slot: slot as usize,
                        }
                    })
                })
                .collect();
            let class_def = crate::compiler::bytecode::ClassDef {
                name: class.name.clone(),
                field_count: class.field_count(),
                parent_id: class.parent.map(|id| id.as_u32()),
                methods,
            };
            self.module_builder.add_class(class_def);
        }

        // Build and return the module
        let builder = std::mem::replace(
            &mut self.module_builder,
            ModuleBuilder::new(String::new()),
        );
        let mut bytecode_module = builder.build();

        // Add reflection data if present
        if let Some(reflection) = reflection_data {
            bytecode_module.flags |= flags::HAS_REFLECTION;
            bytecode_module.reflection = Some(reflection);
        }

        // Add native function table if present
        if !module.native_functions.is_empty() {
            bytecode_module.flags |= flags::HAS_NATIVE_FUNCTIONS;
            bytecode_module.native_functions = module.native_functions.clone();
        }

        // Compute JIT hints at compile time (pre-score functions for JIT candidacy)
        #[cfg(feature = "jit")]
        {
            use crate::jit::analysis::heuristics::HeuristicsAnalyzer;
            let analyzer = HeuristicsAnalyzer::default();
            let candidates = analyzer.select_candidates(&bytecode_module);
            if !candidates.is_empty() {
                bytecode_module.jit_hints = candidates
                    .iter()
                    .map(|c| crate::compiler::bytecode::JitHint {
                        func_index: c.func_index as u32,
                        score: c.score,
                        is_cpu_bound: c.is_cpu_bound,
                    })
                    .collect();
                bytecode_module.flags |= flags::HAS_JIT_HINTS;
            }
        }

        Ok(bytecode_module)
    }

    /// Generate reflection data from IR module
    fn generate_reflection_data(&self, module: &IrModule) -> ReflectionData {
        let mut reflection = ReflectionData::new();

        for ir_class in module.classes() {
            let mut class_reflection = ClassReflectionData::new();

            // Add field reflection data
            for field in &ir_class.fields {
                let type_name = self.get_type_name(field.ty);
                class_reflection.fields.push(FieldReflectionData::new(
                    field.name.clone(),
                    type_name,
                    field.readonly,
                    false, // Instance fields are not static
                ));
            }

            // Add method names from the function table
            for &method_id in &ir_class.methods {
                if let Some(func) = module.get_function(method_id) {
                    class_reflection.method_names.push(func.name.clone());
                }
            }

            reflection.classes.push(class_reflection);
        }

        reflection
    }

    /// Get a human-readable type name for a TypeId
    fn get_type_name(&self, type_id: crate::parser::TypeId) -> String {
        // Map well-known TypeIds to names
        // Pre-interned TypeIds from the type registry:
        // 0 = Number (f64)
        // 1 = String
        // 2 = Boolean
        // 3 = Null
        // 4 = Void
        // 5 = Never
        // 6 = Unknown
        match type_id.as_u32() {
            0 => "number".to_string(),
            1 => "string".to_string(),
            2 => "boolean".to_string(),
            3 => "null".to_string(),
            4 => "void".to_string(),
            5 => "never".to_string(),
            6 => "unknown".to_string(),
            16 => "int".to_string(),
            id => format!("type#{}", id), // Unknown type, use ID as fallback
        }
    }

    /// Generate bytecode for a single function
    fn generate_function(&mut self, func: &IrFunction) -> CompileResult<Function> {
        let param_count = func.param_count() as u8;
        let mut ctx = FunctionContext::new(func.name.clone(), param_count);

        // Pre-allocate slots for parameters
        for (i, param) in func.params.iter().enumerate() {
            ctx.register_slots.insert(param.id.as_u32(), i as u16);
        }

        // Scan IR to find all StoreLocal/LoadLocal with explicit indices and
        // bump next_slot past them to prevent temp registers from overlapping
        // named local slots.
        let mut max_fixed_index: u16 = param_count as u16;
        for block in func.blocks() {
            for instr in &block.instructions {
                match instr {
                    IrInstr::StoreLocal { index, .. } | IrInstr::LoadLocal { index, .. } => {
                        if *index >= max_fixed_index {
                            max_fixed_index = *index + 1;
                        }
                    }
                    _ => {}
                }
            }
        }
        if max_fixed_index > ctx.next_slot {
            ctx.next_slot = max_fixed_index;
        }

        // Emit bytecode for each block
        for block in func.blocks() {
            ctx.record_block_position(block.id);
            self.generate_block(&mut ctx, block)?;
        }

        Ok(ctx.build())
    }

    /// Generate bytecode for a basic block
    fn generate_block(&mut self, ctx: &mut FunctionContext, block: &BasicBlock) -> CompileResult<()> {
        // Emit instructions
        for instr in &block.instructions {
            self.generate_instr(ctx, instr)?;
        }

        // Emit terminator
        self.generate_terminator(ctx, &block.terminator)?;

        Ok(())
    }

    /// Generate bytecode for an instruction
    fn generate_instr(&mut self, ctx: &mut FunctionContext, instr: &IrInstr) -> CompileResult<()> {
        match instr {
            IrInstr::Assign { dest, value } => {
                self.emit_value(ctx, value)?;
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::BinaryOp { dest, op, left, right } => {
                // Load left operand
                self.emit_load_register(ctx, left);
                // Load right operand
                self.emit_load_register(ctx, right);
                // Emit typed operation based on operand TypeIds
                let left_ty = left.ty.as_u32();
                let right_ty = right.ty.as_u32();
                self.emit_binary_op_typed_v2(ctx, *op, left_ty, right_ty);
                // Store result
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::UnaryOp { dest, op, operand } => {
                self.emit_load_register(ctx, operand);
                self.emit_unary_op(ctx, *op, operand.ty.as_u32());
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::Call { dest, func, args } => {
                // Push arguments onto stack
                for arg in args {
                    self.emit_load_register(ctx, arg);
                }
                // Emit call (u32 funcIndex + u16 argCount per spec)
                ctx.emit(Opcode::Call);
                ctx.emit_u32(func.as_u32());
                ctx.emit_u16(args.len() as u16);
                // Store result if needed, or pop if not used
                if let Some(dest) = dest {
                    let slot = ctx.get_or_alloc_slot(dest);
                    self.emit_store_local(ctx, slot);
                } else {
                    // Pop the result that Call pushes since we don't need it
                    ctx.emit(Opcode::Pop);
                }
            }

            IrInstr::CallMethod { dest, object, method, args } => {
                // Push object
                self.emit_load_register(ctx, object);
                // Push arguments
                for arg in args {
                    self.emit_load_register(ctx, arg);
                }
                // Emit call
                ctx.emit(Opcode::CallMethod);
                ctx.emit_u32(*method as u32);
                ctx.emit_u16(args.len() as u16);
                // Store result if needed, or pop if not used
                if let Some(dest) = dest {
                    let slot = ctx.get_or_alloc_slot(dest);
                    self.emit_store_local(ctx, slot);
                } else {
                    // Pop the result that CallMethod pushes since we don't need it
                    ctx.emit(Opcode::Pop);
                }
            }

            IrInstr::NativeCall { dest, native_id, args } => {
                // Push arguments onto stack
                for arg in args {
                    self.emit_load_register(ctx, arg);
                }
                // Emit NativeCall opcode: u16 nativeId + u8 argCount
                ctx.emit(Opcode::NativeCall);
                ctx.emit_u16(*native_id);
                ctx.emit_u8(args.len() as u8);
                // Store result if needed, or pop if not used
                if let Some(dest) = dest {
                    let slot = ctx.get_or_alloc_slot(dest);
                    self.emit_store_local(ctx, slot);
                } else {
                    // Pop the result that NativeCall pushes since we don't need it
                    ctx.emit(Opcode::Pop);
                }
            }

            IrInstr::ModuleNativeCall { dest, local_idx, args } => {
                // Push arguments onto stack
                for arg in args {
                    self.emit_load_register(ctx, arg);
                }
                // Emit ModuleNativeCall opcode: u16 localIdx + u8 argCount
                ctx.emit(Opcode::ModuleNativeCall);
                ctx.emit_u16(*local_idx);
                ctx.emit_u8(args.len() as u8);
                // Store result if needed, or pop if not used
                if let Some(dest) = dest {
                    let slot = ctx.get_or_alloc_slot(dest);
                    self.emit_store_local(ctx, slot);
                } else {
                    ctx.emit(Opcode::Pop);
                }
            }

            IrInstr::InstanceOf { dest, object, class_id } => {
                // Push object onto stack
                self.emit_load_register(ctx, object);
                // Emit InstanceOf opcode with class ID
                ctx.emit(Opcode::InstanceOf);
                ctx.emit_u16(class_id.as_u32() as u16);
                // Store boolean result
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::Cast { dest, object, class_id } => {
                // Push object onto stack
                self.emit_load_register(ctx, object);
                // Emit Cast opcode with class ID
                ctx.emit(Opcode::Cast);
                ctx.emit_u16(class_id.as_u32() as u16);
                // Store casted object
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::LoadLocal { dest, index } => {
                self.emit_load_local_index(ctx, *index);
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::StoreLocal { index, value } => {
                self.emit_load_register(ctx, value);
                self.emit_store_local_index(ctx, *index);
            }

            IrInstr::LoadGlobal { dest, index } => {
                ctx.emit(Opcode::LoadGlobal);
                ctx.emit_u32(*index as u32);
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::StoreGlobal { index, value } => {
                self.emit_load_register(ctx, value);
                ctx.emit(Opcode::StoreGlobal);
                ctx.emit_u32(*index as u32);
            }

            IrInstr::LoadField { dest, object, field } => {
                self.emit_load_register(ctx, object);
                ctx.emit(Opcode::LoadField);
                ctx.emit_u16(*field);
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::StoreField { object, field, value } => {
                // VM expects: [object, value] with value on top
                // VM pops: value first, then object
                self.emit_load_register(ctx, object);
                self.emit_load_register(ctx, value);
                ctx.emit(Opcode::StoreField);
                ctx.emit_u16(*field);
            }

            IrInstr::JsonLoadProperty { dest, object, property } => {
                // Load JSON property by name (duck typing)
                // Push object, then emit JsonGet with property name index
                self.emit_load_register(ctx, object);
                let str_index = self.module_builder.add_string(property.clone())?;
                ctx.emit(Opcode::JsonGet);
                ctx.emit_u32(str_index as u32);
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::JsonStoreProperty { object, property, value } => {
                // Store JSON property by name (duck typing)
                // Push object, push value, then emit JsonSet with property name index
                self.emit_load_register(ctx, object);
                self.emit_load_register(ctx, value);
                let str_index = self.module_builder.add_string(property.clone())?;
                ctx.emit(Opcode::JsonSet);
                ctx.emit_u32(str_index as u32);
            }

            IrInstr::LoadElement { dest, array, index } => {
                self.emit_load_register(ctx, array);
                self.emit_load_register(ctx, index);
                ctx.emit(Opcode::LoadElem);
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::StoreElement { array, index, value } => {
                // Stack order: array, index, value (top)
                // VM pops: value, index, array
                self.emit_load_register(ctx, array);
                self.emit_load_register(ctx, index);
                self.emit_load_register(ctx, value);
                ctx.emit(Opcode::StoreElem);
            }

            IrInstr::NewObject { dest, class } => {
                ctx.emit(Opcode::New);
                ctx.emit_u16(class.as_u32() as u16);
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::NewArray { dest, len, elem_ty: _ } => {
                self.emit_load_register(ctx, len);
                ctx.emit(Opcode::NewArray);
                ctx.emit_u32(0); // Type index (TODO: proper type handling)
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::ArrayLiteral { dest, elements, elem_ty: _ } => {
                // Push all elements
                for elem in elements {
                    self.emit_load_register(ctx, elem);
                }
                ctx.emit(Opcode::ArrayLiteral);
                ctx.emit_u32(0); // Type index
                ctx.emit_u32(elements.len() as u32);
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::ObjectLiteral { dest, class, fields } => {
                // Create the object first (pushes it on the stack)
                ctx.emit(Opcode::ObjectLiteral);
                ctx.emit_u16(class.as_u32() as u16);
                ctx.emit_u16(fields.len() as u16);
                // Initialize each field: push value, then InitObject pops value and peeks object
                for (field_idx, value) in fields {
                    self.emit_load_register(ctx, value);
                    ctx.emit(Opcode::InitObject);
                    ctx.emit_u16(*field_idx);
                }
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::ArrayLen { dest, array } => {
                self.emit_load_register(ctx, array);
                ctx.emit(Opcode::ArrayLen);
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::ArrayPush { array, element } => {
                self.emit_load_register(ctx, array);
                self.emit_load_register(ctx, element);
                ctx.emit(Opcode::ArrayPush);
            }

            IrInstr::ArrayPop { dest, array } => {
                self.emit_load_register(ctx, array);
                ctx.emit(Opcode::ArrayPop);
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::StringLen { dest, string } => {
                self.emit_load_register(ctx, string);
                ctx.emit(Opcode::Slen);
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::Typeof { dest, operand } => {
                self.emit_load_register(ctx, operand);
                ctx.emit(Opcode::Typeof);
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::Phi { .. } => {
                // PHI nodes are handled during SSA deconstruction
                // For now, they should not appear in the IR we receive
                return Err(CompileError::UnsupportedFeature {
                    feature: "PHI nodes in code generation".to_string(),
                });
            }

            IrInstr::MakeClosure { dest, func, captures } => {
                // Push captured variables onto the stack
                for capture in captures {
                    self.emit_load_register(ctx, capture);
                }
                // Emit closure creation
                ctx.emit(Opcode::MakeClosure);
                ctx.emit_u32(func.as_u32());
                ctx.emit_u16(captures.len() as u16);
                // Store result
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::LoadCaptured { dest, index } => {
                ctx.emit(Opcode::LoadCaptured);
                ctx.emit_u16(*index);
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::StoreCaptured { index, value } => {
                self.emit_load_register(ctx, value);
                ctx.emit(Opcode::StoreCaptured);
                ctx.emit_u16(*index);
            }

            IrInstr::SetClosureCapture { closure, index, value } => {
                // Push closure and value onto stack
                self.emit_load_register(ctx, closure);
                self.emit_load_register(ctx, value);
                ctx.emit(Opcode::SetClosureCapture);
                ctx.emit_u16(*index);
                // Result (closure) is left on stack - store back to closure's slot
                let slot = ctx.get_or_alloc_slot(closure);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::NewRefCell { dest, initial_value } => {
                // Push initial value and create RefCell
                self.emit_load_register(ctx, initial_value);
                ctx.emit(Opcode::NewRefCell);
                // Store result
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::LoadRefCell { dest, refcell } => {
                // Load RefCell pointer and get its value
                self.emit_load_register(ctx, refcell);
                ctx.emit(Opcode::LoadRefCell);
                // Store result
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::StoreRefCell { refcell, value } => {
                // Push RefCell pointer and new value
                self.emit_load_register(ctx, refcell);
                self.emit_load_register(ctx, value);
                ctx.emit(Opcode::StoreRefCell);
            }

            IrInstr::CallClosure { dest, closure, args } => {
                // Push closure first (will be below args on stack)
                self.emit_load_register(ctx, closure);
                // Push arguments
                for arg in args {
                    self.emit_load_register(ctx, arg);
                }
                // Emit call with 0xFFFFFFFF to signal closure call
                ctx.emit(Opcode::Call);
                ctx.emit_u32(0xFFFFFFFF); // Special value signals closure call
                ctx.emit_u16(args.len() as u16);
                // Store result if needed, or pop if not used
                if let Some(dest) = dest {
                    let slot = ctx.get_or_alloc_slot(dest);
                    self.emit_store_local(ctx, slot);
                } else {
                    // Pop the result that Call pushes since we don't need it
                    ctx.emit(Opcode::Pop);
                }
            }

            IrInstr::StringCompare { dest, left, right, mode, negate } => {
                // Load operands
                self.emit_load_register(ctx, left);
                self.emit_load_register(ctx, right);

                // Emit comparison based on mode
                match mode {
                    StringCompareMode::Index => {
                        // O(1) index comparison for string literals
                        if *negate {
                            ctx.emit(Opcode::Ine);
                        } else {
                            ctx.emit(Opcode::Ieq);
                        }
                    }
                    StringCompareMode::Full => {
                        // O(n) full string comparison
                        if *negate {
                            ctx.emit(Opcode::Sne);
                        } else {
                            ctx.emit(Opcode::Seq);
                        }
                    }
                }

                // Store result
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::ToString { dest, operand } => {
                self.emit_load_register(ctx, operand);
                ctx.emit(Opcode::ToString);
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::Spawn { dest, func, args } => {
                // Push arguments onto stack in reverse order
                for arg in args.iter().rev() {
                    self.emit_load_register(ctx, arg);
                }
                // Emit spawn opcode with function index and arg count
                ctx.emit(Opcode::Spawn);
                ctx.emit_u16(func.as_u32() as u16);
                ctx.emit_u16(args.len() as u16);
                // Store the Task handle result
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::SpawnClosure { dest, closure, args } => {
                // Push arguments onto stack in reverse order
                for arg in args.iter().rev() {
                    self.emit_load_register(ctx, arg);
                }
                // Push the closure onto stack
                self.emit_load_register(ctx, closure);
                // Emit SpawnClosure: pops closure and args, pushes TaskHandle
                ctx.emit(Opcode::SpawnClosure);
                ctx.emit_u16(args.len() as u16);
                // Store the Task handle result
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::Await { dest, task } => {
                // Push the task handle onto stack
                self.emit_load_register(ctx, task);
                // Emit await opcode - suspends until task completes
                ctx.emit(Opcode::Await);
                // Store the result
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::AwaitAll { dest, tasks } => {
                // Push the tasks array onto stack
                self.emit_load_register(ctx, tasks);
                // Emit wait_all opcode - suspends until all tasks complete
                ctx.emit(Opcode::WaitAll);
                // Store the results array
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::Sleep { duration_ms } => {
                // Push duration onto stack
                self.emit_load_register(ctx, duration_ms);
                // Emit sleep opcode
                ctx.emit(Opcode::Sleep);
            }

            IrInstr::Yield => {
                // Emit yield opcode
                ctx.emit(Opcode::Yield);
            }

            IrInstr::NewMutex { dest } => {
                // Emit NewMutex opcode - pushes mutex reference onto stack
                ctx.emit(Opcode::NewMutex);
                // Store the mutex reference
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::NewChannel { dest, capacity } => {
                // Push capacity onto stack
                self.emit_load_register(ctx, capacity);
                // Emit NewChannel opcode - pops capacity, pushes channel reference
                ctx.emit(Opcode::NewChannel);
                // Store the channel reference
                let slot = ctx.get_or_alloc_slot(dest);
                self.emit_store_local(ctx, slot);
            }

            IrInstr::MutexLock { mutex } => {
                // Push mutex reference onto stack
                self.emit_load_register(ctx, mutex);
                // Emit MutexLock opcode - may block current task
                ctx.emit(Opcode::MutexLock);
            }

            IrInstr::MutexUnlock { mutex } => {
                // Push mutex reference onto stack
                self.emit_load_register(ctx, mutex);
                // Emit MutexUnlock opcode
                ctx.emit(Opcode::MutexUnlock);
            }

            IrInstr::TaskCancel { task } => {
                // Push task handle onto stack
                self.emit_load_register(ctx, task);
                // Emit TaskCancel opcode
                ctx.emit(Opcode::TaskCancel);
            }

            IrInstr::SetupTry { catch_block, finally_block } => {
                // Emit Try opcode with catch and finally offsets
                ctx.emit(Opcode::Try);
                // Record pending jump for catch block (i32 offset)
                ctx.record_pending_try_jump(*catch_block);
                // Record pending jump for finally block (i32 offset, 0 if no finally)
                if let Some(finally) = finally_block {
                    ctx.record_pending_try_jump(*finally);
                } else {
                    // No finally block - emit 0 offset
                    ctx.emit_i32(0);
                }
            }

            IrInstr::EndTry => {
                // Emit EndTry opcode to remove exception handler
                ctx.emit(Opcode::EndTry);
            }

            IrInstr::PopToLocal { index } => {
                // Pop exception from stack directly to local (no register load)
                // The VM pushes the exception value before jumping to catch block
                ctx.emit(Opcode::StoreLocal);
                ctx.emit_u16(*index);
            }
        }

        Ok(())
    }

    /// Generate bytecode for a terminator
    fn generate_terminator(&mut self, ctx: &mut FunctionContext, term: &Terminator) -> CompileResult<()> {
        match term {
            Terminator::Return(None) => {
                ctx.emit(Opcode::ConstNull);
                ctx.emit(Opcode::Return);
            }

            Terminator::Return(Some(reg)) => {
                self.emit_load_register(ctx, reg);
                ctx.emit(Opcode::Return);
            }

            Terminator::Jump(target) => {
                ctx.emit(Opcode::Jmp);
                ctx.record_pending_jump(*target);
            }

            Terminator::Branch { cond, then_block, else_block } => {
                self.emit_load_register(ctx, cond);
                // Jump to else block if condition is false
                ctx.emit(Opcode::JmpIfFalse);
                ctx.record_pending_jump(*else_block);
                // Fall through or jump to then block
                ctx.emit(Opcode::Jmp);
                ctx.record_pending_jump(*then_block);
            }

            Terminator::BranchIfNull {
                value,
                null_block,
                not_null_block,
            } => {
                self.emit_load_register(ctx, value);
                // Jump to null block if value is null
                ctx.emit(Opcode::JmpIfNull);
                ctx.record_pending_jump(*null_block);
                // Jump to not-null block otherwise
                ctx.emit(Opcode::Jmp);
                ctx.record_pending_jump(*not_null_block);
            }

            Terminator::Switch { value, cases, default } => {
                // For now, emit as a series of comparisons
                // TODO: Optimize with jump table for dense cases
                for (case_value, target) in cases {
                    self.emit_load_register(ctx, value);
                    ctx.emit(Opcode::ConstI32);
                    ctx.emit_i32(*case_value);
                    ctx.emit(Opcode::Ieq);
                    ctx.emit(Opcode::JmpIfTrue);
                    ctx.record_pending_jump(*target);
                }
                // Default case
                ctx.emit(Opcode::Jmp);
                ctx.record_pending_jump(*default);
            }

            Terminator::Throw(reg) => {
                self.emit_load_register(ctx, reg);
                ctx.emit(Opcode::Throw);
            }

            Terminator::Unreachable => {
                ctx.emit(Opcode::Trap);
                ctx.emit_u16(1); // Error code for unreachable
            }
        }

        Ok(())
    }

    /// Emit bytecode to load a value
    fn emit_value(&mut self, ctx: &mut FunctionContext, value: &IrValue) -> CompileResult<()> {
        match value {
            IrValue::Register(reg) => {
                self.emit_load_register(ctx, reg);
            }
            IrValue::Constant(constant) => {
                self.emit_constant(ctx, constant)?;
            }
        }
        Ok(())
    }

    /// Emit bytecode to load a constant
    fn emit_constant(&mut self, ctx: &mut FunctionContext, constant: &IrConstant) -> CompileResult<()> {
        match constant {
            IrConstant::I32(value) => {
                ctx.emit(Opcode::ConstI32);
                ctx.emit_i32(*value);
            }
            IrConstant::F64(value) => {
                ctx.emit(Opcode::ConstF64);
                ctx.emit_f64(*value);
            }
            IrConstant::String(s) => {
                let index = self.module_builder.add_string(s.clone())?;
                ctx.emit(Opcode::ConstStr);
                ctx.emit_u16(index);
            }
            IrConstant::Boolean(true) => {
                ctx.emit(Opcode::ConstTrue);
            }
            IrConstant::Boolean(false) => {
                ctx.emit(Opcode::ConstFalse);
            }
            IrConstant::Null => {
                ctx.emit(Opcode::ConstNull);
            }
        }
        Ok(())
    }

    /// Emit bytecode to load a register
    fn emit_load_register(&self, ctx: &mut FunctionContext, reg: &Register) {
        let id = reg.id.as_u32();
        if let Some(&slot) = ctx.register_slots.get(&id) {
            self.emit_load_local(ctx, slot);
        } else {
            // Register not yet assigned - allocate a slot
            let slot = ctx.get_or_alloc_slot(reg);
            self.emit_load_local(ctx, slot);
        }
    }

    /// Emit load local instruction
    fn emit_load_local(&self, ctx: &mut FunctionContext, slot: u16) {
        match slot {
            0 => ctx.emit(Opcode::LoadLocal0),
            1 => ctx.emit(Opcode::LoadLocal1),
            _ => {
                ctx.emit(Opcode::LoadLocal);
                ctx.emit_u16(slot);
            }
        }
    }

    /// Emit load local by index
    fn emit_load_local_index(&self, ctx: &mut FunctionContext, index: u16) {
        match index {
            0 => ctx.emit(Opcode::LoadLocal0),
            1 => ctx.emit(Opcode::LoadLocal1),
            _ => {
                ctx.emit(Opcode::LoadLocal);
                ctx.emit_u16(index);
            }
        }
    }

    /// Emit store local instruction
    fn emit_store_local(&self, ctx: &mut FunctionContext, slot: u16) {
        match slot {
            0 => ctx.emit(Opcode::StoreLocal0),
            1 => ctx.emit(Opcode::StoreLocal1),
            _ => {
                ctx.emit(Opcode::StoreLocal);
                ctx.emit_u16(slot);
            }
        }
    }

    /// Emit store local by index
    fn emit_store_local_index(&self, ctx: &mut FunctionContext, index: u16) {
        match index {
            0 => ctx.emit(Opcode::StoreLocal0),
            1 => ctx.emit(Opcode::StoreLocal1),
            _ => {
                ctx.emit(Opcode::StoreLocal);
                ctx.emit_u16(index);
            }
        }
    }

    /// Emit binary operation (type-aware version) - v2 with proper null/union support
    ///
    /// Pre-interned TypeIds:
    /// 0 = Number (f64), also `float`
    /// 1 = String
    /// 2 = Boolean
    /// 3 = Null
    /// 4 = Void
    /// 5 = Never
    /// 6 = Unknown
    /// 16 = Int (i32)
    fn emit_binary_op_typed_v2(&self, ctx: &mut FunctionContext, op: BinaryOp, left_ty: u32, right_ty: u32) {
        const INT_TYPE_ID: u32 = 16;

        let is_string = left_ty == 1 || right_ty == 1;
        // Generic: null (3), unknown (6), or non-primitive types (>6 except Int)
        let use_generic = left_ty == 3 || right_ty == 3
            || left_ty == 6 || right_ty == 6
            || (left_ty > 6 && left_ty != INT_TYPE_ID)
            || (right_ty > 6 && right_ty != INT_TYPE_ID);
        // Float: either operand is number/float (TypeId 0)
        let is_float = left_ty == 0 || right_ty == 0;
        // Int: both operands are int (TypeId 16)
        let is_int = left_ty == INT_TYPE_ID && right_ty == INT_TYPE_ID;

        let opcode = if is_string {
            // String operations
            match op {
                BinaryOp::Add | BinaryOp::Concat => Opcode::Sconcat,
                BinaryOp::Equal => Opcode::Seq,
                BinaryOp::NotEqual => Opcode::Sne,
                BinaryOp::Less => Opcode::Slt,
                BinaryOp::LessEqual => Opcode::Sle,
                BinaryOp::Greater => Opcode::Sgt,
                BinaryOp::GreaterEqual => Opcode::Sge,
                _ => self.get_integer_opcode(op),
            }
        } else if use_generic && matches!(op, BinaryOp::Equal | BinaryOp::NotEqual) {
            // Use generic comparison opcodes for equality with null/unknown types
            match op {
                BinaryOp::Equal => Opcode::Eq,
                BinaryOp::NotEqual => Opcode::Ne,
                _ => unreachable!(),
            }
        } else if use_generic {
            // For non-equality operations with union/generic types, use float opcodes
            // (F* opcodes auto-convert i32 via value_to_f64)
            self.get_float_opcode(op)
        } else if is_float {
            // Either operand is float/number → use F* opcodes
            // (F* opcodes auto-convert i32 via value_to_f64)
            self.get_float_opcode(op)
        } else if is_int {
            // Both operands are int → use I* opcodes
            self.get_integer_opcode(op)
        } else {
            // Default to integer for boolean, and other primitive types
            self.get_integer_opcode(op)
        };
        ctx.emit(opcode);
    }

    /// Emit binary operation (type-aware version) - legacy
    #[allow(dead_code)]
    fn emit_binary_op_typed(&self, ctx: &mut FunctionContext, op: BinaryOp, is_string: bool, use_generic: bool) {
        let opcode = if is_string {
            // String operations
            match op {
                BinaryOp::Add | BinaryOp::Concat => Opcode::Sconcat,
                BinaryOp::Equal => Opcode::Seq,
                BinaryOp::NotEqual => Opcode::Sne,
                BinaryOp::Less => Opcode::Slt,
                BinaryOp::LessEqual => Opcode::Sle,
                BinaryOp::Greater => Opcode::Sgt,
                BinaryOp::GreaterEqual => Opcode::Sge,
                // Other ops fall back to integer (shouldn't happen for strings)
                _ => self.get_integer_opcode(op),
            }
        } else if use_generic {
            // Use generic comparison opcodes for null/unknown types
            match op {
                BinaryOp::Equal => Opcode::Eq,
                BinaryOp::NotEqual => Opcode::Ne,
                // For other comparison ops with generic types, fall back to integer
                // (this may need to be extended for full generic comparison support)
                _ => self.get_integer_opcode(op),
            }
        } else {
            self.get_integer_opcode(op)
        };
        ctx.emit(opcode);
    }

    /// Get float (f64) opcode for a binary operation
    fn get_float_opcode(&self, op: BinaryOp) -> Opcode {
        match op {
            BinaryOp::Add => Opcode::Fadd,
            BinaryOp::Sub => Opcode::Fsub,
            BinaryOp::Mul => Opcode::Fmul,
            BinaryOp::Div => Opcode::Fdiv,
            BinaryOp::Mod => Opcode::Fmod,
            BinaryOp::Pow => Opcode::Fpow,
            BinaryOp::Equal => Opcode::Feq,
            BinaryOp::NotEqual => Opcode::Fne,
            BinaryOp::Less => Opcode::Flt,
            BinaryOp::LessEqual => Opcode::Fle,
            BinaryOp::Greater => Opcode::Fgt,
            BinaryOp::GreaterEqual => Opcode::Fge,
            // Logical/bitwise operations use integer opcodes
            BinaryOp::And => Opcode::And,
            BinaryOp::Or => Opcode::Or,
            BinaryOp::BitAnd => Opcode::Iand,
            BinaryOp::BitOr => Opcode::Ior,
            BinaryOp::BitXor => Opcode::Ixor,
            BinaryOp::ShiftLeft => Opcode::Ishl,
            BinaryOp::ShiftRight => Opcode::Ishr,
            BinaryOp::UnsignedShiftRight => Opcode::Iushr,
            BinaryOp::Concat => Opcode::Sconcat,
        }
    }

    /// Get integer opcode for a binary operation
    fn get_integer_opcode(&self, op: BinaryOp) -> Opcode {
        match op {
            BinaryOp::Add => Opcode::Iadd,
            BinaryOp::Sub => Opcode::Isub,
            BinaryOp::Mul => Opcode::Imul,
            BinaryOp::Div => Opcode::Idiv,
            BinaryOp::Mod => Opcode::Imod,
            BinaryOp::Pow => Opcode::Ipow,
            BinaryOp::Equal => Opcode::Ieq,
            BinaryOp::NotEqual => Opcode::Ine,
            BinaryOp::Less => Opcode::Ilt,
            BinaryOp::LessEqual => Opcode::Ile,
            BinaryOp::Greater => Opcode::Igt,
            BinaryOp::GreaterEqual => Opcode::Ige,
            BinaryOp::And => Opcode::And,
            BinaryOp::Or => Opcode::Or,
            BinaryOp::BitAnd => Opcode::Iand,
            BinaryOp::BitOr => Opcode::Ior,
            BinaryOp::BitXor => Opcode::Ixor,
            BinaryOp::ShiftLeft => Opcode::Ishl,
            BinaryOp::ShiftRight => Opcode::Ishr,
            BinaryOp::UnsignedShiftRight => Opcode::Iushr,
            BinaryOp::Concat => Opcode::Sconcat,
        }
    }

    /// Emit unary operation with type awareness
    fn emit_unary_op(&self, ctx: &mut FunctionContext, op: UnaryOp, operand_ty: u32) {
        let opcode = match op {
            UnaryOp::Neg => {
                // Use Fneg for float/number type, Ineg for int and other types
                if operand_ty == 0 {
                    Opcode::Fneg
                } else {
                    Opcode::Ineg
                }
            }
            UnaryOp::Not => Opcode::Not,
            UnaryOp::BitNot => Opcode::Inot,
        };
        ctx.emit(opcode);
    }
}
