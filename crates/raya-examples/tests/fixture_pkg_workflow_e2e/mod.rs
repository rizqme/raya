use crate::common::*;
use raya_examples::pkg_workflow_entry;

#[test]
fn pkg_workflow_summary_contract() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("pkg-workflow-summary");

    let out = run_cli_script(&workspace, &pkg_workflow_entry(), &tmp_dir);
    assert_ok_run(&out);

    let summary = std::fs::read_to_string(tmp_dir.join("raya-examples-pkg-workflow/result.txt"))
        .expect("pkg result");
    let fields = parse_summary(&summary);
    assert_eq!(
        fields.get("ok").map(String::as_str),
        Some("true"),
        "{summary}"
    );
    assert_eq!(
        fields.get("dep").map(String::as_str),
        Some("true"),
        "{summary}"
    );
    assert_eq!(
        fields.get("lock").map(String::as_str),
        Some("true"),
        "{summary}"
    );
    assert_eq!(
        fields.get("semver").map(String::as_str),
        Some("true"),
        "{summary}"
    );
    assert_eq!(
        fields.get("path").map(String::as_str),
        Some("true"),
        "{summary}"
    );
    assert_eq!(
        fields.get("process").map(String::as_str),
        Some("true"),
        "{summary}"
    );

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn pkg_workflow_manifest_and_lock_present() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("pkg-workflow-files");

    let out = run_cli_script(&workspace, &pkg_workflow_entry(), &tmp_dir);
    assert_ok_run(&out);

    let dir = tmp_dir.join("raya-examples-pkg-workflow");
    let manifest = std::fs::read_to_string(dir.join("raya.toml")).expect("manifest");
    let lock = std::fs::read_to_string(dir.join("raya.lock")).expect("lock");
    assert!(manifest.contains("[dependencies]"));
    assert!(manifest.contains("local-lib"));
    assert!(lock.contains("name = \"local-lib\""));
    assert!(lock.contains("version = \"0.2.0\""));
    assert!(lock.contains("type = \"path\""));

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn pkg_workflow_dependency_source_contract() {
    let workspace = workspace_root();
    let tmp_dir = unique_tmp_dir("pkg-workflow-dep-contract");

    let out = run_cli_script(&workspace, &pkg_workflow_entry(), &tmp_dir);
    assert_ok_run(&out);

    let dir = tmp_dir.join("raya-examples-pkg-workflow");
    let dep_lib =
        std::fs::read_to_string(dir.join("deps/local-lib/src/lib.raya")).expect("dep lib source");
    let summary = std::fs::read_to_string(dir.join("result.txt")).expect("pkg result");
    assert!(dep_lib.contains("function answer(): number"), "{dep_lib}");
    assert!(summary.contains("dep=true"), "{summary}");
    assert!(summary.contains("semver=true"), "{summary}");

    let _ = std::fs::remove_dir_all(&tmp_dir);
}
