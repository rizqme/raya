#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import time
from collections import Counter
from concurrent.futures import FIRST_COMPLETED, ThreadPoolExecutor, wait
from dataclasses import dataclass
from pathlib import Path
from typing import Any


PASS_RE = re.compile(r"^PASS\s+(.+)$")
SKIP_RE = re.compile(r"^SKIP\s+(.+?):\s+(.*)$")
FAIL_RE = re.compile(r"^FAIL\s+\d+/\d+\s+(.+?):\s+(.*)$")
SUMMARY_RE = re.compile(
    r"^es262:\s+total=(\d+)\s+passed=(\d+)\s+failed=(\d+)\s+skipped=(\d+)\s+elapsed=(.+)$"
)


@dataclass(frozen=True)
class CaseRange:
    start: int
    end: int

    @property
    def size(self) -> int:
        return self.end - self.start + 1

    def split(self) -> tuple["CaseRange", "CaseRange"]:
        midpoint = (self.start + self.end) // 2
        left = CaseRange(self.start, midpoint)
        right = CaseRange(midpoint + 1, self.end)
        return left, right


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Run Raya's Test262 language slice in timeout-bounded chunks and "
            "materialize a full per-case state report."
        )
    )
    parser.add_argument(
        "--binary",
        default="target/debug/raya-es262-conformance",
        help="Path to the already-built raya-es262-conformance binary.",
    )
    parser.add_argument(
        "--selector",
        default="test/language",
        help="Test262 selector to run. Defaults to the language suite.",
    )
    parser.add_argument(
        "--chunk-size",
        type=int,
        default=200,
        help="Initial chunk size before timeout bisection.",
    )
    parser.add_argument(
        "--timeout-seconds",
        type=int,
        default=120,
        help="Per-chunk timeout in seconds.",
    )
    parser.add_argument(
        "--jobs",
        type=int,
        default=min(4, max(1, os.cpu_count() or 1)),
        help="How many chunk subprocesses to run in parallel.",
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=None,
        help="Optional cap on discovered cases for overview runs.",
    )
    parser.add_argument(
        "--output-json",
        default=None,
        help="Path to write the machine-readable state snapshot.",
    )
    parser.add_argument(
        "--output-md",
        default=None,
        help="Path to write the human-readable markdown summary.",
    )
    parser.add_argument(
        "--resume-json",
        default=None,
        help="Optional existing JSON state file to resume from.",
    )
    parser.add_argument(
        "--cwd",
        default=".",
        help="Workspace root used to execute the binary.",
    )
    parser.add_argument(
        "--progress-interval-seconds",
        type=int,
        default=10,
        help="How often to print a heartbeat while waiting on active chunks.",
    )
    return parser.parse_args()


def discover_cases(binary: Path, selector: str, cwd: Path) -> list[str]:
    proc = subprocess.run(
        [str(binary), "--list", selector],
        cwd=cwd,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        check=True,
    )
    cases = [line.strip() for line in proc.stdout.splitlines() if line.strip()]
    return cases


def parse_chunk_output(output: str) -> tuple[dict[str, dict[str, Any]], dict[str, Any] | None]:
    results: dict[str, dict[str, Any]] = {}
    current_fail_path: str | None = None
    current_fail_lines: list[str] = []
    summary: dict[str, Any] | None = None

    def flush_fail() -> None:
        nonlocal current_fail_path, current_fail_lines
        if current_fail_path is None:
            return
        entry = results.setdefault(current_fail_path, {})
        entry["detail"] = "\n".join(current_fail_lines).rstrip()
        current_fail_path = None
        current_fail_lines = []

    for raw_line in output.splitlines():
        line = raw_line.rstrip("\n")
        pass_match = PASS_RE.match(line)
        if pass_match:
            flush_fail()
            path = pass_match.group(1)
            results[path] = {"status": "passed"}
            continue

        skip_match = SKIP_RE.match(line)
        if skip_match:
            flush_fail()
            path, reason = skip_match.groups()
            results[path] = {"status": "skipped", "reason": reason}
            continue

        fail_match = FAIL_RE.match(line)
        if fail_match:
            flush_fail()
            path, reason = fail_match.groups()
            results[path] = {"status": "failed", "reason": reason}
            current_fail_path = path
            current_fail_lines = [line]
            continue

        if current_fail_path is not None:
            if line.startswith("  "):
                current_fail_lines.append(line)
                continue
            if not line or line.startswith("[runtime]"):
                current_fail_lines.append(line)
                continue
            flush_fail()

        summary_match = SUMMARY_RE.match(line)
        if summary_match:
            total, passed, failed, skipped, elapsed = summary_match.groups()
            summary = {
                "total": int(total),
                "passed": int(passed),
                "failed": int(failed),
                "skipped": int(skipped),
                "elapsed": elapsed,
            }

    flush_fail()
    return results, summary


