mod common;

use common::*;
use raya_examples::todo_kv_entry;

#[test]
fn todo_kv_summary_contract() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("todo-kv-summary");

    let out = run_cli_script(&workspace, &todo_kv_entry(), &tmp_dir);
    assert_ok_run(&out);

    let summary = std::fs::read_to_string(tmp_dir.join("raya-examples-todo-kv/result.txt"))
        .expect("todo result");
    let fields = parse_summary(&summary);
    assert_eq!(fields.get("ok").map(String::as_str), Some("true"), "{summary}");
    assert_eq!(
        fields.get("recovered").map(String::as_str),
        Some("true"),
        "{summary}"
    );
    assert_eq!(fields.get("token").map(String::as_str), Some("true"), "{summary}");
    assert_eq!(fields.get("stream").map(String::as_str), Some("true"), "{summary}");
    assert_eq!(
        fields.get("missing").map(String::as_str),
        Some("true"),
        "{summary}"
    );

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn todo_kv_persistence_recovery() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("todo-kv-recovery");

    let out1 = run_cli_script(&workspace, &todo_kv_entry(), &tmp_dir);
    assert_ok_run(&out1);
    let out2 = run_cli_script(&workspace, &todo_kv_entry(), &tmp_dir);
    assert_ok_run(&out2);

    let db = std::fs::read_to_string(tmp_dir.join("raya-examples-todo-kv/kv.db")).expect("kv.db");
    assert!(db.contains("alpha=3"), "db content: {db}");
    assert!(!db.contains("beta=2"), "db content: {db}");

    let _ = std::fs::remove_dir_all(&tmp_dir);
}
