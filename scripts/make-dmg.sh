#!/usr/bin/env bash
# Package telemaco-gui as a macOS .app inside a .dmg. The bundle is unsigned:
# on first launch right-click > Open, or sign with a Developer ID
# (see crates/telemaco-gui/README.md).
set -euo pipefail

cd "$(dirname "$0")/.."
OUT="${1:-target/dmg}"
APP_NAME="Telemaco"

echo "==> building telemaco-gui (release)"
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo build --release -p telemaco-gui

STAGE="$OUT/$APP_NAME.app/Contents/MacOS"
rm -rf "$OUT"
mkdir -p "$STAGE"
cp target/release/telemaco-gui "$STAGE/telemaco-gui"

cat > "$OUT/$APP_NAME.app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>telemaco-gui</string>
    <key>CFBundleIdentifier</key><string>io.github.AlbertoBarrago.telemaco</string>
    <key>CFBundleName</key><string>Telemaco</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleVersion</key><string>0.1.0</string>
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>LSMinimumSystemVersion</key><string>10.15</string>
</dict>
</plist>
PLIST

echo "==> creating $OUT/$APP_NAME.dmg"
hdiutil create -volname "$APP_NAME" -srcfolder "$OUT/$APP_NAME.app" -ov -format UDZO "$OUT/$APP_NAME.dmg" >/dev/null
echo "==> done: $OUT/$APP_NAME.dmg"