def load_resume(path: Path | None) -> dict[str, dict[str, Any]]:
    if path is None or not path.exists():
        return {}
    payload = json.loads(path.read_text())
    return dict(payload.get("results", {}))


def ensure_parent(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)


def reason_cluster(reason: str | None) -> str:
    if not reason:
        return "unknown"
    lowered = reason.lower()
    if "expected function to throw" in lowered:
        return "throw-behavior"
    if "unexpected token" in lowered or "parse error" in lowered:
        return "parser-unsupported-syntax"
    if "cannot read properties of undefined" in lowered:
        return "undefined-property-access"
    if "descriptor" in lowered or "own property" in lowered:
        return "descriptor-shape"
    if "compilation failed" in lowered:
        return "compile-failure"
    if "runtime failed" in lowered:
        return "runtime-failure"
    return reason.split(":")[0][:80]


def section_cluster(path: str) -> str:
    parts = Path(path).parts
    if len(parts) >= 3:
        return "/".join(parts[:3])
    return path


def build_markdown_report(
    selector: str,
    total_cases: int,
    results: dict[str, dict[str, Any]],
    timed_out_cases: list[str],
    infra_errors: list[dict[str, Any]],
    started_at: float,
) -> str:
    counts = Counter(entry["status"] for entry in results.values())
    failed_entries = {
        path: entry for path, entry in results.items() if entry.get("status") == "failed"
    }
    skipped_entries = {
        path: entry for path, entry in results.items() if entry.get("status") == "skipped"
    }
    missing = total_cases - len(results)

    section_counter = Counter(section_cluster(path) for path in failed_entries)
    reason_counter = Counter(reason_cluster(entry.get("reason")) for entry in failed_entries.values())

    lines: list[str] = []
    lines.append(f"# Test262 Language State")
    lines.append("")
    lines.append(f"- selector: `{selector}`")
    lines.append(f"- discovered: `{total_cases}`")
    lines.append(f"- recorded: `{len(results)}`")
    lines.append(f"- passed: `{counts.get('passed', 0)}`")
    lines.append(f"- failed: `{counts.get('failed', 0)}`")
    lines.append(f"- skipped: `{counts.get('skipped', 0)}`")
    lines.append(f"- timed out: `{len(timed_out_cases)}`")
    lines.append(f"- infrastructure errors: `{len(infra_errors)}`")
    lines.append(f"- missing: `{missing}`")
    lines.append(f"- elapsed seconds: `{time.time() - started_at:.2f}`")
    lines.append("")
    lines.append("## Early Overview")
    lines.append("")
    if failed_entries:
        lines.append("What looks healthy:")
        lines.append("- The runner is resilient enough to keep marching through failing chunks instead of aborting on the first red case.")
        if counts.get("passed", 0):
            lines.append("- A non-trivial slice of the selected language corpus already passes unchanged, so the failures are clustered rather than uniformly broken.")
        if skipped_entries:
            lines.append("- Unsupported async/module/host-hook cases are being classified as skips instead of hanging the run.")
        lines.append("")
        lines.append("What is failing first:")
        for section, count in section_counter.most_common(10):
            lines.append(f"- `{section}`: `{count}` failures")
        lines.append("")
        lines.append("Dominant failure clusters:")
        for reason, count in reason_counter.most_common(10):
            lines.append(f"- `{reason}`: `{count}` cases")
    else:
        lines.append("- No failed cases were recorded in this run.")
    lines.append("")

    if timed_out_cases:
        lines.append("## Timed Out Cases")
        lines.append("")
        for path in timed_out_cases[:100]:
            lines.append(f"- `{path}`")
        if len(timed_out_cases) > 100:
            lines.append(f"- ... and `{len(timed_out_cases) - 100}` more")
        lines.append("")

    if infra_errors:
        lines.append("## Infrastructure Errors")
        lines.append("")
        for item in infra_errors[:30]:
            lines.append(
                f"- `{item['range_start']}-{item['range_end']}`: `{item['error']}`"
            )
        if len(infra_errors) > 30:
            lines.append(f"- ... and `{len(infra_errors) - 30}` more")
        lines.append("")

    lines.append("## Sample Failed Cases")
    lines.append("")
    for path, entry in list(sorted(failed_entries.items()))[:50]:
        reason = entry.get("reason", "")
        lines.append(f"- `{path}`: {reason}")

    return "\n".join(lines) + "\n"


