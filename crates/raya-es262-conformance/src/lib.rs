use anyhow::{Context, Result};
use clap::Parser;
use raya_runtime::{BuiltinMode, Runtime, RuntimeOptions, TypeMode};
use regex::Regex;
use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Instant;

const HARNESS_CORE_PRELUDE: &str = r#"
class Test262Error extends Error {
    constructor(message) {
        super(message);
    }
}

function $ERROR(message) {
    if (message == null) {
        throw new Test262Error("");
    }
    throw new Test262Error(String(message));
}

function $DONOTEVALUATE() {
    throw new Test262Error("This statement should not be evaluated.");
}
"#;

const ASSERT_HELPER_PRELUDE: &str = r#"

function __assert(mustBeTrue, message) {
    if (mustBeTrue === true) {
        return;
    }
    if (message == null) {
        $ERROR("Expected assertion to be truthy");
    }
    $ERROR(String(message));
}
"#;

const SAME_VALUE_HELPER_PRELUDE: &str = r#"

function __isSameValue(a, b) {
    if (a === b) {
        if (a === 0) {
            return (1 / a) === (1 / b);
        }
        return true;
    }
    return a !== a && b !== b;
}

function __assert_sameValue(actual, expected, message) {
    if (__isSameValue(actual, expected)) {
        return;
    }
    if (message == null) {
        $ERROR("Expected SameValue");
    }
    $ERROR(String(message));
}

function __assert_notSameValue(actual, expected, message) {
    if (!__isSameValue(actual, expected)) {
        return;
    }
    if (message == null) {
        $ERROR("Expected different values");
    }
    $ERROR(String(message));
}
"#;

const COMPARE_ARRAY_HELPER_PRELUDE: &str = r#"

function __compareArray(actual, expected) {
    if (actual == null || expected == null) {
        return false;
    }
    if (actual.length !== expected.length) {
        return false;
    }
    for (let i = 0; i < actual.length; i = i + 1) {
        if (!__isSameValue(actual[i], expected[i])) {
            return false;
        }
    }
    return true;
}

function __assert_compareArray(actual, expected, message) {
    if (__compareArray(actual, expected)) {
        return;
    }
    if (message == null) {
        $ERROR("Expected arrays to compare equal");
    }
    $ERROR(String(message));
}
"#;

const ASSERT_THROWS_HELPER_PRELUDE: &str = r#"

function __assert_throws(expectedErrorConstructor, func, message) {
    try {
        func();
    } catch (thrown) {
        if (expectedErrorConstructor != null) {
            let actualConstructor = thrown == null ? thrown : thrown.constructor;
            if (actualConstructor !== expectedErrorConstructor) {
                if (message == null) {
                    $ERROR("Expected function to throw the requested constructor");
                }
                $ERROR(String(message));
            }
        }
        return;
    }
    if (message == null) {
        $ERROR("Expected function to throw");
    }
    $ERROR(String(message));
}
"#;

const HOST_262_PRELUDE: &str = r#"
const __262_main_Reflect = Reflect;
const __262_indirect_eval = eval;

