//! Comprehensive IR Generation Tests
//!
//! Tests IR generation for all Raya language features.
//! Run with: cargo test -p raya-compiler --test ir_comprehensive -- --nocapture

use raya_engine::compiler::ir::{IrModule, PrettyPrint};
use raya_engine::compiler::lower::Lowerer;
use raya_engine::parser::{Parser, TypeContext};

fn lower(source: &str) -> IrModule {
    let parser = Parser::new(source).expect("lexer error");
    let (module, interner) = parser.parse().expect("parse error");
    let type_ctx = TypeContext::new();
    let mut lowerer = Lowerer::new(&type_ctx, &interner);
    lowerer.lower_module(&module)
}

#[allow(dead_code)]
fn lower_and_print(source: &str) -> String {
    lower(source).pretty_print()
}

// =============================================================================
// LITERALS
// =============================================================================

mod literals {
    use super::*;

    #[test]
    fn test_integer_literal() {
        let ir = lower("let x = 42;");
        assert_eq!(ir.function_count(), 1);
        let output = ir.pretty_print();
        assert!(output.contains("42"));
    }

    #[test]
    fn test_negative_integer() {
        let ir = lower("let x = -42;");
        let output = ir.pretty_print();
        assert!(output.contains("-"));
    }

    #[test]
    fn test_float_literal() {
        let ir = lower("let x = 3.14;");
        let output = ir.pretty_print();
        assert!(output.contains("3.14"));
    }

    #[test]
    fn test_string_literal() {
        let ir = lower("let x = \"hello\";");
        let output = ir.pretty_print();
        assert!(output.contains("\"hello\""));
    }

    #[test]
    fn test_boolean_true() {
        let ir = lower("let x = true;");
        let output = ir.pretty_print();
        assert!(output.contains("true"));
    }

    #[test]
    fn test_boolean_false() {
        let ir = lower("let x = false;");
        let output = ir.pretty_print();
        assert!(output.contains("false"));
    }

    #[test]
    fn test_null_literal() {
        let ir = lower("let x = null;");
        let output = ir.pretty_print();
        assert!(output.contains("null"));
    }
}

// =============================================================================
// VARIABLES
// =============================================================================

mod variables {
    use super::*;

    #[test]
    fn test_let_declaration() {
        let ir = lower("let x = 1;");
        let output = ir.pretty_print();
        assert!(output.contains("store_local 0"));
    }

    #[test]
    fn test_const_declaration() {
        // Const declarations with literal initializers are folded at compile time
        // and don't emit store instructions
        let ir = lower("const x = 1;");
        let output = ir.pretty_print();
        // With constant folding, const x = 1 doesn't emit store_local
        // Verify IR is generated but no store for the const
        assert!(!output.contains("store_local 0"), "const literals should be folded, not stored");
    }

    #[test]
    fn test_typed_declaration() {
        let ir = lower("let x: number = 1;");
        assert_eq!(ir.function_count(), 1);
    }

    #[test]
    fn test_variable_reference() {
        let ir = lower("let x = 1; let y = x;");
        let output = ir.pretty_print();
        assert!(output.contains("load_local 0"));
    }

    #[test]
    fn test_multiple_declarations() {
        let ir = lower("let a = 1; let b = 2; let c = 3;");
        let output = ir.pretty_print();
        assert!(output.contains("store_local 0"));
        assert!(output.contains("store_local 1"));
        assert!(output.contains("store_local 2"));
    }
}

// =============================================================================
// BINARY OPERATORS
// =============================================================================

mod binary_operators {
    use super::*;

    #[test]
    fn test_addition() {
        let ir = lower("let x = 1 + 2;");
        let output = ir.pretty_print();
        assert!(output.contains("+"));
    }

    #[test]
    fn test_subtraction() {
        let ir = lower("let x = 5 - 3;");
        let output = ir.pretty_print();
        assert!(output.contains("-"));
    }

