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

// ============================================================================
// JIT compilation + execution helpers
// ============================================================================

struct CompiledFunction {
    _jit_module: JITModule,
    entry_fn: JitEntryFn,
}

fn jit_compile(module: &Module, func_idx: usize) -> Result<CompiledFunction, String> {
    let func = &module.functions[func_idx];
    let jit_func =
        lift_function(func, module, func_idx as u32).map_err(|e| format!("Lift: {}", e))?;

    let mut flag_builder = settings::builder();
    flag_builder.set("opt_level", "speed").map_err(|e| format!("{}", e))?;
    flag_builder.set("is_pic", "false").map_err(|e| format!("{}", e))?;
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
        LoweringContext::lower(&jit_func, builder).map_err(|e| format!("Lower: {}", e))?;
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
    unsafe {
        (compiled.entry_fn)(ptr::null(), 0, ptr::null_mut(), 0, ptr::null_mut())
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

fn bench<F: FnMut() -> u64>(name: &str, warmup_iters: u64, bench_iters: u64, mut f: F) -> BenchResult {
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
fn bench_void<F: FnMut()>(name: &str, warmup_iters: u64, bench_iters: u64, mut f: F) -> BenchResult {
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

    for &size in &[10, 50, 100, 500] {
        let code = build_arithmetic_workload(size);
        let module = make_module(
            "bench",
            vec![Function {
                name: "bench_fn".to_string(),
                param_count: 0,
                local_count: 0,
                code,
            }],
        );

        match jit_compile_safe(&module, 0) {
            Ok(compiled) => {
                let result = bench(
                    &format!("exec jit arith({}ops)", size * 4),
                    1_000,
                    1_000_000,
                    || call_jit(&compiled),
                );
                result.print();
            }
            Err(e) => {
                println!("  [skip] arith({}ops): {}", size * 4, e);
            }
        }
    }

    println!();

    // -------------------------------------------------------------------
    // 3. Interpreter Execution (reusing single Vm)
    // -------------------------------------------------------------------
    println!("--- Interpreter Execution ---\n");

    for &size in &[10, 50, 100, 500] {
        let code = build_arithmetic_workload(size);
        let module = make_module(
            "bench",
            vec![Function {
                name: "main".to_string(),
                param_count: 0,
                local_count: 0,
                code,
            }],
        );

        let iters = if size <= 100 { 1_000 } else { 500 };
        let result = bench(
            &format!("exec interp arith({}ops)", size * 4),
            10,
            iters,
            || {
                let mut vm = Vm::with_worker_count(1);
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

    for &size in &[10, 50, 100, 500] {
        let code = build_arithmetic_workload(size);

        // JIT
        let jit_module = make_module(
            "bench",
            vec![Function {
                name: "bench_fn".to_string(),
                param_count: 0,
                local_count: 0,
                code: code.clone(),
            }],
        );

        let jit_per_iter = match jit_compile_safe(&jit_module, 0) {
            Ok(compiled) => {
                let r = bench("", 1_000, 1_000_000, || call_jit(&compiled));
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
                local_count: 0,
                code,
            }],
        );

        let interp_iters = if size <= 100 { 200 } else { 100 };
        let interp_result = bench("", 5, interp_iters, || {
            let mut vm = Vm::with_worker_count(1);
            let r = vm.execute(&interp_module).unwrap();
            r.as_i32().unwrap_or(0) as u64
        });

        let label = format!("arith({}ops)", size * 4);

        match jit_per_iter {
            Some(jit_dur) if jit_dur.as_nanos() > 0 => {
                let speedup =
                    interp_result.per_iter.as_nanos() as f64 / jit_dur.as_nanos() as f64;
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

        let lower_result = bench_void(
            "2. lower + codegen (SSA IR -> native)",
            10,
            iters,
            || {
                let mut flag_builder = settings::builder();
                flag_builder.set("opt_level", "speed").unwrap();
                flag_builder.set("is_pic", "false").unwrap();
                let flags = settings::Flags::new(flag_builder);
                let isa = cranelift_native::builder().unwrap().finish(flags).unwrap();
                let call_conv = isa.default_call_conv();
                let builder =
                    JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
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
                    let builder =
                        cranelift_frontend::FunctionBuilder::new(&mut ctx.func, &mut fbc);
                    LoweringContext::lower(&jit_func, builder).unwrap();
                }

                jit_module.define_function(func_id, &mut ctx).unwrap();
                jit_module.finalize_definitions().unwrap();
            },
        );
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
                    size * 4, interp_val, e
                );
            }
        }
    }

    println!();
    println!("=================================================================");
    println!("  Benchmark complete.");
    println!("=================================================================");
}
