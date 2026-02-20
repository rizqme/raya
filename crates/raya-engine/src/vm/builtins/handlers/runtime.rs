//! Runtime method handlers (std:runtime)
//!
//! Native implementation of std:runtime module for compiling, executing,
//! and managing bytecode at runtime.

use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Instant;

use parking_lot::Mutex;

use crate::compiler::{verify_module, Compiler, Module};
use crate::parser::ast;
use crate::parser::checker::{Binder, ScopeId, TypeChecker};
use crate::parser::{Interner, Parser, TypeContext, TypeId};
use crate::vm::builtin::runtime;
use crate::vm::gc::GarbageCollector as Gc;
use crate::vm::object::{Buffer, RayaString};
use crate::vm::stack::Stack;
use crate::vm::value::Value;
use crate::vm::Vm;
use crate::vm::VmError;
use rustc_hash::FxHashMap;

// ============================================================================
// Compiled Module Registry
// ============================================================================

/// Registry of compiled modules, keyed by integer ID
struct CompiledModuleRegistry {
    modules: HashMap<u32, Module>,
    names: HashMap<String, u32>,
    next_id: u32,
}

impl CompiledModuleRegistry {
    fn new() -> Self {
        Self {
            modules: HashMap::new(),
            names: HashMap::new(),
            next_id: 1,
        }
    }

    fn register(&mut self, module: Module) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.modules.insert(id, module);
        id
    }

    fn register_named(&mut self, name: String, module: Module) -> u32 {
        let id = self.register(module);
        self.names.insert(name, id);
        id
    }

    fn get(&self, id: u32) -> Option<&Module> {
        self.modules.get(&id)
    }
}

static COMPILED_MODULE_REGISTRY: LazyLock<Mutex<CompiledModuleRegistry>> =
    LazyLock::new(|| Mutex::new(CompiledModuleRegistry::new()));

// ============================================================================
// AST Registry (Phase 2)
// ============================================================================

/// A parsed (but not yet type-checked) AST
struct AstEntry {
    ast: ast::Module,
    interner: Interner,
}

/// A type-checked AST ready for compilation
struct TypedAstEntry {
    ast: ast::Module,
    interner: Interner,
    type_ctx: TypeContext,
    #[allow(dead_code)]
    symbols: crate::parser::SymbolTable,
    expr_types: FxHashMap<usize, TypeId>,
}

/// Registry of parsed and type-checked ASTs, keyed by integer ID
struct AstRegistry {
    parsed: HashMap<u32, AstEntry>,
    typed: HashMap<u32, TypedAstEntry>,
    next_id: u32,
}

impl AstRegistry {
    fn new() -> Self {
        Self {
            parsed: HashMap::new(),
            typed: HashMap::new(),
            next_id: 1,
        }
    }

    fn register_parsed(&mut self, entry: AstEntry) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.parsed.insert(id, entry);
        id
    }

    fn register_typed(&mut self, entry: TypedAstEntry) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.typed.insert(id, entry);
        id
    }
}

static AST_REGISTRY: LazyLock<Mutex<AstRegistry>> =
    LazyLock::new(|| Mutex::new(AstRegistry::new()));

// ============================================================================
// VM Instance Registry (Phase 3)
// ============================================================================

/// Permission policy for a VM instance
#[derive(Clone, Debug)]
struct VmPermissions {
    /// Allowed stdlib modules: ["*"] = all, or specific names like ["std:math", "std:logger"]
    allow_stdlib: Vec<String>,
    /// Allow use of Reflect API
    allow_reflect: bool,
    /// Allow querying VM info (Vm.current(), etc.)
    allow_vm_access: bool,
    /// Allow spawning child VMs
    allow_vm_spawn: bool,
    /// Allow loading external .ryb files
    allow_lib_load: bool,
    /// Allow __NATIVE_CALL usage
    allow_native_calls: bool,
    /// Allow Compiler.eval / VmInstance.eval
    allow_eval: bool,
    /// Allow Bytecode.encode / Bytecode.decode
    allow_binary_io: bool,
}

impl VmPermissions {
    /// Root VM: all permissions enabled
    fn root() -> Self {
        Self {
            allow_stdlib: vec!["*".to_string()],
            allow_reflect: true,
            allow_vm_access: true,
            allow_vm_spawn: true,
            allow_lib_load: true,
            allow_native_calls: true,
            allow_eval: true,
            allow_binary_io: true,
        }
    }

    /// Default child permissions (restrictive)
    fn child_default() -> Self {
        Self {
            allow_stdlib: vec!["*".to_string()],
            allow_reflect: false,
            allow_vm_access: true,
            allow_vm_spawn: false,
            allow_lib_load: false,
            allow_native_calls: true,
            allow_eval: true,
            allow_binary_io: true,
        }
    }

    /// Check if a named permission is enabled
    fn has_permission(&self, name: &str) -> bool {
        match name {
            "eval" => self.allow_eval,
            "binaryIO" => self.allow_binary_io,
            "vmSpawn" => self.allow_vm_spawn,
            "vmAccess" => self.allow_vm_access,
            "libLoad" => self.allow_lib_load,
            "reflect" => self.allow_reflect,
            "nativeCalls" => self.allow_native_calls,
            _ => false,
        }
    }

    /// Check if a specific stdlib module is allowed
    fn is_stdlib_allowed(&self, module: &str) -> bool {
        self.allow_stdlib.iter().any(|s| s == "*" || s == module)
    }

    /// Get a comma-separated list of enabled permission names
    fn to_string_list(&self) -> String {
        let mut perms = Vec::new();
        if self.allow_eval { perms.push("eval"); }
        if self.allow_binary_io { perms.push("binaryIO"); }
        if self.allow_vm_spawn { perms.push("vmSpawn"); }
        if self.allow_vm_access { perms.push("vmAccess"); }
        if self.allow_lib_load { perms.push("libLoad"); }
        if self.allow_reflect { perms.push("reflect"); }
        if self.allow_native_calls { perms.push("nativeCalls"); }
        perms.join(",")
    }

    /// Get a comma-separated list of allowed stdlib modules
    fn allowed_stdlib_list(&self) -> String {
        self.allow_stdlib.join(",")
    }
}

/// A single VM instance entry
#[allow(dead_code)]
struct VmInstanceEntry {
    id: u32,
    /// The actual VM — None for root or while borrowed for execution
    vm: Option<Vm>,
    /// Per-instance compiled module registry
    modules: CompiledModuleRegistry,
    /// Parent instance ID (None for root)
    parent_id: Option<u32>,
    /// Child instance IDs
    children: Vec<u32>,
    /// Whether the instance is alive (false after terminate)
    is_alive: bool,
    /// Permission policy
    permissions: VmPermissions,
    /// Debug state for debugger coordination (None = no debugger)
    debug_state: Option<std::sync::Arc<crate::vm::interpreter::DebugState>>,
}

/// Global registry of VM instances
struct VmInstanceRegistry {
    instances: HashMap<u32, VmInstanceEntry>,
    /// The root instance ID (lazily created on first Vm.current())
    root_id: Option<u32>,
    next_id: u32,
}

impl VmInstanceRegistry {
    fn new() -> Self {
        Self {
            instances: HashMap::new(),
            root_id: None,
            next_id: 1,
        }
    }