    #[test]
    fn test_multiplication() {
        let ir = lower("let x = 4 * 5;");
        let output = ir.pretty_print();
        assert!(output.contains("*"));
    }

    #[test]
    fn test_division() {
        let ir = lower("let x = 10 / 2;");
        let output = ir.pretty_print();
        assert!(output.contains("/"));
    }

    #[test]
    fn test_modulo() {
        let ir = lower("let x = 10 % 3;");
        let output = ir.pretty_print();
        assert!(output.contains("%"));
    }

    #[test]
    fn test_less_than() {
        let ir = lower("let x = 1 < 2;");
        let output = ir.pretty_print();
        assert!(output.contains("<"));
    }

    #[test]
    fn test_less_equal() {
        let ir = lower("let x = 1 <= 2;");
        let output = ir.pretty_print();
        assert!(output.contains("<="));
    }

    #[test]
    fn test_greater_than() {
        let ir = lower("let x = 2 > 1;");
        let output = ir.pretty_print();
        assert!(output.contains(">"));
    }

    #[test]
    fn test_greater_equal() {
        let ir = lower("let x = 2 >= 1;");
        let output = ir.pretty_print();
        assert!(output.contains(">="));
    }

    #[test]
    fn test_equal() {
        let ir = lower("let x = 1 == 1;");
        let output = ir.pretty_print();
        assert!(output.contains("=="));
    }

    #[test]
    fn test_not_equal() {
        let ir = lower("let x = 1 != 2;");
        let output = ir.pretty_print();
        assert!(output.contains("!="));
    }

    #[test]
    fn test_strict_equal() {
        let ir = lower("let x = 1 === 1;");
        let output = ir.pretty_print();
        assert!(output.contains("=="));
    }

    #[test]
    fn test_strict_not_equal() {
        let ir = lower("let x = 1 !== 2;");
        let output = ir.pretty_print();
        assert!(output.contains("!="));
    }

    #[test]
    fn test_bitwise_and() {
        let ir = lower("let x = 5 & 3;");
        let output = ir.pretty_print();
        assert!(output.contains("&"));
    }

    #[test]
    fn test_bitwise_or() {
        let ir = lower("let x = 5 | 3;");
        let output = ir.pretty_print();
        assert!(output.contains("|"));
    }

    #[test]
    fn test_bitwise_xor() {
        let ir = lower("let x = 5 ^ 3;");
        let output = ir.pretty_print();
        assert!(output.contains("^"));
    }

    #[test]
    fn test_left_shift() {
        let ir = lower("let x = 1 << 2;");
        let output = ir.pretty_print();
        assert!(output.contains("<<"));
    }

    #[test]
    fn test_right_shift() {
        let ir = lower("let x = 8 >> 2;");
        let output = ir.pretty_print();
        assert!(output.contains(">>"));
    }

    #[test]
    fn test_unsigned_right_shift() {
        let ir = lower("let x = 8 >>> 2;");
        let output = ir.pretty_print();
        assert!(output.contains(">>>"));
    }

    #[test]
    fn test_chained_arithmetic() {
        let ir = lower("let x = 1 + 2 + 3;");
        let output = ir.pretty_print();
        // Should have two addition operations
        assert!(output.matches("+").count() >= 2);
    }

    #[test]
    fn test_mixed_operators() {
        let ir = lower("let x = 1 + 2 * 3;");
        let output = ir.pretty_print();
        assert!(output.contains("+"));
        assert!(output.contains("*"));
    }

    #[test]
    fn test_parenthesized_expression() {
        let ir = lower("let x = (1 + 2) * 3;");
        let output = ir.pretty_print();
        assert!(output.contains("+"));
        assert!(output.contains("*"));
    }
}

// =============================================================================
// UNARY OPERATORS
// =============================================================================

mod unary_operators {
    use super::*;

    #[test]
    fn test_negation() {
        let ir = lower("let x = 5; let y = -x;");
        let output = ir.pretty_print();
        assert!(output.contains("-"));
    }

