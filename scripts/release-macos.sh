#!/usr/bin/env bash
# Archive the macOS app and bundle it into a dmg for a GitHub release.
#
#   scripts/release-macos.sh
#
# Version comes from the VERSION environment variable, falling back to
# the workspace version in Cargo.toml. The app is ad-hoc signed:
# downloaders must approve it once in Privacy & Security, or clear
# quarantine with `xattr -d com.apple.quarantine Paloma.app`.
#
# Outputs:
#   target/macos/Paloma.xcarchive
#   target/macos/Paloma-<version>-macos-<arch>.dmg
set -euo pipefail

cd "$(dirname "$0")/.."

VERSION="${VERSION:-$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)}"
BUILD_NUMBER="$(git rev-list --count HEAD)"

case "$(uname -m)" in
    arm64) ARCH="arm64" ;;
    x86_64) ARCH="amd64" ;;
    *) echo "Unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac

OUT="target/macos"
ARCHIVE="$OUT/Paloma.xcarchive"
APP="$ARCHIVE/Products/Applications/Paloma.app"
DMG="$OUT/Paloma-$VERSION-macos-$ARCH.dmg"

rm -rf "$OUT"
mkdir -p "$OUT"

xcodebuild archive \
    -project gui/macos/Paloma/Paloma.xcodeproj \
    -scheme Paloma \
    -configuration Release \
    -archivePath "$ARCHIVE" \
    MARKETING_VERSION="$VERSION" \
    CURRENT_PROJECT_VERSION="$BUILD_NUMBER"

# Drag-to-install layout: the app next to an /Applications symlink.
STAGING="$(mktemp -d)"
ditto "$APP" "$STAGING/Paloma.app"
ln -s /Applications "$STAGING/Applications"
hdiutil create -volname Paloma -srcfolder "$STAGING" -format UDZO -quiet "$DMG"
rm -rf "$STAGING"

echo "version: $VERSION ($BUILD_NUMBER)"
echo "dmg:     $DMG"
