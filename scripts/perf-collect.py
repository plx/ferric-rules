#!/usr/bin/env python3
"""Collect Criterion benchmark results into a performance manifest.

Runs the three Criterion benchmark suites (engine_bench, waltz_bench,
manners_bench) with reduced CI-appropriate settings, then walks the
target/criterion/ directory to collect estimates into a unified
perf-manifest.json.

Optionally collects CLIPS reference times by running equivalent workloads
through the CLIPS Docker image, providing a frame of reference for
ferric's performance.

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
    --clips-reference         Collect CLIPS reference times via Docker
    --clips-image NAME        Docker image (default: ferric-rules/clips-reference:latest)
    --clips-iterations N      Timed iterations per workload (default: 5)
    --clips-warmup N          Warm-up iterations for CLIPS (default: 1)
    --clips-timeout N         Timeout per CLIPS invocation in seconds (default: 120)
    --workload-dir DIR        Directory for .clp workload files (default: target/bench-workloads)
    --skip-workload-gen       Skip running ferric-bench-gen
"""

import argparse
import json
import os
import subprocess
import sys
import time
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

# Criterion benchmark name -> .clp workload filename (for CLIPS reference)
CLIPS_WORKLOADS = {
    "waltz_5_junctions":   "waltz-5.clp",
    "waltz_20_junctions":  "waltz-20.clp",
    "waltz_50_junctions":  "waltz-50.clp",
    "waltz_100_junctions": "waltz-100.clp",
    "manners_8_guests":    "manners-8.clp",
    "manners_16_guests":   "manners-16.clp",
    "manners_32_guests":   "manners-32.clp",
    "manners_64_guests":   "manners-64.clp",
}


# ---------------------------------------------------------------------------
# Duration formatting
# ---------------------------------------------------------------------------

def _fmt_ns(ns):
    """Format nanoseconds with appropriate unit."""
    if ns is None:
        return "n/a"
    ns = float(ns)
    if ns < 1_000:
        return f"{ns:.0f} ns"
    if ns < 1_000_000:
        return f"{ns / 1_000:.1f} us"
    if ns < 1_000_000_000:
        return f"{ns / 1_000_000:.2f} ms"
    return f"{ns / 1_000_000_000:.3f} s"


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


# ---------------------------------------------------------------------------
# CLIPS reference collection
# ---------------------------------------------------------------------------

def generate_workloads(repo_root, workload_dir):
    """Run ferric-bench-gen to create .clp workload files."""
    print("==> Generating CLIPS workload files...", flush=True)
    cmd = [
        "cargo", "run", "--release", "-p", "ferric-bench-gen", "--",
        "--output-dir", workload_dir,
    ]
    result = subprocess.run(cmd, capture_output=False, cwd=str(repo_root))
    if result.returncode != 0:
        print(f"error: ferric-bench-gen failed with code {result.returncode}",
              file=sys.stderr)
        sys.exit(1)