    #[test]
    fn test_logical_not() {
        let ir = lower("let x = true; let y = !x;");
        let output = ir.pretty_print();
        assert!(output.contains("!"));
    }

    #[test]
    fn test_bitwise_not() {
        let ir = lower("let x = 5; let y = ~x;");
        let output = ir.pretty_print();
        assert!(output.contains("~"));
    }
}

// =============================================================================
// LOGICAL OPERATORS (SHORT-CIRCUIT)
// =============================================================================

mod logical_operators {
    use super::*;

    #[test]
    fn test_logical_and() {
        let ir = lower("let x = true && false;");
        let output = ir.pretty_print();
        // Logical AND should create branching control flow
        assert!(output.contains("branch") || output.contains("phi"));
    }

    #[test]
    fn test_logical_or() {
        let ir = lower("let x = true || false;");
        let output = ir.pretty_print();
        // Logical OR should create branching control flow
        assert!(output.contains("branch") || output.contains("phi"));
    }

    #[test]
    fn test_chained_logical() {
        let ir = lower("let a = true; let b = false; let c = a && b || a;");
        assert!(ir.function_count() >= 1);
    }
}

// =============================================================================
// CONTROL FLOW - IF STATEMENTS
// =============================================================================

mod if_statements {
    use super::*;

    #[test]
    fn test_simple_if() {
        let ir = lower("if (true) { let x = 1; }");
        let output = ir.pretty_print();
        assert!(output.contains("branch"));
    }

    #[test]
    fn test_if_else() {
        let ir = lower("if (true) { let x = 1; } else { let x = 2; }");
        let output = ir.pretty_print();
        assert!(output.contains("branch"));
        // Should have at least 3 blocks: entry, then, else
        assert!(output.contains("bb0"));
        assert!(output.contains("bb1"));
        assert!(output.contains("bb2"));
    }

    #[test]
    fn test_if_else_if() {
        let source = r#"
            let x = 1;
            if (x == 1) {
                let y = 1;
            } else if (x == 2) {
                let y = 2;
            } else {
                let y = 3;
            }
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        assert!(output.contains("branch"));
    }

    #[test]
    fn test_nested_if() {
        let source = r#"
            if (true) {
                if (false) {
                    let x = 1;
                }
            }
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        // Multiple branch instructions
        assert!(output.matches("branch").count() >= 2);
    }

    #[test]
    fn test_if_with_return() {
        let source = r#"
            function foo(x: number): number {
                if (x > 0) {
                    return x;
                }
                return 0;
            }
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        assert!(output.contains("return"));
    }
}

// =============================================================================
// CONTROL FLOW - WHILE LOOPS
// =============================================================================

mod while_loops {
    use super::*;

    #[test]
    fn test_simple_while() {
        let source = r#"
            let x = 0;
            while (x < 10) {
                x = x + 1;
            }
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        assert!(output.contains("while.header"));
        assert!(output.contains("while.body"));
        assert!(output.contains("while.exit"));
    }

    #[test]
    fn test_while_with_break() {
        let source = r#"
            let x = 0;
            while (true) {
                x = x + 1;
                break;
            }
        "#;
        let ir = lower(source);
        assert!(ir.function_count() >= 1);
    }

    #[test]
    fn test_while_with_continue() {
        let source = r#"
            let x = 0;
            while (x < 10) {
                x = x + 1;
                continue;
            }
        "#;
        let ir = lower(source);
        assert!(ir.function_count() >= 1);
    }

    #[test]
    fn test_nested_while() {
        let source = r#"
            let i = 0;
            while (i < 3) {
                let j = 0;
                while (j < 3) {
                    j = j + 1;
                }
                i = i + 1;
            }
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        // Should have multiple while structures
        assert!(output.matches("while.header").count() >= 2);
    }
}

// =============================================================================
// CONTROL FLOW - DO-WHILE LOOPS
// =============================================================================

mod do_while_loops {
    use super::*;

