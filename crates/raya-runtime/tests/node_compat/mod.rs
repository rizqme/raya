use raya_runtime::{BuiltinMode, Runtime, RuntimeOptions, Session};
use std::time::Duration;

fn expect_bool(value: raya_runtime::Value, expected: bool) {
    let actual = value.as_bool().unwrap_or(false);
    assert_eq!(actual, expected, "expected {}, got {:?}", expected, value);
}

fn expect_number(value: raya_runtime::Value, expected: f64) {
    let actual = value
        .as_f64()
        .or_else(|| value.as_i32().map(|n| n as f64))
        .unwrap_or(f64::NAN);
    assert_eq!(actual, expected, "expected {}, got {:?}", expected, value);
}

fn expect_node_compat_string(source: &str, expected: &str) {
    let options = RuntimeOptions {
        builtin_mode: BuiltinMode::NodeCompat,
        ..Default::default()
    };
    let mut session = Session::new(&options);
    let value = session
        .eval(source)
        .expect("node-compat eval should succeed");
    let formatted = session.format_value(&value);
    let actual = formatted
        .strip_prefix('"')
        .and_then(|trimmed| trimmed.strip_suffix('"'))
        .unwrap_or(&formatted)
        .to_string();
    assert_eq!(actual, expected, "expected {:?}, got {}", expected, formatted);
}

fn expect_node_compat_async_global_string(source: &str, expected: &str) {
    let runtime = Runtime::with_options(RuntimeOptions {
        builtin_mode: BuiltinMode::NodeCompat,
        ..Default::default()
    });
    let program = runtime
        .compile_program_source(source)
        .expect("node-compat compile should succeed");
    let mut vm = runtime.create_vm();
    runtime
        .execute_program_with_vm(&program, &mut vm)
        .expect("node-compat execute should succeed");
    assert!(
        vm.wait_all(Duration::from_millis(500)),
        "async work should settle"
    );
    let value = vm
        .builtin_global_named_field_value("globalThis", "__result")
        .expect("globalThis.__result should be set");
    let actual = vm
        .plain_string_value(value)
        .or_else(|| value.as_i32().map(|n| n.to_string()))
        .or_else(|| value.as_u32().map(|n| n.to_string()))
        .or_else(|| value.as_f64().map(|n| n.to_string()))
        .unwrap_or_else(|| format!("{value:?}"));
    assert_eq!(actual, expected, "expected {:?}, got {:?}", expected, actual);
}

#[test]
fn test_node_compat_with_unscopables_assignment_falls_back_to_local_binding() {
    let runtime = Runtime::with_options(RuntimeOptions {
        builtin_mode: BuiltinMode::NodeCompat,
        ..Default::default()
    });

    let value = runtime
        .eval(
            r#"
            var v = 1;
            globalThis[Symbol.unscopables] = { v: true };

            let ref = (x) => {
                var v = x;
                with (globalThis) {
                    v = 20;
                }
                return v == 20 && globalThis.v == 1;
            };

            return ref(10);
            "#,
        )
        .expect("node-compat eval should succeed");

    expect_bool(value, true);
}

#[test]
fn test_node_compat_named_async_function_expression_self_binding_is_stable_in_sloppy_mode() {
    expect_node_compat_async_global_string(
        r#"
        async function run() {
            let ref = async function BindingIdentifier() {
                (() => {
                    BindingIdentifier = 1;
                })();
                return BindingIdentifier;
            };
            return (await ref()) === ref;
        }

        globalThis.__result = String(await run());
        "#,
        "true",
    );
}

#[test]
fn test_node_compat_define_property_preserves_value_for_writable_false() {
    let runtime = Runtime::with_options(RuntimeOptions {
        builtin_mode: BuiltinMode::NodeCompat,
        ..Default::default()
    });

    let value = runtime
        .eval(
            r#"
            let err = new Error("original");
            let d = new Object() as {
                value: string,
                writable: boolean,
                configurable: boolean
            };
            d.value = "locked";
            d.writable = false;
            d.configurable = true;
            Object.defineProperty(err, "lockedField", d);

            let threw = false;
            try {
                err["lockedField"] = "new-value";
            } catch (e) {
                threw = true;
            }

            return !threw && err["lockedField"] == "locked";
            "#,
        )
        .expect("node-compat eval should succeed");

    expect_bool(value, true);
}