function __262_runInRealm(realmGlobal, source) {
    if (typeof source !== "string") {
        return source;
    }

    const trimmed = source.trim();
    if (trimmed.startsWith("var ")) {
        let remainder = trimmed.slice(4).trim();
        if (remainder.endsWith(";")) {
            remainder = remainder.slice(0, -1).trim();
        }
        const eqIndex = remainder.indexOf("=");
        const name = (eqIndex === -1 ? remainder : remainder.slice(0, eqIndex)).trim();
        const initializer = eqIndex === -1 ? null : remainder.slice(eqIndex + 1).trim();
        if (/^[A-Za-z_$][A-Za-z0-9_$]*$/.test(name)) {
            if (initializer == null || initializer === "") {
                realmGlobal[name] = undefined;
                return undefined;
            }
            const initSource = String(initializer);
            return (function() {
            const mainGlobal = globalThis;
            const touched = Object.getOwnPropertyNames(realmGlobal);
            const saved = {};
            const had = {};

            for (let i = 0; i < touched.length; i = i + 1) {
                const key = touched[i];
                had[key] = Object.prototype.hasOwnProperty.call(mainGlobal, key);
                saved[key] = had[key] ? Object.getOwnPropertyDescriptor(mainGlobal, key) : undefined;
                Object.defineProperty(mainGlobal, key, {
                    value: realmGlobal[key],
                    writable: true,
                    enumerable: true,
                    configurable: true,
                });
            }

            try {
                realmGlobal[name] = __262_indirect_eval("(" + initSource + ")");
                return undefined;
            } finally {
                for (let i = 0; i < touched.length; i = i + 1) {
                    const key = touched[i];
                    if (had[key]) {
                        Object.defineProperty(mainGlobal, key, saved[key]);
                    } else {
                        delete mainGlobal[key];
                    }
                }
                if (!Object.prototype.hasOwnProperty.call(had, name)) {
                    delete mainGlobal[name];
                }
            }
            })();
        }
    }

    const mainGlobal = globalThis;
    const touched = Object.getOwnPropertyNames(realmGlobal);
    const saved = {};
    const had = {};

    for (let i = 0; i < touched.length; i = i + 1) {
        const key = touched[i];
        had[key] = Object.prototype.hasOwnProperty.call(mainGlobal, key);
        saved[key] = had[key] ? Object.getOwnPropertyDescriptor(mainGlobal, key) : undefined;
        Object.defineProperty(mainGlobal, key, {
            value: realmGlobal[key],
            writable: true,
            enumerable: true,
            configurable: true,
        });
    }

    try {
        return __262_indirect_eval(source);
    } finally {
        const currentKeys = Object.getOwnPropertyNames(mainGlobal);
        for (let i = 0; i < currentKeys.length; i = i + 1) {
            const key = currentKeys[i];
            if (key === "$262") {
                continue;
            }
            if (!had[key] || Object.prototype.hasOwnProperty.call(realmGlobal, key)) {
                const descriptor = Object.getOwnPropertyDescriptor(mainGlobal, key);
                if (descriptor !== undefined) {
                    Object.defineProperty(realmGlobal, key, descriptor);
                }
            }
        }

        for (let i = 0; i < currentKeys.length; i = i + 1) {
            const key = currentKeys[i];
            if (key === "$262") {
                continue;
            }
            if (!had[key] && touched.indexOf(key) === -1) {
                delete mainGlobal[key];
            }
        }

        for (let i = 0; i < touched.length; i = i + 1) {
            const key = touched[i];
            if (had[key]) {
                Object.defineProperty(mainGlobal, key, saved[key]);
            } else {
                delete mainGlobal[key];
            }
        }
    }
}

function __262_createRealm() {
    const realmGlobal = {
        Object: Object,
        Array: Array,
        Function: Function,
        Error: Error,
        AggregateError: AggregateError,
        EvalError: EvalError,
        RangeError: RangeError,
        ReferenceError: ReferenceError,
        RegExp: RegExp,
        SyntaxError: SyntaxError,
        URIError: URIError,
        Symbol: Symbol,
        TypeError: TypeError,
        Reflect: __262_main_Reflect,
    };
    realmGlobal.globalThis = realmGlobal;
    realmGlobal.eval = function(source) {
        return __262_runInRealm(realmGlobal, source);
    };
    return { global: realmGlobal };
}

function __262_evalScript(source) {
    return __262_indirect_eval(source);
}

function __262_detachArrayBuffer(buffer) {
    if (buffer == null || typeof buffer.detach !== "function") {
        throw new TypeError("$262.detachArrayBuffer requires a detachable ArrayBuffer");
    }
    buffer.detach();
}

const $262 = {
    createRealm: __262_createRealm,
    evalScript: __262_evalScript,
    detachArrayBuffer: __262_detachArrayBuffer,
};
"#;

const IS_CONSTRUCTOR_SHIM: &str = r#"
class __Test262ConstructorProbe {}

function isConstructor(f) {
    if (typeof f !== "function") {
        throw new Test262Error("isConstructor invoked with a non-function value");
    }

    try {
        Reflect.construct(__Test262ConstructorProbe, [], f);
    } catch (_e) {
        return false;
    }
    return true;
}
"#;

const FN_GLOBAL_OBJECT_SHIM: &str = r#"
var __globalObject = Function("return this;")();
function fnGlobalObject() {
    return __globalObject;
}
"#;

const DEFAULT_EXCLUDED_SEGMENTS: &[&str] = &[
    "annexB",
    "intl402",
    "Atomics",
    "SharedArrayBuffer",
    "Temporal",
    "ShadowRealm",
];

#[derive(Debug, Parser)]
#[command(name = "raya-es262-conformance")]
#[command(about = "Run a best-effort ES262 subset of Test262 through Raya")]
pub struct Args {
    #[arg(long, value_name = "PATH")]
    pub root: Option<PathBuf>,

    #[arg(long)]
    pub filter: Option<String>,

    #[arg(long, value_name = "N")]
    pub from: Option<usize>,

    #[arg(long, value_name = "N")]
    pub to: Option<usize>,

    #[arg(long)]
    pub limit: Option<usize>,

    #[arg(long)]
    pub fail_fast: bool,

    #[arg(long)]
    pub verbose: bool,

    #[arg(long)]
    pub show_skips: bool,

    #[arg(long)]
    pub list: bool,

    #[arg(long, value_name = "N")]
    pub jobs: Option<usize>,