    #[test]
    fn test_simple_do_while() {
        let source = r#"
            let x = 0;
            do {
                x = x + 1;
            } while (x < 10);
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        assert!(output.contains("dowhile.body"));
        assert!(output.contains("dowhile.cond"));
        assert!(output.contains("dowhile.exit"));
    }

    #[test]
    fn test_do_while_executes_once() {
        let source = r#"
            let x = 100;
            do {
                x = x + 1;
            } while (x < 10);
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        // Body block comes before condition check
        assert!(output.contains("dowhile.body"));
    }
}

// =============================================================================
// CONTROL FLOW - FOR LOOPS
// =============================================================================

mod for_loops {
    use super::*;

    #[test]
    fn test_simple_for() {
        let source = r#"
            for (let i = 0; i < 10; i = i + 1) {
                let x = i;
            }
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        assert!(output.contains("for.header"));
        assert!(output.contains("for.body"));
        assert!(output.contains("for.update"));
        assert!(output.contains("for.exit"));
    }

    #[test]
    fn test_for_without_init() {
        let source = r#"
            let i = 0;
            for (; i < 10; i = i + 1) {
                let x = i;
            }
        "#;
        let ir = lower(source);
        assert!(ir.function_count() >= 1);
    }

    #[test]
    fn test_for_without_condition() {
        let source = r#"
            for (let i = 0; ; i = i + 1) {
                break;
            }
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        // Without condition, should jump directly to body
        assert!(output.contains("for.header"));
    }

    #[test]
    fn test_for_without_update() {
        let source = r#"
            for (let i = 0; i < 10;) {
                i = i + 1;
            }
        "#;
        let ir = lower(source);
        assert!(ir.function_count() >= 1);
    }

    #[test]
    fn test_for_with_expression_init() {
        let source = r#"
            let i = 0;
            for (i = 0; i < 10; i = i + 1) {
                let x = i;
            }
        "#;
        let ir = lower(source);
        assert!(ir.function_count() >= 1);
    }

    #[test]
    fn test_nested_for() {
        let source = r#"
            for (let i = 0; i < 3; i = i + 1) {
                for (let j = 0; j < 3; j = j + 1) {
                    let sum = i + j;
                }
            }
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        assert!(output.matches("for.header").count() >= 2);
    }
}

// =============================================================================
// CONTROL FLOW - SWITCH
// =============================================================================

mod switch_statements {
    use super::*;

    #[test]
    fn test_simple_switch() {
        let source = r#"
            let x = 1;
            switch (x) {
                case 1:
                    let y = 1;
                    break;
                case 2:
                    let y = 2;
                    break;
            }
        "#;
        let ir = lower(source);
        assert!(ir.function_count() >= 1);
    }

    #[test]
    fn test_switch_with_default() {
        let source = r#"
            let x = 3;
            switch (x) {
                case 1:
                    let y = 1;
                    break;
                default:
                    let y = 0;
            }
        "#;
        let ir = lower(source);
        assert!(ir.function_count() >= 1);
    }
}

// =============================================================================
// FUNCTIONS
// =============================================================================

mod functions {
    use super::*;

    #[test]
    fn test_simple_function() {
        let source = r#"
            function foo() {
                let x = 1;
            }
        "#;
        let ir = lower(source);
        assert!(ir.get_function_by_name("foo").is_some());
    }

    #[test]
    fn test_function_with_params() {
        let source = r#"
            function add(a: number, b: number): number {
                return a + b;
            }
        "#;
        let ir = lower(source);
        let func = ir.get_function_by_name("add").expect("function not found");
        assert_eq!(func.params.len(), 2);
    }

    #[test]
    fn test_function_with_return() {
        let source = r#"
            function foo(): number {
                return 42;
            }
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        assert!(output.contains("return"));
    }

    #[test]
    fn test_function_implicit_return() {
        let source = r#"
            function foo() {
                let x = 1;
            }
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        // Should have implicit return at end
        assert!(output.contains("return"));
    }

