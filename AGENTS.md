# AGENTS.md

Guide for AI agents (and humans) working in this repo. Read this before changing code.

## What this is

Offline macOS Bible reader. Rust workspace, UI in GPUI through `gpui-kit` (crates.io, pinned `=0.7.1`, which pulls `gpui-pre 0.3.8`). Ships BSB, KJV and WEB (public domain). macOS only. Product and design notes live in [`docs/`](docs/README.md). The roadmap ([`docs/05-roadmap.md`](docs/05-roadmap.md)) shows what's done and what's next.

```
crates/
  bible-core/    domain types: books, VerseId/ChapterRef, reference parser ("jn 3:16"), Block/Span content model. No UI, no DB.
  bible-store/   SQLite per translation (read-only), chapter JSON blobs, FTS5 (porter-stemmed) search. `study.rs`: topics, aliases, cross-references (study.db). `search.rs`: ranked search (bm25 + topic + phrase + popularity).
  bible-import/  USFM parser + importer CLI: data/sources/*.usfm -> data/build/*.sqlite, data/sources/{openbible,topical} -> data/build/study.db (topics merged from openbible.info + Nave's + Torrey's; `topical.rs` parses CCEL ThML)
  app/           GPUI app (binary `bible-reader`)
    main.rs         actions, key bindings, app menus, window, `screenshot` dev feature
    reader.rs       ReaderView: title-bar toolbar, tab bar, sidebar, chapter, notes panel, palette host; headless UI tests
    selectable.rs  SelectableBlock: wraps a chapter block's InteractiveText so Root's text selection (drag, ⌘C) works with styled runs
    tabs.rs         Tab (location, translation, back/forward, scroll, optional `side` pane for split view) + save/restore; unit tests
    settings_window.rs  Settings window (⌘,): font, size, line spacing, width, theme; gpui-kit `setting`
    palette.rs      ⌘K palette rows (Go to / Tabs / Recent / Try searching / Commands / Topics / Verses), each carrying its Action; unit tests
    search_page.rs  search results page (⇧⌘F; child module of reader.rs): filters, key passages for a topic, ranked matches with highlights, related verses, open here / new tab / beside
    suggestions.rs  rotating suggested searches; a test checks each returns good results
    translation_picker.rs  rows for the searchable translation Select
    chapter_view.rs Block -> styled text runs (verse numbers, red letter, notes, jump highlight); unit tests
    fonts.rs        bundled typefaces (assets/fonts/*.ttf, OFL) registered at startup for the font pairings
    library.rs      Library (open translations), Settings (persisted JSON: tabs, recents, appearance), data_dir()
    appearance.rs   applies System / Light / Sepia / Dark to gpui-kit's Theme
    tray.rs         menu bar item (gpui-tray)
scripts/
  shot.sh        offscreen screenshot of the UI -> PNG (the main visual check)
  bundle.sh      build "dist/Bible Reader.app"; --install copies to ~/Applications and opens it
  make-icon.swift  regenerates assets/AppIcon.icns
data/sources/    USFM inputs (committed)    data/build/  generated .sqlite (gitignored)
```

Rules of the road:
- Domain crates (`bible-core`, `bible-store`, `bible-import`) must not depend on GPUI.
- Keep `gpui-kit` the only GPUI dependency. Reach GPUI as `gpui_kit::*` and components as `gpui_kit::component::*`.
- Settings live in `~/Library/Application Support/BibleReader/settings.json`. Point `HOME` at a scratch dir for any automated run so you don't clobber the user's real settings and reading position.

## Use gpui-kit components first

Before building UI by hand, check whether gpui-kit already has it. Its components handle keyboard navigation, focus, theming and test ids for you. Browse the source at `~/.cargo/registry/src/*/gpui-component-0.7.1/src/` (one file or folder per component; each has doc comments and tests showing usage). Component docs are at <https://gpui-kit.com>.

| Need | Component (`gpui_kit::component::…`) | Used in |
|---|---|---|
| Toolbar in the window title bar | `TitleBar` | `reader.rs` toolbar |
| Buttons, icon buttons, selected/disabled states | `button::Button` (+ `ButtonVariants`, `Selectable`, `Disableable`) | everywhere |
| Dropdown menu on a button | `menu::DropdownMenu` → `PopupMenu` (`menu`, `menu_with_check`) | theme, text size |
| Searchable dropdown | `select::{Select, SelectState, SelectItem, SearchableVec}` | translation picker |
| Command palette (groups, ↑↓/Enter, shortcut hints) | `command::{Command, CommandState, CommandGroup, CommandItem}` | ⌘K palette |
| Tabs | `tab::{TabBar, Tab}` | tab bar |
| Keyboard key caps | `kbd::Kbd` | palette footer |
| Icons (about 100 Lucide icons) | `IconName` | toolbar, palette |

