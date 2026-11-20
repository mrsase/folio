#!/usr/bin/env bash
# Build an unsigned Folio.app and a zip for the release page.
# Usage: ./scripts/package.sh [version]
set -euo pipefail

version="${1:-0.1.0}"
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

cargo build --release --locked

app="target/Folio.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/folio-app "$app/Contents/MacOS/Folio"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Folio</string>
  <key>CFBundleDisplayName</key><string>Folio</string>
  <key>CFBundleIdentifier</key><string>com.mrsase.folio</string>
  <key>CFBundleVersion</key><string>${version}</string>
  <key>CFBundleShortVersionString</key><string>${version}</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleExecutable</key><string>Folio</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
</dict>
</plist>
PLIST

# Ad-hoc signature only. This is not Developer ID signing or notarization.
codesign --force --deep --sign - "$app" >/dev/null 2>&1 || true

zip="target/Folio-${version}-macos.zip"
rm -f "$zip"
ditto -c -k --keepParent "$app" "$zip"
echo "wrote $zip"
