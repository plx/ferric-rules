# ferric-rules justfile
# Run `just --list` to see all available recipes.
set quiet

# Pin PyO3 to a uv-managed Python 3.12 so builds work regardless of system Python.
export PYO3_PYTHON := `uv python find 3.12 2>/dev/null || echo python3`

default:
    @just --list

# ── Workspace-wide builds ────────────────────────────────────────────────────

# Build the entire workspace (debug)
build:
    cargo build --workspace

# Build the entire workspace (release)
build-release:
    cargo build --workspace --release

# ── Per-crate builds ─────────────────────────────────────────────────────────

# Build a single crate by name (e.g. `just build-crate ferric-parser`)
build-crate crate:
    cargo build -p {{crate}}

# Build the FFI crate with panic=abort (dev)
build-ffi:
    cargo build -p ferric-ffi --profile ffi-dev

# Build the FFI crate with panic=abort (release)
build-ffi-release:
    cargo build -p ferric-ffi --profile ffi-release

# Build the CLI
build-cli:
    cargo build -p ferric-cli

# Build the CLI (release)
build-cli-release:
    cargo build -p ferric-cli --release

# ── Testing ──────────────────────────────────────────────────────────────────

# Run all workspace tests
test:
    cargo test --workspace

# Run tests for a single crate (e.g. `just test-crate ferric-core`)
test-crate crate:
    cargo test -p {{crate}}

# Run tests matching a filter (e.g. `just test-filter rete`)
test-filter filter:
    cargo test --workspace -- {{filter}}

# Run tests for the core crate
test-core:
    cargo test -p ferric-core

# Run tests for the parser crate
test-parser:
    cargo test -p ferric-parser

# Run tests for the runtime crate
test-runtime:
    cargo test -p ferric-runtime

# Run tests for the FFI crate
test-ffi:
    cargo test -p ferric-ffi

# Run tests for the facade crate
test-ferric:
    cargo test -p ferric

# Run tests for the CLI crate
test-cli:
    cargo test -p ferric-cli

# ── Linting & formatting ────────────────────────────────────────────────────

# Check formatting (no changes)
fmt-check:
    cargo fmt --all --check

# Apply formatting fixes
fmt:
    cargo fmt --all

# Run clippy with warnings denied
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Run cargo check
cargo-check:
    cargo check --workspace

# ── Python tooling ───────────────────────────────────────────────────────────

# Helper to run ferric-tools commands
_uv *args:
    cd tools/ferric-tools && uv run {{args}}

# Check Python formatting
py-fmt-check:
    cd tools/ferric-tools && uv run ruff format --check src/ tests/

# Apply Python formatting
py-fmt:
    cd tools/ferric-tools && uv run ruff format src/ tests/

# Run Python linter
py-lint:
    cd tools/ferric-tools && uv run ruff check src/ tests/

# Run Python linter with auto-fix
py-lint-fix:
    cd tools/ferric-tools && uv run ruff check --fix src/ tests/

# Run Python tests (tools)
py-test:
    cd tools/ferric-tools && uv run pytest

# Build and test Python bindings
py-bindings-test:
    cd crates/ferric-python && PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1 uv run maturin develop --quiet && .venv/bin/python -m pytest tests/

# ── Composite checks ────────────────────────────────────────────────────────

# Full preflight: format check, clippy, all tests, cargo check, Python checks, Go lint
check: fmt-check clippy test cargo-check py-fmt-check py-lint py-test py-bindings-test go-lint ts-lint

# Same as `check` — alias for the preflight script
preflight: check

# PR preflight: auto-fix formatting, then clippy + tests + cargo check + Python checks + Go lint
preflight-pr: fmt clippy test cargo-check py-fmt py-lint-fix py-test py-bindings-test go-lint ts-lint

# ── Tracing / profiling ────────────────────────────────────────────────────

# Check that tracing feature compiles and passes clippy + tests
check-tracing:
    cargo check --workspace --exclude ferric-python --features tracing
    cargo clippy --workspace --exclude ferric-python --features tracing --all-targets -- -D warnings
    cargo test --workspace --exclude ferric-python --features tracing

# ── Benchmarks ───────────────────────────────────────────────────────────────

# Run all Criterion benchmarks
bench:
    cargo bench -p ferric

# Run the engine benchmark suite
bench-engine:
    cargo bench -p ferric --bench engine_bench

# Run the waltz benchmark suite
bench-waltz:
    cargo bench -p ferric --bench waltz_bench

# Run serialization format comparison benchmarks
bench-serde:
    cargo bench -p ferric-runtime --features serde --bench serialization_bench

# Run the manners benchmark suite
bench-manners:
    cargo bench -p ferric --bench manners_bench

# Run the join benchmark suite
bench-join:
    cargo bench -p ferric --bench join_bench

# Run the churn benchmark suite
bench-churn:
    cargo bench -p ferric --bench churn_bench

# Run the negation benchmark suite
bench-negation:
    cargo bench -p ferric --bench negation_bench

# Run benchmark threshold evaluation
bench-thresholds:
    ./scripts/bench-thresholds.sh

# Run comparative benchmarks (ferric vs. CLIPS)
bench-compare *args:
    ./scripts/bench-compare.sh {{args}}

# Run scaling regression checks (catches accidentally-quadratic behavior)
scaling-check:
    cargo test -p ferric --test scaling_tests --release -- --ignored --nocapture --test-threads=1

# ── Compatibility assessment ─────────────────────────────────────────────────