def write_snapshot(
    json_path: Path,
    md_path: Path,
    selector: str,
    cases: list[str],
    results: dict[str, dict[str, Any]],
    timed_out_cases: list[str],
    infra_errors: list[dict[str, Any]],
    started_at: float,
) -> None:
    ensure_parent(json_path)
    ensure_parent(md_path)
    payload = {
        "selector": selector,
        "discovered": len(cases),
        "recorded": len(results),
        "timed_out_cases": timed_out_cases,
        "infrastructure_errors": infra_errors,
        "results": results,
        "cases": cases,
        "elapsed_seconds": time.time() - started_at,
    }
    json_path.write_text(json.dumps(payload, indent=2, sort_keys=True))
    md_path.write_text(
        build_markdown_report(
            selector=selector,
            total_cases=len(cases),
            results=results,
            timed_out_cases=timed_out_cases,
            infra_errors=infra_errors,
            started_at=started_at,
        )
    )


def run_chunk(
    binary: Path,
    selector: str,
    case_range: CaseRange,
    timeout_seconds: int,
    cwd: Path,
) -> dict[str, Any]:
    cmd = [
        str(binary),
        "--verbose",
        "--show-skips",
        "--from",
        str(case_range.start),
        "--to",
        str(case_range.end),
        selector,
    ]
    started = time.time()
    try:
        proc = subprocess.run(
            cmd,
            cwd=cwd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            timeout=timeout_seconds,
            check=False,
        )
        output = proc.stdout
        parsed, summary = parse_chunk_output(output)
        return {
            "kind": "completed",
            "range": case_range,
            "returncode": proc.returncode,
            "output": output,
            "results": parsed,
            "summary": summary,
            "elapsed": time.time() - started,
        }
    except subprocess.TimeoutExpired as exc:
        output = exc.stdout or ""
        if isinstance(output, bytes):
            output = output.decode("utf-8", errors="replace")
        return {
            "kind": "timeout",
            "range": case_range,
            "output": output,
            "elapsed": time.time() - started,
        }
    except Exception as exc:  # pragma: no cover - defensive
        return {
            "kind": "error",
            "range": case_range,
            "error": repr(exc),
            "elapsed": time.time() - started,
        }


def default_output_paths(cwd: Path) -> tuple[Path, Path]:
    stamp = time.strftime("%Y_%m_%d")
    base = cwd / "test_analysis" / "test262" / stamp
    return (
        base / "language_state.json",
        base / "language_state.md",
    )


