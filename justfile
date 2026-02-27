# ferric-rules justfile
# Run `just --list` to see all available recipes.

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

# ── Composite checks ────────────────────────────────────────────────────────

# Full preflight: format check, clippy, all tests, cargo check
check: fmt-check clippy test cargo-check

# Same as `check` — alias for the preflight script
preflight: check

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

# Run the manners benchmark suite
bench-manners:
    cargo bench -p ferric --bench manners_bench

# Run benchmark threshold evaluation
bench-thresholds:
    ./scripts/bench-thresholds.sh

# Run comparative benchmarks (ferric vs. CLIPS)
bench-compare *args:
    ./scripts/bench-compare.sh {{args}}

# ── Compatibility assessment ─────────────────────────────────────────────────

# Scan test files and produce a compatibility manifest
compat-scan *args:
    python3 scripts/compat-scan.py {{args}}

# Run compatibility tests against ferric and CLIPS
compat-run *args:
    python3 scripts/compat-run.py {{args}}

# Generate compatibility report from manifest
compat-report *args:
    python3 scripts/compat-report.py {{args}}

# Full compatibility assessment: scan, run, report
assess-compatibility: compat-scan compat-run compat-report

# ── Performance assessment ─────────────────────────────────────────────────

# Collect Criterion benchmark results into a performance manifest
perf-collect *args:
    python3 scripts/perf-collect.py {{args}}

# Generate performance report from manifest
perf-report *args:
    python3 scripts/perf-report.py {{args}}

# Compare two performance manifests
perf-diff *args:
    python3 scripts/perf-diff.py {{args}}

# Full performance assessment: collect (with CLIPS reference) and report
assess-performance: clips-build
    python3 scripts/perf-collect.py --clips-reference
    python3 scripts/perf-report.py

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

# ── Cleanup ──────────────────────────────────────────────────────────────────

# Remove build artifacts
clean:
    cargo clean
