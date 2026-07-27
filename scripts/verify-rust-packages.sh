#!/usr/bin/env bash
#
# Prove that the publishable Rust facade resolves only normalized registry
# dependencies and remains buildable/testable after the source workspace is
# gone. Internal packages are archived in publication order, extracted into a
# clean temporary source, and patched there to model crates.io containing the
# just-produced versions.

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
ferric-rules-core = { path = "packages/ferric-rules-core-$version" }
ferric-rules-parser = { path = "packages/ferric-rules-parser-$version" }
ferric-rules-runtime = { path = "packages/ferric-rules-runtime-$version" }
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
)

echo "verify-rust-packages: clean extracted facade passed offline"
