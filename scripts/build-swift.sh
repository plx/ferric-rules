#!/usr/bin/env bash
# Build a local SwiftPM binary dependency. No signing or distribution service.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
if [[ $(uname -s) != Darwin ]]; then
    echo "build-swift: requires macOS with Xcode and its iOS SDKs" >&2
    exit 1
fi
mode="${1:-all}"
if [[ "$mode" != all && "$mode" != --macos-only ]]; then
    echo "usage: scripts/build-swift.sh [--macos-only]" >&2
    exit 1
fi
package="$root/bindings/swift"
staging="$root/target/swift-package"
if ! cmp -s "$root/examples/embedding/launch-selection.clp" "$package/Tests/FerricTests/Fixtures/launch.clp"; then
    echo "build-swift: refresh the Swift launch fixture from examples/embedding/launch-selection.clp" >&2
    exit 1
fi
mkdir -p "$staging/headers" "$package/Artifacts"
cp "$package/Support/CFerric.h" "$package/Support/module.modulemap" "$staging/headers/"
cp "$root/LICENSE-MIT" "$root/LICENSE-APACHE" "$root/THIRD_PARTY_NOTICES.md" "$package/Artifacts/"
arguments=()
for target in aarch64-apple-darwin aarch64-apple-ios aarch64-apple-ios-sim; do
    if [[ "$mode" == --macos-only && "$target" != aarch64-apple-darwin ]]; then
        continue
    fi
    case "$target" in
        aarch64-apple-darwin) sdk=macosx ;;
        aarch64-apple-ios) sdk=iphoneos ;;
        aarch64-apple-ios-sim) sdk=iphonesimulator ;;
    esac
    rustup target add "$target"
    # Retain symbols: Xcode 27 can reject stripped Rust proc-macro dylibs
    # with a misaligned Mach-O LINKEDIT string pool during cross-compilation.
    SDKROOT="$(xcrun --sdk "$sdk" --show-sdk-path)" \
        MACOSX_DEPLOYMENT_TARGET=15.0 IPHONEOS_DEPLOYMENT_TARGET=18.0 \
        CARGO_TARGET_DIR="$root/target" CARGO_PROFILE_FFI_RELEASE_STRIP=none \
        cargo build --manifest-path "$root/Cargo.toml" --locked \
        -p ferric-rules-ffi --profile ffi-release --features serde --target "$target"
    arguments+=(-library "$root/target/$target/ffi-release/libferric_rules_ffi.a" -headers "$staging/headers")
done
cp "$root/crates/ferric-rules-ffi/ferric.h" "$staging/headers/ferric.h"
# xcodebuild requires a fresh destination; replace only this generated artifact.
rm -rf "$package/Artifacts/CFerric.xcframework"
xcodebuild -create-xcframework "${arguments[@]}" -output "$package/Artifacts/CFerric.xcframework"
echo "Swift package ready: $package"
