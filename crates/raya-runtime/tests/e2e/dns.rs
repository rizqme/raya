//! End-to-end tests for std:dns module

use super::harness::*;

#[test]
fn test_dns_lookup_localhost() {
    expect_bool_with_builtins(
        r#"
        import dns from "std:dns";
        const ips = dns.lookup("localhost");
        return ips.length > 0;
    "#,
        true,
    );
}

#[test]
fn test_dns_lookup4_localhost() {
    expect_bool_with_builtins(
        r#"
        import dns from "std:dns";
        const ips = dns.lookup4("localhost");
        return ips.length > 0;
    "#,
        true,
    );
}
