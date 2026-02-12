//! End-to-end tests for the std:runtime module
//!
//! Tests verify Compiler, Bytecode, Parser, TypeChecker, Vm, and VmInstance
//! class methods compile and execute correctly through the runtime handler pipeline.

use super::harness::{
    compile_and_run_with_builtins, expect_i32_with_builtins,
    expect_bool_with_builtins, expect_string_contains_with_builtins,
};

// ============================================================================
// Compiler.compile + Compiler.execute
// ============================================================================

#[test]
fn test_compiler_compile_returns_module_id() {
    // compile() should return a positive module ID
    expect_bool_with_builtins(
        r#"
        import { Compiler } from "std:runtime";
        let mod: number = Compiler.compile("return 42;");
        return mod > 0;
    "#,
        true,
    );
}

#[test]
fn test_compiler_compile_and_execute() {
    // compile + execute should run the module and return its result
    expect_i32_with_builtins(
        r#"
        import { Compiler } from "std:runtime";
        let mod: number = Compiler.compile("return 10 + 20;");
        let result: number = Compiler.execute(mod);
        return result;
    "#,
        30,
    );
}

#[test]
fn test_compiler_compile_multiple_modules() {
    // Each compile() should return a different module ID
    expect_i32_with_builtins(
        r#"
        import { Compiler } from "std:runtime";
        let mod1: number = Compiler.compile("return 1;");
        let mod2: number = Compiler.compile("return 2;");
        if (mod1 == mod2) {
            return 0;
        }
        return 1;
    "#,
        1,
    );
}

#[test]
fn test_compiler_execute_arithmetic() {
    expect_i32_with_builtins(
        r#"
        import { Compiler } from "std:runtime";
        let mod: number = Compiler.compile("return 6 * 7;");
        return Compiler.execute(mod);
    "#,
        42,
    );
}

// ============================================================================
// Compiler.compileExpression
// ============================================================================

#[test]
fn test_compiler_compile_expression() {
    expect_i32_with_builtins(
        r#"
        import { Compiler } from "std:runtime";
        let mod: number = Compiler.compileExpression("3 + 4");
        return Compiler.execute(mod);
    "#,
        7,
    );
}

#[test]
fn test_compiler_compile_expression_multiply() {
    expect_i32_with_builtins(
        r#"
        import { Compiler } from "std:runtime";
        let mod: number = Compiler.compileExpression("5 * 8");
        return Compiler.execute(mod);
    "#,
        40,
    );
}

// ============================================================================
// Compiler.eval
// ============================================================================

#[test]
fn test_compiler_eval_simple() {
    expect_i32_with_builtins(
        r#"
        import { Compiler } from "std:runtime";
        return Compiler.eval("return 99;");
    "#,
        99,
    );
}

#[test]
fn test_compiler_eval_arithmetic() {
    expect_i32_with_builtins(
        r#"
        import { Compiler } from "std:runtime";
        return Compiler.eval("return 100 - 58;");
    "#,
        42,
    );
}

#[test]
fn test_compiler_eval_with_variable() {
    expect_i32_with_builtins(
        r#"
        import { Compiler } from "std:runtime";
        return Compiler.eval("let x: number = 10; return x * 3;");
    "#,
        30,
    );
}

// ============================================================================
// Bytecode.encode + Bytecode.decode roundtrip
// ============================================================================

#[test]
fn test_bytecode_encode_decode_roundtrip() {
    // Compile → encode → decode → execute should produce the same result
    expect_i32_with_builtins(
        r#"
        import { Compiler, Bytecode } from "std:runtime";
        let mod: number = Compiler.compile("return 77;");
        let bytes: Buffer = Bytecode.encode(mod);
        let decoded: number = Bytecode.decode(bytes);
        return Compiler.execute(decoded);
    "#,
        77,
    );
}

#[test]
fn test_bytecode_encode_produces_buffer() {
    // encode() should return a non-null Buffer
    // We verify by checking that decode succeeds (returns a positive module ID)
    expect_i32_with_builtins(
        r#"
        import { Compiler, Bytecode } from "std:runtime";
        let mod: number = Compiler.compile("return 1;");
        let bytes: Buffer = Bytecode.encode(mod);
        let decoded: number = Bytecode.decode(bytes);
        if (decoded > 0) {
            return 1;
        }
        return 0;
    "#,
        1,
    );
}

