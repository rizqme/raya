//! AST to IR Lowering
//!
//! Converts the type-checked AST into the IR representation.

mod class_methods;
mod control_flow;
mod expr;
mod stmt;

use crate::compiler::ir::{
    BasicBlock, BasicBlockId, ClassId, FunctionId, IrClass, IrConstant, IrField, IrFunction,
    IrInstr, IrModule, IrTypeAlias, IrTypeAliasField, IrValue, Register, RegisterId, Terminator,
    TypeAliasId,
};
use crate::parser::ast::{self, ExportDecl, Expression, Pattern, Statement, VariableKind, Visitor, walk_arrow_function, walk_block_statement, walk_expression};
use crate::parser::token::Span;
use crate::parser::{Interner, Symbol, TypeContext, TypeId};
use rustc_hash::{FxHashMap, FxHashSet};

/// Sentinel TypeId for when the lowerer cannot determine the type.
/// Distinct from TypeId(0) (Number) and TypeId(6) (Unknown).
/// Re-exported from type_registry for convenience.
pub(super) const UNRESOLVED_TYPE_ID: u32 = super::type_registry::UNRESOLVED_TYPE_ID;
pub(super) const UNRESOLVED: TypeId = TypeId::new(UNRESOLVED_TYPE_ID);

// Well-known TypeId constants re-exported from TypeContext for use in lowering submodules.
pub(super) const NUMBER_TYPE_ID: u32 = TypeContext::NUMBER_TYPE_ID;
pub(super) const STRING_TYPE_ID: u32 = TypeContext::STRING_TYPE_ID;
pub(super) const BOOLEAN_TYPE_ID: u32 = TypeContext::BOOLEAN_TYPE_ID;
pub(super) const NULL_TYPE_ID: u32 = TypeContext::NULL_TYPE_ID;
pub(super) const UNKNOWN_TYPE_ID: u32 = TypeContext::UNKNOWN_TYPE_ID;
pub(super) const REGEXP_TYPE_ID: u32 = TypeContext::REGEXP_TYPE_ID;
pub(super) const TASK_TYPE_ID: u32 = TypeContext::TASK_TYPE_ID;
pub(super) const CHANNEL_TYPE_ID: u32 = TypeContext::CHANNEL_TYPE_ID;
pub(super) const MAP_TYPE_ID: u32 = TypeContext::MAP_TYPE_ID;
pub(super) const SET_TYPE_ID: u32 = TypeContext::SET_TYPE_ID;
pub(super) const JSON_TYPE_ID: u32 = TypeContext::JSON_TYPE_ID;
pub(super) const INT_TYPE_ID: u32 = TypeContext::INT_TYPE_ID;
pub(super) const ARRAY_TYPE_ID: u32 = TypeContext::ARRAY_TYPE_ID;

/// Collects identifiers referenced in a function body that match module-level variable names.
/// Used to determine which module-level variables need global promotion (LoadGlobal/StoreGlobal).
struct ModuleVarRefCollector<'a> {
    candidates: &'a FxHashSet<Symbol>,
    referenced: &'a mut FxHashSet<Symbol>,
}

impl<'a> Visitor for ModuleVarRefCollector<'a> {
    fn visit_identifier(&mut self, id: &ast::Identifier) {
        if self.candidates.contains(&id.name) {
            self.referenced.insert(id.name);
        }
    }
}

/// Walks expressions but only collects variable references found INSIDE arrow function bodies.
/// This avoids promoting variables that are only referenced in top-level sequential code
/// (where locals suffice), while still promoting variables accessed by closures.
struct ArrowBodyVarRefCollector<'a> {
    candidates: &'a FxHashSet<Symbol>,
    referenced: &'a mut FxHashSet<Symbol>,
}

impl<'a> Visitor for ArrowBodyVarRefCollector<'a> {
    fn visit_identifier(&mut self, _id: &ast::Identifier) {
        // Ignore identifiers at the top level — only collect inside arrow functions
    }

    fn visit_arrow_function(&mut self, func: &ast::ArrowFunction) {
        // Inside an arrow function body, collect all referenced module-level variables
        let mut collector = ModuleVarRefCollector {
            candidates: self.candidates,
            referenced: self.referenced,
        };
        walk_arrow_function(&mut collector, func);
    }
}

/// JSX compilation options (passed from manifest or CLI)
#[derive(Debug, Clone)]
pub struct JsxOptions {
    /// Factory function name to call (e.g., "createElement", "h")
    pub factory: String,
    /// Fragment identifier (e.g., "Fragment")
    pub fragment: String,
    /// Development mode (adds __source/__self)
    pub development: bool,
}

impl Default for JsxOptions {
    fn default() -> Self {
        Self {
            factory: "createElement".to_string(),
            fragment: "Fragment".to_string(),
            development: false,
        }
    }
}

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
    /// Class type if the field is a class instance (for method resolution)
    class_type: Option<ClassId>,
    /// Type name string (for looking up class by name)
    type_name: Option<String>,
    /// For generic container fields (Map<K,V>, Set<T>): the value type's TypeId.
    /// Used to propagate return types through .get(), .values(), etc.
    value_type: Option<TypeId>,
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

/// Information about a decorator application
#[derive(Clone)]
struct DecoratorInfo {
    /// The decorator expression (e.g., `@Injectable` or `@Controller("/api")`)
    expression: Expression,
}

/// Target of a decorator (used during code generation)
enum DecoratorTarget {
    /// Class decorator - applied to the class itself
    Class { class_id: u32, class_name: String },
    /// Method decorator - applied to a specific method
    Method { class_id: u32, method_name: String },
    /// Field decorator - applied to a specific field
    Field { class_id: u32, field_name: String },
    /// Parameter decorator - applied to a specific parameter
    Parameter { class_id: u32, method_name: String, param_index: u32 },
}

/// Information about a method's decorators
#[derive(Clone)]
struct MethodDecoratorInfo {
    /// Method name
    method_name: Symbol,
    /// Decorators applied to this method
    decorators: Vec<DecoratorInfo>,
}

/// Information about a field's decorators
#[derive(Clone)]
struct FieldDecoratorInfo {
    /// Field name
    field_name: Symbol,
    /// Decorators applied to this field
    decorators: Vec<DecoratorInfo>,
}

/// Information about a parameter's decorators
#[derive(Clone)]
struct ParameterDecoratorInfo {
    /// Method name (or "constructor" for constructor params)
    method_name: String,
    /// Parameter index (0-based)
    param_index: u32,
    /// Decorators applied to this parameter
    decorators: Vec<DecoratorInfo>,
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
    /// Type arg substitutions for generic parent (param_name → concrete TypeId)
    extends_type_subs: Option<std::collections::HashMap<String, TypeId>>,
    /// Number of vtable method slots (including inherited)
    method_slot_count: u16,
    /// Class-level decorators (applied bottom-to-top)
    class_decorators: Vec<DecoratorInfo>,
    /// Method decorators (keyed by method name)
    method_decorators: Vec<MethodDecoratorInfo>,
    /// Field decorators (keyed by field name)
    field_decorators: Vec<FieldDecoratorInfo>,
    /// Parameter decorators (keyed by method name and param index)
    parameter_decorators: Vec<ParameterDecoratorInfo>,
}

/// Loop context for break/continue handling
#[derive(Clone)]
struct LoopContext {
    /// Block to jump to for break
    break_target: BasicBlockId,
    /// Block to jump to for continue
    continue_target: BasicBlockId,
    /// Depth of try_finally_stack when this loop started
    /// (used to know which finally blocks to inline on break/continue)
    try_finally_depth: usize,
}

/// Entry on the try-finally stack for inline finally duplication
#[derive(Clone)]
struct TryFinallyEntry {
    /// Cloned AST of the finally block (inlined at return/break/continue sites)
    finally_body: crate::parser::ast::BlockStatement,
    /// True when we're inside the try body (exception handler is active, need EndTry)
    /// False when we're inside the catch body (handler already consumed)
    in_try_body: bool,
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
    /// Class name to ID mapping (last class registered with a given name wins)
    class_map: FxHashMap<Symbol, ClassId>,
    /// Class info (fields, initializers) for lowering `new` expressions
    class_info_map: FxHashMap<ClassId, ClassInfo>,
    /// Per-declaration class ID (keyed by span start position, survives name collisions)
    class_decl_ids: FxHashMap<usize, ClassId>,
    /// Next function ID
    next_function_id: u32,
    /// Next class ID
    next_class_id: u32,
    /// Type alias name to ID mapping
    type_alias_map: FxHashMap<Symbol, TypeAliasId>,
    /// Next type alias ID
    next_type_alias_id: u32,
    /// Stack of loop contexts for break/continue
    loop_stack: Vec<LoopContext>,
    /// Stack of switch exit blocks (break inside switch targets the switch exit, not the enclosing loop)
    switch_stack: Vec<BasicBlockId>,
    /// Stack of try-finally contexts for inlining finally blocks at return/break/continue
    try_finally_stack: Vec<TryFinallyEntry>,
    /// Pending arrow functions to be added to module (with their assigned func_id)
    pending_arrow_functions: Vec<(u32, IrFunction)>,
    /// Pending classes from nested declarations (inside function bodies)
    pending_classes: Vec<IrClass>,
    /// Counter for generating unique arrow function names
    arrow_counter: u32,
    /// All variables from ancestor scopes (when inside an arrow function)
    /// Maps symbol to its source (immediate parent local or ancestor)
    ancestor_variables: Option<FxHashMap<Symbol, AncestorVar>>,
    /// Captured variables for the current arrow function
    captures: Vec<CaptureInfo>,
    /// Next available capture slot index (shared by both `this` and regular captures)
    next_capture_slot: u16,
    /// Info about the last created closure (for self-recursive closure detection)
    /// Contains (closure_register, Vec<(symbol, capture_index)>)
    last_closure_info: Option<(Register, Vec<(Symbol, u16)>)>,
    /// Function ID of the last lowered arrow (for async closure tracking in var decls)
    last_arrow_func_id: Option<FunctionId>,
    /// Variables that need RefCell wrapping (captured and potentially modified)
    refcell_vars: FxHashSet<Symbol>,
    /// Map from local variable to its RefCell register (for variables stored in RefCells)
    refcell_registers: FxHashMap<u16, Register>,
    /// Variables that are captured by any closure (read or write) - used for per-iteration bindings in loops
    loop_captured_vars: FxHashSet<Symbol>,
    /// Map from variable name to its class type (for field access resolution)
    variable_class_map: FxHashMap<Symbol, ClassId>,
    /// Map from array variable name to its element's class type (for for-of loop type inference)
    array_element_class_map: FxHashMap<Symbol, ClassId>,
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
    /// Method name to vtable slot index (for virtual dispatch)
    method_slot_map: FxHashMap<(ClassId, Symbol), u16>,
    /// Static method name to function ID mapping
    static_method_map: FxHashMap<(ClassId, Symbol), FunctionId>,
    /// Method return type class mapping (for chained method call resolution)
    method_return_class_map: FxHashMap<(ClassId, Symbol), ClassId>,
    /// Function return type class mapping (for method dispatch on objects returned from standalone function calls)
    function_return_class_map: FxHashMap<Symbol, ClassId>,
    /// Method return TypeId mapping (for ALL return types, not just class types)
    /// Populated during class registration. Used for bound method return type propagation.
    method_return_type_map: FxHashMap<(ClassId, Symbol), TypeId>,
    /// Tracks variables holding bound methods: var_name → (class_id, method_name)
    /// Used to propagate return types when calling bound method variables.
    bound_method_vars: FxHashMap<Symbol, (ClassId, Symbol)>,
    /// Next global variable index (for static fields and module-level variables)
    next_global_index: u16,
    /// Module-level variable name to global index mapping.
    /// Variables stored as globals so both main and module-level functions can access them.
    module_var_globals: FxHashMap<Symbol, u16>,
    /// Depth counter: 0 = module top-level, >0 = inside function declaration.
    /// Used to prevent `let x = ...` inside functions from hijacking module globals.
    function_depth: u32,
    /// Set of function IDs that are async closures (should be spawned as Tasks)
    async_closures: FxHashSet<FunctionId>,
    /// Map from local variable index to function ID for closures stored in variables
    /// Used to track async closures for SpawnClosure emission
    closure_locals: FxHashMap<u16, FunctionId>,
    /// Expression types from type checker (maps expr ptr to TypeId)
    expr_types: FxHashMap<usize, TypeId>,
    /// Type map for module-level globals (preserves initializer types through LoadGlobal)
    global_type_map: FxHashMap<u16, TypeId>,
    /// Compile-time constant values (for constant folding)
    /// Maps symbol to its constant value (only for literals)
    constant_map: FxHashMap<Symbol, ConstantValue>,
    /// Object field layout for registers from decode<T> calls
    /// Maps register id → Vec<(field_name, field_index)>
    register_object_fields: FxHashMap<RegisterId, Vec<(String, usize)>>,
    /// Object field layout for local variables holding decoded objects
    /// Maps variable name → Vec<(field_name, field_index)>
    variable_object_fields: FxHashMap<Symbol, Vec<(String, usize)>>,
    /// Native function name table for ModuleNativeCall.
    /// Accumulates symbolic names during lowering; each name gets a module-local index.
    native_function_table: Vec<String>,
    /// Reverse lookup: name → local index (for deduplication)
    native_function_map: FxHashMap<String, u16>,
    /// JSX compilation options (None = JSX not enabled)
    jsx_options: Option<JsxOptions>,
    /// Type parameter names for generic classes (ClassId → ["T", "E", ...])
    class_type_params: FxHashMap<ClassId, Vec<String>>,
    /// Whether to track source spans in IR for source map generation
    emit_sourcemap: bool,
    /// Current source span (set at statement/expression boundaries, used by emit/set_terminator)
    current_span: Span,
    /// Compile errors collected during lowering (e.g., UNRESOLVED type at dispatch)
    errors: Vec<super::error::CompileError>,
    /// Registry for type-specific method/property dispatch (single source of truth)
    type_registry: super::type_registry::TypeRegistry,
    /// Cache of compiled class method functions: "TypeName_methodName" → FunctionId
    class_method_cache: FxHashMap<String, FunctionId>,
    /// ASTs of generic functions (with type_params), stored for specialization at call sites.
    /// Key: function name symbol.
    generic_function_asts: FxHashMap<Symbol, ast::FunctionDecl>,
    /// Active type parameter substitutions during generic function specialization.
    /// Maps type parameter name (e.g., "T") to concrete TypeId (e.g., ARRAY_TYPE_ID).
    /// Empty when not specializing.
    type_param_substitutions: FxHashMap<String, TypeId>,
    /// Cache of already-specialized generic functions: "funcName$typeId1_typeId2" → FunctionId
    specialized_function_cache: FxHashMap<String, FunctionId>,
    /// Inner type for RefCell-wrapped variables (for preserving type info through loads)
    refcell_inner_types: FxHashMap<u16, TypeId>,
}