    /// Get or create the root VM instance handle
    fn get_or_create_root(&mut self) -> u32 {
        if let Some(id) = self.root_id {
            return id;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.root_id = Some(id);
        self.instances.insert(id, VmInstanceEntry {
            id,
            vm: None, // Root has no owned Vm — delegates to global registries
            modules: CompiledModuleRegistry::new(),
            parent_id: None,
            children: Vec::new(),
            is_alive: true,
            permissions: VmPermissions::root(),
            debug_state: None,
        });
        id
    }

    /// Spawn a new child instance
    fn spawn(&mut self, parent_id: u32) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        let vm = Vm::with_worker_count(1);
        self.instances.insert(id, VmInstanceEntry {
            id,
            vm: Some(vm),
            modules: CompiledModuleRegistry::new(),
            parent_id: Some(parent_id),
            children: Vec::new(),
            is_alive: true,
            permissions: VmPermissions::child_default(),
            debug_state: None,
        });
        // Register child in parent
        if let Some(parent) = self.instances.get_mut(&parent_id) {
            parent.children.push(id);
        }
        id
    }

    /// Terminate an instance and all its descendants (cascading)
    fn terminate(&mut self, id: u32) {
        // Collect children first to avoid borrow issues
        let children = self.instances
            .get(&id)
            .map(|e| e.children.clone())
            .unwrap_or_default();

        // Recursively terminate children
        for child_id in children {
            self.terminate(child_id);
        }

        // Terminate this instance
        if let Some(entry) = self.instances.get_mut(&id) {
            entry.is_alive = false;
            entry.vm = None; // Drop the Vm (stops scheduler/worker threads)
            entry.modules = CompiledModuleRegistry::new(); // Clear modules
        }
    }
}

static VM_INSTANCE_REGISTRY: LazyLock<Mutex<VmInstanceRegistry>> =
    LazyLock::new(|| Mutex::new(VmInstanceRegistry::new()));

/// VM start time for uptime tracking
static VM_START_TIME: LazyLock<Instant> = LazyLock::new(Instant::now);

// ============================================================================
// Handler Context
// ============================================================================

/// Context needed for runtime method execution
pub struct RuntimeHandlerContext<'a> {
    pub gc: &'a Mutex<Gc>,
}

// ============================================================================
// Main dispatch
// ============================================================================

/// Check if the current VM has a specific permission
fn check_permission(perm_name: &str) -> Result<(), VmError> {
    let registry = VM_INSTANCE_REGISTRY.lock();
    if let Some(root_id) = registry.root_id {
        if let Some(entry) = registry.instances.get(&root_id) {
            if !entry.permissions.has_permission(perm_name) {
                return Err(VmError::RuntimeError(format!(
                    "Permission denied: {} not allowed",
                    perm_name
                )));
            }
        }
    }
    // No registry or no root = no restrictions (root default)
    Ok(())
}

