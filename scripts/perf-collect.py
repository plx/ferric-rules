#!/usr/bin/env python3
"""Collect Criterion benchmark results into a performance manifest.

Runs the Criterion benchmark suites (engine_bench, waltz_bench,
manners_bench, join_bench, churn_bench, negation_bench) with reduced
CI-appropriate settings, then walks the
target/criterion/ directory to collect estimates into a unified
perf-manifest.json.

Optionally collects CLIPS reference times by running equivalent workloads
through the native CLIPS binary when available (or the CLIPS Docker image
as a fallback), providing a frame of reference for ferric's performance.

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
    --clips-reference         Collect CLIPS reference times
    --clips-runner MODE       CLIPS runner: auto|native|docker (default: auto)
    --clips-bin PATH          CLIPS executable for native runner (default: clips)
    --clips-image NAME        Docker image for docker runner
                              (default: ferric-rules/clips-reference:latest)
    --clips-iterations N      Timed iterations per workload (default: 5)
    --clips-warmup N          Warm-up iterations for CLIPS (default: 1)
    --clips-timeout N         Timeout per CLIPS invocation in seconds (default: 120)
    --workload-dir DIR        Directory for .clp workload files (default: target/bench-workloads)
    --skip-workload-gen       Skip running ferric-bench-gen
"""

import argparse
import json
import os
import shutil
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
    # waltz_bench (11, excluding _run_only)
    ("waltz_5_junctions",     "waltz_bench",  "waltz_5_junctions/new/estimates.json"),
    ("waltz_10_junctions",    "waltz_bench",  "waltz_10_junctions/new/estimates.json"),
    ("waltz_20_junctions",    "waltz_bench",  "waltz_20_junctions/new/estimates.json"),
    ("waltz_50_junctions",    "waltz_bench",  "waltz_50_junctions/new/estimates.json"),
    ("waltz_100_junctions",   "waltz_bench",  "waltz_100_junctions/new/estimates.json"),
    ("waltz_150_junctions",   "waltz_bench",  "waltz_150_junctions/new/estimates.json"),
    ("waltz_200_junctions",   "waltz_bench",  "waltz_200/waltz_200_junctions/new/estimates.json"),
    ("waltz_300_junctions",   "waltz_bench",  "waltz_300/waltz_300_junctions/new/estimates.json"),
    ("waltz_500_junctions",   "waltz_bench",  "waltz_500/waltz_500_junctions/new/estimates.json"),
    ("waltz_750_junctions",   "waltz_bench",  "waltz_750/waltz_750_junctions/new/estimates.json"),
    ("waltz_1000_junctions",  "waltz_bench",  "waltz_1000/waltz_1000_junctions/new/estimates.json"),
    # manners_bench (9, excluding _run_only)
    ("manners_8_guests",      "manners_bench", "manners_8_guests/new/estimates.json"),
    ("manners_16_guests",     "manners_bench", "manners_16_guests/new/estimates.json"),
    ("manners_32_guests",     "manners_bench", "manners_32_guests/new/estimates.json"),
    ("manners_48_guests",     "manners_bench", "manners_48_guests/new/estimates.json"),
    ("manners_64_guests",     "manners_bench", "manners_64/manners_64_guests/new/estimates.json"),
    ("manners_96_guests",     "manners_bench", "manners_96/manners_96_guests/new/estimates.json"),
    ("manners_128_guests",    "manners_bench", "manners_128/manners_128_guests/new/estimates.json"),
    ("manners_256_guests",    "manners_bench", "manners_256/manners_256_guests/new/estimates.json"),
    ("manners_512_guests",    "manners_bench", "manners_512/manners_512_guests/new/estimates.json"),
    # join_bench (10, excluding _run_only)
    ("join_3_wide",           "join_bench",    "join_3_wide/new/estimates.json"),
    ("join_5_wide",           "join_bench",    "join_5_wide/new/estimates.json"),
    ("join_7_wide",           "join_bench",    "join_7_wide/new/estimates.json"),
    ("join_9_wide",           "join_bench",    "join_9_wide/new/estimates.json"),
    ("join_11_wide",          "join_bench",    "join_11_wide/new/estimates.json"),
    ("join_13_wide",          "join_bench",    "join_13_wide/new/estimates.json"),
    ("join_15_wide",          "join_bench",    "join_15/join_15_wide/new/estimates.json"),
    ("join_17_wide",          "join_bench",    "join_17/join_17_wide/new/estimates.json"),
    ("join_19_wide",          "join_bench",    "join_19/join_19_wide/new/estimates.json"),
    ("join_21_wide",          "join_bench",    "join_21/join_21_wide/new/estimates.json"),
    # churn_bench (10, excluding _run_only)
    ("churn_100_facts",       "churn_bench",   "churn_100_facts/new/estimates.json"),
    ("churn_250_facts",       "churn_bench",   "churn_250_facts/new/estimates.json"),
    ("churn_500_facts",       "churn_bench",   "churn_500_facts/new/estimates.json"),
    ("churn_1000_facts",      "churn_bench",   "churn_1000_facts/new/estimates.json"),
    ("churn_2000_facts",      "churn_bench",   "churn_2000_facts/new/estimates.json"),
    ("churn_5000_facts",      "churn_bench",   "churn_5000_facts/new/estimates.json"),
    ("churn_10000_facts",     "churn_bench",   "churn_10000/churn_10000_facts/new/estimates.json"),
    ("churn_25000_facts",     "churn_bench",   "churn_25000/churn_25000_facts/new/estimates.json"),
    ("churn_50000_facts",     "churn_bench",   "churn_50000/churn_50000_facts/new/estimates.json"),
    ("churn_100000_facts",    "churn_bench",   "churn_100000/churn_100000_facts/new/estimates.json"),
    # negation_bench (10, excluding _run_only)
    ("negation_50_blockers",  "negation_bench", "negation_50_blockers/new/estimates.json"),
    ("negation_100_blockers", "negation_bench", "negation_100_blockers/new/estimates.json"),
    ("negation_200_blockers", "negation_bench", "negation_200_blockers/new/estimates.json"),
    ("negation_500_blockers", "negation_bench", "negation_500_blockers/new/estimates.json"),
    ("negation_1000_blockers","negation_bench", "negation_1000_blockers/new/estimates.json"),
    ("negation_2500_blockers","negation_bench", "negation_2500_blockers/new/estimates.json"),
    ("negation_5000_blockers","negation_bench", "negation_5000/negation_5000_blockers/new/estimates.json"),
    ("negation_10000_blockers","negation_bench","negation_10000/negation_10000_blockers/new/estimates.json"),
    ("negation_25000_blockers","negation_bench","negation_25000/negation_25000_blockers/new/estimates.json"),
    ("negation_50000_blockers","negation_bench","negation_50000/negation_50000_blockers/new/estimates.json"),
]

