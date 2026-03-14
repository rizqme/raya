//! Standalone JIT performance benchmark
//!
//! Measures JIT compilation pipeline throughput, native execution speed,
//! and compares with interpreter execution.
//!
//! Run with:
//!   cargo run --example jit_bench --features jit --release
//!
//! All timings are measured via std::time::Instant (no external dependencies).

use std::hint::black_box;
use std::panic;
use std::ptr;
use std::time::{Duration, Instant};

use raya_engine::compiler::bytecode::{ConstantPool, Function, Metadata, Module, Opcode};
use raya_engine::jit::backend::cranelift::lowering::{jit_entry_signature, LoweringContext};
use raya_engine::jit::pipeline::lifter::lift_function;
use raya_engine::jit::runtime::trampoline::JitEntryFn;
use raya_engine::jit::{JitConfig, JitEngine};
use raya_engine::Vm;

use cranelift_codegen::ir;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::Context;
use cranelift_frontend::FunctionBuilderContext;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::Module as CraneliftModule;

// ============================================================================
// NaN-boxing decode
// ============================================================================

fn decode_i32(val: u64) -> i32 {
    val as i32
}

// ============================================================================
// Bytecode builders
// ============================================================================

fn emit(code: &mut Vec<u8>, op: Opcode) {
    code.push(op as u8);
}

fn emit_i32(code: &mut Vec<u8>, val: i32) {
    code.push(Opcode::ConstI32 as u8);
    code.extend_from_slice(&val.to_le_bytes());
}

fn emit_f64(code: &mut Vec<u8>, val: f64) {
    code.push(Opcode::ConstF64 as u8);
    code.extend_from_slice(&val.to_le_bytes());
}

fn emit_load_local(code: &mut Vec<u8>, idx: u16) {
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&idx.to_le_bytes());
}

fn emit_store_local(code: &mut Vec<u8>, idx: u16) {
    code.push(Opcode::StoreLocal as u8);
    code.extend_from_slice(&idx.to_le_bytes());
}

fn emit_jmp_placeholder(code: &mut Vec<u8>, op: Opcode) -> usize {
    code.push(op as u8);
    let pos = code.len(); // i16 immediate position
                          // VM interpreter reads jump operands with read_i16(), and production
                          // compiler codegen also patches i16 relative offsets.
    code.extend_from_slice(&0i16.to_le_bytes());
    pos
}

fn patch_jump(code: &mut [u8], jump_pos: usize, target_pos: usize) {
    // Match compiler codegen semantics:
    // offset is relative to IP after reading i16 immediate.
    let rel = target_pos as isize - (jump_pos as isize + 2);
    let rel_i16 = i16::try_from(rel).expect("benchmark jump offset must fit i16");
    code[jump_pos..jump_pos + 2].copy_from_slice(&rel_i16.to_le_bytes());
}

fn make_module(name: &str, functions: Vec<Function>) -> Module {
    Module {
        magic: *b"RAYA",
        version: 1,
        flags: 0,
        constants: ConstantPool::new(),
        functions,
        classes: vec![],
        metadata: Metadata {
            name: name.to_string(),
            source_file: None,
            generic_templates: vec![],
            template_symbol_table: vec![],
            mono_debug_map: vec![],
            structural_shapes: vec![],
            structural_layouts: vec![],
        },
        exports: vec![],
        imports: vec![],
        checksum: [0; 32],
        reflection: None,
        debug_info: None,
        native_functions: vec![],
        jit_hints: vec![],
    }
}

// ============================================================================
// Workload generators
// ============================================================================

/// Straight-line arithmetic: (1+2)*3 repeated N times, accumulating.
fn build_arithmetic_workload(repeat: usize) -> Vec<u8> {
    let mut code = Vec::new();
    emit_i32(&mut code, 1);
    emit_i32(&mut code, 2);
    emit(&mut code, Opcode::Iadd);
    emit_i32(&mut code, 3);
    emit(&mut code, Opcode::Imul);

    for _ in 1..repeat {
        emit_i32(&mut code, 1);
        emit_i32(&mut code, 2);
        emit(&mut code, Opcode::Iadd);
        emit_i32(&mut code, 3);
        emit(&mut code, Opcode::Imul);
        emit(&mut code, Opcode::Iadd);
    }
    emit(&mut code, Opcode::Return);
    code
}

