//! Closure capture ordering tests
//!
//! These tests verify that captured variables maintain correct identity and ordering,
//! especially in patterns where method receivers and arguments are both captured.
//! The bug pattern: `async { obj.method(arg) }` inside a class method swaps captures.

use super::harness::*;

// ============================================================================
// Category 1: Basic Capture Sanity
// ============================================================================

#[test]
fn test_capture_single_variable() {
    expect_i32(
        "let x = 42;
         let f = (): number => x;
         return f();",
        42,
    );
}

#[test]
fn test_capture_two_variables_order() {
    // a - b ≠ b - a, so swapped captures produce wrong result
    expect_i32(
        "let a = 10;
         let b = 3;
         let f = (): number => a - b;
         return f();",
        7,
    );
}

#[test]
fn test_capture_two_variables_passed_to_function() {
    expect_i32(
        "function sub(x: number, y: number): number { return x - y; }
         let a = 10;
         let b = 3;
         let f = (): number => sub(a, b);
         return f();",
        7,
    );
}

// ============================================================================
// Category 2: Capture Ordering with Method Calls
// ============================================================================

#[test]
fn test_capture_method_call_on_captured_object() {
    expect_i32(
        "class Box {
             val: number;
             constructor(v: number) { this.val = v; }
             get(): number { return this.val; }
         }
         let b = new Box(42);
         let f = (): number => b.get();
         return f();",
        42,
    );
}

#[test]
fn test_capture_method_call_with_captured_arg() {
    // obj.add(val): if captures swap, result = 5 + 100 = 105 vs 100 + 5 = 105
    // Use asymmetric op: base * 10 + x to distinguish
    expect_i32(
        "class Processor {
             base: number;
             constructor(b: number) { this.base = b; }
             compute(x: number): number { return this.base * 10 + x; }
         }
         let obj = new Processor(7);
         let val = 3;
         let f = (): number => obj.compute(val);
         return f();",
        73,
    );
}

#[test]
fn test_capture_two_objects_method_call() {
    // Method on first object, method on second as arg
    expect_i32(
        "class Source {
             id: number;
             constructor(i: number) { this.id = i; }
             combine(other: number): number { return this.id * 10 + other; }
         }
         class Target {
             id: number;
             constructor(i: number) { this.id = i; }
             getId(): number { return this.id; }
         }
         let src = new Source(7);
         let tgt = new Target(3);
         let f = (): number => src.combine(tgt.getId());
         return f();",
        73,
    );
}

// ============================================================================
// Category 3: Captures Inside Class Methods
// ============================================================================

#[test]
fn test_method_arrow_captures_this() {
    expect_i32(
        "class Foo {
             val: number;
             constructor(v: number) { this.val = v; }
             getViaArrow(): number {
                 let f = (): number => this.val;
                 return f();
             }
         }
         let foo = new Foo(42);
         return foo.getViaArrow();",
        42,
    );
}

#[test]
fn test_method_arrow_captures_this_and_param() {
    // this.base * 10 + x: if this/x swap, totally different result
    expect_i32(
        "class Calc {
             base: number;
             constructor(b: number) { this.base = b; }
             compute(x: number): number {
                 let f = (): number => this.base * 10 + x;
                 return f();
             }
         }
         let c = new Calc(7);
         return c.compute(3);",
        73,
    );
}

#[test]
fn test_method_arrow_captures_local_copy_of_this() {
    // let self_ref = this; arrow captures self_ref (pipeThrough pattern)
    expect_i32(
        "class Holder {
             val: number;
             constructor(v: number) { this.val = v; }
             getViaCopy(): number {
                 let self_ref = this;
                 let f = (): number => self_ref.val;
                 return f();
             }
         }
         let h = new Holder(42);
         return h.getViaCopy();",
        42,
    );
}

#[test]
fn test_method_arrow_captures_self_and_local_method_call() {
    // Exact pipeThrough pattern (synchronous):
    // let w = this.worker; let v = p.getVal(); arrow calls w.process(v)
    expect_i32(
        "class Worker {
             id: number;
             constructor(i: number) { this.id = i; }
             process(x: number): number { return this.id * 10 + x; }
         }
         class Provider {
             val: number;
             constructor(v: number) { this.val = v; }
             getVal(): number { return this.val; }
         }
         class Coordinator {
             worker: Worker;
             constructor(w: Worker) { this.worker = w; }
             run(p: Provider): number {
                 let w = this.worker;
                 let v = p.getVal();
                 let f = (): number => w.process(v);
                 return f();
             }
         }
         let worker = new Worker(7);
         let provider = new Provider(3);
         let coord = new Coordinator(worker);
         return coord.run(provider);",
        73,
    );
}

// ============================================================================
// Category 4: Async Block Captures
// ============================================================================

#[test]
fn test_async_block_capture_single() {
    expect_i32_with_builtins(
        "let x = 42;
         let t = async {
             return x;
         };
         return await t;",
        42,
    );
}

