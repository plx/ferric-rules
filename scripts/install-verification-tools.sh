#!/usr/bin/env bash
# Install the pinned formal-verification toolchain used by `just verify`.
#
#   - Kani (bounded model checker for the production Rust code)
#   - Verus (SMT-based deductive verifier for the algorithm models under
#     verification/verus/)
#
# Both tools pin their own Rust toolchains, independent of rust-toolchain.toml.
# Set VERUS_HOME to choose where Verus is unpacked (default: ~/.verus).
set -euo pipefail

KANI_VERSION="0.68.0"
VERUS_RELEASE="0.2026.11.02.8bf68ee"
VERUS_RUST_TOOLCHAIN="1.99.1"
VERUS_HOME="${VERUS_HOME:-$HOME/.verus}"

case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) verus_platform="x86-linux" ;;
    Linux-aarch64) verus_platform="arm64-linux" ;;
    Darwin-x86_64) verus_platform="x86-macos" ;;
    Darwin-arm64) verus_platform="arm64-macos" ;;
    *)
        echo "unsupported platform: $(uname -s)-$(uname -m)" >&2
        exit 1
        ;;
esac

if ! command -v cargo-kani >/dev/null || ! cargo kani --version 2>/dev/null | grep -q "$KANI_VERSION"; then
    cargo install --locked "kani-verifier@$KANI_VERSION"
fi
cargo kani setup

if [[ ! -x "$VERUS_HOME/verus-$verus_platform/verus" ]] ||
    ! grep -q "$VERUS_RELEASE" "$VERUS_HOME/verus-$verus_platform/version.json"; then
    mkdir -p "$VERUS_HOME"
    archive="$(mktemp -d)/verus.zip"
    curl -fsSL -o "$archive" \
        "https://github.com/verus-lang/verus/releases/download/release/$VERUS_RELEASE/verus-$VERUS_RELEASE-$verus_platform.zip"
    rm -rf "${VERUS_HOME:?}/verus-$verus_platform"
    unzip -q -o "$archive" -d "$VERUS_HOME"
fi
rustup toolchain install "$VERUS_RUST_TOOLCHAIN" --profile minimal

echo "Kani:  $(cargo kani --version)"
echo "Verus: $VERUS_HOME/verus-$verus_platform/verus ($VERUS_RELEASE)"
echo "Add $VERUS_HOME/verus-$verus_platform to PATH, or set VERUS=<path to verus>."
