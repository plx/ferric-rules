#!/usr/bin/env bash
# Build the ferric-ffi static library and run the C ABI discriminant-abuse
# harness (crates/ferric-ffi/tests/c/) as a real C subprocess.
#
# The harness is compiled with AddressSanitizer + UndefinedBehaviorSanitizer
# when the local C compiler supports them (falls back to plain compilation
# otherwise). Set FERRIC_REQUIRE_SANITIZERS=1 to make missing sanitizer
# support fatal, as CI does.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

cargo build -p ferric-ffi --profile ffi-dev

CC="${CC:-cc}"
CXX="${CXX:-c++}"
harness_src="crates/ferric-ffi/tests/c/discriminant_abuse.c"
outdir="target/c-harness"
mkdir -p "$outdir"

san_flags=("-fsanitize=address,undefined" "-fno-sanitize-recover=all")
if ! echo 'int main(void){return 0;}' |
    "$CC" "${san_flags[@]}" -x c -o "$outdir/san-probe" - 2>/dev/null; then
    if [[ ${FERRIC_REQUIRE_SANITIZERS:-0} == 1 ]]; then
        echo "ffi-c-harness: ERROR: compiler lacks required ASan/UBSan support" >&2
        exit 1
    fi
    echo "ffi-c-harness: compiler lacks ASan/UBSan support; running unsanitized" >&2
    san_flags=()
fi
rm -f "$outdir/san-probe"

case "$(uname -s)" in
Darwin)
    platform_libs=(-framework Security -framework CoreFoundation -lobjc)
    ;;
*)
    platform_libs=(-lpthread -ldl -lm)
    ;;
esac

# Negative check: a consumer compiled with -fshort-enums (packed-enum ABI)
# must be rejected by the header's enum-width static assertions, where the
# compiler supports the flag.
if echo 'int main(void){return 0;}' |
    "$CC" -std=c11 -fshort-enums -x c -o "$outdir/short-enum-probe" - 2>/dev/null; then
    rm -f "$outdir/short-enum-probe"
    if printf '#include "ferric.h"\nint main(void){return 0;}\n' |
        "$CC" -std=c11 -fshort-enums -fsyntax-only -I crates/ferric-ffi -x c - 2>/dev/null; then
        echo "ffi-c-harness: ERROR: ferric.h compiled under -fshort-enums;" \
            "enum ABI static assertions failed to fire" >&2
        exit 1
    fi
    echo "ffi-c-harness: -fshort-enums correctly rejected by ABI static assertions"
else
    echo "ffi-c-harness: compiler lacks -fshort-enums; skipping negative check" >&2
fi

# Pre-C++11 consumers must use the typedef-based static-assertion fallback,
# and the header must be strictly conforming (-pedantic-errors rejects
# extensions such as trailing enum commas that C++98/03 do not allow).
for cxx_std in c++98 c++03; do
    printf '#include "ferric.h"\nint main(){return 0;}\n' |
        "$CXX" -std="$cxx_std" -pedantic-errors -Wall -Wextra -Werror \
            -DFERRIC_SERDE -fsyntax-only -I crates/ferric-ffi -x c++ -
done

# The typedef fallback must itself reject packed-enum ABIs: probe for
# C++98 -fshort-enums support, then require the header to fail to compile.
if echo 'int main(){return 0;}' |
    "$CXX" -std=c++98 -fshort-enums -x c++ \
        -o "$outdir/cxx98-short-enum-probe" - 2>/dev/null; then
    rm -f "$outdir/cxx98-short-enum-probe"
    if printf '#include "ferric.h"\nint main(){return 0;}\n' |
        "$CXX" -std=c++98 -fshort-enums -DFERRIC_SERDE \
            -fsyntax-only -I crates/ferric-ffi -x c++ - 2>/dev/null; then
        echo "ffi-c-harness: ERROR: ferric.h compiled under C++98 -fshort-enums;" \
            "typedef-fallback ABI assertions failed to fire" >&2
        exit 1
    fi
    echo "ffi-c-harness: C++98 -fshort-enums correctly rejected by typedef fallback"
else
    echo "ffi-c-harness: compiler lacks C++98 -fshort-enums; skipping negative check" >&2
fi

"$CC" -std=c11 -Wall -Wextra -Werror "${san_flags[@]}" \
    -I crates/ferric-ffi \
    -o "$outdir/discriminant_abuse" \
    "$harness_src" \
    target/ffi-dev/libferric_ffi.a \
    "${platform_libs[@]}"

"$outdir/discriminant_abuse"
