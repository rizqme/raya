//! `raya test` — Discover, run, and report tests.

use crate::output::{self, StyledOutput};
use raya_runtime::test_runner::{self, TestFileResult};
use raya_runtime::RuntimeOptions;
use std::path::PathBuf;
use std::time::Instant;
use termcolor::Color;

/// Arguments for the test command.
#[allow(dead_code)]
pub struct TestArgs {
    pub filter: Option<String>,
    pub watch: bool,
    pub coverage: bool,
    pub bail: bool,
    pub timeout: u64,
    pub concurrency: usize,
    pub reporter: String,
    pub file: Option<String>,
    pub update_snapshots: bool,
    pub color: String,
}

pub fn execute(args: TestArgs) -> anyhow::Result<()> {
    let color_choice = output::resolve_color_choice(Some(&args.color));
    let mut out = StyledOutput::new(color_choice);

    // Discover test files
    let test_files = discover_test_files(args.file.as_deref())?;

    if test_files.is_empty() {
        out.warning("No test files found.");
        out.newline();
        out.dim("  Looked for: **/*.test.raya, **/*_test.raya, **/__tests__/**/*.raya");
        out.newline();
        return Ok(());
    }

    // Build runtime options
    let options = RuntimeOptions {
        timeout: args.timeout,
        ..Default::default()
    };

    let overall_start = Instant::now();
    let mut all_results: Vec<TestFileResult> = Vec::new();
    let mut any_failure = false;

    // Run test files
    // TODO: parallel execution when concurrency > 1
    for test_file in &test_files {
        let result = test_runner::run_test_file(test_file, &options);

        match result {
            Ok(file_result) => {
                // Apply name filter
                let file_result = if let Some(ref pattern) = args.filter {
                    filter_results(file_result, pattern)
                } else {
                    file_result
                };

                if file_result.has_failures() {
                    any_failure = true;
                }

                // Print file results (streaming)
                match args.reporter.as_str() {
                    "dot" => print_dot_results(&mut out, &file_result),
                    "json" => print_json_results(&file_result),
                    _ => print_default_results(&mut out, &file_result),
                }

                all_results.push(file_result);

                if any_failure && args.bail {
                    break;
                }
            }
            Err(e) => {
                any_failure = true;
                out.newline();
                out.fail_badge();
                out.plain(&format!("  {} ", test_file.display()));
                out.newline();
                out.error(&format!("  Compilation/execution error: {}", e));
                out.newline();

                if args.bail {
                    break;
                }
            }
        }
    }

    let overall_duration = overall_start.elapsed();

    // Print failure details
    if args.reporter != "json" {
        print_failure_details(&mut out, &all_results);
    }

    // Print summary
    match args.reporter.as_str() {
        "json" => print_json_summary(&all_results, overall_duration.as_secs_f64()),
        _ => print_summary(&mut out, &all_results, &test_files, overall_duration.as_secs_f64()),
    }

    if any_failure {
        std::process::exit(1);
    }

    Ok(())
}

// ── Test Discovery ───────────────────────────────────────────────────────

fn discover_test_files(file_filter: Option<&str>) -> anyhow::Result<Vec<PathBuf>> {
    let cwd = std::env::current_dir()?;
    let mut files = Vec::new();

    if let Some(filter) = file_filter {
        // User-specified glob
        for path in (glob::glob(filter)?).flatten() {
            if path.extension().and_then(|e| e.to_str()) == Some("raya") {
                files.push(path);
            }
        }
    } else {
        // Default discovery patterns
        let patterns = [
            "**/*.test.raya",
            "**/*_test.raya",
            "**/__tests__/**/*.raya",
        ];
        let excludes = ["node_modules", ".raya-cache", "dist", "target", ".worktrees"];

        for pattern in &patterns {
            let full_pattern = cwd.join(pattern);
            if let Ok(entries) = glob::glob(full_pattern.to_str().unwrap_or("")) {
                for path in entries.flatten() {
                    // Check excludes
                    let path_str = path.to_string_lossy();
                    if excludes.iter().any(|ex| path_str.contains(ex)) {
                        continue;
                    }
                    if !files.contains(&path) {
                        files.push(path);
                    }
                }
            }
        }
    }

    files.sort();
    Ok(files)
}

// ── Result Filtering ─────────────────────────────────────────────────────

fn filter_results(mut result: TestFileResult, pattern: &str) -> TestFileResult {
    result.results.results.retain(|r| r.name.contains(pattern));
    result
}

// ── Default Reporter ─────────────────────────────────────────────────────

fn print_default_results(out: &mut StyledOutput, result: &TestFileResult) {
    let cwd = std::env::current_dir().unwrap_or_default();
    let display_path = result
        .file
        .strip_prefix(&cwd)
        .unwrap_or(&result.file)
        .display();

    if result.results.results.is_empty() && result.execution_error.is_none() {
        return;
    }

    // File badge
    out.newline();
    if result.has_failures() {
        out.fail_badge();
    } else {
        out.pass_badge();
    }

    // File path and stats
    out.plain(&format!("  {}", display_path));
    out.dim(&format!(
        " ({} tests, {:.0}ms)",
        result.total(),
        result.duration_ms()
    ));
    out.newline();

    // Individual test results
    for test in &result.results.results {
        if test.passed {
            out.write_styled("   ✓ ", Some(Color::Green), false, false);
            out.write_styled(&test.name, Some(Color::Green), false, false);
        } else {
            out.write_styled("   ✗ ", Some(Color::Red), true, false);
            out.write_styled(&test.name, Some(Color::Red), true, false);
        }
        out.newline();
    }
}

