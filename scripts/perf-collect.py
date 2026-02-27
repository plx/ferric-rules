#!/usr/bin/env python3
"""Collect Criterion benchmark results into a performance manifest.

Runs the three Criterion benchmark suites (engine_bench, waltz_bench,
manners_bench) with reduced CI-appropriate settings, then walks the
target/criterion/ directory to collect estimates into a unified
perf-manifest.json.

Usage:
    python scripts/perf-collect.py [options]

Options:
    --output FILE             Output manifest path (default: target/perf-manifest.json)
    --criterion-dir DIR       Criterion output directory (default: target/criterion)
    --skip-bench              Skip running benchmarks (collect from existing estimates)
    --sample-size N           Criterion sample size (default: 20)
    --warm-up-time N          Criterion warm-up time in seconds (default: 1)
    --measurement-time N      Criterion measurement time in seconds (default: 1)
    --commit-sha SHA          Commit SHA to record in manifest
"""

import argparse
import json
import os
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

# ---------------------------------------------------------------------------
# Benchmark registry
# ---------------------------------------------------------------------------

# (benchmark_name, suite, relative_path_under_criterion_dir)
BENCHMARKS = [
    # engine_bench (9)
    ("engine_create",         "engine_bench", "engine_create/new/estimates.json"),
    ("load_and_run_simple",   "engine_bench", "load_and_run_simple/new/estimates.json"),
    ("load_and_run_chain_4",  "engine_bench", "load_and_run_chain_4/new/estimates.json"),
    ("reset_run_simple",      "engine_bench", "reset_run_simple/new/estimates.json"),
    ("reset_run_20_facts",    "engine_bench", "reset_run_20_facts/new/estimates.json"),
    ("reset_run_negation",    "engine_bench", "reset_run_negation/new/estimates.json"),
    ("reset_run_join_3",      "engine_bench", "reset_run_join_3/new/estimates.json"),
    ("reset_run_retract_3",   "engine_bench", "reset_run_retract_3/new/estimates.json"),
    ("compile_template_rule", "engine_bench", "compile_template_rule/new/estimates.json"),
    # waltz_bench (4, excluding _run_only)
    ("waltz_5_junctions",     "waltz_bench",  "waltz_5_junctions/new/estimates.json"),
    ("waltz_20_junctions",    "waltz_bench",  "waltz_20_junctions/new/estimates.json"),
    ("waltz_50_junctions",    "waltz_bench",  "waltz_50_junctions/new/estimates.json"),
    ("waltz_100_junctions",   "waltz_bench",  "waltz_100_junctions/new/estimates.json"),
    # manners_bench (4, excluding _run_only)
    ("manners_8_guests",      "manners_bench", "manners_8_guests/new/estimates.json"),
    ("manners_16_guests",     "manners_bench", "manners_16_guests/new/estimates.json"),
    ("manners_32_guests",     "manners_bench", "manners_32_guests/new/estimates.json"),
    ("manners_64_guests",     "manners_bench", "manners_64/manners_64_guests/new/estimates.json"),
]

# Suites and their Criterion filter regexes (to exclude _run_only variants)
SUITES = [
    ("engine_bench",  None),              # no filter needed (no _run_only variants)
    ("waltz_bench",   "junctions$"),      # excludes waltz_5_junctions_run_only
    ("manners_bench", "guests$"),         # excludes manners_8_guests_run_only
]


# ---------------------------------------------------------------------------
# Benchmark execution
# ---------------------------------------------------------------------------

def run_benchmarks(sample_size, warm_up_time, measurement_time):
    """Run Criterion benchmark suites with the given settings."""
    base_flags = [
        "--noplot",
        "--sample-size", str(sample_size),
        "--warm-up-time", str(warm_up_time),
        "--measurement-time", str(measurement_time),
    ]

    for suite, filter_regex in SUITES:
        cmd = ["cargo", "bench", "-p", "ferric", "--bench", suite, "--"]
        cmd.extend(base_flags)
        if filter_regex:
            cmd.append(filter_regex)

        print(f"==> Running {suite}...", flush=True)
        result = subprocess.run(cmd, capture_output=False)
        if result.returncode != 0:
            print(f"error: {suite} exited with code {result.returncode}", file=sys.stderr)
            sys.exit(1)


# ---------------------------------------------------------------------------
# Estimates collection
# ---------------------------------------------------------------------------

