use crate::common::*;
use raya_examples::tcp_chat_entry;

#[test]
fn tcp_chat_summary_contract() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("tcp-chat-summary");

    let out = run_cli_script(&workspace, &tcp_chat_entry(), &tmp_dir);
    assert_ok_run(&out);

    let summary = std::fs::read_to_string(tmp_dir.join("raya-examples-tcp-chat/result.txt"))
        .expect("tcp result");
    let fields = parse_summary(&summary);
    assert_eq!(
        fields.get("ok").map(String::as_str),
        Some("true"),
        "{summary}"
    );
    assert_eq!(
        fields.get("rounds").map(String::as_str),
        Some("4"),
        "{summary}"
    );

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn tcp_chat_repeatable_no_hang() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("tcp-chat-repeat");

    // run twice in same process tree to ensure no lingering socket state/hang
    let out1 = run_cli_script(&workspace, &tcp_chat_entry(), &tmp_dir);
    assert_ok_run(&out1);
    let out2 = run_cli_script(&workspace, &tcp_chat_entry(), &tmp_dir);
    assert_ok_run(&out2);

    let summary = std::fs::read_to_string(tmp_dir.join("raya-examples-tcp-chat/result.txt"))
        .expect("tcp result");
    let fields = parse_summary(&summary);
    assert_eq!(
        fields.get("ok").map(String::as_str),
        Some("true"),
        "{summary}"
    );
    assert_eq!(
        fields.get("rounds").map(String::as_str),
        Some("4"),
        "{summary}"
    );

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn tcp_chat_result_shape_contract() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("tcp-chat-shape");

    let out = run_cli_script(&workspace, &tcp_chat_entry(), &tmp_dir);
    assert_ok_run(&out);

    let dir = tmp_dir.join("raya-examples-tcp-chat");
    let summary = std::fs::read_to_string(dir.join("result.txt")).expect("tcp result");
    let fields = parse_summary(&summary);
    assert_eq!(fields.len(), 2, "{summary}");
    assert!(fields.contains_key("ok"), "{summary}");
    assert!(fields.contains_key("rounds"), "{summary}");

    let _ = std::fs::remove_dir_all(&tmp_dir);
}
