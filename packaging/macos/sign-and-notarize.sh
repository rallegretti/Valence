#!/usr/bin/env bash
# One command to sign, notarize and build the release DMG. Run it in
# Terminal on the Mac (not over SSH: the keychain is locked there).
#
#   packaging/macos/sign-and-notarize.sh
#
# The first time, it saves a notarization login for Valence in your keychain
# (profile "valence-notary"): it asks for your Apple ID email, then an
# app-specific password from appleid.apple.com → Sign-In and Security →
# App-Specific Passwords. After that it never asks again.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
PROFILE="${NOTARY_PROFILE:-valence-notary}"
TEAM=297P8BZD3F
if ! xcrun notarytool history --keychain-profile "$PROFILE" >/dev/null 2>&1; then
  echo "One-time setup: saving a notarization login for Valence as \"$PROFILE\"."
  read -rp "Apple ID email: " APPLE_ID
  xcrun notarytool store-credentials "$PROFILE" --apple-id "$APPLE_ID" --team-id "$TEAM"
fi
MACOS_SIGN_IDENTITY="Developer ID Application: Rob Allegretti ($TEAM)" NOTARY_PROFILE="$PROFILE" \
  packaging/macos/package.sh --skip-build
echo
echo "Done. Signed and notarized: dist/Valence-*-macos-universal.dmg"