Other components that will fit upcoming work: `sidebar`, `list`/`virtual_list`, `dialog`, `sheet`, `popover`/`hover_card` (footnote popovers), `setting` (settings window), `switch`, `slider` (font size), `input`, `notification`, `tooltip`, `resizable`.

## Setup

Rust is pinned by `rust-toolchain.toml` (1.99, edition 2024). Command Line Tools are enough; full Xcode is not needed (gpui-kit compiles Metal shaders at runtime).

```sh
cargo run --release -p bible-import   # once: USFM -> data/build/{bsb,kjv,web}.sqlite (~15 s)
```

## Run

```sh
cargo run -p app                      # dev build, reads data/build/
scripts/bundle.sh                     # release .app in dist/ (binary + DBs + icon, ad-hoc signed)
scripts/bundle.sh --install           # ...and copy to ~/Applications, then launch it
open "dist/Bible Reader.app"          # launch a built bundle
```

Translations are found in this order: `$BIBLE_READER_DATA`, then `<bundle>/Contents/Resources/data`, then `data/build` in the repo.

The app keeps running in the menu bar after its window closes (`QuitMode::Explicit`). To stop it from a script, run `pkill -f bible-reader` or `osascript -e 'tell application id "com.biblereader.BibleReader" to quit'`.

## Validate changes

Work down this list. Most changes need steps 1 and 2. Any UI change also needs step 3, and you should look at the PNG.

### 1. Code checks (always)

```sh
cargo test --workspace                 # parser, store, importer, rendering + headless UI tests
cargo clippy --workspace --all-targets # keep at zero warnings
cargo fmt --all
```

### 2. Headless UI tests (behavior, no pixels)

`crates/app/src/reader.rs` → `mod tests` opens a real `ReaderView` in gpui's test platform. The tests click toolbar buttons by element id, type into the palette, press keys, and assert on view state. They need no display and no permissions, and they run in `cargo test`. They skip if `data/build` is empty.

```rust
#[gpui_kit::test]
fn my_test(cx: &mut TestAppContext) {
    let Some((window, view)) = open(cx) else { return };   // helper in reader.rs tests
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);       // lay out a frame first so ids are registered
        window.click("next", cx);      // Button ids: sidebar, back, forward, prev, passage, next, notes, ...
        window.input("jn 3:16", cx);   // types into the focused element
        window.press("enter", cx);
    }).unwrap();
    view.read_with(cx, |v, _| assert_eq!(v.chapter.to_string(), "John 3"));
}
```

Gotchas:
- In a module that does `use super::*` (which globs `gpui_kit::*`), add `use core::prelude::v1::test;`. Without it, `gpui_kit::test` shadows the built-in `#[test]` and the macro fails with "recursion limit reached".
- `window.click/find` only see elements the harness registers, such as gpui-kit `Button`s with an id. Plain `div().id(..)` rows are not registered unless you tag them with `.test_support()`. If a lookup fails, assert on view state instead.
- The test platform renders no pixels and does no real font shaping. Use step 3 to check appearance.
- See `gpui-kit-0.7.1/src/test.rs` (in `~/.cargo/registry/src/*/`) for the full API: `find`, `try_find`, `within`, `click_at`, `hover`, `scroll`, `drag`, `press`, `input`, and `TestAppContextExt::wait_for`.

### 3. Visual check: offscreen screenshot (pixels, no permissions)

```sh
scripts/shot.sh /tmp/shot.png "jn 3:16" "sepia,notes"
```

This builds with `--features screenshot` and runs the real app with a throwaway `HOME`. It applies scripted steps, renders the window offscreen via `Window::render_to_image()` (real Metal, about 2200×1640 at 2x), saves the PNG and exits. It needs no Screen Recording permission and steals no focus. Read the PNG to inspect it.

