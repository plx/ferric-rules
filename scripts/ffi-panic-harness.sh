#!/usr/bin/env bash
# Build unwind-capable FFI artifacts with internal panic injection, exercise
# every C return category in real subprocesses, and audit generated exports.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

CC="${CC:-cc}"
outroot="$root/target/ffi-panic-harness"
mkdir -p "$outroot"

case "$(uname -s)" in
Darwin)
    platform_libs=(-framework Security -framework CoreFoundation -lobjc)
    ;;
*)
    platform_libs=(-lpthread -ldl -lm)
    ;;
esac

audit_symbols() {
    local staticlib="$1"
    local table="$outroot/symbols.txt"
    local expected="$outroot/expected-symbols.txt"
    local authored="$outroot/authored-symbols.txt"
    local exported="$outroot/ferric-symbols.txt"

    case "$(uname -s)" in
    Darwin)
        nm -gU "$staticlib" | awk '{print $NF}' | sed 's/^_//' | sort -u >"$table"
        ;;
    *)
        nm -g --defined-only "$staticlib" |
            awk '{print $NF}' | sed 's/^_//' | sort -u >"$table"
        ;;
    esac

    sed -nE 's/.*[ *](ferric_[a-z0-9_]+)\(.*/\1/p' \
        crates/ferric-rules-ffi/ferric.h | sort -u >"$expected"
    # Derive coverage from the actual authored ABI, not a count that becomes
    # stale when new functions use an already covered return category.
    sed -nE 's/^[[:space:]]*pub( unsafe)? extern "C" fn (ferric_[a-z0-9_]+)\(.*/\2/p' \
        crates/ferric-rules-ffi/src/{engine,error,pinned,types}.rs | sort -u >"$authored"
    if [[ ! -s "$expected" || ! -s "$authored" ]]; then
        echo "ffi-panic-harness: header/source export inventory is empty" >&2
        exit 1
    fi
    if ! diff -u "$authored" "$expected"; then
        echo "ffi-panic-harness: authored/header export coverage differs" >&2
        exit 1
    fi
    awk '/^ferric_[a-z0-9_]+$/ { print }' "$table" >"$exported"
    if ! diff -u "$expected" "$exported"; then
        echo "ffi-panic-harness: header/archive export coverage differs" >&2
        exit 1
    fi
}

for profile in ffi-dev ffi-release; do
    target_dir="$outroot/$profile"
    FERRIC_FFI_TEST_PANIC_INJECTION_BUILD=1 \
        CARGO_TARGET_DIR="$target_dir" cargo build --locked \
        -p ferric-rules-ffi \
        --profile "$profile" \
        --features serde

    staticlib="$target_dir/$profile/libferric_rules_ffi.a"
    binary="$target_dir/panic_containment"
    "$CC" -std=c11 -Wall -Wextra -Werror \
        -I crates/ferric-rules-ffi \
        -o "$binary" \
        crates/ferric-rules-ffi/tests/c/panic_containment.c \
        "$staticlib" \
        "${platform_libs[@]}"
    "$binary"
    audit_symbols "$staticlib"
done
