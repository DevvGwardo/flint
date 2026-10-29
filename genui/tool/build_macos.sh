#!/bin/sh
# A local build is not distribution signing or notarization.
set -eu
set -o pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
FLUTTER_BIN=${FLUTTER_BIN:-"/Volumes/T7 Shield/mac-offload/toolchains/flutter-20261014/bin/flutter"}
export FLUTTER_SUPPRESS_ANALYTICS=true
export DART_SUPPRESS_ANALYTICS=true
test -x "$FLUTTER_BIN" || {
  printf '%s\n' "Set FLUTTER_BIN to an installed stable Flutter executable." >&2
  exit 1
}
cd "$ROOT"
"$FLUTTER_BIN" --suppress-analytics pub get --enforce-lockfile
"$(dirname "$FLUTTER_BIN")/dart" format --output=none --set-exit-if-changed lib test
"$FLUTTER_BIN" --suppress-analytics analyze --no-pub
"$FLUTTER_BIN" --suppress-analytics test --no-pub
"$FLUTTER_BIN" --suppress-analytics build macos --release --no-pub

APP="$ROOT/build/macos/Build/Products/Release/hooter_genui.app"
test -x "$APP/Contents/MacOS/hooter_genui"
test "$(/usr/bin/plutil -extract CFBundleExecutable raw -o - "$APP/Contents/Info.plist")" = hooter_genui
/usr/bin/codesign --verify --deep --strict "$APP"
/usr/bin/codesign --display --entitlements - --xml "$APP" | /usr/bin/python3 -c '
import plistlib, sys
actual = plistlib.loads(sys.stdin.buffer.read())
expected = {
    "com.apple.security.app-sandbox": True,
    "com.apple.security.temporary-exception.files.home-relative-path.read-write":
        ["/.flint/genui-runtime/"],
    "com.apple.security.temporary-exception.sbpl":
        ["(allow network-outbound (home-subpath \"/.flint/genui-runtime/\"))"],
}
if actual != expected:
    sys.exit("Release entitlements differ from the narrow sandbox policy.")
'
printf '\n%s\n' "Local artifact (not distribution-notarized): $APP"
