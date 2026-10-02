#!/bin/bash
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel)"
: "${HOVER_SIGN_IDENTITY:?Set a Developer ID Application signing identity}"
: "${HOVER_NOTARY_PROFILE:?Set the name of a notarytool keychain credential profile}"
[[ "$HOVER_SIGN_IDENTITY" == 'Developer ID Application:'* ]] || { echo 'A Developer ID Application certificate is required' >&2; exit 64; }
"$ROOT/scripts/build-macos.sh"
RID="osx-${HOVER_ARCH:-arm64}"
APP="${HOVER_APP_OUTPUT:-$ROOT/dist/macos-$RID/Hover.app}"
ZIP="$(dirname "$APP")/Hover-$RID.zip"
ditto -c -k --keepParent "$APP" "$ZIP"
xcrun notarytool submit "$ZIP" --keychain-profile "$HOVER_NOTARY_PROFILE" --wait
xcrun stapler staple "$APP"
xcrun stapler validate "$APP"
spctl --assess --type execute --verbose=2 "$APP"
ditto -c -k --keepParent "$APP" "$ZIP"
printf 'Notarized release: %s\n' "$ZIP"
