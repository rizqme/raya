//! End-to-end tests for std:args module

use super::harness::*;

#[test]
fn test_args_parser_creation_smoke() {
    expect_bool_with_builtins(
        r#"
        import args from "std:args";
        const parser = args.parser();
        return parser != null;
    "#,
        true,
    );
}

#[test]
fn test_args_define_string_smoke() {
    expect_bool_with_builtins(
        r#"
        import args from "std:args";
        const parser = args.parser();
        parser.string("output", "desc");
        return true;
    "#,
        true,
    );
}

#[test]
fn test_args_parse_smoke() {
    expect_bool_with_builtins(
        r#"
        import args from "std:args";
        const parser = args.parser();
        parser.string("output", "desc");
        parser.parse(["--output", "x"]);
        return true;
    "#,
        true,
    );
}

#[test]
fn test_method_receives_array_argument_smoke() {
    expect_i32_with_builtins(
        r#"
        class C {
            f(xs: string[]): number {
                return xs.length;
            }
        }
        const c = new C();
        return c.f(["a", "b", "c"]);
    "#,
        3,
    );
}

#[test]
fn test_string_concat_with_array_index_smoke() {
    expect_string_with_builtins(
        r#"
        const arr = ["--output", "x"];
        const s = "" + arr[0];
        return s;
    "#,
        "--output",
    );
}

#[test]
fn test_custom_parse_method_array_arg_smoke() {
    expect_i32_with_builtins(
        r#"
        class P {
            parse(items: string[]): number {
                return items.length;
            }
        }
        const p = new P();
        return p.parse(["a", "b"]);
    "#,
        2,
    );
}

#[test]
fn test_custom_parse_method_flag_values_smoke() {
    expect_string_with_builtins(
        r#"
        class P {
            parse(items: string[]): string {
                return items.length + "|" + items[0] + "|" + items[1] + "|" + items[2];
            }
        }
        const p = new P();
        return p.parse(["--output", "dist/app.js", "-v"]);
    "#,
        "3|--output|dist/app.js|-v",
    );
}

#[test]
fn test_loop_over_array_param_in_method_smoke() {
    expect_i32_with_builtins(
        r#"
        class A {
            parse(inputArgs: string[]): number {
                let i = 0;
                let c = 0;
                while (i < inputArgs.length) {
                    c = c + 1;
                    i = i + 1;
                }
                return c;
            }
        }
        const a = new A();
        return a.parse(["x", "y", "z"]);
    "#,
        3,
    );
}

#[test]
fn test_loop_and_read_flag_like_strings_smoke() {
    expect_i32_with_builtins(
        r#"
        class AP {
            parse(inputArgs: string[]): number {
                let i = 0;
                while (i < inputArgs.length) {
                    const arg = "" + inputArgs[i];
                    if (arg == "--") {
                        return 99;
                    }
                    i = i + 1;
                }
                return i;
            }
        }
        const p = new AP();
        return p.parse(["--output", "x"]);
    "#,
        2,
    );
}

#[test]
fn test_array_param_string_prefix_checks_smoke() {
    expect_string_with_builtins(
        r#"
        class AP {
            probe(inputArgs: string[]): string {
                const arg = "" + inputArgs[0];
                const a = arg.length >= 2 && arg.substring(0, 2) == "--";
                const b = arg.length >= 5 && arg.substring(0, 5) == "--out";
                const c = arg.length == 8;
                return (a ? "1" : "0") + "|" + (b ? "1" : "0") + "|" + (c ? "1" : "0") + "|" + arg;
            }
        }
        const p = new AP();
        return p.probe(["--output"]);
    "#,
        "1|1|1|--output",
    );
}

#[test]
fn test_loop_indexed_array_param_prefix_checks_smoke() {
    expect_string_with_builtins(
        r#"
        class AP {
            probe(inputArgs: string[]): string {
                let idx = 0;
                while (idx < inputArgs.length) {
                    const arg = "" + inputArgs[idx];
                    const a = arg.length >= 2 && arg.substring(0, 2) == "--";
                    return (a ? "1" : "0") + "|" + arg;
                }
                return "empty";
            }
        }
        const p = new AP();
        return p.probe(["--output", "x"]);
    "#,
        "1|--output",
    );
}