#[test]
fn test_async_block_capture_two_variables() {
    expect_i32_with_builtins(
        "let a = 10;
         let b = 3;
         let t = async {
             return a - b;
         };
         return await t;",
        7,
    );
}

#[test]
fn test_async_block_capture_function_call() {
    expect_i32_with_builtins(
        "function sub(x: number, y: number): number { return x - y; }
         let a = 10;
         let b = 3;
         let t = async {
             return sub(a, b);
         };
         return await t;",
        7,
    );
}

#[test]
fn test_async_block_capture_method_call() {
    // Receiver and arg are both captured — ordering matters
    expect_i32_with_builtins(
        "class Processor {
             base: number;
             constructor(b: number) { this.base = b; }
             compute(x: number): number { return this.base * 10 + x; }
         }
         let obj = new Processor(7);
         let val = 3;
         let t = async {
             return obj.compute(val);
         };
         return await t;",
        73,
    );
}

// ============================================================================
// Category 5: Async Blocks Inside Class Methods (the bug pattern)
// ============================================================================

#[test]
fn test_async_block_in_method_captures_this() {
    expect_i32_with_builtins(
        "class Foo {
             val: number;
             constructor(v: number) { this.val = v; }
             asyncGet(): number {
                 let t = async {
                     return this.val;
                 };
                 return await t;
             }
         }
         let foo = new Foo(42);
         return foo.asyncGet();",
        42,
    );
}

#[test]
fn test_async_block_in_method_captures_self_copy() {
    expect_i32_with_builtins(
        "class Foo {
             val: number;
             constructor(v: number) { this.val = v; }
             asyncGet(): number {
                 let me = this;
                 let t = async {
                     return me.val;
                 };
                 return await t;
             }
         }
         let foo = new Foo(42);
         return foo.asyncGet();",
        42,
    );
}

#[test]
fn test_async_block_in_method_captures_self_and_param() {
    expect_i32_with_builtins(
        "class Calc {
             base: number;
             constructor(b: number) { this.base = b; }
             asyncCompute(x: number): number {
                 let me = this;
                 let t = async {
                     return me.base * 10 + x;
                 };
                 return await t;
             }
         }
         let c = new Calc(7);
         return c.asyncCompute(3);",
        73,
    );
}

#[test]
fn test_async_block_in_method_method_call_on_capture() {
    // EXACT pipeThrough pattern: method body creates locals from this/param,
    // async block calls method on captured local with other captured local as arg
    expect_i32_with_builtins(
        "class Worker {
             id: number;
             constructor(i: number) { this.id = i; }
             process(x: number): number { return this.id * 10 + x; }
         }
         class Provider {
             val: number;
             constructor(v: number) { this.val = v; }
             getVal(): number { return this.val; }
         }
         class Coordinator {
             worker: Worker;
             constructor(w: Worker) { this.worker = w; }
             asyncRun(p: Provider): number {
                 let w = this.worker;
                 let v = p.getVal();
                 let t = async {
                     return w.process(v);
                 };
                 return await t;
             }
         }
         let worker = new Worker(7);
         let provider = new Provider(3);
         let coord = new Coordinator(worker);
         return coord.asyncRun(provider);",
        73,
    );
}

// ============================================================================
// Category 6: Exact pipeThrough Reproduction (fire-and-forget + Channel)
// ============================================================================

#[test]
fn test_pipethrough_pattern_fire_and_forget() {
    // Fire-and-forget async block inside method, communicates via Channel
    expect_i32_with_builtins(
        "class Source {
             id: number;
             constructor(i: number) { this.id = i; }
             forward(ch: Channel<number>): void {
                 ch.send(this.id);
                 ch.close();
             }
         }
         class Pipe {
             src: Source;
             ch: Channel<number>;
             constructor(s: Source) {
                 this.src = s;
                 this.ch = new Channel<number>(1);
             }
             run(): number {
                 let s = this.src;
                 let c = this.ch;
                 async {
                     s.forward(c);
                 };
                 return this.ch.receive();
             }
         }
         let src = new Source(42);
         let pipe = new Pipe(src);
         return pipe.run();",
        42,
    );
}

#[test]
fn test_pipethrough_pattern_with_param_getter() {
    // Method param has a getter, result captured, used in async block
    expect_i32_with_builtins(
        "class Reader {
             val: number;
             constructor(v: number) { this.val = v; }
             forward(ch: Channel<number>): void {
                 ch.send(this.val);
                 ch.close();
             }
         }
         class Transform {
             outCh: Channel<number>;
             constructor() { this.outCh = new Channel<number>(1); }
             getOutput(): Channel<number> { return this.outCh; }
         }
         class Pipeline {
             reader: Reader;
             constructor(r: Reader) { this.reader = r; }
             pipe(transform: Transform): Channel<number> {
                 let src = this.reader;
                 let out = transform.getOutput();
                 async {
                     src.forward(out);
                 };
                 return transform.getOutput();
             }
         }
         let reader = new Reader(99);
         let transform = new Transform();
         let pipeline = new Pipeline(reader);
         let ch = pipeline.pipe(transform);
         return ch.receive();",
        99,
    );
}
