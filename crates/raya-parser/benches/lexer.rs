use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use raya_parser::Lexer;

fn bench_keywords(c: &mut Criterion) {
    let source = "function async await class interface type const let if else for while return";

    c.bench_function("lex_keywords", |b| {
        b.iter(|| {
            let lexer = Lexer::new(black_box(source));
            lexer.tokenize().unwrap()
        });
    });
}

fn bench_numbers(c: &mut Criterion) {
    let mut group = c.benchmark_group("numbers");

    let integers = "42 123 0 999 1_000_000";
    group.bench_with_input(
        BenchmarkId::new("integers", "simple"),
        &integers,
        |b, source| {
            b.iter(|| {
                let lexer = Lexer::new(black_box(source));
                lexer.tokenize().unwrap()
            });
        },
    );

    let hex = "0xFF 0x1234 0xDEADBEEF 0xFF_FF";
    group.bench_with_input(
        BenchmarkId::new("hex", "various"),
        &hex,
        |b, source| {
            b.iter(|| {
                let lexer = Lexer::new(black_box(source));
                lexer.tokenize().unwrap()
            });
        },
    );

    let floats = "3.14 2.718 1.414 0.5 123.456e10 1.23e-5";
    group.bench_with_input(
        BenchmarkId::new("floats", "various"),
        &floats,
        |b, source| {
            b.iter(|| {
                let lexer = Lexer::new(black_box(source));
                lexer.tokenize().unwrap()
            });
        },
    );

    group.finish();
}

fn bench_strings(c: &mut Criterion) {
    let mut group = c.benchmark_group("strings");

    let simple = r#""hello" "world" "test""#;
    group.bench_with_input(
        BenchmarkId::new("simple", "3 strings"),
        &simple,
        |b, source| {
            b.iter(|| {
                let lexer = Lexer::new(black_box(source));
                lexer.tokenize().unwrap()
            });
        },
    );

    let escapes = r#""line1\nline2" "tab\there" "quote\"test""#;
    group.bench_with_input(
        BenchmarkId::new("escapes", "basic"),
        &escapes,
        |b, source| {
            b.iter(|| {
                let lexer = Lexer::new(black_box(source));
                lexer.tokenize().unwrap()
            });
        },
    );

    let unicode = r#""\u0048\u0065\u006C\u006C\u006F" "\u{1F600}" "\u4F60\u597D""#;
    group.bench_with_input(
        BenchmarkId::new("unicode", "various"),
        &unicode,
        |b, source| {
            b.iter(|| {
                let lexer = Lexer::new(black_box(source));
                lexer.tokenize().unwrap()
            });
        },
    );

    group.finish();
}

fn bench_template_literals(c: &mut Criterion) {
    let mut group = c.benchmark_group("templates");

    let simple = r#"`Hello, World!`"#;
    group.bench_with_input(
        BenchmarkId::new("simple", "no expressions"),
        &simple,
        |b, source| {
            b.iter(|| {
                let lexer = Lexer::new(black_box(source));
                lexer.tokenize().unwrap()
            });
        },
    );

    let expressions = r#"`Hello, ${name}!` `${a} + ${b} = ${a + b}`"#;
    group.bench_with_input(
        BenchmarkId::new("expressions", "multiple"),
        &expressions,
        |b, source| {
            b.iter(|| {
                let lexer = Lexer::new(black_box(source));
                lexer.tokenize().unwrap()
            });
        },
    );

    let complex = r#"`Result: ${{ x: 42 }.x} and ${items.length * price}`"#;
    group.bench_with_input(
        BenchmarkId::new("complex", "nested and properties"),
        &complex,
        |b, source| {
            b.iter(|| {
                let lexer = Lexer::new(black_box(source));
                lexer.tokenize().unwrap()
            });
        },
    );

    group.finish();
}

fn bench_operators(c: &mut Criterion) {
    let source = "+ - * / % ** == === != !== < > <= >= && || ! ~ & | ^ << >> >>> ++ -- += -= *= /= %= &= |= ^= <<= >>= >>>= ? ?? ?. => . : ( ) { } [ ] ; ,";

    c.bench_function("lex_operators", |b| {
        b.iter(|| {
            let lexer = Lexer::new(black_box(source));
            lexer.tokenize().unwrap()
        });
    });
}

fn bench_real_code(c: &mut Criterion) {
    let mut group = c.benchmark_group("real_code");

    let function_def = r#"
        async function fetchUser(id: number): Promise<User> {
            const response = await fetch(`/api/users/${id}`);
            if (!response.ok) {
                throw new Error("Failed to fetch user");
            }
            return await response.json();
        }
    "#;

    group.throughput(Throughput::Bytes(function_def.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("async_function", "with_template"),
        &function_def,
        |b, source| {
            b.iter(|| {
                let lexer = Lexer::new(black_box(source));
                lexer.tokenize().unwrap()
            });
        },
    );

    let class_def = r#"
        class Calculator {
            private result: number = 0;

            add(x: number): this {
                this.result += x;
                return this;
            }

            multiply(x: number): this {
                this.result *= x;
                return this;
            }

            getResult(): number {
                return this.result;
            }
        }
    "#;

    group.throughput(Throughput::Bytes(class_def.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("class", "with_methods"),
        &class_def,
        |b, source| {
            b.iter(|| {
                let lexer = Lexer::new(black_box(source));
                lexer.tokenize().unwrap()
            });
        },
    );

    let switch_statement = r#"
        switch (typeof value) {
            case "number":
                return value + 1;
            case "string":
                return value.toUpperCase();
            case "boolean":
                return !value;
            default:
                return null;
        }
    "#;

    group.throughput(Throughput::Bytes(switch_statement.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("switch", "typeof_discrimination"),
        &switch_statement,
        |b, source| {
            b.iter(|| {
                let lexer = Lexer::new(black_box(source));
                lexer.tokenize().unwrap()
            });
        },
    );

    group.finish();
}

fn bench_large_file(c: &mut Criterion) {
    let mut group = c.benchmark_group("large_file");

    // Generate a realistic large file
    let mut source = String::new();
    for i in 0..100 {
        source.push_str(&format!(r#"
            function process{i}(data: any): number {{
                const result = data.value * 2;
                if (result > 1000) {{
                    return result;
                }}
                return 0;
            }}
        "#));
    }

    group.throughput(Throughput::Bytes(source.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("100_functions", format!("{} bytes", source.len())),
        &source,
        |b, source| {
            b.iter(|| {
                let lexer = Lexer::new(black_box(source));
                lexer.tokenize().unwrap()
            });
        },
    );

    group.finish();
}

criterion_group!(
    benches,
    bench_keywords,
    bench_numbers,
    bench_strings,
    bench_template_literals,
    bench_operators,
    bench_real_code,
    bench_large_file
);

criterion_main!(benches);