def time_clips_workload(image, repo_root, container_path, timeout):
    """Run a single CLIPS workload in Docker and return elapsed nanoseconds."""
    stdin_text = f'(batch* "{container_path}")\n(reset)\n(run)\n(exit)\n'

    start = time.perf_counter_ns()
    try:
        result = subprocess.run(
            ["docker", "run", "--rm", "-i",
             "-v", f"{repo_root}:/workspace",
             "-w", "/workspace",
             image],
            input=stdin_text,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        return None
    elapsed_ns = time.perf_counter_ns() - start

    if result.returncode != 0:
        return None
    return elapsed_ns


def collect_clips_reference(image, repo_root, workload_dir, warmup, iterations, timeout):
    """Collect CLIPS reference times for all mapped workloads."""
    print(f"\n==> Collecting CLIPS reference times ({iterations} iterations, "
          f"{warmup} warmup)...", flush=True)

    clips_benchmarks = {}

    for bench_name, clp_file in CLIPS_WORKLOADS.items():
        workload_path = os.path.join(workload_dir, clp_file)
        container_path = f"/workspace/{os.path.relpath(workload_path, repo_root)}"

        if not os.path.exists(workload_path):
            print(f"  warning: workload not found: {workload_path}", file=sys.stderr)
            clips_benchmarks[bench_name] = None
            continue

        print(f"    {bench_name} ({clp_file})...", end="", flush=True)

        # Warm-up runs (untimed)
        for _ in range(warmup):
            time_clips_workload(image, repo_root, container_path, timeout)

        # Timed runs
        times = []
        for _ in range(iterations):
            t = time_clips_workload(image, repo_root, container_path, timeout)
            if t is not None:
                times.append(t)

        if times:
            times.sort()
            median_ns = times[len(times) // 2]
            mean_ns = int(sum(times) / len(times))
            clips_benchmarks[bench_name] = {
                "median_ns": median_ns,
                "mean_ns": mean_ns,
                "iterations": len(times),
            }
            print(f" {_fmt_ns(median_ns)}")
        else:
            clips_benchmarks[bench_name] = None
            print(" FAILED")

    collected = sum(1 for v in clips_benchmarks.values() if v is not None)
    print(f"    CLIPS reference: {collected}/{len(CLIPS_WORKLOADS)} collected")

    return clips_benchmarks


# ---------------------------------------------------------------------------
# Manifest assembly
# ---------------------------------------------------------------------------

def collect_manifest(criterion_dir, commit_sha, settings, clips_reference=None):
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
        "version": 2,
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
        "clips_reference": clips_reference,
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
    # CLIPS reference options
    parser.add_argument(
        "--clips-reference", action="store_true",
        help="Collect CLIPS reference times via Docker",
    )
    parser.add_argument(
        "--clips-image", default="ferric-rules/clips-reference:latest",
        metavar="IMAGE",
        help="CLIPS Docker image (default: ferric-rules/clips-reference:latest)",
    )
    parser.add_argument(
        "--clips-iterations", type=int, default=5,
        metavar="N",
        help="Timed iterations per CLIPS workload (default: 5)",
    )
    parser.add_argument(
        "--clips-warmup", type=int, default=1,
        metavar="N",
        help="Warm-up iterations for CLIPS (default: 1)",
    )
    parser.add_argument(
        "--clips-timeout", type=int, default=120,
        metavar="N",
        help="Timeout per CLIPS invocation in seconds (default: 120)",
    )
    parser.add_argument(
        "--workload-dir", default=None, metavar="DIR",
        help="Directory for .clp workload files (default: target/bench-workloads)",
    )
    parser.add_argument(
        "--skip-workload-gen", action="store_true",
        help="Skip running ferric-bench-gen",
    )

    args = parser.parse_args()

    # Resolve paths
    script_dir = Path(__file__).resolve().parent
    repo_root = script_dir.parent
    criterion_dir = args.criterion_dir or str(repo_root / "target" / "criterion")
    output_path = args.output or str(repo_root / "target" / "perf-manifest.json")
    workload_dir = args.workload_dir or str(repo_root / "target" / "bench-workloads")

    # Run Criterion benchmarks
    if not args.skip_bench:
        run_benchmarks(args.sample_size, args.warm_up_time, args.measurement_time)

    # Collect CLIPS reference times
    clips_reference = None
    if args.clips_reference:
        if not args.skip_workload_gen:
            generate_workloads(str(repo_root), workload_dir)

        clips_benchmarks = collect_clips_reference(
            image=args.clips_image,
            repo_root=str(repo_root),
            workload_dir=workload_dir,
            warmup=args.clips_warmup,
            iterations=args.clips_iterations,
            timeout=args.clips_timeout,
        )
        clips_reference = {
            "methodology": "docker_wall_clock",
            "image": args.clips_image,
            "iterations": args.clips_iterations,
            "benchmarks": clips_benchmarks,
        }

    # Collect Criterion results
    settings = {
        "sample_size": args.sample_size,
        "warm_up_time_s": args.warm_up_time,
        "measurement_time_s": args.measurement_time,
    }
    manifest = collect_manifest(criterion_dir, args.commit_sha, settings,
                                clips_reference=clips_reference)

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
    if clips_reference:
        clips_collected = sum(1 for v in clips_reference["benchmarks"].values()
                              if v is not None)
        print(f"    clips reference: {clips_collected}/{len(CLIPS_WORKLOADS)} collected")

    # Exit 1 only if ALL benchmarks are missing
    if summary["collected"] == 0:
        print("error: no benchmark results collected", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
