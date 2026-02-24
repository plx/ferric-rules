#!/usr/bin/env bash
#
# Comparative benchmarks: ferric vs. CLIPS
#
# Measures three phases per engine per workload using the hyperfine
# subtraction method, plus peak memory via /usr/bin/time.
#
# Phase A: (exit)                                → launch only
# Phase B: (load "w.clp") (exit)                 → launch + load
# Phase C: (load "w.clp") (reset) (run) (exit)   → launch + load + eval
#
# Derived:  load ≈ B − A,  eval ≈ C − B

set -euo pipefail

# ── Defaults ─────────────────────────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

WORKLOAD_DIR="${REPO_ROOT}/target/bench-workloads"
RESULTS_DIR="${REPO_ROOT}/target/bench-compare"
FERRIC_BIN="${REPO_ROOT}/target/release/ferric"
CLIPS_BIN="clips"

HYPERFINE_WARMUP=3
HYPERFINE_RUNS=10

ALL_WORKLOADS="waltz-5 waltz-20 waltz-50 waltz-100 manners-8 manners-16 manners-32 manners-64"
QUICK_WORKLOADS="waltz-5 waltz-20 manners-8 manners-16"
WORKLOADS=""

SKIP_BUILD=0
SKIP_GENERATE=0
QUICK=0

# ── Usage ────────────────────────────────────────────────────────────────────

usage() {
    cat <<'EOF'
Usage: scripts/bench-compare.sh [OPTIONS]

Comparative benchmarks: ferric vs. CLIPS.

Options:
  --quick             CI mode: fewer runs, small workloads only
  --runs <N>          hyperfine min-runs (default: 10)
  --warmup <N>        hyperfine warmup iterations (default: 3)
  --workloads <list>  Comma-separated workload names (default: all)
  --skip-build        Skip cargo build (reuse existing binaries)
  --skip-generate     Skip workload generation (reuse existing .clp files)
  --output-dir <dir>  Results directory (default: target/bench-compare)
  -h, --help          Show this help

Prerequisites:
  hyperfine           https://github.com/sharkdp/hyperfine
  clips               apt-get install clips  (Ubuntu)
  jq                  apt-get install jq
EOF
}

# ── Helpers ──────────────────────────────────────────────────────────────────

require_cmd() {
    local cmd="$1"
    local hint="${2:-}"
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "error: required command not found: $cmd" >&2
        [[ -n "$hint" ]] && echo "  hint: $hint" >&2
        exit 1
    fi
}

# Build the stdin command string for a given phase and workload .clp path.
phase_input() {
    local phase="$1"
    local clp="$2"

    case "$phase" in
        A) printf '(exit)\n' ;;
        B) printf '(load "%s")\n(exit)\n' "$clp" ;;
        C) printf '(load "%s")\n(reset)\n(run)\n(exit)\n' "$clp" ;;
    esac
}

# Build the shell command string that hyperfine will benchmark.
bench_cmd() {
    local engine="$1"
    local phase="$2"
    local clp="$3"

    local input
    input="$(phase_input "$phase" "$clp")"

    if [[ "$engine" == "clips" ]]; then
        # printf pipes the CLIPS commands to the clips binary
        echo "printf '%s' '${input}' | ${CLIPS_BIN}"
    else
        echo "printf '%s' '${input}' | ${FERRIC_BIN} repl"
    fi
}

# Extract the median (seconds) from a hyperfine JSON result file.
read_median() {
    local json_file="$1"
    jq -r '.results[0].median' "$json_file"
}

# Extract the stddev (seconds) from a hyperfine JSON result file.
read_stddev() {
    local json_file="$1"
    jq -r '.results[0].stddev' "$json_file"
}

# Measure peak RSS for a single command invocation.
# Writes peak RSS in kilobytes to stdout.
measure_peak_rss_kb() {
    local cmd="$1"
    local tmp
    tmp="$(mktemp)"

    if [[ "$(uname)" == "Linux" ]]; then
        /usr/bin/time -v sh -c "$cmd" >/dev/null 2>"$tmp" || true
        grep "Maximum resident" "$tmp" | awk '{print $NF}'
    else
        # macOS: /usr/bin/time -l reports bytes (not KB)
        /usr/bin/time -l sh -c "$cmd" >/dev/null 2>"$tmp" || true
        local bytes
        bytes=$(grep "maximum resident" "$tmp" | awk '{print $1}')
        if [[ -n "$bytes" ]]; then
            echo $(( bytes / 1024 ))
        else
            echo "0"
        fi
    fi

    rm -f "$tmp"
}

# ── Argument Parsing ─────────────────────────────────────────────────────────