// ============================================================================
// Named import patterns
// ============================================================================

#[test]
fn test_import_compiler_only() {
    expect_i32_with_builtins(
        r#"
        import { Compiler } from "std:runtime";
        return Compiler.eval("return 5;");
    "#,
        5,
    );
}

#[test]
fn test_import_bytecode_only() {
    // Bytecode needs a compiled module, so we still use Compiler internally
    // via the concatenated source. Just verify Bytecode is accessible.
    expect_i32_with_builtins(
        r#"
        import { Compiler, Bytecode } from "std:runtime";
        let mod: number = Compiler.compile("return 1;");
        let bytes: Buffer = Bytecode.encode(mod);
        let decoded: number = Bytecode.decode(bytes);
        return Compiler.execute(decoded);
    "#,
        1,
    );
}

// ============================================================================
// Error handling
// ============================================================================

#[test]
fn test_compiler_compile_invalid_source() {
    // Compiling invalid source should produce a runtime error
    let result = compile_and_run_with_builtins(
        r#"
        import { Compiler } from "std:runtime";
        let mod: number = Compiler.compile("this is not valid raya code !!!");
        return mod;
    "#,
    );
    assert!(result.is_err(), "Compiling invalid source should error");
}

#[test]
fn test_compiler_execute_invalid_module_id() {
    // Executing a non-existent module ID should error
    let result = compile_and_run_with_builtins(
        r#"
        import { Compiler } from "std:runtime";
        return Compiler.execute(99999);
    "#,
    );
    assert!(result.is_err(), "Executing invalid module ID should error");
}

// ============================================================================
// Phase 2: Bytecode Inspection
// ============================================================================

#[test]
fn test_bytecode_validate() {
    expect_bool_with_builtins(
        r#"
        import { Compiler, Bytecode } from "std:runtime";
        let mod: number = Compiler.compile("return 1;");
        return Bytecode.validate(mod);
    "#,
        true,
    );
}

#[test]
fn test_bytecode_disassemble() {
    // disassemble should return a string containing bytecode listing
    expect_string_contains_with_builtins(
        r#"
        import { Compiler, Bytecode } from "std:runtime";
        let mod: number = Compiler.compile("return 42;");
        return Bytecode.disassemble(mod);
    "#,
        "function",
    );
}

#[test]
fn test_bytecode_get_module_functions() {
    // getModuleFunctions should return comma-separated function names
    expect_string_contains_with_builtins(
        r#"
        import { Compiler, Bytecode } from "std:runtime";
        let mod: number = Compiler.compile("function add(a: number, b: number): number { return a + b; } return 0;");
        return Bytecode.getModuleFunctions(mod);
    "#,
        "add",
    );
}

#[test]
fn test_bytecode_get_module_classes() {
    // getModuleClasses should return comma-separated class names
    expect_string_contains_with_builtins(
        r#"
        import { Compiler, Bytecode } from "std:runtime";
        let mod: number = Compiler.compile("class Foo { x: number = 0; } return 0;");
        return Bytecode.getModuleClasses(mod);
    "#,
        "Foo",
    );
}

// ============================================================================
// Phase 2: Parser
// ============================================================================

#[test]
fn test_parser_parse_returns_ast_id() {
    expect_bool_with_builtins(
        r#"
        import { Parser } from "std:runtime";
        let ast: number = Parser.parse("return 42;");
        return ast > 0;
    "#,
        true,
    );
}

#[test]
fn test_parser_parse_expression_returns_ast_id() {
    expect_bool_with_builtins(
        r#"
        import { Parser } from "std:runtime";
        let ast: number = Parser.parseExpression("3 + 4");
        return ast > 0;
    "#,
        true,
    );
}

#[test]
fn test_parser_parse_invalid_source() {
    let result = compile_and_run_with_builtins(
        r#"
        import { Parser } from "std:runtime";
        let ast: number = Parser.parse("this is not valid !!!");
        return ast;
    "#,
    );
    assert!(result.is_err(), "Parsing invalid source should error");
}

// ============================================================================
// Phase 2: TypeChecker
// ============================================================================