#[test]
fn test_node_compat_define_property_blocks_redefine_non_configurable() {
    let runtime = Runtime::with_options(RuntimeOptions {
        builtin_mode: BuiltinMode::NodeCompat,
        ..Default::default()
    });

    let value = runtime
        .eval(
            r#"
            let err = new Error("m1");
            let d1 = new Object() as {
                value: string,
                writable: boolean,
                configurable: boolean
            };
            d1.value = "E1";
            d1.writable = true;
            d1.configurable = false;
            Object.defineProperty(err, "lockedName", d1);

            let redefineThrew = false;
            try {
                let d2 = new Object() as {
                    value: string,
                    writable: boolean,
                    configurable: boolean
                };
                d2.value = "E2";
                d2.writable = true;
                d2.configurable = true;
                Object.defineProperty(err, "lockedName", d2);
            } catch (e) {
                redefineThrew = true;
            }

            return redefineThrew && err["lockedName"] == "E1";
            "#,
        )
        .expect("node-compat eval should succeed");

    expect_bool(value, true);
}

#[test]
fn test_node_compat_define_property_getter_invoked_on_read() {
    let runtime = Runtime::with_options(RuntimeOptions {
        builtin_mode: BuiltinMode::NodeCompat,
        ..Default::default()
    });

    let value = runtime
        .eval(
            r#"
            let o = new Error("x");
            let d = new Object() as {
                get: () => unknown,
                configurable: boolean
            };
            d.get = (): Object => { return new Object(); };
            d.configurable = true;
            Object.defineProperty(o, "customCause", d);
            return o["customCause"] != null;
            "#,
        )
        .expect("node-compat eval should succeed");

    expect_bool(value, true);
}

#[test]
fn test_node_compat_define_property_setter_invoked_on_write() {
    let runtime = Runtime::with_options(RuntimeOptions {
        builtin_mode: BuiltinMode::NodeCompat,
        ..Default::default()
    });

    let value = runtime
        .eval(
            r#"
            let o = new Error("x");
            let hit = false;
            let d = new Object() as {
                set: (v: number) => void,
                configurable: boolean
            };
            d.set = (v: number): void => {
                hit = v == 1;
            };
            d.configurable = true;
            Object.defineProperty(o, "customCause", d);
            o["customCause"] = 1;
            return hit;
            "#,
        )
        .expect("node-compat eval should succeed");

    expect_bool(value, true);
}

#[test]
fn test_node_compat_get_own_property_descriptor_roundtrip() {
    let runtime = Runtime::with_options(RuntimeOptions {
        builtin_mode: BuiltinMode::NodeCompat,
        ..Default::default()
    });

    let value = runtime
        .eval(
            r#"
            let err = new Error("base");
            let d = new Object() as {
                value: string,
                writable: boolean,
                configurable: boolean,
                enumerable: boolean
            };
            d.value = "locked";
            d.writable = false;
            d.configurable = true;
            d.enumerable = false;
            Object.defineProperty(err, "lockedField", d);

            let got = Object.getOwnPropertyDescriptor(err, "lockedField");
            return got != null
                && got.value == "locked"
                && got.writable == false
                && got.configurable == true
                && got.enumerable == false;
            "#,
        )
        .expect("node-compat eval should succeed");

    expect_bool(value, true);
}

#[test]
fn test_node_compat_object_create_with_proto_only_uses_source_defined_surface() {
    let runtime = Runtime::with_options(RuntimeOptions {
        builtin_mode: BuiltinMode::NodeCompat,
        ..Default::default()
    });

    let value = runtime
        .eval(
            r#"
            let proto = { marker: 1 };
            let obj = Object.create(proto);
            return Object.getPrototypeOf(obj) == proto && obj["marker"] == 1;
            "#,
        )
        .expect("node-compat eval should succeed");

    expect_bool(value, true);
}

