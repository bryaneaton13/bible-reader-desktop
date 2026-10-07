# Bible Reader

Offline macOS Bible reader built with Rust, [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) and [gpui-kit](https://gpui-kit.com). Ships BSB, KJV and WEB (all public domain). Planning notes live in [`docs/`](docs/README.md). Working on the code (human or agent)? Start with [`AGENTS.md`](AGENTS.md).

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

Build a real Mac app (release binary, translations and icon in one ad-hoc-signed bundle):

```sh
scripts/bundle.sh             # -> dist/Bible Reader.app
scripts/bundle.sh --install   # also copy to ~/Applications and launch it
```

The app keeps running in the menu bar after you close the window. Quit from the menu bar item or with ⌘Q.

| Keys | Action |
|---|---|
| ⌘K / ⌘P (also ⌘L / ⌘F) | Palette: go to a passage (`jn 3:16`, `1 cor 13`, `ps 23`), search words, switch tabs, reopen recent chapters, run commands |
| ⇧⌘P | Palette with commands only (or type `>`) |
| ⇧⌘F | Search results page: topics (try `pride`), match mode, testament / book / version filters, related verses, open in a new tab or beside |
| ⌘T / ⌘W / ⇧⌘T | New tab / close tab / reopen closed tab |
| ⌃Tab / ⌃⇧Tab (or ⇧⌘] / ⇧⌘[) | Next / previous tab |
| ← / → (or ⌘[ / ⌘], ⌥← / ⌥→) | Previous / next chapter |
| ⌘B (or ⌃⌘S) | Toggle sidebar |
| ⌥⌘B | Focus sidebar: arrows move, Enter opens, ⌥↑ / ⌥↓ change book, Esc returns |
| ⌘Enter (in palette) | Open result in a new tab |
| ⌘, | Settings |
| ⌃⌘← / ⌃⌘→ | Back / forward through history |
| ⌘1 … ⌘3 | Switch translation (or use the searchable dropdown in the toolbar) |
| ⌘+ / ⌘− / ⌘0 | Text size |
| ⌃⌘S | Toggle sidebar |
| ⌥⌘N | Toggle footnotes panel |
| ⇧⌘L | Toggle dark mode (View ▸ Theme or the toolbar for Sepia / Match System) |

Each tab has its own passage, translation and history. Tabs are restored on launch, so they also work as saved places to come back to.

Settings and the last reading position are saved in `~/Library/Application Support/BibleReader/settings.json`.

## Test

```sh
cargo test --workspace                       # unit tests + headless UI tests (clicks, palette, history)
cargo clippy --workspace --all-targets
```

Visual check without screen-recording permission: render the window offscreen to a PNG (uses a throwaway `HOME`, so your settings are untouched).

```sh
scripts/shot.sh /tmp/john3.png "jn 3:16" "sepia,notes"
```

Steps (comma-separated): `light`, `sepia`, `dark`, `system`, `notes`, `nosidebar`, `version:kjv`, `palette:<query>`, `commands:<query>`, `goto:<passage>`, `back`, `forward`, `newtab`, `closetab`, `tab:<n>`. See [`AGENTS.md`](AGENTS.md#validate-changes) for the full validation workflow.

## Layout

```
crates/
  bible-core/    domain types: books, VerseId, reference parser, chapter content model (no UI, no DB)
  bible-store/   SQLite: one read-only DB per translation (FTS5 search), plus study.db (topics, cross-references)
  bible-import/  USFM parser + importer CLI
  app/           GPUI app: reader window, tabs, ⌘K palette, notes panel, menu bar item (gpui-tray)
data/
  sources/       USFM inputs
  openbible/     topics + cross-references (CC-BY, openbible.info), aliases.txt, drop.txt
  topical/       Nave's + Torrey's topical Bibles (public domain, CCEL)
  build/         generated .sqlite / study.db (gitignored)
scripts/         shot.sh (offscreen screenshot), bundle.sh (.app), make-icon.swift
assets/          AppIcon.icns
docs/            research and plans
```