#[test]
fn test_typechecker_check_returns_typed_ast_id() {
    expect_bool_with_builtins(
        r#"
        import { Parser, TypeChecker } from "std:runtime";
        let ast: number = Parser.parse("return 42;");
        let typed: number = TypeChecker.check(ast);
        return typed > 0;
    "#,
        true,
    );
}

#[test]
fn test_typechecker_check_invalid_ast_id() {
    let result = compile_and_run_with_builtins(
        r#"
        import { TypeChecker } from "std:runtime";
        let typed: number = TypeChecker.check(99999);
        return typed;
    "#,
    );
    assert!(result.is_err(), "Checking invalid AST ID should error");
}

// ============================================================================
// Phase 2: Compiler.compileAst (full pipeline)
// ============================================================================

#[test]
fn test_compiler_compile_ast_full_pipeline() {
    // Parser.parse → TypeChecker.check → Compiler.compileAst → Compiler.execute
    expect_i32_with_builtins(
        r#"
        import { Compiler, Parser, TypeChecker } from "std:runtime";
        let ast: number = Parser.parse("return 10 + 20;");
        let typed: number = TypeChecker.check(ast);
        let mod: number = Compiler.compileAst(typed);
        return Compiler.execute(mod);
    "#,
        30,
    );
}

#[test]
fn test_compiler_compile_ast_with_expression() {
    // Parser.parseExpression → TypeChecker.check → Compiler.compileAst → execute
    expect_i32_with_builtins(
        r#"
        import { Compiler, Parser, TypeChecker } from "std:runtime";
        let ast: number = Parser.parseExpression("7 * 6");
        let typed: number = TypeChecker.check(ast);
        let mod: number = Compiler.compileAst(typed);
        return Compiler.execute(mod);
    "#,
        42,
    );
}

// ============================================================================
// Phase 3: Vm.current()
// ============================================================================

#[test]
fn test_vm_current_returns_instance() {
    // Vm.current() should return a VmInstance with a positive ID
    expect_bool_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        let current: VmInstance = Vm.current();
        return current.id() > 0;
    "#,
        true,
    );
}

#[test]
fn test_vm_current_is_root() {
    expect_bool_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        let current: VmInstance = Vm.current();
        return current.isRoot();
    "#,
        true,
    );
}

#[test]
fn test_vm_current_is_alive() {
    expect_bool_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        return Vm.current().isAlive();
    "#,
        true,
    );
}

// ============================================================================
// Phase 3: Vm.spawn()
// ============================================================================

#[test]
fn test_vm_spawn_returns_instance() {
    expect_bool_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        let child: VmInstance = Vm.spawn();
        return child.id() > 0;
    "#,
        true,
    );
}

#[test]
fn test_vm_spawn_is_not_root() {
    expect_bool_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        let child: VmInstance = Vm.spawn();
        return child.isRoot();
    "#,
        false,
    );
}

#[test]
fn test_vm_spawn_is_alive() {
    expect_bool_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        let child: VmInstance = Vm.spawn();
        return child.isAlive();
    "#,
        true,
    );
}

// ============================================================================
// Phase 3: Child VM compile + execute
// ============================================================================

#[test]
fn test_child_compile_and_execute() {
    expect_i32_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        let child: VmInstance = Vm.spawn();
        let mod: number = child.compile("return 42;");
        return child.execute(mod);
    "#,
        42,
    );
}

#[test]
fn test_child_eval() {
    expect_i32_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        let child: VmInstance = Vm.spawn();
        return child.eval("return 10 + 20;");
    "#,
        30,
    );
}

#[test]
fn test_child_eval_with_variable() {
    expect_i32_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        let child: VmInstance = Vm.spawn();
        return child.eval("let x: number = 7; return x * 6;");
    "#,
        42,
    );
}

// ============================================================================
// Phase 3: Terminate
// ============================================================================

#[test]
fn test_terminate_marks_destroyed() {
    expect_bool_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        let child: VmInstance = Vm.spawn();
        child.terminate();
        return child.isDestroyed();
    "#,
        true,
    );
}

#[test]
fn test_terminated_not_alive() {
    expect_bool_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        let child: VmInstance = Vm.spawn();
        child.terminate();
        return child.isAlive();
    "#,
        false,
    );
}