#[test]
fn test_loop_if_chain_with_continue_for_long_options_smoke() {
    expect_string_with_builtins(
        r#"
        class P {
            parse(inputArgs: string[]): string {
                let idx = 0;
                let longHits = 0;
                let pos = 0;
                while (idx < inputArgs.length) {
                    const arg = "" + inputArgs[idx];
                    if (arg.length >= 5 && arg.substring(0, 5) == "--no-") {
                        idx = idx + 1;
                        continue;
                    }
                    if (arg.length >= 2 && arg.substring(0, 2) == "--") {
                        longHits = longHits + 1;
                        if (idx + 1 < inputArgs.length) {
                            idx = idx + 2;
                        } else {
                            idx = idx + 1;
                        }
                        continue;
                    }
                    pos = pos + 1;
                    idx = idx + 1;
                }
                return longHits + "|" + pos;
            }
        }
        const p = new P();
        return p.parse(["--output", "x", "tail"]);
    "#,
        "1|1",
    );
}

#[test]
fn test_array_param_length_reuse_in_comparison_smoke() {
    expect_string_with_builtins(
        r#"
        class P {
            probe(inputArgs: string[]): string {
                const a = inputArgs.length;
                const b = (0 + 1 < inputArgs.length) ? "1" : "0";
                const c = (1 + 1 < inputArgs.length) ? "1" : "0";
                return a + "|" + b + "|" + c;
            }
        }
        const p = new P();
        return p.probe(["--output", "x", "tail"]);
    "#,
        "3|1|1",
    );
}

#[test]
fn test_zero_plus_one_less_than_three_smoke() {
    expect_string_with_builtins(
        r#"
        const a = (0 + 1 < 3) ? "1" : "0";
        const b = (1 + 1 < 3) ? "1" : "0";
        return a + "|" + b;
    "#,
        "1|1",
    );
}

#[test]
fn test_args_without_import_smoke() {
    expect_bool_with_builtins(
        r#"
        const parser = new ArgParser();
        parser.string("output", "desc");
        const res = parser.parse(["--output", "x"]);
        return res.getString("output") == "x";
    "#,
        true,
    );
}

#[test]
fn test_args_without_import_debug_pairs() {
    expect_string_with_builtins(
        r#"
        const parser = new ArgParser();
        parser.string("output", "desc");
        const res = parser.parse(["--output", "x"]);
        const len = res._strings.length;
        const a = len > 0 ? res._strings[0] : "";
        const b = len > 1 ? res._strings[1] : "";
        const pLen = res._present.length;
        const p0 = pLen > 0 ? res._present[0] : "";
        return len + "|" + a + "|" + b + "|" + pLen + "|" + p0;
    "#,
        "2|output|x|1|output",
    );
}

#[test]
fn test_argresult_present_push_smoke() {
    expect_i32_with_builtins(
        r#"
        const r = new ArgResult();
        r._present.push("x");
        return r._present.length;
    "#,
        1,
    );
}

#[test]
fn test_argparser_alias_assignment_smoke() {
    expect_string_with_builtins(
        r#"
        const p = new ArgParser();
        p.string("output", "desc");
        p.boolean("verbose", "desc");
        p.alias("verbose", "v");
        return p._defs[1].alias;
    "#,
        "v",
    );
}

#[test]
fn test_argparser_resolve_alias_smoke() {
    expect_string_with_builtins(
        r#"
        const p = new ArgParser();
        p.boolean("verbose", "desc");
        p.alias("verbose", "v");
        return p._resolveAlias("v");
    "#,
        "verbose",
    );
}

#[test]
fn test_argparser_alias_direct_compare_smoke() {
    expect_bool_with_builtins(
        r#"
        const p = new ArgParser();
        p.boolean("verbose", "desc");
        p.alias("verbose", "v");
        return p._defs[0].alias == "v";
    "#,
        true,
    );
}

#[test]
fn test_argparser_boolean_defaultvalue_smoke() {
    expect_string_with_builtins(
        r#"
        const p = new ArgParser();
        p.boolean("verbose", "desc");
        return p._defs[0].defaultValue;
    "#,
        "false",
    );
}

#[test]
fn test_argparser_defs_growth_smoke() {
    expect_i32_with_builtins(
        r#"
        const p = new ArgParser();
        p.string("output", "desc");
        p.boolean("verbose", "desc");
        return p._defs.length;
    "#,
        2,
    );
}

#[test]
fn test_custom_parser_like_class_defs_growth_smoke() {
    expect_i32_with_builtins(
        r#"
        class P {
            defs: string[];
            defaults: string[];
            constructor() {
                this.defs = [];
                this.defaults = [];
            }
            string(name: string): void {
                this.defs.push(name);
            }
            boolean(name: string): void {
                this.defs.push(name);
                this.defaults[this.defaults.length] = name;
                this.defaults[this.defaults.length] = "false";
            }
        }
        const p = new P();
        p.string("output");
        p.boolean("verbose");
        return p.defs.length;
    "#,
        2,
    );
}

#[test]
fn test_argresult_positionals_length_smoke() {
    expect_i32_with_builtins(
        r#"
        const r = new ArgResult();
        return r.positionals().length;
    "#,
        0,
    );
}

