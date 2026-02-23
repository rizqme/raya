mod common;

use common::*;
use raya_examples::fault_injection_entry;

#[test]
fn fault_injection_summary_contract() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("fault-injection-summary");

    let out = run_cli_script(&workspace, &fault_injection_entry(), &tmp_dir);
    assert_ok_run(&out);

    let summary =
        std::fs::read_to_string(tmp_dir.join("raya-examples-fault-injection/result.txt"))
            .expect("fault result");
    let fields = parse_summary(&summary);
    assert_eq!(fields.get("ok").map(String::as_str), Some("true"), "{summary}");
    assert_eq!(fields.get("json").map(String::as_str), Some("true"), "{summary}");
    assert_eq!(fields.get("hex").map(String::as_str), Some("true"), "{summary}");
    assert_eq!(
        fields.get("missing").map(String::as_str),
        Some("true"),
        "{summary}"
    );
    assert_eq!(fields.get("panic").map(String::as_str), Some("true"), "{summary}");

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn fault_injection_repeatable() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("fault-injection-repeat");

    let out1 = run_cli_script(&workspace, &fault_injection_entry(), &tmp_dir);
    assert_ok_run(&out1);
    let out2 = run_cli_script(&workspace, &fault_injection_entry(), &tmp_dir);
    assert_ok_run(&out2);

    let summary =
        std::fs::read_to_string(tmp_dir.join("raya-examples-fault-injection/result.txt"))
            .expect("fault result");
    assert!(summary.contains("ok=true"), "{summary}");

    let _ = std::fs::remove_dir_all(&tmp_dir);
}