/// Float arithmetic workload.
fn build_float_workload(repeat: usize) -> Vec<u8> {
    let mut code = Vec::new();
    emit_f64(&mut code, 1.5);
    emit_f64(&mut code, 2.5);
    emit(&mut code, Opcode::Fadd);

    for _ in 1..repeat {
        emit_f64(&mut code, 3.14);
        emit(&mut code, Opcode::Fmul);
        emit_f64(&mut code, 1.0);
        emit(&mut code, Opcode::Fsub);
    }
    emit(&mut code, Opcode::Return);
    code
}

/// True branch loop workload with compiler-accurate i16 jump patching.
fn build_branch_loop_workload(iterations: i32) -> Vec<u8> {
    let mut code = Vec::new();

    // i = 0
    emit_i32(&mut code, 0);
    emit_store_local(&mut code, 0);

    let loop_head = code.len();
    // if (i < iterations) continue else exit
    emit_load_local(&mut code, 0);
    emit_i32(&mut code, iterations);
    emit(&mut code, Opcode::Ilt);
    let jmp_exit = emit_jmp_placeholder(&mut code, Opcode::JmpIfFalse);

    // i = i + 1
    emit_load_local(&mut code, 0);
    emit_i32(&mut code, 1);
    emit(&mut code, Opcode::Iadd);
    emit_store_local(&mut code, 0);

    let jmp_back = emit_jmp_placeholder(&mut code, Opcode::Jmp);
    let exit = code.len();
    patch_jump(&mut code, jmp_exit, exit);
    patch_jump(&mut code, jmp_back, loop_head);

    emit_load_local(&mut code, 0);
    emit(&mut code, Opcode::Return);
    code
}

/// More complex mixed workload with nested loops, arithmetic, modulo, and branching.
///
/// Locals:
///   0 = sum
///   1 = outer index (i)
///   2 = inner index (j)
fn build_complex_mixed_workload(outer_iters: i32, inner_iters: i32) -> Vec<u8> {
    let mut code = Vec::new();

    // sum = 0
    emit_i32(&mut code, 0);
    emit_store_local(&mut code, 0);

    // i = 0
    emit_i32(&mut code, 0);
    emit_store_local(&mut code, 1);

    let outer_head = code.len();
    // if (i < outer_iters) continue else exit
    emit_load_local(&mut code, 1);
    emit_i32(&mut code, outer_iters);
    emit(&mut code, Opcode::Ilt);
    let jmp_outer_exit = emit_jmp_placeholder(&mut code, Opcode::JmpIfFalse);

    // j = 0
    emit_i32(&mut code, 0);
    emit_store_local(&mut code, 2);

    let inner_head = code.len();
    // if (j < inner_iters) continue else after_inner
    emit_load_local(&mut code, 2);
    emit_i32(&mut code, inner_iters);
    emit(&mut code, Opcode::Ilt);
    let jmp_after_inner = emit_jmp_placeholder(&mut code, Opcode::JmpIfFalse);

    // t = (i * 3 + j * 2) % 97
    emit_load_local(&mut code, 1);
    emit_i32(&mut code, 3);
    emit(&mut code, Opcode::Imul);
    emit_load_local(&mut code, 2);
    emit_i32(&mut code, 2);
    emit(&mut code, Opcode::Imul);
    emit(&mut code, Opcode::Iadd);
    emit_i32(&mut code, 97);
    emit(&mut code, Opcode::Imod);

    // sum = sum + t
    emit_load_local(&mut code, 0);
    emit(&mut code, Opcode::Iadd);
    emit_store_local(&mut code, 0);

    // if ((j % 2) == 0) sum = sum - 5;
    emit_load_local(&mut code, 2);
    emit_i32(&mut code, 2);
    emit(&mut code, Opcode::Imod);
    emit_i32(&mut code, 0);
    emit(&mut code, Opcode::Ieq);
    let jmp_skip_adjust = emit_jmp_placeholder(&mut code, Opcode::JmpIfFalse);
    emit_load_local(&mut code, 0);
    emit_i32(&mut code, 5);
    emit(&mut code, Opcode::Isub);
    emit_store_local(&mut code, 0);
    let adjust_end = code.len();
    patch_jump(&mut code, jmp_skip_adjust, adjust_end);

    // j = j + 1
    emit_load_local(&mut code, 2);
    emit_i32(&mut code, 1);
    emit(&mut code, Opcode::Iadd);
    emit_store_local(&mut code, 2);

    // jump inner_head
    let jmp_inner_back = emit_jmp_placeholder(&mut code, Opcode::Jmp);
    let after_inner = code.len();
    patch_jump(&mut code, jmp_after_inner, after_inner);
    patch_jump(&mut code, jmp_inner_back, inner_head);

    // i = i + 1
    emit_load_local(&mut code, 1);
    emit_i32(&mut code, 1);
    emit(&mut code, Opcode::Iadd);
    emit_store_local(&mut code, 1);

    // jump outer_head
    let jmp_outer_back = emit_jmp_placeholder(&mut code, Opcode::Jmp);
    let outer_exit = code.len();
    patch_jump(&mut code, jmp_outer_exit, outer_exit);
    patch_jump(&mut code, jmp_outer_back, outer_head);

    // return sum
    emit_load_local(&mut code, 0);
    emit(&mut code, Opcode::Return);
    code
}

