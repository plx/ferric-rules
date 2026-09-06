#!/usr/bin/env bash
# Optional, paired engineering experiment; no performance pass/fail threshold.
set -euo pipefail
base_sha="$1"
head_sha="$2"
output="$3"
mode="${4:-threading}"
[[ "$base_sha" =~ ^[0-9a-f]{40}$ && "$head_sha" =~ ^[0-9a-f]{40}$ ]]
mkdir -p "$output"
output="$(cd "$output" && pwd)"
experiment="$(mktemp -d)"
cleanup() {
    git worktree remove --force "$experiment/base" >/dev/null 2>&1 || true
    git worktree remove --force "$experiment/candidate" >/dev/null 2>&1 || true
    rm -rf "$experiment"
}
trap cleanup EXIT
git worktree add --detach "$experiment/base" "$base_sha"
git worktree add --detach "$experiment/candidate" "$head_sha"
{
    printf 'base=%s\nhead=%s\n' "$base_sha" "$head_sha"
    date -u
    rustc -Vv
    cargo -V
    uname -a
    lscpu
    printf 'features=serde; profile=bench; workspace LTO=true, codegen-units=1\n'
    printf 'Default samples=30; existing Waltz500/serde_medium groups override to10.\n'
} > "$output/environment.txt"
suites=(engine_bench waltz_bench churn_bench join_bench serialization_bench)
filters=('^lifecycle_(load_reset_run|reset_run)_(100|1000)$' '^(waltz_100_junctions|waltz_500/waltz_500_junctions)$' '^churn_(500|2000)_facts$' '^join_(strings|nested_multifields)_(100|1000)$' '^serde_(small|medium)/(serialize|deserialize)$')
package=ferric-rules
expected=16
if [[ "$mode" == threading-capi ]]; then
    package=ferric-rules-ffi
    suites=(capi_bench)
    filters=('^capi/(lifecycle|read_output)/(100|1000)$')
    expected=4
elif [[ "$mode" != threading ]]; then
    echo "Unknown measurement set: $mode" >&2
    exit 1
fi
# Finish both builds before measuring; each revision has its own output directory.
for variant in base candidate; do
    (
        cd "$experiment/$variant"
        build=(cargo bench --locked -p "$package" --features serde --no-run)
        for suite in "${suites[@]}"; do build+=(--bench "$suite"); done
        "${build[@]}" > "$output/$variant-build.log" 2>&1
        git status --porcelain > "$output/$variant-status.txt"
        test ! -s "$output/$variant-status.txt"
    )
done
for round in a b; do
    for variant in base candidate; do
        label="$variant-$round"
        mkdir "$output/$label"
        (
            cd "$experiment/$variant"
            for index in "${!suites[@]}"; do
                suite="${suites[$index]}"
                printf '%s: %s\n' "$label" "$suite"
                cargo bench --locked -p "$package" --features serde --bench "$suite" -- \
                    "${filters[$index]}" --sample-size 30 --measurement-time 3 --warm-up-time 1 \
                    --noplot --save-baseline "$label" > "$output/$label/$suite.log" 2>&1
            done
            cp -R target/criterion "$output/$label/criterion"
        )
    done
done
python3 - "$output" "$expected" <<'PY'
import json
import pathlib
import sys
root = pathlib.Path(sys.argv[1])
expected = int(sys.argv[2])
results = {}
for label in ('base-a', 'candidate-a', 'base-b', 'candidate-b'):
    rows = []
    for path in (root / label / 'criterion').rglob(f'{label}/estimates.json'):
        identity = json.loads((path.parent / 'benchmark.json').read_text())
        estimates = json.loads(path.read_text())
        samples = json.loads((path.parent / 'sample.json').read_text())
        rows.append({'benchmark': identity['full_id'],
                     'median_ns': estimates['median']['point_estimate'],
                     'median_confidence_interval_ns': estimates['median']['confidence_interval'],
                     'samples': len(samples['times'])})
    if len(rows) != expected:
        raise SystemExit(f'{label}: expected {expected} medians, found {len(rows)}')
    results[label] = sorted(rows, key=lambda row: row['benchmark'])
(root / 'medians.json').write_text(json.dumps(results, indent=2) + '\n')
PY
