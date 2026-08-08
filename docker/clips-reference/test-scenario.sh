#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

image_name="${CLIPS_REFERENCE_IMAGE:-ferric-rules/clips-reference}"
image_tag="${CLIPS_REFERENCE_TAG:-latest}"
scratch="$(mktemp -d "$repo_root/tests/examples/.clips-scenario-smoke.XXXXXX")"
scratch_rel="${scratch:$(( ${#repo_root} + 1 ))}"
trap 'rm -rf "$scratch"' EXIT

nonce="0123456789abcdef0123456789abcdef"
auth_key="0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
run_index=0
last_status=0
last_stdout=""
last_stderr=""

fail() {
  echo "scenario smoke: $*" >&2
  exit 1
}

digest() {
  shasum -a 256 "$1" | awk '{print $1}'
}

run_scenario() {
  local plan="$1"
  local source_sha256="$2"
  shift 2
  local composed_sha256
  local command
  local operation
  composed_sha256="$(digest "$plan")"
  run_index=$((run_index + 1))
  last_stdout="$scratch/run-${run_index}.stdout"
  last_stderr="$scratch/run-${run_index}.stderr"

  command=(
    scripts/clips-reference.sh run
    --image "$image_name"
    --tag "$image_tag"
    --quiet
    --scenario "$plan"
    --observer-nonce "$nonce"
    --observer-fixture-id native-scenario-smoke
    --observer-source-sha256 "$source_sha256"
    --observer-composed-sha256 "$composed_sha256"
    --observer-auth-key "$auth_key"
    --observer-container-name "ferric-scenario-smoke-${run_index}"
  )
  for operation in "$@"; do
    command+=(--op "$operation")
  done
  command+=(--op '(ferric-compat-native-complete)')

  set +e
  "${command[@]}" >"$last_stdout" 2>"$last_stderr"
  last_status=$?
  set -e
}

assert_status() {
  local expected="$1"
  [[ "$last_status" -eq "$expected" ]] || {
    sed -n '1,160p' "$last_stderr" >&2
    fail "expected status $expected, got $last_status"
  }
}

assert_stderr() {
  local expected="$1"
  grep -F -- "$expected" "$last_stderr" >/dev/null || {
    sed -n '1,160p' "$last_stderr" >&2
    fail "missing stderr evidence: $expected"
  }
}

cat >"$scratch/primary.clp" <<'CLIPS'
(deftemplate marker (slot value))
(deffacts seed (marker (value primary)))
CLIPS

cat >"$scratch/staged.clp" <<'CLIPS'
(defrule staged
  (marker (value primary))
  =>
  (assert (marker (value fired))))
CLIPS

primary_sha256="$(digest "$scratch/primary.clp")"
staged_sha256="$(digest "$scratch/staged.clp")"
cat >"$scratch/staged.plan" <<PLAN
FERRIC-COMPAT-SCENARIO|1
SOURCE|primary|$primary_sha256|$scratch_rel/primary.clp
SOURCE|staged|$staged_sha256|$scratch_rel/staged.clp
STEP|1|LOAD|primary|stop
STEP|2|RESET|-|stop
STEP|3|LOAD|staged|stop
STEP|4|SET-STRATEGY|breadth|stop
STEP|5|RUN|-1|stop
END
PLAN
run_scenario "$scratch/staged.plan" "$primary_sha256"
assert_status 0
assert_stderr '|PHASE|1|load|BEGIN|'
assert_stderr '|PHASE|4|reset|END|OK|'
assert_stderr '|PHASE|5|load|BEGIN|'
assert_stderr '|PHASE|8|run|END|OK|'
assert_stderr '|RUN|-1|1|0|0|0|0|0|'

cat >"$scratch/action-error.clp" <<'CLIPS'
(deftemplate trace (slot label))
(deffacts input (value nope))
(defrule failing
  (declare (salience 10))
  (value ?value)
  =>
  (assert (trace (label before-error)))
  (+ ?value 1)
  (assert (trace (label after-error))))
(defrule later
  =>
  (assert (trace (label later-rule))))
CLIPS
action_error_sha256="$(digest "$scratch/action-error.clp")"
cat >"$scratch/action-error.plan" <<PLAN
FERRIC-COMPAT-SCENARIO|1
SOURCE|primary|$action_error_sha256|$scratch_rel/action-error.clp
STEP|1|LOAD|primary|stop
STEP|2|RESET|-|stop
STEP|3|RUN|-1|stop
END
PLAN
run_scenario \
  "$scratch/action-error.plan" \
  "$action_error_sha256" \
  '(ferric-compat-native-emit "MODULE|MAIN")'
assert_status 0
assert_stderr '|DIAGNOSTIC|1|run|0|'
assert_stderr '|PHASE|6|run|END|ERROR|'
assert_stderr '|RUN|-1|1|0|1|0|1|0|'
assert_stderr '|PROBE|11|MODULE|MAIN|'
assert_stderr '|LIFECYCLE|3|COMPLETE|'

