#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${ROOT}/THIRD_PARTY_NOTICES.md"
TEMPLATE="${ROOT}/licenses/third-party-notices.hbs"
CONFIG="${ROOT}/about.toml"
CARGO_ABOUT_VERSION="0.9.0"

usage() {
    cat <<'EOF'
Usage: ./scripts/license-notices.sh [generate|check]

Commands:
  generate  Regenerate THIRD_PARTY_NOTICES.md from the locked Cargo graph.
  check     Fail if THIRD_PARTY_NOTICES.md is stale.
EOF
}

ensure_cargo_about() {
    if ! cargo about --version >/dev/null 2>&1; then
        echo "cargo-about is required. Install it with: cargo install --locked cargo-about --version ${CARGO_ABOUT_VERSION}" >&2
        exit 1
    fi

    actual_version="$(cargo about --version | awk '{print $2}')"
    if [[ "${actual_version}" != "${CARGO_ABOUT_VERSION}" ]]; then
        echo "cargo-about ${CARGO_ABOUT_VERSION} is required; found ${actual_version}." >&2
        echo "Install it with: cargo install --locked cargo-about --version ${CARGO_ABOUT_VERSION} --force" >&2
        exit 1
    fi
}

generate_notices() {
    cd "${ROOT}"
    cargo fetch --locked >/dev/null
    cargo about generate \
        --config "${CONFIG}" \
        --workspace \
        --all-features \
        --locked \
        --offline \
        --fail \
        "${TEMPLATE}"
}

command="${1:-check}"

ensure_cargo_about

case "${command}" in
generate)
    generate_notices >"${OUT}"
    ;;
check)
    tmp="$(mktemp)"
    trap 'rm -f "${tmp}"' EXIT
    generate_notices >"${tmp}"
    if ! diff -u "${OUT}" "${tmp}"; then
        echo "THIRD_PARTY_NOTICES.md is stale. Run: just license-notices" >&2
        exit 1
    fi
    ;;
-h | --help | help)
    usage
    ;;
*)
    echo "Unknown command: ${command}" >&2
    usage >&2
    exit 1
    ;;
esac
