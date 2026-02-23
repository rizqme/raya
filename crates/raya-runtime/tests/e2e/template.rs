//! End-to-end tests for std:template module

use super::harness::*;

#[test]
fn test_template_render_interpolation() {
    expect_string_with_builtins(
        r#"
        import template from "std:template";
        return template.render("Hello, {{name}}!", "{\"name\":\"Raya\"}");
    "#,
        "Hello, Raya!",
    );
}

#[test]
fn test_template_section_and_inverted_section() {
    expect_bool_with_builtins(
        r#"
        import template from "std:template";
        const a = template.render("{{#items}}{{.}},{{/items}}", "{\"items\":[\"a\",\"b\"]}");
        const b = template.render("{{^items}}empty{{/items}}", "{\"items\":[]}");
        return a == "a,b," && b == "empty";
    "#,
        true,
    );
}

#[test]
fn test_template_compiled_reuse() {
    expect_bool_with_builtins(
        r#"
        import template from "std:template";
        const c = template.compile("Hi {{name}}");
        const a = c.render("{\"name\":\"A\"}");
        const b = c.render("{\"name\":\"B\"}");
        c.release();
        return a == "Hi A" && b == "Hi B";
    "#,
        true,
    );
}
