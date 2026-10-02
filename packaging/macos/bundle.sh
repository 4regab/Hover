#!/bin/sh
# A local Hover.app around the native build. Run from the repo root on a Mac after
#   cargo build --release -p hover
#   sh packaging/macos/bundle.sh [version] [outdir]
# The version defaults to the workspace's (Cargo.toml), the out folder to dist.
#
# Why a bundle at all: macOS asks for the Microphone and shows Screen Recording and
# notifications only for an app with an Info.plist and an identifier. A bare binary gets
# none of that right. Not signed, not notarized: it is for the Mac it was built on. The
# binary cargo links is ad-hoc signed already; to keep the permissions you grant across
# rebuilds, sign the bundle with one identity yourself (codesign --force --sign - works).
set -eu
VERSION=${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | tr -d '\r' | head -1)}
OUT=${2:-dist}
BIN=target/release/hoverai
[ "$(uname)" = Darwin ] || { echo "this makes a macOS app: run it on a Mac" >&2; exit 1; }
[ -x "$BIN" ] || { echo "build $BIN first: cargo build --release -p hover" >&2; exit 1; }

APP="$OUT/Hover.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/hoverai"

# The file is CRLF in a Windows checkout; XML doesn't mind, but plutil's output is neater without.
tr -d '\r' < packaging/macos/Info.plist | sed "s/@VERSION@/$VERSION/g" > "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"

# The icon: every size an .icns wants, from the 1254-pixel logo.
SET=$(mktemp -d)/Hover.iconset
trap 'rm -rf "$(dirname "$SET")"' EXIT
mkdir -p "$SET"
for s in 16 32 128 256 512; do
  sips -z "$s" "$s" assets/hover.png --out "$SET/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2))
  sips -z "$d" "$d" assets/hover.png --out "$SET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$SET" -o "$APP/Contents/Resources/Hover.icns"

plutil -lint "$APP/Contents/Info.plist" >/dev/null
echo "Hover $VERSION: $APP"
