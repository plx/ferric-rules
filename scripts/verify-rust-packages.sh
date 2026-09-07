#!/usr/bin/env bash
#
# Prove that the publishable Rust facade resolves only normalized registry
# dependencies and remains buildable/testable after the source workspace is
# gone. Internal packages are archived in publication order, extracted into a
# clean temporary source, and patched there to model crates.io containing the
# just-produced versions. An independent application and installed CLI consume
# only these extracted archives and vendored dependencies.

set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

cargo_bin="${CARGO:-cargo}"
version="$(
    sed -n '/^\[workspace\.package\]$/,/^\[/ {
        s/^version = "\([^"]*\)"$/\1/p
    }' Cargo.toml | head -n 1
)"

if [[ -z "$version" ]]; then
    echo "verify-rust-packages: could not read workspace package version" >&2
    exit 1
fi

scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

package_target="$scratch/package-target"
packages="$scratch/packages"
vendor="$scratch/vendor"
consumer_home="$scratch/cargo-home"
expected_files="$root/scripts/expected-ferric-rules-package-files.txt"
mkdir -p "$packages" "$consumer_home"

package_and_extract() {
    local package="$1"

    echo "verify-rust-packages: packaging $package"
    "$cargo_bin" package \
        --quiet \
        -p "$package" \
        --no-verify \
        --allow-dirty \
        --locked \
        --target-dir "$package_target"

    local archive="$package_target/package/$package-$version.crate"
    local extracted="$packages/$package-$version"
    if [[ ! -f "$archive" ]]; then
        echo "verify-rust-packages: missing archive $archive" >&2
        exit 1
    fi

    tar -xzf "$archive" -C "$packages"
    if [[ ! -f "$extracted/Cargo.toml" ]]; then
        echo "verify-rust-packages: missing extracted manifest for $package" >&2
        exit 1
    fi
    if awk '
        /^\[/ {
            dependency_section = ($0 ~ /dependencies\./)
        }
        dependency_section && /^[[:space:]]*path[[:space:]]*=/ {
            found = 1
        }
        END {
            exit !found
        }
    ' "$extracted/Cargo.toml"; then
        echo "verify-rust-packages: normalized $package manifest retained a path" >&2
        exit 1
    fi
}

actual_files="$scratch/ferric-rules-package-files.txt"
"$cargo_bin" package \
    --quiet \
    -p ferric-rules \
    --list \
    --allow-dirty \
    --locked \
    > "$actual_files"
diff -u "$expected_files" "$actual_files"

# crates.io publication order: the independent leaves first, then the runtime,
# then the public facade.
package_and_extract ferric-rules-parser
package_and_extract ferric-rules-core
package_and_extract ferric-rules-runtime
package_and_extract ferric-rules
package_and_extract ferric-rules-cli

if grep -R -F "$root" "$packages" >/dev/null; then
    echo "verify-rust-packages: archive contains an absolute workspace path" >&2
    exit 1
fi

# Vendor only declared registry dependencies. The final Cargo invocation uses
# an empty CARGO_HOME plus offline mode, so it cannot consult the developer's
# package cache or the network.
"$cargo_bin" vendor \
    --quiet \
    --locked \
    --versioned-dirs \
    "$vendor" \
    > "$scratch/vendor-config.txt"

cat > "$scratch/Cargo.toml" <<EOF
[workspace]
members = ["packages/ferric-rules-$version"]
resolver = "2"

[patch.crates-io]
ferric-rules = { path = "packages/ferric-rules-$version" }
ferric-rules-core = { path = "packages/ferric-rules-core-$version" }
ferric-rules-parser = { path = "packages/ferric-rules-parser-$version" }
ferric-rules-runtime = { path = "packages/ferric-rules-runtime-$version" }
EOF

mkdir -p "$scratch/consumer/src"
cp "$root/examples/embedding/launch-selection.clp" "$scratch/consumer/launch.clp"
cat > "$scratch/consumer/Cargo.toml" <<EOF
[package]
name = "external-ferric-consumer"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
ferric-rules = { version = "=$version", features = ["serde"] }
EOF
cat > "$scratch/consumer/src/main.rs" <<'EOF'
use ferric_rules::runtime::{Engine, RunLimit, SerializationFormat};

const EXPECTED: &str = "action session-42 sign-in\n";

fn selected(engine: &Engine) {
    assert_eq!(engine.find_facts("action").unwrap().len(), 1);
    assert_eq!(engine.get_output("t"), Some(EXPECTED));
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = std::fs::read_to_string("launch.clp")?;
    let mut direct = Engine::with_rules(&source)?;
    assert!(direct.find_facts("action")?.is_empty());
    assert_eq!(direct.run(RunLimit::Count(0))?.rules_fired, 0);
    let pending = direct.serialize(SerializationFormat::Cbor)?;
    std::fs::write("rust-pending.cbor", &pending)?;
    assert_eq!(direct.run(RunLimit::Unlimited)?.rules_fired, 1);
    selected(&direct);

    let mut resumed = Engine::deserialize(&pending, SerializationFormat::Cbor)?;
    assert!(resumed.find_facts("action")?.is_empty());
    assert_eq!(resumed.run(RunLimit::Count(1))?.rules_fired, 1);
    selected(&resumed);
    let completed = resumed.serialize(SerializationFormat::Cbor)?;
    let mut completed = Engine::deserialize(&completed, SerializationFormat::Cbor)?;
    assert_eq!(completed.run(RunLimit::Unlimited)?.rules_fired, 0);
    selected(&completed);
    assert!(Engine::with_rules("(defrule broken").is_err());
    println!("external Rust launch and CBOR pending/completed resume passed");
    Ok(())
}
EOF

mkdir -p "$scratch/.cargo"
cat > "$scratch/.cargo/config.toml" <<EOF
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "$vendor"

[net]
offline = true
EOF

(
    cd "$scratch"
    export CARGO_HOME="$consumer_home"
    export CARGO_NET_OFFLINE=true
    export CARGO_TARGET_DIR="$scratch/consumer-target"

    "$cargo_bin" generate-lockfile --quiet --offline
    "$cargo_bin" test \
        --quiet \
        -p ferric-rules \
        --all-features \
        --locked \
        --offline

    # Preserve the original facade-only package contract tests above. Expand
    # the external workspace afterward to add real downstream consumers.
    python3 - "$scratch/Cargo.toml" "$version" <<'PY'
from pathlib import Path
import sys
manifest = Path(sys.argv[1])
version = sys.argv[2]
old = f'members = ["packages/ferric-rules-{version}"]'
new = f'members = ["packages/ferric-rules-{version}", "packages/ferric-rules-cli-{version}", "consumer"]'
text = manifest.read_text()
if text.count(old) != 1:
    raise RuntimeError("unexpected extracted facade workspace manifest")
manifest.write_text(text.replace(old, new))
PY
    "$cargo_bin" generate-lockfile --quiet --offline
    (
        cd "$scratch/consumer"
        "$cargo_bin" run --quiet -p external-ferric-consumer --locked --offline
    )
    "$cargo_bin" install \
        --path "$packages/ferric-rules-cli-$version" \
        --root "$scratch/install" \
        --all-features \
        --locked \
        --offline
)

# Reuse the existing CLI contract checks, including Unicode/CRLF paths and
# structured invalid-input diagnostics. Add observable launch snapshot resume.
python3 - "$root/scripts/test-rust-native-artifact.py" "$scratch" <<'PY'
import importlib.util
import os
from pathlib import Path
import subprocess
import sys

spec = importlib.util.spec_from_file_location("ferric_native_smoke", sys.argv[1])
harness = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = harness
spec.loader.exec_module(harness)
scratch = Path(sys.argv[2])
binary = scratch / "install/bin/ferric"
harness.run_cli_smokes(binary=binary, scratch_root=scratch / "cli-smokes")
consumer = scratch / "consumer"
expected = "action session-42 sign-in\n"

def cli(*args, input_text=None):
    result = subprocess.run([str(binary), *args], cwd=consumer, input=input_text,
                            text=True, encoding="utf-8", capture_output=True,
                            env={key: value for key, value in os.environ.items() if key != "HOME"})
    if result.returncode != 0:
        raise RuntimeError(f"packaged CLI failed {args}: {result.stdout}{result.stderr}")
    return result

if cli("run", "launch.clp").stdout != expected:
    raise RuntimeError("packaged CLI produced the wrong launch action")
cli("snapshot", "launch.clp", "--output", "cli-pending.cbor", "--format", "cbor")
for snapshot in ["cli-pending.cbor", "rust-pending.cbor"]:
    resumed = cli("repl", "--snapshot", snapshot, "--snapshot-format", "cbor",
                  input_text="(run 1)\n(run)\n(facts)\n")
    # The REPL emits its prompt even when reading piped commands.
    output = resumed.stdout.replace("CLIPS> ", "")
    if (output.splitlines().count(expected.rstrip()) != 1
            or output.count("(action session-42 sign-in)") != 1
            or "incorrect" in output):
        raise RuntimeError(f"wrong launch snapshot result: {resumed.stdout}")
    if resumed.stderr.strip():
        raise RuntimeError(f"snapshot resume emitted diagnostics: {resumed.stderr}")
print("external packaged CLI diagnostics and launch snapshot resume passed")
PY

echo "verify-rust-packages: extracted Rust application and CLI passed offline"
