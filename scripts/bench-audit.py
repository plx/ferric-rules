#!/usr/bin/env python3
"""Paired release Criterion measurements for bounded performance audit suites."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import tempfile
import tomllib

SUITES = {
    "audit-host": {
        "ferric-rules": ["cascade_bench", "churn_bench", "negation_bench", "forall_bench", "query_bench", "engine_bench"],
        "ferric-rules-runtime": ["template_registry_bench"],
    },
    "audit-focus": {
        "ferric-rules": ["engine_bench", "module_bench", "strategy_bench", "manners_bench", "waltz_bench"],
    },
    "audit-storage": {
        "ferric-rules": ["engine_bench", "join_bench", "churn_bench", "cascade_bench", "manners_bench", "alpha_fanout_bench"],
        "ferric-rules-core": ["storage_indices_bench"],
    },
    "audit-cascade": {
        "ferric-rules": ["cascade_bench", "churn_bench", "negation_bench", "forall_bench", "engine_bench", "manners_bench"],
        "ferric-rules-core": ["storage_indices_bench"],
    },
    "audit-core": {"ferric-rules-core": ["storage_indices_bench"]},
    "audit-full": {"ferric-rules": ["*"]},
    "audit-runtime": {"ferric-rules-runtime": ["*"]},
    "audit-ffi": {"ferric-rules-ffi": ["capi_bench"]},
}


def read_medians(criterion: Path, label: str, destination: Path) -> dict[str, float]:
    medians = {}
    for estimates in sorted(criterion.glob(f"**/{label}/estimates.json")):
        case = estimates.parent.parent.relative_to(criterion).as_posix()
        median = json.loads(estimates.read_text())["median"]["point_estimate"]
        if not isinstance(median, (float, int)) or not 0 < median < float("inf"):
            raise ValueError(f"invalid median for {case}: {median}")
        medians[case] = median
        shutil.copytree(estimates.parent, destination / case / label, dirs_exist_ok=True)
    if not medians:
        raise ValueError(f"no Criterion measurements for {label}")
    return medians


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("base")
    parser.add_argument("head")
    parser.add_argument("suite", choices=SUITES)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    for revision in (args.base, args.head):
        if not re.fullmatch(r"[0-9a-fA-F]{40}", revision):
            parser.error("base and head must be full 40-digit commit SHAs")
    repo = Path(__file__).resolve().parent.parent
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)

    def capture(*command: str) -> str:
        return subprocess.check_output(command, cwd=repo, text=True).strip()

    def run(command: list[str], cwd: Path, log: str) -> None:
        print(f"{log}: {' '.join(command)}", flush=True)
        with (output / log).open("w") as stream:
            environment = os.environ.copy()
            if command[0] == "cargo":
                environment["CARGO_TARGET_DIR"] = str(cwd / "target")
            subprocess.run(command, cwd=cwd, env=environment, stdout=stream, stderr=subprocess.STDOUT, check=True)

    for revision in (args.base, args.head):
        capture("git", "cat-file", "-e", f"{revision}^{{commit}}")
    profiles = {}
    for name, revision in (("base", args.base), ("head", args.head)):
        manifest = tomllib.loads(capture("git", "show", f"{revision}:Cargo.toml"))
        profiles[name] = {key: value for key, value in manifest.get("profile", {}).items() if key in {"release", "bench"}}
    if profiles["base"] != profiles["head"]:
        raise ValueError("base and head use different release/bench profiles")
    suite = SUITES[args.suite]
    command = ["cargo", "bench"]
    for package in suite:
        command += ["-p", package]
    command += ["--features", "serde"]
    for bench in dict.fromkeys(bench for benches in suite.values() for bench in benches):
        command += ["--bench", bench]
    sampling = ["--noplot", "--sample-size", "20", "--warm-up-time", "1", "--measurement-time", "1"]
    metadata = {
        "base_revision": args.base, "head_revision": args.head,
        "benchmark_source_revision": args.head, "suite": args.suite,
        "command": command, "criterion_arguments": sampling,
        "rustc": capture("rustc", "-Vv"), "platform": platform.platform(),
        "machine": platform.machine(), "rustflags": os.environ.get("RUSTFLAGS", ""),
        "profile": profiles["head"],
        "cpu": Path("/proc/cpuinfo").read_text() if Path("/proc/cpuinfo").exists() else platform.processor(),
        "note": "Identical benchmark sources; both builds finish before alternating base/head/base/head measurements. Existing per-group sample-size overrides remain in effect.",
    }
    (output / "environment.json").write_text(json.dumps(metadata, indent=2) + "\n")
    with tempfile.TemporaryDirectory(prefix="ferric-audit-") as temporary:
        trees = {name: Path(temporary) / name for name in ("base", "head")}
        try:
            for name, revision in (("base", args.base), ("head", args.head)):
                run(["git", "worktree", "add", "--detach", str(trees[name]), revision], repo, f"checkout-{name}.log")
                # Only benchmark source is overlaid. Production code and manifests
                # remain at the selected revision, including its release profile.
                for package in suite:
                    prefix = f"crates/{package}/benches/"
                    files = capture("git", "ls-tree", "-r", "--name-only", args.head, "--", prefix).splitlines()
                    for filename in files:
                        if not filename.startswith(prefix) or ".." in Path(filename).parts:
                            raise ValueError(f"invalid benchmark path: {filename}")
                        destination = trees[name] / filename
                        destination.parent.mkdir(parents=True, exist_ok=True)
                        destination.write_bytes(subprocess.check_output(["git", "show", f"{args.head}:{filename}"], cwd=repo))
                run(command + ["--no-run"], trees[name], f"build-{name}.log")
            rounds = []
            for number in (1, 2):
                values = {}
                for name in ("base", "head"):
                    label = f"audit-{name}-{number}"
                    run(command + ["--", *sampling, "--save-baseline", label], trees[name], f"{label}.log")
                    values[name] = read_medians(trees[name] / "target/criterion", label, output / "criterion")
                if values["base"].keys() != values["head"].keys():
                    raise ValueError("base and head measured different workloads")
                rounds.append({case: {"before_ns": before, "after_ns": values["head"][case],
                    "delta_percent": 100 * (values["head"][case] / before - 1)}
                    for case, before in values["base"].items()})
            if rounds[0].keys() != rounds[1].keys():
                raise ValueError("repeat measured different workloads")
            (output / "measurements.json").write_text(json.dumps({**metadata, "rounds": rounds}, indent=2) + "\n")
            report = [f"# {args.suite}: {len(rounds[0])} paired workloads", "", "Criterion median changes; negative is faster. Raw estimates and logs accompany this report.", "", "| Workload | First pass | Repeat |", "| --- | ---: | ---: |"]
            for case in sorted(rounds[0]):
                report.append(f"| {case} | {rounds[0][case]['delta_percent']:+.2f}% | {rounds[1][case]['delta_percent']:+.2f}% |")
            (output / "report.md").write_text("\n".join(report) + "\n")
        finally:
            for tree in trees.values():
                if tree.exists():
                    subprocess.run(["git", "worktree", "remove", "--force", str(tree)], cwd=repo, check=False)


if __name__ == "__main__":
    main()
