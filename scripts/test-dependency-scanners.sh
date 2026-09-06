#!/usr/bin/env bash
# Small live smoke tests for the actual scanners, not a second policy evaluator.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
root="$PWD"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
evidence="$root/dependency-policy-evidence/scanner-tests"
mkdir -p "$evidence"

# Missing tools must not be mistaken for successful negative tests.
cargo deny --version
npm --version
uvx --from pip-audit==2.10.1 pip-audit --version

expect_rejection() {
    local name="$1"
    shift
    if "$@" > "$evidence/$name.log" 2>&1; then
        echo "ERROR: $name unexpectedly passed" >&2
        exit 1
    fi
}

# Exercise cargo-deny's own config parser; no repository config is modified.
printf '[advisories]\nyanked = "not-a-lint-level"\n' > "$scratch/deny.toml"
expect_rejection malformed-cargo-config cargo deny --locked \
    --config "$scratch/deny.toml" check advisories
rg 'error\[unexpected-value\]' "$evidence/malformed-cargo-config.log"

# lodash's affected template/merge APIs represent applicable runtime findings.
# This package exists only inside the temporary scanner fixture.
mkdir "$scratch/npm"
printf '{"name":"scanner-rejection-test","version":"1.0.0","private":true}\n' \
    > "$scratch/npm/package.json"
npm --prefix "$scratch/npm" install --package-lock-only --ignore-scripts \
    --audit=false --registry=https://registry.npmjs.org/ lodash@4.17.15 \
    > "$evidence/npm-fixture-install.log" 2>&1
expect_rejection vulnerable-npm npm --prefix "$scratch/npm" audit \
    --package-lock-only --json --audit-level=info --registry=https://registry.npmjs.org/
python3 - "$evidence/vulnerable-npm.log" <<'PY'
import json
import sys

report = json.load(open(sys.argv[1]))
assert report["vulnerabilities"]["lodash"]["via"], report
PY

# An older-Python marker must still be scanned on the current Python host.
mkdir "$scratch/python"
cat > "$scratch/python/pylock.toml" <<'TOML'
lock-version = "1.0"
[[packages]]
name = "pytest"
version = "8.4.2"
marker = "python_version < '3.10'"
TOML
expect_rejection vulnerable-python uvx --from pip-audit==2.10.1 pip-audit \
    --locked "$scratch/python" --strict --format json
python3 - "$evidence/vulnerable-python.log" <<'PY'
import json
import sys

# pip-audit's human summary goes to stderr, before or after its JSON report.
lines = open(sys.argv[1]).read().splitlines()
report = json.loads(next(line for line in lines if line.startswith("{")))
assert any(
    "GHSA-6w46-j5rx-g56g" in finding["aliases"]
    for package in report["dependencies"]
    for finding in package["vulns"]
), report
PY
printf 'lock-version = [\n' > "$scratch/python/pylock.toml"
expect_rejection malformed-python-lock uvx --from pip-audit==2.10.1 pip-audit \
    --locked "$scratch/python" --strict
rg 'invalid TOML' "$evidence/malformed-python-lock.log"
echo "Native scanner rejection checks passed."
