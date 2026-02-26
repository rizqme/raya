use raya_runtime::{BuiltinMode, Runtime, RuntimeOptions};

fn expect_bool(value: raya_runtime::Value, expected: bool) {
    let actual = value.as_bool().unwrap_or(false);
    assert_eq!(actual, expected, "expected {}, got {:?}", expected, value);
}

#[test]
fn test_node_compat_define_property_enforces_writable_false() {
    let runtime = Runtime::with_options(RuntimeOptions {
        builtin_mode: BuiltinMode::NodeCompat,
        ..Default::default()
    });

    let value = runtime
        .eval(
            r#"
            let err = new Error("original");
            let d = new Object();
            d.value = "locked";
            d.writable = false;
            d.configurable = true;
            Object.defineProperty(err, "message", d);

            let threw = false;
            try {
                err.message = "new-value";
            } catch (e) {
                threw = true;
            }

            return threw && err.message == "locked";
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
            let d1 = new Object();
            d1.value = "E1";
            d1.writable = true;
            d1.configurable = false;
            Object.defineProperty(err, "name", d1);

            let redefineThrew = false;
            try {
                let d2 = new Object();
                d2.value = "E2";
                d2.writable = true;
                d2.configurable = true;
                Object.defineProperty(err, "name", d2);
            } catch (e) {
                redefineThrew = true;
            }

            return redefineThrew && err.name == "E1";
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
            let d = new Object();
            d.get = (): Object => { return new Object(); };
            d.configurable = true;
            Object.defineProperty(o, "cause", d);
            return o.cause != null;
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
            let seen: Object | null = null;
            let d = new Object();
            d.set = (v: Object): void => {
                hit = true;
                seen = v;
            };
            d.configurable = true;
            Object.defineProperty(o, "cause", d);
            o.cause = new Object();
            return hit && seen != null;
            "#,
        )
        .expect("node-compat eval should succeed");

    expect_bool(value, true);
}
