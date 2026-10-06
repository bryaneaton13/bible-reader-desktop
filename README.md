# Bible Reader

Offline macOS Bible reader built with Rust, [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) and [gpui-kit](https://gpui-kit.com). Ships BSB, KJV and WEB (all public domain). Planning notes live in [`docs/`](docs/README.md).

## Setup

```sh
# Rust (rust-toolchain.toml pins the version)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
. "$HOME/.cargo/env"          # add this line to ~/.zshrc if rustup didn't

# Xcode Command Line Tools (full Xcode not needed: gpui-kit compiles Metal shaders at runtime)
xcode-select --install
```

Bible sources (USFM) are in `data/sources/`. To refresh them:

```sh
curl -L -o /tmp/bsb.zip https://github.com/BSB-publishing/bsb2usfm/releases/download/v5.16/BSB_usfm.zip
curl -L -o /tmp/kjv.zip https://ebible.org/Scriptures/eng-kjv2006_usfm.zip
curl -L -o /tmp/web.zip https://ebible.org/Scriptures/eng-web_usfm.zip
unzip -o /tmp/bsb.zip -d data/sources/bsb; unzip -o /tmp/kjv.zip -d data/sources/kjv; unzip -o /tmp/web.zip -d data/sources/web
```

## Run

```sh
cargo run --release -p bible-import   # USFM -> data/build/{bsb,kjv,web}.sqlite (about 15 s)
cargo run -p app                      # launch the reader (first build takes a few minutes)
```

The app keeps running in the menu bar after you close the window. Quit from the menu bar item or with ⌘Q.

| Keys | Action |
|---|---|
| ⌘K / ⌘L / ⌘F | Go to a passage (`jn 3:16`, `1 cor 13`, `ps 23`) or search words |
| ⌘[ / ⌘] (or ⌥← / ⌥→) | Previous / next chapter |
| ⌘1 … ⌘3 | Switch translation |
| ⌘+ / ⌘− / ⌘0 | Text size |
| ⌃⌘S | Toggle sidebar |
| ⌥⌘N | Toggle footnotes panel |
| ⇧⌘L | Toggle dark mode |

Settings and the last reading position are saved in `~/Library/Application Support/BibleReader/settings.json`.

## Test

```sh
cargo test --workspace                       # parser, store, importer and rendering unit tests
cargo clippy --workspace --all-targets
```

Visual check without screen-recording permission: the `screenshot` feature renders the window offscreen to a PNG and exits.

```sh
cargo build -p app --features screenshot
HOME=/tmp/br-home \
BIBLE_READER_SCREENSHOT=/tmp/john3.png \
BIBLE_READER_GOTO="jn 3:16" \
BIBLE_READER_ACTIONS="light,notes" \
  ./target/debug/bible-reader
```

Actions (comma-separated): `light`, `dark`, `notes`, `nosidebar`, `version:kjv`, `palette:<query>`. Setting `HOME` to a scratch directory keeps the run from changing your real settings.

## Layout

```
crates/
  bible-core/    domain types: books, VerseId, reference parser, chapter content model (no UI, no DB)
  bible-store/   SQLite: one read-only DB per translation, FTS5 search
  bible-import/  USFM parser + importer CLI
  app/           GPUI app: reader window, palette, notes panel, menu bar item (gpui-tray)
data/
  sources/       USFM inputs
  build/         generated .sqlite (gitignored)
docs/            research and plans
```