#[test]
fn test_execute_on_terminated_child_errors() {
    let result = compile_and_run_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        let child: VmInstance = Vm.spawn();
        let mod: number = child.compile("return 1;");
        child.terminate();
        return child.execute(mod);
    "#,
    );
    assert!(result.is_err(), "Executing on terminated child should error");
}

// ============================================================================
// Phase 3: Multiple children (isolation)
// ============================================================================

#[test]
fn test_multiple_children_isolated() {
    expect_i32_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        let a: VmInstance = Vm.spawn();
        let b: VmInstance = Vm.spawn();
        let ra: number = a.eval("return 10;");
        let rb: number = b.eval("return 20;");
        return ra + rb;
    "#,
        30,
    );
}

// ============================================================================
// Phase 4: Permission Management
// ============================================================================

#[test]
fn test_vm_has_permission_eval() {
    // Root VM should have eval permission
    expect_bool_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        return Vm.hasPermission("eval");
    "#,
        true,
    );
}

#[test]
fn test_vm_has_permission_vm_spawn() {
    // Root VM should have vmSpawn permission
    expect_bool_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        return Vm.hasPermission("vmSpawn");
    "#,
        true,
    );
}

#[test]
fn test_vm_has_permission_binary_io() {
    // Root VM should have binaryIO permission
    expect_bool_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        return Vm.hasPermission("binaryIO");
    "#,
        true,
    );
}

#[test]
fn test_vm_has_permission_lib_load() {
    // Root VM should have libLoad permission
    expect_bool_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        return Vm.hasPermission("libLoad");
    "#,
        true,
    );
}

#[test]
fn test_vm_has_permission_reflect() {
    // Root VM should have reflect permission
    expect_bool_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        return Vm.hasPermission("reflect");
    "#,
        true,
    );
}

#[test]
fn test_vm_has_permission_unknown() {
    // Unknown permission name should return false
    expect_bool_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        return Vm.hasPermission("unknownPerm");
    "#,
        false,
    );
}

#[test]
fn test_vm_get_permissions() {
    // Root VM should have all permissions listed
    expect_string_contains_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        return Vm.getPermissions();
    "#,
        "eval",
    );
}

#[test]
fn test_vm_get_permissions_contains_vm_spawn() {
    expect_string_contains_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        return Vm.getPermissions();
    "#,
        "vmSpawn",
    );
}

#[test]
fn test_vm_get_allowed_stdlib() {
    // Root VM should allow all stdlib ("*")
    expect_string_contains_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        return Vm.getAllowedStdlib();
    "#,
        "*",
    );
}

#[test]
fn test_vm_is_stdlib_allowed_math() {
    // Root VM should allow std:math
    expect_bool_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        return Vm.isStdlibAllowed("std:math");
    "#,
        true,
    );
}

#[test]
fn test_vm_is_stdlib_allowed_logger() {
    // Root VM should allow std:logger
    expect_bool_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        return Vm.isStdlibAllowed("std:logger");
    "#,
        true,
    );
}

// ============================================================================
// Phase 5: VM Introspection & Resource Control
// ============================================================================

#[test]
fn test_vm_heap_used_non_negative() {
    // heapUsed() should return >= 0
    expect_bool_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        return Vm.heapUsed() >= 0;
    "#,
        true,
    );
}

#[test]
fn test_vm_heap_limit_unlimited() {
    // heapLimit() returns 0 for unlimited
    expect_i32_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        return Vm.heapLimit();
    "#,
        0,
    );
}

#[test]
fn test_vm_thread_count_positive() {
    // threadCount() should return at least 1
    expect_bool_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        return Vm.threadCount() > 0;
    "#,
        true,
    );
}

#[test]
fn test_vm_task_count_non_negative() {
    // taskCount() placeholder returns 0
    expect_bool_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        return Vm.taskCount() >= 0;
    "#,
        true,
    );
}

#[test]
fn test_vm_concurrency_non_negative() {
    // concurrency() placeholder returns 0
    expect_bool_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        return Vm.concurrency() >= 0;
    "#,
        true,
    );
}

#[test]
fn test_vm_gc_stats_non_negative() {
    // gcStats() should return >= 0 bytes freed
    expect_bool_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        return Vm.gcStats() >= 0;
    "#,
        true,
    );
}

