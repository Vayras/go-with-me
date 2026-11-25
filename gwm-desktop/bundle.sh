#!/bin/bash
# Build gwm-desktop.app (release). With --install, copy it to ~/Applications.
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release
APP=target/gwm-desktop.app
rm -rf "$APP"; mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/gwm-desktop "$APP/Contents/MacOS/gwm-desktop"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>gwm-desktop</string>
  <key>CFBundleDisplayName</key><string>gwm-desktop</string>
  <key>CFBundleIdentifier</key><string>local.go-with-me.gwm-desktop</string>
  <key>CFBundleExecutable</key><string>gwm-desktop</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
codesign --force --sign - "$APP" 2>/dev/null || true   # ad-hoc signature so macOS will open it locally
echo "Built $PWD/$APP"
if [ "${1:-}" = --install ]; then
  mkdir -p "$HOME/Applications"; rm -rf "$HOME/Applications/gwm-desktop.app"; cp -R "$APP" "$HOME/Applications/"
  echo "Installed to ~/Applications/gwm-desktop.app — open it from Launchpad or Spotlight"
fi