/// Handle built-in runtime methods (std:runtime)
pub fn call_runtime_method(
    ctx: &RuntimeHandlerContext,
    stack: &mut Stack,
    method_id: u16,
    arg_count: usize,
) -> Result<(), VmError> {
    // Pop arguments
    let mut args = Vec::with_capacity(arg_count);
    for _ in 0..arg_count {
        args.push(stack.pop()?);
    }
    args.reverse();

    // Helper to get string from Value
    let get_string = |v: Value| -> Result<String, VmError> {
        if !v.is_ptr() {
            return Err(VmError::TypeError("Expected string".to_string()));
        }
        let s_ptr = unsafe { v.as_ptr::<RayaString>() };
        let s = unsafe { &*s_ptr.unwrap().as_ptr() };
        Ok(s.data.clone())
    };

    let result = match method_id {
        runtime::COMPILE => {
            // Compiler.compile(source: string): number
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "Compiler.compile requires 1 argument".to_string(),
                ));
            }
            let source = get_string(args[0])?;
            let module = compile_source(&source)?;
            let mut registry = COMPILED_MODULE_REGISTRY.lock();
            let id = registry.register(module);
            Value::i32(id as i32)
        }

        runtime::COMPILE_EXPRESSION => {
            // Compiler.compileExpression(expr: string): number
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "Compiler.compileExpression requires 1 argument".to_string(),
                ));
            }
            let expr = get_string(args[0])?;
            let wrapped = format!("return {};", expr);
            let module = compile_source(&wrapped)?;
            let mut registry = COMPILED_MODULE_REGISTRY.lock();
            let id = registry.register(module);
            Value::i32(id as i32)
        }

        runtime::EVAL => {
            // Compiler.eval(source: string): number
            check_permission("eval")?;
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "Compiler.eval requires 1 argument".to_string(),
                ));
            }
            let source = get_string(args[0])?;

            // Wrap in a function so `return` works at top level
            let wrapped = format!("function __eval__(): number {{ {} }}\nreturn __eval__();", source);
            let module = compile_source(&wrapped)?;
            execute_module(&module)?
        }

        runtime::EXECUTE => {
            // Compiler.execute(moduleId: number): number
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "Compiler.execute requires 1 argument".to_string(),
                ));
            }
            let module_id = args[0]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for moduleId".to_string()))?
                as u32;

            let registry = COMPILED_MODULE_REGISTRY.lock();
            let module = registry
                .get(module_id)
                .ok_or_else(|| {
                    VmError::RuntimeError(format!("Module not found: {}", module_id))
                })?
                .clone();
            drop(registry);

            execute_module(&module)?
        }

        runtime::EXECUTE_FUNCTION => {
            // Compiler.executeFunction(moduleId: number, funcName: string, ...args): number
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "Compiler.executeFunction requires at least 2 arguments".to_string(),
                ));
            }
            let module_id = args[0]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for moduleId".to_string()))?
                as u32;
            let _func_name = get_string(args[1])?;

            let registry = COMPILED_MODULE_REGISTRY.lock();
            let module = registry
                .get(module_id)
                .ok_or_else(|| {
                    VmError::RuntimeError(format!("Module not found: {}", module_id))
                })?
                .clone();
            drop(registry);

            // For now, execute the module's main entry point
            // TODO: In future phases, support calling specific functions by name
            execute_module(&module)?
        }

        runtime::ENCODE => {
            // Bytecode.encode(moduleId: number): Buffer
            check_permission("binaryIO")?;
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "Bytecode.encode requires 1 argument".to_string(),
                ));
            }
            let module_id = args[0]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for moduleId".to_string()))?
                as u32;

            let registry = COMPILED_MODULE_REGISTRY.lock();
            let module = registry
                .get(module_id)
                .ok_or_else(|| {
                    VmError::RuntimeError(format!("Module not found: {}", module_id))
                })?;
            let bytes = module.encode();
            drop(registry);

            // Create a Buffer from the encoded bytes
            let mut buffer = Buffer::new(bytes.len());
            for (i, &byte) in bytes.iter().enumerate() {
                let _ = buffer.set_byte(i, byte);
            }

            // Allocate on heap
            let gc_ptr = ctx.gc.lock().allocate(buffer);
            unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
        }

        runtime::DECODE => {
            // Bytecode.decode(data: Buffer): number
            check_permission("binaryIO")?;
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "Bytecode.decode requires 1 argument".to_string(),
                ));
            }

            // Read Buffer from heap
            let buf_val = args[0];
            if !buf_val.is_ptr() {
                return Err(VmError::TypeError("Expected Buffer for data".to_string()));
            }
            let buf_ptr = unsafe { buf_val.as_ptr::<Buffer>() };
            let buffer = unsafe { &*buf_ptr.unwrap().as_ptr() };
            let bytes: Vec<u8> = (0..buffer.length()).filter_map(|i| buffer.get_byte(i)).collect();

            let module = Module::decode(&bytes).map_err(|e| {
                VmError::RuntimeError(format!("Failed to decode module: {}", e))
            })?;

            let mut registry = COMPILED_MODULE_REGISTRY.lock();
            let id = registry.register(module);
            Value::i32(id as i32)
        }

        runtime::LOAD_LIBRARY => {
            // Bytecode.loadLibrary(path: string): number
            check_permission("libLoad")?;
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "Bytecode.loadLibrary requires 1 argument".to_string(),
                ));
            }
            let path = get_string(args[0])?;

            let bytes = std::fs::read(&path).map_err(|e| {
                VmError::RuntimeError(format!("Failed to read file '{}': {}", path, e))
            })?;

            let module = Module::decode(&bytes).map_err(|e| {
                VmError::RuntimeError(format!("Failed to decode module from '{}': {}", path, e))
            })?;

            let mut registry = COMPILED_MODULE_REGISTRY.lock();
            let id = registry.register(module);
            Value::i32(id as i32)
        }

        runtime::LOAD_DEPENDENCY => {
            // Bytecode.loadDependency(path: string, name: string): number
            check_permission("libLoad")?;
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "Bytecode.loadDependency requires 2 arguments".to_string(),
                ));
            }
            let path = get_string(args[0])?;
            let name = get_string(args[1])?;

            let bytes = std::fs::read(&path).map_err(|e| {
                VmError::RuntimeError(format!("Failed to read file '{}': {}", path, e))
            })?;

            let module = Module::decode(&bytes).map_err(|e| {
                VmError::RuntimeError(format!(
                    "Failed to decode dependency '{}' from '{}': {}",
                    name, path, e
                ))
            })?;

            let mut registry = COMPILED_MODULE_REGISTRY.lock();
            let id = registry.register_named(name, module);
            Value::i32(id as i32)
        }

        runtime::COMPILE_AST => {
            // Compiler.compileAst(astId: number): number
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "Compiler.compileAst requires 1 argument".to_string(),
                ));
            }
            let ast_id = args[0]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for astId".to_string()))?
                as u32;

            let mut ast_reg = AST_REGISTRY.lock();
            let entry = ast_reg
                .typed
                .remove(&ast_id)
                .ok_or_else(|| {
                    VmError::RuntimeError(format!("Typed AST not found: {}", ast_id))
                })?;
            drop(ast_reg);

            let TypedAstEntry { ast, interner, type_ctx, symbols: _, expr_types } = entry;
            let compiler = Compiler::new(type_ctx, &interner)
                .with_expr_types(expr_types);
            let module = compiler.compile_via_ir(&ast).map_err(|e| {
                VmError::RuntimeError(format!("Compile error: {}", e))
            })?;

            let mut registry = COMPILED_MODULE_REGISTRY.lock();
            let id = registry.register(module);
            Value::i32(id as i32)
        }

        runtime::RESOLVE_DEPENDENCY => {
            // Bytecode.resolveDependency(name: string): number
            check_permission("libLoad")?;
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "Bytecode.resolveDependency requires 1 argument".to_string(),
                ));
            }
            let name = get_string(args[0])?;

            // Search paths in order
            let search_paths = get_dependency_search_paths(&name);

            let mut found_bytes = None;
            for path in &search_paths {
                if let Ok(bytes) = std::fs::read(path) {
                    found_bytes = Some(bytes);
                    break;
                }
            }

            let bytes = found_bytes.ok_or_else(|| {
                VmError::RuntimeError(format!(
                    "Dependency '{}' not found in search paths: {:?}",
                    name, search_paths
                ))
            })?;

            let module = Module::decode(&bytes).map_err(|e| {
                VmError::RuntimeError(format!(
                    "Failed to decode dependency '{}': {}",
                    name, e
                ))
            })?;

            let mut registry = COMPILED_MODULE_REGISTRY.lock();
            let id = registry.register_named(name, module);
            Value::i32(id as i32)
        }

        // ── Bytecode Inspection (Phase 2) ──

        runtime::VALIDATE => {
            // Bytecode.validate(moduleId: number): boolean
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "Bytecode.validate requires 1 argument".to_string(),
                ));
            }
            let module_id = args[0]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for moduleId".to_string()))?
                as u32;

            let registry = COMPILED_MODULE_REGISTRY.lock();
            let module = registry
                .get(module_id)
                .ok_or_else(|| {
                    VmError::RuntimeError(format!("Module not found: {}", module_id))
                })?;
            let is_valid = verify_module(module).is_ok();
            drop(registry);
            Value::bool(is_valid)
        }

        runtime::DISASSEMBLE => {
            // Bytecode.disassemble(moduleId: number): string
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "Bytecode.disassemble requires 1 argument".to_string(),
                ));
            }
            let module_id = args[0]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for moduleId".to_string()))?
                as u32;

            let registry = COMPILED_MODULE_REGISTRY.lock();
            let module = registry
                .get(module_id)
                .ok_or_else(|| {
                    VmError::RuntimeError(format!("Module not found: {}", module_id))
                })?;
            let mut output = String::new();
            for func in &module.functions {
                output.push_str(&format!("function {}:\n", func.name));
                output.push_str(&crate::compiler::disassemble_function(func));
                output.push('\n');
            }
            drop(registry);
            allocate_string(ctx, output)
        }

        runtime::GET_MODULE_NAME => {
            // Bytecode.getModuleName(moduleId: number): string
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "Bytecode.getModuleName requires 1 argument".to_string(),
                ));
            }
            let module_id = args[0]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for moduleId".to_string()))?
                as u32;

            let registry = COMPILED_MODULE_REGISTRY.lock();
            let module = registry
                .get(module_id)
                .ok_or_else(|| {
                    VmError::RuntimeError(format!("Module not found: {}", module_id))
                })?;
            let name = module.metadata.name.clone();
            drop(registry);
            allocate_string(ctx, name)
        }

        runtime::GET_MODULE_FUNCTIONS => {
            // Bytecode.getModuleFunctions(moduleId: number): string
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "Bytecode.getModuleFunctions requires 1 argument".to_string(),
                ));
            }
            let module_id = args[0]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for moduleId".to_string()))?
                as u32;

            let registry = COMPILED_MODULE_REGISTRY.lock();
            let module = registry
                .get(module_id)
                .ok_or_else(|| {
                    VmError::RuntimeError(format!("Module not found: {}", module_id))
                })?;
            let names: Vec<String> = module.functions.iter().map(|f| f.name.clone()).collect();
            drop(registry);
            allocate_string(ctx, names.join(","))
        }

        runtime::GET_MODULE_CLASSES => {
            // Bytecode.getModuleClasses(moduleId: number): string
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "Bytecode.getModuleClasses requires 1 argument".to_string(),
                ));
            }
            let module_id = args[0]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for moduleId".to_string()))?
                as u32;

            let registry = COMPILED_MODULE_REGISTRY.lock();
            let module = registry
                .get(module_id)
                .ok_or_else(|| {
                    VmError::RuntimeError(format!("Module not found: {}", module_id))
                })?;
            let names: Vec<String> = module.classes.iter().map(|c| c.name.clone()).collect();
            drop(registry);
            allocate_string(ctx, names.join(","))
        }

        // ── Parser (Phase 2) ──

        runtime::PARSE => {
            // Parser.parse(source: string): number
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "Parser.parse requires 1 argument".to_string(),
                ));
            }
            let source = get_string(args[0])?;
            let (ast, interner) = parse_source(&source)?;
            let mut ast_reg = AST_REGISTRY.lock();
            let id = ast_reg.register_parsed(AstEntry { ast, interner });
            Value::i32(id as i32)
        }

        runtime::PARSE_EXPRESSION => {
            // Parser.parseExpression(expr: string): number
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "Parser.parseExpression requires 1 argument".to_string(),
                ));
            }
            let expr = get_string(args[0])?;
            let wrapped = format!("return {};", expr);
            let (ast, interner) = parse_source(&wrapped)?;
            let mut ast_reg = AST_REGISTRY.lock();
            let id = ast_reg.register_parsed(AstEntry { ast, interner });
            Value::i32(id as i32)
        }

        // ── TypeChecker (Phase 2) ──

        runtime::CHECK | runtime::CHECK_EXPRESSION => {
            // TypeChecker.check(astId: number): number
            // TypeChecker.checkExpression(astId: number): number
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "TypeChecker.check requires 1 argument".to_string(),
                ));
            }
            let ast_id = args[0]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for astId".to_string()))?
                as u32;

            let ast_reg = AST_REGISTRY.lock();
            let entry = ast_reg
                .parsed
                .get(&ast_id)
                .ok_or_else(|| {
                    VmError::RuntimeError(format!("Parsed AST not found: {}", ast_id))
                })?;
            let ast = entry.ast.clone();
            let interner = entry.interner.clone();
            drop(ast_reg);

            let typed = typecheck_ast(ast.clone(), interner.clone())?;
            let mut ast_reg = AST_REGISTRY.lock();
            let id = ast_reg.register_typed(typed);
            Value::i32(id as i32)
        }

        // ── Vm class (Phase 3) ──

        runtime::VM_CURRENT => {
            // Vm.current(): returns root instance handle
            let mut registry = VM_INSTANCE_REGISTRY.lock();
            let root_id = registry.get_or_create_root();
            Value::i32(root_id as i32)
        }

        runtime::VM_SPAWN => {
            // Vm.spawn(): creates a child VM instance
            let mut registry = VM_INSTANCE_REGISTRY.lock();
            let root_id = registry.get_or_create_root();
            // Check if current VM has vmSpawn permission
            let root = registry.instances.get(&root_id)
                .ok_or_else(|| VmError::RuntimeError("Root VM not found".to_string()))?;
            if !root.permissions.allow_vm_spawn {
                return Err(VmError::RuntimeError(
                    "Permission denied: vmSpawn not allowed".to_string(),
                ));
            }
            let child_id = registry.spawn(root_id);
            Value::i32(child_id as i32)
        }

        // ── Permission management (Phase 4) ──

        runtime::HAS_PERMISSION => {
            // Vm.hasPermission(name: string): boolean
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "Vm.hasPermission requires 1 argument".to_string(),
                ));
            }
            let name = get_string(args[0])?;
            let registry = VM_INSTANCE_REGISTRY.lock();
            let root_id = registry.root_id.unwrap_or(0);
            let has_perm = registry.instances
                .get(&root_id)
                .map(|e| e.permissions.has_permission(&name))
                .unwrap_or(true); // No registry yet = root = all perms
            Value::bool(has_perm)
        }

        runtime::GET_PERMISSIONS => {
            // Vm.getPermissions(): string (comma-separated permission names)
            let registry = VM_INSTANCE_REGISTRY.lock();
            let root_id = registry.root_id.unwrap_or(0);
            let perms_str = registry.instances
                .get(&root_id)
                .map(|e| e.permissions.to_string_list())
                .unwrap_or_else(|| VmPermissions::root().to_string_list());
            drop(registry);
            allocate_string(ctx, perms_str)
        }

        runtime::GET_ALLOWED_STDLIB => {
            // Vm.getAllowedStdlib(): string (comma-separated stdlib module names)
            let registry = VM_INSTANCE_REGISTRY.lock();
            let root_id = registry.root_id.unwrap_or(0);
            let stdlib_str = registry.instances
                .get(&root_id)
                .map(|e| e.permissions.allowed_stdlib_list())
                .unwrap_or_else(|| "*".to_string());
            drop(registry);
            allocate_string(ctx, stdlib_str)
        }

        runtime::IS_STDLIB_ALLOWED => {
            // Vm.isStdlibAllowed(module: string): boolean
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "Vm.isStdlibAllowed requires 1 argument".to_string(),
                ));
            }
            let module_name = get_string(args[0])?;
            let registry = VM_INSTANCE_REGISTRY.lock();
            let root_id = registry.root_id.unwrap_or(0);
            let allowed = registry.instances
                .get(&root_id)
                .map(|e| e.permissions.is_stdlib_allowed(&module_name))
                .unwrap_or(true); // No registry yet = root = all allowed
            Value::bool(allowed)
        }

        // ── VmInstance methods (Phase 3) ──

        runtime::VM_INSTANCE_ID => {
            // instance.id(): returns instance handle
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "VmInstance.id requires 1 argument".to_string(),
                ));
            }
            let handle = args[0]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))?;
            Value::i32(handle)
        }

        runtime::VM_INSTANCE_IS_ROOT => {
            // instance.isRoot(): check if this is the root VM
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "VmInstance.isRoot requires 1 argument".to_string(),
                ));
            }
            let handle = args[0]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))?
                as u32;
            let registry = VM_INSTANCE_REGISTRY.lock();
            Value::bool(registry.root_id == Some(handle))
        }

        runtime::VM_INSTANCE_IS_ALIVE => {
            // instance.isAlive(): check if instance is alive
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "VmInstance.isAlive requires 1 argument".to_string(),
                ));
            }
            let handle = args[0]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))?
                as u32;
            let registry = VM_INSTANCE_REGISTRY.lock();
            let is_alive = registry.instances
                .get(&handle)
                .map(|e| e.is_alive)
                .unwrap_or(false);
            Value::bool(is_alive)
        }

        runtime::VM_INSTANCE_IS_DESTROYED => {
            // instance.isDestroyed(): check if instance is terminated
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "VmInstance.isDestroyed requires 1 argument".to_string(),
                ));
            }
            let handle = args[0]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))?
                as u32;
            let registry = VM_INSTANCE_REGISTRY.lock();
            let is_destroyed = registry.instances
                .get(&handle)
                .map(|e| !e.is_alive)
                .unwrap_or(true);
            Value::bool(is_destroyed)
        }

        runtime::VM_INSTANCE_COMPILE => {
            // instance.compile(source): compile within child VM
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "VmInstance.compile requires 2 arguments".to_string(),
                ));
            }
            let handle = args[0]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))?
                as u32;
            let source = get_string(args[1])?;

            let registry = VM_INSTANCE_REGISTRY.lock();
            let is_root = registry.root_id == Some(handle);
            let has_debug = registry.instances.get(&handle)
                .map(|e| e.debug_state.is_some())
                .unwrap_or(false);
            drop(registry);

            // When debug mode is enabled, compile with sourcemap for line mapping
            let module = if has_debug {
                compile_source_debug(&source)?
            } else {
                compile_source(&source)?
            };

            if is_root {
                // Root delegates to global registry
                let mut global_reg = COMPILED_MODULE_REGISTRY.lock();
                let id = global_reg.register(module);
                Value::i32(id as i32)
            } else {
                let mut registry = VM_INSTANCE_REGISTRY.lock();
                let entry = registry.instances.get_mut(&handle)
                    .ok_or_else(|| VmError::RuntimeError(format!("VM instance not found: {}", handle)))?;
                if !entry.is_alive {
                    return Err(VmError::RuntimeError("VM instance is terminated".to_string()));
                }
                let id = entry.modules.register(module);
                Value::i32(id as i32)
            }
        }

        runtime::VM_INSTANCE_EXECUTE => {
            // instance.execute(moduleId): execute module in child VM
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "VmInstance.execute requires 2 arguments".to_string(),
                ));
            }
            let handle = args[0]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))?
                as u32;
            let module_id = args[1]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for moduleId".to_string()))?
                as u32;

            let registry = VM_INSTANCE_REGISTRY.lock();
            let is_root = registry.root_id == Some(handle);
            drop(registry);

            if is_root {
                // Root delegates to global registry + execute_module
                let global_reg = COMPILED_MODULE_REGISTRY.lock();
                let module = global_reg.get(module_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("Module not found: {}", module_id)))?
                    .clone();
                drop(global_reg);
                execute_module(&module)?
            } else {
                // Child: take Vm, execute, put back
                let (module, mut vm) = {
                    let mut registry = VM_INSTANCE_REGISTRY.lock();
                    let entry = registry.instances.get_mut(&handle)
                        .ok_or_else(|| VmError::RuntimeError(format!("VM instance not found: {}", handle)))?;
                    if !entry.is_alive {
                        return Err(VmError::RuntimeError("VM instance is terminated".to_string()));
                    }
                    let module = entry.modules.get(module_id)
                        .ok_or_else(|| VmError::RuntimeError(format!("Module not found in child VM: {}", module_id)))?
                        .clone();
                    let vm = entry.vm.take()
                        .ok_or_else(|| VmError::RuntimeError("VM instance is currently in use".to_string()))?;
                    (module, vm)
                };

                let result = vm.execute(&module);

                // Put Vm back
                let mut registry = VM_INSTANCE_REGISTRY.lock();
                if let Some(entry) = registry.instances.get_mut(&handle) {
                    entry.vm = Some(vm);
                }
                drop(registry);

                result.map_err(|e| VmError::RuntimeError(format!("Child VM error: {}", e)))?
            }
        }

        runtime::VM_INSTANCE_EVAL => {
            // instance.eval(source): compile + execute in child VM
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "VmInstance.eval requires 2 arguments".to_string(),
                ));
            }
            let handle = args[0]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))?
                as u32;
            let source = get_string(args[1])?;

            let registry = VM_INSTANCE_REGISTRY.lock();
            let is_root = registry.root_id == Some(handle);
            drop(registry);

            // Wrap source for eval (same as Compiler.eval)
            let wrapped = format!(
                "function __eval__(): number {{ {} }}\nreturn __eval__();",
                source
            );
            let module = compile_source(&wrapped)?;

            if is_root {
                execute_module(&module)?
            } else {
                // Take Vm, execute, put back
                let mut vm = {
                    let mut registry = VM_INSTANCE_REGISTRY.lock();
                    let entry = registry.instances.get_mut(&handle)
                        .ok_or_else(|| VmError::RuntimeError(format!("VM instance not found: {}", handle)))?;
                    if !entry.is_alive {
                        return Err(VmError::RuntimeError("VM instance is terminated".to_string()));
                    }
                    entry.vm.take()
                        .ok_or_else(|| VmError::RuntimeError("VM instance is currently in use".to_string()))?
                };

                let result = vm.execute(&module);

                // Put Vm back
                let mut registry = VM_INSTANCE_REGISTRY.lock();
                if let Some(entry) = registry.instances.get_mut(&handle) {
                    entry.vm = Some(vm);
                }
                drop(registry);

                result.map_err(|e| VmError::RuntimeError(format!("Child VM eval error: {}", e)))?
            }
        }

        runtime::VM_INSTANCE_LOAD_BYTECODE => {
            // instance.loadBytecode(bytes): decode bytecode buffer into child VM
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "VmInstance.loadBytecode requires 2 arguments".to_string(),
                ));
            }
            let handle = args[0]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))?
                as u32;
            let buf_val = args[1];
            if !buf_val.is_ptr() {
                return Err(VmError::TypeError("Expected Buffer for bytes".to_string()));
            }
            let buf_ptr = unsafe { buf_val.as_ptr::<Buffer>() }
                .ok_or_else(|| VmError::TypeError("Expected Buffer".to_string()))?;
            let buffer = unsafe { &*buf_ptr.as_ptr() };
            let bytes: Vec<u8> = (0..buffer.length())
                .filter_map(|i| buffer.get_byte(i))
                .collect();
            let module = Module::decode(&bytes).map_err(|e| {
                VmError::RuntimeError(format!("Bytecode decode error: {:?}", e))
            })?;

            let mut registry = VM_INSTANCE_REGISTRY.lock();
            let is_root = registry.root_id == Some(handle);

            if is_root {
                drop(registry);
                let mut global_reg = COMPILED_MODULE_REGISTRY.lock();
                let id = global_reg.register(module);
                Value::i32(id as i32)
            } else {
                let entry = registry.instances.get_mut(&handle)
                    .ok_or_else(|| VmError::RuntimeError(format!("VM instance not found: {}", handle)))?;
                if !entry.is_alive {
                    return Err(VmError::RuntimeError("VM instance is terminated".to_string()));
                }
                let id = entry.modules.register(module);
                Value::i32(id as i32)
            }
        }

        runtime::VM_INSTANCE_RUN_ENTRY => {
            // instance.runEntry(name): run a named function in a loaded module
            if args.len() < 2 {
                return Err(VmError::RuntimeError(
                    "VmInstance.runEntry requires 2 arguments".to_string(),
                ));
            }
            let handle = args[0]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))?
                as u32;
            let func_name = get_string(args[1])?;

            let registry = VM_INSTANCE_REGISTRY.lock();
            let is_root = registry.root_id == Some(handle);
            drop(registry);

            if is_root {
                // Root: look up in global registry and execute
                let global_reg = COMPILED_MODULE_REGISTRY.lock();
                // Find a module that contains the named function
                let mut found_module = None;
                for module in global_reg.modules.values() {
                    if module.functions.iter().any(|f| f.name == func_name) {
                        found_module = Some(module.clone());
                        break;
                    }
                }
                drop(global_reg);
                let module = found_module
                    .ok_or_else(|| VmError::RuntimeError(format!("Function '{}' not found", func_name)))?;
                execute_module(&module)?
            } else {
                // Child: search in child's modules
                let (module, mut vm) = {
                    let mut registry = VM_INSTANCE_REGISTRY.lock();
                    let entry = registry.instances.get_mut(&handle)
                        .ok_or_else(|| VmError::RuntimeError(format!("VM instance not found: {}", handle)))?;
                    if !entry.is_alive {
                        return Err(VmError::RuntimeError("VM instance is terminated".to_string()));
                    }
                    let mut found_module = None;
                    for module in entry.modules.modules.values() {
                        if module.functions.iter().any(|f| f.name == func_name) {
                            found_module = Some(module.clone());
                            break;
                        }
                    }
                    let module = found_module
                        .ok_or_else(|| VmError::RuntimeError(format!("Function '{}' not found in child VM", func_name)))?;
                    let vm = entry.vm.take()
                        .ok_or_else(|| VmError::RuntimeError("VM instance is currently in use".to_string()))?;
                    (module, vm)
                };

                let result = vm.execute(&module);

                // Put Vm back
                let mut registry = VM_INSTANCE_REGISTRY.lock();
                if let Some(entry) = registry.instances.get_mut(&handle) {
                    entry.vm = Some(vm);
                }
                drop(registry);

                result.map_err(|e| VmError::RuntimeError(format!("Child VM runEntry error: {}", e)))?
            }
        }

        runtime::VM_INSTANCE_TERMINATE => {
            // instance.terminate(): kill child and all descendants
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "VmInstance.terminate requires 1 argument".to_string(),
                ));
            }
            let handle = args[0]
                .as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))?
                as u32;

            let mut registry = VM_INSTANCE_REGISTRY.lock();
            if registry.root_id == Some(handle) {
                return Err(VmError::RuntimeError("Cannot terminate root VM".to_string()));
            }
            registry.terminate(handle);
            Value::null()
        }

        // ── VM Introspection & Resource Control (Phase 5) ──

        runtime::HEAP_USED => {
            // Vm.heapUsed(): number — current heap allocation in bytes
            let gc = ctx.gc.lock();
            let allocated = gc.heap_stats().allocated_bytes;
            drop(gc);
            Value::i32(allocated as i32)
        }

        runtime::HEAP_LIMIT => {
            // Vm.heapLimit(): number — max heap size (0 = unlimited)
            Value::i32(0)
        }

        runtime::TASK_COUNT => {
            // Vm.taskCount(): number — total tasks (placeholder)
            Value::i32(0)
        }

        runtime::CONCURRENCY => {
            // Vm.concurrency(): number — tasks actively running (placeholder)
            Value::i32(0)
        }

        runtime::THREAD_COUNT => {
            // Vm.threadCount(): number — available worker threads
            let threads = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1);
            Value::i32(threads as i32)
        }

        runtime::GC_COLLECT => {
            // Vm.gcCollect(): void — trigger manual garbage collection
            ctx.gc.lock().collect();
            Value::null()
        }

        runtime::GC_STATS => {
            // Vm.gcStats(): number — total bytes freed by GC
            let gc = ctx.gc.lock();
            let bytes_freed = gc.stats().bytes_freed;
            drop(gc);
            Value::i32(bytes_freed as i32)
        }

        runtime::VERSION => {
            // Vm.version(): string — Raya VM version
            allocate_string(ctx, "0.1.0".to_string())
        }

        runtime::UPTIME => {
            // Vm.uptime(): number — VM uptime in milliseconds
            let elapsed = VM_START_TIME.elapsed();
            Value::i32(elapsed.as_millis() as i32)
        }

        runtime::LOADED_MODULES => {
            // Vm.loadedModules(): string — comma-separated list of loaded module names
            let registry = COMPILED_MODULE_REGISTRY.lock();
            let names: Vec<String> = registry.names.keys().cloned().collect();
            drop(registry);
            allocate_string(ctx, names.join(","))
        }

        runtime::HAS_MODULE => {
            // Vm.hasModule(name: string): boolean — check if a module is loaded by name
            if args.is_empty() {
                return Err(VmError::RuntimeError(
                    "Vm.hasModule requires 1 argument".to_string(),
                ));
            }
            let name = get_string(args[0])?;
            let registry = COMPILED_MODULE_REGISTRY.lock();
            let has = registry.names.contains_key(&name);
            Value::bool(has)
        }

        // =================================================================
        // VmInstance Debug Control (0x3070-0x3081)
        // =================================================================

        runtime::VM_ENABLE_DEBUG => {
            // instance.enableDebug(): activate DebugState for child VM
            if args.is_empty() {
                return Err(VmError::RuntimeError("enableDebug requires handle".to_string()));
            }
            let handle = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))? as u32;

            let mut registry = VM_INSTANCE_REGISTRY.lock();
            let entry = registry.instances.get_mut(&handle)
                .ok_or_else(|| VmError::RuntimeError(format!("VM instance not found: {}", handle)))?;
            if !entry.is_alive {
                return Err(VmError::RuntimeError("VM instance is terminated".to_string()));
            }

            let ds = std::sync::Arc::new(crate::vm::interpreter::DebugState::new());

            // Attach to child VM's SharedVmState so interpreter can see it
            if let Some(ref vm) = entry.vm {
                *vm.shared_state().debug_state.lock() = Some(ds.clone());
            }
            entry.debug_state = Some(ds);
            Value::null()
        }

        runtime::VM_DEBUG_RUN => {
            // instance.debugRun(moduleId): run module in debug mode
            if args.len() < 2 {
                return Err(VmError::RuntimeError("debugRun requires 2 arguments".to_string()));
            }
            let handle = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))? as u32;
            let module_id = args[1].as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for moduleId".to_string()))? as u32;

            // Take Vm, module, and debug state from registry
            let (module, mut vm, ds) = {
                let mut registry = VM_INSTANCE_REGISTRY.lock();
                let entry = registry.instances.get_mut(&handle)
                    .ok_or_else(|| VmError::RuntimeError(format!("VM instance not found: {}", handle)))?;
                if !entry.is_alive {
                    return Err(VmError::RuntimeError("VM instance is terminated".to_string()));
                }
                let module = entry.modules.get(module_id)
                    .ok_or_else(|| VmError::RuntimeError(format!("Module not found: {}", module_id)))?
                    .clone();
                let vm = entry.vm.take()
                    .ok_or_else(|| VmError::RuntimeError("VM instance is currently in use".to_string()))?;
                let ds = entry.debug_state.clone()
                    .ok_or_else(|| VmError::RuntimeError("Debug not enabled — call enableDebug() first".to_string()))?;
                (module, vm, ds)
            };

            ds.active.store(true, std::sync::atomic::Ordering::Release);

            // Register module & spawn main task in child scheduler
            let module_arc = std::sync::Arc::new(module.clone());
            vm.shared_state().register_module(module_arc.clone())
                .map_err(|e| VmError::RuntimeError(e))?;

            let main_fn_id = module.functions.iter()
                .position(|f| f.name == "main")
                .ok_or_else(|| VmError::RuntimeError("No main function".to_string()))?;

            let main_task = std::sync::Arc::new(
                crate::vm::scheduler::Task::new(main_fn_id, module_arc, None)
            );
            vm.scheduler().spawn(main_task)
                .ok_or_else(|| VmError::RuntimeError("Failed to spawn main task".to_string()))?;

            // Block parent thread until child pauses or completes
            let phase = ds.wait_for_pause();

            // Park Vm back in registry (child worker is blocked on condvar)
            {
                let mut registry = VM_INSTANCE_REGISTRY.lock();
                if let Some(entry) = registry.instances.get_mut(&handle) {
                    entry.vm = Some(vm);
                }
            }

            debug_phase_to_value(ctx, phase)?
        }

        runtime::VM_DEBUG_CONTINUE => {
            // instance.debugContinue(): resume until next pause
            if args.is_empty() {
                return Err(VmError::RuntimeError("debugContinue requires handle".to_string()));
            }
            let handle = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))? as u32;

            let ds = get_debug_state(handle)?;
            ds.signal_resume(crate::vm::interpreter::debug_state::StepMode::None);
            let phase = ds.wait_for_pause();
            debug_phase_to_value(ctx, phase)?
        }

        runtime::VM_DEBUG_STEP_OVER => {
            // instance.debugStepOver(): step to next line at same or lower depth
            if args.is_empty() {
                return Err(VmError::RuntimeError("debugStepOver requires handle".to_string()));
            }
            let handle = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))? as u32;

            let ds = get_debug_state(handle)?;
            let depth = ds.pause_depth.load(std::sync::atomic::Ordering::Acquire) as usize;
            let line = ds.pause_line.load(std::sync::atomic::Ordering::Acquire);
            ds.signal_resume(crate::vm::interpreter::debug_state::StepMode::Over {
                target_depth: depth,
                start_line: line,
            });
            let phase = ds.wait_for_pause();
            debug_phase_to_value(ctx, phase)?
        }

        runtime::VM_DEBUG_STEP_INTO => {
            // instance.debugStepInto(): step to next line at any depth
            if args.is_empty() {
                return Err(VmError::RuntimeError("debugStepInto requires handle".to_string()));
            }
            let handle = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))? as u32;

            let ds = get_debug_state(handle)?;
            let line = ds.pause_line.load(std::sync::atomic::Ordering::Acquire);
            ds.signal_resume(crate::vm::interpreter::debug_state::StepMode::Into {
                start_line: line,
            });
            let phase = ds.wait_for_pause();
            debug_phase_to_value(ctx, phase)?
        }

        runtime::VM_DEBUG_STEP_OUT => {
            // instance.debugStepOut(): run until current function returns
            if args.is_empty() {
                return Err(VmError::RuntimeError("debugStepOut requires handle".to_string()));
            }
            let handle = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))? as u32;

            let ds = get_debug_state(handle)?;
            let depth = ds.pause_depth.load(std::sync::atomic::Ordering::Acquire) as usize;
            ds.signal_resume(crate::vm::interpreter::debug_state::StepMode::Out {
                target_depth: depth,
            });
            let phase = ds.wait_for_pause();
            debug_phase_to_value(ctx, phase)?
        }

        runtime::VM_SET_BREAKPOINT => {
            // instance.setBreakpoint(file, line): returns bp_id
            if args.len() < 3 {
                return Err(VmError::RuntimeError("setBreakpoint requires 3 arguments".to_string()));
            }
            let handle = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))? as u32;
            let file = get_string(args[1])?;
            let line = args[2].as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for line".to_string()))? as u32;

            let ds = get_debug_state(handle)?;

            // Get module from child's registry for line table lookup
            let module = get_child_module(handle)?;
            let (func_id, offset) = ds.resolve_breakpoint(&module, &file, line)
                .map_err(|e| VmError::RuntimeError(e))?;
            let bp_id = ds.add_breakpoint(func_id, offset, file, line);
            Value::i32(bp_id as i32)
        }

        runtime::VM_REMOVE_BREAKPOINT => {
            // instance.removeBreakpoint(bpId)
            if args.len() < 2 {
                return Err(VmError::RuntimeError("removeBreakpoint requires 2 arguments".to_string()));
            }
            let handle = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))? as u32;
            let bp_id = args[1].as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for bpId".to_string()))? as u32;

            let ds = get_debug_state(handle)?;
            ds.remove_breakpoint(bp_id);
            Value::null()
        }

        runtime::VM_LIST_BREAKPOINTS => {
            // instance.listBreakpoints(): JSON string
            if args.is_empty() {
                return Err(VmError::RuntimeError("listBreakpoints requires handle".to_string()));
            }
            let handle = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))? as u32;

            let ds = get_debug_state(handle)?;
            let registry = ds.bp_registry.read();
            let mut entries: Vec<String> = Vec::new();
            for entry in registry.values() {
                entries.push(format!(
                    r#"{{"id":{},"file":"{}","line":{},"enabled":{},"hitCount":{}}}"#,
                    entry.id, entry.file.replace('"', "\\\""), entry.line,
                    entry.enabled, entry.hit_count,
                ));
            }
            let json = format!("[{}]", entries.join(","));
            allocate_string(ctx, json)
        }

        runtime::VM_DEBUG_STACK_TRACE => {
            // instance.debugStackTrace(): JSON array of stack frames
            if args.is_empty() {
                return Err(VmError::RuntimeError("debugStackTrace requires handle".to_string()));
            }
            let handle = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))? as u32;

            let ds = get_debug_state(handle)?;
            let pause_info = ds.pause_info.lock().unwrap();
            if let Some(info) = pause_info.as_ref() {
                // Build single-frame trace from pause info
                // TODO: extend with full call frame stack when child task stack is accessible
                let json = format!(
                    r#"[{{"functionName":"{}","file":"{}","line":{},"column":{},"frameIndex":0}}]"#,
                    info.function_name.replace('"', "\\\""),
                    info.source_file.replace('"', "\\\""),
                    info.line,
                    info.column,
                );
                allocate_string(ctx, json)
            } else {
                allocate_string(ctx, "[]".to_string())
            }
        }

        runtime::VM_DEBUG_GET_LOCALS => {
            // instance.debugGetLocals(frameIndex): JSON array of locals
            if args.len() < 2 {
                return Err(VmError::RuntimeError("debugGetLocals requires 2 arguments".to_string()));
            }
            let handle = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))? as u32;
            let _frame_index = args[1].as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for frameIndex".to_string()))?;

            // Ensure debugger is attached and paused
            let _ds = get_debug_state(handle)?;

            // TODO: Read locals from paused task's stack once we expose task reference.
            // For now, return empty array as a placeholder.
            allocate_string(ctx, "[]".to_string())
        }

        runtime::VM_DEBUG_EVALUATE => {
            // instance.debugEvaluate(expression): eval in paused context
            if args.len() < 2 {
                return Err(VmError::RuntimeError("debugEvaluate requires 2 arguments".to_string()));
            }
            let handle = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))? as u32;
            let _expr = get_string(args[1])?;

            let _ds = get_debug_state(handle)?;

            // TODO: Compile expression in child context, inject temporary task
            allocate_string(ctx, "\"(evaluate not yet implemented)\"".to_string())
        }

        runtime::VM_DEBUG_LOCATION => {
            // instance.debugLocation(): JSON with current pause location
            if args.is_empty() {
                return Err(VmError::RuntimeError("debugLocation requires handle".to_string()));
            }
            let handle = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))? as u32;

            let ds = get_debug_state(handle)?;
            let pause_info = ds.pause_info.lock().unwrap();
            if let Some(info) = pause_info.as_ref() {
                let reason_str = match &info.reason {
                    crate::vm::interpreter::debug_state::PauseReason::Breakpoint(id) => format!("breakpoint({})", id),
                    crate::vm::interpreter::debug_state::PauseReason::Step => "step".to_string(),
                    crate::vm::interpreter::debug_state::PauseReason::DebuggerStatement => "debugger".to_string(),
                    crate::vm::interpreter::debug_state::PauseReason::Entry => "entry".to_string(),
                };
                let json = format!(
                    r#"{{"file":"{}","line":{},"column":{},"functionName":"{}","reason":"{}"}}"#,
                    info.source_file.replace('"', "\\\""),
                    info.line,
                    info.column,
                    info.function_name.replace('"', "\\\""),
                    reason_str,
                );
                allocate_string(ctx, json)
            } else {
                allocate_string(ctx, r#"{"error":"not paused"}"#.to_string())
            }
        }

        runtime::VM_DEBUG_GET_SOURCE => {
            // instance.debugGetSource(file, startLine, endLine): source text
            if args.len() < 4 {
                return Err(VmError::RuntimeError("debugGetSource requires 4 arguments".to_string()));
            }
            let handle = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))? as u32;
            let file = get_string(args[1])?;
            let start_line = args[2].as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for startLine".to_string()))? as usize;
            let end_line = args[3].as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for endLine".to_string()))? as usize;

            let ds = get_debug_state(handle)?;

            // Try source cache first, then fall back to filesystem
            let source = {
                let cache = ds.source_cache.read();
                cache.get(&file).cloned()
            }.or_else(|| std::fs::read_to_string(&file).ok());

            if let Some(source) = source {
                let lines: Vec<&str> = source.lines().collect();
                let start = start_line.saturating_sub(1).min(lines.len());
                let end = end_line.min(lines.len());
                let selected: Vec<&str> = lines[start..end].to_vec();
                allocate_string(ctx, selected.join("\n"))
            } else {
                allocate_string(ctx, format!("(source file '{}' not found)", file))
            }
        }

        runtime::VM_DEBUG_IS_PAUSED => {
            // instance.debugIsPaused(): boolean
            if args.is_empty() {
                return Err(VmError::RuntimeError("debugIsPaused requires handle".to_string()));
            }
            let handle = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))? as u32;

            let ds = get_debug_state(handle)?;
            let phase = ds.phase_lock.lock().unwrap();
            let paused = matches!(*phase, crate::vm::interpreter::debug_state::DebugPhase::Paused);
            Value::bool(paused)
        }

        runtime::VM_DEBUG_GET_VARIABLES => {
            // instance.debugGetVariables(frameIndex): same as getLocals for now
            if args.len() < 2 {
                return Err(VmError::RuntimeError("debugGetVariables requires 2 arguments".to_string()));
            }
            let _handle = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))? as u32;

            // TODO: Read from paused task's stack
            allocate_string(ctx, "[]".to_string())
        }

        runtime::VM_SET_BP_CONDITION => {
            // instance.setBreakpointCondition(bpId, condition): set condition
            if args.len() < 3 {
                return Err(VmError::RuntimeError("setBreakpointCondition requires 3 arguments".to_string()));
            }
            let handle = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))? as u32;
            let bp_id = args[1].as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for bpId".to_string()))? as u32;
            let condition = get_string(args[2])?;

            let ds = get_debug_state(handle)?;
            let mut registry = ds.bp_registry.write();
            if let Some(entry) = registry.get_mut(&bp_id) {
                entry.condition = Some(condition);
            }
            Value::null()
        }

        runtime::VM_DEBUG_BREAK_AT_ENTRY => {
            // instance.debugBreakAtEntry(moduleId): break at first instruction
            if args.len() < 2 {
                return Err(VmError::RuntimeError("debugBreakAtEntry requires 2 arguments".to_string()));
            }
            let handle = args[0].as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for handle".to_string()))? as u32;
            let _module_id = args[1].as_i32()
                .ok_or_else(|| VmError::TypeError("Expected number for moduleId".to_string()))?;

            let ds = get_debug_state(handle)?;
            ds.break_at_entry.store(true, std::sync::atomic::Ordering::Release);
            Value::null()
        }

        _ => {
            return Err(VmError::RuntimeError(format!(
                "Unknown runtime method: {:#06x}",
                method_id
            )));
        }
    };

    stack.push(result)?;
    Ok(())
}

