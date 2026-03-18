"""Collect Criterion benchmark results into a performance manifest."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import time
from pathlib import Path
from typing import Annotated

import typer
from rich.console import Console

from ferric_tools._formatting import fmt_ns
from ferric_tools._manifest import utc_now_iso
from ferric_tools._paths import repo_root

app = typer.Typer(help="Collect Criterion benchmark results into a performance manifest.")
console = Console(stderr=True)

# ---------------------------------------------------------------------------
# Benchmark registry
# ---------------------------------------------------------------------------

BENCHMARKS = [
    # engine_bench (9)
    ("engine_create", "engine_bench", "engine_create/new/estimates.json"),
    ("load_and_run_simple", "engine_bench", "load_and_run_simple/new/estimates.json"),
    ("load_and_run_chain_4", "engine_bench", "load_and_run_chain_4/new/estimates.json"),
    ("reset_run_simple", "engine_bench", "reset_run_simple/new/estimates.json"),
    ("reset_run_20_facts", "engine_bench", "reset_run_20_facts/new/estimates.json"),
    ("reset_run_negation", "engine_bench", "reset_run_negation/new/estimates.json"),
    ("reset_run_join_3", "engine_bench", "reset_run_join_3/new/estimates.json"),
    ("reset_run_retract_3", "engine_bench", "reset_run_retract_3/new/estimates.json"),
    ("compile_template_rule", "engine_bench", "compile_template_rule/new/estimates.json"),
    # waltz_bench (11, excluding _run_only)
    ("waltz_5_junctions", "waltz_bench", "waltz_5_junctions/new/estimates.json"),
    ("waltz_10_junctions", "waltz_bench", "waltz_10_junctions/new/estimates.json"),
    ("waltz_20_junctions", "waltz_bench", "waltz_20_junctions/new/estimates.json"),
    ("waltz_50_junctions", "waltz_bench", "waltz_50_junctions/new/estimates.json"),
    ("waltz_100_junctions", "waltz_bench", "waltz_100_junctions/new/estimates.json"),
    ("waltz_150_junctions", "waltz_bench", "waltz_150_junctions/new/estimates.json"),
    ("waltz_200_junctions", "waltz_bench", "waltz_200/waltz_200_junctions/new/estimates.json"),
    ("waltz_300_junctions", "waltz_bench", "waltz_300/waltz_300_junctions/new/estimates.json"),
    ("waltz_500_junctions", "waltz_bench", "waltz_500/waltz_500_junctions/new/estimates.json"),
    ("waltz_750_junctions", "waltz_bench", "waltz_750/waltz_750_junctions/new/estimates.json"),
    ("waltz_1000_junctions", "waltz_bench", "waltz_1000/waltz_1000_junctions/new/estimates.json"),
    # manners_bench (9, excluding _run_only)
    ("manners_8_guests", "manners_bench", "manners_8_guests/new/estimates.json"),
    ("manners_16_guests", "manners_bench", "manners_16_guests/new/estimates.json"),
    ("manners_32_guests", "manners_bench", "manners_32_guests/new/estimates.json"),
    ("manners_48_guests", "manners_bench", "manners_48_guests/new/estimates.json"),
    ("manners_64_guests", "manners_bench", "manners_64/manners_64_guests/new/estimates.json"),
    ("manners_96_guests", "manners_bench", "manners_96/manners_96_guests/new/estimates.json"),
    ("manners_128_guests", "manners_bench", "manners_128/manners_128_guests/new/estimates.json"),
    ("manners_256_guests", "manners_bench", "manners_256/manners_256_guests/new/estimates.json"),
    ("manners_512_guests", "manners_bench", "manners_512/manners_512_guests/new/estimates.json"),
    # join_bench (10, excluding _run_only)
    ("join_3_wide", "join_bench", "join_3_wide/new/estimates.json"),
    ("join_5_wide", "join_bench", "join_5_wide/new/estimates.json"),
    ("join_7_wide", "join_bench", "join_7_wide/new/estimates.json"),
    ("join_9_wide", "join_bench", "join_9_wide/new/estimates.json"),
    ("join_11_wide", "join_bench", "join_11_wide/new/estimates.json"),
    ("join_13_wide", "join_bench", "join_13_wide/new/estimates.json"),
    ("join_15_wide", "join_bench", "join_15/join_15_wide/new/estimates.json"),
    ("join_17_wide", "join_bench", "join_17/join_17_wide/new/estimates.json"),
    ("join_19_wide", "join_bench", "join_19/join_19_wide/new/estimates.json"),
    ("join_21_wide", "join_bench", "join_21/join_21_wide/new/estimates.json"),
    # churn_bench (10, excluding _run_only)
    ("churn_100_facts", "churn_bench", "churn_100_facts/new/estimates.json"),
    ("churn_250_facts", "churn_bench", "churn_250_facts/new/estimates.json"),
    ("churn_500_facts", "churn_bench", "churn_500_facts/new/estimates.json"),
    ("churn_1000_facts", "churn_bench", "churn_1000_facts/new/estimates.json"),
    ("churn_2000_facts", "churn_bench", "churn_2000_facts/new/estimates.json"),
    ("churn_5000_facts", "churn_bench", "churn_5000_facts/new/estimates.json"),
    ("churn_10000_facts", "churn_bench", "churn_10000/churn_10000_facts/new/estimates.json"),
    ("churn_25000_facts", "churn_bench", "churn_25000/churn_25000_facts/new/estimates.json"),
    ("churn_50000_facts", "churn_bench", "churn_50000/churn_50000_facts/new/estimates.json"),
    ("churn_100000_facts", "churn_bench", "churn_100000/churn_100000_facts/new/estimates.json"),
    # negation_bench (10, excluding _run_only)
    ("negation_50_blockers", "negation_bench", "negation_50_blockers/new/estimates.json"),
    ("negation_100_blockers", "negation_bench", "negation_100_blockers/new/estimates.json"),
    ("negation_200_blockers", "negation_bench", "negation_200_blockers/new/estimates.json"),
    ("negation_500_blockers", "negation_bench", "negation_500_blockers/new/estimates.json"),
    ("negation_1000_blockers", "negation_bench", "negation_1000_blockers/new/estimates.json"),
    ("negation_2500_blockers", "negation_bench", "negation_2500_blockers/new/estimates.json"),
    (
        "negation_5000_blockers",
        "negation_bench",
        "negation_5000/negation_5000_blockers/new/estimates.json",
    ),
    (
        "negation_10000_blockers",
        "negation_bench",
        "negation_10000/negation_10000_blockers/new/estimates.json",
    ),
    (
        "negation_25000_blockers",
        "negation_bench",
        "negation_25000/negation_25000_blockers/new/estimates.json",
    ),
    (
        "negation_50000_blockers",
        "negation_bench",
        "negation_50000/negation_50000_blockers/new/estimates.json",
    ),
    # exists_bench (6, excluding _run_only)
    ("exists_10s_5r", "exists_bench", "exists_10s_5r/new/estimates.json"),
    ("exists_50s_10r", "exists_bench", "exists_50s_10r/new/estimates.json"),
    ("exists_100s_20r", "exists_bench", "exists_100s_20r/new/estimates.json"),
    (
        "exists_200s_50r",
        "exists_bench",
        "exists_200s_50r/exists_200s_50r/new/estimates.json",
    ),
    (
        "exists_500s_100r",
        "exists_bench",
        "exists_500s_100r/exists_500s_100r/new/estimates.json",
    ),
    (
        "exists_1000s_50r",
        "exists_bench",
        "exists_1000s_50r/exists_1000s_50r/new/estimates.json",
    ),
    # forall_bench (7, excluding _run_only)
    ("forall_20_tasks", "forall_bench", "forall_20_tasks/new/estimates.json"),
    ("forall_50_tasks", "forall_bench", "forall_50_tasks/new/estimates.json"),
    ("forall_100_tasks", "forall_bench", "forall_100_tasks/new/estimates.json"),
    (
        "forall_200_tasks",
        "forall_bench",
        "forall_200/forall_200_tasks/new/estimates.json",
    ),
    (
        "forall_500_tasks",
        "forall_bench",
        "forall_500/forall_500_tasks/new/estimates.json",
    ),
    (
        "forall_1000_tasks",
        "forall_bench",
        "forall_1000/forall_1000_tasks/new/estimates.json",
    ),
    (
        "forall_2000_tasks",
        "forall_bench",
        "forall_2000/forall_2000_tasks/new/estimates.json",
    ),
    # strategy_bench (8)
    (
        "strategy_activations200_depth",
        "strategy_bench",
        "strategy_activations200/depth/new/estimates.json",
    ),
    (
        "strategy_activations200_breadth",
        "strategy_bench",
        "strategy_activations200/breadth/new/estimates.json",
    ),
    (
        "strategy_activations200_lex",
        "strategy_bench",
        "strategy_activations200/lex/new/estimates.json",
    ),
    (
        "strategy_activations200_mea",
        "strategy_bench",
        "strategy_activations200/mea/new/estimates.json",
    ),
    (
        "strategy_churn1000_depth",
        "strategy_bench",
        "strategy_churn1000/depth/new/estimates.json",
    ),
    (
        "strategy_churn1000_breadth",
        "strategy_bench",
        "strategy_churn1000/breadth/new/estimates.json",
    ),
    (
        "strategy_churn1000_lex",
        "strategy_bench",
        "strategy_churn1000/lex/new/estimates.json",
    ),
    (
        "strategy_churn1000_mea",
        "strategy_bench",
        "strategy_churn1000/mea/new/estimates.json",
    ),
    # alpha_fanout_bench (5, excluding _run_only)
    (
        "alpha_fanout_10r_100f",
        "alpha_fanout_bench",
        "alpha_fanout_10r_100f/new/estimates.json",
    ),
    (
        "alpha_fanout_50r_500f",
        "alpha_fanout_bench",
        "alpha_fanout_50r_500f/new/estimates.json",
    ),
    (
        "alpha_fanout_100r_1000f",
        "alpha_fanout_bench",
        "alpha_fanout_100r_1000f/alpha_fanout_100r_1000f/new/estimates.json",
    ),
    (
        "alpha_fanout_200r_2000f",
        "alpha_fanout_bench",
        "alpha_fanout_200r_2000f/alpha_fanout_200r_2000f/new/estimates.json",
    ),
    (
        "alpha_fanout_500r_5000f",
        "alpha_fanout_bench",
        "alpha_fanout_500r_5000f/alpha_fanout_500r_5000f/new/estimates.json",
    ),
    # cascade_bench (5, excluding _run_only)
    ("cascade_d3_100k", "cascade_bench", "cascade_d3_100k/new/estimates.json"),
    (
        "cascade_d5_100k",
        "cascade_bench",
        "cascade_d5_100k/cascade_d5_100k/new/estimates.json",
    ),
    (
        "cascade_d7_50k",
        "cascade_bench",
        "cascade_d7_50k/cascade_d7_50k/new/estimates.json",
    ),
    (
        "cascade_d10_30k",
        "cascade_bench",
        "cascade_d10_30k/cascade_d10_30k/new/estimates.json",
    ),
    (
        "cascade_d15_20k",
        "cascade_bench",
        "cascade_d15_20k/cascade_d15_20k/new/estimates.json",
    ),
    # evaluator_bench (13)
    (
        "eval_arith_100",
        "evaluator_bench",
        "eval_arithmetic/eval_arith_100/new/estimates.json",
    ),
    (
        "eval_arith_500",
        "evaluator_bench",
        "eval_arithmetic/eval_arith_500/new/estimates.json",
    ),
    (
        "eval_arith_1000",
        "evaluator_bench",
        "eval_arithmetic/eval_arith_1000/new/estimates.json",
    ),
    (
        "eval_arith_5000",
        "evaluator_bench",
        "eval_arithmetic/eval_arith_5000/new/estimates.json",
    ),
    (
        "eval_defun_100",
        "evaluator_bench",
        "eval_deffunction/eval_defun_100/new/estimates.json",
    ),
    (
        "eval_defun_1000",
        "evaluator_bench",
        "eval_deffunction/eval_defun_1000/new/estimates.json",
    ),
    (
        "eval_defun_10000",
        "evaluator_bench",
        "eval_deffunction/eval_defun_10000/new/estimates.json",
    ),
    (
        "eval_loop_1000",
        "evaluator_bench",
        "eval_loop/eval_loop_1000/new/estimates.json",
    ),
    (
        "eval_loop_10000",
        "evaluator_bench",
        "eval_loop/eval_loop_10000/new/estimates.json",
    ),
    (
        "eval_loop_100000",
        "evaluator_bench",
        "eval_loop/eval_loop_100000/new/estimates.json",
    ),
    (
        "eval_string_100",
        "evaluator_bench",
        "eval_string/eval_string_100/new/estimates.json",
    ),
    (
        "eval_string_500",
        "evaluator_bench",
        "eval_string/eval_string_500/new/estimates.json",
    ),
    (
        "eval_string_1000",
        "evaluator_bench",
        "eval_string/eval_string_1000/new/estimates.json",
    ),
    # query_bench (4, excluding _run_only)
    ("query_100i_10c", "query_bench", "query_100i_10c/new/estimates.json"),
    (
        "query_500i_20c",
        "query_bench",
        "query_500i_20c/query_500i_20c/new/estimates.json",
    ),
    (
        "query_1000i_50c",
        "query_bench",
        "query_1000i_50c/query_1000i_50c/new/estimates.json",
    ),
    (
        "query_5000i_100c",
        "query_bench",
        "query_5000i_100c/query_5000i_100c/new/estimates.json",
    ),
    # compile_bench (5)
    ("compile_10r_5t", "compile_bench", "compile_10r_5t/new/estimates.json"),
    ("compile_50r_10t", "compile_bench", "compile_50r_10t/new/estimates.json"),
    (
        "compile_100r_20t",
        "compile_bench",
        "compile_100r_20t/compile_100r_20t/new/estimates.json",
    ),
    (
        "compile_200r_30t",
        "compile_bench",
        "compile_200r_30t/compile_200r_30t/new/estimates.json",
    ),
    (
        "compile_500r_50t",
        "compile_bench",
        "compile_500r_50t/compile_500r_50t/new/estimates.json",
    ),
    # module_bench (4, excluding _run_only)
    ("module_3m_100i", "module_bench", "module_3m_100i/new/estimates.json"),
    (
        "module_5m_100i",
        "module_bench",
        "module_5m_100i/module_5m_100i/new/estimates.json",
    ),
    (
        "module_10m_50i",
        "module_bench",
        "module_10m_50i/module_10m_50i/new/estimates.json",
    ),
    (
        "module_20m_20i",
        "module_bench",
        "module_20m_20i/module_20m_20i/new/estimates.json",
    ),
    # serialization_bench (9, requires serde feature)
    (
        "serde_small_serialize",
        "serialization_bench",
        "serde_small/serialize/new/estimates.json",
    ),
    (
        "serde_small_deserialize",
        "serialization_bench",
        "serde_small/deserialize/new/estimates.json",
    ),
    (
        "serde_small_compile_baseline",
        "serialization_bench",
        "serde_small/compile_baseline/new/estimates.json",
    ),
    (
        "serde_medium_serialize",
        "serialization_bench",
        "serde_medium/serialize/new/estimates.json",
    ),
    (
        "serde_medium_deserialize",
        "serialization_bench",
        "serde_medium/deserialize/new/estimates.json",
    ),
    (
        "serde_medium_compile_baseline",
        "serialization_bench",
        "serde_medium/compile_baseline/new/estimates.json",
    ),
    (
        "serde_large_serialize",
        "serialization_bench",
        "serde_large/serialize/new/estimates.json",
    ),
    (
        "serde_large_deserialize",
        "serialization_bench",
        "serde_large/deserialize/new/estimates.json",
    ),
    (
        "serde_large_compile_baseline",
        "serialization_bench",
        "serde_large/compile_baseline/new/estimates.json",
    ),
    # constraint_bench (10)
    (
        "constraint_disj_4",
        "constraint_bench",
        "constraint_disjunction/constraint_disj_4/new/estimates.json",
    ),
    (
        "constraint_disj_8",
        "constraint_bench",
        "constraint_disjunction/constraint_disj_8/new/estimates.json",
    ),
    (
        "constraint_disj_16",
        "constraint_bench",
        "constraint_disjunction/constraint_disj_16/new/estimates.json",
    ),
    (
        "constraint_disj_32",
        "constraint_bench",
        "constraint_disjunction/constraint_disj_32/new/estimates.json",
    ),
    (
        "constraint_pred_100",
        "constraint_bench",
        "constraint_predicate/constraint_pred_100/new/estimates.json",
    ),
    (
        "constraint_pred_500",
        "constraint_bench",
        "constraint_predicate/constraint_pred_500/new/estimates.json",
    ),
    (
        "constraint_pred_1000",
        "constraint_bench",
        "constraint_predicate/constraint_pred_1000/new/estimates.json",
    ),
    (
        "constraint_neg_100",
        "constraint_bench",
        "constraint_negation/constraint_neg_100/new/estimates.json",
    ),
    (
        "constraint_neg_500",
        "constraint_bench",
        "constraint_negation/constraint_neg_500/new/estimates.json",
    ),
    (
        "constraint_neg_1000",
        "constraint_bench",
        "constraint_negation/constraint_neg_1000/new/estimates.json",
    ),
]

SUITES = [
    ("engine_bench", None),
    ("waltz_bench", "junctions$"),
    ("manners_bench", "guests$"),
    ("join_bench", "wide$"),
    ("churn_bench", "facts$"),
    ("negation_bench", "blockers$"),
    ("exists_bench", r"[0-9]+r$"),
    ("forall_bench", "tasks$"),
    ("strategy_bench", None),
    ("alpha_fanout_bench", r"[0-9]+f$"),
    ("cascade_bench", r"[0-9]+k$"),
    ("evaluator_bench", None),
    ("query_bench", r"[0-9]+c$"),
    ("compile_bench", None),
    ("module_bench", r"[0-9]+i$"),
    ("serialization_bench", None, ["serde"]),
    ("constraint_bench", None),
]

CLIPS_WORKLOADS = {
    "waltz_5_junctions": "waltz-5.clp",
    "waltz_10_junctions": "waltz-10.clp",
    "waltz_20_junctions": "waltz-20.clp",
    "waltz_50_junctions": "waltz-50.clp",
    "waltz_100_junctions": "waltz-100.clp",
    "waltz_150_junctions": "waltz-150.clp",
    "waltz_200_junctions": "waltz-200.clp",
    "waltz_300_junctions": "waltz-300.clp",
    "waltz_500_junctions": "waltz-500.clp",
    "waltz_750_junctions": "waltz-750.clp",
    "waltz_1000_junctions": "waltz-1000.clp",
    "manners_8_guests": "manners-8.clp",
    "manners_16_guests": "manners-16.clp",
    "manners_32_guests": "manners-32.clp",
    "manners_48_guests": "manners-48.clp",
    "manners_64_guests": "manners-64.clp",
    "manners_96_guests": "manners-96.clp",
    "manners_128_guests": "manners-128.clp",
    "manners_256_guests": "manners-256.clp",
    "manners_512_guests": "manners-512.clp",
    "join_3_wide": "join-3.clp",
    "join_5_wide": "join-5.clp",
    "join_7_wide": "join-7.clp",
    "join_9_wide": "join-9.clp",
    "join_11_wide": "join-11.clp",
    "join_13_wide": "join-13.clp",
    "join_15_wide": "join-15.clp",
    "join_17_wide": "join-17.clp",
    "join_19_wide": "join-19.clp",
    "join_21_wide": "join-21.clp",
    "churn_100_facts": "churn-100.clp",
    "churn_250_facts": "churn-250.clp",
    "churn_500_facts": "churn-500.clp",
    "churn_1000_facts": "churn-1000.clp",
    "churn_2000_facts": "churn-2000.clp",
    "churn_5000_facts": "churn-5000.clp",
    "churn_10000_facts": "churn-10000.clp",
    "churn_25000_facts": "churn-25000.clp",
    "churn_50000_facts": "churn-50000.clp",
    "churn_100000_facts": "churn-100000.clp",
    "negation_50_blockers": "negation-50.clp",
    "negation_100_blockers": "negation-100.clp",
    "negation_200_blockers": "negation-200.clp",
    "negation_500_blockers": "negation-500.clp",
    "negation_1000_blockers": "negation-1000.clp",
    "negation_2500_blockers": "negation-2500.clp",
    "negation_5000_blockers": "negation-5000.clp",
    "negation_10000_blockers": "negation-10000.clp",
    "negation_25000_blockers": "negation-25000.clp",
    "negation_50000_blockers": "negation-50000.clp",
}


def run_benchmarks(sample_size: int, warm_up_time: int, measurement_time: int) -> None:
    """Run Criterion benchmark suites."""
    base_flags = [
        "--noplot",
        "--sample-size",
        str(sample_size),
        "--warm-up-time",
        str(warm_up_time),
        "--measurement-time",
        str(measurement_time),
    ]

    root = repo_root()
    for entry in SUITES:
        suite, filter_regex = entry[0], entry[1]
        features = entry[2] if len(entry) > 2 else None

        bench_source = root / "crates" / "ferric" / "benches" / f"{suite}.rs"
        if not bench_source.exists():
            print(f"==> Skipping {suite} (not present at current checkout)", flush=True)
            continue

        cmd = ["cargo", "bench", "-p", "ferric"]
        if features:
            cmd.extend(["--features", ",".join(features)])
        cmd.extend(["--bench", suite, "--"])
        cmd.extend(base_flags)
        if filter_regex:
            cmd.append(filter_regex)

        print(f"==> Running {suite}...", flush=True)
        result = subprocess.run(cmd, capture_output=False)
        if result.returncode != 0:
            console.print(f"[red]error:[/] {suite} exited with code {result.returncode}")
            raise typer.Exit(1)


def read_estimates(estimates_path: str) -> dict | None:
    """Read a Criterion estimates.json and return extracted metrics."""
    try:
        with open(estimates_path, encoding="utf-8") as f:
            data = json.load(f)
    except (FileNotFoundError, json.JSONDecodeError) as e:
        console.print(f"  [yellow]warning:[/] cannot read {estimates_path}: {e}")
        return None

    median = data.get("median", {})
    mean = data.get("mean", {})
    std_dev = data.get("std_dev", {})
    median_ci = median.get("confidence_interval", {})

    def _floor(v):
        return int(v) if v is not None else None

    return {
        "median_ns": _floor(median.get("point_estimate")),
        "mean_ns": _floor(mean.get("point_estimate")),
        "stddev_ns": _floor(std_dev.get("point_estimate")),
        "median_ci_lower_ns": _floor(median_ci.get("lower_bound")),
        "median_ci_upper_ns": _floor(median_ci.get("upper_bound")),
    }


# ---------------------------------------------------------------------------
# CLIPS reference collection
# ---------------------------------------------------------------------------


def _escape_clips_string(value: str) -> str:
    return value.replace("\\", "\\\\").replace('"', '\\"')


def _build_clips_script(workload_path: str | None = None) -> str:
    lines: list[str] = []
    if workload_path is not None:
        lines.append(f'(load "{_escape_clips_string(workload_path)}")')
        lines.append("(reset)")
        lines.append("(run)")
    lines.append("(exit)")
    return "\n".join(lines) + "\n"


def _resolve_clips_runner(mode: str, clips_bin: str, image: str) -> dict:
    native_path = shutil.which(clips_bin)

    if mode in ("auto", "native") and native_path:
        return {"mode": "native", "clips_bin": native_path}

    if mode == "native":
        console.print(f"[red]error:[/] CLIPS executable not found: {clips_bin}")
        raise typer.Exit(1)

    if shutil.which("docker") is None:
        if mode == "docker":
            console.print("[red]error:[/] docker not found in PATH")
        else:
            console.print(
                f"[red]error:[/] CLIPS executable not found ({clips_bin}) "
                "and docker is not available"
            )
        raise typer.Exit(1)

    inspect = subprocess.run(
        ["docker", "image", "inspect", image],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    if inspect.returncode != 0:
        if mode == "docker":
            console.print(f"[red]error:[/] Docker image not found locally: {image}")
        else:
            console.print(
                f"[red]error:[/] CLIPS executable not found ({clips_bin}), "
                f"and Docker image not found locally: {image}"
            )
        console.print("hint: build the image first with ./scripts/clips-reference.sh build")
        raise typer.Exit(1)

    return {"mode": "docker", "image": image}


def _time_clips_session(runner: dict, root: str, stdin_text: str, timeout: int) -> int | None:
    if runner["mode"] == "native":
        start = time.perf_counter_ns()
        try:
            result = subprocess.run(
                [runner["clips_bin"]],
                input=stdin_text,
                capture_output=True,
                text=True,
                timeout=timeout,
                cwd=root,
            )
        except subprocess.TimeoutExpired:
            return None
        if result.returncode != 0:
            return None
        return time.perf_counter_ns() - start
    else:
        start = time.perf_counter_ns()
        try:
            result = subprocess.run(
                [
                    "docker",
                    "run",
                    "--rm",
                    "-i",
                    "-v",
                    f"{root}:/workspace",
                    "-w",
                    "/workspace",
                    runner["image"],
                ],
                input=stdin_text,
                capture_output=True,
                text=True,
                timeout=timeout,
            )
        except subprocess.TimeoutExpired:
            return None
        if result.returncode != 0:
            return None
        return time.perf_counter_ns() - start


def _clips_workload_path(runner: dict, root: str, workload_path: str) -> str:
    if runner["mode"] == "native":
        return str(Path(workload_path).resolve())
    return f"/workspace/{os.path.relpath(workload_path, root)}"


def _time_clips_sample(runner: dict, root: str, workload_path: str, timeout: int) -> int | None:
    launch_ns = _time_clips_session(runner, root, _build_clips_script(), timeout)
    if launch_ns is None:
        return None
    full_ns = _time_clips_session(runner, root, _build_clips_script(workload_path), timeout)
    if full_ns is None:
        return None
    return max(0, full_ns - launch_ns)


def generate_workloads(root: str, workload_dir: str) -> None:
    print("==> Generating CLIPS workload files...", flush=True)
    cmd = [
        "cargo",
        "run",
        "--release",
        "-p",
        "ferric-bench-gen",
        "--",
        "--output-dir",
        workload_dir,
    ]
    result = subprocess.run(cmd, capture_output=False, cwd=root)
    if result.returncode != 0:
        console.print(f"[red]error:[/] ferric-bench-gen failed with code {result.returncode}")
        raise typer.Exit(1)


def collect_clips_reference(
    runner: dict,
    root: str,
    workload_dir: str,
    warmup: int,
    iterations: int,
    timeout: int,
) -> dict:
    runner_desc = runner.get("clips_bin") or runner.get("image", "?")
    print(
        f"\n==> Collecting CLIPS reference times ({iterations} iterations, "
        f"{warmup} warmup) via {runner['mode']} ({runner_desc})...",
        flush=True,
    )

    clips_benchmarks: dict[str, dict | None] = {}

    for bench_name, clp_file in CLIPS_WORKLOADS.items():
        workload_path = os.path.join(workload_dir, clp_file)
        clips_path = _clips_workload_path(runner, root, workload_path)

        if not os.path.exists(workload_path):
            console.print(f"  [yellow]warning:[/] workload not found: {workload_path}")
            clips_benchmarks[bench_name] = None
            continue

        print(f"    {bench_name} ({clp_file})...", end="", flush=True)

        for _ in range(warmup):
            _time_clips_sample(runner, root, clips_path, timeout)

        times: list[int] = []
        for _ in range(iterations):
            t = _time_clips_sample(runner, root, clips_path, timeout)
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
            print(f" {fmt_ns(median_ns)} (launch-adjusted)")
        else:
            clips_benchmarks[bench_name] = None
            print(" FAILED")

    collected = sum(1 for v in clips_benchmarks.values() if v is not None)
    print(f"    CLIPS reference: {collected}/{len(CLIPS_WORKLOADS)} collected")

    return clips_benchmarks


def collect_manifest(
    criterion_dir: str,
    commit_sha: str | None,
    settings: dict,
    clips_reference: dict | None = None,
) -> dict:
    collected = 0
    missing = 0
    benchmarks: dict[str, dict] = {}

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

    suites_run = sorted({s for _, s, _ in BENCHMARKS})

    return {
        "version": 2,
        "generated": utc_now_iso(),
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


@app.command()
def main(
    output: Annotated[str | None, typer.Option(help="Output manifest path")] = None,
    criterion_dir: Annotated[str | None, typer.Option(help="Criterion output directory")] = None,
    skip_bench: Annotated[bool, typer.Option(help="Skip running benchmarks")] = False,
    sample_size: Annotated[int, typer.Option(help="Criterion sample size")] = 20,
    warm_up_time: Annotated[int, typer.Option(help="Criterion warm-up time in seconds")] = 1,
    measurement_time: Annotated[
        int, typer.Option(help="Criterion measurement time in seconds")
    ] = 1,
    commit_sha: Annotated[str | None, typer.Option(help="Commit SHA to record")] = None,
    clips_reference: Annotated[bool, typer.Option(help="Collect CLIPS reference times")] = False,
    clips_runner: Annotated[str, typer.Option(help="CLIPS runner: auto|native|docker")] = "auto",
    clips_bin: Annotated[str, typer.Option(help="CLIPS executable for native runner")] = "clips",
    clips_image: Annotated[
        str, typer.Option(help="Docker image for docker runner")
    ] = "ferric-rules/clips-reference:latest",
    clips_iterations: Annotated[int, typer.Option(help="Timed iterations per workload")] = 5,
    clips_warmup: Annotated[int, typer.Option(help="Warm-up iterations for CLIPS")] = 1,
    clips_timeout: Annotated[
        int, typer.Option(help="Timeout per CLIPS invocation in seconds")
    ] = 120,
    workload_dir: Annotated[
        str | None, typer.Option(help="Directory for .clp workload files")
    ] = None,
    skip_workload_gen: Annotated[bool, typer.Option(help="Skip running ferric-bench-gen")] = False,
) -> None:
    """Collect Criterion benchmark results into a performance manifest."""
    root = repo_root()
    crit_dir = criterion_dir or str(root / "target" / "criterion")
    output_path = output or str(root / "target" / "perf-manifest.json")
    wl_dir = workload_dir or str(root / "target" / "bench-workloads")

    if not skip_bench:
        run_benchmarks(sample_size, warm_up_time, measurement_time)

    clips_ref = None
    if clips_reference:
        runner = _resolve_clips_runner(clips_runner, clips_bin, clips_image)
        if not skip_workload_gen:
            generate_workloads(str(root), wl_dir)

        clips_benchmarks = collect_clips_reference(
            runner=runner,
            root=str(root),
            workload_dir=wl_dir,
            warmup=clips_warmup,
            iterations=clips_iterations,
            timeout=clips_timeout,
        )
        clips_ref = {
            "methodology": f"{runner['mode']}_wall_clock_launch_adjusted",
            "runner": runner["mode"],
            "iterations": clips_iterations,
            "launch_overhead_adjusted": True,
            "benchmarks": clips_benchmarks,
        }
        if runner["mode"] == "native":
            clips_ref["clips_bin"] = runner["clips_bin"]
        else:
            clips_ref["image"] = runner["image"]

    settings = {
        "sample_size": sample_size,
        "warm_up_time_s": warm_up_time,
        "measurement_time_s": measurement_time,
    }
    manifest = collect_manifest(crit_dir, commit_sha, settings, clips_reference=clips_ref)

    os.makedirs(os.path.dirname(output_path), exist_ok=True)
    with open(output_path, "w", encoding="utf-8") as f:
        json.dump(manifest, f, indent=2)
        f.write("\n")

    summary = manifest["summary"]
    print(f"\n==> Performance manifest written to {output_path}")
    print(
        f"    benchmarks: {summary['collected']}/{summary['total_benchmarks']} collected"
        f" ({summary['missing']} missing)"
    )
    print(f"    suites: {', '.join(summary['suites'])}")
    if clips_ref:
        clips_collected = sum(1 for v in clips_ref["benchmarks"].values() if v is not None)
        print(
            f"    clips reference: {clips_collected}/{len(CLIPS_WORKLOADS)} collected "
            f"({clips_ref['runner']}, launch-adjusted)"
        )

    if summary["collected"] == 0:
        console.print("[red]error:[/] no benchmark results collected")
        raise typer.Exit(1)


if __name__ == "__main__":
    app()
