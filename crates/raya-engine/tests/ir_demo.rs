//! IR Demo - Shows how Raya source code translates to IR
//!
//! Run with: cargo test -p raya-compiler --test ir_demo -- --nocapture

use raya_engine::compiler::ir::PrettyPrint;
use raya_engine::compiler::lower::Lowerer;
use raya_engine::parser::{Parser, TypeContext};

fn compile_to_ir(source: &str) -> String {
    let parser = Parser::new(source).expect("lexer error");
    let (module, interner) = parser.parse().expect("parse error");
    let type_ctx = TypeContext::new();
    let mut lowerer = Lowerer::new(&type_ctx, &interner);
    let ir = lowerer.lower_module(&module);
    ir.pretty_print()
}

#[test]
fn demo_simple_function() {
    println!("\n{}", "=".repeat(60));
    println!("DEMO 1: Simple Add Function");
    println!("{}", "=".repeat(60));

    let source = r#"
function add(a: number, b: number): number {
    return a + b;
}
"#;

    println!("\n--- Source ---");
    println!("{}", source);
    println!("\n--- IR ---");
    println!("{}", compile_to_ir(source));
}

#[test]
fn demo_conditionals() {
    println!("\n{}", "=".repeat(60));
    println!("DEMO 2: Conditional (max function)");
    println!("{}", "=".repeat(60));

    let source = r#"
function max(a: number, b: number): number {
    if (a > b) {
        return a;
    } else {
        return b;
    }
}
"#;

    println!("\n--- Source ---");
    println!("{}", source);
    println!("\n--- IR ---");
    println!("{}", compile_to_ir(source));
}

#[test]
fn demo_while_loop() {
    println!("\n{}", "=".repeat(60));
    println!("DEMO 3: While Loop (sum function)");
    println!("{}", "=".repeat(60));

    let source = r#"
function sum(n: number): number {
    let total = 0;
    let i = 0;
    while (i <= n) {
        total = total + i;
        i = i + 1;
    }
    return total;
}
"#;

    println!("\n--- Source ---");
    println!("{}", source);
    println!("\n--- IR ---");
    println!("{}", compile_to_ir(source));
}

#[test]
fn demo_for_loop() {
    println!("\n{}", "=".repeat(60));
    println!("DEMO 4: For Loop (factorial function)");
    println!("{}", "=".repeat(60));

    let source = r#"
function factorial(n: number): number {
    let result = 1;
    for (let i = 1; i <= n; i = i + 1) {
        result = result * i;
    }
    return result;
}
"#;

    println!("\n--- Source ---");
    println!("{}", source);
    println!("\n--- IR ---");
    println!("{}", compile_to_ir(source));
}

#[test]
fn demo_variable_declarations() {
    println!("\n{}", "=".repeat(60));
    println!("DEMO 5: Variable Declarations");
    println!("{}", "=".repeat(60));

    let source = r#"
let x = 42;
let y = x + 10;
let name = "hello";
let flag = true;
"#;

    println!("\n--- Source ---");
    println!("{}", source);
    println!("\n--- IR ---");
    println!("{}", compile_to_ir(source));
}

#[test]
fn demo_nested_expressions() {
    println!("\n{}", "=".repeat(60));
    println!("DEMO 6: Nested Expressions");
    println!("{}", "=".repeat(60));

    let source = r#"
function calc(a: number, b: number, c: number): number {
    return (a + b) * c - (a / 2);
}
"#;

    println!("\n--- Source ---");
    println!("{}", source);
    println!("\n--- IR ---");
    println!("{}", compile_to_ir(source));
}

#[test]
fn demo_ternary() {
    println!("\n{}", "=".repeat(60));
    println!("DEMO 7: Ternary Expression");
    println!("{}", "=".repeat(60));

    let source = r#"
function abs(n: number): number {
    return n >= 0 ? n : -n;
}
"#;

    println!("\n--- Source ---");
    println!("{}", source);
    println!("\n--- IR ---");
    println!("{}", compile_to_ir(source));
}

#[test]
fn demo_do_while() {
    println!("\n{}", "=".repeat(60));
    println!("DEMO 8: Do-While Loop");
    println!("{}", "=".repeat(60));

    let source = r#"
function countDown(n: number): number {
    let count = n;
    do {
        count = count - 1;
    } while (count > 0);
    return count;
}
"#;

    println!("\n--- Source ---");
    println!("{}", source);
    println!("\n--- IR ---");
    println!("{}", compile_to_ir(source));
}
