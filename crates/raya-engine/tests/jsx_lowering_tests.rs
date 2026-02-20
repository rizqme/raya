//! JSX lowering tests
//!
//! Tests that JSX elements and fragments are correctly desugared
//! into createElement() calls during the AST-to-IR lowering phase.

use raya_engine::compiler::lower::{JsxOptions, Lowerer};
use raya_engine::compiler::ir::PrettyPrint;
use raya_engine::parser::{Parser, TypeContext};

/// Parse source and lower to IR with JSX enabled (default options)
fn lower_jsx(source: &str) -> String {
    let parser = Parser::new(source).expect("lexer error");
    let (module, interner) = parser.parse().expect("parse error");
    let type_ctx = TypeContext::new();
    let mut lowerer = Lowerer::new(&type_ctx, &interner).with_jsx(JsxOptions::default());
    let ir = lowerer.lower_module(&module);
    ir.pretty_print()
}

/// Parse source and lower to IR with custom JSX options
fn lower_jsx_with(source: &str, options: JsxOptions) -> String {
    let parser = Parser::new(source).expect("lexer error");
    let (module, interner) = parser.parse().expect("parse error");
    let type_ctx = TypeContext::new();
    let mut lowerer = Lowerer::new(&type_ctx, &interner).with_jsx(options);
    let ir = lowerer.lower_module(&module);
    ir.pretty_print()
}

/// Parse source and lower to IR WITHOUT JSX enabled
fn lower_no_jsx(source: &str) -> String {
    let parser = Parser::new(source).expect("lexer error");
    let (module, interner) = parser.parse().expect("parse error");
    let type_ctx = TypeContext::new();
    let mut lowerer = Lowerer::new(&type_ctx, &interner);
    let ir = lowerer.lower_module(&module);
    ir.pretty_print()
}

/// Count occurrences of a string in the IR output
fn count_in(ir: &str, needle: &str) -> usize {
    ir.matches(needle).count()
}

// ============================================================================
// Basic Element Lowering
// ============================================================================