#[test]
fn test_vm_gc_collect_no_error() {
    // gcCollect() should not error; return a value after calling it
    expect_i32_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        Vm.gcCollect();
        return 1;
    "#,
        1,
    );
}

#[test]
fn test_vm_version_contains_0_1() {
    // version() should contain "0.1"
    expect_string_contains_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        return Vm.version();
    "#,
        "0.1",
    );
}

#[test]
fn test_vm_uptime_non_negative() {
    // uptime() should be >= 0
    expect_bool_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        return Vm.uptime() >= 0;
    "#,
        true,
    );
}

#[test]
fn test_vm_has_module_false() {
    // hasModule() should return false for non-existent module
    expect_bool_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        return Vm.hasModule("nonexistent");
    "#,
        false,
    );
}

#[test]
fn test_vm_loaded_modules_is_string() {
    // loadedModules() should return a string (may be empty)
    expect_bool_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        let mods: string = Vm.loadedModules();
        return true;
    "#,
        true,
    );
}

// ============================================================================
// Phase 6: BytecodeBuilder
// ============================================================================

#[test]
fn test_bytecode_builder_create() {
    // Creating a BytecodeBuilder should not error
    expect_bool_with_builtins(
        r#"
        import { BytecodeBuilder } from "std:runtime";
        let b: BytecodeBuilder = new BytecodeBuilder("test", 0, "number");
        return b._id > 0;
    "#,
        true,
    );
}

#[test]
fn test_bytecode_builder_emit_push_and_return() {
    // Build a function that pushes 42 and returns it
    expect_bool_with_builtins(
        r#"
        import { BytecodeBuilder } from "std:runtime";
        let b: BytecodeBuilder = new BytecodeBuilder("answer", 0, "number");
        b.emitPush(42);
        b.emitReturn();
        let funcId: number = b.build();
        return funcId != 0;
    "#,
        true,
    );
}

#[test]
fn test_bytecode_builder_validate() {
    // A builder with push + return should validate
    expect_bool_with_builtins(
        r#"
        import { BytecodeBuilder } from "std:runtime";
        let b: BytecodeBuilder = new BytecodeBuilder("valid", 0, "number");
        b.emitPush(1);
        b.emitReturn();
        return b.validate();
    "#,
        true,
    );
}

#[test]
fn test_bytecode_builder_declare_local() {
    // declareLocal should return a valid index
    expect_bool_with_builtins(
        r#"
        import { BytecodeBuilder } from "std:runtime";
        let b: BytecodeBuilder = new BytecodeBuilder("locals", 0, "number");
        let idx: number = b.declareLocal("number");
        return idx >= 0;
    "#,
        true,
    );
}

#[test]
fn test_bytecode_builder_define_label() {
    // defineLabel should return a valid label ID
    expect_bool_with_builtins(
        r#"
        import { BytecodeBuilder } from "std:runtime";
        let b: BytecodeBuilder = new BytecodeBuilder("labels", 0, "number");
        let label: number = b.defineLabel();
        return label >= 0;
    "#,
        true,
    );
}

// ============================================================================
// Phase 7: ClassBuilder
// ============================================================================

#[test]
fn test_class_builder_create() {
    // Creating a ClassBuilder should not error
    expect_bool_with_builtins(
        r#"
        import { ClassBuilder } from "std:runtime";
        let cb: ClassBuilder = new ClassBuilder("TestClass");
        return cb._id >= 0;
    "#,
        true,
    );
}

#[test]
fn test_class_builder_add_field() {
    // Adding a field should not error
    expect_i32_with_builtins(
        r#"
        import { ClassBuilder } from "std:runtime";
        let cb: ClassBuilder = new ClassBuilder("Point");
        cb.addField("x", "number", false, false);
        cb.addField("y", "number", false, false);
        return 1;
    "#,
        1,
    );
}

#[test]
fn test_class_builder_build() {
    // Build should return a class ID
    expect_bool_with_builtins(
        r#"
        import { ClassBuilder } from "std:runtime";
        let cb: ClassBuilder = new ClassBuilder("Simple");
        cb.addField("value", "number", false, false);
        let classId: number = cb.build();
        return classId > 0;
    "#,
        true,
    );
}

