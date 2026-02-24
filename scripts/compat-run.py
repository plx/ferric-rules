#!/usr/bin/env python3
"""Execution runner for CLIPS compatibility assessment.

Runs files classified as testable/pending through both the ferric CLI and
Docker-based reference CLIPS, compares normalized outputs, and updates
the manifest with classification results.

Prerequisites:
    - Docker image built: scripts/clips-reference.sh build --load
    - Ferric CLI built: cargo build --release -p ferric-cli

Usage:
    python scripts/compat-run.py [options]

Options:
    --all               Run all testable files (default on first run)
    --only-pending      Only run files with classification 'pending'
    --only-divergent    Re-run files previously classified as divergent
    --source NAME       Filter by source directory
    --file PATH         Run a single specific file (relative to tests/examples/)
    --timeout SECS      Per-engine timeout in seconds (default: 30)
    --workers N         Parallel worker count (default: 4)
    --manifest FILE     Path to manifest (default: tests/examples/compat-manifest.json)
    --ferric-bin PATH   Path to ferric binary (default: target/release/ferric)
    --skip-clips        Skip running Docker CLIPS (ferric-only mode)
    --dry-run           Show which files would be run without executing
"""

import argparse
import json
import os
import re
import subprocess
import sys
import time
from concurrent.futures import ProcessPoolExecutor, as_completed
from pathlib import Path


# ---------------------------------------------------------------------------
# Output normalization
# ---------------------------------------------------------------------------

# CLIPS Docker output includes the interactive prompt and banner.
# Lines matching these patterns are stripped from CLIPS output.
CLIPS_NOISE_PATTERNS = [
    re.compile(r"^CLIPS>"),
    re.compile(r"^CLIPS \("),
    re.compile(r"^\s+CLIPS \("),
    re.compile(r"^         CLIPS"),
    re.compile(r"^\[CLIPS\]"),
]


def normalize_output(raw, engine):
    """Normalize engine output for comparison.

    Strips engine-specific noise, trailing whitespace, and normalizes
    line endings.
    """
    text = raw.replace("\r\n", "\n")
    lines = text.split("\n")
    result = []

    for line in lines:
        # Strip CLIPS interactive noise
        if engine == "clips":
            skip = False
            for pat in CLIPS_NOISE_PATTERNS:
                if pat.match(line):
                    skip = True
                    break
            if skip:
                continue

        # Strip trailing whitespace
        result.append(line.rstrip())

    # Strip trailing empty lines
    while result and result[-1] == "":
        result.pop()

    return "\n".join(result) + "\n" if result else ""


def normalize_floats(text):
    """Normalize float representations for looser comparison.

    Converts patterns like 25.0000000 to 25.0 for comparison purposes.
    """
    # Match floats with trailing zeros: 25.0000000 -> 25.0
    return re.sub(r"(\d+\.\d*?)0{3,}\d*", lambda m: m.group(1).rstrip("0") or m.group(1) + "0", text)


# ---------------------------------------------------------------------------
# Engine runners
# ---------------------------------------------------------------------------

def run_ferric(file_path, ferric_bin, timeout_secs):
    """Run a .clp file through the ferric CLI.

    Returns dict with exit_code, stdout, stderr, duration_ms, timed_out.
    """
    start = time.monotonic()
    try:
        proc = subprocess.run(
            [ferric_bin, "run", str(file_path)],
            capture_output=True,
            text=True,
            timeout=timeout_secs,
        )
        duration_ms = int((time.monotonic() - start) * 1000)
        return {
            "exit_code": proc.returncode,
            "stdout": proc.stdout,
            "stderr": proc.stderr,
            "duration_ms": duration_ms,
            "timed_out": False,
        }
    except subprocess.TimeoutExpired:
        duration_ms = int((time.monotonic() - start) * 1000)
        return {
            "exit_code": -1,
            "stdout": "",
            "stderr": f"timeout after {timeout_secs}s",
            "duration_ms": duration_ms,
            "timed_out": True,
        }
    except FileNotFoundError:
        return {
            "exit_code": -1,
            "stdout": "",
            "stderr": f"ferric binary not found: {ferric_bin}",
            "duration_ms": 0,
            "timed_out": False,
        }