while [[ $# -gt 0 ]]; do
    case "$1" in
        --quick)
            QUICK=1
            HYPERFINE_WARMUP=1
            HYPERFINE_RUNS=3
            shift
            ;;
        --runs)
            HYPERFINE_RUNS="$2"
            shift 2
            ;;
        --warmup)
            HYPERFINE_WARMUP="$2"
            shift 2
            ;;
        --workloads)
            WORKLOADS="${2//,/ }"
            shift 2
            ;;
        --skip-build)
            SKIP_BUILD=1
            shift
            ;;
        --skip-generate)
            SKIP_GENERATE=1
            shift
            ;;
        --output-dir)
            RESULTS_DIR="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "error: unknown option: $1" >&2
            usage >&2
            exit 1
            ;;
    esac
done

if [[ -z "$WORKLOADS" ]]; then
    if [[ "$QUICK" -eq 1 ]]; then
        WORKLOADS="$QUICK_WORKLOADS"
    else
        WORKLOADS="$ALL_WORKLOADS"
    fi
fi

# ── Prerequisites ────────────────────────────────────────────────────────────

require_cmd hyperfine "https://github.com/sharkdp/hyperfine#installation"
require_cmd jq "apt-get install jq"
require_cmd "$CLIPS_BIN" "apt-get install clips  (Ubuntu/Debian)"

echo "==> Comparative benchmark: ferric vs. CLIPS"
echo "    workloads: $WORKLOADS"
echo "    runs: $HYPERFINE_RUNS, warmup: $HYPERFINE_WARMUP"

# ── Step 1: Build ────────────────────────────────────────────────────────────

if [[ "$SKIP_BUILD" -eq 0 ]]; then
    echo "==> Building ferric (release)..."
    cargo build --release -p ferric-cli -p ferric-bench-gen 2>&1 | tail -1
fi

if [[ ! -x "$FERRIC_BIN" ]]; then
    echo "error: ferric binary not found at $FERRIC_BIN" >&2
    echo "  hint: run without --skip-build, or cargo build --release -p ferric-cli" >&2
    exit 1
fi

# ── Step 2: Generate workloads ───────────────────────────────────────────────

if [[ "$SKIP_GENERATE" -eq 0 ]]; then
    echo "==> Generating workloads..."
    "${REPO_ROOT}/target/release/ferric-bench-gen" --output-dir "$WORKLOAD_DIR"
fi

# ── Step 3: Run hyperfine benchmarks ────────────────────────────────────────

mkdir -p "$RESULTS_DIR"

ENGINES="clips ferric"
PHASES="A B C"

for workload in $WORKLOADS; do
    clp="${WORKLOAD_DIR}/${workload}.clp"

    if [[ ! -f "$clp" ]]; then
        echo "error: workload file not found: $clp" >&2
        exit 1
    fi

    for engine in $ENGINES; do
        for phase in $PHASES; do
            out_json="${RESULTS_DIR}/${engine}-${workload}-phase${phase}.json"
            cmd="$(bench_cmd "$engine" "$phase" "$clp")"

            echo "--- $engine / $workload / phase $phase"
            hyperfine \
                --warmup "$HYPERFINE_WARMUP" \
                --min-runs "$HYPERFINE_RUNS" \
                --export-json "$out_json" \
                --shell=sh \
                "$cmd" 2>&1
        done
    done
done

# ── Step 4: Memory measurement ──────────────────────────────────────────────

echo "==> Measuring peak memory (Phase C only)..."

for workload in $WORKLOADS; do
    clp="${WORKLOAD_DIR}/${workload}.clp"

    for engine in $ENGINES; do
        cmd="$(bench_cmd "$engine" "C" "$clp")"
        rss_kb="$(measure_peak_rss_kb "$cmd")"
        echo "${engine}-${workload}-rss_kb=${rss_kb}" >> "${RESULTS_DIR}/memory.txt"
        echo "  $engine / $workload: ${rss_kb} KB"
    done
done

# ── Step 5: Generate report ─────────────────────────────────────────────────

echo "==> Generating report..."

report_json="${RESULTS_DIR}/report.json"
report_md="${RESULTS_DIR}/report.md"
tmp_entries="$(mktemp)"

