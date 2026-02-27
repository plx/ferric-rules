#!/usr/bin/env python3
"""Generate a standalone performance report from a perf manifest.

Reads the manifest produced by perf-collect.py and generates
human-readable reports in text (stdout) and Markdown formats.

Usage:
    python scripts/perf-report.py [--manifest FILE] [--report FILE]
                                  [--repo OWNER/REPO] [--commit-sha SHA]
"""

import argparse
import json
import sys
from pathlib import Path


def load_manifest(path):
    with open(path, "r", encoding="utf-8") as f:
        return json.load(f)


# ---------------------------------------------------------------------------
# Duration formatting
# ---------------------------------------------------------------------------

def fmt_ns(ns):
    """Format nanoseconds with appropriate unit."""
    if ns is None:
        return "n/a"
    ns = float(ns)
    if ns < 1_000:
        return f"{ns:.0f} ns"
    if ns < 1_000_000:
        return f"{ns / 1_000:.1f} µs"
    if ns < 1_000_000_000:
        return f"{ns / 1_000_000:.2f} ms"
    return f"{ns / 1_000_000_000:.3f} s"


# ---------------------------------------------------------------------------
# Text summary (stdout)
# ---------------------------------------------------------------------------

def print_summary(manifest):
    summary = manifest["summary"]
    generated = manifest.get("generated", "unknown")
    commit_sha = manifest.get("commit_sha", "")
    settings = manifest.get("settings", {})

    print("Performance Assessment Report")
    print("=" * 40)
    print(f"Generated: {generated}")
    if commit_sha:
        print(f"Commit: {commit_sha[:10]}")
    print(f"Settings: sample_size={settings.get('sample_size', '?')}, "
          f"warm_up={settings.get('warm_up_time_s', '?')}s, "
          f"measurement={settings.get('measurement_time_s', '?')}s")
    print()

    print(f"Benchmarks: {summary['collected']}/{summary['total_benchmarks']} collected"
          f" ({summary['missing']} missing)")
    print(f"Suites: {', '.join(summary.get('suites', []))}")
    print()

    # Per-suite breakdown
    suite_stats = {}
    for name, info in manifest.get("benchmarks", {}).items():
        suite = info["suite"]
        if suite not in suite_stats:
            suite_stats[suite] = {"total": 0, "collected": 0}
        suite_stats[suite]["total"] += 1
        if info.get("median_ns") is not None:
            suite_stats[suite]["collected"] += 1

    print(f"{'Suite':<20s} {'Collected':>10s} {'Total':>6s}")
    print(f"{'-'*20} {'-'*10} {'-'*6}")
    for suite in sorted(suite_stats):
        s = suite_stats[suite]
        print(f"{suite:<20s} {s['collected']:>10d} {s['total']:>6d}")

    # Results
    print()
    print(f"{'Benchmark':<28s} {'Median':>12s} {'Mean':>12s} {'Std Dev':>12s}")
    print(f"{'-'*28} {'-'*12} {'-'*12} {'-'*12}")
    for name, info in manifest.get("benchmarks", {}).items():
        median = fmt_ns(info.get("median_ns"))
        mean = fmt_ns(info.get("mean_ns"))
        stddev = fmt_ns(info.get("stddev_ns"))
        print(f"{name:<28s} {median:>12s} {mean:>12s} {stddev:>12s}")


# ---------------------------------------------------------------------------
# Markdown report
# ---------------------------------------------------------------------------

def write_report(manifest, report_path, repo=None, commit_sha=None):
    summary = manifest["summary"]
    generated = manifest.get("generated", "unknown")
    settings = manifest.get("settings", {})

    lines = []
    lines.append("## Performance Report")
    lines.append("")
    lines.append(f"Criterion benchmark results across {summary['total_benchmarks']} benchmarks "
                 f"in {len(summary.get('suites', []))} suites.")
    lines.append(f"Settings: sample\\_size={settings.get('sample_size', '?')}, "
                 f"warm\\_up={settings.get('warm_up_time_s', '?')}s, "
                 f"measurement={settings.get('measurement_time_s', '?')}s")
    lines.append("")

    sha = commit_sha or manifest.get("commit_sha", "")
    if repo and sha:
        commit_link = f"[`{sha[:10]}`](https://github.com/{repo}/commit/{sha})"
        lines.append(f"Commit: {commit_link} | Generated: {generated}")
    else:
        lines.append(f"Generated: {generated}")
    lines.append("")

    # Suite summary
    suite_stats = {}
    for name, info in manifest.get("benchmarks", {}).items():
        suite = info["suite"]
        if suite not in suite_stats:
            suite_stats[suite] = {"total": 0, "collected": 0}
        suite_stats[suite]["total"] += 1
        if info.get("median_ns") is not None:
            suite_stats[suite]["collected"] += 1

    lines.append("### Summary")
    lines.append("")
    lines.append("| Suite | Benchmarks | Status |")
    lines.append("|---|---:|---|")
    for suite in sorted(suite_stats):
        s = suite_stats[suite]
        lines.append(f"| {suite} | {s['total']} | {s['collected']}/{s['total']} collected |")
    lines.append(f"| **total** | **{summary['total_benchmarks']}** | "
                 f"**{summary['collected']}/{summary['total_benchmarks']}** |")

    # Per-benchmark results
    lines.append("")
    lines.append("### Results")
    lines.append("")
    lines.append("| Benchmark | Suite | Median | Mean | Std Dev |")
    lines.append("|---|---|---:|---:|---:|")
    for name, info in manifest.get("benchmarks", {}).items():
        suite = info["suite"]
        median = fmt_ns(info.get("median_ns"))
        mean = fmt_ns(info.get("mean_ns"))
        stddev = fmt_ns(info.get("stddev_ns"))
        lines.append(f"| {name} | {suite} | {median} | {mean} | {stddev} |")

    with open(report_path, "w", encoding="utf-8") as f:
        f.write("\n".join(lines))
        f.write("\n")

    print(f"Report written to {report_path}")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(description="Generate performance assessment report")
    parser.add_argument("--manifest", default=None, metavar="FILE",
                        help="Path to perf-manifest.json")
    parser.add_argument("--report", default=None, metavar="FILE",
                        help="Write self-contained Markdown report")
    parser.add_argument("--repo", default=None, metavar="OWNER/REPO",
                        help="GitHub repository for commit links")
    parser.add_argument("--commit-sha", default=None, metavar="SHA",
                        help="Commit SHA for report links")

    args = parser.parse_args()

    # Resolve paths
    script_dir = Path(__file__).resolve().parent
    repo_root = script_dir.parent
    manifest_path = Path(args.manifest) if args.manifest else repo_root / "target" / "perf-manifest.json"

    if not manifest_path.exists():
        print(f"error: manifest not found: {manifest_path}", file=sys.stderr)
        print("Run scripts/perf-collect.py first.", file=sys.stderr)
        sys.exit(1)

    manifest = load_manifest(manifest_path)

    # Always print summary
    print_summary(manifest)

    if args.report:
        print()
        write_report(manifest, args.report, repo=args.repo, commit_sha=args.commit_sha)


if __name__ == "__main__":
    main()