def run_clips_docker(file_path, repo_root, harness_script, timeout_secs):
    """Run a .clp file through the Docker CLIPS harness.

    Returns dict with exit_code, stdout, stderr, duration_ms, timed_out.
    """
    start = time.monotonic()
    try:
        proc = subprocess.run(
            [harness_script, "run", "--file", str(file_path)],
            capture_output=True,
            text=True,
            timeout=timeout_secs,
            cwd=str(repo_root),
        )
        duration_ms = int((time.monotonic() - start) * 1000)
        return {
            "exit_code": proc.returncode,
            "stdout": proc.stdout,
            "stderr": proc.stderr,
            "duration_ms": duration_ms,
            "timed_out": False,
        }
    except subprocess.TimeoutExpired:
        duration_ms = int((time.monotonic() - start) * 1000)
        return {
            "exit_code": -1,
            "stdout": "",
            "stderr": f"timeout after {timeout_secs}s",
            "duration_ms": duration_ms,
            "timed_out": True,
        }
    except FileNotFoundError:
        return {
            "exit_code": -1,
            "stdout": "",
            "stderr": f"harness script not found: {harness_script}",
            "duration_ms": 0,
            "timed_out": False,
        }


# ---------------------------------------------------------------------------
# Classification
# ---------------------------------------------------------------------------

def classify_results(ferric_result, clips_result):
    """Classify based on ferric and CLIPS results.

    Returns (classification, reason).
    """
    f = ferric_result
    c = clips_result

    # Both timed out
    if f["timed_out"] and (c is None or c["timed_out"]):
        return "incompatible", "timeout-both"

    # Ferric-only mode (no CLIPS result)
    if c is None:
        if f["timed_out"]:
            return "incompatible", "timeout-ferric"
        if f["exit_code"] != 0:
            return "incompatible", "ferric-error"
        # Ferric succeeded, no CLIPS to compare
        return "pending", "ferric-only-clean"

    # Both have results
    if f["timed_out"] and not c["timed_out"]:
        return "divergent", "timeout-ferric"

    if not f["timed_out"] and c["timed_out"]:
        return "divergent", "timeout-clips"

    if f["exit_code"] != 0 and c["exit_code"] == 0:
        return "divergent", "ferric-error"

    if f["exit_code"] == 0 and c["exit_code"] != 0:
        return "divergent", "clips-error"

    if f["exit_code"] != 0 and c["exit_code"] != 0:
        return "incompatible", "both-error"

    # CLIPS may report errors on stdout even with exit code 0 (e.g.,
    # [EXPRNPSR3] Missing function declaration).  Detect these and treat
    # the file as incompatible when both engines effectively fail.
    clips_has_error = bool(re.search(r"\[(?:EXPRNPSR|PRNTUTIL|CSTRCPSR|PRCCODE)\d*\]", c["stdout"]))
    ferric_has_error = f["exit_code"] != 0
    if clips_has_error and not ferric_has_error:
        return "incompatible", "clips-load-error"
    if clips_has_error and ferric_has_error:
        return "incompatible", "both-error"

    # Both succeeded — compare normalized output
    f_out = normalize_output(f["stdout"], "ferric")
    c_out = normalize_output(c["stdout"], "clips")

    if f_out == c_out:
        if f_out.strip() == "":
            return "equivalent", "empty-match"
        return "equivalent", "exact-match"

    # Try with float normalization
    if normalize_floats(f_out) == normalize_floats(c_out):
        return "equivalent", "float-normalized-match"

    return "divergent", "output-mismatch"


# ---------------------------------------------------------------------------
# Worker function (for parallel execution)
# ---------------------------------------------------------------------------