    #[arg(long = "exclude-prefix", value_name = "PATH")]
    pub exclude_prefixes: Vec<String>,

    #[arg(long = "exclude-segment", value_name = "NAME")]
    pub exclude_segments: Vec<String>,

    #[arg(value_name = "PATH")]
    pub selectors: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct Frontmatter {
    pub description: Option<String>,
    pub includes: Vec<String>,
    pub flags: Vec<String>,
    pub features: Vec<String>,
    pub negative: Option<Negative>,
}

#[derive(Debug, Clone, Default)]
pub struct Negative {
    pub phase: Option<String>,
    pub error_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TestCase {
    pub absolute_path: PathBuf,
    pub relative_path: PathBuf,
}

#[derive(Debug, Clone)]
struct LoadedCase {
    pub metadata: Frontmatter,
    pub source: String,
}

#[derive(Debug, Clone)]
pub enum TestOutcome {
    Passed,
    Failed(String),
    Skipped(String),
}

#[derive(Debug, Clone)]
struct CaseRunResult {
    outcome: TestOutcome,
    metadata: Frontmatter,
}

#[derive(Debug, Default)]
pub struct RunSummary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
}

impl RunSummary {
    fn record(&mut self, outcome: &TestOutcome) {
        self.total += 1;
        match outcome {
            TestOutcome::Passed => self.passed += 1,
            TestOutcome::Failed(_) => self.failed += 1,
            TestOutcome::Skipped(_) => self.skipped += 1,
        }
    }
}

pub fn main_entry() -> Result<i32> {
    run(Args::parse())
}

pub fn run(args: Args) -> Result<i32> {
    let root = args
        .root
        .unwrap_or_else(default_test262_root)
        .canonicalize()
        .with_context(|| "failed to resolve Test262 root")?;
    let test_root = root.join("test");
    if !test_root.is_dir() {
        anyhow::bail!(
            "expected Test262 tests under {}, but that directory does not exist",
            test_root.display()
        );
    }

    let mut exclude_segments = DEFAULT_EXCLUDED_SEGMENTS
        .iter()
        .map(|name| (*name).to_string())
        .collect::<BTreeSet<_>>();
    exclude_segments.extend(args.exclude_segments);

    let mut cases = discover_cases(
        &root,
        &args.selectors,
        &args.exclude_prefixes,
        &exclude_segments,
    )?;
    if let Some(filter) = &args.filter {
        cases.retain(|case| case.relative_path.to_string_lossy().contains(filter));
    }
    if args.from == Some(0) {
        anyhow::bail!("--from is 1-based and must be >= 1");
    }
    if args.to == Some(0) {
        anyhow::bail!("--to is 1-based and must be >= 1");
    }
    if let (Some(from), Some(to)) = (args.from, args.to) {
        if from > to {
            anyhow::bail!("--from ({from}) must be <= --to ({to})");
        }
    }
    if args.from.is_some() || args.to.is_some() {
        let from = args.from.unwrap_or(1);
        let start = from.saturating_sub(1);
        let end = args.to.unwrap_or(cases.len()).min(cases.len());
        if start >= cases.len() || start >= end {
            cases.clear();
        } else {
            cases = cases.into_iter().skip(start).take(end - start).collect();
        }
    }
    if let Some(limit) = args.limit {
        cases.truncate(limit);
    }

    if args.list {
        for case in &cases {
            println!("{}", case.relative_path.display());
        }
        return Ok(0);
    }

    let started = Instant::now();
    let mut summary = RunSummary::default();
    let requested_jobs = args.jobs.unwrap_or_else(default_job_count);
    let effective_jobs = requested_jobs.max(1).min(cases.len().max(1));

    if args.fail_fast || effective_jobs == 1 || cases.len() <= 1 {
        let runtime = build_case_runtime();
        for (index, case) in cases.iter().enumerate() {
            if args.verbose || args.fail_fast {
                eprintln!(
                    "RUN {}/{} {}",
                    index + 1,
                    cases.len(),
                    case.relative_path.display()
                );
                let _ = std::io::stderr().flush();
            }

            let result = run_case(&runtime, &root, case);
            summary.record(&result.outcome);

            match &result.outcome {
                TestOutcome::Passed if args.verbose => {
                    println!("PASS {}", case.relative_path.display());
                }
                TestOutcome::Failed(message) => {
                    eprintln!(
                        "{}",
                        format_failure_report(
                            index + 1,
                            cases.len(),
                            case,
                            &result.metadata,
                            message,
                        )
                    );
                    if args.fail_fast {
                        break;
                    }
                }
                TestOutcome::Skipped(reason) if args.show_skips => {
                    println!("SKIP {}: {}", case.relative_path.display(), reason);
                }
                _ => {}
            }
        }
    } else {
        let outcomes = run_cases_parallel(&root, &cases, effective_jobs);
        for (index, (case, result)) in cases.iter().zip(outcomes.iter()).enumerate() {
            summary.record(&result.outcome);
            match &result.outcome {
                TestOutcome::Passed if args.verbose => {
                    println!("PASS {}", case.relative_path.display());
                }
                TestOutcome::Failed(message) => {
                    eprintln!(
                        "{}",
                        format_failure_report(
                            index + 1,
                            cases.len(),
                            case,
                            &result.metadata,
                            message,
                        )
                    );
                }
                TestOutcome::Skipped(reason) if args.show_skips => {
                    println!("SKIP {}: {}", case.relative_path.display(), reason);
                }
                _ => {}
            }
        }
    }

    println!(
        "es262: total={} passed={} failed={} skipped={} elapsed={:.2?}",
        summary.total,
        summary.passed,
        summary.failed,
        summary.skipped,
        started.elapsed()
    );

    Ok(if summary.failed == 0 { 0 } else { 1 })
}

pub fn default_test262_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.join("../../vendor/test262")
}

