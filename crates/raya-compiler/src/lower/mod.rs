//! AST to IR Lowering
//!
//! Converts the type-checked AST into the IR representation.

mod control_flow;
mod expr;
mod stmt;

use crate::ir::{
    BasicBlock, BasicBlockId, ClassId, FunctionId, IrClass, IrField, IrFunction,
    IrInstr, IrModule, Register, RegisterId, Terminator,
};
use raya_parser::ast::{self, Pattern, Statement};
use raya_parser::{Interner, Symbol, TypeContext, TypeId};
use rustc_hash::FxHashMap;

/// AST to IR lowerer
pub struct Lowerer<'a> {
    /// Type context for type information
    type_ctx: &'a TypeContext,
    /// String interner
    interner: &'a Interner,
    /// Current function being lowered
    current_function: Option<IrFunction>,
    /// Current block ID
    current_block: BasicBlockId,
    /// Next register ID
    next_register: u32,
    /// Next block ID
    next_block: u32,
    /// Local variable name to index mapping
    local_map: FxHashMap<Symbol, u16>,
    /// Local variable index to register mapping
    local_registers: FxHashMap<u16, Register>,
    /// Function name to ID mapping
    function_map: FxHashMap<Symbol, FunctionId>,
    /// Class name to ID mapping
    class_map: FxHashMap<Symbol, ClassId>,
    /// Next function ID
    next_function_id: u32,
    /// Next class ID
    next_class_id: u32,
}

impl<'a> Lowerer<'a> {
    /// Create a new lowerer
    pub fn new(type_ctx: &'a TypeContext, interner: &'a Interner) -> Self {
        Self {
            type_ctx,
            interner,
            current_function: None,
            current_block: BasicBlockId(0),
            next_register: 0,
            next_block: 0,
            local_map: FxHashMap::default(),
            local_registers: FxHashMap::default(),
            function_map: FxHashMap::default(),
            class_map: FxHashMap::default(),
            next_function_id: 0,
            next_class_id: 0,
        }
    }

    /// Lower an AST module to IR
    pub fn lower_module(&mut self, module: &ast::Module) -> IrModule {
        let mut ir_module = IrModule::new("main");

        // First pass: collect function and class declarations
        for stmt in &module.statements {
            match stmt {
                Statement::FunctionDecl(func) => {
                    let id = FunctionId::new(self.next_function_id);
                    self.next_function_id += 1;
                    self.function_map.insert(func.name.name, id);
                }
                Statement::ClassDecl(class) => {
                    let id = ClassId::new(self.next_class_id);
                    self.next_class_id += 1;
                    self.class_map.insert(class.name.name, id);
                }
                _ => {}
            }
        }

        // Second pass: lower all declarations
        for stmt in &module.statements {
            match stmt {
                Statement::FunctionDecl(func) => {
                    let ir_func = self.lower_function(func);
                    ir_module.add_function(ir_func);
                }
                Statement::ClassDecl(class) => {
                    let ir_class = self.lower_class(class);
                    ir_module.add_class(ir_class);
                }
                _ => {
                    // Top-level statements go into an implicit main function
                }
            }
        }

        // Create main function for top-level statements if needed
        let top_level_stmts: Vec<_> = module
            .statements
            .iter()
            .filter(|s| !matches!(s, Statement::FunctionDecl(_) | Statement::ClassDecl(_)))
            .collect();

        if !top_level_stmts.is_empty() {
            let main_func = self.lower_top_level_statements(&top_level_stmts);
            ir_module.add_function(main_func);
        }

        ir_module
    }

    /// Lower a function declaration
    fn lower_function(&mut self, func: &ast::FunctionDecl) -> IrFunction {
        // Reset per-function state
        self.next_register = 0;
        self.next_block = 0;
        self.local_map.clear();
        self.local_registers.clear();

        // Get function name
        let name = self.interner.resolve(func.name.name);

        // Create parameter registers
        let mut params = Vec::new();
        for param in &func.params {
            let ty = param
                .type_annotation
                .as_ref()
                .map(|t| self.resolve_type_annotation(t))
                .unwrap_or(TypeId::new(0));
            let reg = self.alloc_register(ty);

            // Extract parameter name from pattern
            if let Pattern::Identifier(ident) = &param.pattern {
                let local_idx = self.allocate_local(ident.name);
                self.local_registers.insert(local_idx, reg.clone());
            }
            params.push(reg);
        }

        // Get return type
        let return_ty = func
            .return_type
            .as_ref()
            .map(|t| self.resolve_type_annotation(t))
            .unwrap_or_else(|| TypeId::new(0)); // void

        // Create function
        let ir_func = IrFunction::new(name, params, return_ty);
        self.current_function = Some(ir_func);

        // Create entry block
        let entry_block = self.alloc_block();
        self.current_block = entry_block;
        self.current_function_mut()
            .add_block(BasicBlock::with_label(entry_block, "entry"));

        // Lower function body
        for stmt in &func.body.statements {
            self.lower_stmt(stmt);
        }

        // Ensure the function ends with a return
        if !self.current_block_is_terminated() {
            self.set_terminator(Terminator::Return(None));
        }

        // Take the function out
        self.current_function.take().unwrap()
    }