#[test]
fn test_node_events_emit_function_payload_cast_path() {
    let runtime = Runtime::with_options(RuntimeOptions {
        builtin_mode: BuiltinMode::NodeCompat,
        ..Default::default()
    });

    let value = runtime
        .eval(
            r#"
            import EventEmitter from "node:events";
            const emitter = new EventEmitter<{ tick: [number] }>();
            emitter.on("tick", (_: number): void => {});
            emitter.emit("tick", 1);
            return emitter.listenerCount("tick") == 1;
            "#,
        )
        .expect("node-compat EventEmitter emit path should succeed");

    expect_bool(value, true);
}

#[test]
fn test_node_compat_identifier_update_and_assignment_share_reference_path() {
    let runtime = Runtime::with_options(RuntimeOptions {
        builtin_mode: BuiltinMode::NodeCompat,
        ..Default::default()
    });

    let value = runtime
        .eval(
            r#"
            let i = 0;
            i++;
            i = i + 1;
            return i == 2;
            "#,
        )
        .expect("node-compat eval should succeed");

    expect_bool(value, true);
}

#[test]
fn test_node_compat_direct_eval_assignment_updates_outer_binding() {
    let runtime = Runtime::with_options(RuntimeOptions {
        builtin_mode: BuiltinMode::NodeCompat,
        ..Default::default()
    });

    let value = runtime
        .eval(
            r#"
            let x = 1;
            eval("x = 2");
            return x == 2;
            "#,
        )
        .expect("node-compat eval should succeed");

    expect_bool(value, true);
}

#[test]
fn test_node_compat_direct_eval_function_binding_delete_uses_runtime_env() {
    expect_node_compat_string(
        r#"
        let initialResult = 0;
        let postDeletion = null;
        let thrownName = "";
        (function() {
            eval("initialResult = f(); delete f; postDeletion = function() { f; }; function f() { return 33; }");
        }());
        try {
            postDeletion();
        } catch (error) {
            thrownName = error != null && error.name ? error.name : String(error);
        }
        return JSON.stringify({
            initialResult,
            hasPostDeletion: postDeletion != null,
            thrownName
        });
        "#,
        r#"{"hasPostDeletion":true,"initialResult":33,"thrownName":"ReferenceError"}"#,
    );
}

#[test]
fn test_node_compat_direct_eval_arrow_params_use_local_bindings() {
    expect_node_compat_string(
        r#"
        return (() => ((a, b) => a + ":" + b)("x", "y"))();
        "#,
        "x:y",
    );
}

#[test]
fn test_node_compat_with_assignment_and_delete_use_identifier_reference() {
    let runtime = Runtime::with_options(RuntimeOptions {
        builtin_mode: BuiltinMode::NodeCompat,
        ..Default::default()
    });

    let value = runtime
        .eval(
            r#"
            let target = { x: 1 };
            let deleted = false;
            with (target) {
                x = 2;
                deleted = delete x;
            }
            return deleted && target.x == undefined;
            "#,
        )
        .expect("node-compat eval should succeed");

    expect_bool(value, true);
}

#[test]
fn test_node_compat_top_level_direct_eval_assignment_updates_outer_binding() {
    let runtime = Runtime::with_options(RuntimeOptions {
        builtin_mode: BuiltinMode::NodeCompat,
        ..Default::default()
    });

    let program = runtime
        .compile_program_source(
            r#"
            let x = 1;
            eval("x = 2");
            x == 2;
            "#,
        )
        .expect("node-compat top-level compile should succeed");
    let value = runtime
        .execute_program(&program)
        .expect("node-compat top-level execute should succeed");

    expect_bool(value, true);
}

