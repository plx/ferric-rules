#!/usr/bin/env bash
# Build one host wheel and use it from a fresh environment outside the checkout.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
package="$root/crates/ferric-rules-python"
if [[ $# -gt 1 ]]; then
    echo "usage: scripts/python-consumer-smoke.sh [python-interpreter]" >&2
    exit 1
fi
# Leave the binding's development environment alone: find, never sync.
candidate="${1:-$(uv python find --project "$package")}"
python_path="$("$candidate" -c 'import sys; print(sys.executable)')"
smoke_dir="$(mktemp -d "${TMPDIR:-/tmp}/ferric-python-consumer.XXXXXX")"
trap 'rm -rf "$smoke_dir"' EXIT
# A caller's TMPDIR must not turn this into an in-checkout import test.
"$python_path" -c '
from pathlib import Path
import sys
root, temporary = (Path(value).resolve() for value in sys.argv[1:])
if root == temporary or root in temporary.parents:
    raise SystemExit("python-consumer-smoke: TMPDIR must be outside the checkout")
' "$root" "$smoke_dir"
# The locked maturin comes from a throwaway environment, not the project .venv.
UV_PROJECT_ENVIRONMENT="$smoke_dir/build-env" uv sync --project "$package" \
    --locked --no-install-project --python "$python_path" --quiet
"$smoke_dir/build-env/bin/maturin" build \
    --manifest-path "$package/Cargo.toml" --release --locked \
    --interpreter "$python_path" --out "$smoke_dir/wheels"
shopt -s nullglob
wheels=("$smoke_dir"/wheels/*.whl)
if [[ ${#wheels[@]} -ne 1 ]]; then
    echo "python-consumer-smoke: expected one host wheel, found ${#wheels[@]}" >&2
    exit 1
fi
wheel_name="${wheels[0]##*/}"
if [[ "$wheel_name" != *-cp39-abi3-*.whl ]]; then
    echo "python-consumer-smoke: expected a cp39-abi3 host wheel, got $wheel_name" >&2
    exit 1
fi
uv venv --python "$python_path" "$smoke_dir/venv"
consumer_python="$smoke_dir/venv/bin/python"
uv pip install --python "$consumer_python" --no-index "${wheels[0]}"
cd "$smoke_dir"
"$consumer_python" -I - <<'PY'
from pathlib import Path
import sys

import ferric

assert sys.prefix != sys.base_prefix, "consumer must use its fresh virtual environment"
module = Path(ferric.__file__).resolve()
assert Path(sys.prefix).resolve() in module.parents, f"import escaped the consumer: {module}"
assert not hasattr(ferric, "engine_instance_count"), "wheel must not enable testing features"

source = """
(deffacts startup (ready))
(defrule complete ?f <- (ready) =>
  (retract ?f)
  (assert (packaged-result 42))
  (printout t "packaged:42" crlf))
"""


def consume(engine):
    result = engine.run()
    assert result.rules_fired == 1
    facts = engine.find_facts("packaged-result")
    assert len(facts) == 1 and facts[0].fields == [42]
    assert engine.get_output("t") == "packaged:42\n"
    assert engine.run().rules_fired == 0


with ferric.Engine.from_source(source) as engine:
    engine.reset()
    checkpoint = engine.serialize()
    consume(engine)
with ferric.Engine.from_snapshot(checkpoint) as restored:
    consume(restored)

print("Python host-wheel consumer: isolated import, rule execution, facts, output and snapshot resume passed")
PY
