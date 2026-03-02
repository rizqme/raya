use raya_engine::compiler::module::StdModuleRegistry;
use raya_engine::vm::Vm;
use raya_runtime::module_system::graph::ProgramGraphBuilder;
use raya_runtime::{Runtime, StdNativeHandler};
use std::hint::black_box;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

fn bench<F>(label: &str, iters: usize, mut f: F)
where
    F: FnMut(),
{
    let start = Instant::now();
    for _ in 0..iters {
        f();
    }
    let elapsed = start.elapsed();
    let micros_total = elapsed.as_secs_f64() * 1_000_000.0;
    let micros_each = micros_total / iters as f64;
    println!(
        "{:<52} {:>8} iters  total={:>10.3} ms  avg={:>8.3} us",
        label,
        iters,
        micros_total / 1000.0,
        micros_each
    );
}

fn unique_dir(base: &PathBuf, counter: &AtomicU64) -> PathBuf {
    let id = counter.fetch_add(1, Ordering::Relaxed);
    base.join(format!("probe-{}", id))
}

fn main() {
    println!("Raya test init/teardown probe");
    println!("-----------------------------------------------");

    bench("Runtime::new()", 200_000, || {
        black_box(Runtime::new());
    });

    bench("StdModuleRegistry::new()", 5_000, || {
        black_box(StdModuleRegistry::new());
    });

    bench(
        "StdModuleRegistry::new() + resolve_specifier(\"node:path\")",
        150_000,
        || {
            black_box(StdModuleRegistry::new().resolve_specifier("node:path"));
        },
    );

    let registry = StdModuleRegistry::new();
    bench(
        "StdModuleRegistry::resolve_specifier(\"node:path\")",
        400_000,
        || {
            black_box(registry.resolve_specifier("node:path"));
        },
    );

    let graph_builder = ProgramGraphBuilder::new();
    let virtual_entry = std::env::temp_dir().join("__probe_entry.raya");
    let graph_source = r#"
import path from "node:path";
export function sum(a: number, b: number): number { return a + b; }
const joined: string = path.join("a", "b");
return joined.length + sum(20, 22);
"#;
    bench("ProgramGraphBuilder::build_from_source()", 500, || {
        black_box(
            graph_builder
                .build_from_source(virtual_entry.clone(), graph_source.to_string())
                .expect("graph build should succeed"),
        );
    });

    let runtime = Runtime::new();
    let simple_source =
        "function add(a: number, b: number): number { return a + b; } return add(20, 22);";
    bench("Runtime::compile(simple source)", 100, || {
        let compiled = runtime
            .compile(simple_source)
            .expect("compile should succeed");
        black_box(compiled.module().functions.len());
    });

    bench("Runtime::eval(simple source)", 100, || {
        black_box(runtime.eval(simple_source).expect("eval should succeed"));
    });

    bench("Vm::with_native_handler + register stdlib", 1_000, || {
        let vm = Vm::with_native_handler(1, Arc::new(StdNativeHandler));
        {
            let mut registry = vm.native_registry().write();
            raya_stdlib::register_stdlib(&mut registry);
            raya_stdlib_posix::register_posix(&mut registry);
        }
        black_box(vm);
    });

    let vm_count = 400usize;
    let mut vms = Vec::with_capacity(vm_count);
    let start_init = Instant::now();
    for _ in 0..vm_count {
        let vm = Vm::with_native_handler(1, Arc::new(StdNativeHandler));
        {
            let mut registry = vm.native_registry().write();
            raya_stdlib::register_stdlib(&mut registry);
            raya_stdlib_posix::register_posix(&mut registry);
        }
        vms.push(vm);
    }
    let init_elapsed = start_init.elapsed();
    let init_us_each = (init_elapsed.as_secs_f64() * 1_000_000.0) / vm_count as f64;
    println!(
        "{:<52} {:>8} vms   total={:>10.3} ms  avg={:>8.3} us",
        "Vm init only (batched)",
        vm_count,
        init_elapsed.as_secs_f64() * 1000.0,
        init_us_each
    );

    let start_drop = Instant::now();
    drop(vms);
    let drop_elapsed = start_drop.elapsed();
    let drop_us_each = (drop_elapsed.as_secs_f64() * 1_000_000.0) / vm_count as f64;
    println!(
        "{:<52} {:>8} vms   total={:>10.3} ms  avg={:>8.3} us",
        "Vm teardown only (drop batched)",
        vm_count,
        drop_elapsed.as_secs_f64() * 1000.0,
        drop_us_each
    );

    let base = std::env::temp_dir().join("raya-test-init-teardown-probe");
    std::fs::create_dir_all(&base).expect("create probe base dir");
    let counter = AtomicU64::new(0);
    bench("tmpdir create + write + remove_dir_all()", 1_000, || {
        let dir = unique_dir(&base, &counter);
        std::fs::create_dir_all(&dir).expect("create temp probe dir");
        std::fs::write(dir.join("x.raya"), "return 1;").expect("write temp file");
        std::fs::remove_dir_all(&dir).expect("remove temp probe dir");
    });
    let _ = std::fs::remove_dir_all(&base);
}