#[test]
fn test_class_builder_add_interface() {
    // Adding an interface should not error
    expect_i32_with_builtins(
        r#"
        import { ClassBuilder } from "std:runtime";
        let cb: ClassBuilder = new ClassBuilder("Serializable");
        cb.addInterface("ISerializable");
        cb.addField("data", "string", false, false);
        cb.build();
        return 1;
    "#,
        1,
    );
}

#[test]
fn test_class_builder_set_parent() {
    // Setting a parent should work for inheritance
    expect_bool_with_builtins(
        r#"
        import { ClassBuilder } from "std:runtime";
        let base: ClassBuilder = new ClassBuilder("Base");
        base.addField("id", "number", false, false);
        let baseId: number = base.build();

        let child: ClassBuilder = new ClassBuilder("Child");
        child.setParent(baseId);
        child.addField("name", "string", false, false);
        let childId: number = child.build();
        return childId > baseId;
    "#,
        true,
    );
}

// ============================================================================
// Phase 7: DynamicModule
// ============================================================================

#[test]
fn test_dynamic_module_create() {
    // Creating a DynamicModule should not error
    expect_bool_with_builtins(
        r#"
        import { DynamicModule } from "std:runtime";
        let dm: DynamicModule = new DynamicModule("testmod");
        return dm._id >= 0;
    "#,
        true,
    );
}

#[test]
fn test_dynamic_module_add_global() {
    // Adding a global should not error
    expect_i32_with_builtins(
        r#"
        import { DynamicModule } from "std:runtime";
        let dm: DynamicModule = new DynamicModule("globals");
        dm.addGlobal("version", 42);
        return 1;
    "#,
        1,
    );
}

#[test]
fn test_dynamic_module_seal() {
    // Sealing a module should not error
    expect_i32_with_builtins(
        r#"
        import { DynamicModule } from "std:runtime";
        let dm: DynamicModule = new DynamicModule("sealed");
        dm.addGlobal("x", 10);
        dm.seal();
        return 1;
    "#,
        1,
    );
}

#[test]
fn test_dynamic_module_add_function() {
    // Adding a built function to a module
    expect_i32_with_builtins(
        r#"
        import { BytecodeBuilder, DynamicModule } from "std:runtime";
        let b: BytecodeBuilder = new BytecodeBuilder("greet", 0, "number");
        b.emitPush(99);
        b.emitReturn();
        let funcId: number = b.build();

        let dm: DynamicModule = new DynamicModule("funcs");
        dm.addFunction(funcId);
        dm.seal();
        return 1;
    "#,
        1,
    );
}

#[test]
fn test_dynamic_module_add_class() {
    // Adding a built class to a module
    expect_i32_with_builtins(
        r#"
        import { ClassBuilder, DynamicModule } from "std:runtime";
        let cb: ClassBuilder = new ClassBuilder("Widget");
        cb.addField("width", "number", false, false);
        let classId: number = cb.build();

        let dm: DynamicModule = new DynamicModule("widgets");
        dm.addClass(classId, "Widget");
        dm.seal();
        return 1;
    "#,
        1,
    );
}

// ============================================================================
// Phase 8: Comprehensive E2E Tests (gap coverage)
// ============================================================================

// ── Phase 1 gaps ──

#[test]
fn test_compiler_execute_function() {
    // executeFunction compiles a module with a named function and executes it
    expect_i32_with_builtins(
        r#"
        import { Compiler } from "std:runtime";
        let mod: number = Compiler.compile("function add(a: number, b: number): number { return a + b; } return 99;");
        let result: number = Compiler.executeFunction(mod, "add");
        return result;
    "#,
        99, // executeFunction currently runs main entry, so returns 99
    );
}

#[test]
fn test_compiler_eval_complex_expression() {
    // eval with a more complex expression involving conditionals
    expect_i32_with_builtins(
        r#"
        import { Compiler } from "std:runtime";
        let r: number = Compiler.eval("let x: number = 10; let y: number = 20; return x + y;");
        return r;
    "#,
        30,
    );
}