/// Matrix multiplication kernel-like workload (integer arithmetic, triple nested loops).
///
/// Computes a deterministic accumulation equivalent to summing all cells of C = A * B
/// for synthetic matrices:
///   A[i,k] = (i+1)*(k+1)
///   B[k,j] = (k+1)*(j+1)
///
/// This avoids array/object overhead and isolates loop+arithmetic behavior.
///
/// Locals:
///   0 = total
///   1 = i
///   2 = j
///   3 = k
///   4 = cell_acc
fn build_matrix_multiply_kernel_workload(n: i32) -> Vec<u8> {
    let mut code = Vec::new();

    // total = 0
    emit_i32(&mut code, 0);
    emit_store_local(&mut code, 0);

    // i = 0
    emit_i32(&mut code, 0);
    emit_store_local(&mut code, 1);

    let i_head = code.len();
    // if (i < n) else exit
    emit_load_local(&mut code, 1);
    emit_i32(&mut code, n);
    emit(&mut code, Opcode::Ilt);
    let jmp_i_exit = emit_jmp_placeholder(&mut code, Opcode::JmpIfFalse);

    // j = 0
    emit_i32(&mut code, 0);
    emit_store_local(&mut code, 2);

    let j_head = code.len();
    // if (j < n) else after_j
    emit_load_local(&mut code, 2);
    emit_i32(&mut code, n);
    emit(&mut code, Opcode::Ilt);
    let jmp_after_j = emit_jmp_placeholder(&mut code, Opcode::JmpIfFalse);

    // k = 0
    emit_i32(&mut code, 0);
    emit_store_local(&mut code, 3);
    // cell_acc = 0
    emit_i32(&mut code, 0);
    emit_store_local(&mut code, 4);

    let k_head = code.len();
    // if (k < n) else after_k
    emit_load_local(&mut code, 3);
    emit_i32(&mut code, n);
    emit(&mut code, Opcode::Ilt);
    let jmp_after_k = emit_jmp_placeholder(&mut code, Opcode::JmpIfFalse);

    // prod = ((i+1)*(k+1))*((k+1)*(j+1))
    // left: (i+1)*(k+1)
    emit_load_local(&mut code, 1);
    emit_i32(&mut code, 1);
    emit(&mut code, Opcode::Iadd);
    emit_load_local(&mut code, 3);
    emit_i32(&mut code, 1);
    emit(&mut code, Opcode::Iadd);
    emit(&mut code, Opcode::Imul);

    // right: (k+1)*(j+1)
    emit_load_local(&mut code, 3);
    emit_i32(&mut code, 1);
    emit(&mut code, Opcode::Iadd);
    emit_load_local(&mut code, 2);
    emit_i32(&mut code, 1);
    emit(&mut code, Opcode::Iadd);
    emit(&mut code, Opcode::Imul);

    // prod
    emit(&mut code, Opcode::Imul);

    // cell_acc += prod
    emit_load_local(&mut code, 4);
    emit(&mut code, Opcode::Iadd);
    emit_store_local(&mut code, 4);

    // k = k + 1
    emit_load_local(&mut code, 3);
    emit_i32(&mut code, 1);
    emit(&mut code, Opcode::Iadd);
    emit_store_local(&mut code, 3);

    // jump k_head
    let jmp_k_back = emit_jmp_placeholder(&mut code, Opcode::Jmp);
    let after_k = code.len();
    patch_jump(&mut code, jmp_after_k, after_k);
    patch_jump(&mut code, jmp_k_back, k_head);

    // total += cell_acc
    emit_load_local(&mut code, 0);
    emit_load_local(&mut code, 4);
    emit(&mut code, Opcode::Iadd);
    emit_store_local(&mut code, 0);

    // j = j + 1
    emit_load_local(&mut code, 2);
    emit_i32(&mut code, 1);
    emit(&mut code, Opcode::Iadd);
    emit_store_local(&mut code, 2);

    // jump j_head
    let jmp_j_back = emit_jmp_placeholder(&mut code, Opcode::Jmp);
    let after_j = code.len();
    patch_jump(&mut code, jmp_after_j, after_j);
    patch_jump(&mut code, jmp_j_back, j_head);

    // i = i + 1
    emit_load_local(&mut code, 1);
    emit_i32(&mut code, 1);
    emit(&mut code, Opcode::Iadd);
    emit_store_local(&mut code, 1);

    // jump i_head
    let jmp_i_back = emit_jmp_placeholder(&mut code, Opcode::Jmp);
    let i_exit = code.len();
    patch_jump(&mut code, jmp_i_exit, i_exit);
    patch_jump(&mut code, jmp_i_back, i_head);

    // return total
    emit_load_local(&mut code, 0);
    emit(&mut code, Opcode::Return);
    code
}

