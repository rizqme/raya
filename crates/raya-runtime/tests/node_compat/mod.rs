use raya_runtime::{BuiltinMode, Runtime, RuntimeOptions};

fn expect_bool(value: raya_runtime::Value, expected: bool) {
    let actual = value.as_bool().unwrap_or(false);
    assert_eq!(actual, expected, "expected {}, got {:?}", expected, value);
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