// ── Dot Reporter ─────────────────────────────────────────────────────────

fn print_dot_results(out: &mut StyledOutput, result: &TestFileResult) {
    let cwd = std::env::current_dir().unwrap_or_default();
    let display_path = result
        .file
        .strip_prefix(&cwd)
        .unwrap_or(&result.file)
        .display();

    for test in &result.results.results {
        if test.passed {
            out.write_styled(".", Some(Color::Green), false, false);
        } else {
            out.write_styled("F", Some(Color::Red), true, false);
        }
    }
    out.dim(&format!("  {}", display_path));
    out.newline();
}

// ── JSON Reporter ────────────────────────────────────────────────────────

fn print_json_results(result: &TestFileResult) {
    let cwd = std::env::current_dir().unwrap_or_default();
    let display_path = result
        .file
        .strip_prefix(&cwd)
        .unwrap_or(&result.file)
        .display()
        .to_string();

    for test in &result.results.results {
        let status = if test.passed { "passed" } else { "failed" };
        let error = test
            .error_message
            .as_deref()
            .unwrap_or("")
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n");
        println!(
            r#"{{"file":"{}","name":"{}","status":"{}","duration_ms":{:.2},"error":"{}"}}"#,
            display_path,
            test.name.replace('"', "\\\""),
            status,
            test.duration_ms,
            error
        );
    }
}

fn print_json_summary(results: &[TestFileResult], duration_secs: f64) {
    let total_passed: usize = results.iter().map(|r| r.passed()).sum();
    let total_failed: usize = results.iter().map(|r| r.failed()).sum();
    let total = total_passed + total_failed;
    println!(
        r#"{{"summary":true,"total":{},"passed":{},"failed":{},"duration_secs":{:.3}}}"#,
        total, total_passed, total_failed, duration_secs
    );
}

// ── Failure Details ──────────────────────────────────────────────────────

fn print_failure_details(out: &mut StyledOutput, results: &[TestFileResult]) {
    let mut has_failures = false;

    for file_result in results {
        for test in &file_result.results.results {
            if test.passed {
                continue;
            }

            if !has_failures {
                has_failures = true;
                out.newline();
                out.dim("──────────────────────────────────────────");
                out.newline();
                out.newline();
            }

            let cwd = std::env::current_dir().unwrap_or_default();
            let display_path = file_result
                .file
                .strip_prefix(&cwd)
                .unwrap_or(&file_result.file)
                .display();

            out.write_styled("  ● ", Some(Color::Red), true, false);
            out.write_styled(&test.name, Some(Color::Red), true, false);
            out.newline();
            out.newline();

            if let Some(ref msg) = test.error_message {
                for line in msg.lines() {
                    out.plain("    ");
                    if line.trim_start().starts_with("Expected:") {
                        out.write_styled(line, Some(Color::Green), false, false);
                    } else if line.trim_start().starts_with("Received:") {
                        out.write_styled(line, Some(Color::Red), false, false);
                    } else {
                        out.plain(line);
                    }
                    out.newline();
                }
            }

            out.newline();
            out.dim(&format!("    at {}", display_path));
            out.newline();
            out.newline();
        }
    }
}

// ── Summary ──────────────────────────────────────────────────────────────

fn print_summary(
    out: &mut StyledOutput,
    results: &[TestFileResult],
    test_files: &[PathBuf],
    duration_secs: f64,
) {
    let total_passed: usize = results.iter().map(|r| r.passed()).sum();
    let total_failed: usize = results.iter().map(|r| r.failed()).sum();
    let total = total_passed + total_failed;
    let files_failed = results.iter().filter(|r| r.has_failures()).count();
    let files_passed = results.len() - files_failed;

    out.newline();
    out.dim("──────────────────────────────────────────");
    out.newline();

    // Tests line
    out.bold("Tests:  ");
    if total_failed > 0 {
        out.error(&format!("{} failed", total_failed));
        out.plain(", ");
    }
    if total_passed > 0 {
        out.success(&format!("{} passed", total_passed));
        out.plain(", ");
    }
    out.bold(&format!("{} total", total));
    out.newline();

    // Files line
    out.bold("Files:  ");
    if files_failed > 0 {
        out.error(&format!("{} failed", files_failed));
        out.plain(", ");
    }
    if files_passed > 0 {
        out.success(&format!("{} passed", files_passed));
        out.plain(", ");
    }
    out.bold(&format!("{} total", test_files.len()));
    out.newline();

    // Time line
    out.bold("Time:   ");
    out.dim(&format!("{:.2}s", duration_secs));
    out.newline();
}
