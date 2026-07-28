#!/usr/bin/env bash
# Build the real-C adapter against the release-profile serde-enabled FFI archive.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

cc="${CC:-cc}"
outdir="target/bindings-conformance"
mkdir -p "$outdir"

case "$(uname -s)" in
Darwin)
    platform_libs=(-framework Security -framework CoreFoundation -lobjc)
    ;;
*)
    platform_libs=(-lpthread -ldl -lm)
    ;;
esac

"$cc" -std=c11 -pedantic-errors -Wall -Wextra -Werror \
    -DFERRIC_SERDE \
    -I crates/ferric-rules-ffi \
    -o "$outdir/c-adapter" \
    tests/bindings-conformance/adapters/c/adapter.c \
    target/release/libferric_rules_ffi.a \
    "${platform_libs[@]}"