- Arg 2 is the passage to open first (`ps 23`, `1 cor 13:4`).
- Arg 3 is a comma-separated list of steps, run in order:
  - `light`, `sepia`, `dark`, `system` set the theme.
  - `font:classic|editorial|modern|clean|garamond|sans` sets the title + body font pairing.
  - `notes` opens the footnotes panel and selects the first note.
  - `nosidebar` hides the sidebar.
  - `version:kjv` switches translation.
  - `search:<query>` opens the search results page (`~` stands for a space, e.g. `search:in:nt~pride`; bare `search:` shows suggestions).
  - `palette:<query>` opens the ⌘K palette with a query. `palette:` with an empty query shows tabs, recent chapters and commands.
  - `commands:<query>` opens the palette in commands-only mode (⇧⌘P).
  - `goto:<passage>`, `back`, `forward` navigate the active tab.
  - `newtab`, `closetab`, `tab:<n>` (1-based) manage tabs.
  - `split` (or `split:<passage>` to also navigate the new right pane), `unsplit`, `closepane`, `focus:left` / `focus:right`, `narrow` (one pane at a time), `dropzone:left` / `dropzone:right` (the tab-drag overlay).
- To start from saved state, set `SHOT_SETTINGS=path/to/settings.json`.
- When you add UI state worth checking, add a step for it in `ReaderView::debug_action` (`reader.rs`, `#[cfg(feature = "screenshot")]`), then list it here and in `scripts/shot.sh`.
- Steps run before the first frame. A window that isn't key doesn't reliably repaint again before the capture.
- Not captured: the macOS traffic lights (drawn by AppKit), open dropdowns (theme, text size, the translation `Select`), the menu bar item, and hover states. Cover dropdowns with a headless test instead; see `translation_dropdown_is_searchable`.

### 4. Live app (only when you need the real thing)

```sh
scripts/bundle.sh && open "dist/Bible Reader.app"
pgrep -fl "Bible Reader.app"                       # process is up
swift -e 'import CoreGraphics; for w in CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]] where (w["kCGWindowOwnerName"] as? String) == "bible-reader" { print(w["kCGWindowNumber"]!, w["kCGWindowBounds"]!) }'
screencapture -x -o -l <window-number> /tmp/live.png   # needs Screen Recording permission for the terminal
```

Live capture and synthetic input (`osascript ... keystroke`, `cliclick`) need Screen Recording and Accessibility permissions, an unlocked screen, and the window on screen. GPUI exposes no accessibility tree, so AppleScript can't find buttons. Prefer steps 2 and 3. The live app also uses your real settings file.

## Gotchas learned the hard way

- **Search:** Misspellings are fixed before searching (`bible-store/src/fuzzy.rs`): first the curated list `misspellings.txt` (`wrong right` per line), then the nearest word in the translation's vocabulary (edit distance 1 for 4-5 letters, 2 for longer; the word still being typed is left alone). `Results.corrected` drives the "Showing results for" note. `bible_store::search` blends bm25 over a porter-stemmed FTS5 index with topic membership (`study.db`), exact phrases and verse popularity (cross-reference votes). Quotes make a phrase, `-word` excludes. Topic aliases live in `data/sources/openbible/aliases.txt`; topic names to skip live in `drop.txt` (reviewed list: people, places, typos, trivia). The importer merges openbible.info, Torrey's and Nave's by stemmed name (a verse several sources list ranks higher), drops proper-name topics via capitalization in the BSB/KJV text, and turns Nave's "See X" entries into aliases. It writes `data/build/study-topics.txt` (every kept topic: origin, passages, corroboration) for auditing. The FTS tokenizer changed, so rebuild data after pulling (`cargo run --release -p bible-import`). Eyeball ranking with `cargo run -q --release -p bible-store --example probe -- "pride"`. `study.db` has the `.db` extension on purpose: `Library::load` treats every `*.sqlite` as a translation. The search page is a mode of the window (`ReaderView.search`), not a tab; `show_active` closes it, and ⌘Enter opens tabs behind it with `open_in_background_tab`.
- **Split view:** a split is one tab with two panes. `Tab.side: Option<Box<Tab>>` is the right pane (a `Tab` reused for its location, translation, history and scroll); `focus_side` says which pane navigation applies to. `ReaderView::tab()` / `tab_mut()` return the *focused* pane, so sidebar, ⌘K, ⌘[ ⌘], ⌘1–3 and the notes panel just work on it. Chapter text lives in `ReaderView.content` keyed by pane id, loaded in `show_active` (focused pane, plus the other pane if missing). `Settings.translation` mirrors the focused pane. Clicking or selecting text in a pane focuses it (capture-phase mouse down on the pane frame). Each pane header has its own translation `Select` (`translation_select` left, `side_select` right); the toolbar one only shows when not split. Below `SPLIT_MIN_WIDTH` (760pt of content) only the focused pane shows, with a switcher row. Only two panes, and a split tab can't be dropped into another.

