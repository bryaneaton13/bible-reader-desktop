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
cp LICENSE "$app/Contents/Resources/LICENSE.txt"
cp -R assets/fonts/licenses "$app/Contents/Resources/font-licenses"
cp data/sources/openbible/README.md "$app/Contents/Resources/openbible-attribution.md"
cp data/sources/topical/README.md "$app/Contents/Resources/topical-attribution.md"
cat > "$app/Contents/Resources/Credits.txt" <<'EOF'
Bible Reader — https://github.com/bryaneaton13/bible-reader-desktop
Code: MIT (see LICENSE.txt).

Bundled Bible texts: Berean Standard Bible (https://berean.bible),
King James Version and World English Bible (https://ebible.org).
Public-domain source texts; no YouVersion text is bundled.

Topics and cross-references: openbible.info, CC BY 4.0.
https://www.openbible.info/topics
https://www.openbible.info/labs/cross-references/
License: https://creativecommons.org/licenses/by/4.0/
Modified: filtered and merged into the app's study index.
Nave's Topical Bible and Torrey's New Topical Textbook: public domain,
via https://ccel.org (see the attribution files).

Bundled fonts: SIL Open Font License (see font-licenses).
Additional credits appear in Settings > About and in the source repository.
EOF

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
