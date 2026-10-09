#!/bin/sh
# Linux packages of the Go build: a .deb (made with ar and tar, so no dpkg is needed to
# build it) and a plain tarball. Run from the repo root after
#   (cd go && go build -tags nowayland,nox11,novulkan -o ../hover-linux ./cmd/hover)
#   WGPU_NATIVE_LIB=path/to/libwgpu_native.so sh packaging/linux/package-linux-go.sh [version] [outdir]
# The binary carries its fonts, icon and music; libwgpu_native.so (the office's renderer) goes
# beside it in /usr/lib/hover. Wayland only: no X11 libraries.
set -eu
VERSION=${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1 | tr -d '\r')}
OUT=${2:-dist}
BIN=${HOVER_BIN:-hover-linux}
LIB=${WGPU_NATIVE_LIB:?set WGPU_NATIVE_LIB to libwgpu_native.so}
[ -x "$BIN" ] || { echo "build $BIN first" >&2; exit 1; }
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT

root="$STAGE/root"
install -Dm755 "$BIN" "$root/usr/bin/hover"
install -Dm755 "$LIB" "$root/usr/lib/hover/libwgpu_native.so"
install -Dm644 app/assets/hover-mark.png "$root/usr/share/icons/hicolor/256x256/apps/hover.png"
install -Dm644 LICENSE "$root/usr/share/doc/hover/copyright"
install -Dm644 THIRD-PARTY-NOTICES.txt "$root/usr/share/doc/hover/THIRD-PARTY-NOTICES.txt"
mkdir -p "$root/usr/share/applications"
sed 's|@EXEC@|hover|' packaging/linux/hover.desktop > "$root/usr/share/applications/hover.desktop"

deb="$STAGE/deb"
mkdir -p "$deb/control"
size=$(du -sk "$root" | cut -f1)
cat > "$deb/control/control" <<EOT
Package: hover
Version: $VERSION
Architecture: amd64
Maintainer: Hover <noreply@example.invalid>
Installed-Size: $size
Depends: libc6, libegl1, libxkbcommon0, libfontconfig1, libfreetype6, xdg-desktop-portal, pipewire-bin | pipewire-utils, wl-clipboard
Recommends: mesa-vulkan-drivers, xdg-desktop-portal-gtk | xdg-desktop-portal-kde | xdg-desktop-portal-wlr, zenity | kdialog, grim
Section: devel
Priority: optional
Description: Agent office in a notch at the top of the screen
 Hands tasks to Kiro, Codex or Cursor, which run headlessly as bots at desks. Needs a
 Wayland compositor with layer-shell (KDE, Sway, Hyprland, ...; GNOME has none).
EOT
(cd "$root" && find . -type f -exec md5sum {} + | sed 's| \./| |') > "$deb/control/md5sums"
echo 2.0 > "$deb/debian-binary"
(cd "$deb/control" && tar --owner=0 --group=0 -czf ../control.tar.gz .)
(cd "$root" && tar --owner=0 --group=0 -czf "$deb/data.tar.gz" .)
rm -f "$OUT/hover_${VERSION}_amd64.deb"
(cd "$deb" && ar rc "$OUT/hover_${VERSION}_amd64.deb" debian-binary control.tar.gz data.tar.gz)

tar --owner=0 --group=0 -C "$root" -czf "$OUT/hover-${VERSION}-linux-x86_64.tar.gz" usr
echo "$OUT/hover_${VERSION}_amd64.deb"
echo "$OUT/hover-${VERSION}-linux-x86_64.tar.gz"
