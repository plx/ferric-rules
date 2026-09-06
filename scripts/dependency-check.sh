#!/usr/bin/env bash
# Delegate policy decisions and report parsing to the ecosystem scanners.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

evidence="${1:-dependency-policy-evidence}"
mkdir -p "$evidence"
evidence="$(cd "$evidence" && pwd)"
status=0

check() {
    local name="$1"
    shift
    if ! "$@" 2>&1 | tee "$evidence/$name.log"; then
        status=1
    fi
}

check rust cargo deny --locked --all-features check advisories bans licenses sources
check notices ./scripts/license-notices.sh check

for project in packages/ferric crates/ferric-rules-napi documentation site; do
    check "${project//\//-}" npm --prefix "$project" audit --package-lock-only \
        --json --audit-level=info --include=dev --include=optional --include=peer \
        --registry=https://registry.npmjs.org/
done

for project in crates/ferric-rules-python tools/ferric-tools; do
    name="${project//\//-}"
    mkdir -p "$evidence/$name"
    # pylock preserves every locked version, including non-host Python/OS markers.
    # pip-audit reads it directly, without installing or executing dependencies.
    if ! uv export --project "$project" --locked --all-groups --all-extras \
        --no-emit-project --format pylock.toml \
        --output-file "$evidence/$name/pylock.toml" > "$evidence/$name-export.log" 2>&1; then
        cat "$evidence/$name-export.log"
        status=1
        continue
    fi
    audit=(--locked "$evidence/$name" --strict --format json)
    if [[ "$project" == crates/ferric-rules-python ]]; then
        # pytest 8 is needed only for Python 3.9 tests. conftest.py supplies a
        # private random basetemp; remove when pytest backports or 3.9 is retired.
        audit+=(--ignore-vuln GHSA-6w46-j5rx-g56g)
    fi
    check "$name" uvx --from pip-audit==2.10.1 pip-audit "${audit[@]}"
done

exit "$status"