fn build_case_runtime() -> Runtime {
    Runtime::with_options(RuntimeOptions {
        builtin_mode: BuiltinMode::NodeCompat,
        type_mode: Some(TypeMode::Js),
        threads: 1,
        max_preemptions: Some(5_000),
        preempt_threshold_ms: Some(250),
        no_jit: true,
        jit_threshold: 32,
        ..Default::default()
    })
}

fn default_job_count() -> usize {
    thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
}

fn run_cases_parallel(root: &Path, cases: &[TestCase], jobs: usize) -> Vec<CaseRunResult> {
    let next_index = AtomicUsize::new(0);
    let results = Mutex::new(vec![None; cases.len()]);

    thread::scope(|scope| {
        for _ in 0..jobs {
            scope.spawn(|| {
                let runtime = build_case_runtime();
                loop {
                    let index = next_index.fetch_add(1, Ordering::Relaxed);
                    if index >= cases.len() {
                        break;
                    }
                    let outcome = run_case(&runtime, root, &cases[index]);
                    results.lock().unwrap()[index] = Some(outcome);
                }
            });
        }
    });

    results
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|outcome| outcome.expect("parallel case result should be populated"))
        .collect()
}

fn discover_cases(
    root: &Path,
    selectors: &[PathBuf],
    exclude_prefixes: &[String],
    exclude_segments: &BTreeSet<String>,
) -> Result<Vec<TestCase>> {
    let mut candidates = Vec::new();
    if selectors.is_empty() {
        collect_js_files(
            root,
            &root.join("test"),
            &mut candidates,
            exclude_prefixes,
            exclude_segments,
        )?;
    } else {
        for selector in selectors {
            let path = resolve_selector_path(root, selector);
            if path.is_dir() {
                collect_js_files(
                    root,
                    &path,
                    &mut candidates,
                    exclude_prefixes,
                    exclude_segments,
                )?;
            } else if path.is_file() {
                let relative_path = path.strip_prefix(root).unwrap_or(&path);
                if is_excluded_relative_path(relative_path, exclude_prefixes, exclude_segments) {
                    continue;
                }
                candidates.push(path);
            } else {
                anyhow::bail!(
                    "selector does not exist under test262 root: {}",
                    selector.display()
                );
            }
        }
    }

    candidates.sort();
    Ok(candidates
        .into_iter()
        .map(|absolute_path| TestCase {
            relative_path: absolute_path
                .strip_prefix(root)
                .unwrap_or(&absolute_path)
                .to_path_buf(),
            absolute_path,
        })
        .collect())
}

fn resolve_selector_path(root: &Path, selector: &Path) -> PathBuf {
    let direct = root.join(selector);
    if direct.exists() {
        return direct;
    }

    let selector_text = selector.to_string_lossy();
    if let Some(rest) = selector_text.strip_prefix("test/expressions") {
        let alias = root.join(format!("test/language/expressions{rest}"));
        if alias.exists() {
            return alias;
        }
    }
    if let Some(rest) = selector_text.strip_prefix("test/statements") {
        let alias = root.join(format!("test/language/statements{rest}"));
        if alias.exists() {
            return alias;
        }
    }

    direct
}

