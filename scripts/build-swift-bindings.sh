#!/usr/bin/env bash
# Build paloma-ffi and generate the Swift bindings the macOS app compiles
# against. Runs standalone or as the app's "Build Rust Bindings" phase.
#
#   scripts/build-swift-bindings.sh [--debug]
#
# Outputs:
#   target/swift/sources/            generated Swift bindings
#   target/swift/include/            C header + module.modulemap; the app
#                                    finds these via SWIFT_INCLUDE_PATHS
#   gui/macos/Paloma/Paloma/Generated/   copy of the bindings, picked up by the
#                                    app target's synchronized folder
set -euo pipefail

cd "$(dirname "$0")/.."

TARGET=aarch64-apple-darwin
PROFILE=release
PROFILE_FLAG=--release
if [[ "${1:-}" == "--debug" ]]; then
    PROFILE=debug
    PROFILE_FLAG=
fi

LIB="target/$TARGET/$PROFILE/libpaloma_ffi.a"
OUT="target/swift"
APP_GENERATED="gui/macos/Paloma/Paloma/Generated"

cargo build -p paloma-ffi --target "$TARGET" ${PROFILE_FLAG}

rm -rf "$OUT"
mkdir -p "$OUT/sources" "$OUT/include"

bindgen() {
    cargo run --quiet -p uniffi-bindgen --bin uniffi-bindgen-swift -- "$@"
}

bindgen --swift-sources "$LIB" "$OUT/sources"
# The module name must match what the generated Swift sources import.
bindgen --headers --modulemap --module-name PalomaCoreFFI \
    --modulemap-filename module.modulemap "$LIB" "$OUT/include"

mkdir -p "$APP_GENERATED"
cp "$OUT/sources/PalomaCore.swift" "$APP_GENERATED/"

echo "bindings:  $APP_GENERATED/PalomaCore.swift"
echo "modulemap: $OUT/include"
echo "library:   $LIB"