    #[test]
    fn test_multiple_functions() {
        let source = r#"
            function foo() { let x = 1; }
            function bar() { let y = 2; }
            function baz() { let z = 3; }
        "#;
        let ir = lower(source);
        assert!(ir.get_function_by_name("foo").is_some());
        assert!(ir.get_function_by_name("bar").is_some());
        assert!(ir.get_function_by_name("baz").is_some());
    }

    #[test]
    fn test_function_call() {
        let source = r#"
            function add(a: number, b: number): number {
                return a + b;
            }
            let result = add(1, 2);
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        assert!(output.contains("call"));
    }

    #[test]
    fn test_recursive_function() {
        let source = r#"
            function factorial(n: number): number {
                if (n <= 1) {
                    return 1;
                }
                return n * factorial(n - 1);
            }
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        assert!(output.contains("call"));
    }
}

// =============================================================================
// EXPRESSIONS
// =============================================================================

mod expressions {
    use super::*;

    #[test]
    fn test_ternary() {
        let source = r#"
            let x = true ? 1 : 2;
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        assert!(output.contains("branch"));
        assert!(output.contains("phi"));
    }

    #[test]
    fn test_nested_ternary() {
        let source = r#"
            let x = 1;
            let y = x == 1 ? 1 : x == 2 ? 2 : 3;
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        assert!(output.matches("branch").count() >= 2);
    }

    #[test]
    fn test_typeof() {
        let source = r#"
            let x = 42;
            let t = typeof x;
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        assert!(output.contains("typeof"));
    }

    #[test]
    fn test_assignment_expression() {
        let source = r#"
            let x = 1;
            x = 2;
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        assert!(output.contains("store_local"));
    }

    #[test]
    fn test_compound_assignment() {
        // Compound assignment (x += 2) is equivalent to (x = x + 2)
        // The parser/lowerer may handle this differently
        let source = r#"
            let x = 1;
            x += 2;
        "#;
        let ir = lower(source);
        // Just verify it generates valid IR
        assert!(ir.function_count() >= 1);
    }
}

// =============================================================================
// ARRAYS
// =============================================================================

mod arrays {
    use super::*;

    #[test]
    fn test_empty_array() {
        let ir = lower("let arr: number[] = [];");
        let output = ir.pretty_print();
        assert!(output.contains("array_literal"));
    }

    #[test]
    fn test_array_literal() {
        let ir = lower("let arr = [1, 2, 3];");
        let output = ir.pretty_print();
        assert!(output.contains("array_literal"));
    }

    #[test]
    fn test_array_index() {
        let source = r#"
            let arr = [1, 2, 3];
            let x = arr[0];
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        assert!(output.contains("load_elem"));
    }

    #[test]
    fn test_array_assignment() {
        let source = r#"
            let arr = [1, 2, 3];
            arr[0] = 10;
        "#;
        let ir = lower(source);
        // Store element should be generated
        assert!(ir.function_count() >= 1);
    }

    #[test]
    fn test_nested_array() {
        let ir = lower("let arr = [[1, 2], [3, 4]];");
        let output = ir.pretty_print();
        assert!(output.contains("array_literal"));
    }
}

// =============================================================================
// OBJECTS
// =============================================================================

mod objects {
    use super::*;

    #[test]
    fn test_empty_object() {
        let ir = lower("let obj = {};");
        let output = ir.pretty_print();
        assert!(output.contains("object_literal"));
    }

    #[test]
    fn test_object_literal() {
        let ir = lower("let obj = { x: 1, y: 2 };");
        let output = ir.pretty_print();
        assert!(output.contains("object_literal"));
    }

    #[test]
    fn test_object_property_access() {
        let source = r#"
            let obj = { x: 1 };
            let val = obj.x;
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        assert!(output.contains("load_field"));
    }

    #[test]
    fn test_nested_object() {
        let ir = lower("let obj = { inner: { x: 1 } };");
        let output = ir.pretty_print();
        assert!(output.contains("object_literal"));
    }
}

// =============================================================================
// CLASSES
// =============================================================================

