#!/usr/bin/env bash
# Capture one revision's corpus before checkout changes, then retain both runs.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
if [[ $# -lt 4 || $# -gt 5 ]]; then
    echo "usage: scripts/corpus-evidence.sh capture|verify|finalize output-dir revision run-id [--allow-unverified]" >&2
    exit 2
fi
action="$1"
output_dir="$2"
revision="$3"
run_id="$4"
if [[ "$action" != capture && "$action" != verify && "$action" != finalize ]]; then
    echo "corpus-evidence: action must be capture, verify, or finalize" >&2
    exit 2
fi
if [[ $# -eq 5 && ( "$action" != verify || "$5" != --allow-unverified ) ]]; then
    echo "corpus-evidence: --allow-unverified is only valid for verify" >&2
    exit 2
fi
mkdir -p "$output_dir"
output_dir="$(cd "$output_dir" && pwd)"
summary="$output_dir/summary.json"
starting="$output_dir/starting"
python=(uv run --project "$root/tools/ferric-tools" --locked python)
cd "$root"

clear_outputs() {
    rm -f "$output_dir/ferric.json" "$output_dir/ferric-status.json" "$output_dir/reference.json"
    rm -f "$output_dir/ferric-exit-code" "$output_dir/reference-exit-code"
    rm -f "$output_dir/ferric.log" "$output_dir/reference.log"
}

if [[ "$action" == capture ]]; then
    : > "$starting"
    "${python[@]}" -m ferric_tools.compat.corpus_summary \
        --output "$summary" --revision "$revision" --run-id "$run_id"
    clear_outputs
    rm -f "$starting"
    exit 0
fi

if [[ ! -f "$summary" ]]; then
    echo "corpus-evidence: capture the revision before verifying it" >&2
    exit 2
fi

finalize() {
    local summary_args=(--update "$summary" --revision "$revision" --run-id "$run_id")
    local recorded_code
    # Capture/verify may have been interrupted between resetting the summary
    # and removing old files. No engine command starts while this marker exists.
    if [[ -f "$starting" ]]; then
        "${python[@]}" -m ferric_tools.compat.corpus_summary \
            --update "$summary" --revision "$revision" --run-id "$run_id" --reset-runs
        clear_outputs
        rm -f "$starting"
    fi
    if [[ -f "$output_dir/ferric-status.json" || -f "$output_dir/ferric.json" || -f "$output_dir/ferric-exit-code" ]]; then
        summary_args+=(--ferric-report "$output_dir/ferric.json" --ferric-status "$output_dir/ferric-status.json")
        if [[ -f "$output_dir/ferric-exit-code" ]]; then
            recorded_code="$(cat "$output_dir/ferric-exit-code")"
            if [[ "$recorded_code" =~ ^-?[0-9]+$ ]]; then
                summary_args+=(--ferric-exit-code "$recorded_code")
            fi
        fi
    fi
    if [[ -f "$output_dir/reference.json" || -f "$output_dir/reference-exit-code" ]]; then
        summary_args+=(--reference-report "$output_dir/reference.json")
        if [[ -f "$output_dir/reference-exit-code" ]]; then
            recorded_code="$(cat "$output_dir/reference-exit-code")"
            if [[ "$recorded_code" =~ ^-?[0-9]+$ ]]; then
                summary_args+=(--reference-exit-code "$recorded_code")
            fi
        fi
    fi
    if [[ "$action" == verify && $# -eq 0 ]]; then
        summary_args+=(--require-verified)
    fi
    "${python[@]}" -m ferric_tools.compat.corpus_summary "${summary_args[@]}"
}

if [[ "$action" == finalize ]]; then
    finalize
    exit 0
fi
# These are this invocation's known output files, never reusable success stamps.
: > "$starting"
"${python[@]}" -m ferric_tools.compat.corpus_summary \
    --update "$summary" --revision "$revision" --run-id "$run_id" --reset-runs
clear_outputs
rm -f "$starting"
set +e
FERRIC_CORPUS_REPORT="$output_dir/ferric.json" \
    FERRIC_CORPUS_STATUS="$output_dir/ferric-status.json" \
    FERRIC_CORPUS_RUN_ID="$run_id" FERRIC_CORPUS_REVISION="$revision" \
    FERRIC_CORPUS_FILTER="" FERRIC_CORPUS_LEVEL="" \
    cargo test -p ferric-rules --test compat_corpus --features serde --locked \
        2>&1 | tee "$output_dir/ferric.log"
ferric_pipeline_codes=("${PIPESTATUS[@]}")
printf '%s\n' "${ferric_pipeline_codes[0]}" > "$output_dir/ferric-exit-code"
"${python[@]}" -m ferric_tools.compat.corpus \
    --timeout 120 --workers 4 --revision "$revision" --run-id "$run_id" \
    --report "$output_dir/reference.json" 2>&1 | tee "$output_dir/reference.log"
reference_pipeline_codes=("${PIPESTATUS[@]}")
printf '%s\n' "${reference_pipeline_codes[0]}" > "$output_dir/reference-exit-code"
set -e

if [[ $# -eq 4 ]]; then
    finalize
else
    finalize --allow-unverified
fi

# A report never converts a failed command or failed log write into success.
for code in "${ferric_pipeline_codes[@]}" "${reference_pipeline_codes[@]}"; do
    if [[ "$code" -ne 0 ]]; then
        exit 1
    fi
done