#[test]
fn test_node_compat_with_assignment_falls_back_to_outer_binding() {
    let runtime = Runtime::with_options(RuntimeOptions {
        builtin_mode: BuiltinMode::NodeCompat,
        ..Default::default()
    });

    let value = runtime
        .eval(
            r#"
            let x = 1;
            with ({}) {
                x = 2;
            }
            return x == 2;
            "#,
        )
        .expect("node-compat eval should succeed");

    expect_bool(value, true);
}

#[test]
fn test_node_compat_with_expression_preserves_body_completion() {
    let runtime = Runtime::with_options(RuntimeOptions {
        builtin_mode: BuiltinMode::NodeCompat,
        ..Default::default()
    });

    let value = runtime
        .eval(
            r#"
            let env = {};
            let o = { p: 1 };
            with (env) {
                o.p;
            }
            "#,
        )
        .expect("node-compat eval should succeed");

    expect_number(value, 1.0);
}

#[test]
fn test_node_compat_with_expression_preserves_prior_completion_when_body_is_empty() {
    let runtime = Runtime::with_options(RuntimeOptions {
        builtin_mode: BuiltinMode::NodeCompat,
        ..Default::default()
    });

    let value = runtime
        .eval(
            r#"
            1;
            with ({}) {
                var x = 1;
            }
            "#,
        )
        .expect("node-compat eval should succeed");

    expect_number(value, 1.0);
}

#[test]
fn test_node_compat_for_let_empty_loop_returns() {
    let runtime = Runtime::with_options(RuntimeOptions {
        builtin_mode: BuiltinMode::NodeCompat,
        ..Default::default()
    });

    let value = runtime
        .eval(
            r#"
            for (let i = 0; i < 1; i++) {}
            return 42;
            "#,
        )
        .expect("node-compat eval should succeed");

    expect_number(value, 42.0);
}

#[test]
fn test_node_compat_for_let_closure_captures_iteration_env() {
    expect_node_compat_string(
        r#"
        let out = [];
        for (let i = 0; i < 3; i++) {
            out.push(() => i);
        }
        return out.map(f => f()).join(",");
        "#,
        "0,1,2",
    );
}

#[test]
fn test_node_compat_for_of_let_closure_captures_iteration_env() {
    expect_node_compat_string(
        r#"
        let out = [];
        for (let x of [0, 1, 2]) {
            out.push(() => x);
        }
        return out.map(f => f()).join(",");
        "#,
        "0,1,2",
    );
}

#[test]
fn test_node_compat_for_in_let_closure_captures_iteration_env() {
    expect_node_compat_string(
        r#"
        let obj = { a: 1, b: 2, c: 3 };
        let out = [];
        for (let k in obj) {
            out.push(() => k);
        }
        return out.map(f => f()).join(",");
        "#,
        "a,b,c",
    );
}

#[test]
fn test_node_compat_for_let_direct_eval_uses_iteration_env() {
    expect_node_compat_string(
        r#"
        let out = [];
        for (let i = 0; i < 3; i++) {
            eval("i = i + 1");
            out.push(i);
        }
        return JSON.stringify(out);
        "#,
        "[1,3]",
    );
}

#[test]
fn test_node_compat_builtin_registry_dispatch_for_map_methods() {
    let runtime = Runtime::with_options(RuntimeOptions {
        builtin_mode: BuiltinMode::NodeCompat,
        ..Default::default()
    });

    let value = runtime
        .eval(
            r#"
            const map = new Map();
            map.set("x", 1);
            return map.get("x") == 1;
            "#,
        )
        .expect("node-compat eval should succeed");

    expect_bool(value, true);
}

#[test]
fn test_node_compat_constructor_value_alias_dispatches_through_runtime_constructor() {
    let runtime = Runtime::with_options(RuntimeOptions {
        builtin_mode: BuiltinMode::NodeCompat,
        ..Default::default()
    });

    let value = runtime
        .eval(
            r#"
            class Foo {
                value = 1;
            }
            const C = Foo;
            const instance = new C();
            return instance.value == 1;
            "#,
        )
        .expect("node-compat eval should succeed");

    expect_bool(value, true);
}