mod classes {
    use super::*;

    #[test]
    fn test_empty_class() {
        let source = r#"
            class Foo {}
        "#;
        let ir = lower(source);
        assert!(ir.get_class_by_name("Foo").is_some());
    }

    #[test]
    fn test_class_with_fields() {
        let source = r#"
            class Point {
                x: number;
                y: number;
            }
        "#;
        let ir = lower(source);
        let class = ir.get_class_by_name("Point").expect("class not found");
        assert_eq!(class.field_count(), 2);
    }

    #[test]
    fn test_class_with_method() {
        let source = r#"
            class Counter {
                value: number;

                increment(): number {
                    return this.value + 1;
                }
            }
        "#;
        let ir = lower(source);
        assert!(ir.get_class_by_name("Counter").is_some());
    }

    #[test]
    fn test_new_expression() {
        let source = r#"
            class Foo {}
            let f = new Foo();
        "#;
        let ir = lower(source);
        // Just verify it parses and generates IR
        assert!(ir.get_class_by_name("Foo").is_some());
        assert!(ir.function_count() >= 1);
    }
}

// =============================================================================
// TRY/CATCH/THROW
// =============================================================================

mod exception_handling {
    use super::*;

    #[test]
    fn test_throw() {
        let source = r#"
            function fail() {
                throw "error";
            }
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        assert!(output.contains("throw"));
    }

    #[test]
    fn test_try_catch() {
        let source = r#"
            try {
                let x = 1;
            } catch (e) {
                let y = 2;
            }
        "#;
        let ir = lower(source);
        assert!(ir.function_count() >= 1);
    }

    #[test]
    fn test_try_finally() {
        let source = r#"
            try {
                let x = 1;
            } finally {
                let y = 2;
            }
        "#;
        let ir = lower(source);
        assert!(ir.function_count() >= 1);
    }

    #[test]
    fn test_try_catch_finally() {
        let source = r#"
            try {
                let x = 1;
            } catch (e) {
                let y = 2;
            } finally {
                let z = 3;
            }
        "#;
        let ir = lower(source);
        assert!(ir.function_count() >= 1);
    }
}

// =============================================================================
// MEMBER EXPRESSIONS
// =============================================================================

mod member_expressions {
    use super::*;

    #[test]
    fn test_dot_access() {
        let source = r#"
            let obj = { x: 1 };
            let val = obj.x;
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        assert!(output.contains("load_field"));
    }

    #[test]
    fn test_bracket_access() {
        let source = r#"
            let obj = { x: 1 };
            let val = obj["x"];
        "#;
        let ir = lower(source);
        assert!(ir.function_count() >= 1);
    }

    #[test]
    fn test_chained_access() {
        let source = r#"
            let obj = { inner: { x: 1 } };
            let val = obj.inner.x;
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        // Multiple load_field operations
        assert!(output.matches("load_field").count() >= 2);
    }
}

// =============================================================================
// METHOD CALLS
// =============================================================================

mod method_calls {
    use super::*;

    #[test]
    fn test_method_call() {
        let source = r#"
            let obj = { foo: 1 };
            obj.toString();
        "#;
        let ir = lower(source);
        assert!(ir.function_count() >= 1);
    }

    #[test]
    fn test_chained_method_calls() {
        let source = r#"
            let s = "hello";
            s.toUpperCase().toLowerCase();
        "#;
        let ir = lower(source);
        assert!(ir.function_count() >= 1);
    }
}

// =============================================================================
// BLOCK STATEMENTS
// =============================================================================

mod blocks {
    use super::*;

    #[test]
    fn test_block_in_if() {
        // Test block statements via if statements (the most common usage)
        let source = r#"
            if (true) {
                let x = 1;
            }
        "#;
        let ir = lower(source);
        assert!(ir.function_count() >= 1);
    }

    #[test]
    fn test_block_with_multiple_statements() {
        let source = r#"
            if (true) {
                let x = 1;
                let y = 2;
                let z = x + y;
            }
        "#;
        let ir = lower(source);
        assert!(ir.function_count() >= 1);
    }

