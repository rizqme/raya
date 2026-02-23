//! End-to-end tests for std:encoding module

use super::harness::*;

#[test]
fn test_encoding_base32_roundtrip() {
    expect_bool_with_builtins(
        r#"
        import encoding from "std:encoding";
        import crypto from "std:crypto";
        const src: Buffer = crypto.fromHex("48656c6c6f");
        const enc: string = encoding.base32.encode(src);
        const out: Buffer = encoding.base32.decode(enc);
        return crypto.toHex(out) == "48656c6c6f";
    "#,
        true,
    );
}

#[test]
fn test_encoding_csv_headers_and_column() {
    expect_bool_with_builtins(
        r#"
        import encoding from "std:encoding";
        const table = encoding.csv.parseWithHeaders("name,age\nAlice,30\nBob,25");
        const headers = table.headers();
        const ages = table.column("age");
        const ok = table.rowCount() == 2 && headers.length == 2 && ages[0] == "30" && ages[1] == "25";
        table.release();
        return ok;
    "#,
        true,
    );
}

#[test]
fn test_encoding_json_build_and_read() {
    expect_bool_with_builtins(
        r#"
        import encoding from "std:encoding";
        const obj = encoding.json.newObject();
        obj.set("name", encoding.json.fromString("raya"));
        obj.set("ok", encoding.json.fromBool(true));
        const name = obj.get("name").string();
        const ok = obj.get("ok").bool();
        obj.release();
        return name == "raya" && ok;
    "#,
        true,
    );
}

#[test]
fn test_encoding_xml_parse() {
    expect_bool_with_builtins(
        r#"
        import encoding from "std:encoding";
        const root = encoding.xml.parse("<root><item id=\"1\">ok</item></root>");
        const item = root.child("item");
        const ok = root.tag() == "root" && item.text() == "ok" && item.attr("id") == "1";
        root.release();
        return ok;
    "#,
        true,
    );
}
