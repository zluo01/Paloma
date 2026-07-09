#!/usr/bin/env bash
# Archive the macOS app and bundle it into a dmg for a GitHub release.
#
#   scripts/release-macos.sh
#
# Version comes from the workspace version in Cargo.toml. The app is
# ad-hoc signed: downloaders must approve it once in Privacy &
# Security, or clear quarantine with
# `xattr -d com.apple.quarantine Scry.app`.
#
# Outputs:
#   target/macos/Scry.xcarchive
#   target/macos/Scry-<version>.dmg
set -euo pipefail

cd "$(dirname "$0")/.."

VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)"
BUILD_NUMBER="$(git rev-list --count HEAD)"

OUT="target/macos"
ARCHIVE="$OUT/Scry.xcarchive"
APP="$ARCHIVE/Products/Applications/Scry.app"
DMG="$OUT/Scry-$VERSION.dmg"

rm -rf "$OUT"
mkdir -p "$OUT"

xcodebuild archive \
    -project gui/macos/Scry/Scry.xcodeproj \
    -scheme Scry \
    -configuration Release \
    -archivePath "$ARCHIVE" \
    MARKETING_VERSION="$VERSION" \
    CURRENT_PROJECT_VERSION="$BUILD_NUMBER"

# Drag-to-install layout: the app next to an /Applications symlink.
STAGING="$(mktemp -d)"
ditto "$APP" "$STAGING/Scry.app"
ln -s /Applications "$STAGING/Applications"
hdiutil create -volname Scry -srcfolder "$STAGING" -format UDZO -quiet "$DMG"
rm -rf "$STAGING"

echo "version: $VERSION ($BUILD_NUMBER)"
echo "dmg:     $DMG"