// ─── Standalone helpers for closure capture pre-scan ───────────────────────

/// Recursively extract all bound identifier names from a pattern.
/// Handles Identifier, Array destructuring, Object destructuring, and Rest patterns.
fn collect_pattern_names(pattern: &ast::Pattern, names: &mut FxHashSet<Symbol>) {
    match pattern {
        Pattern::Identifier(id) => {
            names.insert(id.name);
        }
        Pattern::Array(arr) => {
            for e in arr.elements.iter().flatten() {
                collect_pattern_names(&e.pattern, names);
            }
            if let Some(rest) = &arr.rest {
                collect_pattern_names(rest, names);
            }
        }
        Pattern::Object(obj) => {
            for prop in &obj.properties {
                collect_pattern_names(&prop.value, names);
            }
            if let Some(rest) = &obj.rest {
                names.insert(rest.name);
            }
        }
        Pattern::Rest(rest) => {
            collect_pattern_names(&rest.argument, names);
        }
    }
}

/// Recursively collect all local variable names from statements, including nested scopes.
/// Handles all pattern types and binding forms (destructuring, catch params, function names, etc.).
fn collect_block_local_names(stmts: &[ast::Statement], locals: &mut FxHashSet<Symbol>) {
    for stmt in stmts {
        match stmt {
            Statement::VariableDecl(var) => {
                collect_pattern_names(&var.pattern, locals);
            }
            Statement::FunctionDecl(func) => {
                // Function name is a local binding in the enclosing scope
                locals.insert(func.name.name);
            }
            Statement::For(for_stmt) => {
                if let Some(ast::ForInit::VariableDecl(var)) = &for_stmt.init {
                    collect_pattern_names(&var.pattern, locals);
                }
                recurse_into_body(&for_stmt.body, locals);
            }
            Statement::ForOf(for_of) => {
                match &for_of.left {
                    ast::ForOfLeft::VariableDecl(var) => {
                        collect_pattern_names(&var.pattern, locals);
                    }
                    ast::ForOfLeft::Pattern(pat) => {
                        collect_pattern_names(pat, locals);
                    }
                }
                recurse_into_body(&for_of.body, locals);
            }
            Statement::Try(try_stmt) => {
                collect_block_local_names(&try_stmt.body.statements, locals);
                if let Some(catch) = &try_stmt.catch_clause {
                    if let Some(param) = &catch.param {
                        collect_pattern_names(param, locals);
                    }
                    collect_block_local_names(&catch.body.statements, locals);
                }
                if let Some(finally) = &try_stmt.finally_clause {
                    collect_block_local_names(&finally.statements, locals);
                }
            }
            Statement::Switch(sw) => {
                for case in &sw.cases {
                    collect_block_local_names(&case.consequent, locals);
                }
            }
            Statement::While(w) => recurse_into_body(&w.body, locals),
            Statement::DoWhile(dw) => recurse_into_body(&dw.body, locals),
            Statement::If(if_stmt) => {
                recurse_into_body(&if_stmt.then_branch, locals);
                if let Some(else_br) = &if_stmt.else_branch {
                    recurse_into_body(else_br, locals);
                }
            }
            Statement::Block(block) => {
                collect_block_local_names(&block.statements, locals);
            }
            _ => {}
        }
    }
}

fn recurse_into_body(body: &ast::Statement, locals: &mut FxHashSet<Symbol>) {
    if let Statement::Block(block) = body {
        collect_block_local_names(&block.statements, locals);
    } else {
        collect_block_local_names(std::slice::from_ref(body), locals);
    }
}

// ─── Visitor-based closure capture analysis ────────────────────────────────

/// Walks an enclosing scope to find closure-creating constructs (arrows + nested function decls).
/// When found, delegates to `CapturedRefAnalyzer` to analyze what outer variables the closure references.
/// Uses `walk_*` functions from the Visitor trait for complete AST coverage.
struct ArrowCaptureFinder<'a> {
    outer_locals: &'a FxHashSet<Symbol>,
    refcell_vars: &'a mut FxHashSet<Symbol>,
    loop_captured_vars: &'a mut FxHashSet<Symbol>,
}

impl<'a> ArrowCaptureFinder<'a> {
    /// Analyze a closure body (block form) for captured references to outer variables.
    /// Also scans default parameter expressions.
    fn analyze_closure_body(
        &mut self,
        params: &[ast::Parameter],
        body: &ast::BlockStatement,
    ) {
        let mut closure_locals = FxHashSet::default();
        for param in params {
            collect_pattern_names(&param.pattern, &mut closure_locals);
        }
        collect_block_local_names(&body.statements, &mut closure_locals);

        let mut analyzer = CapturedRefAnalyzer {
            outer_locals: self.outer_locals,
            arrow_locals: &closure_locals,
            refcell_vars: self.refcell_vars,
            loop_captured_vars: self.loop_captured_vars,
        };
        for stmt in &body.statements {
            analyzer.visit_statement(stmt);
        }
        // Scan default parameter expressions for captures (Bug #5)
        for param in params {
            if let Some(default_expr) = &param.default_value {
                analyzer.visit_expression(default_expr);
            }
        }
    }
}

impl Visitor for ArrowCaptureFinder<'_> {
    fn visit_expression(&mut self, expr: &Expression) {
        if let Expression::Arrow(arrow) = expr {
            // Arrow function — analyze body for captures
            match &arrow.body {
                ast::ArrowBody::Expression(body_expr) => {
                    let mut closure_locals = FxHashSet::default();
                    for param in &arrow.params {
                        collect_pattern_names(&param.pattern, &mut closure_locals);
                    }
                    let mut analyzer = CapturedRefAnalyzer {
                        outer_locals: self.outer_locals,
                        arrow_locals: &closure_locals,
                        refcell_vars: self.refcell_vars,
                        loop_captured_vars: self.loop_captured_vars,
                    };
                    analyzer.visit_expression(body_expr);
                    for param in &arrow.params {
                        if let Some(default_expr) = &param.default_value {
                            analyzer.visit_expression(default_expr);
                        }
                    }
                }
                ast::ArrowBody::Block(block) => {
                    self.analyze_closure_body(&arrow.params, block);
                }
            }
            return; // Don't walk into arrow body — it's a separate scope
        }
        walk_expression(self, expr);
    }

    fn visit_function_decl(&mut self, func: &ast::FunctionDecl) {
        // Nested function declaration — creates a closure scope (lowered as synthetic arrow).
        // Analyze its body + default params for captures, then STOP.
        self.analyze_closure_body(&func.params, &func.body);
        // Don't call walk_function_decl — it's a scope boundary
    }

    fn visit_class_decl(&mut self, _: &ast::ClassDecl) {
        // Class declarations are separate scopes — methods/constructors get their OWN
        // scan_for_captured_vars call. Don't descend from the enclosing scope's scan.
    }
}

/// Walks inside a closure body to find identifiers that reference outer-scope variables.
/// - Read captures: any outer-scope identifier → `loop_captured_vars`
/// - Write captures: assignments to outer-scope identifiers → `refcell_vars`
struct CapturedRefAnalyzer<'a> {
    outer_locals: &'a FxHashSet<Symbol>,
    arrow_locals: &'a FxHashSet<Symbol>,
    refcell_vars: &'a mut FxHashSet<Symbol>,
    loop_captured_vars: &'a mut FxHashSet<Symbol>,
}

