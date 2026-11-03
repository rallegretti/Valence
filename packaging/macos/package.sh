#!/usr/bin/env bash
# Build Valence.app as one universal (Apple Silicon + Intel) app, sign it,
# optionally notarize it, and zip it:
#
#   dist/Valence-<version>-macos-universal.zip
#
# Run on a Mac with Xcode (or its command line tools) and both Rust targets:
#   rustup target add aarch64-apple-darwin x86_64-apple-darwin
#
# Usage: packaging/macos/package.sh [--skip-build]
# Environment:
#   MACOS_SIGN_IDENTITY   codesign identity, e.g. "Developer ID Application: Name (TEAMID)".
#                         Default "-" (ad hoc: fine for local testing, not for release).
#   NOTARY_PROFILE        notarytool keychain profile (xcrun notarytool store-credentials).
#                         When set, the app is notarized and the ticket stapled.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
HERE="$ROOT/packaging/macos"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
IDENTITY="${MACOS_SIGN_IDENTITY:--}"
DIST="$ROOT/dist"
WORK="$ROOT/target/macos-package"
APP="$WORK/Valence.app"
export MACOSX_DEPLOYMENT_TARGET=11.0
mkdir -p "$DIST"

if [ "${1:-}" != "--skip-build" ]; then
  for t in aarch64-apple-darwin x86_64-apple-darwin; do
    (cd "$ROOT" && cargo build --locked --profile dist --target "$t")
  done
fi

rm -rf "$WORK"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
lipo -create -output "$APP/Contents/MacOS/valence" \
  "$ROOT/target/aarch64-apple-darwin/dist/valence" \
  "$ROOT/target/x86_64-apple-darwin/dist/valence"
strip -x "$APP/Contents/MacOS/valence"
lipo -info "$APP/Contents/MacOS/valence"
sed "s/@VERSION@/$VERSION/g" "$HERE/Info.plist.in" >"$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist"

# Icon: every size iconutil wants, from the 1024 px master on Apple's grid.
ICONSET="$WORK/Valence.iconset"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z "$s" "$s" "$ROOT/assets/icon/icon-mac-1024.png" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2))
  sips -z "$d" "$d" "$ROOT/assets/icon/icon-mac-1024.png" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/Valence.icns"

TS=()
[ "$IDENTITY" != "-" ] && TS=(--timestamp)
codesign --force --options runtime ${TS[@]+"${TS[@]}"} --entitlements "$HERE/entitlements.plist" \
  --sign "$IDENTITY" "$APP"
codesign --verify --strict --deep --verbose=2 "$APP"

ZIP="$DIST/Valence-$VERSION-macos-universal.zip"
if [ -n "${NOTARY_PROFILE:-}" ]; then
  ditto -c -k --keepParent "$APP" "$WORK/notarize.zip"
  xcrun notarytool submit "$WORK/notarize.zip" --keychain-profile "$NOTARY_PROFILE" --wait
  xcrun stapler staple "$APP"
  xcrun stapler validate "$APP"
  spctl --assess --type execute --verbose=2 "$APP"
else
  echo "note: NOTARY_PROFILE not set, so the app is not notarized"
fi
rm -f "$ZIP"
ditto -c -k --keepParent "$APP" "$ZIP"
echo "wrote $ZIP"
ls -lh "$ZIP"