// ============================================================================
// Internal helpers
// ============================================================================

/// Get the DebugState for a VM instance handle.
fn get_debug_state(handle: u32) -> Result<std::sync::Arc<crate::vm::interpreter::DebugState>, VmError> {
    let registry = VM_INSTANCE_REGISTRY.lock();
    let entry = registry.instances.get(&handle)
        .ok_or_else(|| VmError::RuntimeError(format!("VM instance not found: {}", handle)))?;
    entry.debug_state.clone()
        .ok_or_else(|| VmError::RuntimeError("Debug not enabled — call enableDebug() first".to_string()))
}

/// Get the first compiled module from a child VM instance.
fn get_child_module(handle: u32) -> Result<Module, VmError> {
    let registry = VM_INSTANCE_REGISTRY.lock();
    let entry = registry.instances.get(&handle)
        .ok_or_else(|| VmError::RuntimeError(format!("VM instance not found: {}", handle)))?;
    // Return the most recently compiled module (highest ID)
    entry.modules.modules.values().next().cloned()
        .ok_or_else(|| VmError::RuntimeError("No modules compiled in child VM".to_string()))
}

/// Convert a DebugPhaseSnapshot to a Raya string value ("paused", "completed", "error").
fn debug_phase_to_value(
    ctx: &RuntimeHandlerContext,
    phase: crate::vm::interpreter::debug_state::DebugPhaseSnapshot,
) -> Result<Value, VmError> {
    match phase {
        crate::vm::interpreter::debug_state::DebugPhaseSnapshot::Paused => {
            Ok(allocate_string(ctx, "paused".to_string()))
        }
        crate::vm::interpreter::debug_state::DebugPhaseSnapshot::Completed(_) => {
            Ok(allocate_string(ctx, "completed".to_string()))
        }
        crate::vm::interpreter::debug_state::DebugPhaseSnapshot::Failed(msg) => {
            Ok(allocate_string(ctx, format!("error:{}", msg)))
        }
    }
}