cat >"$scratch/broken.clp" <<'CLIPS'
(defrule incomplete
CLIPS
broken_sha256="$(digest "$scratch/broken.clp")"
cat >"$scratch/continue.plan" <<PLAN
FERRIC-COMPAT-SCENARIO|1
SOURCE|primary|$primary_sha256|$scratch_rel/primary.clp
SOURCE|broken|$broken_sha256|$scratch_rel/broken.clp
SOURCE|staged|$staged_sha256|$scratch_rel/staged.clp
STEP|1|LOAD|primary|stop
STEP|2|LOAD|broken|continue
STEP|3|RESET|-|stop
STEP|4|LOAD|staged|stop
STEP|5|RUN|-1|stop
END
PLAN
run_scenario "$scratch/continue.plan" "$primary_sha256"
assert_status 0
assert_stderr '|DIAGNOSTIC|1|load|1|'
assert_stderr '|PHASE|4|load|END|CONTINUED|'
assert_stderr '|PHASE|10|run|END|OK|'
assert_stderr '|RUN|-1|1|0|0|0|0|0|'

wrong_sha256="$(printf wrong | shasum -a 256 | awk '{print $1}')"
cat >"$scratch/wrong-digest.plan" <<PLAN
FERRIC-COMPAT-SCENARIO|1
SOURCE|primary|$wrong_sha256|$scratch_rel/primary.clp
STEP|1|LOAD|primary|stop
STEP|2|RESET|-|stop
STEP|3|RUN|-1|stop
END
PLAN
run_scenario "$scratch/wrong-digest.plan" "$wrong_sha256"
assert_status 127
assert_stderr 'scenario source digest does not match'

cat >"$scratch/path-escape.plan" <<PLAN
FERRIC-COMPAT-SCENARIO|1
SOURCE|primary|$primary_sha256|tests/examples/../../README.md
STEP|1|LOAD|primary|stop
STEP|2|RESET|-|stop
STEP|3|RUN|-1|stop
END
PLAN
run_scenario "$scratch/path-escape.plan" "$primary_sha256"
assert_status 127
assert_stderr 'scenario SOURCE record is invalid'

dd if=/dev/zero of="$scratch/oversized.clp" bs=1048576 count=16 status=none
printf x >>"$scratch/oversized.clp"
oversized_sha256="$(digest "$scratch/oversized.clp")"
cat >"$scratch/oversized.plan" <<PLAN
FERRIC-COMPAT-SCENARIO|1
SOURCE|primary|$oversized_sha256|$scratch_rel/oversized.clp
STEP|1|LOAD|primary|stop
STEP|2|RESET|-|stop
STEP|3|RUN|-1|stop
END
PLAN
run_scenario "$scratch/oversized.plan" "$oversized_sha256"
assert_status 127
assert_stderr 'scenario source size is invalid'

python3 - "$scratch/bounded.clp" <<'PY'
from pathlib import Path
import sys

size = 16 * 1024 * 1024
prefix = b"(deffacts boundary)\n;"
Path(sys.argv[1]).write_bytes(prefix + (b"x" * (size - len(prefix) - 1)) + b"\n")
PY
ln "$scratch/bounded.clp" "$scratch/bounded-2.clp"
ln "$scratch/bounded.clp" "$scratch/bounded-3.clp"
ln "$scratch/bounded.clp" "$scratch/bounded-4.clp"
bounded_sha256="$(digest "$scratch/bounded.clp")"
cat >"$scratch/exact-aggregate.plan" <<PLAN
FERRIC-COMPAT-SCENARIO|1
SOURCE|primary|$bounded_sha256|$scratch_rel/bounded.clp
SOURCE|second|$bounded_sha256|$scratch_rel/bounded-2.clp
SOURCE|third|$bounded_sha256|$scratch_rel/bounded-3.clp
SOURCE|fourth|$bounded_sha256|$scratch_rel/bounded-4.clp
STEP|1|LOAD|primary|stop
STEP|2|RESET|-|stop
STEP|3|RUN|-1|stop
END
PLAN
run_scenario "$scratch/exact-aggregate.plan" "$bounded_sha256"
assert_status 0
assert_stderr '|PHASE|6|run|END|OK|'

cat >"$scratch/aggregate.plan" <<PLAN
FERRIC-COMPAT-SCENARIO|1
SOURCE|primary|$bounded_sha256|$scratch_rel/bounded.clp
SOURCE|second|$bounded_sha256|$scratch_rel/bounded-2.clp
SOURCE|third|$bounded_sha256|$scratch_rel/bounded-3.clp
SOURCE|fourth|$bounded_sha256|$scratch_rel/bounded-4.clp
SOURCE|fifth|$primary_sha256|$scratch_rel/primary.clp
STEP|1|LOAD|primary|stop
STEP|2|RESET|-|stop
STEP|3|RUN|-1|stop
END
PLAN
run_scenario "$scratch/aggregate.plan" "$bounded_sha256"
assert_status 127
assert_stderr 'scenario aggregate source size is invalid'

provenance="$(scripts/clips-reference.sh provenance --image "$image_name" --tag "$image_tag")"
python3 - "$provenance" <<'PY'
import json
import re
import sys

raw = json.loads(sys.argv[1])
expected = {
    "schema",
    "version",
    "engine",
    "engine_version",
    "package",
    "package_version",
    "platform",
    "binary_sha256",
    "library_sha256",
    "base_image",
    "image_id",
}
assert set(raw) == expected
assert raw["schema"] == "ferric.clips-reference-provenance"
assert raw["version"] == 1
assert raw["engine"] == raw["package"] == "clips"
assert raw["engine_version"] == "6.30"
assert raw["package_version"] == "6.30-4.1"
assert raw["platform"] in {"linux/amd64", "linux/arm64"}
assert re.fullmatch(r"[0-9a-f]{64}", raw["binary_sha256"])
assert re.fullmatch(r"[0-9a-f]{64}", raw["library_sha256"])
assert re.fullmatch(r"debian:bookworm-slim@sha256:[0-9a-f]{64}", raw["base_image"])
assert re.fullmatch(r"sha256:[0-9a-f]{64}", raw["image_id"])
PY

echo "native CLIPS scenario smoke passed"
