//! End-to-end tests for std:url module

use super::harness::*;

#[test]
fn test_url_parse_components() {
    expect_bool_with_builtins(
        r##"
        import { Url } from "std:url";
        const u = new Url("https://user:pass@example.com:8080/path/to?a=1&b=2#frag", "");
        return u.protocol() == "https:" &&
               u.hostname() == "example.com" &&
               u.port() == "8080" &&
               u.pathname() == "/path/to" &&
               u.search() == "?a=1&b=2" &&
               u.hash() == "#frag";
    "##,
        true,
    );
}

#[test]
fn test_url_search_params_manipulation() {
    expect_bool_with_builtins(
        r#"
        import { UrlSearchParams } from "std:url";
        const p = new UrlSearchParams("a=1&b=2");
        p.append("a", "3");
        p.set("b", "4");
        p.delete("missing");
        const allA = p.getAll("a");
        return p.get("b") == "4" && p.has("a") && allA.length == 2 && p.size() == 3;
    "#,
        true,
    );
}

#[test]
fn test_url_with_mutators() {
    expect_string_with_builtins(
        r##"
        import { Url } from "std:url";
        const base = new Url("https://example.com/old", "");
        const u = base.withPathname("/new").withSearch("?x=1").withHash("#ok");
        return u.href();
    "##,
        "https://example.com/new?x=1#ok",
    );
}

#[test]
fn test_url_encode_decode_roundtrip() {
    expect_bool_with_builtins(
        r#"
        const raw = "hello world";
        const enc = __NATIVE_CALL<string>("url.encode", raw);
        const dec = __NATIVE_CALL<string>("url.decode", enc);
        return dec == raw;
    "#,
        true,
    );
}
