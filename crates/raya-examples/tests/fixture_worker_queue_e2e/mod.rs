mod common;

use common::*;
use raya_examples::worker_queue_entry;

#[test]
fn worker_queue_summary_contract() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("worker-queue-summary");

    let out = run_cli_script(&workspace, &worker_queue_entry(), &tmp_dir);
    assert_ok_run(&out);

    let summary = std::fs::read_to_string(tmp_dir.join("raya-examples-worker-queue/result.txt"))
        .expect("worker result");
    let fields = parse_summary(&summary);
    assert_eq!(
        fields.get("ok").map(String::as_str),
        Some("true"),
        "{summary}"
    );
    assert_eq!(
        fields.get("cancelled").map(String::as_str),
        Some("true"),
        "{summary}"
    );
    assert_eq!(
        fields.get("timeout").map(String::as_str),
        Some("true"),
        "{summary}"
    );
    assert_eq!(
        fields.get("count").map(String::as_str),
        Some("5"),
        "{summary}"
    );
    assert_eq!(
        fields.get("processed").map(String::as_str),
        Some("15"),
        "{summary}"
    );
    assert_eq!(
        fields.get("v1").map(String::as_str),
        Some("20"),
        "{summary}"
    );

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn worker_queue_repeatable() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("worker-queue-repeat");

    let out1 = run_cli_script(&workspace, &worker_queue_entry(), &tmp_dir);
    assert_ok_run(&out1);
    let out2 = run_cli_script(&workspace, &worker_queue_entry(), &tmp_dir);
    assert_ok_run(&out2);

    let summary = std::fs::read_to_string(tmp_dir.join("raya-examples-worker-queue/result.txt"))
        .expect("worker result");
    let fields = parse_summary(&summary);
    assert_eq!(
        fields.get("ok").map(String::as_str),
        Some("true"),
        "{summary}"
    );
    assert_eq!(
        fields.get("count").map(String::as_str),
        Some("5"),
        "{summary}"
    );
    assert_eq!(
        fields.get("processed").map(String::as_str),
        Some("15"),
        "{summary}"
    );
    assert_eq!(
        fields.get("v1").map(String::as_str),
        Some("20"),
        "{summary}"
    );

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn worker_queue_artifacts_contract() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("worker-queue-artifacts");

    let out = run_cli_script(&workspace, &worker_queue_entry(), &tmp_dir);
    assert_ok_run(&out);

    let dir = tmp_dir.join("raya-examples-worker-queue");
    let summary = std::fs::read_to_string(dir.join("result.txt")).expect("worker result");
    assert!(summary.contains("cancelled=true"), "{summary}");
    assert!(summary.contains("timeout=true"), "{summary}");
    assert_eq!(std::fs::read_dir(&dir).expect("read dir").count(), 1);

    let _ = std::fs::remove_dir_all(&tmp_dir);
}
