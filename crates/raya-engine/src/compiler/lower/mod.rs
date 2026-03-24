//! AST to IR Lowering
//!
//! Converts the type-checked AST into the IR representation.

mod class_methods;
mod control_flow;
mod expr;
mod stmt;

use crate::compiler::ir::{
    BasicBlock, BasicBlockId, BinaryOp, FunctionId, IrClass, IrConstant, IrField, IrFunction,
    IrInstr, IrMethodKind, IrModule, IrTypeAlias, IrTypeAliasField, IrValue, NominalTypeId,
    Register, RegisterId, Terminator, TypeAliasId,
};
use crate::parser::ast::{
    self, walk_arrow_function, walk_block_statement, walk_expression, walk_function_decl,
    walk_statement, ExportDecl, Expression, Pattern, Statement, VariableKind, Visitor,
};
use crate::parser::token::Span;
use crate::parser::{Interner, Symbol, Type, TypeContext, TypeId};
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
pub(super) const MUTEX_TYPE_ID: u32 = TypeContext::MUTEX_TYPE_ID;
pub(super) const TASK_TYPE_ID: u32 = TypeContext::TASK_TYPE_ID;
pub(super) const CHANNEL_TYPE_ID: u32 = TypeContext::CHANNEL_TYPE_ID;
pub(super) const MAP_TYPE_ID: u32 = TypeContext::MAP_TYPE_ID;
pub(super) const SET_TYPE_ID: u32 = TypeContext::SET_TYPE_ID;
pub(super) const JSON_TYPE_ID: u32 = TypeContext::JSON_TYPE_ID;
pub(super) const JSON_ARRAY_TYPE_ID: u32 = TypeContext::JSON_ARRAY_TYPE_ID;
pub(super) const JSON_OBJECT_TYPE_ID: u32 = TypeContext::JSON_OBJECT_TYPE_ID;
pub(super) const INT_TYPE_ID: u32 = TypeContext::INT_TYPE_ID;
pub(super) const BOOL_TYPE_ID: u32 = TypeContext::BOOLEAN_TYPE_ID;
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

struct GeneratorRequirementCollector {
    requires_snapshot: bool,
}

impl Visitor for GeneratorRequirementCollector {
    fn visit_statement(&mut self, stmt: &Statement) {
        if let Statement::Yield(yield_stmt) = stmt {
            if yield_stmt.is_delegate {
                self.requires_snapshot = true;
                return;
            }
        }
        walk_statement(self, stmt);
    }

    fn visit_function_decl(&mut self, _decl: &ast::FunctionDecl) {}
    fn visit_function_expression(&mut self, _func: &ast::FunctionExpression) {}
    fn visit_arrow_function(&mut self, _func: &ast::ArrowFunction) {}
    fn visit_class_decl(&mut self, _decl: &ast::ClassDecl) {}
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

struct ArgumentsUseCollector<'a> {
    interner: &'a Interner,
    found: Option<Symbol>,
}

impl<'a> Visitor for ArgumentsUseCollector<'a> {
    fn visit_identifier(&mut self, id: &ast::Identifier) {
        if self.found.is_none() && self.interner.resolve(id.name) == "arguments" {
            self.found = Some(id.name);
        }
    }

    fn visit_function_decl(&mut self, _decl: &ast::FunctionDecl) {}

    fn visit_function_expression(&mut self, _func: &ast::FunctionExpression) {}

    fn visit_class_decl(&mut self, _decl: &ast::ClassDecl) {}

    fn visit_arrow_function(&mut self, func: &ast::ArrowFunction) {
        if self.found.is_some() || scope_binds_arguments_in_params(&func.params, self.interner) {
            return;
        }

        for param in &func.params {
            if let Some(default_expr) = &param.default_value {
                self.visit_expression(default_expr);
                if self.found.is_some() {
                    return;
                }
            }
        }

        match &func.body {
            ast::ArrowBody::Expression(expr) => self.visit_expression(expr),
            ast::ArrowBody::Block(block) => {
                if scope_binds_arguments_in_statements(&block.statements, self.interner) {
                    return;
                }
                for stmt in &block.statements {
                    self.visit_statement(stmt);
                    if self.found.is_some() {
                        return;
                    }
                }
            }
        }
    }
}

fn scope_binds_arguments_in_params(params: &[ast::Parameter], interner: &Interner) -> bool {
    let mut names = FxHashSet::default();
    for param in params {
        collect_pattern_names(&param.pattern, &mut names);
    }
    names
        .iter()
        .any(|&symbol| interner.resolve(symbol) == "arguments")
}

fn scope_binds_arguments_in_statements(stmts: &[ast::Statement], interner: &Interner) -> bool {
    let mut locals = FxHashSet::default();
    collect_block_local_names(stmts, &mut locals);
    locals
        .iter()
        .any(|&symbol| interner.resolve(symbol) == "arguments")
}

/// Builds a fallback index for expression types keyed by source span.
///
/// The primary `expr_types` map is pointer-based (AST node identity). Some
/// lowering paths clone expressions (e.g. decorators), which invalidates pointer
/// lookups. Span lookup recovers type info for those cloned nodes.
struct ExprTypeSpanCollector<'a> {
    expr_types: &'a FxHashMap<usize, TypeId>,
    by_span: &'a mut FxHashMap<(usize, usize), TypeId>,
}

