#!/usr/bin/env bash
# Full local check for macOS and Linux: lint, test, release build, and a
# launch smoke test that renders the demo scene and saves a screenshot.
# Usage: scripts/check.sh            (from anywhere in the repo)
set -euo pipefail
cd "$(dirname "$0")/.."

export RUSTFLAGS="${RUSTFLAGS:-} -D warnings"
echo "== $(uname -sm), $(rustc --version)"

echo "== format";  cargo fmt --all -- --check
echo "== clippy";  cargo clippy --workspace --all-targets
echo "== tests";   cargo test --workspace
echo "== build";   cargo build --profile dist

shot="${TMPDIR:-/tmp}/valence-smoke.png"
rm -f "$shot"
echo "== smoke test (opens a window for ~10-30 s)"
# Exit codes: 0 = drew the game, 3 = frame was blank (locked/asleep screen),
# 4 = no screenshot captured.
set +e
VALENCE_SCREENSHOT="$shot" ./target/dist/valence --demo
code=$?
set -e
case $code in
    0) echo "OK: launched, rendered the game and exited cleanly. Screenshot: $shot" ;;
    3) echo "FAIL: the game ran but the frame was blank. Unlock the screen / wake the display and retry." >&2; exit 3 ;;
    *) echo "FAIL: game exited with code $code" >&2; exit 1 ;;
esac
ft="${TMPDIR:-/tmp}/valence-feature-test"
rm -rf "$ft"; mkdir -p "$ft"
echo "== feature test (reactions, exports, settings; opens a window)"
if VALENCE_FEATURE_TEST="$ft" VALENCE_EXPORT_DIR="$ft" ./target/dist/valence && [[ $(ls "$ft"/valence-*.png 2>/dev/null | wc -l) -eq 3 ]]; then
    echo "OK: reaction, molecule and lab exports written to $ft"
else
    echo "FAIL: feature test (see $ft)" >&2
    exit 1
fi
echo "Binary: target/dist/valence"