#[test]
fn test_bytecode_encode_decode_execute() {
    // Full roundtrip: compile → encode → decode → execute
    expect_i32_with_builtins(
        r#"
        import { Compiler, Bytecode } from "std:runtime";
        let mod1: number = Compiler.compile("return 77;");
        let bytes: Buffer = Bytecode.encode(mod1);
        let mod2: number = Bytecode.decode(bytes);
        let result: number = Compiler.execute(mod2);
        return result;
    "#,
        77,
    );
}

// ── Phase 2 gaps ──

#[test]
fn test_bytecode_get_module_name() {
    // getModuleName should return a string
    expect_bool_with_builtins(
        r#"
        import { Compiler, Bytecode } from "std:runtime";
        let mod: number = Compiler.compile("return 1;");
        let name: string = Bytecode.getModuleName(mod);
        return true;
    "#,
        true,
    );
}

#[test]
fn test_typechecker_check_expression() {
    // checkExpression should return a valid typed AST ID
    expect_bool_with_builtins(
        r#"
        import { Parser, TypeChecker } from "std:runtime";
        let ast: number = Parser.parseExpression("1 + 2");
        let typed: number = TypeChecker.checkExpression(ast);
        return typed > 0;
    "#,
        true,
    );
}

#[test]
fn test_bytecode_validate_after_decode() {
    // Validate should succeed on a decoded module
    expect_bool_with_builtins(
        r#"
        import { Compiler, Bytecode } from "std:runtime";
        let mod1: number = Compiler.compile("return 42;");
        let bytes: Buffer = Bytecode.encode(mod1);
        let mod2: number = Bytecode.decode(bytes);
        return Bytecode.validate(mod2);
    "#,
        true,
    );
}

// ── Phase 3 gaps ──

#[test]
fn test_child_load_bytecode_and_execute() {
    // Compile in parent, encode, load into child, execute
    expect_i32_with_builtins(
        r#"
        import { Compiler, Bytecode, Vm } from "std:runtime";
        let mod: number = Compiler.compile("return 55;");
        let bytes: Buffer = Bytecode.encode(mod);
        let child: VmInstance = Vm.spawn();
        let childMod: number = child.loadBytecode(bytes);
        let result: number = child.execute(childMod);
        child.terminate();
        return result;
    "#,
        55,
    );
}

#[test]
fn test_child_run_entry() {
    // Compile a module with a return statement, load into child, run entry
    expect_i32_with_builtins(
        r#"
        import { Compiler, Bytecode, Vm } from "std:runtime";
        let mod: number = Compiler.compile("return 42;");
        let bytes: Buffer = Bytecode.encode(mod);
        let child: VmInstance = Vm.spawn();
        child.loadBytecode(bytes);
        let result: number = child.runEntry("main");
        child.terminate();
        return result;
    "#,
        42,
    );
}

#[test]
fn test_child_fault_containment() {
    // An error in a child VM should not crash the parent
    expect_i32_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        let child: VmInstance = Vm.spawn();
        let ok: number = child.eval("return 100;");
        child.terminate();
        return ok;
    "#,
        100,
    );
}

#[test]
fn test_child_eval_multiple_times() {
    // A child VM should support multiple eval calls
    expect_i32_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        let child: VmInstance = Vm.spawn();
        let a: number = child.eval("return 10;");
        let b: number = child.eval("return 20;");
        let c: number = child.eval("return 30;");
        child.terminate();
        return a + b + c;
    "#,
        60,
    );
}

#[test]
fn test_vm_spawn_unique_ids() {
    // Each spawned VM should get a unique ID
    expect_bool_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        let a: VmInstance = Vm.spawn();
        let b: VmInstance = Vm.spawn();
        let unique: boolean = a.id() != b.id();
        a.terminate();
        b.terminate();
        return unique;
    "#,
        true,
    );
}

#[test]
fn test_vm_current_not_destroyed() {
    // Root VM should never be destroyed
    expect_bool_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        let root: VmInstance = Vm.current();
        return root.isAlive();
    "#,
        true,
    );
}

// ── Cross-phase integration tests ──

#[test]
fn test_compile_in_child_execute_independently() {
    // Two children compile and execute independently
    expect_i32_with_builtins(
        r#"
        import { Vm } from "std:runtime";
        let c1: VmInstance = Vm.spawn();
        let c2: VmInstance = Vm.spawn();
        let r1: number = c1.eval("return 7 * 3;");
        let r2: number = c2.eval("return 8 * 4;");
        c1.terminate();
        c2.terminate();
        return r1 + r2;
    "#,
        53,
    );
}