impl<'a> Visitor for ExprTypeSpanCollector<'a> {
    fn visit_expression(&mut self, expr: &Expression) {
        let expr_id = expr as *const _ as usize;
        if let Some(ty) = self.expr_types.get(&expr_id).copied() {
            let span = expr.span();
            self.by_span.entry((span.start, span.end)).or_insert(ty);
        }
        walk_expression(self, expr);
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
    class_type: Option<NominalTypeId>,
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
    /// Source-level method kind (normal/getter/setter)
    kind: ast::MethodKind,
    /// Visibility for member access semantics.
    visibility: ast::Visibility,
}

/// Information about a constructor parameter (for default value handling)
#[derive(Clone)]
struct ConstructorParamInfo {
    /// Default value expression (if any)
    default_value: Option<Expression>,
}

#[derive(Clone)]
struct PendingConstructorPrologue {
    nominal_type_id: NominalTypeId,
    this_reg: Register,
    param_properties: Vec<(u16, Register)>,
}

/// Information about a static field
#[derive(Clone)]
struct StaticFieldInfo {
    /// Field name (symbol)
    name: Symbol,
    /// Owning class runtime identity
    nominal_type_id: NominalTypeId,
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
    /// Source-level method kind
    kind: ast::MethodKind,
}

#[derive(Clone)]
struct RuntimeClassMethodElement {
    key: ast::PropertyKey,
    func_id: FunctionId,
    kind: ast::MethodKind,
    is_static: bool,
    order: usize,
}

/// Materialized outer-scope binding for class-method environment bridging.
#[derive(Clone, Copy)]
struct MethodEnvBinding {
    /// Dedicated global slot used by method lowering.
    global_idx: u16,
    /// Whether the bridged value is a RefCell pointer.
    is_refcell: bool,
}

/// Information about a decorator application
#[derive(Clone)]
struct DecoratorInfo {
    /// The decorator expression (e.g., `@Injectable` or `@Controller("/api")`)
    expression: Expression,
    /// Type checker result for the original decorator expression.
    /// We store this because `expression` is cloned and pointer-based expr-type
    /// lookup (`get_expr_type`) would otherwise lose the original mapping.
    expr_type: TypeId,
}

/// Target of a decorator (used during code generation)
enum DecoratorTarget {
    /// Class decorator - applied to the class itself
    Class {
        nominal_type_id: u32,
        class_name: String,
    },
    /// Method decorator - applied to a specific method
    Method {
        nominal_type_id: u32,
        class_name: String,
        method_name: String,
    },
    /// Field decorator - applied to a specific field
    Field {
        nominal_type_id: u32,
        class_name: String,
        field_name: String,
    },
    /// Parameter decorator - applied to a specific parameter
    Parameter {
        nominal_type_id: u32,
        class_name: String,
        method_name: String,
        param_index: u32,
    },
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
    /// Static initializer blocks
    static_blocks: Vec<ast::BlockStatement>,
    /// Static methods
    static_methods: Vec<StaticMethodInfo>,
    /// Runtime class method/accessor publication order.
    runtime_method_elements: Vec<RuntimeClassMethodElement>,
    /// Parent class (for inheritance)
    parent_class: Option<NominalTypeId>,
    /// Runtime parent class name for ambient/imported parents not declared locally.
    parent_runtime_name: Option<String>,
    /// Source-level symbol for the parent constructor reference.
    parent_constructor_symbol: Option<Symbol>,
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
    /// Optional label for this loop (for labeled break/continue)
    label: Option<Symbol>,
    /// Block to jump to for break
    break_target: BasicBlockId,
    /// Block to jump to for continue
    continue_target: BasicBlockId,
    /// Active iterator for protocol-based loops that need IteratorClose on abrupt exit.
    iterator_record: Option<Register>,
    /// Depth of try_finally_stack when this loop started
    /// (used to know which finally blocks to inline on break/continue)
    try_finally_depth: usize,
}

#[derive(Clone)]
struct LabelContext {
    label: Symbol,
    break_target: BasicBlockId,
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
    /// Local indices declared callable via type annotations.
    callable_local_hints: FxHashSet<u16>,
    /// Symbols declared callable via type annotations.
    callable_symbol_hints: FxHashSet<Symbol>,
    /// Next local index (for both named and anonymous locals)
    next_local: u16,
    /// Function name to ID mapping
    function_map: FxHashMap<Symbol, FunctionId>,
    /// Per-declaration function ID (keyed by span start position, used for nested fn hoisting)
    function_decl_ids: FxHashMap<usize, FunctionId>,
    /// Function declaration spans already materialized during JS hoist lowering.
    hoisted_function_decl_spans: FxHashSet<usize>,
    /// Set of async function IDs (functions that should be spawned as Tasks)
    async_functions: FxHashSet<FunctionId>,
    /// Class name to ID mapping (last class registered with a given name wins)
    class_map: FxHashMap<Symbol, NominalTypeId>,
    /// Class declarations by symbol in source order: (span_start, nominal_type_id).
    /// Used for position-aware class resolution so later declarations do not
    /// retroactively rewrite earlier code's type bindings.
    class_decl_history: FxHashMap<Symbol, Vec<(usize, NominalTypeId)>>,
    /// Exact synthesized type alias (`__t_*`) to class ID mapping
    type_alias_class_map: FxHashMap<String, NominalTypeId>,
    /// Expanded object TypeId for synthesized `__t_*` aliases -> class ID.
    /// This lets lowered dispatch recover nominal class semantics even when
    /// checker-expanded aliases lose their reference wrapper.
    type_alias_object_class_map: FxHashMap<TypeId, NominalTypeId>,
    /// Class info (fields, initializers) for lowering `new` expressions
    class_info_map: FxHashMap<NominalTypeId, ClassInfo>,
    /// Per-declaration class ID (keyed by span start position, survives name collisions)
    class_decl_ids: FxHashMap<usize, NominalTypeId>,
    /// Class declarations already lowered (keyed by NominalTypeId)
    lowered_nominal_type_ids: FxHashSet<NominalTypeId>,
    /// Lowered class IR keyed by pre-assigned NominalTypeId
    lowered_classes: FxHashMap<NominalTypeId, IrClass>,
    /// Next function ID
    next_function_id: u32,
    /// Next class ID
    next_nominal_type_id: u32,
    /// Type alias name to ID mapping
    type_alias_map: FxHashMap<Symbol, TypeAliasId>,
    /// Resolved checker TypeId for object-capable type aliases (alias name -> TypeId)
    type_alias_resolved_type_map: FxHashMap<String, TypeId>,
    /// Object-layout fields for type aliases (alias name -> ordered fields)
    type_alias_object_fields: FxHashMap<String, Vec<(String, u16, TypeId)>>,
    /// Next type alias ID
    next_type_alias_id: u32,
    /// Pending label for the next loop (set by labeled statements)
    pending_label: Option<Symbol>,
    /// Stack of loop contexts for break/continue
    loop_stack: Vec<LoopContext>,
    /// Stack of non-iteration labeled statement contexts for labeled break
    label_stack: Vec<LabelContext>,
    /// Stack of switch exit blocks (break inside switch targets the switch exit, not the enclosing loop)
    switch_stack: Vec<BasicBlockId>,
    /// Stack of try-finally contexts for inlining finally blocks at return/break/continue
    try_finally_stack: Vec<TryFinallyEntry>,
    /// Pending arrow functions to be added to module (with their assigned func_id)
    pending_arrow_functions: Vec<(u32, IrFunction)>,
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
    variable_class_map: FxHashMap<Symbol, NominalTypeId>,
    /// Variables explicitly annotated as `any`/`unknown`, which must suppress
    /// nominal member dispatch even if their initializer was nominal.
    dynamic_any_vars: FxHashSet<Symbol>,
    /// Map from array variable name to its element's class type (for for-of loop type inference)
    array_element_class_map: FxHashMap<Symbol, NominalTypeId>,
    /// Current class being processed (for method lowering)
    current_class: Option<NominalTypeId>,
    /// Whether the currently lowered callable uses the class constructor as `this`.
    current_method_is_static: bool,
    /// Register holding `this` in current method
    this_register: Option<Register>,
    /// Deferred instance initialization for derived constructors until after
    /// the parent constructor has been invoked via `super()`.
    pending_constructor_prologue: Option<PendingConstructorPrologue>,
    /// Active constructor receiver for implicit and bare returns.
    constructor_return_this: Option<Register>,
    /// Info about `this` from ancestor scope (for arrow functions inside methods)
    this_ancestor_info: Option<AncestorThisInfo>,
    /// Capture index of `this` if it was captured (for LoadCaptured)
    this_captured_idx: Option<u16>,
    /// Synthetic local slot for JS `arguments` in the current executable.
    js_arguments_local: Option<u16>,
    /// Method name to function ID mapping (for method calls)
    method_map: FxHashMap<(NominalTypeId, Symbol), FunctionId>,
    /// Method name to vtable slot index (for virtual dispatch)
    method_slot_map: FxHashMap<(NominalTypeId, Symbol), u16>,
    /// Static method name to function ID mapping
    static_method_map: FxHashMap<(NominalTypeId, Symbol), FunctionId>,
    /// Method return type class mapping (for chained method call resolution)
    method_return_class_map: FxHashMap<(NominalTypeId, Symbol), NominalTypeId>,
    /// Function return type class mapping (for method dispatch on objects returned from standalone function calls)
    function_return_class_map: FxHashMap<Symbol, NominalTypeId>,
    /// Function return type alias mapping (for stable object field layout on alias-typed returns)
    function_return_type_alias_map: FxHashMap<Symbol, String>,
    /// Method return TypeId mapping (for ALL return types, not just class types)
    /// Populated during class registration. Used for bound method return type propagation.
    method_return_type_map: FxHashMap<(NominalTypeId, Symbol), TypeId>,
    /// Method return class-name fallback for forward references
    /// (e.g., `accept(): TcpStream | null` declared before `class TcpStream`).
    method_return_type_alias_map: FxHashMap<(NominalTypeId, Symbol), String>,
    /// Tracks variables holding bound methods: var_name → (nominal_type_id, method_name)
    /// Used to propagate return types when calling bound method variables.
    bound_method_vars: FxHashMap<Symbol, (NominalTypeId, Symbol)>,
    /// Variables bound to constructor/class values (e.g. `let C = Box;`).
    constructor_value_ctor_map: FxHashMap<Symbol, Symbol>,
    constructor_value_type_map: FxHashMap<Symbol, TypeId>,
    /// Next global variable index (for static fields and module-level variables)
    next_global_index: u16,
    /// Module-level variable name to global index mapping.
    /// Variables stored as globals so both main and module-level functions can access them.
    module_var_globals: FxHashMap<Symbol, u16>,
    /// Top-level JS lexical/class bindings stored in shared slots.
    /// These are visible to top-level helper functions and direct eval, but are
    /// not published onto the script global object.
    js_script_lexical_globals: FxHashMap<Symbol, u16>,
    /// Import-local binding symbols, used for import-specific lowering diagnostics.
    import_bindings: FxHashSet<Symbol>,
    /// Ambient builtin globals available without explicit source declarations/imports.
    ambient_builtin_globals: FxHashSet<String>,
    /// Variables initialized from imported class constructors where no local
    /// class metadata is available. These require late-bound member dispatch.
    late_bound_object_vars: FxHashSet<Symbol>,
    /// Constructor symbol for late-bound imported-class instances.
    /// Keyed by local variable symbol.
    late_bound_object_ctor_map: FxHashMap<Symbol, Symbol>,
    /// Checker/lowering TypeId for late-bound imported-class constructor symbols.
    /// Keyed by local variable symbol.
    late_bound_object_type_map: FxHashMap<Symbol, TypeId>,
    /// Synthetic global slot for `export default <expr>` materialization.
    default_export_global: Option<u16>,
    /// Depth counter: 0 = module top-level, >0 = inside function declaration.
    /// Used to prevent `let x = ...` inside functions from hijacking module globals.
    function_depth: u32,
    /// Block nesting depth at module scope.
    /// `0` means true module top-level statement context.
    block_depth: u32,
    /// Set of function IDs that are async closures (should be spawned as Tasks)
    async_closures: FxHashSet<FunctionId>,
    /// Map from local variable index to function ID for closures stored in variables
    /// Used to track async closures for SpawnClosure emission
    closure_locals: FxHashMap<u16, FunctionId>,
    /// Map from module-global variable index to function ID for closures stored in globals.
    /// Used to track async closures for SpawnClosure emission from global variables.
    closure_globals: FxHashMap<u16, FunctionId>,
    /// Stable global slots for top-level JS function declarations.
    /// Stored in source order so later declarations overwrite earlier ones.
    js_top_level_function_globals: Vec<(Symbol, u16, FunctionId)>,
    /// Function-scoped `var` bindings visible on the JS global object.
    js_function_scoped_var_globals: FxHashSet<Symbol>,
    /// Top-level JS class bindings stored in lexical global slots.
    js_top_level_class_globals: FxHashSet<Symbol>,
    /// Expression types from type checker (maps expr ptr to TypeId)
    expr_types: FxHashMap<usize, TypeId>,
    /// Type annotation types from checker (maps annotation ptr to TypeId)
    type_annotation_types: FxHashMap<usize, TypeId>,
    /// Fallback expression types keyed by source span `(start, end)`.
    expr_types_by_span: FxHashMap<(usize, usize), TypeId>,
    /// Type map for module-level globals (preserves initializer types through LoadGlobal)
    global_type_map: FxHashMap<u16, TypeId>,
    /// Enclosing std-wrapper locals exported to dedicated globals for the next class lowering.
    /// Populated by stmt lowering right before lowering a nested class declaration.
    pending_class_method_env_globals: Option<FxHashMap<Symbol, MethodEnvBinding>>,
    /// Active class-method environment globals while lowering a class's methods.
    current_method_env_globals: Option<FxHashMap<Symbol, MethodEnvBinding>>,
    /// Compile-time constant values (for constant folding)
    /// Maps symbol to its constant value (only for literals)
    constant_map: FxHashMap<Symbol, ConstantValue>,
    /// Object field layout for registers from decode<T> calls
    /// Maps register id → Vec<(field_name, field_index)>
    register_object_fields: FxHashMap<RegisterId, Vec<(String, usize)>>,
    /// Structural projection layout for registers whose field access must use shape-slot
    /// remapping instead of direct provider slots.
    register_structural_projection_fields: FxHashMap<RegisterId, Vec<(String, usize)>>,
    /// Nested object field layouts for concrete object fields.
    /// Maps (object register id, field index) -> nested field layout of the stored value.
    register_nested_object_fields: FxHashMap<(RegisterId, u16), Vec<(String, usize)>>,
    /// Object-layout hint for array elements.
    /// Maps array register id -> element object field layout.
    register_array_element_object_fields: FxHashMap<RegisterId, Vec<(String, usize)>>,
    /// Nested array element object layout for concrete object fields.
    /// Maps (object register id, field index) -> array element object layout.
    register_nested_array_element_object_fields: FxHashMap<(RegisterId, u16), Vec<(String, usize)>>,
    /// Object field layout for local variables holding decoded objects
    /// Maps variable name → Vec<(field_name, field_index)>
    variable_object_fields: FxHashMap<Symbol, Vec<(String, usize)>>,
    /// Nested object field layouts for object-typed variables.
    /// Maps variable name -> (field index -> nested field layout).
    variable_nested_object_fields: FxHashMap<Symbol, FxHashMap<u16, Vec<(String, usize)>>>,
    /// Alias name backing object-typed variables (identifier -> type alias name).
    /// Used to prefer declaration-order alias field indices over checker-internal object order.
    variable_object_type_aliases: FxHashMap<Symbol, String>,
    /// Explicit structural projection layout for variables whose static view should
    /// use shape-slot access instead of nominal class dispatch.
    variable_structural_projection_fields: FxHashMap<Symbol, Vec<(String, usize)>>,
    /// Canonical structural shapes referenced by this module.
    module_structural_shapes: FxHashMap<u64, Vec<String>>,
    /// Ordered structural layouts referenced by this module.
    module_structural_layouts: FxHashMap<u32, Vec<String>>,
    /// For variables holding async-call Task results, tracks the awaited value alias type.
    /// Example: `const t = async listener.accept()` records `t -> "__t_m0_TcpStream"`.
    task_result_type_aliases: FxHashMap<Symbol, String>,
    /// Optional filter for object spread field names in the current lowering context.
    /// When set (e.g., typed object literal initializer), spread only copies fields in this set.
    object_spread_target_filter: Option<FxHashSet<String>>,
    /// Optional canonical slot layout target for object literals in the current lowering context.
    /// When set, `lower_object` materializes this full layout (missing fields as null)
    /// to keep structural/union slot positions stable.
    object_literal_target_layout: Option<Vec<String>>,
    /// Native function name table for ModuleNativeCall.
    /// Accumulates symbolic names during lowering; each name gets a module-local index.
    native_function_table: Vec<String>,
    /// Reverse lookup: name → local index (for deduplication)
    native_function_map: FxHashMap<String, u16>,
    /// JSX compilation options (None = JSX not enabled)
    jsx_options: Option<JsxOptions>,
    /// Type parameter names for generic classes (NominalTypeId → ["T", "E", ...])
    class_type_params: FxHashMap<NominalTypeId, Vec<String>>,
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
    /// JS-compatible method extraction mode (`obj.method` is unbound).
    js_this_binding_compat: bool,
    /// Current inherited JS strictness context.
    js_strict_context: bool,
    /// Whether this module is a compiled builtin surface using ECMAScript builtin receiver rules.
    builtin_this_coercion_compat: bool,
    /// Whether unresolved member/call paths may lower to runtime late-bound dispatch.
    allow_unresolved_runtime_fallback: bool,
    /// Optional direct-eval wrapper function name for dynamic JS eval lowering.
    direct_eval_entry_function: Option<String>,
    /// Identifier names resolved through the active direct-eval environment.
    direct_eval_binding_names: FxHashSet<String>,
    /// Whether top-level statement completion should be preserved as the module result.
    track_top_level_completion: bool,
    /// Whether top-level JS var/function declarations should publish to globalThis.
    emit_script_global_bindings: bool,
    /// Whether published top-level JS globals should be configurable.
    script_global_bindings_configurable: bool,
    /// Whether the currently lowered function body is the direct-eval wrapper.
    in_direct_eval_function: bool,
    /// Whether the current lowering context is inside a parameter initializer or a closure
    /// created from one, where direct-eval bindings must shadow later body declarations.
    parameter_scope_eval_mode: bool,
    /// Parameter names that are currently in the JS parameter TDZ while initializers run.
    parameter_tdz_symbols: FxHashSet<Symbol>,
    /// Whether pattern binding should reuse preallocated parameter locals.
    parameter_binding_mode: bool,
    /// Whether the current function body is an arrow-compatible body where `arguments`
    /// should first consult any carried parameter-scope eval bindings.
    body_scope_eval_arguments_mode: bool,
    /// Parameter bindings visible in the currently lowered function body.
    parameter_symbols: FxHashSet<Symbol>,
    /// Currently-visible lexical/class bindings in JS lowering contexts.
    visible_js_lexical_symbols: FxHashSet<Symbol>,
    /// Anonymous local that tracks direct-eval script completion values.
    eval_completion_local: Option<u16>,
    /// Inner type for RefCell-wrapped variables (for preserving type info through loads)
    refcell_inner_types: FxHashMap<u16, TypeId>,
    /// Active per-function array local used to collect lowered generator `yield` values.
    generator_yield_array_local: Option<u16>,
}

// ─── Standalone helpers for closure capture pre-scan ───────────────────────

/// Recursively extract all bound identifier names from a pattern.
/// Handles Identifier, Array destructuring, Object destructuring, and Rest patterns.
pub(super) fn collect_pattern_names(pattern: &ast::Pattern, names: &mut FxHashSet<Symbol>) {
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
pub(super) fn collect_block_local_names(stmts: &[ast::Statement], locals: &mut FxHashSet<Symbol>) {
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
            Statement::ForIn(for_in) => {
                match &for_in.left {
                    ast::ForOfLeft::VariableDecl(var) => {
                        collect_pattern_names(&var.pattern, locals);
                    }
                    ast::ForOfLeft::Pattern(pat) => {
                        collect_pattern_names(pat, locals);
                    }
                }
                recurse_into_body(&for_in.body, locals);
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
            Statement::Labeled(labeled) => {
                collect_block_local_names(std::slice::from_ref(&*labeled.body), locals);
            }
            _ => {}
        }
    }
}

pub(super) fn collect_function_scoped_var_names(
    stmts: &[ast::Statement],
    names: &mut FxHashSet<Symbol>,
) {
    for stmt in stmts {
        match stmt {
            Statement::VariableDecl(var) => {
                if var.kind == ast::VariableKind::Var {
                    collect_pattern_names(&var.pattern, names);
                }
            }
            Statement::For(for_stmt) => {
                if let Some(ast::ForInit::VariableDecl(var)) = &for_stmt.init {
                    if var.kind == ast::VariableKind::Var {
                        collect_pattern_names(&var.pattern, names);
                    }
                }
                recurse_into_function_scoped_var_body(&for_stmt.body, names);
            }
            Statement::ForOf(for_of) => {
                if let ast::ForOfLeft::VariableDecl(var) = &for_of.left {
                    if var.kind == ast::VariableKind::Var {
                        collect_pattern_names(&var.pattern, names);
                    }
                }
                recurse_into_function_scoped_var_body(&for_of.body, names);
            }
            Statement::ForIn(for_in) => {
                if let ast::ForOfLeft::VariableDecl(var) = &for_in.left {
                    if var.kind == ast::VariableKind::Var {
                        collect_pattern_names(&var.pattern, names);
                    }
                }
                recurse_into_function_scoped_var_body(&for_in.body, names);
            }
            Statement::While(while_stmt) => {
                recurse_into_function_scoped_var_body(&while_stmt.body, names);
            }
            Statement::DoWhile(do_while) => {
                recurse_into_function_scoped_var_body(&do_while.body, names);
            }
            Statement::With(with_stmt) => {
                recurse_into_function_scoped_var_body(&with_stmt.body, names);
            }
            Statement::If(if_stmt) => {
                recurse_into_function_scoped_var_body(&if_stmt.then_branch, names);
                if let Some(else_branch) = &if_stmt.else_branch {
                    recurse_into_function_scoped_var_body(else_branch, names);
                }
            }
            Statement::Block(block) => {
                collect_function_scoped_var_names(&block.statements, names);
            }
            Statement::Try(try_stmt) => {
                collect_function_scoped_var_names(&try_stmt.body.statements, names);
                if let Some(catch_clause) = &try_stmt.catch_clause {
                    collect_function_scoped_var_names(&catch_clause.body.statements, names);
                }
                if let Some(finally_clause) = &try_stmt.finally_clause {
                    collect_function_scoped_var_names(&finally_clause.statements, names);
                }
            }
            Statement::Switch(switch_stmt) => {
                for case in &switch_stmt.cases {
                    collect_function_scoped_var_names(&case.consequent, names);
                }
            }
            Statement::Labeled(labeled) => {
                recurse_into_function_scoped_var_body(&labeled.body, names);
            }
            Statement::FunctionDecl(_)
            | Statement::ClassDecl(_)
            | Statement::Return(_)
            | Statement::Yield(_)
            | Statement::Expression(_)
            | Statement::Break(_)
            | Statement::Continue(_)
            | Statement::Throw(_)
            | Statement::ImportDecl(_)
            | Statement::ExportDecl(_)
            | Statement::TypeAliasDecl(_)
            | Statement::Debugger(_)
            | Statement::Empty(_) => {}
        }
    }
}

fn recurse_into_function_scoped_var_body(body: &ast::Statement, names: &mut FxHashSet<Symbol>) {
    if let Statement::Block(block) = body {
        collect_function_scoped_var_names(&block.statements, names);
    } else {
        collect_function_scoped_var_names(std::slice::from_ref(body), names);
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
    fn analyze_closure_body(&mut self, params: &[ast::Parameter], body: &ast::BlockStatement) {
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
        match expr {
            Expression::Arrow(arrow) => {
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
                return;
            }
            Expression::Function(func) => {
                self.analyze_closure_body(&func.params, &func.body);
                return;
            }
            _ => {}
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
        if let Expression::Unary(unary) = expr {
            if matches!(
                unary.operator,
                ast::UnaryOperator::PrefixIncrement
                    | ast::UnaryOperator::PrefixDecrement
                    | ast::UnaryOperator::PostfixIncrement
                    | ast::UnaryOperator::PostfixDecrement
            ) {
                if let Expression::Identifier(ident) = &*unary.operand {
                    if self.outer_locals.contains(&ident.name)
                        && !self.arrow_locals.contains(&ident.name)
                    {
                        self.refcell_vars.insert(ident.name);
                    }
                }
            }
        }
        // Nested arrow = new scope boundary — delegate back to ArrowCaptureFinder
        if matches!(expr, Expression::Arrow(_) | Expression::Function(_)) {
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
    fn visit_variable_decl(&mut self, decl: &ast::VariableDecl) {
        // Variable initializers behave like assignments for capture analysis.
        // JS var hoisting can create closures before the initializer runs, so
        // captured bindings with initializers must be treated as live mutable
        // cells rather than one-time copied values.
        if decl.initializer.is_some() {
            collect_pattern_names(&decl.pattern, self.assigned);
        }
        if let Some(init) = &decl.initializer {
            self.visit_expression(init);
        }
    }

    fn visit_expression(&mut self, expr: &Expression) {
        if let Expression::Assignment(assign) = expr {
            if let Expression::Identifier(ident) = &*assign.left {
                self.assigned.insert(ident.name);
            }
        }
        if matches!(expr, Expression::Arrow(_) | Expression::Function(_)) {
            return; // Don't enter separate scopes
        }
        walk_expression(self, expr);
    }

    fn visit_arrow_function(&mut self, _: &ast::ArrowFunction) {}
    fn visit_function_decl(&mut self, _: &ast::FunctionDecl) {}
    fn visit_class_decl(&mut self, _: &ast::ClassDecl) {}
}

pub(super) fn is_module_wrapper_function_name(name: &str) -> bool {
    name.starts_with("__std_module_")
        || name.starts_with("__raya_mod_init_")
        || name.starts_with("__raya_entry_main_")
        || name.starts_with("__raya_entry_")
}

fn module_wrapper_alias_tag(name: &str) -> Option<String> {
    if let Some(tag) = name.strip_prefix("__std_module_") {
        return Some(tag.to_string());
    }
    if let Some(module_id) = name.strip_prefix("__raya_mod_init_") {
        return Some(format!("m{}", module_id));
    }
    None
}

fn bind_wrapper_alias_to_class(
    lowerer: &mut Lowerer<'_>,
    wrapper_tag: &str,
    class_name_symbol: Symbol,
    nominal_type_id: NominalTypeId,
) {
    let class_name = lowerer.interner.resolve(class_name_symbol);
    let alias = format!("__t_{}_{}", wrapper_tag, class_name);
    if let Some(prev) = lowerer
        .type_alias_class_map
        .insert(alias.clone(), nominal_type_id)
    {
        if prev != nominal_type_id {
            lowerer
                .errors
                .push(super::error::CompileError::InternalError {
                    message: format!(
                        "conflicting wrapper alias mapping for '{}': {} vs {}",
                        alias,
                        prev.as_u32(),
                        nominal_type_id.as_u32()
                    ),
                });
        }
    }
}

/// Visitor that pre-registers class declarations inside nested statement trees.
/// Carries wrapper context (`__std_module_<tag>`, `__raya_mod_init_<id>`) to build exact alias mappings.
struct NestedClassRegistrar<'l, 'a> {
    lowerer: &'l mut Lowerer<'a>,
    wrapper_tag: Option<String>,
}

impl Visitor for NestedClassRegistrar<'_, '_> {
    fn visit_statement(&mut self, stmt: &Statement) {
        let inner = Lowerer::unwrap_export(stmt);
        walk_statement(self, inner);
    }

    fn visit_function_decl(&mut self, decl: &ast::FunctionDecl) {
        let prev = self.wrapper_tag.clone();
        let fn_name = self.lowerer.interner.resolve(decl.name.name);
        if let Some(tag) = module_wrapper_alias_tag(fn_name) {
            self.wrapper_tag = Some(tag);
        }
        walk_function_decl(self, decl);
        self.wrapper_tag = prev;
    }

    fn visit_class_decl(&mut self, decl: &ast::ClassDecl) {
        self.lowerer
            .register_class_with_alias_context(decl, self.wrapper_tag.as_deref());
        ast::walk_class_decl(self, decl);
    }
}

/// Visitor that pre-registers function declarations inside nested statement trees.
/// Used for module-wrapper helper functions so forward sibling calls resolve deterministically.
struct NestedFunctionRegistrar<'l, 'a> {
    lowerer: &'l mut Lowerer<'a>,
}

impl Visitor for NestedFunctionRegistrar<'_, '_> {
    fn visit_statement(&mut self, stmt: &Statement) {
        let inner = Lowerer::unwrap_export(stmt);
        walk_statement(self, inner);
    }

    fn visit_function_decl(&mut self, decl: &ast::FunctionDecl) {
        self.lowerer.register_function_decl(decl);
        walk_function_decl(self, decl);
    }

    fn visit_class_decl(&mut self, _: &ast::ClassDecl) {
        // Nested class methods/constructors are separate lowering paths.
    }
}

impl<'a> Lowerer<'a> {
    fn record_function_return_mappings(
        &mut self,
        fn_name: Symbol,
        return_type: Option<&ast::TypeAnnotation>,
    ) {
        if let Some(ret_type) = return_type {
            if let ast::Type::Reference(type_ref) = &ret_type.ty {
                let ret_name = self.interner.resolve(type_ref.name.name).to_string();
                self.function_return_type_alias_map
                    .insert(fn_name, ret_name);
            }
            if let Some(nominal_type_id) = self.try_extract_class_from_type(ret_type) {
                self.function_return_class_map
                    .insert(fn_name, nominal_type_id);
            }
        }
    }

    fn register_function_decl(&mut self, func: &ast::FunctionDecl) -> FunctionId {
        if let Some(&func_id) = self.function_decl_ids.get(&func.span.start) {
            return func_id;
        }

        let func_id = FunctionId::new(self.next_function_id);
        self.next_function_id += 1;

        self.function_decl_ids.insert(func.span.start, func_id);
        self.function_map.insert(func.name.name, func_id);

        if func.is_async {
            self.async_functions.insert(func_id);
        }

        self.record_function_return_mappings(func.name.name, func.return_type.as_ref());

        // Generic nested functions can still be called via explicit type args.
        if func.type_params.as_ref().is_some_and(|tp| !tp.is_empty()) {
            self.generic_function_asts
                .insert(func.name.name, func.clone());
        }

        func_id
    }

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
            callable_local_hints: FxHashSet::default(),
            callable_symbol_hints: FxHashSet::default(),
            next_local: 0,
            function_map: FxHashMap::default(),
            function_decl_ids: FxHashMap::default(),
            hoisted_function_decl_spans: FxHashSet::default(),
            async_functions: FxHashSet::default(),
            class_map: FxHashMap::default(),
            class_decl_history: FxHashMap::default(),
            type_alias_class_map: FxHashMap::default(),
            type_alias_object_class_map: FxHashMap::default(),
            class_info_map: FxHashMap::default(),
            class_decl_ids: FxHashMap::default(),
            lowered_nominal_type_ids: FxHashSet::default(),
            lowered_classes: FxHashMap::default(),
            next_function_id: 0,
            next_nominal_type_id: 0,
            type_alias_map: FxHashMap::default(),
            type_alias_resolved_type_map: FxHashMap::default(),
            type_alias_object_fields: FxHashMap::default(),
            next_type_alias_id: 0,
            pending_label: None,
            loop_stack: Vec::new(),
            label_stack: Vec::new(),
            switch_stack: Vec::new(),
            try_finally_stack: Vec::new(),
            pending_arrow_functions: Vec::new(),
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
            dynamic_any_vars: FxHashSet::default(),
            array_element_class_map: FxHashMap::default(),
            current_class: None,
            current_method_is_static: false,
            this_register: None,
            pending_constructor_prologue: None,
            constructor_return_this: None,
            this_ancestor_info: None,
            this_captured_idx: None,
            js_arguments_local: None,
            method_map: FxHashMap::default(),
            method_slot_map: FxHashMap::default(),
            static_method_map: FxHashMap::default(),
            method_return_class_map: FxHashMap::default(),
            function_return_class_map: FxHashMap::default(),
            function_return_type_alias_map: FxHashMap::default(),
            method_return_type_map: FxHashMap::default(),
            method_return_type_alias_map: FxHashMap::default(),
            bound_method_vars: FxHashMap::default(),
            constructor_value_ctor_map: FxHashMap::default(),
            constructor_value_type_map: FxHashMap::default(),
            next_global_index: 0,
            module_var_globals: FxHashMap::default(),
            js_script_lexical_globals: FxHashMap::default(),
            import_bindings: FxHashSet::default(),
            ambient_builtin_globals: FxHashSet::default(),
            late_bound_object_vars: FxHashSet::default(),
            late_bound_object_ctor_map: FxHashMap::default(),
            late_bound_object_type_map: FxHashMap::default(),
            default_export_global: None,
            function_depth: 0,
            block_depth: 0,
            async_closures: FxHashSet::default(),
            closure_locals: FxHashMap::default(),
            closure_globals: FxHashMap::default(),
            js_top_level_function_globals: Vec::new(),
            js_function_scoped_var_globals: FxHashSet::default(),
            js_top_level_class_globals: FxHashSet::default(),
            expr_types,
            type_annotation_types: FxHashMap::default(),
            expr_types_by_span: FxHashMap::default(),
            global_type_map: FxHashMap::default(),
            pending_class_method_env_globals: None,
            current_method_env_globals: None,
            constant_map: FxHashMap::default(),
            register_object_fields: FxHashMap::default(),
            register_structural_projection_fields: FxHashMap::default(),
            register_nested_object_fields: FxHashMap::default(),
            register_array_element_object_fields: FxHashMap::default(),
            register_nested_array_element_object_fields: FxHashMap::default(),
            variable_object_fields: FxHashMap::default(),
            variable_nested_object_fields: FxHashMap::default(),
            variable_object_type_aliases: FxHashMap::default(),
            variable_structural_projection_fields: FxHashMap::default(),
            module_structural_shapes: FxHashMap::default(),
            module_structural_layouts: FxHashMap::default(),
            task_result_type_aliases: FxHashMap::default(),
            object_spread_target_filter: None,
            object_literal_target_layout: None,
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
            js_this_binding_compat: false,
            js_strict_context: false,
            builtin_this_coercion_compat: false,
            allow_unresolved_runtime_fallback: true,
            direct_eval_entry_function: None,
            direct_eval_binding_names: FxHashSet::default(),
            track_top_level_completion: false,
            emit_script_global_bindings: true,
            script_global_bindings_configurable: false,
            in_direct_eval_function: false,
            parameter_scope_eval_mode: false,
            parameter_tdz_symbols: FxHashSet::default(),
            parameter_binding_mode: false,
            body_scope_eval_arguments_mode: false,
            parameter_symbols: FxHashSet::default(),
            visible_js_lexical_symbols: FxHashSet::default(),
            eval_completion_local: None,
            generator_yield_array_local: None,
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

    /// Provide checker-resolved type IDs for annotation nodes.
    pub fn with_type_annotation_types(
        mut self,
        type_annotation_types: FxHashMap<usize, TypeId>,
    ) -> Self {
        self.type_annotation_types = type_annotation_types;
        self
    }

    /// Provide ambient builtin global names available via runtime native lookup.
    pub fn with_ambient_builtin_globals(mut self, names: FxHashSet<String>) -> Self {
        self.ambient_builtin_globals = names;
        self
    }

    /// Enable JS-compatible method extraction (`obj.method` is unbound).
    pub fn with_js_this_binding_compat(mut self, enable: bool) -> Self {
        self.js_this_binding_compat = enable;
        self
    }

    /// Enable builtin JS receiver coercion semantics for compiled builtin surfaces.
    pub fn with_builtin_this_coercion_compat(mut self, enable: bool) -> Self {
        self.builtin_this_coercion_compat = enable;
        self
    }

    /// Enable/disable lowering unresolved member dispatch to runtime late-bound paths.
    pub fn with_unresolved_runtime_fallback(mut self, enable: bool) -> Self {
        self.allow_unresolved_runtime_fallback = enable;
        self
    }

    pub fn with_direct_eval_entry_function(mut self, function_name: Option<String>) -> Self {
        self.direct_eval_entry_function = function_name;
        self
    }

    pub fn with_direct_eval_binding_names<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.direct_eval_binding_names = names.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_track_top_level_completion(mut self, enable: bool) -> Self {
        self.track_top_level_completion = enable;
        self
    }

    pub fn with_emit_script_global_bindings(mut self, enable: bool) -> Self {
        self.emit_script_global_bindings = enable;
        self
    }

    pub fn with_script_global_bindings_configurable(mut self, enable: bool) -> Self {
        self.script_global_bindings_configurable = enable;
        self
    }

    pub(super) fn direct_eval_has_parameter_binding(&self, name: &str) -> bool {
        self.parameter_symbols
            .iter()
            .any(|symbol| self.interner.resolve(*symbol) == name)
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

    pub fn module_global_slots(&self) -> std::collections::HashMap<String, u32> {
        let mut slots = std::collections::HashMap::with_capacity(
            self.module_var_globals.len() + usize::from(self.default_export_global.is_some()),
        );
        for (&name, &slot) in &self.module_var_globals {
            slots.insert(self.interner.resolve(name).to_string(), slot as u32);
        }
        if let Some(slot) = self.default_export_global {
            slots.entry("default".to_string()).or_insert(slot as u32);
        }
        slots
    }

    pub fn js_global_bindings(
        &self,
    ) -> Vec<crate::compiler::bytecode::module::JsGlobalBindingInfo> {
        use crate::compiler::bytecode::module::{JsGlobalBindingInfo, JsGlobalBindingKind};

        if !self.js_this_binding_compat
            || self.builtin_this_coercion_compat
            || !self.emit_script_global_bindings
        {
            return Vec::new();
        }

        let function_names = self
            .js_top_level_function_globals
            .iter()
            .map(|(name, _, _)| *name)
            .collect::<FxHashSet<_>>();
        let mut bindings = Vec::with_capacity(
            self.module_var_globals.len() + self.js_script_lexical_globals.len(),
        );

        for (&name, &slot) in &self.module_var_globals {
            if !self.js_function_scoped_var_globals.contains(&name)
                && !function_names.contains(&name)
            {
                continue;
            }
            let kind = if function_names.contains(&name) {
                JsGlobalBindingKind::Function
            } else {
                JsGlobalBindingKind::Var
            };
            bindings.push(JsGlobalBindingInfo {
                name: self.interner.resolve(name).to_string(),
                slot: slot as u32,
                kind,
                published_to_global_object: true,
            });
        }

        for (&name, &slot) in &self.js_script_lexical_globals {
            let kind = if self.js_top_level_class_globals.contains(&name) {
                JsGlobalBindingKind::Class
            } else {
                JsGlobalBindingKind::Lexical
            };
            bindings.push(JsGlobalBindingInfo {
                name: self.interner.resolve(name).to_string(),
                slot: slot as u32,
                kind,
                published_to_global_object: false,
            });
        }

        bindings.sort_by(|a, b| a.name.cmp(&b.name).then(a.slot.cmp(&b.slot)));
        bindings
    }

    pub fn structural_shape_member_sets(&self) -> Vec<Vec<String>> {
        let mut entries = self
            .module_structural_shapes
            .iter()
            .map(|(shape_id, names)| (*shape_id, names.clone()))
            .collect::<Vec<_>>();
        entries.sort_by_key(|(shape_id, _)| *shape_id);
        entries.into_iter().map(|(_, names)| names).collect()
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

    pub(super) fn init_generator_yield_array(&mut self) {
        let local_idx = self.allocate_anonymous_local();
        let len = self.emit_i32_const(0);
        let array_reg = self.alloc_register(TypeId::new(ARRAY_TYPE_ID));
        self.emit(IrInstr::NewArray {
            dest: array_reg.clone(),
            len,
            elem_ty: TypeId::new(UNKNOWN_TYPE_ID),
        });
        self.emit(IrInstr::StoreLocal {
            index: local_idx,
            value: array_reg,
        });
        self.generator_yield_array_local = Some(local_idx);
    }

    pub(super) fn load_generator_yield_array(&mut self) -> Option<Register> {
        let local_idx = self.generator_yield_array_local?;
        let array_reg = self.alloc_register(TypeId::new(ARRAY_TYPE_ID));
        self.emit(IrInstr::LoadLocal {
            dest: array_reg.clone(),
            index: local_idx,
        });
        Some(array_reg)
    }

    pub(super) fn function_uses_generator_snapshot(&self, body: &ast::BlockStatement) -> bool {
        let mut collector = GeneratorRequirementCollector {
            requires_snapshot: false,
        };
        walk_block_statement(&mut collector, body);
        collector.requires_snapshot
    }

    pub(super) fn emit_function_return(&mut self, value: Option<Register>) {
        let return_value = if let Some(yield_array) = self.load_generator_yield_array() {
            let completion = if let Some(value) = value {
                value
            } else {
                let undefined = self.alloc_register(UNRESOLVED);
                self.emit(IrInstr::Assign {
                    dest: undefined.clone(),
                    value: IrValue::Constant(IrConstant::Undefined),
                });
                undefined
            };
            let iterator = self.alloc_register(UNRESOLVED);
            self.emit(IrInstr::NativeCall {
                dest: Some(iterator.clone()),
                native_id: crate::compiler::native_id::OBJECT_GENERATOR_SNAPSHOT_NEW,
                args: vec![yield_array, completion],
            });
            Some(iterator)
        } else {
            value
        };
        self.set_terminator(Terminator::Return(return_value));
    }

    pub(super) fn emit_fallthrough_return(&mut self) {
        if self.in_direct_eval_function {
            let value = self.alloc_register(UNRESOLVED);
            self.emit(IrInstr::NativeCall {
                dest: Some(value.clone()),
                native_id: crate::compiler::native_id::OBJECT_EVAL_GET_COMPLETION,
                args: vec![],
            });
            self.emit_function_return(Some(value));
            return;
        }
        if let Some(local_idx) = self.eval_completion_local {
            let value = self.alloc_register(UNRESOLVED);
            self.emit(IrInstr::LoadLocal {
                dest: value.clone(),
                index: local_idx,
            });
            self.emit_function_return(Some(value));
            return;
        }
        self.emit_function_return(None);
    }

    pub(super) fn emit_bool_const(&mut self, value: bool) -> Register {
        let reg = self.alloc_register(TypeId::new(BOOLEAN_TYPE_ID));
        self.emit(IrInstr::Assign {
            dest: reg.clone(),
            value: IrValue::Constant(IrConstant::Boolean(value)),
        });
        reg
    }

    pub(super) fn init_eval_completion_local(&mut self) {
        if self.in_direct_eval_function {
            self.eval_completion_local = None;
            return;
        }
        let local_idx = self.allocate_anonymous_local();
        let initial = self.alloc_register(UNRESOLVED);
        self.emit(IrInstr::Assign {
            dest: initial.clone(),
            value: IrValue::Constant(IrConstant::Undefined),
        });
        self.emit(IrInstr::StoreLocal {
            index: local_idx,
            value: initial,
        });
        self.eval_completion_local = Some(local_idx);
    }

    pub(super) fn record_eval_completion(&mut self, value: Register) {
        if self.in_direct_eval_function {
            self.emit(IrInstr::NativeCall {
                dest: None,
                native_id: crate::compiler::native_id::OBJECT_EVAL_SET_COMPLETION,
                args: vec![value],
            });
            return;
        }
        if let Some(local_idx) = self.eval_completion_local {
            self.emit(IrInstr::StoreLocal {
                index: local_idx,
                value,
            });
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
            Expression::Identifier(ident) => self.constant_map.get(&ident.name).cloned(),
            // Could extend to support simple constant expressions like 0x0300
            // but for now only support direct literals
            _ => None,
        }
    }

    /// Look up a compile-time constant by symbol
    pub fn lookup_constant(&self, name: Symbol) -> Option<&ConstantValue> {
        self.constant_map.get(&name)
    }

    /// Build a span-keyed fallback index for expression typing.
    fn build_expr_type_span_index(&mut self, module: &ast::Module) {
        self.expr_types_by_span.clear();
        let mut collector = ExprTypeSpanCollector {
            expr_types: &self.expr_types,
            by_span: &mut self.expr_types_by_span,
        };
        for stmt in &module.statements {
            walk_statement(&mut collector, stmt);
        }
    }

    /// Get the TypeId for an expression from the type checker's expr_types map.
    /// Falls back to UNRESOLVED if not found (compiler couldn't determine type).
    fn get_expr_type(&self, expr: &Expression) -> TypeId {
        if let Expression::Identifier(ident) = expr {
            if let Some(&ty) = self.constructor_value_type_map.get(&ident.name) {
                return ty;
            }
        }
        let expr_id = expr as *const _ as usize;
        self.expr_types
            .get(&expr_id)
            .copied()
            .or_else(|| {
                let span = expr.span();
                self.expr_types_by_span
                    .get(&(span.start, span.end))
                    .copied()
            })
            .unwrap_or(UNRESOLVED)
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
                self.errors
                    .push(super::error::CompileError::InternalError { message: msg });
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
        self.build_expr_type_span_index(module);
        let saved_js_strict_context = self.js_strict_context;
        self.js_strict_context = module
            .statements
            .iter()
            .map(Self::unwrap_export)
            .take_while(|stmt| {
                matches!(
                    stmt,
                    Statement::Expression(ast::ExpressionStatement {
                        expression: Expression::StringLiteral(_),
                        ..
                    })
                )
            })
            .any(|stmt| {
                matches!(
                    stmt,
                    Statement::Expression(ast::ExpressionStatement {
                        expression: Expression::StringLiteral(lit),
                        ..
                    }) if lit.raw_literal && self.interner.resolve(lit.value) == "use strict"
                )
            });

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

        // Pre-pass: assign global indices to module-level bindings so both main and
        // module-level functions can access them via LoadGlobal/StoreGlobal.
        //
        // This includes:
        // - import-local bindings (`import { x as y }`, `import d`, `import * as ns`)
        // - module-level variable declarations
        //
        // Imported bindings are runtime values provided by module linkage; pre-registering
        // them avoids unresolved-identifier lowering errors for valid imported references.
        {
            // Step 0: Reserve globals for import-local bindings.
            for raw_stmt in &module.statements {
                let stmt = Self::unwrap_export(raw_stmt);
                if let Statement::ImportDecl(import) = stmt {
                    for specifier in &import.specifiers {
                        let local_name = match specifier {
                            ast::ImportSpecifier::Named { name, alias } => {
                                alias.as_ref().map_or(name.name, |a| a.name)
                            }
                            ast::ImportSpecifier::Namespace(alias) => alias.name,
                            ast::ImportSpecifier::Default(local) => local.name,
                        };
                        let global_index = *self
                            .module_var_globals
                            .entry(local_name)
                            .or_insert_with(|| {
                                let global_index = self.next_global_index;
                                self.next_global_index += 1;
                                global_index
                            });
                        self.import_bindings.insert(local_name);

                        if let Some(imported_ty) = self
                            .type_ctx
                            .lookup_named_type(self.interner.resolve(local_name))
                        {
                            if imported_ty != UNRESOLVED {
                                self.global_type_map
                                    .entry(global_index)
                                    .or_insert(imported_ty);

                                if self.type_has_construct_signature(imported_ty) {
                                    self.mark_constructor_value_binding(
                                        local_name,
                                        local_name,
                                        Some(imported_ty),
                                    );

                                    if let Some(layout) =
                                        self.structural_static_slot_layout_from_type(imported_ty)
                                    {
                                        if !layout.is_empty() {
                                            self.variable_structural_projection_fields.insert(
                                                local_name,
                                                layout
                                                    .into_iter()
                                                    .map(|(field_name, field_idx)| {
                                                        (field_name, field_idx as usize)
                                                    })
                                                    .collect(),
                                            );
                                            self.variable_class_map.remove(&local_name);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // `export default <expr>` must materialize in a stable global slot so
                // binary export tables can reference it as a constant export.
                if matches!(raw_stmt, Statement::ExportDecl(ExportDecl::Default { .. })) {
                    if self.default_export_global.is_none() {
                        let global_index = self.next_global_index;
                        self.next_global_index += 1;
                        self.default_export_global = Some(global_index);
                    }
                }
            }

            // Step 1: Collect candidate module-level variable names (excluding constants)
            let candidates: FxHashSet<Symbol> = module
                .statements
                .iter()
                .flat_map(|s| {
                    let s = Self::unwrap_export(s);
                    if let Statement::VariableDecl(decl) = s {
                        let mut names = FxHashSet::default();
                        collect_pattern_names(&decl.pattern, &mut names);
                        names
                            .into_iter()
                            .filter(|name| !self.constant_map.contains_key(name))
                            .collect::<Vec<_>>()
                    } else {
                        Vec::new()
                    }
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

            // Step 3: Promote all non-constant module-level variables.
            //
            // Rationale:
            // Module-level bindings (including stdlib singletons like `io`) must be
            // accessible from module-level function bodies. Restricting promotion to
            // a reference-analysis subset can miss valid references in some paths and
            // cause unresolved identifiers to lower to null placeholders at runtime
            // (e.g. `io.writeln(...)` inside `function main()`), which then fails with
            // "Expected object for method call".
            //
            // Promoting all non-constant module vars is semantically correct and keeps
            // function access predictable.
            for raw_stmt in &module.statements {
                let stmt = Self::unwrap_export(raw_stmt);
                if let Statement::VariableDecl(decl) = stmt {
                    let mut names = FxHashSet::default();
                    collect_pattern_names(&decl.pattern, &mut names);
                    for name in names {
                        let _was_referenced = referenced.contains(&name);
                        self.module_var_globals.entry(name).or_insert_with(|| {
                            let global_index = self.next_global_index;
                            self.next_global_index += 1;
                            global_index
                        });
                    }
                }
            }

            if self.js_this_binding_compat && !self.builtin_this_coercion_compat {
                let mut js_function_scoped_vars = FxHashSet::default();
                collect_function_scoped_var_names(&module.statements, &mut js_function_scoped_vars);
                self.js_function_scoped_var_globals = js_function_scoped_vars.clone();
                for name in js_function_scoped_vars {
                    self.module_var_globals.entry(name).or_insert_with(|| {
                        let global_index = self.next_global_index;
                        self.next_global_index += 1;
                        global_index
                    });
                }
            }

            for raw_stmt in &module.statements {
                let stmt = Self::unwrap_export(raw_stmt);
                match stmt {
                    Statement::VariableDecl(decl)
                        if decl.kind != crate::parser::ast::VariableKind::Var =>
                    {
                        if let ast::Pattern::Identifier(ident) = &decl.pattern {
                            self.js_script_lexical_globals
                                .entry(ident.name)
                                .or_insert_with(|| {
                                    let global_index = self.next_global_index;
                                    self.next_global_index += 1;
                                    global_index
                                });
                        }
                    }
                    Statement::ClassDecl(class) => {
                        self.js_top_level_class_globals.insert(class.name.name);
                        self.js_script_lexical_globals
                            .entry(class.name.name)
                            .or_insert_with(|| {
                                let global_index = self.next_global_index;
                                self.next_global_index += 1;
                                global_index
                            });
                    }
                    _ => {}
                }
            }
        }

        // First pass: collect function and class declarations
        for raw_stmt in &module.statements {
            let stmt = Self::unwrap_export(raw_stmt);
            self.set_span(stmt.span());
            match stmt {
                Statement::FunctionDecl(func) => {
                    let func_id = self.register_function_decl(func);
                    if self.js_this_binding_compat {
                        let global_index = *self
                            .module_var_globals
                            .entry(func.name.name)
                            .or_insert_with(|| {
                                let global_index = self.next_global_index;
                                self.next_global_index += 1;
                                global_index
                            });
                        self.js_top_level_function_globals.push((
                            func.name.name,
                            global_index,
                            func_id,
                        ));
                        if func.is_async {
                            self.closure_globals.insert(global_index, func_id);
                        }
                    }
                    let wrapper_tag =
                        module_wrapper_alias_tag(self.interner.resolve(func.name.name));
                    self.prepare_executable_body_declarations(&func.body.statements, wrapper_tag);
                }
                Statement::ClassDecl(class) => {
                    let mut visitor = NestedClassRegistrar {
                        lowerer: self,
                        wrapper_tag: None,
                    };
                    visitor.visit_class_decl(class);
                }
                Statement::TypeAliasDecl(type_alias) => {
                    // Register the alias so JSON.parse() results can be cast/projected through it
                    let type_alias_id = TypeAliasId::new(self.next_type_alias_id);
                    self.next_type_alias_id += 1;
                    self.type_alias_map
                        .insert(type_alias.name.name, type_alias_id);
                    let alias_name = self.interner.resolve(type_alias.name.name).to_string();
                    let is_wrapper_alias = alias_name.starts_with("__t_");

                    let mut members: Vec<(String, TypeId, bool)> = Vec::new();
                    match &type_alias.type_annotation.ty {
                        ast::Type::Object(obj_type) => {
                            for member in &obj_type.members {
                                match member {
                                    ast::ObjectTypeMember::Property(prop) => {
                                        members.push((
                                            self.interner.resolve(prop.name.name).to_string(),
                                            self.resolve_type_annotation(&prop.ty),
                                            false,
                                        ));
                                    }
                                    ast::ObjectTypeMember::Method(method) => {
                                        if !is_wrapper_alias {
                                            members.push((
                                                self.interner.resolve(method.name.name).to_string(),
                                                UNRESOLVED,
                                                true,
                                            ));
                                        }
                                    }
                                    ast::ObjectTypeMember::IndexSignature(_) => {}
                                    ast::ObjectTypeMember::CallSignature(_) => {}
                                    ast::ObjectTypeMember::ConstructSignature(_) => {}
                                }
                            }
                        }
                        ast::Type::Union(union_type) => {
                            let mut names = FxHashSet::default();
                            for member in &union_type.types {
                                match &member.ty {
                                    ast::Type::Object(obj_type) => {
                                        for obj_member in &obj_type.members {
                                            match obj_member {
                                                ast::ObjectTypeMember::Property(prop) => {
                                                    names.insert(
                                                        self.interner
                                                            .resolve(prop.name.name)
                                                            .to_string(),
                                                    );
                                                }
                                                ast::ObjectTypeMember::Method(method) => {
                                                    if !is_wrapper_alias {
                                                        names.insert(
                                                            self.interner
                                                                .resolve(method.name.name)
                                                                .to_string(),
                                                        );
                                                    }
                                                }
                                                ast::ObjectTypeMember::IndexSignature(_) => {}
                                                ast::ObjectTypeMember::CallSignature(_) => {}
                                                ast::ObjectTypeMember::ConstructSignature(_) => {}
                                            }
                                        }
                                    }
                                    ast::Type::Reference(type_ref) => {
                                        let ref_name =
                                            self.interner.resolve(type_ref.name.name).to_string();
                                        if let Some(ref_fields) =
                                            self.type_alias_object_fields.get(&ref_name)
                                        {
                                            for (name, _, _) in ref_fields {
                                                names.insert(name.clone());
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            members.extend(names.into_iter().map(|name| (name, UNRESOLVED, false)));
                        }
                        ast::Type::Reference(type_ref) => {
                            let ref_name = self.interner.resolve(type_ref.name.name).to_string();
                            if let Some(ref_fields) = self.type_alias_object_fields.get(&ref_name) {
                                members.extend(
                                    ref_fields
                                        .iter()
                                        .map(|(name, _, ty)| (name.clone(), *ty, false)),
                                );
                            }
                        }
                        _ => {}
                    }

                    if !members.is_empty() {
                        // Canonical slot ABI for structural aliases:
                        // declaration ordering should not affect runtime field slots.
                        members.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.2.cmp(&b.2)));
                        let fields: Vec<(String, u16, TypeId)> = members
                            .into_iter()
                            .enumerate()
                            .map(|(idx, (name, ty, _is_method))| (name, idx as u16, ty))
                            .collect();
                        self.type_alias_object_fields
                            .insert(alias_name.clone(), fields);
                        let alias_ty = self.resolve_type_annotation(&type_alias.type_annotation);
                        self.type_alias_resolved_type_map
                            .insert(alias_name.clone(), alias_ty);
                        if let Some(&nominal_type_id) = self.type_alias_class_map.get(&alias_name) {
                            self.populate_alias_object_class_map(&alias_name, nominal_type_id);
                        }
                        if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                            if let Some(stored) = self.type_alias_object_fields.get(&alias_name) {
                                let summary = stored
                                    .iter()
                                    .map(|(n, i, _)| format!("{}:{}", n, i))
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                eprintln!("[lower] alias fields {} => [{}]", alias_name, summary);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // Second chance: resolve object-wrapper alias TypeIds after classes/functions are
        // fully registered. During the first pass many wrapper aliases are temporarily
        // unresolved; keeping them unresolved breaks class-dispatch recovery later.
        for raw_stmt in &module.statements {
            let stmt = Self::unwrap_export(raw_stmt);
            self.set_span(stmt.span());
            if let Statement::TypeAliasDecl(type_alias) = stmt {
                let alias_name = self.interner.resolve(type_alias.name.name).to_string();
                if !self.type_alias_object_fields.contains_key(&alias_name) {
                    continue;
                }
                let resolved = self.resolve_type_annotation(&type_alias.type_annotation);
                if resolved != UNRESOLVED {
                    self.type_alias_resolved_type_map
                        .insert(alias_name.clone(), resolved);
                } else if let Some(named) = self.type_ctx.lookup_named_type(&alias_name) {
                    self.type_alias_resolved_type_map
                        .insert(alias_name.clone(), named);
                }
            }
        }

        // Reconcile synthesized wrapper aliases after first-pass registration.
        // Type aliases can appear before the wrapper function/class declaration in
        // linked sources, so populate alias->class type bridges once both sides exist.
        let alias_class_pairs: Vec<(String, NominalTypeId)> = self
            .type_alias_class_map
            .iter()
            .map(|(alias, &cid)| (alias.clone(), cid))
            .collect();
        for (alias_name, nominal_type_id) in alias_class_pairs {
            if self.type_alias_object_fields.contains_key(&alias_name) {
                self.populate_alias_object_class_map(&alias_name, nominal_type_id);
            }
        }

        // Pre-pass: populate variable_class_map for module-level variable declarations.
        // This must happen BEFORE the second pass (which lowers functions) so that
        // functions referencing module-level variables (e.g., `math.abs()` where
        // `const math = new Math()`) can resolve the correct class type for method dispatch.
        for raw_stmt in &module.statements {
            let stmt = Self::unwrap_export(raw_stmt);
            self.set_span(stmt.span());
            if let Statement::VariableDecl(decl) = stmt {
                if let Pattern::Identifier(ident) = &decl.pattern {
                    let name = ident.name;
                    // Track class type from explicit type annotation
                    if let Some(type_ann) = &decl.type_annotation {
                        if let Some(nominal_type_id) = self.try_extract_class_from_type(type_ann) {
                            self.variable_class_map.insert(name, nominal_type_id);
                            self.clear_late_bound_object_binding(name);
                        }
                    }
                    // Track class type from new expression (e.g., `const math = new Math()`)
                    #[allow(clippy::collapsible_match)]
                    if !self.variable_class_map.contains_key(&name) {
                        if let Some(init) = &decl.initializer {
                            if let ast::Expression::New(new_expr) = init {
                                if let ast::Expression::Identifier(nominal_type_ident) =
                                    &*new_expr.callee
                                {
                                    let ctor_ty = self
                                        .get_expr_type(&new_expr.callee)
                                        .as_u32()
                                        .ne(&UNRESOLVED_TYPE_ID)
                                        .then(|| self.get_expr_type(&new_expr.callee))
                                        .or_else(|| {
                                            self.type_ctx.lookup_named_type(
                                                self.interner.resolve(nominal_type_ident.name),
                                            )
                                        });
                                    let nominal_type_id = self
                                        .resolve_runtime_bound_new_nominal_type(
                                            nominal_type_ident.name,
                                            ctor_ty,
                                            self.get_expr_type(init)
                                                .as_u32()
                                                .ne(&UNRESOLVED_TYPE_ID)
                                                .then(|| self.get_expr_type(init)),
                                        );
                                    if let Some(nominal_type_id) = nominal_type_id {
                                        self.variable_class_map.insert(name, nominal_type_id);
                                        self.clear_late_bound_object_binding(name);
                                        if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                                            eprintln!(
                                                "[lower] variable_class_map: '{}' = nominal_type_id({}) (from new {}())",
                                                self.interner.resolve(name),
                                                nominal_type_id.as_u32(),
                                                self.interner.resolve(nominal_type_ident.name)
                                            );
                                        }
                                    } else if self
                                        .import_bindings
                                        .contains(&nominal_type_ident.name)
                                        || self.ambient_builtin_globals.contains(
                                            self.interner.resolve(nominal_type_ident.name),
                                        )
                                    {
                                        self.mark_late_bound_object_binding(
                                            name,
                                            nominal_type_ident.name,
                                            ctor_ty,
                                        );
                                        if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                                            eprintln!(
                                                "[lower] late_bound_object_vars: '{}' marked (from runtime-bound new {}())",
                                                self.interner.resolve(name),
                                                self.interner.resolve(nominal_type_ident.name)
                                            );
                                        }
                                    } else if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                                        eprintln!(
                                            "[lower] variable_class_map: '{}' NOT added — class '{}' not in class_map",
                                            self.interner.resolve(name),
                                            self.interner.resolve(nominal_type_ident.name)
                                        );
                                    }
                                }
                            } else if let ast::Expression::TypeCast(cast) = init {
                                // Preserve class dispatch for import bindings such as:
                                //   const path = (__std_exports___node_path.default as __t___node_path_NodePath);
                                if let Some(nominal_type_id) =
                                    self.try_extract_class_from_type(&cast.target_type)
                                {
                                    self.variable_class_map.insert(name, nominal_type_id);
                                    self.clear_late_bound_object_binding(name);
                                    if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                                        eprintln!(
                                            "[lower] variable_class_map: '{}' = nominal_type_id({}) (from cast)",
                                            self.interner.resolve(name),
                                            nominal_type_id.as_u32(),
                                        );
                                    }
                                }
                            } else if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                                // Log why annotation-less non-new expressions are skipped
                                let var_name = self.interner.resolve(name);
                                if !var_name.starts_with('_') {
                                    eprintln!(
                                        "[lower] variable_class_map: '{}' NOT added — initializer is not a new-expr and no class type annotation",
                                        var_name
                                    );
                                }
                            }
                        }
                    } else if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                        let var_name = self.interner.resolve(name);
                        eprintln!(
                            "[lower] variable_class_map: '{}' added from type annotation",
                            var_name
                        );
                    }
                    // Populate object field layout for __std_exports_<tag> variables.
                    // When the initializer is `__std_module_<tag>()`, the return type is
                    // `__std_exports_type_<tag>` whose field layout is already known.
                    // Populating variable_object_fields here makes has_concrete_layout=true
                    // in lower_member, so LoadFieldExact (static index) is emitted instead of
                    // LateBoundMember (name-based lookup that returns null at runtime).
                    if let Some(init) = &decl.initializer {
                        let (call_expr, cast_alias_name) = match init {
                            ast::Expression::Call(call_expr) => (Some(call_expr), None),
                            ast::Expression::TypeCast(cast) => {
                                let alias_name = match &cast.target_type.ty {
                                    ast::Type::Reference(type_ref) => {
                                        Some(self.interner.resolve(type_ref.name.name).to_string())
                                    }
                                    _ => None,
                                };
                                if let ast::Expression::Call(call_expr) = &*cast.object {
                                    (Some(call_expr), alias_name)
                                } else {
                                    (None, alias_name)
                                }
                            }
                            _ => (None, None),
                        };

                        if let Some(call_expr) = call_expr {
                            if let ast::Expression::Identifier(func_ident) = &*call_expr.callee {
                                let func_name = self.interner.resolve(func_ident.name).to_string();
                                let inferred_alias =
                                    if let Some(tag) = func_name.strip_prefix("__std_module_") {
                                        Some(format!("__std_exports_type_{}", tag))
                                    } else if let Some(module_id) =
                                        func_name.strip_prefix("__raya_mod_init_")
                                    {
                                        Some(format!("__raya_mod_exports_type_{}", module_id))
                                    } else {
                                        None
                                    };
                                if let Some(alias_name) = cast_alias_name.or(inferred_alias) {
                                    if let Some(fields) =
                                        self.type_alias_object_fields.get(&alias_name).cloned()
                                    {
                                        let field_layout: Vec<(String, usize)> = fields
                                            .iter()
                                            .map(|(n, idx, _)| (n.clone(), *idx as usize))
                                            .collect();
                                        if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                                            eprintln!(
                                                "[lower] __std_exports prepass: '{}' -> alias='{}' fields=[{}]",
                                                self.interner.resolve(name),
                                                alias_name,
                                                field_layout.iter().map(|(n,i)| format!("{}:{}", n, i)).collect::<Vec<_>>().join(", ")
                                            );
                                        }
                                        self.variable_object_fields.insert(name, field_layout);
                                        self.variable_object_type_aliases.insert(name, alias_name);
                                    } else if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                                        eprintln!(
                                            "[lower] __std_exports prepass: '{}' -> alias='{}' NOT FOUND in type_alias_object_fields",
                                            self.interner.resolve(name),
                                            alias_name
                                        );
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
            self.set_span(stmt.span());
            match stmt {
                Statement::FunctionDecl(func) => {
                    // Use declaration-scoped function IDs so duplicate names across
                    // linked std modules don't alias each other.
                    let func_id = self.function_id_for_decl(func).unwrap_or_else(|| {
                        panic!(
                            "ICE: function '{}' at span {} was not pre-registered",
                            self.interner.resolve(func.name.name),
                            func.span.start
                        )
                    });
                    let ir_func = self.lower_function(func);
                    // Add to pending with pre-assigned ID (will be sorted later)
                    self.pending_arrow_functions
                        .push((func_id.as_u32(), ir_func));
                }
                Statement::ClassDecl(class) => {
                    self.lower_class_declaration(class);
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
        // ClassDecl is included so static initializer blocks execute at declaration position.
        let top_level_stmts: Vec<_> = module
            .statements
            .iter()
            .filter(|s| {
                let inner = Self::unwrap_export(s);
                !matches!(
                    inner,
                    Statement::FunctionDecl(_) | Statement::TypeAliasDecl(_)
                )
            })
            .collect();

        // Reserve main function's ID BEFORE lowering, so arrow functions
        // created during lowering get IDs after main
        let main_func_id = if !top_level_stmts.is_empty() || self.track_top_level_completion {
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

        // Emit classes in strict NominalTypeId order so nominal_type_id == module.classes index.
        for raw_id in 0..self.next_nominal_type_id {
            let nominal_type_id = NominalTypeId::new(raw_id);
            if let Some(ir_class) = self.lowered_classes.remove(&nominal_type_id) {
                ir_module.add_class(ir_class);
            } else {
                self.errors.push(super::error::CompileError::InternalError {
                    message: format!(
                        "registered class id {} was never lowered",
                        nominal_type_id.as_u32()
                    ),
                });
            }
        }

        // Add ALL pending functions (including main and class methods) sorted by func_id
        // This ensures functions are added to the module in the order of their pre-assigned IDs
        if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
            let mut seen = FxHashSet::default();
            for (id, _) in &self.pending_arrow_functions {
                seen.insert(*id);
            }
            let mut missing = Vec::new();
            for id in 0..self.next_function_id {
                if !seen.contains(&id) {
                    missing.push(id);
                    if missing.len() >= 8 {
                        break;
                    }
                }
            }
            if !missing.is_empty() {
                eprintln!(
                    "[lower] missing preassigned function ids (first {}): {:?}; next_function_id={}",
                    missing.len(),
                    missing,
                    self.next_function_id
                );
                for miss in &missing {
                    let mut labels = Vec::new();
                    for (&sym, &fid) in &self.function_map {
                        if fid.as_u32() == *miss {
                            labels.push(format!("function_map:{}", self.interner.resolve(sym)));
                        }
                    }
                    for (&span_start, &fid) in &self.function_decl_ids {
                        if fid.as_u32() == *miss {
                            labels.push(format!("function_decl_ids@{}", span_start));
                        }
                    }
                    for (&(nominal_type_id, method_sym), &fid) in &self.method_map {
                        if fid.as_u32() == *miss {
                            let class_name = self
                                .class_map
                                .iter()
                                .find_map(|(&sym, &id)| {
                                    if id == nominal_type_id {
                                        Some(self.interner.resolve(sym).to_string())
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or_else(|| format!("class{}", nominal_type_id.as_u32()));
                            labels.push(format!(
                                "method_map:{}::{}",
                                class_name,
                                self.interner.resolve(method_sym)
                            ));
                        }
                    }
                    for (&(nominal_type_id, method_sym), &fid) in &self.static_method_map {
                        if fid.as_u32() == *miss {
                            let class_name = self
                                .class_map
                                .iter()
                                .find_map(|(&sym, &id)| {
                                    if id == nominal_type_id {
                                        Some(self.interner.resolve(sym).to_string())
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or_else(|| format!("class{}", nominal_type_id.as_u32()));
                            labels.push(format!(
                                "static_method_map:{}::{}",
                                class_name,
                                self.interner.resolve(method_sym)
                            ));
                        }
                    }
                    for (&nominal_type_id, info) in &self.class_info_map {
                        if info.constructor.is_some_and(|fid| fid.as_u32() == *miss) {
                            let class_name = self
                                .class_map
                                .iter()
                                .find_map(|(&sym, &id)| {
                                    if id == nominal_type_id {
                                        Some(self.interner.resolve(sym).to_string())
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or_else(|| format!("class{}", nominal_type_id.as_u32()));
                            labels.push(format!("constructor:{}", class_name));
                        }
                    }
                    if !labels.is_empty() {
                        eprintln!("[lower] missing id {} labels: {}", miss, labels.join(", "));
                    }
                }
            }
        }
        self.pending_arrow_functions.sort_by_key(|(id, _)| *id);
        for (_id, func) in self.pending_arrow_functions.drain(..) {
            ir_module.add_function(func);
        }

        // Transfer native function table to the IR module
        ir_module.native_functions = self.take_native_function_table();
        ir_module.structural_shapes = self.module_structural_shapes.clone();
        ir_module.structural_layouts = self.module_structural_layouts.clone();

        self.js_strict_context = saved_js_strict_context;
        ir_module
    }

    /// Register class declarations reachable within nested statement blocks.
    /// Needed for module-wrapper functions where classes are function-local.
    fn register_nested_classes_in_block(
        &mut self,
        statements: &[Statement],
        wrapper_tag: Option<String>,
    ) {
        let mut visitor = NestedClassRegistrar {
            lowerer: self,
            wrapper_tag,
        };
        for stmt in statements {
            visitor.visit_statement(stmt);
        }
    }

    fn prepare_executable_body_declarations(
        &mut self,
        statements: &[Statement],
        wrapper_tag: Option<String>,
    ) {
        self.register_nested_functions_in_block(statements);
        self.register_nested_classes_in_block(statements, wrapper_tag);
    }

    /// Register nested function declarations reachable in a statement subtree.
    /// Used for module-wrapper functions so forward sibling helper calls resolve by ID.
    fn register_nested_functions_in_block(&mut self, statements: &[Statement]) {
        let mut visitor = NestedFunctionRegistrar { lowerer: self };
        for stmt in statements {
            visitor.visit_statement(stmt);
        }
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
            let mut collector = ScopeAssignmentCollector {
                assigned: &mut assigned,
            };
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

    fn find_js_arguments_symbol(
        &self,
        params: &[ast::Parameter],
        body: &ast::BlockStatement,
    ) -> Option<Symbol> {
        if scope_binds_arguments_in_params(params, self.interner)
            || scope_binds_arguments_in_statements(&body.statements, self.interner)
        {
            return None;
        }

        let mut collector = ArgumentsUseCollector {
            interner: self.interner,
            found: None,
        };
        for param in params {
            if let Some(default_expr) = &param.default_value {
                collector.visit_expression(default_expr);
                if collector.found.is_some() {
                    return collector.found;
                }
            }
        }
        for stmt in &body.statements {
            collector.visit_statement(stmt);
            if collector.found.is_some() {
                return collector.found;
            }
        }
        collector.found
    }

    fn compute_js_arguments_mapping(
        &self,
        params: &[ast::Parameter],
        param_local_indices: &[Option<u16>],
        is_strict_js: bool,
    ) -> Vec<u16> {
        let simple_params = !is_strict_js
            && params.iter().all(|param| {
                !param.is_rest
                    && param.default_value.is_none()
                    && !param.optional
                    && matches!(param.pattern, Pattern::Identifier(_))
            });
        if !simple_params {
            return Vec::new();
        }

        let mut mapping = vec![u16::MAX; params.len()];
        let mut seen = FxHashSet::default();
        for (index, param) in params.iter().enumerate().rev() {
            let Pattern::Identifier(ident) = &param.pattern else {
                continue;
            };
            if !seen.insert(ident.name) {
                continue;
            }
            if let Some(local_idx) = param_local_indices.get(index).and_then(|slot| *slot) {
                mapping[index] = local_idx;
            }
        }
        mapping
    }

    fn emit_refcell_wrap_for_local(&mut self, local_idx: u16, value_ty: TypeId) {
        if self.refcell_registers.contains_key(&local_idx) {
            return;
        }

        let current_value = self.alloc_register(value_ty);
        self.emit(IrInstr::LoadLocal {
            dest: current_value.clone(),
            index: local_idx,
        });

        let refcell_reg = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::NewRefCell {
            dest: refcell_reg.clone(),
            initial_value: current_value,
        });
        self.local_registers.insert(local_idx, refcell_reg.clone());
        self.refcell_registers
            .insert(local_idx, refcell_reg.clone());
        self.refcell_inner_types.insert(local_idx, value_ty);
        self.emit(IrInstr::StoreLocal {
            index: local_idx,
            value: refcell_reg,
        });
    }

    fn emit_js_arguments_prologue(
        &mut self,
        arguments_symbol: Option<Symbol>,
        params: &[ast::Parameter],
        param_local_indices: &[Option<u16>],
        is_strict_js: bool,
    ) {
        self.js_arguments_local = None;

        if self.in_direct_eval_function {
            self.current_function_mut().js_arguments_mapping.clear();
            return;
        }

        let Some(arguments_symbol) = arguments_symbol else {
            self.current_function_mut().js_arguments_mapping.clear();
            return;
        };

        let mapping = self.compute_js_arguments_mapping(params, param_local_indices, is_strict_js);
        self.current_function_mut().js_arguments_mapping = mapping.clone();

        for (index, maybe_local_idx) in param_local_indices.iter().enumerate() {
            let Some(local_idx) = *maybe_local_idx else {
                continue;
            };
            let needs_refcell = mapping.get(index).copied().unwrap_or(u16::MAX) != u16::MAX
                || matches!(
                    &params[index].pattern,
                    Pattern::Identifier(ident) if self.refcell_vars.contains(&ident.name)
                );
            if !needs_refcell {
                continue;
            }
            let value_ty = self
                .local_registers
                .get(&local_idx)
                .map(|reg| reg.ty)
                .unwrap_or(UNRESOLVED);
            self.emit_refcell_wrap_for_local(local_idx, value_ty);
        }

        let arguments_local = self.allocate_local(arguments_symbol);
        self.js_arguments_local = Some(arguments_local);

        let arguments_reg = self.alloc_register(TypeId::new(JSON_OBJECT_TYPE_ID));
        self.emit(IrInstr::NativeCall {
            dest: Some(arguments_reg.clone()),
            native_id: crate::compiler::native_id::OBJECT_GET_ARGUMENTS_OBJECT,
            args: vec![],
        });

        if self.refcell_vars.contains(&arguments_symbol) {
            let refcell_reg = self.alloc_register(TypeId::new(0));
            self.emit(IrInstr::NewRefCell {
                dest: refcell_reg.clone(),
                initial_value: arguments_reg.clone(),
            });
            self.local_registers
                .insert(arguments_local, refcell_reg.clone());
            self.refcell_registers
                .insert(arguments_local, refcell_reg.clone());
            self.refcell_inner_types
                .insert(arguments_local, arguments_reg.ty);
            self.emit(IrInstr::StoreLocal {
                index: arguments_local,
                value: refcell_reg,
            });
        } else {
            self.local_registers
                .insert(arguments_local, arguments_reg.clone());
            self.emit(IrInstr::StoreLocal {
                index: arguments_local,
                value: arguments_reg,
            });
        }
    }

    /// Collect all local variable names declared in statements
    fn collect_local_names(&self, stmts: &[ast::Statement]) -> FxHashSet<Symbol> {
        let mut locals = FxHashSet::default();
        collect_block_local_names(stmts, &mut locals);
        locals
    }

    fn nominal_type_id_for_decl(&self, class: &ast::ClassDecl) -> Option<NominalTypeId> {
        self.class_decl_ids.get(&class.span.start).copied()
    }

    fn known_property_symbol_from_expr(&mut self, expr: &ast::Expression) -> Option<Symbol> {
        match expr {
            ast::Expression::StringLiteral(lit) => Some(lit.value),
            ast::Expression::IntLiteral(lit) => self.interner.lookup(&lit.value.to_string()),
            ast::Expression::Parenthesized(expr) => {
                self.known_property_symbol_from_expr(&expr.expression)
            }
            _ => None,
        }
    }

    fn known_class_member_symbol(&mut self, key: &ast::PropertyKey) -> Option<Symbol> {
        match key {
            ast::PropertyKey::Identifier(id) => Some(id.name),
            ast::PropertyKey::StringLiteral(lit) => Some(lit.value),
            ast::PropertyKey::IntLiteral(lit) => self.interner.lookup(&lit.value.to_string()),
            ast::PropertyKey::Computed(expr) => self.known_property_symbol_from_expr(expr),
        }
    }

    fn class_member_display_name(&self, key: &ast::PropertyKey, fallback_idx: usize) -> String {
        match key {
            ast::PropertyKey::Identifier(id) => self.interner.resolve(id.name).to_string(),
            ast::PropertyKey::StringLiteral(lit) => self.interner.resolve(lit.value).to_string(),
            ast::PropertyKey::IntLiteral(lit) => lit.value.to_string(),
            ast::PropertyKey::Computed(ast::Expression::StringLiteral(lit)) => {
                self.interner.resolve(lit.value).to_string()
            }
            ast::PropertyKey::Computed(ast::Expression::IntLiteral(lit)) => lit.value.to_string(),
            ast::PropertyKey::Computed(ast::Expression::Parenthesized(expr)) => self
                .class_member_display_name(
                    &ast::PropertyKey::Computed(*expr.expression.clone()),
                    fallback_idx,
                ),
            ast::PropertyKey::Computed(_) => format!("[computed:{fallback_idx}]"),
        }
    }

    pub(super) fn nominal_type_id_for_class_expression(
        &self,
        expr: &ast::Expression,
    ) -> Option<NominalTypeId> {
        match expr {
            ast::Expression::Parenthesized(inner) => {
                self.nominal_type_id_for_class_expression(&inner.expression)
            }
            ast::Expression::Call(call) if call.arguments.is_empty() => {
                let callee = match &*call.callee {
                    ast::Expression::Parenthesized(inner) => &inner.expression,
                    other => other,
                };
                let ast::Expression::Function(function_expr) = callee else {
                    return None;
                };
                let [ast::Statement::ClassDecl(class_decl), ast::Statement::Return(ret)] =
                    function_expr.body.statements.as_slice()
                else {
                    return None;
                };
                let Some(ast::Expression::Identifier(ret_ident)) = &ret.value else {
                    return None;
                };
                if ret_ident.name != class_decl.name.name {
                    return None;
                }
                self.nominal_type_id_for_decl(class_decl)
                    .or_else(|| self.class_map.get(&class_decl.name.name).copied())
            }
            _ => None,
        }
    }

    pub(super) fn class_expression_name_symbol(&self, expr: &ast::Expression) -> Option<Symbol> {
        match expr {
            ast::Expression::Parenthesized(inner) => {
                self.class_expression_name_symbol(&inner.expression)
            }
            ast::Expression::Call(call) if call.arguments.is_empty() => {
                let callee = match &*call.callee {
                    ast::Expression::Parenthesized(inner) => &inner.expression,
                    other => other,
                };
                let ast::Expression::Function(function_expr) = callee else {
                    return None;
                };
                let [ast::Statement::ClassDecl(class_decl), ast::Statement::Return(ret)] =
                    function_expr.body.statements.as_slice()
                else {
                    return None;
                };
                let Some(ast::Expression::Identifier(ret_ident)) = &ret.value else {
                    return None;
                };
                (ret_ident.name == class_decl.name.name).then_some(class_decl.name.name)
            }
            _ => None,
        }
    }

    pub(super) fn function_id_for_decl(&self, func: &ast::FunctionDecl) -> Option<FunctionId> {
        self.function_decl_ids.get(&func.span.start).copied()
    }

    fn lower_class_declaration(&mut self, class: &ast::ClassDecl) {
        let Some(nominal_type_id) = self.nominal_type_id_for_decl(class) else {
            self.errors.push(super::error::CompileError::InternalError {
                message: format!(
                    "class declaration '{}' at span {} was not pre-registered",
                    self.interner.resolve(class.name.name),
                    class.span.start
                ),
            });
            return;
        };

        if self.lowered_nominal_type_ids.contains(&nominal_type_id) {
            return;
        }

        let ir_class = self.lower_class(class);
        self.lowered_nominal_type_ids.insert(nominal_type_id);
        self.lowered_classes.insert(nominal_type_id, ir_class);
    }

    /// Register a class declaration (first-pass registration).
    /// Assigns class ID, collects fields/methods/constructor info, builds ClassInfo.
    /// Must be called before `lower_class` for the same class.
    fn register_class(&mut self, class: &ast::ClassDecl) {
        self.register_class_with_alias_context(class, None);
    }

    fn register_class_with_alias_context(
        &mut self,
        class: &ast::ClassDecl,
        wrapper_tag: Option<&str>,
    ) {
        // Resolve type-name references (method returns/field types) in this class's
        // lexical source position so shadowing picks the right class declaration.
        let saved_span = self.current_span;
        self.current_span = class.span;

        if let Some(&nominal_type_id) = self.class_decl_ids.get(&class.span.start) {
            self.class_map.insert(class.name.name, nominal_type_id);
            if let Some(tag) = wrapper_tag {
                bind_wrapper_alias_to_class(self, tag, class.name.name, nominal_type_id);
            }
            if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                eprintln!(
                    "[lower] reuse_class name={} nominal_type_id={} span={}",
                    self.interner.resolve(class.name.name),
                    nominal_type_id.as_u32(),
                    class.span.start
                );
            }
            self.current_span = saved_span;
            return;
        }

        let nominal_type_id = NominalTypeId::new(self.next_nominal_type_id);
        self.next_nominal_type_id += 1;

        // Track per-declaration class ID (survives name collisions)
        self.class_decl_ids
            .insert(class.span.start, nominal_type_id);
        self.class_decl_history
            .entry(class.name.name)
            .or_default()
            .push((class.span.start, nominal_type_id));

        // Insert into class_map (last class with a given name wins for name-based lookups)
        self.class_map.insert(class.name.name, nominal_type_id);

        if let Some(tag) = wrapper_tag {
            bind_wrapper_alias_to_class(self, tag, class.name.name, nominal_type_id);
        }

        // Store type parameter names for generic classes
        if let Some(ref type_params) = class.type_params {
            if !type_params.is_empty() {
                let param_names: Vec<String> = type_params
                    .iter()
                    .map(|tp| self.interner.resolve(tp.name.name).to_string())
                    .collect();
                self.class_type_params.insert(nominal_type_id, param_names);
            }
        }

        // Resolve parent class if extends clause is present
        let mut extends_type_args: Option<Vec<TypeId>> = None;
        let (parent_class, parent_runtime_name, parent_constructor_symbol) =
            if let Some(ref extends) = class.extends {
                if let ast::Type::Reference(type_ref) = &extends.ty {
                    // Extract type arguments from extends clause (e.g., Base<string>)
                    if let Some(ref type_args) = type_ref.type_args {
                        let resolved: Vec<TypeId> = type_args
                            .iter()
                            .map(|ta| self.resolve_type_annotation(ta))
                            .collect();
                        extends_type_args = Some(resolved);
                    }
                    let parent_name = self.interner.resolve(type_ref.name.name).to_string();
                    let local_parent_class = self
                        .class_decl_history
                        .get(&type_ref.name.name)
                        .and_then(|_| self.class_map.get(&type_ref.name.name).copied());
                    let imported_parent_class = if local_parent_class.is_none() {
                        self.nominal_type_id_from_type_name(&parent_name)
                    } else {
                        None
                    };
                    let parent_class = local_parent_class.or(imported_parent_class);
                    let runtime_name = (imported_parent_class.is_some() || parent_class.is_none())
                        .then_some(parent_name);
                    (parent_class, runtime_name, Some(type_ref.name.name))
                } else {
                    (None, None, None)
                }
            } else {
                (None, None, None)
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
                        let subs: std::collections::HashMap<String, TypeId> = parent_type_params
                            .iter()
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
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        let mut field_index = parent_fields.len() as u16;

        for (member_idx, member) in class.members.iter().enumerate() {
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

                let class_type = type_name
                    .as_ref()
                    .and_then(|name| self.nominal_type_id_from_type_name(name));

                // For array fields like `items: Item[]`, preserve element class info
                // so indexed member access (`this.items[i].field`) can resolve method/field types.
                let array_elem_class_type = field.type_annotation.as_ref().and_then(|t| {
                    if let ast::Type::Array(arr_ty) = &t.ty {
                        if let ast::Type::Reference(elem_ref) = &arr_ty.element_type.ty {
                            return self.nominal_type_id_from_type_name(
                                self.interner.resolve(elem_ref.name.name),
                            );
                        }
                    }
                    None
                });

                if field.is_static {
                    let Some(field_name) = self.known_class_member_symbol(&field.name) else {
                        continue;
                    };
                    let global_index = self.next_global_index;
                    self.next_global_index += 1;
                    static_fields.push(StaticFieldInfo {
                        name: field_name,
                        nominal_type_id,
                        global_index,
                        initializer: field.initializer.clone(),
                    });
                } else {
                    let Some(field_name) = self.known_class_member_symbol(&field.name) else {
                        continue;
                    };
                    // Check if this field shadows a parent field with the same name.
                    // If so, reuse the parent's field index so base class methods
                    // that access `this.x` see the derived class's value.
                    let field_name_str = self.class_member_display_name(&field.name, member_idx);
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
                        name: field_name,
                        index: idx,
                        ty,
                        initializer: field.initializer.clone(),
                        class_type: class_type.or(array_elem_class_type),
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
        let mut runtime_method_elements = Vec::new();
        for (member_idx, member) in class.members.iter().enumerate() {
            if let ast::ClassMember::Method(method) = member {
                if method.body.is_some() {
                    let func_id = FunctionId::new(self.next_function_id);
                    self.next_function_id += 1;

                    if method.is_async {
                        self.async_functions.insert(func_id);
                    }

                    runtime_method_elements.push(RuntimeClassMethodElement {
                        key: method.name.clone(),
                        func_id,
                        kind: method.kind,
                        is_static: method.is_static,
                        order: member_idx,
                    });

                    if let Some(method_name) = self.known_class_member_symbol(&method.name) {
                        if method.is_static {
                            static_methods_vec.push(StaticMethodInfo {
                                name: method_name,
                                func_id,
                                kind: method.kind,
                            });
                            self.static_method_map
                                .insert((nominal_type_id, method_name), func_id);
                        } else {
                            methods.push(ClassMethodInfo {
                                name: method_name,
                                func_id,
                                kind: method.kind,
                                visibility: method.visibility,
                            });
                            self.method_map
                                .insert((nominal_type_id, method_name), func_id);
                        }

                        if let Some(ret_type) = &method.return_type {
                            if let Some(ret_nominal_type_id) =
                                self.try_extract_class_from_type(ret_type)
                            {
                                self.method_return_class_map
                                    .insert((nominal_type_id, method_name), ret_nominal_type_id);
                            } else if let Some(ret_class_name) =
                                self.try_extract_class_name_from_type(ret_type)
                            {
                                self.method_return_type_alias_map
                                    .insert((nominal_type_id, method_name), ret_class_name);
                            }
                            // Store full return TypeId for all return types (bound method propagation)
                            let type_id = self.resolve_type_annotation(ret_type);
                            self.method_return_type_map
                                .insert((nominal_type_id, method_name), type_id);
                        }
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
                    let Some(method_name) = self.known_class_member_symbol(&method.name) else {
                        continue;
                    };
                    let slot = self
                        .find_parent_method_slot(parent_class, method_name)
                        .unwrap_or_else(|| {
                            let s = next_slot;
                            next_slot += 1;
                            s
                        });
                    self.method_slot_map
                        .insert((nominal_type_id, method_name), slot);
                }
            }
        }

        for method_info in &methods {
            let slot = self
                .find_parent_method_slot(parent_class, method_info.name)
                .unwrap_or_else(|| {
                    let s = next_slot;
                    next_slot += 1;
                    s
                });
            self.method_slot_map
                .insert((nominal_type_id, method_info.name), slot);
            // Symbol.iterator keyed protocol bridge:
            // until parser-level computed members land, map iterator protocol
            // helpers onto the `Symbol.iterator` key internally. Prefer
            // `__iteratorObject` when present because it returns a real iterator
            // object; plain `iterator` is only a collection-level fallback.
            let method_name = self.interner.resolve(method_info.name);
            if method_name == "iterator" || method_name == "__iteratorObject" {
                if let Some(symbol_iterator) = self.interner.lookup("Symbol.iterator") {
                    self.method_slot_map
                        .insert((nominal_type_id, symbol_iterator), slot);
                }
            }
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
        if constructor.is_none() {
            // Always materialize a constructor function ID so decorators can
            // receive a class/function target even for classes without explicit ctors.
            let func_id = FunctionId::new(self.next_function_id);
            self.next_function_id += 1;
            constructor = Some(func_id);
        }

        // Decorators
        let class_decorators: Vec<DecoratorInfo> = class
            .decorators
            .iter()
            .map(|d| DecoratorInfo {
                expression: d.expression.clone(),
                expr_type: self.get_expr_type(&d.expression),
            })
            .collect();

        let mut method_decorators = Vec::new();
        for member in &class.members {
            if let ast::ClassMember::Method(method) = member {
                if !method.decorators.is_empty() {
                    let Some(method_name) = self.known_class_member_symbol(&method.name) else {
                        continue;
                    };
                    method_decorators.push(MethodDecoratorInfo {
                        method_name,
                        decorators: method
                            .decorators
                            .iter()
                            .map(|d| DecoratorInfo {
                                expression: d.expression.clone(),
                                expr_type: self.get_expr_type(&d.expression),
                            })
                            .collect(),
                    });
                }
            }
        }

        let mut static_blocks = Vec::new();
        for member in &class.members {
            if let ast::ClassMember::StaticBlock(block) = member {
                static_blocks.push(block.clone());
            }
        }

        let mut field_decorators = Vec::new();
        for member in &class.members {
            if let ast::ClassMember::Field(field) = member {
                if !field.decorators.is_empty() {
                    let Some(field_name) = self.known_class_member_symbol(&field.name) else {
                        continue;
                    };
                    field_decorators.push(FieldDecoratorInfo {
                        field_name,
                        decorators: field
                            .decorators
                            .iter()
                            .map(|d| DecoratorInfo {
                                expression: d.expression.clone(),
                                expr_type: self.get_expr_type(&d.expression),
                            })
                            .collect(),
                    });
                }
            }
        }

        let mut parameter_decorators = Vec::new();
        for (member_idx, member) in class.members.iter().enumerate() {
            match member {
                ast::ClassMember::Method(method) => {
                    let method_name = self.class_member_display_name(&method.name, member_idx);
                    for (index, param) in method.params.iter().enumerate() {
                        if !param.decorators.is_empty() {
                            parameter_decorators.push(ParameterDecoratorInfo {
                                method_name: method_name.clone(),
                                param_index: index as u32,
                                decorators: param
                                    .decorators
                                    .iter()
                                    .map(|d| DecoratorInfo {
                                        expression: d.expression.clone(),
                                        expr_type: self.get_expr_type(&d.expression),
                                    })
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
                                decorators: param
                                    .decorators
                                    .iter()
                                    .map(|d| DecoratorInfo {
                                        expression: d.expression.clone(),
                                        expr_type: self.get_expr_type(&d.expression),
                                    })
                                    .collect(),
                            });
                        }
                    }
                }
                _ => {}
            }
        }

        if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
            let class_name = self.interner.resolve(class.name.name);
            let ctor_dbg = constructor.map(|id| id.as_u32());
            let first_method_dbg = methods.first().map(|m| m.func_id.as_u32());
            eprintln!(
                "[lower] register_class name={} nominal_type_id={} ctor={:?} first_method={:?} methods={}",
                class_name,
                nominal_type_id.as_u32(),
                ctor_dbg,
                first_method_dbg,
                methods.len()
            );
        }

        self.class_info_map.insert(
            nominal_type_id,
            ClassInfo {
                fields,
                methods,
                constructor,
                constructor_params,
                static_fields,
                static_blocks,
                static_methods: static_methods_vec,
                runtime_method_elements,
                parent_class,
                parent_runtime_name,
                parent_constructor_symbol,
                extends_type_subs,
                method_slot_count,
                class_decorators,
                method_decorators,
                field_decorators,
                parameter_decorators,
            },
        );

        self.current_span = saved_span;
    }

    /// Lower a function declaration
    fn lower_function(&mut self, func: &ast::FunctionDecl) -> IrFunction {
        // Track that we're inside a function (prevents var decls from hijacking module globals)
        self.function_depth += 1;
        self.generator_yield_array_local = None;
        let has_js_this_slot = self.js_this_binding_compat;

        // Check if any parameters use destructuring
        let has_destructuring_params = func
            .params
            .iter()
            .any(|p| !matches!(p.pattern, Pattern::Identifier(_) | Pattern::Rest(_)));

        // Reset per-function state
        self.next_register = 0;
        self.next_block = 0;
        self.local_map.clear();
        self.local_registers.clear();
        self.callable_local_hints.clear();
        self.callable_symbol_hints.clear();
        self.register_object_fields.clear();
        self.register_nested_object_fields.clear();
        self.register_array_element_object_fields.clear();
        self.register_nested_array_element_object_fields.clear();
        // IMPORTANT: If there are destructuring parameters, start local allocation AFTER parameter slots
        // to avoid destructured variables overwriting parameter values
        if has_destructuring_params {
            let fixed_param_count =
                func.params.iter().filter(|p| !p.is_rest).count() + usize::from(has_js_this_slot);
            self.next_local = fixed_param_count as u16;
        } else {
            self.next_local = u16::from(has_js_this_slot);
        }
        self.refcell_vars.clear();
        self.refcell_registers.clear();
        self.refcell_inner_types.clear();
        self.loop_captured_vars.clear();
        // Module-level functions do not inherit closure capture state.
        // Without resetting this, stale captures from previously-lowered closures
        // can cause identifiers (e.g. `io`) to resolve via LoadCaptured instead of
        // LoadGlobal, producing invalid receivers at runtime.
        self.ancestor_variables = None;
        self.captures.clear();
        self.next_capture_slot = 0;
        self.this_captured_idx = None;
        self.js_arguments_local = None;
        self.eval_completion_local = None;
        self.parameter_symbols.clear();
        self.visible_js_lexical_symbols.clear();
        // closure_locals maps local-slot indices to async func IDs.  It is
        // strictly per-function: stale entries from a previously-lowered
        // function (e.g. std:math init code registering `wrapped` at slot 2)
        // must not bleed into a subsequent function that happens to allocate
        // the same slot index for an unrelated, non-async local (e.g. `compute`).
        self.closure_locals.clear();

        // Pre-scan to identify captured variables
        let mut locals = FxHashSet::default();
        for param in &func.params {
            collect_pattern_names(&param.pattern, &mut locals);
            collect_pattern_names(&param.pattern, &mut self.parameter_symbols);
        }
        locals.extend(self.collect_local_names(&func.body.statements));
        let arguments_symbol = self.find_js_arguments_symbol(&func.params, &func.body);
        if let Some(arguments_symbol) = arguments_symbol {
            locals.insert(arguments_symbol);
        }
        self.scan_for_captured_vars(&func.body.statements, &func.params, &locals);

        // Get function name
        let name = self.interner.resolve(func.name.name);
        let wrapper_tag = module_wrapper_alias_tag(name);
        self.prepare_executable_body_declarations(&func.body.statements, wrapper_tag);

        // Create parameter registers (excluding rest parameters)
        let mut params = Vec::new();
        let mut rest_param_info = None;
        let mut fixed_param_count = usize::from(has_js_this_slot);
        // Track parameters with destructuring patterns for later binding
        let mut destructure_params: Vec<(usize, &ast::Pattern, Register)> = Vec::new();
        let mut structural_param_bindings: Vec<(Register, TypeId)> = Vec::new();
        let mut param_local_indices = Vec::with_capacity(func.params.len());
        let function_type_params = func.type_params.as_deref();

        if has_js_this_slot {
            let this_reg = self.alloc_register(UNRESOLVED);
            self.this_register = Some(this_reg.clone());
            params.push(this_reg);
        } else {
            self.this_register = None;
        }

        for (decl_param_idx, param) in func.params.iter().enumerate() {
            // Skip rest parameters - they're handled separately
            if param.is_rest {
                // Extract rest parameter info for later processing
                if let Pattern::Identifier(ident) = &param.pattern {
                    let ty = param
                        .type_annotation
                        .as_ref()
                        .map(|t| self.resolve_type_annotation(t))
                        .unwrap_or(UNRESOLVED);

                    rest_param_info = Some((ident.name.clone(), ty));
                }
                continue;
            }

            fixed_param_count += 1;

            let ty = param
                .type_annotation
                .as_ref()
                .map(|t| self.resolve_param_type_from_annotation(t, function_type_params, None))
                .unwrap_or(UNRESOLVED);
            let reg = self.alloc_register(ty);
            if let Some(type_ann) = &param.type_annotation {
                let nominal_type_id = self.resolve_param_nominal_type_from_annotation(
                    type_ann,
                    function_type_params,
                    None,
                );
                let runtime_bound_ctor = nominal_type_id
                    .is_none()
                    .then(|| self.runtime_bound_constructor_from_type_annotation(type_ann))
                    .flatten();
                if nominal_type_id.is_none() && runtime_bound_ctor.is_none() {
                    let expected_ty = self.resolve_param_type_from_annotation(
                        type_ann,
                        function_type_params,
                        None,
                    );
                    if expected_ty != UNRESOLVED {
                        structural_param_bindings.push((reg.clone(), expected_ty));
                        if let Some(layout) =
                            self.structural_projection_layout_from_type_id(expected_ty)
                        {
                            self.register_structural_projection_fields
                                .insert(reg.id, layout.clone());
                        }
                    }
                }
            }

            // Extract parameter name from pattern
            if let Pattern::Identifier(ident) = &param.pattern {
                let local_idx = if has_destructuring_params {
                    let local_idx = decl_param_idx as u16 + u16::from(has_js_this_slot);
                    self.local_map.insert(ident.name, local_idx);
                    local_idx
                } else {
                    self.allocate_local(ident.name)
                };
                self.local_registers.insert(local_idx, reg.clone());
                param_local_indices.push(Some(local_idx));

                // Track class type for parameters with class type annotations
                // so method calls can be statically resolved
                if let Some(type_ann) = &param.type_annotation {
                    let nominal_type_id = self.resolve_param_nominal_type_from_annotation(
                        type_ann,
                        function_type_params,
                        None,
                    );
                    let runtime_bound_ctor = nominal_type_id
                        .is_none()
                        .then(|| self.runtime_bound_constructor_from_type_annotation(type_ann))
                        .flatten();
                    if nominal_type_id.is_none() && runtime_bound_ctor.is_none() {
                        let expected_ty = self.resolve_param_type_from_annotation(
                            type_ann,
                            function_type_params,
                            None,
                        );
                        if let Some(layout) =
                            self.structural_projection_layout_from_type_id(expected_ty)
                        {
                            self.variable_structural_projection_fields
                                .insert(ident.name, layout);
                            self.variable_class_map.remove(&ident.name);
                        } else {
                            self.variable_structural_projection_fields
                                .remove(&ident.name);
                            self.variable_class_map.remove(&ident.name);
                        }
                    } else {
                        self.variable_structural_projection_fields
                            .remove(&ident.name);
                    }
                    if let Some(nominal_type_id) = nominal_type_id {
                        self.variable_class_map.insert(ident.name, nominal_type_id);
                        self.clear_late_bound_object_binding(ident.name);
                    } else if let Some((ctor_symbol, ctor_type)) = runtime_bound_ctor {
                        self.variable_class_map.remove(&ident.name);
                        self.variable_structural_projection_fields
                            .remove(&ident.name);
                        self.mark_late_bound_object_binding(ident.name, ctor_symbol, ctor_type);
                    } else {
                        self.clear_late_bound_object_binding(ident.name);
                    }
                    self.register_variable_type_hints_from_annotation(ident.name, type_ann);
                    if self.type_annotation_is_callable(type_ann) {
                        self.callable_local_hints.insert(local_idx);
                        self.callable_symbol_hints.insert(ident.name);
                    }
                }
            } else {
                // Destructuring pattern: track for later binding after entry block
                param_local_indices.push(None);
                destructure_params.push((decl_param_idx, &param.pattern, reg.clone()));
            }
            params.push(reg);
        }

        // Get return type
        let return_ty = func
            .return_type
            .as_ref()
            .map(|t| self.resolve_type_annotation(t))
            .unwrap_or(UNRESOLVED);
        let visible_length = func
            .params
            .iter()
            .take_while(|param| !param.is_rest && param.default_value.is_none() && !param.optional)
            .count();
        let is_strict_js = self.js_strict_context
            || func
                .body
                .statements
                .iter()
                .take_while(|stmt| {
                    matches!(
                        stmt,
                        Statement::Expression(ast::ExpressionStatement {
                            expression: Expression::StringLiteral(_),
                            ..
                        })
                    )
                })
                .any(|stmt| {
                    matches!(
                        stmt,
                        Statement::Expression(ast::ExpressionStatement {
                            expression: Expression::StringLiteral(lit),
                            ..
                        }) if lit.raw_literal
                            && self.interner.resolve(lit.value) == "use strict"
                    )
                });

        // Create function with fixed parameter count only
        let mut ir_func = IrFunction::new(name, params, return_ty);
        ir_func.uses_js_this_slot = has_js_this_slot;
        ir_func.is_constructible = !func.is_generator;
        ir_func.is_generator = func.is_generator;
        ir_func.visible_length = visible_length;
        ir_func.is_strict_js = is_strict_js;
        if let Some(type_params) = &func.type_params {
            ir_func.type_param_ids = type_params
                .iter()
                .filter_map(|tp| {
                    let param_name = self.interner.resolve(tp.name.name);
                    self.type_ctx.lookup_named_type(param_name)
                })
                .collect();
        }
        if self.emit_sourcemap {
            ir_func.source_span = func.span;
        }
        self.current_function = Some(ir_func);
        let saved_js_strict_context = self.js_strict_context;
        let saved_in_direct_eval_function = self.in_direct_eval_function;
        let saved_parameter_symbols = self.parameter_symbols.clone();
        let saved_visible_js_lexical_symbols = self.visible_js_lexical_symbols.clone();
        let saved_eval_completion_local = self.eval_completion_local;
        let saved_parameter_scope_eval_mode = self.parameter_scope_eval_mode;
        let saved_body_scope_eval_arguments_mode = self.body_scope_eval_arguments_mode;
        let saved_hoisted_function_decl_spans =
            std::mem::take(&mut self.hoisted_function_decl_spans);
        self.js_strict_context = is_strict_js;
        self.in_direct_eval_function = self
            .direct_eval_entry_function
            .as_deref()
            .is_some_and(|target| target == name);
        self.body_scope_eval_arguments_mode = false;

        // Create entry block
        let entry_block = self.alloc_block();
        self.current_block = entry_block;
        self.current_function_mut()
            .add_block(BasicBlock::with_label(entry_block, "entry"));

        if self.in_direct_eval_function {
            self.init_eval_completion_local();
        }

        // Bind destructuring patterns in function parameters
        // This must happen after entry block is created so we can emit instructions
        for (param_idx, pattern, value_reg) in destructure_params {
            // Register object field layout for destructuring
            if let ast::Pattern::Object(_) = pattern {
                if let Some(type_ann) = func
                    .params
                    .get(param_idx)
                    .and_then(|p| p.type_annotation.as_ref())
                {
                    if let Some(field_layout) = self.extract_field_names_from_type(type_ann) {
                        self.register_object_fields
                            .insert(value_reg.id, field_layout);
                    }
                    if let Some(nested_array_layouts) =
                        self.extract_array_element_object_layouts_from_type(type_ann)
                    {
                        for (field_idx, layout) in nested_array_layouts {
                            self.register_nested_array_element_object_fields
                                .insert((value_reg.id, field_idx), layout);
                        }
                    }
                }
            }
            self.bind_pattern(pattern, value_reg);
        }

        // Register structural slot views for typed parameters so slot-based member
        // access works for class/object/interface values uniformly at runtime.
        for (param_reg, expected_ty) in structural_param_bindings {
            if !self.emit_projected_shape_registration_for_register_type(&param_reg, expected_ty) {
                self.emit_structural_slot_registration_for_type(param_reg, expected_ty);
            }
        }

        self.emit_js_arguments_prologue(
            arguments_symbol,
            &func.params,
            &param_local_indices,
            is_strict_js,
        );

        // Emit rest array collection code if present
        if let Some((rest_name, rest_ty)) = rest_param_info {
            self.emit_rest_array_collection(rest_name, rest_ty, fixed_param_count);
        }

        // Emit null-check + default-value for parameters with defaults
        self.emit_default_params(&func.params);
        self.emit_js_function_var_hoists(&func.body.statements);
        self.emit_js_function_captured_lexical_prebindings(&func.body.statements);
        self.emit_js_function_decl_hoists(&func.body.statements);

        if func.is_generator && self.function_uses_generator_snapshot(&func.body) {
            self.init_generator_yield_array();
        }

        // Lower function body
        for stmt in &func.body.statements {
            self.lower_stmt(stmt);
        }

        // Ensure the function ends with a return
        if !self.current_block_is_terminated() {
            self.emit_fallthrough_return();
        }

        // Restore function depth
        self.function_depth -= 1;

        // Take the function out
        let lowered = self.current_function.take().unwrap();
        self.js_strict_context = saved_js_strict_context;
        self.in_direct_eval_function = saved_in_direct_eval_function;
        self.parameter_symbols = saved_parameter_symbols;
        self.visible_js_lexical_symbols = saved_visible_js_lexical_symbols;
        self.eval_completion_local = saved_eval_completion_local;
        self.parameter_scope_eval_mode = saved_parameter_scope_eval_mode;
        self.body_scope_eval_arguments_mode = saved_body_scope_eval_arguments_mode;
        self.hoisted_function_decl_spans = saved_hoisted_function_decl_spans;
        lowered
    }

    /// Lower top-level statements into a main function
    fn lower_top_level_statements(&mut self, stmts: &[&Statement]) -> IrFunction {
        // Reset per-function state
        self.next_register = 0;
        self.next_block = 0;
        self.local_map.clear();
        self.local_registers.clear();
        self.callable_local_hints.clear();
        self.callable_symbol_hints.clear();
        self.next_local = 0;
        self.refcell_vars.clear();
        self.refcell_registers.clear();
        self.refcell_inner_types.clear();
        self.loop_captured_vars.clear();
        self.ancestor_variables = None;
        self.captures.clear();
        self.next_capture_slot = 0;
        self.current_class = None;
        self.this_register = None;
        self.this_ancestor_info = None;
        self.this_captured_idx = None;
        self.js_arguments_local = None;
        self.parameter_symbols.clear();
        self.eval_completion_local = None;
        self.pending_constructor_prologue = None;
        self.current_method_env_globals = None;
        self.pending_class_method_env_globals = None;
        self.hoisted_function_decl_spans.clear();

        // Pre-scan to identify captured variables
        let stmts_owned: Vec<ast::Statement> = stmts.iter().map(|s| (*s).clone()).collect();
        let mut locals = self.collect_local_names(&stmts_owned);
        // Remove module-level globals — they use LoadGlobal/StoreGlobal, not locals
        locals.retain(|name| !self.module_var_globals.contains_key(name));
        locals.retain(|name| !self.js_script_lexical_globals.contains_key(name));
        self.scan_for_captured_vars(&stmts_owned, &[], &locals);

        // Create main function and carry the already-computed module strictness
        // into IR metadata so top-level direct eval inherits it just like any
        // other JS function body.
        let mut ir_func = IrFunction::new("main", vec![], TypeId::new(0));
        ir_func.is_strict_js = self.js_strict_context;
        self.current_function = Some(ir_func);

        // Create entry block
        let entry_block = self.alloc_block();
        self.current_block = entry_block;
        self.current_function_mut()
            .add_block(BasicBlock::with_label(entry_block, "entry"));

        self.emit_js_top_level_var_hoists(stmts);
        self.emit_js_top_level_function_hoists();
        if self.track_top_level_completion {
            self.init_eval_completion_local();
        }

        // Lower statements first (so variable declarations like `let x = 0` are processed)
        for stmt in stmts {
            self.lower_stmt(stmt);
        }

        // Initialize decorators for all classes AFTER statements
        // This ensures variables referenced by decorators are already declared
        self.emit_decorator_initializations();

        // Ensure the function ends with a return
        if !self.current_block_is_terminated() {
            self.emit_fallthrough_return();
        }

        self.generator_yield_array_local = None;

        self.current_function.take().unwrap()
    }

    pub(super) fn emit_js_function_var_hoists(&mut self, statements: &[ast::Statement]) {
        if !self.js_this_binding_compat {
            return;
        }

        let mut var_names = FxHashSet::default();
        collect_function_scoped_var_names(statements, &mut var_names);
        if self.in_direct_eval_function {
            let mut ordered = var_names
                .into_iter()
                .map(|symbol| self.interner.resolve(symbol).to_string())
                .collect::<Vec<_>>();
            ordered.sort();
            for name in ordered {
                let name_reg = self.alloc_register(TypeId::new(STRING_TYPE_ID));
                self.emit(IrInstr::Assign {
                    dest: name_reg.clone(),
                    value: IrValue::Constant(IrConstant::String(name)),
                });
                self.emit(IrInstr::NativeCall {
                    dest: None,
                    native_id: crate::compiler::native_id::OBJECT_EVAL_ENV_DECLARE_VAR,
                    args: vec![name_reg],
                });
            }
            return;
        }

        for name in var_names {
            if self.local_map.contains_key(&name) {
                continue;
            }
            let local_idx = self.allocate_local(name);
            let undefined = self.alloc_register(UNRESOLVED);
            self.emit(IrInstr::Assign {
                dest: undefined.clone(),
                value: IrValue::Constant(IrConstant::Undefined),
            });
            if self.refcell_vars.contains(&name) {
                let refcell_reg = self.alloc_register(TypeId::new(0));
                self.emit(IrInstr::NewRefCell {
                    dest: refcell_reg.clone(),
                    initial_value: undefined,
                });
                self.local_registers.insert(local_idx, refcell_reg.clone());
                self.refcell_registers
                    .insert(local_idx, refcell_reg.clone());
                self.refcell_inner_types.insert(local_idx, UNRESOLVED);
                self.emit(IrInstr::StoreLocal {
                    index: local_idx,
                    value: refcell_reg,
                });
            } else {
                self.local_registers.insert(local_idx, undefined.clone());
                self.emit(IrInstr::StoreLocal {
                    index: local_idx,
                    value: undefined,
                });
            }
        }
    }

    pub(super) fn emit_js_function_captured_lexical_prebindings(
        &mut self,
        statements: &[ast::Statement],
    ) {
        if !self.js_this_binding_compat {
            return;
        }

        let mut local_names = self.collect_local_names(statements);
        let mut var_names = FxHashSet::default();
        collect_function_scoped_var_names(statements, &mut var_names);
        local_names.retain(|name| !var_names.contains(name));

        for name in local_names {
            if !self.refcell_vars.contains(&name) || self.local_map.contains_key(&name) {
                continue;
            }

            let local_idx = self.allocate_local(name);
            let undefined = self.alloc_register(UNRESOLVED);
            self.emit(IrInstr::Assign {
                dest: undefined.clone(),
                value: IrValue::Constant(IrConstant::Undefined),
            });
            let refcell_reg = self.alloc_register(TypeId::new(0));
            self.emit(IrInstr::NewRefCell {
                dest: refcell_reg.clone(),
                initial_value: undefined,
            });
            self.local_registers.insert(local_idx, refcell_reg.clone());
            self.refcell_registers
                .insert(local_idx, refcell_reg.clone());
            self.refcell_inner_types.insert(local_idx, UNRESOLVED);
            self.emit(IrInstr::StoreLocal {
                index: local_idx,
                value: refcell_reg,
            });
        }
    }

    pub(super) fn emit_js_function_decl_hoists(&mut self, statements: &[ast::Statement]) {
        if !self.js_this_binding_compat {
            return;
        }

        for stmt in statements {
            let ast::Statement::FunctionDecl(func_decl) = stmt else {
                continue;
            };
            if self
                .hoisted_function_decl_spans
                .contains(&func_decl.span.start)
            {
                continue;
            }
            self.lower_nested_function_decl_hoist(func_decl);
            self.hoisted_function_decl_spans
                .insert(func_decl.span.start);
        }
    }

    fn emit_js_top_level_var_hoists(&mut self, stmts: &[&Statement]) {
        if !self.js_this_binding_compat {
            return;
        }

        let stmts_owned: Vec<ast::Statement> = stmts.iter().map(|stmt| (*stmt).clone()).collect();
        let mut var_names = FxHashSet::default();
        collect_function_scoped_var_names(&stmts_owned, &mut var_names);
        for name in var_names {
            let Some(&global_idx) = self.module_var_globals.get(&name) else {
                continue;
            };
            let undefined = self.alloc_register(UNRESOLVED);
            self.emit(IrInstr::Assign {
                dest: undefined.clone(),
                value: IrValue::Constant(IrConstant::Undefined),
            });
            self.global_type_map.insert(global_idx, undefined.ty);
            self.emit(IrInstr::StoreGlobal {
                index: global_idx,
                value: undefined.clone(),
            });
            self.emit_js_script_global_binding(name, undefined);
        }
    }

    fn emit_js_top_level_function_hoists(&mut self) {
        if !self.js_this_binding_compat {
            return;
        }

        let hoists = self.js_top_level_function_globals.clone();
        for (name, global_idx, func_id) in hoists {
            let closure = self.alloc_register(TypeId::new(UNKNOWN_TYPE_ID));
            self.emit(IrInstr::MakeClosure {
                dest: closure.clone(),
                func: func_id,
                captures: vec![],
            });
            self.global_type_map.insert(global_idx, closure.ty);
            self.emit(IrInstr::StoreGlobal {
                index: global_idx,
                value: closure.clone(),
            });
            self.emit_js_script_global_binding(name, closure);
        }
    }

    fn emit_js_script_global_binding(&mut self, name: Symbol, value: Register) {
        if !self.js_this_binding_compat || !self.emit_script_global_bindings {
            return;
        }

        let prop_name = self.alloc_register(TypeId::new(STRING_TYPE_ID));
        self.emit(IrInstr::Assign {
            dest: prop_name.clone(),
            value: IrValue::Constant(IrConstant::String(self.interner.resolve(name).to_string())),
        });
        let configurable = self.emit_bool_const(self.script_global_bindings_configurable);
        self.emit(IrInstr::NativeCall {
            dest: None,
            native_id: crate::compiler::native_id::OBJECT_BIND_SCRIPT_GLOBAL,
            args: vec![prop_name, value, configurable],
        });
    }

    /// Lower a class declaration
    fn lower_class(&mut self, class: &ast::ClassDecl) -> IrClass {
        let name = self.interner.resolve(class.name.name);
        let mut ir_class = IrClass::new(name);

        // Get class ID from per-declaration map (safe even when names collide)
        let nominal_type_id = self.nominal_type_id_for_decl(class).unwrap_or_else(|| {
            panic!(
                "ICE: class '{}' at span {} was not pre-registered",
                name, class.span.start
            )
        });
        let class_info = self.class_info_map.get(&nominal_type_id).cloned();
        let runtime_instance_publication = class.members.iter().any(|member| {
            matches!(
                member,
                ast::ClassMember::Method(method)
                    if !method.is_static
                        && method.body.is_some()
                        && matches!(method.name, ast::PropertyKey::Computed(_))
            )
        });
        let runtime_static_publication = class.members.iter().any(|member| {
            matches!(
                member,
                ast::ClassMember::Method(method)
                    if method.is_static
                        && method.body.is_some()
                        && matches!(method.name, ast::PropertyKey::Computed(_))
            )
        });
        ir_class.runtime_instance_publication = runtime_instance_publication;
        ir_class.runtime_static_publication = runtime_static_publication;

        // Set parent class if this class extends another
        if let Some(ref info) = class_info {
            ir_class.parent = info.parent_class;
            ir_class.parent_name = info.parent_runtime_name.clone();
        }

        // Add parent fields first (with their original indices)
        if let Some(ref info) = class_info {
            if let Some(parent_id) = info.parent_class {
                // Recursively get all parent fields
                fn add_parent_fields(
                    lowerer: &Lowerer<'_>,
                    ir_class: &mut IrClass,
                    parent_id: NominalTypeId,
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
        for (member_idx, member) in class.members.iter().enumerate() {
            if let ast::ClassMember::Field(field) = member {
                if !field.is_static {
                    let Some(field_symbol) = self.known_class_member_symbol(&field.name) else {
                        continue;
                    };
                    let field_name = self.class_member_display_name(&field.name, member_idx);
                    let ty = field
                        .type_annotation
                        .as_ref()
                        .map(|t| self.resolve_type_annotation(t))
                        .unwrap_or(TypeId::new(0));
                    // Get the index from class_info since it was computed with parent offset
                    let Some(index) = class_info.as_ref().and_then(|info| {
                        info.fields
                            .iter()
                            .find(|f| f.name == field_symbol)
                            .map(|f| f.index)
                    }) else {
                        self.errors.push(super::error::CompileError::InternalError {
                            message: format!(
                                "missing precomputed field index for '{}.{}'",
                                name, field_name
                            ),
                        });
                        continue;
                    };

                    let mut ir_field = IrField::new(field_name, ty, index);
                    ir_field.readonly = field.is_readonly;
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
                            let Some(index) = class_info.as_ref().and_then(|info| {
                                info.fields
                                    .iter()
                                    .find(|f| f.name == ident.name)
                                    .map(|f| f.index)
                            }) else {
                                self.errors.push(super::error::CompileError::InternalError {
                                    message: format!(
                                        "missing precomputed ctor field index for '{}.{}'",
                                        name,
                                        self.interner.resolve(ident.name)
                                    ),
                                });
                                continue;
                            };
                            let ir_field = IrField::new(field_name, ty, index);
                            ir_class.add_field(ir_field);
                        }
                    }
                }
                break;
            }
        }

        // Lower methods. In JS mode, both instance and static methods carry an
        // implicit `this` slot so member dispatch does not shift user arguments.
        let class_method_env_globals = self.pending_class_method_env_globals.take();
        for (member_idx, member) in class.members.iter().enumerate() {
            if let ast::ClassMember::Method(method) = member {
                // Only lower methods that have a body (not abstract methods)
                if let Some(body) = &method.body {
                    let Some(runtime_method) = class_info
                        .as_ref()
                        .and_then(|info| {
                            info.runtime_method_elements
                                .iter()
                                .find(|elem| elem.order == member_idx)
                        })
                        .cloned()
                    else {
                        continue;
                    };
                    let method_symbol = self.known_class_member_symbol(&method.name);
                    let method_name = self.class_member_display_name(&method.name, member_idx);
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
                    self.callable_local_hints.clear();
                    self.callable_symbol_hints.clear();
                    self.register_object_fields.clear();
                    self.register_nested_object_fields.clear();
                    self.register_array_element_object_fields.clear();
                    self.register_nested_array_element_object_fields.clear();
                    // Reset capture state for this method scope
                    self.refcell_vars.clear();
                    self.refcell_registers.clear();
                    self.refcell_inner_types.clear();
                    self.loop_captured_vars.clear();
                    self.ancestor_variables = None;
                    self.captures.clear();
                    self.next_capture_slot = 0;
                    self.this_captured_idx = None;
                    self.js_arguments_local = None;
                    self.closure_locals.clear();
                    self.current_method_env_globals = class_method_env_globals.clone();

                    // Create parameter registers
                    let mut params = Vec::new();
                    let mut rest_param_info = None;
                    let mut fixed_param_count = 0;

                    let method_has_js_this_slot = self.js_this_binding_compat;
                    if method.is_static {
                        self.current_class = Some(nominal_type_id);
                        self.current_method_is_static = true;
                        if method_has_js_this_slot {
                            let this_reg = self.alloc_register(UNRESOLVED);
                            params.push(this_reg.clone());
                            self.this_register = Some(this_reg);
                            fixed_param_count = 1;
                        } else {
                            self.this_register = None;
                        }
                        // Check if there are destructuring parameters
                        let has_destructuring = method.params.iter().any(|p| {
                            !matches!(p.pattern, Pattern::Identifier(_) | Pattern::Rest(_))
                        });
                        if has_destructuring {
                            let param_count = usize::from(method_has_js_this_slot)
                                + method.params.iter().filter(|p| !p.is_rest).count();
                            self.next_local = param_count as u16;
                        } else if method_has_js_this_slot {
                            self.next_local = 1;
                        }
                    } else {
                        let this_ty = self
                            .type_ctx
                            .lookup_named_type(name)
                            .unwrap_or(TypeId::new(0));
                        self.current_class = Some(nominal_type_id);
                        self.current_method_is_static = false;
                        if method_has_js_this_slot {
                            let this_reg = self.alloc_register(this_ty);
                            params.push(this_reg.clone());
                            self.this_register = Some(this_reg);
                            fixed_param_count = 1;
                        } else {
                            // Legacy non-JS instance methods use an explicit receiver parameter.
                            let this_reg = self.alloc_register(this_ty);
                            params.push(this_reg.clone());
                            self.this_register = Some(this_reg);
                            fixed_param_count = 1;
                        }
                        if let Some(this_sym) = self.interner.lookup("this") {
                            self.variable_class_map.insert(this_sym, nominal_type_id);
                        }
                        let has_destructuring = method.params.iter().any(|p| {
                            !matches!(p.pattern, Pattern::Identifier(_) | Pattern::Rest(_))
                        });
                        if has_destructuring {
                            let param_count =
                                1 + method.params.iter().filter(|p| !p.is_rest).count();
                            self.next_local = param_count as u16;
                        } else {
                            self.next_local = 1;
                        }
                    }

                    // Add explicit parameters (excluding rest parameters)
                    let mut destructure_params: Vec<(usize, &ast::Pattern, Register)> = Vec::new();
                    let mut structural_param_bindings: Vec<(Register, TypeId)> = Vec::new();
                    let mut param_local_indices = Vec::with_capacity(method.params.len());
                    let method_type_params = method.type_params.as_deref();
                    let class_type_params = class.type_params.as_deref();
                    let has_destructuring_params = method
                        .params
                        .iter()
                        .any(|p| !matches!(p.pattern, Pattern::Identifier(_) | Pattern::Rest(_)));

                    for (decl_param_idx, param) in method.params.iter().enumerate() {
                        // Skip rest parameters - they're handled separately
                        if param.is_rest {
                            // Extract rest parameter info for later processing
                            if let Pattern::Identifier(ident) = &param.pattern {
                                let ty = param
                                    .type_annotation
                                    .as_ref()
                                    .map(|t| {
                                        self.resolve_param_type_from_annotation(
                                            t,
                                            method_type_params,
                                            class_type_params,
                                        )
                                    })
                                    .unwrap_or(TypeId::new(ARRAY_TYPE_ID));
                                rest_param_info = Some((ident.name.clone(), ty));
                            }
                            continue;
                        }

                        fixed_param_count += 1;

                        let ty = param
                            .type_annotation
                            .as_ref()
                            .map(|t| {
                                self.resolve_param_type_from_annotation(
                                    t,
                                    method_type_params,
                                    class_type_params,
                                )
                            })
                            .unwrap_or(UNRESOLVED);
                        let reg = self.alloc_register(ty);
                        if let Some(type_ann) = &param.type_annotation {
                            let nominal_type_id = self.resolve_param_nominal_type_from_annotation(
                                type_ann,
                                method_type_params,
                                class_type_params,
                            );
                            let runtime_bound_ctor = nominal_type_id
                                .is_none()
                                .then(|| {
                                    self.runtime_bound_constructor_from_type_annotation(type_ann)
                                })
                                .flatten();
                            if nominal_type_id.is_none() && runtime_bound_ctor.is_none() {
                                let expected_ty = self.resolve_param_type_from_annotation(
                                    type_ann,
                                    method_type_params,
                                    class_type_params,
                                );
                                if expected_ty != UNRESOLVED {
                                    structural_param_bindings.push((reg.clone(), expected_ty));
                                    if let Some(layout) =
                                        self.structural_projection_layout_from_type_id(expected_ty)
                                    {
                                        self.register_structural_projection_fields
                                            .insert(reg.id, layout.clone());
                                    }
                                }
                            }
                        }

                        if let Pattern::Identifier(ident) = &param.pattern {
                            let local_idx = if has_destructuring_params {
                                let local_idx =
                                    decl_param_idx as u16 + u16::from(self.this_register.is_some());
                                self.local_map.insert(ident.name, local_idx);
                                local_idx
                            } else {
                                self.allocate_local(ident.name)
                            };
                            self.local_registers.insert(local_idx, reg.clone());
                            param_local_indices.push(Some(local_idx));

                            // Track class type for parameters with class type annotations
                            if let Some(type_ann) = &param.type_annotation {
                                let param_nominal_type_id = self
                                    .resolve_param_nominal_type_from_annotation(
                                        type_ann,
                                        method_type_params,
                                        class_type_params,
                                    );
                                let runtime_bound_ctor = param_nominal_type_id
                                    .is_none()
                                    .then(|| {
                                        self.runtime_bound_constructor_from_type_annotation(
                                            type_ann,
                                        )
                                    })
                                    .flatten();
                                if param_nominal_type_id.is_none() && runtime_bound_ctor.is_none() {
                                    let expected_ty = self.resolve_param_type_from_annotation(
                                        type_ann,
                                        method_type_params,
                                        class_type_params,
                                    );
                                    if let Some(layout) =
                                        self.structural_projection_layout_from_type_id(expected_ty)
                                    {
                                        self.variable_structural_projection_fields
                                            .insert(ident.name, layout);
                                        self.variable_class_map.remove(&ident.name);
                                    } else {
                                        self.variable_structural_projection_fields
                                            .remove(&ident.name);
                                        self.variable_class_map.remove(&ident.name);
                                    }
                                } else {
                                    self.variable_structural_projection_fields
                                        .remove(&ident.name);
                                }
                                if let Some(param_nominal_type_id) = param_nominal_type_id {
                                    self.variable_class_map
                                        .insert(ident.name, param_nominal_type_id);
                                    self.clear_late_bound_object_binding(ident.name);
                                } else if let Some((ctor_symbol, ctor_type)) = runtime_bound_ctor {
                                    self.variable_class_map.remove(&ident.name);
                                    self.variable_structural_projection_fields
                                        .remove(&ident.name);
                                    self.mark_late_bound_object_binding(
                                        ident.name,
                                        ctor_symbol,
                                        ctor_type,
                                    );
                                } else {
                                    self.clear_late_bound_object_binding(ident.name);
                                }
                                self.register_variable_type_hints_from_annotation(
                                    ident.name, type_ann,
                                );
                                if self.type_annotation_is_callable(type_ann) {
                                    self.callable_local_hints.insert(local_idx);
                                    self.callable_symbol_hints.insert(ident.name);
                                }
                            }
                        } else {
                            // Destructuring pattern: track for later binding after entry block
                            param_local_indices.push(None);
                            destructure_params.push((decl_param_idx, &param.pattern, reg.clone()));
                        }
                        params.push(reg);
                    }

                    // Get return type
                    let return_ty = method
                        .return_type
                        .as_ref()
                        .map(|t| self.resolve_type_annotation(t))
                        .unwrap_or(UNRESOLVED);
                    let visible_length = method
                        .params
                        .iter()
                        .take_while(|param| {
                            !param.is_rest && param.default_value.is_none() && !param.optional
                        })
                        .count();

                    // Create function with mangled name
                    let mut ir_func = IrFunction::new(&full_name, params, return_ty);
                    ir_func.uses_js_this_slot = method_has_js_this_slot;
                    ir_func.is_constructible = false;
                    ir_func.is_generator = method.is_generator;
                    ir_func.visible_length = visible_length;
                    ir_func.is_strict_js = true;
                    ir_func.uses_builtin_this_coercion = self.builtin_this_coercion_compat;
                    let mut type_param_ids = Vec::new();
                    if let Some(class_type_params) = &class.type_params {
                        for tp in class_type_params {
                            let param_name = self.interner.resolve(tp.name.name);
                            if let Some(id) = self.type_ctx.lookup_named_type(param_name) {
                                type_param_ids.push(id);
                            }
                        }
                    }
                    if let Some(method_type_params) = &method.type_params {
                        for tp in method_type_params {
                            let param_name = self.interner.resolve(tp.name.name);
                            if let Some(id) = self.type_ctx.lookup_named_type(param_name) {
                                if !type_param_ids.contains(&id) {
                                    type_param_ids.push(id);
                                }
                            }
                        }
                    }
                    ir_func.type_param_ids = type_param_ids;
                    if self.emit_sourcemap {
                        ir_func.source_span = method.span;
                    }
                    self.current_function = Some(ir_func);
                    let saved_js_strict_context = self.js_strict_context;
                    let saved_in_direct_eval_function = self.in_direct_eval_function;
                    let saved_generator_yield_array_local = self.generator_yield_array_local.take();
                    self.js_strict_context = true;
                    self.in_direct_eval_function = false;
                    let arguments_symbol = self.find_js_arguments_symbol(&method.params, body);

                    {
                        let mut method_locals = FxHashSet::default();
                        for param in &method.params {
                            collect_pattern_names(&param.pattern, &mut method_locals);
                        }
                        method_locals.extend(self.collect_local_names(&body.statements));
                        if let Some(arguments_symbol) = arguments_symbol {
                            method_locals.insert(arguments_symbol);
                        }
                        self.scan_for_captured_vars(
                            &body.statements,
                            &method.params,
                            &method_locals,
                        );
                    }

                    // Create entry block
                    let entry_block = self.alloc_block();
                    self.current_block = entry_block;
                    self.current_function_mut()
                        .add_block(BasicBlock::with_label(entry_block, "entry"));

                    // Bind destructuring patterns in method parameters
                    // This must happen after entry block is created so we can emit instructions
                    for (param_idx, pattern, value_reg) in destructure_params {
                        // Register object field layout for destructuring
                        if let ast::Pattern::Object(_) = pattern {
                            if let Some(type_ann) = method
                                .params
                                .get(param_idx)
                                .and_then(|p| p.type_annotation.as_ref())
                            {
                                if let Some(field_layout) =
                                    self.extract_field_names_from_type(type_ann)
                                {
                                    self.register_object_fields
                                        .insert(value_reg.id, field_layout);
                                }
                                if let Some(nested_array_layouts) =
                                    self.extract_array_element_object_layouts_from_type(type_ann)
                                {
                                    for (field_idx, layout) in nested_array_layouts {
                                        self.register_nested_array_element_object_fields
                                            .insert((value_reg.id, field_idx), layout);
                                    }
                                }
                            }
                        }
                        self.bind_pattern(pattern, value_reg);
                    }

                    for (param_reg, expected_ty) in structural_param_bindings {
                        if !self.emit_projected_shape_registration_for_register_type(
                            &param_reg,
                            expected_ty,
                        ) {
                            self.emit_structural_slot_registration_for_type(param_reg, expected_ty);
                        }
                    }

                    self.emit_js_arguments_prologue(
                        arguments_symbol,
                        &method.params,
                        &param_local_indices,
                        true,
                    );

                    // Emit rest array collection code if present
                    if let Some((rest_name, rest_ty)) = rest_param_info {
                        self.emit_rest_array_collection(rest_name, rest_ty, fixed_param_count);
                    }

                    // Emit null-check + default-value for parameters with defaults
                    self.emit_default_params(&method.params);
                    self.emit_js_function_var_hoists(&body.statements);
                    self.emit_js_function_captured_lexical_prebindings(&body.statements);
                    self.emit_js_function_decl_hoists(&body.statements);

                    if method.is_generator && self.function_uses_generator_snapshot(body) {
                        self.init_generator_yield_array();
                    }

                    // Lower method body
                    for stmt in &body.statements {
                        self.lower_stmt(stmt);
                    }

                    // Ensure the function ends with a return
                    if !self.current_block_is_terminated() {
                        self.emit_fallthrough_return();
                    }

                    // Get the function ID and add to pending methods
                    let method_name_str = method_name.as_str();
                    let func_id = if let Some(method_symbol) = method_symbol {
                        if method.is_static {
                            *self.static_method_map.get(&(nominal_type_id, method_symbol))
                                .unwrap_or_else(|| panic!(
                                    "ICE: static method '{}::{}' not found in static_method_map (nominal_type_id={})",
                                    name, method_name_str, nominal_type_id.as_u32()
                                ))
                        } else {
                            *self
                                .method_map
                                .get(&(nominal_type_id, method_symbol))
                                .unwrap_or_else(|| {
                                    panic!(
                                        "ICE: method '{}::{}' not found in method_map (nominal_type_id={})",
                                        name,
                                        method_name_str,
                                        nominal_type_id.as_u32()
                                    )
                                })
                        }
                    } else {
                        runtime_method.func_id
                    };
                    let ir_func = self.current_function.take().unwrap();
                    self.pending_arrow_functions
                        .push((func_id.as_u32(), ir_func));
                    self.js_strict_context = saved_js_strict_context;
                    self.in_direct_eval_function = saved_in_direct_eval_function;
                    self.generator_yield_array_local = saved_generator_yield_array_local;

                    let ir_method_kind = match method.kind {
                        ast::MethodKind::Normal => IrMethodKind::Normal,
                        ast::MethodKind::Getter => IrMethodKind::Getter,
                        ast::MethodKind::Setter => IrMethodKind::Setter,
                    };

                    // Add methods to the IR class metadata so runtime registration
                    // can materialize both prototype and constructor members.
                    if method.is_static {
                        if method_symbol.is_some() {
                            ir_class.add_static_method(func_id, ir_method_kind);
                        }
                    } else {
                        if let Some(method_symbol) = method_symbol {
                            if let Some(&slot) =
                                self.method_slot_map.get(&(nominal_type_id, method_symbol))
                            {
                                ir_class.add_method_with_slot(func_id, slot, ir_method_kind);
                            } else {
                                ir_class.add_method(func_id, ir_method_kind);
                            }
                        }
                    }

                    // Clear method context
                    self.current_class = None;
                    self.current_method_is_static = false;
                    self.this_register = None;
                    if let Some(this_sym) = self.interner.lookup("this") {
                        self.variable_class_map.remove(&this_sym);
                    }
                    self.current_method_env_globals = None;
                }
            }
        }

        // Lower constructor if present
        let mut explicit_ctor_lowered = false;
        for member in &class.members {
            if let ast::ClassMember::Constructor(ctor) = member {
                explicit_ctor_lowered = true;
                let full_name = format!("{}::constructor", name);

                // Reset per-function state
                self.next_register = 0;
                self.next_block = 0;
                self.next_local = 0;
                self.local_map.clear();
                self.local_registers.clear();
                self.callable_local_hints.clear();
                self.callable_symbol_hints.clear();
                self.register_object_fields.clear();
                self.register_nested_object_fields.clear();
                self.register_array_element_object_fields.clear();
                self.register_nested_array_element_object_fields.clear();
                // Reset capture state for constructor scope
                self.refcell_vars.clear();
                self.refcell_registers.clear();
                self.refcell_inner_types.clear();
                self.loop_captured_vars.clear();
                self.ancestor_variables = None;
                self.captures.clear();
                self.next_capture_slot = 0;
                self.this_captured_idx = None;
                self.js_arguments_local = None;
                self.closure_locals.clear();
                self.current_method_env_globals = class_method_env_globals.clone();

                // Set current class context for 'this' handling
                self.current_class = Some(nominal_type_id);
                self.current_method_is_static = false;

                // Create parameter registers - 'this' is the first parameter
                let mut params = Vec::new();

                // Add 'this' as the first parameter
                // Reserve local slot 0 for 'this'
                let this_ty = self
                    .type_ctx
                    .lookup_named_type(name)
                    .unwrap_or(TypeId::new(0));
                let this_reg = self.alloc_register(this_ty);
                params.push(this_reg.clone());
                self.this_register = Some(this_reg);
                if let Some(this_sym) = self.interner.lookup("this") {
                    self.variable_class_map.insert(this_sym, nominal_type_id);
                }
                self.next_local = 1; // Explicit parameters start at slot 1

                // Add explicit parameters from constructor
                let mut destructure_params: Vec<(usize, &ast::Pattern, Register)> = Vec::new();
                let mut structural_param_bindings: Vec<(Register, TypeId)> = Vec::new();
                let mut rest_param_info = None;
                let mut fixed_param_count = 1usize;
                let mut param_local_indices = Vec::with_capacity(ctor.params.len());
                let class_type_params = class.type_params.as_deref();
                let has_destructuring_params = ctor
                    .params
                    .iter()
                    .any(|p| !matches!(p.pattern, Pattern::Identifier(_) | Pattern::Rest(_)));
                if has_destructuring_params {
                    let fixed_param_slots =
                        ctor.params.iter().filter(|p| !p.is_rest).count() + 1usize;
                    self.next_local = fixed_param_slots as u16;
                }

                for (decl_param_idx, param) in ctor.params.iter().enumerate() {
                    if param.is_rest {
                        let rest_ident = match &param.pattern {
                            Pattern::Identifier(ident) => Some(ident.name),
                            Pattern::Rest(rest) => match rest.argument.as_ref() {
                                Pattern::Identifier(ident) => Some(ident.name),
                                _ => None,
                            },
                            _ => None,
                        };
                        if let Some(rest_name) = rest_ident {
                            let ty = param
                                .type_annotation
                                .as_ref()
                                .map(|t| {
                                    self.resolve_param_type_from_annotation(
                                        t,
                                        class_type_params,
                                        None,
                                    )
                                })
                                .unwrap_or(TypeId::new(crate::parser::TypeContext::ARRAY_TYPE_ID));
                            rest_param_info = Some((rest_name, ty));
                        }
                        continue;
                    }

                    fixed_param_count += 1;

                    let ty = param
                        .type_annotation
                        .as_ref()
                        .map(|t| {
                            self.resolve_param_type_from_annotation(t, class_type_params, None)
                        })
                        .unwrap_or(UNRESOLVED);
                    let reg = self.alloc_register(ty);
                    if let Some(type_ann) = &param.type_annotation {
                        let param_nominal_type_id = self
                            .resolve_param_nominal_type_from_annotation(
                                type_ann,
                                class_type_params,
                                None,
                            );
                        let runtime_bound_ctor = param_nominal_type_id
                            .is_none()
                            .then(|| self.runtime_bound_constructor_from_type_annotation(type_ann))
                            .flatten();
                        if param_nominal_type_id.is_none() && runtime_bound_ctor.is_none() {
                            let expected_ty = self.resolve_param_type_from_annotation(
                                type_ann,
                                class_type_params,
                                None,
                            );
                            if expected_ty != UNRESOLVED {
                                structural_param_bindings.push((reg.clone(), expected_ty));
                                if let Some(layout) =
                                    self.structural_projection_layout_from_type_id(expected_ty)
                                {
                                    self.register_structural_projection_fields
                                        .insert(reg.id, layout.clone());
                                }
                            }
                        }
                    }

                    if let Pattern::Identifier(ident) = &param.pattern {
                        let local_idx = if has_destructuring_params {
                            let local_idx =
                                decl_param_idx as u16 + u16::from(self.this_register.is_some());
                            self.local_map.insert(ident.name, local_idx);
                            local_idx
                        } else {
                            self.allocate_local(ident.name)
                        };
                        self.local_registers.insert(local_idx, reg.clone());
                        param_local_indices.push(Some(local_idx));
                        if let Some(type_ann) = &param.type_annotation {
                            let param_nominal_type_id = self
                                .resolve_param_nominal_type_from_annotation(
                                    type_ann,
                                    class_type_params,
                                    None,
                                );
                            let runtime_bound_ctor = param_nominal_type_id
                                .is_none()
                                .then(|| {
                                    self.runtime_bound_constructor_from_type_annotation(type_ann)
                                })
                                .flatten();
                            if param_nominal_type_id.is_none() && runtime_bound_ctor.is_none() {
                                let expected_ty = self.resolve_param_type_from_annotation(
                                    type_ann,
                                    class_type_params,
                                    None,
                                );
                                if let Some(layout) =
                                    self.structural_projection_layout_from_type_id(expected_ty)
                                {
                                    self.variable_structural_projection_fields
                                        .insert(ident.name, layout);
                                    self.variable_class_map.remove(&ident.name);
                                } else {
                                    self.variable_structural_projection_fields
                                        .remove(&ident.name);
                                    self.variable_class_map.remove(&ident.name);
                                }
                            } else {
                                self.variable_structural_projection_fields
                                    .remove(&ident.name);
                            }
                            if let Some(param_nominal_type_id) = param_nominal_type_id {
                                self.variable_class_map
                                    .insert(ident.name, param_nominal_type_id);
                                self.clear_late_bound_object_binding(ident.name);
                            } else if let Some((ctor_symbol, ctor_type)) = runtime_bound_ctor {
                                self.variable_class_map.remove(&ident.name);
                                self.variable_structural_projection_fields
                                    .remove(&ident.name);
                                self.mark_late_bound_object_binding(
                                    ident.name,
                                    ctor_symbol,
                                    ctor_type,
                                );
                            } else {
                                self.clear_late_bound_object_binding(ident.name);
                            }
                            self.register_variable_type_hints_from_annotation(ident.name, type_ann);
                            if self.type_annotation_is_callable(type_ann) {
                                self.callable_local_hints.insert(local_idx);
                                self.callable_symbol_hints.insert(ident.name);
                            }
                        }
                    } else {
                        // Destructuring pattern: track for later binding after entry block
                        param_local_indices.push(None);
                        destructure_params.push((decl_param_idx, &param.pattern, reg.clone()));
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
                let visible_length = ctor
                    .params
                    .iter()
                    .take_while(|param| {
                        !param.is_rest && param.default_value.is_none() && !param.optional
                    })
                    .count();

                // Create function with mangled name
                let mut ir_func = IrFunction::new(&full_name, params, return_ty);
                ir_func.is_constructible = true;
                ir_func.visible_length = visible_length;
                ir_func.is_strict_js = true;
                if let Some(class_type_params) = &class.type_params {
                    ir_func.type_param_ids = class_type_params
                        .iter()
                        .filter_map(|tp| {
                            let param_name = self.interner.resolve(tp.name.name);
                            self.type_ctx.lookup_named_type(param_name)
                        })
                        .collect();
                }
                if self.emit_sourcemap {
                    ir_func.source_span = ctor.span;
                }
                self.current_function = Some(ir_func);
                let saved_js_strict_context = self.js_strict_context;
                let saved_in_direct_eval_function = self.in_direct_eval_function;
                self.js_strict_context = true;
                self.in_direct_eval_function = false;
                let arguments_symbol = self.find_js_arguments_symbol(&ctor.params, &ctor.body);

                {
                    let mut ctor_locals = FxHashSet::default();
                    for param in &ctor.params {
                        collect_pattern_names(&param.pattern, &mut ctor_locals);
                    }
                    ctor_locals.extend(self.collect_local_names(&ctor.body.statements));
                    if let Some(arguments_symbol) = arguments_symbol {
                        ctor_locals.insert(arguments_symbol);
                    }
                    self.scan_for_captured_vars(&ctor.body.statements, &ctor.params, &ctor_locals);
                }

                // Create entry block
                let entry_block = self.alloc_block();
                self.current_block = entry_block;
                self.current_function_mut()
                    .add_block(BasicBlock::with_label(entry_block, "entry"));

                // Bind destructuring patterns in constructor parameters
                // This must happen after entry block is created so we can emit instructions
                for (param_idx, pattern, value_reg) in destructure_params {
                    // Register object field layout for destructuring
                    if let ast::Pattern::Object(_) = pattern {
                        if let Some(type_ann) = ctor
                            .params
                            .get(param_idx)
                            .and_then(|p| p.type_annotation.as_ref())
                        {
                            if let Some(field_layout) = self.extract_field_names_from_type(type_ann)
                            {
                                self.register_object_fields
                                    .insert(value_reg.id, field_layout);
                            }
                            if let Some(nested_array_layouts) =
                                self.extract_array_element_object_layouts_from_type(type_ann)
                            {
                                for (field_idx, layout) in nested_array_layouts {
                                    self.register_nested_array_element_object_fields
                                        .insert((value_reg.id, field_idx), layout);
                                }
                            }
                        }
                    }
                    self.bind_pattern(pattern, value_reg);
                }

                for (param_reg, expected_ty) in structural_param_bindings {
                    if !self.emit_projected_shape_registration_for_register_type(
                        &param_reg,
                        expected_ty,
                    ) {
                        self.emit_structural_slot_registration_for_type(param_reg, expected_ty);
                    }
                }

                self.emit_js_arguments_prologue(
                    arguments_symbol,
                    &ctor.params,
                    &param_local_indices,
                    true,
                );

                if let Some((rest_name, rest_ty)) = rest_param_info {
                    self.emit_rest_array_collection(rest_name, rest_ty, fixed_param_count);
                }

                // Emit null-check + default-value for constructor parameters with defaults
                self.emit_default_params(&ctor.params);
                self.emit_js_function_var_hoists(&ctor.body.statements);
                self.emit_js_function_captured_lexical_prebindings(&ctor.body.statements);
                self.emit_js_function_decl_hoists(&ctor.body.statements);

                let this_reg = self.this_register.clone().unwrap();
                let saved_constructor_return_this = self.constructor_return_this.take();
                self.constructor_return_this = Some(this_reg.clone());
                let mut param_property_fields: Vec<(u16, Register)> = Vec::new();
                for (param_name, param_reg) in &param_prop_regs {
                    let field_name_str = self.interner.resolve(*param_name);
                    let all_fields = self.get_all_fields(nominal_type_id);
                    if let Some(fi) = all_fields
                        .iter()
                        .find(|f| self.interner.resolve(f.name) == field_name_str)
                    {
                        param_property_fields.push((fi.index, param_reg.clone()));
                    }
                }
                let is_derived = self
                    .class_info_map
                    .get(&nominal_type_id)
                    .and_then(|info| info.parent_class)
                    .is_some();
                if is_derived {
                    self.pending_constructor_prologue = Some(PendingConstructorPrologue {
                        nominal_type_id,
                        this_reg: this_reg.clone(),
                        param_properties: param_property_fields,
                    });
                } else {
                    self.emit_constructor_prologue(
                        nominal_type_id,
                        &this_reg,
                        &param_property_fields,
                    );
                }

                // Lower constructor body
                for stmt in &ctor.body.statements {
                    self.lower_stmt(stmt);
                }

                self.emit_pending_constructor_prologue_if_needed();

                // Ensure the function ends with a return
                if !self.current_block_is_terminated() {
                    self.set_terminator(Terminator::Return(Some(this_reg.clone())));
                }

                // Get the constructor function ID from class_info and add to pending functions
                if let Some(class_info) = self.class_info_map.get(&nominal_type_id) {
                    if let Some(ctor_func_id) = class_info.constructor {
                        let ir_func = self.current_function.take().unwrap();
                        self.pending_arrow_functions
                            .push((ctor_func_id.as_u32(), ir_func));
                    }
                }
                self.js_strict_context = saved_js_strict_context;
                self.in_direct_eval_function = saved_in_direct_eval_function;
                self.constructor_return_this = saved_constructor_return_this;

                // Clear method context
                self.current_class = None;
                self.current_method_is_static = false;
                self.this_register = None;
                self.pending_constructor_prologue = None;
                if let Some(this_sym) = self.interner.lookup("this") {
                    self.variable_class_map.remove(&this_sym);
                }
                self.current_method_env_globals = None;
                break; // Only one constructor
            }
        }

        // Emit implicit constructor when the class omits one.
        if !explicit_ctor_lowered {
            let ctor_func_id = self
                .class_info_map
                .get(&nominal_type_id)
                .and_then(|info| info.constructor)
                .unwrap_or_else(|| {
                    panic!(
                        "ICE: missing constructor function id for class '{}' (nominal_type_id={})",
                        name,
                        nominal_type_id.as_u32()
                    )
                });

            // Reset per-function state
            self.next_register = 0;
            self.next_block = 0;
            self.next_local = 0;
            self.local_map.clear();
            self.local_registers.clear();
            self.callable_local_hints.clear();
            self.callable_symbol_hints.clear();
            self.register_object_fields.clear();
            self.register_nested_object_fields.clear();
            self.register_array_element_object_fields.clear();
            self.register_nested_array_element_object_fields.clear();
            self.refcell_vars.clear();
            self.refcell_registers.clear();
            self.refcell_inner_types.clear();
            self.loop_captured_vars.clear();
            self.ancestor_variables = None;
            self.captures.clear();
            self.next_capture_slot = 0;
            self.this_captured_idx = None;
            self.closure_locals.clear();
            self.current_method_env_globals = class_method_env_globals.clone();

            self.current_class = Some(nominal_type_id);
            self.current_method_is_static = false;

            let parent_constructor_params = self
                .class_info_map
                .get(&nominal_type_id)
                .and_then(|info| info.parent_class)
                .and_then(|parent_id| self.class_info_map.get(&parent_id))
                .map(|info| info.constructor_params.clone())
                .unwrap_or_default();
            let has_parent_constructor =
                self.class_info_map
                    .get(&nominal_type_id)
                    .is_some_and(|info| {
                        info.parent_class.is_some() || info.parent_constructor_symbol.is_some()
                    });
            let forwarded_param_count = if has_parent_constructor {
                parent_constructor_params.len().max(8)
            } else {
                0
            };

            // Implicit derived constructors need forwarding slots even when the
            // parent comes from imported/builtin metadata that does not expose
            // parameter details. Reserve a small variadic prefix for `super(...)`.
            let this_ty = self
                .type_ctx
                .lookup_named_type(name)
                .unwrap_or(TypeId::new(0));
            let this_reg = self.alloc_register(this_ty);
            self.this_register = Some(this_reg.clone());
            if let Some(this_sym) = self.interner.lookup("this") {
                self.variable_class_map.insert(this_sym, nominal_type_id);
            }
            let mut params = vec![this_reg.clone()];
            let mut forwarded_args = Vec::with_capacity(forwarded_param_count + 1);
            forwarded_args.push(this_reg.clone());
            for _ in 0..forwarded_param_count {
                let arg_reg = self.alloc_register(TypeId::new(UNKNOWN_TYPE_ID));
                forwarded_args.push(arg_reg.clone());
                params.push(arg_reg);
            }
            let mut ir_func =
                IrFunction::new(&format!("{}::constructor", name), params, TypeId::new(0));
            ir_func.is_constructible = true;
            ir_func.visible_length = if parent_constructor_params.is_empty() {
                0
            } else {
                parent_constructor_params
                    .iter()
                    .take_while(|param| param.default_value.is_none())
                    .count()
            };
            ir_func.is_strict_js = true;
            if self.emit_sourcemap {
                ir_func.source_span = class.span;
            }
            self.current_function = Some(ir_func);
            let saved_js_strict_context = self.js_strict_context;
            let saved_constructor_return_this = self.constructor_return_this.take();
            self.js_strict_context = true;

            let entry_block = self.alloc_block();
            self.current_block = entry_block;
            self.current_function_mut()
                .add_block(BasicBlock::with_label(entry_block, "entry"));

            let this_reg = self.this_register.clone().unwrap();
            self.constructor_return_this = Some(this_reg.clone());
            let parent_info = self
                .class_info_map
                .get(&nominal_type_id)
                .map(|info| (info.parent_class, info.parent_constructor_symbol));
            let parent_constructor = parent_info.and_then(|(parent_class, parent_symbol)| {
                parent_class
                    .map(|parent_id| self.load_class_value_for_nominal_type(parent_id))
                    .or_else(|| {
                        parent_symbol.map(|symbol| self.lower_runtime_constructor_symbol(symbol))
                    })
            });
            if let Some(parent_constructor) = parent_constructor {
                let new_target = self.load_class_value_for_nominal_type(nominal_type_id);
                let mut native_args = Vec::with_capacity(forwarded_args.len() + 2);
                native_args.push(this_reg.clone());
                native_args.push(parent_constructor);
                native_args.push(new_target);
                native_args.extend(forwarded_args.into_iter().skip(1));
                self.emit(IrInstr::NativeCall {
                    dest: Some(this_reg.clone()),
                    native_id: crate::compiler::native_id::OBJECT_SUPER_CONSTRUCT,
                    args: native_args,
                });
            }
            self.emit_constructor_prologue(nominal_type_id, &this_reg, &[]);
            self.set_terminator(Terminator::Return(Some(this_reg.clone())));

            let ir_func = self.current_function.take().unwrap();
            self.pending_arrow_functions
                .push((ctor_func_id.as_u32(), ir_func));
            self.js_strict_context = saved_js_strict_context;
            self.constructor_return_this = saved_constructor_return_this;

            self.current_class = None;
            self.current_method_is_static = false;
            self.this_register = None;
            self.pending_constructor_prologue = None;
            if let Some(this_sym) = self.interner.lookup("this") {
                self.variable_class_map.remove(&this_sym);
            }
            self.current_method_env_globals = None;
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

                    let field = IrTypeAliasField::new(field_name, ty, prop.optional);
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

    /// Emit rest parameter array collection code.
    /// Collects extra arguments beyond the fixed parameters into an array.
    /// Must be called after entry block creation and parameter registration,
    /// before default params (rest params can't have defaults).
    fn emit_rest_array_collection(
        &mut self,
        rest_name: Symbol,
        rest_array_ty: TypeId,
        fixed_param_count: usize,
    ) {
        // Extract element type from array type
        let elem_ty = if let Some(Type::Array(arr_ty)) = self.type_ctx.get(rest_array_ty) {
            arr_ty.element
        } else {
            // Should not happen if type checker worked correctly
            rest_array_ty // Fallback
        };

        // Get the actual argument count at runtime
        let arg_count_reg = self.alloc_register(TypeId::new(INT_TYPE_ID));
        self.emit(IrInstr::LoadArgCount {
            dest: arg_count_reg.clone(),
        });

        // Clamp rest count at zero so omitted fixed params don't underflow into a huge rest array.
        let fixed_count_val = IrValue::Constant(IrConstant::I32(fixed_param_count as i32));
        let fixed_count_reg = self.alloc_register(TypeId::new(INT_TYPE_ID));
        self.emit(IrInstr::Assign {
            dest: fixed_count_reg.clone(),
            value: fixed_count_val,
        });
        let rest_count_reg = self.alloc_register(TypeId::new(INT_TYPE_ID));
        self.emit(IrInstr::Assign {
            dest: rest_count_reg.clone(),
            value: IrValue::Constant(IrConstant::I32(0)),
        });
        let has_rest_reg = self.alloc_register(TypeId::new(BOOL_TYPE_ID));
        self.emit(IrInstr::BinaryOp {
            dest: has_rest_reg.clone(),
            op: BinaryOp::Greater,
            left: arg_count_reg.clone(),
            right: fixed_count_reg.clone(),
        });
        let rest_count_compute_block = self.alloc_block();
        let rest_count_ready_block = self.alloc_block();
        self.set_terminator(Terminator::Branch {
            cond: has_rest_reg,
            then_block: rest_count_compute_block,
            else_block: rest_count_ready_block,
        });

        self.current_function_mut()
            .add_block(BasicBlock::with_label(
                rest_count_compute_block,
                "rest.count.compute",
            ));
        self.current_block = rest_count_compute_block;
        self.emit(IrInstr::BinaryOp {
            dest: rest_count_reg.clone(),
            op: BinaryOp::Sub,
            left: arg_count_reg,
            right: fixed_count_reg.clone(),
        });
        self.set_terminator(Terminator::Jump(rest_count_ready_block));

        self.current_function_mut()
            .add_block(BasicBlock::with_label(
                rest_count_ready_block,
                "rest.count.ready",
            ));
        self.current_block = rest_count_ready_block;

        // Create array with rest_count size
        let array_reg = self.alloc_register(rest_array_ty);
        self.emit(IrInstr::NewArray {
            dest: array_reg.clone(),
            len: rest_count_reg.clone(),
            elem_ty,
        });

        // Fill array with extra arguments using a loop
        let loop_idx_reg = self.alloc_register(TypeId::new(INT_TYPE_ID));
        let init_block = self.alloc_block();
        let cond_block = self.alloc_block();
        let body_block = self.alloc_block();
        let exit_block = self.alloc_block();

        // Initialize loop index to 0
        self.emit(IrInstr::Assign {
            dest: loop_idx_reg.clone(),
            value: IrValue::Constant(IrConstant::I32(0)),
        });
        self.set_terminator(Terminator::Jump(init_block));

        // Condition block: loop_idx < rest_count
        self.current_function_mut()
            .add_block(BasicBlock::with_label(init_block, "rest.init"));
        self.current_block = init_block;

        let cond_reg = self.alloc_register(TypeId::new(BOOL_TYPE_ID));
        self.emit(IrInstr::BinaryOp {
            dest: cond_reg.clone(),
            op: BinaryOp::Less,
            left: loop_idx_reg.clone(),
            right: rest_count_reg.clone(),
        });
        self.set_terminator(Terminator::Branch {
            cond: cond_reg,
            then_block: body_block,
            else_block: exit_block,
        });

        // Body block: array[loop_idx] = arg[fixed_count + loop_idx]; loop_idx++
        self.current_function_mut()
            .add_block(BasicBlock::with_label(body_block, "rest.body"));
        self.current_block = body_block;

        // Calculate argument index: arg_idx = fixed_count + loop_idx
        let arg_idx_reg = self.alloc_register(TypeId::new(INT_TYPE_ID));
        self.emit(IrInstr::BinaryOp {
            dest: arg_idx_reg.clone(),
            op: BinaryOp::Add,
            left: fixed_count_reg.clone(),
            right: loop_idx_reg.clone(),
        });

        // Load argument from locals by dynamic index
        let arg_val_reg = self.alloc_register(UNRESOLVED);
        self.emit(IrInstr::LoadArgLocal {
            dest: arg_val_reg.clone(),
            index: arg_idx_reg,
        });

        // Store in array
        self.emit(IrInstr::StoreElement {
            array: array_reg.clone(),
            index: loop_idx_reg.clone(),
            value: arg_val_reg,
        });

        // Increment loop index
        let one_reg = self.alloc_register(TypeId::new(INT_TYPE_ID));
        self.emit(IrInstr::Assign {
            dest: one_reg.clone(),
            value: IrValue::Constant(IrConstant::I32(1)),
        });
        self.emit(IrInstr::BinaryOp {
            dest: loop_idx_reg.clone(),
            op: BinaryOp::Add,
            left: loop_idx_reg,
            right: one_reg,
        });

        // Jump back to condition
        self.set_terminator(Terminator::Jump(init_block));

        // Exit block: store array in rest parameter local
        self.current_function_mut()
            .add_block(BasicBlock::with_label(exit_block, "rest.exit"));
        self.current_block = exit_block;

        let rest_local_idx = self
            .lookup_local(rest_name)
            .unwrap_or_else(|| self.allocate_local(rest_name));
        self.emit(IrInstr::StoreLocal {
            index: rest_local_idx,
            value: array_reg.clone(),
        });
        self.local_registers.insert(rest_local_idx, array_reg);
    }

    pub(super) fn js_parameter_environment_required(&self, params: &[ast::Parameter]) -> bool {
        self.js_this_binding_compat
            && params.iter().any(|param| {
                param.default_value.is_some()
                    || param.optional
                    || !matches!(param.pattern, Pattern::Identifier(_) | Pattern::Rest(_))
            })
    }

    pub(super) fn preallocate_parameter_pattern_locals(&mut self, pattern: &ast::Pattern) {
        match pattern {
            Pattern::Identifier(ident) => {
                if !self.local_map.contains_key(&ident.name) {
                    self.allocate_local(ident.name);
                }
            }
            Pattern::Array(array) => {
                for element in array.elements.iter().flatten() {
                    self.preallocate_parameter_pattern_locals(&element.pattern);
                }
                if let Some(rest) = &array.rest {
                    self.preallocate_parameter_pattern_locals(rest);
                }
            }
            Pattern::Object(object) => {
                for property in &object.properties {
                    self.preallocate_parameter_pattern_locals(&property.value);
                }
                if let Some(rest) = &object.rest {
                    if !self.local_map.contains_key(&rest.name) {
                        self.allocate_local(rest.name);
                    }
                }
            }
            Pattern::Rest(rest) => {
                self.preallocate_parameter_pattern_locals(&rest.argument);
            }
        }
    }

    pub(super) fn clear_parameter_tdz_for_pattern(&mut self, pattern: &ast::Pattern) {
        let mut names = FxHashSet::default();
        collect_pattern_names(pattern, &mut names);
        for name in names {
            self.parameter_tdz_symbols.remove(&name);
        }
    }

    fn emit_parameter_default_selection(
        &mut self,
        param: &ast::Parameter,
        raw_slot: u16,
        fallback_ty: TypeId,
    ) -> Register {
        let raw_value = self.alloc_register(fallback_ty);
        self.emit(IrInstr::LoadLocal {
            dest: raw_value.clone(),
            index: raw_slot,
        });

        let Some(default_expr) = &param.default_value else {
            return raw_value;
        };

        let temp_local = self.allocate_anonymous_local();
        let missing_block = self.alloc_block();
        let present_block = self.alloc_block();
        let merge_block = self.alloc_block();

        let undefined_reg = self.alloc_register(UNRESOLVED);
        self.emit(IrInstr::Assign {
            dest: undefined_reg.clone(),
            value: IrValue::Constant(IrConstant::Undefined),
        });
        let is_undefined = self.alloc_register(TypeId::new(BOOLEAN_TYPE_ID));
        self.emit(IrInstr::BinaryOp {
            dest: is_undefined.clone(),
            op: BinaryOp::StrictEqual,
            left: raw_value.clone(),
            right: undefined_reg,
        });
        self.set_terminator(Terminator::Branch {
            cond: is_undefined,
            then_block: missing_block,
            else_block: present_block,
        });

        self.current_function_mut()
            .add_block(BasicBlock::with_label(present_block, "param.present"));
        self.current_block = present_block;
        self.emit(IrInstr::StoreLocal {
            index: temp_local,
            value: raw_value,
        });
        self.set_terminator(Terminator::Jump(merge_block));

        self.current_function_mut()
            .add_block(BasicBlock::with_label(missing_block, "param.default"));
        self.current_block = missing_block;
        let replacement = self.lower_expr(default_expr);
        self.maybe_assign_anonymous_binding_name(&param.pattern, default_expr, &replacement);
        self.emit(IrInstr::StoreLocal {
            index: temp_local,
            value: replacement,
        });
        self.set_terminator(Terminator::Jump(merge_block));

        self.current_function_mut()
            .add_block(BasicBlock::with_label(merge_block, "param.merge"));
        self.current_block = merge_block;

        let final_value = self.alloc_register(UNRESOLVED);
        self.emit(IrInstr::LoadLocal {
            dest: final_value.clone(),
            index: temp_local,
        });
        final_value
    }

    pub(super) fn emit_js_parameter_initialization(
        &mut self,
        params: &[ast::Parameter],
        raw_param_slots: &[u16],
    ) {
        let saved_parameter_scope_eval_mode = self.parameter_scope_eval_mode;
        let saved_parameter_binding_mode = self.parameter_binding_mode;
        let saved_parameter_tdz_symbols = std::mem::take(&mut self.parameter_tdz_symbols);

        self.parameter_scope_eval_mode = true;
        self.parameter_binding_mode = true;

        for param in params {
            let mut names = FxHashSet::default();
            collect_pattern_names(&param.pattern, &mut names);
            self.parameter_tdz_symbols.extend(names);
        }

        let mut raw_slot_iter = raw_param_slots.iter().copied();
        for param in params {
            if param.is_rest {
                continue;
            }

            let raw_slot = raw_slot_iter
                .next()
                .expect("raw parameter slots must cover all non-rest parameters");
            let fallback_ty = param
                .type_annotation
                .as_ref()
                .map(|type_ann| self.resolve_type_annotation(type_ann))
                .unwrap_or(UNRESOLVED);
            let value = self.emit_parameter_default_selection(param, raw_slot, fallback_ty);

            if let ast::Pattern::Object(_) = &param.pattern {
                if let Some(type_ann) = &param.type_annotation {
                    if let Some(field_layout) = self.extract_field_names_from_type(type_ann) {
                        self.register_object_fields.insert(value.id, field_layout);
                    }
                    if let Some(nested_array_layouts) =
                        self.extract_array_element_object_layouts_from_type(type_ann)
                    {
                        for (field_idx, layout) in nested_array_layouts {
                            self.register_nested_array_element_object_fields
                                .insert((value.id, field_idx), layout);
                        }
                    }
                }
            }

            self.bind_pattern(&param.pattern, value);
            self.clear_parameter_tdz_for_pattern(&param.pattern);
        }

        self.parameter_scope_eval_mode = saved_parameter_scope_eval_mode;
        self.parameter_binding_mode = saved_parameter_binding_mode;
        self.parameter_tdz_symbols = saved_parameter_tdz_symbols;
    }

    /// Emit missing-argument normalization and default-value assignment for parameters.
    /// Must be called after entry block creation and parameter registration,
    /// before lowering the function body.
    fn emit_default_params(&mut self, params: &[ast::Parameter]) {
        let saved_parameter_scope_eval_mode = self.parameter_scope_eval_mode;
        self.parameter_scope_eval_mode = true;
        for param in params {
            let Pattern::Identifier(ident) = &param.pattern else {
                continue;
            };
            let Some(&local_idx) = self.local_map.get(&ident.name) else {
                continue;
            };
            if param.default_value.is_none() && !(param.optional && !self.js_this_binding_compat) {
                continue;
            }

            let param_reg = self.alloc_register(UNRESOLVED);
            self.emit(IrInstr::LoadLocal {
                dest: param_reg.clone(),
                index: local_idx,
            });

            let missing_block = self.alloc_block();
            let continue_block = self.alloc_block();
            let null_check_block = self.alloc_block();

            let undefined_reg = self.alloc_register(UNRESOLVED);
            self.emit(IrInstr::Assign {
                dest: undefined_reg.clone(),
                value: IrValue::Constant(IrConstant::Undefined),
            });
            let is_undefined = self.alloc_register(TypeId::new(BOOLEAN_TYPE_ID));
            self.emit(IrInstr::BinaryOp {
                dest: is_undefined.clone(),
                op: BinaryOp::StrictEqual,
                left: param_reg.clone(),
                right: undefined_reg,
            });
            self.set_terminator(Terminator::Branch {
                cond: is_undefined,
                then_block: missing_block,
                else_block: null_check_block,
            });

            self.current_function_mut()
                .add_block(BasicBlock::with_label(null_check_block, "param.nullcheck"));
            self.current_block = null_check_block;
            self.set_terminator(Terminator::BranchIfNull {
                value: param_reg,
                null_block: missing_block,
                not_null_block: continue_block,
            });

            self.current_function_mut()
                .add_block(BasicBlock::with_label(missing_block, "param.missing"));
            self.current_block = missing_block;

            let replacement = if let Some(default_expr) = &param.default_value {
                self.lower_expr(default_expr)
            } else {
                let null_reg = self.alloc_register(TypeId::new(NULL_TYPE_ID));
                self.emit(IrInstr::Assign {
                    dest: null_reg.clone(),
                    value: IrValue::Constant(IrConstant::Null),
                });
                null_reg
            };
            self.emit(IrInstr::StoreLocal {
                index: local_idx,
                value: replacement.clone(),
            });
            self.local_registers.insert(local_idx, replacement);
            self.set_terminator(Terminator::Jump(continue_block));

            self.current_function_mut()
                .add_block(BasicBlock::with_label(continue_block, "param.cont"));
            self.current_block = continue_block;
        }
        self.parameter_scope_eval_mode = saved_parameter_scope_eval_mode;
    }

    /// Find a method's vtable slot in parent class hierarchy
    fn find_parent_method_slot(
        &self,
        parent_class: Option<NominalTypeId>,
        method_name: Symbol,
    ) -> Option<u16> {
        let mut current = parent_class;
        while let Some(nominal_type_id) = current {
            if let Some(&slot) = self.method_slot_map.get(&(nominal_type_id, method_name)) {
                return Some(slot);
            }
            current = self
                .class_info_map
                .get(&nominal_type_id)
                .and_then(|info| info.parent_class);
        }
        None
    }

    /// Resolve a method slot for a class, falling back to inherited slots.
    pub(super) fn find_method_slot(
        &self,
        nominal_type_id: NominalTypeId,
        method_name: Symbol,
    ) -> Option<u16> {
        self.method_slot_map
            .get(&(nominal_type_id, method_name))
            .copied()
            .or_else(|| {
                let parent = self
                    .class_info_map
                    .get(&nominal_type_id)
                    .and_then(|info| info.parent_class);
                self.find_parent_method_slot(parent, method_name)
            })
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

    fn shared_script_binding_slot(&self, name: Symbol) -> Option<u16> {
        if self.builtin_this_coercion_compat {
            self.module_var_globals.get(&name).copied()
        } else {
            self.js_script_lexical_globals
                .get(&name)
                .copied()
                .or_else(|| self.module_var_globals.get(&name).copied())
        }
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
        let static_fields: Vec<(NominalTypeId, Symbol, u16, Expression)> = self
            .class_info_map
            .values()
            .flat_map(|class_info| {
                class_info.static_fields.iter().filter_map(|sf| {
                    sf.initializer
                        .as_ref()
                        .map(|init| (sf.nominal_type_id, sf.name, sf.global_index, init.clone()))
                })
            })
            .collect();

        // Emit initialization for each static field
        for (nominal_type_id, field_name, global_index, initializer) in static_fields {
            let value_reg = self.lower_expr(&initializer);
            self.emit(IrInstr::StoreGlobal {
                index: global_index,
                value: value_reg.clone(),
            });

            let class_value = self.alloc_register(TypeId::new(UNKNOWN_TYPE_ID));
            let nominal_id_reg = self.alloc_register(TypeId::new(NUMBER_TYPE_ID));
            self.emit(IrInstr::Assign {
                dest: nominal_id_reg.clone(),
                value: IrValue::Constant(IrConstant::I32(nominal_type_id.as_u32() as i32)),
            });
            self.emit(IrInstr::NativeCall {
                dest: Some(class_value.clone()),
                native_id: crate::compiler::native_id::OBJECT_GET_CLASS_VALUE,
                args: vec![nominal_id_reg],
            });

            let key_reg = self.alloc_register(TypeId::new(STRING_TYPE_ID));
            self.emit(IrInstr::Assign {
                dest: key_reg.clone(),
                value: IrValue::Constant(IrConstant::String(
                    self.interner.resolve(field_name).to_string(),
                )),
            });
            self.emit(IrInstr::NativeCall {
                dest: None,
                native_id: crate::compiler::native_id::REFLECT_SET,
                args: vec![class_value, key_reg, value_reg],
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
        // Structure: (nominal_type_id, class_name, class_decorators, field_decorators, method_decorators, parameter_decorators)
        #[allow(clippy::type_complexity)]
        // Tuple groups related decorator info; a dedicated struct would add boilerplate for a single use-site.
        let decorator_apps: Vec<(
            NominalTypeId,
            String,
            Vec<DecoratorInfo>,
            Vec<FieldDecoratorInfo>,
            Vec<MethodDecoratorInfo>,
            Vec<ParameterDecoratorInfo>,
        )> = self
            .class_info_map
            .iter()
            .filter_map(|(&nominal_type_id, info)| {
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
                    .find(|(_, &id)| id == nominal_type_id)
                    .map(|(sym, _)| self.interner.resolve(*sym).to_string())
                    .unwrap_or_else(|| format!("class_{}", nominal_type_id.as_u32()));

                Some((
                    nominal_type_id,
                    class_name,
                    info.class_decorators.clone(),
                    info.field_decorators.clone(),
                    info.method_decorators.clone(),
                    info.parameter_decorators.clone(),
                ))
            })
            .collect();

        // Process each class's decorators
        for (
            nominal_type_id,
            class_name,
            class_decorators,
            field_decorators,
            method_decorators,
            parameter_decorators,
        ) in decorator_apps
        {
            let nominal_type_id_val = nominal_type_id.as_u32();

            // 1. Process parameter decorators first (applied before method is decorated)
            for param_dec in &parameter_decorators {
                for dec_info in &param_dec.decorators {
                    self.emit_decorator_call(
                        DecoratorTarget::Parameter {
                            nominal_type_id: nominal_type_id_val,
                            class_name: class_name.clone(),
                            method_name: param_dec.method_name.clone(),
                            param_index: param_dec.param_index,
                        },
                        dec_info,
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
                            nominal_type_id: nominal_type_id_val,
                            class_name: class_name.clone(),
                            field_name: field_name.clone(),
                        },
                        dec_info,
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
                            nominal_type_id: nominal_type_id_val,
                            class_name: class_name.clone(),
                            method_name: method_name.clone(),
                        },
                        dec_info,
                        REGISTER_METHOD_DECORATOR,
                    );
                }
            }

            // 4. Process class decorators (bottom-to-top = reverse order in list)
            for dec_info in class_decorators.iter().rev() {
                self.emit_decorator_call(
                    DecoratorTarget::Class {
                        nominal_type_id: nominal_type_id_val,
                        class_name: class_name.clone(),
                    },
                    dec_info,
                    REGISTER_CLASS_DECORATOR,
                );
            }
        }
    }

    /// Emit code to call a single decorator
    fn emit_decorator_call(
        &mut self,
        target: DecoratorTarget,
        dec_info: &DecoratorInfo,
        registration_native_id: u16,
    ) {
        let decorator_expr = &dec_info.expression;
        // Get decorator name for registration
        let decorator_name = self.get_decorator_name(decorator_expr);

        // Create nominal_type_id register
        let nominal_type_id_val = match &target {
            DecoratorTarget::Class {
                nominal_type_id, ..
            } => *nominal_type_id,
            DecoratorTarget::Method {
                nominal_type_id, ..
            } => *nominal_type_id,
            DecoratorTarget::Field {
                nominal_type_id, ..
            } => *nominal_type_id,
            DecoratorTarget::Parameter {
                nominal_type_id, ..
            } => *nominal_type_id,
        };
        let nominal_type_id_reg = self.alloc_register(TypeId::new(0));
        self.emit(IrInstr::Assign {
            dest: nominal_type_id_reg.clone(),
            value: IrValue::Constant(IrConstant::I32(nominal_type_id_val as i32)),
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

        // Build JS-model decorator arguments from runtime targets.
        // Registration still uses internal class IDs in separate native calls.
        let args = match &target {
            DecoratorTarget::Class {
                nominal_type_id, ..
            } => {
                vec![self.build_class_decorator_target(*nominal_type_id)]
            }
            DecoratorTarget::Method {
                nominal_type_id,
                method_name,
                ..
            } => {
                let target_reg = self.build_method_decorator_target(*nominal_type_id, method_name);
                let method_name_reg = self.alloc_register(TypeId::new(1));
                self.emit(IrInstr::Assign {
                    dest: method_name_reg.clone(),
                    value: IrValue::Constant(IrConstant::String(method_name.clone())),
                });
                vec![target_reg, method_name_reg]
            }
            DecoratorTarget::Field {
                nominal_type_id,
                field_name,
                ..
            } => {
                let target_reg = self.build_class_decorator_target(*nominal_type_id);
                let field_name_reg = self.alloc_register(TypeId::new(1));
                self.emit(IrInstr::Assign {
                    dest: field_name_reg.clone(),
                    value: IrValue::Constant(IrConstant::String(field_name.clone())),
                });
                vec![target_reg, field_name_reg]
            }
            DecoratorTarget::Parameter {
                nominal_type_id,
                method_name,
                param_index,
                ..
            } => {
                let target_reg = self.build_class_decorator_target(*nominal_type_id);
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
                vec![target_reg, method_name_reg, param_index_reg]
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
            let decorator_ty = if dec_info.expr_type != UNRESOLVED {
                dec_info.expr_type
            } else {
                self.get_expr_type(decorator_expr)
            };
            let decorator_ty_raw = decorator_ty.as_u32();
            if !self.type_is_callable(decorator_ty) {
                self.errors
                    .push(crate::compiler::CompileError::InternalError {
                        message: format!(
                            "decorator factory result is not callable (type id {})",
                            decorator_ty_raw
                        ),
                    });
                return;
            }
            let decorator_closure = self.lower_expr(decorator_expr);
            let result_reg = self.alloc_register(TypeId::new(0));
            self.emit(IrInstr::CallClosure {
                dest: Some(result_reg),
                closure: decorator_closure,
                args,
            });
        } else {
            // Case 3: Local variable or other expression - lower and use CallClosure
            let decorator_ty = if dec_info.expr_type != UNRESOLVED {
                dec_info.expr_type
            } else {
                self.get_expr_type(decorator_expr)
            };
            let decorator_ty_raw = decorator_ty.as_u32();
            if !self.type_is_callable(decorator_ty) {
                self.errors
                    .push(crate::compiler::CompileError::InternalError {
                        message: format!(
                            "decorator expression is not callable (type id {})",
                            decorator_ty_raw
                        ),
                    });
                return;
            }
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
                // registerClassDecorator(typeRef, decoratorName)
                self.emit(IrInstr::NativeCall {
                    dest: None,
                    native_id: registration_native_id,
                    args: vec![nominal_type_id_reg, dec_name_reg],
                });
            }
            DecoratorTarget::Method { method_name, .. } => {
                // registerMethodDecorator(typeRef, methodName, decoratorName)
                let method_name_reg = self.alloc_register(TypeId::new(1));
                self.emit(IrInstr::Assign {
                    dest: method_name_reg.clone(),
                    value: IrValue::Constant(IrConstant::String(method_name.clone())),
                });
                self.emit(IrInstr::NativeCall {
                    dest: None,
                    native_id: registration_native_id,
                    args: vec![nominal_type_id_reg, method_name_reg, dec_name_reg],
                });
            }
            DecoratorTarget::Field { field_name, .. } => {
                // registerFieldDecorator(typeRef, fieldName, decoratorName)
                let field_name_reg = self.alloc_register(TypeId::new(1));
                self.emit(IrInstr::Assign {
                    dest: field_name_reg.clone(),
                    value: IrValue::Constant(IrConstant::String(field_name.clone())),
                });
                self.emit(IrInstr::NativeCall {
                    dest: None,
                    native_id: registration_native_id,
                    args: vec![nominal_type_id_reg, field_name_reg, dec_name_reg],
                });
            }
            DecoratorTarget::Parameter {
                method_name,
                param_index,
                ..
            } => {
                // registerParameterDecorator(typeRef, methodName, paramIndex, decoratorName)
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
                    args: vec![
                        nominal_type_id_reg,
                        method_name_reg,
                        param_index_reg,
                        dec_name_reg,
                    ],
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

    /// Build runtime target for class/field/parameter decorators.
    ///
    /// Prefer constructor function closure when available; otherwise materialize
    /// a class instance as a pragmatic runtime target object.
    fn build_class_decorator_target(&mut self, nominal_type_id: u32) -> Register {
        let target = self.alloc_register(TypeId::new(0));
        let ctor_func = self
            .class_info_map
            .get(&NominalTypeId::new(nominal_type_id))
            .and_then(|info| info.constructor);
        if let Some(func) = ctor_func {
            self.emit(IrInstr::MakeClosure {
                dest: target.clone(),
                func,
                captures: vec![],
            });
        } else {
            self.emit(IrInstr::NewType {
                dest: target.clone(),
                nominal_type_id: NominalTypeId::new(nominal_type_id),
            });
        }
        target
    }

    /// Build runtime target for method decorators.
    ///
    /// Prefer the actual method function closure; if unresolved, fall back to
    /// class target semantics.
    fn build_method_decorator_target(
        &mut self,
        nominal_type_id: u32,
        method_name: &str,
    ) -> Register {
        let nominal_type_id = NominalTypeId::new(nominal_type_id);
        let func_id = self.interner.lookup(method_name).and_then(|sym| {
            self.method_map
                .get(&(nominal_type_id, sym))
                .copied()
                .or_else(|| self.static_method_map.get(&(nominal_type_id, sym)).copied())
        });

        if let Some(func) = func_id {
            let target = self.alloc_register(TypeId::new(0));
            self.emit(IrInstr::MakeClosure {
                dest: target.clone(),
                func,
                captures: vec![],
            });
            target
        } else {
            self.build_class_decorator_target(nominal_type_id.as_u32())
        }
    }

    /// Resolve a type annotation to a TypeId
    fn resolve_type_annotation(&self, ty: &ast::TypeAnnotation) -> TypeId {
        use crate::parser::types::ty::{
            ArrayType as TyArray, FunctionType as TyFunction, ObjectType as TyObject,
            PrimitiveType as TyPrim, PropertySignature as TyProperty, TupleType as TyTuple,
            Type as TyType,
        };

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
                self.type_ctx
                    .lookup(&TyType::Primitive(ty_prim))
                    .unwrap_or(UNRESOLVED)
            }
            ast::Type::Reference(type_ref) => {
                let name = self.interner.resolve(type_ref.name.name);
                // Check active type parameter substitutions first (during generic specialization)
                if let Some(&concrete_ty) = self.type_param_substitutions.get(name) {
                    return concrete_ty;
                }
                if name == "Task" {
                    return self
                        .type_ctx
                        .lookup_named_type(crate::parser::TypeContext::PROMISE_TYPE_NAME)
                        .unwrap_or(UNRESOLVED);
                }
                self.type_ctx.lookup_named_type(name).unwrap_or(UNRESOLVED)
            }
            ast::Type::Array(array) => {
                let elem_ty = self.resolve_type_annotation(&array.element_type);
                self.type_ctx
                    .lookup(&TyType::Array(TyArray { element: elem_ty }))
                    .or_else(|| self.type_ctx.lookup_named_type("Array"))
                    .unwrap_or(UNRESOLVED)
            }
            ast::Type::Tuple(tuple) => {
                let elements = tuple
                    .element_types
                    .iter()
                    .map(|elem| self.resolve_type_annotation(elem))
                    .collect::<Vec<_>>();
                self.type_ctx
                    .lookup(&TyType::Tuple(TyTuple { elements }))
                    .or_else(|| self.type_ctx.lookup_named_type("Array"))
                    .unwrap_or(UNRESOLVED)
            }
            ast::Type::Function(func) => {
                let mut params = Vec::with_capacity(func.params.len());
                let mut rest_param = None;
                let mut min_params = 0usize;
                for param in &func.params {
                    let param_ty = self.resolve_type_annotation(&param.ty);
                    if param.is_rest {
                        rest_param = Some(param_ty);
                    } else {
                        if !param.optional {
                            min_params += 1;
                        }
                        params.push(param_ty);
                    }
                }
                let return_ty = self.resolve_type_annotation(&func.return_type);
                self.type_ctx
                    .lookup(&TyType::Function(TyFunction {
                        params,
                        return_type: return_ty,
                        is_async: false,
                        min_params,
                        rest_param,
                    }))
                    .unwrap_or(UNRESOLVED)
            }
            ast::Type::Object(obj) => {
                let mut properties = Vec::with_capacity(obj.members.len());
                let mut index_signature = None;
                let mut call_signatures = Vec::new();
                let mut construct_signatures = Vec::new();
                for member in &obj.members {
                    match member {
                        ast::ObjectTypeMember::Property(prop) => {
                            properties.push(TyProperty {
                                name: self.interner.resolve(prop.name.name).to_string(),
                                ty: self.resolve_type_annotation(&prop.ty),
                                optional: prop.optional,
                                readonly: prop.readonly,
                                visibility: ast::Visibility::Public,
                            });
                        }
                        ast::ObjectTypeMember::Method(method) => {
                            let mut params = Vec::with_capacity(method.params.len());
                            let mut rest_param = None;
                            let mut min_params = 0usize;
                            for param in &method.params {
                                let param_ty = self.resolve_type_annotation(&param.ty);
                                if param.is_rest {
                                    rest_param = Some(param_ty);
                                } else {
                                    if !param.optional {
                                        min_params += 1;
                                    }
                                    params.push(param_ty);
                                }
                            }
                            let return_ty = self.resolve_type_annotation(&method.return_type);
                            let method_ty = self
                                .type_ctx
                                .lookup(&TyType::Function(TyFunction {
                                    params,
                                    return_type: return_ty,
                                    is_async: false,
                                    min_params,
                                    rest_param,
                                }))
                                .unwrap_or(UNRESOLVED);
                            properties.push(TyProperty {
                                name: self.interner.resolve(method.name.name).to_string(),
                                ty: method_ty,
                                optional: method.optional,
                                readonly: false,
                                visibility: ast::Visibility::Public,
                            });
                        }
                        ast::ObjectTypeMember::IndexSignature(index) => {
                            let key_name = self.interner.resolve(index.key_name.name).to_string();
                            let value_ty = self.resolve_type_annotation(&index.value_type);
                            index_signature = Some((key_name, value_ty));
                        }
                        ast::ObjectTypeMember::CallSignature(call_sig) => {
                            let mut params = Vec::with_capacity(call_sig.params.len());
                            let mut rest_param = None;
                            let mut min_params = 0usize;
                            for param in &call_sig.params {
                                let param_ty = self.resolve_type_annotation(&param.ty);
                                if param.is_rest {
                                    rest_param = Some(param_ty);
                                } else {
                                    if !param.optional {
                                        min_params += 1;
                                    }
                                    params.push(param_ty);
                                }
                            }
                            let return_ty = self.resolve_type_annotation(&call_sig.return_type);
                            let call_ty = self
                                .type_ctx
                                .lookup(&TyType::Function(TyFunction {
                                    params,
                                    return_type: return_ty,
                                    is_async: false,
                                    min_params,
                                    rest_param,
                                }))
                                .unwrap_or(UNRESOLVED);
                            call_signatures.push(call_ty);
                        }
                        ast::ObjectTypeMember::ConstructSignature(ctor_sig) => {
                            let mut params = Vec::with_capacity(ctor_sig.params.len());
                            let mut rest_param = None;
                            let mut min_params = 0usize;
                            for param in &ctor_sig.params {
                                let param_ty = self.resolve_type_annotation(&param.ty);
                                if param.is_rest {
                                    rest_param = Some(param_ty);
                                } else {
                                    if !param.optional {
                                        min_params += 1;
                                    }
                                    params.push(param_ty);
                                }
                            }
                            let return_ty = self.resolve_type_annotation(&ctor_sig.return_type);
                            let ctor_ty = self
                                .type_ctx
                                .lookup(&TyType::Function(TyFunction {
                                    params,
                                    return_type: return_ty,
                                    is_async: false,
                                    min_params,
                                    rest_param,
                                }))
                                .unwrap_or(UNRESOLVED);
                            construct_signatures.push(ctor_ty);
                        }
                    }
                }
                self.type_ctx
                    .lookup(&TyType::Object(TyObject {
                        properties,
                        index_signature,
                        call_signatures,
                        construct_signatures,
                    }))
                    .unwrap_or(UNRESOLVED)
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
            ast::Type::StringLiteral(_) => self
                .type_ctx
                .lookup(&TyType::Primitive(TyPrim::String))
                .unwrap_or(UNRESOLVED),
            ast::Type::NumberLiteral(_) => self
                .type_ctx
                .lookup(&TyType::Primitive(TyPrim::Number))
                .unwrap_or(UNRESOLVED),
            ast::Type::BooleanLiteral(_) => self
                .type_ctx
                .lookup(&TyType::Primitive(TyPrim::Boolean))
                .unwrap_or(UNRESOLVED),
            // Parenthesized: unwrap
            ast::Type::Parenthesized(inner) => self.resolve_type_annotation(inner),
            // Function, Object, Intersection, Typeof → UNRESOLVED
            _ => UNRESOLVED,
        }
    }

    /// Extract a NominalTypeId from a type annotation, handling both direct class references
    /// and nullable unions (e.g., `Node | null` → Node's NominalTypeId).
    fn try_extract_class_from_type(&self, type_ann: &ast::TypeAnnotation) -> Option<NominalTypeId> {
        let direct = match &type_ann.ty {
            ast::Type::Reference(type_ref) => {
                self.nominal_type_id_from_type_name(self.interner.resolve(type_ref.name.name))
            }
            ast::Type::Union(union_type) => {
                let mut nominal_type_id = None;
                for member in &union_type.types {
                    match &member.ty {
                        ast::Type::Primitive(ast::PrimitiveType::Null) => {} // skip null
                        ast::Type::Reference(type_ref) => {
                            if nominal_type_id.is_some() {
                                return None; // multiple class refs — ambiguous
                            }
                            nominal_type_id = self.nominal_type_id_from_type_name(
                                self.interner.resolve(type_ref.name.name),
                            );
                        }
                        _ => return None, // non-null, non-class member
                    }
                }
                nominal_type_id
            }
            _ => None,
        };

        if direct.is_some() {
            return direct;
        }

        let resolved = self.resolve_type_annotation(type_ann);
        if resolved != UNRESOLVED {
            if let Some(class_id) = self.nominal_type_id_from_type_id(resolved) {
                return Some(class_id);
            }
            let mut visited = FxHashSet::default();
            if let Some(class_ty) =
                self.resolve_concrete_class_type_for_runtime_slots(resolved, &mut visited)
            {
                if let Some(class_id) = self.nominal_type_id_from_type_id(class_ty) {
                    return Some(class_id);
                }
            }
        }

        None
    }

    /// Extract a class name from a type annotation without requiring the class to
    /// be registered yet. Used for forward references in method return types.
    fn try_extract_class_name_from_type(&self, type_ann: &ast::TypeAnnotation) -> Option<String> {
        match &type_ann.ty {
            ast::Type::Reference(type_ref) => {
                Some(self.interner.resolve(type_ref.name.name).to_string())
            }
            ast::Type::Union(union_type) => {
                let mut class_name: Option<String> = None;
                for member in &union_type.types {
                    match &member.ty {
                        ast::Type::Primitive(ast::PrimitiveType::Null) => {}
                        ast::Type::Reference(type_ref) => {
                            let name = self.interner.resolve(type_ref.name.name).to_string();
                            if class_name.is_some() {
                                return None;
                            }
                            class_name = Some(name);
                        }
                        _ => return None,
                    }
                }
                class_name
            }
            _ => None,
        }
    }

    /// Resolve a class ID from a type name.
    /// Supports direct class names and synthesized wrapper aliases:
    /// `__t_<module>_<ClassName>`.
    pub(super) fn nominal_type_id_from_type_name(&self, type_name: &str) -> Option<NominalTypeId> {
        let pick_scoped = |entries: &Vec<(usize, NominalTypeId)>| -> Option<NominalTypeId> {
            if entries.is_empty() {
                return None;
            }
            let pos = self.current_span.start;
            entries
                .iter()
                .filter(|(span_start, _)| *span_start <= pos)
                .max_by_key(|(span_start, _)| *span_start)
                .map(|(_, cid)| *cid)
                .or_else(|| {
                    entries
                        .iter()
                        .min_by_key(|(span_start, _)| *span_start)
                        .map(|(_, cid)| *cid)
                })
        };

        // 1) Exact class name lookup.
        if let Some(sym) = self.interner.lookup(type_name) {
            if let Some(entries) = self.class_decl_history.get(&sym) {
                if let Some(cid) = pick_scoped(entries) {
                    return Some(cid);
                }
            }
            if let Some(&cid) = self.class_map.get(&sym) {
                return Some(cid);
            }
        }
        for (&sym, entries) in &self.class_decl_history {
            if self.interner.resolve(sym) == type_name {
                if let Some(cid) = pick_scoped(entries) {
                    return Some(cid);
                }
            }
        }
        for (&sym, &cid) in &self.class_map {
            if self.interner.resolve(sym) == type_name {
                return Some(cid);
            }
        }

        // 2) Exact synthesized alias lookup.
        if let Some(&cid) = self.type_alias_class_map.get(type_name) {
            return Some(cid);
        }

        None
    }

    fn resolve_runtime_bound_new_nominal_type(
        &self,
        constructor_symbol: Symbol,
        constructor_type: Option<TypeId>,
        instance_type: Option<TypeId>,
    ) -> Option<NominalTypeId> {
        instance_type
            .and_then(|ty| {
                self.nominal_type_id_from_type_id(ty).or_else(|| {
                    let mut visited = FxHashSet::default();
                    self.resolve_concrete_class_type_for_runtime_slots(ty, &mut visited)
                        .and_then(|class_ty| self.nominal_type_id_from_type_id(class_ty))
                })
            })
            .or_else(|| self.class_map.get(&constructor_symbol).copied())
            .or_else(|| self.variable_class_map.get(&constructor_symbol).copied())
            .or_else(|| {
                self.nominal_type_id_from_type_name(self.interner.resolve(constructor_symbol))
            })
            .or_else(|| {
                constructor_type.and_then(|ty| {
                    self.nominal_type_id_from_type_id(ty).or_else(|| {
                        let mut visited = FxHashSet::default();
                        self.resolve_concrete_class_type_for_runtime_slots(ty, &mut visited)
                            .and_then(|class_ty| self.nominal_type_id_from_type_id(class_ty))
                    })
                })
            })
            .or_else(|| {
                self.type_ctx
                    .lookup_named_type(self.interner.resolve(constructor_symbol))
                    .and_then(|ty| {
                        self.nominal_type_id_from_type_id(ty).or_else(|| {
                            let mut visited = FxHashSet::default();
                            self.resolve_concrete_class_type_for_runtime_slots(ty, &mut visited)
                                .and_then(|class_ty| self.nominal_type_id_from_type_id(class_ty))
                        })
                    })
            })
    }

    fn populate_alias_object_class_map(
        &mut self,
        alias_name: &str,
        nominal_type_id: NominalTypeId,
    ) {
        // Prefer type IDs resolved from explicit type-alias declarations; fall back
        // to named type lookup when available.
        let alias_ty = self
            .type_alias_resolved_type_map
            .get(alias_name)
            .copied()
            .or_else(|| self.type_ctx.lookup_named_type(alias_name))
            .unwrap_or(UNRESOLVED);
        if alias_ty == UNRESOLVED {
            return;
        }

        self.type_alias_object_class_map
            .insert(alias_ty, nominal_type_id);

        // Some checker paths materialize structurally equivalent object/interface
        // TypeIds distinct from the named alias type. Pre-populate those equivalent
        // TypeIds so class dispatch works for alias-backed wrapper values.
        let mut subtype_ctx = crate::parser::types::subtyping::SubtypingContext::new(self.type_ctx);
        for raw_ty in 0..self.type_ctx.len() {
            let candidate_ty = TypeId::new(raw_ty as u32);
            if subtype_ctx.is_subtype(candidate_ty, alias_ty)
                && subtype_ctx.is_subtype(alias_ty, candidate_ty)
            {
                self.type_alias_object_class_map
                    .entry(candidate_ty)
                    .or_insert(nominal_type_id);
            }
        }
    }

    pub(super) fn type_alias_field_lookup(
        &self,
        alias_name: &str,
        field_name: &str,
    ) -> Option<(u16, TypeId)> {
        self.type_alias_object_fields
            .get(alias_name)
            .and_then(|fields| {
                fields
                    .iter()
                    .find(|(name, _, _)| name == field_name)
                    .map(|(_, idx, ty)| (*idx, *ty))
            })
    }

    /// Extract field names from an object type annotation for destructuring
    /// Returns a Vec of (field_name, field_index) tuples
    fn extract_field_names_from_type(
        &self,
        type_ann: &ast::TypeAnnotation,
    ) -> Option<Vec<(String, usize)>> {
        let mut names = Vec::new();
        match &type_ann.ty {
            ast::Type::Object(obj_type) => {
                for member in &obj_type.members {
                    match member {
                        ast::ObjectTypeMember::Property(prop) => {
                            names.push(self.interner.resolve(prop.name.name).to_string());
                        }
                        ast::ObjectTypeMember::Method(_) => {
                            // Methods don't contribute to destructuring field layout
                        }
                        ast::ObjectTypeMember::IndexSignature(_) => {
                            // Index signatures don't contribute named field layout.
                        }
                        ast::ObjectTypeMember::CallSignature(_) => {}
                        ast::ObjectTypeMember::ConstructSignature(_) => {}
                    }
                }
                if names.is_empty() {
                    None
                } else {
                    names.sort_unstable();
                    names.dedup();
                    let fields: Vec<(String, usize)> = names
                        .into_iter()
                        .enumerate()
                        .map(|(idx, name)| (name, idx))
                        .collect();
                    Some(fields)
                }
            }
            _ => None,
        }
    }

    fn extract_array_element_object_layouts_from_type(
        &self,
        type_ann: &ast::TypeAnnotation,
    ) -> Option<FxHashMap<u16, Vec<(String, usize)>>> {
        let ast::Type::Object(obj_type) = &type_ann.ty else {
            return None;
        };

        let mut outer_names: Vec<String> = obj_type
            .members
            .iter()
            .filter_map(|member| match member {
                ast::ObjectTypeMember::Property(prop) => {
                    Some(self.interner.resolve(prop.name.name).to_string())
                }
                ast::ObjectTypeMember::Method(_) => None,
                ast::ObjectTypeMember::IndexSignature(_) => None,
                ast::ObjectTypeMember::CallSignature(_) => None,
                ast::ObjectTypeMember::ConstructSignature(_) => None,
            })
            .collect();
        outer_names.sort_unstable();
        outer_names.dedup();
        let outer_index: FxHashMap<String, u16> = outer_names
            .into_iter()
            .enumerate()
            .map(|(idx, name)| (name, idx as u16))
            .collect();

        let mut layouts: FxHashMap<u16, Vec<(String, usize)>> = FxHashMap::default();
        for member in &obj_type.members {
            let ast::ObjectTypeMember::Property(prop) = member else {
                continue;
            };
            let outer_name = self.interner.resolve(prop.name.name).to_string();
            let Some(&member_idx) = outer_index.get(&outer_name) else {
                continue;
            };
            let ast::Type::Array(arr_ty) = &prop.ty.ty else {
                continue;
            };
            let ast::Type::Object(elem_obj) = &arr_ty.element_type.ty else {
                continue;
            };

            let mut elem_names: Vec<String> = elem_obj
                .members
                .iter()
                .filter_map(|elem_member| match elem_member {
                    ast::ObjectTypeMember::Property(elem_prop) => {
                        Some(self.interner.resolve(elem_prop.name.name).to_string())
                    }
                    ast::ObjectTypeMember::Method(_) => None,
                    ast::ObjectTypeMember::IndexSignature(_) => None,
                    ast::ObjectTypeMember::CallSignature(_) => None,
                    ast::ObjectTypeMember::ConstructSignature(_) => None,
                })
                .collect();
            elem_names.sort_unstable();
            elem_names.dedup();
            let elem_layout: Vec<(String, usize)> = elem_names
                .into_iter()
                .enumerate()
                .map(|(idx, name)| (name, idx))
                .collect();
            if !elem_layout.is_empty() {
                layouts.insert(member_idx, elem_layout);
            }
        }

        if layouts.is_empty() {
            None
        } else {
            Some(layouts)
        }
    }

    /// Propagate object-layout hints from a variable type annotation.
    /// This keeps strict member lowering deterministic for typed parameters,
    /// including nullable aliases like `T | null`.
    fn register_variable_type_hints_from_annotation(
        &mut self,
        var_name: Symbol,
        type_ann: &ast::TypeAnnotation,
    ) {
        if let Some(field_layout) = self.extract_field_names_from_type(type_ann) {
            self.variable_object_fields.insert(var_name, field_layout);
        }

        if let Some(alias_name) = self.try_extract_object_alias_name_from_type(type_ann) {
            self.variable_object_type_aliases
                .insert(var_name, alias_name);
        }
    }

    fn emit_structural_registration_for_ordered_names(&mut self, names: Vec<String>) {
        if names.is_empty() {
            return;
        }

        let mut seen = FxHashSet::default();
        let mut ordered = Vec::with_capacity(names.len());
        for name in names {
            if seen.insert(name.clone()) {
                ordered.push(name);
            }
        }
        if ordered.is_empty() {
            return;
        }

        let mut canonical = ordered;
        canonical.sort_unstable();
        let shape_id = crate::vm::object::shape_id_from_member_names(&canonical);
        self.module_structural_shapes
            .entry(shape_id)
            .or_insert(canonical);
    }

    /// Register canonical member names for a structural shape without attaching
    /// any object-specific runtime view.
    fn emit_structural_shape_name_registration_for_ordered_names(&mut self, names: Vec<String>) {
        self.emit_structural_registration_for_ordered_names(names);
    }

    fn emit_shape_name_registration_for_projection_layout(&mut self, layout: Vec<(String, usize)>) {
        let mut names = layout.into_iter().map(|(name, _)| name).collect::<Vec<_>>();
        names.sort_unstable();
        names.dedup();
        self.emit_structural_shape_name_registration_for_ordered_names(names);
    }

    fn emit_projected_shape_registration_for_register_type(
        &mut self,
        reg: &Register,
        expected_ty: TypeId,
    ) -> bool {
        let Some(layout) = self.structural_projection_layout_from_type_id(expected_ty) else {
            return false;
        };
        self.register_structural_projection_fields
            .entry(reg.id)
            .or_insert_with(|| layout.clone());
        self.emit_shape_name_registration_for_projection_layout(layout);
        true
    }

    fn emit_structural_slot_registration_for_type(
        &mut self,
        object: Register,
        expected_ty: TypeId,
    ) {
        if expected_ty == UNRESOLVED {
            return;
        }
        if self
            .ordered_slot_names_for_concrete_classish_type(expected_ty)
            .is_some()
        {
            // Concrete class member loads/stores use fixed class field indices in IR.
            // Treat them as nominal/exact, not structural projections.
            if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
                eprintln!(
                    "[lower] skip structural shape seeding for concrete class-ish type reg={} expected_ty={}",
                    object.id,
                    self.type_ctx.format_type(expected_ty),
                );
            }
            return;
        }
        let Some(layout) = self.structural_slot_layout_from_type(expected_ty) else {
            return;
        };
        if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
            eprintln!(
                "[lower] seed structural projection reg={} expected_ty={} layout=[{}]",
                object.id,
                self.type_ctx.format_type(expected_ty),
                layout
                    .iter()
                    .map(|(name, idx)| format!("{name}:{idx}"))
                    .collect::<Vec<_>>()
                    .join(",")
            );
        }
        self.register_structural_projection_fields.insert(
            object.id,
            layout
                .iter()
                .map(|(name, idx)| (name.clone(), *idx as usize))
                .collect(),
        );
        self.emit_structural_shape_name_registration_for_ordered_names(
            layout.into_iter().map(|(name, _)| name).collect(),
        );
    }

    fn emit_instance_field_initializers_for_constructor(
        &mut self,
        nominal_type_id: NominalTypeId,
        this_reg: &Register,
    ) {
        let own_fields = self
            .class_info_map
            .get(&nominal_type_id)
            .map(|info| info.fields.clone())
            .unwrap_or_default();
        for field in own_fields {
            if let Some(init_expr) = field.initializer {
                let value = self.lower_expr(&init_expr);
                self.emit(IrInstr::StoreFieldExact {
                    object: this_reg.clone(),
                    field: field.index,
                    value,
                });
            }
        }
    }

    fn emit_constructor_prologue(
        &mut self,
        nominal_type_id: NominalTypeId,
        this_reg: &Register,
        param_properties: &[(u16, Register)],
    ) {
        self.emit_instance_field_initializers_for_constructor(nominal_type_id, this_reg);
        for (field_idx, param_reg) in param_properties {
            self.emit(IrInstr::StoreFieldExact {
                object: this_reg.clone(),
                field: *field_idx,
                value: param_reg.clone(),
            });
        }
    }

    fn emit_pending_constructor_prologue_if_needed(&mut self) {
        if let Some(pending) = self.pending_constructor_prologue.take() {
            self.emit_constructor_prologue(
                pending.nominal_type_id,
                &pending.this_reg,
                &pending.param_properties,
            );
        }
    }

    /// Resolve a type annotation for structural-slot registration.
    ///
    /// Prefers checker-resolved annotation TypeIds to avoid lowering-time
    /// re-resolution drift for aliases and parenthesized types.
    fn resolve_structural_slot_type_from_annotation(
        &self,
        type_ann: &ast::TypeAnnotation,
    ) -> TypeId {
        // During generic specialization, prefer fresh resolution so active
        // type parameter substitutions are reflected in parameter/member lowering.
        if !self.type_param_substitutions.is_empty() {
            let resolved = self.resolve_type_annotation(type_ann);
            if resolved != UNRESOLVED {
                return resolved;
            }
        }

        let ann_id = type_ann as *const _ as usize;
        self.type_annotation_types
            .get(&ann_id)
            .copied()
            .unwrap_or_else(|| self.resolve_type_annotation(type_ann))
    }

    fn resolve_type_param_constraint_type(
        &self,
        name: &str,
        primary: Option<&[ast::TypeParameter]>,
        secondary: Option<&[ast::TypeParameter]>,
    ) -> Option<TypeId> {
        let mut resolve_from = |params: &[ast::TypeParameter]| -> Option<TypeId> {
            for tp in params {
                if self.interner.resolve(tp.name.name) != name {
                    continue;
                }
                if let Some(constraint) = &tp.constraint {
                    let ty = self.resolve_structural_slot_type_from_annotation(constraint);
                    if ty != UNRESOLVED {
                        return Some(ty);
                    }
                }
                if let Some(default) = &tp.default {
                    let ty = self.resolve_structural_slot_type_from_annotation(default);
                    if ty != UNRESOLVED {
                        return Some(ty);
                    }
                }
            }
            None
        };

        if let Some(params) = primary {
            if let Some(ty) = resolve_from(params) {
                return Some(ty);
            }
        }
        if let Some(params) = secondary {
            if let Some(ty) = resolve_from(params) {
                return Some(ty);
            }
        }
        None
    }

    fn resolve_constraint_backed_type_from_annotation(
        &self,
        type_ann: &ast::TypeAnnotation,
        primary: Option<&[ast::TypeParameter]>,
        secondary: Option<&[ast::TypeParameter]>,
    ) -> Option<TypeId> {
        match &type_ann.ty {
            ast::Type::Reference(type_ref) => {
                let name = self.interner.resolve(type_ref.name.name);
                self.resolve_type_param_constraint_type(name, primary, secondary)
            }
            ast::Type::Parenthesized(inner) => {
                self.resolve_constraint_backed_type_from_annotation(inner, primary, secondary)
            }
            ast::Type::Union(union) => {
                let mut found: Option<TypeId> = None;
                for member in &union.types {
                    match &member.ty {
                        ast::Type::Primitive(ast::PrimitiveType::Null) => {}
                        _ => {
                            let ty = self.resolve_constraint_backed_type_from_annotation(
                                member, primary, secondary,
                            )?;
                            match found {
                                None => found = Some(ty),
                                Some(existing) if existing == ty => {}
                                Some(_) => return None,
                            }
                        }
                    }
                }
                found
            }
            _ => None,
        }
    }

    fn resolve_param_type_from_annotation(
        &self,
        type_ann: &ast::TypeAnnotation,
        primary: Option<&[ast::TypeParameter]>,
        secondary: Option<&[ast::TypeParameter]>,
    ) -> TypeId {
        let resolved = self.resolve_structural_slot_type_from_annotation(type_ann);
        let constraint_backed =
            self.resolve_constraint_backed_type_from_annotation(type_ann, primary, secondary);
        if resolved == UNRESOLVED {
            return constraint_backed.unwrap_or(UNRESOLVED);
        }

        if let Some(constraint_ty) = constraint_backed {
            let resolved_has_projection = self
                .structural_projection_layout_from_type_id(resolved)
                .is_some();
            let constraint_has_projection = self
                .structural_projection_layout_from_type_id(constraint_ty)
                .is_some();
            if !resolved_has_projection && constraint_has_projection {
                return constraint_ty;
            }
        }

        resolved
    }

    fn resolve_param_nominal_type_from_annotation(
        &self,
        type_ann: &ast::TypeAnnotation,
        primary: Option<&[ast::TypeParameter]>,
        secondary: Option<&[ast::TypeParameter]>,
    ) -> Option<NominalTypeId> {
        if let Some(class_id) = self.try_extract_class_from_type(type_ann) {
            return Some(class_id);
        }

        let resolved = self.resolve_structural_slot_type_from_annotation(type_ann);
        if resolved != UNRESOLVED {
            if let Some(class_id) = self.nominal_type_id_from_type_id(resolved) {
                return Some(class_id);
            }
            if let Some(crate::parser::types::Type::TypeVar(type_var)) = self.type_ctx.get(resolved)
            {
                if let Some(class_id) = type_var
                    .constraint
                    .and_then(|constraint| self.nominal_type_id_from_type_id(constraint))
                {
                    return Some(class_id);
                }
                if let Some(class_id) = type_var
                    .default
                    .and_then(|default| self.nominal_type_id_from_type_id(default))
                {
                    return Some(class_id);
                }
            }
        }

        self.resolve_constraint_backed_type_from_annotation(type_ann, primary, secondary)
            .and_then(|ty| self.nominal_type_id_from_type_id(ty))
    }

    fn runtime_bound_constructor_from_type_annotation(
        &self,
        type_ann: &ast::TypeAnnotation,
    ) -> Option<(Symbol, Option<TypeId>)> {
        let type_ref = match &type_ann.ty {
            ast::Type::Reference(type_ref) => Some(type_ref),
            ast::Type::Parenthesized(inner) => {
                return self.runtime_bound_constructor_from_type_annotation(inner);
            }
            ast::Type::Union(union_type) => {
                let mut found: Option<&ast::TypeReference> = None;
                for member in &union_type.types {
                    match &member.ty {
                        ast::Type::Primitive(ast::PrimitiveType::Null) => {}
                        ast::Type::Reference(type_ref) => match found {
                            None => found = Some(type_ref),
                            Some(existing) if existing.name.name == type_ref.name.name => {}
                            Some(_) => return None,
                        },
                        _ => return None,
                    }
                }
                found
            }
            _ => None,
        }?;

        let ctor_symbol = type_ref.name.name;
        let ctor_name = self.interner.resolve(ctor_symbol);
        if !self.import_bindings.contains(&ctor_symbol)
            && !self.ambient_builtin_globals.contains(ctor_name)
        {
            return None;
        }
        if matches!(
            ctor_name,
            TypeContext::MUTEX_TYPE_NAME | TypeContext::CHANNEL_TYPE_NAME
        ) {
            return None;
        }
        if self
            .type_ctx
            .lookup_named_type(ctor_name)
            .is_some_and(|ty| self.type_uses_runtime_handle_dispatch(ty))
        {
            return None;
        }

        let resolved = self.resolve_structural_slot_type_from_annotation(type_ann);
        let resolved = (resolved != UNRESOLVED).then_some(resolved);
        if resolved.is_some_and(|ty| self.type_has_checker_validated_class_members(ty)) {
            return None;
        }
        if resolved.is_some_and(|ty| self.type_uses_runtime_handle_dispatch(ty)) {
            return None;
        }
        Some((ctor_symbol, resolved))
    }

    fn try_extract_object_alias_name_from_type(
        &self,
        type_ann: &ast::TypeAnnotation,
    ) -> Option<String> {
        match &type_ann.ty {
            ast::Type::Reference(type_ref) => {
                let name = self.interner.resolve(type_ref.name.name).to_string();
                if self.type_alias_object_fields.contains_key(&name) {
                    Some(name)
                } else {
                    None
                }
            }
            ast::Type::Union(union_type) => {
                let mut found: Option<String> = None;
                for member in &union_type.types {
                    match &member.ty {
                        ast::Type::Primitive(ast::PrimitiveType::Null) => {}
                        ast::Type::Reference(type_ref) => {
                            let name = self.interner.resolve(type_ref.name.name).to_string();
                            if !self.type_alias_object_fields.contains_key(&name) {
                                continue;
                            }
                            match &found {
                                None => found = Some(name),
                                Some(existing) if existing == &name => {}
                                Some(_) => return None,
                            }
                        }
                        _ => {}
                    }
                }
                found
            }
            _ => None,
        }
    }

    fn type_annotation_is_callable(&self, type_ann: &ast::TypeAnnotation) -> bool {
        match &type_ann.ty {
            ast::Type::Function(_) => true,
            ast::Type::Parenthesized(inner) => self.type_annotation_is_callable(inner),
            ast::Type::Union(union) => union
                .types
                .iter()
                .any(|member| self.type_annotation_is_callable(member)),
            _ => false,
        }
    }

    fn expression_is_callable_hint(&self, expr: &Expression) -> bool {
        match expr {
            Expression::Arrow(_) => true,
            Expression::Function(_) => true,
            Expression::TypeCast(cast) => {
                self.type_annotation_is_callable(&cast.target_type)
                    || self.expression_is_callable_hint(&cast.object)
            }
            Expression::Parenthesized(inner) => self.expression_is_callable_hint(&inner.expression),
            _ => false,
        }
    }

    fn clear_late_bound_object_binding(&mut self, name: Symbol) {
        self.late_bound_object_vars.remove(&name);
        self.late_bound_object_ctor_map.remove(&name);
        self.late_bound_object_type_map.remove(&name);
    }

    fn clear_constructor_value_binding(&mut self, name: Symbol) {
        self.constructor_value_ctor_map.remove(&name);
        self.constructor_value_type_map.remove(&name);
    }

    fn mark_constructor_value_binding(
        &mut self,
        name: Symbol,
        constructor_symbol: Symbol,
        constructor_type: Option<TypeId>,
    ) {
        self.constructor_value_ctor_map
            .insert(name, constructor_symbol);
        if let Some(ty) = constructor_type {
            self.constructor_value_type_map.insert(name, ty);
        } else {
            self.constructor_value_type_map.remove(&name);
        }
    }

    pub(super) fn seed_constructor_projection_binding_from_type(
        &mut self,
        name: Symbol,
        preferred_type: Option<TypeId>,
    ) {
        let constructor_ty = preferred_type
            .or_else(|| self.constructor_value_type_map.get(&name).copied())
            .or_else(|| self.type_ctx.lookup_named_type(self.interner.resolve(name)));

        let Some(constructor_ty) = constructor_ty else {
            return;
        };

        if constructor_ty == UNRESOLVED || !self.type_has_construct_signature(constructor_ty) {
            return;
        }

        self.mark_constructor_value_binding(name, name, Some(constructor_ty));
        if let Some(layout) = self.structural_static_slot_layout_from_type(constructor_ty) {
            if !layout.is_empty() {
                self.variable_structural_projection_fields.insert(
                    name,
                    layout
                        .into_iter()
                        .map(|(field_name, field_idx)| (field_name, field_idx as usize))
                        .collect(),
                );
                self.variable_class_map.remove(&name);
            }
        }
    }

    pub(super) fn identifier_requires_late_bound_dispatch(&self, name: Symbol) -> bool {
        if self.class_map.contains_key(&name) {
            return false;
        }

        if self.late_bound_object_vars.contains(&name)
            || self.constructor_value_ctor_map.contains_key(&name)
        {
            return true;
        }

        if self
            .variable_structural_projection_fields
            .contains_key(&name)
        {
            return false;
        }

        let resolved = self.interner.resolve(name);
        self.ambient_builtin_globals.contains(resolved)
            && !self.class_map.contains_key(&name)
            && !self.variable_class_map.contains_key(&name)
    }

    pub(super) fn type_requires_late_bound_dispatch(&self, ty_id: TypeId) -> bool {
        use crate::parser::types::ty::Type;

        match self.type_ctx.get(ty_id) {
            Some(Type::Class(class_ty)) => {
                self.nominal_type_id_from_type_name(&class_ty.name)
                    .is_none()
                    && !self.type_registry.has_builtin_dispatch_type(&class_ty.name)
            }
            Some(Type::Reference(type_ref)) => self
                .type_ctx
                .lookup_named_type(&type_ref.name)
                .is_some_and(|named| self.type_requires_late_bound_dispatch(named)),
            Some(Type::Union(union)) => union
                .members
                .iter()
                .copied()
                .any(|member| self.type_requires_late_bound_dispatch(member)),
            Some(Type::TypeVar(tv)) => tv
                .constraint
                .is_some_and(|constraint| self.type_requires_late_bound_dispatch(constraint)),
            Some(Type::Generic(generic)) => self.type_requires_late_bound_dispatch(generic.base),
            _ => false,
        }
    }

    fn mark_late_bound_object_binding(
        &mut self,
        name: Symbol,
        constructor_symbol: Symbol,
        constructor_type: Option<TypeId>,
    ) {
        self.late_bound_object_vars.insert(name);
        self.late_bound_object_ctor_map
            .insert(name, constructor_symbol);
        if let Some(ty) = constructor_type {
            self.late_bound_object_type_map.insert(name, ty);
        } else {
            self.late_bound_object_type_map.remove(&name);
        }
        if std::env::var("RAYA_DEBUG_LOWER_TRACE").is_ok() {
            let ctor_ty = constructor_type
                .map(|ty| ty.as_u32().to_string())
                .unwrap_or_else(|| "none".to_string());
            eprintln!(
                "[lower] late-bound bind '{}' <- '{}' ctor_ty={}",
                self.interner.resolve(name),
                self.interner.resolve(constructor_symbol),
                ctor_ty
            );
        }
    }
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
        assert!(
            module.function_count() >= 2,
            "Should have at least Injectable and main functions, got {}",
            module.function_count()
        );
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
        assert!(
            module.function_count() >= 3,
            "Should have Log, getUsers, and main functions, got {}",
            module.function_count()
        );
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
        assert!(
            module.function_count() >= 2,
            "Should have Column and main functions, got {}",
            module.function_count()
        );
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
        assert!(
            module.function_count() >= 4,
            "Should have A, B, C, and main functions, got {}",
            module.function_count()
        );
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
        assert!(
            module.function_count() >= 2,
            "Should have Controller and main functions, got {}",
            module.function_count()
        );
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
        assert!(
            main_func.is_some(),
            "Should have main function with decorator init"
        );
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
        assert!(
            module.function_count() >= 5,
            "Should have all decorator functions plus method and main, got {}",
            module.function_count()
        );
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
        assert!(
            module.function_count() >= 3,
            "Should have Route, getUsers, and main functions, got {}",
            module.function_count()
        );
        assert_eq!(module.class_count(), 1, "Should have 1 class");
    }
}

#[cfg(test)]
mod nominal_type_identity_tests {
    use super::*;
    use crate::parser::{Parser, TypeContext};

    #[test]
    fn nested_class_registration_handles_all_statement_shapes() {
        let source = r#"
            function outer(): void {
                if (true) { class IfClass {} } else { class ElseClass {} }
                while (false) { class WhileClass {}; break; }
                do { class DoWhileClass {}; } while (false);
                for (;;) { class ForClass {}; break; }
                for (let x of [1]) { class ForOfClass {}; break; }
                for (let k in { a: 1 }) { class ForInClass {}; break; }
                try { class TryClass {} }
                catch (e) { class CatchClass {} }
                finally { class FinallyClass {} }
                function inner(): void { class InnerClass {} }
            }
        "#;

        let parser = Parser::new(source).expect("lexer error");
        let (module, interner) = parser.parse().expect("parse error");
        let type_ctx = TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let ir = lowerer.lower_module(&module);
        assert!(
            lowerer.errors().is_empty(),
            "unexpected lowerer errors: {:?}",
            lowerer.errors()
        );

        let mut names: Vec<String> = ir.classes().map(|c| c.name.clone()).collect();
        names.sort();
        for expected in [
            "CatchClass",
            "DoWhileClass",
            "ElseClass",
            "FinallyClass",
            "ForClass",
            "ForInClass",
            "ForOfClass",
            "IfClass",
            "InnerClass",
            "TryClass",
            "WhileClass",
        ] {
            assert!(
                names.iter().any(|n| n == expected),
                "missing class '{}' in {:?}",
                expected,
                names
            );
        }
    }

    #[test]
    fn classes_emit_in_nominal_type_id_order() {
        let source = r#"
            class Top {}
            function make(): void {
                class LocalA {}
                if (true) { class LocalB {} }
            }
        "#;

        let parser = Parser::new(source).expect("lexer error");
        let (module, interner) = parser.parse().expect("parse error");
        let type_ctx = TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let ir = lowerer.lower_module(&module);
        assert!(
            lowerer.errors().is_empty(),
            "unexpected lowerer errors: {:?}",
            lowerer.errors()
        );
        let names: Vec<&str> = ir.classes().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["Top", "LocalA", "LocalB"]);
    }

    #[test]
    fn std_alias_mapping_is_exact_per_wrapper_context() {
        let source = r#"
            function __std_module_alpha() {
                class EnvNamespace {}
                return { "default": new EnvNamespace() };
            }

            function __std_module_beta() {
                class EnvNamespace {}
                return { "default": new EnvNamespace() };
            }
        "#;

        let parser = Parser::new(source).expect("lexer error");
        let (module, interner) = parser.parse().expect("parse error");
        let type_ctx = TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let _ir = lowerer.lower_module(&module);
        assert!(
            lowerer.errors().is_empty(),
            "unexpected lowerer errors: {:?}",
            lowerer.errors()
        );

        let alpha = lowerer
            .nominal_type_id_from_type_name("__t_alpha_EnvNamespace")
            .expect("alpha alias should resolve");
        let beta = lowerer
            .nominal_type_id_from_type_name("__t_beta_EnvNamespace")
            .expect("beta alias should resolve");
        assert_ne!(
            alpha, beta,
            "wrapper aliases must map to distinct class ids"
        );
    }

    #[test]
    fn raya_module_alias_mapping_is_exact_per_module_context() {
        let source = r#"
            function __raya_mod_init_1() {
                class EnvNamespace {}
                return { "__default": new EnvNamespace() };
            }

            function __raya_mod_init_2() {
                class EnvNamespace {}
                return { "__default": new EnvNamespace() };
            }
        "#;

        let parser = Parser::new(source).expect("lexer error");
        let (module, interner) = parser.parse().expect("parse error");
        let type_ctx = TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let _ir = lowerer.lower_module(&module);
        assert!(
            lowerer.errors().is_empty(),
            "unexpected lowerer errors: {:?}",
            lowerer.errors()
        );

        let m1 = lowerer
            .nominal_type_id_from_type_name("__t_m1_EnvNamespace")
            .expect("module 1 alias should resolve");
        let m2 = lowerer
            .nominal_type_id_from_type_name("__t_m2_EnvNamespace")
            .expect("module 2 alias should resolve");
        assert_ne!(m1, m2, "module aliases must map to distinct class ids");
    }

    #[test]
    fn module_wrapper_cast_binding_uses_class_dispatch() {
        use crate::compiler::ir::IrInstr;

        let source = r#"
            type __t_env_EnvNamespace = { cwd: () => string };
            type __std_exports_type_env = { default: unknown };

            function __std_module_env(): __std_exports_type_env {
                class EnvNamespace {
                    cwd(): string { return "x"; }
                }
                const env = new EnvNamespace();
                return { default: env };
            }

            const __std_exports_env = __std_module_env();
            const env = (__std_exports_env.default as __t_env_EnvNamespace);
            env.cwd();
        "#;

        let parser = Parser::new(source).expect("lexer error");
        let (module, interner) = parser.parse().expect("parse error");
        let type_ctx = TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let ir = lowerer.lower_module(&module);
        assert!(
            lowerer.errors().is_empty(),
            "unexpected lowerer errors: {:?}",
            lowerer.errors()
        );

        let env_sym = interner.lookup("env").expect("env symbol");
        let env_nominal_type_id = lowerer
            .variable_class_map
            .get(&env_sym)
            .copied()
            .expect("env variable should have class mapping");
        assert_eq!(
            ir.classes[env_nominal_type_id.as_u32() as usize].name,
            "EnvNamespace",
            "env should map to wrapper class id"
        );

        let call_method_count = ir
            .functions
            .iter()
            .flat_map(|f| &f.blocks)
            .flat_map(|b| &b.instructions)
            .filter(|instr| matches!(instr, IrInstr::CallMethodExact { .. }))
            .count();
        assert!(
            call_method_count > 0,
            "expected CallMethodExact dispatch, got no CallMethodExact in module IR"
        );
    }

    #[test]
    fn std_logger_default_cast_infers_nominal_type_identity() {
        let source = r#"
            type __t_logger_LoggerNamespace = { info: (message: string) => void };
            type __std_exports_type_logger = { default: unknown, info: unknown };

            function __std_module_logger(): __std_exports_type_logger {
                function info(message: string): void {}
                class LoggerNamespace {
                    info(message: string): void { info(message); }
                }
                const logger = new LoggerNamespace();
                return { default: logger, info: info };
            }

            const __std_exports_logger = __std_module_logger();
            const logger = (__std_exports_logger.default as __t_logger_LoggerNamespace);
            logger.info("hi");
        "#;

        let parser = Parser::new(source).expect("lexer error");
        let (module, interner) = parser.parse().expect("parse error");
        let type_ctx = TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let _ir = lowerer.lower_module(&module);
        assert!(
            lowerer.errors().is_empty(),
            "unexpected lowerer errors: {:?}",
            lowerer.errors()
        );

        let nominal_type_id = lowerer
            .nominal_type_id_from_type_name("__t_logger_LoggerNamespace")
            .expect("logger alias should resolve");
        let logger_sym = interner.lookup("logger").expect("logger symbol");
        let var_class = lowerer
            .variable_class_map
            .get(&logger_sym)
            .copied()
            .expect("logger variable should preserve class mapping");
        assert_eq!(
            var_class, nominal_type_id,
            "logger cast alias should preserve LoggerNamespace class id"
        );
    }

    #[test]
    fn function_and_method_nullable_class_returns_preserve_nominal_type_identity() {
        let source = r#"
            class JsonValue {
                get(): JsonValue | null { return null; }
            }
            class Util {
                static mk(): JsonValue | null { return null; }
            }

            function make(): JsonValue | null { return null; }

            const fromFn = make();
            const base = new JsonValue();
            const fromMethod = base.get();
            const fromStatic = Util.mk();
        "#;

        let parser = Parser::new(source).expect("lexer error");
        let (module, interner) = parser.parse().expect("parse error");
        let type_ctx = TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let _ir = lowerer.lower_module(&module);
        assert!(
            lowerer.errors().is_empty(),
            "unexpected lowerer errors: {:?}",
            lowerer.errors()
        );

        let json_sym = interner.lookup("JsonValue").expect("JsonValue symbol");
        let json_class = lowerer
            .class_map
            .get(&json_sym)
            .copied()
            .expect("JsonValue class id");

        let from_fn_sym = interner.lookup("fromFn").expect("fromFn symbol");
        let from_method_sym = interner.lookup("fromMethod").expect("fromMethod symbol");
        let from_static_sym = interner.lookup("fromStatic").expect("fromStatic symbol");
        assert_eq!(
            lowerer.variable_class_map.get(&from_fn_sym).copied(),
            Some(json_class),
            "function nullable return should preserve class id"
        );
        assert_eq!(
            lowerer.variable_class_map.get(&from_method_sym).copied(),
            Some(json_class),
            "method nullable return should preserve class id"
        );
        assert_eq!(
            lowerer.variable_class_map.get(&from_static_sym).copied(),
            Some(json_class),
            "static nullable return should preserve class id"
        );
    }

    #[test]
    fn import_local_bindings_are_registered_as_module_globals() {
        let source = r#"
            import { value as importedValue } from "./utils";
            let x: number = importedValue;
        "#;

        let parser = Parser::new(source).expect("lexer error");
        let (module, interner) = parser.parse().expect("parse error");
        let type_ctx = TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let _ir = lowerer.lower_module(&module);

        assert!(
            lowerer.errors().is_empty(),
            "unexpected lowerer errors: {:?}",
            lowerer.errors()
        );

        let imported_sym = interner
            .lookup("importedValue")
            .expect("imported binding symbol");
        assert!(
            lowerer.module_var_globals.contains_key(&imported_sym),
            "import-local binding should be pre-registered as module global"
        );
    }

    #[test]
    fn nominal_type_identifier_value_lowers_to_nominal_type_id_constant() {
        use crate::compiler::ir::{IrConstant, IrInstr, IrValue};

        let source = r#"
            class A {}
            const B = A;
        "#;

        let parser = Parser::new(source).expect("lexer error");
        let (module, interner) = parser.parse().expect("parse error");
        let type_ctx = TypeContext::new();
        let mut lowerer = Lowerer::new(&type_ctx, &interner);
        let ir = lowerer.lower_module(&module);
        assert!(
            lowerer.errors().is_empty(),
            "unexpected lowerer errors: {:?}",
            lowerer.errors()
        );

        let a_sym = interner.lookup("A").expect("A symbol");
        let a_nominal_type_id = lowerer.class_map.get(&a_sym).copied().expect("A class id");
        let expected = a_nominal_type_id.as_u32() as i32;

        let main = ir.get_function_by_name("main").expect("main function");
        let has_nominal_type_id_i32_assign =
            main.blocks
                .iter()
                .flat_map(|b| &b.instructions)
                .any(|instr| {
                    matches!(
                        instr,
                        IrInstr::Assign {
                            value: IrValue::Constant(IrConstant::I32(v)),
                            ..
                        } if *v == expected
                    )
                });
        let has_null_assign = main
            .blocks
            .iter()
            .flat_map(|b| &b.instructions)
            .any(|instr| {
                matches!(
                    instr,
                    IrInstr::Assign {
                        value: IrValue::Constant(IrConstant::Null),
                        ..
                    }
                )
            });
        assert!(
            has_nominal_type_id_i32_assign,
            "expected class id constant assignment in main IR (class id {}), main blocks: {:?}",
            expected, main.blocks
        );
        assert!(
            !has_null_assign,
            "class identifier value should not lower to null in this case"
        );
    }
}
