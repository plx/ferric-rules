#!/usr/bin/env python3
"""Run `ferric check` across extracted test-suite segments with expectations.

This gives a compatibility-focused view that separates:
- passing segments,
- expected failures (intentional invalid fixtures),
- unexpected failures (real blockers),
- expectation mismatches (diagnostic drift / stale expectations).
"""

import argparse
import fnmatch
import json
import subprocess
import sys
from concurrent.futures import ProcessPoolExecutor, as_completed
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path


def load_expectations(path: Path) -> dict:
    if not path.exists():
        return {"entries": {}, "groups": []}
    with path.open("r", encoding="utf-8") as f:
        payload = json.load(f)
    entries = payload.get("entries", {})
    groups = payload.get("groups", [])
    if not isinstance(entries, dict):
        entries = {}
    if not isinstance(groups, list):
        groups = []
    return {"entries": entries, "groups": groups}


def run_check(file_path: Path, ferric_cmd: list[str]) -> tuple[int, str]:
    cmd = ferric_cmd + [str(file_path)]
    proc = subprocess.run(cmd, capture_output=True, text=True)
    stderr = (proc.stderr or "").strip()
    stdout = (proc.stdout or "").strip()
    combined = stderr if stderr else stdout
    return proc.returncode, combined


def summarize_signature(diagnostic: str) -> str:
    if not diagnostic:
        return ""
    first = diagnostic.splitlines()[0]
    # Trim source-position tail to group similar failures.
    marker = " at line "
    idx = first.find(marker)
    if idx >= 0:
        return first[:idx]
    return first


def resolve_expectation(filename: str, expectations: dict) -> dict | None:
    entries = expectations.get("entries", {})
    if filename in entries:
        return entries[filename]

    for group in expectations.get("groups", []):
        pattern = group.get("glob")
        if isinstance(pattern, str) and fnmatch.fnmatch(filename, pattern):
            return group
    return None


def classify_result(filename: str, exit_code: int, diagnostic: str, expectation: dict | None) -> str:
    failed = exit_code != 0
    if expectation is None:
        return "unexpected_fail" if failed else "pass"

    contains = expectation.get("contains", [])
    if not isinstance(contains, list):
        contains = []
    matched = all(token in diagnostic for token in contains)

    if failed and matched:
        return "expected_fail"
    if failed and not matched:
        return "expected_mismatch"
    if not failed:
        return "unexpected_pass"
    return "unexpected_fail"


def worker(args):
    file_path, ferric_cmd = args
    exit_code, diagnostic = run_check(file_path, ferric_cmd)
    return file_path.name, exit_code, diagnostic


def run_jobs(files: list[Path], ferric_cmd: list[str], workers: int):
    args = [(f, ferric_cmd) for f in files]
    max_workers = max(1, workers)

    # Prefer process pool for throughput, but degrade gracefully in restricted
    # environments where semaphore/process limits are blocked.
    try:
        with ProcessPoolExecutor(max_workers=max_workers) as ex:
            futures = [ex.submit(worker, a) for a in args]
            for fut in as_completed(futures):
                yield fut.result()
        return
    except PermissionError:
        pass
    except OSError:
        pass

    with ThreadPoolExecutor(max_workers=max_workers) as ex:
        futures = [ex.submit(worker, a) for a in args]
        for fut in as_completed(futures):
            yield fut.result()


def main() -> int:
    parser = argparse.ArgumentParser(description="Sweep generated segments with expected-failure classification.")
    parser.add_argument(
        "--segments-dir",
        default="tests/generated/test-suite-segments",
        help="Directory containing extracted segment .clp files.",
    )
    parser.add_argument(
        "--expectations",
        default="tests/generated/segment-check-expectations.json",
        help="JSON file containing expected failing segment diagnostics.",
    )
    parser.add_argument(
        "--workers",
        type=int,
        default=4,
        help="Parallel worker count.",
    )
    parser.add_argument(
        "--json-out",
        default=None,
        help="Optional JSON output path for detailed results.",
    )
    parser.add_argument(
        "--ferric-bin",
        default=None,
        help="Path to ferric binary. Defaults to target/debug/ferric if present, else cargo run.",
    )
    args = parser.parse_args()

    segments_dir = Path(args.segments_dir)
    if not segments_dir.is_dir():
        print(f"error: segments directory not found: {segments_dir}", file=sys.stderr)
        return 2

    expectations_path = Path(args.expectations)
    expectations = load_expectations(expectations_path)

    if args.ferric_bin:
        ferric_cmd = [args.ferric_bin, "check"]
    else:
        debug_bin = Path("target/debug/ferric")
        if debug_bin.exists():
            ferric_cmd = [str(debug_bin), "check"]
        else:
            ferric_cmd = ["cargo", "run", "-q", "-p", "ferric-cli", "--", "check"]

    files = sorted(segments_dir.glob("*.clp"))
    if not files:
        print(f"error: no .clp files found in {segments_dir}", file=sys.stderr)
        return 2

    detailed = {}
    counts = {
        "pass": 0,
        "expected_fail": 0,
        "unexpected_fail": 0,
        "expected_mismatch": 0,
        "unexpected_pass": 0,
    }

    sig_counts = {}

    for filename, exit_code, diagnostic in run_jobs(files, ferric_cmd, args.workers):
        expectation = resolve_expectation(filename, expectations)
        status = classify_result(filename, exit_code, diagnostic, expectation)
        counts[status] += 1
        sig = summarize_signature(diagnostic)
        if status in {"unexpected_fail", "expected_mismatch"} and sig:
            sig_counts[sig] = sig_counts.get(sig, 0) + 1
        detailed[filename] = {
            "status": status,
            "exit_code": exit_code,
            "diagnostic": diagnostic,
            "expected": expectation,
        }

    total = len(files)
    print("Segment Check Summary")
    print("=====================")
    print(f"Segments:          {total}")
    print(f"Pass:              {counts['pass']}")
    print(f"Expected fail:     {counts['expected_fail']}")
    print(f"Unexpected fail:   {counts['unexpected_fail']}")
    print(f"Expected mismatch: {counts['expected_mismatch']}")
    print(f"Unexpected pass:   {counts['unexpected_pass']}")

    if sig_counts:
        print("\nTop Unexpected Failure Signatures")
        print("---------------------------------")
        for sig, n in sorted(sig_counts.items(), key=lambda kv: (-kv[1], kv[0]))[:20]:
            print(f"{n:3d}  {sig}")

    if args.json_out:
        out_path = Path(args.json_out)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        payload = {
            "segments_dir": str(segments_dir),
            "expectations": str(expectations_path),
            "ferric_cmd": ferric_cmd,
            "counts": counts,
            "total": total,
            "files": detailed,
        }
        out_path.write_text(json.dumps(payload, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        print(f"\nDetailed JSON: {out_path}")

    has_unexpected = (
        counts["unexpected_fail"] > 0
        or counts["expected_mismatch"] > 0
        or counts["unexpected_pass"] > 0
    )
    return 1 if has_unexpected else 0


if __name__ == "__main__":
    sys.exit(main())