impl Visitor for CapturedRefAnalyzer<'_> {
    fn visit_identifier(&mut self, id: &ast::Identifier) {
        if self.outer_locals.contains(&id.name) && !self.arrow_locals.contains(&id.name) {
            self.loop_captured_vars.insert(id.name);
        }
    }

    fn visit_expression(&mut self, expr: &Expression) {
        // Track write captures — these need RefCell wrapping
        if let Expression::Assignment(assign) = expr {
            if let Expression::Identifier(ident) = &*assign.left {
                if self.outer_locals.contains(&ident.name)
                    && !self.arrow_locals.contains(&ident.name)
                {
                    self.refcell_vars.insert(ident.name);
                }
            }
        }
        // Nested arrow = new scope boundary — delegate back to ArrowCaptureFinder
        if let Expression::Arrow(_) = expr {
            let mut finder = ArrowCaptureFinder {
                outer_locals: self.outer_locals,
                refcell_vars: self.refcell_vars,
                loop_captured_vars: self.loop_captured_vars,
            };
            finder.visit_expression(expr);
            return;
        }
        walk_expression(self, expr);
    }

    fn visit_function_decl(&mut self, func: &ast::FunctionDecl) {
        // Nested function inside closure body — also a scope boundary
        let mut finder = ArrowCaptureFinder {
            outer_locals: self.outer_locals,
            refcell_vars: self.refcell_vars,
            loop_captured_vars: self.loop_captured_vars,
        };
        finder.visit_function_decl(func);
    }

    fn visit_class_decl(&mut self, _: &ast::ClassDecl) {
        // Don't descend into nested class declarations
    }
}

/// Collects all variable names that appear as assignment targets in a scope.
/// Does NOT descend into arrow/function/class bodies (they are separate scopes).
struct ScopeAssignmentCollector<'a> {
    assigned: &'a mut FxHashSet<Symbol>,
}