/// Allocate a string on the GC heap and return as Value
fn allocate_string(ctx: &RuntimeHandlerContext, s: String) -> Value {
    let raya_str = RayaString::new(s);
    let gc_ptr = ctx.gc.lock().allocate(raya_str);
    unsafe { Value::from_ptr(std::ptr::NonNull::new(gc_ptr.as_ptr()).unwrap()) }
}

/// Parse Raya source code to an AST
fn parse_source(source: &str) -> Result<(ast::Module, Interner), VmError> {
    let parser = Parser::new(source).map_err(|e| {
        VmError::RuntimeError(format!("Lexer error: {:?}", e))
    })?;
    parser.parse().map_err(|e| {
        VmError::RuntimeError(format!("Parse error: {:?}", e))
    })
}

/// Type-check a parsed AST, returning a TypedAstEntry
fn typecheck_ast(ast: ast::Module, interner: Interner) -> Result<TypedAstEntry, VmError> {
    let mut type_ctx = TypeContext::new();
    let mut binder = Binder::new(&mut type_ctx, &interner);

    let builtin_sigs = crate::builtins::to_checker_signatures();
    binder.register_builtins(&builtin_sigs);

    let mut symbols = binder.bind_module(&ast).map_err(|e| {
        VmError::RuntimeError(format!("Binding error: {:?}", e))
    })?;

    let checker = TypeChecker::new(&mut type_ctx, &symbols, &interner);
    let check_result = checker.check_module(&ast).map_err(|e| {
        VmError::RuntimeError(format!("Type check error: {:?}", e))
    })?;

    for ((scope_id, name), ty) in check_result.inferred_types {
        symbols.update_type(ScopeId(scope_id), &name, ty);
    }

    Ok(TypedAstEntry {
        ast,
        interner,
        type_ctx,
        symbols,
        expr_types: check_result.expr_types,
    })
}