    /// Lower top-level statements into a main function
    fn lower_top_level_statements(&mut self, stmts: &[&Statement]) -> IrFunction {
        // Reset per-function state
        self.next_register = 0;
        self.next_block = 0;
        self.local_map.clear();
        self.local_registers.clear();

        // Create main function
        let ir_func = IrFunction::new("main", vec![], TypeId::new(0));
        self.current_function = Some(ir_func);

        // Create entry block
        let entry_block = self.alloc_block();
        self.current_block = entry_block;
        self.current_function_mut()
            .add_block(BasicBlock::with_label(entry_block, "entry"));

        // Lower statements
        for stmt in stmts {
            self.lower_stmt(stmt);
        }

        // Ensure the function ends with a return
        if !self.current_block_is_terminated() {
            self.set_terminator(Terminator::Return(None));
        }

        self.current_function.take().unwrap()
    }

    /// Lower a class declaration
    fn lower_class(&mut self, class: &ast::ClassDecl) -> IrClass {
        let name = self.interner.resolve(class.name.name);
        let mut ir_class = IrClass::new(name);

        // Lower fields
        for (idx, member) in class.members.iter().enumerate() {
            if let ast::ClassMember::Field(field) = member {
                let field_name = self.interner.resolve(field.name.name);
                let ty = field
                    .type_annotation
                    .as_ref()
                    .map(|t| self.resolve_type_annotation(t))
                    .unwrap_or(TypeId::new(0));
                ir_class.add_field(IrField::new(field_name, ty, idx as u16));
            }
        }

        // Lower methods (they become separate functions)
        // Note: In a full implementation, methods would be added to the module
        // and their IDs stored in the class. For now, we just create the IR
        // but don't add them to anything.
        for member in &class.members {
            if let ast::ClassMember::Method(method) = member {
                // Only lower methods that have a body (not abstract methods)
                if let Some(body) = &method.body {
                    let method_name = self.interner.resolve(method.name.name);
                    let _full_name = format!("{}::{}", name, method_name);

                    // Reset per-function state
                    self.next_register = 0;
                    self.next_block = 0;
                    self.local_map.clear();
                    self.local_registers.clear();

                    // Create parameter registers
                    let mut params = Vec::new();
                    for param in &method.params {
                        let ty = param
                            .type_annotation
                            .as_ref()
                            .map(|t| self.resolve_type_annotation(t))
                            .unwrap_or(TypeId::new(0));
                        let reg = self.alloc_register(ty);

                        if let Pattern::Identifier(ident) = &param.pattern {
                            let local_idx = self.allocate_local(ident.name);
                            self.local_registers.insert(local_idx, reg.clone());
                        }
                        params.push(reg);
                    }

                    // Get return type
                    let return_ty = method
                        .return_type
                        .as_ref()
                        .map(|t| self.resolve_type_annotation(t))
                        .unwrap_or_else(|| TypeId::new(0));

                    // Create function
                    let ir_func = IrFunction::new(method_name, params, return_ty);
                    self.current_function = Some(ir_func);

                    // Create entry block
                    let entry_block = self.alloc_block();
                    self.current_block = entry_block;
                    self.current_function_mut()
                        .add_block(BasicBlock::with_label(entry_block, "entry"));

                    // Lower method body
                    for stmt in &body.statements {
                        self.lower_stmt(stmt);
                    }

                    // Ensure the function ends with a return
                    if !self.current_block_is_terminated() {
                        self.set_terminator(Terminator::Return(None));
                    }

                    // Take the function (in a full implementation, we'd add it to the module)
                    let _ir_func = self.current_function.take().unwrap();
                }
            }
        }

        ir_class
    }

    /// Allocate a new register
    fn alloc_register(&mut self, ty: TypeId) -> Register {
        let id = RegisterId::new(self.next_register);
        self.next_register += 1;
        Register::new(id, ty)
    }

    /// Allocate a new basic block ID
    fn alloc_block(&mut self) -> BasicBlockId {
        let id = BasicBlockId::new(self.next_block);
        self.next_block += 1;
        id
    }

    /// Allocate a local variable slot
    fn allocate_local(&mut self, name: Symbol) -> u16 {
        let idx = self.local_map.len() as u16;
        self.local_map.insert(name, idx);
        idx
    }

    /// Get the current function mutably
    fn current_function_mut(&mut self) -> &mut IrFunction {
        self.current_function.as_mut().expect("No current function")
    }

    /// Get the current block mutably
    fn current_block_mut(&mut self) -> &mut BasicBlock {
        let block_id = self.current_block;
        self.current_function_mut()
            .get_block_mut(block_id)
            .expect("Current block not found")
    }

    /// Add an instruction to the current block
    fn emit(&mut self, instr: IrInstr) {
        self.current_block_mut().add_instr(instr);
    }

    /// Set the terminator for the current block
    fn set_terminator(&mut self, term: Terminator) {
        self.current_block_mut().set_terminator(term);
    }

    /// Check if the current block is terminated
    fn current_block_is_terminated(&self) -> bool {
        let func = self.current_function.as_ref().expect("No current function");
        func.get_block(self.current_block)
            .map(|b| b.is_terminated())
            .unwrap_or(false)
    }

    /// Resolve a type annotation to a TypeId
    fn resolve_type_annotation(&self, _ty: &ast::TypeAnnotation) -> TypeId {
        // For now, return a placeholder TypeId
        // In a real implementation, this would look up the type in the type context
        TypeId::new(0)
    }

}