- **Fonts:** `Settings.font_pairing` (`library::FontPairing`) picks body, display (titles, headings) and verse-number families. Bundled fonts are static Regular/Bold/Italic TTFs, subset to Latin, and `include_bytes!`'d in `fonts.rs`. Variable fonts don't pick weights on GPUI/macOS, so re-cut new ones with fonttools `varLib.instancer`, then fix the name table so each family is a plain Regular/Bold/Italic set. Verse numbers use `sups`; Lora and Outfit lack superscript digits 0 and 5-9, hence `FontPairing::numbers()`.
- **Icons:** `gpui_kit::application()` does not register assets. `main.rs` calls `.with_assets(gpui_kit::assets::Assets)`. Without it, every `IconName` renders blank. Only about 100 default Lucide icons are embedded; see `default-icons.txt` in `gpui-kit-assets`.
- **Themes:** `Theme::change(mode)` reloads the registered light or dark theme. Sepia is Light plus a `Theme::update` overlay (`appearance.rs`). Re-apply after any `Theme::change`. `ReaderView` observes the window appearance, so `System` follows macOS.
- **Actions from popup menus** (theme and text-size dropdowns) are handled at app level (`cx.on_action` in `main.rs`) or by the focused `Reader` context. App-level handlers work whatever has focus.
- **Title bar:** the toolbar is the title bar (`TitleBar` with transparent titlebar options, traffic lights at (16, 15)). Its left padding leaves room for the traffic lights.
- **Text:** verse numbers are ASCII digits with the OpenType `sups` feature, not Unicode superscripts, because GPUI wraps lines inside "¹⁷". Body text uses explicit `TextRun`s, so it does not inherit font styles from parent divs.
- **Tabs and history:** all per-tab state lives in `tabs::Tab`. `Tab::navigate` records Back history; `ReaderView::show_active` loads and saves whatever the active tab points at. Navigation goes through `OpenPassage` / `go_to`, never by setting fields directly. Tabs are saved to `settings.json` on every change. `book` / `chapter` / `translation` there mirror the active tab for the menu bar.
- **Palette rows run Actions:** each `palette::Row` carries an Action (`OpenPassage`, `ActivateTab`, `NewTab`, …). gpui-kit's `Command` dispatches it on Enter or click, then `on_confirm` closes the palette. `filterable(false)` plus `on_query` hands matching to `palette::build`. To add a palette command, add one line to `palette::commands`. Its key binding hint shows up automatically.
- **`Select` needs a `Window` to change its selection.** `ReaderView` syncs the translation dropdown in `render` (`sync_translation_select`), so `show_active` can run without a window (menu bar, tests).
- **Headless tests and key bindings:** call `crate::bind_keys(cx)` in the test setup to make `window.press("cmd-k")` work. Run `cx.run_until_parked()` after input so deferred actions land.
- macOS has no `timeout` command. Don't wrap runs in it.

## Keyboard map (keep README in sync)

⌘K / ⌘P / ⌘L / ⌘F palette · ⇧⌘F search results page (↑↓ select, Enter open, ⌘Enter new tab behind the page, ⇧Enter beside, Esc back) ·  ⇧⌘P commands · ⌘T new tab · ⌘W close tab (last tab closes the window) · ⇧⌘T reopen closed tab · ⌃Tab / ⌃⇧Tab or ⇧⌘] / ⇧⌘[ next/previous tab · ⇧⌘W close window · ← → / ⌘[ ⌘] / ⌥← ⌥→ prev/next chapter · ⌘Enter in palette: open in new tab · ⌥Enter in palette: open beside (split) · ⌘\ split right / unsplit · ⇧⌘\ close pane · ⌘⌥← ⌘⌥→ focus left/right pane · drag a tab onto the left or right half of the text to split, drag a pane header to the tab strip to unsplit · `in:nt` / `in:john` scopes a palette search · ⌘, settings · ⌘B sidebar · ⌥⌘B focus sidebar (arrows move, Enter opens, ⌥↑↓ book, Esc back) · ⌃⌘← ⌃⌘→ back/forward · ⌘1–3 translation · ⌘+ ⌘− ⌘0 text size · ⌃⌘S sidebar · ⌥⌘N notes · drag to select text, ⌘C copy · ⇧⌘C copy verse with citation · ⌥⌘C copy chapter · ⇧⌘L dark/light · View ▸ Theme for Sepia / System.
