//! AST to IR Lowering
//!
//! Converts the type-checked AST into the IR representation.

mod control_flow;
mod expr;
mod stmt;

use crate::compiler::ir::{
    BasicBlock, BasicBlockId, ClassId, FunctionId, IrClass, IrConstant, IrField, IrFunction,
    IrInstr, IrModule, IrValue, Register, RegisterId, Terminator,
};
use crate::parser::ast::{self, Expression, Pattern, Statement, VariableKind};
use crate::parser::{Interner, Symbol, TypeContext, TypeId};
use rustc_hash::{FxHashMap, FxHashSet};

/// Compile-time constant value (for constant folding)
/// Only literal values that can be evaluated at compile time
#[derive(Debug, Clone)]
pub enum ConstantValue {
    /// Integer constant
    I64(i64),
    /// Float constant
    F64(f64),
    /// String constant
    String(String),
    /// Boolean constant
    Bool(bool),
}

/// Information about a class field
#[derive(Clone)]
struct ClassFieldInfo {
    /// Field name (symbol)
    name: Symbol,
    /// Field index
    index: u16,
    /// Field type
    ty: TypeId,
    /// Default initializer expression (if any)
    initializer: Option<Expression>,
}

/// Information about a class method
#[derive(Clone)]
struct ClassMethodInfo {
    /// Method name (symbol)
    name: Symbol,
    /// Function ID for this method
    func_id: FunctionId,
}

/// Information about a constructor parameter (for default value handling)
#[derive(Clone)]
struct ConstructorParamInfo {
    /// Default value expression (if any)
    default_value: Option<Expression>,
}

/// Information about a static field
#[derive(Clone)]
struct StaticFieldInfo {
    /// Field name (symbol)
    name: Symbol,
    /// Global variable index for this static field
    global_index: u16,
    /// Initial value expression
    initializer: Option<Expression>,
}

/// Information about a static method
#[derive(Clone)]
struct StaticMethodInfo {
    /// Method name (symbol)
    name: Symbol,
    /// Function ID for this static method
    func_id: FunctionId,
}

/// Information about a class gathered during the first pass
#[derive(Clone)]
struct ClassInfo {
    /// Instance fields with their initializers
    fields: Vec<ClassFieldInfo>,
    /// Instance methods
    methods: Vec<ClassMethodInfo>,
    /// Constructor function ID (if class has a constructor)
    constructor: Option<FunctionId>,
    /// Constructor parameters (for default value handling)
    constructor_params: Vec<ConstructorParamInfo>,
    /// Static fields
    static_fields: Vec<StaticFieldInfo>,
    /// Static methods
    static_methods: Vec<StaticMethodInfo>,
    /// Parent class (for inheritance)
    parent_class: Option<ClassId>,
}

/// Loop context for break/continue handling
struct LoopContext {
    /// Block to jump to for break
    break_target: BasicBlockId,
    /// Block to jump to for continue
    continue_target: BasicBlockId,
}

/// Source of an ancestor variable (for closure capture tracking)
#[derive(Clone, Copy, Debug)]
enum AncestorSource {
    /// Variable is in the immediate parent's locals (can LoadLocal at MakeClosure time)
    ImmediateParentLocal(u16),
    /// Variable is from a further ancestor (parent must also capture it)
    Ancestor,
}

/// Information about `this` from an ancestor scope (for arrow functions in methods)
#[derive(Clone, Copy, Debug)]
struct AncestorThisInfo {
    /// Where to load `this` from
    source: AncestorSource,
}

/// Information about a variable from an ancestor scope
#[derive(Clone, Debug)]
struct AncestorVar {
    source: AncestorSource,
    ty: crate::parser::TypeId,
    /// Whether this variable is stored in a RefCell (for mutable capture-by-reference)
    is_refcell: bool,
}