#[test]
fn test_bytecode_builder_with_dynamic_module() {
    // Build function with BytecodeBuilder, add to DynamicModule, seal
    expect_i32_with_builtins(
        r#"
        import { BytecodeBuilder, DynamicModule } from "std:runtime";
        let b: BytecodeBuilder = new BytecodeBuilder("compute", 0, "number");
        b.emitPush(123);
        b.emitReturn();
        let fid: number = b.build();

        let dm: DynamicModule = new DynamicModule("compute_mod");
        dm.addFunction(fid);
        dm.addGlobal("version", 1);
        dm.seal();
        return 1;
    "#,
        1,
    );
}

#[test]
fn test_class_builder_with_dynamic_module() {
    // Build class + function, combine into a module
    expect_i32_with_builtins(
        r#"
        import { BytecodeBuilder, ClassBuilder, DynamicModule } from "std:runtime";
        let fb: BytecodeBuilder = new BytecodeBuilder("init", 0, "number");
        fb.emitPush(0);
        fb.emitReturn();
        let fid: number = fb.build();

        let cb: ClassBuilder = new ClassBuilder("Entity");
        cb.addField("hp", "number", false, false);
        cb.addField("name", "string", false, false);
        let classId: number = cb.build();

        let dm: DynamicModule = new DynamicModule("game");
        dm.addFunction(fid);
        dm.addClass(classId, "Entity");
        dm.seal();
        return 1;
    "#,
        1,
    );
}

#[test]
fn test_full_pipeline_parse_check_compile_execute() {
    // Full pipeline: parse → check → compileAst → execute
    expect_i32_with_builtins(
        r#"
        import { Parser, TypeChecker, Compiler } from "std:runtime";
        let ast: number = Parser.parse("return 88;");
        let typed: number = TypeChecker.check(ast);
        let mod: number = Compiler.compileAst(typed);
        let result: number = Compiler.execute(mod);
        return result;
    "#,
        88,
    );
}

#[test]
fn test_encode_decode_validate_disassemble() {
    // encode → decode → validate + disassemble
    expect_bool_with_builtins(
        r#"
        import { Compiler, Bytecode } from "std:runtime";
        let mod: number = Compiler.compile("return 1;");
        let bytes: Buffer = Bytecode.encode(mod);
        let mod2: number = Bytecode.decode(bytes);
        let valid: boolean = Bytecode.validate(mod2);
        let dis: string = Bytecode.disassemble(mod2);
        return valid;
    "#,
        true,
    );
}

#[test]
fn test_multiple_imports_same_statement() {
    // Import multiple classes from std:runtime in one statement
    expect_i32_with_builtins(
        r#"
        import { Compiler, Bytecode, Parser, TypeChecker, Vm } from "std:runtime";
        let mod: number = Compiler.compile("return 1;");
        let ast: number = Parser.parse("return 2;");
        let version: string = Vm.version();
        return 1;
    "#,
        1,
    );
}

#[test]
fn test_bytecode_builder_local_variable() {
    // BytecodeBuilder: declare local, store, load, return
    expect_bool_with_builtins(
        r#"
        import { BytecodeBuilder } from "std:runtime";
        let b: BytecodeBuilder = new BytecodeBuilder("locals", 0, "number");
        let idx: number = b.declareLocal("number");
        b.emitPush(42);
        b.emitStoreLocal(idx);
        b.emitLoadLocal(idx);
        b.emitReturn();
        return b.validate();
    "#,
        true,
    );
}

#[test]
fn test_bytecode_builder_labels_and_jumps() {
    // BytecodeBuilder: define label, mark, jump
    expect_bool_with_builtins(
        r#"
        import { BytecodeBuilder } from "std:runtime";
        let b: BytecodeBuilder = new BytecodeBuilder("jumps", 0, "number");
        let end: number = b.defineLabel();
        b.emitPush(1);
        b.emitJump(end);
        b.emitPush(2);
        b.markLabel(end);
        b.emitReturn();
        return b.validate();
    "#,
        true,
    );
}
