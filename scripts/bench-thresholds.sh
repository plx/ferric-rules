#!/usr/bin/env bash

set -euo pipefail

if ! command -v jq >/dev/null 2>&1; then
    echo "error: jq is required for benchmark threshold evaluation" >&2
    exit 1
fi

# Keep CI/runtime cost bounded while still producing Criterion estimates.
CRITERION_FLAGS=(--noplot --sample-size 20 --warm-up-time 1 --measurement-time 1)

echo "==> Running benchmark suites for threshold evaluation"
cargo bench -p ferric --bench engine_bench -- "${CRITERION_FLAGS[@]}"
cargo bench -p ferric --bench waltz_bench -- "${CRITERION_FLAGS[@]}"
cargo bench -p ferric --bench manners_bench -- "${CRITERION_FLAGS[@]}"
cargo bench -p ferric --bench join_bench -- "${CRITERION_FLAGS[@]}"
cargo bench -p ferric --bench churn_bench -- "${CRITERION_FLAGS[@]}"
cargo bench -p ferric --bench negation_bench -- "${CRITERION_FLAGS[@]}"

report_dir="target"
report_json="${report_dir}/bench-threshold-report.json"
report_md="${report_dir}/bench-threshold-report.md"
tmp_results="$(mktemp)"
failures=0

read_estimate_ns() {
    local estimates_file="$1"
    jq -r '(.median.point_estimate // .mean.point_estimate) | floor' "${estimates_file}"
}

record_metric() {
    local name="$1"
    local estimates_file="$2"
    local threshold_ns="$3"

    if [[ ! -f "${estimates_file}" ]]; then
        echo "missing estimates file: ${estimates_file}" >&2
        failures=$((failures + 1))
        printf '{"name":"%s","status":"MISSING","threshold_ns":%s,"estimates_file":"%s"}\n' \
            "${name}" "${threshold_ns}" "${estimates_file}" >>"${tmp_results}"
        return
    fi

    local actual_ns
    actual_ns="$(read_estimate_ns "${estimates_file}")"

    local status="PASS"
    if (( actual_ns > threshold_ns )); then
        status="FAIL"
        failures=$((failures + 1))
    fi

    printf '{"name":"%s","status":"%s","actual_ns":%s,"threshold_ns":%s,"delta_ns":%s,"estimates_file":"%s"}\n' \
        "${name}" \
        "${status}" \
        "${actual_ns}" \
        "${threshold_ns}" \
        "$((actual_ns - threshold_ns))" \
        "${estimates_file}" >>"${tmp_results}"
}

# Section 14 targets (workloads):
record_metric \
    "waltz_100_junctions" \
    "target/criterion/waltz_100_junctions/new/estimates.json" \
    10000000000
record_metric \
    "manners_64_guests" \
    "target/criterion/manners_64/manners_64_guests/new/estimates.json" \
    5000000000

# Selected microbenchmarks (absolute guardrails):
record_metric \
    "engine_create" \
    "target/criterion/engine_create/new/estimates.json" \
    5000000
record_metric \
    "load_and_run_simple" \
    "target/criterion/load_and_run_simple/new/estimates.json" \
    50000000
record_metric \
    "reset_run_retract_3" \
    "target/criterion/reset_run_retract_3/new/estimates.json" \
    50000000
record_metric \
    "compile_template_rule" \
    "target/criterion/compile_template_rule/new/estimates.json" \
    50000000

jq -s '.' "${tmp_results}" >"${report_json}"

{
    echo "# Benchmark Threshold Report"
    echo
    echo "| Benchmark | Status | Actual (ns) | Threshold (ns) | Delta (ns) |"
    echo "|---|---|---:|---:|---:|"
    jq -r '.[] | "| \(.name) | \(.status) | \(.actual_ns // "n/a") | \(.threshold_ns) | \(.delta_ns // "n/a") |"' "${report_json}"
} >"${report_md}"

echo "==> Wrote ${report_json}"
echo "==> Wrote ${report_md}"
cat "${report_md}"

rm -f "${tmp_results}"

if (( failures > 0 )); then
    echo "benchmark threshold gate failed: ${failures} metric(s) outside threshold" >&2
    exit 1
fi