#[test]
fn test_method_returned_array_index_smoke() {
    expect_bool_with_builtins(
        r#"
        class X {
            getValues(): string[] {
                return ["a", "b"];
            }
        }
        const x = new X();
        const p = x.getValues();
        return p.length == 2 && p[0] == "a" && p[1] == "b";
    "#,
        true,
    );
}

#[test]
fn test_engine_method_two_args_smoke() {
    expect_i32_with_builtins(
        r#"
        class T {
            g(a: string, b: number): number {
                return b;
            }
            f(): number {
                return this.g("x", 0);
            }
        }
        const t = new T();
        return t.f();
    "#,
        0,
    );
}

#[test]
fn test_engine_recursive_method_two_args_smoke() {
    expect_i32_with_builtins(
        r#"
        class T {
            g(a: string, b: number): number {
                if (b <= 0) {
                    return b;
                }
                return this.g(a, b - 1);
            }
        }
        const t = new T();
        return t.g("x", 3);
    "#,
        0,
    );
}

#[test]
fn test_args_long_and_alias_and_boolean() {
    expect_bool_with_builtins(
        r#"
        import args from "std:args";
        const parser = args.parser();
        parser.string("output", "output path");
        parser.boolean("verbose", "verbose");
        parser.alias("verbose", "v");
        const res = parser.parse(["--output", "dist/app.js", "-v"]);
        return res.getString("output") == "dist/app.js" && res.getBoolean("verbose") && res.has("verbose");
    "#,
        true,
    );
}

#[test]
fn test_args_long_and_alias_debug() {
    expect_string_with_builtins(
        r#"
        import args from "std:args";
        const parser = args.parser();
        parser.string("output", "output path");
        parser.boolean("verbose", "verbose");
        parser.alias("verbose", "v");
        const res = parser.parse(["--output", "dist/app.js", "-v"]);
        const len = res._strings.length;
        const a = len > 0 ? res._strings[0] : "";
        const b = len > 1 ? res._strings[1] : "";
        const pLen = res._present.length;
        const p0 = pLen > 0 ? res._present[0] : "";
        return len + "|" + a + "|" + b + "|" + pLen + "|" + p0;
    "#,
        "4|verbose|true|2|output",
    );
}

#[test]
fn test_engine_nested_member_assignment_smoke() {
    expect_bool_with_builtins(
        r#"
        class Leaf {
            value: string;
            constructor() {
                this.value = "";
            }
        }

        class Root {
            leaf: Leaf;
            constructor() {
                this.leaf = new Leaf();
            }
            set(v: string): void {
                this.leaf.value = v;
            }
        }

        const r = new Root();
        r.set("ok");
        return r.leaf.value == "ok";
    "#,
        true,
    );
}

#[test]
fn test_engine_indexed_member_assignment_smoke() {
    expect_bool_with_builtins(
        r#"
        class Item {
            alias: string;
            constructor() {
                this.alias = "";
            }
        }

        class Holder {
            items: Item[];
            constructor() {
                this.items = [new Item(), new Item()];
            }
            setAlias(i: number, v: string): void {
                this.items[i].alias = v;
            }
        }

        const h = new Holder();
        h.setAlias(1, "v1");
        return h.items[0].alias == "" && h.items[1].alias == "v1";
    "#,
        true,
    );
}

#[test]
fn test_args_defaults_and_no_flag() {
    expect_bool_with_builtins(
        r#"
        import args from "std:args";
        const parser = args.parser();
        parser.stringDefault("mode", "mode", "dev");
        parser.boolean("color", "color");
        const res = parser.parse(["--no-color"]);
        return res.getString("mode") == "dev" && !res.getBoolean("color");
    "#,
        true,
    );
}

#[test]
fn test_args_positionals_and_rest() {
    expect_bool_with_builtins(
        r#"
        import args from "std:args";
        const parser = args.parser();
        parser.string("name", "name");
        const res = parser.parse(["--name", "raya", "build", "src", "--", "--raw", "x"]);
        const p = res.positionals();
        const r = res.rest();
        return p.length == 2 && p[0] == "build" && p[1] == "src" && r.length == 2 && r[0] == "--raw";
    "#,
        true,
    );
}

#[test]
fn test_args_positionals_and_rest_debug() {
    expect_string_with_builtins(
        r#"
        import args from "std:args";
        const parser = args.parser();
        parser.string("name", "name");
        const res = parser.parse(["--name", "raya", "build", "src", "--", "--raw", "x"]);
        const p = res.positionals();
        const r = res.rest();
        return p.length + "|" + p.join(",") + "|" + r.length + "|" + r.join(",") + "|" + res.getString("name");
    "#,
        "2|build,src|2|--raw,x|raya",
    );
}
