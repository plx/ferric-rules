#!/usr/bin/env bash
# Repeat the accepted-request cancellation/panic race with the pinned worker,
# completion envelope, and Rust standard library instrumented by TSan.
set -euo pipefail

task_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$task_root"

if [[ $(uname -s) != Linux || $(uname -m) != x86_64 ]]; then
    echo "pinned-async-tsan: requires x86_64 Linux" >&2
    exit 1
fi

tsan_toolchain="${FERRIC_TSAN_TOOLCHAIN:-nightly-2025-11-04}"
tsan_target="x86_64-unknown-linux-gnu"
outdir="$task_root/target/pinned-async-tsan"

if ! command -v "cargo" >/dev/null 2>&1; then
    echo "pinned-async-tsan: cargo is required" >&2
    exit 1
fi

CARGO_TARGET_DIR="$outdir" \
RUSTFLAGS="-Zsanitizer=thread -Cforce-frame-pointers=yes" \
TSAN_OPTIONS="halt_on_error=1:exitcode=66:detect_deadlocks=1" \
    cargo +"$tsan_toolchain" test \
        --locked \
        -Zbuild-std=std,panic_unwind \
        --target "$tsan_target" \
        -p ferric-rules-pinned \
        --lib \
        engine::completion_tests::cancellation_racing_operation_panic_finalizes_once \
        -- \
        --exact
