#!/usr/bin/env bash
# Regenerate the app icons in assets/icon/ from packaging/icon.py.
# Needs python3 with pycairo, and icotool (icoutils) for the Windows .ico.
# The macOS .icns is made from icon-mac-1024.png by packaging/macos/package.sh
# (iconutil). Outputs are committed, so packaging never needs these tools.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/assets/icon"
ID=io.github.rallegretti.valence
mkdir -p "$OUT"
for s in 16 24 32 48 64 128 256 512; do
  mkdir -p "$OUT/hicolor/${s}x${s}/apps"
  python3 "$ROOT/packaging/icon.py" "$OUT/hicolor/${s}x${s}/apps/$ID.png" "$s"
done
python3 "$ROOT/packaging/icon.py" "$OUT/icon-mac-1024.png" 1024 --mac
icotool -c -o "$OUT/valence.ico" \
  "$OUT"/hicolor/{16x16,24x24,32x32,48x48,64x64,128x128,256x256}/apps/$ID.png
echo "icons written to $OUT"