/// Captured variable information
#[derive(Clone)]
struct CaptureInfo {
    /// Symbol of the captured variable
    symbol: Symbol,
    /// Where this capture comes from
    source: AncestorSource,
    /// Capture index (position in the captures array)
    capture_idx: u16,
    /// Type of the captured variable
    ty: crate::parser::TypeId,
    /// Whether this capture is a RefCell (for mutable capture-by-reference)
    is_refcell: bool,
}

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
    /// Next local index (for both named and anonymous locals)
    next_local: u16,
    /// Function name to ID mapping
    function_map: FxHashMap<Symbol, FunctionId>,
    /// Set of async function IDs (functions that should be spawned as Tasks)
    async_functions: FxHashSet<FunctionId>,
    /// Class name to ID mapping
    class_map: FxHashMap<Symbol, ClassId>,
    /// Class info (fields, initializers) for lowering `new` expressions
    class_info_map: FxHashMap<ClassId, ClassInfo>,
    /// Next function ID
    next_function_id: u32,
    /// Next class ID
    next_class_id: u32,
    /// Stack of loop contexts for break/continue
    loop_stack: Vec<LoopContext>,
    /// Pending arrow functions to be added to module (with their assigned func_id)
    pending_arrow_functions: Vec<(u32, IrFunction)>,
    /// Counter for generating unique arrow function names
    arrow_counter: u32,
    /// All variables from ancestor scopes (when inside an arrow function)
    /// Maps symbol to its source (immediate parent local or ancestor)
    ancestor_variables: Option<FxHashMap<Symbol, AncestorVar>>,
    /// Captured variables for the current arrow function
    captures: Vec<CaptureInfo>,
    /// Info about the last created closure (for self-recursive closure detection)
    /// Contains (closure_register, Vec<(symbol, capture_index)>)
    last_closure_info: Option<(Register, Vec<(Symbol, u16)>)>,
    /// Variables that need RefCell wrapping (captured and potentially modified)
    refcell_vars: FxHashSet<Symbol>,
    /// Map from local variable to its RefCell register (for variables stored in RefCells)
    refcell_registers: FxHashMap<u16, Register>,
    /// Variables that are captured by any closure (read or write) - used for per-iteration bindings in loops
    loop_captured_vars: FxHashSet<Symbol>,
    /// Map from variable name to its class type (for field access resolution)
    variable_class_map: FxHashMap<Symbol, ClassId>,
    /// Current class being processed (for method lowering)
    current_class: Option<ClassId>,
    /// Register holding `this` in current method
    this_register: Option<Register>,
    /// Info about `this` from ancestor scope (for arrow functions inside methods)
    this_ancestor_info: Option<AncestorThisInfo>,
    /// Capture index of `this` if it was captured (for LoadCaptured)
    this_captured_idx: Option<u16>,
    /// Method name to function ID mapping (for method calls)
    method_map: FxHashMap<(ClassId, Symbol), FunctionId>,
    /// Static method name to function ID mapping
    static_method_map: FxHashMap<(ClassId, Symbol), FunctionId>,
    /// Next global variable index (for static fields)
    next_global_index: u16,
    /// Set of function IDs that are async closures (should be spawned as Tasks)
    async_closures: FxHashSet<FunctionId>,
    /// Map from local variable index to function ID for closures stored in variables
    /// Used to track async closures for SpawnClosure emission
    closure_locals: FxHashMap<u16, FunctionId>,
    /// Expression types from type checker (maps expr ptr to TypeId)
    expr_types: FxHashMap<usize, TypeId>,
    /// Compile-time constant values (for constant folding)
    /// Maps symbol to its constant value (only for literals)
    constant_map: FxHashMap<Symbol, ConstantValue>,
}

impl<'a> Lowerer<'a> {
    /// Create a new lowerer
    pub fn new(type_ctx: &'a TypeContext, interner: &'a Interner) -> Self {
        Self::with_expr_types(type_ctx, interner, FxHashMap::default())
    }

    /// Create a new lowerer with expression types from the type checker
    pub fn with_expr_types(
        type_ctx: &'a TypeContext,
        interner: &'a Interner,
        expr_types: FxHashMap<usize, TypeId>,
    ) -> Self {
        Self {
            type_ctx,
            interner,
            current_function: None,
            current_block: BasicBlockId(0),
            next_register: 0,
            next_block: 0,
            local_map: FxHashMap::default(),
            local_registers: FxHashMap::default(),
            next_local: 0,
            function_map: FxHashMap::default(),
            async_functions: FxHashSet::default(),
            class_map: FxHashMap::default(),
            class_info_map: FxHashMap::default(),
            next_function_id: 0,
            next_class_id: 0,
            loop_stack: Vec::new(),
            pending_arrow_functions: Vec::new(),
            arrow_counter: 0,
            ancestor_variables: None,
            captures: Vec::new(),
            last_closure_info: None,
            refcell_vars: FxHashSet::default(),
            refcell_registers: FxHashMap::default(),
            loop_captured_vars: FxHashSet::default(),
            variable_class_map: FxHashMap::default(),
            current_class: None,
            this_register: None,
            this_ancestor_info: None,
            this_captured_idx: None,
            method_map: FxHashMap::default(),
            static_method_map: FxHashMap::default(),
            next_global_index: 0,
            async_closures: FxHashSet::default(),
            closure_locals: FxHashMap::default(),
            expr_types,
            constant_map: FxHashMap::default(),
        }
    }

    /// Try to evaluate an expression as a compile-time constant
    /// Returns Some(ConstantValue) if the expression is a literal, None otherwise
    fn try_eval_constant(&self, expr: &Expression) -> Option<ConstantValue> {
        match expr {
            Expression::IntLiteral(lit) => Some(ConstantValue::I64(lit.value)),
            Expression::FloatLiteral(lit) => Some(ConstantValue::F64(lit.value)),
            Expression::StringLiteral(lit) => {
                let s = self.interner.resolve(lit.value);
                Some(ConstantValue::String(s.to_string()))
            }
            Expression::BooleanLiteral(lit) => Some(ConstantValue::Bool(lit.value)),
            // For identifiers, check if they reference another constant
            Expression::Identifier(ident) => {
                self.constant_map.get(&ident.name).cloned()
            }
            // Could extend to support simple constant expressions like 0x0300
            // but for now only support direct literals
            _ => None,
        }
    }