    #[test]
    fn test_nested_blocks_via_if() {
        let source = r#"
            if (true) {
                let x = 1;
                if (true) {
                    let y = 2;
                    if (true) {
                        let z = 3;
                    }
                }
            }
        "#;
        let ir = lower(source);
        assert!(ir.function_count() >= 1);
    }

    #[test]
    fn test_function_body_block() {
        let source = r#"
            function foo() {
                let x = 1;
                let y = 2;
                let z = 3;
            }
        "#;
        let ir = lower(source);
        assert!(ir.get_function_by_name("foo").is_some());
    }
}

// =============================================================================
// COMPLEX EXPRESSIONS
// =============================================================================

mod complex_expressions {
    use super::*;

    #[test]
    fn test_deeply_nested_arithmetic() {
        let ir = lower("let x = ((1 + 2) * (3 - 4)) / ((5 + 6) * 7);");
        let output = ir.pretty_print();
        assert!(output.contains("+"));
        assert!(output.contains("-"));
        assert!(output.contains("*"));
        assert!(output.contains("/"));
    }

    #[test]
    fn test_complex_conditional() {
        let source = r#"
            function complex(a: number, b: number, c: number): number {
                if (a > b && b > c) {
                    return a;
                } else if (b > a && b > c) {
                    return b;
                } else {
                    return c;
                }
            }
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        assert!(output.contains("branch"));
    }

    #[test]
    fn test_mixed_control_flow() {
        let source = r#"
            function test(n: number): number {
                let sum = 0;
                for (let i = 0; i < n; i = i + 1) {
                    if (i % 2 == 0) {
                        sum = sum + i;
                    } else {
                        sum = sum - i;
                    }
                }
                return sum;
            }
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        assert!(output.contains("for.header"));
        assert!(output.contains("branch"));
    }
}

// =============================================================================
// IR STRUCTURE VALIDATION
// =============================================================================

mod ir_structure {
    use super::*;

    #[test]
    fn test_all_blocks_terminated() {
        let source = r#"
            function test(x: number): number {
                if (x > 0) {
                    return x;
                } else {
                    return -x;
                }
            }
        "#;
        let ir = lower(source);
        // Validation should pass (all blocks have terminators)
        assert!(ir.validate().is_ok());
    }

    #[test]
    fn test_function_has_entry_block() {
        let source = r#"
            function foo() {
                let x = 1;
            }
        "#;
        let ir = lower(source);
        let func = ir.get_function_by_name("foo").unwrap();
        assert!(!func.blocks.is_empty());
    }

    #[test]
    fn test_register_numbering() {
        let source = r#"
            let a = 1;
            let b = 2;
            let c = 3;
            let d = a + b + c;
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        // Registers should be numbered sequentially
        assert!(output.contains("r0:"));
        assert!(output.contains("r1:"));
        assert!(output.contains("r2:"));
    }
}

// =============================================================================
// EDGE CASES
// =============================================================================

mod edge_cases {
    use super::*;

    #[test]
    fn test_empty_program() {
        let ir = lower("");
        // Empty program should produce no functions
        assert_eq!(ir.function_count(), 0);
    }

    #[test]
    fn test_only_comments() {
        let ir = lower("// This is a comment");
        assert_eq!(ir.function_count(), 0);
    }

    #[test]
    fn test_empty_function() {
        let source = "function foo() {}";
        let ir = lower(source);
        let func = ir.get_function_by_name("foo").unwrap();
        assert!(!func.blocks.is_empty());
    }

    #[test]
    fn test_deeply_nested_if() {
        let source = r#"
            if (true) {
                if (true) {
                    if (true) {
                        if (true) {
                            if (true) {
                                let x = 1;
                            }
                        }
                    }
                }
            }
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        assert!(output.matches("branch").count() >= 5);
    }

