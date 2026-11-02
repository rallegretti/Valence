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
VALENCE_SCREENSHOT="$shot" ./target/dist/valence --demo
if [[ -s "$shot" ]]; then
    echo "OK: launched, rendered and exited cleanly. Screenshot: $shot"
else
    echo "FAIL: no screenshot produced" >&2
    exit 1
fi
echo "Binary: target/dist/valence"