/// Compile Raya source code to a Module (full pipeline)
fn compile_source(source: &str) -> Result<Module, VmError> {
    compile_source_impl(source, false)
}

fn compile_source_debug(source: &str) -> Result<Module, VmError> {
    compile_source_impl(source, true)
}

fn compile_source_impl(source: &str, sourcemap: bool) -> Result<Module, VmError> {
    let (ast, interner) = parse_source(source)?;
    let typed = typecheck_ast(ast, interner)?;

    let TypedAstEntry { ast, interner, type_ctx, symbols: _, expr_types } = typed;
    let compiler = Compiler::new(type_ctx, &interner)
        .with_expr_types(expr_types)
        .with_sourcemap(sourcemap);
    compiler.compile_via_ir(&ast).map_err(|e| {
        VmError::RuntimeError(format!("Compile error: {}", e))
    })
}

/// Execute a compiled module and return the result
fn execute_module(module: &Module) -> Result<Value, VmError> {
    let mut vm = Vm::with_worker_count(1);
    vm.execute(module)
}

/// Get dependency search paths for a given name
fn get_dependency_search_paths(name: &str) -> Vec<String> {
    let ryb_name = format!("{}.ryb", name);
    let mut paths = vec![
        format!("./deps/{}", ryb_name),
        format!("./lib/{}", ryb_name),
    ];

    // Add entry_dir/deps/ if we can determine the entry directory
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(format!("{}/deps/{}", cwd.display(), ryb_name));
    }

    // Add ~/.raya/libs/
    if let Some(home) = dirs_home() {
        paths.push(format!("{}/.raya/libs/{}", home, ryb_name));
    }

    paths
}

/// Get the user's home directory
fn dirs_home() -> Option<String> {
    std::env::var("HOME").ok().or_else(|| std::env::var("USERPROFILE").ok())
}
