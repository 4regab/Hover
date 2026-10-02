#!/bin/bash
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel)"
ARCH="${HOVER_ARCH:-arm64}"
case "$ARCH" in arm64) RID=osx-arm64;; x64) RID=osx-x64; ARCH=x86_64;; *) echo 'HOVER_ARCH must be arm64 or x64' >&2; exit 64;; esac
DEST="${HOVER_APP_OUTPUT:-$ROOT/dist/macos-$RID/Hover.app}"
[[ "$DEST" = /*/Hover.app ]] || { echo 'Output must be an absolute path ending in /Hover.app' >&2; exit 64; }
DOTNET="${HOVER_DOTNET:-${DOTNET_ROOT:-$HOME/.dotnet}/dotnet}"
[[ -x "$DOTNET" ]] || DOTNET="$(command -v dotnet)"
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
"$DOTNET" publish "$ROOT/src/Hover.Backend/Hover.Backend.csproj" -c Release -r "$RID" --self-contained true -p:DebugType=None -p:DebugSymbols=false "-p:PathMap=$ROOT=/_/" -o "$RES/backend" --nologo ${HOVER_DOTNET_ARGS:-}
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
cp "$ROOT/src/Hover/Assets/kiro-office.html" "$RES/office/"
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
<key>CFBundleShortVersionString</key><string>2.1.0</string><key>CFBundleVersion</key><string>20100</string>
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
while IFS= read -r -d '' FILE; do
  # grep, not rg: rg is not on a stock Mac, and a missing tool silently skipped signing.
  if file "$FILE" | grep -q 'Mach-O'; then
    if [[ "$FILE" == */Hover.Backend ]]; then codesign "${SIGN_ARGS[@]}" --entitlements "$ROOT/macos/backend.entitlements" "$FILE"
    else codesign "${SIGN_ARGS[@]}" "$FILE"; fi
  fi
done < <(find "$RES/backend" -type f -print0)
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