#[test]
fn test_node_compat_async_generator_function_expression_resolves_after_next() {
    expect_node_compat_async_global_string(
        r#"
        var callCount = 0;
        var ref;
        ref = async function* g() {
            callCount = callCount + 1;
        };
        ref(42, "TC39",).next().then(() => {
            globalThis.__result = "" + callCount;
        }, err => {
            globalThis.__result = "err:" + err;
        });
        return null;
        "#,
        "1",
    );
}

#[test]
fn test_node_compat_async_generator_method_resolves_after_next() {
    expect_node_compat_async_global_string(
        r#"
        var callCount = 0;
        var obj = {
          async *method() {
            callCount = callCount + 1;
          }
        };
        var ref = obj.method;
        ref(42, "TC39",).next().then(() => {
            globalThis.__result = "" + callCount;
        }, err => {
            globalThis.__result = "err:" + err;
        });
        return null;
        "#,
        "1",
    );
}

#[test]
fn test_node_compat_async_private_generator_method_resolves_after_next() {
    expect_node_compat_async_global_string(
        r#"
        var callCount = 0;
        class C {
          async * #method() {
            callCount = callCount + 1;
          }
          get method() {
            return this.#method;
          }
        }
        new C().method(42, "TC39",).next().then(() => {
            globalThis.__result = "" + callCount;
        }, err => {
            globalThis.__result = "err:" + err;
        });
        return null;
        "#,
        "1",
    );
}

#[test]
fn test_node_compat_async_generator_throw_forwards_to_generator_body() {
    expect_node_compat_async_global_string(
        r#"
        async function* gen() {
            try {
                yield 1;
                yield 2;
            } catch (err) {
                return "caught:" + err;
            }
        }
        let it = gen();
        it.next().then(first => {
            it.throw("boom").then(second => {
                it.next().then(third => {
                    globalThis.__result = JSON.stringify([
                        first.value, first.done,
                        second.value, second.done,
                        third.value, third.done
                    ]);
                }, err => {
                    globalThis.__result = "err:third:" + err;
                });
            }, err => {
                globalThis.__result = "err:second:" + err;
            });
        }, err => {
            globalThis.__result = "err:first:" + err;
        });
        return null;
        "#,
        r#"[1,false,"caught:boom",true,null,true]"#,
    );
}

#[test]
fn test_node_compat_async_generator_return_completes_iterator() {
    expect_node_compat_async_global_string(
        r#"
        async function* gen() {
            yield 1;
            yield 2;
        }
        let it = gen();
        it.next().then(first => {
            it.return(9).then(second => {
                it.next().then(third => {
                    globalThis.__result = JSON.stringify([
                        first.value, first.done,
                        second.value, second.done,
                        third.value, third.done
                    ]);
                }, err => {
                    globalThis.__result = "err:third:" + err;
                });
            }, err => {
                globalThis.__result = "err:second:" + err;
            });
        }, err => {
            globalThis.__result = "err:first:" + err;
        });
        return null;
        "#,
        r#"[1,false,9,true,null,true]"#,
    );
}

#[test]
fn test_node_compat_async_generator_for_await_rejects_inner_promise_value() {
    expect_node_compat_async_global_string(
        r#"
        let error = new Error("boom");
        async function* readFile() {
            yield Promise.reject(error);
            yield "unreachable";
        }
        async function* gen() {
            for await (let line of readFile()) {
                yield line;
            }
        }
        let iter = gen();
        iter.next().then(() => {
            globalThis.__result = "resolved";
        }, rejectValue => {
            iter.next().then(({done, value}) => {
                globalThis.__result = JSON.stringify([
                    rejectValue === error,
                    done,
                    value === undefined
                ]);
            }, err => {
                globalThis.__result = "err:second:" + err;
            });
        });
        return null;
        "#,
        r#"[true,true,true]"#,
    );
}
