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

# Run Python tests
py-test:
    cd tools/ferric-tools && uv run pytest

# ── Composite checks ────────────────────────────────────────────────────────

# Full preflight: format check, clippy, all tests, cargo check, Python checks
check: fmt-check clippy test cargo-check py-fmt-check py-lint py-test

# Same as `check` — alias for the preflight script
preflight: check

# PR preflight: auto-fix formatting, then clippy + tests + cargo check + Python checks
preflight-pr: fmt clippy test cargo-check py-fmt py-lint-fix py-test

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

# ── Cleanup ──────────────────────────────────────────────────────────────────

# Remove build artifacts
clean:
    cargo clean