fn collect_js_files(
    root: &Path,
    dir: &Path,
    out: &mut Vec<PathBuf>,
    exclude_prefixes: &[String],
    exclude_segments: &BTreeSet<String>,
) -> Result<()> {
    for entry in
        fs::read_dir(dir).with_context(|| format!("failed to read directory {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let relative_path = path.strip_prefix(root).unwrap_or(&path);
        if is_excluded_relative_path(relative_path, exclude_prefixes, exclude_segments) {
            continue;
        }
        if path.is_dir() {
            collect_js_files(root, &path, out, exclude_prefixes, exclude_segments)?;
            continue;
        }
        let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        if ext == "js" || ext == "mjs" {
            out.push(path);
        }
    }
    Ok(())
}

fn is_excluded_relative_path(
    relative_path: &Path,
    exclude_prefixes: &[String],
    exclude_segments: &BTreeSet<String>,
) -> bool {
    let relative = relative_path.to_string_lossy();
    if exclude_prefixes
        .iter()
        .any(|prefix| relative.starts_with(prefix))
    {
        return true;
    }

    relative_path.components().any(|component| {
        let Component::Normal(name) = component else {
            return false;
        };
        exclude_segments.contains(&name.to_string_lossy().to_string())
    })
}

fn load_case(case: &TestCase) -> Result<LoadedCase> {
    let source = fs::read_to_string(&case.absolute_path)
        .with_context(|| format!("failed to read {}", case.absolute_path.display()))?;
    let (metadata, source) = parse_frontmatter_and_body(&source);
    Ok(LoadedCase { metadata, source })
}

fn parse_frontmatter_and_body(source: &str) -> (Frontmatter, String) {
    let Some(start) = source.find("/*---") else {
        return (Frontmatter::default(), source.to_string());
    };
    let after_start = start + "/*---".len();
    let Some(end_rel) = source[after_start..].find("---*/") else {
        return (Frontmatter::default(), source.to_string());
    };
    let end = after_start + end_rel;
    let frontmatter = &source[after_start..end];
    let body = format!("{}{}", &source[..start], &source[end + "---*/".len()..]);
    (parse_frontmatter(frontmatter), body)
}

fn parse_frontmatter(frontmatter: &str) -> Frontmatter {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Section {
        None,
        Includes,
        Flags,
        Features,
        Negative,
    }

    let mut metadata = Frontmatter::default();
    let mut section = Section::None;

    for raw_line in frontmatter.lines() {
        let line = raw_line.trim_end();
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(value) = trimmed.strip_prefix("description:") {
            metadata.description = Some(value.trim().trim_matches('"').to_string());
            section = Section::None;
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("includes:") {
            metadata.includes.extend(parse_inline_list(value));
            section = if value.trim().starts_with('[') {
                Section::None
            } else {
                Section::Includes
            };
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("flags:") {
            metadata.flags.extend(parse_inline_list(value));
            section = if value.trim().starts_with('[') {
                Section::None
            } else {
                Section::Flags
            };
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("features:") {
            metadata.features.extend(parse_inline_list(value));
            section = if value.trim().starts_with('[') {
                Section::None
            } else {
                Section::Features
            };
            continue;
        }
        if trimmed.starts_with("negative:") {
            metadata.negative.get_or_insert_with(Negative::default);
            section = Section::Negative;
            continue;
        }

        if let Some(value) = trimmed.strip_prefix('-') {
            let item = value.trim().trim_matches('"').to_string();
            match section {
                Section::Includes => metadata.includes.push(item),
                Section::Flags => metadata.flags.push(item),
                Section::Features => metadata.features.push(item),
                Section::None | Section::Negative => {}
            }
            continue;
        }

        if section == Section::Negative {
            if let Some(value) = trimmed.strip_prefix("phase:") {
                metadata
                    .negative
                    .get_or_insert_with(Negative::default)
                    .phase = Some(value.trim().to_string());
                continue;
            }
            if let Some(value) = trimmed.strip_prefix("type:") {
                metadata
                    .negative
                    .get_or_insert_with(Negative::default)
                    .error_type = Some(value.trim().to_string());
                continue;
            }
        }
    }

    metadata
}

fn parse_inline_list(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
        return Vec::new();
    }
    trimmed[1..trimmed.len() - 1]
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| item.trim_matches('"').to_string())
        .collect()
}

fn run_case(runtime: &Runtime, root: &Path, case: &TestCase) -> CaseRunResult {
    let loaded = match load_case(case) {
        Ok(loaded) => loaded,
        Err(error) => {
            return CaseRunResult {
                outcome: TestOutcome::Failed(format!("failed to load case: {error:#}")),
                metadata: Frontmatter::default(),
            };
        }
    };

    let transformed = match prepare_case_source(root, &loaded) {
        Ok(source) => source,
        Err(reason) => {
            return CaseRunResult {
                outcome: TestOutcome::Skipped(reason),
                metadata: loaded.metadata,
            };
        }
    };

    let negative_phase = loaded
        .metadata
        .negative
        .as_ref()
        .and_then(|negative| negative.phase.as_deref());
    let expected_error = loaded
        .metadata
        .negative
        .as_ref()
        .and_then(|negative| negative.error_type.as_deref());

    let temp_path = case_artifact_path(case);

    let outcome = match negative_phase {
        Some("parse") | Some("resolution") => {
            match runtime.compile_program_source_at_path(&transformed, &temp_path) {
                Ok(_) => TestOutcome::Failed("expected compilation to fail".to_string()),
                Err(error) => {
                    if matches_expected_error(&error.to_string(), expected_error) {
                        TestOutcome::Passed
                    } else {
                        TestOutcome::Failed(format!("unexpected compilation error: {}", error))
                    }
                }
            }
        }
        Some("runtime") => match execute_case_program(runtime, &temp_path, &transformed) {
            Ok(()) => TestOutcome::Failed("expected runtime failure".to_string()),
            Err(error) => {
                if matches_expected_error(&error, expected_error) {
                    TestOutcome::Passed
                } else {
                    TestOutcome::Failed(format!("unexpected runtime error: {}", error))
                }
            }
        },
        _ => match execute_case_program(runtime, &temp_path, &transformed) {
            Ok(()) => TestOutcome::Passed,
            Err(error) => TestOutcome::Failed(error),
        },
    };

    let outcome = match outcome {
        TestOutcome::Failed(message) => {
            if let Err(error) = fs::write(&temp_path, &transformed) {
                TestOutcome::Failed(format!(
                    "{}\n(additionally failed to materialize transformed case at {}: {})",
                    message,
                    temp_path.display(),
                    error
                ))
            } else {
                TestOutcome::Failed(message)
            }
        }
        other => {
            let _ = fs::remove_file(&temp_path);
            other
        }
    };

    CaseRunResult {
        outcome,
        metadata: loaded.metadata,
    }
}

fn case_artifact_path(case: &TestCase) -> PathBuf {
    std::env::temp_dir().join(format!(
        "raya-es262-{}-{}.js",
        std::process::id(),
        sanitized_case_stem(&case.relative_path),
    ))
}

fn format_failure_report(
    index: usize,
    total: usize,
    case: &TestCase,
    metadata: &Frontmatter,
    message: &str,
) -> String {
    let mut report = format!(
        "FAIL {}/{} {}: {}",
        index,
        total,
        case.relative_path.display(),
        message
    );
    report.push_str(&format!("\n  source: {}", case.absolute_path.display()));

    if let Some(description) = metadata.description.as_deref() {
        report.push_str(&format!("\n  description: {}", description));
    }
    if !metadata.flags.is_empty() {
        report.push_str(&format!("\n  flags: {}", metadata.flags.join(", ")));
    }
    if !metadata.includes.is_empty() {
        report.push_str(&format!("\n  includes: {}", metadata.includes.join(", ")));
    }
    if !metadata.features.is_empty() {
        report.push_str(&format!("\n  features: {}", metadata.features.join(", ")));
    }
    if let Some(negative) = metadata.negative.as_ref() {
        let phase = negative.phase.as_deref().unwrap_or("?");
        let error_type = negative.error_type.as_deref().unwrap_or("?");
        report.push_str(&format!(
            "\n  negative: phase={} type={}",
            phase, error_type
        ));
    }

    let artifact_path = case_artifact_path(case);
    if artifact_path.exists() {
        report.push_str(&format!("\n  transformed: {}", artifact_path.display()));
    }
    report.push_str(&format!(
        "\n  rerun: cargo run -p raya-es262-conformance -- --fail-fast {}",
        case.relative_path.display()
    ));
    report
}

fn matches_expected_error(actual: &str, expected: Option<&str>) -> bool {
    match expected {
        Some("SyntaxError") => {
            actual.contains("SyntaxError")
                || actual.contains("Lexer error")
                || actual.contains("Parse error")
                || actual.contains("Invalid syntax")
                || actual.contains("Binding error")
        }
        Some(expected_name) => actual.contains(expected_name),
        None => true,
    }
}

fn execute_case_program(
    runtime: &Runtime,
    path: &Path,
    source: &str,
) -> std::result::Result<(), String> {
    let debug_case = std::env::var("RAYA_DEBUG_ES262_CASE").is_ok();
    if debug_case {
        eprintln!("[es262-case] compile:start path={}", path.display());
    }
    let program = runtime
        .compile_program_source_at_path(source, path)
        .map_err(|error| format!("compilation failed: {}", error))?;
    if debug_case {
        eprintln!("[es262-case] compile:done path={}", path.display());
        eprintln!("[es262-case] execute:start path={}", path.display());
    }
    let result = runtime
        .execute_program_and_teardown(&program)
        .map_err(|error| format!("runtime failed: {}", error));
    if debug_case {
        eprintln!(
            "[es262-case] execute:done path={} ok={}",
            path.display(),
            result.is_ok()
        );
    }
    result
}

fn prepare_case_source(root: &Path, case: &LoadedCase) -> std::result::Result<String, String> {
    let is_raw = case.metadata.flags.iter().any(|flag| flag == "raw");
    for flag in &case.metadata.flags {
        match flag.as_str() {
            "async" | "module" | "CanBlockIsFalse" => {
                return Err(format!("unsupported test flag: {}", flag));
            }
            "generated" | "onlyStrict" | "noStrict" | "raw" => {}
            _ => {}
        }
    }

    if case.source.contains("$DONE") {
        return Err("uses async completion callback".to_string());
    }
    if import_export_regex().is_match(&case.source) {
        return Err("uses module syntax".to_string());
    }

    let mut include_sources = String::new();
    for include in &case.metadata.includes {
        match include.as_str() {
            "assert.js" | "sta.js" | "compareArray.js" => {}
            "isConstructor.js" => {
                include_sources.push_str(IS_CONSTRUCTOR_SHIM);
                include_sources.push('\n');
            }
            "fnGlobalObject.js" => {
                include_sources.push_str(FN_GLOBAL_OBJECT_SHIM);
                include_sources.push('\n');
            }
            _ => {
                include_sources.push_str(&load_harness_include(root, include)?);
                include_sources.push('\n');
            }
        }
    }

    let combined_host_source = format!("{}\n{}", case.source, include_sources);
    let supported_host_hooks = supported_262_hooks(&combined_host_source);
    if supported_host_hooks.is_none() && combined_host_source.contains("$262") {
        return Err("uses unsupported $262 host hooks".to_string());
    }

    let transformed = transform_source(&case.source)?;
    let strict_prefix = if case.metadata.flags.iter().any(|flag| flag == "onlyStrict") {
        "\"use strict\";\n"
    } else {
        ""
    };

    let mut final_source = String::new();
    if is_raw {
        if !case.metadata.includes.is_empty()
            || supported_host_hooks.is_some()
            || !strict_prefix.is_empty()
        {
            return Err("raw test requires unsupported harness/strict injection".to_string());
        }
        final_source.push_str(&transformed);
        final_source.push('\n');
        final_source.push_str(&required_harness_prelude(&transformed, &include_sources));
    } else {
        final_source.push_str(strict_prefix);
        final_source.push_str(&required_harness_prelude(&transformed, &include_sources));
        if matches!(supported_host_hooks, Some(true)) {
            final_source.push_str(HOST_262_PRELUDE);
            final_source.push('\n');
        }
        final_source.push_str(&include_sources);
        final_source.push_str(&transformed);
    }
    Ok(final_source)
}

fn load_harness_include(root: &Path, include: &str) -> std::result::Result<String, String> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, std::result::Result<String, String>>>> =
        OnceLock::new();

    let include_path = root.join("harness").join(include);
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    if let Some(cached) = cache.lock().unwrap().get(&include_path).cloned() {
        return cached;
    }

    let loaded = fs::read_to_string(&include_path)
        .map_err(|_| format!("failed to load harness include: {}", include))
        .and_then(|raw| transform_source(&raw));
    cache.lock().unwrap().insert(include_path, loaded.clone());
    loaded
}

fn required_harness_prelude(transformed_source: &str, include_sources: &str) -> String {
    let combined_source = format!("{include_sources}\n{transformed_source}");
    let needs_assert = combined_source.contains("__assert(");
    let needs_same_value = combined_source.contains("__assert_sameValue(")
        || combined_source.contains("__assert_notSameValue(");
    let needs_compare_array = combined_source.contains("__compareArray(")
        || combined_source.contains("__assert_compareArray(");
    let needs_assert_throws = combined_source.contains("__assert_throws(");
    let needs_same_value_core = needs_same_value || needs_compare_array;

    let mut prelude = String::new();
    prelude.push_str(HARNESS_CORE_PRELUDE);
    prelude.push('\n');
    if needs_assert {
        prelude.push_str(ASSERT_HELPER_PRELUDE);
        prelude.push('\n');
    }
    if needs_same_value_core {
        prelude.push_str(SAME_VALUE_HELPER_PRELUDE);
        prelude.push('\n');
    }
    if needs_compare_array {
        prelude.push_str(COMPARE_ARRAY_HELPER_PRELUDE);
        prelude.push('\n');
    }
    if needs_assert_throws {
        prelude.push_str(ASSERT_THROWS_HELPER_PRELUDE);
        prelude.push('\n');
    }
    prelude
}

fn supported_262_hooks(source: &str) -> Option<bool> {
    if !source.contains("$262") {
        return Some(false);
    }

    for captures in host_hook_regex().captures_iter(source) {
        let Some(name) = captures.get(1).map(|m| m.as_str()) else {
            return None;
        };
        if !matches!(name, "createRealm" | "evalScript" | "detachArrayBuffer") {
            return None;
        }
    }

    Some(true)
}

fn transform_source(source: &str) -> std::result::Result<String, String> {
    let mut transformed = source.to_string();
    transformed = transformed.replace("assert.sameValue(", "__assert_sameValue(");
    transformed = transformed.replace("assert.notSameValue(", "__assert_notSameValue(");
    transformed = transformed.replace("assert.throws(", "__assert_throws(");
    transformed = transformed.replace("assert.compareArray(", "__assert_compareArray(");
    transformed = bare_assert_regex()
        .replace_all(&transformed, "${prefix}__assert(")
        .into_owned();
    transformed = compare_array_regex()
        .replace_all(&transformed, "${prefix}__compareArray(")
        .into_owned();

    if unsupported_assert_regex().is_match(&transformed) {
        return Err("uses unsupported assert helper".to_string());
    }

    Ok(transformed)
}

fn sanitized_case_stem(path: &Path) -> String {
    let mut out = String::with_capacity(path.as_os_str().len());
    for ch in path.to_string_lossy().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.len() > 96 {
        out.truncate(96);
    }
    out
}

fn bare_assert_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?P<prefix>^|[^.\w$])assert\s*\(").expect("assert regex should compile")
    })
}

