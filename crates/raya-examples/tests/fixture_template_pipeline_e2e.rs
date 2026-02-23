mod common;

use common::*;
use raya_examples::template_pipeline_entry;

#[test]
fn template_pipeline_summary_contract() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("template-pipeline-summary");

    let out = run_cli_script(&workspace, &template_pipeline_entry(), &tmp_dir);
    assert_ok_run(&out);

    let summary =
        std::fs::read_to_string(tmp_dir.join("raya-examples-template-pipeline/result.txt"))
            .expect("pipeline result");
    let fields = parse_summary(&summary);
    assert_eq!(fields.get("ok").map(String::as_str), Some("true"), "{summary}");
    assert_eq!(fields.get("glob").map(String::as_str), Some("true"), "{summary}");
    assert_eq!(
        fields.get("checksum").map(String::as_str),
        Some("true"),
        "{summary}"
    );
    assert_eq!(fields.get("zip").map(String::as_str), Some("true"), "{summary}");

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn template_pipeline_artifacts_exist() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("template-pipeline-artifacts");

    let out = run_cli_script(&workspace, &template_pipeline_entry(), &tmp_dir);
    assert_ok_run(&out);

    let dir = tmp_dir.join("raya-examples-template-pipeline");
    let report_a = std::fs::read_to_string(dir.join("reports/report-a.txt")).expect("report-a");
    let report_b = std::fs::read_to_string(dir.join("reports/report-b.txt")).expect("report-b");
    assert!(report_a.contains("Report A value=11"), "{report_a}");
    assert!(report_b.contains("Report B value=22"), "{report_b}");
    assert!(dir.join("reports.tar").exists());

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn template_pipeline_summary_entries_contract() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("template-pipeline-summary-entries");

    let out = run_cli_script(&workspace, &template_pipeline_entry(), &tmp_dir);
    assert_ok_run(&out);

    let summary =
        std::fs::read_to_string(tmp_dir.join("raya-examples-template-pipeline/result.txt"))
            .expect("pipeline result");
    let fields = parse_summary(&summary);
    assert_eq!(fields.get("ok").map(String::as_str), Some("true"), "{summary}");
    assert_eq!(fields.get("entries").map(String::as_str), Some("2"), "{summary}");

    let _ = std::fs::remove_dir_all(&tmp_dir);
}