// ============================================================================
// JIT compilation + execution helpers
// ============================================================================

struct CompiledFunction {
    _jit_module: JITModule,
    entry_fn: JitEntryFn,
    local_count: usize,
}

fn jit_compile(module: &Module, func_idx: usize) -> Result<CompiledFunction, String> {
    let func = &module.functions[func_idx];
    let jit_func =
        lift_function(func, module, func_idx as u32).map_err(|e| format!("Lift: {}", e))?;

    let mut flag_builder = settings::builder();
    flag_builder
        .set("opt_level", "speed")
        .map_err(|e| format!("{}", e))?;
    flag_builder
        .set("is_pic", "false")
        .map_err(|e| format!("{}", e))?;
    let flags = settings::Flags::new(flag_builder);

    let isa = cranelift_native::builder()
        .map_err(|e| format!("{}", e))?
        .finish(flags)
        .map_err(|e| format!("{}", e))?;
    let call_conv = isa.default_call_conv();

    let builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
    let mut jit_module = JITModule::new(builder);

    let sig = jit_entry_signature(call_conv);
    let func_id = jit_module
        .declare_function("bench_func", cranelift_module::Linkage::Local, &sig)
        .map_err(|e| format!("{}", e))?;

    let mut ctx = Context::new();
    ctx.func.signature = jit_entry_signature(call_conv);
    ctx.func.name = ir::UserFuncName::user(0, jit_func.func_index);

    let mut func_builder_ctx = FunctionBuilderContext::new();
    {
        let builder =
            cranelift_frontend::FunctionBuilder::new(&mut ctx.func, &mut func_builder_ctx);
        LoweringContext::lower(&jit_func, module, builder).map_err(|e| format!("Lower: {}", e))?;
    }

    jit_module
        .define_function(func_id, &mut ctx)
        .map_err(|e| format!("Define: {:?}", e))?;
    jit_module
        .finalize_definitions()
        .map_err(|e| format!("Finalize: {}", e))?;

    let code_ptr = jit_module.get_finalized_function(func_id);
    let entry_fn: JitEntryFn = unsafe { std::mem::transmute(code_ptr) };

    Ok(CompiledFunction {
        _jit_module: jit_module,
        entry_fn,
        local_count: func.local_count,
    })
}

fn jit_compile_safe(module: &Module, func_idx: usize) -> Result<CompiledFunction, String> {
    match panic::catch_unwind(panic::AssertUnwindSafe(|| jit_compile(module, func_idx))) {
        Ok(result) => result,
        Err(e) => {
            let msg = if let Some(s) = e.downcast_ref::<String>() {
                s.clone()
            } else if let Some(s) = e.downcast_ref::<&str>() {
                s.to_string()
            } else {
                "Unknown panic".to_string()
            };
            Err(format!("Panic: {}", msg))
        }
    }
}