impl Visitor for ScopeAssignmentCollector<'_> {
    fn visit_expression(&mut self, expr: &Expression) {
        if let Expression::Assignment(assign) = expr {
            if let Expression::Identifier(ident) = &*assign.left {
                self.assigned.insert(ident.name);
            }
        }
        if matches!(expr, Expression::Arrow(_)) {
            return; // Don't enter separate scopes
        }
        walk_expression(self, expr);
    }

    fn visit_arrow_function(&mut self, _: &ast::ArrowFunction) {}
    fn visit_function_decl(&mut self, _: &ast::FunctionDecl) {}
    fn visit_class_decl(&mut self, _: &ast::ClassDecl) {}
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
            class_decl_ids: FxHashMap::default(),
            next_function_id: 0,
            next_class_id: 0,
            type_alias_map: FxHashMap::default(),
            next_type_alias_id: 0,
            loop_stack: Vec::new(),
            switch_stack: Vec::new(),
            try_finally_stack: Vec::new(),
            pending_arrow_functions: Vec::new(),
            pending_classes: Vec::new(),
            arrow_counter: 0,
            ancestor_variables: None,
            captures: Vec::new(),
            next_capture_slot: 0,
            last_closure_info: None,
            last_arrow_func_id: None,
            refcell_vars: FxHashSet::default(),
            refcell_registers: FxHashMap::default(),
            loop_captured_vars: FxHashSet::default(),
            refcell_inner_types: FxHashMap::default(),
            variable_class_map: FxHashMap::default(),
            array_element_class_map: FxHashMap::default(),
            current_class: None,
            this_register: None,
            this_ancestor_info: None,
            this_captured_idx: None,
            method_map: FxHashMap::default(),
            method_slot_map: FxHashMap::default(),
            static_method_map: FxHashMap::default(),
            method_return_class_map: FxHashMap::default(),
            function_return_class_map: FxHashMap::default(),
            method_return_type_map: FxHashMap::default(),
            bound_method_vars: FxHashMap::default(),
            next_global_index: 0,
            module_var_globals: FxHashMap::default(),
            function_depth: 0,
            async_closures: FxHashSet::default(),
            closure_locals: FxHashMap::default(),
            expr_types,
            global_type_map: FxHashMap::default(),
            constant_map: FxHashMap::default(),
            register_object_fields: FxHashMap::default(),
            variable_object_fields: FxHashMap::default(),
            native_function_table: Vec::new(),
            native_function_map: FxHashMap::default(),
            jsx_options: None,
            class_type_params: FxHashMap::default(),
            emit_sourcemap: false,
            current_span: Span::default(),
            errors: Vec::new(),
            type_registry: super::type_registry::TypeRegistry::new(type_ctx),
            class_method_cache: FxHashMap::default(),
            generic_function_asts: FxHashMap::default(),
            type_param_substitutions: FxHashMap::default(),
            specialized_function_cache: FxHashMap::default(),
        }
    }

    /// Enable/disable source map span tracking in generated IR
    pub fn with_sourcemap(mut self, enable: bool) -> Self {
        self.emit_sourcemap = enable;
        self
    }

    /// Enable JSX compilation with the given options
    pub fn with_jsx(mut self, options: JsxOptions) -> Self {
        self.jsx_options = Some(options);
        self
    }

    /// Report an unresolved type error at a dispatch point.
    /// Mimics TypeScript's strict type errors — never silently emit incorrect bytecode.
    fn report_unresolved_type(&mut self, context: &str, property: &str) {
        self.errors.push(super::error::CompileError::InternalError {
            message: format!(
                "Cannot resolve type for '{}' access on '{}'. \
                 This is a compiler bug — the type should be known at compile time.",
                context, property
            ),
        });
    }

    /// Get collected compile errors
    pub fn errors(&self) -> &[super::error::CompileError] {
        &self.errors
    }

    /// Resolve a native function name to a module-local index.
    /// Adds the name to the table if not already present.
    fn resolve_native_name(&mut self, name: &str) -> u16 {
        if let Some(&idx) = self.native_function_map.get(name) {
            return idx;
        }
        let idx = self.native_function_table.len() as u16;
        self.native_function_table.push(name.to_string());
        self.native_function_map.insert(name.to_string(), idx);
        idx
    }

    /// Get the native function table (consumed after lowering)
    pub fn take_native_function_table(&mut self) -> Vec<String> {
        std::mem::take(&mut self.native_function_table)
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

    /// Get the TypeId for an expression from the type checker's expr_types map.
    /// Falls back to UNRESOLVED if not found (compiler couldn't determine type).
    fn get_expr_type(&self, expr: &Expression) -> TypeId {
        let expr_id = expr as *const _ as usize;
        self.expr_types.get(&expr_id).copied().unwrap_or(UNRESOLVED)
    }

    /// Normalize a TypeId for dispatch purposes.
    ///
    /// The type checker and lowerer use different TypeId representations:
    /// - Pre-interned canonical IDs (0–17) are directly dispatch-compatible
    /// - Dynamically interned IDs (> 17) represent unions, generics, etc.
    ///
    /// Normalize a type to its canonical dispatch type via the TypeRegistry.
    ///
    /// Maps dynamic IDs back to canonical dispatch IDs. For example:
    /// - `Array<number>` (TypeId 18+) → ARRAY_TYPE_ID (17)
    /// - `string | null` union → String (1) via dominant non-null member
    /// - Ambiguous unions like `string | number` → compile error
    fn normalize_type_for_dispatch(&mut self, type_id: u32) -> u32 {
        match self.type_registry.normalize_type(type_id, self.type_ctx) {
            Ok(id) => id,
            Err(msg) => {
                self.errors.push(super::error::CompileError::InternalError {
                    message: msg,
                });
                UNRESOLVED_TYPE_ID
            }
        }
    }

    /// Unwrap `ExportDecl::Declaration` to get the inner statement.
    /// Returns the statement as-is for non-export statements.
    fn unwrap_export(stmt: &Statement) -> &Statement {
        if let Statement::ExportDecl(ExportDecl::Declaration(inner)) = stmt {
            inner.as_ref()
        } else {
            stmt
        }
    }

    /// Lower an AST module to IR
    pub fn lower_module(&mut self, module: &ast::Module) -> IrModule {
        let mut ir_module = IrModule::new("main");

        // Pre-pass: collect module-level const declarations (for constant folding)
        // These need to be processed before classes/functions so they're available
        for raw_stmt in &module.statements {
            let stmt = Self::unwrap_export(raw_stmt);
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

        // Pre-pass: assign global indices to module-level let/var declarations
        // so both main and module-level functions can access them via LoadGlobal/StoreGlobal.
        // Only promote variables that are actually referenced by module-level function bodies.
        {
            // Step 1: Collect candidate module-level variable names (excluding constants)
            let candidates: FxHashSet<Symbol> = module.statements.iter()
                .filter_map(|s| {
                    let s = Self::unwrap_export(s);
                    if let Statement::VariableDecl(decl) = s {
                        if let Pattern::Identifier(ident) = &decl.pattern {
                            if !self.constant_map.contains_key(&ident.name) {
                                return Some(ident.name);
                            }
                        }
                    }
                    None
                })
                .collect();

            // Step 2: Walk function/closure bodies to find which candidates they reference
            let mut referenced = FxHashSet::default();
            if !candidates.is_empty() {
                for raw_stmt in &module.statements {
                    let stmt = Self::unwrap_export(raw_stmt);
                    match stmt {
                        Statement::FunctionDecl(func) => {
                            let mut collector = ModuleVarRefCollector {
                                candidates: &candidates,
                                referenced: &mut referenced,
                            };
                            walk_block_statement(&mut collector, &func.body);
                        }
                        Statement::Expression(expr_stmt) => {
                            let mut collector = ArrowBodyVarRefCollector {
                                candidates: &candidates,
                                referenced: &mut referenced,
                            };
                            walk_expression(&mut collector, &expr_stmt.expression);
                        }
                        Statement::VariableDecl(decl) => {
                            if let Some(initializer) = &decl.initializer {
                                let mut collector = ArrowBodyVarRefCollector {
                                    candidates: &candidates,
                                    referenced: &mut referenced,
                                };
                                walk_expression(&mut collector, initializer);
                            }
                        }
                        _ => {}
                    }
                }
            }

            // Step 3: Only promote variables that are actually referenced by functions
            for raw_stmt in &module.statements {
                let stmt = Self::unwrap_export(raw_stmt);
                if let Statement::VariableDecl(decl) = stmt {
                    if let Pattern::Identifier(ident) = &decl.pattern {
                        if referenced.contains(&ident.name) {
                            let global_index = self.next_global_index;
                            self.next_global_index += 1;
                            self.module_var_globals.insert(ident.name, global_index);
                        }
                    }
                }
            }
        }

        // First pass: collect function and class declarations
        for raw_stmt in &module.statements {
            let stmt = Self::unwrap_export(raw_stmt);
            match stmt {
                Statement::FunctionDecl(func) => {
                    let id = FunctionId::new(self.next_function_id);
                    self.next_function_id += 1;
                    self.function_map.insert(func.name.name, id);
                    // Track async functions for Spawn emission
                    if func.is_async {
                        self.async_functions.insert(id);
                    }
                    // Track return type for method dispatch on returned objects
                    if let Some(ret_type) = &func.return_type {
                        if let ast::Type::Reference(type_ref) = &ret_type.ty {
                            if let Some(&class_id) = self.class_map.get(&type_ref.name.name) {
                                self.function_return_class_map.insert(func.name.name, class_id);
                            }
                        }
                    }
                    // Store AST for generic functions (needed for call-site specialization)
                    if func.type_params.as_ref().is_some_and(|tp| !tp.is_empty()) {
                        self.generic_function_asts.insert(func.name.name, func.clone());
                    }
                }
                Statement::ClassDecl(class) => {
                    self.register_class(class);
                }
                Statement::TypeAliasDecl(type_alias) => {
                    // Register type alias for JSON decode support
                    let type_alias_id = TypeAliasId::new(self.next_type_alias_id);
                    self.next_type_alias_id += 1;
                    self.type_alias_map.insert(type_alias.name.name, type_alias_id);
                }
                _ => {}
            }
        }

        // Pre-pass: populate variable_class_map for module-level variable declarations.
        // This must happen BEFORE the second pass (which lowers functions) so that
        // functions referencing module-level variables (e.g., `math.abs()` where
        // `const math = new Math()`) can resolve the correct class type for method dispatch.
        for raw_stmt in &module.statements {
            let stmt = Self::unwrap_export(raw_stmt);
            if let Statement::VariableDecl(decl) = stmt {
                if let Pattern::Identifier(ident) = &decl.pattern {
                    let name = ident.name;
                    // Track class type from explicit type annotation
                    if let Some(type_ann) = &decl.type_annotation {
                        if let Some(class_id) = self.try_extract_class_from_type(type_ann) {
                            self.variable_class_map.insert(name, class_id);
                        }
                    }
                    // Track class type from new expression (e.g., `const math = new Math()`)
                    #[allow(clippy::collapsible_match)]
                    if !self.variable_class_map.contains_key(&name) {
                        if let Some(init) = &decl.initializer {
                            if let ast::Expression::New(new_expr) = init {
                                if let ast::Expression::Identifier(class_ident) = &*new_expr.callee {
                                    if let Some(&class_id) = self.class_map.get(&class_ident.name) {
                                        self.variable_class_map.insert(name, class_id);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Second pass: lower all declarations
        // IMPORTANT: All functions must be added to pending_arrow_functions with their pre-assigned IDs
        // so they can be sorted and added to the module in the correct order.
        // This ensures function indices match the pre-assigned IDs used in Call instructions.
        for raw_stmt in &module.statements {
            let stmt = Self::unwrap_export(raw_stmt);
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
                Statement::TypeAliasDecl(type_alias) => {
                    // Only process object types (struct-like type aliases)
                    if let Some(ir_type_alias) = self.lower_type_alias(type_alias) {
                        ir_module.add_type_alias(ir_type_alias);
                    }
                }
                _ => {
                    // Top-level statements go into an implicit main function
                }
            }
        }

        // Collect top-level statements for main function.
        // ExportDecl::Declaration wrapping a func/class/type-alias was already handled above;
        // ExportDecl::Declaration wrapping a VariableDecl needs to go through top-level lowering.
        let top_level_stmts: Vec<_> = module
            .statements
            .iter()
            .filter(|s| {
                let inner = Self::unwrap_export(s);
                !matches!(
                    inner,
                    Statement::FunctionDecl(_)
                        | Statement::ClassDecl(_)
                        | Statement::TypeAliasDecl(_)
                )
            })
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

        // Add pending classes from nested declarations (inside function bodies)
        for ir_class in self.pending_classes.drain(..) {
            ir_module.add_class(ir_class);
        }

        // Add ALL pending functions (including main and class methods) sorted by func_id
        // This ensures functions are added to the module in the order of their pre-assigned IDs
        self.pending_arrow_functions.sort_by_key(|(id, _)| *id);
        for (_id, func) in self.pending_arrow_functions.drain(..) {
            ir_module.add_function(func);
        }

        // Transfer native function table to the IR module
        ir_module.native_functions = self.take_native_function_table();

        ir_module
    }

    /// Pre-scan statements to identify variables that will be captured by closures.
    /// These variables need RefCell wrapping for capture-by-reference semantics.
    /// Uses Visitor-based traversal for complete AST coverage.
    fn scan_for_captured_vars(
        &mut self,
        stmts: &[ast::Statement],
        params: &[ast::Parameter],
        locals: &FxHashSet<Symbol>,
    ) {
        // Phase 1: Find closures (arrows + nested functions) and analyze their captures
        let mut finder = ArrowCaptureFinder {
            outer_locals: locals,
            refcell_vars: &mut self.refcell_vars,
            loop_captured_vars: &mut self.loop_captured_vars,
        };
        for stmt in stmts {
            finder.visit_statement(stmt);
        }
        // Also scan default parameter expressions for closures (Bug #5)
        for param in params {
            if let Some(default_expr) = &param.default_value {
                finder.visit_expression(default_expr);
            }
        }

        // Phase 2: Promote read-captured vars to RefCell if assigned in enclosing scope.
        // This ensures closures see the live value, not a stale copy.
        if !self.loop_captured_vars.is_empty() {
            let mut assigned = FxHashSet::default();
            let mut collector = ScopeAssignmentCollector { assigned: &mut assigned };
            for stmt in stmts {
                collector.visit_statement(stmt);
            }
            for var in self.loop_captured_vars.clone() {
                if assigned.contains(&var) {
                    self.refcell_vars.insert(var);
                }
            }
        }
    }

    /// Collect all local variable names declared in statements
    fn collect_local_names(&self, stmts: &[ast::Statement]) -> FxHashSet<Symbol> {
        let mut locals = FxHashSet::default();
        collect_block_local_names(stmts, &mut locals);
        locals
    }

    /// Register a class declaration (first-pass registration).
    /// Assigns class ID, collects fields/methods/constructor info, builds ClassInfo.
    /// Must be called before `lower_class` for the same class.
    fn register_class(&mut self, class: &ast::ClassDecl) {
        let class_id = ClassId::new(self.next_class_id);
        self.next_class_id += 1;

        // Track per-declaration class ID (survives name collisions)
        self.class_decl_ids.insert(class.span.start, class_id);

        // Insert into class_map (last class with a given name wins for name-based lookups)
        self.class_map.insert(class.name.name, class_id);

        // Store type parameter names for generic classes
        if let Some(ref type_params) = class.type_params {
            if !type_params.is_empty() {
                let param_names: Vec<String> = type_params.iter()
                    .map(|tp| self.interner.resolve(tp.name.name).to_string())
                    .collect();
                self.class_type_params.insert(class_id, param_names);
            }
        }

        // Resolve parent class if extends clause is present
        let mut extends_type_args: Option<Vec<TypeId>> = None;
        let parent_class = if let Some(ref extends) = class.extends {
            if let ast::Type::Reference(type_ref) = &extends.ty {
                // Extract type arguments from extends clause (e.g., Base<string>)
                if let Some(ref type_args) = type_ref.type_args {
                    let resolved: Vec<TypeId> = type_args.iter()
                        .map(|ta| self.resolve_type_annotation(ta))
                        .collect();
                    extends_type_args = Some(resolved);
                }
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

        // Start field index after ALL ancestor fields (not just immediate parent)
        let mut parent_fields = if let Some(parent_id) = parent_class {
            self.get_all_fields(parent_id)
        } else {
            Vec::new()
        };

        // If extends has type args (e.g., extends Base<string>), build substitution
        // map and substitute parent field types with concrete types
        let extends_type_subs = if let Some(ref type_args) = extends_type_args {
            if let Some(parent_id) = parent_class {
                if let Some(parent_type_params) = self.class_type_params.get(&parent_id).cloned() {
                    if parent_type_params.len() == type_args.len() {
                        // Build TypeVar name → concrete TypeId mapping
                        let subs: std::collections::HashMap<String, TypeId> = parent_type_params.iter()
                            .zip(type_args.iter())
                            .map(|(name, &ty)| (name.clone(), ty))
                            .collect();

                        // Substitute type parameter field types with concrete types
                        // Uses field.type_name (original type annotation name) since
                        // the lowerer maps unknown type refs to TypeId(7), not TypeVar
                        for field in &mut parent_fields {
                            if let Some(ref name) = field.type_name {
                                if let Some(&concrete_ty) = subs.get(name.as_str()) {
                                    field.ty = concrete_ty;
                                }
                            }
                        }
                        Some(subs)
                    } else { None }
                } else { None }
            } else { None }
        } else { None };
        let mut field_index = parent_fields.len() as u16;

        for member in &class.members {
            if let ast::ClassMember::Field(field) = member {
                let ty = field
                    .type_annotation
                    .as_ref()
                    .map(|t| self.resolve_type_annotation(t))
                    .unwrap_or(TypeId::new(0));

                let type_name = field.type_annotation.as_ref().and_then(|t| {
                    if let ast::Type::Reference(type_ref) = &t.ty {
                        Some(self.interner.resolve(type_ref.name.name).to_string())
                    } else {
                        None
                    }
                });

                // Extract generic value type for Map<K,V> and Set<T> fields
                let value_type = field.type_annotation.as_ref().and_then(|t| {
                    if let ast::Type::Reference(type_ref) = &t.ty {
                        let name = self.interner.resolve(type_ref.name.name);
                        let type_args = type_ref.type_args.as_ref()?;
                        match name {
                            "Map" => type_args.get(1).map(|v| self.resolve_type_annotation(v)),
                            "Set" => type_args.first().map(|t| self.resolve_type_annotation(t)),
                            _ => None,
                        }
                    } else {
                        None
                    }
                });

                let class_type = type_name.as_ref().and_then(|name| {
                    for (&sym, &cid) in &self.class_map {
                        if self.interner.resolve(sym) == name {
                            return Some(cid);
                        }
                    }
                    None
                });

                if field.is_static {
                    let global_index = self.next_global_index;
                    self.next_global_index += 1;
                    static_fields.push(StaticFieldInfo {
                        name: field.name.name,
                        global_index,
                        initializer: field.initializer.clone(),
                    });
                } else {
                    // Check if this field shadows a parent field with the same name.
                    // If so, reuse the parent's field index so base class methods
                    // that access `this.x` see the derived class's value.
                    let field_name_str = self.interner.resolve(field.name.name);
                    let shadowed_index = parent_fields
                        .iter()
                        .find(|pf| self.interner.resolve(pf.name) == field_name_str)
                        .map(|pf| pf.index);

                    let idx = if let Some(parent_idx) = shadowed_index {
                        parent_idx
                    } else {
                        let idx = field_index;
                        field_index += 1;
                        idx
                    };

                    fields.push(ClassFieldInfo {
                        name: field.name.name,
                        index: idx,
                        ty,
                        initializer: field.initializer.clone(),
                        class_type,
                        type_name,
                        value_type,
                    });
                }
            }
        }

        // Add fields from constructor parameter properties (e.g., `constructor(public x: number)`)
        for member in &class.members {
            if let ast::ClassMember::Constructor(ctor) = member {
                for param in &ctor.params {
                    if param.visibility.is_some() {
                        if let ast::Pattern::Identifier(ident) = &param.pattern {
                            let ty = param
                                .type_annotation
                                .as_ref()
                                .map(|t| self.resolve_type_annotation(t))
                                .unwrap_or(TypeId::new(0));
                            let idx = field_index;
                            field_index += 1;
                            fields.push(ClassFieldInfo {
                                name: ident.name,
                                index: idx,
                                ty,
                                initializer: None,
                                class_type: None,
                                type_name: None,
                                value_type: None,
                            });
                        }
                    }
                }
                break;
            }
        }

        // Collect instance and static method information
        let mut methods = Vec::new();
        let mut static_methods_vec = Vec::new();
        for member in &class.members {
            if let ast::ClassMember::Method(method) = member {
                if method.body.is_some() {
                    let func_id = FunctionId::new(self.next_function_id);
                    self.next_function_id += 1;

                    if method.is_async {
                        self.async_functions.insert(func_id);
                    }

                    if method.is_static {
                        static_methods_vec.push(StaticMethodInfo {
                            name: method.name.name,
                            func_id,
                        });
                        self.static_method_map
                            .insert((class_id, method.name.name), func_id);
                    } else {
                        methods.push(ClassMethodInfo {
                            name: method.name.name,
                            func_id,
                        });
                        self.method_map.insert((class_id, method.name.name), func_id);
                    }

                    if let Some(ret_type) = &method.return_type {
                        if let ast::Type::Reference(type_ref) = &ret_type.ty {
                            if let Some(&ret_class_id) = self.class_map.get(&type_ref.name.name) {
                                self.method_return_class_map.insert((class_id, method.name.name), ret_class_id);
                            }
                        }
                        // Store full return TypeId for all return types (bound method propagation)
                        let type_id = self.resolve_type_annotation(ret_type);
                        self.method_return_type_map.insert((class_id, method.name.name), type_id);
                    }
                }
            }
        }

        // Assign vtable method slots
        let parent_slot_count = parent_class
            .and_then(|pid| self.class_info_map.get(&pid))
            .map_or(0, |info| info.method_slot_count);
        let mut next_slot = parent_slot_count;

        // Reserve vtable slots for abstract methods first (they have no body
        // but need slots so derived classes override at the correct position)
        for member in &class.members {
            if let ast::ClassMember::Method(method) = member {
                if method.body.is_none() && !method.is_static {
                    let method_name = method.name.name;
                    let slot = self.find_parent_method_slot(parent_class, method_name)
                        .unwrap_or_else(|| { let s = next_slot; next_slot += 1; s });
                    self.method_slot_map.insert((class_id, method_name), slot);
                }
            }
        }

        for method_info in &methods {
            let slot = self.find_parent_method_slot(parent_class, method_info.name)
                .unwrap_or_else(|| { let s = next_slot; next_slot += 1; s });
            self.method_slot_map.insert((class_id, method_info.name), slot);
        }
        let method_slot_count = next_slot;

        // Constructor
        let mut constructor = None;
        let mut constructor_params = Vec::new();
        for member in &class.members {
            if let ast::ClassMember::Constructor(ctor) = member {
                let func_id = FunctionId::new(self.next_function_id);
                self.next_function_id += 1;
                constructor = Some(func_id);
                for param in &ctor.params {
                    constructor_params.push(ConstructorParamInfo {
                        default_value: param.default_value.clone(),
                    });
                }
                break;
            }
        }

        // Decorators
        let class_decorators: Vec<DecoratorInfo> = class
            .decorators
            .iter()
            .map(|d| DecoratorInfo { expression: d.expression.clone() })
            .collect();

        let mut method_decorators = Vec::new();
        for member in &class.members {
            if let ast::ClassMember::Method(method) = member {
                if !method.decorators.is_empty() {
                    method_decorators.push(MethodDecoratorInfo {
                        method_name: method.name.name,
                        decorators: method.decorators.iter()
                            .map(|d| DecoratorInfo { expression: d.expression.clone() })
                            .collect(),
                    });
                }
            }
        }

        let mut field_decorators = Vec::new();
        for member in &class.members {
            if let ast::ClassMember::Field(field) = member {
                if !field.decorators.is_empty() {
                    field_decorators.push(FieldDecoratorInfo {
                        field_name: field.name.name,
                        decorators: field.decorators.iter()
                            .map(|d| DecoratorInfo { expression: d.expression.clone() })
                            .collect(),
                    });
                }
            }
        }

        let mut parameter_decorators = Vec::new();
        for member in &class.members {
            match member {
                ast::ClassMember::Method(method) => {
                    let method_name = self.interner.resolve(method.name.name).to_string();
                    for (index, param) in method.params.iter().enumerate() {
                        if !param.decorators.is_empty() {
                            parameter_decorators.push(ParameterDecoratorInfo {
                                method_name: method_name.clone(),
                                param_index: index as u32,
                                decorators: param.decorators.iter()
                                    .map(|d| DecoratorInfo { expression: d.expression.clone() })
                                    .collect(),
                            });
                        }
                    }
                }
                ast::ClassMember::Constructor(ctor) => {
                    for (index, param) in ctor.params.iter().enumerate() {
                        if !param.decorators.is_empty() {
                            parameter_decorators.push(ParameterDecoratorInfo {
                                method_name: "constructor".to_string(),
                                param_index: index as u32,
                                decorators: param.decorators.iter()
                                    .map(|d| DecoratorInfo { expression: d.expression.clone() })
                                    .collect(),
                            });
                        }
                    }
                }
                _ => {}
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
                static_methods: static_methods_vec,
                parent_class,
                extends_type_subs,
                method_slot_count,
                class_decorators,
                method_decorators,
                field_decorators,
                parameter_decorators,
            },
        );
    }

    /// Lower a function declaration
    fn lower_function(&mut self, func: &ast::FunctionDecl) -> IrFunction {
        // Track that we're inside a function (prevents var decls from hijacking module globals)
        self.function_depth += 1;

        // Reset per-function state
        self.next_register = 0;
        self.next_block = 0;
        self.local_map.clear();
        self.local_registers.clear();
        self.next_local = 0;
        self.refcell_vars.clear();
        self.refcell_registers.clear();
        self.refcell_inner_types.clear();
        self.loop_captured_vars.clear();

        // Pre-scan to identify captured variables
        let mut locals = FxHashSet::default();
        for param in &func.params {
            collect_pattern_names(&param.pattern, &mut locals);
        }
        locals.extend(self.collect_local_names(&func.body.statements));
        self.scan_for_captured_vars(&func.body.statements, &func.params, &locals);

        // Get function name
        let name = self.interner.resolve(func.name.name);

        // Create parameter registers
        let mut params = Vec::new();
        for param in &func.params {
            let ty = param
                .type_annotation
                .as_ref()
                .map(|t| self.resolve_type_annotation(t))
                .unwrap_or(UNRESOLVED);
            let reg = self.alloc_register(ty);

            // Extract parameter name from pattern
            if let Pattern::Identifier(ident) = &param.pattern {
                let local_idx = self.allocate_local(ident.name);
                self.local_registers.insert(local_idx, reg.clone());

                // Track class type for parameters with class type annotations
                // so method calls can be statically resolved
                if let Some(type_ann) = &param.type_annotation {
                    if let Some(class_id) = self.try_extract_class_from_type(type_ann) {
                        self.variable_class_map.insert(ident.name, class_id);
                    }
                }
            }
            params.push(reg);
        }

        // Get return type
        let return_ty = func
            .return_type
            .as_ref()
            .map(|t| self.resolve_type_annotation(t))
            .unwrap_or(UNRESOLVED);

        // Create function
        let mut ir_func = IrFunction::new(name, params, return_ty);
        if self.emit_sourcemap {
            ir_func.source_span = func.span;
        }
        self.current_function = Some(ir_func);

        // Create entry block
        let entry_block = self.alloc_block();
        self.current_block = entry_block;
        self.current_function_mut()
            .add_block(BasicBlock::with_label(entry_block, "entry"));

        // Emit null-check + default-value for parameters with defaults
        self.emit_default_params(&func.params);

        // Lower function body
        for stmt in &func.body.statements {
            self.lower_stmt(stmt);
        }

        // Ensure the function ends with a return
        if !self.current_block_is_terminated() {
            self.set_terminator(Terminator::Return(None));
        }

        // Restore function depth
        self.function_depth -= 1;

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
        self.refcell_inner_types.clear();
        self.loop_captured_vars.clear();

        // Pre-scan to identify captured variables
        let stmts_owned: Vec<ast::Statement> = stmts.iter().map(|s| (*s).clone()).collect();
        let mut locals = self.collect_local_names(&stmts_owned);
        // Remove module-level globals — they use LoadGlobal/StoreGlobal, not locals
        locals.retain(|name| !self.module_var_globals.contains_key(name));
        self.scan_for_captured_vars(&stmts_owned, &[], &locals);

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

        // Lower statements first (so variable declarations like `let x = 0` are processed)
        for stmt in stmts {
            self.lower_stmt(stmt);
        }

        // Initialize decorators for all classes AFTER statements
        // This ensures variables referenced by decorators are already declared
        self.emit_decorator_initializations();

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

        // Check if class has //@@json annotation
        for annotation in &class.annotations {
            if annotation.tag == "json" {
                ir_class.json_serializable = true;
            }
        }

        // Get class ID from per-declaration map (safe even when names collide)
        let class_id = self.class_decl_ids.get(&class.span.start)
            .copied()
            .unwrap_or_else(|| *self.class_map.get(&class.name.name).unwrap());
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
                    lowerer: &Lowerer<'_>,
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

                    // Process JSON annotations for this field
                    let mut ir_field = IrField::new(field_name, ty, index);
                    ir_field.readonly = field.is_readonly;
                    for annotation in &field.annotations {
                        if annotation.tag == "json" {
                            if annotation.is_skip() {
                                ir_field.json_skip = true;
                            } else {
                                // Get the JSON field name (if different from struct field)
                                if let Some(json_name) = annotation.json_field_name() {
                                    ir_field.json_name = Some(json_name.to_string());
                                }
                                ir_field.json_omitempty = annotation.has_omitempty();
                            }
                        }
                    }

                    ir_class.add_field(ir_field);
                }
            }
        }

        // Add fields from constructor parameter properties (e.g., `constructor(public x: number)`)
        for member in &class.members {
            if let ast::ClassMember::Constructor(ctor) = member {
                for param in &ctor.params {
                    if param.visibility.is_some() {
                        if let ast::Pattern::Identifier(ident) = &param.pattern {
                            let field_name = self.interner.resolve(ident.name);
                            let ty = param
                                .type_annotation
                                .as_ref()
                                .map(|t| self.resolve_type_annotation(t))
                                .unwrap_or(TypeId::new(0));
                            let index = class_info
                                .as_ref()
                                .and_then(|info| {
                                    info.fields
                                        .iter()
                                        .find(|f| f.name == ident.name)
                                        .map(|f| f.index)
                                })
                                .unwrap_or(0);
                            let ir_field = IrField::new(field_name, ty, index);
                            ir_class.add_field(ir_field);
                        }
                    }
                }
                break;
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
                    // Reset capture state for this method scope
                    self.refcell_vars.clear();
                    self.refcell_registers.clear();
                    self.refcell_inner_types.clear();
                    self.loop_captured_vars.clear();

                    // Create parameter registers
                    let mut params = Vec::new();

                    if method.is_static {
                        // Static method - no 'this' parameter
                        self.current_class = None;
                        self.this_register = None;
                    } else {
                        // Instance method - 'this' is the first parameter
                        // Use the class's actual TypeId for correct dispatch
                        // (e.g., Array → ArrayLen for .length, string → StringLen)
                        let this_ty = self.type_ctx.lookup_named_type(name)
                            .unwrap_or(TypeId::new(0));
                        self.current_class = Some(class_id);
                        let this_reg = self.alloc_register(this_ty);
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
                            .unwrap_or(UNRESOLVED);
                        let reg = self.alloc_register(ty);

                        if let Pattern::Identifier(ident) = &param.pattern {
                            let local_idx = self.allocate_local(ident.name);
                            self.local_registers.insert(local_idx, reg.clone());

                            // Track class type for parameters with class type annotations
                            if let Some(type_ann) = &param.type_annotation {
                                if let Some(param_class_id) = self.try_extract_class_from_type(type_ann) {
                                    self.variable_class_map.insert(ident.name, param_class_id);
                                }
                            }
                        }
                        params.push(reg);
                    }

                    // Get return type
                    let return_ty = method
                        .return_type
                        .as_ref()
                        .map(|t| self.resolve_type_annotation(t))
                        .unwrap_or(UNRESOLVED);

                    // Create function with mangled name
                    let mut ir_func = IrFunction::new(&full_name, params, return_ty);
                    if self.emit_sourcemap {
                        ir_func.source_span = method.span;
                    }
                    self.current_function = Some(ir_func);

                    // Create entry block
                    let entry_block = self.alloc_block();
                    self.current_block = entry_block;
                    self.current_function_mut()
                        .add_block(BasicBlock::with_label(entry_block, "entry"));

                    // Pre-scan method body for captured variables
                    {
                        let mut method_locals = FxHashSet::default();
                        for param in &method.params {
                            collect_pattern_names(&param.pattern, &mut method_locals);
                        }
                        method_locals.extend(self.collect_local_names(&body.statements));
                        self.scan_for_captured_vars(&body.statements, &method.params, &method_locals);
                    }

                    // Emit null-check + default-value for parameters with defaults
                    self.emit_default_params(&method.params);

                    // Lower method body
                    for stmt in &body.statements {
                        self.lower_stmt(stmt);
                    }

                    // Ensure the function ends with a return
                    if !self.current_block_is_terminated() {
                        self.set_terminator(Terminator::Return(None));
                    }

                    // Get the function ID and add to pending methods
                    let method_name_str = self.interner.resolve(method.name.name);
                    let func_id = if method.is_static {
                        *self.static_method_map.get(&(class_id, method.name.name))
                            .unwrap_or_else(|| panic!(
                                "ICE: static method '{}::{}' not found in static_method_map (class_id={})",
                                name, method_name_str, class_id.as_u32()
                            ))
                    } else {
                        *self.method_map.get(&(class_id, method.name.name))
                            .unwrap_or_else(|| panic!(
                                "ICE: method '{}::{}' not found in method_map (class_id={})",
                                name, method_name_str, class_id.as_u32()
                            ))
                    };
                    let ir_func = self.current_function.take().unwrap();
                    self.pending_arrow_functions.push((func_id.as_u32(), ir_func));

                    // Add instance methods to the IR class vtable with slot index
                    if !method.is_static {
                        if let Some(&slot) = self.method_slot_map.get(&(class_id, method.name.name)) {
                            ir_class.add_method_with_slot(func_id, slot);
                        } else {
                            ir_class.add_method(func_id);
                        }
                    }

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
                // Reset capture state for constructor scope
                self.refcell_vars.clear();
                self.refcell_registers.clear();
                self.refcell_inner_types.clear();
                self.loop_captured_vars.clear();

                // Set current class context for 'this' handling
                self.current_class = Some(class_id);

                // Create parameter registers - 'this' is the first parameter
                let mut params = Vec::new();

                // Add 'this' as the first parameter
                // Reserve local slot 0 for 'this'
                let this_ty = self.type_ctx.lookup_named_type(name)
                    .unwrap_or(TypeId::new(0));
                let this_reg = self.alloc_register(this_ty);
                params.push(this_reg.clone());
                self.this_register = Some(this_reg);
                self.next_local = 1; // Explicit parameters start at slot 1

                // Add explicit parameters from constructor
                for param in &ctor.params {
                    let ty = param
                        .type_annotation
                        .as_ref()
                        .map(|t| self.resolve_type_annotation(t))
                        .unwrap_or(UNRESOLVED);
                    let reg = self.alloc_register(ty);

                    if let Pattern::Identifier(ident) = &param.pattern {
                        let local_idx = self.allocate_local(ident.name);
                        self.local_registers.insert(local_idx, reg.clone());
                    }
                    params.push(reg);
                }

                // Collect parameter property registers before params is moved
                let mut param_prop_regs: Vec<(Symbol, Register)> = Vec::new();
                for (i, param) in ctor.params.iter().enumerate() {
                    if param.visibility.is_some() {
                        if let ast::Pattern::Identifier(ident) = &param.pattern {
                            param_prop_regs.push((ident.name, params[i + 1].clone()));
                        }
                    }
                }

                // Constructors implicitly return void
                let return_ty = TypeId::new(0);

                // Create function with mangled name
                let mut ir_func = IrFunction::new(&full_name, params, return_ty);
                if self.emit_sourcemap {
                    ir_func.source_span = ctor.span;
                }
                self.current_function = Some(ir_func);

                // Create entry block
                let entry_block = self.alloc_block();
                self.current_block = entry_block;
                self.current_function_mut()
                    .add_block(BasicBlock::with_label(entry_block, "entry"));

                // Pre-scan constructor body for captured variables
                {
                    let mut ctor_locals = FxHashSet::default();
                    for param in &ctor.params {
                        collect_pattern_names(&param.pattern, &mut ctor_locals);
                    }
                    ctor_locals.extend(self.collect_local_names(&ctor.body.statements));
                    self.scan_for_captured_vars(&ctor.body.statements, &ctor.params, &ctor_locals);
                }

                // Emit null-check + default-value for constructor parameters with defaults
                self.emit_default_params(&ctor.params);

                // Emit field assignments for constructor parameter properties
                for (param_name, param_reg) in &param_prop_regs {
                    let field_name_str = self.interner.resolve(*param_name);
                    let all_fields = self.get_all_fields(class_id);
                    if let Some(fi) = all_fields.iter().find(|f| self.interner.resolve(f.name) == field_name_str) {
                        let field_idx = fi.index;
                        let this_reg = self.this_register.clone().unwrap();
                        self.emit(IrInstr::StoreField {
                            object: this_reg,
                            field: field_idx,
                            value: param_reg.clone(),
                        });
                    }
                }

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

    /// Lower a type alias declaration
    ///
    /// Only processes object types (struct-like type aliases).
    /// Type aliases are automatically JSON decodable when they represent object types.
    fn lower_type_alias(&mut self, type_alias: &ast::TypeAliasDecl) -> Option<IrTypeAlias> {
        // Only process object types
        if let ast::Type::Object(obj_type) = &type_alias.type_annotation.ty {
            let name = self.interner.resolve(type_alias.name.name);
            let mut ir_type_alias = IrTypeAlias::new(name);

            // Process fields from the object type
            for member in &obj_type.members {
                if let ast::ObjectTypeMember::Property(prop) = member {
                    let field_name = self.interner.resolve(prop.name.name);
                    let ty = self.resolve_type_annotation(&prop.ty);

                    let mut field = IrTypeAliasField::new(field_name, ty, prop.optional);

                    // Process JSON annotations for this field
                    for annotation in &prop.annotations {
                        if annotation.tag == "json" {
                            if annotation.is_skip() {
                                field.json_skip = true;
                            } else {
                                if let Some(json_name) = annotation.json_field_name() {
                                    field.json_name = Some(json_name.to_string());
                                }
                                field.json_omitempty = annotation.has_omitempty();
                            }
                        }
                    }

                    ir_type_alias.add_field(field);
                }
            }

            Some(ir_type_alias)
        } else {
            // Not an object type, skip
            None
        }
    }

    /// Allocate a new register
    fn alloc_register(&mut self, ty: TypeId) -> Register {
        let id = RegisterId::new(self.next_register);
        self.next_register += 1;
        Register::new(id, ty)
    }

    /// Emit null-check and default-value assignment for function parameters with defaults.
    /// Must be called after entry block creation and parameter registration,
    /// before lowering the function body.
    fn emit_default_params(&mut self, params: &[ast::Parameter]) {
        for param in params {
            if let Some(ref default_expr) = param.default_value {
                if let Pattern::Identifier(ident) = &param.pattern {
                    if let Some(&local_idx) = self.local_map.get(&ident.name) {
                        // Load the parameter value
                        let param_reg = self.alloc_register(UNRESOLVED);
                        self.emit(IrInstr::LoadLocal {
                            dest: param_reg.clone(),
                            index: local_idx,
                        });

                        // Branch on null
                        let default_block = self.alloc_block();
                        let continue_block = self.alloc_block();
                        self.set_terminator(Terminator::BranchIfNull {
                            value: param_reg,
                            null_block: default_block,
                            not_null_block: continue_block,
                        });

                        // Default block: evaluate default expression and store
                        self.current_function_mut()
                            .add_block(BasicBlock::with_label(default_block, "param.default"));
                        self.current_block = default_block;
                        let default_val = self.lower_expr(default_expr);
                        self.emit(IrInstr::StoreLocal {
                            index: local_idx,
                            value: default_val.clone(),
                        });
                        self.local_registers.insert(local_idx, default_val);
                        self.set_terminator(Terminator::Jump(continue_block));

                        // Continue block
                        self.current_function_mut()
                            .add_block(BasicBlock::with_label(continue_block, "param.cont"));
                        self.current_block = continue_block;
                    }
                }
            }
        }
    }

    /// Find a method's vtable slot in parent class hierarchy
    fn find_parent_method_slot(&self, parent_class: Option<ClassId>, method_name: Symbol) -> Option<u16> {
        let mut current = parent_class;
        while let Some(class_id) = current {
            if let Some(&slot) = self.method_slot_map.get(&(class_id, method_name)) {
                return Some(slot);
            }
            current = self.class_info_map.get(&class_id).and_then(|info| info.parent_class);
        }
        None
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

    /// Add an instruction to the current block.
    /// When sourcemap is enabled, automatically attaches the current source span.
    fn emit(&mut self, instr: IrInstr) {
        if self.emit_sourcemap {
            let span = self.current_span;
            self.current_block_mut().add_instr_spanned(instr, span);
        } else {
            self.current_block_mut().add_instr(instr);
        }
    }

    /// Set the terminator for the current block.
    /// When sourcemap is enabled, automatically attaches the current source span.
    fn set_terminator(&mut self, term: Terminator) {
        if self.emit_sourcemap {
            let span = self.current_span;
            self.current_block_mut().set_terminator_spanned(term, span);
        } else {
            self.current_block_mut().set_terminator(term);
        }
    }

    /// Update the current source span (call at statement/expression boundaries)
    fn set_span(&mut self, span: &Span) {
        self.current_span = *span;
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

    /// Emit decorator initialization code for all classes
    ///
    /// Decorator application order (per spec):
    /// 1. Field decorators (declaration order)
    /// 2. Method decorators (declaration order)
    /// 3. Class decorators (bottom-to-top for multiple decorators)
    ///
    /// For each decorator, we:
    /// 1. Lower the decorator expression (could be identifier or call)
    /// 2. Call the decorator with the appropriate target
    /// 3. Register the decorator application with metadata
    fn emit_decorator_initializations(&mut self) {
        use crate::compiler::native_id::{
            REGISTER_CLASS_DECORATOR, REGISTER_FIELD_DECORATOR, REGISTER_METHOD_DECORATOR,
            REGISTER_PARAMETER_DECORATOR,
        };

        // Collect all decorator applications (clone to avoid borrow issues)
        // Structure: (class_id, class_name, class_decorators, field_decorators, method_decorators, parameter_decorators)
        #[allow(clippy::type_complexity)] // Tuple groups related decorator info; a dedicated struct would add boilerplate for a single use-site.
        let decorator_apps: Vec<(
            ClassId,
            String,
            Vec<DecoratorInfo>,
            Vec<FieldDecoratorInfo>,
            Vec<MethodDecoratorInfo>,
            Vec<ParameterDecoratorInfo>,
        )> = self
            .class_info_map
            .iter()
            .filter_map(|(&class_id, info)| {
                // Only process classes that have decorators
                if info.class_decorators.is_empty()
                    && info.field_decorators.is_empty()
                    && info.method_decorators.is_empty()
                    && info.parameter_decorators.is_empty()
                {
                    return None;
                }

                // Get class name from class_map (reverse lookup)
                let class_name = self
                    .class_map
                    .iter()
                    .find(|(_, &id)| id == class_id)
                    .map(|(sym, _)| self.interner.resolve(*sym).to_string())
                    .unwrap_or_else(|| format!("class_{}", class_id.as_u32()));

                Some((
                    class_id,
                    class_name,
                    info.class_decorators.clone(),
                    info.field_decorators.clone(),
                    info.method_decorators.clone(),
                    info.parameter_decorators.clone(),
                ))
            })
            .collect();

        // Process each class's decorators
        for (class_id, class_name, class_decorators, field_decorators, method_decorators, parameter_decorators) in
            decorator_apps
        {
            let class_id_val = class_id.as_u32();

            // 1. Process parameter decorators first (applied before method is decorated)
            for param_dec in &parameter_decorators {
                for dec_info in &param_dec.decorators {
                    self.emit_decorator_call(
                        DecoratorTarget::Parameter {
                            class_id: class_id_val,
                            method_name: param_dec.method_name.clone(),
                            param_index: param_dec.param_index,
                        },
                        &dec_info.expression,
                        REGISTER_PARAMETER_DECORATOR,
                    );
                }
            }

            // 2. Process field decorators (declaration order)
            for field_dec in &field_decorators {
                let field_name = self.interner.resolve(field_dec.field_name).to_string();
                for dec_info in &field_dec.decorators {
                    self.emit_decorator_call(
                        DecoratorTarget::Field {
                            class_id: class_id_val,
                            field_name: field_name.clone(),
                        },
                        &dec_info.expression,
                        REGISTER_FIELD_DECORATOR,
                    );
                }
            }

            // 3. Process method decorators (declaration order)
            for method_dec in &method_decorators {
                let method_name = self.interner.resolve(method_dec.method_name).to_string();
                for dec_info in &method_dec.decorators {
                    self.emit_decorator_call(
                        DecoratorTarget::Method {
                            class_id: class_id_val,
                            method_name: method_name.clone(),
                        },
                        &dec_info.expression,
                        REGISTER_METHOD_DECORATOR,
                    );
                }
            }

            // 4. Process class decorators (bottom-to-top = reverse order in list)
            for dec_info in class_decorators.iter().rev() {
                self.emit_decorator_call(
                    DecoratorTarget::Class {
                        class_id: class_id_val,
                        class_name: class_name.clone(),
                    },
                    &dec_info.expression,
                    REGISTER_CLASS_DECORATOR,
                );
            }
        }
    }

    /// Emit code to call a single decorator
    fn emit_decorator_call(
        &mut self,
        target: DecoratorTarget,
        decorator_expr: &Expression,
        registration_native_id: u16,
    ) {
        // Get decorator name for registration
        let decorator_name = self.get_decorator_name(decorator_expr);

        // Create class_id register
        let class_id_val = match &target {
            DecoratorTarget::Class { class_id, .. } => *class_id,
            DecoratorTarget::Method { class_id, .. } => *class_id,
            DecoratorTarget::Field { class_id, .. } => *class_id,
            DecoratorTarget::Parameter { class_id, .. } => *class_id,
        };
        let class_id_reg = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::Assign {
            dest: class_id_reg.clone(),
            value: IrValue::Constant(IrConstant::I32(class_id_val as i32)),
        });

        // Determine how to call the decorator based on the expression type
        // There are 3 cases:
        // 1. Direct function identifier (@Injectable) - use IrInstr::Call
        // 2. Factory call (@Controller("/api")) - lower the call, then CallClosure on result
        // 3. Local variable containing closure - load and CallClosure

        // Check if decorator is a direct function reference (identifier in function_map)
        let direct_func_id = match decorator_expr {
            Expression::Identifier(ident) => self.function_map.get(&ident.name).copied(),
            _ => None,
        };

        // Build the arguments based on target type
        let args = match &target {
            DecoratorTarget::Class { .. } => vec![class_id_reg.clone()],
            DecoratorTarget::Method { method_name, .. } => {
                let method_name_reg = self.alloc_register(TypeId::new(1));
                self.emit(IrInstr::Assign {
                    dest: method_name_reg.clone(),
                    value: IrValue::Constant(IrConstant::String(method_name.clone())),
                });
                vec![class_id_reg.clone(), method_name_reg]
            }
            DecoratorTarget::Field { field_name, .. } => {
                let field_name_reg = self.alloc_register(TypeId::new(1));
                self.emit(IrInstr::Assign {
                    dest: field_name_reg.clone(),
                    value: IrValue::Constant(IrConstant::String(field_name.clone())),
                });
                vec![class_id_reg.clone(), field_name_reg]
            }
            DecoratorTarget::Parameter { method_name, param_index, .. } => {
                let method_name_reg = self.alloc_register(TypeId::new(1));
                self.emit(IrInstr::Assign {
                    dest: method_name_reg.clone(),
                    value: IrValue::Constant(IrConstant::String(method_name.clone())),
                });
                let param_index_reg = self.alloc_register(TypeId::new(0));
                self.emit(IrInstr::Assign {
                    dest: param_index_reg.clone(),
                    value: IrValue::Constant(IrConstant::I32(*param_index as i32)),
                });
                vec![class_id_reg.clone(), method_name_reg, param_index_reg]
            }
        };

        // Emit the decorator call
        if let Some(func_id) = direct_func_id {
            // Case 1: Direct function call - use IrInstr::Call
            let result_reg = self.alloc_register(TypeId::new(0));
            self.emit(IrInstr::Call {
                dest: Some(result_reg),
                func: func_id,
                args,
            });
        } else if let Expression::Call(_) = decorator_expr {
            // Case 2: Factory call - lower the factory call, then CallClosure on the result
            // The factory returns a closure that is the actual decorator
            let decorator_closure = self.lower_expr(decorator_expr);
            let result_reg = self.alloc_register(TypeId::new(0));
            self.emit(IrInstr::CallClosure {
                dest: Some(result_reg),
                closure: decorator_closure,
                args,
            });
        } else {
            // Case 3: Local variable or other expression - lower and use CallClosure
            let decorator_reg = self.lower_expr(decorator_expr);
            let result_reg = self.alloc_register(TypeId::new(0));
            self.emit(IrInstr::CallClosure {
                dest: Some(result_reg),
                closure: decorator_reg,
                args,
            });
        }

        // Register the decorator application in the metadata store
        let dec_name_reg = self.alloc_register(TypeId::new(1)); // String type
        self.emit(IrInstr::Assign {
            dest: dec_name_reg.clone(),
            value: IrValue::Constant(IrConstant::String(decorator_name)),
        });

        // Emit registration native call based on target type
        match &target {
            DecoratorTarget::Class { .. } => {
                // registerClassDecorator(classId, decoratorName)
                self.emit(IrInstr::NativeCall {
                    dest: None,
                    native_id: registration_native_id,
                    args: vec![class_id_reg, dec_name_reg],
                });
            }
            DecoratorTarget::Method { method_name, .. } => {
                // registerMethodDecorator(classId, methodName, decoratorName)
                let method_name_reg = self.alloc_register(TypeId::new(1));
                self.emit(IrInstr::Assign {
                    dest: method_name_reg.clone(),
                    value: IrValue::Constant(IrConstant::String(method_name.clone())),
                });
                self.emit(IrInstr::NativeCall {
                    dest: None,
                    native_id: registration_native_id,
                    args: vec![class_id_reg, method_name_reg, dec_name_reg],
                });
            }
            DecoratorTarget::Field { field_name, .. } => {
                // registerFieldDecorator(classId, fieldName, decoratorName)
                let field_name_reg = self.alloc_register(TypeId::new(1));
                self.emit(IrInstr::Assign {
                    dest: field_name_reg.clone(),
                    value: IrValue::Constant(IrConstant::String(field_name.clone())),
                });
                self.emit(IrInstr::NativeCall {
                    dest: None,
                    native_id: registration_native_id,
                    args: vec![class_id_reg, field_name_reg, dec_name_reg],
                });
            }
            DecoratorTarget::Parameter { method_name, param_index, .. } => {
                // registerParameterDecorator(classId, methodName, paramIndex, decoratorName)
                let method_name_reg = self.alloc_register(TypeId::new(1));
                self.emit(IrInstr::Assign {
                    dest: method_name_reg.clone(),
                    value: IrValue::Constant(IrConstant::String(method_name.clone())),
                });
                let param_index_reg = self.alloc_register(TypeId::new(0));
                self.emit(IrInstr::Assign {
                    dest: param_index_reg.clone(),
                    value: IrValue::Constant(IrConstant::I32(*param_index as i32)),
                });
                self.emit(IrInstr::NativeCall {
                    dest: None,
                    native_id: registration_native_id,
                    args: vec![class_id_reg, method_name_reg, param_index_reg, dec_name_reg],
                });
            }
        }
    }

    /// Extract the decorator name from an expression for registration
    fn get_decorator_name(&self, expr: &Expression) -> String {
        match expr {
            Expression::Identifier(ident) => self.interner.resolve(ident.name).to_string(),
            Expression::Call(call) => {
                // For decorator factories like @Controller("/api"), extract "Controller"
                self.get_decorator_name(&call.callee)
            }
            Expression::Member(member) => {
                // For @ns.Decorator, return "ns.Decorator"
                let obj_name = self.get_decorator_name(&member.object);
                let prop_name = self.interner.resolve(member.property.name);
                format!("{}.{}", obj_name, prop_name)
            }
            _ => "unknown".to_string(),
        }
    }

    /// Resolve a type annotation to a TypeId
    fn resolve_type_annotation(&self, ty: &ast::TypeAnnotation) -> TypeId {
        use crate::parser::types::ty::{Type as TyType, PrimitiveType as TyPrim};

        match &ty.ty {
            ast::Type::Primitive(prim) => {
                let ty_prim = match prim {
                    ast::PrimitiveType::Number => TyPrim::Number,
                    ast::PrimitiveType::Int => TyPrim::Int,
                    ast::PrimitiveType::String => TyPrim::String,
                    ast::PrimitiveType::Boolean => TyPrim::Boolean,
                    ast::PrimitiveType::Null => TyPrim::Null,
                    ast::PrimitiveType::Void => TyPrim::Void,
                };
                self.type_ctx.lookup(&TyType::Primitive(ty_prim)).unwrap_or(UNRESOLVED)
            }
            ast::Type::Reference(type_ref) => {
                let name = self.interner.resolve(type_ref.name.name);
                // Check active type parameter substitutions first (during generic specialization)
                if let Some(&concrete_ty) = self.type_param_substitutions.get(name) {
                    return concrete_ty;
                }
                self.type_ctx.lookup_named_type(name).unwrap_or(UNRESOLVED)
            }
            // Array types: string[], number[], T[]
            // Tuple types: [string, number] — arrays at runtime
            ast::Type::Array(_) | ast::Type::Tuple(_) => {
                self.type_ctx.lookup_named_type("Array").unwrap_or(UNRESOLVED)
            }
            // Union types: resolve to dominant non-null member
            ast::Type::Union(union) => {
                let null_id = self.type_ctx.lookup(&TyType::Primitive(TyPrim::Null));
                let mut resolved = UNRESOLVED;
                for member in &union.types {
                    let member_ty = self.resolve_type_annotation(member);
                    if null_id != Some(member_ty) {
                        if resolved == UNRESOLVED {
                            resolved = member_ty;
                        } else if resolved != member_ty {
                            return UNRESOLVED;
                        }
                    }
                }
                resolved
            }
            // Literal types → their primitive
            ast::Type::StringLiteral(_) => {
                self.type_ctx.lookup(&TyType::Primitive(TyPrim::String)).unwrap_or(UNRESOLVED)
            }
            ast::Type::NumberLiteral(_) => {
                self.type_ctx.lookup(&TyType::Primitive(TyPrim::Number)).unwrap_or(UNRESOLVED)
            }
            ast::Type::BooleanLiteral(_) => {
                self.type_ctx.lookup(&TyType::Primitive(TyPrim::Boolean)).unwrap_or(UNRESOLVED)
            }
            // Parenthesized: unwrap
            ast::Type::Parenthesized(inner) => self.resolve_type_annotation(inner),
            // Function, Object, Intersection, Typeof → UNRESOLVED
            _ => UNRESOLVED,
        }
    }

    /// Extract a ClassId from a type annotation, handling both direct class references
    /// and nullable unions (e.g., `Node | null` → Node's ClassId).
    fn try_extract_class_from_type(&self, type_ann: &ast::TypeAnnotation) -> Option<ClassId> {
        match &type_ann.ty {
            ast::Type::Reference(type_ref) => {
                self.class_map.get(&type_ref.name.name).copied()
            }
            ast::Type::Union(union_type) => {
                let mut class_id = None;
                for member in &union_type.types {
                    match &member.ty {
                        ast::Type::Primitive(ast::PrimitiveType::Null) => {} // skip null
                        ast::Type::Reference(type_ref) => {
                            if class_id.is_some() {
                                return None; // multiple class refs — ambiguous
                            }
                            class_id = self.class_map.get(&type_ref.name.name).copied();
                        }
                        _ => return None, // non-null, non-class member
                    }
                }
                class_id
            }
            _ => None,
        }
    }

    /// Get JSON field information from a type for specialized decode
    ///
    /// Returns a list of (json_key, field_name, type_id) tuples if the type
    /// is an object type (inline or via type alias).
    ///
    /// For the MVP, this returns None for type references (type aliases)
    /// since we don't yet have the type alias AST stored for lookup.
    /// Only inline object types are supported for now.
    fn get_json_field_info(&self, ty: &ast::Type) -> Option<Vec<JsonFieldInfo>> {
        match ty {
            ast::Type::Object(obj_type) => {
                // Inline object type: { name: string; age: number; }
                let mut fields = Vec::new();
                for member in &obj_type.members {
                    if let ast::ObjectTypeMember::Property(prop) = member {
                        let field_name = self.interner.resolve(prop.name.name).to_string();

                        // Check for //@@json annotation to get custom JSON key
                        let mut json_key = field_name.clone();
                        let mut skip = false;

                        for annotation in &prop.annotations {
                            if annotation.tag == "json" {
                                if annotation.is_skip() {
                                    skip = true;
                                } else if let Some(name) = annotation.json_field_name() {
                                    json_key = name.to_string();
                                }
                            }
                        }

                        if !skip {
                            let type_id = self.resolve_type_annotation(&prop.ty);
                            fields.push(JsonFieldInfo {
                                json_key,
                                field_name,
                                type_id,
                                optional: prop.optional,
                            });
                        }
                    }
                }
                Some(fields)
            }
            ast::Type::Reference(_type_ref) => {
                // Type reference: look up type alias
                // For the MVP, we fall back to None and use JSON.parse
                // TODO: Store type alias AST for lookup during lowering
                None
            }
            _ => None,
        }
    }

    /// Emit specialized JSON decode for an object type with known fields
    ///
    /// This generates a native call with field metadata that the VM
    /// uses to decode JSON directly into a typed object.
    fn emit_json_decode_with_fields(
        &mut self,
        dest: Register,
        args: Vec<Register>,
        fields: Vec<JsonFieldInfo>,
    ) -> Register {
        use crate::compiler::native_id::JSON_DECODE_OBJECT;

        // For the specialized decode, we pass:
        // - arg[0]: JSON string
        // - arg[1]: field count as i32
        // - arg[2..]: json_key strings for each field
        //
        // The NativeCall uses Register args, so we load constants into registers.

        let mut decode_args = args.clone();

        // Add field count as i32
        let count_reg = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::Assign {
            dest: count_reg.clone(),
            value: IrValue::Constant(IrConstant::I32(fields.len() as i32)),
        });
        decode_args.push(count_reg);

        // Add field info (json_key as string for each field)
        for field in &fields {
            let key_reg = self.alloc_register(TypeId::new(1));
            self.emit(IrInstr::Assign {
                dest: key_reg.clone(),
                value: IrValue::Constant(IrConstant::String(field.json_key.clone())),
            });
            decode_args.push(key_reg);
        }

        self.emit(IrInstr::NativeCall {
            dest: Some(dest.clone()),
            native_id: JSON_DECODE_OBJECT,
            args: decode_args,
        });

        // Track field layout for property access resolution on the decoded object
        let field_layout: Vec<(String, usize)> = fields
            .iter()
            .enumerate()
            .map(|(i, f)| (f.field_name.clone(), i))
            .collect();
        self.register_object_fields.insert(dest.id, field_layout);

        dest
    }

}

/// Information about a JSON field for specialized decode
#[derive(Debug, Clone)]
pub struct JsonFieldInfo {
    /// The key name in JSON (may differ from field name due to //@@json annotation)
    pub json_key: String,
    /// The field name in the target type
    pub field_name: String,
    /// The type of the field
    pub type_id: TypeId,
    /// Whether the field is optional
    pub optional: bool,
}

#[cfg(test)]
mod decorator_tests {
    use super::*;
    use crate::parser::{Parser, TypeContext};

    fn lower_source(source: &str) -> IrModule {
        let parser = Parser::new(source).expect("lexer error");
        let (module, interner) = parser.parse().expect("parse error");
        let type_ctx = TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        lowerer.lower_module(&module)
    }

    #[test]
    fn test_class_decorator_collection() {
        // Test that class decorators are collected during lowering
        let source = r#"
            function Injectable(): void {}

            @Injectable
            class Service {
                name: string;
            }

            // Top-level statement to trigger main function
            let x = 1;
        "#;

        let module = lower_source(source);

        // Should have 2 functions: Injectable and main
        assert!(module.function_count() >= 2, "Should have at least Injectable and main functions, got {}", module.function_count());
        // Should have 1 class: Service
        assert_eq!(module.class_count(), 1, "Should have 1 class");
    }

    #[test]
    fn test_method_decorator_collection() {
        // Test that method decorators are collected
        // Note: Need top-level statement to create main function
        let source = r#"
            function Log(): void {}

            class Api {
                @Log
                getUsers(): void {}
            }

            let x = 1;
        "#;

        let module = lower_source(source);

        // Should have 3 functions: Log, Api::getUsers, and main
        assert!(module.function_count() >= 3, "Should have Log, getUsers, and main functions, got {}", module.function_count());
        assert_eq!(module.class_count(), 1, "Should have 1 class");
    }

    #[test]
    fn test_field_decorator_collection() {
        // Test that field decorators are collected
        let source = r#"
            function Column(): void {}

            class User {
                @Column
                name: string;
            }

            let x = 1;
        "#;

        let module = lower_source(source);

        // Should have functions including Column and main
        assert!(module.function_count() >= 2, "Should have Column and main functions, got {}", module.function_count());
        assert_eq!(module.class_count(), 1, "Should have 1 class");
    }

    #[test]
    fn test_multiple_decorators() {
        // Test multiple decorators on same element
        let source = r#"
            function A(): void {}
            function B(): void {}
            function C(): void {}

            @A
            @B
            @C
            class Foo {}

            let x = 1;
        "#;

        let module = lower_source(source);

        // Should have 5 functions: A, B, C, main
        assert!(module.function_count() >= 4, "Should have A, B, C, and main functions, got {}", module.function_count());
        assert_eq!(module.class_count(), 1, "Should have 1 class");
    }

    #[test]
    fn test_decorator_factory_expression() {
        // Test decorator factory syntax
        let source = r#"
            function Controller(path: string): void {}

            @Controller("/api")
            class Api {}

            let x = 1;
        "#;

        let module = lower_source(source);

        // Should have Controller factory and main
        assert!(module.function_count() >= 2, "Should have Controller and main functions, got {}", module.function_count());
        assert_eq!(module.class_count(), 1, "Should have 1 class");
    }

    #[test]
    fn test_get_decorator_name_from_parsed_code() {
        // Test decorator name extraction via actual parsing
        let source = r#"
            function Injectable(): void {}

            @Injectable
            class Service {}

            let x = 1;
        "#;

        let module = lower_source(source);

        // Verify module was lowered successfully with decorator
        assert_eq!(module.class_count(), 1, "Should have Service class");
        // The main function should have decorator initialization code
        let main_func = module.get_function_by_name("main");
        assert!(main_func.is_some(), "Should have main function with decorator init");
    }

    #[test]
    fn test_class_with_all_decorator_types() {
        // Test a class with class, method, and field decorators
        let source = r#"
            function Entity(): void {}
            function Column(): void {}
            function Validate(): void {}

            @Entity
            class User {
                @Column
                name: string;

                @Validate
                save(): void {}
            }

            let x = 1;
        "#;

        let module = lower_source(source);

        // Should have Entity, Column, Validate, User::save, and main
        assert!(module.function_count() >= 5, "Should have all decorator functions plus method and main, got {}", module.function_count());
        assert_eq!(module.class_count(), 1, "Should have 1 class");
    }

    #[test]
    fn test_nested_decorator_factories() {
        // Test decorator factory with multiple arguments
        let source = r#"
            function Route(method: string, path: string): void {}

            class Api {
                @Route("GET", "/users")
                getUsers(): void {}
            }

            let x = 1;
        "#;

        let module = lower_source(source);

        // Should have Route, Api::getUsers, and main
        assert!(module.function_count() >= 3, "Should have Route, getUsers, and main functions, got {}", module.function_count());
        assert_eq!(module.class_count(), 1, "Should have 1 class");
    }
}
