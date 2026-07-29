#!/usr/bin/env bash
# Build the Rust static library with AddressSanitizer instrumentation, link it
# into Go's cgo binding under Go's ASan mode, and run the real-handle
# lifecycle and recursive multifield-ownership regressions.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

if [[ $(uname -s) != Linux ]]; then
    echo "ffi-go-asan-harness: requires Linux" >&2
    exit 1
fi

asan_toolchain="${FERRIC_ASAN_TOOLCHAIN:-nightly-2025-11-04}"
case "$(uname -m)" in
x86_64)
    asan_target="x86_64-unknown-linux-gnu"
    ;;
aarch64 | arm64)
    asan_target="aarch64-unknown-linux-gnu"
    ;;
*)
    echo "ffi-go-asan-harness: unsupported architecture: $(uname -m)" >&2
    exit 1
    ;;
esac
outdir="$root/target/ffi-go-asan"
staticlib="$outdir/$asan_target/ffi-dev/libferric_rules_ffi.a"
go_staticlib="$root/bindings/go/internal/ffi/lib/libferric_rules_ffi.a"
backup_dir="$(mktemp -d)"
had_staticlib=0

restore_staticlib() {
    if ((had_staticlib)); then
        cp "$backup_dir/libferric_rules_ffi.a" "$go_staticlib"
    else
        rm -f "$go_staticlib"
    fi
    rm -rf "$backup_dir"
}
trap restore_staticlib EXIT

for command in go nm; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "ffi-go-asan-harness: required command not found: $command" >&2
        exit 1
    fi
done

if [[ -f "$go_staticlib" ]]; then
    cp "$go_staticlib" "$backup_dir/libferric_rules_ffi.a"
    had_staticlib=1
fi

CARGO_TARGET_DIR="$outdir" \
CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}" \
RUSTFLAGS="-Zsanitizer=address -Zexternal-clangrt -Cforce-frame-pointers=yes" \
    cargo +"$asan_toolchain" rustc \
        --locked \
        -Zbuild-std=std,panic_unwind \
        --target "$asan_target" \
        -p ferric-rules-ffi \
        --profile ffi-dev \
        --features serde \
        --crate-type staticlib

if ! nm -u "$staticlib" | grep "__asan_" >/dev/null; then
    echo "ffi-go-asan-harness: Rust static library has no ASan instrumentation" >&2
    exit 1
fi

cp "$staticlib" "$go_staticlib"

cd bindings/go
ASAN_OPTIONS="${ASAN_OPTIONS:-detect_leaks=1:halt_on_error=1}" \
    go test -asan -count=1 \
        -run '^(TestEngineRealFFIPostClose|TestManualFFIValueConversionEdges|TestMultifieldAllocatorProvenanceRoundTrip|TestMultifieldCopyDepthBoundaryCleansUp|TestValueMultifieldCopy.*)$' \
        . ./internal/ffi