#[inline(never)]
fn call_jit(compiled: &CompiledFunction) -> u64 {
    let mut exit = raya_engine::jit::runtime::trampoline::JitExitInfo::default();
    let mut local_buf = [0u64; 8];
    assert!(
        compiled.local_count <= local_buf.len(),
        "benchmark workload local_count too large for fixed local buffer"
    );
    unsafe {
        (compiled.entry_fn)(
            ptr::null(),
            0,
            local_buf.as_mut_ptr(),
            compiled.local_count as u32,
            ptr::null_mut(),
            (&mut exit as *mut _),
        )
    }
}

// ============================================================================
// Benchmark harness
// ============================================================================

struct BenchResult {
    name: String,
    iterations: u64,
    total: Duration,
    per_iter: Duration,
}

impl BenchResult {
    fn print(&self) {
        let per_iter_ns = self.per_iter.as_nanos();
        let (val, unit) = if per_iter_ns >= 1_000_000 {
            (per_iter_ns as f64 / 1_000_000.0, "ms")
        } else if per_iter_ns >= 1_000 {
            (per_iter_ns as f64 / 1_000.0, "us")
        } else {
            (per_iter_ns as f64, "ns")
        };
        println!(
            "  {:<45} {:>10.2} {:<3} ({} iters, {:.2?} total)",
            self.name, val, unit, self.iterations, self.total
        );
    }
}

fn bench<F: FnMut() -> u64>(
    name: &str,
    warmup_iters: u64,
    bench_iters: u64,
    mut f: F,
) -> BenchResult {
    for _ in 0..warmup_iters {
        black_box(f());
    }

    let start = Instant::now();
    for _ in 0..bench_iters {
        black_box(f());
    }
    let total = start.elapsed();
    let per_iter = total / bench_iters as u32;

    BenchResult {
        name: name.to_string(),
        iterations: bench_iters,
        total,
        per_iter,
    }
}