    /// Look up a compile-time constant by symbol
    pub fn lookup_constant(&self, name: Symbol) -> Option<&ConstantValue> {
        self.constant_map.get(&name)
    }

    /// Get the TypeId for an expression from the type checker's expr_types map
    /// Falls back to TypeId(0) (Number) if not found
    fn get_expr_type(&self, expr: &Expression) -> TypeId {
        let expr_id = expr as *const _ as usize;
        self.expr_types.get(&expr_id).copied().unwrap_or(TypeId::new(0))
    }

    /// Lower an AST module to IR
    pub fn lower_module(&mut self, module: &ast::Module) -> IrModule {
        let mut ir_module = IrModule::new("main");

        // Pre-pass: collect module-level const declarations (for constant folding)
        // These need to be processed before classes/functions so they're available
        for stmt in &module.statements {
            if let Statement::VariableDecl(decl) = stmt {
                if decl.kind == VariableKind::Const {
                    if let Pattern::Identifier(ident) = &decl.pattern {
                        if let Some(init) = &decl.initializer {
                            if let Some(const_val) = self.try_eval_constant(init) {
                                self.constant_map.insert(ident.name, const_val);
                            }
                        }
                    }
                }
            }
        }

        // First pass: collect function and class declarations
        for stmt in &module.statements {
            match stmt {
                Statement::FunctionDecl(func) => {
                    let id = FunctionId::new(self.next_function_id);
                    self.next_function_id += 1;
                    self.function_map.insert(func.name.name, id);
                    // Track async functions for Spawn emission
                    if func.is_async {
                        self.async_functions.insert(id);
                    }
                }
                Statement::ClassDecl(class) => {
                    let class_id = ClassId::new(self.next_class_id);
                    self.next_class_id += 1;
                    self.class_map.insert(class.name.name, class_id);

                    // Resolve parent class if extends clause is present
                    let parent_class = if let Some(ref extends) = class.extends {
                        if let ast::Type::Reference(type_ref) = &extends.ty {
                            self.class_map.get(&type_ref.name.name).copied()
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    // Collect instance and static field information
                    let mut fields = Vec::new();
                    let mut static_fields = Vec::new();

                    // Start field index after parent's fields (if any)
                    let mut field_index = if let Some(parent_id) = parent_class {
                        // Get parent's field count to offset child field indices
                        self.class_info_map
                            .get(&parent_id)
                            .map(|info| info.fields.len() as u16)
                            .unwrap_or(0)
                    } else {
                        0u16
                    };

                    for member in &class.members {
                        if let ast::ClassMember::Field(field) = member {
                            let ty = field
                                .type_annotation
                                .as_ref()
                                .map(|t| self.resolve_type_annotation(t))
                                .unwrap_or(TypeId::new(0));

                            if field.is_static {
                                // Static field: allocate a global index
                                let global_index = self.next_global_index;
                                self.next_global_index += 1;
                                static_fields.push(StaticFieldInfo {
                                    name: field.name.name,
                                    global_index,
                                    initializer: field.initializer.clone(),
                                });
                            } else {
                                // Instance field
                                fields.push(ClassFieldInfo {
                                    name: field.name.name,
                                    index: field_index,
                                    ty,
                                    initializer: field.initializer.clone(),
                                });
                                field_index += 1;
                            }
                        }
                    }

                    // Collect instance and static method information
                    let mut methods = Vec::new();
                    let mut static_methods = Vec::new();
                    for member in &class.members {
                        if let ast::ClassMember::Method(method) = member {
                            if method.body.is_some() {
                                let func_id = FunctionId::new(self.next_function_id);
                                self.next_function_id += 1;

                                // Track async methods
                                if method.is_async {
                                    self.async_functions.insert(func_id);
                                }

                                if method.is_static {
                                    // Static method
                                    static_methods.push(StaticMethodInfo {
                                        name: method.name.name,
                                        func_id,
                                    });
                                    self.static_method_map
                                        .insert((class_id, method.name.name), func_id);
                                } else {
                                    // Instance method
                                    methods.push(ClassMethodInfo {
                                        name: method.name.name,
                                        func_id,
                                    });
                                    self.method_map.insert((class_id, method.name.name), func_id);
                                }
                            }
                        }
                    }

                    // Check for constructor and assign function ID, collect parameter defaults
                    let mut constructor = None;
                    let mut constructor_params = Vec::new();
                    for member in &class.members {
                        if let ast::ClassMember::Constructor(ctor) = member {
                            let func_id = FunctionId::new(self.next_function_id);
                            self.next_function_id += 1;
                            constructor = Some(func_id);

                            // Collect constructor parameter defaults
                            for param in &ctor.params {
                                constructor_params.push(ConstructorParamInfo {
                                    default_value: param.default_value.clone(),
                                });
                            }
                            break; // Only one constructor allowed
                        }
                    }

                    self.class_info_map.insert(
                        class_id,
                        ClassInfo {
                            fields,
                            methods,
                            constructor,
                            constructor_params,
                            static_fields,
                            static_methods,
                            parent_class,
                        },
                    );
                }
                _ => {}
            }
        }

        // Second pass: lower all declarations
        // IMPORTANT: All functions must be added to pending_arrow_functions with their pre-assigned IDs
        // so they can be sorted and added to the module in the correct order.
        // This ensures function indices match the pre-assigned IDs used in Call instructions.
        for stmt in &module.statements {
            match stmt {
                Statement::FunctionDecl(func) => {
                    // Get the pre-assigned function ID
                    let func_id = self.function_map.get(&func.name.name).copied().unwrap();
                    let ir_func = self.lower_function(func);
                    // Add to pending with pre-assigned ID (will be sorted later)
                    self.pending_arrow_functions.push((func_id.as_u32(), ir_func));
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

        // Collect top-level statements for main function
        let top_level_stmts: Vec<_> = module
            .statements
            .iter()
            .filter(|s| !matches!(s, Statement::FunctionDecl(_) | Statement::ClassDecl(_)))
            .collect();

        // Reserve main function's ID BEFORE lowering, so arrow functions
        // created during lowering get IDs after main
        let main_func_id = if !top_level_stmts.is_empty() {
            let id = self.next_function_id;
            self.next_function_id += 1;
            Some(id)
        } else {
            None
        };

        // Now lower top-level statements (arrow functions will get IDs starting after main)
        if let Some(main_id) = main_func_id {
            let main_func = self.lower_top_level_statements(&top_level_stmts);
            // Add main to pending_arrow_functions with its ID, so it gets sorted correctly
            self.pending_arrow_functions.push((main_id, main_func));
        }

        // Add ALL pending functions (including main and class methods) sorted by func_id
        // This ensures functions are added to the module in the order of their pre-assigned IDs
        self.pending_arrow_functions.sort_by_key(|(id, _)| *id);
        for (_, func) in self.pending_arrow_functions.drain(..) {
            ir_module.add_function(func);
        }

        ir_module
    }

    /// Pre-scan statements to identify variables that will be captured by closures
    /// These variables need RefCell wrapping for capture-by-reference semantics
    fn scan_for_captured_vars(&mut self, stmts: &[ast::Statement], locals: &FxHashSet<Symbol>) {
        for stmt in stmts {
            self.scan_stmt_for_captures(stmt, locals);
        }
    }

    /// Scan a single statement for arrow functions that capture outer variables
    fn scan_stmt_for_captures(&mut self, stmt: &ast::Statement, locals: &FxHashSet<Symbol>) {
        use crate::parser::ast::*;

        match stmt {
            Statement::VariableDecl(var) => {
                // Check the initializer for arrow functions
                if let Some(init) = &var.initializer {
                    self.scan_expr_for_captures(init, locals);
                }
            }
            Statement::Expression(expr) => {
                self.scan_expr_for_captures(&expr.expression, locals);
            }
            Statement::If(if_stmt) => {
                self.scan_expr_for_captures(&if_stmt.condition, locals);
                self.scan_stmt_for_captures(&if_stmt.then_branch, locals);
                if let Some(else_branch) = &if_stmt.else_branch {
                    self.scan_stmt_for_captures(else_branch, locals);
                }
            }
            Statement::While(while_stmt) => {
                self.scan_expr_for_captures(&while_stmt.condition, locals);
                self.scan_stmt_for_captures(&while_stmt.body, locals);
            }
            Statement::For(for_stmt) => {
                if let Some(init) = &for_stmt.init {
                    if let ast::ForInit::Expression(e) = init {
                        self.scan_expr_for_captures(e, locals);
                    }
                }
                if let Some(cond) = &for_stmt.test {
                    self.scan_expr_for_captures(cond, locals);
                }
                if let Some(update) = &for_stmt.update {
                    self.scan_expr_for_captures(update, locals);
                }
                self.scan_stmt_for_captures(&for_stmt.body, locals);
            }
            Statement::Return(ret) => {
                if let Some(e) = &ret.value {
                    self.scan_expr_for_captures(e, locals);
                }
            }
            Statement::Block(block) => {
                for s in &block.statements {
                    self.scan_stmt_for_captures(s, locals);
                }
            }
            _ => {}
        }
    }

    /// Scan an expression for arrow functions that capture outer variables
    fn scan_expr_for_captures(&mut self, expr: &ast::Expression, locals: &FxHashSet<Symbol>) {
        use crate::parser::ast::*;

        match expr {
            Expression::Arrow(arrow) => {
                // Found an arrow function - scan its body for outer variable references
                self.scan_arrow_for_captures(arrow, locals);
            }
            Expression::Binary(binary) => {
                self.scan_expr_for_captures(&binary.left, locals);
                self.scan_expr_for_captures(&binary.right, locals);
            }
            Expression::Unary(unary) => {
                self.scan_expr_for_captures(&unary.operand, locals);
            }
            Expression::Assignment(assign) => {
                self.scan_expr_for_captures(&assign.left, locals);
                self.scan_expr_for_captures(&assign.right, locals);
            }
            Expression::Call(call) => {
                self.scan_expr_for_captures(&call.callee, locals);
                for arg in &call.arguments {
                    self.scan_expr_for_captures(arg, locals);
                }
            }
            Expression::Member(member) => {
                self.scan_expr_for_captures(&member.object, locals);
            }
            Expression::Parenthesized(paren) => {
                self.scan_expr_for_captures(&paren.expression, locals);
            }
            Expression::Conditional(cond) => {
                self.scan_expr_for_captures(&cond.test, locals);
                self.scan_expr_for_captures(&cond.consequent, locals);
                self.scan_expr_for_captures(&cond.alternate, locals);
            }
            _ => {}
        }
    }

    /// Scan an arrow function body for references to outer variables
    fn scan_arrow_for_captures(&mut self, arrow: &ast::ArrowFunction, outer_locals: &FxHashSet<Symbol>) {
        use crate::parser::ast::*;

        // Collect parameter names (these are local to the arrow, not captures)
        let mut arrow_locals: FxHashSet<Symbol> = FxHashSet::default();
        for param in &arrow.params {
            if let Pattern::Identifier(ident) = &param.pattern {
                arrow_locals.insert(ident.name);
            }
        }

        // Scan the body for references to outer_locals
        match &arrow.body {
            ArrowBody::Expression(expr) => {
                self.find_captured_refs(expr, outer_locals, &arrow_locals);
            }
            ArrowBody::Block(block) => {
                // Also scan for local declarations in the block
                let mut block_locals = arrow_locals.clone();
                for stmt in &block.statements {
                    if let Statement::VariableDecl(var) = stmt {
                        if let Pattern::Identifier(ident) = &var.pattern {
                            block_locals.insert(ident.name);
                        }
                    }
                }

                for stmt in &block.statements {
                    self.find_captured_refs_in_stmt(stmt, outer_locals, &block_locals);
                }
            }
        }
    }

    /// Find captured variable references in an expression that are MODIFIED (assigned to)
    /// Only these variables need RefCell wrapping - read-only captures use copy semantics
    /// Also tracks ALL captured variables (read or write) in loop_captured_vars for per-iteration bindings
    fn find_captured_refs(&mut self, expr: &ast::Expression, outer_locals: &FxHashSet<Symbol>, arrow_locals: &FxHashSet<Symbol>) {
        use crate::parser::ast::*;

        match expr {
            Expression::Identifier(ident) => {
                // Track that this outer variable is captured (even read-only)
                // Used for per-iteration bindings in loops
                if outer_locals.contains(&ident.name) && !arrow_locals.contains(&ident.name) {
                    self.loop_captured_vars.insert(ident.name);
                }
            }
            Expression::Binary(binary) => {
                self.find_captured_refs(&binary.left, outer_locals, arrow_locals);
                self.find_captured_refs(&binary.right, outer_locals, arrow_locals);
            }
            Expression::Unary(unary) => {
                self.find_captured_refs(&unary.operand, outer_locals, arrow_locals);
            }
            Expression::Assignment(assign) => {
                // Check if we're assigning to a captured outer variable
                if let Expression::Identifier(ident) = &*assign.left {
                    // If this identifier is from outer scope (not an arrow local), it needs RefCell
                    if outer_locals.contains(&ident.name) && !arrow_locals.contains(&ident.name) {
                        self.refcell_vars.insert(ident.name);
                    }
                }
                // Also check if nested expressions might have captures
                self.find_captured_refs(&assign.left, outer_locals, arrow_locals);
                self.find_captured_refs(&assign.right, outer_locals, arrow_locals);
            }
            Expression::Call(call) => {
                self.find_captured_refs(&call.callee, outer_locals, arrow_locals);
                for arg in &call.arguments {
                    self.find_captured_refs(arg, outer_locals, arrow_locals);
                }
            }
            Expression::Member(member) => {
                self.find_captured_refs(&member.object, outer_locals, arrow_locals);
            }
            Expression::Parenthesized(paren) => {
                self.find_captured_refs(&paren.expression, outer_locals, arrow_locals);
            }
            Expression::Conditional(cond) => {
                self.find_captured_refs(&cond.test, outer_locals, arrow_locals);
                self.find_captured_refs(&cond.consequent, outer_locals, arrow_locals);
                self.find_captured_refs(&cond.alternate, outer_locals, arrow_locals);
            }
            Expression::Arrow(nested_arrow) => {
                // Nested arrow - recurse with updated locals
                self.scan_arrow_for_captures(nested_arrow, outer_locals);
            }
            _ => {}
        }
    }

    /// Find captured variable references in a statement
    fn find_captured_refs_in_stmt(&mut self, stmt: &ast::Statement, outer_locals: &FxHashSet<Symbol>, arrow_locals: &FxHashSet<Symbol>) {
        use crate::parser::ast::*;

        match stmt {
            Statement::VariableDecl(var) => {
                if let Some(init) = &var.initializer {
                    self.find_captured_refs(init, outer_locals, arrow_locals);
                }
            }
            Statement::Expression(expr) => {
                self.find_captured_refs(&expr.expression, outer_locals, arrow_locals);
            }
            Statement::If(if_stmt) => {
                self.find_captured_refs(&if_stmt.condition, outer_locals, arrow_locals);
                self.find_captured_refs_in_stmt(&if_stmt.then_branch, outer_locals, arrow_locals);
                if let Some(else_branch) = &if_stmt.else_branch {
                    self.find_captured_refs_in_stmt(else_branch, outer_locals, arrow_locals);
                }
            }
            Statement::While(while_stmt) => {
                self.find_captured_refs(&while_stmt.condition, outer_locals, arrow_locals);
                self.find_captured_refs_in_stmt(&while_stmt.body, outer_locals, arrow_locals);
            }
            Statement::Return(ret) => {
                if let Some(e) = &ret.value {
                    self.find_captured_refs(e, outer_locals, arrow_locals);
                }
            }
            Statement::Block(block) => {
                for s in &block.statements {
                    self.find_captured_refs_in_stmt(s, outer_locals, arrow_locals);
                }
            }
            _ => {}
        }
    }

    /// Collect all local variable names declared in statements
    fn collect_local_names(&self, stmts: &[ast::Statement]) -> FxHashSet<Symbol> {
        let mut locals = FxHashSet::default();
        self.collect_local_names_recursive(stmts, &mut locals);
        locals
    }

    /// Recursively collect all local variable names from statements, including nested scopes
    fn collect_local_names_recursive(&self, stmts: &[ast::Statement], locals: &mut FxHashSet<Symbol>) {
        for stmt in stmts {
            match stmt {
                ast::Statement::VariableDecl(var) => {
                    if let Pattern::Identifier(ident) = &var.pattern {
                        locals.insert(ident.name);
                    }
                }
                ast::Statement::For(for_stmt) => {
                    // Collect variable from for-loop initializer
                    if let Some(ast::ForInit::VariableDecl(var)) = &for_stmt.init {
                        if let Pattern::Identifier(ident) = &var.pattern {
                            locals.insert(ident.name);
                        }
                    }
                    // Recurse into body
                    if let ast::Statement::Block(block) = &*for_stmt.body {
                        self.collect_local_names_recursive(&block.statements, locals);
                    } else {
                        self.collect_local_names_recursive(&[(*for_stmt.body).clone()], locals);
                    }
                }
                ast::Statement::ForOf(for_of) => {
                    // Collect variable from for-of loop
                    match &for_of.left {
                        ast::ForOfLeft::VariableDecl(var) => {
                            if let Pattern::Identifier(ident) = &var.pattern {
                                locals.insert(ident.name);
                            }
                        }
                        ast::ForOfLeft::Pattern(Pattern::Identifier(ident)) => {
                            locals.insert(ident.name);
                        }
                        _ => {}
                    }
                    // Recurse into body
                    if let ast::Statement::Block(block) = &*for_of.body {
                        self.collect_local_names_recursive(&block.statements, locals);
                    } else {
                        self.collect_local_names_recursive(&[(*for_of.body).clone()], locals);
                    }
                }
                ast::Statement::While(while_stmt) => {
                    if let ast::Statement::Block(block) = &*while_stmt.body {
                        self.collect_local_names_recursive(&block.statements, locals);
                    } else {
                        self.collect_local_names_recursive(&[(*while_stmt.body).clone()], locals);
                    }
                }
                ast::Statement::DoWhile(do_while) => {
                    if let ast::Statement::Block(block) = &*do_while.body {
                        self.collect_local_names_recursive(&block.statements, locals);
                    } else {
                        self.collect_local_names_recursive(&[(*do_while.body).clone()], locals);
                    }
                }
                ast::Statement::If(if_stmt) => {
                    if let ast::Statement::Block(block) = &*if_stmt.then_branch {
                        self.collect_local_names_recursive(&block.statements, locals);
                    } else {
                        self.collect_local_names_recursive(&[(*if_stmt.then_branch).clone()], locals);
                    }
                    if let Some(else_branch) = &if_stmt.else_branch {
                        if let ast::Statement::Block(block) = &**else_branch {
                            self.collect_local_names_recursive(&block.statements, locals);
                        } else {
                            self.collect_local_names_recursive(&[(**else_branch).clone()], locals);
                        }
                    }
                }
                ast::Statement::Block(block) => {
                    self.collect_local_names_recursive(&block.statements, locals);
                }
                _ => {}
            }
        }
    }

    /// Lower a function declaration
    fn lower_function(&mut self, func: &ast::FunctionDecl) -> IrFunction {
        // Reset per-function state
        self.next_register = 0;
        self.next_block = 0;
        self.local_map.clear();
        self.local_registers.clear();
        self.next_local = 0;
        self.refcell_vars.clear();
        self.refcell_registers.clear();
        self.loop_captured_vars.clear();

        // Pre-scan to identify captured variables
        let mut locals = FxHashSet::default();
        for param in &func.params {
            if let Pattern::Identifier(ident) = &param.pattern {
                locals.insert(ident.name);
            }
        }
        locals.extend(self.collect_local_names(&func.body.statements));
        self.scan_for_captured_vars(&func.body.statements, &locals);

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
        self.next_local = 0;
        self.refcell_vars.clear();
        self.refcell_registers.clear();
        self.loop_captured_vars.clear();

        // Pre-scan to identify captured variables
        let stmts_owned: Vec<ast::Statement> = stmts.iter().map(|s| (*s).clone()).collect();
        let locals = self.collect_local_names(&stmts_owned);
        self.scan_for_captured_vars(&stmts_owned, &locals);

        // Create main function
        let ir_func = IrFunction::new("main", vec![], TypeId::new(0));
        self.current_function = Some(ir_func);

        // Create entry block
        let entry_block = self.alloc_block();
        self.current_block = entry_block;
        self.current_function_mut()
            .add_block(BasicBlock::with_label(entry_block, "entry"));

        // Initialize static fields from all classes
        self.emit_static_field_initializations();

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

        // Get class ID and class info
        let class_id = *self.class_map.get(&class.name.name).unwrap();
        let class_info = self.class_info_map.get(&class_id).cloned();

        // Set parent class if this class extends another
        if let Some(ref info) = class_info {
            ir_class.parent = info.parent_class;
        }

        // Add parent fields first (with their original indices)
        if let Some(ref info) = class_info {
            if let Some(parent_id) = info.parent_class {
                // Recursively get all parent fields
                fn add_parent_fields(
                    lowerer: &Lowerer,
                    ir_class: &mut IrClass,
                    parent_id: ClassId,
                ) {
                    if let Some(parent_info) = lowerer.class_info_map.get(&parent_id) {
                        // First add grandparent fields
                        if let Some(grandparent_id) = parent_info.parent_class {
                            add_parent_fields(lowerer, ir_class, grandparent_id);
                        }
                        // Then add parent's own fields
                        for field in &parent_info.fields {
                            let field_name = lowerer.interner.resolve(field.name);
                            ir_class.add_field(IrField::new(field_name, field.ty, field.index));
                        }
                    }
                }
                add_parent_fields(self, &mut ir_class, parent_id);
            }
        }

        // Lower this class's own fields (indices were already adjusted in first pass)
        for member in &class.members {
            if let ast::ClassMember::Field(field) = member {
                if !field.is_static {
                    let field_name = self.interner.resolve(field.name.name);
                    let ty = field
                        .type_annotation
                        .as_ref()
                        .map(|t| self.resolve_type_annotation(t))
                        .unwrap_or(TypeId::new(0));
                    // Get the index from class_info since it was computed with parent offset
                    let index = class_info
                        .as_ref()
                        .and_then(|info| {
                            info.fields
                                .iter()
                                .find(|f| f.name == field.name.name)
                                .map(|f| f.index)
                        })
                        .unwrap_or(0);
                    ir_class.add_field(IrField::new(field_name, ty, index));
                }
            }
        }

        // Lower methods (instance methods have 'this' as first parameter, static methods don't)
        for member in &class.members {
            if let ast::ClassMember::Method(method) = member {
                // Only lower methods that have a body (not abstract methods)
                if let Some(body) = &method.body {
                    let method_name = self.interner.resolve(method.name.name);
                    let full_name = if method.is_static {
                        format!("{}::static::{}", name, method_name)
                    } else {
                        format!("{}::{}", name, method_name)
                    };

                    // Reset per-function state
                    self.next_register = 0;
                    self.next_block = 0;
                    self.next_local = 0;
                    self.local_map.clear();
                    self.local_registers.clear();

                    // Create parameter registers
                    let mut params = Vec::new();

                    if method.is_static {
                        // Static method - no 'this' parameter
                        self.current_class = None;
                        self.this_register = None;
                    } else {
                        // Instance method - 'this' is the first parameter
                        self.current_class = Some(class_id);
                        let this_reg = self.alloc_register(TypeId::new(0)); // Object type
                        params.push(this_reg.clone());
                        self.this_register = Some(this_reg);
                        self.next_local = 1; // Explicit parameters start at slot 1
                    }

                    // Add explicit parameters
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

                    // Create function with mangled name
                    let ir_func = IrFunction::new(&full_name, params, return_ty);
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

                    // Get the function ID and add to pending methods
                    let func_id = if method.is_static {
                        self.static_method_map.get(&(class_id, method.name.name)).unwrap()
                    } else {
                        self.method_map.get(&(class_id, method.name.name)).unwrap()
                    };
                    let ir_func = self.current_function.take().unwrap();
                    self.pending_arrow_functions.push((func_id.as_u32(), ir_func));

                    // Clear method context
                    self.current_class = None;
                    self.this_register = None;
                }
            }
        }

        // Lower constructor if present
        for member in &class.members {
            if let ast::ClassMember::Constructor(ctor) = member {
                let full_name = format!("{}::constructor", name);

                // Reset per-function state
                self.next_register = 0;
                self.next_block = 0;
                self.next_local = 0;
                self.local_map.clear();
                self.local_registers.clear();

                // Set current class context for 'this' handling
                self.current_class = Some(class_id);

                // Create parameter registers - 'this' is the first parameter
                let mut params = Vec::new();

                // Add 'this' as the first parameter (object type)
                // Reserve local slot 0 for 'this'
                let this_reg = self.alloc_register(TypeId::new(0)); // Object type
                params.push(this_reg.clone());
                self.this_register = Some(this_reg);
                self.next_local = 1; // Explicit parameters start at slot 1

                // Add explicit parameters from constructor
                for param in &ctor.params {
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

                // Constructors implicitly return void
                let return_ty = TypeId::new(0);

                // Create function with mangled name
                let ir_func = IrFunction::new(&full_name, params, return_ty);
                self.current_function = Some(ir_func);

                // Create entry block
                let entry_block = self.alloc_block();
                self.current_block = entry_block;
                self.current_function_mut()
                    .add_block(BasicBlock::with_label(entry_block, "entry"));

                // Lower constructor body
                for stmt in &ctor.body.statements {
                    self.lower_stmt(stmt);
                }

                // Ensure the function ends with a return
                if !self.current_block_is_terminated() {
                    self.set_terminator(Terminator::Return(None));
                }

                // Get the constructor function ID from class_info and add to pending functions
                if let Some(class_info) = self.class_info_map.get(&class_id) {
                    if let Some(ctor_func_id) = class_info.constructor {
                        let ir_func = self.current_function.take().unwrap();
                        self.pending_arrow_functions
                            .push((ctor_func_id.as_u32(), ir_func));
                    }
                }

                // Clear method context
                self.current_class = None;
                self.this_register = None;
                break; // Only one constructor
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
        let idx = self.next_local;
        self.next_local += 1;
        self.local_map.insert(name, idx);
        idx
    }

    /// Allocate an anonymous local variable slot (for internal use like loop indices)
    fn allocate_anonymous_local(&mut self) -> u16 {
        let idx = self.next_local;
        self.next_local += 1;
        idx
    }

    /// Look up a local variable by name
    fn lookup_local(&self, name: Symbol) -> Option<u16> {
        self.local_map.get(&name).copied()
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

    /// Emit initialization code for all static fields
    fn emit_static_field_initializations(&mut self) {
        // Collect static fields with their initializers (clone to avoid borrow issues)
        let static_fields: Vec<(u16, Expression)> = self
            .class_info_map
            .values()
            .flat_map(|class_info| {
                class_info
                    .static_fields
                    .iter()
                    .filter_map(|sf| {
                        sf.initializer
                            .as_ref()
                            .map(|init| (sf.global_index, init.clone()))
                    })
            })
            .collect();

        // Emit initialization for each static field
        for (global_index, initializer) in static_fields {
            let value_reg = self.lower_expr(&initializer);
            self.emit(IrInstr::StoreGlobal {
                index: global_index,
                value: value_reg,
            });
        }
    }

    /// Resolve a type annotation to a TypeId
    fn resolve_type_annotation(&self, ty: &ast::TypeAnnotation) -> TypeId {
        // Pre-interned TypeIds: 0=Number, 1=String, 2=Boolean, 3=Null, 4=Void, 5=Never, 6=Unknown
        match &ty.ty {
            ast::Type::Primitive(prim) => {
                // PrimitiveType is an enum, match on it directly
                match prim {
                    ast::PrimitiveType::Number => TypeId::new(0),
                    ast::PrimitiveType::String => TypeId::new(1),
                    ast::PrimitiveType::Boolean => TypeId::new(2),
                    ast::PrimitiveType::Null => TypeId::new(3),
                    ast::PrimitiveType::Void => TypeId::new(4),
                }
            }
            ast::Type::Reference(type_ref) => {
                let name = self.interner.resolve(type_ref.name.name);
                match name {
                    "number" => TypeId::new(0),
                    "string" => TypeId::new(1),
                    "boolean" => TypeId::new(2),
                    "null" => TypeId::new(3),
                    "void" => TypeId::new(4),
                    "never" => TypeId::new(5),
                    "unknown" => TypeId::new(6),
                    // For class types, we return a high TypeId (could be improved)
                    _ => TypeId::new(7),
                }
            }
            // For other type forms (function, union, etc.), default to 0
            _ => TypeId::new(0),
        }
    }

}
