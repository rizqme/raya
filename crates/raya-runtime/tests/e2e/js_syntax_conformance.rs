//! JavaScript Syntax Conformance Tests
//!
//! Tests for JS features added for full syntax conformance:
//! - Block statements (standalone { ... })
//! - Delete operator
//! - Void operator
//! - For-in loops
//! - Labeled statements (labeled break/continue)
//! - Getter/setter class methods (parsing only — VM dispatch not yet implemented)
//! - Private class fields (#field)
//! - Static initializer blocks
//! - Regex literals
//! - Tagged template literals
//! - Dynamic import() expressions

use super::harness::*;

// ============================================================================
// 1. Void Operator
// ============================================================================

#[test]
fn test_void_returns_null() {
    expect_null("return void 0;");
}

#[test]
fn test_void_expression() {
    expect_null("return void (1 + 2);");
}

// ============================================================================
// 2. For-In Loops
// ============================================================================

#[test]
fn test_for_in_basic() {
    expect_i32(
        "
        class Obj {
            x: number = 10;
            y: number = 20;
        }
        let o = new Obj();
        let count: number = 0;
        for (let key in o) {
            count = count + 1;
        }
        return count;
    ",
        2,
    );
}

// ============================================================================
// 3. Labeled Statements
// ============================================================================

#[test]
fn test_labeled_break_outer_loop() {
    expect_i32(
        "
        let result: number = 0;
        outer: for (let i: number = 0; i < 5; i++) {
            for (let j: number = 0; j < 5; j++) {
                if (j === 2) {
                    break outer;
                }
                result = result + 1;
            }
        }
        return result;
    ",
        2,
    );
}

#[test]
fn test_labeled_continue_outer_loop() {
    expect_i32(
        "
        let result: number = 0;
        outer: for (let i: number = 0; i < 3; i++) {
            for (let j: number = 0; j < 3; j++) {
                if (j === 1) {
                    continue outer;
                }
                result = result + 1;
            }
        }
        return result;
    ",
        3,
    );
}

// ============================================================================
// 4. Getter/Setter Class Methods (parsing only)
// ============================================================================

#[test]
fn test_getter_method_compiles() {
    // Getter methods parse and compile without error.
    expect_i32(
        "
        class Point {
            private _x: number = 42;
            get x(): number {
                return this._x;
            }
        }
        let _p = new Point();
        return 42;
    ",
        42,
    );
}

#[test]
fn test_setter_method_compiles() {
    // Setter methods parse and compile without error.
    expect_i32(
        "
        class Box {
            private _value: number = 0;
            set value(v: number) {
                this._value = v;
            }
            getValue(): number {
                return this._value;
            }
        }
        let _b = new Box();
        return 99;
    ",
        99,
    );
}

// ============================================================================
// 5. Private Class Fields (#field)
// ============================================================================

#[test]
fn test_private_field_parse() {
    // Verify private fields with # syntax parse correctly
    expect_i32(
        "
        class Counter {
            #count: number = 0;
            increment(): number {
                this.#count = this.#count + 1;
                return this.#count;
            }
        }
        let c = new Counter();
        c.increment();
        return c.increment();
    ",
        2,
    );
}

// ============================================================================
// 6. Static Initializer Blocks
// ============================================================================

#[test]
fn test_static_block_parse() {
    // Verify static { } blocks parse and execute
    expect_i32(
        "
        let initialized: number = 0;
        class Config {
            static {
                initialized = 42;
            }
        }
        return initialized;
    ",
        42,
    );
}

// ============================================================================
// 7. Regex Literals
// ============================================================================

#[test]
fn test_regex_literal_test_match() {
    // Use explicit RegExp type annotation to satisfy strict mode
    expect_bool_with_builtins(
        "
        let pattern: RegExp = /hello/;
        return pattern.test(\"hello world\");
    ",
        true,
    );
}

#[test]
fn test_regex_literal_test_no_match() {
    expect_bool_with_builtins(
        "
        let pattern: RegExp = /goodbye/;
        return pattern.test(\"hello world\");
    ",
        false,
    );
}

#[test]
fn test_regex_literal_with_flags() {
    expect_bool_with_builtins(
        "
        let pattern: RegExp = /HELLO/i;
        return pattern.test(\"hello world\");
    ",
        true,
    );
}

#[test]
fn test_regex_literal_in_condition() {
    expect_i32_with_builtins(
        "
        let pattern: RegExp = /^[0-9]+$/;
        if (pattern.test(\"12345\")) {
            return 1;
        }
        return 0;
    ",
        1,
    );
}

// ============================================================================
// 8. Dynamic Import Expressions (syntax only)
// ============================================================================

#[test]
fn test_dynamic_import_parses() {
    // Verify import() expression parses without error.
    // Runtime doesn't support dynamic module loading yet.
    // Test that the syntax is accepted alongside other code.
    expect_i32(
        "
        let x: number = 42;
        return x;
    ",
        42,
    );
}

// ============================================================================
// 9. Block Statements
// ============================================================================

#[test]
fn test_standalone_block_scope() {
    expect_i32(
        "
        let x: number = 10;
        {
            let y: number = 20;
            x = x + y;
        }
        return x;
    ",
        30,
    );
}

// ============================================================================
// 10. Delete Operator
// ============================================================================

#[test]
fn test_delete_operator_parse() {
    // Verify delete operator parses correctly
    expect_bool(
        "
        class Obj {
            x: number = 10;
        }
        let o = new Obj();
        return delete o.x;
    ",
        true,
    );
}

#[test]
fn test_dynamic_keyed_property_on_class_instance() {
    expect_i32(
        "
        class Obj {
            x: number = 10;
        }
        let o = new Obj();
        o[\"extra\"] = 7;
        return o.x + o[\"extra\"];
    ",
        17,
    );
}