/// Variant that doesn't return a value (for compilation benchmarks)
fn bench_void<F: FnMut()>(
    name: &str,
    warmup_iters: u64,
    bench_iters: u64,
    mut f: F,
) -> BenchResult {
    for _ in 0..warmup_iters {
        f();
    }

    let start = Instant::now();
    for _ in 0..bench_iters {
        f();
    }
    let total = start.elapsed();
    let per_iter = total / bench_iters as u32;

    BenchResult {
        name: name.to_string(),
        iterations: bench_iters,
        total,
        per_iter,
    }
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    println!("=================================================================");
    println!("  Raya JIT Performance Benchmark");
    println!("=================================================================\n");

    // -------------------------------------------------------------------
    // 1. JIT Compilation Pipeline
    // -------------------------------------------------------------------
    println!("--- JIT Compilation Pipeline (bytecode -> SSA IR -> Cranelift -> native) ---\n");

    for &size in &[10, 50, 100, 500] {
        let code = build_arithmetic_workload(size);
        let module = make_module(
            "bench",
            vec![Function {
                name: "bench_fn".to_string(),
                param_count: 0,
                local_count: 0,
                code: code.clone(),
            }],
        );

        let iters = if size <= 50 { 500 } else { 100 };
        let result = bench_void(
            &format!("compile arith({}ops, {}B bytecode)", size * 4, code.len()),
            10,
            iters,
            || {
                let _ = jit_compile_safe(&module, 0);
            },
        );
        result.print();
    }

    for &size in &[10, 100] {
        let code = build_float_workload(size);
        let module = make_module(
            "bench",
            vec![Function {
                name: "bench_fn".to_string(),
                param_count: 0,
                local_count: 0,
                code: code.clone(),
            }],
        );

        let result = bench_void(
            &format!("compile float({}ops, {}B bytecode)", size * 3, code.len()),
            10,
            200,
            || {
                let _ = jit_compile_safe(&module, 0);
            },
        );
        result.print();
    }

    println!();

    // -------------------------------------------------------------------
    // 2. JIT Execution (native function pointer calls)
    // -------------------------------------------------------------------
    println!("--- JIT Native Execution ---\n");

    for &iters in &[64, 256, 1024, 4096] {
        let code = build_branch_loop_workload(iters);
        let module = make_module(
            "bench",
            vec![Function {
                name: "bench_fn".to_string(),
                param_count: 0,
                local_count: 1,
                code,
            }],
        );

        match jit_compile_safe(&module, 0) {
            Ok(compiled) => {
                let result = bench(
                    &format!("exec jit branch-loop(iters={})", iters),
                    1_000,
                    200_000,
                    || call_jit(&compiled),
                );
                result.print();
            }
            Err(e) => {
                println!("  [skip] branch-loop(iters={}): {}", iters, e);
            }
        }
    }

    println!();

    // -------------------------------------------------------------------
    // 3. Interpreter Execution (reusing single Vm)
    // -------------------------------------------------------------------
    println!("--- Interpreter Execution ---\n");

    for &iters in &[64, 256, 1024, 4096] {
        let code = build_branch_loop_workload(iters);
        let module = make_module(
            "bench",
            vec![Function {
                name: "main".to_string(),
                param_count: 0,
                local_count: 1,
                code,
            }],
        );

        let mut vm = Vm::with_worker_count(1);
        let bench_iters = if iters <= 1024 { 1_000 } else { 300 };
        let result = bench(
            &format!("exec interp branch-loop(iters={})", iters),
            10,
            bench_iters,
            || {
                let r = vm.execute(&module).unwrap();
                r.as_i32().unwrap_or(0) as u64
            },
        );
        result.print();
    }

    println!();

    // -------------------------------------------------------------------
    // 4. JIT Engine Prewarm
    // -------------------------------------------------------------------
    println!("--- JIT Engine Prewarm ---\n");

    for &func_count in &[1, 5, 10] {
        let mut functions = Vec::new();
        for i in 0..func_count {
            let code = build_arithmetic_workload(50);
            functions.push(Function {
                name: format!("func_{}", i),
                param_count: 0,
                local_count: 0,
                code,
            });
        }
        let module = make_module("prewarm_bench", functions);

        let result = bench_void(
            &format!("prewarm {}funcs (arith 200ops each)", func_count),
            5,
            50,
            || {
                let config = JitConfig {
                    min_score: 1.0,
                    min_instruction_count: 2,
                    ..Default::default()
                };
                let mut engine = JitEngine::with_config(config).unwrap();
                let _ = engine.prewarm(&module);
            },
        );
        result.print();
    }

    println!();

    // -------------------------------------------------------------------
    // 5. JIT vs Interpreter Comparison
    // -------------------------------------------------------------------
    println!("--- JIT vs Interpreter Comparison ---\n");
    println!(
        "  {:<30} {:>12} {:>12} {:>10}",
        "Workload", "Interpreter", "JIT Native", "Speedup"
    );
    println!("  {:-<30} {:-<12} {:-<12} {:-<10}", "", "", "", "");

    for &iters in &[64, 256, 1024, 4096] {
        let code = build_branch_loop_workload(iters);

        // JIT
        let jit_module = make_module(
            "bench",
            vec![Function {
                name: "bench_fn".to_string(),
                param_count: 0,
                local_count: 1,
                code: code.clone(),
            }],
        );

        let jit_per_iter = match jit_compile_safe(&jit_module, 0) {
            Ok(compiled) => {
                let r = bench("", 1_000, 200_000, || call_jit(&compiled));
                Some(r.per_iter)
            }
            Err(_) => None,
        };

        // Interpreter
        let interp_module = make_module(
            "bench",
            vec![Function {
                name: "main".to_string(),
                param_count: 0,
                local_count: 1,
                code,
            }],
        );

        let mut vm = Vm::with_worker_count(1);
        let interp_iters = if iters <= 1024 { 200 } else { 100 };
        let interp_result = bench("", 5, interp_iters, || {
            let r = vm.execute(&interp_module).unwrap();
            r.as_i32().unwrap_or(0) as u64
        });

        let label = format!("branch-loop(iters={})", iters);

        match jit_per_iter {
            Some(jit_dur) if jit_dur.as_nanos() > 0 => {
                let speedup = interp_result.per_iter.as_nanos() as f64 / jit_dur.as_nanos() as f64;
                println!(
                    "  {:<30} {:>10.2} us {:>10.2} ns {:>8.1}x",
                    label,
                    interp_result.per_iter.as_nanos() as f64 / 1_000.0,
                    jit_dur.as_nanos() as f64,
                    speedup
                );
            }
            _ => {
                println!(
                    "  {:<30} {:>10.2} us {:>12} {:>10}",
                    label,
                    interp_result.per_iter.as_nanos() as f64 / 1_000.0,
                    "[failed]",
                    "N/A"
                );
            }
        }
    }

    println!();

    // -------------------------------------------------------------------
    // 6. Compilation Pipeline Breakdown
    // -------------------------------------------------------------------
    println!("--- Compilation Pipeline Breakdown (100-op workload) ---\n");

    {
        let code = build_arithmetic_workload(100);
        let module = make_module(
            "bench",
            vec![Function {
                name: "bench_fn".to_string(),
                param_count: 0,
                local_count: 0,
                code,
            }],
        );

        let iters = 200;

        let lift_result = bench_void("1. lift (bytecode -> SSA IR)", 10, iters, || {
            let _ = lift_function(&module.functions[0], &module, 0);
        });
        lift_result.print();

        let jit_func = lift_function(&module.functions[0], &module, 0).unwrap();

        let lower_result = bench_void("2. lower + codegen (SSA IR -> native)", 10, iters, || {
            let mut flag_builder = settings::builder();
            flag_builder.set("opt_level", "speed").unwrap();
            flag_builder.set("is_pic", "false").unwrap();
            let flags = settings::Flags::new(flag_builder);
            let isa = cranelift_native::builder().unwrap().finish(flags).unwrap();
            let call_conv = isa.default_call_conv();
            let builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
            let mut jit_module = JITModule::new(builder);

            let sig = jit_entry_signature(call_conv);
            let func_id = jit_module
                .declare_function("f", cranelift_module::Linkage::Local, &sig)
                .unwrap();

            let mut ctx = Context::new();
            ctx.func.signature = jit_entry_signature(call_conv);
            ctx.func.name = ir::UserFuncName::user(0, 0);

            let mut fbc = FunctionBuilderContext::new();
            {
                let builder = cranelift_frontend::FunctionBuilder::new(&mut ctx.func, &mut fbc);
                LoweringContext::lower(&jit_func, &module, builder).unwrap();
            }

            jit_module.define_function(func_id, &mut ctx).unwrap();
            jit_module.finalize_definitions().unwrap();
        });
        lower_result.print();

        let lift_ns = lift_result.per_iter.as_nanos();
        let lower_ns = lower_result.per_iter.as_nanos();
        let total_ns = lift_ns + lower_ns;
        println!();
        println!(
            "  Lift: {:.1}% | Lower+Codegen: {:.1}%",
            lift_ns as f64 / total_ns as f64 * 100.0,
            lower_ns as f64 / total_ns as f64 * 100.0
        );
    }

    println!();

    // -------------------------------------------------------------------
    // 7. Correctness verification
    // -------------------------------------------------------------------
    println!("--- Correctness Verification ---\n");

    for &size in &[10, 50, 100] {
        let code = build_arithmetic_workload(size);

        let interp_module = make_module(
            "verify",
            vec![Function {
                name: "main".to_string(),
                param_count: 0,
                local_count: 0,
                code: code.clone(),
            }],
        );
        let mut vm = Vm::with_worker_count(1);
        let interp_val = vm.execute(&interp_module).unwrap();

        let jit_module = make_module(
            "verify",
            vec![Function {
                name: "bench_fn".to_string(),
                param_count: 0,
                local_count: 0,
                code,
            }],
        );

        match jit_compile_safe(&jit_module, 0) {
            Ok(compiled) => {
                let jit_raw = call_jit(&compiled);
                let jit_i32 = decode_i32(jit_raw);
                let interp_i32 = interp_val.as_i32().unwrap_or(0);
                let ok = jit_i32 == interp_i32;
                println!(
                    "  arith({}ops): interp={}, jit={} {}",
                    size * 4,
                    interp_i32,
                    jit_i32,
                    if ok { "[OK]" } else { "[MISMATCH!]" }
                );
            }
            Err(e) => {
                println!(
                    "  arith({}ops): interp={:?}, jit=[failed: {}]",
                    size * 4,
                    interp_val,
                    e
                );
            }
        }
    }

    println!();

    // -------------------------------------------------------------------
    // 8. Complex Mixed Workload (nested loops + branches + arithmetic)
    // -------------------------------------------------------------------
    println!("--- Complex Mixed Workload ---\n");
    println!(
        "  {:<30} {:>12} {:>12} {:>10}",
        "Workload", "Interpreter", "JIT Native", "Speedup"
    );
    println!("  {:-<30} {:-<12} {:-<12} {:-<10}", "", "", "", "");

    for &(outer, inner) in &[(32, 32), (64, 32), (64, 64)] {
        let code = build_complex_mixed_workload(outer, inner);
        let label = format!("mixed(o={},i={})", outer, inner);

        let jit_module = make_module(
            "bench",
            vec![Function {
                name: "bench_fn".to_string(),
                param_count: 0,
                local_count: 3,
                code: code.clone(),
            }],
        );

        let interp_module = make_module(
            "bench",
            vec![Function {
                name: "main".to_string(),
                param_count: 0,
                local_count: 3,
                code,
            }],
        );

        let jit_per_iter = match jit_compile_safe(&jit_module, 0) {
            Ok(compiled) => {
                let r = bench("", 1_000, 100_000, || call_jit(&compiled));
                Some(r.per_iter)
            }
            Err(_) => None,
        };

        let mut vm = Vm::with_worker_count(1);
        let interp_result = bench("", 5, 200, || {
            let r = vm.execute(&interp_module).unwrap();
            r.as_i32().unwrap_or(0) as u64
        });

        match jit_per_iter {
            Some(jit_dur) if jit_dur.as_nanos() > 0 => {
                let speedup = interp_result.per_iter.as_nanos() as f64 / jit_dur.as_nanos() as f64;
                println!(
                    "  {:<30} {:>10.2} us {:>10.2} ns {:>8.1}x",
                    label,
                    interp_result.per_iter.as_nanos() as f64 / 1_000.0,
                    jit_dur.as_nanos() as f64,
                    speedup
                );
            }
            _ => {
                println!(
                    "  {:<30} {:>10.2} us {:>12} {:>10}",
                    label,
                    interp_result.per_iter.as_nanos() as f64 / 1_000.0,
                    "[failed]",
                    "N/A"
                );
            }
        }
    }

    println!();

    // -------------------------------------------------------------------
    // 9. Matrix Multiplication Kernel Workload
    // -------------------------------------------------------------------
    println!("--- Matrix Multiplication Kernel (triple loop) ---\n");
    println!(
        "  {:<30} {:>12} {:>12} {:>10}",
        "Workload", "Interpreter", "JIT Native", "Speedup"
    );
    println!("  {:-<30} {:-<12} {:-<12} {:-<10}", "", "", "", "");

    for &n in &[8, 16, 24] {
        let code = build_matrix_multiply_kernel_workload(n);
        let label = format!("matmul-kernel(n={})", n);

        let jit_module = make_module(
            "bench",
            vec![Function {
                name: "bench_fn".to_string(),
                param_count: 0,
                local_count: 5,
                code: code.clone(),
            }],
        );

        let interp_module = make_module(
            "bench",
            vec![Function {
                name: "main".to_string(),
                param_count: 0,
                local_count: 5,
                code,
            }],
        );

        let jit_per_iter = match jit_compile_safe(&jit_module, 0) {
            Ok(compiled) => {
                let r = bench("", 1_000, 50_000, || call_jit(&compiled));
                Some(r.per_iter)
            }
            Err(_) => None,
        };

        let mut vm = Vm::with_worker_count(1);
        let interp_result = bench("", 5, 100, || {
            let r = vm.execute(&interp_module).unwrap();
            r.as_i32().unwrap_or(0) as u64
        });

        match jit_per_iter {
            Some(jit_dur) if jit_dur.as_nanos() > 0 => {
                let speedup = interp_result.per_iter.as_nanos() as f64 / jit_dur.as_nanos() as f64;
                println!(
                    "  {:<30} {:>10.2} us {:>10.2} ns {:>8.1}x",
                    label,
                    interp_result.per_iter.as_nanos() as f64 / 1_000.0,
                    jit_dur.as_nanos() as f64,
                    speedup
                );
            }
            _ => {
                println!(
                    "  {:<30} {:>10.2} us {:>12} {:>10}",
                    label,
                    interp_result.per_iter.as_nanos() as f64 / 1_000.0,
                    "[failed]",
                    "N/A"
                );
            }
        }
    }

    println!();
    println!("=================================================================");
    println!("  Benchmark complete.");
    println!("=================================================================");
}