#[test]
fn test_jsx_intrinsic_element_becomes_string_tag() {
    let ir = lower_jsx(r#"const x = <div />;"#);
    // Intrinsic elements should produce a string constant "div"
    assert!(ir.contains("\"div\""), "IR should contain string \"div\":\n{}", ir);
}

#[test]
fn test_jsx_self_closing_no_children() {
    let ir = lower_jsx(r#"const x = <br />;"#);
    assert!(ir.contains("\"br\""), "IR should contain string \"br\":\n{}", ir);
}

#[test]
fn test_jsx_element_with_string_attr() {
    let ir = lower_jsx(r#"const x = <div className="test" />;"#);
    assert!(ir.contains("\"div\""), "IR should contain \"div\" tag:\n{}", ir);
    assert!(ir.contains("\"test\""), "IR should contain attr value \"test\":\n{}", ir);
    // Should produce an ObjectLiteral for the props
    assert!(ir.contains("object_literal"), "IR should contain ObjectLiteral for props:\n{}", ir);
}

#[test]
fn test_jsx_element_with_expression_attr() {
    let ir = lower_jsx(r#"
        const myId = "hello";
        const x = <div id={myId} />;
    "#);
    assert!(ir.contains("\"div\""), "IR should contain \"div\" tag:\n{}", ir);
    assert!(ir.contains("object_literal"), "IR should contain ObjectLiteral for props:\n{}", ir);
}

#[test]
fn test_jsx_boolean_attribute() {
    let ir = lower_jsx(r#"const x = <input disabled />;"#);
    assert!(ir.contains("\"input\""), "IR should contain \"input\" tag:\n{}", ir);
    // Boolean attribute should produce true
    assert!(ir.contains("true"), "IR should contain true for boolean attr:\n{}", ir);
}

#[test]
fn test_jsx_multiple_attributes() {
    let ir = lower_jsx(r#"const x = <div a="1" b="2" c="3" />;"#);
    assert!(ir.contains("object_literal"), "IR should contain ObjectLiteral for props:\n{}", ir);
    assert!(ir.contains("\"1\""), "IR should contain attr value \"1\":\n{}", ir);
    assert!(ir.contains("\"2\""), "IR should contain attr value \"2\":\n{}", ir);
    assert!(ir.contains("\"3\""), "IR should contain attr value \"3\":\n{}", ir);
}

#[test]
fn test_jsx_no_attributes_produces_null_props() {
    let ir = lower_jsx(r#"const x = <div />;"#);
    // With no attributes, props should be null
    assert!(ir.contains("null"), "IR should contain null for empty props:\n{}", ir);
}

// ============================================================================
// Children
// ============================================================================

#[test]
fn test_jsx_text_child() {
    let ir = lower_jsx(r#"const x = <div>Hello</div>;"#);
    assert!(ir.contains("\"div\""), "IR should contain \"div\" tag:\n{}", ir);
    assert!(ir.contains("\"Hello\""), "IR should contain text child \"Hello\":\n{}", ir);
}

#[test]
fn test_jsx_expression_child() {
    let ir = lower_jsx(r#"
        const name = "world";
        const x = <div>{name}</div>;
    "#);
    assert!(ir.contains("\"div\""), "IR should contain \"div\" tag:\n{}", ir);
}

#[test]
fn test_jsx_nested_elements() {
    let ir = lower_jsx(r#"const x = <div><span>Hi</span></div>;"#);
    assert!(ir.contains("\"div\""), "IR should contain outer \"div\":\n{}", ir);
    assert!(ir.contains("\"span\""), "IR should contain inner \"span\":\n{}", ir);
    assert!(ir.contains("\"Hi\""), "IR should contain text \"Hi\":\n{}", ir);
}

#[test]
fn test_jsx_multiple_children() {
    let ir = lower_jsx(r#"const x = <div><span /><span /></div>;"#);
    // Should have two "span" string constants
    assert!(count_in(&ir, "\"span\"") >= 2, "IR should contain at least two \"span\" references:\n{}", ir);
}

#[test]
fn test_jsx_empty_expression_child_skipped() {
    // Empty expressions {} should be silently skipped
    let ir = lower_jsx(r#"const x = <div>{}</div>;"#);
    assert!(ir.contains("\"div\""), "IR should parse and contain \"div\":\n{}", ir);
}

// ============================================================================
// Component Elements
// ============================================================================

#[test]
fn test_jsx_component_uses_identifier() {
    // Uppercase element → identifier reference (not string)
    let ir = lower_jsx(r#"
        function Button(): void {}
        const x = <Button />;
    "#);
    // Should NOT contain "Button" as a string constant (it's an identifier)
    // The component should be resolved via function_map
    assert!(ir.contains("call"), "IR should contain a call instruction:\n{}", ir);
}

// ============================================================================
// Fragments
// ============================================================================

#[test]
fn test_jsx_fragment_basic() {
    let ir = lower_jsx(r#"const x = <>Hello</>;"#);
    assert!(ir.contains("\"Hello\""), "IR should contain text \"Hello\":\n{}", ir);
}

#[test]
fn test_jsx_fragment_multiple_children() {
    let ir = lower_jsx(r#"const x = <><span /><span /></>;"#);
    assert!(count_in(&ir, "\"span\"") >= 2, "IR should contain two span references:\n{}", ir);
}

// ============================================================================
// Namespaced Elements
// ============================================================================

#[test]
fn test_jsx_namespaced_tag() {
    let ir = lower_jsx(r#"const x = <svg:path />;"#);
    assert!(ir.contains("\"svg:path\""), "IR should contain \"svg:path\" string:\n{}", ir);
}

// ============================================================================
// Spread Attributes
// ============================================================================

#[test]
fn test_jsx_spread_attributes() {
    let ir = lower_jsx(r#"
        const props = {};
        const x = <div {...props} />;
    "#);
    // Spread should produce a native_call to JSON.merge
    assert!(ir.contains("native_call"), "IR should contain native_call for spread:\n{}", ir);
    assert!(ir.contains("JSON.merge"), "IR should contain JSON.merge for spread:\n{}", ir);
}

#[test]
fn test_jsx_mixed_spread_and_regular_attrs() {
    let ir = lower_jsx(r#"
        const props = {};
        const x = <div a="1" {...props} b="2" />;
    "#);
    assert!(ir.contains("\"1\""), "IR should contain attr value \"1\":\n{}", ir);
    assert!(ir.contains("\"2\""), "IR should contain attr value \"2\":\n{}", ir);
    // Spread should produce native_call for merging
    assert!(ir.contains("JSON.merge"), "IR should contain JSON.merge for spread:\n{}", ir);
}

// ============================================================================
// JSX Disabled (no config)
// ============================================================================

#[test]
fn test_jsx_disabled_produces_null() {
    // When JSX options are not set, JSX expressions should lower to null
    let ir = lower_no_jsx(r#"const x = <div />;"#);
    assert!(ir.contains("null"), "IR should contain null when JSX disabled:\n{}", ir);
}

// ============================================================================
// Custom Factory Name
// ============================================================================

#[test]
fn test_jsx_custom_factory() {
    let ir = lower_jsx_with(
        r#"
        function h(): void {}
        const x = <div />;
        "#,
        JsxOptions {
            factory: "h".to_string(),
            fragment: "Fragment".to_string(),
            development: false,
        },
    );
    // With factory="h", the Call should target the "h" function
    assert!(ir.contains("call fn"), "IR should contain a direct call to the custom factory:\n{}", ir);
}

// ============================================================================
// JSX as Attribute Value
// ============================================================================

#[test]
fn test_jsx_element_as_attr_value() {
    let ir = lower_jsx(r#"const x = <div content={<span />} />;"#);
    assert!(ir.contains("\"div\""), "IR should contain outer \"div\":\n{}", ir);
    assert!(ir.contains("\"span\""), "IR should contain inner \"span\":\n{}", ir);
}

// ============================================================================
// Integration: Full Pipeline Compile
// ============================================================================

#[test]
fn test_jsx_compiles_via_ir_pipeline() {
    use raya_engine::compiler::Compiler;
    use raya_engine::compiler::lower::JsxOptions;

    let source = r#"const x = <div className="test">Hello</div>;"#;
    let parser = Parser::new(source).expect("lexer error");
    let (module, interner) = parser.parse().expect("parse error");
    let type_ctx = TypeContext::new();

    let compiler = Compiler::new(type_ctx, &interner)
        .with_jsx(JsxOptions::default());

    // Should compile without error through the full IR pipeline
    let result = compiler.compile_via_ir(&module);
    assert!(result.is_ok(), "JSX should compile via IR: {:?}", result.err());
}

#[test]
fn test_jsx_fragment_compiles_via_ir_pipeline() {
    use raya_engine::compiler::Compiler;
    use raya_engine::compiler::lower::JsxOptions;

    let source = r#"const x = <>Hello</>;"#;
    let parser = Parser::new(source).expect("lexer error");
    let (module, interner) = parser.parse().expect("parse error");
    let type_ctx = TypeContext::new();

    let compiler = Compiler::new(type_ctx, &interner)
        .with_jsx(JsxOptions::default());

    let result = compiler.compile_via_ir(&module);
    assert!(result.is_ok(), "JSX fragment should compile via IR: {:?}", result.err());
}
