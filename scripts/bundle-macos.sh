#!/usr/bin/env bash
# Wraps the kiln binary in a minimal Kiln.app so macOS treats it as an app (Dock, focus,
# screenshots, LaunchServices). Phase 3 replaces this with cargo-packager and signing.
# Usage: scripts/bundle-macos.sh [debug|release]   (default: release)
set -euo pipefail
profile="${1:-release}"
root="$(cd "$(dirname "$0")/.." && pwd)"
if [[ "$profile" == "release" ]]; then cargo build --release -p kiln; else cargo build -p kiln; fi
app="$root/target/$profile/Kiln.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
cp "$root/target/$profile/kiln" "$app/Contents/MacOS/kiln"
version="$(grep -m1 '^version' "$root/crates/kiln/Cargo.toml" | cut -d'"' -f2)"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Kiln</string>
  <key>CFBundleDisplayName</key><string>Kiln</string>
  <key>CFBundleIdentifier</key><string>dev.kiln.Kiln</string>
  <key>CFBundleExecutable</key><string>kiln</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
codesign --force --sign - "$app" >/dev/null 2>&1 || true
echo "$app"
