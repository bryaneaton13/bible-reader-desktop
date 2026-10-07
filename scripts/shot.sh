#!/usr/bin/env bash
# Render the reader window offscreen to a PNG, without screen-recording
# permission or touching your real settings. Good for checking UI changes.
#
#   scripts/shot.sh OUT.png [PASSAGE] [ACTIONS]
#   scripts/shot.sh /tmp/ps23.png "ps 23:2" "sepia,notes"
#
# ACTIONS (comma-separated): light, sepia, dark, system, notes, nosidebar,
# version:kjv, search:<query> (results page; ~ = space), palette:<query>, commands:<query>, goto:<passage>, back,
# forward, newtab, closetab, tab:<n>, pin (active tab), rename:<name>,
# sidebar (focus the sidebar chapter cursor), split[:<passage>] (split right,
# optionally navigating the new pane), unsplit, closepane, focus:left|right,
# narrow (one pane at a time), dropzone:left|right (tab-drag overlay),
# font:classic|editorial|modern|clean|garamond|sans (title + body pairing).
# SHOT_WINDOW=settings captures the Settings window instead of the reader.
# Set SHOT_SETTINGS to a settings.json to start from (e.g. with recents).
set -euo pipefail

out=${1:?usage: scripts/shot.sh OUT.png [PASSAGE] [ACTIONS]}
passage=${2:-}
actions=${3:-}
root=$(cd "$(dirname "$0")/.." && pwd)

cargo build --quiet --manifest-path "$root/Cargo.toml" -p app --features screenshot

home=$(mktemp -d "${TMPDIR:-/tmp}/br-shot.XXXXXX")
trap 'rm -rf "$home"' EXIT
if [[ -n ${SHOT_SETTINGS:-} ]]; then
  mkdir -p "$home/Library/Application Support/BibleReader"
  cp "$SHOT_SETTINGS" "$home/Library/Application Support/BibleReader/settings.json"
fi

mkdir -p "$(dirname "$out")"
HOME=$home \
BIBLE_READER_SCREENSHOT=$out \
BIBLE_READER_GOTO=$passage \
BIBLE_READER_ACTIONS=$actions \
BIBLE_READER_SCREENSHOT_WINDOW=${SHOT_WINDOW:-} \
  "$root/target/debug/bible-reader"