fn compare_array_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?P<prefix>^|[^.\w$])compareArray\s*\(")
            .expect("compareArray regex should compile")
    })
}

fn unsupported_assert_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"assert\.[A-Za-z_$][A-Za-z0-9_$]*")
            .expect("unsupported assert regex should compile")
    })
}

fn host_hook_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"\$262\.([A-Za-z_$][A-Za-z0-9_$]*)")
            .expect("$262 host hook regex should compile")
    })
}

fn import_export_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?m)^\s*(import|export)\b").expect("module regex should compile")
    })
}

impl fmt::Display for TestOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TestOutcome::Passed => write!(f, "PASS"),
            TestOutcome::Failed(message) => write!(f, "FAIL: {}", message),
            TestOutcome::Skipped(reason) => write!(f, "SKIP: {}", reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frontmatter_lists_and_negative_block() {
        let meta = parse_frontmatter(
            r#"
description: sample
includes: [assert.js, sta.js]
flags:
  - onlyStrict
negative:
  phase: runtime
  type: TypeError
"#,
        );

        assert_eq!(meta.description.as_deref(), Some("sample"));
        assert_eq!(meta.includes, vec!["assert.js", "sta.js"]);
        assert_eq!(meta.flags, vec!["onlyStrict"]);
        assert_eq!(
            meta.negative
                .as_ref()
                .and_then(|negative| negative.phase.as_deref()),
            Some("runtime")
        );
        assert_eq!(
            meta.negative
                .as_ref()
                .and_then(|negative| negative.error_type.as_deref()),
            Some("TypeError")
        );
    }

    #[test]
    fn failure_report_includes_case_context() {
        let case = TestCase {
            absolute_path: PathBuf::from("/tmp/test262/test/language/example.js"),
            relative_path: PathBuf::from("test/language/example.js"),
        };
        let metadata = Frontmatter {
            description: Some("sample failure".to_string()),
            includes: vec!["assert.js".to_string()],
            flags: vec!["onlyStrict".to_string()],
            features: vec!["tail-call-optimization".to_string()],
            negative: Some(Negative {
                phase: Some("runtime".to_string()),
                error_type: Some("TypeError".to_string()),
            }),
        };

        let report = format_failure_report(7, 42, &case, &metadata, "runtime failed: boom");

        assert!(report.contains("FAIL 7/42 test/language/example.js: runtime failed: boom"));
        assert!(report.contains("source: /tmp/test262/test/language/example.js"));
        assert!(report.contains("description: sample failure"));
        assert!(report.contains("flags: onlyStrict"));
        assert!(report.contains("includes: assert.js"));
        assert!(report.contains("features: tail-call-optimization"));
        assert!(report.contains("negative: phase=runtime type=TypeError"));
        assert!(report.contains(
            "rerun: cargo run -p raya-es262-conformance -- --fail-fast test/language/example.js"
        ));
    }

    #[test]
    fn excludes_paths_by_prefix_and_segment() {
        let exclude_prefixes = vec!["test/built-ins/Array".to_string()];
        let exclude_segments = ["intl402".to_string()].into_iter().collect::<BTreeSet<_>>();

        assert!(is_excluded_relative_path(
            Path::new("test/built-ins/Array/from.js"),
            &exclude_prefixes,
            &exclude_segments,
        ));
        assert!(is_excluded_relative_path(
            Path::new("test/intl402/Collator/default.js"),
            &exclude_prefixes,
            &exclude_segments,
        ));
        assert!(!is_excluded_relative_path(
            Path::new("test/language/expressions/addition.js"),
            &exclude_prefixes,
            &exclude_segments,
        ));
    }
}