def read_estimates(estimates_path):
    """Read a Criterion estimates.json and return extracted metrics."""
    try:
        with open(estimates_path, "r", encoding="utf-8") as f:
            data = json.load(f)
    except (FileNotFoundError, json.JSONDecodeError) as e:
        print(f"  warning: cannot read {estimates_path}: {e}", file=sys.stderr)
        return None

    median = data.get("median", {})
    mean = data.get("mean", {})
    std_dev = data.get("std_dev", {})
    median_ci = median.get("confidence_interval", {})

    return {
        "median_ns": _floor_or_none(median.get("point_estimate")),
        "mean_ns": _floor_or_none(mean.get("point_estimate")),
        "stddev_ns": _floor_or_none(std_dev.get("point_estimate")),
        "median_ci_lower_ns": _floor_or_none(median_ci.get("lower_bound")),
        "median_ci_upper_ns": _floor_or_none(median_ci.get("upper_bound")),
    }


def _floor_or_none(value):
    """Floor a numeric value to int, or return None."""
    if value is None:
        return None
    return int(value)


def collect_manifest(criterion_dir, commit_sha, settings):
    """Walk expected benchmark paths and build the manifest dict."""
    collected = 0
    missing = 0
    benchmarks = {}

    for name, suite, rel_path in BENCHMARKS:
        estimates_path = os.path.join(criterion_dir, rel_path)
        metrics = read_estimates(estimates_path)

        if metrics is not None:
            benchmarks[name] = {"suite": suite, **metrics}
            collected += 1
        else:
            benchmarks[name] = {
                "suite": suite,
                "median_ns": None,
                "mean_ns": None,
                "stddev_ns": None,
                "median_ci_lower_ns": None,
                "median_ci_upper_ns": None,
            }
            missing += 1

    suites_run = sorted(set(s for _, s, _ in BENCHMARKS))

    manifest = {
        "version": 1,
        "generated": datetime.now(timezone.utc).isoformat(),
        "commit_sha": commit_sha or "",
        "settings": settings,
        "summary": {
            "total_benchmarks": len(BENCHMARKS),
            "collected": collected,
            "missing": missing,
            "suites": suites_run,
        },
        "benchmarks": benchmarks,
    }

    return manifest


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(
        description="Collect Criterion benchmark results into a performance manifest"
    )
    parser.add_argument(
        "--output", default=None, metavar="FILE",
        help="Output manifest path (default: target/perf-manifest.json)",
    )
    parser.add_argument(
        "--criterion-dir", default=None, metavar="DIR",
        help="Criterion output directory (default: target/criterion)",
    )
    parser.add_argument(
        "--skip-bench", action="store_true",
        help="Skip running benchmarks (collect from existing estimates)",
    )
    parser.add_argument(
        "--sample-size", type=int, default=20,
        help="Criterion sample size (default: 20)",
    )
    parser.add_argument(
        "--warm-up-time", type=int, default=1,
        help="Criterion warm-up time in seconds (default: 1)",
    )
    parser.add_argument(
        "--measurement-time", type=int, default=1,
        help="Criterion measurement time in seconds (default: 1)",
    )
    parser.add_argument(
        "--commit-sha", default=None, metavar="SHA",
        help="Commit SHA to record in manifest",
    )

    args = parser.parse_args()

    # Resolve paths
    script_dir = Path(__file__).resolve().parent
    repo_root = script_dir.parent
    criterion_dir = args.criterion_dir or str(repo_root / "target" / "criterion")
    output_path = args.output or str(repo_root / "target" / "perf-manifest.json")

    # Run benchmarks
    if not args.skip_bench:
        run_benchmarks(args.sample_size, args.warm_up_time, args.measurement_time)

    # Collect results
    settings = {
        "sample_size": args.sample_size,
        "warm_up_time_s": args.warm_up_time,
        "measurement_time_s": args.measurement_time,
    }
    manifest = collect_manifest(criterion_dir, args.commit_sha, settings)

    # Write manifest
    os.makedirs(os.path.dirname(output_path), exist_ok=True)
    with open(output_path, "w", encoding="utf-8") as f:
        json.dump(manifest, f, indent=2)
        f.write("\n")

    # Print summary
    summary = manifest["summary"]
    print(f"\n==> Performance manifest written to {output_path}")
    print(f"    benchmarks: {summary['collected']}/{summary['total_benchmarks']} collected"
          f" ({summary['missing']} missing)")
    print(f"    suites: {', '.join(summary['suites'])}")

    # Exit 1 only if ALL benchmarks are missing
    if summary["collected"] == 0:
        print("error: no benchmark results collected", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
