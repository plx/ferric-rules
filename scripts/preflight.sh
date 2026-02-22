#!/usr/bin/env bash

set -euo pipefail

usage() {
    cat <<'EOF'
Usage: ./scripts/preflight.sh [check|fmt|clippy|test|all]

Commands:
  check   Run cargo check for the workspace.
  fmt     Verify formatting.
  clippy  Run clippy with warnings denied.
  test    Run workspace tests.
  all     Run fmt, clippy, test, and check in sequence.
EOF
}

run_check() {
    cargo check --workspace
}

run_fmt() {
    cargo fmt --all --check
}

run_clippy() {
    cargo clippy --workspace --all-targets -- -D warnings
}

run_test() {
    cargo test --workspace
}

command="${1:-all}"

case "${command}" in
check)
    run_check
    ;;
fmt)
    run_fmt
    ;;
clippy)
    run_clippy
    ;;
test)
    run_test
    ;;
all)
    run_fmt
    run_clippy
    run_test
    run_check
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
