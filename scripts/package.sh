#!/usr/bin/env bash
# Build an ad-hoc-signed Mac app and package it for GitHub Releases.
# Uses the host architecture; no Apple Developer account is required.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

# Cargo target overrides could make a host-labeled archive misleading (and
# bundle.sh expects the binary in target/release).
if [[ -n ${CARGO_BUILD_TARGET:-} || -n ${CARGO_TARGET_DIR:-} ]]; then
  echo "Unset CARGO_BUILD_TARGET and CARGO_TARGET_DIR before packaging." >&2
  exit 1
fi

# Never embed the publisher's optional YouVersion app key in a public build.
env -u YVP_APP_KEY "$root/scripts/bundle.sh"
app="$root/dist/Bible Reader.app"
binary="$app/Contents/MacOS/bible-reader"
arch=$(lipo -archs "$binary")
case "$arch" in
  arm64|x86_64) ;;
  *) echo "Unexpected binary architecture: $arch" >&2; exit 1 ;;
esac

codesign --verify --deep --strict "$app"
archive="Bible-Reader-$version-macOS-$arch.zip"
ditto -c -k --sequesterRsrc --keepParent "$app" "$root/dist/$archive"
cd "$root/dist"
shasum -a 256 "$archive" > "$archive.sha256"
echo "==> Packaged $root/dist/$archive"
cat "$archive.sha256"
