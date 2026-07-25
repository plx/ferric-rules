#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

exec uv run \
    --project "$REPO_ROOT/tools/ferric-tools" \
    ferric-next-production-readiness-issue "$@"