def process_file(args):
    """Process a single file through both engines.

    args is a tuple: (rel_path, abs_path, ferric_bin, repo_root, harness_script, timeout, skip_clips)
    Returns (rel_path, ferric_result, clips_result, classification, reason).
    """
    rel_path, abs_path, ferric_bin, repo_root, harness_script, timeout, skip_clips = args

    ferric_result = run_ferric(abs_path, ferric_bin, timeout)

    clips_result = None
    if not skip_clips:
        clips_result = run_clips_docker(abs_path, repo_root, harness_script, timeout)

    classification, reason = classify_results(ferric_result, clips_result)

    return rel_path, ferric_result, clips_result, classification, reason


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(description="Run CLIPS compatibility assessment")
    parser.add_argument("--all", action="store_true", help="Run all testable files")
    parser.add_argument("--only-pending", action="store_true", help="Only run pending files")
    parser.add_argument("--only-divergent", action="store_true", help="Re-run divergent files")
    parser.add_argument("--source", help="Filter by source directory")
    parser.add_argument("--file", help="Run a single file (relative to tests/examples/)")
    parser.add_argument("--timeout", type=int, default=30, help="Per-engine timeout in seconds")
    parser.add_argument("--workers", type=int, default=4, help="Parallel worker count")
    parser.add_argument("--manifest", default=None, help="Path to manifest file")
    parser.add_argument("--ferric-bin", default=None, help="Path to ferric binary")
    parser.add_argument("--skip-clips", action="store_true", help="Skip Docker CLIPS (ferric-only)")
    parser.add_argument("--dry-run", action="store_true", help="Show files without running")

    args = parser.parse_args()

    # Resolve paths
    script_dir = Path(__file__).resolve().parent
    repo_root = script_dir.parent
    examples_dir = repo_root / "tests" / "examples"
    manifest_path = Path(args.manifest) if args.manifest else examples_dir / "compat-manifest.json"
    ferric_bin = args.ferric_bin or str(repo_root / "target" / "release" / "ferric")
    harness_script = str(repo_root / "scripts" / "clips-reference.sh")

    # Load manifest
    if not manifest_path.exists():
        print(f"error: manifest not found: {manifest_path}", file=sys.stderr)
        print("Run scripts/compat-scan.py first.", file=sys.stderr)
        sys.exit(1)

    with open(manifest_path, "r") as f:
        manifest = json.load(f)

    # Validate ferric binary exists
    if not args.dry_run and not Path(ferric_bin).exists():
        print(f"error: ferric binary not found: {ferric_bin}", file=sys.stderr)
        print("Run: cargo build --release -p ferric-cli", file=sys.stderr)
        sys.exit(1)

    # Check Docker CLIPS availability
    if not args.skip_clips and not args.dry_run:
        try:
            result = subprocess.run(
                ["docker", "image", "inspect", "ferric-rules/clips-reference:latest"],
                capture_output=True, timeout=10,
            )
            if result.returncode != 0:
                print("warning: Docker CLIPS image not found. Using --skip-clips mode.", file=sys.stderr)
                print("Build with: scripts/clips-reference.sh build --load", file=sys.stderr)
                args.skip_clips = True
        except (FileNotFoundError, subprocess.TimeoutExpired):
            print("warning: Docker not available. Using --skip-clips mode.", file=sys.stderr)
            args.skip_clips = True

    # Select files to run
    files_to_run = []
    for rel_path, info in manifest["files"].items():
        # Filter by mode
        if args.file:
            if rel_path != args.file:
                continue
        elif args.only_pending:
            if info["classification"] != "pending":
                continue
        elif args.only_divergent:
            if info["classification"] != "divergent":
                continue
        elif args.all:
            if info["reason"] not in ("testable", "ferric-only-clean") and info["classification"] != "pending":
                # Skip pre-classified incompatible files unless they were testable
                if info["classification"] == "incompatible" and info["reason"] != "testable":
                    continue
        else:
            # Default: run pending files
            if info["classification"] != "pending":
                continue

        # Filter by source
        if args.source and info["source"] != args.source:
            continue

        abs_path = examples_dir / rel_path
        if not abs_path.exists():
            continue

        files_to_run.append((rel_path, str(abs_path)))

    if not files_to_run:
        print("No files to run.")
        sys.exit(0)

    print(f"Files to run: {len(files_to_run)}")
    print(f"Timeout: {args.timeout}s per engine")
    print(f"Workers: {args.workers}")
    print(f"Skip CLIPS: {args.skip_clips}")
    print()

    if args.dry_run:
        for rel_path, _ in files_to_run:
            print(f"  {rel_path}")
        sys.exit(0)

    # Prepare worker arguments
    work_items = [
        (rel, abs_p, ferric_bin, str(repo_root), harness_script, args.timeout, args.skip_clips)
        for rel, abs_p in files_to_run
    ]

    # Execute
    completed = 0
    results = {}
    start_time = time.monotonic()

    with ProcessPoolExecutor(max_workers=args.workers) as executor:
        future_to_rel = {
            executor.submit(process_file, item): item[0]
            for item in work_items
        }

        for future in as_completed(future_to_rel):
            rel_path = future_to_rel[future]
            try:
                rel, ferric_result, clips_result, classification, reason = future.result()
                results[rel] = (ferric_result, clips_result, classification, reason)
                completed += 1

                # Progress indicator
                status_char = {
                    "equivalent": ".",
                    "divergent": "D",
                    "incompatible": "X",
                    "pending": "?",
                }.get(classification, "?")
                sys.stdout.write(status_char)
                if completed % 80 == 0:
                    sys.stdout.write(f" [{completed}/{len(files_to_run)}]\n")
                sys.stdout.flush()

            except Exception as e:
                results[rel_path] = (
                    {"exit_code": -1, "stdout": "", "stderr": str(e), "duration_ms": 0, "timed_out": False},
                    None,
                    "incompatible",
                    "runner-error",
                )
                completed += 1
                sys.stdout.write("E")
                sys.stdout.flush()

    elapsed = time.monotonic() - start_time
    print(f"\n\nCompleted {completed} files in {elapsed:.1f}s")

    # Update manifest
    for rel_path, (ferric_result, clips_result, classification, reason) in results.items():
        if rel_path in manifest["files"]:
            entry = manifest["files"][rel_path]
            entry["ferric"] = ferric_result
            entry["clips"] = clips_result
            entry["classification"] = classification
            entry["reason"] = reason

    # Recompute summary
    summary = {"total": 0, "equivalent": 0, "divergent": 0, "incompatible": 0, "pending": 0}
    for info in manifest["files"].values():
        summary["total"] += 1
        cls = info["classification"]
        if cls in summary:
            summary[cls] += 1
    manifest["summary"] = summary

    # Write updated manifest
    with open(manifest_path, "w", encoding="utf-8") as f:
        json.dump(manifest, f, indent=2, ensure_ascii=False)
        f.write("\n")

    print(f"\nManifest updated: {manifest_path}")
    print(f"\nResults:")

    # Quick summary of this run
    run_summary = {"equivalent": 0, "divergent": 0, "incompatible": 0, "pending": 0}
    for _, (_, _, cls, reason) in results.items():
        if cls in run_summary:
            run_summary[cls] += 1

    for cls, count in sorted(run_summary.items()):
        if count > 0:
            print(f"  {cls:15s}: {count}")

    # Show divergent details
    divergent = [(k, v) for k, v in results.items() if v[2] == "divergent"]
    if divergent:
        print(f"\nDivergent files ({len(divergent)}):")
        for rel_path, (ferric_r, clips_r, cls, reason) in divergent[:20]:
            print(f"  {rel_path} ({reason})")
            if reason == "output-mismatch" and clips_r:
                f_out = normalize_output(ferric_r["stdout"], "ferric")[:100]
                c_out = normalize_output(clips_r["stdout"], "clips")[:100]
                print(f"    ferric: {f_out!r}")
                print(f"    clips:  {c_out!r}")
        if len(divergent) > 20:
            print(f"  ... and {len(divergent) - 20} more")


if __name__ == "__main__":
    main()