# Scan test files and produce a compatibility manifest
compat-scan *args:
    just _uv ferric-compat-scan {{args}}

# Run compatibility tests against ferric and CLIPS
compat-run *args:
    just _uv ferric-compat-run {{args}}

# Generate compatibility report from manifest
compat-report *args:
    just _uv ferric-compat-report {{args}}

# Compare two compat manifests
compat-diff *args:
    just _uv ferric-compat-diff {{args}}

# Full compatibility assessment: scan, run, report
assess-compatibility: compat-scan compat-run compat-report

# ── Bat processing ───────────────────────────────────────────────────────────

# Analyze .bat files from the test suite
bat-analyze *args:
    just _uv ferric-bat-analyze {{args}}

# Extract standalone .clp segments from .bat analysis
bat-extract *args:
    just _uv ferric-bat-extract {{args}}

# Convert benchmark .bat files into self-contained .clp files
bat-convert *args:
    just _uv ferric-bat-convert {{args}}

# Generate harness files for library-only .clp files
harness-gen *args:
    just _uv ferric-harness-gen {{args}}

# Run segment check against extracted segments
segment-check *args:
    just _uv ferric-segment-check {{args}}

# ── Performance assessment ─────────────────────────────────────────────────

# Collect Criterion benchmark results into a performance manifest
perf-collect *args:
    just _uv ferric-perf-collect {{args}}

# Generate performance report from manifest
perf-report *args:
    just _uv ferric-perf-report {{args}}

# Compare two performance manifests
perf-diff *args:
    just _uv ferric-perf-diff {{args}}

# Full performance assessment: collect (with CLIPS reference) and report
assess-performance: clips-build
    just _uv ferric-perf-collect --clips-reference
    just _uv ferric-perf-report

# ── CLIPS reference ─────────────────────────────────────────────────────────

# Build the CLIPS Docker reference image
clips-build *args:
    ./scripts/clips-reference.sh build {{args}}

# Run a .clp file through the CLIPS Docker reference
clips-run *args:
    ./scripts/clips-reference.sh run {{args}}

# ── Documentation ────────────────────────────────────────────────────────────

# Build rustdoc for the workspace
doc:
    cargo doc --workspace --no-deps

# Build and open rustdoc in a browser
doc-open:
    cargo doc --workspace --no-deps --open

# ── Go bindings ─────────────────────────────────────────────────────────────

# Build the Rust static library for Go bindings (includes serde for serialization)
build-go-ffi:
    cargo build -p ferric-ffi --release --features serde
    cp target/release/libferric_ffi.a bindings/go/internal/ffi/lib/
    cp crates/ferric-ffi/ferric.h bindings/go/internal/ffi/lib/

# Run Go binding tests
test-go:
    cd bindings/go && go test -v ./...

# Run Go binding tests with race detector
test-go-race:
    cd bindings/go && go test -race -v ./...

# Run Go binding tests repeatedly to detect affinity-sensitive flakes (default: 10 iterations)
test-go-stress count="10":
    cd bindings/go && go test -race -count={{count}} -v ./...

# Version of golangci-lint to install when not already present.
golangci_lint_version := "v2.8.0"

# Runs the golang-ci linter suite (auto-installs to ./bin/ if needed).
run-golang-ci:
    #!/usr/bin/env bash
    set -euo pipefail

    if command -v golangci-lint >/dev/null 2>&1; then
        LINT=golangci-lint
    elif [[ -x ./bin/golangci-lint ]]; then
        LINT=./bin/golangci-lint
    else
        echo "golangci-lint not found; installing {{golangci_lint_version}} to ./bin/ ..." >&2
        curl -sSfL https://raw.githubusercontent.com/golangci/golangci-lint/HEAD/install.sh \
            | sh -s -- -b ./bin {{golangci_lint_version}}
        LINT=./bin/golangci-lint
    fi

    cd bindings/go
    # Adjust relative path after cd
    [[ "$LINT" == ./bin/* ]] && LINT="../../bin/golangci-lint"
    "$LINT" run

# Run Go lint checks for bindings.
go-lint: build-go-ffi
    just run-golang-ci

# Full Go build pipeline: build Rust static lib, then run Go tests
go-full: build-go-ffi test-go-race

# ── Node.js bindings ───────────────────────────────────────────────────────

# Build the napi-rs native addon for Node.js bindings
build-napi:
    cd crates/ferric-napi && npm run build

# Install TypeScript package dependencies
ts-install:
    cd packages/ferric && npm install

# Build the TypeScript package
ts-build: ts-install
    cd packages/ferric && npm run build

# Lint TypeScript (type-check only)
ts-lint: ts-install
    cd packages/ferric && npm run lint

# Run Node.js binding tests
test-napi: build-napi ts-build
    cd packages/ferric && npm test

# Full Node.js build pipeline: build native addon, then build and test TS
napi-full: build-napi ts-build test-napi

# ── Cleanup ──────────────────────────────────────────────────────────────────

# Remove build artifacts
clean:
    cargo clean

# ── Issue tracking ────────────────────────────────────────────────────────────

# Find the next unblocked issue matching comma-separated labels (e.g. `just find-next-matching-issue golang-binding,remediation`)
find-next-matching-issue labels:
    ./scripts/find-next-matching-issue.sh {{labels}}

# List open GitHub issues matching comma-separated labels as a markdown table (e.g. `just list-open-issues golang-binding,remediation`)
list-open-issues labels:
    ./scripts/list-open-issues.sh {{labels}}
