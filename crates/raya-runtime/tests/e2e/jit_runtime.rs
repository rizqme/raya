//! Narrow JIT-enabled runtime e2e coverage for the helper-backed call family.

use super::harness::expect_i32_runtime_jit;

fn assert_jit_was_active(
    telemetry: raya_engine::vm::interpreter::JitTelemetrySnapshot,
    source: &str,
) {
    assert!(
        telemetry.call_samples > 0 || telemetry.loop_samples > 0,
        "Expected JIT profiling activity for:\n{}\ntelemetry={:?}",
        source,
        telemetry
    );
    assert!(
        telemetry.cache_hits > 0 || telemetry.compile_requests_submitted > 0,
        "Expected JIT code usage or an adaptive compile request for:\n{}\ntelemetry={:?}",
        source,
        telemetry
    );
}

#[test]
fn test_runtime_jit_call_family_end_to_end() {
    let source = r#"
class Counter {
    value: number;

    constructor(start: number) {
        this.value = start;
    }

    bump(): number {
        this.value = this.value + 1;
        return this.value;
    }

    static scale(x: number): number {
        return x * 2;
    }
}

function add1(x: number): number {
    return x + 1;
}

function hot(counter: Counter, i: number): number {
    return Counter.scale(add1(counter.bump() + i));
}

let counter = new Counter(0);
let last = 0;
let i = 0;
while (i < 128) {
    last = hot(counter, i);
    i = i + 1;
}
return last + counter.value;
"#;

    let telemetry = expect_i32_runtime_jit(source, 640);
    assert_jit_was_active(telemetry, source);
}

#[test]
fn test_runtime_jit_shape_load_and_method_end_to_end() {
    let source = r#"
type CounterView = {
    value: number,
    inc(delta: number): number,
};

class Counter {
    value: number;

    constructor(start: number) {
        this.value = start;
    }

    inc(delta: number): number {
        this.value = this.value + delta;
        return this.value;
    }
}

function hot(view: CounterView): number {
    return view.inc(1) + view.value;
}

let view = new Counter(0) as CounterView;
let last = 0;
let i = 0;
while (i < 64) {
    last = hot(view);
    i = i + 1;
}
return last + view.value;
"#;

    let telemetry = expect_i32_runtime_jit(source, 192);
    assert_jit_was_active(telemetry, source);
}