# Suites and their Criterion filter regexes (to exclude _run_only variants)
SUITES = [
    ("engine_bench",  None),              # no filter needed (no _run_only variants)
    ("waltz_bench",   "junctions$"),      # excludes waltz_5_junctions_run_only
    ("manners_bench", "guests$"),         # excludes manners_8_guests_run_only
    ("join_bench",    "wide$"),           # excludes join_3_wide_run_only
    ("churn_bench",   "facts$"),          # excludes churn_100_facts_run_only
    ("negation_bench", "blockers$"),      # excludes negation_50_blockers_run_only
]

# Criterion benchmark name -> .clp workload filename (for CLIPS reference)
CLIPS_WORKLOADS = {
    "waltz_5_junctions":     "waltz-5.clp",
    "waltz_10_junctions":    "waltz-10.clp",
    "waltz_20_junctions":    "waltz-20.clp",
    "waltz_50_junctions":    "waltz-50.clp",
    "waltz_100_junctions":   "waltz-100.clp",
    "waltz_150_junctions":   "waltz-150.clp",
    "waltz_200_junctions":   "waltz-200.clp",
    "waltz_300_junctions":   "waltz-300.clp",
    "waltz_500_junctions":   "waltz-500.clp",
    "waltz_750_junctions":   "waltz-750.clp",
    "waltz_1000_junctions":  "waltz-1000.clp",
    "manners_8_guests":      "manners-8.clp",
    "manners_16_guests":     "manners-16.clp",
    "manners_32_guests":     "manners-32.clp",
    "manners_48_guests":     "manners-48.clp",
    "manners_64_guests":     "manners-64.clp",
    "manners_96_guests":     "manners-96.clp",
    "manners_128_guests":    "manners-128.clp",
    "manners_256_guests":    "manners-256.clp",
    "manners_512_guests":    "manners-512.clp",
    "join_3_wide":           "join-3.clp",
    "join_5_wide":           "join-5.clp",
    "join_7_wide":           "join-7.clp",
    "join_9_wide":           "join-9.clp",
    "join_11_wide":          "join-11.clp",
    "join_13_wide":          "join-13.clp",
    "join_15_wide":          "join-15.clp",
    "join_17_wide":          "join-17.clp",
    "join_19_wide":          "join-19.clp",
    "join_21_wide":          "join-21.clp",
    "churn_100_facts":       "churn-100.clp",
    "churn_250_facts":       "churn-250.clp",
    "churn_500_facts":       "churn-500.clp",
    "churn_1000_facts":      "churn-1000.clp",
    "churn_2000_facts":      "churn-2000.clp",
    "churn_5000_facts":      "churn-5000.clp",
    "churn_10000_facts":     "churn-10000.clp",
    "churn_25000_facts":     "churn-25000.clp",
    "churn_50000_facts":     "churn-50000.clp",
    "churn_100000_facts":    "churn-100000.clp",
    "negation_50_blockers":  "negation-50.clp",
    "negation_100_blockers": "negation-100.clp",
    "negation_200_blockers": "negation-200.clp",
    "negation_500_blockers": "negation-500.clp",
    "negation_1000_blockers":"negation-1000.clp",
    "negation_2500_blockers":"negation-2500.clp",
    "negation_5000_blockers":"negation-5000.clp",
    "negation_10000_blockers":"negation-10000.clp",
    "negation_25000_blockers":"negation-25000.clp",
    "negation_50000_blockers":"negation-50000.clp",
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
        # Check if the bench target exists at the current checkout
        bench_source = Path(__file__).resolve().parent.parent / "crates" / "ferric" / "benches" / f"{suite}.rs"
        if not bench_source.exists():
            print(f"==> Skipping {suite} (not present at current checkout)", flush=True)
            continue

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

def _escape_clips_string(value):
    """Escape a Python string for a CLIPS string literal."""
    return value.replace("\\", "\\\\").replace('"', '\\"')


def _build_clips_script(workload_path=None):
    """Build stdin text for a CLIPS invocation."""
    lines = []
    if workload_path is not None:
        lines.append(f'(load "{_escape_clips_string(workload_path)}")')
        lines.append("(reset)")
        lines.append("(run)")
    lines.append("(exit)")
    return "\n".join(lines) + "\n"


def _resolve_clips_runner(mode, clips_bin, image):
    """Resolve the requested CLIPS runner."""
    native_path = shutil.which(clips_bin)

    if mode in ("auto", "native") and native_path:
        return {"mode": "native", "clips_bin": native_path}

    if mode == "native":
        print(f"error: CLIPS executable not found: {clips_bin}", file=sys.stderr)
        sys.exit(1)

    if shutil.which("docker") is None:
        if mode == "docker":
            print("error: docker not found in PATH", file=sys.stderr)
        else:
            print(
                f"error: CLIPS executable not found ({clips_bin}) and docker is not available",
                file=sys.stderr,
            )
        sys.exit(1)

    inspect = subprocess.run(
        ["docker", "image", "inspect", image],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    if inspect.returncode != 0:
        if mode == "docker":
            print(f"error: Docker image not found locally: {image}", file=sys.stderr)
        else:
            print(
                f"error: CLIPS executable not found ({clips_bin}), and Docker image "
                f"not found locally: {image}",
                file=sys.stderr,
            )
        print(
            "hint: build the image first with ./scripts/clips-reference.sh build",
            file=sys.stderr,
        )
        sys.exit(1)

    return {"mode": "docker", "image": image}


def _time_clips_session_native(clips_bin, repo_root, stdin_text, timeout):
    """Run a single CLIPS invocation natively and return elapsed nanoseconds."""
    start = time.perf_counter_ns()
    try:
        result = subprocess.run(
            [clips_bin],
            input=stdin_text,
            capture_output=True,
            text=True,
            timeout=timeout,
            cwd=repo_root,
        )
    except subprocess.TimeoutExpired:
        return None
    elapsed_ns = time.perf_counter_ns() - start

    if result.returncode != 0:
        return None
    return elapsed_ns


def _time_clips_session_docker(image, repo_root, stdin_text, timeout):
    """Run a single CLIPS invocation via Docker and return elapsed nanoseconds."""
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


def _time_clips_session(runner, repo_root, stdin_text, timeout):
    """Run a single CLIPS invocation using the resolved runner."""
    if runner["mode"] == "native":
        return _time_clips_session_native(
            runner["clips_bin"], repo_root, stdin_text, timeout
        )
    return _time_clips_session_docker(
        runner["image"], repo_root, stdin_text, timeout
    )


def _clips_workload_path(runner, repo_root, workload_path):
    """Return the path string CLIPS should load for the selected runner."""
    if runner["mode"] == "native":
        return str(Path(workload_path).resolve())
    return f"/workspace/{os.path.relpath(workload_path, repo_root)}"


def _time_clips_sample(runner, repo_root, workload_path, timeout):
    """Measure a workload and subtract a matched launch-only baseline."""
    launch_ns = _time_clips_session(
        runner, repo_root, _build_clips_script(), timeout
    )
    if launch_ns is None:
        return None

    full_ns = _time_clips_session(
        runner, repo_root, _build_clips_script(workload_path), timeout
    )
    if full_ns is None:
        return None

    return max(0, full_ns - launch_ns)


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


def collect_clips_reference(runner, repo_root, workload_dir, warmup, iterations, timeout):
    """Collect CLIPS reference times for all mapped workloads."""
    runner_desc = runner["clips_bin"] if runner["mode"] == "native" else runner["image"]
    print(
        f"\n==> Collecting CLIPS reference times ({iterations} iterations, "
        f"{warmup} warmup) via {runner['mode']} ({runner_desc})...",
        flush=True,
    )

    clips_benchmarks = {}

    for bench_name, clp_file in CLIPS_WORKLOADS.items():
        workload_path = os.path.join(workload_dir, clp_file)
        clips_path = _clips_workload_path(runner, repo_root, workload_path)

        if not os.path.exists(workload_path):
            print(f"  warning: workload not found: {workload_path}", file=sys.stderr)
            clips_benchmarks[bench_name] = None
            continue

        print(f"    {bench_name} ({clp_file})...", end="", flush=True)

        # Warm-up runs (untimed)
        for _ in range(warmup):
            _time_clips_sample(runner, repo_root, clips_path, timeout)

        # Timed runs
        times = []
        for _ in range(iterations):
            t = _time_clips_sample(runner, repo_root, clips_path, timeout)
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
            print(f" {_fmt_ns(median_ns)} (launch-adjusted)")
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
        help="Collect CLIPS reference times (native CLIPS preferred, Docker fallback)",
    )
    parser.add_argument(
        "--clips-runner", choices=("auto", "native", "docker"), default="auto",
        metavar="MODE",
        help="CLIPS runner: auto|native|docker (default: auto)",
    )
    parser.add_argument(
        "--clips-bin", default="clips", metavar="PATH",
        help="CLIPS executable for native runner (default: clips)",
    )
    parser.add_argument(
        "--clips-image", default="ferric-rules/clips-reference:latest",
        metavar="IMAGE",
        help="CLIPS Docker image for docker runner "
             "(default: ferric-rules/clips-reference:latest)",
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
        clips_runner = _resolve_clips_runner(
            args.clips_runner, args.clips_bin, args.clips_image
        )

        if not args.skip_workload_gen:
            generate_workloads(str(repo_root), workload_dir)

        clips_benchmarks = collect_clips_reference(
            runner=clips_runner,
            repo_root=str(repo_root),
            workload_dir=workload_dir,
            warmup=args.clips_warmup,
            iterations=args.clips_iterations,
            timeout=args.clips_timeout,
        )
        clips_reference = {
            "methodology": f"{clips_runner['mode']}_wall_clock_launch_adjusted",
            "runner": clips_runner["mode"],
            "iterations": args.clips_iterations,
            "launch_overhead_adjusted": True,
            "benchmarks": clips_benchmarks,
        }
        if clips_runner["mode"] == "native":
            clips_reference["clips_bin"] = clips_runner["clips_bin"]
        else:
            clips_reference["image"] = clips_runner["image"]

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
        print(f"    clips reference: {clips_collected}/{len(CLIPS_WORKLOADS)} collected "
              f"({clips_reference['runner']}, launch-adjusted)")

    # Exit 1 only if ALL benchmarks are missing
    if summary["collected"] == 0:
        print("error: no benchmark results collected", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