def main() -> int:
    args = parse_args()
    cwd = Path(args.cwd).resolve()
    binary = Path(args.binary)
    if not binary.is_absolute():
        binary = (cwd / binary).resolve()
    if not binary.exists():
        print(f"binary does not exist: {binary}", file=sys.stderr)
        return 2

    default_json, default_md = default_output_paths(cwd)
    json_path = Path(args.output_json).resolve() if args.output_json else default_json
    md_path = Path(args.output_md).resolve() if args.output_md else default_md
    resume_path = Path(args.resume_json).resolve() if args.resume_json else None

    cases = discover_cases(binary=binary, selector=args.selector, cwd=cwd)
    if args.limit is not None:
        cases = cases[: args.limit]
    if not cases:
        print("no cases discovered", file=sys.stderr)
        return 1

    results = load_resume(resume_path)
    started_at = time.time()
    timed_out_cases: list[str] = []
    infra_errors: list[dict[str, Any]] = []

    pending: list[CaseRange] = []
    chunk_size = max(1, args.chunk_size)
    for start in range(1, len(cases) + 1, chunk_size):
        end = min(len(cases), start + chunk_size - 1)
        unresolved = any(cases[i - 1] not in results for i in range(start, end + 1))
        if unresolved:
            pending.append(CaseRange(start, end))

    def submit_pending(
        executor: ThreadPoolExecutor,
        active: dict[Any, CaseRange],
    ) -> None:
        while pending and len(active) < max(1, args.jobs):
            case_range = pending.pop(0)
            future = executor.submit(
                run_chunk,
                binary,
                args.selector,
                case_range,
                args.timeout_seconds,
                cwd,
            )
            active[future] = case_range

    try:
        with ThreadPoolExecutor(max_workers=max(1, args.jobs)) as executor:
            active: dict[Any, CaseRange] = {}
            submit_pending(executor, active)
            last_progress_log = time.time()

            while active:
                done, _ = wait(
                    active.keys(),
                    timeout=1.0,
                    return_when=FIRST_COMPLETED,
                )
                if not done:
                    now = time.time()
                    if now - last_progress_log >= max(1, args.progress_interval_seconds):
                        print(
                            json.dumps(
                                {
                                    "event": "heartbeat",
                                    "recorded": len(results),
                                    "discovered": len(cases),
                                    "active_ranges": [
                                        [case_range.start, case_range.end]
                                        for case_range in active.values()
                                    ],
                                    "pending_ranges": len(pending),
                                    "elapsed_seconds": now - started_at,
                                }
                            ),
                            flush=True,
                        )
                        last_progress_log = now
                    continue

                for future in done:
                    case_range = active.pop(future)
                    payload = future.result()
                    if payload["kind"] == "completed":
                        parsed_results = payload["results"]
                        expected_paths = cases[case_range.start - 1 : case_range.end]
                        seen = set(parsed_results)
                        missing_paths = [
                            path
                            for path in expected_paths
                            if path not in seen and path not in results
                        ]
                        if missing_paths and case_range.size > 1:
                            left, right = case_range.split()
                            pending.extend([left, right])
                        else:
                            for path in expected_paths:
                                entry = parsed_results.get(path)
                                if entry is not None:
                                    results[path] = entry
                                elif path not in results:
                                    results[path] = {
                                        "status": "unknown",
                                        "reason": "missing from completed chunk output",
                                    }
                    elif payload["kind"] == "timeout":
                        expected_paths = cases[case_range.start - 1 : case_range.end]
                        if case_range.size == 1:
                            path = expected_paths[0]
                            if path not in results:
                                results[path] = {
                                    "status": "timed_out",
                                    "reason": f"chunk timed out after {args.timeout_seconds}s",
                                }
                            timed_out_cases.append(path)
                        else:
                            left, right = case_range.split()
                            pending.extend([left, right])
                    else:
                        infra_errors.append(
                            {
                                "range_start": case_range.start,
                                "range_end": case_range.end,
                                "error": payload["error"],
                            }
                        )
                        expected_paths = cases[case_range.start - 1 : case_range.end]
                        if case_range.size == 1:
                            path = expected_paths[0]
                            results[path] = {
                                "status": "infrastructure_error",
                                "reason": payload["error"],
                            }
                        else:
                            left, right = case_range.split()
                            pending.extend([left, right])

                    write_snapshot(
                        json_path=json_path,
                        md_path=md_path,
                        selector=args.selector,
                        cases=cases,
                        results=results,
                        timed_out_cases=sorted(set(timed_out_cases)),
                        infra_errors=infra_errors,
                        started_at=started_at,
                    )
                    print(
                        json.dumps(
                            {
                                "event": "chunk_complete",
                                "range": [case_range.start, case_range.end],
                                "kind": payload["kind"],
                                "recorded": len(results),
                                "remaining_ranges": len(pending) + len(active),
                            }
                        ),
                        flush=True,
                    )

                submit_pending(executor, active)
    except KeyboardInterrupt:
        write_snapshot(
            json_path=json_path,
            md_path=md_path,
            selector=args.selector,
            cases=cases,
            results=results,
            timed_out_cases=sorted(set(timed_out_cases)),
            infra_errors=infra_errors,
            started_at=started_at,
        )
        print(
            json.dumps(
                {
                    "event": "interrupted",
                    "recorded": len(results),
                    "discovered": len(cases),
                    "json": str(json_path),
                    "markdown": str(md_path),
                },
                indent=2,
            ),
            file=sys.stderr,
        )
        return 130

    write_snapshot(
        json_path=json_path,
        md_path=md_path,
        selector=args.selector,
        cases=cases,
        results=results,
        timed_out_cases=sorted(set(timed_out_cases)),
        infra_errors=infra_errors,
        started_at=started_at,
    )

    counts = Counter(entry["status"] for entry in results.values())
    print(
        json.dumps(
            {
                "selector": args.selector,
                "discovered": len(cases),
                "recorded": len(results),
                "passed": counts.get("passed", 0),
                "failed": counts.get("failed", 0),
                "skipped": counts.get("skipped", 0),
                "timed_out": counts.get("timed_out", 0),
                "unknown": counts.get("unknown", 0),
                "infrastructure_error": counts.get("infrastructure_error", 0),
                "json": str(json_path),
                "markdown": str(md_path),
            },
            indent=2,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
