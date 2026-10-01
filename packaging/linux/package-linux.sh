#!/bin/sh
# Linux packages of the native build: a .deb (made with ar and tar, so no dpkg is
# needed to build it) and a plain tarball. Run from the repo root after
#   cargo build --release -p hover
#   sh packaging/linux/package-linux.sh [version] [outdir]
# The version defaults to the workspace's (Cargo.toml).
# The binary carries its fonts, icon and music; it needs only the system libraries
# listed in Depends.
set -eu
VERSION=${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}
OUT=${2:-dist}
# Installed as hover (see the Makefile).
BIN=target/release/hoverai
[ -x "$BIN" ] || { echo "build $BIN first" >&2; exit 1; }
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT

root="$STAGE/root"
install -Dm755 "$BIN" "$root/usr/bin/hover"
install -Dm644 app/assets/hover-mark.png "$root/usr/share/icons/hicolor/256x256/apps/hover.png"
install -Dm644 LICENSE "$root/usr/share/doc/hover/copyright"
install -Dm644 THIRD-PARTY-NOTICES.txt "$root/usr/share/doc/hover/THIRD-PARTY-NOTICES.txt"
mkdir -p "$root/usr/share/applications"
# One .desktop for the package and make install; StartupWMClass is winit's X11 class,
# the binary's name.
sed 's|@EXEC@|hover|' packaging/linux/hover.desktop > "$root/usr/share/applications/hover.desktop"

# .deb: debian-binary, control.tar.gz, data.tar.gz, in that order (dpkg reads them so).
deb="$STAGE/deb"
mkdir -p "$deb/control"
size=$(du -sk "$root" | cut -f1)
cat > "$deb/control/control" <<EOF
Package: hover
Version: $VERSION
Architecture: amd64
Maintainer: Hover <noreply@example.invalid>
Installed-Size: $size
Depends: libc6, libasound2 | libasound2t64, libfontconfig1, libfreetype6, libxkbcommon-x11-0, libgl1 | libvulkan1
Recommends: mesa-vulkan-drivers, zenity | kdialog
Section: devel
Priority: optional
Description: Agent office in a notch at the top of the screen
 Hands tasks to Kiro, Codex or Cursor, which run headlessly as bots at desks.
EOF
(cd "$root" && find . -type f ! -path ./DEBIAN/\* -exec md5sum {} + | sed 's| \./| |') > "$deb/control/md5sums"
echo 2.0 > "$deb/debian-binary"
(cd "$deb/control" && tar --owner=0 --group=0 -czf ../control.tar.gz .)
(cd "$root" && tar --owner=0 --group=0 -czf "$deb/data.tar.gz" .)
rm -f "$OUT/hover_${VERSION}_amd64.deb"
(cd "$deb" && ar rc "$OUT/hover_${VERSION}_amd64.deb" debian-binary control.tar.gz data.tar.gz)

tar --owner=0 --group=0 -C "$root" -czf "$OUT/hover-${VERSION}-linux-x86_64.tar.gz" usr
echo "$OUT/hover_${VERSION}_amd64.deb"
echo "$OUT/hover-${VERSION}-linux-x86_64.tar.gz"
