#!/usr/bin/env bash
# Build "Bible Reader.app" (release binary + translations + icon), ad-hoc sign it.
#
#   scripts/bundle.sh            # -> dist/Bible Reader.app
#   scripts/bundle.sh --install  # also copy to ~/Applications and launch it
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
name="Bible Reader"
bundle_id="com.biblereader.BibleReader"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)
app="$root/dist/$name.app"

cd "$root"
if ! ls data/build/*.sqlite >/dev/null 2>&1; then
  echo "==> Importing translations"
  cargo run --release --quiet -p bible-import
fi
echo "==> Building release binary"
cargo build --release --quiet -p app

echo "==> Assembling $app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources/data"
cp target/release/bible-reader "$app/Contents/MacOS/bible-reader"
cp data/build/*.sqlite data/build/study.db "$app/Contents/Resources/data/"
cp assets/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"

cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$name</string>
  <key>CFBundleDisplayName</key><string>$name</string>
  <key>CFBundleIdentifier</key><string>$bundle_id</string>
  <key>CFBundleExecutable</key><string>bible-reader</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.books</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
  <!-- "Sign in with YouVersion" redirects the browser back to youversionauth://callback -->
  <key>CFBundleURLTypes</key>
  <array>
    <dict>
      <key>CFBundleURLName</key><string>$bundle_id.youversion-sign-in</string>
      <key>CFBundleURLSchemes</key><array><string>youversionauth</string></array>
    </dict>
  </array>
</dict>
</plist>
EOF

codesign --force --deep --sign - "$app" 2>/dev/null
echo "==> Built $app ($(du -sh "$app" | cut -f1))"

if [[ ${1:-} == "--install" ]]; then
  dest="$HOME/Applications/$name.app"
  mkdir -p "$HOME/Applications"
  # Quit a running copy so the new one launches fresh.
  osascript -e "tell application id \"$bundle_id\" to quit" >/dev/null 2>&1 || true
  rm -rf "$dest"
  cp -R "$app" "$dest"
  echo "==> Installed $dest"
  open "$dest"
fi
