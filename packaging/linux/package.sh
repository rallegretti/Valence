#!/usr/bin/env bash
# Build Valence for Linux x86_64 and package it:
#
#   dist/valence_<version>_amd64.deb             Debian 12+, Ubuntu 22.04+, Mint, Pop!_OS…
#   dist/Valence-<version>-x86_64.AppImage       most distros with glibc 2.35+
#
# The binary is built inside an Ubuntu 22.04 container (Dockerfile here) so it
# runs on older distros too; --local builds on this machine instead (the
# packages then need this machine's glibc or newer).
#
# Usage: packaging/linux/package.sh [--local] [--skip-build]
# Needs: docker (unless --local), dpkg-deb, and appimagetool (on PATH, or set
# APPIMAGETOOL=/path/to/appimagetool-x86_64.AppImage).
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
HERE="$ROOT/packaging/linux"
ID=io.github.rallegretti.valence
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
DIST="$ROOT/dist"
LOCAL=0
SKIP_BUILD=0
for a in "$@"; do
  case "$a" in
    --local) LOCAL=1 ;;
    --skip-build) SKIP_BUILD=1 ;;
    -h | --help) sed -n '2,14p' "$0"; exit 0 ;;
    *) echo "unknown argument: $a" >&2; exit 2 ;;
  esac
done
mkdir -p "$DIST"

if [ "$LOCAL" = 1 ]; then
  TARGET_DIR="$ROOT/target"
  [ "$SKIP_BUILD" = 1 ] || (cd "$ROOT" && cargo build --locked --profile dist)
else
  TARGET_DIR="$ROOT/target/linux-ubuntu2204"
  if [ "$SKIP_BUILD" = 0 ]; then
    docker build -t valence-build:ubuntu22.04 "$HERE"
    mkdir -p "$TARGET_DIR" "$ROOT/target/docker-cargo-home"
    # Run as you, so the build output isn't owned by root.
    docker run --rm --user "$(id -u):$(id -g)" \
      -v "$ROOT:/src" -w /src \
      -e CARGO_HOME=/src/target/docker-cargo-home \
      -e CARGO_TARGET_DIR=/src/target/linux-ubuntu2204 \
      valence-build:ubuntu22.04 \
      cargo build --locked --profile dist
  fi
fi
BIN="$TARGET_DIR/dist/valence"
GLIBC="$(objdump -T "$BIN" | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1 | cut -d_ -f2)"
echo "==> Valence $VERSION, needs glibc $GLIBC"

WORK="$ROOT/target/linux-package"
STAGE="$WORK/root"
rm -rf "$WORK"
install -Dm755 "$BIN" "$STAGE/usr/bin/valence"
strip "$STAGE/usr/bin/valence"
install -Dm644 "$HERE/$ID.desktop" "$STAGE/usr/share/applications/$ID.desktop"
mkdir -p "$STAGE/usr/share/icons"
cp -R "$ROOT/assets/icon/hicolor" "$STAGE/usr/share/icons/"
install -Dm644 "$ROOT/LICENSE" "$STAGE/usr/share/doc/valence/LICENSE"
install -Dm644 "$ROOT/assets/fonts/LICENSE-DejaVu.txt" "$STAGE/usr/share/doc/valence/LICENSE-DejaVu.txt"
# Debian-style copyright summary.
cat >"$STAGE/usr/share/doc/valence/copyright" <<COPYRIGHT
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: Valence
Source: https://github.com/rallegretti/Valence

Files: *
Copyright: Rob Allegretti
License: CC0-1.0
 See /usr/share/common-licenses/CC0-1.0 (also /usr/share/doc/valence/LICENSE).

Files: fonts (DejaVu Sans, embedded in the binary)
Copyright: Bitstream, Inc.; DejaVu changes are in the public domain
License: Bitstream-Vera
 See /usr/share/doc/valence/LICENSE-DejaVu.txt.
COPYRIGHT
install -Dm644 "$ROOT/README.md" "$STAGE/usr/share/doc/valence/README.md"
command -v desktop-file-validate >/dev/null && desktop-file-validate "$STAGE/usr/share/applications/$ID.desktop"

# ---- .deb ---------------------------------------------------------------------------------------
DEB="$WORK/deb"
cp -R "$STAGE" "$DEB"
mkdir -p "$DEB/DEBIAN"
SIZE_KB="$(du -sk "$DEB/usr" | cut -f1)"
# libvulkan1 and the X11/Wayland libraries are loaded at run time (dlopen),
# so they're listed by hand; libGL/EGL is the fallback when Vulkan isn't there.
cat >"$DEB/DEBIAN/control" <<CONTROL
Package: valence
Version: $VERSION
Architecture: amd64
Maintainer: Rob Allegretti <rob.allegretti@gmail.com>
Installed-Size: $SIZE_KB
Depends: libc6 (>= $GLIBC), libgcc-s1, libwayland-client0, libxkbcommon0, libvulkan1 | libegl1
Recommends: libx11-6, libxcursor1, libxi6, libxrandr2, libxkbcommon-x11-0, mesa-vulkan-drivers
Section: education
Priority: optional
Homepage: https://github.com/rallegretti/Valence
Description: Build molecules in 3D and see how chemistry fits together
 Valence is an educational app for experimenting with atoms and molecules.
 Drag elements out of the periodic table, drop them onto each other, and
 watch simplified but realistic bonding rules decide what can join. Each
 molecule gets its common and IUPAC names, its 3D shape and its facts.
 Includes reactions, a compound library, image export and lab data files.
CONTROL
OUT_DEB="$DIST/valence_${VERSION}_amd64.deb"
dpkg-deb --root-owner-group -Zxz --build "$DEB" "$OUT_DEB" >/dev/null
echo "wrote $OUT_DEB"

# ---- AppImage -----------------------------------------------------------------------------------
APPDIR="$WORK/Valence.AppDir"
cp -R "$STAGE" "$APPDIR"
ln -s usr/bin/valence "$APPDIR/AppRun"
cp "$HERE/$ID.desktop" "$APPDIR/$ID.desktop"
cp "$ROOT/assets/icon/hicolor/256x256/apps/$ID.png" "$APPDIR/$ID.png"
ln -s "$ID.png" "$APPDIR/.DirIcon"
TOOL="${APPIMAGETOOL:-$(command -v appimagetool || true)}"
[ -n "$TOOL" ] || { echo "error: appimagetool not found (set APPIMAGETOOL)" >&2; exit 1; }
OUT_AI="$DIST/Valence-$VERSION-x86_64.AppImage"
# Extract-and-run: works without FUSE. The result embeds the static runtime,
# so users don't need libfuse2.
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" --no-appstream "$APPDIR" "$OUT_AI" >/dev/null 2>&1 \
  || ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" --no-appstream "$APPDIR" "$OUT_AI"
echo "wrote $OUT_AI"

dpkg-deb --info "$OUT_DEB" | sed -n '/Package:/,/Description:/p'
ls -lh "$OUT_DEB" "$OUT_AI"
