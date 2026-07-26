#!/usr/bin/env bash
# Build both sides of the raw-engine diagnostic concurrency harness with
# ThreadSanitizer. This is intentionally separate from ffi-c-harness.sh:
# ThreadSanitizer cannot be combined with AddressSanitizer/UBSan, and the Rust
# static library (including std) must be instrumented for this check to be real.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

if [[ $(uname -s) != Linux || $(uname -m) != x86_64 ]]; then
    echo "ffi-tsan-harness: requires x86_64 Linux" >&2
    exit 1
fi

CC="${CC:-clang}"
tsan_toolchain="${FERRIC_TSAN_TOOLCHAIN:-nightly-2025-11-04}"
tsan_target="x86_64-unknown-linux-gnu"
outdir="$root/target/ffi-tsan"
staticlib="$outdir/$tsan_target/ffi-dev/libferric_ffi.a"
binary="$outdir/diagnostic_concurrency"

for command in "$CC" nm timeout; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "ffi-tsan-harness: required command not found: $command" >&2
        exit 1
    fi
done

CARGO_TARGET_DIR="$outdir" \
RUSTFLAGS="-Zsanitizer=thread -Zexternal-clangrt -Cforce-frame-pointers=yes" \
    cargo +"$tsan_toolchain" rustc \
        --locked \
        -Zbuild-std=std,panic_abort \
        --target "$tsan_target" \
        -p ferric-ffi \
        --profile ffi-dev \
        --crate-type staticlib

if ! nm -u "$staticlib" | grep "__tsan_" >/dev/null; then
    echo "ffi-tsan-harness: Rust static library has no TSan instrumentation" >&2
    exit 1
fi

"$CC" -std=c11 -O1 -g -Wall -Wextra -Werror \
    -fsanitize=thread -fno-omit-frame-pointer -pthread \
    -I crates/ferric-ffi \
    crates/ferric-ffi/tests/c/diagnostic_concurrency.c \
    "$staticlib" \
    -ldl -lm \
    -o "$binary"

TSAN_OPTIONS="halt_on_error=1:exitcode=66:detect_deadlocks=1" \
    timeout 120s "$binary"
