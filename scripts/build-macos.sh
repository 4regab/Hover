#!/bin/bash
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel)"
ARCH="${HOVER_ARCH:-arm64}"
case "$ARCH" in arm64) RID=osx-arm64; TRIPLE=aarch64-apple-darwin;; x64) RID=osx-x64; ARCH=x86_64; TRIPLE=x86_64-apple-darwin;; *) echo 'HOVER_ARCH must be arm64 or x64' >&2; exit 64;; esac
DEST="${HOVER_APP_OUTPUT:-$ROOT/dist/macos-$RID/Hover.app}"
[[ "$DEST" = /*/Hover.app ]] || { echo 'Output must be an absolute path ending in /Hover.app' >&2; exit 64; }
# The app's version is the workspace's ([workspace.package] in Cargo.toml, which may have CRLF
# line endings); the build number is major*10000 + minor*100 + patch (2.1.0 was 20100).
VERSION="$(tr -d '\r' < "$ROOT/Cargo.toml" | sed -n '/^\[workspace\.package\]/,/^\[/{s/^version *= *"\(.*\)"/\1/p;}' | head -n 1)"
[[ "$VERSION" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+) ]] || { echo "No version in Cargo.toml's [workspace.package]" >&2; exit 66; }
BUILD=$((10#${BASH_REMATCH[1]} * 10000 + 10#${BASH_REMATCH[2]} * 100 + 10#${BASH_REMATCH[3]}))
mkdir -p "$(dirname "$DEST")"
# Spotlight leaves the build folder alone, so a build is never listed as a second Hover.
touch "$(dirname "$DEST")/.metadata_never_index"
STAGE="$(mktemp -d "$(dirname "$DEST")/.hover-build.XXXXXX")"
trap 'rm -rf "$STAGE"' EXIT
APP="$STAGE/Hover.app"
RES="$APP/Contents/Resources"
mkdir -p "$APP/Contents/MacOS" "$RES/office" "$RES/backend"
npm ci --prefix "$ROOT/web/office"
node "$ROOT/web/office/build.mjs"
# The backend is Rust (crates/hover-backend), the same one Windows and Linux use. The
# toolchain in rust-toolchain.toml may lack the other architecture's target.
if command -v rustup >/dev/null && ! rustup target list --installed | grep -qx "$TRIPLE"; then rustup target add "$TRIPLE"; fi
cargo build --release -p hover-backend --target "$TRIPLE" --manifest-path "$ROOT/Cargo.toml" ${HOVER_CARGO_ARGS:-}
BACKEND="${CARGO_TARGET_DIR:-$ROOT/target}/$TRIPLE/release/hover-backend"
[[ -x "$BACKEND" ]] || { echo "cargo did not make $BACKEND" >&2; exit 66; }
cp "$BACKEND" "$RES/backend/hover-backend"
# Hover.swift holds the top-level entry point, so it compiles as main.swift.
cp "$ROOT/macos/Sources/Hover.swift" "$STAGE/main.swift"
SWIFT_SOURCES=("$STAGE/main.swift")
for SOURCE in "$ROOT"/macos/Sources/*.swift; do
  [[ "$(basename "$SOURCE")" == Hover.swift ]] || SWIFT_SOURCES+=("$SOURCE")
done
SWIFT_FLAGS=(-swift-version 5)
if [[ "${HOVER_SETTINGS_TESTS:-0}" == 1 ]]; then
  SWIFT_SOURCES+=("$ROOT/tests/macos/SettingsSmoke.swift")
  SWIFT_FLAGS+=(-D HOVER_SETTINGS_TESTS)
fi
xcrun swiftc "${SWIFT_FLAGS[@]}" -O -target "$ARCH-apple-macos14.0" -framework AppKit -framework SwiftUI -framework WebKit -framework Security -framework ServiceManagement -framework UserNotifications -framework Carbon -framework ScreenCaptureKit -framework AVFoundation -framework Speech -framework CoreMedia "${SWIFT_SOURCES[@]}" -o "$APP/Contents/MacOS/Hover"
xcrun clang -O2 -arch "$ARCH" -mmacosx-version-min=14.0 "$ROOT/macos/Sources/guardian.c" -o "$RES/hover-guardian"
cp "$ROOT/web/office/dist/kiro-office.html" "$RES/office/"
cp "$ROOT/macos/Resources/office-beats.m4a" "$RES/office/"
cp "$ROOT/LICENSE" "$ROOT/THIRD-PARTY-NOTICES.txt" "$RES/"
ICONSET="$STAGE/Hover.iconset"; mkdir -p "$ICONSET"
for SIZE in 16 32 128 256 512; do
  sips -z "$SIZE" "$SIZE" "$ROOT/assets/hover.png" --out "$ICONSET/icon_${SIZE}x${SIZE}.png" >/dev/null
  DOUBLE=$((SIZE * 2)); sips -z "$DOUBLE" "$DOUBLE" "$ROOT/assets/hover.png" --out "$ICONSET/icon_${SIZE}x${SIZE}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$RES/Hover.icns"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>Hover</string><key>CFBundleDisplayName</key><string>Hover</string>
<key>CFBundleIdentifier</key><string>dev.hover.desktop</string><key>CFBundleExecutable</key><string>Hover</string>
<key>CFBundlePackageType</key><string>APPL</string><key>CFBundleIconFile</key><string>Hover.icns</string>
<key>CFBundleShortVersionString</key><string>$VERSION</string><key>CFBundleVersion</key><string>$BUILD</string>
<key>LSMinimumSystemVersion</key><string>14.0</string><key>LSUIElement</key><true/>
<key>NSHighResolutionCapable</key><true/>
<key>NSMicrophoneUsageDescription</key><string>Hover listens while you hold Control-Option-Space, so you can say a task for an agent.</string>
<key>NSSpeechRecognitionUsageDescription</key><string>Hover turns what you say into a task's words on this Mac.</string>
<key>NSAppTransportSecurity</key><dict><key>NSAllowsLocalNetworking</key><true/></dict>
</dict></plist>
PLIST
IDENTITY="${HOVER_SIGN_IDENTITY:--}"
SIGN_ARGS=(--force --sign "$IDENTITY")
if [[ "$IDENTITY" != - ]]; then SIGN_ARGS+=(--options runtime --timestamp); fi
# An ad hoc signature's designated requirement is its own hash, so every local build
# looks like a new app to Keychain and asks for the login password to reach the
# history key. Local builds name the bundle id instead, so a rebuild is the same app.
# (Developer ID builds keep the team-based requirement codesign makes.)
APP_SIGN_ARGS=("${SIGN_ARGS[@]}")
if [[ "$IDENTITY" == - ]]; then APP_SIGN_ARGS+=(-r='designated => identifier "dev.hover.desktop"'); fi
# A missing or non-Mach-O backend must stop the build, not leave an unsigned helper.
file "$RES/backend/hover-backend" | grep -q 'Mach-O' || { echo 'The backend is not a Mach-O binary' >&2; exit 66; }
codesign "${SIGN_ARGS[@]}" --entitlements "$ROOT/macos/backend.entitlements" "$RES/backend/hover-backend"
codesign "${SIGN_ARGS[@]}" "$RES/hover-guardian"
codesign "${APP_SIGN_ARGS[@]}" --identifier dev.hover.desktop "$APP/Contents/MacOS/Hover"
# The bundle's signature is its main executable's: the hardened runtime (release
# signing) keeps the microphone shut without this entitlement.
codesign "${APP_SIGN_ARGS[@]}" --entitlements "$ROOT/macos/app.entitlements" "$APP"
codesign --verify --deep --strict "$APP"
if [[ -e "$DEST" ]]; then
  [[ -f "$DEST/Contents/Info.plist" && "$(/usr/libexec/PlistBuddy -c 'Print CFBundleIdentifier' "$DEST/Contents/Info.plist")" == dev.hover.desktop ]] || { echo 'Refusing to replace an unrelated app' >&2; exit 65; }
  rm -rf "$DEST"
fi
mv "$APP" "$DEST"
printf 'Built %s\n' "$DEST"