for workload in $WORKLOADS; do
    for engine in $ENGINES; do
        phase_a_json="${RESULTS_DIR}/${engine}-${workload}-phaseA.json"
        phase_b_json="${RESULTS_DIR}/${engine}-${workload}-phaseB.json"
        phase_c_json="${RESULTS_DIR}/${engine}-${workload}-phaseC.json"

        median_a="$(read_median "$phase_a_json")"
        median_b="$(read_median "$phase_b_json")"
        median_c="$(read_median "$phase_c_json")"

        stddev_a="$(read_stddev "$phase_a_json")"
        stddev_b="$(read_stddev "$phase_b_json")"
        stddev_c="$(read_stddev "$phase_c_json")"

        # Derived phase times (seconds)
        launch_s="$median_a"
        load_s="$(echo "$median_b - $median_a" | bc -l)"
        eval_s="$(echo "$median_c - $median_b" | bc -l)"
        total_s="$median_c"

        # Convert to milliseconds for readability
        launch_ms="$(echo "$launch_s * 1000" | bc -l)"
        load_ms="$(echo "$load_s * 1000" | bc -l)"
        eval_ms="$(echo "$eval_s * 1000" | bc -l)"
        total_ms="$(echo "$total_s * 1000" | bc -l)"

        # Peak RSS
        rss_kb="$(grep "^${engine}-${workload}-rss_kb=" "${RESULTS_DIR}/memory.txt" | cut -d= -f2)"

        jq -n \
            --arg workload "$workload" \
            --arg engine "$engine" \
            --argjson launch_ms "$(printf '%.3f' "$launch_ms")" \
            --argjson load_ms "$(printf '%.3f' "$load_ms")" \
            --argjson eval_ms "$(printf '%.3f' "$eval_ms")" \
            --argjson total_ms "$(printf '%.3f' "$total_ms")" \
            --argjson peak_rss_kb "${rss_kb:-0}" \
            --argjson raw_a_median "$median_a" \
            --argjson raw_a_stddev "$stddev_a" \
            --argjson raw_b_median "$median_b" \
            --argjson raw_b_stddev "$stddev_b" \
            --argjson raw_c_median "$median_c" \
            --argjson raw_c_stddev "$stddev_c" \
            '{
                workload: $workload,
                engine: $engine,
                launch_ms: $launch_ms,
                load_ms: $load_ms,
                eval_ms: $eval_ms,
                total_ms: $total_ms,
                peak_rss_kb: $peak_rss_kb,
                raw: {
                    phase_a: { median_s: $raw_a_median, stddev_s: $raw_a_stddev },
                    phase_b: { median_s: $raw_b_median, stddev_s: $raw_b_stddev },
                    phase_c: { median_s: $raw_c_median, stddev_s: $raw_c_stddev }
                }
            }' >> "$tmp_entries"
    done
done

# Build the full JSON report
jq -s '{
    metadata: {
        timestamp: (now | todate),
        workloads: [.[].workload] | unique,
        engines: [.[].engine] | unique
    },
    entries: .
}' "$tmp_entries" > "$report_json"

# Build the Markdown report
{
    echo "# Comparative Benchmark Report"
    echo
    echo "| Workload | Phase | CLIPS (ms) | ferric (ms) | Ratio |"
    echo "|----------|-------|----------:|----------:|------:|"

    for workload in $WORKLOADS; do
        for phase_name in Launch Load Eval Total; do
            field=""
            case "$phase_name" in
                Launch) field="launch_ms" ;;
                Load)   field="load_ms" ;;
                Eval)   field="eval_ms" ;;
                Total)  field="total_ms" ;;
            esac

            clips_val="$(jq -r "select(.workload == \"$workload\" and .engine == \"clips\") | .${field}" "$tmp_entries")"
            ferric_val="$(jq -r "select(.workload == \"$workload\" and .engine == \"ferric\") | .${field}" "$tmp_entries")"

            if [[ -n "$clips_val" ]] && [[ "$clips_val" != "null" ]] && \
               [[ -n "$ferric_val" ]] && [[ "$ferric_val" != "null" ]]; then
                ratio="$(echo "scale=2; $ferric_val / $clips_val" | bc -l 2>/dev/null || echo "n/a")"
                printf "| %-14s | %-6s | %10.3f | %10.3f | %5sx |\n" \
                    "$workload" "$phase_name" "$clips_val" "$ferric_val" "$ratio"
            fi
        done
    done

    echo
    echo "## Peak Memory (Phase C — full run)"
    echo
    echo "| Workload | CLIPS RSS (KB) | ferric RSS (KB) | Ratio |"
    echo "|----------|---------------:|----------------:|------:|"

    for workload in $WORKLOADS; do
        clips_rss="$(jq -r "select(.workload == \"$workload\" and .engine == \"clips\") | .peak_rss_kb" "$tmp_entries")"
        ferric_rss="$(jq -r "select(.workload == \"$workload\" and .engine == \"ferric\") | .peak_rss_kb" "$tmp_entries")"

        if [[ -n "$clips_rss" ]] && [[ "$clips_rss" != "0" ]] && [[ "$clips_rss" != "null" ]]; then
            ratio="$(echo "scale=2; $ferric_rss / $clips_rss" | bc -l 2>/dev/null || echo "n/a")"
        else
            ratio="n/a"
        fi
        printf "| %-14s | %14s | %15s | %5s |\n" \
            "$workload" "${clips_rss:-n/a}" "${ferric_rss:-n/a}" "$ratio"
    done

    echo
    echo "---"
    echo "Generated by \`scripts/bench-compare.sh\`"
    echo "Full data: \`target/bench-compare/report.json\`"
} > "$report_md"

rm -f "$tmp_entries"

echo "==> Wrote ${report_json}"
echo "==> Wrote ${report_md}"
echo
cat "$report_md"