    #[test]
    fn test_many_variables() {
        let source = r#"
            let a = 1; let b = 2; let c = 3; let d = 4; let e = 5;
            let f = 6; let g = 7; let h = 8; let i = 9; let j = 10;
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        assert!(output.contains("store_local 9"));
    }

    #[test]
    fn test_large_function() {
        let source = r#"
            function big(): number {
                let sum = 0;
                sum = sum + 1;
                sum = sum + 2;
                sum = sum + 3;
                sum = sum + 4;
                sum = sum + 5;
                sum = sum + 6;
                sum = sum + 7;
                sum = sum + 8;
                sum = sum + 9;
                sum = sum + 10;
                return sum;
            }
        "#;
        let ir = lower(source);
        let func = ir.get_function_by_name("big").unwrap();
        // Should have many instructions
        assert!(func.instruction_count() > 10);
    }
}

// =============================================================================
// TYPE ANNOTATIONS
// =============================================================================

mod type_annotations {
    use super::*;

    #[test]
    fn test_number_type() {
        let ir = lower("let x: number = 42;");
        assert!(ir.function_count() >= 1);
    }

    #[test]
    fn test_string_type() {
        let ir = lower("let x: string = \"hello\";");
        assert!(ir.function_count() >= 1);
    }

    #[test]
    fn test_boolean_type() {
        let ir = lower("let x: boolean = true;");
        assert!(ir.function_count() >= 1);
    }

    #[test]
    fn test_array_type() {
        let ir = lower("let x: number[] = [1, 2, 3];");
        assert!(ir.function_count() >= 1);
    }

    #[test]
    fn test_function_return_type() {
        let source = r#"
            function foo(): number {
                return 42;
            }
        "#;
        let ir = lower(source);
        assert!(ir.get_function_by_name("foo").is_some());
    }

    #[test]
    fn test_function_param_types() {
        let source = r#"
            function add(a: number, b: number): number {
                return a + b;
            }
        "#;
        let ir = lower(source);
        assert!(ir.get_function_by_name("add").is_some());
    }
}

// =============================================================================
// INTEGRATION TESTS
// =============================================================================

mod integration {
    use super::*;

    #[test]
    fn test_fibonacci() {
        let source = r#"
            function fib(n: number): number {
                if (n <= 1) {
                    return n;
                }
                return fib(n - 1) + fib(n - 2);
            }
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        assert!(output.contains("fn fib"));
        assert!(output.contains("branch"));
        assert!(output.contains("call"));
    }

    #[test]
    fn test_bubble_sort_like() {
        let source = r#"
            function sort(arr: number[], n: number) {
                for (let i = 0; i < n; i = i + 1) {
                    for (let j = 0; j < n - 1; j = j + 1) {
                        let a = arr[j];
                        let b = arr[j + 1];
                        if (a > b) {
                            arr[j] = b;
                            arr[j + 1] = a;
                        }
                    }
                }
            }
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        assert!(output.contains("for.header"));
        assert!(output.contains("load_elem"));
    }

    #[test]
    fn test_calculator() {
        let source = r#"
            function calculate(op: string, a: number, b: number): number {
                if (op == "+") {
                    return a + b;
                } else if (op == "-") {
                    return a - b;
                } else if (op == "*") {
                    return a * b;
                } else if (op == "/") {
                    return a / b;
                }
                return 0;
            }
        "#;
        let ir = lower(source);
        let output = ir.pretty_print();
        assert!(output.contains("fn calculate"));
        assert!(output.matches("branch").count() >= 4);
    }

    #[test]
    fn test_linked_list_like() {
        let source = r#"
            class Node {
                value: number;
                next: Node;
            }

            function sum(head: Node): number {
                let total = 0;
                let current = head;
                while (current != null) {
                    total = total + current.value;
                    current = current.next;
                }
                return total;
            }
        "#;
        let ir = lower(source);
        assert!(ir.get_class_by_name("Node").is_some());
        assert!(ir.get_function_by_name("sum").is_some());
    }
}
